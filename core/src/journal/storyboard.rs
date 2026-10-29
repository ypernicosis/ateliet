//! Storyboards into a notebook: the frames of a `.storyboard` file, drawn
//! as pictures to paste on a page (and drawn again when the storyboard
//! changes).

use crate::board::compose::Pictures;
use crate::board::doc::BoardDoc;
use crate::board::export;
use image::RgbaImage;
use std::path::Path;

/// A frame of a storyboard: where it is, its id, its name and its notes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Frame {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub action: String,
    pub dialogue: String,
}

impl Frame {
    /// The caption a frame gets on a page: its number and name, then the
    /// action and the dialogue.
    pub fn caption(&self) -> String {
        let mut s = format!("{}", self.index + 1);
        if !self.name.trim().is_empty() {
            s.push_str(&format!(". {}", self.name.trim()));
        }
        for part in [&self.action, &self.dialogue] {
            if !part.trim().is_empty() {
                s.push('\n');
                s.push_str(part.trim());
            }
        }
        s
    }
}

fn frames_of(doc: &BoardDoc) -> Vec<Frame> {
    (0..doc.frame_count()).map(|i| {
        let f = doc.frame(i);
        let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
        Frame { index: i, id: doc.frame_id(i), name: s(&f["name"]), action: s(&f["notes"]["action"]), dialogue: s(&f["notes"]["dialogue"]) }
    }).collect()
}

/// The frames of a storyboard file.
pub fn frames(abs: &Path) -> Result<Vec<Frame>, String> {
    Ok(frames_of(&BoardDoc::open(abs)?))
}

/// The frames, each drawn no larger than `max_side` px.
pub fn render_all(abs: &Path, max_side: u32) -> Result<Vec<(Frame, RgbaImage)>, String> {
    let mut doc = BoardDoc::open(abs)?;
    let mut pics = Pictures::default();
    let scale = scale_for(&doc, max_side);
    let list = frames_of(&doc);
    Ok(list.into_iter().map(|f| {
        let (w, h, px) = export::frame_rgba(&mut doc, &mut pics, f.index, scale, false, false);
        (f, RgbaImage::from_raw(w, h, px).unwrap_or_default())
    }).collect())
}

/// One frame (by its id, else by its place), no larger than `max_side`.
pub fn render(abs: &Path, id: &str, index: usize, max_side: u32) -> Result<(Frame, RgbaImage), String> {
    let mut doc = BoardDoc::open(abs)?;
    let list = frames_of(&doc);
    let f = list.iter().find(|f| f.id == id).or_else(|| list.get(index)).cloned().ok_or("o quadro não existe mais no storyboard")?;
    let mut pics = Pictures::default();
    let scale = scale_for(&doc, max_side);
    let (w, h, px) = export::frame_rgba(&mut doc, &mut pics, f.index, scale, false, false);
    Ok((f, RgbaImage::from_raw(w, h, px).unwrap_or_default()))
}

fn scale_for(doc: &BoardDoc, max_side: u32) -> f32 {
    let side = doc.width().max(doc.height()).max(1) as f32;
    (max_side as f32 / side).min(1.0)
}

/// The picture file a frame is saved as, in `folder`.
pub fn file_name(board_path: &str, f: &Frame) -> String {
    let base = board_path.rsplit('/').next().unwrap_or(board_path).trim_end_matches(".storyboard");
    export::frame_file_name(base, f.index, &f.name)
}

/// A storyboard's timeline: its frames per second and how long each frame
/// is held (in frames of the timeline).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub fps: u32,
    pub frames: Vec<(Frame, u32)>,
}

impl Clip {
    /// When frame `i` starts, in seconds.
    pub fn start(&self, i: usize) -> f32 {
        self.frames.iter().take(i).map(|f| f.1).sum::<u32>() as f32 / self.fps.max(1) as f32
    }

