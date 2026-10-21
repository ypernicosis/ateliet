//! DOCX export: the book's HTML (from `export::build`) read back into a small
//! tree and written as WordprocessingML, with styles named like the plugin's
//! (Heading1, FirstParagraph, SceneBreak, Callout…), footnotes and pictures.

use super::export::{self, Assets, Built};
use super::book::Book;
use crate::dataview::index::Index;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ── a tiny HTML tree ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Node {
    Text(String),
    El(El),
}

#[derive(Debug, Clone, Default)]
pub struct El {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl El {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    pub fn has_class(&self, c: &str) -> bool {
        self.attr("class").is_some_and(|cl| cl.split_whitespace().any(|x| x == c))
    }

    pub fn classes(&self) -> Vec<String> {
        self.attr("class").map(|c| c.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        for n in &self.children {
            match n {
                Node::Text(t) => s.push_str(t),
                Node::El(e) => s.push_str(&e.text()),
            }
        }
        s
    }

    fn els(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None })
    }

    /// First descendant matching.
    pub fn find(&self, f: &dyn Fn(&El) -> bool) -> Option<&El> {
        for e in self.els() {
            if f(e) {
                return Some(e);
            }
            if let Some(x) = e.find(f) {
                return Some(x);
            }
        }
        None
    }

    pub fn find_all<'a>(&'a self, f: &dyn Fn(&El) -> bool, out: &mut Vec<&'a El>) {
        for e in self.els() {
            if f(e) {
                out.push(e);
            }
            e.find_all(f, out);
        }
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(j) = tail.find(';').filter(|j| *j < 12) {
            let ent = &tail[1..j];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
                _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(c) = ch {
                out.push(c);
                rest = &tail[j + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

const VOID: [&str; 6] = ["br", "img", "hr", "wbr", "meta", "link"];

/// Parse HTML fragments (well formed enough: our own output and simple raw HTML).
pub fn parse_html(src: &str) -> El {
    let mut stack: Vec<El> = vec![El { tag: "#root".into(), ..Default::default() }];
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    let flush = |stack: &mut Vec<El>, t: &str| {
        if !t.is_empty() {
            stack.last_mut().unwrap().children.push(Node::Text(decode_entities(t)));
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(end) = src[i..].find('>').map(|e| i + e) else { break };
            flush(&mut stack, &src[text_start..i]);
            let inner = &src[i + 1..end];
            i = end + 1;
            text_start = i;
            if inner.starts_with('!') || inner.starts_with('?') {
                continue;
            }
            if let Some(name) = inner.strip_prefix('/') {
                let name = name.trim().to_lowercase();
                // close up to the matching element
                if let Some(pos) = stack.iter().rposition(|e| e.tag == name) {
                    while stack.len() > pos.max(1) {
                        let e = stack.pop().unwrap();
                        stack.last_mut().unwrap().children.push(Node::El(e));
                    }
                }
                continue;
            }
            let self_close = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let (name, rest) = inner.split_once(|c: char| c.is_whitespace()).unwrap_or((inner, ""));
            let mut el = El { tag: name.to_lowercase(), ..Default::default() };
            // attributes: k="v" | k='v' | k=v | k
            let mut r = rest.trim();
            while !r.is_empty() {
                let kend = r.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(r.len());
                let k = r[..kend].to_lowercase();
                r = r[kend..].trim_start();
                let mut v = String::new();
                if let Some(after) = r.strip_prefix('=') {
                    let after = after.trim_start();
                    if let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                        let close = after[1..].find(q).map(|x| x + 1).unwrap_or(after.len());
                        v = after[1..close].to_string();
                        r = after.get(close + 1..).unwrap_or("").trim_start();
                    } else {
                        let e = after.find(char::is_whitespace).unwrap_or(after.len());
                        v = after[..e].to_string();
                        r = after[e..].trim_start();
                    }
                }
                if !k.is_empty() {
                    el.attrs.push((k, decode_entities(&v)));
                }
            }
            if self_close || VOID.contains(&el.tag.as_str()) {
                stack.last_mut().unwrap().children.push(Node::El(el));
            } else {
                stack.push(el);
            }
        } else {
            i += 1;
        }
    }
    flush(&mut stack, &src[text_start.min(src.len())..]);
    while stack.len() > 1 {
        let e = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(Node::El(e));
    }
    stack.pop().unwrap()
}

// ── the writer ─────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Run {
    font: Option<String>,
    b: bool,
    i: bool,
    caps: bool,
    small_caps: bool,
    strike: bool,
    color: Option<String>,
    spacing: Option<i32>,
    size_mul: Option<f64>,
    u: bool,
    highlight: bool,
    vert: Option<&'static str>,
}

#[derive(Clone, Default)]
struct ParaOpts {
    style: String,
    page_break_before: bool,
    shd: Option<String>,
    ind: Option<i32>,
    right: Option<i32>,
    jc: Option<&'static str>,
}

const MM_TWIP: f64 = 1440.0 / 25.4;
const MM_EMU: f64 = 36000.0;

fn x(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn style_class(c: &str) -> Option<Run> {
    let r = Run::default();
    Some(match c {
        "sc-s-grito" => Run { b: true, caps: true, size_mul: Some(1.2), ..r },
        "sc-s-sussurro" => Run { i: true, size_mul: Some(0.85), color: Some("777777".into()), ..r },
        "sc-s-manuscrito" => Run { font: Some("Snell Roundhand".into()), size_mul: Some(1.15), ..r },
        "sc-s-runa" => Run { font: Some("Luminari".into()), ..r },
        "sc-s-maquina" => Run { font: Some("American Typewriter".into()), ..r },
        "sc-s-tela" => Run { font: Some("Courier New".into()), size_mul: Some(0.9), ..r },
        "sc-s-sangue" => Run { color: Some("8A0B0B".into()), b: true, ..r },
        "sc-s-versalete" => Run { small_caps: true, ..r },
        "sc-s-gigante" => Run { size_mul: Some(2.2), ..r },
        "sc-s-minusculo" => Run { size_mul: Some(0.66), ..r },
        "sc-s-espacado" => Run { spacing: Some(80), ..r },
        "sc-s-apagado" => Run { color: Some("AAAAAA".into()), ..r },
        "sc-s-riscado" => Run { strike: true, ..r },
        "sc-s-ouro" => Run { color: Some("A8812F".into()), ..r },
        "sc-s-tremido" => Run { i: true, ..r },
        _ => return None,
    })
}

/// (run, paragraph) looks of a styled block.
fn callout(kind: &str) -> (Run, ParaOpts) {
    let r = Run::default();
    let p = ParaOpts::default();
    match kind {
        "carta" => (Run { i: true, ..r }, ParaOpts { shd: Some("F6EFDF".into()), ind: Some(400), ..p }),
        "diario" => (Run { font: Some("Bradley Hand".into()), ..r }, ParaOpts { ind: Some(400), ..p }),
        "tela" => (Run { color: Some("7FE0C4".into()), font: Some("Courier New".into()), ..r }, ParaOpts { shd: Some("0F1A1C".into()), ind: Some(200), ..p }),
        "mensagem" => (r, ParaOpts { shd: Some("E9E9EE".into()), ind: Some(0), right: Some(2400), ..p }),
        "mensagem-eu" => (Run { color: Some("FFFFFF".into()), ..r }, ParaOpts { shd: Some("2F7BF6".into()), ind: Some(2400), ..p }),
        "poema" => (Run { i: true, ..r }, ParaOpts { ind: Some(1000), jc: Some("left"), ..p }),
        "epigrafe" => (Run { i: true, ..r }, ParaOpts { ind: Some(2600), jc: Some("right"), ..p }),
        "destaque" => (Run { i: true, size_mul: Some(1.3), ..r }, ParaOpts { jc: Some("center"), ..p }),
        "inscricao" => (Run { font: Some("Luminari".into()), ..r }, ParaOpts { jc: Some("center"), ..p }),
        "noticia" => (Run { font: Some("Times New Roman".into()), ..r }, p),
        "placa" => (Run { caps: true, b: true, ..r }, ParaOpts { jc: Some("center"), ..p }),
        "sonho" => (Run { i: true, color: Some("666666".into()), ..r }, ParaOpts { ind: Some(800), ..p }),
        "flashback" => (Run { i: true, ..r }, ParaOpts { ind: Some(600), ..p }),
        "centro" => (r, ParaOpts { jc: Some("center"), ..p }),
        "direita" => (r, ParaOpts { jc: Some("right"), ..p }),
        _ => (r, ParaOpts { ind: Some(400), ..p }),
    }
}

struct Media {
    name: String,
    bytes: Vec<u8>,
    ext: String,
}

pub struct Writer<'a> {
    cfg: &'a serde_json::Value,
    assets: &'a Assets,
    body: Vec<String>,
    rels: Vec<String>,
    media: Vec<Media>,
    media_by_asset: HashMap<String, (String, String)>,
    notes: Vec<(usize, String)>,
    pending_notes: HashMap<String, String>,
    img_n: usize,
    text_w: f64,
    text_h: f64,
    half_pt: i64,
}

impl<'a> Writer<'a> {
    pub fn new(cfg: &'a serde_json::Value, assets: &'a Assets) -> Writer<'a> {
        let m = &cfg["margens"];
        let g = |v: &serde_json::Value, d: f64| v.as_f64().unwrap_or(d);
        Writer {
            cfg,
            assets,
            body: Vec::new(),
            rels: Vec::new(),
            media: Vec::new(),
            media_by_asset: HashMap::new(),
            notes: Vec::new(),
            pending_notes: HashMap::new(),
            img_n: 0,
            text_w: g(&cfg["largura"], 140.0) - g(&m["interna"], 22.0) - g(&m["externa"], 16.0),
            text_h: g(&cfg["altura"], 210.0) - g(&m["superior"], 18.0) - g(&m["inferior"], 20.0),
            half_pt: (g(&cfg["corpo"], 10.5) * 2.0).round() as i64,
        }
    }

    fn rpr(&self, f: &Run) -> String {
        let mut p = String::new();
        if let Some(font) = &f.font {
            let font = x(font);
            p += &format!("<w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:cs=\"{font}\"/>");
        }
        if f.b {
            p += "<w:b/>";
        }
        if f.i {
            p += "<w:i/>";
        }
        if f.caps {
            p += "<w:caps/>";
        }
        if f.small_caps {
            p += "<w:smallCaps/>";
        }
        if f.strike {
            p += "<w:strike/>";
        }
        if let Some(c) = &f.color {
            p += &format!("<w:color w:val=\"{c}\"/>");
        }
        if let Some(s) = f.spacing {
            p += &format!("<w:spacing w:val=\"{s}\"/>");
        }
        if let Some(m) = f.size_mul {
            p += &format!("<w:sz w:val=\"{}\"/>", (self.half_pt as f64 * m).round() as i64);
        }
        if f.u {
            p += "<w:u w:val=\"single\"/>";
        }
        if f.highlight {
            p += "<w:highlight w:val=\"yellow\"/>";
        }
        if let Some(v) = f.vert {
            p += &format!("<w:vertAlign w:val=\"{v}\"/>");
        }
        if p.is_empty() { p } else { format!("<w:rPr>{p}</w:rPr>") }
    }

    fn run(&self, text: &str, f: &Run) -> String {
        format!("<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>", self.rpr(f), x(text))
    }

    fn style_from_css(st: Option<&str>, f: &Run) -> Run {
        let mut g = f.clone();
        let Some(st) = st else { return g };
        for decl in st.split(';') {
            let Some((k, v)) = decl.split_once(':') else { continue };
            let (k, v) = (k.trim().to_lowercase(), v.trim());
            match k.as_str() {
                "color" => {
                    let h = v.trim_start_matches('#');
                    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                        g.color = Some(h.to_uppercase());
                    } else if h.len() == 3 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                        g.color = Some(h.chars().flat_map(|c| [c, c]).collect::<String>().to_uppercase());
                    }
                }
                "font-family" => g.font = v.split(',').next().map(|f| f.replace(['"', '\''], "").trim().to_string()),
                "font-size" => {
                    if let Some(em) = v.strip_suffix("em").and_then(|n| n.parse::<f64>().ok()) {
                        g.size_mul = Some(g.size_mul.unwrap_or(1.0) * em);
                    }
                }
                "font-weight" if v == "bold" || v.starts_with(['6', '7', '8', '9']) => g.b = true,
                _ => {}
            }
        }
        g
    }

    fn inline(&mut self, node: &El, f: &Run) -> String {
        let mut out = String::new();
        for n in &node.children {
            match n {
                Node::Text(t) => {
                    // runs of white space become one space (edges kept, as in HTML)
                    let mut c = String::new();
                    let mut sp = false;
                    for ch in t.chars() {
                        if ch.is_whitespace() && ch != '\u{a0}' {
                            if !sp {
                                c.push(' ');
                            }
                            sp = true;
                        } else {
                            c.push(ch);
                            sp = false;
                        }
                    }
                    let t = c;
                    if !t.is_empty() {
                        out += &self.run(&t, f);
                    }
                }
                Node::El(e) => {
                    let tag = e.tag.as_str();
                    if tag == "br" {
                        out += "<w:r><w:br/></w:r>";
                        continue;
                    }
                    if tag == "img" {
                        out += &self.image(e, "inline", 1.0);
                        continue;
                    }
                    if tag == "sup" && e.has_class("sc-fnref") {
                        let href = e.find(&|x| x.tag == "a").and_then(|a| a.attr("href")).unwrap_or("").trim_start_matches('#').to_string();
                        if let Some(note) = self.pending_notes.get(&href).cloned() {
                            let id = self.notes.len() + 1;
                            self.notes.push((id, note));
                            out += &format!("<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteReference w:id=\"{id}\"/></w:r>");
                        }
                        continue;
                    }
                    let mut g = f.clone();
                    match tag {
                        "em" | "i" | "cite" => g.i = !f.i,
                        "strong" | "b" => g.b = true,
                        "s" | "del" => g.strike = true,
                        "u" | "ins" => g.u = true,
                        "mark" => g.highlight = true,
                        "sup" => g.vert = Some("superscript"),
                        "sub" => g.vert = Some("subscript"),
                        "code" => g.font = Some("Courier New".into()),
                        _ => {}
                    }
                    for c in e.classes() {
                        if let Some(s) = style_class(&c) {
                            let mul = g.size_mul.unwrap_or(1.0) * s.size_mul.unwrap_or(1.0);
                            g = Run {
                                font: s.font.or(g.font),
                                b: g.b || s.b,
                                i: g.i || s.i,
                                caps: g.caps || s.caps,
                                small_caps: g.small_caps || s.small_caps,
                                strike: g.strike || s.strike,
                                color: s.color.or(g.color),
                                spacing: s.spacing.or(g.spacing),
                                size_mul: if mul != 1.0 { Some(mul) } else { g.size_mul },
                                ..g
                            };
                        }
                    }
                    let g = Self::style_from_css(e.attr("style"), &g);
                    out += &self.inline(e, &g);
                }
            }
        }
        out
    }

    fn para(&mut self, runs: String, o: &ParaOpts) {
        let mut p = String::new();
        if !o.style.is_empty() {
            p += &format!("<w:pStyle w:val=\"{}\"/>", o.style);
        }
        if o.page_break_before {
            p += "<w:pageBreakBefore/>";
        }
        if let Some(s) = &o.shd {
            p += &format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{s}\"/>");
        }
        if o.ind.is_some() || o.right.is_some() {
            p += &format!("<w:ind w:left=\"{}\" w:right=\"{}\" w:firstLine=\"0\"/>", o.ind.unwrap_or(0), o.right.unwrap_or(0));
        }
        if let Some(j) = o.jc {
            p += &format!("<w:jc w:val=\"{}\"/>", if j == "justify" { "both" } else { j });
        }
        self.body.push(format!("<w:p>{}{runs}</w:p>", if p.is_empty() { String::new() } else { format!("<w:pPr>{p}</w:pPr>") }));
    }

    fn page_break(&mut self) {
        self.body.push("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>".into());
    }

    fn add_media(&mut self, id: &str) -> Option<(String, String)> {
        if let Some(r) = self.media_by_asset.get(id) {
            return Some(r.clone());
        }
        let a = self.assets.get(id)?;
        if a.bytes.is_empty() || a.ext == "svg" {
            return None;
        }
        let n = self.media.len() + 1;
        let name = format!("image{n}.{}", a.ext);
        let rid = format!("rIdImg{n}");
        self.media.push(Media { name: name.clone(), bytes: a.bytes.clone(), ext: a.ext.clone() });
        self.rels.push(format!("<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{name}\"/>"));
        self.media_by_asset.insert(id.to_string(), (rid.clone(), name.clone()));
        Some((rid, name))
    }

    fn image(&mut self, img: &El, mode: &str, width_frac: f64) -> String {
        let src = img.attr("src").unwrap_or("");
        let Some(id) = src.strip_prefix("sc-asset://") else { return String::new() };
        let Some((rid, name)) = self.add_media(id) else { return String::new() };
        let a = self.assets.get(id).unwrap();
        let ratio = if a.w > 0 && a.h > 0 { a.h as f64 / a.w as f64 } else { 0.75 };
        let mut wmm = self.text_w * width_frac;
        if mode == "inline" && img.has_class("sc-inline-img") {
            wmm = wmm.min(6.0);
        }
        let mut hmm = wmm * ratio;
        if hmm > self.text_h * 0.92 {
            hmm = self.text_h * 0.92;
            wmm = hmm / ratio;
        }
        let (cx, cy) = ((wmm * MM_EMU).round() as i64, (hmm * MM_EMU).round() as i64);
        self.img_n += 1;
        let id = self.img_n;
        let graphic = format!("<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>");
        if mode == "left" || mode == "right" {
            return format!("<w:r><w:drawing><wp:anchor distT=\"0\" distB=\"90000\" distL=\"{}\" distR=\"{}\" simplePos=\"0\" relativeHeight=\"{id}\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"margin\"><wp:align>{mode}</wp:align></wp:positionH><wp:positionV relativeFrom=\"paragraph\"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapSquare wrapText=\"bothSides\"/><wp:docPr id=\"{id}\" name=\"Imagem {id}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>{graphic}</wp:anchor></w:drawing></w:r>", if mode == "right" { 180000 } else { 0 }, if mode == "left" { 180000 } else { 0 });
        }
        format!("<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"{id}\" name=\"Imagem {id}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>{graphic}</wp:inline></w:drawing></w:r>")
    }

    fn width_of(e: &El) -> Option<f64> {
        let st = e.attr("style")?;
        let i = st.find("width:")?;
        let rest = st[i + 6..].trim_start();
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        rest[num.len()..].starts_with('%').then(|| num.parse::<f64>().ok().map(|n| n / 100.0)).flatten()
    }

    fn blocks(&mut self, container: &El, style: Option<&str>, run: &Run, popts: &ParaOpts) {
        for n in &container.children {
            let Node::El(e) = n else {
                if let Node::Text(t) = n {
                    if !t.trim().is_empty() {
                        let r = self.run(t.trim(), run);
                        self.para(r, &ParaOpts { style: style.unwrap_or("Normal").into(), ..popts.clone() });
                    }
                }
                continue;
            };
            let tag = e.tag.as_str();
            if tag == "p" {
                let st = style.map(str::to_string).unwrap_or_else(|| if e.has_class("sc-first") { "FirstParagraph".into() } else { "Normal".into() });
                let r = self.inline(e, run);
                self.para(r, &ParaOpts { style: st, ..popts.clone() });
            } else if tag.len() == 2 && tag.starts_with('h') && tag[1..].parse::<u8>().is_ok() {
                let lv = tag[1..].parse::<u8>().unwrap().clamp(2, 4);
                let r = self.inline(e, &Run::default());
                self.para(r, &ParaOpts { style: format!("Heading{lv}"), ..Default::default() });
            } else if e.has_class("sc-scene") {
                let t = e.text();
                let r = self.run(if t.trim().is_empty() { "⁂" } else { t.trim() }, &Run::default());
                self.para(r, &ParaOpts { style: "SceneBreak".into(), ..Default::default() });
            } else if tag == "img" && e.has_class("sc-fig") {
                let side = if e.has_class("sc-fig-esquerda") { "left" } else { "right" };
                let r = self.image(e, side, Self::width_of(e).unwrap_or(0.45));
                self.para(r, &ParaOpts { style: "Normal".into(), ..Default::default() });
            } else if tag == "figure" {
                let img = e.find(&|x| x.tag == "img").cloned();
                if e.has_class("sc-fig-composta") {
                    self.page_break();
                    let r = img.map(|i| self.image(&i, "block", 1.0)).unwrap_or_default();
                    self.para(r, &ParaOpts { style: "Figure".into(), ..Default::default() });
                    self.page_break();
                    continue;
                }
                let float = if e.has_class("sc-fig-esquerda") { Some("left") } else if e.has_class("sc-fig-direita") { Some("right") } else { None };
                let frac = Self::width_of(e).unwrap_or(if float.is_some() { 0.45 } else { 1.0 });
                if let Some(i) = img {
                    let r = self.image(&i, float.unwrap_or("block"), frac);
                    self.para(r, &ParaOpts { style: if float.is_some() { "Normal".into() } else { "Figure".into() }, ..Default::default() });
                }
                if let Some(cap) = e.find(&|x| x.tag == "figcaption").cloned() {
                    let r = self.inline(&cap, &Run::default());
                    self.para(r, &ParaOpts { style: "Caption".into(), ..Default::default() });
                }
            } else if tag == "aside" {
                let kind = e.classes().into_iter().find_map(|c| c.strip_prefix("sc-c-").map(str::to_string)).unwrap_or_default();
                let (r, mut p) = callout(&kind);
                p.style = "Callout".into();
                let title = e.els().find(|c| c.has_class("sc-callout-title")).cloned();
                if let Some(t) = &title {
                    if kind != "epigrafe" {
                        let rr = self.inline(t, &Run { small_caps: true, ..r.clone() });
                        self.para(rr, &p);
                    }
                }
                let body = e.els().find(|c| c.has_class("sc-callout-body")).cloned().unwrap_or_else(|| e.clone());
                self.blocks(&body, Some("Callout"), &r, &p);
                if let (Some(t), "epigrafe") = (&title, kind.as_str()) {
                    let rr = self.run(&format!("— {}", t.text()), &Run { small_caps: true, ..Default::default() });
                    self.para(rr, &ParaOpts { jc: Some("right"), ..p.clone() });
                }
            } else if tag == "blockquote" {
                self.blocks(e, Some("Quote"), &Run::default(), &ParaOpts::default());
            } else if tag == "ul" || tag == "ol" {
                let mut i: i64 = e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1);
                for li in e.els().filter(|c| c.tag == "li").cloned().collect::<Vec<_>>() {
                    let prefix = if tag == "ol" {
                        i += 1;
                        format!("{}. ", i - 1)
                    } else {
                        "• ".to_string()
                    };
                    let has_block = li.find(&|x| matches!(x.tag.as_str(), "p" | "ul" | "ol")).is_some();
                    if has_block {
                        let r = self.run(&prefix, &Run::default());
                        self.para(r, &ParaOpts { style: "ListPara".into(), ..Default::default() });
                        self.blocks(&li, Some("ListPara"), &Run::default(), &ParaOpts::default());
                    } else {
                        let r = self.run(&prefix, &Run::default()) + &self.inline(&li, &Run::default());
                        self.para(r, &ParaOpts { style: "ListPara".into(), ..Default::default() });
                    }
                }
            } else if tag == "table" {
                let mut rows = Vec::new();
                e.find_all(&|x| x.tag == "tr", &mut rows);
                let rows: Vec<El> = rows.into_iter().cloned().collect();
                let mut xml = String::from("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:jc w:val=\"center\"/></w:tblPr>");
                for tr in rows {
                    xml += "<w:tr>";
                    for td in tr.els().cloned().collect::<Vec<_>>() {
                        let r = self.inline(&td, &Run { b: td.tag == "th", ..Default::default() });
                        xml += &format!("<w:tc><w:tcPr><w:tcBorders><w:top w:val=\"single\" w:sz=\"4\"/><w:left w:val=\"single\" w:sz=\"4\"/><w:bottom w:val=\"single\" w:sz=\"4\"/><w:right w:val=\"single\" w:sz=\"4\"/></w:tcBorders></w:tcPr><w:p>{r}</w:p></w:tc>");
                    }
                    xml += "</w:tr>";
                }
                xml += "</w:tbl>";
                self.body.push(xml);
            } else if tag == "pre" {
                for line in e.text().split('\n') {
                    let r = self.run(line, &Run { font: Some("Courier New".into()), size_mul: Some(0.85), ..Default::default() });
                    self.para(r, &ParaOpts { style: "Code".into(), ..Default::default() });
                }
            } else if e.has_class("sc-pagebreak") {
                self.page_break();
            } else if e.has_class("sc-footnotes") || tag == "header" {
                continue;
            } else if e.els().next().is_some() {
                self.blocks(e, style, run, popts);
            } else if !e.text().trim().is_empty() {
                let r = self.inline(e, &Run::default());
                self.para(r, &ParaOpts { style: "Normal".into(), ..Default::default() });
            }
        }
    }

    pub fn title_page(&mut self, m: &export::Meta) {
        let r = self.run(&m.title, &Run::default());
        self.para(r, &ParaOpts { style: "Title".into(), ..Default::default() });
        if !m.subtitle.is_empty() {
            let r = self.run(&m.subtitle, &Run::default());
            self.para(r, &ParaOpts { style: "Subtitle".into(), ..Default::default() });
        }
        if !m.author.is_empty() {
            let r = self.run(&m.author, &Run::default());
            self.para(r, &ParaOpts { style: "Author".into(), ..Default::default() });
        }
        if !m.dedication.is_empty() {
            self.page_break();
            let r = self.run(&m.dedication, &Run { i: true, ..Default::default() });
            self.para(r, &ParaOpts { style: "Dedication".into(), ..Default::default() });
        }
    }

    pub fn chapter(&mut self, head: &str, blocks: &[String], title: &str) {
        let h = parse_html(head);
        let b = parse_html(&blocks.join(""));
        self.pending_notes.clear();
        let mut lis = Vec::new();
        b.find_all(&|x| x.tag == "li" && x.attr("id").is_some_and(|i| i.starts_with("fn-")), &mut lis);
        for li in lis {
            let mut t = li.text();
            if let Some(i) = t.rfind('↩') {
                t.truncate(i);
            }
            self.pending_notes.insert(li.attr("id").unwrap().to_string(), t.trim().to_string());
        }
        let label = h.find(&|x| x.has_class("sc-ch-num")).map(|e| e.text());
        let title_el = h.find(&|x| x.has_class("sc-ch-title")).cloned();
        if let Some(l) = &label {
            let r = self.run(l, &Run::default());
            self.para(r, &ParaOpts { style: "ChapterLabel".into(), page_break_before: true, ..Default::default() });
        }
        let r = match &title_el {
            Some(t) => self.inline(t, &Run::default()),
            None => self.run(title, &Run::default()),
        };
        self.para(r, &ParaOpts { style: "Heading1".into(), page_break_before: label.is_none(), ..Default::default() });
        if let Some(epi) = h.find(&|x| x.has_class("sc-ch-epi")).cloned() {
            let cite = epi.find(&|x| x.tag == "cite").map(|c| c.text());
            let mut no_cite = epi.clone();
            no_cite.children.retain(|n| !matches!(n, Node::El(e) if e.tag == "cite"));
            let r = self.inline(&no_cite, &Run { i: true, ..Default::default() });
            self.para(r, &ParaOpts { style: "Callout".into(), jc: Some("right"), ind: Some(2600), ..Default::default() });
            if let Some(c) = cite {
                let r = self.run(&format!("— {c}"), &Run { small_caps: true, ..Default::default() });
                self.para(r, &ParaOpts { style: "Callout".into(), jc: Some("right"), ind: Some(2600), ..Default::default() });
            }
        }
        if let Some(art) = h.find(&|x| x.tag == "img").cloned() {
            let r = self.image(&art, "block", 0.7);
            self.para(r, &ParaOpts { style: "Figure".into(), ..Default::default() });
        }
        self.blocks(&b, None, &Run::default(), &ParaOpts::default());
    }

    pub fn build(self, meta: &export::Meta) -> Vec<u8> {
        let cfg = self.cfg;
        let g = |k: &str, d: f64| cfg[k].as_f64().unwrap_or(d);
        let m = |k: &str, d: f64| cfg["margens"][k].as_f64().unwrap_or(d);
        let tw = |mm: f64| (mm * MM_TWIP).round() as i64;
        let line_tw = (g("entrelinha", 1.45) * 240.0).round() as i64;
        let title_font = cfg["fonteTitulos"].as_str().unwrap_or("Cormorant").replace('"', "");
        let body_font = cfg["fonte"].as_str().unwrap_or("Iowan Old Style").replace('"', "");
        let hp = self.half_pt;
        const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";
        let sect = format!(
            "<w:sectPr><w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/><w:footnotePr><w:numFmt w:val=\"decimal\"/></w:footnotePr><w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/><w:titlePg w:val=\"0\"/></w:sectPr>",
            tw(g("largura", 140.0)),
            tw(g("altura", 210.0)),
            tw(m("superior", 18.0)),
            tw(m("externa", 16.0)),
            tw(m("inferior", 20.0)),
            tw(m("interna", 22.0)),
            tw(m("superior", 18.0) * 0.45),
            tw(m("inferior", 20.0) * 0.4)
        );
        let document = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {NS}><w:body>{}{sect}</w:body></w:document>", self.body.join(""));
        let st = |id: &str, name: &str, ppr: &str, rpr: &str, extra: &str| {
            format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"{id}\"{extra}><w:name w:val=\"{name}\"/>{}{}{}</w:style>",
                if id != "Normal" { "<w:basedOn w:val=\"Normal\"/><w:qFormat/>" } else { "" },
                if ppr.is_empty() { String::new() } else { format!("<w:pPr>{ppr}</w:pPr>") },
                if rpr.is_empty() { String::new() } else { format!("<w:rPr>{rpr}</w:rPr>") }
            )
        };
        let tf = format!("<w:rFonts w:ascii=\"{title_font}\" w:hAnsi=\"{title_font}\"/>");
        let sz = |mul: f64| format!("<w:sz w:val=\"{}\"/>", (hp as f64 * mul).round() as i64);
        let styles = [
            st("Normal", "Normal", &format!("<w:ind w:firstLine=\"{}\"/><w:jc w:val=\"{}\"/><w:widowControl/>", (g("recuo", 1.3) * hp as f64 * 10.0).round() as i64, if cfg["justificar"].as_bool().unwrap_or(true) { "both" } else { "left" }), "", " w:default=\"1\""),
            st("FirstParagraph", "Primeiro parágrafo", "<w:ind w:firstLine=\"0\"/>", "", ""),
            st("Heading1", "heading 1", "<w:keepNext/><w:spacing w:before=\"600\" w:after=\"600\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:outlineLvl w:val=\"0\"/>", &format!("{tf}{}", sz(2.0)), ""),
            st("Heading2", "heading 2", "<w:keepNext/><w:spacing w:before=\"360\" w:after=\"180\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:outlineLvl w:val=\"1\"/>", &format!("{tf}<w:b/>{}", sz(1.3)), ""),
            st("Heading3", "heading 3", "<w:keepNext/><w:spacing w:before=\"240\" w:after=\"120\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/><w:outlineLvl w:val=\"2\"/>", &format!("{tf}<w:b/>"), ""),
            st("Heading4", "heading 4", "<w:keepNext/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", &format!("{tf}<w:i/>"), ""),
            st("ChapterLabel", "Rótulo do capítulo", "<w:keepNext/><w:spacing w:before=\"1800\" w:after=\"0\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", &format!("{tf}<w:smallCaps/><w:spacing w:val=\"40\"/><w:color w:val=\"6B5A45\"/>"), ""),
            st("SceneBreak", "Quebra de cena", "<w:spacing w:before=\"240\" w:after=\"240\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", &format!("{tf}<w:spacing w:val=\"80\"/>"), ""),
            st("Figure", "Figura", "<w:spacing w:before=\"120\" w:after=\"120\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", "", ""),
            st("Caption", "Legenda", "<w:spacing w:after=\"200\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", &format!("<w:i/>{}", sz(0.85)), ""),
            st("Quote", "Citação", "<w:spacing w:before=\"120\" w:after=\"120\"/><w:ind w:left=\"720\" w:right=\"720\" w:firstLine=\"0\"/>", &sz(0.95), ""),
            st("Callout", "Bloco estilizado", "<w:spacing w:before=\"60\" w:after=\"60\"/><w:ind w:firstLine=\"0\"/>", "", ""),
            st("ListPara", "Lista", "<w:ind w:left=\"360\" w:firstLine=\"0\"/><w:jc w:val=\"left\"/>", "", ""),
            st("Code", "Código", "<w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/>", "", ""),
            st("Title", "Title", "<w:spacing w:before=\"3000\" w:after=\"400\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", &format!("{tf}{}", sz(3.0)), ""),
            st("Subtitle", "Subtitle", "<w:spacing w:after=\"400\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", "<w:i/>", ""),
            st("Author", "Autor", "<w:spacing w:before=\"1200\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", &format!("{tf}<w:caps/><w:spacing w:val=\"60\"/>"), ""),
            st("Dedication", "Dedicatória", "<w:spacing w:before=\"3000\"/><w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", "<w:i/>", ""),
            st("FootnoteText", "footnote text", "<w:ind w:firstLine=\"0\"/><w:jc w:val=\"left\"/>", &sz(0.8), ""),
            "<w:style w:type=\"character\" w:styleId=\"FootnoteReference\"><w:name w:val=\"footnote reference\"/><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr></w:style>".to_string(),
            st("Footer", "footer", "<w:ind w:firstLine=\"0\"/><w:jc w:val=\"center\"/>", &sz(0.85), ""),
        ]
        .join("\n");
        let styles = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{body_font}\" w:hAnsi=\"{body_font}\" w:cs=\"{body_font}\" w:eastAsia=\"{body_font}\"/><w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/><w:lang w:val=\"pt-BR\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"{line_tw}\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\n{styles}\n</w:styles>");
        let notes: String = self.notes.iter().map(|(id, t)| format!("<w:footnote w:id=\"{id}\"><w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr><w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> {}</w:t></w:r></w:p></w:footnote>", x(t))).collect();
        let footnotes = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:footnotes {NS}><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>{notes}</w:footnotes>");
        let footer = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:ftr {NS}><w:p><w:pPr><w:pStyle w:val=\"Footer\"/></w:pPr><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p></w:ftr>");
        let settings = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:mirrorMargins/>{}<w:defaultTabStop w:val=\"708\"/><w:characterSpacingControl w:val=\"doNotCompress\"/><w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat><w:themeFontLang w:val=\"pt-BR\"/></w:settings>", if cfg["hifenizar"].as_bool().unwrap_or(true) { "<w:autoHyphenation/>" } else { "" });
        let rels = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rIdSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/><Relationship Id=\"rIdFootnotes\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/><Relationship Id=\"rIdFooter\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>{}</Relationships>", self.rels.join(""));
        let mut exts: Vec<&str> = self.media.iter().map(|m| m.ext.as_str()).collect();
        exts.sort();
        exts.dedup();
        let defaults: String = exts.iter().map(|e| format!("<Default Extension=\"{e}\" ContentType=\"{}\"/>", match *e { "jpg" => "image/jpeg", "gif" => "image/gif", _ => "image/png" })).collect();
        let ctypes = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>{defaults}<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/><Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/><Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>");
        let core = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:language>pt-BR</dc:language><dcterms:created xsi:type=\"dcterms:W3CDTF\">{}</dcterms:created></cp:coreProperties>", x(&meta.title), x(&meta.author), chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"));
        let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>";
        let mut files: Vec<(String, Vec<u8>, bool)> = vec![
            ("[Content_Types].xml".into(), ctypes.into_bytes(), true),
            ("_rels/.rels".into(), root_rels.as_bytes().to_vec(), true),
            ("docProps/core.xml".into(), core.into_bytes(), true),
            ("word/document.xml".into(), document.into_bytes(), true),
            ("word/styles.xml".into(), styles.into_bytes(), true),
            ("word/settings.xml".into(), settings.into_bytes(), true),
            ("word/footnotes.xml".into(), footnotes.into_bytes(), true),
            ("word/footer1.xml".into(), footer.into_bytes(), true),
            ("word/_rels/document.xml.rels".into(), rels.into_bytes(), true),
        ];
        for m in self.media {
            files.push((format!("word/media/{}", m.name), m.bytes, false));
        }
        let refs: Vec<(String, &[u8], bool)> = files.iter().map(|(n, d, z)| (n.clone(), d.as_slice(), *z)).collect();
        crate::board::zip::write_with(&refs)
    }
}

