//! Markdown → HTML for the reading view.
//!
//! Output is split into top-level blocks (one `<div>` each in the DOM) with
//! their source line ranges, which is what `MarkdownPostProcessorContext
//! .getSectionInfo()` exposes to plugins. The produced markup follows
//! Obsidian's conventions (`internal-link`, `internal-embed`, `tag`,
//! `callout`, `task-list-item`, …) so themes and plugins find what they expect.

use crate::syntax::{self, in_ranges};
use crate::text::LineIndex;
use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, LinkType, Parser, Tag, TagEnd};
use serde::Serialize;
use std::ops::Range;

#[derive(Serialize, Debug)]
pub struct Block {
    pub html: String,
    #[serde(rename = "lineStart")]
    pub line_start: usize,
    #[serde(rename = "lineEnd")]
    pub line_end: usize,
    pub kind: &'static str,
}

pub fn render(text: &str) -> Vec<Block> {
    let prep = syntax::prepare(text);
    let src = prep.masked.as_str();
    let idx = LineIndex::new(text);
    let mut out = Vec::new();

    if let Some(fm) = &prep.frontmatter {
        out.push(Block { html: String::new(), line_start: 0, line_end: idx.line_of(fm.block.end), kind: "yaml" });
    }

    let mut depth = 0usize;
    let mut cur: Vec<(Event, Range<usize>)> = Vec::new();
    let mut start = 0usize;
    for (ev, r) in Parser::new_ext(src, syntax::md_options()).into_offset_iter() {
        let opens = matches!(ev, Event::Start(_));
        let closes = matches!(ev, Event::End(_));
        if depth == 0 {
            start = r.start;
        }
        if opens {
            depth += 1;
        }
        if closes {
            depth = depth.saturating_sub(1);
        }
        let end = r.end;
        cur.push((ev, r));
        if depth == 0 {
            let end = idx.trim_end(start, end);
            let line_start = idx.line_of(start);
            let line_end = idx.line_of(end);
            let kind = block_kind(&cur[0].0);
            let events = std::mem::take(&mut cur);
            let html = render_block(events, &idx, line_start, &prep.code);
            out.push(Block { html, line_start, line_end, kind });
        }
    }
    out
}

fn block_kind(ev: &Event) -> &'static str {
    match ev {
        Event::Start(Tag::Heading { .. }) => "heading",
        Event::Start(Tag::List(_)) => "list",
        Event::Start(Tag::CodeBlock(_)) => "code",
        Event::Start(Tag::BlockQuote(_)) => "blockquote",
        Event::Start(Tag::Table(_)) => "table",
        Event::Start(Tag::HtmlBlock) | Event::Html(_) => "html",
        Event::Start(Tag::FootnoteDefinition(_)) => "footnoteDefinition",
        Event::Rule => "thematicBreak",
        _ => "paragraph",
    }
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn raw<'a>(s: String) -> Event<'a> {
    Event::InlineHtml(CowStr::from(s))
}

