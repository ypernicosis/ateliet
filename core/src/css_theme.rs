//! What a community theme (or a CSS snippet) sets that a native interface
//! can use: the colour variables of Obsidian's palette
//! (`--background-primary`, `--text-normal`, `--interactive-accent`…) for
//! the light and the dark mode, with `var()`, `calc()`, `rgb()`/`hsl()` and
//! the accent's `--accent-h/s/l` worked out.

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
}
