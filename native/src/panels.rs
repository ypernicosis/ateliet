//! Sidebars and chrome: ribbon, file explorer, search, bookmarks, backlinks,
//! outline, tags, calendar, tab bar, status bar, dialogs and settings.

use crate::app::{App, Ask, AskKind, LeftTab, Mode, RightTab};
use crate::config::{self, VaultConfig};
use crate::vault::{self, Folder};
use ateliet_core::calendar::{self, MonthRequest};
use ateliet_core::search;
use chrono::{Datelike, NaiveDate};
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
    pub took_ms: u128,
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

fn icon_button(ui: &mut egui::Ui, icon: &str, tip: &str, active: bool) -> egui::Response {
    let r = ui.add(egui::Button::new(RichText::new(icon).size(17.0)).frame(false).min_size(egui::vec2(30.0, 30.0)).selected(active));
    r.on_hover_text(tip)
}

pub fn ribbon(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::SidePanel::left("ribbon").exact_width(44.0).resizable(false).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin::symmetric(6, 8)).stroke(egui::Stroke::new(1.0, p.border))).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            if icon_button(ui, "🗀", "Files", app.left_open && app.left_tab == LeftTab::Files).clicked() {
                toggle_left(app, LeftTab::Files);
            }
            if icon_button(ui, "🔍", "Search (Ctrl+Shift+F)", app.left_open && app.left_tab == LeftTab::Search).clicked() {
                toggle_left(app, LeftTab::Search);
            }
            if icon_button(ui, "🔖", "Bookmarks", app.left_open && app.left_tab == LeftTab::Bookmarks).clicked() {
                toggle_left(app, LeftTab::Bookmarks);
            }
            ui.add_space(8.0);
            if icon_button(ui, "📅", "Open today's daily note", false).clicked() {
                app.open_daily(ateliet_core::datefmt::today().date());
            }
            if icon_button(ui, "📝", "Templater: insert template (Alt+E)", false).clicked() {
                app.palette_ui = Some(crate::commands::Palette::templates(app, app.tabs.is_empty()));
            }
            if icon_button(ui, "⌘", "Command palette (Ctrl+P)", false).clicked() {
                app.palette_ui = Some(crate::commands::Palette::commands());
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                if icon_button(ui, "⚙", "Settings", app.settings_open).clicked() {
                    app.settings_open = !app.settings_open;
                }
                if icon_button(ui, "🗄", "Open another vault", false).clicked() {
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

fn header_tabs<T: PartialEq + Copy>(ui: &mut egui::Ui, current: &mut T, tabs: &[(T, &str, &str)]) {
    ui.horizontal(|ui| {
        for (t, icon, tip) in tabs {
            if icon_button(ui, icon, tip, *current == *t).clicked() {
                *current = *t;
            }
        }
    });
    ui.separator();
}

pub fn left(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::SidePanel::left("left").resizable(true).default_width(260.0).width_range(160.0..=520.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin::symmetric(8, 6))).show(ctx, |ui| {
        let mut tab = app.left_tab;
        header_tabs(ui, &mut tab, &[(LeftTab::Files, "🗀", "Files"), (LeftTab::Search, "🔍", "Search"), (LeftTab::Bookmarks, "🔖", "Bookmarks")]);
        app.left_tab = tab;
        match app.left_tab {
            LeftTab::Files => files(app, ui),
            LeftTab::Search => search_panel(app, ui),
            LeftTab::Bookmarks => bookmarks(app, ui),
        }
    });
}

fn files(app: &mut App, ui: &mut egui::Ui) {
    let Some(v) = &app.vault else { return };
    let name = v.name.clone();
    let tree = v.tree.clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&name).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("+ 🗀").on_hover_text("New folder").clicked() {
                app.ask = Some(Ask { title: "New folder".into(), value: String::new(), kind: AskKind::NewFolder(String::new()) });
            }
            if ui.small_button("+ 🗋").on_hover_text("New note (Ctrl+N)").clicked() {
                app.new_note("");
            }
        });
    });
    let active = app.active_path();
    let mut open: Option<(String, bool)> = None;
    let mut ask: Option<Ask> = None;
    let mut new_in: Option<String> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        folder_ui(ui, &tree, "", active.as_deref(), &mut open, &mut ask, &mut new_in, app.palette);
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

