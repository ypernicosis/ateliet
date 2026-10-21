//! The native storyboard editor: frames, layers, brushes, onion skin, undo,
//! playback, on the core's drawing engine and the same `.storyboard` files
//! as the web editor.

use ateliet_core::board::brush::{BrushKind, BrushSettings, Point, Stroke};
use ateliet_core::board::compose::{self, Options, Overlay, Pictures};
use ateliet_core::board::doc::{self, BoardDoc, TileKey};
use ateliet_core::board::tile::{DabMode, Tile, TILE};
use eframe::egui::{self, Color32, RichText, Sense};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush(BrushKind),
    Hand,
    Picker,
}

enum Edit {
    Tiles { layer: String, tiles: Vec<((i32, i32), Option<Tile>, Option<Tile>)> },
    Structure { before: Value, after: Value, tiles: Vec<(TileKey, Option<Tile>)> },
}

struct Active {
    stroke: Stroke,
    layer: String,
    /// Layer offset (canvas = layer + offset).
    off: (f32, f32),
    color: [u8; 3],
    buffer: HashMap<(i32, i32), Tile>,
    /// Tiles of the layer before the stroke (pencil paints the layer directly).
    before: HashMap<(i32, i32), Option<Tile>>,
    alpha_lock: bool,
}

pub struct BoardEditor {
    pub path: String,
    abs: PathBuf,
    pub doc: BoardDoc,
    pics: Pictures,
    pub frame: usize,
    tool: Tool,
    settings: HashMap<BrushKind, BrushSettings>,
    color: [u8; 3],
    zoom: f32,
    pan: egui::Vec2,
    fit: bool,
    active: Option<Active>,
    comp: HashMap<(i32, i32), egui::TextureHandle>,
    stale: HashSet<(i32, i32)>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    pub dirty: bool,
    last_change: Option<Instant>,
    playing: Option<(Instant, usize)>,
    thumbs: HashMap<String, (u64, egui::TextureHandle)>,
    versions: HashMap<String, u64>,
    pub error: Option<String>,
}

impl BoardEditor {
    pub fn open(path: &str, abs: PathBuf) -> Result<BoardEditor, String> {
        let doc = BoardDoc::open(&abs)?;
        Ok(Self::with(path, abs, doc))
    }

    pub fn create(path: &str, abs: PathBuf, kind: &str, w: u32, h: u32) -> Result<BoardEditor, String> {
        let mut doc = BoardDoc::new(kind, w, h, doc::PAPER, 24);
        let mut pics = Pictures::default();
        doc.save(&abs, |d, i| compose::thumbnail_png(d, &mut pics, i))?;
        Ok(Self::with(path, abs, doc))
    }

    fn with(path: &str, abs: PathBuf, doc: BoardDoc) -> BoardEditor {
        let settings = BrushKind::ALL.iter().map(|k| (*k, BrushSettings::defaults(*k))).collect();
        BoardEditor {
            path: path.to_string(),
            abs,
            doc,
            pics: Pictures::default(),
            frame: 0,
            tool: Tool::Brush(BrushKind::Pencil),
            settings,
            color: [0x23, 0x22, 0x1f],
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit: true,
            active: None,
            comp: HashMap::new(),
            stale: HashSet::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            last_change: None,
            playing: None,
            thumbs: HashMap::new(),
            versions: HashMap::new(),
            error: None,
        }
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let pics = &mut self.pics;
        match self.doc.save(&self.abs, |d, i| compose::thumbnail_png(d, pics, i)) {
            Ok(()) => {
                self.dirty = false;
                self.last_change = None;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Could not save: {e}")),
        }
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.last_change = Some(Instant::now());
        let id = self.doc.frame_id(self.frame);
        *self.versions.entry(id).or_default() += 1;
    }

    fn invalidate(&mut self) {
        self.stale.extend(self.comp.keys().copied());
    }

    fn invalidate_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        for k in doc::tiles_in(x0, y0, x1, y1) {
            self.stale.insert(k);
        }
    }

    fn structure(&mut self, f: impl FnOnce(&mut BoardDoc)) {
        let before = self.doc.board.clone();
        let tiles_before: Vec<TileKey> = self.doc.tiles.keys().cloned().collect();
        let snapshot: Vec<(TileKey, Option<Tile>)> = tiles_before
            .iter()
            .map(|k| {
                self.doc.ensure_decoded(k);
                (k.clone(), self.doc.tiles.get(k).and_then(|s| s.tile.clone()))
            })
            .collect();
        f(&mut self.doc);
        let after = self.doc.board.clone();
        // keep the tiles a deletion dropped, so undo brings them back
        let gone: Vec<(TileKey, Option<Tile>)> = snapshot.into_iter().filter(|(k, _)| !self.doc.tiles.contains_key(k)).collect();
        self.undo.push(Edit::Structure { before, after, tiles: gone });
        self.redo.clear();
        self.frame = self.frame.min(self.doc.frame_count().saturating_sub(1));
        self.invalidate();
        self.changed();
    }

