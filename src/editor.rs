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
    /// Live Preview: the text set as the reading view sets it (same fonts,
    /// sizes, line heights and colours), with the Markdown marks hidden
    /// except where the cursor is.
    pub live: bool,
    /// The theme's typography, and a hash of it for the layout cache.
    pub typo: crate::typo::Typo,
    typo_key: u64,
}

/// The notes' font families (the theme's fonts, else Inter and the monospace).
pub fn text_family() -> FontFamily {
    FontFamily::Name("theme-text".into())
}
pub fn bold_family() -> FontFamily {
    FontFamily::Name("theme-text-bold".into())
}
pub fn italic_family() -> FontFamily {
    FontFamily::Name("theme-text-italic".into())
}
pub fn mono_family() -> FontFamily {
    FontFamily::Name("theme-mono".into())
}

impl Styles {
    fn new(size: f32, p: Palette, live: bool) -> Styles {
        let typo = crate::typo::get();
        let mut h = DefaultHasher::new();
        format!("{typo:?}").hash(&mut h);
        Styles { size, p, live, typo, typo_key: h.finish() }
    }

    pub fn source(size: f32, p: Palette) -> Styles {
        Styles::new(size, p, false)
    }

    pub fn live(size: f32, p: Palette) -> Styles {
        Styles::new(size, p, true)
    }

    fn fmt(&self, size: f32, color: Color32) -> TextFormat {
        if self.live {
            // the reading view's paragraph (markdown::Renderer::fmt)
            let lh = if size > self.size * 1.1 { 1.28 } else { self.typo.line };
            return TextFormat { font_id: FontId::new(size, text_family()), color, line_height: Some((size * lh).round()), valign: egui::Align::Center, ..Default::default() };
        }
        TextFormat { font_id: FontId::new(size, text_family()), color, line_height: Some(size * self.typo.line), ..Default::default() }
    }
    fn mono(&self, color: Color32) -> TextFormat {
        if self.live {
            return TextFormat { font_id: FontId::new(self.size * self.typo.code, mono_family()), color, valign: egui::Align::Center, ..Default::default() };
        }
        TextFormat { font_id: FontId::new(self.size * self.typo.code, mono_family()), color, line_height: Some(self.size * 1.5), ..Default::default() }
    }
    /// Code blocks: the reading view's code size in Live Preview.
    fn code(&self, color: Color32) -> TextFormat {
        let mut f = self.mono(color);
        if self.live {
            f.font_id.size = self.size * 0.85;
            f.line_height = Some((self.size * 1.45).round());
        }
        f
    }
    /// Italic: the italic face in Live Preview (as the reading view), slanted otherwise.
    fn italic(&self, f: &mut TextFormat) {
        let bold = f.font_id.family == bold_family();
        if self.live && !bold {
            f.font_id.family = italic_family();
        } else {
            f.italics = true;
        }
        if let Some(c) = self.typo.italic.filter(|_| self.live) {
            f.color = c;
        }
    }
    fn heading_scale(&self, level: usize) -> f32 {
        if self.live {
            self.typo.heading[level - 1]
        } else {
            [1.6, 1.4, 1.25, 1.12, 1.05, 1.0][level - 1]
        }
    }
}

/// Where the marks show: the byte range of the cursor or selection in the
/// text being laid out (`None`: nowhere; source mode shows them all).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Reveal(pub Option<(usize, usize)>);

impl Reveal {
    fn touches(&self, a: usize, b: usize) -> bool {
        self.0.is_some_and(|(x, y)| x <= b && y >= a)
    }
}

