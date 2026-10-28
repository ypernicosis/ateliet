//! The Quadro editor: Kanban columns, loose cards and Excalidraw-style
//! drawings on one infinite canvas, over the core's model and formats
//! (`ateliet_core::quadro`). Kanban boards, `.excalidraw.md` and
//! `.excalidraw` files open here and are saved back in their own format.

mod draw;
mod edit;
mod input;
mod panel;

pub use draw::Pics;

use crate::host::Caches;
use crate::vault::Vault;
use ateliet_core::quadro::element::{Element, Style};
use ateliet_core::quadro::rough::{self, Geometry};
use ateliet_core::quadro::{Board, FileRef, Format};
use eframe::egui::{self, pos2, Pos2, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Screen = origin + world × zoom.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub origin: Pos2,
    pub zoom: f32,
}

impl View {
    pub fn s(&self, p: (f32, f32)) -> Pos2 {
        pos2(self.origin.x + p.0 * self.zoom, self.origin.y + p.1 * self.zoom)
    }
    pub fn w(&self, q: Pos2) -> (f32, f32) {
        ((q.x - self.origin.x) / self.zoom, (q.y - self.origin.y) / self.zoom)
    }
}

/// An element's rough geometry with its fills cut into triangles.
pub struct Geo {
    pub g: Geometry,
    pub tris: Vec<Vec<[usize; 3]>>,
}

/// The geometry of an element, made again only when it changed.
pub fn geo_of<'a>(cache: &'a mut HashMap<String, (u64, Geo)>, e: &Element) -> &'a Geo {
    let fresh = cache.get(e.id()).is_some_and(|(r, _)| *r == e.rev);
    if !fresh {
        let g = rough::geometry(e);
        let tris = g.fills.iter().map(|f| rough::triangulate(&f.pts)).collect();
        cache.insert(e.id().to_string(), (e.rev, Geo { g, tris }));
    }
    &cache[e.id()].1
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Hand,
    Rect,
    Diamond,
    Ellipse,
    Arrow,
    Line,
    Draw,
    Text,
    Eraser,
    Card,
    Lane,
}

/// Something selected.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Sel {
    El(String),
    Card(String),
    Lane(String),
}

/// What the editor asks the app to do.
pub enum Out {
    /// Open a link (a note) from the quadro, in a new tab.
    Open(String),
    Toast(String),
    /// Files were added to the vault.
    Rescan,
}

/// What the app lends the editor for a frame.
pub struct Cx<'a> {
    pub vault: &'a Vault,
    pub caches: &'a mut Caches,
    pub palette: crate::theme::Palette,
}

/// Pictures from the vault (and an image element's own data URL).
pub struct VaultPics<'a> {
    pub vault: &'a Vault,
    pub caches: &'a mut Caches,
    pub source: &'a str,
    pub files: &'a [(String, FileRef)],
    pub data: &'a mut HashMap<String, Option<(egui::TextureHandle, Vec2)>>,
}

impl Pics for VaultPics<'_> {
    fn tex(&mut self, ctx: &egui::Context, key: &str) -> Option<(egui::TextureId, Vec2)> {
        let Some(id) = key.strip_prefix("file:") else {
            return crate::host::image(ctx, self.vault, self.caches, key, self.source);
        };
        let f = self.files.iter().find(|(k, _)| k == id).map(|(_, f)| f)?;
        if let FileRef::Data(v) = f {
            let k = format!("{}#{id}", self.source);
            if !self.data.contains_key(&k) {
                let img = v["dataURL"].as_str().and_then(decode_data_url).map(|img| {
                    let size = [img.width() as usize, img.height() as usize];
                    let c = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    (ctx.load_texture(k.clone(), c, egui::TextureOptions::LINEAR), egui::vec2(size[0] as f32, size[1] as f32))
                });
                self.data.insert(k.clone(), img);
            }
            return self.data[&k].as_ref().map(|(t, s)| (t.id(), *s));
        }
        let path = f.vault_path()?;
        crate::host::image(ctx, self.vault, self.caches, &path, self.source)
    }
}

pub fn decode_data_url(url: &str) -> Option<image::RgbaImage> {
    let bytes = ateliet_core::quadro::data_url_bytes(url)?;
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width() > 2000 || img.height() > 2000 { img.thumbnail(2000, 2000) } else { img };
    Some(img.to_rgba8())
}

