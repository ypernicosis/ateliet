//! The page index: every markdown file of the vault as Dataview sees it
//! (frontmatter, inline fields, tags, links, list items and tasks), plus link
//! resolution and back-links. Built once in parallel and kept current from the
//! vault watcher.

use super::parse::Source;
use super::value::{parse_inline_value, Link, Obj, Value};
use crate::datefmt;
use crate::syntax;
use crate::text::LineIndex;
use crate::vault;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use regex::Regex;
use serde_json::Value as Json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct RawLink {
    pub target: String,
    pub display: Option<String>,
    pub embed: bool,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub line: usize,
    pub line_count: usize,
    pub text: String,
    pub symbol: String,
    pub status: Option<char>,
    /// Index (in `Page::lists`) of the parent item.
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub section: Option<String>,
    pub fields: Obj,
    pub tags: Vec<String>,
    pub links: Vec<RawLink>,
    pub block_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Page {
    pub path: String,
    pub ctime: i64,
    pub mtime: i64,
    pub size: u64,
    pub frontmatter: Obj,
    /// The frontmatter as parsed from YAML (`Null` when there is none).
    pub yaml: Json,
    /// Frontmatter and inline fields, under their own names and canonical names.
    pub fields: Obj,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub links: Vec<RawLink>,
    pub lists: Vec<Item>,
    pub day: Option<chrono::NaiveDateTime>,
    /// Resolved outgoing link targets (paths), in order of appearance.
    pub outlinks: Vec<Link>,
}

#[derive(Clone, Copy, Debug)]
struct FileInfo {
    ctime: i64,
    mtime: i64,
    size: u64,
}

pub struct Index {
    pub root: PathBuf,
    files: HashMap<String, FileInfo>,
    pages: HashMap<String, Page>,
    by_name: HashMap<String, Vec<String>>,
    inlinks: HashMap<String, Vec<String>>,
    /// Bumped on every change, so callers can cache results.
    pub generation: u64,
}

static EMOJI_DATE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(📅|✅|➕|🛫|⏳)️?\s*(\d{4}-\d{2}-\d{2})").unwrap());
static DAY_IN_NAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d{4})-(\d{2})-(\d{2})|(\d{4})(\d{2})(\d{2})").unwrap());
static LIST_MARKER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\s*(?:>\s*)*)([-*+]|\d+[.)])(\s+)(?:\[(.)\]\s?)?").unwrap());
static LINE_PREFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(?:>\s*)*(?:(?:[-*+]|\d+[.)])\s+(?:\[.\]\s+)?)?").unwrap());

/// Dataview's canonical field name: lower case, spaces to `-`, punctuation dropped.
pub fn canonical(name: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in name.trim().chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push('-');
        }
        space = false;
        if c.is_alphanumeric() || c == '_' || c == '-' || (c as u32) > 0x2000 && !c.is_ascii() && !c.is_whitespace() {
            out.extend(c.to_lowercase());
        }
    }
    out
}

fn clean_key(k: &str) -> Option<String> {
    let k = k.trim().trim_matches(|c| matches!(c, '*' | '_' | '~' | '=')).trim();
    if k.is_empty() || k.len() > 150 || k.contains(['[', ']', '(', ')', '`', '|', '#']) || k.contains("://") {
        return None;
    }
    Some(k.to_string())
}

fn merge_field(obj: &mut Obj, key: &str, v: Value) {
    match obj.0.iter_mut().find(|(x, _)| x == key) {
        Some((_, existing)) => {
            *existing = match std::mem::take(existing) {
                Value::List(mut l) => {
                    l.push(v);
                    Value::List(l)
                }
                other => Value::List(vec![other, v]),
            };
        }
        None => obj.0.push((key.to_string(), v)),
    }
}

/// Store a field under its own name and its canonical name; repeated keys collect into a list.
fn push_field(obj: &mut Obj, key: &str, v: Value) {
    let canon = canonical(key);
    merge_field(obj, key, v.clone());
    if !canon.is_empty() && canon != key {
        merge_field(obj, &canon, v);
    }
}

