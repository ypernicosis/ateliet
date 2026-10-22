//! The note editor: a text area with Markdown styled as you type (headings
//! larger, emphasis, code, links, tags, frontmatter and code blocks set apart).

use crate::theme::Palette;
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Stroke, TextFormat};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

static INLINE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?P<code>`[^`\n]+`)",
        r"|(?P<wiki>!?\[\[[^\]\n]+\]\])",
        r"|(?P<mdlink>!?\[[^\]\n]*\]\([^)\n]*\))",
        r"|(?P<url>https?://[^\s)>\]]+)",
        r"|(?P<bold>\*\*[^*\n]+\*\*|__[^_\n]+__)",
        r"|(?P<italic>\*[^*\s][^*\n]*\*|\b_[^_\s][^_\n]*_\b)",
        r"|(?P<strike>~~[^~\n]+~~)",
        r"|(?P<mark>==[^=\n]+==)",
        r"|(?P<field>\[[^\[\]\n]+?::[^\]\n]*\])",
        r"|(?P<tag>(?:^|\s)#[^\s#!-,.:-@\[-^`{-~]+)",
        r"|(?P<comment>%%[^%]*%%)",
    ))
    .unwrap()
});
static HEADING: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(#{1,6})\s").unwrap());
static LIST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\s*)([-*+]|\d+[.)])(\s+)(\[.\]\s)?").unwrap());

pub struct Styles {
    pub size: f32,
    pub p: Palette,
}

impl Styles {
    fn fmt(&self, size: f32, color: Color32) -> TextFormat {
        TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color, line_height: Some(size * 1.62), ..Default::default() }
    }
    fn mono(&self, color: Color32) -> TextFormat {
        TextFormat { font_id: FontId::new(self.size * 0.88, FontFamily::Monospace), color, line_height: Some(self.size * 1.5), ..Default::default() }
    }
}

/// Markdown punctuation (`**`, `[[`, `==`…): there, but quiet.
fn syntax(f: &TextFormat, p: &Palette) -> TextFormat {
    let mut m = f.clone();
    m.color = p.faint.gamma_multiply(0.85);
    m.background = Color32::TRANSPARENT;
    m.underline = Stroke::NONE;
    m.strikethrough = Stroke::NONE;
    m
}

/// Append `s` as opening marker + content + closing marker.
fn wrapped(job: &mut LayoutJob, s: &str, open: usize, close: usize, f: TextFormat, p: &Palette) {
    if s.len() < open + close {
        job.append(s, 0.0, f);
        return;
    }
    let m = syntax(&f, p);
    job.append(&s[..open], 0.0, m.clone());
    job.append(&s[open..s.len() - close], 0.0, f);
    job.append(&s[s.len() - close..], 0.0, m);
}