/// A pointer gesture in progress.
enum Drag {
    Pan,
    Marquee((f32, f32)),
    /// Moving the selection; `moved` once it went somewhere.
    Move { last: (f32, f32), moved: bool },
    /// A card taken out of its place: where it was grabbed, where it is.
    Card { id: String, grab: (f32, f32), at: (f32, f32), moved: bool },
    Resize { handle: usize, start: (f32, f32), box0: (f32, f32, f32, f32), orig: Board },
    Rotate { id: String },
    Point { id: String, index: usize },
    Create { id: String, start: (f32, f32) },
    Draw { id: String },
    Erase(HashSet<String>),
}

/// A text being typed in place.
pub enum Editing {
    Text { id: String, buf: String, new: bool, orig: String },
    Card { id: String, buf: String, new: bool },
    Lane { id: String, buf: String, new: bool },
}

pub struct QuadroEditor {
    pub path: String,
    abs: PathBuf,
    pub board: Board,
    undo: Vec<Board>,
    redo: Vec<Board>,
    pub dirty: bool,
    last_change: Option<Instant>,
    disk_mtime: Option<SystemTime>,
    last_check: Instant,
    pub error: Option<String>,
    pan: Vec2,
    zoom: f32,
    fit: bool,
    tool: Tool,
    /// Keep the tool after drawing (else back to selection).
    lock: bool,
    style: Style,
    sel: Vec<Sel>,
    drag: Option<Drag>,
    editing: Option<Editing>,
    focus_edit: bool,
    geos: HashMap<String, (u64, Geo)>,
    data_tex: HashMap<String, Option<(egui::TextureHandle, Vec2)>>,
    scene: draw::Scene,
    /// Where bindable things were last frame (arrows follow when they move).
    prev_targets: HashMap<String, [f32; 5]>,
    /// A menu open at a point: for what.
    menu: Option<(Option<Sel>, Pos2, (f32, f32))>,
    menu_text: String,
    menu_month: Option<(i32, u32)>,
    insert: Option<String>,
    export: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    hover: Option<Pos2>,
    last_pointer_world: (f32, f32),
    /// A text element to size to its text next frame.
    pending_fit: Option<String>,
    pending_dup: bool,
    props_open: bool,
    pub out: Vec<Out>,
}

impl QuadroEditor {
    pub fn open(path: &str, abs: PathBuf) -> Result<QuadroEditor, String> {
        let text = std::fs::read_to_string(&abs).map_err(|e| e.to_string())?;
        let board = Board::parse(path, &text)?;
        let disk_mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        Ok(QuadroEditor {
            path: path.to_string(),
            abs,
            board,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            last_change: None,
            disk_mtime,
            last_check: Instant::now(),
            error: None,
            pan: Vec2::ZERO,
            zoom: 1.0,
            fit: true,
            tool: Tool::Select,
            lock: false,
            style: Style::default(),
            sel: Vec::new(),
            drag: None,
            editing: None,
            focus_edit: false,
            geos: HashMap::new(),
            data_tex: HashMap::new(),
            scene: draw::Scene::default(),
            prev_targets: HashMap::new(),
            menu: None,
            menu_text: String::new(),
            menu_month: None,
            insert: None,
            export: None,
            hover: None,
            last_pointer_world: (0.0, 0.0),
            pending_fit: None,
            pending_dup: false,
            props_open: false,
            out: Vec::new(),
        })
    }

    /// The file was renamed or moved.
    pub fn moved(&mut self, path: &str, abs: PathBuf) {
        if self.path != path {
            self.path = path.to_string();
            self.abs = abs;
            self.disk_mtime = std::fs::metadata(&self.abs).and_then(|m| m.modified()).ok();
        }
    }

    pub fn title(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        name.trim_end_matches(".md").trim_end_matches(".excalidraw").to_string()
    }

    pub fn is_kanban(&self) -> bool {
        self.board.format == Format::Kanban
    }

    pub fn save(&mut self) {
        self.commit_edit();
        if !self.dirty {
            return;
        }
        let text = self.board.write();
        match ateliet_core::quadro::write_atomic(&self.abs, &text) {
            Ok(()) => {
                self.dirty = false;
                self.last_change = None;
                self.error = None;
                self.disk_mtime = std::fs::metadata(&self.abs).and_then(|m| m.modified()).ok();
            }
            Err(e) => self.error = Some(format!("Não foi possível salvar: {e}")),
        }
    }

