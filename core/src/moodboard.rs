//! Moodboards: infinite boards of reference pictures, notes, colour swatches
//! and frames. This was Pictogrep's "Moodboard" plugin; the documents keep
//! its format and live where it kept them (the plugin's storage in the shared
//! library, `data/plugins/moodboard.json`), so boards made in either app open
//! in the other. Without a shared library they go to the vault
//! (`.obsidian/ateliet/moodboard.json`), in the same format.
//!
//! The store is one JSON object: `index` (the list of boards), `board:<id>`
//! (one document each) and `prefs`. A board keeps pictures by Pictogrep's
//! image id (SHA-256 of the picture's absolute path), never by file name.
//!
//! The geometry here works in board units: an item's box is `x, y, w, h`
//! before its rotation `r` (degrees, about its centre).

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const PLUGIN_ID: &str = "moodboard";

// ── documents ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Note,
    Swatch,
    Frame,
}

/// A window over the source picture, each side in 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub r: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(rename = "imageId", default, skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub gray: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A note's font size, in board units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fs: Option<f64>,
    /// A note's paper colour (`#rrggbb`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// A swatch's colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A frame's tint, an index into [`FRAME_TINTS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<usize>,
    /// Fields this version doesn't know, kept as they were.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Item {
    fn blank(kind: Kind, x: f64, y: f64, w: f64, h: f64) -> Item {
        Item { id: uid(), kind, x, y, w, h, r: 0.0, locked: false, image_id: None, crop: None, flip: false, gray: false, text: None, fs: None, fill: None, color: None, title: None, tint: None, extra: Map::new() }
    }

    pub fn image(image_id: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
        Item { image_id: Some(image_id.to_string()), ..Item::blank(Kind::Image, x, y, w, h) }
    }

    pub fn note(x: f64, y: f64, w: f64, h: f64, fs: f64, fill: Option<String>) -> Item {
        Item { text: Some(String::new()), fs: Some(fs), fill, ..Item::blank(Kind::Note, x, y, w, h) }
    }

    pub fn swatch(color: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
        Item { color: Some(color.to_string()), ..Item::blank(Kind::Swatch, x, y, w, h) }
    }

    pub fn frame(title: &str, x: f64, y: f64, w: f64, h: f64) -> Item {
        Item { title: Some(title.to_string()), tint: Some(0), ..Item::blank(Kind::Frame, x, y, w, h) }
    }

    /// A copy with a new id, moved, unlocked.
    pub fn cloned(&self, dx: f64, dy: f64) -> Item {
        Item { id: uid(), x: self.x + dx, y: self.y + dy, locked: false, ..self.clone() }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// The four corners after rotation (clockwise from the top left).
    pub fn corners(&self) -> [(f64, f64); 4] {
        let (cx, cy) = self.center();
        [(self.x, self.y), (self.x + self.w, self.y), (self.x + self.w, self.y + self.h), (self.x, self.y + self.h)].map(|(x, y)| rot_pt(x, y, cx, cy, self.r))
    }

    /// Whether a board point falls on the item (rotation counted).
    pub fn contains(&self, px: f64, py: f64) -> bool {
        let (cx, cy) = self.center();
        let (lx, ly) = rot_pt(px, py, cx, cy, -self.r);
        lx >= self.x && lx <= self.x + self.w && ly >= self.y && ly <= self.y + self.h
    }

    pub fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    pub fn color(&self) -> &str {
        self.color.as_deref().unwrap_or("#c8a27a")
    }

    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cam {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: i64,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: i64,
    /// "studio" (the tools around the board) or "focus" (only the pictures).
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub bg: String,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub cam: Option<Cam>,
    #[serde(default)]
    pub items: Vec<Item>,
    /// Whether the board's pictures are filed in a "Moodboards" collection.
    #[serde(rename = "tagLibrary", default, skip_serializing_if = "Option::is_none")]
    pub tag_library: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub const UNTITLED: &str = "Moodboard sem título";

impl Board {
    pub fn new(name: &str, mode: &str, bg: &str, pattern: &str) -> Board {
        let now = now_ms();
        let mut extra = Map::new();
        extra.insert("v".into(), json!(1));
        Board { id: uid(), name: name.to_string(), created_at: now, updated_at: now, mode: mode.to_string(), bg: bg.to_string(), pattern: pattern.to_string(), cam: None, items: Vec::new(), tag_library: None, extra }.normalized()
    }

    /// Unknown or missing settings set to their defaults, as the plugin does.
    pub fn normalized(mut self) -> Board {
        if self.name.trim().is_empty() {
            self.name = UNTITLED.into();
        }
        if self.mode != "focus" {
            self.mode = "studio".into();
        }
        if !BACKGROUNDS.iter().any(|b| b.key == self.bg) {
            self.bg = "paper".into();
        }
        if !["plain", "dots", "grid"].contains(&self.pattern.as_str()) {
            self.pattern = "dots".into();
        }
        if self.created_at == 0 {
            self.created_at = now_ms();
        }
        if self.updated_at == 0 {
            self.updated_at = self.created_at;
        }
        self.items.retain(|i| i.w.is_finite() && i.h.is_finite() && i.x.is_finite() && i.y.is_finite());
        self
    }

    pub fn tags_on(&self) -> bool {
        self.tag_library != Some(false)
    }

    pub fn meta(&self) -> Meta {
        let mut imgs: Vec<&Item> = self.items.iter().filter(|i| i.kind == Kind::Image).collect();
        imgs.sort_by(|a, b| (b.w * b.h).total_cmp(&(a.w * a.h)));
        Meta {
            id: self.id.clone(),
            name: self.name.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            mode: self.mode.clone(),
            bg: self.bg.clone(),
            count: self.items.len(),
            cover: imgs.iter().take(4).filter_map(|i| i.image_id.clone()).collect(),
        }
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }
}

/// A board in the list (the store's `index`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: i64,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: i64,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub bg: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub cover: Vec<String>,
}

pub struct Background {
    pub key: &'static str,
    pub label: &'static str,
    pub color: [u8; 3],
    /// The dots or lines of the pattern: colour and alpha.
    pub dot: [u8; 4],
    pub dark: bool,
}

pub const BACKGROUNDS: [Background; 4] = [
    Background { key: "paper", label: "Papel", color: [0xf3, 0xf0, 0xe9], dot: [70, 55, 30, 43], dark: false },
    Background { key: "white", label: "Branco", color: [0xff, 0xff, 0xff], dot: [0, 0, 0, 28], dark: false },
    Background { key: "graphite", label: "Grafite", color: [0x2b, 0x2b, 0x2e], dot: [255, 255, 255, 23], dark: true },
    Background { key: "night", label: "Noite", color: [0x12, 0x12, 0x14], dot: [255, 255, 255, 18], dark: true },
];

pub fn background(key: &str) -> &'static Background {
    BACKGROUNDS.iter().find(|b| b.key == key).unwrap_or(&BACKGROUNDS[0])
}

