//! Commands for the shared picture library: collections, importing, the
//! Pictogrep plugins (served on their own `pgplugin://` origin and called
//! through a permission-checked broker), and Pinterest downloads.

use crate::gallery::{Galleries, Gallery};
use crate::pictogrep::{self, Home, ImportBatch, Imported};
use crate::pinterest::{self, Dest, Downloads, Followed, Plan};
use ateliet_core::images::{ImageEntry, Origin};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::http::{header, Request as HttpRequest, Response as HttpResponse, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};

type Res<T> = Result<T, String>;

// ── process-wide state ────────────────────────────────────────────────────

fn home_slot() -> &'static Mutex<Option<Home>> {
    static SLOT: OnceLock<Mutex<Option<Home>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// The shared library the plugin protocol serves from (the most recently
/// opened window's choice; it is a machine-wide folder anyway).
pub fn set_home(home: Option<Home>) {
    *home_slot().lock().unwrap() = home;
}

fn home() -> Option<Home> {
    home_slot().lock().unwrap().clone()
}

/// Per-launch secret for plugin media URLs.
fn media_key() -> &'static [u8; 32] {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    KEY.get_or_init(|| {
        let mut h = Sha256::new();
        h.update(format!("{:?}{}{:p}", std::time::SystemTime::now(), std::process::id(), &h).as_bytes());
        if let Ok(mut f) = fs::File::open("/dev/urandom") {
            use std::io::Read;
            let mut b = [0u8; 32];
            if f.read_exact(&mut b).is_ok() {
                h.update(b);
            }
        }
        h.finalize().into()
    })
}

