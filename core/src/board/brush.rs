//! Brushes: a stroke is a run of dabs along the smoothed pointer path, spaced
//! by a fraction of their size. Pencil draws straight onto the layer and
//! builds up where lines cross; pen, brush, marker and eraser draw into a
//! stroke buffer that goes onto the layer when the stroke ends, with the
//! stroke's opacity, so a stroke never darkens where it overlaps itself.

use super::tile::DabMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BrushKind {
    Pencil,
    Pen,
    Brush,
    Marker,
    Eraser,
}

impl BrushKind {
    pub const ALL: [BrushKind; 5] = [BrushKind::Pencil, BrushKind::Pen, BrushKind::Brush, BrushKind::Marker, BrushKind::Eraser];

    pub fn name(&self) -> &'static str {
        match self {
            BrushKind::Pencil => "Pencil",
            BrushKind::Pen => "Pen",
            BrushKind::Brush => "Brush",
            BrushKind::Marker => "Marker",
            BrushKind::Eraser => "Eraser",
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            BrushKind::Pencil => "pencil",
            BrushKind::Pen => "pen",
            BrushKind::Brush => "brush",
            BrushKind::Marker => "marker",
            BrushKind::Eraser => "eraser",
        }
    }

    /// Straight onto the layer (no stroke buffer).
    pub fn direct(&self) -> bool {
        *self == BrushKind::Pencil
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub size: f32,
    /// 0..1
    pub opacity: f32,
    /// 0 (raw) .. 0.95 (very steady)
    pub smoothing: f32,
    /// 0 (soft) .. 1 (hard)
    pub hardness: f32,
    pub pressure_size: bool,
    pub pressure_opacity: bool,
}

impl BrushSettings {
    pub fn defaults(k: BrushKind) -> BrushSettings {
        let s = |size, opacity, smoothing, hardness, ps, po| BrushSettings { size, opacity, smoothing, hardness, pressure_size: ps, pressure_opacity: po };
        match k {
            BrushKind::Pencil => s(4.0, 1.0, 0.36, 1.0, true, true),
            BrushKind::Pen => s(5.0, 1.0, 0.5, 1.0, true, false),
            BrushKind::Brush => s(40.0, 0.6, 0.3, 0.15, false, true),
            BrushKind::Marker => s(24.0, 0.5, 0.3, 0.85, false, false),
            BrushKind::Eraser => s(24.0, 1.0, 0.2, 0.9, false, false),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    /// Pressure 0..1 (a mouse is a firm, even 1).
    pub p: f32,
}

/// One dab to paint: centre, radius, hardness, alpha and how it combines.
#[derive(Clone, Copy, Debug)]
pub struct Dab {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub hardness: f32,
    pub alpha: f32,
    pub mode: DabMode,
}

pub struct Stroke {
    pub kind: BrushKind,
    pub s: BrushSettings,
    smoothed: Option<Point>,
    last: Option<Point>,
    carry: f32,
    /// Canvas bounds touched so far.
    pub bounds: (f32, f32, f32, f32),
}

impl Stroke {
    pub fn new(kind: BrushKind, s: BrushSettings) -> Stroke {
        Stroke { kind, s, smoothed: None, last: None, carry: 0.0, bounds: (f32::MAX, f32::MAX, f32::MIN, f32::MIN) }
    }

    /// Add a pointer position; returns the dabs it produced.
    pub fn add(&mut self, pt: Point) -> Vec<Dab> {
        let k = self.s.smoothing.min(0.95);
        let sm = match self.smoothed {
            None => pt,
            Some(s) => {
                let a = 1.0 - k;
                Point { x: s.x + (pt.x - s.x) * a, y: s.y + (pt.y - s.y) * a, p: s.p + (pt.p - s.p) * a.max(0.35) }
            }
        };
        self.smoothed = Some(sm);
        self.segment(sm)
    }

    /// Catch up with the pointer so the line ends where the pen lifted.
    pub fn end(&mut self, pt: Option<Point>) -> Vec<Dab> {
        let mut out = Vec::new();
        if let (Some(pt), Some(from)) = (pt, self.smoothed) {
            for i in 1..=6 {
                let t = i as f32 / 6.0;
                out.extend(self.segment(Point { x: from.x + (pt.x - from.x) * t, y: from.y + (pt.y - from.y) * t, p: from.p + (pt.p - from.p) * t }));
            }
        }
        out
    }

    fn dab_at(&mut self, p: Point) -> Dab {
        let s = &self.s;
        let pressure = p.p.clamp(0.05, 1.0);
        let size = if s.pressure_size { s.size * (0.25 + 0.75 * pressure) } else { s.size };
        let r = (size / 2.0).max(0.5);
        let alpha = match self.kind {
            // graphite: light dabs that build up
            BrushKind::Pencil => (if s.pressure_opacity { s.opacity * (0.35 + 0.65 * pressure) } else { s.opacity }) * 0.55,
            BrushKind::Brush => if s.pressure_opacity { pressure } else { 1.0 },
            _ => 1.0,
        };
        let mode = if self.kind.direct() { DabMode::Over } else { DabMode::Max };
        let b = &mut self.bounds;
        b.0 = b.0.min(p.x - r - 1.0);
        b.1 = b.1.min(p.y - r - 1.0);
        b.2 = b.2.max(p.x + r + 1.0);
        b.3 = b.3.max(p.y + r + 1.0);
        Dab { x: p.x, y: p.y, radius: r, hardness: s.hardness, alpha, mode }
    }

    fn segment(&mut self, to: Point) -> Vec<Dab> {
        let mut out = Vec::new();
        let Some(from) = self.last else {
            self.last = Some(to);
            out.push(self.dab_at(to));
            return out;
        };
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let spacing = (self.s.size * if self.s.hardness > 0.7 { 0.12 } else { 0.08 }).max(0.6);
        let mut t = spacing - self.carry;
        while t <= dist {
            let f = t / dist.max(1e-6);
            out.push(self.dab_at(Point { x: from.x + dx * f, y: from.y + dy * f, p: from.p + (to.p - from.p) * f }));
            t += spacing;
        }
        self.carry = (self.carry + dist) % spacing;
        self.last = Some(to);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_spacing() {
        let mut s = Stroke::new(BrushKind::Pen, BrushSettings { smoothing: 0.0, ..BrushSettings::defaults(BrushKind::Pen) });
        let mut dabs = s.add(Point { x: 0.0, y: 0.0, p: 1.0 });
        dabs.extend(s.add(Point { x: 100.0, y: 0.0, p: 1.0 }));
        assert!(dabs.len() > 100);
        assert!(dabs.windows(2).all(|w| (w[1].x - w[0].x) <= 0.7));
        assert!(s.bounds.2 >= 100.0);
    }
}
