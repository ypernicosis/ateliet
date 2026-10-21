//! Building a book for export: chapters → HTML with the book's styles,
//! front and back matter, pictures; then EPUB 3, Markdown and a one-file web
//! reader (DOCX lives in `docx.rs`). Same structure and classes as the plugin.

use super::book::{self, Book, Chapter};
use super::css;
use super::manuscript::{self, esc, esc_text, md_blocks, md_inline, render_blocks, BlockHost, Embed, Footnotes, InlineHost, Plain};
use super::{body_section, link_text, safe_name, strip_frontmatter};
use crate::dataview::index::Index;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Epub,
    Docx,
    Markdown,
    Web,
}

pub struct Options {
    /// Curly quotes and dashes (the plugin's `tipografia`).
    pub typography: bool,
    /// What a single line break means: "paragrafo" or "linha" (`quebraSimples`).
    pub soft_break: String,
    pub author: String,
}

impl Default for Options {
    fn default() -> Self {
        Options { typography: true, soft_break: "paragrafo".into(), author: String::new() }
    }
}

// ── pictures ───────────────────────────────────────────────────────────────

pub struct Asset {
    pub id: String,
    pub path: String,
    /// File name without extension, for the package.
    pub base: String,
    pub bytes: Vec<u8>,
    pub mime: String,
    pub ext: String,
    pub w: u32,
    pub h: u32,
    pub missing: bool,
}

impl Asset {
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.base, self.ext)
    }
}

#[derive(Default)]
pub struct Assets {
    pub list: Vec<Asset>,
    by_path: HashMap<String, usize>,
}

static TOKEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"sc-asset(-data)?://([a-z0-9-]+)").unwrap());

impl Assets {
    /// A placeholder URL for a vault picture, replaced per format at the end.
    pub fn token(&mut self, path: &str) -> String {
        let i = match self.by_path.get(path) {
            Some(i) => *i,
            None => {
                let id = format!("a{}", self.list.len() + 1);
                let stem = path.rsplit('/').next().unwrap_or(path);
                let stem = stem.rsplit_once('.').map(|(a, _)| a).unwrap_or(stem);
                let slug: String = manuscript::ascii_slug(stem).chars().take(40).collect();
                self.list.push(Asset { base: format!("{id}-{slug}"), id, path: path.to_string(), bytes: Vec::new(), mime: String::new(), ext: String::new(), w: 0, h: 0, missing: false });
                self.by_path.insert(path.to_string(), self.list.len() - 1);
                self.list.len() - 1
            }
        };
        format!("sc-asset://{}", self.list[i].id)
    }

    pub fn get(&self, id: &str) -> Option<&Asset> {
        self.list.iter().find(|a| a.id == id)
    }

    /// Read the pictures, scaled to `max_dim`; Clip Studio files give their
    /// saved preview. SVG stays SVG when the format allows it.
    pub fn load(&mut self, root: &Path, max_dim: u32, svg_ok: bool) {
        for a in &mut self.list {
            if !a.bytes.is_empty() || a.missing {
                continue;
            }
            let abs = root.join(&a.path);
            let ext = a.path.rsplit('.').next().unwrap_or("").to_lowercase();
            let Ok(raw) = std::fs::read(&abs) else {
                a.missing = true;
                continue;
            };
            if ext == "svg" && svg_ok {
                a.bytes = raw;
                a.mime = "image/svg+xml".into();
                a.ext = "svg".into();
                continue;
            }
            let decoded = if ext == "clip" {
                crate::clipstudio::read(&raw).ok().and_then(|d| d.preview).and_then(|png| image::load_from_memory(&png).ok())
            } else {
                image::load_from_memory(&raw).ok()
            };
            let Some(img) = decoded else {
                a.missing = true;
                continue;
            };
            a.w = img.width();
            a.h = img.height();
            // small JPEGs and PNGs go in as they are
            let keep = matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif") && img.width() <= max_dim && img.height() <= max_dim;
            if keep {
                a.bytes = raw;
                a.ext = if ext == "jpeg" { "jpg".into() } else { ext.clone() };
                a.mime = match a.ext.as_str() {
                    "jpg" => "image/jpeg",
                    "png" => "image/png",
                    _ => "image/gif",
                }
                .into();
            } else {
                let (bytes, mime) = crate::images::thumbnail(&img, max_dim);
                let small = if img.width() > max_dim || img.height() > max_dim { img.thumbnail(max_dim, max_dim) } else { img };
                a.w = small.width();
                a.h = small.height();
                a.bytes = bytes;
                a.mime = mime.into();
                a.ext = if mime == "image/png" { "png".into() } else { "jpg".into() };
            }
        }
    }

    /// Replace the placeholders: `f(asset, wants_data_url)`.
    pub fn replace(&self, s: &str, f: impl Fn(&Asset, bool) -> String) -> String {
        TOKEN.replace_all(s, |c: &regex::Captures| self.get(&c[2]).map(|a| f(a, c.get(1).is_some())).unwrap_or_default()).into_owned()
    }

    pub fn data_url(a: &Asset) -> String {
        use base64::Engine;
        format!("data:{};base64,{}", a.mime, base64::engine::general_purpose::STANDARD.encode(&a.bytes))
    }
}

// ── the book ───────────────────────────────────────────────────────────────

pub struct Meta {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub serie: String,
    pub volume: String,
    pub publisher: String,
    pub isbn: String,
    pub year: String,
    pub lang: String,
    pub genre: String,
    pub dedication: String,
    pub epigraph: String,
    pub epigraph_author: String,
    pub credits: String,
    pub synopsis: String,
    pub backcover: String,
    pub about: String,
    pub thanks: String,
}

pub struct BuiltChapter {
    pub id: String,
    pub path: String,
    pub info: Chapter,
    pub label: String,
    pub title: String,
    pub part: String,
    /// One HTML string per block (footnotes last).
    pub blocks: Vec<String>,
    /// The chapter opening (label, title, ornament, art, epigraph).
    pub head: String,
    /// The Markdown body (no frontmatter).
    pub md: String,
}

