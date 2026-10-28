//! The quadro as a PNG: columns, cards and drawings laid out as the page
//! items of the typesetter and drawn by its rasteriser (the same glyph
//! outlines as the book PDFs). World units are pixels at scale 1.

use super::element::Element;
use super::layout::{self, Measure, R};
use super::rough::{self, P};
use super::{Board, FileRef};
use crate::typeset::fonts::{FontDb, FontId};
use crate::typeset::{raster, Glyph, Item, Page, PageKind, Pen, Picture, PT_MM};
use std::collections::{HashMap, HashSet};

pub struct Fonts<'a> {
    pub db: &'a FontDb,
    pub regular: FontId,
    pub bold: FontId,
    pub mono: Option<FontId>,
}

struct DbMeasure<'a, 'b> {
    f: &'a Fonts<'b>,
    pics: &'a mut Pics<'b>,
}

/// Pictures loaded once, by key.
struct Pics<'a> {
    list: Vec<Picture>,
    index: HashMap<String, Option<usize>>,
    load: &'a mut dyn FnMut(&str) -> Option<Picture>,
}

impl Pics<'_> {
    fn get(&mut self, key: &str) -> Option<usize> {
        if let Some(i) = self.index.get(key) {
            return *i;
        }
        let i = (self.load)(key).map(|p| {
            self.list.push(p);
            self.list.len() - 1
        });
        self.index.insert(key.to_string(), i);
        i
    }
}

fn pt(px: f32) -> f32 {
    px / PT_MM
}

/// Greedy word wrap at `max` width (world units).
fn wrap(db: &FontDb, font: FontId, text: &str, size: f32, max: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && db.width(font, &cand, pt(size), &[]) > max {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = cand;
            }
        }
        out.push(line);
    }
    out
}

impl Measure for DbMeasure<'_, '_> {
    fn text(&mut self, text: &str, size: f32, w: f32) -> (f32, f32) {
        let lines = wrap(self.f.db, self.f.regular, text, size, w);
        let width = lines.iter().map(|l| self.f.db.width(self.f.regular, l, pt(size), &[])).fold(0.0, f32::max);
        (width, lines.len() as f32 * size * 1.3)
    }
    fn picture(&mut self, target: &str) -> Option<(f32, f32)> {
        let i = self.pics.get(target)?;
        let p = &self.pics.list[i];
        Some((p.w as f32, p.h as f32))
    }
}

fn text_item(db: &FontDb, font: FontId, s: &str, size: f32, x: f32, baseline: f32, color: [u8; 3]) -> Item {
    let k = pt(size) * PT_MM / db.face(font).upem;
    let mut glyphs = Vec::new();
    let mut gx = 0.0;
    for g in db.shape(font, s, &[]).iter() {
        glyphs.push(Glyph { id: g.id, x: gx + g.dx as f32 * k, y: -(g.dy as f32) * k });
        gx += g.adv as f32 * k;
    }
    Item::Text { font, size: pt(size), x, y: baseline, color, glyphs, text: s.to_string(), map: Vec::new() }
}

fn rounded_rect(r: R, rad: f32) -> Vec<P> {
    let (x, y, w, h) = r;
    let rad = rad.min(w / 2.0).min(h / 2.0);
    let mut pts = Vec::new();
    for (cx, cy, a0) in [(x + w - rad, y + rad, -90.0f32), (x + w - rad, y + h - rad, 0.0), (x + rad, y + h - rad, 90.0), (x + rad, y + rad, 180.0)] {
        for i in 0..=6 {
            let a = (a0 + i as f32 * 15.0).to_radians();
            pts.push((cx + rad * a.cos(), cy + rad * a.sin()));
        }
    }
    pts
}

fn rgb(c: [u8; 4]) -> [u8; 3] {
    [c[0], c[1], c[2]]
}

/// A polyline as an outline the rasteriser strokes without closing it
/// (there and back: the union of the same segments).
fn open_path(pts: &[P], width: f32, color: [u8; 4]) -> Item {
    let mut q = pts.to_vec();
    q.extend(pts.iter().rev().skip(1).copied());
    let path = Item::Path { polys: vec![q], fill: None, fill_alpha: 1.0, stroke: Some(Pen { width, color: rgb(color), dash: None }) };
    with_alpha(path, color[3])
}

