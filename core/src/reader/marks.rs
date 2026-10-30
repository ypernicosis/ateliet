//! Highlights (grifos) kept in the vault, not in a hidden database: each
//! book read gets, with its first highlight or excerpt, a note (in
//! `Livros/`) with the book in its frontmatter, every highlight as a
//! callout followed by its block id, and the places in a JSON block the
//! Obsidian reading view hides:
//!
//! ```markdown
//! > [!grifo|amarelo] cap. 3 · p. 58
//! > O trigo domesticou o homem, e não o contrário.
//!
//! ^g-7k2
//!
//! %% ateliet:leitor
//! {"v":1,"marks":{"g-7k2":{"local":{…},"fim":1234,"cor":"amarelo","criado":"…"}}}
//! %%
//! ```
//!
//! So the highlights show in Obsidian, reach search and Dataview and can
//! be linked (`[[Sapiens#^g-7k2]]`). Deleting a callout in Obsidian deletes
//! the highlight; changing its colour in the header changes it here.

use super::{Locator, Meta};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const FOLDER: &str = "Livros";
pub const COLORS: [&str; 4] = ["amarelo", "azul", "verde", "rosa"];

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub id: String,
    /// Where it starts (with the quote).
    pub loc: Locator,
    /// Where it ends: letters in an EPUB chapter, a glyph's index on a
    /// PDF page (inclusive).
    pub end: usize,
    pub color: String,
    pub note: String,
    pub created: String,
    pub text: String,
    /// "cap. 3 · p. 58".
    pub label: String,
}

impl Mark {
    pub fn new(loc: Locator, end: usize, color: &str, text: &str, label: &str) -> Mark {
        Mark { id: format!("g-{}", short()), loc, end, color: color.to_string(), note: String::new(), created: chrono::Local::now().format("%Y-%m-%dT%H:%M").to_string(), text: text.trim().to_string(), label: label.to_string() }
    }

    fn callout(&self) -> String {
        let mut s = format!("> [!grifo|{}] {}\n", self.color, self.label.trim());
        for l in self.text.lines() {
            s.push_str("> ");
            s.push_str(l);
            s.push('\n');
        }
        if !self.note.trim().is_empty() {
            s.push_str(">\n");
            for (i, l) in self.note.trim().lines().enumerate() {
                s.push_str(if i == 0 { "> Nota: " } else { "> " });
                s.push_str(l);
                s.push('\n');
            }
        }
        s.push_str(&format!("\n^{}\n", self.id));
        s
    }
}

fn short() -> String {
    let id = crate::journal::short_id();
    id.chars().filter(|c| c.is_ascii_alphanumeric()).take(4).collect::<String>().to_lowercase()
}

/// A file name from a title (no characters the vault refuses).
pub fn clean_name(s: &str) -> String {
    let c: String = s.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']' | '\n' | '\r')).collect();
    let c: String = c.trim().chars().take(90).collect();
    let c = c.trim().trim_end_matches('.').to_string();
    if c.is_empty() { "Livro".into() } else { c }
}

/// A new book note for a book file of the vault.
pub fn new_note(meta: &Meta, file: &str) -> String {
    let mut fm: Vec<(&str, Value)> = vec![("leitor", json!("livro")), ("titulo", json!(meta.title))];
    if !meta.author.is_empty() {
        fm.push(("autor", json!(meta.author)));
    }
    if !meta.isbn.is_empty() {
        fm.push(("isbn", json!(meta.isbn)));
    }
    fm.push(("arquivo", json!(format!("[[{file}]]"))));
    fm.push(("criado", json!(chrono::Local::now().format("%Y-%m-%d").to_string())));
    let body = format!("\n# {}\n\n[[{file}|Abrir no Leitor]]\n\n", meta.title);
    crate::scriptorium::note_with(&fm, &body)
}

