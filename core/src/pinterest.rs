//! Pinterest import, compatible with Pictogrep's: the same downloaders
//! (gallery-dl when installed, otherwise a native reader of Pinterest's own
//! JSON endpoints), the same folder rules, the same followed-boards list
//! (`data/web-sources.json`) and download archives, so a board followed in one
//! app is checked by whichever app is open.

use crate::pictogrep::{self, Home, ImportBatch, Imported};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

const MAX_IMAGES: usize = 5000;
const MAX_DOWNLOAD_BYTES: u64 = 2 << 30;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const FOLLOW_LIMIT: usize = 20;
const SYNC_EVERY_SECS: i64 = 24 * 3600;
const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
const PWS_HANDLER: &str = "www/[username]/[slug].js";
const PAGE_SIZE: usize = 250;
const MAX_PAGES: usize = 320;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ── addresses ─────────────────────────────────────────────────────────────

fn is_short_host(host: &str) -> bool {
    host.trim_end_matches('.').eq_ignore_ascii_case("pin.it")
}

/// pinterest.com, pinterest.co.uk, br.pinterest.com, pinterest.de …
fn is_pinterest_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    labels.iter().enumerate().any(|(i, l)| {
        let tail = &labels[i + 1..];
        *l == "pinterest" && ((tail.len() == 1 && (tail[0] == "com" || tail[0].len() == 2)) || (tail.len() == 2 && tail[0] == "co" && tail[1].len() == 2))
    })
}

fn path_parts(url: &Url) -> Vec<String> {
    url.path_segments().map(|s| s.filter(|p| !p.is_empty()).map(String::from).collect()).unwrap_or_default()
}

/// A board, a pin, or a pin.it link, normalized the way Pictogrep does.
pub fn validate_url(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 8192 {
        return Err("paste a Pinterest board address".into());
    }
    let mut url = Url::parse(raw).map_err(|_| "paste a valid Pinterest board address".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.port().is_some() {
        return Err("paste an address from Pinterest".into());
    }
    let host = url.host_str().unwrap_or("").to_string();
    let parts = path_parts(&url);
    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_scheme("https");
    if is_short_host(&host) {
        if parts.len() != 1 {
            return Err("that pin.it link is missing its code".into());
        }
        return Ok(url);
    }
    if !is_pinterest_host(&host) {
        return Err("paste an address from Pinterest (a board, a pin, or a pin.it link)".into());
    }
    let _ = url.set_host(Some(&host.to_lowercase()));
    if parts.len() == 2 && parts[0] == "pin" {
        url.set_path(&format!("/pin/{}/", parts[1]));
        return Ok(url);
    }
    if parts.len() < 2 || ["pin", "ideas", "search"].contains(&parts[0].as_str()) {
        return Err("paste a Pinterest board or pin address, or a pin.it link".into());
    }
    url.set_path(&format!("/{}/{}/", parts[0], parts[1]));
    Ok(url)
}

/// What the import is called: the board's name from its address.
pub fn source_name(url: &Url) -> String {
    let parts = path_parts(url);
    if is_short_host(url.host_str().unwrap_or("")) || (parts.len() == 2 && parts[0] == "pin") {
        return "Pinterest".into();
    }
    match parts.get(1) {
        Some(slug) => {
            let decoded = percent_encoding::percent_decode_str(slug).decode_utf8_lossy().into_owned();
            decoded.replace(['-', '_'], " ")
        }
        None => "Pinterest board".into(),
    }
}

/// Pictogrep's download archive for a followed address, so neither app
/// downloads the same new pins twice.
pub fn archive_path(data_dir: &Path, source: &str) -> PathBuf {
    let digest = Sha256::digest(source.as_bytes());
    let name: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    data_dir.join("web-archives").join(format!("{name}.sqlite3"))
}

// ── downloaders ───────────────────────────────────────────────────────────

/// gallery-dl, looked up where Pictogrep looks for it and where Homebrew and
/// pip put it (an app started from the Dock does not get the shell's PATH).
pub fn gallery_dl() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["pictogrep-gallery-dl.exe", "gallery-dl.exe"] } else { &["pictogrep-gallery-dl", "gallery-dl"] };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications/Pictogrep.app/Contents/MacOS"));
        dirs.push(home.join(".local/bin"));
    }
    dirs.push(PathBuf::from("/Applications/Pictogrep.app/Contents/MacOS"));
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(d));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Pictogrep"));
    }
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

