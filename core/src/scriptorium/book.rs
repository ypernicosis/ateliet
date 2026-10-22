//! Books, chapters and characters, read from the vault index.
//!
//! A book is a note with `scriptorium: livro`; its chapters are the notes in
//! its manuscript folder (`pasta_manuscrito`, "Manuscrito" by default),
//! ordered by `ordem` and then by path; characters are `scriptorium:
//! personagem` notes inside the book's folder or linking to it with `livro`.

use super::{link_text, note_with, pad2, safe_name, to_list};
use crate::dataview::index::{Index, Page};
use once_cell::sync::Lazy;
use serde_json::{json, Value as Json};
use std::cmp::Ordering;
use std::path::Path;

pub const STATUS: [(&str, &str, &str); 4] = [("ideia", "Ideia", "#8b8fa3"), ("rascunho", "Rascunho", "#c9a45c"), ("revisao", "Revisão", "#4f8fbf"), ("final", "Final", "#4f9f6f")];

pub fn status_label(k: &str) -> &'static str {
    STATUS.iter().find(|s| s.0 == k).map(|s| s.1).unwrap_or("Rascunho")
}

pub fn status_color(k: &str) -> &'static str {
    STATUS.iter().find(|s| s.0 == k).map(|s| s.2).unwrap_or("#c9a45c")
}

pub const CHAPTER_TYPES: [(&str, &str); 4] = [("capitulo", "Capítulo"), ("prologo", "Prólogo"), ("interludio", "Interlúdio"), ("epilogo", "Epílogo")];

/// Trim sizes: key, label, width and height in mm.
pub const TRIM_PRESETS: [(&str, &str, f64, f64); 13] = [
    ("14x21", "14 × 21 cm (padrão BR)", 140.0, 210.0),
    ("16x23", "16 × 23 cm", 160.0, 230.0),
    ("13x20", "13 × 20 cm (bolso grande)", 130.0, 200.0),
    ("12x18", "12 × 18 cm (bolso)", 120.0, 180.0),
    ("A5", "A5 (14,8 × 21 cm)", 148.0, 210.0),
    ("6x9", "6 × 9 in (KDP)", 152.4, 228.6),
    ("5.5x8.5", "5,5 × 8,5 in (KDP)", 139.7, 215.9),
    ("5x8", "5 × 8 in (KDP)", 127.0, 203.2),
    ("A4", "A4", 210.0, 297.0),
    ("hq", "Formato americano HQ (17 × 26 cm)", 170.0, 260.0),
    ("manga", "Tankōbon mangá (13 × 18 cm)", 128.0, 182.0),
    ("quadrado", "Quadrado (20 × 20 cm)", 200.0, 200.0),
    ("custom", "Personalizado", 140.0, 210.0),
];

/// Papers: key, label, thickness per page (mm).
pub const PAPERS: [(&str, &str, f64); 8] = [
    ("polen80", "Pólen Soft 80 g", 0.0605),
    ("polen70", "Pólen Soft 70 g", 0.053),
    ("offset75", "Offset 75 g", 0.05),
    ("offset90", "Offset 90 g", 0.06),
    ("couche115", "Couché 115 g (HQ colorida)", 0.05),
    ("kdp-branco", "KDP papel branco", 0.0572),
    ("kdp-creme", "KDP papel creme", 0.0635),
    ("kdp-cor", "KDP colorido premium", 0.0596),
];

/// Spine width (mm): paper × pages + cover board.
pub fn spine_mm(pages: usize, paper: &str, hard: bool) -> f64 {
    let per = PAPERS.iter().find(|p| p.0 == paper).map(|p| p.2).unwrap_or(0.0605);
    ((pages as f64 * per + if hard { 4.0 } else { 0.6 }) * 10.0).round().max(20.0) / 10.0
}

