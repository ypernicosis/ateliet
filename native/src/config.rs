//! Settings shared with the web interface and with Obsidian: the vault's
//! `.obsidian/*.json` files and Ateliet's own `ateliet.json` (known vaults).

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// `~/.config/ateliet/ateliet.json`, the same file the Tauri shell uses.
pub fn app_config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("ateliet").join("ateliet.json")
}

pub fn load_app_config() -> Value {
    fs::read_to_string(app_config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({ "vaults": {} }))
}

pub fn remember_vault(path: &Path) {
    let mut cfg = load_app_config();
    let p = path.to_string_lossy().to_string();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    if !cfg["vaults"].is_object() {
        cfg["vaults"] = json!({});
    }
    cfg["vaults"][&p] = json!({ "path": p, "ts": now });
    cfg["last"] = json!(p);
    let file = app_config_path();
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(file, serde_json::to_string_pretty(&cfg).unwrap_or_default());
}

pub fn last_vault() -> Option<PathBuf> {
    load_app_config()["last"].as_str().map(PathBuf::from).filter(|p| p.is_dir())
}

/// Obsidian's own list of vaults (`obsidian.json`), where the system keeps it.
fn obsidian_vaults() -> Vec<(u64, PathBuf)> {
    let mut files = Vec::new();
    if let Some(c) = dirs::config_dir() {
        files.push(c.join("obsidian").join("obsidian.json"));
    }
    if let Some(h) = dirs::home_dir() {
        // the Flatpak and Snap builds keep their own
        files.push(h.join(".var/app/md.obsidian.Obsidian/config/obsidian/obsidian.json"));
        files.push(h.join("snap/obsidian/current/.config/obsidian/obsidian.json"));
    }
    let mut out = Vec::new();
    for f in files {
        let Some(v) = fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { continue };
        if let Some(o) = v["vaults"].as_object() {
            out.extend(o.values().filter_map(|x| Some((x["ts"].as_u64().unwrap_or(0), PathBuf::from(x["path"].as_str()?)))));
        }
    }
    out
}

/// Known vaults (Ateliet's and Obsidian's), most recent first.
pub fn known_vaults() -> Vec<PathBuf> {
    let cfg = load_app_config();
    let mut v: Vec<(u64, PathBuf)> = cfg["vaults"]
        .as_object()
        .map(|o| o.values().filter_map(|x| Some((x["ts"].as_u64().unwrap_or(0), PathBuf::from(x["path"].as_str()?)))).collect())
        .unwrap_or_default();
    v.extend(obsidian_vaults());
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    let mut seen = std::collections::HashSet::new();
    v.into_iter().map(|x| x.1).filter(|p| p.is_dir() && seen.insert(p.clone())).collect()
}

/// One `.obsidian` folder.
#[derive(Clone)]
pub struct VaultConfig {
    pub dir: PathBuf,
}

impl VaultConfig {
    pub fn new(root: &Path) -> Self {
        VaultConfig { dir: root.join(".obsidian") }
    }

    pub fn read(&self, name: &str) -> Value {
        fs::read_to_string(self.dir.join(name)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
    }

    /// Merge the given keys into a JSON file, keeping the keys we do not know.
    pub fn merge(&self, name: &str, changes: Value) {
        let mut cur = self.read(name);
        if !cur.is_object() {
            cur = json!({});
        }
        if let (Some(o), Some(c)) = (cur.as_object_mut(), changes.as_object()) {
            for (k, v) in c {
                o.insert(k.clone(), v.clone());
            }
        }
        let path = self.dir.join(name);
        if let Some(d) = path.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(path, serde_json::to_string_pretty(&cur).unwrap_or_default());
    }

    /// Remove one key from a JSON file, keeping the rest.
    pub fn remove(&self, name: &str, key: &str) {
        let mut cur = self.read(name);
        if let Some(o) = cur.as_object_mut() {
            if o.remove(key).is_some() {
                let _ = fs::write(self.dir.join(name), serde_json::to_string_pretty(&cur).unwrap_or_default());
            }
        }
    }

    /// A built-in tool's settings: `ateliet/<id>.json`, or the old plugin's data.json.
    pub fn tool(&self, id: &str, plugin: &str) -> Value {
        let own = self.read(&format!("ateliet/{id}.json"));
        if own.is_object() {
            return own;
        }
        self.read(&format!("plugins/{plugin}/data.json"))
    }
}

/// Appearance: light or dark, accent colour, text size.
#[derive(Clone, Debug)]
pub struct Appearance {
    pub dark: bool,
    pub accent: [u8; 3],
    pub font_size: f32,
    /// The community theme (`.obsidian/themes/<name>`), empty for none.
    pub css_theme: String,
    /// Its colour variables and the enabled snippets': (light, dark).
    pub theme_vars: Option<std::sync::Arc<(ateliet_core::css_theme::Vars, ateliet_core::css_theme::Vars)>>,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance { dark: false, accent: [0x8a, 0x5c, 0xf5], font_size: 16.0, css_theme: String::new(), theme_vars: None }
    }
}

impl Appearance {
    pub fn load(cfg: &VaultConfig) -> Self {
        let a = cfg.read("appearance.json");
        let dark = match a["theme"].as_str() {
            Some("moonstone") => false,
            Some("obsidian") => true,
            // "system" or missing: follow the system later; default to light
            _ => false,
        };
        let accent = a["accentColor"].as_str().and_then(parse_hex).unwrap_or([0x8a, 0x5c, 0xf5]);
        let font_size = a["baseFontSize"].as_f64().map(|f| f as f32).unwrap_or(16.0).clamp(10.0, 30.0);
        let mut ap = Appearance { dark, accent, font_size, css_theme: a["cssTheme"].as_str().unwrap_or("").to_string(), theme_vars: None };
        ap.load_css(cfg);
        ap
    }

    /// The themes installed in the vault.
    pub fn themes(cfg: &VaultConfig) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(cfg.dir.join("themes")).into_iter().flatten().flatten().filter(|e| e.path().join("theme.css").is_file()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        v.sort_by_key(|a| a.to_lowercase());
        v
    }

    /// Read the theme's and the enabled snippets' variables.
    pub fn load_css(&mut self, cfg: &VaultConfig) {
        let mut css = String::new();
        if !self.css_theme.is_empty() {
            css += &fs::read_to_string(cfg.dir.join("themes").join(&self.css_theme).join("theme.css")).unwrap_or_default();
        }
        let a = cfg.read("appearance.json");
        for s in a["enabledCssSnippets"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
            css.push('\n');
            css += &fs::read_to_string(cfg.dir.join("snippets").join(format!("{s}.css"))).unwrap_or_default();
        }
        self.theme_vars = if css.contains("--") { Some(std::sync::Arc::new(ateliet_core::css_theme::variables(&css))) } else { None };
    }

    pub fn save(&self, cfg: &VaultConfig) {
        cfg.merge(
            "appearance.json",
            json!({
                "theme": if self.dark { "obsidian" } else { "moonstone" },
                "accentColor": format!("#{:02x}{:02x}{:02x}", self.accent[0], self.accent[1], self.accent[2]),
                "baseFontSize": self.font_size,
                "cssTheme": self.css_theme,
            }),
        );
    }
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}
