//! The page kinds of a storyboard (thumbnails, name, comic, webtoon) and
//! the presentation board: what each starts with, the panels (polygons on
//! the page, cut along a stroke with the page's gutter), the balloons, the
//! reading order, and the page drawn over the layers (paper outside the
//! panels, borders, balloons, numbers) as page items — the same in the
//! editor, the PNGs and the PDF.

use super::doc::{self, BoardDoc};
use crate::typeset::compose;
use crate::typeset::fonts::FontDb;
use crate::typeset::{Item, Pen, PT_MM};
use serde_json::{json, Value as Json};

pub struct KindInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub unit: &'static str,
    pub units: &'static str,
}

pub const KINDS: [KindInfo; 7] = [
    KindInfo { id: "animatic", name: "Animatic", blurb: "Quadros com tempo para cinema e animação: reprodução, papel de cebola, notas de plano.", unit: "quadro", units: "quadros" },
    KindInfo { id: "presentation", name: "Prancha de apresentação", blurb: "Os planos como cartões com as notas, impressos ou enviados em PDF.", unit: "plano", units: "planos" },
    KindInfo { id: "thumbnails", name: "Miniaturas", blurb: "Folhas de quadrinhos pequenos para explorar depressa; marque os melhores com estrela.", unit: "folha", units: "folhas" },
    KindInfo { id: "name", name: "Name (mangá)", blurb: "Páginas de mangá: quadros, balões verticais, da direita para a esquerda, páginas duplas.", unit: "página", units: "páginas" },
    KindInfo { id: "comic", name: "Páginas de HQ", blurb: "Páginas de quadrinhos: quadros, balões e recordatórios, da esquerda para a direita.", unit: "página", units: "páginas" },
    KindInfo { id: "webtoon", name: "Webtoon", blurb: "Uma tira vertical lida rolando, exportada em fatias do tamanho do envio.", unit: "parte", units: "partes" },
    KindInfo { id: "canvas", name: "Tela infinita", blurb: "Uma tela sem fim: sem quadros, sem linha do tempo, sem bordas. Esboce, estude, planeje.", unit: "tela", units: "telas" },
];

pub fn kind_info(k: &str) -> &'static KindInfo {
    KINDS.iter().find(|x| x.id == k).unwrap_or(&KINDS[0])
}

pub fn is_page_kind(k: &str) -> bool {
    matches!(k, "thumbnails" | "name" | "comic" | "webtoon")
}

/// (id, name, trim w, trim h, bleed, inner w, inner h) in mm.
pub type PageSize = (&'static str, &'static str, f32, f32, f32, f32, f32);

pub const NAME_SIZES: [PageSize; 4] = [
    ("b5", "B5 · 182 × 257 mm (revista, tankōbon)", 182.0, 257.0, 5.0, 150.0, 220.0),
    ("a5", "A5 · 148 × 210 mm", 148.0, 210.0, 5.0, 122.0, 180.0),
    ("b6", "B6 · 128 × 182 mm (tankōbon pequeno)", 128.0, 182.0, 5.0, 105.0, 155.0),
    ("a4", "A4 · 210 × 297 mm", 210.0, 297.0, 5.0, 180.0, 270.0),
];

pub const COMIC_SIZES: [PageSize; 5] = [
    ("us", "Comic americano · 6,625 × 10,25 pol", 168.3, 260.4, 3.2, 152.4, 241.3),
    ("album", "Álbum europeu · 225 × 300 mm", 225.0, 300.0, 5.0, 200.0, 270.0),
    ("a4", "A4 · 210 × 297 mm", 210.0, 297.0, 3.0, 186.0, 273.0),
    ("letter", "Carta americana · 8,5 × 11 pol", 215.9, 279.4, 3.2, 190.5, 254.0),
    ("digest", "Digest · 5,5 × 8,5 pol", 139.7, 215.9, 3.2, 120.0, 196.0),
];

pub const THUMB_ASPECTS: [(&str, &str, f32); 6] = [("16:9", "16:9 (cinema, vídeo)", 16.0 / 9.0), ("2.39", "2,39:1 (scope)", 2.39), ("4:3", "4:3", 4.0 / 3.0), ("1:1", "1:1", 1.0), ("9:16", "9:16 (vídeo vertical)", 9.0 / 16.0), ("page", "Página de mangá / HQ", 182.0 / 257.0)];

pub const WEBTOON_WIDTHS: [(u32, &str); 3] = [(800, "800 px (Webtoon)"), (690, "690 px"), (940, "940 px (Tapas)")];

/// Rows of a page layout: (relative height, relative column widths).
pub type Rows = &'static [(f32, &'static [f32])];

