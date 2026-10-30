//! EPUB: `META-INF/container.xml` points to the package; the package
//! gives the metadata, the manifest and the spine; `nav.xhtml` (EPUB 3)
//! or `toc.ncx` (EPUB 2) the contents. Each spine document becomes blocks
//! (`flow::Block`) with the little CSS that changes reading — italics,
//! bold, small caps, alignment, first-line indent, block margins, a page
//! break before — and the book's own fonts, colours and sizes left out.
//! Footnotes kept in `<aside epub:type="footnote">` leave the text and
//! open in a balloon from their reference. A fixed-layout book whose
//! pages are pictures reads like a comic. Encrypted books are refused.

use super::archive::Zip;
use super::flow::{self, Align, Block, Chapter, Kind, Laid, LinkSpec, Span, TextBlock};
use super::xml::{self, Tok};
use super::{Document, Format, Glyph, Hit, Link, Locator, Meta, Quote, Rgba, Target, TextLayer, TocEntry, View};
use crate::typeset::fonts::FontDb;
use crate::typeset::model::Style;
use crate::typeset::Picture;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct Epub {
    zip: Zip,
    fonts: Arc<FontDb>,
    meta: Meta,
    toc: Vec<TocEntry>,
    /// The spine's documents (paths in the archive).
    spine: Vec<String>,
    cover: Option<String>,
    /// A fixed-layout book of pictures: one picture a page.
    fixed: Option<Vec<String>>,
    css: HashMap<String, Arc<Css>>,
    chapters: HashMap<usize, Arc<Chapter>>,
    /// Chapters laid out lately: (chapter, view key) → pages.
    laid: Vec<(usize, String, Arc<Laid>)>,
    faces: Option<(String, crate::typeset::lines::Faces)>,
}

// ── CSS ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
struct Compound {
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Decl {
    italic: Option<bool>,
    bold: Option<bool>,
    smallcaps: Option<bool>,
    upper: Option<bool>,
    mono: Option<bool>,
    strike: Option<bool>,
    sup: Option<bool>,
    scale: Option<f32>,
    align: Option<Align>,
    indent: Option<f32>,
    mt: Option<f32>,
    mb: Option<f32>,
    ml: Option<f32>,
    mr: Option<f32>,
    none: Option<bool>,
    block: Option<bool>,
    break_before: Option<bool>,
}

impl Decl {
    fn over(&mut self, o: &Decl) {
        macro_rules! take {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f; } )* };
        }
        take!(italic, bold, smallcaps, upper, mono, strike, sup, scale, align, indent, mt, mb, ml, mr, none, block, break_before);
    }
}

#[derive(Clone, Debug, Default)]
pub struct Css {
    rules: Vec<(Vec<Compound>, Decl)>,
}

/// A length in em: em, rem, %, px, pt; `pct_em` is what 1% is worth.
fn length(v: &str, pct_em: f32) -> Option<f32> {
    let v = v.trim();
    if v == "0" || v == "auto" {
        return Some(0.0);
    }
    let num = |s: &str| s.trim().parse::<f32>().ok();
    if let Some(n) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
        return num(n);
    }
    if let Some(n) = v.strip_suffix('%') {
        return num(n).map(|x| x * pct_em);
    }
    if let Some(n) = v.strip_suffix("px") {
        return num(n).map(|x| x / 16.0);
    }
    if let Some(n) = v.strip_suffix("pt") {
        return num(n).map(|x| x / 12.0);
    }
    if let Some(n) = v.strip_suffix("ex") {
        return num(n).map(|x| x * 0.5);
    }
    num(v).filter(|x| *x == 0.0)
}

fn declarations(s: &str) -> Decl {
    let mut d = Decl::default();
    for part in s.split(';') {
        let Some((k, v)) = part.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
        match k.as_str() {
            "font-style" => d.italic = Some(v == "italic" || v == "oblique"),
            "font-weight" => d.bold = Some(v == "bold" || v == "bolder" || v.parse::<u32>().is_ok_and(|w| w >= 600)),
            "font-variant" | "font-variant-caps" => d.smallcaps = Some(v.contains("small-caps")),
            "text-transform" => d.upper = Some(v == "uppercase"),
            "font-family" => d.mono = Some(v.contains("mono") || v.contains("courier") || v.contains("consol")),
            "text-decoration" | "text-decoration-line" => d.strike = Some(v.contains("line-through")),
            "vertical-align" => d.sup = Some(v == "super" || v == "sup" || v == "top" || v == "text-top"),
            "font-size" => {
                d.scale = match v.as_str() {
                    "smaller" | "small" => Some(0.85),
                    "x-small" | "xx-small" => Some(0.75),
                    "larger" | "large" => Some(1.15),
                    "x-large" => Some(1.35),
                    "xx-large" => Some(1.6),
                    "medium" | "inherit" | "1em" | "100%" => Some(1.0),
                    _ => v.strip_suffix("em").or_else(|| v.strip_suffix("rem")).and_then(|n| n.trim().parse::<f32>().ok()).or_else(|| v.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()).map(|x| x / 100.0)),
                }
            }
            "text-align" => {
                d.align = match v.as_str() {
                    "center" => Some(Align::Center),
                    "right" | "end" => Some(Align::Right),
                    "justify" => Some(Align::Justify),
                    "left" | "start" => Some(Align::Left),
                    _ => None,
                }
            }
            "text-indent" => d.indent = length(&v, 0.3),
            "margin-top" | "padding-top" => d.mt = length(&v, 0.0).or(d.mt),
            "margin-bottom" | "padding-bottom" => d.mb = length(&v, 0.0).or(d.mb),
            "margin-left" | "padding-left" => d.ml = length(&v, 0.3).or(d.ml),
            "margin-right" | "padding-right" => d.mr = length(&v, 0.3).or(d.mr),
            "margin" => {
                let p: Vec<&str> = v.split_whitespace().collect();
                let at = |i: usize| p.get(i).and_then(|x| length(x, 0.3));
                match p.len() {
                    1 => {
                        d.mt = at(0);
                        d.mb = at(0);
                        d.ml = at(0);
                        d.mr = at(0);
                    }
                    2 => {
                        d.mt = at(0);
                        d.mb = at(0);
                        d.ml = at(1);
                        d.mr = at(1);
                    }
                    3 => {
                        d.mt = at(0);
                        d.ml = at(1);
                        d.mr = at(1);
                        d.mb = at(2);
                    }
                    4 => {
                        d.mt = at(0);
                        d.mr = at(1);
                        d.mb = at(2);
                        d.ml = at(3);
                    }
                    _ => {}
                }
            }
            "display" => {
                d.none = Some(v == "none");
                if v == "block" {
                    d.block = Some(true);
                }
            }
            "visibility" if v == "hidden" => d.none = Some(true),
            "page-break-before" | "break-before" => d.break_before = Some(matches!(v.as_str(), "always" | "page" | "left" | "right" | "recto" | "verso")),
            _ => {}
        }
    }
    // sizes far from the text's are the book's design, not reading: tamed
    if let Some(s) = d.scale {
        d.scale = Some(s.clamp(0.7, 2.2));
    }
    d
}