pub struct Built {
    pub meta: Meta,
    pub chapters: Vec<BuiltChapter>,
    pub cfg: Json,
    pub assets: Assets,
    pub typography: bool,
    pub soft_break: String,
}

struct Host<'a> {
    ix: &'a Index,
    assets: &'a mut Assets,
    source: String,
    notes: Footnotes,
    typography: bool,
    ornament: String,
}

fn resolve_image(ix: &Index, link: &str, from: &str) -> Option<String> {
    let link = link.split('#').next().unwrap_or(link).trim();
    let p = ix.resolve(link, from).or_else(|| {
        // pictures are not always in the index: look for the file itself
        let direct = ix.root.join(link);
        direct.is_file().then(|| link.to_string())
    })?;
    manuscript::is_image_link(&p).then_some(p)
}

impl InlineHost for Host<'_> {
    fn typography(&self) -> bool {
        self.typography
    }

    fn inline_embed(&mut self, inner: &str) -> String {
        let e = manuscript::parse_embed(inner);
        let Some(p) = resolve_image(self.ix, &e.link, &self.source) else { return String::new() };
        let h = if e.px > 0 { format!(" style=\"height:{}em\"", e.px.min(64) as f32 / 16.0) } else { String::new() };
        format!("<img class=\"sc-inline-img\" src=\"{}\" alt=\"\"{h}/>", self.assets.token(&p))
    }

    fn inline_image(&mut self, src: &str, _alt: &str) -> String {
        let src = percent_encoding::percent_decode_str(src).decode_utf8_lossy().into_owned();
        match resolve_image(self.ix, &src, &self.source) {
            Some(p) => format!("<img class=\"sc-inline-img\" src=\"{}\" alt=\"\"/>", self.assets.token(&p)),
            None => String::new(),
        }
    }

    fn footnote_ref(&mut self, id: &str) -> String {
        self.notes.reference(id)
    }

    fn footnote_inline(&mut self, text: &str) -> String {
        self.notes.inline_note(text)
    }
}

impl BlockHost for Host<'_> {
    fn ornament(&self) -> String {
        self.ornament.clone()
    }

    /// Flowing formats: full-page pictures become ordinary figures.
    fn figure(&mut self, b: &Embed) -> String {
        let Some(p) = resolve_image(self.ix, &b.link, &self.source) else {
            return format!("<p class=\"sc-missing\">[imagem não encontrada: {}]</p>", esc(&b.link));
        };
        let src = self.assets.token(&p);
        let pos = if b.pos.is_empty() { "centro" } else { b.pos.as_str() };
        let cap = if b.caption.is_empty() { String::new() } else { format!("<figcaption>{}</figcaption>", md_inline(&b.caption, &mut Plain { typography: self.typography })) };
        let mut w = b.width.clone();
        if w.is_empty() && b.px > 0 {
            w = format!("{}%", ((b.px as f32 / 6.5).round() as u32).min(100));
        }
        if w.is_empty() && (pos == "esquerda" || pos == "direita") {
            w = "45%".into();
        }
        let mut style = Vec::new();
        if !w.is_empty() && !matches!(pos, "pagina" | "sangria" | "dupla") {
            style.push(if matches!(pos, "esquerda" | "direita" | "margem") { format!("width:{w}") } else { format!("width:{w};margin-left:auto;margin-right:auto") });
        }
        if b.contour && (pos == "esquerda" || pos == "direita") {
            return format!("<img class=\"sc-fig sc-fig-{pos} sc-contour\" src=\"{src}\" alt=\"{}\" style=\"{}\"/>", esc(&b.caption), style.join(";"));
        }
        let cls = format!("sc-fig sc-fig-{pos}{}", if b.contour { " sc-contour" } else { "" });
        let st = if style.is_empty() { String::new() } else { format!(" style=\"{}\"", style.join(";")) };
        format!("<figure class=\"{cls}\"{st}><img src=\"{src}\" alt=\"{}\"/>{cap}</figure>", esc(&b.caption))
    }
}

