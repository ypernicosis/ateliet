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
        let _ = std::fs::remove_dir_all(&dir);
    }
}