pub const NOTE_FILLS: [Option<&str>; 7] = [None, Some("#fff1a8"), Some("#ffd9e2"), Some("#d3e7ff"), Some("#d6f2d2"), Some("#e9e1ff"), Some("#ffffff")];
pub const FRAME_TINTS: [Option<&str>; 6] = [None, Some("#ffd666"), Some("#ff8fab"), Some("#6ea8ff"), Some("#78c878"), Some("#a08cff")];

/// `#rrggbb` → rgb.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

// ── ids and time ──────────────────────────────────────────────────────────

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn base36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(D[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// An id like the plugin's: the time in base 36 and six random characters.
pub fn uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let t = now_ms() as u64;
    // no rand crate: mix the clock, a counter and an address
    let seed = t ^ SEQ.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed) ^ (&SEQ as *const _ as u64);
    let mut x = seed.wrapping_mul(0x2545_f491_4f6c_dd1d);
    x ^= x >> 29;
    let r = base36(x % 36u64.pow(6));
    format!("{}{:0>6}", base36(t), r)
}

// ── storage ───────────────────────────────────────────────────────────────

/// The file the boards live in.
pub struct Store {
    pub path: PathBuf,
}

impl Store {
    /// The shared library's plugin storage, or the vault's file.
    pub fn locate(shared: Option<&crate::pictogrep::Home>, vault_root: &Path) -> Store {
        match shared.and_then(|h| crate::pictogrep::storage_path(h, PLUGIN_ID)) {
            Some(path) => Store { path },
            None => Store { path: vault_root.join(".obsidian/ateliet/moodboard.json") },
        }
    }

