//! Fonts for typesetting: the bundled ones and the system's (TrueType,
//! OpenType/CFF and the faces of collections), found by family, style and
//! weight; text shaped with rustybuzz (kerning, ligatures, small caps,
//! old-style figures…), in font units, cached per word.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub type FontId = usize;

type ShapeCache = HashMap<(FontId, u32, String), Arc<Vec<Shaped>>>;

pub struct Face {
    pub family: String,
    pub italic: bool,
    pub weight: u16,
    pub path: Option<PathBuf>,
    pub index: u32,
    bytes: OnceLock<Option<Arc<Vec<u8>>>>,
    pub upem: f32,
    pub ascender: f32,
    pub descender: f32,
    pub cap_height: f32,
    pub cff: bool,
}

impl Face {
    pub fn data(&self) -> Option<Arc<Vec<u8>>> {
        self.bytes
            .get_or_init(|| self.path.as_ref().and_then(|p| std::fs::read(p).ok()).map(Arc::new))
            .clone()
    }
}

/// A shaped glyph, in font units.
#[derive(Clone, Copy, Debug)]
pub struct Shaped {
    pub id: u16,
    pub adv: i32,
    pub dx: i32,
    pub dy: i32,
    /// Byte offset of its cluster in the text.
    pub cluster: u32,
}

#[derive(Default)]
pub struct FontDb {
    pub faces: Vec<Face>,
    cache: Mutex<ShapeCache>,
}

fn name_of(f: &ttf_parser::Face, ids: &[u16]) -> Option<String> {
    for id in ids {
        if let Some(n) = f.names().into_iter().find(|n| n.name_id == *id && n.is_unicode()).and_then(|n| n.to_string()) {
            return Some(n);
        }
    }
    None
}

/// family, italic, weight, units per em, ascender, descender, cap height, CFF outlines
type Info = (String, bool, u16, f32, f32, f32, f32, bool);

fn face_info(data: &[u8], index: u32) -> Option<Info> {
    let f = ttf_parser::Face::parse(data, index).ok()?;
    // typographic family first (16), then the legacy one (1)
    let family = name_of(&f, &[16, 1])?;
    let upem = f.units_per_em() as f32;
    let cap = f.capital_height().map(|c| c as f32).unwrap_or(upem * 0.7);
    let cff = f.tables().cff.is_some();
    Some((family, f.is_italic() || f.is_oblique(), f.weight().to_number(), upem, f.ascender() as f32, f.descender() as f32, cap, cff))
}

