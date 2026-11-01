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
/// Preview sizes on disk (longest side). A picture is shown from the
/// smallest one at least as large as it is on screen, in physical pixels.
const TIERS: [u32; 3] = [THUMB, 480, 960];
/// The picture itself (up to this side), for a reference filling the screen.
const FULL: u32 = 0;
const FULL_MAX: u32 = 2560;

mod pinterest;
pub mod moodboard;
#[cfg(test)]
mod side_tests;

pub fn thumb_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("ateliet").join("thumbnails")
}

enum Thumb {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

/// One preview: a picture at a size, in colour or in greys.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct ThumbKey {
    key: String,
    tier: u32,
    gray: bool,
}

struct Job {
    id: ThumbKey,
    abs: PathBuf,
    mtime: i64,
    size: u64,
}

/// Background decoding of previews (a few threads, newest requests first).
struct Loader {
    queue: Mutex<VecDeque<Job>>,
    ready: Mutex<Vec<(ThumbKey, Option<egui::ColorImage>)>>,
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
                let Some(job) = job else { continue };
                let img = preview(&job.abs, job.mtime, job.size, job.id.tier).map(|i| if job.id.gray { grayscale(i) } else { i });
                l2.ready.lock().unwrap().push((job.id, img));
                c.request_repaint();
            });
        }
        l
    }
}

/// A cached preview of `tier` pixels (made when missing, from a larger
/// cached one or the picture), or the picture itself for `FULL`.
fn preview(abs: &Path, mtime: i64, size: u64, tier: u32) -> Option<egui::ColorImage> {
    let img = if tier == FULL {
        let img = images::open_oriented(abs).ok()?;
        if img.width() > FULL_MAX || img.height() > FULL_MAX { img.resize(FULL_MAX, FULL_MAX, image::imageops::FilterType::Triangle) } else { img }
    } else {
        let dir = thumb_dir();
        let cached = images::thumbnail_path(&dir, abs, mtime, size, tier);
        match image::open(&cached) {
            Ok(i) => i,
            Err(_) => {
                let larger = TIERS.iter().filter(|&&t| t > tier).find_map(|&t| image::open(images::thumbnail_path(&dir, abs, mtime, size, t)).ok());
                let full = larger.or_else(|| images::open_oriented(abs).ok())?;
                let (bytes, _) = images::thumbnail(&full, tier);
                if let Some(d) = cached.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(&cached, &bytes);
                image::load_from_memory(&bytes).ok()?
            }
        }
    };
    let rgba = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

fn grayscale(mut img: egui::ColorImage) -> egui::ColorImage {
    for px in &mut img.pixels {
        let [r, g, b, a] = px.to_srgba_unmultiplied();
        let l = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8;
        *px = Color32::from_rgba_unmultiplied(l, l, l, a);
    }
    img
}

/// Linear filtering with mipmaps: a large preview drawn small stays smooth.
const TEX: egui::TextureOptions = egui::TextureOptions {
    magnification: egui::TextureFilter::Linear,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: Some(egui::TextureFilter::Linear),
};

/// The size to load for a picture drawn `px` physical pixels long.
fn tier_for(px: f32, full: bool) -> u32 {
    TIERS.iter().copied().find(|&t| t as f32 >= px * 0.9).unwrap_or(if full { FULL } else { TIERS[TIERS.len() - 1] })
}

pub struct Viewer {
    pub idx: usize,
    tex: Option<egui::TextureHandle>,
    zoom: f32,
    offset: egui::Vec2,
    fit: bool,
    /// Over the whole window (F).
    full: bool,
}

pub struct ImagesState {
    pub entries: Vec<ImageEntry>,
    /// Bumped whenever `entries` is replaced.
    entries_gen: u64,
    /// The moodboards.
    pub mb: moodboard::Moodboards,
    /// Whether the collections (left) and notes (right) panels are shown in
    /// image mode (the writing mode's sidebars are separate).
    pub panels: [bool; 2],
    index: Arc<Mutex<images::Index>>,
    #[cfg(feature = "clip")]
    vision: Arc<Vision>,
    pub folder: Option<String>,
    pub query: String,
    /// Indices shown, in order.
    shown: Vec<usize>,
    shown_key: String,
    similar_to: Option<usize>,
    thumbs: HashMap<ThumbKey, Thumb>,
    /// When each large preview was last drawn (pass number): unused ones are let go.
    thumb_used: HashMap<ThumbKey, u64>,
    pass: u64,
    loader: Option<Arc<Loader>>,
    pub viewer: Option<Viewer>,
    /// The picture open in the side panel (its key), over the grid.
    pub side_view: Option<String>,
    pub selection: HashSet<usize>,
    scanning: Option<std::sync::mpsc::Receiver<(Vec<ImageEntry>, images::Index)>>,
    root: PathBuf,
    pub cell: f32,
    /// The shared picture library (Pictogrep's folder) in use, if any.
    pub shared: Option<ateliet_core::pictogrep::Home>,
    /// A shared collection shown ("" is the whole shared library).
    pub collection: Option<String>,
    /// Shared collections: (name, members' absolute paths).
    collections: Vec<(String, HashSet<String>)>,
    pin: pinterest::PinState,
    /// A shared collection being named: (its old name, or None for a new one; the text).
    col_edit: Option<(Option<String>, String)>,
    /// A message for the app to show.
    pub notice: Option<String>,
    /// "Similar pictures" asked before the scan finished.
    pending_similar: Option<String>,
    /// Whether the vault's own pictures show in image mode (off: only the
    /// shared library, the external folders and the vault's Pinterest folder).
    pub show_vault: bool,
    /// The order of the pictures: saved last first, saved first first, or shuffled.
    pub order: Order,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    Newest,
    Oldest,
    Shuffled(u64),
}

impl Order {
    fn from_setting(v: &serde_json::Value) -> Order {
        match v.as_str() {
            Some("oldest") => Order::Oldest,
            Some("shuffle") => Order::Shuffled(new_seed()),
            _ => Order::Newest,
        }
    }
    fn setting(self) -> &'static str {
        match self {
            Order::Newest => "newest",
            Order::Oldest => "oldest",
            Order::Shuffled(_) => "shuffle",
        }
    }
}

fn new_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1
}

