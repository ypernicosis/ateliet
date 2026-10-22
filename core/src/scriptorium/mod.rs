//! Scriptorium: books, chapters, characters and writing statistics, on top of
//! plain notes. The data lives where the Obsidian plugin keeps it (frontmatter
//! of the notes and `.obsidian/plugins/scriptorium-omena/data.json`), so a
//! vault works the same in both.

pub mod book;
pub mod css;
pub mod docx;
pub mod enredo;
pub mod estruturas;
pub mod export;
pub mod manuscript;
pub mod mundo;
pub mod sheet;
pub mod stats;

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value as Json;

static FRONTMATTER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\u{feff}?---\r?\n(?s:.*?)\r?\n---[ \t]*(\r?\n|$)").unwrap());
static WORD: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\p{L}\p{N}][\p{L}\p{N}'’\-]*").unwrap());

pub fn strip_frontmatter(text: &str) -> &str {
    match FRONTMATTER.find(text) {
        Some(m) => &text[m.end()..],
        None => text,
    }
}

/// Words of prose: without frontmatter, comments, code, embeds or markup.
pub fn count_words(text: &str) -> usize {
    static CLEAN: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
        [
            (r"(?s)%%.*?%%", " "),
            (r"(?s)```.*?```", " "),
            (r"(?s)<!--.*?-->", " "),
            (r"!\[\[[^\]]*\]\]", " "),
            (r"\[\[(?:[^\]|]*\|)?([^\]]*)\]\]", "$1"),
            (r"\]\{[^}]*\}", " "),
            (r"<[^>]+>", " "),
            (r"(?m)^\s*>\s*\[![^\]]*\][+-]?", " "),
            (r"https?://\S+", " "),
        ]
        .into_iter()
        .map(|(re, to)| (Regex::new(re).unwrap(), to))
        .collect()
    });
    let mut t = strip_frontmatter(text).to_string();
    for (re, to) in CLEAN.iter() {
        t = re.replace_all(&t, *to).into_owned();
    }
    WORD.find_iter(&t).count()
}

/// Scene breaks (`***`, `---`, `___` lines) plus one.
pub fn count_scenes(body: &str) -> usize {
    static BREAK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*([-*_])(\s*[-*_]){2,}\s*$").unwrap());
    1 + BREAK.find_iter(body).filter(|m| {
        let s: String = m.as_str().chars().filter(|c| !c.is_whitespace()).collect();
        s.chars().all(|c| c == s.chars().next().unwrap())
    }).count()
}

pub fn safe_name(s: &str) -> String {
    let t: String = s.chars().filter(|c| !"\\/:*?\"<>|#^[]".contains(*c)).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { "sem-titulo".into() } else { t }
}

pub fn pad2(n: usize) -> String {
    format!("{n:02}")
}

