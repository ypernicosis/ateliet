//! The application: vault, tabs, sidebars, editor/reading view, commands.

use crate::config::{self, Appearance, VaultConfig};
use crate::daily::{self, DailySettings};
use crate::host::{Caches, RenderHost};
use crate::markdown::{self, Action, Doc, Host as _, Renderer};
use crate::panels;
use crate::templater::{self, Templater, TemplaterSettings};
use crate::theme::{self, Palette};
use crate::vault::{self, Vault};
use crate::{commands, editor};
use ateliet_core::dataview::Settings;
use eframe::egui::{self, KeyboardShortcut};
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
    /// Who this tab is, for the panes (two tabs can show the same note).
    pub uid: u64,
    /// The text last copied between tabs of the same note (a hash).
    pub sync: u64,
    pub path: String,
    pub text: String,
    pub saved: String,
    pub mode: Mode,
    pub doc: Option<(u64, Arc<Doc>)>,
    pub last_edit: Option<Instant>,
    /// Cursor (char index) last seen in the editor.
    pub cursor: usize,
    pub highlight: editor::HighlightCache,
    pub find: crate::find::Find,
    /// A heading line to scroll to on the next frame.
    pub scroll_to: Option<usize>,
    pub live: crate::live::LiveState,
    /// A `.storyboard` file open in the drawing editor.
    pub board: Option<Box<crate::board_ui::BoardEditor>>,
    /// A Kanban board or Excalidraw drawing open in the Quadro.
    pub quadro: Option<Box<crate::quadro_ui::QuadroEditor>>,
    /// A Scriptorium screen (the library…) instead of a note.
    pub view: Option<crate::sc::View>,
    /// A book (EPUB, PDF, comic) open in the reader, or the shelf.
    pub reader: Option<Box<crate::reader_ui::Reader>>,
    /// Paths this tab showed before (back) and after going back (forward).
    pub back: Vec<String>,
    pub fwd: Vec<String>,
}

impl Tab {
    pub fn dirty(&self) -> bool {
        match &self.view {
            Some(crate::sc::View::Capa(c)) => return c.dirty(),
            Some(crate::sc::View::Pagina(c)) => return c.dirty(),
            _ => {}
        }
        if let Some(q) = &self.quadro {
            return q.dirty;
        }
        self.text != self.saved
    }

    pub fn title(&self) -> String {
        if let Some(q) = &self.quadro {
            return q.title();
        }
        if let Some(r) = &self.reader {
            return r.title();
        }
        match &self.view {
            Some(v) => v.title(),
            None => vault::basename(&self.path).to_string(),
        }
    }

    pub fn doc(&mut self) -> Arc<Doc> {
        let k = editor::text_hash(&self.text);
        match &self.doc {
            Some((key, d)) if *key == k => d.clone(),
            _ => {
                let d = Arc::new(markdown::parse(&self.text));
                self.doc = Some((k, d.clone()));
                d
            }
        }
    }

    /// The parsed note for what can show it a moment late (the outline):
    /// while the note is being typed, the last parse, so a long note is not
    /// parsed again at every key; parsed again once the typing pauses.
    pub fn doc_settled(&mut self, ctx: &egui::Context) -> Arc<Doc> {
        const PAUSE: Duration = Duration::from_millis(600);
        let typing = self.last_edit.is_some_and(|t| t.elapsed() < PAUSE);
        match &self.doc {
            Some((_, d)) if typing => {
                ctx.request_repaint_after(PAUSE);
                d.clone()
            }
            _ => self.doc(),
        }
    }

    /// The note's words for what shows them every frame (the status bar, the
    /// focus panel): while it is being typed, the count from before the last
    /// keys; counted again once the typing pauses.
    pub fn words(&self, ctx: &egui::Context) -> usize {
        const PAUSE: Duration = Duration::from_millis(600);
        let typing = self.last_edit.is_some_and(|t| t.elapsed() < PAUSE);
        if typing {
            ctx.request_repaint_after(PAUSE);
        }
        crate::writing::word_count(&self.text, self.uid, typing)
    }

    pub fn editor_id(&self) -> egui::Id {
        egui::Id::new(("editor", self.uid, &self.path))
    }

    /// A plain note (not a board, a drawing, a book or a Scriptorium screen).
    pub fn is_note(&self) -> bool {
        self.view.is_none() && self.board.is_none() && self.quadro.is_none() && self.reader.is_none()
    }

    fn text_hash(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.text.hash(&mut h);
        self.saved.hash(&mut h);
        h.finish()
    }
}

/// The height of the strip on top of each pane when the centre is divided.
const PANE_HEAD: f32 = 30.0;

/// A new tab's `uid`.
pub fn next_uid() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
    Manuscript,
    Pictures,
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
    NewFolder(String),
    ConfirmDelete(String),
    /// Several explorer items: to the trash, or into a folder (typed).
    ConfirmDeleteMany(Vec<String>),
    MoveMany(Vec<String>),
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
    /// The egui context (for work started outside a frame's UI code).
    pub ctx: Option<egui::Context>,
    /// The shared library being copied (Settings → Images).
    pub copy_job: Option<crate::settings_pages::CopyJob>,
    pub theme_browser: crate::settings_pages::ThemeBrowser,
    /// The centre divided in panes (`None`: one pane, the active tab).
    pub panes: Option<crate::panes::Node>,
    /// The tab of the pane in use last frame.
    pane_focus: u64,
    /// A pane (or a tab from the bar) being dragged to another place.
    pub pane_drag: Option<u64>,
    pub templater: Templater,
    pub search: panels::SearchState,
    pub calendar: panels::CalendarState,
    pub palette_ui: Option<commands::Palette>,
    pub toasts: Vec<Toast>,
    pub ask: Option<Ask>,
    pub settings_open: bool,
    pub readable: bool,
    pub default_mode: Mode,
    /// Editing shows the note rendered except the block being edited (Obsidian's Live Preview).
    pub live_preview: bool,
    pub locale: String,
    pub hotkeys: Vec<(String, KeyboardShortcut)>,
    pub recent: Vec<String>,
    /// Image mode: the picture library in the centre, the note on the right.
    pub images_mode: bool,
    /// The Journal's notebooks fill the window (the third mode).
    pub journal_mode: bool,
    pub journal: crate::journal_ui::Journal,
    /// The reader beside the Journal or the notes ("Ler ao lado").
    pub side_reader: Option<Box<crate::reader_ui::Reader>>,
    pub images: crate::images_ui::ImagesState,
    pub sc: Option<crate::sc::Sc>,
    pub binder: crate::sc::binder::Binder,
    pub form: Option<crate::form::Form>,
    pub export: Option<crate::sc::export_ui::ExportDialog>,
    pub picker: Option<crate::sc::picker::Picker>,
    pub new_board_dlg: Option<crate::board_ui::NewBoard>,
    /// Set while going back (true) or forward (false) in a tab's history.
    nav: Option<bool>,
    /// Unlinked mentions of a note: (note, index generation, [(source, offset, line)]).
    pub unlinked: Option<(String, u64, Vec<(String, usize, String)>)>,
    /// A link hovered: its target, since when, where; and the preview's rect.
    hover: Option<(String, Instant, egui::Pos2)>,
    hover_rect: Option<egui::Rect>,
    pub updater: crate::update::Updater,
    pub music: crate::music::Music,
    pub video: crate::work_video::Video,
    pub focus: Option<crate::sc::focus::Focus>,
    pub sprint: Option<crate::sc::focus::Sprint>,
    pub focus_restore: (bool, bool),
    themed: bool,
    last_generation: u64,
}

fn system_locale() -> String {
    std::env::var("LC_ALL").or_else(|_| std::env::var("LC_MESSAGES")).or_else(|_| std::env::var("LANG")).ok().map(|l| l.split('.').next().unwrap_or("en").replace('_', "-").to_lowercase()).filter(|l| !l.is_empty() && l != "c" && l != "posix").unwrap_or_else(|| "en".into())
}

