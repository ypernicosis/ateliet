//! Character sheets: attributes in the frontmatter, sections as `## Título`.
//! Everything stays plain Markdown, as in the plugin.

use serde_json::{Map, Value as Json};

/// Keys with a meaning of their own (not shown as attributes).
pub const RESERVED: [&str; 15] = ["scriptorium", "livro", "livros", "retrato", "galeria", "aliados", "inimigos", "frase", "cor", "playlist", "papel", "tags", "aliases", "cssclasses", "position"];

pub struct Sheet {
    pub fm: Map<String, Json>,
    /// YAML that did not parse (kept as it was).
    pub invalid: Option<String>,
    /// Text before the first `##`.
    pub pre: String,
    pub sections: Vec<(String, String)>,
}

pub fn split(text: &str) -> (Option<Map<String, Json>>, Option<String>, String) {
    let body = super::strip_frontmatter(text);
    if body.len() == text.len() {
        return (None, None, text.to_string());
    }
    let head = &text[..text.len() - body.len()];
    let yaml: String = head.trim_start_matches('\u{feff}').lines().skip(1).take_while(|l| l.trim_end() != "---").collect::<Vec<_>>().join("\n");
    match serde_yaml::from_str::<Json>(&yaml) {
        Ok(Json::Object(m)) => (Some(m), None, body.to_string()),
        Ok(Json::Null) => (Some(Map::new()), None, body.to_string()),
        _ => (None, Some(yaml), body.to_string()),
    }
}

impl Sheet {
    pub fn parse(text: &str) -> Sheet {
        let (fm, invalid, body) = split(text);
        let mut pre = Vec::new();
        let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
        for l in body.split('\n') {
            if let Some(t) = l.strip_prefix("## ") {
                sections.push((t.trim().to_string(), Vec::new()));
                continue;
            }
            match sections.last_mut() {
                Some(s) => s.1.push(l),
                None => pre.push(l),
            }
        }
        Sheet { fm: fm.unwrap_or_default(), invalid, pre: pre.join("\n"), sections: sections.into_iter().map(|(t, ls)| (t, ls.join("\n").trim().to_string())).collect() }
    }

    pub fn to_text(&self) -> String {
        let head = match &self.invalid {
            Some(y) => format!("---\n{y}\n---\n"),
            None => {
                let y = if self.fm.is_empty() { String::new() } else { serde_yaml::to_string(&Json::Object(self.fm.clone())).unwrap_or_default() };
                format!("---\n{y}---\n")
            }
        };
        let mut out = self.pre.trim_end().to_string();
        out = if out.is_empty() { "\n".into() } else { format!("{out}\n\n") };
        for (t, body) in &self.sections {
            out.push_str(&format!("## {t}\n\n{}\n\n", body.trim()));
        }
        format!("{head}{out}")
    }

    pub fn str(&self, k: &str) -> String {
        match self.fm.get(k) {
            Some(Json::String(s)) => s.clone(),
            Some(Json::Null) | None => String::new(),
            Some(Json::Array(a)) => a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(", "),
            Some(other) => other.to_string(),
        }
    }

    /// The lists of people (aliados, inimigos and the ones the author made).
    pub fn lists(&self) -> Vec<String> {
        match self.fm.get("listas") {
            Some(Json::Array(a)) => a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect(),
            _ => vec!["aliados".into(), "inimigos".into()],
        }
    }

    pub fn set_lists(&mut self, lists: Vec<String>) {
        if lists == ["aliados", "inimigos"] {
            self.fm.shift_remove("listas");
        } else {
            self.fm.insert("listas".into(), Json::Array(lists.into_iter().map(Json::String).collect()));
        }
    }

    pub fn reserved(&self, k: &str) -> bool {
        RESERVED.contains(&k) || ["listas", "vinculos", "modelo", "modelo_sobre", "caixas"].contains(&k) || self.lists().iter().any(|l| l == k) || k.starts_with("__")
    }

    pub fn attr_keys(&self) -> Vec<String> {
        self.fm.keys().filter(|k| !self.reserved(k)).cloned().collect()
    }

    pub fn list(&self, k: &str) -> Vec<Json> {
        super::to_list(self.fm.get(k).unwrap_or(&Json::Null))
    }

    /// Reorder the attributes (reserved keys first, as they were).
    pub fn reorder_attrs(&mut self, order: &[String]) {
        let mut out = Map::new();
        for (k, v) in &self.fm {
            if self.reserved(k) {
                out.insert(k.clone(), v.clone());
            }
        }
        for k in order {
            out.insert(k.clone(), self.fm.get(k).cloned().unwrap_or(Json::String(String::new())));
        }
        for (k, v) in &self.fm {
            if !out.contains_key(k) {
                out.insert(k.clone(), v.clone());
            }
        }
        self.fm = out;
    }

