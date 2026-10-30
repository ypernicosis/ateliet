//! The text of an EPUB chapter as blocks, set into pages by the
//! typesetter of the Diagramação: lines broken by Knuth and Plass, words
//! hyphenated in the book's language, the page's measure and height those
//! of the window. Each page keeps its items (to draw), its text layer (to
//! select), its links and its pictures.
//!
//! Places in a chapter are counted in letters (whitespace not counted):
//! the same count whatever the font and the window, so a position, a
//! highlight or an excerpt finds its page again after a new layout.

use super::{Glyph, View};
use crate::typeset::fonts::FontDb;
use crate::typeset::lines::{self, Ctx, Faces, Set, K};
use crate::typeset::model::{Run, Style};
use crate::typeset::{Item, Page, PageKind, Picture, PT_MM};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Para,
    Head(u8),
    Pre,
    Caption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Justify,
    Left,
    Center,
    Right,
}

/// Inline text in one style, maybe a link (an index into the chapter's links).
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub st: Style,
    pub link: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct TextBlock {
    pub spans: Vec<Span>,
    pub kind: Kind,
    pub align: Option<Align>,
    /// First-line indent in em (None: the reader's default).
    pub indent: Option<f32>,
    /// Space before and after, in em.
    pub before: f32,
    pub after: f32,
    /// Left and right insets in em (quotes, lists).
    pub left: f32,
    pub right: f32,
    /// A list item's marker.
    pub bullet: Option<String>,
    pub break_before: bool,
    /// Letters before the block in the chapter.
    pub off: usize,
    /// It goes on a paragraph cut by a picture (no indent).
    pub cont: bool,
}

#[derive(Clone, Debug)]
pub enum Block {
    Text(TextBlock),
    Image { src: String, alt: String, off: usize, break_before: bool, before: f32, after: f32 },
    Rule { off: usize },
}

impl Block {
    pub fn off(&self) -> usize {
        match self {
            Block::Text(t) => t.off,
            Block::Image { off, .. } | Block::Rule { off } => *off,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LinkSpec {
    /// `path#fragment` inside the book, or a URL.
    pub href: String,
    pub note: bool,
}

/// A chapter read: its blocks, anchors (id → letters before), links (and
/// their ranges of letters), the notes kept out of the text.
#[derive(Clone, Debug, Default)]
pub struct Chapter {
    pub blocks: Vec<Block>,
    pub anchors: HashMap<String, usize>,
    pub links: Vec<LinkSpec>,
    pub link_ranges: Vec<(usize, usize, usize)>,
    pub notes: HashMap<String, String>,
    pub len: usize,
    pub title: String,
}

/// Letters in a string (whitespace not counted).
pub fn letters(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace() && *c != '\u{ad}').count()
}

impl Chapter {
    /// Each text block's plain text with the letters before it.
    pub fn texts(&self) -> Vec<(usize, String)> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Text(t) => Some((t.off, t.spans.iter().map(|s| s.text.as_str()).collect::<String>())),
            _ => None,
        }).collect()
    }

    /// The text of the block holding a place.
    pub fn block_text_at(&self, off: usize) -> Option<String> {
        let texts = self.texts();
        let i = texts.iter().rposition(|(o, _)| *o <= off)?;
        Some(texts[i].1.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    /// Where a passage is (case and accents ignored), in letters.
    pub fn find(&self, needle: &str) -> Vec<usize> {
        let n = super::fold(needle);
        if n.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (off, text) in self.texts() {
            let hay = super::fold(&text);
            let mut from = 0;
            while let Some(i) = hay[from..].find(&n) {
                let at = from + i;
                out.push(off + letters(&hay[..at]));
                from = at + n.len().max(1);
            }
        }
        out
    }

    /// A few words around a place.
    pub fn context(&self, off: usize, len: usize) -> String {
        let Some(t) = self.block_text_at(off) else { return String::new() };
        let texts = self.texts();
        let start = texts.iter().rfind(|(o, _)| *o <= off).map(|x| x.0).unwrap_or(0);
        // the char index of the place inside the block
        let mut count = start;
        let mut at = 0;
        for (i, c) in t.char_indices() {
            if count >= off {
                at = i;
                break;
            }
            if !c.is_whitespace() {
                count += 1;
            }
            at = i;
        }
        let chars: Vec<char> = t.chars().collect();
        let ci = t[..at].chars().count();
        let a = ci.saturating_sub(50);
        let b = (ci + len + 50).min(chars.len());
        let mut s: String = chars[a..b].iter().collect();
        if a > 0 {
            s = format!("…{s}");
        }
        if b < chars.len() {
            s.push('…');
        }
        s
    }
}

/// One laid-out page.
#[derive(Clone, Debug, Default)]
pub struct LPage {
    pub items: Vec<Item>,
    pub glyphs: Vec<Glyph>,
    /// Link boxes (mm) and the link index.
    pub links: Vec<([f32; 4], usize)>,
    /// Pictures (mm boxes) and their names in the book.
    pub images: Vec<([f32; 4], String)>,
    /// Letters before the page's start.
    pub start: usize,
}

pub struct Laid {
    pub w: f32,
    pub h: f32,
    pub pages: Vec<LPage>,
    pub pics: Vec<Picture>,
    pub paper: [u8; 3],
}

impl Laid {
    /// The page holding a place.
    pub fn page_of(&self, off: usize) -> usize {
        self.pages.iter().rposition(|p| p.start <= off).unwrap_or(0)
    }

    pub fn page(&self, n: usize) -> Page {
        let mut p = Page::new(self.w, self.h, n + 1, PageKind::Flow, self.paper);
        if let Some(lp) = self.pages.get(n) {
            p.items = lp.items.clone();
        }
        p
    }
}

struct Lay<'a> {
    cx: Ctx<'a>,
    view: &'a View,
    pages: Vec<LPage>,
    pics: Vec<Picture>,
    y: f32,
    top: f32,
    bottom: f32,
    x0: f32,
    measure: f32,
    line_no: u32,
    /// Space owed before the next thing (gone at a page's top).
    pending: f32,
}

