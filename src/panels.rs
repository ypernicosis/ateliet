//! Sidebars and chrome: ribbon, file explorer, search, bookmarks, backlinks,
//! outline, tags, calendar, tab bar, status bar, dialogs and settings.

use std::collections::HashMap;
use crate::app::{App, Ask, AskKind, LeftTab, Mode, RightTab};
use crate::config::{self, VaultConfig};
use crate::vault::{self, Folder};
use ateliet_core::calendar::{self, MonthRequest};
use ateliet_core::search;
use chrono::{Datelike, NaiveDate};
use crate::icons;
use crate::kit;
use crate::theme::{self, Palette};
use eframe::egui::{self, Color32, RichText, Sense};

enum FileCmd {
    Bookmark,
    Reveal,
    External,
    Similar,
    /// Move into this folder ("" is the root).
    MoveTo(String),
    /// A new quadro (Kanban) or Excalidraw drawing in this folder.
    NewQuadro,
    NewDrawing,
    /// Move the selected items into this folder.
    MoveManyTo(Vec<String>, String),
    /// Open the selected files, each in a tab.
    OpenMany(Vec<String>),
}

/// The explorer's selection: several files and folders picked with Shift
/// (a range from the anchor) or Ctrl/Cmd (one more, or one less), as in
/// Obsidian. Kept in egui's memory between frames.
#[derive(Clone, Default)]
struct Selection {
    items: Vec<String>,
    /// Where a Shift range starts: the last item clicked.
    anchor: Option<String>,
    /// The rows in the order they were drawn last frame, for ranges.
    order: Vec<String>,
}

impl Selection {
    fn id() -> egui::Id {
        egui::Id::new("explorer-selection")
    }

    fn has(&self, path: &str) -> bool {
        self.items.iter().any(|x| x == path)
    }

    /// A click on `path` with these modifiers; true when it only changed
    /// the selection (the click must not open or fold anything).
    fn click(&mut self, path: &str, m: egui::Modifiers, active: Option<&str>) -> bool {
        if m.shift {
            let anchor = self.anchor.clone().or_else(|| active.map(str::to_string)).unwrap_or_else(|| path.to_string());
            let a = self.order.iter().position(|x| *x == anchor);
            let b = self.order.iter().position(|x| x == path);
            self.items = match (a, b) {
                (Some(a), Some(b)) => self.order[a.min(b)..=a.max(b)].to_vec(),
                _ => vec![path.to_string()],
            };
            self.anchor = Some(anchor);
            return true;
        }
        if m.command {
            if self.items.is_empty() {
                // the open note is where a selection starts
                if let Some(a) = active.filter(|a| *a != path && self.order.iter().any(|x| x == a)) {
                    self.items.push(a.to_string());
                }
            }
            match self.items.iter().position(|x| x == path) {
                Some(i) => {
                    self.items.remove(i);
                }
                None => self.items.push(path.to_string()),
            }
            self.anchor = Some(path.to_string());
            return true;
        }
        self.items.clear();
        self.anchor = Some(path.to_string());
        false
    }

    /// The selection when `path` is part of it with others, else nothing.
    fn group(&self, path: &str) -> Option<Vec<String>> {
        (self.items.len() > 1 && self.has(path)).then(|| self.items.clone())
    }
}

/// Several explorer items being dragged together.
#[derive(Clone)]
struct DraggedMany(Vec<String>);

/// What the explorer shows first (Obsidian's `fileSortOrder` values).
const SORTS: [(&str, &str); 6] = [("alphabetical", "Nome (A a Z)"), ("alphabeticalReverse", "Nome (Z a A)"), ("byModifiedTime", "Editados por último"), ("byModifiedTimeReverse", "Editados primeiro"), ("byCreatedTime", "Criados por último"), ("byCreatedTimeReverse", "Criados primeiro")];

struct Order<'a> {
    sort: &'a str,
    times: &'a HashMap<String, (u64, u64)>,
}

enum TabCmd {
    CloseOthers,
    CloseRight,
    /// Show beside the active tab (a split).
    Beside,
    Reveal,
    OpenExternal,
}
use serde_json::Value;
use std::sync::mpsc::{self, Receiver};

// ── search ────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub run_now: bool,
    last: String,
    results: Vec<search::FileResult>,
    pending: Option<Receiver<Vec<search::FileResult>>>,
}

// ── calendar ──────────────────────────────────────────────────────────────

pub struct CalendarState {
    pub year: i32,
    pub month: u32,
    pub week_start: Option<u32>,
    pub words_per_dot: u32,
    pub confirm: bool,
    cache: Option<(u64, i32, u32, Value)>,
}

impl CalendarState {
    pub fn new() -> Self {
        let t = ateliet_core::datefmt::today();
        CalendarState { year: t.year(), month: t.month(), week_start: None, words_per_dot: 250, confirm: false, cache: None }
    }

    pub fn load(&mut self, cfg: &VaultConfig) {
        let v = cfg.tool("calendar", "calendar");
        let days = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
        self.week_start = v["weekStart"].as_str().and_then(|w| days.iter().position(|d| *d == w.to_lowercase())).map(|i| i as u32);
        self.words_per_dot = v["wordsPerDot"].as_u64().unwrap_or(250) as u32;
        self.confirm = v["shouldConfirmBeforeCreate"].as_bool().unwrap_or(false);
        self.cache = None;
    }
}

impl Default for CalendarState {
    fn default() -> Self {
        Self::new()
    }
}

fn icon_button(ui: &mut egui::Ui, glyph: &str, tip: &str, active: bool, p: Palette) -> egui::Response {
    kit::icon_button(ui, glyph, tip, active, p)
}

pub fn ribbon(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::SidePanel::left("ribbon").exact_width(48.0).resizable(false).show_separator_line(false).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 10, right: 10, top: 10, bottom: 10 })).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.vertical_centered(|ui| {
            if icon_button(ui, icons::FOLDER_CLOSED, "Arquivos", app.left_open && app.left_tab == LeftTab::Files, p).clicked() {
                toggle_left(app, LeftTab::Files);
            }
            if icon_button(ui, icons::SEARCH, "Buscar (Ctrl+Shift+F)", app.left_open && app.left_tab == LeftTab::Search, p).clicked() {
                toggle_left(app, LeftTab::Search);
            }
            if icon_button(ui, icons::BOOKMARK, "Favoritos", app.left_open && app.left_tab == LeftTab::Bookmarks, p).clicked() {
                toggle_left(app, LeftTab::Bookmarks);
            }
            ui.add_space(10.0);
            if icon_button(ui, icons::LIBRARY_BIG, "Biblioteca", app.active_tab().is_some_and(|t| t.path == "scriptorium:library"), p).clicked() {
                app.open_view("library", None);
            }
            if icon_button(ui, icons::IMAGES, "Modo imagem (Ctrl+Shift+I)", app.images_mode, p).clicked() {
                app.images_mode = !app.images_mode;
            }
            storyboards_button(app, ui, p);
            if icon_button(ui, icons::CALENDAR_DAYS, "Nota de hoje", false, p).clicked() {
                app.open_daily(ateliet_core::datefmt::today().date());
            }
            if icon_button(ui, icons::LAYOUT_TEMPLATE, "Inserir modelo (Alt+E)", false, p).clicked() {
                app.palette_ui = Some(crate::commands::Palette::templates(app, app.tabs.is_empty()));
            }
            if icon_button(ui, icons::COMMAND, "Comandos (Ctrl+P)", false, p).clicked() {
                app.palette_ui = Some(crate::commands::Palette::commands());
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                if icon_button(ui, icons::SETTINGS, "Configurações", app.settings_open, p).clicked() {
                    app.settings_open = !app.settings_open;
                }
                let up = app.updater.available();
                let tip = match app.updater.state {
                    crate::update::State::Ready => "O Ateliet foi atualizado: reinicie para usar a versão nova".to_string(),
                    crate::update::State::Building => format!("Atualizando: {}…", app.updater.step),
                    _ => match &app.updater.status {
                        Some(s) if s.available => format!("Atualizar o Ateliet ({} {})", s.behind, if s.behind == 1 { "commit novo" } else { "commits novos" }),
                        _ => "Atualizar o Ateliet".into(),
                    },
                };
                let r = icon_button(ui, icons::CIRCLE_ARROW_DOWN, &tip, up, p);
                if up {
                    ui.painter().circle_filled(r.rect.right_top() + egui::vec2(-7.0, 7.0), 3.5, p.accent_strong);
                }
                if r.clicked() {
                    app.updater.open = true;
                    if app.updater.state == crate::update::State::Idle {
                        app.updater.check();
                    }
                }
                if icon_button(ui, icons::ARCHIVE, "Abrir outro cofre", false, p).clicked() {
                    app.save_all();
                    app.vault = None;
                    app.tabs.clear();
                }
            });
        });
    });
}

/// Storyboards: a new one, an infinite canvas, or one of the vault's, most
/// recently changed first.
fn storyboards_button(app: &mut App, ui: &mut egui::Ui, p: Palette) {
    let on_board = !app.images_mode && app.active_tab().is_some_and(|t| t.board.is_some());
    let r = icon_button(ui, icons::CLAPPERBOARD, "Storyboards", on_board, p);
    let id = ui.make_persistent_id("ribbon-storyboards");
    if r.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    let mut boards: Vec<(String, u64)> = app.vault.as_ref().map(|v| v.files.iter().filter(|e| !e.folder && e.path.to_lowercase().ends_with(".storyboard")).map(|e| (e.path.clone(), e.mtime)).collect()).unwrap_or_default();
    boards.sort_by_key(|b| std::cmp::Reverse(b.1));
    let mut open: Option<String> = None;
    egui::popup::popup_above_or_below_widget(ui, id, &r, egui::AboveOrBelow::Below, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(250.0);
        if kit::menu_item(ui, icons::PLUS, "Novo storyboard…", p) {
            app.new_board_dlg = Some(crate::board_ui::NewBoard::new());
        }
        if kit::menu_item(ui, icons::PEN_TOOL, "Nova tela infinita", p) {
            app.images_mode = false;
            app.new_board("canvas");
        }
        if !boards.is_empty() {
            kit::menu_sep(ui, p);
            for (path, _) in boards.iter().take(14) {
                let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".storyboard");
                if kit::menu_item(ui, icons::CLAPPERBOARD, name, p) {
                    open = Some(path.clone());
                }
            }
            if boards.len() > 14 {
                ui.label(RichText::new(format!("e mais {} no explorador de arquivos", boards.len() - 14)).size(11.5).color(p.faint));
            }
        }
    });
    if let Some(path) = open {
        app.images_mode = false;
        app.open(&path, false);
    }
}

fn toggle_left(app: &mut App, tab: LeftTab) {
    if app.left_open && app.left_tab == tab {
        app.left_open = false;
    } else {
        app.left_open = true;
        app.left_tab = tab;
    }
}

fn header_tabs<T: PartialEq + Copy>(ui: &mut egui::Ui, current: &mut T, tabs: &[(T, &str, &str)], p: Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (t, glyph, tip) in tabs {
            if icon_button(ui, glyph, tip, *current == *t, p).clicked() {
                *current = *t;
            }
        }
    });
    ui.add_space(6.0);
}