fn with_alpha(it: Item, a: u8) -> Item {
    if a == 255 {
        return it;
    }
    Item::Group { m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], clip: None, alpha: a as f32 / 255.0, items: vec![it] }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8)
}

fn card_items(out: &mut Vec<Item>, b: &Board, cb: &layout::CardBox, color: Option<&str>, f: &Fonts, pics: &mut Pics) {
    let fill = [255, 255, 255];
    out.push(Item::Path { polys: vec![rounded_rect(cb.rect, 8.0)], fill: Some(fill), fill_alpha: 1.0, stroke: Some(Pen { width: 1.0, color: [0xdd, 0xdc, 0xd6], dash: None }) });
    if let Some(c) = color.and_then(rough::color) {
        let (x, y, _, h) = cb.rect;
        out.push(Item::Path { polys: vec![rounded_rect((x, y + 6.0, 3.0, h - 12.0), 1.5)], fill: Some(rgb(c)), fill_alpha: 1.0, stroke: None });
    }
    if let Some((target, r)) = &cb.picture {
        if let Some(i) = pics.get(target) {
            out.push(Item::Image { pic: i, x: r.0, y: r.1, w: r.2, h: r.3 });
        }
    }
    let size = layout::TITLE_SIZE;
    let note = cb.info.note().is_some() && cb.info.is_note_card();
    let color = if note { [0x5b, 0x45, 0xb8] } else { [0x22, 0x22, 0x22] };
    for (i, line) in wrap(f.db, f.regular, &cb.title, size, cb.title_rect.2).iter().enumerate() {
        out.push(text_item(f.db, f.regular, line, size, cb.title_rect.0, cb.title_rect.1 + i as f32 * size * 1.3 + size * 1.0, color));
    }
    for (tag, r) in &cb.chips {
        let (fg, _) = layout::tag_colors(b, tag);
        out.push(Item::Path { polys: vec![rounded_rect(*r, 6.0)], fill: Some(mix(fg, [255, 255, 255], 0.82)), fill_alpha: 1.0, stroke: None });
        out.push(text_item(f.db, f.bold, tag, layout::CHIP_SIZE, r.0 + 7.0, r.1 + r.3 / 2.0 + layout::CHIP_SIZE * 0.36, mix(fg, [0, 0, 0], 0.25)));
    }
    if let Some((t, r)) = &cb.meta {
        out.push(text_item(f.db, f.regular, t, layout::META_SIZE, r.0, r.1 + layout::META_SIZE, [0x77, 0x77, 0x77]));
    }
}

