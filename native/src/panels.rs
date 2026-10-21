//! Sidebars and chrome: ribbon, file explorer, search, bookmarks, backlinks,
//! outline, tags, calendar, tab bar, status bar, dialogs and settings.

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
                if icon_button(ui, icons::ARCHIVE, "Abrir outro cofre", false, p).clicked() {
                    app.save_all();
                    app.vault = None;
                    app.tabs.clear();
                }
            });
        });
    });
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
            if kit::icon_button_sized(ui, icons::SQUARE_PEN, "Nova nota (Ctrl+N)", false, p, 24.0, 14.0).clicked() {
                app.new_note("");
            }
        });
    });
    ui.add_space(4.0);
    let active = app.active_path();
    let mut open: Option<(String, bool)> = None;
    let mut ask: Option<Ask> = None;
    let mut new_in: Option<String> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        folder_ui(ui, &tree, "", 0, active.as_deref(), &mut open, &mut ask, &mut new_in, p);
    });
    if let Some((p, nt)) = open {
        app.open(&p, nt);
    }
    if let Some(a) = ask {
        app.ask = Some(a);
    }
    if let Some(f) = new_in {
        app.new_note(&f);
    }
}

fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "md" => icons::FILE_TEXT,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "psd" | "clip" => icons::IMAGE,
        "storyboard" => icons::CLAPPERBOARD,
        "canvas" => icons::NETWORK,
        "mp3" | "wav" | "ogg" | "m4a" | "flac" => icons::MUSIC,
        "mp4" | "mov" | "webm" => icons::FILM,
        _ => icons::FILE,
    }
}