fn s(fm: &Json, k: &str) -> String {
    match fm.get(k) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// "Capítulo Um" → "Um" (for the table of contents).
pub fn label_core(cfg: &Json, label: &str) -> String {
    let pre = cfg["rotuloPrefixo"].as_str().unwrap_or("Capítulo").trim().to_string();
    if pre.is_empty() {
        return label.to_string();
    }
    label.strip_prefix(&format!("{pre} ")).unwrap_or(label).to_string()
}

fn opening_html(cfg: &Json, ch: &Chapter, fm: &Json, title: &str, label: &str, ix: &Index, assets: &mut Assets, source: &str, typography: bool) -> String {
    let mut art = String::new();
    let abertura = link_text(fm.get("abertura").unwrap_or(&Json::Null));
    if !abertura.is_empty() {
        if let Some(p) = resolve_image(ix, &abertura, source) {
            art = format!("<figure class=\"sc-ch-art\"><img src=\"{}\" alt=\"\"/></figure>", assets.token(&p));
        }
    }
    let epi_text = s(fm, "epigrafe");
    let epi = if epi_text.is_empty() {
        String::new()
    } else {
        let who = s(fm, "epigrafe_autor");
        format!("<blockquote class=\"sc-ch-epi\">{}{}</blockquote>", md_inline(&epi_text, &mut Plain { typography }), if who.is_empty() { String::new() } else { format!("<cite>{}</cite>", esc(&who)) })
    };
    let orn_c = cfg["ornamentoAbertura"].as_str().unwrap_or("");
    let orn = if orn_c.is_empty() { String::new() } else { format!("<div class=\"sc-ch-orn\">{}</div>", esc(orn_c)) };
    let show_title = fm.get("mostrar_titulo") != Some(&Json::Bool(false));
    let lab = if label.is_empty() { String::new() } else { format!("<div class=\"sc-ch-num\">{}</div>", esc(label)) };
    let tit = if show_title { format!("<h1 class=\"sc-ch-title\">{}</h1>", md_inline(title, &mut Plain { typography })) } else { String::new() };
    let fio = cfg["fio"].as_str().unwrap_or("nenhum");
    let rule = if fio.is_empty() || fio == "nenhum" { String::new() } else { format!("<div class=\"sc-ch-rule sc-rule-{}\"></div>", esc(fio)) };
    let below = cfg["rotuloPosicao"].as_str() == Some("abaixo");
    let between = cfg["fioLugar"].as_str() == Some("entre") && !lab.is_empty() && !tit.is_empty();
    let inner = match (below, between) {
        (true, true) => format!("{tit}{rule}{lab}"),
        (true, false) => format!("{tit}{lab}{rule}"),
        (false, true) => format!("{lab}{rule}{tit}"),
        (false, false) => format!("{lab}{tit}{rule}"),
    };
    let _ = ch;
    format!("<header class=\"sc-ch-head\">{inner}{orn}{art}{epi}</header>")
}

/// Wrap the first words of the opening paragraph in small caps.
pub fn wrap_lead_words(html: &str, n_words: usize) -> String {
    static P: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)^(<p[^>]*>)(.*)(</p>)$").unwrap());
    static TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
    static WORD: Lazy<Regex> = Lazy::new(|| Regex::new(r"\S+").unwrap());
    let Some(m) = P.captures(html) else { return html.to_string() };
    let inner = &m[2];
    // split into tags and text
    let mut toks: Vec<&str> = Vec::new();
    let mut last = 0;
    for t in TAG.find_iter(inner) {
        toks.push(&inner[last..t.start()]);
        toks.push(t.as_str());
        last = t.end();
    }
    toks.push(&inner[last..]);
    let (mut words, mut depth, mut out, mut done, mut pending) = (0usize, 0i32, String::new(), false, false);
    for t in toks {
        if done || t.is_empty() {
            out.push_str(t);
            continue;
        }
        if t.starts_with('<') {
            if t.starts_with("</") {
                depth -= 1;
            } else if !t.ends_with("/>") && !t.starts_with("<br") && !t.starts_with("<img") && !t.starts_with("<wbr") {
                depth += 1;
            }
            out.push_str(t);
            if pending && depth <= 0 {
                out.push_str("</span>");
                done = true;
            }
            continue;
        }
        if pending {
            out.push_str(t);
            continue;
        }
        let mut cut = None;
        for w in WORD.find_iter(t) {
            words += 1;
            if words >= n_words || w.as_str().ends_with([',', ';', ':', '.', '!', '?', '—', '…']) {
                cut = Some(w.end());
                break;
            }
        }
        match cut {
            None => out.push_str(t),
            Some(c) if depth <= 0 => {
                out.push_str(&t[..c]);
                out.push_str("</span>");
                out.push_str(&t[c..]);
                done = true;
            }
            Some(_) => {
                out.push_str(t);
                pending = true;
            }
        }
    }
    if !done {
        out.push_str("</span>");
    }
    format!("{}<span class=\"sc-lead\">{out}{}", &m[1], &m[3])
}

/// Read and render the whole book.
pub fn build(root: &Path, ix: &Index, b: &Book, opts: &Options) -> Built {
    let cfg = book::layout(b);
    let mut assets = Assets::default();
    let book_text = std::fs::read_to_string(root.join(&b.path)).unwrap_or_default();
    let ornament = cfg["ornamento"].as_str().unwrap_or("⁂").to_string();
    let mut chapters = Vec::new();
    let mut n = 0;
    for info in book::chapters(ix, b) {
        if !info.include {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&info.path)).unwrap_or_default();
        let parsed = md_blocks(&text, &opts.soft_break);
        let id = format!("ch{}", chapters.len() + 1);
        if info.numbered {
            n += 1;
        }
        let label = book::chapter_label(&cfg, &info, n);
        let title = if !label.is_empty() && info.numbered { book::strip_number_prefix(&info.title, n) } else { info.title.clone() };
        let fm = ix.page(&info.path).map(|p| p.yaml.clone()).unwrap_or(Json::Null);
        let mut host = Host { ix, assets: &mut assets, source: info.path.clone(), notes: Footnotes::new(&id, parsed.footnotes.clone()), typography: opts.typography, ornament: ornament.clone() };
        let mut blocks = render_blocks(&parsed.blocks, &mut host, false);
        if blocks.first().is_some_and(|f| f.starts_with("<p class=\"sc-first\"")) {
            let extra = format!("sc-first sc-opener{}", if cfg["capitular"].as_bool().unwrap_or(false) { " sc-dropcap" } else { "" });
            blocks[0] = blocks[0].replacen("class=\"sc-first\"", &format!("class=\"{extra}\""), 1);
            if cfg["versaleteInicio"].as_bool().unwrap_or(false) && cfg["versaleteAlcance"].as_str() == Some("palavras") {
                blocks[0] = wrap_lead_words(&blocks[0], 4);
            }
        }
        let notes = host.notes.html(opts.typography);
        if !notes.is_empty() {
            blocks.push(notes);
        }
        let head = opening_html(&cfg, &info, &fm, &title, &label, ix, &mut assets, &info.path, opts.typography);
        chapters.push(BuiltChapter { id, path: info.path.clone(), part: info.part.clone(), label, title, blocks, head, md: strip_frontmatter(&text).to_string(), info });
    }
    let fm = &b.fm;
    let meta = Meta {
        title: b.title.clone(),
        subtitle: b.subtitle.clone(),
        author: if b.author.is_empty() { opts.author.clone() } else { b.author.clone() },
        serie: b.serie.clone(),
        volume: b.volume.clone(),
        publisher: s(fm, "editora"),
        isbn: s(fm, "isbn"),
        year: Some(s(fm, "ano")).filter(|y| !y.is_empty()).unwrap_or_else(|| chrono::Local::now().format("%Y").to_string()),
        lang: Some(s(fm, "idioma")).filter(|y| !y.is_empty()).unwrap_or_else(|| "pt-BR".into()),
        genre: s(fm, "genero"),
        dedication: s(fm, "dedicatoria"),
        epigraph: s(fm, "epigrafe"),
        epigraph_author: s(fm, "epigrafe_autor"),
        credits: s(fm, "creditos"),
        synopsis: body_section(&book_text, "Sinopse"),
        backcover: body_section(&book_text, "Contracapa"),
        about: body_section(&book_text, "Sobre o autor"),
        thanks: body_section(&book_text, "Agradecimentos"),
    };
    Built { meta, chapters, cfg, assets, typography: opts.typography, soft_break: opts.soft_break.clone() }
}