/// Marks out of sight: there for the cursor, but taking no room.
fn hidden(f: &TextFormat) -> TextFormat {
    let mut m = f.clone();
    m.font_id = FontId::new(0.5, FontFamily::Proportional);
    m.color = Color32::TRANSPARENT;
    m.background = Color32::TRANSPARENT;
    m.underline = Stroke::NONE;
    m.strikethrough = Stroke::NONE;
    m.line_height = Some(1.0);
    m.italics = false;
    m
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
fn wrapped(job: &mut LayoutJob, s: &str, open: usize, close: usize, f: TextFormat, m: &Marks) {
    if s.len() < open + close {
        job.append(s, 0.0, f);
        return;
    }
    let mk = m.mark(&f);
    job.append(&s[..open], 0.0, mk.clone());
    job.append(&s[open..s.len() - close], 0.0, f);
    job.append(&s[s.len() - close..], 0.0, mk);
}

/// How the marks of one span are drawn: quiet, or hidden in Live Preview
/// when the cursor is elsewhere.
struct Marks {
    p: Palette,
    hide: bool,
}

impl Marks {
    fn mark(&self, f: &TextFormat) -> TextFormat {
        if self.hide {
            hidden(f)
        } else {
            syntax(f, &self.p)
        }
    }
}

/// How a line of the note sits in Live Preview, as the reading view would
/// draw its block: where its text starts, which of its first sections are a
/// mark hanging to the left of that (`#`, `>`, `-`), how it is aligned and
/// what is drawn with it.
#[derive(Clone, Debug)]
pub struct LineInfo {
    /// The line's sections in the note's job (its newline included).
    pub sections: std::ops::Range<usize>,
    /// The line's bytes, without the newline.
    pub bytes: std::ops::Range<usize>,
    pub content_x: f32,
    /// How many of the first sections hang left of `content_x`.
    pub lead: usize,
    pub align: LineAlign,
    pub deco: Deco,
    /// Room above the line (between list items).
    pub gap: f32,
    pub newline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineAlign {
    Note,
    List,
    Left,
}

/// What the reading view draws beside a line, drawn by the editor too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Deco {
    None,
    /// A bullet in the marker box of a list item at this level.
    Bullet(usize),
    /// A check box (done or not).
    Task(usize, bool),
    /// Bars of a quote this deep.
    Quote(usize),
    /// A line of a fenced code block (the fences included).
    Code,
}

/// The width of a list item's marker box, plus the gap after it, as the
/// reading view lays out lists.
pub fn list_step(size: f32) -> f32 {
    size * 1.3 + 4.0
}

static QUOTE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(>\s?)+").unwrap());

/// The styled layout, and how each line sits in Live Preview.
pub fn highlight_lines(text: &str, st: &Styles, reveal: Reveal) -> (LayoutJob, Vec<LineInfo>) {
    let mut job = LayoutJob::default();
    let mut lines: Vec<LineInfo> = Vec::new();
    let p = &st.p;
    let base = st.fmt(st.size, p.text);
    let mut in_fm = false;
    let mut in_code = false;
    // the indentation of the open list items, for their levels
    let mut list_cols: Vec<usize> = Vec::new();
    let mut prev_list = false;
    let mut at = 0;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let line_at = at;
        at += line.len();
        let body = line.trim_end_matches(['\n', '\r']);
        let s0 = job.sections.len();
        let mut info = LineInfo { sections: 0..0, bytes: line_at..line_at + body.len(), content_x: 0.0, lead: 0, align: LineAlign::Note, deco: Deco::None, gap: 0.0, newline: line.ends_with('\n') };
        // in Live Preview, line marks (#, >, -) show on the cursor's line only
        let on_line = !st.live || reveal.touches(line_at, line_at + body.len());
        let trimmed = body.trim_start();
        let is_list = !in_fm && !in_code && LIST.is_match(body);
        if !is_list && !body.trim().is_empty() && !body.starts_with([' ', '\t']) {
            list_cols.clear();
        }
        'line: {
            // frontmatter
            if n == 0 && body.trim_end() == "---" {
                in_fm = true;
                job.append(line, 0.0, st.mono(p.faint));
                info.align = LineAlign::Left;
                break 'line;
            }
            if in_fm {
                info.align = LineAlign::Left;
                if body.trim_end() == "---" {
                    in_fm = false;
                    job.append(line, 0.0, st.mono(p.faint));
                    break 'line;
                }
                match body.find(':') {
                    Some(k) if !body.starts_with(' ') && !body.starts_with('-') => {
                        job.append(&line[..k + 1], 0.0, st.mono(p.muted));
                        job.append(&line[k + 1..], 0.0, st.mono(p.text));
                    }
                    _ => job.append(line, 0.0, st.mono(p.text)),
                }
                break 'line;
            }
            // fenced code
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code = !in_code;
                job.append(line, 0.0, st.code(p.faint));
                info.align = LineAlign::Left;
                info.deco = Deco::Code;
                info.content_x = if st.live { 16.0 } else { 0.0 };
                break 'line;
            }
            if in_code {
                let mut f = st.code(p.text);
                if !st.live {
                    f.background = p.code_bg;
                }
                job.append(line, 0.0, f);
                info.align = LineAlign::Left;
                info.deco = Deco::Code;
                info.content_x = if st.live { 16.0 } else { 0.0 };
                break 'line;
            }
            if let Some(h) = HEADING.captures(body) {
                let level = h[1].len();
                let size = st.size * st.heading_scale(level);
                let mut f = st.fmt(size, p.text);
                f.font_id = FontId::new(size, if st.typo.heading_bold[level - 1] || !st.live { bold_family() } else { text_family() });
                if let Some(c) = st.typo.heading_color[level - 1].filter(|_| st.live) {
                    f.color = c;
                }
                if !st.live {
                    f.line_height = Some(size * 1.35);
                }
                let mut mark = f.clone();
                mark.font_id = FontId::new(size, text_family());
                mark.color = p.faint;
                job.append(&line[..h[0].len()], 0.0, if on_line { mark } else { hidden(&f) });
                info.lead = 1;
                inline(&mut job, &line[h[0].len()..], line_at + h[0].len(), f, st, reveal);
                break 'line;
            }
            let mut rest = line;
            let mut rest_at = line_at;
            let mut fmt = base.clone();
            if let Some(q) = QUOTE.find(body).filter(|_| trimmed.starts_with('>')) {
                let n = q.end();
                let depth = q.as_str().matches('>').count();
                job.append(&line[..n], 0.0, if on_line || !st.live { st.fmt(st.size, p.accent_strong) } else { hidden(&base) });
                info.lead = 1;
                info.deco = Deco::Quote(depth);
                info.content_x = if st.live { 18.0 * depth as f32 } else { 0.0 };
                rest = &line[n..];
                rest_at += n;
                if !st.live {
                    fmt.color = p.muted;
                    fmt.italics = true;
                }
            } else if let Some(m) = LIST.captures(body) {
                let end = m.get(0).unwrap().end();
                let marker_end = m.get(3).unwrap().end();
                let col = m.get(1).unwrap().as_str().chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
                while list_cols.last().is_some_and(|&c| c >= col) {
                    list_cols.pop();
                }
                let level = list_cols.len();
                list_cols.push(col);
                let ordered = m[2].starts_with(|c: char| c.is_ascii_digit());
                let task = m.get(4).map(|t| !t.as_str().contains("[ ]"));
                let show = on_line || !st.live || (ordered && task.is_none());
                let mut marker = st.fmt(st.size, if ordered { p.muted } else { p.faint });
                crate::align::flush_left(&mut marker);
                job.append(&line[..marker_end], 0.0, if show { marker } else { hidden_keep(&marker) });
                info.lead = 1;
                info.align = LineAlign::List;
                info.content_x = if st.live { list_step(st.size) * (level + 1) as f32 } else { 0.0 };
                info.deco = match task {
                    Some(done) if !show => Deco::Task(level, done),
                    None if !show => Deco::Bullet(level),
                    _ => Deco::None,
                };
                if prev_list {
                    // the reading view's room between items, with the item spacing of its layout
                    info.gap = st.size * 0.2 + 6.0;
                }
                if let Some(t) = m.get(4) {
                    let done = !t.as_str().contains("[ ]");
                    job.append(&line[marker_end..end], 0.0, if show { st.mono(if done { p.accent_strong } else { p.muted }) } else { hidden(&base) });
                    info.lead = 2;
                    if done {
                        fmt.color = p.faint;
                        fmt.strikethrough = Stroke::new(1.0_f32, p.faint);
                    }
                }
                rest = &line[end..];
                rest_at += end;
            } else if body.trim() == "---" || body.trim() == "***" {
                job.append(line, 0.0, st.fmt(st.size, p.faint));
                break 'line;
            } else if !list_cols.is_empty() && !body.trim().is_empty() {
                // a continuation of a list item: under its text
                info.content_x = if st.live { list_step(st.size) * list_cols.len() as f32 } else { 0.0 };
                info.align = LineAlign::List;
            }
            inline(&mut job, rest, rest_at, fmt, st, reveal);
        }
        prev_list = is_list || (prev_list && !body.trim().is_empty() && body.starts_with([' ', '\t']));
        if job.sections.len() == s0 {
            job.append("", 0.0, base.clone());
        }
        info.sections = s0..job.sections.len();
        lines.push(info);
    }
    // the empty line after a final newline (or of an empty note)
    if text.is_empty() || text.ends_with('\n') {
        let s0 = job.sections.len();
        job.append("", 0.0, base.clone());
        lines.push(LineInfo { sections: s0..s0 + 1, bytes: text.len()..text.len(), content_x: 0.0, lead: 0, align: LineAlign::Note, deco: Deco::None, gap: 0.0, newline: false });
    }
    (job, lines)
}

