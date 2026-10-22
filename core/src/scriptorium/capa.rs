//! The open cover of a book: a note `Capa.md` in its folder whose JSON
//! block (`scriptorium-capa`) holds a composed model (back, spine, front,
//! flaps); its PDF with bleed and a picture of the front.

use super::book::{self, Book};
use super::{read_json_block, write_json_block};
use crate::dataview::index::Index;
use crate::typeset::compose::{self, Env};
use crate::typeset::fonts::FontDb;
use crate::typeset::model::Pics;
use crate::typeset::{pdf, raster, Item, Page, PageKind, Picture};
use serde_json::Value as Json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const LANG: &str = "scriptorium-capa";

pub fn path(b: &Book) -> String {
    format!("{}/Capa.md", b.folder)
}

/// The saved cover, if there is one.
pub fn load(root: &Path, b: &Book) -> Option<Json> {
    let t = std::fs::read_to_string(root.join(path(b))).ok()?;
    read_json_block(&t, LANG).filter(|m| m["camadas"].is_array())
}

/// The saved cover, or the one a book starts with (its cover art on the
/// front, its back-cover text or synopsis on the back).
pub fn load_or_default(root: &Path, b: &Book) -> Json {
    if let Some(m) = load(root, b) {
        return m;
    }
    let text = std::fs::read_to_string(root.join(&b.path)).unwrap_or_default();
    let back = Some(super::body_section(&text, "Contracapa")).filter(|s| !s.trim().is_empty() && !s.starts_with("Texto da contracapa")).unwrap_or_else(|| super::body_section(&text, "Sinopse"));
    let mut m = compose::default_cover(&b.title, &b.author, &b.color, back.trim());
    if let Some(art) = b.fm["capa"].as_str().filter(|s| !s.is_empty()) {
        m["camadas"][0]["img"] = Json::String(link_text(art));
    }
    m
}

/// `[[Artes/x.png|…]]` → `Artes/x.png`.
pub fn link_text(s: &str) -> String {
    let s = s.trim().trim_start_matches("![[").trim_start_matches("[[").trim_end_matches("]]");
    s.split('|').next().unwrap_or(s).trim().to_string()
}

pub fn save(root: &Path, ix: &Index, b: &Book, model: &Json) -> Result<String, String> {
    let p = path(b);
    let full = root.join(&p);
    let text = match std::fs::read_to_string(&full) {
        Ok(t) => t,
        Err(_) => format!("---\nscriptorium: capa\nlivro: \"{}\"\n---\n\nCapa aberta de **{}**. Edite pela visão Capa do Scriptorium.\n", book::note_link(ix, &b.path, &p), b.title),
    };
    book::write(root, &p, &write_json_block(&text, LANG, model))?;
    Ok(p)
}

/// Pages a book will have, from its words, until it is typeset.
pub fn estimate_pages(words: i64, cfg: &Json) -> usize {
    let f = |k: &str, d: f64| cfg[k].as_f64().unwrap_or(d);
    let per = (260.0 * (f("largura", 140.0) * f("altura", 210.0)) / (140.0 * 210.0) * (10.5 / f("corpo", 10.5)) * (1.45 / f("entrelinha", 1.45))).round().max(120.0);
    ((words as f64 / per).round() as usize + 14).max(24)
}

/// The pictures a model uses, read (and filtered) for drawing.
pub struct CoverPics {
    pub pics: Pics,
    pub found: HashMap<(String, String), (usize, u32, u32)>,
}

impl CoverPics {
    pub fn load(root: &Path, ix: &Index, from: &str, model: &Json) -> CoverPics {
        let mut pics = Pics::default();
        let mut found = HashMap::new();
        for (img, filt) in compose::images(model) {
            let p = crate::scriptorium::export::resolve_image(ix, &link_text(&img), from).or_else(|| root.join(&img).is_file().then(|| img.clone()));
            if let Some(i) = p.and_then(|p| pics.filtered(root, &p, &filt)) {
                let pc = &pics.list[i];
                found.insert((img, filt), (i, pc.w, pc.h));
            }
        }
        CoverPics { pics, found }
    }