impl Built {
    /// Back matter ("Agradecimentos", "Sobre o autor") as rendered blocks.
    pub fn back_matter(&mut self, ix: &Index, book_path: &str) -> Vec<(&'static str, &'static str, Vec<String>)> {
        let mut out = Vec::new();
        for (key, title, text) in [("thanks", "Agradecimentos", self.meta.thanks.clone()), ("about", "Sobre o autor", self.meta.about.clone())] {
            if text.trim().is_empty() {
                continue;
            }
            let parsed = md_blocks(&text, &self.soft_break);
            let mut host = Host { ix, assets: &mut self.assets, source: book_path.to_string(), notes: Footnotes::new(&format!("bm-{key}"), parsed.footnotes.clone()), typography: self.typography, ornament: "⁂".into() };
            let mut blocks = render_blocks(&parsed.blocks, &mut host, false);
            let notes = host.notes.html(self.typography);
            if !notes.is_empty() {
                blocks.push(notes);
            }
            out.push((key, title, blocks));
        }
        out
    }
}

/// `Livros/X/Exportações/X.epub`
pub fn out_path(b: &Book, suffix: &str, ext: &str) -> String {
    let folder = if b.folder.is_empty() { "Exportações".to_string() } else { format!("{}/Exportações", b.folder) };
    format!("{folder}/{}{}.{ext}", safe_name(&b.title), if suffix.is_empty() { String::new() } else { format!(" — {suffix}") })
}

fn write_out(root: &Path, rel: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let abs = root.join(rel);
    if let Some(d) = abs.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&abs, bytes).map_err(|e| e.to_string())?;
    Ok(abs)
}

/// HTML from our own renderer → well-formed XHTML (void tags closed, named entities numeric).
pub fn to_xhtml(html: &str) -> String {
    static VOID: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<(br|hr|img|wbr|col|source|meta|link|input)(\s[^<>]*?)?\s*/?>").unwrap());
    static ENT: Lazy<Regex> = Lazy::new(|| Regex::new(r"&([a-zA-Z][a-zA-Z0-9]*);").unwrap());
    static BARE_AMP: Lazy<Regex> = Lazy::new(|| Regex::new(r"&([^a-zA-Z#]|$)").unwrap());
    let t = VOID.replace_all(html, |c: &regex::Captures| format!("<{}{}/>", c[1].to_lowercase(), c.get(2).map(|m| m.as_str().trim_end()).unwrap_or("")));
    let t = BARE_AMP.replace_all(&t, "&amp;$1");
    ENT.replace_all(&t, |c: &regex::Captures| {
        match &c[1] {
            "amp" | "lt" | "gt" | "quot" | "apos" => c[0].to_string(),
            "nbsp" => "&#160;".into(),
            "mdash" => "—".into(),
            "ndash" => "–".into(),
            "hellip" => "…".into(),
            "laquo" => "«".into(),
            "raquo" => "»".into(),
            "copy" => "©".into(),
            _ => String::new(),
        }
    })
    .replace("role=\"doc-noteref\"", "epub:type=\"noteref\" role=\"doc-noteref\"")
    .replace("role=\"doc-endnote\"", "epub:type=\"endnote\"")
}

// ── EPUB ───────────────────────────────────────────────────────────────────

/// The cover picture (the book's `capa`), as JPEG/PNG bytes.
fn cover_image(root: &Path, ix: &Index, b: &Book) -> Option<(Vec<u8>, &'static str)> {
    let capa = link_text(b.fm.get("capa").unwrap_or(&Json::Null));
    if capa.is_empty() {
        return None;
    }
    let p = resolve_image(ix, &capa, &b.path)?;
    let raw = std::fs::read(root.join(&p)).ok()?;
    let img = if p.to_lowercase().ends_with(".clip") { image::load_from_memory(&crate::clipstudio::read(&raw).ok()?.preview?).ok()? } else { image::load_from_memory(&raw).ok()? };
    let (bytes, mime) = crate::images::thumbnail(&img, 1600);
    Some((bytes, if mime == "image/png" { "png" } else { "jpg" }))
}

