//! CLIP: pictures and sentences in the same vector space, which is what makes
//! "search by description" work. Runs ONNX models through ONNX Runtime.
//!
//! The model is `Xenova/clip-vit-base-patch32` (8-bit quantized), downloaded
//! once into the app data folder. The tokenizer is CLIP's byte-level BPE, read
//! from the model's `tokenizer.json`; the image preprocessing is CLIP's
//! (shortest side to 224, centre crop, normalize).

use image::DynamicImage;
use ort::environment::Environment;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::{Tensor, ValueType};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use unicode_normalization::UnicodeNormalization;

/// Identity stored next to every vector. A different model gets a different
/// key, and its vectors are never mixed with these.
pub const MODEL_KEY: &str = "clip-vit-base-patch32-q8";
pub const REPO: &str = "Xenova/clip-vit-base-patch32";
pub const REVISION: &str = "d15189d7028b43f1d3e65039190477f6af591c2a";
/// (file in the repository, local name, approximate size for progress).
pub const FILES: &[(&str, &str, u64)] = &[
    ("tokenizer.json", "tokenizer.json", 2_200_000),
    ("onnx/text_model_quantized.onnx", "text_model.onnx", 64_000_000),
    ("onnx/vision_model_quantized.onnx", "vision_model.onnx", 88_000_000),
];
pub const IMAGE_SIZE: u32 = 224;
const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_1];
const CONTEXT_LENGTH: usize = 77;

pub fn model_ready(dir: &Path) -> bool {
    FILES.iter().all(|(_, local, _)| dir.join(local).is_file())
}

// ── tokenizer ─────────────────────────────────────────────────────────────

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    ranks: HashMap<(String, String), u32>,
    byte_chars: [char; 256],
    pattern: Regex,
    start: u32,
    end: u32,
    cache: Mutex<HashMap<String, Vec<u32>>>,
}

/// GPT-2's reversible byte → printable character table.
fn byte_chars() -> [char; 256] {
    let mut printable: Vec<u32> = (b'!' as u32..=b'~' as u32).chain(0xA1..=0xAC).chain(0xAE..=0xFF).collect();
    let mut chars: Vec<u32> = printable.clone();
    let mut extra = 0;
    for b in 0..256u32 {
        if !printable.contains(&b) {
            printable.push(b);
            chars.push(256 + extra);
            extra += 1;
        }
    }
    let mut table = [' '; 256];
    for (b, c) in printable.iter().zip(chars) {
        table[*b as usize] = char::from_u32(c).unwrap();
    }
    table
}

