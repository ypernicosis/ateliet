//! Moodboards in image mode: the boards of the library as cards, and a board
//! as an infinite canvas of references, notes, colour swatches and frames
//! (Pictogrep's Moodboard plugin, now part of Ateliet; the documents, their
//! format and where they live are in `ateliet_core::moodboard`).
//!
//! Two looks share one canvas: "Estúdio" floats the tools in islands over
//! the board, "Foco" hides them so only the references remain (PureRef).

use super::ImagesState;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::moodboard::{self as mb, Board, Cam, CropStage, Guide, Item, Kind, Meta, Rect, Store};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

const MIN_Z: f64 = 0.03;
const MAX_Z: f64 = 8.0;
const TRAY_W: f32 = 300.0;
const ISLAND: f32 = 44.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Select,
    Hand,
    Note,
    Swatch,
    Frame,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Save {
    Saved,
    Pending,
    Error,
}

/// A drag in progress on the canvas.
enum Op {
    Pan { p0: Pos2, cam0: Cam },
    Marquee { p0: Pos2, base: HashSet<String> },
    Frame { p0: Pos2 },
    Move { start: (f64, f64), p0: Pos2, moved: bool, before: Option<String>, orig: Vec<(String, f64, f64)>, bb0: Rect, others: Vec<Rect>, locked_only: bool },
    Rotate { id: String, c: (f64, f64), a0: f64, r0: f64, before: String, changed: bool },
    Resize1 { id: String, sx: f64, sy: f64, a: (f64, f64), w0: f64, h0: f64, before: String, changed: bool },
    ResizeN { orig: Vec<(String, f64, f64, f64, f64, Option<f64>)>, a: (f64, f64), bb: Rect, sx: f64, sy: f64, before: String, changed: bool },
    Crop { k: &'static str, start: (f64, f64), rect0: Rect },
    Mini,
}

/// A note or a frame title being typed.
struct Edit {
    id: String,
    note: bool,
    is_new: bool,
    before: String,
    buf: String,
    focused: bool,
}

/// What a canvas handle does.
#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Corner(f64, f64),
    Rotate,
}

#[derive(Default)]
struct Tray {
    query: String,
    /// "" all; "c:<name>" a shared collection; "f:<folder>" a folder.
    tag: String,
    picked: Vec<usize>,
    shuffled: Option<Vec<usize>>,
}

pub struct Moodboards {
    /// Moodboards are showing (instead of the mosaic).
    pub open: bool,
    /// Tab, taken from egui's focus walking by the app (studio ⇄ focus).
    pub tab_pressed: bool,
    store: Option<Store>,
    store_for: Option<(std::path::PathBuf, Option<std::path::PathBuf>)>,
    pub boards: Vec<Meta>,
    prefs: serde_json::Value,
    board: Option<Board>,
    cam: Cam,
    sel: HashSet<String>,
    tool: Tool,
    op: Option<Op>,
    undo: Vec<String>,
    redo: Vec<String>,
    clip: Vec<Item>,
    edit: Option<Edit>,
    guides: Vec<Guide>,
    marquee: Option<(Pos2, Pos2)>,
    crop: Option<CropStage>,
    tray: Tray,
    tray_open: bool,
    save: Save,
    dirty_at: Option<Instant>,
    /// Pictogrep image id → library index.
    ids: HashMap<String, usize>,
    ids_gen: u64,
    /// The view rect (screen) of the last frame, for fitting from menus.
    view: egui::Rect,
    fit_pending: bool,
    zoomed_to: Option<String>,
    anim: Option<(Cam, Cam, Instant)>,
    last_nudge: Option<Instant>,
    menu_hit: Option<Option<String>>,
    menu_at: (f64, f64),
    look_open: bool,
    help_open: bool,
    name_buf: String,
    rename: Option<(String, String)>,
    confirm_delete: Option<Meta>,
    filter: String,
    color_before: Option<(String, String)>,
    /// Pictures already filed in the board's collection (by image id).
    filed: HashSet<String>,
    filed_for: String,
    pending_add: Vec<(String, Vec<usize>)>,
    mini: Option<(f64, f64, f64)>,
}

impl Default for Moodboards {
    fn default() -> Self {
        Moodboards {
            open: false,
            tab_pressed: false,
            store: None,
            store_for: None,
            boards: Vec::new(),
            prefs: serde_json::json!({}),
            board: None,
            cam: Cam { x: 0.0, y: 0.0, z: 1.0 },
            sel: HashSet::new(),
            tool: Tool::Select,
            op: None,
            undo: Vec::new(),
            redo: Vec::new(),
            clip: Vec::new(),
            edit: None,
            guides: Vec::new(),
            marquee: None,
            crop: None,
            tray: Tray::default(),
            tray_open: true,
            save: Save::Saved,
            dirty_at: None,
            ids: HashMap::new(),
            ids_gen: u64::MAX,
            view: egui::Rect::NOTHING,
            fit_pending: false,
            zoomed_to: None,
            anim: None,
            last_nudge: None,
            menu_hit: None,
            menu_at: (0.0, 0.0),
            look_open: false,
            help_open: false,
            name_buf: String::new(),
            rename: None,
            confirm_delete: None,
            filter: String::new(),
            color_before: None,
            filed: HashSet::new(),
            filed_for: String::new(),
            pending_add: Vec::new(),
            mini: None,
        }
    }
}

impl Drop for Moodboards {
    fn drop(&mut self) {
        self.flush();
    }
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn hex_color(s: &str) -> Color32 {
    mb::parse_hex(s).map(rgb).unwrap_or(Color32::from_rgb(0xc8, 0xa2, 0x7a))
}

fn ago(ms: i64) -> String {
    let s = ((mb::now_ms() - ms) / 1000).max(0);
    let (n, one, many) = match s {
        s if s < 60 => return "agora".into(),
        s if s < 3600 => (s / 60, "minuto", "minutos"),
        s if s < 86400 => (s / 3600, "hora", "horas"),
        s if s < 604800 => (s / 86400, "dia", "dias"),
        s if s < 2592000 => (s / 604800, "semana", "semanas"),
        s if s < 31536000 => (s / 2592000, "mês", "meses"),
        s => (s / 31536000, "ano", "anos"),
    };
    format!("há {n} {}", if n == 1 { one } else { many })
}

/// Shows a file in the system's file manager.
fn reveal(path: &Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = open::that(path.parent().unwrap_or(path));
}

impl Moodboards {
    pub fn board_open(&self) -> bool {
        self.board.is_some()
    }

    /// Saves now what is waiting to be saved.
    pub fn flush(&mut self) {
        if self.dirty_at.is_none() {
            return;
        }
        self.dirty_at = None;
        let (Some(store), Some(b)) = (&self.store, &mut self.board) else { return };
        b.cam = Some(self.cam);
        match store.save(b) {
            Ok(()) => {
                self.save = Save::Saved;
                let meta = b.meta();
                match self.boards.iter_mut().find(|m| m.id == meta.id) {
                    Some(m) => *m = meta,
                    None => self.boards.push(meta),
                }
                self.boards.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
            }
            Err(_) => self.save = Save::Error,
        }
    }

    fn mark(&mut self, quiet: bool) {
        if let Some(b) = &mut self.board {
            if !quiet {
                b.updated_at = mb::now_ms();
                self.save = Save::Pending;
            }
            if self.dirty_at.is_none() || !quiet {
                self.dirty_at = Some(Instant::now());
            }
        }
    }

    fn snapshot(&self) -> String {
        self.board.as_ref().map(|b| serde_json::to_string(&b.items).unwrap_or_default()).unwrap_or_default()
    }

    fn push_history(&mut self, before: String) {
        self.undo.push(before);
        if self.undo.len() > 150 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, json: &str) {
        if let (Some(b), Ok(items)) = (&mut self.board, serde_json::from_str::<Vec<Item>>(json)) {
            b.items = items;
            let ids: HashSet<String> = b.items.iter().map(|i| i.id.clone()).collect();
            self.sel.retain(|s| ids.contains(s));
        }
        self.mark(false);
    }

    fn undo(&mut self) {
        if self.crop.take().is_some() {
            return;
        }
        self.commit_edit();
        if let Some(prev) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            self.restore(&prev);
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            self.restore(&next);
        }
    }

    fn items(&self) -> &[Item] {
        self.board.as_ref().map(|b| b.items.as_slice()).unwrap_or(&[])
    }

    fn item(&self, id: &str) -> Option<&Item> {
        self.items().iter().find(|i| i.id == id)
    }

    fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        self.board.as_mut()?.items.iter_mut().find(|i| i.id == id)
    }

    fn selected(&self) -> Vec<&Item> {
        self.items().iter().filter(|i| self.sel.contains(&i.id)).collect()
    }

    fn one(&self) -> Option<Item> {
        let s = self.selected();
        (s.len() == 1).then(|| s[0].clone())
    }

    // ── camera ────────────────────────────────────────────────────────────

    fn to_screen(&self, origin: Pos2, p: (f64, f64)) -> Pos2 {
        origin + egui::vec2((p.0 * self.cam.z + self.cam.x) as f32, (p.1 * self.cam.z + self.cam.y) as f32)
    }

    fn to_world(&self, origin: Pos2, s: Pos2) -> (f64, f64) {
        let v = s - origin;
        ((v.x as f64 - self.cam.x) / self.cam.z, (v.y as f64 - self.cam.y) / self.cam.z)
    }

    /// The part of the canvas the islands leave free (screen, canvas-relative).
    fn free_rect(&self, canvas: egui::Rect) -> egui::Rect {
        let studio = self.board.as_ref().is_some_and(|b| b.mode != "focus");
        let left = if studio && self.tray_open && canvas.width() > 760.0 { TRAY_W + 28.0 } else { 0.0 };
        let (top, bottom) = if studio { (ISLAND + 12.0, ISLAND + 12.0) } else { (0.0, 0.0) };
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(canvas.width(), (canvas.height() - bottom).max(top + 100.0)))
    }

    fn view_center_world(&self) -> (f64, f64) {
        let fr = self.free_rect(self.view);
        let c = fr.center();
        ((c.x as f64 - self.cam.x) / self.cam.z, (c.y as f64 - self.cam.y) / self.cam.z)
    }

    fn cam_for_zoom(&self, at: Pos2, z: f64) -> Cam {
        let z = z.clamp(MIN_Z, MAX_Z);
        Cam { x: at.x as f64 - (at.x as f64 - self.cam.x) * z / self.cam.z, y: at.y as f64 - (at.y as f64 - self.cam.y) * z / self.cam.z, z }
    }

    fn zoom_at(&mut self, at: Pos2, z: f64) {
        self.cam = self.cam_for_zoom(at, z);
        self.anim = None;
        self.zoomed_to = None;
        self.mark(true);
    }

    fn animate(&mut self, to: Cam) {
        self.anim = Some((self.cam, to, Instant::now()));
    }

    fn zoom_by(&mut self, f: f64) {
        let c = self.free_rect(self.view).center();
        let to = self.cam_for_zoom(c, self.cam.z * f);
        self.animate(to);
    }

    fn fit(&mut self, bb: Rect, max_z: f64, animate: bool) {
        let fr = self.free_rect(self.view);
        let pad = 48.0;
        let z = if bb.w > 0.0 && bb.h > 0.0 { ((fr.width() as f64 - pad * 2.0) / bb.w).min((fr.height() as f64 - pad * 2.0) / bb.h).clamp(MIN_Z, max_z) } else { 1.0 };
        let c = fr.center();
        let to = Cam { z, x: c.x as f64 - (bb.x + bb.w / 2.0) * z, y: c.y as f64 - (bb.y + bb.h / 2.0) * z };
        if animate {
            self.animate(to);
        } else {
            self.cam = to;
        }
    }

    fn fit_all(&mut self) {
        if self.items().is_empty() {
            let c = self.view.size() / 2.0;
            self.animate(Cam { x: c.x as f64, y: c.y as f64, z: 1.0 });
        } else {
            let bb = mb::bbox(self.items());
            self.fit(bb, 2.0, true);
        }
        self.zoomed_to = None;
    }

    fn fit_selection(&mut self) {
        let s = self.selected();
        if s.is_empty() {
            self.fit_all();
        } else {
            let bb = mb::bbox(s);
            self.fit(bb, 8.0, true);
        }
    }

    // ── board actions ─────────────────────────────────────────────────────

    fn add_items(&mut self, items: Vec<Item>, select: bool) {
        let ids: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();
        if let Some(b) = &mut self.board {
            b.items.extend(items);
        }
        if select {
            self.sel = ids;
        }
        self.mark(false);
    }

    fn set_prop(&mut self, f: impl Fn(&mut Item)) {
        if self.sel.is_empty() {
            return;
        }
        let before = self.snapshot();
        self.push_history(before);
        let sel = self.sel.clone();
        if let Some(b) = &mut self.board {
            for it in b.items.iter_mut().filter(|i| sel.contains(&i.id)) {
                f(it);
            }
        }
        self.mark(false);
    }

    fn toggle_image(&mut self, gray: bool) {
        let imgs: Vec<&Item> = self.selected().into_iter().filter(|i| i.kind == Kind::Image).collect();
        if imgs.is_empty() {
            return;
        }
        let on = !imgs.iter().all(|i| if gray { i.gray } else { i.flip });
        self.set_prop(|i| {
            if i.kind == Kind::Image {
                if gray {
                    i.gray = on;
                } else {
                    i.flip = on;
                }
            }
        });
    }

    fn scale_text(&mut self, f: f64) {
        if !self.selected().iter().any(|i| i.kind == Kind::Note) {
            return;
        }
        self.set_prop(|i| {
            if i.kind == Kind::Note {
                i.fs = Some((i.fs.unwrap_or(20.0) * f).max(2.0));
                i.w *= f;
            }
        });
    }

    fn toggle_lock(&mut self) {
        let lock = !self.selected().iter().all(|i| i.locked);
        self.set_prop(|i| i.locked = lock);
    }

    fn delete_selection(&mut self, notice: &mut Option<String>) {
        let gone: HashSet<String> = self.selected().iter().filter(|i| !i.locked).map(|i| i.id.clone()).collect();
        if gone.is_empty() {
            if !self.sel.is_empty() {
                *notice = Some("Itens travados: destrave com Shift+L".into());
            }
            return;
        }
        let before = self.snapshot();
        self.push_history(before);
        if let Some(b) = &mut self.board {
            b.items.retain(|i| !gone.contains(&i.id));
        }
        self.sel.retain(|s| !gone.contains(s));
        self.mark(false);
    }

    fn duplicate(&mut self) {
        let sel: Vec<Item> = self.selected().into_iter().cloned().collect();
        if sel.is_empty() {
            return;
        }
        let before = self.snapshot();
        self.push_history(before);
        let off = 24.0 / self.cam.z;
        self.add_items(sel.iter().map(|i| i.cloned(off, off)).collect(), true);
    }

    fn copy(&mut self, notice: &mut Option<String>) {
        let sel: Vec<Item> = self.selected().into_iter().cloned().collect();
        if !sel.is_empty() {
            self.clip = sel;
            *notice = Some("Copiado".into());
        }
    }

    fn paste(&mut self, at: (f64, f64)) {
        if self.clip.is_empty() {
            return;
        }
        let before = self.snapshot();
        self.push_history(before);
        let bb = mb::bbox(&self.clip);
        let (dx, dy) = (at.0 - (bb.x + bb.w / 2.0), at.1 - (bb.y + bb.h / 2.0));
        let items = self.clip.iter().map(|i| i.cloned(dx, dy)).collect();
        self.add_items(items, true);
    }

    /// "front", "back", "forward", "backward".
    fn reorder(&mut self, kind: &str) {
        if self.sel.is_empty() {
            return;
        }
        let before = self.snapshot();
        self.push_history(before);
        let sel = self.sel.clone();
        let Some(b) = &mut self.board else { return };
        let is = |i: &Item| sel.contains(&i.id);
        match kind {
            "front" => {
                let (s, o): (Vec<Item>, Vec<Item>) = b.items.drain(..).partition(|i| is(i));
                b.items = o.into_iter().chain(s).collect();
            }
            "back" => {
                let (s, o): (Vec<Item>, Vec<Item>) = b.items.drain(..).partition(|i| is(i));
                b.items = s.into_iter().chain(o).collect();
            }
            "forward" => {
                for i in (0..b.items.len().saturating_sub(1)).rev() {
                    if is(&b.items[i]) && !is(&b.items[i + 1]) {
                        b.items.swap(i, i + 1);
                    }
                }
            }
            _ => {
                for i in 1..b.items.len() {
                    if is(&b.items[i]) && !is(&b.items[i - 1]) {
                        b.items.swap(i, i - 1);
                    }
                }
            }
        }
        self.mark(false);
    }

    fn arrange(&mut self, all: bool) {
        let before = self.snapshot();
        let sel = self.sel.clone();
        let Some(b) = &mut self.board else { return };
        let which: Vec<usize> = (0..b.items.len()).filter(|&i| all || sel.contains(&b.items[i].id)).collect();
        if mb::arrange(&mut b.items, &which) {
            self.push_history(before);
            self.mark(false);
        }
    }

    fn select_all(&mut self) {
        self.sel = self.items().iter().map(|i| i.id.clone()).collect();
    }

    fn create_note(&mut self, w: (f64, f64)) {
        let z = self.cam.z;
        let fill = self.prefs["lastFill"].as_str().map(String::from);
        let it = Item::note(w.0, w.1 - 14.0 / z, if fill.is_some() { 240.0 } else { 300.0 } / z, 30.0 / z, ((20.0 / z) * 100.0).round() / 100.0, fill);
        let before = self.snapshot();
        let id = it.id.clone();
        if let Some(b) = &mut self.board {
            b.items.push(it);
        }
        self.sel = HashSet::from([id.clone()]);
        self.edit = Some(Edit { id, note: true, is_new: true, before, buf: String::new(), focused: false });
    }

    fn create_swatch(&mut self, w: (f64, f64), color: Option<String>) -> String {
        let z = self.cam.z;
        let color = color.or_else(|| self.prefs["lastColor"].as_str().map(String::from)).unwrap_or_else(|| "#c8a27a".into());
        let it = Item::swatch(&color, w.0 - 60.0 / z, w.1 - 75.0 / z, 120.0 / z, 150.0 / z);
        let id = it.id.clone();
        let before = self.snapshot();
        self.push_history(before);
        self.add_items(vec![it], true);
        id
    }

    fn create_frame(&mut self, r: Rect) {
        let n = self.items().iter().filter(|i| i.kind == Kind::Frame).count() + 1;
        let it = Item::frame(&format!("Seção {n}"), r.x, r.y, r.w, r.h);
        let before = self.snapshot();
        self.push_history(before.clone());
        let id = it.id.clone();
        // frames sit beneath what they hold: the back of the stack
        if let Some(b) = &mut self.board {
            b.items.insert(0, it);
        }
        self.sel = HashSet::from([id.clone()]);
        self.mark(false);
        self.start_edit(&id);
    }

    fn start_edit(&mut self, id: &str) {
        let Some(it) = self.item(id).cloned() else { return };
        if it.locked || !matches!(it.kind, Kind::Note | Kind::Frame) {
            return;
        }
        let before = self.snapshot();
        let buf = if it.kind == Kind::Note { it.text().to_string() } else { it.title().to_string() };
        self.sel = HashSet::from([it.id.clone()]);
        self.edit = Some(Edit { id: it.id.clone(), note: it.kind == Kind::Note, is_new: false, before, buf, focused: false });
    }

    fn commit_edit(&mut self) {
        let Some(ed) = self.edit.take() else { return };
        let Some(cur) = self.item(&ed.id).cloned() else { return };
        if ed.note {
            let text = ed.buf.trim_end_matches('\n').to_string();
            if text.trim().is_empty() {
                if let Some(b) = &mut self.board {
                    b.items.retain(|i| i.id != ed.id);
                }
                self.sel.remove(&ed.id);
                if !ed.is_new {
                    self.push_history(ed.before);
                }
            } else if Some(&text) != cur.text.as_ref() || ed.is_new {
                if let Some(it) = self.item_mut(&ed.id) {
                    it.text = Some(text);
                }
                self.push_history(ed.before);
            }
        } else {
            let text = ed.buf.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() && text != cur.title() {
                if let Some(it) = self.item_mut(&ed.id) {
                    it.title = Some(text);
                }
                self.push_history(ed.before);
            }
        }
        self.mark(false);
    }

    fn start_crop(&mut self, id: &str, known: bool) {
        let Some(it) = self.item(id).cloned() else { return };
        if it.kind != Kind::Image || it.locked || !known {
            return;
        }
        self.commit_edit();
        self.apply_crop();
        self.crop = Some(CropStage::start(&it));
        self.sel = HashSet::from([it.id]);
    }

    fn apply_crop(&mut self) {
        let Some(st) = self.crop.take() else { return };
        let before = self.snapshot();
        let changed = match self.item_mut(&st.id) {
            Some(it) => st.apply(it),
            None => false,
        };
        if changed {
            self.push_history(before);
            self.mark(false);
        }
    }

    fn remove_crop(&mut self, id: &str) {
        self.apply_crop();
        if self.item(id).is_none_or(|i| i.crop.is_none()) {
            return;
        }
        let before = self.snapshot();
        self.push_history(before);
        if let Some(it) = self.item_mut(id) {
            mb::remove_crop(it);
        }
        self.mark(false);
    }

    fn prefs_set(&mut self, key: &str, v: serde_json::Value) {
        if !self.prefs.is_object() {
            self.prefs = serde_json::json!({});
        }
        self.prefs[key] = v;
        if let Some(s) = &self.store {
            let _ = s.set_prefs(&self.prefs);
        }
    }

    fn set_mode(&mut self, focus: bool, notice: &mut Option<String>) {
        let Some(b) = &mut self.board else { return };
        b.mode = if focus { "focus" } else { "studio" }.into();
        if focus {
            self.look_open = false;
            *notice = Some("Modo foco: Tab volta ao estúdio".into());
        } else {
            self.tray_open = self.prefs["trayOpen"].as_bool().unwrap_or(true);
        }
        self.prefs_set("lastMode", serde_json::json!(if focus { "focus" } else { "studio" }));
        self.mark(true);
    }

    fn toggle_tray(&mut self) {
        self.tray_open = !self.tray_open;
        if self.board.as_ref().is_some_and(|b| b.mode == "studio") {
            let v = serde_json::json!(self.tray_open);
            self.prefs_set("trayOpen", v);
        }
    }

    fn open_board(&mut self, id: &str) {
        self.flush();
        let Some(store) = &self.store else { return };
        let Some(b) = store.load(id) else { return };
        self.reset_board_state();
        self.name_buf = b.name.clone();
        self.tray_open = self.prefs["trayOpen"].as_bool().unwrap_or(true) && b.mode == "studio";
        match b.cam.filter(|c| c.z.is_finite() && c.z > 0.0) {
            Some(c) => self.cam = c,
            None => self.fit_pending = true,
        }
        self.board = Some(b);
        self.open = true;
    }

    fn reset_board_state(&mut self) {
        self.sel.clear();
        self.undo.clear();
        self.redo.clear();
        self.edit = None;
        self.crop = None;
        self.op = None;
        self.zoomed_to = None;
        self.tray.picked.clear();
        self.anim = None;
        self.look_open = false;
        self.filed.clear();
        self.filed_for.clear();
        self.tool = Tool::Select;
    }

    fn leave_board(&mut self) {
        self.commit_edit();
        self.apply_crop();
        self.flush();
        self.board = None;
    }

    fn new_board(&mut self) {
        let Some(store) = &self.store else { return };
        let p = &self.prefs;
        let b = Board::new(mb::UNTITLED, p["lastMode"].as_str().unwrap_or("studio"), p["lastBg"].as_str().unwrap_or("paper"), p["lastPattern"].as_str().unwrap_or("dots"));
        if store.save(&b).is_ok() {
            self.boards.insert(0, b.meta());
            let id = b.id.clone();
            self.open_board(&id);
            // the name selected, ready to type
            self.rename = Some((String::from("\u{0}board"), String::new()));
        }
    }

    fn duplicate_board(&mut self, id: &str) {
        let Some(store) = &self.store else { return };
        let Some(src) = store.load(id) else { return };
        let now = mb::now_ms();
        let copy = Board { id: mb::uid(), name: format!("{} (cópia)", src.name), created_at: now, updated_at: now, ..src };
        if store.save(&copy).is_ok() {
            self.boards = store.boards();
        }
    }
}