pub const LAYOUTS: [(&str, &str, Rows); 10] = [
    ("one", "Um quadro", &[(1.0, &[1.0])]),
    ("two", "Duas tiras", &[(1.0, &[1.0]), (1.0, &[1.0])]),
    ("three", "Três tiras", &[(1.0, &[1.0]), (1.0, &[1.0]), (1.0, &[1.0])]),
    ("three-split", "Três tiras, a do meio dividida", &[(1.0, &[1.0]), (1.0, &[1.0, 1.0]), (1.0, &[1.0])]),
    ("classic", "Seis quadros (2 × 3)", &[(1.0, &[1.0, 1.0]), (1.0, &[1.0, 1.0]), (1.0, &[1.0, 1.0])]),
    ("nine", "Nove quadros (3 × 3)", &[(1.0, &[1.0, 1.0, 1.0]), (1.0, &[1.0, 1.0, 1.0]), (1.0, &[1.0, 1.0, 1.0])]),
    ("big-top", "Quadro grande, dois embaixo", &[(1.6, &[1.0]), (1.0, &[1.0, 1.0])]),
    ("big-bottom", "Dois em cima, quadro grande", &[(1.0, &[1.0, 1.0]), (1.6, &[1.0])]),
    ("action", "Largo, três estreitos, largo", &[(1.0, &[1.0]), (1.2, &[1.0, 1.0, 1.0]), (1.0, &[1.0])]),
    ("yonkoma", "4-koma (quatro empilhados)", &[(1.0, &[1.0]), (1.0, &[1.0]), (1.0, &[1.0]), (1.0, &[1.0])]),
];

pub const BALLOON_KINDS: [(&str, &str); 6] = [("speech", "Fala"), ("thought", "Pensamento"), ("shout", "Grito"), ("whisper", "Sussurro"), ("narration", "Recordatório"), ("sfx", "Onomatopeia")];

/// Everything a new storyboard of a kind needs.
#[derive(Clone, Debug)]
pub struct KindOptions {
    pub kind: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub background: String,
    pub page_size: String,
    pub dpi: f32,
    pub rtl: bool,
    pub spreads: bool,
    pub layout: String,
    pub pages: usize,
    pub aspect: String,
    pub landscape: bool,
    pub strip_width: u32,
    pub slices: u32,
}

impl KindOptions {
    pub fn defaults(kind: &str) -> KindOptions {
        let paper = matches!(kind, "name" | "comic" | "webtoon" | "thumbnails");
        KindOptions {
            kind: kind.into(),
            width: 1920,
            height: 1080,
            fps: 24,
            background: if paper { "#ffffff".into() } else { doc::PAPER.into() },
            page_size: if kind == "comic" { "us".into() } else { "b5".into() },
            dpi: 150.0,
            rtl: kind == "name",
            spreads: matches!(kind, "name" | "comic"),
            layout: "one".into(),
            pages: if matches!(kind, "name" | "comic") { 4 } else { 1 },
            aspect: "16:9".into(),
            landscape: true,
            strip_width: 800,
            slices: 10,
        }
    }
}

fn mm_px(mm: f32, dpi: f32) -> f32 {
    (mm / 25.4 * dpi).round()
}

/// Canvas size and page setup of a page kind.
pub fn page_geometry(o: &KindOptions) -> (u32, u32, Json) {
    let mut page = json!({ "dpi": 150, "bleed": 0, "inner": { "x": 0, "y": 0, "w": 0, "h": 0 }, "rtl": false, "spreads": false, "gutterX": 12, "gutterY": 24, "border": 4, "mask": true, "clipToPanel": true, "slice": 1280 });
    match o.kind.as_str() {
        "name" | "comic" => {
            let sizes: &[PageSize] = if o.kind == "name" { &NAME_SIZES } else { &COMIC_SIZES };
            let s = sizes.iter().find(|s| s.0 == o.page_size).unwrap_or(&sizes[0]);
            let dpi = if o.dpi > 0.0 { o.dpi } else { 150.0 };
            let bleed = mm_px(s.4, dpi);
            let (w, h) = (mm_px(s.2, dpi) + 2.0 * bleed, mm_px(s.3, dpi) + 2.0 * bleed);
            let (iw, ih) = (mm_px(s.5, dpi), mm_px(s.6, dpi));
            let manga = o.kind == "name";
            page = json!({ "dpi": dpi, "bleed": bleed, "inner": { "x": ((w - iw) / 2.0).round(), "y": ((h - ih) / 2.0).round(), "w": iw, "h": ih }, "rtl": o.rtl, "spreads": o.spreads,
                "gutterX": mm_px(if manga { 2.0 } else { 3.0 }, dpi), "gutterY": mm_px(if manga { 5.0 } else { 3.0 }, dpi), "border": mm_px(0.6, dpi).max(2.0), "mask": true, "clipToPanel": true, "slice": 1280 });
            (w as u32, h as u32, page)
        }
        "thumbnails" => {
            let dpi = 150.0;
            let (w, h) = if o.landscape { (mm_px(297.0, dpi), mm_px(210.0, dpi)) } else { (mm_px(210.0, dpi), mm_px(297.0, dpi)) };
            let m = mm_px(12.0, dpi);
            page = json!({ "dpi": dpi, "bleed": 0, "inner": { "x": m, "y": m, "w": w - 2.0 * m, "h": h - 2.0 * m }, "rtl": false, "spreads": false, "gutterX": mm_px(6.0, dpi), "gutterY": mm_px(6.0, dpi), "border": 3, "mask": true, "clipToPanel": true, "slice": 1280 });
            (w as u32, h as u32, page)
        }
        _ => {
            let w = o.strip_width.max(200) as f32;
            let h = 1280.0 * o.slices.max(1) as f32;
            let m = (w * 0.05).round();
            page = json!({ "dpi": 72, "bleed": 0, "inner": { "x": m, "y": 0, "w": w - 2.0 * m, "h": h }, "rtl": false, "spreads": false, "gutterX": 30, "gutterY": 200, "border": 3, "mask": false, "clipToPanel": false, "slice": 1280 });
            let _ = &mut page;
            (w as u32, h as u32, page)
        }
    }
}

