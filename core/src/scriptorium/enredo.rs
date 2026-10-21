//! Enredo: the plot of a book, kept in `<book>/Enredo/Enredo.md` as a JSON
//! block. The unit is the scene; tramas (plot lines) cross chapters. The
//! data stays a `Json` value so fields this code does not touch (structures,
//! the mural, relations) are written back untouched.

use super::book::{self, Book};
use super::{read_json_block, write_json_block};
use crate::dataview::index::Index;
use serde_json::{json, Value as Json};
use std::path::Path;

pub const LANG: &str = "scriptorium-enredo";

pub const TRAMA_CORES: [&str; 10] = ["#7a2e3b", "#2f6b6b", "#4b5bb3", "#b07a36", "#5d7a3a", "#8b5fbf", "#c0392b", "#3f9f98", "#6b7280", "#a8641b"];

pub const TRAMA_TIPOS: [(&str, &str); 5] = [("principal", "Trama principal"), ("subtrama", "Subtrama"), ("personagem", "Trama de personagem"), ("tema", "Linha temática"), ("misterio", "Mistério")];

pub const PISTA_TIPOS: [(&str, &str); 7] = [("pista", "Pista"), ("misterio", "Mistério"), ("promessa", "Promessa"), ("objeto", "Objeto"), ("segredo", "Segredo"), ("profecia", "Profecia"), ("armadilha", "Arma de Tchekhov")];

pub const ARCO_TIPOS: [(&str, &str); 4] = [("positivo", "Positivo (mentira → verdade)"), ("negativo", "Negativo (verdade → mentira)"), ("plano", "Plano (a verdade muda o mundo)"), ("queda", "Queda (mentira → mentira pior)")];

pub fn uid(prefix: &str) -> String {
    let u = crate::board::doc::uid();
    format!("{prefix}{}", &u[..u.len().min(9)])
}

pub fn path(book: &Book) -> String {
    format!("{}/Enredo/Enredo.md", book.folder)
}

pub fn blank() -> Json {
    json!({ "v": 2, "estrutura": { "ref": "" }, "tramas": [], "cenas": [], "pistas": [], "arcos": [], "eventos": [], "eras": [], "mural": { "cards": [], "links": [], "view": null }, "relacoes": null })
}

pub fn load(root: &Path, book: &Book) -> Json {
    let mut data = blank();
    if let Some(j) = std::fs::read_to_string(root.join(path(book))).ok().and_then(|t| read_json_block(&t, LANG)) {
        if let (Some(d), Json::Object(m)) = (data.as_object_mut(), j) {
            for (k, v) in m {
                d.insert(k, v);
            }
        }
    }
    if data["relacoes"].is_null() {
        // the old board (Enredo/Quadro.md) becomes the "Relações" view
        let old = std::fs::read_to_string(root.join(format!("{}/Enredo/Quadro.md", book.folder))).ok().and_then(|t| read_json_block(&t, "scriptorium-quadro"));
        data["relacoes"] = match old {
            Some(o) if o["cards"].is_array() => {
                let cards: Vec<Json> = o["cards"].as_array().unwrap().iter().filter(|c| !(c["t"] == "nota" && c["texto"].as_str().unwrap_or("").starts_with("Comece aqui"))).cloned().collect();
                json!({ "cards": cards, "links": o.get("links").cloned().unwrap_or(json!([])), "view": null })
            }
            _ => json!({ "cards": [], "links": [], "view": null }),
        };
    }
    migrate_structures(&mut data);
    for key in ["tramas", "cenas", "pistas", "arcos", "eventos", "eras"] {
        if !data[key].is_array() {
            data[key] = json!([]);
        }
    }
    data
}