fn folder_usage(root: &Path) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    for e in walkdir::WalkDir::new(root).into_iter().flatten() {
        if e.file_type().is_file() {
            files += 1;
            bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    (files, bytes)
}

struct Download<'a> {
    url: &'a str,
    dir: &'a Path,
    limit: usize,
    archive: Option<&'a Path>,
    cancel: &'a AtomicBool,
    progress: &'a dyn Fn(usize),
}

fn run_gallery_dl(bin: &Path, d: &Download) -> Result<(), String> {
    let limit = if d.limit == 0 || d.limit > MAX_IMAGES { MAX_IMAGES } else { d.limit };
    let mut cmd = Command::new(bin);
    cmd.args(["--config-ignore", "--no-input", "--range", &format!("1-{}", limit + 1), "--filesize-max", &pictogrep::MAX_IMAGE_BYTES.to_string()])
        .args(["--filter", "extension in ('gif','jpeg','jpg','png','webp')"]);
    if let Some(a) = d.archive {
        if let Some(p) = a.parent() {
            let _ = fs::create_dir_all(p);
        }
        cmd.arg("--download-archive").arg(a);
    }
    cmd.arg("-D").arg(d.dir).args(["-f", "/O", d.url]);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).stdin(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: gallery-dl's launcher spawns the real
        // downloader as a child, and cancelling must stop both.
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| format!("gallery-dl could not start: {e}"))?;
    let kill = |child: &mut std::process::Child| {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    };
    // Drain the output in the background so a chatty run cannot block on a
    // full pipe; keep the tail for error messages.
    let tail = Arc::new(Mutex::new(String::new()));
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)].into_iter().flatten() {
        let tail = tail.clone();
        std::thread::spawn(move || {
            let mut pipe = pipe;
            let mut buf = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = tail.lock().unwrap();
                t.push_str(&String::from_utf8_lossy(&buf[..n]));
                if t.len() > 4000 {
                    let cut = t.len() - 2000;
                    *t = t[cut..].to_string();
                }
            }
        });
    }
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let detail = tail.lock().unwrap().trim().chars().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect::<String>();
            return Err(format!("gallery-dl could not download this address: {}", if detail.is_empty() { status.to_string() } else { detail }));
        }
        if d.cancel.load(Ordering::Relaxed) {
            kill(&mut child);
            return Ok(());
        }
        if started.elapsed() > DOWNLOAD_TIMEOUT {
            kill(&mut child);
            return Err("the download exceeded the 30 minute limit".into());
        }
        let (files, bytes) = folder_usage(d.dir);
        (d.progress)(files);
        if files > MAX_IMAGES + 1 || bytes > MAX_DOWNLOAD_BYTES {
            kill(&mut child);
            return Err("this board holds more than can safely be downloaded at once".into());
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new().try_proxy_from_env(true).timeout_connect(Duration::from_secs(20)).timeout_read(Duration::from_secs(60)).user_agent(USER_AGENT).build()
}

