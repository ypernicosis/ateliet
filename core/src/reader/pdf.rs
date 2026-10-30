//! PDF: pages drawn by hayro; the text layer from a device that walks the
//! page without drawing and notes each glyph (its character and its box);
//! the contents from the document's outline; links from the annotations.
//! A scanned page has no text: it reads and cuts like a picture.

use super::{Document, Format, Glyph, Hit, Link, Locator, Meta, Rgba, Target, TextLayer, TocEntry, View};
use hayro::hayro_interpret::font::{Glyph as PdfGlyph, GlyphRun};
use hayro::hayro_interpret::{self as hi, ClipPath, Device, DrawMode, DrawProps, InterpreterSettings, RectExt, SoftMask};
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, ObjRef};
use hayro::hayro_syntax::Pdf;
use hayro::kurbo::{BezPath, Point, Rect, Shape};
use std::collections::HashMap;
use std::path::Path;

pub struct PdfBook {
    pdf: Pdf,
    meta: Meta,
    toc: Vec<TocEntry>,
    texts: HashMap<usize, TextLayer>,
    /// Page objects in order, to read where the outline and links point.
    refs: Vec<ObjRef>,
}

/// A PDF text string: UTF-16 with its mark, or PDFDocEncoding (read as Latin-1).
pub fn text_string(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xfe && b[1] == 0xff {
        let u: Vec<u16> = b[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&u);
    }
    if b.starts_with(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(&b[3..]).into_owned();
    }
    b.iter().map(|&c| c as char).collect()
}

fn page_refs(xref: &hayro::hayro_syntax::xref::XRef, node: &Dict, out: &mut Vec<ObjRef>, depth: usize) {
    if depth > 64 {
        return;
    }
    let Some(MaybeRef::NotRef(kids)) = node.get_raw::<Array>(b"Kids") else {
        if let Some(MaybeRef::Ref(r)) = node.get_raw::<Array>(b"Kids") {
            if let Some(a) = xref.get::<Array>(ObjectIdentifier::from(r)) {
                kids_of(xref, &a, out, depth);
            }
        }
        return;
    };
    kids_of(xref, &kids, out, depth);
}

fn kids_of(xref: &hayro::hayro_syntax::xref::XRef, kids: &Array, out: &mut Vec<ObjRef>, depth: usize) {
    for k in kids.raw_iter() {
        let MaybeRef::Ref(r) = k else { continue };
        let Some(d) = xref.get::<Dict>(ObjectIdentifier::from(r)) else { continue };
        let is_tree = d.get::<Name>(b"Type").is_some_and(|t| &*t == b"Pages") || d.contains_key(b"Kids");
        if is_tree {
            page_refs(xref, &d, out, depth + 1);
        } else {
            out.push(r);
        }
    }
}

