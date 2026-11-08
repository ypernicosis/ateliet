//! The note editor: a text area with Markdown styled as you type (headings
//! larger, emphasis, code, links, tags, frontmatter and code blocks set apart).

use crate::theme::Palette;
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Stroke, TextFormat};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// The inline spans, in the order they are tried at a position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Span {
    Escape,
    Code,
    Wiki,
    MdLink,
    Url,
    Bold,
    Italic,
    Strike,
    Mark,
    Field,
    Tag,
    Comment,
}

const SPANS: [(Span, &str); 12] = [
    (Span::Escape, r"\\[[:punct:]]"),
    (Span::Code, r"`[^`\n]+`"),
    (Span::Wiki, r"!?\[\[[^\]\n]+\]\]"),
    (Span::MdLink, r"!?\[[^\]\n]*\]\([^)\n]*\)"),
    (Span::Url, r"https?://[^\s)>\]]+"),
    (Span::Bold, r"\*\*[^*\n]+\*\*|__[^_\n]+__"),
    // `_italic_` also needs no letter right before or after it (see `word_bounded`)
    (Span::Italic, r"\*[^*\s][^*\n]*\*|_[^_\s][^_\n]*_"),
    (Span::Strike, r"~~[^~\n]+~~"),
    (Span::Mark, r"==[^=\n]+=="),
    (Span::Field, r"\[[^\[\]\n]+?::[^\]\n]*\]"),
    (Span::Tag, r"(?:^|\s)#[^\s#!-,.:-@\[-^`{-~]+"),
    (Span::Comment, r"%%[^%]*%%"),
];

/// All the spans, without capture groups: finding them takes the regex
/// engine's fast path, where telling the groups apart would not.
static INLINE: Lazy<Regex> = Lazy::new(|| Regex::new(&SPANS.iter().map(|(_, p)| format!("(?:{p})")).collect::<Vec<_>>().join("|")).unwrap());

/// Each span alone, matching a whole text.
static WHOLE: Lazy<Vec<(Span, Regex)>> = Lazy::new(|| SPANS.iter().map(|(k, p)| (*k, Regex::new(&format!("^(?:{p})$")).unwrap())).collect());

/// `_italic_` stands apart from the words around it, as a regex `\b` would
/// say. The `\b` is checked here rather than in the regex: with it, the
/// regex leaves its fast automaton for a backtracking search on any text
/// with accents.
fn word_bounded(text: &str, start: usize, end: usize) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    !text[..start].chars().next_back().is_some_and(word) && !text[end..].chars().next().is_some_and(word)
}

/// The inline spans of `text`, as the alternation finds them from left to right.
fn spans(text: &str) -> impl Iterator<Item = (Span, usize, usize)> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || loop {
        let m = INLINE.find_at(text, at)?;
        let kind = span_of(m.as_str());
        if kind == Span::Italic && m.as_str().starts_with('_') && !word_bounded(text, m.start(), m.end()) {
            // no `_italic_` here: the search goes on after its first underscore
            at = m.start() + 1;
            continue;
        }
        at = if m.end() > m.start() { m.end() } else { m.end() + 1 };
        return Some((kind, m.start(), m.end()));
    })
}

/// Which span a match of `INLINE` is: the first that matches all of it,
/// as the alternation tried them.
fn span_of(s: &str) -> Span {
    WHOLE.iter().find(|(_, r)| r.is_match(s)).map(|(k, _)| *k).unwrap_or(Span::Comment)
}

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
    Rule,
    Table { start: usize, end: usize },
}

/// The width of a list item's marker box, plus the gap after it, as the
/// reading view lays out lists.
pub fn list_step(size: f32) -> f32 {
    size * 1.3 + 4.0
}

static QUOTE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(>\s?)+").unwrap());

fn markdown_rule(body: &str) -> bool {
    if body.len() - body.trim_start_matches(' ').len() > 3 { return false; }
    let mut chars = body.chars().filter(|c| !matches!(c, ' ' | '\t' | '\r'));
    let Some(mark) = chars.next() else { return false };
    matches!(mark, '-' | '*' | '_') && chars.clone().count() >= 2 && chars.all(|c| c == mark)
}

/// Use the Markdown parser so pipes in code, escapes and fenced examples
/// cannot accidentally turn ordinary text into a preview table.
fn table_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut tables = Vec::new();
    let mut start = None;
    for (event, range) in Parser::new_ext(text, ateliet_core::syntax::md_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Table(_)) => start = Some(range.start),
            Event::End(TagEnd::Table) => {
                if let Some(a) = start.take() { tables.push(a..range.end); }
            }
            _ => {}
        }
    }
    tables
}

/// A preview row keeps its cells' source ranges and declared alignments.
fn table_cells(text: &str) -> (Vec<Vec<std::ops::Range<usize>>>, Vec<pulldown_cmark::Alignment>) {
    use pulldown_cmark::{Event, Parser, Tag};
    let mut rows: Vec<Vec<std::ops::Range<usize>>> = Vec::new();
    let mut align = Vec::new();
    for (event, range) in Parser::new_ext(text, ateliet_core::syntax::md_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Table(a)) => align = a,
            Event::Start(Tag::TableHead | Tag::TableRow) => rows.push(Vec::new()),
            Event::Start(Tag::TableCell) => {
                if let Some(row) = rows.last_mut() { row.push(range); }
            }
            _ => {}
        }
    }
    (rows, align)
}

/// What a line takes from the lines above it: whether it is in the
/// frontmatter or in a code block, the indentation of the open list items,
/// and whether the line before was a list item.
#[derive(Clone, Default, Hash, PartialEq, Eq, Debug)]
struct Carry {
    in_fm: bool,
    in_code: bool,
    list_cols: Vec<usize>,
    prev_list: bool,
}

/// The cursor or selection as one line sees it: the part of it on the line,
/// counted from the line's start (`end` is where its text ends).
fn local_reveal(reveal: Reveal, at: usize, end: usize) -> Reveal {
    match reveal.0 {
        Some((x, y)) if x <= end && y >= at => Reveal(Some((x.max(at) - at, y.min(end) - at))),
        _ => Reveal(None),
    }
}

/// One line of the note, styled: its sections, their bytes counted from the
/// line's start, and how it sits. `line` holds its newline, if it has one;
/// `reveal` is counted from the line's start too; `table`: the line is a row
/// of a table drawn as a table (its deco then holds no position).
fn highlight_line(st: &Styles, carry: &mut Carry, first: bool, line: &str, table: bool, reveal: Reveal) -> (Vec<egui::text::LayoutSection>, LineInfo) {
    let mut job = LayoutJob::default();
    let p = &st.p;
    let base = st.fmt(st.size, p.text);
    let line_at = 0;
    let Carry { in_fm, in_code, list_cols, prev_list } = carry;
    let body = line.trim_end_matches(['\n', '\r']);
    let mut info = LineInfo { sections: 0..0, content_x: 0.0, lead: 0, align: LineAlign::Note, deco: Deco::None, gap: 0.0, newline: line.ends_with('\n') };
    // in Live Preview, line marks (#, >, -) show on the cursor's line only
    let on_line = !st.live || reveal.touches(line_at, line_at + body.len());
    let trimmed = body.trim_start();
    let is_list = !*in_fm && !*in_code && LIST.is_match(body);
    if !is_list && !body.trim().is_empty() && !body.starts_with([' ', '\t']) {
        list_cols.clear();
    }
        'line: {
            if table {
                job.append(line, 0.0, hidden(&base));
                info.align = LineAlign::Left;
                info.deco = Deco::Table { start: 0, end: 0 };
                break 'line;
            }
            // frontmatter
            if first && body.trim_end() == "---" {
                *in_fm = true;
                job.append(line, 0.0, st.mono(p.faint));
                info.align = LineAlign::Left;
                break 'line;
            }
            if *in_fm {
                info.align = LineAlign::Left;
                if body.trim_end() == "---" {
                    *in_fm = false;
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
                *in_code = !*in_code;
                job.append(line, 0.0, st.code(p.faint));
                info.align = LineAlign::Left;
                info.deco = Deco::Code;
                info.content_x = if st.live { 16.0 } else { 0.0 };
                break 'line;
            }
            if *in_code {
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
                inline(&mut job, &line[h[0].len()..], h[0].len(), f, st, reveal);
                break 'line;
            }
            let mut rest = line;
            let mut rest_at = 0;
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
                if *prev_list {
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
            } else if markdown_rule(body) {
                if st.live && !on_line {
                    job.append(line, 0.0, hidden(&base));
                    info.deco = Deco::Rule;
                } else {
                    job.append(line, 0.0, st.fmt(st.size, p.faint));
                }
                break 'line;
            } else if !list_cols.is_empty() && !body.trim().is_empty() {
                // a continuation of a list item: under its text
                info.content_x = if st.live { list_step(st.size) * list_cols.len() as f32 } else { 0.0 };
                info.align = LineAlign::List;
            }
            inline(&mut job, rest, rest_at, fmt, st, reveal);
        }
        *prev_list = is_list || (*prev_list && !body.trim().is_empty() && body.starts_with([' ', '\t']));
    if job.sections.is_empty() {
        job.append("", 0.0, base);
    }
    info.sections = 0..job.sections.len();
    (job.sections, info)
}

