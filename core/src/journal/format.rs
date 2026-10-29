//! A notebook's index note and its page notes, read and written so that a
//! note read and written back unchanged is the same text, and an edit
//! touches only what it changes.
//!
//! A page:
//!
//! ```text
//! ---
//! journal: página
//! caderno: "[[Natureza]]"
//! pagina: 3
//! ---
//!
//! ## ^j-txt1
//! Text on the ruling, in Markdown.
//!
//! ## ^j-img1
//! ![[Salentin.jpg]]
//! Hubert Salentin, 1882.
//!
//! %% ateliet:journal
//! {"v":1,"items":[{"id":"txt1","kind":"texto","x":…}, …]}
//! %%
//! ```
//!
//! The json keeps what is not text; `` ` `` and `%` are written as ```
//! and `%` so the comment can't end early or start a code block.

use super::paper::{Columns, PageSize, Paper};
use super::Item;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const BLOCK: &str = "%% ateliet:journal";
pub const VERSION: u64 = 1;

/// The `journal:` value of a note's frontmatter (`caderno`, `página`).
pub fn journal_kind(text: &str) -> Option<String> {
    let fm = crate::syntax::frontmatter(text)?;
    for l in text[fm.yaml].lines() {
        if let Some(rest) = l.strip_prefix("journal:") {
            let v = rest.trim().trim_matches(['"', '\'']).to_lowercase();
            return Some(match v.as_str() {
                "pagina" | "page" => "página".into(),
                "notebook" => "caderno".into(),
                _ => v,
            });
        }
    }
    None
}

pub fn is_notebook(text: &str) -> bool {
    journal_kind(text).as_deref() == Some("caderno")
}

pub fn is_page(text: &str) -> bool {
    journal_kind(text).as_deref() == Some("página")
}

/// Reads only the start of a file: is it a notebook's index note?
pub fn is_notebook_file(abs: &Path) -> bool {
    head_of(abs).is_some_and(|t| is_notebook(&t))
}

fn head_of(abs: &Path) -> Option<String> {
    use std::io::Read;
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(abs).and_then(|mut f| f.read(&mut head)).ok()?;
    Some(String::from_utf8_lossy(&head[..n]).to_string())
}