impl App {
    pub fn new(cc: &eframe::CreationContext, vault: Option<PathBuf>) -> App {
        crate::fonts::install(&cc.egui_ctx);
        crate::fonts::warm_book_fonts();
        let mut app = App {
            vault: None,
            cfg: None,
            appearance: Appearance::default(),
            palette: Palette::new(&Appearance::default()),
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
            ctx: None,
            copy_job: None,
            theme_browser: crate::settings_pages::ThemeBrowser::new(),
            panes: None,
            pane_focus: 0,
            pane_drag: None,
            templater: Templater::default(),
            search: Default::default(),
            calendar: panels::CalendarState::new(),
            palette_ui: None,
            toasts: Vec::new(),
            ask: None,
            settings_open: false,
            readable: true,
            default_mode: Mode::Edit,
            live_preview: true,
            locale: system_locale(),
            hotkeys: Vec::new(),
            recent: Vec::new(),
            images_mode: false,
            images: Default::default(),
            journal_mode: false,
            journal: Default::default(),
            side_reader: None,
            sc: None,
            binder: Default::default(),
            form: None,
            export: None,
            picker: None,
            new_board_dlg: None,
            nav: None,
            unlinked: None,
            hover: None,
            hover_rect: None,
            updater: crate::update::Updater::new(),
            music: Default::default(),
            video: Default::default(),
            focus: None,
            sprint: None,
            focus_restore: (true, true),
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
        self.live_preview = appj["livePreview"].as_bool().unwrap_or(true);
        self.calendar.load(&cfg);
        self.hotkeys = commands::load_hotkeys(&cfg);
        let session = cfg.read("ateliet/native-session.json");
        self.tabs.clear();
        self.caches = Caches::default();
        self.images = Default::default();
        self.images_mode = false;
        self.journal.save_all();
        self.journal = Default::default();
        self.journal_mode = false;
        self.vault = Some(Vault::open(&root, ctx.clone()));
        self.sc = Some(crate::sc::Sc::load(&cfg.dir));
        self.cfg = Some(cfg);
        self.themed = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} — Ateliet", self.vault.as_ref().unwrap().name)));
        // reopen the tabs of the last session
        if let Some(tabs) = session["tabs"].as_array() {
            // the saved tab at each place, by uid (a note open twice comes back twice)
            let mut uids: Vec<Option<u64>> = Vec::new();
            for t in tabs.iter().map(|t| t.as_str().unwrap_or("")) {
                let mut uid = None;
                if !t.is_empty() && (self.vault.as_ref().unwrap().abs(t).is_file() || crate::sc::View::from_path(t).is_some() || t == crate::reader_ui::SHELF) {
                    if let Some(j) = self.tabs.iter().position(|x| x.path == t && x.is_note()) {
                        uid = self.clone_tab(j).map(|k| self.tabs[k].uid);
                    } else {
                        let n = self.tabs.len();
                        self.open(t, true);
                        uid = (self.tabs.len() > n).then(|| self.tabs[self.active].uid);
                    }
                }
                uids.push(uid);
            }
            let saved_active = session["active"].as_u64().unwrap_or(0) as usize;
            let by_uid = |u: Option<u64>, tabs: &[Tab]| u.and_then(|u| tabs.iter().position(|t| t.uid == u));
            self.active = by_uid(uids.get(saved_active).copied().flatten(), &self.tabs).unwrap_or(self.tabs.len().saturating_sub(1));
            // the panes as they were left
            self.panes = crate::panes::Node::from_json(&session["panes"], &|v| v.as_u64().and_then(|i| uids.get(i as usize).copied().flatten()));
            // (a session from before: two panes)
            if self.panes.is_none() {
                let other = session["split"].as_str().and_then(|sp| self.tabs.iter().position(|t| t.path == sp)).filter(|&i| i != self.active);
                if let (Some(o), Some(a)) = (other, self.tabs.get(self.active)) {
                    let right = session["split_right"].as_bool().unwrap_or(true);
                    let f = session["split_frac"].as_f64().unwrap_or(0.5).clamp(0.2, 0.8) as f32;
                    let (l, r) = if right { (a.uid, self.tabs[o].uid) } else { (self.tabs[o].uid, a.uid) };
                    self.panes = Some(crate::panes::Node::Split { down: false, kids: vec![crate::panes::Node::Leaf(l), crate::panes::Node::Leaf(r)], fracs: vec![f, 1.0 - f] });
                }
            }
            self.pane_focus = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
            self.fix_panes();
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
        // reopening the tabs saved the session half restored
        self.save_session();
    }

    pub fn save_session(&self) {
        if let Some(cfg) = &self.cfg {
            let tabs: Vec<&str> = self.tabs.iter().map(|t| t.path.as_str()).collect();
            let path = cfg.dir.join("ateliet/native-session.json");
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let panes = self.panes.as_ref().map(|n| n.to_json(&|u| self.tabs.iter().position(|t| t.uid == u).map(|i| serde_json::json!(i)))).unwrap_or(serde_json::Value::Null);
            let _ = std::fs::write(path, serde_json::json!({ "tabs": tabs, "active": self.active, "recent": self.recent, "panes": panes }).to_string());
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

    /// Open a note (or focus its tab), a book, or the work-session video.
    /// Other non-markdown files open in the system app.
    pub fn open(&mut self, path: &str, new_tab: bool) {
        let Some(v) = &self.vault else { return };
        if let Some(view) = crate::sc::View::from_path(path) {
            if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
                self.focus_tab(i);
                return;
            }
            let tab = Tab { uid: next_uid(), sync: 0, path: path.to_string(), text: String::new(), saved: String::new(), mode: Mode::Read, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), find: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: Some(view), reader: None, back: Vec::new(), fwd: Vec::new() };
            if new_tab || self.tabs.is_empty() {
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
            } else {
                self.replace_active(tab);
            }
            self.save_session();
            return;
        }
        if path == crate::reader_ui::SHELF || ateliet_core::reader::format_of(path).is_some() {
            self.open_book(path, None, new_tab);
            return;
        }
        let lower = path.to_lowercase();
        if ateliet_core::work_video::is_video(std::path::Path::new(path)) {
            let abs = v.abs(path);
            self.video.play_local(abs);
            return;
        }
        let is_board = lower.ends_with(".storyboard");
        let is_quadro = lower.ends_with(".excalidraw") || (lower.ends_with(".md") && crate::quadro_ui::is_quadro(&v.abs(path)));
        if !lower.ends_with(".md") && !is_board && !is_quadro {
            let _ = open::that(v.abs(path));
            return;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.focus_tab(i);
            return;
        }
        let board = if is_board {
            match crate::board_ui::BoardEditor::open(path, v.abs(path)) {
                Ok(b) => Some(Box::new(b)),
                Err(e) => {
                    self.toast(format!("Não foi possível abrir {path}: {e}"));
                    return;
                }
            }
        } else {
            None
        };
        let quadro = if is_quadro {
            match crate::quadro_ui::QuadroEditor::open(path, v.abs(path)) {
                Ok(mut q) => {
                    if let Some(c) = &self.cfg {
                        q.set_backup_dir(c.dir.join("ateliet").join("quadro"));
                    }
                    Some(Box::new(q))
                }
                Err(e) => {
                    self.toast(format!("Não foi possível abrir {path}: {e}"));
                    return;
                }
            }
        } else {
            None
        };
        // characters open as their sheet (the plugin's "abrirFichas"), maps as maps
        if !is_board && !is_quadro {
            // the index may still be building (session restore): read the note itself then
            let kind = v.index.read().unwrap().page(path).map(|p| ateliet_core::scriptorium::book::kind(p).to_string()).unwrap_or_else(|| {
                let text = std::fs::read_to_string(v.abs(path)).unwrap_or_default();
                ateliet_core::scriptorium::sheet::split(&text).0.and_then(|m| m.get("scriptorium").and_then(|k| k.as_str()).map(str::to_string)).unwrap_or_default()
            });
            let sheets = self.sc.as_ref().is_some_and(|s| s.settings.data["abrirFichas"].as_bool().unwrap_or(true));
            let view = match kind.as_str() {
                "personagem" | "modelo-ficha" if sheets => Some(crate::sc::View::Sheet(Box::new(crate::sc::sheet_ui::SheetView::open(path, &v.root)))),
                "mapa" => Some(crate::sc::View::Mapa(Box::new(crate::sc::mapa_ui::MapaView::open(path)))),
                _ => None,
            };
            if let Some(view) = view {
                let tab = Tab { uid: next_uid(), sync: 0, path: path.to_string(), text: String::new(), saved: String::new(), mode: Mode::Read, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), find: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: Some(view), reader: None, back: Vec::new(), fwd: Vec::new() };
                if new_tab || self.tabs.is_empty() {
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                } else {
                    self.replace_active(tab);
                }
                self.recent.retain(|p| p != path);
                self.recent.insert(0, path.to_string());
                self.save_session();
                return;
            }
        }
        let text = if is_board || is_quadro { String::new() } else { std::fs::read_to_string(v.abs(path)).unwrap_or_default() };
        let mut tab = Tab { uid: next_uid(), sync: 0, path: path.to_string(), saved: text.clone(), text, mode: self.default_mode, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), find: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: None, reader: None, back: Vec::new(), fwd: Vec::new() };
        tab.board = board;
        tab.quadro = quadro;
        if new_tab || self.tabs.is_empty() {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        } else {
            self.replace_active(tab);
        }
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(30);
        self.save_session();
    }

    /// Show `tab` in place of the active one; the history moves along.
    pub(crate) fn replace_active(&mut self, mut tab: Tab) {
        self.save_tab(self.active);
        let old = &mut self.tabs[self.active];
        let (mut back, mut fwd) = (std::mem::take(&mut old.back), std::mem::take(&mut old.fwd));
        match self.nav.take() {
            // going back: the page left goes to forward
            Some(true) => fwd.push(old.path.clone()),
            Some(false) => back.push(old.path.clone()),
            None => {
                back.push(old.path.clone());
                fwd.clear();
            }
        }
        back.truncate(100);
        tab.back = back;
        tab.fwd = fwd;
        // the same tab for the panes, showing something else
        tab.uid = old.uid;
        self.tabs[self.active] = tab;
    }

    /// Back (or forward) in the active tab's history.
    pub fn go_history(&mut self, back: bool) {
        let Some(t) = self.tabs.get_mut(self.active) else { return };
        let list = if back { &mut t.back } else { &mut t.fwd };
        let Some(path) = list.pop() else { return };
        if self.tabs.iter().any(|t| t.path == path) {
            // shown in another tab already: go there
            self.open(&path, false);
            return;
        }
        self.nav = Some(back);
        self.open(&path, false);
        self.nav = None;
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
                    self.toast(format!("Não foi possível criar {name}: {e}"));
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
        let closing_active = i == self.active;
        let uid = self.tabs[i].uid;
        // a pane in use closing: the one before it (or after) takes the focus
        let next = self.panes.as_ref().filter(|n| closing_active && n.contains(uid)).and_then(|n| {
            let l = n.leaves();
            let k = l.iter().position(|u| *u == uid)?;
            l.get(k.wrapping_sub(1)).or(l.get(k + 1)).copied()
        });
        self.tabs.remove(i);
        if let Some(n) = self.panes.take() {
            self.panes = n.remove(uid);
        }
        if let Some(j) = next.and_then(|u| self.uid_index(u)) {
            self.active = j;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        } else if i < self.active {
            self.active -= 1;
        }
        if next.is_some() {
            self.pane_focus = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
        }
        self.fix_panes();
        self.save_session();
    }

    /// Make tab `i` the one in use. When it is shown in another pane, the
    /// panes stay as they are and only the focus moves; else the pane in use
    /// shows it.
    pub fn focus_tab(&mut self, i: usize) {
        if i >= self.tabs.len() || i == self.active {
            return;
        }
        self.active = i;
        self.fix_panes();
    }

    pub fn uid_index(&self, uid: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.uid == uid)
    }

    /// The panes kept in line with the tabs: closed tabs leave their pane, the
    /// active tab is in one (in the pane in use, when it was elsewhere), and a
    /// single pane left is no division at all.
    pub fn fix_panes(&mut self) {
        let Some(mut n) = self.panes.take() else {
            self.pane_focus = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
            return;
        };
        for u in n.leaves() {
            if self.uid_index(u).is_none() {
                match n.remove(u) {
                    Some(m) => n = m,
                    None => return,
                }
            }
        }
        let Some(active) = self.tabs.get(self.active).map(|t| t.uid) else { return };
        if !n.contains(active) {
            let target = Some(self.pane_focus).filter(|u| n.contains(*u)).unwrap_or_else(|| n.leaves()[0]);
            n.replace(target, active);
        }
        self.pane_focus = active;
        self.panes = match n {
            crate::panes::Node::Leaf(_) => None,
            n => Some(n),
        };
    }

    /// Whether the centre is divided in panes now.
    pub fn split_shown(&self) -> bool {
        self.panes.is_some()
    }

    /// A second tab for the note of tab `i` (to show it in two panes), right
    /// after it. Only plain notes; the two keep the same text.
    pub fn clone_tab(&mut self, i: usize) -> Option<usize> {
        let t = self.tabs.get(i).filter(|t| t.is_note())?;
        let tab = Tab { uid: next_uid(), sync: t.text_hash(), path: t.path.clone(), text: t.text.clone(), saved: t.saved.clone(), mode: t.mode, doc: None, last_edit: None, cursor: t.cursor, highlight: Default::default(), find: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: None, reader: None, back: Vec::new(), fwd: Vec::new() };
        self.tabs[i].sync = tab.sync;
        self.tabs.insert(i + 1, tab);
        if self.active > i {
            self.active += 1;
        }
        Some(i + 1)
    }

    /// Tabs of the same note keep the same text: what changed in one (typing,
    /// a template, a save) goes to the others.
    pub fn sync_clones(&mut self) {
        let n = self.tabs.len();
        for i in 0..n {
            if !self.tabs[i].is_note() || self.tabs[..i].iter().any(|t| t.path == self.tabs[i].path && t.is_note()) {
                continue;
            }
            let group: Vec<usize> = (i..n).filter(|&j| self.tabs[j].is_note() && self.tabs[j].path == self.tabs[i].path).collect();
            if group.len() < 2 {
                continue;
            }
            let hashes: Vec<u64> = group.iter().map(|&j| self.tabs[j].text_hash()).collect();
            let changed = |k: usize| hashes[k] != self.tabs[group[k]].sync;
            // the tab in use wins when several changed
            let src = group.iter().position(|&j| j == self.active).filter(|&k| changed(k)).or_else(|| (0..group.len()).find(|&k| changed(k)));
            let Some(k) = src else { continue };
            let (text, saved, h) = (self.tabs[group[k]].text.clone(), self.tabs[group[k]].saved.clone(), hashes[k]);
            for (m, &j) in group.iter().enumerate() {
                let t = &mut self.tabs[j];
                if m != k {
                    t.text = text.clone();
                    t.saved = saved.clone();
                    t.cursor = t.cursor.min(t.text.chars().count());
                }
                t.sync = h;
            }
        }
    }

    /// Divide the pane in use: a new pane on `side` of it, with the same note
    /// (`same`) or another one (the next tab not shown, else the last note
    /// seen). The new pane is the one in use.
    pub fn split_pane(&mut self, side: crate::panes::Side, same: bool) {
        let Some(target) = self.tabs.get(self.active).map(|t| t.uid) else {
            self.toast("Abra uma nota para dividir a tela.");
            return;
        };
        let shown = |app: &App, i: usize| app.tabs[i].uid == target || app.panes.as_ref().is_some_and(|n| n.contains(app.tabs[i].uid));
        let mut new = None;
        if same {
            new = self.clone_tab(self.active);
            if new.is_none() {
                self.toast("Só notas podem ser abertas duas vezes; abrindo outra ao lado.");
            }
        }
        if new.is_none() {
            // the nearest tab not shown yet, after the active one first
            let a = self.active;
            new = (a + 1..self.tabs.len()).chain((0..a).rev()).find(|&i| !shown(self, i));
        }
        if new.is_none() {
            let here = self.active_path();
            let other = self.vault.as_ref().and_then(|v| self.recent.iter().find(|p| Some(*p) != here.as_ref() && !self.tabs.iter().any(|t| &t.path == *p) && v.abs(p).is_file()).cloned());
            if let Some(o) = other {
                let n = self.tabs.len();
                let keep = self.panes.take();
                self.open(&o, true);
                self.panes = keep;
                new = (self.tabs.len() > n).then_some(self.active);
            }
        }
        // nothing else open: the same note, when it is one
        if new.is_none() {
            new = self.uid_index(target).and_then(|i| self.clone_tab(i));
        }
        let Some(i) = new else {
            self.toast("Abra outra nota para dividir a tela.");
            return;
        };
        let uid = self.tabs[i].uid;
        let mut n = self.panes.take().unwrap_or(crate::panes::Node::Leaf(target));
        n.insert(target, uid, side);
        self.panes = Some(n);
        self.active = i;
        self.pane_focus = uid;
        self.save_session();
    }

    /// Put the tab of `uid` (a pane, or a tab dragged from the bar) on `side`
    /// of the pane of `target`; in the middle, the two trade places.
    pub fn dock(&mut self, uid: u64, target: u64, side: crate::panes::Side) {
        use crate::panes::{Node, Side};
        if uid == target {
            return;
        }
        let Some(active) = self.tabs.get(self.active).map(|t| t.uid) else { return };
        let mut n = self.panes.take().unwrap_or(Node::Leaf(active));
        if !n.contains(target) {
            self.panes = Some(n).filter(|n| !matches!(n, Node::Leaf(_)));
            return;
        }
        if side == Side::Center {
            if n.contains(uid) {
                n.swap(uid, target);
            } else {
                n.replace(target, uid);
            }
        } else {
            if n.contains(uid) {
                n = n.remove(uid).unwrap_or(Node::Leaf(target));
            }
            n.insert(target, uid, side);
        }
        self.panes = n.normalized();
        if let Some(i) = self.uid_index(uid) {
            self.active = i;
        }
        self.pane_focus = uid;
        self.fix_panes();
        self.save_session();
    }

    /// Close the pane of `uid`; its tab stays in the bar unless it was a
    /// second tab of a note shown elsewhere.
    pub fn close_pane(&mut self, uid: u64) {
        let Some(i) = self.uid_index(uid) else { return };
        let twin = self.tabs.iter().any(|t| t.uid != uid && t.path == self.tabs[i].path && t.is_note());
        if twin {
            self.close_tab(i);
            return;
        }
        let Some(n) = self.panes.take() else { return };
        let l = n.leaves();
        let k = l.iter().position(|u| *u == uid);
        self.panes = n.remove(uid);
        if self.active == i {
            let next = k.and_then(|k| l.get(k.wrapping_sub(1)).or(l.get(k + 1))).and_then(|u| self.uid_index(*u));
            if let Some(j) = next {
                self.active = j;
                self.pane_focus = self.tabs[j].uid;
            }
        }
        self.fix_panes();
        self.save_session();
    }

    /// Show tab `i` in a pane beside the one in use, which stays in use.
    pub fn show_beside(&mut self, i: usize) {
        let Some(uid) = self.tabs.get(i).map(|t| t.uid) else { return };
        let Some(active) = self.tabs.get(self.active).map(|t| t.uid) else { return };
        if self.panes.as_ref().is_some_and(|n| n.contains(uid)) || uid == active {
            return;
        }
        let mut n = self.panes.take().unwrap_or(crate::panes::Node::Leaf(active));
        n.insert(active, uid, crate::panes::Side::Right);
        self.panes = Some(n);
        self.save_session();
    }

    /// Open a file in a pane beside the one in use (Obsidian's "Open to the
    /// right"); the new pane is the one in use. The note in use opens a
    /// second time.
    pub fn open_beside(&mut self, path: &str) {
        let Some(before) = self.tabs.get(self.active).map(|t| (t.uid, t.path.clone())) else {
            self.open(path, true);
            return;
        };
        if before.1 == path {
            self.split_pane(crate::panes::Side::Right, true);
            return;
        }
        // shown in another pane already: just go there
        if let Some(i) = self.tabs.iter().position(|t| t.path == path && self.panes.as_ref().is_some_and(|n| n.contains(t.uid))) {
            self.focus_tab(i);
            return;
        }
        let keep = self.panes.take();
        self.open(path, true);
        self.panes = keep;
        // (it may have been open already, in no pane)
        let Some(new) = self.tabs.get(self.active).map(|t| t.uid).filter(|u| *u != before.0) else { return };
        let mut n = self.panes.take().unwrap_or(crate::panes::Node::Leaf(before.0));
        if !n.contains(new) {
            n.insert(before.0, new, crate::panes::Side::Right);
        }
        self.panes = Some(n);
        self.pane_focus = new;
        self.fix_panes();
        self.save_session();
    }

    /// Join every pane back into one (the one in use).
    pub fn join_panes(&mut self) {
        self.panes = None;
        self.save_session();
    }

    /// The panes side by side become stacked, and the other way round.
    pub fn flip_panes(&mut self) {
        if let Some(n) = self.panes.as_mut() {
            n.flip();
            self.save_session();
        }
    }

    /// Every pane in each row or column the same size.
    pub fn even_panes(&mut self) {
        if let Some(n) = self.panes.as_mut() {
            n.even_all();
            self.save_session();
        }
    }

    pub fn save_tab(&mut self, i: usize) {
        let Some(v) = &self.vault else { return };
        let Some(t) = self.tabs.get_mut(i) else { return };
        if let Some(b) = t.board.as_mut() {
            b.save();
            return;
        }
        if let Some(q) = t.quadro.as_mut() {
            q.save();
            let err = q.error.take();
            v.index.write().unwrap().update(&[t.path.clone()]);
            if let Some(e) = err {
                self.toast(e);
            }
            return;
        }
        if let Some(crate::sc::View::Sheet(s)) = t.view.as_mut() {
            if let Some(p) = s.save(&v.root) {
                v.index.write().unwrap().update(&[p]);
            }
            return;
        }
        if let Some(crate::sc::View::Trilha(tv)) = t.view.as_mut() {
            let saved = {
                let ix = v.index.read().unwrap();
                tv.save(&v.root, &ix, &self.sc.as_ref().map(|s| s.author()).unwrap_or_default())
            };
            if let Some(p) = saved {
                v.index.write().unwrap().update(&[p]);
            }
            return;
        }
        if let Some(crate::sc::View::Mapa(m)) = t.view.as_mut() {
            if let Some(p) = m.save(&v.root) {
                v.index.write().unwrap().update(&[p]);
            }
            return;
        }
        if let Some(crate::sc::View::Mundo(m)) = t.view.as_mut() {
            let saved = {
                let ix = v.index.read().unwrap();
                m.save(&v.root, &ix)
            };
            if let Some(p) = saved {
                v.index.write().unwrap().update(&[p]);
            }
            return;
        }
        if let Some(crate::sc::View::Enredo(e)) = t.view.as_mut() {
            let author = self.sc.as_ref().map(|s| s.author()).unwrap_or_default();
            let saved = {
                let ix = v.index.read().unwrap();
                e.save(&v.root, &ix, &author)
            };
            if let Some(p) = saved {
                v.index.write().unwrap().update(&[p]);
            }
            return;
        }
        if !t.dirty() {
            return;
        }
        match v.write(&t.path, &t.text) {
            Ok(()) => {
                t.saved = t.text.clone();
                t.last_edit = None;
            }
            Err(e) => {
                let msg = format!("Não foi possível salvar {}: {e}", t.path);
                self.toast(msg);
            }
        }
    }

    pub fn save_all(&mut self) {
        for i in 0..self.tabs.len() {
            self.save_tab(i);
        }
        self.journal.save_all();
    }

    pub fn new_note(&mut self, folder: &str) {
        let Some(v) = &self.vault else { return };
        let base = if folder.is_empty() { "Sem título".to_string() } else { format!("{folder}/Sem título") };
        let path = v.available_path(&base, "md");
        if let Err(e) = v.write(&path, "") {
            self.toast(format!("Não foi possível criar a nota: {e}"));
            return;
        }
        self.vault.as_mut().unwrap().rescan();
        self.open(&path, true);
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.mode = Mode::Edit;
        }
        // the folder's template fills the new note
        if let (Some(template), Some(ctx)) = (templater::Templater::folder_template(&self.templater_settings, &path), self.ctx.clone()) {
            if let Some(v) = &self.vault {
                if v.abs(&template).exists() {
                    v.index.write().unwrap().update(std::slice::from_ref(&path));
                    self.templater.run(&ctx, v, self.dv_settings.clone(), &template, &path.clone(), String::new(), String::new(), templater::Mode::NewNote { path: path.clone() });
                }
            }
        }
        self.ask = Some(Ask { title: "Nome da nova nota".into(), value: "Sem título".into(), kind: AskKind::Rename(path) });
    }

    /// A new storyboard (or infinite canvas) next to the open note.
    /// A storyboard of a kind (pages, thumbnails, webtoon…) in the folder of the open note.
    pub fn new_board_of(&mut self, o: ateliet_core::board::pages::KindOptions) {
        let Some(v) = &self.vault else { return };
        let folder = self.active_path().map(|p| vault::parent(&p).to_string()).unwrap_or_default();
        let name = match o.kind.as_str() {
            "canvas" => "Canvas",
            "name" => "Name",
            "comic" => "HQ",
            "thumbnails" => "Miniaturas",
            "webtoon" => "Webtoon",
            "presentation" => "Prancha",
            _ => "Storyboard",
        };
        let base = if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
        let path = v.available_path(&base, "storyboard");
        let doc = ateliet_core::board::pages::board_for_kind(&o);
        match crate::board_ui::BoardEditor::create_doc(&path, v.abs(&path), doc) {
            Ok(_) => {
                self.vault.as_mut().unwrap().rescan();
                self.open(&path, true);
            }
            Err(e) => self.toast(format!("Não foi possível criar {path}: {e}")),
        }
    }

    /// A new quadro (a Kanban note the plugin also reads) or an Excalidraw
    /// drawing (`.excalidraw.md`) in `folder`, opened and named.
    pub fn new_quadro(&mut self, folder: &str, drawing: bool) {
        let Some(v) = &self.vault else { return };
        let name = if drawing { "Desenho" } else { "Quadro" };
        let base = if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
        let (path, text) = if drawing {
            (v.available_path(&base, "excalidraw.md"), ateliet_core::quadro::excalidraw::new_md())
        } else {
            (v.available_path(&base, "md"), crate::quadro_ui::new_board_text())
        };
        if let Err(e) = v.write(&path, &text) {
            self.toast(format!("Não foi possível criar {path}: {e}"));
            return;
        }
        v.index.write().unwrap().update(std::slice::from_ref(&path));
        self.vault.as_mut().unwrap().rescan();
        self.open(&path, true);
        let stem = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".md").to_string();
        self.ask = Some(Ask { title: if drawing { "Nome do desenho".into() } else { "Nome do quadro".into() }, value: stem, kind: AskKind::Rename(path) });
    }

    /// A Clip Studio file as a new storyboard beside the active note.
    pub fn new_board_from_clip(&mut self, file: &std::path::Path) {
        let Some(v) = &self.vault else { return };
        let read = std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| ateliet_core::clipstudio::read(&b));
        let clip = match read {
            Ok(c) => c,
            Err(e) => return self.toast(format!("Não foi possível ler o arquivo: {e}")),
        };
        let folder = self.active_path().map(|p| vault::parent(&p).to_string()).unwrap_or_default();
        let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Storyboard".into());
        let base = if folder.is_empty() { name } else { format!("{folder}/{name}") };
        let path = v.available_path(&base, "storyboard");
        let (doc, approx) = ateliet_core::board::clip::board_from_clip(&clip, 24);
        match crate::board_ui::BoardEditor::create_doc(&path, v.abs(&path), doc) {
            Ok(_) => {
                self.vault.as_mut().unwrap().rescan();
                self.open(&path, true);
                self.toast(format!("{} camadas importadas.{}", clip.layers.len(), ateliet_core::board::clip::summary(&clip, &approx)));
            }
            Err(e) => self.toast(format!("Não foi possível criar {path}: {e}")),
        }
    }

    pub fn new_board(&mut self, kind: &str) {
        let Some(v) = &self.vault else { return };
        let folder = self.active_path().map(|p| vault::parent(&p).to_string()).unwrap_or_default();
        let name = if kind == "canvas" { "Canvas" } else { "Storyboard" };
        let base = if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
        let path = v.available_path(&base, "storyboard");
        let (w, h) = (1920, 1080);
        match crate::board_ui::BoardEditor::create(&path, v.abs(&path), kind, w, h) {
            Ok(mut b) => {
                b.save();
                self.vault.as_mut().unwrap().rescan();
                self.open(&path, true);
            }
            Err(e) => self.toast(format!("Não foi possível criar {path}: {e}")),
        }
    }

    /// Rename/move a file or folder; the open tabs and the links to it in
    /// other notes follow.
    pub fn rename(&mut self, from: &str, new_name: &str) {
        self.save_all();
        let Some(v) = &self.vault else { return };
        let dir = vault::parent(from);
        let ext = if from.to_lowercase().ends_with(".md") && !new_name.to_lowercase().ends_with(".md") { ".md" } else { "" };
        let to = if new_name.contains('/') || dir.is_empty() { format!("{new_name}{ext}") } else { format!("{dir}/{new_name}{ext}") };
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
        // what moves: the file, or every file in the folder
        let prefix = format!("{from}/");
        let moves: Vec<(String, String)> = if v.abs(from).is_dir() {
            v.files.iter().filter(|e| !e.folder && e.path.starts_with(&prefix)).map(|e| (e.path.clone(), format!("{to}/{}", &e.path[prefix.len()..]))).collect()
        } else {
            vec![(from.to_string(), to.clone())]
        };
        if let Err(e) = std::fs::rename(v.abs(from), v.abs(&to)) {
            self.toast(format!("Não foi possível renomear: {e}"));
            return;
        }
        // links to it, resolved with the index as it was before the move
        let mut relinked = 0;
        let update_links = self.cfg.as_ref().map(|c| c.read("app.json")["alwaysUpdateLinks"].as_bool().unwrap_or(true)).unwrap_or(true);
        if let Some(ix) = v.index.read().ok().filter(|_| update_links) {
            let mut sources: Vec<String> = moves.iter().flat_map(|(old, _)| ix.inlinks(old)).collect();
            sources.sort();
            sources.dedup();
            for src in sources {
                // a note that moved too is read where it is now
                let now = moves.iter().find(|(o, _)| *o == src).map(|(_, n)| n.clone()).unwrap_or_else(|| src.clone());
                let Ok(text) = std::fs::read_to_string(v.abs(&now)) else { continue };
                let moved = |t: &str| -> Option<String> {
                    let r = ix.resolve(t, &src)?;
                    moves.iter().find(|(o, _)| *o == r).map(|(_, n)| n.clone())
                };
                if let Some(new_text) = ateliet_core::relink::rewrite(&text, &moved) {
                    if v.write(&now, &new_text).is_ok() {
                        relinked += 1;
                        for t in &mut self.tabs {
                            if t.path == now || t.path == src {
                                t.text = new_text.clone();
                                t.saved = new_text.clone();
                            }
                        }
                    }
                }
            }
        }
        // the Enredo keeps chapters (and cards) by path: follow the move there too
        {
            use ateliet_core::scriptorium::{book as scb, enredo};
            let mut notes: Vec<String> = Vec::new();
            if let Ok(ix) = v.index.read() {
                for (old, _) in &moves {
                    if let Some(b) = scb::book_for(&ix, old, "") {
                        let p = enredo::path(&b);
                        if !notes.contains(&p) {
                            notes.push(p);
                        }
                    }
                }
            }
            for p in notes {
                let now = moves.iter().find(|(o, _)| *o == p).map(|(_, n)| n.clone()).unwrap_or(p);
                if let Err(e) = enredo::relink_file(&v.root, &now, &moves) {
                    eprintln!("enredo: {e}");
                }
            }
        }
        for t in &mut self.tabs {
            if let Some(crate::sc::View::Enredo(e)) = t.view.as_mut() {
                e.moved(&moves);
            }
            if t.path == from {
                t.path = to.clone();
            } else if let Some(rest) = t.path.strip_prefix(&prefix) {
                t.path = format!("{to}/{rest}");
            }
            if let Some(q) = t.quadro.as_mut() {
                q.moved(&t.path, v.abs(&t.path));
            }
        }
        let mut touched: Vec<String> = moves.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect();
        touched.push(from.to_string());
        touched.push(to.clone());
        v.index.write().unwrap().update(&touched);
        // a chapter renamed by its file: the title it had from the old name follows
        let retitle: Vec<(String, String)> = {
            use ateliet_core::scriptorium::book as scb;
            let ix = v.index.read().unwrap();
            moves
                .iter()
                .filter(|(o, n)| scb::title_from_name(o) != scb::title_from_name(n) && scb::is_chapter(&ix, n))
                .filter(|(o, n)| ix.page(n).and_then(|pg| pg.yaml.get("titulo")).and_then(|t| t.as_str()) == Some(scb::title_from_name(o).as_str()))
                .map(|(_, n)| (n.clone(), scb::title_from_name(n)))
                .collect()
        };
        self.vault.as_mut().unwrap().rescan();
        for (p, t) in retitle {
            self.edit_file(&p, |text| ateliet_core::scriptorium::set_frontmatter(text, "titulo", Some(&serde_json::json!(t))));
        }
        self.save_session();
        if relinked > 0 {
            self.toast(format!("Links atualizados em {relinked} {}.", if relinked == 1 { "nota" } else { "notas" }));
        }
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
            self.toast(format!("Não foi possível excluir: {e}"));
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
            Err(e) => self.toast(format!("Não foi possível criar a nota diária: {e}")),
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
            Action::EditPage(id) => {
                if let Some(src) = self.active_path() {
                    self.save_all();
                    self.open(&format!("scriptorium:pagina:{src}#{id}"), true);
                }
            }
            Action::Play(src) => {
                let source = self.active_path().unwrap_or_default();
                self.sc_apply(crate::sc::Action::Music(crate::music::Cmd::PlaySrc(src, source)));
            }
            Action::External(target) => {
                let source = self.active_path().unwrap_or_default();
                if let Some(v) = &self.vault {
                    match v.resolve(&target, &source) {
                        // books open in the reader, the rest in the system's app
                        Some(p) if ateliet_core::reader::format_of(&p).is_some() => self.open_book(&p, None, true),
                        Some(p) => {
                            let _ = open::that(v.abs(&p));
                        }
                        None => self.toast(format!("“{target}” não foi encontrado.")),
                    }
                }
            }
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

    /// Templater's "replace templates in the active file".
    pub fn replace_templates(&mut self, ctx: &egui::Context) {
        let Some(t) = self.tabs.get(self.active).filter(|t| t.view.is_none() && t.board.is_none() && t.quadro.is_none()) else { return };
        let path = t.path.clone();
        let text = t.text.clone();
        self.save_tab(self.active);
        let Some(v) = &self.vault else { return };
        self.templater.run(ctx, v, self.dv_settings.clone(), &path.clone(), &path.clone(), text, String::new(), templater::Mode::Replace { path });
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
            // the core Templates plugin's {{title}}, {{date}}, {{time}}
            let mut out = out;
            let target = match &f.mode {
                templater::Mode::Insert { path, .. } | templater::Mode::NewNote { path } | templater::Mode::Replace { path } => path.clone(),
            };
            let core = self.cfg.as_ref().map(|c| c.read("templates.json")).unwrap_or_default();
            let title = vault::basename(&target).trim_end_matches(".md").to_string();
            let replaced = templater::core_placeholders(&out.text, &title, &core);
            if replaced != out.text {
                // cursors keep their place only when nothing before them changed length; drop them otherwise
                out.cursors.clear();
                out.text = replaced;
            }
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
                templater::Mode::Replace { path } => {
                    if let Some(t) = self.tabs.iter_mut().find(|t| &t.path == path) {
                        t.text = out.text.clone();
                        t.last_edit = Some(Instant::now());
                    }
                    (path.clone(), 0)
                }
                templater::Mode::NewNote { path } => {
                    if let Some(v) = &self.vault {
                        let _ = v.write(path, &out.text);
                    }
                    self.vault.as_mut().unwrap().rescan();
                    self.open(path, true);
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
    /// Tab in a storyboard toggles its focus mode (Shift+Tab hides the
    /// timeline) instead of walking egui's focus through the buttons.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if ctx.wants_keyboard_input() {
            return;
        }
        // the same for a moodboard (studio and focus looks)
        if self.images_mode && self.images.mb.open && self.images.mb.board_open() {
            raw.events.retain(|e| match e {
                egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } if !modifiers.any() => {
                    if *pressed {
                        self.images.mb.tab_pressed = true;
                    }
                    false
                }
                _ => true,
            });
            return;
        }
        let Some(b) = self.tabs.get_mut(self.active).and_then(|t| t.board.as_mut()) else { return };
        raw.events.retain(|e| match e {
            egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } => {
                if *pressed {
                    b.tab_pressed = Some(modifiers.shift);
                }
                false
            }
            _ => true,
        });
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
        self.drops_and_paste(ctx);
        if !self.themed {
            self.palette = theme::apply(ctx, &self.appearance);
            self.themed = true;
        }
        if self.vault.is_none() {
            self.video.tick(ctx);
            self.video.compact_bar(ctx, self.palette);
            self.video.rail(ctx, self.palette);
            panels::vault_picker(self, ctx);
            self.video.sync(ctx, frame);
            return;
        }
        // changes on disk
        // the mouse's back and forward buttons
        let (mb, mf) = ctx.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        if mb || mf {
            self.go_history(mb);
        }
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
            self.images.changed(&changed);
            self.journal.changed(&changed);
            for p in &changed {
                self.caches.images.remove(p);
            }
        }
        let generation = self.vault.as_ref().unwrap().index.read().map(|ix| ix.generation).unwrap_or(0);
        if generation != self.last_generation {
            self.last_generation = generation;
            let active = self.active_path();
            if let (Some(sc), Some(v)) = (self.sc.as_mut(), self.vault.as_ref()) {
                sc.tick(v, active.as_deref());
            }
            ctx.request_repaint();
        }
        // tabs of the same note (one note in two panes) keep the same text
        self.sync_clones();
        // autosave a second after the last keystroke
        // the editors' spare layouts go when they have long been out of use
        for t in &mut self.tabs {
            t.highlight.trim();
        }
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
        self.music_tick(ctx);
        commands::shortcuts(self, ctx);
        self.video.tick(ctx);
        self.video.compact_bar(ctx, self.palette);

        self.sprint_tick();
        if self.focus.is_some() && !self.images_mode && !self.journal_mode {
            self.video.rail(ctx, self.palette);
            self.central(ctx);
            self.focus_hud(ctx);
            commands::show_palette(self, ctx);
            self.form_ui(ctx);
            panels::toasts(self, ctx);
            self.video.sync(ctx, frame);
            return;
        }
        let reading = !self.journal_mode && !self.images_mode && self.tabs.get(self.active).and_then(|t| t.reader.as_ref()).is_some_and(|r| r.immersive);
        if !(self.journal_mode && self.journal.immersive) && !reading {
            panels::ribbon(self, ctx);
        }
        if self.journal_mode {
            self.journal_mode_ui(ctx);
        } else if self.images_mode {
            self.image_mode(ctx);
        } else if reading {
            self.video.rail(ctx, self.palette);
            self.central(ctx);
        } else {
            // Preserve a writing column on small windows without changing the
            // user's sidebar preference; it returns when video is collapsed.
            if self.left_open && !(self.video.docked() && ctx.screen_rect().width() < 960.0) {
                panels::left(self, ctx);
            }
            if self.right_open || (self.video.docked() && self.side_reader.is_none()) {
                panels::right(self, ctx);
            }
            // the reader beside the notes, as beside the Journal
            self.side_reader_ui(ctx);
            panels::status_bar(self, ctx);
            panels::tab_bar(self, ctx);
            self.central(ctx);
        }

        self.updater.tick(ctx);
        if let Some(n) = self.updater.notice.take() {
            self.toast(n);
        }
        panels::update_window(self, ctx);
        commands::show_palette(self, ctx);
        panels::ask_dialog(self, ctx);
        self.form_ui(ctx);
        self.export_ui(ctx);
        self.picker_ui(ctx);
        if let Some(d) = self.new_board_dlg.as_mut() {
            match d.ui(ctx, self.palette) {
                Some(Some(o)) => {
                    let clip = d.clip.take();
                    self.new_board_dlg = None;
                    self.images_mode = false;
                    match clip {
                        Some(f) => self.new_board_from_clip(&f),
                        None => self.new_board_of(o),
                    }
                }
                Some(None) => self.new_board_dlg = None,
                None => {}
            }
        }
        panels::settings_window(self, ctx);
        self.templater.show_prompt(ctx, self.palette);
        panels::toasts(self, ctx);
        self.video.sync(ctx, frame);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.video.shutdown();
        if let (Some(v), Some(sc)) = (&self.vault, &self.sc) {
            let ix = v.index.read().unwrap();
            self.music.flush(&v.root, &ix, &sc.author());
        }
        if let Some(sc) = self.sc.as_mut() {
            sc.settings.save();
        }
        self.save_all();
        self.save_session();
        self.save_readers();
    }
}

