//! The image library of each open vault: a background indexer that prepares
//! every picture for search by description, the CLIP model download, and the
//! commands the image views call.
//!
//! One `Gallery` per window. It scans the vault (and the external folders
//! the user added), keeps the index in `.obsidian/ateliet/images/index.bin`,
//! and analyses new or edited pictures a few at a time in its own thread.

use ateliet_core::clip::{self, Clip};
use ateliet_core::images::{self, ImageEntry, Index, Source};
use notify::Watcher;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

type Res<T> = Result<T, String>;

const BATCH: usize = 8;
const THUMB_SIZE: u32 = 480;

pub fn model_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ATELIET_CLIP_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ateliet").join("models").join(clip::MODEL_KEY)
}

pub fn thumb_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("ateliet").join("thumbnails")
}

// ── the model, shared by every window ─────────────────────────────────────

#[derive(Default)]
pub struct Model {
    clip: Mutex<Option<Arc<Clip>>>,
    load_error: Mutex<Option<String>>,
    downloading: AtomicBool,
    received: AtomicU64,
    total: AtomicU64,
    download_error: Mutex<Option<String>>,
    queries: Mutex<HashMap<String, Vec<f32>>>,
}

#[derive(Serialize, Clone)]
pub struct ModelStatus {
    ready: bool,
    loaded: bool,
    downloading: bool,
    received: u64,
    total: u64,
    error: Option<String>,
    size: u64,
}

impl Model {
    fn status(&self) -> ModelStatus {
        ModelStatus {
            ready: clip::model_ready(&model_dir()),
            loaded: self.clip.lock().unwrap().is_some(),
            downloading: self.downloading.load(Ordering::Relaxed),
            received: self.received.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            error: self.download_error.lock().unwrap().clone().or_else(|| self.load_error.lock().unwrap().clone()),
            size: clip::FILES.iter().map(|f| f.2).sum(),
        }
    }

    /// The loaded model, loading it on first use. `None` until downloaded.
    fn get(&self) -> Option<Arc<Clip>> {
        let mut slot = self.clip.lock().unwrap();
        if let Some(c) = slot.as_ref() {
            return Some(c.clone());
        }
        let dir = model_dir();
        if !clip::model_ready(&dir) || self.load_error.lock().unwrap().is_some() {
            return None;
        }
        match Clip::load(&dir) {
            Ok(c) => {
                let c = Arc::new(c);
                *slot = Some(c.clone());
                Some(c)
            }
            Err(e) => {
                eprintln!("image search model: {e}");
                *self.load_error.lock().unwrap() = Some(e);
                None
            }
        }
    }

    fn text_vector(&self, clip: &Clip, query: &str) -> Res<Vec<f32>> {
        let key = query.trim().to_lowercase();
        if let Some(v) = self.queries.lock().unwrap().get(&key) {
            return Ok(v.clone());
        }
        let v = clip.embed_text(&key)?;
        let mut cache = self.queries.lock().unwrap();
        if cache.len() > 256 {
            cache.clear();
        }
        cache.insert(key, v.clone());
        Ok(v)
    }

    fn download(self: &Arc<Self>, app: AppHandle) {
        if self.downloading.swap(true, Ordering::SeqCst) {
            return;
        }
        *self.download_error.lock().unwrap() = None;
        *self.load_error.lock().unwrap() = None;
        let me = self.clone();
        std::thread::spawn(move || {
            let result = me.fetch_files(&app);
            me.downloading.store(false, Ordering::SeqCst);
            if let Err(e) = &result {
                *me.download_error.lock().unwrap() = Some(e.clone());
            }
            let _ = app.emit("images-model", me.status());
        });
    }

