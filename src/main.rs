#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Ateliet desktop shell: Tauri glue around `ateliet_core`.

mod gallery;
mod pictogrep;
mod pinterest;
mod process;
mod shared;
mod update;

use ateliet_core::{metadata, render, search, vault};
use base64::Engine;
use notify::Watcher;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::http::{header, Request as HttpRequest, Response as HttpResponse, StatusCode};
use tauri::ipc::Response;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

type Res<T> = Result<T, String>;

fn e2s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Default)]
struct AppState {
    watchers: Mutex<HashMap<String, notify::RecommendedWatcher>>,
}

// ── configuration ─────────────────────────────────────────────────────────

fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("ateliet").join("ateliet.json")
}

fn load_config() -> Value {
    fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({ "vaults": {} }))
}

fn save_config(v: &Value) {
    let p = config_path();
    let _ = fs::create_dir_all(p.parent().unwrap());
    let _ = fs::write(p, serde_json::to_string_pretty(v).unwrap_or_default());
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn remember_vault(path: &str) {
    let mut c = load_config();
    if !c["vaults"].is_object() {
        c["vaults"] = json!({});
    }
    c["vaults"][path] = json!({ "ts": now_ms() });
    c["last"] = json!(path);
    save_config(&c);
}

#[tauri::command]
fn known_vaults() -> Value {
    let mut out: HashMap<String, Value> = HashMap::new();
    let c = load_config();
    if let Some(v) = c["vaults"].as_object() {
        for (p, meta) in v {
            out.insert(p.clone(), json!({ "path": p, "ts": meta["ts"], "source": "ateliet" }));
        }
    }
    // Vaults known to Obsidian, so the same vault can be opened in both apps.
    let obs = dirs::config_dir().map(|d| d.join("obsidian").join("obsidian.json"));
    if let Some(Ok(s)) = obs.map(fs::read_to_string) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(vs) = v["vaults"].as_object() {
                for (_, meta) in vs {
                    if let Some(p) = meta["path"].as_str() {
                        out.entry(p.to_string()).or_insert_with(|| json!({ "path": p, "ts": meta["ts"], "source": "obsidian" }));
                    }
                }
            }
        }
    }
    let mut list: Vec<Value> = out
        .into_values()
        .filter(|v| Path::new(v["path"].as_str().unwrap_or("")).is_dir())
        .map(|mut v| {
            let p = v["path"].as_str().unwrap_or("").to_string();
            v["name"] = json!(Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(p));
            v
        })
        .collect();
    list.sort_by(|a, b| b["ts"].as_u64().unwrap_or(0).cmp(&a["ts"].as_u64().unwrap_or(0)));
    json!({ "vaults": list, "last": c["last"] })
}

#[tauri::command]
fn forget_vault(path: String) {
    let mut c = load_config();
    if let Some(v) = c["vaults"].as_object_mut() {
        v.remove(&path);
    }
    if c["last"].as_str() == Some(&path) {
        c["last"] = Value::Null;
    }
    save_config(&c);
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    app.dialog().file().blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn create_vault(parent: String, name: String) -> Res<String> {
    let p = Path::new(&parent).join(name.trim());
    fs::create_dir_all(p.join(".obsidian")).map_err(e2s)?;
    Ok(p.to_string_lossy().into_owned())
}

fn vault_url(path: &str) -> String {
    format!("index.html?vault={}", percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC))
}

fn open_window(app: &tauri::AppHandle, vault_path: Option<&str>) -> Res<()> {
    let label = format!("w{}", now_ms());
    let url = vault_path.map(vault_url).unwrap_or_else(|| "index.html".into());
    let b = WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title("Ateliet")
        .inner_size(1280.0, 820.0)
        .min_inner_size(480.0, 360.0)
        .disable_drag_drop_handler();
    #[cfg(target_os = "macos")]
    // Centre the traffic lights in the 40px header row (`--header-height`).
    let b = b.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true).traffic_light_position(tauri::LogicalPosition::new(16.0, 20.0));
    b.build().map_err(e2s)?;
    Ok(())
}

#[tauri::command]
fn open_vault(app: tauri::AppHandle, path: String, new_window: bool) -> Res<String> {
    if !Path::new(&path).is_dir() {
        return Err(format!("Not a folder: {path}"));
    }
    remember_vault(&path);
    if new_window {
        open_window(&app, Some(&path))?;
    }
    Ok(vault_url(&path))
}

#[tauri::command]
fn open_launcher(app: tauri::AppHandle) -> Res<()> {
    open_window(&app, None)
}

