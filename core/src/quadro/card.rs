//! A card's text as the Kanban plugin reads it: `[[links]]`, `![[embeds]]`,
//! `#tags`, a date (`@{2024-05-01}` or `@[[2024-05-01]]`) and a time
//! (`@@{14:30}`). Tags, dates and times are shown apart from the title.

/// One piece of a card's title.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Text(String),
    /// (target, shown text)
    Link(String, String),
    Embed(String),
    Url(String, String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub segs: Vec<Seg>,
    pub tags: Vec<String>,
    pub date: Option<String>,
    pub time: Option<String>,
}

impl Info {
    /// The note the card stands for: its first link or embed that is not a picture.
    pub fn note(&self) -> Option<&str> {
        self.segs.iter().find_map(|s| match s {
            Seg::Link(t, _) => Some(t.as_str()),
            Seg::Embed(t) if !is_picture(t) => Some(t.as_str()),
            _ => None,
        })
    }

    pub fn picture(&self) -> Option<&str> {
        self.segs.iter().find_map(|s| match s {
            Seg::Embed(t) if is_picture(t) => Some(t.as_str()),
            _ => None,
        })
    }

    /// The card is only a link to a note (clicking it opens the note).
    pub fn is_note_card(&self) -> bool {
        let real: Vec<&Seg> = self.segs.iter().filter(|s| !matches!(s, Seg::Text(t) if t.trim().is_empty())).collect();
        matches!(real.as_slice(), [Seg::Link(..)] | [Seg::Embed(_)]) && self.note().is_some()
    }

    pub fn plain(&self) -> String {
        self.segs
            .iter()
            .map(|s| match s {
                Seg::Text(t) => t.clone(),
                Seg::Link(_, a) | Seg::Url(_, a) => a.clone(),
                Seg::Embed(t) => t.clone(),
            })
            .collect()
    }
}

pub fn is_picture(target: &str) -> bool {
    let t = target.split(['#', '|']).next().unwrap_or(target).to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"].iter().any(|e| t.ends_with(e))
}

/// The link target without a heading, block or alias.
pub fn link_target(inner: &str) -> (String, String) {
    let (target, alias) = match inner.split_once('|') {
        Some((t, a)) => (t.to_string(), a.to_string()),
        None => (inner.to_string(), String::new()),
    };
    let shown = if alias.is_empty() {
        let base = target.split('#').next().unwrap_or(&target);
        base.rsplit('/').next().unwrap_or(base).trim_end_matches(".md").to_string()
    } else {
        alias
    };
    (target, shown)
}

fn tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// Read a card's text (`date_trigger` is the plugin's "@", `time_trigger` "@@").
pub fn parse(text: &str, date_trigger: &str, time_trigger: &str) -> Info {
    let mut info = Info::default();
    let mut cur = String::new();
    let b = text;
    let mut i = 0;
    let push_text = |cur: &mut String, segs: &mut Vec<Seg>| {
        if !cur.is_empty() {
            segs.push(Seg::Text(std::mem::take(cur)));
        }
    };
    while i < b.len() {
        let rest = &b[i..];
        // time before date: "@@{" starts with "@"
        if !time_trigger.is_empty() && rest.starts_with(&format!("{time_trigger}{{")) {
            if let Some(end) = rest.find('}') {
                info.time = Some(rest[time_trigger.len() + 1..end].to_string());
                i += end + 1;
                continue;
            }
        }
        if !date_trigger.is_empty() && rest.starts_with(&format!("{date_trigger}{{")) {
            if let Some(end) = rest.find('}') {
                info.date = Some(rest[date_trigger.len() + 1..end].to_string());
                i += end + 1;
                continue;
            }
        }
        if !date_trigger.is_empty() && rest.starts_with(&format!("{date_trigger}[[")) {
            if let Some(end) = rest.find("]]") {
                info.date = Some(rest[date_trigger.len() + 2..end].to_string());
                i += end + 2;
                continue;
            }
        }
        if rest.starts_with("![[") {
            if let Some(end) = rest.find("]]") {
                push_text(&mut cur, &mut info.segs);
                info.segs.push(Seg::Embed(rest[3..end].split('|').next().unwrap_or("").to_string()));
                i += end + 2;
                continue;
            }
        }
        if rest.starts_with("[[") {
            if let Some(end) = rest.find("]]") {
                push_text(&mut cur, &mut info.segs);
                let (t, shown) = link_target(&rest[2..end]);
                info.segs.push(Seg::Link(t, shown));
                i += end + 2;
                continue;
            }
        }
        // [text](url): a markdown link
        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close..].find(')') {
                    let label = &rest[1..close];
                    let url = &rest[close + 2..close + end];
                    if !label.contains('\n') && !url.contains(' ') {
                        push_text(&mut cur, &mut info.segs);
                        let decoded = percent_encoding::percent_decode_str(url).decode_utf8_lossy().to_string();
                        if url.contains("://") {
                            info.segs.push(Seg::Url(url.to_string(), label.to_string()));
                        } else {
                            info.segs.push(Seg::Link(decoded.trim_end_matches(".md").to_string(), label.to_string()));
                        }
                        i += close + end + 1;
                        continue;
                    }
                }
            }
        }
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            push_text(&mut cur, &mut info.segs);
            info.segs.push(Seg::Url(rest[..end].to_string(), rest[..end].to_string()));
            i += end;
            continue;
        }
        // #tag at a word start, not a heading "# "
        let at_word = cur.is_empty() || cur.ends_with(char::is_whitespace);
        if rest.starts_with('#') && at_word && rest[1..].chars().next().is_some_and(|c| tag_char(c) && !c.is_ascii_digit() || c.is_alphabetic()) {
            let len = 1 + rest[1..].find(|c: char| !tag_char(c)).unwrap_or(rest.len() - 1);
            info.tags.push(rest[..len].to_string());
            i += len;
            continue;
        }
        let c = rest.chars().next().unwrap();
        cur.push(c);
        i += c.len_utf8();
    }
    push_text(&mut cur, &mut info.segs);
    // the spaces a removed tag or date left behind
    if let Some(Seg::Text(t)) = info.segs.last_mut() {
        *t = t.trim_end().to_string();
    }
    if let Some(Seg::Text(t)) = info.segs.first_mut() {
        *t = t.trim_start().to_string();
    }
    for s in info.segs.iter_mut() {
        if let Seg::Text(t) = s {
            while t.contains("  ") {
                *t = t.replace("  ", " ");
            }
            *t = t.replace("**", "").replace("__", "");
        }
    }
    info.segs.retain(|s| !matches!(s, Seg::Text(t) if t.is_empty()));
    info
}

