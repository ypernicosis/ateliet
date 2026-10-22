//! Interactive maps: a note (`scriptorium: mapa`) whose JSON block holds a
//! base picture and layers over it (pins linked to notes or to other maps,
//! labels, stamps, routes, areas). Positions are percentages of the picture.
//! Distances come from a flat scale (km across the picture) or from a world
//! map in Mollweide projection (the whole sphere in an ellipse); layers can
//! exist only between two years, and the map can be seen in a given year.

use super::book::{self, Book};
use super::{read_json_block, write_json_block};
use crate::dataview::index::Index;
use serde_json::{json, Value as Json};
use std::path::Path;

pub const LANG: &str = "scriptorium-mapa";

/// Travel speeds (km a day) when the world sets none.
pub const TRAVEL: [(&str, f64); 4] = [("a pé", 30.0), ("a cavalo", 50.0), ("carroça", 25.0), ("navio a vela", 150.0)];

pub const STAMP_INK: &str = "#3b2a1a";
pub const STAMP_FILL: &str = "#efe2c4";
pub const PIN: &str = "#8b1e1e";

pub fn blank() -> Json {
    json!({ "v": 2, "imagem": "", "camadas": [], "view": null })
}

pub fn load(root: &Path, path: &str) -> Json {
    let mut d = std::fs::read_to_string(root.join(path)).ok().and_then(|t| read_json_block(&t, LANG)).filter(|j| j.is_object()).unwrap_or_else(blank);
    if !d["camadas"].is_array() {
        d["camadas"] = json!([]);
    }
    d
}

pub fn save(root: &Path, path: &str, data: &Json) -> Result<(), String> {
    let full = root.join(path);
    let text = std::fs::read_to_string(&full).map_err(|e| e.to_string())?;
    std::fs::write(&full, write_json_block(&text, LANG, data)).map_err(|e| e.to_string())
}

/// A new map note in `<home>/Mapas/`; returns its path.
pub fn create(root: &Path, ix: &Index, home: &Book, name: &str, image: &str, world: Option<(&str, &str)>) -> Result<String, String> {
    let name = super::safe_name(if name.trim().is_empty() { "Mapa" } else { name });
    let p = book::unique(root, &format!("{}/Mapas/{name}.md", home.folder));
    let head = format!(
        "---\nscriptorium: mapa\nlivro: \"{}\"\nmundo: \"{}\"\n---\n\nMapa interativo. Abra pela visão de mapa do Scriptorium.\n",
        book::note_link(ix, &home.path, &p),
        world.map(|w| w.1).unwrap_or("")
    );
    let base = image.rsplit('/').next().unwrap_or(image).to_lowercase();
    let escala = if base.contains("mollweide") || base.contains("mapa_mundi") { json!({ "proj": "mollweide", "raio": 6371, "elipse": [0, 0, 100, 100] }) } else { Json::Null };
    let d = json!({ "v": 2, "imagem": image, "camadas": [], "view": null, "mundo": world.map(|w| w.0).unwrap_or(""), "escala": escala });
    book::write(root, &p, &write_json_block(&head, LANG, &d))?;
    Ok(p)
}

/// The maps kept in these books' folders.
pub fn list(ix: &Index, books: &[Book]) -> Vec<String> {
    let mut out: Vec<String> = ix
        .file_paths()
        .into_iter()
        .cloned()
        .filter(|p| books.iter().any(|b| p.starts_with(&format!("{}/", b.folder))))
        .filter(|p| ix.page(p).is_some_and(|pg| book::kind(pg) == "mapa"))
        .collect();
    out.sort();
    out
}

// ── scale ────────────────────────────────────────────────────────────────

fn ellipse(esc: &Json) -> [f64; 4] {
    let e = esc["elipse"].as_array();
    let g = |i: usize, d: f64| e.and_then(|a| a.get(i)).and_then(|v| v.as_f64()).unwrap_or(d);
    [g(0, 0.0), g(1, 0.0), g(2, 100.0), g(3, 100.0)]
}

/// Latitude and longitude (radians) of a point of a Mollweide world map.
pub fn lat_lon(esc: &Json, p: [f64; 2]) -> Option<(f64, f64)> {
    let e = ellipse(esc);
    let x = (p[0] - e[0]) / (e[2] - e[0]) * 2.0 - 1.0;
    let y = 1.0 - (p[1] - e[1]) / (e[3] - e[1]) * 2.0;
    if x * x + y * y > 1.0001 {
        return None;
    }
    let th = y.clamp(-1.0, 1.0).asin();
    let lam = if th.cos().abs() < 1e-9 { 0.0 } else { std::f64::consts::PI * x / th.cos() };
    let phi = ((2.0 * th + (2.0 * th).sin()) / std::f64::consts::PI).clamp(-1.0, 1.0).asin();
    Some((phi, lam))
}

/// Kilometres between two points (percent of the picture `iw`×`ih`).
pub fn dist_km(esc: &Json, iw: f64, ih: f64, a: [f64; 2], b: [f64; 2]) -> Option<f64> {
    if !esc.is_object() {
        return None;
    }
    if esc["proj"] == "mollweide" {
        let (p0, l0) = lat_lon(esc, a)?;
        let (p1, l1) = lat_lon(esc, b)?;
        let r = esc["raio"].as_f64().filter(|r| *r > 0.0).unwrap_or(6371.0);
        let h = ((p1 - p0) / 2.0).sin().powi(2) + p0.cos() * p1.cos() * ((l1 - l0) / 2.0).sin().powi(2);
        return Some(2.0 * r * h.sqrt().min(1.0).asin());
    }
    let w = esc["kmLargura"].as_f64().filter(|w| *w > 0.0)?;
    let dx = (b[0] - a[0]) / 100.0 * iw;
    let dy = (b[1] - a[1]) / 100.0 * ih;
    Some(dx.hypot(dy) / iw * w)
}

pub fn path_km(esc: &Json, iw: f64, ih: f64, pts: &[[f64; 2]]) -> Option<f64> {
    let mut t = 0.0;
    for w in pts.windows(2) {
        t += dist_km(esc, iw, ih, w[0], w[1])?;
    }
    Some(t)
}

