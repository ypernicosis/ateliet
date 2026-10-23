//! The hand-drawn look: each element as rough strokes and fills, after
//! rough.js as Excalidraw sets it up (two slightly bowed passes per line,
//! overlapping ellipses, hachure at -41°, arrowheads by kind). Driven by
//! the element's seed, so a drawing looks the same every time and in the
//! exported picture.

use super::element::{rotate, Element};

pub type P = (f32, f32);

#[derive(Clone, Debug)]
pub struct Path {
    pub pts: Vec<P>,
    pub width: f32,
    pub color: [u8; 4],
    /// Freehand strokes: round joins.
    pub round: bool,
}

#[derive(Clone, Debug)]
pub struct Fill {
    pub pts: Vec<P>,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub fills: Vec<Fill>,
    pub strokes: Vec<Path>,
}

/// A CSS colour (`#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`, a few names);
/// None for transparent.
pub fn color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().to_lowercase();
    if s.is_empty() || s == "transparent" || s == "none" {
        return None;
    }
    if let Some(h) = s.strip_prefix('#') {
        let v = |i: usize, n: usize| u8::from_str_radix(&h[i..i + n], 16).ok();
        return match h.len() {
            3 => Some([v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17, 255]),
            4 => Some([v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17, v(3, 1)? * 17]),
            6 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, 255]),
            8 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, v(6, 2)?]),
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
        let n: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if n.len() >= 3 {
            let a = n.get(3).map(|a| (a * 255.0) as u8).unwrap_or(255);
            return Some([n[0] as u8, n[1] as u8, n[2] as u8, a]);
        }
    }
    Some(match s.as_str() {
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "orange" => [255, 165, 0, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        _ => [30, 30, 30, 255],
    })
}

/// rough.js's seeded random (Park–Miller).
struct Rng(i32);

impl Rng {
    fn next(&mut self) -> f32 {
        if self.0 == 0 {
            self.0 = 1;
        }
        self.0 = self.0.wrapping_mul(48271);
        ((self.0 & 0x7fff_ffff) as f64 / 2_147_483_648.0) as f32
    }
}

struct Opts {
    roughness: f32,
    bowing: f32,
    max_offset: f32,
    preserve: bool,
    multi: bool,
}

struct Gen {
    rng: Rng,
    o: Opts,
}

fn bezier(a: P, b: P, c: P, d: P, n: usize, out: &mut Vec<P>) {
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        out.push((
            u * u * u * a.0 + 3.0 * u * u * t * b.0 + 3.0 * u * t * t * c.0 + t * t * t * d.0,
            u * u * u * a.1 + 3.0 * u * u * t * b.1 + 3.0 * u * t * t * c.1 + t * t * t * d.1,
        ));
    }
}

impl Gen {
    fn off(&mut self, x: f32, gain: f32) -> f32 {
        self.o.roughness * gain * (self.rng.next() * 2.0 * x - x)
    }

    /// One pass of a rough line (rough.js `_line`), as points.
    fn line(&mut self, a: P, b: P, overlay: bool, out: &mut Vec<P>) {
        let len = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        let gain = if len < 200.0 {
            1.0
        } else if len > 500.0 {
            0.4
        } else {
            -0.0016668 * len + 1.233334
        };
        let mut offset = self.o.max_offset;
        if offset * offset * 100.0 > len * len {
            offset = len / 10.0;
        }
        let half = offset / 2.0;
        let diverge = 0.2 + self.rng.next() * 0.2;
        let mid_x = self.o.bowing * self.o.max_offset * (b.1 - a.1) / 200.0;
        let mid_y = self.o.bowing * self.o.max_offset * (a.0 - b.0) / 200.0;
        let mid_x = self.off(mid_x, gain);
        let mid_y = self.off(mid_y, gain);
        let o = if overlay { half } else { offset };
        let pv = self.o.preserve;
        let start = if pv { a } else { (a.0 + self.off(o, gain), a.1 + self.off(o, gain)) };
        let c1 = (mid_x + a.0 + (b.0 - a.0) * diverge + self.off(o, gain), mid_y + a.1 + (b.1 - a.1) * diverge + self.off(o, gain));
        let c2 = (mid_x + a.0 + 2.0 * (b.0 - a.0) * diverge + self.off(o, gain), mid_y + a.1 + 2.0 * (b.1 - a.1) * diverge + self.off(o, gain));
        let end = if pv { b } else { (b.0 + self.off(o, gain), b.1 + self.off(o, gain)) };
        if out.is_empty() || out.last().is_some_and(|l| (l.0 - start.0).abs() + (l.1 - start.1).abs() > 0.01) {
            out.push(start);
        }
        let n = ((len / 18.0) as usize).clamp(3, 14);
        if self.o.roughness == 0.0 {
            out.push(end);
        } else {
            bezier(start, c1, c2, end, n, out);
        }
    }

