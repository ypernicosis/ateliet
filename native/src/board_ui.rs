//! The native storyboard editor: frames, layers, brushes, onion skin, undo,
//! playback, on the core's drawing engine and the same `.storyboard` files
//! as the web editor.

use ateliet_core::board::brush::{BrushKind, BrushSettings, Point, Stroke};
use ateliet_core::board::compose::{self, Options, Overlay, Pictures};
use ateliet_core::board::doc::{self, BoardDoc, TileKey};
use ateliet_core::board::tile::{DabMode, Tile, TILE};
use crate::icons;
use crate::kit;
use eframe::egui::{self, Color32, RichText, Sense};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod pages;
mod refs;
mod timeline;
mod tools;
pub use pages::NewBoard;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush(BrushKind),
    Hand,
    Picker,
    /// Page kinds: cut, make and reshape panels.
    Panel,
    /// Page kinds: balloons, captions and sound effects.
    Balloon,
    Fill,
    Lasso,
    Shape,
    Select,
    Move,
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
    pg: pages::PageState,
    /// A panel to zoom to (centre x, y, width, height).
    zoom_target: Option<(f32, f32, f32, f32)>,
    tl: timeline::TimelineState,
    rf: refs::RefState,
    ts: tools::ToolState,
    /// The view mirrored (a check of proportions, not a change).
    mirror: bool,
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

    /// A new storyboard file from a document made elsewhere (a kind's start).
    pub fn create_doc(path: &str, abs: PathBuf, mut doc: BoardDoc) -> Result<BoardEditor, String> {
        let mut pics = Pictures::default();
        doc.save(&abs, |d, i| compose::thumbnail_png(d, &mut pics, i))?;
        Ok(Self::with(path, abs, doc))
    }

    fn with(path: &str, abs: PathBuf, doc: BoardDoc) -> BoardEditor {
        let pg = pages::PageState::new(doc.kind());
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
            pg,
            zoom_target: None,
            tl: timeline::TimelineState::default(),
            rf: refs::RefState::default(),
            ts: tools::ToolState::default(),
            mirror: false,
        }
    }

    pub fn save(&mut self) {
        self.commit_float();
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
            Err(e) => self.error = Some(format!("Não foi possível salvar: {e}")),
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
        ui.painter().rect_filled(stage, 0.0, if p.dark { Color32::from_gray(22) } else { Color32::from_rgb(0xec, 0xec, 0xe9) });
        if self.fit && !self.doc.infinite() {
            let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
            self.zoom = ((stage.width() - 60.0) / w).min((stage.height() - 60.0) / h).clamp(0.05, 8.0);
            self.pan = egui::Vec2::ZERO;
        }
        if let Some((cx, cy, w, h)) = self.zoom_target.take() {
            self.zoom = ((stage.width() - 60.0) / w).min((stage.height() - 60.0) / h).clamp(0.05, 8.0);
            self.fit = false;
            let size = egui::vec2(self.doc.width() as f32, self.doc.height() as f32) * self.zoom;
            self.pan = size / 2.0 - egui::vec2(cx, cy) * self.zoom;
        }
        let origin = self.origin(stage);
        let mirror = self.mirror && !self.is_pages() && !self.doc.infinite();
        let mw = self.doc.width() as f32;
        let to_canvas = move |pos: egui::Pos2, z: f32, o: egui::Pos2| {
            let x = (pos.x - o.x) / z;
            egui::pos2(if mirror { mw - x } else { x }, (pos.y - o.y) / z)
        };
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
        if !panning && self.playing.is_none() && matches!(self.tool, Tool::Panel | Tool::Balloon) && self.is_pages() {
            self.page_input(ui, &resp, origin);
        }
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
            let z = self.zoom;
            let tc = move |pos: egui::Pos2| to_canvas(pos, z, origin);
            if let Some(e) = self.tool_input(ui, &resp, &tc) {
                toast = Some(e);
            }
            let alt_pick = ui.input(|i| i.modifiers.alt) && matches!(self.tool, Tool::Brush(_) | Tool::Fill | Tool::Shape);
            if (self.tool == Tool::Picker || alt_pick) && resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let c = to_canvas(pos, self.zoom, origin);
                    let opts = Options { background: true, onion: false, overlay: None, exporting: false };
                    let px = compose::region(&mut self.doc, &mut self.pics, self.frame, c.x.floor() as i32, c.y.floor() as i32, 1, 1, &opts);
                    if px[3] > 0 {
                        let a = px[3] as u32;
                        self.color = [((px[0] as u32 * 255) / a) as u8, ((px[1] as u32 * 255) / a) as u8, ((px[2] as u32 * 255) / a) as u8];
                        if !alt_pick {
                            self.tool = Tool::Brush(BrushKind::Pen);
                        }
                    }
                }
            }
        }
        // the page and its tiles
        let clip = ui.painter().with_clip_rect(stage);
        let (cw, ch) = (self.doc.width() as f32, self.doc.height() as f32);
        if !self.doc.infinite() {
            let page = egui::Rect::from_min_size(origin, egui::vec2(cw, ch) * self.zoom);
            clip.add(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }) }.as_shape(page, 2.0));
        }
        let (a0, a1) = (to_canvas(stage.min, self.zoom, origin), to_canvas(stage.max, self.zoom, origin));
        let view0 = egui::pos2(a0.x.min(a1.x), a0.y.min(a1.y));
        let view1 = egui::pos2(a0.x.max(a1.x), a0.y.max(a1.y));
        let (vx0, vy0, vx1, vy1) = if self.doc.infinite() { (view0.x, view0.y, view1.x, view1.y) } else { (view0.x.max(0.0), view0.y.max(0.0), view1.x.min(cw), view1.y.min(ch)) };
        self.ts.view = (vx0, vy0, vx1, vy1);
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
                    if mirror {
                        // drawn as in a mirror: the tile's place and its picture flipped
                        let pw = cw * self.zoom;
                        let x0 = origin.x + pw - (rr.max.x - origin.x);
                        rr = egui::Rect::from_min_size(egui::pos2(x0, rr.min.y), rr.size());
                        let mut mesh = egui::Mesh::with_texture(tex.id());
                        mesh.add_rect_with_uv(rr, uv, Color32::WHITE);
                        // the picture's columns in reverse
                        for v in mesh.vertices.iter_mut() {
                            v.uv.x = uv.min.x + uv.max.x - v.uv.x;
                        }
                        clip.add(egui::Shape::mesh(mesh));
                        continue;
                    }
                    clip.image(tex.id(), rr, uv, Color32::WHITE);
                }
            }
        }
        if pending {
            ui.ctx().request_repaint();
        }
        if self.is_pages() {
            self.page_draw(ui, &clip, origin);
        }
        if !self.doc.infinite() {
            self.draw_guides(&clip, origin);
        }
        {
            let z = self.zoom;
            let fc = move |c: egui::Pos2| egui::pos2(origin.x + (if mirror { mw - c.x } else { c.x }) * z, origin.y + c.y * z);
            self.tool_draw(ui, &clip, &fc);
        }
        if mirror {
            clip.text(stage.left_top() + egui::vec2(12.0, 10.0), egui::Align2::LEFT_TOP, "Vista espelhada (F)", egui::FontId::proportional(11.0), p.faint);
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
        let pages_kind = self.is_pages();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            // tools: a segmented group
            egui::Frame::new().fill(p.bg_secondary).corner_radius(9.0).inner_margin(3.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (t, glyph, tip) in [
                    (Tool::Brush(BrushKind::Pencil), icons::PENCIL, "Lápis (B)"),
                    (Tool::Brush(BrushKind::Pen), icons::PEN_TOOL, "Caneta (P)"),
                    (Tool::Brush(BrushKind::Brush), icons::PAINTBRUSH, "Pincel"),
                    (Tool::Brush(BrushKind::Marker), icons::HIGHLIGHTER, "Marcador"),
                    (Tool::Brush(BrushKind::Eraser), icons::ERASER, "Borracha (E)"),
                    (Tool::Fill, icons::PAINT_BUCKET, "Balde (G)"),
                    (Tool::Lasso, icons::LASSO, "Laço de preenchimento (L) · Alt apaga"),
                    (Tool::Shape, icons::SHAPES, "Formas e setas de câmera (U) · Shift alinha, Alt desenha do centro"),
                    (Tool::Select, icons::SQUARE_DASHED, "Seleção (M) · Shift: quadrado ou círculo · Delete apaga · Ctrl+D tira"),
                    (Tool::Move, icons::MOVE, "Mover e escalar (V): arraste a seleção; os cantos escalam (Shift mantém a proporção); Enter aplica, Esc desfaz. Sem seleção, move a camada"),
                    (Tool::Hand, icons::HAND, "Mão (H, ou segure Espaço)"),
                    (Tool::Picker, icons::PIPETTE, "Conta-gotas (I)"),
                    (Tool::Panel, icons::SQUARE_SPLIT_HORIZONTAL, "Quadros (J): arraste através de um quadro para cortá-lo; num espaço vazio, um quadro novo"),
                    (Tool::Balloon, icons::MESSAGE_CIRCLE, "Balões (W): arraste ou clique e digite"),
                ] {
                    if matches!(t, Tool::Panel | Tool::Balloon) && !pages_kind {
                        continue;
                    }
                    let sel = self.tool == t;
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(30.0, 28.0), Sense::click());
                    if sel {
                        ui.painter().rect(rect, 7.0, p.bg, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                    } else if r.hovered() {
                        ui.painter().rect_filled(rect, 7.0, p.bg_hover);
                    }
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(15.0), if sel { p.text } else { p.muted });
                    if r.on_hover_text(tip).clicked() {
                        self.tool = t;
                    }
                }
            });
            ui.add_space(6.0);
            // colour: the current one as a disc that opens a picker, then the palette
            let c = Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
            let (rect, r) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
            ui.painter().circle_filled(rect.center(), 11.0, c);
            ui.painter().circle_stroke(rect.center(), 13.0, egui::Stroke::new(1.0_f32, p.border_strong));
            let popup = ui.make_persistent_id("board-colour");
            if r.on_hover_text("Cor").clicked() {
                ui.memory_mut(|m| m.toggle_popup(popup));
            }
            egui::popup_below_widget(ui, popup, &ui.interact(rect, popup.with("anchor"), Sense::hover()), egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                let mut cc = c;
                if egui::color_picker::color_picker_color32(ui, &mut cc, egui::color_picker::Alpha::Opaque) {
                    self.color = [cc.r(), cc.g(), cc.b()];
                }
            });
            for sw in self.doc.palette().into_iter().take(10) {
                let col = ateliet_core::board::tile::parse_color(&sw);
                let (rect, r) = ui.allocate_exact_size(egui::vec2(18.0, 28.0), Sense::click());
                let cc = Color32::from_rgb(col[0], col[1], col[2]);
                ui.painter().circle_filled(rect.center(), if r.hovered() { 7.5 } else { 6.5 }, cc);
                if col == self.color {
                    ui.painter().circle_stroke(rect.center(), 9.5, egui::Stroke::new(1.5_f32, p.text));
                }
                if r.on_hover_text(&sw).clicked() {
                    self.color = col;
                    if matches!(self.tool, Tool::Brush(BrushKind::Eraser) | Tool::Hand | Tool::Picker) {
                        self.tool = Tool::Brush(BrushKind::Pen);
                    }
                }
            }
            ui.add_space(8.0);
            if let Some(k) = self.brush() {
                let s = self.settings.get_mut(&k).unwrap();
                ui.label(kit::icon(icons::CIRCLE, 12.0).color(p.faint)).on_hover_text("Tamanho");
                ui.spacing_mut().slider_width = 90.0;
                ui.add(egui::Slider::new(&mut s.size, 1.0..=300.0).logarithmic(true).show_value(false));
                ui.label(RichText::new(format!("{:.0}", s.size)).size(12.0).color(p.muted));
                let more = ui.make_persistent_id("brush-more");
                let r = kit::icon_button(ui, icons::SLIDERS_HORIZONTAL, "Opacidade e estabilização", false, p);
                if r.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(more));
                }
                egui::popup_below_widget(ui, more, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(220.0);
                    ui.spacing_mut().slider_width = 150.0;
                    ui.label(RichText::new("Opacidade").size(12.0).color(p.muted));
                    ui.add(egui::Slider::new(&mut s.opacity, 0.05..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Estabilização").size(12.0).color(p.muted));
                    ui.add(egui::Slider::new(&mut s.smoothing, 0.0..=0.95).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Dureza").size(12.0).color(p.muted));
                    ui.add(egui::Slider::new(&mut s.hardness, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                });
            } else {
                self.tool_options(ui, p);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let kind = self.doc.kind().to_string();
                if kind == "webtoon" && kit::icon_button(ui, icons::SQUARE_SPLIT_HORIZONTAL, "Exportar em fatias para envio (PNG)", false, p).clicked() {
                    self.export_webtoon_slices();
                }
                if pages_kind && kit::icon_button(ui, icons::FILE_DOWN, "Exportar as páginas em PDF (tamanho de impressão, com sangria)", false, p).clicked() {
                    self.export_pages_pdf();
                } else if !pages_kind && kind != "canvas" && kit::icon_button(ui, icons::FILE_DOWN, "Prancha em PDF (A4 deitado, seis planos por página, com as notas)", false, p).clicked() {
                    self.export_presentation_pdf();
                }
                if pages_kind && kit::icon_button(ui, icons::DOWNLOAD, "Exportar esta página como PNG", false, p).clicked() {
                    self.export_page_png();
                } else if !pages_kind && kit::icon_button(ui, icons::DOWNLOAD, "Exportar este quadro como PNG", false, p).clicked() {
                    let (w, h, px) = compose::frame_image(&mut self.doc, &mut self.pics, self.frame, 0, true);
                    let png = ateliet_core::board::tile::encode_png(&px, w, h);
                    let stem = self.abs.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let out = self.abs.with_file_name(format!("{stem} - {}.png", self.frame + 1));
                    match std::fs::write(&out, png) {
                        Ok(()) => self.error = Some(format!("Exportado: {}", out.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{:.0}%", self.zoom * 100.0)).size(12.0).color(p.faint));
                if kit::icon_button(ui, icons::MAXIMIZE, "Ajustar à tela (0)", false, p).clicked() {
                    self.fit = true;
                }
                self.guide_menu(ui, p);
                let onion = self.doc.board["onion"]["enabled"].as_bool().unwrap_or(false);
                if kit::icon_button(ui, icons::LAYERS_2, "Papel de cebola (O)", onion, p).clicked() {
                    self.doc.board["onion"]["enabled"] = json!(!onion);
                    self.invalidate();
                    self.dirty = true;
                }
                ui.add_space(6.0);
                ui.add_enabled_ui(!self.redo.is_empty(), |ui| {
                    if kit::icon_button(ui, icons::REDO_2, "Refazer (Ctrl+Shift+Z)", false, p).clicked() {
                        self.redo();
                    }
                });
                ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                    if kit::icon_button(ui, icons::UNDO_2, "Desfazer (Ctrl+Z)", false, p).clicked() {
                        self.undo();
                    }
                });
            });
        });
    }

    fn frames_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let n = self.doc.frame_count();
        ui.horizontal(|ui| {
            let units = ateliet_core::board::pages::kind_info(self.doc.kind()).units;
            let mut t = units.to_string();
            if let Some(c) = t.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            ui.label(RichText::new(t).font(kit::medium(13.0)).color(p.text));
            ui.label(RichText::new(n.to_string()).size(12.0).color(p.faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if kit::icon_button_sized(ui, icons::TRASH_2, "Excluir quadro", false, p, 24.0, 13.0).clicked() && n > 1 {
                    let f = self.frame;
                    self.structure(|d| d.delete_frame(f));
                }
                if kit::icon_button_sized(ui, icons::COPY, "Duplicar quadro", false, p, 24.0, 13.0).clicked() {
                    let f = self.frame;
                    let mut new = 0;
                    self.structure(|d| new = d.duplicate_frame(f));
                    self.frame = new;
                    self.invalidate();
                }
                if kit::icon_button_sized(ui, icons::PLUS, "Novo quadro depois deste", false, p, 24.0, 14.0).clicked() {
                    let f = self.frame;
                    let mut new = 0;
                    self.structure(|d| new = ateliet_core::board::pages::add_page(d, f + 1, "one"));
                    self.frame = new;
                    self.invalidate();
                }
            });
        });
        ui.add_space(4.0);
        if !self.is_pages() {
        ui.horizontal(|ui| {
            let playing = self.playing.is_some();
            if kit::button(ui, Some(if playing { icons::SQUARE } else { icons::PLAY }), if playing { "Parar" } else { "Reproduzir" }, p).clicked() {
                self.playing = if playing { None } else { Some((Instant::now(), self.frame)) };
                self.invalidate();
            }
            ui.label(RichText::new(format!("{} qps", self.doc.fps())).size(12.0).color(p.faint));
        });
        }
        ui.add_space(10.0);
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
                let (rect, r) = ui.allocate_exact_size(egui::vec2(w, h + 20.0), Sense::click());
                let img_rect = egui::Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(w - 6.0, h - 6.0));
                ui.painter().rect_filled(img_rect, 6.0, p.bg_hover);
                if let Some((_, tex)) = self.thumbs.get(&id) {
                    ui.painter().add(egui::epaint::RectShape::filled(img_rect, 6.0, Color32::WHITE).with_texture(tex.id(), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))));
                }
                ui.painter().rect_stroke(img_rect, 6.0, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                if sel {
                    ui.painter().rect_stroke(img_rect.expand(2.5), 8.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                } else if r.hovered() {
                    ui.painter().rect_stroke(img_rect.expand(2.5), 8.0, egui::Stroke::new(1.0_f32, p.border_strong), egui::StrokeKind::Inside);
                }
                let f = self.doc.frame(i);
                let name = f["name"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("{}", i + 1));
                ui.painter().text(egui::pos2(img_rect.min.x + 1.0, img_rect.max.y + 6.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(11.5), if sel { p.text } else { p.faint });
                if r.clicked() {
                    go = Some(i);
                }
                r.context_menu(|ui| {
                    if kit::menu_item(ui, icons::ARROW_UP, "Mover para cima", p) && i > 0 {
                        swap = Some((i, i - 1));
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, icons::ARROW_DOWN, "Mover para baixo", p) && i + 1 < n {
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
                self.finish_editing();
                self.pg.sel_panel = None;
                self.pg.sel_balloon = None;
                self.frame = i;
                self.invalidate();
            }
        }
    }

    fn layers_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let f = self.frame;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Camadas").font(kit::medium(13.0)).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button_sized(ui, icons::PLUS, "Nova camada", false, p, 24.0, 14.0).clicked() {
                    let n = self.doc.frame_layers(f).len() + 1;
                    self.structure(|d| {
                        d.add_layer(f, &format!("Camada {n}"));
                    });
                }
            });
        });
        ui.add_space(6.0);
        let ids = self.doc.frame_layers(f);
        let active = self.doc.active_layer(f);
        let mut changed_props = false;
        let mut set_active = None;
        let mut op: Option<(String, &str)> = None;
        ui.spacing_mut().item_spacing.y = 2.0;
        for id in ids.iter().rev() {
            let sel = *id == active;
            let l = self.doc.layer(id).clone();
            let w = ui.available_width();
            let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 32.0), Sense::click());
            ui.painter().rect_filled(rect, 7.0, if sel { p.bg_active } else if r.hovered() { p.bg_hover } else { Color32::TRANSPARENT });
            let vis = l["visible"].as_bool().unwrap_or(true);
            let eye = egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, rect.center().y), egui::vec2(24.0, 24.0));
            let er = ui.interact(eye, ui.id().with(("eye", id)), Sense::click());
            ui.painter().text(eye.center(), egui::Align2::CENTER_CENTER, if vis { icons::EYE } else { icons::EYE_OFF }, kit::icon_font(13.0), if er.hovered() { p.text } else if vis { p.muted } else { p.faint });
            if er.on_hover_text("Mostrar / esconder").clicked() {
                self.doc.layer_mut(id)["visible"] = json!(!vis);
                changed_props = true;
            }
            let kind = l["kind"].as_str().unwrap_or("raster");
            let name = l["name"].as_str().unwrap_or("Camada").to_string();
            let glyph = match kind {
                "image" => Some(icons::IMAGE),
                "trace" => Some(icons::SCAN_SEARCH),
                _ => None,
            };
            let mut x = rect.left() + 34.0;
            if let Some(g) = glyph {
                ui.painter().text(egui::pos2(x + 6.0, rect.center().y), egui::Align2::CENTER_CENTER, g, kit::icon_font(12.0), p.faint);
                x += 18.0;
            }
            ui.painter().text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, &name, if sel { kit::medium(13.0) } else { egui::FontId::proportional(13.0) }, if vis { p.text } else { p.faint });
            let mut rx = rect.right() - 12.0;
            for (on, g) in [(l["locked"].as_bool() == Some(true), icons::LOCK), (l["clip"].as_bool() == Some(true), icons::CORNER_DOWN_RIGHT)] {
                if on {
                    ui.painter().text(egui::pos2(rx, rect.center().y), egui::Align2::RIGHT_CENTER, g, kit::icon_font(12.0), p.faint);
                    rx -= 18.0;
                }
            }
            if r.clicked() {
                set_active = Some(id.clone());
            }
            if sel {
                egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 6, top: 6, bottom: 10 }).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    let mut op_v = l["opacity"].as_f64().unwrap_or(1.0) as f32;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Opacidade").size(12.0).color(p.muted));
                        ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(50.0);
                        if ui.add(egui::Slider::new(&mut op_v, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed() {
                            self.doc.layer_mut(id)["opacity"] = json!(op_v);
                            changed_props = true;
                        }
                    });
                    let blend = l["blend"].as_str().unwrap_or("source-over").to_string();
                    egui::ComboBox::from_id_salt(("blend", id)).selected_text(blend_name(&blend)).width(ui.available_width()).show_ui(ui, |ui| {
                        for (b, name) in BLENDS {
                            if ui.selectable_label(blend == *b, *name).clicked() {
                                self.doc.layer_mut(id)["blend"] = json!(b);
                                changed_props = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (key, label, tip) in [("clip", "Recorte", "Desenha só onde a camada de baixo tem pixels"), ("locked", "Travar", "Impede desenhar nesta camada"), ("alphaLock", "Alfa", "Mantém transparente o que é transparente")] {
                            let on = l[key].as_bool().unwrap_or(false);
                            if ui.add(egui::Button::new(RichText::new(label).size(12.0).color(if on { p.on_accent } else { p.muted })).fill(if on { p.accent_strong } else { Color32::TRANSPARENT }).corner_radius(12.0)).on_hover_text(tip).clicked() {
                                self.doc.layer_mut(id)[key] = json!(!on);
                                changed_props = true;
                            }
                        }
                    });
                    if kind == "trace" && self.trace_props(ui, id, p) {
                        changed_props = true;
                    }
                    ui.horizontal(|ui| {
                        let mut name = l["name"].as_str().unwrap_or_default().to_string();
                        if ui.add(egui::TextEdit::singleline(&mut name).desired_width((ui.available_width() - 90.0).max(60.0)).margin(egui::Margin::symmetric(8, 4))).changed() {
                            self.doc.layer_mut(id)["name"] = json!(name);
                            self.dirty = true;
                        }
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if kit::icon_button_sized(ui, icons::TRASH_2, "Excluir camada", false, p, 26.0, 13.0).clicked() {
                            op = Some((id.clone(), "delete"));
                        }
                        if kit::icon_button_sized(ui, icons::ARROW_DOWN, "Descer", false, p, 26.0, 13.0).clicked() {
                            op = Some((id.clone(), "down"));
                        }
                        if kit::icon_button_sized(ui, icons::ARROW_UP, "Subir", false, p, 26.0, 13.0).clicked() {
                            op = Some((id.clone(), "up"));
                        }
                    });
                });
            }
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
            if k(egui::Key::F) {
                self.mirror = !self.mirror;
                self.invalidate();
            }
            if k(egui::Key::O) && self.has_timeline() {
                let on = self.doc.board["onion"]["enabled"].as_bool().unwrap_or(false);
                self.doc.board["onion"]["enabled"] = json!(!on);
                self.invalidate();
                self.dirty = true;
            }
            if self.is_pages() {
                if k(egui::Key::J) {
                    self.tool = Tool::Panel;
                }
                if k(egui::Key::W) {
                    self.tool = Tool::Balloon;
                }
                if matches!(self.tool, Tool::Panel | Tool::Balloon) {
                    self.page_keys(ui);
                }
            }
            if k(egui::Key::ArrowRight) && self.frame + 1 < self.doc.frame_count() {
                self.frame += 1;
                self.invalidate();
            }
            if k(egui::Key::ArrowLeft) && self.frame > 0 {
                self.frame -= 1;
                self.invalidate();
            }
            // Alt+← / Alt+→: the reference beside the drawing
            let alt = |key| ui.input(|i| i.key_pressed(key) && i.modifiers.alt && !i.modifiers.command);
            if alt(egui::Key::ArrowRight) {
                self.step_ref(1);
            }
            if alt(egui::Key::ArrowLeft) {
                self.step_ref(-1);
            }
            self.trace_keys(ui);
            self.tool_keys(ui);
        }
        // autosave two seconds after the last change
        if self.dirty && self.active.is_none() && self.ts.float.is_none() && self.last_change.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            self.save();
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(600));
        }
        // the toolbar wraps inside the panel; the rest is laid out from the panel's own rect
        let full = ui.available_rect_before_wrap();
        let bar = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(full), |ui| {
            ui.set_max_width(full.width());
            egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 }).show(ui, |ui| {
                ui.set_width(full.width() - 24.0);
                self.toolbar(ui, p)
            });
        });
        let mut toast = None;
        let avail = egui::Rect::from_min_max(egui::pos2(full.min.x, bar.response.rect.max.y), full.max);
        ui.painter().hline(avail.x_range(), avail.top() - 0.5, egui::Stroke::new(1.0_f32, p.border));
        let left_w = if self.doc.kind() == "canvas" { 0.0 } else { 176.0 };
        let right_w = 240.0;
        let left = egui::Rect::from_min_max(avail.min, egui::pos2(avail.min.x + left_w, avail.max.y));
        let right = egui::Rect::from_min_max(egui::pos2(avail.max.x - right_w, avail.min.y), avail.max);
        let center = egui::Rect::from_min_max(egui::pos2(left.max.x, avail.min.y), egui::pos2(right.min.x, avail.max.y));
        if left_w > 0.0 {
            ui.painter().rect_filled(left, 0.0, p.bg_secondary);
            ui.painter().vline(left.right() - 0.5, left.y_range(), egui::Stroke::new(1.0_f32, p.border));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left.shrink2(egui::vec2(12.0, 12.0))), |ui| self.frames_ui(ui, p));
        }
        ui.painter().rect_filled(right, 0.0, p.bg_secondary);
        ui.painter().vline(right.left() + 0.5, right.y_range(), egui::Stroke::new(1.0_f32, p.border));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right.shrink2(egui::vec2(12.0, 12.0))), |ui| {
            egui::ScrollArea::vertical().id_salt("layers").show(ui, |ui| {
                if self.doc.kind() != "canvas" {
                    self.refs_ui(ui, p);
                }
                if self.is_pages() {
                    self.page_ui(ui, p);
                } else if self.doc.kind() != "canvas" {
                    self.notes_ui(ui, p);
                }
                self.layers_ui(ui, p)
            });
        });
        let (canvas, strip) = if self.has_timeline() {
            let sh = 104.0;
            (egui::Rect::from_min_max(center.min, egui::pos2(center.max.x, center.max.y - sh)), Some(egui::Rect::from_min_max(egui::pos2(center.min.x, center.max.y - sh), center.max)))
        } else {
            (center, None)
        };
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(canvas), |ui| {
            if let Some(t) = self.canvas_ui(ui, p) {
                toast = Some(t);
            }
        });
        if let Some(s) = strip {
            ui.painter().rect_filled(s, 0.0, p.bg);
            ui.painter().hline(s.x_range(), s.top() + 0.5, egui::Stroke::new(1.0_f32, p.border));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(s.shrink2(egui::vec2(10.0, 6.0))), |ui| self.timeline_ui(ui, p));
        }
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