    pub fn rename_key(&mut self, from: &str, to: &str) {
        let mut out = Map::new();
        for (k, v) in &self.fm {
            out.insert(if k == from { to.to_string() } else { k.clone() }, v.clone());
        }
        self.fm = out;
    }

    /// The size the author gave a section's box (`caixas`): its height in
    /// points (`None`: as tall as its text) and its width ("inteira",
    /// "meia" or "" for the default).
    pub fn box_size(&self, title: &str) -> (Option<f32>, String) {
        let b = self.fm.get("caixas").and_then(|c| c.get(title));
        let h = b.and_then(|b| b.get("altura")).and_then(|v| v.as_f64()).map(|v| v as f32);
        let w = b.and_then(|b| b.get("largura")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        (h, w)
    }

    pub fn set_box_size(&mut self, title: &str, height: Option<f32>, width: &str) {
        let mut m = self.fm.get("caixas").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        let mut b = Map::new();
        if let Some(h) = height {
            b.insert("altura".into(), Json::from(h.round() as i64));
        }
        if !width.is_empty() {
            b.insert("largura".into(), Json::String(width.into()));
        }
        if b.is_empty() {
            m.shift_remove(title);
        } else {
            m.insert(title.into(), Json::Object(b));
        }
        if m.is_empty() {
            self.fm.shift_remove("caixas");
        } else {
            self.fm.insert("caixas".into(), Json::Object(m));
        }
    }

    /// Bring a sheet up to a model: the model's attributes, lists and
    /// sections the sheet lacks are added (empty attributes, the model's
    /// text in sections, the model's box sizes); nothing the sheet has is
    /// changed or removed. Sections follow the model's order, the sheet's
    /// own ones after. True when something was added.
    pub fn apply_model(&mut self, model: &Sheet) -> bool {
        let before = self.to_text();
        let skip = ["scriptorium", "livro", "livros", "modelo", "modelo_sobre", "listas", "caixas", "vinculos"];
        let model_lists = model.lists();
        let mut lists = self.lists();
        for l in &model_lists {
            if !lists.contains(l) {
                lists.push(l.clone());
            }
        }
        for (k, v) in &model.fm {
            if skip.contains(&k.as_str()) || self.fm.contains_key(k) {
                continue;
            }
            let empty = match v {
                Json::Array(_) => Json::Array(vec![]),
                _ if model_lists.contains(k) => Json::Array(vec![]),
                _ => Json::String(String::new()),
            };
            self.fm.insert(k.clone(), empty);
        }
        for l in &lists {
            if !self.fm.contains_key(l) {
                self.fm.insert(l.clone(), Json::Array(vec![]));
            }
        }
        self.set_lists(lists);
        let key = |t: &str| t.trim().to_lowercase();
        let mut mine = std::mem::take(&mut self.sections);
        let mut out = Vec::new();
        for (t, body) in &model.sections {
            match mine.iter().position(|(m, _)| key(m) == key(t)) {
                Some(i) => out.push(mine.remove(i)),
                None => out.push((t.clone(), body.clone())),
            }
        }
        out.extend(mine);
        self.sections = out;
        let titles: Vec<String> = self.sections.iter().map(|s| s.0.clone()).collect();
        for t in titles {
            let (h, w) = self.box_size(&t);
            if h.is_none() && w.is_empty() {
                if let Some((_, (mh, mw))) = model.sections.iter().find(|(m, _)| key(m) == key(&t)).map(|(m, _)| (m, model.box_size(m))) {
                    self.set_box_size(&t, mh, &mw);
                }
            }
        }
        self.to_text() != before
    }

    /// The note people wrote about someone in a list (`vinculos`).
    pub fn bond(&self, name: &str) -> String {
        self.fm.get("vinculos").and_then(|v| v.get(name)).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }

    pub fn set_bond(&mut self, name: &str, note: &str) {
        let mut m = self.fm.get("vinculos").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        if note.trim().is_empty() {
            m.shift_remove(name);
        } else {
            m.insert(name.to_string(), Json::String(note.to_string()));
        }
        if m.is_empty() {
            self.fm.shift_remove("vinculos");
        } else {
            self.fm.insert("vinculos".into(), Json::Object(m));
        }
    }
}

const LABELS: [(&str, &str); 19] = [("nome_completo", "nome completo"), ("filiacao", "clã / filiação"), ("nivel_ars", "nível do Ars"), ("estilo_luta", "estilo de luta"), ("simbolo", "aura / símbolo"), ("cor_aura", "cor da aura"), ("ars", "Ars"), ("vortice", "vórtice"), ("inspiracao", "inspiração"), ("genero", "gênero"), ("pecado", "pecado"), ("iga", "nº IGA"), ("orientacao", "orientação"), ("descricao", "descrição"), ("ocupacao", "ocupação"), ("origem", "origem"), ("nacao", "nação"), ("religiao", "religião"), ("familia", "família")];

pub fn attr_label(k: &str) -> String {
    LABELS.iter().find(|l| l.0 == k).map(|l| l.1.to_string()).unwrap_or_else(|| k.replace('_', " "))
}

/// The Lucide icon name of a section, from its title.
pub fn section_icon(title: &str) -> &'static str {
    let t = title.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["físic", "fisic", "aparên", "aparen", "visual"]) {
        "scan-face"
    } else if has(&["personal", "temper", "psico"]) {
        "heart"
    } else if has(&["arco", "jornada", "evolu"]) {
        "route"
    } else if has(&["nota"]) {
        "sticky-note"
    } else if has(&["hist", "passado", "origem", "backstory"]) {
        "scroll"
    } else if has(&["poder", "habilid", "magia", "dom"]) {
        "sparkles"
    } else if has(&["rela", "víncul", "vincul", "famíl", "famil"]) {
        "users"
    } else if has(&["segredo", "medo", "fobia", "fraquez", "restri", "custo"]) {
        "lock"
    } else if has(&["fala", "voz", "bordão", "bordao"]) {
        "message-square-quote"
    } else if has(&["objetiv", "desejo", "motiva", "sonho", "ambi"]) {
        "target"
    } else if t == "ars" || has(&["ars vortices", "vórtice", "vortice"]) {
        "sparkles"
    } else {
        "align-left"
    }
}

