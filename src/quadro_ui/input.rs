//! The pointer and the keyboard on the canvas: tools, selection, moving
//! and resizing, carrying cards between columns, zoom and panning.

use super::edit::Hit;
use super::{draw, Cx, Drag, Editing, Out, QuadroEditor, Sel, Tool, View};
use ateliet_core::quadro::card;
use ateliet_core::quadro::element::{rotate, Element};
use eframe::egui::{self, pos2, vec2, Pos2, Rect, Stroke};
use serde_json::json;

type P = (f32, f32);

/// Which handle: the eight of the box, the rotation knob, a point of a line.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Handle {
    Box(usize),
    Rotate,
    Point(usize),
}

fn snap_angle(a: P, b: P) -> P {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    let step = 15f32.to_radians();
    let ang = (dy.atan2(dx) / step).round() * step;
    (a.0 + len * ang.cos(), a.1 + len * ang.sin())
}

impl QuadroEditor {
    /// The selection's handles on screen.
    pub(super) fn handles(&self, v: &View) -> Vec<(Handle, Pos2)> {
        let mut out = Vec::new();
        if self.drag.is_some() && !matches!(self.drag, Some(Drag::Resize { .. }) | Some(Drag::Rotate { .. }) | Some(Drag::Point { .. })) {
            return out;
        }
        // one line or arrow: its points
        if let [Sel::El(id)] = self.sel.as_slice() {
            if let Some(e) = self.board.element(id) {
                if matches!(e.kind(), "line" | "arrow") {
                    for (i, p) in e.abs_points().iter().enumerate() {
                        out.push((Handle::Point(i), v.s(*p)));
                    }
                    return out;
                }
                if !e.is_linear() && !e.locked() {
                    let (_, y0, _, _) = e.bounds();
                    let c = e.center();
                    let knob = rotate((c.0, y0 - 24.0 / v.zoom), c, e.angle());
                    out.push((Handle::Rotate, v.s(knob)));
                }
            }
        }
        let Some((x0, y0, x1, y1)) = self.selection_box() else { return out };
        let only_lanes = self.sel.iter().all(|s| matches!(s, Sel::Lane(_)));
        let only_cards = self.sel.iter().all(|s| matches!(s, Sel::Card(_) | Sel::Lane(_)));
        let pts = [(x0, y0), ((x0 + x1) / 2.0, y0), (x1, y0), (x1, (y0 + y1) / 2.0), (x1, y1), ((x0 + x1) / 2.0, y1), (x0, y1), (x0, (y0 + y1) / 2.0)];
        for (i, p) in pts.iter().enumerate() {
            // columns and cards only get wider or narrower
            if (only_lanes || only_cards) && i != 3 && i != 7 {
                continue;
            }
            // cards in a column take its width
            if self.sel.iter().any(|s| matches!(s, Sel::Card(id) if self.board.cards.iter().all(|c| &c.id != id))) {
                continue;
            }
            out.push((Handle::Box(i), v.s(*p)));
        }
        out
    }

    fn handle_at(&self, v: &View, q: Pos2) -> Option<Handle> {
        self.handles(v).into_iter().find(|(_, p)| p.distance(q) <= 7.0).map(|(h, _)| h)
    }

    pub(super) fn draw_selection(&self, painter: &egui::Painter, v: &View, p: crate::theme::Palette) {
        let col = p.accent_strong;
        for s in &self.sel {
            if let Sel::El(id) = s {
                let Some(e) = self.board.element(id) else { continue };
                let (x0, y0, x1, y1) = e.bounds();
                let pad = 4.0 / v.zoom;
                let c = e.center();
                let corners: Vec<Pos2> = [(x0 - pad, y0 - pad), (x1 + pad, y0 - pad), (x1 + pad, y1 + pad), (x0 - pad, y1 + pad), (x0 - pad, y0 - pad)].iter().map(|q| v.s(rotate(*q, c, e.angle()))).collect();
                painter.add(egui::Shape::line(corners, Stroke::new(1.0_f32, col.gamma_multiply(0.7))));
            }
        }
        if self.sel.len() > 1 {
            if let Some((x0, y0, x1, y1)) = self.selection_box() {
                painter.rect_stroke(Rect::from_min_max(v.s((x0, y0)), v.s((x1, y1))).expand(6.0), 2.0, Stroke::new(1.0_f32, col.gamma_multiply(0.5)), egui::StrokeKind::Outside);
            }
        }
        for (h, q) in self.handles(v) {
            match h {
                Handle::Rotate | Handle::Point(_) => {
                    painter.circle(q, 5.0, p.bg, Stroke::new(1.5_f32, col));
                }
                Handle::Box(_) => {
                    painter.rect(Rect::from_center_size(q, vec2(9.0, 9.0)), 2.0, p.bg, Stroke::new(1.5_f32, col), egui::StrokeKind::Middle);
                }
            }
        }
    }

    /// Back to the selection tool after drawing one thing (unless locked).
    fn tool_done(&mut self) {
        if !self.lock {
            self.tool = Tool::Select;
        }
    }

    fn start_edit(&mut self, e: Editing) {
        self.commit_edit();
        self.editing = Some(e);
        self.focus_edit = true;
    }

    pub(super) fn edit_card(&mut self, id: &str, new: bool) {
        let buf = self.board.card(id).map(|c| c.text.clone()).unwrap_or_default();
        self.start_edit(Editing::Card { id: id.to_string(), buf, new });
    }

    pub(super) fn edit_lane(&mut self, id: &str, new: bool) {
        let buf = self.board.lanes.iter().find(|l| l.id == id).map(|l| l.title.clone()).unwrap_or_default();
        self.start_edit(Editing::Lane { id: id.to_string(), buf, new });
    }

    pub(super) fn edit_text(&mut self, id: &str, new: bool) {
        let buf = self.board.element(id).map(|e| e.raw_text().to_string()).unwrap_or_default();
        // the text changes as it is typed: the state before it, for undo
        if !new {
            self.checkpoint();
        }
        self.start_edit(Editing::Text { id: id.to_string(), orig: buf.clone(), buf, new });
    }

