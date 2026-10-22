//! Composed pages (comics, manga, free pages, text over art) and the open
//! cover: a model of layers (panels, pictures, text, balloons, sound
//! effects, shapes) placed in percentages of a zone (the page, or the back,
//! spine, front and flaps of the cover) turned into page items.

use super::fonts::{FontDb, FontId};
use super::{Glyph, Item, Pen, IDENTITY, PT_MM};
use serde_json::{json, Value as Json};
use std::sync::atomic::{AtomicU64, Ordering};

pub const LAYER_TYPES: [(&str, &str); 6] = [("painel", "Painel (requadro)"), ("imagem", "Imagem"), ("texto", "Texto"), ("balao", "Balão"), ("sfx", "Onomatopeia (SFX)"), ("forma", "Forma")];

pub const BALLOON_TYPES: [(&str, &str); 6] = [("fala", "Fala"), ("pensamento", "Pensamento"), ("grito", "Grito"), ("sussurro", "Sussurro"), ("narracao", "Narração"), ("eletronico", "Eletrônico / rádio")];

pub const TEMPLATES: [(&str, &str); 17] = [
    ("livre", "Página livre (vazia)"),
    ("sangria", "Arte sangrada (página inteira)"),
    ("1", "1 painel"),
    ("2h", "2 faixas horizontais"),
    ("2v", "2 colunas"),
    ("3h", "3 faixas"),
    ("2x2", "Grade 2 × 2"),
    ("3x2", "Grade 3 × 2"),
    ("3x3", "Grade 3 × 3 (nove painéis)"),
    ("tira", "4 tiras"),
    ("L", "Grande em cima + 2 embaixo"),
    ("T", "2 em cima + grande embaixo"),
    ("manga-diag", "Mangá — diagonais"),
    ("manga-impacto", "Mangá — impacto com inserção"),
    ("ilustrado", "Ilustração + texto (illustra-novel)"),
    ("texto-sobre-arte", "Texto sobre arte sangrada"),
    ("texto-moldura", "Texto com moldura ilustrada"),
];

pub const FILTERS: [(&str, &str); 6] = [("", "Nenhum"), ("pb", "Preto e branco"), ("sepia", "Sépia"), ("contraste", "Alto contraste"), ("noite", "Noite"), ("desfoque", "Desfoque")];

pub const ZONE_NAMES: [(&str, &str); 6] = [("capa", "Capa"), ("contracapa", "Contracapa"), ("lombada", "Lombada"), ("orelha-esq", "Orelha esquerda"), ("orelha-dir", "Orelha direita"), ("tudo", "Capa aberta inteira")];

