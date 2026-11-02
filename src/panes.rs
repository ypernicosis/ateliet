//! The centre divided in panes: a tree of splits, each one a row (side by
//! side) or a column (one above the other) of panes or of other splits. A
//! leaf holds the `uid` of the tab it shows; the same note can be in two
//! leaves through two tabs of the same path (see `App::clone_tab`).

use eframe::egui::{self, Pos2, Rect};
use serde_json::{json, Value};

/// The smallest a pane gets while a line between panes is dragged, in points.
pub const MIN_PANE: f32 = 240.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Leaf(u64),
    /// `down`: the kids are stacked (one above the other); else side by side.
    /// `fracs` are the kids' shares of the length, summing to 1.
    Split { down: bool, kids: Vec<Node>, fracs: Vec<f32> },
}

/// Where a pane dropped onto another one goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
    /// In place of it (the two swap).
    Center,
}

impl Side {
    fn down(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
    fn after(self) -> bool {
        matches!(self, Side::Right | Side::Bottom)
    }

    /// The side of `rect` the pointer is nearest, or the middle when it is in
    /// the middle third.
    pub fn at(rect: Rect, pos: Pos2) -> Side {
        let u = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        let v = ((pos.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        if (0.3..=0.7).contains(&u) && (0.3..=0.7).contains(&v) {
            return Side::Center;
        }
        // the nearest edge, as a fraction of each length
        let d = [(u, Side::Left), (1.0 - u, Side::Right), (v, Side::Top), (1.0 - v, Side::Bottom)];
        d.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1).unwrap_or(Side::Center)
    }

    /// The part of `rect` a pane dropped here would take.
    pub fn preview(self, rect: Rect) -> Rect {
        let (w, h) = (rect.width() / 2.0, rect.height() / 2.0);
        match self {
            Side::Left => Rect::from_min_size(rect.min, egui::vec2(w, rect.height())),
            Side::Right => Rect::from_min_max(egui::pos2(rect.right() - w, rect.top()), rect.max),
            Side::Top => Rect::from_min_size(rect.min, egui::vec2(rect.width(), h)),
            Side::Bottom => Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - h), rect.max),
            Side::Center => rect,
        }
    }
}

/// A line between two panes: the split it belongs to (as a path of kid
/// indices from the root), the kid before it, and the split's rect.
#[derive(Clone, Debug)]
pub struct Handle {
    pub path: Vec<usize>,
    pub k: usize,
    pub down: bool,
    /// The line itself (zero thickness along its axis).
    pub at: f32,
    pub span: Rect,
}