impl Default for ImagesState {
    fn default() -> Self {
        ImagesState {
            panels: [true, true],
            entries: Vec::new(),
            entries_gen: 0,
            mb: moodboard::Moodboards::default(),
            index: Arc::new(Mutex::new(images::Index::new(MODEL_KEY, 512))),
            #[cfg(feature = "clip")]
            vision: Arc::new(Vision::default()),
            folder: None,
            query: String::new(),
            shown: Vec::new(),
            shown_key: String::from("\u{0}"),
            similar_to: None,
            thumbs: HashMap::new(),
            thumb_used: HashMap::new(),
            pass: 0,
            loader: None,
            viewer: None,
            side_view: None,
            selection: HashSet::new(),
            scanning: None,
            root: PathBuf::new(),
            cell: 170.0,
            shared: None,
            collection: None,
            collections: Vec::new(),
            pin: pinterest::PinState::default(),
            col_edit: None,
            notice: None,
            pending_similar: None,
            show_vault: false,
            order: Order::Newest,
        }
    }
}

/// What image mode asks the app to do.
pub enum ImageAction {
    Insert(Vec<String>),
    /// Move vault pictures into a vault folder (their links follow).
    MoveTo(Vec<String>, String),
    NewNote(Vec<String>),
    OpenNote(String),
    /// Open (creating it if missing) the note of a vault folder: `<folder>/<folder>.md`.
    CollectionNote(String),
    /// Show or hide a side panel (true: the left one).
    TogglePanel(bool),
    /// Put pictures on the Journal's open page.
    SendToJournal(Vec<String>),
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

    /// The image settings kept by Ateliet (`ateliet/images.json`).
    pub fn settings(root: &Path) -> serde_json::Value {
        std::fs::read_to_string(root.join(".obsidian/ateliet/images.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| serde_json::json!({}))
    }

    pub fn save_settings(root: &Path, patch: serde_json::Value) {
        let mut s = Self::settings(root);
        if !s.is_object() {
            s = serde_json::json!({});
        }
        for (k, v) in patch.as_object().into_iter().flatten() {
            s[k] = v.clone();
        }
        let p = root.join(".obsidian/ateliet/images.json");
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(&s).unwrap_or_default());
    }

    /// The folders scanned: the vault, the shared library's, the external ones.
    fn sources(root: &Path, shared: Option<&ateliet_core::pictogrep::Home>, external: &[String]) -> Vec<Source> {
        use ateliet_core::images::Origin;
        let mut s = vec![Source::vault(root.to_path_buf())];
        let mut add = |p: PathBuf, origin: Origin| {
            if p.is_dir() && !p.starts_with(root) && !s.iter().any(|x| x.root == p) {
                s.push(Source::outside(p, origin));
            }
        };
        if let Some(h) = shared {
            let _ = std::fs::create_dir_all(h.library());
            for r in ateliet_core::pictogrep::source_roots(h) {
                add(r, Origin::Shared);
            }
        }
        for e in external {
            add(PathBuf::from(e), Origin::External);
        }
        s
    }

    pub fn rescan(&mut self) {
        if self.root.as_os_str().is_empty() {
            return;
        }
        let root = self.root.clone();
        let st = Self::settings(&root);
        let shared = match st["shared"].as_str().unwrap_or("auto").trim() {
            "" => None,
            "auto" => ateliet_core::pictogrep::detect(),
            path => Some(ateliet_core::pictogrep::Home::new(PathBuf::from(path))).filter(|h| h.root.is_dir()),
        };
        self.show_vault = st["show_vault"].as_bool().unwrap_or(false);
        if self.order.setting() != st["order"].as_str().unwrap_or("newest") {
            self.order = Order::from_setting(&st["order"]);
        }
        self.shown_key = String::from("\u{0}");
        let external: Vec<String> = st["external"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        self.collections = shared.as_ref().map(|h| ateliet_core::pictogrep::collection_names(h).into_iter().map(|n| {
            let members = ateliet_core::pictogrep::collection_images(h, &n).into_iter().map(|p| p.to_string_lossy().to_string()).collect();
            (n, members)
        }).collect()).unwrap_or_default();
        self.shared = shared.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let index_path = root.join(".obsidian/ateliet/images/index.bin");
            let mut index = images::Index::load(&index_path, MODEL_KEY, 512);
            let found = images::scan(&Self::sources(&root, shared.as_ref(), &external));
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
                self.entries_gen += 1;
                *self.index.lock().unwrap() = index;
                #[cfg(feature = "clip")]
                self.vision.index(self.index.clone(), self.entries.clone(), self.root.join(".obsidian/ateliet/images/index.bin"), ctx.clone());
                self.scanning = None;
                self.shown_key = String::from("\u{0}");
                self.selection.clear();
                self.viewer = None;
                if let Some(k) = self.pending_similar.take() {
                    self.show_similar(&k);
                }
            }
        }
        if let Some(l) = &self.loader {
            let done: Vec<_> = std::mem::take(&mut *l.ready.lock().unwrap());
            for (id, img) in done {
                let t = match img {
                    Some(img) => Thumb::Ready(ctx.load_texture(format!("thumb:{}:{}:{}", id.key, id.tier, id.gray), img, TEX)),
                    None => Thumb::Failed,
                };
                self.thumbs.insert(id, t);
            }
        }
        // large previews not drawn for a few seconds go (the small ones stay)
        self.pass = ctx.cumulative_pass_nr();
        if self.thumb_used.len() > 120 && self.pass % 60 == 0 {
            let now = self.pass;
            let stale: Vec<ThumbKey> = self.thumb_used.iter().filter(|(_, &at)| now.saturating_sub(at) > 240).map(|(k, _)| k.clone()).collect();
            for k in stale {
                self.thumb_used.remove(&k);
                self.thumbs.remove(&k);
            }
        }
    }

    /// The small colour preview of a picture.
    fn thumb(&mut self, i: usize) -> Option<egui::TextureId> {
        self.thumb_sized(i, THUMB as f32, false, false)
    }

    /// The best preview loaded for a picture drawn `px` physical pixels long,
    /// asking for a sharper one when the loaded ones are too small; `full`
    /// allows the picture itself beyond the largest preview.
    fn thumb_sized(&mut self, i: usize, px: f32, full: bool, gray: bool) -> Option<egui::TextureId> {
        let want = tier_for(px, full);
        let e = &self.entries[i];
        let id = ThumbKey { key: e.key.clone(), tier: want, gray };
        if want != THUMB || gray {
            self.thumb_used.insert(id.clone(), self.pass);
        }
        if !self.thumbs.contains_key(&id) {
            self.thumbs.insert(id.clone(), Thumb::Loading);
            if let Some(l) = &self.loader {
                l.queue.lock().unwrap().push_back(Job { id: id.clone(), abs: PathBuf::from(&e.abs), mtime: e.mtime, size: e.size });
                l.wake.notify_one();
            }
        }
        if let Some(Thumb::Ready(t)) = self.thumbs.get(&id) {
            return Some(t.id());
        }
        // meanwhile the sharpest one already loaded
        for tier in [FULL, 960, 480, THUMB] {
            if let Some(Thumb::Ready(t)) = self.thumbs.get(&ThumbKey { key: e.key.clone(), tier, gray }) {
                return Some(t.id());
            }
        }
        // greys not ready yet: the colour one rather than nothing
        if gray {
            return self.thumb_sized(i, px, full, false);
        }
        None
    }

