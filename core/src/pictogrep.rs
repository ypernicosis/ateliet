//! The shared picture library: an installed Pictogrep's data folder, read and
//! written exactly the way Pictogrep does, so both apps see the same pictures,
//! the same collections, the same followed Pinterest boards and the same
//! plugin data.
//!
//! Layout of the folder (Pictogrep 0.11):
//!
//! ```text
//! library/                         imported pictures
//! collections/<name>/              one symlink per picture (or images.json on Windows)
//! data/library-state.json          {"sources": [...], "images": [...], "updatedAt": n}
//! data/embeddings-v1.bin           CLIP vectors (PGE1/PGE2 records)
//! data/web-sources.json           followed boards and pages
//! data/plugins/<id>.json           plugin storage
//! plugins/<id>/plugin.json         installed plugins
//! ```
//!
//! While Pictogrep runs it owns `library-state.json`; Ateliet then only adds
//! files and asks it to rescan. When it is closed, Ateliet updates the state
//! file itself.

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 8765;
pub const MAX_IMAGE_BYTES: u64 = 100 << 20;
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif"];

// ── paths ─────────────────────────────────────────────────────────────────

/// Lexical clean of an absolute path, like Go's filepath.Clean: Pictogrep keys
/// pictures by this spelling, so ids and links must use it too.
pub fn clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Pictogrep's image id: SHA-256 of the cleaned absolute path, in hex.
pub fn image_id(path: &Path) -> String {
    hex(&Sha256::digest(clean(path).to_string_lossy().as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn is_image_name(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Home {
    pub root: PathBuf,
}

impl Home {
    pub fn new(root: PathBuf) -> Self {
        Home { root: clean(&root) }
    }
    pub fn library(&self) -> PathBuf {
        self.root.join("library")
    }
    pub fn collections(&self) -> PathBuf {
        self.root.join("collections")
    }
    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn plugins(&self) -> PathBuf {
        self.root.join("plugins")
    }
    pub fn state_path(&self) -> PathBuf {
        self.data().join("library-state.json")
    }
    /// Where Pictogrep keeps config.json (the Pinterest auto-sync switch).
    pub fn config_path(&self) -> PathBuf {
        if let Ok(p) = std::env::var("PICTOGREP_CONFIG") {
            return PathBuf::from(p);
        }
        if std::env::var("PICTOGREP_HOME").is_ok() {
            return self.root.join("config.json");
        }
        dirs::config_dir().unwrap_or_else(|| self.root.clone()).join("pictogrep").join("config.json")
    }
    fn looks_valid(&self) -> bool {
        self.state_path().is_file() || (self.library().is_dir() && self.collections().is_dir())
    }
}

/// Where Ateliet notes the folder a running Pictogrep reported, so the
/// library is still found after Pictogrep closes.
fn remembered_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("ateliet").join("pictogrep-home.txt"))
}

fn remembered() -> Option<PathBuf> {
    let text = fs::read_to_string(remembered_path()?).ok()?;
    let p = text.trim();
    (!p.is_empty()).then(|| PathBuf::from(p))
}

fn remember(home: &Home) {
    let Some(path) = remembered_path() else { return };
    if remembered().as_deref() != Some(home.root.as_path()) {
        let _ = write_atomic(&path, format!("{}\n", home.root.display()).as_bytes());
    }
}

/// The folders Pictogrep could be using on this machine, most likely first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("PICTOGREP_HOME") {
        out.push(PathBuf::from(p));
    }
    // The folder a running Pictogrep reported last time (it may be anywhere).
    out.extend(remembered());
    if let Ok(p) = std::env::var("XDG_DATA_HOME") {
        out.push(PathBuf::from(p).join("pictogrep"));
    }
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".local").join("share").join("pictogrep"));
        out.push(home.join("Library").join("Application Support").join("Pictogrep"));
        out.push(home.join("Library").join("Application Support").join("pictogrep"));
    }
    if let Ok(p) = std::env::var("LOCALAPPDATA") {
        out.push(PathBuf::from(p).join("Pictogrep"));
    }
    out
}

/// Finds the shared library: a running Pictogrep says where its folder is;
/// otherwise the first known location that holds a Pictogrep library.
pub fn detect() -> Option<Home> {
    if let Some(home) = running_instance().map(|(_, h)| h) {
        remember(&home);
        return Some(home);
    }
    candidates().into_iter().map(Home::new).find(|h| h.looks_valid())
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(timeout).build()
}

/// A Pictogrep answering on its default port, with the folder it serves.
pub fn running_instance() -> Option<(u16, Home)> {
    let url = format!("http://127.0.0.1:{DEFAULT_PORT}/api/app/state");
    let v: Value = serde_json::from_str(&agent(Duration::from_millis(600)).get(&url).call().ok()?.into_string().ok()?).ok()?;
    let home = v["paths"]["home"].as_str()?;
    Some((DEFAULT_PORT, Home::new(PathBuf::from(home))))
}