impl Lay<'_> {
    fn page(&mut self) -> &mut LPage {
        self.pages.last_mut().unwrap()
    }

    fn new_page(&mut self, start: usize) {
        self.pages.push(LPage { start, ..Default::default() });
        self.y = self.top;
        self.pending = 0.0;
    }

    fn at_top(&self) -> bool {
        (self.y - self.top).abs() < 0.01
    }

    /// Ascent and descent (mm) of the body face at a size in pt.
    fn metrics(&self, font: usize, size: f32) -> (f32, f32) {
        let f = self.cx.db.face(font);
        (f.ascender / f.upem * size * PT_MM, -f.descender / f.upem * size * PT_MM)
    }

    fn text(&mut self, b: &TextBlock, links: &[(usize, usize, usize)], first_in_chapter: bool) {
        let em = self.cx.size * PT_MM;
        let (scale, title) = match b.kind {
            Kind::Head(l) => ([1.0, 1.7, 1.4, 1.22, 1.1, 1.0, 1.0][l.min(6) as usize], true),
            Kind::Caption => (0.88, false),
            _ => (1.0, false),
        };
        if b.break_before && !self.at_top() && !first_in_chapter {
            self.new_page(b.off);
        }
        let before = match b.kind {
            Kind::Head(l) if l <= 2 => (if first_in_chapter { 2.5_f32 } else { 1.6 }).max(b.before),
            Kind::Head(_) => 1.2_f32.max(b.before),
            _ => b.before,
        } * em;
        let after = match b.kind {
            Kind::Head(l) if l <= 2 => 1.1_f32.max(b.after),
            Kind::Head(_) => 0.5_f32.max(b.after),
            _ => b.after,
        } * em;
        self.pending = self.pending.max(before);
        let runs: Vec<Run> = b.spans.iter().map(|s| {
            let mut st = s.st.clone();
            if st.scale == 0.0 {
                st.scale = 1.0;
            }
            if st.opacity == 0.0 {
                st.opacity = 1.0;
            }
            st.scale *= scale;
            st.title_font |= title;
            if s.link.is_some() {
                st.color = Some(self.view.link);
            }
            Run { text: s.text.clone(), st }
        }).collect();
        if runs.iter().all(|r| r.text.trim().is_empty()) {
            self.pending = self.pending.max(after);
            return;
        }
        let left = b.left * em + if b.bullet.is_some() { 1.4 * em } else { 0.0 };
        let right = b.right * em;
        let measure = (self.measure - left - right).max(em * 6.0);
        let align = b.align.unwrap_or(match b.kind {
            Kind::Head(l) if l <= 2 => Align::Center,
            Kind::Head(_) => Align::Left,
            Kind::Pre => Align::Left,
            Kind::Caption => Align::Center,
            Kind::Para => if self.view.justify { Align::Justify } else { Align::Left },
        });
        let align = if align == Align::Justify && !self.view.justify { Align::Left } else { align };
        let indent = match (b.kind, b.indent) {
            (Kind::Para, Some(i)) => i * em,
            (Kind::Para, None) if !b.cont && b.before < 0.3 && matches!(align, Align::Justify | Align::Left) => 1.3 * em,
            _ => 0.0,
        };
        let hyph = self.view.hyphenate && matches!(b.kind, Kind::Para) && align != Align::Center;
        let items = lines::items(&self.cx, &runs, hyph);
        let width = |n: usize| if n == 0 { measure - indent.max(0.0) } else { measure };
        let brk = lines::breaks(&items, &width, align == Align::Justify);
        let how = match align {
            Align::Justify => Set::Justify,
            Align::Left => Set::Left,
            Align::Center => Set::Center,
            Align::Right => Set::Right,
        };
        let set = lines::set(&items, &brk, &width, how);
        let lh = self.cx.size * PT_MM * self.view.leading * scale.max(1.0) * if title { 0.92 } else { 1.0 };
        let body = self.cx.faces.pick(&Style::plain());
        let mut off = b.off;
        for (ln, line) in set.iter().enumerate() {
            // a heading keeps its first line with the next text
            let need = if matches!(b.kind, Kind::Head(_)) && ln == 0 { lh * 3.0 } else { lh };
            if self.y + self.pending + need > self.bottom && !self.at_top() {
                self.new_page(off);
            }
            if !self.at_top() {
                self.y += self.pending;
            }
            self.pending = 0.0;
            // the baseline in the line box
            let (asc, desc) = self.metrics(body, self.cx.size * scale);
            let base = self.y + (lh - asc - desc) / 2.0 + asc;
            let lx = self.x0 + left + if ln == 0 { indent.max(0.0) } else { 0.0 };
            if ln == 0 {
                if let Some(bul) = &b.bullet {
                    let p = self.cx.piece(bul, &Style::plain());
                    let bx = self.x0 + b.left * em + (1.4 * em - p.w - 0.4 * em).max(0.0);
                    self.page().items.push(Item::Text { font: p.font, size: p.size, x: bx, y: base, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map });
                }
            }
            let soft_end = matches!(items.get(brk[ln]), Some(K::Pen { hyphen: Some(_), .. }));
            let line_no = self.line_no;
            self.line_no += 1;
            let mut new_glyphs: Vec<Glyph> = Vec::new();
            let n_text: usize = line.items.iter().filter(|i| matches!(i, Item::Text { .. })).count();
            let mut text_i = 0;
            for it in &line.items {
                let placed = crate::typeset::layout::shift(it, lx, base);
                if let Item::Text { font, size, x, y, glyphs, map, .. } = &placed {
                    text_i += 1;
                    let (a, d) = self.metrics(*font, *size);
                    for (j, g) in glyphs.iter().enumerate() {
                        let gx = x + g.x;
                        let s = map.get(j).map(|m| m.1.clone()).unwrap_or_default();
                        // to the next glyph, but not over the space after a word
                        let own = if s.is_empty() { 0.0 } else { self.cx.db.width(*font, &s, *size, &[]) };
                        let gx1 = match glyphs.get(j + 1) {
                            Some(n) if own > 0.0 => (x + n.x).min(gx + own * 1.05),
                            Some(n) => x + n.x,
                            None => gx + own,
                        };
                        let last = text_i == n_text && j + 1 == glyphs.len();
                        let soft = soft_end && last && s == "-";
                        new_glyphs.push(Glyph { ch: s.clone(), r: [gx, y - a, gx1.max(gx + 0.05), y + d], line: line_no, off, soft });
                        if !soft {
                            off += letters(&s);
                        }
                    }
                }
                self.page().items.push(placed);
            }
            // spaces between words are not glyphs of the layout: put them in the text layer
            let mut with_spaces: Vec<Glyph> = Vec::with_capacity(new_glyphs.len() + 8);
            for g in new_glyphs {
                if let Some(p) = with_spaces.last() {
                    if g.r[0] - p.r[2] > em * 0.12 && !p.ch.ends_with(' ') {
                        with_spaces.push(Glyph { ch: " ".into(), r: [p.r[2], p.r[1], g.r[0], p.r[3]], line: line_no, off: p.off + letters(&p.ch), soft: false });
                    }
                }
                with_spaces.push(g);
            }
            for g in &with_spaces {
                if let Some((_, _, li)) = links.iter().find(|(a, e, _)| g.off >= *a && g.off < *e && !g.ch.trim().is_empty()) {
                    let pg = self.page();
                    match pg.links.last_mut() {
                        Some((r, l)) if *l == *li && (r[1] - g.r[1]).abs() < 0.1 => *r = [r[0].min(g.r[0]), r[1].min(g.r[1]), r[2].max(g.r[2]), r[3].max(g.r[3])],
                        _ => pg.links.push((g.r, *li)),
                    }
                }
            }
            self.page().glyphs.extend(with_spaces);
            self.y += lh;
        }
        self.pending = after;
    }

    fn image(&mut self, src: &str, off: usize, pic: Option<Picture>, break_before: bool, before: f32, after: f32) {
        let Some(pic) = pic else { return };
        let em = self.cx.size * PT_MM;
        if break_before && !self.at_top() {
            self.new_page(off);
        }
        self.pending = self.pending.max(before * em);
        let avail_h = self.bottom - self.top;
        // natural size at 96 ppi, never wider than the measure nor taller than the page
        let (nw, nh) = (pic.w as f32 * 0.2646, pic.h as f32 * 0.2646);
        let full = pic.w > 500 || pic.h > 700;
        let mut w = if full { self.measure } else { nw.min(self.measure) };
        let mut h = w * pic.h as f32 / pic.w.max(1) as f32;
        if !full && h > nh {
            h = nh;
            w = h * pic.w as f32 / pic.h.max(1) as f32;
        }
        if h > avail_h {
            h = avail_h;
            w = h * pic.w as f32 / pic.h.max(1) as f32;
        }
        let room = self.bottom - self.y - self.pending;
        if h > room && !self.at_top() {
            if room > avail_h * 0.5 && room > h * 0.75 {
                let k = room / h;
                h *= k;
                w *= k;
            } else {
                self.new_page(off);
            }
        }
        if !self.at_top() {
            self.y += self.pending;
        }
        self.pending = 0.0;
        let x = self.x0 + (self.measure - w) / 2.0;
        let idx = self.pics.len();
        self.pics.push(pic);
        let y = self.y;
        let pg = self.page();
        pg.items.push(Item::Image { pic: idx, x, y, w, h });
        pg.images.push(([x, y, x + w, y + h], src.to_string()));
        self.y += h;
        self.pending = (after * em).max(em * 0.6);
    }

    fn rule(&mut self, off: usize) {
        let em = self.cx.size * PT_MM;
        if self.y + self.pending + em * 2.0 > self.bottom {
            self.new_page(off);
            return;
        }
        self.y += self.pending.max(em * 0.8);
        let w = self.measure * 0.25;
        let x = self.x0 + (self.measure - w) / 2.0;
        let c = self.cx.color_of(&Style { opacity: 0.45, ..Style::plain() });
        let y = self.y;
        self.page().items.push(Item::Line { x0: x, y0: y, x1: x + w, y1: y, width: 0.25, color: c, dash: None });
        self.pending = em * 0.8;
    }
}