    fn refresh_shown(&mut self) {
        let key = format!("{:?}|{:?}|{}|{:?}|{}|{}|{:?}", self.folder, self.collection, self.query, self.similar_to, self.entries.len(), self.show_vault, self.order);
        if key == self.shown_key {
            return;
        }
        self.shown_key = key;
        let members = self.collection.as_ref().and_then(|c| self.collections.iter().find(|x| &x.0 == c)).map(|x| &x.1);
        let candidates: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.visible(&self.entries[i]))
            .filter(|&i| self.folder.as_ref().is_none_or(|f| &self.entries[i].folder == f))
            .filter(|&i| match (&self.collection, members) {
                (None, _) => true,
                (Some(c), _) if c.is_empty() => self.entries[i].origin == ateliet_core::images::Origin::Shared,
                (Some(_), Some(m)) => m.contains(&self.entries[i].abs),
                _ => false,
            })
            .collect();
        let index = self.index.clone();
        let ix = index.lock().unwrap();
        self.shown = if let Some(s) = self.similar_to {
            match ix.vector(&self.entries[s].key).map(|v| v.to_vec()) {
                Some(v) => images::rank(&self.entries, &ix, &candidates, &v, &[]).into_iter().map(|x| x.0).collect(),
                None => vec![s],
            }
        } else if self.query.trim().is_empty() {
            let mut c = candidates;
            match self.order {
                Order::Newest => c.sort_by_key(|&i| std::cmp::Reverse(self.entries[i].created)),
                Order::Oldest => c.sort_by_key(|&i| self.entries[i].created),
                Order::Shuffled(seed) => {
                    // the same shuffle until asked again, whatever the scan order
                    c.sort_by(|&a, &b| self.entries[a].key.cmp(&self.entries[b].key));
                    images::shuffle(&mut c, seed);
                }
            }
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

    /// Whether a picture shows in image mode: the vault's own only when
    /// asked for (its Pinterest folder always, where downloads go without
    /// a shared library).
    pub fn visible(&self, e: &ImageEntry) -> bool {
        self.show_vault || e.origin != ateliet_core::images::Origin::Vault || e.key.starts_with("Pinterest/")
    }

    /// Change and keep the order of the pictures.
    pub fn set_order(&mut self, o: Order) {
        self.order = o;
        Self::save_settings(&self.root, serde_json::json!({ "order": o.setting() }));
    }

    /// Folders with pictures: (folder, count, cover index).
    pub fn folders(&self) -> Vec<(String, usize, usize)> {
        let mut m: HashMap<&str, (usize, usize, i64)> = HashMap::new();
        // the shared library has its own rows (its collections)
        for (i, e) in self.entries.iter().enumerate().filter(|(_, e)| e.origin != ateliet_core::images::Origin::Shared && self.visible(e)) {
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
    /// The shared collections read again (after one was made, renamed, filled…).
    fn reload_collections(&mut self) {
        self.collections = self.shared.as_ref().map(|h| ateliet_core::pictogrep::collection_names(h).into_iter().map(|n| {
            let members = ateliet_core::pictogrep::collection_images(h, &n).into_iter().map(|p| p.to_string_lossy().to_string()).collect();
            (n, members)
        }).collect()).unwrap_or_default();
        self.shown_key = String::from("\u{0}");
    }

    /// Pictures put in a shared collection (as links, the way Pictogrep keeps them).
    fn add_to_collection(&mut self, name: &str, idx: &[usize]) -> usize {
        let Some(home) = self.shared.clone() else { return 0 };
        let mut n = 0;
        for &i in idx {
            if ateliet_core::pictogrep::link(&home, name, Path::new(&self.entries[i].abs)).unwrap_or(false) {
                n += 1;
            }
        }
        ateliet_core::pictogrep::library_changed(&home, &idx.iter().map(|&i| PathBuf::from(&self.entries[i].abs)).collect::<Vec<_>>());
        self.reload_collections();
        n
    }

    /// The pictures a drag carries: the selection when the dragged one is in it.
    /// The keys of dragged pictures (indices into the library).
    pub fn keys_of(&self, idx: &[usize]) -> Vec<String> {
        idx.iter().filter_map(|&i| self.entries.get(i)).map(|e| e.key.clone()).collect()
    }

    fn dragged(&self, i: usize) -> Vec<usize> {
        if self.selection.contains(&i) { self.selection.iter().copied().collect() } else { vec![i] }
    }

    pub fn collections_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) -> Option<ImageAction> {
        let mut action = None;
        self.poll(ui.ctx());
        self.pin_tick(ui.ctx());
        self.pin_ui(ui.ctx(), p);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Imagens").font(kit::medium(13.5)).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let running = self.pin.downloads.snapshot().state == "running";
                if kit::icon_button_sized(ui, icons::DOWNLOAD, if running { "Baixando do Pinterest…" } else { "Trazer do Pinterest" }, running, p, 24.0, 13.0).clicked() {
                    self.pin.open = true;
                }
            });
        });
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let all = self.entries.iter().filter(|e| self.visible(e)).count();
        if kit::Row::new("Todas as imagens").icon(icons::IMAGES).trailing(all.to_string()).selected(self.folder.is_none() && self.collection.is_none() && !self.mb.open).show(ui, p).clicked() {
            self.close_moodboards();
            self.folder = None;
            self.collection = None;
            self.similar_to = None;
            self.viewer = None;
        }
        if self.shared.is_some() {
            let n = self.entries.iter().filter(|e| e.origin == ateliet_core::images::Origin::Shared).count();
            if kit::Row::new("Biblioteca compartilhada").icon(icons::LIBRARY_BIG).trailing(n.to_string()).selected(self.collection.as_deref() == Some("") && !self.mb.open).show(ui, p).on_hover_text("A pasta do Pictogrep: as mesmas imagens e coleções nos dois apps").clicked() {
                self.close_moodboards();
                self.folder = None;
                self.collection = Some(String::new());
                self.similar_to = None;
                self.viewer = None;
            }
            let names: Vec<(String, usize)> = self.collections.iter().map(|(n, m)| (n.clone(), m.len())).collect();
            let home = self.shared.clone();
            for (name, n) in names {
                // renaming in place
                if let Some((Some(old), text)) = &mut self.col_edit {
                    if *old == name {
                        let r = ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width()).margin(egui::Margin::symmetric(8, 4)));
                        let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                        if !(enter || esc || r.clicked_elsewhere()) {
                            r.request_focus();
                        } else {
                            let (old, text) = (old.clone(), text.trim().to_string());
                            if enter && !text.is_empty() && text != old {
                                if let Some(h) = &home {
                                    match ateliet_core::pictogrep::rename_collection(h, &old, &text) {
                                        Ok(new) => {
                                            if self.collection.as_deref() == Some(old.as_str()) {
                                                self.collection = Some(new);
                                            }
                                        }
                                        Err(e) => self.notice = Some(e),
                                    }
                                }
                            }
                            self.col_edit = None;
                            self.reload_collections();
                        }
                        continue;
                    }
                }
                let r = kit::Row::new(&name).icon(icons::FOLDER_CLOSED).depth(1).trailing(n.to_string()).selected(self.collection.as_deref() == Some(name.as_str()) && !self.mb.open).show(ui, p);
                // pictures dropped on a collection join it
                if let Some(idx) = r.dnd_release_payload::<Vec<usize>>() {
                    let k = self.add_to_collection(&name, &idx);
                    self.notice = Some(format!("{k} na coleção {name}"));
                }
                if r.dnd_hover_payload::<Vec<usize>>().is_some() {
                    ui.painter().rect_stroke(r.rect, 6.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                }
                if r.clicked() {
                    self.close_moodboards();
                    self.folder = None;
                    self.collection = Some(name.clone());
                    self.similar_to = None;
                    self.viewer = None;
                }
                r.context_menu(|ui| {
                    if kit::menu_item(ui, icons::PEN_LINE, "Renomear", p) {
                        self.col_edit = Some((Some(name.clone()), name.clone()));
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, icons::TRASH_2, "Excluir a coleção (as imagens ficam)", p) {
                        if let Some(h) = &home {
                            let _ = ateliet_core::pictogrep::delete_collection(h, &name);
                        }
                        if self.collection.as_deref() == Some(name.as_str()) {
                            self.collection = None;
                        }
                        self.reload_collections();
                        ui.close_menu();
                    }
                });
            }
            // a new collection: named, Enter
            if let Some((None, text)) = &mut self.col_edit {
                let r = ui.add(egui::TextEdit::singleline(text).hint_text("Nome da coleção").desired_width(ui.available_width()).margin(egui::Margin::symmetric(8, 4)));
                let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if !(enter || esc || r.clicked_elsewhere()) {
                    r.request_focus();
                } else {
                    let text = text.trim().to_string();
                    if enter && !text.is_empty() {
                        if let Some(h) = &home {
                            match ateliet_core::pictogrep::create_collection(h, &text) {
                                Ok(n) => {
                                    // the selected pictures go in right away
                                    let sel: Vec<usize> = self.selection.iter().copied().collect();
                                    if !sel.is_empty() {
                                        self.add_to_collection(&n, &sel);
                                    }
                                }
                                Err(e) => self.notice = Some(e),
                            }
                        }
                    }
                    self.col_edit = None;
                    self.reload_collections();
                }
            } else if kit::Row::new("Nova coleção").icon(icons::PLUS).depth(1).show(ui, p).clicked() {
                self.col_edit = Some((None, String::new()));
            }
        }
        // moodboards: the list, and each board
        let boards = self.moodboard_list();
        let open_board = self.open_board_id();
        if kit::Row::new("Moodboards").icon(icons::LAYOUT_DASHBOARD).trailing(boards.len().to_string()).selected(self.mb.open && open_board.is_none()).show(ui, p).on_hover_text("Quadros de referência com as imagens da biblioteca").clicked() {
            self.show_moodboards(None);
        }
        if self.mb.open {
            for b in &boards {
                let r = kit::Row::new(&b.name).icon(icons::FRAME).depth(1).trailing(b.count.to_string()).selected(open_board.as_deref() == Some(b.id.as_str())).show(ui, p);
                // pictures dropped on a board go on it
                if let Some(idx) = r.dnd_release_payload::<Vec<usize>>() {
                    self.queue_add_to_board(&b.id, (*idx).clone());
                }
                if r.dnd_hover_payload::<Vec<usize>>().is_some() {
                    ui.painter().rect_stroke(r.rect, 6.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                }
                if r.clicked() {
                    self.show_moodboards(Some(&b.id));
                }
            }
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
                // pictures of the vault dropped on a folder move there (links follow)
                if let Some(idx) = r.dnd_release_payload::<Vec<usize>>() {
                    let keys: Vec<String> = idx.iter().filter(|&&i| self.entries[i].origin == ateliet_core::images::Origin::Vault && self.entries[i].folder != folder).map(|&i| self.entries[i].key.clone()).collect();
                    if !keys.is_empty() {
                        action = Some(ImageAction::MoveTo(keys, folder.clone()));
                    }
                }
                if r.dnd_hover_payload::<Vec<usize>>().is_some() {
                    ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                }
                if r.on_hover_text(&folder).clicked() {
                    self.close_moodboards();
                    self.collection = None;
                    self.folder = Some(folder.clone());
                    self.similar_to = None;
                    self.viewer = None;
                }
            }
        });
        action
    }

    /// The mosaic or the viewer (centre in image mode).
    pub fn center_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette, vault: &vault::Vault) -> Option<ImageAction> {
        self.poll(ui.ctx());
        if self.mb.open {
            // the board takes the whole centre (the tools float over it)
            let mut action = None;
            if self.mb.board_open() {
                self.moodboard_ui(ui, p);
            } else {
                ui.horizontal(|ui| {
                    if let Some(a) = panel_toggle(ui, self.panels, true, p) {
                        action = Some(a);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(a) = panel_toggle(ui, self.panels, false, p) {
                            action = Some(a);
                        }
                    });
                });
                ui.add_space(10.0);
                self.moodboard_ui(ui, p);
            }
            return action;
        }
        if self.viewer.is_some() {
            return self.viewer_ui(ui, p, vault);
        }
        let mut action = None;
        // the notes toggle sits at the right edge, the rest wraps before it
        let bar = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 30.0));
        let right = egui::Rect::from_min_max(egui::pos2(bar.right() - 30.0, bar.top()), bar.max);
        // a child that doesn't move the cursor: the bar starts at the same top
        let mut corner = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if let Some(a) = panel_toggle(&mut corner, self.panels, false, p) {
            action = Some(a);
        }
        ui.scope(|ui| {
            ui.set_max_width(ui.available_width() - 36.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if let Some(a) = panel_toggle(ui, self.panels, true, p) {
                    action = Some(a);
                }
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
                ui.add_space(4.0);
                // the order: by when they were saved, or shuffled
                let (glyph, tip) = match self.order {
                    Order::Newest => (icons::CALENDAR_CLOCK, "Salvas por último primeiro"),
                    Order::Oldest => (icons::CALENDAR_RANGE, "Na ordem em que foram salvas"),
                    Order::Shuffled(_) => (icons::SHUFFLE, "Embaralhadas"),
                };
                let or = kit::icon_button_sized(ui, glyph, tip, false, p, 26.0, 14.0);
                let oid = ui.make_persistent_id("images-order");
                if or.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(oid));
                }
                let mut pick = None;
                egui::popup_below_widget(ui, oid, &or, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(220.0);
                    for (o, g, l) in [(Order::Newest, icons::CALENDAR_CLOCK, "Salvas por último primeiro"), (Order::Oldest, icons::CALENDAR_RANGE, "Na ordem em que foram salvas"), (Order::Shuffled(0), icons::SHUFFLE, "Embaralhar")] {
                        let on = std::mem::discriminant(&o) == std::mem::discriminant(&self.order);
                        if kit::menu_entry(ui, g, l, if on { "✓" } else { "" }, true, false, p) {
                            pick = Some(o);
                        }
                    }
                });
                if matches!(self.order, Order::Shuffled(_)) && kit::icon_button_sized(ui, icons::REFRESH_CW, "Embaralhar de novo", false, p, 26.0, 14.0).clicked() {
                    pick = Some(Order::Shuffled(0));
                }
                if let Some(o) = pick {
                    self.set_order(if let Order::Shuffled(_) = o { Order::Shuffled(new_seed()) } else { o });
                }
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
                if let Some(f) = self.folder.clone().filter(|f| !f.is_empty() && !Path::new(f).is_absolute()) {
                    if self.selection.is_empty() {
                        ui.add_space(8.0);
                        let name = f.rsplit('/').next().unwrap_or(&f).to_string();
                        if kit::button(ui, Some(icons::FILE_TEXT), "Nota da coleção", p).on_hover_text(format!("{f}/{name}.md: o que esta pasta reúne")).clicked() {
                            action = Some(ImageAction::CollectionNote(f));
                        }
                    }
                }
                if self.scanning.is_some() {
                    ui.spinner();
                }
                #[cfg(feature = "clip")]
                self.vision_bar(ui, p);
            });
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
        // justified rows: every picture in its own proportions, the rows
        // about the chosen height, stretched to fill the width
        let avail_w = ui.available_width() - 4.0;
        let shown = self.shown.clone();
        let rows = justified_rows(&shown.iter().map(|&i| aspect(&self.entries[i])).collect::<Vec<_>>(), avail_w, self.cell, gap);
        let total = rows.last().map(|r| r.y + r.h).unwrap_or(0.0);
        let mut open: Option<usize> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, view| {
            let origin = ui.min_rect().min;
            ui.allocate_space(egui::vec2(avail_w, total));
            for row in rows.iter().filter(|r| r.y + r.h >= view.top() - 200.0 && r.y <= view.bottom() + 200.0) {
                let mut x = 0.0;
                for (k, w) in row.widths.iter().enumerate() {
                    let i = shown[row.start + k];
                    let rect = egui::Rect::from_min_size(origin + egui::vec2(x, row.y), egui::vec2(*w, row.h));
                    x += w + gap;
                    let resp = ui.interact(rect, ui.id().with(("tile", i)), Sense::click_and_drag());
                if resp.drag_started() {
                    resp.dnd_set_drag_payload(self.dragged(i));
                }
                ui.painter().rect_filled(rect, 10.0, p.bg_secondary);
                // a preview as sharp as the tile is large on this screen
                let px = rect.width().max(rect.height()) * ui.ctx().pixels_per_point();
                if let Some(t) = self.thumb_sized(i, px, false, false) {
                    // the whole picture: the tile already has its proportions
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().add(egui::epaint::RectShape::filled(rect, 10.0, Color32::WHITE).with_texture(t, uv));
                }
                let sel = self.selection.contains(&i);
                if resp.hovered() || sel {
                    // a soft shade at the foot with the name
                    let foot = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 30.0), rect.max);
                    ui.painter().rect_filled(foot, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, Color32::from_black_alpha(110));
                    let mut job = egui::text::LayoutJob::single_section(self.entries[i].name.clone(), egui::TextFormat { font_id: egui::FontId::proportional(11.5), color: Color32::WHITE, ..Default::default() });
                    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 20.0);
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
                    if kit::menu_item(ui, icons::SCAN_SEARCH, "Imagens semelhantes", p) {
                        self.similar_to = Some(i);
                        self.query.clear();
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, icons::CORNER_DOWN_RIGHT, "Inserir na nota", p) {
                        action = Some(ImageAction::Insert(vec![self.entries[i].key.clone()]));
                        ui.close_menu();
                    }
                    let boards = self.mb.boards.clone();
                    ui.menu_button("Adicionar ao moodboard", |ui| {
                        if boards.is_empty() {
                            ui.label(RichText::new("Nenhum moodboard ainda").size(12.0).color(p.faint));
                        }
                        for b in &boards {
                            if ui.button(&b.name).clicked() {
                                let idx = self.dragged(i);
                                self.queue_add_to_board(&b.id, idx);
                                ui.close_menu();
                            }
                        }
                    });
                    if self.shared.is_some() {
                        let names: Vec<String> = self.collections.iter().map(|c| c.0.clone()).collect();
                        ui.menu_button("Adicionar à coleção", |ui| {
                            for n in names {
                                if ui.button(&n).clicked() {
                                    let idx = self.dragged(i);
                                    let k = self.add_to_collection(&n, &idx);
                                    self.notice = Some(format!("{k} na coleção {n}"));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    if kit::menu_item(ui, icons::SQUARE_CHECK, if sel { "Desmarcar" } else { "Selecionar" }, p) {
                        if !self.selection.remove(&i) {
                            self.selection.insert(i);
                        }
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, icons::NOTEBOOK_PEN, "Mandar ao Journal", p) {
                        let idx = self.dragged(i);
                        action = Some(ImageAction::SendToJournal(self.keys_of(&idx)));
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", p) {
                        let _ = open::that(&self.entries[i].abs);
                        ui.close_menu();
                    }
                });
                }
            }
        });
        if let Some(i) = open {
            self.viewer = Some(Viewer { idx: i, tex: None, zoom: 1.0, offset: egui::Vec2::ZERO, fit: true, full: false });
        }
        action
    }

    /// The pictures in the side panel of the writing mode: search, small
    /// squares; a click puts the picture in the note.
    pub fn side_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) -> Option<ImageAction> {
        self.poll(ui.ctx());
        self.refresh_shown();
        if let Some(key) = self.side_view.clone() {
            match self.shown.iter().position(|&i| self.entries[i].key == key) {
                Some(pos) => return self.side_picture(ui, pos, p),
                None => self.side_view = None,
            }
        }
        let mut action = None;
        ui.horizontal(|ui| {
            let w = ui.available_width() - 62.0;
            ui.allocate_ui(egui::vec2(w.max(80.0), 26.0), |ui| kit::search_field(ui, &mut self.query, "Buscar imagens", p));
            self.order_buttons(ui, p);
        });
        ui.add_space(8.0);
        self.refresh_shown();
        if self.shown.is_empty() {
            ui.label(RichText::new(if self.scanning.is_some() { "Procurando imagens…" } else { "Nenhuma imagem." }).size(12.5).color(p.faint));
            return None;
        }
        let gap = 6.0;
        // room for the scroll bar
        let avail_w = ui.available_width() - 14.0;
        let cols = ((avail_w + gap) / (88.0 + gap)).floor().max(2.0) as usize;
        let cell = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let rows = self.shown.len().div_ceil(cols);
        let shown = self.shown.clone();
        egui::ScrollArea::vertical().id_salt("side-pictures").auto_shrink([false, false]).show_rows(ui, cell + gap, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in 0..cols {
                        let Some(&i) = shown.get(r * cols + c) else { break };
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, cell), Sense::click_and_drag());
                        ui.painter().rect_filled(rect, 6.0, p.bg_hover);
                        if let Some(t) = self.thumb(i) {
                            let e = &self.entries[i];
                            let k = (e.width.max(1) as f32 / e.height.max(1) as f32).max(1e-3);
                            let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
                            ui.painter().add(egui::epaint::RectShape::filled(rect, 6.0, Color32::WHITE).with_texture(t, uv));
                        }
                        if resp.hovered() {
                            ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                        }
                        if resp.drag_started() {
                            resp.dnd_set_drag_payload(vec![i]);
                        }
                        if resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            self.drag_ghost(ui, i, p);
                        }
                        let name = self.entries[i].name.clone();
                        let resp = resp.on_hover_text(format!("{name}\nClique para ver; arraste para a nota"));
                        if resp.clicked() {
                            self.side_view = Some(self.entries[i].key.clone());
                        }
                        resp.context_menu(|ui| {
                            if kit::menu_item(ui, icons::IMAGE, "Ver aqui", p) {
                                self.side_view = Some(self.entries[i].key.clone());
                            }
                            if kit::menu_item(ui, icons::FILE_PLUS, "Pôr na nota", p) {
                                action = Some(ImageAction::Insert(vec![self.entries[i].key.clone()]));
                            }
                            if kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", p) {
                                let _ = open::that(&self.entries[i].abs);
                            }
                        });
                    }
                });
                ui.add_space(gap - ui.spacing().item_spacing.y);
            }
        });
        action
    }

    /// The order of the pictures (shared with image mode) and, when
    /// shuffled, a new shuffle.
    fn order_buttons(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        ui.spacing_mut().item_spacing.x = 2.0;
        let (glyph, tip) = match self.order {
            Order::Newest => (icons::CALENDAR_CLOCK, "Salvas por último primeiro"),
            Order::Oldest => (icons::CALENDAR_RANGE, "Na ordem em que foram salvas"),
            Order::Shuffled(_) => (icons::SHUFFLE, "Embaralhadas"),
        };
        let or = kit::icon_button_sized(ui, glyph, tip, false, p, 26.0, 14.0);
        let oid = ui.make_persistent_id("side-images-order");
        if or.clicked() {
            ui.memory_mut(|m| m.toggle_popup(oid));
        }
        let mut pick = None;
        egui::popup_below_widget(ui, oid, &or, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(220.0);
            for (o, g, l) in [(Order::Newest, icons::CALENDAR_CLOCK, "Salvas por último primeiro"), (Order::Oldest, icons::CALENDAR_RANGE, "Na ordem em que foram salvas"), (Order::Shuffled(0), icons::SHUFFLE, "Embaralhar")] {
                let on = std::mem::discriminant(&o) == std::mem::discriminant(&self.order);
                if kit::menu_entry(ui, g, l, if on { "✓" } else { "" }, true, false, p) {
                    pick = Some(o);
                }
            }
        });
        let again = matches!(self.order, Order::Shuffled(_));
        if kit::icon_button_sized(ui, if again { icons::REFRESH_CW } else { icons::SHUFFLE }, if again { "Embaralhar de novo" } else { "Embaralhar" }, false, p, 26.0, 14.0).clicked() {
            pick = Some(Order::Shuffled(0));
        }
        if let Some(o) = pick {
            self.set_order(if let Order::Shuffled(_) = o { Order::Shuffled(new_seed()) } else { o });
        }
    }

    /// A small copy of the picture under the pointer while it is dragged.
    fn drag_ghost(&mut self, ui: &egui::Ui, i: usize, p: crate::theme::Palette) {
        let Some(pos) = ui.ctx().pointer_interact_pos() else { return };
        let Some(t) = self.thumb(i) else { return };
        let e = &self.entries[i];
        let k = e.width.max(1) as f32 / e.height.max(1) as f32;
        let size = if k > 1.0 { egui::vec2(96.0, 96.0 / k) } else { egui::vec2(96.0 * k, 96.0) };
        let rect = egui::Rect::from_min_size(pos + egui::vec2(10.0, 10.0), size);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("side-picture-ghost")));
        painter.rect_filled(rect.expand(2.0), 6.0, p.bg);
        painter.add(egui::epaint::RectShape::filled(rect, 5.0, Color32::from_white_alpha(220)).with_texture(t, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))));
    }

    /// One picture large in the side panel: back to the grid, the ones
    /// before and after, drag it onto the note.
    fn side_picture(&mut self, ui: &mut egui::Ui, pos: usize, p: crate::theme::Palette) -> Option<ImageAction> {
        let mut action = None;
        let i = self.shown[pos];
        let e = self.entries[i].clone();
        let n = self.shown.len();
        let mut go: Option<usize> = None;
        let mut back = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if kit::icon_button(ui, icons::ARROW_LEFT, "Voltar às imagens", false, p).clicked() {
                back = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", false, p).clicked() {
                    let _ = open::that(&e.abs);
                }
                if kit::icon_button(ui, icons::CHEVRON_RIGHT, "Próxima", false, p).clicked() {
                    go = Some((pos + 1) % n);
                }
                ui.label(RichText::new(format!("{} / {n}", pos + 1)).size(11.5).color(p.faint));
                if kit::icon_button(ui, icons::CHEVRON_LEFT, "Anterior", false, p).clicked() {
                    go = Some((pos + n - 1) % n);
                }
            });
        });
        ui.add_space(6.0);
        let avail = ui.available_size();
        let k = e.width.max(1) as f32 / e.height.max(1) as f32;
        let max_h = (avail.y - 70.0).max(120.0);
        let mut size = egui::vec2(avail.x, avail.x / k);
        if size.y > max_h {
            size = egui::vec2(max_h * k, max_h);
        }
        let (row, _) = ui.allocate_exact_size(egui::vec2(avail.x, size.y), Sense::hover());
        let rect = egui::Rect::from_center_size(row.center(), size);
        let resp = ui.interact(rect, ui.id().with(("side-picture", i)), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 6.0, p.bg_hover);
        let px = size.x.max(size.y) * ui.ctx().pixels_per_point();
        if let Some(t) = self.thumb_sized(i, px, true, false) {
            ui.painter().add(egui::epaint::RectShape::filled(rect, 6.0, Color32::WHITE).with_texture(t, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))));
        }
        if resp.drag_started() {
            resp.dnd_set_drag_payload(vec![i]);
        }
        if resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            self.drag_ghost(ui, i, p);
        } else if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        resp.on_hover_text("Arraste para a nota");
        ui.add_space(8.0);
        ui.label(RichText::new(&e.name).font(kit::medium(13.0)).color(p.text));
        ui.label(RichText::new(format!("{} × {}", e.width, e.height)).size(11.5).color(p.faint));
        ui.add_space(6.0);
        if kit::button(ui, Some(icons::FILE_PLUS), "Pôr na nota", p).on_hover_text("No ponto do cursor da nota").clicked() {
            action = Some(ImageAction::Insert(vec![e.key.clone()]));
        }
        // ← and → walk through them with the pointer over the panel, while
        // nothing is being typed in (a storyboard uses them for its frames)
        if !ui.ctx().wants_keyboard_input() && ui.ui_contains_pointer() {
            if ui.input(|inp| inp.key_pressed(egui::Key::ArrowRight)) {
                go = Some((pos + 1) % n);
            } else if ui.input(|inp| inp.key_pressed(egui::Key::ArrowLeft)) {
                go = Some((pos + n - 1) % n);
            } else if ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
                back = true;
            }
        }
        if let Some(g) = go {
            self.side_view = Some(self.entries[self.shown[g]].key.clone());
        }
        if back {
            self.side_view = None;
        }
        action
    }

    /// The pictures in the Journal's panel: search, squares to drag onto a
    /// page.
    pub fn material_ui(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        self.poll(ui.ctx());
        kit::search_field(ui, &mut self.query, "Buscar imagens", p);
        ui.add_space(8.0);
        // the vault's own pictures too (image mode may hide them)
        let keep = self.show_vault;
        self.show_vault = true;
        self.refresh_shown();
        self.show_vault = keep;
        if self.shown.is_empty() {
            ui.label(RichText::new(if self.scanning.is_some() { "Procurando imagens…" } else { "Nenhuma imagem." }).size(12.5).color(p.faint));
            return;
        }
        let gap = 6.0;
        let avail_w = ui.available_width() - 14.0;
        let cols = ((avail_w + gap) / (96.0 + gap)).floor().max(2.0) as usize;
        let cell = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let rows = self.shown.len().div_ceil(cols);
        let shown = self.shown.clone();
        egui::ScrollArea::vertical().id_salt("journal-pictures").auto_shrink([false, false]).show_rows(ui, cell + gap, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in 0..cols {
                        let Some(&i) = shown.get(r * cols + c) else { break };
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, cell), Sense::click_and_drag());
                        ui.painter().rect_filled(rect, 6.0, p.bg_hover);
                        if let Some(t) = self.thumb(i) {
                            let e = &self.entries[i];
                            let k = (e.width.max(1) as f32 / e.height.max(1) as f32).max(1e-3);
                            let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
                            ui.painter().add(egui::epaint::RectShape::filled(rect, 6.0, Color32::WHITE).with_texture(t, uv));
                        }
                        if resp.hovered() {
                            ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if resp.drag_started() {
                            resp.dnd_set_drag_payload(crate::journal_ui::Payload::Picture(self.entries[i].key.clone()));
                        }
                        let name = self.entries[i].name.clone();
                        resp.on_hover_text(name);
                    }
                });
                ui.add_space(gap - ui.spacing().item_spacing.y);
            }
        });
    }

    /// Pictures like this one (from the file explorer).
    pub fn show_similar(&mut self, key: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.key == key) {
            self.folder = None;
            self.collection = None;
            self.query.clear();
            self.similar_to = Some(i);
            self.viewer = None;
        } else if self.scanning.is_some() {
            self.pending_similar = Some(key.to_string());
        }
    }

    /// "Trazer do Pinterest…".
    pub fn open_pinterest(&mut self) {
        self.pin.open = true;
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
        let panels = self.panels;
        let mut toggle = None;
        ui.horizontal(|ui| {
            if let Some(a) = panel_toggle(ui, panels, true, p) {
                toggle = Some(a);
            }
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if kit::icon_button(ui, icons::ARROW_LEFT, "Voltar (Esc)", false, p).clicked() || (esc && !v.full) {
                close = true;
            }
            if esc && v.full {
                v.full = false;
            }
            ui.add_space(4.0);
            ui.label(RichText::new(&e.name).font(kit::medium(14.0)).color(p.text));
            // the size only where the buttons on the right leave room for it
            if ui.available_width() > 380.0 {
                ui.label(RichText::new(format!("{} × {}", e.width, e.height)).size(12.0).color(p.faint));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if let Some(a) = panel_toggle(ui, panels, false, p) {
                    toggle = Some(a);
                }
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
                // drag this onto the note beside (or onto a collection)
                let (hr, handle) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::drag());
                ui.painter().rect_filled(hr, 6.0, if handle.hovered() || handle.dragged() { p.bg_hover } else { Color32::TRANSPARENT });
                ui.painter().text(hr.center(), egui::Align2::CENTER_CENTER, icons::GRIP_VERTICAL, kit::icon_font(15.0), if handle.hovered() { p.text } else { p.muted });
                if handle.drag_started() {
                    handle.dnd_set_drag_payload(vec![v.idx]);
                }
                if handle.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else {
                    handle.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Arraste para a nota ou para uma coleção");
                }
                if kit::icon_button(ui, icons::SCAN_SEARCH, "Tamanho real (1)", false, p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Num1)) {
                    v.fit = false;
                    v.zoom = 1.0;
                    v.offset = egui::Vec2::ZERO;
                }
                if kit::icon_button(ui, icons::MAXIMIZE, "Ajustar (0)", false, p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Num0)) {
                    v.fit = true;
                    v.offset = egui::Vec2::ZERO;
                }
                if kit::icon_button(ui, icons::MAXIMIZE_2, "Tela cheia (F)", v.full, p).clicked() {
                    v.full = !v.full;
                    v.fit = true;
                    v.offset = egui::Vec2::ZERO;
                }
            });
        });
        ui.add_space(8.0);
        // F: the whole window; + and −: zoom
        let typing = ui.ctx().wants_keyboard_input();
        if !typing && ui.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.is_none()) {
            v.full = !v.full;
            v.fit = true;
            v.offset = egui::Vec2::ZERO;
        }
        let (zin, zout) = ui.input(|i| (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals), i.key_pressed(egui::Key::Minus)));
        if !typing && (zin || zout) {
            v.zoom = (v.zoom * if zin { 1.25 } else { 0.8 }).clamp(0.05, 16.0);
            v.fit = false;
        }
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
            if v.full {
                // the picture alone over the whole window, on black
                let screen = ui.ctx().screen_rect();
                let tid = t.id();
                let k = if v.fit { (screen.width() / size.x).min(screen.height() / size.y).min(1.0) } else { v.zoom };
                egui::Area::new(egui::Id::new("viewer-full")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ui.ctx(), |ui| {
                    let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
                    ui.painter().rect_filled(r, 0.0, Color32::BLACK);
                    let img = egui::Rect::from_center_size(r.center() + v.offset, size * k);
                    ui.painter().image(tid, img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    ui.painter().text(r.left_top() + egui::vec2(16.0, 14.0), egui::Align2::LEFT_TOP, format!("{}   ·   Esc sai, ← → passam, + − zoom", e.name), egui::FontId::proportional(12.0), Color32::from_gray(150));
                    if resp.dragged() {
                        v.offset += resp.drag_delta();
                    }
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        v.zoom = (k * (1.0 + scroll * 0.002)).clamp(0.05, 16.0);
                        v.fit = false;
                    }
                    if resp.double_clicked() {
                        v.full = false;
                    }
                });
            }
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
            *v = Viewer { idx: i, tex: None, zoom: 1.0, offset: egui::Vec2::ZERO, fit: true, full: false };
        } else if step != 0 {
            if let Some(pos) = self.shown.iter().position(|&i| i == v.idx) {
                let n = self.shown.len() as i64;
                let next = self.shown[((pos as i64 + step).rem_euclid(n)) as usize];
                *v = Viewer { idx: next, tex: None, zoom: 1.0, offset: egui::Vec2::ZERO, fit: true, full: v.full };
            }
        }
        if close {
            self.viewer = None;
        }
        toggle.or(action)
    }
}