pub fn thumb_aspect(a: &str) -> f32 {
    THUMB_ASPECTS.iter().find(|x| x.0 == a).map(|x| x.2).unwrap_or(16.0 / 9.0)
}

// ── geometry ───────────────────────────────────────────────────────────

pub type Pt = (f32, f32);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn num(v: &Json, d: f32) -> f32 {
    v.as_f64().map(|x| x as f32).unwrap_or(d)
}

pub fn rect_of(v: &Json) -> R {
    R { x: num(&v["x"], 0.0), y: num(&v["y"], 0.0), w: num(&v["w"], 0.0), h: num(&v["h"], 0.0) }
}

pub fn points(panel: &Json) -> Vec<Pt> {
    panel["points"].as_array().map(|a| a.iter().map(|q| (num(&q["x"], 0.0), num(&q["y"], 0.0))).collect()).unwrap_or_default()
}

pub fn set_points(panel: &mut Json, pts: &[Pt]) {
    panel["points"] = json!(pts.iter().map(|p| json!({ "x": (p.0 * 10.0).round() / 10.0, "y": (p.1 * 10.0).round() / 10.0 })).collect::<Vec<_>>());
}

pub fn rect_panel(r: R) -> Json {
    let mut p = json!({ "id": doc::uid() });
    set_points(&mut p, &[(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)]);
    p
}

pub fn inside(poly: &[Pt], p: Pt) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
    }
    c
}

