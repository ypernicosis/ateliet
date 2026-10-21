//! Image mode: every picture of the vault as a mosaic, the folders that hold
//! pictures as collections, search by name (and by likeness with the vectors
//! the image index already holds), and a viewer with zoom, the notes that use
//! the picture and similar pictures.

use crate::vault;
use ateliet_core::images::{self, ImageEntry, Source};
use eframe::egui::{self, Color32, RichText, Sense};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

const MODEL_KEY: &str = "clip-vit-base-patch32-q8";
const THUMB: u32 = 240;
const THUMB_LARGE: u32 = 480;

pub fn thumb_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("ateliet").join("thumbnails")
}

enum Thumb {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

/// Background decoding of previews (a few threads, newest requests first).
struct Loader {
    queue: Mutex<VecDeque<(String, PathBuf, i64, u64)>>,
    ready: Mutex<Vec<(String, Option<egui::ColorImage>)>>,
    wake: Condvar,
}

impl Loader {
    fn start(ctx: egui::Context) -> Arc<Loader> {
        let l = Arc::new(Loader { queue: Mutex::new(VecDeque::new()), ready: Mutex::new(Vec::new()), wake: Condvar::new() });
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
        for _ in 0..n {
            let (l2, c) = (l.clone(), ctx.clone());
            std::thread::spawn(move || loop {
                let job = {
                    let mut q = l2.queue.lock().unwrap();
                    while q.is_empty() {
                        q = l2.wake.wait(q).unwrap();
                    }
                    q.pop_back()
                };
                let Some((key, abs, mtime, size)) = job else { continue };
                let img = preview(&abs, mtime, size);
                l2.ready.lock().unwrap().push((key, img));
                c.request_repaint();
            });
        }
        l
    }
}

/// A cached preview (shared with the web shell's cache), made when missing.
fn preview(abs: &Path, mtime: i64, size: u64) -> Option<egui::ColorImage> {
    let dir = thumb_dir();
    let cached = images::thumbnail_path(&dir, abs, mtime, size, THUMB);
    let img = match image::open(&cached) {
        Ok(i) => i,
        Err(_) => {
            let larger = images::thumbnail_path(&dir, abs, mtime, size, THUMB_LARGE);
            let full = image::open(&larger).ok().or_else(|| images::open_oriented(abs).ok())?;
            let (bytes, _) = images::thumbnail(&full, THUMB);
            if let Some(d) = cached.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&cached, &bytes);
            image::load_from_memory(&bytes).ok()?
        }
    };
    let rgba = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

pub struct Viewer {
    pub idx: usize,
    tex: Option<egui::TextureHandle>,
    zoom: f32,
    offset: egui::Vec2,
    fit: bool,
}

pub struct ImagesState {
    pub entries: Vec<ImageEntry>,
    index: images::Index,
    pub folder: Option<String>,
    pub query: String,
    /// Indices shown, in order.
    shown: Vec<usize>,
    shown_key: String,
    similar_to: Option<usize>,
    thumbs: HashMap<String, Thumb>,
    loader: Option<Arc<Loader>>,
    pub viewer: Option<Viewer>,
    pub selection: HashSet<usize>,
    scanning: Option<std::sync::mpsc::Receiver<(Vec<ImageEntry>, images::Index)>>,
    root: PathBuf,
    pub cell: f32,
}

impl Default for ImagesState {
    fn default() -> Self {
        ImagesState {
            entries: Vec::new(),
            index: images::Index::new(MODEL_KEY, 512),
            folder: None,
            query: String::new(),
            shown: Vec::new(),
            shown_key: String::from("\u{0}"),
            similar_to: None,
            thumbs: HashMap::new(),
            loader: None,
            viewer: None,
            selection: HashSet::new(),
            scanning: None,
            root: PathBuf::new(),
            cell: 170.0,
        }
    }
}

/// What image mode asks the app to do.
pub enum ImageAction {
    Insert(Vec<String>),
    NewNote(Vec<String>),
    OpenNote(String),
}

impl ImagesState {
    pub fn open(&mut self, root: &Path, ctx: &egui::Context) {
        if self.root == root && (!self.entries.is_empty() || self.scanning.is_some()) {
            return;
        }
        self.root = root.to_path_buf();
        self.rescan();
        if self.loader.is_none() {
            self.loader = Some(Loader::start(ctx.clone()));
        }
    }

