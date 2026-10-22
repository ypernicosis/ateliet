//! The book as typesetting sees it: chapters (opening, blocks), front and
//! back matter, pictures. Built from the same parsed Markdown as the other
//! exports; the text of a line goes through the same inline renderer and
//! its few tags become styled runs.

use super::{hex, Picture};
use crate::dataview::index::Index;
use crate::scriptorium::book::Book;
use crate::scriptorium::export::{self, Meta, Options};
use crate::scriptorium::manuscript::{md_blocks, md_inline, Block, Embed, Footnotes, InlineHost, Plain};
use crate::scriptorium::link_text;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub italic: bool,
    pub bold: bool,
    pub smallcaps: bool,
    /// All small caps (capitals too).
    pub allcaps_small: bool,
    pub upper: bool,
    pub sup: bool,
    pub scale: f32,
    /// Letter spacing in em.
    pub track: f32,
    pub color: Option<[u8; 3]>,
    /// 0–1 (1 = solid).
    pub opacity: f32,
    pub strike: bool,
    pub mono: bool,
    pub title_font: bool,
}

impl Style {
    pub fn plain() -> Style {
        Style { scale: 1.0, opacity: 1.0, ..Default::default() }
    }
}

#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub st: Style,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Justify,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
pub struct Para {
    pub runs: Vec<Run>,
    /// No first-line indent (after a heading, a break, the opening).
    pub first: bool,
    pub align: Option<Align>,
    pub dropcap: bool,
    /// Small caps on the first line (or the first words).
    pub opener: bool,
}

#[derive(Clone, Debug)]
pub enum Blk {
    P(Para),
    H { level: usize, runs: Vec<Run> },
    Scene,
    Figure { pic: usize, frac: f32, caption: Vec<Run> },
    /// Quotes and callouts (`kind` empty for a quote).
    Boxed { kind: String, title: Vec<Run>, blocks: Vec<Blk> },
    List { ordered: bool, start: u64, items: Vec<Vec<Blk>> },
    Code(String),
    Notes(Vec<Vec<Run>>),
    PageBreak { recto: bool },
    Missing(String),
}

pub struct Opening {
    pub label: String,
    pub title: Vec<Run>,
    pub show_title: bool,
    pub art: Option<usize>,
    pub epigraph: Option<(Vec<Run>, String)>,
}

pub struct Chapter {
    pub id: String,
    pub path: String,
    pub title: String,
    pub label: String,
    pub part: String,
    pub head: Opening,
    pub blocks: Vec<Blk>,
}

pub struct BookModel {
    pub meta: Meta,
    pub cfg: Json,
    pub chapters: Vec<Chapter>,
    /// (title, blocks)
    pub back: Vec<(String, Vec<Blk>)>,
    pub pics: Vec<Picture>,
    pub dedication: Vec<Run>,
    pub epigraph: Vec<Run>,
    pub credits: Vec<Run>,
}

