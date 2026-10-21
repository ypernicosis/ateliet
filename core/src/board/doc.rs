//! A `.storyboard` document: `board.json` (kept as JSON so fields this code
//! does not know survive a save), the layers' tiles, pictures kept inside
//! the file and the frame thumbnails.

use super::tile::{Tile, TILE};
use super::zip;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const PAPER: &str = "#e0dedb";
pub const DEFAULT_PALETTE: [&str; 12] = ["#23221f", "#5b5853", "#9a968f", "#ffffff", "#c0392b", "#e67e22", "#f1c40f", "#27ae60", "#2980b9", "#8e44ad", "#6fa8dc", "#f4b6c2"];

/// One tile of a layer: its PNG as stored, and the decoded pixels once needed.
#[derive(Default)]
pub struct Slot {
    pub png: Option<Vec<u8>>,
    pub tile: Option<Tile>,
    pub dirty: bool,
}

pub type TileKey = (String, i32, i32);

pub struct BoardDoc {
    pub board: Value,
    pub tiles: HashMap<TileKey, Slot>,
    /// `refs/…` entries (pictures kept inside the storyboard).
    pub refs: HashMap<String, Vec<u8>>,
    pub thumbs: HashMap<String, Vec<u8>>,
    pub preview: Option<Vec<u8>>,
    /// Entries this code does not know, written back as they were.
    pub other: Vec<(String, Vec<u8>)>,
    /// Frames whose picture changed since the last save (thumbnails to redo).
    pub changed_frames: HashSet<String>,
}

pub fn uid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0) ^ N.fetch_add(0x9e37_79b9, std::sync::atomic::Ordering::Relaxed).rotate_left(17);
    let mut x = t | 1;
    let mut s = String::new();
    for _ in 0..10 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(x % 36) as usize] as char);
    }
    s
}