/// The empty line after a final newline (or of an empty note).
fn final_line(st: &Styles) -> (Vec<egui::text::LayoutSection>, LineInfo) {
    let mut job = LayoutJob::default();
    job.append("", 0.0, st.fmt(st.size, st.p.text));
    (job.sections, LineInfo { sections: 0..1, content_x: 0.0, lead: 0, align: LineAlign::Note, deco: Deco::None, gap: 0.0, newline: false })
}

/// The styled layout of a whole note, and how each line sits in Live Preview.
#[cfg(test)]
pub fn highlight_lines(text: &str, st: &Styles, reveal: Reveal) -> (LayoutJob, Vec<LineInfo>) {
    let mut job = LayoutJob { text: text.to_owned(), ..Default::default() };
    let mut lines = Vec::new();
    let tables = if st.live { table_ranges(text) } else { Vec::new() };
    let mut carry = Carry::default();
    let mut at = 0;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let end = at + line.trim_end_matches(['\n', '\r']).len();
        let table = tables.iter().find(|r| r.start < end && r.end > at && !reveal.touches(r.start, r.end.saturating_sub(1)));
        let local = if st.live { local_reveal(reveal, at, end) } else { Reveal(None) };
        let (sections, mut info) = highlight_line(st, &mut carry, n == 0, line, table.is_some(), local);
        if let Some(r) = table {
            info.deco = Deco::Table { start: r.start, end: r.end };
        }
        push_line(&mut job, &mut lines, sections, info, at);
        at += line.len();
    }
    if text.is_empty() || text.ends_with('\n') {
        let (sections, info) = final_line(st);
        push_line(&mut job, &mut lines, sections, info, text.len());
    }
    (job, lines)
}

