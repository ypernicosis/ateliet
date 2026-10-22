//! Interactive maps, native: a picture with pins (to notes or to other
//! maps), labels, stamps, routes and areas over it; a ruler that gives the
//! distance and the days of travel; the map in a given year of the world.
//! The data is the plugin's (`scriptorium-mapa` block of the note).

use super::library::Cx;
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::scriptorium::book::{self, Book};
use ateliet_core::scriptorium::enredo::{self as en, str_of};
use ateliet_core::scriptorium::mapa::{self as ma, TimeScale};
use ateliet_core::scriptorium::mundo as mu;
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Mover,
    Pino,
    Rotulo,
    Carimbo,
    Rota,
    Area,
    Regua,
}

const TOOLS: [(Tool, &str, &str); 7] = [
    (Tool::Mover, icons::HAND, "Mover e selecionar"),
    (Tool::Pino, icons::MAP_PIN, "Pino (liga a uma nota ou a outro mapa)"),
    (Tool::Rotulo, icons::TYPE, "Rótulo"),
    (Tool::Carimbo, icons::TREES, "Carimbo"),
    (Tool::Rota, icons::ROUTE, "Rota: clique os pontos, duplo clique termina"),
    (Tool::Area, icons::PENTAGON, "Área ou território"),
    (Tool::Regua, icons::RULER, "Régua: distância e tempo de viagem (duplo clique termina)"),
];

const COLORS: [&str; 7] = ["#8b1e1e", "#3b2a1a", "#2f5d7c", "#3f6b3a", "#7a4f9a", "#b07a1f", "#111111"];

enum Grab {
    Pan(Vec2),
    Layer { id: String, start: [f64; 2], orig: Json, moved: bool },
    Time,
}

/// What a picked file is for.
#[derive(Clone, PartialEq)]
enum PickFor {
    Base,
    Submap(String),
    Stamp,
}

struct LayerEdit {
    id: String,
    fields: Vec<(&'static str, String, String, String)>,
    focus: bool,
}

struct ScaleEdit {
    mollweide: bool,
    km: String,
    raio: String,
    elipse: [String; 4],
}

type Loaded = Option<(egui::ColorImage, f64, f64)>;

pub struct MapaView {
    pub path: String,
    d: Json,
    loaded: bool,
    mtime: Option<SystemTime>,
    dirty: bool,
    last_edit: Option<Instant>,
    tool: Tool,
    stamp: String,
    sel: Option<String>,
    locked: bool,
    tmp: Option<(Tool, Vec<[f64; 2]>)>,
    hover: Option<[f64; 2]>,
    ruler: Option<Vec<[f64; 2]>>,
    trail: Vec<(String, String)>,
    tex: Option<(String, egui::TextureHandle)>,
    iw: f64,
    ih: f64,
    loading: Option<(String, Receiver<Loaded>)>,
    stamps: HashMap<(String, String, String, usize), egui::TextureHandle>,
    grab: Option<Grab>,
    time_open: bool,
    edit: Option<LayerEdit>,
    scale: Option<ScaleEdit>,
    calibrate: Option<String>,
    pick: Option<(PickFor, String)>,
    preview_ellipse: Option<[f64; 4]>,
    menu: Option<String>,
    book: Option<Book>,
    world: Option<Json>,
    world_at: Option<Instant>,
    toast: Option<String>,
    exporting: Option<Receiver<Result<String, String>>>,
    canvas: Rect,
}

impl MapaView {
    pub fn open(path: &str) -> MapaView {
        MapaView {
            path: path.into(),
            d: ma::blank(),
            loaded: false,
            mtime: None,
            dirty: false,
            last_edit: None,
            tool: Tool::Mover,
            stamp: "arvore".into(),
            sel: None,
            locked: false,
            tmp: None,
            hover: None,
            ruler: None,
            trail: Vec::new(),
            tex: None,
            iw: 2000.0,
            ih: 1400.0,
            loading: None,
            stamps: HashMap::new(),
            grab: None,
            time_open: false,
            edit: None,
            scale: None,
            calibrate: None,
            pick: None,
            preview_ellipse: None,
            menu: None,
            book: None,
            world: None,
            world_at: None,
            toast: None,
            exporting: None,
            canvas: Rect::NOTHING,
        }
    }

    pub fn title(&self) -> String {
        name_of(&self.path)
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
    }

    fn file_mtime(&self, root: &Path) -> Option<SystemTime> {
        std::fs::metadata(root.join(&self.path)).and_then(|m| m.modified()).ok()
    }

    pub fn save(&mut self, root: &Path) -> Option<String> {
        if !self.dirty {
            return None;
        }
        let r = ma::save(root, &self.path, &self.d).ok().map(|_| self.path.clone());
        self.dirty = false;
        self.mtime = self.file_mtime(root);
        r
    }

    fn layers(&self) -> Vec<Json> {
        self.d["camadas"].as_array().cloned().unwrap_or_default()
    }

    fn layer_mut(&mut self, id: &str) -> Option<&mut Json> {
        self.d["camadas"].as_array_mut()?.iter_mut().find(|l| l["id"] == id)
    }

    fn ano(&self) -> Option<f64> {
        self.d["ano"].as_f64()
    }

    fn cal(&self) -> Json {
        self.world.as_ref().map(|w| w["calendario"].clone()).unwrap_or(Json::Null)
    }

    /// Follow a pin into another map (the trail lets the writer climb back).
    fn go(&mut self, root: &Path, path: &str, trail: Vec<(String, String)>) {
        let _ = self.save(root);
        let keep_tex = std::mem::take(&mut self.stamps);
        *self = MapaView::open(path);
        self.stamps = keep_tex;
        self.trail = trail;
    }

    fn view(&self) -> Option<(f32, f32, f32)> {
        let v = &self.d["view"];
        Some((v["x"].as_f64()? as f32, v["y"].as_f64()? as f32, v["z"].as_f64()? as f32))
    }

    fn set_view(&mut self, v: (f32, f32, f32)) {
        self.d["view"] = json!({ "x": v.0, "y": v.1, "z": v.2 });
    }

    fn fit(&self, size: Vec2) -> (f32, f32, f32) {
        let z = (size.x / self.iw as f32).min(size.y / self.ih as f32) * 0.95;
        ((size.x - self.iw as f32 * z) / 2.0, (size.y - self.ih as f32 * z) / 2.0, z)
    }

    fn load_image(&mut self, ctx: &egui::Context, root: &Path, cx: &Cx) {
        if let Some((key, rx)) = &self.loading {
            if let Ok(r) = rx.try_recv() {
                let key = key.clone();
                self.loading = None;
                if let Some((img, w, h)) = r {
                    self.iw = w;
                    self.ih = h;
                    self.tex = Some((key.clone(), ctx.load_texture(format!("map:{key}"), img, egui::TextureOptions::LINEAR)));
                } else {
                    self.tex = None;
                }
            }
            return;
        }
        let img = str_of(&self.d, "imagem").to_string();
        if img.is_empty() {
            return;
        }
        let Some(full) = cx.vault.resolve(&img, &self.path).or_else(|| Some(img.clone()).filter(|p| root.join(p).exists())) else { return };
        if self.tex.as_ref().is_some_and(|t| t.0 == full) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let abs = root.join(&full);
        let max = ctx.input(|i| i.max_texture_side).min(4096) as u32;
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let r = image::open(&abs).ok().map(|im| {
                let (w, h) = (im.width() as f64, im.height() as f64);
                let im = if im.width() > max || im.height() > max { im.resize(max, max, image::imageops::FilterType::Triangle) } else { im };
                let rgba = im.to_rgba8();
                (egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()), w, h)
            });
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        self.loading = Some((full, rx));
    }

    fn load_world(&mut self, cx: &Cx) {
        if self.world_at.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        self.world_at = Some(Instant::now());
        let author = cx.sc.author();
        let ix = cx.vault.index.read().unwrap();
        let Some(b) = book::book_for(&ix, &self.path, &author).or_else(|| book::books(&ix, &author).into_iter().find(|b| self.path.starts_with(&format!("{}/", b.folder)))) else { return };
        let home = mu::home(&cx.vault.root, &ix, &b, &author);
        let data = mu::load(&cx.vault.root, &home);
        let ws = data["mundos"].as_array().cloned().unwrap_or_default();
        let id = str_of(&self.d, "mundo");
        self.world = ws.iter().find(|w| !id.is_empty() && w["id"] == id).cloned().or(if ws.len() == 1 { ws.first().cloned() } else { None });
        self.book = Some(b);
    }

