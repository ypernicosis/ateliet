//! A book's soundtrack (`<book>/Trilha.md`, the plugin's format): tracks
//! with their metadata and cover, the themes (leitmotifs) they belong to,
//! the diary of what played while each chapter was written and what each
//! chapter was "written listening to". Tracks come from a playlist of the
//! macOS Music app, from links or from audio files of the vault.

use super::book::{self, Book};
use super::{read_json_block, write_json_block};
use crate::dataview::index::Index;
use serde_json::{json, Value as Json};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

pub const LANG: &str = "scriptorium-trilha";

pub const TEMA_TIPOS: [(&str, &str); 7] = [("personagem", "Personagem"), ("capitulo", "Capítulo"), ("cena", "Cena"), ("trama", "Trama"), ("batida", "Ato / batida"), ("lugar", "Lugar"), ("livro", "Livro inteiro")];

pub const AUDIO_EXT: [&str; 7] = ["mp3", "m4a", "ogg", "wav", "flac", "aac", "opus"];

pub fn path(b: &Book) -> String {
    format!("{}/Trilha.md", b.folder)
}

pub fn blank() -> Json {
    json!({ "v": 1, "fonte": null, "faixas": [], "diario": {}, "inspiracao": {}, "ouvidas": {} })
}

pub fn load(root: &Path, b: &Book) -> Json {
    let mut d = blank();
    if let Some(Json::Object(m)) = std::fs::read_to_string(root.join(path(b))).ok().and_then(|t| read_json_block(&t, LANG)) {
        for (k, v) in m {
            d[k] = v;
        }
    }
    if !d["faixas"].is_array() {
        d["faixas"] = json!([]);
    }
    for k in ["diario", "inspiracao", "ouvidas"] {
        if !d[k].is_object() {
            d[k] = json!({});
        }
    }
    d
}

pub fn save(root: &Path, ix: &Index, b: &Book, data: &Json) -> Result<String, String> {
    let p = path(b);
    let text = match std::fs::read_to_string(root.join(&p)) {
        Ok(t) => write_json_block(&t, LANG, data),
        Err(_) => {
            let head = format!(
                "---\nscriptorium: trilha\nlivro: \"{}\"\n---\n\nTrilha sonora de **{}**: faixas, temas e o diário do que tocava enquanto você escrevia. Abra pela tela Trilha do Scriptorium.\n",
                book::note_link(ix, &b.path, &p),
                b.title
            );
            write_json_block(&head, LANG, data)
        }
    };
    book::write(root, &p, &text)?;
    Ok(p)
}

/// Like an ASCII slug but keeping letters of any script (Japanese, Cyrillic…).
pub fn uni_slug(s: &str) -> String {
    let s = if s.is_empty() { "x" } else { s };
    let plain: String = s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().nfc().collect::<String>().to_lowercase();
    let mut out = String::new();
    for c in plain.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t = out.trim_matches('-').to_string();
    if t.is_empty() {
        "x".into()
    } else {
        t
    }
}

pub fn track_key(artist: &str, name: &str) -> String {
    format!("{}::{}", uni_slug(if artist.is_empty() { "?" } else { artist }), uni_slug(if name.is_empty() { "?" } else { name }))
}

/// For finding repeats: "(feat. …)", "(Remastered …)", "- Single" do not count.
fn dup_key(t: &Json) -> String {
    let paren = regex::Regex::new(r"(?i)\s*[(\[](feat\.?|with|com|remaster(ed)?|ao vivo|live|bonus track|deluxe)[^)\]]*[)\]]").unwrap();
    let tail = regex::Regex::new(r"(?i)\s+-\s+(single|remaster(ed)?.*)$").unwrap();
    let clean = |x: &str| tail.replace(&paren.replace_all(x, ""), "").to_string();
    format!("{}::{}", uni_slug(&clean(str_of(t, "artista"))), uni_slug(&clean(str_of(t, "nome"))))
}