    fn fetch_files(&self, app: &AppHandle) -> Res<()> {
        let dir = model_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let pending: Vec<_> = clip::FILES.iter().filter(|(_, local, _)| !dir.join(local).is_file()).collect();
        self.total.store(pending.iter().map(|f| f.2).sum(), Ordering::Relaxed);
        self.received.store(0, Ordering::Relaxed);
        let mut last_emit = Instant::now();
        let agent = ureq::AgentBuilder::new().try_proxy_from_env(true).timeout_connect(Duration::from_secs(30)).build();
        for (remote, local, approx) in pending {
            let url = format!("https://huggingface.co/{}/resolve/{}/{}", clip::REPO, clip::REVISION, remote);
            let resp = agent.get(&url).call().map_err(|e| format!("{remote}: {e}"))?;
            if let Some(len) = resp.header("content-length").and_then(|v| v.parse::<u64>().ok()) {
                self.total.fetch_add(len, Ordering::Relaxed);
                self.total.fetch_sub(*approx, Ordering::Relaxed);
            }
            let part = dir.join(format!("{local}.part"));
            let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
            let mut reader = resp.into_reader();
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = reader.read(&mut buf).map_err(|e| format!("{remote}: {e}"))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                self.received.fetch_add(n as u64, Ordering::Relaxed);
                if last_emit.elapsed() > Duration::from_millis(250) {
                    last_emit = Instant::now();
                    let _ = app.emit("images-model", self.status());
                }
            }
            out.sync_all().map_err(|e| e.to_string())?;
            drop(out);
            fs::rename(&part, dir.join(local)).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

// ── one vault's library ───────────────────────────────────────────────────

#[derive(Default)]
struct Library {
    sources: Vec<Source>,
    entries: Vec<ImageEntry>,
    by_key: HashMap<String, usize>,
    index: Index,
    dirty: bool,
    failed: HashSet<String>,
    scanned: bool,
    auto: bool,
    working: bool,
    last_save: Option<Instant>,
}

impl Library {
    fn set_entries(&mut self, entries: Vec<ImageEntry>) {
        self.by_key = entries.iter().enumerate().map(|(i, e)| (e.key.clone(), i)).collect();
        self.entries = entries;
    }

    fn pending(&self, limit: usize) -> Vec<ImageEntry> {
        self.entries.iter().filter(|e| !self.index.has_vector(&e.key) && !self.failed.contains(&e.key)).take(limit).cloned().collect()
    }

    fn indexed(&self) -> usize {
        self.entries.iter().filter(|e| self.index.has_vector(&e.key)).count()
    }
}

pub struct Gallery {
    label: String,
    vault: PathBuf,
    index_path: PathBuf,
    app: AppHandle,
    lib: Mutex<Library>,
    wake: Condvar,
    stop: AtomicBool,
    model: Arc<Model>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

#[derive(Serialize, Clone)]
pub struct Status {
    total: usize,
    indexed: usize,
    failed: usize,
    working: bool,
    scanned: bool,
    auto: bool,
    model: ModelStatus,
}

impl Gallery {
    fn status(&self) -> Status {
        let lib = self.lib.lock().unwrap();
        Status {
            total: lib.entries.len(),
            indexed: lib.indexed(),
            failed: lib.failed.len(),
            working: lib.working,
            scanned: lib.scanned,
            auto: lib.auto,
            model: self.model.status(),
        }
    }

    fn emit_status(&self) {
        let _ = self.app.emit_to(self.label.as_str(), "images-progress", self.status());
    }

    fn emit_changed(&self) {
        let _ = self.app.emit_to(self.label.as_str(), "images-changed", ());
    }

    fn sources_for(&self, external: &[String]) -> Vec<Source> {
        let mut s = vec![Source { root: self.vault.clone(), external: false }];
        for e in external {
            let p = PathBuf::from(e);
            if p.is_dir() && !p.starts_with(&self.vault) {
                s.push(Source { root: p, external: true });
            }
        }
        s
    }

    /// Full scan (in the calling thread). Cheap after the first time: only new
    /// or edited files have their headers read.
    fn rescan(&self) {
        let sources = self.lib.lock().unwrap().sources.clone();
        let found = images::scan(&sources);
        {
            let mut lib = self.lib.lock().unwrap();
            let entries = lib.index.update_from_scan(&found);
            lib.set_entries(entries);
            lib.scanned = true;
            lib.dirty = true;
        }
        self.wake.notify_all();
        self.emit_changed();
        self.emit_status();
    }

    /// Updates the given absolute paths; anything that is not a plain picture
    /// file (a folder rename, say) falls back to a full rescan.
    fn refresh(&self, paths: Vec<PathBuf>) {
        let mut full = false;
        let mut changed = false;
        {
            let mut lib = self.lib.lock().unwrap();
            if !lib.scanned {
                return;
            }
            for p in &paths {
                let Some(source) = lib.sources.iter().filter(|s| p.starts_with(&s.root)).max_by_key(|s| s.root.as_os_str().len()).cloned() else { continue };
                if p.is_dir() {
                    full = true;
                    break;
                }
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if !images::is_image(&name) {
                    // A deleted folder looks like an unknown path.
                    if !p.exists() && name.rsplit_once('.').is_none() {
                        full = true;
                        break;
                    }
                    continue;
                }
                let key = images::key_for(&source, p);
                match images::describe(&source, p) {
                    Some(found) => {
                        let fresh = lib.index.update_from_scan(std::slice::from_ref(&found)).remove(0);
                        lib.failed.remove(&fresh.key);
                        let mut entries = std::mem::take(&mut lib.entries);
                        match lib.by_key.get(&fresh.key) {
                            Some(&i) => entries[i] = fresh,
                            None => entries.push(fresh),
                        }
                        entries.sort_by(|a, b| a.key.cmp(&b.key));
                        lib.set_entries(entries);
                        changed = true;
                    }
                    None => {
                        if lib.by_key.contains_key(&key) {
                            let entries: Vec<ImageEntry> = std::mem::take(&mut lib.entries).into_iter().filter(|e| e.key != key).collect();
                            lib.set_entries(entries);
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                lib.dirty = true;
            }
        }
        if full {
            self.rescan();
        } else if changed {
            self.wake.notify_all();
            self.emit_changed();
            self.emit_status();
        }
    }

    fn save(&self, lib: &mut Library) {
        if lib.dirty {
            if let Err(e) = lib.index.save(&self.index_path) {
                eprintln!("image index: {e}");
            }
            lib.dirty = false;
        }
        lib.last_save = Some(Instant::now());
    }

    fn watch_external(self: &Arc<Self>) {
        let roots: Vec<PathBuf> = self.lib.lock().unwrap().sources.iter().filter(|s| s.external).map(|s| s.root.clone()).collect();
        if roots.is_empty() {
            *self.watcher.lock().unwrap() = None;
            return;
        }
        let me = Arc::downgrade(self);
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if !matches!(ev.kind, notify::EventKind::Access(_)) {
                    for p in ev.paths {
                        let _ = tx.send(p);
                    }
                }
            }
        });
        let Ok(mut watcher) = watcher else { return };
        for r in &roots {
            let _ = watcher.watch(r, notify::RecursiveMode::Recursive);
        }
        *self.watcher.lock().unwrap() = Some(watcher);
        std::thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut batch = HashSet::from([first]);
                while let Ok(p) = rx.recv_timeout(Duration::from_millis(300)) {
                    batch.insert(p);
                }
                let Some(g) = me.upgrade() else { break };
                if g.stop.load(Ordering::Relaxed) {
                    break;
                }
                g.refresh(batch.into_iter().collect());
            }
        });
    }