#[allow(clippy::too_many_arguments)]
fn folder_ui(ui: &mut egui::Ui, f: &Folder, path: &str, active: Option<&str>, open: &mut Option<(String, bool)>, ask: &mut Option<Ask>, new_in: &mut Option<String>, p: crate::theme::Palette) {
    for (name, sub) in &f.folders {
        let full = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
        let id = ui.make_persistent_id(("folder", &full));
        let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        let header = state.show_header(ui, |ui| {
            let r = ui.add(egui::Label::new(RichText::new(name).color(p.text)).sense(Sense::click()));
            r.context_menu(|ui| {
                if ui.button("New note").clicked() {
                    *new_in = Some(full.clone());
                    ui.close_menu();
                }
                if ui.button("New folder").clicked() {
                    *ask = Some(Ask { title: "New folder".into(), value: String::new(), kind: AskKind::NewFolder(full.clone()) });
                    ui.close_menu();
                }
                if ui.button("Rename").clicked() {
                    *ask = Some(Ask { title: "Rename folder".into(), value: name.clone(), kind: AskKind::Rename(full.clone()) });
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    *ask = Some(Ask { title: format!("Delete the folder “{name}” and everything in it?"), value: String::new(), kind: AskKind::ConfirmDelete(full.clone()) });
                    ui.close_menu();
                }
            });
            r
        });
        let (_, hr, _) = header.body(|ui| folder_ui(ui, sub, &full, active, open, ask, new_in, p));
        if hr.inner.clicked() {
            let mut s = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
            s.toggle(ui);
            s.store(ui.ctx());
        }
    }
    for (name, full) in &f.files {
        let label = if name.to_lowercase().ends_with(".md") { &name[..name.len() - 3] } else { name.as_str() };
        let is_active = active == Some(full.as_str());
        let is_md = name.to_lowercase().ends_with(".md");
        let text = RichText::new(label).color(if is_md { p.text } else { p.muted });
        let r = ui.add(egui::Button::new(text).frame(false).selected(is_active).wrap_mode(egui::TextWrapMode::Truncate));
        if r.clicked() {
            *open = Some((full.clone(), ui.input(|i| i.modifiers.command)));
        }
        if r.middle_clicked() {
            *open = Some((full.clone(), true));
        }
        r.context_menu(|ui| {
            if ui.button("Open in new tab").clicked() {
                *open = Some((full.clone(), true));
                ui.close_menu();
            }
            if ui.button("Rename").clicked() {
                *ask = Some(Ask { title: "Rename".into(), value: label.to_string(), kind: AskKind::Rename(full.clone()) });
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                *ask = Some(Ask { title: format!("Delete “{label}”?"), value: String::new(), kind: AskKind::ConfirmDelete(full.clone()) });
                ui.close_menu();
            }
        });
    }
}

fn search_panel(app: &mut App, ui: &mut egui::Ui) {
    let Some(v) = &app.vault else { return };
    let s = &mut app.search;
    let r = ui.add(egui::TextEdit::singleline(&mut s.query).hint_text("Search…  (tag:, path:, \"phrase\", -word)").desired_width(f32::INFINITY));
    if s.run_now {
        r.request_focus();
    }
    if (s.run_now || (r.changed() && s.query.len() >= 2) || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && s.query != s.last {
        s.run_now = false;
        s.last = s.query.clone();
        let (tx, rx) = mpsc::channel();
        let (root, q, c) = (v.root.clone(), s.query.clone(), ui.ctx().clone());
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let res = search::search(&root, &q, false, 200);
            let _ = tx.send(res);
            let _ = t;
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
    if s.pending.is_some() {
        ui.spinner();
    }
    if !s.query.is_empty() && s.pending.is_none() {
        let total: usize = s.results.iter().map(|r| r.total).sum();
        ui.label(RichText::new(format!("{} files · {} matches", s.results.len(), total)).small().color(app.palette.muted));
    }
    let mut open: Option<String> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for r in &s.results {
            let name = vault::basename(&r.path);
            if ui.add(egui::Button::new(RichText::new(name).strong()).frame(false).wrap_mode(egui::TextWrapMode::Truncate)).clicked() {
                open = Some(r.path.clone());
            }
            for m in r.matches.iter().take(4) {
                let text: String = m.text.chars().take(160).collect();
                if ui.add(egui::Label::new(RichText::new(text.trim()).small().color(app.palette.muted)).sense(Sense::click()).truncate()).clicked() {
                    open = Some(r.path.clone());
                }
            }
            ui.add_space(4.0);
        }
    });
    if let Some(p) = open {
        app.open(&p, false);
    }
}

