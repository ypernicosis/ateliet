//! A forgiving XML/XHTML reader: enough for an EPUB's container, package,
//! navigation and chapters. It never fails: unknown entities stay as
//! written, unclosed tags close at the end, HTML-style void tags are fine.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// `<name a="b">` (`empty` for `<name/>`), the name in lower case and
    /// without its namespace prefix.
    Open { name: String, attrs: Vec<(String, String)>, empty: bool },
    Close(String),
    Text(String),
}

/// A tag's attribute, by local name (`epub:type` is found as `type` too).
pub fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).or_else(|| attrs.iter().find(|(k, _)| k.rsplit(':').next() == Some(key))).map(|(_, v)| v.as_str())
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

pub fn entity(name: &str) -> Option<String> {
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "shy" => '\u{ad}',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "middot" => '·',
        "bull" => '•',
        "sect" => '§',
        "para" => '¶',
        "times" => '×',
        "divide" => '÷',
        "thinsp" => '\u{2009}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "zwj" => '\u{200d}',
        "zwnj" => '\u{200c}',
        "iexcl" => '¡',
        "iquest" => '¿',
        "ordf" => 'ª',
        "ordm" => 'º',
        "aacute" => 'á',
        "eacute" => 'é',
        "iacute" => 'í',
        "oacute" => 'ó',
        "uacute" => 'ú',
        "Aacute" => 'Á',
        "Eacute" => 'É',
        "Iacute" => 'Í',
        "Oacute" => 'Ó',
        "Uacute" => 'Ú',
        "agrave" => 'à',
        "Agrave" => 'À',
        "egrave" => 'è',
        "acirc" => 'â',
        "ecirc" => 'ê',
        "ocirc" => 'ô',
        "Acirc" => 'Â',
        "Ecirc" => 'Ê',
        "Ocirc" => 'Ô',
        "atilde" => 'ã',
        "otilde" => 'õ',
        "Atilde" => 'Ã',
        "Otilde" => 'Õ',
        "ccedil" => 'ç',
        "Ccedil" => 'Ç',
        "uuml" => 'ü',
        "ouml" => 'ö',
        "auml" => 'ä',
        "euml" => 'ë',
        "iuml" => 'ï',
        "ntilde" => 'ñ',
        "szlig" => 'ß',
        _ => {
            let n = name.strip_prefix('#')?;
            let v = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            return char::from_u32(v).map(|c| c.to_string());
        }
    };
    Some(c.to_string())
}

/// Text with its entities read.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail[1..].find(';').filter(|e| *e <= 12).and_then(|e| entity(&tail[1..e + 1]).map(|c| (c, e))) {
            Some((c, e)) => {
                out.push_str(&c);
                rest = &tail[e + 2..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_whitespace() || b[i] == '/') {
            i += 1;
        }
        let k0 = i;
        while i < b.len() && !b[i].is_whitespace() && b[i] != '=' && b[i] != '/' {
            i += 1;
        }
        if k0 == i {
            i += 1;
            continue;
        }
        let key: String = b[k0..i].iter().collect();
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < b.len() && b[i] == '=' {
            i += 1;
            while i < b.len() && b[i].is_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == '"' || b[i] == '\'') {
                let q = b[i];
                i += 1;
                let v0 = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                val = b[v0..i].iter().collect();
                i += 1;
            } else {
                let v0 = i;
                while i < b.len() && !b[i].is_whitespace() {
                    i += 1;
                }
                val = b[v0..i].iter().collect();
            }
        }
        out.push((key.to_ascii_lowercase(), unescape(&val)));
    }
    out
}

/// The tokens of a document (comments, declarations and processing
/// instructions dropped; CDATA as text).
pub fn tokens(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut rest = src;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push(Tok::Text(unescape(rest)));
            break;
        };
        if lt > 0 {
            out.push(Tok::Text(unescape(&rest[..lt])));
        }
        rest = &rest[lt..];
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map(|e| &r[e + 3..]).unwrap_or("");
            continue;
        }
        if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let e = r.find("]]>").unwrap_or(r.len());
            out.push(Tok::Text(r[..e].to_string()));
            rest = r.get(e + 3..).unwrap_or("");
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = rest.find('>').map(|e| &rest[e + 1..]).unwrap_or("");
            continue;
        }
        // the tag's end, quotes respected
        let mut end = None;
        let mut quote: Option<char> = None;
        for (i, c) in rest.char_indices().skip(1) {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == '>' => {
                    end = Some(i);
                    break;
                }
                None if c == '<' => {
                    // a stray '<' in text
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push(Tok::Text("<".into()));
            rest = &rest[1..];
            continue;
        };
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Tok::Close(local(name.trim())));
            continue;
        }
        let empty = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let (name, attrs) = match inner.find(|c: char| c.is_whitespace()) {
            Some(s) => (&inner[..s], parse_attrs(&inner[s..])),
            None => (inner, Vec::new()),
        };
        let name = local(name);
        let void = matches!(name.as_str(), "br" | "img" | "hr" | "meta" | "link" | "input" | "col" | "wbr" | "source" | "area" | "base");
        out.push(Tok::Open { name, attrs, empty: empty || void });
    }
    out
}

