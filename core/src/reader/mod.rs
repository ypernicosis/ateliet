//! The reader (Leitor): books in EPUB, documents in PDF and comics (CBZ,
//! CBR, CB7) read inside the app, as a source for the Journal — every
//! passage can be selected and kept, every region of a page cut out and
//! put on a Journal page (docs/LEITOR.md).
//!
//! - `archive` — zips read entry by entry; RAR and 7z through `bsdtar`.
//! - `xml`     — a forgiving XML/XHTML tokenizer.
//! - `epub`    — container, package, spine, navigation; chapters as blocks.
//! - `flow`    — blocks set into pages by the typesetter (Knuth–Plass,
//!   hyphenation), with the text layer.
//! - `pdf`     — pages drawn by hayro and their text layer.
//! - `comic`   — pages of a comic archive, in natural order.
//! - `marks`   — highlights kept in the book's note in the vault.
//! - `state`   — reading positions and preferences (`leitor.json`).
//!
//! The three formats sit behind [`Document`]. Pages are addressed by
//! (chapter, page): a PDF or a comic has one chapter; an EPUB is laid out
//! a chapter at a time, so a chapter's page count depends on the view.

pub mod archive;
pub mod comic;
pub mod epub;
pub mod flow;
pub mod marks;
pub mod pdf;
pub mod state;
pub mod xml;

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Epub,
    Pdf,
    Comic,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Epub => "EPUB",
            Format::Pdf => "PDF",
            Format::Comic => "HQ",
        }
    }
}

/// What the reader opens, by extension.
pub fn format_of(path: &str) -> Option<Format> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "epub" => Some(Format::Epub),
        "pdf" => Some(Format::Pdf),
        "cbz" | "cbr" | "cb7" | "cbt" => Some(Format::Comic),
        _ => None,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub title: String,
    pub author: String,
    pub lang: String,
    /// Pages turn right to left (manga, Arabic, Hebrew).
    pub rtl: bool,
    pub publisher: String,
    pub isbn: String,
}

#[derive(Clone, Debug)]
pub struct TocEntry {
    pub title: String,
    pub level: usize,
    pub chap: usize,
    /// A page of a fixed document, or the offset in an EPUB chapter.
    pub page: Option<usize>,
    pub offset: Option<usize>,
}

/// A passage quoted with a few words around it, to find it again when the
/// file changes (another edition, another EPUB of the same book).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    #[serde(rename = "exato", default)]
    pub exact: String,
    #[serde(rename = "antes", default, skip_serializing_if = "String::is_empty")]
    pub before: String,
    #[serde(rename = "depois", default, skip_serializing_if = "String::is_empty")]
    pub after: String,
}

/// A place in a book. A reading position is never a page number alone: in
/// an EPUB the page changes with the window and the font. It is the
/// chapter and the count of letters (spaces not counted) before the place.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Locator {
    /// The book in the vault.
    #[serde(rename = "arquivo", default)]
    pub file: String,
    /// EPUB: the spine item.
    #[serde(rename = "capitulo", default, skip_serializing_if = "Option::is_none")]
    pub chap: Option<usize>,
    /// EPUB: letters before the place in the chapter.
    #[serde(rename = "posicao", default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    /// PDF and comics: the page, from 1.
    #[serde(rename = "pagina", default, skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    /// A region of the page, in fractions (x0, y0, x1, y1).
    #[serde(rename = "regiao", default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<[f32; 4]>,
    #[serde(rename = "citacao", default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<Quote>,
}

impl Locator {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    pub fn from_json(v: &serde_json::Value) -> Option<Locator> {
        serde_json::from_value(v.clone()).ok().filter(|l: &Locator| !l.file.is_empty())
    }

    /// "cap. 3 · p. 58", as callouts and citations show it.
    pub fn label(&self, page_shown: Option<usize>) -> String {
        let mut parts = Vec::new();
        if let Some(c) = self.chap {
            parts.push(format!("cap. {}", c + 1));
        }
        if let Some(p) = self.page.or(page_shown) {
            parts.push(format!("p. {p}"));
        }
        parts.join(" · ")
    }
}

/// How an EPUB is set: the page's size (mm, from the window), the type
/// and the paper.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub w: f32,
    pub h: f32,
    /// Body font family ("" = the book's default serif).
    pub font: String,
    /// Body size in pt.
    pub size: f32,
    /// Line height, a multiple of the size.
    pub leading: f32,
    /// Side margin, mm.
    pub margin: f32,
    pub justify: bool,
    pub hyphenate: bool,
    pub paper: [u8; 3],
    pub ink: [u8; 3],
    pub link: [u8; 3],
}