    fn apply_edit(&mut self, e: &Edit, undo: bool) {
        match e {
            Edit::Tiles { layer, tiles } => {
                for (k, before, after) in tiles {
                    let t = if undo { before } else { after };
                    self.doc.set_tile(layer, k.0, k.1, t.clone());
                }
                self.doc.mark_layer(layer);
            }
            Edit::Structure { before, after, tiles } => {
                self.doc.board = if undo { before.clone() } else { after.clone() };
                if undo {
                    for (k, t) in tiles {
                        self.doc.set_tile(&k.0, k.1, k.2, t.clone());
                    }
                }
                self.frame = self.frame.min(self.doc.frame_count().saturating_sub(1));
                for i in 0..self.doc.frame_count() {
                    self.doc.mark_frame(i);
                }
            }
        }
        self.invalidate();
        self.changed();
    }

    fn undo(&mut self) {
        if let Some(e) = self.undo.pop() {
            self.apply_edit(&e, true);
            self.redo.push(e);
        }
    }

    fn redo(&mut self) {
        if let Some(e) = self.redo.pop() {
            self.apply_edit(&e, false);
            self.undo.push(e);
        }
    }

    fn brush(&self) -> Option<BrushKind> {
        match self.tool {
            Tool::Brush(k) => Some(k),
            _ => None,
        }
    }

    // ── strokes ────────────────────────────────────────────────────────────

    fn begin(&mut self, at: egui::Pos2) -> Result<(), String> {
        let Some(kind) = self.brush() else { return Ok(()) };
        let layer = self.doc.active_layer(self.frame);
        let l = self.doc.layer(&layer).clone();
        if l["kind"].as_str().unwrap_or("raster") != "raster" {
            return Err("Pick a drawing layer to draw on.".into());
        }
        if l["locked"].as_bool() == Some(true) {
            return Err("This layer is locked.".into());
        }
        if l["visible"].as_bool() == Some(false) {
            return Err("This layer is hidden.".into());
        }
        let off = (l["x"].as_f64().unwrap_or(0.0) as f32, l["y"].as_f64().unwrap_or(0.0) as f32);
        let s = self.settings[&kind];
        let mut a = Active { stroke: Stroke::new(kind, s), layer, off, color: self.color, buffer: HashMap::new(), before: HashMap::new(), alpha_lock: l["alphaLock"].as_bool() == Some(true) };
        let dabs = a.stroke.add(Point { x: at.x - off.0, y: at.y - off.1, p: 1.0 });
        self.active = Some(a);
        self.paint(dabs);
        Ok(())
    }

    fn paint(&mut self, dabs: Vec<ateliet_core::board::brush::Dab>) {
        let Some(a) = &mut self.active else { return };
        let t = TILE as f32;
        let mut touched = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for d in dabs {
            for (tx, ty) in doc::tiles_in(d.x - d.radius - 1.0, d.y - d.radius - 1.0, d.x + d.radius + 1.0, d.y + d.radius + 1.0) {
                let (lx, ly) = (d.x - tx as f32 * t, d.y - ty as f32 * t);
                if a.stroke.kind.direct() {
                    if !a.before.contains_key(&(tx, ty)) {
                        let prev = self.doc.tile(&a.layer, tx, ty).cloned();
                        a.before.insert((tx, ty), prev);
                    }
                    let tile = self.doc.tile_mut(&a.layer, tx, ty);
                    if a.alpha_lock {
                        let mut tmp = Tile::default();
                        tmp.dab(lx, ly, d.radius, d.hardness, a.color, d.alpha, DabMode::Over);
                        tile.merge(&tmp, 1.0, false, true);
                    } else {
                        tile.dab(lx, ly, d.radius, d.hardness, a.color, d.alpha, d.mode);
                    }
                } else {
                    let mode = if a.stroke.kind == BrushKind::Eraser { DabMode::Max } else { d.mode };
                    a.buffer.entry((tx, ty)).or_default().dab(lx, ly, d.radius, d.hardness, a.color, d.alpha, mode);
                }
            }
            touched = (touched.0.min(d.x - d.radius - 1.0), touched.1.min(d.y - d.radius - 1.0), touched.2.max(d.x + d.radius + 1.0), touched.3.max(d.y + d.radius + 1.0));
        }
        if touched.0 < touched.2 {
            let off = a.off;
            self.invalidate_rect(touched.0 + off.0, touched.1 + off.1, touched.2 + off.0, touched.3 + off.1);
        }
    }