/// Build the styled layout for the whole note.
pub fn highlight(text: &str, st: &Styles) -> LayoutJob {
    let mut job = LayoutJob::default();
    let p = &st.p;
    let base = st.fmt(st.size, p.text);
    let mut in_fm = false;
    let mut in_code = false;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let body = line.trim_end_matches(['\n', '\r']);
        // frontmatter
        if n == 0 && body.trim_end() == "---" {
            in_fm = true;
            job.append(line, 0.0, st.mono(p.faint));
            continue;
        }
        if in_fm {
            if body.trim_end() == "---" {
                in_fm = false;
                job.append(line, 0.0, st.mono(p.faint));
                continue;
            }
            match body.find(':') {
                Some(k) if !body.starts_with(' ') && !body.starts_with('-') => {
                    job.append(&line[..k + 1], 0.0, st.mono(p.muted));
                    job.append(&line[k + 1..], 0.0, st.mono(p.text));
                }
                _ => job.append(line, 0.0, st.mono(p.text)),
            }
            continue;
        }
        // fenced code
        let trimmed = body.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            job.append(line, 0.0, st.mono(p.faint));
            continue;
        }
        if in_code {
            let mut f = st.mono(p.text);
            f.background = p.code_bg;
            job.append(line, 0.0, f);
            continue;
        }
        if let Some(h) = HEADING.captures(body) {
            let level = h[1].len();
            let size = st.size * [1.6, 1.4, 1.25, 1.12, 1.05, 1.0][level - 1];
            let mut mark = st.fmt(size, p.faint);
            mark.font_id = FontId::new(size, FontFamily::Proportional);
            mark.line_height = Some(size * 1.35);
            job.append(&line[..h[0].len()], 0.0, mark);
            let mut f = st.fmt(size, p.text);
            f.font_id = FontId::new(size, FontFamily::Name("bold".into()));
            f.line_height = Some(size * 1.35);
            inline(&mut job, &line[h[0].len()..], f, st);
            continue;
        }
        let mut rest = line;
        let mut fmt = base.clone();
        if trimmed.starts_with('>') {
            let n = line.len() - line.trim_start().len() + 1;
            job.append(&line[..n], 0.0, st.fmt(st.size, p.accent_strong));
            rest = &line[n..];
            fmt.color = p.muted;
            fmt.italics = true;
        } else if let Some(m) = LIST.captures(body) {
            let end = m.get(0).unwrap().end();
            let marker_end = m.get(3).unwrap().end();
            job.append(&line[..marker_end], 0.0, st.fmt(st.size, p.faint));
            if let Some(task) = m.get(4) {
                let done = !task.as_str().contains("[ ]");
                job.append(&line[marker_end..end], 0.0, st.mono(if done { p.accent_strong } else { p.muted }));
                if done {
                    fmt.color = p.faint;
                    fmt.strikethrough = Stroke::new(1.0_f32, p.faint);
                }
            }
            rest = &line[end..];
        } else if body.trim() == "---" || body.trim() == "***" {
            job.append(line, 0.0, st.fmt(st.size, p.faint));
            continue;
        }
        inline(&mut job, rest, fmt, st);
    }
    job
}

fn inline(job: &mut LayoutJob, text: &str, base: TextFormat, st: &Styles) {
    let p = &st.p;
    let mut at = 0;
    for c in INLINE.captures_iter(text) {
        let m = c.get(0).unwrap();
        let (mut start, end) = (m.start(), m.end());
        if c.name("tag").is_some() && text[start..].starts_with(char::is_whitespace) {
            start += text[start..].chars().next().unwrap().len_utf8();
        }
        if start > at {
            job.append(&text[at..start], 0.0, base.clone());
        }
        let s = &text[start..end];
        let mut f = base.clone();
        if c.name("code").is_some() {
            f = st.mono(p.text);
            f.background = p.code_bg;
            wrapped(job, s, 1, 1, f, p);
        } else if c.name("wiki").is_some() {
            f.color = p.link();
            let open = if s.starts_with('!') { 3 } else { 2 };
            // [[target|alias]]: the target is syntax too when an alias is shown
            match s.find('|') {
                Some(bar) if bar > open => {
                    let m = syntax(&f, p);
                    job.append(&s[..bar + 1], 0.0, m.clone());
                    job.append(&s[bar + 1..s.len() - 2], 0.0, f);
                    job.append(&s[s.len() - 2..], 0.0, m);
                }
                _ => wrapped(job, s, open, 2, f, p),
            }
        } else if c.name("mdlink").is_some() {
            f.color = p.link();
            let mid = s.find("](").unwrap_or(s.len());
            let open = if s.starts_with('!') { 2 } else { 1 };
            let m = syntax(&f, p);
            job.append(&s[..open], 0.0, m.clone());
            job.append(&s[open..mid], 0.0, f);
            job.append(&s[mid..], 0.0, m);
        } else if c.name("url").is_some() {
            f.color = p.link();
            f.underline = Stroke::new(1.0_f32, p.link().gamma_multiply(0.4));
            job.append(s, 0.0, f);
        } else if c.name("bold").is_some() {
            f.font_id = FontId::new(f.font_id.size, FontFamily::Name("bold".into()));
            wrapped(job, s, 2, 2, f, p);
        } else if c.name("italic").is_some() {
            f.italics = true;
            wrapped(job, s, 1, 1, f, p);
        } else if c.name("strike").is_some() {
            f.strikethrough = Stroke::new(1.0_f32, f.color);
            wrapped(job, s, 2, 2, f, p);
        } else if c.name("mark").is_some() {
            f.background = p.highlight;
            wrapped(job, s, 2, 2, f, p);
        } else if c.name("field").is_some() {
            f.color = p.muted;
            match s.find("::") {
                Some(k) => {
                    let m = syntax(&f, p);
                    job.append(&s[..1], 0.0, m.clone());
                    job.append(&s[1..k], 0.0, f.clone());
                    job.append(&s[k..k + 2], 0.0, m.clone());
                    let mut v = base.clone();
                    v.color = p.text;
                    job.append(&s[k + 2..s.len() - 1], 0.0, v);
                    job.append(&s[s.len() - 1..], 0.0, m);
                }
                None => job.append(s, 0.0, f),
            }
        } else if c.name("tag").is_some() {
            f.color = p.accent_strong;
            job.append(s, 0.0, f);
        } else if c.name("comment").is_some() {
            f.color = p.faint;
            f.italics = true;
            job.append(s, 0.0, f);
        } else {
            job.append(s, 0.0, f);
        }
        at = end;
    }
    if at < text.len() {
        job.append(&text[at..], 0.0, base);
    }
}

