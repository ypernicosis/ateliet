//! Markdown → a small document tree (with Obsidian's wikilinks, embeds, tags,
//! callouts, highlights, inline fields and custom task statuses), drawn with
//! egui. A paragraph is one laid-out text block, so it wraps and selects like
//! text; links inside it are found from the click position.

use ateliet_core::syntax;
use ateliet_core::text::LineIndex;
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Sense, Stroke, TextFormat};
use once_cell::sync::Lazy;
use pulldown_cmark::{CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};
use regex::Regex;
use std::ops::Range;
use std::sync::Arc;

// ── document tree ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub mark: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Note(String),
    Url(String),
    Tag(String),
}

#[derive(Clone, Debug)]
pub enum Inline {
    Text(String, Style),
    Code(String),
    Link { target: Target, children: Vec<Inline> },
    Image { target: String, alt: String, internal: bool },
    Break,
    Math(String),
    Field { key: String, value: String, paren: bool },
}

#[derive(Clone, Debug)]
pub struct Item {
    pub task: Option<char>,
    pub line: usize,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Heading { level: u8, inlines: Vec<Inline>, line: usize },
    Para(Vec<Inline>),
    List { start: Option<u64>, items: Vec<Item> },
    Quote(Vec<Block>),
    Callout { kind: String, title: Vec<Inline>, body: Vec<Block>, folded: Option<bool>, line: usize },
    Code { lang: String, text: String },
    Table { header: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
    Html(String),
    Math(String),
    /// `![[note]]` or `![[image.png]]` alone in its paragraph.
    Embed { target: String, alt: String },
}

pub struct Doc {
    pub blocks: Vec<Block>,
    /// (level, text, line)
    pub headings: Vec<(u8, String, usize)>,
}

static WIKI: Lazy<Regex> = Lazy::new(|| Regex::new(r"(!?)\[\[([^\[\]\n]+?)\]\]").unwrap());
static TASK: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+\[(.)\]\s?").unwrap());
static CALLOUT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[!([^\]\s]+)\]([+-]?)\s*").unwrap());

/// Rewrite Obsidian syntax CommonMark does not know into links pulldown-cmark
/// parses (line numbers are kept: no newline is added or removed).
fn prepare(text: &str) -> String {
    let prep = syntax::prepare(text);
    let src = prep.masked;
    let in_code = |at: usize| syntax::in_ranges(&prep.code, at);
    let mut out = String::with_capacity(src.len() + 64);
    let mut at = 0;
    // wikilinks and embeds
    let mut spans: Vec<(Range<usize>, String)> = Vec::new();
    for c in WIKI.captures_iter(&src) {
        let m = c.get(0).unwrap();
        if in_code(m.start()) {
            continue;
        }
        let (target, alias) = syntax::split_wikilink(&c[2]);
        let enc = target.replace('>', "%3E");
        let label = alias.map(str::to_string).unwrap_or_else(|| syntax::link_display(target));
        let label = label.replace('[', "\\[").replace(']', "\\]");
        let rep = if &c[1] == "!" { format!("![{label}](<wiki:{enc}>)") } else { format!("[{label}](<wiki:{enc}>)") };
        spans.push((m.range(), rep));
    }
    // tags (outside links and code)
    for (r, tag) in syntax::find_tags(&src) {
        if in_code(r.start) || spans.iter().any(|(s, _)| s.start <= r.start && r.start < s.end) {
            continue;
        }
        spans.push((r.clone(), format!("[{}](<tag:{}>)", tag.replace('_', "\\_"), &tag[1..])));
    }
    // ==highlights==
    for m in syntax::HIGHLIGHT.find_iter(&src) {
        if in_code(m.start()) || spans.iter().any(|(s, _)| s.start < m.end() && m.start() < s.end) {
            continue;
        }
        spans.push((m.range(), format!("<mark>{}</mark>", &src[m.start() + 2..m.end() - 2])));
    }
    spans.sort_by_key(|(r, _)| r.start);
    for (r, rep) in spans {
        if r.start < at {
            continue;
        }
        out.push_str(&src[at..r.start]);
        out.push_str(&rep);
        at = r.end;
    }
    out.push_str(&src[at..]);
    out
}

struct Builder<'a> {
    events: std::iter::Peekable<pulldown_cmark::OffsetIter<'a>>,
    lines: LineIndex<'a>,
    headings: Vec<(u8, String, usize)>,
}

pub fn parse(text: &str) -> Doc {
    let src = prepare(text);
    let mut b = Builder { events: Parser::new_ext(&src, syntax::md_options()).into_offset_iter().peekable(), lines: LineIndex::new(&src), headings: Vec::new() };
    let blocks = b.blocks(None);
    Doc { blocks, headings: b.headings }
}

