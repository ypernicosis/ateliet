//! A small form dialog: a few labelled fields and a submit button. What the
//! form is for travels in `purpose`; the app acts on it when it is submitted.

use eframe::egui;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Kind {
    Text,
    Area,
    /// (value, label) pairs
    Select(Vec<(String, String)>),
}

#[derive(Clone, Debug)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub kind: Kind,
    pub value: String,
    pub hint: String,
}

impl Field {
    pub fn text(key: &str, label: &str, value: &str) -> Field {
        Field { key: key.into(), label: label.into(), kind: Kind::Text, value: value.into(), hint: String::new() }
    }

    pub fn area(key: &str, label: &str, value: &str) -> Field {
        Field { kind: Kind::Area, ..Field::text(key, label, value) }
    }

    pub fn select(key: &str, label: &str, value: &str, options: &[(&str, &str)]) -> Field {
        Field { kind: Kind::Select(options.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()), ..Field::text(key, label, value) }
    }

    pub fn hint(mut self, h: &str) -> Field {
        self.hint = h.into();
        self
    }
}

#[derive(Clone, Debug)]
pub enum Purpose {
    NewBook,
    NewChapter { book: String },
    NewCharacter { book: String },
    RenameChapter { path: String },
    ChapterPart { path: String },
    ConfirmTrash { path: String },
}

pub struct Form {
    pub title: String,
    pub fields: Vec<Field>,
    pub submit: String,
    pub purpose: Purpose,
    /// A line of explanation above the fields.
    pub note: String,
    focused: bool,
}

impl Form {
    pub fn new(title: &str, purpose: Purpose, submit: &str, fields: Vec<Field>) -> Form {
        Form { title: title.into(), fields, submit: submit.into(), purpose, note: String::new(), focused: false }
    }

    pub fn with_note(mut self, n: &str) -> Form {
        self.note = n.into();
        self
    }

    /// Show the form; returns the values when submitted, and whether it closed.
    pub fn show(&mut self, ctx: &egui::Context) -> (Option<HashMap<String, String>>, bool) {
        let mut out = None;
        let mut close = false;
        egui::Window::new(self.title.clone()).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 110.0]).show(ctx, |ui| {
            ui.set_width(420.0);
            if !self.note.is_empty() {
                ui.label(&self.note);
                ui.add_space(6.0);
            }
            egui::Grid::new("form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                for (i, f) in self.fields.iter_mut().enumerate() {
                    ui.label(&f.label);
                    match &f.kind {
                        Kind::Text => {
                            let r = ui.add(egui::TextEdit::singleline(&mut f.value).hint_text(&f.hint).desired_width(260.0));
                            if i == 0 && !self.focused {
                                r.request_focus();
                                self.focused = true;
                            }
                        }
                        Kind::Area => {
                            ui.add(egui::TextEdit::multiline(&mut f.value).hint_text(&f.hint).desired_width(260.0).desired_rows(4));
                        }
                        Kind::Select(opts) => {
                            let current = opts.iter().find(|o| o.0 == f.value).map(|o| o.1.clone()).unwrap_or_default();
                            egui::ComboBox::from_id_salt(("form-select", &f.key)).selected_text(current).width(260.0).show_ui(ui, |ui| {
                                for (v, l) in opts {
                                    ui.selectable_value(&mut f.value, v.clone(), l);
                                }
                            });
                        }
                    }
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let has_area = self.fields.iter().any(|f| matches!(f.kind, Kind::Area));
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && (i.modifiers.command || (!has_area && !i.modifiers.shift)));
                if ui.button(&self.submit).clicked() || enter {
                    out = Some(self.fields.iter().map(|f| (f.key.clone(), f.value.clone())).collect());
                    close = true;
                }
                if ui.button("Cancelar").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        (out, close)
    }
}