pub fn epub(root: &Path, ix: &Index, b: &Book, opts: &Options) -> Result<PathBuf, String> {
    let mut built = build(root, ix, b, opts);
    let back = built.back_matter(ix, &b.path);
    built.assets.load(root, 2200, true);
    let meta = &built.meta;
    let lang = meta.lang.clone();
    let cover = cover_image(root, ix, b);
    let uuid = {
        use sha2::Digest;
        let h = sha2::Sha256::digest(format!("{}{}{}", b.path, meta.title, chrono::Local::now().timestamp_nanos_opt().unwrap_or(0)).as_bytes());
        let x: String = h.iter().take(16).map(|b| format!("{b:02x}")).collect();
        format!("urn:uuid:{}-{}-{}-{}-{}", &x[0..8], &x[8..12], &x[12..16], &x[16..20], &x[20..32])
    };
    let page = |title: &str, body: &str, kind: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{lang}\" xml:lang=\"{lang}\"><head><meta charset=\"UTF-8\"/><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"../styles/book.css\"/></head><body class=\"sc-text\"{}>{body}</body></html>",
            esc(title),
            if kind.is_empty() { String::new() } else { format!(" epub:type=\"{kind}\"") }
        )
    };
    struct Doc {
        id: String,
        href: String,
        title: String,
        xhtml: String,
        toc: bool,
        linear: bool,
    }
    let mut docs: Vec<Doc> = Vec::new();
    let mut files: Vec<(String, Vec<u8>, bool)> = vec![("mimetype".into(), b"application/epub+zip".to_vec(), false)];
    files.push(("META-INF/container.xml".into(), b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>".to_vec(), true));
    if let Some((bytes, ext)) = &cover {
        files.push((format!("OEBPS/images/cover.{ext}"), bytes.clone(), false));
        docs.push(Doc { id: "cover".into(), href: "text/cover.xhtml".into(), title: "Capa".into(), xhtml: page("Capa", &format!("<div style=\"text-align:center;margin:0;padding:0\"><img src=\"../images/cover.{ext}\" alt=\"{}\" style=\"max-width:100%;max-height:100vh\"/></div>", esc(&meta.title)), "cover"), toc: false, linear: false });
    }
    let typo = opts.typography;
    let credits: String = [format!("© {} {}. Todos os direitos reservados.", esc(&meta.year), esc(&meta.author)), if meta.isbn.is_empty() { String::new() } else { format!("ISBN {}", esc(&meta.isbn)) }, esc(&meta.publisher)].into_iter().filter(|l| !l.is_empty()).map(|l| format!("<p>{l}</p>")).collect();
    docs.push(Doc {
        id: "titlepage".into(),
        href: "text/titlepage.xhtml".into(),
        title: meta.title.clone(),
        xhtml: page(&meta.title, &format!("<section class=\"sc-titlepage\" epub:type=\"titlepage\"><div class=\"sc-tp-title\">{}</div>{}<div class=\"sc-tp-author\">{}</div></section><section class=\"sc-credits\" epub:type=\"copyright-page\">{credits}</section>", esc(&meta.title), if meta.subtitle.is_empty() { String::new() } else { format!("<div class=\"sc-tp-sub\">{}</div>", esc(&meta.subtitle)) }, esc(&meta.author)), ""),
        toc: false,
        linear: true,
    });
    if !meta.dedication.is_empty() {
        docs.push(Doc { id: "dedication".into(), href: "text/dedication.xhtml".into(), title: "Dedicatória".into(), xhtml: page("Dedicatória", &format!("<section class=\"sc-dedic\" epub:type=\"dedication\"><p>{}</p></section>", to_xhtml(&md_inline(&meta.dedication, &mut Plain { typography: typo }))), ""), toc: false, linear: true });
    }
    if !meta.epigraph.is_empty() {
        docs.push(Doc { id: "epigraph".into(), href: "text/epigraph.xhtml".into(), title: "Epígrafe".into(), xhtml: page("Epígrafe", &format!("<section class=\"sc-epigraph\" epub:type=\"epigraph\"><p>{}</p>{}</section>", to_xhtml(&md_inline(&meta.epigraph, &mut Plain { typography: typo })), if meta.epigraph_author.is_empty() { String::new() } else { format!("<cite>{}</cite>", esc(&meta.epigraph_author)) }), ""), toc: false, linear: true });
    }
    let image_href = |a: &Asset, _: bool| format!("../images/{}", a.file_name());
    let mut last_part = String::new();
    let mut part_n = 0;
    for ch in &built.chapters {
        if !ch.part.is_empty() && ch.part != last_part {
            last_part = ch.part.clone();
            part_n += 1;
            let label = built.cfg["rotuloParte"].as_str().unwrap_or("Parte");
            docs.push(Doc { id: format!("part{part_n}"), href: format!("text/part{part_n}.xhtml"), title: ch.part.clone(), xhtml: page(&ch.part, &format!("<section class=\"sc-part-page\" epub:type=\"part\"><div class=\"sc-part-label\">{}</div><h1 class=\"sc-part-title\">{}</h1></section>", esc(label), esc(&ch.part)), ""), toc: true, linear: true });
        }
        let body = built.assets.replace(&format!("{}{}", ch.head, ch.blocks.join("")), image_href);
        let kind = match ch.info.tipo.as_str() {
            "prologo" => "prologue",
            "epilogo" => "epilogue",
            _ => "chapter",
        };
        docs.push(Doc {
            id: ch.id.clone(),
            href: format!("text/{}.xhtml", ch.id),
            title: if ch.label.is_empty() { ch.title.clone() } else { format!("{} — {}", ch.label, ch.title) },
            xhtml: page(&ch.title, &format!("<section epub:type=\"{kind}\" role=\"doc-{kind}\">{}</section>", to_xhtml(&body)), ""),
            toc: true,
            linear: true,
        });
    }
    for (key, title, blocks) in &back {
        let body = built.assets.replace(&blocks.join(""), image_href);
        let kind = if *key == "thanks" { "acknowledgments" } else { "appendix" };
        docs.push(Doc { id: key.to_string(), href: format!("text/{key}.xhtml"), title: title.to_string(), xhtml: page(title, &format!("<section epub:type=\"{kind}\"><h1 class=\"sc-ch-title\" style=\"text-align:center\">{title}</h1>{}</section>", to_xhtml(&body)), ""), toc: true, linear: true });
    }
    let toc_items: String = docs.iter().filter(|d| d.toc).map(|d| format!("<li><a href=\"{}\">{}</a></li>", d.href.trim_start_matches("text/"), esc(&d.title))).collect();
    let first = built.chapters.first().map(|c| format!("<li><a epub:type=\"bodymatter\" href=\"{}.xhtml\">Início</a></li>", c.id)).unwrap_or_default();
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"{lang}\" xml:lang=\"{lang}\"><head><meta charset=\"UTF-8\"/><title>Sumário</title><link rel=\"stylesheet\" type=\"text/css\" href=\"../styles/book.css\"/></head><body class=\"sc-text\"><nav epub:type=\"toc\" id=\"toc\" role=\"doc-toc\"><h1 class=\"sc-ch-title\" style=\"text-align:center\">Sumário</h1><ol>{toc_items}</ol></nav><nav epub:type=\"landmarks\" hidden=\"hidden\"><ol>{}<li><a epub:type=\"toc\" href=\"nav.xhtml\">Sumário</a></li>{first}</ol></nav></body></html>",
        if cover.is_some() { "<li><a epub:type=\"cover\" href=\"cover.xhtml\">Capa</a></li>" } else { "" }
    );
    files.push(("OEBPS/text/nav.xhtml".into(), nav.into_bytes(), true));
    for d in &docs {
        files.push((format!("OEBPS/{}", d.href), d.xhtml.clone().into_bytes(), true));
    }
    files.push(("OEBPS/styles/book.css".into(), css::reflow_css(&built.cfg, "").into_bytes(), true));
    for a in built.assets.list.iter().filter(|a| !a.bytes.is_empty()) {
        files.push((format!("OEBPS/images/{}", a.file_name()), a.bytes.clone(), a.ext == "svg"));
    }
    let mut manifest = vec![
        "<item id=\"nav\" href=\"text/nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>".to_string(),
        "<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>".to_string(),
        "<item id=\"css\" href=\"styles/book.css\" media-type=\"text/css\"/>".to_string(),
    ];
    for d in &docs {
        manifest.push(format!("<item id=\"{}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>", d.id, d.href));
    }
    for a in built.assets.list.iter().filter(|a| !a.bytes.is_empty()) {
        manifest.push(format!("<item id=\"img-{}\" href=\"images/{}\" media-type=\"{}\"/>", a.id, a.file_name(), a.mime));
    }
    if let Some((_, ext)) = &cover {
        manifest.push(format!("<item id=\"cover-image\" href=\"images/cover.{ext}\" media-type=\"{}\" properties=\"cover-image\"/>", if *ext == "png" { "image/png" } else { "image/jpeg" }));
    }
    let id = if meta.isbn.is_empty() { uuid.clone() } else { format!("urn:isbn:{}", esc(&meta.isbn)) };
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let opt = |tag: &str, v: &str| if v.is_empty() { String::new() } else { format!("<dc:{tag}>{}</dc:{tag}>\n", esc(v)) };
    let serie = if meta.serie.is_empty() {
        String::new()
    } else {
        format!("<meta property=\"belongs-to-collection\" id=\"col\">{}</meta><meta refines=\"#col\" property=\"collection-type\">series</meta>{}\n", esc(&meta.serie), if meta.volume.is_empty() { String::new() } else { format!("<meta refines=\"#col\" property=\"group-position\">{}</meta>", esc(&meta.volume)) })
    };
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{id}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:creator id=\"author\">{}</dc:creator>\n<dc:language>{}</dc:language>\n{}{}{}<dc:date>{}</dc:date>\n<meta property=\"dcterms:modified\">{now}</meta>\n{serie}{}</metadata>\n<manifest>\n{}\n</manifest>\n<spine toc=\"ncx\">\n{}\n</spine>\n</package>",
        esc(&meta.title),
        esc(&meta.author),
        esc(&lang),
        opt("publisher", &meta.publisher),
        opt("description", &meta.synopsis),
        opt("subject", &meta.genre),
        esc(&meta.year),
        if cover.is_some() { "<meta name=\"cover\" content=\"cover-image\"/>\n" } else { "" },
        manifest.join("\n"),
        // the table of contents is in the spine too (the landmarks point at it)
        docs.iter().map(|d| format!("<itemref idref=\"{}\"{}/>", d.id, if d.linear { "" } else { " linear=\"no\"" })).chain(std::iter::once("<itemref idref=\"nav\" linear=\"no\"/>".to_string())).collect::<Vec<_>>().join("\n"),
    );
    files.push(("OEBPS/content.opf".into(), opf.into_bytes(), true));
    let nav_points: String = docs.iter().filter(|d| d.toc).enumerate().map(|(i, d)| format!("<navPoint id=\"np{0}\" playOrder=\"{0}\"><navLabel><text>{1}</text></navLabel><content src=\"{2}\"/></navPoint>", i + 1, esc(&d.title), d.href)).collect();
    let ncx = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\"><head><meta name=\"dtb:uid\" content=\"{id}\"/></head><docTitle><text>{}</text></docTitle><navMap>{nav_points}</navMap></ncx>", esc(&meta.title));
    files.push(("OEBPS/toc.ncx".into(), ncx.into_bytes(), true));
    let refs: Vec<(String, &[u8], bool)> = files.iter().map(|(n, d, z)| (n.clone(), d.as_slice(), *z)).collect();
    let zip = crate::board::zip::write_with(&refs);
    write_out(root, &out_path(b, "", "epub"), &zip)
}