fn render_block(events: Vec<(Event, Range<usize>)>, idx: &LineIndex, line_start: usize, code: &[Range<usize>]) -> String {
    // Merge adjacent text so `[[`, `#tag` and `==hl==` are seen whole.
    let mut merged: Vec<(Event, Range<usize>)> = Vec::with_capacity(events.len());
    for (ev, r) in events {
        if let (Event::Text(t), Some((Event::Text(prev), pr))) = (&ev, merged.last_mut()) {
            let mut s = prev.to_string();
            s.push_str(t);
            *prev = CowStr::from(s);
            pr.end = r.end;
            continue;
        }
        merged.push((ev, r));
    }

    let mut out: Vec<Event> = Vec::with_capacity(merged.len());
    let mut in_code = false;
    let mut link_depth = 0usize;
    let mut callouts: Vec<bool> = Vec::new();
    let mut skip_until_image_end: Option<String> = None; // collecting alt text
    let mut strip_task_prefix = false;
    let mut i = 0;
    while i < merged.len() {
        let (ev, r) = &merged[i];
        if let Some(alt) = skip_until_image_end.as_mut() {
            match ev {
                Event::End(TagEnd::Image) => {
                    let alt = std::mem::take(alt);
                    skip_until_image_end = None;
                    // The opening tag was emitted with a placeholder; patch alt in.
                    if let Some(Event::InlineHtml(h)) = out.last_mut() {
                        let s = h.replace("\u{0}ALT\u{0}", &esc(&alt));
                        *h = CowStr::from(s);
                    }
                }
                Event::Text(t) | Event::Code(t) => alt.push_str(t),
                _ => {}
            }
            i += 1;
            continue;
        }
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                let n = *level as u8;
                let t = syntax::heading_text(idx.line_text(idx.line_of(r.start)));
                out.push(raw(format!("<h{n} data-heading=\"{}\" dir=\"auto\">", esc(&t))));
            }
            Event::End(TagEnd::Heading(level)) => out.push(raw(format!("</h{}>", *level as u8))),
            Event::Start(Tag::Paragraph) => out.push(raw("<p dir=\"auto\">".into())),
            Event::End(TagEnd::Paragraph) => out.push(raw("</p>\n".into())),
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code = true;
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                if lang.is_empty() {
                    out.push(raw("<pre><code>".into()));
                } else {
                    let l = esc(&lang);
                    out.push(raw(format!("<pre class=\"language-{l}\"><code class=\"language-{l} is-loaded\">")));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                out.push(raw("</code></pre>\n".into()));
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let callout = match (merged.get(i + 1), merged.get(i + 2)) {
                    (Some((Event::Start(Tag::Paragraph), _)), Some((Event::Text(t), _))) => syntax::CALLOUT.captures(t).map(|c| {
                        (c[1].to_lowercase(), c[2].to_string(), c.get(0).unwrap().end())
                    }),
                    _ => None,
                };
                match callout {
                    None => {
                        callouts.push(false);
                        out.push(raw("<blockquote dir=\"auto\">\n".into()));
                    }
                    Some((kind, fold, prefix_len)) => {
                        callouts.push(true);
                        let mut cls = String::from("callout");
                        if !fold.is_empty() {
                            cls.push_str(" is-collapsible");
                        }
                        if fold == "-" {
                            cls.push_str(" is-collapsed");
                        }
                        out.push(raw(format!(
                            "<div data-callout-metadata=\"\" data-callout-fold=\"{}\" data-callout=\"{}\" class=\"{}\"><div class=\"callout-title\" dir=\"auto\"><div class=\"callout-icon\"></div><div class=\"callout-title-inner\">",
                            esc(&fold), esc(&kind), cls
                        )));
                        // Title: rest of the first line, or the capitalised type.
                        let first = match &merged[i + 2].0 {
                            Event::Text(t) => t[prefix_len..].to_string(),
                            _ => String::new(),
                        };
                        let mut j = i + 3;
                        let mut title_events: Vec<Event> = Vec::new();
                        if !first.trim().is_empty() {
                            title_events.extend(inline_text(&first, merged[i + 2].1.start, code));
                        }
                        while j < merged.len() {
                            match &merged[j].0 {
                                Event::SoftBreak | Event::HardBreak | Event::End(TagEnd::Paragraph) => break,
                                Event::Text(t) => title_events.extend(inline_text(t, merged[j].1.start, code)),
                                e => title_events.push(e.clone()),
                            }
                            j += 1;
                        }
                        if title_events.is_empty() {
                            let mut c = kind.chars();
                            let t = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
                            title_events.push(Event::Text(CowStr::from(t)));
                        }
                        out.extend(title_events);
                        let fold_icon = if fold.is_empty() { "" } else { "<div class=\"callout-fold\"></div>" };
                        let style = if fold == "-" { " style=\"display: none;\"" } else { "" };
                        out.push(raw(format!("</div>{fold_icon}</div><div class=\"callout-content\"{style}>\n")));
                        // Skip the consumed first line; reopen the paragraph if it continues.
                        match merged.get(j).map(|x| &x.0) {
                            Some(Event::End(TagEnd::Paragraph)) => i = j + 1,
                            Some(_) => {
                                out.push(raw("<p dir=\"auto\">".into()));
                                i = j + 1;
                            }
                            None => i = j,
                        }
                        continue;
                    }
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if callouts.pop().unwrap_or(false) {
                    out.push(raw("</div></div>\n".into()));
                } else {
                    out.push(raw("</blockquote>\n".into()));
                }
            }
            Event::Start(Tag::Item) => {
                let line = idx.line_of(r.start);
                let rel = line - line_start;
                match syntax::TASK.captures(idx.line_text(line)) {
                    Some(c) => {
                        let ch = c[1].to_string();
                        let checked = ch != " ";
                        strip_task_prefix = !matches!(ch.as_str(), " " | "x" | "X");
                        out.push(raw(format!(
                            "<li data-line=\"{rel}\" data-task=\"{}\" class=\"task-list-item{}\" dir=\"auto\"><input data-line=\"{rel}\" type=\"checkbox\" class=\"task-list-item-checkbox\"{}>",
                            esc(&ch),
                            if checked { " is-checked" } else { "" },
                            if checked { " checked" } else { "" }
                        )));
                    }
                    None => out.push(raw(format!("<li data-line=\"{rel}\" dir=\"auto\">"))),
                }
            }
            Event::End(TagEnd::Item) => out.push(raw("</li>\n".into())),
            Event::TaskListMarker(_) => {}
            Event::Start(Tag::Link { link_type, dest_url, title, .. }) => {
                link_depth += 1;
                let t = if title.is_empty() { String::new() } else { format!(" aria-label=\"{}\"", esc(title)) };
                if matches!(link_type, LinkType::Autolink | LinkType::Email) || syntax::is_external_url(dest_url) {
                    let href = if matches!(link_type, LinkType::Email) && !dest_url.starts_with("mailto:") {
                        format!("mailto:{dest_url}")
                    } else {
                        dest_url.to_string()
                    };
                    out.push(raw(format!("<a{t} href=\"{}\" class=\"external-link\" target=\"_blank\" rel=\"noopener nofollow\">", esc(&href))));
                } else {
                    let target = syntax::percent_decode(dest_url);
                    out.push(raw(format!(
                        "<a{t} data-href=\"{}\" href=\"{}\" class=\"internal-link\" target=\"_blank\" rel=\"noopener nofollow\">",
                        esc(&target),
                        esc(dest_url)
                    )));
                }
            }
            Event::End(TagEnd::Link) => {
                link_depth = link_depth.saturating_sub(1);
                out.push(raw("</a>".into()));
            }
            Event::Start(Tag::Image { dest_url, title, .. }) => {
                let t = esc(title);
                if syntax::is_external_url(dest_url) {
                    out.push(raw(format!("<img src=\"{}\" alt=\"\u{0}ALT\u{0}\" title=\"{t}\" referrerpolicy=\"no-referrer\">", esc(dest_url))));
                } else {
                    let target = syntax::percent_decode(dest_url);
                    out.push(raw(format!(
                        "<span alt=\"\u{0}ALT\u{0}\" src=\"{}\" class=\"internal-embed\" tabindex=\"-1\" contenteditable=\"false\"></span>",
                        esc(&target)
                    )));
                }
                skip_until_image_end = Some(String::new());
            }
            Event::InlineMath(m) => out.push(raw(format!("<span class=\"math math-inline\">{}</span>", esc(m)))),
            Event::DisplayMath(m) => out.push(raw(format!("<div class=\"math math-block\">{}</div>", esc(m)))),
            Event::Text(t) if !in_code && link_depth == 0 => {
                let mut s: &str = t;
                if strip_task_prefix {
                    strip_task_prefix = false;
                    if s.starts_with('[') && s.len() >= 3 {
                        let end = s.char_indices().nth(3).map(|x| x.0).unwrap_or(s.len());
                        s = s[end..].trim_start();
                    }
                }
                // Hide a trailing `^block-id` at the end of a block.
                let at_end = matches!(
                    merged.get(i + 1).map(|x| &x.0),
                    None | Some(Event::End(_)) | Some(Event::SoftBreak) | Some(Event::HardBreak) | Some(Event::Start(Tag::List(_)))
                );
                let owned;
                if at_end {
                    if let Some(m) = syntax::BLOCK_ID.find(s) {
                        owned = s[..m.start()].to_string();
                        s = &owned;
                    }
                }
                out.extend(inline_text(s, r.start, code));
            }
            e => out.push(e.clone()),
        }
        i += 1;
    }
    let mut s = String::new();
    html::push_html(&mut s, out.into_iter());
    s
}