pub fn bounds(poly: &[Pt]) -> R {
    if poly.is_empty() {
        return R::default();
    }
    let (x0, y0) = poly.iter().fold((f32::MAX, f32::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
    let (x1, y1) = poly.iter().fold((f32::MIN, f32::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
    R { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// The part of a convex polygon where n·(p − a) ≥ d (Sutherland–Hodgman).
fn clip_half(poly: &[Pt], a: Pt, n: Pt, d: f32) -> Vec<Pt> {
    let side = |p: Pt| n.0 * (p.0 - a.0) + n.1 * (p.1 - a.1) - d;
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sp, sq) = (side(p), side(q));
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp >= 0.0) != (sq >= 0.0) {
            let t = sp / (sp - sq);
            out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
        }
    }
    out
}

pub fn area(poly: &[Pt]) -> f32 {
    let mut s = 0.0;
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        s += p.0 * q.1 - q.0 * p.1;
    }
    s / 2.0
}

/// Whether the line a→b passes through the polygon (vertices on both sides,
/// and the stroke itself reaches it).
pub fn line_cuts(poly: &[Pt], a: Pt, b: Pt) -> bool {
    let n = (-(b.1 - a.1), b.0 - a.0);
    let (mut pos, mut neg) = (false, false);
    for p in poly {
        let s = n.0 * (p.0 - a.0) + n.1 * (p.1 - a.1);
        if s > 1e-6 {
            pos = true;
        }
        if s < -1e-6 {
            neg = true;
        }
    }
    if !pos || !neg {
        return false;
    }
    let r = bounds(poly);
    a.0.max(b.0) >= r.x && a.0.min(b.0) <= r.x + r.w && a.1.max(b.1) >= r.y && a.1.min(b.1) <= r.y + r.h
}

/// A panel cut along a→b, leaving `gap` between the halves.
pub fn cut_panel(panel: &Json, a: Pt, b: Pt, gap: f32) -> Vec<Json> {
    let pts = points(panel);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-6);
    let n = (-(b.1 - a.1) / len, (b.0 - a.0) / len);
    let one = clip_half(&pts, a, n, gap / 2.0);
    let two = clip_half(&pts, a, (-n.0, -n.1), gap / 2.0);
    let mut out = Vec::new();
    for half in [one, two] {
        if half.len() >= 3 && area(&half).abs() > 100.0 {
            let mut p = json!({ "id": doc::uid() });
            set_points(&mut p, &half);
            out.push(p);
        }
    }
    if out.len() == 2 {
        out
    } else {
        vec![panel.clone()]
    }
}

/// Panels in reading order: rows top to bottom, each left→right (or right→left).
pub fn reading_order(panels: &[Json], rtl: bool) -> Vec<usize> {
    let mut items: Vec<(usize, R)> = panels.iter().enumerate().map(|(i, p)| (i, bounds(&points(p)))).collect();
    items.sort_by(|a, b| a.1.y.partial_cmp(&b.1.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut rows: Vec<Vec<(usize, R)>> = Vec::new();
    for it in items {
        match rows.last_mut() {
            Some(row) if it.1.y < row[0].1.y + row[0].1.h * 0.5 => row.push(it),
            _ => rows.push(vec![it]),
        }
    }
    let mut out = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| {
            let (ka, kb) = if rtl { (-(a.1.x + a.1.w), -(b.1.x + b.1.w)) } else { (a.1.x, b.1.x) };
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.extend(row.into_iter().map(|x| x.0));
    }
    out
}

pub fn layout_panels(inner: R, rows: Rows, gx: f32, gy: f32, rtl: bool) -> Vec<Json> {
    let total_h: f32 = rows.iter().map(|r| r.0).sum();
    let usable_h = inner.h - gy * (rows.len() as f32 - 1.0);
    let mut out = Vec::new();
    let mut y = inner.y;
    for (rh, cols) in rows {
        let h = usable_h * rh / total_h;
        let total_w: f32 = cols.iter().sum();
        let usable_w = inner.w - gx * (cols.len() as f32 - 1.0);
        let mut cs: Vec<f32> = cols.to_vec();
        if rtl {
            cs.reverse();
        }
        let mut x = inner.x;
        for c in cs {
            let w = usable_w * c / total_w;
            out.push(rect_panel(R { x: x.round(), y: y.round(), w: w.round(), h: h.round() }));
            x += w + gx;
        }
        y += h + gy;
    }
    out
}

/// A grid of panels of one aspect (thumbnails), centred, with room for numbers.
pub fn grid_panels(inner: R, cols: usize, rows: usize, aspect: f32, gap: f32, label: f32) -> Vec<Json> {
    let cw = (inner.w - gap * (cols as f32 - 1.0)) / cols as f32;
    let ch = (inner.h - gap * (rows as f32 - 1.0)) / rows as f32 - label;
    let (mut w, mut h) = (cw, cw / aspect);
    if h > ch {
        h = ch;
        w = ch * aspect;
    }
    let tw = cols as f32 * w + (cols as f32 - 1.0) * gap;
    let th = rows as f32 * (h + label) + (rows as f32 - 1.0) * gap;
    let (x0, y0) = (inner.x + (inner.w - tw) / 2.0, inner.y + (inner.h - th) / 2.0);
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            out.push(rect_panel(R { x: (x0 + c as f32 * (w + gap)).round(), y: (y0 + r as f32 * (h + label + gap)).round(), w: w.round(), h: h.round() }));
        }
    }
    out
}

/// How many cells of an aspect fit a sheet nicely (about a dozen that fill it).
pub fn grid_for(inner: R, aspect: f32) -> (usize, usize) {
    let mut best = (3, 3, -1.0f32);
    for cols in 2..=6 {
        for rows in 2..=6 {
            let (cw, ch) = (inner.w / cols as f32, inner.h / rows as f32);
            let w = cw.min(ch * aspect);
            let fill = w * (w / aspect) * (cols * rows) as f32 / (inner.w * inner.h);
            let n = (cols * rows) as f32;
            let score = fill - (n - 12.0).abs() * 0.02;
            if score > best.2 {
                best = (cols, rows, score);
            }
        }
    }
    (best.0, best.1)
}

pub fn inner_rect(board: &Json) -> R {
    let i = rect_of(&board["page"]["inner"]);
    if i.w > 0.0 && i.h > 0.0 {
        i
    } else {
        let b = num(&board["page"]["bleed"], 0.0);
        R { x: b, y: b, w: num(&board["width"], 1920.0) - 2.0 * b, h: num(&board["height"], 1080.0) - 2.0 * b }
    }
}

/// The panels a fresh page of this board starts with.
pub fn start_panels(board: &Json, layout: &str) -> Vec<Json> {
    let page = &board["page"];
    if !page.is_object() || board["kind"] == "webtoon" {
        return Vec::new();
    }
    let inner = inner_rect(board);
    let (gx, gy) = (num(&page["gutterX"], 12.0), num(&page["gutterY"], 24.0));
    if board["kind"] == "thumbnails" {
        let a = num(&board["thumbAspect"], 16.0 / 9.0);
        let (c, r) = grid_for(inner, a);
        let label = (num(&board["width"], 1920.0).min(num(&board["height"], 1080.0)) / 40.0).round();
        return grid_panels(inner, c, r, a, gx, label);
    }
    let spec = LAYOUTS.iter().find(|l| l.0 == layout).unwrap_or(&LAYOUTS[0]);
    if spec.0 == "one" {
        return vec![rect_panel(inner)];
    }
    layout_panels(inner, spec.2, gx, gy, page["rtl"].as_bool().unwrap_or(false))
}

/// A new storyboard of a kind.
pub fn board_for_kind(o: &KindOptions) -> BoardDoc {
    if o.kind == "canvas" || !is_page_kind(&o.kind) {
        return BoardDoc::new(&o.kind, o.width, o.height, &o.background, o.fps);
    }
    let (w, h, page) = page_geometry(o);
    let mut d = BoardDoc::new(&o.kind, w, h, &o.background, o.fps);
    d.board["page"] = page;
    if o.kind == "thumbnails" {
        d.board["thumbAspect"] = json!(thumb_aspect(&o.aspect));
    }
    d.board["frames"][0]["panels"] = json!(start_panels(&d.board, &o.layout));
    d.board["frames"][0]["balloons"] = json!([]);
    for i in 1..o.pages.max(1) {
        add_page(&mut d, i, &o.layout);
    }
    d
}

/// A new frame, or a new page with its starting panels (a thumbnail sheet
/// repeats the previous sheet's grid).
pub fn add_page(d: &mut BoardDoc, index: usize, layout: &str) -> usize {
    let at = d.add_frame(index);
    if is_page_kind(d.kind()) {
        let prev = if at > 0 { d.board["frames"][at - 1]["panels"].as_array().cloned().unwrap_or_default() } else { Vec::new() };
        let fid = d.frame_id(at);
        let panels: Vec<Json> = if d.kind() == "thumbnails" && !prev.is_empty() {
            prev.iter().map(|p| {
                let mut q = p.clone();
                q["id"] = json!(format!("{}-{}", p["id"].as_str().unwrap_or(""), &fid[..4.min(fid.len())]));
                q["star"] = json!(false);
                q
            }).collect()
        } else {
            start_panels(&d.board, layout)
        };
        d.board["frames"][at]["panels"] = json!(panels);
        d.board["frames"][at]["balloons"] = json!([]);
    }
    at
}

/// The facing page of page i in a spread (page 1 alone, then pairs), if any.
pub fn facing(board: &Json, i: usize, n: usize) -> Option<usize> {
    if !board["page"]["spreads"].as_bool().unwrap_or(false) || i == 0 {
        return None;
    }
    let j = if i % 2 == 1 { i + 1 } else { i - 1 };
    (j < n).then_some(j)
}

/// Whether page i sits on the left of its spread (manga mirrors it).
pub fn is_left(board: &Json, i: usize) -> bool {
    let even = (i + 1) % 2 == 0;
    if board["page"]["rtl"].as_bool().unwrap_or(false) {
        !even
    } else {
        even
    }
}

pub fn new_balloon(kind: &str, r: R, vertical: bool, size: f32) -> Json {
    let tail = matches!(kind, "speech" | "thought" | "shout" | "whisper").then(|| json!({ "x": r.x + r.w * 0.3, "y": r.y + r.h + (r.h * 0.4).max(30.0) }));
    json!({ "id": doc::uid(), "kind": kind, "x": r.x, "y": r.y, "w": r.w.max(40.0), "h": r.h.max(30.0), "text": "", "vertical": vertical, "size": size, "tail": tail })
}

// ── drawing ────────────────────────────────────────────────────────────

const INK: [u8; 3] = [0x11, 0x11, 0x11];
const PAPER_W: [u8; 3] = [255, 255, 255];

type Poly = Vec<(f32, f32)>;

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Poly {
    let n = 64;
    (0..n).map(|i| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        (cx + rx * a.cos(), cy + ry * a.sin())
    }).collect()
}