    fn stamp_tex(&mut self, ctx: &egui::Context, key: &str, ink: &str, fill: &str, px: f32) -> egui::TextureId {
        let size = if px > 120.0 { 256 } else { 128 };
        let k = (key.to_string(), ink.to_string(), fill.to_string(), size);
        if let Some(t) = self.stamps.get(&k) {
            return t.id();
        }
        let raw = ma::rasterize(key, ink, fill, size);
        let img = egui::ColorImage::from_rgba_premultiplied([size, size], &raw);
        let t = ctx.load_texture(format!("stamp:{key}:{ink}:{fill}:{size}"), img, egui::TextureOptions::LINEAR);
        let id = t.id();
        self.stamps.insert(k, t);
        id
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let root = cx.vault.root.clone();
        // the note: read it, and again when it changes on disk
        let m = self.file_mtime(&root);
        if !self.loaded || (!self.dirty && m != self.mtime && self.grab.is_none()) {
            self.d = ma::load(&root, &self.path);
            self.mtime = m;
            self.loaded = true;
        }
        if self.dirty && self.last_edit.is_some_and(|t| t.elapsed() > Duration::from_millis(700)) && self.grab.is_none() {
            if let Some(path) = self.save(&root) {
                acts.push(Action::Saved(path));
            }
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        self.load_world(cx);
        self.load_image(ui.ctx(), &root, cx);
        if let Some(rx) = &self.exporting {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
            if let Ok(r) = rx.try_recv() {
                self.exporting = None;
                match r {
                    Ok(p) => {
                        self.toast = Some(format!("Mapa exportado: {p}"));
                        acts.push(Action::Saved(p));
                    }
                    Err(e) => self.toast = Some(format!("Não foi possível exportar: {e}")),
                }
            }
        }

        let canvas = ui.available_rect_before_wrap();
        self.canvas = canvas;
        let (_, resp) = ui.allocate_exact_size(canvas.size(), Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 0.0, p.bg_secondary);
        let has_image = !str_of(&self.d, "imagem").is_empty();
        if self.view().is_none() && (self.tex.is_some() || !has_image) {
            let v = self.fit(canvas.size());
            self.set_view(v);
        }
        let mut view = self.view().unwrap_or_else(|| self.fit(canvas.size()));
        let (iw, ih) = (self.iw, self.ih);
        let sp = |q: [f64; 2], v: (f32, f32, f32)| canvas.min + vec2(v.0, v.1) + vec2((q[0] / 100.0 * iw) as f32, (q[1] / 100.0 * ih) as f32) * v.2;
        let wp = |s: Pos2, v: (f32, f32, f32)| {
            let w = (s - canvas.min - vec2(v.0, v.1)) / v.2;
            [w.x as f64 / iw * 100.0, w.y as f64 / ih * 100.0]
        };
        // the picture
        let base = Rect::from_min_size(sp([0.0, 0.0], view), vec2(iw as f32, ih as f32) * view.2);
        painter.rect_filled(base.translate(vec2(0.0, 3.0)), 2.0, Color32::from_black_alpha(if p.dark { 70 } else { 22 }));
        match &self.tex {
            Some((_, t)) => {
                painter.image(t.id(), base, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                painter.rect_filled(base, 2.0, super::color(ma::STAMP_FILL));
            }
        }
        // the layers
        let layers = self.layers();
        let ano = self.ano();
        let s = (iw / 100.0) as f32 * view.2;
        let mut hits: Vec<(String, Hit)> = Vec::new();
        for l in &layers {
            if !ma::visible(l, ano) {
                continue;
            }
            let id = str_of(l, "id").to_string();
            let sel = self.sel.as_deref() == Some(id.as_str());
            let col = l["cor"].as_str().filter(|c| !c.is_empty());
            match str_of(l, "t") {
                "area" => {
                    let pts: Vec<Pos2> = pts_of(l).iter().map(|q| sp(*q, view)).collect();
                    if pts.len() < 3 {
                        continue;
                    }
                    let c = super::color(col.unwrap_or(ma::PIN));
                    let opac = l["opac"].as_f64().unwrap_or(0.18) as f32;
                    fill_poly(&painter, &pts, c.gamma_multiply(opac));
                    let mut ring = pts.clone();
                    ring.push(pts[0]);
                    painter.extend(egui::Shape::dashed_line(&ring, Stroke::new((s * 0.18).max(1.0), c), s * 0.8, s * 0.5));
                    if sel {
                        painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(1.5_f32, p.accent_strong)));
                    }
                    hits.push((id, Hit::Poly(pts)));
                }
                "rota" => {
                    let pts: Vec<Pos2> = pts_of(l).iter().map(|q| sp(*q, view)).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    let c = super::color(col.unwrap_or(ma::PIN));
                    let w = (s * l["espessura"].as_f64().unwrap_or(0.3) as f32).max(1.2);
                    if sel {
                        painter.add(egui::Shape::line(pts.clone(), Stroke::new(w + 5.0, p.accent_strong.gamma_multiply(0.45))));
                    }
                    if l["tracejado"] == false {
                        painter.add(egui::Shape::line(pts.clone(), Stroke::new(w, c)));
                    } else {
                        painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(w, c), s * 0.9, s * 0.6));
                    }
                    hits.push((id, Hit::Line(pts, w.max(8.0))));
                }
                "carimbo" => {
                    let c = sp(xy(l), view);
                    let w = l["w"].as_f64().unwrap_or(4.0) as f32 * s;
                    let r = Rect::from_center_size(c, vec2(w, w));
                    if !r.expand(w).intersects(canvas) {
                        continue;
                    }
                    let key = str_of(l, "stamp").to_string();
                    let tex = if ma::stamp(&key).is_some() {
                        Some(self.stamp_tex(ui.ctx(), &key, col.unwrap_or(ma::STAMP_INK), l["fundo"].as_str().unwrap_or(ma::STAMP_FILL), w))
                    } else {
                        crate::host::image(ui.ctx(), cx.vault, cx.caches, &key, &self.path).map(|t| t.0)
                    };
                    if let Some(t) = tex {
                        image_rotated(&painter, t, c, w, l["rot"].as_f64().unwrap_or(0.0) as f32);
                    }
                    if sel {
                        painter.rect_stroke(r.expand(3.0), 4.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Outside);
                    }
                    hits.push((id, Hit::Rect(r)));
                }
                "rotulo" => {
                    let a = sp(xy(l), view);
                    let fs = (l["tam"].as_f64().unwrap_or(1.6) as f32 * s).max(2.0);
                    let fam = if l["ital"] == false { "cormorant" } else { "cormorant-italic" };
                    let r = map_text(&painter, a, str_of(l, "texto"), fs, fam, super::color(col.unwrap_or("#3b2a1a")), super::color(l["halo"].as_str().unwrap_or("#f3e6c8")), l["rot"].as_f64().unwrap_or(0.0) as f32, l["espaco"].as_f64().unwrap_or(0.12) as f32);
                    if sel {
                        painter.rect_stroke(r.expand(3.0), 4.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Outside);
                    }
                    hits.push((id, Hit::Rect(r)));
                }
                "pino" => {
                    let a = sp(xy(l), view);
                    // pins follow the map's scale, but stay big enough to see and click
                    let k = (l["tam"].as_f64().unwrap_or(1.4) as f32 * s / 20.0).max(0.8);
                    let c = super::color(col.unwrap_or(ma::PIN));
                    let drop: Vec<Pos2> = pin_shape().iter().map(|q| a + vec2(q.0, q.1) * k).collect();
                    painter.add(egui::Shape::convex_polygon(drop.clone(), c, Stroke::new((2.0 * k).max(1.0), Color32::WHITE)));
                    painter.circle_filled(a + vec2(0.0, -22.0) * k, 4.5 * k, Color32::WHITE);
                    let mut r = Rect::from_min_max(a + vec2(-12.0, -34.0) * k, a + vec2(12.0, 0.0) * k);
                    let label = str_of(l, "rotulo");
                    if !label.is_empty() {
                        let lr = map_text(&painter, a + vec2(0.0, 16.0) * k, label, 14.0 * k, "cormorant-bold", Color32::from_rgb(0x2a, 0x1d, 0x12), Color32::from_rgb(0xf3, 0xe6, 0xc8), 0.0, 0.0);
                        r = r.union(lr);
                    }
                    if !str_of(l, "submapa").is_empty() {
                        painter.text(a + vec2(9.0, -31.0) * k, egui::Align2::CENTER_CENTER, icons::ZOOM_IN, kit::icon_font((9.0 * k).max(6.0)), Color32::WHITE);
                    }
                    if sel {
                        painter.rect_stroke(r.expand(3.0), 4.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Outside);
                    }
                    hits.push((id, Hit::Rect(r)));
                }
                _ => {}
            }
        }
        // a path being drawn, the ruler, the ellipse being set
        let rp: Option<Vec<[f64; 2]>> = self.ruler.clone().or_else(|| match &self.tmp {
            Some((Tool::Regua, pts)) => Some(pts.iter().cloned().chain(self.hover).collect()),
            _ => None,
        });
        if let Some(rp) = rp.as_ref().filter(|r| !r.is_empty()) {
            let pts: Vec<Pos2> = rp.iter().map(|q| sp(*q, view)).collect();
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(4.0_f32, Color32::from_black_alpha(190))));
            painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(2.0_f32, Color32::WHITE), 6.0, 6.0));
            for q in &pts {
                painter.circle(*q, 4.0, Color32::WHITE, Stroke::new(1.5_f32, Color32::from_black_alpha(220)));
            }
        }
        if let Some((t, pts)) = &self.tmp {
            if *t != Tool::Regua && !pts.is_empty() {
                let v: Vec<Pos2> = pts.iter().cloned().chain(self.hover).map(|q| sp(q, view)).collect();
                let c = super::color(ma::PIN);
                if *t == Tool::Area && v.len() >= 3 {
                    fill_poly(&painter, &v, c.gamma_multiply(0.15));
                    let mut ring = v.clone();
                    ring.push(v[0]);
                    painter.extend(egui::Shape::dashed_line(&ring, Stroke::new(2.0_f32, c), 8.0, 5.0));
                } else {
                    painter.extend(egui::Shape::dashed_line(&v, Stroke::new((s * 0.3).max(2.0), c), 8.0, 6.0));
                }
                for q in v.iter().take(pts.len()) {
                    painter.circle_filled(*q, 3.0, c);
                }
            }
        }
        if let Some(e) = self.preview_ellipse {
            let (a, b) = (sp([e[0], e[1]], view), sp([e[2], e[3]], view));
            let r = Rect::from_two_pos(a, b);
            let ring: Vec<Pos2> = (0..=96).map(|i| {
                let t = i as f32 / 96.0 * std::f32::consts::TAU;
                pos2(r.center().x + r.width() / 2.0 * t.cos(), r.center().y + r.height() / 2.0 * t.sin())
            }).collect();
            painter.extend(egui::Shape::dashed_line(&ring, Stroke::new(2.0_f32, Color32::from_rgb(0xe1, 0x1d, 0x48)), 9.0, 6.0));
        }

        // input
        let pointer = ui.ctx().pointer_latest_pos();
        let hit = |pos: Pos2| -> Option<String> {
            hits.iter().rev().find(|(_, h)| h.contains(pos)).map(|(id, _)| id.clone())
        };
        let dialogs = self.any_dialog();
        let mut cmds: Vec<Cmd> = Vec::new();
        if !dialogs {
            if let Some(pos) = pointer.filter(|q| canvas.contains(*q)) {
                if self.tmp.is_some() {
                    let h = wp(pos, view);
                    if self.hover != Some(h) {
                        self.hover = Some(h);
                    }
                }
            }
            if resp.drag_started() {
                if let Some(pos) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()) {
                    let on = hit(pos);
                    self.grab = match on {
                        Some(id) if !self.locked && self.tool == Tool::Mover => {
                            self.sel = Some(id.clone());
                            let orig = layers.iter().find(|l| l["id"] == id.as_str()).cloned().unwrap_or(Json::Null);
                            Some(Grab::Layer { id, start: wp(pos, view), orig, moved: false })
                        }
                        _ => Some(Grab::Pan(vec2(view.0, view.1) - pos.to_vec2())),
                    };
                }
            }
            if resp.dragged() {
                if let Some(pos) = pointer {
                    match &mut self.grab {
                        Some(Grab::Pan(o)) => {
                            view.0 = o.x + pos.x;
                            view.1 = o.y + pos.y;
                        }
                        Some(Grab::Layer { id, start, orig, moved }) => {
                            let q = wp(pos, view);
                            let (dx, dy) = (q[0] - start[0], q[1] - start[1]);
                            *moved = true;
                            let mut nl = orig.clone();
                            if let Some(a) = orig["pts"].as_array() {
                                nl["pts"] = json!(a.iter().map(|p| json!([p[0].as_f64().unwrap_or(0.0) + dx, p[1].as_f64().unwrap_or(0.0) + dy])).collect::<Vec<_>>());
                            } else {
                                nl["x"] = json!(orig["x"].as_f64().unwrap_or(0.0) + dx);
                                nl["y"] = json!(orig["y"].as_f64().unwrap_or(0.0) + dy);
                            }
                            let id = id.clone();
                            if let Some(l) = self.layer_mut(&id) {
                                *l = nl;
                            }
                        }
                        _ => {}
                    }
                }
            }
            if resp.drag_stopped() {
                if let Some(Grab::Layer { moved: true, .. }) = &self.grab {
                    self.changed();
                }
                if let Some(Grab::Pan(_)) = &self.grab {
                    self.changed();
                }
                self.grab = None;
            }
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let q = wp(pos, view);
                    let on = hit(pos);
                    if self.locked || (self.tool == Tool::Mover && ui.input(|i| i.modifiers.command)) {
                        if let Some(l) = on.as_ref().and_then(|id| layers.iter().find(|l| l["id"] == id.as_str())) {
                            if str_of(l, "t") == "pino" {
                                if !str_of(l, "submapa").is_empty() {
                                    cmds.push(Cmd::Submap(str_of(l, "submapa").into()));
                                } else if !str_of(l, "link").is_empty() {
                                    acts.push(Action::OpenLink(link_text(str_of(l, "link")), self.path.clone(), true));
                                }
                            }
                        }
                    } else {
                        match self.tool {
                            Tool::Mover => self.sel = on,
                            Tool::Pino => cmds.push(Cmd::Add(json!({ "t": "pino", "x": q[0], "y": q[1], "rotulo": "", "cor": ma::PIN }), true)),
                            Tool::Rotulo => cmds.push(Cmd::Add(json!({ "t": "rotulo", "x": q[0], "y": q[1], "texto": "Nome do lugar", "tam": 1.6 }), true)),
                            Tool::Carimbo => cmds.push(Cmd::Add(json!({ "t": "carimbo", "x": q[0], "y": q[1], "w": 4, "stamp": self.stamp }), false)),
                            Tool::Rota | Tool::Area | Tool::Regua => {
                                if self.tool == Tool::Regua {
                                    self.ruler = None;
                                }
                                match &mut self.tmp {
                                    Some((_, pts)) => pts.push(q),
                                    None => self.tmp = Some((self.tool, vec![q])),
                                }
                            }
                        }
                    }
                }
            }
            if resp.double_clicked() || resp.triple_clicked() {
                if self.tmp.is_some() {
                    // the double click's first click added a point: drop the duplicate
                    if let Some((_, pts)) = &mut self.tmp {
                        while pts.len() >= 2 && pts[pts.len() - 1] == pts[pts.len() - 2] {
                            pts.pop();
                        }
                    }
                    cmds.push(Cmd::Finish);
                } else if let Some(id) = resp.interact_pointer_pos().and_then(hit) {
                    if !self.locked {
                        cmds.push(Cmd::Edit(id));
                    }
                }
            }
            // wheel zooms around the pointer; horizontal scroll pans
            if resp.hovered() {
                let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
                let f = if zoom != 1.0 { zoom } else { (scroll.y * 0.0025).exp() };
                if f != 1.0 {
                    if let Some(pos) = pointer {
                        let nz = (view.2 * f).clamp(0.02, 12.0);
                        let rel = pos - canvas.min;
                        view.0 = rel.x - (rel.x - view.0) * nz / view.2;
                        view.1 = rel.y - (rel.y - view.1) * nz / view.2;
                        view.2 = nz;
                        self.last_edit = Some(Instant::now());
                        self.dirty = true;
                    }
                }
                if scroll.x != 0.0 {
                    view.0 += scroll.x;
                }
            }
            let focused = ui.ctx().memory(|m| m.focused().is_some());
            if !focused {
                let (enter, esc, del) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete)));
                if enter && self.tmp.is_some() {
                    cmds.push(Cmd::Finish);
                }
                if esc {
                    self.tmp = None;
                    self.ruler = None;
                }
                if del && !self.locked {
                    if let Some(id) = self.sel.take() {
                        cmds.push(Cmd::Remove(id));
                    }
                }
            }
            // cursor
            if self.grab.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if let Some(pos) = pointer.filter(|q| canvas.contains(*q)) {
                if !self.locked && self.tool != Tool::Mover {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                } else if hit(pos).is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
            if resp.secondary_clicked() {
                self.menu = resp.interact_pointer_pos().and_then(hit);
                if self.menu.is_some() {
                    self.sel = self.menu.clone();
                }
            }
        }
        if !self.locked {
            let target = self.menu.clone();
            resp.context_menu(|ui| {
                let Some(id) = target.clone() else {
                    ui.label(RichText::new("Escolha uma ferramenta na barra para desenhar").color(p.faint));
                    return;
                };
                let Some(l) = layers.iter().find(|l| l["id"] == id.as_str()).cloned() else { return };
                let t = str_of(&l, "t");
                if kit::menu_item(ui, icons::PENCIL, "Editar…", p) {
                    cmds.push(Cmd::Edit(id.clone()));
                }
                if t == "pino" {
                    if !str_of(&l, "submapa").is_empty() && kit::menu_item(ui, icons::ZOOM_IN, "Entrar no submapa", p) {
                        cmds.push(Cmd::Submap(str_of(&l, "submapa").into()));
                    }
                    if !str_of(&l, "link").is_empty() && kit::menu_item(ui, icons::FILE_TEXT, "Abrir nota ligada", p) {
                        acts.push(Action::OpenLink(link_text(str_of(&l, "link")), self.path.clone(), true));
                    }
                    if kit::menu_item(ui, icons::MAP, if str_of(&l, "submapa").is_empty() { "Abre um mapa (submapa)…" } else { "Trocar submapa…" }, p) {
                        self.pick = Some((PickFor::Submap(id.clone()), String::new()));
                    }
                }
                if t == "rota" {
                    if let Some(km) = self.d["escala"].is_object().then(|| ma::path_km(&self.d["escala"], iw, ih, &pts_of(&l))).flatten() {
                        let (k, tr) = ma::travel(km, &ma::modes(self.world.as_ref()));
                        ui.label(RichText::new(format!("{k} · {}", tr.first().cloned().unwrap_or_default())).size(12.5).color(p.muted));
                    }
                }
                if t == "carimbo" || t == "rotulo" {
                    kit::menu_sep(ui, p);
                    if kit::menu_item(ui, icons::PLUS, "Maior", p) {
                        cmds.push(Cmd::Scale(id.clone(), 1.25));
                    }
                    if kit::menu_item(ui, icons::MINUS, "Menor", p) {
                        cmds.push(Cmd::Scale(id.clone(), 0.8));
                    }
                    if kit::menu_item(ui, icons::ROTATE_CW, "Girar 15°", p) {
                        cmds.push(Cmd::Rotate(id.clone()));
                    }
                }
                kit::menu_sep(ui, p);
                for c in COLORS {
                    let on = l["cor"] == c;
                    if kit::menu_check(ui, on, &color_name(c), p) {
                        cmds.push(Cmd::Set(id.clone(), "cor", json!(c)));
                    }
                }
                kit::menu_sep(ui, p);
                if kit::menu_item(ui, icons::COPY, "Duplicar", p) {
                    cmds.push(Cmd::Duplicate(id.clone()));
                }
                if kit::menu_danger(ui, icons::TRASH_2, "Excluir", p) {
                    cmds.push(Cmd::Remove(id.clone()));
                }
            });
        }
        let root2 = root.clone();
        for c in cmds {
            self.cmd(c, &root2, cx);
        }
        // keep the view
        if self.view() != Some(view) && (self.tex.is_some() || !has_image) {
            self.set_view(view);
        }

        // ── overlays ────────────────────────────────────────────────────
        self.crumbs(ui.ctx(), p, canvas, &root, &mut acts);
        self.toolbar(ui.ctx(), p, canvas, &root, cx, &mut acts);
        if self.tool == Tool::Carimbo && !self.locked {
            self.palette_ui(ui.ctx(), p, canvas, cx);
        }
        self.readout(ui.ctx(), p, canvas);
        if self.time_open {
            self.time_bar(ui.ctx(), p, canvas);
        }
        if !has_image {
            egui::Area::new(egui::Id::new(("map-empty", &self.path))).order(egui::Order::Middle).fixed_pos(canvas.center() - vec2(150.0, 40.0)).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).inner_margin(16.0).corner_radius(12.0).show(ui, |ui| {
                    ui.set_width(268.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Este mapa ainda não tem imagem.").color(p.muted));
                        ui.add_space(8.0);
                        if kit::primary_icon(ui, icons::IMAGE, "Escolher imagem do mapa", p).clicked() {
                            self.pick = Some((PickFor::Base, String::new()));
                        }
                    });
                });
            });
        } else if self.tex.is_none() && self.loading.is_some() {
            painter.text(canvas.center(), egui::Align2::CENTER_CENTER, "Carregando a imagem…", egui::FontId::proportional(13.0), p.faint);
        }
        self.dialogs(ui.ctx(), p, &root, cx);
        if let Some(t) = self.toast.take() {
            acts.push(Action::Toast(t));
        }
        acts
    }

    fn any_dialog(&self) -> bool {
        self.edit.is_some() || self.scale.is_some() || self.calibrate.is_some() || self.pick.is_some()
    }

    fn cmd(&mut self, c: Cmd, root: &Path, cx: &mut Cx) {
        match c {
            Cmd::Add(mut l, edit) => {
                let id = en::uid("m");
                l["id"] = json!(id);
                self.d["camadas"].as_array_mut().unwrap().push(l);
                self.sel = Some(id.clone());
                self.changed();
                if edit {
                    self.open_edit(&id);
                }
            }
            Cmd::Edit(id) => self.open_edit(&id),
            Cmd::Remove(id) => {
                self.d["camadas"].as_array_mut().unwrap().retain(|l| l["id"] != id.as_str());
                self.sel = None;
                self.changed();
            }
            Cmd::Duplicate(id) => {
                let Some(mut c) = self.layer_mut(&id).cloned() else { return };
                let nid = en::uid("m");
                c["id"] = json!(nid);
                if let Some(a) = c["pts"].as_array() {
                    c["pts"] = json!(a.iter().map(|p| json!([p[0].as_f64().unwrap_or(0.0) + 1.0, p[1].as_f64().unwrap_or(0.0) + 1.0])).collect::<Vec<_>>());
                } else {
                    c["x"] = json!(c["x"].as_f64().unwrap_or(0.0) + 1.0);
                    c["y"] = json!(c["y"].as_f64().unwrap_or(0.0) + 1.0);
                }
                self.d["camadas"].as_array_mut().unwrap().push(c);
                self.sel = Some(nid);
                self.changed();
            }
            Cmd::Scale(id, f) => {
                if let Some(l) = self.layer_mut(&id) {
                    if l["t"] == "carimbo" {
                        l["w"] = json!(l["w"].as_f64().unwrap_or(4.0) * f);
                    } else {
                        l["tam"] = json!(l["tam"].as_f64().unwrap_or(1.6) * f);
                    }
                }
                self.changed();
            }
            Cmd::Rotate(id) => {
                if let Some(l) = self.layer_mut(&id) {
                    l["rot"] = json!((l["rot"].as_f64().unwrap_or(0.0) + 15.0) % 360.0);
                }
                self.changed();
            }
            Cmd::Set(id, k, v) => {
                if let Some(l) = self.layer_mut(&id) {
                    l[k] = v;
                }
                self.changed();
            }
            Cmd::Finish => {
                let Some((t, pts)) = self.tmp.take() else { return };
                self.hover = None;
                if t == Tool::Regua {
                    self.ruler = (pts.len() >= 2).then_some(pts);
                    return;
                }
                if pts.len() >= 2 {
                    let kind = if t == Tool::Area { "area" } else { "rota" };
                    self.cmd(Cmd::Add(json!({ "t": kind, "pts": pts, "cor": ma::PIN, "rotulo": "" }), t == Tool::Area), root, cx);
                }
            }
            Cmd::Submap(link) => {
                let target = link_text(&link);
                match cx.vault.resolve(&target, &self.path).filter(|p| root.join(p).exists()) {
                    Some(p) => {
                        let mut trail = self.trail.clone();
                        trail.push((self.path.clone(), name_of(&self.path)));
                        self.go(root, &p, trail);
                    }
                    None => self.toast = Some("Submapa não encontrado.".into()),
                }
            }
        }
    }

    fn open_edit(&mut self, id: &str) {
        let Some(l) = self.layers().into_iter().find(|l| l["id"] == id) else { return };
        let cal = self.cal();
        let g = |k: &str, d: &str| l[k].as_str().map(String::from).or_else(|| l[k].as_f64().map(|v| fmt_f(v))).unwrap_or_else(|| d.to_string());
        let mut f: Vec<(&'static str, String, String, String)> = match str_of(&l, "t") {
            "pino" => vec![("rotulo", "Nome".into(), g("rotulo", ""), String::new()), ("link", "Nota ligada (nome ou [[link]])".into(), link_text(str_of(&l, "link")), "Ex.: Elevenlore".into()), ("cor", "Cor".into(), g("cor", ma::PIN), String::new())],
            "rotulo" => vec![("texto", "Texto".into(), g("texto", ""), String::new()), ("tam", "Tamanho (% da largura)".into(), g("tam", "1.6"), String::new()), ("rot", "Rotação (°)".into(), g("rot", "0"), String::new()), ("cor", "Cor".into(), g("cor", "#3b2a1a"), String::new())],
            "carimbo" => vec![("w", "Tamanho (% da largura)".into(), g("w", "4"), String::new()), ("rot", "Rotação (°)".into(), g("rot", "0"), String::new()), ("cor", "Cor da tinta".into(), g("cor", ma::STAMP_INK), String::new())],
            _ => vec![("rotulo", "Nome".into(), g("rotulo", ""), String::new()), ("cor", "Cor".into(), g("cor", ma::PIN), String::new())],
        };
        let antes = cal["eraAntes"].as_str().unwrap_or("AC").to_string();
        f.push(("de", format!("Existe desde (ano, ex.: 300 {antes})"), l["de"].as_f64().map(|y| mu::year(y, &cal)).unwrap_or_default(), "vazio = sempre".into()));
        f.push(("ate", "Existe até (ano)".into(), l["ate"].as_f64().map(|y| mu::year(y, &cal)).unwrap_or_default(), "vazio = até hoje".into()));
        self.edit = Some(LayerEdit { id: id.into(), fields: f, focus: true });
    }

    // ── overlays ────────────────────────────────────────────────────────

    fn crumbs(&mut self, ctx: &egui::Context, p: Palette, canvas: Rect, root: &Path, acts: &mut Vec<Action>) {
        egui::Area::new(egui::Id::new(("map-crumbs", &self.path))).order(egui::Order::Middle).fixed_pos(canvas.left_top() + vec2(16.0, 14.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(6, 4)).corner_radius(9.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if let Some(b) = &self.book {
                        if kit::quiet(ui, Some(icons::GLOBE), "Mundo", p).on_hover_text("Os mapas do mundo").clicked() {
                            acts.push(Action::View("mapas".into(), Some(b.path.clone())));
                        }
                    }
                    let trail = self.trail.clone();
                    let mut go = None;
                    for (i, (path, name)) in trail.iter().enumerate() {
                        ui.label(RichText::new("›").color(p.faint));
                        if kit::quiet(ui, None, name, p).clicked() {
                            go = Some((path.clone(), trail[..i].to_vec()));
                        }
                    }
                    if self.book.is_some() || !trail.is_empty() {
                        ui.label(RichText::new("›").color(p.faint));
                    }
                    ui.label(RichText::new(name_of(&self.path)).font(kit::medium(13.0)).color(p.text));
                    if let (Some(a), false) = (self.ano(), self.time_open) {
                        let y = mu::year(a, &self.cal());
                        if kit::chip(ui, Some(icons::HOURGLASS), &y, true, p).on_hover_text("O mapa mostra este ano; clique para mudar").clicked() {
                            self.time_open = true;
                        }
                    }
                    if self.d["legado"].as_bool().unwrap_or(false) {
                        ui.label(RichText::new("versão anterior").size(11.5).color(p.faint));
                    }
                    if let Some((path, t)) = go {
                        self.go(root, &path, t);
                    }
                });
            });
        });
    }

    fn toolbar(&mut self, ctx: &egui::Context, p: Palette, canvas: Rect, root: &Path, cx: &Cx, acts: &mut Vec<Action>) {
        let id = egui::Id::new(("map-bar", &self.path));
        let w = ctx.memory(|m| m.area_rect(id)).map(|r| r.width()).unwrap_or(470.0);
        let x = (canvas.center().x - w / 2.0).max(canvas.left() + 16.0);
        let y = if canvas.width() < 980.0 { canvas.top() + 56.0 } else { canvas.top() + 14.0 };
        egui::Area::new(id).order(egui::Order::Middle).fixed_pos(pos2(x, y)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(4, 3)).corner_radius(9.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if kit::icon_button(ui, icons::ZOOM_OUT, "Afastar", false, p).clicked() {
                        self.zoom(1.0 / 1.25, canvas);
                    }
                    if kit::icon_button(ui, icons::ZOOM_IN, "Aproximar", false, p).clicked() {
                        self.zoom(1.25, canvas);
                    }
                    if kit::icon_button(ui, icons::MAXIMIZE, "Enquadrar", false, p).clicked() {
                        let v = self.fit(canvas.size());
                        self.set_view(v);
                        self.changed();
                    }
                    sep(ui, p);
                    for (t, g, tip) in TOOLS {
                        let enabled = !self.locked || t == Tool::Mover;
                        let r = ui.add_enabled_ui(enabled, |ui| kit::icon_button(ui, g, tip, self.tool == t, p)).inner;
                        if r.clicked() {
                            self.tool = t;
                            self.tmp = None;
                            self.hover = None;
                            if t != Tool::Regua {
                                self.ruler = None;
                            }
                        }
                    }
                    sep(ui, p);
                    if kit::icon_button(ui, icons::HOURGLASS, "Linha do tempo: o mapa num ano", self.time_open, p).clicked() {
                        self.time_open = !self.time_open;
                    }
                    if kit::icon_button(ui, icons::SCALING, "Escala e projeção", false, p).clicked() {
                        self.open_scale();
                    }
                    if kit::icon_button(ui, if self.locked { icons::LOCK } else { icons::LOCK_OPEN }, "Travar (modo leitura: clicar num pino abre a nota)", self.locked, p).clicked() {
                        self.locked = !self.locked;
                        if self.locked {
                            self.tool = Tool::Mover;
                            self.tmp = None;
                        }
                    }
                    ui.menu_button(kit::icon(icons::ELLIPSIS, 15.0).color(p.muted), |ui| {
                        if kit::menu_item(ui, icons::IMAGE, "Trocar imagem base…", p) {
                            self.pick = Some((PickFor::Base, String::new()));
                            ui.close_menu();
                        }
                        if kit::menu_entry(ui, icons::IMAGE_DOWN, "Exportar PNG", "", self.exporting.is_none() && self.tex.is_some(), false, p) {
                            self.export(root, cx);
                            ui.close_menu();
                        }
                        if kit::menu_item(ui, icons::FILE_TEXT, "Abrir como Markdown", p) {
                            let _ = self.save(root);
                            acts.push(Action::AsMarkdown(self.path.clone()));
                            ui.close_menu();
                        }
                    });
                });
            });
        });
    }

    fn zoom(&mut self, f: f32, canvas: Rect) {
        let Some((x, y, z)) = self.view() else { return };
        let nz = (z * f).clamp(0.02, 12.0);
        let c = canvas.size() / 2.0;
        self.set_view((c.x - (c.x - x) * nz / z, c.y - (c.y - y) * nz / z, nz));
        self.changed();
    }

    fn palette_ui(&mut self, ctx: &egui::Context, p: Palette, canvas: Rect, cx: &mut Cx) {
        let top = if canvas.width() < 980.0 { 104.0 } else { 62.0 };
        egui::Area::new(egui::Id::new(("map-stamps", &self.path))).order(egui::Order::Middle).fixed_pos(canvas.left_top() + vec2(16.0, top)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(10.0).corner_radius(10.0).show(ui, |ui| {
                ui.set_width(196.0);
                ui.label(RichText::new("CARIMBOS").size(10.5).color(p.faint));
                ui.add_space(4.0);
                let keys: Vec<(String, String)> = ma::STAMPS.iter().map(|s| (s.0.to_string(), s.1.to_string())).collect();
                let cell = 44.0;
                egui::Grid::new("stamps").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                    for (i, (k, name)) in keys.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                        let on = self.stamp == *k;
                        if on || resp.hovered() {
                            ui.painter().rect_filled(r, 7.0, if on { p.bg_active } else { p.bg_hover });
                        }
                        if on {
                            ui.painter().rect_stroke(r, 7.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Inside);
                        }
                        let t = self.stamp_tex(ui.ctx(), k, ma::STAMP_INK, ma::STAMP_FILL, 40.0);
                        ui.painter().image(t, r.shrink(5.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        if resp.on_hover_text(name).clicked() {
                            self.stamp = k.clone();
                        }
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
                let mine: Vec<String> = cx.vault.index.read().unwrap().file_paths().filter(|f| (f.contains("/Carimbos/") || f.starts_with("Carimbos/")) && is_image(f)).cloned().collect();
                if !mine.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("SEUS CARIMBOS").size(10.5).color(p.faint));
                    ui.add_space(4.0);
                    egui::Grid::new("my-stamps").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                        for (i, f) in mine.iter().enumerate() {
                            let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                            let on = self.stamp == *f;
                            if on || resp.hovered() {
                                ui.painter().rect_filled(r, 7.0, if on { p.bg_active } else { p.bg_hover });
                            }
                            if let Some((t, sz)) = crate::host::image(ui.ctx(), cx.vault, cx.caches, f, "") {
                                let k = (r.width() - 10.0) / sz.x.max(sz.y);
                                ui.painter().image(t, Rect::from_center_size(r.center(), sz * k), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            if resp.on_hover_text(name_of(f)).clicked() {
                                self.stamp = f.clone();
                            }
                            if i % 4 == 3 {
                                ui.end_row();
                            }
                        }
                    });
                }
                ui.add_space(8.0);
                if kit::button(ui, Some(icons::PLUS), "Adicionar meu desenho", p).clicked() {
                    self.pick = Some((PickFor::Stamp, String::new()));
                }
                ui.label(RichText::new("Seus desenhos ficam na pasta Carimbos do livro.").size(11.5).color(p.faint));
            });
        });
    }

    fn measured(&self) -> Option<(String, Vec<[f64; 2]>, bool)> {
        if let Some(r) = self.ruler.as_ref().filter(|r| r.len() > 1) {
            return Some(("Régua".into(), r.clone(), true));
        }
        if let Some((Tool::Regua, pts)) = &self.tmp {
            let v: Vec<[f64; 2]> = pts.iter().cloned().chain(self.hover).collect();
            if v.len() > 1 {
                return Some(("Régua".into(), v, false));
            }
        }
        let l = self.sel.as_ref().and_then(|id| self.layers().into_iter().find(|l| l["id"] == id.as_str()))?;
        if l["t"] != "rota" {
            return None;
        }
        let pts = pts_of(&l);
        (pts.len() > 1).then(|| (if str_of(&l, "rotulo").is_empty() { "Rota".to_string() } else { str_of(&l, "rotulo").to_string() }, pts, false))
    }

    fn readout(&mut self, ctx: &egui::Context, p: Palette, canvas: Rect) {
        let Some((label, pts, is_ruler)) = self.measured() else { return };
        let bottom = if self.time_open { 96.0 } else { 16.0 };
        egui::Area::new(egui::Id::new(("map-readout", &self.path))).order(egui::Order::Middle).pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(canvas.left_bottom() + vec2(16.0, -bottom)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(12.0).corner_radius(10.0).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.label(RichText::new(label.to_uppercase()).size(10.5).color(p.faint));
                let esc = self.d["escala"].clone();
                if !esc.is_object() {
                    ui.label(RichText::new("Sem escala").font(kit::medium(16.0)).color(p.text));
                    if kit::quiet(ui, Some(icons::SCALING), "Definir escala…", p).clicked() {
                        self.open_scale();
                    }
                    return;
                }
                match ma::path_km(&esc, self.iw, self.ih, &pts) {
                    None => {
                        ui.label(RichText::new("Fora do mundo").font(kit::medium(16.0)).color(p.text));
                    }
                    Some(km) => {
                        let (k, t) = ma::travel(km, &ma::modes(self.world.as_ref()));
                        ui.label(RichText::new(k).font(kit::medium(18.0)).color(p.text));
                        ui.label(RichText::new(t.join(" · ")).size(12.0).color(p.muted));
                    }
                }
                if is_ruler {
                    ui.add_space(4.0);
                    if esc["proj"] != "mollweide" && kit::quiet(ui, Some(icons::RULER), "Usar esta medida como escala…", p).clicked() {
                        self.calibrate = Some(String::new());
                    }
                    if kit::quiet(ui, Some(icons::ROUTE), "Guardar como rota", p).clicked() {
                        let pts = self.ruler.take().unwrap_or_default();
                        let id = en::uid("m");
                        self.d["camadas"].as_array_mut().unwrap().push(json!({ "id": id, "t": "rota", "pts": pts, "cor": ma::PIN, "rotulo": "" }));
                        self.sel = Some(id.clone());
                        self.changed();
                        self.open_edit(&id);
                    }
                }
            });
        });
    }

    fn time_bar(&mut self, ctx: &egui::Context, p: Palette, canvas: Rect) {
        let eras: Vec<Json> = self.world.as_ref().and_then(|w| w["eras"].as_array().cloned()).unwrap_or_default();
        let layers = self.layers();
        let cal = self.cal();
        let w = (canvas.width() - 64.0).min(720.0);
        egui::Area::new(egui::Id::new(("map-time", &self.path))).order(egui::Order::Middle).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(canvas.center_bottom() - vec2(0.0, 16.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(12.0).corner_radius(10.0).show(ui, |ui| {
                ui.set_width(w);
                let Some(ts) = TimeScale::new(&layers, &eras) else {
                    ui.label(RichText::new("Dê \"existe desde / até\" aos elementos (duplo clique) ou eras ao mundo para ver o mapa mudar com o tempo.").size(12.5).color(p.muted));
                    return;
                };
                ui.horizontal(|ui| {
                    let ano = self.ano();
                    ui.label(RichText::new(ano.map(|a| mu::year(a, &cal)).unwrap_or_else(|| "Todas as épocas".into())).font(kit::medium(14.0)).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::chip(ui, None, "Tudo", ano.is_none(), p).clicked() {
                            self.d["ano"] = Json::Null;
                            self.changed();
                        }
                        if !layers.iter().any(|l| l["de"].is_number() || l["ate"].is_number()) {
                            ui.label(RichText::new("Nenhum elemento tem datas ainda").size(11.5).color(p.faint));
                        }
                    });
                });
                ui.add_space(6.0);
                let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click_and_drag());
                let painter = ui.painter_at(r.expand(8.0));
                for (seg, (a, b)) in ts.segs.iter().zip(ts.spans()) {
                    let sr = Rect::from_min_max(pos2(r.left() + a as f32 * r.width(), r.top()), pos2(r.left() + b as f32 * r.width(), r.bottom())).shrink2(vec2(1.0, 0.0));
                    let c = if seg.cor.is_empty() { p.accent } else { super::color(&seg.cor) };
                    painter.rect_filled(sr, 5.0, c.gamma_multiply(if p.dark { 0.35 } else { 0.22 }));
                    if !seg.nome.is_empty() {
                        let g = painter.layout_no_wrap(seg.nome.clone(), egui::FontId::proportional(11.0), p.text);
                        if g.size().x < sr.width() - 8.0 {
                            painter.galley(sr.center() - g.size() / 2.0, g, p.text);
                        }
                    }
                    if resp.hovered() && ui.rect_contains_pointer(sr) {
                        resp.clone().on_hover_text(format!("{} · {} – {}", seg.nome, mu::year(seg.de, &cal), mu::year(seg.ate, &cal)));
                    }
                }
                if let Some(a) = self.ano() {
                    let x = r.left() + ts.to_pos(a) as f32 * r.width();
                    painter.line_segment([pos2(x, r.top() - 4.0), pos2(x, r.bottom() + 4.0)], Stroke::new(2.0_f32, p.accent_strong));
                    painter.circle(pos2(x, r.top() - 4.0), 5.0, p.bg, Stroke::new(2.0_f32, p.accent_strong));
                }
                if resp.dragged() || resp.clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let f = ((pos.x - r.left()) / r.width()).clamp(0.0, 1.0) as f64;
                        self.d["ano"] = json!(ts.from_pos(f));
                        self.grab = Some(Grab::Time);
                    }
                }
                if resp.drag_stopped() || resp.clicked() {
                    self.grab = None;
                    self.changed();
                }
            });
        });
    }

    fn open_scale(&mut self) {
        let e = &self.d["escala"];
        let el: Vec<f64> = e["elipse"].as_array().map(|a| a.iter().filter_map(|v| v.as_f64()).collect()).filter(|v: &Vec<f64>| v.len() == 4).unwrap_or(vec![0.0, 0.0, 100.0, 100.0]);
        self.scale = Some(ScaleEdit {
            mollweide: e["proj"] == "mollweide",
            km: e["kmLargura"].as_f64().map(fmt_f).unwrap_or_default(),
            raio: fmt_f(e["raio"].as_f64().unwrap_or(6371.0)),
            elipse: [fmt_f(el[0]), fmt_f(el[1]), fmt_f(el[2]), fmt_f(el[3])],
        });
    }

    // ── dialogs ─────────────────────────────────────────────────────────

    fn dialogs(&mut self, ctx: &egui::Context, p: Palette, root: &Path, cx: &mut Cx) {
        if self.edit.is_some() {
            self.edit_dialog(ctx, p);
        } else if self.scale.is_some() {
            self.scale_dialog(ctx, p, root, cx);
        } else if self.calibrate.is_some() {
            self.calibrate_dialog(ctx, p);
        } else if self.pick.is_some() {
            self.pick_dialog(ctx, p, root, cx);
        }
    }

    fn edit_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(e) = self.edit.as_mut() else { return };
        let mut done = None;
        kit::scrim(ctx, p);
        egui::Window::new("map-edit").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 100.0]).show(ctx, |ui| {
            ui.set_width(380.0);
            if kit::cmd_enter(ui.ctx()) {
                done = Some(true);
            }
            ui.label(RichText::new("Editar").font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            for (i, (k, label, v, hint)) in e.fields.iter_mut().enumerate() {
                ui.label(RichText::new(label.as_str()).size(12.0).color(p.muted));
                if *k == "cor" {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for c in COLORS {
                            let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                            ui.painter().circle_filled(r.center(), 9.0, super::color(c));
                            if v.eq_ignore_ascii_case(c) {
                                ui.painter().circle_stroke(r.center(), 11.0, Stroke::new(1.5_f32, p.accent_strong));
                            }
                            if resp.on_hover_text(color_name(c)).clicked() {
                                *v = c.into();
                            }
                        }
                        ui.add(egui::TextEdit::singleline(v).desired_width(80.0).margin(egui::Margin::symmetric(6, 4)));
                    });
                } else {
                    let r = ui.add(egui::TextEdit::singleline(v).desired_width(f32::INFINITY).hint_text(RichText::new(hint.as_str()).color(p.faint)).margin(egui::Margin::symmetric(8, 6)));
                    if i == 0 && e.focus {
                        r.request_focus();
                        if r.has_focus() {
                            e.focus = false;
                        }
                    }
                }
                ui.add_space(6.0);
            }
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary(ui, "Salvar", p).clicked() {
                    done = Some(true);
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                let e = self.edit.take().unwrap();
                let cal = self.cal();
                let (antes, depois) = (cal["eraAntes"].as_str().unwrap_or("").to_string(), cal["eraDepois"].as_str().unwrap_or("").to_string());
                if let Some(l) = self.layer_mut(&e.id) {
                    for (k, _, v, _) in e.fields {
                        let v = v.trim().to_string();
                        match k {
                            "de" | "ate" => match mu::parse_range(&v, &antes, &depois).filter(|_| !v.is_empty()) {
                                Some(r) => l[k] = json!(r.0),
                                None => {
                                    l.as_object_mut().map(|o| o.remove(k));
                                }
                            },
                            "tam" | "rot" | "w" => l[k] = json!(v.replace(',', ".").parse::<f64>().unwrap_or(0.0)),
                            "link" => l[k] = json!(if v.is_empty() { String::new() } else { format!("[[{}]]", link_text(&v)) }),
                            _ => l[k] = json!(v),
                        }
                    }
                }
                self.changed();
            }
            Some(false) => self.edit = None,
            None => {}
        }
    }

    fn scale_dialog(&mut self, ctx: &egui::Context, p: Palette, root: &Path, cx: &Cx) {
        let Some(s) = self.scale.as_mut() else { return };
        let mut done = None;
        let mut detect = false;
        egui::Window::new("map-scale").title_bar(false).collapsible(false).resizable(false).pivot(egui::Align2::RIGHT_TOP).fixed_pos(self.canvas.right_top() + vec2(-16.0, 14.0)).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Escala e projeção").font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            if let Some(i) = kit::segmented(ui, &[("", "Plana (região, cidade)"), ("", "Mundo inteiro (Mollweide)")], if s.mollweide { 1 } else { 0 }, p) {
                s.mollweide = i == 1;
            }
            ui.add_space(8.0);
            if s.mollweide {
                ui.label(RichText::new("O mundo inteiro numa elipse, como um mapa-múndi. As distâncias passam a ser medidas sobre a esfera.").size(12.0).color(p.muted));
                ui.add_space(6.0);
                ui.label(RichText::new("Raio do planeta (km)").size(12.0).color(p.muted));
                ui.add(egui::TextEdit::singleline(&mut s.raio).desired_width(120.0).margin(egui::Margin::symmetric(8, 5)));
                ui.add_space(6.0);
                ui.label(RichText::new("Elipse na imagem (% esquerda, topo, direita, base)").size(12.0).color(p.muted));
                ui.horizontal(|ui| {
                    for v in s.elipse.iter_mut() {
                        ui.add(egui::TextEdit::singleline(v).desired_width(52.0).margin(egui::Margin::symmetric(6, 5)));
                    }
                    if kit::button(ui, None, "Detectar", p).clicked() {
                        detect = true;
                    }
                });
                ui.label(RichText::new("A linha tracejada vermelha mostra a elipse no mapa.").size(11.5).color(p.faint));
            } else {
                ui.label(RichText::new("Largura do mapa (km)").size(12.0).color(p.muted));
                ui.add(egui::TextEdit::singleline(&mut s.km).desired_width(140.0).hint_text(RichText::new("Ex.: 1200").color(p.faint)).margin(egui::Margin::symmetric(8, 5)));
                ui.label(RichText::new("Ou meça um trecho conhecido com a régua e use \"Usar esta medida como escala\".").size(11.5).color(p.faint));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if kit::quiet(ui, Some(icons::TRASH_2), "Tirar a escala", p).clicked() {
                    done = Some(2);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::primary(ui, "Salvar", p).clicked() {
                        done = Some(1);
                    }
                    if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = Some(0);
                    }
                });
            });
        });
        let num = |t: &str| t.trim().replace(',', ".").parse::<f64>().ok();
        if detect {
            let img = str_of(&self.d, "imagem").to_string();
            if let Some(full) = cx.vault.resolve(&img, &self.path) {
                match image::open(root.join(full)) {
                    Ok(im) => {
                        let e = ma::detect_ellipse(&im.to_rgba8());
                        s.elipse = [fmt_f(e[0]), fmt_f(e[1]), fmt_f(e[2]), fmt_f(e[3])];
                    }
                    Err(e) => self.toast = Some(format!("Não consegui detectar: {e}")),
                }
            }
        }
        self.preview_ellipse = if s.mollweide {
            let e: Vec<f64> = s.elipse.iter().map(|v| num(v).unwrap_or(0.0)).collect();
            Some([e[0], e[1], e[2], e[3]])
        } else {
            None
        };
        match done {
            Some(1) => {
                let s = self.scale.take().unwrap();
                self.d["escala"] = if s.mollweide {
                    json!({ "proj": "mollweide", "raio": num(&s.raio).filter(|r| *r > 0.0).unwrap_or(6371.0), "elipse": s.elipse.iter().map(|v| num(v).unwrap_or(0.0)).collect::<Vec<_>>() })
                } else {
                    match num(&s.km).filter(|k| *k > 0.0) {
                        Some(k) => json!({ "proj": "plana", "kmLargura": k }),
                        None => Json::Null,
                    }
                };
                self.preview_ellipse = None;
                self.changed();
            }
            Some(2) => {
                self.scale = None;
                self.d["escala"] = Json::Null;
                self.preview_ellipse = None;
                self.changed();
            }
            Some(_) => {
                self.scale = None;
                self.preview_ellipse = None;
            }
            None => {}
        }
    }

    fn calibrate_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(v) = self.calibrate.as_mut() else { return };
        let mut done = None;
        kit::scrim(ctx, p);
        egui::Window::new("map-cal").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 120.0]).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new("Escala pela régua").font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            ui.label(RichText::new("Quantos km tem o trecho medido?").size(12.0).color(p.muted));
            ui.add(egui::TextEdit::singleline(v).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6))).request_focus();
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary(ui, "Definir", p).clicked() {
                    done = Some(true);
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        if let Some(ok) = done {
            let v = self.calibrate.take().unwrap();
            if ok {
                let km = v.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
                let pts = self.ruler.clone().unwrap_or_default();
                // the ruler's length in picture pixels
                let px = ma::path_km(&json!({ "proj": "plana", "kmLargura": self.iw }), self.iw, self.ih, &pts).unwrap_or(0.0);
                if km > 0.0 && px > 0.0 {
                    self.d["escala"] = json!({ "proj": "plana", "kmLargura": km / px * self.iw });
                    self.changed();
                }
            }
        }
    }

    fn pick_dialog(&mut self, ctx: &egui::Context, p: Palette, root: &Path, cx: &mut Cx) {
        let Some((what, filter)) = self.pick.as_mut() else { return };
        let what = what.clone();
        let ix = cx.vault.index.read().unwrap();
        let mut items: Vec<String> = match &what {
            PickFor::Submap(_) => ix.file_paths().filter(|f| *f != &self.path && ix.page(f).is_some_and(|pg| book::kind(pg) == "mapa")).cloned().collect(),
            _ => ix.file_paths().filter(|f| is_image(f)).cloned().collect(),
        };
        drop(ix);
        items.sort();
        let f = mu::deaccent(filter);
        items.retain(|x| f.is_empty() || mu::deaccent(x).contains(&f));
        let mut chosen = None;
        let mut close = false;
        kit::scrim(ctx, p);
        egui::Window::new("map-pick").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(match what { PickFor::Base => "Imagem base do mapa", PickFor::Submap(_) => "Mapa que este pino abre", PickFor::Stamp => "Desenho para virar carimbo" }).font(kit::medium(16.0)).color(p.text));
            if what == PickFor::Stamp {
                ui.label(RichText::new("Um PNG transparente fica melhor.").size(12.0).color(p.muted));
            }
            ui.add_space(6.0);
            ui.add(egui::TextEdit::singleline(filter).hint_text(RichText::new("Filtrar").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6))).request_focus();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                if items.is_empty() {
                    ui.label(RichText::new(if matches!(what, PickFor::Submap(_)) { "Crie primeiro o mapa da região (Mundo › Mapas › Novo mapa)." } else { "Nenhuma imagem no cofre." }).color(p.faint));
                }
                for it in items.iter().take(400) {
                    if kit::Row::new(it).icon(if matches!(what, PickFor::Submap(_)) { icons::MAP } else { icons::IMAGE }).show(ui, p).clicked() {
                        chosen = Some(it.clone());
                    }
                }
            });
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if let Some(c) = chosen {
            self.pick = None;
            match what {
                PickFor::Base => {
                    self.d["imagem"] = json!(c);
                    self.d["view"] = Json::Null;
                    self.tex = None;
                    self.changed();
                }
                PickFor::Submap(id) => {
                    let name = name_of(&c);
                    if let Some(l) = self.layer_mut(&id) {
                        l["submapa"] = json!(format!("[[{name}]]"));
                        if str_of(l, "rotulo").is_empty() {
                            l["rotulo"] = json!(name);
                        }
                    }
                    self.changed();
                }
                PickFor::Stamp => {
                    let folder = self.book.as_ref().map(|b| format!("{}/Carimbos", b.folder)).unwrap_or_else(|| "Carimbos".into());
                    let dest = book::unique(root, &format!("{folder}/{}", c.rsplit('/').next().unwrap_or(&c)));
                    let ok = std::fs::create_dir_all(root.join(&folder)).and_then(|_| std::fs::copy(root.join(&c), root.join(&dest))).is_ok();
                    if ok {
                        cx.vault.index.write().unwrap().update(std::slice::from_ref(&dest));
                        self.stamp = dest;
                    } else {
                        self.toast = Some("Não foi possível copiar o desenho.".into());
                    }
                }
            }
        } else if close {
            self.pick = None;
        }
    }

    fn export(&mut self, root: &Path, cx: &Cx) {
        let _ = self.save(root);
        let img = str_of(&self.d, "imagem").to_string();
        let base = cx.vault.resolve(&img, &self.path).filter(|p| root.join(p).exists());
        let (root, note, d, iw, ih) = (root.to_path_buf(), self.path.clone(), self.d.clone(), self.iw, self.ih);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(super::mapa_export::export(&root, &note, base.as_deref(), &d, iw, ih));
        });
        self.exporting = Some(rx);
        self.toast = Some("Exportando o mapa…".into());
    }
}