/// A hidden mark that keeps the flags of `f` (the list's flush left).
fn hidden_keep(f: &TextFormat) -> TextFormat {
    let mut h = hidden(f);
    h.underline = f.underline;
    h
}

fn inline(job: &mut LayoutJob, text: &str, offset: usize, base: TextFormat, st: &Styles, reveal: Reveal) {
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
        let marks = Marks { p: *p, hide: st.live && !reveal.touches(offset + start, offset + end) };
        let mut f = base.clone();
        if c.name("code").is_some() {
            f = st.mono(p.text);
            f.background = p.code_bg;
            wrapped(job, s, 1, 1, f, &marks);
        } else if c.name("wiki").is_some() {
            f.color = p.link();
            if st.live {
                f.underline = Stroke::new(1.0_f32, p.link().gamma_multiply(0.5));
            }
            let open = if s.starts_with('!') { 3 } else { 2 };
            // [[target|alias]]: the target is syntax too when an alias is shown
            match s.find('|') {
                Some(bar) if bar > open => {
                    let m = marks.mark(&f);
                    job.append(&s[..bar + 1], 0.0, m.clone());
                    job.append(&s[bar + 1..s.len() - 2], 0.0, f);
                    job.append(&s[s.len() - 2..], 0.0, m);
                }
                _ => wrapped(job, s, open, 2, f, &marks),
            }
        } else if c.name("mdlink").is_some() {
            f.color = p.link();
            if st.live {
                f.underline = Stroke::new(1.0_f32, p.link().gamma_multiply(0.5));
            }
            let mid = s.find("](").unwrap_or(s.len());
            let open = if s.starts_with('!') { 2 } else { 1 };
            let m = marks.mark(&f);
            job.append(&s[..open], 0.0, m.clone());
            job.append(&s[open..mid], 0.0, f);
            job.append(&s[mid..], 0.0, m);
        } else if c.name("url").is_some() {
            f.color = p.link();
            f.underline = Stroke::new(1.0_f32, p.link().gamma_multiply(0.4));
            job.append(s, 0.0, f);
        } else if c.name("bold").is_some() {
            f.font_id = FontId::new(f.font_id.size, bold_family());
            if let Some(c) = st.typo.bold.filter(|_| st.live) {
                f.color = c;
            }
            wrapped(job, s, 2, 2, f, &marks);
        } else if c.name("italic").is_some() {
            st.italic(&mut f);
            wrapped(job, s, 1, 1, f, &marks);
        } else if c.name("strike").is_some() {
            f.strikethrough = Stroke::new(1.0_f32, f.color);
            wrapped(job, s, 2, 2, f, &marks);
        } else if c.name("mark").is_some() {
            f.background = p.highlight;
            wrapped(job, s, 2, 2, f, &marks);
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
            if st.live {
                f.background = p.accent_soft;
            }
            job.append(s, 0.0, f);
        } else if c.name("comment").is_some() {
            f.color = p.faint;
            st.italic(&mut f);
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

/// Layout cache: rebuilding the styled job only when the text, the theme or
/// (in Live Preview) the cursor's span change; and the composed Live
/// Preview galley, also by width and alignment.
#[derive(Default)]
pub struct HighlightCache {
    key: u64,
    job: Option<LayoutJob>,
    lines: Vec<LineInfo>,
    live_key: u64,
    live: Option<(std::sync::Arc<egui::Galley>, std::sync::Arc<Vec<DecoShape>>)>,
}

impl HighlightCache {
    fn key(text: &str, st: &Styles, reveal: Reveal) -> u64 {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        st.size.to_bits().hash(&mut h);
        st.p.dark.hash(&mut h);
        st.p.accent.hash(&mut h);
        st.live.hash(&mut h);
        st.typo_key.hash(&mut h);
        if st.live {
            reveal.hash(&mut h);
        }
        h.finish()
    }

    pub fn get(&mut self, text: &str, st: &Styles, reveal: Reveal) -> LayoutJob {
        let key = Self::key(text, st, reveal);
        if self.key != key || self.job.is_none() {
            let (job, lines) = highlight_lines(text, st, reveal);
            self.job = Some(job);
            self.lines = lines;
            self.key = key;
        }
        self.job.clone().unwrap()
    }

    /// The Live Preview galley at `width`, and what to draw with it.
    pub fn live(&mut self, fonts: &egui::text::Fonts, text: &str, st: &Styles, reveal: Reveal, width: f32) -> (std::sync::Arc<egui::Galley>, std::sync::Arc<Vec<DecoShape>>) {
        let job = self.get(text, st, reveal);
        let a = crate::align::current();
        let mut h = DefaultHasher::new();
        self.key.hash(&mut h);
        width.to_bits().hash(&mut h);
        (a as u8).hash(&mut h);
        let k = h.finish();
        if self.live_key != k || self.live.is_none() {
            let (g, d) = compose(fonts, job, &self.lines, width, a, st);
            self.live = Some((std::sync::Arc::new(g), std::sync::Arc::new(d)));
            self.live_key = k;
        }
        self.live.clone().unwrap()
    }
}

/// Something the reading view draws around the text, in galley coordinates.
#[derive(Clone, Debug)]
pub enum DecoShape {
    Bullet(egui::Pos2),
    Check(egui::Pos2, bool),
    Bar(f32, f32, f32),
    CodeBox(egui::Rect),
}

/// The note's lines laid out one by one, each at its own indent and width,
/// with its own alignment, and joined into one galley for the text editor.
fn compose(fonts: &egui::text::Fonts, job: LayoutJob, lines: &[LineInfo], width: f32, note: crate::align::TextAlign, st: &Styles) -> (egui::Galley, Vec<DecoShape>) {
    use egui::epaint::text::Row;
    let job = std::sync::Arc::new(job);
    let mut rows: Vec<Row> = Vec::new();
    let mut decos = Vec::new();
    let mut y = 0.0_f32;
    let mut bounds = egui::Rect::NOTHING;
    let (mut verts, mut idx) = (0, 0);
    let mut ppp = 1.0;
    let mut code_top: Option<f32> = None;
    let mut quote: Vec<(f32, f32)> = Vec::new(); // the open bars: top, x
    let sub = |range: std::ops::Range<usize>, bytes: &std::ops::Range<usize>| -> LayoutJob {
        let mut j = LayoutJob { text: job.text[bytes.clone()].to_string(), ..Default::default() };
        for sec in &job.sections[range] {
            let a = sec.byte_range.start.clamp(bytes.start, bytes.end) - bytes.start;
            let b = sec.byte_range.end.clamp(bytes.start, bytes.end) - bytes.start;
            j.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: a..b, format: sec.format.clone() });
        }
        j
    };
    for (li, line) in lines.iter().enumerate() {
        y += line.gap;
        let align = match line.align {
            LineAlign::Note => note,
            LineAlign::List => crate::align::for_lists(note),
            LineAlign::Left => crate::align::TextAlign::Left,
        };
        // the hanging mark: its width, from the line's start to the text
        let lead_w = if line.lead > 0 {
            let lead = line.sections.start..line.sections.start + line.lead;
            let end = job.sections[lead.end - 1].byte_range.end.min(line.bytes.end);
            let mut j = sub(lead, &(line.bytes.start..end.max(line.bytes.start)));
            j.wrap.max_width = f32::INFINITY;
            fonts.layout_job(j).rect.width()
        } else {
            0.0
        };
        let wrap = (width - line.content_x).max(40.0);
        let mut j = sub(line.sections.clone(), &line.bytes);
        j.wrap.max_width = wrap;
        let g = crate::align::layout(fonts, j, wrap, align);
        ppp = g.pixels_per_point;
        let x0 = line.content_x - lead_w;
        let n = g.rows.len();
        let top = y;
        for (ri, row) in g.rows.iter().enumerate() {
            let mut row = row.clone();
            // whole pixels, or the text blurs
            let d = egui::vec2(if ri == 0 { x0 } else { x0 + lead_w }, y).round();
            for gl in &mut row.glyphs {
                gl.pos += d;
                gl.section_index += line.sections.start as u32;
            }
            row.section_index_at_start += line.sections.start as u32;
            row.rect = row.rect.translate(d);
            row.visuals.mesh.translate(d);
            row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(d);
            if ri + 1 == n {
                row.ends_with_newline = line.newline;
            }
            bounds = bounds.union(row.visuals.mesh_bounds);
            verts += row.visuals.mesh.vertices.len();
            idx += row.visuals.mesh.indices.len();
            rows.push(row);
        }
        let first = rows[rows.len() - n].rect;
        y = top + g.rect.height().max(first.height());
        let step = list_step(st.size);
        match line.deco {
            Deco::Bullet(level) => decos.push(DecoShape::Bullet(egui::pos2(step * level as f32 + st.size * 0.65, first.center().y))),
            Deco::Task(level, done) => decos.push(DecoShape::Check(egui::pos2(step * level as f32 + st.size * 0.65, first.center().y), done)),
            _ => {}
        }
        // code: one box from the opening fence to the closing one
        let next_code = lines.get(li + 1).is_some_and(|l| l.deco == Deco::Code);
        if line.deco == Deco::Code {
            let t = *code_top.get_or_insert(top);
            let fence = matches!(job.text[line.bytes.clone()].trim_start().get(..3), Some("```") | Some("~~~"));
            if !next_code || (fence && t != top) {
                decos.push(DecoShape::CodeBox(egui::Rect::from_min_max(egui::pos2(0.0, t), egui::pos2(width, y))));
                code_top = None;
            }
        } else {
            code_top = None;
        }
        // quotes: a bar per level, from the first line to the last
        let depth = if let Deco::Quote(d) = line.deco { d } else { 0 };
        while quote.len() > depth {
            let (t, x) = quote.pop().unwrap();
            decos.push(DecoShape::Bar(x, t, top));
        }
        while quote.len() < depth {
            quote.push((top, 18.0 * quote.len() as f32 + 1.0));
        }
    }
    for (t, x) in quote.drain(..) {
        decos.push(DecoShape::Bar(x, t, y));
    }
    let rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(width, y));
    let galley = egui::Galley { job, rows, elided: false, rect, mesh_bounds: bounds, num_vertices: verts, num_indices: idx, pixels_per_point: ppp };
    (galley, decos)
}

