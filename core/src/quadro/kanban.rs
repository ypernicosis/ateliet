//! The Kanban plugin's markdown (obsidian-kanban's `list` format), read
//! the way the plugin reads it and written the way it writes it, so a file
//! saved here and one saved by the plugin are the same text. The Quadro's
//! own data goes in `%% ateliet:quadro %%` just before the settings: the
//! plugin finds its settings by reading back from the end of the file, so
//! nothing may follow them.

use super::element::Element;
use super::{Board, Card, FileRef, Format, Lane, CARD_W};
use serde_json::{json, Map, Value};

/// `**Complete**` and the archive heading in the plugin's languages.
const COMPLETE: &[&str] = &["Complete", "Concluído", "Fertiggestellt", "Completato", "完了", "완료됨", "Выполнено", "完成"];
const ARCHIVE: &[&str] = &["Archive", "Arquivado", "Archiv", "Archivio", "アーカイブ", "보관됨", "Архивировать", "归档"];

const QUADRO: &str = "%% ateliet:quadro";
const SETTINGS: &str = "%% kanban:settings";

/// A `%% name` / ``` / json / ``` / `%%` block: its json and the line range.
fn find_block(lines: &[&str], name: &str, last: bool) -> Option<(Value, usize, usize)> {
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.trim_end() == name).map(|(i, _)| i).collect();
    let start = if last { *starts.last()? } else { *starts.first()? };
    let open = start + 1;
    if !lines.get(open)?.trim_start().starts_with("```") {
        return None;
    }
    let close = (open + 1..lines.len()).find(|&i| lines[i].trim_end() == "```")?;
    let json: String = lines[open + 1..close].join("\n");
    let end = if lines.get(close + 1).is_some_and(|l| l.trim_end() == "%%") { close + 1 } else { close };
    Some((serde_json::from_str(&json).unwrap_or(Value::Null), start, end))
}

fn heading(line: &str) -> Option<&str> {
    let t = line.trim_start_matches('#');
    let level = line.len() - t.len();
    if (1..=6).contains(&level) && (t.starts_with(' ') || t.is_empty()) {
        Some(t.trim())
    } else {
        None
    }
}

fn list_item(line: &str) -> Option<&str> {
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = line.strip_prefix(m) {
            return Some(r);
        }
    }
    if matches!(line, "-" | "*" | "+") {
        return Some("");
    }
    None
}

fn is_break(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && (t.chars().all(|c| c == '*') || t.chars().all(|c| c == '-') || t.chars().all(|c| c == '_'))
}

/// The plugin's `parseLaneTitle`: "Title (5)" carries a limit of 5.
fn lane_title(raw: &str) -> (String, u32) {
    let t = raw.replace("<br>", "\n").trim().to_string();
    if let Some(open) = t.rfind('(') {
        if t.ends_with(')') {
            let n = &t[open + 1..t.len() - 1];
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                return (t[..open].trim_end().to_string(), n.parse().unwrap_or(0));
            }
        }
    }
    (t, 0)
}

/// One `- [x] text` item with its continuation lines.
fn card_of(first: &str, more: &[String]) -> (Card, Option<String>) {
    let (check, rest) = match first.strip_prefix('[') {
        Some(r) if r.chars().nth(1) == Some(']') => {
            let c = r.chars().next().unwrap_or(' ');
            let after = &r[c.len_utf8() + 1..];
            (c, after.strip_prefix(' ').unwrap_or(after))
        }
        _ => (' ', first),
    };
    let mut raw = rest.to_string();
    let mut indent = None;
    for l in more {
        raw.push('\n');
        raw.push_str(l);
        if indent.is_none() {
            if l.starts_with('\t') {
                indent = Some("\t".to_string());
            } else if l.starts_with("    ") {
                indent = Some("    ".to_string());
            }
        }
    }
    // the plugin's replaceBrs, dedentNewLines and removeBlockId
    let text = raw.replace("<br>", "\n");
    let text = text.trim();
    let mut lines: Vec<String> = text.split('\n').enumerate().map(|(i, l)| if i == 0 { l.to_string() } else { l.strip_prefix("    ").or_else(|| l.strip_prefix('\t')).unwrap_or(l).to_string() }).collect();
    // the plugin writes the id after the first line; markdown has it at the end
    let mut block = None;
    let n = lines.len();
    for k in [0, n.saturating_sub(1)] {
        if block.is_some() {
            break;
        }
        let Some(line) = lines.get_mut(k) else { continue };
        if let Some(i) = line.rfind(" ^").or_else(|| line.rfind("\t^")) {
            let id = &line[i + 2..];
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                block = Some(id.to_string());
                *line = line[..i].trim_end().to_string();
            }
        }
    }
    let text = lines.iter().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
    let id = block.clone().unwrap_or_else(super::element::new_id);
    (Card { id, text, check, block, x: 0.0, y: 0.0, w: CARD_W, color: None }, indent)
}

