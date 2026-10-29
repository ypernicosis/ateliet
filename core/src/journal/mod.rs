//! The Journal: notebooks of fixed-size pages, turned one by one, where
//! text sits on the paper's ruling and pictures are cut out and pasted over
//! it (see `docs/JOURNAL.md`).
//!
//! A notebook is a folder: an index note (`journal: caderno` in its
//! frontmatter: size, paper, columns, cover, bookmark) and one note per page
//! (`journal: página`, with its number in `pagina`). A page keeps what is
//! text in its body, one `## ^j-<id>` section per item, so Obsidian, the
//! search and Dataview read it as a note; where things are (box, rotation,
//! layer, crop, look) goes in a `%% ateliet:journal %%` comment at the end,
//! as the Quadro does with Kanban boards.
//!
//! - `paper`  — sizes, papers and columns, and where the ruling falls on a page.
//! - `format` — reading and writing the index note and the page notes.
//! - `cut`    — a picture cut out with its mask, for the sticker drawer.
//! - `storyboard` — the frames of a storyboard, drawn to paste on a page.
//! - `texture` — the cover's textures, made from its colour.
//!
//! Lengths are millimetres of the page, from its top left corner.

pub mod cut;
pub mod format;
pub mod paper;
pub mod storyboard;
pub mod texture;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use format::{Notebook, Page, PageRef};
pub use paper::{Columns, Layout, PageSize, Paper, Ruling};

/// What an item is. Unknown kinds (from a newer version) are kept by name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Kind {
    /// Text on the ruling; its Markdown is the item's section.
    Text,
    /// A quoted passage with its source (a `[!trecho]` callout).
    Excerpt,
    /// A picture of the vault (`![[…]]`), with a caption under the embed.
    Image,
    /// A picture already cut out, from the sticker drawer.
    Sticker,
    /// A handwritten page scanned (the picture, and its transcription).
    Scan,
    /// A note of the vault shown live (`![[note#section]]`).
    Note,
    /// Strokes drawn by hand.
    Ink,
    /// A straight or broken line.
    Line,
    /// A strip of tape.
    Tape,
    Other(String),
}

impl From<String> for Kind {
    fn from(s: String) -> Kind {
        match s.as_str() {
            "texto" => Kind::Text,
            "trecho" => Kind::Excerpt,
            "imagem" => Kind::Image,
            "adesivo" => Kind::Sticker,
            "digitalizacao" => Kind::Scan,
            "nota" => Kind::Note,
            "tinta" => Kind::Ink,
            "linha" => Kind::Line,
            "fita" => Kind::Tape,
            _ => Kind::Other(s),
        }
    }
}

impl From<Kind> for String {
    fn from(k: Kind) -> String {
        match k {
            Kind::Text => "texto".into(),
            Kind::Excerpt => "trecho".into(),
            Kind::Image => "imagem".into(),
            Kind::Sticker => "adesivo".into(),
            Kind::Scan => "digitalizacao".into(),
            Kind::Note => "nota".into(),
            Kind::Ink => "tinta".into(),
            Kind::Line => "linha".into(),
            Kind::Tape => "fita".into(),
            Kind::Other(s) => s,
        }
    }
}

impl Kind {
    /// Kinds whose content is a section of the note's body.
    pub fn has_section(&self) -> bool {
        matches!(self, Kind::Text | Kind::Excerpt | Kind::Image | Kind::Sticker | Kind::Scan | Kind::Note)
    }

    pub fn is_picture(&self) -> bool {
        matches!(self, Kind::Image | Kind::Sticker | Kind::Scan)
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Kind::Text | Kind::Excerpt)
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// A window over the source picture, each side in 0..1 (as the moodboards).
pub use crate::moodboard::Crop;

/// A cut that is not a rectangle: one of the stock shapes, or a path drawn
/// round the object (points in 0..1 of the item's box).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Mask {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shape: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pts: Vec<[f64; 2]>,
}

/// The stock shapes a picture can be cut to (`Mask::shape`).
pub const SHAPES: &[(&str, &str)] = &[("circulo", "Círculo"), ("arco", "Arco"), ("estrela", "Estrela"), ("coracao", "Coração"), ("selo", "Selo serrilhado"), ("polaroide", "Polaroide")];