    pub fn rescan(&mut self) {
        let root = self.root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let index_path = root.join(".obsidian/ateliet/images/index.bin");
            let mut index = images::Index::load(&index_path, MODEL_KEY, 512);
            let found = images::scan(&[Source::vault(root.clone())]);
            let entries = index.update_from_scan(&found);
            let _ = tx.send((entries, index));
        });
        self.scanning = Some(rx);
    }

    /// Picture files changed on disk.
    pub fn changed(&mut self, rels: &[String]) {
        if rels.iter().any(|r| images::is_image(r)) && !self.root.as_os_str().is_empty() {
            self.rescan();
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.scanning {
            if let Ok((entries, index)) = rx.try_recv() {
                self.entries = entries;
                self.index = index;
                self.scanning = None;
                self.shown_key = String::from("\u{0}");
                self.selection.clear();
                self.viewer = None;
            }
        }
        if let Some(l) = &self.loader {
            let done: Vec<_> = std::mem::take(&mut *l.ready.lock().unwrap());
            for (key, img) in done {
                let t = match img {
                    Some(img) => Thumb::Ready(ctx.load_texture(format!("thumb:{key}"), img, egui::TextureOptions::LINEAR)),
                    None => Thumb::Failed,
                };
                self.thumbs.insert(key, t);
            }
        }
    }

    fn thumb(&mut self, i: usize) -> Option<egui::TextureId> {
        let e = &self.entries[i];
        match self.thumbs.get(&e.key) {
            Some(Thumb::Ready(t)) => Some(t.id()),
            Some(_) => None,
            None => {
                self.thumbs.insert(e.key.clone(), Thumb::Loading);
                if let Some(l) = &self.loader {
                    l.queue.lock().unwrap().push_back((e.key.clone(), PathBuf::from(&e.abs), e.mtime, e.size));
                    l.wake.notify_one();
                }
                None
            }
        }
    }

    fn refresh_shown(&mut self) {
        let key = format!("{:?}|{}|{:?}|{}", self.folder, self.query, self.similar_to, self.entries.len());
        if key == self.shown_key {
            return;
        }
        self.shown_key = key;
        let candidates: Vec<usize> = (0..self.entries.len()).filter(|&i| self.folder.as_ref().is_none_or(|f| &self.entries[i].folder == f)).collect();
        self.shown = if let Some(s) = self.similar_to {
            match self.index.vector(&self.entries[s].key).map(|v| v.to_vec()) {
                Some(v) => images::rank(&self.entries, &self.index, &candidates, &v, &[]).into_iter().map(|x| x.0).collect(),
                None => vec![s],
            }
        } else if self.query.trim().is_empty() {
            let mut c = candidates;
            c.sort_by_key(|&i| std::cmp::Reverse(self.entries[i].mtime));
            c
        } else {
            let words = images::query_words(&self.query);
            images::rank_by_name(&self.entries, &candidates, &words).into_iter().map(|x| x.0).collect()
        };
    }

    /// Folders with pictures: (folder, count, cover index).
    pub fn folders(&self) -> Vec<(String, usize, usize)> {
        let mut m: HashMap<&str, (usize, usize, i64)> = HashMap::new();
        for (i, e) in self.entries.iter().enumerate() {
            let f = m.entry(e.folder.as_str()).or_insert((0, i, i64::MIN));
            f.0 += 1;
            if e.mtime > f.2 {
                f.1 = i;
                f.2 = e.mtime;
            }
        }
        let mut v: Vec<(String, usize, usize)> = m.into_iter().map(|(k, (n, c, _))| (k.to_string(), n, c)).collect();
        v.sort_by_key(|x| x.0.to_lowercase());
        v
    }

    /// The collections column (left side in image mode).
    pub fn collections_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        self.poll(ui.ctx());
        ui.label(RichText::new("Collections").strong());
        ui.add_space(4.0);
        let all = self.entries.len();
        if ui.add(egui::Button::new(format!("All pictures  ({all})")).frame(false).selected(self.folder.is_none())).clicked() {
            self.folder = None;
            self.similar_to = None;
            self.viewer = None;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (folder, n, cover) in self.folders() {
                let name = if folder.is_empty() { "(vault root)".to_string() } else { folder.rsplit('/').next().unwrap_or(&folder).to_string() };
                let sel = self.folder.as_deref() == Some(folder.as_str());
                let resp = ui.horizontal(|ui| {
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::click());
                    match self.thumb(cover) {
                        Some(t) => {
                            egui::Image::new((t, egui::vec2(36.0, 36.0))).corner_radius(4).paint_at(ui, rect);
                        }
                        None => {
                            ui.painter().rect_filled(rect, 4.0, p.bg_hover);
                        }
                    }
                    let l = ui.add(egui::Button::new(RichText::new(format!("{name}  ")).color(p.text)).frame(false).selected(sel));
                    ui.label(RichText::new(n.to_string()).small().color(p.muted));
                    r.clicked() || l.clicked()
                });
                if resp.inner {
                    self.folder = Some(folder.clone());
                    self.similar_to = None;
                    self.viewer = None;
                }
                resp.response.on_hover_text(&folder);
            }
        });
    }

    /// The mosaic or the viewer (centre in image mode).
    pub fn center_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette, vault: &vault::Vault) -> Option<ImageAction> {
        self.poll(ui.ctx());
        if self.viewer.is_some() {
            return self.viewer_ui(ui, p, vault);
        }
        let mut action = None;
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Search pictures by name…").desired_width(320.0));
            if r.changed() {
                self.similar_to = None;
            }
            if self.similar_to.is_some() && ui.button("✖ Similar").on_hover_text("Back to all pictures").clicked() {
                self.similar_to = None;
            }
            ui.add(egui::Slider::new(&mut self.cell, 90.0..=320.0).show_value(false)).on_hover_text("Size");
            if !self.selection.is_empty() {
                ui.label(RichText::new(format!("{} selected", self.selection.len())).color(p.muted));
                if ui.button("Insert in note").clicked() {
                    let keys = self.selected_keys();
                    action = Some(ImageAction::Insert(keys));
                }
                if ui.button("New note").clicked() {
                    action = Some(ImageAction::NewNote(self.selected_keys()));
                }
                if ui.button("Clear").clicked() {
                    self.selection.clear();
                }
            }
            if self.scanning.is_some() {
                ui.spinner();
            }
        });
        ui.add_space(6.0);
        self.refresh_shown();
        if self.shown.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new(if self.scanning.is_some() { "Looking for pictures…" } else { "No pictures." }).size(18.0).color(p.muted));
            });
            return action;
        }
        let cell = self.cell;
        let gap = 6.0;
        let cols = ((ui.available_width() + gap) / (cell + gap)).floor().max(1.0) as usize;
        let rows = self.shown.len().div_ceil(cols);
        let shown = self.shown.clone();
        let mut open: Option<usize> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, cell + gap, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in 0..cols {
                        let Some(&i) = shown.get(r * cols + c) else { break };
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, cell), Sense::click());
                        ui.painter().rect_filled(rect, 6.0, p.bg_secondary);
                        if let Some(t) = self.thumb(i) {
                            let e = &self.entries[i];
                            let (w, h) = (e.width.max(1) as f32, e.height.max(1) as f32);
                            let s = (cell / w).min(cell / h);
                            let size = egui::vec2(w * s, h * s);
                            let r2 = egui::Rect::from_center_size(rect.center(), size);
                            egui::Image::new((t, size)).corner_radius(4).paint_at(ui, r2);
                        }
                        let sel = self.selection.contains(&i);
                        if sel {
                            ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(3.0_f32, p.accent), egui::StrokeKind::Inside);
                        } else if resp.hovered() {
                            ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                        }
                        let name = self.entries[i].name.clone();
                        let resp = resp.on_hover_text(name);
                        if resp.clicked() {
                            if ui.input(|inp| inp.modifiers.command || inp.modifiers.shift) {
                                if !self.selection.remove(&i) {
                                    self.selection.insert(i);
                                }
                            } else {
                                open = Some(i);
                            }
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Find similar pictures").clicked() {
                                self.similar_to = Some(i);
                                self.query.clear();
                                ui.close_menu();
                            }
                            if ui.button("Insert in note").clicked() {
                                action = Some(ImageAction::Insert(vec![self.entries[i].key.clone()]));
                                ui.close_menu();
                            }
                            if ui.button(if sel { "Unselect" } else { "Select" }).clicked() {
                                if !self.selection.remove(&i) {
                                    self.selection.insert(i);
                                }
                                ui.close_menu();
                            }
                            if ui.button("Open in default app").clicked() {
                                let _ = open::that(&self.entries[i].abs);
                                ui.close_menu();
                            }
                        });
                    }
                });
                ui.add_space(gap - ui.spacing().item_spacing.y);
            }
        });
        if let Some(i) = open {
            self.viewer = Some(Viewer { idx: i, tex: None, zoom: 1.0, offset: egui::Vec2::ZERO, fit: true });
        }
        action
    }

    fn selected_keys(&self) -> Vec<String> {
        let mut v: Vec<usize> = self.selection.iter().copied().collect();
        v.sort();
        v.into_iter().map(|i| self.entries[i].key.clone()).collect()
    }

    fn viewer_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette, vault: &vault::Vault) -> Option<ImageAction> {
        let mut action = None;
        let Some(v) = &mut self.viewer else { return None };
        let e = self.entries[v.idx].clone();
        let mut close = false;
        let mut step: i64 = 0;
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            ui.label(RichText::new(&e.name).strong());
            ui.label(RichText::new(format!("{}×{}", e.width, e.height)).small().color(p.muted));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Fit").clicked() || ui.input(|i| i.key_pressed(egui::Key::Num0)) {
                v.fit = true;
                v.offset = egui::Vec2::ZERO;
            }
            if ui.button("1:1").clicked() || ui.input(|i| i.key_pressed(egui::Key::Num1)) {
                v.fit = false;
                v.zoom = 1.0;
                v.offset = egui::Vec2::ZERO;
            }
            if ui.button("Insert in note").clicked() {
                action = Some(ImageAction::Insert(vec![e.key.clone()]));
            }
            if ui.button("New note with it").clicked() {
                action = Some(ImageAction::NewNote(vec![e.key.clone()]));
            }
            if ui.button("Open in default app").clicked() {
                let _ = open::that(&e.abs);
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            step = 1;
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            step = -1;
        }
        if v.tex.is_none() {
            if let Ok(img) = images::open_oriented(Path::new(&e.abs)) {
                let img = if img.width() > 4096 || img.height() > 4096 { img.thumbnail(4096, 4096) } else { img };
                let rgba = img.to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                v.tex = Some(ui.ctx().load_texture(format!("view:{}", e.key), ci, egui::TextureOptions::LINEAR));
            }
        }
        // side column: notes using the picture, similar pictures
        let side_w = 240.0;
        let avail = ui.available_rect_before_wrap();
        let stage = egui::Rect::from_min_max(avail.min, egui::pos2(avail.max.x - side_w - 10.0, avail.max.y));
        let side = egui::Rect::from_min_max(egui::pos2(stage.max.x + 10.0, avail.min.y), avail.max);
        let resp = ui.allocate_rect(stage, Sense::click_and_drag());
        ui.painter().rect_filled(stage, 6.0, if p.dark { Color32::from_gray(18) } else { Color32::from_gray(235) });
        if let Some(t) = &v.tex {
            let size = t.size_vec2();
            let fit = (stage.width() / size.x).min(stage.height() / size.y).min(1.0);
            if v.fit {
                v.zoom = fit;
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    let old = v.zoom;
                    v.zoom = (v.zoom * (1.0 + scroll * 0.002)).clamp(0.05, 16.0);
                    v.fit = false;
                    if let Some(ptr) = resp.hover_pos() {
                        let rel = ptr - stage.center() - v.offset;
                        v.offset -= rel * (v.zoom / old - 1.0);
                    }
                }
            }
            if resp.dragged() {
                v.offset += resp.drag_delta();
                v.fit = false;
            }
            if resp.double_clicked() {
                v.fit = !v.fit;
                if !v.fit {
                    v.zoom = 1.0;
                }
                v.offset = egui::Vec2::ZERO;
            }
            let shown = size * v.zoom;
            let rect = egui::Rect::from_center_size(stage.center() + v.offset, shown);
            ui.painter().with_clip_rect(stage).image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        let mut similar: Vec<usize> = Vec::new();
        if let Some(vec) = self.index.vector(&e.key).map(|x| x.to_vec()) {
            let all: Vec<usize> = (0..self.entries.len()).collect();
            similar = images::rank(&self.entries, &self.index, &all, &vec, &[]).into_iter().map(|x| x.0).filter(|&i| i != v.idx).take(12).collect();
        }
        let users = vault.index.read().map(|ix| ix.inlinks(&e.key)).unwrap_or_default();
        let mut go_to: Option<usize> = None;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(side), |ui| {
            ui.label(RichText::new(format!("Notes using it ({})", users.len())).strong());
            for n in &users {
                if ui.add(egui::Button::new(vault::basename(n)).frame(false)).clicked() {
                    action = Some(ImageAction::OpenNote(n.clone()));
                }
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Similar").strong());
            if similar.is_empty() {
                ui.label(RichText::new("Pictures are compared once the image index has analysed them (in the web shell, for now).").small().color(p.muted));
            }
            ui.horizontal_wrapped(|ui| {
                for &i in &similar {
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), Sense::click());
                    ui.painter().rect_filled(rect, 4.0, p.bg_secondary);
                    if let Some(t) = self.thumb(i) {
                        egui::Image::new((t, egui::vec2(72.0, 72.0))).maintain_aspect_ratio(true).corner_radius(4).paint_at(ui, rect);
                    }
                    if r.clicked() {
                        go_to = Some(i);
                    }
                }
            });
        });
        let v = self.viewer.as_mut().unwrap();
        if let Some(i) = go_to {
            *v = Viewer { idx: i, tex: None, zoom: 1.0, offset: egui::Vec2::ZERO, fit: true };
        } else if step != 0 {
            if let Some(pos) = self.shown.iter().position(|&i| i == v.idx) {
                let n = self.shown.len() as i64;
                let next = self.shown[((pos as i64 + step).rem_euclid(n)) as usize];
                *v = Viewer { idx: next, tex: None, zoom: 1.0, offset: egui::Vec2::ZERO, fit: true };
            }
        }
        if close {
            self.viewer = None;
        }
        action
    }
}