/// Layout cache: rebuilding the styled job only when the text, width or theme change.
#[derive(Default)]
pub struct HighlightCache {
    key: u64,
    job: Option<LayoutJob>,
}

impl HighlightCache {
    pub fn get(&mut self, text: &str, st: &Styles) -> LayoutJob {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        st.size.to_bits().hash(&mut h);
        st.p.dark.hash(&mut h);
        st.p.accent.hash(&mut h);
        let key = h.finish();
        if self.key != key || self.job.is_none() {
            self.job = Some(highlight(text, st));
            self.key = key;
        }
        self.job.clone().unwrap()
    }
}

/// Character index ↔ byte offset helpers for the editor's cursor.
pub fn char_to_byte(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

pub fn byte_to_char(s: &str, b: usize) -> usize {
    s[..b.min(s.len())].chars().count()
}

/// UTF-16 offset (what the script engine reports) → byte offset.
pub fn utf16_to_byte(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (b, c) in s.char_indices() {
        if n >= u {
            return b;
        }
        n += c.len_utf16();
    }
    s.len()
}

/// The selection of an editor, as sorted char indices.
fn selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
    let r = egui::TextEdit::load_state(ctx, id)?.cursor.char_range()?;
    let (a, b) = (r.primary.index, r.secondary.index);
    Some((a.min(b), a.max(b)))
}

fn set_selection(ctx: &egui::Context, id: egui::Id, a: usize, b: usize) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
    state.store(ctx, id);
}

/// Put `open`/`close` around the selection, or take them off when they are
/// already there (inside or just outside it).
pub fn toggle_wrap(text: &mut String, ctx: &egui::Context, id: egui::Id, open: &str, close: &str) {
    let (a, b) = selection(ctx, id).unwrap_or((0, 0));
    let (ba, bb) = (char_to_byte(text, a), char_to_byte(text, b));
    let (no, nc) = (open.chars().count(), close.chars().count());
    let sel = &text[ba..bb];
    if sel.len() >= open.len() + close.len() && sel.starts_with(open) && sel.ends_with(close) {
        let inner = sel[open.len()..sel.len() - close.len()].to_string();
        text.replace_range(ba..bb, &inner);
        set_selection(ctx, id, a, b - no - nc);
    } else if text[..ba].ends_with(open) && text[bb..].starts_with(close) {
        text.replace_range(bb..bb + close.len(), "");
        text.replace_range(ba - open.len()..ba, "");
        set_selection(ctx, id, a - no, b - no);
    } else {
        text.insert_str(bb, close);
        text.insert_str(ba, open);
        set_selection(ctx, id, a + no, b + no);
    }
    ctx.memory_mut(|m| m.request_focus(id));
}

fn copy_selection(text: &str, ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
    let (a, b) = selection(ctx, id)?;
    if a == b {
        return None;
    }
    let (ba, bb) = (char_to_byte(text, a), char_to_byte(text, b));
    ctx.copy_text(text[ba..bb].to_string());
    Some((ba, bb))
}

