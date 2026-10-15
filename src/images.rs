//! The image library: every picture in the vault (plus optional external
//! folders), a persistent index with dimensions, content digests and CLIP
//! vectors, and the searches built on it.
//!
//! Keys are vault-relative paths (`Anexos/gato.jpg`) for pictures inside the
//! vault and absolute paths for pictures in external folders. Everything
//! derived from a picture is rebuildable; only the vectors are expensive, so
//! they are kept in the vault (`.obsidian/ateliet/images/index.bin`) and
//! survive renames through the content digest.

use image::{DynamicImage, ImageDecoder, ImageReader};
use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

pub const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif"];

pub fn is_image(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    EXTENSIONS.contains(&ext.as_str())
}

/// Where a picture comes from: the vault, a folder the user added, or the
/// shared picture library (the Pictogrep data folder).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    #[default]
    Vault,
    External,
    Shared,
}

/// A folder the library reads pictures from.
#[derive(Clone, Debug)]
pub struct Source {
    pub root: PathBuf,
    /// Outside the vault: keys are absolute paths.
    pub external: bool,
    pub origin: Origin,
}

impl Source {
    pub fn vault(root: PathBuf) -> Self {
        Source { root, external: false, origin: Origin::Vault }
    }
    pub fn outside(root: PathBuf, origin: Origin) -> Self {
        Source { root, external: true, origin }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ImageEntry {
    /// Vault-relative path, or absolute path for external pictures.
    pub key: String,
    /// Absolute path on disk.
    pub abs: String,
    pub name: String,
    /// Parent folder: vault-relative ("" is the vault root) or absolute.
    pub folder: String,
    pub mtime: i64,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub external: bool,
    pub origin: Origin,
}

// ── scanning ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Found {
    pub key: String,
    pub abs: PathBuf,
    pub folder: String,
    pub mtime: i64,
    pub size: u64,
    pub external: bool,
    pub origin: Origin,
}

pub fn mtime_ms(m: &fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn slash(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// The library key of a path under `source` (whether or not it exists).
pub fn key_for(source: &Source, abs: &Path) -> String {
    if source.external {
        abs.to_string_lossy().into_owned()
    } else {
        slash(abs.strip_prefix(&source.root).unwrap_or(abs))
    }
}

/// Describes one file, or `None` when it is not a picture of the library.
pub fn describe(source: &Source, abs: &Path) -> Option<Found> {
    let name = abs.file_name()?.to_string_lossy().into_owned();
    if name.starts_with('.') || !is_image(&name) {
        return None;
    }
    let rel = abs.strip_prefix(&source.root).ok()?;
    if rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
        return None;
    }
    let meta = fs::metadata(abs).ok()?;
    if !meta.is_file() {
        return None;
    }
    let key = key_for(source, abs);
    let folder = if source.external {
        abs.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        key.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default()
    };
    Some(Found { key, abs: abs.to_path_buf(), folder, mtime: mtime_ms(&meta), size: meta.len(), external: source.external, origin: source.origin })
}

/// Every picture under the sources. Dot-folders (`.obsidian`, `.trash`,
/// `.git` …) and `node_modules` are skipped, like the vault listing does.
pub fn scan(sources: &[Source]) -> Vec<Found> {
    let mut out: Vec<Found> = sources
        .par_iter()
        .flat_map_iter(|source| {
            let walker = WalkDir::new(&source.root).follow_links(true).into_iter().filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                e.depth() == 0 || !(n.starts_with('.') || n == "node_modules")
            });
            let mut found = Vec::new();
            for e in walker.flatten() {
                if e.file_type().is_file() || e.path_is_symlink() {
                    if let Some(f) = describe(source, e.path()) {
                        found.push(f);
                    }
                }
            }
            found
        })
        .collect();
    let mut seen = HashSet::new();
    out.retain(|f| seen.insert(f.key.clone()));
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

// ── decoding ──────────────────────────────────────────────────────────────

/// Decodes a picture with its EXIF orientation applied.
pub fn open_oriented(path: &Path) -> image::ImageResult<DynamicImage> {
    let mut decoder = ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Pixel size as displayed (orientation applied), read from the header only.
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut decoder = ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    use image::metadata::Orientation::*;
    match decoder.orientation().unwrap_or(NoTransforms) {
        Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH => Some((h, w)),
        _ => Some((w, h)),
    }
}

pub fn file_digest(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

/// A preview that fits in `max`×`max`. Pictures with transparency stay PNG so
/// cut-outs keep their shape; everything else becomes a JPEG.
pub fn thumbnail(img: &DynamicImage, max: u32) -> (Vec<u8>, &'static str) {
    let small = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img.clone() };
    let mut out = Vec::new();
    if small.color().has_alpha() {
        let _ = small.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
        (out, "image/png")
    } else {
        let rgb = small.to_rgb8();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
        let _ = enc.encode_image(&rgb);
        (out, "image/jpeg")
    }
}

/// Where the cached preview of a picture lives. The stat is part of the name,
/// so an edited picture simply misses the cache.
pub fn thumbnail_path(cache_dir: &Path, abs: &Path, mtime: i64, size: u64, max: u32) -> PathBuf {
    let mut h = Sha256::new();
    h.update(abs.to_string_lossy().as_bytes());
    h.update(mtime.to_le_bytes());
    h.update(size.to_le_bytes());
    let hex: String = h.finalize()[..12].iter().map(|b| format!("{b:02x}")).collect();
    cache_dir.join(&hex[..2]).join(format!("{hex}-{max}"))
}

// ── the persistent index ──────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub mtime: i64,
    pub size: u64,
    pub width: u32,
    pub height: u32,
    pub digest: Option<[u8; 32]>,
    pub vector: Option<Vec<f32>>,
}

