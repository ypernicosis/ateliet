//! Live Preview renders the resting note as blocks. Editing uses one buffer
//! for the entire document, so selections, deletion and undo cross paragraphs.
//! Escape returns to the fully rendered note, including embeds and queries.

use crate::editor;
use crate::markdown::{self, Doc};
use ateliet_core::syntax;
use ateliet_core::text::LineIndex;
use pulldown_cmark::{Event, Parser};
use std::sync::Arc;

#[derive(Clone)]
pub struct Segment {
    /// First and last line (inclusive).
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub doc: Arc<Doc>,
    pub frontmatter: bool,
    /// A hash of its text, naming its drawn height.
    pub hash: u64,
    /// It has controls of its own (links, tasks, callouts, queries): a
    /// simple click on it does not open the editor.
    pub interactive: bool,
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
            let hash = editor::text_hash(&t) ^ fm as u64;
            let interactive = ["[[", "](", "[ ]", "[x]", "[X]", "[!", "```dataview"].iter().any(|m| t.contains(m));
            Segment { start: s, end: e, text: t, doc, frontmatter: fm, hash, interactive }
        })
        .collect()
}

/// The segments of a note, recomputed when its text changes.
#[derive(Default)]
pub struct LiveState {
    key: u64,
    pub segs: Vec<Segment>,
    /// An editing session owns the whole note (always starts at line zero).
    pub active: Option<usize>,
    pub buf: String,
    pub buf_end: usize,
    /// Where to put the cursor when a block opens (char index), or the end.
    pub cursor_at: Option<usize>,
    /// In the source editor: the query block (by its order) shown as code.
    pub query_edit: Option<usize>,
    query_focus: bool,
    query_hl: Vec<editor::HighlightCache>,
    /// The blocks as drawn this frame, top to bottom: (top, bottom, first
    /// line, last line). Where a dragged picture would land.
    pub spots: Vec<(f32, f32, usize, usize)>,
    /// Last rendered text hit targets, retained for press-and-drag activation.
    edit_targets: Vec<(egui::Rect, usize)>,
    /// A press begun on rendered text must keep its anchor through editor activation.
    drag_anchor: Option<usize>,
    /// The drawn heights of the segments, to draw only those in view.
    pub heights: crate::heights::Heights,
}

impl LiveState {
    pub fn refresh(&mut self, text: &str) {
        let k = editor::text_hash(text);
        if k != self.key || self.segs.is_empty() {
            self.segs = segments(text);
            self.key = k;
        }
    }

    pub fn open_document(&mut self, text: &str, cursor: usize) {
        self.active = Some(0);
        self.buf = text.to_string();
        self.buf_end = text.split('\n').count().saturating_sub(1);
        self.cursor_at = Some(cursor.min(text.chars().count()));
        self.drag_anchor = None;
    }

    /// The line a picture dropped at height `y` goes before, and the height
    /// to mark it at; None when the note was not drawn in Live Preview.
    pub fn drop_line(&self, y: f32) -> Option<(usize, f32)> {
        let mut prev_bottom = None;
        for &(top, bottom, start, _) in &self.spots {
            if y < (top + bottom) / 2.0 {
                let mark = prev_bottom.map(|b: f32| (b + top) / 2.0).unwrap_or(top - 4.0);
                return Some((start, mark));
            }
            prev_bottom = Some(bottom);
        }
        self.spots.last().map(|&(_, bottom, _, end)| (end + 1, bottom + 4.0))
    }