pub static DEFAULT_LAYOUT: Lazy<Json> = Lazy::new(|| {
    serde_json::from_str(r##"{
        "formato": "14x21", "largura": 140, "altura": 210,
        "margens": { "interna": 22, "externa": 16, "superior": 18, "inferior": 20 },
        "sangria": 3,
        "fonte": "Iowan Old Style", "fonteTitulos": "Cormorant", "corpo": 10.5, "entrelinha": 1.45, "recuo": 1.3,
        "justificar": true, "hifenizar": true, "fimCapitulo": true, "capitular": true, "versaleteInicio": true,
        "aberturaImpar": true, "descida": 28, "numeracaoCapitulo": "extenso", "ornamentoAbertura": "❦",
        "cabecalho": true, "folio": "externo", "ornamento": "⁂",
        "paginasIniciais": { "falsaFolha": true, "rosto": true, "creditos": true, "dedicatoria": true, "sumario": true },
        "papel": "polen80",
        "corTexto": "#1a1410",
        "espacoParagrafo": "auto",
        "capitularLinhas": 3, "capitularEstilo": "afundada", "capitularFonte": "", "capitularCor": "", "versaleteAlcance": "linha",
        "rotuloPrefixo": "Capítulo", "rotuloPosicao": "acima", "rotuloTamanho": 0.95, "rotuloCaixa": "versalete",
        "tituloTamanho": 2, "tituloPeso": 500, "tituloCaixa": "normal", "tituloAlinhamento": "centro", "tituloItalico": false, "tituloEspacamento": 0,
        "fio": "nenhum", "fioLugar": "abaixo",
        "cabecalhoVerso": "livro", "cabecalhoRecto": "capitulo", "cabecalhoEstilo": "versalete", "cabecalhoFonte": "texto",
        "folioPosicao": "pe", "folioFormato": "{n}", "folioAbertura": true, "folioAlgarismos": "antigo",
        "corTitulos": "", "corDestaque": "", "corPapel": "#ffffff",
        "rotuloParte": "Parte",
        "cssExtra": ""
    }"##).unwrap()
});

/// Ready-made looks: key, label, description, settings.
pub static LAYOUT_PRESETS: Lazy<Vec<(&'static str, &'static str, &'static str, Json)>> = Lazy::new(|| {
    vec![
        ("classico", "Romance clássico", "Serifa elegante, capitular e versalete", json!({ "fonte": "Iowan Old Style", "fonteTitulos": "Cormorant", "corpo": 10.5, "entrelinha": 1.45, "recuo": 1.3, "justificar": true, "capitular": true, "versaleteInicio": true, "numeracaoCapitulo": "extenso", "ornamento": "⁂", "ornamentoAbertura": "❦", "descida": 28, "cabecalho": true })),
        ("epico", "Fantasia épica", "Títulos lapidares, numeração romana", json!({ "fonte": "Crimson Text", "fonteTitulos": "Cinzel", "corpo": 11, "entrelinha": 1.42, "recuo": 1.2, "justificar": true, "capitular": true, "versaleteInicio": true, "numeracaoCapitulo": "romano", "ornamento": "❦", "ornamentoAbertura": "✦", "descida": 32, "cabecalho": true })),
        ("moderno", "Contemporâneo", "Leve, arejado, sem capitular", json!({ "fonte": "Lora", "fonteTitulos": "Playfair Display", "corpo": 10.5, "entrelinha": 1.5, "recuo": 1.2, "justificar": true, "capitular": false, "versaleteInicio": false, "numeracaoCapitulo": "numero", "ornamento": "✦ ✦ ✦", "ornamentoAbertura": "", "descida": 22, "cabecalho": true })),
        ("ilustrado", "Illustra-novel", "Formato maior, respiro para arte e HQ", json!({ "formato": "16x23", "largura": 160, "altura": 230, "fonte": "Libre Baskerville", "fonteTitulos": "Cinzel Decorative", "corpo": 10.25, "entrelinha": 1.55, "recuo": 1.2, "justificar": true, "capitular": true, "versaleteInicio": true, "numeracaoCapitulo": "extenso", "ornamento": "— ✧ —", "ornamentoAbertura": "✧", "descida": 24, "margens": { "interna": 20, "externa": 15, "superior": 16, "inferior": 18 } })),
        ("manga", "Mangá / HQ", "Tankōbon, margens curtas, páginas compostas", json!({ "formato": "manga", "largura": 128, "altura": 182, "fonte": "Iowan Old Style", "fonteTitulos": "Cinzel", "corpo": 9.5, "entrelinha": 1.4, "capitular": false, "versaleteInicio": false, "aberturaImpar": false, "cabecalho": false, "numeracaoCapitulo": "so-numero", "margens": { "interna": 12, "externa": 9, "superior": 10, "inferior": 12 } })),
        ("poesia", "Poesia", "Alinhado à esquerda, sem hifenização", json!({ "fonte": "Cormorant", "fonteTitulos": "Cormorant", "corpo": 12, "entrelinha": 1.5, "recuo": 0, "justificar": false, "hifenizar": false, "capitular": false, "versaleteInicio": false, "numeracaoCapitulo": "nenhum", "ornamento": "❦", "ornamentoAbertura": "", "descida": 30 })),
    ]
});