/// Strokes everything, then fills everything: shapes read as one outline.
fn outline(out: &mut Vec<Item>, shapes: Vec<Poly>, lw: f32, dash: Option<(f32, f32)>) {
    for s in &shapes {
        out.push(Item::Path { polys: vec![s.clone()], fill: None, fill_alpha: 1.0, stroke: Some(Pen { width: lw * 2.0, color: INK, dash }) });
    }
    for s in shapes {
        out.push(Item::Path { polys: vec![s], fill: Some(PAPER_W), fill_alpha: 1.0, stroke: None });
    }
}

fn tail(bl: &Json, width: f32) -> Option<Poly> {
    let t = &bl["tail"];
    if !t.is_object() {
        return None;
    }
    let r = rect_of(bl);
    let (tx, ty) = (num(&t["x"], 0.0), num(&t["y"], 0.0));
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let ang = (ty - cy).atan2(tx - cx);
    let base = r.w.min(r.h) * width;
    let (nx, ny) = (-ang.sin(), ang.cos());
    // the base a little inside, so the fill hides the seam
    let (bx, by) = (cx + ang.cos() * r.w / 2.0 * 0.7, cy + ang.sin() * r.h / 2.0 * 0.7);
    Some(vec![(bx + nx * base, by + ny * base), (tx, ty), (bx - nx * base, by - ny * base)])
}

const BALLOON_FONTS: [&str; 7] = ["Hiragino Sans", "Hiragino Kaku Gothic ProN", "Noto Sans CJK JP", "Noto Sans JP", "Yu Gothic", "Meiryo", "Inter"];

