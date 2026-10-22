//! Mundo: a codex over the notes the writer already has (each heading of a
//! long note, or each note of a folder, becomes an entry), the world's
//! calendar and eras, family trees and a dictionary over the language
//! notes' tables. The organisation lives in `<first book of the series>/
//! Mundo/Mundo.md` (a JSON block); the notes are read where they are.

use super::book::{self, Book};
use super::{read_json_block, write_json_block};
use crate::dataview::index::Index;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const LANG: &str = "scriptorium-mundo";

/// key, name, icon (Lucide name)
pub const CATS: [(&str, &str, &str); 17] = [
    ("visao", "Visão geral", "globe"),
    ("geografia", "Geografia e lugares", "mountain"),
    ("povos", "Povos e culturas", "users"),
    ("casas", "Casas e facções", "shield"),
    ("organizacoes", "Organizações", "landmark"),
    ("religiao", "Religião e mito", "flame"),
    ("magia", "Éter e Vórtices", "orbit"),
    ("bestiario", "Bestiário", "paw-print"),
    ("linguas", "Línguas e escritas", "languages"),
    ("tempo", "Tempo e medidas", "calendar-clock"),
    ("historia", "História e eras", "hourglass"),
    ("arte", "Arte e cultura", "palette"),
    ("itens", "Itens e tecnologia", "gem"),
    ("entidades", "Entidades", "ghost"),
    ("conceitos", "Conceitos e temas", "lightbulb"),
    ("personagens", "Origens e pessoas", "user"),
    ("pesquisa", "Pesquisa", "book-open"),
];

pub fn cat_name(k: &str) -> &'static str {
    CATS.iter().find(|c| c.0 == k).map(|c| c.1).unwrap_or("")
}

static RULES: Lazy<Vec<(&'static str, Regex)>> = Lazy::new(|| {
    [
        ("linguas", r"(?i)l[ií]ngu|idioma|escritas?\b|l[eé]xic|gram[aá]tic|fonolog|can[oô]ne.?ling|dialet|diacronia|qaynn|qayano|daetch|conlang|glifo"),
        ("bestiario", r"(?i)besti[aá]ri|fauna|flora|criatura|monstro"),
        ("casas", r"(?i)\bcasas?\b|dinast|her[aá]ld|armorial|bras[aã]o"),
        ("religiao", r"(?i)religi|\bmito|pante[aã]o|teolog|demonolog|selamento|sagrad|\bdeus(es)?\b|yauh|culto|inferno"),
        ("magia", r"(?i)[eé]ter\b|v[oó]rtice|magia|harmonia|esp[ií]rito"),
        ("tempo", r"(?i)\btempo\b|calend|astronom|medida|\bc[eé]u\b|esta[cç][oõ]es|semana|\bmeses\b"),
        ("historia", r"(?i)hist[oó]ria|\beras?\b|cronolog|guerra|crep[uú]sculo"),
        ("arte", r"(?i)\barte|m[uú]sica|oralidade|literatura|cultura material"),
        ("povos", r"(?i)\bpovos?\b|etnograf|culturas?\b|asamba|jalenk|tah[eé]lin|[üu]zgentai|qirenm|vaels|koraen|\bgrupo\b"),
        ("organizacoes", r"(?i)\biga\b|instituto|organiza|escola|infraestrutura|ensino|sa[uú]de|institui|torneio|anexo"),
        ("geografia", r"(?i)geograf|clima|\bmapas?\b|continent|oceano|placas|reinos?\b|pa[ií]ses|cidade|bairro|planeta|bioma|regi[oõ]es|mundos?\b|caminhada|movimento"),
        ("itens", r"(?i)\bite(m|ns)\b|ferrament|tecnolog|\barmas?\b"),
        ("entidades", r"(?i)entidade"),
        ("conceitos", r"(?i)conceito|\btemas?\b|protagonismo|antagonismo"),
    ]
    .into_iter()
    .map(|(k, r)| (k, Regex::new(r).unwrap()))
    .collect()
});

/// The category whose term shows up first in the title wins.
pub fn classify(text: &str, fallback: &str) -> String {
    let mut best: Option<(&str, usize)> = None;
    for (k, re) in RULES.iter() {
        if let Some(m) = re.find(text) {
            if best.is_none_or(|b| m.start() < b.1) {
                best = Some((k, m.start()));
            }
        }
    }
    best.map(|b| b.0.to_string()).unwrap_or_else(|| if fallback.is_empty() { "visao".into() } else { fallback.into() })
}