#[derive(Clone, Debug)]
pub struct Book {
    pub path: String,
    pub folder: String,
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub serie: String,
    pub volume: String,
    pub goal: i64,
    pub deadline: String,
    pub color: String,
    pub manuscript: String,
    pub created: i64,
    pub modified: i64,
    pub fm: Json,
}

#[derive(Clone, Debug)]
pub struct Chapter {
    pub path: String,
    pub tipo: String,
    pub title: String,
    pub status: String,
    pub part: String,
    pub pov: String,
    pub goal: i64,
    pub numbered: bool,
    pub include: bool,
    pub order: Option<f64>,
    pub mtime: i64,
}

pub fn kind(p: &Page) -> &str {
    p.yaml.get("scriptorium").and_then(|v| v.as_str()).unwrap_or("")
}

fn s(fm: &Json, k: &str) -> String {
    match fm.get(k) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn num(fm: &Json, k: &str) -> Option<f64> {
    match fm.get(k)? {
        Json::Number(n) => n.as_f64(),
        Json::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn basename(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(b, _)| b).unwrap_or(name)
}

fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn join(a: &str, b: &str) -> String {
    if a.is_empty() { b.to_string() } else { format!("{a}/{b}") }
}

pub fn book_of(p: &Page, default_author: &str) -> Book {
    let fm = &p.yaml;
    let folder = dirname(&p.path).to_string();
    let manuscript = join(&folder, &Some(s(fm, "pasta_manuscrito")).filter(|x| !x.is_empty()).unwrap_or_else(|| "Manuscrito".into()));
    Book {
        path: p.path.clone(),
        title: Some(s(fm, "titulo")).filter(|x| !x.is_empty()).unwrap_or_else(|| basename(&p.path).to_string()),
        subtitle: s(fm, "subtitulo"),
        author: Some(s(fm, "autor")).filter(|x| !x.is_empty()).unwrap_or_else(|| default_author.to_string()),
        serie: s(fm, "serie"),
        volume: s(fm, "volume"),
        goal: num(fm, "meta_palavras").unwrap_or(0.0) as i64,
        deadline: s(fm, "prazo"),
        color: Some(s(fm, "cor")).filter(|x| !x.is_empty()).unwrap_or_else(|| "#2f6b6b".into()),
        manuscript,
        folder,
        created: p.ctime,
        modified: p.mtime,
        fm: fm.clone(),
    }
}

/// Series together (single books last), volumes in order, then title.
fn book_order(a: &Book, b: &Book) -> Ordering {
    let (sa, sb) = (a.serie.trim(), b.serie.trim());
    if sa.is_empty() != sb.is_empty() {
        return if sa.is_empty() { Ordering::Greater } else { Ordering::Less };
    }
    if sa != sb {
        return crate::text::natural_cmp(sa, sb);
    }
    let (va, vb) = (a.volume.parse::<f64>().ok(), b.volume.parse::<f64>().ok());
    match (va, vb) {
        (Some(x), Some(y)) if x != y => return x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => return Ordering::Less,
        (None, Some(_)) => return Ordering::Greater,
        _ => {}
    }
    crate::text::natural_cmp(&a.title, &b.title)
}

pub fn books(ix: &Index, default_author: &str) -> Vec<Book> {
    let mut out: Vec<Book> = ix.page_paths().filter_map(|p| ix.page(p)).filter(|p| kind(p) == "livro").map(|p| book_of(p, default_author)).collect();
    out.sort_by(book_order);
    out
}

pub fn book_by_path(ix: &Index, path: &str, default_author: &str) -> Option<Book> {
    ix.page(path).filter(|p| kind(p) == "livro").map(|p| book_of(p, default_author))
}

/// The book a note belongs to: the book itself, the deepest book folder
/// holding it, or the book its `livro` field links to.
pub fn book_for(ix: &Index, path: &str, default_author: &str) -> Option<Book> {
    let page = ix.page(path)?;
    if kind(page) == "livro" {
        return Some(book_of(page, default_author));
    }
    let mut best: Option<Book> = None;
    for b in books(ix, default_author) {
        if !b.folder.is_empty() && path.starts_with(&format!("{}/", b.folder)) && best.as_ref().is_none_or(|x| b.folder.len() > x.folder.len()) {
            best = Some(b);
        }
    }
    if best.is_some() {
        return best;
    }
    let l = link_text(page.yaml.get("livro").unwrap_or(&Json::Null));
    if !l.is_empty() {
        if let Some(t) = ix.resolve(&l, path) {
            return book_by_path(ix, &t, default_author);
        }
    }
    None
}

pub fn is_chapter(ix: &Index, path: &str) -> bool {
    let Some(p) = ix.page(path) else { return false };
    match kind(p) {
        "capitulo" => true,
        "" => book_for(ix, path, "").is_some_and(|b| path.starts_with(&format!("{}/", b.manuscript))),
        _ => false,
    }
}

pub fn chapter_info(p: &Page) -> Chapter {
    let fm = &p.yaml;
    let tipo = Some(s(fm, "tipo").to_lowercase()).filter(|x| !x.is_empty()).unwrap_or_else(|| "capitulo".into());
    static NUM_PREFIX: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"^\d+\s*[—\-–.]\s*").unwrap());
    let title = Some(s(fm, "titulo")).filter(|x| !x.is_empty()).unwrap_or_else(|| NUM_PREFIX.replace(basename(&p.path), "").into_owned());
    let status = s(fm, "status");
    Chapter {
        path: p.path.clone(),
        numbered: tipo == "capitulo" && fm.get("numerar") != Some(&Json::Bool(false)),
        tipo,
        title,
        status: if super::book::STATUS.iter().any(|x| x.0 == status) { status } else { "rascunho".into() },
        part: s(fm, "parte"),
        pov: s(fm, "pov"),
        goal: num(fm, "meta").unwrap_or(0.0) as i64,
        include: fm.get("incluir") != Some(&Json::Bool(false)),
        order: num(fm, "ordem"),
        mtime: p.mtime,
    }
}