impl Default for View {
    fn default() -> View {
        View { w: 140.0, h: 200.0, font: String::new(), size: 12.0, leading: 1.45, margin: 14.0, justify: true, hyphenate: true, paper: [255, 253, 248], ink: [32, 28, 24], link: [92, 78, 140] }
    }
}

impl View {
    /// A key for caching what depends on the view.
    pub fn key(&self) -> String {
        format!("{:.1}x{:.1}|{}|{:.2}|{:.2}|{:.1}|{}|{}|{:?}|{:?}|{:?}", self.w, self.h, self.font, self.size, self.leading, self.margin, self.justify, self.hyphenate, self.paper, self.ink, self.link)
    }
}

/// One glyph of a page's text: its text, its box in fractions of the page
/// (x0, y0, x1, y1), its line, and where it is in the chapter.
#[derive(Clone, Debug)]
pub struct Glyph {
    pub ch: String,
    pub r: [f32; 4],
    pub line: u32,
    /// EPUB: letters before it in the chapter. Elsewhere: its index.
    pub off: usize,
    /// A hyphen the layout added at a line's end (not in the book).
    pub soft: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TextLayer {
    pub glyphs: Vec<Glyph>,
}

/// Ligatures taken apart, for copying and searching.
pub fn unligate(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            '\u{ad}' => {}
            c => out.push(c),
        }
    }
    out
}