pub fn left(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::SidePanel::left("left").resizable(true).show_separator_line(false).default_width(264.0).width_range(180.0..=520.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 10, right: 10, top: 10, bottom: 8 })).show(ctx, |ui| {
        let clip = ui.clip_rect();
        ui.painter().vline(clip.right() - 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
        match app.left_tab {
            LeftTab::Files => files(app, ui),
            LeftTab::Search => search_panel(app, ui),
            LeftTab::Bookmarks => bookmarks(app, ui),
        }
    });
}

fn files(app: &mut App, ui: &mut egui::Ui) {
    let Some(v) = &app.vault else { return };
    let p = app.palette;
    let name = v.name.clone();
    let tree = v.tree.clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&name).font(kit::medium(theme::UI)).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if kit::icon_button_sized(ui, icons::FOLDER_PLUS, "Nova pasta", false, p, 24.0, 14.0).clicked() {
                app.ask = Some(Ask { title: "Nova pasta".into(), value: String::new(), kind: AskKind::NewFolder(String::new()) });
            }
            if kit::icon_button_sized(ui, icons::SQUARE_KANBAN, "Novo quadro", false, p, 24.0, 14.0).clicked() {
                app.new_quadro("", false);
            }
            if kit::icon_button_sized(ui, icons::SQUARE_PEN, "Nova nota (Ctrl+N)", false, p, 24.0, 14.0).clicked() {
                app.new_note("");
            }
            let sr = kit::icon_button_sized(ui, icons::ARROW_UP_DOWN, "Ordem", false, p, 24.0, 14.0);
            let sid = ui.make_persistent_id("file-sort");
            if sr.clicked() {
                ui.memory_mut(|m| m.toggle_popup(sid));
            }
            let cur = app.cfg.as_ref().map(|c| c.read("app.json")["fileSortOrder"].as_str().unwrap_or("alphabetical").to_string()).unwrap_or_default();
            egui::popup_below_widget(ui, sid, &sr, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                ui.set_min_width(190.0);
                for (k, l) in SORTS {
                    if ui.selectable_label(cur == k, l).clicked() {
                        if let Some(c) = &app.cfg {
                            c.merge("app.json", serde_json::json!({ "fileSortOrder": k }));
                        }
                    }
                }
            });
        });
    });
    ui.add_space(4.0);
    let active = app.active_path();
    let mut open: Option<(String, bool)> = None;
    let mut ask: Option<Ask> = None;
    let mut new_in: Option<String> = None;
    let marks = app.cfg.as_ref().map(|c| bookmarked(&c.read("bookmarks.json"))).unwrap_or_default();
    let mut cmd: Option<(String, FileCmd)> = None;
    let sort = app.cfg.as_ref().map(|c| c.read("app.json")["fileSortOrder"].as_str().unwrap_or("alphabetical").to_string()).unwrap_or_default();
    let times: HashMap<String, (u64, u64)> = match &app.vault {
        Some(v) if !sort.starts_with("alpha") => v.files.iter().map(|e| (e.path.clone(), (e.mtime, e.ctime))).collect(),
        _ => HashMap::new(),
    };
    let order = Order { sort: &sort, times: &times };
    let mut sel: Selection = ui.data(|d| d.get_temp(Selection::id())).unwrap_or_default();
    let mut drawn: Vec<String> = Vec::new();
    let resp = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let mut fx = FolderCx { active: active.as_deref(), open: &mut open, ask: &mut ask, new_in: &mut new_in, marks: &marks, cmd: &mut cmd, order: &order, sel: &mut sel, drawn: &mut drawn, p };
        folder_ui(ui, &tree, "", 0, &mut fx);
        // the space below: drop here to move to the root; a click clears the selection
        let (rest, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ui.available_height().max(40.0)), Sense::click());
        if let Some(c) = dropped(&r, "") {
            cmd = Some(c);
        }
        if r.dnd_hover_payload::<String>().is_some() || r.dnd_hover_payload::<DraggedMany>().is_some() {
            ui.painter().rect_stroke(rest.shrink(2.0), 6.0, egui::Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Inside);
        }
        if r.clicked() {
            sel.items.clear();
        }
    });
    if !sel.items.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.ctx().wants_keyboard_input() {
        sel.items.clear();
    }
    sel.order = drawn;
    sel.items.retain(|x| sel.order.contains(x));
    ui.data_mut(|d| d.insert_temp(Selection::id(), sel));
    let _ = resp;
    if let Some((p, nt)) = open {
        app.open(&p, nt);
    }
    if let Some(a) = ask {
        app.ask = Some(a);
    }
    if let Some(f) = new_in {
        app.new_note(&f);
    }
    if let Some((path, c)) = cmd {
        match c {
            FileCmd::Bookmark => {
                if let Some(cfg) = &app.cfg {
                    toggle_bookmark(cfg, &path);
                }
            }
            FileCmd::MoveTo(folder) => move_into(app, &path, &folder),
            FileCmd::MoveManyTo(items, folder) => {
                // an item inside another selected folder moves with it
                let tops: Vec<&String> = items.iter().filter(|x| !items.iter().any(|y| y != *x && x.starts_with(&format!("{y}/")))).collect();
                for it in tops {
                    if *it != folder && !folder.starts_with(&format!("{it}/")) {
                        move_into(app, it, &folder);
                    }
                }
                ui.data_mut(|d| d.remove::<Selection>(Selection::id()));
            }
            FileCmd::OpenMany(items) => {
                for it in items {
                    app.open(&it, true);
                }
            }
            FileCmd::NewQuadro => app.new_quadro(&path, false),
            FileCmd::NewDrawing => app.new_quadro(&path, true),
            FileCmd::Similar => {
                if let Some(root) = app.vault.as_ref().map(|v| v.root.clone()) {
                    if let Some(c) = app.ctx.clone() {
                        app.images.open(&root, &c);
                    }
                }
                app.images.show_similar(&path);
                app.images_mode = true;
            }
            FileCmd::Reveal | FileCmd::External => {
                if let Some(v) = &app.vault {
                    let abs = v.abs(&path);
                    let _ = open::that(if matches!(c, FileCmd::Reveal) { abs.parent().map(|d| d.to_path_buf()).unwrap_or(abs) } else { abs });
                }
            }
        }
    }
}

/// Move a file or folder into `folder` ("" is the root), keeping its name.
fn move_into(app: &mut App, path: &str, folder: &str) {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let to = if folder.is_empty() { name } else { format!("{folder}/{name}") };
    if to != path {
        app.rename(path, &to);
    }
}

fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "md" => icons::FILE_TEXT,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "psd" | "clip" => icons::IMAGE,
        "storyboard" => icons::CLAPPERBOARD,
        "excalidraw" => icons::PEN_TOOL,
        "canvas" => icons::NETWORK,
        "mp3" | "wav" | "ogg" | "m4a" | "flac" => icons::MUSIC,
        "mp4" | "mov" | "webm" => icons::FILM,
        _ => icons::FILE,
    }
}