    /// A closed or open run of straight segments, in one or two passes.
    fn poly(&mut self, pts: &[P], closed: bool) -> Vec<Vec<P>> {
        let mut passes = Vec::new();
        let n = if self.o.multi { 2 } else { 1 };
        for pass in 0..n {
            let mut out = Vec::new();
            let count = if closed { pts.len() } else { pts.len().saturating_sub(1) };
            for i in 0..count {
                self.line(pts[i], pts[(i + 1) % pts.len()], pass == 1, &mut out);
            }
            passes.push(out);
        }
        passes
    }

    /// A smooth curve through points (Catmull–Rom), sampled.
    fn curve(pts: &[P]) -> Vec<P> {
        if pts.len() < 3 {
            return pts.to_vec();
        }
        let mut out = vec![pts[1]];
        for i in 1..pts.len() - 2 {
            let (p0, p1, p2, p3) = (pts[i - 1], pts[i], pts[i + 1], pts[i + 2]);
            let b1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
            let b2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
            let len = ((p2.0 - p1.0).powi(2) + (p2.1 - p1.1).powi(2)).sqrt();
            bezier(p1, b1, b2, p2, ((len / 10.0) as usize).clamp(3, 24), &mut out);
        }
        out
    }

    /// A curve through points, roughened (rough.js `_curveWithOffset`).
    fn rough_curve(&mut self, pts: &[P]) -> Vec<Vec<P>> {
        let mut passes = Vec::new();
        let n = if self.o.multi { 2 } else { 1 };
        for pass in 0..n {
            let off = if pass == 0 { 1.0 + self.o.roughness * 0.2 } else { 1.5 * (1.0 + self.o.roughness * 0.22) };
            let mut ps = Vec::with_capacity(pts.len() + 2);
            let j = |g: &mut Gen| g.off(off, 1.0);
            ps.push((pts[0].0 + j(self), pts[0].1 + j(self)));
            for p in pts {
                ps.push((p.0 + j(self), p.1 + j(self)));
            }
            let last = *pts.last().unwrap();
            ps.push((last.0 + j(self), last.1 + j(self)));
            passes.push(Self::curve(&ps));
        }
        passes
    }