// ── Markdown ───────────────────────────────────────────────────────────────

/// One portable Markdown file plus an `assets` folder.
pub fn markdown(root: &Path, ix: &Index, b: &Book, opts: &Options) -> Result<PathBuf, String> {
    let built = build(root, ix, b, opts);
    let folder_rel = format!("{}/{} (Markdown)", out_path(b, "", "md").rsplit_once('/').map(|(d, _)| d).unwrap_or("Exportações"), safe_name(&b.title));
    let folder = root.join(&folder_rel);
    std::fs::create_dir_all(folder.join("assets")).map_err(|e| e.to_string())?;
    let mut used: HashMap<String, String> = HashMap::new();
    let mut add_asset = |path: &str| -> Option<String> {
        if let Some(r) = used.get(path) {
            return Some(r.clone());
        }
        let mut one = Assets::default();
        one.token(path);
        one.load(root, 3000, true);
        let a = one.list.pop()?;
        if a.bytes.is_empty() {
            return None;
        }
        let stem = path.rsplit('/').next().unwrap_or(path);
        let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
        let name = format!("{}.{}", manuscript::ascii_slug(stem), a.ext);
        std::fs::write(folder.join("assets").join(&name), &a.bytes).ok()?;
        let rel = format!("assets/{name}");
        used.insert(path.to_string(), rel.clone());
        Some(rel)
    };
    let m = &built.meta;
    let mut lines = vec![format!("# {}", m.title), String::new()];
    if !m.subtitle.is_empty() {
        lines.extend([format!("*{}*", m.subtitle), String::new()]);
    }
    if !m.author.is_empty() {
        lines.extend([format!("**{}**", m.author), String::new()]);
    }
    if !m.synopsis.is_empty() {
        lines.extend([format!("> {}", m.synopsis.replace('\n', "\n> ")), String::new()]);
    }
    static COMMENT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)%%.*?%%").unwrap());
    static TOOL: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)```(playlist|scriptorium-[\w-]+)[^\n]*\n.*?\n```").unwrap());
    static EMBED: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\[\[([^\]]+)\]\]").unwrap());
    static SPAN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[([^\[\]]+)\]\{[^}]+\}").unwrap());
    static ALIAS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\[([^\]|]+)\|([^\]]+)\]\]").unwrap());
    static WIKI: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());
    static BREAK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?im)^>\s*\[!(pagina|pagina-impar|quebra)\][^\n]*$").unwrap());
    static CALLOUT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?im)^>\s*\[!([^\]]+)\][+-]?\s*(.*)$").unwrap());
    for ch in &built.chapters {
        let md = COMMENT.replace_all(&ch.md, "");
        let md = TOOL.replace_all(&md, "");
        let md = EMBED.replace_all(&md, |c: &regex::Captures| {
            let e = manuscript::parse_embed(&c[1]);
            match resolve_image(ix, &e.link, &ch.path).and_then(|p| add_asset(&p)) {
                Some(rel) => format!("![{}]({})", e.caption, rel.replace(' ', "%20")),
                None => String::new(),
            }
        });
        let md = SPAN.replace_all(&md, "$1");
        let md = ALIAS.replace_all(&md, "$2");
        let md = WIKI.replace_all(&md, |c: &regex::Captures| c[1].rsplit('#').next().unwrap_or(&c[1]).to_string());
        let md = BREAK.replace_all(&md, "\n<div style=\"page-break-after: always\"></div>\n");
        let md = CALLOUT.replace_all(&md, |c: &regex::Captures| {
            let t = if c[2].trim().is_empty() {
                let k = &c[1];
                let mut cs = k.chars();
                cs.next().map(|f| f.to_uppercase().collect::<String>() + cs.as_str()).unwrap_or_default()
            } else {
                c[2].to_string()
            };
            format!("> **{t}**  ")
        });
        let heading = if ch.label.is_empty() { ch.title.clone() } else { format!("{} — {}", ch.label, ch.title) };
        lines.extend([String::new(), "---".into(), String::new(), format!("## {heading}"), String::new(), md.trim().to_string(), String::new()]);
    }
    for (t, v) in [("Agradecimentos", &m.thanks), ("Sobre o autor", &m.about)] {
        if !v.is_empty() {
            lines.extend([String::new(), "---".into(), String::new(), format!("## {t}"), String::new(), v.clone()]);
        }
    }
    let out = folder.join(format!("{}.md", safe_name(&b.title)));
    std::fs::write(&out, lines.join("\n")).map_err(|e| e.to_string())?;
    Ok(out)
}