#[allow(clippy::too_many_arguments)]
/// Bookmarked paths (the files at any depth of `bookmarks.json`).
fn bookmarked(b: &Value) -> Vec<String> {
    fn walk(items: &[Value], out: &mut Vec<String>) {
        for it in items {
            if let Some(p) = it["path"].as_str() {
                out.push(p.to_string());
            }
            walk(it["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), out);
        }
    }
    let mut out = Vec::new();
    walk(b["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), &mut out);
    out
}

/// Add or remove a file from the top level of `bookmarks.json`.
fn toggle_bookmark(cfg: &VaultConfig, path: &str) {
    let mut b = cfg.read("bookmarks.json");
    let mut items = b["items"].as_array().cloned().unwrap_or_default();
    fn strip(items: &mut Vec<Value>, path: &str) -> bool {
        let before = items.len();
        items.retain(|it| it["path"] != path);
        let mut hit = items.len() != before;
        for it in items.iter_mut() {
            if let Some(sub) = it["items"].as_array_mut() {
                hit |= strip(sub, path);
            }
        }
        hit
    }
    if !strip(&mut items, path) {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        items.push(serde_json::json!({ "type": "file", "ctime": ms, "path": path }));
    }
    b = serde_json::json!({ "items": items });
    cfg.merge("bookmarks.json", b);
}

/// What drawing the explorer's tree reads and changes.
struct FolderCx<'a> {
    active: Option<&'a str>,
    open: &'a mut Option<(String, bool)>,
    ask: &'a mut Option<Ask>,
    new_in: &'a mut Option<String>,
    marks: &'a [String],
    cmd: &'a mut Option<(String, FileCmd)>,
    order: &'a Order<'a>,
    sel: &'a mut Selection,
    /// The rows in drawing order (for Shift ranges next frame).
    drawn: &'a mut Vec<String>,
    p: Palette,
}

/// Drag and drop on a row: dragging a selected item takes the whole
/// selection; a folder takes what is dropped on it.
fn row_dnd(ui: &egui::Ui, r: &egui::Response, full: &str, folder: bool, fx: &mut FolderCx) {
    if r.drag_started() {
        match fx.sel.group(full) {
            Some(items) => r.dnd_set_drag_payload(DraggedMany(items)),
            None => r.dnd_set_drag_payload(full.to_string()),
        }
    }
    if !folder {
        return;
    }
    // a file or folder dropped here moves in (not a folder into itself)
    if let Some(c) = dropped(r, full) {
        *fx.cmd = Some(c);
    }
    if r.dnd_hover_payload::<String>().is_some() || r.dnd_hover_payload::<DraggedMany>().is_some() {
        ui.painter().rect_stroke(r.rect, 6.0, egui::Stroke::new(1.5_f32, fx.p.accent_strong), egui::StrokeKind::Inside);
    }
}

/// What was dropped on `folder`, as the move it asks for. The payload's type
/// is checked first: taking it as the wrong type would throw it away.
fn dropped(r: &egui::Response, folder: &str) -> Option<(String, FileCmd)> {
    if egui::DragAndDrop::has_payload_of_type::<DraggedMany>(&r.ctx) {
        let many = r.dnd_release_payload::<DraggedMany>()?;
        return Some((String::new(), FileCmd::MoveManyTo(many.0.clone(), folder.to_string())));
    }
    let src = r.dnd_release_payload::<String>()?;
    (*src != folder && !folder.starts_with(&format!("{src}/"))).then(|| ((*src).clone(), FileCmd::MoveTo(folder.to_string())))
}

/// The right-click menu of a row that is part of a selection of several.
fn group_menu(ui: &mut egui::Ui, items: &[String], fx: &mut FolderCx) {
    let p = fx.p;
    ui.label(RichText::new(format!("{} itens selecionados", items.len())).size(12.0).color(p.faint));
    let files: Vec<String> = items.iter().filter(|x| x.contains('.')).cloned().collect();
    if !files.is_empty() && kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir em abas", p) {
        *fx.cmd = Some((String::new(), FileCmd::OpenMany(files.clone())));
        ui.close_menu();
    }
    if kit::menu_item(ui, icons::LINK, "Copiar links", p) {
        let links: Vec<String> = items.iter().map(|x| format!("[[{}]]", vault::basename(x).trim_end_matches(".md"))).collect();
        ui.ctx().copy_text(links.join("\n"));
        ui.close_menu();
    }
    if kit::menu_item(ui, icons::FOLDER_OPEN, "Mover para…", p) {
        *fx.ask = Some(Ask { title: format!("Mover {} itens para a pasta", items.len()), value: String::new(), kind: AskKind::MoveMany(items.to_vec()) });
        ui.close_menu();
    }
    kit::menu_sep(ui, p);
    if kit::menu_danger(ui, icons::TRASH_2, &format!("Excluir {} itens", items.len()), p) {
        *fx.ask = Some(Ask { title: format!("Excluir {} itens?", items.len()), value: String::new(), kind: AskKind::ConfirmDeleteMany(items.to_vec()) });
        ui.close_menu();
    }
}

fn folder_ui(ui: &mut egui::Ui, f: &Folder, path: &str, depth: usize, fx: &mut FolderCx) {
    let p = fx.p;
    let order = fx.order;
    let mut folders: Vec<(&String, &Folder)> = f.folders.iter().collect();
    if order.sort == "alphabeticalReverse" {
        folders.reverse();
    }
    for (name, sub) in folders {
        let full = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
        let id = ui.make_persistent_id(("folder", &full));
        let is_open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        fx.drawn.push(full.clone());
        let r = kit::Row::new(name).depth(depth).chevron(is_open).selected(fx.sel.has(&full)).draggable().show(ui, p);
        row_dnd(ui, &r, &full, true, fx);
        if r.clicked() && !fx.sel.click(&full, ui.input(|i| i.modifiers), fx.active) {
            ui.data_mut(|d| d.insert_temp(id, !is_open));
        }
        if let Some(items) = fx.sel.group(&full) {
            r.context_menu(|ui| group_menu(ui, &items, fx));
        } else {
            folder_menu(&r, name, &full, fx);
        }
        if is_open {
            folder_ui(ui, sub, &full, depth + 1, fx);
        }
    }
    folder_files(ui, f, depth, fx);
}

fn folder_menu(r: &egui::Response, name: &str, full: &str, fx: &mut FolderCx) {
    let p = fx.p;
    let full = full.to_string();
    let (new_in, ask, cmd) = (&mut *fx.new_in, &mut *fx.ask, &mut *fx.cmd);
    {
        r.context_menu(|ui| {
            if kit::menu_item(ui, icons::FILE_PLUS, "Nova nota", p) {
                *new_in = Some(full.clone());
                ui.close_menu();
            }
            if kit::menu_item(ui, icons::SQUARE_KANBAN, "Novo quadro", p) {
                *cmd = Some((full.clone(), FileCmd::NewQuadro));
                ui.close_menu();
            }
            if kit::menu_item(ui, icons::PEN_TOOL, "Novo desenho (Excalidraw)", p) {
                *cmd = Some((full.clone(), FileCmd::NewDrawing));
                ui.close_menu();
            }
            if kit::menu_item(ui, icons::FOLDER_PLUS, "Nova pasta", p) {
                *ask = Some(Ask { title: "Nova pasta".into(), value: String::new(), kind: AskKind::NewFolder(full.clone()) });
                ui.close_menu();
            }
            if kit::menu_item(ui, icons::FOLDER_OPEN, "Mostrar na pasta", p) {
                *cmd = Some((format!("{full}/."), FileCmd::Reveal));
            }
            kit::menu_sep(ui, p);
            if kit::menu_item(ui, icons::PENCIL, "Renomear", p) {
                *ask = Some(Ask { title: "Renomear pasta".into(), value: name.to_string(), kind: AskKind::Rename(full.clone()) });
                ui.close_menu();
            }
            if kit::menu_danger(ui, icons::TRASH_2, "Excluir", p) {
                *ask = Some(Ask { title: format!("Excluir a pasta “{name}” e tudo dentro dela?"), value: String::new(), kind: AskKind::ConfirmDelete(full.clone()) });
                ui.close_menu();
            }
        });
    }
}

fn folder_files(ui: &mut egui::Ui, f: &Folder, depth: usize, fx: &mut FolderCx) {
    let p = fx.p;
    let order = fx.order;
    let mut files: Vec<(&String, &String)> = f.files.iter().collect();
    match order.sort {
        "alphabeticalReverse" => files.reverse(),
        "byModifiedTime" => files.sort_by_key(|(_, full)| std::cmp::Reverse(order.times.get(*full).map(|t| t.0).unwrap_or(0))),
        "byModifiedTimeReverse" => files.sort_by_key(|(_, full)| order.times.get(*full).map(|t| t.0).unwrap_or(0)),
        "byCreatedTime" => files.sort_by_key(|(_, full)| std::cmp::Reverse(order.times.get(*full).map(|t| t.1).unwrap_or(0))),
        "byCreatedTimeReverse" => files.sort_by_key(|(_, full)| order.times.get(*full).map(|t| t.1).unwrap_or(0)),
        _ => {}
    }
    for (name, full) in files {
        let is_md = name.to_lowercase().ends_with(".md");
        let label = if is_md { &name[..name.len() - 3] } else { name.as_str() };
        let is_active = fx.active == Some(full.as_str());
        fx.drawn.push(full.clone());
        // a selection of several shows instead of the open note
        let selected = if fx.sel.items.len() > 1 { fx.sel.has(full) } else { is_active || fx.sel.has(full) };
        // files line up with the folder names above them (past the chevron)
        let r = kit::Row::new(label).depth(depth).icon(file_icon(name)).selected(selected).dim(!is_md && !name.ends_with(".storyboard") && !name.ends_with(".excalidraw")).draggable().show(ui, p);
        row_dnd(ui, &r, full, false, fx);
        if r.clicked() && !fx.sel.click(full, ui.input(|i| i.modifiers), fx.active) {
            *fx.open = Some((full.clone(), false));
        }
        if r.middle_clicked() {
            *fx.open = Some((full.clone(), true));
        }
        if let Some(items) = fx.sel.group(full) {
            r.context_menu(|ui| group_menu(ui, &items, fx));
            continue;
        }
        let (open, ask, cmd, marks) = (&mut *fx.open, &mut *fx.ask, &mut *fx.cmd, fx.marks);
        r.context_menu(|ui| {
            if kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir em nova aba", p) {
                *open = Some((full.clone(), true));
            }
            let marked = marks.iter().any(|m| m == full);
            if kit::menu_item(ui, icons::BOOKMARK, if marked { "Remover dos favoritos" } else { "Favoritar" }, p) {
                *cmd = Some((full.clone(), FileCmd::Bookmark));
            }
            kit::menu_sep(ui, p);
            if kit::menu_item(ui, icons::LINK, "Copiar link", p) {
                ui.ctx().copy_text(format!("[[{label}]]"));
            }
            if kit::menu_item(ui, icons::COPY, "Copiar caminho", p) {
                ui.ctx().copy_text(full.clone());
            }
            if kit::menu_item(ui, icons::FOLDER_OPEN, "Mostrar na pasta", p) {
                *cmd = Some((full.clone(), FileCmd::Reveal));
            }
            if !is_md && kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", p) {
                *cmd = Some((full.clone(), FileCmd::External));
            }
            if ateliet_core::pictogrep::is_image_name(name) && kit::menu_item(ui, icons::SCAN_SEARCH, "Imagens semelhantes", p) {
                *cmd = Some((full.clone(), FileCmd::Similar));
            }
            kit::menu_sep(ui, p);
            if kit::menu_item(ui, icons::PENCIL, "Renomear", p) {
                *ask = Some(Ask { title: "Renomear".into(), value: label.to_string(), kind: AskKind::Rename(full.clone()) });
            }
            if kit::menu_danger(ui, icons::TRASH_2, "Excluir", p) {
                *ask = Some(Ask { title: format!("Excluir “{label}”?"), value: String::new(), kind: AskKind::ConfirmDelete(full.clone()) });
                ui.close_menu();
            }
        });
    }
}

fn search_panel(app: &mut App, ui: &mut egui::Ui) {
    let Some(v) = &app.vault else { return };
    let p = app.palette;
    let s = &mut app.search;
    let r = kit::search_field(ui, &mut s.query, "Buscar no cofre", p);
    if s.run_now {
        r.request_focus();
    }
    if (s.run_now || (r.changed() && s.query.len() >= 2) || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && s.query != s.last {
        s.run_now = false;
        s.last = s.query.clone();
        let (tx, rx) = mpsc::channel();
        let (root, q, c) = (v.root.clone(), s.query.clone(), ui.ctx().clone());
        std::thread::spawn(move || {
            let _ = tx.send(search::search(&root, &q, false, 200));
            c.request_repaint();
        });
        s.pending = Some(rx);
    }
    s.run_now = false;
    if let Some(rx) = &s.pending {
        if let Ok(res) = rx.try_recv() {
            s.results = res;
            s.pending = None;
        }
    }
    ui.add_space(6.0);
    if s.pending.is_some() {
        ui.spinner();
    } else if !s.query.is_empty() {
        let total: usize = s.results.iter().map(|r| r.total).sum();
        ui.label(RichText::new(format!("{} arquivos · {} ocorrências", s.results.len(), total)).small().color(p.faint));
    } else {
        ui.label(RichText::new("tag:  path:  \"frase exata\"  -excluir").small().color(p.faint));
    }
    ui.add_space(4.0);
    let mut open: Option<String> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for r in &s.results {
            let name = vault::basename(&r.path);
            if kit::Row::new(name).icon(icons::FILE_TEXT).trailing(r.total.to_string()).show(ui, p).clicked() {
                open = Some(r.path.clone());
            }
            for m in r.matches.iter().take(3) {
                let text: String = m.text.trim().chars().take(160).collect();
                let resp = ui.horizontal(|ui| {
                    ui.add_space(30.0);
                    ui.add(egui::Label::new(RichText::new(text).small().color(p.muted)).sense(Sense::click()).truncate())
                });
                if resp.inner.clicked() {
                    open = Some(r.path.clone());
                }
            }
            ui.add_space(6.0);
        }
    });
    if let Some(p) = open {
        app.open(&p, false);
    }
}