fn media_sig(path: &str) -> String {
    let mut h = Sha256::new();
    h.update(media_key());
    h.update(path.as_bytes());
    h.finalize()[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn shared_of(g: &Gallery) -> Res<Home> {
    g.lib.lock().unwrap().shared.clone().ok_or_else(|| "no shared picture library is in use (Settings → Images)".to_string())
}

// ── collections and importing ─────────────────────────────────────────────

#[tauri::command]
pub fn shared_info(window: tauri::Window, state: State<Galleries>) -> Res<Value> {
    let g = state.get(window.label())?;
    let lib = g.lib.lock().unwrap();
    let Some(home) = lib.shared.clone() else { return Ok(json!({ "home": null })) };
    let shared: Vec<&ImageEntry> = lib.entries.iter().filter(|e| e.origin == Origin::Shared).collect();
    let by_path: HashMap<String, &ImageEntry> = shared.iter().map(|e| (pictogrep::clean(Path::new(&e.abs)).to_string_lossy().into_owned(), *e)).collect();
    let mut sorted = HashSet::new();
    let collections: Vec<Value> = pictogrep::collection_names(&home)
        .into_iter()
        .map(|name| {
            let members: Vec<&ImageEntry> = pictogrep::collection_images(&home, &name).iter().filter_map(|p| by_path.get(&*p.to_string_lossy()).copied()).collect();
            for m in &members {
                sorted.insert(m.key.clone());
            }
            let cover = members.iter().max_by_key(|e| e.mtime).map(|e| e.abs.clone()).unwrap_or_default();
            json!({ "name": name, "count": members.len(), "cover": cover })
        })
        .collect();
    let unsorted = shared.iter().filter(|e| !sorted.contains(&e.key)).count();
    let plugins: Vec<Value> = pictogrep::plugins(&home).into_iter().map(|p| serde_json::to_value(p).unwrap_or(Value::Null)).collect();
    Ok(json!({
        "home": home.root, "library": home.library(), "running": pictogrep::is_running(&home),
        "total": shared.len(), "unsorted": unsorted, "collections": collections, "plugins": plugins,
        "galleryDl": pinterest::gallery_dl().map(|p| p.to_string_lossy().into_owned()),
    }))
}

#[derive(Deserialize)]
pub struct CollectionAction {
    action: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    into: String,
    #[serde(default)]
    paths: Vec<String>,
}

/// create / rename / delete a collection; add / remove pictures (absolute
/// paths). Pictures from outside the shared library are copied into it first.
#[tauri::command]
pub fn shared_collection(window: tauri::Window, state: State<Galleries>, request: CollectionAction) -> Res<Value> {
    let g = state.get(window.label())?;
    let home = shared_of(&g)?;
    let out = match request.action.as_str() {
        "create" => json!({ "name": pictogrep::create_collection(&home, &request.name)? }),
        "rename" => json!({ "name": pictogrep::rename_collection(&home, &request.name, &request.into)? }),
        "delete" => {
            pictogrep::delete_collection(&home, &request.name)?;
            json!({})
        }
        "add" => {
            let roots = pictogrep::source_roots(&home);
            let mut batch: Option<ImportBatch> = None;
            let mut added = 0;
            for p in &request.paths {
                let mut path = pictogrep::clean(Path::new(p));
                if !roots.iter().any(|r| path.starts_with(r)) {
                    let b = batch.get_or_insert_with(|| new_batch(&g, &home));
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    path = match pictogrep::import_file(&path, &name, &home.library(), b)? {
                        Imported::New(p) | Imported::Duplicate(p) => p,
                    };
                }
                if pictogrep::link(&home, &request.name, &path)? {
                    added += 1;
                }
            }
            if let Some(b) = batch {
                finish_import(&g, &home, b.added);
            }
            json!({ "added": added })
        }
        "remove" => {
            let removed = request.paths.iter().filter(|p| pictogrep::unlink(&home, &request.name, Path::new(p))).count();
            json!({ "removed": removed })
        }
        // A picture about to be deleted leaves every collection, like
        // Pictogrep does, so no broken entries stay behind.
        "forget" => {
            let names = pictogrep::collection_names(&home);
            let removed = request.paths.iter().map(|p| names.iter().filter(|n| pictogrep::unlink(&home, n, Path::new(p))).count()).sum::<usize>();
            json!({ "removed": removed })
        }
        other => return Err(format!("unknown collection action {other}")),
    };
    g.emit_changed();
    Ok(out)
}

/// The duplicate check for imports into the shared library, using digests
/// the image index already knows.
fn new_batch(g: &Gallery, home: &Home) -> ImportBatch {
    let (paths, digests) = shared_digests(g, home);
    ImportBatch::new(&paths, &digests)
}

fn shared_digests(g: &Gallery, home: &Home) -> (Vec<PathBuf>, HashMap<PathBuf, [u8; 32]>) {
    let lib = g.lib.lock().unwrap();
    let roots = pictogrep::source_roots(home);
    let mut paths = Vec::new();
    let mut digests = HashMap::new();
    for e in lib.entries.iter().filter(|e| e.origin == Origin::Shared) {
        let p = pictogrep::clean(Path::new(&e.abs));
        if !roots.iter().any(|r| p.starts_with(r)) {
            continue;
        }
        if let Some(d) = lib.index.records.get(&e.key).and_then(|r| r.digest) {
            digests.insert(p.clone(), d);
        }
        paths.push(p);
    }
    (paths, digests)
}

fn finish_import(g: &Gallery, home: &Home, added: Vec<PathBuf>) {
    pictogrep::library_changed(home, &added);
    if !added.is_empty() {
        g.refresh(added);
    }
}

/// A picture dropped or pasted into the shared library (raw bytes; headers
/// x-name and, optionally, x-collection).
#[tauri::command]
pub fn shared_import_bytes(window: tauri::Window, state: State<Galleries>, request: tauri::ipc::Request<'_>) -> Res<Value> {
    let g = state.get(window.label())?;
    let home = shared_of(&g)?;
    let header = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()).unwrap_or_default();
    let (name, collection) = (header("x-name"), header("x-collection"));
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else { return Err("expected raw body".into()) };
    let tmp = std::env::temp_dir().join(format!("ateliet-import-{}-{}", std::process::id(), data.len()));
    fs::write(&tmp, data).map_err(|e| e.to_string())?;
    let mut batch = new_batch(&g, &home);
    let result = pictogrep::import_file(&tmp, &name, &home.library(), &mut batch);
    let _ = fs::remove_file(&tmp);
    let (path, duplicate) = match result? {
        Imported::New(p) => (p, false),
        Imported::Duplicate(p) => (p, true),
    };
    if !collection.is_empty() {
        pictogrep::link(&home, &collection, &path)?;
    }
    finish_import(&g, &home, batch.added);
    g.emit_changed();
    Ok(json!({ "path": path, "duplicate": duplicate }))
}

// ── plugins ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn plugin_list(window: tauri::Window, state: State<Galleries>) -> Res<Vec<pictogrep::Plugin>> {
    let g = state.get(window.label())?;
    Ok(pictogrep::plugins(&shared_of(&g)?))
}