/// Whether Pictogrep currently holds its library lock (it is running).
#[cfg(unix)]
pub fn is_running(home: &Home) -> bool {
    use std::os::unix::io::AsRawFd;
    let canonical = fs::canonicalize(&home.root).unwrap_or_else(|_| home.root.clone());
    let key = hex(&Sha256::digest(canonical.to_string_lossy().as_bytes())[..16]);
    let Some(cache) = dirs::cache_dir() else { return false };
    let Ok(file) = fs::File::open(cache.join("pictogrep").join("locks").join(format!("{key}.lock"))) else { return false };
    // SAFETY: plain flock on a file descriptor we own for the duration.
    let free = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0;
    if free {
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    }
    !free
}

#[cfg(not(unix))]
pub fn is_running(home: &Home) -> bool {
    running_instance().is_some_and(|(_, h)| h == *home)
}

// ── library state ─────────────────────────────────────────────────────────

pub fn read_state(home: &Home) -> Value {
    fs::read_to_string(home.state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
}

/// The folders the shared library reads: `library/` plus every folder
/// Pictogrep indexes in place.
pub fn source_roots(home: &Home) -> Vec<PathBuf> {
    let mut roots = vec![home.library()];
    if let Some(list) = read_state(home)["sources"].as_array() {
        for s in list.iter().filter_map(Value::as_str) {
            let p = clean(Path::new(s));
            if p.is_dir() && !roots.iter().any(|r| p.starts_with(r)) {
                roots.push(p);
            }
        }
    }
    roots
}

pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

/// Tells the library that pictures were added: a running Pictogrep is asked
/// to rescan; a closed one finds them in its state file on the next start.
pub fn library_changed(home: &Home, added: &[PathBuf]) {
    if added.is_empty() {
        return;
    }
    if is_running(home) {
        if let Some((port, h)) = running_instance() {
            if h == *home {
                let _ = agent(Duration::from_secs(5)).post(&format!("http://127.0.0.1:{port}/api/app/index")).set("Content-Type", "application/json").send_string(r#"{"mode":"incremental"}"#);
            }
        }
        // Otherwise Pictogrep picks them up in its own periodic rescan.
        return;
    }
    let mut state = read_state(home);
    if !state.is_object() {
        state = json!({});
    }
    let mut images: Vec<String> = state["images"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let known: HashSet<String> = images.iter().cloned().collect();
    for p in added {
        let s = clean(p).to_string_lossy().into_owned();
        if !known.contains(&s) {
            images.push(s);
        }
    }
    images.sort();
    state["images"] = json!(images);
    if !state["sources"].is_array() {
        state["sources"] = json!([]);
    }
    state["updatedAt"] = json!(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    let _ = write_atomic(&home.state_path(), (serde_json::to_string_pretty(&state).unwrap_or_default() + "\n").as_bytes());
}

// ── collections ───────────────────────────────────────────────────────────

/// Pictogrep's folder-name rules: lowercase, `/` nests, letters, digits, `-`
/// and `_` survive, spaces become `-`, at most 80 characters.
pub fn collection_name(value: &str) -> Result<String, String> {
    // Accents are folded first ("Mãos" → "maos") rather than dropped; the
    // result is still a name Pictogrep accepts as its own.
    use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
    let value: String = value.trim().to_lowercase().replace('\\', "/").nfd().filter(|c| !is_combining_mark(*c)).collect();
    let mut parts = Vec::new();
    for part in value.split('/') {
        let mut b = String::new();
        for c in part.chars() {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
                b.push(c);
            } else if c == ' ' {
                b.push('-');
            }
        }
        let clean = b.trim_matches(|c| c == '-' || c == '_').to_string();
        if clean.is_empty() {
            return Err("a collection name must contain letters or numbers".into());
        }
        parts.push(clean);
    }
    let name = parts.join("/");
    if name.len() > 80 {
        return Err("a collection name must be at most 80 characters".into());
    }
    Ok(name)
}

pub fn collection_names(home: &Home) -> Vec<String> {
    let root = home.collections();
    let mut names = Vec::new();
    for e in walkdir::WalkDir::new(&root).min_depth(1).into_iter().flatten() {
        if !e.file_type().is_dir() {
            continue;
        }
        let rel = e.path().strip_prefix(&root).unwrap_or(e.path()).components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        if collection_name(&rel).ok().as_deref() == Some(rel.as_str()) {
            names.push(rel);
        }
    }
    names.sort();
    names
}

/// The pictures of one collection: its symlinks (read, not resolved, which is
/// the spelling the library uses) plus any legacy images.json.
pub fn collection_images(home: &Home, name: &str) -> Vec<PathBuf> {
    let dir = home.collections().join(name);
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(text) = fs::read_to_string(dir.join("images.json")) {
        if let Ok(list) = serde_json::from_str::<Vec<String>>(&text) {
            out.extend(list.into_iter().map(|s| clean(Path::new(&s))));
        }
    }
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "images.json" {
                continue;
            }
            let path = e.path();
            let target = fs::read_link(&path).map(|t| if t.is_absolute() { t } else { dir.join(t) }).or_else(|_| fs::canonicalize(&path));
            if let Ok(t) = target {
                if is_image_name(&t.to_string_lossy()) {
                    out.push(clean(&t));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    out.retain(|p| p.exists() && seen.insert(p.clone()));
    out
}

pub fn create_collection(home: &Home, name: &str) -> Result<String, String> {
    let name = collection_name(name)?;
    fs::create_dir_all(home.collections().join(&name)).map_err(|e| e.to_string())?;
    Ok(name)
}

fn unique_entry(dir: &Path, base: &str) -> PathBuf {
    let first = dir.join(base);
    if fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (base.to_string(), String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem}-{n}{ext}"))).find(|p| fs::symlink_metadata(p).is_err()).unwrap()
}

/// Adds a picture to a collection (creating it), as Pictogrep's linkTag does.
pub fn link(home: &Home, name: &str, picture: &Path) -> Result<bool, String> {
    let name = collection_name(name)?;
    let dir = home.collections().join(&name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let picture = clean(picture);
    if collection_images(home, &name).contains(&picture) {
        return Ok(false);
    }
    let base = picture.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image.jpg".into());
    let entry = unique_entry(&dir, &base);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&picture, &entry);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&picture, &entry);
    if linked.is_err() {
        // Windows without the right to make links: the manifest format.
        let mut list = collection_images(home, &name);
        list.push(picture);
        let strings: Vec<String> = list.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        write_atomic(&dir.join("images.json"), (serde_json::to_string_pretty(&strings).unwrap() + "\n").as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(true)
}

/// Removes a picture from a collection (the picture itself stays).
pub fn unlink(home: &Home, name: &str, picture: &Path) -> bool {
    let dir = home.collections().join(name);
    let picture = clean(picture);
    let mut removed = false;
    if let Ok(text) = fs::read_to_string(dir.join("images.json")) {
        if let Ok(list) = serde_json::from_str::<Vec<String>>(&text) {
            let kept: Vec<String> = list.iter().filter(|s| clean(Path::new(s)) != picture).cloned().collect();
            if kept.len() != list.len() {
                removed = true;
                let _ = write_atomic(&dir.join("images.json"), (serde_json::to_string_pretty(&kept).unwrap() + "\n").as_bytes());
            }
        }
    }
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            if let Ok(t) = fs::read_link(e.path()) {
                if clean(&t) == picture {
                    let _ = fs::remove_file(e.path());
                    removed = true;
                }
            }
        }
    }
    removed
}

pub fn rename_collection(home: &Home, from: &str, to: &str) -> Result<String, String> {
    let to = collection_name(to)?;
    let (a, b) = (home.collections().join(from), home.collections().join(&to));
    if b.exists() {
        return Err(format!("a collection named {to} already exists"));
    }
    if b.starts_with(&a) {
        return Err("a collection cannot be moved inside itself".into());
    }
    if let Some(p) = b.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    fs::rename(a, b).map_err(|e| e.to_string())?;
    Ok(to)
}

/// Deletes a collection's links (never the pictures). Refuses when it has
/// sub-collections, like Pictogrep.
pub fn delete_collection(home: &Home, name: &str) -> Result<(), String> {
    if collection_names(home).iter().any(|n| n.starts_with(&format!("{name}/"))) {
        return Err(format!("move the sub-collections of {name} out first"));
    }
    fs::remove_dir_all(home.collections().join(name)).map_err(|e| e.to_string())
}

// ── importing ─────────────────────────────────────────────────────────────

/// Pictogrep's file-name rules for imported pictures.
pub fn safe_image_name(value: &str) -> Option<String> {
    let name = Path::new(value).file_name()?.to_string_lossy().into_owned();
    let (stem, ext) = name.rsplit_once('.')?;
    let ext = ext.to_lowercase();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return None;
    }
    let mapped: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || "._- ".contains(c) { c } else { '-' }).collect();
    let mut stem = mapped.split_whitespace().collect::<Vec<_>>().join("-").trim_matches(|c| c == '.' || c == '-').to_string();
    if stem.is_empty() {
        stem = "image".into();
    }
    stem.truncate(100);
    Some(format!("{stem}.{ext}"))
}

pub fn unique_file(dir: &Path, name: &str) -> PathBuf {
    unique_entry(dir, name)
}

/// "Is this picture already in the library", answered by content.
pub struct ImportBatch {
    pub digests: HashMap<[u8; 32], PathBuf>,
    pub added: Vec<PathBuf>,
}

impl ImportBatch {
    /// `known` supplies digests already computed elsewhere (the image index);
    /// the rest are read from disk.
    pub fn new(paths: &[PathBuf], known: &HashMap<PathBuf, [u8; 32]>) -> Self {
        use rayon::prelude::*;
        let pairs: Vec<([u8; 32], PathBuf)> = paths
            .par_iter()
            .filter_map(|p| known.get(p).copied().or_else(|| crate::images::file_digest(p).ok()).map(|d| (d, p.clone())))
            .collect();
        let mut digests = HashMap::new();
        for (d, p) in pairs {
            digests.entry(d).or_insert(p);
        }
        ImportBatch { digests, added: Vec::new() }
    }
}

pub enum Imported {
    New(PathBuf),
    Duplicate(PathBuf),
}

/// Copies one picture into `dir` under a safe, unique name. Exact duplicates
/// of a picture already in the batch are not kept.
pub fn import_file(source: &Path, display_name: &str, dir: &Path, batch: &mut ImportBatch) -> Result<Imported, String> {
    let name = safe_image_name(display_name).ok_or("unsupported picture type")?;
    let meta = fs::metadata(source).map_err(|e| e.to_string())?;
    if meta.len() == 0 || meta.len() > MAX_IMAGE_BYTES {
        return Err("pictures must be between 1 byte and 100 MB".into());
    }
    if crate::images::dimensions(source).is_none() {
        return Err("not a valid picture".into());
    }
    let digest = crate::images::file_digest(source).map_err(|e| e.to_string())?;
    if let Some(existing) = batch.digests.get(&digest) {
        if existing.exists() {
            return Ok(Imported::Duplicate(existing.clone()));
        }
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let target = clean(&unique_file(dir, &name));
    fs::copy(source, &target).map_err(|e| e.to_string())?;
    batch.digests.insert(digest, target.clone());
    batch.added.push(target.clone());
    Ok(Imported::New(target))
}

// ── vectors already computed by Pictogrep ─────────────────────────────────

pub struct SharedVector {
    pub mtime: i64,
    pub digest: Option<[u8; 32]>,
    pub vector: Vec<f32>,
}

/// Reads Pictogrep's CLIP store (same model as Ateliet's), so pictures it
/// already analysed are not analysed again. Damaged records are skipped.
pub fn read_vectors(home: &Home, dims: usize) -> HashMap<PathBuf, SharedVector> {
    let mut out = HashMap::new();
    let Ok(data) = fs::read(home.data().join("embeddings-v1.bin")) else { return out };
    let mut pos = 0;
    while pos + 16 <= data.len() {
        let magic = &data[pos..pos + 4];
        let header = match magic {
            b"PGE1" => 16,
            b"PGE2" => 48,
            _ => {
                pos += 1;
                continue;
            }
        };
        let path_len = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let total = header + path_len + dims * 4 + 4;
        if path_len == 0 || path_len > 1 << 20 || pos + total > data.len() {
            pos += 1;
            continue;
        }
        let rec = &data[pos..pos + total];
        let crc = u32::from_le_bytes(rec[total - 4..].try_into().unwrap());
        if crc32(&rec[..total - 4]) != crc {
            pos += 1;
            continue;
        }
        let mtime = u64::from_le_bytes(rec[8..16].try_into().unwrap()) as i64;
        let digest = (header == 48).then(|| <[u8; 32]>::try_from(&rec[16..48]).unwrap()).filter(|d| d.iter().any(|b| *b != 0));
        let path = clean(Path::new(&String::from_utf8_lossy(&rec[header..header + path_len]).into_owned()));
        let vector = rec[header + path_len..total - 4].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        out.insert(path, SharedVector { mtime, digest, vector });
        pos += total;
    }
    out
}

/// Writes vectors in Pictogrep's PGE2 record format.
fn write_vectors(path: &Path, vectors: &HashMap<PathBuf, SharedVector>) -> std::io::Result<()> {
    let mut out = Vec::new();
    let mut keys: Vec<&PathBuf> = vectors.keys().collect();
    keys.sort();
    for p in keys {
        let v = &vectors[p];
        let name = p.to_string_lossy();
        let start = out.len();
        out.extend_from_slice(b"PGE2");
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(&(v.mtime as u64).to_le_bytes());
        out.extend_from_slice(&v.digest.unwrap_or([0; 32]));
        out.extend_from_slice(name.as_bytes());
        for f in &v.vector {
            out.extend_from_slice(&f.to_le_bytes());
        }
        let c = crc32(&out[start..]);
        out.extend_from_slice(&c.to_le_bytes());
    }
    write_atomic(path, &out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

// ── a copy of the whole library ───────────────────────────────────────────

/// How far a copy is.
#[derive(Serialize, Clone, Debug, Default)]
pub struct CopyProgress {
    pub files: u64,
    #[serde(rename = "totalFiles")]
    pub total_files: u64,
    pub bytes: u64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: u64,
}

/// What a library holds, before copying it.
#[derive(Serialize, Clone, Debug, Default)]
pub struct CopyPlan {
    pub files: u64,
    pub bytes: u64,
    pub pictures: u64,
    pub collections: u64,
    pub plugins: u64,
    pub embeddings: bool,
    pub storyboards: u64,
}

enum Entry {
    Dir,
    File(u64),
    Link(PathBuf),
}

/// Every entry under `root` (relative paths, parents before children).
/// Symbolic links are listed, never followed.
fn walk(root: &Path) -> std::io::Result<Vec<(PathBuf, Entry)>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let mut names: Vec<_> = fs::read_dir(root.join(&rel))?.filter_map(Result::ok).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let name = e.file_name();
            let lossy = name.to_string_lossy();
            // Half-written files from an atomic write in progress, and Finder litter.
            if lossy.ends_with(".tmp") || lossy == ".DS_Store" {
                continue;
            }
            let r = rel.join(&name);
            let meta = fs::symlink_metadata(e.path())?;
            if meta.file_type().is_symlink() {
                out.push((r, Entry::Link(fs::read_link(e.path())?)));
            } else if meta.is_dir() {
                out.push((r.clone(), Entry::Dir));
                stack.push(r);
            } else if meta.is_file() {
                out.push((r, Entry::File(meta.len())));
            }
        }
    }
    Ok(out)
}

pub fn copy_plan(from: &Home) -> Result<CopyPlan, String> {
    if !from.looks_valid() {
        return Err(format!("{} is not a Pictogrep library", from.root.display()));
    }
    let entries = walk(&from.root).map_err(|e| e.to_string())?;
    let mut plan = CopyPlan { embeddings: from.data().join("embeddings-v1.bin").is_file(), ..Default::default() };
    for (rel, e) in &entries {
        let top = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        match e {
            Entry::File(n) => {
                plan.files += 1;
                plan.bytes += n;
                let name = rel.to_string_lossy();
                if top == "library" && is_image_name(&name) {
                    plan.pictures += 1;
                }
                if top == "storyboards" && name.ends_with(".png") && rel.components().count() == 2 {
                    plan.storyboards += 1;
                }
            }
            Entry::Dir if rel.components().count() == 2 && top == "plugins" => plan.plugins += 1,
            _ => {}
        }
    }
    plan.collections = collection_names(from).len() as u64;
    Ok(plan)
}

/// The spellings of a folder that can appear inside files that point into it.
fn root_spellings(root: &Path) -> Vec<String> {
    let mut out = vec![root.to_string_lossy().into_owned()];
    if let Ok(c) = fs::canonicalize(root) {
        let c = c.to_string_lossy().into_owned();
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Replaces `from` with `to` where it is a whole path prefix (followed by a
/// separator, a quote or the end), so `/a/pictogrep-old` is left alone.
fn replace_prefix(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(from) {
        let after = &rest[i + from.len()..];
        let whole = after.is_empty() || after.starts_with(['/', '\\', '"']);
        out.push_str(&rest[..i]);
        out.push_str(if whole { to } else { from });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// JSON files point at pictures by absolute path: those paths move with the copy.
fn rewrite_json(text: &str, from: &[String], to: &Path) -> String {
    let to_raw = to.to_string_lossy().into_owned();
    let esc = |s: &str| serde_json::to_string(s).map(|q| q[1..q.len() - 1].to_string()).unwrap_or_else(|_| s.to_string());
    let mut out = text.to_string();
    for f in from {
        out = replace_prefix(&out, &esc(f), &esc(&to_raw));
        if esc(f) != *f {
            out = replace_prefix(&out, f, &to_raw);
        }
    }
    out
}

fn moved(p: &Path, from: &[String], to: &Path) -> Option<PathBuf> {
    from.iter().find_map(|f| p.strip_prefix(f).ok()).map(|rel| to.join(rel))
}

#[cfg(unix)]
fn make_link(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(windows)]
fn make_link(target: &Path, at: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, at)
    } else {
        std::os::windows::fs::symlink_file(target, at)
    }
}

fn keep_mtime(src: &Path, dst: &Path) {
    if let Ok(t) = fs::metadata(src).and_then(|m| m.modified()) {
        if let Ok(f) = fs::File::options().write(true).open(dst) {
            let _ = f.set_modified(t);
        }
    }
}

/// Copies a whole Pictogrep library into `to` (which must be new or empty),
/// so it can be used without Pictogrep: pictures, collections, the library
/// state, analysed pictures, followed boards, plugins and their data,
/// storyboards. Paths that pointed into the old folder (collection links,
/// JSON state, the vectors) point into the copy. Files keep their dates, so
/// nothing is analysed again. The copy is made next to `to` and renamed into
/// place at the end; a failed or cancelled copy leaves nothing behind.
/// On APFS, Btrfs and XFS the copies share blocks until changed.
pub fn copy_home(from: &Home, to: &Path, cancel: &std::sync::atomic::AtomicBool, mut progress: impl FnMut(&CopyProgress)) -> Result<Home, String> {
    use std::sync::atomic::Ordering;
    if !from.looks_valid() {
        return Err(format!("{} is not a Pictogrep library", from.root.display()));
    }
    let to = clean(to);
    let src_canon = fs::canonicalize(&from.root).unwrap_or_else(|_| from.root.clone());
    let to_canon = to.parent().and_then(|p| fs::canonicalize(p).ok()).map(|p| p.join(to.file_name().unwrap_or_default())).unwrap_or_else(|| to.clone());
    if to_canon.starts_with(&src_canon) || src_canon.starts_with(&to_canon) {
        return Err("The copy has to go outside Pictogrep's folder (and not around it).".into());
    }
    if to.exists() && fs::read_dir(&to).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Err(format!("{} is not empty. Choose a new or empty folder.", to.display()));
    }
    let entries = walk(&from.root).map_err(|e| format!("Could not read {}: {e}", from.root.display()))?;
    let mut p = CopyProgress::default();
    for (_, e) in &entries {
        if let Entry::File(n) = e {
            p.total_files += 1;
            p.total_bytes += n;
        }
    }
    let part = PathBuf::from(format!("{}.copying", to.display()));
    let _ = fs::remove_dir_all(&part);
    fs::create_dir_all(&part).map_err(|e| format!("Could not create {}: {e}", part.display()))?;
    let from_roots = root_spellings(&from.root);
    let vectors_rel = Path::new("data").join("embeddings-v1.bin");
    let result = (|| -> Result<(), String> {
        let mut last = std::time::Instant::now();
        for (rel, e) in &entries {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let src = from.root.join(rel);
            let dst = part.join(rel);
            match e {
                Entry::Dir => fs::create_dir_all(&dst).map_err(|e| e.to_string())?,
                Entry::Link(target) => {
                    // Links into the old folder point at the same file in the copy (its final place).
                    let target = if target.is_absolute() { moved(&clean(target), &from_roots, &to).unwrap_or_else(|| target.clone()) } else { target.clone() };
                    make_link(&target, &dst).map_err(|e| format!("{}: {e}", rel.display()))?;
                }
                Entry::File(n) => {
                    let name = rel.to_string_lossy();
                    if name.ends_with(".json") {
                        let text = fs::read_to_string(&src).map_err(|e| format!("{}: {e}", rel.display()))?;
                        fs::write(&dst, rewrite_json(&text, &from_roots, &to)).map_err(|e| format!("{}: {e}", rel.display()))?;
                    } else if *rel == vectors_rel {
                        let mut done = false;
                        for dims in [512, 768, 1024] {
                            let found = read_vectors(from, dims);
                            if !found.is_empty() {
                                let moved_vecs: HashMap<PathBuf, SharedVector> = found.into_iter().map(|(k, v)| (moved(&k, &from_roots, &to).unwrap_or(k), v)).collect();
                                write_vectors(&dst, &moved_vecs).map_err(|e| format!("{}: {e}", rel.display()))?;
                                done = true;
                                break;
                            }
                        }
                        if !done {
                            fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", rel.display()))?;
                        }
                    } else {
                        fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", rel.display()))?;
                    }
                    keep_mtime(&src, &dst);
                    p.files += 1;
                    p.bytes += n;
                    if last.elapsed() > Duration::from_millis(150) {
                        progress(&p);
                        last = std::time::Instant::now();
                    }
                }
            }
        }
        // Where this copy came from (Pictogrep ignores the file).
        let note = json!({ "copiedFrom": from.root, "copiedAt": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) });
        write_atomic(&part.join("data").join("ateliet-copy.json"), (serde_json::to_string_pretty(&note).unwrap_or_default() + "\n").as_bytes()).map_err(|e| e.to_string())?;
        if to.exists() {
            fs::remove_dir(&to).map_err(|e| e.to_string())?;
        }
        fs::rename(&part, &to).map_err(|e| format!("Could not move the copy into {}: {e}", to.display()))?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&part);
        return Err(e);
    }
    progress(&p);
    Ok(Home::new(to))
}

/// Where a library was copied from, if it is a copy made by Ateliet.
pub fn copied_from(home: &Home) -> Option<String> {
    let v: Value = serde_json::from_str(&fs::read_to_string(home.data().join("ateliet-copy.json")).ok()?).ok()?;
    v["copiedFrom"].as_str().map(String::from)
}

/// Where Ateliet keeps a library of its own by default.
pub fn own_library_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ateliet").join("library")
}

// ── plugins ───────────────────────────────────────────────────────────────

/// The capabilities a Pictogrep plugin can declare (API version 0).
pub const CAPABILITIES: &[&str] = &["images.list", "images.search", "images.read", "images.tag", "images.reveal", "storage.kv", "ui.panel", "ui.openExternal"];

#[derive(Serialize, Clone, Debug)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    pub permissions: Vec<String>,
    pub paid: bool,
    pub panel: Value,
    #[serde(skip)]
    pub dir: PathBuf,
}

/// Installed plugins, validated the way Pictogrep validates them: a broken
/// or half-copied plugin is skipped, never fatal.
pub fn plugins(home: &Home) -> Vec<Plugin> {
    let mut found: HashMap<String, Plugin> = HashMap::new();
    let Ok(rd) = fs::read_dir(home.plugins()) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let Ok(text) = fs::read_to_string(dir.join("plugin.json")) else { continue };
        let Ok(m) = serde_json::from_str::<Value>(&text) else { continue };
        let s = |k: &str| m[k].as_str().unwrap_or("").to_string();
        let (id, entry, version) = (s("id"), s("entry"), s("version"));
        if id.is_empty() || entry.is_empty() || version.is_empty() || version == "0.0.0" {
            continue;
        }
        let Some(entry_path) = plugin_file(&dir, &entry) else { continue };
        if !entry_path.is_file() {
            continue;
        }
        let permissions = m["permissions"].as_array().map(|a| a.iter().filter_map(Value::as_str).filter(|p| CAPABILITIES.contains(p)).map(String::from).collect()).unwrap_or_default();
        let name = Some(s("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
        found.insert(id.clone(), Plugin { id, name, version, entry, permissions, paid: m["paid"].as_bool().unwrap_or(false), panel: m["panel"].clone(), dir });
    }
    let mut list: Vec<Plugin> = found.into_values().collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list
}

/// A file inside a plugin's directory, refusing anything that escapes it.
pub fn plugin_file(dir: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = dir.to_path_buf();
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p => out.push(p),
        }
    }
    (out != dir).then_some(out)
}