fn plain(inl: &[Inline]) -> String {
    let mut s = String::new();
    for i in inl {
        match i {
            Inline::Text(t, _) | Inline::Code(t) | Inline::Math(t) => s.push_str(t),
            Inline::Link { children, .. } => s.push_str(&plain(children)),
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::Break => s.push(' '),
            Inline::Field { key, value, .. } => s.push_str(&format!("{key}: {value}")),
        }
    }
    s
}

/// Merge neighbouring text runs and pick out `[key:: value]` fields.
fn tidy(inl: Vec<Inline>) -> Vec<Inline> {
    let mut merged: Vec<Inline> = Vec::new();
    for i in inl {
        if let (Some(Inline::Text(prev, ps)), Inline::Text(t, s)) = (merged.last_mut(), &i) {
            if ps == s {
                prev.push_str(t);
                continue;
            }
        }
        merged.push(i);
    }
    let mut out = Vec::new();
    for i in merged {
        match i {
            Inline::Text(t, s) if t.contains("::") => {
                let mut at = 0;
                for f in find_fields(&t) {
                    if f.0.start > at {
                        out.push(Inline::Text(t[at..f.0.start].to_string(), s));
                    }
                    out.push(Inline::Field { key: f.1, value: f.2, paren: f.3 });
                    at = f.0.end;
                }
                if at < t.len() {
                    out.push(Inline::Text(t[at..].to_string(), s));
                }
            }
            Inline::Link { target, children } => out.push(Inline::Link { target, children: tidy(children) }),
            other => out.push(other),
        }
    }
    out
}

