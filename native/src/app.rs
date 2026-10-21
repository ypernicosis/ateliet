//! The application: vault, tabs, sidebars, editor/reading view, commands.

use crate::config::{self, Appearance, VaultConfig};
use crate::daily::{self, DailySettings};
use crate::host::{Caches, RenderHost};
use crate::markdown::{self, Action, Doc, Renderer};
use crate::panels;
use crate::templater::{self, Templater, TemplaterSettings};
use crate::theme::{self, Palette};
use crate::vault::{self, Vault};
use crate::{commands, editor};
use ateliet_core::dataview::Settings;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Edit,
    Read,
}

pub struct Tab {
    pub path: String,
    pub text: String,
    pub saved: String,
    pub mode: Mode,
    pub doc: Option<(u64, Arc<Doc>)>,
    pub last_edit: Option<Instant>,
    /// Cursor (char index) last seen in the editor.
    pub cursor: usize,
    pub highlight: editor::HighlightCache,
    /// A heading line to scroll to on the next frame.
    pub scroll_to: Option<usize>,
}

impl Tab {
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn doc(&mut self) -> Arc<Doc> {
        let mut h = DefaultHasher::new();
        self.text.hash(&mut h);
        let k = h.finish();
        match &self.doc {
            Some((key, d)) if *key == k => d.clone(),
            _ => {
                let d = Arc::new(markdown::parse(&self.text));
                self.doc = Some((k, d.clone()));
                d
            }
        }
    }