fn plugin_base(window: &tauri::Window) -> String {
    let _ = window;
    if cfg!(windows) { "http://pgplugin.localhost".into() } else { "pgplugin://localhost".into() }
}

/// An image record in the shape Pictogrep's API gives plugins.
fn record(e: &ImageEntry, tags: &HashMap<String, Vec<String>>, base: &str, score: Option<f32>) -> Value {
    let path = pictogrep::clean(Path::new(&e.abs)).to_string_lossy().into_owned();
    let sig = media_sig(&path);
    let enc = percent_encoding::utf8_percent_encode(&path, percent_encoding::NON_ALPHANUMERIC).to_string();
    let mut r = json!({
        "id": pictogrep::image_id(Path::new(&path)), "name": e.name, "path": path, "mtime": e.mtime / 1000,
        "url": format!("{base}/media?p={enc}&s={sig}&original=1"),
        "thumbnailUrl": format!("{base}/media?p={enc}&s={sig}&size=960"),
    });
    if let Some(t) = tags.get(&path).filter(|t| !t.is_empty()) {
        r["tags"] = json!(t);
    }
    if e.width > 0 {
        r["width"] = json!(e.width);
        r["height"] = json!(e.height);
    }
    if let Some(s) = score {
        r["score"] = json!(s);
    }
    r
}

fn tag_map(home: Option<&Home>) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(h) = home {
        for name in pictogrep::collection_names(h) {
            for p in pictogrep::collection_images(h, &name) {
                out.entry(p.to_string_lossy().into_owned()).or_default().push(name.clone());
            }
        }
    }
    out
}

fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
}

/// One plugin call, checked against the plugin's declared permissions here
/// (the iframe is sandboxed and can only reach Ateliet through this).
#[tauri::command]
pub async fn plugin_call(window: tauri::Window, state: State<'_, Galleries>, id: String, method: String, args: Value) -> Res<Value> {
    let g = state.get(window.label())?;
    let base = plugin_base(&window);
    tauri::async_runtime::spawn_blocking(move || call(&g, &base, &id, &method, &args)).await.map_err(|e| e.to_string())?
}