impl Tokenizer {
    /// Reads a Hugging Face `tokenizer.json` (BPE model with `</w>` suffixes).
    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let model = &v["model"];
        let vocab: HashMap<String, u32> = model["vocab"]
            .as_object()
            .ok_or("tokenizer.json has no vocabulary")?
            .iter()
            .filter_map(|(k, id)| Some((k.clone(), id.as_u64()? as u32)))
            .collect();
        let mut ranks = HashMap::new();
        for (rank, m) in model["merges"].as_array().ok_or("tokenizer.json has no merges")?.iter().enumerate() {
            let pair = match m {
                Value::String(s) => s.split_once(' ').map(|(a, b)| (a.to_string(), b.to_string())),
                Value::Array(a) => match (a.first().and_then(Value::as_str), a.get(1).and_then(Value::as_str)) {
                    (Some(x), Some(y)) => Some((x.to_string(), y.to_string())),
                    _ => None,
                },
                _ => None,
            };
            if let Some(p) = pair {
                ranks.insert(p, rank as u32);
            }
        }
        let special = |name: &str| -> Option<u32> {
            v["added_tokens"].as_array()?.iter().find(|t| t["content"] == name)?["id"].as_u64().map(|x| x as u32).or_else(|| vocab.get(name).copied())
        };
        let start = special("<|startoftext|>").or_else(|| vocab.get("<|startoftext|>").copied()).ok_or("no start token")?;
        let end = special("<|endoftext|>").or_else(|| vocab.get("<|endoftext|>").copied()).ok_or("no end token")?;
        let pattern = Regex::new(r"(?i)<\|startoftext\|>|<\|endoftext\|>|'s|'t|'re|'ve|'m|'ll|'d|[\p{L}]+|[\p{N}]|[^\s\p{L}\p{N}]+").unwrap();
        Ok(Tokenizer { vocab, ranks, byte_chars: byte_chars(), pattern, start, end, cache: Mutex::new(HashMap::new()) })
    }

    fn bpe(&self, token: &str) -> Vec<u32> {
        if let Some(hit) = self.cache.lock().unwrap().get(token) {
            return hit.clone();
        }
        let chars: Vec<char> = token.chars().collect();
        let mut word: Vec<String> = chars.iter().map(|c| c.to_string()).collect();
        if let Some(last) = word.last_mut() {
            last.push_str("</w>");
        }
        while word.len() > 1 {
            let best = (0..word.len() - 1)
                .filter_map(|i| self.ranks.get(&(word[i].clone(), word[i + 1].clone())).map(|r| (*r, i)))
                .min();
            let Some((_, _)) = best else { break };
            let (a, b) = {
                let i = best.unwrap().1;
                (word[i].clone(), word[i + 1].clone())
            };
            let mut merged = Vec::with_capacity(word.len());
            let mut i = 0;
            while i < word.len() {
                if i + 1 < word.len() && word[i] == a && word[i + 1] == b {
                    merged.push(format!("{a}{b}"));
                    i += 2;
                } else {
                    merged.push(word[i].clone());
                    i += 1;
                }
            }
            word = merged;
        }
        let ids: Vec<u32> = word.iter().filter_map(|w| self.vocab.get(w).copied()).collect();
        self.cache.lock().unwrap().insert(token.to_string(), ids.clone());
        ids
    }

    /// Token ids with start/end markers, truncated to CLIP's 77 positions.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let clean: String = text.nfc().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        let mut ids = vec![self.start];
        for m in self.pattern.find_iter(&clean) {
            let mapped: String = m.as_str().bytes().map(|b| self.byte_chars[b as usize]).collect();
            ids.extend(self.bpe(&mapped));
        }
        ids.truncate(CONTEXT_LENGTH - 1);
        ids.push(self.end);
        ids
    }
}

// ── image preprocessing ───────────────────────────────────────────────────