fn compound(s: &str) -> Option<Compound> {
    let s = s.trim();
    if s.is_empty() || s.contains(':') || s.contains('[') {
        return None;
    }
    let mut c = Compound::default();
    let mut cur = String::new();
    let mut kind = 't';
    let flush = |cur: &mut String, kind: char, c: &mut Compound| {
        if cur.is_empty() {
            return;
        }
        match kind {
            '.' => c.classes.push(std::mem::take(cur)),
            '#' => c.id = Some(std::mem::take(cur)),
            _ => {
                let t = std::mem::take(cur).to_ascii_lowercase();
                if t != "*" {
                    c.tag = Some(t.rsplit('|').next().unwrap_or(&t).to_string());
                }
            }
        }
    };
    for ch in s.chars() {
        if ch == '.' || ch == '#' {
            flush(&mut cur, kind, &mut c);
            kind = ch;
        } else {
            cur.push(ch);
        }
    }
    flush(&mut cur, kind, &mut c);
    Some(c)
}

impl Css {
    pub fn parse(src: &str) -> Css {
        let mut css = Css::default();
        let mut s = String::with_capacity(src.len());
        // comments out
        let mut rest = src;
        while let Some(i) = rest.find("/*") {
            s.push_str(&rest[..i]);
            rest = rest[i + 2..].find("*/").map(|e| &rest[i + 2 + e + 2..]).unwrap_or("");
        }
        s.push_str(rest);
        css.block(&s);
        css
    }

    fn block(&mut self, s: &str) {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let Some(open) = s[i..].find('{').map(|x| x + i) else { break };
            let prelude = s[i..open].trim();
            // the matching close
            let mut depth = 0;
            let mut close = b.len();
            for (j, &c) in b.iter().enumerate().skip(open) {
                if c == b'{' {
                    depth += 1;
                } else if c == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        close = j;
                        break;
                    }
                }
            }
            let body = &s[open + 1..close.min(s.len())];
            if prelude.starts_with("@media") || prelude.starts_with("@supports") {
                if !prelude.contains("print") || prelude.contains("screen") {
                    self.block(body);
                }
            } else if !prelude.starts_with('@') {
                let d = declarations(body);
                for sel in prelude.split(',') {
                    let parts: Option<Vec<Compound>> = sel.split(|c: char| c.is_whitespace() || c == '>' || c == '+' || c == '~').filter(|p| !p.trim().is_empty()).map(compound).collect();
                    if let Some(p) = parts.filter(|p| !p.is_empty()) {
                        self.rules.push((p, d.clone()));
                    }
                }
            }
            i = close + 1;
        }
    }
}

/// An element as the cascade sees it.
#[derive(Clone, Debug)]
struct El {
    name: String,
    classes: Vec<String>,
    id: Option<String>,
}

fn matches(c: &Compound, e: &El) -> bool {
    c.tag.as_ref().is_none_or(|t| *t == e.name) && c.id.as_ref().is_none_or(|i| e.id.as_ref() == Some(i)) && c.classes.iter().all(|k| e.classes.iter().any(|x| x == k))
}

fn rule_applies(sel: &[Compound], stack: &[El], e: &El) -> bool {
    let Some((last, before)) = sel.split_last() else { return false };
    if !matches(last, e) {
        return false;
    }
    // the rest, in order, among the ancestors
    let mut k = stack.len();
    for c in before.iter().rev() {
        loop {
            if k == 0 {
                return false;
            }
            k -= 1;
            if matches(c, &stack[k]) {
                break;
            }
        }
    }
    true
}

// ── XHTML → blocks ─────────────────────────────────────────────────────

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li" | "ul" | "ol" | "dl" | "dt" | "dd" | "pre" | "figure" | "figcaption" | "section" | "article" | "aside" | "header" | "footer" | "nav" | "main" | "table" | "tr" | "caption" | "address" | "center" | "hgroup" | "body" | "html" | "thead" | "tbody" | "tfoot" | "details" | "summary" | "fieldset" | "legend"
    )
}

/// The style an element brings (on top of what it inherits).
fn tag_defaults(name: &str) -> Decl {
    let mut d = Decl::default();
    match name {
        "em" | "i" | "cite" | "var" | "dfn" | "address" => d.italic = Some(true),
        "strong" | "b" | "th" | "dt" => d.bold = Some(true),
        "sup" => d.sup = Some(true),
        "sub" | "small" => d.scale = Some(0.82),
        "code" | "kbd" | "samp" | "tt" | "pre" => d.mono = Some(true),
        "s" | "strike" | "del" => d.strike = Some(true),
        "blockquote" => {
            d.ml = Some(1.6);
            d.mr = Some(1.2);
            d.mt = Some(0.6);
            d.mb = Some(0.6);
        }
        "dd" => d.ml = Some(1.6),
        "ul" | "ol" => {
            d.mt = Some(0.4);
            d.mb = Some(0.4);
        }
        "center" => d.align = Some(Align::Center),
        "figcaption" | "caption" => {
            d.align = Some(Align::Center);
            d.italic = Some(true);
        }
        "figure" => {
            d.mt = Some(0.6);
            d.mb = Some(0.6);
        }
        _ => {}
    }
    d
}

/// The inherited state of an element.
#[derive(Clone, Debug)]
struct Cur {
    st: Style,
    align: Option<Align>,
    indent: Option<f32>,
    /// Left and right insets so far (em).
    left: f32,
    right: f32,
    pre: bool,
    link: Option<usize>,
    /// Inside a footnote kept out of the text: its id.
    note: Option<String>,
    skip: bool,
    heading: Option<u8>,
    caption: bool,
}