    pub fn close(&mut self) {
        self.active = None;
        self.drag_anchor = None;
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
    let base = egui::Id::new(("source-pieces", tab.uid, &tab.path));
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
    fn blocks_and_document_editing() {
        let t = "---\na: 1\n---\n# Title\n\nPara one\nstill one\n\n- a\n- b\n\n```js\nx\n```\n";
        let s = segments(t);
        let spans: Vec<(usize, usize)> = s.iter().map(|x| (x.start, x.end)).collect();
        assert_eq!(spans, vec![(0, 2), (3, 3), (5, 6), (8, 9), (11, 13)]);
        let mut l = LiveState::default();
        l.open_document(t, t.chars().count());
        assert_eq!(l.active, Some(0));
        assert_eq!(l.buf, t);
        assert_eq!(l.buf_end, t.split('\n').count() - 1);
        assert_eq!(l.cursor_at, Some(t.chars().count()));
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

/// Resting notes keep their rich rendering; an editing session spans the whole note.
pub fn show(ui: &mut egui::Ui, tab: &mut Tab, host: &mut dyn Host, st: &editor::Styles) -> Vec<Action> {
    let id = tab.editor_id();
    let press = ui.input(|i| i.pointer.primary_pressed() && !i.modifiers.command);
    let at = ui.input(|i| i.pointer.interact_pos());
    let target = at.and_then(|at| tab.live.edit_targets.iter().find(|(rect, _)| rect.contains(at))).copied();
    if tab.live.active.is_none() && press && !ui.memory(|m| m.any_popup_open()) {
        if let Some((rect, line)) = target {
            let cursor = if let Some(seg) = tab.live.segs.iter().find(|s| s.start == line) {
                let offset: usize = tab.text.split_inclusive('\n').take(line).map(|l| l.chars().count()).sum();
                let galley = editor::galley(ui, &seg.text, &mut tab.highlight, st, editor::Reveal(None), rect.width());
                offset + galley.cursor_from_pos(at.unwrap() - rect.min).ccursor.index
            } else { tab.text.chars().count() };
            tab.live.open_document(&tab.text, cursor);
            tab.live.drag_anchor = Some(cursor);
        }
    }
    if tab.live.active.is_none() && (tab.text.trim().is_empty() || tab.find.pending.is_some() || tab.scroll_to.is_some()) {
        tab.live.open_document(&tab.text, tab.cursor);
    }
    if tab.live.active.is_some() {
        if ui.ctx().memory(|m| m.has_focus(id))
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            tab.live.close();
            ui.ctx().memory_mut(|m| m.surrender_focus(id));
        } else {
            document_editor(ui, tab, st);
            return Vec::new();
        }
    }

    let mut actions = Vec::new();
    let mut prop_edit = None;
    tab.live.refresh(&tab.text);
    // out of the tab while drawing (not copied: a long note has thousands)
    let segs = std::mem::take(&mut tab.live.segs);
    let mut heights = std::mem::take(&mut tab.live.heights);
    let mut pass = heights.pass(ui);
    tab.live.edit_targets.clear();
    let size = host.size();
    let width = ui.available_width();
    // the pointer, asked once: thousands of segments are laid out each frame
    let click = ui.input(|i| i.pointer.primary_clicked().then(|| i.pointer.interact_pos()).flatten());
    let hover = ui.input(|i| i.pointer.hover_pos());
    for (i, seg) in segs.iter().enumerate() {
        if i > 0 {
            let gap = match seg.doc.blocks.first() {
                Some(crate::markdown::Block::Heading { level, .. }) => size * if *level <= 2 { 1.5 } else { 1.1 },
                _ => size * crate::typo::get().gap,
            };
            ui.add_space(gap);
        }
        let before = actions.len();
        let rect = pass.piece(ui, seg.hash, || crate::heights::estimate(&seg.text, size, width), false, |ui| {
            if seg.frontmatter {
                if !crate::sc::focus::ON.with(|f| f.get()) {
                    if let Some(edit) = properties(ui, &seg.text, host) {
                        prop_edit = Some(edit);
                    }
                }
            } else {
                let mut renderer = Renderer::new(host);
                renderer.blocks(ui, &seg.doc.blocks);
                for action in renderer.actions.drain(..) {
                    actions.push(match action {
                        Action::ToggleTask(line, checked) => Action::ToggleTask(line + seg.start, checked),
                        other => other,
                    });
                }
            }
        });
        tab.live.spots.push((rect.top(), rect.bottom(), seg.start, seg.end));
        if !seg.frontmatter {
            if click.is_some_and(|p| rect.contains(p))
                && !tab.live.edit_targets.iter().any(|(r, _)| *r == rect)
                && actions.len() == before {
                let offset = tab.text.split_inclusive('\n').take(seg.start).map(|l| l.chars().count()).sum();
                tab.live.open_document(&tab.text, offset);
                ui.ctx().request_repaint();
            }
            // Interactive blocks keep their own controls on a simple click.
            let interactive = seg.interactive;
            if !interactive { tab.live.edit_targets.push((rect, seg.start)); }
            if !interactive && hover.is_some_and(|p| rect.contains(p)) && ui.rect_contains_pointer(rect) && actions.len() == before {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
        }
    }
    pass.finish(ui);
    tab.live.segs = segs;
    tab.live.heights = heights;
    let below = (ui.clip_rect().bottom() - ui.cursor().top()).clamp(160.0, 4000.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), below), egui::Sense::click());
    tab.live.edit_targets.push((rect, usize::MAX));
    if resp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Text); }
    if resp.clicked() {
        tab.live.open_document(&tab.text, tab.text.chars().count());
        ui.ctx().request_repaint();
    }
    if let Some((key, value)) = prop_edit {
        tab.text = ateliet_core::scriptorium::set_frontmatter(&tab.text, &key, value.as_ref());
        tab.last_edit = Some(std::time::Instant::now());
    }
    actions
}

