//! Links follow a renamed or moved file: wiki links (`[[…]]`, `![[…]]`,
//! with `#heading` and `|alias` kept) and Markdown links (`[…](…)`) whose
//! target resolved to an old path are rewritten to the new one, in the
//! style they were written (by name, or by path). Code is left alone.

/// How a link to `to` is written when the old link named only the file.
fn short(to: &str) -> String {
    let name = to.rsplit('/').next().unwrap_or(to);
    name.strip_suffix(".md").unwrap_or(name).to_string()
}

fn long(to: &str) -> String {
    to.strip_suffix(".md").unwrap_or(to).to_string()
}

fn encode(s: &str) -> String {
    s.replace('%', "%25").replace(' ', "%20").replace('(', "%28").replace(')', "%29")
}

/// The new target text for a link that pointed at a moved file.
fn new_target(old: &str, to: &str, markdown: bool) -> String {
    if markdown {
        // Markdown links keep their path style, with the extension
        let decoded = crate::syntax::percent_decode(old);
        let t = if decoded.contains('/') { to.to_string() } else { to.rsplit('/').next().unwrap_or(to).to_string() };
        let t = if decoded.to_lowercase().ends_with(".md") || !to.ends_with(".md") { t } else { t.strip_suffix(".md").unwrap_or(&t).to_string() };
        return encode(&t);
    }
    let keeps_ext = old.to_lowercase().ends_with(".md");
    let mut t = if old.contains('/') { long(to) } else { short(to) };
    if !to.ends_with(".md") {
        // attachments keep their extension
        t = if old.contains('/') { to.to_string() } else { to.rsplit('/').next().unwrap_or(to).to_string() };
    } else if keeps_ext {
        t.push_str(".md");
    }
    t
}

/// `text` with every link that `resolve` maps onto a key of `moved` pointed
/// at the new path; `None` when nothing changed.
pub fn rewrite(text: &str, moved: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut in_fence = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let b = line.as_bytes();
        let mut i = 0;
        let mut last = 0;
        while i < b.len() {
            // inline code
            if b[i] == b'`' {
                if let Some(j) = line[i + 1..].find('`') {
                    i += j + 2;
                    continue;
                }
            }
            if b[i] == b'[' && i + 1 < b.len() && b[i + 1] == b'[' {
                if let Some(end) = line[i + 2..].find("]]") {
                    let inner = &line[i + 2..i + 2 + end];
                    let (target_sub, alias) = match inner.split_once('|') {
                        Some((t, a)) => (t, Some(a)),
                        None => (inner, None),
                    };
                    let (target, sub) = match target_sub.split_once('#') {
                        Some((t, s)) => (t, Some(s)),
                        None => (target_sub, None),
                    };
                    if !target.is_empty() {
                        if let Some(to) = moved(target) {
                            let nt = new_target(target, &to, false);
                            out.push_str(&line[last..i + 2]);
                            out.push_str(&nt);
                            if let Some(s) = sub {
                                out.push('#');
                                out.push_str(s);
                            }
                            if let Some(a) = alias {
                                out.push('|');
                                out.push_str(a);
                            }
                            out.push_str("]]");
                            last = i + 2 + end + 2;
                            changed = true;
                        }
                    }
                    i += 2 + end + 2;
                    continue;
                }
            }
            if b[i] == b']' && i + 1 < b.len() && b[i + 1] == b'(' {
                if let Some(end) = line[i + 2..].find(')') {
                    let url = &line[i + 2..i + 2 + end];
                    let (path, sub) = match url.split_once('#') {
                        Some((p, s)) => (p, Some(s)),
                        None => (url, None),
                    };
                    let external = path.contains("://") || path.starts_with("mailto:") || path.is_empty() || path.starts_with('<');
                    if !external {
                        if let Some(to) = moved(&crate::syntax::percent_decode(path)) {
                            out.push_str(&line[last..i + 2]);
                            out.push_str(&new_target(path, &to, true));
                            if let Some(s) = sub {
                                out.push('#');
                                out.push_str(s);
                            }
                            out.push(')');
                            last = i + 2 + end + 1;
                            changed = true;
                        }
                    }
                    i += 2 + end + 1;
                    continue;
                }
            }
            i += line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        out.push_str(&line[last..]);
    }
    changed.then_some(out)
}

