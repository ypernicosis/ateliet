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

pub const PISTA_TIPOS: [(&str, &str); 8] = [("pista", "Pista"), ("foreshadowing", "Foreshadowing"), ("misterio", "Mistério"), ("promessa", "Promessa"), ("objeto", "Objeto"), ("segredo", "Segredo"), ("profecia", "Profecia"), ("armadilha", "Arma de Tchekhov")];

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

/// A scene without a chapter that happens somewhere between two chapters
/// (paths, or `plan:<id>`), kept in `entre: [de, ate]`.
pub fn range_of(s: &Json) -> Option<(&str, &str)> {
    if cap_of(s).is_some() {
        return None;
    }
    let a = s["entre"].as_array()?;
    let de = a.first()?.as_str().filter(|x| !x.is_empty())?;
    let ate = a.get(1).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).unwrap_or(de);
    Some((de, ate))
}

/// Place a scene somewhere between two chapters (None = no range).
pub fn set_range(data: &mut Json, id: &str, range: Option<(&str, &str)>) {
    let Some(i) = scene_index(data, id) else { return };
    let Some(o) = data["cenas"][i].as_object_mut() else { return };
    match range {
        Some((de, ate)) => {
            o.insert("cap".into(), Json::Null);
            o.insert("entre".into(), json!([de, ate]));
        }
        None => {
            o.remove("entre");
        }
    }
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
    if cap.is_some() {
        if let Some(o) = data["cenas"][i].as_object_mut() {
            o.remove("entre");
        }
    }
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
        if s["entre"].as_array().is_some_and(|a| a.iter().any(|x| x == key.as_str())) {
            s.as_object_mut().map(|o| o.remove("entre"));
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
        for x in s["entre"].as_array_mut().into_iter().flatten() {
            if x == key.as_str() {
                *x = json!(file);
            }
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

/// Notes were renamed or moved (`(old, new)` paths): the references the
/// plot keeps by path follow them (scene chapters, planned chapters' files,
/// structure ranges, chapter and character cards). True if anything changed.
pub fn relink(data: &mut Json, moves: &[(String, String)]) -> bool {
    fn fix(v: &mut Json, moves: &[(String, String)]) -> bool {
        let Some(s) = v.as_str() else { return false };
        match moves.iter().find(|(o, _)| o == s) {
            Some((_, n)) => {
                *v = json!(n);
                true
            }
            None => false,
        }
    }
    let mut ch = false;
    for s in data["cenas"].as_array_mut().into_iter().flatten() {
        ch |= fix(&mut s["cap"], moves);
        for x in s["entre"].as_array_mut().into_iter().flatten() {
            ch |= fix(x, moves);
        }
    }
    for p in data["planejados"].as_array_mut().into_iter().flatten() {
        if p.get("arquivo").is_some() {
            ch |= fix(&mut p["arquivo"], moves);
        }
        for f in p["arquivos"].as_array_mut().into_iter().flatten() {
            ch |= fix(f, moves);
        }
    }
    for a in data["estruturas"].as_array_mut().into_iter().flatten() {
        for k in ["de", "ate"] {
            if a.get(k).is_some() {
                ch |= fix(&mut a[k], moves);
            }
        }
    }
    for board in ["mural", "relacoes"] {
        for c in data[board]["cards"].as_array_mut().into_iter().flatten() {
            if c.get("link").is_some() {
                ch |= fix(&mut c["link"], moves);
            }
        }
    }
    ch
}

/// `relink` on the Enredo note at `rel` (relative to `root`), written back
/// only when something changed.
pub fn relink_file(root: &Path, rel: &str, moves: &[(String, String)]) -> Result<bool, String> {
    let full = root.join(rel);
    let Ok(text) = std::fs::read_to_string(&full) else { return Ok(false) };
    let Some(mut data) = read_json_block(&text, LANG) else { return Ok(false) };
    if !relink(&mut data, moves) {
        return Ok(false);
    }
    std::fs::write(&full, write_json_block(&text, LANG, &data)).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Planned chapters whose files are gone (or never existed) take the written
/// chapter with the same number, when no other plan has it: a chapter made
/// in the manuscript and the one planned in the outline are the same chapter,
/// not two. `written` is (path, number) of the numbered chapters, in order.
/// Their scenes go along. True if anything changed.
pub fn auto_link(data: &mut Json, written: &[(String, usize)]) -> bool {
    let Some(plans) = data["planejados"].as_array() else { return false };
    let exists = |f: &str| written.iter().any(|(p, _)| p == f);
    let taken: Vec<String> = plans.iter().flat_map(plan_files).filter(|f| exists(f)).collect();
    let mut links: Vec<(String, Vec<String>, String)> = Vec::new();
    for p in plans {
        let files = plan_files(p);
        if files.iter().any(|f| exists(f)) {
            continue;
        }
        let Some(n) = p["num"].as_f64().filter(|n| n.fract() == 0.0 && *n >= 1.0) else { continue };
        let Some((path, _)) = written.iter().find(|(_, k)| *k as f64 == n) else { continue };
        if taken.contains(path) || links.iter().any(|l| &l.2 == path) {
            continue;
        }
        links.push((p["id"].as_str().unwrap_or("").to_string(), files, path.clone()));
    }
    for (id, stale, path) in &links {
        let key = format!("plan:{id}");
        for s in data["cenas"].as_array_mut().into_iter().flatten() {
            if s["cap"] == key.as_str() || s["cap"].as_str().is_some_and(|c| stale.iter().any(|f| f == c)) {
                s["cap"] = json!(path);
            }
            for x in s["entre"].as_array_mut().into_iter().flatten() {
                if x == key.as_str() || x.as_str().is_some_and(|c| stale.iter().any(|f| f == c)) {
                    *x = json!(path);
                }
            }
        }
        if let Some(p) = data["planejados"].as_array_mut().and_then(|a| a.iter_mut().find(|p| p["id"] == id.as_str())) {
            p["arquivo"] = json!(path);
            p["arquivos"] = json!([path]);
        }
    }
    !links.is_empty()
}

/// (path, number) of the book's numbered chapters, as the grid numbers them.
pub fn numbered(ix: &Index, book: &Book) -> Vec<(String, usize)> {
    let mut n = 0;
    book::chapters(ix, book)
        .into_iter()
        .filter(|c| c.numbered && c.include)
        .map(|c| {
            n += 1;
            (c.path, n)
        })
        .collect()
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

// ── quick capture ─────────────────────────────────────────────────────────

/// A scene as typed in the outline's quick entry, before it is placed:
/// `@pov`, `#trama` and `^cap` (`^3`, `^3-6`) are read off the title.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Draft {
    pub titulo: String,
    pub resumo: String,
    pub pov: Option<String>,
    pub tramas: Vec<String>,
    /// Chapter labels: one chapter, or a range.
    pub cap: Option<(String, Option<String>)>,
}

/// The longest first line still taken as a title; longer text is all summary.
const TITLE_MAX: usize = 100;

/// `- `, `* `, `+ `, `• `, `– `, `1. `, `1) `, `## `, with an optional `[ ]`.
fn list_item(line: &str) -> Option<(usize, &str)> {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"^([ \t]*)(?:[-*+•–]|\d{1,3}[.)]|#{1,6})[ \t]+(?:\[[ xX]\][ \t]+)?(.*)$").unwrap());
    let c = RE.captures(line)?;
    let indent = c[1].chars().map(|ch| if ch == '\t' { 4 } else { 1 }).sum();
    Some((indent, c.get(2).map(|m| m.as_str()).unwrap_or("")))
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).map(|ch| if ch == '\t' { 4 } else { 1 }).sum()
}

/// Lines under a title: dedented, blank runs collapsed, trimmed.
fn body(lines: &[&str]) -> String {
    let min = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| indent_of(l)).min().unwrap_or(0);
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if l.trim().is_empty() {
            if out.last().is_some_and(|x| !x.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        let mut cut = 0;
        let mut w = 0;
        for (k, ch) in l.char_indices() {
            if w >= min || !ch.is_whitespace() {
                break;
            }
            w += if ch == '\t' { 4 } else { 1 };
            cut = k + ch.len_utf8();
        }
        out.push(l[cut..].trim_end().to_string());
    }
    out.join("\n").trim().to_string()
}

/// A title without its marks, then the point of view, the plot lines and the chapter(s).
pub type Tokens = (String, Option<String>, Vec<String>, Option<(String, Option<String>)>);

/// Read `@pov`, `#trama` and `^cap` off a title; `_` stands for a space.
pub fn take_tokens(title: &str) -> Tokens {
    let mut words = Vec::new();
    let mut pov = None;
    let mut tramas = Vec::new();
    let mut cap = None;
    for w in title.split_whitespace() {
        let clean = |t: &str| t.trim_end_matches([',', '.', ';', ':', '!', '?']).replace('_', " ");
        let mut ch = w.chars();
        let first = ch.next();
        let rest = ch.as_str();
        match first {
            Some('@') if !rest.is_empty() => pov = Some(clean(rest)),
            Some('#') if !rest.is_empty() && !rest.starts_with('#') => tramas.push(clean(rest)),
            Some('^') if !rest.is_empty() => {
                let r = clean(rest);
                let mut it = r.splitn(2, ['-', '–']);
                let a = it.next().unwrap_or("").trim().to_string();
                let b = it.next().map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
                if !a.is_empty() {
                    cap = Some((a, b));
                }
            }
            _ => words.push(w),
        }
    }
    (words.join(" "), pov, tramas, cap)
}

fn draft(title_line: &str, rest: &[&str]) -> Draft {
    let t = title_line.trim();
    if t.chars().count() > TITLE_MAX {
        let mut all = vec![title_line];
        all.extend_from_slice(rest);
        return Draft { resumo: body(&all), ..Default::default() };
    }
    let (titulo, pov, tramas, cap) = take_tokens(t);
    Draft { titulo, resumo: body(rest), pov, tramas, cap }
}

/// What was typed or pasted in the quick entry, as scenes: a list (`-`,
/// `*`, `1.`…) gives a scene per item, with the lines under an item (and
/// deeper items) as its summary; plain text gives one scene, the first
/// line as title and the rest as summary.
pub fn parse_capture(text: &str) -> Vec<Draft> {
    let lines: Vec<&str> = text.lines().collect();
    let items: Vec<(usize, usize)> = lines.iter().enumerate().filter_map(|(k, l)| list_item(l).map(|(ind, _)| (k, ind))).collect();
    let mut out = Vec::new();
    let one = |ls: &[&str], out: &mut Vec<Draft>| {
        let Some(first) = ls.iter().position(|l| !l.trim().is_empty()) else { return };
        let d = draft(ls[first], &ls[first + 1..]);
        if !d.titulo.is_empty() || !d.resumo.is_empty() || d.pov.is_some() || !d.tramas.is_empty() {
            out.push(d);
        }
    };
    if items.is_empty() {
        one(&lines, &mut out);
        return out;
    }
    let base = items.iter().map(|x| x.1).min().unwrap_or(0);
    let heads: Vec<usize> = items.iter().filter(|x| x.1 == base).map(|x| x.0).collect();
    one(&lines[..heads[0]], &mut out);
    for (n, &h) in heads.iter().enumerate() {
        let end = heads.get(n + 1).copied().unwrap_or(lines.len());
        let (_, rest) = list_item(lines[h]).unwrap_or((0, lines[h]));
        let d = draft(rest, &lines[h + 1..end]);
        if !d.titulo.is_empty() || !d.resumo.is_empty() {
            out.push(d);
        }
    }
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

    #[test]
    fn relinks_paths() {
        let mut d = blank();
        d["cenas"] = json!([{ "id": "a", "cap": "L/M/01 — Um.md" }, { "id": "b", "cap": "L/M/02 — Dois.md" }, { "id": "c", "cap": null }]);
        d["planejados"] = json!([{ "id": "p", "arquivo": "L/M/01 — Um.md", "arquivos": ["L/M/01 — Um.md"] }]);
        d["estruturas"] = json!([{ "id": "e1", "escopo": "capitulos", "de": "L/M/01 — Um.md", "ate": "L/M/02 — Dois.md" }]);
        d["mural"]["cards"] = json!([{ "t": "capitulo", "link": "L/M/01 — Um.md" }]);
        let moves = vec![("L/M/01 — Um.md".to_string(), "L/M/01 — Primeiro.md".to_string())];
        assert!(relink(&mut d, &moves));
        assert_eq!(d["cenas"][0]["cap"], json!("L/M/01 — Primeiro.md"));
        assert_eq!(d["cenas"][1]["cap"], json!("L/M/02 — Dois.md"));
        assert!(d["cenas"][2]["cap"].is_null());
        assert_eq!(d["planejados"][0]["arquivo"], json!("L/M/01 — Primeiro.md"));
        assert_eq!(d["planejados"][0]["arquivos"], json!(["L/M/01 — Primeiro.md"]));
        assert_eq!(d["estruturas"][0]["de"], json!("L/M/01 — Primeiro.md"));
        assert_eq!(d["mural"]["cards"][0]["link"], json!("L/M/01 — Primeiro.md"));
        assert!(!relink(&mut d, &moves));
    }

    #[test]
    fn plans_take_written_chapters() {
        let mut d = blank();
        d["planejados"] = json!([
            { "id": "p1", "num": 1, "arquivo": "M/03 — Um.md", "arquivos": ["M/03 — Um.md"] },
            { "id": "p2", "num": 2, "titulo": "" },
            { "id": "p3", "num": 3, "arquivos": ["M/apagado.md"] },
            { "id": "p4", "num": 4 }
        ]);
        d["cenas"] = json!([{ "id": "a", "cap": "plan:p2" }, { "id": "b", "cap": "M/apagado.md" }]);
        let written = vec![("M/03 — Um.md".to_string(), 1), ("M/04 — Dois.md".to_string(), 2), ("M/05 — Tres.md".to_string(), 3)];
        assert!(auto_link(&mut d, &written));
        assert_eq!(d["planejados"][1]["arquivos"], json!(["M/04 — Dois.md"]));
        assert_eq!(d["planejados"][2]["arquivos"], json!(["M/05 — Tres.md"]));
        assert!(d["planejados"][3]["arquivo"].is_null());
        assert_eq!(d["cenas"][0]["cap"], json!("M/04 — Dois.md"));
        assert_eq!(d["cenas"][1]["cap"], json!("M/05 — Tres.md"));
        assert!(!auto_link(&mut d, &written));
    }

    #[test]
    fn capture_lists_and_text() {
        let d = parse_capture("Ana acha a carta @ana #mistério ^3");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].titulo, "Ana acha a carta");
        assert_eq!(d[0].pov.as_deref(), Some("ana"));
        assert_eq!(d[0].tramas, vec!["mistério".to_string()]);
        assert_eq!(d[0].cap, Some(("3".into(), None)));
        let d = parse_capture("- Um ^3-6\n  detalhe do um\n  - sub\n\n- Dois #Casa_Velha\n* Três");
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].titulo, "Um");
        assert_eq!(d[0].cap, Some(("3".into(), Some("6".into()))));
        assert_eq!(d[0].resumo, "detalhe do um\n- sub");
        assert_eq!(d[1].tramas, vec!["Casa Velha".to_string()]);
        assert_eq!(d[2].titulo, "Três");
        let d = parse_capture("A fuga\nEles saem à noite.\n\nNa estrada, chove.");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].titulo, "A fuga");
        assert_eq!(d[0].resumo, "Eles saem à noite.\n\nNa estrada, chove.");
        let long = "x".repeat(120);
        let d = parse_capture(&long);
        assert_eq!(d[0].titulo, "");
        assert_eq!(d[0].resumo, long);
        assert!(parse_capture("  \n ").is_empty());
        let d = parse_capture("1. a\n2) b\n- [ ] c");
        assert_eq!(d.iter().map(|x| x.titulo.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
    }

    #[test]
    fn ranges() {
        let mut d = blank();
        d["cenas"] = json!([{ "id": "a", "cap": null, "ord": 1 }]);
        set_range(&mut d, "a", Some(("c1", "plan:p")));
        assert_eq!(range_of(&d["cenas"][0]), Some(("c1", "plan:p")));
        d["planejados"] = json!([{ "id": "p", "num": 2 }]);
        link_planned(&mut d, "p", "c2");
        assert_eq!(range_of(&d["cenas"][0]), Some(("c1", "c2")));
        assert!(relink(&mut d, &[("c1".into(), "c0".into())]));
        assert_eq!(range_of(&d["cenas"][0]), Some(("c0", "c2")));
        move_scene(&mut d, "a", Some("c2"), None, None);
        assert!(d["cenas"][0].get("entre").is_none());
        assert_eq!(range_of(&d["cenas"][0]), None);
    }
}