    /// Edit (or make) the text inside a shape.
    pub(super) fn edit_inside(&mut self, ctx: &egui::Context, shape: &str) {
        let existing = self.board.element(shape).and_then(|e| e.bound().into_iter().find(|(_, k)| k == "text").map(|(i, _)| i));
        if let Some(t) = existing.filter(|t| self.board.element(t).is_some_and(|e| !e.deleted())) {
            self.edit_text(&t, false);
            return;
        }
        self.checkpoint();
        let mut st = self.style.clone();
        st.align = "center".into();
        let Some(c) = self.board.element(shape) else { return };
        let (cx, cy) = c.center();
        let mut t = Element::new("text", cx, cy, 10.0, 10.0, &st);
        t.set("containerId", json!(shape));
        t.set("verticalAlign", json!("middle"));
        t.set("strokeColor", c.get("strokeColor").clone());
        let id = t.id().to_string();
        self.board.elements.push(t);
        if let Some(c) = self.board.element_mut(shape) {
            c.add_bound(&id, "text");
        }
        self.fit_text(ctx, &id);
        self.changed();
        self.edit_text(&id, true);
    }

    /// The link a card or element points to (Ctrl/Cmd+click opens it).
    fn link_of(&self, h: &Hit) -> Option<String> {
        let trig = ateliet_core::quadro::layout::triggers(&self.board);
        match h {
            Hit::Card(id) => {
                let c = self.board.card(id)?;
                let info = card::parse(&c.text, &trig.0, &trig.1);
                info.note().map(str::to_string).or_else(|| info.segs.iter().find_map(|s| if let card::Seg::Url(u, _) = s { Some(u.clone()) } else { None }))
            }
            Hit::El(id) => {
                let e = self.board.element(id)?;
                let from = |s: &str| s.find("[[").and_then(|a| s[a + 2..].find("]]").map(|b| s[a + 2..a + 2 + b].split('|').next().unwrap_or("").to_string()));
                let link = e.get("link").as_str().filter(|l| !l.is_empty()).map(|l| from(l).unwrap_or_else(|| l.to_string()));
                link.or_else(|| from(e.raw_text())).or_else(|| {
                    // the text inside a shape
                    e.bound().into_iter().find(|(_, k)| k == "text").and_then(|(t, _)| self.board.element(&t).and_then(|t| from(t.raw_text())))
                })
            }
            _ => None,
        }
    }

    fn open_link(&mut self, link: String) {
        if link.starts_with("http://") || link.starts_with("https://") {
            let _ = open::that(&link);
        } else {
            self.out.push(Out::Open(link));
        }
    }

