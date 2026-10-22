//! Focus mode (Tab): only the drawing and the reference, the panels gone.
//! The reference goes beside or under the drawing (automatic: wherever the
//! two come out the same size on screen), is pinned to a side, or floats
//! over the drawing as a small window. A slim bar at the top, hidden while
//! a stroke is drawn, keeps what is needed at hand. The choices are kept in
//! the storyboard.

use super::*;

#[derive(Default)]
pub(super) struct FocusState {
    pub on: bool,
    pub hide_timeline: bool,
    /// Dragging the divider.
    split_drag: bool,
    /// Moving (0) or resizing (1) the floating reference: start pointer, start rect.
    float_drag: Option<(u8, egui::Pos2, egui::Rect)>,
}

const PLACES: [(&str, &str); 6] = [("auto", "Automático"), ("right", "À direita"), ("left", "À esquerda"), ("bottom", "Embaixo"), ("top", "Em cima"), ("float", "Janela flutuante")];

impl BoardEditor {
    fn view(&self) -> &Value {
        &self.doc.board["view"]
    }

    fn set_view(&mut self, k: &str, v: Value) {
        if !self.doc.board["view"].is_object() {
            self.doc.board["view"] = json!({});
        }
        self.doc.board["view"][k] = v;
        self.dirty = true;
    }

    fn ref_place(&self) -> String {
        self.view()["refPlace"].as_str().unwrap_or("auto").to_string()
    }

    /// The shown reference's proportion (height / width).
    fn ref_aspect(&mut self, ctx: &egui::Context) -> Option<f32> {
        let (_, r) = self.shown_ref()?;
        let (_, size) = self.ref_texture(ctx, &r)?;
        Some(size.y / size.x.max(1.0))
    }

    /// Where the reference goes in `area` and what share it takes when the
    /// drawing and the picture come out the same size: (side, share).
    fn auto_split(&self, area: egui::Rect, ra: f32) -> (&'static str, f32) {
        let fa = self.doc.height() as f32 / self.doc.width().max(1) as f32;
        let (w, h) = (area.width(), area.height());
        // side by side: both as tall as h_side
        let h_side = h.min(w / (1.0 / fa + 1.0 / ra));
        let side_share = (1.0 / ra) / (1.0 / fa + 1.0 / ra);
        // stacked: both as wide as w_stack
        let w_stack = w.min(h / (fa + ra));
        let stack_share = ra / (fa + ra);
        // the larger drawing wins
        if h_side / fa >= w_stack { ("right", side_share) } else { ("bottom", stack_share) }
    }