fn yaml(text: &str) -> Map<String, Value> {
    crate::syntax::frontmatter(text).and_then(|fm| crate::syntax::parse_yaml(&text[fm.yaml])).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

// ── the notebook ──────────────────────────────────────────────────────────

/// A page of a notebook as the index lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct PageRef {
    /// The note, vault-relative.
    pub path: String,
    /// `pagina` of its frontmatter.
    pub number: usize,
    /// The file name without its number and `.md` (often empty).
    pub title: String,
    pub date: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notebook {
    /// The index note, vault-relative, and the folder it is in.
    pub path: String,
    pub folder: String,
    pub name: String,
    pub size: PageSize,
    pub paper: Paper,
    pub columns: Columns,
    /// The cover's colour (`#rrggbb`) and picture.
    pub cover: String,
    pub cover_image: Option<String>,
    pub start: Option<String>,
    /// The page it opens at.
    pub marker: usize,
    /// A diary: "today's page" adds a dated page at the end.
    pub daily: bool,
    pub pages: Vec<PageRef>,
}

pub const COVERS: &[&str] = &["#2f4a3a", "#3b4a6b", "#6b3b3b", "#8a6a3a", "#4a3b5c", "#2e2e33", "#b9b2a5", "#5f7a74"];

impl Notebook {
    /// The index note's settings; None when it is not a notebook.
    pub fn parse(path: &str, text: &str) -> Option<Notebook> {
        if !is_notebook(text) {
            return None;
        }
        let fm = yaml(text);
        let get = |k: &str| fm.get(k).and_then(str_of);
        let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string();
        let folder = path.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default();
        Some(Notebook {
            path: path.to_string(),
            folder,
            name,
            size: PageSize::parse(&get("tamanho").unwrap_or_else(|| "A5".into())),
            paper: Paper::parse(&get("papel").unwrap_or_else(|| "grade-5mm".into())),
            columns: Columns::parse(&get("colunas").unwrap_or_default()),
            cover: get("capa").filter(|c| c.starts_with('#')).unwrap_or_else(|| COVERS[0].to_string()),
            cover_image: get("capa-imagem").map(|s| s.trim_start_matches("[[").trim_end_matches("]]").to_string()),
            start: get("inicio"),
            marker: fm.get("marcador").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(1) as usize,
            daily: fm.get("diario").and_then(|v| v.as_bool()).unwrap_or(false),
            pages: Vec::new(),
        })
    }

    /// The text of a new index note.
    pub fn new_text(folder: &str, size: &str, paper: &str, columns: &str, cover: &str, start: &str, daily: bool) -> String {
        let mut fm = vec![
            ("journal", json!("caderno")),
            ("tamanho", json!(size)),
            ("papel", json!(paper)),
            ("colunas", json!(columns)),
            ("capa", json!(cover)),
            ("inicio", json!(start)),
            ("marcador", json!(1)),
        ];
        if daily {
            fm.push(("diario", json!(true)));
        }
        let body = format!("\n```dataview\nTABLE WITHOUT ID pagina AS \"p.\", file.link AS \"Página\", data\nFROM \"{}\" WHERE journal = \"página\" SORT pagina\n```\n", folder.replace('"', ""));
        crate::scriptorium::note_with(&fm.iter().map(|(k, v)| (*k, v.clone())).collect::<Vec<_>>(), &body)
    }

    /// Reads the pages in the notebook's folder (`root` is the vault),
    /// in their order.
    pub fn scan(&mut self, root: &Path) {
        self.pages.clear();
        let dir = if self.folder.is_empty() { root.to_path_buf() } else { root.join(&self.folder) };
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.to_lowercase().ends_with(".md") || name.starts_with('.') {
                continue;
            }
            let rel = if self.folder.is_empty() { name.clone() } else { format!("{}/{name}", self.folder) };
            if rel == self.path {
                continue;
            }
            let Some(head) = head_of(&e.path()) else { continue };
            if !is_page(&head) {
                continue;
            }
            // the frontmatter may be longer than the head
            let text = if crate::syntax::frontmatter(&head).is_some() { head } else { std::fs::read_to_string(e.path()).unwrap_or_default() };
            let fm = yaml(&text);
            let number = fm.get("pagina").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0) as usize;
            self.pages.push(PageRef { path: rel, number, title: page_title(&name), date: fm.get("data").and_then(str_of) });
        }
        self.pages.sort_by(|a, b| a.number.cmp(&b.number).then_with(|| a.path.cmp(&b.path)));
    }

    /// The file a page of this notebook should have, for its place.
    pub fn page_path(&self, number: usize, title: &str) -> String {
        let name = page_file_name(number, title, self.pages.len().max(number));
        if self.folder.is_empty() {
            name
        } else {
            format!("{}/{name}", self.folder)
        }
    }

    /// After a reorder: the renames that bring the file names in line with
    /// the order (`pages` is already in the new order), old → new.
    pub fn renames(&self) -> Vec<(String, String)> {
        self.pages.iter().enumerate().filter_map(|(i, p)| {
            let want = self.page_path(i + 1, &p.title);
            (want != p.path).then(|| (p.path.clone(), want))
        }).collect()
    }
}

/// `003 Bétulas na Escócia.md`, `003.md`; at least three digits.
pub fn page_file_name(number: usize, title: &str, count: usize) -> String {
    let digits = count.to_string().len().max(3);
    let clean: String = title.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']')).collect();
    let clean = clean.trim();
    if clean.is_empty() {
        format!("{number:0digits$}.md")
    } else {
        format!("{number:0digits$} {clean}.md")
    }
}

/// A page file's title: its name without the number in front and `.md`.
pub fn page_title(file: &str) -> String {
    let stem = file.strip_suffix(".md").unwrap_or(file);
    let rest = stem.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.len() == stem.len() {
        return stem.to_string();
    }
    rest.trim_start_matches([' ', '-', '_', '.']).to_string()
}

/// Set `pagina` (and keep everything else as it was).
pub fn set_number(text: &str, n: usize) -> String {
    crate::scriptorium::set_frontmatter(text, "pagina", Some(&json!(n)))
}

/// Set the notebook's bookmark.
pub fn set_marker(text: &str, n: usize) -> String {
    crate::scriptorium::set_frontmatter(text, "marcador", Some(&json!(n)))
}

