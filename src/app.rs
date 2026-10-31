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

    pub fn editor_id(&self) -> egui::Id {
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
    /// A split: the path of the tab in the other pane, and whether that pane is on the right.
    pub split: Option<String>,
    pub split_right: bool,
    /// Where the line between the two panes is, as a fraction of the width.
    pub split_frac: f32,
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
            split: None,
            split_right: true,
            split_frac: 0.5,
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
            for t in tabs.iter().filter_map(|t| t.as_str()) {
                if self.vault.as_ref().unwrap().abs(t).is_file() || crate::sc::View::from_path(t).is_some() || t == crate::reader_ui::SHELF {
                    self.open(t, true);
                }
            }
            self.active = session["active"].as_u64().unwrap_or(0) as usize;
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            }
            // the split as it was left
            self.split = session["split"].as_str().filter(|sp| self.tabs.iter().enumerate().any(|(i, t)| t.path == *sp && i != self.active)).map(str::to_string);
            self.split_right = session["split_right"].as_bool().unwrap_or(true);
            self.split_frac = session["split_frac"].as_f64().map(|f| (f as f32).clamp(0.2, 0.8)).unwrap_or(0.5);
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
            let _ = std::fs::write(path, serde_json::json!({ "tabs": tabs, "active": self.active, "recent": self.recent, "split": self.split, "split_right": self.split_right, "split_frac": self.split_frac }).to_string());
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
            let tab = Tab { path: path.to_string(), text: String::new(), saved: String::new(), mode: Mode::Read, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: Some(view), reader: None, back: Vec::new(), fwd: Vec::new() };
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
                let tab = Tab { path: path.to_string(), text: String::new(), saved: String::new(), mode: Mode::Read, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: Some(view), reader: None, back: Vec::new(), fwd: Vec::new() };
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
        let mut tab = Tab { path: path.to_string(), saved: text.clone(), text, mode: self.default_mode, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: None, reader: None, back: Vec::new(), fwd: Vec::new() };
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
        let path = self.tabs.remove(i).path;
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        } else if i < self.active {
            self.active -= 1;
        }
        // a pane of a split closed: the other one takes the whole centre
        if let Some(sp) = self.split.take() {
            if sp != path {
                match self.tabs.iter().position(|t| t.path == sp) {
                    Some(j) if closing_active => self.active = j,
                    Some(_) => self.split = Some(sp),
                    None => {}
                }
            }
        }
        self.save_session();
    }

    /// Make tab `i` the one in use. When it is the one shown in the other
    /// pane of a split, the two panes stay as they are and only the focus moves.
    pub fn focus_tab(&mut self, i: usize) {
        if i >= self.tabs.len() || i == self.active {
            return;
        }
        if self.split.as_deref() == Some(self.tabs[i].path.as_str()) {
            self.split = self.tabs.get(self.active).map(|t| t.path.clone());
            self.split_right = !self.split_right;
        }
        self.active = i;
    }

    /// Whether the centre is divided in two panes now.
    pub fn split_shown(&self) -> bool {
        self.split.as_ref().is_some_and(|sp| self.tabs.iter().enumerate().any(|(i, t)| &t.path == sp && i != self.active))
    }

    /// Open a file in a pane beside the one in use (Obsidian's "Open to the
    /// right"); the new pane is the one in use.
    pub fn open_beside(&mut self, path: &str) {
        let Some(before) = self.active_path() else {
            self.open(path, true);
            return;
        };
        if before == path {
            return;
        }
        // shown in the other pane already: just go there
        if self.split_shown() && self.split.as_deref() == Some(path) {
            if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
                self.focus_tab(i);
            }
            return;
        }
        self.split = None;
        self.open(path, true);
        if self.active_path().as_deref() == Some(path) {
            self.split = Some(before);
            self.split_right = false;
        }
        self.save_session();
    }

    /// Divide the centre in two panes, or join them again. The other pane
    /// shows the neighbouring tab or, with a single tab, the last note seen.
    pub fn toggle_split(&mut self) {
        if self.split_shown() {
            self.split = None;
        } else if self.tabs.len() > 1 {
            let j = if self.active > 0 { self.active - 1 } else { self.active + 1 };
            self.split = Some(self.tabs[j].path.clone());
            self.split_right = true;
        } else {
            let here = self.active_path();
            let other = self.vault.as_ref().and_then(|v| self.recent.iter().find(|p| Some(*p) != here.as_ref() && v.abs(p).is_file()).cloned());
            match (here, other) {
                (Some(_), Some(o)) => {
                    let i = self.active;
                    self.open(&o, true);
                    // the note in use stays in use, on the left
                    self.split = self.active_path();
                    self.split_right = true;
                    self.active = i;
                }
                _ => self.toast("Abra outra nota para dividir a tela."),
            }
        }
        self.save_session();
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
        for t in &mut self.tabs {
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
        self.vault.as_mut().unwrap().rescan();
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

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
            self.video.ui(ctx, self.palette);
            panels::vault_picker(self, ctx);
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
        self.music_tick(ctx);
        commands::shortcuts(self, ctx);
        self.video.tick(ctx);
        self.video.ui(ctx, self.palette);

        self.sprint_tick();
        if self.focus.is_some() && !self.images_mode && !self.journal_mode {
            self.central(ctx);
            self.focus_hud(ctx);
            commands::show_palette(self, ctx);
            self.form_ui(ctx);
            panels::toasts(self, ctx);
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
            self.central(ctx);
        } else {
            if self.left_open {
                panels::left(self, ctx);
            }
            if self.right_open {
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
        if let Some(n) = self.images.notice.take() {
            self.toast(n);
        }
        if let Some(v) = &self.vault {
            let root = v.root.clone();
            self.images.open(&root, ctx);
        }
        egui::SidePanel::left("collections").resizable(true).show_separator_line(false).default_width(248.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 10, right: 10, top: 12, bottom: 10 })).show_animated(ctx, self.images.panels[0], |ui| {
            let clip = ui.clip_rect();
            ui.painter().vline(clip.right() - 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
            if let Some(crate::images_ui::ImageAction::MoveTo(keys, folder)) = self.images.collections_ui(ui, p) {
                moves = Some((keys, folder));
            }
        });
        if let Some((keys, folder)) = moves {
            self.move_pictures(keys, &folder);
        }
        egui::SidePanel::right("image-notes").resizable(true).show_separator_line(false).default_width(420.0).width_range(260.0..=900.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::ZERO)).show_animated(ctx, self.images.panels[1], |ui| {
            let clip = ui.clip_rect();
            ui.painter().vline(clip.left() + 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
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
            if egui::DragAndDrop::has_payload_of_type::<Vec<usize>>(ctx) && ui.rect_contains_pointer(panel) {
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
            self.insert_pictures(&dropped);
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
        if self.tabs.is_empty() {
            self.new_note("");
            self.ask = None;
        }
        // pictures from outside the vault (shared library, external folders) are copied in first
        let keys: Vec<String> = keys.iter().map(|k| self.picture_into_vault(k)).collect();
        let embeds: String = keys.iter().map(|k| format!("![[{}]]\n", k.rsplit('/').next().unwrap_or(k))).collect();
        let Some(note) = self.embed_at_cursor(&embeds) else { return };
        self.toast(format!("{} picture(s) added to {note}", keys.len()));
    }

    /// Embed lines at the cursor of the note in use, as a block of their own
    /// and never inside the frontmatter. The note's name, or None when the tab
    /// in use is not a note.
    pub fn embed_at_cursor(&mut self, embeds: &str) -> Option<String> {
        let t = self.tabs.get_mut(self.active).filter(|t| t.view.is_none() && t.board.is_none() && t.quadro.is_none() && t.reader.is_none())?;
        let mut b = editor::char_to_byte(&t.text, t.cursor.min(t.text.chars().count()));
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

    fn central(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let frame = egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::ZERO);
        let mut actions: Vec<Action> = Vec::new();
        let mut sc_actions = Vec::new();
        let mut open_board: Option<String> = None;
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            if self.tabs.is_empty() {
                panels::empty_state(self, ui);
                return;
            }
            // a split: the other pane shows the tab of `split`
            let other = self.split.as_ref().and_then(|sp| self.tabs.iter().position(|t| &t.path == sp)).filter(|&i| i != self.active);
            let Some(other) = other else {
                self.pane_ui(ui, &mut actions, &mut sc_actions, &mut open_board, true);
                return;
            };
            let full = ui.available_rect_before_wrap();
            let mid = (full.left() + full.width() * self.split_frac).round();
            let left = egui::Rect::from_min_max(full.min, egui::pos2(mid - 0.5, full.max.y));
            let right = egui::Rect::from_min_max(egui::pos2(mid + 0.5, full.min.y), full.max);
            let (li, ri) = if self.split_right { (self.active, other) } else { (other, self.active) };
            let handle = egui::Rect::from_center_size(egui::pos2(mid, full.center().y), egui::vec2(9.0, full.height()));
            let focused = self.active;
            let mut focus_to = None;
            for (rect, i) in [(left, li), (right, ri)] {
                let pressed = ui.input(|inp| inp.pointer.any_pressed() && inp.pointer.interact_pos().is_some_and(|q| rect.contains(q) && !handle.contains(q)));
                if pressed && i != focused {
                    focus_to = Some(i);
                }
                self.active = i;
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect).id_salt(("pane", i)), |ui| {
                    ui.set_clip_rect(rect);
                    self.pane_ui(ui, &mut actions, &mut sc_actions, &mut open_board, true);
                });
                self.active = focused;
                // the pane in use is marked at the top
                if i == focused {
                    ui.painter().hline(rect.x_range(), rect.top() + 1.0, egui::Stroke::new(2.0_f32, p.accent_strong));
                }
            }
            // the line between the panes moves; a double click puts it back in the middle
            let hr = ui.interact(handle, ui.id().with("split-handle"), egui::Sense::click_and_drag());
            if hr.hovered() || hr.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if hr.dragged() {
                if let Some(q) = hr.interact_pointer_pos() {
                    self.split_frac = ((q.x - full.left()) / full.width().max(1.0)).clamp(0.2, 0.8);
                }
            }
            if hr.drag_stopped() {
                self.save_session();
            }
            if hr.double_clicked() {
                self.split_frac = 0.5;
                self.save_session();
            }
            let line = if hr.hovered() || hr.dragged() { egui::Stroke::new(2.0_f32, p.accent_strong) } else { egui::Stroke::new(1.0_f32, p.border) };
            ui.painter().vline(mid, full.y_range(), line);
            if let Some(i) = focus_to {
                // the other pane becomes the one in use; the sides stay where they are
                self.split = Some(self.tabs[self.active].path.clone());
                self.split_right = !self.split_right;
                self.active = i;
            }
        });
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
            crate::align::set_current(crate::align::TextAlign::of_note(&tab.text));
            let max_w = if readable && centered { crate::typo::get().line_width.unwrap_or(46.0 * size) } else { f32::INFINITY };
            let pad = if centered { 32.0 } else { 14.0 };
            egui::ScrollArea::vertical().id_salt(("note", &tab.path, tab.mode == Mode::Read, centered)).auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_width();
                let w = avail.min(max_w + 2.0 * pad);
                let side = ((avail - w) / 2.0).max(0.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(side + pad);
                    ui.vertical(|ui| {
                        ui.set_width((w - 2.0 * pad).max(200.0));
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
                            Mode::Edit if !crate::live::query_blocks(&tab.text).is_empty() => {
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
            crate::align::set_current(crate::align::TextAlign::Left);
        }
    }
}