// ── the parts that need the library (pictures, previews) ──────────────────

impl ImagesState {
    /// The store follows the shared library setting; read again on change.
    fn mb_ready(&mut self) {
        let key = (self.root.clone(), self.shared.as_ref().map(|h| h.root.clone()));
        if self.mb.store_for.as_ref() != Some(&key) {
            self.mb.flush();
            let store = Store::locate(self.shared.as_ref(), &self.root);
            self.mb.boards = store.boards();
            self.mb.prefs = store.prefs();
            self.mb.store = Some(store);
            self.mb.store_for = Some(key);
            self.mb.board = None;
        }
        if self.mb.ids_gen != self.entries_gen {
            // every spelling a board may have saved, so boards keep their
            // pictures however the library folder was reached
            self.mb.ids = self.entries.iter().enumerate().flat_map(|(i, e)| ateliet_core::pictogrep::image_ids(self.shared.as_ref(), Path::new(&e.abs)).into_iter().map(move |id| (id, i))).collect();
            self.mb.ids_gen = self.entries_gen;
        }
        // pictures sent from the mosaic to a board
        for (board, idx) in std::mem::take(&mut self.mb.pending_add) {
            self.add_to_board(&board, &idx);
        }
    }

    /// Opens the moodboards (the list, or a board by id).
    pub fn show_moodboards(&mut self, board: Option<&str>) {
        self.mb_ready();
        self.mb.open = true;
        self.viewer = None;
        match board {
            Some(id) => self.mb.open_board(id),
            None => self.mb.leave_board(),
        }
    }

    pub fn close_moodboards(&mut self) {
        self.mb.leave_board();
        self.mb.open = false;
    }

    pub fn moodboard_list(&mut self) -> Vec<Meta> {
        self.mb_ready();
        self.mb.boards.clone()
    }

    pub fn open_board_id(&self) -> Option<String> {
        self.mb.board.as_ref().filter(|_| self.mb.open).map(|b| b.id.clone())
    }

    /// Pictures from the mosaic put on a board (beside what is there).
    pub fn add_to_board(&mut self, board_id: &str, idx: &[usize]) {
        let aspects: Vec<f64> = idx.iter().map(|&i| super::aspect(&self.entries[i]) as f64).collect();
        let ids: Vec<String> = idx.iter().map(|&i| ateliet_core::pictogrep::image_id(Path::new(&self.entries[i].abs))).collect();
        let open = self.mb.board.as_ref().is_some_and(|b| b.id == board_id);
        let mut b = if open { self.mb.board.take().unwrap() } else {
            match self.mb.store.as_ref().and_then(|s| s.load(board_id)) {
                Some(b) => b,
                None => return,
            }
        };
        let z = b.cam.map(|c| c.z).unwrap_or(1.0);
        let at = if b.items.is_empty() { (0.0, 0.0) } else {
            let bb = mb::bbox(&b.items);
            (bb.x + bb.w + 400.0 / z, bb.y + bb.h / 2.0)
        };
        let boxes = mb::place(&aspects, at, 320.0 / z, 220.0 / z, 14.0 / z);
        let before = serde_json::to_string(&b.items).unwrap_or_default();
        for (r, id) in boxes.iter().zip(&ids) {
            b.items.push(Item::image(id, r.x, r.y, r.w, r.h));
        }
        b.updated_at = mb::now_ms();
        let name = b.name.clone();
        if open {
            self.mb.board = Some(b);
            self.mb.push_history(before);
            self.mb.mark(false);
        } else if let Some(s) = &self.mb.store {
            if s.save(&b).is_ok() {
                self.mb.boards = s.boards();
            }
        }
        self.notice = Some(format!("{} no moodboard {name}", if idx.len() == 1 { "1 imagem".to_string() } else { format!("{} imagens", idx.len()) }));
    }

    /// Queues pictures for a board (from a menu drawn while borrowing self).
    pub fn queue_add_to_board(&mut self, board_id: &str, idx: Vec<usize>) {
        self.mb.pending_add.push((board_id.to_string(), idx));
    }

    /// Files the board's pictures in its "Moodboards" collection of the
    /// shared library (adding only; taking out is done in Pictogrep).
    fn file_board_pictures(&mut self) {
        let Some(home) = self.shared.clone() else { return };
        let Some(b) = &self.mb.board else { return };
        if !b.tags_on() {
            return;
        }
        let name = mb::board_collection(&b.name);
        if self.mb.filed_for != name {
            self.mb.filed_for = name.clone();
            self.mb.filed.clear();
        }
        let todo: Vec<(String, usize)> = b.items.iter().filter_map(|i| i.image_id.clone()).filter(|id| !self.mb.filed.contains(id)).filter_map(|id| self.mb.ids.get(&id).map(|&i| (id, i))).collect();
        if todo.is_empty() {
            return;
        }
        let mut paths = Vec::new();
        for (id, i) in todo {
            let abs = std::path::PathBuf::from(&self.entries[i].abs);
            if ateliet_core::pictogrep::link(&home, &name, &abs).is_ok() {
                paths.push(abs);
            }
            self.mb.filed.insert(id);
        }
        if !paths.is_empty() {
            ateliet_core::pictogrep::library_changed(&home, &paths);
            self.reload_collections();
        }
    }

