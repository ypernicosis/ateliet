//! Commands: the palette (Ctrl+P), the quick switcher (Ctrl+O), template
//! pickers and keyboard shortcuts, including those of `hotkeys.json`.

use crate::icons;
use crate::kit;
use crate::app::{App, LeftTab, Mode};
use crate::config::VaultConfig;
use crate::vault;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    NewNote,
    OpenDaily,
    ToggleMode,
    Save,
    CloseTab,
    Search,
    Switcher,
    InsertTemplate,
    NewFromTemplate,
    ToggleLeft,
    ToggleRight,
    Settings,
    Rename,
    Delete,
    Outline,
    Backlinks,
    Calendar,
    ChangeVault,
    ImageMode,
    NewStoryboard,
    NewCanvas,
    Library,
    Manuscript,
    NewBook,
    NewChapter,
    NewCharacter,
    ContinueBook,
    ExportBook,
}

const COMMANDS: &[(Cmd, &str, &str)] = &[
    (Cmd::NewNote, "file-explorer:new-file", "Nova nota"),
    (Cmd::OpenDaily, "daily-notes", "Notas diárias: Abrir a nota de hoje"),
    (Cmd::ToggleMode, "markdown:toggle-preview", "Alternar modo leitura"),
    (Cmd::Save, "editor:save-file", "Salvar arquivo"),
    (Cmd::CloseTab, "workspace:close", "Fechar aba"),
    (Cmd::Search, "global-search:open", "Busca: Buscar em todos os arquivos"),
    (Cmd::Switcher, "switcher:open", "Navegação: Ir para arquivo"),
    (Cmd::InsertTemplate, "templater-obsidian:insert-templater", "Templater: Inserir modelo"),
    (Cmd::NewFromTemplate, "templater-obsidian:create-new-note-from-template", "Templater: Nova nota a partir de modelo"),
    (Cmd::ToggleLeft, "app:toggle-left-sidebar", "Alternar barra lateral esquerda"),
    (Cmd::ToggleRight, "app:toggle-right-sidebar", "Alternar painel direito"),
    (Cmd::Settings, "app:open-settings", "Abrir configurações"),
    (Cmd::Rename, "workspace:edit-file-title", "Renomear arquivo"),
    (Cmd::Delete, "app:delete-file", "Excluir arquivo"),
    (Cmd::Outline, "outline:open", "Estrutura: Mostrar títulos"),
    (Cmd::Backlinks, "backlink:open", "Backlinks: Mostrar backlinks"),
    (Cmd::Calendar, "calendar:show-calendar-view", "Calendário: Abrir"),
    (Cmd::ChangeVault, "app:open-vault", "Abrir outro cofre"),
    (Cmd::ImageMode, "images:toggle-mode", "Imagens: Alternar modo imagem"),
    (Cmd::NewStoryboard, "storyboard:new", "Storyboard: Novo storyboard"),
    (Cmd::NewCanvas, "storyboard:new-canvas", "Storyboard: Nova tela infinita"),
    (Cmd::Library, "scriptorium-omena:abrir-biblioteca", "Scriptorium: Abrir biblioteca"),
    (Cmd::Manuscript, "scriptorium-omena:abrir-manuscrito", "Scriptorium: Painel do manuscrito (capítulos)"),
    (Cmd::NewBook, "scriptorium-omena:novo-livro", "Scriptorium: Novo livro"),
    (Cmd::NewChapter, "scriptorium-omena:novo-capitulo", "Scriptorium: Novo capítulo"),
    (Cmd::NewCharacter, "scriptorium-omena:novo-personagem", "Scriptorium: Novo personagem"),
    (Cmd::ContinueBook, "scriptorium-omena:continuar", "Scriptorium: Continuar escrevendo"),
    (Cmd::ExportBook, "scriptorium-omena:exportar", "Scriptorium: Exportar livro…"),
];

