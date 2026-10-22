//! A map as a PNG: the picture with its layers (those of the year shown)
//! painted in software, labels in Cormorant like on screen.

use ab_glyph::{Font, FontRef, OutlineCurve};
use ateliet_core::scriptorium::book;
use ateliet_core::scriptorium::enredo::str_of;
use ateliet_core::scriptorium::mapa::{self as ma, Raster};
use serde_json::Value as Json;
use std::path::Path;

const MAX: u32 = 4096;

/// Paint `d` over its picture (`base`, a vault path) and write
/// `<note> (exportado).png` beside the note; returns its path.
pub fn export(root: &Path, note: &str, base: Option<&str>, d: &Json, iw: f64, ih: f64) -> Result<String, String> {
    let (w, h, rgba) = match base.map(|b| image::open(root.join(b))) {
        Some(Ok(im)) => {
            let im = if im.width() > MAX || im.height() > MAX { im.resize(MAX, MAX, image::imageops::FilterType::CatmullRom) } else { im };
            let rgba = im.to_rgba8();
            (rgba.width() as usize, rgba.height() as usize, rgba.into_raw())
        }
        Some(Err(e)) => return Err(e.to_string()),
        None => {
            let k = (MAX as f64 / iw.max(ih)).min(1.0);
            let (w, h) = ((iw * k).round().max(1.0) as usize, (ih * k).round().max(1.0) as usize);
            let paper = ma::color(ma::STAMP_FILL).unwrap_or([239.0, 226.0, 196.0]);
            (w, h, (0..w * h).flat_map(|_| [paper[0] as u8, paper[1] as u8, paper[2] as u8, 255]).collect())
        }
    };
    let mut r = Raster::from_rgba(w, h, rgba, 2);
    let s = w as f64 / 100.0;
    let at = |q: [f64; 2]| (q[0] / 100.0 * w as f64, q[1] / 100.0 * h as f64);
    let ano = d["ano"].as_f64();
    let fonts = Fonts::new()?;
    for l in d["camadas"].as_array().into_iter().flatten() {
        if !ma::visible(l, ano) {
            continue;
        }
        let col = |dflt: &str| ma::color(l["cor"].as_str().filter(|c| !c.is_empty()).unwrap_or(dflt)).unwrap_or([0.0, 0.0, 0.0]);
        let pts: Vec<(f64, f64)> = l["pts"].as_array().map(|a| a.iter().map(|p| at([p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)])).collect()).unwrap_or_default();
        let xy = at([l["x"].as_f64().unwrap_or(0.0), l["y"].as_f64().unwrap_or(0.0)]);
        match str_of(l, "t") {
            "area" if pts.len() >= 3 => {
                let c = col(ma::PIN);
                r.fill(std::slice::from_ref(&pts), c, l["opac"].as_f64().unwrap_or(0.18));
                let mut ring = pts.clone();
                ring.push(pts[0]);
                r.stroke(&ma::dashes(&ring, s * 0.8, s * 0.5), s * 0.18, c, 1.0);
            }
            "rota" if pts.len() >= 2 => {
                let c = col(ma::PIN);
                let wd = s * l["espessura"].as_f64().unwrap_or(0.3);
                let lines = if l["tracejado"] == false { vec![pts] } else { ma::dashes(&pts, s * 0.9, s * 0.6) };
                r.stroke(&lines, wd, c, 1.0);
            }
            "carimbo" => {
                let sw = l["w"].as_f64().unwrap_or(4.0) * s;
                let key = str_of(l, "stamp");
                let (px, pw, ph) = if ma::stamp(key).is_some() {
                    let n = (sw.ceil() as usize).clamp(8, 1024);
                    (ma::rasterize(key, l["cor"].as_str().unwrap_or(ma::STAMP_INK), l["fundo"].as_str().unwrap_or(ma::STAMP_FILL), n), n, n)
                } else {
                    match image::open(root.join(key)) {
                        Ok(im) => {
                            let im = im.to_rgba8();
                            let (a, b) = (im.width() as usize, im.height() as usize);
                            let mut raw = im.into_raw();
                            for c in raw.chunks_mut(4) {
                                let al = c[3] as u32;
                                for v in c.iter_mut().take(3) {
                                    *v = ((*v as u32 * al + 127) / 255) as u8;
                                }
                            }
                            (raw, a, b)
                        }
                        Err(_) => continue,
                    }
                };
                // fit the drawing in the stamp's square, then turn it about its centre
                let k = sw / pw.max(ph) as f64;
                let (ox, oy) = (xy.0 - pw as f64 * k / 2.0, xy.1 - ph as f64 * k / 2.0);
                let place = [k, 0.0, 0.0, k, ox, oy];
                let m = ma::compose(&ma::rotate_about(l["rot"].as_f64().unwrap_or(0.0), xy.0, xy.1), &place);
                r.image(&px, pw, ph, m);
            }
            "rotulo" => {
                let fs = l["tam"].as_f64().unwrap_or(1.6) * s;
                let font = if l["ital"] == false { &fonts.regular } else { &fonts.italic };
                let halo = ma::color(l["halo"].as_str().unwrap_or("#f3e6c8")).unwrap_or([243.0, 230.0, 200.0]);
                let glyphs = text(font, str_of(l, "texto"), fs, l["espaco"].as_f64().unwrap_or(0.12), xy, l["rot"].as_f64().unwrap_or(0.0));
                r.stroke(&glyphs, fs * 0.18, halo, 1.0);
                r.fill_edges(&edges(&glyphs), col("#3b2a1a"), 1.0);
            }
            "pino" => {
                let k = l["tam"].as_f64().unwrap_or(1.4) * s / 20.0;
                let drop: Vec<(f64, f64)> = ma::path_points("M0 0C-6-10-12-14-12-22A12 12 0 1 1 12-22C12-14 6-10 0 0Z").into_iter().flatten().map(|q| (xy.0 + q.0 * k, xy.1 + q.1 * k)).collect();
                r.fill(std::slice::from_ref(&drop), col(ma::PIN), 1.0);
                r.stroke(std::slice::from_ref(&drop), 2.0 * k, [255.0, 255.0, 255.0], 1.0);
                let dot: Vec<(f64, f64)> = (0..24).map(|i| {
                    let t = i as f64 / 24.0 * std::f64::consts::TAU;
                    (xy.0 + 4.5 * k * t.cos(), xy.1 - 22.0 * k + 4.5 * k * t.sin())
                }).collect();
                r.fill(&[dot], [255.0, 255.0, 255.0], 1.0);
                let label = str_of(l, "rotulo");
                if !label.is_empty() {
                    let glyphs = text(&fonts.bold, label, 14.0 * k, 0.0, (xy.0, xy.1 + 16.0 * k), 0.0);
                    r.stroke(&glyphs, 4.0 * k, [243.0, 230.0, 200.0], 1.0);
                    r.fill_edges(&edges(&glyphs), [42.0, 29.0, 18.0], 1.0);
                }
            }
            _ => {}
        }
    }
    let out = r.into_rgba();
    let folder = note.rsplit_once('/').map(|x| x.0).unwrap_or("");
    let name = super::mapa_ui::name_of(note);
    let rel = book::unique(root, &format!("{}{name} (exportado).png", if folder.is_empty() { String::new() } else { format!("{folder}/") }));
    image::RgbaImage::from_raw(w as u32, h as u32, out).ok_or("imagem inválida")?.save(root.join(&rel)).map_err(|e| e.to_string())?;
    Ok(rel)
}