/// Is this note the book note of `file`? (Its `arquivo` links to it.)
pub fn is_note_of(text: &str, file: &str) -> bool {
    let Some(fm) = crate::syntax::frontmatter(text) else { return false };
    let Some(y) = crate::syntax::parse_yaml(&text[fm.yaml.clone()]) else { return false };
    if y["leitor"].as_str() != Some("livro") {
        return false;
    }
    let a = y["arquivo"].as_str().unwrap_or("").trim_start_matches("[[").trim_end_matches("]]");
    let a = a.split('|').next().unwrap_or(a);
    a == file || (!a.contains('/') && file.rsplit('/').next() == Some(a))
}

const OPEN: &str = "%% ateliet:leitor";

/// The JSON block: (where it starts, where it ends, its data).
fn data_block(text: &str) -> Option<(usize, usize, Value)> {
    let a = text.find(OPEN)?;
    let body = &text[a + OPEN.len()..];
    let e = body.find("%%")?;
    let v: Value = serde_json::from_str(body[..e].trim()).unwrap_or(json!({}));
    let mut end = a + OPEN.len() + e + 2;
    if text[end..].starts_with('\n') {
        end += 1;
    }
    Some((a, end, v))
}

/// The callouts of the note: id → (first line, last line (the id's), colour, note).
fn callouts(lines: &[&str]) -> HashMap<String, (usize, usize, String, String)> {
    let mut out = HashMap::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        let Some(id) = t.strip_prefix('^').filter(|id| id.starts_with("g-") && !id.contains(' ')) else { continue };
        // the callout just above (a blank line between)
        let mut j = i;
        while j > 0 && lines[j - 1].trim().is_empty() {
            j -= 1;
        }
        let end_q = j;
        while j > 0 && lines[j - 1].trim_start().starts_with('>') {
            j -= 1;
        }
        if j == end_q {
            continue;
        }
        let head = lines[j].trim_start();
        let color = head.strip_prefix("> [!grifo|").and_then(|r| r.split(']').next()).unwrap_or("amarelo").trim().to_string();
        let note = lines[j..end_q].iter().find_map(|l| l.trim_start().strip_prefix("> Nota:")).map(|n| n.trim().to_string()).unwrap_or_default();
        out.insert(id.to_string(), (j, i, color, note));
    }
    out
}