    /// The indexer: analyses pending pictures a batch at a time while the
    /// model is available and automatic indexing is on.
    fn work(self: Arc<Self>) {
        let mut announced_idle = false;
        loop {
            if self.stop.load(Ordering::Relaxed) {
                let mut lib = self.lib.lock().unwrap();
                self.save(&mut lib);
                return;
            }
            let batch = {
                let mut lib = self.lib.lock().unwrap();
                let ready = lib.scanned && lib.auto && clip::model_ready(&model_dir());
                let batch = if ready { lib.pending(BATCH) } else { Vec::new() };
                if batch.is_empty() {
                    if lib.scanned && ready {
                        // Idle: forget pictures that are gone for good, then rest.
                        let live: HashSet<String> = lib.entries.iter().map(|e| e.key.clone()).collect();
                        let live_ref: HashSet<&str> = live.iter().map(|s| s.as_str()).collect();
                        if lib.index.prune(&live_ref) > 0 {
                            lib.dirty = true;
                        }
                    }
                    self.save(&mut lib);
                    let was_working = lib.working;
                    lib.working = false;
                    drop(lib);
                    if was_working || !announced_idle {
                        announced_idle = true;
                        self.emit_status();
                    }
                    let lib = self.lib.lock().unwrap();
                    let _ = self.wake.wait_timeout(lib, Duration::from_secs(5)).unwrap();
                    continue;
                }
                lib.working = true;
                batch
            };
            announced_idle = false;
            let Some(model) = self.model.get() else {
                let mut lib = self.lib.lock().unwrap();
                lib.working = false;
                drop(lib);
                self.emit_status();
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            self.process(&model, batch);
            self.emit_status();
        }
    }

    fn process(&self, model: &Clip, batch: Vec<ImageEntry>) {
        // Content digests first: a renamed or moved picture gets its old
        // vector back without being analysed again.
        let digests: Vec<Option<[u8; 32]>> = batch.par_iter().map(|e| images::file_digest(Path::new(&e.abs)).ok()).collect();
        let mut todo = Vec::new();
        {
            let mut lib = self.lib.lock().unwrap();
            let live: HashSet<&str> = lib.entries.iter().map(|e| e.key.as_str()).collect();
            let orphans = lib.index.orphans_by_digest(&live);
            for (e, d) in batch.into_iter().zip(digests) {
                let Some(d) = d else {
                    lib.failed.insert(e.key.clone());
                    continue;
                };
                if let Some(old) = orphans.get(&d) {
                    if let Some(old_rec) = lib.index.records.remove(old) {
                        if let Some(rec) = lib.index.records.get_mut(&e.key) {
                            rec.digest = Some(d);
                            rec.vector = old_rec.vector;
                            lib.dirty = true;
                            continue;
                        }
                    }
                }
                todo.push((e, d));
            }
        }
        if todo.is_empty() {
            return;
        }
        let thumbs = thumb_dir();
        let prepared: Vec<Option<Vec<f32>>> = todo
            .par_iter()
            .map(|(e, _)| {
                let img = images::open_oriented(Path::new(&e.abs)).ok()?;
                // The gallery preview, made while the picture is decoded anyway.
                let tp = images::thumbnail_path(&thumbs, Path::new(&e.abs), e.mtime, e.size, THUMB_SIZE);
                if !tp.exists() {
                    let (bytes, _) = images::thumbnail(&img, THUMB_SIZE);
                    if let Some(dir) = tp.parent() {
                        let _ = fs::create_dir_all(dir);
                    }
                    let _ = fs::write(&tp, bytes);
                }
                Some(clip::pixel_values(&img))
            })
            .collect();
        let inputs: Vec<Vec<f32>> = prepared.iter().flatten().cloned().collect();
        let vectors = match model.embed_images(&inputs) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("image search: {err}");
                let mut lib = self.lib.lock().unwrap();
                for (e, _) in &todo {
                    lib.failed.insert(e.key.clone());
                }
                return;
            }
        };
        let mut vectors = vectors.into_iter();
        let mut lib = self.lib.lock().unwrap();
        for ((e, d), p) in todo.into_iter().zip(prepared) {
            if p.is_none() {
                lib.failed.insert(e.key.clone());
                continue;
            }
            let Some(v) = vectors.next() else { break };
            // Skip if the file changed while it was being analysed.
            if let Some(rec) = lib.index.records.get_mut(&e.key) {
                if rec.mtime == e.mtime && rec.size == e.size {
                    rec.digest = Some(d);
                    rec.vector = Some(v);
                    lib.dirty = true;
                }
            }
        }
        if lib.last_save.map_or(true, |t| t.elapsed() > Duration::from_secs(10)) {
            self.save(&mut lib);
        }
    }
}