/// One editor owns all characters, including newlines between rendered blocks.
fn document_editor(ui: &mut egui::Ui, tab: &mut Tab, st: &editor::Styles) {
    let id = tab.editor_id();
    if let Some(ci) = tab.live.cursor_at.take() {
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(ci))));
        state.store(ui.ctx(), id);
        ui.ctx().memory_mut(|m| m.request_focus(id));
    }
    if let Some(line) = tab.scroll_to.take() {
        let byte = tab.text.split_inclusive('\n').take(line).map(str::len).sum::<usize>();
        crate::find::request_scroll(ui.ctx(), id, byte..byte);
    }
    if let Some(range) = tab.find.pending.take() {
        crate::find::request_scroll(ui.ctx(), id, range);
    }
    let response = ui.add(editor::editor_widget(&mut tab.text, id, &mut tab.highlight, st, 1));
    let range = egui::TextEdit::load_state(ui.ctx(), id).and_then(|s| s.cursor.char_range());
    if let Some(range) = range {
        tab.cursor = range.primary.index;
    }
    if response.changed() {
        tab.last_edit = Some(std::time::Instant::now());
        ui.ctx().request_repaint();
        if crate::sc::focus::TYPEWRITER.with(|t| t.get()) {
            let (galley, origin) = tab.highlight.rendered.as_ref().unwrap();
            let cursor = galley.from_ccursor(egui::text::CCursor::new(tab.cursor));
            ui.scroll_to_rect(galley.pos_from_cursor(&cursor).translate(origin.to_vec2()), Some(egui::Align::Center));
        }
    }
    // Keep integrations in note coordinates; never splice an old block back into it.
    tab.live.active = Some(0);
    if response.changed() {
        tab.live.buf.clone_from(&tab.text);
        tab.live.buf_end = tab.text.split('\n').count().saturating_sub(1);
    }
    // Reuse the exact geometry TextEdit already laid out, at its actual width.
    let (galley, origin) = tab.highlight.rendered.as_ref().unwrap();
    if let Some(anchor) = tab.live.drag_anchor {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            let cursor = galley.cursor_from_pos(pos - *origin).ccursor;
            let primary = if ui.input(|i| i.pointer.primary_pressed()) { egui::text::CCursor::new(anchor) } else { cursor };
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            let next = egui::text::CCursorRange::two(egui::text::CCursor::new(anchor), primary);
            let moved = state.cursor.char_range() != Some(next);
            state.cursor.set_char_range(Some(next));
            state.store(ui.ctx(), id);
            ui.ctx().memory_mut(|m| m.request_focus(id));
            tab.cursor = primary.index;
            if moved { ui.ctx().request_repaint(); }
            if pos.y < ui.clip_rect().top() || pos.y > ui.clip_rect().bottom() {
                ui.scroll_to_rect(galley.pos_from_cursor(&galley.from_ccursor(primary)).translate(origin.to_vec2()), None);
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) { tab.live.drag_anchor = None; }
    }
    let mut line = 0;
    for row in &galley.rows {
        if origin.y + row.min_y() > ui.clip_rect().bottom() { break; }
        if origin.y + row.max_y() < ui.clip_rect().top() {
            if row.ends_with_newline { line += 1; }
            continue;
        }
        tab.live.spots.push((origin.y + row.min_y(), origin.y + row.max_y(), line, line));
        if row.ends_with_newline { line += 1; }
    }
}