/// Adds one line, styled on its own, to the note's job at byte `at`.
#[cfg(test)]
fn push_line(job: &mut LayoutJob, lines: &mut Vec<LineInfo>, sections: Vec<egui::text::LayoutSection>, mut info: LineInfo, at: usize) {
    let s0 = job.sections.len();
    for mut sec in sections {
        sec.byte_range = sec.byte_range.start + at..sec.byte_range.end + at;
        job.sections.push(sec);
    }
    info.sections = s0..job.sections.len();
    lines.push(info);
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
    for (kind, start, end) in spans(text) {
        let mut start = start;
        if kind == Span::Tag && text[start..].starts_with(char::is_whitespace) {
            start += text[start..].chars().next().unwrap().len_utf8();
        }
        if start > at {
            job.append(&text[at..start], 0.0, base.clone());
        }
        let s = &text[start..end];
        let marks = Marks { p: *p, hide: st.live && !reveal.touches(offset + start, offset + end) };
        let mut f = base.clone();
        if kind == Span::Escape {
            let m = marks.mark(&f);
            job.append(&s[..1], 0.0, m);
            job.append(&s[1..], 0.0, f);
        } else if kind == Span::Code {
            f = st.mono(p.text);
            f.background = p.code_bg;
            wrapped(job, s, 1, 1, f, &marks);
        } else if kind == Span::Wiki {
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
        } else if kind == Span::MdLink {
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
        } else if kind == Span::Url {
            f.color = p.link();
            f.underline = Stroke::new(1.0_f32, p.link().gamma_multiply(0.4));
            job.append(s, 0.0, f);
        } else if kind == Span::Bold {
            f.font_id = FontId::new(f.font_id.size, bold_family());
            if let Some(c) = st.typo.bold.filter(|_| st.live) {
                f.color = c;
            }
            wrapped(job, s, 2, 2, f, &marks);
        } else if kind == Span::Italic {
            st.italic(&mut f);
            wrapped(job, s, 1, 1, f, &marks);
        } else if kind == Span::Strike {
            f.strikethrough = Stroke::new(1.0_f32, f.color);
            wrapped(job, s, 2, 2, f, &marks);
        } else if kind == Span::Mark {
            f.background = p.highlight;
            wrapped(job, s, 2, 2, f, &marks);
        } else if kind == Span::Field {
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
        } else if kind == Span::Tag {
            f.color = p.accent_strong;
            if st.live {
                f.background = p.accent_soft;
            }
            job.append(s, 0.0, f);
        } else if kind == Span::Comment {
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

/// The editor's layout of a note, kept from frame to frame and redone line by
/// line: an edit, or the cursor moving into a span whose marks it shows, lays
/// out again only the lines it changed; the others keep their rows, moved down
/// or up when the lines above them grow or shrink.
///
/// Two layouts are kept: egui's text edit lays the note out once before the
/// keys of a frame and once after them, and still holds the first while the
/// second is made, so the second is made by reworking the other one.
///
/// While a text edit holds the layout (`lend`), the cache keeps no strong
/// reference to it: egui paints a selection into the galley itself and copies
/// a shared galley first, which for a long note is the whole layout, every
/// frame. The editor gives it back after the frame ([`HighlightCache::give_back`])
/// and the selection is taken out of it before it is used again.
#[derive(Default)]
pub struct HighlightCache {
    pub(crate) rendered: Option<(Arc<egui::Galley>, egui::Pos2)>,
    cur: Option<Layout>,
    spare: Option<Layout>,
    /// The spare being copied from the current layout, out of the frame.
    copying: Option<std::thread::JoinHandle<Layout>>,
    lend: bool,
    /// When the editor last had the keyboard: the spare is let go after a while
    /// without it (it doubles the memory of a long note).
    focused_at: Option<std::time::Instant>,
    /// The tables of the last text, by its hash.
    tables: (u64, Vec<std::ops::Range<usize>>),
}

/// One layout of the note.
struct Layout {
    galley: Option<Arc<egui::Galley>>,
    /// The galley while a text edit holds it.
    lent: std::sync::Weak<egui::Galley>,
    lines: Vec<Placed>,
    /// Each row's vertices as laid out (a painted selection adds some).
    verts: Vec<u32>,
    /// The width, alignment and styles it was laid out with.
    params: u64,
    /// `params`, the text and what the cursor reveals.
    key: u64,
    decos: Arc<Vec<DecoShape>>,
}

/// A line of the layout.
struct Placed {
    /// What the line's look depends on besides its text: what it takes from
    /// the lines above, whether it is the first, its table, the cursor on it.
    ctx: u64,
    /// Neither the cursor nor a drawn table is on it: its look depends on its
    /// text and the lines above only.
    quiet: bool,
    /// What it passes on to the line below.
    carry: Carry,
    /// Its bytes, its newline included.
    len: usize,
    newline: bool,
    sections: usize,
    rows: usize,
    gap: f32,
    /// Where its rows start (after the gap) and how tall they are.
    top: f32,
    height: f32,
    /// Its first row, from `top`.
    first: egui::Rect,
    deco: Deco,
    fence: bool,
    /// A table row: its height, whether it is the header, its columns, and
    /// its cells (from `top`).
    table: Option<(f32, bool, usize)>,
    cells: Vec<(egui::Pos2, Arc<egui::Galley>)>,
}

/// One line laid out at the top of its own room: rows set across, from y = 0,
/// their sections counted from the line's first.
struct LineLayout {
    rows: Vec<egui::epaint::text::Row>,
    height: f32,
    first: egui::Rect,
    table: Option<(f32, bool, usize)>,
    cells: Vec<(egui::Pos2, Arc<egui::Galley>)>,
}

fn ahash_of(v: impl Hash) -> u64 {
    egui::ahash::RandomState::with_seeds(0x41, 0x74, 0x65, 0x6c).hash_one(v)
}

/// The bytes `a` and `b` share at their start and, after those, at their end.
fn common_affixes(a: &str, b: &str) -> (usize, usize) {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let n = x.len().min(y.len());
    let mut p = 0;
    while p + 64 <= n && x[p..p + 64] == y[p..p + 64] {
        p += 64;
    }
    while p < n && x[p] == y[p] {
        p += 1;
    }
    let m = n - p;
    let mut s = 0;
    while s + 64 <= m && x[x.len() - s - 64..x.len() - s] == y[y.len() - s - 64..y.len() - s] {
        s += 64;
    }
    while s < m && x[x.len() - 1 - s] == y[y.len() - 1 - s] {
        s += 1;
    }
    (p, s)
}

impl Layout {
    fn empty(params: u64) -> Layout {
        Layout { galley: None, lent: std::sync::Weak::new(), lines: Vec::new(), verts: Vec::new(), params, key: 0, decos: Arc::default() }
    }

    /// A copy, rows and all.
    fn duplicate(&self) -> Layout {
        Layout {
            galley: self.galley.as_ref().map(|g| Arc::new((**g).clone())),
            lent: std::sync::Weak::new(),
            lines: self.lines.iter().map(Placed::copy).collect(),
            verts: self.verts.clone(),
            params: self.params,
            key: self.key,
            decos: self.decos.clone(),
        }
    }

    /// Whether the cache alone holds the galley, so it can be reworked in place.
    fn unique(&self) -> bool {
        self.galley.as_ref().is_some_and(|g| Arc::strong_count(g) == 1 && Arc::weak_count(g) == 0)
    }

    /// Takes a selection egui painted into the rows out of them again.
    fn clean(&mut self) {
        let Some(g) = self.galley.as_mut() else { return };
        let dirty = |g: &egui::Galley| g.rows.iter().zip(&self.verts).any(|(r, &n)| r.visuals.mesh.vertices.len() != n as usize);
        if !dirty(g) {
            return;
        }
        let g = Arc::make_mut(g);
        for (row, &n) in g.rows.iter_mut().zip(&self.verts) {
            let mesh = &mut row.visuals.mesh;
            let n = n as usize;
            if mesh.vertices.len() > n {
                // a rectangle is four vertices, and its two triangles were put first among the glyphs'
                let rects = (mesh.vertices.len() - n) / 4;
                let at = row.visuals.glyph_index_start;
                mesh.indices.drain(at..(at + 6 * rects).min(mesh.indices.len()));
                mesh.vertices.truncate(n);
                row.visuals.mesh_bounds = mesh.calc_bounds();
            }
        }
    }
}

impl HighlightCache {
    /// The note laid out at `width`, and what to draw with it.
    pub fn live(&mut self, fonts: &egui::text::Fonts, text: &str, st: &Styles, reveal: Reveal, width: f32) -> (Arc<egui::Galley>, Arc<Vec<DecoShape>>) {
        let note = crate::align::current();
        let params = ahash_of((st.size.to_bits(), st.p.dark, st.p.accent, st.live, st.typo_key, width.to_bits(), note as u8));
        let reveal = if st.live { reveal } else { Reveal(None) };
        let hash = text_hash(text);
        let key = ahash_of((params, hash, text.len(), reveal));
        if let Some(cur) = self.cur.as_mut() {
            if cur.galley.is_none() {
                cur.galley = cur.lent.upgrade();
                cur.lent = std::sync::Weak::new();
            }
        }
        if self.cur.as_ref().is_some_and(|c| c.key == key && c.galley.is_some()) {
            let cur = self.cur.as_mut().unwrap();
            cur.clean();
            return self.hand_out();
        }
        // a layout to rework: the spare, else the current one if nothing else holds it
        // the spare copied out of the frame, if it is still on its way
        self.settle_copy(true);
        let old = self.cur.take().filter(|o| o.params == params && o.galley.is_some());
        let mut spare = self.spare.take().filter(|s| s.params == params);
        let mut work = if spare.as_ref().is_some_and(|s| s.unique()) {
            self.spare = old;
            spare.take().unwrap()
        } else {
            match old {
                Some(o) if o.unique() => {
                    self.spare = spare;
                    o
                }
                Some(o) => {
                    // held elsewhere (by the text edit): rework a copy, keep it as the spare
                    let copy = o.duplicate();
                    self.spare = Some(o);
                    copy
                }
                None => Layout::empty(params),
            }
        };
        let whole = work.galley.is_none();
        work.clean();
        self.update_tables(text, hash, st);
        work.update(fonts, text, st, reveal, width, note, &self.tables.1);
        work.key = key;
        self.cur = Some(work);
        if whole && self.lend {
            // the text edit will hold this one when the first key is typed
            self.copy_spare();
        }
        self.hand_out()
    }

    /// Starts copying the current layout into a spare, on another thread:
    /// a long note's layout takes a moment to copy.
    fn copy_spare(&mut self) {
        if self.spare.is_some() || self.copying.is_some() {
            return;
        }
        let Some(cur) = self.cur.as_ref() else { return };
        let Some(galley) = cur.galley.clone().or_else(|| cur.lent.upgrade()) else { return };
        let mut copy = Layout { galley: None, lent: std::sync::Weak::new(), lines: cur.lines.iter().map(Placed::copy).collect(), verts: cur.verts.clone(), params: cur.params, key: cur.key, decos: cur.decos.clone() };
        self.copying = Some(std::thread::spawn(move || {
            copy.galley = Some(Arc::new((*galley).clone()));
            copy
        }));
    }

    /// Takes the spare copied on another thread once it is done (or, with
    /// `wait`, waits for it).
    fn settle_copy(&mut self, wait: bool) {
        if self.copying.as_ref().is_some_and(|h| wait || h.is_finished()) {
            let copy = self.copying.take().unwrap().join().ok();
            if self.spare.is_none() {
                self.spare = copy;
            }
        }
    }

    /// Lets the spare go when the editor has not had the keyboard for a
    /// while; it is copied again when it gets it back.
    pub fn trim(&mut self) {
        const IDLE: std::time::Duration = std::time::Duration::from_secs(20);
        if self.focused_at.is_none_or(|t| t.elapsed() > IDLE) && (self.spare.is_some() || self.copying.is_some()) {
            self.spare = None;
            self.copying = None;
        }
    }

    fn hand_out(&mut self) -> (Arc<egui::Galley>, Arc<Vec<DecoShape>>) {
        let cur = self.cur.as_mut().unwrap();
        let g = cur.galley.clone().unwrap();
        if self.lend {
            cur.lent = Arc::downgrade(&g);
            cur.galley = None;
        }
        (g, cur.decos.clone())
    }

    fn update_tables(&mut self, text: &str, hash: u64, st: &Styles) {
        if !st.live {
            self.tables = (0, Vec::new());
            return;
        }
        let k = ahash_of((hash, text.len()));
        if self.tables.0 != k || k == 0 {
            // a table needs a delimiter row, and that a pipe
            self.tables = (k, if text.contains('|') { table_ranges(text) } else { Vec::new() });
        }
    }

    /// The galley a text edit used this frame, given back to the cache.
    pub(crate) fn give_back(&mut self, galley: &Arc<egui::Galley>, focused: bool) {
        self.lend = false;
        if let Some(cur) = self.cur.as_mut().filter(|c| c.galley.is_none()) {
            cur.galley = Some(galley.clone());
            cur.lent = std::sync::Weak::new();
        }
        self.settle_copy(false);
        if focused {
            self.focused_at = Some(std::time::Instant::now());
            // typing is near: the spare is made ready
            self.copy_spare();
        }
    }

    /// What the current layout draws around the text.
    pub(crate) fn decos(&self) -> Arc<Vec<DecoShape>> {
        self.cur.as_ref().map(|c| c.decos.clone()).unwrap_or_default()
    }
}

impl Placed {
    fn copy(&self) -> Placed {
        Placed { carry: self.carry.clone(), cells: self.cells.clone(), deco: self.deco, ..*self }
    }
}

impl Layout {
    /// Lays out `text`, keeping the rows of the lines that did not change.
    ///
    /// First what each line becomes is worked out, laying out again only the
    /// lines whose text or look changed. Then, if every line kept its count
    /// of rows and sections (a letter typed, the cursor moved), the rows are
    /// rewritten where they are; otherwise the rows and sections are moved
    /// into new vectors around the lines laid out again.
    #[allow(clippy::too_many_arguments)]
    fn update(&mut self, fonts: &egui::text::Fonts, text: &str, st: &Styles, reveal: Reveal, width: f32, note: crate::align::TextAlign, tables: &[std::ops::Range<usize>]) {
        let Layout { galley, lines: old_lines, verts, decos, .. } = self;
        let galley = galley.get_or_insert_with(|| {
            Arc::new(egui::Galley {
                job: Arc::new(LayoutJob::default()),
                rows: Vec::new(),
                elided: false,
                rect: egui::Rect::ZERO,
                mesh_bounds: egui::Rect::NOTHING,
                num_vertices: 0,
                num_indices: 0,
                pixels_per_point: fonts.pixels_per_point(),
            })
        });
        let g = Arc::get_mut(galley).expect("the layout being reworked is the cache's alone");
        let job = Arc::make_mut(&mut g.job);
        let old_text = std::mem::take(&mut job.text);

        // the lines the edit did not touch, at the start and at the end
        let (pre, suf) = common_affixes(&old_text, text);
        let (old_len, new_len) = (old_text.len(), text.len());
        let (mut a, mut head) = (0, 0);
        while a < old_lines.len() && old_lines[a].newline && head + old_lines[a].len <= pre {
            head += old_lines[a].len;
            a += 1;
        }
        let (mut b, mut tail) = (old_lines.len(), old_len);
        while b > a {
            let start = tail - old_lines[b - 1].len;
            if start + suf < old_len || start < head {
                break;
            }
            // where it starts now, which must still be a line's start
            let new_start = start + new_len - old_len;
            let boundary = start + suf > old_len || new_start == 0 || text.as_bytes()[new_start - 1] == b'\n';
            if !boundary || new_start < head {
                break;
            }
            tail = start;
            b -= 1;
        }
        let mid_end = tail + new_len - old_len;
        let mut srcs: Vec<(Option<usize>, usize)> = Vec::with_capacity(old_lines.len() + 8);
        srcs.extend((0..a).map(|i| (Some(i), old_lines[i].len)));
        srcs.extend(text[head..mid_end].split_inclusive('\n').map(|l| (None, l.len())));
        if b == old_lines.len() && (text.is_empty() || text.ends_with('\n')) {
            srcs.push((None, 0));
        }
        srcs.extend((b..old_lines.len()).map(|i| (Some(i), old_lines[i].len)));

        // what each line becomes, and where it now starts
        enum Fate {
            Keep(usize),
            Fresh(Box<(LineLayout, Vec<egui::text::LayoutSection>, Placed)>),
        }
        let table_keys: Vec<u64> = tables.iter().map(|r| ahash_of(&text[r.clone()])).collect();
        let mut table_cells_cache = std::collections::HashMap::new();
        let mut fates = Vec::with_capacity(srcs.len());
        let mut tops = Vec::with_capacity(srcs.len());
        // what the line above passes on: `carry`, or that of an old line kept
        let mut carry = Carry::default();
        let mut carry_of: Option<usize> = None;
        let mut prev_kept: Option<usize> = None;
        let (mut at, mut y, mut ti) = (0, 0.0_f32, 0);
        let n_lines = srcs.len();
        for (k, &(src, len)) in srcs.iter().enumerate() {
            let line = &text[at..at + len];
            let pseudo = len == 0 && k + 1 == n_lines;
            let end = at + line.trim_end_matches(['\n', '\r']).len();
            while tables.get(ti).is_some_and(|r| r.end <= at) {
                ti += 1;
            }
            let table = tables.get(ti).filter(|r| !pseudo && r.start < end && r.end > at && !reveal.touches(r.start, r.end.saturating_sub(1)));
            let local = local_reveal(reveal, at, end);
            let quiet = local.0.is_none() && table.is_none();
            // an untouched line under the old line above it, as plain as it was: as it was
            let fast = src.is_some_and(|i| i > 0 && k > 0 && prev_kept == Some(i - 1) && quiet && old_lines[i].quiet);
            let mut ctx = 0;
            if !fast {
                if let Some(j) = carry_of.take() {
                    carry.clone_from(&old_lines[j].carry);
                }
                ctx = ahash_of((&carry, k == 0, pseudo, table.map(|r| (table_keys[ti], at - r.start)), local));
            }
            let keep = src.filter(|&i| fast || old_lines[i].ctx == ctx);
            if let Some(i) = keep {
                let o = &old_lines[i];
                y += o.gap;
                tops.push(y);
                y += o.height;
                carry_of = Some(i);
                fates.push(Fate::Keep(i));
            } else {
                let (sections, mut info) = if pseudo { final_line(st) } else { highlight_line(st, &mut carry, k == 0, line, table.is_some(), local) };
                if !st.live {
                    info.gap = 0.0;
                    info.lead = 0;
                }
                let table_src = table.map(|r| {
                    let cells = table_cells_cache.entry(r.start).or_insert_with(|| table_cells(&text[r.clone()]));
                    (&text[r.clone()], &*cells, at - r.start)
                });
                let laid = layout_line(fonts, &line[..end - at], &sections, &info, table_src, width, note, st);
                y += info.gap;
                tops.push(y);
                y += laid.height;
                let placed = Placed {
                    ctx,
                    quiet,
                    carry: carry.clone(),
                    len,
                    newline: info.newline,
                    sections: sections.len(),
                    rows: laid.rows.len(),
                    gap: info.gap,
                    top: 0.0,
                    height: laid.height,
                    first: laid.first,
                    deco: info.deco,
                    fence: matches!(line.trim_start().get(..3), Some("```") | Some("~~~")),
                    table: laid.table,
                    cells: laid.cells.clone(),
                };
                fates.push(Fate::Fresh(Box::new((laid, sections, placed))));
            }
            prev_kept = keep;
            at += len;
        }

        // a fresh line's rows, moved to its place in the note
        let place = |row: &mut egui::epaint::text::Row, top: f32, s0: usize| {
            let d = egui::vec2(0.0, top.round());
            for gl in &mut row.glyphs {
                gl.pos += d;
                gl.section_index += s0 as u32;
            }
            row.section_index_at_start += s0 as u32;
            row.rect = row.rect.translate(d);
            row.visuals.mesh.translate(d);
            row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(d);
        };
        // the rows of lines kept that move: gathered in runs, moved together at the end
        let mut runs = Runs::default();
        let shift_secs = |secs: &mut [egui::text::LayoutSection], d: i64| {
            for sec in secs {
                sec.byte_range = (sec.byte_range.start as i64 + d) as usize..(sec.byte_range.end as i64 + d) as usize;
            }
        };
        let in_place = fates.len() == old_lines.len()
            && fates.iter().enumerate().all(|(k, f)| match f {
                Fate::Keep(i) => *i == k,
                Fate::Fresh(f) => old_lines[k].rows == f.2.rows && old_lines[k].sections == f.2.sections,
            });
        if in_place {
            let (mut r, mut s, mut at, mut old_at) = (0, 0, 0, 0);
            for (k, fate) in fates.into_iter().enumerate() {
                let o = &mut old_lines[k];
                let (rows_n, secs_n, old_line_len) = (o.rows, o.sections, o.len);
                let top = tops[k];
                match fate {
                    Fate::Keep(_) => {
                        let dy = top.round() - o.top.round();
                        runs.add(r..r + rows_n, dy, 0);
                        if at != old_at {
                            shift_secs(&mut job.sections[s..s + secs_n], at as i64 - old_at as i64);
                        }
                        o.top = top;
                    }
                    Fate::Fresh(f) => {
                        let (laid, sections, mut placed) = *f;
                        for (j, mut row) in laid.rows.into_iter().enumerate() {
                            place(&mut row, top, s);
                            verts[r + j] = row.visuals.mesh.vertices.len() as u32;
                            g.rows[r + j] = row;
                        }
                        for (j, mut sec) in sections.into_iter().enumerate() {
                            sec.byte_range = sec.byte_range.start + at..sec.byte_range.end + at;
                            job.sections[s + j] = sec;
                        }
                        placed.top = top;
                        *o = placed;
                    }
                }
                at += o.len;
                old_at += old_line_len;
                r += rows_n;
                s += secs_n;
            }
        } else {
            // the stretches of old lines replaced by fresh ones, and what moves for the lines kept
            struct Edit {
                old: std::ops::Range<usize>,
                rows: std::ops::Range<usize>,
                secs: std::ops::Range<usize>,
                fresh: Vec<(LineLayout, Vec<egui::text::LayoutSection>, Placed)>,
            }
            let mut edits: Vec<Edit> = Vec::new();
            let mut moves = Vec::with_capacity(fates.len()); // per line kept: dy, dsec, dbyte
            let (mut oi, mut o_row, mut o_sec, mut o_at) = (0, 0, 0, 0);
            let (mut n_row, mut n_sec, mut at) = (0, 0, 0);
            let mut open: Option<Edit> = None;
            for (k, fate) in fates.into_iter().enumerate() {
                let top = tops[k];
                match fate {
                    Fate::Keep(i) => {
                        let e = open.get_or_insert_with(|| Edit { old: oi..oi, rows: o_row..o_row, secs: o_sec..o_sec, fresh: Vec::new() });
                        while oi < i {
                            let o = &old_lines[oi];
                            (o_row, o_sec, o_at) = (o_row + o.rows, o_sec + o.sections, o_at + o.len);
                            oi += 1;
                        }
                        (e.old.end, e.rows.end, e.secs.end) = (oi, o_row, o_sec);
                        if let Some(e) = open.take().filter(|e| !e.old.is_empty() || !e.fresh.is_empty()) {
                            edits.push(e);
                        }
                        let o = &old_lines[i];
                        moves.push(Some((top.round() - o.top.round(), n_sec as i64 - o_sec as i64, at as i64 - o_at as i64, top)));
                        (n_row, n_sec, at) = (n_row + o.rows, n_sec + o.sections, at + o.len);
                        (o_row, o_sec, o_at) = (o_row + o.rows, o_sec + o.sections, o_at + o.len);
                        oi = i + 1;
                    }
                    Fate::Fresh(f) => {
                        let (mut laid, mut sections, mut placed) = *f;
                        for row in &mut laid.rows {
                            place(row, top, n_sec);
                        }
                        for sec in &mut sections {
                            sec.byte_range = sec.byte_range.start + at..sec.byte_range.end + at;
                        }
                        placed.top = top;
                        (n_row, n_sec, at) = (n_row + placed.rows, n_sec + placed.sections, at + placed.len);
                        moves.push(None);
                        open.get_or_insert_with(|| Edit { old: oi..oi, rows: o_row..o_row, secs: o_sec..o_sec, fresh: Vec::new() }).fresh.push((laid, sections, placed));
                    }
                }
            }
            // the old lines after the last one kept go too
            let mut e = open.take().unwrap_or(Edit { old: oi..oi, rows: o_row..o_row, secs: o_sec..o_sec, fresh: Vec::new() });
            (e.old.end, e.rows.end, e.secs.end) = (old_lines.len(), g.rows.len(), job.sections.len());
            if !e.old.is_empty() || !e.fresh.is_empty() {
                edits.push(e);
            }
            // from the end, so the ranges before stay where they are
            for e in edits.into_iter().rev() {
                let mut new_rows = Vec::new();
                let mut new_verts = Vec::new();
                let mut new_secs = Vec::new();
                let mut new_lines = Vec::new();
                for (laid, sections, placed) in e.fresh {
                    new_verts.extend(laid.rows.iter().map(|r| r.visuals.mesh.vertices.len() as u32));
                    new_rows.extend(laid.rows);
                    new_secs.extend(sections);
                    new_lines.push(placed);
                }
                verts.splice(e.rows.clone(), new_verts);
                g.rows.splice(e.rows, new_rows);
                job.sections.splice(e.secs, new_secs);
                old_lines.splice(e.old, new_lines);
            }
            // the lines kept move down or up, and their sections and bytes on
            let (mut r, mut s) = (0, 0);
            for (line, mv) in old_lines.iter_mut().zip(moves) {
                if let Some((dy, dsec, dbyte, top)) = mv {
                    runs.add(r..r + line.rows, dy, dsec);
                    if dbyte != 0 {
                        shift_secs(&mut job.sections[s..s + line.sections], dbyte);
                    }
                    line.top = top;
                }
                r += line.rows;
                s += line.sections;
            }
        }

        runs.apply(&mut g.rows);
        job.text = text.to_owned();
        let mut bounds = egui::Rect::NOTHING;
        let (mut nv, mut ni) = (0, 0);
        for row in &g.rows {
            bounds = bounds.union(row.visuals.mesh_bounds);
            nv += row.visuals.mesh.vertices.len();
            ni += row.visuals.mesh.indices.len();
        }
        g.rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(width, y));
        g.mesh_bounds = bounds;
        g.num_vertices = nv;
        g.num_indices = ni;
        g.elided = false;
        g.pixels_per_point = fonts.pixels_per_point();
        *decos = Arc::new(if st.live { decos_of(old_lines, width, st) } else { Vec::new() });
    }
}

/// Rows that move down or up (and whose sections are renumbered), gathered
/// in runs of the same move.
#[derive(Default)]
struct Runs(Vec<(std::ops::Range<usize>, f32, i64)>);

impl Runs {
    fn add(&mut self, rows: std::ops::Range<usize>, dy: f32, dsec: i64) {
        if (dy == 0.0 && dsec == 0) || rows.is_empty() {
            return;
        }
        match self.0.last_mut() {
            Some((last, y, s)) if last.end == rows.start && *y == dy && *s == dsec => last.end = rows.end,
            _ => self.0.push((rows, dy, dsec)),
        }
    }

    /// Moves the rows; a long run is split between the processor's cores,
    /// since a long note below an edit is millions of glyphs and vertices.
    fn apply(self, rows: &mut [egui::epaint::text::Row]) {
        const ALONE: usize = 2048;
        for (range, dy, dsec) in self.0 {
            let part = &mut rows[range];
            if part.len() <= ALONE {
                move_rows(part, dy, dsec);
                continue;
            }
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
            let per = part.len().div_ceil(cores).max(ALONE);
            std::thread::scope(|scope| {
                for chunk in part.chunks_mut(per) {
                    scope.spawn(move || move_rows(chunk, dy, dsec));
                }
            });
        }
    }
}

fn move_rows(rows: &mut [egui::epaint::text::Row], dy: f32, dsec: i64) {
    let d = egui::vec2(0.0, dy);
    for row in rows {
        for gl in &mut row.glyphs {
            gl.pos += d;
            gl.section_index = (gl.section_index as i64 + dsec) as u32;
        }
        row.section_index_at_start = (row.section_index_at_start as i64 + dsec) as u32;
        if dy != 0.0 {
            row.rect = row.rect.translate(d);
            row.visuals.mesh.translate(d);
            row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(d);
        }
    }
}

/// One line laid out on its own, at the top of its room.
#[allow(clippy::too_many_arguments)]
fn layout_line(
    fonts: &egui::text::Fonts,
    body: &str,
    sections: &[egui::text::LayoutSection],
    line: &LineInfo,
    table: Option<(&str, &(Vec<Vec<std::ops::Range<usize>>>, Vec<pulldown_cmark::Alignment>), usize)>,
    width: f32,
    note: crate::align::TextAlign,
    st: &Styles,
) -> LineLayout {
    let sub = |range: std::ops::Range<usize>, len: usize| -> LayoutJob {
        let mut j = LayoutJob { text: body[..len].to_string(), ..Default::default() };
        for sec in &sections[range] {
            let a = sec.byte_range.start.min(len);
            let b = sec.byte_range.end.min(len);
            j.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: a..b, format: sec.format.clone() });
        }
        j
    };
    let mut table_height = None;
    let mut table_row = None;
    let mut cells = Vec::new();
    if let Some((src, (table_rows, alignments), offset)) = table {
        // The delimiter has no cells, but remains one source row for cursor mapping.
        let source_row = table_rows.iter().enumerate().find(|(_, cells)| cells.first().is_some_and(|r| r.start >= offset && r.start <= offset + body.len()));
        table_height = Some(1.0);
        if let Some((row_index, row_cells)) = source_row {
            let columns = alignments.len().max(1);
            let col_width = width / columns as f32;
            let padding = (st.size * 0.5).min(col_width * 0.2);
            let mut height = st.size * st.typo.line + padding * 2.0;
            for (col, cell) in row_cells.iter().take(columns).enumerate() {
                let mut cell_job = LayoutJob::default();
                let mut fmt = st.fmt(st.size, st.p.text);
                if row_index == 0 {
                    fmt.font_id.family = bold_family();
                }
                let raw = &src[cell.clone()];
                inline(&mut cell_job, raw.trim(), cell.start, fmt, st, Reveal(None));
                let w = (col_width - padding * 2.0).max(1.0);
                cell_job.wrap.max_width = w;
                let alignment = match alignments.get(col) {
                    Some(pulldown_cmark::Alignment::Center) => crate::align::TextAlign::Center,
                    Some(pulldown_cmark::Alignment::Right) => crate::align::TextAlign::Right,
                    _ => crate::align::TextAlign::Left,
                };
                let galley = crate::align::layout(fonts, cell_job, w, alignment);
                height = height.max(galley.rect.height() + padding * 2.0);
                cells.push((egui::pos2(col as f32 * col_width + padding, padding), galley));
            }
            table_row = Some((height, row_index == 0, columns));
            table_height = Some(height);
        }
    }
    let align = match line.align {
        LineAlign::Note => note,
        LineAlign::List => crate::align::for_lists(note),
        LineAlign::Left => crate::align::TextAlign::Left,
    };
    // the hanging mark: its width, from the line's start to the text
    let lead_w = if line.lead > 0 {
        let end = sections[line.lead - 1].byte_range.end.min(body.len());
        let mut j = sub(0..line.lead, end);
        j.wrap.max_width = f32::INFINITY;
        crate::align::layout_owned(fonts, j, f32::INFINITY, crate::align::TextAlign::Left).rect.width()
    } else {
        0.0
    };
    let wrap = (width - line.content_x).max(40.0);
    let mut j = sub(0..sections.len(), body.len());
    j.wrap.max_width = wrap;
    if let Some(height) = table_height {
        j.wrap.max_width = f32::INFINITY;
        j.first_row_min_height = height;
    }
    if line.deco == Deco::Rule {
        j.first_row_min_height = st.size * st.typo.line;
    }
    let g = crate::align::layout_owned(fonts, j, wrap, align);
    let x0 = line.content_x - lead_w;
    let n = g.rows.len();
    let height = g.rect.height();
    let mut rows = g.rows;
    for (ri, row) in rows.iter_mut().enumerate() {
        // whole pixels, or the text blurs
        let d = egui::vec2(if ri == 0 { x0 } else { x0 + lead_w }, 0.0).round();
        if d.x != 0.0 {
            for gl in &mut row.glyphs {
                gl.pos += d;
            }
            row.rect = row.rect.translate(d);
            row.visuals.mesh.translate(d);
            row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(d);
        }
        if ri + 1 == n {
            row.ends_with_newline = line.newline;
        }
    }
    let first = rows.first().map(|r| r.rect).unwrap_or(egui::Rect::ZERO);
    LineLayout { height: height.max(first.height()), first, table: table_row, cells, rows }
}

