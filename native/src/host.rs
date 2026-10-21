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
