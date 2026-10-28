//! Changes to the quadro: what is under the pointer, selection, moving,
//! deleting, grouping and order, arrows that follow what they point to,
//! text inside shapes, cards and columns, copy and paste (Excalidraw's
//! clipboard JSON, so drawings travel to and from excalidraw.com).

use super::{draw, Cx, Out, QuadroEditor, Sel};
use ateliet_core::quadro::element::{self, dist, rotate, Element};
use ateliet_core::quadro::layout;
use ateliet_core::quadro::{Card, FileRef, Format, Lane, CARD_W, LANE_GAP, LANE_W};
use eframe::egui::{self, pos2};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// What is under a point.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Check(String),
    Card(String),
    /// A column's header button: 0 collapse, 1 add a card, 2 menu.
    LaneButton(usize, usize),
    LaneFoot(usize),
    LaneHead(usize),
    Lane(usize),
    El(String),
}

type P = (f32, f32);

const BINDABLE: &[&str] = &["rectangle", "diamond", "ellipse", "text", "image", "embeddable", "iframe", "frame", "magicframe"];

impl QuadroEditor {
    pub(super) fn hit(&self, w: P, tol: f32) -> Option<Hit> {
        let q = pos2(w.0, w.1);
        for cb in self.scene.loose.iter().rev() {
            if draw::rr(cb.rect).contains(q) {
                if draw::check_rect(cb).expand(3.0).contains(q) {
                    return Some(Hit::Check(cb.id.clone()));
                }
                return Some(Hit::Card(cb.id.clone()));
            }
        }
        // what is drawn over the columns is hit before them
        if let Some(h) = self.hit_elements(w, tol, true) {
            return Some(h);
        }
        for (i, lb) in self.scene.lanes.iter().enumerate().rev() {
            let r = draw::rr(lb.rect);
            if !r.contains(q) {
                continue;
            }
            for (k, b) in draw::lane_buttons(lb).iter().enumerate() {
                if b.contains(q) {
                    return Some(Hit::LaneButton(i, k));
                }
            }
            for cb in &lb.cards {
                if draw::rr(cb.rect).contains(q) {
                    if draw::check_rect(cb).expand(3.0).contains(q) {
                        return Some(Hit::Check(cb.id.clone()));
                    }
                    return Some(Hit::Card(cb.id.clone()));
                }
            }
            if !self.board.lanes[i].collapsed && draw::lane_foot(lb).contains(q) {
                return Some(Hit::LaneFoot(i));
            }
            if w.1 < lb.rect.1 + layout::HEADER_H {
                return Some(Hit::LaneHead(i));
            }
            return Some(Hit::Lane(i));
        }
        self.hit_elements(w, tol, false)
    }

    fn hit_elements(&self, w: P, tol: f32, over: bool) -> Option<Hit> {
        for e in self.board.elements.iter().rev() {
            if e.deleted() || e.locked() || super::is_overlay(e) != over {
                continue;
            }
            if e.hit(w.0, w.1, tol) {
                // a shape's text picks the shape
                let id = e.container().filter(|c| self.board.element(c).is_some()).unwrap_or(e.id());
                return Some(Hit::El(id.to_string()));
            }
        }
        None
    }

    /// An element with the others of its outermost group.
    pub(super) fn with_group(&self, id: &str) -> Vec<String> {
        let Some(e) = self.board.element(id) else { return vec![id.to_string()] };
        match e.group_ids().last() {
            Some(g) => self.board.elements.iter().filter(|x| !x.deleted() && x.group_ids().contains(g)).map(|x| x.id().to_string()).collect(),
            None => vec![id.to_string()],
        }
    }

    pub(super) fn select_hit(&mut self, h: &Hit, add: bool) {
        let items: Vec<Sel> = match h {
            Hit::El(id) => self.with_group(id).into_iter().map(Sel::El).collect(),
            Hit::Card(id) | Hit::Check(id) => vec![Sel::Card(id.clone())],
            Hit::LaneHead(i) | Hit::Lane(i) | Hit::LaneButton(i, _) | Hit::LaneFoot(i) => vec![Sel::Lane(self.board.lanes[*i].id.clone())],
        };
        if add {
            if items.iter().all(|s| self.sel.contains(s)) {
                self.sel.retain(|s| !items.contains(s));
            } else {
                for s in items {
                    if !self.sel.contains(&s) {
                        self.sel.push(s);
                    }
                }
            }
        } else if !items.iter().all(|s| self.sel.contains(s)) {
            self.sel = items;
        }
    }

    pub(super) fn sel_els(&self) -> Vec<String> {
        self.sel.iter().filter_map(|s| if let Sel::El(id) = s { Some(id.clone()) } else { None }).collect()
    }

    /// World box of a selected thing.
    pub(super) fn sel_rect(&self, s: &Sel) -> Option<(f32, f32, f32, f32)> {
        match s {
            Sel::El(id) => self.board.element(id).map(|e| e.aabb()),
            Sel::Card(id) => self.scene.card(id).map(|c| (c.rect.0, c.rect.1, c.rect.0 + c.rect.2, c.rect.1 + c.rect.3)),
            Sel::Lane(id) => self.scene.lanes.iter().find(|l| &l.id == id).map(|l| (l.rect.0, l.rect.1, l.rect.0 + l.rect.2, l.rect.1 + l.rect.3)),
        }
    }