struct Builder<'a> {
    path: &'a str,
    css: &'a [Arc<Css>],
    ch: Chapter,
    cur: Option<TextBlock>,
    /// Letters so far.
    count: usize,
    /// Margin owed before the next block (em) and a page break.
    pending_before: f32,
    pending_break: bool,
    /// The next paragraph starts flush (after a heading, at the start).
    flush_next: bool,
    /// The marker of the next block (a list item's).
    bullet: Option<String>,
    lists: Vec<(bool, u64)>,
    note_text: String,
    open_link: Option<(usize, usize)>,
    in_title: bool,
}

impl Builder<'_> {
    fn flush(&mut self) {
        if let Some(mut b) = self.cur.take() {
            // trailing space out
            while let Some(last) = b.spans.last_mut() {
                let t = last.text.trim_end_matches([' ', '\n']).to_string();
                if t.is_empty() {
                    b.spans.pop();
                } else {
                    last.text = t;
                    break;
                }
            }
            if b.spans.iter().any(|s| !s.text.trim().is_empty()) {
                if matches!(b.kind, Kind::Head(_)) {
                    self.flush_next = true;
                    if self.ch.title.is_empty() {
                        self.ch.title = b.spans.iter().map(|s| s.text.as_str()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
                    }
                } else if b.kind == Kind::Para {
                    self.flush_next = false;
                }
                self.ch.blocks.push(Block::Text(b));
            }
        }
    }

    fn start(&mut self, c: &Cur) {
        if self.cur.is_some() {
            return;
        }
        let kind = match (c.heading, c.pre, c.caption) {
            (Some(l), _, _) => Kind::Head(l),
            (_, true, _) => Kind::Pre,
            (_, _, true) => Kind::Caption,
            _ => Kind::Para,
        };
        let indent = match c.indent {
            Some(i) => Some(i),
            None if self.flush_next && kind == Kind::Para => Some(0.0),
            None => None,
        };
        self.cur = Some(TextBlock {
            spans: Vec::new(),
            kind,
            align: c.align,
            indent,
            before: std::mem::take(&mut self.pending_before),
            after: 0.0,
            left: c.left,
            right: c.right,
            bullet: self.bullet.take(),
            break_before: std::mem::take(&mut self.pending_break),
            off: self.count,
            cont: false,
        });
    }

    fn text(&mut self, t: &str, c: &Cur) {
        if c.skip {
            return;
        }
        if c.note.is_some() {
            self.note_text.push_str(t);
            return;
        }
        let s: String = if c.pre {
            t.replace("\r\n", "\n")
        } else {
            let mut o = String::with_capacity(t.len());
            let mut sp = false;
            for ch in t.chars() {
                if ch.is_whitespace() && ch != '\u{a0}' {
                    sp = true;
                } else {
                    if sp {
                        o.push(' ');
                    }
                    sp = false;
                    o.push(ch);
                }
            }
            if sp {
                o.push(' ');
            }
            o
        };
        if s.is_empty() {
            return;
        }
        if self.cur.is_none() {
            if s.trim().is_empty() {
                return;
            }
            self.start(c);
        }
        let b = self.cur.as_mut().unwrap();
        let at_start = b.spans.iter().all(|x| x.text.is_empty() || x.text.ends_with('\n'));
        let s = if at_start && !c.pre { s.trim_start().to_string() } else { s };
        // one space between spans
        let s = if !c.pre && s.starts_with(' ') && b.spans.last().is_some_and(|l| l.text.ends_with(' ')) { s[1..].to_string() } else { s };
        if s.is_empty() {
            return;
        }
        self.count += flow::letters(&s);
        match b.spans.last_mut() {
            Some(l) if l.st == c.st && l.link == c.link => l.text.push_str(&s),
            _ => b.spans.push(Span { text: s, st: c.st.clone(), link: c.link }),
        }
    }
}