/// "[[pasta/Ana|Ana]]" → "Ana"
pub fn rel_name(v: &Json) -> String {
    let s = match v {
        Json::String(s) => s.clone(),
        other => super::link_text(other),
    };
    let s = s.trim().trim_start_matches("[[").trim_end_matches("]]");
    let s = s.rsplit('|').next().unwrap_or(s);
    s.rsplit('/').next().unwrap_or(s).to_string()
}

/// Two initials for an avatar.
pub fn initials(name: &str) -> String {
    let out: String = name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphabetic())).take(2).flat_map(char::to_uppercase).collect();
    if out.is_empty() { "?".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let t = "---\nscriptorium: personagem\npapel: protagonista\nidade: 17\nvortice: fogo\naliados:\n  - \"[[Bia]]\"\n---\n\n## Personalidade\n\nTeimosa.\n\n## Notas\n\n";
        let mut s = Sheet::parse(t);
        assert_eq!(s.attr_keys(), vec!["idade", "vortice"]);
        assert_eq!(s.sections[0], ("Personalidade".to_string(), "Teimosa.".to_string()));
        assert_eq!(rel_name(&s.list("aliados")[0]), "Bia");
        s.reorder_attrs(&["vortice".into(), "idade".into()]);
        s.set_bond("Bia", "melhor amiga");
        let out = s.to_text();
        let back = Sheet::parse(&out);
        assert_eq!(back.attr_keys(), vec!["vortice", "idade"]);
        assert_eq!(back.bond("Bia"), "melhor amiga");
        assert!(out.contains("## Personalidade\n\nTeimosa.\n\n## Notas"));
        assert_eq!(attr_label("vortice"), "vórtice");
        assert_eq!(initials("Ana Clara"), "AC");
    }

    #[test]
    fn model_and_boxes() {
        let mut s = Sheet::parse("---\nscriptorium: personagem\nidade: 15\n---\n\n## Personalidade\n\nCarente.\n\n## Segredos\n\nX\n");
        let mut m = Sheet::parse("---\nscriptorium: modelo-ficha\nidade: ''\netnia: ''\nfamilia: []\nlistas: [familia]\n---\n\n## Aparência\n\n## Personalidade\n\nTrês adjetivos.\n");
        m.set_box_size("Aparência", Some(180.0), "meia");
        assert!(s.apply_model(&m));
        assert_eq!(s.str("idade"), "15");
        assert_eq!(s.attr_keys(), vec!["idade", "etnia"]);
        assert_eq!(s.lists(), vec!["aliados", "inimigos", "familia"]);
        let t: Vec<&str> = s.sections.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(t, vec!["Aparência", "Personalidade", "Segredos"]);
        assert_eq!(s.sections[1].1, "Carente.");
        assert_eq!(s.box_size("Aparência"), (Some(180.0), "meia".to_string()));
        assert!(!s.apply_model(&m), "applying twice changes nothing");
        s.set_box_size("Aparência", None, "");
        assert!(!s.fm.contains_key("caixas"));
    }
}