pub fn str_of<'a>(v: &'a Json, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

pub fn fmt_dur(sec: f64) -> String {
    let s = sec.round().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn fmt_min(sec: f64) -> String {
    let m = (sec / 60.0).round() as i64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02}", m / 60, m % 60)
    }
}

pub fn tema_label(t: &str) -> &str {
    TEMA_TIPOS.iter().find(|x| x.0 == t).map(|x| x.1).unwrap_or(t)
}

pub fn find(data: &Json, key: &str) -> Option<usize> {
    data["faixas"].as_array()?.iter().position(|t| t["k"] == key)
}

/// A track of the soundtrack, or one only heard while writing.
pub fn meta(data: &Json, key: &str) -> Option<Json> {
    find(data, key).map(|i| data["faixas"][i].clone()).or_else(|| data["ouvidas"].get(key).cloned())
}

/// Pairs (kept, repeat) of tracks that are the same: same id in the app, or same artist and name.
pub fn duplicates(data: &Json) -> Vec<(usize, usize)> {
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    let mut out = Vec::new();
    for (i, t) in data["faixas"].as_array().into_iter().flatten().enumerate() {
        let id = t["app"]["tid"].as_str().filter(|s| !s.is_empty()).map(|s| format!("tid:{s}"));
        let dk = dup_key(t);
        if let Some(&k) = id.as_ref().and_then(|id| seen.get(id)).or(seen.get(&dk)) {
            out.push((k, i));
            continue;
        }
        if let Some(id) = id {
            seen.insert(id, i);
        }
        seen.insert(dk, i);
    }
    out
}

fn move_key(data: &mut Json, from: &str, to: &str) {
    if from.is_empty() || from == to {
        return;
    }
    if let Some(d) = data["diario"].as_object_mut() {
        for ch in d.values_mut() {
            if let Some(v) = ch.as_object_mut().and_then(|o| o.remove(from)) {
                let n = ch[to].as_f64().unwrap_or(0.0) + v.as_f64().unwrap_or(0.0);
                ch[to] = json!(n);
            }
        }
    }
    if let Some(m) = data["inspiracao"].as_object_mut() {
        for v in m.values_mut() {
            if v["k"] == from {
                v["k"] = json!(to);
            }
        }
    }
    if data["ouvidas"].get(from).is_some() && data["ouvidas"].get(to).is_none() {
        if let Some(v) = data["ouvidas"].as_object_mut().and_then(|o| o.remove(from)) {
            data["ouvidas"][to] = v;
        }
    }
}