impl Mask {
    /// The outline in 0..1 of the box, as a closed polygon (`n` points or so
    /// for the curves). An empty mask is the rectangle.
    pub fn outline(&self, n: usize) -> Vec<(f64, f64)> {
        use std::f64::consts::{PI, TAU};
        let n = n.max(12);
        if !self.pts.is_empty() {
            return self.pts.iter().map(|p| (p[0], p[1])).collect();
        }
        match self.shape.as_str() {
            "circulo" => (0..n).map(|i| {
                let a = i as f64 / n as f64 * TAU;
                (0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin())
            }).collect(),
            // a window with a round top
            "arco" => {
                let mut v = vec![(0.0, 1.0)];
                for i in 0..=n / 2 {
                    let a = PI + i as f64 / (n / 2) as f64 * PI;
                    v.push((0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin()));
                }
                v.push((1.0, 1.0));
                v
            }
            "estrela" => (0..10).map(|i| {
                let a = -PI / 2.0 + i as f64 * PI / 5.0;
                let r = if i % 2 == 0 { 0.5 } else { 0.21 };
                (0.5 + r * a.cos(), 0.52 + r * a.sin())
            }).collect(),
            "coracao" => (0..n).map(|i| {
                let t = i as f64 / n as f64 * TAU;
                let x = 16.0 * t.sin().powi(3);
                let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
                (0.5 + x / 34.0, 0.47 - y / 34.0)
            }).collect(),
            // a postage stamp: a rectangle with scalloped edges
            "selo" => {
                let teeth = 9;
                let mut v = Vec::new();
                let side = |v: &mut Vec<(f64, f64)>, a: (f64, f64), b: (f64, f64), inward: (f64, f64)| {
                    for k in 0..teeth * 2 {
                        let t = k as f64 / (teeth * 2) as f64;
                        let d = if k % 2 == 1 { 0.035 } else { 0.0 };
                        v.push((a.0 + (b.0 - a.0) * t + inward.0 * d, a.1 + (b.1 - a.1) * t + inward.1 * d));
                    }
                };
                side(&mut v, (0.0, 0.0), (1.0, 0.0), (0.0, 1.0));
                side(&mut v, (1.0, 0.0), (1.0, 1.0), (-1.0, 0.0));
                side(&mut v, (1.0, 1.0), (0.0, 1.0), (0.0, -1.0));
                side(&mut v, (0.0, 1.0), (0.0, 0.0), (1.0, 0.0));
                v
            }
            _ => vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        }
    }

    /// A polaroid keeps the picture square and leaves a white frame, wider
    /// at the bottom: where the picture sits inside the box (0..1).
    pub fn window(&self) -> (f64, f64, f64, f64) {
        if self.shape == "polaroide" {
            (0.06, 0.05, 0.88, 0.75)
        } else {
            (0.0, 0.0, 1.0, 1.0)
        }
    }
}

/// How an item looks besides its content.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Look {
    /// `pb`, `sepia`, `contraste`, `desfoque`, `granulado`, `fotocopia`,
    /// `reticula`, `impressao` (the compositor's filters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// A sticker's edge: a margin of colour round the cut, in mm.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub border: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_color: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shadow: bool,
    /// Multiply over the paper (the ruling shows through a print).
    #[serde(default, skip_serializing_if = "is_false")]
    pub multiply: bool,
}

/// A text's type.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct TextStyle {
    /// `tema` (the app's), `serifa`, `mao` (handwriting) or a family name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub font: String,
    /// How many ruling lines one line of text takes (1 by default).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lines: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// `centro` or `direita`; left otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub align: String,
    /// The type smaller or larger within its line (1 by default).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub size: f64,
    /// The first line starts this far in (mm); the others at the box's
    /// edge: writing begun mid-line wraps back to the column's margin.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub indent: f64,
}

impl TextStyle {
    pub fn size(&self) -> f64 {
        if self.size > 0.0 {
            self.size
        } else {
            1.0
        }
    }

    pub fn lines(&self) -> f64 {
        if self.lines > 0.0 {
            self.lines
        } else {
            1.0
        }
    }
}

/// A text that goes on in another box (on the next page): the ids of the
/// box before and the box after.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Flow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// This box starts a new paragraph (the text broke at a line's end).
    #[serde(default, skip_serializing_if = "is_false")]
    pub para: bool,
}

