//! Vault file system helpers. Paths given to the UI are vault-relative and use
//! `/` separators, like Obsidian's `TAbstractFile.path`.

use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub path: String,
    pub folder: bool,
    pub size: u64,
    pub ctime: u64,
    pub mtime: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Stat {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub size: u64,
    pub ctime: u64,
    pub mtime: u64,
}

fn ms(t: std::io::Result<std::time::SystemTime>) -> u64 {
    t.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn stat(path: &Path) -> Option<Stat> {
    let m = fs::metadata(path).ok()?;
    let mtime = ms(m.modified());
    let ctime = match ms(m.created()) {
        0 => mtime,
        c => c,
    };
    Some(Stat { kind: if m.is_dir() { "folder" } else { "file" }, size: if m.is_dir() { 0 } else { m.len() }, ctime, mtime })
}

pub fn rel_path(root: &Path, p: &Path) -> String {
    let r = p.strip_prefix(root).unwrap_or(p);
    r.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// Every file and folder of the vault, skipping dot-entries (the config
/// folder, `.git`, `.trash` …) exactly like Obsidian does.
pub fn list(root: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let walker = WalkDir::new(root).follow_links(true).into_iter().filter_entry(|e| {
        e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')
    });
    for e in walker.flatten() {
        if e.depth() == 0 {
            continue;
        }
        let Some(s) = stat(e.path()) else { continue };
        out.push(Entry { path: rel_path(root, e.path()), folder: s.kind == "folder", size: s.size, ctime: s.ctime, mtime: s.mtime });
    }
    out
}

#[derive(Serialize, Default)]
pub struct DirList {
    pub files: Vec<String>,
    pub folders: Vec<String>,
}

/// Direct children of a directory, as names.
pub fn list_dir(path: &Path) -> std::io::Result<DirList> {
    let mut out = DirList::default();
    for e in fs::read_dir(path)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false)
            || (e.file_type().map(|t| t.is_symlink()).unwrap_or(false) && e.path().is_dir());
        if is_dir {
            out.folders.push(name)
        } else {
            out.files.push(name)
        }
    }
    out.files.sort();
    out.folders.sort();
    Ok(out)
}

pub fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for e in fs::read_dir(from)? {
            let e = e?;
            copy_recursive(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(p) = to.parent() {
            fs::create_dir_all(p)?;
        }
        fs::copy(from, to).map(|_| ())
    }
}