/// A chapter's XHTML → blocks.
pub fn chapter(src: &str, path: &str, css: &[Arc<Css>]) -> Chapter {
    let toks = xml::tokens(src);
    let mut b = Builder { path, css, ch: Chapter::default(), cur: None, count: 0, pending_before: 0.0, pending_break: false, flush_next: true, bullet: None, lists: Vec::new(), note_text: String::new(), open_link: None, in_title: false };
    let root = Cur { st: Style::plain(), align: None, indent: None, left: 0.0, right: 0.0, pre: false, link: None, note: None, skip: false, heading: None, caption: false };
    let mut stack: Vec<(El, Cur, bool, Decl)> = Vec::new();
    let mut els: Vec<El> = Vec::new();
    for t in toks {
        match t {
            Tok::Open { name, attrs, empty } => {
                let parent = stack.last().map(|s| s.1.clone()).unwrap_or_else(|| root.clone());
                let el = El { name: name.clone(), classes: xml::attr(&attrs, "class").map(|c| c.split_whitespace().map(str::to_string).collect()).unwrap_or_default(), id: xml::attr(&attrs, "id").map(str::to_string) };
                if let Some(id) = &el.id {
                    b.ch.anchors.entry(id.clone()).or_insert(b.count);
                }
                if name == "title" {
                    b.in_title = !empty;
                }
                // the cascade: the tag's own, the rules, the inline style
                let mut d = tag_defaults(&name);
                for css in b.css {
                    for (sel, decl) in &css.rules {
                        if rule_applies(sel, &els, &el) {
                            d.over(decl);
                        }
                    }
                }
                if let Some(st) = xml::attr(&attrs, "style") {
                    d.over(&declarations(st));
                }
                let mut c = parent.clone();
                let etype = xml::attr(&attrs, "type").unwrap_or("").to_string();
                let role = xml::attr(&attrs, "role").unwrap_or("").to_string();
                if matches!(name.as_str(), "head" | "script" | "style" | "template") || d.none == Some(true) && !matches!(name.as_str(), "html" | "body") {
                    c.skip = true;
                }
                let blockish = is_block(&name) || d.block == Some(true);
                let footnote = name == "aside" && (etype.contains("footnote") || etype.contains("endnote") || etype.contains("rearnote") || etype == "note" || role.contains("doc-footnote") || role.contains("doc-endnote"));
                if footnote && c.note.is_none() {
                    c.note = Some(el.id.clone().unwrap_or_default());
                    b.note_text.clear();
                }
                if blockish && !c.skip && c.note.is_none() {
                    b.flush();
                    if d.break_before == Some(true) {
                        b.pending_break = true;
                    }
                    b.pending_before = b.pending_before.max(d.mt.unwrap_or(0.0));
                    c.left += d.ml.unwrap_or(0.0).clamp(-2.0, 6.0);
                    c.right += d.mr.unwrap_or(0.0).clamp(0.0, 6.0);
                    c.indent = d.indent.or(if is_block(&name) && name != "p" && name != "div" { None } else { parent.indent });
                    c.align = d.align.or(parent.align);
                }
                if let Some(v) = d.italic {
                    c.st.italic = v;
                }
                if let Some(v) = d.bold {
                    c.st.bold = v;
                }
                if let Some(v) = d.smallcaps {
                    c.st.smallcaps = v;
                }
                if let Some(v) = d.upper {
                    c.st.upper = v;
                }
                if let Some(v) = d.mono {
                    c.st.mono = v;
                }
                if let Some(v) = d.strike {
                    c.st.strike = v;
                }
                if let Some(v) = d.sup {
                    c.st.sup = v;
                }
                if let Some(s) = d.scale {
                    if !matches!(name.as_str(), "body" | "html") {
                        c.st.scale = (c.st.scale * s).clamp(0.65, 2.4);
                    }
                }
                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        c.heading = name[1..].parse().ok();
                        c.st.bold = false;
                        c.st.scale = 1.0;
                    }
                    "pre" => c.pre = true,
                    "figcaption" | "caption" => c.caption = true,
                    "ul" | "ol" => {
                        let start = xml::attr(&attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1);
                        b.lists.push((name == "ol", start));
                        c.left += 0.4;
                    }
                    "li" => {
                        let mark = match b.lists.last_mut() {
                            Some((true, n)) => {
                                let m = format!("{n}.");
                                *n += 1;
                                m
                            }
                            _ => "•".to_string(),
                        };
                        b.bullet = Some(mark);
                    }
                    "br" if !c.skip => {
                        if c.note.is_some() {
                            b.note_text.push('\n');
                        } else if let Some(cur) = b.cur.as_mut() {
                            if let Some(l) = cur.spans.last_mut() {
                                let t = l.text.trim_end_matches(' ').to_string();
                                l.text = t + "\n";
                            }
                        }
                    }
                    "hr" if !c.skip && c.note.is_none() => {
                        b.flush();
                        b.ch.blocks.push(Block::Rule { off: b.count });
                        b.flush_next = true;
                    }
                    "img" | "image" if !c.skip && c.note.is_none() => {
                        let src = xml::attr(&attrs, "src").or_else(|| xml::attr(&attrs, "href")).unwrap_or("");
                        if !src.is_empty() {
                            let had_text = b.cur.as_ref().is_some_and(|t| t.spans.iter().any(|s| !s.text.trim().is_empty()));
                            let tiny = xml::attr(&attrs, "height").and_then(|h| h.trim_end_matches("px").parse::<f32>().ok()).is_some_and(|h| h < 24.0);
                            if !tiny {
                                b.flush();
                                let before = std::mem::take(&mut b.pending_before);
                                let brk = std::mem::take(&mut b.pending_break);
                                b.ch.blocks.push(Block::Image { src: xml::join(b.path, src), alt: xml::attr(&attrs, "alt").unwrap_or("").to_string(), off: b.count, break_before: brk, before, after: d.mb.unwrap_or(0.0) });
                                if had_text {
                                    b.start(&c);
                                    if let Some(cur) = b.cur.as_mut() {
                                        cur.cont = true;
                                    }
                                }
                            }
                        }
                    }
                    "td" | "th" if !c.skip => {
                        if let Some(cur) = b.cur.as_mut() {
                            if cur.spans.iter().any(|s| !s.text.trim().is_empty()) {
                                if let Some(l) = cur.spans.last_mut() {
                                    l.text.push_str("  ·  ");
                                }
                            }
                        }
                    }
                    "a" => {
                        if let Some(href) = xml::attr(&attrs, "href").filter(|h| !h.is_empty()) {
                            let internal = !href.contains("://") && !href.starts_with("mailto:");
                            let target = if internal { if let Some(f) = href.strip_prefix('#') { format!("{}#{f}", b.path) } else { let (p, f) = href.split_once('#').unwrap_or((href, "")); let j = xml::join(b.path, p); if f.is_empty() { j } else { format!("{j}#{f}") } } } else { href.to_string() };
                            let note = internal && (etype.contains("noteref") || role.contains("doc-noteref"));
                            b.ch.links.push(LinkSpec { href: target, note });
                            c.link = Some(b.ch.links.len() - 1);
                            if !empty {
                                b.open_link = Some((b.ch.links.len() - 1, b.count));
                            }
                        }
                    }
                    _ => {}
                }
                if empty {
                    if let Some(id) = el.id.as_ref().filter(|_| footnote) {
                        b.ch.notes.insert(id.clone(), String::new());
                    }
                    continue;
                }
                els.push(el.clone());
                stack.push((el, c, blockish, d));
            }
            Tok::Close(name) => {
                let Some(pos) = stack.iter().rposition(|(e, ..)| e.name == name) else { continue };
                while stack.len() > pos {
                    let (el, c, blockish, d) = stack.pop().unwrap();
                    els.pop();
                    if el.name == "title" {
                        b.in_title = false;
                    }
                    let parent_note = stack.last().and_then(|s| s.1.note.clone());
                    if c.note.is_some() && parent_note.is_none() {
                        let text = b.note_text.split_whitespace().collect::<Vec<_>>().join(" ");
                        if let Some(id) = c.note.clone().filter(|i| !i.is_empty()) {
                            b.ch.notes.insert(id, text);
                        }
                        b.note_text.clear();
                        continue;
                    }
                    if c.note.is_some() {
                        if blockish {
                            b.note_text.push('\n');
                        }
                        continue;
                    }
                    if el.name == "a" {
                        if let Some((li, start)) = b.open_link.take() {
                            if c.link == Some(li) {
                                b.ch.link_ranges.push((start, b.count, li));
                                // a short number linking into the book: a note reference
                                let text: String = b.cur.as_ref().map(|t| t.spans.iter().filter(|s| s.link == Some(li)).map(|s| s.text.as_str()).collect()).unwrap_or_default();
                                let t = text.trim();
                                if !b.ch.links[li].note && !b.ch.links[li].href.contains("://") && !t.is_empty() && t.chars().count() <= 4 && t.chars().all(|ch| ch.is_ascii_digit() || "*†‡§¹²³⁴⁵⁶⁷⁸⁹⁰[]()".contains(ch)) {
                                    b.ch.links[li].note = true;
                                }
                            }
                        }
                    }
                    if matches!(el.name.as_str(), "ul" | "ol") {
                        b.lists.pop();
                    }
                    if blockish && !c.skip {
                        b.flush();
                        b.pending_before = b.pending_before.max(d.mb.unwrap_or(0.0));
                    }
                }
            }
            Tok::Text(t) => {
                if b.in_title {
                    continue;
                }
                let c = stack.last().map(|s| s.1.clone()).unwrap_or_else(|| root.clone());
                b.text(&t, &c);
            }
        }
    }
    b.flush();
    b.ch.len = b.count;
    b.ch
}