    /// An ellipse: two overlapping passes (rough.js `ellipseWithParams`).
    fn ellipse(&mut self, cx: f32, cy: f32, w: f32, h: f32) -> (Vec<Vec<P>>, Vec<P>) {
        let (mut rx, mut ry) = ((w / 2.0).abs(), (h / 2.0).abs());
        let psq = (std::f32::consts::TAU * ((rx * rx + ry * ry) / 2.0).sqrt()).sqrt();
        let steps = (9.0f32).max(9.0 / 200f32.sqrt() * psq).ceil();
        let inc = std::f32::consts::TAU / steps;
        let outline: Vec<P> = (0..64).map(|i| {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            (cx + rx * a.cos(), cy + ry * a.sin())
        }).collect();
        if self.o.roughness == 0.0 {
            let mut o = outline.clone();
            o.push(outline[0]);
            return (vec![o], outline);
        }
        rx += self.off(rx * 0.05, 1.0);
        ry += self.off(ry * 0.05, 1.0);
        let mut passes = Vec::new();
        let n = if self.o.multi { 2 } else { 1 };
        for pass in 0..n {
            let offset = if pass == 0 { 1.0 } else { 1.5 };
            let overlap = if pass == 0 { inc * (0.1 + self.off(0.4, 1.0).abs() * 0.25) } else { 0.0 };
            let rad = self.off(0.5, 1.0) - std::f32::consts::FRAC_PI_2;
            let mut pts = Vec::new();
            pts.push((self.off(offset, 1.0) + cx + 0.9 * rx * (rad - inc).cos(), self.off(offset, 1.0) + cy + 0.9 * ry * (rad - inc).sin()));
            let end = std::f32::consts::TAU + rad - 0.01;
            let mut a = rad;
            while a < end {
                pts.push((self.off(offset, 1.0) + cx + rx * a.cos(), self.off(offset, 1.0) + cy + ry * a.sin()));
                a += inc;
            }
            let t = std::f32::consts::TAU + rad + overlap * 0.5;
            pts.push((self.off(offset, 1.0) + cx + rx * t.cos(), self.off(offset, 1.0) + cy + ry * t.sin()));
            pts.push((self.off(offset, 1.0) + cx + 0.98 * rx * (rad + overlap).cos(), self.off(offset, 1.0) + cy + 0.98 * ry * (rad + overlap).sin()));
            pts.push((self.off(offset, 1.0) + cx + 0.9 * rx * (rad + overlap * 0.5).cos(), self.off(offset, 1.0) + cy + 0.9 * ry * (rad + overlap * 0.5).sin()));
            passes.push(Self::curve(&pts));
        }
        (passes, outline)
    }

    /// A polygon with rounded corners (cut `r` along each side), roughened.
    fn rounded(&mut self, pts: &[P], r: f32) -> (Vec<Vec<P>>, Vec<P>) {
        let n = pts.len();
        let mut cuts = Vec::with_capacity(n);
        for i in 0..n {
            let (prev, v, next) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let l1 = ((v.0 - prev.0).powi(2) + (v.1 - prev.1).powi(2)).sqrt().max(0.001);
            let l2 = ((next.0 - v.0).powi(2) + (next.1 - v.1).powi(2)).sqrt().max(0.001);
            let r = r.min(l1 / 2.0).min(l2 / 2.0);
            let a = (v.0 + (prev.0 - v.0) * r / l1, v.1 + (prev.1 - v.1) * r / l1);
            let b = (v.0 + (next.0 - v.0) * r / l2, v.1 + (next.1 - v.1) * r / l2);
            cuts.push((a, v, b));
        }
        let corner = |a: P, v: P, b: P, out: &mut Vec<P>| {
            for k in 1..=6 {
                let t = k as f32 / 6.0;
                let u = 1.0 - t;
                out.push((u * u * a.0 + 2.0 * u * t * v.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * v.1 + t * t * b.1));
            }
        };
        let mut outline = Vec::new();
        for i in 0..n {
            let (a, v, b) = cuts[i];
            outline.push(a);
            corner(a, v, b, &mut outline);
        }
        let mut passes = Vec::new();
        let count = if self.o.multi { 2 } else { 1 };
        for pass in 0..count {
            let mut out = Vec::new();
            for i in 0..n {
                let (a, v, b) = cuts[i];
                let next_a = cuts[(i + 1) % n].0;
                if out.is_empty() {
                    out.push(a);
                }
                let j = if pass == 0 { 0.0 } else { 0.6 };
                let mut c = Vec::new();
                corner((a.0 + self.off(j, 1.0), a.1 + self.off(j, 1.0)), v, b, &mut c);
                out.extend(c);
                self.line(b, next_a, pass == 1, &mut out);
            }
            passes.push(out);
        }
        (passes, outline)
    }
}

