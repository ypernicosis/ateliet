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

/// The folders Pictogrep could be using on this machine, most likely first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("PICTOGREP_HOME") {
        out.push(PathBuf::from(p));
    }
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

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
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
            .filter_map(|p| known.get(p).copied().or_else(|| ateliet_core::images::file_digest(p).ok()).map(|d| (d, p.clone())))
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
    if ateliet_core::images::dimensions(source).is_none() {
        return Err("not a valid picture".into());
    }
    let digest = ateliet_core::images::file_digest(source).map_err(|e| e.to_string())?;
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
