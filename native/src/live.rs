//! Live Preview: the note is split into top-level blocks; every block is shown
//! rendered except the one being edited, which becomes a text editor of its
//! own lines. Clicking a block edits it; arrows at its edges move to the
//! neighbouring block; Escape shows it rendered again.

use crate::editor;
use crate::markdown::{self, Doc};
use ateliet_core::syntax;
use ateliet_core::text::LineIndex;
use pulldown_cmark::{Event, Parser};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone)]
pub struct Segment {
    /// First and last line (inclusive).
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub doc: Arc<Doc>,
    pub frontmatter: bool,
}

/// Top-level blocks of a note with their source lines.
pub fn segments(text: &str) -> Vec<Segment> {
    let prep = syntax::prepare(text);
    let idx = LineIndex::new(text);
    let mut ranges: Vec<(usize, usize, bool)> = Vec::new();
    if let Some(fm) = &prep.frontmatter {
        ranges.push((0, idx.line_of(fm.block.end.saturating_sub(1).max(fm.block.start)), true));
    }
    let mut depth = 0usize;
    let mut start = 0usize;
    for (ev, r) in Parser::new_ext(&prep.masked, syntax::md_options()).into_offset_iter() {
        match ev {
            Event::Start(_) => {
                if depth == 0 {
                    start = r.start;
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = idx.trim_end(start, r.end.min(text.len())).max(start + 1).min(text.len());
                    let last = idx.line_of(end.saturating_sub(1).max(start));
                    ranges.push((idx.line_of(start), last, false));
                }
            }
            Event::Rule | Event::Html(_) | Event::DisplayMath(_) if depth == 0 => {
                let last = idx.line_of(r.end.saturating_sub(1).max(r.start));
                ranges.push((idx.line_of(r.start), last, false));
            }
            _ => {}
        }
    }
    // blocks the parser nested (list items) may overlap; keep them disjoint and ordered
    ranges.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize, bool)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    let lines: Vec<&str> = text.split('\n').collect();
    merged
        .into_iter()
        .map(|(s, e, fm)| {
            let e = e.min(lines.len().saturating_sub(1));
            let t = lines[s..=e].join("\n");
            let doc = Arc::new(if fm { markdown::Doc { blocks: Vec::new(), headings: Vec::new() } } else { markdown::parse(&t) });
            Segment { start: s, end: e, text: t, doc, frontmatter: fm }
        })
        .collect()
}

/// The segments of a note, recomputed when its text changes.
#[derive(Default)]
pub struct LiveState {
    key: u64,
    pub segs: Vec<Segment>,
    /// The block being edited: its first line, and its text while editing.
    pub active: Option<usize>,
    pub buf: String,
    pub buf_end: usize,
    pub highlight: editor::HighlightCache,
    /// Where to put the cursor when a block opens (char index), or the end.
    pub cursor_at: Option<usize>,
}

impl LiveState {
    pub fn refresh(&mut self, text: &str) {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        let k = h.finish();
        if k != self.key || self.segs.is_empty() {
            self.segs = segments(text);
            self.key = k;
        }
    }

    pub fn open(&mut self, seg: &Segment, cursor_at_end: bool) {
        self.active = Some(seg.start);
        self.buf = seg.text.clone();
        self.buf_end = seg.end;
        self.cursor_at = Some(if cursor_at_end { seg.text.chars().count() } else { 0 });
    }

    pub fn close(&mut self) {
        self.active = None;
    }