/// Excalidraw's `getCornerRadius`.
fn corner_radius(x: f32, e: &Element) -> f32 {
    match e.get("roundness").get("type").and_then(|t| t.as_i64()) {
        Some(3) => {
            let fixed = e.get("roundness").get("value").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(32.0);
            if x <= fixed / 0.25 {
                x * 0.25
            } else {
                fixed
            }
        }
        Some(_) => x * 0.25,
        None => 0.0,
    }
}

/// Excalidraw's `adjustRoughness`: small shapes are drawn steadier.
fn roughness_of(e: &Element) -> f32 {
    let r = e.fo("roughness", 1.0);
    let (w, h) = (e.w().abs(), e.h().abs());
    let (max, min) = (w.max(h), w.min(h));
    if (min >= 20.0 && max >= 50.0) || (min >= 15.0 && !e.get("roundness").is_null()) || (e.is_linear() && max >= 50.0) {
        return r;
    }
    (r / if max < 10.0 { 3.0 } else { 2.0 }).min(2.5)
}

pub fn with_alpha(c: [u8; 4], opacity: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (c[3] as f32 * opacity.clamp(0.0, 1.0)) as u8]
}

/// Lines across a polygon every `gap`, at `angle` (radians).
pub fn hatch(poly: &[P], gap: f32, angle: f32) -> Vec<(P, P)> {
    if poly.len() < 3 || gap <= 0.5 {
        return Vec::new();
    }
    let c = poly.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let c = (c.0 / poly.len() as f32, c.1 / poly.len() as f32);
    let rot: Vec<P> = poly.iter().map(|p| rotate(*p, c, -angle)).collect();
    let (y0, y1) = rot.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let mut out = Vec::new();
    let mut y = y0 + gap / 2.0;
    let mut guard = 0;
    while y < y1 && guard < 4000 {
        guard += 1;
        let mut xs = Vec::new();
        for i in 0..rot.len() {
            let (a, b) = (rot[i], rot[(i + 1) % rot.len()]);
            if (a.1 > y) != (b.1 > y) {
                xs.push(a.0 + (y - a.1) * (b.0 - a.0) / (b.1 - a.1));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks(2) {
            if let [a, b] = pair {
                out.push((rotate((*a, y), c, angle), rotate((*b, y), c, angle)));
            }
        }
        y += gap;
    }
    out
}

/// Split a polyline into dashes.
pub fn dashes(pts: &[P], on: f32, off: f32) -> Vec<Vec<P>> {
    let mut out = Vec::new();
    let mut cur: Vec<P> = Vec::new();
    let mut drawing = true;
    let mut left = on;
    for w in pts.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        while len > 0.0 {
            let step = left.min(len);
            let t = step / len;
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if drawing {
                if cur.is_empty() {
                    cur.push(a);
                }
                cur.push(p);
            }
            left -= step;
            len -= step;
            a = p;
            if left <= 0.0 {
                if drawing && cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                drawing = !drawing;
                left = if drawing { on } else { off };
            }
        }
    }
    if drawing && cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// An arrowhead at `tip`, coming from `from` (Excalidraw's sizes and angles).
fn arrowhead(g: &mut Geometry, kind: &str, tip: P, from: P, seg_len: f32, width: f32, color: [u8; 4]) {
    let d = ((tip.0 - from.0).powi(2) + (tip.1 - from.1).powi(2)).sqrt().max(0.001);
    let (nx, ny) = ((tip.0 - from.0) / d, (tip.1 - from.1) / d);
    let size: f32 = match kind {
        "arrow" => 25.0,
        "diamond" | "diamond_outline" => 12.0,
        k if k.starts_with("crowfoot") => 20.0,
        _ => 15.0,
    };
    let min = size.min(seg_len * if kind.starts_with("diamond") { 0.25 } else { 0.5 });
    let back = (tip.0 - nx * min, tip.1 - ny * min);
    let angle = match kind {
        "bar" => 90.0f32,
        "arrow" => 20.0,
        _ => 25.0,
    }
    .to_radians();
    let p3 = rotate(back, tip, -angle);
    let p4 = rotate(back, tip, angle);
    match kind {
        "triangle" => g.fills.push(Fill { pts: vec![tip, p3, p4], color }),
        "triangle_outline" => g.strokes.push(Path { pts: vec![tip, p3, p4, tip], width, color, round: false }),
        "bar" => {
            let a = rotate(back, tip, -angle);
            let b = rotate(back, tip, angle);
            g.strokes.push(Path { pts: vec![(tip.0 + a.0 - back.0, tip.1 + a.1 - back.1), (tip.0 + b.0 - back.0, tip.1 + b.1 - back.1)], width, color, round: false });
        }
        "dot" | "circle" | "circle_outline" => {
            let r = (min + width - 2.0).max(2.0) / 2.0;
            let c = (tip.0 - nx * r, tip.1 - ny * r);
            let pts: Vec<P> = (0..=24).map(|i| {
                let a = i as f32 / 24.0 * std::f32::consts::TAU;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            }).collect();
            if kind == "circle_outline" {
                g.strokes.push(Path { pts, width, color, round: false });
            } else {
                g.fills.push(Fill { pts, color });
            }
        }
        "diamond" | "diamond_outline" => {
            let mid = (tip.0 - nx * min, tip.1 - ny * min);
            let far = (tip.0 - nx * min * 2.0, tip.1 - ny * min * 2.0);
            let side = (-ny * min * 0.5, nx * min * 0.5);
            let pts = vec![tip, (mid.0 + side.0, mid.1 + side.1), far, (mid.0 - side.0, mid.1 - side.1)];
            if kind == "diamond" {
                g.fills.push(Fill { pts, color });
            } else {
                let mut p = pts.clone();
                p.push(tip);
                g.strokes.push(Path { pts: p, width, color, round: false });
            }
        }
        _ => {
            g.strokes.push(Path { pts: vec![p3, tip, p4], width, color, round: false });
        }
    }
}

/// The strokes and fills of an element, in world coordinates.
pub fn geometry(e: &Element) -> Geometry {
    let mut g = Geometry::default();
    if e.deleted() {
        return g;
    }
    let opacity = e.fo("opacity", 100.0) / 100.0;
    let stroke = color(e.s("strokeColor")).map(|c| with_alpha(c, opacity));
    let fill = color(e.s("backgroundColor")).map(|c| with_alpha(c, opacity));
    let sw = e.fo("strokeWidth", 2.0);
    let style = e.s("strokeStyle");
    let solid = style.is_empty() || style == "solid";
    let width = if solid { sw } else { sw + 0.5 };
    let roughness = roughness_of(e);
    let mut gen = Gen { rng: Rng(e.seed() as i32), o: Opts { roughness, bowing: 1.0, max_offset: 2.0, preserve: e.fo("roughness", 1.0) < 2.0, multi: solid } };
    let b = e.bounds();
    let (x0, y0, x1, y1) = b;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut outline: Vec<P> = Vec::new();
    let passes: Vec<Vec<P>>;
    let mut closed_fill = true;
    match e.kind() {
        "rectangle" => {
            let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
            let r = corner_radius((x1 - x0).min(y1 - y0), e);
            if r > 0.5 {
                let (p, o) = gen.rounded(&pts, r);
                passes = p;
                outline = o;
            } else {
                passes = gen.poly(&pts, true);
                outline = pts.to_vec();
            }
        }
        "diamond" => {
            let pts = [(cx, y0), (x1, cy), (cx, y1), (x0, cy)];
            let r = corner_radius((x1 - x0).abs().min((y1 - y0).abs()) * 0.5, e);
            if r > 0.5 {
                let (p, o) = gen.rounded(&pts, r);
                passes = p;
                outline = o;
            } else {
                passes = gen.poly(&pts, true);
                outline = pts.to_vec();
            }
        }
        "ellipse" => {
            let (p, o) = gen.ellipse(cx, cy, x1 - x0, y1 - y0);
            passes = p;
            outline = o;
        }
        "frame" | "magicframe" => {
            passes = vec![vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]];
            closed_fill = false;
        }
        "embeddable" | "iframe" => {
            passes = vec![vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]];
            outline = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        }
        "line" | "arrow" => {
            let pts = e.abs_points();
            if pts.len() < 2 {
                return g;
            }
            let curved = !e.get("roundness").is_null() && pts.len() > 2 && !e.get("elbowed").as_bool().unwrap_or(false);
            let smooth = if curved {
                let mut ext = vec![pts[0]];
                ext.extend(pts.iter().copied());
                ext.push(*pts.last().unwrap());
                Gen::curve(&ext)
            } else {
                pts.clone()
            };
            passes = if roughness == 0.0 {
                vec![smooth.clone()]
            } else if curved {
                gen.rough_curve(&pts)
            } else {
                gen.poly(&pts, false)
            };
            closed_fill = pts.len() > 2 && (pts[0].0 - pts[pts.len() - 1].0).abs() + (pts[0].1 - pts[pts.len() - 1].1).abs() < 1.0;
            if closed_fill {
                outline = smooth.clone();
            }
            if e.kind() == "arrow" {
                if let Some(c) = stroke {
                    let n = smooth.len();
                    let seg = |a: P, b: P| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                    // the direction at each end: a point a little way back along the path
                    let back_from = |path: &[P]| -> (P, f32) {
                        let tip = path[0];
                        let mut acc = 0.0;
                        for p in &path[1..] {
                            acc = seg(tip, *p);
                            if acc > 4.0 {
                                return (*p, if curved { acc.max(seg(pts[0], pts[1])) } else { seg(pts[0], pts[1]) });
                            }
                        }
                        (*path.last().unwrap(), acc)
                    };
                    if let Some(h) = e.get("endArrowhead").as_str() {
                        let rev: Vec<P> = smooth.iter().rev().copied().collect();
                        let (from, _) = back_from(&rev);
                        let last_seg = seg(pts[pts.len() - 1], pts[pts.len() - 2]);
                        arrowhead(&mut g, h, smooth[n - 1], from, if curved { last_seg.max(seg(smooth[n - 1], from)) } else { last_seg }, width, c);
                    }
                    if let Some(h) = e.get("startArrowhead").as_str() {
                        let (from, l) = back_from(&smooth);
                        arrowhead(&mut g, h, smooth[0], from, l, width, c);
                    }
                }
            }
        }
        "freedraw" => {
            let pts = e.abs_points();
            let press: Vec<f32> = e.get("pressures").as_array().map(|a| a.iter().filter_map(|p| p.as_f64().map(|x| x as f32)).collect()).unwrap_or_default();
            let simulated = e.get("simulatePressure").as_bool().unwrap_or(true);
            let avg = if simulated || press.is_empty() { 0.5 } else { press.iter().sum::<f32>() / press.len() as f32 };
            if let Some(c) = stroke {
                let w = (sw * 4.25 * (1.0 - 0.6 * (1.0 - avg))).max(1.0);
                g.strokes.push(Path { pts: if pts.len() == 1 { vec![pts[0], (pts[0].0 + 0.1, pts[0].1)] } else { pts.clone() }, width: w, color: c, round: true });
            }
            if let Some(f) = fill {
                if pts.len() > 2 {
                    g.fills.insert(0, Fill { pts, color: f });
                }
            }
            return finish(g, e);
        }
        _ => return g,
    }
    if let (Some(f), true) = (fill, closed_fill && outline.len() > 2) {
        match e.s("fillStyle") {
            "solid" => g.fills.push(Fill { pts: outline.clone(), color: f }),
            kind => {
                let gap = (sw * 4.0).max(4.0);
                let angle = (-41.0f32).to_radians();
                let fw = (sw / 2.0).max(0.6);
                // hachure lines are single passes
                let mut lines = hatch(&outline, gap, angle);
                if kind == "cross-hatch" {
                    lines.extend(hatch(&outline, gap, angle + std::f32::consts::FRAC_PI_2));
                }
                gen.o.multi = false;
                for (a, b) in lines {
                    let mut out = Vec::new();
                    gen.line(a, b, false, &mut out);
                    g.strokes.push(Path { pts: out, width: fw, color: f, round: false });
                }
            }
        }
    }
    if let Some(c) = stroke {
        for p in passes {
            if p.len() < 2 {
                continue;
            }
            if solid {
                g.strokes.push(Path { pts: p, width, color: c, round: false });
            } else {
                let (on, off) = if style == "dashed" { (8.0, 8.0 + sw) } else { (1.5, 6.0 + sw) };
                for d in dashes(&p, on, off) {
                    g.strokes.push(Path { pts: d, width, color: c, round: style == "dotted" });
                }
            }
        }
    }
    finish(g, e)
}