/// A stroke drawn by hand: points with their width (mm).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Stroke {
    pub pts: Vec<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub r: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Lies over both pages of the spread (kept on the left one).
    #[serde(default, skip_serializing_if = "is_false")]
    pub across: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Mask>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub look: Look,
    /// The item this one is attached to (a caption to its picture): it
    /// moves when that one moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Text goes round this item: `caixa` (its box) or `forma` (its cut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<Flow>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub style: TextStyle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strokes: Vec<Stroke>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pts: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Fields this version doesn't know, kept as they were.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
    /// The item's section of the note (after its `## ^j-…` line), as it is
    /// in the file; None for kinds without one, or when the section is gone.
    #[serde(skip)]
    pub body: Option<String>,
}

impl Item {
    pub fn new(kind: Kind, x: f64, y: f64, w: f64, h: f64) -> Item {
        let prefix = match kind {
            Kind::Text => "txt",
            Kind::Excerpt => "tr",
            Kind::Image | Kind::Sticker => "img",
            Kind::Scan => "scan",
            Kind::Note => "nota",
            _ => "it",
        };
        Item {
            id: format!("{prefix}{}", short_id()),
            kind,
            x,
            y,
            w,
            h,
            r: 0.0,
            locked: false,
            across: false,
            crop: None,
            flip: false,
            mask: None,
            look: Look::default(),
            anchor: None,
            wrap: None,
            flow: None,
            style: TextStyle::default(),
            strokes: Vec::new(),
            pts: Vec::new(),
            color: None,
            extra: Map::new(),
            body: None,
        }
    }

    /// Round what was measured with the pointer to what matters on paper
    /// (a hundredth of a millimetre; points to 1/10000 of the box), so the
    /// note's json stays short.
    pub fn tidy(&mut self) {
        let r = |v: &mut f64, k: f64| *v = (*v * k).round() / k;
        for v in [&mut self.x, &mut self.y, &mut self.w, &mut self.h, &mut self.r] {
            r(v, 100.0);
        }
        if let Some(c) = self.crop.as_mut() {
            for v in [&mut c.x, &mut c.y, &mut c.w, &mut c.h] {
                r(v, 10000.0);
            }
        }
        for s in self.strokes.iter_mut() {
            for q in s.pts.iter_mut() {
                r(&mut q[0], 10000.0);
                r(&mut q[1], 10000.0);
                r(&mut q[2], 1000.0);
            }
        }
        for q in self.pts.iter_mut().chain(self.mask.iter_mut().flat_map(|m| m.pts.iter_mut())) {
            r(&mut q[0], 10000.0);
            r(&mut q[1], 10000.0);
        }
        r(&mut self.look.border, 100.0);
    }

    /// The section's text without the blank lines that close it.
    pub fn content(&self) -> &str {
        self.body.as_deref().map(|b| b.trim_end_matches(['\n', '\r'])).unwrap_or("")
    }

    /// Set the section's text (it is written with a blank line after it).
    pub fn set_content(&mut self, text: &str) {
        let t = text.trim_end_matches(['\n', '\r']);
        self.body = Some(format!("{t}\n\n"));
    }

    pub fn corners(&self) -> [(f64, f64); 4] {
        crate::canvas::corners(self.x, self.y, self.w, self.h, self.r)
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        crate::canvas::contains(self.x, self.y, self.w, self.h, self.r, px, py)
    }