pub fn chapters(ix: &Index, book: &Book) -> Vec<Chapter> {
    let prefix = format!("{}/", book.manuscript);
    let mut out: Vec<Chapter> = ix
        .page_paths()
        .filter(|p| p.starts_with(&prefix))
        .filter_map(|p| ix.page(p))
        .filter(|p| matches!(kind(p), "" | "capitulo"))
        .map(chapter_info)
        .collect();
    out.sort_by(|a, b| match (a.order, b.order) {
        (Some(x), Some(y)) if x != y => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        _ => crate::text::natural_cmp(&a.path, &b.path),
    });
    out
}

pub fn characters(ix: &Index, book: &Book) -> Vec<String> {
    let prefix = format!("{}/", book.folder);
    let mut out: Vec<String> = ix
        .page_paths()
        .filter_map(|p| ix.page(p))
        .filter(|p| kind(p) == "personagem")
        .filter(|p| {
            if !book.folder.is_empty() && p.path.starts_with(&prefix) {
                return true;
            }
            let mut l = to_list(p.yaml.get("livro").unwrap_or(&Json::Null));
            l.extend(to_list(p.yaml.get("livros").unwrap_or(&Json::Null)));
            l.iter().any(|x| ix.resolve(&link_text(x), &p.path).as_deref() == Some(book.path.as_str()))
        })
        .map(|p| p.path.clone())
        .collect();
    out.sort_by(|a, b| crate::text::natural_cmp(basename(a), basename(b)));
    out
}