/// `[key:: value]` / `(key:: value)` (the rules of the core's inline fields).
pub fn find_fields(line: &str) -> Vec<(Range<usize>, String, String, bool)> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let open = b[i];
        if (open == b'[' || open == b'(') && !(open == b'[' && (b.get(i + 1) == Some(&b'[') || (i > 0 && b[i - 1] == b'['))) {
            let close = if open == b'[' { b']' } else { b')' };
            let mut depth = 0;
            let mut end = None;
            for (j, &c) in b.iter().enumerate().skip(i) {
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                }
            }
            if let Some(end) = end {
                let inner = &line[i + 1..end];
                if let Some(sep) = inner.find("::") {
                    let key = inner[..sep].trim().trim_matches(|c| matches!(c, '*' | '_' | '~' | '=')).trim();
                    if !key.is_empty() && key.len() <= 150 && !key.contains(['[', ']', '(', ')', '`', '|', '#']) {
                        out.push((i..end + 1, key.to_string(), inner[sep + 2..].trim().to_string(), open == b'('));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

impl<'a> Builder<'a> {
    fn blocks(&mut self, until: Option<TagEnd>) -> Vec<Block> {
        let mut out = Vec::new();
        let mut loose: Vec<Inline> = Vec::new();
        while let Some((ev, r)) = self.events.next() {
            match ev {
                Event::End(t) if Some(t) == until => break,
                Event::Start(tag) => {
                    if !loose.is_empty() && !is_inline(&tag) {
                        out.push(Block::Para(tidy(std::mem::take(&mut loose))));
                    }
                    if is_inline(&tag) {
                        let mut style = Style::default();
                        self.inline_start(tag, &mut loose, &mut style);
                        continue;
                    }
                    if let Some(b) = self.block(tag, r.start) {
                        out.push(b);
                    }
                }
                Event::Rule => {
                    flush(&mut out, &mut loose);
                    out.push(Block::Rule);
                }
                Event::Html(h) => {
                    flush(&mut out, &mut loose);
                    out.push(Block::Html(h.to_string()));
                }
                Event::DisplayMath(m) => {
                    flush(&mut out, &mut loose);
                    out.push(Block::Math(m.to_string()));
                }
                Event::TaskListMarker(_) => {}
                other => {
                    let mut style = Style::default();
                    self.inline_event(other, &mut loose, &mut style);
                }
            }
        }
        flush(&mut out, &mut loose);
        out
    }

    fn block(&mut self, tag: Tag<'a>, start: usize) -> Option<Block> {
        Some(match tag {
            Tag::Paragraph => {
                let inl = self.inlines(TagEnd::Paragraph);
                // an embed alone in its paragraph is a block
                if let [Inline::Image { target, alt, internal: true }] = inl.as_slice() {
                    return Some(Block::Embed { target: target.clone(), alt: alt.clone() });
                }
                Block::Para(inl)
            }
            Tag::Heading { level, .. } => {
                let inl = self.inlines(TagEnd::Heading(level));
                let line = self.lines.line_of(start);
                self.headings.push((level as u8, plain(&inl), line));
                Block::Heading { level: level as u8, inlines: inl, line }
            }
            Tag::BlockQuote(_) => {
                let line = self.lines.line_of(start);
                let mut body = self.blocks(Some(TagEnd::BlockQuote(None)));
                // the end tag carries the kind; accept any blockquote end
                if let Some(Block::Para(first)) = body.first_mut() {
                    let text = match first.first() {
                        Some(Inline::Text(t, _)) => t.clone(),
                        _ => String::new(),
                    };
                    if let Some(c) = CALLOUT.captures(&text) {
                        let kind = c[1].to_lowercase();
                        let folded = match &c[2] {
                            "-" => Some(true),
                            "+" => Some(false),
                            _ => None,
                        };
                        let rest = text[c.get(0).unwrap().end()..].to_string();
                        let mut title_inl = first.clone();
                        if rest.is_empty() {
                            title_inl.remove(0);
                        } else {
                            title_inl[0] = Inline::Text(rest, Style::default());
                        }
                        // the title ends at the first line break
                        let split = title_inl.iter().position(|i| matches!(i, Inline::Break)).unwrap_or(title_inl.len());
                        let after: Vec<Inline> = title_inl.drain(split..).skip(1).collect();
                        let mut blocks: Vec<Block> = body.drain(1..).collect();
                        if !after.is_empty() {
                            blocks.insert(0, Block::Para(after));
                        }
                        if title_inl.is_empty() {
                            let mut t = kind.clone();
                            if let Some(f) = t.get_mut(0..1) {
                                f.make_ascii_uppercase();
                            }
                            title_inl.push(Inline::Text(t, Style::default()));
                        }
                        return Some(Block::Callout { kind, title: title_inl, body: blocks, folded, line });
                    }
                }
                Block::Quote(body)
            }
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let mut text = String::new();
                for (ev, _) in self.events.by_ref() {
                    match ev {
                        Event::Text(t) => text.push_str(&t),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                Block::Code { lang, text: text.trim_end_matches('\n').to_string() }
            }
            Tag::List(first) => {
                let mut items = Vec::new();
                while let Some((ev, r)) = self.events.next() {
                    match ev {
                        Event::Start(Tag::Item) => {
                            let line = self.lines.line_of(r.start);
                            let lt = self.lines.line_text(line);
                            let task = TASK.captures(lt).and_then(|c| c[1].chars().next());
                            let mut blocks = self.blocks(Some(TagEnd::Item));
                            if task.is_some() {
                                strip_task_marker(&mut blocks);
                            }
                            items.push(Item { task, line, blocks });
                        }
                        Event::End(TagEnd::List(_)) => break,
                        _ => {}
                    }
                }
                Block::List { start: first, items }
            }
            Tag::Table(_) => {
                let mut header = Vec::new();
                let mut rows = Vec::new();
                while let Some((ev, _)) = self.events.next() {
                    match ev {
                        Event::Start(Tag::TableHead) => header = self.cells(TagEnd::TableHead),
                        Event::Start(Tag::TableRow) => rows.push(self.cells(TagEnd::TableRow)),
                        Event::End(TagEnd::Table) => break,
                        _ => {}
                    }
                }
                Block::Table { header, rows }
            }
            Tag::HtmlBlock => {
                let mut h = String::new();
                for (ev, _) in self.events.by_ref() {
                    match ev {
                        Event::Html(t) | Event::Text(t) => h.push_str(&t),
                        Event::End(TagEnd::HtmlBlock) => break,
                        _ => {}
                    }
                }
                Block::Html(h)
            }
            Tag::FootnoteDefinition(_) => Block::Quote(self.blocks(Some(TagEnd::FootnoteDefinition))),
            Tag::Item => Block::List { start: None, items: vec![Item { task: None, line: 0, blocks: self.blocks(Some(TagEnd::Item)) }] },
            _ => return None,
        })
    }

    fn cells(&mut self, end: TagEnd) -> Vec<Vec<Inline>> {
        let mut out = Vec::new();
        while let Some((ev, _)) = self.events.next() {
            match ev {
                Event::Start(Tag::TableCell) => out.push(self.inlines(TagEnd::TableCell)),
                Event::End(t) if t == end => break,
                _ => {}
            }
        }
        out
    }

    fn inlines(&mut self, end: TagEnd) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut style = Style::default();
        while let Some((ev, _)) = self.events.next() {
            match ev {
                Event::End(t) if t == end => break,
                Event::Start(tag) => self.inline_start(tag, &mut out, &mut style),
                Event::End(TagEnd::Emphasis) => style.italic = false,
                Event::End(TagEnd::Strong) => style.bold = false,
                Event::End(TagEnd::Strikethrough) => style.strike = false,
                other => self.inline_event(other, &mut out, &mut style),
            }
        }
        tidy(out)
    }

    fn inline_start(&mut self, tag: Tag<'a>, out: &mut Vec<Inline>, style: &mut Style) {
        match tag {
            Tag::Emphasis => style.italic = true,
            Tag::Strong => style.bold = true,
            Tag::Strikethrough => style.strike = true,
            Tag::Link { link_type, dest_url, .. } => {
                let children = self.inlines(TagEnd::Link);
                let d = dest_url.to_string();
                let target = if let Some(t) = d.strip_prefix("wiki:") {
                    Target::Note(t.replace("%3E", ">"))
                } else if let Some(t) = d.strip_prefix("tag:") {
                    Target::Tag(t.to_string())
                } else if matches!(link_type, LinkType::Email) {
                    Target::Url(format!("mailto:{d}"))
                } else if syntax::is_external_url(&d) {
                    Target::Url(d)
                } else {
                    Target::Note(syntax::percent_decode(&d))
                };
                out.push(Inline::Link { target, children });
            }
            Tag::Image { dest_url, .. } => {
                let alt = plain(&self.inlines(TagEnd::Image));
                let d = dest_url.to_string();
                match d.strip_prefix("wiki:") {
                    Some(t) => out.push(Inline::Image { target: t.replace("%3E", ">"), alt, internal: true }),
                    None => {
                        let internal = !syntax::is_external_url(&d);
                        out.push(Inline::Image { target: if internal { syntax::percent_decode(&d) } else { d }, alt, internal });
                    }
                }
            }
            _ => {}
        }
    }

    fn inline_event(&mut self, ev: Event<'a>, out: &mut Vec<Inline>, style: &mut Style) {
        match ev {
            Event::Text(t) => out.push(Inline::Text(t.to_string(), *style)),
            Event::Code(t) => out.push(Inline::Code(t.to_string())),
            Event::SoftBreak => out.push(Inline::Text(" ".into(), *style)),
            Event::HardBreak => out.push(Inline::Break),
            Event::InlineMath(m) => out.push(Inline::Math(m.to_string())),
            Event::InlineHtml(h) => match h.trim() {
                "<mark>" => style.mark = true,
                "</mark>" => style.mark = false,
                "<br>" | "<br/>" | "<br />" => out.push(Inline::Break),
                "<b>" | "<strong>" => style.bold = true,
                "</b>" | "</strong>" => style.bold = false,
                "<i>" | "<em>" => style.italic = true,
                "</i>" | "</em>" => style.italic = false,
                _ => {}
            },
            Event::FootnoteReference(f) => out.push(Inline::Text(format!("[{f}]"), Style { ..*style })),
            _ => {}
        }
    }
}

fn is_inline(tag: &Tag) -> bool {
    matches!(tag, Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. })
}

fn flush(out: &mut Vec<Block>, loose: &mut Vec<Inline>) {
    if !loose.is_empty() {
        let inl = tidy(std::mem::take(loose));
        if let [Inline::Image { target, alt, internal: true }] = inl.as_slice() {
            out.push(Block::Embed { target: target.clone(), alt: alt.clone() });
        } else {
            out.push(Block::Para(inl));
        }
    }
}

/// Custom statuses (`[/]`, `[-]`…) are not tasks for CommonMark: drop the marker text.
fn strip_task_marker(blocks: &mut [Block]) {
    if let Some(Block::Para(inl)) = blocks.first_mut() {
        if let Some(Inline::Text(t, _)) = inl.first_mut() {
            if let Some(c) = Regex::new(r"^\s*\[.\]\s?").ok().and_then(|re| re.find(t).map(|m| m.end())) {
                t.drain(..c);
            }
        }
    }
}

// ── drawing ────────────────────────────────────────────────────────────────

/// What a click in the rendered note asks for.
#[derive(Clone, Debug)]
pub enum Action {
    Open(String, bool),
    Url(String),
    Tag(String),
    ToggleTask(usize, char),
    /// A task shown by a query: (note, line, current status).
    ToggleTaskIn(String, usize, char),
}

/// What the renderer needs from the app.
pub trait Host {
    fn palette(&self) -> crate::theme::Palette;
    fn size(&self) -> f32;
    fn source(&self) -> &str;
    fn resolved(&self, target: &str) -> bool;
    fn image(&mut self, ctx: &egui::Context, target: &str) -> Option<(egui::TextureId, egui::Vec2)>;
    /// A `dataview` / `dataviewjs` block: the result JSON (cached by the app).
    fn query(&mut self, lang: &str, source: &str) -> serde_json::Value;
    /// An inline query (`= expr` or `$= code`).
    fn inline_query(&mut self, js: bool, expr: &str) -> serde_json::Value;
    /// A note embedded with `![[note]]`: its path and parsed document.
    fn embed(&mut self, target: &str) -> Option<(String, Arc<Doc>)>;
}

/// Text with clickable ranges, laid out as one block.
pub struct Runs {
    pub job: LayoutJob,
    pub links: Vec<(Range<usize>, Action)>,
    chars: usize,
}

impl Runs {
    pub fn new() -> Runs {
        Runs { job: LayoutJob::default(), links: Vec::new(), chars: 0 }
    }

    pub fn push(&mut self, text: &str, fmt: TextFormat) {
        self.job.append(text, 0.0, fmt);
        self.chars += text.chars().count();
    }

    pub fn link(&mut self, text: &str, fmt: TextFormat, action: Action) {
        let start = self.chars;
        self.push(text, fmt);
        self.links.push((start..self.chars, action));
    }

    pub fn is_empty(&self) -> bool {
        self.chars == 0
    }

    /// Lay out and draw; returns the action of a clicked link.
    pub fn show(self, ui: &mut egui::Ui) -> Option<Action> {
        let w = ui.available_width();
        self.show_width(ui, w)
    }

    /// Table cells: their natural width up to a limit (a grid starts narrow).
    pub fn show_cell(self, ui: &mut egui::Ui) -> Option<Action> {
        self.show_width(ui, 380.0)
    }

    pub fn show_width(self, ui: &mut egui::Ui, width: f32) -> Option<Action> {
        if self.is_empty() {
            return None;
        }
        let mut job = self.job;
        job.wrap.max_width = width;
        let galley = ui.fonts(|f| f.layout_job(job));
        let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
        let hover_link = resp.hover_pos().and_then(|p| {
            let c = galley.cursor_from_pos(p - rect.min).ccursor.index;
            self.links.iter().find(|(r, _)| r.contains(&c) || (c > 0 && r.end == c && r.contains(&(c - 1)))).map(|(_, a)| a.clone())
        });
        if hover_link.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().galley(rect.min, galley, ui.visuals().text_color());
        if resp.clicked() {
            if let Some(a) = hover_link {
                let new_tab = ui.input(|i| i.modifiers.command);
                return Some(match a {
                    Action::Open(t, _) => Action::Open(t, new_tab),
                    other => other,
                });
            }
        }
        None
    }
}

impl Default for Runs {
    fn default() -> Self {
        Runs::new()
    }
}

pub struct Renderer<'h> {
    pub host: &'h mut dyn Host,
    pub actions: Vec<Action>,
    pub depth: usize,
    /// Scroll so the heading on this source line is at the top.
    pub scroll_to: Option<usize>,
    /// Drawing inside a table cell.
    pub in_cell: bool,
}

pub fn callout_color(kind: &str, p: &crate::theme::Palette) -> Color32 {
    match kind {
        "info" | "todo" | "note" => Color32::from_rgb(0x08, 0x6d, 0xdd),
        "abstract" | "summary" | "tldr" | "tip" | "hint" | "important" => Color32::from_rgb(0x00, 0xbf, 0xbc),
        "success" | "check" | "done" => Color32::from_rgb(0x08, 0xb9, 0x4e),
        "question" | "help" | "faq" | "warning" | "caution" | "attention" => Color32::from_rgb(0xec, 0x75, 0x00),
        "failure" | "fail" | "missing" | "danger" | "error" | "bug" => Color32::from_rgb(0xe9, 0x31, 0x47),
        "example" => Color32::from_rgb(0x78, 0x52, 0xee),
        "quote" | "cite" => p.muted,
        _ => Color32::from_rgb(0x08, 0x6d, 0xdd),
    }
}

fn callout_icon(kind: &str) -> &'static str {
    match kind {
        "info" => "ℹ",
        "todo" => "☑",
        "tip" | "hint" | "important" => "🔥",
        "abstract" | "summary" | "tldr" => "📋",
        "success" | "check" | "done" => "✔",
        "question" | "help" | "faq" => "❓",
        "warning" | "caution" | "attention" => "⚠",
        "failure" | "fail" | "missing" => "✖",
        "danger" | "error" => "⚡",
        "bug" => "🐞",
        "example" => "📑",
        "quote" | "cite" => "❝",
        _ => "✎",
    }
}