impl App {
    /// Pictures of the vault moved to another folder: renamed with their links.
    fn move_pictures(&mut self, keys: Vec<String>, folder: &str) {
        let n = keys.len();
        for k in keys {
            let name = k.rsplit('/').next().unwrap_or(&k).to_string();
            let to = if folder.is_empty() { name } else { format!("{folder}/{name}") };
            self.rename(&k, &to);
        }
        self.images.rescan();
        self.toast(format!("{n} {} para {}", if n == 1 { "imagem movida" } else { "imagens movidas" }, if folder.is_empty() { "a raiz" } else { folder }));
    }

    fn image_mode(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let mut moves: Option<(Vec<String>, String)> = None;
        let mut dropped: Vec<String> = Vec::new();
        let mut drop_line: Option<usize> = None;
        if let Some(n) = self.images.notice.take() {
            self.toast(n);
        }
        if let Some(v) = &self.vault {
            let root = v.root.clone();
            self.images.open(&root, ctx);
        }
        egui::SidePanel::left("collections").resizable(true).show_separator_line(false).default_width(248.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 10, right: 10, top: 12, bottom: 10 })).show_animated(ctx, self.images.panels[0] && !(self.video.docked() && ctx.screen_rect().width() < 960.0), |ui| {
            let clip = ui.clip_rect();
            ui.painter().vline(clip.right() - 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
            if let Some(crate::images_ui::ImageAction::MoveTo(keys, folder)) = self.images.collections_ui(ui, p) {
                moves = Some((keys, folder));
            }
        });
        if let Some((keys, folder)) = moves {
            self.move_pictures(keys, &folder);
        }
        if self.video.docked() {
            ctx.animate_bool_with_time(egui::Id::new("image-notes").with("animation"), true, 0.0);
        }
        let notes_max = if self.video.docked() { (ctx.available_rect().width() - 320.0).clamp(260.0, 900.0) } else { 900.0 };
        egui::SidePanel::right("image-notes").resizable(true).show_separator_line(false).default_width(420.0).width_range(260.0..=notes_max).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::ZERO)).show_animated(ctx, self.images.panels[1] || self.video.docked(), |ui| {
            let clip = ui.clip_rect();
            ui.painter().vline(clip.left() + 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
            if self.video.docked() { egui::Frame::new().inner_margin(10).show(ui, |ui| self.video.dock(ui, p)); }
            if !self.images.panels[1] { return; }
            if self.tabs.is_empty() {
                panels::empty_state(self, ui);
            } else {
                let mut actions = Vec::new();
                egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 10, top: 8, bottom: 8 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let name = self.tabs.get(self.active).map(|t| t.title()).unwrap_or_default();
                        ui.label(crate::kit::icon(crate::icons::FILE_TEXT, 13.0).color(p.faint));
                        ui.label(egui::RichText::new(name).font(crate::kit::medium(13.5)).color(p.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            if crate::kit::icon_button(ui, crate::icons::PEN_LINE, "Voltar à escrita (Ctrl+Shift+I)", false, p).clicked() {
                                self.images_mode = false;
                            }
                            if crate::kit::icon_button(ui, crate::icons::SEARCH, "Trocar de nota", false, p).clicked() {
                                self.palette_ui = Some(commands::Palette::files());
                            }
                        });
                    });
                });
                let clip = ui.clip_rect();
                ui.painter().hline(clip.x_range(), ui.cursor().top() - 0.5, egui::Stroke::new(1.0_f32, p.border));
                self.note_ui(ui, &mut actions, false);
                for a in actions {
                    self.apply(a);
                }
            }
            // pictures dragged from the library or the viewer go into the note
            let panel = ui.max_rect();
            if !self.tabs.is_empty() {
                if let Some((_, keys, line)) = self.picture_drop(ui, &[(panel, self.active)]) {
                    dropped = keys;
                    drop_line = line;
                }
            } else if egui::DragAndDrop::has_payload_of_type::<Vec<usize>>(ctx) && ui.rect_contains_pointer(panel) {
                ui.painter().rect_stroke(panel.shrink(2.0), 6.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                if ctx.input(|i| i.pointer.any_released()) {
                    if let Some(idx) = egui::DragAndDrop::take_payload::<Vec<usize>>(ctx) {
                        dropped = self.images.keys_of(&idx);
                    }
                }
            }
        });
        if !dropped.is_empty() {
            if self.tabs.is_empty() {
                let folder = self.images.folder.clone().filter(|f| !std::path::Path::new(f).is_absolute()).unwrap_or_default();
                self.new_note(&folder);
                self.ask = None;
            }
            self.insert_pictures_at(&dropped, drop_line);
        }
        let mut act = None;
        // a moodboard runs to the edges; the mosaic has a margin
        let margin = if self.images.mb.open && self.images.mb.board_open() { egui::Margin::ZERO } else { egui::Margin { left: 18, right: 18, top: 14, bottom: 10 } };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(margin)).show(ctx, |ui| {
            if let Some(v) = &self.vault {
                act = self.images.center_ui(ui, p, v);
            }
        });
        match act {
            Some(crate::images_ui::ImageAction::Insert(keys)) => self.insert_pictures(&keys),
            Some(crate::images_ui::ImageAction::NewNote(keys)) => {
                let folder = self.images.folder.clone().filter(|f| !std::path::Path::new(f).is_absolute()).unwrap_or_default();
                self.new_note(&folder);
                self.ask = None;
                self.insert_pictures(&keys);
            }
            Some(crate::images_ui::ImageAction::OpenNote(n)) => self.open(&n, false),
            Some(crate::images_ui::ImageAction::MoveTo(keys, folder)) => self.move_pictures(keys, &folder),
            Some(crate::images_ui::ImageAction::CollectionNote(folder)) => self.collection_note(&folder),
            Some(crate::images_ui::ImageAction::TogglePanel(left)) => {
                let i = if left { 0 } else { 1 };
                self.images.panels[i] = !self.images.panels[i];
            }
            Some(crate::images_ui::ImageAction::SendToJournal(keys)) => {
                if self.journal.book.is_some() {
                    self.journal.pending_pictures.extend(keys);
                    self.images_mode = false;
                    self.journal_mode = true;
                } else {
                    self.toast("Abra um caderno no Journal para mandar imagens a ele.");
                }
            }
            None => {}
        }
    }

    /// The note of a picture folder, `<folder>/<folder>.md`, made with a
    /// heading when it doesn't exist yet.
    fn collection_note(&mut self, folder: &str) {
        let Some(v) = &self.vault else { return };
        let name = folder.rsplit('/').next().unwrap_or(folder);
        let path = format!("{folder}/{name}.md");
        if !v.abs(&path).exists() {
            if let Err(e) = v.write(&path, &format!("# {name}\n\n")) {
                self.toast(format!("Não foi possível criar a nota: {e}"));
                return;
            }
            self.vault.as_mut().unwrap().rescan();
        }
        self.open(&path, false);
    }

    /// A picture key as a vault path: one outside the vault is copied into the
    /// attachment folder (`importFolder`, or the open note's folder).
    pub fn picture_into_vault(&mut self, key: &str) -> String {
        let p = std::path::Path::new(key);
        let Some(v) = &self.vault else { return key.to_string() };
        if !p.is_absolute() {
            return key.to_string();
        }
        // already in the vault: its own path
        if let Ok(rel) = p.strip_prefix(&v.root) {
            return rel.to_string_lossy().replace('\\', "/");
        }
        let folder = self.attachment_folder();
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "imagem.png".into());
        let (stem, ext) = name.rsplit_once('.').map(|(a, b)| (a.to_string(), format!(".{b}"))).unwrap_or((name.clone(), String::new()));
        let mut rel = if folder.is_empty() { name.clone() } else { format!("{folder}/{name}") };
        let mut n = 1;
        while v.abs(&rel).exists() {
            n += 1;
            rel = if folder.is_empty() { format!("{stem} {n}{ext}") } else { format!("{folder}/{stem} {n}{ext}") };
        }
        if let Some(d) = v.abs(&rel).parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::copy(p, v.abs(&rel)) {
            Ok(_) => rel,
            Err(_) => key.to_string(),
        }
    }

    /// Where files brought into notes go: the images setting's import
    /// folder, else Obsidian's attachment folder (`/` the root, `./` the
    /// note's folder), else the note's folder.
    pub fn attachment_folder(&self) -> String {
        let Some(v) = &self.vault else { return String::new() };
        let st = crate::images_ui::ImagesState::settings(&v.root);
        if let Some(f) = st["importFolder"].as_str().filter(|s| !s.is_empty()) {
            return f.trim_matches('/').to_string();
        }
        let here = self.active_path().map(|a| vault::parent(&a).to_string()).unwrap_or_default();
        let att = self.cfg.as_ref().map(|c| c.read("app.json")["attachmentFolderPath"].as_str().unwrap_or("./").to_string()).unwrap_or_else(|| "./".into());
        match att.as_str() {
            "/" | "" => String::new(),
            "./" | "." => here,
            a if a.starts_with("./") => {
                let sub = a.trim_start_matches("./").trim_matches('/');
                if here.is_empty() { sub.to_string() } else { format!("{here}/{sub}") }
            }
            a => a.trim_matches('/').to_string(),
        }
    }

    /// Files dropped on the window, and an image pasted into a note.
    fn drops_and_paste(&mut self, ctx: &egui::Context) {
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && self.video.captures_drop(ctx) { return; }
        let board = self.tabs.get(self.active).is_some_and(|t| t.board.is_some());
        // in the Journal, the note behind it is not where things go
        let note = !self.journal_mode && self.tabs.get(self.active).is_some_and(|t| t.board.is_none() && t.quadro.is_none() && t.view.is_none() && t.path.to_lowercase().ends_with(".md"));
        if !dropped.is_empty() && !self.images_mode && !self.journal_mode {
            if board {
                if let Some(b) = self.tabs.get_mut(self.active).and_then(|t| t.board.as_mut()) {
                    b.add_reference_paths(&dropped);
                }
            } else if note || self.tabs.is_empty() {
                let keys: Vec<String> = dropped.iter().map(|p| p.to_string_lossy().to_string()).collect();
                self.insert_pictures(&keys);
                if let Some(v) = self.vault.as_mut() {
                    v.rescan();
                }
            }
        }
        // a picture on the clipboard (no text): saved as an attachment and embedded
        let paste = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(t) if t.is_empty()) || matches!(e, egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } if modifiers.command)));
        if paste && note && !self.images_mode {
            let Ok(mut cb) = arboard::Clipboard::new() else { return };
            if cb.get_text().is_ok_and(|t| !t.is_empty()) {
                return;
            }
            let Ok(img) = cb.get_image() else { return };
            let png = ateliet_core::board::tile::encode_png(&img.bytes, img.width as u32, img.height as u32);
            let folder = self.attachment_folder();
            let Some(v) = &self.vault else { return };
            let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
            let base = if folder.is_empty() { format!("Pasted image {stamp}") } else { format!("{folder}/Pasted image {stamp}") };
            let rel = v.available_path(&base, "png");
            if let Some(d) = v.abs(&rel).parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if std::fs::write(v.abs(&rel), png).is_ok() {
                self.vault.as_mut().unwrap().rescan();
                self.insert_pictures(&[rel]);
            }
        }
    }

    /// `![[picture]]` lines at the cursor of the open note (a new note when none is open).
    pub fn insert_pictures(&mut self, keys: &[String]) {
        self.insert_pictures_at(keys, None);
    }

    /// The same, before line `line` of the note when given (where a picture
    /// was dropped), else at the cursor.
    pub fn insert_pictures_at(&mut self, keys: &[String], line: Option<usize>) {
        if self.tabs.is_empty() {
            self.new_note("");
            self.ask = None;
        }
        // a storyboard takes them as references
        if self.tabs.get(self.active).is_some_and(|t| t.board.is_some()) {
            let Some(v) = &self.vault else { return };
            let paths: Vec<std::path::PathBuf> = keys.iter().map(|k| if std::path::Path::new(k).is_absolute() { std::path::PathBuf::from(k) } else { v.abs(k) }).collect();
            if let Some(b) = self.tabs[self.active].board.as_mut() {
                b.add_reference_paths(&paths);
            }
            return;
        }
        if self.tabs.get(self.active).is_none_or(|t| t.view.is_some() || t.quadro.is_some() || t.reader.is_some()) {
            self.toast("Abra uma nota (ou um storyboard) para pôr a imagem nela");
            return;
        }
        // pictures from outside the vault (shared library, external folders) are copied in first
        let keys: Vec<String> = keys.iter().map(|k| self.picture_into_vault(k)).collect();
        let embeds: String = keys.iter().map(|k| format!("![[{}]]\n", k.rsplit('/').next().unwrap_or(k))).collect();
        let Some(note) = self.embed_at(&embeds, line) else { return };
        self.toast(if keys.len() == 1 { format!("Imagem posta em {note}") } else { format!("{} imagens postas em {note}", keys.len()) });
    }

    /// Embed lines at the cursor of the note in use, as a block of their own
    /// and never inside the frontmatter. The note's name, or None when the tab
    /// in use is not a note.
    pub fn embed_at_cursor(&mut self, embeds: &str) -> Option<String> {
        self.embed_at(embeds, None)
    }

    /// Embed lines before line `line` of the note (its end past the last),
    /// or at the cursor; the same rules as `embed_at_cursor`.
    pub fn embed_at(&mut self, embeds: &str, line: Option<usize>) -> Option<String> {
        let t = self.tabs.get_mut(self.active).filter(|t| t.view.is_none() && t.board.is_none() && t.quadro.is_none() && t.reader.is_none())?;
        let mut b = match line {
            Some(l) => {
                let mut at = 0;
                for _ in 0..l {
                    match t.text[at..].find('\n') {
                        Some(j) => at += j + 1,
                        None => {
                            at = t.text.len();
                            break;
                        }
                    }
                }
                at
            }
            None => editor::char_to_byte(&t.text, t.cursor.min(t.text.chars().count())),
        };
        // never inside the frontmatter: after it
        if let Some(rest) = t.text.strip_prefix("---\n") {
            if let Some(i) = rest.find("\n---") {
                let end = 4 + i + 4;
                let end = t.text[end..].find('\n').map(|j| end + j + 1).unwrap_or(t.text.len());
                if b < end {
                    b = end;
                }
            }
        }
        // a picture is a block: never split a line, go to its end
        if b > 0 && !t.text[..b].ends_with('\n') {
            b = t.text[b..].find('\n').map(|j| b + j).unwrap_or(t.text.len());
        }
        let prefix = if b > 0 && !t.text[..b].ends_with('\n') { "\n" } else { "" };
        t.text.insert_str(b, &format!("{prefix}{embeds}"));
        t.cursor = editor::byte_to_char(&t.text, b) + prefix.chars().count() + embeds.chars().count();
        t.last_edit = Some(Instant::now());
        t.live.close();
        Some(vault::basename(&t.path).trim_end_matches(".md").to_string())
    }

    /// One pane of the centre: the active tab (a Scriptorium screen, a
    /// storyboard or a note).
    fn pane_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>, sc_actions: &mut Vec<crate::sc::Action>, open_board: &mut Option<String>, centered: bool) {
        let p = self.palette;
        if self.tabs.get(self.active).is_some_and(|t| t.reader.is_some()) {
            self.reader_pane(ui);
            return;
        }
        if self.tabs.get(self.active).is_some_and(|t| t.view.is_some()) {
            let acts = {
                let (Some(v), Some(sc)) = (self.vault.as_ref(), self.sc.as_mut()) else { return };
                let mut cx = crate::sc::library::Cx { vault: v, sc, caches: &mut self.caches, palette: p, dv: &self.dv_settings, music: &self.music };
                match self.tabs[self.active].view.as_mut().unwrap() {
                    crate::sc::View::Library(l) => l.ui(ui, &mut cx),
                    crate::sc::View::Cast(c) => c.ui(ui, &mut cx),
                    crate::sc::View::Sheet(s) => s.ui(ui, &mut cx),
                    crate::sc::View::Enredo(e) => e.ui(ui, &mut cx),
                    crate::sc::View::Mundo(m) => m.ui(ui, &mut cx),
                    crate::sc::View::Mapa(m) => m.ui(ui, &mut cx),
                    crate::sc::View::Trilha(t) => t.ui(ui, &mut cx),
                    crate::sc::View::Diagramacao(d) => d.ui(ui, &mut cx),
                    crate::sc::View::Capa(c) => c.ui(ui, &mut cx),
                    crate::sc::View::Pagina(c) => c.ui(ui, &mut cx),
                    crate::sc::View::Storyboards(h) => h.ui(ui, &mut cx),
                }
            };
            // a map can walk into its submaps: the tab follows
            let t = &mut self.tabs[self.active];
            if let Some(crate::sc::View::Mapa(m)) = t.view.as_ref() {
                if t.path != m.path {
                    t.path = m.path.clone();
                }
            }
            // the Enredo can move to another book of the series
            if let Some(crate::sc::View::Enredo(e)) = t.view.as_ref() {
                let want = format!("scriptorium:enredo:{}", e.book);
                if t.path != want {
                    t.path = want;
                }
            }
            *sc_actions = acts;
            return;
        }
        if self.tabs.get(self.active).is_some_and(|t| t.quadro.is_some()) {
            let outs = {
                let Some(v) = self.vault.as_ref() else { return };
                let mut cx = crate::quadro_ui::Cx { vault: v, caches: &mut self.caches, palette: p };
                self.tabs[self.active].quadro.as_mut().unwrap().ui(ui, &mut cx)
            };
            for o in outs {
                match o {
                    crate::quadro_ui::Out::Open(link) => self.open_link(&link, true),
                    crate::quadro_ui::Out::Toast(t) => self.toast(t),
                    crate::quadro_ui::Out::Rescan => {
                        if let Some(v) = self.vault.as_mut() {
                            v.rescan();
                        }
                    }
                }
            }
            return;
        }
        if let Some(b) = self.tabs.get_mut(self.active).and_then(|t| t.board.as_mut()) {
            let msg = b.ui(ui, p);
            *open_board = b.open.take();
            if let Some(msg) = msg {
                self.toast(msg);
            }
            return;
        }
        self.note_ui(ui, actions, centered);
    }

    /// Pictures dragged over the panes (from the side panel): a note shows
    /// where they will go (the line between blocks in Live Preview, else the
    /// cursor or the end), a storyboard takes them as references. On release:
    /// (tab, picture keys, line).
    fn picture_drop(&mut self, ui: &egui::Ui, panes: &[(egui::Rect, usize)]) -> Option<(usize, Vec<String>, Option<usize>)> {
        let ctx = ui.ctx();
        if !egui::DragAndDrop::has_payload_of_type::<Vec<usize>>(ctx) {
            return None;
        }
        let pos = ctx.pointer_latest_pos()?;
        let &(rect, i) = panes.iter().find(|(r, _)| r.contains(pos))?;
        let t = self.tabs.get(i)?;
        let note = t.view.is_none() && t.board.is_none() && t.quadro.is_none() && t.reader.is_none();
        if !note && t.board.is_none() {
            return None;
        }
        let p = self.palette;
        let painter = ui.painter_at(rect);
        painter.rect_stroke(rect.shrink(2.0), 6.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
        let spot = if note { t.live.drop_line(pos.y) } else { None };
        if let Some((_, y)) = spot {
            let y = y.clamp(rect.top() + 4.0, rect.bottom() - 4.0);
            painter.hline((rect.left() + 24.0)..=(rect.right() - 24.0), y, egui::Stroke::new(2.0_f32, p.accent_strong));
            painter.circle_filled(egui::pos2(rect.left() + 24.0, y), 3.5, p.accent_strong);
        }
        // the end of the note when nothing tells where (the reading view)
        let line = spot.map(|s| s.0).or_else(|| (note && t.mode == Mode::Read).then(|| t.text.split('\n').count()));
        if !ctx.input(|inp| inp.pointer.any_released()) {
            return None;
        }
        let idx = egui::DragAndDrop::take_payload::<Vec<usize>>(ctx)?;
        Some((i, self.images.keys_of(&idx), line))
    }

    fn central(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let frame = egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::ZERO);
        let mut actions: Vec<Action> = Vec::new();
        let mut sc_actions = Vec::new();
        let mut open_board: Option<String> = None;
        let mut dropped = None;
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            if self.tabs.is_empty() {
                panels::empty_state(self, ui);
                return;
            }
            self.fix_panes();
            let full = ui.available_rect_before_wrap();
            let Some(tree) = self.panes.clone() else {
                self.pane_ui(ui, &mut actions, &mut sc_actions, &mut open_board, true);
                dropped = self.picture_drop(ui, &[(full, self.active)]);
                if let Some(uid) = self.tabs.get(self.active).map(|t| t.uid) {
                    self.dock_ui(ui, &[(uid, full)]);
                }
                return;
            };
            let mut tree = tree;
            tree.fit(full);
            let (rects, handles) = tree.layout(full);
            self.panes = Some(tree);
            let grip = |h: &crate::panes::Handle| {
                if h.down {
                    egui::Rect::from_center_size(egui::pos2(h.span.center().x, h.at), egui::vec2(h.span.width(), 9.0))
                } else {
                    egui::Rect::from_center_size(egui::pos2(h.at, h.span.center().y), egui::vec2(9.0, h.span.height()))
                }
            };
            let grips: Vec<egui::Rect> = handles.iter().map(grip).collect();
            let focused = self.active;
            let mut focus_to = None;
            let mut close = None;
            let mut bodies = Vec::new();
            for &(uid, rect) in &rects {
                let Some(i) = self.uid_index(uid) else { continue };
                let pressed = ui.input(|inp| inp.pointer.any_pressed() && inp.pointer.interact_pos().is_some_and(|q| rect.contains(q) && !grips.iter().any(|g| g.contains(q))));
                if pressed && i != focused {
                    focus_to = Some(i);
                }
                let head = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + PANE_HEAD));
                let body = egui::Rect::from_min_max(egui::pos2(rect.min.x, head.max.y), rect.max);
                if self.pane_head(ui, head, i, i == focused) {
                    close = Some(uid);
                }
                self.active = i;
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body).id_salt(("pane", uid)), |ui| {
                    ui.set_clip_rect(body);
                    self.pane_ui(ui, &mut actions, &mut sc_actions, &mut open_board, true);
                });
                self.active = focused;
                bodies.push((body, i));
            }
            // the lines between the panes move; a double click evens the two around it
            for (h, g) in handles.iter().zip(&grips) {
                let hr = ui.interact(*g, ui.id().with(("pane-line", &h.path, h.k)), egui::Sense::click_and_drag());
                if hr.hovered() || hr.dragged() {
                    ui.ctx().set_cursor_icon(if h.down { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
                }
                if hr.dragged() {
                    if let (Some(q), Some(n)) = (hr.interact_pointer_pos(), self.panes.as_mut()) {
                        n.drag(h, if h.down { q.y } else { q.x });
                    }
                }
                if hr.drag_stopped() {
                    self.save_session();
                }
                if hr.double_clicked() {
                    if let Some(n) = self.panes.as_mut() {
                        n.even(h);
                    }
                    self.save_session();
                }
                let line = if hr.hovered() || hr.dragged() { egui::Stroke::new(2.0_f32, p.accent_strong) } else { egui::Stroke::new(1.0_f32, crate::theme::mix(p.bg, p.border, 0.65)) };
                if h.down {
                    ui.painter().hline(h.span.x_range(), h.at, line);
                } else {
                    ui.painter().vline(h.at, h.span.y_range(), line);
                }
            }
            dropped = self.picture_drop(ui, &bodies);
            self.dock_ui(ui, &rects);
            if let Some(i) = focus_to.filter(|&i| i < self.tabs.len()) {
                // the other pane becomes the one in use; the panes stay where they are
                self.active = i;
                self.pane_focus = self.tabs[i].uid;
            }
            if let Some(uid) = close {
                self.close_pane(uid);
            }
        });
        // pictures dragged from the side panel: into the pane they were dropped on
        if let Some((i, keys, line)) = dropped {
            if i != self.active && i < self.tabs.len() {
                self.active = i;
                self.pane_focus = self.tabs[i].uid;
            }
            self.insert_pictures_at(&keys, line);
        }
        // a storyboard the editor made (thumbnails turned into an animatic)
        if let Some(path) = open_board {
            if let Some(v) = self.vault.as_mut() {
                v.rescan();
            }
            self.open(&path, true);
        }
        self.hover_preview(ctx, &mut actions);
        for a in actions {
            self.apply(a);
        }
        for a in sc_actions {
            self.sc_apply(a);
        }
        self.sync_clones();
    }

    /// The strip on top of each pane when the centre is divided: what it
    /// shows, a grip to drag it elsewhere, and a button to close it. True
    /// when that button was clicked.
    fn pane_head(&mut self, ui: &mut egui::Ui, rect: egui::Rect, i: usize, focused: bool) -> bool {
        let p = self.palette;
        let t = &self.tabs[i];
        let uid = t.uid;
        let painter = ui.painter_at(rect);
        // One continuous page: focus lives in the title, not in a coloured
        // rule or a second shaded toolbar above the note.
        painter.rect_filled(rect, 0.0, p.bg);
        let xr = egui::Rect::from_center_size(egui::pos2(rect.right() - 18.0, rect.center().y), egui::vec2(24.0, 24.0));
        let drag_rect = egui::Rect::from_min_max(rect.min, egui::pos2(xr.left() - 2.0, rect.max.y));
        let r = ui.interact(drag_rect, ui.id().with(("pane-head", uid)), egui::Sense::click_and_drag());
        if r.drag_started() {
            self.pane_drag = Some(uid);
        }
        if r.hovered() && self.pane_drag.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let cy = rect.center().y;
        let ink = if focused { p.text } else { p.muted };
        let copies = if t.is_note() { self.tabs.iter().filter(|o| o.path == t.path && o.is_note()).count() } else { 1 };
        let twin = copies > 1;
        let title = format!("{}{}{}", t.title(), if twin { format!("  ×{copies}") } else { String::new() }, if t.dirty() { " •" } else { "" });
        let mut job = egui::text::LayoutJob::single_section(title, egui::TextFormat { font_id: if focused { crate::kit::medium(12.0) } else { egui::FontId::proportional(12.0) }, color: if focused { p.text } else { p.muted }, ..Default::default() });
        // Reserve the close target even while hidden so titles never jump.
        job.wrap = egui::text::TextWrapping::truncate_at_width((xr.left() - rect.left() - 22.0).max(0.0));
        let galley = ui.fonts(|f| f.layout_job(job));
        painter.galley(egui::pos2(rect.left() + 16.0, cy - galley.size().y / 2.0), galley, ink);
        if twin {
            r.clone().on_hover_text("A mesma nota em outro painel: o que escrever num aparece no outro. Arraste para mudar de lugar.");
        } else {
            r.clone().on_hover_text("Arraste para mudar o painel de lugar: solte na borda de outro painel para pô-lo ao lado, acima ou abaixo, ou no meio para trocar os dois.");
        }
        let xresp = ui.interact(xr, ui.id().with(("pane-close", uid)), egui::Sense::click()).on_hover_text("Fechar o painel");
        if xresp.hovered() {
            painter.rect_filled(xr, 5.0, p.bg_active);
        }
        if ui.rect_contains_pointer(rect) || xresp.has_focus() {
            painter.text(xr.center(), egui::Align2::CENTER_CENTER, crate::icons::X, crate::kit::icon_font(12.0), p.muted);
        }
        xresp.clicked()
    }

    /// A pane (or a tab from the bar) dragged over the panes: the part of the
    /// pane under the pointer it would take is shown, and on release it goes
    /// there.
    fn dock_ui(&mut self, ui: &egui::Ui, panes: &[(u64, egui::Rect)]) {
        let Some(uid) = self.pane_drag else { return };
        let ctx = ui.ctx();
        if self.uid_index(uid).is_none() {
            self.pane_drag = None;
            return;
        }
        let pos = ctx.pointer_latest_pos();
        let released = ctx.input(|i| i.pointer.any_released() || !i.pointer.any_down());
        let spot = pos.and_then(|q| panes.iter().find(|(_, r)| r.contains(q)).map(|&(t, r)| (t, r, crate::panes::Side::at(r, q))));
        if released {
            self.pane_drag = None;
            if let Some((target, _, side)) = spot {
                self.dock(uid, target, side);
            }
            return;
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let Some((target, rect, side)) = spot else { return };
        if target == uid {
            return;
        }
        let p = self.palette;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("pane-dock")));
        let zone = side.preview(rect).shrink(4.0);
        painter.rect(zone, 8.0, p.accent_strong.gamma_multiply(0.18), egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
        let hint = match side {
            crate::panes::Side::Left => "À esquerda",
            crate::panes::Side::Right => "À direita",
            crate::panes::Side::Top => "Acima",
            crate::panes::Side::Bottom => "Abaixo",
            crate::panes::Side::Center => "Neste lugar",
        };
        painter.text(zone.center(), egui::Align2::CENTER_CENTER, hint, crate::kit::medium(13.0), p.accent_strong);
        ctx.request_repaint();
    }

    /// Hovering a note link for a moment shows the note in a small window,
    /// which stays while the pointer is over it.
    fn hover_preview(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let now_frame = ctx.cumulative_pass_nr();
        let hovered: Option<(String, egui::Pos2, u64)> = ctx.data(|d| d.get_temp(egui::Id::new("md-hover-link")));
        let fresh = hovered.filter(|h| h.2 + 1 >= now_frame);
        let ptr = ctx.input(|i| i.pointer.hover_pos());
        let over_popup = matches!((self.hover_rect, ptr), (Some(r), Some(q)) if r.expand(8.0).contains(q));
        match (&fresh, &self.hover) {
            (Some((t, pos, _)), Some((h, _, _))) if t == h => {
                let _ = pos;
            }
            (Some((t, pos, _)), _) => self.hover = Some((t.clone(), Instant::now(), *pos)),
            (None, _) if over_popup => {}
            (None, _) => {
                self.hover = None;
                self.hover_rect = None;
            }
        }
        let Some((target, since, pos)) = self.hover.clone() else { return };
        if since.elapsed() < Duration::from_millis(450) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let Some(vault) = self.vault.as_ref() else { return };
        let source = self.active_path().unwrap_or_default();
        let p = self.palette;
        let size = self.appearance.font_size * 0.9;
        let mut host = RenderHost { vault, palette: p, size, source: &source, caches: &mut self.caches, settings: &self.dv_settings };
        let Some((path, doc)) = host.embed(&target) else { return };
        let area = egui::Area::new(egui::Id::new("link-preview")).order(egui::Order::Foreground).fixed_pos(pos + egui::vec2(12.0, 16.0)).constrain(true);
        let resp = area.show(ctx, |ui| {
            egui::Frame::new().fill(p.bg).stroke(egui::Stroke::new(1.0_f32, p.border)).corner_radius(10.0).shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(50) }).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                ui.set_width(420.0);
                ui.label(egui::RichText::new(vault::basename(&path).trim_end_matches(".md")).font(crate::kit::medium(14.0)).color(p.muted));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(320.0).id_salt("link-preview-scroll").show(ui, |ui| {
                    let mut r = Renderer::new(&mut host);
                    r.blocks(ui, &doc.blocks);
                    actions.append(&mut r.actions);
                });
            });
        });
        self.hover_rect = Some(resp.response.rect);
    }

    /// The open note: title and editor / Live Preview / reading view.
    fn note_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>, centered: bool) {
        let p = self.palette;
        {
            let size = self.appearance.font_size;
            let readable = self.readable;
            let active = self.active;
            let Some(vault) = self.vault.as_ref() else { return };
            let tab = &mut self.tabs[active];
            if tab.find.bar(ui, &tab.path, &mut tab.text) {
                tab.last_edit = Some(std::time::Instant::now());
                tab.live.buf.clone_from(&tab.text);
                tab.live.buf_end = tab.text.split('\n').count().saturating_sub(1);
            }
            crate::find::set_highlight(&tab.find);
            tab.live.spots.clear();
            crate::align::set_current(crate::align::TextAlign::of_note(&tab.text));
            let max_w = if readable && centered { crate::typo::get().line_width.unwrap_or(46.0 * size) } else { f32::INFINITY };
            // a narrow pane (the centre divided) keeps its room for the text
            let pad = if centered && ui.available_width() > 420.0 { 32.0 } else { 14.0 };
            let salt = ("note", tab.path.clone(), tab.mode == Mode::Read, centered);
            // the editor's lines left for later (a long note just opened, or another width)
            let editing = tab.mode == Mode::Edit && (!self.live_preview || tab.live.active.is_some());
            let grown = if editing && tab.highlight.incomplete() {
                let st = if self.live_preview { editor::Styles::live(size, p) } else { editor::Styles::source(size, p) };
                let reveal = editor::reveal_of(ui.ctx(), tab.editor_id(), &tab.text);
                let busy = ui.input(|i| !i.events.is_empty() || i.pointer.any_down() || i.smooth_scroll_delta != egui::Vec2::ZERO);
                ui.ctx().fonts(|f| tab.highlight.advance(f, &tab.text, &st, reveal, busy))
            } else {
                0.0
            };
            // what the rendered note measured out of view last frame moves the view with it
            let area = crate::heights::keep_view(ui, egui::ScrollArea::vertical().id_salt(&salt), &salt, &mut tab.live.heights, grown);
            area.auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_width();
                let w = avail.min(max_w + 2.0 * pad);
                let side = ((avail - w) / 2.0).max(0.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(side + pad);
                    ui.vertical(|ui| {
                        ui.set_width((w - 2.0 * pad).max(140.0));
                        if centered {
                            ui.add_space(28.0);
                            // inline title
                            ui.label(egui::RichText::new(vault::basename(&tab.path)).size(size * 1.9).family(egui::FontFamily::Name("theme-text-bold".into())).color(p.text));
                            ui.add_space(10.0);
                        }
                        // writing tools ("/" menu, selection bar) in chapters
                        if tab.mode == Mode::Edit {
                            let author = self.sc.as_ref().map(|s| s.author()).unwrap_or_default();
                            let ix = vault.index.read().unwrap();
                            let chars = ateliet_core::scriptorium::book::is_chapter(&ix, &tab.path).then(|| ateliet_core::scriptorium::book::book_for(&ix, &tab.path, &author).map(|b| ateliet_core::scriptorium::book::characters(&ix, &b)).unwrap_or_default());
                            crate::writing::set_chapter(chars);
                        } else {
                            crate::writing::set_chapter(None);
                        }
                        match tab.mode {
                            Mode::Edit if self.live_preview => {
                                let st = editor::Styles::live(size, p);
                                let path = tab.path.clone();
                                let mut host = RenderHost { vault, palette: p, size, source: &path, caches: &mut self.caches, settings: &self.dv_settings };
                                let mut acts = crate::live::show(ui, tab, &mut host, &st);
                                actions.append(&mut acts);
                            }
                            // the source, with Dataview results shown in place
                            Mode::Edit if !tab.find.open && !crate::live::query_blocks(&tab.text).is_empty() => {
                                let st = editor::Styles::source(size, p);
                                let blocks = crate::live::query_blocks(&tab.text);
                                let path = tab.path.clone();
                                let mut host = RenderHost { vault, palette: p, size, source: &path, caches: &mut self.caches, settings: &self.dv_settings };
                                let mut acts = crate::live::source_with_queries(ui, tab, &mut host, &st, &blocks);
                                actions.append(&mut acts);
                            }
                            Mode::Edit => {
                                let st = editor::Styles::source(size, p);
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
                                if let Some(range) = tab.find.pending.take() {
                                    crate::find::request_scroll(ui.ctx(), id, range);
                                }
                                let before = tab.text.len();
                                let before_hash = {
                                    let mut h = DefaultHasher::new();
                                    tab.text.hash(&mut h);
                                    h.finish()
                                };
                                let resp = ui.add(editor::editor_widget(&mut tab.text, id, &mut tab.highlight, &st, 30));
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
                            Mode::Read if tab.find.open => {
                                // Source line ranges let a match in any paragraph (not only a
                                // heading) navigate without leaving the reading view.
                                tab.live.refresh(&tab.text);
                                let target = tab.find.pending.take();
                                let line = target.as_ref().map(|r| tab.text[..r.start].bytes().filter(|b| *b == b'\n').count());
                                let mut host = RenderHost { vault, palette: p, size, source: &tab.path, caches: &mut self.caches, settings: &self.dv_settings };
                                let gap = size * crate::typo::get().gap;
                                let width = ui.available_width();
                                // only the segments in view (and the one searched for) are drawn
                                let mut heights = std::mem::take(&mut tab.live.heights);
                                let mut pass = heights.pass(ui);
                                for (i, seg) in tab.live.segs.iter().enumerate() {
                                    if i > 0 { ui.add_space(gap); }
                                    let is_target = line.is_some_and(|l| seg.start <= l && l <= seg.end);
                                    if is_target {
                                        let offset: usize = tab.text.split_inclusive('\n').take(seg.start).map(str::len).sum();
                                        if let Some(range) = &target {
                                            crate::find::begin_target(&seg.text, range.start.saturating_sub(offset).min(seg.text.len()));
                                        }
                                    }
                                    let rect = pass.piece(ui, seg.hash ^ 0x5eed_0f_f1d, || crate::heights::estimate(&seg.text, size, width), is_target, |ui| {
                                        let mut r = Renderer::new(&mut host);
                                        if seg.frontmatter {
                                            crate::find::plain(ui, &seg.text, egui::FontId::monospace(size), p.muted);
                                        } else {
                                            r.blocks(ui, &seg.doc.blocks);
                                        }
                                        actions.append(&mut r.actions);
                                    });
                                    if is_target && !crate::find::end_target() {
                                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                                    }
                                }
                                pass.finish(ui);
                                tab.live.heights = heights;
                            }
                            Mode::Read => {
                                let doc = tab.doc();
                                let doc_key = editor::text_hash(&tab.text);
                                let mut host = RenderHost { vault, palette: p, size, source: &tab.path, caches: &mut self.caches, settings: &self.dv_settings };
                                let mut r = Renderer::new(&mut host);
                                r.scroll_to = tab.scroll_to.take();
                                // only the blocks in view (and a heading scrolled to) are drawn
                                let mut heights = std::mem::take(&mut tab.live.heights);
                                let mut pass = heights.pass(ui);
                                let line = r.typo.line;
                                for (i, b) in doc.blocks.iter().enumerate() {
                                    if i > 0 {
                                        // as Renderer::blocks spaces them
                                        let gap = match b {
                                            markdown::Block::Space(_) => 0.0,
                                            markdown::Block::Heading { level, .. } => size * if *level <= 2 { 1.5 } else { 1.1 },
                                            _ => size * r.typo.gap,
                                        };
                                        ui.add_space(gap);
                                    }
                                    let target = matches!(b, markdown::Block::Heading { line, .. } if r.scroll_to == Some(*line));
                                    pass.piece(ui, doc_key ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15), || size * line * 2.0, target, |ui| r.block(ui, b));
                                }
                                pass.finish(ui);
                                tab.live.heights = heights;
                                actions.append(&mut r.actions);
                            }
                        }
                        ui.add_space(200.0);
                    });
                });
            });
            crate::find::clear_highlight();
            crate::align::set_current(crate::align::TextAlign::Left);
        }
    }
}
