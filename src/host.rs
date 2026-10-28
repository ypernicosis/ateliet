//! What the renderer asks of the app, with caches: pictures as textures,
//! query results per index generation, parsed embedded notes.

use crate::markdown::{self, Doc, Host};
use crate::theme::Palette;
use crate::vault::Vault;
use ateliet_core::dataview::{query, Settings};
use ateliet_core::script;
use eframe::egui;
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Default)]
pub struct Caches {
    pub images: HashMap<String, Option<(egui::TextureHandle, egui::Vec2)>>,
    /// key → (index generation, result)
    pub queries: HashMap<u64, (u64, Value)>,
    pub docs: HashMap<String, (std::time::SystemTime, Arc<Doc>)>,
    /// Composed pages drawn for reading: key of (note, model) → picture.
    pub composed: HashMap<u64, Composed>,
    /// Quadros embedded in notes: path → (modified, board); their drawn
    /// geometry and pictures kept in the file itself.
    pub quadros: HashMap<String, (std::time::SystemTime, Arc<ateliet_core::quadro::Board>)>,
    pub quadro_geo: HashMap<String, (u64, crate::quadro_ui::Geo)>,
    pub quadro_data: HashMap<String, Option<(egui::TextureHandle, egui::Vec2)>>,
    /// Whether an embedded note is a quadro: path → (modified, answer).
    pub quadro_kind: HashMap<String, (std::time::SystemTime, bool)>,
}

pub enum Composed {
    Pending(std::sync::mpsc::Receiver<Option<egui::ColorImage>>),
    Ready(egui::TextureHandle, egui::Vec2),
    Failed,
}

impl Caches {
    pub fn clear_queries(&mut self) {
        self.queries.clear();
    }
}

pub struct RenderHost<'a> {
    pub vault: &'a Vault,
    pub palette: Palette,
    pub size: f32,
    pub source: &'a str,
    pub caches: &'a mut Caches,
    pub settings: &'a Settings,
}

fn key(parts: &[&str]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

impl RenderHost<'_> {
    fn generation(&self) -> u64 {
        self.vault.index.read().map(|ix| ix.generation).unwrap_or(0)
    }

    fn cached(&mut self, k: u64, run: impl FnOnce(&Self) -> Value) -> Value {
        let generation = self.generation();
        if let Some((g, v)) = self.caches.queries.get(&k) {
            if *g == generation {
                return v.clone();
            }
        }
        let v = run(self);
        self.caches.queries.insert(k, (generation, v.clone()));
        v
    }

    fn script_host(&self) -> script::Host {
        script::Host { index: self.vault.index.clone(), settings: self.settings.clone(), root: self.vault.root.clone(), prompt: None, log: None, timeout_secs: 10 }
    }
}

/// A picture of the vault as a texture (cached; large ones scaled down).
pub fn image(ctx: &egui::Context, vault: &Vault, caches: &mut Caches, target: &str, source: &str) -> Option<(egui::TextureId, egui::Vec2)> {
    let path = vault.resolve(target, source)?;
    if let Some(cached) = caches.images.get(&path) {
        return cached.as_ref().map(|(t, s)| (t.id(), *s));
    }
    let loaded = image::open(vault.abs(&path)).ok().map(|img| {
        let img = if img.width() > 1600 || img.height() > 1600 { img.thumbnail(1600, 1600) } else { img };
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let tex = ctx.load_texture(path.clone(), color, egui::TextureOptions::LINEAR);
        (tex, egui::vec2(size[0] as f32, size[1] as f32))
    });
    let out = loaded.as_ref().map(|(t, s)| (t.id(), *s));
    caches.images.insert(path, loaded);
    out
}

/// The front of a book's composed cover (`Capa.md`), drawn in the
/// background; None while it is drawn or when the book has none.
pub fn cover_front(ctx: &egui::Context, vault: &Vault, caches: &mut Caches, b: &ateliet_core::scriptorium::book::Book) -> Option<(egui::TextureId, egui::Vec2)> {
    use ateliet_core::scriptorium::capa;
    let rel = capa::path(b);
    let mtime = std::fs::metadata(vault.abs(&rel)).and_then(|m| m.modified()).ok()?;
    let k = key(&["capa", &b.path, &format!("{mtime:?}"), &b.fm["capa"].to_string()]);
    if !caches.composed.contains_key(&k) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (root, index, b) = (vault.root.clone(), vault.index.clone(), b.clone());
        std::thread::spawn(move || {
            let out = (|| {
                let m = capa::load(&root, &b)?;
                let ix = index.read().ok()?;
                let db = crate::fonts::book_fonts();
                let pic = capa::front_rgba(&root, &ix, &b, &m, &db, 200, 420);
                Some(egui::ColorImage::from_rgba_unmultiplied([pic.w as usize, pic.h as usize], &pic.rgba))
            })();
            let _ = tx.send(out);
        });
        caches.composed.insert(k, Composed::Pending(rx));
    }
    let slot = caches.composed.get_mut(&k)?;
    if let Composed::Pending(rx) = slot {
        match rx.try_recv() {
            Ok(Some(img)) => {
                let size = egui::vec2(img.size[0] as f32, img.size[1] as f32);
                *slot = Composed::Ready(ctx.load_texture(format!("capa-{k}"), img, egui::TextureOptions::LINEAR), size);
            }
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => *slot = Composed::Failed,
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(80)),
        }
    }
    match slot {
        Composed::Ready(t, s) => Some((t.id(), *s)),
        _ => None,
    }
}