/// Byte ranges of the text that are links or code (where a mention is not one).
fn covered(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut fence = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") || fence {
            if t.starts_with("```") || t.starts_with("~~~") {
                fence = !fence;
            }
            out.push((at, at + line.len()));
            at += line.len();
            continue;
        }
        // frontmatter is not prose either
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let (open, close): (&str, &str) = if line[i..].starts_with("[[") { ("[[", "]]") } else if b[i] == b'`' { ("`", "`") } else if b[i] == b'[' { ("[", ")") } else { ("", "") };
            if !open.is_empty() {
                if let Some(j) = line[i + open.len()..].find(close) {
                    let end = i + open.len() + j + close.len();
                    out.push((at + i, at + end));
                    i = end;
                    continue;
                }
            }
            i += line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        at += line.len();
    }
    out
}

fn frontmatter_end(text: &str) -> usize {
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(i) = rest.find("\n---") {
            return 4 + i + 4;
        }
    }
    0
}

/// Where `name` is mentioned as a whole word (any case) outside links,
/// code and frontmatter: (byte offset, a line of context).
pub fn unlinked_mentions(text: &str, name: &str) -> Vec<(usize, String)> {
    let name = name.trim();
    if name.chars().count() < 2 {
        return Vec::new();
    }
    let lower = text.to_lowercase();
    let needle = name.to_lowercase();
    // lowercasing can change byte lengths; only search when it did not
    if lower.len() != text.len() {
        return Vec::new();
    }
    let skip = covered(text);
    let fm = frontmatter_end(text);
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find(&needle) {
        let s = from + i;
        let e = s + needle.len();
        from = e;
        if s < fm || word(text[..s].chars().next_back()) || word(text[e..].chars().next()) || skip.iter().any(|(a, b)| s >= *a && s < *b) {
            continue;
        }
        let ls = text[..s].rfind('\n').map(|x| x + 1).unwrap_or(0);
        let le = text[e..].find('\n').map(|x| e + x).unwrap_or(text.len());
        out.push((s, text[ls..le].trim().to_string()));
    }
    out
}

/// The mention at `at` turned into a link to `target` (shown as written).
pub fn link_mention(text: &str, at: usize, len: usize, target: &str) -> String {
    let shown = &text[at..at + len];
    let link = if shown == target { format!("[[{target}]]") } else { format!("[[{target}|{shown}]]") };
    format!("{}{}{}", &text[..at], link, &text[at + len..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_follow() {
        let moved = |t: &str| -> Option<String> {
            match t.to_lowercase().as_str() {
                "ana" | "pessoas/ana" | "pessoas/ana.md" => Some("Elenco/Ana Clara.md".into()),
                "img.png" => Some("Anexos/img.png".into()),
                _ => None,
            }
        };
        let text = "Ver [[Ana]], [[Pessoas/Ana#Infância|ela]] e ![[img.png]].\n`[[Ana]]` fica.\n```\n[[Ana]]\n```\n[x](Pessoas/Ana.md) [y](https://a.b/Ana) [[Outra]]\n";
        let out = rewrite(text, &moved).unwrap();
        assert!(out.contains("[[Ana Clara]], [[Elenco/Ana Clara#Infância|ela]] e ![[img.png]]"));
        assert!(out.contains("`[[Ana]]` fica."));
        assert!(out.contains("```\n[[Ana]]\n```"));
        assert!(out.contains("[x](Elenco/Ana%20Clara.md)"));
        assert!(out.contains("[y](https://a.b/Ana) [[Outra]]"));
        assert!(rewrite("nada aqui", &moved).is_none());
        let t = "---\ntitle: Ana\n---\nAna veio. [[Ana]] e `Ana` e Anabela. Falei com ana.\n";
        let m = unlinked_mentions(t, "Ana");
        assert_eq!(m.len(), 2);
        let linked = link_mention(t, m[1].0, 3, "Ana");
        assert!(linked.ends_with("Falei com [[Ana|ana]].\n"));
    }
}