/// Expand Obsidian inline syntax inside a plain text run.
fn inline_text<'a>(text: &str, _offset: usize, _code: &[Range<usize>]) -> Vec<Event<'a>> {
    let _ = in_ranges;
    let mut out = Vec::new();
    let mut last = 0;
    for c in syntax::WIKILINK.captures_iter(text) {
        let m = c.get(0).unwrap();
        if m.start() > last {
            highlights_and_tags(&text[last..m.start()], &mut out);
        }
        let (target, alias) = syntax::split_wikilink(&c[2]);
        if &c[1] == "!" {
            out.push(raw(format!(
                "<span alt=\"{}\" src=\"{}\" class=\"internal-embed\" tabindex=\"-1\" contenteditable=\"false\"></span>",
                esc(alias.unwrap_or(target)),
                esc(target)
            )));
        } else {
            let display = alias.map(|a| a.to_string()).unwrap_or_else(|| syntax::link_display(target));
            out.push(raw(format!(
                "<a data-href=\"{t}\" href=\"{t}\" class=\"internal-link\" target=\"_blank\" rel=\"noopener nofollow\">{}</a>",
                esc(&display),
                t = esc(target)
            )));
        }
        last = m.end();
    }
    if last < text.len() {
        highlights_and_tags(&text[last..], &mut out);
    }
    out
}