pub fn parse(text: &str) -> Result<Board, String> {
    let text = text.replace("\r\n", "\n");
    let mut b = Board::empty(Format::Kanban);
    // the frontmatter, kept as written
    let body = match super::frontmatter(&text) {
        Some(fm) => {
            let start = fm.as_ptr() as usize - text.as_ptr() as usize;
            let close = start + fm.len();
            let end = text[close..].find('\n').map(|i| close + i + 1).unwrap_or(text.len());
            b.kept.frontmatter = text[..end].to_string();
            &text[end..]
        }
        None => return Err("quadro sem frontmatter".into()),
    };
    let mut lines: Vec<&str> = body.split('\n').collect();
    // the settings: the last block, the one the plugin reads
    if let Some((v, s, e)) = find_block(&lines, SETTINGS, true) {
        if let Value::Object(m) = v {
            b.kept.settings = m;
        }
        lines.drain(s..=e);
    }
    let quadro = match find_block(&lines, QUADRO, false) {
        Some((v, s, e)) => {
            lines.drain(s..=e);
            v
        }
        None => Value::Null,
    };
    // columns: a heading, maybe **Complete**, then the list
    let mut prev_break = false;
    let mut in_archive = false;
    let mut item: Option<(String, Vec<String>)> = None;
    let mut blank_run = 0;
    let mut indent: Option<String> = None;
    let flush = |item: &mut Option<(String, Vec<String>)>, b: &mut Board, in_archive: bool, indent: &mut Option<String>| {
        if let Some((first, mut more)) = item.take() {
            while more.last().is_some_and(|l| l.trim().is_empty()) {
                more.pop();
            }
            let (card, ind) = card_of(&first, &more);
            if indent.is_none() {
                *indent = ind;
            }
            if in_archive {
                b.archive.push(card);
            } else if let Some(l) = b.lanes.last_mut() {
                l.cards.push(card);
            }
        }
    };
    for line in &lines {
        if let Some(h) = heading(line) {
            flush(&mut item, &mut b, in_archive, &mut indent);
            if prev_break && ARCHIVE.contains(&h) {
                in_archive = true;
                b.kept.archive_title = h.to_string();
            } else {
                in_archive = false;
                let (title, max) = lane_title(h);
                let mut lane = Lane::new(&title);
                lane.max = max;
                b.lanes.push(lane);
            }
            prev_break = false;
            blank_run = 0;
            continue;
        }
        if line.trim().is_empty() {
            blank_run += 1;
            if let Some((_, more)) = item.as_mut() {
                more.push(String::new());
            }
            continue;
        }
        if is_break(line) && item.is_none() || is_break(line) && blank_run > 0 {
            flush(&mut item, &mut b, in_archive, &mut indent);
            prev_break = true;
            blank_run = 0;
            continue;
        }
        prev_break = false;
        if b.lanes.is_empty() && !in_archive {
            continue;
        }
        if let Some(r) = list_item(line) {
            flush(&mut item, &mut b, in_archive, &mut indent);
            item = Some((r.to_string(), Vec::new()));
            blank_run = 0;
            continue;
        }
        let trimmed = line.trim();
        if item.is_none() && !in_archive {
            if let Some(inner) = trimmed.strip_prefix("**").and_then(|t| t.strip_suffix("**")) {
                if COMPLETE.contains(&inner) {
                    if let Some(l) = b.lanes.last_mut() {
                        l.complete = Some(inner.to_string());
                    }
                }
            }
            blank_run = 0;
            continue;
        }
        // a continuation: indented, or a lazy line right after the item's text
        let indented = line.starts_with("    ") || line.starts_with('\t') || line.starts_with("  ");
        if let Some((_, more)) = item.as_mut() {
            if indented || blank_run == 0 {
                more.push(line.to_string());
            } else {
                // a paragraph after the list ends it
                flush(&mut item, &mut b, in_archive, &mut indent);
            }
        }
        blank_run = 0;
    }
    flush(&mut item, &mut b, in_archive, &mut indent);
    b.kept.indent = indent.unwrap_or_else(|| "\t".into());
    if let Some(c) = b.kept.settings.get("list-collapse").and_then(|v| v.as_array()) {
        for (l, v) in b.lanes.iter_mut().zip(c) {
            l.collapsed = v.as_bool().unwrap_or(false);
        }
    }
    b.default_layout();
    read_quadro(&mut b, &quadro);
    Ok(b)
}