fn call(g: &Gallery, base: &str, id: &str, method: &str, args: &Value) -> Res<Value> {
    let home = shared_of(g)?;
    let plugin = pictogrep::plugins(&home).into_iter().find(|p| p.id == id).ok_or_else(|| format!("no plugin named {id} is installed"))?;
    let permission = if method.starts_with("storage.kv") { "storage.kv" } else { method };
    if !plugin.permissions.iter().any(|p| p == permission) {
        return Err(format!("plugin {id} did not declare {permission}"));
    }
    // Plugins work on the shared library, as they do inside Pictogrep, so the
    // collections they fill stay meaningful in both apps.
    let by_id = |lib: &crate::gallery::Library, wanted: &str| -> Option<ImageEntry> { lib.entries.iter().find(|e| e.origin == Origin::Shared && pictogrep::image_id(Path::new(&e.abs)) == wanted).cloned() };
    match method {
        "images.list" => {
            let tags = tag_map(Some(&home));
            let lib = g.lib.lock().unwrap();
            let s = |k: &str| args[k].as_str().map(String::from).or_else(|| args[k].as_i64().map(|n| n.to_string())).unwrap_or_default();
            let mut list: Vec<&ImageEntry> = lib.entries.iter().filter(|e| e.origin == Origin::Shared).collect();
            let tag = s("tag");
            if !tag.is_empty() {
                let name = pictogrep::collection_name(&tag)?;
                let members: HashSet<String> = pictogrep::collection_images(&home, &name).into_iter().map(|p| p.to_string_lossy().into_owned()).collect();
                list.retain(|e| members.contains(&*pictogrep::clean(Path::new(&e.abs)).to_string_lossy()));
            }
            let source = s("source");
            if !source.is_empty() {
                let root = pictogrep::clean(Path::new(&source));
                list.retain(|e| Path::new(&e.abs).starts_with(&root));
            }
            if s("mode") == "unsorted" {
                list.retain(|e| tags.get(&*pictogrep::clean(Path::new(&e.abs)).to_string_lossy()).is_none());
            }
            let mut seed = s("seed").parse::<u64>().unwrap_or(0);
            if seed == 0 || seed >= 1 << 48 {
                seed = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1) as u64 % ((1 << 48) - 1)) + 1;
            }
            match s("mode").as_str() {
                "recent" => list.sort_by(|a, b| b.mtime.cmp(&a.mtime)),
                "random" => {
                    let mut idx: Vec<usize> = (0..list.len()).collect();
                    ateliet_core::images::shuffle(&mut idx, seed);
                    list = idx.into_iter().map(|i| list[i]).collect();
                }
                _ => {}
            }
            let total = list.len();
            let offset = s("offset").parse::<usize>().unwrap_or(0).min(total);
            let count = s("count").parse::<usize>().ok().filter(|c| *c >= 1).unwrap_or(120).min(500);
            let images: Vec<Value> = list[offset..(offset + count).min(total)].iter().map(|e| record(e, &tags, base, None)).collect();
            Ok(json!({ "ok": true, "images": images, "total": total, "offset": offset, "seed": seed }))
        }
        "images.search" => {
            let query = args["query"].as_str().unwrap_or("").trim().to_string();
            let tags = tag_map(Some(&home));
            // By description when the image model is ready, else by name,
            // which is what Pictogrep's plugin API answers.
            let vector = g.model.get().and_then(|c| g.model.text_vector(&c, &query).ok()).filter(|_| !query.is_empty());
            let lib = g.lib.lock().unwrap();
            let all: Vec<usize> = (0..lib.entries.len()).filter(|&i| lib.entries[i].origin == Origin::Shared).collect();
            let words = ateliet_core::images::query_words(&query);
            let (ai, ranked) = match &vector {
                Some(v) => (true, ateliet_core::images::rank(&lib.entries, &lib.index, &all, v, &words)),
                None => (false, ateliet_core::images::rank_by_name(&lib.entries, &all, &words)),
            };
            let images: Vec<Value> = ranked.into_iter().take(80).map(|(i, s)| record(&lib.entries[i], &tags, base, Some(s))).collect();
            Ok(json!({ "ok": true, "images": images, "query": query, "ai": ai }))
        }
        "images.read" => {
            let tags = tag_map(Some(&home));
            let lib = g.lib.lock().unwrap();
            let e = by_id(&lib, args["id"].as_str().unwrap_or("")).ok_or("Pictogrep returned 404")?;
            Ok(json!({ "ok": true, "image": record(&e, &tags, base, None) }))
        }
        "images.tag" => {
            let e = by_id(&g.lib.lock().unwrap(), args["id"].as_str().unwrap_or("")).ok_or("unknown image")?;
            let name = pictogrep::collection_name(args["tag"].as_str().unwrap_or(""))?;
            let added = pictogrep::link(&home, &name, Path::new(&e.abs))?;
            g.emit_changed();
            Ok(json!({ "ok": true, "tag": name, "added": added }))
        }
        "images.reveal" => {
            let e = by_id(&g.lib.lock().unwrap(), args["imageId"].as_str().or(args["id"].as_str()).unwrap_or("")).ok_or("unknown image")?;
            reveal(Path::new(&e.abs));
            Ok(json!({ "ok": true, "path": e.abs }))
        }
        "storage.kv.get" => Ok(json!({ "ok": true, "value": pictogrep::storage_get(&home, id) })),
        "storage.kv.set" => {
            pictogrep::storage_set(&home, id, args["key"].as_str().unwrap_or(""), args["value"].clone())?;
            Ok(json!({ "ok": true }))
        }
        other => Err(format!("unknown capability: {other}")),
    }
}

const SDK: &str = include_str!("plugin-sdk.js");

fn reply(status: StatusCode, ctype: &str, body: Vec<u8>) -> HttpResponse<Vec<u8>> {
    HttpResponse::builder().status(status).header(header::CONTENT_TYPE, ctype).header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*").body(body).unwrap()
}