/// A fresh id for a layer ("p", "t"…).
pub fn uid(prefix: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let mut v = (t << 8) ^ n.wrapping_mul(2654435761);
    let mut s = String::new();
    while v > 0 && s.len() < 7 {
        let d = (v % 36) as u32;
        s.push(char::from_digit(d, 36).unwrap_or('0'));
        v /= 36;
    }
    format!("{prefix}{s}")
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zone {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where a model is drawn: its size, its zones, bleed and margins (mm).
#[derive(Clone, Debug)]
pub struct Env {
    /// One page.
    pub page_w: f32,
    pub page_h: f32,
    /// The whole surface (a spread, an open cover).
    pub w: f32,
    pub h: f32,
    pub zones: Vec<(String, Zone)>,
    pub bleed: f32,
    pub spine: f32,
    /// interna, externa, superior, inferior
    pub margins: [f32; 4],
    pub cover: bool,
}

impl Env {
    pub fn zone(&self, name: &str) -> Zone {
        let find = |n: &str| self.zones.iter().find(|z| z.0 == n).map(|z| z.1);
        find(name).or_else(|| find("pagina")).unwrap_or(Zone { x: 0.0, y: 0.0, w: self.w, h: self.h })
    }
}

fn f(v: &Json, d: f32) -> f32 {
    v.as_f64().map(|x| x as f32).or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(d)
}

fn st<'a>(v: &'a Json, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn margins(cfg: &Json) -> [f32; 4] {
    let m = &cfg["margens"];
    [f(&m["interna"], 18.0), f(&m["externa"], 14.0), f(&m["superior"], 16.0), f(&m["inferior"], 18.0)]
}

/// A page or a two-page spread (`tipo: dupla`) of a book with this layout.
pub fn page_env(model: &Json, cfg: &Json, print: bool) -> Env {
    let (w, h) = (f(&cfg["largura"], 140.0), f(&cfg["altura"], 210.0));
    let dupla = st(model, "tipo") == "dupla";
    let mut zones = vec![("pagina".to_string(), Zone { x: 0.0, y: 0.0, w: if dupla { 2.0 * w } else { w }, h })];
    if dupla {
        zones.push(("esquerda".into(), Zone { x: 0.0, y: 0.0, w, h }));
        zones.push(("direita".into(), Zone { x: w, y: 0.0, w, h }));
    }
    Env { page_w: w, page_h: h, w: if dupla { 2.0 * w } else { w }, h, zones, bleed: if print { f(&cfg["sangria"], 3.0) } else { 0.0 }, spine: 0.0, margins: margins(cfg), cover: false }
}

/// The open cover: flaps, back, spine (from the pages and paper unless
/// fixed), front.
pub fn cover_env(cover: &Json, cfg: &Json, pages: usize) -> Env {
    let (w, h) = (f(&cfg["largura"], 140.0), f(&cfg["altura"], 210.0));
    let hard = cover["capaDura"].as_bool().unwrap_or(false);
    let paper = Some(st(cover, "papel")).filter(|p| !p.is_empty()).or(cfg["papel"].as_str()).unwrap_or("polen80");
    let spine = if cover["lombadaAuto"] == false { f(&cover["lombada"], 10.0) } else { crate::scriptorium::book::spine_mm(pages.max(1), paper, hard) as f32 };
    let flap = f(&cover["orelha"], 0.0);
    let mut zones = Vec::new();
    let mut x = 0.0;
    if flap > 0.0 {
        zones.push(("orelha-esq".to_string(), Zone { x, y: 0.0, w: flap, h }));
        x += flap;
    }
    zones.push(("contracapa".into(), Zone { x, y: 0.0, w, h }));
    x += w;
    zones.push(("lombada".into(), Zone { x, y: 0.0, w: spine, h }));
    x += spine;
    zones.push(("capa".into(), Zone { x, y: 0.0, w, h }));
    x += w;
    if flap > 0.0 {
        zones.push(("orelha-dir".into(), Zone { x, y: 0.0, w: flap, h }));
        x += flap;
    }
    zones.push(("tudo".into(), Zone { x: 0.0, y: 0.0, w: x, h }));
    zones.push(("pagina".into(), Zone { x: 0.0, y: 0.0, w: x, h }));
    Env { page_w: w, page_h: h, w: x, h, zones, bleed: if hard { 18.0 } else { f(&cfg["sangria"], 3.0) }, spine, margins: margins(cfg), cover: true }
}

/// A layer's box in mm (the whole surface and its bleed when bled).
pub fn layer_rect(l: &Json, env: &Env) -> Zone {
    let b = env.bleed;
    let zn = st(l, "zona");
    let z = env.zone(zn);
    if l["sangria"].as_bool().unwrap_or(false) {
        if !zn.is_empty() && zn != "pagina" && zn != "tudo" && env.zones.iter().any(|q| q.0 == zn) {
            let mut r = Zone { x: z.x, y: -b, w: z.w, h: env.h + 2.0 * b };
            if z.x <= 0.01 {
                r.x -= b;
                r.w += b;
            }
            if z.x + z.w >= env.w - 0.01 {
                r.w += b;
            }
            return r;
        }
        return Zone { x: -b, y: -b, w: env.w + 2.0 * b, h: env.h + 2.0 * b };
    }
    let mut r = Zone { x: z.x + f(&l["x"], 0.0) / 100.0 * z.w, y: z.y + f(&l["y"], 0.0) / 100.0 * z.h, w: f(&l["w"], 10.0) / 100.0 * z.w, h: f(&l["h"], 10.0) / 100.0 * z.h };
    // pictures and shapes touching the paper's edge run on into the bleed
    let t = st(l, "t");
    let forma = st(l, "forma");
    if b > 0.0 && (t == "imagem" || t == "forma") && f(&l["rot"], 0.0) == 0.0 && forma != "elipse" && forma != "linha" {
        let e = 0.05;
        if r.x <= e {
            r.w += r.x + b;
            r.x = -b;
        }
        if r.y <= e {
            r.h += r.y + b;
            r.y = -b;
        }
        if r.x + r.w >= env.w - e {
            r.w = env.w + b - r.x;
        }
        if r.y + r.h >= env.h - e {
            r.h = env.h + b - r.y;
        }
    }
    r
}

pub fn zone_point(p: &Json, l: &Json, env: &Env) -> (f32, f32) {
    let z = env.zone(st(l, "zona"));
    (z.x + f(&p[0], 0.0) / 100.0 * z.w, z.y + f(&p[1], 0.0) / 100.0 * z.h)
}

pub fn color(v: &str) -> Option<[u8; 3]> {
    super::hex(v)
}

// ── text ───────────────────────────────────────────────────────────────

/// What a text box needs.
pub struct TextBox<'a> {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: &'a str,
    pub font: &'a str,
    /// "balao", "sans" or "" (serif)
    pub kind: &'a str,
    pub size: f32,
    pub color: [u8; 3],
    pub align: &'a str,
    pub valign: &'a str,
    pub lh: f32,
    pub weight: u16,
    pub italic: bool,
    /// letter spacing, pt
    pub spacing: f32,
    /// halo, pt
    pub stroke: f32,
    pub stroke_color: [u8; 3],
    pub vertical: bool,
    pub upper: bool,
}

fn pick_font(db: &FontDb, family: &str, kind: &str, italic: bool, weight: u16) -> FontId {
    let generic: &[&str] = if kind == "balao" {
        &["Comic Neue", "Marker Felt", "Chalkboard SE", "Comic Sans MS", "Inter"]
    } else if kind == "sans" || family.eq_ignore_ascii_case("helvetica") || family.eq_ignore_ascii_case("impact") {
        &["Impact", "Helvetica Neue", "Helvetica", "Arial", "Inter"]
    } else if family.to_lowercase().contains("mono") || family.to_lowercase().contains("courier") {
        &["Courier New", "Menlo", "DejaVu Sans Mono", "EB Garamond"]
    } else {
        &["EB Garamond"]
    };
    std::iter::once(family).filter(|f| !f.trim().is_empty()).chain(generic.iter().copied()).find_map(|f| db.find(f, italic, weight)).unwrap_or(0)
}

/// Shaped text: glyphs (mm from the start), the glyph→text map, width.
struct Shaped {
    font: FontId,
    glyphs: Vec<Glyph>,
    map: Vec<(u16, String)>,
    w: f32,
}

fn shape(db: &FontDb, font: FontId, text: &str, size: f32, spacing: f32) -> Shaped {
    let mut font = font;
    let mut s = db.shape(font, text, &["kern", "liga"]);
    if s.iter().any(|g| g.id == 0) {
        for c in ["EB Garamond", "Cormorant Garamond", "Inter", "Noto Sans Symbols 2", "DejaVu Sans"].iter().filter_map(|f| db.find(f, false, 400)) {
            let s2 = db.shape(c, text, &["kern", "liga"]);
            if !s2.iter().any(|g| g.id == 0) {
                font = c;
                s = s2;
                break;
            }
        }
    }
    let k = size * PT_MM / db.face(font).upem;
    let mut pen = 0.0;
    let mut glyphs = Vec::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    for (i, g) in s.iter().enumerate() {
        glyphs.push(Glyph { id: g.id, x: pen + g.dx as f32 * k, y: -g.dy as f32 * k });
        let end = s.iter().skip(i + 1).map(|o| o.cluster).find(|c| *c != g.cluster).unwrap_or(text.len() as u32);
        let (a, b) = (g.cluster.min(end) as usize, g.cluster.max(end) as usize);
        map.push((g.id, text.get(a..b).unwrap_or("").to_string()));
        pen += g.adv as f32 * k + spacing;
    }
    Shaped { font, glyphs, map, w: pen }
}

struct Line {
    words: Vec<Shaped>,
    w: f32,
    end: bool,
}

fn wrap(db: &FontDb, font: FontId, text: &str, max_w: f32, size: f32, spacing: f32) -> (Vec<Line>, f32) {
    let space = shape(db, font, " ", size, spacing).w.max(size * PT_MM * 0.25);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur: Vec<Shaped> = Vec::new();
        let mut cw = 0.0;
        for word in para.split_whitespace() {
            let sw = shape(db, font, word, size, spacing);
            if !cur.is_empty() && cw + space + sw.w > max_w {
                lines.push(Line { words: std::mem::take(&mut cur), w: cw, end: false });
                cw = 0.0;
            }
            if cur.is_empty() && sw.w > max_w && word.chars().count() > 3 {
                // a word wider than the box: broken by characters
                let mut chunk = String::new();
                for ch in word.chars() {
                    let t = format!("{chunk}{ch}");
                    if !chunk.is_empty() && shape(db, font, &t, size, spacing).w > max_w {
                        let s = shape(db, font, &chunk, size, spacing);
                        lines.push(Line { w: s.w, words: vec![s], end: false });
                        chunk = ch.to_string();
                    } else {
                        chunk = t;
                    }
                }
                let s = shape(db, font, &chunk, size, spacing);
                cw = s.w;
                cur.push(s);
                continue;
            }
            cw += if cur.is_empty() { sw.w } else { space + sw.w };
            cur.push(sw);
        }
        lines.push(Line { words: cur, w: cw, end: true });
    }
    (lines, space)
}