/// Fold repeats into the first copy (themes, notes, diary) and re-key
/// tracks whose old keys collided. Returns (removed, re-keyed).
pub fn tidy(data: &mut Json) -> (usize, usize) {
    let dups = duplicates(data);
    for &(keep, t) in &dups {
        let dup = data["faixas"][t].clone();
        let mut k = data["faixas"][keep].clone();
        for m in dup["temas"].as_array().into_iter().flatten() {
            let has = k["temas"].as_array().is_some_and(|a| a.iter().any(|x| x["t"] == m["t"] && x["ref"] == m["ref"]));
            if !has {
                if !k["temas"].is_array() {
                    k["temas"] = json!([]);
                }
                k["temas"].as_array_mut().unwrap().push(m.clone());
            }
        }
        let (dn, kn) = (str_of(&dup, "notas").to_string(), str_of(&k, "notas").to_string());
        if !dn.is_empty() && !kn.contains(&dn) {
            k["notas"] = json!([kn, dn].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n"));
        }
        if str_of(&k, "capa").is_empty() && !str_of(&dup, "capa").is_empty() {
            k["capa"] = dup["capa"].clone();
        }
        let (dk, kk) = (str_of(&dup, "k").to_string(), str_of(&k, "k").to_string());
        data["faixas"][keep] = k;
        let shared = data["faixas"].as_array().unwrap().iter().enumerate().any(|(i, x)| i != t && x["k"] == dk.as_str());
        if dk != kk && !shared {
            move_key(data, &dk, &kk);
        }
    }
    let gone: std::collections::HashSet<usize> = dups.iter().map(|d| d.1).collect();
    let mut i = 0;
    data["faixas"].as_array_mut().unwrap().retain(|_| {
        let keep = !gone.contains(&i);
        i += 1;
        keep
    });
    // keys that collided: recompute from the full spelling
    let faixas = data["faixas"].as_array().cloned().unwrap_or_default();
    let count = |k: &str| faixas.iter().filter(|x| x["k"] == k).count();
    let capa_use = |c: &str| faixas.iter().filter(|x| x["capa"] == c).count();
    let mut rekeyed = 0;
    for i in 0..faixas.len() {
        let t = data["faixas"][i].clone();
        let nk = track_key(str_of(&t, "artista"), str_of(&t, "nome"));
        if nk == str_of(&t, "k") || data["faixas"].as_array().unwrap().iter().enumerate().any(|(j, x)| j != i && x["k"] == nk.as_str()) {
            continue;
        }
        if count(str_of(&t, "k")) == 1 {
            move_key(data, str_of(&t, "k"), &nk);
        }
        if !str_of(&t, "capa").is_empty() && capa_use(str_of(&t, "capa")) > 1 {
            if let Some(o) = data["faixas"][i].as_object_mut() {
                o.remove("capa");
            }
        }
        data["faixas"][i]["k"] = json!(nk);
        rekeyed += 1;
    }
    (dups.len(), rekeyed)
}

/// What played while this chapter was written, most first.
pub fn diary_for(data: &Json, chapter: &str, pending: Option<&Json>) -> Vec<(String, f64)> {
    let mut d: Vec<(String, f64)> = data["diario"][chapter].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0))).collect()).unwrap_or_default();
    if let Some(p) = pending.and_then(|p| p[chapter].as_object()) {
        for (k, v) in p {
            match d.iter_mut().find(|x| &x.0 == k) {
                Some(x) => x.1 += v.as_f64().unwrap_or(0.0),
                None => d.push((k.clone(), v.as_f64().unwrap_or(0.0))),
            }
        }
    }
    d.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    d
}

/// Seconds of writing to music: `{chapter: {key: secs}}` added to the diary;
/// tracks not in the soundtrack are kept in `ouvidas`.
pub fn add_diary(data: &mut Json, chapters: &Json, metas: &Json) {
    for (ch, tracks) in chapters.as_object().into_iter().flatten() {
        if !data["diario"][ch].is_object() {
            data["diario"][ch] = json!({});
        }
        for (k, s) in tracks.as_object().into_iter().flatten() {
            let n = data["diario"][ch][k].as_f64().unwrap_or(0.0) + s.as_f64().unwrap_or(0.0);
            data["diario"][ch][k] = json!(n.round());
        }
    }
    for (k, m) in metas.as_object().into_iter().flatten() {
        if find(data, k).is_none() {
            data["ouvidas"][k] = m.clone();
        }
    }
}

/// Tracks whose themes point at this chapter (directly, its POV or its part).
pub fn theme_keys_for_chapter(data: &Json, chapter: &str, fm: &Json) -> Vec<String> {
    let pov = super::link_text(&fm["pov"]);
    let parte = fm["parte"].as_str().unwrap_or("");
    let mut keys = Vec::new();
    for t in data["faixas"].as_array().into_iter().flatten() {
        for m in t["temas"].as_array().into_iter().flatten() {
            let r = str_of(m, "ref");
            let hit = match str_of(m, "t") {
                "capitulo" => r == chapter,
                "personagem" => !pov.is_empty() && r.ends_with(&format!("/{pov}.md")),
                "trama" => !parte.is_empty() && r == parte,
                _ => false,
            };
            if hit {
                keys.push(str_of(t, "k").to_string());
            }
        }
    }
    keys
}

