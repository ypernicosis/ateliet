//! Markdown → blocks → XHTML, in the "Scriptorium dialect" the plugin reads:
//!
//! - `[texto]{.grito cor=#c00}`: text styles
//! - `![[arte.psd|direita 40% contorno "Legenda"]]`: illustrations placed in the page
//! - `> [!carta] Título`: styled blocks (letter, diary, screen, poem, epigraph…)
//! - `> [!pagina]` / `> [!pagina-impar]`: page breaks
//! - ```` ```scriptorium-pagina ````: composed pages (comics)
//!
//! The HTML uses the plugin's class names, so its CSS applies unchanged.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value as Json;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

pub const TEXT_STYLES: [(&str, &str); 23] = [
    ("grito", "Grito — negrito, caixa-alta, maior"),
    ("sussurro", "Sussurro — pequeno, itálico, apagado"),
    ("manuscrito", "Manuscrito — caligrafia"),
    ("runa", "Rúnico — fonte arcana"),
    ("maquina", "Máquina de escrever"),
    ("tela", "Tela / terminal"),
    ("glitch", "Glitch — aberração cromática"),
    ("tremido", "Tremido — treme na web, oblíquo no papel"),
    ("sangue", "Sangue — vermelho escuro"),
    ("brilho", "Brilho — aura luminosa"),
    ("versalete", "Versalete (small caps)"),
    ("gigante", "Gigante"),
    ("minusculo", "Minúsculo"),
    ("espacado", "Espaçado"),
    ("invertido", "Espelhado"),
    ("de-cabeca", "De cabeça para baixo"),
    ("vertical", "Vertical (mangá)"),
    ("apagado", "Apagado (quase invisível)"),
    ("borrado", "Borrado"),
    ("riscado", "Riscado ondulado"),
    ("ouro", "Dourado"),
    ("sombra", "Sombra densa"),
    ("contorno", "Letra vazada (contorno)"),
];

pub const CALLOUT_STYLES: [(&str, &str); 18] = [
    ("carta", "Carta"),
    ("diario", "Diário"),
    ("tela", "Tela / terminal"),
    ("mensagem", "Mensagem (balão de chat, esquerda)"),
    ("mensagem-eu", "Mensagem (balão de chat, direita)"),
    ("poema", "Poema"),
    ("epigrafe", "Epígrafe"),
    ("destaque", "Citação em destaque"),
    ("inscricao", "Inscrição / runas"),
    ("noticia", "Recorte de jornal"),
    ("placa", "Placa / cartaz"),
    ("sonho", "Sonho"),
    ("flashback", "Flashback"),
    ("colunas", "Duas colunas"),
    ("centro", "Centralizado"),
    ("direita", "Alinhado à direita"),
    ("pagina", "Quebra de página"),
    ("pagina-impar", "Quebra para página ímpar"),
];

pub const IMG_EXT: [&str; 10] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "avif", "bmp", "psd", "clip"];

/// Text inside elements.
pub fn esc_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Text inside attributes.
pub fn esc(s: &str) -> String {
    esc_text(s).replace('"', "&quot;")
}

/// "Ação Rápida" → "acao-rapida".
pub fn ascii_slug(s: &str) -> String {
    let t: String = s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for c in t.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "x".into() } else { out }
}

