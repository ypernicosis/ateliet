//! The interface's building blocks, so every screen speaks the same visual
//! language: icon buttons, list rows, primary and quiet buttons, section
//! labels, hairlines, toasts.

use crate::icons;
use crate::theme::{Palette, UI, UI_SMALL};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Response, RichText, Sense, Stroke, Ui};

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// An icon as text (for buttons and labels).
pub fn icon(glyph: &str, size: f32) -> RichText {
    RichText::new(glyph).font(icon_font(size))
}

/// A square, frameless icon button: muted at rest, a soft fill on hover,
/// the "selected" fill when active.
pub fn icon_button(ui: &mut Ui, glyph: &str, tip: &str, active: bool, p: Palette) -> Response {
    icon_button_sized(ui, glyph, tip, active, p, 28.0, 16.0)
}

pub fn icon_button_sized(ui: &mut Ui, glyph: &str, tip: &str, active: bool, p: Palette, side: f32, size: f32) -> Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if active {
            p.bg_active
        } else if r.is_pointer_button_down_on() {
            p.bg_active
        } else if r.hovered() {
            p.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6.0, fill);
        let color = if active || r.hovered() { p.text } else { p.muted };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, icon_font(size), color);
    }
    if tip.is_empty() { r } else { r.on_hover_text(tip) }
}

/// The one strong button of a screen.
pub fn primary(ui: &mut Ui, label: &str, p: Palette) -> Response {
    let r = ui.add(egui::Button::new(RichText::new(label).font(medium(UI)).color(p.on_accent)).fill(p.accent_strong).stroke(Stroke::NONE).corner_radius(7.0).min_size(egui::vec2(0.0, 30.0)));
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// A destructive confirmation.
pub fn danger(ui: &mut Ui, label: &str, p: Palette) -> Response {
    ui.add(egui::Button::new(RichText::new(label).font(medium(UI)).color(Color32::WHITE)).fill(p.error).stroke(Stroke::NONE).corner_radius(7.0).min_size(egui::vec2(0.0, 30.0)))
}

/// A primary button with an icon before the label.
pub fn primary_icon(ui: &mut Ui, glyph: &str, label: &str, p: Palette) -> Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(glyph, 0.0, egui::TextFormat { font_id: icon_font(14.0), color: p.on_accent, valign: egui::Align::Center, ..Default::default() });
    job.append(label, 8.0, egui::TextFormat { font_id: medium(UI), color: p.on_accent, valign: egui::Align::Center, ..Default::default() });
    let r = ui.add(egui::Button::new(job).fill(p.accent_strong).stroke(Stroke::NONE).corner_radius(7.0).min_size(egui::vec2(0.0, 30.0)));
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// A secondary button: hairline outline, with an optional icon.
pub fn button(ui: &mut Ui, glyph: Option<&str>, label: &str, p: Palette) -> Response {
    let mut job = egui::text::LayoutJob::default();
    if let Some(g) = glyph {
        job.append(g, 0.0, egui::TextFormat { font_id: icon_font(14.0), color: p.muted, valign: egui::Align::Center, ..Default::default() });
    }
    job.append(label, if glyph.is_some() { 8.0 } else { 0.0 }, egui::TextFormat { font_id: FontId::proportional(UI), color: p.text, valign: egui::Align::Center, ..Default::default() });
    ui.add(egui::Button::new(job).corner_radius(7.0).min_size(egui::vec2(0.0, 30.0)))
}

/// A frameless text button (links, "show more"…).
pub fn quiet(ui: &mut Ui, glyph: Option<&str>, label: &str, p: Palette) -> Response {
    let mut job = egui::text::LayoutJob::default();
    if let Some(g) = glyph {
        job.append(g, 0.0, egui::TextFormat { font_id: icon_font(13.0), color: p.muted, valign: egui::Align::Center, ..Default::default() });
    }
    job.append(label, if glyph.is_some() { 6.0 } else { 0.0 }, egui::TextFormat { font_id: FontId::proportional(UI), color: p.muted, valign: egui::Align::Center, ..Default::default() });
    ui.add(egui::Button::new(job).frame(false))
}

/// A small label above a group ("Files", "Linked mentions"…).
pub fn section(ui: &mut Ui, title: &str, p: Palette) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).font(medium(UI_SMALL)).color(p.faint));
    ui.add_space(2.0);
}