impl TextLayer {
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The text of glyphs `a..=b`: lines joined by spaces, hyphens of the
    /// layout dropped, a word hyphenated at a line's end joined again.
    pub fn text(&self, a: usize, b: usize) -> String {
        let (a, b) = (a.min(b), a.max(b));
        let mut out = String::new();
        let mut last_line = None;
        for (i, g) in self.glyphs.iter().enumerate().skip(a).take(b + 1 - a) {
            if g.soft {
                continue;
            }
            if let Some(l) = last_line {
                if l != g.line {
                    // a hard hyphen at the line's end before a lowercase letter: a word cut by the PDF's layout
                    let cut = out.ends_with('-') && !out.ends_with(" -") && g.ch.chars().next().is_some_and(|c| c.is_lowercase()) && self.glyphs.get(i.wrapping_sub(1)).is_some_and(|p| p.ch == "-" && p.line == l);
                    let after_soft = i > 0 && self.glyphs[i - 1].soft;
                    if cut {
                        out.pop();
                    } else if !after_soft && !out.ends_with(char::is_whitespace) && !g.ch.starts_with(char::is_whitespace) {
                        out.push(' ');
                    }
                }
            }
            last_line = Some(g.line);
            out.push_str(&g.ch);
        }
        let t = unligate(&out);
        t.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub fn all_text(&self) -> String {
        if self.glyphs.is_empty() {
            return String::new();
        }
        self.text(0, self.glyphs.len() - 1)
    }

    /// The glyph under a point (fractions), or the nearest on the nearest
    /// line when `near` (for drags that leave the text).
    pub fn hit(&self, x: f32, y: f32, near: bool) -> Option<usize> {
        if let Some(i) = self.glyphs.iter().position(|g| x >= g.r[0] && x <= g.r[2] && y >= g.r[1] && y <= g.r[3]) {
            return Some(i);
        }
        if !near {
            return None;
        }
        // the nearest line by its vertical span, then the nearest glyph in it
        let mut best: Option<(f32, usize)> = None;
        for (i, g) in self.glyphs.iter().enumerate() {
            let dy = if y < g.r[1] { g.r[1] - y } else if y > g.r[3] { y - g.r[3] } else { 0.0 };
            let dx = if x < g.r[0] { g.r[0] - x } else if x > g.r[2] { x - g.r[2] } else { 0.0 };
            let d = dy * 1000.0 + dx;
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|b| b.1)
    }

    fn is_word(&self, i: usize) -> bool {
        self.glyphs[i].ch.chars().any(|c| c.is_alphanumeric() || c == '\'' || c == '’' || c == '-')
    }

    /// The word around glyph `i`.
    pub fn word(&self, i: usize) -> (usize, usize) {
        if i >= self.glyphs.len() || !self.is_word(i) {
            return (i, i);
        }
        let mut a = i;
        while a > 0 && self.is_word(a - 1) && self.glyphs[a - 1].line == self.glyphs[i].line && gap_ok(&self.glyphs[a - 1], &self.glyphs[a]) {
            a -= 1;
        }
        let mut b = i;
        while b + 1 < self.glyphs.len() && self.is_word(b + 1) && self.glyphs[b + 1].line == self.glyphs[i].line && gap_ok(&self.glyphs[b], &self.glyphs[b + 1]) {
            b += 1;
        }
        (a, b)
    }

    /// The paragraph around glyph `i`: lines joined while the gap between
    /// them stays that of a line.
    pub fn para(&self, i: usize) -> (usize, usize) {
        if self.glyphs.is_empty() {
            return (0, 0);
        }
        let lines = self.lines();
        let li = lines.iter().position(|(a, b, _)| i >= *a && i <= *b).unwrap_or(0);
        let gap = |k: usize| -> f32 { lines[k + 1].2[1] - lines[k].2[3] };
        let height = |k: usize| -> f32 { lines[k].2[3] - lines[k].2[1] };
        let mut a = li;
        while a > 0 && gap(a - 1) < height(a) * 0.6 && !self.starts_para(&lines, a) {
            a -= 1;
        }
        let mut b = li;
        while b + 1 < lines.len() && gap(b) < height(b) * 0.6 && !self.starts_para(&lines, b + 1) {
            b += 1;
        }
        (lines[a].0, lines[b].1)
    }

    fn starts_para(&self, lines: &[(usize, usize, [f32; 4])], k: usize) -> bool {
        // an indented first line
        k > 0 && lines[k].2[0] > lines[k - 1].2[0] + (lines[k].2[3] - lines[k].2[1]) * 0.8
    }

    /// Lines: (first glyph, last glyph, box).
    pub fn lines(&self) -> Vec<(usize, usize, [f32; 4])> {
        let mut out: Vec<(usize, usize, [f32; 4])> = Vec::new();
        for (i, g) in self.glyphs.iter().enumerate() {
            match out.last_mut() {
                Some(l) if self.glyphs[l.1].line == g.line => {
                    l.1 = i;
                    l.2 = [l.2[0].min(g.r[0]), l.2[1].min(g.r[1]), l.2[2].max(g.r[2]), l.2[3].max(g.r[3])];
                }
                _ => out.push((i, i, g.r)),
            }
        }
        out
    }

    /// Boxes covering glyphs `a..=b`, one per line.
    pub fn rects(&self, a: usize, b: usize) -> Vec<[f32; 4]> {
        let (a, b) = (a.min(b), a.max(b));
        let mut out: Vec<(u32, [f32; 4])> = Vec::new();
        for g in self.glyphs.iter().skip(a).take(b + 1 - a) {
            match out.last_mut() {
                Some((l, r)) if *l == g.line => *r = [r[0].min(g.r[0]), r[1].min(g.r[1]), r[2].max(g.r[2]), r[3].max(g.r[3])],
                _ => out.push((g.line, g.r)),
            }
        }
        out.into_iter().map(|(_, r)| r).collect()
    }

    /// The glyphs whose centres fall in a region (fractions).
    pub fn inside(&self, r: [f32; 4]) -> Option<(usize, usize)> {
        let ins: Vec<usize> = self.glyphs.iter().enumerate().filter(|(_, g)| {
            let (cx, cy) = ((g.r[0] + g.r[2]) / 2.0, (g.r[1] + g.r[3]) / 2.0);
            cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3]
        }).map(|(i, _)| i).collect();
        Some((*ins.first()?, *ins.last()?))
    }

    /// Glyphs `a..=b` with a few words before and after, to find them again.
    pub fn quote(&self, a: usize, b: usize) -> Quote {
        let (a, b) = (a.min(b), a.max(b));
        let exact = self.text(a, b);
        let before: String = if a > 0 { self.text(a.saturating_sub(60), a - 1) } else { String::new() };
        let after: String = if b + 1 < self.glyphs.len() { self.text(b + 1, (b + 60).min(self.glyphs.len() - 1)) } else { String::new() };
        let before = before.split_whitespace().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
        let after = after.split_whitespace().take(5).collect::<Vec<_>>().join(" ");
        Quote { exact, before, after }
    }

