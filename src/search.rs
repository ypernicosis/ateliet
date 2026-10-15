//! Full text search with a subset of Obsidian's query syntax:
//! words (AND), `"exact phrases"`, `-exclusions`, `OR`, `path:`, `file:`,
//! `tag:`, `line:(…)`, `section:` is treated as plain text.

use rayon::prelude::*;
use regex::Regex;
use serde::Serialize;
use std::path::Path;

#[derive(Debug)]
enum Term {
    Text(Regex),
    Path(String),
    File(String),
    Tag(Regex),
}

#[derive(Debug)]
struct Clause {
    neg: bool,
    any: Vec<Term>, // OR-ed alternatives
}

#[derive(Serialize)]
pub struct LineMatch {
    pub line: usize,
    pub text: String,
    /// UTF-16 [start, end) ranges inside `text`.
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Serialize)]
pub struct FileResult {
    pub path: String,
    pub name_match: bool,
    pub matches: Vec<LineMatch>,
    pub total: usize,
}

fn tokenize(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut paren = 0;
    for c in q.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c)
            }
            '(' if !quoted => {
                paren += 1;
                cur.push(c)
            }
            ')' if !quoted => {
                paren -= 1;
                cur.push(c)
            }
            c if c.is_whitespace() && !quoted && paren <= 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')).unwrap_or(s);
    s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(s).to_string()
}

fn text_regex(s: &str, case: bool) -> Option<Regex> {
    if let Some(r) = s.strip_prefix('/').and_then(|x| x.strip_suffix('/')) {
        return Regex::new(&format!("{}{}", if case { "" } else { "(?i)" }, r)).ok();
    }
    Regex::new(&format!("{}{}", if case { "" } else { "(?i)" }, regex::escape(s))).ok()
}

fn parse(q: &str, case: bool) -> Vec<Clause> {
    let mut clauses: Vec<Clause> = Vec::new();
    let mut pending_or = false;
    for tok in tokenize(q) {
        if tok == "OR" {
            pending_or = true;
            continue;
        }
        let (neg, body) = match tok.strip_prefix('-') {
            Some(b) if !b.is_empty() => (true, b.to_string()),
            _ => (false, tok.clone()),
        };
        let term = if let Some(v) = body.strip_prefix("path:") {
            Some(Term::Path(unquote(v).to_lowercase()))
        } else if let Some(v) = body.strip_prefix("file:") {
            Some(Term::File(unquote(v).to_lowercase()))
        } else if let Some(v) = body.strip_prefix("tag:") {
            let t = unquote(v);
            let t = t.trim_start_matches('#');
            Regex::new(&format!(r"(?i)(?:^|\s)#{}(?:[/\s]|$)", regex::escape(t))).ok().map(Term::Tag)
        } else if let Some(v) = body.strip_prefix("line:").or_else(|| body.strip_prefix("content:")).or_else(|| body.strip_prefix("section:")) {
            text_regex(&unquote(v), case).map(Term::Text)
        } else {
            text_regex(&unquote(&body), case).map(Term::Text)
        };
        let Some(term) = term else { continue };
        if pending_or {
            if let Some(last) = clauses.last_mut() {
                last.any.push(term);
                pending_or = false;
                continue;
            }
        }
        pending_or = false;
        clauses.push(Clause { neg, any: vec![term] });
    }
    clauses
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn search(root: &Path, query: &str, case_sensitive: bool, limit: usize) -> Vec<FileResult> {
    let clauses = parse(query, case_sensitive);
    if clauses.is_empty() {
        return Vec::new();
    }
    let files: Vec<_> = crate::vault::list(root).into_iter().filter(|e| !e.folder).collect();
    let mut results: Vec<FileResult> = files
        .par_iter()
        .filter_map(|e| {
            let lower_path = e.path.to_lowercase();
            let name = lower_path.rsplit('/').next().unwrap_or(&lower_path).to_string();
            let is_md = lower_path.ends_with(".md");
            let content = if is_md { std::fs::read_to_string(root.join(&e.path)).unwrap_or_default() } else { String::new() };
            let mut name_match = false;
            let mut text_res: Vec<&Regex> = Vec::new();
            for c in &clauses {
                let hit = c.any.iter().any(|t| match t {
                    Term::Path(p) => lower_path.contains(p),
                    Term::File(f) => name.contains(f),
                    Term::Tag(r) => r.is_match(&content),
                    Term::Text(r) => {
                        let n = r.is_match(&e.path);
                        if n && !c.neg {
                            name_match = true;
                        }
                        n || r.is_match(&content)
                    }
                });
                if hit == c.neg {
                    return None;
                }
                if !c.neg {
                    for t in &c.any {
                        if let Term::Text(r) | Term::Tag(r) = t {
                            text_res.push(r);
                        }
                    }
                }
            }
            let mut matches = Vec::new();
            let mut total = 0;
            for (ln, line) in content.lines().enumerate() {
                let mut ranges: Vec<(usize, usize)> = Vec::new();
                for r in &text_res {
                    for m in r.find_iter(line) {
                        if m.start() == m.end() {
                            continue;
                        }
                        ranges.push((utf16_len(&line[..m.start()]), utf16_len(&line[..m.end()])));
                    }
                }
                if !ranges.is_empty() {
                    total += ranges.len();
                    if matches.len() < 30 {
                        ranges.sort();
                        matches.push(LineMatch { line: ln, text: line.chars().take(400).collect(), ranges });
                    }
                }
            }
            Some(FileResult { path: e.path.clone(), name_match, matches, total })
        })
        .collect();
    results.sort_by(|a, b| b.name_match.cmp(&a.name_match).then(b.total.cmp(&a.total)).then(a.path.cmp(&b.path)));
    results.truncate(limit);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing() {
        let c = parse(r#"foo "bar baz" -qux path:notes tag:#x a OR b"#, false);
        assert_eq!(c.len(), 6);
        assert!(c[2].neg);
        assert_eq!(c[5].any.len(), 2);
    }
}