/// The `pgplugin://` protocol: plugin files (`/plugin/<id>/<path>`), the SDK
/// (`/assets/plugin-sdk.js`) and signed library media (`/media?p=&s=`).
/// Plugin documents get Pictogrep's CSP: no network, nothing outside this
/// origin.
pub fn serve_plugin(req: HttpRequest<Vec<u8>>) -> HttpResponse<Vec<u8>> {
    let uri = req.uri();
    let path = percent_encoding::percent_decode_str(uri.path()).decode_utf8_lossy().into_owned();
    let query = uri.query().unwrap_or("");
    let param = |k: &str| query.split('&').find_map(|kv| kv.split_once('=').filter(|(a, _)| *a == k).map(|(_, v)| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned()));
    if path == "/assets/plugin-sdk.js" {
        return reply(StatusCode::OK, "text/javascript", SDK.as_bytes().to_vec());
    }
    if path == "/media" {
        let (Some(p), Some(s)) = (param("p"), param("s")) else { return reply(StatusCode::FORBIDDEN, "text/plain", b"forbidden".to_vec()) };
        if media_sig(&p) != s {
            return reply(StatusCode::FORBIDDEN, "text/plain", b"invalid media token".to_vec());
        }
        if param("original").as_deref() == Some("1") {
            return match fs::read(&p) {
                Ok(b) => reply(StatusCode::OK, mime(&p), b),
                Err(_) => reply(StatusCode::NOT_FOUND, "text/plain", vec![]),
            };
        }
        let size = param("size").and_then(|s| s.parse().ok()).unwrap_or(960);
        return match crate::gallery::serve_thumbnail(&p, size) {
            Some((b, t)) => reply(StatusCode::OK, t, b),
            None => reply(StatusCode::NOT_FOUND, "text/plain", vec![]),
        };
    }
    let Some(rest) = path.strip_prefix("/plugin/") else { return reply(StatusCode::NOT_FOUND, "text/plain", vec![]) };
    let (id, file) = rest.split_once('/').unwrap_or((rest, ""));
    let Some(home) = home() else { return reply(StatusCode::NOT_FOUND, "text/plain", b"no shared library".to_vec()) };
    let Some(plugin) = pictogrep::plugins(&home).into_iter().find(|p| p.id == id) else { return reply(StatusCode::NOT_FOUND, "text/plain", format!("no plugin named {id} is installed").into_bytes()) };
    let rel = if file.is_empty() { plugin.entry.clone() } else { file.to_string() };
    let Some(target) = pictogrep::plugin_file(&plugin.dir, &rel) else { return reply(StatusCode::FORBIDDEN, "text/plain", b"path escapes the plugin directory".to_vec()) };
    match fs::read(&target) {
        Ok(body) => {
            let origin = if cfg!(windows) { "http://pgplugin.localhost" } else { "pgplugin://localhost pgplugin:" };
            HttpResponse::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime(&target.to_string_lossy()))
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .header(
                    "Content-Security-Policy",
                    format!("default-src 'none'; script-src 'self' 'unsafe-inline' {origin}; style-src 'self' 'unsafe-inline' {origin}; img-src 'self' data: blob: {origin}; font-src data:; connect-src 'none'"),
                )
                .body(body)
                .unwrap()
        }
        Err(_) => reply(StatusCode::NOT_FOUND, "text/plain", format!("{rel} not found").into_bytes()),
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

// ── Pinterest ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinterestRequest {
    url: String,
    /// "new" (a collection named after the board), "existing", or "direct".
    #[serde(default)]
    mode: String,
    #[serde(default)]
    folder: String,
    /// Download the board's past pins (otherwise only the newest few).
    #[serde(default = "yes")]
    backfill: bool,
    #[serde(default)]
    follow: bool,
    #[serde(default)]
    skip_existing: bool,
    /// Vault folder for imports when no shared library is in use.
    #[serde(default)]
    vault_folder: String,
}

fn yes() -> bool {
    true
}

fn data_dir_for(g: &Gallery, home: &Option<Home>) -> PathBuf {
    match home {
        Some(h) => h.data(),
        None => g.vault.join(".obsidian").join("ateliet"),
    }
}

fn emit_job(app: &AppHandle, downloads: &Downloads) {
    let _ = app.emit("pinterest-progress", downloads.snapshot());
}