    /// Where `needle` is on the page (case and accents ignored): glyph ranges.
    pub fn find(&self, needle: &str) -> Vec<(usize, usize)> {
        let n = fold(needle);
        if n.is_empty() {
            return Vec::new();
        }
        // the page's text with each char's glyph
        let mut hay = String::new();
        let mut owner: Vec<usize> = Vec::new();
        let mut last_line = None;
        let mut after_soft = false;
        for (i, g) in self.glyphs.iter().enumerate() {
            if g.soft {
                after_soft = true;
                continue;
            }
            let joined = std::mem::take(&mut after_soft);
            if !joined && last_line.is_some_and(|l| l != g.line) && !hay.ends_with(' ') {
                hay.push(' ');
                owner.push(i);
            }
            last_line = Some(g.line);
            if g.ch.trim().is_empty() {
                if !hay.is_empty() && !hay.ends_with(' ') {
                    hay.push(' ');
                    owner.push(i);
                }
                continue;
            }
            for c in fold(&g.ch).chars() {
                hay.push(c);
                owner.push(i);
            }
        }
        let hay_chars: Vec<char> = hay.chars().collect();
        let n_chars: Vec<char> = n.chars().collect();
        let mut out = Vec::new();
        if hay_chars.len() < n_chars.len() {
            return out;
        }
        let mut i = 0;
        while i + n_chars.len() <= hay_chars.len() {
            if hay_chars[i..i + n_chars.len()] == n_chars[..] {
                out.push((owner[i], owner[i + n_chars.len() - 1]));
                i += n_chars.len();
            } else {
                i += 1;
            }
        }
        out
    }
}

fn gap_ok(a: &Glyph, b: &Glyph) -> bool {
    b.r[0] - a.r[2] < (a.r[3] - a.r[1]) * 0.25
}

/// Lower case without accents, spaces collapsed: for searching.
pub fn fold(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let t: String = unligate(s).nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase();
    let mut out = String::with_capacity(t.len());
    let mut space = false;
    for c in t.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    out
}

/// A link on a page.
#[derive(Clone, Debug)]
pub struct Link {
    pub r: [f32; 4],
    pub target: Target,
    /// A note reference (`epub:type="noteref"`, or a number linking into
    /// the book): shown in a balloon rather than followed.
    pub note: bool,
}

#[derive(Clone, Debug)]
pub enum Target {
    /// A chapter and a place in it.
    At { chap: usize, offset: Option<usize>, page: Option<usize> },
    Url(String),
}

/// A picture of a page, its size in pixels and straight RGBA.
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Rgba {
    /// A region (fractions) cut out.
    pub fn crop(&self, r: [f32; 4]) -> Rgba {
        let x0 = ((r[0].clamp(0.0, 1.0) * self.w as f32) as u32).min(self.w.saturating_sub(1));
        let y0 = ((r[1].clamp(0.0, 1.0) * self.h as f32) as u32).min(self.h.saturating_sub(1));
        let x1 = ((r[2].clamp(0.0, 1.0) * self.w as f32).ceil() as u32).clamp(x0 + 1, self.w);
        let y1 = ((r[3].clamp(0.0, 1.0) * self.h as f32).ceil() as u32).clamp(y0 + 1, self.h);
        let (w, h) = (x1 - x0, y1 - y0);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y1 {
            let s = ((y * self.w + x0) * 4) as usize;
            px.extend_from_slice(&self.px[s..s + (w * 4) as usize]);
        }
        Rgba { w, h, px }
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        let img = image::RgbaImage::from_raw(self.w, self.h, self.px.clone()).ok_or("imagem inválida")?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    }
}

/// A hit of a search across the book.
#[derive(Clone, Debug)]
pub struct Hit {
    pub chap: usize,
    pub page: Option<usize>,
    pub offset: Option<usize>,
    /// The words around it.
    pub context: String,
}