fn highlights_and_tags<'a>(text: &str, out: &mut Vec<Event<'a>>) {
    let mut last = 0;
    for c in syntax::HIGHLIGHT.captures_iter(text) {
        let m = c.get(0).unwrap();
        if m.start() > last {
            tags(&text[last..m.start()], out);
        }
        out.push(raw("<mark>".into()));
        tags(&c[1], out);
        out.push(raw("</mark>".into()));
        last = m.end();
    }
    if last < text.len() {
        tags(&text[last..], out);
    }
}

fn tags<'a>(text: &str, out: &mut Vec<Event<'a>>) {
    let mut last = 0;
    for (r, tag) in syntax::find_tags(text) {
        if r.start > last {
            out.push(Event::Text(CowStr::from(text[last..r.start].to_string())));
        }
        out.push(raw(format!(
            "<a href=\"{t}\" class=\"tag\" target=\"_blank\" rel=\"noopener nofollow\">{t}</a>",
            t = esc(&tag)
        )));
        last = r.end;
    }
    if last < text.len() {
        out.push(Event::Text(CowStr::from(text[last..].to_string())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(t: &str) -> String {
        render(t).into_iter().map(|b| b.html).collect()
    }

    #[test]
    fn blocks_and_lines() {
        let b = render("---\na: 1\n---\n# H\n\npara\nline2\n\n- a\n- b\n");
        let kinds: Vec<_> = b.iter().map(|b| (b.kind, b.line_start, b.line_end)).collect();
        assert_eq!(kinds, vec![("yaml", 0, 2), ("heading", 3, 3), ("paragraph", 5, 6), ("list", 8, 9)]);
    }

    #[test]
    fn obsidian_markup() {
        let h = html("Go [[Note#Sec]] and [[A|b]] ==hi== #tag ![[pic.png|200]]");
        assert!(h.contains("class=\"internal-link\""), "{h}");
        assert!(h.contains(">Note &gt; Sec</a>"), "{h}");
        assert!(h.contains(">b</a>"));
        assert!(h.contains("<mark>hi</mark>"));
        assert!(h.contains("class=\"tag\""));
        assert!(h.contains("src=\"pic.png\" class=\"internal-embed\""));
        assert!(h.contains("alt=\"200\""));
    }

    #[test]
    fn callouts_and_tasks() {
        let h = html("> [!warning]- Be **careful**\n> body text\n");
        assert!(h.contains("data-callout=\"warning\""), "{h}");
        assert!(h.contains("is-collapsed"));
        assert!(h.contains("Be <strong>careful</strong>"), "{h}");
        assert!(h.contains("<p dir=\"auto\">body text</p>"), "{h}");
        let h = html("- [ ] todo ^id1\n- [/] half\n");
        assert!(h.contains("data-task=\" \""), "{h}");
        assert!(h.contains("data-task=\"/\""));
        assert!(!h.contains("^id1"), "{h}");
        assert!(!h.contains("[/]"), "{h}");
    }

    #[test]
    fn code_is_untouched() {
        let h = html("```dataview\nLIST #tag [[x]]\n```\n");
        assert!(h.contains("language-dataview"));
        assert!(h.contains("LIST #tag [[x]]"), "{h}");
        let h = html("%% hidden [[x]] %% shown");
        assert!(!h.contains("hidden"));
    }
}