/// A small element tree, for the package and navigation documents.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn attr(&self, k: &str) -> Option<&str> {
        attr(&self.attrs, k)
    }

    /// Every descendant named `name`, in document order.
    pub fn all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for k in &self.kids {
            if k.name == name {
                out.push(k);
            }
            k.all(name, out);
        }
    }

    pub fn find_all(&self, name: &str) -> Vec<&Node> {
        let mut v = Vec::new();
        self.all(name, &mut v);
        v
    }

    pub fn first(&self, name: &str) -> Option<&Node> {
        self.find_all(name).into_iter().next()
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name == name)
    }

    /// All the text inside, spaces collapsed.
    pub fn all_text(&self) -> String {
        let mut s = self.text.clone();
        for k in &self.kids {
            s.push(' ');
            s.push_str(&k.all_text());
        }
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

pub fn tree(src: &str) -> Node {
    let mut stack: Vec<Node> = vec![Node::default()];
    for t in tokens(src) {
        match t {
            Tok::Open { name, attrs, empty } => {
                let n = Node { name, attrs, ..Default::default() };
                if empty {
                    stack.last_mut().unwrap().kids.push(n);
                } else {
                    stack.push(n);
                }
            }
            Tok::Close(name) => {
                if let Some(pos) = stack.iter().rposition(|n| n.name == name) {
                    if pos == 0 {
                        continue;
                    }
                    while stack.len() > pos {
                        let n = stack.pop().unwrap();
                        stack.last_mut().unwrap().kids.push(n);
                    }
                }
            }
            Tok::Text(t) => {
                let top = stack.last_mut().unwrap();
                top.text.push_str(&t);
            }
        }
    }
    while stack.len() > 1 {
        let n = stack.pop().unwrap();
        stack.last_mut().unwrap().kids.push(n);
    }
    stack.pop().unwrap()
}

/// A path relative to the document at `base` (both inside the archive).
pub fn join(base: &str, rel: &str) -> String {
    let rel = rel.split('#').next().unwrap_or(rel);
    let rel = percent_encoding::percent_decode_str(rel).decode_utf8_lossy().to_string();
    if rel.starts_with('/') {
        return rel.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// `id` attributes → their order of appearance (for anchors).
pub fn ids(toks: &[Tok]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for (i, t) in toks.iter().enumerate() {
        if let Tok::Open { attrs, .. } = t {
            if let Some(id) = attr(attrs, "id") {
                m.entry(id.to_string()).or_insert(i);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgiving() {
        let t = tokens("<p class=\"a\">Olá&nbsp;&mdash; <i>mundo</i> &bogus; 3 < 4<br><epub:x/></p>");
        assert!(matches!(&t[0], Tok::Open { name, .. } if name == "p"));
        assert_eq!(t[1], Tok::Text("Olá\u{a0}— ".into()));
        assert!(t.iter().any(|x| matches!(x, Tok::Text(s) if s.contains("&bogus;"))));
        assert!(t.iter().any(|x| matches!(x, Tok::Open { name, empty: true, .. } if name == "br")));
        assert!(t.iter().any(|x| matches!(x, Tok::Open { name, empty: true, .. } if name == "x")));
        let n = tree("<package><metadata><dc:title>Livro</dc:title></metadata><spine><itemref idref=\"c1\"/></spine></package>");
        assert_eq!(n.first("title").unwrap().all_text(), "Livro");
        assert_eq!(n.first("itemref").unwrap().attr("idref"), Some("c1"));
        assert_eq!(join("OEBPS/text/c1.xhtml", "../images/a%20b.jpg#x"), "OEBPS/images/a b.jpg");
    }
}