    fn editor_id(&self) -> egui::Id {
        egui::Id::new(("editor", &self.path))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeftTab {
    Files,
    Search,
    Bookmarks,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RightTab {
    Backlinks,
    Outline,
    Tags,
    Calendar,
}

pub struct Toast {
    pub text: String,
    pub until: Instant,
}

/// A small dialog waiting for text (rename, new note…).
pub struct Ask {
    pub title: String,
    pub value: String,
    pub kind: AskKind,
}

#[derive(Clone, Debug)]
pub enum AskKind {
    Rename(String),
    NewNote(String),
    NewFolder(String),
    ConfirmDelete(String),
}

pub struct App {
    pub vault: Option<Vault>,
    pub cfg: Option<VaultConfig>,
    pub appearance: Appearance,
    pub palette: Palette,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub left_open: bool,
    pub right_open: bool,
    pub left_tab: LeftTab,
    pub right_tab: RightTab,
    pub caches: Caches,
    pub dv_settings: Settings,
    pub daily: DailySettings,
    pub templater_settings: TemplaterSettings,
    pub templater: Templater,
    pub search: panels::SearchState,
    pub calendar: panels::CalendarState,
    pub palette_ui: Option<commands::Palette>,
    pub toasts: Vec<Toast>,
    pub ask: Option<Ask>,
    pub settings_open: bool,
    pub readable: bool,
    pub default_mode: Mode,
    pub locale: String,
    pub hotkeys: Vec<(String, KeyboardShortcut)>,
    pub recent: Vec<String>,
    themed: bool,
    last_generation: u64,
}

fn system_locale() -> String {
    std::env::var("LC_ALL").or_else(|_| std::env::var("LC_MESSAGES")).or_else(|_| std::env::var("LANG")).ok().map(|l| l.split('.').next().unwrap_or("en").replace('_', "-").to_lowercase()).filter(|l| !l.is_empty() && l != "c" && l != "posix").unwrap_or_else(|| "en".into())
}

impl App {
    pub fn new(cc: &eframe::CreationContext, vault: Option<PathBuf>) -> App {
        crate::fonts::install(&cc.egui_ctx);
        let mut app = App {
            vault: None,
            cfg: None,
            appearance: Appearance { dark: false, accent: [0x8a, 0x5c, 0xf5], font_size: 16.0 },
            palette: Palette::new(&Appearance { dark: false, accent: [0x8a, 0x5c, 0xf5], font_size: 16.0 }),
            tabs: Vec::new(),
            active: 0,
            left_open: true,
            right_open: true,
            left_tab: LeftTab::Files,
            right_tab: RightTab::Calendar,
            caches: Caches::default(),
            dv_settings: Settings::default(),
            daily: DailySettings::default(),
            templater_settings: TemplaterSettings::default(),
            templater: Templater::default(),
            search: Default::default(),
            calendar: panels::CalendarState::new(),
            palette_ui: None,
            toasts: Vec::new(),
            ask: None,
            settings_open: false,
            readable: true,
            default_mode: Mode::Edit,
            locale: system_locale(),
            hotkeys: Vec::new(),
            recent: Vec::new(),
            themed: false,
            last_generation: 0,
        };
        if let Some(v) = vault {
            app.open_vault(&cc.egui_ctx, v);
        }
        app
    }

    pub fn open_vault(&mut self, ctx: &egui::Context, root: PathBuf) {
        self.save_all();
        config::remember_vault(&root);
        let cfg = VaultConfig::new(&root);
        self.appearance = Appearance::load(&cfg);
        self.daily = DailySettings::load(&cfg);
        self.templater_settings = TemplaterSettings::load(&cfg);
        let dv = cfg.tool("dataview", "dataview");
        let s = |k: &str, d: &str| dv[k].as_str().filter(|x| !x.is_empty()).unwrap_or(d).to_string();
        self.dv_settings = Settings {
            date_format: s("defaultDateFormat", "MMMM dd, yyyy"),
            datetime_format: s("defaultDateTimeFormat", "h:mm a - MMMM dd, yyyy"),
            locale: self.locale.clone(),
            null_as: s("renderNullAs", "\\-"),
            id_column: s("tableIdColumnName", "File"),
            group_column: s("tableGroupColumnName", "Group"),
            show_count: true,
        };
        let appj = cfg.read("app.json");
        self.readable = appj["readableLineLength"].as_bool().unwrap_or(true);
        self.default_mode = if appj["defaultViewMode"].as_str() == Some("preview") { Mode::Read } else { Mode::Edit };
        self.calendar.load(&cfg);
        self.hotkeys = commands::load_hotkeys(&cfg);
        let session = cfg.read("ateliet/native-session.json");
        self.tabs.clear();
        self.caches = Caches::default();
        self.vault = Some(Vault::open(&root, ctx.clone()));
        self.cfg = Some(cfg);
        self.themed = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — Ateliet", self.vault.as_ref().unwrap().name)));
        // reopen the tabs of the last session
        if let Some(tabs) = session["tabs"].as_array() {
            for t in tabs.iter().filter_map(|t| t.as_str()) {
                if self.vault.as_ref().unwrap().abs(t).is_file() {
                    self.open(t, true);
                }
            }
            self.active = session["active"].as_u64().unwrap_or(0) as usize;
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            }
        }
        // for tests and scripts: ATELIET_OPEN=<note> [ATELIET_MODE=read]
        if let Ok(p) = std::env::var("ATELIET_OPEN") {
            self.open(&p, true);
            if std::env::var("ATELIET_MODE").as_deref() == Ok("read") {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.mode = Mode::Read;
                }
            }
        }
        self.recent = session["recent"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    }

    fn save_session(&self) {
        if let Some(cfg) = &self.cfg {
            let tabs: Vec<&str> = self.tabs.iter().map(|t| t.path.as_str()).collect();
            let path = cfg.dir.join("ateliet/native-session.json");
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(path, serde_json::json!({ "tabs": tabs, "active": self.active, "recent": self.recent }).to_string());
        }
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), until: Instant::now() + Duration::from_secs(4) });
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_path(&self) -> Option<String> {
        self.active_tab().map(|t| t.path.clone())
    }

    // ── files and tabs ─────────────────────────────────────────────────────