/// A book open for reading. The text-flowing format (EPUB) lays out a
/// chapter for a view; the fixed ones ignore the view.
pub trait Document {
    fn format(&self) -> Format;
    fn meta(&self) -> &Meta;
    fn toc(&self) -> &[TocEntry];
    /// Chapters (1 for a PDF or a comic).
    fn chapters(&self) -> usize;
    /// Does the text flow (EPUB), or are the pages fixed?
    fn reflows(&self) -> bool {
        false
    }
    /// The chapter's pages for the view (laid out now if needed).
    fn pages(&mut self, chap: usize, view: &View) -> usize;
    /// A page's size (any unit): its proportions.
    fn size(&mut self, chap: usize, page: usize, view: &View) -> (f32, f32);
    /// A page drawn `px_w` pixels wide (a comic page at most its own size).
    fn render(&mut self, chap: usize, page: usize, view: &View, px_w: u32) -> Result<Rgba, String>;
    /// The page's text (None for pictures without text).
    fn text(&mut self, chap: usize, page: usize, view: &View) -> Option<TextLayer>;
    fn links(&mut self, _chap: usize, _page: usize, _view: &View) -> Vec<Link> {
        Vec::new()
    }
    /// The book's pictures on a page: where, and their name in the book.
    fn images(&mut self, _chap: usize, _page: usize, _view: &View) -> Vec<([f32; 4], String)> {
        Vec::new()
    }
    /// A file inside the book (a picture).
    fn resource(&self, _name: &str) -> Option<Vec<u8>> {
        None
    }
    /// A note's text, for a note reference.
    fn note_text(&mut self, _chap: usize, _offset: usize) -> Option<String> {
        None
    }
    /// The page a locator points to.
    fn locate(&mut self, loc: &Locator, view: &View) -> (usize, usize);
    /// A locator for the start of a page (without the file).
    fn locator(&mut self, chap: usize, page: usize, view: &View) -> Locator;
    /// The place of a glyph of a page.
    fn locator_at(&mut self, chap: usize, page: usize, view: &View, glyph: &Glyph) -> Locator {
        let _ = glyph;
        self.locator(chap, page, view)
    }
    fn search(&mut self, query: &str, view: &View) -> Vec<Hit>;
    /// The cover, `px_w` wide.
    fn cover(&mut self, px_w: u32) -> Option<Rgba>;
}

/// A file's identity for the reading state: a hash of its start and its
/// size, so a position survives renaming or moving the file.
pub fn fingerprint(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut h = Sha256::new();
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    h.update(len.to_le_bytes());
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = vec![0u8; 64 * 1024];
        let n = f.read(&mut buf).unwrap_or(0);
        h.update(&buf[..n]);
    }
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Opens a book: the format by the file's first bytes, then by its name.
pub fn open(path: &Path, fonts: std::sync::Arc<crate::typeset::fonts::FontDb>) -> Result<Box<dyn Document>, String> {
    let name = path.to_string_lossy().to_string();
    let m = archive::magic(path);
    let fmt = match (m, format_of(&name)) {
        (archive::Magic::Pdf, _) => Format::Pdf,
        (archive::Magic::Zip, Some(Format::Epub)) => Format::Epub,
        (archive::Magic::Zip, _) => {
            // a zip with a package is an EPUB whatever its name
            let z = archive::Zip::open(path)?;
            if z.has("META-INF/container.xml") {
                Format::Epub
            } else {
                Format::Comic
            }
        }
        (archive::Magic::Rar | archive::Magic::SevenZip | archive::Magic::Tar, _) => Format::Comic,
        (_, Some(f)) => f,
        _ => return Err("formato desconhecido".into()),
    };
    match fmt {
        Format::Pdf => Ok(Box::new(pdf::PdfBook::open(path)?)),
        Format::Epub => Ok(Box::new(epub::Epub::open(path, fonts)?)),
        Format::Comic => Ok(Box::new(comic::Comic::open(path)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(ch: &str, x: f32, line: u32) -> Glyph {
        let y = line as f32 * 0.05;
        Glyph { ch: ch.into(), r: [x, y, x + 0.01, y + 0.03], line, off: 0, soft: false }
    }

    #[test]
    fn text_of_a_selection() {
        let mut gl = Vec::new();
        for (i, c) in "pala".chars().enumerate() {
            gl.push(g(&c.to_string(), 0.1 + i as f32 * 0.01, 0));
        }
        let mut h = g("-", 0.14, 0);
        h.soft = true;
        gl.push(h);
        for (i, c) in "vra ﬁm".chars().enumerate() {
            gl.push(g(&c.to_string(), 0.1 + i as f32 * 0.01, 1));
        }
        let t = TextLayer { glyphs: gl };
        assert_eq!(t.all_text(), "palavra fim");
        assert_eq!(t.find("PALAVRA"), vec![(0, 7)]);
        assert_eq!(t.find("vra fim"), vec![(5, 10)]);
        assert_eq!(fold("Coração  Ação"), "coracao acao");
        let l = Locator { file: "Livros/a.epub".into(), chap: Some(2), offset: Some(10), ..Default::default() };
        assert_eq!(Locator::from_json(&l.to_json()), Some(l));
    }
}
