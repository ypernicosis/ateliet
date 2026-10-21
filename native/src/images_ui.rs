//! Image mode: every picture of the vault as a mosaic, the folders that hold
//! pictures as collections, search by name (and by likeness with the vectors
//! the image index already holds), and a viewer with zoom, the notes that use
//! the picture and similar pictures.

use crate::vault;
use ateliet_core::images::{self, ImageEntry, Source};
use crate::icons;
use crate::kit;
use eframe::egui::{self, Color32, RichText, Sense};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
#[cfg(feature = "clip")]
use crate::vision::Vision;

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
    index: Arc<Mutex<images::Index>>,
    #[cfg(feature = "clip")]
    vision: Arc<Vision>,
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
            index: Arc::new(Mutex::new(images::Index::new(MODEL_KEY, 512))),
            #[cfg(feature = "clip")]
            vision: Arc::new(Vision::default()),
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
                *self.index.lock().unwrap() = index;
                #[cfg(feature = "clip")]
                self.vision.index(self.index.clone(), self.entries.clone(), self.root.join(".obsidian/ateliet/images/index.bin"), ctx.clone());
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
        let index = self.index.clone();
        let ix = index.lock().unwrap();
        self.shown = if let Some(s) = self.similar_to {
            match ix.vector(&self.entries[s].key).map(|v| v.to_vec()) {
                Some(v) => images::rank(&self.entries, &ix, &candidates, &v, &[]).into_iter().map(|x| x.0).collect(),
                None => vec![s],
            }
        } else if self.query.trim().is_empty() {
            let mut c = candidates;
            c.sort_by_key(|&i| std::cmp::Reverse(self.entries[i].mtime));
            c
        } else {
            let words = images::query_words(&self.query);
            #[cfg(feature = "clip")]
            if let Some(v) = self.vision.ready().then(|| self.vision.text_vector(&self.query)).flatten() {
                // by description, then pictures not analysed yet whose name matches
                let mut out: Vec<usize> = images::rank(&self.entries, &ix, &candidates, &v, &words).into_iter().map(|x| x.0).collect();
                let unanalysed: Vec<usize> = candidates.iter().copied().filter(|&i| !ix.has_vector(&self.entries[i].key)).collect();
                out.extend(images::rank_by_name(&self.entries, &unanalysed, &words).into_iter().map(|x| x.0));
                self.shown = out;
                return;
            }
            images::rank_by_name(&self.entries, &candidates, &words).into_iter().map(|x| x.0).collect()
        };
    }

    /// Model download / analysis progress (description search).
    #[cfg(feature = "clip")]
    fn vision_bar(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        use std::sync::atomic::Ordering;
        let v = &self.vision;
        if v.downloading.load(Ordering::Relaxed) {
            let total = v.total.load(Ordering::Relaxed).max(1) as f32;
            let got = v.received.load(Ordering::Relaxed) as f32;
            ui.add(egui::ProgressBar::new(got / total).desired_width(160.0).text(format!("modelo {:.0} de {:.0} MB", got / 1e6, total / 1e6)));
        } else if !v.ready() {
            if kit::button(ui, Some(icons::SPARKLES), "Buscar por descrição", p).on_hover_text("Baixa o modelo CLIP uma vez (cerca de 150 MB). As imagens são analisadas neste computador.").clicked() {
                v.download(ui.ctx().clone());
            }
        } else {
            let waiting = v.waiting.load(Ordering::Relaxed);
            if waiting > 0 {
                ui.label(RichText::new(format!("analisando {} de {waiting}", v.done.load(Ordering::Relaxed))).small().color(p.muted));
            } else if self.scanning.is_none() && !self.entries.is_empty() {
                // a model that arrived after the scan: start analysing
                let missing = { let ix = self.index.lock().unwrap(); self.entries.iter().any(|e| !ix.has_vector(&e.key)) };
                if missing {
                    v.index(self.index.clone(), self.entries.clone(), self.root.join(".obsidian/ateliet/images/index.bin"), ui.ctx().clone());
                }
            }
        }
        if let Some(e) = v.error.lock().unwrap().clone() {
            ui.label(RichText::new(e).small().color(p.error));
        }
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
        ui.label(RichText::new("Imagens").font(kit::medium(13.5)).color(p.text));
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let all = self.entries.len();
        if kit::Row::new("Todas as imagens").icon(icons::IMAGES).trailing(all.to_string()).selected(self.folder.is_none()).show(ui, p).clicked() {
            self.folder = None;
            self.similar_to = None;
            self.viewer = None;
        }
        ui.add_space(10.0);
        kit::section(ui, "Coleções", p);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (folder, n, cover) in self.folders() {
                let name = if folder.is_empty() { "Raiz do cofre".to_string() } else { folder.rsplit('/').next().unwrap_or(&folder).to_string() };
                let sel = self.folder.as_deref() == Some(folder.as_str());
                let w = ui.available_width();
                let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 44.0), Sense::click());
                ui.painter().rect_filled(rect, 8.0, if sel { p.bg_active } else if r.hovered() { p.bg_hover } else { Color32::TRANSPARENT });
                let thumb = egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::vec2(32.0, 32.0));
                match self.thumb(cover) {
                    Some(t) => {
                        let e = &self.entries[cover];
                        let (iw, ih) = (e.width.max(1) as f32, e.height.max(1) as f32);
                        let k = (iw / ih).max(1e-3);
                        let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
                        ui.painter().add(egui::epaint::RectShape::filled(thumb, 6.0, Color32::WHITE).with_texture(t, uv));
                    }
                    None => {
                        ui.painter().rect_filled(thumb, 6.0, p.bg_active);
                    }
                }
                ui.painter().text(egui::pos2(thumb.right() + 10.0, rect.center().y), egui::Align2::LEFT_CENTER, &name, if sel { kit::medium(13.0) } else { egui::FontId::proportional(13.0) }, p.text);
                ui.painter().text(egui::pos2(rect.right() - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, n.to_string(), egui::FontId::proportional(11.5), p.faint);
                if r.on_hover_text(&folder).clicked() {
                    self.folder = Some(folder.clone());
                    self.similar_to = None;
                    self.viewer = None;
                }
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
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            #[cfg(feature = "clip")]
            let hint = if self.vision.ready() { "Descreva uma imagem" } else { "Buscar imagens pelo nome" };
            #[cfg(not(feature = "clip"))]
            let hint = "Buscar imagens pelo nome";
            ui.allocate_ui(egui::vec2(320.0, 30.0), |ui| {
                let r = kit::search_field(ui, &mut self.query, hint, p);
                if r.changed() {
                    self.similar_to = None;
                }
            });
            if self.similar_to.is_some() && kit::button(ui, Some(icons::X), "Semelhantes", p).on_hover_text("Voltar a todas as imagens").clicked() {
                self.similar_to = None;
            }
            ui.add_space(4.0);
            ui.label(kit::icon(icons::LAYOUT_GRID, 13.0).color(p.faint));
            ui.spacing_mut().slider_width = 90.0;
            ui.add(egui::Slider::new(&mut self.cell, 90.0..=320.0).show_value(false)).on_hover_text("Tamanho das miniaturas");
            if !self.selection.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("{} {}", self.selection.len(), if self.selection.len() == 1 { "selecionada" } else { "selecionadas" })).color(p.muted));
                if kit::primary(ui, "Inserir na nota", p).clicked() {
                    let keys = self.selected_keys();
                    action = Some(ImageAction::Insert(keys));
                }
                if kit::button(ui, Some(icons::FILE_PLUS), "Nova nota", p).clicked() {
                    action = Some(ImageAction::NewNote(self.selected_keys()));
                }
                if kit::quiet(ui, None, "Limpar", p).clicked() {
                    self.selection.clear();
                }
            }
            if self.scanning.is_some() {
                ui.spinner();
            }
            #[cfg(feature = "clip")]
            self.vision_bar(ui, p);
        });
        ui.add_space(14.0);
        self.refresh_shown();
        if self.shown.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(90.0);
                ui.label(kit::icon(icons::IMAGES, 30.0).color(p.faint));
                ui.add_space(12.0);
                ui.label(RichText::new(if self.scanning.is_some() { "Procurando imagens…" } else { "Nenhuma imagem" }).font(kit::medium(15.0)).color(p.muted));
            });
            return action;
        }
        let gap = 10.0;
        // as many columns as the chosen size allows, stretched to fill the width
        let avail_w = ui.available_width() - 4.0;
        let cols = ((avail_w + gap) / (self.cell + gap)).floor().max(1.0) as usize;
        let cell = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
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
                        ui.painter().rect_filled(rect, 10.0, p.bg_secondary);
                        if let Some(t) = self.thumb(i) {
                            // fill the square, cropping the long side
                            let e = &self.entries[i];
                            let k = (e.width.max(1) as f32 / e.height.max(1) as f32).max(1e-3);
                            let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
                            ui.painter().add(egui::epaint::RectShape::filled(rect, 10.0, Color32::WHITE).with_texture(t, uv));
                        }
                        let sel = self.selection.contains(&i);
                        if resp.hovered() || sel {
                            // a soft shade at the foot with the name
                            let foot = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 30.0), rect.max);
                            ui.painter().rect_filled(foot, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, Color32::from_black_alpha(110));
                            let mut job = egui::text::LayoutJob::single_section(self.entries[i].name.clone(), egui::TextFormat { font_id: egui::FontId::proportional(11.5), color: Color32::WHITE, ..Default::default() });
                            job.wrap = egui::text::TextWrapping::truncate_at_width(cell - 20.0);
                            let g = ui.fonts(|f| f.layout_job(job));
                            ui.painter().galley(egui::pos2(foot.left() + 10.0, foot.center().y - g.size().y / 2.0), g, Color32::WHITE);
                        }
                        if sel {
                            ui.painter().rect_stroke(rect, 10.0, egui::Stroke::new(3.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                            let c = egui::pos2(rect.right() - 16.0, rect.top() + 16.0);
                            ui.painter().circle_filled(c, 10.0, p.accent_strong);
                            ui.painter().text(c, egui::Align2::CENTER_CENTER, icons::CHECK, kit::icon_font(12.0), p.on_accent);
                        }
                        let resp = resp;
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
                            if ui.button("Imagens semelhantes").clicked() {
                                self.similar_to = Some(i);
                                self.query.clear();
                                ui.close_menu();
                            }
                            if ui.button("Inserir na nota").clicked() {
                                action = Some(ImageAction::Insert(vec![self.entries[i].key.clone()]));
                                ui.close_menu();
                            }
                            if ui.button(if sel { "Desmarcar" } else { "Selecionar" }).clicked() {
                                if !self.selection.remove(&i) {
                                    self.selection.insert(i);
                                }
                                ui.close_menu();
                            }
                            if ui.button("Abrir no app padrão").clicked() {
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
            if kit::icon_button(ui, icons::ARROW_LEFT, "Voltar (Esc)", false, p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            ui.add_space(4.0);
            ui.label(RichText::new(&e.name).font(kit::medium(14.0)).color(p.text));
            ui.label(RichText::new(format!("{} × {}", e.width, e.height)).size(12.0).color(p.faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if kit::icon_button(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", false, p).clicked() {
                    let _ = open::that(&e.abs);
                }
                if kit::icon_button(ui, icons::FILE_PLUS, "Nova nota com esta imagem", false, p).clicked() {
                    action = Some(ImageAction::NewNote(vec![e.key.clone()]));
                }
                ui.add_space(6.0);
                if kit::primary(ui, "Inserir na nota", p).clicked() {
                    action = Some(ImageAction::Insert(vec![e.key.clone()]));
                }
                ui.add_space(6.0);
                if kit::icon_button(ui, icons::SCAN_SEARCH, "Tamanho real (1)", false, p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Num1)) {
                    v.fit = false;
                    v.zoom = 1.0;
                    v.offset = egui::Vec2::ZERO;
                }
                if kit::icon_button(ui, icons::MAXIMIZE, "Ajustar (0)", false, p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Num0)) {
                    v.fit = true;
                    v.offset = egui::Vec2::ZERO;
                }
            });
        });
        ui.add_space(8.0);
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
        ui.painter().rect_filled(stage, 10.0, if p.dark { Color32::from_gray(18) } else { p.bg_secondary });
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
        {
            let ix = self.index.lock().unwrap();
            if let Some(vec) = ix.vector(&e.key).map(|x| x.to_vec()) {
                let all: Vec<usize> = (0..self.entries.len()).collect();
                similar = images::rank(&self.entries, &ix, &all, &vec, &[]).into_iter().map(|x| x.0).filter(|&i| i != v.idx).take(12).collect();
            }
        }
        let users = vault.index.read().map(|ix| ix.inlinks(&e.key)).unwrap_or_default();
        let mut go_to: Option<usize> = None;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(side), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            kit::section(ui, &format!("Notas que usam · {}", users.len()), p);
            if users.is_empty() {
                ui.label(RichText::new("Nenhuma ainda.").size(12.0).color(p.faint));
            }
            for n in &users {
                if kit::Row::new(vault::basename(n)).icon(icons::FILE_TEXT).show(ui, p).clicked() {
                    action = Some(ImageAction::OpenNote(n.clone()));
                }
            }
            ui.add_space(14.0);
            kit::section(ui, "Semelhantes", p);
            if similar.is_empty() {
                ui.label(RichText::new("Aparecem depois que as imagens forem analisadas para a busca por descrição.").size(12.0).color(p.faint));
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for &i in &similar {
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), Sense::click());
                    ui.painter().rect_filled(rect, 8.0, p.bg_secondary);
                    if let Some(t) = self.thumb(i) {
                        let e = &self.entries[i];
                        let k = (e.width.max(1) as f32 / e.height.max(1) as f32).max(1e-3);
                        let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
                        ui.painter().add(egui::epaint::RectShape::filled(rect, 8.0, Color32::WHITE).with_texture(t, uv));
                    }
                    if r.hovered() {
                        ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
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
