//! Templater runs: the template executes in the core's script engine on a
//! worker thread; prompts and suggesters come back here as dialogs.

use crate::config::VaultConfig;
use crate::vault::Vault;
use ateliet_core::dataview::Settings;
use ateliet_core::script::{self, TemplateOutput, TemplateRun};
use eframe::egui;
use serde_json::{json, Value};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct TemplaterSettings {
    pub folder: String,
    pub hotkey_templates: Vec<String>,
    /// Run the template of the deepest matching folder on new notes.
    pub folder_templates: Vec<(String, String)>,
    pub trigger_on_create: bool,
}

impl TemplaterSettings {
    pub fn load(cfg: &VaultConfig) -> Self {
        let v = cfg.tool("templater", "templater-obsidian");
        TemplaterSettings {
            folder: v["templates_folder"].as_str().unwrap_or_default().to_string(),
            hotkey_templates: v["enabled_templates_hotkeys"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
            folder_templates: v["folder_templates"].as_array().map(|a| a.iter().filter_map(|x| Some((x["folder"].as_str()?.to_string(), x["template"].as_str()?.to_string()))).filter(|(f, t)| !f.is_empty() && !t.is_empty()).collect()).unwrap_or_default(),
            trigger_on_create: v["trigger_on_file_creation"].as_bool().unwrap_or(false),
        }
    }

    pub fn save(&self, cfg: &VaultConfig) {
        let mut v = cfg.tool("templater", "templater-obsidian");
        if !v.is_object() {
            v = json!({});
        }
        v["templates_folder"] = json!(self.folder);
        v["enabled_templates_hotkeys"] = json!(self.hotkey_templates);
        v["folder_templates"] = json!(self.folder_templates.iter().map(|(f, t)| json!({ "folder": f, "template": t })).collect::<Vec<_>>());
        v["trigger_on_file_creation"] = json!(self.trigger_on_create);
        cfg.merge("ateliet/templater.json", v);
    }
}

/// Where the output goes.
#[derive(Clone, Debug)]
pub enum Mode {
    /// Insert at the cursor of an open note (char index).
    Insert { path: String, at: usize },
    /// Fill a new note.
    NewNote { path: String },
    /// The note's own `<% %>` commands replaced by their results.
    Replace { path: String },
}

pub struct Prompt {
    pub request: Value,
    pub reply: Sender<Option<Value>>,
    pub input: String,
    pub picked: Vec<bool>,
}

pub struct Finished {
    pub mode: Mode,
    pub result: Result<TemplateOutput, String>,
}

pub struct Templater {
    prompts_rx: Receiver<(Value, Sender<Option<Value>>)>,
    prompts_tx: Sender<(Value, Sender<Option<Value>>)>,
    done_rx: Receiver<Finished>,
    done_tx: Sender<Finished>,
    pub current: Option<Prompt>,
    pub running: usize,
}

impl Default for Templater {
    fn default() -> Self {
        let (prompts_tx, prompts_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        Templater { prompts_rx, prompts_tx, done_rx, done_tx, current: None, running: 0 }
    }
}

impl Templater {
    /// The folder template for a new note at `path` (the deepest folder that matches).
    pub fn folder_template(s: &TemplaterSettings, path: &str) -> Option<String> {
        if !s.trigger_on_create {
            return None;
        }
        s.folder_templates
            .iter()
            .filter(|(f, _)| {
                let f = f.trim_matches('/');
                f.is_empty() || f == "/" || path.starts_with(&format!("{f}/"))
            })
            .max_by_key(|(f, _)| f.trim_matches('/').len())
            .map(|(_, t)| if t.to_lowercase().ends_with(".md") { t.clone() } else { format!("{t}.md") })
    }

    pub fn templates(&self, vault: &Vault, s: &TemplaterSettings) -> Vec<String> {
        let folder = s.folder.trim().trim_matches('/');
        let mut v: Vec<String> = vault.markdown_files().map(|e| e.path.clone()).filter(|p| folder.is_empty() || p.starts_with(&format!("{folder}/"))).collect();
        v.sort();
        v
    }

    /// Start a run; `content` is the target's current text.
    #[allow(clippy::too_many_arguments)]
    pub fn run(&mut self, ctx: &egui::Context, vault: &Vault, settings: Settings, template: &str, target: &str, content: String, selection: String, mode: Mode) {
        let template_text = match std::fs::read_to_string(vault.abs(template)) {
            Ok(t) => t,
            Err(e) => {
                let _ = self.done_tx.send(Finished { mode, result: Err(format!("cannot read {template}: {e}")) });
                return;
            }
        };
        let tx = self.prompts_tx.clone();
        let c = ctx.clone();
        let prompt: script::PromptFn = Arc::new(move |req: Value| {
            let (reply_tx, reply_rx) = mpsc::channel();
            if tx.send((req, reply_tx)).is_err() {
                return None;
            }
            c.request_repaint();
            reply_rx.recv().ok().flatten()
        });
        let host = script::Host { index: vault.index.clone(), settings, root: vault.root.clone(), prompt: Some(prompt), log: None, timeout_secs: 60 };
        let run = TemplateRun {
            template_path: template.to_string(),
            template_text,
            target_path: target.to_string(),
            target_content: content,
            selection,
            run_mode: if matches!(mode, Mode::NewNote { .. }) { 0 } else { 1 },
            week_start: 0,
        };
        let done = self.done_tx.clone();
        let c = ctx.clone();
        self.running += 1;
        std::thread::spawn(move || {
            let result = script::run_template(&host, &run);
            let _ = done.send(Finished { mode, result });
            c.request_repaint();
        });
    }