pub fn storage_path(home: &Home, id: &str) -> Option<PathBuf> {
    (!id.is_empty() && !id.contains(['/', '\\']) && id != "." && id != "..").then(|| home.data().join("plugins").join(format!("{id}.json")))
}

pub fn storage_get(home: &Home, id: &str) -> Value {
    storage_path(home, id).and_then(|p| fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str::<Value>(&s).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

pub fn storage_set(home: &Home, id: &str, key: &str, value: Value) -> Result<(), String> {
    let path = storage_path(home, id).ok_or("invalid plugin id")?;
    let mut store = storage_get(home, id);
    store[key] = value;
    let text = serde_json::to_string_pretty(&store).map_err(|e| e.to_string())? + "\n";
    if text.len() > 1 << 20 {
        return Err("plugin storage is limited to 1 MB".into());
    }
    write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> Home {
        let dir = std::env::temp_dir().join(format!("ateliet-pg-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("library")).unwrap();
        fs::create_dir_all(dir.join("collections")).unwrap();
        fs::create_dir_all(dir.join("data")).unwrap();
        Home::new(dir)
    }

    #[test]
    fn names_follow_pictogrep_rules() {
        assert_eq!(collection_name("  Mãos e Pés / Poses ").unwrap(), "maos-e-pes/poses");
        assert!(collection_name("///").is_err());
        assert_eq!(safe_image_name("Foto (1)!.JPG").unwrap(), "Foto--1.jpg");
        assert_eq!(safe_image_name("a b  c.png").unwrap(), "a-b-c.png");
        assert!(safe_image_name("doc.pdf").is_none());
    }

    #[test]
    fn ids_match_pictogrep() {
        // sha256("/a/b.jpg")
        assert_eq!(image_id(Path::new("/a/./x/../b.jpg")), hex(&Sha256::digest(b"/a/b.jpg")));
    }

    #[test]
    fn collections_link_unlink_and_list() {
        let home = temp_home("coll");
        let pic = home.library().join("cat.png");
        image::DynamicImage::new_rgb8(4, 4).save(&pic).unwrap();
        assert!(link(&home, "Gatos/Pretos", &pic).unwrap());
        assert!(!link(&home, "gatos/pretos", &pic).unwrap());
        assert_eq!(collection_names(&home), vec!["gatos", "gatos/pretos"]);
        assert_eq!(collection_images(&home, "gatos/pretos"), vec![clean(&pic)]);
        assert!(delete_collection(&home, "gatos").is_err());
        assert!(unlink(&home, "gatos/pretos", &pic));
        assert!(collection_images(&home, "gatos/pretos").is_empty());
        assert!(pic.exists());
        let _ = fs::remove_dir_all(&home.root);
    }

    #[test]
    fn import_skips_duplicates_and_updates_closed_state() {
        let home = temp_home("import");
        let src = home.root.join("in.png");
        image::DynamicImage::new_rgb8(5, 5).save(&src).unwrap();
        let mut batch = ImportBatch::new(&[], &HashMap::new());
        let Imported::New(first) = import_file(&src, "Meu Desenho.png", &home.library(), &mut batch).unwrap() else { panic!() };
        assert_eq!(first.file_name().unwrap(), "Meu-Desenho.png");
        assert!(matches!(import_file(&src, "again.png", &home.library(), &mut batch).unwrap(), Imported::Duplicate(_)));
        library_changed(&home, &batch.added);
        let state = read_state(&home);
        assert_eq!(state["images"][0].as_str().unwrap(), first.to_string_lossy());
        let _ = fs::remove_dir_all(&home.root);
    }

    #[test]
    fn reads_pictogrep_vector_records() {
        let home = temp_home("vec");
        let path = "/lib/a.jpg";
        let mut rec = Vec::new();
        rec.extend_from_slice(b"PGE2");
        rec.extend_from_slice(&(path.len() as u32).to_le_bytes());
        rec.extend_from_slice(&1234u64.to_le_bytes());
        rec.extend_from_slice(&[9u8; 32]);
        rec.extend_from_slice(path.as_bytes());
        for i in 0..3 {
            rec.extend_from_slice(&(i as f32).to_le_bytes());
        }
        let c = crc32(&rec);
        rec.extend_from_slice(&c.to_le_bytes());
        let mut data = b"garbage".to_vec();
        data.extend_from_slice(&rec);
        fs::write(home.data().join("embeddings-v1.bin"), data).unwrap();
        let v = read_vectors(&home, 3);
        let r = &v[Path::new(path)];
        assert_eq!((r.mtime, r.digest, r.vector.clone()), (1234, Some([9; 32]), vec![0.0, 1.0, 2.0]));
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let _ = fs::remove_dir_all(&home.root);
    }

    #[cfg(unix)]
    #[test]
    fn copies_a_library_with_links_state_and_vectors_moved() {
        use std::sync::atomic::AtomicBool;
        let home = temp_home("copy");
        let pic = home.library().join("cat.png");
        image::DynamicImage::new_rgb8(4, 4).save(&pic).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        fs::File::options().write(true).open(&pic).unwrap().set_modified(old).unwrap();
        link(&home, "gatos", &pic).unwrap();
        library_changed(&home, &[pic.clone()]);
        fs::write(home.data().join("other.json"), format!("{{\"a\":\"{}-old/x\",\"b\":\"{}\"}}", home.root.display(), pic.display())).unwrap();
        let mut vecs = HashMap::new();
        vecs.insert(clean(&pic), SharedVector { mtime: 1_600_000_000, digest: Some([7; 32]), vector: vec![0.5; 512] });
        write_vectors(&home.data().join("embeddings-v1.bin"), &vecs).unwrap();
        fs::write(home.data().join("half.json.tmp"), "x").unwrap();
        let plan = copy_plan(&home).unwrap();
        assert_eq!((plan.pictures, plan.collections, plan.embeddings), (1, 1, true));

        let dest = std::env::temp_dir().join(format!("ateliet-pg-copy-dest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dest);
        assert!(copy_home(&home, &home.root.join("inside"), &AtomicBool::new(false), |_| {}).is_err());
        let copy = copy_home(&home, &dest, &AtomicBool::new(false), |_| {}).unwrap();
        let new_pic = copy.library().join("cat.png");
        assert_eq!(collection_images(&copy, "gatos"), vec![clean(&new_pic)]);
        assert_eq!(read_state(&copy)["images"][0].as_str().unwrap(), new_pic.to_string_lossy());
        let other = fs::read_to_string(copy.data().join("other.json")).unwrap();
        assert!(other.contains(&format!("{}-old/x", home.root.display())) && other.contains(&*new_pic.to_string_lossy()));
        assert_eq!(fs::metadata(&new_pic).unwrap().modified().unwrap(), old);
        let v = read_vectors(&copy, 512);
        assert_eq!(v[&clean(&new_pic)].digest, Some([7; 32]));
        assert!(!copy.data().join("half.json.tmp").exists());
        assert_eq!(copied_from(&copy).as_deref(), Some(&*home.root.to_string_lossy()));
        // The original is untouched, and a second copy into a full folder is refused.
        assert_eq!(collection_images(&home, "gatos"), vec![clean(&pic)]);
        assert!(copy_home(&home, &dest, &AtomicBool::new(false), |_| {}).is_err());
        let _ = fs::remove_dir_all(&home.root);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn prefix_replacement_respects_path_boundaries() {
        assert_eq!(replace_prefix("\"/a/pg/x\" \"/a/pg-old\" \"/a/pg\"", "/a/pg", "/b"), "\"/b/x\" \"/a/pg-old\" \"/b\"");
    }

    #[test]
    fn plugins_are_validated() {
        let home = temp_home("plug");
        let ok = home.plugins().join("ok");
        fs::create_dir_all(ok.join("ui")).unwrap();
        fs::write(ok.join("ui/index.html"), "<p>hi</p>").unwrap();
        fs::write(ok.join("plugin.json"), r#"{"id":"dev.ok","version":"1.0.0","entry":"ui/index.html","permissions":["images.list","fs.write"]}"#).unwrap();
        let bad = home.plugins().join("bad");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("plugin.json"), r#"{"id":"dev.bad","version":"0.0.0","entry":"x.html"}"#).unwrap();
        let escape = home.plugins().join("esc");
        fs::create_dir_all(&escape).unwrap();
        fs::write(escape.join("plugin.json"), r#"{"id":"dev.esc","version":"1.0.0","entry":"../ok/ui/index.html"}"#).unwrap();
        let list = plugins(&home);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].id.as_str(), list[0].permissions.clone()), ("dev.ok", vec!["images.list".to_string()]));
        storage_set(&home, "dev.ok", "k", json!({"a": 1})).unwrap();
        assert_eq!(storage_get(&home, "dev.ok")["k"]["a"], 1);
        let _ = fs::remove_dir_all(&home.root);
    }
}