    /// Before a change: the state to come back to.
    fn checkpoint(&mut self) {
        self.undo.push(self.board.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Drop the last checkpoint (the gesture changed nothing).
    fn uncheckpoint(&mut self) {
        self.undo.pop();
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.last_change = Some(Instant::now());
    }

    fn undo(&mut self) {
        self.commit_edit();
        if let Some(b) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.board, b));
            self.after_history();
        }
    }

    fn redo(&mut self) {
        self.commit_edit();
        if let Some(b) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.board, b));
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        let b = &self.board;
        self.sel.retain(|s| match s {
            Sel::El(id) => b.element(id).is_some_and(|e| !e.deleted()),
            Sel::Card(id) => b.card(id).is_some(),
            Sel::Lane(id) => b.lanes.iter().any(|l| &l.id == id),
        });
        self.prev_targets.clear();
        self.changed();
    }

    /// The file changed on disk (the Obsidian plugin saved it): read it
    /// again when nothing here is waiting to be saved.
    fn watch_disk(&mut self) {
        if self.last_check.elapsed() < Duration::from_millis(1500) {
            return;
        }
        self.last_check = Instant::now();
        let now = std::fs::metadata(&self.abs).and_then(|m| m.modified()).ok();
        if now.is_none() || now == self.disk_mtime || self.dirty || self.drag.is_some() || self.editing.is_some() {
            return;
        }
        self.disk_mtime = now;
        let Ok(text) = std::fs::read_to_string(&self.abs) else { return };
        match Board::parse(&self.path, &text) {
            Ok(b) => {
                self.checkpoint();
                self.board = b;
                self.after_history();
                self.dirty = false;
                self.out.push(Out::Toast(format!("{} mudou no disco e foi relido.", self.title())));
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The whole editor; what it asks of the app comes back in `out`.
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Out> {
        self.watch_disk();
        if let Some(rx) = &self.export {
            match rx.try_recv() {
                Ok(Ok(path)) => {
                    self.export = None;
                    self.out.push(Out::Rescan);
                    self.out.push(Out::Toast(format!("PNG exportado: {path}")));
                }
                Ok(Err(e)) => {
                    self.export = None;
                    self.out.push(Out::Toast(format!("Não foi possível exportar: {e}")));
                }
                Err(_) => ui.ctx().request_repaint_after(Duration::from_millis(100)),
            }
        }
        // autosave two seconds after the last change
        if self.dirty && self.drag.is_none() && self.editing.is_none() && self.last_change.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            self.save();
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(600));
        }
        if !ui.ctx().wants_keyboard_input() && self.editing.is_none() {
            self.keys(ui, cx);
        }
        let p = cx.palette;
        let full = ui.available_rect_before_wrap();
        let bar = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(full), |ui| {
            egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 }).show(ui, |ui| {
                ui.set_width(full.width() - 24.0);
                self.toolbar(ui, cx);
            });
        });
        let canvas = egui::Rect::from_min_max(egui::pos2(full.min.x, bar.response.rect.max.y), full.max);
        ui.painter().hline(canvas.x_range(), canvas.top() - 0.5, egui::Stroke::new(1.0_f32, p.border));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(canvas), |ui| {
            ui.set_clip_rect(canvas);
            self.canvas(ui, cx, canvas);
        });
        if let Some(e) = self.error.take() {
            self.out.push(Out::Toast(e));
        }
        std::mem::take(&mut self.out)
    }

    fn view(&self, canvas: egui::Rect) -> View {
        View { origin: canvas.min + self.pan, zoom: self.zoom }
    }

    /// Everything, or nothing: the box around all that is on the canvas.
    fn bounds(&self) -> Option<egui::Rect> {
        let mut out: Option<egui::Rect> = None;
        let mut add = |r: egui::Rect| out = Some(out.map_or(r, |o| o.union(r)));
        for l in &self.scene.lanes {
            add(draw::rr(l.rect));
        }
        for c in &self.scene.loose {
            add(draw::rr(c.rect));
        }
        for e in self.board.elements.iter().filter(|e| !e.deleted()) {
            let (x0, y0, x1, y1) = e.aabb();
            add(egui::Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)));
        }
        out
    }

    fn fit_view(&mut self, canvas: egui::Rect) {
        self.fit = false;
        let Some(b) = self.bounds() else {
            self.zoom = 1.0;
            self.pan = canvas.size() / 2.0 - egui::vec2(200.0, 120.0);
            return;
        };
        let b = b.expand(40.0);
        self.zoom = (canvas.width() / b.width()).min(canvas.height() / b.height()).clamp(0.1, 1.0);
        self.pan = canvas.size() / 2.0 - b.center().to_vec2() * self.zoom;
    }

    /// The canvas: ground, drawings, columns and cards, the selection.
    fn canvas(&mut self, ui: &mut egui::Ui, cx: &mut Cx, canvas: egui::Rect) {
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        let resp = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
        {
            let mut pics = VaultPics { vault: cx.vault, caches: cx.caches, source: &self.path, files: &self.board.files, data: &mut self.data_tex };
            self.scene = draw::layout(&ctx, &self.board, &mut pics);
        }
        if self.fit {
            self.fit_view(canvas);
        }
        self.follow_bindings();
        self.pointer(ui, &resp, canvas, cx);
        // a change made by the pointer shows now
        {
            let mut pics = VaultPics { vault: cx.vault, caches: cx.caches, source: &self.path, files: &self.board.files, data: &mut self.data_tex };
            self.scene = draw::layout(&ctx, &self.board, &mut pics);
        }
        let v = self.view(canvas);
        let painter = ui.painter_at(canvas);
        draw::ground(&painter, canvas, &v, p);
        let editing_id = match &self.editing {
            Some(Editing::Text { id, .. }) => Some(id.clone()),
            _ => None,
        };
        let card_ghost = match &self.drag {
            Some(Drag::Card { id, moved: true, .. }) => Some(id.clone()),
            _ => None,
        };
        let drop_at = match &self.drag {
            Some(Drag::Card { at, moved: true, .. }) => self.drop_target(*at),
            _ => None,
        };
        let mut pics = VaultPics { vault: cx.vault, caches: cx.caches, source: &self.path, files: &self.board.files, data: &mut self.data_tex };
        let view_rect = egui::Rect::from_min_max(pos2(v.w(canvas.min).0, v.w(canvas.min).1), pos2(v.w(canvas.max).0, v.w(canvas.max).1));
        let draw_els = |over: bool, geos: &mut HashMap<String, (u64, Geo)>, pics: &mut VaultPics| {
            for e in self.board.elements.iter().filter(|e| !e.deleted() && is_overlay(e) == over) {
                let (x0, y0, x1, y1) = e.aabb();
                if !view_rect.expand(40.0).intersects(egui::Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))) {
                    continue;
                }
                let g = geo_of(geos, e);
                draw::element(&painter, &ctx, e, g, &v, p, pics, editing_id.as_deref() == Some(e.id()));
            }
        };
        draw_els(false, &mut self.geos, &mut pics);
        let hover = self.hover;
        let hover_w = hover.map(|h| v.w(h));
        let sel_has = |s: &Sel| self.sel.contains(s);
        let editing_lane = match &self.editing {
            Some(Editing::Lane { id, .. }) => Some(id.clone()),
            _ => None,
        };
        let editing_card = match &self.editing {
            Some(Editing::Card { id, .. }) => Some(id.clone()),
            _ => None,
        };
        for (l, lb) in self.board.lanes.iter().zip(&self.scene.lanes) {
            draw::lane(&painter, l, lb, &v, p, sel_has(&Sel::Lane(l.id.clone())), hover, editing_lane.as_deref() == Some(l.id.as_str()));
            for (c, cb) in l.cards.iter().zip(&lb.cards) {
                if card_ghost.as_deref() == Some(c.id.as_str()) || editing_card.as_deref() == Some(c.id.as_str()) {
                    if editing_card.as_deref() == Some(c.id.as_str()) {
                        painter.rect(draw::wr(draw::rr(cb.rect), &v), 8.0, draw::lane_colors(p).1, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                    }
                    continue;
                }
                let hov = hover_w.is_some_and(|h| draw::rr(cb.rect).contains(pos2(h.0, h.1)));
                draw::card(&painter, &ctx, &self.board, c, cb, &v, p, &mut pics, sel_has(&Sel::Card(c.id.clone())), hov, false);
            }
        }
        draw_els(true, &mut self.geos, &mut pics);
        for (c, cb) in self.board.cards.iter().zip(&self.scene.loose) {
            if card_ghost.as_deref() == Some(c.id.as_str()) {
                continue;
            }
            if editing_card.as_deref() == Some(c.id.as_str()) {
                painter.rect(draw::wr(draw::rr(cb.rect), &v), 8.0, draw::lane_colors(p).1, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                continue;
            }
            let hov = hover_w.is_some_and(|h| draw::rr(cb.rect).contains(pos2(h.0, h.1)));
            draw::card(&painter, &ctx, &self.board, c, cb, &v, p, &mut pics, sel_has(&Sel::Card(c.id.clone())), hov, false);
        }
        // the card being carried, and where it would land
        if let Some(Drag::Card { id, grab, at, moved: true }) = &self.drag {
            if let (Some(c), Some(cb)) = (self.board.card(id), self.scene.card(id)) {
                if let Some((li, idx)) = drop_at {
                    let lb = &self.scene.lanes[li];
                    let y = if idx < lb.cards.len() { lb.cards[idx].rect.1 - ateliet_core::quadro::layout::CARD_GAP / 2.0 } else { lb.cards.last().map(|c| c.rect.1 + c.rect.3 + ateliet_core::quadro::layout::CARD_GAP / 2.0).unwrap_or(lb.rect.1 + ateliet_core::quadro::layout::HEADER_H) };
                    let x0 = lb.rect.0 + 8.0;
                    painter.line_segment([v.s((x0, y)), v.s((x0 + lb.rect.2 - 16.0, y))], egui::Stroke::new(3.0_f32, p.accent_strong));
                }
                let mut moved = cb.clone();
                let (dx, dy) = (at.0 - grab.0 - cb.rect.0, at.1 - grab.1 - cb.rect.1);
                shift_box(&mut moved, dx, dy);
                draw::card(&painter, &ctx, &self.board, c, &moved, &v, p, &mut pics, true, false, true);
            }
        }
        drop(pics);
        self.draw_selection(&painter, &v, p);
        if let Some(Drag::Marquee(a)) = &self.drag {
            if let Some(h) = hover {
                let r = egui::Rect::from_two_pos(v.s(*a), h);
                painter.rect(r, 0.0, p.accent_soft.gamma_multiply(0.4), egui::Stroke::new(1.0_f32, p.accent_strong), egui::StrokeKind::Inside);
            }
        }
        self.edit_overlay(ui, &v, p);
        self.menus(ui, cx, &v);
        self.props(ui, canvas, p);
        self.insert_popup(ui, cx, canvas);
        // zoom and the file's format in the corner
        let kind = match self.board.format {
            Format::Kanban => "Kanban",
            Format::ExcalidrawMd => "Excalidraw (.md)",
            Format::Excalidraw => "Excalidraw",
        };
        painter.text(canvas.right_bottom() + egui::vec2(-12.0, -10.0), egui::Align2::RIGHT_BOTTOM, format!("{kind} · {:.0}%", self.zoom * 100.0), egui::FontId::proportional(11.5), p.faint);
    }
}