// ── inline HTML → runs ─────────────────────────────────────────────────

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find(';').filter(|e| *e <= 10) {
            Some(e) => {
                let ent = &tail[1..e];
                let c = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
                    _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                match c {
                    Some(c) => {
                        out.push(c);
                        rest = &tail[e + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &tail[1..];
                    }
                }
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let k = format!("{name}=\"");
    let i = tag.find(&k)? + k.len();
    let e = tag[i..].find('"')?;
    Some(&tag[i..i + e])
}

/// A text style class (`sc-s-…`) or inline CSS onto a style.
fn apply_class(st: &mut Style, cls: &str) {
    match cls {
        "sc-s-grito" => {
            st.bold = true;
            st.upper = true;
            st.track += 0.04;
            st.scale *= 1.22;
        }
        "sc-s-sussurro" => {
            st.scale *= 0.84;
            st.italic = true;
            st.opacity *= 0.66;
            st.track += 0.02;
        }
        "sc-s-manuscrito" => {
            st.italic = true;
            st.scale *= 1.1;
        }
        "sc-s-runa" | "sc-s-espacado" => st.track += if cls == "sc-s-runa" { 0.04 } else { 0.32 },
        "sc-s-maquina" | "sc-s-tela" => {
            st.mono = true;
            st.scale *= 0.9;
        }
        "sc-s-sangue" => {
            st.color = Some([0x8a, 0x0b, 0x0b]);
            st.bold = true;
        }
        "sc-s-ouro" => st.color = Some([0xa8, 0x81, 0x2f]),
        "sc-s-versalete" => {
            st.smallcaps = true;
            st.track += 0.05;
        }
        "sc-s-gigante" => st.scale *= 2.2,
        "sc-s-minusculo" => st.scale *= 0.66,
        "sc-s-apagado" => st.opacity *= 0.3,
        "sc-s-borrado" => st.opacity *= 0.85,
        "sc-s-riscado" => st.strike = true,
        "sc-lead" => {
            st.smallcaps = true;
            st.track += 0.03;
        }
        _ => {}
    }
}

fn apply_css(st: &mut Style, css: &str) {
    for decl in css.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let v = v.trim();
        match k.trim() {
            "color" => st.color = hex(v).or(st.color),
            "font-size" => {
                if let Some(n) = v.strip_suffix("em").and_then(|n| n.parse::<f32>().ok()) {
                    st.scale *= n;
                }
            }
            "font-weight" => st.bold = v.parse::<u32>().map(|w| w >= 600).unwrap_or(v == "bold"),
            "letter-spacing" => {
                if let Some(n) = v.strip_suffix("em").and_then(|n| n.parse::<f32>().ok()) {
                    st.track += n;
                }
            }
            "opacity" => st.opacity *= v.parse::<f32>().unwrap_or(1.0),
            _ => {}
        }
    }
}

/// The inline HTML of our renderer as styled runs ("\n" for line breaks).
pub fn runs(html: &str, base: &Style) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    let mut stack: Vec<(String, Style)> = vec![(String::new(), base.clone())];
    let mut rest = html;
    let push = |out: &mut Vec<Run>, text: String, st: &Style| {
        if text.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(r) if r.st == *st => r.text.push_str(&text),
            _ => out.push(Run { text, st: st.clone() }),
        }
    };
    while !rest.is_empty() {
        match rest.find('<') {
            Some(0) => {
                let Some(e) = rest.find('>') else {
                    push(&mut out, unescape(rest), &stack.last().unwrap().1);
                    break;
                };
                let tag = &rest[1..e];
                rest = &rest[e + 1..];
                if let Some(name) = tag.strip_prefix('/') {
                    let name = name.trim().to_lowercase();
                    if let Some(i) = stack.iter().rposition(|s| s.0 == name) {
                        if i > 0 {
                            stack.truncate(i);
                        }
                    }
                    continue;
                }
                let name: String = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_lowercase();
                if name == "br" {
                    push(&mut out, "\n".into(), &stack.last().unwrap().1);
                    continue;
                }
                if name == "img" || name == "wbr" {
                    continue;
                }
                let mut st = stack.last().unwrap().1.clone();
                match name.as_str() {
                    "em" | "i" | "cite" | "var" => st.italic = !st.italic,
                    "strong" | "b" => st.bold = true,
                    "sup" => st.sup = true,
                    "small" => st.scale *= 0.85,
                    "big" => st.scale *= 1.2,
                    "s" | "del" => st.strike = true,
                    "code" | "kbd" => st.mono = true,
                    "span" => {
                        for c in attr(tag, "class").unwrap_or("").split_whitespace() {
                            apply_class(&mut st, c);
                        }
                        apply_css(&mut st, attr(tag, "style").unwrap_or(""));
                    }
                    _ => {}
                }
                if !tag.ends_with('/') {
                    stack.push((name, st));
                }
            }
            Some(i) => {
                push(&mut out, unescape(&rest[..i]), &stack.last().unwrap().1);
                rest = &rest[i..];
            }
            None => {
                push(&mut out, unescape(rest), &stack.last().unwrap().1);
                break;
            }
        }
    }
    out
}

// ── blocks ─────────────────────────────────────────────────────────────

struct Host<'a> {
    ix: &'a Index,
    root: &'a Path,
    source: String,
    notes: Footnotes,
    typography: bool,
    pics: &'a mut Pics,
}

impl InlineHost for Host<'_> {
    fn typography(&self) -> bool {
        self.typography
    }
    fn footnote_ref(&mut self, id: &str) -> String {
        self.notes.reference(id)
    }
    fn footnote_inline(&mut self, text: &str) -> String {
        self.notes.inline_note(text)
    }
}

/// Pictures read once, scaled down for the page.
#[derive(Default)]
pub struct Pics {
    pub list: Vec<Picture>,
    by_path: HashMap<String, Option<usize>>,
}