/// One structure for the whole book → several, each over a stretch; a
/// scene's `batida` becomes `batidas: ["structure:beat"]`.
fn migrate_structures(data: &mut Json) {
    if !data["estruturas"].is_array() {
        let r = data["estrutura"]["ref"].as_str().unwrap_or("").to_string();
        data["estruturas"] = if r.is_empty() { json!([]) } else { json!([{ "id": "e1", "ref": r, "escopo": "livro" }]) };
    }
    let first = data["estruturas"].get(0).and_then(|e| e["id"].as_str()).map(String::from);
    if let Some(cenas) = data["cenas"].as_array_mut() {
        for s in cenas {
            let Some(o) = s.as_object_mut() else { continue };
            if let Some(b) = o.remove("batida") {
                if !o.contains_key("batidas") {
                    let list = match (&first, b.as_str()) {
                        (Some(f), Some(b)) if !b.is_empty() => json!([format!("{f}:{b}")]),
                        _ => json!([]),
                    };
                    o.insert("batidas".into(), list);
                }
            }
        }
    }
}

pub fn save(root: &Path, ix: &Index, book: &Book, data: &Json) -> Result<(), String> {
    let p = path(book);
    let full = root.join(&p);
    let text = match std::fs::read_to_string(&full) {
        Ok(t) => write_json_block(&t, LANG, data),
        Err(_) => {
            let head = format!(
                "---\nscriptorium: enredo\nlivro: \"{}\"\n---\n\nEnredo de **{}**: cenas, tramas, pistas, arcos, cronologia, mural e relações. Abra pela tela Enredo do Scriptorium.\n",
                book::note_link(ix, &book.path, &p),
                book.title
            );
            write_json_block(&head, LANG, data)
        }
    };
    if let Some(dir) = full.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&full, text).map_err(|e| e.to_string())
}

/// A column of the grid: a chapter that exists, or one planned in the outline.
#[derive(Clone, Debug)]
pub struct Col {
    /// The chapter's path, or `plan:<id>` for a planned one.
    pub path: String,
    pub title: String,
    pub label: String,
    pub status: String,
    pub part: String,
    pub tipo: String,
    pub numbered: bool,
    pub include: bool,
    pub planned: Option<Json>,
    /// The plan entry an existing chapter belongs to.
    pub plan: Option<Json>,
}

impl Col {
    pub fn is_file(&self) -> bool {
        self.planned.is_none()
    }
}

fn plan_files(p: &Json) -> Vec<String> {
    match p["arquivos"].as_array() {
        Some(a) if !a.is_empty() => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        _ => p["arquivo"].as_str().filter(|s| !s.is_empty()).map(|s| vec![s.to_string()]).unwrap_or_default(),
    }
}

/// Chapters in book order; planned chapters (from the outline, without a
/// file yet) go after the written chapter with the previous number.
pub fn chapters(ix: &Index, book: &Book, data: &Json) -> Vec<Col> {
    let mut n = 0;
    let mut real: Vec<Col> = book::chapters(ix, book)
        .into_iter()
        .map(|c| {
            if c.numbered && c.include {
                n += 1;
            }
            let label = if c.numbered {
                n.to_string()
            } else {
                match c.tipo.as_str() {
                    "prologo" => "P",
                    "epilogo" => "E",
                    "interludio" => "I",
                    _ => "·",
                }
                .to_string()
            };
            Col { path: c.path, title: c.title, label, status: c.status, part: c.part, tipo: c.tipo, numbered: c.numbered, include: c.include, planned: None, plan: None }
        })
        .collect();
    let mut plan: Vec<Json> = data["planejados"].as_array().cloned().unwrap_or_default();
    if plan.is_empty() {
        return real;
    }
    let num = |p: &Json| p["num"].as_f64();
    plan.sort_by(|a, b| num(a).unwrap_or(1e9).partial_cmp(&num(b).unwrap_or(1e9)).unwrap_or(std::cmp::Ordering::Equal));
    for p in &plan {
        for fp in plan_files(p) {
            if let Some(c) = real.iter_mut().find(|c| c.path == fp) {
                c.plan = Some(p.clone());
                if c.part.is_empty() {
                    c.part = p["parte"].as_str().unwrap_or("").to_string();
                }
            }
        }
    }
    let col_num = |c: &Col| c.plan.as_ref().or(c.planned.as_ref()).and_then(|q| q["num"].as_f64());
    for p in &plan {
        if plan_files(p).iter().any(|fp| real.iter().any(|c| &c.path == fp)) {
            continue;
        }
        let pn = num(p).unwrap_or(1e9);
        let mut at: isize = -1;
        for (k, c) in real.iter().enumerate() {
            if col_num(c).is_some_and(|q| q < pn) {
                at = k as isize;
            }
        }
        if at < 0 {
            at = match real.iter().position(|c| col_num(c).is_some_and(|q| q > pn)) {
                Some(nx) => nx as isize - 1,
                None => real.len() as isize - 1,
            };
        }
        let title = p["titulo"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| match num(p) {
            Some(x) => format!("Capítulo {x}"),
            None => "Capítulo planejado".into(),
        });
        real.insert(
            (at + 1) as usize,
            Col {
                path: format!("plan:{}", p["id"].as_str().unwrap_or("")),
                title,
                label: num(p).map(|x| x.to_string()).unwrap_or_else(|| "·".into()),
                status: "ideia".into(),
                part: p["parte"].as_str().unwrap_or("").to_string(),
                tipo: "capitulo".into(),
                numbered: true,
                include: true,
                planned: Some(p.clone()),
                plan: None,
            },
        );
    }
    real
}

