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
    /// Where the block was clicked to open it: the cursor goes there.
    pub click: Option<egui::Pos2>,
    /// In the source editor: the query block (by its order) shown as code.
    pub query_edit: Option<usize>,
    query_focus: bool,
    query_hl: Vec<editor::HighlightCache>,
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
        self.click = None;
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

/// The `dataview` / `dataviewjs` fenced blocks of a note, as byte ranges
/// from the opening fence to the end of the closing one's line.
pub fn query_blocks(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    // the open fence: its character, length, start, and whether it's a query
    let mut open: Option<(char, usize, usize, bool)> = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let indent = body.len() - body.trim_start_matches(' ').len();
        let t = body.trim_start_matches(' ');
        let fence = t.chars().next().filter(|c| (*c == '`' || *c == '~') && indent <= 3).map(|c| (c, t.chars().take_while(|x| *x == c).count())).filter(|f| f.1 >= 3);
        match (open, fence) {
            (None, Some((c, n))) => {
                let lang = t[n..].trim().split_whitespace().next().unwrap_or("").to_lowercase();
                open = Some((c, n, at, lang == "dataview" || lang == "dataviewjs"));
            }
            (Some((c, n, start, query)), Some((c2, n2))) if c2 == c && n2 >= n && t[n2..].trim().is_empty() => {
                if query {
                    out.push(start..at + line.len());
                }
                open = None;
            }
            _ => {}
        }
        at += line.len();
    }
    out
}