/// A balloon (speech, thought, shout, whisper, caption, sound effect) and its text.
pub fn balloon(bl: &Json, db: &FontDb) -> Vec<Item> {
    let mut out = Vec::new();
    let r = rect_of(bl);
    let size = num(&bl["size"], 28.0);
    let lw = (size / 9.0).max(1.5);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let kind = bl["kind"].as_str().unwrap_or("speech");
    match kind {
        "speech" | "whisper" => {
            let mut shapes = Vec::new();
            if let Some(t) = tail(bl, 0.12) {
                shapes.push(t);
            }
            shapes.push(ellipse(cx, cy, r.w / 2.0, r.h / 2.0));
            outline(&mut out, shapes, lw, (kind == "whisper").then_some((lw * 3.0, lw * 2.5)));
        }
        "thought" => {
            let mut shapes = Vec::new();
            let n = 11;
            let rr = r.w.min(r.h) * 0.2;
            for i in 0..n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                shapes.push(ellipse(cx + a.cos() * (r.w / 2.0 - rr * 0.6), cy + a.sin() * (r.h / 2.0 - rr * 0.6), rr, rr * 0.85));
            }
            shapes.push(ellipse(cx, cy, r.w * 0.42, r.h * 0.4));
            let t = &bl["tail"];
            if t.is_object() {
                let (tx, ty) = (num(&t["x"], 0.0), num(&t["y"], 0.0));
                for k in 1..=3 {
                    let tt = k as f32 / 4.0;
                    let rk = r.w.min(r.h) * (0.09 - k as f32 * 0.018);
                    shapes.push(ellipse(cx + (tx - cx) * (0.55 + tt * 0.45), cy + (ty - cy) * (0.55 + tt * 0.45), rk.max(3.0), (rk * 0.8).max(3.0)));
                }
            }
            outline(&mut out, shapes, lw, None);
        }
        "shout" => {
            let n = 18;
            let star: Poly = (0..n * 2).map(|i| {
                let a = i as f32 / (n * 2) as f32 * std::f32::consts::TAU;
                let k = if i % 2 == 1 { 0.78 } else { 1.08 };
                (cx + a.cos() * r.w / 2.0 * k, cy + a.sin() * r.h / 2.0 * k)
            }).collect();
            let mut shapes = Vec::new();
            if let Some(t) = tail(bl, 0.1) {
                shapes.push(t);
            }
            shapes.push(star);
            outline(&mut out, shapes, lw, None);
        }
        "narration" => {
            out.push(Item::Path { polys: vec![vec![(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)]], fill: Some(PAPER_W), fill_alpha: 1.0, stroke: Some(Pen { width: lw, color: INK, dash: None }) });
        }
        _ => {}
    }
    out.extend(balloon_text(bl, db));
    out
}

/// Text inside a balloon: wrapped lines, or vertical columns right to left.
fn balloon_text(bl: &Json, db: &FontDb) -> Vec<Item> {
    let text = bl["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return Vec::new();
    }
    let kind = bl["kind"].as_str().unwrap_or("speech");
    let sfx = kind == "sfx";
    let size = num(&bl["size"], 28.0);
    // the units are pixels: a size of `size` px is size / PT_MM "pt"
    let pt = size / PT_MM;
    let font = compose::first_font(db, &BALLOON_FONTS, false, if sfx || kind == "shout" { 800 } else { 500 });
    let r = rect_of(bl);
    let k = if kind == "narration" || sfx { 0.9 } else { 0.72 };
    let (iw, ih) = (r.w * k, r.h * k);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let face = db.face(font);
    let mid = (face.ascender + face.descender) / 2.0 / face.upem * size;
    let mut items = Vec::new();
    let mut put = |s: &str, x: f32, y: f32, centered: bool, turn: bool| {
        let (f, glyphs, map, w) = compose::run(db, font, s, pt, 0.0);
        let x0 = if centered { x - w / 2.0 } else { x };
        let t = Item::Text { font: f, size: pt, x: x0, y: y + mid, color: INK, glyphs, text: s.to_string(), map };
        let t = if turn { Item::Group { m: [0.0, 1.0, -1.0, 0.0, x + y, y - x], clip: None, alpha: 1.0, items: vec![t] } } else { t };
        if sfx {
            let halo = match &t {
                Item::Group { m, items: inner, .. } => Item::Group { m: *m, clip: None, alpha: 1.0, items: vec![Item::Outline { text: Box::new(inner[0].clone()), width: (size / 5.0).max(3.0), color: PAPER_W }] },
                _ => Item::Outline { text: Box::new(t.clone()), width: (size / 5.0).max(3.0), color: PAPER_W },
            };
            items.push(halo);
        }
        items.push(t);
    };
    if bl["vertical"].as_bool().unwrap_or(false) {
        let (line_h, col_w) = (size * 1.08, size * 1.3);
        let per = ((ih / line_h).floor() as usize).max(1);
        let mut cols: Vec<Vec<char>> = Vec::new();
        for para in text.split('\n') {
            let chars: Vec<char> = para.chars().collect();
            if chars.is_empty() {
                cols.push(Vec::new());
            }
            for c in chars.chunks(per) {
                cols.push(c.to_vec());
            }
        }
        let total = cols.len() as f32 * col_w;
        for (i, col) in cols.iter().enumerate() {
            let x = cx + total / 2.0 - col_w * (i as f32 + 0.5);
            let top = cy - col.len() as f32 * line_h / 2.0 + line_h / 2.0;
            for (j, ch) in col.iter().enumerate() {
                let y = top + j as f32 * line_h;
                // long vowel marks and dashes turn upright in vertical text
                let turn = "ー—–-〜~…".contains(*ch);
                put(&ch.to_string(), x, y, true, turn);
            }
        }
        return items;
    }
    let mut lines: Vec<String> = Vec::new();
    let width = |s: &str| compose::run(db, font, s, pt, 0.0).3;
    for para in text.split('\n') {
        // words, or single characters for scripts without spaces (CJK)
        let tokens: Vec<String> = if para.contains(char::is_whitespace) { para.split_inclusive(char::is_whitespace).map(String::from).collect() } else { para.chars().map(String::from).collect() };
        let mut cur = String::new();
        for t in tokens {
            let next = format!("{cur}{t}");
            if width(next.trim()) > iw && !cur.trim().is_empty() {
                lines.push(cur.trim().to_string());
                cur = t.trim_start().to_string();
            } else {
                cur = next;
            }
        }
        lines.push(cur.trim().to_string());
    }
    let line_h = size * 1.2;
    let top = cy - lines.len() as f32 * line_h / 2.0 + line_h / 2.0;
    for (i, l) in lines.iter().enumerate() {
        put(l, cx, top + i as f32 * line_h, true, false);
    }
    items
}