impl Host for RenderHost<'_> {
    fn palette(&self) -> Palette {
        self.palette
    }

    fn size(&self) -> f32 {
        self.size
    }

    fn source(&self) -> &str {
        self.source
    }

    fn resolved(&self, target: &str) -> bool {
        let t = target.split('#').next().unwrap_or(target);
        t.is_empty() || self.vault.resolve(t, self.source).is_some_and(|p| self.vault.abs(&p).exists())
    }

    fn image(&mut self, ctx: &egui::Context, target: &str) -> Option<(egui::TextureId, egui::Vec2)> {
        image(ctx, self.vault, self.caches, target, self.source)
    }

    fn abs_path(&self, target: &str) -> Option<std::path::PathBuf> {
        let p = self.vault.abs(&self.vault.resolve(target, self.source)?);
        p.exists().then_some(p)
    }

    fn query(&mut self, lang: &str, source: &str) -> Value {
        let k = key(&[lang, source, self.source]);
        let origin = self.source.to_string();
        let lang = lang.to_string();
        let src = source.to_string();
        self.cached(k, move |h| {
            if lang == "dataviewjs" {
                script::run_dataviewjs(&h.script_host(), &src, &origin)
            } else {
                let ix = h.vault.index.read().unwrap();
                query::run(&ix, h.settings, &origin, &src)
            }
        })
    }

    fn inline_query(&mut self, js: bool, expr: &str) -> Value {
        let k = key(&[if js { "$=" } else { "=" }, expr, self.source]);
        let origin = self.source.to_string();
        let e = expr.to_string();
        self.cached(k, move |h| {
            if js {
                script::run_inline_js(&h.script_host(), &e, &origin)
            } else {
                let ix = h.vault.index.read().unwrap();
                query::inline(&ix, h.settings, &origin, &e)
            }
        })
    }

    fn composed(&mut self, ctx: &egui::Context, model: &str) -> Option<(egui::TextureId, egui::Vec2)> {
        let k = key(&["pagina", model, self.source]);
        if !self.caches.composed.contains_key(&k) {
            let (tx, rx) = std::sync::mpsc::channel();
            let (root, index, source, model) = (self.vault.root.clone(), self.vault.index.clone(), self.source.to_string(), model.to_string());
            std::thread::spawn(move || {
                let out = (|| {
                    use ateliet_core::scriptorium::{book, capa};
                    use ateliet_core::typeset::{compose, raster};
                    let m: Value = serde_json::from_str(&model).ok()?;
                    let ix = index.read().ok()?;
                    let cfg = book::book_for(&ix, &source, "").map(|b| book::layout(&b)).unwrap_or_else(|| book::DEFAULT_LAYOUT.clone());
                    let env = compose::page_env(&m, &cfg, false);
                    let pics = capa::CoverPics::load(&root, &ix, &source, &m);
                    drop(ix);
                    let db = crate::fonts::book_fonts();
                    let page = capa::page(&m, &env, &db, &pics);
                    let (w, h, px) = raster::render(&page, &db, &pics.pics.list, 900.0 / page.w.max(page.h));
                    Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px))
                })();
                let _ = tx.send(out);
            });
            self.caches.composed.insert(k, Composed::Pending(rx));
        }
        let slot = self.caches.composed.get_mut(&k)?;
        if let Composed::Pending(rx) = slot {
            match rx.try_recv() {
                Ok(Some(img)) => {
                    let size = egui::vec2(img.size[0] as f32, img.size[1] as f32);
                    *slot = Composed::Ready(ctx.load_texture(format!("pagina-{k}"), img, egui::TextureOptions::LINEAR), size);
                }
                Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => *slot = Composed::Failed,
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(80)),
            }
        }
        match slot {
            Composed::Ready(t, s) => Some((t.id(), *s)),
            _ => None,
        }
    }

    fn quadro(&mut self, ui: &mut egui::Ui, target: &str) -> Option<(String, egui::Response)> {
        let path = self.vault.resolve(target, self.source)?;
        let abs = self.vault.abs(&path);
        let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok()?;
        let is = match self.caches.quadro_kind.get(&path) {
            Some((t, b)) if *t == mtime => *b,
            _ => {
                let b = crate::quadro_ui::is_quadro(&abs);
                self.caches.quadro_kind.insert(path.clone(), (mtime, b));
                b
            }
        };
        if !is || path == self.source {
            return None;
        }
        let r = crate::quadro_ui::embed(ui, self.vault, self.caches, &path, self.palette)?;
        Some((path, r))
    }

    fn embed(&mut self, target: &str) -> Option<(String, Arc<Doc>)> {
        let path = self.vault.resolve(target.split('#').next().unwrap_or(target), self.source)?;
        if !path.to_lowercase().ends_with(".md") || path == self.source {
            return None;
        }
        let abs = self.vault.abs(&path);
        let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok()?;
        if let Some((t, d)) = self.caches.docs.get(&path) {
            if *t == mtime {
                return Some((path, d.clone()));
            }
        }
        let text = std::fs::read_to_string(&abs).ok()?;
        let doc = Arc::new(markdown::parse(&text));
        self.caches.docs.insert(path.clone(), (mtime, doc.clone()));
        Some((path, doc))
    }
}