impl Node {
    pub fn leaves(&self) -> Vec<u64> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<u64>) {
        match self {
            Node::Leaf(u) => out.push(*u),
            Node::Split { kids, .. } => kids.iter().for_each(|k| k.collect(out)),
        }
    }

    pub fn contains(&self, uid: u64) -> bool {
        match self {
            Node::Leaf(u) => *u == uid,
            Node::Split { kids, .. } => kids.iter().any(|k| k.contains(uid)),
        }
    }

    /// Put `new` in the leaf of `old`.
    pub fn replace(&mut self, old: u64, new: u64) -> bool {
        match self {
            Node::Leaf(u) if *u == old => {
                *u = new;
                true
            }
            Node::Leaf(_) => false,
            Node::Split { kids, .. } => kids.iter_mut().any(|k| k.replace(old, new)),
        }
    }

    /// Exchange the places of two leaves.
    pub fn swap(&mut self, a: u64, b: u64) {
        const TMP: u64 = u64::MAX;
        self.replace(a, TMP);
        self.replace(b, a);
        self.replace(TMP, b);
    }

    /// Take the leaf of `uid` out; its share goes to its neighbours. `None`
    /// when nothing is left.
    pub fn remove(self, uid: u64) -> Option<Node> {
        match self {
            Node::Leaf(u) if u == uid => None,
            Node::Leaf(u) => Some(Node::Leaf(u)),
            Node::Split { down, kids, fracs } => {
                let mut nk = Vec::new();
                let mut nf = Vec::new();
                for (k, f) in kids.into_iter().zip(fracs) {
                    if let Some(k) = k.remove(uid) {
                        nk.push(k);
                        nf.push(f);
                    }
                }
                Node::Split { down, kids: nk, fracs: nf }.normalized()
            }
        }
    }

    /// Splits of one kid become that kid; a split inside one of the same
    /// direction is flattened into it; shares sum to 1.
    pub fn normalized(self) -> Option<Node> {
        match self {
            Node::Leaf(u) => Some(Node::Leaf(u)),
            Node::Split { down, kids, fracs } => {
                let mut nk = Vec::new();
                let mut nf = Vec::new();
                for (k, f) in kids.into_iter().zip(fracs) {
                    match k.normalized() {
                        Some(Node::Split { down: d2, kids: k2, fracs: f2 }) if d2 == down => {
                            for (a, b) in k2.into_iter().zip(f2) {
                                nk.push(a);
                                nf.push(b * f);
                            }
                        }
                        Some(k) => {
                            nk.push(k);
                            nf.push(f);
                        }
                        None => {}
                    }
                }
                match nk.len() {
                    0 => None,
                    1 => nk.pop(),
                    _ => {
                        let sum: f32 = nf.iter().sum();
                        let n = nf.len() as f32;
                        let nf = if sum > 0.0 { nf.iter().map(|f| f / sum).collect() } else { vec![1.0 / n; nf.len()] };
                        Some(Node::Split { down, kids: nk, fracs: nf })
                    }
                }
            }
        }
    }

    /// Put a leaf for `new` beside the leaf of `target`. Next to it in the
    /// same split when that split runs the same way, else the target's leaf
    /// becomes a split of the two. The two share the target's room.
    pub fn insert(&mut self, target: u64, new: u64, side: Side) -> bool {
        if side == Side::Center {
            return self.replace(target, new);
        }
        let down = side.down();
        match self {
            Node::Leaf(u) if *u == target => {
                let (a, b) = if side.after() { (Node::Leaf(*u), Node::Leaf(new)) } else { (Node::Leaf(new), Node::Leaf(*u)) };
                *self = Node::Split { down, kids: vec![a, b], fracs: vec![0.5, 0.5] };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { down: d, kids, fracs } => {
                if *d == down {
                    if let Some(i) = kids.iter().position(|k| matches!(k, Node::Leaf(u) if *u == target)) {
                        let half = fracs[i] / 2.0;
                        fracs[i] = half;
                        let at = if side.after() { i + 1 } else { i };
                        kids.insert(at, Node::Leaf(new));
                        fracs.insert(at, half);
                        return true;
                    }
                }
                kids.iter_mut().any(|k| k.insert(target, new, side))
            }
        }
    }

    /// Every leaf's rect, and the lines between panes.
    pub fn layout(&self, rect: Rect) -> (Vec<(u64, Rect)>, Vec<Handle>) {
        let mut panes = Vec::new();
        let mut handles = Vec::new();
        self.lay(rect, &mut Vec::new(), &mut panes, &mut handles);
        (panes, handles)
    }

    fn lay(&self, rect: Rect, path: &mut Vec<usize>, panes: &mut Vec<(u64, Rect)>, handles: &mut Vec<Handle>) {
        match self {
            Node::Leaf(u) => panes.push((*u, rect)),
            Node::Split { down, kids, fracs } => {
                let len = if *down { rect.height() } else { rect.width() };
                let start = if *down { rect.top() } else { rect.left() };
                let mut pos = start;
                for (i, (k, f)) in kids.iter().zip(fracs).enumerate() {
                    let end = if i + 1 == kids.len() { start + len } else { (pos + len * f).round() };
                    // half a point of room on each side of a line
                    let a = if i == 0 { pos } else { pos + 0.5 };
                    let b = if i + 1 == kids.len() { end } else { end - 0.5 };
                    let r = if *down { Rect::from_x_y_ranges(rect.x_range(), a..=b) } else { Rect::from_x_y_ranges(a..=b, rect.y_range()) };
                    path.push(i);
                    k.lay(r, path, panes, handles);
                    path.pop();
                    if i + 1 < kids.len() {
                        handles.push(Handle { path: path.clone(), k: i, down: *down, at: end, span: rect });
                    }
                    pos = end;
                }
            }
        }
    }

    fn at_path(&mut self, path: &[usize]) -> Option<&mut Node> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => match self {
                Node::Split { kids, .. } => kids.get_mut(*i)?.at_path(rest),
                Node::Leaf(_) => None,
            },
        }
    }

    /// Move the line `h` to `pos` (along its axis), keeping the two panes
    /// around it no smaller than `MIN_PANE`.
    pub fn drag(&mut self, h: &Handle, pos: f32) {
        let Some(Node::Split { fracs, .. }) = self.at_path(&h.path) else { return };
        let len = if h.down { h.span.height() } else { h.span.width() }.max(1.0);
        let start = if h.down { h.span.top() } else { h.span.left() };
        let before: f32 = fracs[..h.k].iter().sum();
        let pair = fracs[h.k] + fracs[h.k + 1];
        let min = (MIN_PANE / len).min(pair / 2.0);
        let f = ((pos - start) / len - before).clamp(min, pair - min);
        fracs[h.k] = f;
        fracs[h.k + 1] = pair - f;
    }

    /// Shares changed so that no pane is narrower (or shorter) than
    /// `MIN_PANE` in `rect`, when there is room for that: the room comes from
    /// the bigger ones.
    pub fn fit(&mut self, rect: Rect) {
        let Node::Split { down, kids, fracs } = self else { return };
        let len = if *down { rect.height() } else { rect.width() }.max(1.0);
        let n = fracs.len() as f32;
        let min = (MIN_PANE / len).min(1.0 / n);
        for _ in 0..4 {
            let short: f32 = fracs.iter().map(|f| (min - f).max(0.0)).sum();
            if short <= 1e-4 {
                break;
            }
            let spare: f32 = fracs.iter().map(|f| (f - min).max(0.0)).sum();
            if spare <= 1e-4 {
                break;
            }
            let take = (short / spare).min(1.0);
            for f in fracs.iter_mut() {
                *f = if *f < min { min } else { *f - (*f - min) * take };
            }
        }
        let sum: f32 = fracs.iter().sum();
        fracs.iter_mut().for_each(|f| *f /= sum);
        // and inside each kid, with the room it got
        let mut pos = if *down { rect.top() } else { rect.left() };
        for (k, f) in kids.iter_mut().zip(fracs.iter()) {
            let l = len * f;
            let r = if *down { Rect::from_x_y_ranges(rect.x_range(), pos..=pos + l) } else { Rect::from_x_y_ranges(pos..=pos + l, rect.y_range()) };
            k.fit(r);
            pos += l;
        }
    }

    /// The two panes around line `h` back to the same size.
    pub fn even(&mut self, h: &Handle) {
        let Some(Node::Split { fracs, .. }) = self.at_path(&h.path) else { return };
        let pair = fracs[h.k] + fracs[h.k + 1];
        fracs[h.k] = pair / 2.0;
        fracs[h.k + 1] = pair / 2.0;
    }

    /// Every pane of each split the same size.
    pub fn even_all(&mut self) {
        if let Node::Split { kids, fracs, .. } = self {
            let n = fracs.len() as f32;
            fracs.iter_mut().for_each(|f| *f = 1.0 / n);
            kids.iter_mut().for_each(Node::even_all);
        }
    }

    /// Every split turned the other way (side by side ⇄ stacked).
    pub fn flip(&mut self) {
        if let Node::Split { down, kids, .. } = self {
            *down = !*down;
            kids.iter_mut().for_each(Node::flip);
        }
    }

    /// Whether the outermost split stacks the panes.
    pub fn stacked(&self) -> bool {
        matches!(self, Node::Split { down: true, .. })
    }

    /// For the session file, with each leaf as `name(uid)`.
    pub fn to_json(&self, name: &dyn Fn(u64) -> Option<Value>) -> Value {
        match self {
            Node::Leaf(u) => name(*u).unwrap_or(Value::Null),
            Node::Split { down, kids, fracs } => json!({ "down": down, "kids": kids.iter().map(|k| k.to_json(name)).collect::<Vec<_>>(), "fracs": fracs }),
        }
    }

    /// Back from the session file; leaves that `uid` cannot place are left out.
    pub fn from_json(v: &Value, uid: &dyn Fn(&Value) -> Option<u64>) -> Option<Node> {
        if let Some(kids) = v["kids"].as_array() {
            let fracs = v["fracs"].as_array();
            let mut nk = Vec::new();
            let mut nf = Vec::new();
            for (i, k) in kids.iter().enumerate() {
                if let Some(n) = Node::from_json(k, uid) {
                    nk.push(n);
                    nf.push(fracs.and_then(|f| f.get(i)).and_then(Value::as_f64).unwrap_or(1.0).max(0.02) as f32);
                }
            }
            return Node::Split { down: v["down"].as_bool().unwrap_or(false), kids: nk, fracs: nf }.normalized();
        }
        uid(v).map(Node::Leaf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 600.0))
    }

    #[test]
    fn insert_beside_and_below() {
        let mut n = Node::Leaf(1);
        assert!(n.insert(1, 2, Side::Right));
        assert!(n.insert(2, 3, Side::Right));
        assert_eq!(n.leaves(), vec![1, 2, 3]);
        // the same direction: a third pane in the same row
        assert!(matches!(&n, Node::Split { down: false, kids, .. } if kids.len() == 3));
        assert!(n.insert(3, 4, Side::Bottom));
        assert_eq!(n.leaves(), vec![1, 2, 3, 4]);
        let (panes, handles) = n.layout(r());
        assert_eq!(panes.len(), 4);
        assert_eq!(handles.len(), 3);
        let r4 = panes.iter().find(|p| p.0 == 4).unwrap().1;
        let r3 = panes.iter().find(|p| p.0 == 3).unwrap().1;
        assert!(r4.top() > r3.bottom() - 1.0);
        assert!((r4.width() - r3.width()).abs() < 0.01);
    }

    #[test]
    fn remove_collapses() {
        let mut n = Node::Leaf(1);
        n.insert(1, 2, Side::Right);
        n.insert(2, 3, Side::Bottom);
        let n = n.remove(2).unwrap();
        assert_eq!(n.leaves(), vec![1, 3]);
        assert!(matches!(&n, Node::Split { down: false, kids, .. } if kids.len() == 2));
        let n = n.remove(1).unwrap();
        assert_eq!(n, Node::Leaf(3));
        assert!(n.remove(3).is_none());
    }

    #[test]
    fn drag_keeps_minimum() {
        let mut n = Node::Leaf(1);
        n.insert(1, 2, Side::Right);
        n.insert(2, 3, Side::Right);
        // the new pane shares the room of the one it was put beside
        let Node::Split { fracs, .. } = &n else { panic!() };
        assert_eq!(fracs, &vec![0.5, 0.25, 0.25]);
        let (_, h) = n.layout(r());
        n.drag(&h[0], -500.0);
        let (panes, _) = n.layout(r());
        assert!(panes[0].1.width() >= MIN_PANE - 1.0);
        // the third pane did not move
        let Node::Split { fracs, .. } = &n else { panic!() };
        assert!((fracs[2] - 0.25).abs() < 0.001);
        assert!((fracs.iter().sum::<f32>() - 1.0).abs() < 0.001);
    }

    #[test]
    fn fit_gives_small_panes_room() {
        let mut n = Node::Split { down: false, kids: vec![Node::Leaf(1), Node::Leaf(2), Node::Leaf(3)], fracs: vec![0.1, 0.1, 0.8] };
        n.fit(r());
        let (panes, _) = n.layout(r());
        assert!(panes.iter().all(|p| p.1.width() >= MIN_PANE - 1.5), "{panes:?}");
        // no room for everyone: the same size each
        let mut n = Node::Split { down: false, kids: vec![Node::Leaf(1), Node::Leaf(2), Node::Leaf(3), Node::Leaf(4), Node::Leaf(5)], fracs: vec![0.05, 0.05, 0.05, 0.05, 0.8] };
        n.fit(r());
        let Node::Split { fracs, .. } = &n else { panic!() };
        assert!(fracs.iter().all(|f| (f - 0.2).abs() < 0.01), "{fracs:?}");
    }

    #[test]
    fn side_from_pointer() {
        let rect = r();
        assert_eq!(Side::at(rect, egui::pos2(500.0, 300.0)), Side::Center);
        assert_eq!(Side::at(rect, egui::pos2(20.0, 300.0)), Side::Left);
        assert_eq!(Side::at(rect, egui::pos2(980.0, 300.0)), Side::Right);
        assert_eq!(Side::at(rect, egui::pos2(500.0, 10.0)), Side::Top);
        assert_eq!(Side::at(rect, egui::pos2(500.0, 590.0)), Side::Bottom);
    }

    #[test]
    fn json_round_trip_and_swap() {
        let mut n = Node::Leaf(1);
        n.insert(1, 2, Side::Bottom);
        n.insert(2, 3, Side::Left);
        let j = n.to_json(&|u| Some(json!(u)));
        let back = Node::from_json(&j, &|v| v.as_u64()).unwrap();
        assert_eq!(back.leaves(), n.leaves());
        n.swap(1, 3);
        assert_eq!(n.leaves(), vec![3, 1, 2]);
        n.flip();
        assert!(!n.stacked());
    }
}