/// One of Pinterest's resource endpoints, called the way its own web app does:
/// the X-Pinterest-PWS-Handler header is what makes them answer without a login.
fn resource(agent: &ureq::Agent, site: &Url, name: &str, options: Value) -> Result<Option<Value>, String> {
    let mut endpoint = site.clone();
    endpoint.set_path(&format!("/resource/{name}/get/"));
    let data = json!({ "options": options, "context": {} }).to_string();
    endpoint.query_pairs_mut().clear().append_pair("source_url", site.path()).append_pair("data", &data);
    match agent
        .get(endpoint.as_str())
        .set("Accept", "application/json, text/javascript, */*, q=0.01")
        .set("Accept-Language", "en-US,en;q=0.9")
        .set("X-Pinterest-PWS-Handler", PWS_HANDLER)
        .set("Referer", site.as_str())
        .call()
    {
        Ok(r) => serde_json::from_str::<Value>(&r.into_string().map_err(|e| e.to_string())?).map(Some).map_err(|e| e.to_string()),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// The picture of a pin: the original when there is one, else the largest.
fn pin_image(pin: &Value, base: &Url) -> Option<(String, String)> {
    let id = pin["id"].as_str()?.to_string();
    let images = pin["images"].as_object()?;
    let best = images
        .get("orig")
        .and_then(|o| o["url"].as_str())
        .map(String::from)
        .or_else(|| images.values().filter(|i| i["url"].is_string()).max_by_key(|i| i["width"].as_u64().unwrap_or(0) * i["height"].as_u64().unwrap_or(0)).and_then(|i| i["url"].as_str()).map(String::from))?;
    let resolved = base.join(best.trim()).ok()?;
    let ext = resolved.path().rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    (resolved.scheme().starts_with("http") && pictogrep::is_image_name(&format!("x.{ext}"))).then(|| (resolved.to_string(), format!("pinterest:{id}")))
}

/// Every picture of a public board (or the one picture of a pin).
fn native_list(agent: &ureq::Agent, mut board: Url, limit: usize) -> Result<Vec<(String, String)>, String> {
    let short = is_short_host(board.host_str().unwrap_or(""));
    if short {
        let r = agent.get(board.as_str()).call().map_err(|e| format!("that pin.it link could not be opened: {e}"))?;
        board = Url::parse(r.get_url()).map_err(|e| e.to_string())?;
    }
    let parts = path_parts(&board);
    if parts.len() == 2 && parts[0] == "pin" {
        let answer = resource(agent, &board, "PinResource", json!({ "id": parts[1], "field_set_key": "detailed" }))?.ok_or("that pin does not exist any more")?;
        return pin_image(&answer["resource_response"]["data"], &board).map(|i| vec![i]).ok_or_else(|| "there is no picture on that pin; video and idea pins cannot be saved".into());
    }
    if parts.len() < 2 {
        return Err(if short { "that pin.it link does not lead anywhere any more".into() } else { "that is a Pinterest profile, not a board".into() });
    }
    let answer = resource(agent, &board, "BoardResource", json!({ "username": parts[0], "slug": parts[1] }))?;
    let board_id = answer.as_ref().and_then(|a| a["resource_response"]["data"]["id"].as_str()).map(String::from).ok_or("there is no public board at that address (private boards cannot be read)")?;
    let mut images = Vec::new();
    let mut seen = HashSet::new();
    let mut bookmark = String::new();
    for _ in 0..MAX_PAGES {
        if images.len() >= limit {
            break;
        }
        let mut options = json!({ "board_id": board_id, "page_size": PAGE_SIZE });
        if !bookmark.is_empty() {
            options["bookmarks"] = json!([bookmark]);
        }
        let Ok(Some(page)) = resource(agent, &board, "BoardFeedResource", options) else { break };
        for pin in page["resource_response"]["data"].as_array().into_iter().flatten() {
            if let Some((url, id)) = pin_image(pin, &board) {
                if seen.insert(id.clone()) {
                    images.push((url, id));
                }
            }
        }
        let next = page["resource_response"]["bookmark"].as_str().or_else(|| page["resource"]["options"]["bookmarks"][0].as_str()).unwrap_or("").to_string();
        if next.is_empty() || next == "-end-" || next == bookmark {
            break;
        }
        bookmark = next;
    }
    images.truncate(limit);
    Ok(images)
}

fn run_native(d: &Download) -> Result<(), String> {
    let agent = http();
    let limit = if d.limit == 0 || d.limit > MAX_IMAGES { MAX_IMAGES } else { d.limit };
    let target = Url::parse(d.url).map_err(|e| e.to_string())?;
    let images = native_list(&agent, target, limit)?;
    let archive = d.archive.map(|a| PathBuf::from(format!("{}.native", a.display())));
    let seen: HashSet<String> = archive.as_ref().and_then(|a| fs::read_to_string(a).ok()).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default();
    let mut saved = 0;
    let mut budget = 0u64;
    let started = Instant::now();
    for (index, (url, id)) in images.into_iter().filter(|(_, id)| !seen.contains(id)).take(limit).enumerate() {
        if d.cancel.load(Ordering::Relaxed) || started.elapsed() > DOWNLOAD_TIMEOUT {
            break;
        }
        let ext = url.rsplit_once('.').map(|(_, e)| e.split(['?', '#']).next().unwrap_or("jpg").to_lowercase()).filter(|e| pictogrep::is_image_name(&format!("x.{e}"))).unwrap_or_else(|| "jpg".into());
        let dest = d.dir.join(format!("{index:05}.{ext}"));
        // A dead pin is not a failed import: skip it and keep going.
        let Ok(resp) = agent.get(&url).timeout(Duration::from_secs(300)).call() else { continue };
        if !resp.content_type().starts_with("image/") {
            continue;
        }
        let mut body = Vec::new();
        if resp.into_reader().take(pictogrep::MAX_IMAGE_BYTES + 1).read_to_end(&mut body).is_err() || body.is_empty() || body.len() as u64 > pictogrep::MAX_IMAGE_BYTES {
            continue;
        }
        if fs::write(&dest, &body).is_err() {
            continue;
        }
        budget += body.len() as u64;
        saved += 1;
        (d.progress)(saved);
        if let Some(a) = &archive {
            if let Some(p) = a.parent() {
                let _ = fs::create_dir_all(p);
            }
            if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(a) {
                let _ = writeln!(f, "{id}");
            }
        }
        if budget >= MAX_DOWNLOAD_BYTES {
            break;
        }
    }
    Ok(())
}

// ── the import ────────────────────────────────────────────────────────────

/// Where downloaded pictures go.
#[derive(Clone)]
pub enum Dest {
    /// The shared library, optionally collected in a collection.
    Shared { home: Home, collection: Option<String> },
    /// A folder of the vault (when no shared library is in use).
    Vault { dir: PathBuf },
}

pub struct Plan {
    pub source: String,
    pub name: String,
    pub dest: Dest,
    pub archive: Option<PathBuf>,
    pub limit: usize,
    pub link_duplicates: bool,
    pub allow_empty: bool,
    /// Pictures already there, for the duplicate check, with digests known.
    pub existing: Vec<PathBuf>,
    pub digests: HashMap<PathBuf, [u8; 32]>,
}

pub struct Outcome {
    pub result: Value,
    pub added: Vec<PathBuf>,
}

/// Downloads into a temporary folder, then moves every new picture into the
/// destination. Whatever arrived before a cancel is still imported.
pub fn run(plan: &Plan, cancel: &AtomicBool, progress: &dyn Fn(&str, usize, usize)) -> Result<Outcome, String> {
    let tmp = std::env::temp_dir().join(format!("ateliet-download-{}-{}", std::process::id(), Instant::now().elapsed().as_nanos() ^ now() as u128));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let cleanup = TempDir(tmp.clone());
    let report = |files: usize| progress("downloading", files, 0);
    let d = Download { url: &plan.source, dir: &tmp, limit: plan.limit, archive: plan.archive.as_deref(), cancel, progress: &report };
    let downloaded = match gallery_dl() {
        Some(bin) => run_gallery_dl(&bin, &d),
        None => run_native(&d),
    };
    if let Err(e) = downloaded {
        if !cancel.load(Ordering::Relaxed) {
            return Err(e);
        }
    }
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&tmp).into_iter().flatten().filter(|e| e.file_type().is_file() && pictogrep::is_image_name(&e.file_name().to_string_lossy())).map(|e| e.into_path()).collect();
    files.sort();
    files.truncate(MAX_IMAGES);
    if files.is_empty() && !plan.allow_empty && !cancel.load(Ordering::Relaxed) {
        return Err("nothing at that address could be downloaded as a picture".into());
    }
    let (dir, collection, home) = match &plan.dest {
        Dest::Shared { home, collection } => (home.library(), collection.clone(), Some(home.clone())),
        Dest::Vault { dir } => (dir.clone(), None, None),
    };
    let mut batch = ImportBatch::new(&plan.existing, &plan.digests);
    let mut in_collection: HashSet<PathBuf> = match (&home, &collection) {
        (Some(h), Some(c)) => pictogrep::collection_images(h, c).into_iter().collect(),
        _ => HashSet::new(),
    };
    let (mut imported, mut skipped, mut linked, mut failed) = (0, 0, 0, 0);
    let mut last_error = String::new();
    let total = files.len();
    for (i, file) in files.iter().enumerate() {
        progress("importing", i, total);
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match pictogrep::import_file(file, &name, &dir, &mut batch) {
            Ok(Imported::New(path)) => {
                imported += 1;
                if let (Some(h), Some(c)) = (&home, &collection) {
                    if in_collection.insert(path.clone()) {
                        let _ = pictogrep::link(h, c, &path);
                    }
                }
            }
            Ok(Imported::Duplicate(path)) => match (&home, &collection) {
                (Some(h), Some(c)) if plan.link_duplicates && !in_collection.contains(&path) => {
                    in_collection.insert(path.clone());
                    let _ = pictogrep::link(h, c, &path);
                    linked += 1;
                }
                _ => skipped += 1,
            },
            Err(e) => {
                failed += 1;
                last_error = e;
            }
        }
        let _ = fs::remove_file(file);
    }
    progress("importing", total, total);
    if let Some(h) = &home {
        if let Some(c) = &collection {
            if total > 0 {
                let _ = pictogrep::create_collection(h, c);
            }
        }
        pictogrep::library_changed(h, &batch.added);
    }
    drop(cleanup);
    Ok(Outcome {
        result: json!({
            "board": plan.name, "sourceUrl": plan.source, "folder": collection.unwrap_or_default(),
            "total": total, "imported": imported, "skipped": skipped, "linked": linked, "failed": failed, "lastError": last_error,
        }),
        added: batch.added,
    })
}

/// Removes the download folder however the import ends.
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ── followed boards (shared with Pictogrep) ───────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Followed {
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub last_sync_at: i64,
    #[serde(default)]
    pub added_at: i64,
}

pub fn followed_path(data_dir: &Path) -> PathBuf {
    data_dir.join("web-sources.json")
}

pub fn followed(data_dir: &Path) -> Vec<Followed> {
    fs::read_to_string(followed_path(data_dir)).ok().and_then(|t| serde_json::from_str::<Vec<Followed>>(&t).ok()).unwrap_or_default().into_iter().filter(|f| !f.url.trim().is_empty()).collect()
}

fn save_followed(data_dir: &Path, list: &[Followed]) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let path = followed_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(list).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

pub fn follow(data_dir: &Path, mut entry: Followed) -> Result<(), String> {
    let mut list = followed(data_dir);
    match list.iter_mut().find(|f| f.url == entry.url) {
        Some(existing) => {
            if entry.added_at == 0 {
                entry.added_at = existing.added_at;
            }
            *existing = entry;
        }
        None => list.push(entry),
    }
    save_followed(data_dir, &list)
}

pub fn forget(data_dir: &Path, url: &str) -> Result<(), String> {
    let list: Vec<Followed> = followed(data_dir).into_iter().filter(|f| f.url != url).collect();
    let _ = fs::remove_file(archive_path(data_dir, url));
    let _ = fs::remove_file(PathBuf::from(format!("{}.native", archive_path(data_dir, url).display())));
    save_followed(data_dir, &list)
}

/// Pictogrep's "check followed pages automatically" switch (config.json web.autoSync).
pub fn auto_sync(home: &Home) -> bool {
    fs::read_to_string(home.config_path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["web"]["autoSync"].as_bool()).unwrap_or(true)
}

pub fn due(f: &Followed) -> bool {
    now() - f.last_sync_at >= SYNC_EVERY_SECS
}

pub fn mark_synced(data_dir: &Path, url: &str) {
    let mut list = followed(data_dir);
    if let Some(f) = list.iter_mut().find(|f| f.url == url) {
        f.last_sync_at = now();
        let _ = save_followed(data_dir, &list);
    }
}

pub const FOLLOW_NEWEST: usize = FOLLOW_LIMIT;

// ── job state ─────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Default)]
pub struct Job {
    pub state: String,
    pub board: String,
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub automatic: bool,
    pub stopping: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Downloads {
    pub job: Mutex<Job>,
    pub cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub sync_loop: AtomicBool,
}

impl Downloads {
    pub fn snapshot(&self) -> Job {
        let mut j = self.job.lock().unwrap().clone();
        if j.state.is_empty() {
            j.state = "idle".into();
        }
        j
    }

    pub fn start(&self, board: &str, automatic: bool) -> Option<Arc<AtomicBool>> {
        let mut job = self.job.lock().unwrap();
        if job.state == "running" {
            return None;
        }
        *job = Job { state: "running".into(), board: board.into(), phase: "downloading".into(), automatic, ..Default::default() };
        let flag = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().unwrap() = Some(flag.clone());
        Some(flag)
    }

    pub fn progress(&self, phase: &str, done: usize, total: usize) {
        let mut job = self.job.lock().unwrap();
        job.phase = phase.into();
        job.done = done;
        job.total = total;
    }

    pub fn finish(&self, outcome: Result<Value, String>) {
        let cancelled = self.cancel.lock().unwrap().take().is_some_and(|c| c.load(Ordering::Relaxed));
        let mut job = self.job.lock().unwrap();
        job.phase.clear();
        job.stopping = false;
        match outcome {
            Ok(r) => {
                job.state = if cancelled { "cancelled".into() } else { "done".into() };
                job.result = Some(r);
            }
            Err(e) => {
                job.state = "error".into();
                job.error = Some(e);
            }
        }
    }

    pub fn stop(&self) -> bool {
        match self.cancel.lock().unwrap().as_ref() {
            Some(c) => {
                c.store(true, Ordering::Relaxed);
                self.job.lock().unwrap().stopping = true;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_normalized_like_pictogrep() {
        assert_eq!(validate_url("http://BR.Pinterest.com/ana/gatos-pretos/?x=1#y").unwrap().as_str(), "https://br.pinterest.com/ana/gatos-pretos/");
        assert_eq!(validate_url("https://www.pinterest.com/pin/12345").unwrap().path(), "/pin/12345/");
        assert_eq!(validate_url("https://pin.it/AbC").unwrap().as_str(), "https://pin.it/AbC");
        assert!(validate_url("https://pinterest.co.uk/ana/refs").is_ok());
        assert!(validate_url("https://example.com/ana/refs").is_err());
        assert!(validate_url("https://pinterest.com/ana").is_err());
        assert!(validate_url("https://pinterest.com/search/pins/?q=x").is_err());
        assert_eq!(source_name(&validate_url("https://pinterest.com/ana/m%C3%A3os_e-poses/").unwrap()), "mãos e poses");
    }

    #[test]
    fn pin_images_prefer_the_original() {
        let base = Url::parse("https://www.pinterest.com/ana/refs/").unwrap();
        let pin = json!({ "id": "7", "images": { "236x": { "url": "https://i.pinimg.com/236x/a.jpg", "width": 236, "height": 300 }, "orig": { "url": "//i.pinimg.com/originals/a.jpg", "width": 900, "height": 1200 } } });
        assert_eq!(pin_image(&pin, &base), Some(("https://i.pinimg.com/originals/a.jpg".into(), "pinterest:7".into())));
        let video = json!({ "id": "8", "images": { "orig": { "url": "https://v.pinimg.com/x.mp4" } } });
        assert_eq!(pin_image(&video, &base), None);
    }

    #[test]
    fn followed_list_roundtrip() {
        let dir = std::env::temp_dir().join(format!("ateliet-follow-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        follow(&dir, Followed { url: "https://pinterest.com/a/b/".into(), name: "b".into(), folder: "b".into(), last_sync_at: 1, added_at: 1 }).unwrap();
        follow(&dir, Followed { url: "https://pinterest.com/a/b/".into(), name: "b".into(), folder: "c".into(), last_sync_at: 2, added_at: 0 }).unwrap();
        let list = followed(&dir);
        assert_eq!((list.len(), list[0].folder.as_str(), list[0].added_at), (1, "c", 1));
        assert!(due(&list[0]));
        forget(&dir, "https://pinterest.com/a/b/").unwrap();
        assert!(followed(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