/// The source editor of a note with Dataview blocks: the text around them is
/// edited as source, and each query shows its result (click its corner to
/// edit the query; Escape shows the result again).
pub fn source_with_queries(ui: &mut egui::Ui, tab: &mut Tab, host: &mut dyn Host, st: &editor::Styles, blocks: &[std::ops::Range<usize>]) -> Vec<Action> {
    let mut actions = Vec::new();
    let p = st.p;
    // the pieces: text, query, text, query… (text pieces may be empty)
    let mut pieces: Vec<(bool, String)> = Vec::new();
    let mut at = 0;
    for b in blocks {
        pieces.push((false, tab.text[at..b.start].to_string()));
        pieces.push((true, tab.text[b.clone()].to_string()));
        at = b.end;
    }
    pieces.push((false, tab.text[at..].to_string()));
    let n_texts = pieces.iter().filter(|x| !x.0).count() + blocks.len();
    if tab.live.query_hl.len() < n_texts {
        tab.live.query_hl.resize_with(n_texts, Default::default);
    }
    if tab.live.query_edit.is_some_and(|q| q >= blocks.len()) {
        tab.live.query_edit = None;
    }
    let base = egui::Id::new(("source-pieces", &tab.path));
    let mut changed = false;
    let mut q = 0;
    let n_pieces = pieces.len();
    for (i, (query, text)) in pieces.iter_mut().enumerate() {
        let id = base.with(i);
        if !*query {
            // an empty piece between two queries still takes the cursor
            if text.is_empty() && i > 0 && i + 1 < n_pieces {
                continue;
            }
            let r = ui.add(editor::editor_widget(text, id, &mut tab.live.query_hl[i], st, 1));
            changed |= r.changed();
            continue;
        }
        let k = q;
        q += 1;
        if tab.live.query_edit == Some(k) {
            let r = ui.add(editor::editor_widget(text, id, &mut tab.live.query_hl[i], st, 1));
            changed |= r.changed();
            if std::mem::take(&mut tab.live.query_focus) {
                r.request_focus();
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) || (r.lost_focus() && !r.has_focus()) {
                tab.live.query_edit = None;
            }
            continue;
        }
        // the result, with a quiet way back to the query
        let top = ui.cursor().min;
        ui.add_space(4.0);
        let doc = markdown::parse(text);
        let mut r = Renderer::new(&mut *host);
        r.blocks(ui, &doc.blocks);
        actions.append(&mut r.actions);
        ui.add_space(4.0);
        let rect = egui::Rect::from_min_max(top, egui::pos2(top.x + ui.available_width(), ui.cursor().min.y));
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Outside);
            let br = egui::Rect::from_min_size(egui::pos2(rect.right() - 30.0, rect.top() + 2.0), egui::vec2(26.0, 24.0));
            let b = ui.interact(br, id.with("edit"), egui::Sense::click());
            ui.painter().rect_filled(br, 5.0, if b.hovered() { p.bg_active } else { p.bg_hover });
            ui.painter().text(br.center(), egui::Align2::CENTER_CENTER, crate::icons::CODE, crate::kit::icon_font(13.0), p.muted);
            if b.on_hover_text("Editar a consulta").clicked() {
                tab.live.query_edit = Some(k);
                tab.live.query_focus = true;
            }
        }
    }
    if changed {
        tab.text = pieces.into_iter().map(|x| x.1).collect();
        tab.last_edit = Some(std::time::Instant::now());
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_blocks_are_found() {
        let t = "a\n```dataviewjs\ndv.paragraph(1)\n```\nb\n~~~js\n```dataview\n~~~\n```dataview\nLIST\n```";
        let b = query_blocks(t);
        assert_eq!(b.len(), 2, "a fence inside another code block is not a query");
        assert_eq!(&t[b[0].clone()], "```dataviewjs\ndv.paragraph(1)\n```\n");
        assert_eq!(&t[b[1].clone()], "```dataview\nLIST\n```");
    }

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

/// A change to one frontmatter key: a new value, or `None` to remove it.
type PropEdit = (String, Option<serde_json::Value>);

/// The frontmatter as editable properties: a check box, a number, text, or
/// a list of chips, by what each value is; plus adding and removing keys.
fn properties(ui: &mut egui::Ui, text: &str, host: &dyn Host) -> Option<PropEdit> {
    use serde_json::Value as J;
    let p = host.palette();
    let yaml = text.trim_start_matches("---").trim_end().trim_end_matches("---");
    let data = syntax::parse_yaml(yaml).unwrap_or(J::Null);
    let empty = serde_json::Map::new();
    let o = data.as_object().unwrap_or(&empty);
    let mut edit: Option<PropEdit> = None;
    let fs = 13.0;
    egui::Frame::new().fill(p.bg_secondary).corner_radius(8.0).inner_margin(egui::Margin { left: 14, right: 10, top: 10, bottom: 8 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        for (k, v) in o {
            ui.horizontal_top(|ui| {
                let (lr, _) = ui.allocate_exact_size(egui::vec2(120.0, 20.0), egui::Sense::hover());
                ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, k, egui::FontId::proportional(fs), p.faint);
                let id = egui::Id::new(("prop", k.as_str()));
                let value_w = (ui.available_width() - 24.0).max(60.0);
                ui.allocate_ui_with_layout(egui::vec2(value_w, 20.0), egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                ui.set_width(value_w);
                match v {
                    J::Bool(b) => {
                        let mut on = *b;
                        if ui.checkbox(&mut on, "").changed() {
                            edit = Some((k.clone(), Some(J::Bool(on))));
                        }
                    }
                    J::Number(n) => {
                        let mut x = n.as_f64().unwrap_or(0.0);
                        let int = x.fract() == 0.0;
                        if ui.add(egui::DragValue::new(&mut x).speed(if int { 1.0 } else { 0.1 }).max_decimals(if int { 0 } else { 6 })).changed() {
                            let v = if x.fract() == 0.0 && x.abs() < 1e15 { J::from(x as i64) } else { J::from(x) };
                            edit = Some((k.clone(), Some(v)));
                        }
                    }
                    J::Array(a) => {
                        {
                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                            for (i, item) in a.iter().enumerate() {
                                let label = item.as_str().map(str::to_string).unwrap_or_else(|| item.to_string());
                                egui::Frame::new().fill(p.bg_hover).corner_radius(10.0).inner_margin(egui::Margin { left: 8, right: 4, top: 1, bottom: 1 }).show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    ui.label(egui::RichText::new(&label).size(12.5).color(p.text));
                                    if ui.add(egui::Label::new(egui::RichText::new("×").size(12.5).color(p.faint)).sense(egui::Sense::click())).on_hover_text("Tirar").clicked() {
                                        let mut na = a.clone();
                                        na.remove(i);
                                        edit = Some((k.clone(), Some(J::Array(na))));
                                    }
                                });
                            }
                            // a new item: typed, then Enter
                            let mut draft: String = ui.data(|d| d.get_temp(id.with("draft"))).unwrap_or_default();
                            let r = ui.add(egui::TextEdit::singleline(&mut draft).hint_text("+").desired_width(70.0).frame(false).font(egui::FontId::proportional(12.5)));
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !draft.trim().is_empty() {
                                let mut na = a.clone();
                                na.push(J::String(draft.trim().to_string()));
                                edit = Some((k.clone(), Some(J::Array(na))));
                                draft.clear();
                                r.request_focus();
                            }
                            ui.data_mut(|d| d.insert_temp(id.with("draft"), draft));
                        }
                    }
                    other => {
                        let mut s = match other {
                            J::String(s) => s.clone(),
                            J::Null => String::new(),
                            o => o.to_string(),
                        };
                        let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(f32::INFINITY).frame(false).hint_text("vazio").font(egui::FontId::proportional(fs)));
                        if r.changed() {
                            edit = Some((k.clone(), Some(J::String(s))));
                        }
                    }
                }
                });
                if ui.add(egui::Label::new(egui::RichText::new("×").size(fs).color(p.faint)).sense(egui::Sense::click())).on_hover_text("Remover a propriedade").clicked() {
                    edit = Some((k.clone(), None));
                }
            });
        }
        // a new property: its name, Enter
        let nid = egui::Id::new(("prop-new", text.len() / 1_000_000));
        let mut name: String = ui.data(|d| d.get_temp(nid)).unwrap_or_default();
        let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text("+ Adicionar propriedade").desired_width(200.0).frame(false).font(egui::FontId::proportional(12.5)));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty() && !o.contains_key(name.trim()) {
            edit = Some((name.trim().to_string(), Some(J::String(String::new()))));
            name.clear();
        }
        ui.data_mut(|d| d.insert_temp(nid, name));
    });
    ui.add_space(host.size() * 0.6);
    edit
}

