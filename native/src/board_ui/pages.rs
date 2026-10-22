//! The page kinds in the storyboard editor: the panel tool (drag across a
//! panel to cut it with the gutter, drag in empty space for a new one,
//! corners and moves for the selected one), the balloon tool (drag or click
//! to make one, type in place, aim the tail), the page drawn over the
//! layers, the page settings and the page exports.

use super::*;
use ateliet_core::board::pages::{self as pg, Pt, R};
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum PMode {
    Line,
    Move,
    Vertex(usize),
    BNew,
    BMove,
    BTail,
    BCorner(u8),
}

pub(super) struct PDrag {
    mode: PMode,
    a: Pt,
    b: Pt,
    alt: bool,
    shift: bool,
    orig: Value,
    before: Value,
}

pub(super) struct PageState {
    pub sel_panel: Option<String>,
    pub sel_balloon: Option<String>,
    drag: Option<PDrag>,
    pub bkind: String,
    pub bvertical: bool,
    pub bsize: f32,
    /// The balloon being typed in, and the board before.
    editing: Option<(String, Value)>,
    pub numbers: bool,
    deco: Option<(egui::TextureHandle, String)>,
    job: Option<(String, Receiver<egui::ColorImage>)>,
    last_down: Option<(Instant, String)>,
    layout: String,
}

impl PageState {
    pub fn new(kind: &str) -> PageState {
        PageState { sel_panel: None, sel_balloon: None, drag: None, bkind: "speech".into(), bvertical: kind == "name", bsize: 28.0, editing: None, numbers: true, deco: None, job: None, last_down: None, layout: "one".into() }
    }
}

fn num(v: &Value, d: f32) -> f32 {
    v.as_f64().map(|x| x as f32).unwrap_or(d)
}

impl BoardEditor {
    pub(super) fn is_pages(&self) -> bool {
        pg::is_page_kind(self.doc.kind()) && self.doc.board["page"].is_object()
    }

    fn panels(&self) -> Vec<Value> {
        self.doc.frame(self.frame)["panels"].as_array().cloned().unwrap_or_default()
    }

    fn balloons(&self) -> Vec<Value> {
        self.doc.frame(self.frame)["balloons"].as_array().cloned().unwrap_or_default()
    }

    fn panels_mut(&mut self) -> &mut Vec<Value> {
        let f = &mut self.doc.board["frames"][self.frame];
        if !f["panels"].is_array() {
            f["panels"] = json!([]);
        }
        f["panels"].as_array_mut().unwrap()
    }

    fn balloons_mut(&mut self) -> &mut Vec<Value> {
        let f = &mut self.doc.board["frames"][self.frame];
        if !f["balloons"].is_array() {
            f["balloons"] = json!([]);
        }
        f["balloons"].as_array_mut().unwrap()
    }

    /// One undo step from `before` to the board as it is now.
    pub(super) fn commit_board(&mut self, before: Value) {
        if before == self.doc.board {
            return;
        }
        let after = self.doc.board.clone();
        self.undo.push(Edit::Structure { before, after, tiles: Vec::new() });
        self.redo.clear();
        self.doc.mark_frame(self.frame);
        self.changed();
    }

    fn panel_at(&self, c: Pt) -> Option<String> {
        self.panels().iter().rev().find(|p| pg::inside(&pg::points(p), c)).map(|p| p["id"].as_str().unwrap_or("").to_string())
    }

    fn balloon_at(&self, c: Pt) -> Option<String> {
        self.balloons().iter().rev().find(|b| {
            let r = pg::rect_of(b);
            c.0 >= r.x && c.0 <= r.x + r.w && c.1 >= r.y && c.1 <= r.y + r.h
        }).map(|b| b["id"].as_str().unwrap_or("").to_string())
    }

    /// Corners stick to the inner frame, the page and other panels' edges (± gutter).
    fn snap(&self, p: Pt) -> Pt {
        let tol = 10.0 / self.zoom;
        let page = &self.doc.board["page"];
        let inner = pg::inner_rect(&self.doc.board);
        let (gx, gy) = (num(&page["gutterX"], 12.0), num(&page["gutterY"], 24.0));
        let mut xs = vec![inner.x, inner.x + inner.w, 0.0, self.doc.width() as f32];
        let mut ys = vec![inner.y, inner.y + inner.h, 0.0, self.doc.height() as f32];
        for pn in self.panels() {
            if pn["id"].as_str() == self.pg.sel_panel.as_deref() {
                continue;
            }
            let r = pg::bounds(&pg::points(&pn));
            xs.extend([r.x - gx, r.x + r.w + gx, r.x, r.x + r.w]);
            ys.extend([r.y - gy, r.y + r.h + gy, r.y, r.y + r.h]);
        }
        let near = |v: f32, l: &[f32]| l.iter().copied().fold(f32::INFINITY, |b, c| if (c - v).abs() < (b - v).abs() { c } else { b });
        let (nx, ny) = (near(p.0, &xs), near(p.1, &ys));
        (if (nx - p.0).abs() < tol { nx } else { p.0 }, if (ny - p.1).abs() < tol { ny } else { p.1 })
    }

    fn straight(a: Pt, b: Pt) -> Pt {
        if (b.0 - a.0).abs() > (b.1 - a.1).abs() {
            (b.0, a.1)
        } else {
            (a.0, b.1)
        }
    }