// ── vault ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct VaultInfo {
    path: String,
    name: String,
    files: Vec<vault::Entry>,
}

#[tauri::command]
fn vault_init(app: tauri::AppHandle, window: tauri::Window, state: State<AppState>, path: String) -> Res<VaultInfo> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("Vault not found: {path}"));
    }
    remember_vault(&path);
    let files = vault::list(&root);

    // File watcher → batched "vault-fs" events with vault-relative paths.
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, notify::EventKind::Access(_)) {
                return;
            }
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    })
    .map_err(e2s)?;
    watcher.watch(&root, notify::RecursiveMode::Recursive).map_err(e2s)?;
    let label = window.label().to_string();
    let root2 = root.clone();
    let app2 = app.clone();
    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut batch: HashSet<PathBuf> = HashSet::from([first]);
            let deadline = std::time::Instant::now() + Duration::from_millis(80);
            while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
                match rx.recv_timeout(left) {
                    Ok(p) => {
                        batch.insert(p);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(_) => return,
                }
            }
            let rels: Vec<String> = batch
                .iter()
                .map(|p| vault::rel_path(&root2, p))
                .filter(|r| !r.is_empty() && !r.starts_with(".git/") && r != ".git" && !r.starts_with(".trash"))
                .collect();
            if !rels.is_empty() && app2.emit_to(label.as_str(), "vault-fs", rels).is_err() {
                break;
            }
        }
    });
    state.watchers.lock().unwrap().insert(window.label().to_string(), watcher);

    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
    Ok(VaultInfo { path, name, files })
}

/// Parse every markdown file of the vault in parallel.
#[tauri::command]
async fn vault_metadata_all(path: String) -> Res<Value> {
    let root = PathBuf::from(&path);
    let files: Vec<vault::Entry> = vault::list(&root).into_iter().filter(|e| !e.folder && e.path.to_lowercase().ends_with(".md")).collect();
    let pairs: Vec<(String, Value)> = files
        .par_iter()
        .filter_map(|e| {
            let text = fs::read_to_string(root.join(&e.path)).ok()?;
            Some((e.path.clone(), json!({ "meta": metadata::parse(&text), "mtime": e.mtime, "size": e.size })))
        })
        .collect();
    Ok(Value::Object(pairs.into_iter().collect()))
}

#[tauri::command]
fn md_parse(text: String) -> Value {
    metadata::parse(&text)
}

#[tauri::command]
fn md_render(text: String) -> Vec<render::Block> {
    render::render(&text)
}

#[tauri::command]
async fn search_vault(root: String, query: String, case_sensitive: bool) -> Vec<search::FileResult> {
    search::search(Path::new(&root), &query, case_sensitive, 500)
}

// ── file system (absolute paths) ──────────────────────────────────────────

#[tauri::command]
async fn fs_read_text(path: String) -> Res<String> {
    let b = fs::read(&path).map_err(|e| format!("{e}: {path}"))?;
    Ok(String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

#[tauri::command]
async fn fs_read_bytes(path: String) -> Res<Response> {
    fs::read(&path).map(Response::new).map_err(|e| format!("{e}: {path}"))
}

fn ensure_parent(p: &Path) {
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
}

#[tauri::command]
async fn fs_write_text(path: String, data: String) -> Res<Option<vault::Stat>> {
    let p = PathBuf::from(&path);
    ensure_parent(&p);
    fs::write(&p, data).map_err(e2s)?;
    Ok(vault::stat(&p))
}

#[tauri::command]
async fn fs_append(path: String, data: String) -> Res<Option<vault::Stat>> {
    use std::io::Write;
    let p = PathBuf::from(&path);
    ensure_parent(&p);
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&p).map_err(e2s)?;
    f.write_all(data.as_bytes()).map_err(e2s)?;
    Ok(vault::stat(&p))
}

/// Binary write: raw body, target path in the `x-path` header (percent-encoded).
#[tauri::command]
async fn fs_write_bytes(request: tauri::ipc::Request<'_>) -> Res<Option<vault::Stat>> {
    let path = request
        .headers()
        .get("x-path")
        .and_then(|v| v.to_str().ok())
        .map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned())
        .ok_or("missing x-path header")?;
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("expected raw body".into());
    };
    let p = PathBuf::from(&path);
    ensure_parent(&p);
    fs::write(&p, data).map_err(e2s)?;
    Ok(vault::stat(&p))
}

#[tauri::command]
fn fs_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
fn fs_stat(path: String) -> Option<vault::Stat> {
    vault::stat(Path::new(&path))
}

#[tauri::command]
fn fs_list(path: String) -> Res<vault::DirList> {
    vault::list_dir(Path::new(&path)).map_err(e2s)
}