fn text_item(s: &Shaped, size: f32, x: f32, y: f32, color: [u8; 3]) -> Item {
    Item::Text { font: s.font, size, x, y, color, glyphs: s.glyphs.clone(), text: s.map.iter().map(|m| m.1.as_str()).collect(), map: s.map.clone() }
}

/// A block of text in a box, as items (with its halo under it).
pub fn text_box(o: &TextBox, db: &FontDb) -> Vec<Item> {
    let size_mm = o.size * PT_MM;
    let lh = size_mm * o.lh;
    let text = if o.upper { o.text.to_uppercase() } else { o.text.to_string() };
    let font = pick_font(db, o.font, o.kind, o.italic, o.weight);
    let sp = o.spacing * PT_MM;
    let mut words: Vec<Item> = Vec::new();
    if o.vertical {
        // columns from right to left, letters turned a quarter
        let (cols, space) = wrap(db, font, &text, o.h.max(1.0), o.size, sp);
        let total = cols.len() as f32 * lh;
        let mut x0 = o.x + o.w - (o.w - total) / 2.0 - lh / 2.0;
        if o.align == "right" {
            x0 = o.x + o.w - lh / 2.0;
        }
        let face = db.face(font);
        let central = (face.ascender + face.descender) / 2.0 / face.upem * size_mm;
        for (i, c) in cols.iter().enumerate() {
            let cx = x0 - i as f32 * lh;
            let m = [0.0, 1.0, -1.0, 0.0, cx + o.y, o.y - cx];
            let mut pen = cx;
            let mut its = Vec::new();
            for w in &c.words {
                its.push(text_item(w, o.size, pen, o.y + central, o.color));
                pen += w.w + space;
            }
            words.push(Item::Group { m, clip: None, alpha: 1.0, items: its });
        }
    } else {
        let (lines, space) = wrap(db, font, &text, o.w.max(1.0), o.size, sp);
        let total = lines.len() as f32 * lh;
        let mut y0 = o.y + size_mm * 0.8 + (lh - size_mm) / 2.0;
        match o.valign {
            "middle" => y0 += (o.h - total) / 2.0,
            "bottom" => y0 += o.h - total,
            _ => {}
        }
        for (i, l) in lines.iter().enumerate() {
            let y = y0 + i as f32 * lh;
            let just = o.align == "justify" && !l.end && l.words.len() > 1;
            let gap = if just { space + (o.w - l.w) / (l.words.len() - 1) as f32 } else { space };
            let mut x = match o.align {
                "center" => o.x + (o.w - l.w) / 2.0,
                "right" => o.x + o.w - l.w,
                _ => o.x,
            };
            for w in &l.words {
                words.push(text_item(w, o.size, x, y, o.color));
                x += w.w + gap;
            }
        }
    }
    if o.stroke > 0.0 {
        let halo = |it: &Item| -> Item {
            match it {
                Item::Group { m, items, .. } => Item::Group { m: *m, clip: None, alpha: 1.0, items: items.iter().map(|t| Item::Outline { text: Box::new(t.clone()), width: o.stroke * PT_MM, color: o.stroke_color }).collect() },
                t => Item::Outline { text: Box::new(t.clone()), width: o.stroke * PT_MM, color: o.stroke_color },
            }
        };
        let mut out: Vec<Item> = words.iter().map(halo).collect();
        out.extend(words);
        return out;
    }
    words
}

// ── shapes ─────────────────────────────────────────────────────────────

type Poly = Vec<(f32, f32)>;

