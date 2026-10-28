//! Markdown → a small document tree (with Obsidian's wikilinks, embeds, tags,
//! callouts, highlights, inline fields and custom task statuses), drawn with
//! egui. A paragraph is one laid-out text block, so it wraps and selects like
//! text; links inside it are found from the click position.

use std::collections::HashMap;
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
    /// A footnote reference, by its number in order of first use.
    FootRef(u32),
    /// `$$…$$` met inside a paragraph (split out into a block when parsing).
    DisplayMath(String),
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
    /// The footnotes, numbered, gathered at the end of the note.
    Footnotes(Vec<(u32, Vec<Block>)>),
    /// Blank lines beyond the one that separates two blocks, kept as space.
    Space(usize),
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
    /// Footnote labels by number, and their definitions.
    foot: HashMap<String, u32>,
    defs: Vec<(String, Vec<Block>)>,
    /// Blocks a paragraph split off (display math in its middle).
    pending: Vec<Block>,
}

pub fn parse(text: &str) -> Doc {
    let src = prepare(text);
    let mut b = Builder { events: Parser::new_ext(&src, syntax::md_options()).into_offset_iter().peekable(), lines: LineIndex::new(&src), headings: Vec::new(), foot: HashMap::new(), defs: Vec::new(), pending: Vec::new() };
    let mut blocks = b.blocks(None);
    if !b.defs.is_empty() {
        // numbered as first referenced; ones never referenced come after
        let mut notes: Vec<(u32, Vec<Block>)> = Vec::new();
        let mut next = b.foot.len() as u32 + 1;
        for (label, body) in std::mem::take(&mut b.defs) {
            let n = match b.foot.get(&label) {
                Some(n) => *n,
                None => {
                    next += 1;
                    next - 1
                }
            };
            notes.push((n, body));
        }
        notes.sort_by_key(|x| x.0);
        blocks.push(Block::Footnotes(notes));
    }
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
            Inline::FootRef(_) => {}
            Inline::DisplayMath(m) => s.push_str(m),
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
        // the last line of the block before (top level only): the blank lines
        // written between two blocks show as that much space
        let mut last_line: Option<usize> = None;
        while let Some((ev, r)) = self.events.next() {
            if until.is_none() && r.end > r.start && !matches!(ev, Event::End(_)) {
                let first = self.lines.line_of(r.start);
                if let Some(prev) = last_line {
                    let blank = first.saturating_sub(prev + 1);
                    if blank > 1 && loose.is_empty() {
                        out.push(Block::Space(blank - 1));
                    }
                }
                last_line = Some(self.lines.line_of(r.end - 1).max(first));
            }
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
                    out.append(&mut self.pending);
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
                // display math splits the paragraph: text, formula, text
                if inl.iter().any(|i| matches!(i, Inline::DisplayMath(_))) {
                    let mut parts: Vec<Block> = Vec::new();
                    let mut cur: Vec<Inline> = Vec::new();
                    for i in inl {
                        match i {
                            Inline::DisplayMath(m) => {
                                let text = tidy(std::mem::take(&mut cur));
                                if text.iter().any(|x| !matches!(x, Inline::Text(t, _) if t.trim().is_empty()) && !matches!(x, Inline::Break)) {
                                    parts.push(Block::Para(text));
                                }
                                parts.push(Block::Math(m));
                            }
                            other => cur.push(other),
                        }
                    }
                    let text = tidy(cur);
                    if text.iter().any(|x| !matches!(x, Inline::Text(t, _) if t.trim().is_empty()) && !matches!(x, Inline::Break)) {
                        parts.push(Block::Para(text));
                    }
                    let first = parts.remove(0);
                    self.pending = parts;
                    return Some(first);
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
            Tag::FootnoteDefinition(label) => {
                let body = self.blocks(Some(TagEnd::FootnoteDefinition));
                self.defs.push((label.to_string(), body));
                return None;
            }
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
            // a single line break shows as one, as in Obsidian (without strict line breaks)
            Event::SoftBreak => out.push(Inline::Break),
            Event::HardBreak => out.push(Inline::Break),
            Event::InlineMath(m) => out.push(Inline::Math(m.to_string())),
            Event::DisplayMath(m) => out.push(Inline::DisplayMath(m.to_string())),
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
            Event::FootnoteReference(f) => {
                let k = self.foot.len() as u32 + 1;
                let n = *self.foot.entry(f.to_string()).or_insert(k);
                out.push(Inline::FootRef(n));
            }
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
    /// Play a link or an audio file of the vault (a `playlist` block).
    Play(String),
    /// Open a composed page (its id) of this note in the compositor.
    EditPage(String),
    /// A file of the vault (video, PDF…) in the system's app.
    External(String),
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
    /// A quadro (Kanban board or Excalidraw drawing) embedded with
    /// `![[…]]`, drawn small; its path and the picture's response.
    fn quadro(&mut self, _ui: &mut egui::Ui, _target: &str) -> Option<(String, egui::Response)> {
        None
    }
    /// A composed page (`scriptorium-pagina`) drawn as a picture, when ready.
    fn composed(&mut self, _ctx: &egui::Context, _model: &str) -> Option<(egui::TextureId, egui::Vec2)> {
        None
    }
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
        self.show_aligned(ui, w, crate::align::current())
    }

    /// Table cells: their natural width up to a limit (a grid starts narrow).
    pub fn show_cell(self, ui: &mut egui::Ui) -> Option<Action> {
        self.show_width(ui, 380.0)
    }

    pub fn show_width(self, ui: &mut egui::Ui, width: f32) -> Option<Action> {
        self.show_aligned(ui, width, crate::align::TextAlign::Left)
    }

    /// Laid out with the note's alignment (justified, centred, to the right).
    pub fn show_aligned(self, ui: &mut egui::Ui, width: f32, align: crate::align::TextAlign) -> Option<Action> {
        if self.is_empty() {
            return None;
        }
        let mut job = self.job;
        job.wrap.max_width = width;
        let galley = ui.fonts(|f| crate::align::layout(f, job, width, align));
        let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
        let hover_link = resp.hover_pos().and_then(|p| {
            let c = galley.cursor_from_pos(p - rect.min).ccursor.index;
            self.links.iter().find(|(r, _)| r.contains(&c) || (c > 0 && r.end == c && r.contains(&(c - 1)))).map(|(_, a)| a.clone())
        });
        if hover_link.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        // a note link under the pointer: the app may show its preview
        if let (Some(Action::Open(t, _)), Some(pos)) = (&hover_link, resp.hover_pos()) {
            let frame = ui.ctx().cumulative_pass_nr();
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("md-hover-link"), (t.clone(), pos, frame)));
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

/// A Mermaid flowchart drawn: boxes in their shapes, edges with arrows and
/// labels, scaled down to the width when it is wider.
fn mermaid_ui(ui: &mut egui::Ui, chart: &ateliet_core::mermaid::Chart, size: f32, p: crate::theme::Palette) {
    use ateliet_core::mermaid::{Line, Shape};
    let fs = size * 0.82;
    let font = FontId::proportional(fs);
    let measure = |t: &str| {
        let g = ui.fonts(|f| f.layout_no_wrap(t.to_string(), font.clone(), p.text));
        (g.size().x, g.size().y)
    };
    let lay = ateliet_core::mermaid::layout(chart, &measure);
    let pad = 16.0;
    let avail = ui.available_width() - 2.0 * pad;
    let k = if lay.width > avail { (avail / lay.width).max(0.3) } else { 1.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), lay.height * k + 2.0 * pad), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.bg_secondary);
    let ox = rect.left() + pad + (avail - lay.width * k).max(0.0) / 2.0;
    let oy = rect.top() + pad;
    let to = |x: f32, y: f32| egui::pos2(ox + x * k, oy + y * k);
    let boxes: Vec<egui::Rect> = lay.boxes.iter().map(|b| egui::Rect::from_min_max(to(b.0, b.1), to(b.0 + b.2, b.1 + b.3))).collect();
    let ink = p.muted;
    // where the line between two centres leaves a box
    let exit = |r: egui::Rect, toward: egui::Pos2| -> egui::Pos2 {
        let c = r.center();
        let d = toward - c;
        if d.length() < 1e-3 {
            return c;
        }
        let tx = if d.x.abs() > 1e-3 { (r.width() / 2.0) / d.x.abs() } else { f32::INFINITY };
        let ty = if d.y.abs() > 1e-3 { (r.height() / 2.0) / d.y.abs() } else { f32::INFINITY };
        c + d * tx.min(ty)
    };
    for e in &chart.edges {
        let (a, b) = (boxes[e.from], boxes[e.to]);
        let (pa, pb) = (exit(a, b.center()), exit(b, a.center()));
        let w: f32 = match e.line { Line::Thick => 2.6, _ => 1.3 };
        let st = Stroke::new(w, ink);
        if e.line == Line::Dotted {
            painter.extend(egui::Shape::dashed_line(&[pa, pb], st, 4.0, 3.0));
        } else {
            painter.line_segment([pa, pb], st);
        }
        if e.arrow {
            let dir = (pb - pa).normalized();
            let side = egui::vec2(-dir.y, dir.x);
            let l = 9.0 * k.max(0.6);
            painter.add(egui::Shape::convex_polygon(vec![pb, pb - dir * l + side * l * 0.45, pb - dir * l - side * l * 0.45], ink, Stroke::NONE));
        }
        if !e.label.is_empty() {
            let mid = pa + (pb - pa) * 0.5;
            let g = painter.layout_no_wrap(e.label.clone(), FontId::proportional(fs * 0.9 * k.max(0.7)), p.muted);
            let r = egui::Rect::from_center_size(mid, g.size() + egui::vec2(8.0, 4.0));
            painter.rect_filled(r, 4.0, p.bg_secondary);
            painter.galley(r.center() - g.size() / 2.0, g, p.muted);
        }
    }
    for (i, nd) in chart.nodes.iter().enumerate() {
        let r = boxes[i];
        let fill = p.bg;
        let st = Stroke::new(1.3_f32, p.border_strong);
        match nd.shape {
            Shape::Diamond => {
                let c = r.center();
                painter.add(egui::Shape::convex_polygon(vec![egui::pos2(c.x, r.top()), egui::pos2(r.right(), c.y), egui::pos2(c.x, r.bottom()), egui::pos2(r.left(), c.y)], fill, st));
            }
            Shape::Circle => {
                painter.circle(r.center(), r.width().min(r.height()) / 2.0, fill, st);
            }
            Shape::Hexagon => {
                let d = r.height() * 0.3;
                painter.add(egui::Shape::convex_polygon(vec![egui::pos2(r.left() + d, r.top()), egui::pos2(r.right() - d, r.top()), egui::pos2(r.right(), r.center().y), egui::pos2(r.right() - d, r.bottom()), egui::pos2(r.left() + d, r.bottom()), egui::pos2(r.left(), r.center().y)], fill, st));
            }
            Shape::Flag => {
                let d = r.height() * 0.3;
                painter.add(egui::Shape::convex_polygon(vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), egui::pos2(r.left() + d, r.center().y)], fill, st));
            }
            Shape::Round => {
                painter.rect(r, 8.0 * k, fill, st, egui::StrokeKind::Inside);
            }
            Shape::Stadium | Shape::Cylinder => {
                painter.rect(r, r.height() / 2.0, fill, st, egui::StrokeKind::Inside);
            }
            Shape::Subroutine => {
                painter.rect(r, 2.0, fill, st, egui::StrokeKind::Inside);
                painter.vline(r.left() + 6.0 * k, r.y_range(), st);
                painter.vline(r.right() - 6.0 * k, r.y_range(), st);
            }
            Shape::Rect => {
                painter.rect(r, 3.0, fill, st, egui::StrokeKind::Inside);
            }
        }
        let g = painter.layout(nd.label.clone(), FontId::proportional(fs * k), p.text, f32::INFINITY);
        painter.galley(r.center() - g.size() / 2.0, g, p.text);
    }
}