impl FontDb {
    /// The bundled fonts (bytes compiled into the program).
    pub fn new(bundled: &[&'static [u8]]) -> FontDb {
        let mut db = FontDb::default();
        for b in bundled {
            let data = Arc::new(b.to_vec());
            if let Some((family, italic, weight, upem, asc, desc, cap, cff)) = face_info(&data, 0) {
                let lock = OnceLock::new();
                let _ = lock.set(Some(data));
                db.faces.push(Face { family, italic, weight, path: None, index: 0, bytes: lock, upem, ascender: asc, descender: desc, cap_height: cap, cff });
            }
        }
        db
    }

    /// Add the fonts installed in the system (their tables are read now, the bytes when used).
    pub fn scan_system(&mut self) {
        let mut dirs: Vec<PathBuf> = ["/System/Library/Fonts", "/Library/Fonts", "/usr/share/fonts", "/usr/local/share/fonts", "C:\\Windows\\Fonts"].iter().map(PathBuf::from).collect();
        if let Some(h) = std::env::var_os("HOME") {
            let h = PathBuf::from(h);
            dirs.push(h.join("Library/Fonts"));
            dirs.push(h.join(".fonts"));
            dirs.push(h.join(".local/share/fonts"));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(l).join("Microsoft/Windows/Fonts"));
        }
        for d in dirs {
            for e in walkdir::WalkDir::new(&d).max_depth(4).into_iter().flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
                if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
                    continue;
                }
                self.add_file(p);
            }
        }
    }

    pub fn add_file(&mut self, p: &Path) {
        let Ok(data) = std::fs::read(p) else { return };
        let n = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for i in 0..n {
            if let Some((family, italic, weight, upem, asc, desc, cap, cff)) = face_info(&data, i) {
                if self.faces.iter().any(|f| f.family == family && f.italic == italic && f.weight == weight) {
                    continue;
                }
                self.faces.push(Face { family, italic, weight, path: Some(p.to_path_buf()), index: i, bytes: OnceLock::new(), upem, ascender: asc, descender: desc, cap_height: cap, cff });
            }
        }
    }

    /// The families available, sorted.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.faces.iter().map(|f| f.family.clone()).filter(|f| !f.starts_with('.')).collect();
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup();
        v
    }

    /// The face of a family closest to a style and weight.
    pub fn find(&self, family: &str, italic: bool, weight: u16) -> Option<FontId> {
        let fam = family.trim().trim_matches(|c| c == '"' || c == '\'').to_lowercase();
        let cands: Vec<usize> = (0..self.faces.len()).filter(|&i| self.faces[i].family.to_lowercase() == fam).collect();
        if cands.is_empty() {
            return None;
        }
        cands.into_iter().min_by_key(|&i| {
            let f = &self.faces[i];
            (if f.italic != italic { 10_000 } else { 0 }) + (f.weight as i32 - weight as i32).unsigned_abs() as i32
        })
    }

    pub fn face(&self, id: FontId) -> &Face {
        &self.faces[id]
    }

    /// Shape text with OpenType features (e.g. `smcp`, `onum`); cached.
    pub fn shape(&self, id: FontId, text: &str, features: &[&str]) -> Arc<Vec<Shaped>> {
        let fkey = features.iter().fold(0u32, |a, f| a.wrapping_mul(31).wrapping_add(f.bytes().fold(0u32, |x, b| x.wrapping_mul(131).wrapping_add(b as u32))));
        let key = (id, fkey, text.to_string());
        if let Some(v) = self.cache.lock().unwrap().get(&key) {
            return v.clone();
        }
        let out = Arc::new(self.shape_now(id, text, features));
        let mut c = self.cache.lock().unwrap();
        if c.len() > 200_000 {
            c.clear();
        }
        c.insert(key, out.clone());
        out
    }

    fn shape_now(&self, id: FontId, text: &str, features: &[&str]) -> Vec<Shaped> {
        let face = &self.faces[id];
        let Some(data) = face.data() else { return Vec::new() };
        let Some(rb) = rustybuzz::Face::from_slice(&data, face.index) else { return Vec::new() };
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let feats: Vec<rustybuzz::Feature> = features.iter().filter_map(|f| {
            let b = f.as_bytes();
            (b.len() == 4).then(|| rustybuzz::Feature::new(ttf_parser::Tag::from_bytes(&[b[0], b[1], b[2], b[3]]), 1, ..))
        }).collect();
        let out = rustybuzz::shape(&rb, &feats, buf);
        out.glyph_infos().iter().zip(out.glyph_positions()).map(|(i, p)| Shaped { id: i.glyph_id as u16, adv: p.x_advance, dx: p.x_offset, dy: p.y_offset, cluster: i.cluster }).collect()
    }

    /// Width of text in mm at `size` pt.
    pub fn width(&self, id: FontId, text: &str, size: f32, features: &[&str]) -> f32 {
        let s = self.shape(id, text, features);
        let units: i32 = s.iter().map(|g| g.adv).sum();
        units as f32 / self.faces[id].upem * size * super::PT_MM
    }

    /// Whether the face has a glyph for this character.
    pub fn covers(&self, id: FontId, c: char) -> bool {
        let face = &self.faces[id];
        face.data().and_then(|d| ttf_parser::Face::parse(&d, face.index).ok().map(|f| f.glyph_index(c).is_some())).unwrap_or(false)
    }
}