fn default_shortcut(c: Cmd) -> Option<KeyboardShortcut> {
    let cmd = Modifiers::COMMAND;
    Some(match c {
        Cmd::NewNote => KeyboardShortcut::new(cmd, Key::N),
        Cmd::ToggleMode => KeyboardShortcut::new(cmd, Key::E),
        Cmd::Save => KeyboardShortcut::new(cmd, Key::S),
        Cmd::CloseTab => KeyboardShortcut::new(cmd, Key::W),
        Cmd::Search => KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::F),
        Cmd::Switcher => KeyboardShortcut::new(cmd, Key::O),
        Cmd::InsertTemplate => KeyboardShortcut::new(Modifiers::ALT, Key::E),
        Cmd::NewFromTemplate => KeyboardShortcut::new(Modifiers::ALT, Key::N),
        Cmd::Settings => KeyboardShortcut::new(cmd, Key::Comma),
        Cmd::ImageMode => KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::I),
        _ => return None,
    })
}

fn key_from_name(k: &str) -> Option<Key> {
    let k = k.trim();
    if k.chars().count() == 1 {
        let c = k.chars().next().unwrap().to_ascii_uppercase();
        return Key::from_name(&c.to_string()).or(match c {
            ',' => Some(Key::Comma),
            '.' => Some(Key::Period),
            '/' => Some(Key::Slash),
            ';' => Some(Key::Semicolon),
            '[' => Some(Key::OpenBracket),
            ']' => Some(Key::CloseBracket),
            '-' => Some(Key::Minus),
            '=' => Some(Key::Equals),
            '`' => Some(Key::Backtick),
            _ => None,
        });
    }
    Key::from_name(k).or(match k {
        "ArrowUp" => Some(Key::ArrowUp),
        "ArrowDown" => Some(Key::ArrowDown),
        "ArrowLeft" => Some(Key::ArrowLeft),
        "ArrowRight" => Some(Key::ArrowRight),
        _ => None,
    })
}

/// Custom shortcuts from `hotkeys.json` (Obsidian's format).
pub fn load_hotkeys(cfg: &VaultConfig) -> Vec<(String, KeyboardShortcut)> {
    let v = cfg.read("hotkeys.json");
    let mut out = Vec::new();
    if let Some(o) = v.as_object() {
        for (id, list) in o {
            for h in list.as_array().cloned().unwrap_or_default() {
                let Some(key) = h["key"].as_str().and_then(key_from_name) else { continue };
                let mut m = Modifiers::NONE;
                for md in h["modifiers"].as_array().cloned().unwrap_or_default() {
                    match md.as_str().unwrap_or_default() {
                        "Mod" => m |= Modifiers::COMMAND,
                        "Ctrl" => m |= Modifiers::CTRL,
                        "Meta" => m |= Modifiers::MAC_CMD,
                        "Alt" => m |= Modifiers::ALT,
                        "Shift" => m |= Modifiers::SHIFT,
                        _ => {}
                    }
                }
                out.push((id.clone(), KeyboardShortcut::new(m, key)));
            }
        }
    }
    out
}