    pub(super) fn pointer(&mut self, ui: &mut egui::Ui, resp: &egui::Response, canvas: Rect, cx: &mut Cx) {
        let ctx = ui.ctx().clone();
        self.hover = ui.input(|i| i.pointer.hover_pos()).filter(|q| canvas.contains(*q));
        let v = self.view(canvas);
        if let Some(h) = self.hover {
            self.last_pointer_world = v.w(h);
        }
        let mods = ui.input(|i| i.modifiers);
        // zoom with the wheel (or a pinch) at the pointer
        if resp.hovered() && self.menu.is_none() {
            let scroll = ui.input(|i| i.smooth_scroll_delta);
            let pinch = ui.input(|i| i.zoom_delta());
            let factor = if pinch != 1.0 { pinch } else { (1.0 + scroll.y * 0.0025).clamp(0.5, 2.0) };
            if factor != 1.0 {
                if let Some(q) = self.hover {
                    let before = v.w(q);
                    self.zoom = (self.zoom * factor).clamp(0.05, 8.0);
                    self.pan = q - canvas.min - vec2(before.0, before.1) * self.zoom;
                    self.fit = false;
                }
            }
            // sideways scroll pans
            if scroll.x != 0.0 && pinch == 1.0 {
                self.pan.x += scroll.x;
            }
        }
        let space = self.editing.is_none() && ui.input(|i| i.key_down(egui::Key::Space));
        let panning = self.tool == Tool::Hand || space || ui.input(|i| i.pointer.middle_down()) || matches!(self.drag, Some(Drag::Pan));
        if panning {
            if resp.hovered() {
                ctx.set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            }
            if resp.drag_started() {
                self.drag = Some(Drag::Pan);
            }
            if resp.dragged() {
                self.pan += resp.drag_delta();
            }
            if resp.drag_stopped() {
                self.drag = None;
            }
            if matches!(self.drag, Some(Drag::Pan)) || resp.dragged() {
                return;
            }
        }
        let v = self.view(canvas);
        // a note or picture dragged from the file explorer
        if let Some(path) = resp.dnd_release_payload::<String>() {
            let at = self.hover.map(|h| v.w(h)).unwrap_or(self.last_pointer_world);
            if cx.vault.abs(&path).is_file() {
                self.insert_file(cx, &ctx, &path, at);
            }
            return;
        }
        if resp.dnd_hover_payload::<String>().is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Copy);
        }
        let tol = 6.0 / self.zoom;
        // cursors
        if let Some(h) = self.hover.filter(|_| self.drag.is_none()) {
            match self.tool {
                Tool::Select => {
                    if let Some(hd) = self.handle_at(&v, h) {
                        ctx.set_cursor_icon(match hd {
                            Handle::Box(0) | Handle::Box(4) => egui::CursorIcon::ResizeNwSe,
                            Handle::Box(2) | Handle::Box(6) => egui::CursorIcon::ResizeNeSw,
                            Handle::Box(1) | Handle::Box(5) => egui::CursorIcon::ResizeVertical,
                            Handle::Box(_) => egui::CursorIcon::ResizeHorizontal,
                            _ => egui::CursorIcon::Crosshair,
                        });
                    } else {
                        match self.hit(v.w(h), tol) {
                            Some(Hit::Card(id)) if self.board.card(&id).is_some_and(|c| card::parse(&c.text, "@", "@@").is_note_card()) => ctx.set_cursor_icon(egui::CursorIcon::PointingHand),
                            Some(Hit::Check(_) | Hit::LaneButton(..) | Hit::LaneFoot(_)) => ctx.set_cursor_icon(egui::CursorIcon::PointingHand),
                            Some(Hit::El(_) | Hit::Card(_) | Hit::LaneHead(_)) => ctx.set_cursor_icon(egui::CursorIcon::Move),
                            _ => {}
                        }
                    }
                }
                Tool::Text => ctx.set_cursor_icon(egui::CursorIcon::Text),
                Tool::Eraser | Tool::Draw | Tool::Rect | Tool::Diamond | Tool::Ellipse | Tool::Arrow | Tool::Line => ctx.set_cursor_icon(egui::CursorIcon::Crosshair),
                _ => {}
            }
        }
        if resp.secondary_clicked() {
            if let Some(h) = self.hover {
                let w = v.w(h);
                let target = match self.hit(w, tol) {
                    Some(hit) => {
                        if !matches!(&hit, Hit::El(id) if self.sel.contains(&Sel::El(id.clone()))) {
                            self.select_hit(&hit, false);
                        }
                        self.sel.first().cloned()
                    }
                    None => {
                        self.sel.clear();
                        None
                    }
                };
                self.menu = Some((target, h, w));
                self.menu_text.clear();
                self.menu_month = None;
            }
        }
        if resp.double_clicked() && self.tool == Tool::Select {
            let Some(h) = self.hover else { return };
            let w = v.w(h);
            match self.hit(w, tol) {
                Some(Hit::Card(id)) => self.edit_card(&id, false),
                Some(Hit::LaneHead(i)) => {
                    let id = self.board.lanes[i].id.clone();
                    self.edit_lane(&id, false);
                }
                Some(Hit::El(id)) => {
                    let kind = self.board.element(&id).map(|e| e.kind().to_string()).unwrap_or_default();
                    match kind.as_str() {
                        "text" => self.edit_text(&id, false),
                        "rectangle" | "ellipse" | "diamond" => self.edit_inside(&ctx, &id),
                        _ => {}
                    }
                }
                None => {
                    if self.is_kanban() {
                        let id = self.add_loose_card((w.0 - 120.0, w.1 - 20.0), "");
                        self.sel = vec![Sel::Card(id.clone())];
                        self.edit_card(&id, true);
                    } else {
                        self.new_text(&ctx, w);
                    }
                }
                _ => {}
            }
            return;
        }
        if resp.clicked() {
            let Some(h) = self.hover else { return };
            let w = v.w(h);
            self.menu = None;
            self.commit_edit();
            match self.tool {
                Tool::Select => self.click(w, tol, mods),
                Tool::Text => match self.hit(w, tol) {
                    Some(Hit::El(id)) if self.board.element(&id).is_some_and(|e| e.kind() == "text") => {
                        self.sel = vec![Sel::El(id.clone())];
                        self.edit_text(&id, false);
                    }
                    _ => {
                        self.new_text(&ctx, w);
                        self.tool_done();
                    }
                },
                Tool::Card => {
                    match self.drop_target(w) {
                        Some((li, _)) => {
                            if let Some(id) = self.add_card_to_lane(li, "") {
                                self.sel = vec![Sel::Card(id.clone())];
                                self.edit_card(&id, true);
                            } else {
                                self.out.push(Out::Toast("A coluna está cheia.".into()));
                            }
                        }
                        None => {
                            let id = self.add_loose_card((w.0 - 120.0, w.1 - 20.0), "");
                            self.sel = vec![Sel::Card(id.clone())];
                            self.edit_card(&id, true);
                        }
                    }
                    self.tool_done();
                }
                Tool::Lane => {
                    let id = self.add_lane(Some((w.0 - 136.0, w.1 - 22.0)), "Nova coluna");
                    self.sel = vec![Sel::Lane(id.clone())];
                    self.edit_lane(&id, true);
                    self.tool_done();
                }
                Tool::Eraser => {
                    if let Some(Hit::El(id)) = self.hit(w, tol) {
                        self.checkpoint();
                        let mut gone: std::collections::HashSet<String> = self.with_group(&id).into_iter().collect();
                        for e in &self.board.elements {
                            if e.container().is_some_and(|c| gone.contains(c)) {
                                gone.insert(e.id().to_string());
                            }
                        }
                        self.remove_elements(&gone);
                        self.sel.retain(|s| !matches!(s, Sel::El(i) if gone.contains(i)));
                        self.changed();
                    }
                }
                _ => {}
            }
            return;
        }
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(resp.interact_pointer_pos().unwrap_or(canvas.center()));
            self.menu = None;
            self.commit_edit();
            self.start_drag(&v, origin, tol, mods);
        }
        if let Some(q) = ui.input(|i| i.pointer.interact_pos()) {
            if resp.dragged_by(egui::PointerButton::Primary) && self.drag.is_some() {
                self.update_drag(v.w(q), mods);
            }
        }
        if resp.drag_stopped() && self.drag.is_some() {
            self.end_drag(&ctx, mods);
        }
    }

    pub(super) fn new_text(&mut self, ctx: &egui::Context, w: P) {
        self.checkpoint();
        let size = self.style.font_size;
        let e = Element::new("text", w.0, w.1 - size * 0.6, 10.0, size * 1.25, &self.style);
        let id = e.id().to_string();
        self.board.elements.push(e);
        self.fit_text(ctx, &id);
        self.sel = vec![Sel::El(id.clone())];
        self.changed();
        self.edit_text(&id, true);
    }

    fn click(&mut self, w: P, tol: f32, mods: egui::Modifiers) {
        let hit = self.hit(w, tol);
        let Some(h) = hit else {
            if !mods.shift {
                self.sel.clear();
            }
            return;
        };
        if mods.command {
            if let Some(l) = self.link_of(&h) {
                self.open_link(l);
                return;
            }
        }
        match &h {
            Hit::Check(id) => {
                self.checkpoint();
                if let Some(c) = self.board.card_mut(id) {
                    c.check = if c.check == ' ' { 'x' } else { ' ' };
                }
                self.changed();
            }
            Hit::LaneButton(i, 0) => {
                self.checkpoint();
                self.board.lanes[*i].collapsed = !self.board.lanes[*i].collapsed;
                self.changed();
            }
            Hit::LaneButton(i, 1) | Hit::LaneFoot(i) => {
                if let Some(id) = self.add_card_to_lane(*i, "") {
                    self.sel = vec![Sel::Card(id.clone())];
                    self.edit_card(&id, true);
                } else {
                    let l = &self.board.lanes[*i];
                    self.out.push(Out::Toast(format!("A coluna “{}” está cheia (limite {}).", l.title, l.max)));
                }
            }
            Hit::LaneButton(i, _) => {
                let lid = self.board.lanes[*i].id.clone();
                self.sel = vec![Sel::Lane(lid.clone())];
                let at = self.scene.lanes[*i].rect;
                self.menu = Some((Some(Sel::Lane(lid)), self.hover.unwrap_or_default(), (at.0 + at.2, at.1 + 30.0)));
                self.menu_text.clear();
            }
            Hit::Card(id) if !mods.shift => {
                // a card that is a note opens it
                let note = self.board.card(id).map(|c| card::parse(&c.text, "@", "@@")).filter(|i| i.is_note_card()).and_then(|i| i.note().map(str::to_string));
                self.sel = vec![Sel::Card(id.clone())];
                if let Some(n) = note {
                    self.open_link(n);
                }
            }
            _ => self.select_hit(&h, mods.shift),
        }
    }

    fn start_drag(&mut self, v: &View, origin: Pos2, tol: f32, mods: egui::Modifiers) {
        let w = v.w(origin);
        match self.tool {
            Tool::Select => {
                if let Some(h) = self.handle_at(v, origin) {
                    self.checkpoint();
                    self.drag = Some(match h {
                        Handle::Box(i) => Drag::Resize { handle: i, start: w, box0: self.selection_box().unwrap_or((w.0, w.1, w.0 + 1.0, w.1 + 1.0)), orig: self.board.clone() },
                        Handle::Rotate => Drag::Rotate { id: self.sel_els()[0].clone() },
                        Handle::Point(i) => Drag::Point { id: self.sel_els()[0].clone(), index: i },
                    });
                    return;
                }
                match self.hit(w, tol) {
                    Some(Hit::Card(id)) | Some(Hit::Check(id)) if !(self.sel.len() > 1 && self.sel.contains(&Sel::Card(id.clone()))) => {
                        self.sel = vec![Sel::Card(id.clone())];
                        let top = self.scene.card(&id).map(|c| (c.rect.0, c.rect.1)).unwrap_or(w);
                        self.checkpoint();
                        self.drag = Some(Drag::Card { id, grab: (w.0 - top.0, w.1 - top.1), at: w, moved: false });
                    }
                    Some(h) => {
                        if !mods.shift || !self.sel.iter().any(|s| matches!((s, &h), (Sel::El(a), Hit::El(b)) if a == b)) {
                            self.select_hit(&h, mods.shift);
                        }
                        self.checkpoint();
                        self.drag = Some(Drag::Move { last: w, moved: false });
                    }
                    None => {
                        if !mods.shift {
                            self.sel.clear();
                        }
                        self.drag = Some(Drag::Marquee(w));
                    }
                }
            }
            Tool::Rect | Tool::Diamond | Tool::Ellipse | Tool::Arrow | Tool::Line => {
                let kind = match self.tool {
                    Tool::Rect => "rectangle",
                    Tool::Diamond => "diamond",
                    Tool::Ellipse => "ellipse",
                    Tool::Arrow => "arrow",
                    _ => "line",
                };
                self.checkpoint();
                let mut e = Element::new(kind, w.0, w.1, 0.0, 0.0, &self.style);
                if e.is_linear() {
                    e.set_abs_points(&[w, w]);
                }
                let id = e.id().to_string();
                self.board.elements.push(e);
                if kind == "arrow" {
                    let t = self.bind_target(w, &id);
                    self.set_binding(&id, false, t);
                }
                self.drag = Some(Drag::Create { id, start: w });
            }
            Tool::Draw => {
                self.checkpoint();
                let mut e = Element::new("freedraw", w.0, w.1, 0.0, 0.0, &self.style);
                e.set("backgroundColor", json!("transparent"));
                e.set_abs_points(&[w]);
                let id = e.id().to_string();
                self.board.elements.push(e);
                self.drag = Some(Drag::Draw { id });
            }
            Tool::Eraser => {
                self.checkpoint();
                self.drag = Some(Drag::Erase(Default::default()));
                self.update_drag(w, mods);
            }
            _ => {}
        }
    }

    fn update_drag(&mut self, w: P, mods: egui::Modifiers) {
        let Some(drag) = self.drag.take() else { return };
        self.drag = Some(match drag {
            Drag::Move { last, .. } => {
                let (dx, dy) = (w.0 - last.0, w.1 - last.1);
                if dx != 0.0 || dy != 0.0 {
                    self.move_sel(dx, dy);
                }
                Drag::Move { last: w, moved: true }
            }
            Drag::Card { id, grab, .. } => Drag::Card { id, grab, at: w, moved: true },
            Drag::Resize { handle, start, box0, orig } => {
                self.resize(handle, box0, w, &orig, mods.shift);
                Drag::Resize { handle, start, box0, orig }
            }
            Drag::Rotate { id } => {
                if let Some(e) = self.board.element(&id) {
                    let c = e.center();
                    let mut a = (w.1 - c.1).atan2(w.0 - c.0) + std::f32::consts::FRAC_PI_2;
                    if mods.shift {
                        let s = 15f32.to_radians();
                        a = (a / s).round() * s;
                    }
                    let a = a.rem_euclid(std::f32::consts::TAU);
                    if let Some(e) = self.board.element_mut(&id) {
                        e.set_f("angle", a);
                    }
                    self.center_bound_text(&id);
                    self.changed();
                }
                Drag::Rotate { id }
            }
            Drag::Point { id, index } => {
                if let Some(e) = self.board.element(&id) {
                    let mut pts = e.abs_points();
                    if index < pts.len() {
                        let anchor = if index > 0 { pts[index - 1] } else if pts.len() > 1 { pts[1] } else { w };
                        pts[index] = if mods.shift { snap_angle(anchor, w) } else { w };
                        if let Some(e) = self.board.element_mut(&id) {
                            e.set_abs_points(&pts);
                        }
                        self.changed();
                    }
                }
                Drag::Point { id, index }
            }
            Drag::Create { id, start } => {
                if let Some(e) = self.board.element_mut(&id) {
                    if e.is_linear() {
                        let end = if mods.shift { snap_angle(start, w) } else { w };
                        e.set_abs_points(&[start, end]);
                    } else {
                        let (mut dx, mut dy) = (w.0 - start.0, w.1 - start.1);
                        if mods.shift {
                            let m = dx.abs().max(dy.abs());
                            dx = m * dx.signum();
                            dy = m * dy.signum();
                        }
                        e.set_f("x", start.0.min(start.0 + dx));
                        e.set_f("y", start.1.min(start.1 + dy));
                        e.set_f("width", dx.abs());
                        e.set_f("height", dy.abs());
                    }
                }
                self.changed();
                Drag::Create { id, start }
            }
            Drag::Draw { id } => {
                if let Some(e) = self.board.element(&id) {
                    let mut pts = e.abs_points();
                    if pts.last().is_none_or(|l| ((l.0 - w.0).powi(2) + (l.1 - w.1).powi(2)).sqrt() > 1.0 / self.zoom) {
                        pts.push(w);
                        if let Some(e) = self.board.element_mut(&id) {
                            e.set_abs_points(&pts);
                        }
                        self.changed();
                    }
                }
                Drag::Draw { id }
            }
            Drag::Erase(mut set) => {
                if let Some(Hit::El(id)) = self.hit(w, 6.0 / self.zoom) {
                    for g in self.with_group(&id) {
                        set.insert(g);
                    }
                    // seen at once: fade them
                    for i in &set {
                        if let Some(e) = self.board.element_mut(i) {
                            if e.fo("opacity", 100.0) > 30.0 {
                                let o = e.fo("opacity", 100.0);
                                e.v.insert("ateliet-opacity".into(), json!(o));
                                e.set_f("opacity", 25.0);
                            }
                        }
                    }
                }
                Drag::Erase(set)
            }
            other => other,
        });
    }

    fn end_drag(&mut self, ctx: &egui::Context, mods: egui::Modifiers) {
        let Some(drag) = self.drag.take() else { return };
        match drag {
            Drag::Pan => {}
            Drag::Move { moved, .. } => {
                if !moved {
                    self.uncheckpoint();
                } else {
                    self.unbind_moved_arrows();
                    if self.sel.iter().any(|s| matches!(s, Sel::Lane(_))) {
                        self.board.sort_lanes();
                    }
                }
            }
            Drag::Card { id, grab, at, moved } => {
                if !moved {
                    self.uncheckpoint();
                    return;
                }
                if let Some(msg) = self.drop_card(&id, at, (at.0 - grab.0, at.1 - grab.1)) {
                    self.uncheckpoint();
                    self.out.push(Out::Toast(msg));
                }
            }
            Drag::Marquee(a) => {
                let b = self.last_pointer_world;
                let r = Rect::from_two_pos(pos2(a.0, a.1), pos2(b.0, b.1));
                let inside = |x: (f32, f32, f32, f32)| r.contains_rect(Rect::from_min_max(pos2(x.0, x.1), pos2(x.2, x.3)));
                let mut picked = Vec::new();
                for e in self.board.elements.iter().filter(|e| !e.deleted() && e.container().is_none()) {
                    if inside(e.aabb()) {
                        for g in self.with_group(e.id()) {
                            let s = Sel::El(g);
                            if !picked.contains(&s) {
                                picked.push(s);
                            }
                        }
                    }
                }
                for cb in self.scene.loose.iter().chain(self.scene.lanes.iter().flat_map(|l| l.cards.iter())) {
                    if inside((cb.rect.0, cb.rect.1, cb.rect.0 + cb.rect.2, cb.rect.1 + cb.rect.3)) {
                        picked.push(Sel::Card(cb.id.clone()));
                    }
                }
                for lb in &self.scene.lanes {
                    if inside((lb.rect.0, lb.rect.1, lb.rect.0 + lb.rect.2, lb.rect.1 + lb.rect.3)) {
                        picked.push(Sel::Lane(lb.id.clone()));
                        // a whole column goes as one
                        picked.retain(|s| !matches!(s, Sel::Card(c) if lb.cards.iter().any(|x| &x.id == c)));
                    }
                }
                if mods.shift {
                    for s in picked {
                        if !self.sel.contains(&s) {
                            self.sel.push(s);
                        }
                    }
                } else {
                    self.sel = picked;
                }
            }
            Drag::Resize { .. } => {
                let texts: Vec<String> = self.board.elements.iter().filter(|e| e.container().is_some_and(|c| self.sel.contains(&Sel::El(c.to_string())))).map(|e| e.id().to_string()).collect();
                for t in texts {
                    self.fit_text(ctx, &t);
                }
                if self.sel.iter().any(|s| matches!(s, Sel::Lane(_))) {
                    self.board.sort_lanes();
                }
            }
            Drag::Rotate { .. } => {}
            Drag::Point { id, index } => {
                let (n, kind) = self.board.element(&id).map(|e| (e.points().len(), e.kind().to_string())).unwrap_or((0, String::new()));
                if kind == "arrow" && (index == 0 || index + 1 == n) {
                    let end = index + 1 == n && index > 0;
                    let p = self.board.element(&id).map(|e| e.abs_points()[index]).unwrap_or_default();
                    let t = self.bind_target(p, &id);
                    self.set_binding(&id, end, t);
                    self.reroute(&id);
                }
            }
            Drag::Create { id, .. } => {
                let Some(e) = self.board.element(&id) else { return };
                let (x0, y0, x1, y1) = e.bounds();
                let tiny = (x1 - x0).max(y1 - y0) < 4.0 / self.zoom.max(0.2);
                if tiny {
                    self.board.elements.retain(|e| e.id() != id);
                    self.uncheckpoint();
                    return;
                }
                if e.kind() == "arrow" {
                    let end = *e.abs_points().last().unwrap();
                    let t = self.bind_target(end, &id).filter(|t| Some(t) != self.board.element(&id).and_then(|e| e.binding(false)).as_ref());
                    self.set_binding(&id, true, t);
                    self.reroute(&id);
                }
                self.sel = vec![Sel::El(id)];
                self.tool_done();
            }
            Drag::Draw { .. } => {}
            Drag::Erase(set) => {
                for e in self.board.elements.iter_mut() {
                    if let Some(o) = e.v.remove("ateliet-opacity") {
                        e.set("opacity", o);
                    }
                }
                if set.is_empty() {
                    self.uncheckpoint();
                    return;
                }
                let mut gone = set;
                for e in &self.board.elements {
                    if e.container().is_some_and(|c| gone.contains(c)) {
                        gone.insert(e.id().to_string());
                    }
                }
                self.remove_elements(&gone);
                self.sel.retain(|s| !matches!(s, Sel::El(i) if gone.contains(i)));
                self.changed();
            }
        }
    }

    /// Scale the selection from its box at the start of the drag.
    fn resize(&mut self, handle: usize, b0: (f32, f32, f32, f32), w: P, orig: &ateliet_core::quadro::Board, keep: bool) {
        let (x0, y0, x1, y1) = b0;
        let (mut nx0, mut ny0, mut nx1, mut ny1) = b0;
        if matches!(handle, 0 | 6 | 7) {
            nx0 = w.0.min(x1 - 4.0);
        }
        if matches!(handle, 2..=4) {
            nx1 = w.0.max(x0 + 4.0);
        }
        if matches!(handle, 0..=2) {
            ny0 = w.1.min(y1 - 4.0);
        }
        if matches!(handle, 4..=6) {
            ny1 = w.1.max(y0 + 4.0);
        }
        let mut sx = (nx1 - nx0) / (x1 - x0).max(0.001);
        let mut sy = (ny1 - ny0) / (y1 - y0).max(0.001);
        if keep && matches!(handle, 0 | 2 | 4 | 6) {
            let s = sx.max(sy);
            sx = s;
            sy = s;
            if matches!(handle, 0 | 6) {
                nx0 = nx1 - (x1 - x0) * s;
            } else {
                nx1 = nx0 + (x1 - x0) * s;
            }
            if matches!(handle, 0 | 2) {
                ny0 = ny1 - (y1 - y0) * s;
            } else {
                ny1 = ny0 + (y1 - y0) * s;
            }
        }
        let _ = (nx1, ny1);
        let fx = |x: f32| nx0 + (x - x0) * sx;
        let fy = |y: f32| ny0 + (y - y0) * sy;
        let single_text = matches!(self.sel.as_slice(), [Sel::El(id)] if orig.element(id).is_some_and(|e| e.kind() == "text"));
        let ids = {
            let mut ids = self.sel_els();
            for e in &orig.elements {
                if e.container().is_some_and(|c| ids.iter().any(|i| i == c)) {
                    ids.push(e.id().to_string());
                }
            }
            ids
        };
        for id in &ids {
            let Some(o) = orig.element(id) else { continue };
            let mut e = o.clone();
            if e.is_linear() {
                let pts: Vec<P> = o.abs_points().iter().map(|p| (fx(p.0), fy(p.1))).collect();
                e.set_abs_points(&pts);
            } else if e.container().is_none() || !ids.iter().any(|i| Some(i.as_str()) == e.container()) {
                e.set_f("x", fx(o.x()));
                e.set_f("y", fy(o.y()));
                e.set_f("width", o.w() * sx);
                e.set_f("height", o.h() * sy);
                if single_text {
                    e.set_f("fontSize", (o.fo("fontSize", 20.0) * sy).max(4.0));
                }
            }
            e.rev = ateliet_core::quadro::element::next_rev();
            e.touched = true;
            if let Some(slot) = self.board.element_mut(id) {
                *slot = e;
            }
        }
        let sel = self.sel.clone();
        for s in &sel {
            match s {
                Sel::Card(id) => {
                    if let (Some(o), Some(c)) = (orig.cards.iter().find(|c| &c.id == id), self.board.cards.iter_mut().find(|c| &c.id == id)) {
                        c.x = fx(o.x);
                        c.y = fy(o.y);
                        c.w = (o.w * sx).max(140.0);
                    }
                }
                Sel::Lane(id) => {
                    if let (Some(o), Some(l)) = (orig.lanes.iter().find(|l| &l.id == id), self.board.lanes.iter_mut().find(|l| &l.id == id)) {
                        l.x = fx(o.x);
                        l.y = fy(o.y);
                        l.w = (o.w * sx).max(160.0);
                    }
                }
                Sel::El(_) => {}
            }
        }
        let containers: Vec<String> = self.sel_els();
        for c in containers {
            self.center_bound_text(&c);
        }
        self.changed();
    }

    pub(super) fn select_all(&mut self) {
        let mut s: Vec<Sel> = self.board.elements.iter().filter(|e| !e.deleted() && e.container().is_none()).map(|e| Sel::El(e.id().to_string())).collect();
        s.extend(self.board.lanes.iter().map(|l| Sel::Lane(l.id.clone())));
        s.extend(self.board.cards.iter().map(|c| Sel::Card(c.id.clone())));
        self.sel = s;
    }

    pub(super) fn keys(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let ctx = ui.ctx().clone();
        let cmd = |k| ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, k)));
        let cmd_shift = |k| ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, k)));
        if cmd_shift(egui::Key::Z) || cmd(egui::Key::Y) {
            self.redo();
        } else if cmd(egui::Key::Z) {
            self.undo();
        }
        if cmd(egui::Key::A) {
            self.select_all();
        }
        if cmd(egui::Key::D) {
            self.duplicate(cx);
        }
        if cmd_shift(egui::Key::G) {
            self.group(false);
        } else if cmd(egui::Key::G) {
            self.group(true);
        }
        if cmd_shift(egui::Key::CloseBracket) {
            self.reorder(true);
        }
        if cmd_shift(egui::Key::OpenBracket) {
            self.reorder(false);
        }
        // copy, cut, paste (the window turns Ctrl+C/X/V into these events)
        let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
        let mut pasted_text = false;
        for e in &events {
            match e {
                egui::Event::Copy | egui::Event::Cut => {
                    if let Some(j) = self.copy_json() {
                        ctx.copy_text(j);
                        if matches!(e, egui::Event::Cut) {
                            self.delete_sel();
                        }
                    }
                }
                egui::Event::Paste(t) if !t.is_empty() => {
                    pasted_text = true;
                    let at = self.paste_point();
                    self.paste_text(cx, &ctx, t, at);
                }
                _ => {}
            }
        }
        let paste_key = events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } if modifiers.command) || matches!(e, egui::Event::Paste(t) if t.is_empty()));
        if paste_key && !pasted_text {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                if let Ok(img) = cb.get_image() {
                    let png = ateliet_core::board::tile::encode_png(&img.bytes, img.width as u32, img.height as u32);
                    let at = self.paste_point();
                    self.paste_picture(cx, &ctx, png, at);
                }
            }
        }
        let plain = |k| ui.input(|i| i.key_pressed(k) && i.modifiers.is_none());
        if plain(egui::Key::Delete) || plain(egui::Key::Backspace) {
            self.delete_sel();
        }
        if plain(egui::Key::Escape) {
            self.sel.clear();
            self.menu = None;
            self.tool = Tool::Select;
        }
        if plain(egui::Key::Enter) {
            match self.sel.as_slice() {
                [Sel::Card(id)] => {
                    let id = id.clone();
                    self.edit_card(&id, false);
                }
                [Sel::Lane(id)] => {
                    let id = id.clone();
                    self.edit_lane(&id, false);
                }
                [Sel::El(id)] => {
                    let id = id.clone();
                    match self.board.element(&id).map(|e| e.kind().to_string()).as_deref() {
                        Some("text") => self.edit_text(&id, false),
                        Some("rectangle" | "ellipse" | "diamond") => self.edit_inside(&ctx, &id),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Num1) && i.modifiers.shift && !i.modifiers.command) {
            self.fit = true;
        }
        // nudge
        let step = if ui.input(|i| i.modifiers.shift) { 10.0 } else { 1.0 };
        for (k, d) in [(egui::Key::ArrowLeft, (-step, 0.0)), (egui::Key::ArrowRight, (step, 0.0)), (egui::Key::ArrowUp, (0.0, -step)), (egui::Key::ArrowDown, (0.0, step))] {
            if ui.input(|i| i.key_pressed(k) && !i.modifiers.command && !i.modifiers.alt) && !self.sel.is_empty() {
                self.checkpoint();
                self.move_sel(d.0, d.1);
                self.unbind_moved_arrows();
            }
        }
        let tools: &[(egui::Key, egui::Key, Tool)] = &[
            (egui::Key::V, egui::Key::Num1, Tool::Select),
            (egui::Key::R, egui::Key::Num2, Tool::Rect),
            (egui::Key::D, egui::Key::Num3, Tool::Diamond),
            (egui::Key::O, egui::Key::Num4, Tool::Ellipse),
            (egui::Key::A, egui::Key::Num5, Tool::Arrow),
            (egui::Key::L, egui::Key::Num6, Tool::Line),
            (egui::Key::P, egui::Key::Num7, Tool::Draw),
            (egui::Key::T, egui::Key::Num8, Tool::Text),
            (egui::Key::E, egui::Key::Num0, Tool::Eraser),
            (egui::Key::H, egui::Key::H, Tool::Hand),
        ];
        for (a, b, t) in tools {
            if plain(*a) || plain(*b) {
                self.tool = *t;
            }
        }
        if self.is_kanban() {
            if plain(egui::Key::C) {
                self.tool = Tool::Card;
            }
            if plain(egui::Key::K) {
                self.tool = Tool::Lane;
            }
        }
        if plain(egui::Key::Q) {
            self.lock = !self.lock;
        }
        if plain(egui::Key::I) {
            self.insert = Some(String::new());
        }
    }

    /// Where pasted things go: the pointer, else the middle of what is on screen.
    fn paste_point(&self) -> P {
        if self.hover.is_some() {
            self.last_pointer_world
        } else {
            self.selection_box().map(|b| (b.2 + 40.0, b.1)).unwrap_or(self.last_pointer_world)
        }
    }

    /// Commit the text being typed.
    pub(super) fn commit_edit(&mut self) {
        let Some(ed) = self.editing.take() else { return };
        match ed {
            Editing::Card { id, buf, new } => {
                let buf = buf.trim().to_string();
                let old = self.board.card(&id).map(|c| c.text.clone());
                if buf.is_empty() && (new || old.as_deref() == Some("")) {
                    // an empty new card is not kept
                    self.board.cards.retain(|c| c.id != id);
                    for l in self.board.lanes.iter_mut() {
                        l.cards.retain(|c| c.id != id);
                    }
                    self.sel.retain(|s| s != &Sel::Card(id.clone()));
                    self.changed();
                    return;
                }
                if old.as_deref() != Some(buf.as_str()) {
                    if !new {
                        self.checkpoint();
                    }
                    if let Some(c) = self.board.card_mut(&id) {
                        c.text = buf;
                    }
                    self.changed();
                }
            }
            Editing::Lane { id, buf, new } => {
                let t = buf.trim().replace('\n', " ");
                let t = if t.is_empty() { "Coluna".to_string() } else { t };
                if self.board.lanes.iter().find(|l| l.id == id).is_some_and(|l| l.title != t) {
                    if !new {
                        self.checkpoint();
                    }
                    if let Some(l) = self.board.lanes.iter_mut().find(|l| l.id == id) {
                        l.title = t;
                    }
                    self.changed();
                }
            }
            Editing::Text { id, buf, new, orig } => {
                if buf.trim().is_empty() {
                    let mut gone = std::collections::HashSet::new();
                    gone.insert(id.clone());
                    self.remove_elements(&gone);
                    // the shape it sat in forgets it
                    for e in self.board.elements.iter_mut() {
                        e.remove_bound(&id);
                    }
                    self.sel.retain(|s| s != &Sel::El(id.clone()));
                    if new {
                        self.uncheckpoint();
                    }
                    self.changed();
                    return;
                }
                if self.board.element(&id).is_some_and(|e| e.raw_text() != buf) {
                    if let Some(e) = self.board.element_mut(&id) {
                        e.set_text(&buf);
                    }
                    self.changed();
                }
                if !new && buf == orig {
                    self.uncheckpoint();
                } else {
                    self.changed();
                }
                self.pending_fit = Some(id);
            }
        }
    }

    /// The text field over what is being typed.
    pub(super) fn edit_overlay(&mut self, ui: &mut egui::Ui, v: &View, p: crate::theme::Palette) {
        let ctx = ui.ctx().clone();
        if let Some(id) = self.pending_fit.take() {
            self.fit_text(&ctx, &id);
        }
        let Some(ed) = self.editing.as_mut() else { return };
        let (rect, size, multiline_enter, align, color) = match ed {
            Editing::Card { id, .. } => {
                let Some(cb) = self.scene.card(id) else {
                    self.editing = None;
                    return;
                };
                let r = draw::wr(draw::rr(cb.rect), v).shrink(6.0 * v.zoom.min(1.0));
                (r, (14.0 * v.zoom).clamp(8.0, 40.0), false, egui::Align::LEFT, p.text)
            }
            Editing::Lane { id, .. } => {
                let Some(lb) = self.scene.lanes.iter().find(|l| &l.id == id) else {
                    self.editing = None;
                    return;
                };
                let r = Rect::from_min_size(v.s((lb.rect.0 + 8.0, lb.rect.1 + 8.0)), vec2((lb.rect.2 - 100.0).max(60.0) * v.zoom, 28.0 * v.zoom));
                (r, (14.0 * v.zoom).clamp(8.0, 40.0), false, egui::Align::LEFT, p.text)
            }
            Editing::Text { id, .. } => {
                let Some(e) = self.board.element(id) else {
                    self.editing = None;
                    return;
                };
                let (x0, y0, x1, y1) = e.bounds();
                let size = (e.fo("fontSize", 20.0) * v.zoom).clamp(6.0, 120.0);
                let min_w = if e.container().is_some() { (x1 - x0) * v.zoom } else { ((x1 - x0) * v.zoom).max(size * 4.0) };
                let r = Rect::from_min_size(v.s((x0, y0)), vec2(min_w + size, ((y1 - y0) * v.zoom).max(size * 1.3)));
                let align = match e.s("textAlign") {
                    "center" => egui::Align::Center,
                    "right" => egui::Align::RIGHT,
                    _ => egui::Align::LEFT,
                };
                let c = ateliet_core::quadro::rough::color(e.s("strokeColor")).unwrap_or([30, 30, 30, 255]);
                (r, size, true, align, draw::ink(c, p.dark))
            }
        };
        let buf = match ed {
            Editing::Card { buf, .. } | Editing::Lane { buf, .. } | Editing::Text { buf, .. } => buf,
        };
        let focus = std::mem::take(&mut self.focus_edit);
        let mut done = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::new("quadro-edit")).fixed_pos(rect.min).order(egui::Order::Foreground).show(&ctx, |ui| {
            // Enter ends a card or a title (Shift+Enter: a new line); a text takes Enter
            let enter = !multiline_enter && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
            if enter {
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            }
            let te = egui::TextEdit::multiline(buf)
                .font(egui::FontId::proportional(size))
                .text_color(color)
                .horizontal_align(align)
                .desired_width(rect.width())
                .desired_rows(1)
                .frame(false)
                .margin(egui::Margin::ZERO)
                .background_color(egui::Color32::TRANSPARENT);
            let r = egui::Frame::new().fill(if multiline_enter { egui::Color32::TRANSPARENT } else { draw::lane_colors(p).1 }).show(ui, |ui| ui.add(te)).inner;
            if focus {
                r.request_focus();
            }
            if enter {
                done = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                if multiline_enter {
                    done = true;
                } else {
                    cancel = true;
                }
            }
            if r.lost_focus() && !focus {
                done = true;
            }
        });
        if cancel {
            let ed = self.editing.take();
            if let Some(Editing::Card { id, new: true, .. }) = ed {
                self.editing = Some(Editing::Card { id, buf: String::new(), new: true });
                self.commit_edit();
            }
        } else if done {
            self.commit_edit();
        }
        // a text grows while it is typed
        if let Some(Editing::Text { id, buf, .. }) = &self.editing {
            let (id, buf) = (id.clone(), buf.clone());
            if self.board.element(&id).is_some_and(|e| e.raw_text() != buf) {
                if let Some(e) = self.board.element_mut(&id) {
                    e.set_text(&buf);
                }
                self.fit_text(&ctx, &id);
                self.changed();
            }
        }
    }
}