/// A picture's width over its height, kept within reason for odd sizes.
fn aspect(e: &ImageEntry) -> f32 {
    if e.width == 0 || e.height == 0 {
        return 1.0;
    }
    (e.width as f32 / e.height as f32).clamp(0.25, 4.0)
}

struct JRow {
    start: usize,
    y: f32,
    h: f32,
    widths: Vec<f32>,
}

/// Lays pictures out in rows of about `target` height that fill `width`
/// exactly; the last row keeps the target height instead of stretching.
fn justified_rows(aspects: &[f32], width: f32, target: f32, gap: f32) -> Vec<JRow> {
    let mut rows = Vec::new();
    let (mut start, mut y) = (0, 0.0);
    while start < aspects.len() {
        let mut end = start;
        let mut sum = 0.0;
        while end < aspects.len() {
            sum += aspects[end];
            end += 1;
            if sum * target + gap * (end - start - 1) as f32 >= width {
                break;
            }
        }
        let gaps = gap * (end - start - 1) as f32;
        let full = sum * target + gaps >= width;
        // a single very wide picture never gets taller than the target
        let h = if full { ((width - gaps) / sum).min(target * 2.0) } else { target };
        let mut widths: Vec<f32> = aspects[start..end].iter().map(|a| (a * h).floor()).collect();
        // what flooring left over goes to the last picture, so the row ends flush
        if full && h < target * 2.0 {
            let used: f32 = widths.iter().sum::<f32>() + gaps;
            if let Some(w) = widths.last_mut() {
                *w += width - used;
            }
        }
        rows.push(JRow { start, y, h, widths });
        y += h + gap;
        start = end;
    }
    rows
}

