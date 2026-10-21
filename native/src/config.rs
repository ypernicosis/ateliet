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

/// Known vaults, most recent first.
pub fn known_vaults() -> Vec<PathBuf> {
    let cfg = load_app_config();
    let mut v: Vec<(u64, PathBuf)> = cfg["vaults"]
        .as_object()
        .map(|o| o.values().filter_map(|x| Some((x["ts"].as_u64().unwrap_or(0), PathBuf::from(x["path"].as_str()?)))).collect())
        .unwrap_or_default();
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    v.into_iter().map(|x| x.1).filter(|p| p.is_dir()).collect()
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
        Appearance { dark, accent, font_size }
    }

    pub fn save(&self, cfg: &VaultConfig) {
        cfg.merge(
            "appearance.json",
            json!({
                "theme": if self.dark { "obsidian" } else { "moonstone" },
                "accentColor": format!("#{:02x}{:02x}{:02x}", self.accent[0], self.accent[1], self.accent[2]),
                "baseFontSize": self.font_size,
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