/// Draw the note in Live Preview; returns the actions of clicked links and tasks.
pub fn show(ui: &mut egui::Ui, tab: &mut Tab, host: &mut dyn Host, st: &editor::Styles) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut prop_edit: Option<PropEdit> = None;
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
    // the same rhythm as the reading view: more room above headings
    let gap_before = |seg: &Segment| match seg.doc.blocks.first() {
        Some(crate::markdown::Block::Heading { level, .. }) => size * if *level <= 2 { 1.5 } else { 1.1 },
        _ => size * crate::typo::get().gap,
    };
    for (i, seg) in segs.iter().enumerate() {
        if let Some(a) = active {
            if seg.start >= a && seg.start <= buf_end || (seg.start < a && seg.end >= a) {
                if !drawn_editor {
                    drawn_editor = true;
                    if i > 0 {
                        ui.add_space(gap_before(seg));
                    }
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
                if i > 0 {
                    ui.add_space(size * 0.7);
                }
                if let Some(Nav::Close) = editor_block(ui, tab, id, st) {
                    tab.live.close();
                }
            }
        }
        if i > 0 {
            ui.add_space(gap_before(seg));
        }
        let top = ui.cursor().min;
        let before = actions.len();
        let dim = active.is_some() && crate::sc::focus::DIM.with(|d| d.get());
        if seg.frontmatter {
            if !crate::sc::focus::ON.with(|f| f.get()) {
                prop_edit = properties(ui, &seg.text, host);
            }
        } else {
            let mut r = Renderer::new(host);
            if dim {
                ui.scope(|ui| {
                    ui.multiply_opacity(0.28);
                    r.blocks(ui, &seg.doc.blocks);
                });
            } else {
                r.blocks(ui, &seg.doc.blocks);
            }
            for a in r.actions.drain(..) {
                actions.push(match a {
                    Action::ToggleTask(line, c) => Action::ToggleTask(line + seg.start, c),
                    other => other,
                });
            }
        }
        let rect = egui::Rect::from_min_max(top, egui::pos2(top.x + ui.available_width(), ui.cursor().min.y));
        let clicked = ui.input(|inp| inp.pointer.primary_clicked() && inp.pointer.interact_pos().is_some_and(|p| rect.contains(p)));
        // the properties are edited with their own widgets, not as YAML
        if clicked && actions.len() == before && !seg.frontmatter {
            open = Some((i, true));
            tab.live.click = ui.input(|inp| inp.pointer.interact_pos());
        }
        if ui.rect_contains_pointer(rect) && actions.len() == before && !seg.frontmatter {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
    }
    if active.is_some() && !drawn_editor {
        if !segs.is_empty() {
            ui.add_space(size * 0.7);
        }
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
    // a property edited: the frontmatter key changes, the rest stays as written
    if let Some((key, value)) = prop_edit {
        tab.text = ateliet_core::scriptorium::set_frontmatter(&tab.text, &key, value.as_ref());
        tab.last_edit = Some(std::time::Instant::now());
    }
    actions
}

enum Nav {
    Up,
    Down,
    Close,
}

fn editor_block(ui: &mut egui::Ui, tab: &mut Tab, id: egui::Id, st: &editor::Styles) -> Option<Nav> {
    let mut nav = None;
    if let Some(pos) = tab.live.click.take() {
        // the block opened where it was clicked: the text is set as it was
        // drawn (marks hidden), so the click lands on the same letter
        let g = editor::galley(ui, &tab.live.buf, &mut tab.live.highlight, st, editor::Reveal(None), ui.available_width());
        tab.live.cursor_at = Some(g.cursor_from_pos(pos - ui.cursor().min).ccursor.index);
    }
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
    let resp = ui.add(editor::editor_widget(&mut tab.live.buf, id, &mut tab.live.highlight, st, 1));
    if tab.live.buf != before && crate::sc::focus::TYPEWRITER.with(|t| t.get()) {
        // keep the line being typed in the middle of the screen
        let cursor = egui::TextEdit::load_state(ui.ctx(), id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index).unwrap_or(0);
        let lines = tab.live.buf.split('\n').count().max(1) as f32;
        let at = tab.live.buf.chars().take(cursor).filter(|c| *c == '\n').count() as f32;
        let r = resp.rect;
        let y = r.top() + r.height() * ((at + 0.5) / lines);
        ui.scroll_to_rect(egui::Rect::from_center_size(egui::pos2(r.center().x, y), egui::vec2(10.0, st.size)), Some(egui::Align::Center));
    }
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
