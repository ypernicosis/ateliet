//! Obsidian-flavoured syntax that CommonMark does not know about:
//! frontmatter, `%%comments%%`, `[[wikilinks]]`, `#tags`, `^block-ids`,
//! `> [!callouts]` and `==highlights==`.

use once_cell::sync::Lazy;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use std::ops::Range;

pub static WIKILINK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(!?)\[\[([^\[\]\n]+?)\]\]").unwrap());
/// `#tag` — letters, digits, `_`, `-`, `/` and any non-ASCII; must not be purely numeric.
pub static TAG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r##"(?:^|[\s(\[{])(#[^\s#\x21-\x2C\x2E\x3A-\x40\x5B-\x5E\x60\x7B-\x7E]+)"##).unwrap()
});
pub static BLOCK_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:^|\s)\^([A-Za-z0-9\-]+)\s*$").unwrap());
pub static CALLOUT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\[!([^\]\s]+)\]([+-]?)[ \t]*").unwrap());
pub static TASK: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(?:[-*+]|\d+[.)])\s+\[(.)\]").unwrap());
pub static HIGHLIGHT: Lazy<Regex> = Lazy::new(|| Regex::new(r"==([^=\n](?:[^\n]*?[^=\n])?)==").unwrap());

pub fn md_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
}

pub struct Frontmatter {
    /// Byte range of the YAML body (between the fences).
    pub yaml: Range<usize>,
    /// Byte range of the whole block, fences included (end excludes the newline).
    pub block: Range<usize>,
}

pub fn frontmatter(text: &str) -> Option<Frontmatter> {
    let first_nl = text.find('\n')?;
    if text[..first_nl].trim_end() != "---" {
        return None;
    }
    let body_start = first_nl + 1;
    let mut pos = body_start;
    loop {
        let rest = &text[pos..];
        let line_end = rest.find('\n').map(|i| pos + i).unwrap_or(text.len());
        let line = text[pos..line_end].trim_end_matches('\r');
        if line.trim_end() == "---" {
            return Some(Frontmatter { yaml: body_start..pos, block: 0..pos + line.len() });
        }
        if line_end >= text.len() {
            return None;
        }
        pos = line_end + 1;
    }
}

pub fn parse_yaml(src: &str) -> Option<serde_json::Value> {
    if src.trim().is_empty() {
        return Some(serde_json::Value::Object(Default::default()));
    }
    let v: serde_yaml::Value = serde_yaml::from_str(src).ok()?;
    let j = serde_json::to_value(v).ok()?;
    if j.is_object() {
        Some(j)
    } else {
        None
    }
}

/// Byte ranges of inline code, code blocks, math and raw HTML — places where
/// links and tags must not be detected.
pub fn code_ranges(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut block_start: Option<usize> = None;
    for (ev, r) in Parser::new_ext(text, md_options()).into_offset_iter() {
        match ev {
            Event::Start(Tag::CodeBlock(_)) => block_start = Some(r.start),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(s) = block_start.take() {
                    out.push(s..r.end);
                }
            }
            Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_) | Event::Html(_) | Event::InlineHtml(_) => {
                out.push(r)
            }
            _ => {}
        }
    }
    out
}

pub fn in_ranges(ranges: &[Range<usize>], at: usize) -> bool {
    ranges.iter().any(|r| r.start <= at && at < r.end)
}

/// `%%comment%%` ranges outside of code.
pub fn comment_ranges(text: &str, code: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut open: Option<usize> = None;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'%' && bytes[i + 1] == b'%' && !in_ranges(code, i) {
            match open.take() {
                None => open = Some(i),
                Some(s) => out.push(s..i + 2),
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    if let Some(s) = open {
        out.push(s..text.len());
    }
    out
}

/// Obsidian-normalised source: frontmatter and comments blanked out (offsets kept).
pub struct Prepared {
    pub masked: String,
    pub frontmatter: Option<Frontmatter>,
    pub code: Vec<Range<usize>>,
}

pub fn prepare(text: &str) -> Prepared {
    let fm = frontmatter(text);
    let mut buf = text.as_bytes().to_vec();
    if let Some(f) = &fm {
        crate::text::blank_range(&mut buf, f.block.start, f.block.end);
    }
    let mut masked = String::from_utf8(buf).unwrap_or_else(|_| text.to_string());
    let mut code = code_ranges(&masked);
    if masked.contains("%%") {
        let comments = comment_ranges(&masked, &code);
        if !comments.is_empty() {
            let mut buf = masked.into_bytes();
            for c in &comments {
                crate::text::blank_range(&mut buf, c.start, c.end);
            }
            masked = String::from_utf8(buf).unwrap_or_default();
            code = code_ranges(&masked);
        }
    }
    Prepared { masked, frontmatter: fm, code }
}

/// Split `target#sub|alias` into (target-with-subpath, alias).
pub fn split_wikilink(inner: &str) -> (&str, Option<&str>) {
    // A pipe escaped for tables (`\|`) still separates the alias.
    if let Some(i) = inner.find('|') {
        let mut t = &inner[..i];
        if t.ends_with('\\') {
            t = &t[..t.len() - 1];
        }
        (t.trim(), Some(inner[i + 1..].trim()))
    } else {
        (inner.trim(), None)
    }
}

/// What Obsidian shows for a link without alias: `Note > Heading`.
pub fn link_display(target: &str) -> String {
    if let Some((file, sub)) = target.split_once('#') {
        let parts: Vec<&str> = sub.split('#').filter(|s| !s.is_empty()).collect();
        if file.is_empty() {
            parts.join(" > ")
        } else {
            std::iter::once(file).chain(parts).collect::<Vec<_>>().join(" > ")
        }
    } else {
        target.to_string()
    }
}

pub fn is_external_url(dest: &str) -> bool {
    let d = dest.trim();
    if let Some(i) = d.find(':') {
        let scheme = &d[..i];
        return !scheme.is_empty()
            && scheme.len() > 1 // `C:` drive letters are paths
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    }
    false
}

pub fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

/// Find tags in `text`, returning (byte range, tag incl. `#`).
pub fn find_tags(text: &str) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    for c in TAG.captures_iter(text) {
        let m = c.get(1).unwrap();
        let tag = m.as_str();
        if tag[1..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        out.push((m.range(), tag.to_string()));
    }
    out
}

pub fn heading_text(line: &str) -> String {
    let t = line.trim_start().trim_start_matches('#').trim();
    // optional closing sequence: `## Title ##`
    let stripped = t.trim_end_matches('#');
    let t = if stripped.len() < t.len() && (stripped.is_empty() || stripped.ends_with(' ')) { stripped.trim_end() } else { t };
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_detects_fences() {
        let t = "---\ntitle: x\ntags: [a]\n---\n# Hi";
        let f = frontmatter(t).unwrap();
        assert_eq!(&t[f.yaml.clone()], "title: x\ntags: [a]\n");
        assert_eq!(&t[f.block.clone()], "---\ntitle: x\ntags: [a]\n---");
        assert!(frontmatter("# no").is_none());
    }

    #[test]
    fn tags() {
        let t = "a #tag and #nested/tag-1, #123 not, x#no, (#yes) #ação";
        let tags: Vec<String> = find_tags(t).into_iter().map(|x| x.1).collect();
        assert_eq!(tags, vec!["#tag", "#nested/tag-1", "#yes", "#ação"]);
    }

    #[test]
    fn wikilink_display() {
        assert_eq!(link_display("Note#Head"), "Note > Head");
        assert_eq!(link_display("#Head"), "Head");
        assert_eq!(split_wikilink("a#b|c"), ("a#b", Some("c")));
    }
}