fn ext(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

// ── embeds ─────────────────────────────────────────────────────────────────

/// `![[arte.png|direita 40% contorno "Legenda"]]`
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Embed {
    pub link: String,
    /// esquerda, direita, centro, pagina, sangria, dupla, margem, fundo, abertura
    pub pos: String,
    /// "40%" or "60mm"
    pub width: String,
    pub px: u32,
    pub contour: bool,
    pub float: bool,
    pub caption: String,
}

pub fn parse_embed(inner: &str) -> Embed {
    let (target, opt) = match inner.split_once('|') {
        Some((t, o)) => (t.trim(), o.trim().to_string()),
        None => (inner.trim(), String::new()),
    };
    let mut e = Embed { link: target.split('#').next().unwrap_or("").to_string(), ..Default::default() };
    static CAP: Lazy<Regex> = Lazy::new(|| Regex::new(r#""([^"]*)"|“([^”]*)”"#).unwrap());
    let mut opt = opt;
    if let Some(c) = CAP.captures(&opt) {
        e.caption = c.get(1).or(c.get(2)).map(|m| m.as_str().to_string()).unwrap_or_default();
        let m = c.get(0).unwrap();
        opt = format!("{} {}", &opt[..m.start()], &opt[m.end()..]);
    }
    static WIDTH: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d+(\.\d+)?(%|mm)$").unwrap());
    static SIZE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\d+)(x\d+)?$").unwrap());
    for tok in opt.split_whitespace() {
        let t = tok.to_lowercase();
        let pos = match t.as_str() {
            "esquerda" | "left" => Some("esquerda"),
            "direita" | "right" => Some("direita"),
            "centro" | "center" => Some("centro"),
            "pagina" | "página" | "full" => Some("pagina"),
            "sangria" | "bleed" => Some("sangria"),
            "dupla" | "spread" => Some("dupla"),
            "margem" => Some("margem"),
            "fundo" => Some("fundo"),
            "abertura" => Some("abertura"),
            _ => None,
        };
        if let Some(p) = pos {
            e.pos = p.into();
        } else if WIDTH.is_match(&t) {
            e.width = t;
        } else if let Some(c) = SIZE.captures(&t) {
            e.px = c[1].parse().unwrap_or(0);
        } else if t == "contorno" || t == "wrap" {
            e.contour = true;
        } else if t == "flutuar" || t == "float" {
            e.float = true;
        }
    }
    e
}

pub fn is_image_link(link: &str) -> bool {
    IMG_EXT.contains(&ext(link).as_str())
}

// ── inline ─────────────────────────────────────────────────────────────────

/// What inline rendering asks of the book being built.
pub trait InlineHost {
    fn typography(&self) -> bool {
        true
    }
    /// `![[x]]` inside a line.
    fn inline_embed(&mut self, _inner: &str) -> String {
        String::new()
    }
    /// `![alt](src)` inside a line.
    fn inline_image(&mut self, _src: &str, _alt: &str) -> String {
        String::new()
    }
    fn footnote_ref(&mut self, _id: &str) -> String {
        String::new()
    }
    fn footnote_inline(&mut self, _text: &str) -> String {
        String::new()
    }
}

/// Inline host with nothing to resolve (plain text, titles).
pub struct Plain {
    pub typography: bool,
}

impl InlineHost for Plain {
    fn typography(&self) -> bool {
        self.typography
    }
}

fn style_attrs(s: &str) -> (Vec<String>, String, String) {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)\.([^\s.#=}]+)|#([\w-]+)|([\wçãõáéíóú-]+)=("[^"]*"|'[^']*'|[^\s}]+)"#).unwrap());
    let mut classes = Vec::new();
    let mut styles = Vec::new();
    let mut id = String::new();
    for c in RE.captures_iter(s) {
        if let Some(m) = c.get(1) {
            classes.push(format!("sc-s-{}", ascii_slug(m.as_str())));
        } else if let Some(m) = c.get(2) {
            id = m.as_str().to_string();
        } else if let (Some(k), Some(v)) = (c.get(3), c.get(4)) {
            let k = k.as_str().to_lowercase();
            let mut v = v.as_str().trim_matches(|c| c == '"' || c == '\'').to_string();
            if k == "giro" || k == "rotacao" {
                styles.push(format!("display:inline-block;transform:rotate({}deg)", v.parse::<f64>().unwrap_or(0.0)));
                continue;
            }
            let prop = match k.as_str() {
                "cor" | "color" => "color",
                "tamanho" | "size" => "font-size",
                "fonte" | "font" => "font-family",
                "fundo" | "bg" => "background",
                "peso" => "font-weight",
                "espaco" => "letter-spacing",
                "opacidade" => "opacity",
                "alinhar" => "text-align",
                "sombra" => "text-shadow",
                _ => continue,
            };
            if prop == "font-family" && v.contains(' ') && !v.contains(['"', ',']) {
                v = format!("'{v}'");
            }
            if prop == "font-size" && !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.') {
                v.push_str("em");
            }
            styles.push(format!("{prop}:{}", v.replace(['<', '>', '"'], "")));
        }
    }
    (classes, id, styles.join(";"))
}

fn sanitize_tag(tag: &str) -> String {
    static ON: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)\s(on\w+)\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#).unwrap());
    static JS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)javascript:").unwrap());
    static BR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^<(br|wbr)\s*>$").unwrap());
    let t = ON.replace_all(tag, "");
    let t = JS.replace_all(&t, "");
    BR.replace(&t, "<$1/>").into_owned()
}

/// Curly quotes, dashes and ellipses.
pub fn smart_typography(text: &str) -> String {
    static DASH3: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"(^|[^-])---(?!-)").unwrap());
    static DASH2: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"(^|[^-])--(?!-)").unwrap());
    static OPEN2: Lazy<Regex> = Lazy::new(|| Regex::new("(^|[\\s(\\[{—\u{1}\u{2}>])\"").unwrap());
    static OPEN1: Lazy<Regex> = Lazy::new(|| Regex::new("(^|[\\s(\\[{—\u{1}\u{2}>])'").unwrap());
    let t = DASH3.replace_all(text, "$1—").into_owned();
    let t = DASH2.replace_all(&t, "$1—").into_owned();
    let t = t.replace("...", "…");
    let t = OPEN2.replace_all(&t, "$1“").replace('"', "”");
    let t = OPEN1.replace_all(&t, "$1‘").replace('\'', "’");
    t
}