fn panel_toggle(ui: &mut egui::Ui, panels: [bool; 2], left: bool, p: crate::theme::Palette) -> Option<ImageAction> {
    let open = panels[if left { 0 } else { 1 }];
    let (glyph, tip) = match (left, open) {
        (true, true) => (icons::PANEL_LEFT_CLOSE, "Ocultar coleções"),
        (true, false) => (icons::PANEL_LEFT_OPEN, "Mostrar coleções"),
        (false, true) => (icons::PANEL_RIGHT_CLOSE, "Ocultar notas"),
        (false, false) => (icons::PANEL_RIGHT_OPEN, "Mostrar notas"),
    };
    kit::icon_button(ui, glyph, tip, false, p).clicked().then_some(ImageAction::TogglePanel(left))
}

#[cfg(test)]
mod tests {
    use super::justified_rows;

    #[test]
    fn rows_fill_the_width_and_keep_proportions() {
        let rows = justified_rows(&[1.0, 2.0, 0.5, 1.5, 1.0, 0.5], 600.0, 200.0, 10.0);
        let first = &rows[0];
        let used: f32 = first.widths.iter().sum::<f32>() + 10.0 * (first.widths.len() - 1) as f32;
        assert!((used - 600.0).abs() < 0.01, "{used}");
        // the widths keep each picture's proportions at the row's height
        assert!((first.widths[1] - 2.0 * first.h).abs() < 1.0);
        // the last row is not stretched
        let last = rows.last().unwrap();
        assert_eq!(last.h, 200.0);
        assert_eq!(rows[1].y, first.h + 10.0);
    }
}