impl PdfBook {
    pub fn open(path: &Path) -> Result<PdfBook, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let pdf = Pdf::new(bytes).map_err(|e| match format!("{e:?}") {
            s if s.contains("Encrypt") || s.contains("Password") => "o PDF é protegido por senha".to_string(),
            s => format!("não foi possível ler o PDF ({s})"),
        })?;
        if pdf.pages().is_empty() {
            return Err("o PDF não tem páginas".into());
        }
        let md = pdf.metadata();
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let meta = Meta {
            title: md.title.as_deref().map(text_string).map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("untitled") && !s.ends_with(".doc") && !s.ends_with(".docx")).unwrap_or(stem),
            author: md.author.as_deref().map(text_string).map(|s| s.trim().to_string()).unwrap_or_default(),
            ..Default::default()
        };
        let mut b = PdfBook { pdf, meta, toc: Vec::new(), texts: HashMap::new(), refs: Vec::new() };
        let xref = b.pdf.xref();
        if let Some(root) = xref.get::<Dict>(xref.root_id()) {
            if let Some(pages) = root.get::<Dict>(b"Pages") {
                let mut refs = Vec::new();
                page_refs(xref, &pages, &mut refs, 0);
                b.refs = refs;
            }
            let mut toc = Vec::new();
            if let Some(first) = root.get::<Dict>(b"Outlines").and_then(|o| o.get::<Dict>(b"First")) {
                let mut budget = 5000;
                b.outline(&root, first, 0, &mut toc, &mut budget);
            }
            b.toc = toc;
        }
        Ok(b)
    }

    fn page_of(&self, r: ObjRef) -> Option<usize> {
        self.refs.iter().position(|x| x.obj_number == r.obj_number && x.gen_number == r.gen_number)
    }

    /// The page a destination points to (an array, a name or a string).
    fn dest_page(&self, root: &Dict, d: &Object, depth: usize) -> Option<usize> {
        if depth > 4 {
            return None;
        }
        match d {
            Object::Array(a) => match a.raw_iter().next()? {
                MaybeRef::Ref(r) => self.page_of(r),
                MaybeRef::NotRef(Object::Number(n)) => Some(n.as_i64().max(0) as usize),
                _ => None,
            },
            Object::Dict(dd) => self.dest_page(root, &dd.get::<Object>(b"D")?, depth + 1),
            Object::Name(n) => self.named(root, n.as_ref(), depth),
            Object::String(s) => self.named(root, s.as_bytes(), depth),
            _ => None,
        }
    }

    fn named(&self, root: &Dict, key: &[u8], depth: usize) -> Option<usize> {
        if let Some(o) = root.get::<Dict>(b"Dests").and_then(|d| d.get::<Object>(key)) {
            return self.dest_page(root, &o, depth + 1);
        }
        let tree = root.get::<Dict>(b"Names")?.get::<Dict>(b"Dests")?;
        let o = name_tree(&tree, key, 0)?;
        self.dest_page(root, &o, depth + 1)
    }

    fn outline(&self, root: &Dict, mut item: Dict, level: usize, out: &mut Vec<TocEntry>, budget: &mut usize) {
        loop {
            if *budget == 0 || level > 12 {
                return;
            }
            *budget -= 1;
            let title = item.get::<hayro::hayro_syntax::object::String>(b"Title").map(|s| text_string(s.as_bytes())).unwrap_or_default();
            let dest = item.get::<Object>(b"Dest").or_else(|| item.get::<Dict>(b"A").and_then(|a| a.get::<Object>(b"D")));
            let page = dest.and_then(|d| self.dest_page(root, &d, 0));
            out.push(TocEntry { title: title.split_whitespace().collect::<Vec<_>>().join(" "), level, chap: 0, page, offset: None });
            if let Some(kid) = item.get::<Dict>(b"First") {
                self.outline(root, kid, level + 1, out, budget);
            }
            match item.get::<Dict>(b"Next") {
                Some(n) => item = n,
                None => return,
            }
        }
    }

    fn layer(&mut self, page: usize) -> TextLayer {
        if let Some(t) = self.texts.get(&page) {
            return t.clone();
        }
        let t = self.extract(page);
        self.texts.insert(page, t.clone());
        t
    }

    fn extract(&self, page: usize) -> TextLayer {
        let pages = self.pdf.pages();
        let Some(p) = pages.get(page) else { return TextLayer::default() };
        let (pw, ph) = p.render_dimensions();
        let transform = hi::TransformExt::to_kurbo(&p.initial_transform(true));
        let cache = hi::InterpreterCache::new();
        let mut settings = InterpreterSettings::default();
        settings.render_annotations = false;
        let mut ctx = hi::Context::new(transform, Rect::new(0.0, 0.0, pw as f64, ph as f64), &cache, p.xref(), settings);
        let mut dev = TextDevice { raw: Vec::new() };
        hi::interpret_page(p, &mut ctx, &mut dev);
        build_layer(dev.raw, pw.max(1.0), ph.max(1.0))
    }
}

