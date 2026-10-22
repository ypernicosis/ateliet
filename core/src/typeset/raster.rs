//! A page as a picture (the preview), drawn with the same glyph outlines
//! and positions as the PDF.

use super::fonts::{FontDb, FontId};
use super::{Item, Page, Picture, PT_MM};
use crate::scriptorium::mapa::{dashes, Edge, Raster};
use std::collections::HashMap;
use std::sync::Mutex;

type Outline = Vec<((f32, f32), (f32, f32))>;

struct Builder {
    edges: Outline,
    start: (f32, f32),
    cur: (f32, f32),
}

impl ttf_parser::OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = (x, y);
        self.cur = (x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.edges.push((self.cur, (x, y)));
        self.cur = (x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.cur;
        let mut prev = p0;
        for i in 1..=6 {
            let t = i as f32 / 6.0;
            let u = 1.0 - t;
            let q = (u * u * p0.0 + 2.0 * u * t * x1 + t * t * x, u * u * p0.1 + 2.0 * u * t * y1 + t * t * y);
            self.edges.push((prev, q));
            prev = q;
        }
        self.cur = (x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.cur;
        let mut prev = p0;
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            let f = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
            let q = (f(p0.0, x1, x2, x), f(p0.1, y1, y2, y));
            self.edges.push((prev, q));
            prev = q;
        }
        self.cur = (x, y);
    }
    fn close(&mut self) {
        if self.cur != self.start {
            self.edges.push((self.cur, self.start));
        }
        self.cur = self.start;
    }
}

type Outlines = Mutex<HashMap<(FontId, u16), std::sync::Arc<Outline>>>;

static OUTLINES: once_cell::sync::Lazy<Outlines> = once_cell::sync::Lazy::new(Default::default);

fn outline(db: &FontDb, f: FontId, g: u16) -> std::sync::Arc<Outline> {
    if let Some(o) = OUTLINES.lock().unwrap().get(&(f, g)) {
        return o.clone();
    }
    let face = db.face(f);
    let mut b = Builder { edges: Vec::new(), start: (0.0, 0.0), cur: (0.0, 0.0) };
    if let Some(d) = face.data() {
        if let Ok(tf) = ttf_parser::Face::parse(&d, face.index) {
            tf.outline_glyph(ttf_parser::GlyphId(g), &mut b);
        }
    }
    let o = std::sync::Arc::new(b.edges);
    OUTLINES.lock().unwrap().insert((f, g), o.clone());
    o
}

fn rgb(c: [u8; 3]) -> [f64; 3] {
    [c[0] as f64, c[1] as f64, c[2] as f64]
}

type M = [f64; 6];

fn ap(m: &M, x: f32, y: f32) -> (f64, f64) {
    let (x, y) = (x as f64, y as f64);
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn mul(m: &M, n: &M) -> M {
    [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3], m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5]]
}

/// The page at `ppm` pixels per mm, as straight RGBA.
pub fn render(page: &Page, db: &FontDb, pics: &[Picture], ppm: f32) -> (u32, u32, Vec<u8>) {
    let w = (page.w * ppm).round().max(1.0) as usize;
    let h = (page.h * ppm).round().max(1.0) as usize;
    let paper: Vec<u8> = (0..w * h).flat_map(|_| [page.paper[0], page.paper[1], page.paper[2], 255]).collect();
    let mut r = Raster::from_rgba(w, h, paper, 2);
    let s = ppm as f64;
    draw(&mut r, &page.items, &[s, 0.0, 0.0, s, 0.0, 0.0], db, pics, 1.0);
    (w as u32, h as u32, r.into_rgba())
}

/// A text item's glyph outlines, each edge in raster space.
fn glyph_edges(it: &Item, m: &M, db: &FontDb) -> Vec<Edge> {
    let Item::Text { font, size, x, y, glyphs, .. } = it else { return Vec::new() };
    let face = db.face(*font);
    let k = *size * PT_MM / face.upem;
    let mut edges: Vec<Edge> = Vec::new();
    for g in glyphs {
        let o = outline(db, *font, g.id);
        let (ox, oy) = (x + g.x, y + g.y);
        edges.extend(o.iter().map(|(a, b)| (ap(m, ox + a.0 * k, oy - a.1 * k), ap(m, ox + b.0 * k, oy - b.1 * k))));
    }
    edges
}

