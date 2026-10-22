//! Two pages of the settings: "Arquivos e links" (Obsidian's app.json keys
//! for new notes, attachments, links and deleted files) and "Atalhos" (the
//! commands' shortcuts, written to hotkeys.json as Obsidian does).

use crate::app::App;
use crate::commands::{self, COMMANDS};
use crate::config::VaultConfig;
use crate::kit;
use crate::theme::Palette;
use eframe::egui::{self, KeyboardShortcut, Modifiers, RichText};
use serde_json::{json, Value};

fn choice(ui: &mut egui::Ui, id: &str, current: &str, options: &[(&str, &str)]) -> Option<String> {
    let mut out = None;
    let shown = options.iter().find(|o| o.0 == current).map(|o| o.1).unwrap_or(options[0].1);
    egui::ComboBox::from_id_salt(id).width(220.0).selected_text(shown).show_ui(ui, |ui| {
        for (k, l) in options {
            if ui.selectable_label(current == *k, *l).clicked() {
                out = Some(k.to_string());
            }
        }
    });
    out
}

pub fn files_page(ui: &mut egui::Ui, cfg: &VaultConfig, p: Palette) {
    let a = cfg.read("app.json");
    let s = |k: &str, d: &str| a[k].as_str().unwrap_or(d).to_string();
    let field = |ui: &mut egui::Ui, v: &mut String| ui.add(egui::TextEdit::singleline(v).desired_width(220.0).margin(egui::Margin::symmetric(8, 5))).changed();
    let loc = s("newFileLocation", "current");
    if let Some(v) = kit::setting(ui, "Onde ficam as notas novas", "", p, |ui| choice(ui, "newFileLocation", &loc, &[("current", "Na pasta da nota aberta"), ("root", "Na raiz do cofre"), ("folder", "Numa pasta escolhida")])) {
        cfg.merge("app.json", json!({ "newFileLocation": v }));
    }
    if loc == "folder" {
        let mut f = s("newFileFolderPath", "");
        if kit::setting(ui, "Pasta das notas novas", "", p, |ui| field(ui, &mut f)) {
            cfg.merge("app.json", json!({ "newFileFolderPath": f }));
        }
    }
    let mut att = s("attachmentFolderPath", "/");
    if kit::setting(ui, "Pasta de anexos", "Onde vão as imagens e arquivos colados ou trazidos para as notas. “/” é a raiz; “./” é a pasta da nota.", p, |ui| field(ui, &mut att)) {
        cfg.merge("app.json", json!({ "attachmentFolderPath": att }));
    }
    let fmt = s("newLinkFormat", "shortest");
    if let Some(v) = kit::setting(ui, "Formato dos links novos", "", p, |ui| choice(ui, "newLinkFormat", &fmt, &[("shortest", "O mais curto possível"), ("relative", "Relativo à nota"), ("absolute", "Caminho completo no cofre")])) {
        cfg.merge("app.json", json!({ "newLinkFormat": v }));
    }
    let mut md = a["useMarkdownLinks"].as_bool().unwrap_or(false);
    if kit::setting(ui, "Links em Markdown", "[texto](caminho.md) em vez de [[nota]].", p, |ui| kit::toggle(ui, &mut md, p).changed()) {
        cfg.merge("app.json", json!({ "useMarkdownLinks": md }));
    }
    let mut upd = a["alwaysUpdateLinks"].as_bool().unwrap_or(true);
    if kit::setting(ui, "Atualizar links ao renomear", "Renomear ou mover um arquivo corrige os links para ele nas outras notas.", p, |ui| kit::toggle(ui, &mut upd, p).changed()) {
        cfg.merge("app.json", json!({ "alwaysUpdateLinks": upd }));
    }
    let trash = s("trashOption", "system");
    if let Some(v) = kit::setting(ui, "Arquivos excluídos", "", p, |ui| choice(ui, "trashOption", &trash, &[("system", "Para a lixeira do sistema"), ("local", "Para a pasta .trash do cofre")])) {
        cfg.merge("app.json", json!({ "trashOption": v }));
    }
}