fn f(v: &Value, k: &str, d: f32) -> f32 {
    v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(d)
}

/// The Quadro's own data: column positions (matched by title, then by
/// order), loose cards, drawings and their pictures.
fn read_quadro(b: &mut Board, q: &Value) {
    if !q.is_object() {
        return;
    }
    if let Some(saved) = q["lanes"].as_array() {
        let mut used = vec![false; saved.len()];
        let mut placed = vec![false; b.lanes.len()];
        for pass in 0..2 {
            for (i, l) in b.lanes.iter_mut().enumerate() {
                if placed[i] {
                    continue;
                }
                let found = (0..saved.len()).find(|&j| !used[j] && if pass == 0 { saved[j]["title"].as_str() == Some(l.title.as_str()) } else { j == i });
                if let Some(j) = found {
                    used[j] = true;
                    placed[i] = true;
                    l.x = f(&saved[j], "x", l.x);
                    l.y = f(&saved[j], "y", l.y);
                    l.w = f(&saved[j], "w", l.w).max(120.0);
                }
            }
        }
        // columns new in the file go to the right of the others
        let mut right = b.lanes.iter().zip(&placed).filter(|(_, p)| **p).map(|(l, _)| l.x + l.w).fold(f32::MIN, f32::max);
        for (i, l) in b.lanes.iter_mut().enumerate() {
            if !placed[i] && right > f32::MIN {
                l.x = right + super::LANE_GAP;
                l.y = 0.0;
                right = l.x + l.w;
            }
        }
    }
    for c in q["cards"].as_array().into_iter().flatten() {
        let text = c["text"].as_str().unwrap_or("").to_string();
        let id = c["id"].as_str().map(str::to_string).unwrap_or_else(super::element::new_id);
        let check = c["check"].as_str().and_then(|s| s.chars().next()).unwrap_or(' ');
        b.cards.push(Card { id, text, check, block: None, x: f(c, "x", 0.0), y: f(c, "y", 0.0), w: f(c, "w", CARD_W), color: c["color"].as_str().map(str::to_string) });
    }
    b.elements = q["elements"].as_array().into_iter().flatten().filter_map(|e| Element::from_value(e.clone())).collect();
    if let Some(files) = q["files"].as_object() {
        b.files = files.iter().map(|(k, v)| (k.clone(), FileRef::Vault(v.as_str().unwrap_or("").to_string()))).collect();
    }
}

fn quadro_json(b: &Board) -> Option<String> {
    let bound = b.bound_ids();
    let elements: Vec<Value> = b.elements.iter().map(|e| e.to_value()).collect();
    if elements.is_empty() && b.cards.is_empty() && b.is_default_layout() {
        return None;
    }
    let lanes: Vec<Value> = b.lanes.iter().map(|l| json!({ "title": l.title, "x": l.x.round(), "y": l.y.round(), "w": l.w.round() })).collect();
    let cards: Vec<Value> = b
        .cards
        .iter()
        .map(|c| {
            let mut m = json!({ "id": c.id, "text": c.text, "check": c.check.to_string(), "x": c.x.round(), "y": c.y.round(), "w": c.w.round() });
            if let Some(col) = &c.color {
                m["color"] = json!(col);
            }
            m
        })
        .collect();
    let used: std::collections::HashSet<&str> = b.elements.iter().filter_map(|e| e.get("fileId").as_str()).collect();
    let files: Map<String, Value> = b
        .files
        .iter()
        .filter(|(k, _)| used.contains(k.as_str()))
        .filter_map(|(k, f)| match f {
            FileRef::Vault(p) => Some((k.clone(), json!(p))),
            _ => None,
        })
        .collect();
    let _ = bound;
    let v = json!({ "version": 1, "lanes": lanes, "cards": cards, "elements": elements, "files": files });
    // inside a %% comment and a code fence: no "%%" or "```" may appear
    Some(serde_json::to_string(&v).unwrap_or_default().replace('`', "\\u0060").replace('%', "\\u0025"))
}