pub fn hairline(ui: &mut Ui, p: Palette) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, p.border));
}

/// One row of a list (file tree, outline, results, menus).
pub struct Row<'a> {
    pub label: &'a str,
    pub icon: Option<&'a str>,
    pub depth: usize,
    pub selected: bool,
    pub dim: bool,
    /// `Some(open)` draws a disclosure chevron.
    pub chevron: Option<bool>,
    pub trailing: Option<String>,
    /// A coloured dot instead of an icon.
    pub dot: Option<Color32>,
    pub height: f32,
}

impl<'a> Row<'a> {
    pub fn new(label: &'a str) -> Row<'a> {
        Row { label, icon: None, depth: 0, selected: false, dim: false, chevron: None, trailing: None, dot: None, height: 28.0 }
    }
    pub fn icon(mut self, g: &'a str) -> Self {
        self.icon = Some(g);
        self
    }
    pub fn depth(mut self, d: usize) -> Self {
        self.depth = d;
        self
    }
    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }
    pub fn dim(mut self, d: bool) -> Self {
        self.dim = d;
        self
    }
    pub fn chevron(mut self, open: bool) -> Self {
        self.chevron = Some(open);
        self
    }
    pub fn trailing(mut self, t: impl Into<String>) -> Self {
        self.trailing = Some(t.into());
        self
    }
    pub fn dot(mut self, c: Color32) -> Self {
        self.dot = Some(c);
        self
    }

    pub fn show(self, ui: &mut Ui, p: Palette) -> Response {
        let w = ui.available_width();
        let (rect, r) = ui.allocate_exact_size(egui::vec2(w, self.height), Sense::click());
        if !ui.is_rect_visible(rect) {
            return r;
        }
        let painter = ui.painter_at(rect);
        let fill = if self.selected {
            p.bg_active
        } else if r.hovered() {
            p.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, CornerRadius::same(6), fill);
        let mut x = rect.left() + 8.0 + self.depth as f32 * 14.0;
        let cy = rect.center().y;
        if let Some(open) = self.chevron {
            painter.text(egui::pos2(x + 6.0, cy), Align2::CENTER_CENTER, if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }, icon_font(12.0), p.faint);
            x += 16.0;
        }
        if let Some(c) = self.dot {
            painter.circle_filled(egui::pos2(x + 4.0, cy), 3.5, c);
            x += 16.0;
        } else if let Some(g) = self.icon {
            painter.text(egui::pos2(x + 7.0, cy), Align2::CENTER_CENTER, g, icon_font(14.0), if self.selected { p.text } else { p.muted });
            x += 22.0;
        }
        let mut right = rect.right() - 8.0;
        if let Some(t) = &self.trailing {
            let mut job = egui::text::LayoutJob::single_section(t.clone(), egui::TextFormat { font_id: FontId::proportional(UI_SMALL), color: p.faint, ..Default::default() });
            job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() * 0.5);
            let g = ui.fonts(|f| f.layout_job(job));
            let gw = g.size().x;
            painter.galley(egui::pos2(right - gw, cy - g.size().y / 2.0), g, p.faint);
            right -= gw.max(24.0) + 10.0;
        }
        let color = if self.dim { p.faint } else if self.selected { p.text } else { p.text.gamma_multiply(0.92) };
        let font = if self.selected { medium(UI) } else { FontId::proportional(UI) };
        let galley = painter.layout(self.label.to_string(), font, color, f32::INFINITY);
        let max = (right - x).max(10.0);
        let g = if galley.size().x > max {
            let mut job = egui::text::LayoutJob::single_section(self.label.to_string(), egui::TextFormat { font_id: if self.selected { medium(UI) } else { FontId::proportional(UI) }, color, ..Default::default() });
            job.wrap = egui::text::TextWrapping::truncate_at_width(max);
            ui.fonts(|f| f.layout_job(job))
        } else {
            galley
        };
        painter.galley(egui::pos2(x, cy - g.size().y / 2.0), g, color);
        r
    }
}