fn draw(r: &mut Raster, items: &[Item], m: &M, db: &FontDb, pics: &[Picture], alpha: f64) {
    let sc = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
    let poly = |ps: &[Vec<(f32, f32)>]| -> Vec<Vec<(f64, f64)>> { ps.iter().map(|p| p.iter().map(|q| ap(m, q.0, q.1)).collect()).collect() };
    for it in items {
        match it {
            Item::Rect { x, y, w: rw, h: rh, color } => {
                r.fill(&poly(&[vec![(*x, *y), (x + rw, *y), (x + rw, y + rh), (*x, y + rh)]]), rgb(*color), alpha);
            }
            Item::Line { x0, y0, x1, y1, width, color, dash } => {
                let line = vec![ap(m, *x0, *y0), ap(m, *x1, *y1)];
                let lines = match dash {
                    Some((a, b)) => dashes(&line, *a as f64 * sc, *b as f64 * sc),
                    None => vec![line],
                };
                r.stroke(&lines, (*width as f64 * sc).max(0.6), rgb(*color), alpha);
            }
            Item::Image { pic, x, y, w: iw, h: ih } => {
                let Some(p) = pics.get(*pic) else { continue };
                let mut pm = p.rgba.clone();
                for c in pm.chunks_mut(4) {
                    let a = c[3] as u32;
                    for v in c.iter_mut().take(3) {
                        *v = ((*v as u32 * a + 127) / 255) as u8;
                    }
                }
                let place = [*iw as f64 / p.w as f64, 0.0, 0.0, *ih as f64 / p.h as f64, *x as f64, *y as f64];
                r.image(&pm, p.w as usize, p.h as usize, mul(m, &place));
            }
            Item::Text { color, .. } => {
                r.fill_edges(&glyph_edges(it, m, db), rgb(*color), alpha);
            }
            Item::Outline { text, width, color } => {
                let segs: Vec<Vec<(f64, f64)>> = glyph_edges(text, m, db).into_iter().map(|(a, b)| vec![a, b]).collect();
                r.stroke(&segs, *width as f64 * sc, rgb(*color), alpha);
            }
            Item::Path { polys, fill, fill_alpha, stroke } => {
                let ps = poly(polys);
                if let Some(f) = fill {
                    r.fill(&ps, rgb(*f), alpha * *fill_alpha as f64);
                }
                if let Some(s) = stroke {
                    // closed outlines
                    let closed: Vec<Vec<(f64, f64)>> = ps.iter().filter(|p| p.len() > 1).map(|p| {
                        let mut q = p.clone();
                        if p.len() > 2 {
                            q.push(p[0]);
                        }
                        q
                    }).collect();
                    let lines = match s.dash {
                        Some((a, b)) => closed.iter().flat_map(|p| dashes(p, a as f64 * sc, b as f64 * sc)).collect(),
                        None => closed,
                    };
                    r.stroke(&lines, (s.width as f64 * sc).max(0.5), rgb(s.color), alpha);
                }
            }
            Item::Gradient { x, y, w: gw, h: gh, color, a0, a1, across } => {
                // coverage of the box, then each pixel's opacity along it
                let mut cov = Raster::new(r.w, r.h, r.ss());
                cov.fill(&poly(&[vec![(*x, *y), (x + gw, *y), (x + gw, y + gh), (*x, y + gh)]]), [255.0; 3], 1.0);
                let det = m[0] * m[3] - m[1] * m[2];
                if det.abs() < 1e-12 {
                    continue;
                }
                let inv = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det, (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det];
                let mut layer = Raster::new(r.w, r.h, r.ss());
                let c = rgb(*color);
                for py in 0..r.h {
                    for px in 0..r.w {
                        let i = (py * r.w + px) * 4;
                        let k = cov.px[i + 3];
                        if k == 0 {
                            continue;
                        }
                        let (u, v) = (inv[0] * (px as f64 + 0.5) + inv[2] * (py as f64 + 0.5) + inv[4], inv[1] * (px as f64 + 0.5) + inv[3] * (py as f64 + 0.5) + inv[5]);
                        let t = if *across { (u - *x as f64) / *gw as f64 } else { (v - *y as f64) / *gh as f64 }.clamp(0.0, 1.0);
                        let a = (*a0 as f64 + (*a1 - *a0) as f64 * t) * k as f64 / 255.0 * alpha;
                        for ch in 0..3 {
                            layer.px[i + ch] = (c[ch] * a).round() as u8;
                        }
                        layer.px[i + 3] = (255.0 * a).round() as u8;
                    }
                }
                r.over(&layer, None, 1.0);
            }
            Item::Group { m: gm, clip, alpha: ga, items } => {
                let gm = [gm[0] as f64, gm[1] as f64, gm[2] as f64, gm[3] as f64, gm[4] as f64, gm[5] as f64];
                let mm = mul(m, &gm);
                if clip.is_none() && *ga >= 0.999 {
                    draw(r, items, &mm, db, pics, alpha);
                    continue;
                }
                let mut layer = Raster::new(r.w, r.h, r.ss());
                draw(&mut layer, items, &mm, db, pics, 1.0);
                let mask = clip.as_ref().map(|cl| {
                    let mut mr = Raster::new(r.w, r.h, r.ss());
                    let ps: Vec<Vec<(f64, f64)>> = cl.iter().map(|p| p.iter().map(|q| ap(&mm, q.0, q.1)).collect()).collect();
                    mr.fill(&ps, [255.0; 3], 1.0);
                    mr.px.chunks(4).map(|c| c[3]).collect::<Vec<u8>>()
                });
                r.over(&layer, mask.as_deref(), alpha * *ga as f64);
            }
        }
    }
}
