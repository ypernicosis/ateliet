//! The timeline of an animatic (and the presentation board's strip): frames
//! as clips as wide as their duration, a ruler in seconds and the playhead;
//! drag a clip's right edge to change its length, drag the clip to move it.
//! Also the guides drawn over the frame and the onion skin's settings.

use super::*;

pub(super) struct TimelineState {
    /// Pixels a frame of time (at the board's fps).
    pub px: f32,
    drag: Option<TDrag>,
}

impl Default for TimelineState {
    fn default() -> Self {
        TimelineState { px: 2.5, drag: None }
    }
}

enum TDrag {
    Edge { i: usize, start_x: f32, orig: f32, before: Value },
    Move { i: usize, to: usize },
}

fn dur(v: &Value) -> f32 {
    v["duration"].as_f64().unwrap_or(24.0) as f32
}

impl BoardEditor {
    pub(super) fn has_timeline(&self) -> bool {
        matches!(self.doc.kind(), "animatic" | "presentation")
    }

    /// The time where frame `i` starts (in frames).
    fn start_of(&self, i: usize) -> f32 {
        (0..i).map(|k| dur(self.doc.frame(k))).sum()
    }

    pub(super) fn timeline_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let n = self.doc.frame_count();
        let fps = self.doc.fps().max(1) as f32;
        let clock = self.doc.kind() == "animatic";
        let px = if clock { self.tl.px } else { 0.0 };
        let card = 110.0;
        let width_of = |d: f32| if clock { (d * px).max(18.0) } else { card };
        let total: f32 = (0..n).map(|i| width_of(dur(self.doc.frame(i)))).sum::<f32>() + 8.0 * n as f32;
        let h = ui.available_height();
        // zoom the strip with Ctrl + wheel
        if clock && ui.rect_contains_pointer(ui.max_rect()) {
            let (dy, cmd) = ui.input(|i| (i.raw_scroll_delta.y, i.modifiers.command));
            if cmd && dy != 0.0 {
                self.tl.px = (self.tl.px * if dy > 0.0 { 1.12 } else { 1.0 / 1.12 }).clamp(0.4, 20.0);
            }
        }
        egui::ScrollArea::horizontal().id_salt("timeline").auto_shrink([false, false]).show(ui, |ui| {
            let (rect, bg) = ui.allocate_exact_size(egui::vec2(total.max(ui.available_width()), h - 4.0), Sense::click());
            let painter = ui.painter_at(rect);
            let ruler_h = if clock { 16.0 } else { 0.0 };
            // ruler: seconds
            if clock {
                let mut t = 0.0;
                let step = if px * fps < 40.0 { (40.0 / (px * fps)).ceil() } else { 1.0 };
                while t * fps * px < rect.width() {
                    let x = rect.left() + t * fps * px;
                    painter.vline(x, rect.top()..=rect.top() + 6.0, egui::Stroke::new(1.0_f32, p.border_strong));
                    painter.text(egui::pos2(x + 3.0, rect.top()), egui::Align2::LEFT_TOP, format!("{}s", t as i32), egui::FontId::proportional(9.5), p.faint);
                    t += step;
                }
            }
            // clips
            let mut x = rect.left();
            let mut rects = Vec::new();
            for i in 0..n {
                let w = width_of(dur(self.doc.frame(i)));
                let r = egui::Rect::from_min_size(egui::pos2(x, rect.top() + ruler_h + 4.0), egui::vec2(w, rect.height() - ruler_h - 8.0));
                rects.push(r);
                x += w + 8.0;
            }
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let (pressed, down, released) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released()));
            // dragging
            if let (Some(TDrag::Edge { i, start_x, orig, .. }), Some(q)) = (&self.tl.drag, pointer) {
                if down && clock {
                    let d = (orig + (q.x - start_x) / px).round().max(1.0);
                    let (i, d) = (*i, d);
                    self.doc.board["frames"][i]["duration"] = json!(d);
                }
            }
            if let (Some(TDrag::Move { i, .. }), Some(q)) = (&self.tl.drag, pointer) {
                let i = *i;
                let to = rects.iter().position(|r| q.x < r.center().x).unwrap_or(n.saturating_sub(1)).min(n.saturating_sub(1));
                self.tl.drag = Some(TDrag::Move { i, to });
            }
            for (i, r) in rects.iter().enumerate() {
                let id = self.doc.frame_id(i);
                let sel = i == self.frame;
                painter.rect_filled(*r, 6.0, p.bg_hover);
                if let Some((_, tex)) = self.thumbs.get(&id) {
                    let aspect = self.doc.width() as f32 / self.doc.height().max(1) as f32;
                    let ih = (r.height() - 16.0).max(10.0);
                    let iw = (ih * aspect).min(r.width() - 4.0);
                    let ir = egui::Rect::from_min_size(r.min + egui::vec2(2.0, 2.0), egui::vec2(iw, iw / aspect));
                    painter.image(tex.id(), ir, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                if let Some(c) = self.doc.frame(i)["color"].as_str() {
                    let col = ateliet_core::board::tile::parse_color(c);
                    painter.rect_filled(egui::Rect::from_min_size(egui::pos2(r.left(), r.bottom() - 3.0), egui::vec2(r.width(), 3.0)), 0.0, Color32::from_rgb(col[0], col[1], col[2]));
                }
                let label = if clock { format!("{}  {:.1}s", i + 1, dur(self.doc.frame(i)) / fps).replace('.', ",") } else { format!("{}", i + 1) };
                painter.text(egui::pos2(r.left() + 5.0, r.bottom() - 4.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(10.5), if sel { p.text } else { p.muted });
                painter.rect_stroke(*r, 6.0, egui::Stroke::new(if sel { 2.0_f32 } else { 1.0 }, if sel { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
                if let Some(TDrag::Move { i: from, to }) = &self.tl.drag {
                    if *to == i && *from != i {
                        let xx = if to > from { r.right() + 4.0 } else { r.left() - 4.0 };
                        painter.vline(xx, r.y_range(), egui::Stroke::new(2.0_f32, p.accent_strong));
                    }
                }
                // the right edge changes the length
                if clock {
                    let edge = egui::Rect::from_min_max(egui::pos2(r.right() - 5.0, r.top()), egui::pos2(r.right() + 3.0, r.bottom()));
                    if pointer.is_some_and(|q| edge.contains(q)) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                }
            }
            if pressed && bg.hovered() {
                if let Some(q) = pointer {
                    if let Some(i) = rects.iter().position(|r| r.contains(q) || (clock && q.x >= r.right() - 5.0 && q.x <= r.right() + 3.0 && q.y >= r.top() && q.y <= r.bottom())) {
                        let r = rects[i];
                        if clock && q.x >= r.right() - 5.0 {
                            self.tl.drag = Some(TDrag::Edge { i, start_x: q.x, orig: dur(self.doc.frame(i)), before: self.doc.board.clone() });
                        } else {
                            if i != self.frame {
                                self.frame = i;
                                self.invalidate();
                            }
                            self.tl.drag = Some(TDrag::Move { i, to: i });
                        }
                    } else if clock && q.y < rect.top() + ruler_h + 4.0 {
                        // the ruler: go to that time
                        let t = (q.x - rect.left()) / px;
                        let mut acc = 0.0;
                        for i in 0..n {
                            acc += dur(self.doc.frame(i));
                            if t < acc {
                                self.frame = i;
                                self.invalidate();
                                break;
                            }
                        }
                    }
                }
            }
            if released {
                match self.tl.drag.take() {
                    Some(TDrag::Edge { before, .. }) => {
                        if before != self.doc.board {
                            let after = self.doc.board.clone();
                            self.undo.push(Edit::Structure { before, after, tiles: Vec::new() });
                            self.redo.clear();
                            self.changed();
                        }
                    }
                    Some(TDrag::Move { i, to }) if i != to => {
                        self.structure(|d| d.move_frame(i, to));
                        self.frame = to;
                    }
                    _ => {}
                }
            }
            // the playhead while playing
            if clock {
                if let Some((start, from)) = self.playing {
                    let t = self.start_of(from) + start.elapsed().as_secs_f32() * fps;
                    let total_t: f32 = (0..n).map(|i| dur(self.doc.frame(i))).sum::<f32>().max(1.0);
                    let x = rect.left() + (t % total_t) * px;
                    painter.vline(x, rect.y_range(), egui::Stroke::new(1.5_f32, p.error));
                }
            }
        });
    }

    /// Thirds, centre, safe areas and the grid over the frame.
    pub(super) fn draw_guides(&self, clip: &egui::Painter, origin: egui::Pos2) {
        let g = &self.doc.board["guides"];
        let (w, h) = (self.doc.width() as f32 * self.zoom, self.doc.height() as f32 * self.zoom);
        let r = egui::Rect::from_min_size(origin, egui::vec2(w, h));
        let line = egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x2f, 0x80, 0xed, 120));
        if g["thirds"].as_bool().unwrap_or(false) {
            for k in [1.0, 2.0] {
                clip.vline(r.left() + w * k / 3.0, r.y_range(), line);
                clip.hline(r.x_range(), r.top() + h * k / 3.0, line);
            }
        }
        if g["center"].as_bool().unwrap_or(false) {
            let c = r.center();
            clip.line_segment([c - egui::vec2(12.0, 0.0), c + egui::vec2(12.0, 0.0)], line);
            clip.line_segment([c - egui::vec2(0.0, 12.0), c + egui::vec2(0.0, 12.0)], line);
        }
        if g["safe"].as_bool().unwrap_or(false) {
            for k in [0.9, 0.8] {
                let s = egui::Rect::from_center_size(r.center(), r.size() * k);
                clip.rect_stroke(s, 0.0, egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0xff, 0xb3, 0x00, 150)), egui::StrokeKind::Middle);
            }
        }
        let n = g["grid"].as_u64().unwrap_or(0);
        if n >= 2 {
            let cell = w.min(h) / n as f32;
            let mut x = r.left() + cell;
            while x < r.right() - 0.5 {
                clip.vline(x, r.y_range(), egui::Stroke::new(1.0_f32, Color32::from_black_alpha(40)));
                x += cell;
            }
            let mut y = r.top() + cell;
            while y < r.bottom() - 0.5 {
                clip.hline(r.x_range(), y, egui::Stroke::new(1.0_f32, Color32::from_black_alpha(40)));
                y += cell;
            }
        }
    }

    /// The guides' and the onion skin's settings, in popups of the toolbar.
    pub(super) fn guide_menu(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let r = kit::icon_button(ui, icons::LAYOUT_GRID, "Guias: terços, centro, áreas seguras, grade", ["thirds", "center", "safe"].iter().any(|k| self.doc.board["guides"][k].as_bool() == Some(true)) || self.doc.board["guides"]["grid"].as_u64().unwrap_or(0) >= 2, p);
        let id = ui.make_persistent_id("board-guides");
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        egui::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(200.0);
            for (k, label) in [("thirds", "Terços"), ("center", "Centro"), ("safe", "Áreas seguras")] {
                let mut on = self.doc.board["guides"][k].as_bool().unwrap_or(false);
                if ui.checkbox(&mut on, label).changed() {
                    self.doc.board["guides"][k] = json!(on);
                    self.dirty = true;
                }
            }
            let mut n = self.doc.board["guides"]["grid"].as_u64().unwrap_or(0) as u32;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Grade").size(12.0).color(p.muted));
                if ui.add(egui::Slider::new(&mut n, 0..=16).custom_formatter(|v, _| if v < 2.0 { "sem".into() } else { format!("{v:.0}") })).changed() {
                    self.doc.board["guides"]["grid"] = json!(if n < 2 { 0 } else { n });
                    self.dirty = true;
                }
            });
        });
        let onion = self.doc.board["onion"]["enabled"].as_bool().unwrap_or(false);
        let r = kit::icon_button(ui, icons::CHEVRON_DOWN, "Ajustes do papel de cebola", false, p);
        let id = ui.make_persistent_id("board-onion");
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        egui::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(220.0);
            let o = &mut self.doc.board["onion"];
            let mut changed = false;
            let mut on = onion;
            changed |= ui.checkbox(&mut on, "Mostrar quadros vizinhos (O)").changed();
            o["enabled"] = json!(on);
            for (k, label) in [("before", "Antes"), ("after", "Depois")] {
                let mut v = o[k].as_u64().unwrap_or(1) as u32;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).size(12.0).color(p.muted));
                    if ui.add(egui::Slider::new(&mut v, 0..=5)).changed() {
                        o[k] = json!(v);
                        changed = true;
                    }
                });
            }
            let mut op = o["opacity"].as_f64().unwrap_or(0.3) as f32;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Opacidade").size(12.0).color(p.muted));
                if ui.add(egui::Slider::new(&mut op, 0.05..=0.9).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed() {
                    o["opacity"] = json!(op);
                    changed = true;
                }
            });
            for (k, label) in [("tint", "Tingir (antes vermelho, depois verde)"), ("fade", "Esmaecer com a distância"), ("pictures", "Incluir as imagens colocadas")] {
                let mut b = o[k].as_bool().unwrap_or(k != "pictures");
                if ui.checkbox(&mut b, label).changed() {
                    o[k] = json!(b);
                    changed = true;
                }
            }
            if changed {
                self.invalidate();
                self.dirty = true;
            }
        });
    }
}