/// A text field with a leading icon (search boxes).
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, p: Palette) -> Response {
    let r = egui::Frame::new().fill(if p.dark { Color32::from_rgb(0x15, 0x15, 0x15) } else { p.bg }).stroke(Stroke::new(1.0_f32, p.border_strong)).corner_radius(7.0).inner_margin(egui::Margin { left: 8, right: 8, top: 4, bottom: 4 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(icon(icons::SEARCH, 13.0).color(p.faint));
            ui.add(egui::TextEdit::singleline(text).hint_text(RichText::new(hint).color(p.faint)).frame(false).desired_width(f32::INFINITY))
        })
        .inner
    });
    r.inner
}

/// An on/off switch.
pub fn toggle(ui: &mut Ui, on: &mut bool, p: Palette) -> Response {
    let (rect, mut r) = ui.allocate_exact_size(egui::vec2(34.0, 20.0), Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(r.id, *on, 0.12);
    let track = if *on { p.accent_strong } else { p.border_strong };
    ui.painter().rect_filled(rect, 10.0, track);
    let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    r
}

/// A settings row: label and description on the left, the control on the right.
pub fn setting<R>(ui: &mut Ui, label: &str, desc: &str, p: Palette, control: impl FnOnce(&mut Ui) -> R) -> R {
    let mut out = None;
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.allocate_ui_with_layout(egui::vec2(w * 0.55, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.label(RichText::new(label).font(medium(UI)).color(p.text));
            if !desc.is_empty() {
                ui.label(RichText::new(desc).size(UI_SMALL + 0.5).color(p.faint));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            out = Some(control(ui));
        });
    });
    ui.add_space(12.0);
    hairline(ui, p);
    ui.add_space(12.0);
    out.unwrap()
}

/// A veil over the app behind a modal (drawn, not clickable).
pub fn scrim(ctx: &egui::Context, p: Palette) {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("modal-scrim")).order(egui::Order::Middle).interactable(false).fixed_pos(screen.min).show(ctx, |ui| {
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(if p.dark { 110 } else { 46 }));
    });
}

/// A segmented control: icon + label pills on a soft track. Returns the
/// index clicked.
pub fn segmented(ui: &mut Ui, items: &[(&str, &str)], selected: usize, p: Palette) -> Option<usize> {
    let mut out = None;
    egui::Frame::new().fill(p.bg_secondary).corner_radius(9.0).inner_margin(3.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, (glyph, label)) in items.iter().enumerate() {
                let on = i == selected;
                let mut job = egui::text::LayoutJob::default();
                if !glyph.is_empty() {
                    job.append(glyph, 0.0, egui::TextFormat { font_id: icon_font(13.0), color: if on { p.text } else { p.muted }, valign: egui::Align::Center, ..Default::default() });
                }
                if !label.is_empty() {
                    job.append(label, if glyph.is_empty() { 0.0 } else { 6.0 }, egui::TextFormat { font_id: if on { medium(12.5) } else { FontId::proportional(12.5) }, color: if on { p.text } else { p.muted }, valign: egui::Align::Center, ..Default::default() });
                }
                let g = ui.fonts(|f| f.layout_job(job));
                let (rect, r) = ui.allocate_exact_size(egui::vec2(g.size().x + 20.0, 26.0), Sense::click());
                if on {
                    ui.painter().rect(rect, 7.0, p.bg, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                } else if r.hovered() {
                    ui.painter().rect_filled(rect, 7.0, p.bg_hover);
                }
                ui.painter().galley(rect.center() - g.size() / 2.0, g, p.text);
                if r.clicked() {
                    out = Some(i);
                }
            }
        });
    });
    out
}