    fn read(&self) -> Map<String, Value> {
        std::fs::read_to_string(&self.path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        }).unwrap_or_default()
    }

    /// Sets keys (`None` removes), re-reading the file first so what another
    /// app wrote to other keys stays.
    fn write(&self, keys: &[(&str, Option<Value>)]) -> Result<(), String> {
        let mut m = self.read();
        for (k, v) in keys {
            match v {
                Some(v) => {
                    m.insert(k.to_string(), v.clone());
                }
                None => {
                    m.remove(*k);
                }
            }
        }
        let text = serde_json::to_string_pretty(&Value::Object(m)).map_err(|e| e.to_string())? + "\n";
        crate::pictogrep::write_atomic(&self.path, text.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn boards(&self) -> Vec<Meta> {
        let m = self.read();
        let mut v: Vec<Meta> = m.get("index").and_then(|v| serde_json::from_value::<Vec<Value>>(v.clone()).ok()).unwrap_or_default().into_iter().filter_map(|x| serde_json::from_value::<Meta>(x).ok()).filter(|b| !b.id.is_empty()).collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
        v
    }

    pub fn load(&self, id: &str) -> Option<Board> {
        let m = self.read();
        let b = m.get(&format!("board:{id}")).and_then(|v| serde_json::from_value::<Board>(v.clone()).ok());
        match b {
            Some(b) => Some(b.normalized()),
            // a board in the list without its document: an empty one
            None => self.boards().into_iter().find(|x| x.id == id).map(|meta| {
                Board { id: meta.id, name: meta.name, created_at: meta.created_at, updated_at: meta.updated_at, mode: meta.mode, bg: meta.bg, pattern: String::new(), cam: None, items: Vec::new(), tag_library: None, extra: Map::new() }.normalized()
            }),
        }
    }

    /// The board's document and its row in the list.
    pub fn save(&self, b: &Board) -> Result<(), String> {
        let mut index: Vec<Value> = self.read().get("index").and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let meta = serde_json::to_value(b.meta()).map_err(|e| e.to_string())?;
        match index.iter().position(|x| x["id"].as_str() == Some(b.id.as_str())) {
            Some(i) => index[i] = meta,
            None => index.push(meta),
        }
        let doc = serde_json::to_value(b).map_err(|e| e.to_string())?;
        self.write(&[(&format!("board:{}", b.id), Some(doc)), ("index", Some(Value::Array(index)))])
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let index: Vec<Value> = self.read().get("index").and_then(|v| v.as_array().cloned()).unwrap_or_default().into_iter().filter(|x| x["id"].as_str() != Some(id)).collect();
        self.write(&[(&format!("board:{id}"), None), ("index", Some(Value::Array(index)))])
    }

    pub fn prefs(&self) -> Value {
        self.read().get("prefs").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}))
    }

    pub fn set_prefs(&self, prefs: &Value) -> Result<(), String> {
        self.write(&[("prefs", Some(prefs.clone()))])
    }
}

/// The collection a board files its pictures under, as Pictogrep names it.
pub fn board_collection(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c == '/' || c == '\\' { '-' } else { c }).filter(|c| !c.is_control()).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean: String = clean.chars().take(60).collect();
    let leaf = if clean.chars().any(|c| c.is_alphanumeric()) { clean.trim().to_string() } else { "Moodboard".into() };
    crate::pictogrep::collection_name(&format!("Moodboards/{leaf}")).unwrap_or_else(|_| "moodboards/moodboard".into())
}

// ── geometry ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && self.x + self.w > o.x && self.y < o.y + o.h && self.y + self.h > o.y
    }

    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.x + o.w <= self.x + self.w && o.y + o.h <= self.y + self.h
    }
}