fn bookmarks(app: &mut App, ui: &mut egui::Ui) {
    let Some(cfg) = &app.cfg else { return };
    let p = app.palette;
    let b = cfg.read("bookmarks.json");
    let mut open = None;
    fn walk(items: &[Value], ui: &mut egui::Ui, depth: usize, open: &mut Option<String>, p: Palette) {
        for it in items {
            match it["type"].as_str() {
                Some("file") => {
                    let path = it["path"].as_str().unwrap_or_default();
                    let title = it["title"].as_str().map(str::to_string).unwrap_or_else(|| vault::basename(path).to_string());
                    if kit::Row::new(&title).depth(depth).icon(icons::FILE_TEXT).show(ui, p).clicked() {
                        *open = Some(path.to_string());
                    }
                }
                Some("group") => {
                    let title = it["title"].as_str().unwrap_or("Grupo");
                    let id = ui.make_persistent_id(("bm-group", title, depth));
                    let is_open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
                    if kit::Row::new(title).depth(depth).chevron(is_open).show(ui, p).clicked() {
                        ui.data_mut(|d| d.insert_temp(id, !is_open));
                    }
                    if is_open {
                        walk(it["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), ui, depth + 1, open, p);
                    }
                }
                _ => {}
            }
        }
    }
    let items = b["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        ui.label(RichText::new("Nenhum favorito.").color(p.faint));
    }
    ui.spacing_mut().item_spacing.y = 1.0;
    walk(&items, ui, 0, &mut open, p);
    if let Some(p) = open {
        app.open(&p, false);
    }
}

pub fn right(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::SidePanel::right("right").resizable(true).show_separator_line(false).default_width(290.0).width_range(220.0..=520.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 10, right: 10, top: 10, bottom: 8 })).show(ctx, |ui| {
        let clip = ui.clip_rect();
        ui.painter().vline(clip.left() + 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
        let mut tab = app.right_tab;
        header_tabs(ui, &mut tab, &[(RightTab::Calendar, icons::CALENDAR, "Calendário"), (RightTab::Backlinks, icons::LINK, "Backlinks"), (RightTab::Outline, icons::LIST, "Estrutura"), (RightTab::Tags, icons::HASH, "Tags"), (RightTab::Manuscript, icons::BOOK_TEXT, "Manuscrito"), (RightTab::Pictures, icons::IMAGES, "Imagens")], p);
        app.right_tab = tab;
        match app.right_tab {
            RightTab::Backlinks => backlinks(app, ui),
            RightTab::Outline => outline(app, ui),
            RightTab::Tags => tags(app, ui),
            RightTab::Calendar => calendar_panel(app, ui),
            RightTab::Pictures => {
                if let Some(root) = app.vault.as_ref().map(|v| v.root.clone()) {
                    app.images.open(&root, ui.ctx());
                }
                if let Some(crate::images_ui::ImageAction::Insert(keys)) = app.images.side_ui(ui, app.palette) {
                    app.insert_pictures(&keys);
                }
            }
            RightTab::Manuscript => {
                let active = app.active_path();
                let acts = {
                    let (Some(v), Some(sc)) = (app.vault.as_ref(), app.sc.as_mut()) else { return };
                    let mut cx = crate::sc::library::Cx { vault: v, sc, caches: &mut app.caches, palette: app.palette, dv: &app.dv_settings, music: &app.music };
                    app.binder.ui(ui, &mut cx, active.as_deref())
                };
                for a in acts {
                    app.sc_apply(a);
                }
            }
        }
    });
}

fn backlinks(app: &mut App, ui: &mut egui::Ui) {
    let p = app.palette;
    let (Some(v), Some(path)) = (&app.vault, app.active_path()) else {
        ui.label(RichText::new("Nenhum arquivo aberto.").color(p.faint));
        return;
    };
    let (inl, outl): (Vec<String>, Vec<String>) = {
        let ix = v.index.read().unwrap();
        (ix.inlinks(&path), ix.page(&path).map(|p| p.outlinks.iter().map(|l| l.path.clone()).collect()).unwrap_or_default())
    };
    let mut open = None;
    ui.spacing_mut().item_spacing.y = 1.0;
    kit::section(ui, &format!("Menções com link · {}", inl.len()), p);
    for s in &inl {
        if kit::Row::new(vault::basename(s)).icon(icons::ARROW_LEFT).show(ui, p).clicked() {
            open = Some(s.clone());
        }
    }
    ui.add_space(10.0);
    kit::section(ui, &format!("Links de saída · {}", outl.len()), p);
    for s in &outl {
        let exists = v.abs(s).exists();
        if kit::Row::new(vault::basename(s)).icon(icons::ARROW_RIGHT).dim(!exists).show(ui, p).clicked() {
            open = Some(s.clone());
        }
    }
    // notes naming this one without a link
    let generation = v.index.read().map(|ix| ix.generation).unwrap_or(0);
    if app.unlinked.as_ref().is_none_or(|u| u.0 != path || u.1 != generation) {
        let name = vault::basename(&path).trim_end_matches(".md").to_string();
        let mut found = Vec::new();
        if path.to_lowercase().ends_with(".md") {
            for e in v.markdown_files() {
                if e.path == path || inl.contains(&e.path) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(v.abs(&e.path)) else { continue };
                for (at, line) in ateliet_core::relink::unlinked_mentions(&text, &name) {
                    found.push((e.path.clone(), at, line));
                }
            }
        }
        app.unlinked = Some((path.clone(), generation, found));
    }
    let mentions = app.unlinked.as_ref().map(|u| u.2.clone()).unwrap_or_default();
    ui.add_space(10.0);
    kit::section(ui, &format!("Menções sem link · {}", mentions.len()), p);
    let mut link: Option<(String, usize)> = None;
    for (src, at, line) in mentions.iter().take(60) {
        if kit::Row::new(vault::basename(src)).icon(icons::FILE_TEXT).show(ui, p).clicked() {
            open = Some(src.clone());
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_space(8.0);
            let short: String = line.chars().take(140).collect();
            ui.label(RichText::new(short).size(11.5).color(p.faint));
            if ui.add(egui::Label::new(RichText::new("Linkar").size(11.5).color(p.accent_strong)).sense(Sense::click())).on_hover_text("Transforma esta menção em link").clicked() {
                link = Some((src.clone(), *at));
            }
        });
        ui.add_space(4.0);
    }
    if let Some((src, at)) = link {
        let name = vault::basename(&path).trim_end_matches(".md").to_string();
        if let Ok(text) = std::fs::read_to_string(v.abs(&src)) {
            let new_text = ateliet_core::relink::link_mention(&text, at, name.len(), &name);
            if v.write(&src, &new_text).is_ok() {
                v.index.write().unwrap().update(&[src.clone()]);
                for t in &mut app.tabs {
                    if t.path == src && !t.dirty() {
                        t.text = new_text.clone();
                        t.saved = new_text.clone();
                    }
                }
                app.unlinked = None;
            }
        }
    }
    if let Some(p) = open {
        app.open_link(&p, false);
    }
}

fn outline(app: &mut App, ui: &mut egui::Ui) {
    let p = app.palette;
    let Some(tab) = app.tabs.get_mut(app.active) else { return };
    if tab.view.is_some() || tab.board.is_some() || tab.quadro.is_some() {
        ui.label(RichText::new("Sem títulos.").color(p.faint));
        return;
    }
    let doc = tab.doc();
    if doc.headings.is_empty() {
        ui.label(RichText::new("Sem títulos.").color(p.faint));
    }
    ui.spacing_mut().item_spacing.y = 1.0;
    let min = doc.headings.iter().map(|h| h.0).min().unwrap_or(1);
    for (level, text, line) in &doc.headings {
        if kit::Row::new(text).depth((level - min) as usize).show(ui, p).clicked() {
            tab.scroll_to = Some(*line);
        }
    }
}

fn tags(app: &mut App, ui: &mut egui::Ui) {
    let Some(v) = &app.vault else { return };
    let p = app.palette;
    let tags = v.index.read().unwrap().tag_counts();
    let mut clicked = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for (t, n) in &tags {
            if kit::Row::new(t.trim_start_matches('#')).icon(icons::HASH).trailing(n.to_string()).show(ui, p).clicked() {
                clicked = Some(t.trim_start_matches('#').to_string());
            }
        }
    });
    if let Some(t) = clicked {
        app.apply(crate::markdown::Action::Tag(t));
    }
}

fn calendar_panel(app: &mut App, ui: &mut egui::Ui) {
    let Some(v) = &app.vault else { return };
    let p = app.palette;
    let generation = v.index.read().map(|ix| ix.generation).unwrap_or(0);
    let c = &mut app.calendar;
    let fresh = matches!(&c.cache, Some((g, y, m, _)) if *g == generation && *y == c.year && *m == c.month);
    if !fresh {
        let req = MonthRequest { year: c.year, month: c.month, format: app.daily.format.clone(), folder: app.daily.folder.clone(), week_start: c.week_start, words_per_dot: c.words_per_dot, locale: app.locale.clone() };
        let data = calendar::month(&v.root, &v.index.read().unwrap(), &req);
        c.cache = Some((generation, c.year, c.month, data));
    }
    let data = c.cache.as_ref().unwrap().3.clone();
    let month = data["month"].as_str().unwrap_or_default();
    let mut first_char_upper: String = month.chars().take(1).flat_map(char::to_uppercase).collect();
    first_char_upper.push_str(&month.chars().skip(1).collect::<String>());
    ui.horizontal(|ui| {
        ui.label(RichText::new(first_char_upper).font(kit::bold(16.0)).color(p.text));
        ui.label(RichText::new(data["year"].to_string()).font(kit::medium(16.0)).color(p.faint));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if kit::icon_button_sized(ui, icons::CHEVRON_RIGHT, "Próximo mês", false, p, 24.0, 14.0).clicked() {
                if c.month == 12 {
                    c.month = 1;
                    c.year += 1;
                } else {
                    c.month += 1;
                }
            }
            if ui.add(egui::Button::new(RichText::new(if app.locale.starts_with("pt") { "Hoje" } else { "Today" }).size(12.0).color(p.muted)).frame(false)).clicked() {
                let t = ateliet_core::datefmt::today();
                c.year = t.year();
                c.month = t.month();
            }
            if kit::icon_button_sized(ui, icons::CHEVRON_LEFT, "Mês anterior", false, p, 24.0, 14.0).clicked() {
                if c.month == 1 {
                    c.month = 12;
                    c.year -= 1;
                } else {
                    c.month -= 1;
                }
            }
        });
    });
    ui.add_space(6.0);
    let active = app.tabs.get(app.active).map(|t| t.path.clone());
    let mut open_day: Option<NaiveDate> = None;
    let cell = ((ui.available_width() - 6.0 * 2.0) / 7.0).max(24.0);
    egui::Grid::new("calendar").spacing([2.0, 2.0]).min_col_width(cell).max_col_width(cell).show(ui, |ui| {
        for w in data["weekdays"].as_array().cloned().unwrap_or_default() {
            ui.vertical_centered(|ui| ui.label(RichText::new(w.as_str().unwrap_or_default().chars().take(3).collect::<String>()).font(kit::medium(10.5)).color(p.faint)));
        }
        ui.end_row();
        for week in data["weeks"].as_array().cloned().unwrap_or_default() {
            for d in week["days"].as_array().cloned().unwrap_or_default() {
                let day = d["day"].as_u64().unwrap_or(0);
                let in_month = d["inMonth"].as_bool().unwrap_or(false);
                let today = d["today"].as_bool().unwrap_or(false);
                let is_active = active.as_deref() == d["path"].as_str();
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, 34.0), Sense::click());
                let bg = if is_active { p.accent_strong } else if resp.hovered() { p.bg_hover } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(rect.shrink(1.0), 8.0, bg);
                if today && !is_active {
                    ui.painter().rect_stroke(rect.shrink(1.5), 8.0, egui::Stroke::new(1.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                }
                let color = if is_active { p.on_accent } else if today { p.accent_strong } else if in_month { p.text } else { p.faint };
                let font = if today { kit::bold(12.5) } else { egui::FontId::proportional(12.5) };
                ui.painter().text(rect.center_top() + egui::vec2(0.0, 7.0), egui::Align2::CENTER_TOP, day.to_string(), font, color);
                let dots = d["dots"].as_u64().unwrap_or(0) as usize;
                let tasks = d["openTasks"].as_u64().unwrap_or(0) > 0;
                let n = dots + tasks as usize;
                let dot_color = if is_active { p.on_accent } else { p.faint };
                for i in 0..n {
                    let x = rect.center().x + (i as f32 - (n as f32 - 1.0) / 2.0) * 6.0;
                    let c = egui::pos2(x, rect.bottom() - 7.0);
                    if i < dots {
                        ui.painter().circle_filled(c, 1.8, dot_color);
                    } else {
                        ui.painter().circle_stroke(c, 1.8, egui::Stroke::new(1.0_f32, dot_color));
                    }
                }
                if resp.clicked() {
                    open_day = d["date"].as_str().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
                }
            }
            ui.end_row();
        }
    });
    if let Some(d) = open_day {
        app.open_daily(d);
    }
}