pub fn str_of<'a>(v: &'a Json, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

pub fn cap_of(s: &Json) -> Option<&str> {
    s["cap"].as_str().filter(|c| !c.is_empty())
}

pub fn tramas_of(s: &Json) -> Vec<&str> {
    s["tramas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default()
}

fn ord(s: &Json) -> f64 {
    s["ord"].as_f64().unwrap_or(0.0)
}

/// Indices into `cenas`, in reading order (chapter, then order; without a chapter last).
pub fn ordered(cols: &[Col], data: &Json) -> Vec<usize> {
    let cenas = data["cenas"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let idx = |s: &Json| cap_of(s).and_then(|c| cols.iter().position(|x| x.path == c)).unwrap_or(1_000_000);
    let mut out: Vec<usize> = (0..cenas.len()).collect();
    out.sort_by(|&a, &b| idx(&cenas[a]).cmp(&idx(&cenas[b])).then(ord(&cenas[a]).partial_cmp(&ord(&cenas[b])).unwrap_or(std::cmp::Ordering::Equal)));
    out
}

/// Scenes of one chapter (None = ideas), in order.
pub fn scenes_in(data: &Json, cap: Option<&str>) -> Vec<usize> {
    let cenas = data["cenas"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let mut out: Vec<usize> = (0..cenas.len()).filter(|&i| cap_of(&cenas[i]) == cap).collect();
    out.sort_by(|&a, &b| ord(&cenas[a]).partial_cmp(&ord(&cenas[b])).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn scene_index(data: &Json, id: &str) -> Option<usize> {
    data["cenas"].as_array()?.iter().position(|s| s["id"] == id)
}

pub fn trama<'a>(data: &'a Json, id: &str) -> Option<&'a Json> {
    data["tramas"].as_array()?.iter().find(|t| t["id"] == id)
}

pub fn new_scene(data: &mut Json, cap: Option<&str>, tramas: &[&str]) -> String {
    let next = (scenes_in(data, cap).iter().map(|&i| ord(&data["cenas"][i])).fold(0.0, f64::max) + 1.0).floor() as i64;
    let id = uid("s");
    let s = json!({ "id": id, "titulo": "", "resumo": "", "cap": cap, "ord": next, "tramas": tramas, "pov": "", "local": "", "quando": "", "batidas": [], "status": "ideia", "campos": {}, "notas": "" });
    data["cenas"].as_array_mut().unwrap().push(s);
    id
}

pub fn new_trama(data: &mut Json, nome: &str) -> String {
    let n = data["tramas"].as_array().map(|a| a.len()).unwrap_or(0);
    let id = uid("t");
    data["tramas"].as_array_mut().unwrap().push(json!({ "id": id, "nome": nome, "cor": TRAMA_CORES[n % TRAMA_CORES.len()], "tipo": "subtrama", "desc": "" }));
    id
}

/// Move a scene to a chapter (None = ideas), before `before` or at the end.
/// With `trama`, the scene leaves `from` and joins `to` ("__none" = no plot line).
pub fn move_scene(data: &mut Json, id: &str, cap: Option<&str>, before: Option<&str>, trama: Option<(Option<&str>, &str)>) {
    let Some(i) = scene_index(data, id) else { return };
    data["cenas"][i]["cap"] = json!(cap);
    if let Some((from, to)) = trama {
        let mut tr: Vec<String> = tramas_of(&data["cenas"][i]).into_iter().filter(|t| Some(*t) != from).map(String::from).collect();
        if to != "__none" && !tr.iter().any(|t| t == to) {
            tr.insert(0, to.to_string());
        }
        data["cenas"][i]["tramas"] = json!(tr);
    }
    let mut sibs: Vec<usize> = scenes_in(data, cap).into_iter().filter(|&j| j != i).collect();
    let at = before.and_then(|b| sibs.iter().position(|&j| data["cenas"][j]["id"] == b)).unwrap_or(sibs.len());
    sibs.insert(at, i);
    for (k, j) in sibs.into_iter().enumerate() {
        data["cenas"][j]["ord"] = json!(k + 1);
    }
}

pub fn delete_scene(data: &mut Json, id: &str) {
    if let Some(a) = data["cenas"].as_array_mut() {
        a.retain(|s| s["id"] != id);
    }
    for p in data["pistas"].as_array_mut().into_iter().flatten() {
        for k in ["plantios", "colheitas"] {
            if let Some(l) = p[k].as_array_mut() {
                l.retain(|x| x["cena"] != id);
            }
        }
    }
    for a in data["arcos"].as_array_mut().into_iter().flatten() {
        if let Some(l) = a["pontos"].as_array_mut() {
            l.retain(|x| x["cena"] != id);
        }
    }
}

pub fn duplicate_scene(data: &mut Json, id: &str) -> Option<String> {
    let i = scene_index(data, id)?;
    let mut c = data["cenas"][i].clone();
    let nid = uid("s");
    c["id"] = json!(nid);
    c["titulo"] = json!(format!("{} (cópia)", c["titulo"].as_str().filter(|s| !s.is_empty()).unwrap_or("Cena")));
    data["cenas"].as_array_mut()?.push(c);
    Some(nid)
}

pub fn delete_trama(data: &mut Json, id: &str) {
    if let Some(a) = data["tramas"].as_array_mut() {
        a.retain(|t| t["id"] != id);
    }
    for s in data["cenas"].as_array_mut().into_iter().flatten() {
        if let Some(l) = s["tramas"].as_array_mut() {
            l.retain(|x| x != id);
        }
    }
}

/// Planned chapter: drop it from the outline; its scenes go back to ideas.
pub fn delete_planned(data: &mut Json, id: &str) {
    let key = format!("plan:{id}");
    for s in data["cenas"].as_array_mut().into_iter().flatten() {
        if s["cap"] == key.as_str() {
            s["cap"] = Json::Null;
        }
    }
    if let Some(a) = data["planejados"].as_array_mut() {
        a.retain(|p| p["id"] != id);
    }
}

/// Link a planned chapter to a file: its scenes move there.
pub fn link_planned(data: &mut Json, id: &str, file: &str) {
    let key = format!("plan:{id}");
    for s in data["cenas"].as_array_mut().into_iter().flatten() {
        if s["cap"] == key.as_str() {
            s["cap"] = json!(file);
        }
    }
    if let Some(p) = data["planejados"].as_array_mut().and_then(|a| a.iter_mut().find(|p| p["id"] == id)) {
        let mut files = plan_files(p);
        if !files.iter().any(|f| f == file) {
            files.push(file.to_string());
        }
        p["arquivo"] = json!(files[0]);
        p["arquivos"] = json!(files);
    }
}

/// Distinct parts (arcs) in chapter order.
pub fn parts(cols: &[Col]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in cols {
        if !c.part.is_empty() && !out.contains(&c.part) {
            out.push(c.part.clone());
        }
    }
    out
}

/// Books in the same series, by volume.
pub fn saga_books(ix: &Index, book: &Book, author: &str) -> Vec<Book> {
    let serie = book.serie.trim();
    if serie.is_empty() {
        return vec![book.clone()];
    }
    let mut out: Vec<Book> = book::books(ix, author).into_iter().filter(|b| b.serie.trim() == serie).collect();
    out.sort_by(|a, b| {
        let va = a.volume.parse::<f64>().unwrap_or(0.0);
        let vb = b.volume.parse::<f64>().unwrap_or(0.0);
        va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.title.cmp(&b.title))
    });
    out
}

/// "Ano 3, inverno" → 3.95 · "2016-03-04" → 2016.17 · "1288" → 1288 · "500 a.C." → -500
pub fn parse_quando(txt: &str) -> Option<f64> {
    let s = txt.to_lowercase();
    static ISO: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"(-?\d{1,6})-(\d{1,2})(?:-(\d{1,2}))?").unwrap());
    static NUM: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"(-?\d+(?:[.,]\d+)?)").unwrap());
    static AC: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"a\.?\s?c\b|antes de cristo|\bac\b|bce").unwrap());
    if let Some(c) = ISO.captures(&s) {
        let y: f64 = c[1].parse().ok()?;
        let m: f64 = c[2].parse().ok()?;
        let d = c.get(3).and_then(|d| d.as_str().parse::<f64>().ok()).map(|d| (d - 1.0) / 365.0).unwrap_or(0.0);
        return Some(y + (m - 1.0) / 12.0 + d);
    }
    let m = NUM.captures(&s)?;
    let mut y: f64 = m[1].replace(',', ".").parse().ok()?;
    if AC.is_match(&s) && y > 0.0 {
        y = -y;
    }
    const MESES: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
    for (i, mes) in MESES.iter().enumerate() {
        if regex::Regex::new(&format!(r"\b{mes}")).unwrap().is_match(&s) {
            return Some(y + i as f64 / 12.0);
        }
    }
    for (k, v) in [("primavera", 0.2), ("verão", 0.45), ("verao", 0.45), ("outono", 0.7), ("inverno", 0.95)] {
        if s.contains(k) {
            return Some(y + v);
        }
    }
    Some(y)
}