impl Pics {
    pub fn load(&mut self, root: &Path, path: &str) -> Option<usize> {
        if let Some(i) = self.by_path.get(path) {
            return *i;
        }
        let raw = std::fs::read(root.join(path)).ok();
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        let pic = raw.and_then(|raw| {
            let img = if ext == "clip" {
                crate::clipstudio::read(&raw).ok().and_then(|d| d.preview).and_then(|p| image::load_from_memory(&p).ok())?
            } else {
                image::load_from_memory(&raw).ok()?
            };
            let rgb_jpeg = matches!(ext.as_str(), "jpg" | "jpeg") && img.color() == image::ColorType::Rgb8 && img.width() <= 3000 && img.height() <= 3000;
            let img = if img.width() > 3000 || img.height() > 3000 { img.resize(3000, 3000, image::imageops::FilterType::CatmullRom) } else { img };
            let rgba = img.to_rgba8();
            Some(Picture { w: rgba.width(), h: rgba.height(), rgba: rgba.into_raw(), jpeg: rgb_jpeg.then_some(raw) })
        });
        let i = pic.map(|p| {
            self.list.push(p);
            self.list.len() - 1
        });
        self.by_path.insert(path.to_string(), i);
        i
    }
}

fn frac_of(e: &Embed) -> f32 {
    let w = e.width.trim();
    if let Some(p) = w.strip_suffix('%').and_then(|p| p.parse::<f32>().ok()) {
        return (p / 100.0).clamp(0.1, 1.0);
    }
    if e.px > 0 {
        return (e.px as f32 / 650.0).clamp(0.1, 1.0);
    }
    if matches!(e.pos.as_str(), "esquerda" | "direita") {
        return 0.45;
    }
    1.0
}

fn convert(blocks: &[Block], h: &mut Host) -> Vec<Blk> {
    let mut out = Vec::new();
    let mut prev = "start";
    for b in blocks {
        let inl = |h: &mut Host, t: &str, st: &Style| runs(&md_inline(t, h).replace('\n', "<br/>"), st);
        let base = Style::plain();
        match b {
            Block::P { text, .. } => {
                let first = matches!(prev, "start" | "h" | "scene" | "fig" | "pagebreak");
                out.push(Blk::P(Para { runs: inl(h, text, &base), first, align: None, dropcap: false, opener: false }));
                prev = "p";
            }
            Block::H { level, text, .. } => {
                out.push(Blk::H { level: *level, runs: inl(h, text, &base) });
                prev = "h";
            }
            Block::Scene { .. } => {
                out.push(Blk::Scene);
                prev = "scene";
            }
            Block::Fig { e, .. } => {
                match export::resolve_image(h.ix, &e.link, &h.source).and_then(|p| h.pics.load(h.root, &p)) {
                    Some(pic) => {
                        let cap = if e.caption.is_empty() { Vec::new() } else { runs(&md_inline(&e.caption, &mut Plain { typography: h.typography }), &Style { italic: true, scale: 0.8, ..Style::plain() }) };
                        out.push(Blk::Figure { pic, frac: frac_of(e), caption: cap });
                    }
                    None => out.push(Blk::Missing(format!("[imagem não encontrada: {}]", e.link))),
                }
                prev = "fig";
            }
            Block::Callout { kind, title, blocks, .. } => {
                let t = if title.is_empty() { Vec::new() } else { inl(h, title, &base) };
                let inner = convert(blocks, h);
                out.push(Blk::Boxed { kind: kind.clone(), title: t, blocks: inner });
                prev = "callout";
            }
            Block::Quote { blocks, .. } => {
                let inner = convert(blocks, h);
                out.push(Blk::Boxed { kind: String::new(), title: Vec::new(), blocks: inner });
                prev = "quote";
            }
            Block::List { ordered, start, items, .. } => {
                let items = items.iter().map(|it| convert(it, h)).collect();
                out.push(Blk::List { ordered: *ordered, start: *start, items });
                prev = "list";
            }
            Block::Table { head, rows, .. } => {
                // tables are rare in fiction: one line per row, cells apart
                let mut lines = vec![head.join(" · ")];
                lines.extend(rows.iter().map(|r| r.join(" · ")));
                out.push(Blk::Code(lines.join("\n")));
                prev = "table";
            }
            Block::Code { text, .. } => {
                out.push(Blk::Code(text.clone()));
                prev = "code";
            }
            Block::Html { html, .. } => {
                let t = runs(html, &base);
                if t.iter().any(|r| !r.text.trim().is_empty()) {
                    out.push(Blk::P(Para { runs: t, first: false, align: None, dropcap: false, opener: false }));
                }
                prev = "p";
            }
            Block::Page { .. } => {
                prev = "pagebreak";
            }
            Block::PageBreak { recto, .. } => {
                out.push(Blk::PageBreak { recto: *recto });
                prev = "pagebreak";
            }
        }
    }
    out
}