/// Drawn over the columns: lines, arrows, strokes and loose text (they
/// point at and write on cards); shapes and pictures stay under them.
pub fn is_overlay(e: &Element) -> bool {
    matches!(e.kind(), "arrow" | "line" | "freedraw") || (e.kind() == "text" && e.container().is_none())
}

/// Move a laid-out card (for drawing it where it is carried).
fn shift_box(cb: &mut ateliet_core::quadro::layout::CardBox, dx: f32, dy: f32) {
    let s = |r: &mut (f32, f32, f32, f32)| {
        r.0 += dx;
        r.1 += dy;
    };
    s(&mut cb.rect);
    s(&mut cb.title_rect);
    if let Some((_, r)) = cb.picture.as_mut() {
        s(r);
    }
    for (_, r) in cb.chips.iter_mut() {
        s(r);
    }
    if let Some((_, r)) = cb.meta.as_mut() {
        s(r);
    }
}

/// A new quadro note (Kanban columns the plugin reads).
pub fn new_board_text() -> String {
    ateliet_core::quadro::new_board_text(&["A fazer", "Fazendo", "Feito"])
}

/// Is this vault file opened by the Quadro?
pub fn is_quadro(abs: &std::path::Path) -> bool {
    ateliet_core::quadro::is_quadro_file(abs)
}