/// A chapter set for a view. `picture` decodes a picture of the book.
pub fn layout(ch: &Chapter, view: &View, db: &FontDb, faces: Faces, picture: &dyn Fn(&str) -> Option<Picture>) -> Laid {
    let cx = Ctx { db, faces, size: view.size, color: view.ink, paper: view.paper, hyphenate: view.hyphenate, figures: None };
    let em = view.size * PT_MM;
    // a measure of about 66 characters, centred
    let measure = (view.w - 2.0 * view.margin).min(em * 34.0).max(em * 8.0);
    let x0 = (view.w - measure) / 2.0;
    let top = (view.margin * 0.8).max(em * 1.5);
    let bottom = view.h - (view.margin * 0.8).max(em * 1.8);
    let mut l = Lay { cx, view, pages: Vec::new(), pics: Vec::new(), y: top, top, bottom, x0, measure, line_no: 0, pending: 0.0 };
    l.new_page(0);
    let mut first = true;
    for b in &ch.blocks {
        match b {
            Block::Text(t) => {
                l.text(t, &ch.link_ranges, first);
                first = false;
            }
            Block::Image { src, off, break_before, before, after, .. } => {
                let pic = picture(src);
                l.image(src, *off, pic, *break_before, *before, *after);
                first = false;
            }
            Block::Rule { off } => l.rule(*off),
        }
    }
    // a page left empty at the end
    while l.pages.len() > 1 && l.pages.last().is_some_and(|p| p.items.is_empty()) {
        l.pages.pop();
    }
    // the text layer and the boxes in fractions of the page
    let (w, h) = (view.w, view.h);
    for p in &mut l.pages {
        for g in &mut p.glyphs {
            g.r = [g.r[0] / w, g.r[1] / h, g.r[2] / w, g.r[3] / h];
        }
        for (r, _) in &mut p.links {
            *r = [r[0] / w, r[1] / h, r[2] / w, r[3] / h];
        }
        for (r, _) in &mut p.images {
            *r = [r[0] / w, r[1] / h, r[2] / w, r[3] / h];
        }
        if let Some(g) = p.glyphs.first() {
            p.start = g.off;
        }
    }
    if let Some(p0) = l.pages.first_mut() {
        p0.start = 0;
    }
    // a page's start never goes back
    for i in 1..l.pages.len() {
        if l.pages[i].start < l.pages[i - 1].start {
            l.pages[i].start = l.pages[i - 1].start;
        }
    }
    Laid { w, h, pages: l.pages, pics: l.pics, paper: view.paper }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::testfonts::BUNDLED;

    fn para(text: &str, off: usize) -> Block {
        Block::Text(TextBlock { spans: vec![Span { text: text.into(), st: Style::plain(), link: None }], kind: Kind::Para, align: None, indent: None, before: 0.0, after: 0.0, left: 0.0, right: 0.0, bullet: None, break_before: false, off, cont: false })
    }

    #[test]
    fn pages_and_places() {
        let db = FontDb::new(&BUNDLED);
        let faces = crate::typeset::layout::faces(&db, &serde_json::json!({}));
        let text = "Era uma vez uma torre de vidro onde morava uma tecelã que fiava a luz do sol. ".repeat(12);
        let mut ch = Chapter::default();
        let mut off = 0;
        for _ in 0..12 {
            ch.blocks.push(para(&text, off));
            off += letters(&text);
        }
        ch.len = off;
        let view = View { w: 100.0, h: 140.0, ..View::default() };
        let laid = layout(&ch, &view, &db, faces, &|_| None);
        assert!(laid.pages.len() > 3, "{} pages", laid.pages.len());
        // places grow page by page, and the last letter is on the last page
        let starts: Vec<usize> = laid.pages.iter().map(|p| p.start).collect();
        assert!(starts.windows(2).all(|w| w[0] < w[1]), "{starts:?}");
        assert_eq!(laid.page_of(off - 1), laid.pages.len() - 1);
        let g = &laid.pages[1].glyphs;
        assert!(g.iter().all(|g| g.r[0] >= 0.0 && g.r[2] <= 1.0));
        // the text of a page reads back without the layout's hyphens
        let t = super::super::TextLayer { glyphs: g.clone() };
        let s = t.all_text();
        assert!(s.contains("torre de vidro"), "{s}");
        assert!(!s.contains("- "), "{s}");
    }
}