#[tauri::command]
fn fs_mkdir(path: String) -> Res<()> {
    fs::create_dir_all(&path).map_err(e2s)
}

#[tauri::command]
fn fs_remove(path: String) -> Res<()> {
    fs::remove_file(&path).map_err(e2s)
}

#[tauri::command]
fn fs_rmdir(path: String, recursive: bool) -> Res<()> {
    if recursive {
        fs::remove_dir_all(&path).map_err(e2s)
    } else {
        fs::remove_dir(&path).map_err(e2s)
    }
}

#[tauri::command]
fn fs_rename(from: String, to: String) -> Res<()> {
    let t = PathBuf::from(&to);
    ensure_parent(&t);
    fs::rename(&from, &t).map_err(e2s)
}

#[tauri::command]
fn fs_copy(from: String, to: String) -> Res<()> {
    vault::copy_recursive(Path::new(&from), Path::new(&to)).map_err(e2s)
}

#[tauri::command]
fn fs_trash(path: String) -> Res<()> {
    trash::delete(&path).map_err(e2s)
}

/// Obsidian's "local trash": move into `<vault>/.trash/`.
#[tauri::command]
fn fs_trash_local(vault: String, rel: String) -> Res<()> {
    let from = Path::new(&vault).join(&rel);
    let name = from.file_name().ok_or("bad path")?.to_string_lossy().into_owned();
    let dir = Path::new(&vault).join(".trash");
    fs::create_dir_all(&dir).map_err(e2s)?;
    let mut to = dir.join(&name);
    let mut n = 1;
    while to.exists() {
        to = dir.join(format!("{name} {n}"));
        n += 1;
    }
    fs::rename(from, to).map_err(e2s)
}

// ── shell & network ───────────────────────────────────────────────────────

#[tauri::command]
fn open_external(url: String) -> Res<()> {
    open::that_detached(url).map_err(e2s)
}

#[tauri::command]
fn show_in_folder(path: String) -> Res<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg("-R").arg(&path).spawn().map_err(e2s)?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn().map_err(e2s)?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let dir = Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        open::that_detached(dir).map_err(e2s)
    }
}

#[derive(Deserialize)]
struct HttpReq {
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
    body_b64: Option<String>,
}

#[derive(Serialize)]
struct HttpResp {
    status: u16,
    headers: HashMap<String, String>,
    body_b64: String,
}

/// `requestUrl` — no CORS, like Obsidian's desktop implementation.
#[tauri::command]
async fn http_request(req: HttpReq) -> Res<HttpResp> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build();
    let mut r = agent.request(req.method.as_deref().unwrap_or("GET"), &req.url);
    for (k, v) in req.headers.unwrap_or_default() {
        r = r.set(&k, &v);
    }
    let res = if let Some(b) = req.body_b64 {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b).map_err(e2s)?;
        r.send_bytes(&bytes)
    } else if let Some(b) = req.body {
        r.send_string(&b)
    } else {
        r.call()
    };
    let resp = match res {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(e2s(e)),
    };
    let status = resp.status();
    let mut headers = HashMap::new();
    for name in resp.headers_names() {
        if let Some(v) = resp.header(&name) {
            headers.insert(name.to_lowercase(), v.to_string());
        }
    }
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut resp.into_reader(), &mut buf).map_err(e2s)?;
    Ok(HttpResp { status, headers, body_b64: base64::engine::general_purpose::STANDARD.encode(buf) })
}

#[tauri::command]
fn app_info() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "home": dirs::home_dir().map(|p| p.to_string_lossy().into_owned()),
        "configDir": config_path().parent().map(|p| p.to_string_lossy().into_owned()),
    })
}

// ── debugging (only when ATELIET_DEBUG=<dir> is set) ─────────────────────

fn debug_dir() -> Option<PathBuf> {
    std::env::var("ATELIET_DEBUG").ok().map(PathBuf::from)
}

#[tauri::command]
fn debug_enabled() -> bool {
    debug_dir().is_some()
}

#[tauri::command]
fn debug_log(level: String, msg: String) {
    if let Some(d) = debug_dir() {
        use std::io::Write;
        let _ = fs::create_dir_all(&d);
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(d.join("console.log")) {
            let _ = writeln!(f, "[{level}] {msg}");
        }
    }
}

/// Takes `<debug dir>/eval.js` (if present) so the UI can evaluate it.
#[tauri::command]
fn debug_take_eval() -> Option<String> {
    let p = debug_dir()?.join("eval.js");
    let s = fs::read_to_string(&p).ok()?;
    let _ = fs::remove_file(&p);
    Some(s)
}