    fn finish(&mut self, at: Option<egui::Pos2>) {
        let Some(mut a) = self.active.take() else { return };
        let end = a.stroke.end(at.map(|p| Point { x: p.x - a.off.0, y: p.y - a.off.1, p: 1.0 }));
        self.active = Some(a);
        self.paint(end);
        let a = self.active.take().unwrap();
        let mut tiles = Vec::new();
        if a.stroke.kind.direct() {
            for (k, before) in a.before {
                let after = self.doc.tile(&a.layer, k.0, k.1).cloned();
                tiles.push((k, before, after));
            }
        } else {
            let s = self.settings[&a.stroke.kind];
            let erase = a.stroke.kind == BrushKind::Eraser;
            for (k, buf) in &a.buffer {
                let before = self.doc.tile(&a.layer, k.0, k.1).cloned();
                if erase && before.is_none() {
                    continue;
                }
                self.doc.tile_mut(&a.layer, k.0, k.1).merge(buf, s.opacity, erase, a.alpha_lock);
                let after = self.doc.tile(&a.layer, k.0, k.1).cloned();
                tiles.push((*k, before, after));
            }
            let t = TILE as f32;
            for k in a.buffer.keys() {
                self.invalidate_rect(k.0 as f32 * t + a.off.0, k.1 as f32 * t + a.off.1, (k.0 + 1) as f32 * t + a.off.0, (k.1 + 1) as f32 * t + a.off.1);
            }
        }
        if !tiles.is_empty() {
            self.doc.mark_layer(&a.layer);
            self.undo.push(Edit::Tiles { layer: a.layer.clone(), tiles });
            self.redo.clear();
            self.changed();
        }
    }

    // ── drawing the canvas ─────────────────────────────────────────────────

    fn composite(&mut self, ctx: &egui::Context, k: (i32, i32)) -> egui::TextureHandle {
        let t = TILE as i32;
        let overlay = self.active.as_ref().filter(|a| !a.stroke.kind.direct()).map(|a| Overlay { layer: &a.layer, tiles: &a.buffer, opacity: self.settings[&a.stroke.kind].opacity, erase: a.stroke.kind == BrushKind::Eraser, alpha_lock: a.alpha_lock });
        let opts = Options { background: true, onion: self.playing.is_none(), overlay, exporting: false };
        let px = compose::region(&mut self.doc, &mut self.pics, self.frame, k.0 * t, k.1 * t, TILE, TILE, &opts);
        let img = egui::ColorImage::from_rgba_premultiplied([TILE, TILE], &px);
        match self.comp.get_mut(&k) {
            Some(tex) => {
                tex.set(img, TILE_TEX);
                tex.clone()
            }
            None => {
                let tex = ctx.load_texture(format!("board:{}:{}:{}", self.path, k.0, k.1), img, TILE_TEX);
                self.comp.insert(k, tex.clone());
                tex
            }
        }
    }

    fn origin(&self, stage: egui::Rect) -> egui::Pos2 {
        if self.doc.infinite() {
            stage.center() + self.pan
        } else {
            stage.center() + self.pan - egui::vec2(self.doc.width() as f32, self.doc.height() as f32) * self.zoom / 2.0
        }
    }