pub fn rot_pt(x: f64, y: f64, cx: f64, cy: f64, deg: f64) -> (f64, f64) {
    if deg == 0.0 {
        return (x, y);
    }
    let (s, c) = deg.to_radians().sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

/// The box around items, rotation counted.
pub fn bbox<'a>(items: impl IntoIterator<Item = &'a Item>) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for it in items {
        for (x, y) in it.corners() {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if !x0.is_finite() {
        return Rect::default();
    }
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// What a frame holds: items whose centre is inside it, frames wholly inside.
pub fn frame_children(items: &[Item], frame: usize) -> Vec<usize> {
    let f = &items[frame];
    let fr = Rect { x: f.x, y: f.y, w: f.w, h: f.h };
    items.iter().enumerate().filter(|(i, it)| {
        if *i == frame {
            return false;
        }
        if it.kind == Kind::Frame {
            return fr.contains_rect(&Rect { x: it.x, y: it.y, w: it.w, h: it.h });
        }
        let (cx, cy) = it.center();
        cx >= f.x && cx <= f.x + f.w && cy >= f.y && cy <= f.y + f.h
    }).map(|(i, _)| i).collect()
}

/// PureRef-style "optimize": shelf-pack the items without resizing them,
/// tallest first, into rows about as wide as the group is tall. Locked items
/// and frames stay. Returns whether anything moved.
pub fn arrange(items: &mut [Item], which: &[usize]) -> bool {
    let idx: Vec<usize> = which.iter().copied().filter(|&i| !items[i].locked && items[i].kind != Kind::Frame).collect();
    if idx.len() < 2 {
        return false;
    }
    let bb = bbox(idx.iter().map(|&i| &items[i]));
    for &i in &idx {
        items[i].r = 0.0;
    }
    let mut sorted = idx.clone();
    sorted.sort_by(|&a, &b| items[b].h.total_cmp(&items[a].h));
    let area: f64 = idx.iter().map(|&i| items[i].w * items[i].h).sum();
    let avg_h = idx.iter().map(|&i| items[i].h).sum::<f64>() / idx.len() as f64;
    let gap = avg_h * 0.05;
    let widest = idx.iter().map(|&i| items[i].w).fold(0.0, f64::max);
    let row_w = widest.max(area.sqrt() * 1.35);
    let (mut x, mut y, mut row_h) = (bb.x, bb.y, 0.0f64);
    for i in sorted {
        let it = &mut items[i];
        if x > bb.x && x + it.w > bb.x + row_w {
            x = bb.x;
            y += row_h + gap;
            row_h = 0.0;
        }
        it.x = x;
        it.y = y;
        x += it.w + gap;
        row_h = row_h.max(it.h);
    }
    true
}

/// A snapping guide: a vertical (`v`) or horizontal line at `at`, drawn from
/// `from` to `to` along the other axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub v: bool,
    pub at: f64,
    pub from: f64,
    pub to: f64,
}

/// Aligns a moving box's edges and centre with nearby boxes within `th`.
pub fn snap(bb0: Rect, mut dx: f64, mut dy: f64, others: &[Rect], th: f64) -> (f64, f64, Vec<Guide>) {
    let b = Rect { x: bb0.x + dx, y: bb0.y + dy, ..bb0 };
    let mx = [b.x, b.x + b.w / 2.0, b.x + b.w];
    let my = [b.y, b.y + b.h / 2.0, b.y + b.h];
    let mut best_x: Option<(f64, f64, Rect)> = None;
    let mut best_y: Option<(f64, f64, Rect)> = None;
    for o in others {
        for m in mx {
            for v in [o.x, o.x + o.w / 2.0, o.x + o.w] {
                let d = v - m;
                if d.abs() < th && best_x.is_none_or(|bx| d.abs() < bx.0.abs()) {
                    best_x = Some((d, v, *o));
                }
            }
        }
        for m in my {
            for v in [o.y, o.y + o.h / 2.0, o.y + o.h] {
                let d = v - m;
                if d.abs() < th && best_y.is_none_or(|by| d.abs() < by.0.abs()) {
                    best_y = Some((d, v, *o));
                }
            }
        }
    }
    if let Some((d, ..)) = best_x {
        dx += d;
    }
    if let Some((d, ..)) = best_y {
        dy += d;
    }
    let nb = Rect { x: bb0.x + dx, y: bb0.y + dy, ..bb0 };
    let mut guides = Vec::new();
    if let Some((_, at, o)) = best_x {
        guides.push(Guide { v: true, at, from: nb.y.min(o.y), to: (nb.y + nb.h).max(o.y + o.h) });
    }
    if let Some((_, at, o)) = best_y {
        guides.push(Guide { v: false, at, from: nb.x.min(o.x), to: (nb.x + nb.w).max(o.x + o.w) });
    }
    (dx, dy, guides)
}

/// One corner or edge dragged: the new box of an item resized from the
/// opposite anchor `a` (a board point) towards `p`. `sx`/`sy` are −1, 0 or 1
/// (which side moves). Pictures keep their proportions unless `free`; notes
/// only change width.
pub fn resize_one(it: &Item, w0: f64, h0: f64, a: (f64, f64), sx: f64, sy: f64, p: (f64, f64), free: bool, min: f64) -> (f64, f64, f64, f64) {
    let (s, c) = it.r.to_radians().sin_cos();
    let (vx, vy) = (p.0 - a.0, p.1 - a.1);
    let (lx, ly) = (vx * c + vy * s, -vx * s + vy * c);
    let (nw, nh) = match it.kind {
        Kind::Note => ((sx * lx).max(min * 3.0), it.h),
        Kind::Frame => ((sx * lx).max(min * 2.0), (sy * ly).max(min * 2.0)),
        Kind::Image if free => ((sx * lx).max(min * 2.0), (sy * ly).max(min * 2.0)),
        _ => {
            let k = (min / w0.min(h0)).max(((sx * lx) / w0).max((sy * ly) / h0));
            (w0 * k, h0 * k)
        }
    };
    let (ox, oy) = (sx * nw / 2.0, sy * nh / 2.0);
    let (cx, cy) = (a.0 + ox * c - oy * s, a.1 + ox * s + oy * c);
    (cx - nw / 2.0, cy - nh / 2.0, nw, nh)
}

// ── crop ──────────────────────────────────────────────────────────────────

/// A crop in progress: the whole picture laid out ("display frame", mirrored
/// when flipped) at the item's scale and pinned in place, with the window
/// being dragged over it.
#[derive(Clone, Debug, PartialEq)]
pub struct CropStage {
    pub id: String,
    /// The whole picture's size on the board.
    pub fw: f64,
    pub fh: f64,
    /// Where the window started, in the display frame.
    pub o0: (f64, f64),
    pub w0: f64,
    pub h0: f64,
    /// The item's centre and rotation when cropping started.
    pub c0: (f64, f64),
    pub r: f64,
    /// The window now, in the display frame.
    pub rect: Rect,
}

impl CropStage {
    pub fn start(it: &Item) -> CropStage {
        let c = it.crop.unwrap_or(Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let (fw, fh) = (it.w / c.w, it.h / c.h);
        let o0 = ((if it.flip { 1.0 - c.x - c.w } else { c.x }) * fw, c.y * fh);
        CropStage { id: it.id.clone(), fw, fh, o0, w0: it.w, h0: it.h, c0: it.center(), r: it.r, rect: Rect { x: o0.0, y: o0.1, w: it.w, h: it.h } }
    }

    /// Board point → display frame.
    pub fn local(&self, p: (f64, f64)) -> (f64, f64) {
        let (dx, dy) = rot_pt(p.0, p.1, self.c0.0, self.c0.1, -self.r);
        (dx - self.c0.0 + self.o0.0 + self.w0 / 2.0, dy - self.c0.1 + self.o0.1 + self.h0 / 2.0)
    }

    /// Display frame → board point.
    pub fn world(&self, q: (f64, f64)) -> (f64, f64) {
        let (dx, dy) = (q.0 - self.o0.0 - self.w0 / 2.0, q.1 - self.o0.1 - self.h0 / 2.0);
        rot_pt(self.c0.0 + dx, self.c0.1 + dy, self.c0.0, self.c0.1, self.r)
    }

    /// A handle ("move", "n", "se"…) dragged by `d` (display frame) from `r0`.
    pub fn drag(&mut self, k: &str, r0: Rect, d: (f64, f64), keep_ratio: bool, min: f64) {
        let (mut x, mut y, mut w, mut h) = (r0.x, r0.y, r0.w, r0.h);
        if k == "move" {
            x = (r0.x + d.0).clamp(0.0, (self.fw - r0.w).max(0.0));
            y = (r0.y + d.1).clamp(0.0, (self.fh - r0.h).max(0.0));
        } else {
            if k.contains('w') {
                x = (r0.x + d.0).clamp(0.0, r0.x + r0.w - min);
                w = r0.x + r0.w - x;
            }
            if k.contains('e') {
                w = (r0.w + d.0).clamp(min, self.fw - r0.x);
            }
            if k.contains('n') {
                y = (r0.y + d.1).clamp(0.0, r0.y + r0.h - min);
                h = r0.y + r0.h - y;
            }
            if k.contains('s') {
                h = (r0.h + d.1).clamp(min, self.fh - r0.y);
            }
            if keep_ratio && k.len() == 2 {
                let ar = r0.w / r0.h;
                if w / h > ar {
                    w = h * ar;
                } else {
                    h = w / ar;
                }
                if k.contains('w') {
                    x = r0.x + r0.w - w;
                }
                if k.contains('n') {
                    y = r0.y + r0.h - h;
                }
            }
        }
        self.rect = Rect { x, y, w, h };
    }

    pub fn reset(&mut self) {
        self.rect = Rect { x: 0.0, y: 0.0, w: self.fw, h: self.fh };
    }

    /// The item as cropped; false when nothing changed.
    pub fn apply(&self, it: &mut Item) -> bool {
        let Rect { x, y, w, h } = self.rect;
        let eps = 1e-6 * self.fw.max(self.fh);
        if (x - self.o0.0).abs() < eps && (y - self.o0.1).abs() < eps && (w - self.w0).abs() < eps && (h - self.h0).abs() < eps {
            return false;
        }
        let full = x < eps && y < eps && (w - self.fw).abs() < eps && (h - self.fh).abs() < eps;
        it.crop = (!full).then(|| Crop { x: (if it.flip { self.fw - x - w } else { x }) / self.fw, y: y / self.fh, w: w / self.fw, h: h / self.fh });
        let c = self.world((x + w / 2.0, y + h / 2.0));
        it.w = w;
        it.h = h;
        it.x = c.0 - w / 2.0;
        it.y = c.1 - h / 2.0;
        true
    }
}

/// The whole picture again, where it is: the full frame's centre, seen from
/// the current window, becomes the item's centre.
pub fn remove_crop(it: &mut Item) -> bool {
    let Some(c) = it.crop.take() else { return false };
    let (fw, fh) = (it.w / c.w, it.h / c.h);
    let disp_x = (if it.flip { 1.0 - c.x - c.w } else { c.x }) * fw;
    let (dx, dy) = (fw / 2.0 - (disp_x + it.w / 2.0), fh / 2.0 - (c.y * fh + it.h / 2.0));
    let (cx, cy) = it.center();
    let (nx, ny) = rot_pt(cx + dx, cy + dy, cx, cy, it.r);
    it.w = fw;
    it.h = fh;
    it.x = nx - fw / 2.0;
    it.y = ny - fh / 2.0;
    true
}

// ── palettes ──────────────────────────────────────────────────────────────

/// Up to five colours of an RGBA picture: a coarse histogram over 4-bit
/// buckets, weighted toward saturated colours (so a palette isn't five
/// greys of background), then the strongest buckets that are visibly apart.
pub fn palette(rgba: &[u8]) -> Vec<[u8; 3]> {
    let mut buckets: std::collections::HashMap<u32, (u64, u64, u64, u64)> = std::collections::HashMap::new();
    for px in rgba.chunks_exact(4) {
        if px[3] < 128 {
            continue;
        }
        let key = ((px[0] as u32 >> 4) << 8) | ((px[1] as u32 >> 4) << 4) | (px[2] as u32 >> 4);
        let b = buckets.entry(key).or_default();
        b.0 += 1;
        b.1 += px[0] as u64;
        b.2 += px[1] as u64;
        b.3 += px[2] as u64;
    }
    let mut colors: Vec<([f64; 3], f64)> = buckets.values().map(|&(n, r, g, b)| {
        let c = [r as f64 / n as f64, g as f64 / n as f64, b as f64 / n as f64];
        let max = c[0].max(c[1]).max(c[2]);
        let min = c[0].min(c[1]).min(c[2]);
        (c, n as f64 * (0.55 + if max > 0.0 { (max - min) / max } else { 0.0 }))
    }).collect();
    colors.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut picked: Vec<[f64; 3]> = Vec::new();
    for min_dist in [70.0, 45.0, 25.0] {
        for (c, _) in &colors {
            if picked.len() >= 5 {
                break;
            }
            let far = picked.iter().all(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt() >= min_dist);
            if far {
                picked.push(*c);
            }
        }
        if picked.len() >= 5 {
            break;
        }
    }
    picked.into_iter().map(|c| c.map(|v| v.round().clamp(0.0, 255.0) as u8)).collect()
}

/// Where a palette of `n` swatches goes beside a picture: a row under (or
/// over) it, or a column beside it, the first spot that covers nothing else.
/// `unit` is one screen pixel in board units (swatches stay a readable size).
pub fn palette_spots(pic: Rect, n: usize, unit: f64, others: &[Rect]) -> Vec<Rect> {
    let n = n.max(1) as f64;
    let gap = pic.w.min(pic.h) * 0.025;
    let row_w = pic.w.min(n * 140.0 * unit);
    let row_sw = (row_w - gap * (n - 1.0)) / n;
    let col_sw = ((pic.h - gap * (n - 1.0)) / n / 1.25).min(140.0 * unit);
    let row = |x: f64, y: f64| (Rect { x, y, w: row_w, h: row_sw * 1.25 }, true);
    let col = |x: f64, y: f64| (Rect { x, y, w: col_sw, h: n * col_sw * 1.25 + (n - 1.0) * gap }, false);
    let spots = [row(pic.x, pic.y + pic.h + gap * 2.0), col(pic.x + pic.w + gap * 2.0, pic.y), col(pic.x - col_sw - gap * 2.0, pic.y), row(pic.x, pic.y - row_sw * 1.25 - gap * 2.0)];
    let (spot, is_row) = spots.iter().copied().find(|(s, _)| !others.iter().any(|o| s.intersects(o))).unwrap_or(spots[0]);
    (0..n as usize).map(|i| {
        let i = i as f64;
        if is_row {
            Rect { x: spot.x + i * (row_sw + gap), y: spot.y, w: row_sw, h: row_sw * 1.25 }
        } else {
            Rect { x: spot.x, y: spot.y + i * (col_sw * 1.25 + gap), w: col_sw, h: col_sw * 1.25 }
        }
    }).collect()
}

/// Where new pictures go around `at`: one alone `long` units on its longer
/// side, several in rows `row_h` high, about as many rows as columns.
/// `aspects` are width over height; returns boxes in order.
pub fn place(aspects: &[f64], at: (f64, f64), long: f64, row_h: f64, gap: f64) -> Vec<Rect> {
    let mut boxes: Vec<Rect> = Vec::new();
    if aspects.len() == 1 {
        let a = aspects[0];
        let (w, h) = if a >= 1.0 { (long, long / a) } else { (long * a, long) };
        boxes.push(Rect { x: 0.0, y: 0.0, w, h });
    } else {
        let per_row = (aspects.len() as f64).sqrt().ceil() as usize;
        let (mut x, mut y) = (0.0, 0.0);
        for (i, a) in aspects.iter().enumerate() {
            if i > 0 && i % per_row == 0 {
                x = 0.0;
                y += row_h + gap;
            }
            let w = row_h * a;
            boxes.push(Rect { x, y, w, h: row_h });
            x += w + gap;
        }
    }
    let bw = boxes.iter().map(|b| b.x + b.w).fold(0.0, f64::max);
    let bh = boxes.iter().map(|b| b.y + b.h).fold(0.0, f64::max);
    for b in &mut boxes {
        b.x += at.0 - bw / 2.0;
        b.y += at.1 - bh / 2.0;
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(x: f64, y: f64, w: f64, h: f64) -> Item {
        Item::image("abc", x, y, w, h)
    }

    #[test]
    fn plugin_documents_round_trip() {
        let src = json!({
            "id": "lz1", "name": "Vórtices", "createdAt": 1, "updatedAt": 2, "mode": "focus", "bg": "night", "pattern": "grid",
            "cam": {"x": 10.5, "y": -3, "z": 0.5}, "v": 1, "future": {"keep": true},
            "items": [
                {"id": "a", "type": "image", "imageId": "f00", "x": 0, "y": 0, "w": 100, "h": 50, "r": 12, "flip": true, "crop": {"x": 0.1, "y": 0, "w": 0.5, "h": 1}},
                {"id": "b", "type": "note", "text": "olá", "fs": 20, "fill": null, "x": 0, "y": 0, "w": 300, "h": 30, "r": 0},
                {"id": "c", "type": "swatch", "color": "#aabbcc", "x": 0, "y": 0, "w": 120, "h": 150, "r": 0, "locked": true},
                {"id": "d", "type": "frame", "title": "Seção 1", "tint": 2, "x": 0, "y": 0, "w": 640, "h": 420, "r": 0}
            ]
        });
        let b: Board = serde_json::from_value(src).unwrap();
        assert_eq!(b.items.len(), 4);
        assert!(b.items[0].flip && b.items[0].crop.is_some() && b.items[0].r == 12.0);
        assert!(b.items[2].locked);
        assert_eq!(b.items[3].tint, Some(2));
        let back = serde_json::to_value(&b).unwrap();
        assert_eq!(back["future"]["keep"], json!(true), "unknown fields stay");
        assert_eq!(back["items"][0]["imageId"], json!("f00"));
        assert_eq!(back["items"][1]["type"], json!("note"));
        assert!(back["items"][1].get("locked").is_none());
    }

    #[test]
    fn store_keeps_other_keys_and_the_index() {
        let dir = std::env::temp_dir().join(format!("ateliet-mb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store { path: dir.join("moodboard.json") };
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&store.path, r#"{"prefs": {"wheel": "zoom"}, "other": 1}"#).unwrap();
        let mut b = Board::new("Um", "studio", "paper", "dots");
        b.items.push(img(0.0, 0.0, 10.0, 10.0));
        store.save(&b).unwrap();
        assert_eq!(store.boards().len(), 1);
        assert_eq!(store.boards()[0].count, 1);
        assert_eq!(store.load(&b.id).unwrap().items.len(), 1);
        assert_eq!(store.prefs()["wheel"], json!("zoom"));
        store.delete(&b.id).unwrap();
        assert!(store.boards().is_empty());
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(raw["other"], json!(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_and_bounds() {
        let mut it = img(0.0, 0.0, 100.0, 50.0);
        it.r = 90.0;
        let bb = bbox([&it]);
        assert!((bb.w - 50.0).abs() < 1e-9 && (bb.h - 100.0).abs() < 1e-9);
        assert!(it.contains(50.0, 70.0), "rotated, it reaches below its unrotated box");
        assert!(!it.contains(5.0, 25.0));
    }

    #[test]
    fn arrange_packs_without_overlaps() {
        let mut items: Vec<Item> = (0..6).map(|i| img(i as f64 * 7.0, i as f64 * 3.0, 100.0 + i as f64 * 10.0, 80.0 + i as f64 * 5.0)).collect();
        let all: Vec<usize> = (0..items.len()).collect();
        assert!(arrange(&mut items, &all));
        for a in 0..items.len() {
            for b in a + 1..items.len() {
                let ra = Rect { x: items[a].x, y: items[a].y, w: items[a].w, h: items[a].h };
                let rb = Rect { x: items[b].x, y: items[b].y, w: items[b].w, h: items[b].h };
                assert!(!ra.intersects(&rb), "{a} and {b} overlap");
            }
        }
    }

    #[test]
    fn snapping_aligns_edges() {
        let other = Rect { x: 200.0, y: 0.0, w: 50.0, h: 50.0 };
        let (dx, dy, g) = snap(Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 }, 147.0, 2.0, &[other], 6.0);
        assert_eq!((dx, dy), (150.0, 0.0), "the right edge meets the other's left edge, tops align");
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn crop_and_uncrop_keep_the_picture_in_place() {
        let mut it = img(10.0, 20.0, 200.0, 100.0);
        it.r = 30.0;
        let mut st = CropStage::start(&it);
        st.drag("e", st.rect, (-100.0, 0.0), false, 1.0);
        assert!(st.apply(&mut it));
        assert!((it.w - 100.0).abs() < 1e-9);
        assert_eq!(it.crop, Some(Crop { x: 0.0, y: 0.0, w: 0.5, h: 1.0 }));
        assert!(remove_crop(&mut it));
        assert!((it.x - 10.0).abs() < 1e-6 && (it.y - 20.0).abs() < 1e-6 && (it.w - 200.0).abs() < 1e-9);
        // flipped: the window keeps the mirrored side
        let mut f = img(0.0, 0.0, 200.0, 100.0);
        f.flip = true;
        let mut st = CropStage::start(&f);
        st.drag("e", st.rect, (-100.0, 0.0), false, 1.0);
        st.apply(&mut f);
        assert_eq!(f.crop.unwrap().x, 0.5);
    }

    #[test]
    fn resizing_keeps_proportions_and_the_anchor() {
        let it = img(0.0, 0.0, 100.0, 50.0);
        // dragging the bottom-right corner from the top-left anchor
        let (x, y, w, h) = resize_one(&it, 100.0, 50.0, (0.0, 0.0), 1.0, 1.0, (200.0, 60.0), false, 1.0);
        assert_eq!((x, y, w, h), (0.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn palettes_pick_distinct_colours() {
        let mut px = Vec::new();
        for i in 0..400 {
            let c = if i < 200 { [240, 240, 240] } else if i < 300 { [200, 30, 30] } else { [30, 30, 200] };
            px.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        let p = palette(&px);
        assert_eq!(p.len(), 3);
        assert!(p.contains(&[200, 30, 30]) && p.contains(&[30, 30, 200]));
    }

    #[test]
    fn collections_follow_pictogrep_names() {
        assert_eq!(board_collection("Vórtices"), "moodboards/vortices");
        assert!(!board_collection("a / b").contains("a/b"), "a slash in the name is not a subfolder");
        assert_eq!(board_collection("!!!"), "moodboards/moodboard");
        assert_ne!(uid(), uid());
    }

    #[test]
    fn placing_centres_the_group() {
        let b = place(&[1.0, 2.0, 0.5, 1.0], (0.0, 0.0), 320.0, 100.0, 10.0);
        let bb = (b.iter().map(|r| r.x).fold(f64::INFINITY, f64::min), b.iter().map(|r| r.x + r.w).fold(f64::NEG_INFINITY, f64::max));
        assert!((bb.0 + bb.1).abs() < 1e-9);
        assert_eq!(b[1].w, 200.0);
    }
}