    /// Finished runs to apply.
    pub fn poll(&mut self) -> Vec<Finished> {
        let mut out = Vec::new();
        while let Ok(f) = self.done_rx.try_recv() {
            self.running = self.running.saturating_sub(1);
            out.push(f);
        }
        if self.current.is_none() {
            if let Ok((request, reply)) = self.prompts_rx.try_recv() {
                let input = request["value"].as_str().unwrap_or_default().to_string();
                let n = request["labels"].as_array().map(|a| a.len()).unwrap_or(0);
                self.current = Some(Prompt { request, reply, input, picked: vec![false; n] });
            }
        }
        out
    }

    /// The dialog of a waiting prompt.
    pub fn show_prompt(&mut self, ctx: &egui::Context, pal: crate::theme::Palette) {
        let Some(p) = &mut self.current else { return };
        let kind = p.request["kind"].as_str().unwrap_or_default().to_string();
        if kind == "clipboard" {
            let text = ctx.input(|_| String::new());
            let _ = p.reply.send(Some(json!(text)));
            self.current = None;
            return;
        }
        let mut answer: Option<Option<Value>> = None;
        let title = p.request["text"].as_str().or(p.request["placeholder"].as_str()).filter(|s| !s.is_empty()).unwrap_or("Templater").to_string();
        crate::kit::scrim(ctx, pal);
        let sel_id = egui::Id::new("tp-suggest-sel");
        egui::Window::new("templater-prompt").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 110.0]).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(egui::RichText::new(&title).font(crate::kit::medium(15.0)).color(pal.text));
            ui.add_space(12.0);
            match kind.as_str() {
                "prompt" => {
                    let multiline = p.request["multiline"].as_bool().unwrap_or(false);
                    let r = if multiline {
                        ui.add(egui::TextEdit::multiline(&mut p.input).desired_width(f32::INFINITY).desired_rows(4).margin(egui::Margin::symmetric(10, 7)))
                    } else {
                        ui.add(egui::TextEdit::singleline(&mut p.input).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 7)))
                    };
                    r.request_focus();
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && (!multiline || i.modifiers.command));
                    ui.add_space(14.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::kit::primary(ui, "Confirmar", pal).clicked() || enter {
                            answer = Some(Some(json!(p.input.clone())));
                        }
                        if crate::kit::button(ui, None, "Cancelar", pal).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            answer = Some(None);
                        }
                    });
                }
                "suggester" => {
                    let labels: Vec<String> = p.request["labels"].as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default();
                    let r = crate::kit::search_field(ui, &mut p.input, "Filtrar", pal);
                    r.request_focus();
                    let needle = p.input.to_lowercase();
                    let shown: Vec<usize> = (0..labels.len()).filter(|&i| labels[i].to_lowercase().contains(&needle)).collect();
                    let mut sel: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(0);
                    if r.changed() {
                        sel = 0;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !shown.is_empty() {
                        sel = (sel + 1) % shown.len();
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !shown.is_empty() {
                        sel = (sel + shown.len() - 1) % shown.len();
                    }
                    sel = sel.min(shown.len().saturating_sub(1));
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (k, &i) in shown.iter().enumerate() {
                            let row = crate::kit::Row::new(&labels[i]).selected(k == sel).show(ui, pal);
                            if k == sel {
                                row.scroll_to_me(None);
                            }
                            if row.clicked() {
                                answer = Some(Some(json!(i)));
                            }
                        }
                        if shown.is_empty() {
                            ui.label(egui::RichText::new("Nada encontrado").color(pal.faint));
                        }
                    });
                    ui.data_mut(|d| d.insert_temp(sel_id, sel));
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(&i) = shown.get(sel) {
                            answer = Some(Some(json!(i)));
                        }
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        answer = Some(None);
                    }
                }
                "multi_suggester" => {
                    let labels: Vec<String> = p.request["labels"].as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default();
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (i, l) in labels.iter().enumerate() {
                            if let Some(b) = p.picked.get_mut(i) {
                                ui.checkbox(b, l);
                            }
                        }
                    });
                    ui.add_space(14.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::kit::primary(ui, "Confirmar", pal).clicked() {
                            let picked: Vec<usize> = p.picked.iter().enumerate().filter(|(_, b)| **b).map(|(i, _)| i).collect();
                            answer = Some(Some(json!(picked)));
                        }
                        if crate::kit::button(ui, None, "Cancelar", pal).clicked() {
                            answer = Some(None);
                        }
                    });
                }
                _ => answer = Some(None),
            }
        });
        if answer.is_some() {
            ctx.data_mut(|d| d.remove::<usize>(sel_id));
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        }
        if let Some(a) = answer {
            let _ = p.reply.send(a);
            self.current = None;
        }
    }
}