fn bookmarks(app: &mut App, ui: &mut egui::Ui) {
    let Some(cfg) = &app.cfg else { return };
    let b = cfg.read("bookmarks.json");
    let mut open = None;
    fn walk(items: &[Value], ui: &mut egui::Ui, open: &mut Option<String>) {
        for it in items {
            match it["type"].as_str() {
                Some("file") => {
                    let path = it["path"].as_str().unwrap_or_default();
                    let title = it["title"].as_str().map(str::to_string).unwrap_or_else(|| vault::basename(path).to_string());
                    if ui.add(egui::Button::new(title).frame(false)).clicked() {
                        *open = Some(path.to_string());
                    }
                }
                Some("group") => {
                    ui.collapsing(it["title"].as_str().unwrap_or("Group"), |ui| walk(it["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), ui, open));
                }
                _ => {}
            }
        }
    }
    let items = b["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        ui.label(RichText::new("No bookmarks.").color(app.palette.muted));
    }
    walk(&items, ui, &mut open);
    if let Some(p) = open {
        app.open(&p, false);
    }
}

pub fn right(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    egui::SidePanel::right("right").resizable(true).default_width(290.0).width_range(200.0..=520.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin::symmetric(8, 6))).show(ctx, |ui| {
        let mut tab = app.right_tab;
        header_tabs(ui, &mut tab, &[(RightTab::Calendar, "📅", "Calendar"), (RightTab::Backlinks, "↙", "Backlinks"), (RightTab::Outline, "☰", "Outline"), (RightTab::Tags, "#", "Tags")]);
        app.right_tab = tab;
        match app.right_tab {
            RightTab::Backlinks => backlinks(app, ui),
            RightTab::Outline => outline(app, ui),
            RightTab::Tags => tags(app, ui),
            RightTab::Calendar => calendar_panel(app, ui),
        }
    });
}

fn backlinks(app: &mut App, ui: &mut egui::Ui) {
    let (Some(v), Some(path)) = (&app.vault, app.active_path()) else {
        ui.label(RichText::new("No file is open.").color(app.palette.muted));
        return;
    };
    let (inl, outl): (Vec<String>, Vec<String>) = {
        let ix = v.index.read().unwrap();
        (ix.inlinks(&path), ix.page(&path).map(|p| p.outlinks.iter().map(|l| l.path.clone()).collect()).unwrap_or_default())
    };
    let mut open = None;
    ui.label(RichText::new(format!("Linked mentions ({})", inl.len())).strong());
    for s in &inl {
        if ui.add(egui::Button::new(vault::basename(s)).frame(false)).clicked() {
            open = Some(s.clone());
        }
    }
    ui.add_space(10.0);
    ui.label(RichText::new(format!("Outgoing links ({})", outl.len())).strong());
    for s in &outl {
        let exists = v.abs(s).exists();
        let t = RichText::new(vault::basename(s)).color(if exists { app.palette.text } else { app.palette.faint });
        if ui.add(egui::Button::new(t).frame(false)).clicked() {
            open = Some(s.clone());
        }
    }
    if let Some(p) = open {
        app.open_link(&p, false);
    }
}

fn outline(app: &mut App, ui: &mut egui::Ui) {
    let Some(tab) = app.tabs.get_mut(app.active) else { return };
    let doc = tab.doc();
    if doc.headings.is_empty() {
        ui.label(RichText::new("No headings.").color(app.palette.muted));
    }
    let min = doc.headings.iter().map(|h| h.0).min().unwrap_or(1);
    for (level, text, line) in &doc.headings {
        ui.horizontal(|ui| {
            ui.add_space(((level - min) as f32) * 14.0);
            if ui.add(egui::Button::new(text).frame(false).wrap_mode(egui::TextWrapMode::Truncate)).clicked() {
                tab.scroll_to = Some(*line);
            }
        });
    }
}

fn tags(app: &mut App, ui: &mut egui::Ui) {
    let Some(v) = &app.vault else { return };
    let tags = v.index.read().unwrap().tag_counts();
    let mut clicked = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (t, n) in &tags {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(t).color(app.palette.accent)).frame(false)).clicked() {
                    clicked = Some(t.trim_start_matches('#').to_string());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(n.to_string()).small().color(app.palette.muted)));
            });
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
        ui.label(RichText::new(first_char_upper).size(18.0).strong());
        ui.label(RichText::new(data["year"].to_string()).size(18.0).color(p.accent));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("›").clicked() {
                if c.month == 12 {
                    c.month = 1;
                    c.year += 1;
                } else {
                    c.month += 1;
                }
            }
            if ui.small_button(if app.locale.starts_with("pt") { "Hoje" } else { "Today" }).clicked() {
                let t = ateliet_core::datefmt::today();
                c.year = t.year();
                c.month = t.month();
            }
            if ui.small_button("‹").clicked() {
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
            ui.vertical_centered(|ui| ui.label(RichText::new(w.as_str().unwrap_or_default().to_uppercase()).size(10.0).color(p.muted)));
        }
        ui.end_row();
        for week in data["weeks"].as_array().cloned().unwrap_or_default() {
            for d in week["days"].as_array().cloned().unwrap_or_default() {
                let day = d["day"].as_u64().unwrap_or(0);
                let in_month = d["inMonth"].as_bool().unwrap_or(false);
                let today = d["today"].as_bool().unwrap_or(false);
                let is_active = active.as_deref() == d["path"].as_str();
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, 34.0), Sense::click());
                let bg = if is_active { p.accent } else if resp.hovered() { p.bg_hover } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(rect, 4.0, bg);
                let color = if is_active { Color32::WHITE } else if today { p.accent } else if in_month { p.text } else { p.faint };
                let font = egui::FontId::new(13.0, if today { egui::FontFamily::Name("bold".into()) } else { egui::FontFamily::Proportional });
                ui.painter().text(rect.center_top() + egui::vec2(0.0, 6.0), egui::Align2::CENTER_TOP, day.to_string(), font, color);
                let dots = d["dots"].as_u64().unwrap_or(0) as usize;
                let tasks = d["openTasks"].as_u64().unwrap_or(0) > 0;
                let n = dots + tasks as usize;
                let dot_color = if is_active { Color32::WHITE } else { p.muted };
                for i in 0..n {
                    let x = rect.center().x + (i as f32 - (n as f32 - 1.0) / 2.0) * 6.0;
                    let c = egui::pos2(x, rect.bottom() - 7.0);
                    if i < dots {
                        ui.painter().circle_filled(c, 2.0, dot_color);
                    } else {
                        ui.painter().circle_stroke(c, 2.0, egui::Stroke::new(1.0, dot_color));
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
    egui::TopBottomPanel::top("tabs").frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin::symmetric(6, 4))).show(ctx, |ui| {
        let mut close = None;
        let mut select = None;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(if app.left_open { "◀" } else { "▶" }).frame(false)).on_hover_text("Toggle left sidebar").clicked() {
                    app.left_open = !app.left_open;
                }
                for (i, t) in app.tabs.iter().enumerate() {
                    let active = i == app.active;
                    let fill = if active { p.bg } else { Color32::TRANSPARENT };
                    egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                        let name = format!("{}{}", vault::basename(&t.path), if t.dirty() { " •" } else { "" });
                        let r = ui.add(egui::Label::new(RichText::new(name).color(if active { p.text } else { p.muted })).sense(Sense::click()).truncate());
                        if r.clicked() {
                            select = Some(i);
                        }
                        if r.middle_clicked() {
                            close = Some(i);
                        }
                        if ui.add(egui::Button::new(RichText::new("×").color(p.faint)).frame(false)).clicked() {
                            close = Some(i);
                        }
                    });
                }
                if ui.add(egui::Button::new("+").frame(false)).on_hover_text("New note").clicked() {
                    app.new_note("");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(if app.right_open { "▶" } else { "◀" }).frame(false)).on_hover_text("Toggle right sidebar").clicked() {
                        app.right_open = !app.right_open;
                    }
                    if let Some(t) = app.tabs.get_mut(app.active) {
                        let (icon, tip) = if t.mode == Mode::Edit { ("📖", "Reading view (Ctrl+E)") } else { ("✏", "Editing view (Ctrl+E)") };
                        if ui.add(egui::Button::new(icon).frame(false)).on_hover_text(tip).clicked() {
                            t.mode = if t.mode == Mode::Edit { Mode::Read } else { Mode::Edit };
                        }
                    }
                });
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
    egui::TopBottomPanel::bottom("status").frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin::symmetric(10, 3))).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if app.vault.as_ref().is_some_and(|v| !v.indexed.load(std::sync::atomic::Ordering::SeqCst)) {
                ui.spinner();
                ui.label(RichText::new("Indexing…").small().color(p.muted));
            }
            if app.templater.running > 0 {
                ui.label(RichText::new("Running template…").small().color(p.muted));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(t) = app.tabs.get(app.active) {
                    let words = ateliet_core::calendar::count_words(&t.text);
                    let chars = t.text.chars().count();
                    let back = app.vault.as_ref().map(|v| v.index.read().map(|ix| ix.inlinks(&t.path).len()).unwrap_or(0)).unwrap_or(0);
                    ui.label(RichText::new(format!("{back} backlinks")).small().color(p.muted));
                    ui.label(RichText::new(format!("{words} words  {chars} characters")).small().color(p.muted));
                }
            });
        });
    });
}