/// A track of a playlist of the Music app.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct AppTrack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub dur: f64,
    #[serde(default)]
    pub year: Json,
    #[serde(default)]
    pub genre: String,
}

/// Bring a playlist's tracks in, keeping the themes, notes and covers of
/// those already there, the tracks added by hand, and leaving out the
/// ones the writer removed. Returns the indices that still need a cover,
/// how many repeats the playlist had and how many stayed out.
pub fn merge_playlist(data: &mut Json, pid: &str, pname: &str, tracks: &[AppTrack], today: &str) -> (Vec<usize>, usize, usize) {
    let old: Vec<Json> = data["faixas"].as_array().cloned().unwrap_or_default();
    let out: std::collections::HashSet<String> = data["removidas"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
    let mut faixas: Vec<Json> = Vec::new();
    let (mut rep, mut fora) = (0, 0);
    let mut need = Vec::new();
    for t in tracks {
        let k = track_key(&t.artist, &t.name);
        if faixas.iter().any(|x| x["k"] == k.as_str() || x["app"]["tid"] == t.id.as_str()) {
            rep += 1;
            continue;
        }
        if out.contains(&k) {
            fora += 1;
            continue;
        }
        let by_tid = old.iter().find(|x| x["app"]["tid"] == t.id.as_str() && track_key(str_of(x, "artista"), str_of(x, "nome")) == k);
        let prev = by_tid.or_else(|| old.iter().find(|x| x["k"] == k.as_str())).cloned().unwrap_or(json!({}));
        let mut item = json!({ "temas": [], "notas": "" });
        for (kk, v) in prev.as_object().into_iter().flatten() {
            item[kk] = v.clone();
        }
        let year = match &t.year {
            Json::Number(n) if n.as_i64().unwrap_or(0) > 0 => json!(n),
            _ => json!(""),
        };
        for (kk, v) in [("k", json!(k)), ("nome", json!(t.name)), ("artista", json!(t.artist)), ("album", json!(t.album)), ("ano", year), ("genero", json!(t.genre)), ("dur", json!(t.dur.round() as i64)), ("app", json!({ "pid": pid, "tid": t.id }))] {
            item[kk] = v;
        }
        if !str_of(&prev, "k").is_empty() && str_of(&prev, "k") != k {
            item["capa"] = json!("");
        }
        if str_of(&item, "capa").is_empty() {
            need.push(faixas.len());
        }
        faixas.push(item);
    }
    // tracks added by hand (links, files) stay
    for t in &old {
        if t["app"].is_null() && !faixas.iter().any(|x| x["k"] == t["k"]) {
            faixas.push(t.clone());
        }
    }
    data["faixas"] = json!(faixas);
    data["fonte"] = json!({ "tipo": "app", "playlist": { "id": pid, "nome": pname }, "atualizado": today });
    (need, rep, fora)
}

/// Lines of a `playlist` property: "Title | link" or just the link.
pub fn playlist_items(items: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in items {
        let mut raw = raw.trim().to_string();
        if raw.is_empty() || raw.starts_with('#') || raw.starts_with("//") {
            continue;
        }
        let mut title = String::new();
        if let Some(i) = raw.rfind(" | ").filter(|i| *i > 0) {
            title = raw[..i].trim().to_string();
            raw = raw[i + 3..].trim().to_string();
        }
        let src = raw.trim_start_matches("- ").to_string();
        out.push((title, src));
    }
    out
}

/// Which service a link plays on, for "Abrir no …".
pub fn service(src: &str) -> Option<&'static str> {
    let s = src.to_lowercase();
    if s.contains("open.spotify.com/") {
        Some("Spotify")
    } else if s.contains("music.apple.com/") {
        Some("Apple Music")
    } else if s.contains("youtube.com/") || s.contains("youtu.be/") {
        Some("YouTube")
    } else if s.contains("soundcloud.com/") {
        Some("SoundCloud")
    } else {
        None
    }
}