pub fn notes_of_kind(ix: &Index, book: &Book, k: &str) -> Vec<String> {
    let prefix = format!("{}/", book.folder);
    let mut out: Vec<String> = ix.page_paths().filter(|p| p.starts_with(&prefix)).filter_map(|p| ix.page(p)).filter(|p| kind(p) == k).map(|p| p.path.clone()).collect();
    out.sort();
    out
}

/// The layout settings of a book: defaults with the book's `diagramacao` over them.
pub fn layout(book: &Book) -> Json {
    let mut cfg = DEFAULT_LAYOUT.clone();
    if let Some(Json::Object(saved)) = book.fm.get("diagramacao") {
        for (k, v) in saved {
            match (cfg.get_mut(k), v) {
                (Some(Json::Object(dst)), Json::Object(src)) => {
                    for (a, b) in src {
                        dst.insert(a.clone(), b.clone());
                    }
                }
                _ => {
                    cfg[k] = v.clone();
                }
            }
        }
    }
    let f = cfg["formato"].as_str().unwrap_or("14x21").to_string();
    if let Some(p) = TRIM_PRESETS.iter().find(|p| p.0 == f && f != "custom") {
        cfg["largura"] = json!(p.2);
        cfg["altura"] = json!(p.3);
    }
    cfg
}