pub fn empty_state(app: &mut App, ui: &mut egui::Ui) {
    let p = app.palette;
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(RichText::new("No file is open").size(22.0).color(p.muted));
        ui.add_space(12.0);
        if ui.link("Create new note (Ctrl+N)").clicked() {
            app.new_note("");
        }
        if ui.link("Go to file (Ctrl+O)").clicked() {
            app.palette_ui = Some(crate::commands::Palette::files());
        }
        if ui.link("Open today's daily note").clicked() {
            app.open_daily(ateliet_core::datefmt::today().date());
        }
        ui.add_space(16.0);
        let recent: Vec<String> = app.recent.iter().take(8).cloned().collect();
        for r in recent {
            if ui.add(egui::Button::new(RichText::new(vault::basename(&r)).color(p.muted)).frame(false)).clicked() {
                app.open(&r, true);
            }
        }
    });
}

// ── dialogs ────────────────────────────────────────────────────────────────

pub fn ask_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(ask) = &mut app.ask else { return };
    let mut done: Option<bool> = None;
    egui::Window::new(ask.title.clone()).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 140.0]).show(ctx, |ui| {
        if !matches!(ask.kind, AskKind::ConfirmDelete(_)) {
            let r = ui.add(egui::TextEdit::singleline(&mut ask.value).desired_width(340.0));
            r.request_focus();
        }
        ui.horizontal(|ui| {
            let ok_label = if matches!(ask.kind, AskKind::ConfirmDelete(_)) { "Delete" } else { "OK" };
            if ui.button(ok_label).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
            AskKind::NewNote(folder) if !value.is_empty() => {
                if let Some(v) = &app.vault {
                    let path = if folder.is_empty() { format!("{value}.md") } else { format!("{folder}/{value}.md") };
                    let _ = v.write(&path, "");
                    app.vault.as_mut().unwrap().rescan();
                    app.open(&path, true);
                }
            }
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
    egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -36.0]).show(ctx, |ui| {
        for t in &app.toasts {
            egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(&t.text));
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