fn element_items(out: &mut Vec<Item>, b: &Board, e: &Element, f: &Fonts, pics: &mut Pics) {
    if e.deleted() {
        return;
    }
    let mut items = Vec::new();
    match e.kind() {
        "text" => {
            let size = e.fo("fontSize", 20.0);
            let lh = e.fo("lineHeight", 1.25) * size;
            let font = if e.get("fontFamily").as_i64() == Some(3) { f.mono.unwrap_or(f.regular) } else { f.regular };
            let color = rough::color(e.s("strokeColor")).unwrap_or([30, 30, 30, 255]);
            let text = if e.s("text").is_empty() { e.raw_text() } else { e.s("text") };
            let w = e.w();
            for (i, line) in text.split('\n').enumerate() {
                let lw = f.db.width(font, line, pt(size), &[]);
                let x = match e.s("textAlign") {
                    "center" => e.x() + (w - lw) / 2.0,
                    "right" => e.x() + w - lw,
                    _ => e.x(),
                };
                items.push(text_item(f.db, font, line, size, x, e.y() + i as f32 * lh + lh / 2.0 + size * 0.35, rgb(color)));
            }
            let a = (e.fo("opacity", 100.0) / 100.0 * color[3] as f32) as u8;
            if a < 255 {
                items = vec![with_alpha(Item::Group { m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], clip: None, alpha: 1.0, items }, a)];
            }
        }
        "image" => {
            let key = e.get("fileId").as_str().and_then(|id| b.file(id)).and_then(|r| match r {
                FileRef::Data(v) => v["dataURL"].as_str().map(|s| format!("data:{}", s.trim_start_matches("data:"))),
                other => other.vault_path(),
            });
            if let Some(i) = key.and_then(|k| pics.get(&k)) {
                let (x0, y0, x1, y1) = e.bounds();
                items.push(Item::Image { pic: i, x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
            }
        }
        _ => {
            let g = rough::geometry(e);
            for fl in &g.fills {
                items.push(with_alpha(Item::Path { polys: vec![fl.pts.clone()], fill: Some(rgb(fl.color)), fill_alpha: 1.0, stroke: None }, fl.color[3]));
            }
            for s in &g.strokes {
                items.push(open_path(&s.pts, s.width, s.color));
            }
            if matches!(e.kind(), "frame" | "magicframe") {
                let name = e.get("name").as_str().unwrap_or("Frame");
                items.push(text_item(f.db, f.regular, name, 14.0, e.x(), e.y() - 6.0, [0x88, 0x88, 0x88]));
            }
        }
    }
    // text and pictures turn with the element (shapes come rotated already)
    let a = e.angle();
    if a != 0.0 && matches!(e.kind(), "text" | "image") {
        let c = e.center();
        let (s, co) = a.sin_cos();
        let m = [co, s, -s, co, c.0 - co * c.0 + s * c.1, c.1 - s * c.0 - co * c.1];
        out.push(Item::Group { m, clip: None, alpha: 1.0, items });
    } else {
        out.extend(items);
    }
}

fn union(a: Option<R>, r: R) -> Option<R> {
    let (x0, y0, x1, y1) = (r.0, r.1, r.0 + r.2, r.1 + r.3);
    Some(match a {
        None => (x0, y0, x1 - x0, y1 - y0),
        Some(b) => {
            let (bx0, by0, bx1, by1) = (b.0, b.1, b.0 + b.2, b.1 + b.3);
            let (nx0, ny0, nx1, ny1) = (bx0.min(x0), by0.min(y0), bx1.max(x1), by1.max(y1));
            (nx0, ny0, nx1 - nx0, ny1 - ny0)
        }
    })
}

/// Draw the board (or only the ids in `only`) at `scale` pixels per unit.
/// `load` gives a picture for a vault target or a `data:` URL.
pub fn render(b: &Board, f: &Fonts, load: &mut dyn FnMut(&str) -> Option<Picture>, only: Option<&HashSet<String>>, scale: f32) -> Result<(u32, u32, Vec<u8>), String> {
    let mut data_load = |k: &str| -> Option<Picture> {
        if let Some(rest) = k.strip_prefix("data:") {
            let bytes = super::data_url_bytes(rest)?;
            let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
            return Some(Picture { w: img.width(), h: img.height(), rgba: img.into_raw(), jpeg: None });
        }
        load(k)
    };
    let mut pics = Pics { list: Vec::new(), index: HashMap::new(), load: &mut data_load };
    let trig = layout::triggers(b);
    let want = |id: &str| only.is_none_or(|s| s.contains(id));
    let mut items = Vec::new();
    let mut bounds: Option<R> = None;
    // columns and their cards
    let lanes: Vec<(usize, layout::LaneBox)> = {
        let mut m = DbMeasure { f, pics: &mut pics };
        b.lanes.iter().enumerate().map(|(i, l)| (i, layout::lane_box(l, &trig, &mut m))).collect()
    };
    for (i, lb) in &lanes {
        let l = &b.lanes[*i];
        let lane_on = want(&l.id);
        let cards: Vec<&layout::CardBox> = lb.cards.iter().filter(|c| lane_on || want(&c.id)).collect();
        if !lane_on && cards.is_empty() {
            continue;
        }
        if lane_on {
            bounds = union(bounds, lb.rect);
            items.push(Item::Path { polys: vec![rounded_rect(lb.rect, 10.0)], fill: Some([0xf2, 0xf1, 0xee]), fill_alpha: 1.0, stroke: Some(Pen { width: 1.0, color: [0xe0, 0xdf, 0xda], dash: None }) });
            let title = if l.max > 0 { format!("{}  {}/{}", l.title, l.cards.len(), l.max) } else { format!("{}  {}", l.title, l.cards.len()) };
            items.push(text_item(f.db, f.bold, &title, 14.0, lb.rect.0 + 12.0, lb.rect.1 + 27.0, [0x22, 0x22, 0x22]));
        }
        for cb in cards {
            bounds = union(bounds, cb.rect);
            card_items(&mut items, b, cb, None, f, &mut pics);
        }
    }
    let loose: Vec<layout::CardBox> = {
        let mut m = DbMeasure { f, pics: &mut pics };
        b.cards.iter().filter(|c| want(&c.id)).map(|c| layout::card_box(c, c.x, c.y, c.w, &trig, &mut m)).collect()
    };
    // as on the canvas: shapes and pictures, the columns, then lines,
    // arrows, strokes and loose text, then loose cards
    let mut under = Vec::new();
    let mut over = Vec::new();
    for e in b.elements.iter().filter(|e| !e.deleted() && (want(e.id()) || e.container().is_some_and(&want))) {
        let (x0, y0, x1, y1) = e.aabb();
        let pad = e.fo("strokeWidth", 2.0) + 4.0;
        bounds = union(bounds, (x0 - pad, y0 - pad, x1 - x0 + 2.0 * pad, y1 - y0 + 2.0 * pad));
        let top = matches!(e.kind(), "arrow" | "line" | "freedraw") || (e.kind() == "text" && e.container().is_none());
        element_items(if top { &mut over } else { &mut under }, b, e, f, &mut pics);
    }
    let mut page_items = under;
    page_items.extend(items);
    page_items.extend(over);
    for (cb, c) in loose.iter().zip(b.cards.iter().filter(|c| want(&c.id))) {
        bounds = union(bounds, cb.rect);
        card_items(&mut page_items, b, cb, c.color.as_deref(), f, &mut pics);
    }
    let Some(bx) = bounds else { return Err("nada para exportar".into()) };
    let pad = 24.0;
    let (w, h) = (bx.2 + 2.0 * pad, bx.3 + 2.0 * pad);
    let scale = scale.min(8000.0 / w.max(h)).max(0.05);
    let bg = b.kept.scene.get("appState").and_then(|a| a.get("viewBackgroundColor")).and_then(|v| v.as_str()).and_then(rough::color).map(rgb).unwrap_or([255, 255, 255]);
    let mut page = Page::new(w, h, 1, PageKind::Fixed, bg);
    page.items = vec![Item::Group { m: [1.0, 0.0, 0.0, 1.0, pad - bx.0, pad - bx.1], clip: None, alpha: 1.0, items: page_items }];
    Ok(raster::render(&page, f.db, &pics.list, scale))
}

/// The board as PNG bytes.
pub fn png(b: &Board, f: &Fonts, load: &mut dyn FnMut(&str) -> Option<Picture>, only: Option<&HashSet<String>>, scale: f32) -> Result<Vec<u8>, String> {
    let (w, h, px) = render(b, f, load, only, scale)?;
    Ok(crate::board::tile::encode_png(&px, w, h))
}

#[cfg(test)]
mod tests {
    use super::super::element::Style;
    use super::*;

    #[test]
    fn draws_columns_cards_and_shapes() {
        let inter: &'static [u8] = Box::leak(std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/fonts/Inter-Regular.ttf")).unwrap().into_boxed_slice());
        let db = FontDb::new(&[inter]);
        let f = Fonts { db: &db, regular: 0, bold: 0, mono: None };
        let mut b = super::super::kanban::parse(&super::super::new_board_text(&["A fazer", "Feito"])).unwrap();
        b.lanes[0].cards.push(super::super::Card::new("Escrever o capítulo #livro @{2024-05-01}"));
        let mut st = Style::default();
        st.background = "#ffc9c9".into();
        st.fill = "solid".into();
        b.elements.push(Element::new("ellipse", 100.0, 300.0, 120.0, 80.0, &st));
        let (w, h, px) = render(&b, &f, &mut |_| None, None, 1.0).unwrap();
        assert!(w > 500 && h > 300);
        // the ellipse's fill, under its centre
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        let (cx, cy) = (24 + 160, (24.0 + 340.0) as u32);
        assert_eq!(&at(cx, cy)[..3], &[0xff, 0xc9, 0xc9]);
        // some dark ink (text) in the first column's header
        assert!((24..60).any(|y| (36..140).any(|x| at(x, y)[0] < 100)));
    }
}