fn plan_for(g: &Gallery, home: &Option<Home>, url: &str, name: &str, collection: Option<String>, vault_dir: Option<PathBuf>, limit: usize, archive: Option<PathBuf>, link_duplicates: bool, allow_empty: bool) -> Plan {
    let (dest, existing, digests) = match (home, vault_dir) {
        (Some(h), _) => {
            let (paths, digests) = shared_digests(g, h);
            (Dest::Shared { home: h.clone(), collection }, paths, digests)
        }
        (None, dir) => {
            let dir = dir.unwrap_or_else(|| g.vault.join("Pinterest"));
            let lib = g.lib.lock().unwrap();
            let mut paths = Vec::new();
            let mut digests = HashMap::new();
            for e in lib.entries.iter().filter(|e| Path::new(&e.abs).starts_with(&dir)) {
                let p = PathBuf::from(&e.abs);
                if let Some(d) = lib.index.records.get(&e.key).and_then(|r| r.digest) {
                    digests.insert(p.clone(), d);
                }
                paths.push(p);
            }
            (Dest::Vault { dir }, paths, digests)
        }
    };
    Plan { source: url.into(), name: name.into(), dest, archive, limit, link_duplicates, allow_empty, existing, digests }
}

fn run_plan(app: AppHandle, g: Arc<Gallery>, downloads: Arc<Downloads>, plan: Plan, cancel: Arc<std::sync::atomic::AtomicBool>) {
    std::thread::spawn(move || {
        let report = {
            let (app, downloads) = (app.clone(), downloads.clone());
            let last = Mutex::new(std::time::Instant::now() - Duration::from_secs(1));
            move |phase: &str, done: usize, total: usize| {
                downloads.progress(phase, done, total);
                let mut l = last.lock().unwrap();
                if l.elapsed() > Duration::from_millis(300) || done == total {
                    *l = std::time::Instant::now();
                    emit_job(&app, &downloads);
                }
            }
        };
        let outcome = pinterest::run(&plan, &cancel, &report);
        match outcome {
            Ok(o) => {
                if !o.added.is_empty() {
                    g.refresh(o.added);
                }
                downloads.finish(Ok(o.result));
            }
            Err(e) => downloads.finish(Err(e)),
        }
        g.emit_changed();
        emit_job(&app, &downloads);
    });
}

#[tauri::command]
pub fn pinterest_start(app: AppHandle, window: tauri::Window, state: State<Galleries>, downloads: State<Arc<Downloads>>, request: PinterestRequest) -> Res<pinterest::Job> {
    let g = state.get(window.label())?;
    let url = pinterest::validate_url(&request.url)?;
    let source = url.to_string();
    let name = pinterest::source_name(&url);
    let home = g.lib.lock().unwrap().shared.clone();
    if !request.backfill && !request.follow {
        return Err("choose to download the board, to follow it, or both".into());
    }
    let mode = if request.mode.is_empty() { "new" } else { request.mode.as_str() };
    if mode == "direct" && request.follow {
        return Err("following a board needs a folder to save into".into());
    }
    let (collection, vault_dir) = match (&home, mode) {
        (Some(h), "existing") => {
            let n = pictogrep::collection_name(&request.folder)?;
            if !pictogrep::collection_names(h).contains(&n) {
                return Err("choose a collection you already have".into());
            }
            (Some(n), None)
        }
        (Some(_), "direct") => (None, None),
        (Some(_), _) => (Some(pictogrep::collection_name(&if request.folder.trim().is_empty() { name.replace('.', " ") } else { request.folder.replace('.', " ") }).map_err(|_| "that folder name cannot be used; pick another one".to_string())?), None),
        (None, m) => {
            let base = g.vault.join(if request.vault_folder.trim().is_empty() { "Pinterest" } else { request.vault_folder.trim() });
            let dir = match m {
                "direct" => base,
                "existing" => g.vault.join(request.folder.trim().trim_matches('/')),
                _ => base.join(if request.folder.trim().is_empty() { name.as_str() } else { request.folder.trim() }),
            };
            (None, Some(dir))
        }
    };
    let data = data_dir_for(&g, &home);
    let archive = Some(pinterest::archive_path(&data, &source));
    let cancel = downloads.start(&name, false).ok_or("a download is already running")?;
    if request.follow {
        let folder = collection.clone().or_else(|| vault_dir.as_ref().map(|d| d.strip_prefix(&g.vault).unwrap_or(d).to_string_lossy().replace('\\', "/"))).unwrap_or_default();
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let _ = pinterest::follow(&data, Followed { url: source.clone(), name: name.clone(), folder, last_sync_at: t, added_at: t });
    }
    let limit = if request.backfill { 0 } else { pinterest::FOLLOW_NEWEST };
    let plan = plan_for(&g, &home, &source, &name, collection, vault_dir, limit, archive, !request.skip_existing, false);
    run_plan(app.clone(), g, downloads.inner().clone(), plan, cancel);
    emit_job(&app, &downloads);
    Ok(downloads.snapshot())
}