#[derive(Debug, Default)]
pub struct Index {
    /// Identity of the model that produced the vectors.
    pub model: String,
    pub dims: usize,
    pub records: HashMap<String, Record>,
}

const MAGIC: &[u8; 4] = b"ATI1";

impl Index {
    pub fn new(model: &str, dims: usize) -> Self {
        Index { model: model.into(), dims, records: HashMap::new() }
    }

    /// Reads an index. Vectors from a different model are dropped (they are
    /// not comparable), everything else is kept.
    pub fn load(path: &Path, model: &str, dims: usize) -> Self {
        let mut index = Index::new(model, dims);
        let Ok(data) = fs::read(path) else { return index };
        let _ = index.decode(&data);
        index
    }

    fn decode(&mut self, data: &[u8]) -> Option<()> {
        let mut r = Reader { data, pos: 0 };
        if r.take(4)? != MAGIC {
            return None;
        }
        let file_dims = r.u32()? as usize;
        let len = r.u32()? as usize;
        let file_model = String::from_utf8_lossy(r.take(len)?).into_owned();
        let keep_vectors = file_model == self.model && file_dims == self.dims;
        let count = r.u32()?;
        for _ in 0..count {
            let len = r.u32()? as usize;
            let key = String::from_utf8_lossy(r.take(len)?).into_owned();
            let mtime = r.u64()? as i64;
            let size = r.u64()?;
            let width = r.u32()?;
            let height = r.u32()?;
            let flags = r.take(1)?[0];
            let digest = if flags & 1 != 0 { Some(r.take(32)?.try_into().ok()?) } else { None };
            let vector = if flags & 2 != 0 {
                let raw = r.take(file_dims * 4)?;
                let v: Vec<f32> = raw.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
                keep_vectors.then_some(v)
            } else {
                None
            };
            self.records.insert(key, Record { mtime, size, width, height, digest, vector });
        }
        Some(())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.records.len() * (self.dims * 4 + 96) + 64);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.dims as u32).to_le_bytes());
        out.extend_from_slice(&(self.model.len() as u32).to_le_bytes());
        out.extend_from_slice(self.model.as_bytes());
        let mut keys: Vec<&String> = self.records.keys().collect();
        keys.sort();
        out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
        for key in keys {
            let rec = &self.records[key];
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&(rec.mtime as u64).to_le_bytes());
            out.extend_from_slice(&rec.size.to_le_bytes());
            out.extend_from_slice(&rec.width.to_le_bytes());
            out.extend_from_slice(&rec.height.to_le_bytes());
            let vector = rec.vector.as_ref().filter(|v| v.len() == self.dims);
            out.push(rec.digest.is_some() as u8 | (vector.is_some() as u8) << 1);
            if let Some(d) = &rec.digest {
                out.extend_from_slice(d);
            }
            if let Some(v) = vector {
                for x in v {
                    out.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
        out
    }

    /// Atomic write: a crash mid-save never leaves a truncated index behind.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&self.encode())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)
    }

    /// Turns scan results into entries, reusing the stored dimensions of
    /// unchanged files and reading the headers of new or edited ones.
    /// Records of edited files lose their digest and vector.
    pub fn update_from_scan(&mut self, found: &[Found]) -> Vec<ImageEntry> {
        let fresh: Vec<(usize, (u32, u32))> = found
            .par_iter()
            .enumerate()
            .filter(|(_, f)| self.records.get(&f.key).is_none_or(|r| r.mtime != f.mtime || r.size != f.size))
            .map(|(i, f)| (i, dimensions(&f.abs).unwrap_or((0, 0))))
            .collect();
        for (i, (width, height)) in fresh {
            let f = &found[i];
            self.records.insert(f.key.clone(), Record { mtime: f.mtime, size: f.size, width, height, digest: None, vector: None });
        }
        found
            .iter()
            .map(|f| {
                let r = &self.records[&f.key];
                ImageEntry {
                    key: f.key.clone(),
                    abs: f.abs.to_string_lossy().into_owned(),
                    name: f.abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    folder: f.folder.clone(),
                    mtime: f.mtime,
                    size: f.size,
                    width: r.width,
                    height: r.height,
                    external: f.external,
                    origin: f.origin,
                }
            })
            .collect()
    }

    pub fn vector(&self, key: &str) -> Option<&[f32]> {
        self.records.get(key)?.vector.as_deref().filter(|v| v.len() == self.dims)
    }

    pub fn has_vector(&self, key: &str) -> bool {
        self.vector(key).is_some()
    }

    /// Records whose file is gone, by digest: a picture that reappears under a
    /// new name (renamed, moved) takes its vector back instead of being
    /// analysed again.
    pub fn orphans_by_digest(&self, live: &HashSet<&str>) -> HashMap<[u8; 32], String> {
        self.records
            .iter()
            .filter(|(k, r)| !live.contains(k.as_str()) && r.vector.is_some())
            .filter_map(|(k, r)| r.digest.map(|d| (d, k.clone())))
            .collect()
    }

    /// Forgets records of files that no longer exist.
    pub fn prune(&mut self, live: &HashSet<&str>) -> usize {
        let before = self.records.len();
        self.records.retain(|k, _| live.contains(k.as_str()));
        before - self.records.len()
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
}