pub fn new_layer(name: &str, kind: &str) -> Value {
    json!({ "id": uid(), "name": name, "kind": kind, "visible": true, "opacity": 1, "blend": "source-over", "locked": false, "clip": false, "x": 0, "y": 0 })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl BoardDoc {
    pub fn new(kind: &str, width: u32, height: u32, background: &str, fps: u32) -> BoardDoc {
        let infinite = kind == "canvas";
        let board = json!({
            "format": 1,
            "kind": kind,
            "width": width,
            "height": height,
            "infinite": infinite,
            "background": background,
            "fps": fps,
            "frames": [],
            "layers": {},
            "references": [],
            "palette": DEFAULT_PALETTE,
            "recent": [],
            "guides": { "thirds": false, "center": false, "safe": false, "grid": 0, "mirror": false },
            "onion": { "enabled": false, "before": 1, "after": 1, "opacity": 0.3, "tint": true, "colorBefore": "#d0453b", "colorAfter": "#2f9e5a", "fade": true, "pictures": false },
            "created": now_ms(),
            "modified": now_ms(),
        });
        let mut d = BoardDoc { board, tiles: HashMap::new(), refs: HashMap::new(), thumbs: HashMap::new(), preview: None, other: Vec::new(), changed_frames: HashSet::new() };
        d.add_frame(0);
        d
    }

    pub fn open(path: &Path) -> Result<BoardDoc, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<BoardDoc, String> {
        let entries = zip::read(bytes)?;
        let mut board: Option<Value> = None;
        let mut d = BoardDoc { board: Value::Null, tiles: HashMap::new(), refs: HashMap::new(), thumbs: HashMap::new(), preview: None, other: Vec::new(), changed_frames: HashSet::new() };
        for (name, data) in entries {
            if name == "board.json" {
                board = Some(serde_json::from_slice(&data).map_err(|e| format!("board.json: {e}"))?);
            } else if let Some(rest) = name.strip_prefix("tiles/").and_then(|r| r.strip_suffix(".png")) {
                // tiles/<layer>/<x>_<y>.png
                if let Some((layer, xy)) = rest.rsplit_once('/') {
                    if let Some((x, y)) = xy.split_once('_') {
                        if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                            d.tiles.insert((layer.to_string(), x, y), Slot { png: Some(data), tile: None, dirty: false });
                            continue;
                        }
                    }
                }
                d.other.push((name, data));
            } else if name.starts_with("refs/") {
                d.refs.insert(name, data);
            } else if let Some(id) = name.strip_prefix("thumbs/").and_then(|r| r.strip_suffix(".png")) {
                d.thumbs.insert(id.to_string(), data);
            } else if name == "preview.png" {
                d.preview = Some(data);
            } else {
                d.other.push((name, data));
            }
        }
        d.board = board.ok_or("not a storyboard file (no board.json)")?;
        d.normalize();
        Ok(d)
    }

    /// Fill in what an older or hand-edited board lacks.
    fn normalize(&mut self) {
        let b = &mut self.board;
        if !b["layers"].is_object() {
            b["layers"] = json!({});
        }
        for (k, def) in [("width", json!(1920)), ("height", json!(1080)), ("fps", json!(24)), ("background", json!(PAPER)), ("kind", json!("animatic")), ("references", json!([])), ("palette", json!(DEFAULT_PALETTE))] {
            if b[k].is_null() {
                b[k] = def;
            }
        }
        if b["kind"] == "canvas" {
            b["infinite"] = json!(true);
        }
        if !b["frames"].as_array().is_some_and(|f| !f.is_empty()) {
            b["frames"] = json!([]);
            self.add_frame(0);
            return;
        }
        let layer_ids: HashSet<String> = self.board["layers"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        let mut missing = Vec::new();
        if let Some(frames) = self.board["frames"].as_array_mut() {
            for (i, f) in frames.iter_mut().enumerate() {
                let layers: Vec<Value> = f["layers"].as_array().cloned().unwrap_or_default().into_iter().filter(|id| id.as_str().is_some_and(|s| layer_ids.contains(s))).collect();
                f["layers"] = json!(layers);
                if layers.is_empty() {
                    missing.push(i);
                }
                if f["duration"].as_f64().unwrap_or(0.0) < 1.0 {
                    f["duration"] = json!(24);
                }
            }
        }
        for i in missing {
            let l = new_layer("Drawing", "raster");
            let id = l["id"].as_str().unwrap().to_string();
            self.board["layers"][&id] = l;
            self.board["frames"][i]["layers"] = json!([id]);
            self.board["frames"][i]["active"] = json!(id);
        }
        let frames = self.frame_count();
        for i in 0..frames {
            let f = &self.board["frames"][i];
            let active = f["active"].as_str().unwrap_or_default().to_string();
            let layers: Vec<String> = f["layers"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
            if !layers.contains(&active) {
                self.board["frames"][i]["active"] = json!(layers.last());
            }
        }
    }

    // ── reading the model ──────────────────────────────────────────────────

    pub fn width(&self) -> u32 {
        self.board["width"].as_u64().unwrap_or(1920) as u32
    }
    pub fn height(&self) -> u32 {
        self.board["height"].as_u64().unwrap_or(1080) as u32
    }
    pub fn infinite(&self) -> bool {
        self.board["infinite"].as_bool().unwrap_or(false)
    }
    pub fn kind(&self) -> &str {
        self.board["kind"].as_str().unwrap_or("animatic")
    }
    pub fn fps(&self) -> u32 {
        self.board["fps"].as_u64().unwrap_or(24).max(1) as u32
    }
    pub fn frame_count(&self) -> usize {
        self.board["frames"].as_array().map(|a| a.len()).unwrap_or(0)
    }
    pub fn frame(&self, i: usize) -> &Value {
        &self.board["frames"][i]
    }
    pub fn frame_id(&self, i: usize) -> String {
        self.board["frames"][i]["id"].as_str().unwrap_or_default().to_string()
    }
    /// Layer ids of a frame, bottom to top.
    pub fn frame_layers(&self, i: usize) -> Vec<String> {
        self.board["frames"][i]["layers"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }
    pub fn active_layer(&self, i: usize) -> String {
        self.board["frames"][i]["active"].as_str().map(str::to_string).unwrap_or_else(|| self.frame_layers(i).last().cloned().unwrap_or_default())
    }
    pub fn layer(&self, id: &str) -> &Value {
        &self.board["layers"][id]
    }
    pub fn layer_mut(&mut self, id: &str) -> &mut Value {
        &mut self.board["layers"][id]
    }
    pub fn palette(&self) -> Vec<String> {
        self.board["palette"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }
    pub fn background(&self) -> String {
        self.board["background"].as_str().unwrap_or(PAPER).to_string()
    }

    // ── editing the model ──────────────────────────────────────────────────

    /// Shared layers (in every frame), in the order of the first frame that has them.
    fn shared_layers(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for i in 0..self.frame_count() {
            for id in self.frame_layers(i) {
                if self.layer(&id)["shared"].as_bool() == Some(true) && !seen.contains(&id) {
                    seen.push(id);
                }
            }
        }
        seen
    }

    /// A new frame at `index` with the shared layers and one drawing layer.
    pub fn add_frame(&mut self, index: usize) -> usize {
        let l = new_layer("Drawing", "raster");
        let id = l["id"].as_str().unwrap().to_string();
        self.board["layers"][&id] = l;
        let mut layers = self.shared_layers();
        layers.push(id.clone());
        let prev = if index > 0 { self.board["frames"][index - 1]["duration"].as_f64() } else { None };
        let duration = prev.unwrap_or((self.fps() as f64 * 2.0).round().max(1.0));
        let frame = json!({ "id": uid(), "name": "", "duration": duration, "layers": layers, "active": id, "notes": { "action": "", "dialogue": "", "camera": "", "shot": "", "sound": "" } });
        let frames = self.board["frames"].as_array_mut().unwrap();
        let at = index.min(frames.len());
        frames.insert(at, frame);
        self.touch();
        at
    }

    /// A copy of a frame right after it (its own layers get copied pixels).
    pub fn duplicate_frame(&mut self, i: usize) -> usize {
        let mut f = self.board["frames"][i].clone();
        let mut new_ids = Vec::new();
        for id in self.frame_layers(i) {
            if self.layer(&id)["shared"].as_bool() == Some(true) {
                new_ids.push(id);
                continue;
            }
            let mut l = self.layer(&id).clone();
            let nid = uid();
            l["id"] = json!(nid);
            self.board["layers"][&nid] = l;
            let keys: Vec<TileKey> = self.tiles.keys().filter(|k| k.0 == id).cloned().collect();
            for k in keys {
                self.ensure_decoded(&k);
                let tile = self.tiles.get(&k).and_then(|s| s.tile.clone());
                self.tiles.insert((nid.clone(), k.1, k.2), Slot { png: None, tile, dirty: true });
            }
            if f["active"] == json!(id) {
                f["active"] = json!(nid);
            }
            new_ids.push(nid);
        }
        f["id"] = json!(uid());
        f["layers"] = json!(new_ids);
        let frames = self.board["frames"].as_array_mut().unwrap();
        frames.insert(i + 1, f);
        let id = self.frame_id(i + 1);
        self.changed_frames.insert(id);
        self.touch();
        i + 1
    }

    pub fn delete_frame(&mut self, i: usize) {
        if self.frame_count() <= 1 {
            return;
        }
        self.board["frames"].as_array_mut().unwrap().remove(i);
        self.drop_unused_layers();
        self.touch();
    }

    pub fn move_frame(&mut self, from: usize, to: usize) {
        let frames = self.board["frames"].as_array_mut().unwrap();
        if from >= frames.len() || to >= frames.len() {
            return;
        }
        let f = frames.remove(from);
        frames.insert(to, f);
        self.touch();
    }

    /// A new drawing layer above the active one.
    pub fn add_layer(&mut self, frame: usize, name: &str) -> String {
        let l = new_layer(name, "raster");
        let id = l["id"].as_str().unwrap().to_string();
        self.board["layers"][&id] = l;
        let mut layers = self.frame_layers(frame);
        let active = self.active_layer(frame);
        let at = layers.iter().position(|x| *x == active).map(|p| p + 1).unwrap_or(layers.len());
        layers.insert(at, id.clone());
        self.board["frames"][frame]["layers"] = json!(layers);
        self.board["frames"][frame]["active"] = json!(id);
        self.touch();
        id
    }

    pub fn delete_layer(&mut self, frame: usize, id: &str) {
        let mut layers = self.frame_layers(frame);
        if layers.len() <= 1 {
            return;
        }
        layers.retain(|x| x != id);
        self.board["frames"][frame]["layers"] = json!(layers);
        if self.active_layer(frame) == id {
            self.board["frames"][frame]["active"] = json!(layers.last());
        }
        self.drop_unused_layers();
        self.mark_frame(frame);
        self.touch();
    }

    /// Move a layer up (+1) or down (-1) in a frame's stack.
    pub fn move_layer(&mut self, frame: usize, id: &str, dir: i32) {
        let mut layers = self.frame_layers(frame);
        let Some(i) = layers.iter().position(|x| x == id) else { return };
        let j = i as i32 + dir;
        if j < 0 || j as usize >= layers.len() {
            return;
        }
        layers.swap(i, j as usize);
        self.board["frames"][frame]["layers"] = json!(layers);
        self.mark_frame(frame);
        self.touch();
    }

    fn drop_unused_layers(&mut self) {
        let used: HashSet<String> = (0..self.frame_count()).flat_map(|i| self.frame_layers(i)).collect();
        let all: Vec<String> = self.board["layers"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        for id in all {
            if !used.contains(&id) {
                self.board["layers"].as_object_mut().unwrap().remove(&id);
                self.tiles.retain(|k, _| k.0 != id);
            }
        }
    }

    pub fn touch(&mut self) {
        self.board["modified"] = json!(now_ms());
    }

    /// Every frame showing this layer needs a new thumbnail.
    pub fn mark_layer(&mut self, layer: &str) {
        for i in 0..self.frame_count() {
            if self.frame_layers(i).iter().any(|x| x == layer) {
                let id = self.frame_id(i);
                self.changed_frames.insert(id);
            }
        }
    }

    pub fn mark_frame(&mut self, i: usize) {
        let id = self.frame_id(i);
        self.changed_frames.insert(id);
    }

    // ── tiles ──────────────────────────────────────────────────────────────

    pub fn ensure_decoded(&mut self, k: &TileKey) {
        if let Some(s) = self.tiles.get_mut(k) {
            if s.tile.is_none() {
                s.tile = s.png.as_deref().and_then(Tile::from_png);
                if s.tile.is_none() {
                    s.tile = Some(Tile::default());
                }
            }
        }
    }

    pub fn tile(&mut self, layer: &str, tx: i32, ty: i32) -> Option<&Tile> {
        let k = (layer.to_string(), tx, ty);
        if !self.tiles.contains_key(&k) {
            return None;
        }
        self.ensure_decoded(&k);
        self.tiles.get(&k).and_then(|s| s.tile.as_ref())
    }

    /// A tile to paint on (created empty when missing); marks it changed.
    pub fn tile_mut(&mut self, layer: &str, tx: i32, ty: i32) -> &mut Tile {
        let k = (layer.to_string(), tx, ty);
        self.ensure_decoded(&k);
        let slot = self.tiles.entry(k).or_insert_with(|| Slot { png: None, tile: Some(Tile::default()), dirty: true });
        slot.dirty = true;
        slot.png = None;
        slot.tile.get_or_insert_with(Tile::default)
    }

    /// Replace (or remove, with `None`) a tile — used by undo.
    pub fn set_tile(&mut self, layer: &str, tx: i32, ty: i32, t: Option<Tile>) {
        let k = (layer.to_string(), tx, ty);
        match t {
            Some(t) => {
                self.tiles.insert(k, Slot { png: None, tile: Some(t), dirty: true });
            }
            None => {
                self.tiles.remove(&k);
            }
        }
    }

    /// Tile coordinates a layer has pixels in.
    pub fn layer_tiles(&self, layer: &str) -> Vec<(i32, i32)> {
        self.tiles.keys().filter(|k| k.0 == layer).map(|k| (k.1, k.2)).collect()
    }

    pub fn reference_bytes(&self, path: &str) -> Option<Vec<u8>> {
        if path.starts_with("refs/") {
            self.refs.get(path).cloned()
        } else {
            std::fs::read(path).ok()
        }
    }

    // ── saving ─────────────────────────────────────────────────────────────

    /// The zip bytes; `thumb` makes a frame's thumbnail PNG (for changed frames).
    pub fn to_bytes(&mut self, mut thumb: impl FnMut(&mut BoardDoc, usize) -> Option<Vec<u8>>) -> Vec<u8> {
        // encode changed tiles, drop empty ones
        let keys: Vec<TileKey> = self.tiles.iter().filter(|(_, s)| s.dirty || s.png.is_none()).map(|(k, _)| k.clone()).collect();
        for k in keys {
            let empty = self.tiles.get(&k).and_then(|s| s.tile.as_ref()).is_none_or(|t| t.is_empty());
            if empty {
                self.tiles.remove(&k);
                continue;
            }
            if let Some(s) = self.tiles.get_mut(&k) {
                s.png = s.tile.as_ref().map(Tile::to_png);
                s.dirty = false;
            }
        }
        // thumbnails of changed frames, and the preview (first frame)
        let changed: Vec<usize> = (0..self.frame_count()).filter(|&i| self.changed_frames.contains(&self.frame_id(i)) || !self.thumbs.contains_key(&self.frame_id(i))).collect();
        for i in changed {
            if let Some(png) = thumb(self, i) {
                let id = self.frame_id(i);
                if i == 0 {
                    self.preview = Some(png.clone());
                }
                self.thumbs.insert(id, png);
            }
        }
        let live: HashSet<String> = (0..self.frame_count()).map(|i| self.frame_id(i)).collect();
        self.thumbs.retain(|k, _| live.contains(k));
        self.changed_frames.clear();
        let json = serde_json::to_vec(&self.board).unwrap_or_default();
        let mut names: Vec<(String, Vec<u8>)> = vec![("board.json".into(), json)];
        if let Some(p) = &self.preview {
            names.push(("preview.png".into(), p.clone()));
        }
        for (id, data) in &self.thumbs {
            names.push((format!("thumbs/{id}.png"), data.clone()));
        }
        let mut tiles: Vec<(&TileKey, &Slot)> = self.tiles.iter().collect();
        tiles.sort_by(|a, b| a.0.cmp(b.0));
        for (k, s) in tiles {
            if let Some(png) = &s.png {
                names.push((format!("tiles/{}/{}_{}.png", k.0, k.1, k.2), png.clone()));
            }
        }
        for (name, data) in &self.refs {
            names.push((name.clone(), data.clone()));
        }
        names.extend(self.other.iter().cloned());
        let refs: Vec<(String, &[u8])> = names.iter().map(|(n, d)| (n.clone(), d.as_slice())).collect();
        zip::write(&refs)
    }

    /// Write atomically (temp file + rename).
    pub fn save(&mut self, path: &Path, thumb: impl FnMut(&mut BoardDoc, usize) -> Option<Vec<u8>>) -> Result<(), String> {
        let bytes = self.to_bytes(thumb);
        let tmp = path.with_extension("storyboard-tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

/// Tile coordinates covering a canvas rectangle.
pub fn tiles_in(x0: f32, y0: f32, x1: f32, y1: f32) -> impl Iterator<Item = (i32, i32)> {
    let t = TILE as f32;
    let (a, b, c, d) = ((x0 / t).floor() as i32, (y0 / t).floor() as i32, ((x1 - 0.001) / t).floor() as i32, ((y1 - 0.001) / t).floor() as i32);
    (b..=d).flat_map(move |ty| (a..=c).map(move |tx| (tx, ty)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::tile::DabMode;

    #[test]
    fn new_edit_save_open() {
        let mut d = BoardDoc::new("animatic", 1280, 720, PAPER, 24);
        assert_eq!(d.frame_count(), 1);
        let layer = d.active_layer(0);
        d.tile_mut(&layer, 0, 0).dab(50.0, 50.0, 8.0, 1.0, [10, 20, 30], 1.0, DabMode::Over);
        d.board["custom"] = json!({ "kept": true });
        d.add_frame(1);
        d.duplicate_frame(0);
        assert_eq!(d.frame_count(), 3);
        let bytes = d.to_bytes(|_, _| Some(vec![1, 2, 3]));
        let mut back = BoardDoc::from_bytes(&bytes).unwrap();
        assert_eq!(back.frame_count(), 3);
        assert_eq!(back.board["custom"]["kept"], true);
        assert_eq!(back.width(), 1280);
        let t = back.tile(&layer, 0, 0).unwrap();
        assert_eq!(t.px[(50 * TILE + 50) * 4 + 3], 255);
        // the duplicate has its own layer with the same pixels
        let dup_layer = back.active_layer(1);
        assert_ne!(dup_layer, layer);
        assert!(back.tile(&dup_layer, 0, 0).is_some());
        assert_eq!(back.thumbs.len(), 3);
        assert!(back.preview.is_some());
    }
}