/// Ctrl+B / Ctrl+I while the editor has the focus, and the right-click menu.
fn editing_extras(text: &mut String, resp: &egui::Response, ui: &egui::Ui, id: egui::Id, p: &Palette) {
    let ctx = ui.ctx().clone();
    if resp.has_focus() {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
            toggle_wrap(text, &ctx, id, "**", "**");
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::I)) {
            toggle_wrap(text, &ctx, id, "*", "*");
        }
    }
    resp.context_menu(|ui| {
        let p = *p;
        let has_sel = selection(&ctx, id).is_some_and(|(a, b)| a != b);
        if crate::kit::menu_entry(ui, crate::icons::SCISSORS, "Recortar", "Ctrl+X", has_sel, false, p) {
            if let Some((ba, bb)) = copy_selection(text, &ctx, id) {
                let a = byte_to_char(text, ba);
                text.replace_range(ba..bb, "");
                set_selection(&ctx, id, a, a);
            }
        }
        if crate::kit::menu_entry(ui, crate::icons::COPY, "Copiar", "Ctrl+C", has_sel, false, p) {
            copy_selection(text, &ctx, id);
        }
        if crate::kit::menu_entry(ui, crate::icons::CLIPBOARD, "Colar", "Ctrl+V", true, false, p) {
            if let Ok(clip) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                let (a, b) = selection(&ctx, id).unwrap_or((0, 0));
                let (ba, bb) = (char_to_byte(text, a), char_to_byte(text, b));
                text.replace_range(ba..bb, &clip);
                let end = a + clip.chars().count();
                set_selection(&ctx, id, end, end);
            }
        }
        if crate::kit::menu_entry(ui, "", "Selecionar tudo", "Ctrl+A", true, false, p) {
            set_selection(&ctx, id, 0, text.chars().count());
        }
        crate::kit::menu_sep(ui, p);
        for (glyph, label, keys, open, close) in [
            (crate::icons::BOLD, "Negrito", "Ctrl+B", "**", "**"),
            (crate::icons::ITALIC, "Itálico", "Ctrl+I", "*", "*"),
            (crate::icons::HIGHLIGHTER, "Realce", "", "==", "=="),
            (crate::icons::CODE, "Código", "", "`", "`"),
            (crate::icons::LINK, "Link interno", "", "[[", "]]"),
            (crate::icons::EYE_OFF, "Comentário", "", "%%", "%%"),
        ] {
            if crate::kit::menu_entry(ui, glyph, label, keys, true, false, p) {
                toggle_wrap(text, &ctx, id, open, close);
            }
        }
    });
}

/// The note editor; `min_rows` keeps an empty note clickable.
pub fn editor_widget<'a>(text: &'a mut String, id: egui::Id, cache: &'a mut HighlightCache, st: &'a Styles, min_rows: usize) -> impl egui::Widget + 'a {
    let rows = text.split('\n').count().max(min_rows);
    move |ui: &mut egui::Ui| {
        let mut layouter = |ui: &egui::Ui, s: &str, wrap: f32| {
            let mut job = cache.get(s, st);
            job.wrap.max_width = wrap;
            ui.fonts(|f| f.layout_job(job))
        };
        // a right click must not move the caret: the menu acts on the selection
        let prev = selection(ui.ctx(), id);
        crate::writing::before(ui, id);
        let out = egui::TextEdit::multiline(text)
            .id(id)
            .frame(false)
            .desired_width(f32::INFINITY)
            .desired_rows(rows)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui);
        let resp = out.response.clone();
        let wrote = crate::writing::after(ui, id, text, &out, st.p);
        if resp.hovered() && ui.input(|i| i.pointer.secondary_pressed()) {
            if let Some((a, b)) = prev {
                set_selection(ui.ctx(), id, a, b);
            }
        }
        let before = text.len();
        editing_extras(text, &resp, ui, id, &st.p);
        let mut resp = resp;
        if text.len() != before || wrote {
            resp.mark_changed();
        }
        resp
    }
}
