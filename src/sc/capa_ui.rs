//! Capa: the book in 3D (drag to turn it), the open cover with its
//! guides, the spine's thickness simulated for other page counts and
//! papers, the cover's PDF (with bleed) and a picture of the front; the
//! compositor edits it.

use super::compositor::{self, Compositor, Event};
use super::library::Cx;
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::scriptorium::book::{self, Book, PAPERS};
use ateliet_core::scriptorium::capa::{self, CoverPics};
use ateliet_core::typeset::compose::{self, Env};
use ateliet_core::typeset::raster;
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

struct Sim {
    on: bool,
    pages: usize,
    paper: String,
    hard: Option<bool>,
}

pub struct CapaView {
    pub book: String,
    model: Option<Json>,
    edit: Option<Compositor>,
    sim: Sim,
    yaw: f32,
    pitch: f32,
    shown: String,
    job: Option<Receiver<(String, egui::ColorImage)>>,
    tex: Option<egui::TextureHandle>,
    pics: Option<(Vec<(String, String)>, Arc<CoverPics>)>,
    out: Option<Receiver<Result<PathBuf, String>>>,
    pick: Option<String>,
    toast: Option<String>,
}

impl CapaView {
    pub fn new(book: &str) -> CapaView {
        CapaView { book: book.into(), model: None, edit: None, sim: Sim { on: false, pages: 320, paper: String::new(), hard: None }, yaw: 32.0, pitch: -6.0, shown: String::new(), job: None, tex: None, pics: None, out: None, pick: None, toast: None }
    }

    pub fn title(&self) -> String {
        "Capa".into()
    }