/// What a page draws over its layers: paper outside the panels, the
/// panels' borders, the balloons and (optionally) the reading order.
pub fn decorations(board: &Json, frame: &Json, db: &FontDb, numbers: bool) -> Vec<Item> {
    let page = &board["page"];
    let mut out = Vec::new();
    if !page.is_object() {
        return out;
    }
    let (w, h) = (num(&board["width"], 1920.0), num(&board["height"], 1080.0));
    let empty = vec![];
    let panels = frame["panels"].as_array().unwrap_or(&empty);
    let bg = board["background"].as_str().unwrap_or("#ffffff");
    if !panels.is_empty() && page["mask"].as_bool().unwrap_or(true) && bg != "transparent" {
        let paper = crate::board::tile::parse_color(bg);
        let mut polys: Vec<Poly> = vec![vec![(-1.0, -1.0), (w + 1.0, -1.0), (w + 1.0, h + 1.0), (-1.0, h + 1.0)]];
        for p in panels {
            let mut pts = points(p);
            // holes wind the other way
            if area(&pts) > 0.0 {
                pts.reverse();
            }
            polys.push(pts);
        }
        out.push(Item::Path { polys, fill: Some(paper), fill_alpha: 1.0, stroke: None });
    }
    let border = num(&page["border"], 4.0);
    if !panels.is_empty() && border > 0.0 {
        for p in panels {
            out.push(Item::Path { polys: vec![points(p)], fill: None, fill_alpha: 1.0, stroke: Some(Pen { width: border, color: INK, dash: None }) });
        }
    }
    for bl in frame["balloons"].as_array().unwrap_or(&empty) {
        out.extend(balloon(bl, db));
    }
    if numbers && panels.len() > 1 {
        let rtl = page["rtl"].as_bool().unwrap_or(false);
        let s = (w.min(h) / 60.0).max(12.0);
        let pt = s / PT_MM;
        let font = compose::first_font(db, &["Inter", "Helvetica Neue", "Arial"], false, 600);
        for (k, i) in reading_order(panels, rtl).into_iter().enumerate() {
            let r = bounds(&points(&panels[i]));
            let star = panels[i]["star"].as_bool().unwrap_or(false);
            let label = format!("{}{}", k + 1, if star { " ★" } else { "" });
            let (f, glyphs, map, tw) = compose::run(db, font, &label, pt, 0.0);
            if board["kind"] == "thumbnails" {
                out.push(Item::Text { font: f, size: pt, x: r.x, y: r.y + r.h + s * 1.2, color: if star { [0xc7, 0x7d, 0x00] } else { [0x55, 0x55, 0x55] }, glyphs, text: label, map });
            } else {
                let cw = tw + s * 0.6;
                let x = if rtl { r.x + r.w - cw - s * 0.3 } else { r.x + s * 0.3 };
                out.push(Item::Path { polys: vec![vec![(x, r.y + s * 0.3), (x + cw, r.y + s * 0.3), (x + cw, r.y + s * 1.6), (x, r.y + s * 1.6)]], fill: Some([255, 255, 255]), fill_alpha: 0.85, stroke: None });
                out.push(Item::Text { font: f, size: pt, x: x + s * 0.3, y: r.y + s * 1.3, color: [0x2f, 0x6f, 0xd6], glyphs, text: label, map });
            }
        }
    }
    out
}

// ── export ─────────────────────────────────────────────────────────────

/// A page with what is drawn on it and its decorations, in pixels.
fn page_of(doc: &mut BoardDoc, pics: &mut super::compose::Pictures, db: &FontDb, i: usize, numbers: bool) -> (crate::typeset::Page, crate::typeset::Picture) {
    let (w, h, px) = super::compose::frame_image(doc, pics, i, 0, true);
    let pic = crate::typeset::Picture { w, h, rgba: px, jpeg: None };
    let mut pg = crate::typeset::Page::new(w as f32, h as f32, i + 1, crate::typeset::PageKind::Fixed, [255, 255, 255]);
    pg.items.push(Item::Image { pic: 0, x: 0.0, y: 0.0, w: w as f32, h: h as f32 });
    let frame = doc.frame(i).clone();
    pg.items.extend(decorations(&doc.board, &frame, db, numbers));
    (pg, pic)
}