impl<'h> Renderer<'h> {
    pub fn new(host: &'h mut dyn Host) -> Self {
        Renderer { host, actions: Vec::new(), depth: 0, scroll_to: None, in_cell: false }
    }

    fn fmt(&self, size: f32, style: Style) -> TextFormat {
        let p = self.host.palette();
        let family = if style.bold { FontFamily::Name("bold".into()) } else if style.italic { FontFamily::Name("italic".into()) } else { FontFamily::Proportional };
        let mut f = TextFormat { font_id: FontId::new(size, family), color: p.text, ..Default::default() };
        if style.italic && style.bold {
            f.italics = true;
        }
        if style.strike {
            f.strikethrough = Stroke::new(1.0, p.text);
        }
        if style.mark {
            f.background = p.highlight;
        }
        f
    }

    /// Append inlines to a run list.
    pub fn runs(&mut self, runs: &mut Runs, inl: &[Inline], size: f32, base: Style, color: Option<Color32>) {
        let p = self.host.palette();
        for i in inl {
            match i {
                Inline::Text(t, s) => {
                    let mut f = self.fmt(size, merge(base, *s));
                    if let Some(c) = color {
                        f.color = c;
                    }
                    runs.push(t, f);
                }
                Inline::Code(t) => {
                    let trimmed = t.trim_start();
                    if let Some(expr) = trimmed.strip_prefix("$=").or_else(|| trimmed.strip_prefix('=')) {
                        let js = trimmed.starts_with("$=");
                        let res = self.host.inline_query(js, expr.trim());
                        crate::dv::value_runs(self, runs, &res, size, true);
                        continue;
                    }
                    let mut f = TextFormat { font_id: FontId::new(size * 0.88, FontFamily::Monospace), color: p.text, background: p.code_bg, ..Default::default() };
                    if let Some(c) = color {
                        f.color = c;
                    }
                    runs.push(t, f);
                }
                Inline::Math(t) => {
                    let f = TextFormat { font_id: FontId::new(size * 0.9, FontFamily::Monospace), color: p.muted, italics: true, ..Default::default() };
                    runs.push(t, f);
                }
                Inline::Break => runs.push("\n", self.fmt(size, base)),
                Inline::Field { key, value, paren } => {
                    let mono = |c: Color32, bg: Color32| TextFormat { font_id: FontId::new(size * 0.85, FontFamily::Monospace), color: c, background: bg, ..Default::default() };
                    if !paren {
                        runs.push(&format!(" {key} "), mono(p.muted, p.bg_hover));
                    }
                    runs.push(&format!(" {value} "), mono(p.text, p.bg_secondary));
                }
                Inline::Link { target, children } => {
                    let (action, ok) = match target {
                        Target::Note(t) => (Action::Open(t.clone(), false), self.host.resolved(t)),
                        Target::Url(u) => (Action::Url(u.clone()), true),
                        Target::Tag(t) => (Action::Tag(t.clone()), true),
                    };
                    let mut sub = Runs::new();
                    self.runs(&mut sub, children, size, base, None);
                    for mut section in sub.job.sections {
                        let text = &sub.job.text[section.byte_range.clone()];
                        section.format.color = if ok { p.link() } else { p.link().gamma_multiply(0.6) };
                        if matches!(target, Target::Tag(_)) {
                            section.format.background = p.accent.gamma_multiply(0.12);
                        } else {
                            section.format.underline = Stroke::new(1.0, section.format.color.gamma_multiply(if ok { 0.5 } else { 0.3 }));
                        }
                        runs.link(text, section.format, action.clone());
                    }
                }
                Inline::Image { alt, target, .. } => {
                    let label = if alt.is_empty() { target.clone() } else { alt.clone() };
                    let f = TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color: p.link(), ..Default::default() };
                    runs.link(&format!("🖼 {label}"), f, Action::Open(target.clone(), false));
                }
            }
        }
    }

    fn paragraph(&mut self, ui: &mut egui::Ui, inl: &[Inline], size: f32, base: Style, color: Option<Color32>) {
        // pictures inside a paragraph are drawn between its text parts
        let mut part: Vec<Inline> = Vec::new();
        for i in inl {
            if let Inline::Image { target, alt, internal } = i {
                self.flush_runs(ui, &part, size, base, color);
                part.clear();
                self.image(ui, target, alt, *internal);
            } else {
                part.push(i.clone());
            }
        }
        self.flush_runs(ui, &part, size, base, color);
    }

    fn flush_runs(&mut self, ui: &mut egui::Ui, inl: &[Inline], size: f32, base: Style, color: Option<Color32>) {
        if inl.is_empty() {
            return;
        }
        let mut runs = Runs::new();
        self.runs(&mut runs, inl, size, base, color);
        let a = if self.in_cell { runs.show_cell(ui) } else { runs.show(ui) };
        if let Some(a) = a {
            self.actions.push(a);
        }
    }

    fn image(&mut self, ui: &mut egui::Ui, target: &str, alt: &str, internal: bool) {
        if !internal {
            ui.hyperlink_to(format!("🖼 {}", if alt.is_empty() { target } else { alt }), target);
            return;
        }
        let ctx = ui.ctx().clone();
        match self.host.image(&ctx, target) {
            Some((tex, size)) => {
                // `![[img.png|300]]`: a number as alias sets the width
                let want = alt.trim().split('x').next().and_then(|w| w.parse::<f32>().ok());
                let max = ui.available_width();
                let w = want.unwrap_or(size.x).min(max);
                let h = size.y * w / size.x.max(1.0);
                ui.add(egui::Image::new((tex, egui::vec2(w, h))));
            }
            None => {
                let p = self.host.palette();
                ui.colored_label(p.faint, format!("“{target}” could not be found."));
            }
        }
    }

    pub fn blocks(&mut self, ui: &mut egui::Ui, blocks: &[Block]) {
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                ui.add_space(self.host.size() * 0.45);
            }
            self.block(ui, b);
        }
    }

    pub fn block(&mut self, ui: &mut egui::Ui, b: &Block) {
        let size = self.host.size();
        let p = self.host.palette();
        match b {
            Block::Heading { level, inlines, line } => {
                let scale = [1.75, 1.45, 1.25, 1.12, 1.05, 1.0][(*level as usize).clamp(1, 6) - 1];
                ui.add_space(size * 0.4);
                let top = ui.cursor().min;
                self.paragraph(ui, inlines, size * scale, Style { bold: true, ..Default::default() }, None);
                if self.scroll_to == Some(*line) && self.depth == 0 {
                    ui.scroll_to_rect(egui::Rect::from_min_size(top, egui::vec2(10.0, size * scale)), Some(egui::Align::TOP));
                    self.scroll_to = None;
                }
                if *level <= 1 {
                    ui.add_space(2.0);
                }
            }
            Block::Para(inl) => self.paragraph(ui, inl, size, Style::default(), None),
            Block::List { start, items } => {
                for (n, it) in items.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let marker_w = size * 1.4;
                        match it.task {
                            Some(c) => {
                                let mut checked = c != ' ';
                                let r = ui.add_sized([marker_w, size * 1.3], egui::Checkbox::without_text(&mut checked));
                                if r.clicked() {
                                    self.actions.push(Action::ToggleTask(it.line, c));
                                }
                            }
                            None => {
                                let text = match start {
                                    Some(s) => format!("{}.", s + n as u64),
                                    None => "•".to_string(),
                                };
                                ui.add_sized([marker_w, size * 1.3], egui::Label::new(egui::RichText::new(text).size(size).color(p.muted)));
                            }
                        }
                        ui.vertical(|ui| {
                            let done = matches!(it.task, Some(c) if c != ' ');
                            for (k, bl) in it.blocks.iter().enumerate() {
                                if k > 0 {
                                    ui.add_space(2.0);
                                }
                                match bl {
                                    Block::Para(inl) if done => self.paragraph(ui, inl, size, Style { strike: true, ..Default::default() }, Some(p.faint)),
                                    other => self.block(ui, other),
                                }
                            }
                        });
                    });
                }
            }
            Block::Quote(body) => {
                let resp = egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 4, top: 2, bottom: 2 }).show(ui, |ui| self.blocks(ui, body));
                let r = resp.response.rect;
                ui.painter().line_segment([r.left_top(), r.left_bottom()], Stroke::new(2.5, p.accent.gamma_multiply(0.7)));
            }
            Block::Callout { kind, title, body, folded, line } => {
                let color = callout_color(kind, &p);
                let id = ui.make_persistent_id(("callout", self.host.source().to_string(), *line));
                let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(folded != &Some(true));
                egui::Frame::new().fill(color.gamma_multiply(0.1)).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut runs = Runs::new();
                    let tf = TextFormat { font_id: FontId::new(size, FontFamily::Name("bold".into())), color, ..Default::default() };
                    runs.push(&format!("{}  ", callout_icon(kind)), tf.clone());
                    self.runs(&mut runs, title, size, Style { bold: true, ..Default::default() }, Some(color));
                    if folded.is_some() {
                        runs.push(if open { "  ▾" } else { "  ▸" }, tf);
                    }
                    let resp_before = ui.cursor();
                    if let Some(a) = runs.show(ui) {
                        self.actions.push(a);
                    } else if folded.is_some() && ui.rect_contains_pointer(egui::Rect::from_min_size(resp_before.min, egui::vec2(ui.available_width(), size * 1.5))) && ui.input(|i| i.pointer.primary_clicked()) {
                        open = !open;
                        ui.data_mut(|d| d.insert_temp(id, open));
                    }
                    if open && !body.is_empty() {
                        ui.add_space(6.0);
                        self.blocks(ui, body);
                    }
                });
            }
            Block::Code { lang, text } => {
                let l = lang.to_lowercase();
                if l == "dataview" || l == "dataviewjs" {
                    let res = self.host.query(&l, text);
                    crate::dv::render(self, ui, &res);
                    return;
                }
                egui::Frame::new().fill(p.code_bg).corner_radius(egui::CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::horizontal().id_salt(("code", text.len(), lang)).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(text).monospace().size(size * 0.85).color(p.text)).extend());
                    });
                });
            }
            Block::Table { header, rows } => {
                let cols = header.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
                self.in_cell = true;
                egui::ScrollArea::horizontal().id_salt(("table", rows.len(), cols)).show(ui, |ui| {
                    egui::Grid::new(ui.next_auto_id()).striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
                        for c in 0..cols {
                            let cell = header.get(c).cloned().unwrap_or_default();
                            self.paragraph(ui, &cell, size, Style { bold: true, ..Default::default() }, None);
                        }
                        ui.end_row();
                        for r in rows {
                            for c in 0..cols {
                                let cell = r.get(c).cloned().unwrap_or_default();
                                ui.vertical(|ui| self.paragraph(ui, &cell, size, Style::default(), None));
                            }
                            ui.end_row();
                        }
                    });
                });
                self.in_cell = false;
            }
            Block::Rule => {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            Block::Html(h) => {
                let text = strip_tags(h);
                if !text.trim().is_empty() {
                    ui.label(egui::RichText::new(text.trim()).size(size).color(p.muted));
                }
            }
            Block::Math(m) => {
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(m.trim()).monospace().italics().size(size).color(p.muted)));
            }
            Block::Embed { target, alt } => {
                let ext = target.rsplit('.').next().unwrap_or("").to_lowercase();
                if ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&ext.as_str()) {
                    self.image(ui, target.split('#').next().unwrap_or(target), alt, true);
                    return;
                }
                if self.depth >= 2 {
                    return;
                }
                match self.host.embed(target) {
                    Some((path, doc)) => {
                        egui::Frame::new().stroke(Stroke::new(1.0, p.border)).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let mut runs = Runs::new();
                            let f = TextFormat { font_id: FontId::new(size * 0.8, FontFamily::Proportional), color: p.muted, ..Default::default() };
                            runs.link(crate::vault::basename(&path), f, Action::Open(path.clone(), false));
                            if let Some(a) = runs.show(ui) {
                                self.actions.push(a);
                            }
                            self.depth += 1;
                            let section = target.split_once('#').map(|(_, s)| s.to_string());
                            let blocks = match section {
                                Some(s) => section_blocks(&doc, &s),
                                None => doc.blocks.clone(),
                            };
                            self.blocks(ui, &blocks);
                            self.depth -= 1;
                        });
                    }
                    None => {
                        ui.colored_label(p.faint, format!("“{target}” could not be found."));
                    }
                }
            }
        }
    }
}