pub fn run(app: &mut App, ctx: &egui::Context, c: Cmd) {
    match c {
        Cmd::NewNote => {
            let folder = app.active_path().map(|p| vault::parent(&p).to_string()).unwrap_or_default();
            app.new_note(&folder);
        }
        Cmd::NewStoryboard => app.new_board("animatic"),
        Cmd::Library => app.open_view("library", None),
        Cmd::Manuscript => {
            let b = app.current_book().map(|b| b.path);
            app.open_view("binder", b);
        }
        Cmd::NewBook => app.new_book_form(),
        Cmd::NewChapter | Cmd::NewCharacter | Cmd::ContinueBook | Cmd::ExportBook => match app.current_book() {
            Some(b) => app.sc_apply(match c {
                Cmd::NewChapter => crate::sc::Action::NewChapter(b.path),
                Cmd::NewCharacter => crate::sc::Action::NewCharacter(b.path),
                Cmd::ExportBook => crate::sc::Action::Export(b.path),
                _ => crate::sc::Action::Continue(b.path),
            }),
            None => app.toast("Abra um capítulo ou escolha um livro na Biblioteca."),
        },
        Cmd::NewCanvas => app.new_board("canvas"),
        Cmd::OpenDaily => app.open_daily(ateliet_core::datefmt::today().date()),
        Cmd::ToggleMode => {
            if let Some(t) = app.tabs.get_mut(app.active) {
                t.mode = if t.mode == Mode::Edit { Mode::Read } else { Mode::Edit };
            }
        }
        Cmd::Save => {
            let i = app.active;
            app.save_tab(i);
        }
        Cmd::CloseTab => {
            let i = app.active;
            app.close_tab(i);
        }
        Cmd::Search => {
            app.left_open = true;
            app.left_tab = LeftTab::Search;
            app.search.run_now = true;
        }
        Cmd::Switcher => app.palette_ui = Some(Palette::files()),
        Cmd::InsertTemplate => app.palette_ui = Some(Palette::templates(app, app.tabs.is_empty())),
        Cmd::NewFromTemplate => app.palette_ui = Some(Palette::templates(app, true)),
        Cmd::ToggleLeft => app.left_open = !app.left_open,
        Cmd::ToggleRight => app.right_open = !app.right_open,
        Cmd::Settings => app.settings_open = !app.settings_open,
        Cmd::Rename => {
            if let Some(p) = app.active_path() {
                app.ask = Some(crate::app::Ask { title: "Rename".into(), value: vault::basename(&p).to_string(), kind: crate::app::AskKind::Rename(p) });
            }
        }
        Cmd::Delete => {
            if let Some(p) = app.active_path() {
                app.ask = Some(crate::app::Ask { title: format!("Delete “{}”?", vault::basename(&p)), value: String::new(), kind: crate::app::AskKind::ConfirmDelete(p) });
            }
        }
        Cmd::Outline => {
            app.right_open = true;
            app.right_tab = crate::app::RightTab::Outline;
        }
        Cmd::Backlinks => {
            app.right_open = true;
            app.right_tab = crate::app::RightTab::Backlinks;
        }
        Cmd::Calendar => {
            app.right_open = true;
            app.right_tab = crate::app::RightTab::Calendar;
        }
        Cmd::ImageMode => app.images_mode = !app.images_mode,
        Cmd::ChangeVault => {
            app.save_all();
            app.vault = None;
            app.tabs.clear();
        }
    }
    let _ = ctx;
}

pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    if app.palette_ui.is_some() || app.ask.is_some() || app.templater.current.is_some() {
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            app.palette_ui = None;
        }
        return;
    }
    if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::P))) {
        app.palette_ui = Some(Palette::commands());
        return;
    }
    // hotkeys.json first (custom bindings win), then the defaults
    let custom = app.hotkeys.clone();
    for (id, sc) in custom {
        if !ctx.input_mut(|i| i.consume_shortcut(&sc)) {
            continue;
        }
        if let Some((c, _, _)) = COMMANDS.iter().find(|(_, cid, _)| *cid == id) {
            run(app, ctx, *c);
            return;
        }
        if let Some(path) = id.strip_prefix("templater-obsidian:") {
            if app.vault.as_ref().is_some_and(|v| v.abs(path).is_file()) {
                let new_note = app.tabs.is_empty();
                app.run_template(ctx, path, new_note);
                return;
            }
        }
    }
    for (c, _, _) in COMMANDS {
        if let Some(sc) = default_shortcut(*c) {
            if ctx.input_mut(|i| i.consume_shortcut(&sc)) {
                run(app, ctx, *c);
                return;
            }
        }
    }
}

// ── palette ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum Kind {
    Commands,
    Files,
    Templates { new_note: bool },
}

pub struct Palette {
    pub kind: Kind,
    pub query: String,
    pub selected: usize,
}

impl Palette {
    pub fn commands() -> Self {
        Palette { kind: Kind::Commands, query: String::new(), selected: 0 }
    }
    pub fn files() -> Self {
        Palette { kind: Kind::Files, query: String::new(), selected: 0 }
    }
    pub fn templates(_app: &App, new_note: bool) -> Self {
        Palette { kind: Kind::Templates { new_note }, query: String::new(), selected: 0 }
    }
}

/// Fuzzy match: every query character in order; earlier and contiguous hits score higher.
pub fn fuzzy(query: &str, text: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut ti = 0;
    let mut last: Option<usize> = None;
    for qc in query.to_lowercase().chars() {
        if qc == ' ' {
            continue;
        }
        let pos = (ti..t.len()).find(|&i| t[i] == qc)?;
        score += 10 - (pos as i64 - last.map(|l| l as i64 + 1).unwrap_or(0)).min(9);
        if pos == 0 || !t[pos - 1].is_alphanumeric() {
            score += 5;
        }
        last = Some(pos);
        ti = pos + 1;
    }
    Some(score - t.len() as i64 / 10)
}