/// Draw what the reading view draws around the text: list bullets and check
/// boxes, quote bars, the code block's box.
fn paint_decos(painter: &egui::Painter, at: egui::Pos2, decos: &[DecoShape], size: f32, p: &Palette) {
    let v = at.to_vec2();
    let typo = crate::typo::get();
    for d in decos {
        match d {
            DecoShape::Bullet(c) => {
                painter.circle_filled(*c + v, (size * 0.16).max(2.2), typo.bullet.unwrap_or(p.faint));
            }
            DecoShape::Check(c, done) => {
                let r = egui::Rect::from_center_size(*c + v, egui::vec2(size * 0.9, size * 0.9));
                // as egui draws the reading view's check box
                painter.rect_filled(r, 3.0, p.bg_active);
                if *done {
                    let k = r.width();
                    let _ = painter.line(vec![r.min + egui::vec2(k * 0.22, k * 0.52), r.min + egui::vec2(k * 0.42, k * 0.72), r.min + egui::vec2(k * 0.78, k * 0.28)], Stroke::new(1.5_f32, p.text));
                }
            }
            DecoShape::Bar(x, t, b) => {
                painter.line_segment([egui::pos2(at.x + x, at.y + t + 2.0), egui::pos2(at.x + x, at.y + b - 2.0)], Stroke::new(typo.quote_width, typo.quote.unwrap_or(p.border_strong)));
            }
            DecoShape::CodeBox(r) => {
                painter.rect(r.translate(v), 8.0, p.code_bg, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
            }
        }
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
pub(crate) fn selection(ctx: &egui::Context, id: egui::Id) -> Option<(usize, usize)> {
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

/// "--" typed becomes "—"; a third "-" right after turns it back into
/// "---" (rules and frontmatter). Code (fenced or between backticks) and
/// "<!--" are left alone. Pure: returns the new text's cursor and the dash
/// made, or `None` when nothing changes.
pub fn dash_rule(text: &mut String, cursor: usize, made: Option<usize>) -> Option<(usize, Option<usize>)> {
    let b = char_to_byte(text, cursor);
    let head = &text[..b];
    if let Some(at) = made {
        if head.ends_with("—-") && byte_to_char(text, b - "—-".len()) == at {
            text.replace_range(b - "—-".len()..b, "---");
            return Some((cursor + 1, None));
        }
    }
    if !head.ends_with("--") {
        return None;
    }
    let rest = &head[..head.len() - 2];
    if rest.ends_with('-') || rest.ends_with("<!") || rest.ends_with('—') {
        return None;
    }
    let line_start = rest.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let fences = rest[..line_start].lines().filter(|l| { let t = l.trim_start(); t.starts_with("```") || t.starts_with("~~~") }).count();
    if fences % 2 == 1 || rest[line_start..].matches('`').count() % 2 == 1 {
        return None;
    }
    text.replace_range(b - 2..b, "—");
    Some((cursor - 1, Some(cursor - 2)))
}

/// Apply [`dash_rule`] to an egui editor when a "-" was just typed in it;
/// true when the text changed.
pub fn auto_dash(ui: &egui::Ui, id: egui::Id, text: &mut String) -> bool {
    let ctx = ui.ctx();
    let slot = id.with("dash-made");
    let typed: Option<bool> = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.as_str() == "-") } else { None }).last());
    let Some(dash) = typed else { return false };
    let made: Option<usize> = ctx.data(|d| d.get_temp(slot));
    ctx.data_mut(|d| d.remove::<usize>(slot));
    if !dash || !ctx.memory(|m| m.has_focus(id)) {
        return false;
    }
    let Some((a, b)) = selection(ctx, id) else { return false };
    if a != b {
        return false;
    }
    let Some((cur, mark)) = dash_rule(text, b, made) else { return false };
    set_selection(ctx, id, cur, cur);
    if let Some(m) = mark {
        ctx.data_mut(|d| d.insert_temp(slot, m));
    }
    true
}

