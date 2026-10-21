//! Markdown → `CachedMetadata`, mirroring the shape documented in
//! `obsidian.d.ts` so `app.metadataCache.getFileCache(file)` behaves the same.

use crate::syntax::{self, in_ranges};
use crate::text::LineIndex;
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
use serde_json::{json, Map, Value};
use std::ops::Range;

struct Section {
    kind: &'static str,
    start: usize,
    end: usize,
    id: Option<String>,
}

struct Item {
    start: usize,
    line: usize,
    content_end: Option<usize>,
    parent: i64,
}

pub fn parse(text: &str) -> Value {
    let prep = syntax::prepare(text);
    let src = prep.masked.as_str();
    let idx = LineIndex::new(text);
    let mut out = Map::new();

    let mut sections: Vec<Section> = Vec::new();
    let mut headings = Vec::new();
    let mut list_items = Vec::new();
    let mut links: Vec<(usize, Value)> = Vec::new();
    let mut embeds: Vec<(usize, Value)> = Vec::new();
    let mut blocks = Map::new();
    let mut footnotes = Vec::new();
    let mut link_ranges: Vec<Range<usize>> = Vec::new();

    // ── frontmatter ───────────────────────────────────────────────────────
    if let Some(fm) = &prep.frontmatter {
        let pos = idx.pos(fm.block.start, fm.block.end);
        sections.push(Section { kind: "yaml", start: fm.block.start, end: fm.block.end, id: None });
        if let Some(data) = syntax::parse_yaml(&text[fm.yaml.clone()]) {
            let mut fm_links = Vec::new();
            collect_frontmatter_links(&data, "", &mut fm_links);
            if !fm_links.is_empty() {
                out.insert("frontmatterLinks".into(), Value::Array(fm_links));
            }
            out.insert("frontmatter".into(), data);
        }
        out.insert("frontmatterPosition".into(), pos);
    }

    // ── block structure ───────────────────────────────────────────────────
    let mut depth = 0usize;
    let mut top: Option<(&'static str, usize)> = None;
    let mut lists: Vec<usize> = Vec::new(); // first line of each open list
    let mut items: Vec<Item> = Vec::new();
    let mut md_link: Option<(usize, bool, String)> = None; // (start, is_embed, dest)
    let mut md_link_text = String::new();

    for (ev, r) in Parser::new_ext(src, syntax::md_options()).into_offset_iter() {
        match &ev {
            Event::Start(tag) => {
                if depth == 0 {
                    let kind = match tag {
                        Tag::Heading { .. } => "heading",
                        Tag::List(_) => "list",
                        Tag::CodeBlock(_) => "code",
                        Tag::BlockQuote(_) => {
                            let first = strip_quote(idx.line_text(idx.line_of(r.start)));
                            if syntax::CALLOUT.is_match(first) {
                                "callout"
                            } else {
                                "blockquote"
                            }
                        }
                        Tag::Table(_) => "table",
                        Tag::HtmlBlock => "html",
                        Tag::FootnoteDefinition(_) => "footnoteDefinition",
                        Tag::Paragraph if src[r.clone()].trim_start().starts_with("$$") => "math",
                        _ => "paragraph",
                    };
                    top = Some((kind, r.start));
                }
                depth += 1;
                match tag {
                    Tag::Heading { level, .. } => {
                        let line = idx.line_of(r.start);
                        let end = idx.trim_end(r.start, r.end);
                        headings.push(json!({
                            "heading": syntax::heading_text(idx.line_text(line)),
                            "level": *level as u8,
                            "position": idx.pos(r.start, end),
                        }));
                    }
                    Tag::List(_) => {
                        if let Some(it) = items.last_mut() {
                            if it.content_end.is_none() {
                                it.content_end = Some(idx.trim_end(it.start, r.start));
                            }
                        }
                        lists.push(idx.line_of(r.start));
                    }
                    Tag::Item => {
                        let line = idx.line_of(r.start);
                        let parent = match items.last() {
                            Some(p) => p.line as i64,
                            None => -(*lists.last().unwrap_or(&line) as i64),
                        };
                        items.push(Item { start: r.start, line, content_end: None, parent });
                    }
                    Tag::Link { link_type, dest_url, .. } => {
                        if !matches!(link_type, LinkType::Autolink | LinkType::Email)
                            && !syntax::is_external_url(dest_url)
                            && !dest_url.starts_with('#')
                        {
                            md_link = Some((r.start, false, dest_url.to_string()));
                            md_link_text.clear();
                        }
                        link_ranges.push(r.clone());
                    }
                    Tag::Image { dest_url, .. } => {
                        if !syntax::is_external_url(dest_url) {
                            md_link = Some((r.start, true, dest_url.to_string()));
                            md_link_text.clear();
                        }
                        link_ranges.push(r.clone());
                    }
                    Tag::FootnoteDefinition(id) => {
                        footnotes.push(json!({ "id": id.to_string(), "position": idx.pos(r.start, idx.trim_end(r.start, r.end)) }));
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                depth = depth.saturating_sub(1);
                match tag {
                    TagEnd::List(_) => {
                        lists.pop();
                    }
                    TagEnd::Item => {
                        if let Some(it) = items.pop() {
                            let end = it.content_end.unwrap_or_else(|| idx.trim_end(it.start, r.end));
                            let mut li = Map::new();
                            li.insert("position".into(), idx.pos(it.start, end));
                            li.insert("parent".into(), json!(it.parent));
                            if let Some(c) = syntax::TASK.captures(idx.line_text(it.line)) {
                                li.insert("task".into(), json!(c[1].to_string()));
                            }
                            let last = idx.line_text(idx.line_of(end));
                            if let Some(c) = syntax::BLOCK_ID.captures(last) {
                                let id = c[1].to_string();
                                blocks.insert(id.clone(), json!({ "id": id, "position": idx.pos(it.start, end) }));
                                li.insert("id".into(), json!(id));
                            }
                            list_items.push((it.start, Value::Object(li)));
                        }
                    }
                    TagEnd::Link | TagEnd::Image => {
                        if let Some((start, is_embed, dest)) = md_link.take() {
                            let link = syntax::percent_decode(&dest);
                            let v = json!({
                                "link": link,
                                "original": &text[start..r.end],
                                "displayText": md_link_text.clone(),
                                "position": idx.pos(start, r.end),
                            });
                            if is_embed {
                                embeds.push((start, v))
                            } else {
                                links.push((start, v))
                            }
                        }
                    }
                    _ => {}
                }
                if depth == 0 {
                    if let Some((kind, start)) = top.take() {
                        let end = idx.trim_end(start, r.end);
                        sections.push(Section { kind, start, end, id: None });
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if md_link.is_some() {
                    md_link_text.push_str(t);
                }
            }
            Event::Rule if depth == 0 => {
                let end = idx.trim_end(r.start, r.end);
                sections.push(Section { kind: "thematicBreak", start: r.start, end, id: None });
            }
            Event::Html(_) | Event::DisplayMath(_) if depth == 0 => {
                let end = idx.trim_end(r.start, r.end);
                sections.push(Section { kind: "html", start: r.start, end, id: None });
            }
            _ => {}
        }
    }

    // ── block ids on sections ─────────────────────────────────────────────
    sections.sort_by_key(|s| s.start);
    let mut i = 0;
    while i < sections.len() {
        let last = idx.line_text(idx.line_of(sections[i].end));
        if let Some(c) = syntax::BLOCK_ID.captures(last) {
            let id = c[1].to_string();
            let s = &sections[i];
            let lone = s.kind == "paragraph" && last.trim() == format!("^{id}");
            // A lone `^id` paragraph names the table/quote/list right above it.
            let target = if lone && i > 0 && matches!(sections[i - 1].kind, "table" | "blockquote" | "callout" | "list" | "code" | "math") {
                i - 1
            } else {
                i
            };
            if sections[target].kind != "list" || target != i {
                let t = &sections[target];
                blocks.insert(id.clone(), json!({ "id": id, "position": idx.pos(t.start, sections[i].end) }));
                sections[target].id = Some(id);
            }
        }
        i += 1;
    }

    // ── wikilinks & embeds ────────────────────────────────────────────────
    for c in syntax::WIKILINK.captures_iter(src) {
        let m = c.get(0).unwrap();
        if in_ranges(&prep.code, m.start()) || in_ranges(&link_ranges, m.start()) {
            continue;
        }
        let inner = c.get(2).unwrap().as_str();
        let (target, alias) = syntax::split_wikilink(inner);
        if target.is_empty() {
            continue;
        }
        let v = json!({
            "link": target,
            "original": m.as_str(),
            "displayText": alias.map(|a| a.to_string()).unwrap_or_else(|| target.to_string()),
            "position": idx.pos(m.start(), m.end()),
        });
        link_ranges.push(m.range());
        if &c[1] == "!" {
            embeds.push((m.start(), v))
        } else {
            links.push((m.start(), v))
        }
    }

    // ── tags ──────────────────────────────────────────────────────────────
    let mut tags = Vec::new();
    for (r, tag) in syntax::find_tags(src) {
        if in_ranges(&prep.code, r.start) || in_ranges(&link_ranges, r.start) {
            continue;
        }
        tags.push(json!({ "tag": tag, "position": idx.pos(r.start, r.end) }));
    }

    // ── assemble ──────────────────────────────────────────────────────────
    links.sort_by_key(|x| x.0);
    embeds.sort_by_key(|x| x.0);
    list_items.sort_by_key(|x| x.0);
    let put = |out: &mut Map<String, Value>, k: &str, v: Vec<Value>| {
        if !v.is_empty() {
            out.insert(k.into(), Value::Array(v));
        }
    };
    put(&mut out, "links", links.into_iter().map(|x| x.1).collect());
    put(&mut out, "embeds", embeds.into_iter().map(|x| x.1).collect());
    put(&mut out, "tags", tags);
    put(&mut out, "headings", headings);
    put(&mut out, "listItems", list_items.into_iter().map(|x| x.1).collect());
    put(&mut out, "footnotes", footnotes);
    put(
        &mut out,
        "sections",
        sections
            .iter()
            .map(|s| {
                let mut v = json!({ "type": s.kind, "position": idx.pos(s.start, s.end) });
                if let Some(id) = &s.id {
                    v["id"] = json!(id);
                }
                v
            })
            .collect(),
    );
    if !blocks.is_empty() {
        out.insert("blocks".into(), Value::Object(blocks));
    }
    Value::Object(out)
}

fn strip_quote(line: &str) -> &str {
    let mut s = line.trim_start();
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    s
}

fn collect_frontmatter_links(v: &Value, key: &str, out: &mut Vec<Value>) {
    match v {
        Value::String(s) => {
            let s = s.trim();
            if let Some(c) = syntax::WIKILINK.captures(s) {
                if c.get(0).unwrap().as_str() == s && &c[1] != "!" {
                    let (target, alias) = syntax::split_wikilink(&c[2]);
                    out.push(json!({
                        "key": key,
                        "link": target,
                        "original": s,
                        "displayText": alias.unwrap_or(target),
                    }));
                }
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                collect_frontmatter_links(x, &format!("{key}.{i}"), out);
            }
        }
        Value::Object(o) => {
            for (k, x) in o {
                let k = if key.is_empty() { k.clone() } else { format!("{key}.{k}") };
                collect_frontmatter_links(x, &k, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_document() {
        let t = "---\ntags: [a, b]\nup: \"[[Home]]\"\n---\n# Título 😀\n\nSee [[Note#Sec|alias]] and [md](Other%20Note.md) #tag\n\n- [ ] task ^blk\n  - [x] child\n- plain\n\n> [!info] Callout\n> body\n\n```js\n// [[not a link]] #nottag\n```\n%% [[hidden]] %%\n![[img.png]]\n";
        let m = parse(t);
        assert_eq!(m["frontmatter"]["tags"], json!(["a", "b"]));
        assert_eq!(m["frontmatterLinks"][0]["link"], "Home");
        assert_eq!(m["headings"][0]["heading"], "Título 😀");
        let links: Vec<&str> = m["links"].as_array().unwrap().iter().map(|l| l["link"].as_str().unwrap()).collect();
        assert_eq!(links, vec!["Note#Sec", "Other Note.md"]);
        assert_eq!(m["links"][0]["displayText"], "alias");
        assert_eq!(m["embeds"][0]["link"], "img.png");
        assert_eq!(m["tags"].as_array().unwrap().len(), 1);
        assert_eq!(m["tags"][0]["tag"], "#tag");
        let items = m["listItems"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["task"], " ");
        assert_eq!(items[0]["id"], "blk");
        assert_eq!(items[1]["task"], "x");
        assert_eq!(items[1]["parent"], items[0]["position"]["start"]["line"]);
        assert!(items[0]["parent"].as_i64().unwrap() < 0);
        let kinds: Vec<&str> = m["sections"].as_array().unwrap().iter().map(|s| s["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["yaml", "heading", "paragraph", "list", "callout", "code", "paragraph"]);
        assert!(m["blocks"]["blk"].is_object());
    }

    #[test]
    fn utf16_offsets() {
        let t = "😀 [[A]]";
        let m = parse(t);
        // 😀 is 2 UTF-16 units, plus a space
        assert_eq!(m["links"][0]["position"]["start"]["offset"], 3);
        assert_eq!(m["links"][0]["position"]["start"]["col"], 3);
    }
}