    fn canvas_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) -> Option<String> {
        let mut toast = None;
        let stage = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(stage, Sense::click_and_drag());
        ui.painter().rect_filled(stage, 0.0, if p.dark { Color32::from_gray(28) } else { Color32::from_gray(205) });
        if self.fit && !self.doc.infinite() {
            let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
            self.zoom = ((stage.width() - 60.0) / w).min((stage.height() - 60.0) / h).clamp(0.05, 8.0);
            self.pan = egui::Vec2::ZERO;
        }
        let origin = self.origin(stage);
        let to_canvas = |pos: egui::Pos2, z: f32, o: egui::Pos2| egui::pos2((pos.x - o.x) / z, (pos.y - o.y) / z);
        // zoom with the wheel at the pointer
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta);
            let pinch = ui.input(|i| i.zoom_delta());
            let factor = if pinch != 1.0 { pinch } else { (1.0 + scroll.y * 0.0025).clamp(0.5, 2.0) };
            if factor != 1.0 {
                if let Some(ptr) = resp.hover_pos() {
                    let before = to_canvas(ptr, self.zoom, origin);
                    self.zoom = (self.zoom * factor).clamp(0.05, 16.0);
                    self.fit = false;
                    let o2 = self.origin(stage);
                    let after = egui::pos2(o2.x + before.x * self.zoom, o2.y + before.y * self.zoom);
                    self.pan += ptr - after;
                }
            }
        }
        let panning = self.tool == Tool::Hand || ui.input(|i| i.pointer.middle_down() || i.key_down(egui::Key::Space));
        if panning && resp.dragged() {
            self.pan += resp.drag_delta();
            self.fit = false;
        }
        let origin = self.origin(stage);
        // strokes, from every pointer event of the frame
        if !panning && self.playing.is_none() {
            if let Some(Tool::Brush(_)) = Some(self.tool) {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        if let Err(e) = self.begin(to_canvas(pos, self.zoom, origin)) {
                            toast = Some(e);
                        }
                    }
                }
                if self.active.is_some() {
                    let moves: Vec<egui::Pos2> = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::PointerMoved(p) = e { Some(*p) } else { None }).collect());
                    for m in moves {
                        let c = to_canvas(m, self.zoom, origin);
                        let off = self.active.as_ref().unwrap().off;
                        let dabs = self.active.as_mut().unwrap().stroke.add(Point { x: c.x - off.0, y: c.y - off.1, p: 1.0 });
                        self.paint(dabs);
                    }
                    if !ui.input(|i| i.pointer.primary_down()) {
                        let last = ui.input(|i| i.pointer.latest_pos()).map(|p| to_canvas(p, self.zoom, origin));
                        self.finish(last);
                    }
                }
            }
            if self.tool == Tool::Picker && resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let c = to_canvas(pos, self.zoom, origin);
                    let opts = Options { background: true, onion: false, overlay: None, exporting: false };
                    let px = compose::region(&mut self.doc, &mut self.pics, self.frame, c.x.floor() as i32, c.y.floor() as i32, 1, 1, &opts);
                    if px[3] > 0 {
                        let a = px[3] as u32;
                        self.color = [((px[0] as u32 * 255) / a) as u8, ((px[1] as u32 * 255) / a) as u8, ((px[2] as u32 * 255) / a) as u8];
                        self.tool = Tool::Brush(BrushKind::Pen);
                    }
                }
            }
        }
        // the page and its tiles
        let clip = ui.painter().with_clip_rect(stage);
        let (cw, ch) = (self.doc.width() as f32, self.doc.height() as f32);
        if !self.doc.infinite() {
            let page = egui::Rect::from_min_size(origin, egui::vec2(cw, ch) * self.zoom);
            clip.rect_filled(page.translate(egui::vec2(3.0, 4.0)), 0.0, Color32::from_black_alpha(40));
        }
        let view0 = to_canvas(stage.min, self.zoom, origin);
        let view1 = to_canvas(stage.max, self.zoom, origin);
        let (vx0, vy0, vx1, vy1) = if self.doc.infinite() { (view0.x, view0.y, view1.x, view1.y) } else { (view0.x.max(0.0), view0.y.max(0.0), view1.x.min(cw), view1.y.min(ch)) };
        let mut budget = if self.active.is_some() { 8 } else { 6 };
        let mut pending = false;
        if vx0 < vx1 && vy0 < vy1 {
            for k in doc::tiles_in(vx0, vy0, vx1, vy1).collect::<Vec<_>>() {
                let need = !self.comp.contains_key(&k) || self.stale.contains(&k);
                let tex = if need && budget > 0 {
                    budget -= 1;
                    self.stale.remove(&k);
                    Some(self.composite(ui.ctx(), k))
                } else {
                    if need {
                        pending = true;
                    }
                    self.comp.get(&k).cloned()
                };
                if let Some(tex) = tex {
                    let t = TILE as f32;
                    let r = egui::Rect::from_min_size(egui::pos2(origin.x + k.0 as f32 * t * self.zoom, origin.y + k.1 as f32 * t * self.zoom), egui::vec2(t, t) * self.zoom);
                    // bounded pages: only the part inside the page
                    let mut uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    let mut rr = r;
                    if !self.doc.infinite() {
                        let page = egui::Rect::from_min_size(origin, egui::vec2(cw, ch) * self.zoom);
                        let inter = r.intersect(page);
                        if inter.width() <= 0.0 || inter.height() <= 0.0 {
                            continue;
                        }
                        uv = egui::Rect::from_min_max(egui::pos2((inter.min.x - r.min.x) / r.width(), (inter.min.y - r.min.y) / r.height()), egui::pos2((inter.max.x - r.min.x) / r.width(), (inter.max.y - r.min.y) / r.height()));
                        rr = inter;
                    }
                    clip.image(tex.id(), rr, uv, Color32::WHITE);
                }
            }
        }
        if pending {
            ui.ctx().request_repaint();
        }
        // brush outline
        if let (Some(k), Some(ptr)) = (self.brush(), resp.hover_pos()) {
            if !panning {
                let r = self.settings[&k].size * self.zoom / 2.0;
                clip.circle_stroke(ptr, r.max(1.5), egui::Stroke::new(1.0_f32, Color32::from_gray(120)));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        } else if panning && resp.hovered() {
            ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        toast
    }

    // ── panels ─────────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        ui.horizontal_wrapped(|ui| {
            for (t, icon, tip) in [
                (Tool::Brush(BrushKind::Pencil), "✏", "Pencil (B)"),
                (Tool::Brush(BrushKind::Pen), "✒", "Pen (P)"),
                (Tool::Brush(BrushKind::Brush), "🖌", "Brush"),
                (Tool::Brush(BrushKind::Marker), "▮", "Marker"),
                (Tool::Brush(BrushKind::Eraser), "⌫", "Eraser (E)"),
                (Tool::Hand, "✋", "Hand (H, or hold Space)"),
                (Tool::Picker, "💧", "Pick a colour (I)"),
            ] {
                if ui.add(egui::Button::new(RichText::new(icon).size(16.0)).selected(self.tool == t).min_size(egui::vec2(30.0, 28.0))).on_hover_text(tip).clicked() {
                    self.tool = t;
                }
            }
            ui.separator();
            let mut c = Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
            if ui.color_edit_button_srgba(&mut c).changed() {
                self.color = [c.r(), c.g(), c.b()];
            }
            for sw in self.doc.palette().into_iter().take(12) {
                let col = ateliet_core::board::tile::parse_color(&sw);
                let (rect, r) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
                ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(col[0], col[1], col[2]));
                if col == self.color {
                    ui.painter().rect_stroke(rect.expand(2.0), 3.0, egui::Stroke::new(1.5_f32, p.accent), egui::StrokeKind::Outside);
                }
                if r.on_hover_text(&sw).clicked() {
                    self.color = col;
                    if matches!(self.tool, Tool::Brush(BrushKind::Eraser) | Tool::Hand | Tool::Picker) {
                        self.tool = Tool::Brush(BrushKind::Pen);
                    }
                }
            }
            ui.end_row();
            if let Some(k) = self.brush() {
                let s = self.settings.get_mut(&k).unwrap();
                ui.add(egui::Slider::new(&mut s.size, 1.0..=300.0).logarithmic(true).text("size").max_decimals(0));
                ui.add(egui::Slider::new(&mut s.opacity, 0.05..=1.0).text("opacity").max_decimals(2));
                ui.add(egui::Slider::new(&mut s.smoothing, 0.0..=0.95).text("steady").max_decimals(2));
                ui.end_row();
            }
            ui.separator();
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("↶")).on_hover_text("Undo (Ctrl+Z)").clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("↷")).on_hover_text("Redo (Ctrl+Shift+Z)").clicked() {
                self.redo();
            }
            let mut onion = self.doc.board["onion"]["enabled"].as_bool().unwrap_or(false);
            if ui.toggle_value(&mut onion, "🧅").on_hover_text("Onion skin").changed() {
                self.doc.board["onion"]["enabled"] = json!(onion);
                self.invalidate();
                self.dirty = true;
            }
            if ui.button("⛶").on_hover_text("Fit (0)").clicked() {
                self.fit = true;
            }
            ui.label(RichText::new(format!("{:.0}%", self.zoom * 100.0)).small().color(p.muted));
            if ui.button("Export PNG").on_hover_text("This frame as a PNG next to the storyboard").clicked() {
                let (w, h, px) = compose::frame_image(&mut self.doc, &mut self.pics, self.frame, 0, true);
                let png = ateliet_core::board::tile::encode_png(&px, w, h);
                let stem = self.abs.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let out = self.abs.with_file_name(format!("{stem} - {}.png", self.frame + 1));
                if let Err(e) = std::fs::write(&out, png) {
                    self.error = Some(e.to_string());
                }
            }
            if self.dirty {
                ui.label(RichText::new("●").color(p.muted)).on_hover_text("Unsaved changes (saved automatically)");
            }
        });
    }

    fn frames_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let n = self.doc.frame_count();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {}", n, if self.doc.kind() == "animatic" { "frames" } else { "pages" })).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗑").on_hover_text("Delete frame").clicked() && n > 1 {
                    let f = self.frame;
                    self.structure(|d| d.delete_frame(f));
                }
                if ui.small_button("⧉").on_hover_text("Duplicate frame").clicked() {
                    let f = self.frame;
                    let mut new = 0;
                    self.structure(|d| new = d.duplicate_frame(f));
                    self.frame = new;
                    self.invalidate();
                }
                if ui.small_button("+").on_hover_text("New frame after this one").clicked() {
                    let f = self.frame;
                    let mut new = 0;
                    self.structure(|d| new = d.add_frame(f + 1));
                    self.frame = new;
                    self.invalidate();
                }
            });
        });
        ui.horizontal(|ui| {
            let playing = self.playing.is_some();
            if ui.button(if playing { "⏹ Stop" } else { "▶ Play" }).clicked() {
                self.playing = if playing { None } else { Some((Instant::now(), self.frame)) };
                self.invalidate();
            }
            ui.label(RichText::new(format!("{} fps", self.doc.fps())).small().color(p.muted));
        });
        ui.separator();
        let mut go: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let mut made_thumb = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for i in 0..n {
                let id = self.doc.frame_id(i);
                let version = *self.versions.get(&id).unwrap_or(&0);
                let have = self.thumbs.get(&id).map(|(v, _)| *v);
                if have != Some(version) && !made_thumb && self.active.is_none() {
                    // one thumbnail per frame drawn, from the saved PNG or the picture
                    let png = if version == 0 { self.doc.thumbs.get(&id).cloned() } else { None };
                    let img = png.and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8()).unwrap_or_else(|| {
                        let (w, h, px) = compose::frame_image(&mut self.doc, &mut self.pics, i, 200, false);
                        image::RgbaImage::from_raw(w, h, px).unwrap_or_default()
                    });
                    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                    let tex = ui.ctx().load_texture(format!("frame:{id}"), ci, egui::TextureOptions::LINEAR);
                    self.thumbs.insert(id.clone(), (version, tex));
                    made_thumb = true;
                    ui.ctx().request_repaint();
                }
                let sel = i == self.frame;
                let w = ui.available_width() - 8.0;
                let aspect = self.doc.height() as f32 / self.doc.width().max(1) as f32;
                let h = (w * aspect).min(180.0);
                let (rect, r) = ui.allocate_exact_size(egui::vec2(w, h + 18.0), Sense::click());
                let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, h));
                ui.painter().rect_filled(img_rect, 3.0, p.bg_hover);
                if let Some((_, tex)) = self.thumbs.get(&id) {
                    ui.painter().image(tex.id(), img_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                if sel {
                    ui.painter().rect_stroke(img_rect, 3.0, egui::Stroke::new(2.5_f32, p.accent), egui::StrokeKind::Outside);
                }
                let f = self.doc.frame(i);
                let name = f["name"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("{}", i + 1));
                ui.painter().text(egui::pos2(rect.min.x, img_rect.max.y + 2.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(11.0), p.muted);
                if r.clicked() {
                    go = Some(i);
                }
                r.context_menu(|ui| {
                    if ui.button("Move up").clicked() && i > 0 {
                        swap = Some((i, i - 1));
                        ui.close_menu();
                    }
                    if ui.button("Move down").clicked() && i + 1 < n {
                        swap = Some((i, i + 1));
                        ui.close_menu();
                    }
                });
                ui.add_space(6.0);
            }
        });
        if let Some((a, b)) = swap {
            self.structure(|d| d.move_frame(a, b));
            self.frame = b;
        }
        if let Some(i) = go {
            if i != self.frame {
                self.frame = i;
                self.invalidate();
            }
        }
    }

    fn layers_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let f = self.frame;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layers").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("+").on_hover_text("New layer").clicked() {
                    let n = self.doc.frame_layers(f).len() + 1;
                    self.structure(|d| {
                        d.add_layer(f, &format!("Layer {n}"));
                    });
                }
            });
        });
        ui.separator();
        let ids = self.doc.frame_layers(f);
        let active = self.doc.active_layer(f);
        let mut changed_props = false;
        let mut set_active = None;
        let mut op: Option<(String, &str)> = None;
        for id in ids.iter().rev() {
            let sel = *id == active;
            let l = self.doc.layer(id).clone();
            egui::Frame::new().fill(if sel { p.accent.gamma_multiply(0.15) } else { Color32::TRANSPARENT }).corner_radius(4).inner_margin(egui::Margin::symmetric(4, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut vis = l["visible"].as_bool().unwrap_or(true);
                    if ui.add(egui::Button::new(if vis { "👁" } else { "○" }).frame(false)).on_hover_text("Show / hide").clicked() {
                        vis = !vis;
                        self.doc.layer_mut(id)["visible"] = json!(vis);
                        changed_props = true;
                    }
                    let kind = l["kind"].as_str().unwrap_or("raster");
                    let icon = match kind {
                        "image" => "🖼 ",
                        "trace" => "◩ ",
                        _ => "",
                    };
                    let name = format!("{icon}{}", l["name"].as_str().unwrap_or("Layer"));
                    if ui.add(egui::Label::new(RichText::new(name).color(p.text)).sense(Sense::click()).truncate()).clicked() {
                        set_active = Some(id.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if l["locked"].as_bool() == Some(true) {
                            ui.label("🔒");
                        }
                        if l["clip"].as_bool() == Some(true) {
                            ui.label("↴");
                        }
                    });
                });
                if sel {
                    let mut op_v = l["opacity"].as_f64().unwrap_or(1.0) as f32;
                    if ui.add(egui::Slider::new(&mut op_v, 0.0..=1.0).text("opacity").max_decimals(2)).changed() {
                        self.doc.layer_mut(id)["opacity"] = json!(op_v);
                        changed_props = true;
                    }
                    let blend = l["blend"].as_str().unwrap_or("source-over").to_string();
                    egui::ComboBox::from_id_salt(("blend", id)).selected_text(blend_name(&blend)).show_ui(ui, |ui| {
                        for (b, name) in BLENDS {
                            if ui.selectable_label(blend == *b, *name).clicked() {
                                self.doc.layer_mut(id)["blend"] = json!(b);
                                changed_props = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        let mut clip = l["clip"].as_bool().unwrap_or(false);
                        if ui.checkbox(&mut clip, "clip").on_hover_text("Draw only where the layer below has pixels").changed() {
                            self.doc.layer_mut(id)["clip"] = json!(clip);
                            changed_props = true;
                        }
                        let mut lock = l["locked"].as_bool().unwrap_or(false);
                        if ui.checkbox(&mut lock, "lock").changed() {
                            self.doc.layer_mut(id)["locked"] = json!(lock);
                            changed_props = true;
                        }
                        let mut al = l["alphaLock"].as_bool().unwrap_or(false);
                        if ui.checkbox(&mut al, "α").on_hover_text("Keep transparent parts transparent").changed() {
                            self.doc.layer_mut(id)["alphaLock"] = json!(al);
                            changed_props = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.small_button("▲").on_hover_text("Move up").clicked() {
                            op = Some((id.clone(), "up"));
                        }
                        if ui.small_button("▼").on_hover_text("Move down").clicked() {
                            op = Some((id.clone(), "down"));
                        }
                        if ui.small_button("🗑").on_hover_text("Delete layer").clicked() {
                            op = Some((id.clone(), "delete"));
                        }
                        let mut name = l["name"].as_str().unwrap_or_default().to_string();
                        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0)).changed() {
                            self.doc.layer_mut(id)["name"] = json!(name);
                            self.dirty = true;
                        }
                    });
                }
            });
        }
        if let Some(id) = set_active {
            self.doc.board["frames"][f]["active"] = json!(id);
            self.dirty = true;
        }
        if changed_props {
            self.doc.mark_frame(f);
            self.invalidate();
            self.changed();
        }
        if let Some((id, what)) = op {
            match what {
                "up" => self.structure(|d| d.move_layer(f, &id, 1)),
                "down" => self.structure(|d| d.move_layer(f, &id, -1)),
                _ => self.structure(|d| d.delete_layer(f, &id)),
            }
        }
    }

    /// Advance playback by the frames' durations.
    fn tick(&mut self, ctx: &egui::Context) {
        let Some((start, from)) = self.playing else { return };
        let fps = self.doc.fps() as f32;
        let elapsed = start.elapsed().as_secs_f32() * fps;
        let total: f32 = (0..self.doc.frame_count()).map(|i| self.doc.frame(i)["duration"].as_f64().unwrap_or(24.0) as f32).sum();
        let start_at: f32 = (0..from).map(|i| self.doc.frame(i)["duration"].as_f64().unwrap_or(24.0) as f32).sum();
        let mut t = (start_at + elapsed) % total.max(1.0);
        let mut i = 0;
        while i < self.doc.frame_count() {
            let d = self.doc.frame(i)["duration"].as_f64().unwrap_or(24.0) as f32;
            if t < d {
                break;
            }
            t -= d;
            i += 1;
        }
        if i != self.frame && i < self.doc.frame_count() {
            self.frame = i;
            self.invalidate();
        }
        ctx.request_repaint_after(Duration::from_millis((1000.0 / fps) as u64));
    }

    /// The whole editor; returns a message to show, if any.
    pub fn ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) -> Option<String> {
        self.tick(ui.ctx());
        // keys (when no text field has focus)
        if ui.ctx().memory(|m| m.focused().is_none()) {
            let k = |key| ui.input(|i| i.key_pressed(key) && i.modifiers.is_none());
            if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z))) {
                self.redo();
            } else if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z))) {
                self.undo();
            }
            if k(egui::Key::B) {
                self.tool = Tool::Brush(BrushKind::Pencil);
            }
            if k(egui::Key::P) {
                self.tool = Tool::Brush(BrushKind::Pen);
            }
            if k(egui::Key::E) {
                self.tool = Tool::Brush(BrushKind::Eraser);
            }
            if k(egui::Key::H) {
                self.tool = Tool::Hand;
            }
            if k(egui::Key::I) {
                self.tool = Tool::Picker;
            }
            if k(egui::Key::Num0) {
                self.fit = true;
            }
            if k(egui::Key::ArrowRight) && self.frame + 1 < self.doc.frame_count() {
                self.frame += 1;
                self.invalidate();
            }
            if k(egui::Key::ArrowLeft) && self.frame > 0 {
                self.frame -= 1;
                self.invalidate();
            }
        }
        // autosave two seconds after the last change
        if self.dirty && self.active.is_none() && self.last_change.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            self.save();
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(600));
        }
        // the toolbar wraps inside the panel; the rest is laid out from the panel's own rect
        let full = ui.available_rect_before_wrap();
        let bar = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(full), |ui| {
            ui.set_max_width(full.width());
            egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.set_max_width(full.width() - 16.0);
                self.toolbar(ui, p)
            });
        });
        let mut toast = None;
        let avail = egui::Rect::from_min_max(egui::pos2(full.min.x, bar.response.rect.max.y), full.max);
        let left_w = if self.doc.kind() == "canvas" { 0.0 } else { 170.0 };
        let right_w = 230.0;
        let left = egui::Rect::from_min_max(avail.min, egui::pos2(avail.min.x + left_w, avail.max.y));
        let right = egui::Rect::from_min_max(egui::pos2(avail.max.x - right_w, avail.min.y), avail.max);
        let center = egui::Rect::from_min_max(egui::pos2(left.max.x, avail.min.y), egui::pos2(right.min.x, avail.max.y));
        if left_w > 0.0 {
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left.shrink(6.0)), |ui| self.frames_ui(ui, p));
        }
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right.shrink(6.0)), |ui| {
            egui::ScrollArea::vertical().id_salt("layers").show(ui, |ui| self.layers_ui(ui, p));
        });
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(center), |ui| {
            if let Some(t) = self.canvas_ui(ui, p) {
                toast = Some(t);
            }
        });
        if let Some(e) = self.error.take() {
            toast = Some(e);
        }
        toast
    }
}

const BLENDS: &[(&str, &str)] = &[
    ("source-over", "Normal"),
    ("multiply", "Multiply"),
    ("darken", "Darken"),
    ("color-burn", "Color burn"),
    ("screen", "Screen"),
    ("lighten", "Lighten"),
    ("color-dodge", "Color dodge"),
    ("lighter", "Add"),
    ("overlay", "Overlay"),
    ("soft-light", "Soft light"),
    ("hard-light", "Hard light"),
    ("difference", "Difference"),
    ("exclusion", "Exclusion"),
];

fn blend_name(b: &str) -> &'static str {
    BLENDS.iter().find(|(k, _)| *k == b).map(|(_, n)| *n).unwrap_or("Normal")
}

/// Mipmapped, so thin lines stay continuous when zoomed far out.
const TILE_TEX: egui::TextureOptions = egui::TextureOptions { magnification: egui::TextureFilter::Linear, minification: egui::TextureFilter::Linear, wrap_mode: egui::TextureWrapMode::ClampToEdge, mipmap_mode: Some(egui::TextureFilter::Linear) };