#[cfg(test)]
mod editing_tests {
    use super::*;
    use crate::app::Mode;
    use egui::{Event, Key, Modifiers, PointerButton, Pos2};

    struct TestHost;
    impl Host for TestHost {
        fn palette(&self) -> crate::theme::Palette { crate::theme::Palette::new(&crate::config::Appearance::default()) }
        fn size(&self) -> f32 { 16.0 }
        fn source(&self) -> &str { "note.md" }
        fn resolved(&self, _: &str) -> bool { true }
        fn image(&mut self, _: &egui::Context, _: &str) -> Option<(egui::TextureId, egui::Vec2)> { None }
        fn query(&mut self, _: &str, _: &str) -> serde_json::Value { serde_json::Value::Null }
        fn inline_query(&mut self, _: bool, _: &str) -> serde_json::Value { serde_json::Value::Null }
        fn embed(&mut self, _: &str) -> Option<(String, Arc<Doc>)> { None }
    }

    fn note(text: &str) -> Tab {
        Tab {
            uid: crate::app::next_uid(), sync: 0, path: "note.md".into(),
            text: text.into(), saved: text.into(), mode: Mode::Edit, doc: None,
            last_edit: None, cursor: 0, highlight: Default::default(), find: Default::default(),
            scroll_to: None, live: Default::default(), board: None, quadro: None,
            view: None, reader: None, back: Vec::new(), fwd: Vec::new(),
        }
    }

