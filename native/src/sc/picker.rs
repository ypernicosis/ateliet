//! A searchable list in a modal (pictures, characters…), in the palette's style.

use crate::commands::fuzzy;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use eframe::egui::{self, Key, RichText};

#[derive(Clone, Debug)]
pub enum Purpose {
    Portrait(String),
    Gallery(String),
    /// (sheet path, list key)
    Person(String, String),
}

#[derive(Clone, Debug)]
pub struct Picker {
    pub title: String,
    /// (label, value, detail)
    pub items: Vec<(String, String, String)>,
    pub purpose: Purpose,
    /// Offer the typed text itself as a choice (free names).
    pub free: bool,
    pub icon: &'static str,
    query: String,
    selected: usize,
}

impl Picker {
    pub fn new(title: &str, items: Vec<(String, String, String)>, purpose: Purpose, icon: &'static str) -> Picker {
        Picker { title: title.into(), items, purpose, free: false, icon, query: String::new(), selected: 0 }
    }

    /// Show; returns the chosen value (and whether it closed).
    pub fn show(&mut self, ctx: &egui::Context, p: Palette) -> (Option<String>, bool) {
        let mut shown: Vec<(i64, usize)> = self.items.iter().enumerate().filter_map(|(i, it)| fuzzy(&self.query, &it.0).or_else(|| fuzzy(&self.query, &it.2)).map(|s| (s, i))).collect();
        if !self.query.is_empty() {
            shown.sort_by_key(|x| std::cmp::Reverse(x.0));
        }
        shown.truncate(80);
        let free = self.free && !self.query.trim().is_empty();
        let n = shown.len() + free as usize;
        if ctx.input(|i| i.key_pressed(Key::ArrowDown)) && n > 0 {
            self.selected = (self.selected + 1) % n;
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowUp)) && n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
        self.selected = self.selected.min(n.saturating_sub(1));
        let mut chosen = None;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        kit::scrim(ctx, p);
        egui::Window::new("picker").title_bar(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).frame(egui::Frame::window(&ctx.style()).inner_margin(egui::Margin::ZERO).corner_radius(12.0)).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 18, top: 14, bottom: 14 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(kit::icon(icons::SEARCH, 16.0).color(p.faint));
                    ui.add_space(10.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(RichText::new(&self.title).color(p.faint)).frame(false).desired_width(f32::INFINITY).font(egui::FontId::proportional(16.0)));
                    r.request_focus();
                    if r.changed() {
                        self.selected = 0;
                    }
                });
            });
            kit::hairline(ui, p);
            egui::Frame::new().inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (k, &(_, i)) in shown.iter().enumerate() {
                        let (label, value, detail) = &self.items[i];
                        let mut row = kit::Row::new(label).icon(self.icon).selected(k == self.selected).trailing(detail.clone());
                        row.height = 34.0;
                        let r = row.show(ui, p);
                        if k == self.selected {
                            r.scroll_to_me(None);
                        }
                        if r.clicked() {
                            chosen = Some(value.clone());
                        }
                    }
                    if free {
                        let label = format!("Usar o nome “{}”", self.query.trim());
                        let mut row = kit::Row::new(&label).icon(icons::PENCIL).selected(self.selected == shown.len());
                        row.height = 34.0;
                        if row.show(ui, p).clicked() {
                            chosen = Some(self.query.trim().to_string());
                        }
                    }
                    if n == 0 {
                        ui.add_space(12.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new("Nada encontrado").color(p.faint)));
                        ui.add_space(12.0);
                    }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(Key::Enter)) && n > 0 {
            chosen = Some(if self.selected < shown.len() { self.items[shown[self.selected].1].1.clone() } else { self.query.trim().to_string() });
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
        }
        if chosen.is_some() {
            close = true;
        }
        (chosen, close)
    }
}
