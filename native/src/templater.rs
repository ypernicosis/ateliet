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
}

impl TemplaterSettings {
    pub fn load(cfg: &VaultConfig) -> Self {
        let v = cfg.tool("templater", "templater-obsidian");
        TemplaterSettings {
            folder: v["templates_folder"].as_str().unwrap_or_default().to_string(),
            hotkey_templates: v["enabled_templates_hotkeys"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
        }
    }

    pub fn save(&self, cfg: &VaultConfig) {
        let mut v = cfg.tool("templater", "templater-obsidian");
        if !v.is_object() {
            v = json!({});
        }
        v["templates_folder"] = json!(self.folder);
        v["enabled_templates_hotkeys"] = json!(self.hotkey_templates);
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
    pub fn show_prompt(&mut self, ctx: &egui::Context) {
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
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 120.0]).default_width(420.0).show(ctx, |ui| match kind.as_str() {
            "prompt" => {
                let multiline = p.request["multiline"].as_bool().unwrap_or(false);
                let r = if multiline { ui.add(egui::TextEdit::multiline(&mut p.input).desired_width(400.0)) } else { ui.add(egui::TextEdit::singleline(&mut p.input).desired_width(400.0)) };
                r.request_focus();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && (!multiline || i.modifiers.command));
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || enter {
                        answer = Some(Some(json!(p.input.clone())));
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        answer = Some(None);
                    }
                });
            }
            "suggester" => {
                let labels: Vec<String> = p.request["labels"].as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default();
                let r = ui.add(egui::TextEdit::singleline(&mut p.input).hint_text("Filter…").desired_width(400.0));
                r.request_focus();
                let needle = p.input.to_lowercase();
                let shown: Vec<usize> = (0..labels.len()).filter(|&i| labels[i].to_lowercase().contains(&needle)).collect();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for &i in &shown {
                        if ui.selectable_label(false, &labels[i]).clicked() {
                            answer = Some(Some(json!(i)));
                        }
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Some(&i) = shown.first() {
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
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        let picked: Vec<usize> = p.picked.iter().enumerate().filter(|(_, b)| **b).map(|(i, _)| i).collect();
                        answer = Some(Some(json!(picked)));
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(None);
                    }
                });
            }
            _ => answer = Some(None),
        });
        if let Some(a) = answer {
            let _ = p.reply.send(a);
            self.current = None;
        }
    }
}
