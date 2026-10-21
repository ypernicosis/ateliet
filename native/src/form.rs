//! A small form dialog: a few labelled fields and a submit button. What the
//! form is for travels in `purpose`; the app acts on it when it is submitted.

use crate::kit;
use crate::theme::Palette;
use eframe::egui::{self, RichText};
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
    pub palette: Palette,
    focused: bool,
}

impl Form {
    pub fn new(title: &str, purpose: Purpose, submit: &str, fields: Vec<Field>) -> Form {
        Form { title: title.into(), fields, submit: submit.into(), purpose, note: String::new(), palette: crate::theme::Palette::new(&crate::config::Appearance { dark: false, accent: [0x76, 0xce, 0xf4], font_size: 16.0 }), focused: false }
    }

    pub fn with_note(mut self, n: &str) -> Form {
        self.note = n.into();
        self
    }

    /// Show the form; returns the values when submitted, and whether it closed.
    pub fn show(&mut self, ctx: &egui::Context) -> (Option<HashMap<String, String>>, bool) {
        let mut out = None;
        let mut close = false;
        let p = self.palette;
        kit::scrim(ctx, p);
        egui::Window::new(self.title.clone()).title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 100.0]).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new(&self.title).font(kit::medium(16.0)).color(p.text));
            if !self.note.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(&self.note).color(p.muted));
            }
            ui.add_space(14.0);
            for (i, f) in self.fields.iter_mut().enumerate() {
                ui.label(RichText::new(&f.label).size(12.0).color(p.muted));
                ui.add_space(2.0);
                match &f.kind {
                    Kind::Text => {
                        let r = ui.add(egui::TextEdit::singleline(&mut f.value).hint_text(RichText::new(&f.hint).color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 7)));
                        if i == 0 && !self.focused {
                            r.request_focus();
                            self.focused = true;
                        }
                    }
                    Kind::Area => {
                        ui.add(egui::TextEdit::multiline(&mut f.value).hint_text(RichText::new(&f.hint).color(p.faint)).desired_width(f32::INFINITY).desired_rows(4).margin(egui::Margin::symmetric(10, 7)));
                    }
                    Kind::Select(opts) => {
                        let current = opts.iter().find(|o| o.0 == f.value).map(|o| o.1.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_salt(("form-select", &f.key)).selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
                            for (v, l) in opts {
                                ui.selectable_value(&mut f.value, v.clone(), l);
                            }
                        });
                    }
                }
                ui.add_space(10.0);
            }
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_area = self.fields.iter().any(|f| matches!(f.kind, Kind::Area));
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && (i.modifiers.command || (!has_area && !i.modifiers.shift)));
                let submit = if matches!(self.purpose, Purpose::ConfirmTrash { .. }) { kit::danger(ui, &self.submit, p) } else { kit::primary(ui, &self.submit, p) };
                if submit.clicked() || enter {
                    out = Some(self.fields.iter().map(|f| (f.key.clone(), f.value.clone())).collect());
                    close = true;
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                if has_area {
                    ui.label(RichText::new("Ctrl+Enter para confirmar").size(11.0).color(p.faint));
                }
            });
        });
        (out, close)
    }
}