    pub(super) fn selection_box(&self) -> Option<(f32, f32, f32, f32)> {
        self.sel.iter().filter_map(|s| self.sel_rect(s)).reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    /// Elements that move with the selection: the selected ones and the
    /// text inside them.
    fn moving_els(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sel_els();
        for e in &self.board.elements {
            if e.container().is_some_and(|c| ids.iter().any(|i| i == c)) && !ids.iter().any(|i| i == e.id()) {
                ids.push(e.id().to_string());
            }
        }
        ids
    }

    pub(super) fn move_sel(&mut self, dx: f32, dy: f32) {
        for id in self.moving_els() {
            if let Some(e) = self.board.element_mut(&id) {
                e.translate(dx, dy);
            }
        }
        let sel = self.sel.clone();
        for s in &sel {
            match s {
                Sel::Card(id) => {
                    if let Some(c) = self.board.cards.iter_mut().find(|c| &c.id == id) {
                        c.x += dx;
                        c.y += dy;
                    }
                }
                Sel::Lane(id) => {
                    if let Some(l) = self.board.lanes.iter_mut().find(|l| &l.id == id) {
                        l.x += dx;
                        l.y += dy;
                    }
                }
                Sel::El(_) => {}
            }
        }
        self.changed();
    }

    /// After moving: arrows moved without what they point to come loose.
    pub(super) fn unbind_moved_arrows(&mut self) {
        let moving: HashSet<String> = self.moving_els().into_iter().chain(self.sel.iter().filter_map(|s| if let Sel::Card(id) = s { Some(id.clone()) } else { None })).collect();
        for id in self.sel_els() {
            for end in [false, true] {
                let Some(t) = self.board.element(&id).and_then(|e| if e.kind() == "arrow" { e.binding(end) } else { None }) else { continue };
                if !moving.contains(&t) {
                    self.set_binding(&id, end, None);
                }
            }
        }
    }

    pub(super) fn delete_sel(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint();
        let sel = std::mem::take(&mut self.sel);
        let mut gone: HashSet<String> = HashSet::new();
        for s in &sel {
            match s {
                Sel::El(id) => {
                    gone.insert(id.clone());
                }
                Sel::Card(id) => {
                    self.board.cards.retain(|c| &c.id != id);
                    for l in self.board.lanes.iter_mut() {
                        l.cards.retain(|c| &c.id != id);
                    }
                    gone.insert(id.clone());
                }
                Sel::Lane(id) => {
                    if let Some(l) = self.board.lanes.iter().find(|l| &l.id == id) {
                        gone.extend(l.cards.iter().map(|c| c.id.clone()));
                    }
                    self.board.lanes.retain(|l| &l.id != id);
                }
            }
        }
        // the text inside deleted shapes goes too
        for e in &self.board.elements {
            if e.container().is_some_and(|c| gone.contains(c)) {
                gone.insert(e.id().to_string());
            }
        }
        self.remove_elements(&gone);
        self.changed();
    }

    /// Take elements out: a Kanban quadro drops them; an Excalidraw file
    /// marks them deleted, as Excalidraw does. Bindings to them are undone.
    pub(super) fn remove_elements(&mut self, gone: &HashSet<String>) {
        let tomb = self.board.format != Format::Kanban;
        for e in self.board.elements.iter_mut() {
            if gone.contains(e.id()) {
                if tomb {
                    e.set("isDeleted", json!(true));
                }
                continue;
            }
            for (end, key) in [(false, "startBinding"), (true, "endBinding")] {
                if e.binding(end).is_some_and(|t| gone.contains(&t)) {
                    e.set(key, Value::Null);
                }
            }
            let bound = e.bound();
            for (b, _) in bound {
                if gone.contains(&b) {
                    e.remove_bound(&b);
                }
            }
            if e.container().is_some_and(|c| gone.contains(c)) {
                e.set("containerId", Value::Null);
            }
        }
        if !tomb {
            self.board.elements.retain(|e| !gone.contains(e.id()));
        }
    }

    /// Bring to the front (true) or send to the back.
    pub(super) fn reorder(&mut self, front: bool) {
        let ids: HashSet<String> = self.moving_els().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        let (mut picked, rest): (Vec<Element>, Vec<Element>) = std::mem::take(&mut self.board.elements).into_iter().partition(|e| ids.contains(e.id()));
        for e in picked.iter_mut() {
            e.touched = true;
        }
        self.board.elements = if front { rest.into_iter().chain(picked).collect() } else { picked.into_iter().chain(rest).collect() };
        self.changed();
    }

    pub(super) fn group(&mut self, on: bool) {
        let ids = self.sel_els();
        if ids.len() < 2 && on {
            return;
        }
        self.checkpoint();
        let g = element::new_id();
        for id in &ids {
            if let Some(e) = self.board.element_mut(id) {
                let mut gs = e.group_ids();
                if on {
                    gs.push(g.clone());
                } else {
                    gs.pop();
                }
                e.set("groupIds", json!(gs));
            }
        }
        self.changed();
    }

    /// Change the look of the selected elements (and of new ones).
    pub(super) fn restyle(&mut self, key: &str, v: Value) {
        let ids = self.moving_els();
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        for id in ids {
            if let Some(e) = self.board.element_mut(&id) {
                let applies = match key {
                    "fontSize" | "textAlign" | "fontFamily" => e.kind() == "text",
                    "startArrowhead" | "endArrowhead" => e.kind() == "arrow",
                    "backgroundColor" | "fillStyle" => !matches!(e.kind(), "text" | "line" | "arrow") || e.kind() == "line",
                    "roundness" => matches!(e.kind(), "rectangle" | "diamond" | "arrow" | "line"),
                    _ => true,
                };
                if applies {
                    let v = if key == "roundness" && v.as_bool() == Some(true) {
                        json!({ "type": if matches!(e.kind(), "arrow" | "line") { 2 } else { 3 } })
                    } else if key == "roundness" {
                        Value::Null
                    } else {
                        v.clone()
                    };
                    e.set(key, v);
                }
            }
        }
        self.changed();
    }

    // ── bindings ──────────────────────────────────────────────────────

    /// The outline of something an arrow can point to (world polygon).
    fn target_poly(&self, id: &str) -> Option<Vec<P>> {
        if let Some(e) = self.board.element(id).filter(|e| !e.deleted()) {
            let (x0, y0, x1, y1) = e.bounds();
            let c = e.center();
            let a = e.angle();
            let pts: Vec<P> = match e.kind() {
                "ellipse" => (0..36).map(|i| {
                    let t = i as f32 / 36.0 * std::f32::consts::TAU;
                    (c.0 + (x1 - x0) / 2.0 * t.cos(), c.1 + (y1 - y0) / 2.0 * t.sin())
                }).collect(),
                "diamond" => vec![(c.0, y0), (x1, c.1), (c.0, y1), (x0, c.1)],
                _ => vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
            };
            return Some(pts.into_iter().map(|p| rotate(p, c, a)).collect());
        }
        let cb = self.scene.card(id)?;
        let (x, y, w, h) = cb.rect;
        Some(vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)])
    }

    fn center_of(&self, id: &str) -> Option<P> {
        let poly = self.target_poly(id)?;
        let n = poly.len() as f32;
        Some(poly.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n)))
    }

    /// Put a bound arrow's ends on the edges of what they point to.
    pub(super) fn reroute(&mut self, id: &str) {
        let Some(a) = self.board.element(id) else { return };
        let pts = a.abs_points();
        let n = pts.len();
        if n < 2 {
            return;
        }
        let mut new = pts.clone();
        for end in [false, true] {
            let Some(t) = a.binding(end) else { continue };
            let Some(poly) = self.target_poly(&t) else { continue };
            let idx = if end { n - 1 } else { 0 };
            let other = if end { n - 2 } else { 1 };
            let aim = if n == 2 { a.binding(!end).and_then(|o| self.center_of(&o)).unwrap_or(pts[other]) } else { pts[other] };
            let c = self.center_of(&t).unwrap_or(aim);
            let gap = a.get(if end { "endBinding" } else { "startBinding" }).get("gap").and_then(|g| g.as_f64()).unwrap_or(6.0) as f32;
            if let Some(p) = edge_point(&poly, aim, c) {
                let d = dist(p, aim).max(0.001);
                let g = gap.min(d * 0.5);
                new[idx] = (p.0 + (aim.0 - p.0) / d * g, p.1 + (aim.1 - p.1) / d * g);
            }
        }
        if new.iter().zip(&pts).any(|(a, b)| dist(*a, *b) > 0.3) {
            if let Some(a) = self.board.element_mut(id) {
                a.set_abs_points(&new);
            }
            self.changed();
        }
    }

    /// Arrows follow what they point to when it moved since the last frame.
    pub(super) fn follow_bindings(&mut self) {
        let mut now: HashMap<String, [f32; 5]> = HashMap::new();
        for e in self.board.elements.iter().filter(|e| !e.deleted() && BINDABLE.contains(&e.kind())) {
            let (x0, y0, x1, y1) = e.bounds();
            now.insert(e.id().to_string(), [x0, y0, x1, y1, e.angle()]);
        }
        for cb in self.scene.loose.iter().chain(self.scene.lanes.iter().flat_map(|l| l.cards.iter())) {
            now.insert(cb.id.clone(), [cb.rect.0, cb.rect.1, cb.rect.0 + cb.rect.2, cb.rect.1 + cb.rect.3, 0.0]);
        }
        let changed: HashSet<String> = now.iter().filter(|(k, v)| self.prev_targets.get(*k).is_some_and(|p| p != *v)).map(|(k, _)| k.clone()).collect();
        self.prev_targets = now;
        if changed.is_empty() {
            return;
        }
        let arrows: Vec<String> = self.board.elements.iter().filter(|e| !e.deleted() && [e.binding(false), e.binding(true)].iter().flatten().any(|t| changed.contains(t))).map(|e| e.id().to_string()).collect();
        for a in arrows {
            self.reroute(&a);
        }
        // the text inside a shape that moved stays in its middle
        let containers: Vec<String> = changed.iter().filter(|id| self.board.element(id).is_some_and(|e| !e.bound().is_empty())).cloned().collect();
        for c in containers {
            self.center_bound_text(&c);
        }
    }

    /// What an arrow end at `w` could point to.
    pub(super) fn bind_target(&self, w: P, arrow: &str) -> Option<String> {
        for cb in self.scene.loose.iter().rev().chain(self.scene.lanes.iter().flat_map(|l| l.cards.iter())) {
            if draw::rr(cb.rect).expand(8.0).contains(pos2(w.0, w.1)) {
                return Some(cb.id.clone());
            }
        }
        for e in self.board.elements.iter().rev() {
            if e.deleted() || e.id() == arrow || !BINDABLE.contains(&e.kind()) || e.container().is_some() {
                continue;
            }
            let (x0, y0, x1, y1) = e.bounds();
            let c = e.center();
            let q = rotate(w, c, -e.angle());
            let inside = match e.kind() {
                "ellipse" => (((q.0 - c.0) / ((x1 - x0) / 2.0 + 8.0)).powi(2) + ((q.1 - c.1) / ((y1 - y0) / 2.0 + 8.0)).powi(2)) <= 1.0,
                _ => q.0 >= x0 - 8.0 && q.0 <= x1 + 8.0 && q.1 >= y0 - 8.0 && q.1 <= y1 + 8.0,
            };
            if inside {
                return Some(e.id().to_string());
            }
        }
        None
    }

    pub(super) fn set_binding(&mut self, arrow: &str, end: bool, target: Option<String>) {
        let key = if end { "endBinding" } else { "startBinding" };
        let old = self.board.element(arrow).and_then(|a| a.binding(end));
        if old == target {
            return;
        }
        if let Some(o) = &old {
            let other = self.board.element(arrow).and_then(|a| a.binding(!end));
            if other.as_deref() != Some(o.as_str()) {
                if let Some(t) = self.board.element_mut(o) {
                    t.remove_bound(arrow);
                }
            }
        }
        if let Some(a) = self.board.element_mut(arrow) {
            a.set(key, match &target {
                Some(t) => json!({ "elementId": t, "focus": 0, "gap": 6 }),
                None => Value::Null,
            });
        }
        if let Some(t) = &target {
            if let Some(te) = self.board.element_mut(t) {
                te.add_bound(arrow, "arrow");
            }
        }
        self.changed();
    }

    // ── text ──────────────────────────────────────────────────────────

    /// The size of a text in world units.
    pub(super) fn measure(&self, ctx: &egui::Context, text: &str, size: f32, wrap: f32, mono: bool) -> (f32, f32) {
        let g = draw::text_galley(ctx, if text.is_empty() { " " } else { text }, size, mono, egui::Color32::WHITE, wrap, "left");
        (g.size().x.ceil(), g.size().y.ceil())
    }

    /// Size a text element to its text (inside its shape, wrapped to it).
    pub(super) fn fit_text(&mut self, ctx: &egui::Context, id: &str) {
        let Some(e) = self.board.element(id) else { return };
        let size = e.fo("fontSize", 20.0);
        let mono = e.get("fontFamily").as_i64() == Some(3);
        let text = e.raw_text().to_string();
        if let Some(c) = e.container().map(str::to_string) {
            let Some(ce) = self.board.element(&c) else { return };
            let pad = if ce.kind() == "ellipse" || ce.kind() == "diamond" { ce.w().abs() * 0.15 + 5.0 } else { 5.0 };
            let w = (ce.w().abs() - 2.0 * pad).max(size);
            let (_, h) = self.measure(ctx, &text, size, w, mono);
            let grow = h + 2.0 * pad;
            if let Some(t) = self.board.element_mut(id) {
                t.set_f("width", w);
                t.set_f("height", h);
            }
            if let Some(ce) = self.board.element_mut(&c) {
                if ce.h().abs() < grow {
                    ce.set_f("height", grow);
                }
            }
            self.center_bound_text(&c);
            return;
        }
        let (w, h) = self.measure(ctx, &text, size, f32::INFINITY, mono);
        if let Some(t) = self.board.element_mut(id) {
            t.set_f("width", w);
            t.set_f("height", h);
        }
    }

    /// The text inside a shape goes to its middle.
    pub(super) fn center_bound_text(&mut self, container: &str) {
        let Some(ce) = self.board.element(container) else { return };
        let (cx, cy) = ce.center();
        let (x0, y0, x1, _) = ce.bounds();
        let a = ce.angle();
        let pad = 5.0;
        let ids: Vec<String> = ce.bound().into_iter().filter(|(_, k)| k == "text").map(|(i, _)| i).collect();
        for id in ids {
            if let Some(t) = self.board.element_mut(&id) {
                let (w, h) = (t.w(), t.h());
                let x = match t.s("textAlign") {
                    "left" => x0 + pad,
                    "right" => x1 - pad - w,
                    _ => cx - w / 2.0,
                };
                let y = if t.s("verticalAlign") == "top" { y0 + pad } else { cy - h / 2.0 };
                t.set_f("x", x);
                t.set_f("y", y);
                t.set_f("angle", a);
            }
        }
    }

    // ── cards and columns ─────────────────────────────────────────────

    pub(super) fn lane_full(&self, li: usize) -> bool {
        let l = &self.board.lanes[li];
        l.max > 0 && l.cards.len() >= l.max as usize
    }

    /// Where a carried card would go: (column, index), from the point.
    pub(super) fn drop_target(&self, at: P) -> Option<(usize, usize)> {
        for (i, lb) in self.scene.lanes.iter().enumerate().rev() {
            let r = draw::rr(lb.rect).expand(6.0);
            if !r.contains(pos2(at.0, at.1)) || self.board.lanes[i].collapsed {
                continue;
            }
            let idx = lb.cards.iter().position(|c| at.1 < c.rect.1 + c.rect.3 / 2.0).unwrap_or(lb.cards.len());
            return Some((i, idx));
        }
        None
    }

    /// Take a card out of wherever it is.
    pub(super) fn take_card(&mut self, id: &str) -> Option<(Card, Option<(usize, usize)>)> {
        if let Some(i) = self.board.cards.iter().position(|c| c.id == id) {
            return Some((self.board.cards.remove(i), None));
        }
        for (li, l) in self.board.lanes.iter_mut().enumerate() {
            if let Some(i) = l.cards.iter().position(|c| c.id == id) {
                return Some((l.cards.remove(i), Some((li, i))));
            }
        }
        None
    }

    /// Drop a carried card: into a column at the point, else loose there.
    pub(super) fn drop_card(&mut self, id: &str, at: P, top_left: P) -> Option<String> {
        let target = self.drop_target(at);
        if let Some((li, _)) = target {
            let already = self.board.lanes[li].cards.iter().any(|c| c.id == id);
            if !already && self.lane_full(li) {
                return Some(format!("A coluna “{}” está cheia (limite {}).", self.board.lanes[li].title, self.board.lanes[li].max));
            }
        }
        let (mut card, from) = self.take_card(id)?;
        let was_complete = from.is_some_and(|(li, _)| self.board.lanes[li].complete.is_some());
        match target {
            Some((li, mut idx)) => {
                if let Some((fl, fi)) = from {
                    if fl == li && fi < idx {
                        idx -= 1;
                    }
                }
                let lane = &mut self.board.lanes[li];
                // a done column marks its cards done, as the plugin does
                if lane.complete.is_some() {
                    card.check = 'x';
                } else if was_complete {
                    card.check = ' ';
                }
                lane.cards.insert(idx.min(lane.cards.len()), card);
            }
            None => {
                card.x = top_left.0;
                card.y = top_left.1;
                if card.w <= 0.0 {
                    card.w = CARD_W;
                }
                if was_complete {
                    card.check = ' ';
                }
                self.board.cards.push(card);
            }
        }
        self.changed();
        None
    }

    pub(super) fn add_card_to_lane(&mut self, li: usize, text: &str) -> Option<String> {
        if self.lane_full(li) {
            return None;
        }
        self.checkpoint();
        let mut c = Card::new(text);
        if self.board.lanes[li].complete.is_some() {
            c.check = 'x';
        }
        let id = c.id.clone();
        self.board.lanes[li].cards.push(c);
        self.changed();
        Some(id)
    }

    pub(super) fn add_loose_card(&mut self, at: P, text: &str) -> String {
        self.checkpoint();
        let mut c = Card::new(text);
        c.x = at.0;
        c.y = at.1;
        let id = c.id.clone();
        self.board.cards.push(c);
        self.changed();
        id
    }

    pub(super) fn add_lane(&mut self, at: Option<P>, title: &str) -> String {
        self.checkpoint();
        let mut l = Lane::new(title);
        match at {
            Some(p) => {
                l.x = p.0;
                l.y = p.1;
            }
            None => {
                let right = self.board.lanes.iter().map(|l| l.x + l.w).fold(f32::MIN, f32::max);
                l.x = if right > f32::MIN { right + LANE_GAP } else { 0.0 };
                l.y = self.board.lanes.first().map(|l| l.y).unwrap_or(0.0);
                l.w = LANE_W;
            }
        }
        let id = l.id.clone();
        self.board.lanes.push(l);
        self.board.sort_lanes();
        self.changed();
        id
    }

    /// Swap a column with its neighbour (place and order).
    pub(super) fn shift_lane(&mut self, li: usize, right: bool) {
        let lj = if right { li + 1 } else { li.wrapping_sub(1) };
        if lj >= self.board.lanes.len() {
            return;
        }
        self.checkpoint();
        let (ax, ay) = (self.board.lanes[li].x, self.board.lanes[li].y);
        let (bx, by) = (self.board.lanes[lj].x, self.board.lanes[lj].y);
        self.board.lanes[li].x = bx;
        self.board.lanes[li].y = by;
        self.board.lanes[lj].x = ax;
        self.board.lanes[lj].y = ay;
        self.board.lanes.swap(li, lj);
        self.changed();
    }

    // ── pictures and notes of the vault ───────────────────────────────

    fn folder(&self) -> String {
        crate::vault::parent(&self.path).to_string()
    }

    /// A picture's file id for an image element (the vault path, or the
    /// picture itself in a plain `.excalidraw`).
    fn file_for(&mut self, cx: &Cx, path: &str) -> Option<String> {
        if let Some((id, _)) = self.board.files.iter().find(|(_, f)| f.vault_path().as_deref() == Some(path)) {
            return Some(id.clone());
        }
        let id: String = (0..40).map(|i| format!("{:x}", (element::random_u64() >> (i % 16)) & 0xf)).collect();
        let r = if self.board.format == Format::Excalidraw {
            let bytes = std::fs::read(cx.vault.abs(path)).ok()?;
            let ext = path.rsplit('.').next().unwrap_or("png").to_lowercase();
            let mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "svg" => "image/svg+xml",
                _ => "image/png",
            };
            FileRef::Data(json!({ "mimeType": mime, "id": id, "dataURL": ateliet_core::quadro::data_url(mime, &bytes), "created": element::now_ms() }))
        } else {
            FileRef::Vault(path.to_string())
        };
        self.board.files.push((id.clone(), r));
        Some(id)
    }

    /// A note or picture of the vault onto the canvas at `at`.
    pub(super) fn insert_file(&mut self, cx: &mut Cx, ctx: &egui::Context, path: &str, at: P) {
        let lower = path.to_lowercase();
        let picture = ateliet_core::quadro::card::is_picture(&lower);
        let link = if lower.ends_with(".md") { path[..path.len() - 3].to_string() } else { path.to_string() };
        if self.is_kanban() {
            let text = if picture { format!("![[{link}]]") } else { format!("[[{link}]]") };
            let id = self.add_loose_card(at, &text);
            self.sel = vec![Sel::Card(id)];
            return;
        }
        if picture {
            let size = {
                let mut pics = super::VaultPics { vault: cx.vault, caches: cx.caches, source: &self.path, files: &[], data: &mut self.data_tex };
                use super::Pics;
                pics.tex(ctx, path).map(|(_, s)| s).unwrap_or(egui::vec2(300.0, 200.0))
            };
            let k = (400.0 / size.x.max(size.y)).min(1.0);
            let (w, h) = (size.x * k, size.y * k);
            self.checkpoint();
            let Some(fid) = self.file_for(cx, path) else { return };
            let mut e = Element::new("image", at.0 - w / 2.0, at.1 - h / 2.0, w, h, &self.style);
            e.set("fileId", json!(fid));
            e.set("strokeColor", json!("transparent"));
            let id = e.id().to_string();
            self.board.elements.push(e);
            self.sel = vec![Sel::El(id)];
        } else {
            self.checkpoint();
            let name = link.rsplit('/').next().unwrap_or(&link).to_string();
            let mut e = Element::new("text", at.0, at.1, 10.0, 10.0, &self.style);
            e.set("text", json!(name));
            e.set("originalText", json!(format!("[[{link}]]")));
            e.set("rawText", json!(format!("[[{link}]]")));
            e.set("link", json!(format!("[[{link}]]")));
            let id = e.id().to_string();
            self.board.elements.push(e);
            let size = self.board.element(&id).map(|e| e.fo("fontSize", 20.0)).unwrap_or(20.0);
            let (w, h) = self.measure(ctx, &name, size, f32::INFINITY, false);
            if let Some(e) = self.board.element_mut(&id) {
                e.set_f("width", w);
                e.set_f("height", h);
            }
            self.sel = vec![Sel::El(id)];
        }
        self.changed();
    }

    /// A picture from the clipboard: saved beside the quadro, then placed.
    pub(super) fn paste_picture(&mut self, cx: &mut Cx, ctx: &egui::Context, png: Vec<u8>, at: P) {
        let folder = self.folder();
        let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        let base = if folder.is_empty() { format!("Pasted image {stamp}") } else { format!("{folder}/Pasted image {stamp}") };
        let rel = cx.vault.available_path(&base, "png");
        if std::fs::write(cx.vault.abs(&rel), png).is_err() {
            return;
        }
        self.out.push(Out::Rescan);
        self.insert_file(cx, ctx, &rel, at);
    }

    // ── clipboard ─────────────────────────────────────────────────────

    /// The selection as Excalidraw's clipboard JSON (cards and columns ride
    /// along under `quadro`).
    pub(super) fn copy_json(&self) -> Option<String> {
        if self.sel.is_empty() {
            return None;
        }
        let ids: HashSet<String> = self.moving_els().into_iter().collect();
        let els: Vec<Value> = self.board.elements.iter().filter(|e| ids.contains(e.id()) && !e.deleted()).map(|e| e.to_value()).collect();
        let mut files = Map::new();
        let mut vault_files = Map::new();
        for e in self.board.elements.iter().filter(|e| ids.contains(e.id())) {
            if let Some(fid) = e.get("fileId").as_str() {
                match self.board.file(fid) {
                    Some(FileRef::Data(v)) => {
                        files.insert(fid.to_string(), v.clone());
                    }
                    Some(f) => {
                        if let Some(p) = f.vault_path() {
                            vault_files.insert(fid.to_string(), json!(p));
                        }
                    }
                    None => {}
                }
            }
        }
        let card_json = |c: &Card, x: f32, y: f32, w: f32| json!({ "id": c.id, "text": c.text, "check": c.check.to_string(), "x": x, "y": y, "w": w, "color": c.color });
        let mut cards = Vec::new();
        let mut lanes = Vec::new();
        for s in &self.sel {
            match s {
                Sel::Card(id) => {
                    if let (Some(c), Some(cb)) = (self.board.card(id), self.scene.card(id)) {
                        cards.push(card_json(c, cb.rect.0, cb.rect.1, cb.rect.2));
                    }
                }
                Sel::Lane(id) => {
                    if let Some(l) = self.board.lanes.iter().find(|l| &l.id == id) {
                        lanes.push(json!({ "title": l.title, "max": l.max, "complete": l.complete, "x": l.x, "y": l.y, "w": l.w, "cards": l.cards.iter().map(|c| card_json(c, 0.0, 0.0, 0.0)).collect::<Vec<_>>() }));
                    }
                }
                Sel::El(_) => {}
            }
        }
        Some(serde_json::to_string(&json!({ "type": "excalidraw/clipboard", "elements": els, "files": files, "quadro": { "cards": cards, "lanes": lanes, "files": vault_files } })).unwrap_or_default())
    }

    /// Paste text: our (or Excalidraw's) clipboard JSON, else a card or a
    /// text element with it.
    pub(super) fn paste_text(&mut self, cx: &mut Cx, ctx: &egui::Context, text: &str, at: P) {
        let v: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
        if v["type"].as_str() == Some("excalidraw/clipboard") || v["type"].as_str() == Some("excalidraw") {
            self.paste_scene(cx, &v, at);
            return;
        }
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.is_kanban() {
            let id = self.add_loose_card(at, text);
            self.sel = vec![Sel::Card(id)];
        } else {
            self.checkpoint();
            let mut e = Element::new("text", at.0, at.1, 10.0, 10.0, &self.style);
            e.set_text(text);
            let id = e.id().to_string();
            self.board.elements.push(e);
            self.fit_text(ctx, &id);
            self.sel = vec![Sel::El(id)];
            self.changed();
        }
    }

    fn paste_scene(&mut self, cx: &mut Cx, v: &Value, at: P) {
        self.checkpoint();
        let els: Vec<Element> = v["elements"].as_array().into_iter().flatten().filter_map(|e| Element::from_value(e.clone())).filter(|e| !e.deleted()).collect();
        let q = &v["quadro"];
        // where it all was, so it lands centred on the pointer
        let mut b: Option<(f32, f32, f32, f32)> = None;
        let mut add = |r: (f32, f32, f32, f32)| b = Some(b.map_or(r, |o| (o.0.min(r.0), o.1.min(r.1), o.2.max(r.2), o.3.max(r.3))));
        for e in &els {
            add(e.aabb());
        }
        for c in q["cards"].as_array().into_iter().flatten() {
            let (x, y) = (c["x"].as_f64().unwrap_or(0.0) as f32, c["y"].as_f64().unwrap_or(0.0) as f32);
            add((x, y, x + c["w"].as_f64().unwrap_or(240.0) as f32, y + 60.0));
        }
        for l in q["lanes"].as_array().into_iter().flatten() {
            let (x, y) = (l["x"].as_f64().unwrap_or(0.0) as f32, l["y"].as_f64().unwrap_or(0.0) as f32);
            add((x, y, x + l["w"].as_f64().unwrap_or(272.0) as f32, y + 200.0));
        }
        let Some(bx) = b else {
            self.uncheckpoint();
            return;
        };
        let (dx, dy) = (at.0 - (bx.0 + bx.2) / 2.0, at.1 - (bx.1 + bx.3) / 2.0);
        // fresh ids, and every reference to them
        let mut ids: HashMap<String, String> = HashMap::new();
        for e in &els {
            ids.insert(e.id().to_string(), element::new_id());
        }
        let mut cards_new: Vec<Card> = Vec::new();
        for c in q["cards"].as_array().into_iter().flatten() {
            let mut card = Card::new(c["text"].as_str().unwrap_or(""));
            if let Some(old) = c["id"].as_str() {
                ids.insert(old.to_string(), card.id.clone());
            }
            card.check = c["check"].as_str().and_then(|s| s.chars().next()).unwrap_or(' ');
            card.x = c["x"].as_f64().unwrap_or(0.0) as f32 + dx;
            card.y = c["y"].as_f64().unwrap_or(0.0) as f32 + dy;
            card.w = c["w"].as_f64().unwrap_or(CARD_W as f64) as f32;
            card.color = c["color"].as_str().map(str::to_string);
            cards_new.push(card);
        }
        let mut groups: HashMap<String, String> = HashMap::new();
        let mut new_sel = Vec::new();
        let vault_files = q["files"].as_object().cloned().unwrap_or_default();
        for mut e in els {
            let old = e.id().to_string();
            e.set("id", json!(ids[&old]));
            e.translate(dx, dy);
            let gs: Vec<String> = e.group_ids().into_iter().map(|g| groups.entry(g).or_insert_with(element::new_id).clone()).collect();
            e.set("groupIds", json!(gs));
            if let Some(c) = e.container().map(str::to_string) {
                e.set("containerId", ids.get(&c).map(|n| json!(n)).unwrap_or(Value::Null));
            }
            for key in ["startBinding", "endBinding"] {
                if let Some(t) = e.get(key).get("elementId").and_then(|x| x.as_str()).map(str::to_string) {
                    match ids.get(&t) {
                        Some(n) => {
                            let mut bnd = e.get(key).clone();
                            bnd["elementId"] = json!(n);
                            e.set(key, bnd);
                        }
                        None => e.set(key, Value::Null),
                    }
                }
            }
            let bound: Vec<Value> = e.bound().into_iter().filter_map(|(i, k)| ids.get(&i).map(|n| json!({ "id": n, "type": k }))).collect();
            e.set("boundElements", if bound.is_empty() { Value::Null } else { json!(bound) });
            e.v.remove("index");
            e.set("seed", json!((element::random_u64() % 2_000_000_000) as i64));
            // the picture comes along, in this file's way of keeping it
            if let Some(fid) = e.get("fileId").as_str().map(str::to_string) {
                if self.board.file(&fid).is_none() {
                    let path = vault_files.get(&fid).and_then(|p| p.as_str()).map(str::to_string);
                    let data = v["files"].get(&fid).cloned();
                    match (path, data) {
                        (Some(p), _) if self.board.format == Format::Excalidraw => {
                            if let Some(n) = self.file_for(cx, &p) {
                                e.set("fileId", json!(n));
                            }
                        }
                        (Some(p), _) => self.board.files.push((fid.clone(), FileRef::Vault(p))),
                        (None, Some(d)) if self.board.format == Format::Excalidraw => self.board.files.push((fid.clone(), FileRef::Data(d))),
                        (None, Some(d)) => {
                            // a picture inside the clipboard: saved beside the quadro
                            if let Some(img) = d["dataURL"].as_str().and_then(super::decode_data_url) {
                                let png = ateliet_core::board::tile::encode_png(img.as_raw(), img.width(), img.height());
                                let folder = self.folder();
                                let base = if folder.is_empty() { format!("Pasted image {fid}") } else { format!("{folder}/Pasted image {}", &fid[..fid.len().min(8)]) };
                                let rel = cx.vault.available_path(&base, "png");
                                if std::fs::write(cx.vault.abs(&rel), png).is_ok() {
                                    self.board.files.push((fid.clone(), FileRef::Vault(rel)));
                                    self.out.push(Out::Rescan);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            if e.container().is_none() {
                new_sel.push(Sel::El(e.id().to_string()));
            }
            self.board.elements.push(e);
        }
        if self.is_kanban() {
            for c in cards_new {
                new_sel.push(Sel::Card(c.id.clone()));
                self.board.cards.push(c);
            }
            for l in q["lanes"].as_array().into_iter().flatten() {
                let mut lane = Lane::new(l["title"].as_str().unwrap_or("Coluna"));
                lane.max = l["max"].as_u64().unwrap_or(0) as u32;
                lane.complete = l["complete"].as_str().map(str::to_string);
                lane.x = l["x"].as_f64().unwrap_or(0.0) as f32 + dx;
                lane.y = l["y"].as_f64().unwrap_or(0.0) as f32 + dy;
                lane.w = l["w"].as_f64().unwrap_or(LANE_W as f64) as f32;
                for c in l["cards"].as_array().into_iter().flatten() {
                    let mut card = Card::new(c["text"].as_str().unwrap_or(""));
                    card.check = c["check"].as_str().and_then(|s| s.chars().next()).unwrap_or(' ');
                    lane.cards.push(card);
                }
                new_sel.push(Sel::Lane(lane.id.clone()));
                self.board.lanes.push(lane);
            }
            self.board.sort_lanes();
        } else {
            // a drawing has no cards: their text comes as text
            for c in cards_new {
                let mut e = Element::new("text", c.x, c.y, c.w, 24.0, &self.style);
                e.set_text(&c.text);
                new_sel.push(Sel::El(e.id().to_string()));
                self.board.elements.push(e);
            }
        }
        self.sel = new_sel;
        self.changed();
    }

    /// The selection copied beside itself.
    pub(super) fn duplicate(&mut self, cx: &mut Cx) {
        if let Some(j) = self.copy_json() {
            let v: Value = serde_json::from_str(&j).unwrap_or(Value::Null);
            if let Some(b) = self.selection_box() {
                self.paste_scene(cx, &v, ((b.0 + b.2) / 2.0 + 24.0, (b.1 + b.3) / 2.0 + 24.0));
            }
        }
    }
}

/// Where the line from `aim` to the centre `c` meets the outline (from
/// inside: where the ray from the centre through `aim` leaves it).
fn edge_point(poly: &[P], aim: P, c: P) -> Option<P> {
    let n = poly.len();
    let cross = |a: P, b: P, p: P, q: P| -> Option<(f32, P)> {
        let r = (b.0 - a.0, b.1 - a.1);
        let s = (q.0 - p.0, q.1 - p.1);
        let den = r.0 * s.1 - r.1 * s.0;
        if den.abs() < 1e-9 {
            return None;
        }
        let t = ((p.0 - a.0) * s.1 - (p.1 - a.1) * s.0) / den;
        let u = ((p.0 - a.0) * r.1 - (p.1 - a.1) * r.0) / den;
        ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| (t, (a.0 + t * r.0, a.1 + t * r.1)))
    };
    let inside = element::inside(poly, aim);
    let (a, b) = if inside {
        let d = dist(aim, c).max(0.001);
        (c, (c.0 + (aim.0 - c.0) / d * 1e5, c.1 + (aim.1 - c.1) / d * 1e5))
    } else {
        (aim, c)
    };
    (0..n).filter_map(|i| cross(a, b, poly[i], poly[(i + 1) % n])).min_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal)).map(|(_, p)| p)
}