    /// Put the edited lines back into the note.
    pub fn splice(&mut self, text: &str) -> String {
        let Some(start) = self.active else { return text.to_string() };
        let lines: Vec<&str> = text.split('\n').collect();
        let end = self.buf_end.min(lines.len().saturating_sub(1));
        let mut out: Vec<&str> = Vec::with_capacity(lines.len());
        out.extend_from_slice(&lines[..start.min(lines.len())]);
        let buf = self.buf.clone();
        let mut s = out.join("\n");
        if !out.is_empty() {
            s.push('\n');
        }
        s.push_str(&buf);
        if end + 1 < lines.len() {
            s.push('\n');
            s.push_str(&lines[end + 1..].join("\n"));
        }
        self.buf_end = start + buf.split('\n').count() - 1;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_and_splice() {
        let t = "---\na: 1\n---\n# Title\n\nPara one\nstill one\n\n- a\n- b\n\n```js\nx\n```\n";
        let s = segments(t);
        let spans: Vec<(usize, usize)> = s.iter().map(|x| (x.start, x.end)).collect();
        assert_eq!(spans, vec![(0, 2), (3, 3), (5, 6), (8, 9), (11, 13)]);
        let mut l = LiveState::default();
        l.open(&s[2], true);
        l.buf = "Para one changed\nstill one\nand three".into();
        let out = l.splice(t);
        assert!(out.contains("# Title\n\nPara one changed\nstill one\nand three\n\n- a"));
        assert_eq!(l.buf_end, 7);
    }
}

// ── drawing ────────────────────────────────────────────────────────────────

use crate::app::Tab;
use crate::markdown::{Action, Host, Renderer};
use eframe::egui;

fn properties(ui: &mut egui::Ui, text: &str, host: &dyn Host) {
    let p = host.palette();
    let yaml = text.trim_start_matches("---").trim_end().trim_end_matches("---");
    let data = syntax::parse_yaml(yaml).unwrap_or(serde_json::Value::Null);
    let Some(o) = data.as_object().filter(|o| !o.is_empty()) else { return };
    egui::Frame::new().fill(p.bg_secondary).corner_radius(8.0).inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 10 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new(("props", text.len())).spacing([20.0, 5.0]).show(ui, |ui| {
            for (k, v) in o {
                ui.label(egui::RichText::new(k).size(13.0).color(p.faint));
                let shown = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Array(a) => a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(", "),
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                };
                ui.label(egui::RichText::new(if shown.is_empty() { "—".to_string() } else { shown }).size(13.0).color(p.text));
                ui.end_row();
            }
        });
    });
    ui.add_space(host.size() * 0.6);
}