/// The cursor or selection of an editor as a byte range of `text`.
fn reveal_of(ctx: &egui::Context, id: egui::Id, text: &str) -> Reveal {
    if !ctx.memory(|m| m.has_focus(id)) {
        return Reveal(None);
    }
    Reveal(selection(ctx, id).map(|(a, b)| (char_to_byte(text, a), char_to_byte(text, b))))
}

/// The galley an editor shows for `text` at `width`, as the layouter makes it.
pub fn galley(ui: &egui::Ui, text: &str, cache: &mut HighlightCache, st: &Styles, reveal: Reveal, width: f32) -> std::sync::Arc<egui::Galley> {
    if st.live {
        return ui.fonts(|f| cache.live(f, text, st, reveal, width).0);
    }
    let mut job = cache.get(text, st, reveal);
    job.wrap.max_width = width;
    ui.fonts(|f| crate::align::layout(f, job, width, crate::align::current()))
}

/// The cursor of an editor, as tall as the font of the letter beside it (the
/// one before it, or after it at the start of a row), blinking as egui's does.
/// Marks set out of sight (a font of half a point) do not count as a letter.
fn paint_caret(ui: &egui::Ui, id: egui::Id, out: &egui::text_edit::TextEditOutput, stroke: Stroke) {
    let Some(range) = out.cursor_range else { return };
    if !out.response.has_focus() || stroke.is_empty() {
        return;
    }
    let galley = &out.galley;
    let at = galley.pos_from_cursor(&range.primary);
    let (x, mid) = (at.center().x, at.center().y);
    let seen = |g: &&egui::epaint::text::Glyph| g.font_height >= 2.0;
    let glyph = galley.rows.iter().find(|r| r.min_y() <= mid && mid < r.max_y()).and_then(|r| {
        let before = r.glyphs.iter().rev().filter(|g| g.pos.x < x - 0.01).find(seen);
        before.or_else(|| r.glyphs.iter().filter(|g| g.pos.x >= x - 0.01).find(seen))
    });
    let (top, bottom) = match glyph {
        Some(g) => (g.pos.y - g.font_ascent, g.pos.y - g.font_ascent + g.font_height),
        None => (at.min.y, at.max.y),
    };
    let (top, bottom) = (top.max(at.min.y), bottom.min(at.max.y).max(top.max(at.min.y)));
    let ctx = ui.ctx();
    let (now, key) = (ctx.input(|i| i.time), (range.primary.ccursor.index, galley.job.text.len()));
    let slot = id.with("caret-since");
    let (last, mut since): ((usize, usize), f64) = ctx.data(|d| d.get_temp(slot)).unwrap_or((key, now));
    if last != key {
        since = now;
    }
    ctx.data_mut(|d| d.insert_temp(slot, (key, since)));
    let blink = ui.visuals().text_cursor.clone();
    if blink.blink {
        let cycle = (blink.on_duration + blink.off_duration).max(0.1);
        let t = ((now - since) % cycle as f64) as f32;
        if t >= blink.on_duration {
            ctx.request_repaint_after_secs(cycle - t);
            return;
        }
        ctx.request_repaint_after_secs(blink.on_duration - t);
    }
    let x = out.galley_pos.x + x;
    let dy = out.galley_pos.y;
    ui.painter().with_clip_rect(out.text_clip_rect).line_segment([egui::pos2(x, dy + top), egui::pos2(x, dy + bottom)], stroke);
}