// ── tabs, status bar, empty state ──────────────────────────────────────────

pub fn tab_bar(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::TopBottomPanel::top("tabs").show_separator_line(false).exact_height(42.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 8, right: 8, top: 7, bottom: 0 })).show(ctx, |ui| {
        let clip = ui.clip_rect();
        ui.painter().hline(clip.x_range(), clip.bottom() - 0.5, egui::Stroke::new(1.0_f32, p.border));
        let mut close = None;
        let mut tab_cmd: Option<(usize, TabCmd)> = None;
        let n_tabs = app.tabs.len();
        let mut select = None;
        let mut focus = false;
        // dragging a tab: (from, pointer x, released)
        let mut drag: Option<(usize, f32, bool)> = None;
        let mut first_x = 0.0;
        let mut tab_step = 0.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (g, tip) = if app.left_open { (icons::PANEL_LEFT_CLOSE, "Esconder barra lateral") } else { (icons::PANEL_LEFT_OPEN, "Mostrar barra lateral") };
            if icon_button(ui, g, tip, false, p).clicked() {
                app.left_open = !app.left_open;
            }
            ui.add_space(4.0);
            let right_w = 118.0;
            let avail = (ui.available_width() - right_w).max(100.0);
            let n = app.tabs.len().max(1) as f32;
            let tab_w = (avail / n - 2.0).clamp(110.0, 220.0);
            egui::ScrollArea::horizontal().max_width(avail).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, t) in app.tabs.iter().enumerate() {
                        let active = i == app.active;
                        let (rect, r) = ui.allocate_exact_size(egui::vec2(tab_w, 35.0), Sense::click_and_drag());
                        if i == 0 {
                            first_x = rect.left();
                            tab_step = tab_w + 2.0;
                        }
                        if r.dragged() || r.drag_stopped() {
                            if let Some(ptr) = ui.input(|inp| inp.pointer.latest_pos()) {
                                drag = Some((i, ptr.x, r.drag_stopped()));
                            }
                        }
                        let rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - 0.5));
                        // the pointer over the tab, its close button included (that
                        // button takes the hover from the tab when under the pointer)
                        let hovered = ui.rect_contains_pointer(rect) && !egui::DragAndDrop::has_any_payload(ui.ctx());
                        let shape = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y + 1.0));
                        if active {
                            ui.painter().rect(shape, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, p.bg, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                            // the active tab opens onto the page
                            ui.painter().hline(shape.x_range().shrink(1.0), shape.bottom() - 0.5, egui::Stroke::new(2.0_f32, p.bg));
                        } else if hovered {
                            ui.painter().rect_filled(shape.shrink2(egui::vec2(0.0, 3.0)).translate(egui::vec2(0.0, -2.0)), 7.0, p.bg_hover);
                        }
                        let glyph = match &t.view {
                            Some(crate::sc::View::Sheet(_)) => icons::CONTACT,
                            Some(crate::sc::View::Cast(_)) => icons::USERS,
                            Some(crate::sc::View::Enredo(_)) => icons::WAYPOINTS,
                            Some(crate::sc::View::Mundo(_)) => icons::GLOBE,
                            Some(crate::sc::View::Mapa(_)) => icons::MAP,
                            Some(crate::sc::View::Trilha(_)) => icons::MUSIC,
                            Some(crate::sc::View::Diagramacao(_)) => icons::LAYOUT_TEMPLATE,
                            Some(crate::sc::View::Capa(_)) => icons::BOOK_OPEN,
                            Some(crate::sc::View::Pagina(_)) => icons::LAYERS,
                            Some(_) => icons::LIBRARY_BIG,
                            None if t.board.is_some() => icons::CLAPPERBOARD,
                            None if t.quadro.as_ref().is_some_and(|q| q.is_kanban()) => icons::SQUARE_KANBAN,
                            None if t.quadro.is_some() => icons::PEN_TOOL,
                            None => icons::FILE_TEXT,
                        };
                        let cy = rect.center().y - 1.0;
                        ui.painter().text(egui::pos2(rect.left() + 16.0, cy), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(13.0), if active { p.muted } else { p.faint });
                        let title = format!("{}{}", t.title(), if t.dirty() { " •" } else { "" });
                        let color = if active { p.text } else { p.muted };
                        let mut job = egui::text::LayoutJob::single_section(title, egui::TextFormat { font_id: if active { kit::medium(12.5) } else { egui::FontId::proportional(12.5) }, color, ..Default::default() });
                        job.wrap = egui::text::TextWrapping::truncate_at_width(tab_w - if active || hovered { 58.0 } else { 38.0 });
                        let galley = ui.fonts(|f| f.layout_job(job));
                        ui.painter().galley(egui::pos2(rect.left() + 29.0, cy - galley.size().y / 2.0), galley, color);
                        if active || hovered {
                            let xr = egui::Rect::from_center_size(egui::pos2(rect.right() - 16.0, cy), egui::vec2(20.0, 20.0));
                            let xresp = ui.interact(xr, ui.id().with(("close-tab", i)), Sense::click());
                            if xresp.hovered() {
                                ui.painter().rect_filled(xr, 5.0, p.bg_active);
                            }
                            ui.painter().text(xr.center(), egui::Align2::CENTER_CENTER, icons::X, kit::icon_font(12.0), p.muted);
                            if xresp.clicked() {
                                close = Some(i);
                            }
                        }
                        if r.clicked() && close.is_none() {
                            select = Some(i);
                        }
                        if r.middle_clicked() {
                            close = Some(i);
                        }
                        let is_file = !t.path.starts_with("scriptorium:");
                        r.context_menu(|ui| {
                            if kit::menu_entry(ui, icons::X, "Fechar", "Ctrl+W", true, false, p) {
                                close = Some(i);
                            }
                            if kit::menu_entry(ui, "", "Fechar as outras", "", n_tabs > 1, false, p) {
                                tab_cmd = Some((i, TabCmd::CloseOthers));
                            }
                            if kit::menu_entry(ui, "", "Fechar as da direita", "", i + 1 < n_tabs, false, p) {
                                tab_cmd = Some((i, TabCmd::CloseRight));
                            }
                            if i != app.active && kit::menu_entry(ui, icons::PANEL_RIGHT, "Abrir ao lado", "", true, false, p) {
                                tab_cmd = Some((i, TabCmd::Beside));
                            }
                            if is_file {
                                kit::menu_sep(ui, p);
                                if kit::menu_item(ui, icons::COPY, "Copiar caminho", p) {
                                    ui.ctx().copy_text(t.path.clone());
                                }
                                if kit::menu_item(ui, icons::LINK, "Copiar link", p) {
                                    ui.ctx().copy_text(format!("[[{}]]", vault::basename(&t.path)));
                                }
                                if kit::menu_item(ui, icons::FOLDER_OPEN, "Mostrar na pasta", p) {
                                    tab_cmd = Some((i, TabCmd::Reveal));
                                }
                                if kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", p) {
                                    tab_cmd = Some((i, TabCmd::OpenExternal));
                                }
                            }
                        });
                    }
                    // where a dragged tab would land
                    if let Some((from, x, released)) = drag {
                        let to = (((x - first_x) / tab_step.max(1.0)).floor().max(0.0) as usize).min(n_tabs.saturating_sub(1));
                        if released {
                            if to != from {
                                let t = app.tabs.remove(from);
                                app.tabs.insert(to, t);
                                // the active tab stays the same tab
                                app.active = if app.active == from {
                                    to
                                } else if from < app.active && to >= app.active {
                                    app.active - 1
                                } else if from > app.active && to <= app.active {
                                    app.active + 1
                                } else {
                                    app.active
                                };
                                app.save_session();
                            }
                        } else if to != from {
                            let lx = first_x + to as f32 * tab_step + if to > from { tab_step - 2.0 } else { -1.0 };
                            let top = ui.min_rect().top();
                            ui.painter().vline(lx, egui::Rangef::new(top + 4.0, top + 31.0), egui::Stroke::new(2.0_f32, p.accent_strong));
                        }
                    }
                    if kit::icon_button_sized(ui, icons::PLUS, "Nova nota", false, p, 26.0, 14.0).clicked() {
                        app.new_note("");
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let (g, tip) = if app.right_open { (icons::PANEL_RIGHT_CLOSE, "Esconder painel direito") } else { (icons::PANEL_RIGHT_OPEN, "Mostrar painel direito") };
                if icon_button(ui, g, tip, false, p).clicked() {
                    app.right_open = !app.right_open;
                }
                if let Some(t) = app.tabs.get_mut(app.active) {
                    if t.view.is_none() && t.board.is_none() && t.quadro.is_none() {
                        focus = kit::icon_button(ui, icons::FOCUS, "Modo foco", false, p).clicked();
                        // the note's alignment, kept in its frontmatter
                        let cur = crate::align::TextAlign::of_note(&t.text);
                        let ar = icon_button(ui, cur.icon(), "Alinhamento do texto", false, p);
                        let aid = ui.make_persistent_id("note-align");
                        if ar.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(aid));
                        }
                        egui::popup::popup_above_or_below_widget(ui, aid, &ar, egui::AboveOrBelow::Below, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(200.0);
                            for a in crate::align::ALL {
                                if kit::menu_entry(ui, a.icon(), a.label(), if a == cur { "✓" } else { "" }, true, false, p) && a != cur {
                                    crate::align::apply(ui.ctx(), t, a);
                                }
                            }
                        });
                        let (g, tip) = if t.mode == Mode::Edit { (icons::BOOK_OPEN, "Modo leitura (Ctrl+E)") } else { (icons::PENCIL, "Modo edição (Ctrl+E)") };
                        if icon_button(ui, g, tip, false, p).clicked() {
                            t.mode = if t.mode == Mode::Edit { Mode::Read } else { Mode::Edit };
                        }
                    }
                }
            });
        });
        if let Some(i) = select {
            app.active = i;
        }
        if let Some(i) = close {
            app.close_tab(i);
        }
        if let Some((i, cmd)) = tab_cmd {
            match cmd {
                TabCmd::CloseOthers => {
                    for j in (0..app.tabs.len()).rev().filter(|j| *j != i) {
                        app.close_tab(j);
                    }
                }
                TabCmd::CloseRight => {
                    for j in (i + 1..app.tabs.len()).rev() {
                        app.close_tab(j);
                    }
                }
                TabCmd::Beside => {
                    if let Some(t) = app.tabs.get(i) {
                        app.split = Some(t.path.clone());
                        app.split_right = true;
                    }
                }
                TabCmd::Reveal | TabCmd::OpenExternal => {
                    if let (Some(v), Some(t)) = (&app.vault, app.tabs.get(i)) {
                        let abs = v.abs(&t.path);
                        let target = if matches!(cmd, TabCmd::Reveal) { abs.parent().map(|d| d.to_path_buf()).unwrap_or(abs) } else { abs };
                        let _ = open::that(target);
                    }
                }
            }
        }
        if focus {
            app.toggle_focus();
        }
    });
}