pub fn roman(mut n: usize) -> String {
    let map = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut out = String::new();
    for (v, s) in map {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// A number in words, up to 999 ("vinte e três").
pub fn extenso(n: usize) -> String {
    const U: [&str; 20] = ["zero", "um", "dois", "três", "quatro", "cinco", "seis", "sete", "oito", "nove", "dez", "onze", "doze", "treze", "quatorze", "quinze", "dezesseis", "dezessete", "dezoito", "dezenove"];
    const D: [&str; 10] = ["", "", "vinte", "trinta", "quarenta", "cinquenta", "sessenta", "setenta", "oitenta", "noventa"];
    const C: [&str; 10] = ["", "cento", "duzentos", "trezentos", "quatrocentos", "quinhentos", "seiscentos", "setecentos", "oitocentos", "novecentos"];
    match n {
        0..=19 => U[n].into(),
        20..=99 => format!("{}{}", D[n / 10], if n % 10 > 0 { format!(" e {}", U[n % 10]) } else { String::new() }),
        100 => "cem".into(),
        101..=999 => format!("{}{}", C[n / 100], if n % 100 > 0 { format!(" e {}", extenso(n % 100)) } else { String::new() }),
        _ => n.to_string(),
    }
}

/// Thousands with a dot, as in pt-BR ("12.345").
pub fn fmt_n(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `[[Target|Label]]`, a link object or plain text → the link's target.
pub fn link_text(v: &Json) -> String {
    let s = match v {
        Json::Null => return String::new(),
        Json::String(s) => s.clone(),
        Json::Object(o) => return o.get("path").and_then(|p| p.as_str()).unwrap_or_default().to_string(),
        Json::Array(a) if a.len() == 1 => {
            // YAML reads an unquoted [[x]] as a nested list
            if let Json::Array(b) = &a[0] {
                if let Some(Json::String(s)) = b.first() {
                    return s.split('|').next().unwrap_or_default().split('#').next().unwrap_or_default().trim().to_string();
                }
            }
            return link_text(&a[0]);
        }
        other => other.to_string(),
    };
    let s = s.trim();
    let s = s.strip_prefix('!').unwrap_or(s);
    let s = s.strip_prefix("[[").unwrap_or(s);
    let s = s.strip_suffix("]]").unwrap_or(s);
    s.split('|').next().unwrap_or_default().split('#').next().unwrap_or_default().trim().to_string()
}

pub fn to_list(v: &Json) -> Vec<Json> {
    match v {
        Json::Null => Vec::new(),
        Json::String(s) if s.is_empty() => Vec::new(),
        Json::Array(a) => a.clone(),
        other => vec![other.clone()],
    }
}

// ── frontmatter editing ────────────────────────────────────────────────────

fn yaml_key(line: &str) -> Option<String> {
    if line.starts_with([' ', '\t', '-', '#']) {
        return None;
    }
    let (k, _) = line.split_once(':')?;
    let k = k.trim();
    let k = k.strip_prefix('"').and_then(|k| k.strip_suffix('"')).or_else(|| k.strip_prefix('\'').and_then(|k| k.strip_suffix('\''))).unwrap_or(k);
    Some(k.to_string())
}

/// `key: value` as YAML lines (with the trailing newline).
pub fn yaml_entry(key: &str, value: &Json) -> String {
    let mut m = serde_json::Map::new();
    m.insert(key.to_string(), value.clone());
    let s = serde_yaml::to_string(&Json::Object(m)).unwrap_or_default();
    if s.ends_with('\n') { s } else { format!("{s}\n") }
}

/// Set (or with `None` remove) one top-level frontmatter key, leaving the
/// other lines exactly as they were.
pub fn set_frontmatter(text: &str, key: &str, value: Option<&Json>) -> String {
    let (yaml, body, bom) = match FRONTMATTER.find(text) {
        Some(m) => {
            let block = m.as_str();
            let bom = block.starts_with('\u{feff}');
            let inner = block.trim_start_matches('\u{feff}');
            let inner = &inner[inner.find('\n').map(|i| i + 1).unwrap_or(0)..];
            let end = inner.rfind("\n---").map(|i| i + 1).unwrap_or(0);
            (inner[..end].to_string(), &text[m.end()..], bom)
        }
        None => {
            if value.is_none() {
                return text.to_string();
            }
            (String::new(), text, false)
        }
    };
    let lines: Vec<&str> = yaml.split_inclusive('\n').collect();
    let mut out = String::new();
    let mut i = 0;
    let mut done = false;
    while i < lines.len() {
        let l = lines[i];
        if yaml_key(l.trim_end_matches(['\r', '\n'])).as_deref() == Some(key) {
            let mut j = i + 1;
            while j < lines.len() && (lines[j].starts_with([' ', '\t', '-']) || lines[j].trim().is_empty()) {
                j += 1;
            }
            if let Some(v) = value {
                if !done {
                    out.push_str(&yaml_entry(key, v));
                }
            }
            done = true;
            i = j;
            continue;
        }
        out.push_str(l);
        i += 1;
    }
    if !done {
        if let Some(v) = value {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&yaml_entry(key, v));
        }
    }
    format!("{}---\n{out}---\n{body}", if bom { "\u{feff}" } else { "" })
}

/// A note with frontmatter built from `fm` (in the given order) and a body.
pub fn note_with(fm: &[(&str, Json)], body: &str) -> String {
    let mut out = String::from("---\n");
    for (k, v) in fm {
        out.push_str(&yaml_entry(k, v));
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

/// The text under `## name` (up to the next `##`).
pub fn body_section(text: &str, name: &str) -> String {
    let body = strip_frontmatter(text);
    let mut out = Vec::new();
    let mut inside = false;
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if inside {
                break;
            }
            inside = h.trim().eq_ignore_ascii_case(name);
            continue;
        }
        if inside {
            out.push(line);
        }
    }
    out.join("\n").trim().to_string()
}

/// Read a ```lang block holding JSON.
pub fn read_json_block(text: &str, lang: &str) -> Option<Json> {
    let re = Regex::new(&format!(r"(?s)```{}[^\n]*\n(.*?)\n```", regex::escape(lang))).ok()?;
    serde_json::from_str(re.captures(text)?.get(1)?.as_str()).ok()
}

/// Replace (or append) a ```lang block holding JSON.
pub fn write_json_block(text: &str, lang: &str, v: &Json) -> String {
    let block = format!("```{lang}\n{}\n```", serde_json::to_string_pretty(v).unwrap_or_default());
    let re = Regex::new(&format!(r"(?s)```{}[^\n]*\n.*?\n```", regex::escape(lang))).unwrap();
    if re.is_match(text) {
        re.replace(text, regex::NoExpand(&block)).into_owned()
    } else {
        format!("{}\n\n{block}\n", text.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn words() {
        assert_eq!(count_words("---\na: 1\n---\nOlá, mundo! [[Nota|dois]] %%oculto%% d'água"), 4);
        assert_eq!(count_scenes("a\n\n***\n\nb\n\n- - -\n"), 3);
    }

    #[test]
    fn frontmatter_edit() {
        let t = "---\ntitulo: X\nstatus: rascunho\ntags:\n  - a\nordem: 2\n---\ncorpo\n";
        let t2 = set_frontmatter(t, "status", Some(&json!("final")));
        assert_eq!(t2, "---\ntitulo: X\nstatus: final\ntags:\n  - a\nordem: 2\n---\ncorpo\n");
        let t3 = set_frontmatter(&t2, "tags", None);
        assert_eq!(t3, "---\ntitulo: X\nstatus: final\nordem: 2\n---\ncorpo\n");
        let t4 = set_frontmatter("corpo", "parte", Some(&json!("Ato I")));
        assert_eq!(t4, "---\nparte: Ato I\n---\ncorpo");
        assert_eq!(set_frontmatter(&t3, "incluir", Some(&json!(false))), "---\ntitulo: X\nstatus: final\nordem: 2\nincluir: false\n---\ncorpo\n");
    }

    #[test]
    fn numbers() {
        assert_eq!(extenso(23), "vinte e três");
        assert_eq!(extenso(101), "cento e um");
        assert_eq!(roman(14), "XIV");
        assert_eq!(fmt_n(1234567), "1.234.567");
        assert_eq!(link_text(&json!("[[Livro|L]]")), "Livro");
        assert_eq!(body_section("---\n---\n## Sinopse\n\nUm dia.\n\n## Notas\nx", "sinopse"), "Um dia.");
    }
}