// ── a page ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// The frontmatter and whatever comes before the first item's section.
    pub head: String,
    /// The layers, bottom to top.
    pub items: Vec<Item>,
    /// The sections' ids in the order they are in the note.
    pub order: Vec<String>,
    /// Sections with no item in the json (written in Obsidian): id, text.
    pub loose: Vec<(String, String)>,
    /// The json's other keys (`v`, `papel`, …), `items` in its place.
    pub meta: Map<String, Value>,
    /// The block as read, and its value, to write it back unchanged.
    raw: Option<(String, Value)>,
    /// What follows the block.
    pub tail: String,
}

/// `## ^j-<id>` on a line of its own.
fn section_id(line: &str) -> Option<&str> {
    let rest = line.trim_end_matches('\r').strip_prefix("## ^j-")?.trim_end();
    (!rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(rest)
}

fn escape(json: &str) -> String {
    json.replace('`', "\\u0060").replace('%', "\\u0025")
}

impl Page {
    pub fn empty() -> Page {
        let mut meta = Map::new();
        meta.insert("v".into(), json!(VERSION));
        meta.insert("items".into(), json!([]));
        Page { head: String::new(), items: Vec::new(), order: Vec::new(), loose: Vec::new(), meta, raw: None, tail: String::new() }
    }

    /// A new page's note.
    pub fn new_text(notebook: &str, number: usize, date: &str) -> String {
        let fm = [("journal", json!("página")), ("caderno", json!(format!("[[{notebook}]]"))), ("pagina", json!(number)), ("data", json!(date))];
        let head = crate::scriptorium::note_with(&fm, "\n");
        let mut p = Page::empty();
        p.head = head;
        p.write()
    }

    pub fn parse(text: &str) -> Page {
        let mut page = Page::empty();
        // the block: the last `%% ateliet:journal` line, its json, `%%`
        let mut pre_end = text.len();
        let mut starts = Vec::new();
        let mut pos = 0;
        for l in text.split_inclusive('\n') {
            if l.trim_end() == BLOCK {
                starts.push(pos);
            }
            pos += l.len();
        }
        for &s in starts.iter().rev() {
            let after = &text[s..];
            let first_nl = after.find('\n').map(|i| i + 1).unwrap_or(after.len());
            let mut p = s + first_nl;
            let mut json = String::new();
            let mut end = None;
            for l in text[p..].split_inclusive('\n') {
                if l.trim() == "%%" {
                    end = Some(p + l.len());
                    break;
                }
                json.push_str(l);
                p += l.len();
            }
            let Some(end) = end else { continue };
            if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(json.trim()) {
                page.items = m.get("items").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|i| serde_json::from_value::<Item>(i.clone()).ok()).collect()).unwrap_or_default();
                page.raw = Some((text[s..end].to_string(), Value::Object(m.clone())));
                page.meta = m;
                if !page.meta.contains_key("items") {
                    page.meta.insert("items".into(), json!([]));
                }
                pre_end = s;
                page.tail = text[end..].to_string();
                break;
            }
        }
        let pre = &text[..pre_end];
        // the sections, outside code blocks
        let mut heads: Vec<(usize, usize, String)> = Vec::new();
        let mut pos = 0;
        let mut fence: Option<String> = None;
        let body_from = crate::syntax::frontmatter(pre).map(|f| f.block.end).unwrap_or(0);
        for l in pre.split_inclusive('\n') {
            let t = l.trim_start();
            if pos >= body_from {
                if let Some(f) = &fence {
                    if t.trim_end().starts_with(f.as_str()) {
                        fence = None;
                    }
                } else if t.starts_with("```") || t.starts_with("~~~") {
                    fence = Some(t[..3].to_string());
                } else if let Some(id) = section_id(l.trim_end_matches('\n')) {
                    heads.push((pos, pos + l.len(), id.to_string()));
                }
            }
            pos += l.len();
        }
        page.head = pre[..heads.first().map(|h| h.0).unwrap_or(pre.len())].to_string();
        for (k, (_, body_start, id)) in heads.iter().enumerate() {
            let end = heads.get(k + 1).map(|h| h.0).unwrap_or(pre.len());
            let body = pre[*body_start..end].to_string();
            page.order.push(id.clone());
            match page.items.iter_mut().find(|i| &i.id == id && i.body.is_none()) {
                Some(it) => it.body = Some(body),
                None => page.loose.push((id.clone(), body)),
            }
        }
        page
    }

    fn value(&self) -> Value {
        let mut m = self.meta.clone();
        m.insert("items".into(), Value::Array(self.items.iter().map(|i| serde_json::to_value(i).unwrap_or(Value::Null)).collect()));
        Value::Object(m)
    }

    pub fn write(&self) -> String {
        let mut out = self.head.clone();
        let mut done = std::collections::HashSet::new();
        let section = |out: &mut String, id: &str, body: &str| {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("## ^j-");
            out.push_str(id);
            out.push('\n');
            out.push_str(body);
        };
        for id in &self.order {
            if done.contains(id) {
                continue;
            }
            if let Some((_, body)) = self.loose.iter().find(|(l, _)| l == id) {
                section(&mut out, id, body);
                done.insert(id.clone());
            } else if let Some(it) = self.items.iter().find(|i| &i.id == id) {
                if let Some(b) = &it.body {
                    section(&mut out, id, b);
                    done.insert(id.clone());
                }
            }
        }
        for it in &self.items {
            if let (Some(b), false) = (&it.body, done.contains(&it.id)) {
                section(&mut out, &it.id, b);
                done.insert(it.id.clone());
            }
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        let v = self.value();
        match &self.raw {
            Some((raw, old)) if *old == v => out.push_str(raw),
            _ => {
                if !out.is_empty() && !out.ends_with("\n\n") {
                    out.push('\n');
                }
                out.push_str(BLOCK);
                out.push('\n');
                out.push_str(&escape(&serde_json::to_string(&v).unwrap_or_default()));
                out.push_str("\n%%\n");
            }
        }
        out.push_str(&self.tail);
        out
    }

    /// The json block as it is now (what the backup copy keeps).
    pub fn block_value(&self) -> Value {
        self.value()
    }

    /// Does the note have its block? (A plugin that rewrites the note may
    /// leave it out.)
    pub fn has_block(&self) -> bool {
        self.raw.is_some()
    }

    /// Puts back a block lost from the note, from its backup copy: the
    /// items come back and find their sections again.
    pub fn restore(&mut self, v: &Value) {
        let Some(m) = v.as_object() else { return };
        let items: Vec<Item> = m.get("items").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|i| serde_json::from_value::<Item>(i.clone()).ok()).collect()).unwrap_or_default();
        self.meta = m.clone();
        self.items = items;
        let loose = std::mem::take(&mut self.loose);
        for (id, body) in loose {
            match self.items.iter_mut().find(|i| i.id == id && i.body.is_none()) {
                Some(it) => it.body = Some(body),
                None => self.loose.push((id, body)),
            }
        }
    }

    /// The note's frontmatter, as json.
    pub fn frontmatter(&self) -> Map<String, Value> {
        yaml(&self.head)
    }

    pub fn number(&self) -> usize {
        self.frontmatter().get("pagina").and_then(|v| v.as_u64()).unwrap_or(0) as usize
    }

    /// A paper of its own, when it differs from the notebook's.
    pub fn paper(&self) -> Option<Paper> {
        self.meta.get("papel").and_then(|v| v.as_str()).map(Paper::parse)
    }

    pub fn set_paper(&mut self, p: Option<Paper>) {
        match p {
            Some(p) => {
                self.meta.insert("papel".into(), json!(p.name()));
            }
            None => {
                self.meta.remove("papel");
            }
        }
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// Set a top-level frontmatter key of the page.
    pub fn set_front(&mut self, key: &str, value: Option<&Value>) {
        self.head = crate::scriptorium::set_frontmatter(&self.head, key, value);
    }

    /// Links to the notes the page takes excerpts from (`fontes`), once each.
    pub fn add_source(&mut self, link: &str) {
        let fm = self.frontmatter();
        let mut list: Vec<Value> = fm.get("fontes").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let want = format!("[[{link}]]");
        if list.iter().any(|v| v.as_str() == Some(&want)) {
            return;
        }
        list.push(Value::String(want));
        self.set_front("fontes", Some(&Value::Array(list)));
    }
}

