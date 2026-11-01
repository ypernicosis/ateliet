//! Literal, Unicode-aware search within one open note. No vault files change.

use eframe::egui::{self, Color32, Key, Modifiers};
use regex::{Regex, RegexBuilder};
use std::cell::RefCell;
use std::ops::Range;

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    focus: bool,
    pattern: Option<Regex>,
    text_key: Option<u64>,
    compiled: String,
    matches: Vec<Range<usize>>,
    current: usize,
    /// A source byte range awaiting navigation, without moving keyboard focus.
    pub pending: Option<Range<usize>>,
}

impl Find {
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
        self.pending = self.matches.get(self.current).cloned();
    }

    fn refresh(&mut self, text: &str) {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hash);
        let key = hash.finish();
        let changed = self.compiled != self.query;
        if !changed && self.text_key == Some(key) { return; }
        self.text_key = Some(key);
        if changed {
            self.compiled = self.query.clone();
            self.pattern = pattern(&self.query);
            self.current = 0;
        }
        let matches = self.pattern.as_ref().map(|p| p.find_iter(text).map(|m| m.range()).collect()).unwrap_or_default();
        if changed || self.matches != matches {
            self.matches = matches;
            self.current = self.current.min(self.matches.len().saturating_sub(1));
            self.pending = self.matches.get(self.current).cloned();
        }
    }

    fn step(&mut self, backwards: bool) {
        if self.matches.is_empty() {
            return;
        }
        let n = self.matches.len();
        self.current = if backwards { (self.current + n - 1) % n } else { (self.current + 1) % n };
        self.pending = Some(self.matches[self.current].clone());
    }

    pub fn close(&mut self) {
        self.open = false;
        self.pending = None;
    }

    /// Drawn above the note's scroll area, so it stays visible while navigating.
    pub fn bar(&mut self, ui: &mut egui::Ui, path: &str, text: &str) {
        if !self.open {
            return;
        }
        let mut close = false;
        let mut field = None;
        ui.push_id(("note-find", path), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Buscar na nota");
                let response = ui.add(egui::TextEdit::singleline(&mut self.query).id_source("query").hint_text("Palavra ou trecho…").desired_width(190.0));
                field = Some(response.id);
                if response.has_focus() || response.lost_focus() {
                    close |= ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
                }
                if std::mem::take(&mut self.focus) {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.query.chars().count()))));
                        state.store(ui.ctx(), response.id);
                    }
                }
                self.refresh(text);
                let enter = (response.has_focus() || response.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
                if enter {
                    let backwards = ui.input(|i| i.modifiers.shift);
                    ui.input_mut(|i| { i.consume_key(if backwards { Modifiers::SHIFT } else { Modifiers::NONE }, Key::Enter); });
                    self.step(backwards);
                    response.request_focus();
                }
                let count = if self.matches.is_empty() { "0 / 0".into() } else { format!("{} / {}", self.current + 1, self.matches.len()) };
                ui.label(count);
                if ui.add_enabled(!self.matches.is_empty(), egui::Button::new("↑")).on_hover_text("Anterior (Shift+Enter)").clicked() {
                    self.step(true);
                }
                if ui.add_enabled(!self.matches.is_empty(), egui::Button::new("↓")).on_hover_text("Próximo (Enter)").clicked() {
                    self.step(false);
                }
                close |= ui.button("×").on_hover_text("Fechar (Esc)").clicked();
                if !self.query.is_empty() && self.matches.is_empty() {
                    ui.label("Nenhum resultado");
                }
            });
        });
        if close {
            self.close();
            if let Some(id) = field { ui.ctx().memory_mut(|m| m.surrender_focus(id)); }
        }
        ui.separator();
    }
}

fn pattern(query: &str) -> Option<Regex> {
    if query.is_empty() { None } else { RegexBuilder::new(&regex::escape(query)).case_insensitive(true).build().ok() }
}