/// PNG snapshot of the webview (debug builds of the UI tests use it).
#[cfg(target_os = "macos")]
#[tauri::command]
fn debug_snapshot(window: tauri::WebviewWindow, path: String) -> Res<()> {
    if debug_dir().is_none() {
        return Err("debug mode is off".into());
    }
    window
        .with_webview(move |wv| unsafe {
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            let webview = wv.inner() as *mut AnyObject;
            let block = block2::RcBlock::new(move |image: *mut AnyObject, _err: *mut AnyObject| {
                if image.is_null() {
                    return;
                }
                let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
                let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
                let dict: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
                let png: *mut AnyObject = msg_send![rep, representationUsingType: 4usize, properties: dict];
                let len: usize = msg_send![png, length];
                let bytes: *const u8 = msg_send![png, bytes];
                let _ = std::fs::write(&path, std::slice::from_raw_parts(bytes, len));
            });
            let _: () = msg_send![webview, takeSnapshotWithConfiguration: std::ptr::null_mut::<AnyObject>(), completionHandler: &*block];
        })
        .map_err(e2s)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn debug_snapshot(_path: String) -> Res<()> {
    Err("not supported on this platform".into())
}

// ── ateliet:// protocol ───────────────────────────────────────────────────

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "webm" => "video/webm",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "html" | "htm" => "text/html",
        "md" | "txt" => "text/plain; charset=utf-8",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn respond(status: StatusCode, ctype: &str, body: Vec<u8>) -> HttpResponse<Vec<u8>> {
    HttpResponse::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap()
}

fn query_param(q: &str, key: &str) -> Option<String> {
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_encoding::percent_decode_str(&v.replace('+', " ")).decode_utf8_lossy().into_owned())
    })
}

/// Synchronous fs bridge for the Node `fs` shim (used through sync XHR).
fn sync_fs(op: &str, q: &str, body: &[u8]) -> HttpResponse<Vec<u8>> {
    let path = query_param(q, "path").unwrap_or_default();
    let json_ok = |v: Value| respond(StatusCode::OK, "application/json", v.to_string().into_bytes());
    let err = |e: String| respond(StatusCode::NOT_FOUND, "text/plain", e.into_bytes());
    match op {
        "read" => match fs::read(&path) {
            Ok(b) => respond(StatusCode::OK, "application/octet-stream", b),
            Err(e) => err(e.to_string()),
        },
        "stat" => match vault::stat(Path::new(&path)) {
            Some(s) => json_ok(serde_json::to_value(s).unwrap()),
            None => err("ENOENT".into()),
        },
        "list" => match vault::list_dir(Path::new(&path)) {
            Ok(l) => json_ok(serde_json::to_value(l).unwrap()),
            Err(e) => err(e.to_string()),
        },
        "write" => {
            let p = PathBuf::from(&path);
            ensure_parent(&p);
            match fs::write(&p, body) {
                Ok(_) => json_ok(json!(true)),
                Err(e) => err(e.to_string()),
            }
        }
        "mkdir" => match fs::create_dir_all(&path) {
            Ok(_) => json_ok(json!(true)),
            Err(e) => err(e.to_string()),
        },
        "remove" => {
            let p = Path::new(&path);
            let r = if p.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
            match r {
                Ok(_) => json_ok(json!(true)),
                Err(e) => err(e.to_string()),
            }
        }
        "exec" => {
            let cmd = query_param(q, "cmd").unwrap_or_default();
            let cwd = query_param(q, "cwd");
            json_ok(process::exec_sync(&cmd, cwd.as_deref(), body))
        }
        "rename" => match fs::rename(&path, query_param(q, "to").unwrap_or_default()) {
            Ok(_) => json_ok(json!(true)),
            Err(e) => err(e.to_string()),
        },
        _ => err(format!("unknown op {op}")),
    }
}