pub fn is_audio(path: &str) -> bool {
    let l = path.to_lowercase();
    AUDIO_EXT.iter().any(|e| l.ends_with(&format!(".{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(uni_slug("Ólafur Arnalds"), "olafur-arnalds");
        assert_eq!(uni_slug("坂本龍一"), "坂本龍一");
        assert_eq!(uni_slug(""), "x");
        assert_eq!(track_key("", "Nuvole Bianche"), "x::nuvole-bianche");
        assert_eq!(fmt_dur(125.4), "2:05");
        assert_eq!(fmt_min(3720.0), "1 h 02");
        assert_eq!(fmt_min(600.0), "10 min");
    }

    #[test]
    fn repeats_fold() {
        let mut d = blank();
        d["faixas"] = json!([
            { "k": "a::x", "nome": "X", "artista": "A", "temas": [{ "t": "livro", "ref": "L" }] },
            { "k": "a::x-2", "nome": "X (Remastered 2011)", "artista": "A", "notas": "chuva", "temas": [{ "t": "lugar", "ref": "Mar" }] },
            { "k": "b::y", "nome": "Y", "artista": "B" }
        ]);
        d["diario"] = json!({ "c.md": { "a::x-2": 60 } });
        assert_eq!(duplicates(&d), vec![(0, 1)]);
        let (removed, _) = tidy(&mut d);
        assert_eq!(removed, 1);
        assert_eq!(d["faixas"].as_array().unwrap().len(), 2);
        assert_eq!(d["faixas"][0]["temas"].as_array().unwrap().len(), 2);
        assert_eq!(d["faixas"][0]["notas"], "chuva");
        assert_eq!(d["diario"]["c.md"]["a::x"], 60.0);
    }

    #[test]
    fn playlist_merge() {
        let mut d = blank();
        d["faixas"] = json!([
            { "k": "a::x", "nome": "X", "artista": "A", "temas": [{ "t": "livro", "ref": "L" }], "capa": "c.jpg", "app": { "pid": "P", "tid": "1" } },
            { "k": "manual::m", "nome": "M", "artista": "manual", "link": "https://open.spotify.com/track/1" }
        ]);
        d["removidas"] = json!(["c::z"]);
        let tr = |id: &str, a: &str, n: &str| AppTrack { id: id.into(), name: n.into(), artist: a.into(), dur: 200.4, ..Default::default() };
        let (need, rep, fora) = merge_playlist(&mut d, "P", "Livro", &[tr("1", "A", "X"), tr("2", "B", "Y"), tr("2", "B", "Y"), tr("3", "C", "Z")], "2026-10-22");
        assert_eq!((rep, fora), (1, 1));
        let f = d["faixas"].as_array().unwrap();
        assert_eq!(f.len(), 3);
        assert_eq!(f[0]["temas"].as_array().unwrap().len(), 1);
        assert_eq!(f[0]["capa"], "c.jpg");
        assert_eq!(f[1]["dur"], 200);
        assert_eq!(need, vec![1]);
        assert_eq!(f[2]["k"], "manual::m");
        assert_eq!(d["fonte"]["playlist"]["nome"], "Livro");
    }

    #[test]
    fn diary() {
        let mut d = blank();
        d["faixas"] = json!([{ "k": "a::x", "nome": "X" }]);
        add_diary(&mut d, &json!({ "c.md": { "a::x": 30.4, "b::y": 90 } }), &json!({ "b::y": { "nome": "Y", "artista": "B" } }));
        assert_eq!(diary_for(&d, "c.md", Some(&json!({ "c.md": { "a::x": 100 } })))[0], ("a::x".to_string(), 130.0));
        assert_eq!(d["ouvidas"]["b::y"]["nome"], "Y");
        assert_eq!(playlist_items(&["Chuva | https://youtu.be/abc".into(), "# nota".into()]), vec![("Chuva".to_string(), "https://youtu.be/abc".to_string())]);
    }
}