fn notes_of(h: &Host) -> Option<Blk> {
    if h.notes.order.is_empty() {
        return None;
    }
    let list = h.notes.order.iter().map(|id| runs(&md_inline(h.notes.defs.get(id).map(String::as_str).unwrap_or(""), &mut Plain { typography: h.typography }), &Style::plain())).collect();
    Some(Blk::Notes(list))
}

/// Read the book into the typesetting model.
pub fn build(root: &Path, ix: &Index, b: &Book, opts: &Options) -> BookModel {
    let built = export::build(root, ix, b, opts);
    let cfg = built.cfg.clone();
    let mut pics = Pics::default();
    let mut chapters = Vec::new();
    for ch in &built.chapters {
        let parsed = md_blocks(&ch.md, &opts.soft_break);
        let fm = ix.page(&ch.path).map(|p| p.yaml.clone()).unwrap_or(Json::Null);
        let mut host = Host { ix, root, source: ch.path.clone(), notes: Footnotes::new(&ch.id, parsed.footnotes.clone()), typography: opts.typography, pics: &mut pics };
        let mut blocks = convert(&parsed.blocks, &mut host);
        if let Some(Blk::P(p)) = blocks.first_mut() {
            p.first = true;
            p.dropcap = cfg["capitular"].as_bool().unwrap_or(false);
            p.opener = cfg["versaleteInicio"].as_bool().unwrap_or(false);
        }
        if let Some(n) = notes_of(&host) {
            blocks.push(n);
        }
        let art = {
            let a = link_text(fm.get("abertura").unwrap_or(&Json::Null));
            if a.is_empty() {
                None
            } else {
                export::resolve_image(ix, &a, &ch.path).and_then(|p| pics.load(root, &p))
            }
        };
        let epi = fm["epigrafe"].as_str().filter(|s| !s.is_empty()).map(|e| (runs(&md_inline(e, &mut Plain { typography: opts.typography }), &Style::plain()), fm["epigrafe_autor"].as_str().unwrap_or("").to_string()));
        let head = Opening { label: ch.label.clone(), title: runs(&md_inline(&ch.title, &mut Plain { typography: opts.typography }), &Style::plain()), show_title: fm.get("mostrar_titulo") != Some(&Json::Bool(false)), art, epigraph: epi };
        chapters.push(Chapter { id: ch.id.clone(), path: ch.path.clone(), title: ch.title.clone(), label: ch.label.clone(), part: ch.part.clone(), head, blocks });
    }
    let mut back = Vec::new();
    for (title, text) in [("Agradecimentos", built.meta.thanks.clone()), ("Sobre o autor", built.meta.about.clone())] {
        if text.trim().is_empty() {
            continue;
        }
        let parsed = md_blocks(&text, &opts.soft_break);
        let mut host = Host { ix, root, source: b.path.clone(), notes: Footnotes::new("bm", parsed.footnotes.clone()), typography: opts.typography, pics: &mut pics };
        let mut blocks = convert(&parsed.blocks, &mut host);
        if let Some(n) = notes_of(&host) {
            blocks.push(n);
        }
        back.push((title.to_string(), blocks));
    }
    let inl = |t: &str| runs(&md_inline(t, &mut Plain { typography: opts.typography }).replace('\n', "<br/>"), &Style::plain());
    let meta = built.meta;
    BookModel { dedication: inl(&meta.dedication), epigraph: inl(&meta.epigraph), credits: inl(&meta.credits), meta, cfg, chapters, back, pics: pics.list }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_runs() {
        let r = runs("um <strong>forte</strong> e <em>leve</em>, <span class=\"sc-s-grito\" style=\"color:#c00\">grito</span>&amp;<br/>fim", &Style::plain());
        let t: Vec<&str> = r.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(t, vec!["um ", "forte", " e ", "leve", ", ", "grito", "&\nfim"]);
        assert!(r[1].st.bold && r[3].st.italic);
        assert!(r[5].st.upper && r[5].st.color == Some([0xcc, 0, 0]));
        // emphasis inside italics turns roman again
        let r = runs("<em>a <em>b</em> c</em>", &Style::plain());
        assert!(r[0].st.italic && !r[1].st.italic && r[2].st.italic);
        assert_eq!(runs("<sup class=\"sc-fnref\"><a href=\"#x\">1</a></sup>", &Style::plain())[0].st.sup, true);
    }
}