    pub fn dirty(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.dirty())
    }

    /// Pages as typeset (Diagramação, print) or estimated from the words.
    fn real_pages(&self, cx: &mut Cx, b: &Book, cfg: &Json) -> (usize, bool) {
        if let Some(n) = cx.sc.settings.data["pageCounts"][&b.path].as_u64() {
            return (n as usize, true);
        }
        let words = {
            let ix = cx.vault.index.read().unwrap();
            cx.sc.stats(&ix, b).words as i64
        };
        (capa::estimate_pages(words, cfg), false)
    }

    /// The model as shown: the simulation changes the paper and the board.
    fn shown_model(&self, m: &Json) -> Json {
        let mut m = m.clone();
        if self.sim.on {
            if !self.sim.paper.is_empty() {
                m["papel"] = json!(self.sim.paper);
            }
            if let Some(h) = self.sim.hard {
                m["capaDura"] = json!(h);
            }
        }
        m
    }

    fn render(&mut self, cx: &Cx, b: &Book, m: &Json, env: &Env, ppm: f32) {
        let key = format!("{ppm}{}{}", m, env.spine);
        if key == self.shown || self.job.is_some() {
            return;
        }
        self.shown = key.clone();
        let (tx, rx) = mpsc::channel();
        let (m, env, root, index, from) = (m.clone(), env.clone(), cx.vault.root.clone(), cx.vault.index.clone(), b.path.clone());
        let keys = compose::images(&m);
        let cached = self.pics.as_ref().filter(|(k, _)| *k == keys).map(|(_, p)| p.clone());
        let pics = cached.unwrap_or_else(|| {
            let ix = index.read().unwrap();
            Arc::new(CoverPics::load(&root, &ix, &from, &m))
        });
        self.pics = Some((keys, pics.clone()));
        std::thread::spawn(move || {
            let db = crate::fonts::book_fonts();
            let page = capa::page(&m, &env, &db, &pics);
            let (w, h, px) = raster::render(&page, &db, &pics.pics.list, ppm);
            let _ = tx.send((key, egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px)));
        });
        self.job = Some(rx);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let author = cx.sc.author();
        let b = {
            let ix = cx.vault.index.read().unwrap();
            book::book_by_path(&ix, &self.book, &author)
        };
        let Some(b) = b else {
            ui.label("Livro não encontrado.");
            return acts;
        };
        let cfg = book::layout(&b);
        let (real, exact) = self.real_pages(cx, &b, &cfg);
        if self.model.is_none() {
            self.model = Some(capa::load_or_default(&cx.vault.root, &b));
        }
        if let Some(rx) = &self.out {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
            if let Ok(r) = rx.try_recv() {
                self.out = None;
                match r {
                    Ok(path) => {
                        let rel = path.strip_prefix(&cx.vault.root).map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                        self.toast = Some(format!("Salvo em {rel}"));
                        acts.push(Action::Saved(rel));
                    }
                    Err(e) => self.toast = Some(format!("Não foi possível exportar: {e}")),
                }
            }
        }
        // editing
        if let Some(ed) = self.edit.as_mut() {
            match ed.ui(ui, cx) {
                Some(Event::Save(m)) => {
                    let saved = {
                        let ix = cx.vault.index.read().unwrap();
                        capa::save(&cx.vault.root, &ix, &b, &m)
                    };
                    match saved {
                        Ok(rel) => {
                            acts.push(Action::Saved(rel));
                            ed.mark_saved();
                            self.model = Some(m);
                            self.toast = Some("Capa salva.".into());
                        }
                        Err(e) => self.toast = Some(format!("Não consegui salvar: {e}")),
                    }
                }
                Some(Event::Close) => self.edit = None,
                None => {}
            }
            if let Some(t) = self.toast.take() {
                acts.push(Action::Toast(t));
            }
            return acts;
        }
        let m0 = self.model.clone().unwrap_or_default();
        let m = self.shown_model(&m0);
        let pages = if self.sim.on { self.sim.pages } else { real };
        let env = compose::cover_env(&m, &cfg, pages);
        if let Some(rx) = &self.job {
            ui.ctx().request_repaint_after(Duration::from_millis(60));
            if let Ok((_, img)) = rx.try_recv() {
                self.job = None;
                self.tex = Some(ui.ctx().load_texture("capa", img, egui::TextureOptions::LINEAR));
            }
        }
        egui::TopBottomPanel::top(ui.id().with("capa-bar")).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 16, right: 14, top: 10, bottom: 8 })).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let room = (ui.available_width() - 420.0).max(160.0);
                ui.allocate_ui_with_layout(vec2(room, 40.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new("Capa").font(kit::bold(17.0)).color(p.text));
                    let sp = format!("{:.1}", env.spine).replace('.', ",");
                    let kick = if self.sim.on {
                        format!("Simulando {pages} páginas · lombada {sp} mm · o PDF da capa usa as {real} reais")
                    } else {
                        format!("{} · capa aberta · lombada {sp} mm · {pages} páginas{}", b.title, if exact { "" } else { " (estimativa: diagrame para o número exato)" })
                    };
                    ui.add(egui::Label::new(RichText::new(kick).size(11.5).color(p.faint)).truncate());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let busy = self.out.is_some();
                    if ui.add_enabled_ui(!busy, |ui| kit::button(ui, Some(icons::IMAGE_DOWN), "PNG", p)).inner.on_hover_text("A frente da capa em PNG").clicked() {
                        self.export(cx, &b, &m0, real, false);
                    }
                    if ui.add_enabled_ui(!busy, |ui| kit::button(ui, Some(icons::FILE_DOWN), "PDF", p)).inner.on_hover_text("A capa aberta em PDF, com sangria").clicked() {
                        self.export(cx, &b, &m0, real, true);
                    }
                    if kit::button(ui, Some(icons::IMAGE), "Trocar arte", p).clicked() {
                        self.pick = Some(String::new());
                    }
                    if kit::primary_icon(ui, icons::PENCIL, "Editar capa", p).clicked() {
                        let back = std::fs::read_to_string(cx.vault.root.join(&b.path)).map(|t| ateliet_core::scriptorium::body_section(&t, "Contracapa")).unwrap_or_default();
                        self.edit = Some(Compositor::new(m0.clone(), true, cfg.clone(), real, &b.path, &format!("Capa — {}", b.title)).with_back_text(back.trim()));
                    }
                    if kit::icon_button(ui, icons::RULER, "Simular espessura: outro número de páginas, outro papel", self.sim.on, p).clicked() {
                        self.sim.on = !self.sim.on;
                        if self.sim.on {
                            self.sim.pages = real.max(320);
                        }
                    }
                });
            });
        });
        if self.sim.on {
            egui::TopBottomPanel::top(ui.id().with("capa-sim")).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 16, right: 14, top: 2, bottom: 10 })).show_inside(ui, |ui| self.sim_panel(ui, p, &b, &cfg, &m0, real));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg_secondary)).show_inside(ui, |ui| {
            let avail = ui.available_rect_before_wrap();
            let b2 = env.bleed;
            let (tw, th) = (env.w + 2.0 * b2, env.h + 2.0 * b2);
            // the open cover below, the book above
            let flat_h = (avail.height() * 0.42).max(160.0);
            let z = ((avail.width() - 80.0) / tw).min((flat_h - 40.0) / th).max(0.2);
            let ppp = ui.ctx().pixels_per_point();
            let ppm = ((z * ppp * 1.6).clamp(1.5, 6.0) * 4.0).round() / 4.0;
            self.render(cx, &b, &m, &env, ppm);
            let stage = Rect::from_min_max(avail.min, pos2(avail.right(), avail.bottom() - flat_h));
            let resp = ui.interact(stage, ui.id().with("capa-3d"), Sense::drag());
            if resp.dragged() {
                let d = resp.drag_delta();
                self.yaw += d.x * 0.5;
                self.pitch = (self.pitch - d.y * 0.3).clamp(-60.0, 60.0);
            }
            if let Some(t) = &self.tex {
                self.book3d(ui.painter_at(stage), stage, t.id(), &env, p);
            }
            ui.painter().text(pos2(stage.center().x, stage.bottom() - 12.0), egui::Align2::CENTER_CENTER, "Arraste para girar", egui::FontId::proportional(11.0), p.faint);
            let flat = Rect::from_center_size(pos2(avail.center().x, avail.bottom() - flat_h / 2.0 - 8.0), vec2(tw * z, th * z));
            let painter = ui.painter_at(avail);
            painter.rect_filled(flat.translate(vec2(0.0, 3.0)), 2.0, Color32::from_black_alpha(if p.dark { 90 } else { 30 }));
            if let Some(t) = &self.tex {
                painter.image(t.id(), flat, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            guides(&painter, flat, &env, z);
            painter.text(pos2(flat.center().x, flat.bottom() + 16.0), egui::Align2::CENTER_CENTER, "Azul: corte · vermelho: sangria · verde: dobras · amarelo: área de segurança", egui::FontId::proportional(11.0), p.faint);
        });
        if self.pick.is_some() {
            self.pick_dialog(ui.ctx(), p, cx, &b, &mut acts);
        }
        if let Some(t) = self.toast.take() {
            acts.push(Action::Toast(t));
        }
        acts
    }

    fn export(&mut self, cx: &Cx, b: &Book, m: &Json, pages: usize, pdf: bool) {
        let (tx, rx) = mpsc::channel();
        let (root, index, b, m) = (cx.vault.root.clone(), cx.vault.index.clone(), b.clone(), m.clone());
        std::thread::spawn(move || {
            let db = crate::fonts::book_fonts();
            let ix = index.read().unwrap();
            let r = if pdf { capa::cover_pdf(&root, &ix, &b, &m, &db, pages) } else { capa::front_png(&root, &ix, &b, &m, &db, pages, 1800) };
            let _ = tx.send(r);
        });
        self.out = Some(rx);
        self.toast = Some(if pdf { "Gerando a capa aberta em PDF…" } else { "Gerando a frente em PNG…" }.into());
    }

    fn sim_panel(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, cfg: &Json, m0: &Json, real: usize) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(RichText::new("Páginas").size(12.5).color(p.muted));
            let mut v = self.sim.pages as f32;
            if ui.add(egui::Slider::new(&mut v, 16.0..=1200.0).step_by(4.0).show_value(true)).changed() {
                self.sim.pages = v as usize;
            }
            ui.label(RichText::new("Papel").size(12.5).color(p.muted));
            let book_paper = PAPERS.iter().find(|x| Some(x.0) == m0["papel"].as_str().filter(|s| !s.is_empty()).or(cfg["papel"].as_str())).map(|x| x.1).unwrap_or("—");
            let cur = self.sim.paper.clone();
            egui::ComboBox::from_id_salt("capa-sim-paper").width(170.0).selected_text(if cur.is_empty() { format!("Do livro ({book_paper})") } else { PAPERS.iter().find(|x| x.0 == cur).map(|x| x.1.to_string()).unwrap_or(cur.clone()) }).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_empty(), format!("Do livro ({book_paper})")).clicked() {
                    self.sim.paper.clear();
                }
                for (k, l, _) in PAPERS.iter() {
                    if ui.selectable_label(cur == *k, *l).clicked() {
                        self.sim.paper = k.to_string();
                    }
                }
            });
            let mut hard = self.sim.hard.unwrap_or(m0["capaDura"].as_bool().unwrap_or(false));
            if kit::toggle(ui, &mut hard, p).changed() {
                self.sim.hard = Some(hard);
            }
            ui.label(RichText::new("Capa dura").size(12.5).color(p.muted));
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if kit::chip(ui, None, &format!("Agora · {real} p."), self.sim.pages == real, p).on_hover_text("O número atual de páginas").clicked() {
                self.sim.pages = real;
            }
            if b.goal > 0 {
                let per = capa::estimate_pages(b.goal, cfg);
                let proj = per.div_ceil(2) * 2;
                if kit::chip(ui, None, &format!("Meta · {proj} p."), self.sim.pages == proj, p).on_hover_text(format!("Projeção pela meta de {} palavras", b.goal)).clicked() {
                    self.sim.pages = proj;
                }
            }
            for v in [200, 350, 500, 800] {
                if kit::chip(ui, None, &format!("{v} p."), self.sim.pages == v, p).clicked() {
                    self.sim.pages = v;
                }
            }
        });
        if m0["lombadaAuto"] == false {
            ui.label(RichText::new("A lombada desta capa está fixa em mm (Editar capa › Lombada automática). A simulação só muda a espessura quando ela é automática.").size(11.5).color(p.faint));
        }
    }

    /// The book as a box: front, back and spine from the open cover; the
    /// pages' edges in paper colour.
    fn book3d(&self, painter: egui::Painter, stage: Rect, tex: egui::TextureId, env: &Env, p: Palette) {
        let (w, h, d) = (env.page_w, env.h, env.spine.max(1.0));
        let k = ((stage.width() * 0.55) / w.max(d)).min((stage.height() * 0.78) / h);
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        let focal = 4.0 * w.max(h);
        let c = stage.center() - vec2(0.0, 6.0);
        let proj = |x: f32, y: f32, z: f32| -> (Pos2, f32) {
            // turn about the vertical axis, then tilt
            let (x1, z1) = (x * yaw.cos() + z * yaw.sin(), -x * yaw.sin() + z * yaw.cos());
            let (y2, z2) = (y * pitch.cos() - z1 * pitch.sin(), y * pitch.sin() + z1 * pitch.cos());
            let s = focal / (focal - z2);
            (pos2(c.x + x1 * s * k, c.y + y2 * s * k), z2)
        };
        let b = env.bleed;
        let (tw, th) = (env.w + 2.0 * b, env.h + 2.0 * b);
        let uv = |name: &str| {
            let zn = env.zone(name);
            ((zn.x + b) / tw, b / th, (zn.x + zn.w + b) / tw, (h + b) / th)
        };
        let (hw, hh, hd) = (w / 2.0, h / 2.0, d / 2.0);
        // corners: top-left, top-right, bottom-right, bottom-left as seen from outside
        let faces: [([(f32, f32, f32); 4], Option<(f32, f32, f32, f32)>, Color32); 6] = [
            ([(-hw, -hh, hd), (hw, -hh, hd), (hw, hh, hd), (-hw, hh, hd)], Some(uv("capa")), Color32::WHITE),
            ([(hw, -hh, -hd), (-hw, -hh, -hd), (-hw, hh, -hd), (hw, hh, -hd)], Some(uv("contracapa")), Color32::WHITE),
            ([(-hw, -hh, -hd), (-hw, -hh, hd), (-hw, hh, hd), (-hw, hh, -hd)], Some(uv("lombada")), Color32::WHITE),
            ([(hw - 1.0, -hh + 1.5, hd), (hw - 1.0, -hh + 1.5, -hd), (hw - 1.0, hh - 1.5, -hd), (hw - 1.0, hh - 1.5, hd)], None, Color32::from_rgb(0xf3, 0xec, 0xda)),
            ([(-hw, -hh + 1.0, -hd + 0.4), (hw - 1.5, -hh + 1.0, -hd + 0.4), (hw - 1.5, -hh + 1.0, hd - 0.4), (-hw, -hh + 1.0, hd - 0.4)], None, Color32::from_rgb(0xec, 0xe4, 0xd0)),
            ([(-hw, hh - 1.0, hd - 0.4), (hw - 1.5, hh - 1.0, hd - 0.4), (hw - 1.5, hh - 1.0, -hd + 0.4), (-hw, hh - 1.0, -hd + 0.4)], None, Color32::from_rgb(0xe2, 0xd9, 0xc2)),
        ];
        // a soft shadow under the book
        let base = proj(0.0, hh, 0.0).0;
        painter.add(egui::Shape::ellipse_filled(base + vec2(0.0, 6.0), vec2(w * k * 0.62, 10.0), Color32::from_black_alpha(if p.dark { 70 } else { 28 })));
        let mut order: Vec<(f32, usize)> = Vec::new();
        for (i, f) in faces.iter().enumerate() {
            let q: Vec<(Pos2, f32)> = f.0.iter().map(|v| proj(v.0, v.1, v.2)).collect();
            let area: f32 = (0..4).map(|j| {
                let (a, b) = (q[j].0, q[(j + 1) % 4].0);
                a.x * b.y - b.x * a.y
            }).sum();
            if area <= 0.0 {
                continue;
            }
            order.push((q.iter().map(|x| x.1).sum::<f32>() / 4.0, i));
        }
        order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, i) in order {
            let (corners, uvs, col) = &faces[i];
            let n = if uvs.is_some() { 8 } else { 1 };
            let mut mesh = egui::Mesh::with_texture(if uvs.is_some() { tex } else { egui::TextureId::default() });
            let lerp3 = |a: (f32, f32, f32), b: (f32, f32, f32), t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
            for r in 0..=n {
                for cc in 0..=n {
                    let (u, v) = (cc as f32 / n as f32, r as f32 / n as f32);
                    let top = lerp3(corners[0], corners[1], u);
                    let bot = lerp3(corners[3], corners[2], u);
                    let pt = lerp3(top, bot, v);
                    let (pos, _) = proj(pt.0, pt.1, pt.2);
                    let t = match uvs {
                        Some((u0, v0, u1, v1)) => pos2(u0 + (u1 - u0) * u, v0 + (v1 - v0) * v),
                        None => egui::epaint::WHITE_UV,
                    };
                    mesh.vertices.push(egui::epaint::Vertex { pos, uv: t, color: *col });
                }
            }
            for r in 0..n {
                for cc in 0..n {
                    let i0 = (r * (n + 1) + cc) as u32;
                    let i1 = i0 + 1;
                    let i2 = i0 + (n + 1) as u32;
                    let i3 = i2 + 1;
                    mesh.indices.extend_from_slice(&[i0, i1, i3, i0, i3, i2]);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
            if uvs.is_none() && i == 3 {
                // the pages' edge: fine lines
                let q: Vec<Pos2> = corners.iter().map(|v| proj(v.0, v.1, v.2).0).collect();
                for j in 1..8 {
                    let t = j as f32 / 8.0;
                    let a = q[0] + (q[1] - q[0]) * t;
                    let b2 = q[3] + (q[2] - q[3]) * t;
                    painter.line_segment([a, b2], Stroke::new(0.5_f32, Color32::from_black_alpha(14)));
                }
            }
        }
    }

    fn pick_dialog(&mut self, ctx: &egui::Context, p: Palette, cx: &Cx, b: &Book, acts: &mut Vec<Action>) {
        let Some(filter) = self.pick.as_mut() else { return };
        let ix = cx.vault.index.read().unwrap();
        let mut items: Vec<String> = ix.file_paths().filter(|f| compositor::is_art(f)).cloned().collect();
        drop(ix);
        items.sort();
        let fl = filter.to_lowercase();
        items.retain(|x| fl.is_empty() || x.to_lowercase().contains(&fl));
        let mut chosen = None;
        let mut close = false;
        kit::scrim(ctx, p);
        egui::Window::new("capa-pick").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Imagem da capa (frente)").font(kit::medium(16.0)).color(p.text));
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
            acts.push(Action::SetFm(b.path.clone(), "capa".into(), Some(json!(format!("[[{c}]]")))));
            // the front's art layer follows, in the saved cover too
            if let Some(m) = self.model.as_mut() {
                if let Some(a) = m["camadas"].as_array_mut() {
                    let i = a.iter().position(|l| l["id"] == "arte-capa").or_else(|| a.iter().position(|l| l["t"] == "imagem" && l["zona"].as_str().unwrap_or("capa") == "capa"));
                    if let Some(i) = i {
                        a[i]["img"] = json!(c);
                    }
                }
                if capa::load(&cx.vault.root, b).is_some() {
                    let ix = cx.vault.index.read().unwrap();
                    if let Ok(rel) = capa::save(&cx.vault.root, &ix, b, m) {
                        acts.push(Action::Saved(rel));
                    }
                }
            }
        } else if close {
            self.pick = None;
        }
    }
}