fn finish(mut g: Geometry, e: &Element) -> Geometry {
    let a = e.angle();
    if a != 0.0 {
        let c = e.center();
        for f in g.fills.iter_mut() {
            for p in f.pts.iter_mut() {
                *p = rotate(*p, c, a);
            }
        }
        for s in g.strokes.iter_mut() {
            for p in s.pts.iter_mut() {
                *p = rotate(*p, c, a);
            }
        }
    }
    g
}

/// Triangles of a simple polygon (ear clipping), as index triples.
pub fn triangulate(pts: &[P]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f32 = (0..n).map(|i| {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        a.0 * b.1 - b.0 * a.1
    }).sum();
    let mut idx: Vec<usize> = (0..n).collect();
    if area < 0.0 {
        idx.reverse();
    }
    let cross = |a: P, b: P, c: P| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut cut = None;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross(a, b, c) <= 0.0 {
                continue;
            }
            let ear = idx.iter().all(|&k| {
                if k == ia || k == ib || k == ic {
                    return true;
                }
                let p = pts[k];
                !(cross(a, b, p) > 0.0 && cross(b, c, p) > 0.0 && cross(c, a, p) > 0.0)
            });
            if ear {
                cut = Some(i);
                out.push([ia, ib, ic]);
                break;
            }
        }
        match cut {
            Some(i) => {
                idx.remove(i);
            }
            None => {
                // not simple: fan the rest
                for i in 1..idx.len() - 1 {
                    out.push([idx[0], idx[i], idx[i + 1]]);
                }
                return out;
            }
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::element::Style;
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(color("#1e1e1e"), Some([30, 30, 30, 255]));
        assert_eq!(color("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(color("transparent"), None);
        assert_eq!(color("rgba(10, 20, 30, 0.5)"), Some([10, 20, 30, 127]));
    }

    #[test]
    fn shapes_are_rough_and_stable() {
        let mut st = Style::default();
        st.background = "#a5d8ff".into();
        let r = Element::new("rectangle", 0.0, 0.0, 200.0, 100.0, &st);
        let g1 = geometry(&r);
        let g2 = geometry(&r);
        assert!(g1.strokes.len() > 2, "outline passes and hachure lines");
        assert_eq!(format!("{:?}", g1.strokes[0].pts), format!("{:?}", g2.strokes[0].pts), "same seed, same drawing");
        // the points stay near the box
        for s in &g1.strokes {
            for p in &s.pts {
                assert!(p.0 > -8.0 && p.0 < 208.0 && p.1 > -8.0 && p.1 < 108.0);
            }
        }
        let mut a = Element::new("arrow", 0.0, 0.0, 100.0, 0.0, &st);
        a.set_abs_points(&[(0.0, 0.0), (100.0, 0.0)]);
        let g = geometry(&a);
        // two passes and the two sides of the head
        assert!(g.strokes.iter().any(|s| s.pts.len() == 3 && (s.pts[1].0 - 100.0).abs() < 0.01));
    }

    #[test]
    fn hatching_and_triangles() {
        let sq = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        let lines = hatch(&sq, 10.0, 0.0);
        assert_eq!(lines.len(), 10);
        let l = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (50.0, 40.0), (0.0, 100.0)];
        assert_eq!(triangulate(&l).len(), 3);
        assert_eq!(dashes(&[(0.0, 0.0), (100.0, 0.0)], 10.0, 10.0).len(), 5);
    }
}