/// The plugin's `indentNewLines` and `addBlockId`.
fn item_md(c: &Card, indent: &str) -> String {
    let mut text = c.text.trim().replace('\n', &format!("\n{indent}"));
    if let Some(id) = &c.block {
        let end = text.find('\n').unwrap_or(text.len());
        text.insert_str(end, &format!(" ^{id}"));
    }
    format!("- [{}] {}", c.check, text)
}

pub fn write(b: &mut Board) -> String {
    // cards an arrow points to keep their id in the file (`^id`), so the
    // arrow finds them again
    let bound = b.bound_ids();
    for l in b.lanes.iter_mut() {
        for c in l.cards.iter_mut() {
            if c.block.is_none() && bound.contains(&c.id) {
                c.block = Some(c.id.clone());
            }
        }
    }
    let indent = if b.kept.indent.is_empty() { "\t".to_string() } else { b.kept.indent.clone() };
    let mut out = b.kept.frontmatter.clone();
    if out.is_empty() {
        out = "---\n\nkanban-plugin: board\n\n---\n".into();
    }
    out.push('\n');
    for l in &b.lanes {
        let title = if l.max > 0 { format!("{} ({})", l.title, l.max) } else { l.title.clone() };
        let mut lines = vec![format!("## {}", title.trim().replace('\n', "<br>")), String::new()];
        if let Some(m) = &l.complete {
            lines.push(format!("**{m}**"));
        }
        for c in &l.cards {
            lines.push(item_md(c, &indent));
        }
        lines.extend([String::new(), String::new(), String::new()]);
        out.push_str(&lines.join("\n"));
    }
    if !b.archive.is_empty() {
        let title = if b.kept.archive_title.is_empty() { "Archive" } else { &b.kept.archive_title };
        let mut lines = vec!["***".to_string(), String::new(), format!("## {title}"), String::new()];
        lines.extend(b.archive.iter().map(|c| item_md(c, &indent)));
        out.push_str(&lines.join("\n"));
    }
    if let Some(q) = quadro_json(b) {
        out.push_str(&format!("\n\n{QUADRO}\n```\n{q}\n```\n%%"));
    }
    let mut settings = b.kept.settings.clone();
    if !settings.contains_key("kanban-plugin") {
        settings.insert("kanban-plugin".into(), json!("board"));
    }
    if settings.contains_key("list-collapse") || b.lanes.iter().any(|l| l.collapsed) {
        settings.insert("list-collapse".into(), json!(b.lanes.iter().map(|l| l.collapsed).collect::<Vec<_>>()));
    }
    b.kept.settings = settings.clone();
    out.push_str(&format!("\n\n{SETTINGS}\n```\n{}\n```\n%%", serde_json::to_string(&settings).unwrap_or_default()));
    out
}

#[cfg(test)]
mod tests {
    use super::super::element::Style;
    use super::*;

    /// A board as the plugin writes it (lanes, a done column, multi-line
    /// and block-id cards, dates, tags, the archive and settings).
    const PLUGIN: &str = "---\n\nkanban-plugin: board\n\n---\n\n## Shelf\n\n- [ ] Science Adventure Series\n- [ ] Create Mod #minecraft\n\n\n## To Do (17)\n\n- [ ] [[Dead Cells]]\n- [ ] Papers Please @{2024-05-01}\n- [ ] Multi ^abc123\n\tlinha dois\n\tlinha três\n\n\n## Finished\n\n**Complete**\n- [x] ANIMAL WELL #puzzle #platformer\n\n\n## Vazia\n\n\n\n***\n\n## Archive\n\n- [x] Velho\n\n%% kanban:settings\n```\n{\"kanban-plugin\":\"board\",\"list-collapse\":[false,true,false,false],\"show-checkboxes\":true}\n```\n%%";