    pub fn bbox(&self) -> crate::canvas::Rect {
        let c = self.corners();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (x, y) in c {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        crate::canvas::Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    /// The picture a picture item shows: the target of the first embed of
    /// its section (`![[target]]` or `![](target)`), without `|size`.
    pub fn picture(&self) -> Option<String> {
        embed_target(self.content())
    }

    /// What follows the picture's embed: its caption.
    pub fn caption(&self) -> String {
        let c = self.content();
        let mut lines = c.lines();
        for l in lines.by_ref() {
            if embed_target(l).is_some() {
                break;
            }
        }
        lines.collect::<Vec<_>>().join("\n").trim().to_string()
    }

    pub fn set_picture(&mut self, target: &str, caption: &str) {
        let cap = caption.trim();
        if cap.is_empty() {
            self.set_content(&format!("![[{target}]]"));
        } else {
            self.set_content(&format!("![[{target}]]\n{cap}"));
        }
    }
}

/// The target of the first `![[…]]` (or `![…](…)`) in a text.
pub fn embed_target(text: &str) -> Option<String> {
    if let Some(s) = text.find("![[") {
        let rest = &text[s + 3..];
        let e = rest.find("]]")?;
        let t = rest[..e].split('|').next().unwrap_or("").trim();
        return (!t.is_empty()).then(|| t.to_string());
    }
    let s = text.find("![")?;
    let rest = &text[s + 2..];
    let close = rest.find("](")?;
    let rest = &rest[close + 2..];
    let e = rest.find(')')?;
    let t = rest[..e].split_whitespace().next().unwrap_or("");
    let t = percent_encoding::percent_decode_str(t).decode_utf8_lossy().to_string();
    (!t.is_empty()).then_some(t)
}

/// A short id for an item: unique enough inside a notebook, readable in the
/// block id (`^j-txt4k2f9`).
pub fn short_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mut x = t ^ N.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 29;
    const A: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    (0..6).map(|i| A[((x >> (i * 6)) % 36) as usize] as char).collect()
}

/// An excerpt's parts: the passage, the source line of the callout, and the
/// link at the end (`— [fonte](url)`), read from its section.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ExcerptParts {
    pub title: String,
    pub text: String,
    pub foot: String,
}

/// Reads a `> [!trecho] Title` callout.
pub fn excerpt_parts(section: &str) -> ExcerptParts {
    let mut out = ExcerptParts::default();
    let mut body = Vec::new();
    for (i, l) in section.lines().enumerate() {
        let q = l.strip_prefix("> ").or_else(|| l.strip_prefix('>')).unwrap_or(l);
        if i == 0 {
            if let Some(rest) = q.trim_start().strip_prefix("[!trecho]") {
                out.title = rest.trim_start_matches(['-', '+']).trim().to_string();
                continue;
            }
        }
        body.push(q.to_string());
    }
    if let Some(last) = body.last() {
        if last.trim_start().starts_with('—') {
            out.foot = last.trim_start().trim_start_matches('—').trim().to_string();
            body.pop();
        }
    }
    out.text = body.join("\n").trim().to_string();
    out
}

/// An excerpt's section: the passage quoted under its source.
pub fn excerpt_section(title: &str, text: &str, foot: &str) -> String {
    let mut s = format!("> [!trecho] {}", title.trim());
    for l in text.trim().lines() {
        if l.is_empty() {
            s.push_str("\n>");
        } else {
            s.push_str("\n> ");
            s.push_str(l);
        }
    }
    if !foot.trim().is_empty() {
        s.push_str("\n> — ");
        s.push_str(foot.trim());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_round_trip() {
        let s = excerpt_section("The Mountain Birch Project · Mossy Earth", "Montane woodland is ==rare==.\n\nSecond.", "[vídeo](https://mossy.earth)");
        let p = excerpt_parts(&s);
        assert_eq!(p.title, "The Mountain Birch Project · Mossy Earth");
        assert_eq!(p.text, "Montane woodland is ==rare==.\n\nSecond.");
        assert_eq!(p.foot, "[vídeo](https://mossy.earth)");
    }

    #[test]
    fn pictures_and_captions() {
        let mut it = Item::new(Kind::Image, 0.0, 0.0, 10.0, 10.0);
        it.set_picture("Salentin.jpg", "Hubert Salentin, 1882.");
        assert_eq!(it.picture().as_deref(), Some("Salentin.jpg"));
        assert_eq!(it.caption(), "Hubert Salentin, 1882.");
        assert_eq!(embed_target("![a](Anexos/a%20b.png)").as_deref(), Some("Anexos/a b.png"));
    }

    #[test]
    fn masks_close() {
        for (s, _) in SHAPES {
            let m = Mask { shape: s.to_string(), pts: vec![] };
            let o = m.outline(48);
            assert!(o.len() >= 4, "{s}");
            assert!(o.iter().all(|p| p.0 >= -0.01 && p.0 <= 1.01 && p.1 >= -0.01 && p.1 <= 1.01), "{s}");
        }
    }

    #[test]
    fn kinds_by_name() {
        let k: Kind = "trecho".to_string().into();
        assert_eq!(k, Kind::Excerpt);
        let o: Kind = "holograma".to_string().into();
        assert_eq!(String::from(o), "holograma");
    }
}