struct Fonts {
    regular: FontRef<'static>,
    italic: FontRef<'static>,
    bold: FontRef<'static>,
}

impl Fonts {
    fn new() -> Result<Fonts, String> {
        let f = |b: &'static [u8]| FontRef::try_from_slice(b).map_err(|e| e.to_string());
        Ok(Fonts { regular: f(crate::fonts::CORMORANT)?, italic: f(crate::fonts::CORMORANT_ITALIC)?, bold: f(crate::fonts::CORMORANT_BOLD)? })
    }
}

/// A line of text as polylines (one per flattened curve): centred on
/// `anchor` (its baseline), `size` px to the em, turned `deg` degrees.
fn text(font: &FontRef, t: &str, size: f64, spacing: f64, anchor: (f64, f64), deg: f64) -> Vec<Vec<(f64, f64)>> {
    let upm = font.units_per_em().unwrap_or(1000.0) as f64;
    let k = size / upm;
    let mut pen = 0.0;
    let mut placed: Vec<(ab_glyph::GlyphId, f64)> = Vec::new();
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in t.chars() {
        let g = font.glyph_id(ch);
        if let Some(p) = prev {
            pen += font.kern_unscaled(p, g) as f64 * k;
        }
        placed.push((g, pen));
        pen += font.h_advance_unscaled(g) as f64 * k + size * spacing;
        prev = Some(g);
    }
    let width = pen - if placed.is_empty() { 0.0 } else { size * spacing };
    let (sn, cs) = deg.to_radians().sin_cos();
    let place = |x: f64, y: f64| {
        let (x, y) = (x - width / 2.0, y);
        (anchor.0 + x * cs - y * sn, anchor.1 + x * sn + y * cs)
    };
    let mut out = Vec::new();
    for (g, x0) in placed {
        let Some(o) = font.outline(g) else { continue };
        let pt = |p: ab_glyph::Point| place(x0 + p.x as f64 * k, -(p.y as f64) * k);
        for c in o.curves {
            let poly: Vec<(f64, f64)> = match c {
                OutlineCurve::Line(a, b) => vec![pt(a), pt(b)],
                OutlineCurve::Quad(a, b, c) => (0..=8).map(|i| {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    pt(ab_glyph::point(u * u * a.x + 2.0 * u * t * b.x + t * t * c.x, u * u * a.y + 2.0 * u * t * b.y + t * t * c.y))
                }).collect(),
                OutlineCurve::Cubic(a, b, c, d) => (0..=12).map(|i| {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    let f = |p0: f32, p1: f32, p2: f32, p3: f32| u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3;
                    pt(ab_glyph::point(f(a.x, b.x, c.x, d.x), f(a.y, b.y, c.y, d.y)))
                }).collect(),
            };
            out.push(poly);
        }
    }
    out
}

/// The pieces of glyph outlines as edges (they already wind as the font says).
fn edges(polys: &[Vec<(f64, f64)>]) -> Vec<ma::Edge> {
    polys.iter().flat_map(|p| p.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>()).collect()
}
