//! What a community theme (or a CSS snippet) sets that a native interface
//! can use: the colour variables of Obsidian's palette
//! (`--background-primary`, `--text-normal`, `--interactive-accent`…) for
//! the light and the dark mode, with `var()`, `calc()`, `rgb()`/`hsl()` and
//! the accent's `--accent-h/s/l` worked out; the typography and spacing of
//! the notes (`--font-text-theme`, `--h1-size`, `--line-height-normal`,
//! `--file-line-width`…); and the fonts a theme embeds with `@font-face`.

use std::collections::HashMap;

pub type Vars = HashMap<String, String>;

/// The variables of a style sheet: (light, dark). Rules on `body`, `:root`,
/// `.theme-light` and `.theme-dark` count; later ones win.
pub fn variables(css: &str) -> (Vars, Vars) {
    let css = strip_comments(css);
    let (mut base, mut light, mut dark) = (Vars::new(), Vars::new(), Vars::new());
    // rules on the body with more classes (a theme's variants, a platform):
    // the first one of each mode, under the plain rules
    let (mut light2, mut dark2) = (Vars::new(), Vars::new());
    for (sel, body) in rules(&css) {
        let mut targets = [false; 5];
        for part in split_commas(&sel) {
            if let Some(m) = context(part) {
                targets[m as usize] = true;
            }
        }
        if !targets.iter().any(|t| *t) {
            continue;
        }
        for (k, v) in declarations(body) {
            if targets[0] {
                base.insert(k.clone(), v.clone());
            }
            if targets[1] {
                light.insert(k.clone(), v.clone());
            }
            if targets[2] {
                dark.insert(k.clone(), v.clone());
            }
            if targets[3] {
                light2.entry(k.clone()).or_insert_with(|| v.clone());
            }
            if targets[4] {
                dark2.entry(k).or_insert(v);
            }
        }
    }
    let mut l = light2;
    l.extend(base.clone());
    l.extend(light);
    let mut d = dark2;
    d.extend(base);
    d.extend(dark);
    (l, d)
}

/// Commas outside parentheses.
fn split_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut d, mut start) = (0, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' => d += 1,
            ')' => d -= 1,
            ',' if d == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// 0 every mode, 1 light, 2 dark; 3 and 4 the same on a body with more
/// classes; None for anything inside the page.
fn context(sel: &str) -> Option<u8> {
    let mut s = sel.trim().to_string();
    for w in ["html", ":root", "body"] {
        s = s.replace(w, " ");
    }
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.as_str() {
        "" => return Some(0),
        ".theme-light" | ".theme-light.theme-light" => return Some(1),
        ".theme-dark" | ".theme-dark.theme-dark" => return Some(2),
        _ => {}
    }
    // one compound selector on the body: classes, :not(), :is()
    if s.contains(' ') || s.contains('>') || s.contains('+') || s.contains('~') || s.contains('[') {
        return None;
    }
    let mode = if s.contains(".theme-light") { 3 } else if s.contains(".theme-dark") { 4 } else { return None };
    // what holds for the desktop
    let mut plain = String::new();
    let mut depth = 0;
    let mut neg = false;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if depth == 0 && s[i..].starts_with(":not(") {
            neg = true;
        }
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    neg = false;
                }
            }
            _ if !neg => plain.push(b[i] as char),
            _ => {}
        }
        i += 1;
    }
    if ["is-mobile", "is-phone", "is-tablet", "is-android", "is-ios", "print"].iter().any(|c| plain.contains(c)) {
        return None;
    }
    Some(mode)
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Top-level and nested rules as (selector, declarations); at-rules other
/// than media queries are skipped.
fn rules(css: &str) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    let b = css.as_bytes();
    let mut i = 0;
    let mut start = 0;
    while i < b.len() {
        match b[i] {
            b'{' => {
                let sel = css[start..i].trim().to_string();
                // the matching brace
                let mut depth = 1;
                let mut j = i + 1;
                while j < b.len() && depth > 0 {
                    match b[j] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                let body = &css[i + 1..j.saturating_sub(1).max(i + 1)];
                if sel.starts_with('@') {
                    if sel.starts_with("@media") || sel.starts_with("@supports") || sel.starts_with("@layer") {
                        // the dark mode by preference is not the app's dark mode: skip media queries on it
                        if !sel.contains("prefers-color-scheme") {
                            out.extend(rules(body));
                        }
                    }
                } else if body.contains('{') {
                    out.extend(rules(body));
                } else {
                    out.push((sel, body));
                }
                i = j;
                start = j;
                continue;
            }
            b'}' | b';' if start <= i => start = i + 1,
            _ => {}
        }
        i += 1;
    }
    out
}

