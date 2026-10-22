//! Fill (G), lasso fill (L), shapes and camera arrows (U), selection (M)
//! and move/scale (V). Fills sample what is on screen (or only the layer),
//! shapes follow Shift (squares, 45°) and Alt (from the centre), a selection
//! is moved and scaled as a floating piece and laid back on Enter.

use super::*;
use ateliet_core::board::paint::{self, Mode, ShapeKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectMode {
    Rect,
    Lasso,
    Ellipse,
}

pub(super) struct ToolOpts {
    pub fill_tol: u32,
    pub fill_expand: u32,
    /// Sample every visible layer (else only the active one).
    pub fill_all: bool,
    pub fill_behind: bool,
    pub lasso_opacity: f32,
    pub shape: ShapeKind,
    pub shape_width: f32,
    pub shape_fill: bool,
    pub select: SelectMode,
}

impl Default for ToolOpts {
    fn default() -> Self {
        ToolOpts { fill_tol: 40, fill_expand: 2, fill_all: true, fill_behind: false, lasso_opacity: 1.0, shape: ShapeKind::Rect, shape_width: 4.0, shape_fill: false, select: SelectMode::Rect }
    }
}

/// Pixels lifted from a layer, moved and scaled before they go back.
pub(super) struct Float {
    layer: String,
    off: (f32, f32),
    px: Vec<u8>,
    w: usize,
    h: usize,
    /// Where it sits now (canvas): x, y, width, height.
    rect: egui::Rect,
    /// Turned around its centre (radians).
    angle: f32,
    flip: bool,
    tex: Option<egui::TextureHandle>,
    /// Tiles of the layer before the lift (and before laying back).
    before: HashMap<(i32, i32), Option<Tile>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Move,
    Corner(i8, i8),
    Rotate,
}

fn rot(v: egui::Vec2, a: f32) -> egui::Vec2 {
    let (s, c) = a.sin_cos();
    egui::vec2(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// The corners of a rectangle turned around its centre.
fn corners(r: egui::Rect, a: f32) -> [egui::Pos2; 4] {
    let c = r.center();
    [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].map(|q| c + rot(q - c, a))
}

#[derive(Default)]
pub(super) struct ToolState {
    pub opts: ToolOpts,
    /// Points of a lasso or selection being drawn (canvas).
    pts: Vec<egui::Pos2>,
    /// A shape being drawn: start, end, Shift, Alt.
    shape: Option<(egui::Pos2, egui::Pos2, bool, bool)>,
    /// The selection outline (canvas).
    pub sel: Option<Vec<egui::Pos2>>,
    pub float: Option<Float>,
    drag: Option<(Handle, egui::Pos2, egui::Rect, f32)>,
    /// Moving a whole layer: id, its offset at the start, the board before.
    layer_drag: Option<(String, (f64, f64), Value)>,
    /// The visible part of the canvas (for fills on the infinite canvas).
    pub view: (f32, f32, f32, f32),
    lasso_erase: bool,
}

impl Default for SelectMode {
    fn default() -> Self {
        SelectMode::Rect
    }
}

fn bbox(pts: &[egui::Pos2]) -> egui::Rect {
    let mut r = egui::Rect::NOTHING;
    for p in pts {
        r.extend_with(*p);
    }
    r
}

impl BoardEditor {
    /// The active layer when it can be drawn on: id, offset, alpha lock.
    fn drawable(&self) -> Result<(String, (f32, f32), bool), String> {
        let id = self.doc.active_layer(self.frame);
        let l = self.doc.layer(&id);
        if l["kind"].as_str().unwrap_or("raster") != "raster" {
            return Err("Escolha uma camada de desenho.".into());
        }
        if l["locked"].as_bool() == Some(true) {
            return Err("Esta camada está travada.".into());
        }
        if l["visible"].as_bool() == Some(false) {
            return Err("Esta camada está escondida.".into());
        }
        let off = (l["x"].as_f64().unwrap_or(0.0) as f32, l["y"].as_f64().unwrap_or(0.0) as f32);
        Ok((id, off, l["alphaLock"].as_bool() == Some(true)))
    }

    /// The box (canvas) a region tool may paint in, clipped to the page.
    fn clip_box(&self, r: egui::Rect) -> Option<(i32, i32, usize, usize)> {
        let r = if self.doc.infinite() { r } else { r.intersect(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.doc.width() as f32, self.doc.height() as f32))) };
        if !(r.width() > 0.0 && r.height() > 0.0) {
            return None;
        }
        let (x0, y0) = (r.min.x.floor() as i32, r.min.y.floor() as i32);
        let (x1, y1) = (r.max.x.ceil() as i32, r.max.y.ceil() as i32);
        Some((x0, y0, (x1 - x0) as usize, (y1 - y0) as usize))
    }

    fn commit_tiles(&mut self, layer: &str, tiles: Vec<paint::TileChange>) {
        if tiles.is_empty() {
            return;
        }
        self.doc.mark_layer(layer);
        self.undo.push(Edit::Tiles { layer: layer.to_string(), tiles });
        self.redo.clear();
        self.invalidate();
        self.changed();
    }

    fn fill_at(&mut self, c: egui::Pos2) -> Result<(), String> {
        let (layer, off, lock) = self.drawable()?;
        let region = if self.doc.infinite() {
            let v = self.ts.view;
            egui::Rect::from_min_max(egui::pos2(v.0, v.1), egui::pos2(v.2, v.3))
        } else {
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.doc.width() as f32, self.doc.height() as f32))
        };
        if !region.contains(c) {
            return Ok(());
        }
        let Some((x0, y0, w, h)) = self.clip_box(region) else { return Ok(()) };
        if w * h > 40_000_000 {
            return Err("Aproxime para preencher: a área na tela é grande demais.".into());
        }
        let o = &self.ts.opts;
        let (tol, expand, all, behind) = (o.fill_tol, o.fill_expand, o.fill_all, o.fill_behind);
        let src = if all {
            let opts = Options { background: false, onion: false, overlay: None, exporting: true };
            compose::region(&mut self.doc, &mut self.pics, self.frame, x0, y0, w, h, &opts)
        } else {
            paint::read_layer(&mut self.doc, &layer, x0 - off.0 as i32, y0 - off.1 as i32, w, h)
        };
        let Some(mut mask) = paint::flood(&src, w, h, c.x.floor() as i64 - x0 as i64, c.y.floor() as i64 - y0 as i64, tol) else { return Ok(()) };
        for _ in 0..expand {
            paint::dilate(&mut mask, w, h);
        }
        mask.iter_mut().for_each(|m| *m = if *m != 0 { 255 } else { 0 });
        let mode = if lock { Mode::Atop } else if behind { Mode::Behind } else { Mode::Over };
        let ch = paint::apply_mask(&mut self.doc, &layer, x0 - off.0 as i32, y0 - off.1 as i32, w, h, &mask, self.color, 1.0, mode);
        self.commit_tiles(&layer, ch);
        Ok(())
    }

    fn lasso_fill(&mut self, pts: &[egui::Pos2], erase: bool) -> Result<(), String> {
        if pts.len() < 3 {
            return Ok(());
        }
        let (layer, off, lock) = self.drawable()?;
        let Some((x0, y0, w, h)) = self.clip_box(bbox(pts).expand(2.0)) else { return Ok(()) };
        let poly: Vec<(f64, f64)> = pts.iter().map(|p| (p.x as f64, p.y as f64)).collect();
        let cov = paint::polygon_mask(&[poly], x0 as f64, y0 as f64, w, h);
        let mode = if erase { Mode::Erase } else if lock { Mode::Atop } else { Mode::Over };
        let a = self.ts.opts.lasso_opacity;
        let ch = paint::apply_mask(&mut self.doc, &layer, x0 - off.0 as i32, y0 - off.1 as i32, w, h, &cov, self.color, a, mode);
        self.commit_tiles(&layer, ch);
        Ok(())
    }

    /// Start and end of the shape after Shift and Alt.
    fn shape_geometry(&self) -> Option<(egui::Pos2, egui::Pos2)> {
        let (mut a, mut b, shift, alt) = self.ts.shape?;
        let line = matches!(self.ts.opts.shape, ShapeKind::Line | ShapeKind::Arrow);
        if shift {
            if line {
                let step = std::f32::consts::FRAC_PI_4;
                let ang = ((b.y - a.y).atan2(b.x - a.x) / step).round() * step;
                let d = a.distance(b);
                b = a + egui::vec2(ang.cos(), ang.sin()) * d;
            } else {
                let d = (b.x - a.x).abs().max((b.y - a.y).abs());
                let sx = if b.x >= a.x { 1.0 } else { -1.0 };
                let sy = if b.y >= a.y { 1.0 } else { -1.0 };
                b = a + egui::vec2(sx * d, sy * d);
            }
        }
        if alt && !line {
            a = egui::pos2(2.0 * a.x - b.x, 2.0 * a.y - b.y);
        }
        Some((a, b))
    }

    fn draw_shape(&mut self) -> Result<(), String> {
        let Some((a, b)) = self.shape_geometry() else { return Ok(()) };
        if a.distance(b) * self.zoom < 2.0 {
            return Ok(());
        }
        let (layer, off, lock) = self.drawable()?;
        let o = &self.ts.opts;
        let (kind, width, fill) = (o.shape, o.shape_width, o.shape_fill);
        let pad = width * 4.0 + 12.0;
        let Some((x0, y0, w, h)) = self.clip_box(egui::Rect::from_two_pos(a, b).expand(pad)) else { return Ok(()) };
        let cov = paint::shape_mask(kind, (a.x as f64, a.y as f64), (b.x as f64, b.y as f64), width as f64, fill, x0 as f64, y0 as f64, w, h);
        let ch = paint::apply_mask(&mut self.doc, &layer, x0 - off.0 as i32, y0 - off.1 as i32, w, h, &cov, self.color, 1.0, if lock { Mode::Atop } else { Mode::Over });
        self.commit_tiles(&layer, ch);
        Ok(())
    }

    fn select_polygon(&self, shift: bool) -> Vec<egui::Pos2> {
        match self.ts.opts.select {
            SelectMode::Lasso => self.ts.pts.clone(),
            m => {
                if self.ts.pts.len() < 2 {
                    return Vec::new();
                }
                let a = self.ts.pts[0];
                let mut b = *self.ts.pts.last().unwrap();
                if shift {
                    let d = (b.x - a.x).abs().max((b.y - a.y).abs());
                    b = a + egui::vec2(if b.x >= a.x { d } else { -d }, if b.y >= a.y { d } else { -d });
                }
                if m == SelectMode::Rect {
                    vec![a, egui::pos2(b.x, a.y), b, egui::pos2(a.x, b.y)]
                } else {
                    paint::ellipse_points((a.x as f64, a.y as f64), (b.x as f64, b.y as f64), 64).into_iter().map(|q| egui::pos2(q.0 as f32, q.1 as f32)).collect()
                }
            }
        }
    }

    /// The selection's pixels as a coverage mask over its box (canvas).
    fn selection_mask(&self) -> Option<((i32, i32, usize, usize), Vec<u8>)> {
        let sel = self.ts.sel.as_ref()?;
        let b = self.clip_box(bbox(sel).expand(1.0))?;
        let poly: Vec<(f64, f64)> = sel.iter().map(|p| (p.x as f64, p.y as f64)).collect();
        Some((b, paint::polygon_mask(&[poly], b.0 as f64, b.1 as f64, b.2, b.3)))
    }

    /// Delete what the selection covers on the active layer.
    pub(super) fn clear_selection(&mut self) {
        let Ok((layer, off, _)) = self.drawable() else { return };
        let Some(((x0, y0, w, h), cov)) = self.selection_mask() else { return };
        let ch = paint::apply_mask(&mut self.doc, &layer, x0 - off.0 as i32, y0 - off.1 as i32, w, h, &cov, [0, 0, 0], 1.0, Mode::Erase);
        self.commit_tiles(&layer, ch);
    }

    /// Lift the selected pixels (or all of the layer's, without a selection
    /// it is the layer that moves) into a floating piece.
    fn lift(&mut self) -> Result<bool, String> {
        let (layer, off, _) = self.drawable()?;
        let Some(((x0, y0, w, h), cov)) = self.selection_mask() else { return Ok(false) };
        let (lx, ly) = (x0 - off.0 as i32, y0 - off.1 as i32);
        let mut px = paint::read_layer(&mut self.doc, &layer, lx, ly, w, h);
        for (i, &k) in cov.iter().enumerate() {
            for c in 0..4 {
                px[i * 4 + c] = ateliet_core::board::tile::mul255(px[i * 4 + c] as u32, k as u32) as u8;
            }
        }
        if px.chunks_exact(4).all(|p| p[3] == 0) {
            return Err("Não há nada desenhado na seleção desta camada.".into());
        }
        let ch = paint::apply_mask(&mut self.doc, &layer, lx, ly, w, h, &cov, [0, 0, 0], 1.0, Mode::Erase);
        let mut before = HashMap::new();
        for (k, b, _) in ch {
            before.insert(k, b);
        }
        let rect = egui::Rect::from_min_size(egui::pos2(x0 as f32, y0 as f32), egui::vec2(w as f32, h as f32));
        self.ts.float = Some(Float { layer, off, px, w, h, rect, angle: 0.0, flip: false, tex: None, before });
        self.ts.sel = None;
        self.invalidate();
        Ok(true)
    }

    /// Lay the floating piece back on its layer (one undo step with the lift).
    pub(super) fn commit_float(&mut self) {
        let Some(f) = self.ts.float.take() else { return };
        let r = f.rect;
        let quad = corners(r, f.angle);
        let b = bbox(&quad);
        let (x0, y0) = (b.min.x.floor(), b.min.y.floor());
        let (w, h) = ((b.max.x.ceil() - x0) as usize, (b.max.y.ceil() - y0) as usize);
        let mut before = f.before;
        if w > 0 && h > 0 && w * h < 80_000_000 {
            let c = r.center();
            let px = paint::place_image(&f.px, f.w, f.h, (c.x as f64, c.y as f64), (r.width() as f64, r.height() as f64), f.angle as f64, f.flip, x0 as f64, y0 as f64, w, h);
            let ch = paint::apply_image(&mut self.doc, &f.layer, x0 as i32 - f.off.0 as i32, y0 as i32 - f.off.1 as i32, w, h, &px, 1.0, Mode::Over);
            for (k, b, _) in ch {
                before.entry(k).or_insert(b);
            }
        }
        let mut tiles = Vec::new();
        for (k, b) in before {
            let after = self.doc.tile(&f.layer, k.0, k.1).cloned();
            tiles.push((k, b, after));
        }
        // the piece's outline stays selected where it landed
        self.ts.sel = Some(quad.to_vec());
        self.commit_tiles(&f.layer, tiles);
    }

    /// Put the floating piece back where it was, unchanged.
    fn cancel_float(&mut self) {
        let Some(f) = self.ts.float.take() else { return };
        for (k, b) in f.before {
            self.doc.set_tile(&f.layer, k.0, k.1, b);
        }
        self.doc.mark_layer(&f.layer);
        self.invalidate();
    }

    fn handle_at(&self, c: egui::Pos2, r: egui::Rect, angle: f32) -> Option<Handle> {
        let tol = 9.0 / self.zoom;
        // in the piece's own (unturned) frame
        let c = r.center() + rot(c - r.center(), -angle);
        if c.distance(egui::pos2(r.center().x, r.min.y - 24.0 / self.zoom)) <= tol {
            return Some(Handle::Rotate);
        }
        for (sx, sy) in [(-1i8, -1i8), (1, -1), (1, 1), (-1, 1)] {
            let q = egui::pos2(if sx < 0 { r.min.x } else { r.max.x }, if sy < 0 { r.min.y } else { r.max.y });
            if q.distance(c) <= tol {
                return Some(Handle::Corner(sx, sy));
            }
        }
        r.expand(tol).contains(c).then_some(Handle::Move)
    }

    /// The selection (or the whole frame) as a picture on the clipboard:
    /// the active layer alone, or everything seen there with the paper.
    fn copy_image(&mut self, seen: bool) -> Result<(), String> {
        let (x0, y0, w, h, cov) = match self.selection_mask() {
            Some(((x0, y0, w, h), cov)) => (x0, y0, w, h, Some(cov)),
            None if !self.doc.infinite() => (0, 0, self.doc.width() as usize, self.doc.height() as usize, None),
            None => return Err("Selecione o que copiar.".into()),
        };
        let mut px = if seen {
            let opts = Options { background: true, onion: false, overlay: None, exporting: true };
            compose::region(&mut self.doc, &mut self.pics, self.frame, x0, y0, w, h, &opts)
        } else {
            let id = self.doc.active_layer(self.frame);
            let l = self.doc.layer(&id);
            let off = (l["x"].as_f64().unwrap_or(0.0) as i32, l["y"].as_f64().unwrap_or(0.0) as i32);
            paint::read_layer(&mut self.doc, &id, x0 - off.0, y0 - off.1, w, h)
        };
        if let Some(cov) = cov {
            for (i, &k) in cov.iter().enumerate() {
                for c in 0..4 {
                    px[i * 4 + c] = ateliet_core::board::tile::mul255(px[i * 4 + c] as u32, k as u32) as u8;
                }
            }
        }
        let straight = ateliet_core::board::tile::unpremultiply(&px);
        let img = arboard::ImageData { width: w, height: h, bytes: std::borrow::Cow::Owned(straight) };
        arboard::Clipboard::new().and_then(|mut c| c.set_image(img)).map_err(|e| format!("Não consegui copiar: {e}"))?;
        self.error = Some(if seen { "Copiado o que se vê.".into() } else { "Copiado da camada.".into() });
        Ok(())
    }

    /// Pointer input for the region tools; returns a message for the user.
    pub(super) fn tool_input(&mut self, ui: &egui::Ui, resp: &egui::Response, tc: &dyn Fn(egui::Pos2) -> egui::Pos2) -> Option<String> {
        let mods = ui.input(|i| i.modifiers);
        let pos = resp.interact_pointer_pos().or(resp.hover_pos()).map(tc);
        // drags start where the button went down, not where they were noticed
        let origin = ui.input(|i| i.pointer.press_origin()).map(tc).or(pos);
        let down = ui.input(|i| i.pointer.primary_down());
        let mut err = None;
        match self.tool {
            Tool::Fill => {
                if resp.clicked() {
                    if let Some(c) = pos {
                        err = self.fill_at(c).err();
                    }
                }
            }
            Tool::Lasso | Tool::Select => {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(c) = origin {
                        self.ts.pts = vec![c];
                        self.ts.lasso_erase = mods.alt;
                    }
                } else if !self.ts.pts.is_empty() {
                    if let Some(c) = pos {
                        let lasso = self.tool == Tool::Lasso || self.ts.opts.select == SelectMode::Lasso;
                        let last = *self.ts.pts.last().unwrap();
                        if !lasso {
                            self.ts.pts.truncate(1);
                            self.ts.pts.push(c);
                        } else if last.distance(c) * self.zoom >= 2.0 {
                            self.ts.pts.push(c);
                        }
                    }
                    if !down {
                        if self.tool == Tool::Lasso {
                            let pts = std::mem::take(&mut self.ts.pts);
                            err = self.lasso_fill(&pts, self.ts.lasso_erase).err();
                        } else {
                            let poly = self.select_polygon(mods.shift);
                            self.ts.pts.clear();
                            let r = bbox(&poly);
                            self.ts.sel = (poly.len() >= 3 && r.width() * self.zoom > 2.0 && r.height() * self.zoom > 2.0).then_some(poly);
                        }
                    }
                }
                if self.tool == Tool::Select && resp.clicked() {
                    self.ts.sel = None;
                }
            }
            Tool::Shape => {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(c) = origin {
                        self.ts.shape = Some((c, c, mods.shift, mods.alt));
                    }
                } else if let Some(s) = &mut self.ts.shape {
                    if let Some(c) = pos {
                        *s = (s.0, c, mods.shift, mods.alt);
                    }
                    if !down {
                        err = self.draw_shape().err();
                        self.ts.shape = None;
                    }
                }
            }
            Tool::Move => {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(c) = origin {
                        if self.ts.float.is_none() && self.ts.sel.as_ref().is_some_and(|s| bbox(s).expand(9.0 / self.zoom).contains(c)) {
                            match self.lift() {
                                Err(e) => err = Some(e),
                                Ok(_) => {}
                            }
                        }
                        if let Some(f) = &self.ts.float {
                            if let Some(h) = self.handle_at(c, f.rect, f.angle) {
                                self.ts.drag = Some((h, c, f.rect, f.angle));
                            } else {
                                self.commit_float();
                            }
                        } else if self.ts.sel.is_none() && err.is_none() {
                            match self.drawable() {
                                Ok((id, _, _)) => {
                                    let l = self.doc.layer(&id);
                                    let o = (l["x"].as_f64().unwrap_or(0.0), l["y"].as_f64().unwrap_or(0.0));
                                    self.ts.layer_drag = Some((id, o, self.doc.board.clone()));
                                    self.ts.drag = Some((Handle::Move, c, egui::Rect::NOTHING, 0.0));
                                }
                                Err(e) => err = Some(e),
                            }
                        }
                    }
                } else if let Some((h, start, r0, a0)) = self.ts.drag {
                    if let Some(c) = pos {
                        let d = c - start;
                        if let Some((id, o, _)) = &self.ts.layer_drag {
                            let (id, o) = (id.clone(), *o);
                            let l = self.doc.layer_mut(&id);
                            l["x"] = json!((o.0 + d.x as f64).round());
                            l["y"] = json!((o.1 + d.y as f64).round());
                            self.invalidate();
                        } else if let Some(f) = &mut self.ts.float {
                            match h {
                                Handle::Move => f.rect = r0.translate(d),
                                Handle::Rotate => {
                                    let c0 = r0.center();
                                    let turn = (c - c0).angle() - (start - c0).angle();
                                    let mut a = a0 + turn;
                                    if mods.shift {
                                        let step = 15f32.to_radians();
                                        a = (a / step).round() * step;
                                    }
                                    f.angle = a;
                                }
                                Handle::Corner(sx, sy) => {
                                    let d = rot(d, -a0);
                                    let fixed = egui::pos2(if sx < 0 { r0.max.x } else { r0.min.x }, if sy < 0 { r0.max.y } else { r0.min.y });
                                    let mut moving = egui::pos2(if sx < 0 { r0.min.x } else { r0.max.x }, if sy < 0 { r0.min.y } else { r0.max.y }) + d;
                                    if mods.shift {
                                        // keep the proportions
                                        let k = ((moving.x - fixed.x) / (r0.width() * sx as f32)).max((moving.y - fixed.y) / (r0.height() * sy as f32)).max(0.02);
                                        moving = egui::pos2(fixed.x + r0.width() * sx as f32 * k, fixed.y + r0.height() * sy as f32 * k);
                                    }
                                    let local = egui::Rect::from_two_pos(fixed, moving);
                                    let c0 = r0.center();
                                    f.rect = egui::Rect::from_center_size(c0 + rot(local.center() - c0, a0), local.size());
                                }
                            }
                        }
                    }
                    if !down {
                        self.ts.drag = None;
                        if let Some((id, _, before)) = self.ts.layer_drag.take() {
                            let after = self.doc.board.clone();
                            if after != before {
                                self.doc.mark_layer(&id);
                                self.undo.push(Edit::Structure { before, after, tiles: Vec::new() });
                                self.redo.clear();
                                self.changed();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        err
    }

    /// Keys of the region tools.
    pub(super) fn tool_keys(&mut self, ui: &egui::Ui) {
        let k = |key| ui.input(|i| i.key_pressed(key) && i.modifiers.is_none());
        for (key, t) in [(egui::Key::G, Tool::Fill), (egui::Key::L, Tool::Lasso), (egui::Key::U, Tool::Shape), (egui::Key::M, Tool::Select), (egui::Key::V, Tool::Move)] {
            if k(key) {
                self.tool = t;
            }
        }
        if self.ts.float.is_some() {
            if k(egui::Key::Enter) {
                self.commit_float();
            }
            if k(egui::Key::Escape) {
                self.cancel_float();
            }
            if ui.input(|i| i.key_pressed(egui::Key::H) && i.modifiers.shift) {
                if let Some(f) = &mut self.ts.float {
                    f.flip = !f.flip;
                    f.tex = None;
                }
            }
        } else if self.ts.sel.is_some() {
            if k(egui::Key::Delete) || k(egui::Key::Backspace) {
                self.clear_selection();
            }
            if k(egui::Key::Escape) || ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::D)) {
                self.ts.sel = None;
            }
        }
        if ui.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::A)) && !self.doc.infinite() {
            let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
            self.ts.sel = Some(vec![egui::pos2(0.0, 0.0), egui::pos2(w, 0.0), egui::pos2(w, h), egui::pos2(0.0, h)]);
            self.tool = Tool::Select;
        }
        // [ and ]: smaller and bigger
        let (smaller, bigger) = ui.input(|i| (i.key_pressed(egui::Key::OpenBracket), i.key_pressed(egui::Key::CloseBracket)));
        if smaller || bigger {
            let k = if bigger { 1.2 } else { 1.0 / 1.2 };
            if let Tool::Brush(b) = self.tool {
                if let Some(s) = self.settings.get_mut(&b) {
                    s.size = (s.size * k).clamp(1.0, 300.0);
                }
            } else if self.tool == Tool::Shape {
                self.ts.opts.shape_width = (self.ts.opts.shape_width * k).clamp(1.0, 80.0);
            }
        }
        // copy: the selection of the active layer, or (Shift) what is seen there
        let copy = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)) || (i.modifiers.command && i.key_pressed(egui::Key::C)));
        if copy {
            let all = ui.input(|i| i.modifiers.shift);
            if let Err(e) = self.copy_image(all) {
                self.error = Some(e);
            }
        }
        // leaving the move tool lays the piece down
        if self.ts.float.is_some() && !matches!(self.tool, Tool::Move | Tool::Select) {
            self.commit_float();
        }
    }

    /// What the region tools draw over the canvas.
    pub(super) fn tool_draw(&mut self, ui: &egui::Ui, painter: &egui::Painter, fc: &dyn Fn(egui::Pos2) -> egui::Pos2) {
        let col = Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
        let blue = Color32::from_rgb(0x2f, 0x80, 0xed);
        let shift = ui.input(|i| i.modifiers.shift);
        // a lasso or selection being drawn
        if !self.ts.pts.is_empty() {
            let poly: Vec<egui::Pos2> = if self.tool == Tool::Select { self.select_polygon(shift) } else { self.ts.pts.clone() };
            let pts: Vec<egui::Pos2> = poly.iter().map(|p| fc(*p)).collect();
            if pts.len() >= 2 {
                if self.tool == Tool::Lasso {
                    let fill = if self.ts.lasso_erase { Color32::from_white_alpha(90) } else { col.gamma_multiply(0.35) };
                    painter.add(egui::Shape::Path(egui::epaint::PathShape { points: pts.clone(), closed: true, fill, stroke: egui::Stroke::NONE.into() }));
                }
                let mut closed = pts.clone();
                closed.push(pts[0]);
                painter.extend(egui::Shape::dashed_line(&closed, egui::Stroke::new(1.0_f32, if self.tool == Tool::Lasso { Color32::BLACK } else { blue }), 4.0, 3.0));
            }
        }
        // the shape being drawn
        if let Some((a, b)) = self.shape_geometry() {
            let w = (self.ts.opts.shape_width * self.zoom).max(1.0);
            let st = egui::Stroke::new(w, col.gamma_multiply(0.85));
            let (sa, sb) = (fc(a), fc(b));
            match self.ts.opts.shape {
                ShapeKind::Line => {
                    painter.line_segment([sa, sb], st);
                }
                ShapeKind::Arrow => {
                    painter.line_segment([sa, sb], st);
                    let head: Vec<egui::Pos2> = paint::arrow_head((a.x as f64, a.y as f64), (b.x as f64, b.y as f64), self.ts.opts.shape_width as f64).into_iter().map(|q| fc(egui::pos2(q.0 as f32, q.1 as f32))).collect();
                    painter.add(egui::Shape::convex_polygon(head, col.gamma_multiply(0.85), egui::Stroke::NONE));
                }
                ShapeKind::Rect | ShapeKind::Ellipse => {
                    let poly: Vec<egui::Pos2> = if self.ts.opts.shape == ShapeKind::Rect {
                        vec![a, egui::pos2(b.x, a.y), b, egui::pos2(a.x, b.y)]
                    } else {
                        paint::ellipse_points((a.x as f64, a.y as f64), (b.x as f64, b.y as f64), 96).into_iter().map(|q| egui::pos2(q.0 as f32, q.1 as f32)).collect()
                    };
                    let pts: Vec<egui::Pos2> = poly.into_iter().map(fc).collect();
                    let fill = if self.ts.opts.shape_fill { col.gamma_multiply(0.85) } else { Color32::TRANSPARENT };
                    painter.add(egui::Shape::Path(egui::epaint::PathShape { points: pts, closed: true, fill, stroke: if self.ts.opts.shape_fill { egui::Stroke::NONE.into() } else { st.into() } }));
                }
            }
        }
        // the selection: marching outline
        if let Some(sel) = &self.ts.sel {
            let mut pts: Vec<egui::Pos2> = sel.iter().map(|p| fc(*p)).collect();
            if let Some(f) = pts.first().copied() {
                pts.push(f);
            }
            painter.extend(egui::Shape::dashed_line(&pts, egui::Stroke::new(1.0_f32, Color32::WHITE), 4.0, 4.0));
            let off: Vec<egui::Pos2> = pts.iter().map(|p| *p + egui::vec2(0.5, 0.5)).collect();
            painter.extend(egui::Shape::dashed_line_with_offset(&off, egui::Stroke::new(1.0_f32, blue), &[4.0], &[4.0], 4.0));
        }
        // the floating piece, with its handles
        if let Some(f) = &mut self.ts.float {
            if f.tex.is_none() {
                let straight = ateliet_core::board::tile::unpremultiply(&f.px);
                let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &straight);
                f.tex = Some(ui.ctx().load_texture("board-float", img, egui::TextureOptions::LINEAR));
            }
            // the turned rectangle on screen (the mirrored view flips it by itself)
            let q = corners(f.rect, f.angle).map(fc);
            if let Some(t) = &f.tex {
                let (u0, u1) = if f.flip { (1.0, 0.0) } else { (0.0, 1.0) };
                let mut mesh = egui::Mesh::with_texture(t.id());
                for (p, uv) in q.iter().zip([egui::pos2(u0, 0.0), egui::pos2(u1, 0.0), egui::pos2(u1, 1.0), egui::pos2(u0, 1.0)]) {
                    mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv, color: Color32::WHITE });
                }
                mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                painter.add(egui::Shape::mesh(mesh));
            }
            let stroke = egui::Stroke::new(1.0_f32, blue);
            painter.add(egui::Shape::closed_line(q.to_vec(), stroke));
            for c in q {
                painter.rect(egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0)), 1.0, Color32::WHITE, stroke, egui::StrokeKind::Inside);
            }
            // the turning handle above the top edge
            let r = f.rect;
            let top = fc(r.center() + rot(egui::pos2(r.center().x, r.min.y) - r.center(), f.angle));
            let knob = fc(r.center() + rot(egui::pos2(r.center().x, r.min.y - 24.0 / self.zoom) - r.center(), f.angle));
            painter.line_segment([top, knob], stroke);
            painter.circle(knob, 5.0, Color32::WHITE, stroke);
        }
    }

    /// Settings of the region tool in the toolbar.
    pub(super) fn tool_options(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let o = &mut self.ts.opts;
        let small = |s: &str| RichText::new(s).size(12.0).color(p.muted);
        match self.tool {
            Tool::Fill => {
                ui.label(small("Tolerância"));
                ui.spacing_mut().slider_width = 80.0;
                ui.add(egui::Slider::new(&mut o.fill_tol, 0..=128).show_value(false));
                ui.label(small("Expandir"));
                ui.add(egui::DragValue::new(&mut o.fill_expand).range(0..=8).suffix(" px"));
                kit::toggle(ui, &mut o.fill_all, p).on_hover_text("Ligado: os limites vêm de todas as camadas visíveis. Desligado: só desta camada.");
                ui.label(small("Todas as camadas"));
                kit::toggle(ui, &mut o.fill_behind, p);
                ui.label(small("Por trás"));
            }
            Tool::Lasso => {
                ui.label(small("Opacidade"));
                ui.spacing_mut().slider_width = 80.0;
                ui.add(egui::Slider::new(&mut o.lasso_opacity, 0.05..=1.0).show_value(false));
            }
            Tool::Shape => {
                for (k, g, tip) in [(ShapeKind::Line, icons::SLASH, "Linha"), (ShapeKind::Rect, icons::SQUARE, "Retângulo"), (ShapeKind::Ellipse, icons::CIRCLE, "Elipse"), (ShapeKind::Arrow, icons::MOVE_UP_RIGHT, "Seta de câmera")] {
                    if kit::icon_button_sized(ui, g, tip, o.shape == k, p, 26.0, 14.0).clicked() {
                        o.shape = k;
                    }
                }
                ui.label(small("Espessura"));
                ui.add(egui::DragValue::new(&mut o.shape_width).range(1.0..=80.0).speed(0.3));
                if matches!(o.shape, ShapeKind::Rect | ShapeKind::Ellipse) {
                    kit::toggle(ui, &mut o.shape_fill, p);
                    ui.label(small("Preenchida"));
                }
            }
            Tool::Select | Tool::Move => {
                for (k, g, tip) in [(SelectMode::Rect, icons::SQUARE_DASHED, "Retângulo"), (SelectMode::Ellipse, icons::CIRCLE, "Elipse"), (SelectMode::Lasso, icons::LASSO, "Laço")] {
                    if kit::icon_button_sized(ui, g, tip, o.select == k && self.tool == Tool::Select, p, 26.0, 14.0).clicked() {
                        o.select = k;
                        self.tool = Tool::Select;
                    }
                }
                let has = self.ts.sel.is_some();
                let floating = self.ts.float.is_some();
                if floating {
                    if kit::button(ui, None, "Aplicar (Enter)", p).clicked() {
                        self.commit_float();
                    }
                    if kit::icon_button_sized(ui, icons::FLIP_HORIZONTAL_2, "Espelhar (Shift+H)", false, p, 26.0, 14.0).clicked() {
                        if let Some(f) = &mut self.ts.float {
                            f.flip = !f.flip;
                            f.tex = None;
                        }
                    }
                } else if has {
                    if kit::icon_button_sized(ui, icons::TRASH_2, "Apagar o que está na seleção (Delete)", false, p, 26.0, 14.0).clicked() {
                        self.clear_selection();
                    }
                    if kit::icon_button_sized(ui, icons::X, "Tirar a seleção (Ctrl+D)", false, p, 26.0, 14.0).clicked() {
                        self.ts.sel = None;
                    }
                }
            }
            _ => {}
        }
    }
}
