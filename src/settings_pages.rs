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

/// Copying the shared library into one of Ateliet's own, in the background.
pub struct CopyJob {
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub state: std::sync::Arc<std::sync::Mutex<(ateliet_core::pictogrep::CopyProgress, Option<Result<std::path::PathBuf, String>>)>>,
}

/// "Copiar biblioteca…": what the shared library holds, where the copy goes,
/// the progress; when done, the copy becomes the library in use.
pub fn copy_library_ui(app: &mut App, ui: &mut egui::Ui, root: &std::path::Path, p: Palette) -> bool {
    use std::sync::atomic::Ordering;
    let mut rescan = false;
    if let Some(job) = &app.copy_job {
        let (prog, done) = job.state.lock().map(|s| (s.0.clone(), s.1.clone())).unwrap_or_default();
        match done {
            Some(Ok(dest)) => {
                crate::images_ui::ImagesState::save_settings(root, json!({ "shared": dest.to_string_lossy() }));
                app.copy_job = None;
                rescan = true;
                app.toast("Biblioteca copiada; o Ateliet passa a usar a cópia.");
            }
            Some(Err(e)) => {
                app.copy_job = None;
                app.toast(format!("A cópia não terminou: {e}"));
            }
            None => {
                let frac = if prog.total_bytes > 0 { prog.bytes as f32 / prog.total_bytes as f32 } else { 0.0 };
                ui.horizontal(|ui| {
                    ui.add(egui::ProgressBar::new(frac).desired_width(300.0).text(format!("{} de {} arquivos", prog.files, prog.total_files)));
                    if kit::button(ui, None, "Cancelar", p).clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                });
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        return rescan;
    }
    let st = crate::images_ui::ImagesState::settings(root);
    let home = match st["shared"].as_str().unwrap_or("auto").trim() {
        "" => None,
        "auto" => app.images.shared.clone(),
        path => Some(ateliet_core::pictogrep::Home::new(std::path::PathBuf::from(path))).filter(|h| h.root.is_dir()),
    };
    let Some(home) = home else { return false };
    if ateliet_core::pictogrep::copied_from(&home).is_some() {
        ui.label(RichText::new("A biblioteca em uso já é uma cópia só do Ateliet.").size(12.0).color(p.faint));
        return false;
    }
    let plan_id = egui::Id::new("copy-plan");
    let plan: Option<Result<ateliet_core::pictogrep::CopyPlan, String>> = ui.data(|d| d.get_temp(plan_id));
    if kit::button(ui, Some(crate::icons::COPY), "Copiar biblioteca…", p).on_hover_text("Uma cópia da pasta do Pictogrep só para o Ateliet: imagens, coleções, análises, quadros do Pinterest e storyboards").clicked() {
        ui.data_mut(|d| d.insert_temp(plan_id, ateliet_core::pictogrep::copy_plan(&home)));
    }
    match plan {
        Some(Ok(pl)) => {
            let dest = ateliet_core::pictogrep::own_library_dir();
            ui.label(RichText::new(format!("{} imagens, {} coleções, {} storyboards, {} arquivos ({:.1} MB) para {}", pl.pictures, pl.collections, pl.storyboards, pl.files, pl.bytes as f64 / 1e6, dest.display())).size(12.0).color(p.muted));
            ui.horizontal(|ui| {
                if kit::primary(ui, "Copiar", p).clicked() {
                    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let state = std::sync::Arc::new(std::sync::Mutex::new((ateliet_core::pictogrep::CopyProgress::default(), None)));
                    let (c2, s2, home2, dest2) = (cancel.clone(), state.clone(), home.clone(), dest.clone());
                    std::thread::spawn(move || {
                        let r = ateliet_core::pictogrep::copy_home(&home2, &dest2, &c2, |pr| {
                            if let Ok(mut s) = s2.lock() {
                                s.0 = pr.clone();
                            }
                        });
                        if let Ok(mut s) = s2.lock() {
                            s.1 = Some(r.map(|h| h.root));
                        }
                    });
                    app.copy_job = Some(CopyJob { cancel, state });
                    ui.data_mut(|d| d.remove::<Result<ateliet_core::pictogrep::CopyPlan, String>>(plan_id));
                }
                if kit::button(ui, None, "Agora não", p).clicked() {
                    ui.data_mut(|d| d.remove::<Result<ateliet_core::pictogrep::CopyPlan, String>>(plan_id));
                }
            });
        }
        Some(Err(e)) => {
            ui.label(RichText::new(e).size(12.0).color(p.faint));
        }
        None => {}
    }
    rescan
}