// ── the book ───────────────────────────────────────────────────────────

fn nav_entries(n: &xml::Node, level: usize, base: &str, out: &mut Vec<(String, usize, String)>) {
    for li in n.kids.iter().filter(|k| k.name == "li") {
        let a = li.kids.iter().find(|k| k.name == "a" || k.name == "span");
        if let Some(a) = a {
            let title = a.all_text();
            let href = a.attr("href").map(|h| {
                let (p, f) = h.split_once('#').unwrap_or((h, ""));
                let j = if p.is_empty() { base.to_string() } else { xml::join(base, p) };
                if f.is_empty() { j } else { format!("{j}#{f}") }
            });
            if let Some(href) = href {
                out.push((title, level, href));
            }
        }
        if let Some(ol) = li.kids.iter().find(|k| k.name == "ol" || k.name == "ul") {
            nav_entries(ol, level + 1, base, out);
        }
    }
}

fn ncx_entries(n: &xml::Node, level: usize, base: &str, out: &mut Vec<(String, usize, String)>) {
    for np in n.kids.iter().filter(|k| k.name == "navpoint") {
        let title = np.child("navlabel").map(|l| l.all_text()).unwrap_or_default();
        if let Some(src) = np.child("content").and_then(|c| c.attr("src")) {
            let (p, f) = src.split_once('#').unwrap_or((src, ""));
            let j = xml::join(base, p);
            out.push((title, level, if f.is_empty() { j } else { format!("{j}#{f}") }));
        }
        ncx_entries(np, level + 1, base, out);
    }
}