// ── Tauri state and commands ──────────────────────────────────────────────

#[derive(Default)]
pub struct Galleries {
    map: Mutex<HashMap<String, Arc<Gallery>>>,
    model: Arc<Model>,
}

impl Galleries {
    fn get(&self, label: &str) -> Res<Arc<Gallery>> {
        self.map.lock().unwrap().get(label).cloned().ok_or_else(|| "image library not open".to_string())
    }

    pub fn close(&self, label: &str) {
        if let Some(g) = self.map.lock().unwrap().remove(label) {
            g.stop.store(true, Ordering::Relaxed);
            g.wake.notify_all();
            *g.watcher.lock().unwrap() = None;
        }
    }
}

#[tauri::command]
pub fn images_open(app: AppHandle, window: tauri::Window, state: State<Galleries>, vault: String, config_dir: String, external: Vec<String>, auto: bool) -> Res<Status> {
    state.close(window.label());
    let root = PathBuf::from(&vault);
    let index_path = root.join(&config_dir).join("ateliet").join("images").join("index.bin");
    let gallery = Arc::new(Gallery {
        label: window.label().to_string(),
        vault: root,
        index_path: index_path.clone(),
        app,
        lib: Mutex::new(Library::default()),
        wake: Condvar::new(),
        stop: AtomicBool::new(false),
        model: state.model.clone(),
        watcher: Mutex::new(None),
    });
    {
        let mut lib = gallery.lib.lock().unwrap();
        lib.index = Index::load(&index_path, clip::MODEL_KEY, 512);
        lib.sources = gallery.sources_for(&external);
        lib.auto = auto;
    }
    state.map.lock().unwrap().insert(window.label().to_string(), gallery.clone());
    let g = gallery.clone();
    std::thread::spawn(move || {
        g.rescan();
        g.watch_external();
        g.work();
    });
    Ok(gallery.status())
}