/// The text with its date replaced (None removes it), as the plugin writes it.
pub fn with_date(text: &str, date: Option<&str>, trigger: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    let open = format!("{trigger}{{");
    let open_link = format!("{trigger}[[");
    loop {
        let a = rest.find(&open).filter(|&i| !rest[..i].ends_with(trigger) || trigger.is_empty());
        let b = rest.find(&open_link);
        let next = match (a, b) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (x, y) => x.or(y),
        };
        let Some(i) = next else { break };
        out.push_str(&rest[..i]);
        let close = if rest[i..].starts_with(&open_link) { "]]" } else { "}" };
        match rest[i..].find(close) {
            Some(e) => rest = &rest[i + e + close.len()..],
            None => {
                out.push_str(&rest[i..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    let mut out = out.split('\n').map(|l| l.trim_end().replace("  ", " ")).collect::<Vec<_>>().join("\n");
    if let Some(d) = date {
        // on the first line, as the plugin adds it
        let first_end = out.find('\n').unwrap_or(out.len());
        let sep = if out[..first_end].is_empty() { "" } else { " " };
        out.insert_str(first_end, &format!("{sep}{trigger}{{{d}}}"));
    }
    out
}

/// Add a tag (on the first line), unless it is there already.
pub fn with_tag(text: &str, tag: &str) -> String {
    let tag = if tag.starts_with('#') { tag.to_string() } else { format!("#{tag}") };
    let info = parse(text, "@", "@@");
    if info.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
        return text.to_string();
    }
    let first_end = text.find('\n').unwrap_or(text.len());
    let mut out = text.to_string();
    let sep = if text[..first_end].is_empty() { "" } else { " " };
    out.insert_str(first_end, &format!("{sep}{tag}"));
    out
}

/// Take a tag out of the text.
pub fn without_tag(text: &str, tag: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let at_word = out.is_empty() || out.ends_with(char::is_whitespace);
        if at_word && rest.len() >= tag.len() && rest[..tag.len()].eq_ignore_ascii_case(tag) && !rest[tag.len()..].starts_with(tag_char) {
            i += tag.len();
            if out.ends_with(' ') && (rest[tag.len()..].starts_with(' ') || rest.len() == tag.len() || rest[tag.len()..].starts_with('\n')) {
                out.pop();
            }
            continue;
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_card() {
        let i = parse("Ler [[Livro A|o livro]] #leitura #rpg @{2024-05-01} @@{14:30}", "@", "@@");
        assert_eq!(i.tags, vec!["#leitura", "#rpg"]);
        assert_eq!(i.date.as_deref(), Some("2024-05-01"));
        assert_eq!(i.time.as_deref(), Some("14:30"));
        assert_eq!(i.segs, vec![Seg::Text("Ler ".into()), Seg::Link("Livro A".into(), "o livro".into())]);
        assert!(!i.is_note_card());
        let n = parse("[[Pasta/Nota]]", "@", "@@");
        assert!(n.is_note_card());
        assert_eq!(n.note(), Some("Pasta/Nota"));
        let p = parse("![[foto.png]]", "@", "@@");
        assert_eq!(p.picture(), Some("foto.png"));
        assert!(!p.is_note_card());
        // a heading-like "# " and "#1" are not tags
        assert!(parse("item #1 e # título", "@", "@@").tags.is_empty());
    }

    #[test]
    fn edits_dates_and_tags() {
        assert_eq!(with_date("Tarefa @{2024-01-01} feita", Some("2024-02-02"), "@"), "Tarefa feita @{2024-02-02}");
        assert_eq!(with_date("Tarefa @{2024-01-01}", None, "@"), "Tarefa");
        assert_eq!(with_date("Tarefa\nmais", Some("2024-02-02"), "@"), "Tarefa @{2024-02-02}\nmais");
        assert_eq!(with_tag("Tarefa", "urgente"), "Tarefa #urgente");
        assert_eq!(with_tag("Tarefa #urgente", "#urgente"), "Tarefa #urgente");
        assert_eq!(without_tag("Tarefa #urgente #rpg", "#urgente"), "Tarefa #rpg");
    }
}
