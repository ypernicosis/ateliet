//! Commands for the built-in tools that replaced community plugins:
//! Dataview (queries, inline queries, dataviewjs), Templater and the calendar.
//! One page index per open vault, built in the background on first use and
//! kept current by the vault watcher.

use ateliet_core::calendar::{self, MonthRequest};
use ateliet_core::dataview::query;
use ateliet_core::dataview::{Index, Settings};
use ateliet_core::script::{self, Host, TemplateRun};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{Emitter, State};

type Res<T> = Result<T, String>;

type Gate = Arc<(Mutex<bool>, std::sync::Condvar)>;

#[derive(Default)]
pub struct Tools {
    indexes: Mutex<HashMap<PathBuf, Arc<RwLock<Index>>>>,
    /// Changes seen while an index is being built, applied when it is ready.
    pending: Mutex<HashMap<PathBuf, Vec<String>>>,
    building: Mutex<HashMap<PathBuf, Gate>>,
    prompts: Mutex<HashMap<u64, mpsc::Sender<Option<Value>>>>,
    next_prompt: AtomicU64,
}

impl Tools {
    /// The index of a vault, building it (once, even when asked concurrently) if needed.
    fn index(&self, root: &Path) -> Arc<RwLock<Index>> {
        if let Some(ix) = self.indexes.lock().unwrap().get(root) {
            return ix.clone();
        }
        let (gate, builder) = {
            let mut b = self.building.lock().unwrap();
            match b.get(root) {
                Some(g) => (g.clone(), false),
                None => {
                    let g = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
                    b.insert(root.to_path_buf(), g.clone());
                    (g, true)
                }
            }
        };
        if builder {
            let started = std::time::Instant::now();
            let mut ix = Index::build(root);
            if let Some(changed) = self.pending.lock().unwrap().remove(root) {
                ix.update(&changed);
            }
            eprintln!("[ateliet] tools: indexed {} notes in {} ms", ix.len(), started.elapsed().as_millis());
            let ix = Arc::new(RwLock::new(ix));
            self.indexes.lock().unwrap().insert(root.to_path_buf(), ix.clone());
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
            self.building.lock().unwrap().remove(root);
            return ix;
        }
        let mut done = gate.0.lock().unwrap();
        while !*done {
            done = gate.1.wait(done).unwrap();
        }
        drop(done);
        self.indexes.lock().unwrap().get(root).cloned().unwrap_or_else(|| self.index(root))
    }

    /// Start indexing a vault in the background (called when a vault opens).
    pub fn prewarm(self: &Arc<Self>, root: PathBuf) {
        let me = self.clone();
        std::thread::spawn(move || {
            // let the window finish booting first
            std::thread::sleep(Duration::from_secs(3));
            me.index(&root);
        });
    }

    /// Vault watcher hook: re-read changed paths.
    pub fn touched(&self, root: &Path, rels: &[String]) {
        let ix = self.indexes.lock().unwrap().get(root).cloned();
        match ix {
            Some(ix) => ix.write().unwrap_or_else(|e| e.into_inner()).update(rels),
            None => {
                if self.building.lock().unwrap().contains_key(root) {
                    self.pending.lock().unwrap().entry(root.to_path_buf()).or_default().extend(rels.iter().cloned());
                }
            }
        }
    }
}

pub type Shared = Arc<Tools>;

/// Display settings sent by the interface (from the Dataview settings).
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DvSettings {
    date_format: Option<String>,
    date_time_format: Option<String>,
    render_null_as: Option<String>,
    table_id_column_name: Option<String>,
    table_group_column_name: Option<String>,
    locale: Option<String>,
}

impl DvSettings {
    fn settings(&self) -> Settings {
        let d = Settings::default();
        let pick = |v: &Option<String>, fallback: String| v.clone().filter(|s| !s.is_empty()).unwrap_or(fallback);
        Settings {
            date_format: pick(&self.date_format, d.date_format),
            datetime_format: pick(&self.date_time_format, d.datetime_format),
            null_as: self.render_null_as.clone().unwrap_or(d.null_as),
            id_column: pick(&self.table_id_column_name, d.id_column),
            group_column: pick(&self.table_group_column_name, d.group_column),
            locale: pick(&self.locale, d.locale),
            show_count: true,
        }
    }
}