/// Trim, bleed, folds and safe areas over the open cover.
fn guides(painter: &egui::Painter, flat: Rect, env: &Env, z: f32) {
    let b = env.bleed;
    let to = |x: f32, y: f32| pos2(flat.left() + (x + b) * z, flat.top() + (y + b) * z);
    let blue = Color32::from_rgb(0x00, 0xa3, 0xff);
    let green = Color32::from_rgb(0x00, 0xc8, 0x53);
    let amber = Color32::from_rgba_unmultiplied(0xff, 0xb3, 0x00, 200);
    let red = Color32::from_rgb(0xe5, 0x48, 0x4d);
    let dashed = |a: Pos2, c: Pos2, col: Color32| {
        painter.extend(egui::Shape::dashed_line(&[a, c], Stroke::new(1.0_f32, col), 4.0, 3.0));
    };
    if b > 0.0 {
        let r = flat;
        for (a, c) in [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())] {
            dashed(a, c, red);
        }
    }
    painter.rect_stroke(Rect::from_min_max(to(0.0, 0.0), to(env.w, env.h)), 0.0, Stroke::new(1.0_f32, blue), egui::StrokeKind::Middle);
    for (name, zn) in &env.zones {
        if name == "pagina" || name == "tudo" {
            continue;
        }
        if zn.x > 0.01 {
            dashed(to(zn.x, -b), to(zn.x, env.h + b), green);
        }
        let safe = 5.0;
        if zn.w > 2.0 * safe {
            let r = Rect::from_min_max(to(zn.x + safe, safe), to(zn.x + zn.w - safe, env.h - safe));
            for (a, c) in [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())] {
                dashed(a, c, amber);
            }
        }
    }
}