fn name_tree<'a>(node: &Dict<'a>, key: &[u8], depth: usize) -> Option<Object<'a>> {
    if depth > 16 {
        return None;
    }
    if let Some(names) = node.get::<Array>(b"Names") {
        let mut it = names.iter::<Object>();
        while let Some(k) = it.next() {
            let v = it.next()?;
            let kb: Vec<u8> = match &k {
                Object::String(s) => s.as_bytes().to_vec(),
                Object::Name(n) => n.as_ref().to_vec(),
                _ => continue,
            };
            if kb == key {
                return Some(v);
            }
        }
    }
    if let Some(kids) = node.get::<Array>(b"Kids") {
        for k in kids.iter::<Dict>() {
            if let Some(v) = name_tree(&k, key, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

/// A glyph as the device saw it: its text, origin, box (in page points).
struct RawGlyph {
    ch: String,
    origin: Point,
    /// The glyph's own x axis (unit) on the page and its height.
    axis: (f64, f64),
    h: f64,
    /// Its width along the baseline.
    adv: f64,
    r: Rect,
}

struct TextDevice {
    raw: Vec<RawGlyph>,
}

impl<'a> Device<'a> for TextDevice {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: hi::BlendMode) {}
    fn draw_image(&mut self, _: hi::Image<'a, '_>, _: hi::ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        let gs = run.glyphs();
        for (i, g) in gs.iter().enumerate() {
            let ch = match g.as_unicode() {
                Some(hi::hayro_cmap::BfString::Char(c)) => c.to_string(),
                Some(hi::hayro_cmap::BfString::String(s)) => s,
                None => continue,
            };
            if ch.is_empty() || ch.chars().all(|c| c.is_control()) {
                continue;
            }
            let t = props.transform * g.transform();
            let origin = t * Point::ZERO;
            // the width: to the next glyph's origin, or the outline's extent
            let next = gs.get(i + 1).map(|n| props.transform * n.transform() * Point::ZERO);
            let x_axis = t * Point::new(1000.0, 0.0) - origin;
            let unit = x_axis.hypot().max(1e-6);
            let axis = (x_axis.x / unit, x_axis.y / unit);
            let h = (t * Point::new(0.0, 1000.0) - origin).hypot();
            let mut adv = match &**g {
                PdfGlyph::Outline(o) => {
                    let bb = o.outline().bounding_box();
                    if bb.width() > 0.0 { (bb.x1.max(0.0) + 40.0) / 1000.0 * unit } else { 0.25 * unit }
                }
                _ => 0.5 * unit,
            };
            if let Some(n) = next {
                let d = n - origin;
                let along = d.x * axis.0 + d.y * axis.1;
                let across = (d.x * -axis.1 + d.y * axis.0).abs();
                if along > 0.0 && along < unit * 3.0 && across < h * 0.3 {
                    adv = along;
                }
            }
            let mut r = Rect::from_points(origin, origin);
            for p in [Point::new(0.0, -220.0), Point::new(adv / unit * 1000.0, -220.0), Point::new(adv / unit * 1000.0, 860.0), Point::new(0.0, 860.0)] {
                r = r.union_pt(t * p);
            }
            self.raw.push(RawGlyph { ch, origin, axis, h, adv, r });
        }
    }
}

/// Raw glyphs → the page's text layer: lines by the baseline, spaces where
/// the gap between glyphs is a space's, boxes in fractions of the page.
fn build_layer(raw: Vec<RawGlyph>, pw: f32, ph: f32) -> TextLayer {
    let mut out: Vec<Glyph> = Vec::with_capacity(raw.len());
    let mut line = 0u32;
    let mut prev: Option<&RawGlyph> = None;
    let fr = |r: &Rect| [(r.x0 as f32 / pw).clamp(0.0, 1.0), (r.y0 as f32 / ph).clamp(0.0, 1.0), (r.x1 as f32 / pw).clamp(0.0, 1.0), (r.y1 as f32 / ph).clamp(0.0, 1.0)];
    for g in &raw {
        if let Some(p) = prev {
            let d = g.origin - p.origin;
            let along = d.x * p.axis.0 + d.y * p.axis.1;
            let across = (d.x * -p.axis.1 + d.y * p.axis.0).abs();
            let h = p.h.max(g.h).max(1e-3);
            let new_line = across > h * 0.45 || along < -h * 0.6 || along > h * 12.0;
            if new_line {
                line += 1;
            } else {
                // the gap between the previous glyph's box and this one
                let gap = along - p.adv;
                let p_space = p.ch.ends_with(char::is_whitespace);
                let g_space = g.ch.starts_with(char::is_whitespace);
                if gap > h * 0.16 && !p_space && !g_space {
                    let r = Rect::new(p.r.x1.min(g.r.x0), p.r.y0.min(g.r.y0), g.r.x0.max(p.r.x1), p.r.y1.max(g.r.y1));
                    out.push(Glyph { ch: " ".into(), r: fr(&r), line, off: out.len(), soft: false });
                }
            }
        }
        let n = out.len();
        out.push(Glyph { ch: g.ch.clone(), r: fr(&g.r), line, off: n, soft: false });
        prev = Some(g);
    }
    // a run of spaces drawn as glyphs: the layer keeps one
    out.dedup_by(|b, a| a.line == b.line && a.ch.trim().is_empty() && b.ch.trim().is_empty());
    for (i, g) in out.iter_mut().enumerate() {
        g.off = i;
    }
    TextLayer { glyphs: out }
}

impl Document for PdfBook {
    fn format(&self) -> Format {
        Format::Pdf
    }

    fn meta(&self) -> &Meta {
        &self.meta
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn chapters(&self) -> usize {
        1
    }

    fn pages(&mut self, _chap: usize, _view: &View) -> usize {
        self.pdf.pages().len()
    }

    fn size(&mut self, _chap: usize, page: usize, _view: &View) -> (f32, f32) {
        self.pdf.pages().get(page).map(|p| p.render_dimensions()).unwrap_or((595.0, 842.0))
    }

    fn render(&mut self, _chap: usize, page: usize, _view: &View, px_w: u32) -> Result<Rgba, String> {
        let pages = self.pdf.pages();
        let p = pages.get(page).ok_or("página fora do documento")?;
        let (pw, _) = p.render_dimensions();
        let scale = (px_w.max(1) as f32 / pw.max(1.0)).min(12.0);
        let cache = hayro::RenderCache::new();
        let pix = hayro::render(p, &cache, &InterpreterSettings::default(), &hayro::RenderSettings::default(), &hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE });
        Ok(Rgba { w: pix.width() as u32, h: pix.height() as u32, px: pix.data_as_u8_slice().to_vec() })
    }

    fn text(&mut self, _chap: usize, page: usize, _view: &View) -> Option<TextLayer> {
        let t = self.layer(page);
        (!t.is_empty()).then_some(t)
    }

    fn links(&mut self, _chap: usize, page: usize, _view: &View) -> Vec<Link> {
        let mut out = Vec::new();
        let xref = self.pdf.xref();
        let Some(root) = xref.get::<Dict>(xref.root_id()) else { return out };
        let pages = self.pdf.pages();
        let Some(p) = pages.get(page) else { return out };
        let (pw, ph) = p.render_dimensions();
        let tf = hi::TransformExt::to_kurbo(&p.initial_transform(true));
        let Some(annots) = p.raw().get::<Array>(b"Annots") else { return out };
        for a in annots.iter::<Dict>() {
            if a.get::<Name>(b"Subtype").is_none_or(|s| &*s != b"Link") {
                continue;
            }
            let Some(r) = a.get::<hayro::hayro_syntax::object::Rect>(b"Rect") else { continue };
            let k = r.to_kurbo();
            let rr = (tf * k.to_path(0.1)).bounding_box();
            let fr = [(rr.x0 as f32 / pw).clamp(0.0, 1.0), (rr.y0 as f32 / ph).clamp(0.0, 1.0), (rr.x1 as f32 / pw).clamp(0.0, 1.0), (rr.y1 as f32 / ph).clamp(0.0, 1.0)];
            let action = a.get::<Dict>(b"A");
            let target = if let Some(uri) = action.as_ref().and_then(|x| x.get::<hayro::hayro_syntax::object::String>(b"URI")) {
                Some(Target::Url(String::from_utf8_lossy(uri.as_bytes()).into_owned()))
            } else {
                let d = a.get::<Object>(b"Dest").or_else(|| action.as_ref().and_then(|x| x.get::<Object>(b"D")));
                d.and_then(|d| self.dest_page(&root, &d, 0)).map(|pg| Target::At { chap: 0, offset: None, page: Some(pg) })
            };
            if let Some(target) = target {
                out.push(Link { r: fr, target, note: false });
            }
        }
        out
    }

    fn locate(&mut self, loc: &Locator, _view: &View) -> (usize, usize) {
        let n = self.pdf.pages().len();
        let mut page = loc.page.unwrap_or(1).saturating_sub(1).min(n.saturating_sub(1));
        // the quote, on the page or near it, when the file changed
        if let Some(q) = loc.quote.as_ref().filter(|q| q.exact.len() >= 12) {
            if self.layer(page).find(&q.exact).is_empty() {
                let near: Vec<usize> = (1..=6).flat_map(|d| [page.checked_sub(d), Some(page + d)]).flatten().filter(|p| *p < n).collect();
                if let Some(p) = near.into_iter().find(|p| !self.layer(*p).find(&q.exact).is_empty()) {
                    page = p;
                }
            }
        }
        (0, page)
    }

    fn locator(&mut self, _chap: usize, page: usize, _view: &View) -> Locator {
        Locator { page: Some(page + 1), ..Default::default() }
    }

    fn search(&mut self, query: &str, _view: &View) -> Vec<Hit> {
        let mut out = Vec::new();
        let n = self.pdf.pages().len();
        for page in 0..n {
            let t = self.layer(page);
            for (a, b) in t.find(query) {
                let ctx = t.text(a.saturating_sub(40), (b + 40).min(t.glyphs.len().saturating_sub(1)));
                out.push(Hit { chap: 0, page: Some(page), offset: Some(a), context: ctx });
                if out.len() >= 500 {
                    return out;
                }
            }
        }
        out
    }

    fn cover(&mut self, px_w: u32) -> Option<Rgba> {
        self.render(0, 0, &View::default(), px_w).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::{fonts::FontDb, pdf, Glyph as TGlyph, Item, Page, PageKind, PT_MM};

    #[test]
    fn text_layer_of_a_page() {
        let db = FontDb::new(&crate::typeset::testfonts::BUNDLED);
        let f = db.find("EB Garamond", false, 500).unwrap();
        let mut page = Page::new(140.0, 210.0, 1, PageKind::Flow, [255, 255, 255]);
        for (i, line) in ["O trigo domesticou o homem,", "e não o contrário."].iter().enumerate() {
            let s = db.shape(f, line, &[]);
            let k = 11.0 * PT_MM / db.face(f).upem;
            let mut pen = 0.0;
            let mut glyphs = Vec::new();
            let mut map = Vec::new();
            for g in s.iter() {
                glyphs.push(TGlyph { id: g.id, x: pen, y: 0.0 });
                let end = s.iter().map(|o| o.cluster).filter(|c| *c > g.cluster).min().unwrap_or(line.len() as u32);
                map.push((g.id, line[g.cluster as usize..end as usize].to_string()));
                pen += g.adv as f32 * k;
            }
            page.items.push(Item::Text { font: f, size: 11.0, x: 20.0, y: 40.0 + i as f32 * 6.0, color: [0, 0, 0], glyphs, text: line.to_string(), map });
        }
        let bytes = pdf::write(&[page], &db, &[], &pdf::Meta { title: "Sapiens".into(), author: "Harari".into(), lang: "pt-BR".into() }).unwrap();
        let dir = std::env::temp_dir().join(format!("ateliet-pdf-{}", crate::journal::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.pdf");
        std::fs::write(&p, bytes).unwrap();
        let mut b = PdfBook::open(&p).unwrap();
        assert_eq!(b.meta().title, "Sapiens");
        let v = View::default();
        let t = b.text(0, 0, &v).expect("text");
        let s = t.all_text();
        assert_eq!(s, "O trigo domesticou o homem, e não o contrário.");
        let (a, e) = t.find("domesticou")[0];
        let r = t.rects(a, e);
        assert_eq!(r.len(), 1);
        assert!(r[0][0] > 20.0 / 140.0 && r[0][1] > 0.1 && r[0][1] < 0.2, "{r:?}");
        assert_eq!(t.word(a + 2), (a, e));
        let img = b.render(0, 0, &v, 280).unwrap();
        assert_eq!(img.w, 280);
        assert_eq!(b.search("NÃO O CONTRÁRIO", &v).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