struct Store(Vec<String>);

impl Store {
    fn keep(&mut self, html: String) -> String {
        self.0.push(html);
        format!("\u{1}{}\u{2}", self.0.len() - 1)
    }
}

fn fancy_replace(re: &fancy_regex::Regex, s: &str, mut f: impl FnMut(&fancy_regex::Captures) -> String) -> String {
    let mut out = String::new();
    let mut last = 0;
    let mut pos = 0;
    while pos <= s.len() {
        let Ok(Some(c)) = re.captures_from_pos(s, pos) else { break };
        let m = c.get(0).unwrap();
        out.push_str(&s[last..m.start()]);
        out.push_str(&f(&c));
        last = m.end();
        pos = if m.end() > m.start() { m.end() } else { m.end() + s[m.end()..].chars().next().map(char::len_utf8).unwrap_or(1) };
    }
    out.push_str(&s[last.min(s.len())..]);
    out
}

/// One line of Markdown (with the Scriptorium additions) → HTML.
pub fn md_inline(src: &str, h: &mut dyn InlineHost) -> String {
    static CODE: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"(`+)([\s\S]*?[^`])\1(?!`)").unwrap());
    static ESCAPE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\\([\\`*_{}\[\]()#+\-.!~=|>"'])"#).unwrap());
    static TAGS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)</?(span|br|sup|sub|small|u|mark|b|i|em|strong|s|del|ins|kbd|abbr|ruby|rt|rp|big|q|cite|var|bdi|bdo|wbr)(\s[^<>]*)?/?>").unwrap());
    static EMBED: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\[\[([^\]]+)\]\]").unwrap());
    static IMAGE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap());
    static WIKI: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());
    static FNREF: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[\^([^\]\s]+)\]").unwrap());
    static FNINL: Lazy<Regex> = Lazy::new(|| Regex::new(r"\^\[([^\]]+)\]").unwrap());
    static SPAN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[([^\[\]]+)\]\{([^}]+)\}").unwrap());
    static LINK: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap());
    static AUTO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(^|[\s(])(https?://[^\s<]+[^\s<.,;:!?)])").unwrap());
    static B3: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"\*\*\*(?=\S)([\s\S]*?\S)\*\*\*").unwrap());
    static B2: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"\*\*(?=\S)([\s\S]*?\S)\*\*").unwrap());
    static U2: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"(^|[^\w])__(?=\S)([\s\S]*?\S)__(?!\w)").unwrap());
    static I1: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"\*(?=\S)([\s\S]*?\S)\*").unwrap());
    static U1: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"(^|[^\w])_(?=\S)([\s\S]*?\S)_(?!\w)").unwrap());
    static STRIKE: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"~~(?=\S)([\s\S]*?\S)~~").unwrap());
    static MARK: Lazy<fancy_regex::Regex> = Lazy::new(|| fancy_regex::Regex::new(r"==(?=\S)([\s\S]*?\S)==").unwrap());
    static SLOT: Lazy<Regex> = Lazy::new(|| Regex::new("\u{1}(\\d+)\u{2}").unwrap());

    let mut st = Store(Vec::new());
    let s = fancy_replace(&CODE, src, |c| st.keep(format!("<code>{}</code>", esc_text(c[2].trim()))));
    let s = ESCAPE.replace_all(&s, |c: &regex::Captures| st.keep(esc_text(&c[1]))).into_owned();
    let s = TAGS.replace_all(&s, |c: &regex::Captures| st.keep(sanitize_tag(&c[0]))).into_owned();
    let s = EMBED.replace_all(&s, |c: &regex::Captures| {
        let html = h.inline_embed(&c[1]);
        st.keep(html)
    }).into_owned();
    let s = IMAGE.replace_all(&s, |c: &regex::Captures| {
        let html = h.inline_image(&c[2], &c[1]);
        st.keep(html)
    }).into_owned();
    let s = esc_text(&s);
    let s = WIKI.replace_all(&s, |c: &regex::Captures| {
        let inner = &c[1];
        match inner.split_once('|') {
            Some((_, alias)) => alias.to_string(),
            None => inner.rsplit('#').next().unwrap_or(inner).rsplit('/').next().unwrap_or(inner).to_string(),
        }
    }).into_owned();
    let s = FNREF.replace_all(&s, |c: &regex::Captures| {
        let html = h.footnote_ref(&c[1]);
        st.keep(html)
    }).into_owned();
    let mut s = FNINL.replace_all(&s, |c: &regex::Captures| {
        let html = h.footnote_inline(&c[1]);
        st.keep(html)
    }).into_owned();
    for _ in 0..4 {
        let before = s.clone();
        s = SPAN.replace_all(&s, |c: &regex::Captures| {
            let (classes, id, style) = style_attrs(&c[2]);
            let mut open = String::from("<span");
            if !classes.is_empty() {
                open.push_str(&format!(" class=\"{}\"", classes.join(" ")));
            }
            if !style.is_empty() {
                open.push_str(&format!(" style=\"{}\"", esc(&style)));
            }
            if !id.is_empty() {
                open.push_str(&format!(" id=\"{}\"", esc(&id)));
            }
            open.push('>');
            let a = st.keep(open);
            let b = st.keep("</span>".into());
            format!("{a}{}{b}", &c[1])
        }).into_owned();
        if s == before {
            break;
        }
    }
    let s = LINK.replace_all(&s, |c: &regex::Captures| {
        let a = st.keep(format!("<a href=\"{}\">", esc(&c[2])));
        let b = st.keep("</a>".into());
        format!("{a}{}{b}", &c[1])
    }).into_owned();
    let s = AUTO.replace_all(&s, |c: &regex::Captures| {
        let url = &c[2];
        format!("{}{}", &c[1], st.keep(format!("<a href=\"{}\">{}</a>", esc(url), url)))
    }).into_owned();
    let wrap = |st: &mut Store, open: &str, close: &str, inner: &str| format!("{}{inner}{}", st.keep(open.into()), st.keep(close.into()));
    let s = fancy_replace(&B3, &s, |c| wrap(&mut st, "<strong><em>", "</em></strong>", &c[1]));
    let s = fancy_replace(&B2, &s, |c| wrap(&mut st, "<strong>", "</strong>", &c[1]));
    let s = fancy_replace(&U2, &s, |c| format!("{}{}", &c[1], wrap(&mut st, "<strong>", "</strong>", &c[2])));
    let s = fancy_replace(&I1, &s, |c| wrap(&mut st, "<em>", "</em>", &c[1]));
    let s = fancy_replace(&U1, &s, |c| format!("{}{}", &c[1], wrap(&mut st, "<em>", "</em>", &c[2])));
    let s = fancy_replace(&STRIKE, &s, |c| wrap(&mut st, "<s>", "</s>", &c[1]));
    let mut s = fancy_replace(&MARK, &s, |c| wrap(&mut st, "<mark>", "</mark>", &c[1]));
    if h.typography() {
        s = smart_typography(&s);
    }
    for _ in 0..6 {
        if !SLOT.is_match(&s) {
            break;
        }
        s = SLOT.replace_all(&s, |c: &regex::Captures| st.0.get(c[1].parse::<usize>().unwrap_or(usize::MAX)).cloned().unwrap_or_default()).into_owned();
    }
    s
}

// ── blocks ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    P { text: String, line: Option<usize> },
    H { level: usize, text: String, line: Option<usize> },
    Scene { line: Option<usize> },
    Fig { e: Embed, line: Option<usize> },
    Callout { kind: String, title: String, blocks: Vec<Block>, line: Option<usize> },
    Quote { blocks: Vec<Block>, line: Option<usize> },
    List { ordered: bool, start: u64, items: Vec<Vec<Block>>, line: Option<usize> },
    Table { head: Vec<String>, rows: Vec<Vec<String>>, line: Option<usize> },
    Code { lang: String, text: String, line: Option<usize> },
    Html { html: String, line: Option<usize> },
    /// A composed page (`scriptorium-pagina`).
    Page { json: Json, line: Option<usize> },
    PageBreak { recto: bool, line: Option<usize> },
}

impl Block {
    pub fn line(&self) -> Option<usize> {
        match self {
            Block::P { line, .. } | Block::H { line, .. } | Block::Scene { line } | Block::Fig { line, .. } | Block::Callout { line, .. } | Block::Quote { line, .. } | Block::List { line, .. } | Block::Table { line, .. } | Block::Code { line, .. } | Block::Html { line, .. } | Block::Page { line, .. } | Block::PageBreak { line, .. } => *line,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Block::P { .. } => "p",
            Block::H { .. } => "h",
            Block::Scene { .. } => "scene",
            Block::Fig { .. } => "fig",
            Block::Callout { .. } => "callout",
            Block::Quote { .. } => "quote",
            Block::List { .. } => "list",
            Block::Table { .. } => "table",
            Block::Code { .. } => "code",
            Block::Html { .. } => "html",
            Block::Page { .. } => "page",
            Block::PageBreak { .. } => "pagebreak",
        }
    }
}

pub struct Parsed {
    pub blocks: Vec<Block>,
    pub footnotes: HashMap<String, String>,
    /// Lines taken by the frontmatter.
    pub fm_lines: usize,
}

/// `***`, `- - -`, `___`…
pub fn is_scene_line(l: &str) -> bool {
    let t: String = l.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && t.chars().all(|c| c == '-') || t.len() >= 3 && t.chars().all(|c| c == '*') || t.len() >= 3 && t.chars().all(|c| c == '_')
}

static FENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(`{3,}|~{3,})\s*([^\s`]*)\s*(.*)$").unwrap());
static HEADING: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(#{1,6})\s+(.*?)\s*#*\s*$").unwrap());
static FOOTDEF: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[\^([^\]]+)\]:\s?(.*)$").unwrap());
static LIST_ITEM: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\s*)([-*+]|\d+[.)])\s+(.*)$").unwrap());
static CALLOUT_HEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[!([^\]]+)\]([+-]?)\s*(.*)$").unwrap());
static HTML_START: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*<(div|figure|section|aside|table|center|p|svg|details|iframe|h\d|hr|img|blockquote)[\s>/]").unwrap());
static EMBED_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^!\[\[([^\]]+)\]\]$").unwrap());
static IMAGE_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^!\[([^\]]*)\]\(([^)\s]+)\)$").unwrap());

fn is_block_start(l: &str) -> bool {
    static FENCE_START: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(`{3,}|~{3,})").unwrap());
    static H: Lazy<Regex> = Lazy::new(|| Regex::new(r"^#{1,6}\s").unwrap());
    static Q: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*>").unwrap());
    static EMB: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*!\[\[[^\]]+\]\]\s*$").unwrap());
    static FD: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[\^[^\]]+\]:").unwrap());
    FENCE_START.is_match(l) || H.is_match(l) || Q.is_match(l) || is_scene_line(l) || LIST_ITEM.is_match(l) || EMB.is_match(l) || FD.is_match(l)
}

fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// Parse a chapter (frontmatter skipped; `%% %%` and `<!-- -->` dropped).
pub fn md_blocks(src: &str, soft_break: &str) -> Parsed {
    let full = src.replace("\r\n", "\n").replace('\r', "\n");
    let body = super::strip_frontmatter(&full);
    let fm_lines = full[..full.len() - body.len()].matches('\n').count();
    // comments go, line breaks stay (so source lines still match)
    static COMMENT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)%%.*?%%|<!--.*?-->").unwrap());
    static FENCED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?ms)^```.*?^```").unwrap());
    let mut text = String::new();
    let mut last = 0;
    for m in FENCED.find_iter(body) {
        text.push_str(&COMMENT.replace_all(&body[last..m.start()], |c: &regex::Captures| c[0].chars().filter(|&c| c == '\n').collect::<String>()));
        text.push_str(m.as_str());
        last = m.end();
    }
    text.push_str(&COMMENT.replace_all(&body[last..], |c: &regex::Captures| c[0].chars().filter(|&c| c == '\n').collect::<String>()));
    let lines: Vec<&str> = text.split('\n').collect();
    let mut footnotes = HashMap::new();
    let blocks = parse_lines(&lines, &mut footnotes, soft_break, true, None);
    Parsed { blocks, footnotes, fm_lines }
}