    /// The moodboards in the centre of image mode.
    pub fn moodboard_ui(&mut self, ui: &mut egui::Ui, p: Palette) {
        self.mb_ready();
        if let Some(t) = self.mb.dirty_at {
            if t.elapsed() > Duration::from_millis(500) {
                self.mb.flush();
                self.file_board_pictures();
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(520));
            }
        }
        if self.mb.board.is_some() {
            self.board_ui(ui, p);
        } else {
            self.home_ui(ui, p);
        }
    }

    // ── the list of boards ────────────────────────────────────────────────

    fn home_ui(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Moodboards").font(kit::medium(20.0)).color(p.text));
                ui.label(RichText::new("Quadros de referência com as imagens da sua biblioteca.").size(12.5).color(p.muted));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.mb.boards.is_empty() && kit::primary_icon(ui, icons::PLUS, "Novo moodboard", p).clicked() {
                    self.mb.new_board();
                }
                if self.mb.boards.len() >= 7 {
                    ui.allocate_ui(egui::vec2(220.0, 30.0), |ui| kit::search_field(ui, &mut self.mb.filter, "Filtrar", p));
                }
            });
        });
        ui.add_space(18.0);
        if self.mb.boards.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(70.0);
                ui.label(kit::icon(icons::LAYOUT_DASHBOARD, 34.0).color(p.faint));
                ui.add_space(14.0);
                ui.label(RichText::new("Comece seu primeiro moodboard").font(kit::medium(16.0)).color(p.text));
                ui.add_space(4.0);
                ui.label(RichText::new("Arraste imagens da biblioteca para um quadro infinito, anote ideias e separe tudo em seções.").size(12.5).color(p.muted));
                ui.add_space(14.0);
                if kit::primary_icon(ui, icons::PLUS, "Novo moodboard", p).clicked() {
                    self.mb.new_board();
                }
            });
            return;
        }
        let q = self.mb.filter.trim().to_lowercase();
        let list: Vec<Meta> = self.mb.boards.iter().filter(|b| q.is_empty() || b.name.to_lowercase().contains(&q)).cloned().collect();
        let gap = 18.0;
        let avail = ui.available_width() - 4.0;
        let cols = ((avail + gap) / (240.0 + gap)).floor().max(1.0) as usize;
        let card_w = (avail - gap * (cols as f32 - 1.0)) / cols as f32;
        let cover_h = card_w * 0.75;
        let mut open: Option<String> = None;
        let mut dup: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for row in list.chunks(cols) {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for meta in row {
                        let (rect, r) = ui.allocate_exact_size(egui::vec2(card_w, cover_h + 52.0), Sense::click());
                        let cover = egui::Rect::from_min_size(rect.min, egui::vec2(card_w, cover_h));
                        let bg = mb::background(&meta.bg);
                        ui.painter().rect_filled(cover, 10.0, if meta.cover.is_empty() { rgb(bg.color) } else { p.bg_secondary });
                        let ids: Vec<usize> = meta.cover.iter().filter_map(|id| self.mb.ids.get(id).copied()).collect();
                        let cells: Vec<egui::Rect> = match ids.len() {
                            0 => vec![],
                            1 => vec![cover],
                            2 => { let w = (cover.width() - 3.0) / 2.0; (0..2).map(|i| egui::Rect::from_min_size(cover.min + egui::vec2(i as f32 * (w + 3.0), 0.0), egui::vec2(w, cover.height()))).collect() }
                            3 => {
                                let w = (cover.width() - 3.0) / 2.0;
                                let h = (cover.height() - 3.0) / 2.0;
                                vec![egui::Rect::from_min_size(cover.min, egui::vec2(w, cover.height())), egui::Rect::from_min_size(cover.min + egui::vec2(w + 3.0, 0.0), egui::vec2(w, h)), egui::Rect::from_min_size(cover.min + egui::vec2(w + 3.0, h + 3.0), egui::vec2(w, h))]
                            }
                            _ => { let w = (cover.width() - 3.0) / 2.0; let h = (cover.height() - 3.0) / 2.0; (0..4).map(|i| egui::Rect::from_min_size(cover.min + egui::vec2((i % 2) as f32 * (w + 3.0), (i / 2) as f32 * (h + 3.0)), egui::vec2(w, h))).collect() }
                        };
                        let ppp = ui.ctx().pixels_per_point();
                        for (cell, &i) in cells.iter().zip(&ids) {
                            if let Some(t) = self.thumb_sized(i, cell.width().max(cell.height()) * ppp, false, false) {
                                let e = &self.entries[i];
                                let k = (e.width.max(1) as f32 / e.height.max(1) as f32) / (cell.width() / cell.height());
                                let uv = if k > 1.0 { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0 / k, 1.0)) } else { egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(1.0, k)) };
                                let cr = egui::CornerRadius { nw: if cell.min == cover.min { 10 } else { 0 }, ne: if (cell.right() - cover.right()).abs() < 0.5 && (cell.top() - cover.top()).abs() < 0.5 { 10 } else { 0 }, sw: if (cell.left() - cover.left()).abs() < 0.5 && (cell.bottom() - cover.bottom()).abs() < 0.5 { 10 } else { 0 }, se: if cell.max == cover.max { 10 } else { 0 } };
                                ui.painter().add(egui::epaint::RectShape::filled(*cell, cr, Color32::WHITE).with_texture(t, uv));
                            }
                        }
                        if ids.is_empty() {
                            ui.painter().text(cover.center(), egui::Align2::CENTER_CENTER, icons::IMAGES, kit::icon_font(26.0), if bg.dark { Color32::from_gray(120) } else { Color32::from_gray(150) });
                        }
                        if r.hovered() {
                            ui.painter().rect_stroke(cover, 10.0, Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                        }
                        if meta.mode == "focus" {
                            let badge = egui::Rect::from_min_size(cover.min + egui::vec2(10.0, 10.0), egui::vec2(52.0, 20.0));
                            ui.painter().rect_filled(badge, 10.0, Color32::from_rgba_unmultiplied(20, 20, 24, 160));
                            ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, "Foco", egui::FontId::proportional(11.0), Color32::WHITE);
                        }
                        // name (renamed in place) and what's in it
                        let name_pos = egui::pos2(rect.left() + 2.0, cover.bottom() + 10.0);
                        let renaming = self.mb.rename.as_ref().is_some_and(|(id, _)| *id == meta.id);
                        if renaming {
                            let nr = egui::Rect::from_min_size(name_pos - egui::vec2(2.0, 2.0), egui::vec2(card_w - 30.0, 22.0));
                            let (_, buf) = self.mb.rename.as_mut().unwrap();
                            let te = ui.put(nr, egui::TextEdit::singleline(buf).font(kit::medium(14.0)).margin(egui::Margin::symmetric(4, 2)));
                            let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                            if !(enter || esc || te.lost_focus() || te.clicked_elsewhere()) {
                                te.request_focus();
                            } else {
                                let (id, text) = self.mb.rename.take().unwrap();
                                let text = text.trim().to_string();
                                if !esc && !text.is_empty() && text != meta.name {
                                    if let Some(s) = &self.mb.store {
                                        if let Some(mut b) = s.load(&id) {
                                            b.name = text;
                                            b.updated_at = mb::now_ms();
                                            let _ = s.save(&b);
                                            self.mb.boards = s.boards();
                                        }
                                    }
                                }
                            }
                        } else {
                            let mut job = egui::text::LayoutJob::single_section(meta.name.clone(), egui::TextFormat { font_id: kit::medium(14.0), color: p.text, ..Default::default() });
                            job.wrap = egui::text::TextWrapping::truncate_at_width(card_w - 36.0);
                            let g = ui.fonts(|f| f.layout_job(job));
                            ui.painter().galley(name_pos, g, p.text);
                        }
                        let count = match meta.count { 0 => "vazio".to_string(), 1 => "1 item".into(), n => format!("{n} itens") };
                        ui.painter().text(name_pos + egui::vec2(0.0, 20.0), egui::Align2::LEFT_TOP, format!("{count} · {}", ago(meta.updated_at)), egui::FontId::proportional(12.0), p.muted);
                        // the card's menu
                        let mr = egui::Rect::from_min_size(egui::pos2(rect.right() - 28.0, cover.bottom() + 8.0), egui::vec2(26.0, 26.0));
                        let more = ui.interact(mr, ui.id().with(("board-more", &meta.id)), Sense::click());
                        ui.painter().rect_filled(mr, 6.0, if more.hovered() { p.bg_hover } else { Color32::TRANSPARENT });
                        ui.painter().text(mr.center(), egui::Align2::CENTER_CENTER, icons::ELLIPSIS, kit::icon_font(15.0), p.muted);
                        let menu = |ui: &mut egui::Ui, mbs: &mut Moodboards, open: &mut Option<String>, dup: &mut Option<String>| {
                            if kit::menu_item(ui, icons::LAYOUT_DASHBOARD, "Abrir", p) {
                                *open = Some(meta.id.clone());
                            }
                            if kit::menu_item(ui, icons::PEN_LINE, "Renomear", p) {
                                mbs.rename = Some((meta.id.clone(), meta.name.clone()));
                            }
                            if kit::menu_item(ui, icons::COPY, "Duplicar", p) {
                                *dup = Some(meta.id.clone());
                            }
                            kit::menu_sep(ui, p);
                            if kit::menu_danger(ui, icons::TRASH_2, "Excluir", p) {
                                mbs.confirm_delete = Some(meta.clone());
                            }
                        };
                        let mid = ui.id().with(("board-menu", &meta.id));
                        if more.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(mid));
                        }
                        egui::popup::popup_below_widget(ui, mid, &more, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(170.0);
                            menu(ui, &mut self.mb, &mut open, &mut dup);
                        });
                        r.context_menu(|ui| menu(ui, &mut self.mb, &mut open, &mut dup));
                        if r.clicked() && !more.hovered() {
                            open = Some(meta.id.clone());
                        }
                        if r.double_clicked() && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| p.y > cover.bottom()) {
                            self.mb.rename = Some((meta.id.clone(), meta.name.clone()));
                        }
                    }
                });
                ui.add_space(gap);
            }
        });
        if let Some(id) = dup {
            self.mb.duplicate_board(&id);
        }
        if let Some(id) = open {
            self.mb.open_board(&id);
        }
        self.confirm_delete_ui(ui.ctx(), p);
    }

    fn confirm_delete_ui(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(meta) = self.mb.confirm_delete.clone() else { return };
        kit::scrim(ctx, p);
        let mut close = false;
        egui::Window::new("mb-delete").title_bar(false).resizable(false).collapsible(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).frame(egui::Frame::popup(&ctx.style()).inner_margin(20.0)).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(format!("Excluir “{}”?", meta.name)).font(kit::medium(15.0)).color(p.text));
            ui.add_space(6.0);
            ui.label(RichText::new("O quadro some; as imagens continuam na biblioteca.").size(12.5).color(p.muted));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::danger(ui, "Excluir", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(s) = &self.mb.store {
                            let _ = s.delete(&meta.id);
                            self.mb.boards = s.boards();
                        }
                        close = true;
                    }
                    if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
        });
        if close {
            self.mb.confirm_delete = None;
        }
    }

    // ── a board ───────────────────────────────────────────────────────────

    fn known(&self, it: &Item) -> Option<usize> {
        it.image_id.as_ref().and_then(|id| self.mb.ids.get(id).copied())
    }

    fn board_ui(&mut self, ui: &mut egui::Ui, p: Palette) {
        let canvas = ui.available_rect_before_wrap();
        let origin = canvas.min;
        self.mb.view = egui::Rect::from_min_size(Pos2::ZERO, canvas.size());
        if self.mb.fit_pending {
            self.mb.fit_pending = false;
            if self.mb.items().is_empty() {
                let c = canvas.size() / 2.0;
                self.mb.cam = Cam { x: c.x as f64, y: c.y as f64, z: 1.0 };
            } else {
                let bb = mb::bbox(self.mb.items());
                self.mb.fit(bb, 1.0, false);
            }
        }
        if let Some((from, to, t0)) = self.mb.anim {
            let k = (t0.elapsed().as_secs_f64() / 0.26).min(1.0);
            let e = 1.0 - (1.0 - k).powi(3);
            self.mb.cam = Cam { x: from.x + (to.x - from.x) * e, y: from.y + (to.y - from.y) * e, z: from.z * (to.z / from.z).powf(e) };
            if k >= 1.0 {
                self.mb.anim = None;
                self.mb.mark(true);
            } else {
                ui.ctx().request_repaint();
            }
        }
        let resp = ui.allocate_rect(canvas, Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        let (bg, pattern, focus) = {
            let b = self.mb.board.as_ref().unwrap();
            (mb::background(&b.bg), b.pattern.clone(), b.mode == "focus")
        };
        let ink = if bg.dark { Color32::from_rgb(0xec, 0xec, 0xf1) } else { Color32::from_rgb(0x1d, 0x1d, 0x22) };
        let sel_color = if bg.dark { Color32::from_rgb(0xa8, 0xa5, 0xff) } else { Color32::from_rgb(0x69, 0x65, 0xdb) };
        painter.rect_filled(canvas, 0.0, rgb(bg.color));
        self.paint_pattern(&painter, canvas, bg, &pattern);

        // keyboard first: what it changes is drawn this frame
        let mut notice: Option<String> = None;
        self.board_keys(ui, origin, &resp, &mut notice);

        // items
        let ppp = ui.ctx().pixels_per_point();
        let items: Vec<Item> = self.mb.items().to_vec();
        let mut heights: Vec<(String, f64)> = Vec::new();
        for it in &items {
            if let Some(h) = self.paint_item(&painter, origin, it, ink, sel_color, ppp, ui) {
                heights.push((it.id.clone(), h));
            }
        }
        // notes grow with their text (not an edit: no history)
        let eps = 0.5 / self.mb.cam.z.max(0.01);
        for (id, h) in heights {
            if let Some(it) = self.mb.item_mut(&id) {
                if (it.h - h).abs() > eps {
                    it.h = h;
                }
            }
        }
        self.paint_overlay(&painter, origin, sel_color);

        // pointer on the canvas
        self.board_pointer(ui, origin, &resp, &mut notice);
        self.board_menu(&resp, origin, p, &mut notice);

        // the note or title being typed
        self.edit_ui(ui, origin, ink);

        if !focus {
            self.islands(ui, canvas, p, &mut notice);
        } else {
            // a quiet way back, top right
            let r = egui::Rect::from_min_size(egui::pos2(canvas.right() - 168.0, canvas.top() + 12.0), egui::vec2(156.0, 28.0));
            let near = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| h.y < canvas.top() + 90.0 && h.x > canvas.right() - 300.0);
            if near {
                let b = ui.put(r, egui::Button::new(RichText::new("Voltar ao estúdio   Tab").size(12.0)).corner_radius(14.0));
                if b.clicked() {
                    self.mb.set_mode(false, &mut notice);
                }
            }
            if self.mb.crop.is_some() {
                self.selbar(ui, origin, canvas, p);
            }
        }
        self.drop_ui(ui, origin, &resp, sel_color);
        self.help_ui(ui.ctx(), p);
        if let Some(n) = notice {
            self.notice = Some(n);
        }
        if self.mb.op.is_some() || self.mb.anim.is_some() {
            ui.ctx().request_repaint();
        }
    }

    fn paint_pattern(&self, painter: &egui::Painter, canvas: egui::Rect, bg: &mb::Background, pattern: &str) {
        if pattern == "plain" {
            return;
        }
        let dot = Color32::from_rgba_unmultiplied(bg.dot[0], bg.dot[1], bg.dot[2], bg.dot[3]);
        let mut step = 24.0 * self.mb.cam.z as f32;
        while step < 14.0 {
            step *= 2.0;
        }
        while step > 80.0 {
            step /= 2.0;
        }
        let ox = (self.mb.cam.x as f32).rem_euclid(step);
        let oy = (self.mb.cam.y as f32).rem_euclid(step);
        if pattern == "grid" {
            let mut x = canvas.left() + ox;
            while x < canvas.right() {
                painter.vline(x, canvas.y_range(), Stroke::new(1.0_f32, dot));
                x += step;
            }
            let mut y = canvas.top() + oy;
            while y < canvas.bottom() {
                painter.hline(canvas.x_range(), y, Stroke::new(1.0_f32, dot));
                y += step;
            }
        } else {
            let mut mesh = egui::Mesh::default();
            let mut y = canvas.top() + oy;
            while y < canvas.bottom() {
                let mut x = canvas.left() + ox;
                while x < canvas.right() {
                    mesh.add_colored_rect(egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(2.2, 2.2)), dot);
                    x += step;
                }
                y += step;
            }
            painter.add(mesh);
        }
    }

    /// Draws an item; for a note, returns the height its text needs.
    fn paint_item(&mut self, painter: &egui::Painter, origin: Pos2, it: &Item, ink: Color32, sel_color: Color32, ppp: f32, ui: &egui::Ui) -> Option<f64> {
        let z = self.mb.cam.z;
        let (cx, cy) = it.center();
        let quad = |x0: f64, y0: f64, x1: f64, y1: f64| -> [Pos2; 4] {
            [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| {
                let q = mb::rot_pt(x, y, cx, cy, it.r);
                self.mb.to_screen(origin, q)
            })
        };
        let corners = quad(it.x, it.y, it.x + it.w, it.y + it.h);
        let clip = painter.clip_rect();
        let bounds = egui::Rect::from_points(&corners);
        // frames' titles hang above them
        if !bounds.expand(40.0).intersects(clip) {
            return None;
        }
        let editing = self.mb.edit.as_ref().is_some_and(|e| e.id == it.id);
        match it.kind {
            Kind::Image => {
                let cropping = self.mb.crop.as_ref().is_some_and(|c| c.id == it.id);
                if cropping {
                    return None;
                }
                let c = it.crop.unwrap_or(mb::Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
                match self.known(it) {
                    Some(i) => {
                        let px = ((it.w / c.w).max(it.h / c.h) * z) as f32 * ppp;
                        if let Some(t) = self.thumb_sized(i, px, true, it.gray) {
                            let (u0, u1) = if it.flip { (c.x + c.w, c.x) } else { (c.x, c.x + c.w) };
                            let uv = [(u0, c.y), (u1, c.y), (u1, c.y + c.h), (u0, c.y + c.h)];
                            let mut mesh = egui::Mesh::with_texture(t);
                            for (k, pt) in corners.iter().enumerate() {
                                mesh.vertices.push(egui::epaint::Vertex { pos: *pt, uv: egui::pos2(uv[k].0 as f32, uv[k].1 as f32), color: Color32::WHITE });
                            }
                            mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                            painter.add(mesh);
                        } else {
                            painter.add(egui::Shape::convex_polygon(corners.to_vec(), ink.gamma_multiply(0.08), Stroke::NONE));
                        }
                    }
                    None => {
                        painter.add(egui::Shape::convex_polygon(corners.to_vec(), ink.gamma_multiply(0.08), Stroke::new(1.0_f32, ink.gamma_multiply(0.2))));
                        let msg = if self.scanning.is_some() { "Carregando…" } else { "Imagem não encontrada" };
                        let g = ui.fonts(|f| f.layout_no_wrap(msg.into(), egui::FontId::proportional(12.0), ink.gamma_multiply(0.7)));
                        if g.size().x + 12.0 < bounds.width() {
                            painter.galley(bounds.center() - g.size() / 2.0, g, ink);
                        } else if bounds.width() > 18.0 {
                            painter.text(bounds.center(), egui::Align2::CENTER_CENTER, icons::IMAGES, kit::icon_font(14.0), ink.gamma_multiply(0.5));
                        }
                    }
                }
                None
            }
            Kind::Note => {
                let fs = it.fs.unwrap_or(20.0);
                let filled = it.fill.as_deref().and_then(mb::parse_hex);
                let (padx, pady) = if filled.is_some() { (0.9 * fs, 0.8 * fs) } else { (0.6 * fs, 0.45 * fs) };
                let color = if filled.is_some() { Color32::from_rgb(0x1d, 0x1d, 0x22) } else { ink };
                let size = (fs * z) as f32;
                let wrap = ((it.w - padx * 2.0) * z).max(4.0) as f32;
                let text = if editing { self.mb.edit.as_ref().unwrap().buf.clone() } else { it.text().to_string() };
                let line_h = size * 1.32;
                let galley = (size > 0.5).then(|| ui.fonts(|f| {
                    let mut job = egui::text::LayoutJob::single_section(if text.is_empty() { " ".into() } else { text.clone() }, egui::TextFormat { font_id: egui::FontId::proportional(size), color, line_height: Some(line_h), ..Default::default() });
                    job.wrap.max_width = wrap;
                    f.layout_job(job)
                }));
                let text_h = galley.as_ref().map(|g| g.size().y as f64 / z).unwrap_or(fs * 1.32);
                let need = text_h.max(fs * 1.32) + pady * 2.0;
                let h_now = it.h;
                let corners = quad(it.x, it.y, it.x + it.w, it.y + h_now);
                if let Some(fc) = filled {
                    // a soft shadow under the paper
                    let shadow: Vec<Pos2> = corners.iter().map(|p| *p + egui::vec2(0.0, 3.0)).collect();
                    painter.add(egui::Shape::convex_polygon(shadow, Color32::from_black_alpha(22), Stroke::NONE));
                    painter.add(egui::Shape::convex_polygon(corners.to_vec(), rgb(fc), Stroke::NONE));
                }
                if editing {
                    return Some(need);
                }
                if let Some(g) = galley {
                    if text.is_empty() {
                        let pos = self.mb.to_screen(origin, mb::rot_pt(it.x + padx, it.y + pady, cx, cy, it.r));
                        painter.add(egui::epaint::TextShape::new(pos, ui.fonts(|f| f.layout_no_wrap("Escreva…".into(), egui::FontId::proportional(size), color.gamma_multiply(0.45))), color).with_angle(it.r.to_radians() as f32));
                    } else {
                        let pos = self.mb.to_screen(origin, mb::rot_pt(it.x + padx, it.y + pady, cx, cy, it.r));
                        painter.add(egui::epaint::TextShape::new(pos, g, color).with_angle(it.r.to_radians() as f32));
                    }
                }
                Some(need)
            }
            Kind::Swatch => {
                let shadow: Vec<Pos2> = corners.iter().map(|p| *p + egui::vec2(0.0, 3.0)).collect();
                painter.add(egui::Shape::convex_polygon(shadow, Color32::from_black_alpha(26), Stroke::NONE));
                painter.add(egui::Shape::convex_polygon(corners.to_vec(), Color32::WHITE, Stroke::NONE));
                let pad = it.w * 0.05;
                let fs = (it.w * 0.11).max(6.0);
                let chip_bottom = it.y + it.h - pad - fs * 1.45;
                let chip = quad(it.x + pad, it.y + pad, it.x + it.w - pad, chip_bottom);
                painter.add(egui::Shape::convex_polygon(chip.to_vec(), hex_color(it.color()), Stroke::new(1.0_f32, Color32::from_black_alpha(14))));
                let size = (fs * z) as f32;
                if size > 3.0 {
                    let pos = self.mb.to_screen(origin, mb::rot_pt(it.x + pad, chip_bottom + fs * 0.35, cx, cy, it.r));
                    let g = ui.fonts(|f| f.layout_no_wrap(it.color().to_uppercase(), egui::FontId::monospace(size), Color32::from_rgb(0x2a, 0x2a, 0x30)));
                    painter.add(egui::epaint::TextShape::new(pos, g, Color32::from_rgb(0x2a, 0x2a, 0x30)).with_angle(it.r.to_radians() as f32));
                }
                None
            }
            Kind::Frame => {
                let r = egui::Rect::from_two_pos(corners[0], corners[2]);
                if let Some(t) = it.tint.and_then(|t| mb::FRAME_TINTS.get(t).copied().flatten()) {
                    painter.rect_filled(r, 10.0, hex_color(t).gamma_multiply(0.15));
                }
                painter.rect_stroke(r, 10.0, Stroke::new(1.5_f32, ink.gamma_multiply(0.22)), egui::StrokeKind::Inside);
                if !editing {
                    let sel = self.mb.sel.contains(&it.id);
                    painter.text(r.left_top() + egui::vec2(2.0, -6.0), egui::Align2::LEFT_BOTTOM, it.title(), kit::medium(13.0), if sel { sel_color } else { ink.gamma_multiply(0.75) });
                }
                None
            }
        }
    }

    /// The title a frame is picked by (screen).
    fn frame_title_rect(&self, origin: Pos2, it: &Item, ui: &egui::Ui) -> egui::Rect {
        let tl = self.mb.to_screen(origin, (it.x, it.y));
        let w = ui.fonts(|f| f.layout_no_wrap(it.title().to_string(), kit::medium(13.0), Color32::WHITE).size().x).max(30.0);
        egui::Rect::from_min_max(tl + egui::vec2(0.0, -24.0), tl + egui::vec2(w + 6.0, -2.0))
    }

    /// The item under a screen point, topmost first.
    fn hit(&self, origin: Pos2, pos: Pos2, ui: &egui::Ui) -> Option<String> {
        let w = self.mb.to_world(origin, pos);
        for it in self.mb.items().iter().rev() {
            let hit = match it.kind {
                Kind::Frame => self.frame_title_rect(origin, it, ui).contains(pos),
                _ => it.contains(w.0, w.1),
            };
            if hit {
                return Some(it.id.clone());
            }
        }
        None
    }

    /// Selection handles on screen.
    fn handles(&self, origin: Pos2) -> Vec<(Handle, Pos2)> {
        if self.mb.edit.is_some() || matches!(self.mb.op, Some(Op::Move { moved: true, .. })) || self.mb.crop.is_some() {
            return Vec::new();
        }
        let sel = self.mb.selected();
        let mut out = Vec::new();
        if sel.len() == 1 {
            let it = sel[0];
            if it.locked {
                return out;
            }
            let (cx, cy) = it.center();
            let pt = |x: f64, y: f64| self.mb.to_screen(origin, mb::rot_pt(x, y, cx, cy, it.r));
            if it.kind == Kind::Note {
                out.push((Handle::Corner(1.0, 0.0), pt(it.x + it.w, cy)));
                out.push((Handle::Corner(-1.0, 0.0), pt(it.x, cy)));
            } else {
                for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    out.push((Handle::Corner(sx, sy), pt(cx + sx * it.w / 2.0, cy + sy * it.h / 2.0)));
                }
            }
            if it.kind != Kind::Frame {
                let top = pt(cx, it.y);
                let dir = (top - pt(cx, cy)).normalized();
                out.push((Handle::Rotate, top + dir * 24.0));
            }
        } else if sel.len() > 1 && sel.iter().any(|i| !i.locked) {
            let bb = mb::bbox(sel);
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                out.push((Handle::Corner(sx, sy), self.mb.to_screen(origin, (bb.x + bb.w * (sx + 1.0) / 2.0, bb.y + bb.h * (sy + 1.0) / 2.0))));
            }
        }
        out
    }

    fn crop_handles(&self, origin: Pos2) -> Vec<(&'static str, Pos2)> {
        let Some(st) = &self.mb.crop else { return Vec::new() };
        let r = st.rect;
        let pts = [("nw", r.x, r.y), ("n", r.x + r.w / 2.0, r.y), ("ne", r.x + r.w, r.y), ("e", r.x + r.w, r.y + r.h / 2.0), ("se", r.x + r.w, r.y + r.h), ("s", r.x + r.w / 2.0, r.y + r.h), ("sw", r.x, r.y + r.h), ("w", r.x, r.y + r.h / 2.0)];
        pts.iter().map(|&(k, x, y)| (k, self.mb.to_screen(origin, st.world((x, y))))).collect()
    }

    fn paint_overlay(&mut self, painter: &egui::Painter, origin: Pos2, sel_color: Color32) {
        // the crop stage: the whole picture dim, the window bright
        if let Some(st) = self.mb.crop.clone() {
            if let Some(it) = self.mb.item(&st.id).cloned() {
                if let Some(i) = self.known(&it) {
                    let q = |x: f64, y: f64| self.mb.to_screen(origin, st.world((x, y)));
                    let full = [q(0.0, 0.0), q(st.fw, 0.0), q(st.fw, st.fh), q(0.0, st.fh)];
                    let r = st.rect;
                    let win = [q(r.x, r.y), q(r.x + r.w, r.y), q(r.x + r.w, r.y + r.h), q(r.x, r.y + r.h)];
                    let px = (st.fw.max(st.fh) * self.mb.cam.z) as f32 * painter.ctx().pixels_per_point();
                    if let Some(t) = self.thumb_sized(i, px, true, it.gray) {
                        let flip = |u: f64| if it.flip { 1.0 - u } else { u };
                        let draw = |pts: [Pos2; 4], u0: f64, v0: f64, u1: f64, v1: f64, tint: Color32| {
                            let uv = [(flip(u0), v0), (flip(u1), v0), (flip(u1), v1), (flip(u0), v1)];
                            let mut mesh = egui::Mesh::with_texture(t);
                            for k in 0..4 {
                                mesh.vertices.push(egui::epaint::Vertex { pos: pts[k], uv: egui::pos2(uv[k].0 as f32, uv[k].1 as f32), color: tint });
                            }
                            mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                            painter.add(mesh);
                        };
                        draw(full, 0.0, 0.0, 1.0, 1.0, Color32::from_white_alpha(90));
                        draw(win, r.x / st.fw, r.y / st.fh, (r.x + r.w) / st.fw, (r.y + r.h) / st.fh, Color32::WHITE);
                    }
                    painter.add(egui::Shape::closed_line(win.to_vec(), Stroke::new(1.5_f32, sel_color)));
                    for (_, pos) in self.crop_handles(origin) {
                        painter.rect(egui::Rect::from_center_size(pos, egui::vec2(9.0, 9.0)), 2.0, Color32::WHITE, Stroke::new(1.5_f32, sel_color), egui::StrokeKind::Middle);
                    }
                }
            }
        }
        if let Some((a, b)) = self.mb.marquee {
            let r = egui::Rect::from_two_pos(a, b);
            painter.rect(r, 2.0, sel_color.gamma_multiply(0.1), Stroke::new(1.0_f32, sel_color), egui::StrokeKind::Inside);
        }
        for g in &self.mb.guides {
            let (a, b) = if g.v { ((g.at, g.from), (g.at, g.to)) } else { ((g.from, g.at), (g.to, g.at)) };
            painter.line_segment([self.mb.to_screen(origin, a), self.mb.to_screen(origin, b)], Stroke::new(1.0_f32, Color32::from_rgb(0xff, 0x4d, 0x8d)));
        }
        if self.mb.crop.is_some() {
            return;
        }
        let sel: Vec<Item> = self.mb.selected().into_iter().cloned().collect();
        let outline = |it: &Item, w: f32| {
            let (cx, cy) = it.center();
            let pts: Vec<Pos2> = [(it.x, it.y), (it.x + it.w, it.y), (it.x + it.w, it.y + it.h), (it.x, it.y + it.h)].iter().map(|&(x, y)| self.mb.to_screen(origin, mb::rot_pt(x, y, cx, cy, it.r))).collect();
            painter.add(egui::Shape::closed_line(pts, Stroke::new(w, sel_color)));
        };
        if sel.len() == 1 {
            outline(&sel[0], 1.5);
            if sel[0].locked {
                let (cx, cy) = sel[0].center();
                let tr = self.mb.to_screen(origin, mb::rot_pt(sel[0].x + sel[0].w, sel[0].y, cx, cy, sel[0].r));
                let tag = egui::Rect::from_min_size(tr + egui::vec2(-26.0, -24.0), egui::vec2(26.0, 20.0));
                painter.rect_filled(tag, 6.0, sel_color);
                painter.text(tag.center(), egui::Align2::CENTER_CENTER, icons::LOCK, kit::icon_font(11.0), Color32::WHITE);
            }
        } else if sel.len() > 1 {
            for it in &sel {
                outline(it, 1.0);
            }
            let bb = mb::bbox(&sel);
            painter.rect_stroke(egui::Rect::from_two_pos(self.mb.to_screen(origin, (bb.x, bb.y)), self.mb.to_screen(origin, (bb.x + bb.w, bb.y + bb.h))), 0.0, Stroke::new(1.5_f32, sel_color), egui::StrokeKind::Middle);
        }
        for (h, pos) in self.handles(origin) {
            match h {
                Handle::Rotate => {
                    painter.circle(pos, 5.5, Color32::WHITE, Stroke::new(1.5_f32, sel_color));
                }
                Handle::Corner(..) => {
                    painter.rect(egui::Rect::from_center_size(pos, egui::vec2(9.0, 9.0)), 2.0, Color32::WHITE, Stroke::new(1.5_f32, sel_color), egui::StrokeKind::Middle);
                }
            }
        }
    }

    fn board_pointer(&mut self, ui: &egui::Ui, origin: Pos2, resp: &egui::Response, notice: &mut Option<String>) {
        let (pressed, down, released, pos, mods, mid_pressed, double) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down() || i.pointer.middle_down(), i.pointer.any_released(), i.pointer.interact_pos(), i.modifiers, i.pointer.button_pressed(egui::PointerButton::Middle), i.pointer.button_double_clicked(egui::PointerButton::Primary)));
        let hovered = resp.hovered() || resp.contains_pointer();
        let space = ui.input(|i| i.key_down(egui::Key::Space)) && !ui.ctx().wants_keyboard_input();
        // wheel: pan, or zoom with Ctrl/⌘ (or when the wheel is set to zoom)
        if hovered && self.mb.op.is_none() {
            let (scroll, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom_delta != 1.0 {
                if let Some(at) = ui.input(|i| i.pointer.hover_pos()) {
                    self.mb.zoom_at(at - origin.to_vec2(), self.mb.cam.z * zoom_delta as f64);
                }
            } else if scroll != Vec2::ZERO {
                if self.mb.prefs["wheel"].as_str() == Some("zoom") {
                    if let Some(at) = ui.input(|i| i.pointer.hover_pos()) {
                        self.mb.zoom_at(at - origin.to_vec2(), self.mb.cam.z * (scroll.y as f64 * 0.004).exp());
                    }
                } else {
                    self.mb.cam.x += scroll.x as f64;
                    self.mb.cam.y += scroll.y as f64;
                    self.mb.zoomed_to = None;
                    self.mb.anim = None;
                    self.mb.mark(true);
                }
            }
        }
        let Some(pos) = pos else { return };
        let w = self.mb.to_world(origin, pos);
        // a double click is known on the second release, while the press
        // that came before it is still a drag in progress: end that first
        if double && hovered {
            if self.mb.op.is_some() {
                self.end_op(origin);
            }
            self.board_double(ui, origin, pos);
            return;
        }
        if (pressed || mid_pressed) && hovered && self.mb.op.is_none() {
            self.mb.look_open = false;
            if self.mb.edit.is_some() {
                self.mb.commit_edit();
            }
            if mid_pressed || space || self.mb.tool == Tool::Hand {
                self.mb.op = Some(Op::Pan { p0: pos, cam0: self.mb.cam });
                return;
            }
            if let Some(st) = &self.mb.crop {
                let k = self.crop_handles(origin).into_iter().find(|(_, hp)| hp.distance(pos) < 9.0).map(|(k, _)| k);
                let inside = {
                    let q = st.local(w);
                    q.0 >= st.rect.x && q.0 <= st.rect.x + st.rect.w && q.1 >= st.rect.y && q.1 <= st.rect.y + st.rect.h
                };
                match k.or(inside.then_some("move")) {
                    Some(k) => self.mb.op = Some(Op::Crop { k, start: st.local(w), rect0: st.rect }),
                    None => self.mb.apply_crop(),
                }
                return;
            }
            if let Some((h, _)) = self.handles(origin).into_iter().find(|(_, hp)| hp.distance(pos) < 9.0) {
                self.start_transform(h, w);
                return;
            }
            match self.mb.tool {
                Tool::Note => {
                    self.mb.create_note(w);
                    self.mb.tool = Tool::Select;
                    return;
                }
                Tool::Swatch => {
                    let id = self.mb.create_swatch(w, None);
                    self.mb.tool = Tool::Select;
                    self.open_color(&id);
                    return;
                }
                Tool::Frame => {
                    self.mb.op = Some(Op::Frame { p0: pos });
                    self.mb.marquee = Some((pos, pos));
                    return;
                }
                _ => {}
            }
            let add = mods.shift || mods.command;
            match self.hit(origin, pos, ui) {
                Some(id) => {
                    if add {
                        if !self.mb.sel.remove(&id) {
                            self.mb.sel.insert(id);
                            self.start_move(w, pos, mods.alt);
                        }
                        return;
                    }
                    if !self.mb.sel.contains(&id) {
                        self.mb.sel = HashSet::from([id]);
                    }
                    self.start_move(w, pos, mods.alt);
                }
                None => {
                    let base = if add { self.mb.sel.clone() } else { HashSet::new() };
                    if !add {
                        self.mb.sel.clear();
                    }
                    self.mb.op = Some(Op::Marquee { p0: pos, base });
                    self.mb.marquee = Some((pos, pos));
                }
            }
            return;
        }
        if self.mb.op.is_some() && down {
            self.board_drag(ui, origin, pos, w, mods, notice);
        }
        if self.mb.op.is_some() && (released || !down) {
            self.end_op(origin);
        }
        // the cursor
        if hovered && self.mb.op.is_none() {
            let icon = if space || self.mb.tool == Tool::Hand {
                egui::CursorIcon::Grab
            } else if matches!(self.mb.tool, Tool::Note) {
                egui::CursorIcon::Text
            } else if matches!(self.mb.tool, Tool::Swatch | Tool::Frame) {
                egui::CursorIcon::Crosshair
            } else if self.handles(origin).iter().any(|(_, hp)| hp.distance(pos) < 9.0) || self.crop_handles(origin).iter().any(|(_, hp)| hp.distance(pos) < 9.0) {
                egui::CursorIcon::Crosshair
            } else {
                egui::CursorIcon::Default
            };
            ui.ctx().set_cursor_icon(icon);
        } else if matches!(self.mb.op, Some(Op::Pan { .. })) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }

    fn board_double(&mut self, ui: &egui::Ui, origin: Pos2, pos: Pos2) {
        if self.mb.crop.is_some() {
            self.mb.apply_crop();
            return;
        }
        if self.mb.edit.is_some() || self.mb.tool != Tool::Select {
            return;
        }
        let w = self.mb.to_world(origin, pos);
        match self.hit(origin, pos, ui).and_then(|id| self.mb.item(&id).cloned()) {
            None => self.mb.create_note(w),
            Some(it) => match it.kind {
                Kind::Note | Kind::Frame => self.mb.start_edit(&it.id),
                Kind::Swatch if !it.locked => self.open_color(&it.id),
                Kind::Image => {
                    if self.mb.zoomed_to.as_deref() == Some(it.id.as_str()) {
                        self.mb.fit_all();
                    } else {
                        let bb = mb::bbox([&it]);
                        self.mb.fit(bb, 8.0, true);
                        self.mb.zoomed_to = Some(it.id.clone());
                    }
                }
                _ => {}
            },
        }
    }

    fn start_move(&mut self, w: (f64, f64), p0: Pos2, alt: bool) {
        let mut ids: Vec<String> = self.mb.selected().iter().filter(|i| !i.locked).map(|i| i.id.clone()).collect();
        let mut before = Some(self.mb.snapshot());
        if alt && !ids.is_empty() {
            // Alt-drag: the copies move, the originals stay
            self.mb.push_history(before.take().unwrap());
            let clones: Vec<Item> = ids.iter().filter_map(|id| self.mb.item(id)).map(|i| i.cloned(0.0, 0.0)).collect();
            ids = clones.iter().map(|c| c.id.clone()).collect();
            self.mb.add_items(clones, true);
        }
        let items = self.mb.items();
        let mut moving: Vec<String> = ids.clone();
        for id in &ids {
            if let Some(fi) = items.iter().position(|i| &i.id == id && i.kind == Kind::Frame) {
                for c in mb::frame_children(items, fi) {
                    if !items[c].locked && !moving.contains(&items[c].id) {
                        moving.push(items[c].id.clone());
                    }
                }
            }
        }
        let orig: Vec<(String, f64, f64)> = moving.iter().filter_map(|id| self.mb.item(id)).map(|i| (i.id.clone(), i.x, i.y)).collect();
        let bb0 = mb::bbox(moving.iter().filter_map(|id| self.mb.item(id)));
        let view = self.mb.view;
        let (a, b) = (self.mb.to_world(Pos2::ZERO, view.min), self.mb.to_world(Pos2::ZERO, view.max));
        let others: Vec<Rect> = self.mb.items().iter().filter(|i| !moving.contains(&i.id)).map(|i| mb::bbox([i])).filter(|r| r.x + r.w >= a.0 && r.x <= b.0 && r.y + r.h >= a.1 && r.y <= b.1).take(400).collect();
        let locked_only = moving.is_empty() && !self.mb.sel.is_empty();
        self.mb.op = Some(Op::Move { start: w, p0, moved: false, before, orig, bb0, others, locked_only });
    }

    fn start_transform(&mut self, h: Handle, w: (f64, f64)) {
        let items: Vec<Item> = self.mb.selected().into_iter().filter(|i| !i.locked).cloned().collect();
        if items.is_empty() {
            return;
        }
        let before = self.mb.snapshot();
        match h {
            Handle::Rotate => {
                let it = &items[0];
                let c = it.center();
                self.mb.op = Some(Op::Rotate { id: it.id.clone(), c, a0: (w.1 - c.1).atan2(w.0 - c.0), r0: it.r, before, changed: false });
            }
            Handle::Corner(sx, sy) => {
                if items.len() == 1 {
                    let it = &items[0];
                    let (cx, cy) = it.center();
                    let a = mb::rot_pt(cx - sx * it.w / 2.0, cy - sy * it.h / 2.0, cx, cy, it.r);
                    self.mb.op = Some(Op::Resize1 { id: it.id.clone(), sx, sy, a, w0: it.w, h0: it.h, before, changed: false });
                } else {
                    let bb = mb::bbox(&items);
                    let a = (if sx > 0.0 { bb.x } else { bb.x + bb.w }, if sy > 0.0 { bb.y } else { bb.y + bb.h });
                    let orig = items.iter().map(|i| (i.id.clone(), i.x, i.y, i.w, i.h, i.fs)).collect();
                    self.mb.op = Some(Op::ResizeN { orig, a, bb, sx, sy, before, changed: false });
                }
            }
        }
    }

    fn board_drag(&mut self, _ui: &egui::Ui, origin: Pos2, pos: Pos2, w: (f64, f64), mods: egui::Modifiers, notice: &mut Option<String>) {
        let z = self.mb.cam.z;
        let Some(op) = self.mb.op.as_mut() else { return };
        match op {
            Op::Pan { p0, cam0 } => {
                self.mb.cam.x = cam0.x + (pos.x - p0.x) as f64;
                self.mb.cam.y = cam0.y + (pos.y - p0.y) as f64;
                self.mb.zoomed_to = None;
            }
            Op::Marquee { p0, base } => {
                let p0 = *p0;
                let base = base.clone();
                self.mb.marquee = Some((p0, pos));
                let a = self.mb.to_world(origin, egui::pos2(p0.x.min(pos.x), p0.y.min(pos.y)));
                let b = self.mb.to_world(origin, egui::pos2(p0.x.max(pos.x), p0.y.max(pos.y)));
                let area = Rect { x: a.0, y: a.1, w: b.0 - a.0, h: b.1 - a.1 };
                let hits: Vec<String> = self.mb.items().iter().filter(|it| {
                    let bb = mb::bbox([*it]);
                    if it.kind == Kind::Frame { area.contains_rect(&bb) } else { bb.x <= b.0 && bb.x + bb.w >= a.0 && bb.y <= b.1 && bb.y + bb.h >= a.1 }
                }).map(|i| i.id.clone()).collect();
                self.mb.sel = base.into_iter().chain(hits).collect();
            }
            Op::Frame { p0 } => {
                self.mb.marquee = Some((*p0, pos));
            }
            Op::Move { start, p0, moved, orig, bb0, others, locked_only, .. } => {
                if !*moved {
                    if p0.distance(pos) < 3.0 {
                        return;
                    }
                    *moved = true;
                    if *locked_only {
                        *notice = Some("Itens travados: destrave com Shift+L".into());
                    }
                }
                let (mut dx, mut dy) = (w.0 - start.0, w.1 - start.1);
                self.mb.guides.clear();
                if !mods.command && !others.is_empty() {
                    let (sx, sy, g) = mb::snap(*bb0, dx, dy, others, 6.0 / z);
                    dx = sx;
                    dy = sy;
                    self.mb.guides = g;
                }
                let orig = orig.clone();
                if let Some(b) = &mut self.mb.board {
                    for (id, x, y) in orig {
                        if let Some(it) = b.items.iter_mut().find(|i| i.id == id) {
                            it.x = x + dx;
                            it.y = y + dy;
                        }
                    }
                }
            }
            Op::Crop { k, start, rect0 } => {
                let (k, start, rect0) = (*k, *start, *rect0);
                if let Some(st) = &mut self.mb.crop {
                    let q = st.local(w);
                    st.drag(k, rect0, (q.0 - start.0, q.1 - start.1), mods.shift, 12.0 / z);
                }
            }
            Op::Rotate { id, c, a0, r0, changed, .. } => {
                let a = (w.1 - c.1).atan2(w.0 - c.0);
                let mut r = *r0 + (a - *a0).to_degrees();
                if mods.shift {
                    r = (r / 15.0).round() * 15.0;
                } else {
                    for s in [0.0, 90.0, 180.0, -90.0, -180.0, 270.0] {
                        if (r - s).abs() < 2.5 {
                            r = s;
                        }
                    }
                }
                r = (r + 180.0).rem_euclid(360.0) - 180.0;
                *changed = true;
                let id = id.clone();
                if let Some(it) = self.mb.item_mut(&id) {
                    it.r = (r * 10.0).round() / 10.0;
                }
            }
            Op::Resize1 { id, sx, sy, a, w0, h0, changed, .. } => {
                *changed = true;
                let (id, sx, sy, a, w0, h0) = (id.clone(), *sx, *sy, *a, *w0, *h0);
                if let Some(it) = self.mb.item_mut(&id) {
                    let free = mods.shift && it.kind == Kind::Image;
                    let (x, y, nw, nh) = mb::resize_one(it, w0, h0, a, sx, sy, w, free, 16.0 / z);
                    it.x = x;
                    it.y = y;
                    it.w = nw;
                    it.h = nh;
                }
            }
            Op::ResizeN { orig, a, bb, sx, sy, changed, .. } => {
                let s = ((*sx * (w.0 - a.0)) / bb.w).max((*sy * (w.1 - a.1)) / bb.h).max(0.03);
                *changed = true;
                let (orig, a) = (orig.clone(), *a);
                if let Some(b) = &mut self.mb.board {
                    for (id, x, y, ow, oh, fs) in orig {
                        if let Some(it) = b.items.iter_mut().find(|i| i.id == id) {
                            it.x = a.0 + (x - a.0) * s;
                            it.y = a.1 + (y - a.1) * s;
                            it.w = ow * s;
                            it.h = oh * s;
                            if it.kind == Kind::Note {
                                it.fs = fs.map(|f| f * s);
                            }
                        }
                    }
                }
            }
            Op::Mini => {}
        }
    }

    fn end_op(&mut self, origin: Pos2) {
        let Some(op) = self.mb.op.take() else { return };
        match op {
            Op::Pan { .. } => self.mb.mark(true),
            Op::Move { moved, before, orig, .. } => {
                if moved && !orig.is_empty() {
                    if let Some(b) = before {
                        self.mb.push_history(b);
                    }
                    self.mb.mark(false);
                }
            }
            Op::Frame { p0 } => {
                let pos = self.mb.marquee.map(|m| m.1).unwrap_or(p0);
                let a = self.mb.to_world(origin, egui::pos2(p0.x.min(pos.x), p0.y.min(pos.y)));
                let b = self.mb.to_world(origin, egui::pos2(p0.x.max(pos.x), p0.y.max(pos.y)));
                let z = self.mb.cam.z;
                let r = if b.0 - a.0 > 30.0 / z && b.1 - a.1 > 30.0 / z {
                    Rect { x: a.0, y: a.1, w: b.0 - a.0, h: b.1 - a.1 }
                } else {
                    let s = self.mb.to_world(origin, p0);
                    Rect { x: s.0 - 320.0 / z, y: s.1 - 210.0 / z, w: 640.0 / z, h: 420.0 / z }
                };
                self.mb.tool = Tool::Select;
                self.mb.marquee = None;
                self.mb.create_frame(r);
            }
            Op::Rotate { before, changed, .. } | Op::Resize1 { before, changed, .. } | Op::ResizeN { before, changed, .. } => {
                if changed {
                    self.mb.push_history(before);
                    self.mb.mark(false);
                }
            }
            Op::Marquee { .. } | Op::Crop { .. } | Op::Mini => {}
        }
        self.mb.marquee = None;
        self.mb.guides.clear();
    }

    fn open_color(&mut self, id: &str) {
        if self.mb.item(id).is_none() {
            return;
        }
        let before = self.mb.snapshot();
        self.mb.color_before = Some((id.to_string(), before));
        self.mb.sel = HashSet::from([id.to_string()]);
    }

    /// Five colours of a picture as swatches beside it.
    fn extract_palette(&mut self, id: &str) {
        let Some(it) = self.mb.item(id).cloned() else { return };
        let Some(i) = self.known(&it) else { return };
        let e = &self.entries[i];
        let Some(img) = super::preview(Path::new(&e.abs), e.mtime, e.size, super::THUMB) else {
            self.notice = Some("Não foi possível ler as cores da imagem".into());
            return;
        };
        // only the visible (cropped) part counts
        let c = it.crop.unwrap_or(mb::Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let [w, h] = img.size;
        let (x0, y0) = ((c.x * w as f64) as usize, (c.y * h as f64) as usize);
        let (x1, y1) = ((((c.x + c.w) * w as f64) as usize).min(w), (((c.y + c.h) * h as f64) as usize).min(h));
        let mut rgba = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                rgba.extend_from_slice(&img.pixels[y * w + x].to_srgba_unmultiplied());
            }
        }
        let colors = mb::palette(&rgba);
        if colors.is_empty() {
            return;
        }
        let pic = mb::bbox([&it]);
        let others: Vec<Rect> = self.mb.items().iter().filter(|o| o.kind != Kind::Frame && o.id != it.id).map(|o| mb::bbox([o])).collect();
        let spots = mb::palette_spots(pic, colors.len(), 1.0 / self.mb.cam.z, &others);
        let before = self.mb.snapshot();
        self.mb.push_history(before);
        let items = colors.iter().zip(spots).map(|(c, r)| Item::swatch(&mb::hex(*c), r.x, r.y, r.w, r.h)).collect();
        self.mb.add_items(items, true);
    }

    fn board_keys(&mut self, ui: &egui::Ui, origin: Pos2, _resp: &egui::Response, notice: &mut Option<String>) {
        if self.mb.confirm_delete.is_some() || self.mb.help_open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.mb.help_open = false;
            return;
        }
        if self.mb.edit.is_some() {
            let note = self.mb.edit.as_ref().unwrap().note;
            let done = ui.input(|i| i.key_pressed(egui::Key::Escape) || (i.key_pressed(egui::Key::Enter) && (i.modifiers.command || !note)));
            if done {
                self.mb.commit_edit();
            }
            return;
        }
        if ui.ctx().wants_keyboard_input() || self.mb.help_open {
            return;
        }
        // copy, cut and paste arrive as events of their own
        let (copy, cut, paste) = ui.input(|i| {
            let mut c = (false, false, false);
            for e in &i.events {
                match e {
                    egui::Event::Copy => c.0 = true,
                    egui::Event::Cut => c.1 = true,
                    egui::Event::Paste(_) => c.2 = true,
                    _ => {}
                }
            }
            c
        });
        let pointer_world = ui.input(|i| i.pointer.hover_pos()).filter(|p| self.mb.view.translate(origin.to_vec2()).contains(*p)).map(|p| self.mb.to_world(origin, p));
        if copy || cut {
            self.mb.copy(notice);
            if cut {
                self.mb.delete_selection(notice);
            }
        }
        if paste {
            let at = pointer_world.unwrap_or_else(|| self.mb.view_center_world());
            self.mb.paste(at);
        }
        let k = |key: egui::Key| ui.input(|i| i.key_pressed(key));
        let mods = ui.input(|i| i.modifiers);
        if self.mb.crop.is_some() {
            if k(egui::Key::Enter) {
                self.mb.apply_crop();
            } else if k(egui::Key::Escape) {
                self.mb.crop = None;
            } else if k(egui::Key::R) && mods.shift {
                if let Some(st) = &mut self.mb.crop {
                    st.reset();
                }
            }
            return;
        }
        if mods.command {
            if k(egui::Key::Z) {
                if mods.shift { self.mb.redo() } else { self.mb.undo() }
            } else if k(egui::Key::Y) {
                self.mb.redo();
            } else if k(egui::Key::A) {
                self.mb.select_all();
            } else if k(egui::Key::D) {
                self.mb.duplicate();
            } else if k(egui::Key::CloseBracket) {
                self.mb.reorder("front");
            } else if k(egui::Key::OpenBracket) {
                self.mb.reorder("back");
            } else if k(egui::Key::Num0) {
                let s: Vec<Item> = self.mb.selected().into_iter().cloned().collect();
                let bb = if s.is_empty() { mb::bbox(self.mb.items()) } else { mb::bbox(&s) };
                self.mb.fit(bb, 8.0, true);
            }
            return;
        }
        if mods.shift && (k(egui::Key::Num1) || k(egui::Key::Num2) || k(egui::Key::Num0)) {
            if k(egui::Key::Num1) {
                self.mb.fit_all();
            } else if k(egui::Key::Num2) {
                self.mb.fit_selection();
            } else {
                let c = self.mb.free_rect(self.mb.view).center();
                let to = self.mb.cam_for_zoom(c, 1.0);
                self.mb.animate(to);
            }
            return;
        }
        let arrows = [(egui::Key::ArrowLeft, -1.0, 0.0), (egui::Key::ArrowRight, 1.0, 0.0), (egui::Key::ArrowUp, 0.0, -1.0), (egui::Key::ArrowDown, 0.0, 1.0)];
        for (key, ax, ay) in arrows {
            if k(key) && self.mb.selected().iter().any(|i| !i.locked) {
                if self.mb.last_nudge.is_none_or(|t| t.elapsed() > Duration::from_millis(600)) {
                    let before = self.mb.snapshot();
                    self.mb.push_history(before);
                }
                self.mb.last_nudge = Some(Instant::now());
                let step = if mods.shift { 10.0 } else { 1.0 } / self.mb.cam.z;
                let sel = self.mb.sel.clone();
                if let Some(b) = &mut self.mb.board {
                    for it in b.items.iter_mut().filter(|i| sel.contains(&i.id) && !i.locked) {
                        it.x += ax * step;
                        it.y += ay * step;
                    }
                }
                self.mb.mark(false);
            }
        }
        let one = self.mb.one();
        if std::mem::take(&mut self.mb.tab_pressed) {
            let focus = self.mb.board.as_ref().is_some_and(|b| b.mode != "focus");
            self.mb.set_mode(focus, notice);
        }
        if k(egui::Key::Escape) {
            if self.mb.look_open {
                self.mb.look_open = false;
            } else if self.mb.tool != Tool::Select {
                self.mb.tool = Tool::Select;
            } else if !self.mb.tray.picked.is_empty() {
                self.mb.tray.picked.clear();
            } else {
                self.mb.sel.clear();
            }
        }
        if k(egui::Key::Delete) || k(egui::Key::Backspace) {
            self.mb.delete_selection(notice);
        }
        if k(egui::Key::Enter) {
            if let Some(it) = &one {
                match it.kind {
                    Kind::Note | Kind::Frame => self.mb.start_edit(&it.id),
                    Kind::Swatch => self.open_color(&it.id),
                    Kind::Image => self.mb.fit_selection(),
                }
            }
        }
        let typed: Vec<String> = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
        for t in typed {
            match t.as_str() {
                "v" | "V" if !mods.shift => self.mb.tool = Tool::Select,
                "h" if !mods.shift => self.mb.tool = Tool::Hand,
                "H" => self.mb.toggle_image(false),
                "t" | "T" => self.mb.tool = Tool::Note,
                "c" | "C" => self.mb.tool = Tool::Swatch,
                "f" | "F" => self.mb.tool = Tool::Frame,
                "l" => self.mb.toggle_tray(),
                "L" => self.mb.toggle_lock(),
                "g" | "G" => self.mb.toggle_image(true),
                "p" | "P" => {
                    if let Some(it) = one.as_ref().filter(|i| i.kind == Kind::Image) {
                        self.extract_palette(&it.id);
                    }
                }
                "r" => {
                    if let Some(it) = one.as_ref() {
                        let known = self.known(it).is_some();
                        self.mb.start_crop(&it.id, known);
                    }
                }
                "R" => {
                    if let Some(it) = one.as_ref() {
                        self.mb.remove_crop(&it.id);
                    }
                }
                "m" | "M" => {
                    let off = self.mb.prefs["minimap"].as_str() == Some("off");
                    self.mb.prefs_set("minimap", serde_json::json!(if off { "auto" } else { "off" }));
                }
                "O" => {
                    let all = self.mb.sel.len() < 2;
                    self.mb.arrange(all);
                }
                "?" => self.mb.help_open = true,
                "+" | "=" => self.mb.zoom_by(1.25),
                "-" => self.mb.zoom_by(0.8),
                "]" => self.mb.reorder("forward"),
                "[" => self.mb.reorder("backward"),
                _ => {}
            }
        }
    }

    fn board_menu(&mut self, resp: &egui::Response, origin: Pos2, p: Palette, notice: &mut Option<String>) {
        if resp.secondary_clicked() || (resp.hovered() && resp.ctx.input(|i| i.pointer.secondary_pressed())) {
            if let Some(pos) = resp.ctx.input(|i| i.pointer.interact_pos()) {
                self.mb.commit_edit();
                self.mb.apply_crop();
                // the item under the pointer joins (or becomes) the selection
                let w = self.mb.to_world(origin, pos);
                let under = self.mb.items().iter().rev().find(|it| match it.kind {
                    Kind::Frame => false,
                    _ => it.contains(w.0, w.1),
                }).map(|i| i.id.clone());
                if let Some(id) = &under {
                    if !self.mb.sel.contains(id) {
                        self.mb.sel = HashSet::from([id.clone()]);
                    }
                }
                self.mb.menu_hit = Some(under);
                self.mb.menu_at = w;
            }
        }
        let on_item = matches!(self.mb.menu_hit, Some(Some(_)));
        resp.context_menu(|ui| {
            ui.set_min_width(230.0);
            let at = self.mb.menu_at;
            if on_item && !self.mb.sel.is_empty() {
                let one = self.mb.one();
                let imgs = self.mb.selected().iter().any(|i| i.kind == Kind::Image);
                let locked = self.mb.selected().iter().all(|i| i.locked);
                if imgs && kit::menu_entry(ui, icons::FLIP_HORIZONTAL_2, "Espelhar", "⇧H", true, false, p) {
                    self.mb.toggle_image(false);
                    ui.close_menu();
                }
                if imgs && kit::menu_entry(ui, icons::CONTRAST, "Tons de cinza", "G", true, false, p) {
                    self.mb.toggle_image(true);
                    ui.close_menu();
                }
                if let Some(it) = one.clone() {
                    match it.kind {
                        Kind::Image => {
                            let known = self.known(&it).is_some();
                            if !it.locked && kit::menu_entry(ui, icons::CROP, "Recortar", "R", known, false, p) {
                                self.mb.start_crop(&it.id, known);
                                ui.close_menu();
                            }
                            if it.crop.is_some() && !it.locked && kit::menu_entry(ui, icons::CROP, "Desfazer recorte", "⇧R", true, false, p) {
                                self.mb.remove_crop(&it.id);
                                ui.close_menu();
                            }
                            if kit::menu_entry(ui, icons::PIPETTE, "Extrair paleta", "P", known, false, p) {
                                self.extract_palette(&it.id);
                                ui.close_menu();
                            }
                            if kit::menu_entry(ui, icons::FOLDER_OPEN, "Mostrar na pasta", "", known, false, p) {
                                if let Some(i) = self.known(&it) {
                                    reveal(Path::new(&self.entries[i].abs));
                                }
                                ui.close_menu();
                            }
                            if kit::menu_entry(ui, icons::ZOOM_IN, "Aproximar", "⇧2", true, false, p) {
                                self.mb.fit_selection();
                                ui.close_menu();
                            }
                        }
                        Kind::Note => {
                            if kit::menu_entry(ui, icons::PEN_LINE, "Editar texto", "Enter", !it.locked, false, p) {
                                self.mb.start_edit(&it.id);
                                ui.close_menu();
                            }
                        }
                        Kind::Swatch => {
                            if kit::menu_entry(ui, icons::DROPLET, "Cor…", "Enter", !it.locked, false, p) {
                                self.open_color(&it.id);
                                ui.close_menu();
                            }
                        }
                        Kind::Frame => {
                            if kit::menu_entry(ui, icons::PEN_LINE, "Renomear", "Enter", !it.locked, false, p) {
                                self.mb.start_edit(&it.id);
                                ui.close_menu();
                            }
                        }
                    }
                }
                kit::menu_sep(ui, p);
                if kit::menu_entry(ui, icons::BRING_TO_FRONT, "Trazer para a frente", "Ctrl+]", true, false, p) {
                    self.mb.reorder("front");
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::SEND_TO_BACK, "Enviar para trás", "Ctrl+[", true, false, p) {
                    self.mb.reorder("back");
                    ui.close_menu();
                }
                kit::menu_sep(ui, p);
                if self.mb.sel.len() > 1 && kit::menu_entry(ui, icons::LAYOUT_DASHBOARD, "Organizar", "⇧O", true, false, p) {
                    self.mb.arrange(false);
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::COPY, "Duplicar", "Ctrl+D", true, false, p) {
                    self.mb.duplicate();
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::COPY, "Copiar", "Ctrl+C", true, false, p) {
                    self.mb.copy(notice);
                    ui.close_menu();
                }
                if kit::menu_entry(ui, if locked { icons::LOCK_OPEN } else { icons::LOCK }, if locked { "Destravar" } else { "Travar" }, "⇧L", true, false, p) {
                    self.mb.toggle_lock();
                    ui.close_menu();
                }
                kit::menu_sep(ui, p);
                if kit::menu_entry(ui, icons::TRASH_2, "Excluir", "⌫", true, true, p) {
                    self.mb.delete_selection(notice);
                    ui.close_menu();
                }
            } else {
                let has = !self.mb.items().is_empty();
                if kit::menu_entry(ui, icons::CLIPBOARD, "Colar", "Ctrl+V", !self.mb.clip.is_empty(), false, p) {
                    self.mb.paste(at);
                    ui.close_menu();
                }
                kit::menu_sep(ui, p);
                if kit::menu_entry(ui, icons::TYPE, "Nota", "T", true, false, p) {
                    self.mb.create_note(at);
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::DROPLET, "Amostra de cor", "C", true, false, p) {
                    let id = self.mb.create_swatch(at, None);
                    self.open_color(&id);
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::FRAME, "Seção", "F", true, false, p) {
                    let z = self.mb.cam.z;
                    self.mb.create_frame(Rect { x: at.0 - 320.0 / z, y: at.1 - 210.0 / z, w: 640.0 / z, h: 420.0 / z });
                    ui.close_menu();
                }
                kit::menu_sep(ui, p);
                if kit::menu_entry(ui, icons::SQUARE_CHECK, "Selecionar tudo", "Ctrl+A", has, false, p) {
                    self.mb.select_all();
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::LAYOUT_DASHBOARD, "Organizar tudo", "⇧O", self.mb.items().len() > 1, false, p) {
                    self.mb.arrange(true);
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::MAXIMIZE, "Ver tudo", "⇧1", true, false, p) {
                    self.mb.fit_all();
                    ui.close_menu();
                }
                kit::menu_sep(ui, p);
                if kit::menu_entry(ui, icons::IMAGES, "Biblioteca", "L", true, false, p) {
                    self.mb.toggle_tray();
                    ui.close_menu();
                }
                let focus = self.mb.board.as_ref().is_some_and(|b| b.mode == "focus");
                if kit::menu_entry(ui, icons::FOCUS, if focus { "Modo estúdio" } else { "Modo foco" }, "Tab", true, false, p) {
                    self.mb.set_mode(!focus, notice);
                    ui.close_menu();
                }
                if kit::menu_entry(ui, icons::CIRCLE_HELP, "Atalhos", "?", true, false, p) {
                    self.mb.help_open = true;
                    ui.close_menu();
                }
            }
        });
    }

    /// The text box over a note or a frame title being typed.
    fn edit_ui(&mut self, ui: &mut egui::Ui, origin: Pos2, ink: Color32) {
        let Some(ed) = &self.mb.edit else { return };
        let Some(it) = self.mb.item(&ed.id).cloned() else {
            self.mb.edit = None;
            return;
        };
        let z = self.mb.cam.z;
        let id = egui::Id::new(("mb-edit", &it.id));
        let center = self.mb.to_screen(origin, it.center());
        let top_left = self.mb.to_screen(origin, (it.x, it.y));
        let ed = self.mb.edit.as_mut().unwrap();
        let resp = if ed.note {
            let fs = it.fs.unwrap_or(20.0);
            let filled = it.fill.as_deref().and_then(mb::parse_hex);
            let (padx, pady) = if filled.is_some() { (0.9 * fs, 0.8 * fs) } else { (0.6 * fs, 0.45 * fs) };
            let color = if filled.is_some() { Color32::from_rgb(0x1d, 0x1d, 0x22) } else { ink };
            // typed unrotated, where the note is
            let c = center;
            let size = egui::vec2(((it.w - padx * 2.0) * z) as f32, ((it.h - pady * 2.0) * z).max(fs * 1.32 * z) as f32);
            let rect = egui::Rect::from_center_size(c, size);
            let font = egui::FontId::proportional((fs * z) as f32);
            ui.put(rect, egui::TextEdit::multiline(&mut ed.buf).id(id).font(font).text_color(color).frame(false).margin(egui::Margin::ZERO).desired_width(size.x).hint_text("Escreva…"))
        } else {
            let rect = egui::Rect::from_min_size(top_left + egui::vec2(0.0, -24.0), egui::vec2(260.0, 22.0));
            ui.put(rect, egui::TextEdit::singleline(&mut ed.buf).id(id).font(kit::medium(13.0)).text_color(ink).margin(egui::Margin::symmetric(2, 2)))
        };
        if !ed.focused {
            resp.request_focus();
            // everything selected, ready to be replaced
            let n = ed.buf.chars().count();
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                st.store(ui.ctx(), id);
            }
            ed.focused = true;
        } else if resp.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.mb.commit_edit();
        }
    }

    // ── chrome: islands, selection bar, tray, minimap ─────────────────────

    fn island(ui: &mut egui::Ui, rect: egui::Rect, p: Palette, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
        let frame = egui::Frame::new().fill(p.bg).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(5, 5)).shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 70 } else { 22 }) });
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        frame.show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            add(ui)
        }).response
    }

    fn islands(&mut self, ui: &mut egui::Ui, canvas: egui::Rect, p: Palette, notice: &mut Option<String>) {
        let origin = canvas.min;
        // top left: back, name, saved
        let mut back = false;
        let r = egui::Rect::from_min_size(canvas.min + egui::vec2(12.0, 12.0), egui::vec2(420.0, ISLAND));
        let save = self.mb.save;
        let rename_now = self.mb.rename.as_ref().is_some_and(|(id, _)| id == "\u{0}board");
        let tr = Self::island(ui, r, p, |ui| {
            if kit::icon_button(ui, icons::ARROW_LEFT, "Moodboards", false, p).clicked() {
                back = true;
            }
            let w = ui.fonts(|f| f.layout_no_wrap(self.mb.name_buf.clone(), kit::medium(14.0), p.text).size().x).clamp(70.0, 300.0) + 16.0;
            let te = ui.add(egui::TextEdit::singleline(&mut self.mb.name_buf).font(kit::medium(14.0)).frame(false).desired_width(w).margin(egui::Margin::symmetric(6, 4)));
            if rename_now {
                te.request_focus();
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.mb.name_buf.chars().count()))));
                    st.store(ui.ctx(), te.id);
                }
            }
            if te.changed() {
                let name = self.mb.name_buf.trim();
                let name = if name.is_empty() { mb::UNTITLED.to_string() } else { name.to_string() };
                if let Some(b) = &mut self.mb.board {
                    b.name = name;
                }
                self.mb.mark(false);
            }
            if te.lost_focus() && self.mb.name_buf.trim().is_empty() {
                self.mb.name_buf = mb::UNTITLED.into();
            }
            let (t, c) = match save {
                Save::Saved => ("Salvo", p.faint),
                Save::Pending => ("Salvando…", p.faint),
                Save::Error => ("Erro ao salvar", p.error),
            };
            ui.label(RichText::new(t).size(11.0).color(c));
            ui.add_space(4.0);
        });
        if rename_now {
            self.mb.rename = None;
        }
        let _ = tr;
        if back {
            self.mb.leave_board();
            return;
        }
        // tools, centred
        let tools = [(Tool::Select, icons::MOUSE_POINTER_2, "Selecionar — V"), (Tool::Hand, icons::HAND, "Mão — H"), (Tool::Note, icons::TYPE, "Nota — T"), (Tool::Swatch, icons::DROPLET, "Amostra de cor — C"), (Tool::Frame, icons::FRAME, "Seção — F")];
        let tw = 5.0 * 30.0 + 12.0 + 8.0;
        let r = egui::Rect::from_min_size(egui::pos2(canvas.center().x - tw / 2.0, canvas.top() + 12.0), egui::vec2(tw, ISLAND));
        Self::island(ui, r, p, |ui| {
            for (i, (t, g, tip)) in tools.iter().enumerate() {
                if i == 2 {
                    ui.add_space(4.0);
                    let (sr, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), Sense::hover());
                    ui.painter().vline(sr.center().x, sr.y_range(), Stroke::new(1.0_f32, p.border));
                    ui.add_space(4.0);
                }
                if kit::icon_button(ui, g, tip, self.mb.tool == *t, p).clicked() {
                    self.mb.tool = *t;
                }
            }
        });
        // top right
        let r = egui::Rect::from_min_size(egui::pos2(canvas.right() - 12.0 - 4.0 * 30.0 - 12.0, canvas.top() + 12.0), egui::vec2(4.0 * 30.0 + 12.0, ISLAND));
        let mut look_btn: Option<egui::Response> = None;
        Self::island(ui, r, p, |ui| {
            if kit::icon_button(ui, icons::IMAGES, "Biblioteca — L", self.mb.tray_open, p).clicked() {
                self.mb.toggle_tray();
            }
            let lb = kit::icon_button(ui, icons::PALETTE, "Aparência", self.mb.look_open, p);
            if lb.clicked() {
                self.mb.look_open = !self.mb.look_open;
            }
            look_btn = Some(lb);
            if kit::icon_button(ui, icons::FOCUS, "Modo foco — Tab", false, p).clicked() {
                self.mb.set_mode(true, notice);
            }
            if kit::icon_button(ui, icons::CIRCLE_HELP, "Atalhos — ?", self.mb.help_open, p).clicked() {
                self.mb.help_open = true;
            }
        });
        // bottom left: zoom
        let r = egui::Rect::from_min_size(egui::pos2(canvas.left() + 12.0 + if self.mb.tray_open && canvas.width() > 760.0 { TRAY_W + 16.0 } else { 0.0 }, canvas.bottom() - 12.0 - ISLAND), egui::vec2(4.0 * 30.0 + 56.0, ISLAND));
        Self::island(ui, r, p, |ui| {
            if kit::icon_button(ui, icons::MINUS, "Afastar", false, p).clicked() {
                self.mb.zoom_by(0.8);
            }
            let label = format!("{:.0}%", self.mb.cam.z * 100.0);
            if ui.add(egui::Button::new(RichText::new(label).size(12.0)).frame(false).min_size(egui::vec2(50.0, 28.0))).on_hover_text("100% (Shift+0)").clicked() {
                let c = self.mb.free_rect(self.mb.view).center();
                let to = self.mb.cam_for_zoom(c, 1.0);
                self.mb.animate(to);
            }
            if kit::icon_button(ui, icons::PLUS, "Aproximar", false, p).clicked() {
                self.mb.zoom_by(1.25);
            }
            if kit::icon_button(ui, icons::MAXIMIZE, "Ver tudo (Shift+1)", false, p).clicked() {
                self.mb.fit_all();
            }
        });
        // bottom right: undo, redo
        let r = egui::Rect::from_min_size(egui::pos2(canvas.right() - 12.0 - 2.0 * 30.0 - 12.0, canvas.bottom() - 12.0 - ISLAND), egui::vec2(2.0 * 30.0 + 12.0, ISLAND));
        Self::island(ui, r, p, |ui| {
            ui.add_enabled_ui(!self.mb.undo.is_empty(), |ui| {
                if kit::icon_button(ui, icons::UNDO_2, "Desfazer (Ctrl+Z)", false, p).clicked() {
                    self.mb.undo();
                }
            });
            ui.add_enabled_ui(!self.mb.redo.is_empty(), |ui| {
                if kit::icon_button(ui, icons::REDO_2, "Refazer (Ctrl+Shift+Z)", false, p).clicked() {
                    self.mb.redo();
                }
            });
        });
        self.minimap(ui, canvas, p);
        if self.mb.tray_open {
            self.tray_ui(ui, canvas, p);
        }
        self.selbar(ui, origin, canvas, p);
        if self.mb.look_open {
            if let Some(lb) = look_btn {
                self.look_ui(ui, &lb, p);
            }
        }
        self.color_ui(ui, origin, p);
    }

    fn selbar(&mut self, ui: &mut egui::Ui, origin: Pos2, canvas: egui::Rect, p: Palette) {
        let sel: Vec<Item> = self.mb.selected().into_iter().cloned().collect();
        if sel.is_empty() || self.mb.edit.is_some() || self.mb.op.is_some() || self.mb.color_before.is_some() {
            return;
        }
        let mut bb = mb::bbox(&sel);
        if let Some(st) = &self.mb.crop {
            let r = st.rect;
            let pts = [(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)].map(|q| st.world(q));
            let (x0, x1) = (pts.iter().map(|q| q.0).fold(f64::INFINITY, f64::min), pts.iter().map(|q| q.0).fold(f64::NEG_INFINITY, f64::max));
            let (y0, y1) = (pts.iter().map(|q| q.1).fold(f64::INFINITY, f64::min), pts.iter().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max));
            bb = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        }
        let a = self.mb.to_screen(origin, (bb.x, bb.y));
        let b = self.mb.to_screen(origin, (bb.x + bb.w, bb.y + bb.h));
        let kinds: HashSet<Kind> = sel.iter().map(|i| i.kind).collect();
        let only = (kinds.len() == 1).then(|| sel[0].kind);
        // the bar's width: measured from what goes in it
        let n_buttons = if self.mb.crop.is_some() { 0 } else {
            let mut n = 5;
            match only {
                Some(Kind::Image) => n += if sel.len() == 1 { 5 } else { 2 },
                Some(Kind::Note) => n += 2 + mb::NOTE_FILLS.len(),
                Some(Kind::Swatch) if sel.len() == 1 => n += 4,
                Some(Kind::Frame) => n += mb::FRAME_TINTS.len() + 1,
                _ => {}
            }
            if sel.len() > 1 && sel.iter().any(|i| i.kind != Kind::Frame) {
                n += 1;
            }
            n
        };
        let bw = if self.mb.crop.is_some() { 250.0 } else { n_buttons as f32 * 30.0 + 40.0 };
        let bh = ISLAND;
        let lift = if self.mb.crop.is_none() && sel.len() == 1 && sel[0].kind != Kind::Frame && !sel[0].locked { 44.0 } else { 14.0 };
        let mut top = a.y - bh - lift;
        if top < canvas.top() + 62.0 {
            top = b.y + 14.0;
        }
        if top + bh > canvas.bottom() - 62.0 {
            top = ((a.y + b.y) / 2.0 - bh / 2.0).clamp(canvas.top() + 62.0, (canvas.bottom() - 62.0 - bh).max(canvas.top() + 62.0));
        }
        let min_left = canvas.left() + if self.mb.tray_open && canvas.width() > 760.0 && self.mb.board.as_ref().is_some_and(|b| b.mode != "focus") { TRAY_W + 28.0 } else { 0.0 } + 8.0;
        let left = ((a.x + b.x) / 2.0 - bw / 2.0).clamp(min_left, (canvas.right() - bw - 8.0).max(min_left));
        let r = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(bw, bh));
        let mut notice = None;
        Self::island(ui, r, p, |ui| {
            if self.mb.crop.is_some() {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add_space(4.0);
                if ui.add(egui::Button::new("Restaurar").frame(false)).on_hover_text("A imagem inteira (Shift+R)").clicked() {
                    if let Some(st) = &mut self.mb.crop {
                        st.reset();
                    }
                }
                if ui.add(egui::Button::new("Cancelar").frame(false)).clicked() {
                    self.mb.crop = None;
                }
                if kit::primary(ui, "Aplicar", p).clicked() {
                    self.mb.apply_crop();
                }
                return;
            }
            let sep = |ui: &mut egui::Ui| {
                ui.add_space(3.0);
                let (sr, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), Sense::hover());
                ui.painter().vline(sr.center().x, sr.y_range(), Stroke::new(1.0_f32, p.border));
                ui.add_space(3.0);
            };
            match only {
                Some(Kind::Image) => {
                    let imgs: Vec<&Item> = sel.iter().filter(|i| i.kind == Kind::Image).collect();
                    if kit::icon_button(ui, icons::FLIP_HORIZONTAL_2, "Espelhar — Shift+H", imgs.iter().all(|i| i.flip), p).clicked() {
                        self.mb.toggle_image(false);
                    }
                    if kit::icon_button(ui, icons::CONTRAST, "Tons de cinza — G", imgs.iter().all(|i| i.gray), p).clicked() {
                        self.mb.toggle_image(true);
                    }
                    if sel.len() == 1 {
                        let it = &sel[0];
                        let known = self.known(it);
                        if kit::icon_button(ui, icons::CROP, "Recortar — R", it.crop.is_some(), p).clicked() {
                            self.mb.start_crop(&it.id, known.is_some());
                        }
                        if kit::icon_button(ui, icons::PIPETTE, "Extrair paleta — P", false, p).clicked() {
                            self.extract_palette(&it.id);
                        }
                        if kit::icon_button(ui, icons::FOLDER_OPEN, "Mostrar na pasta", false, p).clicked() {
                            if let Some(i) = known {
                                reveal(Path::new(&self.entries[i].abs));
                            }
                        }
                    }
                    sep(ui);
                }
                Some(Kind::Note) => {
                    if kit::icon_button(ui, icons::MINUS, "Texto menor", false, p).clicked() {
                        self.mb.scale_text(0.8);
                    }
                    if kit::icon_button(ui, icons::PLUS, "Texto maior", false, p).clicked() {
                        self.mb.scale_text(1.25);
                    }
                    sep(ui);
                    let cur = if sel.iter().all(|i| i.fill == sel[0].fill) { Some(sel[0].fill.clone()) } else { None };
                    for f in mb::NOTE_FILLS {
                        let on = cur.as_ref().is_some_and(|c| c.as_deref() == f);
                        if Self::dot(ui, f.map(hex_color), on, p).clicked() {
                            let v = f.map(String::from);
                            self.mb.set_prop(|i| i.fill = v.clone());
                            self.mb.prefs_set("lastFill", f.map(|s| serde_json::json!(s)).unwrap_or(serde_json::Value::Null));
                        }
                    }
                    sep(ui);
                }
                Some(Kind::Swatch) if sel.len() == 1 => {
                    let it = &sel[0];
                    let mut hex = ui.data(|d| d.get_temp::<(String, String)>(egui::Id::new("mb-hex"))).filter(|h| h.0 == it.id).map(|h| h.1).unwrap_or_else(|| it.color().to_uppercase());
                    let te = ui.add(egui::TextEdit::singleline(&mut hex).font(egui::FontId::monospace(12.0)).desired_width(70.0));
                    if te.changed() {
                        ui.data_mut(|d| d.insert_temp(egui::Id::new("mb-hex"), (it.id.clone(), hex.clone())));
                    }
                    if te.lost_focus() {
                        let v = format!("#{}", hex.trim().trim_start_matches('#')).to_lowercase();
                        if mb::parse_hex(&v).is_some() && v != it.color().to_lowercase() {
                            self.mb.set_prop(|i| i.color = Some(v.clone()));
                            self.mb.prefs_set("lastColor", serde_json::json!(v));
                        }
                        ui.data_mut(|d| d.remove::<(String, String)>(egui::Id::new("mb-hex")));
                    }
                    if kit::icon_button(ui, icons::DROPLET, "Cor", false, p).clicked() {
                        self.open_color(&it.id);
                    }
                    sep(ui);
                }
                Some(Kind::Frame) => {
                    let cur = if sel.iter().all(|i| i.tint.unwrap_or(0) == sel[0].tint.unwrap_or(0)) { Some(sel[0].tint.unwrap_or(0)) } else { None };
                    for (k, t) in mb::FRAME_TINTS.iter().enumerate() {
                        if Self::dot(ui, t.map(hex_color), cur == Some(k), p).clicked() {
                            self.mb.set_prop(|i| i.tint = Some(k));
                        }
                    }
                    if sel.len() == 1 && kit::icon_button(ui, icons::PEN_LINE, "Renomear", false, p).clicked() {
                        self.mb.start_edit(&sel[0].id);
                    }
                    sep(ui);
                }
                _ => {}
            }
            if sel.len() > 1 && sel.iter().any(|i| i.kind != Kind::Frame) {
                if kit::icon_button(ui, icons::LAYOUT_DASHBOARD, "Organizar — Shift+O", false, p).clicked() {
                    self.mb.arrange(false);
                }
                sep(ui);
            }
            let locked = sel.iter().all(|i| i.locked);
            if kit::icon_button(ui, icons::BRING_TO_FRONT, "Trazer para a frente", false, p).clicked() {
                self.mb.reorder("front");
            }
            if kit::icon_button(ui, icons::SEND_TO_BACK, "Enviar para trás", false, p).clicked() {
                self.mb.reorder("back");
            }
            if kit::icon_button(ui, if locked { icons::LOCK_OPEN } else { icons::LOCK }, if locked { "Destravar — Shift+L" } else { "Travar — Shift+L" }, locked, p).clicked() {
                self.mb.toggle_lock();
            }
            if kit::icon_button(ui, icons::COPY, "Duplicar — Ctrl+D", false, p).clicked() {
                self.mb.duplicate();
            }
            if kit::icon_button(ui, icons::TRASH_2, "Excluir", false, p).clicked() {
                self.mb.delete_selection(&mut notice);
            }
        });
        if let Some(n) = notice {
            self.notice = Some(n);
        }
    }

    fn dot(ui: &mut egui::Ui, c: Option<Color32>, on: bool, p: Palette) -> egui::Response {
        let (r, resp) = ui.allocate_exact_size(egui::vec2(24.0, 28.0), Sense::click());
        let center = r.center();
        match c {
            Some(c) => {
                ui.painter().circle(center, 8.0, c, Stroke::new(1.0_f32, Color32::from_black_alpha(30)));
            }
            None => {
                ui.painter().circle_stroke(center, 8.0, Stroke::new(1.0_f32, p.border_strong));
                ui.painter().line_segment([center + egui::vec2(-5.5, 5.5), center + egui::vec2(5.5, -5.5)], Stroke::new(1.0_f32, p.muted));
            }
        }
        if on {
            ui.painter().circle_stroke(center, 11.0, Stroke::new(1.5_f32, p.accent_strong));
        } else if resp.hovered() {
            ui.painter().circle_stroke(center, 11.0, Stroke::new(1.0_f32, p.border_strong));
        }
        resp
    }

    /// The colour of a swatch being picked.
    fn color_ui(&mut self, ui: &mut egui::Ui, origin: Pos2, p: Palette) {
        let Some((id, before)) = self.mb.color_before.clone() else { return };
        let Some(it) = self.mb.item(&id).cloned() else {
            self.mb.color_before = None;
            return;
        };
        let mut c = hex_color(it.color());
        let at = self.mb.to_screen(origin, (it.x + it.w, it.y));
        let mut done = false;
        let area = egui::Area::new(egui::Id::new("mb-color")).order(egui::Order::Foreground).fixed_pos(at + egui::vec2(12.0, 0.0)).constrain(true).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
                    let v = mb::hex([c.r(), c.g(), c.b()]);
                    if let Some(i) = self.mb.item_mut(&id) {
                        i.color = Some(v);
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if kit::primary(ui, "Pronto", p).clicked() {
                        done = true;
                    }
                });
            });
        });
        if done || area.response.clicked_elsewhere() || ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
            self.mb.color_before = None;
            let now = self.mb.item(&id).map(|i| i.color().to_string()).unwrap_or_default();
            if self.mb.snapshot() != before {
                self.mb.push_history(before);
                self.mb.prefs_set("lastColor", serde_json::json!(now));
                self.mb.mark(false);
            }
        }
    }

    fn look_ui(&mut self, ui: &mut egui::Ui, anchor: &egui::Response, p: Palette) {
        let pos = egui::pos2(anchor.rect.right() - 280.0, anchor.rect.bottom() + 14.0);
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("mb-look")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(270.0);
                let Some(b) = self.mb.board.clone() else { return };
                kit::section(ui, "Fundo", p);
                ui.horizontal(|ui| {
                    for bg in &mb::BACKGROUNDS {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(46.0, 34.0), Sense::click());
                        ui.painter().rect(r, 7.0, rgb(bg.color), Stroke::new(if b.bg == bg.key { 2.0_f32 } else { 1.0 }, if b.bg == bg.key { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
                        let d = Color32::from_rgba_unmultiplied(bg.dot[0], bg.dot[1], bg.dot[2], (bg.dot[3] as u16 * 2).min(255) as u8);
                        for k in 0..6 {
                            ui.painter().circle_filled(r.min + egui::vec2(9.0 + (k % 3) as f32 * 14.0, 11.0 + (k / 3) as f32 * 12.0), 1.0, d);
                        }
                        if resp.on_hover_text(bg.label).clicked() {
                            if let Some(bm) = &mut self.mb.board {
                                bm.bg = bg.key.into();
                            }
                            self.mb.prefs_set("lastBg", serde_json::json!(bg.key));
                            self.mb.mark(true);
                        }
                    }
                });
                ui.add_space(6.0);
                kit::section(ui, "Padrão", p);
                let pats = ["plain", "dots", "grid"];
                if let Some(i) = kit::segmented(ui, &[("", "Liso"), ("", "Pontos"), ("", "Grade")], pats.iter().position(|x| *x == b.pattern).unwrap_or(1), p) {
                    if let Some(bm) = &mut self.mb.board {
                        bm.pattern = pats[i].into();
                    }
                    self.mb.prefs_set("lastPattern", serde_json::json!(pats[i]));
                    self.mb.mark(true);
                }
                ui.add_space(6.0);
                kit::section(ui, "Roda do mouse", p);
                let zoom = self.mb.prefs["wheel"].as_str() == Some("zoom");
                if let Some(i) = kit::segmented(ui, &[("", "Move"), ("", "Zoom")], zoom as usize, p) {
                    self.mb.prefs_set("wheel", serde_json::json!(if i == 1 { "zoom" } else { "pan" }));
                }
                ui.add_space(6.0);
                kit::section(ui, "Minimapa", p);
                let off = self.mb.prefs["minimap"].as_str() == Some("off");
                if let Some(i) = kit::segmented(ui, &[("", "Automático"), ("", "Desligado")], off as usize, p) {
                    self.mb.prefs_set("minimap", serde_json::json!(if i == 1 { "off" } else { "auto" }));
                }
                if self.shared.is_some() {
                    ui.add_space(6.0);
                    kit::section(ui, "Biblioteca", p);
                    if let Some(i) = kit::segmented(ui, &[("", "Guardar numa coleção"), ("", "Não")], (!b.tags_on()) as usize, p) {
                        if let Some(bm) = &mut self.mb.board {
                            bm.tag_library = if i == 0 { None } else { Some(false) };
                        }
                        self.mb.mark(true);
                    }
                    let hint = if b.tags_on() { format!("As imagens do quadro entram na coleção {} (só são acrescentadas; tirar é no Pictogrep).", mb::board_collection(&b.name)) } else { "As imagens do quadro não são guardadas em coleção.".into() };
                    ui.label(RichText::new(hint).size(11.5).color(p.muted));
                }
            });
        });
        if area.response.clicked_elsewhere() && !anchor.clicked() && !anchor.hovered() {
            close = true;
        }
        if close {
            self.mb.look_open = false;
        }
    }

    fn help_ui(&mut self, ctx: &egui::Context, p: Palette) {
        if !self.mb.help_open {
            return;
        }
        let mut open = true;
        let rows: [(&str, &[(&str, &str)]); 4] = [
            ("Quadro", &[("Mover o quadro", "Espaço + arrastar · botão do meio"), ("Rolar / zoom", "Roda · Ctrl + roda"), ("Ver tudo", "Shift+1"), ("Ver a seleção", "Shift+2"), ("Zoom 100%", "Shift+0"), ("Estúdio / foco", "Tab"), ("Biblioteca", "L"), ("Minimapa", "M")]),
            ("Ferramentas", &[("Selecionar", "V"), ("Mão", "H"), ("Nota", "T · duplo clique"), ("Amostra de cor", "C"), ("Seção", "F")]),
            ("Edição", &[("Vários", "Shift/Ctrl + clique"), ("Duplicar arrastando", "Alt + arrastar"), ("Sem alinhar", "Ctrl + arrastar"), ("Empurrar", "setas · Shift"), ("Ordem", "[ ] · Ctrl+[ Ctrl+]"), ("Travar", "Shift+L"), ("Duplicar", "Ctrl+D"), ("Copiar / colar", "Ctrl+C · Ctrl+V"), ("Desfazer / refazer", "Ctrl+Z · Ctrl+Shift+Z"), ("Excluir", "Delete"), ("Editar item", "Enter · duplo clique")]),
            ("Imagem", &[("Espelhar", "Shift+H"), ("Tons de cinza", "G"), ("Paleta", "P"), ("Organizar", "Shift+O"), ("Aproximar", "duplo clique · Enter"), ("Recortar / desfazer", "R · Shift+R")]),
        ];
        egui::Window::new("Atalhos do moodboard").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.set_width(520.0);
            egui::Grid::new("mb-help").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                for (title, list) in rows {
                    ui.label(RichText::new(title).font(kit::medium(12.5)).color(p.faint));
                    ui.end_row();
                    for (a, b) in list {
                        ui.label(RichText::new(*a).color(p.text));
                        ui.label(RichText::new(*b).size(12.0).color(p.muted));
                        ui.end_row();
                    }
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Arrastar uma seção leva junto o que está dentro dela.").size(12.0).color(p.muted));
        });
        if !open {
            self.mb.help_open = false;
        }
    }

    fn minimap(&mut self, ui: &mut egui::Ui, canvas: egui::Rect, p: Palette) {
        if self.mb.prefs["minimap"].as_str() == Some("off") || self.mb.items().is_empty() {
            return;
        }
        let (mw, mh) = (200.0f32, 132.0f32);
        let r = egui::Rect::from_min_size(egui::pos2(canvas.right() - 14.0 - mw - 8.0, canvas.bottom() - 62.0 - mh - 8.0), egui::vec2(mw + 8.0, mh + 8.0));
        let content = mb::bbox(self.mb.items());
        let fr = self.mb.free_rect(self.mb.view);
        let a = self.mb.to_world(Pos2::ZERO, fr.min);
        let b = self.mb.to_world(Pos2::ZERO, fr.max);
        let view = Rect { x: a.0, y: a.1, w: b.0 - a.0, h: b.1 - a.1 };
        let hovered = ui.rect_contains_pointer(r);
        let dragging = matches!(self.mb.op, Some(Op::Mini));
        if view.contains_rect(&content) && !hovered && !dragging {
            return;
        }
        // the scale stays put while dragging inside the map
        if !dragging || self.mb.mini.is_none() {
            let (x0, y0) = (content.x.min(view.x), content.y.min(view.y));
            let (x1, y1) = ((content.x + content.w).max(view.x + view.w), (content.y + content.h).max(view.y + view.h));
            let pad = 8.0;
            let s = ((mw as f64 - pad * 2.0) / (x1 - x0)).min((mh as f64 - pad * 2.0) / (y1 - y0));
            self.mb.mini = Some((s, (mw as f64 - (x1 - x0) * s) / 2.0 - x0 * s, (mh as f64 - (y1 - y0) * s) / 2.0 - y0 * s));
        }
        let (s, ox, oy) = self.mb.mini.unwrap();
        let bg = mb::background(&self.mb.board.as_ref().unwrap().bg);
        let mut map_rect = egui::Rect::NOTHING;
        Self::island(ui, r, p, |ui| {
            let (mr, _) = ui.allocate_exact_size(egui::vec2(mw, mh), Sense::hover());
            map_rect = mr;
            let painter = ui.painter_at(mr);
            painter.rect_filled(mr, 4.0, rgb(bg.color));
            let ink = if bg.dark { Color32::WHITE } else { Color32::BLACK };
            for it in self.mb.items() {
                let c = it.center();
                let at = mr.min + egui::vec2((ox + c.0 * s) as f32, (oy + c.1 * s) as f32);
                let sz = egui::vec2((it.w * s).max(1.0) as f32, (it.h * s).max(1.0) as f32);
                let rr = egui::Rect::from_center_size(at, sz);
                let fill = match it.kind {
                    Kind::Image => ink.gamma_multiply(0.25),
                    Kind::Note => it.fill.as_deref().map(hex_color).unwrap_or(ink.gamma_multiply(0.3)),
                    Kind::Swatch => hex_color(it.color()),
                    Kind::Frame => Color32::TRANSPARENT,
                };
                painter.rect_filled(rr, 0.0, fill);
                if it.kind == Kind::Frame {
                    painter.rect_stroke(rr, 0.0, Stroke::new(1.0_f32, ink.gamma_multiply(0.35)), egui::StrokeKind::Inside);
                }
            }
            let vr = egui::Rect::from_min_size(mr.min + egui::vec2((ox + view.x * s) as f32, (oy + view.y * s) as f32), egui::vec2((view.w * s) as f32, (view.h * s) as f32));
            painter.rect_stroke(vr, 1.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Middle);
        });
        let resp = ui.interact(map_rect, egui::Id::new("mb-minimap"), Sense::click_and_drag()).on_hover_text("Clique ou arraste para ir até lá");
        if resp.is_pointer_button_down_on() {
            self.mb.op = Some(Op::Mini);
            if let Some(pos) = resp.interact_pointer_pos() {
                let (wx, wy) = (((pos.x - map_rect.left()) as f64 - ox) / s, ((pos.y - map_rect.top()) as f64 - oy) / s);
                let c = fr.center();
                self.mb.cam.x = c.x as f64 - wx * self.mb.cam.z;
                self.mb.cam.y = c.y as f64 - wy * self.mb.cam.z;
                self.mb.anim = None;
                self.mb.zoomed_to = None;
            }
        }
        if dragging && !ui.input(|i| i.pointer.primary_down()) {
            self.mb.op = None;
            self.mb.mark(true);
        }
    }

    fn tray_list(&self) -> Vec<usize> {
        let q = self.mb.tray.query.trim().to_lowercase();
        let tag = &self.mb.tray.tag;
        let members = tag.strip_prefix("c:").and_then(|c| self.collections.iter().find(|x| x.0 == c)).map(|x| &x.1);
        let mut base: Vec<usize> = match &self.mb.tray.shuffled {
            Some(s) => s.clone(),
            None => {
                let mut v: Vec<usize> = (0..self.entries.len()).collect();
                v.sort_by_key(|&i| std::cmp::Reverse(self.entries[i].mtime));
                v
            }
        };
        base.retain(|&i| {
            let Some(e) = self.entries.get(i) else { return false };
            if let Some(f) = tag.strip_prefix("f:") {
                if e.folder != f {
                    return false;
                }
            }
            if tag.starts_with("c:") && !members.is_some_and(|m| m.contains(&e.abs)) {
                return false;
            }
            q.is_empty() || e.name.to_lowercase().contains(&q) || e.folder.to_lowercase().contains(&q)
        });
        base
    }

    fn tray_ui(&mut self, ui: &mut egui::Ui, canvas: egui::Rect, p: Palette) {
        let r = egui::Rect::from_min_max(canvas.min + egui::vec2(12.0, ISLAND + 24.0), egui::pos2(canvas.left() + 12.0 + TRAY_W, canvas.bottom() - ISLAND - 24.0));
        if r.height() < 160.0 {
            return;
        }
        let list = self.tray_list();
        let used: HashSet<usize> = self.mb.items().iter().filter_map(|i| self.known(i)).collect();
        let frame = egui::Frame::new().fill(p.bg).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(12.0).inner_margin(10.0).shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 70 } else { 22 }) });
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
        let mut place: Option<Vec<usize>> = None;
        frame.show(&mut child, |ui| {
            ui.set_min_size(r.size() - egui::vec2(20.0, 20.0));
            ui.set_max_size(r.size() - egui::vec2(20.0, 20.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Biblioteca").font(kit::medium(13.0)).color(p.text));
                ui.label(RichText::new(if self.scanning.is_some() { "carregando…".to_string() } else { format!("{}", list.len()) }).size(12.0).color(p.faint));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if kit::icon_button_sized(ui, icons::X, "Fechar (L)", false, p, 24.0, 13.0).clicked() {
                        self.mb.toggle_tray();
                    }
                    if kit::icon_button_sized(ui, icons::SHUFFLE, "Embaralhar", self.mb.tray.shuffled.is_some(), p, 24.0, 13.0).clicked() {
                        self.mb.tray.shuffled = match self.mb.tray.shuffled {
                            Some(_) => None,
                            None => {
                                let mut v: Vec<usize> = (0..self.entries.len()).collect();
                                // a cheap, good-enough shuffle
                                let mut x = mb::now_ms() as u64 | 1;
                                for i in (1..v.len()).rev() {
                                    x ^= x << 13;
                                    x ^= x >> 7;
                                    x ^= x << 17;
                                    v.swap(i, (x % (i as u64 + 1)) as usize);
                                }
                                Some(v)
                            }
                        };
                    }
                });
            });
            ui.add_space(4.0);
            kit::search_field(ui, &mut self.mb.tray.query, "Buscar na biblioteca", p);
            ui.add_space(6.0);
            // collections and folders as chips
            egui::ScrollArea::horizontal().id_salt("mb-chips").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if kit::chip(ui, None, "Todas", self.mb.tray.tag.is_empty(), p).clicked() {
                        self.mb.tray.tag.clear();
                    }
                    let cols: Vec<String> = self.collections.iter().map(|c| c.0.clone()).collect();
                    for c in cols {
                        let key = format!("c:{c}");
                        if kit::chip(ui, Some(icons::FOLDER_CLOSED), &c.replace('/', " › "), self.mb.tray.tag == key, p).clicked() {
                            self.mb.tray.tag = key;
                        }
                    }
                    for (f, _, _) in self.folders() {
                        let key = format!("f:{f}");
                        let name = if f.is_empty() { "Raiz do cofre".to_string() } else { f.rsplit('/').next().unwrap_or(&f).to_string() };
                        if kit::chip(ui, None, &name, self.mb.tray.tag == key, p).clicked() {
                            self.mb.tray.tag = key;
                        }
                    }
                });
            });
            ui.add_space(6.0);
            let foot_h = 30.0;
            let grid_h = (ui.available_height() - foot_h - 6.0).max(60.0);
            // two-column masonry: each picture goes to the shorter column
            let gap = 6.0;
            let col_w = ((ui.available_width() - 10.0 - gap) / 2.0).floor();
            let mut tops = [0.0f32, 0.0f32];
            let mut cells: Vec<(usize, egui::Rect)> = Vec::with_capacity(list.len());
            for &i in &list {
                let e = &self.entries[i];
                let h = (col_w / super::aspect(e)).clamp(col_w * 0.4, col_w * 2.5);
                let c = if tops[0] <= tops[1] { 0 } else { 1 };
                cells.push((i, egui::Rect::from_min_size(egui::pos2(c as f32 * (col_w + gap), tops[c]), egui::vec2(col_w, h))));
                tops[c] += h + gap;
            }
            let total = tops[0].max(tops[1]);
            let ppp = ui.ctx().pixels_per_point();
            egui::ScrollArea::vertical().id_salt("mb-tray").max_height(grid_h).min_scrolled_height(grid_h).auto_shrink([false, false]).show_viewport(ui, |ui, view| {
                let origin = ui.min_rect().min;
                ui.allocate_space(egui::vec2(col_w * 2.0 + gap, total));
                if list.is_empty() {
                    ui.label(RichText::new(if self.entries.is_empty() { "A biblioteca está vazia." } else { "Nada encontrado." }).size(12.0).color(p.faint));
                }
                for (i, cell) in cells.iter().filter(|(_, c)| c.bottom() >= view.top() - 300.0 && c.top() <= view.bottom() + 300.0) {
                    let rect = cell.translate(origin.to_vec2());
                    let resp = ui.interact(rect, ui.id().with(("mb-thumb", i)), Sense::click_and_drag());
                    ui.painter().rect_filled(rect, 6.0, p.bg_secondary);
                    if let Some(t) = self.thumb_sized(*i, rect.width().max(rect.height()) * ppp, false, false) {
                        ui.painter().add(egui::epaint::RectShape::filled(rect, 6.0, Color32::WHITE).with_texture(t, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))));
                    }
                    let picked = self.mb.tray.picked.contains(i);
                    if picked {
                        ui.painter().rect_stroke(rect, 6.0, Stroke::new(3.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                    } else if resp.hovered() {
                        ui.painter().rect_stroke(rect, 6.0, Stroke::new(2.0_f32, p.accent), egui::StrokeKind::Inside);
                    }
                    if used.contains(i) {
                        // already on the board
                        let c = rect.right_top() + egui::vec2(-12.0, 12.0);
                        ui.painter().circle_filled(c, 8.0, Color32::from_black_alpha(140));
                        ui.painter().text(c, egui::Align2::CENTER_CENTER, icons::CHECK, kit::icon_font(10.0), Color32::WHITE);
                    }
                    if resp.drag_started() {
                        let ids = if picked { self.mb.tray.picked.clone() } else { vec![*i] };
                        resp.dnd_set_drag_payload(ids);
                    }
                    if resp.double_clicked() {
                        place = Some(vec![*i]);
                    } else if resp.clicked() {
                        let multi = ui.input(|inp| inp.modifiers.command || inp.modifiers.shift);
                        let pk = &mut self.mb.tray.picked;
                        if multi {
                            match pk.iter().position(|x| x == i) {
                                Some(k) => {
                                    pk.remove(k);
                                }
                                None => pk.push(*i),
                            }
                        } else if pk.len() == 1 && pk[0] == *i {
                            pk.clear();
                        } else {
                            *pk = vec![*i];
                        }
                    }
                    resp.on_hover_text(&self.entries[*i].name);
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if self.mb.tray.picked.is_empty() {
                    ui.label(RichText::new("Arraste para o quadro · Ctrl-clique escolhe várias").size(11.5).color(p.faint));
                } else {
                    let n = self.mb.tray.picked.len();
                    if kit::primary(ui, &format!("Adicionar {n}"), p).clicked() {
                        place = Some(self.mb.tray.picked.clone());
                    }
                    if kit::quiet(ui, None, "Limpar", p).clicked() {
                        self.mb.tray.picked.clear();
                    }
                }
            });
        });
        if let Some(idx) = place {
            let at = self.mb.view_center_world();
            self.place_images(&idx, at);
            self.mb.tray.picked.clear();
        }
    }

    /// Pictures from the library onto the open board, centred on `at`.
    fn place_images(&mut self, idx: &[usize], at: (f64, f64)) {
        let idx: Vec<usize> = idx.iter().copied().filter(|&i| i < self.entries.len()).collect();
        if idx.is_empty() {
            return;
        }
        let z = self.mb.cam.z;
        let aspects: Vec<f64> = idx.iter().map(|&i| super::aspect(&self.entries[i]) as f64).collect();
        let boxes = mb::place(&aspects, at, 320.0 / z, 220.0 / z, 14.0 / z);
        let before = self.mb.snapshot();
        self.mb.push_history(before);
        let items = idx.iter().zip(boxes).map(|(&i, r)| Item::image(&ateliet_core::pictogrep::image_id(Path::new(&self.entries[i].abs)), r.x, r.y, r.w, r.h)).collect();
        self.mb.add_items(items, true);
    }

    /// Pictures dropped on the canvas (from the tray or the mosaic).
    fn drop_ui(&mut self, ui: &egui::Ui, origin: Pos2, resp: &egui::Response, sel_color: Color32) {
        let carrying = egui::DragAndDrop::payload::<Vec<usize>>(ui.ctx());
        let Some(ids) = carrying else { return };
        let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else { return };
        // what's carried, under the pointer
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("mb-drag")));
        if let Some(&first) = ids.first() {
            let e = &self.entries[first];
            let a = super::aspect(e);
            let size = if a >= 1.0 { egui::vec2(90.0, 90.0 / a) } else { egui::vec2(90.0 * a, 90.0) };
            let r = egui::Rect::from_min_size(pos + egui::vec2(10.0, 10.0), size);
            if let Some(t) = self.thumb(first) {
                painter.add(egui::epaint::RectShape::filled(r, 6.0, Color32::from_white_alpha(220)).with_texture(t, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))));
            }
            if ids.len() > 1 {
                let c = r.right_top();
                painter.circle_filled(c, 11.0, sel_color);
                painter.text(c, egui::Align2::CENTER_CENTER, ids.len().to_string(), egui::FontId::proportional(11.0), Color32::WHITE);
            }
        }
        let over = resp.rect.contains(pos) && !(self.mb.tray_open && self.mb.board.as_ref().is_some_and(|b| b.mode != "focus") && pos.x < resp.rect.left() + TRAY_W + 16.0);
        if over {
            ui.painter().rect_stroke(resp.rect.shrink(3.0), 8.0, Stroke::new(2.0_f32, sel_color.gamma_multiply(0.7)), egui::StrokeKind::Inside);
            if ui.input(|i| i.pointer.any_released()) {
                if let Some(ids) = egui::DragAndDrop::take_payload::<Vec<usize>>(ui.ctx()) {
                    let w = self.mb.to_world(origin, pos);
                    self.place_images(&ids, w);
                    self.mb.tray.picked.clear();
                }
            }
        }
    }
}
