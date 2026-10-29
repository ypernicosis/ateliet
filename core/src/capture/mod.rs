//! Excerpts (trechos): every piece of text that reaches a Journal page from
//! outside it — a passage of a book, of a web page, of the manuscript — is
//! one, kept as a short note in `Trechos/` with its source in the
//! frontmatter, so it can be searched, linked and queried without the
//! Journal ("every excerpt of this book", "excerpts not used yet").
//!
//! - `inbox` — what arrives from outside the app, through a folder.

pub mod inbox;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Where an excerpt came from.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct SourceRef {
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The book's note in the vault (`Livros/…`), when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Where in the book (the reader's locator), as it gave it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<Value>,
}

impl SourceRef {
    /// "Title · Author", as the callout's header shows it.
    pub fn line(&self) -> String {
        match (&self.author, self.title.trim()) {
            (Some(a), "") => a.clone(),
            (Some(a), t) if !a.trim().is_empty() => format!("{t} · {}", a.trim()),
            (_, t) => t.to_string(),
        }
    }

    /// The link closing an excerpt: `[fonte](url)`, `[[book]]`, or nothing.
    pub fn foot(&self) -> String {
        if let Some(b) = &self.book {
            return format!("[[{}]]", b.trim_end_matches(".md"));
        }
        match &self.url {
            Some(u) if !u.is_empty() => format!("[fonte]({u})"),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Snippet {
    #[serde(default)]
    pub id: String,
    /// The passage, in Markdown.
    #[serde(default)]
    pub text: String,
    /// A region cut out of a page, when the excerpt is a picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// A comment that came with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub source: SourceRef,
    /// When it was taken (`YYYY-MM-DD HH:MM`).
    #[serde(default)]
    pub captured: String,
    /// `leitor`, `web`, `scriptorium`, `manual`.
    #[serde(default)]
    pub origin: String,
}

pub const FOLDER: &str = "Trechos";

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

impl Snippet {
    pub fn new(text: &str, source: SourceRef, origin: &str) -> Snippet {
        Snippet { id: crate::journal::short_id(), text: text.trim().to_string(), image: None, note: None, source, captured: now(), origin: origin.to_string() }
    }

    /// The note's file name: the source and the start of the text.
    pub fn file_name(&self) -> String {
        let words: String = self.text.split_whitespace().take(7).collect::<Vec<_>>().join(" ");
        let base = match self.source.title.trim() {
            "" => words,
            t => format!("{t} – {words}"),
        };
        let clean: String = base.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']' | '\n' | '\r')).collect();
        let clean: String = clean.chars().take(80).collect();
        let clean = clean.trim().trim_end_matches('.');
        if clean.is_empty() {
            format!("Trecho {}.md", self.captured.replace(':', "h"))
        } else {
            format!("{clean}.md")
        }
    }

    /// The note: the source in the frontmatter, the passage in a callout.
    pub fn to_note(&self) -> String {
        let mut fm: Vec<(&str, Value)> = vec![("tipo", json!("trecho")), ("fonte", json!(self.source.title))];
        if let Some(a) = &self.source.author {
            fm.push(("autor", json!(a)));
        }
        if let Some(b) = &self.source.book {
            fm.push(("livro", json!(format!("[[{}]]", b.trim_end_matches(".md")))));
        }
        if let Some(u) = &self.source.url {
            fm.push(("url", json!(u)));
        }
        if let Some(l) = &self.source.locator {
            fm.push(("local", l.clone()));
        }
        fm.push(("capturado", json!(self.captured)));
        if !self.origin.is_empty() {
            fm.push(("origem", json!(self.origin)));
        }
        let mut body = String::from("\n");
        body.push_str(&crate::journal::excerpt_section(&self.source.line(), &self.text, &self.source.foot()));
        body.push('\n');
        if let Some(img) = &self.image {
            body.push_str(&format!("\n![[{img}]]\n"));
        }
        if let Some(n) = self.note.as_deref().filter(|n| !n.trim().is_empty()) {
            body.push('\n');
            body.push_str(n.trim());
            body.push('\n');
        }
        crate::scriptorium::note_with(&fm, &body)
    }

    /// An excerpt's note read back; None when the note is not one.
    pub fn from_note(text: &str) -> Option<Snippet> {
        let fm = crate::syntax::frontmatter(text)?;
        let y = crate::syntax::parse_yaml(&text[fm.yaml.clone()])?;
        if y["tipo"].as_str() != Some("trecho") {
            return None;
        }
        let body = &text[fm.block.end..];
        let s = |k: &str| y[k].as_str().map(|v| v.to_string()).filter(|v| !v.is_empty());
        // the callout, then the rest (a picture, a comment)
        let mut quote = String::new();
        let mut rest = String::new();
        let mut in_quote = false;
        let mut quote_done = false;
        for l in body.lines() {
            if !quote_done && l.trim_start().starts_with("> [!trecho]") {
                in_quote = true;
            }
            if in_quote && l.starts_with('>') {
                quote.push_str(l);
                quote.push('\n');
                continue;
            }
            if in_quote {
                in_quote = false;
                quote_done = true;
            }
            rest.push_str(l);
            rest.push('\n');
        }
        let parts = crate::journal::excerpt_parts(&quote);
        let image = crate::journal::embed_target(&rest);
        let note: String = rest.lines().filter(|l| crate::journal::embed_target(l).is_none()).collect::<Vec<_>>().join("\n").trim().to_string();
        Some(Snippet {
            id: String::new(),
            text: parts.text,
            image,
            note: (!note.is_empty()).then_some(note),
            source: SourceRef {
                title: s("fonte").unwrap_or_default(),
                author: s("autor"),
                book: s("livro").map(|b| b.trim_start_matches("[[").trim_end_matches("]]").to_string()),
                url: s("url"),
                locator: y.get("local").cloned().filter(|v| !v.is_null()),
            },
            captured: s("capturado").unwrap_or_default(),
            origin: s("origem").unwrap_or_default(),
        })
    }

    /// The excerpt as a page's section (`> [!trecho] …`).
    pub fn section(&self) -> String {
        crate::journal::excerpt_section(&self.source.line(), &self.text, &self.source.foot())
    }
}

/// Is this an excerpt's note? Reads only its frontmatter.
pub fn is_snippet_note(text: &str) -> bool {
    crate::syntax::frontmatter(text).is_some_and(|fm| text[fm.yaml].lines().any(|l| l.trim() == "tipo: trecho"))
}

/// A free name for `name` in `dir`: `name`, `name 2`, `name 3`…
pub fn free_name(dir: &std::path::Path, name: &str) -> String {
    let (stem, ext) = name.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((name.to_string(), String::new()));
    let mut n = 1;
    loop {
        let cand = if n == 1 { format!("{stem}{ext}") } else { format!("{stem} {n}{ext}") };
        if !dir.join(&cand).exists() {
            return cand;
        }
        n += 1;
    }
}

/// Writes an excerpt's note into `folder` of the vault; its vault path.
pub fn save(root: &std::path::Path, folder: &str, s: &Snippet) -> Result<String, String> {
    let dir = if folder.is_empty() { root.to_path_buf() } else { root.join(folder) };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = free_name(&dir, &s.file_name());
    crate::quadro::write_atomic(&dir.join(&name), &s.to_note()).map_err(|e| e.to_string())?;
    Ok(if folder.is_empty() { name } else { format!("{folder}/{name}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_read_back() {
        let mut s = Snippet::new("Montane woodland is a virtually extinct habitat.\n\nOnly a mere 4%.", SourceRef { title: "The Mountain Birch Project".into(), author: Some("Mossy Earth".into()), url: Some("https://mossy.earth".into()), ..Default::default() }, "web");
        s.note = Some("Ver o vídeo inteiro.".into());
        s.image = Some("Anexos/b.png".into());
        let t = s.to_note();
        assert!(is_snippet_note(&t));
        let back = Snippet::from_note(&t).unwrap();
        assert_eq!(back.text, s.text);
        assert_eq!(back.source.title, "The Mountain Birch Project");
        assert_eq!(back.source.author.as_deref(), Some("Mossy Earth"));
        assert_eq!(back.note.as_deref(), Some("Ver o vídeo inteiro."));
        assert_eq!(back.image.as_deref(), Some("Anexos/b.png"));
        assert!(s.file_name().starts_with("The Mountain Birch Project – Montane woodland"));
        assert!(s.section().contains("— [fonte](https://mossy.earth)"));
    }
}
