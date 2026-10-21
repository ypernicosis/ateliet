//! The open vault: its files, the page index (from the core) and a watcher
//! that keeps both current.

use ateliet_core::dataview::Index;
use ateliet_core::vault as corevault;
use notify::Watcher;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// A folder of the explorer tree.
#[derive(Default, Debug, Clone)]
pub struct Folder {
    pub folders: BTreeMap<String, Folder>,
    /// file name → vault-relative path
    pub files: BTreeMap<String, String>,
}

pub struct Vault {
    pub root: PathBuf,
    pub name: String,
    pub files: Vec<corevault::Entry>,
    pub tree: Folder,
    pub index: Arc<RwLock<Index>>,
    /// Set by the indexing thread when the first build finishes.
    pub indexed: Arc<std::sync::atomic::AtomicBool>,
    changes: Receiver<Vec<String>>,
    _watcher: Option<notify::RecommendedWatcher>,
}

fn build_tree(files: &[corevault::Entry]) -> Folder {
    let mut root = Folder::default();
    for e in files {
        let parts: Vec<&str> = e.path.split('/').collect();
        let mut node = &mut root;
        let last = parts.len() - 1;
        for (i, part) in parts.iter().enumerate() {
            if i == last && !e.folder {
                node.files.insert(part.to_string(), e.path.clone());
            } else {
                node = node.folders.entry(part.to_string()).or_default();
            }
        }
    }
    root
}

impl Vault {
    pub fn open(root: &Path, ctx: eframe::egui::Context) -> Vault {
        let files = corevault::list(root);
        let tree = build_tree(&files);
        let index = Arc::new(RwLock::new(Index::empty(root)));
        let indexed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let (ix, done, r, c) = (index.clone(), indexed.clone(), root.to_path_buf(), ctx.clone());
            std::thread::spawn(move || {
                let started = Instant::now();
                let built = Index::build(&r);
                eprintln!("[ateliet] indexed {} notes in {} ms", built.len(), started.elapsed().as_millis());
                *ix.write().unwrap() = built;
                done.store(true, std::sync::atomic::Ordering::SeqCst);
                c.request_repaint();
            });
        }
        // watcher → batched vault-relative paths; the index is updated off the UI thread
        let (tx, rx) = mpsc::channel::<Vec<String>>();
        let (raw_tx, raw_rx) = mpsc::channel::<PathBuf>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for p in ev.paths {
                    let _ = raw_tx.send(p);
                }
            }
        })
        .ok()
        .and_then(|mut w| w.watch(root, notify::RecursiveMode::Recursive).ok().map(|_| w));
        {
            let (r, ix, c) = (root.to_path_buf(), index.clone(), ctx);
            std::thread::spawn(move || {
                while let Ok(first) = raw_rx.recv() {
                    let mut batch = vec![first];
                    let deadline = Instant::now() + Duration::from_millis(120);
                    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                        match raw_rx.recv_timeout(left) {
                            Ok(p) => batch.push(p),
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(_) => return,
                        }
                    }
                    let mut rels: Vec<String> = batch.iter().map(|p| corevault::rel_path(&r, p)).filter(|r| !r.is_empty() && !r.split('/').any(|s| s.starts_with('.'))).collect();
                    rels.sort();
                    rels.dedup();
                    if rels.is_empty() {
                        continue;
                    }
                    ix.write().unwrap().update(&rels);
                    if tx.send(rels).is_err() {
                        return;
                    }
                    c.request_repaint();
                }
            });
        }
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.to_string_lossy().into_owned());
        Vault { root: root.to_path_buf(), name, files, tree, index, indexed, changes: rx, _watcher: watcher }
    }

    /// Paths changed on disk since the last call (the file list is refreshed when there are any).
    pub fn poll_changes(&mut self) -> Vec<String> {
        let mut all = Vec::new();
        while let Ok(r) = self.changes.try_recv() {
            all.extend(r);
        }
        if !all.is_empty() {
            self.rescan();
        }
        all
    }

    pub fn rescan(&mut self) {
        self.files = corevault::list(&self.root);
        self.tree = build_tree(&self.files);
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn markdown_files(&self) -> impl Iterator<Item = &corevault::Entry> {
        self.files.iter().filter(|e| !e.folder && e.path.to_lowercase().ends_with(".md"))
    }

    /// Resolve a link target to a vault path (Obsidian's rules, from the index).
    pub fn resolve(&self, link: &str, source: &str) -> Option<String> {
        let target = link.split('#').next().unwrap_or(link);
        if target.is_empty() {
            return Some(source.to_string());
        }
        if let Some(p) = self.index.read().ok().and_then(|ix| ix.resolve(target, source)) {
            return Some(p);
        }
        // before the index is ready: by exact path or file name
        let lower = target.to_lowercase();
        self.files
            .iter()
            .filter(|e| !e.folder)
            .find(|e| {
                let p = e.path.to_lowercase();
                p == lower || p == format!("{lower}.md") || p.rsplit('/').next() == Some(&lower) || p.rsplit('/').next() == Some(&format!("{lower}.md"))
            })
            .map(|e| e.path.clone())
    }

    /// A free path `base.ext`, `base 1.ext`, …
    pub fn available_path(&self, base: &str, ext: &str) -> String {
        let mut p = format!("{base}.{ext}");
        let mut n = 1;
        while self.abs(&p).exists() {
            p = format!("{base} {n}.{ext}");
            n += 1;
        }
        p
    }

    /// Write a note, creating folders; atomic (temp file + rename).
    pub fn write(&self, rel: &str, text: &str) -> std::io::Result<()> {
        let abs = self.abs(rel);
        if let Some(d) = abs.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = abs.with_extension("ateliet-tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &abs)
    }
}

/// Last path segment without `.md`.
pub fn basename(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md").unwrap_or(name)
}

pub fn parent(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}