/// Page i as straight RGBA at `scale` (1 = the page's pixels).
pub fn page_image(doc: &mut BoardDoc, pics: &mut super::compose::Pictures, db: &FontDb, i: usize, scale: f32, numbers: bool) -> (u32, u32, Vec<u8>) {
    let (pg, pic) = page_of(doc, pics, db, i, numbers);
    crate::typeset::raster::render(&pg, db, &[pic], scale)
}

/// The pages as a PDF at print size (pixels at the page's dpi), with the
/// trim inside the bleed.
pub fn pages_pdf(doc: &mut BoardDoc, pics: &mut super::compose::Pictures, db: &FontDb, title: &str) -> Result<Vec<u8>, String> {
    let dpi = num(&doc.board["page"]["dpi"], 150.0).max(1.0);
    let bleed = num(&doc.board["page"]["bleed"], 0.0);
    let k = 25.4 / dpi;
    let mut pages = Vec::new();
    let mut pictures = Vec::new();
    for i in 0..doc.frame_count() {
        let (pg, pic) = page_of(doc, pics, db, i, false);
        let n = pictures.len();
        pictures.push(pic);
        let items: Vec<Item> = pg.items.into_iter().map(|it| match it {
            Item::Image { x, y, w, h, .. } => Item::Image { pic: n, x, y, w, h },
            o => o,
        }).collect();
        let mut out = crate::typeset::Page::new(pg.w * k, pg.h * k, i + 1, crate::typeset::PageKind::Fixed, [255, 255, 255]);
        out.items = vec![Item::Group { m: [k, 0.0, 0.0, k, 0.0, 0.0], clip: None, alpha: 1.0, items }];
        out.trim = (bleed * k, bleed * k, (pg.w - 2.0 * bleed) * k, (pg.h - 2.0 * bleed) * k);
        pages.push(out);
    }
    crate::typeset::pdf::write(&pages, db, &pictures, &crate::typeset::pdf::Meta { title: title.into(), author: String::new(), lang: String::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutting_and_order() {
        let p = rect_panel(R { x: 0.0, y: 0.0, w: 200.0, h: 300.0 });
        // a horizontal stroke across the panel: two tiers with the gutter between
        let cut = cut_panel(&p, (-10.0, 150.0), (210.0, 150.0), 20.0);
        assert_eq!(cut.len(), 2);
        let (a, b) = (bounds(&points(&cut[0])), bounds(&points(&cut[1])));
        let (top, bottom) = if a.y < b.y { (a, b) } else { (b, a) };
        assert!((top.h - 140.0).abs() < 0.5 && (bottom.y - 160.0).abs() < 0.5);
        assert!(!line_cuts(&points(&p), (-10.0, 400.0), (210.0, 400.0)));
        // two columns, read right to left
        let row = layout_panels(R { x: 0.0, y: 0.0, w: 200.0, h: 100.0 }, &[(1.0, &[1.0, 1.0])], 10.0, 10.0, false);
        let order = reading_order(&row, true);
        assert!(bounds(&points(&row[order[0]])).x > bounds(&points(&row[order[1]])).x);
    }

    #[test]
    fn pages_export() {
        let mut o = KindOptions::defaults("comic");
        o.layout = "big-top".into();
        o.pages = 2;
        o.dpi = 60.0;
        let mut d = board_for_kind(&o);
        d.board["frames"][0]["balloons"] = json!([new_balloon("speech", R { x: 60.0, y: 60.0, w: 120.0, h: 60.0 }, false, 14.0)]);
        d.board["frames"][0]["balloons"][0]["text"] = json!("Olá!");
        let db = FontDb::new(&[include_bytes!("../../../native/assets/fonts/Inter-Regular.ttf")]);
        let mut pics = super::super::compose::Pictures::default();
        let (w, h, px) = page_image(&mut d, &mut pics, &db, 0, 1.0, true);
        assert_eq!((w, h), (d.width(), d.height()));
        // the border of the first panel is drawn in ink at the inner frame's corner
        let inner = inner_rect(&d.board);
        let i = ((inner.y as u32 + 20) * w + inner.x as u32) as usize * 4;
        assert!(px[i] < 80, "border pixel {:?}", &px[i..i + 4]);
        let pdf = pages_pdf(&mut d, &mut pics, &db, "HQ").unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn page_kinds_start_with_panels() {
        let mut o = KindOptions::defaults("name");
        o.layout = "classic".into();
        let d = board_for_kind(&o);
        assert_eq!(d.frame_count(), 4);
        assert_eq!(d.board["frames"][0]["panels"].as_array().unwrap().len(), 6);
        // B5 at 150 dpi: 182 mm + 2 × 5 mm bleed
        assert_eq!(d.width(), (mm_px(182.0, 150.0) + 2.0 * mm_px(5.0, 150.0)) as u32);
        let t = board_for_kind(&KindOptions::defaults("thumbnails"));
        assert!(t.board["frames"][0]["panels"].as_array().unwrap().len() >= 4);
    }
}