enum Choice {
    Cmd(Cmd),
    File(String),
    Create(String),
    Template(String, bool),
}

pub fn show_palette(app: &mut App, ctx: &egui::Context) {
    let Some(pal) = &mut app.palette_ui else { return };
    let mut items: Vec<(i64, String, String, Choice)> = Vec::new();
    match &pal.kind {
        Kind::Commands => {
            for (c, _, name) in COMMANDS {
                if let Some(s) = fuzzy(&pal.query, name) {
                    let sc = default_shortcut(*c).map(|k| ctx.format_shortcut(&k)).unwrap_or_default();
                    items.push((s, name.to_string(), sc, Choice::Cmd(*c)));
                }
            }
            if pal.query.is_empty() {
                items.sort_by(|a, b| a.1.cmp(&b.1));
            }
        }
        Kind::Files => {
            if let Some(v) = &app.vault {
                for e in v.markdown_files() {
                    let name = vault::basename(&e.path);
                    if let Some(s) = fuzzy(&pal.query, name).map(|s| s + 20).or_else(|| fuzzy(&pal.query, &e.path)) {
                        items.push((s, name.to_string(), vault::parent(&e.path).to_string(), Choice::File(e.path.clone())));
                    }
                }
                if pal.query.is_empty() {
                    items.clear();
                    for r in &app.recent {
                        items.push((0, vault::basename(r).to_string(), vault::parent(r).to_string(), Choice::File(r.clone())));
                    }
                } else if !pal.query.trim().is_empty() {
                    items.push((i64::MIN + 1, format!("Criar “{}”", pal.query.trim()), String::new(), Choice::Create(pal.query.trim().to_string())));
                }
            }
        }
        Kind::Templates { new_note } => {
            if let Some(v) = &app.vault {
                for t in app.templater.templates(v, &app.templater_settings) {
                    if let Some(s) = fuzzy(&pal.query, &t) {
                        items.push((s, vault::basename(&t).to_string(), vault::parent(&t).to_string(), Choice::Template(t.clone(), *new_note)));
                    }
                }
            }
        }
    }
    if !pal.query.is_empty() {
        items.sort_by_key(|x| std::cmp::Reverse(x.0));
    }
    items.truncate(60);
    let n = items.len();
    if ctx.input(|i| i.key_pressed(Key::ArrowDown)) && n > 0 {
        pal.selected = (pal.selected + 1) % n;
    }
    if ctx.input(|i| i.key_pressed(Key::ArrowUp)) && n > 0 {
        pal.selected = (pal.selected + n - 1) % n;
    }
    pal.selected = pal.selected.min(n.saturating_sub(1));
    let mut chosen: Option<usize> = None;
    let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
    let placeholder = match pal.kind {
        Kind::Commands => "Digite um comando",
        Kind::Files => "Encontre ou crie uma nota",
        Kind::Templates { .. } => "Escolha um modelo",
    };
    let p = app.palette;
    kit::scrim(ctx, p);
    egui::Window::new("palette")
        .title_bar(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
        .frame(egui::Frame::window(&ctx.style()).inner_margin(egui::Margin::ZERO).corner_radius(12.0))
        .show(ctx, |ui| {
            ui.set_width(600.0);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 18, top: 14, bottom: 14 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(kit::icon(icons::SEARCH, 16.0).color(p.faint));
                    ui.add_space(10.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut pal.query).hint_text(RichText::new(placeholder).color(p.faint)).frame(false).desired_width(f32::INFINITY).font(egui::FontId::proportional(16.0)));
                    r.request_focus();
                    if r.changed() {
                        pal.selected = 0;
                    }
                });
            });
            kit::hairline(ui, p);
            egui::Frame::new().inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, (_, title, note, choice)) in items.iter().enumerate() {
                        let sel = i == pal.selected;
                        let w = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 36.0), egui::Sense::click());
                        if sel || resp.hovered() {
                            ui.painter().rect_filled(rect, 7.0, if sel { p.bg_active } else { p.bg_hover });
                        }
                        let glyph = match choice {
                            Choice::Cmd(_) => icons::CHEVRON_RIGHT,
                            Choice::File(_) => icons::FILE_TEXT,
                            Choice::Create(_) => icons::PLUS,
                            Choice::Template(..) => icons::LAYOUT_TEMPLATE,
                        };
                        let cy = rect.center().y;
                        ui.painter().text(egui::pos2(rect.left() + 20.0, cy), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(14.0), if sel { p.text } else { p.faint });
                        // "Group: name" → the group quieter than the name
                        let mut job = egui::text::LayoutJob::default();
                        let body = egui::FontId::proportional(13.5);
                        match (matches!(choice, Choice::Cmd(_)), title.split_once(": ")) {
                            (true, Some((group, name))) => {
                                job.append(&format!("{group}  "), 0.0, egui::TextFormat { font_id: body.clone(), color: p.faint, ..Default::default() });
                                job.append(name, 0.0, egui::TextFormat { font_id: body.clone(), color: p.text, ..Default::default() });
                            }
                            _ => job.append(title, 0.0, egui::TextFormat { font_id: body.clone(), color: p.text, ..Default::default() }),
                        }
                        let mut right = rect.right() - 12.0;
                        if !note.is_empty() {
                            if matches!(choice, Choice::Cmd(_)) {
                                // shortcut as key caps
                                for key in note.split('+').collect::<Vec<_>>().into_iter().rev() {
                                    let g = ui.painter().layout_no_wrap(key.to_string(), egui::FontId::proportional(11.0), p.muted);
                                    let cap = egui::Rect::from_min_size(egui::pos2(right - g.size().x - 12.0, cy - 10.0), egui::vec2(g.size().x + 12.0, 20.0));
                                    ui.painter().rect(cap, 5.0, p.bg_secondary, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                                    ui.painter().galley(egui::pos2(cap.left() + 6.0, cy - g.size().y / 2.0), g, p.muted);
                                    right = cap.left() - 4.0;
                                }
                            } else {
                                let g = ui.painter().layout_no_wrap(note.clone(), egui::FontId::proportional(11.5), p.faint);
                                let max = (rect.width() * 0.4).min(g.size().x);
                                let mut nj = egui::text::LayoutJob::single_section(note.clone(), egui::TextFormat { font_id: egui::FontId::proportional(11.5), color: p.faint, ..Default::default() });
                                nj.wrap = egui::text::TextWrapping::truncate_at_width(max);
                                let g = ui.fonts(|f| f.layout_job(nj));
                                ui.painter().galley(egui::pos2(right - g.size().x, cy - g.size().y / 2.0), g, p.faint);
                                right -= max + 12.0;
                            }
                        }
                        job.wrap = egui::text::TextWrapping::truncate_at_width((right - rect.left() - 44.0).max(40.0));
                        let g = ui.fonts(|f| f.layout_job(job));
                        ui.painter().galley(egui::pos2(rect.left() + 40.0, cy - g.size().y / 2.0), g, p.text);
                        if sel {
                            resp.scroll_to_me(None);
                        }
                        if resp.clicked() {
                            chosen = Some(i);
                        }
                    }
                    if items.is_empty() {
                        ui.add_space(14.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new("Nada encontrado").color(p.faint)));
                        ui.add_space(14.0);
                    }
                });
            });
            kit::hairline(ui, p);
            egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 18, top: 8, bottom: 9 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    for t in ["↑↓  navegar", "↵  abrir", "esc  fechar"] {
                        ui.label(RichText::new(t).size(11.0).color(p.faint));
                    }
                });
            });
            if ui.input(|i| i.key_pressed(Key::Enter)) && n > 0 {
                chosen = Some(pal.selected);
            }
        });
    if let Some(i) = chosen {
        // the Enter that chose must not also confirm whatever opens next
        ctx.input_mut(|inp| {
            inp.consume_key(Modifiers::NONE, Key::Enter);
            inp.consume_key(Modifiers::COMMAND, Key::Enter);
        });
        let choice = items.swap_remove(i).3;
        app.palette_ui = None;
        let new_tab = ctx.input(|i| i.modifiers.command);
        match choice {
            Choice::Cmd(c) => run(app, ctx, c),
            Choice::File(f) => app.open(&f, new_tab),
            Choice::Create(name) => app.open_link(&name, new_tab),
            Choice::Template(t, new_note) => app.run_template(ctx, &t, new_note),
        }
        close = false;
    }
    if close {
        app.palette_ui = None;
    }
}