/// Inline fields of one line: `key:: value` filling the line, and bracketed
/// `[key:: value]` / `(key:: value)` anywhere in it.
pub fn line_fields(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    let mut bracketed_spans: Vec<(usize, usize)> = Vec::new();
    while i < b.len() {
        let open = b[i];
        if (open == b'[' || open == b'(') && !(open == b'[' && (b.get(i + 1) == Some(&b'[') || (i > 0 && b[i - 1] == b'['))) {
            let close = if open == b'[' { b']' } else { b')' };
            let mut depth = 0i32;
            let mut j = i;
            let mut end = None;
            while j < b.len() {
                if b[j] == open {
                    depth += 1;
                } else if b[j] == close {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                }
                j += 1;
            }
            if let Some(end) = end {
                let inner = &line[i + 1..end];
                if let Some(sep) = inner.find("::") {
                    if let Some(key) = clean_key(&inner[..sep]) {
                        out.push((key, inner[sep + 2..].trim().to_string()));
                        bracketed_spans.push((i, end + 1));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    if bracketed_spans.is_empty() {
        let body_start = LINE_PREFIX.find(line).map(|m| m.end()).unwrap_or(0);
        let body = &line[body_start..];
        if let Some(sep) = body.find("::") {
            if let Some(key) = clean_key(&body[..sep]) {
                if !body[..sep].starts_with(char::is_whitespace) {
                    out.push((key, body[sep + 2..].trim().to_string()));
                }
            }
        }
    }
    out
}

fn tag_list(v: &Json) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |s: &str| {
        for t in s.split([',', ' ']).map(str::trim).filter(|t| !t.is_empty()) {
            out.push(if t.starts_with('#') { t.to_string() } else { format!("#{t}") });
        }
    };
    match v {
        Json::String(s) => push(s),
        Json::Array(a) => {
            for x in a {
                if let Some(s) = x.as_str() {
                    push(s)
                }
            }
        }
        _ => {}
    }
    out
}

fn string_list(v: &Json) -> Vec<String> {
    match v {
        Json::String(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        Json::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

fn add_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

fn raw_links_in(text: &str) -> Vec<RawLink> {
    let mut out = Vec::new();
    for c in syntax::WIKILINK.captures_iter(text) {
        let (target, alias) = syntax::split_wikilink(&c[2]);
        if !target.is_empty() {
            out.push(RawLink { target: target.to_string(), display: alias.map(str::to_string), embed: &c[1] == "!" });
        }
    }
    out
}

/// Parse one note into a page (without resolved links).
pub fn parse_page(path: &str, text: &str, ctime: i64, mtime: i64, size: u64) -> Page {
    let meta = crate::metadata::parse(text);
    let prep = syntax::prepare(text);
    let idx = LineIndex::new(text);
    let masked_idx = LineIndex::new(&prep.masked);

    let mut fields = Obj::default();
    let mut frontmatter = Obj::default();
    let mut tags = Vec::new();
    let mut aliases = Vec::new();
    let yaml = meta.get("frontmatter").cloned().unwrap_or(Json::Null);
    if let Some(Json::Object(fm)) = meta.get("frontmatter") {
        for (k, v) in fm {
            let val = Value::from_frontmatter(v);
            frontmatter.insert(k.clone(), val.clone());
            push_field(&mut fields, k, val);
            match k.to_lowercase().as_str() {
                "tags" | "tag" => tag_list(v).into_iter().for_each(|t| add_unique(&mut tags, t)),
                "aliases" | "alias" => aliases.extend(string_list(v)),
                _ => {}
            }
        }
    }

    // links from the body (resolved later) and frontmatter
    let mut links: Vec<RawLink> = Vec::new();
    for key in ["links", "embeds"] {
        if let Some(Json::Array(a)) = meta.get(key) {
            for l in a {
                let target = l["link"].as_str().unwrap_or_default();
                if target.is_empty() {
                    continue;
                }
                links.push(RawLink {
                    target: target.to_string(),
                    display: l["displayText"].as_str().filter(|d| *d != target).map(str::to_string),
                    embed: key == "embeds",
                });
            }
        }
    }
    if let Some(Json::Array(a)) = meta.get("frontmatterLinks") {
        for l in a {
            if let Some(t) = l["link"].as_str() {
                links.push(RawLink { target: t.to_string(), display: None, embed: false });
            }
        }
    }
    if let Some(Json::Array(a)) = meta.get("tags") {
        for t in a {
            if let Some(t) = t["tag"].as_str() {
                add_unique(&mut tags, t.to_string());
            }
        }
    }

    // headings by line, for task sections
    let mut headings: Vec<(usize, String)> = Vec::new();
    if let Some(Json::Array(a)) = meta.get("headings") {
        for h in a {
            headings.push((h["position"]["start"]["line"].as_u64().unwrap_or(0) as usize, h["heading"].as_str().unwrap_or_default().to_string()));
        }
    }
    let section_for = |line: usize| headings.iter().rev().find(|(l, _)| *l <= line).map(|(_, h)| h.clone());

    // list items
    let mut lists: Vec<Item> = Vec::new();
    let mut by_line: HashMap<usize, usize> = HashMap::new();
    if let Some(Json::Array(a)) = meta.get("listItems") {
        for li in a {
            let start = li["position"]["start"]["line"].as_u64().unwrap_or(0) as usize;
            let end = li["position"]["end"]["line"].as_u64().unwrap_or(start as u64) as usize;
            let parent_line = li["parent"].as_i64().unwrap_or(-1);
            let first = idx.line_text(start);
            let (symbol, body) = match LIST_MARKER.captures(first) {
                Some(c) => (c[2].to_string(), first[c.get(0).unwrap().end()..].to_string()),
                None => ("-".to_string(), first.trim().to_string()),
            };
            let status = li["task"].as_str().and_then(|s| s.chars().next());
            let parent = if parent_line >= 0 { by_line.get(&(parent_line as usize)).copied() } else { None };
            let item = Item {
                line: start,
                line_count: end - start + 1,
                text: body,
                symbol,
                status,
                parent,
                children: Vec::new(),
                section: section_for(start),
                fields: Obj::default(),
                tags: Vec::new(),
                links: Vec::new(),
                block_id: li["id"].as_str().map(str::to_string),
            };
            by_line.insert(start, lists.len());
            if let Some(p) = parent {
                let n = lists.len();
                lists[p].children.push(n);
            }
            lists.push(item);
        }
    }
    // continuation lines (not belonging to a child) join the item text
    for i in 0..lists.len() {
        let (start, count) = (lists[i].line, lists[i].line_count);
        let child_starts: Vec<usize> = lists[i].children.iter().map(|&c| lists[c].line).collect();
        let stop = child_starts.iter().min().copied().unwrap_or(start + count);
        for l in start + 1..stop.min(start + count) {
            let t = idx.line_text(l).trim();
            if !t.is_empty() {
                lists[i].text.push('\n');
                lists[i].text.push_str(t);
            }
        }
    }

    // inline fields, line by line on the masked text (no frontmatter, comments or code)
    for line in 0..masked_idx.line_count() {
        let start = masked_idx.line_start(line);
        if syntax::in_ranges(&prep.code, start) {
            continue;
        }
        let lt = masked_idx.line_text(line);
        if !lt.contains("::") {
            continue;
        }
        let found = line_fields(lt);
        if found.is_empty() {
            continue;
        }
        let owner = lists.iter().enumerate().filter(|(_, it)| it.line <= line && line < it.line + it.line_count).max_by_key(|(_, it)| it.line).map(|(i, _)| i);
        for (k, v) in found {
            let val = parse_inline_value(&v);
            push_field(&mut fields, &k, val.clone());
            if let Some(o) = owner {
                push_field(&mut lists[o].fields, &k, val);
            }
        }
    }
    for it in &mut lists {
        for (_, t) in syntax::find_tags(&it.text) {
            add_unique(&mut it.tags, t);
        }
        it.links = raw_links_in(&it.text);
        for c in EMOJI_DATE.captures_iter(&it.text.clone()) {
            let key = match &c[1] {
                "📅" => "due",
                "✅" => "completion",
                "➕" => "created",
                "🛫" => "start",
                _ => "scheduled",
            };
            if let Some(d) = datefmt::parse_iso(&c[2]) {
                if it.fields.get(key).is_none() {
                    it.fields.insert(key, Value::Date(d));
                }
            }
        }
    }

    let name = path.rsplit('/').next().unwrap_or(path);
    let day = DAY_IN_NAME
        .captures(name)
        .and_then(|c| {
            let g = |a: usize, b: usize| c.get(a).or(c.get(b)).map(|m| m.as_str().to_string()).unwrap_or_default();
            datefmt::parse_iso(&format!("{}-{}-{}", g(1, 4), g(2, 5), g(3, 6)))
        })
        .or_else(|| match fields.get("date") {
            Some(Value::Date(d)) => Some(*d),
            _ => None,
        });

    Page { path: path.to_string(), ctime, mtime, size, frontmatter, yaml, fields, tags, aliases, links, lists, day, outlinks: Vec::new() }
}

fn lower_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_lowercase()
}

fn parent_dir(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

fn resolve_relative(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for seg in rel.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

impl Index {
    pub fn empty(root: &Path) -> Index {
        Index { root: root.to_path_buf(), files: HashMap::new(), pages: HashMap::new(), by_name: HashMap::new(), inlinks: HashMap::new(), generation: 0 }
    }

    /// Scan and parse the whole vault.
    pub fn build(root: &Path) -> Index {
        let mut ix = Index::empty(root);
        let entries: Vec<vault::Entry> = vault::list(root).into_iter().filter(|e| !e.folder).collect();
        for e in &entries {
            ix.files.insert(e.path.clone(), FileInfo { ctime: e.ctime as i64, mtime: e.mtime as i64, size: e.size });
        }
        let pages: Vec<Page> = entries
            .par_iter()
            .filter(|e| e.path.to_lowercase().ends_with(".md"))
            .filter_map(|e| {
                let text = std::fs::read_to_string(root.join(&e.path)).ok()?;
                Some(parse_page(&e.path, &text, e.ctime as i64, e.mtime as i64, e.size))
            })
            .collect();
        for p in pages {
            ix.pages.insert(p.path.clone(), p);
        }
        ix.rebuild_names();
        ix.relink();
        ix
    }

    /// Re-read the given vault-relative paths (files or folders, existing or not).
    pub fn update(&mut self, rels: &[String]) {
        let mut changed = false;
        for rel in rels {
            let abs = self.root.join(rel);
            let st = vault::stat(&abs);
            match st {
                Some(s) if s.kind == "folder" => {
                    let prefix = format!("{rel}/");
                    let mut seen = HashSet::new();
                    for e in walkdir::WalkDir::new(&abs).follow_links(true).into_iter().flatten() {
                        if e.file_type().is_dir() {
                            continue;
                        }
                        let r = vault::rel_path(&self.root, e.path());
                        if r.split('/').any(|seg| seg.starts_with('.')) {
                            continue;
                        }
                        seen.insert(r.clone());
                        changed |= self.update_file(&r);
                    }
                    let gone: Vec<String> = self.files.keys().filter(|p| p.starts_with(&prefix) && !seen.contains(*p)).cloned().collect();
                    for g in gone {
                        changed |= self.remove_file(&g);
                    }
                }
                Some(_) => changed |= self.update_file(rel),
                None => {
                    changed |= self.remove_file(rel);
                    let prefix = format!("{rel}/");
                    let gone: Vec<String> = self.files.keys().filter(|p| p.starts_with(&prefix)).cloned().collect();
                    for g in gone {
                        changed |= self.remove_file(&g);
                    }
                }
            }
        }
        if changed {
            self.rebuild_names();
            self.relink();
        }
    }

    fn update_file(&mut self, rel: &str) -> bool {
        if rel.split('/').any(|seg| seg.starts_with('.')) {
            return false;
        }
        let Some(st) = vault::stat(&self.root.join(rel)) else { return self.remove_file(rel) };
        let info = FileInfo { ctime: st.ctime as i64, mtime: st.mtime as i64, size: st.size };
        if let Some(old) = self.files.get(rel) {
            if old.mtime == info.mtime && old.size == info.size && (self.pages.contains_key(rel) || !rel.to_lowercase().ends_with(".md")) {
                return false;
            }
        }
        self.files.insert(rel.to_string(), info);
        if rel.to_lowercase().ends_with(".md") {
            if let Ok(text) = std::fs::read_to_string(self.root.join(rel)) {
                self.pages.insert(rel.to_string(), parse_page(rel, &text, info.ctime, info.mtime, info.size));
            }
        }
        true
    }

    fn remove_file(&mut self, rel: &str) -> bool {
        let a = self.files.remove(rel).is_some();
        let b = self.pages.remove(rel).is_some();
        a || b
    }

    fn rebuild_names(&mut self) {
        self.by_name.clear();
        for p in self.files.keys() {
            let n = lower_name(p);
            if let Some(stem) = n.strip_suffix(".md") {
                self.by_name.entry(stem.to_string()).or_default().push(p.clone());
            }
            self.by_name.entry(n).or_default().push(p.clone());
        }
    }

    /// Resolve every page's links and recompute back-links.
    fn relink(&mut self) {
        let resolved: Vec<(String, Vec<Link>)> = self
            .pages
            .values()
            .map(|p| {
                let mut seen = HashSet::new();
                let mut out = Vec::new();
                for l in &p.links {
                    let link = self.link_from(&l.target, l.display.clone(), l.embed, &p.path);
                    if seen.insert(link.path.clone()) {
                        out.push(Link { subpath: None, display: None, ..link });
                    }
                }
                (p.path.clone(), out)
            })
            .collect();
        self.inlinks.clear();
        for (src, links) in resolved {
            for l in &links {
                if self.files.contains_key(&l.path) {
                    self.inlinks.entry(l.path.clone()).or_default().push(src.clone());
                }
            }
            if let Some(p) = self.pages.get_mut(&src) {
                p.outlinks = links;
            }
        }
        for v in self.inlinks.values_mut() {
            v.sort();
            v.dedup();
        }
        self.generation += 1;
    }

    /// Obsidian's resolution order: exact path, relative to the source, then
    /// by name (same folder first, then the shortest path).
    pub fn resolve(&self, linkpath: &str, source: &str) -> Option<String> {
        let lp = syntax::percent_decode(linkpath.trim().trim_start_matches("./"));
        let lp = lp.trim_start_matches('/');
        if lp.is_empty() {
            return self.files.contains_key(source).then(|| source.to_string());
        }
        let has = |p: &str| -> Option<String> {
            if self.files.contains_key(p) {
                return Some(p.to_string());
            }
            let lower = p.to_lowercase();
            self.by_name.get(&lower_name(p)).and_then(|c| c.iter().find(|x| x.to_lowercase() == lower).cloned())
        };
        let dir = parent_dir(source);
        for base in [lp.to_string(), resolve_relative(dir, lp)] {
            if let Some(p) = has(&base).or_else(|| has(&format!("{base}.md"))) {
                return Some(p);
            }
        }
        let lower = lp.to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower);
        let has_ext = name.rsplit_once('.').is_some_and(|(_, e)| !e.is_empty() && !e.contains(' '));
        let mut cands: Vec<&String> = self.by_name.get(name).map(|v| v.iter().collect()).unwrap_or_default();
        if !has_ext {
            if let Some(more) = self.by_name.get(&format!("{name}.md")) {
                cands.extend(more.iter());
            }
        }
        cands.sort();
        cands.dedup();
        let matching: Vec<&String> = cands
            .into_iter()
            .filter(|p| {
                if !lower.contains('/') {
                    return true;
                }
                let pl = p.to_lowercase();
                let no_ext = pl.strip_suffix(".md").unwrap_or(&pl);
                no_ext.ends_with(&lower) || pl.ends_with(&lower)
            })
            .collect();
        if matching.len() <= 1 {
            return matching.first().map(|s| s.to_string());
        }
        if let Some(same) = matching.iter().find(|p| parent_dir(p) == dir) {
            return Some(same.to_string());
        }
        matching.iter().min_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b))).map(|s| s.to_string())
    }

    /// A link value for `target` (with `#subpath`) written in `source`.
    pub fn link_from(&self, target: &str, display: Option<String>, embed: bool, source: &str) -> Link {
        let (path, sub) = match target.split_once('#') {
            Some((p, s)) => (p, Some(s.to_string())),
            None => (target, None),
        };
        let resolved = if path.is_empty() { Some(source.to_string()) } else { self.resolve(path, source) };
        let path = resolved.unwrap_or_else(|| {
            let p = syntax::percent_decode(path);
            if p.to_lowercase().ends_with(".md") || p.contains('.') {
                p
            } else {
                format!("{p}.md")
            }
        });
        Link { path, display, subpath: sub, embed }
    }

    /// Notes linking to `path`.
    pub fn inlinks(&self, path: &str) -> Vec<String> {
        self.inlinks.get(path).cloned().unwrap_or_default()
    }

    /// Every tag with the number of notes carrying it.
    pub fn tag_counts(&self) -> Vec<(String, usize)> {
        let mut m: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for p in self.pages.values() {
            for t in &p.tags {
                *m.entry(t.clone()).or_default() += 1;
            }
        }
        m.into_iter().collect()
    }

    pub fn page(&self, path: &str) -> Option<&Page> {
        self.pages.get(path)
    }

    pub fn page_paths(&self) -> impl Iterator<Item = &String> {
        self.pages.keys()
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    // ── sources ────────────────────────────────────────────────────────────

    pub fn source_paths(&self, src: &Source, origin: &str) -> Result<Vec<String>, String> {
        let set = self.source_set(src, origin)?;
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        Ok(v)
    }

    fn source_set(&self, src: &Source, origin: &str) -> Result<HashSet<String>, String> {
        Ok(match src {
            Source::All => self.pages.keys().cloned().collect(),
            Source::Tag(t) => {
                let t = t.to_lowercase();
                let prefix = format!("{t}/");
                self.pages
                    .values()
                    .filter(|p| p.tags.iter().any(|x| {
                        let x = x.to_lowercase();
                        x == t || x.starts_with(&prefix)
                    }))
                    .map(|p| p.path.clone())
                    .collect()
            }
            Source::Folder(f) => {
                let f = f.trim_matches('/');
                if f.is_empty() {
                    return Ok(self.pages.keys().cloned().collect());
                }
                // a file path (`"Folder/Note"`) selects that note
                let as_file = if f.to_lowercase().ends_with(".md") { f.to_string() } else { format!("{f}.md") };
                if self.pages.contains_key(&as_file) {
                    return Ok(HashSet::from([as_file]));
                }
                let prefix = format!("{f}/");
                self.pages.keys().filter(|p| p.starts_with(&prefix)).cloned().collect()
            }
            Source::LinkedFrom(target) => {
                let Some(dest) = self.resolve(target.split('#').next().unwrap_or(target), origin) else { return Ok(HashSet::new()) };
                self.inlinks.get(&dest).map(|v| v.iter().cloned().collect()).unwrap_or_default()
            }
            Source::Outgoing(target) => {
                let Some(src) = self.resolve(target.split('#').next().unwrap_or(target), origin) else { return Ok(HashSet::new()) };
                self.pages.get(&src).map(|p| p.outlinks.iter().filter(|l| self.pages.contains_key(&l.path)).map(|l| l.path.clone()).collect()).unwrap_or_default()
            }
            Source::Not(s) => {
                let ex = self.source_set(s, origin)?;
                self.pages.keys().filter(|p| !ex.contains(*p)).cloned().collect()
            }
            Source::And(a, b) => {
                let a = self.source_set(a, origin)?;
                let b = self.source_set(b, origin)?;
                a.intersection(&b).cloned().collect()
            }
            Source::Or(a, b) => {
                let mut a = self.source_set(a, origin)?;
                a.extend(self.source_set(b, origin)?);
                a
            }
        })
    }

    // ── rows ───────────────────────────────────────────────────────────────

    /// The `file` object of a page.
    pub fn file_object(&self, p: &Page) -> Obj {
        let name = p.path.rsplit('/').next().unwrap_or(&p.path);
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s, e),
            None => (name, ""),
        };
        let mut all_tags: Vec<String> = Vec::new();
        for t in &p.tags {
            let mut acc = String::new();
            for (i, seg) in t.split('/').enumerate() {
                if i > 0 {
                    acc.push('/');
                }
                acc.push_str(seg);
                add_unique(&mut all_tags, acc.clone());
            }
        }
        let ctime = datefmt::from_millis(p.ctime);
        let mtime = datefmt::from_millis(p.mtime);
        let midnight = |d: chrono::NaiveDateTime| d.date().and_time(chrono::NaiveTime::MIN);
        let inlinks = self.inlinks.get(&p.path).map(|v| v.iter().map(|s| Value::Link(Link::file(s))).collect()).unwrap_or_default();
        let items: Vec<Value> = (0..p.lists.len()).map(|i| self.item_object(p, i)).collect();
        let tasks: Vec<Value> = p.lists.iter().zip(&items).filter(|(it, _)| it.status.is_some()).map(|(_, v)| v.clone()).collect();
        let mut o = Obj::default();
        o.insert("name", Value::str(stem));
        o.insert("path", Value::str(&p.path));
        o.insert("folder", Value::str(parent_dir(&p.path)));
        o.insert("ext", Value::str(ext));
        o.insert("link", Value::Link(Link::file(&p.path)));
        o.insert("size", Value::Number(p.size as f64));
        o.insert("ctime", Value::Date(ctime));
        o.insert("cday", Value::Date(midnight(ctime)));
        o.insert("mtime", Value::Date(mtime));
        o.insert("mday", Value::Date(midnight(mtime)));
        o.insert("tags", Value::List(all_tags.into_iter().map(Value::String).collect()));
        o.insert("etags", Value::List(p.tags.iter().cloned().map(Value::String).collect()));
        o.insert("inlinks", Value::List(inlinks));
        o.insert("outlinks", Value::List(p.outlinks.iter().cloned().map(Value::Link).collect()));
        o.insert("aliases", Value::List(p.aliases.iter().cloned().map(Value::String).collect()));
        o.insert("tasks", Value::List(tasks));
        o.insert("lists", Value::List(items));
        o.insert("frontmatter", Value::Object(p.frontmatter.clone()));
        o.insert("day", p.day.map(Value::Date).unwrap_or(Value::Null));
        o.insert("starred", Value::Bool(false));
        o
    }

    pub fn item_object(&self, p: &Page, i: usize) -> Value {
        let it = &p.lists[i];
        let children: Vec<Value> = it.children.iter().map(|&c| self.item_object(p, c)).collect();
        let completed = matches!(it.status, Some('x') | Some('X'));
        let fully = completed
            && it.children.iter().all(|&c| {
                fn done(p: &Page, i: usize) -> bool {
                    let it = &p.lists[i];
                    (it.status.is_none() || matches!(it.status, Some('x') | Some('X'))) && it.children.iter().all(|&c| done(p, c))
                }
                done(p, c)
            });
        let section = it.section.as_ref().map(|h| Link { path: p.path.clone(), display: None, subpath: Some(h.clone()), embed: false });
        let link = match &it.block_id {
            Some(b) => Link { path: p.path.clone(), display: None, subpath: Some(format!("^{b}")), embed: false },
            None => section.clone().unwrap_or_else(|| Link::file(&p.path)),
        };
        let mut o = Obj::default();
        o.insert("symbol", Value::str(&it.symbol));
        o.insert("link", Value::Link(link));
        o.insert("section", section.clone().map(Value::Link).unwrap_or(Value::Null));
        o.insert("header", section.map(Value::Link).unwrap_or(Value::Null));
        o.insert("text", Value::str(&it.text));
        o.insert("visual", Value::str(&it.text));
        o.insert("tags", Value::List(it.tags.iter().cloned().map(Value::String).collect()));
        o.insert("line", Value::Number(it.line as f64));
        o.insert("lineCount", Value::Number(it.line_count as f64));
        o.insert("path", Value::str(&p.path));
        o.insert("list", Value::Number(it.line as f64));
        o.insert("blockId", it.block_id.clone().map(Value::String).unwrap_or(Value::Null));
        o.insert("parent", it.parent.map(|pi| Value::Number(p.lists[pi].line as f64)).unwrap_or(Value::Null));
        o.insert("children", Value::List(children));
        o.insert(
            "outlinks",
            Value::List(it.links.iter().map(|l| Value::Link(self.link_from(&l.target, l.display.clone(), l.embed, &p.path))).collect()),
        );
        o.insert("task", Value::Bool(it.status.is_some()));
        o.insert("annotated", Value::Bool(!it.fields.0.is_empty()));
        o.insert("real", Value::Bool(true));
        if let Some(s) = it.status {
            o.insert("status", Value::String(s.to_string()));
            o.insert("checked", Value::Bool(s != ' '));
            o.insert("completed", Value::Bool(completed));
            o.insert("fullyCompleted", Value::Bool(fully));
        }
        for (k, v) in &it.fields.0 {
            o.insert(k.clone(), v.clone());
        }
        Value::Object(o)
    }

    /// The row a query sees for a page: its fields plus `file`.
    pub fn page_row(&self, path: &str) -> Option<Value> {
        let p = self.pages.get(path)?;
        let mut o = p.fields.clone();
        o.insert("file", Value::Object(self.file_object(p)));
        Some(Value::Object(o))
    }

    /// A link value pointing at an existing page dereferences to its row.
    pub fn deref(&self, l: &Link) -> Option<Value> {
        self.page_row(&l.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_in_lines() {
        assert_eq!(line_fields("Status:: em andamento"), vec![("Status".to_string(), "em andamento".to_string())]);
        assert_eq!(line_fields("- **Valor**:: 120"), vec![("Valor".to_string(), "120".to_string())]);
        assert_eq!(line_fields("pago [valor:: 10] e (cat:: luz)"), vec![("valor".into(), "10".into()), ("cat".into(), "luz".into())]);
        assert_eq!(line_fields("[due:: [[2026-01-01]]]"), vec![("due".into(), "[[2026-01-01]]".into())]);
        assert!(line_fields("see https://x.y::z").is_empty());
        assert!(line_fields("a [[link]] b").is_empty());
    }

    #[test]
    fn canonical_names() {
        assert_eq!(canonical("Due Date"), "due-date");
        assert_eq!(canonical("Preço (R$)"), "preço-r");
        assert_eq!(canonical("Ação"), "ação");
    }

    #[test]
    fn page_parsing() {
        let t = "---\ntags: [fin, mes/2026]\nano: 2026\ninicio: 2026-01-05\n---\n# Receitas\n\n- [ ] pagar luz [valor:: 120] 📅 2026-10-30\n  - [x] sub\n- salário:: 5000\n\nTotal:: 10\nVer [[Outra]] #extra\n";
        let p = parse_page("Fin/2026-10-01.md", t, 0, 0, 0);
        assert_eq!(p.tags, vec!["#fin", "#mes/2026", "#extra"]);
        assert!(matches!(p.fields.get("ano"), Some(Value::Number(n)) if *n == 2026.0));
        assert!(matches!(p.fields.get("inicio"), Some(Value::Date(_))));
        assert!(matches!(p.fields.get("total"), Some(Value::Number(n)) if *n == 10.0));
        assert!(matches!(p.fields.get("Total"), Some(Value::Number(_))));
        assert!(matches!(p.fields.get("salário"), Some(Value::Number(n)) if *n == 5000.0));
        assert_eq!(p.lists.len(), 3);
        assert_eq!(p.lists[0].status, Some(' '));
        assert!(matches!(p.lists[0].fields.get("due"), Some(Value::Date(_))));
        assert!(matches!(p.lists[0].fields.get("valor"), Some(Value::Number(n)) if *n == 120.0));
        assert_eq!(p.lists[0].children, vec![1]);
        assert_eq!(p.lists[1].parent, Some(0));
        assert_eq!(p.lists[0].section.as_deref(), Some("Receitas"));
        assert!(p.day.is_some());
    }
}