/// The note editor; `min_rows` keeps an empty note clickable.
pub fn editor_widget<'a>(text: &'a mut String, id: egui::Id, cache: &'a mut HighlightCache, st: &'a Styles, min_rows: usize) -> impl egui::Widget + 'a {
    let rows = text.split('\n').count().max(min_rows);
    move |ui: &mut egui::Ui| {
        let reveal = if st.live { reveal_of(ui.ctx(), id, text) } else { Reveal(None) };
        if st.live {
            // bullets, bars and code boxes go under the text
            let w = ui.available_width();
            let decos = ui.fonts(|f| cache.live(f, text, st, reveal, w).1);
            paint_decos(ui.painter(), ui.cursor().min, &decos, st.size, &st.p);
        }
        let before_text = text.clone();
        let mut layouter = |ui: &egui::Ui, s: &str, wrap: f32| galley(ui, s, cache, st, reveal, wrap);
        // a right click must not move the caret: the menu acts on the selection
        let prev = selection(ui.ctx(), id);
        crate::writing::before(ui, id);
        let mut edit = egui::TextEdit::multiline(text)
            .id(id)
            .frame(false)
            .desired_width(f32::INFINITY)
            .desired_rows(rows)
            .lock_focus(true)
            .layouter(&mut layouter);
        if st.live {
            // where the reading view puts the text
            edit = edit.margin(egui::Margin::ZERO);
        }
        // egui's cursor spans the whole row, however loose its spacing or
        // large the heading; this one is painted below, the letter's height
        let cursor_stroke = ui.visuals().text_cursor.stroke;
        ui.visuals_mut().text_cursor.stroke = Stroke::NONE;
        let out = edit.show(ui);
        crate::find::paint(ui, &out.galley, out.galley_pos);
        crate::find::scroll_editor(ui, id, &out);
        ui.visuals_mut().text_cursor.stroke = cursor_stroke;
        paint_caret(ui, id, &out, cursor_stroke);
        let resp = out.response.clone();
        let wrote = crate::writing::after(ui, id, text, &out, st.p);
        if resp.hovered() && ui.input(|i| i.pointer.secondary_pressed()) {
            if let Some((a, b)) = prev {
                set_selection(ui.ctx(), id, a, b);
            }
        }
        let dashed = auto_dash(ui, id, text);
        let before = text.len();
        editing_extras(text, &resp, ui, id, &st.p);
        // the cursor moved into or out of a span: show its marks next frame
        if st.live && (reveal_of(ui.ctx(), id, text) != reveal || *text != before_text) {
            ui.ctx().request_repaint();
        }
        let mut resp = resp;
        if text.len() != before || wrote || dashed {
            resp.mark_changed();
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles() -> Styles {
        Styles::live(16.0, crate::theme::Palette::new(&crate::config::Appearance::default()))
    }

    #[test]
    fn dashes() {
        let mut t = String::from("Ele disse --");
        assert_eq!(dash_rule(&mut t, 12, None), Some((11, Some(10))));
        assert_eq!(t, "Ele disse —");
        // a third "-" right after: a rule
        let mut t = String::from("—-");
        assert_eq!(dash_rule(&mut t, 2, Some(0)), Some((3, None)));
        assert_eq!(t, "---");
        let mut t = String::from("----");
        assert_eq!(dash_rule(&mut t, 4, None), None);
        for s in ["`a --", "<!--", "```\nx --"] {
            let mut t = String::from(s);
            assert_eq!(dash_rule(&mut t, s.chars().count(), None), None, "{s}");
        }
        let mut t = String::from("```\nx\n```\nfim --");
        let n = t.chars().count();
        assert!(dash_rule(&mut t, n, None).is_some());
        assert!(t.ends_with("fim —"));
    }

    #[test]
    fn list_levels_and_indents() {
        let st = styles();
        let (_, lines) = highlight_lines("- a\n    - b\n  continua\n- [x] c\n\n> q\n", &st, Reveal(None));
        let step = list_step(16.0);
        assert_eq!(lines[0].deco, Deco::Bullet(0));
        assert_eq!(lines[1].deco, Deco::Bullet(1));
        assert_eq!(lines[1].content_x, step * 2.0);
        assert_eq!(lines[2].content_x, step * 2.0, "a continuation sits under its item's text");
        assert_eq!(lines[3].deco, Deco::Task(0, true));
        assert_eq!(lines[5].deco, Deco::Quote(1));
        assert_eq!(lines.len(), 7, "the empty line after the last newline counts");
        // on the cursor's line the mark shows instead of the bullet
        let (_, lines) = highlight_lines("- a\n- b", &st, Reveal(Some((1, 1))));
        assert_eq!(lines[0].deco, Deco::None);
        assert_eq!(lines[1].deco, Deco::Bullet(0));
    }

    #[test]
    fn composed_galley_holds_every_character() {
        let st = styles();
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        for text in ["", "a", "a\n", "# T\n\npara **b** [[x|y]]\n- one two three four five six seven eight nine ten\n    - b\n> q\n```\ncode\n```\n", "\n\n"] {
            for a in crate::align::ALL {
                crate::align::set_current(a);
                let mut cache = HighlightCache::default();
                let (g, _) = cache.live(&fonts, text, &st, Reveal(None), 120.0);
                let n: usize = g.rows.iter().map(|r| r.char_count_including_newline()).sum();
                assert_eq!(n, text.chars().count(), "{text:?} {a:?}");
                assert_eq!(g.rows.iter().filter(|r| r.ends_with_newline).count(), text.matches('\n').count());
                assert!(!g.rows.last().unwrap().ends_with_newline);
            }
        }
        crate::align::set_current(crate::align::TextAlign::Left);
    }
}