// ── searching ─────────────────────────────────────────────────────────────

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

/// The words of a file name, for name search: lowercased, with `_`, `-`, `.`
/// and `/` treated as spaces.
pub fn searchable_name(key: &str) -> String {
    key.to_lowercase().chars().map(|c| if matches!(c, '_' | '-' | '.' | '/' | '\\') { ' ' } else { c }).collect()
}

/// Fraction of the query words found in the name (0 when nothing matches).
pub fn name_score(words: &[String], name: &str) -> f32 {
    if words.is_empty() {
        return 0.0;
    }
    words.iter().filter(|w| name.contains(w.as_str())).count() as f32 / words.len() as f32
}

pub fn query_words(query: &str) -> Vec<String> {
    searchable_name(query).split_whitespace().map(|s| s.to_string()).collect()
}

/// Ranks `candidates` (indices into `entries`) by similarity to `vector`,
/// best first. Pictures without a vector are left out. `words` adds a small
/// bonus for name matches, so a picture literally called "cat" wins ties
/// against other cats.
pub fn rank(entries: &[ImageEntry], index: &Index, candidates: &[usize], vector: &[f32], words: &[String]) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = candidates
        .par_iter()
        .filter_map(|&i| {
            let e = &entries[i];
            let v = index.vector(&e.key)?;
            let bonus = if words.is_empty() { 0.0 } else { 0.05 * name_score(words, &searchable_name(&e.key)) };
            Some((i, dot(vector, v) + bonus))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored
}

/// Name-only search, for when the model is not available.
pub fn rank_by_name(entries: &[ImageEntry], candidates: &[usize], words: &[String]) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = candidates
        .iter()
        .filter_map(|&i| {
            let s = name_score(words, &searchable_name(&entries[i].key));
            (s > 0.0).then_some((i, s))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored
}

/// Deterministic shuffle (xorshift), so paging through a random order with
/// the same seed never repeats or skips a picture.
pub fn shuffle(items: &mut [usize], seed: u64) {
    let mut s = seed | 1;
    for i in (1..items.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        items.swap(i, (s % (i as u64 + 1)) as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str) -> ImageEntry {
        ImageEntry { key: key.into(), abs: format!("/v/{key}"), name: key.into(), folder: String::new(), mtime: 1, size: 1, width: 1, height: 1, external: false, origin: Origin::Vault }
    }

    #[test]
    fn index_roundtrip_keeps_everything() {
        let mut idx = Index::new("m", 3);
        idx.records.insert("a.jpg".into(), Record { mtime: 5, size: 9, width: 10, height: 20, digest: Some([7; 32]), vector: Some(vec![1.0, 0.0, 0.5]) });
        idx.records.insert("b/c.png".into(), Record { mtime: -1, size: 0, width: 0, height: 0, digest: None, vector: None });
        let mut back = Index::new("m", 3);
        back.decode(&idx.encode()).unwrap();
        assert_eq!(back.records, idx.records);
    }

    #[test]
    fn vectors_of_another_model_are_dropped() {
        let mut idx = Index::new("old", 2);
        idx.records.insert("a.jpg".into(), Record { mtime: 1, size: 1, width: 3, height: 4, digest: Some([1; 32]), vector: Some(vec![1.0, 0.0]) });
        let mut back = Index::new("new", 2);
        back.decode(&idx.encode()).unwrap();
        let r = &back.records["a.jpg"];
        assert_eq!((r.width, r.vector.as_ref()), (3, None));
        assert_eq!(r.digest, Some([1; 32]));
    }

    #[test]
    fn truncated_index_is_ignored_without_panicking() {
        let mut idx = Index::new("m", 2);
        idx.records.insert("a.jpg".into(), Record { vector: Some(vec![1.0, 2.0]), ..Default::default() });
        let data = idx.encode();
        for cut in 0..data.len() {
            let mut back = Index::new("m", 2);
            let _ = back.decode(&data[..cut]);
        }
    }

    #[test]
    fn rank_orders_by_similarity_and_skips_unindexed() {
        let entries = vec![entry("cat.jpg"), entry("dog.jpg"), entry("new.jpg")];
        let mut idx = Index::new("m", 2);
        idx.records.insert("cat.jpg".into(), Record { vector: Some(vec![1.0, 0.0]), ..Default::default() });
        idx.records.insert("dog.jpg".into(), Record { vector: Some(vec![0.0, 1.0]), ..Default::default() });
        let r = rank(&entries, &idx, &[0, 1, 2], &[0.2, 0.9], &[]);
        assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 0]);
    }

    #[test]
    fn name_search_matches_words_in_paths() {
        let entries = vec![entry("refs/hand_pose-01.png"), entry("cats/tabby.jpg")];
        let words = query_words("Hand pose");
        let r = rank_by_name(&entries, &[0, 1], &words);
        assert_eq!(r, vec![(0, 1.0)]);
    }

    #[test]
    fn shuffle_is_a_stable_permutation() {
        let mut a: Vec<usize> = (0..50).collect();
        let mut b = a.clone();
        shuffle(&mut a, 42);
        shuffle(&mut b, 42);
        assert_eq!(a, b);
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(a, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn scan_finds_pictures_and_skips_hidden_folders() {
        let dir = std::env::temp_dir().join(format!("ateliet-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Anexos")).unwrap();
        fs::create_dir_all(dir.join(".obsidian")).unwrap();
        let img = DynamicImage::new_rgb8(30, 20);
        img.save(dir.join("Anexos/a.png")).unwrap();
        img.save(dir.join(".obsidian/hidden.png")).unwrap();
        fs::write(dir.join("note.md"), "x").unwrap();
        let found = scan(&[Source::vault(dir.clone())]);
        assert_eq!(found.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["Anexos/a.png"]);
        let mut idx = Index::new("m", 2);
        let entries = idx.update_from_scan(&found);
        assert_eq!((entries[0].width, entries[0].height, entries[0].folder.as_str()), (30, 20, "Anexos"));
        let _ = fs::remove_dir_all(&dir);
    }
}