#[allow(clippy::too_many_arguments)]
fn folder_ui(ui: &mut egui::Ui, f: &Folder, path: &str, depth: usize, active: Option<&str>, open: &mut Option<(String, bool)>, ask: &mut Option<Ask>, new_in: &mut Option<String>, p: Palette) {
    for (name, sub) in &f.folders {
        let full = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
        let id = ui.make_persistent_id(("folder", &full));
        let is_open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let r = kit::Row::new(name).depth(depth).chevron(is_open).show(ui, p);
        if r.clicked() {
            ui.data_mut(|d| d.insert_temp(id, !is_open));
        }
        r.context_menu(|ui| {
            if ui.button("Nova nota").clicked() {
                *new_in = Some(full.clone());
                ui.close_menu();
            }
            if ui.button("Nova pasta").clicked() {
                *ask = Some(Ask { title: "Nova pasta".into(), value: String::new(), kind: AskKind::NewFolder(full.clone()) });
                ui.close_menu();
            }
            if ui.button("Renomear").clicked() {
                *ask = Some(Ask { title: "Renomear pasta".into(), value: name.clone(), kind: AskKind::Rename(full.clone()) });
                ui.close_menu();
            }
            if ui.button("Excluir").clicked() {
                *ask = Some(Ask { title: format!("Excluir a pasta “{name}” e tudo dentro dela?"), value: String::new(), kind: AskKind::ConfirmDelete(full.clone()) });
                ui.close_menu();
            }
        });
        if is_open {
            folder_ui(ui, sub, &full, depth + 1, active, open, ask, new_in, p);
        }
    }
    for (name, full) in &f.files {
        let is_md = name.to_lowercase().ends_with(".md");
        let label = if is_md { &name[..name.len() - 3] } else { name.as_str() };
        let is_active = active == Some(full.as_str());
        // files line up with the folder names above them (past the chevron)
        let r = kit::Row::new(label).depth(depth).icon(file_icon(name)).selected(is_active).dim(!is_md && !name.ends_with(".storyboard")).show(ui, p);
        if r.clicked() {
            *open = Some((full.clone(), ui.input(|i| i.modifiers.command)));
        }
        if r.middle_clicked() {
            *open = Some((full.clone(), true));
        }
        r.context_menu(|ui| {
            if ui.button("Abrir em nova aba").clicked() {
                *open = Some((full.clone(), true));
                ui.close_menu();
            }
            if ui.button("Renomear").clicked() {
                *ask = Some(Ask { title: "Renomear".into(), value: label.to_string(), kind: AskKind::Rename(full.clone()) });
                ui.close_menu();
            }
            if ui.button("Excluir").clicked() {
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
        header_tabs(ui, &mut tab, &[(RightTab::Calendar, icons::CALENDAR, "Calendário"), (RightTab::Backlinks, icons::LINK, "Backlinks"), (RightTab::Outline, icons::LIST, "Estrutura"), (RightTab::Tags, icons::HASH, "Tags"), (RightTab::Manuscript, icons::BOOK_TEXT, "Manuscrito")], p);
        app.right_tab = tab;
        match app.right_tab {
            RightTab::Backlinks => backlinks(app, ui),
            RightTab::Outline => outline(app, ui),
            RightTab::Tags => tags(app, ui),
            RightTab::Calendar => calendar_panel(app, ui),
            RightTab::Manuscript => {
                let active = app.active_path();
                let acts = {
                    let (Some(v), Some(sc)) = (app.vault.as_ref(), app.sc.as_mut()) else { return };
                    let mut cx = crate::sc::library::Cx { vault: v, sc, caches: &mut app.caches, palette: app.palette };
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
    if let Some(p) = open {
        app.open_link(&p, false);
    }
}

fn outline(app: &mut App, ui: &mut egui::Ui) {
    let p = app.palette;
    let Some(tab) = app.tabs.get_mut(app.active) else { return };
    if tab.view.is_some() || tab.board.is_some() {
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
        let mut select = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (g, tip) = if app.left_open { (icons::PANEL_LEFT_CLOSE, "Esconder barra lateral") } else { (icons::PANEL_LEFT_OPEN, "Mostrar barra lateral") };
            if icon_button(ui, g, tip, false, p).clicked() {
                app.left_open = !app.left_open;
            }
            ui.add_space(4.0);
            let right_w = 70.0;
            let avail = (ui.available_width() - right_w).max(100.0);
            let n = app.tabs.len().max(1) as f32;
            let tab_w = (avail / n - 2.0).clamp(110.0, 220.0);
            egui::ScrollArea::horizontal().max_width(avail).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, t) in app.tabs.iter().enumerate() {
                        let active = i == app.active;
                        let (rect, r) = ui.allocate_exact_size(egui::vec2(tab_w, 35.0), Sense::click());
                        let rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - 0.5));
                        let hovered = r.hovered();
                        let shape = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y + 1.0));
                        if active {
                            ui.painter().rect(shape, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, p.bg, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                            // the active tab opens onto the page
                            ui.painter().hline(shape.x_range().shrink(1.0), shape.bottom() - 0.5, egui::Stroke::new(2.0_f32, p.bg));
                        } else if hovered {
                            ui.painter().rect_filled(shape.shrink2(egui::vec2(0.0, 3.0)).translate(egui::vec2(0.0, -2.0)), 7.0, p.bg_hover);
                        }
                        let glyph = match &t.view {
                            Some(_) => icons::LIBRARY_BIG,
                            None if t.board.is_some() => icons::CLAPPERBOARD,
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
                    if t.view.is_none() && t.board.is_none() {
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                if let Some(t) = app.tabs.get(app.active).filter(|t| t.view.is_none() && t.board.is_none()) {
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
    let confirm = matches!(ask.kind, AskKind::ConfirmDelete(_));
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
    let mut open = true;
    let mut retheme = false;
    egui::Window::new("Settings").open(&mut open).default_width(480.0).collapsible(false).show(ctx, |ui| {
        let Some(cfg) = app.cfg.clone() else { return };
        egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
            ui.heading("Appearance");
            if ui.checkbox(&mut app.appearance.dark, "Dark theme").changed() {
                retheme = true;
            }
            ui.horizontal(|ui| {
                ui.label("Accent colour");
                let mut c = egui::Color32::from_rgb(app.appearance.accent[0], app.appearance.accent[1], app.appearance.accent[2]);
                if ui.color_edit_button_srgba(&mut c).changed() {
                    app.appearance.accent = [c.r(), c.g(), c.b()];
                    retheme = true;
                }
            });
            if ui.add(egui::Slider::new(&mut app.appearance.font_size, 12.0..=26.0).text("Font size")).changed() {
                retheme = true;
            }
            if ui.checkbox(&mut app.readable, "Readable line length").changed() {
                cfg.merge("app.json", serde_json::json!({ "readableLineLength": app.readable }));
            }
            let mut read = app.default_mode == Mode::Read;
            if ui.checkbox(&mut read, "Open notes in reading view").changed() {
                app.default_mode = if read { Mode::Read } else { Mode::Edit };
                cfg.merge("app.json", serde_json::json!({ "defaultViewMode": if read { "preview" } else { "source" } }));
            }
            ui.separator();
            ui.heading("Daily notes");
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Date format");
                changed |= ui.text_edit_singleline(&mut app.daily.format).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Folder");
                changed |= ui.text_edit_singleline(&mut app.daily.folder).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Template");
                changed |= ui.text_edit_singleline(&mut app.daily.template).changed();
            });
            if changed {
                app.daily.save(&cfg);
            }
            ui.separator();
            ui.heading("Templater");
            ui.horizontal(|ui| {
                ui.label("Template folder");
                if ui.text_edit_singleline(&mut app.templater_settings.folder).changed() {
                    app.templater_settings.save(&cfg);
                }
            });
            ui.separator();
            ui.heading("Dataview");
            let mut dv_changed = false;
            ui.horizontal(|ui| {
                ui.label("Date format");
                dv_changed |= ui.text_edit_singleline(&mut app.dv_settings.date_format).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Date and time format");
                dv_changed |= ui.text_edit_singleline(&mut app.dv_settings.datetime_format).changed();
            });
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
            ui.separator();
            ui.heading("Calendar");
            let c = &mut app.calendar;
            let mut wpd = c.words_per_dot as f32;
            if ui.add(egui::Slider::new(&mut wpd, 50.0..=1000.0).text("Words per dot")).changed() {
                c.words_per_dot = wpd as u32;
                cfg.merge("ateliet/calendar.json", serde_json::json!({ "wordsPerDot": c.words_per_dot }));
            }
            ui.separator();
            ui.label(RichText::new(format!("Vault: {}", app.vault.as_ref().map(|v| v.root.display().to_string()).unwrap_or_default())).small().color(app.palette.muted));
        });
    });
    if retheme {
        if let Some(cfg) = &app.cfg {
            app.appearance.save(cfg);
        }
        app.palette = crate::theme::apply(ctx, &app.appearance);
        app.caches.clear_queries();
    }
    app.settings_open = open;
}

pub fn vault_picker(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.2);
            ui.label(RichText::new("Ateliet").size(34.0).family(egui::FontFamily::Name("bold".into())));
            ui.add_space(20.0);
            let mut chosen = None;
            for v in config::known_vaults().into_iter().take(12) {
                let name = v.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if ui.add(egui::Button::new(RichText::new(format!("{name}   {}", v.display())).size(15.0)).min_size(egui::vec2(420.0, 30.0))).clicked() {
                    chosen = Some(v);
                }
            }
            ui.add_space(14.0);
            if ui.button(RichText::new("Open folder as vault…").size(15.0)).clicked() {
                chosen = rfd::FileDialog::new().pick_folder();
            }
            if let Some(v) = chosen {
                app.open_vault(ctx, v);
            }
        });
    });
}