thread_local! {
    static HIGHLIGHT: RefCell<Option<Regex>> = const { RefCell::new(None) };
    static TARGET: RefCell<Option<usize>> = const { RefCell::new(None) };
    static SCROLLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Scoped to the note pane; sidebars and other split panes are unaffected.
pub fn set_highlight(find: &Find) {
    HIGHLIGHT.with(|h| *h.borrow_mut() = if find.open { find.pattern.clone() } else { None });
}

/// A rendered block's source occurrence, counted before Markdown removes marks.
pub fn begin_target(text: &str, byte: usize) {
    let ordinal = HIGHLIGHT.with(|h| h.borrow().as_ref().map(|p| p.find_iter(&text[..byte]).count()));
    TARGET.with(|t| *t.borrow_mut() = ordinal);
    SCROLLED.with(|s| s.set(false));
}

pub fn end_target() -> bool {
    TARGET.with(|t| *t.borrow_mut() = None);
    SCROLLED.with(|s| s.replace(false))
}

pub fn clear_highlight() {
    HIGHLIGHT.with(|h| *h.borrow_mut() = None);
    TARGET.with(|t| *t.borrow_mut() = None);
}

pub fn plain(ui: &mut egui::Ui, text: &str, font: egui::FontId, color: Color32) {
    let galley = ui.fonts(|f| f.layout(text.to_owned(), font, color, ui.available_width()));
    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    paint(ui, &galley, rect.min);
    ui.painter().galley(rect.min, galley, color);
}

/// Paint on the galley's real wrapped rows, keeping fonts and Markdown intact.
pub fn paint(ui: &egui::Ui, galley: &egui::Galley, origin: egui::Pos2) {
    HIGHLIGHT.with(|h| {
        let h = h.borrow();
        let Some(pattern) = h.as_ref() else { return };
        let mut byte = 0;
        let mut chars = 0;
        for m in pattern.find_iter(&galley.job.text) {
            let a = chars + galley.job.text[byte..m.start()].chars().count();
            let b = a + m.as_str().chars().count();
            byte = m.end();
            chars = b;
            let target = TARGET.with(|t| {
                let mut t = t.borrow_mut();
                match *t {
                    Some(0) => { *t = None; true }
                    Some(n) => { *t = Some(n - 1); false }
                    None => false,
                }
            });
            let rects = range_rects(galley, a..b);
            if target {
                if let Some(rect) = rects.first() {
                    ui.scroll_to_rect(rect.translate(origin.to_vec2()), Some(egui::Align::Center));
                    SCROLLED.with(|s| s.set(true));
                }
            }
            for rect in rects {
                ui.painter().rect_filled(rect.translate(origin.to_vec2()), 2.0, Color32::from_rgba_unmultiplied(225, 170, 35, 65));
            }
        }
    });
}

fn range_rects(galley: &egui::Galley, range: Range<usize>) -> Vec<egui::Rect> {
    let mut offset = 0;
    let mut rects = Vec::new();
    for row in &galley.rows {
        if offset >= range.end { break; }
        let end = offset + row.char_count_excluding_newline();
        let a = range.start.max(offset);
        let b = range.end.min(end);
        if a < b {
            let x = row.x_offset(a - offset);
            let right = row.x_offset(b - offset);
            rects.push(egui::Rect::from_min_max(egui::pos2(x, row.min_y()), egui::pos2(right, row.max_y())));
        }
        offset += row.char_count_including_newline();
    }
    rects
}

pub fn request_scroll(ctx: &egui::Context, id: egui::Id, range: Range<usize>) {
    ctx.data_mut(|d| d.insert_temp(id.with("find-scroll"), range));
}

pub fn scroll_editor(ui: &egui::Ui, id: egui::Id, out: &egui::text_edit::TextEditOutput) {
    let range = ui.ctx().data_mut(|d| d.remove_temp::<Range<usize>>(id.with("find-scroll")));
    if let Some(range) = range {
        let text = &out.galley.job.text;
        let boundary = |at: usize| {
            let mut at = at.min(text.len());
            while !text.is_char_boundary(at) { at -= 1; }
            at
        };
        let a = text[..boundary(range.start)].chars().count();
        let b = text[..boundary(range.end)].chars().count();
        if let Some(rect) = range_rects(&out.galley, a..b).first() {
            ui.scroll_to_rect(rect.translate(out.galley_pos.to_vec2()), Some(egui::Align::Center));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_unicode_search_and_wraparound() {
        let mut find = Find { query: "AÇÃO".into(), ..Default::default() };
        let text = "日本 ação e AÇÃO";
        find.refresh(text);
        assert_eq!(find.matches.len(), 2);
        for range in &find.matches { assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end)); }
        find.step(true);
        assert_eq!(find.current, 1);
        find.step(false);
        assert_eq!(find.current, 0);
        find.query = "[a+b]".into();
        find.refresh("[a+b] aaab");
        assert_eq!(find.matches, vec![0..5]);
    }

    #[test]
    fn edits_query_changes_and_empty_results() {
        let mut find = Find { query: "a".into(), ..Default::default() };
        find.refresh("a a");
        find.step(false);
        find.refresh("a");
        assert_eq!(find.current, 0);
        assert_eq!(find.pending, Some(0..1));
        find.query = "missing".into();
        find.refresh("a");
        find.step(true);
        assert!(find.matches.is_empty());
        assert!(find.pending.is_none());
        find.query.clear();
        find.refresh("a");
        assert!(find.matches.is_empty());
    }
    #[test]
    fn keyboard_navigation_keeps_focus_and_never_edits_note() {
        let ctx = egui::Context::default();
        let mut find = Find { query: "ação".into(), ..Default::default() };
        find.open();
        let text = "ação, AÇÃO, ação";
        let frame = |find: &mut Find, key: Option<(Key, Modifiers)>| {
            let mut input = egui::RawInput::default();
            if let Some((key, modifiers)) = key {
                input.modifiers = modifiers;
                input.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
            }
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| find.bar(ui, "Nota.md", text));
            });
        };
        frame(&mut find, None);
        assert!(ctx.wants_keyboard_input());
        frame(&mut find, Some((Key::Enter, Modifiers::NONE)));
        assert_eq!(find.current, 1);
        assert!(ctx.wants_keyboard_input());
        frame(&mut find, Some((Key::Enter, Modifiers::SHIFT)));
        assert_eq!(find.current, 0);
        frame(&mut find, Some((Key::Escape, Modifiers::NONE)));
        assert!(!find.open);
        assert!(find.pending.is_none());
        assert_eq!(text, "ação, AÇÃO, ação");
    }

    #[test]
    fn wrapped_highlights_follow_glyph_geometry() {
        let fonts = egui::text::Fonts::new(1.0, 2048, egui::FontDefinitions::default());
        let galley = fonts.layout("one two three four five".into(), egui::FontId::proportional(16.0), Color32::WHITE, 70.0);
        let rects = range_rects(&galley, 0..23);
        assert!(rects.len() > 1);
        for rect in rects { assert!(rect.width() > 0.0 && rect.height() > 0.0); }
    }

}