/// A page changed here (`ours`, from `base` as it was read) and on disk
/// (`disk`) at once: what each side changed is kept. Where both changed the
/// same section, the disk's version stays; those ids come back.
pub fn merge(ours: &Page, base: &Page, disk: &Page) -> (Page, Vec<String>) {
    let mut out = ours.clone();
    let mut conflicts = Vec::new();
    out.head = if ours.head != base.head && disk.head == base.head { ours.head.clone() } else { disk.head.clone() };
    out.order = disk.order.clone();
    out.tail = disk.tail.clone();
    out.raw = disk.raw.clone();
    let body_of = |p: &Page, id: &str| -> Option<String> { p.item(id).and_then(|i| i.body.clone()).or_else(|| p.loose.iter().find(|(l, _)| l == id).map(|(_, b)| b.clone())) };
    for it in out.items.iter_mut() {
        let b = body_of(base, &it.id);
        let d = body_of(disk, &it.id);
        if it.body != b {
            if d != b {
                conflicts.push(it.id.clone());
                it.body = d;
            }
        } else {
            it.body = d;
        }
    }
    out.loose = disk.loose.iter().filter(|(id, _)| !out.items.iter().any(|i| &i.id == id)).cloned().collect();
    (out, conflicts)
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Item};
    use super::*;

    const SAMPLE: &str = "---\njournal: página\ncaderno: \"[[Natureza]]\"\npagina: 3\ndata: 2023-11-05\ntags: [escócia, florestas]\n---\n\nIntrodução escrita no Obsidian.\n\n## ^j-txt1\n> [!trecho] The Mountain Birch Project · Mossy Earth\n> Montane woodland is ==rare==. 100%\n\n## ^j-mg1\nEsse vídeo me foi recomendado… #paisagem\n\n```\n## ^j-falso\n```\n\n## ^j-solto\nEscrito no Obsidian.\n\n%% ateliet:journal\n{\"v\":1,\"lado\":\"direita\",\"items\":[{\"id\":\"txt1\",\"kind\":\"trecho\",\"x\":10.0,\"y\":20.0,\"w\":80.0,\"h\":40.0,\"futuro\":{\"a\":1}},{\"id\":\"mg1\",\"kind\":\"texto\",\"x\":5.0,\"y\":20.0,\"w\":30.0,\"h\":60.0},{\"id\":\"img9\",\"kind\":\"imagem\",\"x\":0.0,\"y\":0.0,\"w\":1.0,\"h\":1.0}]}\n%%\n";

    #[test]
    fn unchanged_page_is_the_same_text() {
        let p = Page::parse(SAMPLE);
        assert_eq!(p.items.len(), 3);
        assert_eq!(p.loose.len(), 1);
        assert_eq!(p.items[0].content(), "> [!trecho] The Mountain Birch Project · Mossy Earth\n> Montane woodland is ==rare==. 100%");
        assert!(p.items[1].content().ends_with("```\n## ^j-falso\n```"));
        assert!(p.items[2].body.is_none());
        assert_eq!(p.items[0].extra["futuro"]["a"], 1);
        assert_eq!(p.number(), 3);
        assert_eq!(p.write(), SAMPLE);
    }

    #[test]
    fn edits_round_trip() {
        let mut p = Page::parse(SAMPLE);
        p.items[1].set_content("Outro comentário com `código` e 50%.");
        p.items[1].x = 7.5;
        let mut it = Item::new(Kind::Text, 1.0, 2.0, 3.0, 4.0);
        it.set_content("Novo");
        let id = it.id.clone();
        p.items.push(it);
        p.add_source("Trechos/Mossy Earth");
        let text = p.write();
        assert!(text.contains("fontes:"));
        assert!(!text[text.find(BLOCK).unwrap()..].contains('`'));
        let q = Page::parse(&text);
        assert_eq!(q.items.len(), 4);
        assert_eq!(q.item(&id).unwrap().content(), "Novo");
        assert_eq!(q.items[1].content(), "Outro comentário com `código` e 50%.");
        assert_eq!(q.items[1].x, 7.5);
        assert_eq!(q.meta["lado"], "direita");
        assert_eq!(q.loose.len(), 1);
        assert_eq!(q.write(), text);
    }

    #[test]
    fn a_lost_block_comes_back() {
        let p = Page::parse(SAMPLE);
        let backup = p.block_value();
        let cut = &SAMPLE[..SAMPLE.find(BLOCK).unwrap()];
        let mut q = Page::parse(cut);
        assert!(!q.has_block() && q.items.is_empty() && q.loose.len() == 3);
        q.restore(&backup);
        assert_eq!(q.items.len(), 3);
        assert_eq!(q.loose.len(), 1);
        assert!(q.item("mg1").unwrap().content().starts_with("Esse vídeo"));
    }

    #[test]
    fn both_sides_keep_their_changes() {
        let base = Page::parse(SAMPLE);
        let mut ours = base.clone();
        ours.items[1].x = 99.0;
        ours.items[0].set_content("> [!trecho] Mudado aqui");
        let disk_text = SAMPLE.replace("Esse vídeo me foi recomendado…", "Editado no Obsidian.").replace("pagina: 3", "pagina: 3\nhumor: bom");
        let disk = Page::parse(&disk_text);
        let (m, c) = merge(&ours, &base, &disk);
        assert!(c.is_empty());
        assert_eq!(m.items[1].x, 99.0);
        assert!(m.items[1].content().starts_with("Editado no Obsidian."));
        assert_eq!(m.items[0].content(), "> [!trecho] Mudado aqui");
        assert!(m.head.contains("humor: bom"));
        // the same section changed on both sides: the disk's stays
        let disk2 = Page::parse(&SAMPLE.replace("Montane woodland", "Floresta"));
        let (m2, c2) = merge(&ours, &base, &disk2);
        assert_eq!(c2, vec!["txt1".to_string()]);
        assert!(m2.items[0].content().contains("Floresta"));
    }

    #[test]
    fn a_new_page_reads_back() {
        let t = Page::new_text("Natureza", 2, "2026-10-29");
        assert!(is_page(&t));
        let p = Page::parse(&t);
        assert_eq!(p.number(), 2);
        assert!(p.items.is_empty());
        assert_eq!(p.write(), t);
    }

    #[test]
    fn notebooks() {
        let t = Notebook::new_text("Cadernos/Natureza", "A5", "grade-5mm", "cornell-externa", "#2f4a3a", "2023-11-01", false);
        let nb = Notebook::parse("Cadernos/Natureza/Natureza.md", &t).unwrap();
        assert_eq!(nb.folder, "Cadernos/Natureza");
        assert_eq!(nb.columns, Columns::CornellOuter);
        assert_eq!(nb.marker, 1);
        let t2 = set_marker(&t, 4);
        assert_eq!(Notebook::parse("x.md", &t2).unwrap().marker, 4);
        assert!(t2.contains("```dataview"));
        assert_eq!(page_title("003 Bétulas na Escócia.md"), "Bétulas na Escócia");
        assert_eq!(page_title("002.md"), "");
        assert_eq!(page_file_name(7, "Bétulas", 12), "007 Bétulas.md");
    }

    #[test]
    fn scans_and_renames() {
        let dir = std::env::temp_dir().join(format!("ateliet-journal-{}", super::super::short_id()));
        let nbdir = dir.join("Cadernos/N");
        std::fs::create_dir_all(&nbdir).unwrap();
        std::fs::write(nbdir.join("N.md"), Notebook::new_text("Cadernos/N", "A5", "liso", "nenhuma", "#000000", "2026-01-01", false)).unwrap();
        std::fs::write(nbdir.join("002.md"), Page::new_text("N", 2, "2026-01-02")).unwrap();
        std::fs::write(nbdir.join("001 Rosto.md"), Page::new_text("N", 1, "2026-01-01")).unwrap();
        std::fs::write(nbdir.join("outra.md"), "# outra").unwrap();
        let mut nb = Notebook::parse("Cadernos/N/N.md", &std::fs::read_to_string(nbdir.join("N.md")).unwrap()).unwrap();
        nb.scan(&dir);
        assert_eq!(nb.pages.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), ["Cadernos/N/001 Rosto.md", "Cadernos/N/002.md"]);
        nb.pages.swap(0, 1);
        assert_eq!(nb.renames(), vec![("Cadernos/N/002.md".to_string(), "Cadernos/N/001.md".to_string()), ("Cadernos/N/001 Rosto.md".to_string(), "Cadernos/N/002 Rosto.md".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