fn key_name(k: egui::Key) -> String {
    let n = k.name();
    if n.chars().count() == 1 { n.to_uppercase() } else { n.to_string() }
}

fn entry(sc: &KeyboardShortcut) -> Value {
    let mut m = Vec::new();
    if sc.modifiers.command || sc.modifiers.ctrl {
        m.push("Mod");
    }
    if sc.modifiers.alt {
        m.push("Alt");
    }
    if sc.modifiers.shift {
        m.push("Shift");
    }
    json!({ "modifiers": m, "key": key_name(sc.logical_key) })
}

pub fn hotkeys_page(app: &mut App, ui: &mut egui::Ui, cfg: &VaultConfig, p: Palette) {
    let ctx = ui.ctx().clone();
    let cap_id = egui::Id::new("hotkey-capture");
    let mut capturing: Option<String> = ui.data(|d| d.get_temp(cap_id));
    let filter_id = egui::Id::new("hotkey-filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Filtrar comandos").desired_width(320.0).margin(egui::Margin::symmetric(8, 5)));
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    ui.add_space(8.0);
    // the key pressed while capturing
    if let Some(id) = capturing.clone() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } if !matches!(key, egui::Key::Escape) => Some(KeyboardShortcut::new(*modifiers, *key)),
                _ => None,
            })
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            capturing = None;
        } else if let Some(sc) = pressed {
            let sc = KeyboardShortcut::new(Modifiers { mac_cmd: false, command: sc.modifiers.command || sc.modifiers.ctrl, ctrl: false, ..sc.modifiers }, sc.logical_key);
            cfg.merge("hotkeys.json", json!({ id: [entry(&sc)] }));
            app.hotkeys = commands::load_hotkeys(cfg);
            capturing = None;
        }
    }
    let mut set = None;
    let mut reset = None;
    let q = filter.to_lowercase();
    // the commands, then the templates on the palette
    let mut rows: Vec<(String, String, Option<KeyboardShortcut>)> = COMMANDS.iter().map(|(c, id, name)| (id.to_string(), name.to_string(), commands::default_shortcut(*c))).collect();
    for t in &app.templater_settings.hotkey_templates {
        let path = if t.to_lowercase().ends_with(".md") { t.clone() } else { format!("{t}.md") };
        rows.push((format!("templater-obsidian:{path}"), format!("Templater: Inserir {}", path.trim_end_matches(".md")), None));
    }
    for (id, name, default) in rows {
        if !q.is_empty() && !name.to_lowercase().contains(&q) {
            continue;
        }
        let custom: Vec<&KeyboardShortcut> = app.hotkeys.iter().filter(|(h, _)| *h == id).map(|(_, k)| k).collect();
        let shown = if capturing.as_deref() == Some(id.as_str()) {
            "Pressione as teclas…".to_string()
        } else if !custom.is_empty() {
            custom.iter().map(|k| ctx.format_shortcut(k)).collect::<Vec<_>>().join(", ")
        } else {
            default.map(|k| ctx.format_shortcut(&k)).unwrap_or_else(|| "—".into())
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).size(13.0).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !custom.is_empty() && kit::icon_button_sized(ui, crate::icons::ROTATE_CCW, "Voltar ao padrão", false, p, 22.0, 12.0).clicked() {
                    reset = Some(id.clone());
                }
                if kit::icon_button_sized(ui, crate::icons::PLUS, "Definir atalho", false, p, 22.0, 12.0).clicked() {
                    set = Some(id.clone());
                }
                ui.label(RichText::new(shown).size(12.5).color(if custom.is_empty() { p.faint } else { p.accent_strong }));
            });
        });
        ui.add_space(2.0);
    }
    if let Some(id) = set {
        capturing = Some(id);
    }
    if let Some(id) = reset {
        cfg.remove("hotkeys.json", &id);
        app.hotkeys = commands::load_hotkeys(cfg);
    }
    ui.data_mut(|d| match capturing {
        Some(c) => d.insert_temp(cap_id, c),
        None => d.remove::<String>(cap_id),
    });
}