fn serve(req: HttpRequest<Vec<u8>>) -> HttpResponse<Vec<u8>> {
    let uri = req.uri();
    let raw_path = uri.path();
    let q = uri.query().unwrap_or("");
    if let Some(op) = raw_path.strip_prefix("/__fs/") {
        return sync_fs(op, q, req.body());
    }
    if raw_path == "/__thumb" {
        let path = query_param(q, "path").unwrap_or_default();
        let width = query_param(q, "w").and_then(|w| w.parse().ok()).unwrap_or(480);
        return match gallery::serve_thumbnail(&path, width) {
            Some((bytes, ctype)) => HttpResponse::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, ctype)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
                .body(bytes)
                .unwrap(),
            None => respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
        };
    }
    let path = percent_encoding::percent_decode_str(raw_path).decode_utf8_lossy().into_owned();
    // Windows: `/C:/…` → `C:/…`
    #[cfg(windows)]
    let path = path.trim_start_matches('/').to_string();
    let ctype = mime_for(&path);
    let Ok(meta) = fs::metadata(&path) else {
        return respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec());
    };
    let len = meta.len();
    // Range requests, needed for audio/video seeking.
    if let Some(range) = req.headers().get(header::RANGE).and_then(|v| v.to_str().ok()) {
        if let Some(spec) = range.strip_prefix("bytes=") {
            let (s, e) = spec.split_once('-').unwrap_or((spec, ""));
            let start: u64 = s.parse().unwrap_or(0);
            let end: u64 = e.parse().unwrap_or_else(|_| (start + 4 * 1024 * 1024).min(len) - 1).min(len.saturating_sub(1));
            if start <= end {
                use std::io::{Read, Seek, SeekFrom};
                let mut f = match fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => return respond(StatusCode::NOT_FOUND, "text/plain", vec![]),
                };
                let mut buf = vec![0u8; (end - start + 1) as usize];
                let _ = f.seek(SeekFrom::Start(start));
                let _ = f.read_exact(&mut buf);
                return HttpResponse::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_TYPE, ctype)
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                    .body(buf)
                    .unwrap();
            }
        }
    }
    match fs::read(&path) {
        Ok(b) => HttpResponse::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ctype)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(b)
            .unwrap(),
        Err(_) => respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(process::Procs::default())
        .manage(gallery::Galleries::default())
        .manage(std::sync::Arc::new(pinterest::Downloads::default()))
        .register_asynchronous_uri_scheme_protocol("ateliet", |_ctx, req, responder| {
            std::thread::spawn(move || responder.respond(serve(req)));
        })
        // Pictogrep plugins: their own origin, so a plugin can reach nothing
        // but its files, the SDK and the pictures handed to it.
        .register_asynchronous_uri_scheme_protocol("pgplugin", |_ctx, req, responder| {
            std::thread::spawn(move || responder.respond(shared::serve_plugin(req)));
        })
        .invoke_handler(tauri::generate_handler![
            known_vaults,
            forget_vault,
            pick_folder,
            create_vault,
            open_vault,
            open_launcher,
            vault_init,
            vault_metadata_all,
            md_parse,
            md_render,
            search_vault,
            fs_read_text,
            fs_read_bytes,
            fs_write_text,
            fs_write_bytes,
            fs_append,
            fs_exists,
            fs_stat,
            fs_list,
            fs_mkdir,
            fs_remove,
            fs_rmdir,
            fs_rename,
            fs_copy,
            fs_trash,
            fs_trash_local,
            open_external,
            show_in_folder,
            http_request,
            app_info,
            debug_enabled,
            debug_log,
            debug_take_eval,
            debug_snapshot,
            process::proc_spawn,
            process::proc_write,
            process::proc_close_stdin,
            process::proc_kill,
            update::update_check,
            update::update_apply,
            update::update_restart,
            gallery::images_open,
            gallery::images_configure,
            gallery::images_refresh,
            gallery::images_rescan,
            gallery::images_status,
            gallery::images_model_download,
            gallery::images_reindex,
            gallery::images_query,
            gallery::images_folders,
            shared::shared_info,
            shared::shared_collection,
            shared::shared_import_bytes,
            shared::plugin_list,
            shared::plugin_call,
            shared::pinterest_start,
            shared::pinterest_status,
            shared::pinterest_cancel,
            shared::pinterest_followed,
            shared::pinterest_forget,
            shared::pinterest_check,
        ])
        .setup(|app| {
            // `ateliet <vault-path>` opens that vault; otherwise reopen the last one.
            let arg = std::env::args().nth(1).filter(|a| Path::new(a).is_dir());
            let last = load_config()["last"].as_str().map(|s| s.to_string()).filter(|p| Path::new(p).is_dir());
            let target = arg.map(|a| fs::canonicalize(&a).map(|p| p.to_string_lossy().into_owned()).unwrap_or(a)).or(last);
            if let Some(t) = &target {
                remember_vault(t);
            }
            open_window(app.handle(), target.as_deref()).map_err(Box::<dyn std::error::Error>::from)?;
            shared::start_sync_loop(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.try_state::<AppState>() {
                    state.watchers.lock().unwrap().remove(window.label());
                }
                if let Some(galleries) = window.try_state::<gallery::Galleries>() {
                    galleries.close(window.label());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Ateliet");
}