/// A small outlined pill with an optional icon (pickers, filters).
pub fn chip(ui: &mut Ui, glyph: Option<&str>, label: &str, active: bool, p: Palette) -> Response {
    let mut job = egui::text::LayoutJob::default();
    let fg = if active { p.accent_strong } else { p.muted };
    if let Some(g) = glyph {
        job.append(g, 0.0, egui::TextFormat { font_id: icon_font(12.5), color: fg, valign: egui::Align::Center, ..Default::default() });
    }
    job.append(label, if glyph.is_some() { 6.0 } else { 0.0 }, egui::TextFormat { font_id: FontId::proportional(12.5), color: if active { p.accent_strong } else { p.text }, valign: egui::Align::Center, ..Default::default() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(260.0);
    let g = ui.fonts(|f| f.layout_job(job));
    let (rect, r) = ui.allocate_exact_size(egui::vec2(g.size().x + 20.0, 26.0), Sense::click());
    let fill = if active { p.accent_soft } else if r.hovered() { p.bg_hover } else { Color32::TRANSPARENT };
    ui.painter().rect(rect, 13.0, fill, Stroke::new(1.0_f32, if active { p.accent_soft } else { p.border_strong }), egui::StrokeKind::Inside);
    ui.painter().galley(egui::pos2(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, p.text);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// A context-menu entry: icon, label, optional shortcut on the right.
/// Closes the menu when clicked.
pub fn menu_item(ui: &mut Ui, glyph: &str, label: &str, p: Palette) -> bool {
    menu_entry(ui, glyph, label, "", true, false, p)
}

/// A destructive entry (shown in the error colour).
pub fn menu_danger(ui: &mut Ui, glyph: &str, label: &str, p: Palette) -> bool {
    menu_entry(ui, glyph, label, "", true, true, p)
}

pub fn menu_entry(ui: &mut Ui, glyph: &str, label: &str, keys: &str, enabled: bool, danger: bool, p: Palette) -> bool {
    let need = ui.fonts(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(UI), p.text).size().x + if keys.is_empty() { 0.0 } else { f.layout_no_wrap(keys.to_string(), FontId::proportional(UI_SMALL), p.faint).size().x + 24.0 }) + 52.0;
    let w = need.max(220.0).max(ui.min_rect().width());
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right().max(ui.min_rect().right()), rect.bottom()));
    if r.hovered() && enabled {
        ui.painter().rect_filled(rect, 5.0, p.bg_hover);
    }
    let fg = if !enabled { p.faint } else if danger { p.error } else { p.text };
    let cy = rect.center().y;
    if !glyph.is_empty() {
        ui.painter().text(egui::pos2(rect.left() + 15.0, cy), Align2::CENTER_CENTER, glyph, icon_font(14.0), if danger && enabled { p.error } else if enabled { p.muted } else { p.faint });
    }
    ui.painter().text(egui::pos2(rect.left() + 32.0, cy), Align2::LEFT_CENTER, label, FontId::proportional(UI), fg);
    if !keys.is_empty() {
        ui.painter().text(egui::pos2(rect.right() - 10.0, cy), Align2::RIGHT_CENTER, keys, FontId::proportional(UI_SMALL), p.faint);
    }
    let clicked = enabled && r.clicked();
    if clicked {
        ui.close_menu();
    }
    clicked
}

/// A context-menu entry with a check mark when on.
pub fn menu_check(ui: &mut Ui, on: bool, label: &str, p: Palette) -> bool {
    menu_entry(ui, if on { icons::CHECK } else { "" }, label, "", true, false, p)
}

/// A thin line between groups of a menu.
pub fn menu_sep(ui: &mut Ui, p: Palette) {
    ui.add_space(3.0);
    let w = ui.min_rect().width().max(220.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), Sense::hover());
    ui.painter().hline(rect.x_range().shrink(6.0), rect.center().y, Stroke::new(1.0_f32, p.border));
    ui.add_space(3.0);
}
