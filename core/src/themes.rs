//! The community theme catalog (the list Obsidian's theme browser reads) and
//! installing a theme the way Obsidian does: `theme.css` and `manifest.json`
//! in `.obsidian/themes/<name>/`.

use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CATALOG_URL: &str = "https://raw.githubusercontent.com/obsidianmd/obsidian-releases/HEAD/community-css-themes.json";

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub author: String,
    /// `owner/repo` on GitHub.
    pub repo: String,
    /// The screenshot's path inside the repository.
    pub screenshot: String,
    /// "dark", "light" or both.
    pub modes: Vec<String>,
    /// Themes made before Obsidian 0.16 ship `obsidian.css` instead of `theme.css`.
    pub legacy: bool,
}

impl Theme {
    pub fn screenshot_url(&self) -> Option<String> {
        (!self.screenshot.is_empty()).then(|| raw(&self.repo, &self.screenshot))
    }
}

fn raw(repo: &str, file: &str) -> String {
    let file: String = file.split('/').map(|s| percent(s)).collect::<Vec<_>>().join("/");
    format!("https://raw.githubusercontent.com/{repo}/HEAD/{file}")
}

fn percent(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out += &format!("%{b:02X}");
        }
    }
    out
}

/// The catalog's entries; ones without a name or a valid repository are left out.
pub fn parse_catalog(json: &str) -> Result<Vec<Theme>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("catálogo inválido: {e}"))?;
    let s = |e: &Value, k: &str| e[k].as_str().unwrap_or("").trim().to_string();
    Ok(v.as_array()
        .ok_or("catálogo inválido")?
        .iter()
        .map(|e| Theme {
            name: s(e, "name"),
            author: s(e, "author"),
            repo: s(e, "repo"),
            screenshot: s(e, "screenshot"),
            modes: e["modes"].as_array().into_iter().flatten().filter_map(|m| m.as_str()).map(String::from).collect(),
            legacy: e["legacy"].as_bool().unwrap_or(false),
        })
        .filter(|t| !t.name.is_empty() && valid_repo(&t.repo))
        .collect())
}

fn valid_repo(r: &str) -> bool {
    let mut parts = r.split('/');
    let ok = |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p != "." && p != ".." && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)));
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// The folder name for a theme: its name, without characters a path can't hold.
pub fn folder_name(name: &str) -> Option<String> {
    let n: String = name.chars().filter(|c| !"/\\:*?\"<>|".contains(*c) && !c.is_control()).collect();
    let n = n.trim().trim_start_matches('.').to_string();
    (!n.is_empty()).then_some(n)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().try_proxy_from_env(true).timeout_connect(Duration::from_secs(20)).timeout_read(Duration::from_secs(60)).build()
}

fn get(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let r = agent().get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("{url}: HTTP {code}"),
        e => format!("{url}: {e}"),
    })?;
    let mut buf = Vec::new();
    r.into_reader().take(limit).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

pub fn fetch_catalog() -> Result<Vec<Theme>, String> {
    let bytes = get(CATALOG_URL, 8 << 20)?;
    parse_catalog(&String::from_utf8_lossy(&bytes))
}

pub fn fetch_screenshot(t: &Theme) -> Result<Vec<u8>, String> {
    get(&t.screenshot_url().ok_or("sem imagem")?, 16 << 20)
}

/// Downloads the theme's CSS and manifest (a manifest is made up when the
/// repository has none).
pub fn download(t: &Theme) -> Result<(String, String), String> {
    let files: &[&str] = if t.legacy { &["obsidian.css", "theme.css"] } else { &["theme.css", "obsidian.css"] };
    let mut err = String::new();
    let mut css = None;
    for f in files {
        match get(&raw(&t.repo, f), 16 << 20) {
            Ok(b) => {
                css = Some(String::from_utf8_lossy(&b).into_owned());
                break;
            }
            Err(e) => err = e,
        }
    }
    let css = css.ok_or(err)?;
    let manifest = get(&raw(&t.repo, "manifest.json"), 1 << 20)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({ "name": t.name, "version": "0.0.0", "minAppVersion": "0.16.0", "author": t.author }));
    Ok((css, serde_json::to_string_pretty(&manifest).unwrap_or_default()))
}

/// Writes the theme into `themes_dir/<name>/`, replacing an older install;
/// returns the folder name (what `cssTheme` in appearance.json names).
pub fn install(themes_dir: &Path, name: &str, css: &str, manifest: &str) -> Result<String, String> {
    let folder = folder_name(name).ok_or("nome de tema inválido")?;
    let dir: PathBuf = themes_dir.join(&folder);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (file, text) in [("theme.css", css), ("manifest.json", manifest)] {
        let tmp = dir.join(format!(".{file}.tmp"));
        fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        fs::rename(&tmp, dir.join(file)).map_err(|e| e.to_string())?;
    }
    Ok(folder)
}

/// The installed version of a theme (from its manifest), if any.
pub fn installed_version(themes_dir: &Path, name: &str) -> Option<String> {
    let dir = themes_dir.join(folder_name(name)?);
    if !dir.join("theme.css").is_file() {
        return None;
    }
    let m: Value = fs::read_to_string(dir.join("manifest.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    Some(m["version"].as_str().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_catalog_and_skips_bad_entries() {
        let json = r#"[
            {"name": "Minimal", "author": "kepano", "repo": "kepano/obsidian-minimal", "screenshot": "dark simple.png", "modes": ["dark", "light"]},
            {"name": "Old", "author": "x", "repo": "x/old-theme", "screenshot": "s.png", "modes": ["dark"], "legacy": true},
            {"name": "Bad", "author": "x", "repo": "../etc", "screenshot": "", "modes": []},
            {"name": "", "author": "x", "repo": "x/y", "screenshot": "", "modes": []}
        ]"#;
        let list = parse_catalog(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].modes, vec!["dark", "light"]);
        assert!(list[1].legacy);
        assert_eq!(list[0].screenshot_url().unwrap(), "https://raw.githubusercontent.com/kepano/obsidian-minimal/HEAD/dark%20simple.png");
    }

    #[test]
    fn installs_into_a_folder_named_after_the_theme() {
        let dir = std::env::temp_dir().join(format!("ateliet-themes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(install(&dir, "Things: 2/ok", "body{}", "{\"version\":\"1.2\"}").unwrap(), "Things 2ok");
        assert_eq!(fs::read_to_string(dir.join("Things 2ok/theme.css")).unwrap(), "body{}");
        assert_eq!(installed_version(&dir, "Things: 2/ok").as_deref(), Some("1.2"));
        assert!(install(&dir, "..", "", "").is_err());
        assert_eq!(installed_version(&dir, "Nope"), None);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod net {
    #[test]
    #[ignore = "network"]
    fn fetches_the_catalog_and_a_theme() {
        let list = super::fetch_catalog().unwrap();
        assert!(list.len() > 100);
        let t = list.iter().find(|t| t.name == "Minimal").unwrap();
        let (css, manifest) = super::download(t).unwrap();
        assert!(css.contains("--"));
        assert!(manifest.contains("version"));
    }
}
