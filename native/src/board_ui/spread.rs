//! Spreads drawn as one: the two facing pages joined, brushes, fill, lasso
//! and shapes reach across the spine. Each page keeps its own part: after a
//! stroke, what went past this page's edge moves to the matching layer of
//! the facing page, and undo takes both back together. Separated again,
//! nothing drawn is lost.

use super::*;
use ateliet_core::board::paint::{self, Mode, TileChange};

impl BoardEditor {
    /// The id of this spread (its first page), when the storyboard shows spreads.
    fn spread_id(&self) -> Option<String> {
        let (j, _) = self.spread()?;
        Some(self.doc.frame_id(self.frame.min(j)))
    }

    pub(super) fn is_joined(&self) -> bool {
        self.spread_id().is_some_and(|id| self.doc.board["page"]["joined"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(&id))))
    }

    /// The facing page and its offset when this spread is drawn as one.
    pub(super) fn joined_facing(&self) -> Option<(usize, f32)> {
        self.spread().filter(|_| self.is_joined())
    }

    pub(super) fn toggle_joined(&mut self) {
        let Some(id) = self.spread_id() else { return };
        let on = self.is_joined();
        self.structure(|d| {
            let mut list: Vec<Value> = d.board["page"]["joined"].as_array().cloned().unwrap_or_default();
            list.retain(|x| x.as_str() != Some(&id));
            if !on {
                list.push(json!(id));
            }
            d.board["page"]["joined"] = json!(list);
        });
        self.spread_tex = None;
        self.error = Some(if on { "Páginas separadas.".into() } else { "Páginas unidas: os pincéis passam pela lombada.".into() });
    }

    /// The layer on the facing page that receives what crosses the spine:
    /// the one with the same name, else that page's active one, else its top
    /// drawing layer. None for a layer that page also shows (shared).
    fn spread_target(&self, layer: &str, j: usize) -> Option<String> {
        let l = self.doc.layer(layer);
        let ids = self.doc.frame_layers(j);
        if ids.iter().any(|x| x == layer) {
            return None;
        }
        let raster: Vec<&String> = ids.iter().filter(|id| self.doc.layer(id)["kind"].as_str().unwrap_or("raster") == "raster").collect();
        let active = self.doc.active_layer(j);
        let t = raster.iter().find(|id| self.doc.layer(id)["name"] == l["name"]).or_else(|| raster.iter().find(|id| ***id == active)).or(raster.last())?;
        (self.doc.layer(t)["locked"].as_bool() != Some(true)).then(|| (*t).clone())
    }

    /// After a change to `layer`'s tiles on a joined spread: move what lies
    /// past this page's edge to the facing page. The changes to `layer` get
    /// their final state; returns the facing layer's changes, if any.
    fn settle_spread(&mut self, layer: &str, tiles: &mut [TileChange]) -> Option<(String, Vec<TileChange>)> {
        let (j, off) = self.joined_facing()?;
        let target = self.spread_target(layer, j)?;
        let l = self.doc.layer(layer).clone();
        let fl = self.doc.layer(&target).clone();
        let (lx, ly) = (l["x"].as_f64().unwrap_or(0.0).round() as i32, l["y"].as_f64().unwrap_or(0.0).round() as i32);
        let (fx, fy) = (fl["x"].as_f64().unwrap_or(0.0).round() as i32, fl["y"].as_f64().unwrap_or(0.0).round() as i32);
        let (w, h) = (self.doc.width() as i32, self.doc.height() as i32);
        let off = off.round() as i32;
        // past this page's edge, in this layer's coordinates
        let (ox, oy, ow, oh) = ((if off < 0 { off } else { w }) - lx, -ly, off.abs(), h);
        let t = TILE as i32;
        let mode = if fl["alphaLock"].as_bool() == Some(true) { Mode::Atop } else { Mode::Over };
        let mut out: Vec<TileChange> = Vec::new();
        for (k, _, _) in tiles.iter() {
            let (x0, y0) = ((k.0 * t).max(ox), (k.1 * t).max(oy));
            let (x1, y1) = (((k.0 + 1) * t).min(ox + ow), ((k.1 + 1) * t).min(oy + oh));
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let (rw, rh) = ((x1 - x0) as usize, (y1 - y0) as usize);
            let px = paint::read_layer(&mut self.doc, layer, x0, y0, rw, rh);
            if px.chunks_exact(4).all(|p| p[3] == 0) {
                continue;
            }
            // into the facing layer: its coordinates are this layer's + offset − spine − its offset
            for c in paint::apply_image(&mut self.doc, &target, x0 + lx - off - fx, y0 + ly - fy, rw, rh, &px, 1.0, mode) {
                if !out.iter().any(|o| o.0 == c.0) {
                    out.push(c);
                } else if let Some(o) = out.iter_mut().find(|o| o.0 == c.0) {
                    o.2 = c.2;
                }
            }
            paint::write_layer(&mut self.doc, layer, x0, y0, rw, rh, &vec![0u8; rw * rh * 4]);
        }
        for (k, _, after) in tiles.iter_mut() {
            *after = self.doc.tile(layer, k.0, k.1).cloned();
        }
        if out.is_empty() {
            return None;
        }
        self.doc.mark_layer(&target);
        let fid = self.doc.frame_id(j);
        *self.versions.entry(fid).or_default() += 1;
        self.spread_tex = None;
        Some((target, out))
    }

    /// One step of undo for a change to a layer's tiles (and, on a joined
    /// spread, what crossed to the facing page).
    pub(super) fn push_tiles(&mut self, layer: &str, mut tiles: Vec<TileChange>) {
        if tiles.is_empty() {
            return;
        }
        let facing = self.settle_spread(layer, &mut tiles);
        self.doc.mark_layer(layer);
        let edit = Edit::Tiles { layer: layer.to_string(), tiles };
        self.undo.push(match facing {
            Some((t, ch)) => Edit::Group(vec![edit, Edit::Tiles { layer: t, tiles: ch }]),
            None => edit,
        });
        self.redo.clear();
        self.changed();
    }

    /// The box region tools may paint in: the page, or both joined pages.
    pub(super) fn draw_bounds(&self) -> egui::Rect {
        let page = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.doc.width() as f32, self.doc.height() as f32));
        match self.joined_facing() {
            Some((_, off)) => page.union(page.translate(egui::vec2(off, 0.0))),
            None => page,
        }
    }
}