pub fn fmt_quando(v: f64) -> String {
    let y = v.floor() as i64;
    if y < 0 {
        format!("{} a.C.", -y)
    } else {
        y.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quando() {
        assert_eq!(parse_quando("1288"), Some(1288.0));
        assert_eq!(parse_quando("500 a.C."), Some(-500.0));
        assert!((parse_quando("Ano 3, inverno").unwrap() - 3.95).abs() < 1e-9);
        assert!((parse_quando("2016-03-01").unwrap() - (2016.0 + 2.0 / 12.0)).abs() < 1e-9);
        assert!((parse_quando("ano 10, mar").unwrap() - (10.0 + 2.0 / 12.0)).abs() < 1e-9);
        assert_eq!(parse_quando("um dia"), None);
        assert_eq!(fmt_quando(-499.5), "500 a.C.");
    }

    #[test]
    fn scenes() {
        let mut d = blank();
        d["cenas"] = json!([{ "id": "a", "cap": "c1", "ord": 1, "batida": "b2" }]);
        d["estrutura"] = json!({ "ref": "x" });
        migrate_structures(&mut d);
        assert_eq!(d["cenas"][0]["batidas"], json!(["e1:b2"]));
        let t = new_trama(&mut d, "Ana");
        let b = new_scene(&mut d, Some("c1"), &[&t]);
        let c = new_scene(&mut d, None, &[]);
        assert_eq!(d["cenas"][1]["ord"], json!(2));
        move_scene(&mut d, &c, Some("c1"), Some("a"), Some((None, &t)));
        let order: Vec<&str> = scenes_in(&d, Some("c1")).into_iter().map(|i| d["cenas"][i]["id"].as_str().unwrap()).collect();
        assert_eq!(order, vec![c.as_str(), "a", b.as_str()]);
        assert_eq!(tramas_of(&d["cenas"][2]), vec![t.as_str()]);
        delete_trama(&mut d, &t);
        assert!(tramas_of(&d["cenas"][2]).is_empty());
        delete_scene(&mut d, "a");
        assert_eq!(d["cenas"].as_array().unwrap().len(), 2);
    }
}