    pub fn get(&self, img: &str, filt: &str) -> Option<(usize, u32, u32)> {
        self.found.get(&(img.to_string(), filt.to_string())).copied()
    }
}

/// The whole open cover as one page with its bleed.
pub fn page(model: &Json, env: &Env, db: &FontDb, pics: &CoverPics) -> Page {
    let b = env.bleed;
    let look = |p: &str, f: &str| pics.get(p, f);
    let items = compose::render(model, env, db, &look);
    let mut pg = Page::new(env.w + 2.0 * b, env.h + 2.0 * b, 1, PageKind::Fixed, [255, 255, 255]);
    pg.trim = (b, b, env.w, env.h);
    pg.items = items.iter().map(|it| crate::typeset::layout::shift(it, b, b)).collect();
    pg
}

/// One zone (the front, the spine…) as a page of its own, without bleed.
pub fn zone_page(model: &Json, env: &Env, db: &FontDb, pics: &CoverPics, zone: &str) -> Page {
    let z = env.zone(zone);
    let look = |p: &str, f: &str| pics.get(p, f);
    let items = compose::render(model, env, db, &look);
    let mut pg = Page::new(z.w, z.h, 1, PageKind::Fixed, [255, 255, 255]);
    pg.items = vec![Item::Group { m: [1.0, 0.0, 0.0, 1.0, -z.x, -z.y], clip: Some(vec![vec![(z.x, z.y), (z.x + z.w, z.y), (z.x + z.w, z.y + z.h), (z.x, z.y + z.h)]]), alpha: 1.0, items }];
    pg
}

fn write_out(root: &Path, rel: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let abs = root.join(rel);
    if let Some(d) = abs.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&abs, bytes).map_err(|e| e.to_string())?;
    Ok(abs)
}

/// "<livro> — capa aberta.pdf" in `Exportações/`.
pub fn cover_pdf(root: &Path, ix: &Index, b: &Book, model: &Json, db: &FontDb, pages: usize) -> Result<PathBuf, String> {
    let cfg = book::layout(b);
    let env = compose::cover_env(model, &cfg, pages);
    let pics = CoverPics::load(root, ix, &b.path, model);
    let pg = page(model, &env, db, &pics);
    let bytes = pdf::write(&[pg], db, &pics.pics.list, &pdf::Meta { title: format!("{} — capa", b.title), author: b.author.clone(), lang: "pt-BR".into() })?;
    write_out(root, &crate::scriptorium::export::out_path(b, "capa aberta", "pdf"), &bytes)
}

/// "<livro> — capa.png": the front, `px` wide.
pub fn front_png(root: &Path, ix: &Index, b: &Book, model: &Json, db: &FontDb, pages: usize, px: u32) -> Result<PathBuf, String> {
    let cfg = book::layout(b);
    let env = compose::cover_env(model, &cfg, pages);
    let pics = CoverPics::load(root, ix, &b.path, model);
    let pg = zone_page(model, &env, db, &pics, "capa");
    let (w, h, rgba) = raster::render(&pg, db, &pics.pics.list, px as f32 / pg.w);
    let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("imagem inválida")?;
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    write_out(root, &crate::scriptorium::export::out_path(b, "capa", "png"), &bytes)
}

/// The front as straight RGBA, `px` wide (EPUB covers, the library).
pub fn front_rgba(root: &Path, ix: &Index, b: &Book, model: &Json, db: &FontDb, pages: usize, px: u32) -> Picture {
    let cfg = book::layout(b);
    let env = compose::cover_env(model, &cfg, pages);
    let pics = CoverPics::load(root, ix, &b.path, model);
    let pg = zone_page(model, &env, db, &pics, "capa");
    let (w, h, rgba) = raster::render(&pg, db, &pics.pics.list, px as f32 / pg.w);
    Picture { w, h, rgba, jpeg: None }
}
