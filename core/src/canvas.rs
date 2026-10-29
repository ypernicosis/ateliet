//! Geometry shared by the free-form surfaces (moodboards and the Journal's
//! pages): boxes, rotation about a centre, and snapping a moving box to
//! other boxes and to lines.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && self.x + self.w > o.x && self.y < o.y + o.h && self.y + self.h > o.y
    }

    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.x + o.w <= self.x + self.w && o.y + o.h <= self.y + self.h
    }
}

pub fn rot_pt(x: f64, y: f64, cx: f64, cy: f64, deg: f64) -> (f64, f64) {
    if deg == 0.0 {
        return (x, y);
    }
    let (s, c) = deg.to_radians().sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

/// A snapping guide: a vertical (`v`) or horizontal line at `at`, drawn from
/// `from` to `to` along the other axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub v: bool,
    pub at: f64,
    pub from: f64,
    pub to: f64,
}

/// Aligns a moving box's edges and centre with nearby boxes within `th`.
pub fn snap(bb0: Rect, mut dx: f64, mut dy: f64, others: &[Rect], th: f64) -> (f64, f64, Vec<Guide>) {
    let b = Rect { x: bb0.x + dx, y: bb0.y + dy, ..bb0 };
    let mx = [b.x, b.x + b.w / 2.0, b.x + b.w];
    let my = [b.y, b.y + b.h / 2.0, b.y + b.h];
    let mut best_x: Option<(f64, f64, Rect)> = None;
    let mut best_y: Option<(f64, f64, Rect)> = None;
    for o in others {
        for m in mx {
            for v in [o.x, o.x + o.w / 2.0, o.x + o.w] {
                let d = v - m;
                if d.abs() < th && best_x.is_none_or(|bx| d.abs() < bx.0.abs()) {
                    best_x = Some((d, v, *o));
                }
            }
        }
        for m in my {
            for v in [o.y, o.y + o.h / 2.0, o.y + o.h] {
                let d = v - m;
                if d.abs() < th && best_y.is_none_or(|by| d.abs() < by.0.abs()) {
                    best_y = Some((d, v, *o));
                }
            }
        }
    }
    if let Some((d, ..)) = best_x {
        dx += d;
    }
    if let Some((d, ..)) = best_y {
        dy += d;
    }
    let nb = Rect { x: bb0.x + dx, y: bb0.y + dy, ..bb0 };
    let mut guides = Vec::new();
    if let Some((_, at, o)) = best_x {
        guides.push(Guide { v: true, at, from: nb.y.min(o.y), to: (nb.y + nb.h).max(o.y + o.h) });
    }
    if let Some((_, at, o)) = best_y {
        guides.push(Guide { v: false, at, from: nb.x.min(o.x), to: (nb.x + nb.w).max(o.x + o.w) });
    }
    (dx, dy, guides)
}


/// Snaps a moving box's edges and centre to fixed lines too: vertical lines
/// at `xs`, horizontal ones at `ys` (a page's margins, columns and ruling).
/// The boxes in `others` and the lines compete; the nearest wins on each axis.
pub fn snap_lines(bb0: Rect, dx: f64, dy: f64, others: &[Rect], xs: &[f64], ys: &[f64], th: f64) -> (f64, f64, Vec<Guide>) {
    let (mut dx2, mut dy2, mut guides) = snap(bb0, dx, dy, others, th);
    let b = Rect { x: bb0.x + dx, y: bb0.y + dy, ..bb0 };
    let near = |marks: [f64; 3], lines: &[f64]| -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64)> = None;
        for m in marks {
            for &v in lines {
                let d = v - m;
                if d.abs() < th && best.is_none_or(|b| d.abs() < b.0.abs()) {
                    best = Some((d, v));
                }
            }
        }
        best
    };
    if let Some((d, at)) = near([b.x, b.x + b.w / 2.0, b.x + b.w], xs) {
        if !guides.iter().any(|g| g.v) || d.abs() < (dx2 - dx).abs() {
            guides.retain(|g| !g.v);
            dx2 = dx + d;
            guides.push(Guide { v: true, at, from: f64::NEG_INFINITY, to: f64::INFINITY });
        }
    }
    if let Some((d, at)) = near([b.y, b.y + b.h / 2.0, b.y + b.h], ys) {
        if !guides.iter().any(|g| !g.v) || d.abs() < (dy2 - dy).abs() {
            guides.retain(|g| g.v);
            dy2 = dy + d;
            guides.push(Guide { v: false, at, from: f64::NEG_INFINITY, to: f64::INFINITY });
        }
    }
    (dx2, dy2, guides)
}

/// The four corners of a box `x, y, w, h` turned `r` degrees about its centre
/// (top left, top right, bottom right, bottom left).
pub fn corners(x: f64, y: f64, w: f64, h: f64, r: f64) -> [(f64, f64); 4] {
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(px, py)| rot_pt(px, py, cx, cy, r))
}

/// Is a point inside a turned box?
pub fn contains(x: f64, y: f64, w: f64, h: f64, r: f64, px: f64, py: f64) -> bool {
    let (lx, ly) = rot_pt(px, py, x + w / 2.0, y + h / 2.0, -r);
    lx >= x && lx <= x + w && ly >= y && ly <= y + h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_win_when_nearer() {
        let b = Rect { x: 10.0, y: 10.0, w: 20.0, h: 10.0 };
        let (dx, dy, g) = snap_lines(b, 0.4, 0.0, &[], &[11.0], &[], 2.0);
        assert!((dx - 1.0).abs() < 1e-9 && dy == 0.0);
        assert_eq!(g.len(), 1);
        assert!(contains(0.0, 0.0, 10.0, 4.0, 90.0, 5.0, 6.0));
        assert!(!contains(0.0, 0.0, 10.0, 4.0, 0.0, 5.0, 6.0));
    }
}