pub fn status_bar(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::TopBottomPanel::bottom("status").show_separator_line(false).exact_height(26.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(14, 4))).show(ctx, |ui| {
        ui.horizontal(|ui| {
            let small = |s: String| RichText::new(s).size(11.5).color(p.faint);
            if app.vault.as_ref().is_some_and(|v| !v.indexed.load(std::sync::atomic::Ordering::SeqCst)) {
                ui.add(egui::Spinner::new().size(11.0).color(p.faint));
                ui.label(small("Indexando…".into()));
            }
            if app.templater.running > 0 {
                ui.label(small("Executando modelo…".into()));
            }
            mini_player(app, ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                if let Some(s) = app.sprint.as_ref() {
                    let words = app.sc.as_ref().map(|s| s.settings.today_words(None)).unwrap_or(0);
                    if crate::sc::focus::status_pill(ui, p, s, words).clicked() {
                        if let Some(s) = app.sprint.as_mut() {
                            s.toggle_pause();
                        }
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                }
                if let Some(t) = app.tabs.get(app.active).filter(|t| t.view.is_none() && t.board.is_none() && t.quadro.is_none()) {
                    let words = ateliet_core::scriptorium::count_words(&t.text);
                    let chars = t.text.chars().count();
                    let back = app.vault.as_ref().map(|v| v.index.read().map(|ix| ix.inlinks(&t.path).len()).unwrap_or(0)).unwrap_or(0);
                    ui.label(small(format!("{back} {}", if back == 1 { "backlink" } else { "backlinks" })));
                    ui.label(small(format!("{} caracteres", ateliet_core::scriptorium::fmt_n(chars as i64))));
                    ui.label(small(format!("{} palavras", ateliet_core::scriptorium::fmt_n(words as i64))));
                    if let Some(sc) = app.sc.as_ref() {
                        let today = sc.settings.today_words(None);
                        if today > 0 {
                            ui.label(RichText::new(format!("{} hoje", ateliet_core::scriptorium::fmt_n(today))).size(11.5).color(p.accent_strong));
                        }
                    }
                }
            });
        });
    });
}

/// What plays, with play/pause and next; its menu writes with music or opens the soundtrack.
fn mini_player(app: &mut App, ui: &mut egui::Ui) {
    let p = app.palette;
    let now = app.music.now.clone();
    let mut cmd = None;
    let mut write = false;
    let mut open = false;
    ui.spacing_mut().item_spacing.x = 4.0;
    let menu = |ui: &mut egui::Ui, write: &mut bool, open: &mut bool| {
        if kit::menu_item(ui, icons::PEN_LINE, "Escrever ouvindo…", p) {
            *write = true;
            ui.close_menu();
        }
        if kit::menu_item(ui, icons::MUSIC, "Abrir a Trilha do livro", p) {
            *open = true;
            ui.close_menu();
        }
    };
    if now.some() {
        if kit::icon_button_sized(ui, icons::SKIP_BACK, "Anterior", false, p, 20.0, 12.0).clicked() {
            cmd = Some(crate::music::Cmd::Previous);
        }
        let g = if now.playing { icons::PAUSE } else { icons::PLAY };
        if kit::icon_button_sized(ui, g, if now.playing { "Pausar" } else { "Tocar" }, false, p, 20.0, 12.0).clicked() {
            cmd = Some(crate::music::Cmd::PlayPause);
        }
        if kit::icon_button_sized(ui, icons::SKIP_FORWARD, "Próxima", false, p, 20.0, 12.0).clicked() {
            cmd = Some(crate::music::Cmd::Next);
        }
        let lab = if now.artist.is_empty() { now.name.clone() } else { format!("{} — {}", now.name, now.artist) };
        let g = crate::sc::enredo_ui::truncated(ui, &lab, egui::FontId::proportional(11.5), if now.playing { p.text } else { p.faint }, 280.0);
        let r = ui.add(egui::Label::new(g).sense(egui::Sense::click()));
        if now.dur > 0.0 {
            let f = (now.pos / now.dur).clamp(0.0, 1.0) as f32;
            let y = r.rect.bottom() + 1.0;
            ui.painter().line_segment([egui::pos2(r.rect.left(), y), egui::pos2(r.rect.right(), y)], egui::Stroke::new(1.5_f32, p.border));
            ui.painter().line_segment([egui::pos2(r.rect.left(), y), egui::pos2(r.rect.left() + r.rect.width() * f, y)], egui::Stroke::new(1.5_f32, p.accent_strong));
        }
        r.context_menu(|ui| menu(ui, &mut write, &mut open));
        if r.clicked() {
            open = true;
        }
    } else {
        let r = kit::icon_button_sized(ui, icons::MUSIC, "Música para escrever", false, p, 20.0, 12.0);
        let id = ui.make_persistent_id("mini-player-menu");
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        egui::popup_above_or_below_widget(ui, id, &r, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(220.0);
            menu(ui, &mut write, &mut open);
        });
    }
    if let Some(c) = cmd {
        app.sc_apply(crate::sc::Action::Music(c));
    }
    if write {
        app.write_with();
    }
    if open {
        match app.current_book() {
            Some(b) => app.open_view("trilha", Some(b.path)),
            None => app.toast("Abra um capítulo ou escolha um livro na Biblioteca."),
        }
    }
}

pub fn empty_state(app: &mut App, ui: &mut egui::Ui) {
    let p = app.palette;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.26).max(30.0));
        ui.label(kit::icon(icons::FEATHER, 30.0).color(p.faint));
        ui.add_space(14.0);
        ui.label(RichText::new("Nada aberto").font(kit::medium(17.0)).color(p.text));
        ui.add_space(4.0);
        ui.label(RichText::new("Comece uma nota, abra um arquivo ou volte ao livro.").color(p.muted));
        ui.add_space(22.0);
        ui.allocate_ui(egui::vec2(300.0, 200.0), |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let rows = [(icons::SQUARE_PEN, "Nova nota", "Ctrl+N"), (icons::SEARCH, "Ir para arquivo", "Ctrl+O"), (icons::CALENDAR_DAYS, "Nota de hoje", ""), (icons::LIBRARY_BIG, "Biblioteca", "")];
            for (i, (g, label, key)) in rows.into_iter().enumerate() {
                if kit::Row::new(label).icon(g).trailing(key).show(ui, p).clicked() {
                    match i {
                        0 => app.new_note(""),
                        1 => app.palette_ui = Some(crate::commands::Palette::files()),
                        2 => app.open_daily(ateliet_core::datefmt::today().date()),
                        _ => app.open_view("library", None),
                    }
                }
            }
            let recent: Vec<String> = app.recent.iter().filter(|r| !r.starts_with("scriptorium:")).take(6).cloned().collect();
            if !recent.is_empty() {
                ui.add_space(18.0);
                kit::section(ui, "Recentes", p);
                for r in recent {
                    if kit::Row::new(vault::basename(&r)).icon(icons::FILE_TEXT).show(ui, p).clicked() {
                        app.open(&r, true);
                    }
                }
            }
        });
    });
}

// ── dialogs ────────────────────────────────────────────────────────────────

pub fn ask_dialog(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    let Some(ask) = &mut app.ask else { return };
    let mut done: Option<bool> = None;
    let confirm = matches!(ask.kind, AskKind::ConfirmDelete(_) | AskKind::ConfirmDeleteMany(_));
    kit::scrim(ctx, p);
    egui::Window::new(ask.title.clone()).title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 140.0]).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(RichText::new(&ask.title).font(kit::medium(15.0)).color(p.text));
        ui.add_space(12.0);
        if confirm {
            ui.label(RichText::new("Os arquivos vão para a lixeira.").color(p.muted));
        } else {
            let r = ui.add(egui::TextEdit::singleline(&mut ask.value).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 7)));
            r.request_focus();
        }
        ui.add_space(16.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = if confirm { kit::danger(ui, "Excluir", p) } else { kit::primary(ui, "Confirmar", p) };
            if ok.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
        });
    });
    if let Some(ok) = done {
        let ask = app.ask.take().unwrap();
        if !ok {
            return;
        }
        let value = ask.value.trim().to_string();
        match ask.kind {
            AskKind::Rename(from) if !value.is_empty() => app.rename(&from, &value),
            AskKind::NewFolder(parent) if !value.is_empty() => {
                if let Some(v) = &app.vault {
                    let path = if parent.is_empty() { value } else { format!("{parent}/{value}") };
                    let _ = std::fs::create_dir_all(v.abs(&path));
                    app.vault.as_mut().unwrap().rescan();
                }
            }
            AskKind::ConfirmDelete(path) => app.delete(&path),
            AskKind::ConfirmDeleteMany(items) => {
                // a folder's contents go with it
                for it in items.iter().filter(|x| !items.iter().any(|y| y != *x && x.starts_with(&format!("{y}/")))) {
                    app.delete(it);
                }
                ctx.data_mut(|d| d.remove::<Selection>(Selection::id()));
            }
            AskKind::MoveMany(items) => {
                let folder = value.trim_matches('/').to_string();
                if let Some(v) = &app.vault {
                    let _ = std::fs::create_dir_all(v.abs(&folder));
                }
                for it in items.iter().filter(|x| !items.iter().any(|y| y != *x && x.starts_with(&format!("{y}/")))) {
                    if *it != folder && !folder.starts_with(&format!("{it}/")) {
                        move_into(app, it, &folder);
                    }
                }
                ctx.data_mut(|d| d.remove::<Selection>(Selection::id()));
            }
            _ => {}
        }
    }
}

