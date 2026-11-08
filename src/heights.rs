//! Drawing only what is in view of a long rendered note.
//!
//! A rendered note (Live Preview at rest, the reading view) is a column of
//! pieces drawn one under the other. Each piece's height is remembered by its
//! content, the width and the typography; a piece out of view takes that room
//! without being drawn. A piece not measured yet takes an estimate and is
//! measured out of sight, a few each frame, so a long note opens at once and
//! becomes exact while it is read. When a piece above the view turns out
//! taller or shorter than its estimate, the view is scrolled by the difference
//! on the next frame, together with the new height, so what is in view stays
//! where it is.

use eframe::egui;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a frame may spend measuring pieces out of view.
const BUDGET: Duration = Duration::from_millis(6);

#[derive(Default)]
pub struct Heights {
    known: HashMap<u64, f32>,
    /// How far the view must move on the next frame, when the measured
    /// heights of pieces above it take their place.
    shift: f32,
}

/// One frame's drawing of the pieces, top to bottom.
pub struct Pass<'h> {
    heights: &'h mut Heights,
    clip: egui::Rect,
    style: u64,
    until: Instant,
    /// What the pieces above the view measured this frame add to their room.
    shift: f32,
    /// Some piece is still unmeasured.
    pending: bool,
    seen: Vec<u64>,
}

fn hash_of(v: impl std::hash::Hash) -> u64 {
    egui::ahash::RandomState::with_seeds(7, 11, 13, 17).hash_one(v)
}

impl Heights {
    /// Starts drawing the pieces in `ui`, whose width they share.
    pub fn pass(&mut self, ui: &egui::Ui) -> Pass<'_> {
        let typo = crate::typo::get();
        let style = hash_of((ui.available_width().to_bits(), format!("{typo:?}"), ui.style().spacing.item_spacing.y.to_bits()));
        Pass { heights: self, clip: ui.clip_rect(), style, until: Instant::now() + BUDGET, shift: 0.0, pending: false, seen: Vec::new() }
    }
}

impl Pass<'_> {
    /// Draws one piece, or takes its room when it is out of view. `key` names
    /// its content; `estimate` is its height before it is measured; `always`
    /// draws it wherever it is (a piece scrolled to, a find target). Returns
    /// the piece's rectangle.
    pub fn piece(&mut self, ui: &mut egui::Ui, key: u64, estimate: impl FnOnce() -> f32, always: bool, draw: impl FnOnce(&mut egui::Ui)) -> egui::Rect {
        let key = hash_of((key, self.style));
        self.seen.push(key);
        let top = ui.cursor().min;
        let width = ui.available_width();
        let known = self.heights.known.get(&key).copied();
        let room = known.unwrap_or_else(estimate);
        // a piece above the view is never drawn: its new height would move what is in view
        let in_view = top.y < self.clip.bottom() + self.clip.height() * 0.5 && top.y + room > self.clip.top();
        if always || in_view {
            draw(ui);
            let h = ui.cursor().min.y - top.y;
            self.heights.known.insert(key, h);
            return egui::Rect::from_min_size(top, egui::vec2(width, h));
        }
        if known.is_none() {
            if Instant::now() < self.until {
                let mut sight = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(top, egui::vec2(width, f32::INFINITY))).layout(*ui.layout()).invisible());
                draw(&mut sight);
                let h = sight.cursor().min.y - top.y;
                self.heights.known.insert(key, h);
                if top.y + room <= self.clip.top() {
                    self.shift += h - room;
                }
            } else {
                self.pending = true;
            }
        }
        ui.add_space(room);
        egui::Rect::from_min_size(top, egui::vec2(width, room))
    }

    /// Ends the frame's drawing: keeps the view in place and asks for the
    /// frames that measure what is left.
    pub fn finish(self, ui: &egui::Ui) {
        if self.shift != 0.0 {
            // applied by `keep_view` before the scroll area of the next frame
            self.heights.shift += self.shift;
            ui.ctx().request_repaint();
        }
        if self.pending {
            ui.ctx().request_repaint();
        }
        // heights of pieces long gone (edited, another width) are dropped
        if self.heights.known.len() > self.seen.len() * 3 + 256 {
            let seen: std::collections::HashSet<u64> = self.seen.into_iter().collect();
            self.heights.known.retain(|k, _| seen.contains(k));
        }
    }
}

/// The note's scroll area, moved by what the pieces above the view grew or
/// shrank when they were measured on the frame before (and by `more`, what
/// the editor's lines above the view grew), so what is in view stays put.
/// `salt` is the scroll area's id salt; call before showing it.
pub fn keep_view(ui: &egui::Ui, area: egui::ScrollArea, salt: impl std::hash::Hash, heights: &mut Heights, more: f32) -> egui::ScrollArea {
    let shift = std::mem::take(&mut heights.shift) + more;
    if shift == 0.0 {
        return area;
    }
    let id = ui.make_persistent_id(egui::Id::new(salt));
    let y = egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |s| s.offset.y);
    area.vertical_scroll_offset((y + shift).max(0.0))
}

/// A first guess at a rendered piece's height from its source: its lines,
/// wrapped at the width with an average letter of half the font size.
pub fn estimate(text: &str, size: f32, width: f32) -> f32 {
    let per_row = (width / (size * 0.5)).max(8.0);
    let rows: f32 = text.split('\n').map(|l| (l.chars().count() as f32 / per_row).ceil().max(1.0)).sum();
    rows * size * crate::typo::get().line
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 400 pieces 50 points tall, guessed at 10: the view opens at the
    /// 100th, and must still show it once the pieces above are measured.
    #[test]
    fn measuring_the_pieces_above_keeps_the_view_in_place() {
        let ctx = egui::Context::default();
        let mut heights = Heights::default();
        let mut first_in_view = Vec::new();
        for frame in 0..4 {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut area = keep_view(ui, egui::ScrollArea::vertical().id_salt("note"), "note", &mut heights, 0.0);
                    if frame == 0 {
                        area = area.vertical_scroll_offset(1000.0);
                    }
                    area.show(ui, |ui| {
                        let clip = ui.clip_rect();
                        let mut pass = heights.pass(ui);
                        let mut top = None;
                        for i in 0..400u64 {
                            let r = pass.piece(ui, i, || 10.0, false, |ui| {
                                ui.allocate_exact_size(egui::vec2(100.0, 50.0), egui::Sense::hover());
                            });
                            if top.is_none() && r.bottom() > clip.top() + 1.0 {
                                top = Some((i, r.top() - clip.top()));
                            }
                        }
                        pass.finish(ui);
                        first_in_view.push(top.unwrap());
                    });
                });
            });
        }
        let (piece, _) = first_in_view[0];
        for (i, &(p, dy)) in first_in_view.iter().enumerate().skip(1) {
            assert_eq!(p, piece, "frame {i}: {first_in_view:?}");
            assert!(dy.abs() < 60.0, "frame {i}: {first_in_view:?}");
        }
    }
}