enum Cmd {
    Add(Json, bool),
    Edit(String),
    Remove(String),
    Duplicate(String),
    Scale(String, f64),
    Rotate(String),
    Set(String, &'static str, Json),
    Finish,
    Submap(String),
}

enum Hit {
    Rect(Rect),
    Poly(Vec<Pos2>),
    Line(Vec<Pos2>, f32),
}

impl Hit {
    fn contains(&self, q: Pos2) -> bool {
        match self {
            Hit::Rect(r) => r.contains(q),
            Hit::Poly(pts) => {
                let mut inside = false;
                let n = pts.len();
                for i in 0..n {
                    let (a, b) = (pts[i], pts[(i + n - 1) % n]);
                    if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
                        inside = !inside;
                    }
                }
                inside
            }
            Hit::Line(pts, w) => pts.windows(2).any(|s| dist_seg(q, s[0], s[1]) < *w / 2.0 + 3.0),
        }
    }
}

fn dist_seg(q: Pos2, a: Pos2, b: Pos2) -> f32 {
    let d = b - a;
    let t = if d.length_sq() > 0.0 { ((q - a).dot(d) / d.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + d * t).distance(q)
}

fn sep(ui: &mut egui::Ui, p: Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 20.0), Sense::hover());
    ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0_f32, p.border));
}