pub fn toasts(app: &mut App, ctx: &egui::Context) {
    app.toasts.retain(|t| t.until > std::time::Instant::now());
    if app.toasts.is_empty() {
        return;
    }
    let p = app.palette;
    egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -44.0]).show(ctx, |ui| {
        for t in &app.toasts {
            let fill = if p.dark { egui::Color32::from_rgb(0xee, 0xee, 0xea) } else { egui::Color32::from_rgb(0x22, 0x22, 0x21) };
            let text = if p.dark { egui::Color32::from_rgb(0x1a, 0x1a, 0x1a) } else { egui::Color32::from_rgb(0xf4, 0xf4, 0xf1) };
            egui::Frame::new().fill(fill).corner_radius(9.0).inner_margin(egui::Margin::symmetric(14, 9)).shadow(egui::Shadow { offset: [0, 6], blur: 20, spread: 0, color: egui::Color32::from_black_alpha(40) }).show(ui, |ui| {
                ui.label(RichText::new(&t.text).color(text));
            });
            ui.add_space(6.0);
        }
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}

pub fn settings_window(app: &mut App, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    let p = app.palette;
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut retheme = false;
    let page_id = egui::Id::new("settings-page");
    let mut page: usize = ctx.data(|d| d.get_temp(page_id)).unwrap_or(0);
    kit::scrim(ctx, p);
    let pages = [(icons::PALETTE, "Aparência"), (icons::PEN_LINE, "Editor"), (icons::CALENDAR_DAYS, "Notas diárias"), (icons::LAYOUT_TEMPLATE, "Templater"), (icons::TABLE, "Dataview"), (icons::CALENDAR, "Calendário"), (icons::IMAGES, "Imagens"), (icons::FOLDER, "Arquivos e links"), (icons::COMMAND, "Atalhos"), (icons::INFO, "Sobre")];
    let win = egui::Window::new("settings").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).frame(egui::Frame::window(&ctx.style()).inner_margin(egui::Margin::ZERO).corner_radius(14.0)).show(ctx, |ui| {
        let Some(cfg) = app.cfg.clone() else { return };
        ui.set_width(760.0);
        ui.set_height(520.0);
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        ui.horizontal_top(|ui| {
            // categories
            let (side, _) = ui.allocate_exact_size(egui::vec2(200.0, 520.0), Sense::hover());
            ui.painter().rect_filled(side, egui::CornerRadius { nw: 14, sw: 14, ne: 0, se: 0 }, p.bg_secondary);
            ui.painter().vline(side.right() - 0.5, side.y_range(), egui::Stroke::new(1.0_f32, p.border));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(side.shrink2(egui::vec2(10.0, 14.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Configurações").font(kit::medium(14.0)).color(p.text));
                ui.add_space(12.0);
                for (i, (g, label)) in pages.iter().enumerate() {
                    if kit::Row::new(label).icon(g).selected(page == i).show(ui, p).clicked() {
                        page = i;
                    }
                }
            });
            // the page
            let (body, _) = ui.allocate_exact_size(egui::vec2(560.0, 520.0), Sense::hover());
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body.shrink2(egui::vec2(28.0, 22.0))).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(pages[page].1).font(kit::bold(18.0)).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::icon_button(ui, icons::X, "Fechar (Esc)", false, p).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(18.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let field = |ui: &mut egui::Ui, v: &mut String| ui.add(egui::TextEdit::singleline(v).desired_width(220.0).margin(egui::Margin::symmetric(8, 5))).changed();
                    match page {
                        0 if app.theme_browser.open => {
                            let cur = app.appearance.css_theme.clone();
                            if let Some(t) = app.theme_browser.ui(ui, &cfg.dir.join("themes"), &cur, p) {
                                app.appearance.css_theme = t;
                                app.appearance.load_css(&cfg);
                                retheme = true;
                            }
                        }
                        0 => {
                            if kit::setting(ui, "Tema escuro", "Interface e notas em fundo escuro.", p, |ui| kit::toggle(ui, &mut app.appearance.dark, p).changed()) {
                                retheme = true;
                            }
                            let changed = kit::setting(ui, "Cor de destaque", "Links, seleção e botões principais.", p, |ui| {
                                let mut c = egui::Color32::from_rgb(app.appearance.accent[0], app.appearance.accent[1], app.appearance.accent[2]);
                                let r = ui.color_edit_button_srgba(&mut c);
                                if r.changed() {
                                    app.appearance.accent = [c.r(), c.g(), c.b()];
                                }
                                r.changed()
                            });
                            retheme |= changed;
                            let themes = crate::config::Appearance::themes(&cfg);
                            let cur = app.appearance.css_theme.clone();
                            let mut pick: Option<String> = None;
                            kit::setting(ui, "Tema da comunidade", "As cores do tema instalado em .obsidian/themes (o resto do CSS não é aplicado).", p, |ui| {
                                egui::ComboBox::from_id_salt("css-theme").width(180.0).selected_text(if cur.is_empty() { "Nenhum".to_string() } else { cur.clone() }).show_ui(ui, |ui| {
                                    if ui.selectable_label(cur.is_empty(), "Nenhum").clicked() {
                                        pick = Some(String::new());
                                    }
                                    for t in &themes {
                                        if ui.selectable_label(&cur == t, t.as_str()).clicked() {
                                            pick = Some(t.clone());
                                        }
                                    }
                                });
                            });
                            kit::setting(ui, "Explorar temas", "O catálogo de temas da comunidade do Obsidian, com prévias.", p, |ui| {
                                if kit::button(ui, Some(icons::PALETTE), "Explorar…", p).clicked() {
                                    app.theme_browser.open = true;
                                }
                            });
                            if let (Some(t), c) = (pick, &cfg) {
                                app.appearance.css_theme = t;
                                app.appearance.load_css(c);
                                retheme = true;
                            }
                            if kit::setting(ui, "Tamanho do texto", "O tamanho das notas; a interface não muda.", p, |ui| {
                                ui.spacing_mut().slider_width = 140.0;
                                ui.add(egui::Slider::new(&mut app.appearance.font_size, 12.0..=26.0).max_decimals(0)).changed()
                            }) {
                                retheme = true;
                            }
                        }
                        1 => {
                            if kit::setting(ui, "Largura de leitura", "Limita a largura do texto para uma leitura confortável.", p, |ui| kit::toggle(ui, &mut app.readable, p).changed()) {
                                cfg.merge("app.json", serde_json::json!({ "readableLineLength": app.readable }));
                            }
                            let mut read = app.default_mode == Mode::Read;
                            if kit::setting(ui, "Abrir em modo leitura", "Notas abrem renderizadas; Ctrl+E alterna.", p, |ui| kit::toggle(ui, &mut read, p).changed()) {
                                app.default_mode = if read { Mode::Read } else { Mode::Edit };
                                cfg.merge("app.json", serde_json::json!({ "defaultViewMode": if read { "preview" } else { "source" } }));
                            }
                            if kit::setting(ui, "Pré-visualização ao vivo", "Mostra a nota formatada, exceto o bloco em edição.", p, |ui| kit::toggle(ui, &mut app.live_preview, p).changed()) {
                                cfg.merge("app.json", serde_json::json!({ "livePreview": app.live_preview }));
                            }
                        }
                        2 => {
                            let mut changed = false;
                            changed |= kit::setting(ui, "Formato da data", "Como o nome da nota diária é escrito (ex.: YYYY-MM-DD).", p, |ui| field(ui, &mut app.daily.format));
                            changed |= kit::setting(ui, "Pasta", "Onde as notas diárias são criadas.", p, |ui| field(ui, &mut app.daily.folder));
                            changed |= kit::setting(ui, "Modelo", "Nota usada como ponto de partida.", p, |ui| field(ui, &mut app.daily.template));
                            if changed {
                                app.daily.save(&cfg);
                            }
                        }
                        3 => {
                            let mut changed = false;
                            if kit::setting(ui, "Pasta de modelos", "Onde ficam os modelos do Templater.", p, |ui| field(ui, &mut app.templater_settings.folder)) {
                                changed = true;
                            }
                            changed |= kit::setting(ui, "Rodar ao criar notas", "Uma nota nova recebe o modelo da pasta onde foi criada (a mais funda que tiver um).", p, |ui| kit::toggle(ui, &mut app.templater_settings.trigger_on_create, p).changed());
                            ui.add_space(10.0);
                            ui.label(RichText::new("Modelos por pasta").font(kit::medium(13.0)).color(p.text));
                            let mut remove = None;
                            for (i, (folder, template)) in app.templater_settings.folder_templates.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    changed |= ui.add(egui::TextEdit::singleline(folder).hint_text("pasta").desired_width(170.0)).changed();
                                    ui.label(RichText::new("→").color(p.faint));
                                    changed |= ui.add(egui::TextEdit::singleline(template).hint_text("modelo").desired_width(220.0)).changed();
                                    if kit::icon_button_sized(ui, icons::X, "Remover", false, p, 22.0, 12.0).clicked() {
                                        remove = Some(i);
                                    }
                                });
                            }
                            if let Some(i) = remove {
                                app.templater_settings.folder_templates.remove(i);
                                changed = true;
                            }
                            if kit::button(ui, Some(icons::PLUS), "Modelo para uma pasta", p).clicked() {
                                app.templater_settings.folder_templates.push((String::new(), String::new()));
                            }
                            ui.add_space(10.0);
                            ui.label(RichText::new("Modelos na paleta de comandos").font(kit::medium(13.0)).color(p.text));
                            ui.label(RichText::new("Cada um vira um comando “Templater: Inserir …”, que pode ganhar atalho em hotkeys.json.").size(12.0).color(p.faint));
                            let mut remove = None;
                            for (i, t) in app.templater_settings.hotkey_templates.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    changed |= ui.add(egui::TextEdit::singleline(t).hint_text("modelo").desired_width(400.0)).changed();
                                    if kit::icon_button_sized(ui, icons::X, "Remover", false, p, 22.0, 12.0).clicked() {
                                        remove = Some(i);
                                    }
                                });
                            }
                            if let Some(i) = remove {
                                app.templater_settings.hotkey_templates.remove(i);
                                changed = true;
                            }
                            if kit::button(ui, Some(icons::PLUS), "Modelo na paleta", p).clicked() {
                                app.templater_settings.hotkey_templates.push(String::new());
                            }
                            if changed {
                                app.templater_settings.save(&cfg);
                            }
                        }
                        4 => {
                            let mut dv_changed = false;
                            dv_changed |= kit::setting(ui, "Formato de data", "Como as datas aparecem em consultas.", p, |ui| field(ui, &mut app.dv_settings.date_format));
                            dv_changed |= kit::setting(ui, "Formato de data e hora", "", p, |ui| field(ui, &mut app.dv_settings.datetime_format));
                            if dv_changed {
                                app.caches.clear_queries();
                                let mut v = cfg.tool("dataview", "dataview");
                                if !v.is_object() {
                                    v = serde_json::json!({});
                                }
                                v["defaultDateFormat"] = serde_json::json!(app.dv_settings.date_format);
                                v["defaultDateTimeFormat"] = serde_json::json!(app.dv_settings.datetime_format);
                                cfg.merge("ateliet/dataview.json", v);
                            }
                        }
                        5 => {
                            let c = &mut app.calendar;
                            let mut wpd = c.words_per_dot as f32;
                            if kit::setting(ui, "Palavras por ponto", "Cada ponto sob um dia representa esta quantidade de palavras.", p, |ui| {
                                ui.spacing_mut().slider_width = 140.0;
                                ui.add(egui::Slider::new(&mut wpd, 50.0..=1000.0).max_decimals(0)).changed()
                            }) {
                                c.words_per_dot = wpd as u32;
                                cfg.merge("ateliet/calendar.json", serde_json::json!({ "wordsPerDot": c.words_per_dot }));
                            }
                        }
                        6 => {
                            let root = app.vault.as_ref().map(|v| v.root.clone()).unwrap_or_default();
                            // the library state (and its automatic detection) loaded once
                            app.images.open(&root, ui.ctx());
                            let st = crate::images_ui::ImagesState::settings(&root);
                            let shared = st["shared"].as_str().unwrap_or("auto").to_string();
                            let mut rescan = false;
                            kit::setting(ui, "Biblioteca compartilhada", "A pasta de dados do Pictogrep: as mesmas imagens e coleções nos dois apps.", p, |ui| {
                                let label = match shared.as_str() {
                                    "auto" => "Automática".to_string(),
                                    "" => "Desligada".to_string(),
                                    path => path.rsplit(['/', '\\']).next().unwrap_or(path).to_string(),
                                };
                                egui::ComboBox::from_id_salt("img-shared").width(170.0).selected_text(label).show_ui(ui, |ui| {
                                    if ui.selectable_label(shared == "auto", "Automática (a do Pictogrep)").clicked() {
                                        crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({ "shared": "auto" }));
                                        rescan = true;
                                    }
                                    if ui.selectable_label(shared.is_empty(), "Desligada").clicked() {
                                        crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({ "shared": "" }));
                                        rescan = true;
                                    }
                                    if ui.selectable_label(false, "Escolher pasta…").clicked() {
                                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                            crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({ "shared": d.to_string_lossy() }));
                                            rescan = true;
                                        }
                                    }
                                });
                            });
                            // the library the setting points at (image mode may not have loaded it yet)
                            let in_use = match shared.trim() {
                                "" => None,
                                "auto" => app.images.shared.clone(),
                                path => Some(ateliet_core::pictogrep::Home::new(std::path::PathBuf::from(path))).filter(|h| h.root.is_dir()),
                            };
                            match &in_use {
                                Some(h) => ui.label(RichText::new(format!("Em uso: {}{}", h.root.display(), if ateliet_core::pictogrep::is_running(h) { " (o Pictogrep está aberto)" } else { "" })).size(11.5).color(p.faint)),
                                None => ui.label(RichText::new("Nenhuma biblioteca compartilhada em uso.").size(11.5).color(p.faint)),
                            };
                            ui.add_space(6.0);
                            let mut show_vault = st["show_vault"].as_bool().unwrap_or(false);
                            kit::setting(ui, "Imagens do cofre no modo imagens", "Desligado, o modo imagens mostra só a biblioteca (os downloads do Pinterest) e as pastas de fora do cofre. O painel de imagens da escrita continua com todas.", p, |ui| {
                                if kit::toggle(ui, &mut show_vault, p).changed() {
                                    crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({ "show_vault": show_vault }));
                                    app.images.show_vault = show_vault;
                                }
                            });
                            ui.add_space(6.0);
                            rescan |= crate::settings_pages::copy_library_ui(app, ui, &root, p);
                            ui.add_space(10.0);
                            ui.label(RichText::new("Pastas de fora do cofre").font(kit::medium(13.0)).color(p.text));
                            ui.label(RichText::new("As imagens ficam onde estão; só são copiadas para o cofre quando usadas numa nota.").size(11.5).color(p.faint));
                            let external: Vec<String> = st["external"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                            let mut remove = None;
                            for e in &external {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(e).size(12.5).color(p.text));
                                    if kit::icon_button_sized(ui, icons::X, "Tirar da biblioteca", false, p, 20.0, 11.0).clicked() {
                                        remove = Some(e.clone());
                                    }
                                });
                            }
                            if kit::button(ui, Some(icons::FOLDER_CLOSED), "Adicionar pasta…", p).clicked() {
                                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                    let d = d.to_string_lossy().to_string();
                                    if !d.starts_with(&root.to_string_lossy().to_string()) && !external.contains(&d) {
                                        let mut list = external.clone();
                                        list.push(d);
                                        crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({ "external": list }));
                                        rescan = true;
                                    }
                                }
                            }
                            if let Some(r) = remove {
                                let list: Vec<String> = external.into_iter().filter(|x| *x != r).collect();
                                crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({ "external": list }));
                                rescan = true;
                            }
                            ui.add_space(10.0);
                            let mut folder = st["importFolder"].as_str().unwrap_or("").to_string();
                            if kit::setting(ui, "Pasta das imagens trazidas", "Onde ficam as cópias das imagens de fora usadas nas notas (vazio: a pasta da nota).", p, |ui| field(ui, &mut folder)) {
                                crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({ "importFolder": folder.trim().trim_matches('/') }));
                            }
                            if rescan {
                                app.images.rescan();
                            }
                        }
                        7 => crate::settings_pages::files_page(ui, &cfg, p),
                        8 => crate::settings_pages::hotkeys_page(app, ui, &cfg, p),
                        _ => {
                            ui.label(RichText::new("Ateliet").font(kit::bold(16.0)).color(p.text));
                            ui.add_space(4.0);
                            ui.label(RichText::new("Desenhado pelo próprio app, sem navegador embutido.").color(p.muted));
                            ui.add_space(16.0);
                            kit::section(ui, "Cofre", p);
                            ui.label(RichText::new(app.vault.as_ref().map(|v| v.root.display().to_string()).unwrap_or_default()).size(12.5).color(p.muted));
                            ui.add_space(16.0);
                            kit::section(ui, "Fontes", p);
                            ui.label(RichText::new("Inter (SIL OFL 1.1) · ícones Lucide (ISC)").size(12.5).color(p.muted));
                        }
                    }
                });
            });
        });
    });
    ctx.data_mut(|d| d.insert_temp(page_id, page));
    // a click outside closes (but not the click that opened it)
    if let Some(w) = win {
        let outside = ctx.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|pos| !w.response.rect.contains(pos) && pos.x > 48.0));
        if outside && !ctx.memory(|m| m.any_popup_open()) {
            close = true;
        }
    }
    if retheme {
        if let Some(cfg) = &app.cfg {
            app.appearance.save(cfg);
        }
        app.palette = crate::theme::apply(ctx, &app.appearance);
        app.caches.clear_queries();
    }
    if close {
        app.settings_open = false;
    }
}