pub fn docx(root: &Path, ix: &Index, b: &Book, opts: &export::Options) -> Result<PathBuf, String> {
    let mut built: Built = export::build(root, ix, b, opts);
    let back = built.back_matter(ix, &b.path);
    built.assets.load(root, 3000, false);
    let mut w = Writer::new(&built.cfg, &built.assets);
    w.title_page(&built.meta);
    for ch in &built.chapters {
        w.chapter(&ch.head, &ch.blocks, &ch.title);
    }
    for (_, title, blocks) in &back {
        w.chapter(&format!("<header class=\"sc-ch-head\"><h1 class=\"sc-ch-title\">{title}</h1></header>"), blocks, title);
    }
    let bytes = w.build(&built.meta);
    let rel = export::out_path(b, "", "docx");
    let abs = root.join(&rel);
    if let Some(d) = abs.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&abs, bytes).map_err(|e| e.to_string())?;
    Ok(abs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_tree() {
        let t = parse_html("<p class=\"sc-first\">Um <em>dois</em> &amp; três<br/>quatro</p><img src=\"x\"/>");
        let p = t.find(&|e| e.tag == "p").unwrap();
        assert!(p.has_class("sc-first"));
        assert_eq!(p.text(), "Um dois & trêsquatro");
        assert!(t.find(&|e| e.tag == "img").is_some());
    }
}