fn parse_lines(lines: &[&str], footnotes: &mut HashMap<String, String>, soft_break: &str, top: bool, soft: Option<&str>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        let at = if top { Some(i) } else { None };
        if let Some(m) = FENCE.captures(line) {
            let fence = m[1].to_string();
            let lang = m[2].to_lowercase();
            let mut body = Vec::new();
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim().starts_with(&fence) {
                body.push(lines[j]);
                j += 1;
            }
            i = j + 1;
            if lang == "scriptorium-pagina" {
                if let Ok(json) = serde_json::from_str::<Json>(&body.join("\n")) {
                    out.push(Block::Page { json, line: at });
                }
            } else if lang == "playlist" || lang.starts_with("scriptorium-") || lang == "dataview" || lang == "dataviewjs" || lang == "tasks" {
                // tool blocks stay out of the book
            } else {
                out.push(Block::Code { lang, text: body.join("\n"), line: at });
            }
            continue;
        }
        if let Some(m) = HEADING.captures(line) {
            out.push(Block::H { level: m[1].len(), text: m[2].to_string(), line: at });
            i += 1;
            continue;
        }
        if is_scene_line(line) {
            out.push(Block::Scene { line: at });
            i += 1;
            continue;
        }
        if let Some(m) = FOOTDEF.captures(line) {
            let mut txt = vec![m[2].to_string()];
            let mut j = i + 1;
            while j < lines.len() && (lines[j].starts_with("  ") || lines[j].starts_with('\t')) && !blank(lines[j]) {
                txt.push(lines[j].trim().to_string());
                j += 1;
            }
            footnotes.insert(m[1].to_string(), txt.join(" "));
            i = j;
            continue;
        }
        if line.trim_start().starts_with('>') {
            let mut inner: Vec<String> = Vec::new();
            let mut j = i;
            while j < lines.len() && lines[j].trim_start().starts_with('>') {
                let l = lines[j].trim_start().trim_start_matches('>');
                inner.push(l.strip_prefix(' ').unwrap_or(l).to_string());
                j += 1;
            }
            i = j;
            let refs: Vec<&str> = inner.iter().map(String::as_str).collect();
            if let Some(c) = CALLOUT_HEAD.captures(refs[0]) {
                let kind = ascii_slug(c[1].trim());
                if matches!(kind.as_str(), "pagina" | "quebra" | "pagina-impar" | "quebra-de-pagina") {
                    out.push(Block::PageBreak { recto: kind == "pagina-impar", line: at });
                } else {
                    let blocks = parse_lines(&refs[1..], footnotes, soft_break, false, Some("linha"));
                    out.push(Block::Callout { kind, title: c[3].to_string(), blocks, line: at });
                }
            } else {
                let blocks = parse_lines(&refs, footnotes, soft_break, false, Some("linha"));
                out.push(Block::Quote { blocks, line: at });
            }
            continue;
        }
        let is_row = |l: &str| {
            let t = l.trim();
            t.starts_with('|') && t.ends_with('|') && t.len() > 1
        };
        static SEP: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\|?\s*:?-{2,}").unwrap());
        if is_row(line) && i + 1 < lines.len() && SEP.is_match(lines[i + 1]) {
            let split = |l: &str| -> Vec<String> {
                let t = l.trim();
                let t = t.strip_prefix('|').unwrap_or(t);
                let t = t.strip_suffix('|').unwrap_or(t);
                t.split('|').map(|c| c.trim().to_string()).collect()
            };
            let head = split(line);
            let mut rows = Vec::new();
            let mut j = i + 2;
            while j < lines.len() && is_row(lines[j]) {
                rows.push(split(lines[j]));
                j += 1;
            }
            out.push(Block::Table { head, rows, line: at });
            i = j;
            continue;
        }
        if let Some(m) = LIST_ITEM.captures(line) {
            let base = m[1].len();
            let ordered = m[2].chars().next().is_some_and(|c| c.is_ascii_digit());
            let start = if ordered { m[2].trim_end_matches(['.', ')']).parse().unwrap_or(1) } else { 1 };
            let mut items: Vec<Vec<String>> = Vec::new();
            let mut j = i;
            static TASK: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[[ xX]\]\s*").unwrap());
            while j < lines.len() {
                let l = lines[j];
                if let Some(mm) = LIST_ITEM.captures(l) {
                    if mm[1].len() <= base + 1 && mm[2].chars().next().is_some_and(|c| c.is_ascii_digit()) == ordered {
                        items.push(vec![TASK.replace(&mm[3], "").into_owned()]);
                        j += 1;
                        continue;
                    }
                }
                let indented = |s: &str| (s.starts_with("  ") || s.starts_with('\t')) && !blank(s);
                if blank(l) {
                    if j + 1 < lines.len() && indented(lines[j + 1]) && !items.is_empty() {
                        items.last_mut().unwrap().push(String::new());
                        j += 1;
                        continue;
                    }
                    break;
                }
                if indented(l) && !items.is_empty() {
                    let t = l.strip_prefix('\t').unwrap_or(l);
                    let n = t.len() - t.trim_start_matches(' ').len();
                    items.last_mut().unwrap().push(t[n.min(4)..].to_string());
                    j += 1;
                    continue;
                }
                if !is_block_start(l) && !items.is_empty() {
                    items.last_mut().unwrap().push(l.to_string());
                    j += 1;
                    continue;
                }
                break;
            }
            i = j;
            let items = items.iter().map(|it| parse_lines(&it.iter().map(String::as_str).collect::<Vec<_>>(), footnotes, soft_break, false, Some("linha"))).collect();
            out.push(Block::List { ordered, start, items, line: at });
            continue;
        }
        if HTML_START.is_match(line) {
            let mut html = Vec::new();
            let mut j = i;
            while j < lines.len() && !blank(lines[j]) {
                html.push(lines[j]);
                j += 1;
            }
            i = j;
            out.push(Block::Html { html: html.join("\n"), line: at });
            continue;
        }
        if let Some(m) = EMBED_LINE.captures(line.trim()) {
            let target = m[1].split('|').next().unwrap_or("").split('#').next().unwrap_or("");
            if is_image_link(target) {
                out.push(Block::Fig { e: parse_embed(&m[1]), line: at });
                i += 1;
                continue;
            }
        }
        if let Some(m) = IMAGE_LINE.captures(line.trim()) {
            if !m[2].starts_with("http") {
                let src = percent_encoding::percent_decode_str(&m[2]).decode_utf8_lossy().into_owned();
                let spec = if m[1].is_empty() { src } else { format!("{src}|{}", &m[1]) };
                out.push(Block::Fig { e: parse_embed(&spec), line: at });
                i += 1;
                continue;
            }
        }
        // a paragraph
        let mut para = vec![line];
        let mut j = i + 1;
        while j < lines.len() && !blank(lines[j]) && !is_block_start(lines[j]) {
            para.push(lines[j]);
            j += 1;
        }
        i = j;
        let mode = soft.unwrap_or(soft_break);
        if mode == "paragrafo" {
            for (k, p) in para.iter().enumerate() {
                out.push(Block::P { text: p.trim().to_string(), line: at.map(|a| a + k) });
            }
        } else {
            out.push(Block::P { text: para.iter().map(|x| x.trim()).collect::<Vec<_>>().join("\n"), line: at });
        }
    }
    out
}