/// A list bullet the height of a line of text at `size`.
pub fn bullet(ui: &mut egui::Ui, size: f32, color: Color32, hollow: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size * 1.1, (size * 1.62).round()), Sense::hover());
    let c = rect.center();
    let r = (size * 0.16).max(2.2);
    if hollow {
        ui.painter().circle_stroke(c, r, Stroke::new(1.0_f32, color));
    } else {
        ui.painter().circle_filled(c, r, color);
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
    use crate::icons as i;
    match kind {
        "info" => i::INFO,
        "todo" => i::CIRCLE_DOT,
        "tip" | "hint" | "important" => i::FLAME,
        "abstract" | "summary" | "tldr" => i::CLIPBOARD,
        "success" | "check" | "done" => i::CHECK,
        "question" | "help" | "faq" => i::CIRCLE_HELP,
        "warning" | "caution" | "attention" => i::TRIANGLE_ALERT,
        "failure" | "fail" | "missing" => i::X,
        "danger" | "error" => i::TRIANGLE_ALERT,
        "bug" => i::TRIANGLE_ALERT,
        "example" => i::LIST,
        "quote" | "cite" => i::QUOTE,
        _ => i::PENCIL,
    }
}

impl<'h> Renderer<'h> {
    pub fn new(host: &'h mut dyn Host) -> Self {
        Renderer { host, actions: Vec::new(), depth: 0, scroll_to: None, in_cell: false }
    }