    /// How long frames `from..=to` last, in seconds.
    pub fn span(&self, from: usize, to: usize) -> f32 {
        self.frames.iter().skip(from).take(to.saturating_sub(from) + 1).map(|f| f.1).sum::<u32>() as f32 / self.fps.max(1) as f32
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// The timeline of a storyboard file.
pub fn clip(abs: &Path) -> Result<Clip, String> {
    let doc = BoardDoc::open(abs)?;
    let frames = frames_of(&doc).into_iter().map(|f| {
        let d = doc.frame(f.index)["duration"].as_f64().unwrap_or(24.0).round().clamp(1.0, 65535.0) as u32;
        (f, d)
    }).collect();
    Ok(Clip { fps: doc.fps().max(1), frames })
}

/// Frames `from..=to` drawn no larger than `max_side`, each with how long
/// it is held (seconds).
pub fn render_span(abs: &Path, from: usize, to: usize, max_side: u32) -> Result<Vec<(RgbaImage, f32)>, String> {
    let mut doc = BoardDoc::open(abs)?;
    let mut pics = Pictures::default();
    let scale = scale_for(&doc, max_side);
    let fps = doc.fps().max(1) as f32;
    let n = doc.frame_count();
    if n == 0 {
        return Err("o storyboard não tem quadros".into());
    }
    let to = to.min(n - 1);
    Ok((from.min(to)..=to).map(|i| {
        let (w, h, px) = export::frame_rgba(&mut doc, &mut pics, i, scale, false, false);
        let d = doc.frame(i)["duration"].as_f64().unwrap_or(24.0).max(1.0) as f32 / fps;
        (RgbaImage::from_raw(w, h, px).unwrap_or_default(), d)
    }).collect())
}

/// Frames as an animated PNG (it plays in Obsidian and in browsers).
pub fn apng(frames: &[(RgbaImage, f32)]) -> Vec<u8> {
    let imgs: Vec<(u32, u32, Vec<u8>)> = frames.iter().map(|(i, _)| (i.width(), i.height(), i.as_raw().clone())).collect();
    let delays: Vec<(u16, u16)> = frames.iter().map(|(_, d)| ((d * 1000.0).round().clamp(1.0, 65535.0) as u16, 1000)).collect();
    export::apng(&imgs, &delays)
}

/// The picture a storyboard is saved as on a page: a span, animated or one
/// frame.
pub fn span_file_name(board_path: &str, from: usize, to: usize, animated: bool) -> String {
    let base = export::safe(board_path.rsplit('/').next().unwrap_or(board_path).trim_end_matches(".storyboard"));
    if animated {
        format!("{base} {}–{}.png", from + 1, to + 1)
    } else {
        format!("{base} {}.png", from + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_render_and_caption() {
        let dir = std::env::temp_dir().join(format!("ateliet-sb-{}", super::super::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Cena.storyboard");
        let mut doc = BoardDoc::new("animatic", 320, 180, "#ffffff", 12);
        doc.add_frame(1);
        doc.board["frames"][0]["name"] = serde_json::json!("Abertura");
        doc.board["frames"][0]["notes"]["action"] = serde_json::json!("Ela entra.");
        doc.save(&path, |_, _| None).unwrap();
        let all = render_all(&path, 160).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].1.dimensions(), (160, 90));
        assert_eq!(all[0].0.caption(), "1. Abertura\nEla entra.");
        let (f, img) = render(&path, &all[1].0.id, 0, 64).unwrap();
        assert_eq!(f.index, 1);
        assert_eq!(img.width(), 64);
        assert_eq!(file_name("Pasta/Cena.storyboard", &all[0].0), "Cena 001 Abertura.png");
        let c = clip(&path).unwrap();
        assert_eq!(c.len(), 2);
        assert!(c.span(0, 1) > c.start(1));
        let span = render_span(&path, 1, 5, 64).unwrap();
        assert_eq!(span.len(), 1);
        let png = apng(&render_span(&path, 0, 1, 32).unwrap());
        assert!(png.windows(4).any(|w| w == b"acTL"));
        assert_eq!(span_file_name("A/Cena.storyboard", 0, 1, true), "Cena 1–2.png");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
