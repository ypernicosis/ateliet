//! Search pictures by description (CLIP), with the same model, model folder
//! and index file as before: the model is downloaded once, pictures
//! are analysed in the background a few at a time, renamed pictures keep
//! their vectors (matched by content), and the index is saved as it goes.

use ateliet_core::clip::{self, Clip};
use ateliet_core::images::{self, ImageEntry, Index};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn model_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ATELIET_CLIP_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ateliet").join("models").join(clip::MODEL_KEY)
}

#[derive(Default)]
pub struct Vision {
    clip: Mutex<Option<Arc<Clip>>>,
    pub error: Mutex<Option<String>>,
    pub downloading: AtomicBool,
    pub received: AtomicU64,
    pub total: AtomicU64,
    queries: Mutex<HashMap<String, Vec<f32>>>,
    /// Pictures analysed / waiting in the current pass.
    pub done: AtomicUsize,
    pub waiting: AtomicUsize,
    working: AtomicBool,
}

impl Vision {
    pub fn ready(&self) -> bool {
        clip::model_ready(&model_dir())
    }

    fn model(&self) -> Option<Arc<Clip>> {
        let mut slot = self.clip.lock().unwrap();
        if let Some(c) = slot.as_ref() {
            return Some(c.clone());
        }
        if !self.ready() || self.error.lock().unwrap().is_some() {
            return None;
        }
        match Clip::load(&model_dir()) {
            Ok(c) => {
                let c = Arc::new(c);
                *slot = Some(c.clone());
                Some(c)
            }
            Err(e) => {
                *self.error.lock().unwrap() = Some(e);
                None
            }
        }
    }

    /// The vector of a description (cached).
    pub fn text_vector(&self, query: &str) -> Option<Vec<f32>> {
        let key = query.trim().to_lowercase();
        if let Some(v) = self.queries.lock().unwrap().get(&key) {
            return Some(v.clone());
        }
        let v = self.model()?.embed_text(&key).ok()?;
        self.queries.lock().unwrap().insert(key, v.clone());
        Some(v)
    }

    pub fn download(self: &Arc<Self>, ctx: eframe::egui::Context) {
        if self.downloading.swap(true, Ordering::SeqCst) {
            return;
        }
        *self.error.lock().unwrap() = None;
        let me = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = me.fetch(&ctx) {
                *me.error.lock().unwrap() = Some(e);
            }
            me.downloading.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    fn fetch(&self, ctx: &eframe::egui::Context) -> Result<(), String> {
        let dir = model_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let pending: Vec<_> = clip::FILES.iter().filter(|(_, local, _)| !dir.join(local).is_file()).collect();
        self.total.store(pending.iter().map(|f| f.2).sum(), Ordering::Relaxed);
        self.received.store(0, Ordering::Relaxed);
        let agent = ureq::AgentBuilder::new().try_proxy_from_env(true).timeout_connect(Duration::from_secs(30)).build();
        let mut last = Instant::now();
        for (remote, local, approx) in pending {
            let url = format!("https://huggingface.co/{}/resolve/{}/{}", clip::REPO, clip::REVISION, remote);
            let resp = agent.get(&url).call().map_err(|e| format!("{remote}: {e}"))?;
            if let Some(len) = resp.header("content-length").and_then(|v| v.parse::<u64>().ok()) {
                self.total.fetch_add(len, Ordering::Relaxed);
                self.total.fetch_sub(*approx, Ordering::Relaxed);
            }
            let part = dir.join(format!("{local}.part"));
            let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
            let mut reader = resp.into_reader();
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = reader.read(&mut buf).map_err(|e| format!("{remote}: {e}"))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                self.received.fetch_add(n as u64, Ordering::Relaxed);
                if last.elapsed() > Duration::from_millis(250) {
                    last = Instant::now();
                    ctx.request_repaint();
                }
            }
            out.sync_all().map_err(|e| e.to_string())?;
            drop(out);
            std::fs::rename(&part, dir.join(local)).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Analyse the pictures without a vector, in the background.
    pub fn index(self: &Arc<Self>, index: Arc<Mutex<Index>>, entries: Vec<ImageEntry>, index_path: PathBuf, ctx: eframe::egui::Context) {
        if !self.ready() || self.working.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            clip::lower_thread_priority();
            let todo: Vec<ImageEntry> = {
                let ix = index.lock().unwrap();
                entries.into_iter().filter(|e| !ix.has_vector(&e.key)).collect()
            };
            me.waiting.store(todo.len(), Ordering::Relaxed);
            me.done.store(0, Ordering::Relaxed);
            let live: HashSet<String> = index.lock().unwrap().records.keys().cloned().collect();
            let mut last_save = Instant::now();
            for batch in todo.chunks(8) {
                me.process(batch, &index, &live);
                me.done.fetch_add(batch.len(), Ordering::Relaxed);
                ctx.request_repaint();
                if last_save.elapsed() > Duration::from_secs(10) {
                    let _ = index.lock().unwrap().save(&index_path);
                    last_save = Instant::now();
                }
            }
            let _ = index.lock().unwrap().save(&index_path);
            me.working.store(false, Ordering::SeqCst);
            me.waiting.store(0, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }

    fn process(&self, batch: &[ImageEntry], index: &Arc<Mutex<Index>>, live: &HashSet<String>) {
        let mut todo = Vec::new();
        for e in batch {
            let Ok(d) = images::file_digest(Path::new(&e.abs)) else { continue };
            let mut ix = index.lock().unwrap();
            // a renamed or moved picture takes its old vector back
            let live_ref: HashSet<&str> = live.iter().map(|s| s.as_str()).collect();
            let orphans = ix.orphans_by_digest(&live_ref);
            if let Some(old) = orphans.get(&d).cloned() {
                if let Some(old_rec) = ix.records.remove(&old) {
                    if let Some(rec) = ix.records.get_mut(&e.key) {
                        rec.digest = Some(d);
                        rec.vector = old_rec.vector;
                        continue;
                    }
                }
            }
            drop(ix);
            todo.push((e, d));
        }
        let inputs: Vec<(usize, Vec<f32>)> = todo.iter().enumerate().filter_map(|(i, (e, _))| images::open_oriented(Path::new(&e.abs)).ok().map(|img| (i, clip::pixel_values(&img)))).collect();
        if inputs.is_empty() {
            return;
        }
        let Some(model) = self.model() else { return };
        let Ok(vectors) = model.embed_images(&inputs.iter().map(|x| x.1.clone()).collect::<Vec<_>>()) else { return };
        let mut ix = index.lock().unwrap();
        for ((i, _), v) in inputs.iter().zip(vectors) {
            let (e, d) = &todo[*i];
            if let Some(rec) = ix.records.get_mut(&e.key) {
                if rec.mtime == e.mtime && rec.size == e.size {
                    rec.digest = Some(*d);
                    rec.vector = Some(v);
                }
            }
        }
    }
}