    #[test]
    fn reads_the_plugin_format() {
        let b = parse(PLUGIN).unwrap();
        assert_eq!(b.lanes.len(), 4);
        assert_eq!(b.lanes[0].title, "Shelf");
        assert_eq!(b.lanes[1].title, "To Do");
        assert_eq!(b.lanes[1].max, 17);
        assert!(b.lanes[1].collapsed);
        assert_eq!(b.lanes[1].cards[2].text, "Multi\nlinha dois\nlinha três");
        assert_eq!(b.lanes[1].cards[2].block.as_deref(), Some("abc123"));
        assert_eq!(b.lanes[1].cards[2].id, "abc123");
        assert_eq!(b.lanes[2].complete.as_deref(), Some("Complete"));
        assert_eq!(b.lanes[2].cards[0].check, 'x');
        assert!(b.lanes[3].cards.is_empty());
        assert_eq!(b.archive.len(), 1);
        assert_eq!(b.kept.settings["show-checkboxes"], json!(true));
        assert_eq!(b.kept.indent, "\t");
    }

    #[test]
    fn writes_what_it_read() {
        let mut b = parse(PLUGIN).unwrap();
        assert_eq!(b.write(), PLUGIN);
    }

    #[test]
    fn keeps_the_quadro_data() {
        let mut b = parse(PLUGIN).unwrap();
        b.lanes[2].x = 900.0;
        b.lanes[2].y = 300.0;
        let mut loose = Card::new("Ideia solta com `crase` e 100%");
        loose.x = 50.0;
        loose.y = -200.0;
        b.cards.push(loose.clone());
        let mut arrow = Element::new("arrow", 0.0, 0.0, 10.0, 10.0, &Style::default());
        let target = b.lanes[0].cards[0].id.clone();
        arrow.set("endBinding", json!({ "elementId": target, "focus": 0, "gap": 6 }));
        arrow.set("startBinding", json!({ "elementId": loose.id, "focus": 0, "gap": 6 }));
        b.elements.push(arrow);
        let text = b.write();
        // the plugin still finds its settings at the very end, and the block before them
        assert!(text.ends_with("```\n%%"));
        let q = text.find(QUADRO).unwrap();
        assert!(q < text.find(SETTINGS).unwrap());
        assert!(!text[q..].contains("`crase`"));
        // the card an arrow points to got its id in the file
        assert!(text.contains(&format!("- [ ] Science Adventure Series ^{target}")));
        let mut back = parse(&text).unwrap();
        assert_eq!(back.lanes[2].x, 900.0);
        assert_eq!(back.lanes[2].y, 300.0);
        assert_eq!(back.cards, vec![loose]);
        assert_eq!(back.elements.len(), 1);
        assert_eq!(back.elements[0].binding(true).as_deref(), Some(target.as_str()));
        assert_eq!(back.lanes[0].cards[0].id, target);
        // and writes the same again
        assert_eq!(back.write(), text);
    }

    #[test]
    fn new_board_is_a_plugin_board() {
        let text = super::super::new_board_text(&["A fazer", "Feito"]);
        assert_eq!(text, "---\n\nkanban-plugin: board\n\n---\n\n## A fazer\n\n\n\n## Feito\n\n\n\n\n\n%% kanban:settings\n```\n{\"kanban-plugin\":\"board\"}\n```\n%%");
        let b = parse(&text).unwrap();
        assert_eq!(b.lanes.len(), 2);
    }

    #[test]
    fn plain_items_and_portuguese_markers() {
        let t = "---\nkanban-plugin: board\n---\n\n## Feito\n\n**Concluído**\n- item sem caixa\n- [x] feito<br>em duas linhas\n\n***\n\n## Arquivado\n\n- [x] antigo\n";
        let mut b = parse(t).unwrap();
        assert_eq!(b.lanes.len(), 1);
        assert_eq!(b.lanes[0].complete.as_deref(), Some("Concluído"));
        assert_eq!(b.lanes[0].cards[0].text, "item sem caixa");
        assert_eq!(b.lanes[0].cards[1].text, "feito\nem duas linhas");
        assert_eq!(b.archive.len(), 1);
        let out = b.write();
        assert!(out.contains("**Concluído**\n- [ ] item sem caixa\n- [x] feito\n\tem duas linhas\n"));
        assert!(out.contains("***\n\n## Arquivado\n\n- [x] antigo"));
    }
}