pub fn name_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string()
}

fn link_text(l: &str) -> String {
    l.trim().trim_start_matches("[[").trim_end_matches("]]").split('|').next().unwrap_or("").to_string()
}

pub fn is_image(f: &str) -> bool {
    let l = f.to_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|e| l.ends_with(e))
}

fn fmt_f(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn color_name(c: &str) -> String {
    match c {
        "#8b1e1e" => "Vermelho",
        "#3b2a1a" => "Sépia",
        "#2f5d7c" => "Azul",
        "#3f6b3a" => "Verde",
        "#7a4f9a" => "Violeta",
        "#b07a1f" => "Ouro",
        _ => "Preto",
    }
    .into()
}

fn xy(l: &Json) -> [f64; 2] {
    [l["x"].as_f64().unwrap_or(0.0), l["y"].as_f64().unwrap_or(0.0)]
}

fn pts_of(l: &Json) -> Vec<[f64; 2]> {
    l["pts"].as_array().map(|a| a.iter().map(|p| [p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)]).collect()).unwrap_or_default()
}

/// The pin's drop (the plugin's SVG path, 24 units wide, tip at 0,0).
fn pin_shape() -> &'static [(f32, f32)] {
    static PIN: once_cell::sync::Lazy<Vec<(f32, f32)>> = once_cell::sync::Lazy::new(|| {
        let mut v: Vec<(f32, f32)> = ma::path_points("M0 0C-6-10-12-14-12-22A12 12 0 1 1 12-22C12-14 6-10 0 0Z").into_iter().flatten().map(|(x, y)| (x as f32, y as f32)).collect();
        v.dedup();
        if v.len() > 1 && v.first() == v.last() {
            v.pop();
        }
        v
    });
    &PIN
}