/// What block rendering asks of the book being built.
pub trait BlockHost: InlineHost {
    fn ornament(&self) -> String {
        "⁂".into()
    }
    fn figure(&mut self, _e: &Embed) -> String {
        String::new()
    }
    fn composed(&mut self, _json: &Json) -> String {
        String::new()
    }
    /// Source line numbers as `data-l` (for clicking from the preview to the text).
    fn line_numbers(&self) -> bool {
        false
    }
}

impl BlockHost for Plain {}

/// Blocks → one HTML string per top-level block.
pub fn render_blocks(blocks: &[Block], h: &mut dyn BlockHost, nested: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut prev = "start";
    for b in blocks {
        let inl = |h: &mut dyn BlockHost, t: &str| md_inline(t, h).replace('\n', "<br/>");
        let html = match b {
            Block::P { text, .. } => {
                let first = matches!(prev, "start" | "h" | "scene" | "fig-block" | "pagebreak" | "page");
                format!("<p{}>{}</p>", if first { " class=\"sc-first\"" } else { "" }, inl(h, text))
            }
            Block::H { level, text, .. } => {
                let lv = (level + 1).min(6);
                format!("<h{lv} class=\"sc-h\">{}</h{lv}>", inl(h, text))
            }
            Block::Scene { .. } => format!("<div class=\"sc-scene\" role=\"separator\">{}</div>", esc(&h.ornament())),
            Block::Fig { e, .. } => h.figure(e),
            Block::Callout { kind, title, blocks, .. } => {
                let inner = render_blocks(blocks, h, true).join("");
                let title = if title.is_empty() { String::new() } else { format!("<div class=\"sc-callout-title\">{}</div>", inl(h, title)) };
                format!("<aside class=\"sc-callout sc-c-{}\">{title}<div class=\"sc-callout-body\">{inner}</div></aside>", esc(kind))
            }
            Block::Quote { blocks, .. } => format!("<blockquote>{}</blockquote>", render_blocks(blocks, h, true).join("")),
            Block::List { ordered, start, items, .. } => {
                let tag = if *ordered { "ol" } else { "ul" };
                let mut s = format!("<{tag}{}>", if *ordered && *start != 1 { format!(" start=\"{start}\"") } else { String::new() });
                for it in items {
                    match it.as_slice() {
                        [Block::P { text, .. }] => s.push_str(&format!("<li>{}</li>", inl(h, text))),
                        _ => s.push_str(&format!("<li>{}</li>", render_blocks(it, h, true).join(""))),
                    }
                }
                s.push_str(&format!("</{tag}>"));
                s
            }
            Block::Table { head, rows, .. } => {
                let mut s = String::from("<table class=\"sc-table\"><thead><tr>");
                for c in head {
                    s.push_str(&format!("<th>{}</th>", inl(h, c)));
                }
                s.push_str("</tr></thead><tbody>");
                for r in rows {
                    s.push_str("<tr>");
                    for c in r {
                        s.push_str(&format!("<td>{}</td>", inl(h, c)));
                    }
                    s.push_str("</tr>");
                }
                s.push_str("</tbody></table>");
                s
            }
            Block::Code { text, .. } => format!("<pre class=\"sc-code\"><code>{}</code></pre>", esc_text(text)),
            Block::Html { html, .. } => html.clone(),
            Block::Page { json, .. } => h.composed(json),
            Block::PageBreak { recto, .. } => format!("<div class=\"sc-pagebreak\"{}></div>", if *recto { " data-recto=\"1\"" } else { "" }),
        };
        let html = match (b.line(), nested || !h.line_numbers()) {
            (Some(l), false) if !html.is_empty() => {
                static OPEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^<([a-zA-Z][\w-]*)").unwrap());
                OPEN.replace(&html, format!("<$1 data-l=\"{l}\"").as_str()).into_owned()
            }
            _ => html,
        };
        if !html.is_empty() {
            out.push(html);
        }
        prev = match b {
            Block::Fig { e, .. } if matches!(e.pos.as_str(), "pagina" | "sangria" | "dupla" | "centro" | "") => "fig-block",
            other => other.kind(),
        };
    }
    out
}