    fn sdist(&self, a: Pt, b: Pt) -> f32 {
        ((a.0 - b.0) * self.zoom).hypot((a.1 - b.1) * self.zoom)
    }

    // ── pointer ─────────────────────────────────────────────────────

    pub(super) fn page_input(&mut self, ui: &egui::Ui, resp: &egui::Response, origin: egui::Pos2) {
        let to_c = |q: egui::Pos2, z: f32| ((q.x - origin.x) / z, (q.y - origin.y) / z);
        let (pressed, released, alt, shift, pos) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.modifiers.alt, i.modifiers.shift, i.pointer.interact_pos()));
        if pressed && resp.hovered() {
            if let Some(q) = pos {
                let c = to_c(q, self.zoom);
                if self.tool == Tool::Panel {
                    self.panel_down(c, alt, shift);
                } else {
                    self.balloon_down(c);
                }
            }
        }
        if let (Some(q), true) = (pos, self.pg.drag.is_some()) {
            let c = to_c(q, self.zoom);
            if let Some(d) = self.pg.drag.as_mut() {
                d.b = c;
                d.shift = shift;
            }
            self.drag_move(c);
            ui.ctx().request_repaint();
        }
        if released && self.pg.drag.is_some() {
            self.drag_up();
        }
    }

    fn panel_down(&mut self, c: Pt, alt: bool, shift: bool) {
        let before = self.doc.board.clone();
        if let Some(sel) = self.pg.sel_panel.clone().and_then(|id| self.panels().into_iter().find(|p| p["id"].as_str() == Some(id.as_str()))) {
            if let Some(v) = pg::points(&sel).iter().position(|q| self.sdist(*q, c) < 10.0) {
                self.pg.drag = Some(PDrag { mode: PMode::Vertex(v), a: c, b: c, alt, shift, orig: sel, before });
                return;
            }
        }
        let hit = if alt { None } else { self.panel_at(c) };
        if let Some(id) = hit {
            if self.pg.last_down.as_ref().is_some_and(|(t, i)| *i == id && t.elapsed() < Duration::from_millis(350)) {
                self.zoom_to_panel(&id);
                self.pg.last_down = None;
                return;
            }
            self.pg.last_down = Some((Instant::now(), id.clone()));
            let was = self.pg.sel_panel.as_deref() == Some(id.as_str());
            self.pg.sel_panel = Some(id.clone());
            if was {
                let orig = self.panels().into_iter().find(|p| p["id"].as_str() == Some(id.as_str())).unwrap_or_default();
                self.pg.drag = Some(PDrag { mode: PMode::Move, a: c, b: c, alt, shift, orig, before });
            }
            return;
        }
        self.pg.drag = Some(PDrag { mode: PMode::Line, a: c, b: c, alt, shift, orig: Value::Null, before });
    }

    fn zoom_to_panel(&mut self, id: &str) {
        let Some(p) = self.panels().into_iter().find(|p| p["id"].as_str() == Some(id)) else { return };
        let r = pg::bounds(&pg::points(&p));
        self.zoom_target = Some((r.x + r.w / 2.0, r.y + r.h / 2.0, r.w, r.h));
    }

    fn balloon_down(&mut self, c: Pt) {
        self.finish_editing();
        let before = self.doc.board.clone();
        if let Some(sel) = self.pg.sel_balloon.clone().and_then(|id| self.balloons().into_iter().find(|b| b["id"].as_str() == Some(id.as_str()))) {
            let r = pg::rect_of(&sel);
            let t = &sel["tail"];
            if t.is_object() && self.sdist((num(&t["x"], 0.0), num(&t["y"], 0.0)), c) < 10.0 {
                self.pg.drag = Some(PDrag { mode: PMode::BTail, a: c, b: c, alt: false, shift: false, orig: sel, before });
                return;
            }
            for (k, q) in [(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)].into_iter().enumerate() {
                if self.sdist(q, c) < 10.0 {
                    self.pg.drag = Some(PDrag { mode: PMode::BCorner(k as u8), a: c, b: c, alt: false, shift: false, orig: sel, before });
                    return;
                }
            }
        }
        if let Some(id) = self.balloon_at(c) {
            if self.pg.last_down.as_ref().is_some_and(|(t, i)| *i == id && t.elapsed() < Duration::from_millis(350)) {
                self.pg.sel_balloon = Some(id.clone());
                self.pg.editing = Some((id, before));
                return;
            }
            self.pg.last_down = Some((Instant::now(), id.clone()));
            self.pg.sel_balloon = Some(id.clone());
            let orig = self.balloons().into_iter().find(|b| b["id"].as_str() == Some(id.as_str())).unwrap_or_default();
            self.pg.drag = Some(PDrag { mode: PMode::BMove, a: c, b: c, alt: false, shift: false, orig, before });
            return;
        }
        self.pg.sel_balloon = None;
        self.pg.drag = Some(PDrag { mode: PMode::BNew, a: c, b: c, alt: false, shift: false, orig: Value::Null, before });
    }

    fn drag_move(&mut self, c: Pt) {
        let Some(d) = self.pg.drag.as_ref() else { return };
        let (dx, dy) = (c.0 - d.a.0, c.1 - d.a.1);
        let (mode, orig) = (d.mode, d.orig.clone());
        let id = orig["id"].as_str().unwrap_or("").to_string();
        match mode {
            PMode::Move | PMode::Vertex(_) => {
                let pts = pg::points(&orig);
                let new: Vec<Pt> = match mode {
                    PMode::Move => pts.iter().map(|q| ((q.0 + dx).round(), (q.1 + dy).round())).collect(),
                    PMode::Vertex(v) => {
                        let moved = self.snap((pts[v].0 + dx, pts[v].1 + dy));
                        pts.iter().enumerate().map(|(i, q)| if i == v { moved } else { *q }).collect()
                    }
                    _ => pts,
                };
                if let Some(p) = self.panels_mut().iter_mut().find(|p| p["id"].as_str() == Some(id.as_str())) {
                    pg::set_points(p, &new);
                }
            }
            PMode::BMove | PMode::BTail | PMode::BCorner(_) => {
                let o = orig.clone();
                if let Some(bl) = self.balloons_mut().iter_mut().find(|b| b["id"].as_str() == Some(id.as_str())) {
                    let r = pg::rect_of(&o);
                    match mode {
                        PMode::BMove => {
                            bl["x"] = json!(r.x + dx);
                            bl["y"] = json!(r.y + dy);
                            if o["tail"].is_object() {
                                bl["tail"] = json!({ "x": num(&o["tail"]["x"], 0.0) + dx, "y": num(&o["tail"]["y"], 0.0) + dy });
                            }
                        }
                        PMode::BTail => bl["tail"] = json!({ "x": c.0, "y": c.1 }),
                        PMode::BCorner(k) => {
                            let (left, top) = (k == 0 || k == 3, k == 0 || k == 1);
                            let (x0, x1) = if left { (r.x + dx, r.x + r.w) } else { (r.x, r.x + r.w + dx) };
                            let (y0, y1) = if top { (r.y + dy, r.y + r.h) } else { (r.y, r.y + r.h + dy) };
                            bl["x"] = json!(x0.min(x1));
                            bl["y"] = json!(y0.min(y1));
                            bl["w"] = json!((x1 - x0).abs().max(30.0));
                            bl["h"] = json!((y1 - y0).abs().max(24.0));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn drag_up(&mut self) {
        let Some(d) = self.pg.drag.take() else { return };
        match d.mode {
            PMode::Move | PMode::Vertex(_) | PMode::BMove | PMode::BTail | PMode::BCorner(_) => self.commit_board(d.before),
            PMode::Line => {
                let a = d.a;
                if self.sdist(a, d.b) < 6.0 {
                    self.pg.sel_panel = None;
                    return;
                }
                let b = if d.shift { Self::straight(a, d.b) } else { d.b };
                let page = self.doc.board["page"].clone();
                let cuts: Vec<String> = if d.alt { Vec::new() } else { self.panels().iter().filter(|pn| { let pts = pg::points(pn); pg::line_cuts(&pts, a, b) && !pg::inside(&pts, a) && !pg::inside(&pts, b) }).map(|p| p["id"].as_str().unwrap_or("").to_string()).collect() };
                if !cuts.is_empty() {
                    // a mostly horizontal cut separates tiers, and the other way round
                    let horiz = (b.0 - a.0).abs() > (b.1 - a.1).abs();
                    let gap = if horiz { num(&page["gutterY"], 24.0) } else { num(&page["gutterX"], 12.0) };
                    let next: Vec<Value> = self.panels().into_iter().flat_map(|pn| if cuts.iter().any(|c| pn["id"].as_str() == Some(c.as_str())) { pg::cut_panel(&pn, a, b, gap) } else { vec![pn] }).collect();
                    *self.panels_mut() = next;
                    self.pg.sel_panel = None;
                    self.commit_board(d.before);
                    return;
                }
                let r = R { x: a.0.min(b.0), y: a.1.min(b.1), w: (b.0 - a.0).abs(), h: (b.1 - a.1).abs() };
                if r.w < 8.0 || r.h < 8.0 {
                    return;
                }
                let p0 = self.snap((r.x, r.y));
                let p1 = self.snap((r.x + r.w, r.y + r.h));
                let panel = pg::rect_panel(R { x: p0.0.round(), y: p0.1.round(), w: (p1.0 - p0.0).round(), h: (p1.1 - p0.1).round() });
                self.pg.sel_panel = panel["id"].as_str().map(String::from);
                self.panels_mut().push(panel);
                self.commit_board(d.before);
            }
            PMode::BNew => {
                let (a, b) = (d.a, d.b);
                let s = self.pg.bsize;
                let mut r = R { x: a.0.min(b.0), y: a.1.min(b.1), w: (b.0 - a.0).abs(), h: (b.1 - a.1).abs() };
                if self.sdist(a, b) < 8.0 {
                    // a click: room for a short line at the chosen size
                    let (w, h) = if self.pg.bvertical { (s * 4.5, s * 8.0) } else { (s * 9.0, s * 4.2) };
                    r = R { x: a.0 - w / 2.0, y: a.1 - h / 2.0, w, h };
                }
                let bl = pg::new_balloon(&self.pg.bkind, r, self.pg.bvertical, s);
                let id = bl["id"].as_str().unwrap_or("").to_string();
                self.balloons_mut().push(bl);
                self.commit_board(d.before);
                self.pg.sel_balloon = Some(id.clone());
                self.pg.editing = Some((id, self.doc.board.clone()));
            }
        }
    }

    pub(super) fn finish_editing(&mut self) {
        if let Some((_, before)) = self.pg.editing.take() {
            self.commit_board(before);
        }
    }

    pub(super) fn page_keys(&mut self, ui: &egui::Ui) {
        if self.pg.editing.is_some() {
            return;
        }
        let (del, esc, enter, star) = ui.input(|i| (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter), i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "*"))));
        if del {
            let before = self.doc.board.clone();
            if let Some(id) = self.pg.sel_balloon.take() {
                self.balloons_mut().retain(|b| b["id"].as_str() != Some(id.as_str()));
            } else if let Some(id) = self.pg.sel_panel.take() {
                self.panels_mut().retain(|b| b["id"].as_str() != Some(id.as_str()));
            }
            self.commit_board(before);
        }
        if esc {
            self.pg.sel_balloon = None;
            self.pg.sel_panel = None;
        }
        if enter {
            if let Some(id) = self.pg.sel_balloon.clone() {
                self.pg.editing = Some((id, self.doc.board.clone()));
            }
        }
        if star {
            if let Some(id) = self.pg.sel_panel.clone() {
                let before = self.doc.board.clone();
                if let Some(p) = self.panels_mut().iter_mut().find(|p| p["id"].as_str() == Some(id.as_str())) {
                    let s = p["star"].as_bool().unwrap_or(false);
                    p["star"] = json!(!s);
                }
                self.commit_board(before);
            }
        }
    }

    // ── drawing ─────────────────────────────────────────────────────

    /// The page over the layers (drawn in the background at the view's
    /// scale), the webtoon slices, the selection and what is being dragged.
    pub(super) fn page_draw(&mut self, ui: &egui::Ui, clip: &egui::Painter, origin: egui::Pos2) {
        let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
        let page = egui::Rect::from_min_size(origin, egui::vec2(w, h) * self.zoom);
        let ppp = ui.ctx().pixels_per_point();
        let scale = ((self.zoom * ppp).min(4096.0 / w.max(h)) * 8.0).round() / 8.0;
        let frame = self.doc.frame(self.frame).clone();
        let key = format!("{}|{}|{}|{}|{scale}|{}", self.doc.board["page"], frame["panels"], frame["balloons"], self.doc.board["background"], self.pg.numbers);
        if let Some((k, rx)) = &self.pg.job {
            if let Ok(img) = rx.try_recv() {
                let k = k.clone();
                self.pg.job = None;
                let tex = ui.ctx().load_texture("board-page", img, egui::TextureOptions::LINEAR);
                self.pg.deco = Some((tex, k));
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            }
        }
        if self.pg.job.is_none() && self.pg.deco.as_ref().map(|d| &d.1) != Some(&key) {
            let (tx, rx) = mpsc::channel();
            let board = json!({ "page": self.doc.board["page"], "width": w, "height": h, "background": self.doc.board["background"], "kind": self.doc.kind() });
            let numbers = self.pg.numbers;
            std::thread::spawn(move || {
                let db = crate::fonts::book_fonts();
                let items = pg::decorations(&board, &frame, &db, numbers);
                let (pw, ph, px) = ateliet_core::typeset::raster::render_layer(&items, w, h, &db, &[], scale);
                let _ = tx.send(egui::ColorImage::from_rgba_unmultiplied([pw as usize, ph as usize], &px));
            });
            self.pg.job = Some((key, rx));
        }
        if let Some((t, _)) = &self.pg.deco {
            clip.image(t.id(), page, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        let to = |p: Pt| egui::pos2(origin.x + p.0 * self.zoom, origin.y + p.1 * self.zoom);
        let blue = Color32::from_rgb(0x2f, 0x80, 0xed);
        let page_v = &self.doc.board["page"];
        // trim and inner frame
        let bleed = num(&page_v["bleed"], 0.0);
        if bleed > 0.0 {
            clip.rect_stroke(egui::Rect::from_min_max(to((bleed, bleed)), to((w - bleed, h - bleed))), 0.0, egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xe5, 0x48, 0x4d, 140)), egui::StrokeKind::Middle);
        }
        let inner = pg::inner_rect(&self.doc.board);
        let ir = egui::Rect::from_min_max(to((inner.x, inner.y)), to((inner.x + inner.w, inner.y + inner.h)));
        for (a, b) in [(ir.left_top(), ir.right_top()), (ir.right_top(), ir.right_bottom()), (ir.right_bottom(), ir.left_bottom()), (ir.left_bottom(), ir.left_top())] {
            clip.extend(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x2f, 0x80, 0xed, 90)), 5.0, 4.0));
        }
        if self.doc.kind() == "webtoon" {
            let slice = num(&page_v["slice"], 1280.0).max(100.0);
            let mut y = slice;
            while y < h {
                clip.extend(egui::Shape::dashed_line(&[to((0.0, y)), to((w, y))], egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xe5, 0x48, 0x4d, 160)), 6.0, 5.0));
                clip.text(to((w, y)) + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, format!("{}", (y / slice) as usize), egui::FontId::proportional(10.0), Color32::from_rgb(0xe5, 0x48, 0x4d));
                y += slice;
            }
        }
        // selection
        if let Some(sel) = self.pg.sel_panel.as_ref().and_then(|id| self.panels().into_iter().find(|p| p["id"].as_str() == Some(id.as_str()))) {
            let pts: Vec<egui::Pos2> = pg::points(&sel).into_iter().map(to).collect();
            clip.add(egui::Shape::closed_line(pts.clone(), egui::Stroke::new(2.0_f32, blue)));
            for q in pts {
                clip.rect(egui::Rect::from_center_size(q, egui::vec2(10.0, 10.0)), 1.0, Color32::WHITE, egui::Stroke::new(1.5_f32, blue), egui::StrokeKind::Middle);
            }
        }
        if let Some(sel) = self.pg.sel_balloon.as_ref().and_then(|id| self.balloons().into_iter().find(|b| b["id"].as_str() == Some(id.as_str()))) {
            let r = pg::rect_of(&sel);
            let rr = egui::Rect::from_min_max(to((r.x, r.y)), to((r.x + r.w, r.y + r.h)));
            for (a, b) in [(rr.left_top(), rr.right_top()), (rr.right_top(), rr.right_bottom()), (rr.right_bottom(), rr.left_bottom()), (rr.left_bottom(), rr.left_top())] {
                clip.extend(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(1.5_f32, blue), 5.0, 3.0));
            }
            for q in [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom()] {
                clip.rect(egui::Rect::from_center_size(q, egui::vec2(8.0, 8.0)), 1.0, Color32::WHITE, egui::Stroke::new(1.5_f32, blue), egui::StrokeKind::Middle);
            }
            if sel["tail"].is_object() {
                clip.circle_filled(to((num(&sel["tail"]["x"], 0.0), num(&sel["tail"]["y"], 0.0))), 6.0, blue);
            }
        }
        if let Some(d) = &self.pg.drag {
            match d.mode {
                PMode::Line => {
                    let b = if d.shift { Self::straight(d.a, d.b) } else { d.b };
                    let cuts = !d.alt && self.panels().iter().any(|pn| { let pts = pg::points(pn); pg::line_cuts(&pts, d.a, b) && !pg::inside(&pts, d.a) && !pg::inside(&pts, b) });
                    if cuts {
                        clip.extend(egui::Shape::dashed_line(&[to(d.a), to(b)], egui::Stroke::new(2.0_f32, Color32::from_rgb(0xe5, 0x53, 0x4b)), 8.0, 5.0));
                    } else {
                        let rr = egui::Rect::from_two_pos(to(d.a), to(b));
                        clip.rect_stroke(rr, 0.0, egui::Stroke::new(1.5_f32, blue), egui::StrokeKind::Middle);
                    }
                }
                PMode::BNew => {
                    clip.rect_stroke(egui::Rect::from_two_pos(to(d.a), to(d.b)), 0.0, egui::Stroke::new(1.0_f32, blue), egui::StrokeKind::Middle);
                }
                _ => {}
            }
        }
        // typing in a balloon: a field over it
        if let Some((id, _)) = self.pg.editing.clone() {
            let Some(bl) = self.balloons().into_iter().find(|b| b["id"].as_str() == Some(id.as_str())) else {
                self.pg.editing = None;
                return;
            };
            let r = pg::rect_of(&bl);
            let rr = egui::Rect::from_min_max(to((r.x, r.y)), to((r.x + r.w, r.y + r.h)));
            let size = egui::vec2(rr.width().max(180.0), rr.height().max(70.0));
            let mut text = bl["text"].as_str().unwrap_or("").to_string();
            let mut done = false;
            egui::Area::new(egui::Id::new(("balloon-edit", &id))).fixed_pos(rr.center() - size / 2.0).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                egui::Frame::new().fill(Color32::from_white_alpha(245)).stroke(egui::Stroke::new(1.0_f32, blue)).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                    let te = egui::TextEdit::multiline(&mut text).desired_width(size.x - 12.0).desired_rows(3).hint_text("Digite a fala…  (Esc: pronto)").text_color(Color32::from_gray(20)).frame(false);
                    let r = ui.add_sized(size - egui::vec2(12.0, 12.0), te);
                    if !r.has_focus() && !r.lost_focus() {
                        r.request_focus();
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape) || (i.modifiers.command && i.key_pressed(egui::Key::Enter))) || r.lost_focus() {
                        done = true;
                    }
                });
            });
            if text != bl["text"].as_str().unwrap_or("") {
                if let Some(b) = self.balloons_mut().iter_mut().find(|b| b["id"].as_str() == Some(id.as_str())) {
                    b["text"] = json!(text);
                }
            }
            if done {
                self.finish_editing();
            }
        }
    }

    // ── side panel ──────────────────────────────────────────────────

    pub(super) fn page_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let before = self.doc.board.clone();
        let w = ui.available_width();
        ui.set_max_width(w);
        ui.spacing_mut().slider_width = (w - 130.0).max(60.0);
        let info = pg::kind_info(self.doc.kind());
        ui.label(RichText::new(match info.id { "thumbnails" => "Folha", "webtoon" => "Tira", _ => "Página" }).font(kit::medium(13.0)).color(p.text));
        ui.add_space(4.0);
        // the balloon tool's options, and the selected balloon
        if self.tool == Tool::Balloon || self.pg.sel_balloon.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for (k, name) in pg::BALLOON_KINDS {
                    if kit::chip(ui, None, name, self.pg.bkind == k, p).clicked() {
                        self.pg.bkind = k.into();
                        if let Some(id) = self.pg.sel_balloon.clone() {
                            if let Some(b) = self.balloons_mut().iter_mut().find(|b| b["id"].as_str() == Some(id.as_str())) {
                                b["kind"] = json!(k);
                                if matches!(k, "narration" | "sfx") {
                                    b["tail"] = Value::Null;
                                } else if !b["tail"].is_object() {
                                    let r = pg::rect_of(b);
                                    b["tail"] = json!({ "x": r.x + r.w * 0.3, "y": r.y + r.h + (r.h * 0.4).max(30.0) });
                                }
                            }
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                if kit::toggle(ui, &mut self.pg.bvertical, p).changed() {
                    let v = self.pg.bvertical;
                    if let Some(id) = self.pg.sel_balloon.clone() {
                        if let Some(b) = self.balloons_mut().iter_mut().find(|b| b["id"].as_str() == Some(id.as_str())) {
                            b["vertical"] = json!(v);
                        }
                    }
                }
                ui.label(RichText::new("Texto vertical").size(12.0).color(p.muted));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tamanho").size(12.0).color(p.muted));
                if ui.add(egui::Slider::new(&mut self.pg.bsize, 10.0..=120.0).logarithmic(true)).changed() {
                    let s = self.pg.bsize;
                    if let Some(id) = self.pg.sel_balloon.clone() {
                        if let Some(b) = self.balloons_mut().iter_mut().find(|b| b["id"].as_str() == Some(id.as_str())) {
                            b["size"] = json!(s.round());
                        }
                    }
                }
            });
            if let Some(id) = self.pg.sel_balloon.clone() {
                let cur = self.balloons().into_iter().find(|b| b["id"].as_str() == Some(id.as_str())).map(|b| b["text"].as_str().unwrap_or("").to_string());
                if let Some(mut t) = cur {
                    if ui.add(egui::TextEdit::multiline(&mut t).desired_rows(2).desired_width(f32::INFINITY).hint_text("Fala")).changed() {
                        if let Some(b) = self.balloons_mut().iter_mut().find(|b| b["id"].as_str() == Some(id.as_str())) {
                            b["text"] = json!(t);
                        }
                    }
                    if kit::quiet(ui, Some(icons::TRASH_2), "Excluir balão", p).clicked() {
                        self.balloons_mut().retain(|b| b["id"].as_str() != Some(id.as_str()));
                        self.pg.sel_balloon = None;
                    }
                }
            }
            ui.add_space(6.0);
            kit::hairline(ui, p);
            ui.add_space(6.0);
        }
        if let Some(id) = self.pg.sel_panel.clone() {
            ui.horizontal(|ui| {
                if self.doc.kind() == "thumbnails" {
                    let star = self.panels().iter().find(|x| x["id"].as_str() == Some(id.as_str())).and_then(|x| x["star"].as_bool()).unwrap_or(false);
                    if kit::button(ui, Some(icons::STAR), if star { "Tirar a estrela" } else { "Estrela (*)" }, p).clicked() {
                        if let Some(x) = self.panels_mut().iter_mut().find(|x| x["id"].as_str() == Some(id.as_str())) {
                            x["star"] = json!(!star);
                        }
                    }
                }
                if kit::quiet(ui, Some(icons::TRASH_2), "Excluir quadro", p).clicked() {
                    self.panels_mut().retain(|x| x["id"].as_str() != Some(id.as_str()));
                    self.pg.sel_panel = None;
                }
            });
            ui.add_space(6.0);
        }
        // layouts for this page
        if self.doc.kind() != "webtoon" {
            ui.horizontal(|ui| {
                let cur = pg::LAYOUTS.iter().find(|l| l.0 == self.pg.layout).map(|l| l.1).unwrap_or("Um quadro");
                egui::ComboBox::from_id_salt("page-layout").width(ui.available_width() - 76.0).selected_text(RichText::new(cur).size(12.0)).show_ui(ui, |ui| {
                    for (id, name, _) in pg::LAYOUTS {
                        if ui.selectable_label(self.pg.layout == id, name).clicked() {
                            self.pg.layout = id.into();
                        }
                    }
                });
                if kit::button(ui, None, "Aplicar", p).on_hover_text("Refaz os quadros desta página").clicked() {
                    let panels = pg::start_panels(&self.doc.board, &self.pg.layout);
                    *self.panels_mut() = panels;
                    self.pg.sel_panel = None;
                }
            });
        }
        let page = &mut self.doc.board["page"];
        let mut row = |ui: &mut egui::Ui, label: &str, key: &str, max: f32| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(12.0).color(p.muted));
                let mut v = num(&page[key], 0.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::DragValue::new(&mut v).range(0.0..=max).speed(1.0).suffix(" px")).changed() {
                        page[key] = json!(v.round());
                    }
                });
            });
        };
        row(ui, "Medianiz lado a lado", "gutterX", 400.0);
        row(ui, "Medianiz entre tiras", "gutterY", 800.0);
        row(ui, "Borda dos quadros", "border", 40.0);
        let mut flag = |ui: &mut egui::Ui, label: &str, key: &str| {
            ui.horizontal(|ui| {
                let mut on = page[key].as_bool().unwrap_or(false);
                if kit::toggle(ui, &mut on, p).changed() {
                    page[key] = json!(on);
                }
                ui.label(RichText::new(label).size(12.0).color(p.muted));
            });
        };
        flag(ui, "Papel fora dos quadros", "mask");
        flag(ui, "Da direita para a esquerda", "rtl");
        ui.horizontal(|ui| {
            kit::toggle(ui, &mut self.pg.numbers, p);
            ui.label(RichText::new("Ordem de leitura").size(12.0).color(p.muted));
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Roteiro desta página").size(12.0).color(p.muted));
        let mut script = self.doc.frame(self.frame)["script"].as_str().unwrap_or("").to_string();
        if ui.add(egui::TextEdit::multiline(&mut script).desired_rows(4).desired_width(f32::INFINITY).hint_text("O trecho do roteiro que a página cobre")).changed() {
            self.doc.board["frames"][self.frame]["script"] = json!(script);
            self.dirty = true;
            self.last_change = Some(Instant::now());
        }
        ui.add_space(8.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
        if self.doc.board != before && self.pg.drag.is_none() && self.pg.editing.is_none() {
            // script edits are saved, not undone
            let mut b2 = before.clone();
            b2["frames"][self.frame]["script"] = self.doc.board["frames"][self.frame]["script"].clone();
            if b2 != self.doc.board {
                self.commit_board(before);
            }
        }
    }

    // ── export ──────────────────────────────────────────────────────

    pub(super) fn export_page_png(&mut self) {
        let db = crate::fonts::book_fonts();
        let (w, h, px) = pg::page_image(&mut self.doc, &mut self.pics, &db, self.frame, 1.0, false);
        let png = ateliet_core::board::tile::encode_png(&px, w, h);
        let stem = self.abs.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let out = self.abs.with_file_name(format!("{stem} - {}.png", self.frame + 1));
        self.error = Some(match std::fs::write(&out, png) {
            Ok(()) => format!("Exportado: {}", out.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            Err(e) => e.to_string(),
        });
    }

    pub(super) fn export_pages_pdf(&mut self) {
        let db = crate::fonts::book_fonts();
        let stem = self.abs.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let out = self.abs.with_file_name(format!("{stem}.pdf"));
        self.error = Some(match pg::pages_pdf(&mut self.doc, &mut self.pics, &db, &stem).and_then(|b| std::fs::write(&out, b).map_err(|e| e.to_string())) {
            Ok(()) => format!("PDF das páginas: {}", out.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            Err(e) => format!("Não foi possível exportar: {e}"),
        });
    }
}

impl BoardEditor {
    /// The shot's notes (animatic, presentation board): shot, camera,
    /// action, dialogue, sound, and its length.
    pub(super) fn notes_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let f = self.frame;
        ui.label(RichText::new("Plano").font(kit::medium(13.0)).color(p.text));
        ui.add_space(4.0);
        let fps = self.doc.fps().max(1) as f32;
        let mut secs = num(&self.doc.frame(f)["duration"], 24.0) / fps;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Duração").size(12.0).color(p.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::DragValue::new(&mut secs).range(0.04..=600.0).speed(0.05).max_decimals(2).suffix(" s")).changed() {
                    self.doc.board["frames"][f]["duration"] = json!((secs * fps).round().max(1.0));
                    self.dirty = true;
                    self.last_change = Some(Instant::now());
                }
            });
        });
        let w = ui.available_width();
        for (k, label, rows) in [("shot", "Enquadramento", 1), ("camera", "Câmera", 1), ("action", "Ação", 3), ("dialogue", "Diálogo", 2), ("sound", "Som", 1)] {
            ui.label(RichText::new(label).size(12.0).color(p.muted));
            let mut v = self.doc.frame(f)["notes"][k].as_str().unwrap_or("").to_string();
            let te = if rows > 1 { egui::TextEdit::multiline(&mut v).desired_rows(rows).desired_width(w) } else { egui::TextEdit::singleline(&mut v).desired_width(w) };
            if ui.add(te).changed() {
                if !self.doc.board["frames"][f]["notes"].is_object() {
                    self.doc.board["frames"][f]["notes"] = json!({});
                }
                self.doc.board["frames"][f]["notes"][k] = json!(v);
                self.dirty = true;
                self.last_change = Some(Instant::now());
            }
        }
        ui.add_space(8.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
    }

    pub(super) fn export_presentation_pdf(&mut self) {
        let db = crate::fonts::book_fonts();
        let stem = self.abs.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let out = self.abs.with_file_name(format!("{stem} - prancha.pdf"));
        self.error = Some(match pg::presentation_pdf(&mut self.doc, &mut self.pics, &db, &stem, 6).and_then(|b| std::fs::write(&out, b).map_err(|e| e.to_string())) {
            Ok(()) => format!("Prancha em PDF: {}", out.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            Err(e) => format!("Não foi possível exportar: {e}"),
        });
    }

    pub(super) fn export_webtoon_slices(&mut self) {
        let db = crate::fonts::book_fonts();
        let stem = self.abs.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = self.abs.with_file_name(format!("{stem} - fatias"));
        let _ = std::fs::create_dir_all(&dir);
        let slices = pg::webtoon_slices(&mut self.doc, &mut self.pics, &db);
        let parts = self.doc.frame_count();
        let mut n = 0;
        for (part, k, w, h, px) in slices {
            let name = if parts > 1 { format!("{stem} parte {} {k:02}.png", part + 1) } else { format!("{stem} {k:02}.png") };
            if std::fs::write(dir.join(name), ateliet_core::board::tile::encode_png(&px, w, h)).is_ok() {
                n += 1;
            }
        }
        self.error = Some(format!("{n} fatias em {}", dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
    }
}

/// Choosing what kind of storyboard to make, with its options.
pub struct NewBoard {
    pub o: pg::KindOptions,
}

impl NewBoard {
    pub fn new() -> NewBoard {
        NewBoard { o: pg::KindOptions::defaults("animatic") }
    }

    /// Some(Some(options)) to create, Some(None) to cancel.
    pub fn ui(&mut self, ctx: &egui::Context, p: crate::theme::Palette) -> Option<Option<pg::KindOptions>> {
        let mut out = None;
        kit::scrim(ctx, p);
        egui::Window::new("new-board").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0]).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("Novo storyboard").font(kit::medium(17.0)).color(p.text));
            ui.add_space(8.0);
            let icons_of = |k: &str| match k {
                "animatic" => icons::CLAPPERBOARD,
                "presentation" => icons::PRESENTATION,
                "thumbnails" => icons::LAYOUT_GRID,
                "name" => icons::BOOK_OPEN,
                "comic" => icons::BOOK_OPEN_TEXT,
                "webtoon" => icons::SMARTPHONE,
                _ => icons::INFINITY,
            };
            egui::Grid::new("kinds").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                for (i, k) in pg::KINDS.iter().enumerate() {
                    let sel = self.o.kind == k.id;
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(272.0, 68.0), Sense::click());
                    ui.painter().rect(rect, 8.0, if sel { p.bg_active } else if r.hovered() { p.bg_hover } else { p.bg }, egui::Stroke::new(1.0_f32, if sel { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
                    ui.painter().text(rect.left_center() + egui::vec2(18.0, 0.0), egui::Align2::CENTER_CENTER, icons_of(k.id), kit::icon_font(16.0), if sel { p.accent_strong } else { p.muted });
                    ui.painter().text(rect.left_top() + egui::vec2(36.0, 10.0), egui::Align2::LEFT_TOP, k.name, kit::medium(13.0), p.text);
                    let g = ui.painter().layout(k.blurb.to_string(), egui::FontId::proportional(11.0), p.faint, 226.0);
                    ui.painter().galley(rect.left_top() + egui::vec2(36.0, 27.0), g, p.faint);
                    if r.clicked() {
                        self.o = pg::KindOptions::defaults(k.id);
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(10.0);
            let o = &mut self.o;
            match o.kind.as_str() {
                "name" | "comic" => {
                    let sizes: &[pg::PageSize] = if o.kind == "name" { &pg::NAME_SIZES } else { &pg::COMIC_SIZES };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Formato").size(12.5).color(p.muted));
                        egui::ComboBox::from_id_salt("nb-size").width(300.0).selected_text(sizes.iter().find(|s| s.0 == o.page_size).map(|s| s.1).unwrap_or(sizes[0].1)).show_ui(ui, |ui| {
                            for s in sizes {
                                if ui.selectable_label(o.page_size == s.0, s.1).clicked() {
                                    o.page_size = s.0.into();
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Resolução").size(12.5).color(p.muted));
                        for d in [150.0, 300.0, 600.0] {
                            if kit::chip(ui, None, &format!("{d:.0} dpi"), o.dpi == d, p).clicked() {
                                o.dpi = d;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Páginas").size(12.5).color(p.muted));
                        ui.add(egui::DragValue::new(&mut o.pages).range(1..=400));
                        ui.add_space(12.0);
                        kit::toggle(ui, &mut o.rtl, p);
                        ui.label(RichText::new("Da direita para a esquerda").size(12.5).color(p.muted));
                        kit::toggle(ui, &mut o.spreads, p);
                        ui.label(RichText::new("Páginas duplas").size(12.5).color(p.muted));
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Primeiro layout").size(12.5).color(p.muted));
                        egui::ComboBox::from_id_salt("nb-layout").width(260.0).selected_text(pg::LAYOUTS.iter().find(|l| l.0 == o.layout).map(|l| l.1).unwrap_or("Um quadro")).show_ui(ui, |ui| {
                            for (id, name, _) in pg::LAYOUTS {
                                if ui.selectable_label(o.layout == id, name).clicked() {
                                    o.layout = id.into();
                                }
                            }
                        });
                    });
                }
                "thumbnails" => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Proporção").size(12.5).color(p.muted));
                        egui::ComboBox::from_id_salt("nb-aspect").width(220.0).selected_text(pg::THUMB_ASPECTS.iter().find(|a| a.0 == o.aspect).map(|a| a.1).unwrap_or("16:9")).show_ui(ui, |ui| {
                            for (id, name, _) in pg::THUMB_ASPECTS {
                                if ui.selectable_label(o.aspect == id, name).clicked() {
                                    o.aspect = id.into();
                                }
                            }
                        });
                        kit::toggle(ui, &mut o.landscape, p);
                        ui.label(RichText::new("Folha deitada").size(12.5).color(p.muted));
                    });
                }
                "webtoon" => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Largura").size(12.5).color(p.muted));
                        for (w, name) in pg::WEBTOON_WIDTHS {
                            if kit::chip(ui, None, name, o.strip_width == w, p).clicked() {
                                o.strip_width = w;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Fatias de 1280 px").size(12.5).color(p.muted));
                        ui.add(egui::DragValue::new(&mut o.slices).range(1..=200));
                    });
                }
                "canvas" => {}
                _ => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Tamanho").size(12.5).color(p.muted));
                        for (w, h, name) in [(1920, 1080, "1920 × 1080"), (1280, 720, "1280 × 720"), (2048, 858, "2,39:1"), (1080, 1920, "Vertical")] {
                            if kit::chip(ui, None, name, o.width == w && o.height == h, p).clicked() {
                                o.width = w;
                                o.height = h;
                            }
                        }
                    });
                }
            }
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary(ui, "Criar", p).clicked() {
                    out = Some(Some(self.o.clone()));
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    out = Some(None);
                }
            });
        });
        out
    }
}