    /// Open a note (or focus its tab). Non-markdown files open in the system app.
    pub fn open(&mut self, path: &str, new_tab: bool) {
        let Some(v) = &self.vault else { return };
        if !path.to_lowercase().ends_with(".md") {
            let _ = open::that(v.abs(path));
            return;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.active = i;
            return;
        }
        let text = std::fs::read_to_string(v.abs(path)).unwrap_or_default();
        let tab = Tab { path: path.to_string(), saved: text.clone(), text, mode: self.default_mode, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), scroll_to: None };
        if new_tab || self.tabs.is_empty() {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        } else {
            self.save_tab(self.active);
            self.tabs[self.active] = tab;
        }
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(30);
        self.save_session();
    }

    /// Follow a link from the active note; a missing note is created.
    pub fn open_link(&mut self, target: &str, new_tab: bool) {
        let Some(v) = &self.vault else { return };
        let source = self.active_path().unwrap_or_default();
        let (file, sub) = match target.split_once('#') {
            Some((f, s)) => (f, Some(s.to_string())),
            None => (target, None),
        };
        let path = match v.resolve(file, &source) {
            Some(p) if v.abs(&p).exists() => p,
            _ => {
                let name = if file.to_lowercase().ends_with(".md") { file.to_string() } else { format!("{file}.md") };
                if let Err(e) = v.write(&name, "") {
                    self.toast(format!("Could not create {name}: {e}"));
                    return;
                }
                v.index.write().unwrap().update(std::slice::from_ref(&name));
                self.vault.as_mut().unwrap().rescan();
                name
            }
        };
        self.open(&path, new_tab);
        if let Some(sub) = sub {
            if let Some(t) = self.tabs.get_mut(self.active) {
                let doc = t.doc();
                let want = sub.to_lowercase();
                if let Some((_, _, line)) = doc.headings.iter().find(|(_, h, _)| h.to_lowercase() == want) {
                    t.scroll_to = Some(*line);
                }
            }
        }
    }

    pub fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.save_tab(i);
        self.tabs.remove(i);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        } else if i < self.active {
            self.active -= 1;
        }
        self.save_session();
    }

    pub fn save_tab(&mut self, i: usize) {
        let Some(v) = &self.vault else { return };
        let Some(t) = self.tabs.get_mut(i) else { return };
        if !t.dirty() {
            return;
        }
        match v.write(&t.path, &t.text) {
            Ok(()) => {
                t.saved = t.text.clone();
                t.last_edit = None;
            }
            Err(e) => {
                let msg = format!("Could not save {}: {e}", t.path);
                self.toast(msg);
            }
        }
    }

    pub fn save_all(&mut self) {
        for i in 0..self.tabs.len() {
            self.save_tab(i);
        }
    }

    pub fn new_note(&mut self, folder: &str) {
        let Some(v) = &self.vault else { return };
        let base = if folder.is_empty() { "Untitled".to_string() } else { format!("{folder}/Untitled") };
        let path = v.available_path(&base, "md");
        if let Err(e) = v.write(&path, "") {
            self.toast(format!("Could not create a note: {e}"));
            return;
        }
        self.vault.as_mut().unwrap().rescan();
        self.open(&path, true);
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.mode = Mode::Edit;
        }
        self.ask = Some(Ask { title: "Name of the new note".into(), value: "Untitled".into(), kind: AskKind::Rename(path) });
    }

    /// Rename/move a file and update the open tabs (links elsewhere are kept as they are).
    pub fn rename(&mut self, from: &str, new_name: &str) {
        self.save_all();
        let Some(v) = &self.vault else { return };
        let dir = vault::parent(from);
        let ext = if from.to_lowercase().ends_with(".md") && !new_name.to_lowercase().ends_with(".md") { ".md" } else { "" };
        let to = if new_name.contains('/') { format!("{new_name}{ext}") } else if dir.is_empty() { format!("{new_name}{ext}") } else { format!("{dir}/{new_name}{ext}") };
        if to == from {
            return;
        }
        if v.abs(&to).exists() {
            self.toast(format!("{to} already exists"));
            return;
        }
        if let Some(d) = v.abs(&to).parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::rename(v.abs(from), v.abs(&to)) {
            self.toast(format!("Could not rename: {e}"));
            return;
        }
        let prefix = format!("{from}/");
        for t in &mut self.tabs {
            if t.path == from {
                t.path = to.clone();
            } else if let Some(rest) = t.path.strip_prefix(&prefix) {
                t.path = format!("{to}/{rest}");
            }
        }
        v.index.write().unwrap().update(&[from.to_string(), to.clone()]);
        self.vault.as_mut().unwrap().rescan();
        self.save_session();
    }

    pub fn delete(&mut self, path: &str) {
        let Some(v) = &self.vault else { return };
        let abs = v.abs(path);
        let root = v.root.clone();
        let index = v.index.clone();
        let local = self.cfg.as_ref().map(|c| c.read("app.json")["trashOption"].as_str() == Some("local")).unwrap_or(false);
        let res = if local {
            let dest = root.join(".trash").join(path.rsplit('/').next().unwrap_or(path));
            std::fs::create_dir_all(root.join(".trash")).and_then(|_| std::fs::rename(&abs, dest))
        } else {
            trash::delete(&abs).map_err(std::io::Error::other)
        };
        if let Err(e) = res {
            self.toast(format!("Could not delete: {e}"));
            return;
        }
        let prefix = format!("{path}/");
        let open: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, t)| t.path == path || t.path.starts_with(&prefix)).map(|(i, _)| i).rev().collect();
        for i in open {
            self.tabs[i].saved = self.tabs[i].text.clone();
            self.close_tab(i);
        }
        index.write().unwrap().update(&[path.to_string()]);
        self.vault.as_mut().unwrap().rescan();
    }

    pub fn open_daily(&mut self, date: chrono::NaiveDate) {
        let Some(v) = &self.vault else { return };
        match daily::ensure(v, &self.daily, date, &self.locale) {
            Ok(p) => {
                v.index.write().unwrap().update(std::slice::from_ref(&p));
                self.vault.as_mut().unwrap().rescan();
                self.open(&p, false);
            }
            Err(e) => self.toast(format!("Could not create the daily note: {e}")),
        }
    }

    // ── actions from rendered notes ────────────────────────────────────────

    pub fn apply(&mut self, a: Action) {
        match a {
            Action::Open(t, new_tab) => self.open_link(&t, new_tab),
            Action::Url(u) => {
                let _ = open::that(u);
            }
            Action::Tag(t) => {
                self.left_open = true;
                self.left_tab = LeftTab::Search;
                self.search.query = format!("tag:#{t}");
                self.search.run_now = true;
            }
            Action::ToggleTask(line, c) => {
                if let Some(p) = self.active_path() {
                    self.toggle_task(&p, line, c);
                }
            }
            Action::ToggleTaskIn(path, line, c) => self.toggle_task(&path, line, c),
        }
    }

    fn toggle_task(&mut self, path: &str, line: usize, status: char) {
        let re = regex::Regex::new(r"^(\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+\[)(.)(\])").unwrap();
        let flip = |text: &str| -> String {
            let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
            if let Some(l) = lines.get_mut(line) {
                *l = re.replace(l, |c: &regex::Captures| format!("{}{}{}", &c[1], if status == ' ' { 'x' } else { ' ' }, &c[3])).into_owned();
            }
            lines.join("\n")
        };
        if let Some(t) = self.tabs.iter_mut().find(|t| t.path == path) {
            t.text = flip(&t.text);
            t.last_edit = Some(Instant::now() - Duration::from_secs(5));
        } else if let Some(v) = &self.vault {
            if let Ok(text) = std::fs::read_to_string(v.abs(path)) {
                let _ = v.write(path, &flip(&text));
                v.index.write().unwrap().update(&[path.to_string()]);
            }
        }
        let idx: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, t)| t.path == path).map(|(i, _)| i).collect();
        for i in idx {
            self.save_tab(i);
        }
        if let Some(v) = &self.vault {
            v.index.write().unwrap().update(&[path.to_string()]);
        }
    }

    // ── Templater ──────────────────────────────────────────────────────────

    pub fn run_template(&mut self, ctx: &egui::Context, template: &str, new_note: bool) {
        let Some(v) = &self.vault else { return };
        if !new_note {
            if let Some(t) = self.tabs.get(self.active) {
                let (path, text, at) = (t.path.clone(), t.text.clone(), t.cursor);
                self.templater.run(ctx, v, self.dv_settings.clone(), template, &path.clone(), text, String::new(), templater::Mode::Insert { path, at });
                return;
            }
        }
        let folder = self.active_path().map(|p| vault::parent(&p).to_string()).unwrap_or_default();
        let base = if folder.is_empty() { "Untitled".to_string() } else { format!("{folder}/Untitled") };
        let path = v.available_path(&base, "md");
        if v.write(&path, "").is_err() {
            return;
        }
        v.index.write().unwrap().update(std::slice::from_ref(&path));
        self.templater.run(ctx, v, self.dv_settings.clone(), template, &path.clone(), String::new(), String::new(), templater::Mode::NewNote { path });
    }

    fn finish_templates(&mut self) {
        for f in self.templater.poll() {
            let out = match f.result {
                Ok(o) if o.cancelled => {
                    if let templater::Mode::NewNote { path } = &f.mode {
                        if let Some(v) = &self.vault {
                            let _ = std::fs::remove_file(v.abs(path));
                        }
                    }
                    continue;
                }
                Ok(o) => o,
                Err(e) => {
                    self.toast(format!("Templater: {}", e.lines().next().unwrap_or_default()));
                    continue;
                }
            };
            let (path, start) = match &f.mode {
                templater::Mode::Insert { path, at } => {
                    if let Some(t) = self.tabs.iter_mut().find(|t| &t.path == path) {
                        let b = editor::char_to_byte(&t.text, *at);
                        t.text.insert_str(b, &out.text);
                        t.last_edit = Some(Instant::now());
                        (path.clone(), b)
                    } else {
                        continue;
                    }
                }
                templater::Mode::NewNote { path } => {
                    if let Some(v) = &self.vault {
                        let _ = v.write(path, &out.text);
                    }
                    self.vault.as_mut().unwrap().rescan();
                    self.open(path, false);
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.text = out.text.clone();
                        t.saved = out.text.clone();
                        t.mode = Mode::Edit;
                    }
                    (path.clone(), 0)
                }
            };
            // the cursor of `tp.file.cursor()`
            if let (Some(&c), Some(t)) = (out.cursors.first(), self.tabs.iter_mut().find(|t| t.path == path)) {
                let b = start + editor::utf16_to_byte(&out.text, c);
                let ci = editor::byte_to_char(&t.text, b);
                t.cursor = ci;
                let id = t.editor_id();
                PENDING_CURSOR.with(|p| *p.borrow_mut() = Some((id, ci)));
            }
            let mut current = path.clone();
            for op in &out.ops {
                match op["op"].as_str() {
                    Some("rename") => {
                        if let Some(to) = op["path"].as_str() {
                            if to != current {
                                let name = to.strip_suffix(".md").unwrap_or(to).to_string();
                                self.rename(&current, &name);
                                current = to.to_string();
                            }
                        }
                    }
                    Some("open") => {
                        if let Some(p) = op["path"].as_str() {
                            let p = p.to_string();
                            self.vault.as_mut().unwrap().rescan();
                            self.open(&p, true);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

thread_local! {
    /// An editor cursor to set on the next frame (after a template is inserted).
    static PENDING_CURSOR: std::cell::RefCell<Option<(egui::Id, usize)>> = const { std::cell::RefCell::new(None) };
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.themed {
            self.palette = theme::apply(ctx, &self.appearance);
            self.themed = true;
        }
        if self.vault.is_none() {
            panels::vault_picker(self, ctx);
            return;
        }
        // changes on disk
        let changed = self.vault.as_mut().unwrap().poll_changes();
        if !changed.is_empty() {
            let v = self.vault.as_ref().unwrap();
            for t in &mut self.tabs {
                if changed.contains(&t.path) && !t.dirty() {
                    if let Ok(text) = std::fs::read_to_string(v.abs(&t.path)) {
                        if text != t.text {
                            t.text = text.clone();
                            t.saved = text;
                        }
                    }
                }
            }
            self.caches.docs.retain(|p, _| !changed.contains(p));
            for p in &changed {
                self.caches.images.remove(p);
            }
        }
        let generation = self.vault.as_ref().unwrap().index.read().map(|ix| ix.generation).unwrap_or(0);
        if generation != self.last_generation {
            self.last_generation = generation;
            ctx.request_repaint();
        }
        // autosave a second after the last keystroke
        let due: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, t)| t.dirty() && t.last_edit.is_some_and(|e| e.elapsed() > Duration::from_millis(900))).map(|(i, _)| i).collect();
        for i in due {
            self.save_tab(i);
            if let (Some(v), Some(t)) = (&self.vault, self.tabs.get(i)) {
                v.index.write().unwrap().update(std::slice::from_ref(&t.path));
            }
        }
        if self.tabs.iter().any(|t| t.dirty()) {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.finish_templates();
        commands::shortcuts(self, ctx);

        panels::ribbon(self, ctx);
        if self.left_open {
            panels::left(self, ctx);
        }
        if self.right_open {
            panels::right(self, ctx);
        }
        panels::status_bar(self, ctx);
        panels::tab_bar(self, ctx);
        self.central(ctx);

        commands::show_palette(self, ctx);
        panels::ask_dialog(self, ctx);
        panels::settings_window(self, ctx);
        self.templater.show_prompt(ctx);
        panels::toasts(self, ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_all();
        self.save_session();
    }
}

impl App {
    fn central(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let frame = egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::ZERO);
        let mut actions: Vec<Action> = Vec::new();
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            if self.tabs.is_empty() {
                panels::empty_state(self, ui);
                return;
            }
            let size = self.appearance.font_size;
            let readable = self.readable;
            let active = self.active;
            let Some(vault) = self.vault.as_ref() else { return };
            let tab = &mut self.tabs[active];
            let max_w = if readable { 46.0 * size } else { f32::INFINITY };
            egui::ScrollArea::vertical().id_salt(("note", &tab.path, tab.mode == Mode::Read)).auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_width();
                let w = avail.min(max_w + 2.0 * 32.0);
                let side = ((avail - w) / 2.0).max(0.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(side + 32.0);
                    ui.vertical(|ui| {
                        ui.set_width((w - 64.0).max(200.0));
                        ui.add_space(28.0);
                        // inline title
                        ui.label(egui::RichText::new(vault::basename(&tab.path)).size(size * 1.9).family(egui::FontFamily::Name("bold".into())).color(p.text));
                        ui.add_space(10.0);
                        match tab.mode {
                            Mode::Edit => {
                                let st = editor::Styles { size, p };
                                let id = tab.editor_id();
                                if let Some((pid, ci)) = PENDING_CURSOR.with(|c| c.borrow_mut().take()) {
                                    if pid == id {
                                        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                                        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(ci))));
                                        state.store(ui.ctx(), id);
                                        ui.ctx().memory_mut(|m| m.request_focus(id));
                                    } else {
                                        PENDING_CURSOR.with(|c| *c.borrow_mut() = Some((pid, ci)));
                                    }
                                }
                                if let Some(line) = tab.scroll_to.take() {
                                    let b = tab.text.split_inclusive('\n').take(line).map(str::len).sum::<usize>();
                                    let ci = editor::byte_to_char(&tab.text, b);
                                    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(ci))));
                                    state.store(ui.ctx(), id);
                                    ui.ctx().memory_mut(|m| m.request_focus(id));
                                }
                                let before = tab.text.len();
                                let before_hash = {
                                    let mut h = DefaultHasher::new();
                                    tab.text.hash(&mut h);
                                    h.finish()
                                };
                                let resp = ui.add(editor::editor_widget(&mut tab.text, id, &mut tab.highlight, &st));
                                if let Some(state) = egui::TextEdit::load_state(ui.ctx(), id) {
                                    if let Some(r) = state.cursor.char_range() {
                                        tab.cursor = r.primary.index;
                                    }
                                }
                                let after_hash = {
                                    let mut h = DefaultHasher::new();
                                    tab.text.hash(&mut h);
                                    h.finish()
                                };
                                if resp.changed() || before != tab.text.len() || before_hash != after_hash {
                                    tab.last_edit = Some(Instant::now());
                                }
                            }
                            Mode::Read => {
                                let doc = tab.doc();
                                let mut host = RenderHost { vault, palette: p, size, source: &tab.path, caches: &mut self.caches, settings: &self.dv_settings };
                                let mut r = Renderer::new(&mut host);
                                r.scroll_to = tab.scroll_to.take();
                                r.blocks(ui, &doc.blocks);
                                actions.append(&mut r.actions);
                            }
                        }
                        ui.add_space(200.0);
                    });
                });
            });
        });
        for a in actions {
            self.apply(a);
        }
    }
}