fn num(n: f64) -> String {
    if n < 10.0 {
        let s = format!("{:.1}", n);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        s.replace('.', ",")
    } else {
        super::fmt_n(n.round() as i64)
    }
}

/// The world's travel speeds (`viagem: [[name, km/day], …]`) or the defaults.
pub fn modes(world: Option<&Json>) -> Vec<(String, f64)> {
    let w: Vec<(String, f64)> = world
        .and_then(|w| w["viagem"].as_array())
        .map(|a| a.iter().filter_map(|m| Some((m.get(0)?.as_str()?.to_string(), m.get(1)?.as_f64()?))).filter(|m| m.1 > 0.0).collect())
        .unwrap_or_default();
    if w.is_empty() {
        TRAVEL.iter().map(|(n, v)| (n.to_string(), *v)).collect()
    } else {
        w
    }
}

/// "1.240 km" and "a pé 41 dias · a cavalo 25 dias · …"
pub fn travel(km: f64, modes: &[(String, f64)]) -> (String, Vec<String>) {
    let t = modes
        .iter()
        .map(|(n, v)| {
            let d = km / v;
            if d < 1.0 {
                format!("{n} {} h", num(d * 24.0))
            } else {
                format!("{n} {} {}", num(d), if d < 1.5 { "dia" } else { "dias" })
            }
        })
        .collect();
    (format!("{} km", num(km)), t)
}

/// The ellipse of a world map, found by its contrast with the corners'
/// colour (percent: left, top, right, bottom).
pub fn detect_ellipse(img: &image::RgbaImage) -> [f64; 4] {
    let w0 = 500u32;
    let small = image::imageops::resize(img, w0, ((img.height() as f64 / img.width().max(1) as f64) * w0 as f64).round().max(8.0) as u32, image::imageops::FilterType::Triangle);
    let (w, h) = (small.width() as i64, small.height() as i64);
    let at = |x: i64, y: i64| {
        let p = small.get_pixel(x.clamp(0, w - 1) as u32, y.clamp(0, h - 1) as u32);
        [p[0] as f64, p[1] as f64, p[2] as f64]
    };
    let corners = [at(2, 2), at(w - 3, 2), at(2, h - 3), at(w - 3, h - 3)];
    let bg: Vec<f64> = (0..3).map(|c| corners.iter().map(|q| q[c]).sum::<f64>() / 4.0).collect();
    let diff = |q: [f64; 3]| (q[0] - bg[0]).abs() + (q[1] - bg[1]).abs() + (q[2] - bg[2]).abs();
    let row_in = |y: i64| (0..w).step_by(2).filter(|x| diff(at(*x, y)) > 60.0).count() as f64 > w as f64 * 0.02;
    let col_in = |x: i64| (0..h).step_by(2).filter(|y| diff(at(x, *y)) > 60.0).count() as f64 > h as f64 * 0.02;
    let (mid_row, mid_col) = (h / 2, w / 2);
    let run_h = 12.max((h as f64 * 0.08).round() as i64);
    let run_w = 12.max((w as f64 * 0.08).round() as i64);
    // the edge is where a long run off the background starts (a title's strokes are short)
    let edge = |from: i64, to: i64, step: i64, get: &dyn Fn(i64) -> [f64; 3], run: i64| -> i64 {
        let mut k = from;
        while k != to {
            if diff(get(k)) >= 60.0 {
                let (mut n, mut m) = (0, 0);
                while m < run && k + m * step != to {
                    if diff(get(k + m * step)) >= 45.0 {
                        n += 1;
                    }
                    m += 1;
                }
                if m >= run && n as f64 >= run as f64 * 0.7 {
                    return k;
                }
            }
            k += step;
        }
        from
    };
    let mut l = edge(0, w - 1, 1, &|x| at(x, mid_row), run_w);
    let mut r = edge(w - 1, 0, -1, &|x| at(x, mid_row), run_w);
    let mut t = edge(0, h - 1, 1, &|y| at(mid_col, y), run_h);
    let mut b = edge(h - 1, 0, -1, &|y| at(mid_col, y), run_h);
    // titles and credits sit in the middle: measure top and bottom half a radius to each side
    let ea = (r - l) as f64 / 2.0;
    let ecx = (l + r) as f64 / 2.0;
    let k = 0.75f64.sqrt();
    let est: Vec<(f64, f64)> = [-0.5, 0.5]
        .iter()
        .map(|u| {
            let x = ((ecx + u * ea).round() as i64).clamp(0, w - 1);
            let tt = edge(0, h - 1, 1, &|y| at(x, y), run_h) as f64;
            let bb = edge(h - 1, 0, -1, &|y| at(x, y), run_h) as f64;
            ((tt + bb) / 2.0, (bb - tt) / (2.0 * k))
        })
        .collect();
    if (est[0].0 - est[1].0).abs() < h as f64 * 0.03 && (est[0].1 - est[1].1).abs() < h as f64 * 0.03 {
        let cy = (est[0].0 + est[1].0) / 2.0;
        let hb = (est[0].1 + est[1].1) / 2.0;
        t = 0.max((cy - hb).round() as i64);
        b = (h - 1).min((cy + hb).round() as i64 - 1);
    }
    if ((r - l) as f64) < w as f64 * 0.3 || ((b - t) as f64) < h as f64 * 0.3 {
        l = 0;
        while l < w && !col_in(l) {
            l += 1;
        }
        r = w - 1;
        while r > 0 && !col_in(r) {
            r -= 1;
        }
        t = 0;
        while t < h && !row_in(t) {
            t += 1;
        }
        b = h - 1;
        while b > 0 && !row_in(b) {
            b -= 1;
        }
    }
    let f = |v: f64| (v * 100.0).round() / 100.0;
    [f(l as f64 / w as f64 * 100.0), f(t as f64 / h as f64 * 100.0), f((r + 1) as f64 / w as f64 * 100.0), f((b + 1) as f64 / h as f64 * 100.0)]
}

// ── time ─────────────────────────────────────────────────────────────────