impl Epub {
    pub fn open(path: &Path, fonts: Arc<FontDb>) -> Result<Epub, String> {
        let zip = Zip::open(path)?;
        if let Ok(enc) = zip.read_string("META-INF/encryption.xml") {
            // font obfuscation is not DRM; anything else is
            let t = xml::tree(&enc);
            let drm = t.find_all("encryptionmethod").iter().any(|m| {
                let a = m.attr("algorithm").unwrap_or("");
                !a.contains("idpf.org/2008/embedding") && !a.contains("ns.adobe.com/pdf/enc#RC")
            });
            if drm {
                return Err("o livro é protegido por DRM e não pode ser lido".into());
            }
        }
        let container = zip.read_string("META-INF/container.xml").map_err(|_| "EPUB sem META-INF/container.xml".to_string())?;
        let ct = xml::tree(&container);
        let opf_path = ct.first("rootfile").and_then(|r| r.attr("full-path")).ok_or("EPUB sem pacote (rootfile)")?.to_string();
        let opf = xml::tree(&zip.read_string(&opf_path)?);
        let md = opf.first("metadata");
        let text_of = |name: &str| md.and_then(|m| m.first(name)).map(|n| n.all_text()).unwrap_or_default();
        let mut meta = Meta { title: text_of("title"), author: md.map(|m| m.find_all("creator").iter().map(|n| n.all_text()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")).unwrap_or_default(), lang: text_of("language"), publisher: text_of("publisher"), ..Default::default() };
        if let Some(m) = md {
            meta.isbn = m.find_all("identifier").iter().map(|n| n.all_text()).find(|s| s.to_lowercase().contains("isbn") || s.chars().filter(|c| c.is_ascii_digit()).count() == 13).map(|s| s.trim_start_matches("urn:isbn:").to_string()).unwrap_or_default();
        }
        if meta.title.is_empty() {
            meta.title = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        }
        // the manifest: id → (path, media type, properties)
        let mut manifest: HashMap<String, (String, String, String)> = HashMap::new();
        for it in opf.find_all("item") {
            if let (Some(id), Some(href)) = (it.attr("id"), it.attr("href")) {
                manifest.insert(id.to_string(), (xml::join(&opf_path, href), it.attr("media-type").unwrap_or("").to_string(), it.attr("properties").unwrap_or("").to_string()));
            }
        }
        let spine_node = opf.first("spine");
        meta.rtl = spine_node.and_then(|s| s.attr("page-progression-direction")) == Some("rtl");
        let mut spine: Vec<String> = Vec::new();
        for ir in opf.find_all("itemref") {
            if let Some((p, mt, _)) = ir.attr("idref").and_then(|i| manifest.get(i)) {
                if mt.contains("html") || mt.contains("xml") || mt.is_empty() || p.ends_with("html") || p.ends_with(".htm") {
                    spine.push(p.clone());
                }
            }
        }
        if spine.is_empty() {
            return Err("EPUB sem capítulos na espinha".into());
        }
        let cover = manifest.values().find(|(_, _, pr)| pr.split_whitespace().any(|x| x == "cover-image")).map(|x| x.0.clone()).or_else(|| {
            let id = md?.find_all("meta").into_iter().find(|m| m.attr("name") == Some("cover"))?.attr("content")?.to_string();
            manifest.get(&id).map(|x| x.0.clone())
        });
        let mut ep = Epub { zip, fonts, meta, toc: Vec::new(), spine, cover, fixed: None, css: HashMap::new(), chapters: HashMap::new(), laid: Vec::new(), faces: None };
        // the contents
        let mut raw: Vec<(String, usize, String)> = Vec::new();
        if let Some((nav_path, _, _)) = manifest.values().find(|(_, _, pr)| pr.split_whitespace().any(|x| x == "nav")) {
            if let Ok(src) = ep.zip.read_string(nav_path) {
                let t = xml::tree(&src);
                let navs = t.find_all("nav");
                let toc_nav = navs.iter().find(|n| n.attr("type").is_some_and(|t| t.contains("toc"))).or(navs.first());
                if let Some(ol) = toc_nav.and_then(|n| n.first("ol")) {
                    nav_entries(ol, 0, nav_path, &mut raw);
                }
            }
        }
        if raw.is_empty() {
            let ncx = spine_node.and_then(|s| s.attr("toc")).and_then(|id| manifest.get(id)).map(|x| x.0.clone()).or_else(|| manifest.values().find(|(_, mt, _)| mt.contains("ncx")).map(|x| x.0.clone()));
            if let Some(ncx) = ncx {
                if let Ok(src) = ep.zip.read_string(&ncx) {
                    let t = xml::tree(&src);
                    if let Some(nm) = t.first("navmap") {
                        ncx_entries(nm, 0, &ncx, &mut raw);
                    }
                }
            }
        }
        for (title, level, href) in raw {
            let (p, frag) = href.split_once('#').map(|(a, b)| (a.to_string(), Some(b.to_string()))).unwrap_or((href.clone(), None));
            let Some(chap) = ep.spine.iter().position(|s| *s == p) else { continue };
            let offset = frag.and_then(|f| ep.chapter(chap).anchors.get(&f).copied());
            ep.toc.push(TocEntry { title, level, chap, page: None, offset });
        }
        // a fixed-layout book made of pictures
        let pre_paginated = md.is_some_and(|m| m.find_all("meta").iter().any(|x| x.attr("property") == Some("rendition:layout") && x.all_text() == "pre-paginated"));
        if pre_paginated {
            let mut pics = Vec::new();
            for i in 0..ep.spine.len() {
                let c = ep.chapter(i);
                let imgs: Vec<&String> = c.blocks.iter().filter_map(|b| if let Block::Image { src, .. } = b { Some(src) } else { None }).collect();
                if imgs.len() == 1 && c.len < 40 {
                    pics.push(imgs[0].clone());
                } else {
                    pics.clear();
                    break;
                }
            }
            if !pics.is_empty() {
                ep.fixed = Some(pics);
            }
        }
        Ok(ep)
    }

    /// The stylesheets a chapter links to, and its own `<style>`.
    fn sheets(&mut self, src: &str, path: &str) -> Vec<Arc<Css>> {
        let mut out = Vec::new();
        for t in xml::tokens(src.split("</head>").next().unwrap_or("")) {
            if let Tok::Open { name, attrs, .. } = t {
                if name == "link" && xml::attr(&attrs, "rel").is_some_and(|r| r.contains("stylesheet")) {
                    if let Some(h) = xml::attr(&attrs, "href") {
                        let p = xml::join(path, h);
                        if !self.css.contains_key(&p) {
                            let css = self.zip.read_string(&p).map(|s| Css::parse(&s)).unwrap_or_default();
                            self.css.insert(p.clone(), Arc::new(css));
                        }
                        out.push(self.css[&p].clone());
                    }
                }
            }
        }
        let mut rest = src;
        while let Some(i) = rest.find("<style") {
            let Some(s) = rest[i..].find('>') else { break };
            let body = &rest[i + s + 1..];
            let e = body.find("</style").unwrap_or(body.len());
            out.push(Arc::new(Css::parse(&body[..e])));
            rest = &body[e..];
        }
        out
    }

    pub fn chapter(&mut self, i: usize) -> Arc<Chapter> {
        if let Some(c) = self.chapters.get(&i) {
            return c.clone();
        }
        let path = self.spine.get(i).cloned().unwrap_or_default();
        let src = self.zip.read_string(&path).unwrap_or_default();
        let css = self.sheets(&src, &path);
        let c = Arc::new(chapter(&src, &path, &css));
        self.chapters.insert(i, c.clone());
        c
    }

    fn picture(&self, name: &str) -> Option<Picture> {
        let b = self.zip.read(name).ok()?;
        let img = image::load_from_memory(&b).ok()?;
        let img = if img.width() > 1600 { img.resize(1600, 1600 * img.height() / img.width().max(1), image::imageops::FilterType::Triangle) } else { img };
        let rgba = img.to_rgba8();
        Some(Picture { w: rgba.width(), h: rgba.height(), rgba: rgba.into_raw(), jpeg: None })
    }

    fn faces(&mut self, view: &View) -> crate::typeset::lines::Faces {
        if let Some((k, f)) = &self.faces {
            if *k == view.font {
                return f.clone();
            }
        }
        let f = crate::typeset::layout::faces(&self.fonts, &serde_json::json!({ "fonte": view.font }));
        self.faces = Some((view.font.clone(), f.clone()));
        f
    }

    pub fn laid(&mut self, chap: usize, view: &View) -> Arc<Laid> {
        let key = view.key();
        if let Some(i) = self.laid.iter().position(|(c, k, _)| *c == chap && *k == key) {
            let hit = self.laid.remove(i);
            let l = hit.2.clone();
            self.laid.push(hit);
            return l;
        }
        let ch = self.chapter(chap);
        let faces = self.faces(view);
        let fonts = self.fonts.clone();
        let laid = Arc::new(flow::layout(&ch, view, &fonts, faces, &|name| self.picture(name)));
        self.laid.push((chap, key, laid.clone()));
        if self.laid.len() > 8 {
            self.laid.remove(0);
        }
        laid
    }

    fn target(&mut self, href: &str) -> Option<(usize, Option<usize>)> {
        let (p, f) = href.split_once('#').unwrap_or((href, ""));
        let chap = self.spine.iter().position(|s| s == p)?;
        let off = if f.is_empty() { None } else { self.chapter(chap).anchors.get(f).copied() };
        Some((chap, off))
    }

    pub fn spine_len(&self) -> usize {
        self.spine.len()
    }
}

impl Document for Epub {
    fn format(&self) -> Format {
        Format::Epub
    }

    fn meta(&self) -> &Meta {
        &self.meta
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn chapters(&self) -> usize {
        if self.fixed.is_some() { 1 } else { self.spine.len() }
    }

    fn reflows(&self) -> bool {
        self.fixed.is_none()
    }

    fn pages(&mut self, chap: usize, view: &View) -> usize {
        if let Some(f) = &self.fixed {
            return f.len();
        }
        self.laid(chap, view).pages.len()
    }

    fn size(&mut self, _chap: usize, page: usize, view: &View) -> (f32, f32) {
        if let Some(f) = &self.fixed {
            let name = f.get(page).cloned().unwrap_or_default();
            if let Ok(b) = self.zip.read(&name) {
                if let Ok((w, h)) = image::ImageReader::new(std::io::Cursor::new(&b)).with_guessed_format().map_err(|e| e.to_string()).and_then(|r| r.into_dimensions().map_err(|e| e.to_string())) {
                    return (w as f32, h as f32);
                }
            }
            return (2.0, 3.0);
        }
        (view.w, view.h)
    }

    fn render(&mut self, chap: usize, page: usize, view: &View, px_w: u32) -> Result<Rgba, String> {
        if let Some(f) = &self.fixed {
            let name = f.get(page).ok_or("página fora do livro")?.clone();
            let b = self.zip.read(&name)?;
            let img = image::load_from_memory(&b).map_err(|e| e.to_string())?.to_rgba8();
            let (w, h) = img.dimensions();
            if px_w >= w {
                return Ok(Rgba { w, h, px: img.into_raw() });
            }
            let nh = ((h as f64) * px_w as f64 / w as f64).round().max(1.0) as u32;
            let small = image::imageops::resize(&img, px_w, nh, image::imageops::FilterType::Triangle);
            return Ok(Rgba { w: px_w, h: nh, px: small.into_raw() });
        }
        let laid = self.laid(chap, view);
        let pg = laid.page(page.min(laid.pages.len().saturating_sub(1)));
        let ppm = px_w as f32 / view.w.max(1.0);
        let (w, h, px) = crate::typeset::raster::render(&pg, &self.fonts, &laid.pics, ppm);
        Ok(Rgba { w, h, px })
    }

    fn text(&mut self, chap: usize, page: usize, view: &View) -> Option<TextLayer> {
        if self.fixed.is_some() {
            return None;
        }
        let laid = self.laid(chap, view);
        let g = laid.pages.get(page)?.glyphs.clone();
        (!g.is_empty()).then_some(TextLayer { glyphs: g })
    }

    fn links(&mut self, chap: usize, page: usize, view: &View) -> Vec<Link> {
        if self.fixed.is_some() {
            return Vec::new();
        }
        let laid = self.laid(chap, view);
        let ch = self.chapter(chap);
        let Some(p) = laid.pages.get(page) else { return Vec::new() };
        let mut out = Vec::new();
        for (r, li) in &p.links {
            let Some(spec) = ch.links.get(*li) else { continue };
            let target = if spec.href.contains("://") || spec.href.starts_with("mailto:") {
                Target::Url(spec.href.clone())
            } else {
                match self.target(&spec.href) {
                    Some((c, off)) => Target::At { chap: c, offset: off, page: None },
                    None => continue,
                }
            };
            out.push(Link { r: *r, target, note: spec.note });
        }
        out
    }

    fn images(&mut self, chap: usize, page: usize, view: &View) -> Vec<([f32; 4], String)> {
        if let Some(f) = &self.fixed {
            return f.get(page).map(|n| vec![([0.0, 0.0, 1.0, 1.0], n.clone())]).unwrap_or_default();
        }
        self.laid(chap, view).pages.get(page).map(|p| p.images.clone()).unwrap_or_default()
    }

    fn resource(&self, name: &str) -> Option<Vec<u8>> {
        self.zip.read(name).ok()
    }

    fn note_text(&mut self, chap: usize, offset: usize) -> Option<String> {
        let ch = self.chapter(chap);
        // an aside kept out of the text, by its anchor
        if let Some((id, _)) = ch.anchors.iter().find(|(id, o)| **o == offset && ch.notes.contains_key(*id)) {
            return ch.notes.get(id).cloned().filter(|t| !t.is_empty());
        }
        ch.block_text_at(offset)
    }

    fn locate(&mut self, loc: &Locator, view: &View) -> (usize, usize) {
        if self.fixed.is_some() {
            return (0, loc.page.unwrap_or(1).saturating_sub(1));
        }
        let n = self.spine.len();
        let mut chap = loc.chap.unwrap_or(0).min(n.saturating_sub(1));
        let mut off = loc.offset;
        if let Some(q) = loc.quote.as_ref().filter(|q| q.exact.chars().count() >= 8) {
            // the quote where the place says, else anywhere in the book
            let near = |ep: &mut Epub, c: usize| -> Option<usize> {
                let hits = ep.chapter(c).find(&q.exact);
                let want = off.unwrap_or(0);
                hits.into_iter().min_by_key(|h| h.abs_diff(want))
            };
            match near(self, chap) {
                Some(h) => off = Some(h),
                None => {
                    if let Some((c, h)) = (0..n).find_map(|c| near(self, c).map(|h| (c, h))) {
                        chap = c;
                        off = Some(h);
                    }
                }
            }
        }
        let laid = self.laid(chap, view);
        (chap, off.map(|o| laid.page_of(o)).unwrap_or(0))
    }

    fn locator(&mut self, chap: usize, page: usize, view: &View) -> Locator {
        if self.fixed.is_some() {
            return Locator { page: Some(page + 1), ..Default::default() };
        }
        let laid = self.laid(chap, view);
        let start = laid.pages.get(page).map(|p| p.start).unwrap_or(0);
        let quote = laid.pages.get(page).map(|p| {
            let t = TextLayer { glyphs: p.glyphs.iter().take(80).cloned().collect() };
            let s = t.all_text();
            Quote { exact: s.split_whitespace().take(8).collect::<Vec<_>>().join(" "), ..Default::default() }
        }).filter(|q| !q.exact.is_empty());
        Locator { chap: Some(chap), offset: Some(start), quote, ..Default::default() }
    }

    fn locator_at(&mut self, chap: usize, page: usize, view: &View, glyph: &Glyph) -> Locator {
        let mut l = self.locator(chap, page, view);
        if self.fixed.is_none() {
            l.offset = Some(glyph.off);
        }
        l
    }

    fn search(&mut self, query: &str, _view: &View) -> Vec<Hit> {
        let mut out = Vec::new();
        if self.fixed.is_some() {
            return out;
        }
        let qlen = query.chars().count();
        for c in 0..self.spine.len() {
            let ch = self.chapter(c);
            for off in ch.find(query) {
                out.push(Hit { chap: c, page: None, offset: Some(off), context: ch.context(off, qlen) });
                if out.len() >= 500 {
                    return out;
                }
            }
        }
        out
    }

    fn cover(&mut self, px_w: u32) -> Option<Rgba> {
        let name = self.cover.clone().or_else(|| self.fixed.as_ref().and_then(|f| f.first().cloned())).or_else(|| {
            let c = self.chapter(0);
            c.blocks.iter().find_map(|b| if let Block::Image { src, .. } = b { Some(src.clone()) } else { None })
        })?;
        let b = self.zip.read(&name).ok()?;
        let img = image::load_from_memory(&b).ok()?;
        let img = img.resize(px_w, px_w * 4, image::imageops::FilterType::Triangle).to_rgba8();
        Some(Rgba { w: img.width(), h: img.height(), px: img.into_raw() })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small EPUB 3 in a temporary folder.
    pub fn sample(dir: &Path) -> std::path::PathBuf {
        let container = r#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Sapiens</dc:title><dc:creator>Yuval Noah Harari</dc:creator><dc:language>pt-BR</dc:language></metadata>
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="text/c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/c2.xhtml" media-type="application/xhtml+xml"/><item id="css" href="s.css" media-type="text/css"/></manifest>
<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#;
        let nav = r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="text/c1.xhtml">Um animal insignificante</a></li><li><a href="text/c2.xhtml#trigo">A revolução agrícola</a></li></ol></nav></body></html>"#;
        let c1 = r##"<html><head><link rel="stylesheet" href="../s.css"/></head><body><h1>Um animal insignificante</h1><p>Há cerca de 13,5 bilhões de anos, matéria, energia, tempo e espaço surgiram no que é conhecido como o Big Bang.<a epub:type="noteref" href="#n1">1</a></p><p class="sc">A história dessas características fundamentais&nbsp;do universo é chamada física.</p><aside epub:type="footnote" id="n1"><p>Uma nota de rodapé.</p></aside></body></html>"##;
        let c2 = r#"<html><body><h2>A revolução agrícola</h2><p id="trigo">O trigo domesticou o homem, e não o contrário.</p><p><img src="../img/a.png" alt="figura"/></p></body></html>"#;
        let css = ".sc { font-variant: small-caps; text-indent: 0; margin-top: 1em }";
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(40, 30, image::Rgba([10, 120, 10, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        let z = crate::board::zip::write_with(&[
            ("mimetype".into(), b"application/epub+zip", false),
            ("META-INF/container.xml".into(), container.as_bytes(), true),
            ("OEBPS/content.opf".into(), opf.as_bytes(), true),
            ("OEBPS/nav.xhtml".into(), nav.as_bytes(), true),
            ("OEBPS/text/c1.xhtml".into(), c1.as_bytes(), true),
            ("OEBPS/text/c2.xhtml".into(), c2.as_bytes(), true),
            ("OEBPS/s.css".into(), css.as_bytes(), true),
            ("OEBPS/img/a.png".into(), &png[..], false),
        ]);
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("Sapiens.epub");
        std::fs::write(&p, z).unwrap();
        p
    }

    #[test]
    fn reads_a_book() {
        let dir = std::env::temp_dir().join(format!("ateliet-epub-{}", crate::journal::short_id()));
        let p = sample(&dir);
        let db = Arc::new(FontDb::new(&crate::typeset::testfonts::BUNDLED));
        let mut e = Epub::open(&p, db).unwrap();
        assert_eq!(e.meta().title, "Sapiens");
        assert_eq!(e.meta().author, "Yuval Noah Harari");
        assert_eq!(e.chapters(), 2);
        assert_eq!(e.toc().len(), 2);
        assert_eq!(e.toc()[1].chap, 1);
        let trigo = e.chapter(1).anchors["trigo"];
        assert_eq!(e.toc()[1].offset, Some(trigo));
        let c1 = e.chapter(0);
        assert!(c1.notes.get("n1").is_some_and(|n| n == "Uma nota de rodapé."), "{:?}", c1.notes);
        assert!(c1.links[0].note);
        let Block::Text(sc) = &c1.blocks[2] else { panic!() };
        assert!(sc.spans[0].st.smallcaps);
        assert_eq!(sc.indent, Some(0.0));
        assert!(!c1.texts().iter().any(|(_, t)| t.contains("Uma nota")));
        let view = View { w: 110.0, h: 160.0, ..View::default() };
        assert!(e.pages(0, &view) >= 1);
        let t = e.text(0, 0, &view).unwrap();
        assert!(t.all_text().contains("Big Bang"), "{}", t.all_text());
        let links = e.links(0, 0, &view);
        assert!(links.iter().any(|l| l.note));
        let img = e.render(1, 0, &view, 300).unwrap();
        assert_eq!(img.w, 300);
        assert_eq!(e.images(1, 0, &view).len(), 1);
        let hits = e.search("domesticou O HOMEM", &view);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chap, 1);
        let loc = Locator { chap: Some(0), quote: Some(Quote { exact: "O trigo domesticou o homem".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(e.locate(&loc, &view).0, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod look {
    /// READER_OUT=<dir> cargo test reader::epub::look -- --nocapture
    #[test]
    fn page_png() {
        let Ok(out) = std::env::var("READER_OUT") else { return };
        let dir = std::path::Path::new(&out);
        let p = super::tests::sample(dir);
        let db = std::sync::Arc::new(crate::typeset::fonts::FontDb::new(&crate::typeset::testfonts::BUNDLED));
        let mut e = super::Epub::open(&p, db).unwrap();
        let view = super::View { w: 110.0, h: 160.0, ..super::View::default() };
        use super::Document;
        for c in 0..2 {
            let r = e.render(c, 0, &view, 660).unwrap();
            image::RgbaImage::from_raw(r.w, r.h, r.px).unwrap().save(dir.join(format!("c{c}.png"))).unwrap();
        }
    }
}