fn merge(a: Style, b: Style) -> Style {
    Style { bold: a.bold || b.bold, italic: a.italic || b.italic, strike: a.strike || b.strike, mark: a.mark || b.mark }
}

fn strip_tags(h: &str) -> String {
    static TAGS: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]*>").unwrap());
    TAGS.replace_all(h, "").into_owned()
}

/// The blocks under a heading (for `![[note#Heading]]`).
fn section_blocks(doc: &Doc, heading: &str) -> Vec<Block> {
    let want = heading.trim().to_lowercase();
    let mut out = Vec::new();
    let mut level = None;
    for b in &doc.blocks {
        if let Block::Heading { level: l, inlines, .. } = b {
            if let Some(lv) = level {
                if *l <= lv {
                    break;
                }
            } else if plain(inlines).trim().to_lowercase() == want {
                level = Some(*l);
                continue;
            }
        }
        if level.is_some() {
            out.push(b.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree() {
        let d = parse("---\na: 1\n---\n# Title [[Note|alias]]\n\nText **bold** #tag ==hi== [k:: v]\n\n- [ ] task\n- [/] doing\n  - child\n\n> [!warning]- Careful\n> body\n\n![[pic.png]]\n\n```dataview\nLIST\n```\n");
        assert_eq!(d.headings, vec![(1, "Title alias".to_string(), 3)]);
        let kinds: Vec<&str> = d.blocks.iter().map(|b| match b {
            Block::Heading { .. } => "h",
            Block::Para(_) => "p",
            Block::List { .. } => "list",
            Block::Callout { .. } => "callout",
            Block::Embed { .. } => "embed",
            Block::Code { .. } => "code",
            _ => "?",
        }).collect();
        assert_eq!(kinds, vec!["h", "p", "list", "callout", "embed", "code"]);
        let Block::List { items, .. } = &d.blocks[2] else { panic!() };
        assert_eq!(items[0].task, Some(' '));
        assert_eq!(items[1].task, Some('/'));
        assert_eq!(items[0].line, 7);
        let Block::Para(p) = &d.blocks[1] else { panic!() };
        assert!(p.iter().any(|i| matches!(i, Inline::Link { target: Target::Tag(t), .. } if t == "tag")));
        assert!(p.iter().any(|i| matches!(i, Inline::Field { key, .. } if key == "k")));
        assert!(p.iter().any(|i| matches!(i, Inline::Text(_, s) if s.mark)));
        let Block::Callout { kind, folded, .. } = &d.blocks[3] else { panic!() };
        assert_eq!(kind, "warning");
        assert_eq!(*folded, Some(true));
    }
}