/// "### 3. Os Povos das Marés (autônimo: **Tahélin**)" → "Os Povos das Marés (autônimo: Tahélin)"
pub fn clean_title(h: &str) -> String {
    static ESC: Lazy<Regex> = Lazy::new(|| Regex::new(r"\\([\\`*_{}\[\]()#+\-.!|])").unwrap());
    static EMPH: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*?|__|`").unwrap());
    static WB: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*worldbuilding\s*[—–-]\s*").unwrap());
    static PARTE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*(parte\s+[ivxlc]+\s*[—–-]\s*)").unwrap());
    static NUM: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*(\d+(\.\d+)*|[ivxlc]+)[.)]?\s*[—–-]?\s+").unwrap());
    static DASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*[—–-]\s*").unwrap());
    let s = ESC.replace_all(h, "$1");
    let s = EMPH.replace_all(&s, "");
    let s = WB.replace(&s, "");
    let s = PARTE.replace(&s, "");
    let s = NUM.replace(&s, "");
    let s = DASH.replace(&s, "");
    let t = s.trim();
    if t.is_empty() { h.trim().to_string() } else { t.to_string() }
}

const STOP: [&str; 16] = ["Parte", "Resumo", "Síntese", "Apêndice", "Coda", "Nota", "Introdução", "Índice", "Mapas", "Associação", "Características", "Espinha", "Frame", "O", "A", "Os"];

fn starts_upper(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase())
}

/// Names the codex looks for in the chapters: bold or italic names in the
/// heading, and the heading's short core.
pub fn terms(raw: &str) -> Vec<String> {
    static EM: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*([^*]{3,40})\*\*|(?:^|[^*])\*([^*]{3,40})\*").unwrap());
    static SPLIT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+[—–]\s+|:\s|\s\(").unwrap());
    static ART: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^(o|a|os|as|um|uma)\s+").unwrap());
    let mut out: Vec<String> = Vec::new();
    for c in EM.captures_iter(raw) {
        let t = c.get(1).or(c.get(2)).map(|m| m.as_str().trim()).unwrap_or("");
        if starts_upper(t) && t.split_whitespace().count() <= 3 && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    let clean = clean_title(raw);
    let core = SPLIT.split(&clean).next().unwrap_or("").trim().to_string();
    let core = ART.replace(&core, "").to_string();
    let words: Vec<&str> = core.split_whitespace().collect();
    if core.chars().count() >= 3 && words.len() <= 3 && starts_upper(&core) && !STOP.contains(&words[0]) && words[0] != "As" && !out.contains(&core) {
        out.push(core);
    }
    out
}

/// "≈200 000 – 12 000 AC" → (-200000, -12000) · "≈1 100 – 2 840 DC" → (1100, 2840)
pub fn parse_range(txt: &str, antes: &str, depois: &str) -> Option<(f64, f64)> {
    let s: String = txt.chars().map(|c| if c == '\u{a0}' || c == '\u{202f}' || c == '\u{2009}' { ' ' } else { c }).collect();
    let a = if antes.is_empty() { "AC" } else { antes };
    let d = if depois.is_empty() { "DC" } else { depois };
    let re = Regex::new(&format!(r"(?i)(\d{{1,3}}(?:[ .]\d{{3}})*|\d+)\s*({}|{}|a\.\s?c\.|d\.\s?c\.)?", regex::escape(a), regex::escape(d))).ok()?;
    let mut nums: Vec<(f64, char)> = Vec::new();
    for c in re.captures_iter(&s) {
        let Ok(n) = c[1].replace([' ', '.'], "").parse::<f64>() else { continue };
        let mk = c.get(2).map(|m| m.as_str().to_uppercase().replace(['.', ' '], "")).unwrap_or_default();
        let era = if mk.is_empty() { ' ' } else if mk == a.to_uppercase() || mk == "AC" { 'a' } else { 'd' };
        nums.push((n, era));
        if nums.len() == 2 {
            break;
        }
    }
    match nums.len() {
        0 => None,
        1 => {
            let v = if nums[0].1 == 'a' { -nums[0].0 } else { nums[0].0 };
            Some((v, v))
        }
        _ => {
            if nums[0].1 == ' ' {
                nums[0].1 = if nums[1].1 == ' ' { 'd' } else { nums[1].1 };
            }
            if nums[1].1 == ' ' {
                nums[1].1 = 'd';
            }
            let v: Vec<f64> = nums.iter().map(|x| if x.1 == 'a' { -x.0 } else { x.0 }).collect();
            Some(if v[0] <= v[1] { (v[0], v[1]) } else { (v[1], v[0]) })
        }
    }
}

pub fn year(y: f64, cal: &Json) -> String {
    if y.is_nan() {
        return String::new();
    }
    let n = super::fmt_n(y.abs().round() as i64);
    if y < 0.0 {
        format!("{n} {}", cal["eraAntes"].as_str().unwrap_or("AC"))
    } else if y == 0.0 {
        "ano 0".into()
    } else {
        format!("{n} {}", cal["eraDepois"].as_str().unwrap_or("DC"))
    }
}

// ── calendar ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Month {
    pub nome: String,
    pub local: String,
    pub dias: i64,
    pub ancora: String,
    pub intercalar: bool,
}

/// A lunisolar calendar with a leap month in a cycle (or a plain solar one).
pub struct Calendar<'a> {
    pub c: &'a Json,
}

impl Calendar<'_> {
    pub fn months(&self) -> Vec<Month> {
        self.c["meses"].as_array().into_iter().flatten().map(|m| Month { nome: m["nome"].as_str().unwrap_or("").into(), local: m["local"].as_str().unwrap_or("").into(), dias: m["dias"].as_i64().unwrap_or(30), ancora: m["ancora"].as_str().unwrap_or("").into(), intercalar: false }).collect()
    }
    fn ic(&self) -> Option<&Json> {
        let ic = &self.c["intercalar"];
        (ic.is_object() && ic["ciclo"].as_i64().unwrap_or(0) > 0).then_some(ic)
    }
    pub fn is_leap(&self, y: i64) -> bool {
        let Some(ic) = self.ic() else { return false };
        let c = ic["ciclo"].as_i64().unwrap();
        let pos = y.rem_euclid(c) + 1;
        ic["anos"].as_array().is_some_and(|a| a.iter().any(|x| x.as_i64() == Some(pos)))
    }
    pub fn months_of(&self, y: i64) -> Vec<Month> {
        let mut ms = self.months();
        if let (Some(ic), true) = (self.ic(), self.is_leap(y)) {
            let at = (ic["depois"].as_i64().unwrap_or(6).max(0) as usize).min(ms.len());
            ms.insert(at, Month { nome: ic["nome"].as_str().unwrap_or("").into(), local: ic["local"].as_str().unwrap_or("").into(), dias: ic["dias"].as_i64().unwrap_or(30), ancora: String::new(), intercalar: true });
        }
        ms
    }
    pub fn year_len(&self, y: i64) -> i64 {
        self.months_of(y).iter().map(|m| m.dias).sum()
    }
    /// (years, days) of a full cycle.
    pub fn cycle(&self) -> (i64, i64) {
        let base: i64 = self.months().iter().map(|m| m.dias).sum();
        match self.ic() {
            None => (1, base),
            Some(ic) => {
                let c = ic["ciclo"].as_i64().unwrap();
                let n = ic["anos"].as_array().map(|a| a.len()).unwrap_or(0) as i64;
                (c, base * c + n * ic["dias"].as_i64().unwrap_or(30))
            }
        }
    }
    pub fn days_before_year(&self, y: i64) -> i64 {
        let (cy, cd) = self.cycle();
        let full = y.div_euclid(cy);
        let mut d = full * cd;
        for k in full * cy..y {
            d += self.year_len(k);
        }
        d
    }
    pub fn to_abs(&self, y: i64, mi: usize, day: i64) -> i64 {
        let ms = self.months_of(y);
        self.days_before_year(y) + ms.iter().take(mi).map(|m| m.dias).sum::<i64>() + (day - 1)
    }
    /// (year, month index, day)
    pub fn from_abs(&self, abs: i64) -> (i64, usize, i64) {
        let (cy, cd) = self.cycle();
        if cd <= 0 {
            return (0, 0, 1);
        }
        let mut y = abs.div_euclid(cd) * cy;
        let mut rest = abs - self.days_before_year(y);
        while rest < 0 {
            y -= 1;
            rest += self.year_len(y);
        }
        while rest >= self.year_len(y) {
            rest -= self.year_len(y);
            y += 1;
        }
        let ms = self.months_of(y);
        let mut mi = 0;
        while mi + 1 < ms.len() && rest >= ms[mi].dias {
            rest -= ms[mi].dias;
            mi += 1;
        }
        (y, mi, rest + 1)
    }
    /// (weekday name, block within the month when the week restarts each month)
    pub fn weekday(&self, y: i64, mi: usize, d: i64) -> Option<(String, Option<i64>)> {
        let sem = self.c["semana"].as_array().filter(|a| !a.is_empty())?;
        let n = sem.len() as i64;
        if self.c["semanaPorMes"].as_bool().unwrap_or(false) {
            let k = ((d - 1) % n) as usize;
            let mut nome = sem[k]["nome"].as_str().unwrap_or("").to_string();
            if let Some(x) = self.c["diaExtra"].as_str().filter(|x| !x.is_empty() && d == 30) {
                nome = format!("{x} ({nome})");
            }
            return Some((nome, Some((d - 1) / n + 1)));
        }
        let abs = self.to_abs(y, mi, d) + self.c["inicioSemana"].as_i64().unwrap_or(0);
        Some((sem[abs.rem_euclid(n) as usize]["nome"].as_str().unwrap_or("").to_string(), None))
    }
    pub fn describe(&self, y: i64, mi: usize, d: i64) -> String {
        let ms = self.months_of(y);
        let Some(m) = ms.get(mi) else { return String::new() };
        let wd = match self.weekday(y, mi, d) {
            Some((n, Some(b))) => format!("{n}, {b}º {} · ", self.c["semanaNome"].as_str().filter(|s| !s.is_empty()).unwrap_or("semana")),
            Some((n, None)) => format!("{n} · "),
            None => String::new(),
        };
        format!("{wd}{d} de {} de {}", m.nome, year(y as f64, self.c))
    }
    /// Add days, months or years to a date.
    pub fn add(&self, y: i64, mi: usize, d: i64, n: i64, unit: char) -> (i64, usize, i64) {
        match unit {
            'd' => self.from_abs(self.to_abs(y, mi, d) + n),
            'y' => {
                let y2 = y + n;
                let ms = self.months_of(y2);
                let mi2 = mi.min(ms.len().saturating_sub(1));
                (y2, mi2, d.min(ms.get(mi2).map(|m| m.dias).unwrap_or(30)))
            }
            _ => {
                let (mut yy, mut mm) = (y, mi as i64 + n);
                while mm >= self.months_of(yy).len() as i64 {
                    mm -= self.months_of(yy).len() as i64;
                    yy += 1;
                }
                while mm < 0 {
                    yy -= 1;
                    mm += self.months_of(yy).len() as i64;
                }
                let ms = self.months_of(yy);
                (yy, mm as usize, d.min(ms[mm as usize].dias))
            }
        }
    }
}

fn table_rows(txt: &str) -> Vec<Vec<String>> {
    let lines: Vec<&str> = txt.lines().collect();
    let Some(start) = lines.iter().position(|l| l.starts_with('|')) else { return vec![] };
    let mut rows = Vec::new();
    let mut i = start + 2;
    while i < lines.len() && lines[i].starts_with('|') {
        let cells: Vec<&str> = lines[i].split('|').collect();
        if cells.len() >= 2 {
            rows.push(cells[1..cells.len() - 1].iter().map(|c| c.replace(['*'], "").trim().to_string()).collect());
        }
        i += 1;
    }
    rows
}

/// A calendar read from a note's tables (months; week days; leap cycle).
pub fn calendar_from_md(md: &str) -> Json {
    let sec = |title: &str| -> String {
        let re = Regex::new(&format!(r"(?im)^#{{2,5}}[^\n]*{title}")).unwrap();
        match re.find(md) {
            Some(m) => md[m.start()..].chars().take(6000).collect(),
            None => String::new(),
        }
    };
    let mut cal = json!({ "nome": "Calendário", "meses": [], "semana": [], "semanaPorMes": false, "eraAntes": "AC", "eraDepois": "DC", "presente": null });
    static PAREN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\(?\d+\)?$").unwrap());
    static APOS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)ap[oó]s o (\d+)").unwrap());
    for r in table_rows(&sec("os meses")) {
        let dias: i64 = r.get(4).and_then(|x| x.trim().split(|c: char| !c.is_ascii_digit()).next()).and_then(|x| x.parse().ok()).unwrap_or(30);
        if PAREN.is_match(&r[0]) && r[0].starts_with('(') {
            let mut ic = json!({ "nome": r.get(1).cloned().unwrap_or_default(), "local": r.get(2).cloned().unwrap_or_default(), "dias": 30, "depois": 6, "ciclo": 19, "anos": [3, 6, 8, 11, 14, 17, 19] });
            if let Some(c) = r.get(3).and_then(|x| APOS.captures(x)) {
                ic["depois"] = json!(c[1].parse::<i64>().unwrap_or(6));
            }
            cal["intercalar"] = ic;
        } else if r.get(1).is_some_and(|x| !x.is_empty()) {
            cal["meses"].as_array_mut().unwrap().push(json!({ "nome": r[1], "local": r.get(2).cloned().unwrap_or_default(), "dias": dias, "ancora": r.get(3).cloned().unwrap_or_default() }));
        }
    }
    let sem = sec("a semana");
    for r in table_rows(&sem) {
        if r.get(1).is_some_and(|x| !x.is_empty()) {
            cal["semana"].as_array_mut().unwrap().push(json!({ "nome": r[1], "local": r.get(2).cloned().unwrap_or_default(), "uso": r.get(3).cloned().unwrap_or_default() }));
        }
    }
    if Regex::new(r"(?i)cinco blocos de seis dias|seisseno").unwrap().is_match(&sem) {
        cal["semanaPorMes"] = json!(true);
        cal["semanaNome"] = json!("seisseno");
        let dl = Regex::new(r"(?i)chamado de \*\*([^*]+)\*\*").unwrap().captures(&sem).map(|c| c[1].to_string()).unwrap_or_else(|| "Dia da Lua".into());
        cal["diaExtra"] = json!(dl);
    }
    if let (Some(c), true) = (Regex::new(r"(?i)ciclo de (\d+) anos").unwrap().captures(md), cal["intercalar"].is_object()) {
        cal["intercalar"]["ciclo"] = json!(c[1].parse::<i64>().unwrap_or(19));
    }
    if let Some(c) = Regex::new(r"(?i)presente[^.\n]{0,60}?(\d[\d. ]{2,})\s*(D\.?C\.?|DC)").unwrap().captures(md) {
        cal["presente"] = json!(c[1].replace([' ', '.'], "").parse::<i64>().ok());
    }
    cal
}

/// Eras from a periodisation table ("| **I. Alvorecer** | … | ≈200 000 – 12 000 AC | … |").
pub fn eras_from_md(md: &str) -> Vec<Json> {
    let Some(m) = Regex::new(r"(?im)^#{2,4}[^\n]*periodiza").unwrap().find(md) else { return vec![] };
    const CORES: [&str; 8] = ["#8a6d3b", "#5d7a3a", "#2f6b6b", "#4b5bb3", "#7a2e3b", "#a8641b", "#6b4f8a", "#3f9f98"];
    let mut eras = Vec::new();
    for c in table_rows(&md[m.start()..]) {
        let Some(r) = c.get(2).and_then(|x| parse_range(x, "", "")) else { continue };
        eras.push(json!({ "id": super::enredo::uid("era"), "nome": clean_title(&c[0]), "nativo": c.get(1).cloned().unwrap_or_default(), "de": r.0, "ate": r.1, "desc": c.get(3).cloned().unwrap_or_default(), "cor": CORES[eras.len() % CORES.len()] }));
    }
    eras
}

/// A Markdown table of a note, under the heading it sits in.
pub struct Table {
    pub head: String,
    pub cols: Vec<String>,
    pub rows: Vec<(Vec<String>, usize)>,
}

pub fn tables(md: &str) -> Vec<Table> {
    let lines: Vec<&str> = md.lines().collect();
    let sep = Regex::new(r"^\|\s*:?-{2,}").unwrap();
    let mut out = Vec::new();
    let mut head = String::new();
    let mut i = 0;
    let cells = |l: &str| -> Vec<String> {
        let v: Vec<&str> = l.split('|').collect();
        if v.len() < 2 { vec![] } else { v[1..v.len() - 1].iter().map(|c| c.trim().to_string()).collect() }
    };
    while i < lines.len() {
        if let Some(h) = heading(lines[i]) {
            head = clean_title(h.1);
            i += 1;
            continue;
        }
        if lines[i].starts_with('|') && i + 1 < lines.len() && sep.is_match(lines[i + 1]) {
            let cols = cells(lines[i]).into_iter().map(|c| c.replace('*', "")).collect();
            let mut rows = Vec::new();
            let mut k = i + 2;
            while k < lines.len() && lines[k].starts_with('|') {
                rows.push((cells(lines[k]), k));
                k += 1;
            }
            out.push(Table { head: head.clone(), cols, rows });
            i = k;
            continue;
        }
        i += 1;
    }
    out
}

// ── where the world lives ────────────────────────────────────────────────

/// The world belongs to the series: the book that already has a Mundo.md,
/// or the first volume.
pub fn home(root: &Path, ix: &Index, b: &Book, author: &str) -> Book {
    let saga = super::enredo::saga_books(ix, b, author);
    saga.iter().find(|x| root.join(format!("{}/Mundo/Mundo.md", x.folder)).exists()).or(saga.first()).cloned().unwrap_or_else(|| b.clone())
}

pub fn path(home: &Book) -> String {
    format!("{}/Mundo/Mundo.md", home.folder)
}

pub fn blank() -> Json {
    json!({ "v": 1, "mundos": [], "arvores": [], "fixados": [], "classif": {}, "termos": {} })
}

pub fn load(root: &Path, home: &Book) -> Json {
    let mut data = blank();
    if let Some(Json::Object(m)) = std::fs::read_to_string(root.join(path(home))).ok().and_then(|t| read_json_block(&t, LANG)) {
        for (k, v) in m {
            data[k] = v;
        }
    }
    for k in ["mundos", "arvores", "fixados"] {
        if !data[k].is_array() {
            data[k] = json!([]);
        }
    }
    for k in ["classif", "termos"] {
        if !data[k].is_object() {
            data[k] = json!({});
        }
    }
    data
}

pub fn save(root: &Path, ix: &Index, home: &Book, data: &Json) -> Result<String, String> {
    let p = path(home);
    let full = root.join(&p);
    let text = match std::fs::read_to_string(&full) {
        Ok(t) => write_json_block(&t, LANG, data),
        Err(_) => {
            let head = format!(
                "---\nscriptorium: mundo\nlivro: \"{}\"\n---\n\nMundo de **{}**: códice, calendário, eras, genealogias e línguas. Abra pela tela Mundo do Scriptorium. As notas do mundo continuam onde estão; este arquivo guarda só a organização.\n",
                book::note_link(ix, &home.path, &p),
                if home.serie.is_empty() { &home.title } else { &home.serie }
            );
            write_json_block(&head, LANG, data)
        }
    };
    if let Some(d) = full.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&full, text).map_err(|e| e.to_string())?;
    Ok(p)
}

// ── the codex ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Entry {
    pub key: String,
    pub cat: String,
    pub titulo: String,
    pub path: String,
    pub heading: Option<String>,
    pub line: usize,
    pub end: usize,
    pub words: usize,
    pub snippet: String,
    pub fonte: String,
    pub terms: Vec<String>,
    pub children: Vec<String>,
    pub parent: Option<String>,
    pub legado: bool,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub path: String,
    pub fonte: String,
    pub cat: String,
    pub legado: bool,
}

#[derive(Default)]
pub struct Codex {
    pub entries: Vec<Entry>,
    pub by_key: HashMap<String, usize>,
    pub images: Vec<Image>,
    /// What the index was built from (paths and mtimes, classifications).
    pub sig: String,
}

impl Codex {
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.by_key.get(key).map(|&i| &self.entries[i])
    }
    fn push(&mut self, e: Entry) {
        if self.by_key.contains_key(&e.key) {
            return;
        }
        self.by_key.insert(e.key.clone(), self.entries.len());
        self.entries.push(e);
    }
}

fn is_image(p: &str) -> bool {
    let l = p.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"].iter().any(|e| l.ends_with(e))
}

/// Notes and images of a source (a folder or a single file), sorted.
pub fn source_files(root: &Path, src: &Json) -> (Vec<String>, Vec<String>) {
    let p = src["caminho"].as_str().unwrap_or("").trim_matches('/').to_string();
    let abs = root.join(&p);
    if abs.is_file() {
        return if p.ends_with(".md") { (vec![p], vec![]) } else if is_image(&p) { (vec![], vec![p]) } else { (vec![], vec![]) };
    }
    if !abs.is_dir() {
        return (vec![], vec![]);
    }
    let ign: Vec<String> = src["ignorar"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(|x| format!("{p}/{}", x.trim_matches('/'))).collect();
    let (mut md, mut img) = (Vec::new(), Vec::new());
    fn walk(root: &Path, dir: &str, ign: &[String], md: &mut Vec<String>, img: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let rel = format!("{dir}/{name}");
            if ign.iter().any(|x| rel == *x || rel.starts_with(&format!("{x}/"))) {
                continue;
            }
            let ft = e.file_type();
            if ft.as_ref().is_ok_and(|t| t.is_dir()) {
                walk(root, &rel, ign, md, img);
            } else if rel.ends_with(".md") {
                md.push(rel);
            } else if is_image(&rel) {
                img.push(rel);
            }
        }
    }
    walk(root, &p, &ign, &mut md, &mut img);
    md.sort_by(|a, b| crate::text::natural_cmp(a, b));
    img.sort_by(|a, b| crate::text::natural_cmp(a, b));
    (md, img)
}

fn mtime(root: &Path, p: &str) -> u128 {
    std::fs::metadata(root.join(p)).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis()).unwrap_or(0)
}

pub fn signature(root: &Path, w: &Json, data: &Json) -> String {
    let mut parts = Vec::new();
    for s in w["fontes"].as_array().into_iter().flatten() {
        let (md, img) = source_files(root, s);
        for f in md.iter().chain(&img) {
            parts.push(format!("{f}:{}", mtime(root, f)));
        }
    }
    format!("{}|{}|{}|{}", parts.join("|"), data["classif"], data["termos"], w["fontes"])
}

/// `(level, text)` of a Markdown heading line.
fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_end();
    let n = t.chars().take_while(|c| *c == '#').count();
    if n == 0 || n > 6 || !t[n..].starts_with([' ', '\t']) {
        return None;
    }
    Some((n, t[n..].trim().trim_end_matches('#').trim()))
}

struct H {
    raw: String,
    level: usize,
    line: usize,
}

fn headings(lines: &[&str]) -> Vec<H> {
    let mut out = Vec::new();
    let mut fence = false;
    let mut fm = lines.first().is_some_and(|l| l.trim() == "---");
    for (i, l) in lines.iter().enumerate() {
        if fm {
            if i > 0 && l.trim() == "---" {
                fm = false;
            }
            continue;
        }
        let t = l.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        if let Some((level, raw)) = heading(l) {
            out.push(H { raw: raw.to_string(), level, line: i });
        }
    }
    out
}

fn count_words(lines: &[&str], a: usize, b: usize) -> usize {
    static SEP: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*\|?\s*:?-{2,}").unwrap());
    let mut n = 0;
    for l in &lines[a.min(lines.len())..b.min(lines.len())] {
        if l.is_empty() || SEP.is_match(l) {
            continue;
        }
        let mut inword = false;
        for c in l.chars() {
            let w = c.is_alphanumeric();
            if w && !inword {
                n += 1;
            }
            inword = w;
        }
    }
    n
}

fn snippet(lines: &[&str], a: usize, b: usize) -> String {
    static STRIP: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*?|__|`|\[\[([^\]|]*\|)?|\]\]").unwrap());
    static SKIP: Lazy<Regex> = Lazy::new(|| Regex::new(r"^#|^\||^!\[|^---|^```|^>").unwrap());
    for l in lines.iter().take(b.min(a + 60).min(lines.len())).skip(a) {
        let t = l.trim();
        if t.is_empty() || SKIP.is_match(t) {
            continue;
        }
        let s = STRIP.replace_all(t, "");
        let s = s.trim_start_matches(['-', '*']).trim_start();
        return s.chars().take(220).collect();
    }
    String::new()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn basename(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).trim_end_matches(".md").to_string()
}