/// Footnotes of one chapter: references numbered in order of appearance.
pub struct Footnotes {
    pub chapter: String,
    pub defs: HashMap<String, String>,
    pub order: Vec<String>,
    inline: usize,
}

impl Footnotes {
    pub fn new(chapter: &str, defs: HashMap<String, String>) -> Footnotes {
        Footnotes { chapter: chapter.into(), defs, order: Vec::new(), inline: 0 }
    }

    pub fn reference(&mut self, id: &str) -> String {
        let n = match self.order.iter().position(|x| x == id) {
            Some(i) => i + 1,
            None => {
                self.order.push(id.to_string());
                self.order.len()
            }
        };
        let c = &self.chapter;
        format!("<sup class=\"sc-fnref\"><a href=\"#fn-{c}-{n}\" id=\"fnref-{c}-{n}\" role=\"doc-noteref\">{n}</a></sup>")
    }

    pub fn inline_note(&mut self, text: &str) -> String {
        let id = format!("__inline{}", self.inline);
        self.inline += 1;
        self.defs.insert(id.clone(), text.to_string());
        self.reference(&id)
    }

    pub fn html(&self, typography: bool) -> String {
        if self.order.is_empty() {
            return String::new();
        }
        let c = &self.chapter;
        let mut s = String::from("<section class=\"sc-footnotes\" role=\"doc-endnotes\"><ol>");
        for (i, id) in self.order.iter().enumerate() {
            let n = i + 1;
            let text = md_inline(self.defs.get(id).map(String::as_str).unwrap_or(""), &mut Plain { typography });
            s.push_str(&format!("<li id=\"fn-{c}-{n}\" role=\"doc-endnote\"><p>{text} <a href=\"#fnref-{c}-{n}\" class=\"sc-fnback\" role=\"doc-backlink\">↩</a></p></li>"));
        }
        s.push_str("</ol></section>");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inl(s: &str) -> String {
        md_inline(s, &mut Plain { typography: true })
    }

    #[test]
    fn inline_markup() {
        assert_eq!(inl("um **forte** e *leve*"), "um <strong>forte</strong> e <em>leve</em>");
        assert_eq!(inl("[grito]{.grito cor=#c00}"), "<span class=\"sc-s-grito\" style=\"color:#c00\">grito</span>");
        assert_eq!(inl("\"Olá\" -- disse..."), "“Olá” — disse…");
        assert_eq!(inl("`a*b*c`"), "<code>a*b*c</code>");
        assert_eq!(inl("[[Nota|alias]] e [[pasta/Outra]]"), "alias e Outra");
        assert_eq!(inl("snake_case_word"), "snake_case_word");
        assert_eq!(inl("a < b & c"), "a &lt; b &amp; c");
    }

    #[test]
    fn blocks() {
        let src = "---\ntitulo: x\n---\nPrimeiro.\nSegundo.\n\n***\n\n> [!carta] Querida\n> Texto da carta.\n\n- um\n- dois\n\n![[arte.png|direita 40% contorno \"Legenda\"]]\n\n> [!pagina]\n\nFim[^1].\n\n[^1]: Uma nota.";
        let p = md_blocks(src, "paragrafo");
        let kinds: Vec<&str> = p.blocks.iter().map(|b| b.kind()).collect();
        assert_eq!(kinds, vec!["p", "p", "scene", "callout", "list", "fig", "pagebreak", "p"]);
        assert_eq!(p.footnotes["1"], "Uma nota.");
        assert_eq!(p.fm_lines, 3);
        match &p.blocks[5] {
            Block::Fig { e, .. } => {
                assert_eq!(e.pos, "direita");
                assert_eq!(e.width, "40%");
                assert!(e.contour);
                assert_eq!(e.caption, "Legenda");
            }
            _ => unreachable!(),
        }
        let html = render_blocks(&p.blocks, &mut Plain { typography: true }, false);
        assert_eq!(html[0], "<p class=\"sc-first\">Primeiro.</p>");
        assert_eq!(html[1], "<p>Segundo.</p>");
        assert!(html[3].starts_with("<aside class=\"sc-callout sc-c-carta\"><div class=\"sc-callout-title\">Querida</div>"));
    }
}