#[tauri::command]
pub fn pinterest_status(downloads: State<Arc<Downloads>>) -> pinterest::Job {
    downloads.snapshot()
}

#[tauri::command]
pub fn pinterest_cancel(app: AppHandle, downloads: State<Arc<Downloads>>) -> Res<()> {
    if !downloads.stop() {
        return Err("nothing is downloading".into());
    }
    emit_job(&app, &downloads);
    Ok(())
}

/// Followed Pinterest boards (pages Pictogrep follows on other sites are
/// left to it).
#[tauri::command]
pub fn pinterest_followed(window: tauri::Window, state: State<Galleries>) -> Res<Value> {
    let g = state.get(window.label())?;
    let home = g.lib.lock().unwrap().shared.clone();
    let list: Vec<Followed> = pinterest::followed(&data_dir_for(&g, &home)).into_iter().filter(|f| pinterest::validate_url(&f.url).is_ok()).collect();
    Ok(json!({ "boards": list, "autoSync": home.as_ref().map(pinterest::auto_sync).unwrap_or(true), "galleryDl": pinterest::gallery_dl().is_some() }))
}

#[tauri::command]
pub fn pinterest_forget(window: tauri::Window, state: State<Galleries>, url: String) -> Res<()> {
    let g = state.get(window.label())?;
    let home = g.lib.lock().unwrap().shared.clone();
    pinterest::forget(&data_dir_for(&g, &home), &url)
}

/// Checks one followed board now (its newest pins).
#[tauri::command]
pub fn pinterest_check(app: AppHandle, window: tauri::Window, state: State<Galleries>, downloads: State<Arc<Downloads>>, url: String) -> Res<()> {
    let g = state.get(window.label())?;
    check_followed(&app, &g, downloads.inner().clone(), &url, false)
}

fn check_followed(app: &AppHandle, g: &Arc<Gallery>, downloads: Arc<Downloads>, url: &str, automatic: bool) -> Res<()> {
    let home = g.lib.lock().unwrap().shared.clone();
    let data = data_dir_for(g, &home);
    let f = pinterest::followed(&data).into_iter().find(|f| f.url == url).ok_or("that board is not followed")?;
    let source = pinterest::validate_url(&f.url)?.to_string();
    let (collection, vault_dir) = match &home {
        Some(_) => (pictogrep::collection_name(&f.folder.replace('.', " ")).ok(), None),
        None => (None, Some(g.vault.join(&f.folder))),
    };
    let cancel = downloads.start(&f.name, automatic).ok_or("a download is already running")?;
    pinterest::mark_synced(&data, &f.url);
    let plan = plan_for(g, &home, &source, &f.name, collection, vault_dir, pinterest::FOLLOW_NEWEST, Some(pinterest::archive_path(&data, &source)), true, true);
    run_plan(app.clone(), g.clone(), downloads, plan, cancel);
    Ok(())
}

/// Once an hour: check followed boards that are due, unless Pictogrep is open
/// (then it does it) or its automatic checks are switched off.
pub fn start_sync_loop(app: AppHandle) {
    let downloads: Arc<Downloads> = app.state::<Arc<Downloads>>().inner().clone();
    if downloads.sync_loop.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(120));
        loop {
            let galleries = app.state::<Galleries>();
            if let Some(g) = galleries.any() {
                let home = g.lib.lock().unwrap().shared.clone();
                let allowed = match &home {
                    Some(h) => pinterest::auto_sync(h) && !pictogrep::is_running(h),
                    None => true,
                };
                if allowed && downloads.snapshot().state != "running" {
                    let data = data_dir_for(&g, &home);
                    if let Some(f) = pinterest::followed(&data).into_iter().find(|f| pinterest::due(f) && pinterest::validate_url(&f.url).is_ok()) {
                        let _ = check_followed(&app, &g, downloads.clone(), &f.url, true);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}