/// CLIP's input for one picture: RGB, shortest side resized to 224 (bicubic),
/// centre-cropped to 224×224, normalized, channels first.
pub fn pixel_values(img: &DynamicImage) -> Vec<f32> {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let (nw, nh) = if w <= h {
        (IMAGE_SIZE, ((h as u64 * IMAGE_SIZE as u64) / w as u64).max(IMAGE_SIZE as u64) as u32)
    } else {
        (((w as u64 * IMAGE_SIZE as u64) / h as u64).max(IMAGE_SIZE as u64) as u32, IMAGE_SIZE)
    };
    let rgb = img.to_rgb8();
    let resized = image::imageops::resize(&rgb, nw, nh, image::imageops::FilterType::CatmullRom);
    let (left, top) = ((nw - IMAGE_SIZE) / 2, (nh - IMAGE_SIZE) / 2);
    let plane = (IMAGE_SIZE * IMAGE_SIZE) as usize;
    let mut out = vec![0f32; 3 * plane];
    for y in 0..IMAGE_SIZE {
        for x in 0..IMAGE_SIZE {
            let p = resized.get_pixel(x + left, y + top);
            let i = (y * IMAGE_SIZE + x) as usize;
            for c in 0..3 {
                out[c * plane + i] = (p[c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    out
}

// ── the model ─────────────────────────────────────────────────────────────

struct Io {
    inputs: Vec<(String, Vec<i64>)>,
    output: String,
}

fn io_of(session: &Session, preferred_output: &str) -> Io {
    let shape_of = |t: &ValueType| match t {
        ValueType::Tensor { shape, .. } => shape.iter().copied().collect(),
        _ => Vec::new(),
    };
    let inputs = session.inputs().iter().map(|i| (i.name().to_string(), shape_of(i.dtype()))).collect();
    let outputs = session.outputs();
    let output = outputs
        .iter()
        .find(|o| o.name() == preferred_output)
        .or_else(|| outputs.iter().find(|o| shape_of(o.dtype()).len() == 2))
        .or(outputs.first())
        .map(|o| o.name().to_string())
        .unwrap_or_default();
    Io { inputs, output }
}

pub struct Clip {
    vision: Mutex<Session>,
    text: Mutex<Session>,
    vision_io: Io,
    text_io: Io,
    pub tokenizer: Tokenizer,
}

/// ONNX Runtime allows one environment per process, and it must outlive
/// every session, so it is created once and kept.
fn environment() -> Result<Environment, String> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name("ateliet").build().map_err(|e| e.to_string())).clone()
}

fn session(path: &Path, threads: usize) -> Result<Session, String> {
    Session::builder(&environment()?)
        .map_err(|e| e.to_string())?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        .with_intra_threads(threads)
        .map_err(|e| e.to_string())?
        .commit_from_file(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    crate::images::normalize(&mut v);
    v
}

impl Clip {
    pub fn load(dir: &Path) -> Result<Clip, String> {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(1).max(1);
        let tokenizer = Tokenizer::from_json(&std::fs::read_to_string(dir.join("tokenizer.json")).map_err(|e| e.to_string())?)?;
        let vision = session(&dir.join("vision_model.onnx"), threads)?;
        let text = session(&dir.join("text_model.onnx"), threads.min(4))?;
        let vision_io = io_of(&vision, "image_embeds");
        let text_io = io_of(&text, "text_embeds");
        Ok(Clip { vision: Mutex::new(vision), text: Mutex::new(text), vision_io, text_io, tokenizer })
    }

    /// Unit vectors for a batch of `pixel_values`.
    pub fn embed_images(&self, batch: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, String> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        let fixed_batch = self.vision_io.inputs.first().and_then(|(_, s)| s.first().copied()).filter(|d| *d > 0);
        if fixed_batch.is_some_and(|b| b as usize != batch.len()) {
            return batch.iter().map(|one| Ok(self.embed_images(std::slice::from_ref(one))?.remove(0))).collect();
        }
        let s = IMAGE_SIZE as usize;
        let data: Vec<f32> = batch.iter().flatten().copied().collect();
        let tensor = Tensor::from_array(([batch.len(), 3, s, s], data)).map_err(|e| e.to_string())?;
        let name = self.vision_io.inputs.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "pixel_values".into());
        let mut session = self.vision.lock().unwrap();
        let outputs = session.run(ort::inputs![name => tensor]).map_err(|e| e.to_string())?;
        let (shape, values) = outputs[self.vision_io.output.as_str()].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let dims = *shape.last().unwrap_or(&0) as usize;
        if dims == 0 || values.len() != dims * batch.len() {
            return Err("unexpected image embedding shape".into());
        }
        Ok(values.chunks(dims).map(|c| normalized(c.to_vec())).collect())
    }

    /// Unit vector for a sentence.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, String> {
        let mut ids: Vec<i64> = self.tokenizer.encode(text).into_iter().map(|x| x as i64).collect();
        let real = ids.len();
        let fixed_len = self.text_io.inputs.iter().find(|(n, _)| n == "input_ids").and_then(|(_, s)| s.get(1).copied()).filter(|d| *d > 0);
        if let Some(len) = fixed_len {
            ids.resize(len as usize, 0);
        }
        let len = ids.len();
        let mask: Vec<i64> = (0..len).map(|i| (i < real) as i64).collect();
        let mut inputs: Vec<(String, Tensor<i64>)> = Vec::new();
        for (name, _) in &self.text_io.inputs {
            let data = match name.as_str() {
                "attention_mask" => mask.clone(),
                "position_ids" => (0..len as i64).collect(),
                _ => ids.clone(),
            };
            inputs.push((name.clone(), Tensor::from_array(([1usize, len], data)).map_err(|e| e.to_string())?));
        }
        let mut session = self.text.lock().unwrap();
        let outputs = session.run(inputs).map_err(|e| e.to_string())?;
        let (_, values) = outputs[self.text_io.output.as_str()].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        if values.is_empty() {
            return Err("empty text embedding".into());
        }
        Ok(normalized(values.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_tokenizer() -> Tokenizer {
        // vocabulary: single byte-chars, their </w> forms, two merges.
        let mut vocab = serde_json::Map::new();
        let mut id = 0;
        for c in ["c", "a", "t", "s"] {
            vocab.insert(c.into(), id.into());
            id += 1;
            vocab.insert(format!("{c}</w>"), id.into());
            id += 1;
        }
        for t in ["ca", "cat</w>", "<|startoftext|>", "<|endoftext|>"] {
            vocab.insert(t.into(), id.into());
            id += 1;
        }
        let json = serde_json::json!({ "model": { "vocab": vocab, "merges": ["c a", "ca t</w>"] }, "added_tokens": [] });
        Tokenizer::from_json(&json.to_string()).unwrap()
    }

    #[test]
    fn byte_table_is_a_bijection() {
        let t = byte_chars();
        let set: std::collections::HashSet<char> = t.iter().copied().collect();
        assert_eq!(set.len(), 256);
        assert_eq!(t[b'a' as usize], 'a');
        assert_eq!(t[b' ' as usize], 'Ġ');
    }

    #[test]
    fn bpe_merges_by_rank_and_wraps_with_markers() {
        let tok = tiny_tokenizer();
        // "CAT  cats": lowercased, whitespace collapsed; "cats" → c a t s</w> → ca t s</w>
        assert_eq!(tok.encode("CAT  cats"), vec![10, 9, 8, 4, 7, 11]);
    }

    #[test]
    fn long_text_is_truncated_to_the_context() {
        let tok = tiny_tokenizer();
        let ids = tok.encode(&"cat ".repeat(200));
        assert_eq!(ids.len(), CONTEXT_LENGTH);
        assert_eq!(ids.last(), Some(&11));
    }

    #[test]
    fn pixel_values_are_cropped_and_normalized() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(400, 300, image::Rgb([255, 0, 128])));
        let v = pixel_values(&img);
        assert_eq!(v.len(), 3 * 224 * 224);
        let plane = 224 * 224;
        assert!((v[0] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-4);
        assert!((v[plane] - (0.0 - MEAN[1]) / STD[1]).abs() < 1e-4);
    }

    /// Token ids from OpenAI's reference tokenizer, checked against the real
    /// vocabulary when it is available (see below).
    #[test]
    fn tokenizer_matches_reference_when_available() {
        let Ok(dir) = std::env::var("ATELIET_CLIP_TEST_DIR") else { return };
        let tok = Tokenizer::from_json(&std::fs::read_to_string(Path::new(&dir).join("tokenizer.json")).unwrap()).unwrap();
        let cases: &[(&str, &[u32])] = &[
            ("a photo of a cat", &[49406, 320, 1125, 539, 320, 2368, 49407]),
            ("Mãos e anatomia, 3 poses!", &[49406, 76, 19339, 1299, 324, 33759, 1073, 267, 274, 9773, 256, 49407]),
            ("  Cute   and PASTEL  ", &[49406, 2242, 537, 19457, 49407]),
            ("it's a dog's life", &[49406, 585, 568, 320, 1929, 568, 970, 49407]),
            ("東京の夜景", &[49406, 48338, 21078, 105, 21575, 23170, 250, 48132, 363, 49407]),
        ];
        for (text, ids) in cases {
            assert_eq!(&tok.encode(text), ids, "{text}");
        }
    }

    /// End-to-end check against real model files, when available:
    /// `ATELIET_CLIP_TEST_DIR=<dir with tokenizer.json, *_model.onnx> cargo test`
    #[test]
    fn model_runs_when_available() {
        let Ok(dir) = std::env::var("ATELIET_CLIP_TEST_DIR") else { return };
        let clip = Clip::load(Path::new(&dir)).unwrap();
        let t = clip.embed_text("a photo of a cat").unwrap();
        assert!((t.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-3);
        let img = DynamicImage::new_rgb8(64, 48);
        let v = clip.embed_images(&[pixel_values(&img), pixel_values(&img)]).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), t.len());
    }
}