#[tauri::command]
pub fn images_configure(window: tauri::Window, state: State<Galleries>, external: Vec<String>, auto: bool) -> Res<()> {
    let g = state.get(window.label())?;
    let rescan = {
        let mut lib = g.lib.lock().unwrap();
        lib.auto = auto;
        let sources = g.sources_for(&external);
        let changed = sources.iter().map(|s| &s.root).ne(lib.sources.iter().map(|s| &s.root));
        lib.sources = sources;
        changed
    };
    g.wake.notify_all();
    if rescan {
        std::thread::spawn(move || {
            g.rescan();
            g.watch_external();
        });
    } else {
        g.emit_status();
    }
    Ok(())
}

/// Vault-relative paths reported by the vault watcher.
#[tauri::command]
pub fn images_refresh(window: tauri::Window, state: State<Galleries>, paths: Vec<String>) -> Res<()> {
    let g = state.get(window.label())?;
    let abs: Vec<PathBuf> = paths.iter().map(|p| g.vault.join(p)).collect();
    std::thread::spawn(move || g.refresh(abs));
    Ok(())
}

#[tauri::command]
pub fn images_rescan(window: tauri::Window, state: State<Galleries>) -> Res<()> {
    let g = state.get(window.label())?;
    std::thread::spawn(move || g.rescan());
    Ok(())
}

#[tauri::command]
pub fn images_status(window: tauri::Window, state: State<Galleries>) -> Res<Status> {
    Ok(state.get(window.label())?.status())
}

#[tauri::command]
pub fn images_model_download(app: AppHandle, state: State<Galleries>) {
    state.model.download(app);
}

/// Analyse every picture again (keeps nothing but the dimensions).
#[tauri::command]
pub fn images_reindex(window: tauri::Window, state: State<Galleries>) -> Res<()> {
    let g = state.get(window.label())?;
    {
        let mut lib = g.lib.lock().unwrap();
        for r in lib.index.records.values_mut() {
            r.vector = None;
        }
        lib.failed.clear();
        lib.dirty = true;
    }
    g.wake.notify_all();
    g.emit_status();
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    #[serde(default)]
    search: String,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    sort: String,
    #[serde(default)]
    seed: u64,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    count: usize,
    #[serde(default)]
    similar_to: Option<String>,
    #[serde(default)]
    keys: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct Item {
    #[serde(flatten)]
    entry: ImageEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<f32>,
    indexed: bool,
}

#[derive(Serialize)]
pub struct Page {
    total: usize,
    offset: usize,
    seed: u64,
    /// "browse", "semantic", "name" or "similar".
    mode: &'static str,
    items: Vec<Item>,
}

fn in_folder(e: &ImageEntry, folder: &str, recursive: bool) -> bool {
    if e.folder == folder {
        return true;
    }
    if !recursive {
        return false;
    }
    if folder.is_empty() {
        return !e.external;
    }
    e.folder.len() > folder.len() && e.folder.starts_with(folder) && matches!(e.folder.as_bytes()[folder.len()], b'/' | b'\\')
}

#[tauri::command]
pub async fn images_query(window: tauri::Window, state: State<'_, Galleries>, query: Query) -> Res<Page> {
    let g = state.get(window.label())?;
    let model = state.model.clone();
    tauri::async_runtime::spawn_blocking(move || run_query(&g, &model, query)).await.map_err(|e| e.to_string())?
}

fn run_query(g: &Gallery, model: &Model, q: Query) -> Res<Page> {
    let count = if q.count == 0 { 120 } else { q.count.min(1000) };
    let search = q.search.trim().to_string();
    // The text vector is computed before taking the library lock: the model
    // may need loading, and the indexer must not wait on that.
    let text_vector = if !search.is_empty() && q.similar_to.is_none() {
        let has_vectors = g.lib.lock().unwrap().index.records.values().any(|r| r.vector.is_some());
        match (has_vectors, model.get()) {
            (true, Some(clip)) => Some(model.text_vector(&clip, &search)?),
            _ => None,
        }
    } else {
        None
    };
    let lib = g.lib.lock().unwrap();
    let mut candidates: Vec<usize> = match (&q.keys, &q.folder) {
        (Some(keys), _) => keys.iter().filter_map(|k| lib.by_key.get(k).copied()).collect(),
        (None, Some(folder)) => (0..lib.entries.len()).filter(|&i| in_folder(&lib.entries[i], folder, q.recursive)).collect(),
        (None, None) => (0..lib.entries.len()).collect(),
    };
    let words = images::query_words(&search);
    let mut seed = q.seed;
    let (mode, ranked): (&'static str, Vec<(usize, Option<f32>)>) = if let Some(key) = &q.similar_to {
        let Some(v) = lib.index.vector(key).map(|v| v.to_vec()) else {
            return Ok(Page { total: 0, offset: 0, seed, mode: "similar", items: Vec::new() });
        };
        candidates.retain(|&i| lib.entries[i].key != *key);
        ("similar", images::rank(&lib.entries, &lib.index, &candidates, &v, &[]).into_iter().map(|(i, s)| (i, Some(s))).collect())
    } else if let Some(v) = &text_vector {
        let mut ranked: Vec<(usize, Option<f32>)> = images::rank(&lib.entries, &lib.index, &candidates, v, &words).into_iter().map(|(i, s)| (i, Some(s))).collect();
        // Pictures not analysed yet can still be found by name.
        let unindexed: Vec<usize> = candidates.iter().copied().filter(|&i| !lib.index.has_vector(&lib.entries[i].key)).collect();
        ranked.extend(images::rank_by_name(&lib.entries, &unindexed, &words).into_iter().map(|(i, _)| (i, None)));
        ("semantic", ranked)
    } else if !search.is_empty() {
        ("name", images::rank_by_name(&lib.entries, &candidates, &words).into_iter().map(|(i, s)| (i, Some(s))).collect())
    } else {
        match q.sort.as_str() {
            "recent" => candidates.sort_by(|&a, &b| lib.entries[b].mtime.cmp(&lib.entries[a].mtime)),
            "oldest" => candidates.sort_by(|&a, &b| lib.entries[a].mtime.cmp(&lib.entries[b].mtime)),
            "name" => candidates.sort_by(|&a, &b| lib.entries[a].name.to_lowercase().cmp(&lib.entries[b].name.to_lowercase())),
            "keys" => {}
            _ => {
                if seed == 0 {
                    seed = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1) as u64) | 1;
                }
                images::shuffle(&mut candidates, seed);
            }
        }
        ("browse", candidates.into_iter().map(|i| (i, None)).collect())
    };
    let total = ranked.len();
    let offset = q.offset.min(total);
    let items = ranked[offset..(offset + count).min(total)]
        .iter()
        .map(|&(i, score)| Item { entry: lib.entries[i].clone(), score, indexed: lib.index.has_vector(&lib.entries[i].key) })
        .collect();
    Ok(Page { total, offset, seed, mode, items })
}