/// Only what differs from the defaults (what goes into `diagramacao`).
pub fn layout_diff(cfg: &Json) -> Json {
    let mut out = serde_json::Map::new();
    out.insert("formato".into(), cfg["formato"].clone());
    let custom = cfg["formato"].as_str() == Some("custom");
    if let Json::Object(m) = cfg {
        for (k, v) in m {
            if k == "formato" || ((k == "largura" || k == "altura") && !custom) {
                continue;
            }
            match DEFAULT_LAYOUT.get(k) {
                Some(d) if same(d, v) => {}
                None if v.is_null() || v.as_str() == Some("") => {}
                _ => {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
    }
    Json::Object(out)
}

fn same(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Object(x), Json::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        (Json::Number(_), _) | (_, Json::Number(_)) => (a.as_f64().unwrap_or(0.0) - b.as_f64().unwrap_or(0.0)).abs() < 1e-6,
        _ => a == b,
    }
}

// ── creating things ────────────────────────────────────────────────────────

pub fn unique(root: &Path, path: &str) -> String {
    if !root.join(path).exists() {
        return path.to_string();
    }
    let (base, ext) = path.rsplit_once('.').unwrap_or((path, ""));
    for i in 2..999 {
        let p = format!("{base} {i}.{ext}");
        if !root.join(&p).exists() {
            return p;
        }
    }
    path.to_string()
}

pub fn write(root: &Path, rel: &str, text: &str) -> Result<(), String> {
    let abs = root.join(rel);
    if let Some(d) = abs.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(abs, text).map_err(|e| e.to_string())
}

pub struct NewBook {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub serie: String,
    pub volume: String,
    pub genre: String,
    pub goal: i64,
    pub deadline: String,
    pub synopsis: String,
    pub folder: String,
}

/// A book folder with its subfolders, the book note and a first chapter.
/// Returns the paths written (book note first).
pub fn create_book(root: &Path, v: &NewBook) -> Result<Vec<String>, String> {
    let title = safe_name(if v.title.trim().is_empty() { "Novo livro" } else { &v.title });
    let base = if v.folder.trim().is_empty() { "Livros".to_string() } else { v.folder.trim().trim_matches('/').to_string() };
    let mut folder = join(&base, &title);
    let mut n = 2;
    while root.join(&folder).exists() {
        folder = join(&base, &format!("{title} {n}"));
        n += 1;
    }
    for sub in ["Manuscrito", "Personagens", "Enredo", "Mapas", "Artes", "Exportações"] {
        std::fs::create_dir_all(root.join(&folder).join(sub)).map_err(|e| e.to_string())?;
    }
    let volume = if v.volume.is_empty() { json!("") } else { v.volume.parse::<f64>().map(|x| json!(x)).unwrap_or(json!(v.volume)) };
    let fm = [
        ("scriptorium", json!("livro")),
        ("titulo", json!(if v.title.trim().is_empty() { title.clone() } else { v.title.clone() })),
        ("subtitulo", json!(v.subtitle)),
        ("autor", json!(v.author)),
        ("serie", json!(v.serie)),
        ("volume", volume),
        ("genero", json!(v.genre)),
        ("meta_palavras", json!(if v.goal > 0 { v.goal } else { 80000 })),
        ("prazo", json!(v.deadline)),
        ("status", json!("rascunho")),
        ("cor", json!("#2f6b6b")),
        ("capa", json!("")),
        ("editora", json!("")),
        ("isbn", json!("")),
        ("ano", json!(chrono::Local::now().format("%Y").to_string().parse::<i64>().unwrap_or(2026))),
        ("idioma", json!("pt-BR")),
        ("dedicatoria", json!("")),
        ("epigrafe", json!("")),
        ("playlist", json!([])),
    ];
    let body = format!("\n## Sinopse\n\n{}\n\n## Contracapa\n\nTexto da contracapa (aparece no verso da capa e no EPUB).\n\n## Sobre o autor\n\n\n## Agradecimentos\n\n\n## Notas\n\n", v.synopsis);
    let book_path = join(&folder, &format!("{title}.md"));
    write(root, &book_path, &note_with(&fm, &body))?;
    let ch = chapter_note(root, &join(&folder, "Manuscrito"), 1, 1.0, "Capítulo um", "", "capitulo")?;
    Ok(vec![book_path, ch])
}

fn chapter_note(root: &Path, manuscript: &str, n: usize, order: f64, title: &str, part: &str, tipo: &str) -> Result<String, String> {
    let path = unique(root, &join(manuscript, &format!("{} — {}.md", pad2(n), safe_name(title))));
    let mut fm = vec![("scriptorium", json!("capitulo")), ("ordem", json!(order as i64)), ("titulo", json!(title)), ("status", json!("rascunho")), ("tipo", json!(if tipo.is_empty() { "capitulo" } else { tipo }))];
    if !part.is_empty() {
        fm.push(("parte", json!(part)));
    }
    write(root, &path, &note_with(&fm, "\n"))?;
    Ok(path)
}

pub fn create_chapter(root: &Path, ix: &Index, book: &Book, title: &str, part: &str, tipo: &str) -> Result<String, String> {
    let chs = chapters(ix, book);
    let n = chs.len() + 1;
    let max = chs.iter().filter_map(|c| c.order).fold(0.0_f64, f64::max);
    let title = if title.trim().is_empty() { format!("Capítulo {n}") } else { title.trim().to_string() };
    chapter_note(root, &book.manuscript, n, max + 1.0, &title, part.trim(), tipo)
}

/// `[[Name]]` when the name alone finds the note, else `[[path|Name]]`.
pub fn note_link(ix: &Index, target: &str, from: &str) -> String {
    let name = basename(target);
    if ix.resolve(name, from).as_deref() == Some(target) {
        format!("[[{name}]]")
    } else {
        format!("[[{}|{name}]]", target.strip_suffix(".md").unwrap_or(target))
    }
}

pub fn create_character(root: &Path, ix: &Index, book: &Book, name: &str) -> Result<String, String> {
    let path = unique(root, &join(&join(&book.folder, "Personagens"), &format!("{}.md", safe_name(name))));
    let fm = [
        ("scriptorium", json!("personagem")),
        ("livro", json!(note_link(ix, &book.path, &path))),
        ("papel", json!("")),
        ("retrato", json!("")),
        ("apelidos", json!("")),
        ("grupo", json!("")),
        ("idade", json!("")),
        ("frase", json!("")),
        ("aliados", json!([])),
        ("inimigos", json!([])),
        ("cor", json!("")),
    ];
    let body = "\n## Descrição física\n\n\n## Personalidade\n\n\n## Arco do personagem\n\n\n## Notas\n\n";
    write(root, &path, &note_with(&fm, body))?;
    Ok(path)
}

/// Estimated printed pages from the word count and the layout.
pub fn page_estimate(words: i64, cfg: &Json) -> usize {
    let w = cfg["largura"].as_f64().unwrap_or(140.0);
    let h = cfg["altura"].as_f64().unwrap_or(210.0);
    let body = cfg["corpo"].as_f64().unwrap_or(10.5);
    let lead = cfg["entrelinha"].as_f64().unwrap_or(1.45);
    let per = (260.0 * (w * h) / (140.0 * 210.0) * (10.5 / body) * (1.45 / lead)).round();
    ((words as f64 / per.max(120.0)).round() as usize + 14).max(24)
}

/// Strip a leading "Capítulo 3 — " style number from a chapter title.
pub fn strip_number_prefix(title: &str, n: usize) -> String {
    let re = regex::Regex::new(&format!(r"(?i)^\s*(cap[ií]tulo\s+)?({n}|{})\s*[—\-–.:]\s*", super::roman(n))).unwrap();
    re.replace(title, "").into_owned()
}

/// "Capítulo Vinte e Três", "XXIII", "23"… as the layout asks.
pub fn chapter_label(cfg: &Json, ch: &Chapter, n: usize) -> String {
    if !ch.numbered {
        return CHAPTER_TYPES.iter().find(|t| t.0 == ch.tipo && ch.tipo != "capitulo").map(|t| t.1.to_string()).unwrap_or_default();
    }
    let prefix = cfg["rotuloPrefixo"].as_str().unwrap_or("Capítulo");
    let cap = |s: String| {
        let mut c = s.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    };
    let words = |s: String| s.split(' ').map(|w| if w == "e" { w.to_string() } else { cap(w.to_string()) }).collect::<Vec<_>>().join(" ");
    let with = |x: String| if prefix.is_empty() { x } else { format!("{prefix} {x}") };
    match cfg["numeracaoCapitulo"].as_str().unwrap_or("extenso") {
        "extenso" => with(words(super::extenso(n))),
        "romano" => with(super::roman(n)),
        "numero" => with(n.to_string()),
        "so-numero" => n.to_string(),
        "so-romano" => super::roman(n),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("sc-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn book_lifecycle() {
        let root = tmp();
        let nb = NewBook { title: "Vórtices".into(), subtitle: String::new(), author: "Eu".into(), serie: "Saga".into(), volume: "1".into(), genre: String::new(), goal: 0, deadline: String::new(), synopsis: "Era uma vez.".into(), folder: String::new() };
        let paths = create_book(&root, &nb).unwrap();
        assert_eq!(paths[0], "Livros/Vórtices/Vórtices.md");
        let mut ix = Index::build(&root);
        let bs = books(&ix, "");
        assert_eq!(bs.len(), 1);
        let b = &bs[0];
        assert_eq!(b.title, "Vórtices");
        assert_eq!(b.goal, 80000);
        assert_eq!(b.manuscript, "Livros/Vórtices/Manuscrito");
        let c2 = create_chapter(&root, &ix, b, "A queda", "Ato I", "capitulo").unwrap();
        ix.update(&[c2.clone()]);
        let chs = chapters(&ix, b);
        assert_eq!(chs.len(), 2);
        assert_eq!(chs[1].title, "A queda");
        assert_eq!(chs[1].part, "Ato I");
        assert!(is_chapter(&ix, &c2));
        assert_eq!(book_for(&ix, &c2, "").unwrap().path, b.path);
        let ch = create_character(&root, &ix, b, "Ana").unwrap();
        ix.update(&[ch.clone()]);
        assert_eq!(characters(&ix, b), vec![ch]);
        let cfg = layout(b);
        assert_eq!(cfg["largura"], json!(140.0));
        assert_eq!(chapter_label(&cfg, &chs[1], 23), "Capítulo Vinte e Três");
        let _ = std::fs::remove_dir_all(&root);
    }
}
