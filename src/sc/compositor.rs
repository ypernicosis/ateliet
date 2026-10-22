//! Compositor: the visual editor of composed pages (comics, manga,
//! free pages, text over art) and of the open cover. The model is drawn in
//! the background by the same code as the PDF; the selection, its handles
//! and the guides are drawn over it.

use super::library::Cx;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::scriptorium::book::PAPERS;
use ateliet_core::scriptorium::capa::{self, CoverPics};
use ateliet_core::typeset::compose::{self, Env};
use ateliet_core::typeset::raster;
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub enum Event {
    Save(Json),
    Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Move,
    Pan,
    Tail,
    Pt(usize),
    Rot,
    Resize(&'static str),
}

struct Drag {
    id: String,
    mode: Mode,
    start: (f32, f32),
    orig: Json,
    moved: bool,
}

struct Rendered {
    gen: u64,
    img: egui::ColorImage,
    ppm: f32,
    pics: Arc<CoverPics>,
    keys: Vec<(String, String)>,
}

pub struct Compositor {
    pub model: Json,
    pub capa: bool,
    pub cfg: Json,
    pub pages: usize,
    /// The note the pictures are resolved from.
    from: String,
    pub title: String,
    sel: Option<String>,
    history: Vec<String>,
    future: Vec<String>,
    last_commit: Instant,
    saved: String,
    zoom: Option<f32>,
    guides: bool,
    snap: bool,
    drag: Option<Drag>,
    gen: u64,
    shown: String,
    job: Option<Receiver<Rendered>>,
    tex: Option<(egui::TextureHandle, u64)>,
    pics: Option<(Vec<(String, String)>, Arc<CoverPics>)>,
    pick: Option<(String, String)>,
    confirm: Option<String>,
    sinopse: String,
}

fn f(v: &Json, d: f32) -> f32 {
    v.as_f64().map(|x| x as f32).unwrap_or(d)
}

fn s<'a>(v: &'a Json, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn snap(on: bool, v: f32) -> f32 {
    if on {
        (v * 2.0).round() / 2.0
    } else {
        (v * 100.0).round() / 100.0
    }
}

fn rot_pt(p: (f32, f32), c: (f32, f32), deg: f32) -> (f32, f32) {
    let (sn, cs) = deg.to_radians().sin_cos();
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    (c.0 + dx * cs - dy * sn, c.1 + dx * sn + dy * cs)
}

fn in_poly(p: (f32, f32), poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

impl Compositor {
    pub fn new(model: Json, capa: bool, cfg: Json, pages: usize, from: &str, title: &str) -> Compositor {
        let mut model = model;
        if !model["camadas"].is_array() {
            model["camadas"] = json!([]);
        }
        let snap0 = model.to_string();
        Compositor {
            model,
            capa,
            cfg,
            pages,
            from: from.into(),
            title: title.into(),
            sel: None,
            history: vec![snap0.clone()],
            future: Vec::new(),
            last_commit: Instant::now(),
            saved: snap0,
            zoom: None,
            guides: true,
            snap: true,
            drag: None,
            gen: 0,
            shown: String::new(),
            job: None,
            tex: None,
            pics: None,
            pick: None,
            confirm: None,
            sinopse: String::new(),
        }
    }

    pub fn with_back_text(mut self, t: &str) -> Self {
        self.sinopse = t.to_string();
        self
    }

    pub fn dirty(&self) -> bool {
        self.model.to_string() != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.model.to_string();
    }

    pub fn env(&self) -> Env {
        if self.capa {
            compose::cover_env(&self.model, &self.cfg, self.pages)
        } else {
            compose::page_env(&self.model, &self.cfg, true)
        }
    }

    fn layers(&self) -> Vec<Json> {
        self.model["camadas"].as_array().cloned().unwrap_or_default()
    }

    fn layer_mut(&mut self, id: &str) -> Option<&mut Json> {
        self.model["camadas"].as_array_mut()?.iter_mut().find(|l| s(l, "id") == id)
    }

    fn layer(&self, id: &str) -> Option<&Json> {
        self.model["camadas"].as_array()?.iter().find(|l| s(l, "id") == id)
    }

    // ── history ─────────────────────────────────────────────────────

    /// Record the model when it changed (changes close together are one step).
    fn commit(&mut self) {
        let now = self.model.to_string();
        if self.history.last() == Some(&now) {
            return;
        }
        if self.last_commit.elapsed() < Duration::from_millis(450) && self.history.len() > 1 && self.drag.is_none() {
            *self.history.last_mut().unwrap() = now;
        } else {
            self.history.push(now);
            if self.history.len() > 120 {
                self.history.remove(0);
            }
        }
        self.future.clear();
        self.last_commit = Instant::now();
    }

    fn step(&mut self) {
        // the next change starts a step of its own
        self.last_commit = Instant::now() - Duration::from_secs(5);
    }

    fn undo(&mut self) {
        self.commit();
        if self.history.len() < 2 {
            return;
        }
        self.future.push(self.history.pop().unwrap());
        self.model = serde_json::from_str(self.history.last().unwrap()).unwrap_or(json!({}));
        if self.sel.as_ref().is_some_and(|id| self.layer(id).is_none()) {
            self.sel = None;
        }
        self.step();
    }

    fn redo(&mut self) {
        let Some(sn) = self.future.pop() else { return };
        self.model = serde_json::from_str(&sn).unwrap_or(json!({}));
        self.history.push(sn);
        self.step();
    }

    // ── layers ──────────────────────────────────────────────────────

    fn add(&mut self, mut l: Json) {
        if self.capa && s(&l, "zona").is_empty() {
            l["zona"] = json!("capa");
        }
        if s(&l, "id").is_empty() {
            let t = s(&l, "t").chars().next().unwrap_or('l').to_string();
            l["id"] = json!(compose::uid(&t));
        }
        let id = s(&l, "id").to_string();
        let pick = matches!(s(&l, "t"), "imagem" | "painel");
        if let Some(a) = self.model["camadas"].as_array_mut() {
            a.push(l);
        }
        self.step();
        self.sel = Some(id.clone());
        if pick {
            self.pick = Some((id, String::new()));
        }
    }

    fn duplicate(&mut self) {
        let Some(id) = self.sel.clone() else { return };
        let Some(mut c) = self.layer(&id).cloned() else { return };
        let t = s(&c, "t").chars().next().unwrap_or('l').to_string();
        let nid = compose::uid(&t);
        c["id"] = json!(nid);
        if !c["sangria"].as_bool().unwrap_or(false) {
            shift_layer(&mut c, 3.0, 3.0);
        }
        if let Some(a) = self.model["camadas"].as_array_mut() {
            a.push(c);
        }
        self.sel = Some(nid);
        self.step();
    }

    fn remove(&mut self) {
        let Some(id) = self.sel.take() else { return };
        if let Some(a) = self.model["camadas"].as_array_mut() {
            a.retain(|l| s(l, "id") != id);
        }
        self.step();
    }

    /// Move the selection in the stack: +1 up, -1 down, ±99 to the top or bottom.
    fn restack(&mut self, d: i32) {
        let Some(id) = self.sel.clone() else { return };
        let Some(a) = self.model["camadas"].as_array_mut() else { return };
        let Some(i) = a.iter().position(|l| s(l, "id") == id) else { return };
        let n = a.len() as i32;
        let j = match d {
            99 => n - 1,
            -99 => 0,
            _ => (i as i32 + d).clamp(0, n - 1),
        } as usize;
        let l = a.remove(i);
        a.insert(j, l);
        self.step();
    }

    // ── drawing ─────────────────────────────────────────────────────

    fn want_render(&mut self, cx: &Cx, ppm: f32) {
        let key = format!("{ppm:.2}{}", self.model);
        if key == self.shown || self.job.is_some() {
            return;
        }
        self.shown = key;
        self.gen += 1;
        let gen = self.gen;
        let (tx, rx) = mpsc::channel();
        let model = self.model.clone();
        let env = self.env();
        let root = cx.vault.root.clone();
        let index = cx.vault.index.clone();
        let from = self.from.clone();
        let keys = compose::images(&model);
        let cached = self.pics.as_ref().filter(|(k, _)| *k == keys).map(|(_, p)| p.clone());
        std::thread::spawn(move || {
            let pics = cached.unwrap_or_else(|| {
                let ix = index.read().unwrap();
                Arc::new(CoverPics::load(&root, &ix, &from, &model))
            });
            let db = crate::fonts::book_fonts();
            let page = capa::page(&model, &env, &db, &pics);
            let (w, h, px) = raster::render(&page, &db, &pics.pics.list, ppm);
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            let _ = tx.send(Rendered { gen, img, ppm, pics, keys });
        });
        self.job = Some(rx);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.job {
            ctx.request_repaint_after(Duration::from_millis(60));
            if let Ok(r) = rx.try_recv() {
                self.job = None;
                let _ = r.ppm;
                let t = ctx.load_texture("compositor", r.img, egui::TextureOptions::LINEAR);
                self.tex = Some((t, r.gen));
                self.pics = Some((r.keys, r.pics));
            }
        }
    }

    /// The editor: tools and layers, the page, the properties.
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Option<Event> {
        let p = cx.palette;
        let mut ev = None;
        self.poll(ui.ctx());
        egui::TopBottomPanel::top(ui.id().with("cmp-top")).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 16, right: 12, top: 10, bottom: 8 })).show_inside(ui, |ui| {
            ev = self.top(ui, p);
        });
        egui::SidePanel::left(ui.id().with("cmp-tools")).resizable(false).exact_width(196.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 12, right: 10, top: 10, bottom: 10 })).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("cmp-tools-s").auto_shrink([false, false]).show(ui, |ui| self.tools(ui, p));
        });
        egui::SidePanel::right(ui.id().with("cmp-props")).resizable(false).exact_width(284.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 10 })).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("cmp-props-s").auto_shrink([false, false]).show(ui, |ui| self.props(ui, p));
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg_secondary)).show_inside(ui, |ui| self.stage(ui, p, cx));
        self.keys(ui.ctx());
        if self.drag.is_none() {
            self.commit();
        }
        if self.pick.is_some() {
            self.pick_dialog(ui.ctx(), p, cx);
        }
        if let Some(t) = self.confirm.clone() {
            self.confirm_dialog(ui.ctx(), p, &t);
        }
        ev
    }

    fn top(&mut self, ui: &mut egui::Ui, p: Palette) -> Option<Event> {
        let mut ev = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new(&self.title).font(kit::bold(15.5)).color(p.text));
            ui.add_space(8.0);
            if self.capa {
                if kit::quiet(ui, Some(icons::ROTATE_CCW), "Recriar capa padrão", p).clicked() {
                    self.confirm = Some("capa".into());
                }
            } else {
                egui::ComboBox::from_id_salt("cmp-tpl").width(170.0).selected_text(RichText::new("Modelo de página…").size(12.5)).show_ui(ui, |ui| {
                    for (k, v) in compose::TEMPLATES {
                        if ui.selectable_label(false, v).clicked() {
                            if self.layers().is_empty() {
                                let env = self.env();
                                self.model["camadas"] = json!(compose::template(k, &env));
                                self.sel = None;
                                self.step();
                            } else {
                                self.confirm = Some(k.to_string());
                            }
                        }
                    }
                });
                let dupla = s(&self.model, "tipo") == "dupla";
                if let Some(i) = kit::segmented(ui, &[("", "Página"), ("", "Dupla")], if dupla { 1 } else { 0 }, p) {
                    self.model["tipo"] = json!(if i == 1 { "dupla" } else { "pagina" });
                    self.zoom = None;
                    self.step();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if kit::primary_icon(ui, icons::CHECK, "Salvar", p).clicked() {
                    self.commit();
                    ev = Some(Event::Save(self.model.clone()));
                }
                if kit::button(ui, None, "Fechar", p).clicked() {
                    ev = Some(Event::Close);
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if kit::icon_button(ui, icons::UNDO_2, "Desfazer (⌘Z)", false, p).clicked() {
                self.undo();
            }
            if kit::icon_button(ui, icons::REDO_2, "Refazer (⇧⌘Z)", false, p).clicked() {
                self.redo();
            }
            ui.add_space(8.0);
            let z = self.zoom.unwrap_or(3.0);
            if kit::icon_button(ui, icons::ZOOM_OUT, "Afastar", false, p).clicked() {
                self.zoom = Some((z / 1.2).max(0.5));
            }
            if kit::icon_button(ui, icons::MAXIMIZE, "Ajustar à tela", self.zoom.is_none(), p).clicked() {
                self.zoom = None;
            }
            if kit::icon_button(ui, icons::ZOOM_IN, "Aproximar", false, p).clicked() {
                self.zoom = Some((z * 1.2).min(16.0));
            }
            ui.add_space(8.0);
            if kit::icon_button(ui, icons::RULER, "Guias: corte, sangria, dobras, área de segurança", self.guides, p).clicked() {
                self.guides = !self.guides;
            }
            if kit::icon_button(ui, icons::MAGNET, "Ímã (arredonda a meio por cento)", self.snap, p).clicked() {
                self.snap = !self.snap;
            }
            if self.dirty() {
                ui.add_space(10.0);
                ui.label(RichText::new("alterações não salvas").size(11.5).color(p.faint));
            }
        });
        ev
    }

    fn tools(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.label(RichText::new("ADICIONAR").size(10.5).color(p.faint));
        ui.add_space(4.0);
        let full = |ui: &mut egui::Ui, g: &str, l: &str| -> bool { ui.add_sized([ui.available_width(), 28.0], egui::Button::new(RichText::new(format!("{g}  {l}")).size(12.5)).fill(p.bg_secondary).stroke(Stroke::NONE)).clicked() };
        let g = |g: &str| RichText::new(g).font(kit::icon_font(12.0));
        let _ = g;
        if full(ui, icons::SQUARE, "Painel") {
            self.add(json!({ "t": "painel", "x": 20, "y": 20, "w": 60, "h": 35, "borda": 0.6 }));
        }
        if full(ui, icons::IMAGE, "Imagem") {
            self.add(json!({ "t": "imagem", "x": 15, "y": 15, "w": 70, "h": 45, "img": "", "borda": 0 }));
        }
        if full(ui, icons::TYPE, "Texto") {
            self.add(json!({ "t": "texto", "x": 15, "y": 60, "w": 70, "h": 20, "texto": "Texto", "tam": 11, "alin": "left", "entre": 1.35 }));
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Balões").size(11.5).color(p.muted));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (k, v) in [("fala", "Fala"), ("pensamento", "Pensar"), ("grito", "Grito"), ("sussurro", "Sussurro"), ("narracao", "Narração"), ("eletronico", "Rádio")] {
                if kit::chip(ui, None, v, false, p).clicked() {
                    let narr = k == "narracao";
                    self.add(json!({ "t": "balao", "forma": k, "x": 30, "y": 20, "w": 32, "h": 14, "texto": if narr { "Enquanto isso…" } else { "Fala!" }, "tam": 9, "cauda": if narr { Json::Null } else { json!([40, 42]) } }));
                }
            }
        });
        ui.add_space(4.0);
        if full(ui, icons::ZAP, "Onomatopeia") {
            self.add(json!({ "t": "sfx", "x": 25, "y": 30, "w": 50, "h": 18, "texto": "KRAAASH", "tam": 44, "rot": -8, "inclina": -8 }));
        }
        if full(ui, icons::SHAPES, "Forma") {
            self.add(json!({ "t": "forma", "forma": "ret", "x": 25, "y": 25, "w": 30, "h": 20, "fundo": "#000000", "opac": 0.5 }));
        }
        if full(ui, icons::SUNSET, "Degradê") {
            self.add(json!({ "t": "forma", "forma": "gradiente", "x": 0, "y": 55, "w": 100, "h": 45, "fundo": "#000000", "grad0": 0, "grad1": 0.8 }));
        }
        ui.add_space(12.0);
        ui.label(RichText::new("CAMADAS").size(10.5).color(p.faint));
        ui.add_space(4.0);
        let list = self.layers();
        if list.is_empty() {
            ui.label(RichText::new("Nenhuma camada ainda.").size(12.0).color(p.faint));
        }
        let mut toggle = None;
        for l in list.iter().rev() {
            let id = s(l, "id").to_string();
            let glyph = match s(l, "t") {
                "painel" => icons::SQUARE,
                "imagem" => icons::IMAGE,
                "texto" => icons::TYPE,
                "balao" => icons::MESSAGE_CIRCLE,
                "sfx" => icons::ZAP,
                _ => icons::SHAPES,
            };
            let txt = s(l, "texto");
            let mut name = if !txt.is_empty() {
                txt.chars().take(22).collect::<String>()
            } else if !s(l, "img").is_empty() {
                capa::link_text(s(l, "img")).rsplit('/').next().unwrap_or("").to_string()
            } else {
                compose::LAYER_TYPES.iter().find(|t| t.0 == s(l, "t")).map(|t| t.1.to_string()).unwrap_or_default()
            };
            if self.capa && !s(l, "zona").is_empty() {
                name = format!("[{}] {name}", s(l, "zona"));
            }
            let hidden = l["oculto"].as_bool().unwrap_or(false);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if kit::icon_button_sized(ui, if hidden { icons::EYE_OFF } else { icons::EYE }, "Mostrar/ocultar", false, p, 22.0, 12.0).clicked() {
                    toggle = Some(id.clone());
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let r = kit::Row::new(&name).icon(glyph).selected(self.sel.as_deref() == Some(id.as_str())).dim(hidden).show(ui, p);
                    if r.clicked() {
                        self.sel = Some(id.clone());
                    }
                });
            });
        }
        if let Some(id) = toggle {
            if let Some(l) = self.layer_mut(&id) {
                let h = l["oculto"].as_bool().unwrap_or(false);
                if h {
                    l.as_object_mut().map(|o| o.remove("oculto"));
                } else {
                    l["oculto"] = json!(true);
                }
            }
            self.step();
        }
    }

    // ── the page ────────────────────────────────────────────────────

    fn stage(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx) {
        let env = self.env();
        let b = env.bleed;
        let (tw, th) = (env.w + 2.0 * b, env.h + 2.0 * b);
        let avail = ui.available_rect_before_wrap();
        let fit = ((avail.width() - 48.0) / tw).min((avail.height() - 48.0) / th).max(0.3);
        let z = self.zoom.unwrap_or(fit);
        let ppp = ui.ctx().pixels_per_point();
        let ppm = ((z * ppp).min(4096.0 / tw.max(th)) * 4.0).round() / 4.0;
        self.want_render(cx, ppm);
        egui::ScrollArea::both().id_salt("cmp-stage").auto_shrink([false, false]).show(ui, |ui| {
            let size = vec2(tw * z, th * z);
            let room = vec2(size.x.max(avail.width() - 2.0), size.y.max(avail.height() - 2.0));
            let (outer, _) = ui.allocate_exact_size(room, Sense::hover());
            let rect = Rect::from_center_size(outer.center(), size);
            let resp = ui.interact(rect, ui.id().with("cmp-canvas"), Sense::click_and_drag());
            let painter = ui.painter_at(outer);
            painter.rect_filled(rect.translate(vec2(0.0, 3.0)).expand(1.0), 2.0, Color32::from_black_alpha(if p.dark { 90 } else { 30 }));
            match &self.tex {
                Some((t, _)) => {
                    painter.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    painter.rect_filled(rect, 0.0, Color32::WHITE);
                    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Compondo…", egui::FontId::proportional(13.0), Color32::GRAY);
                }
            }
            let to = |x: f32, y: f32| pos2(rect.left() + (x + b) * z, rect.top() + (y + b) * z);
            let from = |q: Pos2| ((q.x - rect.left()) / z - b, (q.y - rect.top()) / z - b);
            if self.guides {
                self.draw_guides(&painter, &env, &to, z);
            }
            self.interact(ui, &resp, &env, &to, &from, z);
            self.overlay(&painter, p, &env, &to, z);
        });
    }

    fn draw_guides(&self, painter: &egui::Painter, env: &Env, to: &dyn Fn(f32, f32) -> Pos2, z: f32) {
        let b = env.bleed;
        let dashed = |a: Pos2, c: Pos2, col: Color32| {
            painter.extend(egui::Shape::dashed_line(&[a, c], Stroke::new(1.0_f32, col), 5.0, 4.0));
        };
        let rect_d = |x: f32, y: f32, w: f32, h: f32, col: Color32| {
            let (a, c) = (to(x, y), to(x + w, y + h));
            dashed(a, pos2(c.x, a.y), col);
            dashed(pos2(c.x, a.y), c, col);
            dashed(c, pos2(a.x, c.y), col);
            dashed(pos2(a.x, c.y), a, col);
        };
        let blue = Color32::from_rgb(0x00, 0xa3, 0xff);
        let red = Color32::from_rgb(0xe5, 0x48, 0x4d);
        let green = Color32::from_rgb(0x00, 0xc8, 0x53);
        let amber = Color32::from_rgba_unmultiplied(0xff, 0xb3, 0x00, 200);
        if b > 0.0 {
            rect_d(-b, -b, env.w + 2.0 * b, env.h + 2.0 * b, red);
        }
        painter.rect_stroke(Rect::from_min_max(to(0.0, 0.0), to(env.w, env.h)), 0.0, Stroke::new(1.0_f32, blue), egui::StrokeKind::Middle);
        if env.cover {
            for (name, zn) in &env.zones {
                if name == "pagina" || name == "tudo" {
                    continue;
                }
                if zn.x > 0.01 {
                    dashed(to(zn.x, -b), to(zn.x, env.h + b), green);
                }
                let safe = 5.0;
                if zn.w > 2.0 * safe {
                    rect_d(zn.x + safe, safe, zn.w - 2.0 * safe, env.h - 2.0 * safe, amber);
                }
                let label = match name.as_str() {
                    "contracapa" => "CONTRACAPA",
                    "lombada" => "LOMBADA",
                    "capa" => "CAPA",
                    _ => "ORELHA",
                };
                painter.text(to(zn.x + zn.w / 2.0, env.h + b) + vec2(0.0, 10.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional((9.0 * z / 3.0).clamp(8.0, 11.0)), blue);
            }
        } else {
            let [mi, me, ms, mb] = env.margins;
            let n = if env.w > env.page_w + 0.1 { 2 } else { 1 };
            for i in 0..n {
                let x0 = i as f32 * env.page_w;
                let (l, r) = if n == 2 { if i == 0 { (me, mi) } else { (mi, me) } } else { ((mi + me) / 2.0, (mi + me) / 2.0) };
                rect_d(x0 + l, ms, env.page_w - l - r, env.h - ms - mb, amber);
            }
            if n == 2 {
                dashed(to(env.page_w, -b), to(env.page_w, env.h + b), green);
            }
        }
    }

    /// The selected layer's box corners in mm (turned with it).
    fn corners(&self, l: &Json, env: &Env) -> [(f32, f32); 4] {
        let r = compose::layer_rect(l, env);
        let c = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let rot = f(&l["rot"], 0.0);
        [(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)].map(|q| rot_pt(q, c, rot))
    }

    fn handles(&self, l: &Json, env: &Env) -> Vec<(Mode, (f32, f32))> {
        let mut out = Vec::new();
        let r = compose::layer_rect(l, env);
        let c = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let rot = f(&l["rot"], 0.0);
        let has_pts = s(l, "t") == "painel" && l["pts"].as_array().is_some_and(|a| a.len() >= 3);
        if l["sangria"].as_bool().unwrap_or(false) {
            return out;
        }
        if !has_pts {
            for (k, x, y) in [("nw", 0.0, 0.0), ("n", 0.5, 0.0), ("ne", 1.0, 0.0), ("e", 1.0, 0.5), ("se", 1.0, 1.0), ("s", 0.5, 1.0), ("sw", 0.0, 1.0), ("w", 0.0, 0.5)] {
                out.push((Mode::Resize(k), rot_pt((r.x + r.w * x, r.y + r.h * y), c, rot)));
            }
        }
        if let Some(pts) = l["pts"].as_array().filter(|_| has_pts) {
            for (i, q) in pts.iter().enumerate() {
                out.push((Mode::Pt(i), compose::zone_point(q, l, env)));
            }
        }
        if s(l, "t") == "balao" {
            if let Some(c2) = l["cauda"].as_array().filter(|a| a.len() == 2) {
                let zn = env.zone(s(l, "zona"));
                out.push((Mode::Tail, (zn.x + f(&c2[0], 0.0) / 100.0 * zn.w, zn.y + f(&c2[1], 0.0) / 100.0 * zn.h)));
            }
        }
        out
    }

    fn overlay(&self, painter: &egui::Painter, p: Palette, env: &Env, to: &dyn Fn(f32, f32) -> Pos2, z: f32) {
        let Some(l) = self.sel.as_ref().and_then(|id| self.layer(id)) else { return };
        let accent = p.accent_strong;
        let c = self.corners(l, env);
        let pts: Vec<Pos2> = c.iter().map(|q| to(q.0, q.1)).collect();
        painter.add(egui::Shape::closed_line(pts.clone(), Stroke::new(1.4_f32, accent)));
        if l["sangria"].as_bool().unwrap_or(false) {
            return;
        }
        let has_pts = s(l, "t") == "painel" && l["pts"].as_array().is_some_and(|a| a.len() >= 3);
        if !has_pts {
            // rotation handle above the top edge
            let top = pos2((pts[0].x + pts[1].x) / 2.0, (pts[0].y + pts[1].y) / 2.0);
            let rot = f(&l["rot"], 0.0).to_radians();
            let up = vec2(rot.sin(), -rot.cos());
            let h = top + up * 22.0;
            painter.line_segment([top, h], Stroke::new(1.0_f32, accent));
            painter.circle(h, 6.0, p.bg, Stroke::new(1.4_f32, accent));
        }
        for (m, q) in self.handles(l, env) {
            let at = to(q.0, q.1);
            match m {
                Mode::Tail => {
                    painter.circle(at, 5.5, Color32::from_rgb(0xff, 0xb3, 0x00), Stroke::new(1.4_f32, Color32::WHITE));
                }
                Mode::Pt(_) => {
                    painter.circle(at, 5.0, accent, Stroke::new(1.4_f32, Color32::WHITE));
                }
                _ => {
                    painter.rect(Rect::from_center_size(at, vec2(8.0, 8.0)), 1.5, p.bg, Stroke::new(1.4_f32, accent), egui::StrokeKind::Middle);
                }
            }
        }
        let _ = z;
    }

    fn hit(&self, q: (f32, f32), env: &Env) -> Option<String> {
        for l in self.layers().iter().rev() {
            if l["oculto"].as_bool().unwrap_or(false) {
                continue;
            }
            if let Some(pts) = l["pts"].as_array().filter(|a| a.len() >= 3 && s(l, "t") == "painel") {
                let poly: Vec<(f32, f32)> = pts.iter().map(|p| compose::zone_point(p, l, env)).collect();
                if in_poly(q, &poly) {
                    return Some(s(l, "id").to_string());
                }
                continue;
            }
            if in_poly(q, &self.corners(l, env)) {
                return Some(s(l, "id").to_string());
            }
        }
        None
    }

    fn interact(&mut self, ui: &egui::Ui, resp: &egui::Response, env: &Env, to: &dyn Fn(f32, f32) -> Pos2, from: &dyn Fn(Pos2) -> (f32, f32), z: f32) {
        let alt = ui.input(|i| i.modifiers.alt);
        let shift = ui.input(|i| i.modifiers.shift);
        // zoom: ⌘/Ctrl + wheel the page, Alt + wheel the art in its panel
        if resp.hovered() {
            let (dy, cmd) = ui.input(|i| (i.raw_scroll_delta.y, i.modifiers.command));
            if dy != 0.0 && cmd {
                let k = if dy > 0.0 { 1.08 } else { 1.0 / 1.08 };
                self.zoom = Some((z * k).clamp(0.3, 16.0));
            } else if dy != 0.0 && alt {
                if let Some(id) = self.sel.clone() {
                    if let Some(l) = self.layer_mut(&id).filter(|l| matches!(s(l, "t"), "painel" | "imagem")) {
                        let zm = f(&l["zoom"], 1.0) * if dy > 0.0 { 1.06 } else { 1.0 / 1.06 };
                        l["zoom"] = json!((zm.clamp(0.2, 8.0) * 1000.0).round() / 1000.0);
                    }
                }
            }
        }
        if resp.double_clicked() {
            if let Some(q) = resp.interact_pointer_pos() {
                if let Some(id) = self.hit(from(q), env) {
                    if self.layer(&id).is_some_and(|l| matches!(s(l, "t"), "painel" | "imagem")) {
                        self.pick = Some((id, String::new()));
                    }
                }
            }
            return;
        }
        if resp.drag_started() {
            // where the button went down, not where the drag was noticed
            let Some(q) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()) else { return };
            let mm = from(q);
            // a handle of the selection first
            let mut grabbed = None;
            if let Some(l) = self.sel.as_ref().and_then(|id| self.layer(id)).cloned() {
                for (m, h) in self.handles(&l, env) {
                    if to(h.0, h.1).distance(q) < 8.0 {
                        grabbed = Some(m);
                        break;
                    }
                }
                let has_pts = s(&l, "t") == "painel" && l["pts"].as_array().is_some_and(|a| a.len() >= 3);
                if grabbed.is_none() && !has_pts && !l["sangria"].as_bool().unwrap_or(false) {
                    let c = self.corners(&l, env);
                    let (a, b2) = (to(c[0].0, c[0].1), to(c[1].0, c[1].1));
                    let top = pos2((a.x + b2.x) / 2.0, (a.y + b2.y) / 2.0);
                    let rot = f(&l["rot"], 0.0).to_radians();
                    if (top + vec2(rot.sin(), -rot.cos()) * 22.0).distance(q) < 9.0 {
                        grabbed = Some(Mode::Rot);
                    }
                }
            }
            let id = if grabbed.is_some() { self.sel.clone() } else { self.hit(mm, env) };
            self.sel = id.clone();
            let Some(id) = id else { return };
            let Some(orig) = self.layer(&id).cloned() else { return };
            let mode = grabbed.unwrap_or(if alt && matches!(s(&orig, "t"), "painel" | "imagem") { Mode::Pan } else { Mode::Move });
            self.step();
            self.drag = Some(Drag { id, mode, start: mm, orig, moved: false });
        }
        if resp.clicked() {
            if let Some(q) = resp.interact_pointer_pos() {
                self.sel = self.hit(from(q), env);
            }
        }
        if let (Some(d), Some(q)) = (self.drag.as_mut(), resp.interact_pointer_pos()) {
            if resp.dragged() {
                let cur = from(q);
                let snap_on = self.snap;
                let (dxm, dym) = (cur.0 - d.start.0, cur.1 - d.start.1);
                d.moved = d.moved || dxm.abs() + dym.abs() > 0.2;
                if d.moved {
                    let o = d.orig.clone();
                    let mode = d.mode;
                    let id = d.id.clone();
                    let zn = env.zone(s(&o, "zona"));
                    let mut l = o.clone();
                    apply_drag(&mut l, &o, mode, cur, (dxm, dym), zn, env, snap_on, shift);
                    if let Some(t) = self.layer_mut(&id) {
                        *t = l;
                    }
                }
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
            self.step();
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.memory(|m| m.focused().is_some()) || self.pick.is_some() || self.confirm.is_some() {
            return;
        }
        let (z, sz, d, del, esc, arrows, shift) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            let mut ar = (0.0f32, 0.0f32);
            if i.key_pressed(egui::Key::ArrowLeft) {
                ar.0 -= 1.0;
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                ar.0 += 1.0;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                ar.1 -= 1.0;
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                ar.1 += 1.0;
            }
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::Escape),
                ar,
                i.modifiers.shift,
            )
        });
        if z {
            self.undo();
        }
        if sz {
            self.redo();
        }
        if d {
            self.duplicate();
        }
        if del {
            self.remove();
        }
        if esc {
            self.sel = None;
        }
        if arrows != (0.0, 0.0) {
            let k = if shift { 3.0 } else { 0.5 };
            if let Some(id) = self.sel.clone() {
                if let Some(l) = self.layer_mut(&id).filter(|l| !l["sangria"].as_bool().unwrap_or(false)) {
                    shift_layer(l, arrows.0 * k, arrows.1 * k);
                }
            }
        }
    }

    // ── properties ──────────────────────────────────────────────────

    fn props(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.spacing_mut().item_spacing.y = 6.0;
        let Some(id) = self.sel.clone().filter(|id| self.layer(id).is_some()) else {
            self.page_props(ui, p);
            return;
        };
        let t = s(self.layer(&id).unwrap(), "t").to_string();
        let title = compose::LAYER_TYPES.iter().find(|x| x.0 == t).map(|x| x.1).unwrap_or("Camada");
        ui.label(RichText::new(title).font(kit::medium(14.0)).color(p.text));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if kit::icon_button(ui, icons::CHEVRONS_UP, "Trazer para a frente", false, p).clicked() {
                self.restack(99);
            }
            if kit::icon_button(ui, icons::CHEVRON_UP, "Subir", false, p).clicked() {
                self.restack(1);
            }
            if kit::icon_button(ui, icons::CHEVRON_DOWN, "Descer", false, p).clicked() {
                self.restack(-1);
            }
            if kit::icon_button(ui, icons::CHEVRONS_DOWN, "Enviar para trás", false, p).clicked() {
                self.restack(-99);
            }
            if kit::icon_button(ui, icons::COPY, "Duplicar (⌘D)", false, p).clicked() {
                self.duplicate();
            }
            if kit::icon_button(ui, icons::TRASH_2, "Excluir", false, p).clicked() {
                self.remove();
            }
        });
        if self.sel.is_none() {
            return;
        }
        kit::hairline(ui, p);
        let capa = self.capa;
        let mut pick = false;
        let Some(l) = self.layer_mut(&id) else { return };
        let mut pr = Props { l, p };
        if capa {
            pr.choice_ui(ui, "Zona", "zona", &compose::ZONE_NAMES, "capa");
        }
        if t == "imagem" || t == "painel" {
            let img = capa::link_text(s(pr.l, "img"));
            let name = if img.is_empty() { "Escolher arte…".to_string() } else { img.rsplit('/').next().unwrap_or("").to_string() };
            if ui.add_sized([ui.available_width(), 28.0], egui::Button::new(RichText::new(format!("{}  {name}", icons::IMAGE)).size(12.5))).on_hover_text(img.as_str()).clicked() {
                pick = true;
            }
            if !img.is_empty() && kit::quiet(ui, Some(icons::X), "Remover arte", p).clicked() {
                pr.l.as_object_mut().map(|o| o.remove("img"));
            }
            pr.check(ui, "Sangrar (página inteira)", "sangria");
            pr.choice_ui(ui, "Ajuste", "ajuste", &[("", "Cobrir"), ("conter", "Conter"), ("esticar", "Esticar")], "");
            pr.slider(ui, "Zoom da arte", "zoom", 1.0, 0.2, 5.0);
            pr.slider(ui, "Deslocamento X", "ox", 0.0, -100.0, 100.0);
            pr.slider(ui, "Deslocamento Y", "oy", 0.0, -100.0, 100.0);
            pr.choice_ui(ui, "Filtro", "filtro", &compose::FILTERS, "");
            pr.check(ui, "Espelhar", "espelhar");
            pr.color(ui, "Fundo", "fundo", "#ffffff");
            pr.number(ui, "Borda (mm)", "borda", if t == "painel" { 0.6 } else { 0.0 }, 0.1, 0.0, 20.0);
            pr.color(ui, "Cor da borda", "corBorda", "#111111");
            if t == "imagem" {
                pr.choice_ui(ui, "Forma", "forma", &[("", "Retângulo"), ("elipse", "Elipse")], "");
            }
            pr.number(ui, "Cantos arredondados", "raio", 0.0, 0.5, 0.0, 100.0);
            if t == "painel" {
                let has = pr.l["pts"].is_array();
                if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(RichText::new(format!("{}  {}", icons::SPLINE, if has { "Voltar a retângulo" } else { "Inclinar (editar cantos)" })).size(12.5))).clicked() {
                    if has {
                        pr.l.as_object_mut().map(|o| o.remove("pts"));
                    } else {
                        let (x, y, w, h) = (f(&pr.l["x"], 0.0), f(&pr.l["y"], 0.0), f(&pr.l["w"], 10.0), f(&pr.l["h"], 10.0));
                        pr.l["pts"] = json!([[x, y], [x + w, y], [x + w, y + h], [x, y + h]]);
                    }
                }
            }
        }
        if matches!(t.as_str(), "texto" | "balao" | "sfx") {
            ui.label(RichText::new("Texto").size(12.0).color(p.muted));
            let mut txt = s(pr.l, "texto").to_string();
            if ui.add(egui::TextEdit::multiline(&mut txt).desired_rows(3).desired_width(f32::INFINITY)).changed() {
                pr.l["texto"] = json!(txt);
            }
            pr.font(ui, "Fonte", "fonte");
            pr.number(ui, "Tamanho (pt)", "tam", match t.as_str() { "sfx" => 40.0, "balao" => 9.0, _ => 11.0 }, 0.5, 2.0, 400.0);
            pr.color(ui, "Cor", "cor", if t == "sfx" { "#c8102e" } else { "#111111" });
            if t != "sfx" {
                pr.choice_ui(ui, "Alinhamento", "alin", &[("left", "Esquerda"), ("center", "Centro"), ("right", "Direita"), ("justify", "Justificado")], if t == "balao" { "center" } else { "left" });
            }
            if t == "texto" {
                pr.choice_ui(ui, "Vertical", "valign", &[("top", "Topo"), ("middle", "Meio"), ("bottom", "Base")], "top");
            }
            pr.number(ui, "Entrelinha", "entre", if t == "texto" { 1.35 } else { 1.15 }, 0.05, 0.6, 4.0);
            pr.choice_ui(ui, "Peso", "peso", &[("", "Normal"), ("300", "Leve"), ("600", "Seminegrito"), ("700", "Negrito"), ("900", "Black")], "");
            pr.check(ui, "Itálico", "ital");
            pr.number(ui, "Espaçamento (pt)", "espaco", 0.0, 0.1, -5.0, 40.0);
            pr.choice_ui(ui, "Caixa", "caixa", &[("", if t == "balao" { "Alta (padrão HQ)" } else { "Normal" }), ("alta", "ALTA"), ("normal", "Normal")], "");
            pr.check(ui, "Escrita vertical (mangá)", "vertical");
            pr.number(ui, "Contorno (pt)", "contorno", if t == "sfx" { 3.0 } else { 0.0 }, 0.1, 0.0, 30.0);
            pr.color(ui, "Cor do contorno", "corContorno", "#ffffff");
        }
        if t == "texto" {
            pr.color(ui, "Fundo da caixa", "fundo", "#ffffff");
            pr.slider(ui, "Opacidade do fundo", "fundoOpac", 1.0, 0.0, 1.0);
            pr.number(ui, "Respiro interno (mm)", "pad", 0.0, 0.5, 0.0, 50.0);
            pr.number(ui, "Cantos", "raio", 0.0, 0.5, 0.0, 50.0);
            pr.number(ui, "Borda (mm)", "borda", 0.0, 0.1, 0.0, 10.0);
        }
        if t == "balao" {
            pr.choice_ui(ui, "Tipo", "forma", &compose::BALLOON_TYPES, "fala");
            let narr = s(pr.l, "forma") == "narracao";
            pr.color(ui, "Fundo", "fundo", if narr { "#f4e9c8" } else { "#ffffff" });
            pr.number(ui, "Borda (mm)", "borda", 0.45, 0.05, 0.0, 5.0);
            pr.color(ui, "Cor da borda", "corBorda", "#111111");
            if !narr {
                let has = pr.l["cauda"].is_array();
                if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(RichText::new(format!("{}  {}", icons::CORNER_DOWN_LEFT, if has { "Remover cauda" } else { "Adicionar cauda" })).size(12.5))).clicked() {
                    if has {
                        pr.l.as_object_mut().map(|o| o.remove("cauda"));
                    } else {
                        let (x, y, w, h) = (f(&pr.l["x"], 0.0), f(&pr.l["y"], 0.0), f(&pr.l["w"], 10.0), f(&pr.l["h"], 10.0));
                        pr.l["cauda"] = json!([x + w / 2.0, y + h + 14.0]);
                    }
                }
                if has {
                    pr.slider(ui, "Curva da cauda", "caudaCurva", 0.25, -1.5, 1.5);
                    pr.slider(ui, "Largura da cauda", "caudaLargura", 0.22, 0.05, 0.6);
                }
            }
        }
        if t == "sfx" {
            pr.slider(ui, "Inclinação", "inclina", 0.0, -40.0, 40.0);
        }
        if t == "forma" {
            pr.choice_ui(ui, "Forma", "forma", &[("ret", "Retângulo"), ("elipse", "Elipse"), ("linha", "Linha"), ("gradiente", "Degradê")], "ret");
            pr.color(ui, "Cor", "fundo", "#000000");
            if s(pr.l, "forma") == "gradiente" {
                pr.slider(ui, "Opacidade inicial", "grad0", 0.0, 0.0, 1.0);
                pr.slider(ui, "Opacidade final", "grad1", 0.85, 0.0, 1.0);
                let across = f(&pr.l["gradY"], 1.0) == 0.0;
                if let Some(i) = kit::segmented(ui, &[("", "Vertical ↓"), ("", "Horizontal →")], if across { 1 } else { 0 }, p) {
                    if i == 1 {
                        pr.l["gradY"] = json!(0);
                        pr.l["gradX"] = json!(1);
                    } else {
                        pr.l["gradY"] = json!(1);
                        pr.l.as_object_mut().map(|o| o.remove("gradX"));
                    }
                }
            } else {
                pr.number(ui, "Borda (mm)", "borda", 0.0, 0.1, 0.0, 20.0);
                pr.color(ui, "Cor da borda", "corBorda", "#111111");
                pr.check(ui, "Tracejado", "tracejado");
                pr.number(ui, "Cantos", "raio", 0.0, 0.5, 0.0, 100.0);
            }
        }
        kit::hairline(ui, p);
        ui.label(RichText::new("Posição (%)").size(12.0).color(p.muted));
        ui.horizontal(|ui| {
            for (k, lab) in [("x", "X"), ("y", "Y"), ("w", "L"), ("h", "A")] {
                ui.label(RichText::new(lab).size(11.5).color(p.faint));
                let mut v = f(&pr.l[k], 0.0);
                if ui.add(egui::DragValue::new(&mut v).speed(0.25).max_decimals(1)).changed() {
                    pr.l[k] = json!((v * 100.0).round() / 100.0);
                }
            }
        });
        pr.number(ui, "Rotação (°)", "rot", 0.0, 1.0, -360.0, 360.0);
        pr.slider(ui, "Opacidade", "opac", 1.0, 0.0, 1.0);
        if pick {
            self.pick = Some((id, String::new()));
        }
    }

    fn page_props(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.label(RichText::new(if self.capa { "Capa" } else { "Página" }).font(kit::medium(14.0)).color(p.text));
        let env = self.env();
        let pages = self.pages;
        let cfg_paper = self.cfg["papel"].as_str().unwrap_or("polen80").to_string();
        let capa = self.capa;
        let mut pr = Props { l: &mut self.model, p };
        pr.color(ui, "Fundo", "fundo", if capa { "#1b2a2c" } else { "#ffffff" });
        if capa {
            ui.label(RichText::new(format!("Lombada {:.1} mm · {pages} páginas · capa aberta {} × {} mm + sangria {} mm", env.spine, env.w.round(), env.h.round(), env.bleed).replace('.', ",")).size(11.5).color(p.faint));
            let paper_label = PAPERS.iter().find(|x| x.0 == cfg_paper).map(|x| x.1).unwrap_or("—");
            let mut opts: Vec<(&str, String)> = vec![("", format!("Do livro ({paper_label})"))];
            opts.extend(PAPERS.iter().map(|x| (x.0, x.1.to_string())));
            let cur = s(pr.l, "papel").to_string();
            row(ui, p, "Papel", |ui| {
                egui::ComboBox::from_id_salt("cmp-paper").width(150.0).selected_text(opts.iter().find(|o| o.0 == cur).map(|o| o.1.clone()).unwrap_or_default()).show_ui(ui, |ui| {
                    for (k, v) in &opts {
                        if ui.selectable_label(cur == *k, v.as_str()).clicked() {
                            pr.l["papel"] = json!(k);
                        }
                    }
                });
            });
            let mut auto = pr.l["lombadaAuto"] != false;
            row(ui, p, "Lombada automática", |ui| {
                if kit::toggle(ui, &mut auto, p).changed() {
                    pr.l["lombadaAuto"] = json!(auto);
                }
            });
            if !auto {
                pr.number(ui, "Lombada (mm)", "lombada", 12.0, 0.5, 1.0, 120.0);
            }
            pr.number(ui, "Orelhas (mm, 0 = sem)", "orelha", 0.0, 5.0, 0.0, 120.0);
            pr.check(ui, "Capa dura", "capaDura");
        } else {
            pr.check(ui, "Flutuar (o texto pode passar à frente)", "flutuar");
            ui.add_space(6.0);
            ui.label(RichText::new("Clique numa camada para editar. Alt + arrastar move a arte dentro do painel; Alt + rolar dá zoom na arte. Shift mantém a proporção.").size(11.5).color(p.faint));
        }
    }

    // ── dialogs ─────────────────────────────────────────────────────

    fn pick_dialog(&mut self, ctx: &egui::Context, p: Palette, cx: &Cx) {
        let Some((id, filter)) = self.pick.as_mut() else { return };
        let id = id.clone();
        let ix = cx.vault.index.read().unwrap();
        let mut items: Vec<String> = ix.file_paths().filter(|f| is_art(f)).cloned().collect();
        drop(ix);
        items.sort();
        let fl = filter.to_lowercase();
        items.retain(|x| fl.is_empty() || x.to_lowercase().contains(&fl));
        let mut chosen = None;
        let mut close = false;
        kit::scrim(ctx, p);
        egui::Window::new("cmp-pick").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Escolha a arte").font(kit::medium(16.0)).color(p.text));
            ui.label(RichText::new("png, jpg, webp, gif ou clip").size(12.0).color(p.muted));
            ui.add_space(6.0);
            ui.add(egui::TextEdit::singleline(filter).hint_text(RichText::new("Filtrar").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6))).request_focus();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                if items.is_empty() {
                    ui.label(RichText::new("Nenhuma imagem no cofre.").color(p.faint));
                }
                for it in items.iter().take(400) {
                    if kit::Row::new(it).icon(icons::IMAGE).show(ui, p).clicked() {
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
            if let Some(l) = self.layer_mut(&id) {
                l["img"] = json!(c);
            }
            self.step();
        } else if close {
            self.pick = None;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, p: Palette, what: &str) {
        let mut done = None;
        kit::scrim(ctx, p);
        egui::Window::new("cmp-confirm").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, -40.0]).show(ctx, |ui| {
            ui.set_width(360.0);
            let (t, d, ok) = if what == "capa" { ("Recriar capa", "Isso substitui todas as camadas da capa.", "Recriar") } else { ("Trocar modelo", "Substituir as camadas atuais pelo modelo escolhido?", "Substituir") };
            ui.label(RichText::new(t).font(kit::medium(16.0)).color(p.text));
            ui.label(RichText::new(d).size(12.5).color(p.muted));
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary(ui, ok, p).clicked() {
                    done = Some(true);
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                if what == "capa" {
                    let title = self.title.trim_start_matches("Capa — ").to_string();
                    let fresh = compose::default_cover(&title, "", "", &self.sinopse);
                    let mut c = fresh["camadas"].clone();
                    // keep the art already chosen
                    if let Some(img) = self.layers().iter().find(|l| s(l, "id") == "arte-capa").map(|l| s(l, "img").to_string()) {
                        c[0]["img"] = json!(img);
                    }
                    self.model["camadas"] = c;
                } else {
                    let env = self.env();
                    self.model["camadas"] = json!(compose::template(what, &env));
                }
                self.sel = None;
                self.step();
                self.confirm = None;
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }
}

/// Move a layer (its corners and its tail too) by % of its zone.
fn shift_layer(l: &mut Json, dx: f32, dy: f32) {
    l["x"] = json!(f(&l["x"], 0.0) + dx);
    l["y"] = json!(f(&l["y"], 0.0) + dy);
    if let Some(pts) = l["pts"].as_array_mut() {
        for q in pts.iter_mut() {
            *q = json!([f(&q[0], 0.0) + dx, f(&q[1], 0.0) + dy]);
        }
    }
    if let Some(c) = l["cauda"].as_array().filter(|c| c.len() == 2).cloned() {
        l["cauda"] = json!([f(&c[0], 0.0) + dx, f(&c[1], 0.0) + dy]);
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_drag(l: &mut Json, o: &Json, mode: Mode, cur: (f32, f32), d: (f32, f32), z: compose::Zone, env: &Env, snap_on: bool, shift: bool) {
    let (dx, dy) = (d.0 / z.w * 100.0, d.1 / z.h * 100.0);
    match mode {
        Mode::Move => {
            if o["sangria"].as_bool().unwrap_or(false) {
                return;
            }
            let nx = snap(snap_on, f(&o["x"], 0.0) + dx);
            let ny = snap(snap_on, f(&o["y"], 0.0) + dy);
            let (ddx, ddy) = (nx - f(&o["x"], 0.0), ny - f(&o["y"], 0.0));
            *l = o.clone();
            shift_layer(l, ddx, ddy);
            l["x"] = json!(nx);
            l["y"] = json!(ny);
        }
        Mode::Pan => {
            let r = compose::layer_rect(o, env);
            l["ox"] = json!(((f(&o["ox"], 0.0) + d.0 / r.w * 100.0) * 100.0).round() / 100.0);
            l["oy"] = json!(((f(&o["oy"], 0.0) + d.1 / r.h * 100.0) * 100.0).round() / 100.0);
        }
        Mode::Tail => {
            l["cauda"] = json!([((cur.0 - z.x) / z.w * 1000.0).round() / 10.0, ((cur.1 - z.y) / z.h * 1000.0).round() / 10.0]);
        }
        Mode::Pt(i) => {
            let Some(pts) = o["pts"].as_array() else { return };
            let mut pts: Vec<(f32, f32)> = pts.iter().map(|q| (f(&q[0], 0.0), f(&q[1], 0.0))).collect();
            if i < pts.len() {
                pts[i] = (snap(snap_on, (cur.0 - z.x) / z.w * 100.0), snap(snap_on, (cur.1 - z.y) / z.h * 100.0));
            }
            let (x0, y0) = pts.iter().fold((f32::MAX, f32::MAX), |a, q| (a.0.min(q.0), a.1.min(q.1)));
            let (x1, y1) = pts.iter().fold((f32::MIN, f32::MIN), |a, q| (a.0.max(q.0), a.1.max(q.1)));
            l["pts"] = json!(pts.iter().map(|q| [q.0, q.1]).collect::<Vec<_>>());
            l["x"] = json!(x0);
            l["y"] = json!(y0);
            l["w"] = json!(x1 - x0);
            l["h"] = json!(y1 - y0);
        }
        Mode::Rot => {
            let r = compose::layer_rect(o, env);
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let mut a = (cur.1 - cy).atan2(cur.0 - cx).to_degrees() + 90.0;
            if shift {
                a = (a / 15.0).round() * 15.0;
            } else if snap_on {
                a = a.round();
            }
            if a > 180.0 {
                a -= 360.0;
            }
            l["rot"] = json!((a * 10.0).round() / 10.0);
        }
        Mode::Resize(m) => {
            // in the layer's turned frame
            let th = -f(&o["rot"], 0.0).to_radians();
            let lx = (d.0 * th.cos() - d.1 * th.sin()) / z.w * 100.0;
            let ly = (d.0 * th.sin() + d.1 * th.cos()) / z.h * 100.0;
            let (ox, oy, ow, oh) = (f(&o["x"], 0.0), f(&o["y"], 0.0), f(&o["w"], 10.0), f(&o["h"], 10.0));
            let (mut x, mut y, mut w, mut h) = (ox, oy, ow, oh);
            if m.contains('e') {
                w = ow + lx;
            }
            if m.contains('s') {
                h = oh + ly;
            }
            if m.contains('w') {
                x = ox + lx;
                w = ow - lx;
            }
            if m.contains('n') {
                y = oy + ly;
                h = oh - ly;
            }
            if shift && ow > 0.0 && oh > 0.0 {
                let ratio = oh / ow;
                if m == "n" || m == "s" {
                    w = h / ratio;
                } else {
                    h = w * ratio;
                }
            }
            if w < 1.0 {
                if m.contains('w') {
                    x -= 1.0 - w;
                }
                w = 1.0;
            }
            if h < 1.0 {
                if m.contains('n') {
                    y -= 1.0 - h;
                }
                h = 1.0;
            }
            l["x"] = json!(snap(snap_on, x));
            l["y"] = json!(snap(snap_on, y));
            l["w"] = json!(snap(snap_on, w));
            l["h"] = json!(snap(snap_on, h));
        }
    }
}

pub fn is_art(f: &str) -> bool {
    let l = f.to_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif", ".clip"].iter().any(|e| l.ends_with(e))
}

fn row(ui: &mut egui::Ui, p: Palette, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.5).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// Property controls bound to a layer's (or the page's) JSON.
struct Props<'a> {
    l: &'a mut Json,
    p: Palette,
}

impl Props<'_> {
    fn clear(&mut self, k: &str) {
        if let Some(o) = self.l.as_object_mut() {
            o.remove(k);
        }
    }

    fn number(&mut self, ui: &mut egui::Ui, label: &str, k: &str, d: f32, step: f32, min: f32, max: f32) {
        let mut v = f(&self.l[k], d);
        let p = self.p;
        row(ui, p, label, |ui| {
            if ui.add(egui::DragValue::new(&mut v).speed(step).range(min..=max).max_decimals(2)).changed() {
                self.l[k] = json!((v * 100.0).round() / 100.0);
            }
        });
    }

    fn slider(&mut self, ui: &mut egui::Ui, label: &str, k: &str, d: f32, min: f32, max: f32) {
        let mut v = f(&self.l[k], d);
        ui.label(RichText::new(label).size(12.5).color(self.p.text));
        if ui.add(egui::Slider::new(&mut v, min..=max).max_decimals(2)).changed() {
            self.l[k] = json!((v * 100.0).round() / 100.0);
        }
    }

    fn check(&mut self, ui: &mut egui::Ui, label: &str, k: &str) {
        let mut on = self.l[k].as_bool().unwrap_or(false);
        let p = self.p;
        row(ui, p, label, |ui| {
            if kit::toggle(ui, &mut on, p).changed() {
                if on {
                    self.l[k] = json!(true);
                } else {
                    self.clear(k);
                }
            }
        });
    }

    fn color(&mut self, ui: &mut egui::Ui, label: &str, k: &str, d: &str) {
        let set = self.l[k].as_str().filter(|s| !s.is_empty()).map(String::from);
        let c = super::color(set.as_deref().unwrap_or(d));
        let mut rgb = [c.r(), c.g(), c.b()];
        let p = self.p;
        row(ui, p, label, |ui| {
            if set.is_some() && kit::icon_button_sized(ui, icons::X, "Sem cor", false, p, 20.0, 11.0).clicked() {
                self.clear(k);
                return;
            }
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                self.l[k] = json!(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
            }
        });
    }

    fn choice_ui(&mut self, ui: &mut egui::Ui, label: &str, k: &str, opts: &[(&str, &str)], d: &str) {
        let cur = match &self.l[k] {
            Json::String(s) => s.clone(),
            Json::Number(n) => n.to_string(),
            _ => d.to_string(),
        };
        let shown = opts.iter().find(|o| o.0 == cur).map(|o| o.1).unwrap_or(cur.as_str()).to_string();
        let p = self.p;
        row(ui, p, label, |ui| {
            egui::ComboBox::from_id_salt(("cmp-choice", k)).width(140.0).selected_text(shown).show_ui(ui, |ui| {
                for (v, lab) in opts {
                    if ui.selectable_label(cur == *v, *lab).clicked() {
                        if v.is_empty() {
                            self.clear(k);
                        } else if let Ok(n) = v.parse::<f64>() {
                            self.l[k] = json!(n);
                        } else {
                            self.l[k] = json!(v);
                        }
                    }
                }
            });
        });
    }

    fn font(&mut self, ui: &mut egui::Ui, label: &str, k: &str) {
        let cur = s(self.l, k).to_string();
        let fams = super::diagramacao_ui::font_families();
        let p = self.p;
        row(ui, p, label, |ui| {
            egui::ComboBox::from_id_salt(("cmp-font", k)).width(150.0).height(360.0).selected_text(if cur.is_empty() { "padrão".to_string() } else { cur.clone() }).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_empty(), "padrão").clicked() {
                    self.clear(k);
                }
                for fm in &fams {
                    if ui.selectable_label(cur.eq_ignore_ascii_case(fm), fm.as_str()).clicked() {
                        self.l[k] = json!(fm);
                    }
                }
            });
        });
    }
}