// ── web reader ─────────────────────────────────────────────────────────────

/// A single HTML file that reads well on a phone: table of contents, light
/// and dark, pictures inside.
pub fn web(root: &Path, ix: &Index, b: &Book, opts: &Options) -> Result<PathBuf, String> {
    let mut built = build(root, ix, b, opts);
    let back = built.back_matter(ix, &b.path);
    built.assets.load(root, 2000, true);
    let m = &built.meta;
    let data = |a: &Asset, _: bool| Assets::data_url(a);
    let mut toc = String::new();
    let mut body = String::new();
    body.push_str(&format!(
        "<section class=\"sc-titlepage\"><div class=\"sc-tp-title\">{}</div>{}<div class=\"sc-tp-author\">{}</div></section>",
        esc(&m.title),
        if m.subtitle.is_empty() { String::new() } else { format!("<div class=\"sc-tp-sub\">{}</div>", esc(&m.subtitle)) },
        esc(&m.author)
    ));
    let mut last_part = String::new();
    for ch in &built.chapters {
        if !ch.part.is_empty() && ch.part != last_part {
            last_part = ch.part.clone();
            toc.push_str(&format!("<li class=\"part\">{}</li>", esc(&ch.part)));
            body.push_str(&format!("<section class=\"sc-part-page\"><div class=\"sc-part-label\">{}</div><h1 class=\"sc-part-title\">{}</h1></section>", esc(built.cfg["rotuloParte"].as_str().unwrap_or("Parte")), esc(&ch.part)));
        }
        toc.push_str(&format!("<li><a href=\"#{}\">{}{}</a></li>", ch.id, if ch.label.is_empty() { String::new() } else { format!("<span>{}</span> ", esc(&label_core(&built.cfg, &ch.label))) }, esc_text(&ch.title)));
        body.push_str(&format!("<section class=\"chapter\" id=\"{}\">{}{}</section>", ch.id, built.assets.replace(&ch.head, data), built.assets.replace(&ch.blocks.join(""), data)));
    }
    for (key, title, blocks) in &back {
        toc.push_str(&format!("<li><a href=\"#{key}\">{title}</a></li>"));
        body.push_str(&format!("<section class=\"chapter\" id=\"{key}\"><header class=\"sc-ch-head\"><h1 class=\"sc-ch-title\">{title}</h1></header>{}</section>", built.assets.replace(&blocks.join(""), data)));
    }
    let reader_css = r#"
html{background:#fbfaf7;color:#1d1a16}
body{max-width:36em;margin:0 auto;padding:4em 1.4em 6em;font-size:1.12rem}
.chapter{margin-top:5em}
nav{position:fixed;top:0;left:0;bottom:0;width:min(20em,82vw);background:#fff;box-shadow:0 0 40px rgba(0,0,0,.12);padding:1.5em;overflow:auto;transform:translateX(-105%);transition:transform .2s;font-family:-apple-system,Inter,"Helvetica Neue",sans-serif;font-size:.9rem;z-index:3}
nav.open{transform:none}
nav ol{list-style:none;padding:0;margin:0}
nav li{margin:.5em 0}
nav li.part{margin-top:1.4em;font-variant:small-caps;letter-spacing:.12em;opacity:.6}
nav a{color:inherit;text-decoration:none}
nav a span{opacity:.55;margin-right:.3em}
#toggle{position:fixed;top:1em;left:1em;z-index:4;border:0;border-radius:2em;padding:.5em .9em;background:rgba(255,255,255,.9);box-shadow:0 2px 12px rgba(0,0,0,.12);font:inherit;font-size:.85rem;cursor:pointer}
@media (prefers-color-scheme:dark){html{background:#161514;color:#e4e1da}nav{background:#1f1e1c}#toggle{background:rgba(40,40,38,.9);color:#e4e1da}}
"#;
    let html = format!(
        "<!doctype html>\n<html lang=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{}{reader_css}</style></head><body class=\"sc-text\"><button id=\"toggle\" onclick=\"document.querySelector('nav').classList.toggle('open')\">Sumário</button><nav onclick=\"if(event.target.closest('a'))this.classList.remove('open')\"><ol>{toc}</ol></nav>{body}</body></html>",
        esc(&m.lang),
        esc(&m.title),
        css::reflow_css(&built.cfg, "")
    );
    write_out(root, &out_path(b, "leitor", "html"), html.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports() {
        let root = std::env::temp_dir().join(format!("sc-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let nb = book::NewBook { title: "Vórtices".into(), subtitle: "Livro um".into(), author: "Eu".into(), serie: String::new(), volume: String::new(), genre: String::new(), goal: 0, deadline: String::new(), synopsis: "Uma torre.".into(), folder: String::new() };
        let paths = book::create_book(&root, &nb).unwrap();
        // a picture and a chapter using the dialect
        let img = image::RgbImage::from_pixel(40, 30, image::Rgb([200, 100, 50]));
        img.save(root.join("Livros/Vórtices/Artes/torre.png")).unwrap();
        std::fs::write(root.join(&paths[1]), "---\nscriptorium: capitulo\nordem: 1\ntitulo: Capítulo um\n---\nEra uma vez uma \"torre\" de vidro[^1].\n\n***\n\n> [!carta] Querida\n> Venha logo.\n\n![[torre.png|direita 40% \"A torre\"]]\n\n[^1]: Nota & detalhe.\n").unwrap();
        let ix = Index::build(&root);
        let b = book::books(&ix, "").remove(0);
        let opts = Options::default();
        let built = build(&root, &ix, &b, &opts);
        assert_eq!(built.chapters.len(), 1);
        let ch = &built.chapters[0];
        assert_eq!(ch.label, "Capítulo Um");
        assert!(ch.blocks[0].contains("sc-dropcap"), "{}", ch.blocks[0]);
        assert!(ch.blocks[0].contains("“torre”"));
        assert!(ch.blocks.last().unwrap().contains("sc-footnotes"));
        assert!(ch.blocks.iter().any(|h| h.contains("sc-fig-direita") && h.contains("sc-asset://a1")));
        let e = epub(&root, &ix, &b, &opts).unwrap();
        let entries = crate::board::zip::read(&std::fs::read(&e).unwrap()).unwrap();
        assert_eq!(entries[0].0, "mimetype");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"OEBPS/content.opf"));
        assert!(names.iter().any(|n| n.starts_with("OEBPS/images/a1-torre")));
        let chx = entries.iter().find(|(n, _)| n == "OEBPS/text/ch1.xhtml").unwrap();
        let x = String::from_utf8(chx.1.clone()).unwrap();
        assert!(x.contains("../images/a1-torre.png"));
        assert!(x.contains("epub:type=\"noteref\""));
        let md = markdown(&root, &ix, &b, &opts).unwrap();
        let mt = std::fs::read_to_string(md).unwrap();
        assert!(mt.contains("## Capítulo Um — Capítulo um") || mt.contains("## Capítulo Um"));
        assert!(mt.contains("![A torre](assets/torre.png)"));
        let d = super::super::docx::docx(&root, &ix, &b, &opts).unwrap();
        let dz = crate::board::zip::read(&std::fs::read(&d).unwrap()).unwrap();
        let doc = dz.iter().find(|(n, _)| n == "word/document.xml").unwrap();
        let doc = String::from_utf8(doc.1.clone()).unwrap();
        assert!(doc.contains("ChapterLabel") && doc.contains("w:footnoteReference") && doc.contains("r:embed=\"rIdImg1\""), "{doc}");
        assert!(dz.iter().any(|(n, _)| n == "word/media/image1.png"));
        let w = web(&root, &ix, &b, &opts).unwrap();
        assert!(std::fs::read_to_string(w).unwrap().contains("data:image/png;base64"));
        if let Ok(keep) = std::env::var("SC_KEEP") {
            for f in [&e, &d] {
                let _ = std::fs::copy(f, Path::new(&keep).join(f.file_name().unwrap()));
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