/// Build the codex of a world from its sources.
pub fn index(root: &Path, w: &Json, data: &Json) -> Codex {
    let mut cx = Codex { sig: signature(root, w, data), ..Default::default() };
    let classif = &data["classif"];
    let cls = |k: &str| classif[k].as_str().map(String::from);
    for src in w["fontes"].as_array().into_iter().flatten() {
        let (md, img) = source_files(root, src);
        let rotulo = src["rotulo"].as_str().unwrap_or("").to_string();
        let src_cat = src["cat"].as_str().unwrap_or("").to_string();
        let legado_all = src["legado"].as_bool().unwrap_or(false);
        let legado_titles: Vec<String> = src["legadoTitulos"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(|x| x.to_lowercase()).collect();
        let is_legacy = |title: &str| legado_all || legado_titles.iter().any(|t| clean_title(title).to_lowercase().starts_with(t.as_str()));
        for f in img {
            let cat = if src_cat.is_empty() { classify(&f, "visao") } else { src_cat.clone() };
            cx.images.push(Image { path: f, fonte: rotulo.clone(), cat, legado: legado_all });
        }
        for f in md {
            let Ok(text) = std::fs::read_to_string(root.join(&f)) else { continue };
            let lines: Vec<&str> = text.lines().collect();
            let mut hs = headings(&lines);
            let mut hi = lines.len();
            let mut base: Option<usize> = None;
            if let Some(secao) = src["secao"].as_str().filter(|s| !s.is_empty()) {
                let want = secao.to_lowercase();
                let want_c = clean_title(secao).to_lowercase();
                let plain = |h: &H| h.raw.replace(['*', '_'], "").trim().to_lowercase();
                let Some(s0) = hs.iter().position(|h| plain(h).starts_with(&want)).or_else(|| hs.iter().position(|h| clean_title(&h.raw).to_lowercase().starts_with(&want_c))) else { continue };
                let (lo, lvl) = (hs[s0].line, hs[s0].level);
                hi = hs.iter().find(|h| h.line > lo && h.level <= lvl).map(|h| h.line).unwrap_or(lines.len());
                base = Some(lvl);
                let raw0 = hs[s0].raw.clone();
                hs.retain(|h| h.line > lo && h.line < hi && !clean_title(&h.raw).trim().is_empty());
                if hs.is_empty() {
                    hs = vec![H { raw: raw0, level: lvl, line: lo }];
                    base = Some(lvl.saturating_sub(1));
                }
            }
            let file_cat = src_cat.clone();
            let as_file = src["tipo"] == "pasta" || hs.is_empty();
            let min_level = hs.iter().map(|h| h.level).min().unwrap_or(1);
            let fname = basename(&f);
            if as_file {
                let key = f.clone();
                let cat = cls(&key).unwrap_or_else(|| if file_cat.is_empty() { classify(&fname.replace('_', " "), "visao") } else { file_cat.clone() });
                let nice = fname.replace('_', " ");
                let legado = is_legacy(&fname);
                let mut e = Entry { key: key.clone(), cat: cat.clone(), titulo: capitalize(&clean_title(&nice)), path: f.clone(), heading: None, line: 0, end: lines.len(), words: count_words(&lines, 0, lines.len()), snippet: snippet(&lines, 0, lines.len()), fonte: rotulo.clone(), terms: terms(&fname), children: vec![], parent: None, legado };
                let title_only = hs.iter().filter(|h| h.level == min_level).count() == 1 && hs.first().is_some_and(|h| h.level == min_level);
                let kids_level = if title_only { min_level + 1 } else { min_level };
                let mut kids = Vec::new();
                for (k, h) in hs.iter().enumerate().filter(|(_, h)| h.level == kids_level) {
                    let end = hs[k + 1..].iter().find(|x| x.level <= h.level).map(|x| x.line).unwrap_or(lines.len());
                    let k2 = format!("{f}#{}", h.raw);
                    kids.push(Entry { key: k2.clone(), cat: cls(&k2).unwrap_or_else(|| cat.clone()), titulo: clean_title(&h.raw), path: f.clone(), heading: Some(h.raw.clone()), line: h.line, end, words: count_words(&lines, h.line, end), snippet: snippet(&lines, h.line + 1, end), fonte: e.fonte.clone(), terms: terms(&h.raw), children: vec![], parent: Some(key.clone()), legado });
                    e.children.push(k2);
                }
                cx.push(e);
                for k in kids {
                    cx.push(k);
                }
                continue;
            }
            if base.is_none() && hs.len() > 1 && hs[0].level == min_level && hs.iter().filter(|h| h.level == min_level).count() == 1 {
                hs.remove(0);
            }
            // outline: top headings (no ancestor in the stretch) and their direct children
            let mut stack: Vec<usize> = Vec::new();
            let mut tops: Vec<(usize, Vec<usize>)> = Vec::new();
            for i in 0..hs.len() {
                while stack.last().is_some_and(|&s| hs[s].level >= hs[i].level) {
                    stack.pop();
                }
                if stack.is_empty() {
                    tops.push((i, vec![]));
                } else if stack.len() == 1 {
                    tops.last_mut().unwrap().1.push(i);
                }
                stack.push(i);
            }
            let end_of = |i: usize| hs[i + 1..].iter().find(|x| x.level <= hs[i].level).map(|x| x.line).unwrap_or(hi);
            for (t, kids) in tops {
                let h = &hs[t];
                let end = end_of(t);
                let key = format!("{f}#{}", h.raw);
                let cat = cls(&key).unwrap_or_else(|| if file_cat.is_empty() { classify(&h.raw, "visao") } else { file_cat.clone() });
                let legado = is_legacy(&h.raw);
                let fonte = if rotulo.is_empty() { fname.clone() } else { rotulo.clone() };
                let mut e = Entry { key: key.clone(), cat: cat.clone(), titulo: clean_title(&h.raw), path: f.clone(), heading: Some(h.raw.clone()), line: h.line, end, words: count_words(&lines, h.line, end), snippet: snippet(&lines, h.line + 1, end), fonte: fonte.clone(), terms: terms(&h.raw), children: vec![], parent: None, legado };
                let mut kid_entries = Vec::new();
                for k in kids {
                    let c0 = &hs[k];
                    let end2 = end_of(k).min(end);
                    let k2 = format!("{f}#{}", c0.raw);
                    kid_entries.push(Entry { key: k2.clone(), cat: cls(&k2).unwrap_or_else(|| cat.clone()), titulo: clean_title(&c0.raw), path: f.clone(), heading: Some(c0.raw.clone()), line: c0.line, end: end2, words: count_words(&lines, c0.line, end2), snippet: snippet(&lines, c0.line + 1, end2), fonte: fonte.clone(), terms: terms(&c0.raw), children: vec![], parent: Some(key.clone()), legado });
                    e.children.push(k2);
                }
                cx.push(e);
                for k in kid_entries {
                    cx.push(k);
                }
            }
        }
    }
    if let Some(t) = data["termos"].as_object() {
        for (k, v) in t {
            if let Some(&i) = cx.by_key.get(k) {
                cx.entries[i].terms = v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
            }
        }
    }
    cx
}

pub fn section_text(root: &Path, e: &Entry) -> String {
    let text = std::fs::read_to_string(root.join(&e.path)).unwrap_or_default();
    text.lines().skip(e.line).take(e.end.saturating_sub(e.line)).collect::<Vec<_>>().join("\n")
}

/// Search the sources' whole text: entry key → the line around the hit.
pub fn full_search(root: &Path, cx: &Codex, q: &str) -> HashMap<String, String> {
    let needle = q.to_lowercase();
    let mut files: HashMap<&str, Vec<String>> = HashMap::new();
    for e in &cx.entries {
        files.entry(e.path.as_str()).or_insert_with(|| std::fs::read_to_string(root.join(&e.path)).unwrap_or_default().lines().map(String::from).collect());
    }
    let mut hits = HashMap::new();
    for e in &cx.entries {
        let lines = &files[e.path.as_str()];
        let stop = e.children.iter().filter_map(|k| cx.get(k)).map(|c| c.line).min().unwrap_or(e.end).min(e.end);
        for l in lines.iter().take(stop).skip(e.line) {
            let low = l.to_lowercase();
            if let Some(i) = low.find(&needle) {
                let chars: Vec<char> = l.chars().collect();
                let ci = low[..i].chars().count();
                let s: String = chars[ci.saturating_sub(60)..(ci + 120).min(chars.len())].iter().collect();
                hits.insert(e.key.clone(), s.replace(['*', '`'], ""));
                break;
            }
        }
    }
    hits
}

/// In which chapters each name appears: term → indices into `chapters`.
pub fn appearances(root: &Path, chapters: &[String], cx: &Codex) -> HashMap<String, Vec<usize>> {
    let texts: Vec<String> = chapters.iter().map(|p| std::fs::read_to_string(root.join(p)).unwrap_or_default()).collect();
    let mut terms: Vec<&String> = cx.entries.iter().flat_map(|e| e.terms.iter()).collect();
    terms.sort();
    terms.dedup();
    let mut out = HashMap::new();
    for t in terms {
        let Ok(re) = Regex::new(&format!(r"(^|[^\p{{L}}\p{{N}}]){}([^\p{{L}}\p{{N}}]|$)", regex::escape(t))) else { continue };
        let found: Vec<usize> = texts.iter().enumerate().filter(|(_, x)| re.is_match(x)).map(|(i, _)| i).collect();
        if !found.is_empty() {
            out.insert(t.clone(), found);
        }
    }
    out
}

/// A row of a language note's table.
#[derive(Clone, Debug)]
pub struct LexRow {
    pub lang: String,
    pub path: String,
    pub head: String,
    pub cols: Vec<String>,
    pub cells: Vec<String>,
    pub line: usize,
    /// Lowercase, without accents, for searching.
    pub text: String,
}

pub fn deaccent(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase()
}

/// Every table of the world's language notes, as searchable rows.
pub fn lexicon(root: &Path, w: &Json) -> Vec<LexRow> {
    let mut out = Vec::new();
    for src in w["fontes"].as_array().into_iter().flatten() {
        if src["cat"] != "linguas" {
            continue;
        }
        let base = src["caminho"].as_str().unwrap_or("").trim_matches('/').to_string();
        let (md, _) = source_files(root, src);
        for f in md {
            let Ok(mut txt) = std::fs::read_to_string(root.join(&f)) else { continue };
            let mut offset = 0;
            if let Some(secao) = src["secao"].as_str().filter(|s| !s.is_empty()) {
                let Some(m) = Regex::new(&format!(r"(?im)^#{{1,6}}\s+{}", regex::escape(secao))).unwrap().find(&txt) else { continue };
                let lvl = txt[m.start()..].chars().take_while(|c| *c == '#').count();
                let rest = &txt[m.start() + 1..];
                let end = Regex::new(&format!(r"(?m)^#{{1,{lvl}}}\s")).unwrap().find(rest).map(|e| m.start() + 1 + e.start()).unwrap_or(txt.len());
                offset = txt[..m.start()].lines().count();
                txt = txt[m.start()..end].to_string();
            }
            let parent = f.rsplit_once('/').map(|x| x.0).unwrap_or("");
            let in_sub = src["tipo"] == "pasta" && parent != base;
            let fname = basename(&f);
            let lang = if in_sub {
                parent.rsplit('/').next().unwrap_or(parent).to_string()
            } else if Regex::new(r"(?i)l[eé]xico.?compar").unwrap().is_match(&fname) {
                "Comparado".into()
            } else if src["tipo"] == "pasta" {
                "Visão geral".into()
            } else {
                src["rotulo"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| clean_title(&fname.replace('_', " ")))
            };
            for t in tables(&txt) {
                for (cells, line) in &t.rows {
                    let text = deaccent(&cells.join(" ").replace(['*', '`'], ""));
                    out.push(LexRow { lang: lang.clone(), path: f.clone(), head: t.head.clone(), cols: t.cols.clone(), cells: cells.clone(), line: offset + line, text });
                }
            }
        }
    }
    out
}

// ── family trees ─────────────────────────────────────────────────────────

pub const TREE_W: f32 = 190.0;
pub const TREE_H: f32 = 150.0;

/// Positions of the people of a tree: generation by parents, couples side
/// by side, children under the middle of their parents. Returns id → (x, y)
/// and the whole size.
pub fn family_layout(pessoas: &[Json]) -> (BTreeMap<String, (f32, f32, usize)>, f32, f32) {
    let id = |p: &Json| p["id"].as_str().unwrap_or("").to_string();
    let ids: Vec<String> = pessoas.iter().map(id).collect();
    let has = |x: &str| ids.iter().any(|i| i == x);
    let list = |p: &Json, k: &str| -> Vec<String> { p[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).filter(|x| has(x)).collect() };
    let mut gen: HashMap<String, usize> = ids.iter().map(|i| (i.clone(), 0)).collect();
    for _ in 0..pessoas.len() + 2 {
        let mut changed = false;
        for p in pessoas {
            let ps = list(p, "pais");
            let mut g = if ps.is_empty() { gen[&id(p)] } else { ps.iter().map(|x| gen[x]).max().unwrap() + 1 };
            if ps.is_empty() {
                for s in list(p, "conjuges") {
                    if pessoas.iter().find(|q| id(q) == s).is_some_and(|q| !list(q, "pais").is_empty()) {
                        g = g.max(gen[&s]);
                    }
                }
            }
            if g != gen[&id(p)] {
                gen.insert(id(p), g);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let maxg = gen.values().copied().max().unwrap_or(0);
    let order: HashMap<String, usize> = ids.iter().enumerate().map(|(i, x)| (x.clone(), i)).collect();
    let mut pos: BTreeMap<String, (f32, f32, usize)> = BTreeMap::new();
    for r in 0..=maxg {
        let row: Vec<&Json> = pessoas.iter().filter(|p| gen[&id(p)] == r).collect();
        if row.is_empty() {
            continue;
        }
        let key = |p: &Json, pos: &BTreeMap<String, (f32, f32, usize)>| -> f64 {
            let ps: Vec<f32> = list(p, "pais").iter().filter_map(|x| pos.get(x).map(|v| v.0)).collect();
            if !ps.is_empty() {
                return (ps.iter().sum::<f32>() / ps.len() as f32) as f64;
            }
            list(p, "conjuges").iter().find_map(|x| pos.get(x).map(|v| v.0 as f64)).unwrap_or(1e9 + order[&id(p)] as f64)
        };
        let in_tree_parents = |p: &Json| !list(p, "pais").is_empty();
        let mut main: Vec<&Json> = row.iter().copied().filter(|p| in_tree_parents(p) || !list(p, "conjuges").iter().any(|s| row.iter().any(|q| id(q) == *s && in_tree_parents(q)))).collect();
        main.sort_by(|a, b| key(a, &pos).partial_cmp(&key(b, &pos)).unwrap_or(std::cmp::Ordering::Equal).then(order[&id(a)].cmp(&order[&id(b)])));
        let mut seq: Vec<&Json> = Vec::new();
        let mut placed: Vec<String> = Vec::new();
        for p in main {
            if placed.contains(&id(p)) {
                continue;
            }
            seq.push(p);
            placed.push(id(p));
            for s in list(p, "conjuges") {
                if let Some(q) = row.iter().find(|x| id(x) == s) {
                    if !placed.contains(&s) {
                        seq.push(q);
                        placed.push(s);
                    }
                }
            }
        }
        for p in &row {
            if !placed.contains(&id(p)) {
                seq.push(p);
                placed.push(id(p));
            }
        }
        let mut prev = f32::NEG_INFINITY;
        for p in seq {
            let ps: Vec<f32> = list(p, "pais").iter().filter_map(|x| pos.get(x).map(|v| v.0)).collect();
            let sp = list(p, "conjuges").into_iter().find(|x| pos.get(x).is_some_and(|v| v.2 == r));
            let mut want = if !ps.is_empty() { ps.iter().sum::<f32>() / ps.len() as f32 } else if let Some(s) = sp { pos[&s].0 + TREE_W } else { prev + TREE_W };
            if !want.is_finite() {
                want = 0.0;
            }
            let x = want.max(prev + TREE_W);
            pos.insert(id(p), (x, r as f32 * TREE_H, r));
            prev = x;
        }
    }
    let min_x = pos.values().map(|v| v.0).fold(f32::INFINITY, f32::min);
    let max_x = pos.values().map(|v| v.0).fold(f32::NEG_INFINITY, f32::max);
    let max_y = pos.values().map(|v| v.1).fold(0.0, f32::max);
    let min_x = if min_x.is_finite() { min_x } else { 0.0 };
    for v in pos.values_mut() {
        v.0 -= min_x;
    }
    let w = if max_x.is_finite() { max_x - min_x } else { 0.0 } + TREE_W;
    (pos, w, max_y + TREE_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_ranges() {
        assert_eq!(clean_title("3. Os Povos das Marés (autônimo: **Tahélin**)"), "Os Povos das Marés (autônimo: Tahélin)");
        assert_eq!(classify("Os povos — macrogrupos etnolinguísticos", ""), "povos");
        assert_eq!(classify("Coisa qualquer", ""), "visao");
        assert_eq!(terms("A Casa **Vanasbern** — dinastia"), vec!["Vanasbern".to_string(), "Casa Vanasbern".to_string()]);
        assert_eq!(parse_range("≈200 000 – 12 000 AC", "", ""), Some((-200000.0, -12000.0)));
        assert_eq!(parse_range("≈1 100 – 2 840 DC", "", ""), Some((1100.0, 2840.0)));
        assert_eq!(parse_range("300 AC – 150", "", ""), Some((-300.0, 150.0)));
        assert_eq!(year(-3000.0, &json!({})), "3.000 AC");
    }

    #[test]
    fn calendar() {
        let c = json!({ "meses": (0..12).map(|i| json!({ "nome": format!("M{i}"), "dias": 30 })).collect::<Vec<_>>(), "intercalar": { "nome": "X", "dias": 30, "depois": 6, "ciclo": 19, "anos": [3, 6, 8, 11, 14, 17, 19] }, "semana": [{ "nome": "a" }, { "nome": "b" }, { "nome": "c" }] });
        let cal = Calendar { c: &c };
        assert!(cal.is_leap(2));
        assert!(!cal.is_leap(0));
        assert_eq!(cal.year_len(2), 390);
        assert_eq!(cal.cycle(), (19, 360 * 19 + 7 * 30));
        for abs in [-5000, -1, 0, 1, 359, 360, 1000, 123456] {
            let (y, mi, d) = cal.from_abs(abs);
            assert_eq!(cal.to_abs(y, mi, d), abs);
        }
        assert_eq!(cal.months_of(2)[6].nome, "X");
        assert_eq!(cal.add(1, 11, 30, 1, 'd'), (2, 0, 1));
    }

    #[test]
    fn codex() {
        let dir = std::env::temp_dir().join(format!("ateliet-mundo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Mundo/Línguas")).unwrap();
        std::fs::write(dir.join("Mundo/Bíblia.md"), "# Bíblia do Mundo\n\n## Geografia\nTerras altas.\n### O Reino de **Sadaka**\nUm reino.\n## Religião e mito\nDeuses.\n").unwrap();
        std::fs::write(dir.join("Mundo/Línguas/Qayano.md"), "# Léxico\n\n| Palavra | Sentido |\n|---|---|\n| **ahn** | água |\n| tol | pedra |\n").unwrap();
        let w = json!({ "id": "w1", "fontes": [{ "tipo": "nota", "caminho": "Mundo/Bíblia.md", "rotulo": "Bíblia" }, { "tipo": "pasta", "caminho": "Mundo/Línguas", "cat": "linguas" }] });
        let cx = index(&dir, &w, &blank());
        let geo = cx.get("Mundo/Bíblia.md#Geografia").unwrap();
        assert_eq!(geo.cat, "geografia");
        assert_eq!(geo.children, vec!["Mundo/Bíblia.md#O Reino de **Sadaka**".to_string()]);
        assert_eq!(cx.get(&geo.children[0]).unwrap().terms, vec!["Sadaka".to_string(), "Reino de Sadaka".to_string()]);
        assert_eq!(cx.get("Mundo/Bíblia.md#Religião e mito").unwrap().cat, "religiao");
        assert!(cx.get("Mundo/Línguas/Qayano.md").is_some());
        let lex = lexicon(&dir, &w);
        assert_eq!(lex.len(), 2);
        assert_eq!(lex[0].lang, "Visão geral");
        let hits = full_search(&dir, &cx, "deuses");
        assert!(hits.contains_key("Mundo/Bíblia.md#Religião e mito"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tree() {
        let ps = vec![json!({ "id": "a", "conjuges": ["b"] }), json!({ "id": "b", "conjuges": ["a"] }), json!({ "id": "c", "pais": ["a", "b"] }), json!({ "id": "d", "pais": ["a", "b"] })];
        let (pos, w, h) = family_layout(&ps);
        assert_eq!(pos["c"].2, 1);
        assert_eq!(pos["b"].0 - pos["a"].0, TREE_W);
        assert!(w >= 2.0 * TREE_W && h == 2.0 * TREE_H);
    }
}