pub fn vault_picker(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg_secondary)).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.16).max(30.0));
            ui.label(kit::icon(icons::FEATHER, 34.0).color(p.accent_strong));
            ui.add_space(12.0);
            ui.label(RichText::new("Ateliet").font(kit::bold(30.0)).color(p.text));
            ui.add_space(4.0);
            ui.label(RichText::new("Escolha um cofre para começar.").color(p.muted));
            ui.add_space(26.0);
            let mut chosen = None;
            egui::Frame::new().fill(p.bg).stroke(egui::Stroke::new(1.0_f32, p.border)).corner_radius(12.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                let vaults = config::known_vaults();
                if vaults.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Nenhum cofre aberto ainda.").color(p.faint));
                    ui.add_space(10.0);
                }
                for v in vaults.into_iter().take(10) {
                    let name = v.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let parent = v.parent().map(|d| d.display().to_string()).unwrap_or_default();
                    let mut row = kit::Row::new(&name).icon(icons::FOLDER_CLOSED).trailing(parent);
                    row.height = 38.0;
                    if row.show(ui, p).clicked() {
                        chosen = Some(v);
                    }
                }
            });
            ui.add_space(16.0);
            if kit::primary_icon(ui, icons::FOLDER_OPEN, "Abrir pasta como cofre", p).clicked() {
                chosen = rfd::FileDialog::new().pick_folder();
            }
            if let Some(v) = chosen {
                app.open_vault(ctx, v);
            }
        });
    });
}

/// "Atualizar o Ateliet": what changed upstream, the build's progress, restart.
pub fn update_window(app: &mut App, ctx: &egui::Context) {
    if !app.updater.open {
        return;
    }
    use crate::update::State;
    let p = app.palette;
    let mut close = false;
    let mut save_and_restart = false;
    kit::scrim(ctx, p);
    egui::Window::new("update").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0]).show(ctx, |ui| {
        let u = &mut app.updater;
        ui.set_width(520.0);
        ui.label(RichText::new("Atualizar o Ateliet").font(kit::medium(17.0)).color(p.text));
        ui.add_space(8.0);
        let mut auto = u.auto;
        if kit::setting(ui, "Atualizar automaticamente", "A cada meia hora o Ateliet olha o código de onde foi compilado; havendo commits novos, baixa e recompila em segundo plano, e a versão nova começa com o app.", p, |ui| kit::toggle(ui, &mut auto, p).changed()) {
            u.set_auto(auto);
        }
        ui.add_space(6.0);
        match u.state {
            State::Checking => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(RichText::new("Procurando atualizações…").color(p.muted));
                });
            }
            State::Building => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(RichText::new(format!("{}…", if u.step.is_empty() { "Começando" } else { &u.step })).color(p.muted));
                });
            }
            State::Ready => {
                ui.label(RichText::new("A versão nova está pronta. Reinicie o Ateliet para usá-la.").color(p.text));
            }
            State::Failed => {
                ui.label(RichText::new("A atualização falhou:").color(p.error));
            }
            State::Idle => match &u.status {
                Some(s) if s.available => {
                    ui.label(RichText::new(format!("{} {} em {}:", s.behind, if s.behind == 1 { "commit novo" } else { "commits novos" }, s.upstream)).color(p.text));
                    egui::ScrollArea::vertical().max_height(160.0).id_salt("upd-commits").show(ui, |ui| {
                        for c in &s.commits {
                            ui.label(RichText::new(c).size(12.0).monospace().color(p.muted));
                        }
                    });
                }
                Some(s) => {
                    ui.label(RichText::new(format!("O Ateliet está em dia ({}).", &s.current[..s.current.len().min(8)])).color(p.muted));
                }
                None => {}
            },
        }
        if let Some(e) = &u.error {
            ui.label(RichText::new(e).size(12.0).color(p.error));
        }
        if !u.log.is_empty() {
            ui.add_space(6.0);
            egui::Frame::new().fill(p.code_bg).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).id_salt("upd-log").show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for l in &u.log {
                        ui.label(RichText::new(l).size(11.0).monospace().color(p.muted));
                    }
                });
            });
        }
        ui.add_space(10.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match u.state {
                State::Ready => {
                    if kit::primary(ui, "Reiniciar agora", p).clicked() {
                        save_and_restart = true;
                    }
                }
                State::Idle if u.status.as_ref().is_some_and(|s| s.available) => {
                    if kit::primary(ui, "Atualizar", p).clicked() {
                        u.update();
                    }
                }
                State::Idle | State::Failed => {
                    if kit::button(ui, Some(icons::ROTATE_CW), "Procurar de novo", p).clicked() {
                        u.state = State::Idle;
                        u.check();
                    }
                }
                _ => {}
            }
            if kit::button(ui, None, "Fechar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if close {
        app.updater.open = false;
    }
    if save_and_restart {
        app.save_all();
        app.updater.restart();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel() -> Selection {
        Selection { order: ["A", "B", "C", "D", "E"].iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }
    const SHIFT: egui::Modifiers = egui::Modifiers { shift: true, ..egui::Modifiers::NONE };
    const CMD: egui::Modifiers = egui::Modifiers::COMMAND;

    #[test]
    fn shift_selects_a_range_from_the_last_click() {
        let mut s = sel();
        assert!(!s.click("B", egui::Modifiers::NONE, None), "a plain click opens");
        assert!(s.click("D", SHIFT, None));
        assert_eq!(s.items, ["B", "C", "D"]);
        // the anchor stays: Shift again moves the other end
        s.click("A", SHIFT, None);
        assert_eq!(s.items, ["A", "B"]);
        // with nothing clicked yet, the range starts at the open note
        let mut s = sel();
        s.click("E", SHIFT, Some("C"));
        assert_eq!(s.items, ["C", "D", "E"]);
    }

    #[test]
    fn command_adds_and_removes_one() {
        let mut s = sel();
        s.click("D", CMD, Some("A"));
        assert_eq!(s.items, ["A", "D"], "the open note starts the selection");
        s.click("A", CMD, Some("A"));
        assert_eq!(s.items, ["D"]);
        assert!(s.group("D").is_none(), "one item is not a group");
        s.click("E", CMD, None);
        assert_eq!(s.group("E").unwrap(), ["D", "E"]);
        s.click("B", egui::Modifiers::NONE, None);
        assert!(s.items.is_empty());
    }
}