#[derive(Serialize)]
pub struct FolderInfo {
    folder: String,
    count: usize,
    cover: String,
    external: bool,
    latest: i64,
}

/// Folders that directly contain pictures, with a cover (the newest picture).
#[tauri::command]
pub fn images_folders(window: tauri::Window, state: State<Galleries>) -> Res<Vec<FolderInfo>> {
    let g = state.get(window.label())?;
    let lib = g.lib.lock().unwrap();
    let mut map: HashMap<&str, FolderInfo> = HashMap::new();
    for e in &lib.entries {
        let f = map.entry(e.folder.as_str()).or_insert_with(|| FolderInfo { folder: e.folder.clone(), count: 0, cover: e.abs.clone(), external: e.external, latest: i64::MIN });
        f.count += 1;
        if e.mtime > f.latest {
            f.latest = e.mtime;
            f.cover = e.abs.clone();
        }
    }
    let mut out: Vec<FolderInfo> = map.into_values().collect();
    out.sort_by(|a, b| a.folder.to_lowercase().cmp(&b.folder.to_lowercase()));
    Ok(out)
}

/// Serves a cached preview: `/__thumb?path=<absolute>&w=<size>`.
pub fn serve_thumbnail(path: &str, width: u32) -> Option<(Vec<u8>, &'static str)> {
    let abs = Path::new(path);
    let meta = fs::metadata(abs).ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
    let max = match width {
        0..=260 => 240,
        261..=520 => THUMB_SIZE,
        _ => 960,
    };
    let tp = images::thumbnail_path(&thumb_dir(), abs, mtime, meta.len(), max);
    let sniff = |b: &[u8]| if b.starts_with(b"\x89PNG") { "image/png" } else { "image/jpeg" };
    if let Ok(bytes) = fs::read(&tp) {
        let t = sniff(&bytes);
        return Some((bytes, t));
    }
    let img = images::open_oriented(abs).ok()?;
    let (bytes, t) = images::thumbnail(&img, max);
    if let Some(dir) = tp.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&tp, &bytes);
    Some((bytes, t))
}