    /// The reference picture in `rect`: fitted, with wheel zoom, drag and
    /// the drawing's grid over it.
    fn ref_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect, p: crate::theme::Palette) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, p.bg_secondary);
        let Some((i, r)) = self.shown_ref() else {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Sem referência: adicione na barra lateral", egui::FontId::proportional(12.0), p.faint);
            return;
        };
        let id = r["id"].as_str().unwrap_or("").to_string();
        if self.rf.view_of != id {
            self.rf.view_of = id.clone();
            self.rf.zoom = 1.0;
            self.rf.pan = egui::Vec2::ZERO;
        }
        let Some((tex, size)) = self.ref_texture(ui.ctx(), &r) else { return };
        let resp = ui.interact(rect, ui.id().with("ref-view"), Sense::click_and_drag());
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let z0 = self.rf.zoom.max(0.1);
                let z1 = (z0 * (1.0 + scroll * 0.0025)).clamp(0.2, 12.0);
                if let Some(ptr) = resp.hover_pos() {
                    // zoom at the pointer
                    let c = rect.center() + self.rf.pan;
                    self.rf.pan += (ptr - c) * (1.0 - z1 / z0);
                }
                self.rf.zoom = z1;
            }
        }
        if resp.dragged() {
            self.rf.pan += resp.drag_delta();
        }
        if resp.double_clicked() {
            self.rf.zoom = 1.0;
            self.rf.pan = egui::Vec2::ZERO;
        }
        let k = (rect.width() / size.x).min(rect.height() / size.y) * self.rf.zoom.max(0.1);
        let ir = egui::Rect::from_center_size(rect.center() + self.rf.pan, size * k);
        painter.image(tex, ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        // the same grid as on the drawing, to draw by the grid method
        let n = self.doc.board["guides"]["grid"].as_u64().unwrap_or(0);
        if n >= 2 {
            let cell = ir.width().min(ir.height()) / n as f32;
            let st = egui::Stroke::new(1.0_f32, Color32::from_black_alpha(60));
            let mut x = ir.left() + cell;
            while x < ir.right() - 0.5 {
                painter.vline(x, ir.y_range(), st);
                x += cell;
            }
            let mut y = ir.top() + cell;
            while y < ir.bottom() - 0.5 {
                painter.hline(ir.x_range(), y, st);
                y += cell;
            }
        }
        let n_refs = self.references().len();
        painter.text(rect.left_bottom() + egui::vec2(8.0, -6.0), egui::Align2::LEFT_BOTTOM, format!("{} / {}  ·  Alt+← →", i + 1, n_refs), egui::FontId::proportional(11.0), p.faint);
    }

    /// The whole editor in focus mode.
    pub(super) fn focus_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) -> Option<String> {
        let mut toast = None;
        let full = ui.available_rect_before_wrap();
        let strip_h = if self.has_timeline() && !self.fx.hide_timeline { 104.0 } else { 0.0 };
        let area = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.max.y - strip_h));
        let place = self.ref_place();
        let has_ref = self.doc.kind() != "canvas" && !self.references().is_empty();
        let ra = self.ref_aspect(ui.ctx()).unwrap_or(0.75);
        let (side, fit_share) = match place.as_str() {
            "auto" => self.auto_split(area, ra),
            "right" | "left" => ("x", self.auto_split(area, ra).1),
            "float" => ("float", 0.0),
            _ => ("y", self.auto_split(area, ra).1),
        };
        let side = match (place.as_str(), side) {
            ("auto", s) => s.to_string(),
            ("float", _) => "float".to_string(),
            (pl, _) => pl.to_string(),
        };
        let share = self.view()["refSplit"].as_f64().map(|v| v as f32).unwrap_or(fit_share).clamp(0.1, 0.9);
        let horizontal = matches!(side.as_str(), "right" | "left");
        let (canvas, refr, divider) = if !has_ref || side == "float" {
            (area, None, None)
        } else {
            let d = 6.0;
            let total = if horizontal { area.width() } else { area.height() } - d;
            let rs = total * share;
            let (c, r, dv) = match side.as_str() {
                "right" => (egui::Rect::from_min_max(area.min, egui::pos2(area.max.x - rs - d, area.max.y)), egui::Rect::from_min_max(egui::pos2(area.max.x - rs, area.min.y), area.max), egui::Rect::from_min_max(egui::pos2(area.max.x - rs - d, area.min.y), egui::pos2(area.max.x - rs, area.max.y))),
                "left" => (egui::Rect::from_min_max(egui::pos2(area.min.x + rs + d, area.min.y), area.max), egui::Rect::from_min_max(area.min, egui::pos2(area.min.x + rs, area.max.y)), egui::Rect::from_min_max(egui::pos2(area.min.x + rs, area.min.y), egui::pos2(area.min.x + rs + d, area.max.y))),
                "top" => (egui::Rect::from_min_max(egui::pos2(area.min.x, area.min.y + rs + d), area.max), egui::Rect::from_min_max(area.min, egui::pos2(area.max.x, area.min.y + rs)), egui::Rect::from_min_max(egui::pos2(area.min.x, area.min.y + rs), egui::pos2(area.max.x, area.min.y + rs + d))),
                _ => (egui::Rect::from_min_max(area.min, egui::pos2(area.max.x, area.max.y - rs - d)), egui::Rect::from_min_max(egui::pos2(area.min.x, area.max.y - rs), area.max), egui::Rect::from_min_max(egui::pos2(area.min.x, area.max.y - rs - d), egui::pos2(area.max.x, area.max.y - rs))),
            };
            (c, Some(r), Some((dv, total, fit_share)))
        };
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(canvas), |ui| {
            if let Some(t) = self.canvas_ui(ui, p) {
                toast = Some(t);
            }
        });
        if let Some(r) = refr {
            self.ref_view(ui, r, p);
        }
        // the divider: drag to size, it snaps to the fit and to thirds and half; double click fits again
        if let Some((dv, total, fit)) = divider {
            let resp = ui.interact(dv, ui.id().with("focus-divider"), Sense::click_and_drag());
            let hot = resp.hovered() || resp.dragged();
            ui.painter().rect_filled(dv, 0.0, if hot { p.border_strong } else { p.border });
            if hot {
                ui.ctx().set_cursor_icon(if horizontal { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
            }
            if resp.dragged() {
                self.fx.split_drag = true;
                if let Some(ptr) = resp.interact_pointer_pos() {
                    let rs = match side.as_str() {
                        "right" => area.max.x - ptr.x,
                        "left" => ptr.x - area.min.x,
                        "top" => ptr.y - area.min.y,
                        _ => area.max.y - ptr.y,
                    };
                    let mut s = (rs / total).clamp(0.1, 0.9);
                    for snap in [fit, 1.0 / 3.0, 0.5, 2.0 / 3.0] {
                        if (s - snap).abs() < 0.02 {
                            s = snap;
                        }
                    }
                    self.set_view("refSplit", json!(s));
                    self.fit = true;
                }
            } else {
                self.fx.split_drag = false;
            }
            if resp.double_clicked() {
                self.set_view("refSplit", Value::Null);
                self.fit = true;
            }
        }
        if has_ref && side == "float" {
            self.float_ref(ui, canvas, ra, p);
        }
        if strip_h > 0.0 {
            let s = egui::Rect::from_min_max(egui::pos2(full.min.x, full.max.y - strip_h), full.max);
            ui.painter().rect_filled(s, 0.0, p.bg);
            ui.painter().hline(s.x_range(), s.top() + 0.5, egui::Stroke::new(1.0_f32, p.border));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(s.shrink2(egui::vec2(10.0, 6.0))), |ui| self.timeline_ui(ui, p));
        }
        // the slim bar, out of the way while drawing
        if self.active.is_none() {
            self.focus_bar(ui, canvas, p);
        }
        toast
    }

    /// The reference as a small window over the drawing.
    fn float_ref(&mut self, ui: &mut egui::Ui, area: egui::Rect, ra: f32, p: crate::theme::Palette) {
        let title_h = 22.0;
        let stored = self.view()["refFloat"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect::<Vec<_>>());
        let free = self.view()["refFloatFree"].as_bool().unwrap_or(false);
        let mut r = match stored {
            Some(v) if v.len() == 4 => egui::Rect::from_min_size(area.min + egui::vec2(v[0] * area.width(), v[1] * area.height()), egui::vec2(v[2] * area.width(), v[3] * area.height())),
            _ => {
                let w = (area.width() * 0.3).min(area.height() * 0.6 / ra.max(0.1));
                egui::Rect::from_min_size(egui::pos2(area.max.x - w - 16.0, area.min.y + 52.0), egui::vec2(w, w * ra + title_h))
            }
        };
        if !free {
            // the window takes the picture's shape until resized by hand
            r.set_height(r.width() * ra + title_h);
        }
        let title = egui::Rect::from_min_size(r.min, egui::vec2(r.width(), title_h));
        let corner = egui::Rect::from_min_size(r.max - egui::vec2(14.0, 14.0), egui::vec2(14.0, 14.0));
        let tr = ui.interact(title, ui.id().with("float-title"), Sense::click_and_drag());
        let cr = ui.interact(corner, ui.id().with("float-corner"), Sense::drag());
        if tr.drag_started() {
            self.fx.float_drag = Some((0, tr.interact_pointer_pos().unwrap_or(r.min), r));
        }
        if cr.drag_started() {
            self.fx.float_drag = Some((1, cr.interact_pointer_pos().unwrap_or(r.max), r));
        }
        let mut changed = false;
        if let Some((kind, start, r0)) = self.fx.float_drag {
            if let Some(ptr) = ui.input(|i| i.pointer.latest_pos()) {
                let d = ptr - start;
                r = if kind == 0 {
                    let mut m = r0.translate(d);
                    // stick to the edges
                    for (edge, near) in [(area.left(), m.left()), (area.right() - m.width(), m.left())] {
                        if (near - edge).abs() < 12.0 {
                            m = m.translate(egui::vec2(edge - near, 0.0));
                        }
                    }
                    for (edge, near) in [(area.top(), m.top()), (area.bottom() - m.height(), m.top())] {
                        if (near - edge).abs() < 12.0 {
                            m = m.translate(egui::vec2(0.0, edge - near));
                        }
                    }
                    m
                } else {
                    egui::Rect::from_min_size(r0.min, (r0.size() + d).max(egui::vec2(120.0, 90.0)))
                };
                changed = true;
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                self.fx.float_drag = None;
                if kind == 1 {
                    self.set_view("refFloatFree", json!(true));
                }
            }
        }
        if tr.double_clicked() {
            self.set_view("refFloatFree", json!(false));
            r.set_height(r.width() * ra + title_h);
            changed = true;
        }
        if changed {
            let v = [(r.min.x - area.min.x) / area.width(), (r.min.y - area.min.y) / area.height(), r.width() / area.width(), r.height() / area.height()];
            self.set_view("refFloat", json!(v));
        }
        let painter = ui.painter();
        painter.add(egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: Color32::from_black_alpha(60) }.as_shape(r, 8.0));
        painter.rect_filled(r, 8.0, p.bg);
        painter.text(title.left_center() + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, "Referência", egui::FontId::proportional(11.5), p.muted);
        let body = egui::Rect::from_min_max(egui::pos2(r.min.x + 2.0, title.max.y), r.max - egui::vec2(2.0, 2.0));
        self.ref_view(ui, body, p);
        ui.painter().text(corner.center(), egui::Align2::CENTER_CENTER, "◢", egui::FontId::proportional(10.0), p.faint);
        ui.painter().rect_stroke(r, 8.0, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
    }

    fn focus_bar(&mut self, ui: &mut egui::Ui, canvas: egui::Rect, p: crate::theme::Palette) {
        let w = 520.0_f32.min(canvas.width() - 16.0);
        let rect = egui::Rect::from_min_size(egui::pos2(canvas.center().x - w / 2.0, canvas.top() + 8.0), egui::vec2(w, 36.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            egui::Frame::new().fill(p.bg).corner_radius(10.0).stroke(egui::Stroke::new(1.0_f32, p.border)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if kit::icon_button(ui, icons::UNDO_2, "Desfazer (Ctrl+Z)", false, p).clicked() {
                        self.undo();
                    }
                    if let Some(k) = self.brush() {
                        let s = self.settings.get_mut(&k).unwrap();
                        ui.spacing_mut().slider_width = 70.0;
                        ui.add(egui::Slider::new(&mut s.size, 1.0..=300.0).logarithmic(true).show_value(false)).on_hover_text("Tamanho ([ e ])");
                    }
                    let traces = self.doc.frame_layers(self.frame).into_iter().filter(|id| self.doc.layer(id)["kind"] == "trace").collect::<Vec<_>>();
                    if !traces.is_empty() {
                        let on = traces.iter().any(|id| self.doc.layer(id)["visible"].as_bool().unwrap_or(true));
                        if kit::icon_button(ui, icons::SCAN_SEARCH, "Decalque (T)", on, p).clicked() {
                            for id in &traces {
                                self.doc.layer_mut(id)["visible"] = json!(!on);
                            }
                            self.doc.mark_frame(self.frame);
                            self.invalidate();
                            self.dirty = true;
                        }
                    }
                    if self.has_timeline() {
                        let onion = self.doc.board["onion"]["enabled"].as_bool().unwrap_or(false);
                        if kit::icon_button(ui, icons::LAYERS_2, "Papel de cebola (O)", onion, p).clicked() {
                            self.doc.board["onion"]["enabled"] = json!(!onion);
                            self.invalidate();
                            self.dirty = true;
                        }
                    }
                    if !self.is_pages() && kit::icon_button(ui, icons::FLIP_HORIZONTAL_2, "Espelhar a vista (F)", self.mirror, p).clicked() {
                        self.mirror = !self.mirror;
                        self.invalidate();
                    }
                    ui.add_space(4.0);
                    if kit::icon_button(ui, icons::CHEVRON_LEFT, "Quadro anterior (←)", false, p).clicked() && self.frame > 0 {
                        self.frame -= 1;
                        self.invalidate();
                    }
                    ui.label(RichText::new(format!("{}/{}", self.frame + 1, self.doc.frame_count())).size(11.5).color(p.faint));
                    if kit::icon_button(ui, icons::CHEVRON_RIGHT, "Quadro seguinte (→)", false, p).clicked() && self.frame + 1 < self.doc.frame_count() {
                        self.frame += 1;
                        self.invalidate();
                    }
                    if kit::icon_button(ui, icons::PLUS, "Quadro novo depois deste", false, p).clicked() {
                        let f = self.frame;
                        let mut new = 0;
                        self.structure(|d| new = ateliet_core::board::pages::add_page(d, f + 1, "one"));
                        self.frame = new;
                        self.invalidate();
                    }
                    ui.add_space(4.0);
                    let pr = kit::icon_button(ui, icons::PANELS_TOP_LEFT, "Onde fica a referência", false, p);
                    let pid = ui.make_persistent_id("focus-place");
                    if pr.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(pid));
                    }
                    let cur = self.ref_place();
                    egui::popup_below_widget(ui, pid, &pr, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(170.0);
                        for (k, l) in PLACES {
                            if ui.selectable_label(cur == k, l).clicked() {
                                self.set_view("refPlace", json!(k));
                                self.set_view("refSplit", Value::Null);
                                self.fit = true;
                            }
                        }
                    });
                    let fs = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    if kit::icon_button(ui, icons::MAXIMIZE_2, "Tela cheia do sistema", fs, p).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
                    }
                    if kit::icon_button(ui, icons::X, "Sair do modo foco (Esc ou Tab)", false, p).clicked() {
                        self.fx.on = false;
                        self.fit = true;
                    }
                });
            });
        });
    }

    /// Tab, Shift+Tab and Esc.
    pub(super) fn focus_keys(&mut self, ui: &egui::Ui) {
        let t = self.tab_pressed.take();
        let (tab, shift_tab) = (t == Some(false), t == Some(true));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if tab {
            self.fx.on = !self.fx.on;
            self.fit = true;
        }
        if shift_tab && self.fx.on {
            self.fx.hide_timeline = !self.fx.hide_timeline;
            self.fit = true;
        }
        if esc && self.fx.on && self.ts.float.is_none() && self.ts.sel.is_none() {
            self.fx.on = false;
            self.fit = true;
        }
    }
}