/// What the reading view draws around the lines: bullets, check boxes, quote
/// bars, code boxes, rules and tables, in galley coordinates.
fn decos_of(lines: &[Placed], width: f32, st: &Styles) -> Vec<DecoShape> {
    let mut decos = Vec::new();
    let mut code_top: Option<f32> = None;
    let mut quote: Vec<(f32, f32)> = Vec::new(); // the open bars: top, x
    let step = list_step(st.size);
    let mut y = 0.0;
    for (li, line) in lines.iter().enumerate() {
        let top = line.top;
        y = top + line.height;
        if let Some((height, header, columns)) = line.table {
            decos.push(DecoShape::TableRow(egui::Rect::from_min_size(egui::pos2(0.0, top), egui::vec2(width, height)), header, columns));
            for (at, galley) in &line.cells {
                decos.push(DecoShape::TableCell(*at + egui::vec2(0.0, top), galley.clone()));
            }
        }
        let first_y = line.first.center().y + top.round();
        match line.deco {
            Deco::Rule => decos.push(DecoShape::Rule(egui::Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(width, y)))),
            Deco::Bullet(level) => decos.push(DecoShape::Bullet(egui::pos2(step * level as f32 + st.size * 0.65, first_y))),
            Deco::Task(level, done) => decos.push(DecoShape::Check(egui::pos2(step * level as f32 + st.size * 0.65, first_y), done)),
            _ => {}
        }
        // code: one box from the opening fence to the closing one
        let next_code = lines.get(li + 1).is_some_and(|l| l.deco == Deco::Code);
        if line.deco == Deco::Code {
            let t = *code_top.get_or_insert(top);
            if !next_code || (line.fence && t != top) {
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
    decos
}

/// Something the reading view draws around the text, in galley coordinates.
#[derive(Clone, Debug)]
pub enum DecoShape {
    Bullet(egui::Pos2),
    Check(egui::Pos2, bool),
    Bar(f32, f32, f32),
    CodeBox(egui::Rect),
    Rule(egui::Rect),
    TableRow(egui::Rect, bool, usize),
    TableCell(egui::Pos2, std::sync::Arc<egui::Galley>),
}

/// Draw what the reading view draws around the text: list bullets and check
/// boxes, quote bars, the code block's box.
fn deco_shapes(clip: egui::Rect, at: egui::Pos2, decos: &[DecoShape], size: f32, p: &Palette) -> Vec<egui::Shape> {
    let mut shapes = Vec::new();
    let v = at.to_vec2();
    let typo = crate::typo::get();
    for d in decos {
        let visible = match d {
            DecoShape::Bullet(c) | DecoShape::Check(c, _) => egui::Rect::from_center_size(*c + v, egui::vec2(size * 2.0, size * 2.0)),
            DecoShape::Bar(x, t, b) => egui::Rect::from_min_max(egui::pos2(at.x + x - 2.0, at.y + t), egui::pos2(at.x + x + 2.0, at.y + b)),
            DecoShape::CodeBox(r) | DecoShape::Rule(r) | DecoShape::TableRow(r, _, _) => r.translate(v),
            DecoShape::TableCell(pos, galley) => galley.rect.translate((*pos + v).to_vec2()),
        };
        if !visible.intersects(clip) { continue; }
        match d {
            DecoShape::TableRow(r, header, columns) => {
                let r = r.translate(v);
                if *header { shapes.push(egui::Shape::rect_filled(r, 0.0, p.bg_active)); }
                shapes.push(egui::Shape::rect_stroke(r, 0.0, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside));
                for col in 1..*columns {
                    let x = r.left() + r.width() * col as f32 / *columns as f32;
                    shapes.push(egui::Shape::line_segment([egui::pos2(x, r.top()), egui::pos2(x, r.bottom())], Stroke::new(1.0_f32, p.border)));
                }
            }
            DecoShape::TableCell(pos, galley) => shapes.push(egui::Shape::galley(*pos + v, galley.clone(), p.text)),
            DecoShape::Rule(r) => {
                let r = r.translate(v);
                shapes.push(egui::Shape::line_segment([r.left_center(), r.right_center()], Stroke::new(1.0_f32, p.border_strong)));
            }
            DecoShape::Bullet(c) => {
                shapes.push(egui::Shape::circle_filled(*c + v, (size * 0.16).max(2.2), typo.bullet.unwrap_or(p.faint)));
            }
            DecoShape::Check(c, done) => {
                let r = egui::Rect::from_center_size(*c + v, egui::vec2(size * 0.9, size * 0.9));
                // as egui draws the reading view's check box
                shapes.push(egui::Shape::rect_filled(r, 3.0, p.bg_active));
                if *done {
                    let k = r.width();
                    shapes.push(egui::Shape::line(vec![r.min + egui::vec2(k * 0.22, k * 0.52), r.min + egui::vec2(k * 0.42, k * 0.72), r.min + egui::vec2(k * 0.78, k * 0.28)], Stroke::new(1.5_f32, p.text)));
                }
            }
            DecoShape::Bar(x, t, b) => {
                shapes.push(egui::Shape::line_segment([egui::pos2(at.x + x, at.y + t + 2.0), egui::pos2(at.x + x, at.y + b - 2.0)], Stroke::new(typo.quote_width, typo.quote.unwrap_or(p.border_strong))));
            }
            DecoShape::CodeBox(r) => {
                shapes.push(egui::Shape::rect_filled(r.translate(v), 8.0, p.code_bg));
                shapes.push(egui::Shape::rect_stroke(r.translate(v), 8.0, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside));
            }
        }
    }
    shapes
}

/// A fast hash of a note's text, for the caches that must notice when it
/// changes: they ask every frame, and a long note is megabytes.
pub fn text_hash(text: &str) -> u64 {
    egui::ahash::RandomState::with_seeds(0x41, 0x74, 0x65, 0x6c).hash_one(text)
}

/// Character index ↔ byte offset helpers for the editor's cursor.
pub fn char_to_byte(s: &str, ci: usize) -> usize {
    // whole blocks first: counting the bytes that start a character is fast
    let (mut seen, mut at) = (0, 0);
    for block in s.as_bytes().chunks(4096) {
        let n = block.iter().filter(|&&b| (b as i8) >= -64).count();
        if seen + n > ci {
            break;
        }
        seen += n;
        at += block.len();
    }
    // a block may end inside a character it already counted: start at that character
    if !s.is_char_boundary(at) {
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        seen -= 1;
    }
    s[at..].char_indices().nth(ci - seen).map(|(b, _)| at + b).unwrap_or(s.len())
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
    ui.fonts(|f| cache.live(f, text, st, reveal, width).0)
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
    move |ui: &mut egui::Ui| {
        let reveal = if st.live { reveal_of(ui.ctx(), id, text) } else { Reveal(None) };
        // Reserve the background slot; layout only at TextEdit's actual wrap width.
        let deco_slot = st.live.then(|| ui.painter().add(egui::Shape::Noop));
        // without input the text cannot change here: idle frames skip copying it
        let busy = ui.input(|i| !i.events.is_empty() || i.pointer.any_down());
        let before_text = busy.then(|| text.clone());
        // the text edit has the layout to itself while it runs (see HighlightCache)
        cache.rendered = None;
        cache.lend = true;
        let mut layouter = |ui: &egui::Ui, s: &str, wrap: f32| galley(ui, s, cache, st, reveal, wrap);
        // a right click must not move the caret: the menu acts on the selection
        let prev = selection(ui.ctx(), id);
        crate::writing::before(ui, id);
        let mut edit = egui::TextEdit::multiline(text)
            .id(id)
            .frame(false)
            .desired_width(f32::INFINITY)
            .desired_rows(min_rows)
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
        cache.give_back(&out.galley, out.response.has_focus());
        cache.rendered = Some((out.galley.clone(), out.galley_pos));
        if let Some(slot) = deco_slot {
            let decos = &cache.decos();
            ui.painter().set(slot, egui::Shape::Vec(deco_shapes(ui.clip_rect(), out.galley_pos, decos, st.size, &st.p)));
            for deco in decos.iter() {
                if let DecoShape::TableCell(pos, galley) = deco {
                    crate::find::paint(ui, galley, out.galley_pos + pos.to_vec2());
                }
            }
        }
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
        if st.live && (reveal_of(ui.ctx(), id, text) != reveal || before_text.is_some_and(|b| b != *text)) {
            ui.ctx().request_repaint();
        }
        let mut resp = resp;
        if text.len() != before || wrote || dashed {
            resp.mark_changed();
        }
        resp
    }
}

/// Fails unless two galleys hold the same text, sections, rows, glyphs and meshes.
#[cfg(test)]
pub(crate) fn assert_same_layout(a: &egui::Galley, b: &egui::Galley, what: &str) {
    assert_eq!(a.job.text, b.job.text, "{what}: text");
    assert_eq!(a.job.sections.len(), b.job.sections.len(), "{what}: sections");
    for (i, (x, y)) in a.job.sections.iter().zip(&b.job.sections).enumerate() {
        assert_eq!(x.byte_range, y.byte_range, "{what}: section {i}");
        assert!(x.format == y.format, "{what}: format of section {i}");
    }
    assert_eq!(a.rows.len(), b.rows.len(), "{what}: rows");
    for (i, (x, y)) in a.rows.iter().zip(&b.rows).enumerate() {
        assert_eq!(x.rect, y.rect, "{what}: rect of row {i}");
        assert_eq!(x.section_index_at_start, y.section_index_at_start, "{what}: row {i}");
        assert_eq!(x.ends_with_newline, y.ends_with_newline, "{what}: newline of row {i}");
        assert_eq!(x.glyphs.len(), y.glyphs.len(), "{what}: glyphs of row {i}");
        for (g, h) in x.glyphs.iter().zip(&y.glyphs) {
            assert_eq!((g.chr, g.pos, g.section_index), (h.chr, h.pos, h.section_index), "{what}: row {i}");
        }
        assert_eq!(x.visuals.mesh.indices, y.visuals.mesh.indices, "{what}: indices of row {i}");
        assert_eq!(x.visuals.mesh.vertices.len(), y.visuals.mesh.vertices.len(), "{what}: vertices of row {i}");
        assert!(x.visuals.mesh.vertices.iter().zip(&y.visuals.mesh.vertices).all(|(v, w)| v.pos == w.pos && v.uv == w.uv && v.color == w.color), "{what}: vertices of row {i}");
        // an empty mesh has no bounds (infinite, or not a number once moved)
        let (p, q) = (x.visuals.mesh_bounds, y.visuals.mesh_bounds);
        assert!(p == q || (!p.is_finite() && !q.is_finite()), "{what}: bounds of row {i}: {p:?} {q:?}");
    }
    assert_eq!(a.rect, b.rect, "{what}: rect");
    assert_eq!((a.num_vertices, a.num_indices), (b.num_vertices, b.num_indices), "{what}: mesh size");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles() -> Styles {
        Styles::live(16.0, crate::theme::Palette::new(&crate::config::Appearance::default()))
    }

    /// A long manuscript: paragraphs, headings, lists, quotes and code.
    fn manuscript(min_bytes: usize) -> String {
        // numbered, so lines do not repeat (and no layout is reused for that)
        let mut text = String::new();
        let mut n = 0;
        while text.len() < min_bytes {
            n += 1;
            text += &format!("## Capítulo {n}\n\nUma nota longa ({n}) com **ênfase**, [[referência {n}]] e texto corrido que planeja a história, com frases que quebram em várias linhas na largura do editor.\n\n- um item {n} com *itálico*\n- outro item {n}\n  - um item dentro {n}\n\n> uma citação curta {n}\n\n```\nlet codigo = {n};\n```\n\nMais um parágrafo {n} com `código`, ==marca== e #tag no fim da frase.\n\n");
        }
        text
    }

    /// Times one Live Preview layout after `edit`, with the cache warm.
    fn time_live(cache: &mut HighlightCache, fonts: &egui::text::Fonts, text: &str, st: &Styles, reveal: Reveal) -> std::time::Duration {
        let start = std::time::Instant::now();
        let _ = cache.live(fonts, text, st, reveal, 700.0);
        start.elapsed()
    }

    #[test]
    #[ignore = "benchmark: cargo test --release -- --ignored --nocapture large_note"]
    fn large_note_typing_and_cursor_moves() {
        let st = styles();
        let mut text = manuscript(2_000_000);
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        let mut cache = HighlightCache::default();
        let mid = text.len() / 2;
        let line = text[mid..].find("Uma nota").unwrap() + mid;
        let t = time_live(&mut cache, &fonts, &text, &st, Reveal(Some((line, line))));
        eprintln!("{} bytes, first layout: {t:?}", text.len());
        {
            let (g, _) = cache.live(&fonts, &text, &st, Reveal(Some((line, line))), 700.0);
            let glyphs: usize = g.rows.iter().map(|r| r.glyphs.len()).sum();
            eprintln!("{} rows, {glyphs} glyphs, {} vertices, {} indices", g.rows.len(), g.num_vertices, g.num_indices);
            // what egui does each frame a selection is painted on a shared galley
            let start = std::time::Instant::now();
            let copy = (*g).clone();
            eprintln!("cloning the galley: {:?}", start.elapsed());
            drop(copy);
        }
        // the cursor walks along plain text in a paragraph
        let mut moves = std::time::Duration::ZERO;
        for k in 1..=10 {
            moves += time_live(&mut cache, &fonts, &text, &st, Reveal(Some((line + k, line + k))));
        }
        eprintln!("cursor move inside a paragraph: {:?}", moves / 10);
        // the cursor goes into the bold span and out again
        let bold = text[line..].find("**").unwrap() + line;
        let t = time_live(&mut cache, &fonts, &text, &st, Reveal(Some((bold + 3, bold + 3))));
        eprintln!("cursor enters a bold span: {t:?}");
        // typing in the middle of the note
        let mut typing = std::time::Duration::ZERO;
        let mut at = line + 4;
        for c in "palavra".chars() {
            text.insert(at, c);
            at += 1;
            typing += time_live(&mut cache, &fonts, &text, &st, Reveal(Some((at, at))));
        }
        eprintln!("typing a letter: {:?}", typing / 7);
        // a new line
        text.insert(at, '\n');
        at += 1;
        let t = time_live(&mut cache, &fonts, &text, &st, Reveal(Some((at, at))));
        eprintln!("a new line: {t:?}");
    }

    /// A note with a bit of everything that carries from line to line.
    fn mixed_note() -> String {
        "---\ntitulo: Nota\ntags: [a, b]\n---\n# Título\n\nUm parágrafo com **negrito**, *itálico*, `código`, [[link|apelido]] e #tag.\n\n- item um\n  - item dentro\n- [ ] tarefa\n- [x] feita\n\n> uma citação\n> > dentro\n\n```\nlet x = 1;\n```\n\n| A | B |\n| :- | -: |\n| **c** | d |\n\n---\n\nFim do texto, que é longo o bastante para quebrar em mais de uma linha na largura do editor de teste.".into()
    }

    #[test]
    fn incremental_layout_matches_a_fresh_one_after_every_edit() {
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        for st in [styles(), Styles::source(16.0, crate::theme::Palette::new(&crate::config::Appearance::default()))] {
            let mut text = mixed_note();
            let mut cache = HighlightCache::default();
            let check = |cache: &mut HighlightCache, text: &str, reveal: Reveal, what: &str| {
                let (g, d) = cache.live(&fonts, text, &st, reveal, 320.0);
                let (fresh, fd) = HighlightCache::default().live(&fonts, text, &st, reveal, 320.0);
                assert_same_layout(&g, &fresh, what);
                assert_eq!(format!("{d:?}").len(), format!("{fd:?}").len(), "{what}: decorations");
            };
            check(&mut cache, &text, Reveal(None), "first");
            let at = |text: &str, s: &str| text.find(s).unwrap();
            // the cursor walks into spans, a list, the table and out again
            for probe in ["negrito", "link", "item dentro", "tarefa", "citação", "let x", "| A", "**c**", "Fim"] {
                let p = at(&text, probe) + 1;
                check(&mut cache, &text, Reveal(Some((p, p))), probe);
            }
            // a selection over several lines
            let (a, b) = (at(&text, "Um parágrafo"), at(&text, "feita"));
            check(&mut cache, &text, Reveal(Some((a, b))), "selection");
            let edits: Vec<(&str, Box<dyn Fn(&mut String)>)> = vec![
                ("a letter", Box::new(|t: &mut String| { let p = t.find("parágrafo").unwrap(); t.insert(p, 'x'); })),
                ("a new line", Box::new(|t: &mut String| { let p = t.find("com **").unwrap(); t.insert(p, '\n'); })),
                ("bold made", Box::new(|t: &mut String| { let p = t.find("Fim").unwrap(); t.insert_str(p, "**"); })),
                ("a fence opened", Box::new(|t: &mut String| { let p = t.find("- item um").unwrap(); t.insert_str(p, "```\n"); })),
                ("the fence closed", Box::new(|t: &mut String| { let p = t.find("```\n- item").unwrap(); t.replace_range(p..p + 4, ""); })),
                ("a heading", Box::new(|t: &mut String| { let p = t.find("Fim").unwrap(); t.insert_str(p, "## "); })),
                ("a table cell", Box::new(|t: &mut String| { let p = t.find("| d |").unwrap(); t.insert_str(p + 2, "longo "); })),
                ("lines deleted", Box::new(|t: &mut String| { let (a, b) = (t.find("> uma").unwrap(), t.find("```\nlet").unwrap()); t.replace_range(a..b, ""); })),
                ("frontmatter broken", Box::new(|t: &mut String| { t.remove(0); })),
                ("frontmatter back", Box::new(|t: &mut String| { t.insert(0, '-'); })),
                ("text appended", Box::new(|t: &mut String| { t.push_str("\nmais\n"); })),
                ("the end trimmed", Box::new(|t: &mut String| { t.pop(); })),
                ("a line at the start", Box::new(|t: &mut String| { t.insert_str(0, "antes\n"); })),
                ("all gone", Box::new(|t: &mut String| t.clear())),
                ("all back", Box::new(|t: &mut String| *t = mixed_note())),
            ];
            for (what, edit) in edits {
                edit(&mut text);
                let p = text.len() / 2;
                let p = (0..=p).rev().find(|&i| text.is_char_boundary(i)).unwrap();
                check(&mut cache, &text, Reveal(Some((p, p))), what);
                check(&mut cache, &text, Reveal(None), what);
            }
        }
    }

    #[test]
    fn a_selection_painted_into_a_lent_layout_is_taken_out_again() {
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        let st = styles();
        let text = mixed_note();
        let mut cache = HighlightCache::default();
        cache.lend = true;
        let (mut g, _) = cache.live(&fonts, &text, &st, Reveal(None), 320.0);
        // the spare is copied from it on another thread first
        cache.settle_copy(true);
        let glyphs = g.rows[0].glyphs.as_ptr();
        // as the text edit paints a selection of its rows 2 to 9
        let range = egui::text::CursorRange::two(g.from_ccursor(egui::text::CCursor::new(30)), g.from_ccursor(egui::text::CCursor::new(200)));
        egui::text_selection::visuals::paint_text_selection(&mut g, &egui::Visuals::default(), &range, None);
        assert_eq!(g.rows[0].glyphs.as_ptr(), glyphs, "the layout was lent, not copied");
        cache.give_back(&g, false);
        drop(g);
        let (again, _) = cache.live(&fonts, &text, &st, Reveal(None), 320.0);
        let (fresh, _) = HighlightCache::default().live(&fonts, &text, &st, Reveal(None), 320.0);
        assert_same_layout(&again, &fresh, "after the selection");
    }

    #[test]
    #[ignore = "profiling aid: laying out a long note (for valgrind --tool=callgrind)"]
    fn profile_layout() {
        let text = manuscript(200_000);
        let st = styles();
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        for _ in 0..2 {
            let _ = HighlightCache::default().live(&fonts, &text, &st, Reveal(None), 640.0);
        }
    }

    #[test]
    fn tables_render_in_live_editor_and_reveal_source_only_when_entered() {
        let st = styles();
        let text = "Antes\n\n| Item | Definição |\n| :--- | ---: |\n| **Texto** | Uma definição longa que precisa quebrar dentro da célula |\n| 日本 | ação |\n\nDepois";
        let ranges = table_ranges(text);
        assert_eq!(ranges.len(), 1);
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        let mut cache = HighlightCache::default();
        let (galley, decos) = cache.live(&fonts, text, &st, Reveal(Some((0, 0))), 300.0);
        assert_eq!(galley.job.text, text);
        assert_eq!(galley.rows.iter().map(|r| r.char_count_including_newline()).sum::<usize>(), text.chars().count());
        let cells: Vec<_> = decos.iter().filter_map(|d| match d { DecoShape::TableCell(_, g) => Some(g), _ => None }).collect();
        assert_eq!(cells.len(), 6);
        assert!(cells.iter().any(|g| g.rows.len() > 1));
        assert!(cells.iter().any(|g| g.job.text == "日本"));
        assert_eq!(decos.iter().filter(|d| matches!(d, DecoShape::TableRow(_, _, _))).count(), 3);
        let inside = ranges[0].start + 2;
        let (_, raw) = cache.live(&fonts, text, &st, Reveal(Some((inside, inside))), 300.0);
        assert!(!raw.iter().any(|d| matches!(d, DecoShape::TableCell(..))));
        let (_, rendered) = cache.live(&fonts, text, &st, Reveal(None), 300.0);
        assert!(rendered.iter().any(|d| matches!(d, DecoShape::TableCell(..))));
        let (_, source_lines) = highlight_lines(text, &Styles::source(16.0, st.p), Reveal(None));
        assert!(!source_lines.iter().any(|l| matches!(l.deco, Deco::Table { .. })));
    }

    #[test]
    fn table_parser_ignores_fenced_examples_and_respects_escaped_pipes() {
        assert!(table_ranges("```md\n| a | b |\n| --- | --- |\n| c | d |\n```").is_empty());
        let (rows, align) = table_cells("| a | b |\n| --- | --- |\n| x\\|y | z |");
        assert_eq!(align.len(), 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].len(), 2);
    }

    #[test]
    fn live_preview_renders_escaped_punctuation_and_rules_without_changing_source() {
        let st = styles();
        let text = "Antes\n\n1\\. sacrificar o mundo\n\n---\n\nDepois";
        let (job, lines) = highlight_lines(text, &st, Reveal(None));
        assert_eq!(job.text, text);
        let slash = text.find('\\').unwrap();
        let syntax = job.sections.iter().find(|s| s.byte_range.contains(&slash)).unwrap();
        assert_eq!(syntax.format.color, Color32::TRANSPARENT);
        assert_eq!(lines[4].deco, Deco::Rule);
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        let mut cache = HighlightCache::default();
        let (g, decos) = cache.live(&fonts, text, &st, Reveal(None), 400.0);
        assert!(decos.iter().any(|d| matches!(d, DecoShape::Rule(_))));
        assert_eq!(g.rows.iter().map(|r| r.char_count_including_newline()).sum::<usize>(), text.chars().count());
        let source = Styles::source(16.0, st.p);
        let (raw, _) = highlight_lines(text, &source, Reveal(None));
        assert_ne!(raw.sections.iter().find(|s| s.byte_range.contains(&slash)).unwrap().format.color, Color32::TRANSPARENT);
    }

    #[test]
    fn a_multimegabyte_note_reuses_its_layout_on_idle_frames() {
        let st = styles();
        let text = "Uma nota longa com **ênfase**, [[referência]] e texto para planejar a história.\n\n".repeat(32_000);
        assert!(text.len() > 2_400_000);
        let fonts = egui::text::Fonts::new(1.0, 2048, crate::fonts::definitions());
        let mut cache = HighlightCache::default();
        let (first, decos) = cache.live(&fonts, &text, &st, Reveal(None), 700.0);
        let start = std::time::Instant::now();
        for _ in 0..8 {
            let (next, next_decos) = cache.live(&fonts, &text, &st, Reveal(None), 700.0);
            assert!(std::sync::Arc::ptr_eq(&first, &next));
            assert!(std::sync::Arc::ptr_eq(&decos, &next_decos));
        }
        eprintln!("8 idle cache hits, {} bytes: {:?}", text.len(), start.elapsed());
    }

    #[test]
    fn spans_are_found_as_the_regex_with_groups_found_them() {
        let old = Regex::new(concat!(
            r"(?P<escape>\\[[:punct:]])",
            r"|(?P<code>`[^`\n]+`)",
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
        )).unwrap();
        let names = ["escape", "code", "wiki", "mdlink", "url", "bold", "italic", "strike", "mark", "field", "tag", "comment"];
        let lines = [
            "Uma frase com **negrito**, *itálico*, _sublinhado_ e __forte__.",
            "snake_case_nome e é_x_ e _ação_ e x_y_ e _a_b e (_entre_) e _fim_",
            "ação_ _é_ __ _ _ çã_o_ _ü_ 日本_語_ _日本_",
            "[[link|apelido]] ![[img.png]] [texto](url) ![alt](a.png) [campo:: valor]",
            "https://exemplo.com/a_b_c e #tag #ação #12 x#y \\* `código _x_` ==marca== ~~risco~~ %%nota%%",
            "#começo e fim_",
        ];
        for line in lines {
            let want: Vec<(&str, usize, usize)> = old.captures_iter(line).map(|c| {
                let m = c.get(0).unwrap();
                (*names.iter().find(|n| c.name(n).is_some()).unwrap(), m.start(), m.end())
            }).collect();
            let got: Vec<(&str, usize, usize)> = spans(line).map(|(k, a, b)| (names[SPANS.iter().position(|(s, _)| *s == k).unwrap()], a, b)).collect();
            assert_eq!(got, want, "{line}");
        }
    }

    #[test]
    fn char_to_byte_matches_char_indices_across_blocks() {
        let text = "ação 日本 🎭 texto\n".repeat(900);
        let naive = |ci: usize| text.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(text.len());
        let n = text.chars().count();
        for ci in (0..n + 3).step_by(7).chain(n.saturating_sub(10)..n + 3) {
            assert_eq!(char_to_byte(&text, ci), naive(ci), "char {ci}");
        }
        assert_eq!(char_to_byte("", 0), 0);
        assert_eq!(char_to_byte("é", 1), 2);
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