fn declarations(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in body.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c == ';' && depth <= 0 {
            push_decl(&mut out, &cur);
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    push_decl(&mut out, &cur);
    out
}

fn push_decl(out: &mut Vec<(String, String)>, d: &str) {
    if let Some((k, v)) = d.split_once(':') {
        let k = k.trim();
        if k.starts_with("--") {
            out.push((k.to_string(), v.trim().trim_end_matches("!important").trim().to_string()));
        }
    }
}

/// A value with its `var()`s replaced (fallbacks used when missing).
pub fn resolve(vars: &Vars, v: &str, depth: usize) -> Option<String> {
    if depth > 24 {
        return None;
    }
    let Some(i) = v.find("var(") else { return Some(v.to_string()) };
    let open = i + 4;
    let mut d = 1;
    let mut j = open;
    let b = v.as_bytes();
    while j < b.len() && d > 0 {
        match b[j] {
            b'(' => d += 1,
            b')' => d -= 1,
            _ => {}
        }
        j += 1;
    }
    if d != 0 {
        return None;
    }
    let inner = &v[open..j - 1];
    let (name, fallback) = match split_top(inner) {
        Some((n, f)) => (n.trim(), Some(f.trim())),
        None => (inner.trim(), None),
    };
    let got = match vars.get(name) {
        Some(x) => resolve(vars, x, depth + 1),
        None => None,
    };
    let rep = match (got, fallback) {
        (Some(g), _) => g,
        (None, Some(f)) => resolve(vars, f, depth + 1)?,
        (None, None) => return None,
    };
    resolve(vars, &format!("{}{}{}", &v[..i], rep, &v[j..]), depth + 1)
}

/// "a, b" at the top level (not inside parentheses).
fn split_top(s: &str) -> Option<(&str, &str)> {
    let mut d = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => d += 1,
            ')' => d -= 1,
            ',' if d == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

/// A tiny calc(): + - * / and parentheses over numbers, units dropped.
fn calc(s: &str) -> Option<f64> {
    let toks: Vec<char> = s.chars().collect();
    let mut p = 0;
    fn expr(t: &[char], p: &mut usize) -> Option<f64> {
        let mut v = term(t, p)?;
        loop {
            skip(t, p);
            match t.get(*p) {
                Some('+') => {
                    *p += 1;
                    v += term(t, p)?;
                }
                Some('-') => {
                    *p += 1;
                    v -= term(t, p)?;
                }
                _ => return Some(v),
            }
        }
    }
    fn term(t: &[char], p: &mut usize) -> Option<f64> {
        let mut v = atom(t, p)?;
        loop {
            skip(t, p);
            match t.get(*p) {
                Some('*') => {
                    *p += 1;
                    v *= atom(t, p)?;
                }
                Some('/') => {
                    *p += 1;
                    let d = atom(t, p)?;
                    v = if d == 0.0 { 0.0 } else { v / d };
                }
                _ => return Some(v),
            }
        }
    }
    fn atom(t: &[char], p: &mut usize) -> Option<f64> {
        skip(t, p);
        if t.get(*p) == Some(&'(') {
            *p += 1;
            let v = expr(t, p)?;
            skip(t, p);
            if t.get(*p) == Some(&')') {
                *p += 1;
            }
            return Some(v);
        }
        if t[*p..].starts_with(&['c', 'a', 'l', 'c', '(']) {
            *p += 4;
            return atom(t, p);
        }
        let start = *p;
        if matches!(t.get(*p), Some('-') | Some('+')) {
            *p += 1;
        }
        while t.get(*p).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
            *p += 1;
        }
        let n: String = t[start..*p].iter().collect();
        while t.get(*p).is_some_and(|c| c.is_ascii_alphabetic() || *c == '%') {
            *p += 1;
        }
        n.parse().ok()
    }
    fn skip(t: &[char], p: &mut usize) {
        while t.get(*p).is_some_and(|c| c.is_whitespace()) {
            *p += 1;
        }
    }
    let v = expr(&toks, &mut p)?;
    Some(v)
}

/// The numbers of a colour function's arguments (commas, spaces or "/").
fn args(s: &str) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    let mut d = 0;
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<f64>| -> Option<()> {
        let t = cur.trim();
        if !t.is_empty() {
            out.push(calc(t)?);
        }
        cur.clear();
        Some(())
    };
    for c in s.chars() {
        match c {
            '(' => {
                d += 1;
                cur.push(c);
            }
            ')' => {
                d -= 1;
                cur.push(c);
            }
            ',' | '/' if d == 0 => flush(&mut cur, &mut out)?,
            c if c.is_whitespace() && d == 0 => flush(&mut cur, &mut out)?,
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut out)?;
    Some(out)
}

fn hsl(h: f64, s: f64, l: f64) -> [u8; 3] {
    let (h, s, l) = (h.rem_euclid(360.0) / 360.0, (s / 100.0).clamp(0.0, 1.0), (l / 100.0).clamp(0.0, 1.0));
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |t: f64| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

/// A colour value (after `resolve`) as RGBA.
pub fn color(v: &str) -> Option<[u8; 4]> {
    let v = v.trim().to_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let h: String = if h.len() == 3 || h.len() == 4 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
        let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        return match h.len() {
            6 => Some([p(0)?, p(2)?, p(4)?, 255]),
            8 => Some([p(0)?, p(2)?, p(4)?, p(6)?]),
            _ => None,
        };
    }
    let named = match v.as_str() {
        "white" => Some([255, 255, 255, 255]),
        "black" => Some([0, 0, 0, 255]),
        "transparent" => Some([0, 0, 0, 0]),
        "red" => Some([255, 0, 0, 255]),
        "gray" | "grey" => Some([128, 128, 128, 255]),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let (name, rest) = v.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    let a = args(inner)?;
    let alpha = |i: usize| a.get(i).map(|x| if *x > 1.0 { x / 100.0 } else { *x }).unwrap_or(1.0);
    match name.trim() {
        "rgb" | "rgba" if a.len() >= 3 => Some([a[0].round().clamp(0.0, 255.0) as u8, a[1].round().clamp(0.0, 255.0) as u8, a[2].round().clamp(0.0, 255.0) as u8, (alpha(3) * 255.0).round().clamp(0.0, 255.0) as u8]),
        "hsl" | "hsla" if a.len() >= 3 => {
            let c = hsl(a[0], a[1], a[2]);
            Some([c[0], c[1], c[2], (alpha(3) * 255.0).round().clamp(0.0, 255.0) as u8])
        }
        _ => None,
    }
}

/// A variable of the theme as a colour.
pub fn var_color(vars: &Vars, name: &str) -> Option<[u8; 4]> {
    let v = vars.get(name)?;
    color(&resolve(vars, v, 0)?)
}

/// The variables Obsidian itself provides from the accent colour.
pub fn accent_vars(rgb: [u8; 3]) -> Vars {
    let (r, g, b) = (rgb[0] as f64 / 255.0, rgb[1] as f64 / 255.0, rgb[2] as f64 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let mut v = Vars::new();
    v.insert("--accent-h".into(), format!("{h:.1}"));
    v.insert("--accent-s".into(), format!("{:.1}%", s * 100.0));
    v.insert("--accent-l".into(), format!("{:.1}%", l * 100.0));
    v.insert("--color-accent".into(), "hsl(var(--accent-h), var(--accent-s), var(--accent-l))".into());
    v.insert("--interactive-accent".into(), "var(--color-accent)".into());
    v
}

// ── typography ────────────────────────────────────────────────────────────

/// What a theme says of the notes' type and spacing. Sizes are in pixels
/// at the note's base size; `None` where the theme says nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Typography {
    /// Font families in order of preference (the CSS list).
    pub text_font: Vec<String>,
    pub mono_font: Vec<String>,
    pub interface_font: Vec<String>,
    pub line_height: Option<f32>,
    /// Heading sizes as multiples of the base size, weights, colours.
    pub heading_size: [Option<f32>; 6],
    pub heading_weight: [Option<u16>; 6],
    pub heading_color: [Option<[u8; 4]>; 6],
    /// The readable line width (px).
    pub line_width: Option<f32>,
    /// Space between paragraphs, in multiples of the base size.
    pub paragraph_spacing: Option<f32>,
    pub bold_color: Option<[u8; 4]>,
    pub italic_color: Option<[u8; 4]>,
    pub link_color: Option<[u8; 4]>,
    pub quote_border: Option<[u8; 4]>,
    pub quote_border_width: Option<f32>,
    pub bullet_color: Option<[u8; 4]>,
    pub highlight: Option<[u8; 4]>,
    /// Inline and block code size, as a multiple of the base size.
    pub code_size: Option<f32>,
}

/// A length in pixels: px, em/rem (of `base`), %, or a bare number of `per`.
pub fn length(v: &str, base: f32, per: f32) -> Option<f32> {
    let v = v.trim();
    let num = |s: &str| s.trim().parse::<f32>().ok();
    if let Some(x) = v.strip_suffix("px") {
        return num(x);
    }
    if let Some(x) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
        return num(x).map(|n| n * base);
    }
    if let Some(x) = v.strip_suffix('%') {
        return num(x).map(|n| n / 100.0 * base);
    }
    if let Some(x) = v.strip_suffix("pt") {
        return num(x).map(|n| n * 4.0 / 3.0);
    }
    num(v).map(|n| n * per)
}

/// A CSS font list: the families, without quotes and generic names.
pub fn font_list(v: &str) -> Vec<String> {
    split_commas(v)
        .into_iter()
        .map(|f| f.trim().trim_matches(['"', '\'']).trim().to_string())
        .filter(|f| !f.is_empty() && !f.starts_with("var(") && !["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "ui-sans-serif", "ui-serif", "ui-monospace", "inherit", "initial", "-apple-system", "BlinkMacSystemFont", "??"].contains(&f.as_str()))
        .collect()
}

fn weight(v: &str) -> Option<u16> {
    match v.trim() {
        "normal" => Some(400),
        "bold" => Some(700),
        "bolder" => Some(800),
        "lighter" => Some(300),
        x => x.parse().ok(),
    }
}

/// The typography a theme sets, for a base text size (px).
pub fn typography(vars: &Vars, base: f32) -> Typography {
    let get = |k: &str| vars.get(k).and_then(|v| resolve(vars, v, 0)).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let col = |k: &str| var_color(vars, k);
    let fonts = |keys: &[&str]| keys.iter().find_map(|k| get(k).map(|v| font_list(&v)).filter(|l| !l.is_empty())).unwrap_or_default();
    let mut t = Typography {
        text_font: fonts(&["--font-text-theme", "--font-text-override", "--font-text"]),
        mono_font: fonts(&["--font-monospace-theme", "--font-monospace-override", "--font-monospace"]),
        interface_font: fonts(&["--font-interface-theme", "--font-interface-override", "--font-interface"]),
        ..Default::default()
    };
    t.line_height = get("--line-height-normal").and_then(|v| if v.ends_with("px") || v.ends_with("em") { length(&v, base, 1.0).map(|px| px / base) } else { v.parse().ok() }).filter(|x| (1.0..=3.0).contains(x));
    for i in 0..6 {
        let n = i + 1;
        t.heading_size[i] = get(&format!("--h{n}-size")).and_then(|v| length(&v, base, base)).map(|px| px / base).filter(|x| (0.6..=4.0).contains(x));
        t.heading_weight[i] = get(&format!("--h{n}-weight")).and_then(|v| weight(&v));
        t.heading_color[i] = col(&format!("--h{n}-color"));
    }
    t.line_width = get("--file-line-width").and_then(|v| length(&v, base, 1.0)).filter(|x| (300.0..=3000.0).contains(x));
    t.paragraph_spacing = get("--p-spacing").and_then(|v| length(&v, base, base)).map(|px| px / base).filter(|x| (0.0..=4.0).contains(x));
    t.bold_color = col("--bold-color");
    t.italic_color = col("--italic-color");
    t.link_color = col("--link-color");
    t.quote_border = col("--blockquote-border-color");
    t.quote_border_width = get("--blockquote-border-thickness").and_then(|v| length(&v, base, 1.0)).filter(|x| (0.0..=12.0).contains(x));
    t.bullet_color = col("--list-marker-color");
    t.highlight = col("--text-highlight-bg");
    t.code_size = get("--code-size").and_then(|v| length(&v, base, base)).map(|px| px / base).filter(|x| (0.5..=1.5).contains(x));
    t
}

/// A font a theme carries inside its CSS (`@font-face` with a `data:` URL).
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedFont {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub data: Vec<u8>,
}

/// The fonts embedded in a style sheet that can be used as they are
/// (TrueType and OpenType; WOFF ones are skipped).
pub fn embedded_fonts(css: &str) -> Vec<EmbeddedFont> {
    use base64::Engine as _;
    let mut out = Vec::new();
    let mut rest = css;
    while let Some(i) = rest.find("@font-face") {
        rest = &rest[i + 10..];
        let Some(open) = rest.find('{') else { break };
        let Some(close) = rest[open..].find('}') else { break };
        let body = &rest[open + 1..open + close];
        rest = &rest[open + close..];
        let prop = |name: &str| -> Option<&str> {
            let k = body.find(name)?;
            let after = body[k + name.len()..].trim_start().strip_prefix(':')?;
            Some(after.split(';').next().unwrap_or(after).trim())
        };
        let Some(family) = prop("font-family").map(|f| f.trim_matches(['"', '\'']).to_string()) else { continue };
        let weight = prop("font-weight").and_then(|w| weight(w.split_whitespace().next().unwrap_or(w))).unwrap_or(400);
        let italic = prop("font-style").is_some_and(|s| s.contains("italic") || s.contains("oblique"));
        // the src's data URLs, the first usable one
        let Some(src_at) = body.find("src") else { continue };
        let mut src = &body[src_at..];
        while let Some(d) = src.find("data:") {
            src = &src[d..];
            let end = src.find([')', '"', '\'']).unwrap_or(src.len());
            let url = &src[..end];
            src = &src[end..];
            let Some((_, b64)) = url.split_once("base64,") else { continue };
            let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
            let Ok(data) = base64::engine::general_purpose::STANDARD.decode(clean.as_bytes()) else { continue };
            // TrueType, OpenType (CFF) or a collection; not WOFF
            if data.len() > 12 && (data.starts_with(&[0, 1, 0, 0]) || data.starts_with(b"OTTO") || data.starts_with(b"true") || data.starts_with(b"ttcf")) {
                out.push(EmbeddedFont { family: family.clone(), weight, italic, data });
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_and_colours() {
        let css = r#"
            /* a comment { with braces } */
            body { --ctp-base: 30, 30, 46; --base-l: 15%; }
            .theme-light { --background-primary: #fafafa; --text-normal: rgb(20 20 20 / 1); }
            .theme-dark { --background-primary: rgb(var(--ctp-base)); --text-normal: hsl(210, calc(20% - 5%), calc(var(--base-l) + 70%)); }
            .theme-dark .markdown-preview-view { --background-primary: red; }
            @media (max-width: 400px) { body { --x: 1px; } }
            body.theme-dark { --text-muted: var(--missing, #888); }
        "#;
        let (l, d) = variables(css);
        assert_eq!(var_color(&l, "--background-primary"), Some([0xfa, 0xfa, 0xfa, 255]));
        assert_eq!(var_color(&d, "--background-primary"), Some([30, 30, 46, 255]));
        assert_eq!(var_color(&l, "--text-normal"), Some([20, 20, 20, 255]));
        let t = var_color(&d, "--text-normal").unwrap();
        assert!(t[0] > 200 && t[2] > t[0]);
        assert_eq!(var_color(&d, "--text-muted"), Some([0x88, 0x88, 0x88, 255]));
        assert_eq!(color("rgba(0, 0, 0, 0.5)"), Some([0, 0, 0, 128]));
        let a = accent_vars([0x76, 0xce, 0xf4]);
        let c = var_color(&a, "--interactive-accent").unwrap();
        assert!((c[0] as i32 - 0x76).abs() <= 2 && (c[2] as i32 - 0xf4).abs() <= 2);
    }

    #[test]
    fn installed_themes() {
        let Ok(dir) = std::env::var("THEMES_DIR") else { return };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let css = std::fs::read_to_string(e.path().join("theme.css")).unwrap_or_default();
            let (l, d) = variables(&css);
            for (mode, v) in [("light", l), ("dark", d)] {
                let mut v2 = accent_vars([0x76, 0xce, 0xf4]);
                v2.extend(v);
                let keys = ["--background-primary", "--background-secondary", "--text-normal", "--text-muted", "--interactive-accent", "--background-modifier-border"];
                let got: Vec<String> = keys.iter().map(|k| var_color(&v2, k).map(|c| format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])).unwrap_or("-".into())).collect();
                eprintln!("{:12} {mode:5} {}", e.file_name().to_string_lossy(), got.join(" "));
            }
        }
    }

    #[test]
    fn typography_of_a_theme() {
        let css = ":root { --font-text-theme: 'Literata', Georgia, serif; --h1-size: 2em; --h2-size: 1.5rem; --h3-size: 22px; --h1-weight: 400; --h2-color: #ff0000; --line-height-normal: 1.7; --file-line-width: 45rem; --p-spacing: 1.5rem; --bold-color: var(--h2-color); --code-size: 0.875em; --blockquote-border-thickness: 3px; }";
        let (l, _) = variables(css);
        let t = typography(&l, 16.0);
        assert_eq!(t.text_font, ["Literata", "Georgia"]);
        assert_eq!(t.heading_size[0], Some(2.0));
        assert_eq!(t.heading_size[1], Some(1.5));
        assert_eq!(t.heading_size[2], Some(22.0 / 16.0));
        assert_eq!(t.heading_weight[0], Some(400));
        assert_eq!(t.heading_color[1].map(|c| c[0]), Some(255));
        assert_eq!(t.line_height, Some(1.7));
        assert_eq!(t.line_width, Some(720.0));
        assert_eq!(t.paragraph_spacing, Some(1.5));
        assert_eq!(t.bold_color.map(|c| c[0]), Some(255), "through var()");
        assert_eq!(t.code_size, Some(0.875));
        assert_eq!(t.quote_border_width, Some(3.0));
        assert_eq!(typography(&Vars::new(), 16.0), Typography::default());
    }

    #[test]
    fn fonts_embedded_in_css() {
        use base64::Engine as _;
        let ttf = [0u8, 1, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 7, 7];
        let woff = *b"wOFF0000000000";
        let b = |d: &[u8]| base64::engine::general_purpose::STANDARD.encode(d);
        let css = format!("@font-face {{ font-family: \"Nice\"; font-weight: 700; font-style: italic; src: url(data:font/woff;base64,{}) format('woff'), url(\"data:font/ttf;base64,{}\") format('truetype'); }} @font-face {{ font-family: Other; src: url(data:font/woff;base64,{}); }}", b(&woff), b(&ttf), b(&woff));
        let f = embedded_fonts(&css);
        assert_eq!(f.len(), 1, "the WOFF-only face is skipped");
        assert_eq!((f[0].family.as_str(), f[0].weight, f[0].italic), ("Nice", 700, true));
        assert_eq!(f[0].data, ttf);
    }
}