fn host_for(tools: &Tools, root: &Path, settings: Settings, window: Option<tauri::Window>, prompts: Option<Arc<Tools>>) -> Host {
    let log_window = window.clone();
    let prompt: Option<script::PromptFn> = match (window, prompts) {
        (Some(w), Some(t)) => Some(Arc::new(move |req: Value| {
            let id = t.next_prompt.fetch_add(1, Ordering::SeqCst) + 1;
            let (tx, rx) = mpsc::channel();
            t.prompts.lock().unwrap().insert(id, tx);
            let mut payload = req;
            payload["id"] = json!(id);
            if w.emit_to(w.label(), "tp-prompt", payload).is_err() {
                t.prompts.lock().unwrap().remove(&id);
                return None;
            }
            let answer = rx.recv_timeout(Duration::from_secs(3600)).ok().flatten();
            t.prompts.lock().unwrap().remove(&id);
            answer
        })),
        _ => None,
    };
    let log: script::LogFn = Arc::new(move |level: &str, msg: &str| {
        eprintln!("[script {level}] {msg}");
        if let Some(w) = &log_window {
            let _ = w.emit_to(w.label(), "script-log", json!({ "level": level, "message": msg }));
        }
    });
    Host { index: tools.index(root), settings, root: root.to_path_buf(), prompt, log: Some(log), timeout_secs: 30 }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

fn vault_root(vault: &str) -> Res<PathBuf> {
    let p = PathBuf::from(vault);
    if p.is_dir() {
        Ok(p)
    } else {
        Err(format!("Vault not found: {vault}"))
    }
}

#[tauri::command]
pub async fn dv_query(tools: State<'_, Shared>, vault: String, origin: String, source: String, settings: DvSettings) -> Res<Value> {
    let root = vault_root(&vault)?;
    let t = tools.inner().clone();
    blocking(move || {
        let ix = t.index(&root);
        let ix = ix.read().unwrap_or_else(|e| e.into_inner());
        query::run(&ix, &settings.settings(), &origin, &source)
    })
    .await
}

#[tauri::command]
pub async fn dv_inline(tools: State<'_, Shared>, vault: String, origin: String, expr: String, settings: DvSettings) -> Res<Value> {
    let root = vault_root(&vault)?;
    let t = tools.inner().clone();
    blocking(move || {
        let ix = t.index(&root);
        let ix = ix.read().unwrap_or_else(|e| e.into_inner());
        query::inline(&ix, &settings.settings(), &origin, &expr)
    })
    .await
}

#[tauri::command]
pub async fn dv_js(window: tauri::Window, tools: State<'_, Shared>, vault: String, origin: String, code: String, inline: bool, settings: DvSettings) -> Res<Value> {
    let root = vault_root(&vault)?;
    let t = tools.inner().clone();
    blocking(move || {
        let host = host_for(&t, &root, settings.settings(), Some(window), None);
        if inline {
            script::run_inline_js(&host, &code, &origin)
        } else {
            script::run_dataviewjs(&host, &code, &origin)
        }
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateArgs {
    vault: String,
    template: String,
    /// The template text when it does not come from a file (replace-in-file mode).
    template_text: Option<String>,
    target: String,
    /// The target's current content (the editor may hold unsaved text).
    content: Option<String>,
    selection: Option<String>,
    run_mode: u32,
    locale: Option<String>,
    week_start: Option<u32>,
}

#[tauri::command]
pub async fn tp_run(window: tauri::Window, tools: State<'_, Shared>, args: TemplateArgs) -> Res<Value> {
    let root = vault_root(&args.vault)?;
    let t = tools.inner().clone();
    blocking(move || -> Res<Value> {
        let template_text = match args.template_text {
            Some(text) => text,
            None => {
                let rel = args.template.trim_start_matches('/');
                if rel.split('/').any(|s| s == "..") {
                    return Err("invalid template path".into());
                }
                std::fs::read_to_string(root.join(rel)).map_err(|e| format!("cannot read template {rel}: {e}"))?
            }
        };
        let content = args.content.unwrap_or_else(|| std::fs::read_to_string(root.join(&args.target)).unwrap_or_default());
        let settings = Settings { locale: args.locale.clone().unwrap_or_else(|| "en".into()), ..Settings::default() };
        let mut host = host_for(&t, &root, settings, Some(window), Some(t.clone()));
        host.timeout_secs = 60;
        let run = TemplateRun {
            template_path: args.template,
            template_text,
            target_path: args.target,
            target_content: content,
            selection: args.selection.unwrap_or_default(),
            run_mode: args.run_mode,
            week_start: args.week_start.unwrap_or(0),
        };
        let out = script::run_template(&host, &run)?;
        Ok(json!({ "text": out.text, "cursors": out.cursors, "ops": out.ops, "cancelled": out.cancelled }))
    })
    .await?
}

#[tauri::command]
pub fn tp_answer(tools: State<'_, Shared>, id: u64, value: Option<Value>) {
    if let Some(tx) = tools.prompts.lock().unwrap().remove(&id) {
        let _ = tx.send(value.filter(|v| !v.is_null()));
    }
}

#[tauri::command]
pub async fn calendar_month(tools: State<'_, Shared>, vault: String, request: MonthRequest) -> Res<Value> {
    let root = vault_root(&vault)?;
    let t = tools.inner().clone();
    blocking(move || {
        let ix = t.index(&root);
        let ix = ix.read().unwrap_or_else(|e| e.into_inner());
        calendar::month(&root, &ix, &request)
    })
    .await
}