/// A filled (possibly concave) polygon.
fn fill_poly(painter: &egui::Painter, pts: &[Pos2], c: Color32) {
    let tri = ma::triangulate(&pts.iter().map(|q| (q.x as f64, q.y as f64)).collect::<Vec<_>>());
    let mut mesh = egui::Mesh::default();
    for q in pts {
        mesh.colored_vertex(*q, c);
    }
    for [a, b, cc] in tri {
        mesh.add_triangle(a as u32, b as u32, cc as u32);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// A square picture centred on `c`, `w` wide, turned `deg` degrees.
fn image_rotated(painter: &egui::Painter, tex: egui::TextureId, c: Pos2, w: f32, deg: f32) {
    let (s, co) = deg.to_radians().sin_cos();
    let h = w / 2.0;
    let rot = |x: f32, y: f32| c + vec2(x * co - y * s, x * s + y * co);
    let mut mesh = egui::Mesh::with_texture(tex);
    let corners = [(-h, -h, 0.0, 0.0), (h, -h, 1.0, 0.0), (h, h, 1.0, 1.0), (-h, h, 0.0, 1.0)];
    for (x, y, u, v) in corners {
        mesh.vertices.push(egui::epaint::Vertex { pos: rot(x, y), uv: pos2(u, v), color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Text on the map: centred on `a` (its baseline), with a halo, turned
/// `deg` degrees; returns its (unturned) box.
#[allow(clippy::too_many_arguments)]
fn map_text(painter: &egui::Painter, a: Pos2, text: &str, size: f32, family: &str, color: Color32, halo: Color32, deg: f32, spacing: f32) -> Rect {
    let font = egui::FontId::new(size, egui::FontFamily::Name(family.into()));
    let mk = |c: Color32| {
        let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font.clone(), color: c, extra_letter_spacing: size * spacing, ..Default::default() });
        job.wrap.max_width = f32::INFINITY;
        painter.ctx().fonts(|f| f.layout_job(job))
    };
    let g = mk(color);
    let sz = g.size();
    let asc = size * 0.78;
    let ang = deg.to_radians();
    let (s, c) = ang.sin_cos();
    let off = vec2(-sz.x / 2.0, -asc);
    let pos = a + vec2(off.x * c - off.y * s, off.x * s + off.y * c);
    if size > 3.0 {
        let hw = (size * 0.09).max(1.0);
        let gh = mk(halo);
        for k in 0..8 {
            let t = k as f32 / 8.0 * std::f32::consts::TAU;
            painter.add(egui::epaint::TextShape::new(pos + vec2(t.cos(), t.sin()) * hw, gh.clone(), halo).with_angle(ang));
        }
    }
    painter.add(egui::epaint::TextShape::new(pos, g, color).with_angle(ang));
    Rect::from_min_size(a + off, sz)
}