/// Shared for previews in notes: the board of a file, read again when it changes.
pub fn cached_board(caches: &mut Caches, vault: &Vault, path: &str) -> Option<Arc<Board>> {
    let abs = vault.abs(path);
    let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok()?;
    if let Some((t, b)) = caches.quadros.get(path) {
        if *t == mtime {
            return Some(b.clone());
        }
    }
    let text = std::fs::read_to_string(&abs).ok()?;
    let b = Arc::new(Board::parse(path, &text).ok()?);
    caches.quadros.insert(path.to_string(), (mtime, b.clone()));
    Some(b)
}

/// A quadro embedded in a note (`![[quadro]]`): a picture of it; a click opens it.
pub fn embed(ui: &mut egui::Ui, vault: &Vault, caches: &mut Caches, path: &str, p: crate::theme::Palette) -> Option<egui::Response> {
    let b = cached_board(caches, vault, path)?;
    let mut geos = std::mem::take(&mut caches.quadro_geo);
    let mut data = std::mem::take(&mut caches.quadro_data);
    let r = {
        let mut pics = VaultPics { vault, caches, source: path, files: &b.files, data: &mut data };
        draw::preview(ui, &b, p, &mut pics, &mut geos, 420.0)
    };
    caches.quadro_geo = geos;
    caches.quadro_data = data;
    Some(r.on_hover_cursor(egui::CursorIcon::PointingHand))
}