    fn frame(ctx: &egui::Context, tab: &mut Tab, events: Vec<Event>, modifiers: Modifiers) -> egui::FullOutput {
        let busy = !events.is_empty();
        ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(700.0, 1000.0))),
            events, modifiers, ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                tab.live.spots.clear();
                let st = editor::Styles::live(16.0, TestHost.palette());
                // as the app does before the note's scroll area
                if tab.highlight.incomplete() {
                    let reveal = editor::reveal_of(ctx, tab.editor_id(), &tab.text);
                    ctx.fonts(|f| tab.highlight.advance(f, &tab.text, &st, reveal, busy));
                }
                show(ui, tab, &mut TestHost, &st);
            });
        })
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::fonts::definitions());
        // a blinking caret asks for frames by the clock, which a test cannot count on
        ctx.style_mut(|s| s.visuals.text_cursor.blink = false);
        ctx
    }

    #[test]
    fn editing_around_a_table_preserves_source_and_undo() {
        let ctx = context();
        let original = "Antes\n\n| Item | Definição |\n| --- | --- |\n| Texto | ação |\n\nDepois";
        let mut tab = note(original);
        tab.live.open_document(&tab.text, original.chars().count());
        let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
        let _ = frame(&ctx, &mut tab, vec![Event::Text("!".into())], Modifiers::NONE);
        assert_eq!(tab.text, format!("{original}!"));
        assert_eq!(tab.live.buf, tab.text);
        let command = Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ctrl: !cfg!(target_os = "macos"), ..Modifiers::NONE };
        let _ = frame(&ctx, &mut tab, vec![key(Key::Z, command)], command);
        assert_eq!(tab.text, original);
        // A click on a rendered cell must enter the source table without losing text.
        let (g, origin) = tab.highlight.rendered.as_ref().unwrap();
        let row = g.rows.iter().find(|r| r.rect.height() > 20.0 && r.min_y() > 40.0).unwrap();
        let at = *origin + egui::vec2(120.0, row.rect.center().y);
        let _ = frame(&ctx, &mut tab, vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], Modifiers::NONE);
        let _ = frame(&ctx, &mut tab, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
        assert_eq!(tab.text, original);
        let cursor = editor::char_to_byte(&tab.text, tab.cursor);
        let table = tab.text.find("| Item").unwrap()..tab.text.find("\n\nDepois").unwrap();
        assert!(table.contains(&cursor), "cursor {cursor} must enter {table:?}");
    }

    #[test]
    fn mouse_drag_selects_across_separate_rendered_paragraphs() {
        let ctx = context();
        let mut tab = note("Primeira linha.\n\nSegunda linha.\n\nTerceira linha.");
        for _ in 0..2 { let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE); }
        let start = egui::pos2(12.0, (tab.live.spots[0].0 + tab.live.spots[0].1) / 2.0);
        let _ = frame(&ctx, &mut tab, vec![
            Event::PointerMoved(start),
            Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        ], Modifiers::NONE);
        assert_eq!(tab.live.active, Some(0));
        assert_eq!(tab.live.buf, tab.text);
        let last = tab.live.spots.last().unwrap();
        let end = egui::pos2(180.0, (last.0 + last.1) / 2.0);
        let _ = frame(&ctx, &mut tab, vec![Event::PointerMoved(end)], Modifiers::NONE);
        let _ = frame(&ctx, &mut tab, vec![
            Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ], Modifiers::NONE);
        let (a, b) = editor::selection(&ctx, tab.editor_id()).unwrap();
        assert!(b > tab.text.find("Terceira").unwrap(), "selection {a}..{b} must cross paragraph boundaries");
        assert!(a < tab.text.find("Segunda").unwrap());
        let copied = frame(&ctx, &mut tab, vec![Event::Copy], Modifiers::NONE);
        assert!(copied.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t.contains("Segunda linha.") && t.contains("\n\n"))), "{:?}; selection={:?}, focused={:?}", copied.platform_output.commands, editor::selection(&ctx, tab.editor_id()), ctx.memory(|m| m.focused()));
    }

    #[test]
    fn deleting_selected_lines_shrinks_immediately_and_undo_restores_them() {
        let ctx = context();
        let original = "# Título\n\nUm parágrafo.\n\nOutro parágrafo.";
        let mut tab = note(original);
        tab.live.open_document(&tab.text, 0);
        let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
        let id = tab.editor_id();
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0), egui::text::CCursor::new(original.chars().count()),
        )));
        state.store(&ctx, id);
        let _ = frame(&ctx, &mut tab, vec![key(Key::Backspace, Modifiers::NONE)], Modifiers::NONE);
        assert_eq!(tab.text, "");
        assert_eq!(tab.live.buf, "");
        assert_eq!(tab.live.buf_end, 0);
        let erased_height = ctx.read_response(id).unwrap().rect.height();
        let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
        assert!((erased_height - ctx.read_response(id).unwrap().rect.height()).abs() < 1.0, "height must update without cursor movement");
        let command = Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ctrl: !cfg!(target_os = "macos"), ..Modifiers::NONE };
        let _ = frame(&ctx, &mut tab, vec![key(Key::Z, command)], command);
        assert_eq!(tab.text, original);
    }

    #[test]
    fn idle_editing_reuses_the_exact_textedit_layout_and_does_not_repaint_continuously() {
        let ctx = context();
        let mut tab = note(&"Um parágrafo de planejamento.\n\n".repeat(400));
        tab.live.open_document(&tab.text, 0);
        for _ in 0..4 { let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE); }
        // the lines below the view are laid out over the first frames
        for _ in 0..200 {
            if !tab.highlight.incomplete() { break; }
            let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
        }
        assert!(!tab.highlight.incomplete());
        let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
        let first = tab.highlight.rendered.as_ref().unwrap().0.clone();
        for _ in 0..5 {
            let out = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
            assert!(Arc::ptr_eq(&first, &tab.highlight.rendered.as_ref().unwrap().0));
            assert!(out.viewport_output[&egui::ViewportId::ROOT].repaint_delay > std::time::Duration::from_millis(20), "{:?}", ctx.repaint_causes());
        }
    }

    /// The layout the editor shows must be the one a fresh layout of its text makes.
    fn assert_fresh(ctx: &egui::Context, tab: &mut Tab, what: &str) {
        // the lines left for later are laid out first
        for _ in 0..2000 {
            if !tab.highlight.incomplete() {
                break;
            }
            let _ = frame(ctx, tab, vec![], Modifiers::NONE);
        }
        assert!(!tab.highlight.incomplete(), "{what}: the layout never completed");
        let (g, _) = tab.highlight.rendered.as_ref().unwrap();
        let reveal = editor::selection(ctx, tab.editor_id()).map(|(a, b)| (editor::char_to_byte(&tab.text, a), editor::char_to_byte(&tab.text, b)));
        let st = editor::Styles::live(16.0, TestHost.palette());
        // the context's fonts: their atlas places the glyphs the editor's galley uses
        let (fresh, _) = ctx.fonts(|f| editor::HighlightCache::default().live(f, &tab.text, &st, editor::Reveal(reveal), g.rect.width()));
        editor::assert_same_layout(g, &fresh, what);
    }

    #[test]
    fn typing_and_selecting_in_a_long_note_rework_its_layout_without_copying_it() {
        let ctx = context();
        let original = "# Capítulo\n\nUm parágrafo com **ênfase** e [[ligação]].\n\n- um item\n\n".repeat(60);
        let mut tab = note(&original);
        let start = original.find("ênfase").unwrap();
        tab.live.open_document(&tab.text, original[..start].chars().count());
        for _ in 0..2 { let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE); }
        assert_fresh(&ctx, &mut tab, "opened");
        for (what, events) in [
            ("a letter", vec![Event::Text("x".into())]),
            ("another", vec![Event::Text("y".into())]),
            ("a new line", vec![key(Key::Enter, Modifiers::NONE)]),
            ("bold", vec![Event::Text("**".into())]),
            ("deleted", vec![key(Key::Backspace, Modifiers::NONE), key(Key::Backspace, Modifiers::NONE)]),
        ] {
            let _ = frame(&ctx, &mut tab, events, Modifiers::NONE);
            assert_fresh(&ctx, &mut tab, what);
        }
        // a selection grows over several rows: egui paints it into the layout it was lent
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        // (the cursor's span changes too, so the two layouts kept take turns)
        let mut seen = std::collections::HashSet::new();
        for _ in 0..6 {
            let _ = frame(&ctx, &mut tab, vec![key(Key::ArrowDown, shift)], shift);
            seen.insert(tab.highlight.rendered.as_ref().unwrap().0.rows.as_ptr() as usize);
        }
        assert!(seen.len() <= 2, "the layout must not be copied to paint a selection: {} copies", seen.len());
        let (a, b) = editor::selection(&ctx, tab.editor_id()).unwrap();
        assert!(b > a);
        // typing over the selection replaces it
        let _ = frame(&ctx, &mut tab, vec![Event::Text("z".into())], Modifiers::NONE);
        assert!(tab.text.contains("ênz") || tab.text.contains('z'));
        let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
        assert_fresh(&ctx, &mut tab, "after the selection was replaced");
    }

    #[test]
    #[ignore = "benchmark: cargo test --release -- --ignored --nocapture long_note_frames"]
    fn long_note_frames() {
        let ctx = context();
        let mut text = String::new();
        let mut n = 0;
        while text.len() < 2_000_000 {
            n += 1;
            text += &format!("## Capítulo {n}\n\nUma nota longa ({n}) com **ênfase**, [[referência {n}]] e texto corrido que planeja a história, com frases que quebram em várias linhas na largura do editor.\n\n- um item {n} com *itálico*\n- outro item {n}\n\n> uma citação curta {n}\n\nMais um parágrafo {n} com `código` e ==marca==.\n\n");
        }
        let mut tab = note(&text);
        let mid = text[text.len() / 2..].find("Uma nota").unwrap() + text.len() / 2 + 4;
        tab.live.open_document(&tab.text, text[..mid].chars().count());
        let time = |tab: &mut Tab, events: Vec<Event>, modifiers: Modifiers| {
            let t = std::time::Instant::now();
            let _ = frame(&ctx, tab, events, modifiers);
            t.elapsed()
        };
        eprintln!("{} bytes, first frame: {:?}", tab.text.len(), time(&mut tab, vec![], Modifiers::NONE));
        let _ = time(&mut tab, vec![], Modifiers::NONE);
        // as a person does, between opening the note and typing (the rest is laid out meanwhile)
        let mut frames = 0;
        let rest: std::time::Duration = std::iter::from_fn(|| (tab.highlight.incomplete() && frames < 500).then(|| { frames += 1; time(&mut tab, vec![], Modifiers::NONE) })).sum();
        eprintln!("the rest laid out while idle: {frames} frames, {rest:?}");
        let idle: std::time::Duration = (0..10).map(|_| time(&mut tab, vec![], Modifiers::NONE)).sum();
        eprintln!("idle frame: {:?}", idle / 10);
        let each: Vec<std::time::Duration> = "palavra".chars().map(|c| time(&mut tab, vec![Event::Text(c.into())], Modifiers::NONE) + time(&mut tab, vec![], Modifiers::NONE)).collect();
        eprintln!("a letter typed (its frame and the next): {:?} on average; the first {:?}, the others at most {:?}", each.iter().sum::<std::time::Duration>() / 7, each[0], each[1..].iter().max().unwrap());
        let moves: std::time::Duration = (0..10).map(|_| time(&mut tab, vec![key(Key::ArrowRight, Modifiers::NONE)], Modifiers::NONE) + time(&mut tab, vec![], Modifiers::NONE)).sum();
        eprintln!("cursor moved (its frame and the next): {:?}", moves / 10);
        if std::env::var("ATELIET_PROFILE").is_ok() { eprintln!("ENTER"); }
        let enter = time(&mut tab, vec![key(Key::Enter, Modifiers::NONE)], Modifiers::NONE) + time(&mut tab, vec![], Modifiers::NONE);
        eprintln!("a new line (its frame and the next): {enter:?}");
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        let select: std::time::Duration = (0..10).map(|_| time(&mut tab, vec![key(Key::ArrowDown, shift)], shift) + time(&mut tab, vec![], shift)).sum();
        eprintln!("selection grown a line (its frame and the next): {:?}", select / 10);
        let held: std::time::Duration = (0..10).map(|_| time(&mut tab, vec![], shift)).sum();
        eprintln!("idle frame with a selection: {:?}", held / 10);
        let t = std::time::Instant::now();
        let _ = crate::markdown::parse(&tab.text);
        eprintln!("parsing the note (the outline, on each change): {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = ateliet_core::scriptorium::count_words(&tab.text);
        eprintln!("counting its words (the status bar, on each change): {:?}", t.elapsed());
    }

    #[test]
    fn scrolling_far_into_lines_left_for_later_shows_them_laid_out() {
        let ctx = context();
        let text = "## Parte\n\nUm parágrafo com **ênfase** e texto corrido que quebra em mais de uma linha no editor de teste.\n\n".repeat(400);
        let mut tab = note(&text);
        tab.live.open_document(&tab.text, 0);
        let run = |tab: &mut Tab, offset: Option<f32>| {
            let mut clip = egui::Rect::NOTHING;
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 800.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let st = editor::Styles::live(16.0, TestHost.palette());
                    // busy: nothing is laid out for later in these frames
                    if tab.highlight.incomplete() {
                        let reveal = editor::reveal_of(ctx, tab.editor_id(), &tab.text);
                        ctx.fonts(|f| tab.highlight.advance(f, &tab.text, &st, reveal, true));
                    }
                    let mut area = egui::ScrollArea::vertical();
                    if let Some(y) = offset {
                        area = area.vertical_scroll_offset(y);
                    }
                    area.show(ui, |ui| {
                        clip = ui.clip_rect();
                        show(ui, tab, &mut TestHost, &st);
                    });
                });
            });
            clip
        };
        let _ = run(&mut tab, None);
        assert!(tab.highlight.incomplete(), "the lines below the view wait");
        // a jump far down, as dragging the scroll bar does
        let total = tab.highlight.rendered.as_ref().unwrap().0.rect.height();
        let clip = run(&mut tab, Some(total * 0.6));
        let (g, origin) = tab.highlight.rendered.as_ref().unwrap();
        let in_view: Vec<_> = g.rows.iter().filter(|r| r.rect.translate(origin.to_vec2()).intersects(clip)).collect();
        assert!(!in_view.is_empty());
        assert!(in_view.iter().all(|r| editor::drawn(r)), "every row in view is laid out and drawn");
    }

    #[test]
    #[ignore = "benchmark: cargo test --release -- --ignored --nocapture long_note_resized"]
    fn long_note_resized() {
        let ctx = context();
        let mut text = String::new();
        let mut n = 0;
        while text.len() < 2_000_000 {
            n += 1;
            text += &format!("## Capítulo {n}\n\nUma nota longa ({n}) com **ênfase**, [[referência {n}]] e texto corrido que planeja a história, com frases que quebram em várias linhas na largura do editor.\n\n- um item {n} com *itálico*\n- outro item {n}\n\n> uma citação curta {n}\n\nMais um parágrafo {n} com `código` e ==marca==.\n\n");
        }
        let mut tab = note(&text);
        let mid = text[text.len() / 2..].find("Uma nota").unwrap() + text.len() / 2 + 4;
        tab.live.open_document(&tab.text, text[..mid].chars().count());
        // `busy`: the window is being dragged (the app lays nothing out for later then)
        let run = |tab: &mut Tab, width: f32, busy: bool| {
            let t = std::time::Instant::now();
            let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 1000.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    tab.live.spots.clear();
                    let st = editor::Styles::live(16.0, TestHost.palette());
                    if tab.highlight.incomplete() {
                        let reveal = editor::reveal_of(ctx, tab.editor_id(), &tab.text);
                        ctx.fonts(|f| tab.highlight.advance(f, &tab.text, &st, reveal, busy));
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| { show(ui, tab, &mut TestHost, &st); });
                });
            });
            t.elapsed()
        };
        eprintln!("{} bytes, opened at the start: first frame {:?}", tab.text.len(), run(&mut tab, 700.0, false));
        let mut frames = 0;
        let mut worst = std::time::Duration::ZERO;
        while tab.highlight.incomplete() && frames < 2000 {
            worst = worst.max(run(&mut tab, 700.0, false));
            frames += 1;
        }
        eprintln!("completed in {frames} frames, the slowest {worst:?}");
        let mut worst = std::time::Duration::ZERO;
        let mut total = std::time::Duration::ZERO;
        for k in 0..30 {
            let d = run(&mut tab, 700.0 - 4.0 * k as f32, true);
            worst = worst.max(d);
            total += d;
        }
        eprintln!("window dragged narrower over 30 frames: {:?} a frame, the slowest {worst:?}", total / 30);
        // the pause after the drag, then what is left
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut frames = 0;
        let mut worst = std::time::Duration::ZERO;
        while tab.highlight.incomplete() && frames < 4000 {
            worst = worst.max(run(&mut tab, 584.0, false));
            frames += 1;
        }
        eprintln!("then completed in {frames} frames, the slowest {worst:?}");
    }

    #[test]
    #[ignore = "benchmark: cargo test --release -- --ignored --nocapture long_note_at_rest"]
    fn long_note_at_rest() {
        let ctx = context();
        let mut text = String::new();
        let mut n = 0;
        while text.len() < 2_000_000 {
            n += 1;
            text += &format!("## Capítulo {n}\n\nUma nota longa ({n}) com **ênfase**, [[referência {n}]] e texto corrido que planeja a história, com frases que quebram em várias linhas na largura do editor.\n\n- um item {n} com *itálico*\n- outro item {n}\n\n> uma citação curta {n}\n\nMais um parágrafo {n} com `código` e ==marca==.\n\n");
        }
        let mut tab = note(&text);
        let time = |tab: &mut Tab| {
            let t = std::time::Instant::now();
            let _ = frame(&ctx, tab, vec![], Modifiers::NONE);
            t.elapsed()
        };
        eprintln!("rendered, not edited: first frame {:?}", time(&mut tab));
        let idle: std::time::Duration = (0..5).map(|_| time(&mut tab)).sum();
        eprintln!("rendered, not edited: next frames {:?}", idle / 5);
        let t = std::time::Instant::now();
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        let st = editor::Styles::live(16.0, TestHost.palette());
        let _ = editor::highlight_lines(&text, &st, editor::Reveal(None));
        eprintln!("styling every line, no layout: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = editor::HighlightCache::default().live(&fonts, &text, &st, editor::Reveal(None), 640.0);
        eprintln!("styling and laying out every line: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = query_blocks(&text);
        eprintln!("query blocks: {:?}", t.elapsed());
    }

    #[test]
    fn shift_down_crosses_blank_lines_without_switching_editors() {
        let ctx = context();
        let mut tab = note("Um.\n\nDois.\n\nTrês.");
        tab.live.open_document(&tab.text, 0);
        let _ = frame(&ctx, &mut tab, vec![], Modifiers::NONE);
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        for _ in 0..4 { let _ = frame(&ctx, &mut tab, vec![key(Key::ArrowDown, shift)], shift); }
        let (a, b) = editor::selection(&ctx, tab.editor_id()).unwrap();
        assert_eq!(a, 0);
        assert!(b > tab.text.find("Dois").unwrap());
        assert_eq!(tab.live.active, Some(0));
    }
}