fn rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Poly {
    if r <= 0.0 {
        return vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut p = Vec::new();
    for (cx, cy, a0) in [(x + w - r, y + r, -90.0f32), (x + w - r, y + h - r, 0.0), (x + r, y + h - r, 90.0), (x + r, y + r, 180.0)] {
        for i in 0..=8 {
            let a = (a0 + i as f32 * 90.0 / 8.0).to_radians();
            p.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    p
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Poly {
    let n = 72;
    (0..n).map(|i| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        (cx + rx * a.cos(), cy + ry * a.sin())
    }).collect()
}

fn quad(p: &mut Poly, a: (f32, f32), c: (f32, f32), b: (f32, f32)) {
    for i in 1..=12 {
        let t = i as f32 / 12.0;
        let u = 1.0 - t;
        p.push((u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1));
    }
}

/// An arc of radius `r` from `a` to `b`, clockwise on the page (the small one).
fn arc(p: &mut Poly, a: (f32, f32), b: (f32, f32), r: f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let d = dx.hypot(dy);
    if d < 1e-6 {
        return;
    }
    let r = r.max(d / 2.0);
    let hlen = (r * r - d * d / 4.0).max(0.0).sqrt();
    let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    // the centre on the side that makes the sweep clockwise (y down)
    let c = (m.0 - dy / d * hlen, m.1 + dx / d * hlen);
    let a0 = (a.1 - c.1).atan2(a.0 - c.0);
    let mut a1 = (b.1 - c.1).atan2(b.0 - c.0);
    while a1 < a0 {
        a1 += std::f32::consts::TAU;
    }
    if a1 - a0 > std::f32::consts::PI {
        a1 -= std::f32::consts::TAU;
    }
    for i in 1..=10 {
        let t = a0 + (a1 - a0) * i as f32 / 10.0;
        p.push((c.0 + r * t.cos(), c.1 + r * t.sin()));
    }
}

fn hash_seed(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for c in s.encode_utf16() {
        h ^= c as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Balloon shapes: (under, main, over) items and the text's box.
fn balloon(l: &Json, r: Zone, tail: Option<(f32, f32)>, sw: f32, fill: [u8; 3], ink: [u8; 3]) -> (Vec<Item>, Zone) {
    let (cx, cy, rx, ry) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0);
    let forma = Some(st(l, "forma")).filter(|s| !s.is_empty()).unwrap_or("fala");
    let pen = Some(Pen { width: sw, color: ink, dash: None });
    let path = |p: Poly, stroke: Option<Pen>| Item::Path { polys: vec![p], fill: Some(fill), fill_alpha: 1.0, stroke };
    let inside = tail.is_some_and(|t| ((t.0 - cx) / rx).powi(2) + ((t.1 - cy) / ry).powi(2) < 1.05);
    let tail_poly = |straight: bool| -> Option<Poly> {
        let t = tail.filter(|_| !inside)?;
        let th = ((t.1 - cy) / ry).atan2((t.0 - cx) / rx);
        let d = f(&l["caudaLargura"], 0.22);
        let b1 = (cx + rx * (th - d).cos(), cy + ry * (th - d).sin());
        let b2 = (cx + rx * (th + d).cos(), cy + ry * (th + d).sin());
        if straight {
            return Some(vec![b1, t, b2]);
        }
        let curve = l["caudaCurva"].as_f64().map(|v| v as f32).unwrap_or(0.25);
        let (dx, dy) = (t.0 - cx, t.1 - cy);
        let (px, py) = (-dy * curve * 0.35, dx * curve * 0.35);
        let c1 = ((b1.0 + t.0) / 2.0 + px, (b1.1 + t.1) / 2.0 + py);
        let c2 = ((b2.0 + t.0) / 2.0 + px * 0.6, (b2.1 + t.1) / 2.0 + py * 0.6);
        let mut p = vec![b1];
        quad(&mut p, b1, c1, t);
        quad(&mut p, t, c2, b2);
        Some(p)
    };
    let with_tail = |main: Poly, straight: bool, dash: Option<(f32, f32)>| -> Vec<Item> {
        let pen = Some(Pen { width: sw, color: ink, dash });
        let mut v = Vec::new();
        let tp = tail_poly(straight);
        if let Some(t) = &tp {
            v.push(path(t.clone(), pen));
        }
        v.push(path(main, pen));
        if let Some(t) = tp {
            // the tail's join hidden under the balloon's fill
            v.push(Item::Path { polys: vec![t], fill: Some(fill), fill_alpha: 1.0, stroke: None });
        }
        v
    };
    match forma {
        "narracao" => {
            let bg = color(st(l, "fundo")).unwrap_or([0xf4, 0xe9, 0xc8]);
            (vec![Item::Path { polys: vec![rect(r.x, r.y, r.w, r.h, 0.0)], fill: Some(bg), fill_alpha: 1.0, stroke: pen }], Zone { x: r.x + 1.6, y: r.y + 1.2, w: r.w - 3.2, h: r.h - 2.4 })
        }
        "grito" => {
            let mut s = hash_seed(st(l, "id")).max(1);
            let mut rnd = || {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                s as f32 / 4294967296.0
            };
            let n = 14 + (rnd() * 6.0).round() as usize;
            let pts: Poly = (0..n * 2).map(|k| {
                let a = std::f32::consts::TAU * k as f32 / (n * 2) as f32;
                let s = if k % 2 == 1 { 0.86 + rnd() * 0.06 } else { 1.08 + rnd() * 0.12 };
                (cx + rx * s * a.cos(), cy + ry * s * a.sin())
            }).collect();
            (with_tail(pts, true, None), Zone { x: cx - rx * 0.66, y: cy - ry * 0.6, w: rx * 1.32, h: ry * 1.2 })
        }
        "pensamento" => {
            let n = 11;
            let pts: Poly = (0..=n).map(|k| {
                let a = std::f32::consts::TAU * k as f32 / n as f32;
                (cx + rx * 0.92 * a.cos(), cy + ry * 0.92 * a.sin())
            }).collect();
            let mut p = vec![pts[0]];
            for k in 1..=n {
                let c = (pts[k].0 - pts[k - 1].0).hypot(pts[k].1 - pts[k - 1].1);
                arc(&mut p, pts[k - 1], pts[k], c * 0.62);
            }
            let mut v = vec![path(p, pen)];
            if let Some(t) = tail.filter(|_| !inside) {
                let th = ((t.1 - cy) / ry).atan2((t.0 - cx) / rx);
                let e = (cx + rx * th.cos(), cy + ry * th.sin());
                let base = rx.min(ry);
                for (tt, s) in [(0.28, 0.17), (0.58, 0.11), (0.86, 0.065)] {
                    let b = (e.0 + (t.0 - e.0) * tt, e.1 + (t.1 - e.1) * tt);
                    v.push(path(ellipse(b.0, b.1, base * s, base * s), pen));
                }
            }
            (v, Zone { x: cx - rx * 0.64, y: cy - ry * 0.6, w: rx * 1.28, h: ry * 1.2 })
        }
        "eletronico" => {
            let mut pts = Vec::new();
            let step = 2.4;
            let mut edge = |x0: f32, y0: f32, x1: f32, y1: f32| {
                let len = (x1 - x0).hypot(y1 - y0);
                let n = ((len / step).round() as usize).max(2);
                for k in 0..n {
                    let t = k as f32 / n as f32;
                    let off = if k % 2 == 1 { 0.9 } else { -0.9 };
                    let (nx, ny) = (-(y1 - y0) / len, (x1 - x0) / len);
                    pts.push((x0 + (x1 - x0) * t + nx * off, y0 + (y1 - y0) * t + ny * off));
                }
            };
            edge(r.x, r.y, r.x + r.w, r.y);
            edge(r.x + r.w, r.y, r.x + r.w, r.y + r.h);
            edge(r.x + r.w, r.y + r.h, r.x, r.y + r.h);
            edge(r.x, r.y + r.h, r.x, r.y);
            (with_tail(pts, true, None), Zone { x: r.x + 2.0, y: r.y + 1.6, w: r.w - 4.0, h: r.h - 3.2 })
        }
        _ => {
            let dash = (forma == "sussurro").then_some((sw * 3.0, sw * 2.4));
            (with_tail(ellipse(cx, cy, rx, ry), false, dash), Zone { x: cx - rx * 0.72, y: cy - ry * 0.68, w: rx * 1.44, h: ry * 1.36 })
        }
    }
}

fn rotate_about(deg: f32, cx: f32, cy: f32) -> [f32; 6] {
    let (s, c) = deg.to_radians().sin_cos();
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

/// A picture for a layer: (index among the pictures, width, height) px.
pub type Pictures<'a> = dyn Fn(&str, &str) -> Option<(usize, u32, u32)> + 'a;

/// The model's layers as items, in mm from the top left of the trim
/// (the bleed lies at negative coordinates and past the size).
pub fn render(model: &Json, env: &Env, db: &FontDb, pics: &Pictures) -> Vec<Item> {
    let b = env.bleed;
    let mut out = Vec::new();
    let bg = color(st(model, "fundo")).unwrap_or(if env.cover { [0x1b, 0x2a, 0x2c] } else { [255, 255, 255] });
    out.push(Item::Rect { x: -b, y: -b, w: env.w + 2.0 * b, h: env.h + 2.0 * b, color: bg });
    let empty = vec![];
    for l in model["camadas"].as_array().unwrap_or(&empty) {
        if l["oculto"].as_bool().unwrap_or(false) {
            continue;
        }
        out.extend(layer(l, env, db, pics));
    }
    out
}

/// One layer as items.
pub fn layer(l: &Json, env: &Env, db: &FontDb, pics: &Pictures) -> Vec<Item> {
    let r = layer_rect(l, env);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let rot = f(&l["rot"], 0.0);
    let opac = l["opac"].as_f64().map(|v| v as f32).unwrap_or(1.0);
    let t = st(l, "t");
    let mut its: Vec<Item> = Vec::new();
    let mut m = if rot != 0.0 { rotate_about(rot, cx, cy) } else { IDENTITY };
    let ink = |k: &str, d: [u8; 3]| color(st(l, k)).unwrap_or(d);
    match t {
        "painel" | "imagem" => {
            let shape: Poly = if let (true, Some(pts)) = (t == "painel", l["pts"].as_array().filter(|p| p.len() >= 3)) {
                pts.iter().map(|p| zone_point(p, l, env)).collect()
            } else if st(l, "forma") == "elipse" {
                ellipse(cx, cy, r.w / 2.0, r.h / 2.0)
            } else {
                rect(r.x, r.y, r.w, r.h, f(&l["raio"], 0.0))
            };
            let mut inner = Vec::new();
            let fill = color(st(l, "fundo")).or((t == "painel").then_some([255, 255, 255]));
            if let Some(c) = fill {
                inner.push(Item::Path { polys: vec![shape.clone()], fill: Some(c), fill_alpha: 1.0, stroke: None });
            }
            let img = st(l, "img");
            if !img.is_empty() {
                if let Some((pic, pw, ph)) = pics(img, st(l, "filtro")) {
                    let z = f(&l["zoom"], 1.0);
                    let (iw, ih) = (r.w * z, r.h * z);
                    let ix = r.x + (r.w - iw) / 2.0 + f(&l["ox"], 0.0) / 100.0 * r.w;
                    let iy = r.y + (r.h - ih) / 2.0 + f(&l["oy"], 0.0) / 100.0 * r.h;
                    let (pw, ph) = (pw.max(1) as f32, ph.max(1) as f32);
                    let (dw, dh) = match st(l, "ajuste") {
                        "esticar" => (iw, ih),
                        "conter" => {
                            let k = (iw / pw).min(ih / ph);
                            (pw * k, ph * k)
                        }
                        _ => {
                            let k = (iw / pw).max(ih / ph);
                            (pw * k, ph * k)
                        }
                    };
                    let (dx, dy) = (ix + (iw - dw) / 2.0, iy + (ih - dh) / 2.0);
                    let im = Item::Image { pic, x: dx, y: dy, w: dw, h: dh };
                    if l["espelhar"].as_bool().unwrap_or(false) {
                        inner.push(Item::Group { m: [-1.0, 0.0, 0.0, 1.0, 2.0 * (ix + iw / 2.0), 0.0], clip: None, alpha: 1.0, items: vec![im] });
                    } else {
                        inner.push(im);
                    }
                }
            }
            its.push(Item::Group { m: IDENTITY, clip: Some(vec![shape.clone()]), alpha: 1.0, items: inner });
            let bw = if l["borda"].is_null() { if t == "painel" && !l["sangria"].as_bool().unwrap_or(false) { 0.6 } else { 0.0 } } else { f(&l["borda"], 0.0) };
            if bw > 0.0 {
                its.push(Item::Path { polys: vec![shape], fill: None, fill_alpha: 1.0, stroke: Some(Pen { width: bw, color: ink("corBorda", [0x11; 3]), dash: None }) });
            }
        }
        "texto" => {
            let pad = f(&l["pad"], 0.0);
            if let Some(bg) = color(st(l, "fundo")) {
                let bw = f(&l["borda"], 0.0);
                its.push(Item::Path { polys: vec![rect(r.x, r.y, r.w, r.h, f(&l["raio"], 0.0))], fill: Some(bg), fill_alpha: f(&l["fundoOpac"], 1.0), stroke: (bw > 0.0).then(|| Pen { width: bw, color: ink("corBorda", [0x11; 3]), dash: None }) });
            }
            its.extend(text_box(
                &TextBox {
                    x: r.x + pad,
                    y: r.y + pad,
                    w: r.w - 2.0 * pad,
                    h: r.h - 2.0 * pad,
                    text: st(l, "texto"),
                    font: st(l, "fonte"),
                    kind: "",
                    size: f(&l["tam"], 11.0),
                    color: ink("cor", [0x11; 3]),
                    align: Some(st(l, "alin")).filter(|s| !s.is_empty()).unwrap_or("left"),
                    valign: Some(st(l, "valign")).filter(|s| !s.is_empty()).unwrap_or("top"),
                    lh: f(&l["entre"], 1.35),
                    weight: f(&l["peso"], 400.0) as u16,
                    italic: l["ital"].as_bool().unwrap_or(false),
                    spacing: f(&l["espaco"], 0.0),
                    stroke: f(&l["contorno"], 0.0),
                    stroke_color: ink("corContorno", [255; 3]),
                    vertical: l["vertical"].as_bool().unwrap_or(false),
                    upper: st(l, "caixa") == "alta",
                },
                db,
            ));
        }
        "balao" => {
            let z = env.zone(st(l, "zona"));
            let tail = l["cauda"].as_array().filter(|c| c.len() == 2).map(|c| (z.x + f(&c[0], 0.0) / 100.0 * z.w, z.y + f(&c[1], 0.0) / 100.0 * z.h));
            let sw = if l["borda"].is_null() { 0.45 } else { f(&l["borda"], 0.45) };
            let (parts, bx) = balloon(l, r, tail, sw, ink("fundo", [255; 3]), ink("corBorda", [0x11; 3]));
            its.extend(parts);
            let forma = st(l, "forma");
            its.extend(text_box(
                &TextBox {
                    x: bx.x,
                    y: bx.y,
                    w: bx.w,
                    h: bx.h,
                    text: st(l, "texto"),
                    font: st(l, "fonte"),
                    kind: "balao",
                    size: f(&l["tam"], 9.0),
                    color: ink("cor", [0x11; 3]),
                    align: Some(st(l, "alin")).filter(|s| !s.is_empty()).unwrap_or("center"),
                    valign: "middle",
                    lh: f(&l["entre"], 1.15),
                    weight: f(&l["peso"], if forma == "grito" { 700.0 } else { 400.0 }) as u16,
                    italic: l["ital"].as_bool().unwrap_or(false) || forma == "sussurro",
                    spacing: 0.0,
                    stroke: 0.0,
                    stroke_color: [255; 3],
                    vertical: l["vertical"].as_bool().unwrap_or(false),
                    upper: if st(l, "caixa").is_empty() { true } else { st(l, "caixa") == "alta" },
                },
                db,
            ));
        }
        "sfx" => {
            let skew = f(&l["inclina"], 0.0);
            if rot != 0.0 || skew != 0.0 {
                let k = skew.to_radians().tan();
                // rotate about the centre, then lean
                m = super::mul(&rotate_about(rot, cx, cy), &[1.0, 0.0, k, 1.0, -k * cy, 0.0]);
            }
            its.extend(text_box(
                &TextBox {
                    x: r.x,
                    y: r.y,
                    w: r.w,
                    h: r.h,
                    text: st(l, "texto"),
                    font: Some(st(l, "fonte")).filter(|s| !s.is_empty()).unwrap_or("Impact"),
                    kind: "sans",
                    size: f(&l["tam"], 40.0),
                    color: ink("cor", [0xc8, 0x10, 0x2e]),
                    align: "center",
                    valign: "middle",
                    lh: f(&l["entre"], 0.95),
                    weight: f(&l["peso"], 800.0) as u16,
                    italic: l["ital"].as_bool().unwrap_or(false),
                    spacing: f(&l["espaco"], 1.0),
                    stroke: if l["contorno"].is_null() { 3.0 } else { f(&l["contorno"], 3.0) },
                    stroke_color: ink("corContorno", [0x11; 3]),
                    vertical: l["vertical"].as_bool().unwrap_or(false),
                    upper: true,
                },
                db,
            ));
        }
        "forma" => {
            let fill = color(st(l, "fundo"));
            let bw = f(&l["borda"], 0.0);
            let dashed = l["tracejado"].as_bool().unwrap_or(false);
            let pen = |w: f32, c: [u8; 3]| Pen { width: w, color: c, dash: dashed.then_some((w * 3.0, w * 2.0)) };
            match st(l, "forma") {
                "elipse" => its.push(Item::Path { polys: vec![ellipse(cx, cy, r.w / 2.0, r.h / 2.0)], fill, fill_alpha: 1.0, stroke: (bw > 0.0).then(|| pen(bw, ink("corBorda", [0x11; 3]))) }),
                "linha" => {
                    let w = if bw > 0.0 { bw } else { 0.5 };
                    let c = color(st(l, "corBorda")).or(fill).unwrap_or([0x11; 3]);
                    its.push(Item::Path { polys: vec![vec![(r.x, r.y), (r.x + r.w, r.y + r.h)]], fill: None, fill_alpha: 1.0, stroke: Some(pen(w, c)) });
                }
                "gradiente" => {
                    let c = fill.unwrap_or([0, 0, 0]);
                    let (a0, a1) = (f(&l["grad0"], 0.0), f(&l["grad1"], 0.85));
                    let horizontal = f(&l["gradY"], 1.0) == 0.0 || f(&l["gradX"], 0.0) == 1.0 && f(&l["gradY"], 1.0) == 0.0;
                    its.push(Item::Gradient { x: r.x, y: r.y, w: r.w, h: r.h, color: c, a0, a1, across: horizontal });
                }
                _ => its.push(Item::Path { polys: vec![rect(r.x, r.y, r.w, r.h, f(&l["raio"], 0.0))], fill, fill_alpha: 1.0, stroke: (bw > 0.0).then(|| pen(bw, ink("corBorda", [0x11; 3]))) }),
            }
        }
        _ => {}
    }
    if its.is_empty() {
        return its;
    }
    if m == IDENTITY && opac >= 0.999 {
        return its;
    }
    vec![Item::Group { m, clip: None, alpha: opac, items: its }]
}

// ── models ─────────────────────────────────────────────────────────────

/// Layers of a ready-made page, in % of the page.
pub fn template(name: &str, env: &Env) -> Vec<Json> {
    let (w, h) = (env.page_w, env.page_h);
    let [mi, me, ms, mb] = env.margins;
    let side = (mi + me) / 2.0;
    let g = 3.2;
    let (l, r, t, b) = (side / w * 100.0, 100.0 - side / w * 100.0, ms / h * 100.0, 100.0 - mb / h * 100.0);
    let (gx, gy) = (g / w * 100.0, g / h * 100.0);
    let panel = |x0: f32, y0: f32, x1: f32, y1: f32| json!({ "t": "painel", "id": uid("p"), "x": x0, "y": y0, "w": x1 - x0, "h": y1 - y0, "borda": 0.6 });
    let grid = |rows: usize, cols: usize| -> Vec<Json> {
        let ch = (b - t - gy * (rows - 1) as f32) / rows as f32;
        let cw = (r - l - gx * (cols - 1) as f32) / cols as f32;
        let mut out = Vec::new();
        for ri in 0..rows {
            for ci in 0..cols {
                let (x0, y0) = (l + ci as f32 * (cw + gx), t + ri as f32 * (ch + gy));
                out.push(panel(x0, y0, x0 + cw, y0 + ch));
            }
        }
        out
    };
    let mid = |a: f32, b: f32, k: f32| a + (b - a) * k;
    let bled = || json!({ "t": "imagem", "id": uid("i"), "x": 0, "y": 0, "w": 100, "h": 100, "sangria": true, "img": "" });
    match name {
        "sangria" => vec![bled()],
        "1" => grid(1, 1),
        "2h" => grid(2, 1),
        "2v" => grid(1, 2),
        "3h" => grid(3, 1),
        "2x2" => grid(2, 2),
        "3x2" => grid(3, 2),
        "3x3" => grid(3, 3),
        "tira" => grid(4, 1),
        "L" => {
            let ya = mid(t, b, 0.58);
            let cw = (r - l - gx) / 2.0;
            vec![panel(l, t, r, ya - gy / 2.0), panel(l, ya + gy / 2.0, l + cw, b), panel(l + cw + gx, ya + gy / 2.0, r, b)]
        }
        "T" => {
            let ya = mid(t, b, 0.36);
            let cw = (r - l - gx) / 2.0;
            vec![panel(l, t, l + cw, ya - gy / 2.0), panel(l + cw + gx, t, r, ya - gy / 2.0), panel(l, ya + gy / 2.0, r, b)]
        }
        "manga-diag" => {
            let (y1, y2) = (mid(t, b, 0.3), mid(t, b, 0.66));
            let (a, bb) = (mid(l, r, 0.62), mid(l, r, 0.38));
            let (top, bot) = (y1 + gy / 2.0, y2 - gy / 2.0);
            let cw = (r - l - gx) / 2.0;
            vec![
                panel(l, t, r, y1 - gy / 2.0),
                json!({ "t": "painel", "id": uid("p"), "x": l, "y": top, "w": a - l, "h": bot - top, "borda": 0.6, "pts": [[l, top], [a - gx / 2.0, top], [bb - gx / 2.0, bot], [l, bot]] }),
                json!({ "t": "painel", "id": uid("p"), "x": bb, "y": top, "w": r - bb, "h": bot - top, "borda": 0.6, "pts": [[a + gx / 2.0, top], [r, top], [r, bot], [bb + gx / 2.0, bot]] }),
                panel(l, y2 + gy / 2.0, l + cw * 0.8, b),
                panel(l + cw * 0.8 + gx, y2 + gy / 2.0, r, b),
            ]
        }
        "manga-impacto" => {
            let mut p = panel(mid(l, r, 0.55), mid(t, b, 0.66), r, b);
            p["borda"] = json!(1.2);
            vec![bled(), p, json!({ "t": "sfx", "id": uid("s"), "x": 8, "y": 10, "w": 60, "h": 20, "texto": "DOOOM", "tam": 54, "rot": -8 })]
        }
        "ilustrado" => {
            let mut p = panel(l, t, r, mid(t, b, 0.5));
            p["borda"] = json!(0);
            let y = mid(t, b, 0.5) + gy * 1.5;
            vec![p, json!({ "t": "texto", "id": uid("t"), "x": l, "y": y, "w": r - l, "h": b - y, "texto": "Escreva aqui o texto que convive com a ilustração…", "tam": 10.5, "entre": 1.45, "alin": "justify", "fonte": "" })]
        }
        "texto-sobre-arte" => vec![
            bled(),
            json!({ "t": "forma", "id": uid("g"), "forma": "gradiente", "x": 0, "y": 45, "w": 100, "h": 55, "fundo": "#000000", "grad0": 0, "grad1": 0.85 }),
            json!({ "t": "texto", "id": uid("t"), "x": l, "y": 62, "w": r - l, "h": b - 62.0, "texto": "O texto flutua sobre a arte, como uma legenda de cinema.", "tam": 12, "entre": 1.4, "alin": "left", "cor": "#f4ead2", "fonte": "" }),
        ],
        "texto-moldura" => vec![
            bled(),
            json!({ "t": "texto", "id": uid("t"), "x": l + 6.0, "y": t + 8.0, "w": r - l - 12.0, "h": b - t - 16.0, "texto": "Texto dentro de uma moldura ilustrada.", "tam": 11, "entre": 1.45, "alin": "justify", "fundo": "#fbf6ea", "fundoOpac": 0.92, "pad": 6, "raio": 2, "fonte": "" }),
        ],
        _ => Vec::new(),
    }
}

pub fn new_page(tipo: &str, tpl: &str, env: &Env) -> Json {
    json!({ "id": uid("pg"), "v": 1, "tipo": if tipo.is_empty() { "pagina" } else { tipo }, "fundo": "#ffffff", "camadas": template(tpl, env) })
}

fn shade(hex: &str, amt: f32) -> String {
    let Some(c) = color(hex) else { return hex.to_string() };
    let g = |v: u8| -> u8 {
        let v = v as f32;
        (if amt < 0.0 { v * (1.0 + amt) } else { v + (255.0 - v) * amt }).round().clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", g(c[0]), g(c[1]), g(c[2]))
}

/// The cover a book starts with: art on the front under a veil, title and
/// author, the spine, the back text and a place for the barcode.
pub fn default_cover(title: &str, author: &str, book_color: &str, back: &str) -> Json {
    let t = if title.is_empty() { "Título" } else { title };
    let surname = author.split_whitespace().last().unwrap_or("");
    json!({
        "id": "capa", "v": 1, "tipo": "capa",
        "fundo": if book_color.is_empty() { "#14262a".to_string() } else { shade(book_color, -0.55) },
        "papel": "", "orelha": 0, "capaDura": false, "lombadaAuto": true,
        "camadas": [
            { "t": "imagem", "id": "arte-capa", "zona": "capa", "x": 0, "y": 0, "w": 100, "h": 100, "sangria": true, "img": "" },
            { "t": "forma", "id": "veu", "zona": "capa", "forma": "gradiente", "x": 0, "y": 0, "w": 100, "h": 45, "fundo": "#000000", "grad0": 0.75, "grad1": 0, "gradY": 1 },
            { "t": "texto", "id": "titulo", "zona": "capa", "x": 8, "y": 9, "w": 84, "h": 22, "texto": t.to_uppercase(), "fonte": "Cinzel", "tam": 30, "cor": "#f1e2b8", "alin": "center", "valign": "middle", "peso": 700, "espaco": 2, "entre": 1.05 },
            { "t": "texto", "id": "autor", "zona": "capa", "x": 10, "y": 86, "w": 80, "h": 7, "texto": (if author.is_empty() { "Autor" } else { author }).to_uppercase(), "fonte": "Cormorant", "tam": 13, "cor": "#f1e2b8", "alin": "center", "valign": "middle", "espaco": 3 },
            { "t": "texto", "id": "lombada-titulo", "zona": "lombada", "x": 0, "y": 18, "w": 100, "h": 64, "texto": t, "fonte": "Cinzel", "tam": 11, "cor": "#f1e2b8", "alin": "center", "vertical": true, "peso": 700 },
            { "t": "texto", "id": "lombada-autor", "zona": "lombada", "x": 0, "y": 84, "w": 100, "h": 12, "texto": surname, "fonte": "Cormorant", "tam": 8, "cor": "#f1e2b8", "alin": "center", "vertical": true },
            { "t": "texto", "id": "sinopse", "zona": "contracapa", "x": 14, "y": 16, "w": 72, "h": 52, "texto": if back.is_empty() { "Texto da contracapa…" } else { back }, "fonte": "Iowan Old Style", "tam": 10.5, "cor": "#efe6d2", "alin": "justify", "entre": 1.5 },
            { "t": "forma", "id": "codigo", "zona": "contracapa", "forma": "ret", "x": 58, "y": 80, "w": 30, "h": 12, "fundo": "#ffffff", "borda": 0 },
            { "t": "texto", "id": "codigo-txt", "zona": "contracapa", "x": 58, "y": 80, "w": 30, "h": 12, "texto": "ISBN / código de barras", "fonte": "Helvetica", "tam": 6, "cor": "#555", "alin": "center", "valign": "middle" }
        ]
    })
}

/// The pictures a model uses (with their filters).
pub fn images(model: &Json) -> Vec<(String, String)> {
    let empty = vec![];
    model["camadas"].as_array().unwrap_or(&empty).iter().filter(|l| !st(l, "img").is_empty()).map(|l| (st(l, "img").to_string(), st(l, "filtro").to_string())).collect()
}

/// A picture's pixels through one of the layer filters (straight RGBA).
pub fn filter(rgba: &mut [u8], w: u32, h: u32, name: &str) {
    let mat = |m: [f32; 12], px: &mut [u8]| {
        for c in px.chunks_mut(4) {
            let (r, g, b) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
            for (k, v) in c.iter_mut().take(3).enumerate() {
                *v = ((m[k * 4] * r + m[k * 4 + 1] * g + m[k * 4 + 2] * b + m[k * 4 + 3]) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    };
    match name {
        "pb" => mat([0.2126, 0.7152, 0.0722, 0.0, 0.2126, 0.7152, 0.0722, 0.0, 0.2126, 0.7152, 0.0722, 0.0], rgba),
        "sepia" => mat([0.393, 0.769, 0.189, 0.0, 0.349, 0.686, 0.168, 0.0, 0.272, 0.534, 0.131, 0.0], rgba),
        "noite" => mat([0.3, 0.3, 0.3, 0.0, 0.3, 0.4, 0.4, 0.02, 0.4, 0.5, 0.7, 0.08], rgba),
        "contraste" => {
            for c in rgba.chunks_mut(4) {
                for v in c.iter_mut().take(3) {
                    *v = ((*v as f32 / 255.0 * 1.4 - 0.2) * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        "desfoque" => {
            // a small box blur, twice (close to a Gaussian)
            let r = ((w.max(h) as f32) / 400.0).round().max(1.0) as i64;
            for _ in 0..2 {
                let src = rgba.to_vec();
                let (w, h) = (w as i64, h as i64);
                for y in 0..h {
                    for x in 0..w {
                        let mut s = [0u32; 4];
                        let mut n = 0;
                        for dy in -r..=r {
                            for dx in -r..=r {
                                let (xx, yy) = ((x + dx).clamp(0, w - 1), (y + dy).clamp(0, h - 1));
                                let i = ((yy * w + xx) * 4) as usize;
                                for k in 0..4 {
                                    s[k] += src[i + k] as u32;
                                }
                                n += 1;
                            }
                        }
                        let i = ((y * w + x) * 4) as usize;
                        for k in 0..4 {
                            rgba[i + k] = (s[k] / n) as u8;
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_zones_add_up() {
        let cfg = json!({ "largura": 140, "altura": 210, "papel": "polen80", "sangria": 3 });
        let mut c = default_cover("Vórtices", "Ana Souza", "", "");
        c["orelha"] = json!(80);
        let e = cover_env(&c, &cfg, 200);
        let back = e.zone("contracapa");
        let spine = e.zone("lombada");
        let front = e.zone("capa");
        assert_eq!(back.x, 80.0);
        assert!((spine.x - 220.0).abs() < 1e-3);
        assert!((front.x - (220.0 + e.spine)).abs() < 1e-3);
        assert!((e.w - (2.0 * 80.0 + 2.0 * 140.0 + e.spine)).abs() < 1e-3);
        // a bled layer on the front runs past the right edge into the bleed
        let l = json!({ "zona": "capa", "sangria": true });
        let r = layer_rect(&l, &e);
        assert!((r.x - front.x).abs() < 1e-3 && (r.w - (front.w)).abs() < 1e-3);
    }

    #[test]
    fn templates_fill_the_page() {
        let cfg = json!({ "largura": 128, "altura": 182 });
        let env = page_env(&json!({}), &cfg, true);
        assert_eq!(template("3x3", &env).len(), 9);
        assert_eq!(template("manga-diag", &env).len(), 5);
        let m = new_page("pagina", "2x2", &env);
        assert_eq!(m["camadas"].as_array().unwrap().len(), 4);
    }
}
