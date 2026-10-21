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
        TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color, ..Default::default() }
    }
    fn mono(&self, color: Color32) -> TextFormat {
        TextFormat { font_id: FontId::new(self.size * 0.9, FontFamily::Monospace), color, ..Default::default() }
    }
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
            }
            job.append(line, 0.0, st.mono(p.muted));
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
            job.append(&line[..h[0].len()], 0.0, mark);
            let mut f = st.fmt(size, p.text);
            f.font_id = FontId::new(size, FontFamily::Name("bold".into()));
            inline(&mut job, &line[h[0].len()..], f, st);
            continue;
        }
        let mut rest = line;
        let mut fmt = base.clone();
        if trimmed.starts_with('>') {
            let n = line.len() - line.trim_start().len() + 1;
            job.append(&line[..n], 0.0, st.fmt(st.size, p.accent));
            rest = &line[n..];
            fmt.color = p.muted;
        } else if let Some(m) = LIST.captures(body) {
            let end = m.get(0).unwrap().end();
            let marker_end = m.get(3).unwrap().end();
            job.append(&line[..marker_end], 0.0, st.fmt(st.size, p.accent));
            if let Some(task) = m.get(4) {
                let done = !task.as_str().contains("[ ]");
                job.append(&line[marker_end..end], 0.0, st.mono(p.accent));
                if done {
                    fmt.color = p.faint;
                    fmt.strikethrough = Stroke::new(1.0, p.faint);
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
        } else if c.name("wiki").is_some() || c.name("mdlink").is_some() || c.name("url").is_some() {
            f.color = p.link();
            f.underline = Stroke::new(1.0, p.link().gamma_multiply(0.4));
        } else if c.name("bold").is_some() {
            f.font_id = FontId::new(f.font_id.size, FontFamily::Name("bold".into()));
        } else if c.name("italic").is_some() {
            f.italics = true;
        } else if c.name("strike").is_some() {
            f.strikethrough = Stroke::new(1.0, f.color);
        } else if c.name("mark").is_some() {
            f.background = p.highlight;
        } else if c.name("field").is_some() {
            f = st.mono(p.muted);
            f.background = p.bg_secondary;
        } else if c.name("tag").is_some() {
            f.color = p.accent;
        } else if c.name("comment").is_some() {
            f.color = p.faint;
        }
        job.append(s, 0.0, f);
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

pub fn editor_widget<'a>(text: &'a mut String, id: egui::Id, cache: &'a mut HighlightCache, st: &'a Styles) -> impl egui::Widget + 'a {
    move |ui: &mut egui::Ui| {
        let mut layouter = |ui: &egui::Ui, s: &str, wrap: f32| {
            let mut job = cache.get(s, st);
            job.wrap.max_width = wrap;
            ui.fonts(|f| f.layout_job(job))
        };
        egui::TextEdit::multiline(text)
            .id(id)
            .frame(false)
            .desired_width(f32::INFINITY)
            .desired_rows(30)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui)
            .response
    }
}
