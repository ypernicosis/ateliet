//! The reader's state: where each book was left, when it was last seen,
//! and the reading preferences, in `<config>/ateliet/leitor.json`. A book
//! is known by its fingerprint (start of the file and size), so the place
//! survives renaming or moving the file. The place is not written in the
//! book's note, so the note is not rewritten at every page turned.

use super::Locator;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BookState {
    #[serde(rename = "arquivo", default)]
    pub file: String,
    #[serde(rename = "titulo", default)]
    pub title: String,
    #[serde(rename = "autor", default)]
    pub author: String,
    #[serde(rename = "local", default)]
    pub at: Option<Locator>,
    /// 0–1, as shown on the shelf.
    #[serde(rename = "progresso", default)]
    pub progress: f32,
    #[serde(rename = "visto", default)]
    pub seen: String,
    /// Preferences fixed for this book (over the format's).
    #[serde(rename = "prefs", default, skip_serializing_if = "Value::is_null")]
    pub prefs: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(rename = "livros", default)]
    pub books: HashMap<String, BookState>,
    /// Preferences by format ("epub", "pdf", "hq") and general ones.
    #[serde(default)]
    pub prefs: Value,
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join("ateliet").join("leitor.json")
}

impl State {
    pub fn load(config_dir: &Path) -> State {
        std::fs::read_to_string(path(config_dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        let p = path(config_dir);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let t = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::quadro::write_atomic(&p, &t).map_err(|e| e.to_string())
    }

    /// Books by when they were last seen, newest first.
    pub fn recent(&self) -> Vec<(&String, &BookState)> {
        let mut v: Vec<(&String, &BookState)> = self.books.iter().collect();
        v.sort_by(|a, b| b.1.seen.cmp(&a.1.seen));
        v
    }

    /// The fingerprint a vault file was last seen with.
    pub fn by_file(&self, file: &str) -> Option<(&String, &BookState)> {
        self.books.iter().find(|(_, b)| b.file == file)
    }
}

/// The time now, as the state keeps it.
pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}