/// The highlights of a book note (those whose callout is still there).
pub fn read(text: &str) -> Vec<Mark> {
    let Some((_, _, data)) = data_block(text) else { return Vec::new() };
    let lines: Vec<&str> = text.lines().collect();
    let found = callouts(&lines);
    let mut out = Vec::new();
    if let Some(m) = data["marks"].as_object() {
        for (id, v) in m {
            let Some((_, _, color, note)) = found.get(id) else { continue };
            let Some(loc) = v.get("local").and_then(|l| serde_json::from_value::<Locator>(l.clone()).ok()) else { continue };
            out.push(Mark {
                id: id.clone(),
                loc,
                end: v["fim"].as_u64().unwrap_or(0) as usize,
                color: if COLORS.contains(&color.as_str()) { color.clone() } else { v["cor"].as_str().unwrap_or("amarelo").to_string() },
                note: if note.is_empty() { v["nota"].as_str().unwrap_or("").to_string() } else { note.clone() },
                created: v["criado"].as_str().unwrap_or("").to_string(),
                text: v["texto"].as_str().unwrap_or("").to_string(),
                label: v["rotulo"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    out.sort_by_key(|m| (m.loc.chap, m.loc.page, m.loc.offset));
    out
}

/// The note with these highlights: callouts of removed ones taken out,
/// changed ones written again, new ones added at the end; the rest of the
/// note (what was written in it) kept.
pub fn write(text: &str, marks: &[Mark]) -> String {
    let (head, old) = match data_block(text) {
        Some((a, e, v)) => (format!("{}{}", &text[..a], &text[e..]), v),
        None => (text.to_string(), json!({})),
    };
    let lines: Vec<&str> = head.lines().collect();
    let found = callouts(&lines);
    let by_id: HashMap<&str, &Mark> = marks.iter().map(|m| (m.id.as_str(), m)).collect();
    // what changed here since the note was written
    let changed: HashSet<&str> = marks.iter().filter(|m| {
        let o = &old["marks"][&m.id];
        o.is_null() || o["cor"].as_str() != Some(m.color.as_str()) || o["nota"].as_str().unwrap_or("") != m.note
    }).map(|m| m.id.as_str()).collect();
    let mut skip: Vec<(usize, usize, Option<&Mark>)> = found.iter().filter_map(|(id, (a, e, _, _))| match by_id.get(id.as_str()) {
        None => Some((*a, *e, None)),
        Some(m) if changed.contains(id.as_str()) => Some((*a, *e, Some(*m))),
        _ => None,
    }).collect();
    skip.sort_by_key(|x| x.0);
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((a, e, m)) = skip.iter().find(|x| x.0 == i) {
            if let Some(m) = m {
                out.push_str(&m.callout());
            } else {
                // the blank line after a removed callout goes too
                if lines.get(e + 1).is_some_and(|l| l.trim().is_empty()) {
                    i = e + 2;
                    continue;
                }
            }
            i = e + 1;
            let _ = a;
            continue;
        }
        out.push_str(lines[i]);
        out.push('\n');
        i += 1;
    }
    let fresh: Vec<&Mark> = marks.iter().filter(|m| !found.contains_key(&m.id)).collect();
    for m in fresh {
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&m.callout());
    }
    let mut data = serde_json::Map::new();
    for m in marks {
        data.insert(m.id.clone(), json!({ "local": m.loc.to_json(), "fim": m.end, "cor": m.color, "nota": m.note, "criado": m.created, "texto": m.text, "rotulo": m.label }));
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(OPEN);
    out.push('\n');
    out.push_str(&serde_json::to_string(&json!({ "v": 1, "marks": data })).unwrap_or_default());
    out.push_str("\n%%\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_round_trip() {
        let meta = Meta { title: "Sapiens".into(), author: "Yuval Noah Harari".into(), ..Default::default() };
        let note = new_note(&meta, "Livros/Arquivos/Sapiens.epub");
        assert!(is_note_of(&note, "Livros/Arquivos/Sapiens.epub"));
        let loc = Locator { file: "Livros/Arquivos/Sapiens.epub".into(), chap: Some(2), offset: Some(100), ..Default::default() };
        let a = Mark::new(loc.clone(), 140, "amarelo", "O trigo domesticou o homem.", "cap. 3 · p. 58");
        let mut b = Mark::new(loc, 300, "azul", "Outro trecho.", "cap. 3");
        b.note = "comparar com Mossy Earth".into();
        let t = write(&note, &[a.clone(), b.clone()]);
        assert!(t.contains("> [!grifo|amarelo] cap. 3 · p. 58\n> O trigo domesticou o homem.\n\n^"));
        assert!(t.contains("> Nota: comparar com Mossy Earth"));
        let back = read(&t);
        assert_eq!(back.len(), 2);
        assert_eq!(back.iter().find(|m| m.id == a.id).unwrap().end, 140);
        // written in the note in Obsidian: kept
        let t = t.replace("# Sapiens\n", "# Sapiens\n\nMinhas impressões.\n");
        // the colour changed in Obsidian
        let t = t.replace("[!grifo|azul]", "[!grifo|verde]");
        let back = read(&t);
        assert_eq!(back.iter().find(|m| m.id == b.id).unwrap().color, "verde");
        // the callout deleted in Obsidian: the highlight goes
        let cut = t.find("> [!grifo|amarelo]").unwrap();
        let id_line = format!("^{}\n", a.id);
        let end = t.find(&id_line).unwrap() + id_line.len();
        let t2 = format!("{}{}", &t[..cut], &t[end..]);
        assert_eq!(read(&t2).len(), 1);
        // removed in the reader: its callout goes, the rest stays
        let t3 = write(&t, &[back.into_iter().find(|m| m.id == b.id).unwrap()]);
        assert!(!t3.contains(&a.id));
        assert!(t3.contains("Minhas impressões."));
        assert!(t3.contains("[!grifo|verde]"));
        assert_eq!(read(&t3).len(), 1);
    }
}