    fn fmt(&self, size: f32, style: Style) -> TextFormat {
        let p = self.host.palette();
        let family = if style.bold { FontFamily::Name("bold".into()) } else if style.italic { FontFamily::Name("italic".into()) } else { FontFamily::Proportional };
        let lh = if size > self.host.size() * 1.1 { 1.28 } else { 1.62 };
        let mut f = TextFormat { font_id: FontId::new(size, family), color: p.text, line_height: Some((size * lh).round()), valign: egui::Align::Center, ..Default::default() };
        if style.italic && style.bold {
            f.italics = true;
        }
        if style.strike {
            f.strikethrough = Stroke::new(1.0_f32, p.text);
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
                    // TeX read as symbols, in the text's italic
                    let mut f = self.fmt(size, merge(base, Style { italic: true, ..Default::default() }));
                    if let Some(c) = color {
                        f.color = c;
                    }
                    runs.push(&ateliet_core::texmath::convert(t), f);
                }
                Inline::DisplayMath(_) => {}
                Inline::FootRef(n) => {
                    // a small raised number in the accent colour
                    let f = TextFormat { font_id: FontId::new(size * 0.68, FontFamily::Proportional), color: p.accent_strong, valign: egui::Align::TOP, ..Default::default() };
                    runs.push(&n.to_string(), f);
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
                            section.format.background = p.accent_soft;
                            section.format.color = p.accent_strong;
                        } else {
                            section.format.underline = Stroke::new(1.0_f32, section.format.color.gamma_multiply(if ok { 0.5 } else { 0.3 }));
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
        let size = self.host.size();
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                // headings bring their own space above
                let gap = match b {
                    Block::Space(_) => 0.0,
                    Block::Heading { level, .. } => size * if *level <= 2 { 1.5 } else { 1.1 },
                    _ => size * 0.7,
                };
                ui.add_space(gap);
            }
            self.block(ui, b);
        }
    }

    pub fn block(&mut self, ui: &mut egui::Ui, b: &Block) {
        let size = self.host.size();
        let p = self.host.palette();
        match b {
            Block::Heading { level, inlines, line } => {
                let scale = [1.6, 1.32, 1.16, 1.05, 1.0, 0.92][(*level as usize).clamp(1, 6) - 1];
                let top = ui.cursor().min;
                self.paragraph(ui, inlines, size * scale, Style { bold: true, ..Default::default() }, None);
                if self.scroll_to == Some(*line) && self.depth == 0 {
                    ui.scroll_to_rect(egui::Rect::from_min_size(top, egui::vec2(10.0, size * scale)), Some(egui::Align::TOP));
                    self.scroll_to = None;
                }
                ui.add_space(size * 0.05);
            }
            Block::Para(inl) => self.paragraph(ui, inl, size, Style::default(), None),
            Block::List { start, items } => {
                for (n, it) in items.iter().enumerate() {
                    if n > 0 {
                        ui.add_space(size * 0.2);
                    }
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let marker_w = size * 1.3;
                        match it.task {
                            Some(c) => {
                                let mut checked = c != ' ';
                                let r = ui.add_sized([marker_w, (size * 1.62).round()], egui::Checkbox::without_text(&mut checked));
                                if r.clicked() {
                                    self.actions.push(Action::ToggleTask(it.line, c));
                                }
                            }
                            None => {
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(marker_w, (size * 1.62).round()), Sense::hover());
                                match start {
                                    Some(s) => {
                                        ui.painter().text(egui::pos2(rect.right() - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, format!("{}.", s + n as u64), FontId::proportional(size), p.muted);
                                    }
                                    None => {
                                        ui.painter().circle_filled(egui::pos2(rect.center().x, rect.center().y), (size * 0.16).max(2.2), p.faint);
                                    }
                                }
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
                let resp = egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 4, top: 2, bottom: 2 }).show(ui, |ui| self.blocks(ui, body));
                let r = resp.response.rect;
                ui.painter().line_segment([r.left_top() + egui::vec2(1.0, 2.0), r.left_bottom() + egui::vec2(1.0, -2.0)], Stroke::new(2.0_f32, p.border_strong));
            }
            Block::Callout { kind, title, body, folded, line } => {
                let color = callout_color(kind, &p);
                let id = ui.make_persistent_id(("callout", self.host.source().to_string(), *line));
                let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(folded != &Some(true));
                let fill = if p.dark { color.gamma_multiply(0.14) } else { color.gamma_multiply(0.07) };
                egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 12 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut runs = Runs::new();
                    let tf = TextFormat { font_id: FontId::new(size * 0.95, FontFamily::Name("icons".into())), color, valign: egui::Align::Center, ..Default::default() };
                    runs.push(callout_icon(kind), tf.clone());
                    runs.push("  ", self.fmt(size, Style::default()));
                    self.runs(&mut runs, title, size, Style { bold: true, ..Default::default() }, Some(color));
                    if folded.is_some() {
                        runs.push("  ", self.fmt(size, Style::default()));
                        runs.push(if open { crate::icons::CHEVRON_DOWN } else { crate::icons::CHEVRON_RIGHT }, tf);
                    }
                    let resp_before = ui.cursor();
                    if let Some(a) = runs.show(ui) {
                        self.actions.push(a);
                    } else if folded.is_some() && ui.rect_contains_pointer(egui::Rect::from_min_size(resp_before.min, egui::vec2(ui.available_width(), size * 1.5))) && ui.input(|i| i.pointer.primary_clicked()) {
                        open = !open;
                        ui.data_mut(|d| d.insert_temp(id, open));
                    }
                    if open && !body.is_empty() {
                        ui.add_space(size * 0.4);
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
                if l == ateliet_core::typeset::compose::PAGE_LANG {
                    if let Ok(model) = serde_json::from_str::<serde_json::Value>(text) {
                        let id = model["id"].as_str().unwrap_or("").to_string();
                        let dupla = model["tipo"].as_str() == Some("dupla");
                        let w = ui.available_width().min(if dupla { 640.0 } else { 360.0 });
                        ui.vertical_centered(|ui| {
                            match self.host.composed(ui.ctx(), text) {
                                Some((tex, sz)) => {
                                    let h = w * sz.y / sz.x;
                                    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
                                    ui.painter().rect_filled(r.translate(egui::vec2(0.0, 2.0)), 2.0, egui::Color32::from_black_alpha(if p.dark { 80 } else { 24 }));
                                    ui.painter().image(tex, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).double_clicked() {
                                        self.actions.push(Action::EditPage(id.clone()));
                                    }
                                }
                                None => {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(w, w * 1.4), Sense::hover());
                                    ui.painter().rect_filled(r, 2.0, p.code_bg);
                                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Compondo a página…", egui::FontId::proportional(size * 0.8), p.faint);
                                }
                            }
                            ui.add_space(4.0);
                            if crate::kit::quiet(ui, Some(crate::icons::PENCIL), if dupla { "Editar página dupla" } else { "Editar página composta" }, p).clicked() {
                                self.actions.push(Action::EditPage(id.clone()));
                            }
                        });
                        return;
                    }
                }
                if l == "mermaid" {
                    if let Some(chart) = ateliet_core::mermaid::parse(text) {
                        mermaid_ui(ui, &chart, size, p);
                        return;
                    }
                }
                if l == "playlist" {
                    let lines: Vec<String> = text.lines().map(String::from).collect();
                    let items = ateliet_core::scriptorium::trilha::playlist_items(&lines);
                    egui::Frame::new().fill(p.code_bg).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if items.is_empty() {
                            ui.label(egui::RichText::new("Playlist vazia. Cole links do Spotify, Apple Music, YouTube, SoundCloud ou [[áudio.mp3]] do cofre, um por linha.").size(size * 0.85).color(p.faint));
                        }
                        for (title, src) in items {
                            let local = !src.starts_with("http");
                            let name = if !title.is_empty() { title.clone() } else if local { src.trim_start_matches('!').trim_start_matches("[[").trim_end_matches("]]").to_string() } else { src.clone() };
                            let kind = if local { "áudio do cofre".to_string() } else { ateliet_core::scriptorium::trilha::service(&src).map(|s| format!("abrir no {s}")).unwrap_or_else(|| "abrir".into()) };
                            let r = crate::kit::Row::new(&name).icon(if local { crate::icons::PLAY } else { crate::icons::MUSIC }).trailing(kind).show(ui, p);
                            if r.clicked() {
                                self.actions.push(Action::Play(src.clone()));
                            }
                        }
                    });
                    return;
                }
                egui::Frame::new().fill(p.code_bg).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
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
                    // hairlines between rows instead of stripes
                    let mut ys: Vec<f32> = Vec::new();
                    let g = egui::Grid::new(ui.next_auto_id()).striped(false).spacing([28.0, 9.0]).show(ui, |ui| {
                        for c in 0..cols {
                            let cell = header.get(c).cloned().unwrap_or_default();
                            self.paragraph(ui, &cell, size * 0.88, Style { bold: true, ..Default::default() }, Some(p.muted));
                        }
                        ui.end_row();
                        ys.push(ui.cursor().top() - 4.5);
                        for r in rows {
                            for c in 0..cols {
                                let cell = r.get(c).cloned().unwrap_or_default();
                                ui.vertical(|ui| self.paragraph(ui, &cell, size, Style::default(), None));
                            }
                            ui.end_row();
                            ys.push(ui.cursor().top() - 4.5);
                        }
                    });
                    let x = g.response.rect.x_range();
                    let last = ys.len().saturating_sub(1);
                    for (k, y) in ys.into_iter().enumerate() {
                        if k == last && k > 0 {
                            break;
                        }
                        ui.painter().hline(x, y, Stroke::new(1.0_f32, if k == 0 { p.border_strong } else { p.border }));
                    }
                });
                self.in_cell = false;
            }
            Block::Space(n) => ui.add_space(size * 1.45 * *n as f32),
            Block::Rule => {
                ui.add_space(size * 0.6);
                let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, p.border_strong));
                ui.add_space(size * 0.6);
            }
            Block::Html(h) => {
                let text = strip_tags(h);
                if !text.trim().is_empty() {
                    ui.label(egui::RichText::new(text.trim()).size(size).color(p.muted));
                }
            }
            Block::Math(m) => {
                let text = ateliet_core::texmath::convert(m.trim());
                ui.add_space(size * 0.3);
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(text.trim()).family(FontFamily::Name("italic".into())).size(size * 1.12).color(p.text)));
                ui.add_space(size * 0.3);
            }
            Block::Footnotes(notes) => {
                ui.add_space(size * 0.8);
                let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, p.border));
                ui.add_space(size * 0.5);
                for (n, body) in notes {
                    ui.horizontal_top(|ui| {
                        let (nr, _) = ui.allocate_exact_size(egui::vec2(size * 1.6, size * 1.3), Sense::hover());
                        ui.painter().text(nr.right_top() + egui::vec2(-6.0, size * 0.18), egui::Align2::RIGHT_TOP, format!("{n}."), FontId::proportional(size * 0.85), p.muted);
                        ui.vertical(|ui| {
                            ui.scope(|ui| {
                                ui.style_mut().override_font_id = Some(FontId::proportional(size * 0.88));
                                self.blocks(ui, body);
                            });
                        });
                    });
                }
            }
            Block::Embed { target, alt } => {
                let ext = target.rsplit('.').next().unwrap_or("").to_lowercase();
                if ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&ext.as_str()) {
                    self.image(ui, target.split('#').next().unwrap_or(target), alt, true);
                    return;
                }
                // audio plays here; video and PDF open in the system's app
                let media = match ext.as_str() {
                    "mp3" | "wav" | "m4a" | "ogg" | "flac" | "webm" | "3gp" => Some((crate::icons::PLAY, "áudio · tocar", true)),
                    "mp4" | "mov" | "mkv" | "ogv" | "avi" => Some((crate::icons::FILM, "vídeo · abrir", false)),
                    "pdf" => Some((crate::icons::FILE_TEXT, "PDF · abrir", false)),
                    _ => None,
                };
                if let Some((icon, kind, audio)) = media {
                    let file = target.split('#').next().unwrap_or(target).to_string();
                    let name = file.rsplit('/').next().unwrap_or(&file).to_string();
                    egui::Frame::new().fill(p.bg_secondary).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let r = crate::kit::Row::new(&name).icon(icon).trailing(kind.to_string()).show(ui, p);
                        if r.clicked() {
                            self.actions.push(if audio { Action::Play(format!("[[{file}]]")) } else { Action::External(file.clone()) });
                        }
                    });
                    return;
                }
                if self.depth >= 2 {
                    return;
                }
                if let Some((path, r)) = self.host.quadro(ui, target.split('#').next().unwrap_or(target)) {
                    if r.on_hover_text("Abrir no Quadro").clicked() {
                        self.actions.push(Action::Open(path, false));
                    }
                    return;
                }
                match self.host.embed(target) {
                    Some((path, doc)) => {
                        egui::Frame::new().fill(p.bg_secondary).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
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

    #[test]
    fn blank_lines_and_line_breaks_are_kept() {
        let d = parse("um\ndois\n\n\n\ntrês\n\nquatro");
        let Block::Para(inl) = &d.blocks[0] else { panic!() };
        assert!(inl.iter().any(|i| matches!(i, Inline::Break)));
        assert!(matches!(d.blocks[1], Block::Space(2)), "{:?}", d.blocks);
        assert!(matches!(d.blocks[2], Block::Para(_)));
        assert!(matches!(d.blocks[3], Block::Para(_)), "one blank line is the usual gap");
    }
}