/// Draw the note in Live Preview; returns the actions of clicked links and tasks.
pub fn show(ui: &mut egui::Ui, tab: &mut Tab, host: &mut dyn Host, st: &editor::Styles) -> Vec<Action> {
    let mut actions = Vec::new();
    tab.live.refresh(&tab.text);
    let segs = tab.live.segs.clone();
    let id = egui::Id::new(("live-editor", &tab.path));
    if segs.is_empty() && tab.live.active.is_none() {
        // an empty note: write right away
        tab.live.active = Some(0);
        tab.live.buf = tab.text.clone();
        tab.live.buf_end = tab.text.split('\n').count().saturating_sub(1);
        tab.live.cursor_at = Some(tab.text.chars().count());
    }
    let active = tab.live.active;
    let buf_end = tab.live.buf_end;
    let mut drawn_editor = false;
    let mut open: Option<(usize, bool)> = None;
    let size = host.size();
    for (i, seg) in segs.iter().enumerate() {
        if let Some(a) = active {
            if seg.start >= a && seg.start <= buf_end || (seg.start < a && seg.end >= a) {
                if !drawn_editor {
                    drawn_editor = true;
                    if let Some(nav) = editor_block(ui, tab, id, st) {
                        match nav {
                            Nav::Up => {
                                if let Some(prev) = segs.iter().rev().find(|s| s.end < a) {
                                    open = Some((segs.iter().position(|s| s.start == prev.start).unwrap(), true));
                                }
                            }
                            Nav::Down => {
                                let end = tab.live.buf_end;
                                if let Some(next) = segs.iter().find(|s| s.start > end) {
                                    open = Some((segs.iter().position(|s| s.start == next.start).unwrap(), false));
                                }
                            }
                            Nav::Close => tab.live.close(),
                        }
                    }
                }
                continue;
            }
            if seg.start > buf_end && !drawn_editor {
                drawn_editor = true;
                if let Some(Nav::Close) = editor_block(ui, tab, id, st) {
                    tab.live.close();
                }
            }
        }
        // the same rhythm as the reading view: more room above headings
        if i > 0 {
            let gap = match seg.doc.blocks.first() {
                Some(crate::markdown::Block::Heading { level, .. }) => size * if *level <= 2 { 1.5 } else { 1.1 },
                _ => size * 0.7,
            };
            ui.add_space(gap);
        }
        let top = ui.cursor().min;
        let before = actions.len();
        if seg.frontmatter {
            properties(ui, &seg.text, host);
        } else {
            let mut r = Renderer::new(host);
            r.blocks(ui, &seg.doc.blocks);
            for a in r.actions.drain(..) {
                actions.push(match a {
                    Action::ToggleTask(line, c) => Action::ToggleTask(line + seg.start, c),
                    other => other,
                });
            }
        }
        let rect = egui::Rect::from_min_max(top, egui::pos2(top.x + ui.available_width(), ui.cursor().min.y));
        let clicked = ui.input(|inp| inp.pointer.primary_clicked() && inp.pointer.interact_pos().is_some_and(|p| rect.contains(p)));
        if clicked && actions.len() == before {
            open = Some((i, true));
        }
        if ui.rect_contains_pointer(rect) && actions.len() == before {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
    }
    if active.is_some() && !drawn_editor {
        if let Some(Nav::Close) = editor_block(ui, tab, id, st) {
            tab.live.close();
        }
    }
    // the space below the note: continue writing at the end
    let below = ui.clip_rect().bottom() - ui.cursor().top();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), below.clamp(160.0, 4000.0)), egui::Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    let _ = rect;
    if resp.clicked() {
        if !tab.text.ends_with("\n\n") {
            if !tab.text.ends_with('\n') && !tab.text.is_empty() {
                tab.text.push('\n');
            }
            tab.text.push('\n');
        }
        let last = tab.text.split('\n').count() - 1;
        tab.live.active = Some(last);
        tab.live.buf = String::new();
        tab.live.buf_end = last;
        tab.live.cursor_at = Some(0);
        tab.last_edit = Some(std::time::Instant::now());
    }
    if let Some((i, at_end)) = open {
        if let Some(seg) = tab.live.segs.get(i).cloned() {
            tab.live.open(&seg, at_end);
        }
    }
    actions
}

enum Nav {
    Up,
    Down,
    Close,
}

fn editor_block(ui: &mut egui::Ui, tab: &mut Tab, id: egui::Id, st: &editor::Styles) -> Option<Nav> {
    ui.add_space(st.size * 0.45);
    let mut nav = None;
    if let Some(ci) = tab.live.cursor_at.take() {
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(ci))));
        state.store(ui.ctx(), id);
        ui.ctx().memory_mut(|m| m.request_focus(id));
    }
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    if focused {
        let cursor = egui::TextEdit::load_state(ui.ctx(), id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index).unwrap_or(0);
        let b = editor::char_to_byte(&tab.live.buf, cursor);
        let on_first = !tab.live.buf[..b].contains('\n');
        let on_last = !tab.live.buf[b..].contains('\n');
        if on_first && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            nav = Some(Nav::Up);
        } else if on_last && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            nav = Some(Nav::Down);
        } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            nav = Some(Nav::Close);
        }
    }
    let before = tab.live.buf.clone();
    let p = st.p;
    egui::Frame::new().fill(p.bg_secondary).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
        ui.add(editor::editor_widget(&mut tab.live.buf, id, &mut tab.live.highlight, st, 1));
    });
    if tab.live.buf != before {
        tab.text = tab.live.splice(&tab.text);
        tab.last_edit = Some(std::time::Instant::now());
    }
    if let Some(state) = egui::TextEdit::load_state(ui.ctx(), id) {
        if let Some(r) = state.cursor.char_range() {
            // cursor in note coordinates, for Templater insertions
            let line_start: usize = tab.text.split_inclusive('\n').take(tab.live.active.unwrap_or(0)).map(|l| l.chars().count()).sum();
            tab.cursor = line_start + r.primary.index;
        }
    }
    nav
}