/// Whether a layer exists in the year the map shows.
pub fn visible(l: &Json, ano: Option<f64>) -> bool {
    let Some(a) = ano else { return true };
    !(l["de"].as_f64().is_some_and(|d| a < d) || l["ate"].as_f64().is_some_and(|d| a > d))
}

pub struct Seg {
    pub de: f64,
    pub ate: f64,
    pub nome: String,
    pub cor: String,
    w: f64,
}

/// The time bar: one stretch per era, each as wide as the square root of
/// its length (so a short era stays visible next to a long one).
pub struct TimeScale {
    pub segs: Vec<Seg>,
    tot: f64,
}

impl TimeScale {
    pub fn new(camadas: &[Json], eras: &[Json]) -> Option<TimeScale> {
        let mut ys = Vec::new();
        for l in camadas {
            ys.extend(l["de"].as_f64());
            ys.extend(l["ate"].as_f64());
        }
        let mut eras: Vec<&Json> = eras.iter().filter(|e| e["de"].is_number() && e["ate"].is_number()).collect();
        eras.sort_by(|a, b| a["de"].as_f64().partial_cmp(&b["de"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        for e in &eras {
            ys.push(e["de"].as_f64().unwrap_or(0.0));
            ys.push(e["ate"].as_f64().unwrap_or(0.0));
        }
        if ys.is_empty() {
            return None;
        }
        let lo = ys.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mk = |de: f64, ate: f64, nome: &str, cor: &str| Seg { de, ate, nome: nome.into(), cor: cor.into(), w: (ate - de).max(1.0).sqrt() };
        let segs: Vec<Seg> = if eras.is_empty() {
            vec![mk(lo, hi, "", "")]
        } else {
            eras.iter().map(|e| mk(e["de"].as_f64().unwrap_or(0.0), e["ate"].as_f64().unwrap_or(0.0), e["nome"].as_str().unwrap_or(""), e["cor"].as_str().unwrap_or(""))).collect()
        };
        let tot = segs.iter().map(|s| s.w).sum();
        Some(TimeScale { segs, tot })
    }

    /// Where each era starts and ends on the bar (0–1).
    pub fn spans(&self) -> Vec<(f64, f64)> {
        let mut acc = 0.0;
        self.segs
            .iter()
            .map(|s| {
                let a = acc / self.tot;
                acc += s.w;
                (a, acc / self.tot)
            })
            .collect()
    }

    pub fn to_pos(&self, y: f64) -> f64 {
        let mut acc = 0.0;
        for (i, s) in self.segs.iter().enumerate() {
            if y <= s.ate || i == self.segs.len() - 1 {
                return (acc + ((y - s.de) / (s.ate - s.de).max(1.0)).clamp(0.0, 1.0) * s.w) / self.tot;
            }
            acc += s.w;
        }
        1.0
    }

    pub fn from_pos(&self, p: f64) -> f64 {
        let t = p * self.tot;
        let mut acc = 0.0;
        for (i, s) in self.segs.iter().enumerate() {
            if t <= acc + s.w || i == self.segs.len() - 1 {
                return (s.de + ((t - acc) / s.w).clamp(0.0, 1.0) * (s.ate - s.de)).round();
            }
            acc += s.w;
        }
        self.segs.last().map(|s| s.ate).unwrap_or(0.0)
    }
}

// ── stamps ───────────────────────────────────────────────────────────────

/// key, name, drawing (SVG in a 100×100 box; fill "F" is the paper colour,
/// "I" the ink).
pub const STAMPS: [(&str, &str, &str); 26] = [
    ("arvore", "Árvore", r#"<path d="M50 94V60M50 68 40 58M50 72 60 62" fill="none"/><path d="M50 12C30 12 20 27 24 40 14 46 18 62 32 62 36 70 64 70 68 62 82 62 86 46 76 40 80 27 70 12 50 12Z" fill="F"/>"#),
    ("pinheiro", "Pinheiro", r#"<path d="M50 6 30 38H40L22 62H36L18 86H82L64 62H78L60 38H70Z" fill="F"/><path d="M50 86V96" fill="none"/>"#),
    ("floresta", "Floresta", r#"<g transform="translate(-6 18) scale(.6)"><path d="M50 6 30 38H40L22 62H36L18 86H82L64 62H78L60 38H70Z" fill="F"/></g><g transform="translate(40 14) scale(.6)"><path d="M50 6 30 38H40L22 62H36L18 86H82L64 62H78L60 38H70Z" fill="F"/></g><g transform="translate(17 -2) scale(.66)"><path d="M50 12C30 12 20 27 24 40 14 46 18 62 32 62 36 70 64 70 68 62 82 62 86 46 76 40 80 27 70 12 50 12Z" fill="F"/><path d="M50 66V96" fill="none"/></g>"#),
    ("montanha", "Montanha", r#"<path d="M6 88 42 20 58 48 66 36 94 88Z" fill="F"/><path d="M33 37 42 20 50 35 45 32 40 39Z" fill="I"/><path d="M47 42 58 88M53 54 64 88M70 50 80 88" fill="none" stroke-width="2"/>"#),
    ("montanhas", "Cordilheira", r#"<path d="M2 90 30 36 46 62 62 22 98 90Z" fill="F"/><path d="M54 37 62 22 70 36 65 33 60 39Z" fill="I"/><path d="M66 46 80 90M36 52 44 90" fill="none" stroke-width="2"/>"#),
    ("colina", "Colina", r#"<path d="M4 84C24 46 76 46 96 84Z" fill="F"/><path d="M60 62 66 84M70 66 74 84" fill="none" stroke-width="2"/>"#),
    ("vulcao", "Vulcão", r#"<path d="M6 90 36 34H64L94 90Z" fill="F"/><path d="M36 34 44 44 50 36 56 46 64 34" fill="I"/><path d="M46 28C40 20 50 16 46 8M56 26C62 18 52 14 58 6" fill="none" stroke-width="2.5"/>"#),
    ("lago", "Lago", r##"<path d="M12 60C12 34 86 28 88 54 90 78 18 84 12 60Z" fill="#cfe0e6"/><path d="M30 56C36 52 40 60 46 56M52 64C58 60 62 68 68 64" fill="none" stroke-width="2"/>"##),
    ("castelo", "Castelo", r#"<path d="M22 92V50H78V92Z" fill="F"/><path d="M10 92V32H32V92ZM68 92V32H90V92ZM40 50V22H60V50Z" fill="F"/><path d="M10 32V26H15V32M20 32V26H25V32M27 32V26H32V32M68 32V26H73V32M78 32V26H83V32M85 32V26H90V32" fill="none" stroke-width="2.5"/><path d="M50 22V6L62 10 50 14" fill="none"/><path d="M43 92V78A7 7 0 0 1 57 78V92Z" fill="I"/>"#),
    ("torre", "Torre", r#"<path d="M38 94V30H62V94Z" fill="F"/><path d="M33 30 50 6 67 30Z" fill="F"/><path d="M47 44H53V54H47Z" fill="I"/>"#),
    ("cidade", "Cidade", r#"<path d="M6 92V56H22V92M22 92V40H38V92M38 92V64H52V92M52 92V30H66V92M66 92V48H80V92M80 92V60H94V92" fill="F"/><path d="M59 30V18" fill="none"/><path d="M4 92H96" fill="none"/>"#),
    ("vila", "Vila", r#"<g transform="translate(-4 20) scale(.55)"><path d="M24 90V52H76V90Z" fill="F"/><path d="M16 54 50 22 84 54Z" fill="F"/></g><g transform="translate(44 26) scale(.5)"><path d="M24 90V52H76V90Z" fill="F"/><path d="M16 54 50 22 84 54Z" fill="F"/></g><g transform="translate(22 2) scale(.55)"><path d="M24 90V52H76V90Z" fill="F"/><path d="M16 54 50 22 84 54Z" fill="F"/></g>"#),
    ("casa", "Casa", r#"<path d="M24 90V52H76V90Z" fill="F"/><path d="M16 54 50 22 84 54Z" fill="F"/><path d="M44 90V70H56V90Z" fill="I"/>"#),
    ("templo", "Templo", r#"<path d="M14 34 50 12 86 34Z" fill="F"/><path d="M16 34H84V39H16ZM10 84H90V92H10Z" fill="F"/><path d="M22 39V84M38 39V84M62 39V84M78 39V84" fill="none" stroke-width="4"/>"#),
    ("ruina", "Ruína", r#"<path d="M20 90V42M32 90V58M60 90V34M72 90V64" fill="none" stroke-width="5"/><path d="M14 42H34M54 34H76M8 90H92" fill="none"/><path d="M40 90 44 84 50 88 54 82" fill="none" stroke-width="2"/>"#),
    ("caverna", "Caverna", r#"<path d="M8 88C14 36 86 36 92 88Z" fill="F"/><path d="M32 88C34 62 66 62 68 88Z" fill="I"/>"#),
    ("ponte", "Ponte", r#"<path d="M6 48H94M6 56H94" fill="none"/><path d="M6 56V84M94 56V84M18 84C18 62 82 62 82 84" fill="none"/><path d="M20 48V40M40 48V40M60 48V40M80 48V40" fill="none" stroke-width="2"/>"#),
    ("porto", "Porto", r#"<path d="M50 20V86M36 30H64M22 66C24 84 40 90 50 90 60 90 76 84 78 66M16 72 22 62 28 72M72 72 78 62 84 72" fill="none"/><circle cx="50" cy="14" r="6" fill="F"/>"#),
    ("navio", "Navio", r#"<path d="M12 66H88L74 86H26Z" fill="F"/><path d="M50 66V10" fill="none"/><path d="M53 14C76 24 76 54 53 60Z" fill="F"/><path d="M47 22C30 30 30 52 47 58Z" fill="F"/><path d="M50 10 62 6 50 2" fill="none" stroke-width="2"/>"#),
    ("farol", "Farol", r##"<path d="M40 92 44 34H56L60 92Z" fill="F"/><path d="M42 52H58M41 70H59" fill="none" stroke-width="5"/><path d="M42 34V24H58V34Z" fill="#f6d36b"/><path d="M40 24 50 12 60 24Z" fill="F"/><path d="M32 28 16 22M68 28 84 22M32 32 16 36M68 32 84 36" fill="none" stroke-width="2"/>"##),
    ("acampamento", "Acampamento", r#"<path d="M14 86 50 22 86 86Z" fill="F"/><path d="M38 86 50 60 62 86Z" fill="I"/><path d="M50 22V12" fill="none"/>"#),
    ("mina", "Mina", r#"<path d="M26 86 70 30" fill="none" stroke-width="5"/><path d="M48 14C66 16 84 28 88 44 76 34 62 28 50 26Z" fill="F"/>"#),
    ("monstro", "Monstro marinho", r#"<path d="M4 72C14 50 24 50 28 64 32 78 42 78 46 62 50 46 60 46 64 60 68 74 78 74 82 54 84 44 92 40 96 48" fill="none" stroke-width="4"/><circle cx="91" cy="46" r="2" fill="I"/><path d="M4 80C20 76 30 84 44 80 58 76 70 84 96 80" fill="none" stroke-width="1.5"/>"#),
    ("caveira", "Perigo", r#"<path d="M26 46C26 18 74 18 74 46 74 58 66 60 64 66H36C34 60 26 58 26 46Z" fill="F"/><circle cx="40" cy="44" r="7" fill="I"/><circle cx="60" cy="44" r="7" fill="I"/><path d="M40 66V76H60V66M46 66V76M54 66V76" fill="none" stroke-width="2"/><path d="M14 86 86 70M14 70 86 86" fill="none" stroke-width="4"/>"#),
    ("rosa", "Rosa dos ventos", r#"<circle cx="50" cy="50" r="30" fill="none" stroke-width="1.5"/><path d="M50 4 58 42 96 50 58 58 50 96 42 58 4 50 42 42Z" fill="F"/><path d="M50 4 58 42 50 50ZM96 50 58 58 50 50ZM50 96 42 58 50 50ZM4 50 42 42 50 50Z" fill="I"/><path d="M50 22 54 46 78 50 54 54 50 78 46 54 22 50 46 46Z" fill="F" stroke-width="1.5" transform="rotate(45 50 50)"/>"#),
    ("xis", "Tesouro (X)", r##"<path d="M24 24 76 76M76 24 24 76" fill="none" stroke="#8b1e1e" stroke-width="9"/>"##),
];

pub fn stamp(key: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    STAMPS.iter().find(|s| s.0 == key)
}

type Affine = [f64; 6]; // a b c d e f: x' = a x + c y + e, y' = b x + d y + f

fn mul(m: &Affine, n: &Affine) -> Affine {
    [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3], m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5]]
}

fn apply(m: &Affine, p: (f64, f64)) -> (f64, f64) {
    (m[0] * p.0 + m[2] * p.1 + m[4], m[1] * p.0 + m[3] * p.1 + m[5])
}

fn nums(s: &str) -> Vec<f64> {
    let re = regex::Regex::new(r"-?(?:\d+\.?\d*|\.\d+)(?:e-?\d+)?").unwrap();
    re.find_iter(s).filter_map(|m| m.as_str().parse().ok()).collect()
}

fn transform(s: &str) -> Affine {
    let mut m: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let re = regex::Regex::new(r"(\w+)\(([^)]*)\)").unwrap();
    for c in re.captures_iter(s) {
        let v = nums(&c[2]);
        let g = |i: usize, d: f64| v.get(i).copied().unwrap_or(d);
        let t: Affine = match &c[1] {
            "translate" => [1.0, 0.0, 0.0, 1.0, g(0, 0.0), g(1, 0.0)],
            "scale" => [g(0, 1.0), 0.0, 0.0, g(1, g(0, 1.0)), 0.0, 0.0],
            "rotate" => {
                let (a, cx, cy) = (g(0, 0.0).to_radians(), g(1, 0.0), g(2, 0.0));
                let (s, co) = a.sin_cos();
                [co, s, -s, co, cx - co * cx + s * cy, cy - s * cx - co * cy]
            }
            _ => continue,
        };
        m = mul(&m, &t);
    }
    m
}

/// An SVG path's subpaths as polylines (curves and arcs flattened).
pub fn path_points(d: &str) -> Vec<Vec<(f64, f64)>> {
    let re = regex::Regex::new(r"[MmLlHhVvCcSsQqAaZz]|-?(?:\d+\.?\d*|\.\d+)(?:e-?\d+)?").unwrap();
    let toks: Vec<&str> = re.find_iter(d).map(|m| m.as_str()).collect();
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let (mut x, mut y, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
    let mut cmd = 'M';
    let mut i = 0;
    let is_cmd = |t: &str| t.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    let take = |i: &mut usize, n: usize| -> Option<Vec<f64>> {
        let v: Vec<f64> = toks.get(*i..*i + n)?.iter().map(|t| t.parse().ok()).collect::<Option<Vec<f64>>>()?;
        *i += n;
        Some(v)
    };
    while i < toks.len() {
        if is_cmd(toks[i]) {
            cmd = toks[i].chars().next().unwrap();
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                if !cur.is_empty() {
                    cur.push((sx, sy));
                    out.push(std::mem::take(&mut cur));
                }
                x = sx;
                y = sy;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let Some(v) = take(&mut i, 2) else { break };
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                x = ox + v[0];
                y = oy + v[1];
                sx = x;
                sy = y;
                cur.push((x, y));
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let Some(v) = take(&mut i, 2) else { break };
                x = ox + v[0];
                y = oy + v[1];
                cur.push((x, y));
            }
            'H' => {
                let Some(v) = take(&mut i, 1) else { break };
                x = if rel { x + v[0] } else { v[0] };
                cur.push((x, y));
            }
            'V' => {
                let Some(v) = take(&mut i, 1) else { break };
                y = if rel { y + v[0] } else { v[0] };
                cur.push((x, y));
            }
            'C' => {
                let Some(v) = take(&mut i, 6) else { break };
                let (p0, p1, p2, p3) = ((x, y), (ox + v[0], oy + v[1]), (ox + v[2], oy + v[3]), (ox + v[4], oy + v[5]));
                for k in 1..=16 {
                    let t = k as f64 / 16.0;
                    let u = 1.0 - t;
                    cur.push((u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0, u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1));
                }
                x = p3.0;
                y = p3.1;
            }
            'A' => {
                let Some(v) = take(&mut i, 7) else { break };
                let (x2, y2) = (ox + v[5], oy + v[6]);
                arc(&mut cur, (x, y), (x2, y2), v[0].abs(), v[1].abs(), v[2], v[3] != 0.0, v[4] != 0.0);
                x = x2;
                y = y2;
            }
            _ => {
                // S, Q and others are not used by the stamps: skip a number
                i += 1;
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn arc(cur: &mut Vec<(f64, f64)>, p1: (f64, f64), p2: (f64, f64), mut rx: f64, mut ry: f64, rot: f64, large: bool, sweep: bool) {
    if rx == 0.0 || ry == 0.0 {
        cur.push(p2);
        return;
    }
    let (s, c) = rot.to_radians().sin_cos();
    let dx = (p1.0 - p2.0) / 2.0;
    let dy = (p1.1 - p2.1) / 2.0;
    let x1 = c * dx + s * dy;
    let y1 = -s * dx + c * dy;
    let lam = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    if large == sweep {
        k = -k;
    }
    let cx1 = k * rx * y1 / ry;
    let cy1 = -k * ry * x1 / rx;
    let cx = c * cx1 - s * cy1 + (p1.0 + p2.0) / 2.0;
    let cy = s * cx1 + c * cy1 + (p1.1 + p2.1) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f64::consts::TAU;
    }
    for i in 1..=16 {
        let t = t1 + dt * i as f64 / 16.0;
        let (ex, ey) = (rx * t.cos(), ry * t.sin());
        cur.push((c * ex - s * ey + cx, s * ex + c * ey + cy));
    }
}

fn circle(cx: f64, cy: f64, r: f64) -> Vec<(f64, f64)> {
    (0..=40).map(|i| {
        let t = i as f64 / 40.0 * std::f64::consts::TAU;
        (cx + r * t.cos(), cy + r * t.sin())
    }).collect()
}

fn hex(s: &str) -> Option<[f64; 3]> {
    let s = s.trim().trim_start_matches('#');
    let s: String = if s.len() == 3 { s.chars().flat_map(|c| [c, c]).collect() } else { s.to_string() };
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(&s, 16).ok()?;
    Some([((v >> 16) & 255) as f64, ((v >> 8) & 255) as f64, (v & 255) as f64])
}

struct Shape {
    polys: Vec<Vec<(f64, f64)>>,
    fill: Option<[f64; 3]>,
    stroke: Option<[f64; 3]>,
    width: f64,
}

fn shapes(svg: &str, ink: [f64; 3], fill: [f64; 3]) -> Vec<Shape> {
    let tag = regex::Regex::new(r"<(/?)(\w+)([^>]*?)(/?)>").unwrap();
    let attr = regex::Regex::new(r#"([\w-]+)="([^"]*)""#).unwrap();
    let mut stack: Vec<Affine> = vec![[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]];
    let mut out = Vec::new();
    for c in tag.captures_iter(svg) {
        let top = *stack.last().unwrap();
        if &c[1] == "/" {
            if &c[2] == "g" && stack.len() > 1 {
                stack.pop();
            }
            continue;
        }
        let a: std::collections::HashMap<&str, &str> = attr.captures_iter(c.get(3).unwrap().as_str()).map(|m| (m.get(1).unwrap().as_str(), m.get(2).unwrap().as_str())).collect();
        let m = a.get("transform").map(|t| mul(&top, &transform(t))).unwrap_or(top);
        match &c[2] {
            "g" => {
                if c[4].is_empty() {
                    stack.push(m);
                }
                continue;
            }
            "path" | "circle" => {}
            _ => continue,
        }
        let polys = if &c[2] == "path" {
            path_points(a.get("d").copied().unwrap_or(""))
        } else {
            let g = |k: &str| a.get(k).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            vec![circle(g("cx"), g("cy"), g("r"))]
        };
        let scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
        let polys = polys.into_iter().map(|p| p.into_iter().map(|q| apply(&m, q)).collect()).collect();
        let paint = |v: Option<&&str>, d: Option<[f64; 3]>| match v.copied() {
            Some("none") | Some("transparent") => None,
            Some("F") => Some(fill),
            Some("I") => Some(ink),
            Some(h) => hex(h).or(d),
            None => d,
        };
        out.push(Shape {
            polys,
            fill: paint(a.get("fill"), Some([0.0, 0.0, 0.0])),
            stroke: paint(a.get("stroke"), Some(ink)),
            width: a.get("stroke-width").and_then(|v| v.parse().ok()).unwrap_or(3.0) * scale,
        });
    }
    out
}

/// A stamp drawn into a `size`×`size` premultiplied RGBA picture.
pub fn rasterize(key: &str, ink: &str, fill: &str, size: usize) -> Vec<u8> {
    let svg = stamp(key).map(|s| s.2).unwrap_or("");
    let ink = hex(ink).unwrap_or([59.0, 42.0, 26.0]);
    let fill = hex(fill).unwrap_or([239.0, 226.0, 196.0]);
    let mut r = Raster::new(size, size, 3);
    let k = size as f64 / 100.0;
    for sh in shapes(svg, ink, fill) {
        let polys: Vec<Vec<(f64, f64)>> = sh.polys.iter().map(|p| p.iter().map(|q| (q.0 * k, q.1 * k)).collect()).collect();
        if let Some(c) = sh.fill {
            r.fill(&polys, c, 1.0);
        }
        if let Some(c) = sh.stroke {
            r.stroke(&polys, sh.width * k, c, 1.0);
        }
    }
    r.px
}

pub type Edge = ((f64, f64), (f64, f64));

/// A small software painter: shapes filled with the nonzero rule
/// (supersampled), strokes as the union of their segments and round joins,
/// pictures under an affine map. Pixels are premultiplied RGBA.
pub struct Raster {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    ss: usize,
}

impl Raster {
    pub fn new(w: usize, h: usize, ss: usize) -> Raster {
        Raster { w, h, px: vec![0; w * h * 4], ss: ss.max(1) }
    }

    /// Over an opaque picture (straight RGBA).
    pub fn from_rgba(w: usize, h: usize, rgba: Vec<u8>, ss: usize) -> Raster {
        let mut px = rgba;
        for c in px.chunks_mut(4) {
            let a = c[3] as u32;
            for v in c.iter_mut().take(3) {
                *v = ((*v as u32 * a + 127) / 255) as u8;
            }
        }
        Raster { w, h, px, ss: ss.max(1) }
    }

    /// Straight RGBA.
    pub fn into_rgba(self) -> Vec<u8> {
        let mut px = self.px;
        for c in px.chunks_mut(4) {
            let a = c[3] as u32;
            if a > 0 && a < 255 {
                for v in c.iter_mut().take(3) {
                    *v = ((*v as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        px
    }

    fn blend(&mut self, x: usize, y: usize, rgb: [f64; 3], a: f64) {
        let i = (y * self.w + x) * 4;
        let d = &mut self.px[i..i + 4];
        for c in 0..3 {
            d[c] = (rgb[c] * a + d[c] as f64 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
        }
        d[3] = (255.0 * a + d[3] as f64 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    }

    /// Fill the region the edges enclose (nonzero winding).
    pub fn fill_edges(&mut self, edges: &[Edge], rgb: [f64; 3], alpha: f64) {
        if edges.is_empty() || alpha <= 0.0 {
            return;
        }
        let ss = self.ss as f64;
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (a, b) in edges {
            x0 = x0.min(a.0).min(b.0);
            x1 = x1.max(a.0).max(b.0);
            y0 = y0.min(a.1).min(b.1);
            y1 = y1.max(a.1).max(b.1);
        }
        let px0 = (x0.floor().max(0.0)) as usize;
        let px1 = (x1.ceil().min(self.w as f64)) as usize;
        let py0 = (y0.floor().max(0.0)) as usize;
        let py1 = (y1.ceil().min(self.h as f64)) as usize;
        if px0 >= px1 || py0 >= py1 {
            return;
        }
        let bw = px1 - px0;
        let mut cov = vec![0u16; bw];
        let mut xs: Vec<(f64, i32)> = Vec::new();
        let full = (self.ss * self.ss) as f64;
        for py in py0..py1 {
            cov.iter_mut().for_each(|c| *c = 0);
            for k in 0..self.ss {
                let y = py as f64 + (k as f64 + 0.5) / ss;
                xs.clear();
                for (a, b) in edges {
                    if (a.1 <= y && b.1 > y) || (b.1 <= y && a.1 > y) {
                        let t = (y - a.1) / (b.1 - a.1);
                        xs.push((a.0 + t * (b.0 - a.0), if b.1 > a.1 { 1 } else { -1 }));
                    }
                }
                if xs.len() < 2 {
                    continue;
                }
                xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let mut wind = 0;
                for i in 0..xs.len() - 1 {
                    wind += xs[i].1;
                    if wind == 0 {
                        continue;
                    }
                    // samples whose centres fall in [xa, xb)
                    let sa = ((xs[i].0 - px0 as f64) * ss - 0.5).ceil().max(0.0) as i64;
                    let sb = (((xs[i + 1].0 - px0 as f64) * ss - 0.5).ceil() as i64).min((bw * self.ss) as i64);
                    let mut sx = sa;
                    while sx < sb {
                        cov[sx as usize / self.ss] += 1;
                        sx += 1;
                    }
                }
            }
            for (i, c) in cov.iter().enumerate() {
                if *c > 0 {
                    self.blend(px0 + i, py, rgb, *c as f64 / full * alpha);
                }
            }
        }
    }

    pub fn fill(&mut self, polys: &[Vec<(f64, f64)>], rgb: [f64; 3], alpha: f64) {
        let mut e = Vec::new();
        for p in polys {
            if p.len() < 2 {
                continue;
            }
            for w in p.windows(2) {
                e.push((w[0], w[1]));
            }
            e.push((p[p.len() - 1], p[0]));
        }
        self.fill_edges(&e, rgb, alpha);
    }

    /// Lines `width` wide with round joins and caps.
    pub fn stroke(&mut self, polys: &[Vec<(f64, f64)>], width: f64, rgb: [f64; 3], alpha: f64) {
        self.fill_edges(&stroke_edges(polys, width), rgb, alpha);
    }

    /// A premultiplied picture `sw`×`sh`, placed by `m` (picture → raster).
    pub fn image(&mut self, src: &[u8], sw: usize, sh: usize, m: [f64; 6]) {
        let det = m[0] * m[3] - m[1] * m[2];
        if det.abs() < 1e-12 || sw == 0 || sh == 0 {
            return;
        }
        let inv = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det, (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det];
        let corners = [(0.0, 0.0), (sw as f64, 0.0), (0.0, sh as f64), (sw as f64, sh as f64)].map(|q| apply(&m, q));
        let x0 = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
        let x1 = (corners.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max).ceil() as usize).min(self.w);
        let y0 = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
        let y1 = (corners.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max).ceil() as usize).min(self.h);
        let get = |x: i64, y: i64, c: usize| -> f64 {
            if x < 0 || y < 0 || x >= sw as i64 || y >= sh as i64 {
                0.0
            } else {
                src[(y as usize * sw + x as usize) * 4 + c] as f64
            }
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let (u, v) = apply(&inv, (x as f64 + 0.5, y as f64 + 0.5));
                let (u, v) = (u - 0.5, v - 0.5);
                if u < -1.0 || v < -1.0 || u > sw as f64 || v > sh as f64 {
                    continue;
                }
                let (fx, fy) = (u.floor(), v.floor());
                let (tx, ty) = (u - fx, v - fy);
                let (ix, iy) = (fx as i64, fy as i64);
                let mut s = [0.0; 4];
                for (c, sc) in s.iter_mut().enumerate() {
                    let a = get(ix, iy, c) * (1.0 - tx) + get(ix + 1, iy, c) * tx;
                    let b = get(ix, iy + 1, c) * (1.0 - tx) + get(ix + 1, iy + 1, c) * tx;
                    *sc = a * (1.0 - ty) + b * ty;
                }
                if s[3] <= 0.0 {
                    continue;
                }
                let k = 1.0 - s[3] / 255.0;
                let i = (y * self.w + x) * 4;
                for c in 0..4 {
                    self.px[i + c] = (s[c] + self.px[i + c] as f64 * k).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// The outline of thick lines, as edges whose pieces all wind the same way.
pub fn stroke_edges(polys: &[Vec<(f64, f64)>], width: f64) -> Vec<Edge> {
    let hw = width / 2.0;
    let mut e: Vec<Edge> = Vec::new();
    let mut push = |pts: Vec<(f64, f64)>| {
        let area: f64 = (0..pts.len()).map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a.0 * b.1 - b.0 * a.1
        }).sum();
        let mut pts = pts;
        if area < 0.0 {
            pts.reverse();
        }
        for i in 0..pts.len() {
            e.push((pts[i], pts[(i + 1) % pts.len()]));
        }
    };
    let n = ((hw * 2.0).ceil() as usize).clamp(8, 32);
    for p in polys {
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = dx.hypot(dy);
            if l < 1e-9 {
                continue;
            }
            let (nx, ny) = (-dy / l * hw, dx / l * hw);
            push(vec![(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)]);
        }
        for q in p {
            push((0..n).map(|i| {
                let t = i as f64 / n as f64 * std::f64::consts::TAU;
                (q.0 + hw * t.cos(), q.1 + hw * t.sin())
            }).collect());
        }
    }
    e
}

/// A line cut into dashes.
pub fn dashes(p: &[(f64, f64)], on: f64, off: f64) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let (mut drawing, mut left) = (true, on);
    for w in p.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut l = (b.0 - a.0).hypot(b.1 - a.1);
        while l > 1e-9 {
            let step = left.min(l);
            let t = step / l;
            let m = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if drawing {
                if cur.is_empty() {
                    cur.push(a);
                }
                cur.push(m);
            }
            a = m;
            l -= step;
            left -= step;
            if left <= 1e-9 {
                if drawing && cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                drawing = !drawing;
                left = if drawing { on } else { off };
            }
        }
    }
    if drawing && cur.len() > 1 {
        out.push(cur);
    }
    out
}

pub fn color(hexs: &str) -> Option<[f64; 3]> {
    hex(hexs)
}

pub fn rotate_about(deg: f64, cx: f64, cy: f64) -> [f64; 6] {
    transform(&format!("rotate({deg} {cx} {cy})"))
}

pub fn compose(m: &[f64; 6], n: &[f64; 6]) -> [f64; 6] {
    mul(m, n)
}

/// Triangles over a simple polygon (ear clipping), as indices.
pub fn triangulate(pts: &[(f64, f64)]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f64 = (0..n).map(|i| {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        a.0 * b.1 - b.0 * a.1
    }).sum();
    let mut idx: Vec<usize> = (0..n).collect();
    if area < 0.0 {
        idx.reverse();
    }
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut cut = None;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            if cross(a, b, c) <= 0.0 {
                continue;
            }
            let inside = idx.iter().any(|&j| j != ia && j != ib && j != ic && cross(a, b, pts[j]) >= 0.0 && cross(b, c, pts[j]) >= 0.0 && cross(c, a, pts[j]) >= 0.0);
            if !inside {
                cut = Some(i);
                out.push([ia, ib, ic]);
                break;
            }
        }
        match cut {
            Some(i) => {
                idx.remove(i);
            }
            None => {
                // not simple: close it with a fan
                for k in 1..idx.len() - 1 {
                    out.push([idx[0], idx[k], idx[k + 1]]);
                }
                return out;
            }
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_and_sphere() {
        let flat = json!({ "proj": "plana", "kmLargura": 1000 });
        let d = dist_km(&flat, 2000.0, 1000.0, [0.0, 0.0], [50.0, 0.0]).unwrap();
        assert!((d - 500.0).abs() < 1e-6);
        let sph = json!({ "proj": "mollweide", "raio": 6371, "elipse": [0, 0, 100, 100] });
        // centre to the right edge of the equator: half the circumference… of longitude π
        let d = dist_km(&sph, 2000.0, 1000.0, [50.0, 50.0], [100.0, 50.0]).unwrap();
        assert!((d - std::f64::consts::PI * 6371.0).abs() < 1.0, "{d}");
        assert!(dist_km(&sph, 2000.0, 1000.0, [0.0, 0.0], [50.0, 50.0]).is_none());
        assert!(dist_km(&Json::Null, 1.0, 1.0, [0.0, 0.0], [1.0, 1.0]).is_none());
    }

    #[test]
    fn travel_text() {
        let (km, t) = travel(60.0, &modes(None));
        assert_eq!(km, "60 km");
        assert_eq!(t[0], "a pé 2 dias");
        assert_eq!(t[3], "navio a vela 9,6 h");
        let (km, _) = travel(1234.4, &modes(None));
        assert_eq!(km, "1.234 km");
    }

    #[test]
    fn time() {
        let l = json!({ "de": 100, "ate": 200 });
        assert!(visible(&l, None));
        assert!(visible(&l, Some(150.0)));
        assert!(!visible(&l, Some(50.0)));
        assert!(!visible(&l, Some(250.0)));
        let ts = TimeScale::new(&[l], &[json!({ "de": 0, "ate": 100, "nome": "A" }), json!({ "de": 100, "ate": 10000, "nome": "B" })]).unwrap();
        assert_eq!(ts.segs.len(), 2);
        let p = ts.to_pos(100.0);
        assert!((p - 10.0 / (10.0 + 9900f64.sqrt())).abs() < 1e-9);
        assert_eq!(ts.from_pos(p), 100.0);
        assert_eq!(ts.from_pos(1.0), 10000.0);
    }

    #[test]
    fn ears() {
        // an L: 6 corners, 4 triangles
        let l = [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)];
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        let area: f64 = t.iter().map(|[a, b, c]| {
            let (a, b, c) = (l[*a], l[*b], l[*c]);
            ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() / 2.0
        }).sum();
        assert!((area - 3.0).abs() < 1e-9);
    }

    #[test]
    fn stamps_draw() {
        for s in STAMPS.iter() {
            let px = rasterize(s.0, STAMP_INK, STAMP_FILL, 48);
            assert_eq!(px.len(), 48 * 48 * 4);
            assert!(px.chunks(4).filter(|p| p[3] > 0).count() > 40, "{} is empty", s.0);
        }
        // the tree's crown is filled with the paper colour
        let px = rasterize("arvore", "#000000", "#ff0000", 100);
        let c = &px[(30 * 100 + 50) * 4..(30 * 100 + 50) * 4 + 4];
        assert_eq!(c, &[255, 0, 0, 255]);
    }

    #[test]
    fn raster_shapes() {
        // a 10×10 square, half covered by a 5-wide stroke along its middle
        let mut r = Raster::new(20, 20, 2);
        r.fill(&[vec![(5.0, 5.0), (15.0, 5.0), (15.0, 15.0), (5.0, 15.0)]], [255.0, 0.0, 0.0], 1.0);
        assert_eq!(&r.px[(10 * 20 + 10) * 4..(10 * 20 + 10) * 4 + 4], &[255, 0, 0, 255]);
        assert_eq!(r.px[(2 * 20 + 2) * 4 + 3], 0);
        let mut r = Raster::new(20, 20, 2);
        r.stroke(&[vec![(2.0, 10.0), (10.0, 10.0), (18.0, 10.0)]], 4.0, [0.0, 0.0, 255.0], 1.0);
        assert_eq!(r.px[(10 * 20 + 10) * 4 + 3], 255);
        assert_eq!(r.px[(5 * 20 + 10) * 4 + 3], 0);
        // overlapping pieces of one stroke do not darken twice
        let mut r = Raster::new(20, 20, 2);
        r.stroke(&[vec![(2.0, 10.0), (10.0, 10.0), (18.0, 10.0)]], 4.0, [0.0, 0.0, 255.0], 0.5);
        assert_eq!(r.px[(10 * 20 + 10) * 4 + 3], 128);
        let d = dashes(&[(0.0, 0.0), (10.0, 0.0)], 2.0, 1.0);
        assert_eq!(d.len(), 4);
    }
}
