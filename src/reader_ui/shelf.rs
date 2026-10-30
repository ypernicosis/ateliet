//! The shelf: the first screen when no book is open. The books of the
//! vault (EPUB, PDF, CBZ, CBR, CB7) as covers, "Continuar lendo" on top,
//! a filter by format and by name. No numbers nor charts: the
//! Scriptorium's Library is the place for numbers.

use super::Cx;
use crate::{icons, kit};
use ateliet_core::reader::state::State;
use ateliet_core::reader::{format_of, Format, Locator};
use eframe::egui::{self, vec2, Color32, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Default)]
pub struct Shelf {
    query: String,
    filter: Option<Format>,
    covers: HashMap<String, Option<egui::TextureHandle>>,
    asked: HashMap<String, ()>,
    tx: Option<Sender<(String, Option<egui::ColorImage>)>>,
    rx: Option<Receiver<(String, Option<egui::ColorImage>)>>,
    running: usize,
    queue: Vec<String>,
    pub close_side: bool,
}

impl Shelf {
    fn cover(&mut self, ctx: &egui::Context, cx: &Cx, file: &str) -> Option<egui::TextureHandle> {
        if let Some(c) = self.covers.get(file) {
            return c.clone();
        }
        if self.asked.insert(file.to_string(), ()).is_none() {
            self.queue.push(file.to_string());
        }
        if self.tx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.tx = Some(tx);
            self.rx = Some(rx);
        }
        // two covers at a time, the last asked first (the ones on screen)
        while self.running < 2 {
            let Some(f) = self.queue.pop() else { break };
            self.running += 1;
            let tx = self.tx.clone().unwrap();
            let abs = cx.vault.abs(&f);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let img = ateliet_core::reader::open(&abs, crate::fonts::book_fonts()).ok().and_then(|mut d| d.cover(240)).map(|r| egui::ColorImage::from_rgba_unmultiplied([r.w as usize, r.h as usize], &r.px));
                let _ = tx.send((f, img));
                ctx.request_repaint();
            });
        }
        None
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else { return };
        while let Ok((f, img)) = rx.try_recv() {
            self.running = self.running.saturating_sub(1);
            let t = img.map(|i| ctx.load_texture(format!("leitor-capa:{f}"), i, egui::TextureOptions::LINEAR));
            self.covers.insert(f, t);
        }
    }

    /// The shelf; the book chosen (and where to open it).
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &Cx, state: &mut State, side: bool) -> Option<(String, Option<Locator>)> {
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let mut chosen = None;
        let mut books: Vec<(String, u64)> = cx.vault.files.iter().filter(|e| !e.folder && format_of(&e.path).is_some()).map(|e| (e.path.clone(), e.mtime)).collect();
        books.sort_by_key(|a| a.0.to_lowercase());
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, p.bg);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 32, right: 32, top: 26, bottom: 30 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Estante").font(kit::medium(22.0)).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if side && kit::icon_button(ui, icons::X, "Fechar o leitor ao lado", false, p).clicked() {
                            self.close_side = true;
                        }
                        ui.add_sized(vec2(200.0, 26.0), |ui: &mut egui::Ui| kit::search_field(ui, &mut self.query, "Buscar livros", p));
                    });
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Os livros em EPUB, PDF e quadrinhos (CBZ, CBR, CB7) do vault. Clique num livro para lê-lo aqui.").size(12.5).color(p.faint));
                ui.add_space(10.0);
                // continue reading
                let recent: Vec<(String, f32)> = state.recent().into_iter().filter(|(_, b)| books.iter().any(|(f, _)| *f == b.file)).take(6).map(|(_, b)| (b.file.clone(), b.progress)).collect();
                if !recent.is_empty() && self.query.is_empty() && self.filter.is_none() {
                    kit::section(ui, "Continuar lendo", p);
                    ui.horizontal_wrapped(|ui| {
                        for (f, prog) in &recent {
                            if self.card(ui, cx, f, Some(*prog)) {
                                chosen = Some((f.clone(), None));
                            }
                        }
                    });
                    ui.add_space(14.0);
                }
                ui.horizontal(|ui| {
                    kit::section(ui, &format!("Todos · {}", books.len()), p);
                    ui.add_space(12.0);
                    for (f, label) in [(None, "Todos"), (Some(Format::Epub), "EPUB"), (Some(Format::Pdf), "PDF"), (Some(Format::Comic), "HQ")] {
                        if kit::chip(ui, None, label, self.filter == f, p).clicked() {
                            self.filter = f;
                        }
                    }
                });
                ui.add_space(6.0);
                if books.is_empty() {
                    ui.add_space(20.0);
                    ui.label(RichText::new("Nenhum livro no vault ainda. Copie um .epub, .pdf ou .cbz para qualquer pasta do vault.").size(13.0).color(p.muted));
                    return;
                }
                let q = ateliet_core::reader::fold(&self.query);
                ui.horizontal_wrapped(|ui| {
                    for (f, _) in &books {
                        if let Some(fmt) = self.filter {
                            if format_of(f) != Some(fmt) {
                                continue;
                            }
                        }
                        if !q.is_empty() && !ateliet_core::reader::fold(f).contains(&q) {
                            continue;
                        }
                        let prog = state.by_file(f).map(|x| x.1.progress);
                        if self.card(ui, cx, f, prog) {
                            chosen = Some((f.clone(), None));
                        }
                    }
                });
            });
        });
        chosen
    }

    /// A book's cover with its name; true when clicked.
    fn card(&mut self, ui: &mut egui::Ui, cx: &Cx, file: &str, progress: Option<f32>) -> bool {
        let p = cx.palette;
        let (w, h) = (128.0, 184.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h + 44.0), Sense::click());
        let cover = egui::Rect::from_min_size(rect.min, vec2(w, h));
        let ctx = ui.ctx().clone();
        let tex = if ui.is_rect_visible(rect) { self.cover(&ctx, cx, file) } else { None };
        let painter = ui.painter();
        match tex {
            Some(t) => {
                let s = t.size_vec2();
                let k = (w / s.x).min(h / s.y);
                let r = egui::Rect::from_center_size(cover.center(), s * k);
                let r = egui::Rect::from_min_size(egui::pos2(r.min.x, cover.bottom() - r.height()), r.size());
                painter.rect_filled(r.translate(vec2(0.0, 2.0)), 2.0, Color32::from_black_alpha(30));
                painter.image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                painter.rect_filled(cover, 3.0, p.bg_secondary);
                painter.rect_stroke(cover, 3.0, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                let name = crate::vault::basename(file).rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| crate::vault::basename(file).to_string());
                let g = painter.layout(name, egui::FontId::proportional(13.0), p.muted, w - 20.0);
                painter.galley(cover.center() - g.size() / 2.0, g, p.muted);
            }
        }
        if resp.hovered() {
            painter.rect_stroke(cover.expand(2.0), 4.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Outside);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let name = file.rsplit('/').next().unwrap_or(file);
        let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or(name);
        let ext = name.rsplit_once('.').map(|x| x.1.to_uppercase()).unwrap_or_default();
        let g = painter.layout(stem.to_string(), egui::FontId::proportional(12.0), p.text, w);
        let first_line = g.rows.first().map(|r| r.rect.height()).unwrap_or(14.0);
        painter.with_clip_rect(egui::Rect::from_min_size(egui::pos2(rect.left(), cover.bottom() + 6.0), vec2(w, first_line * 2.0 + 1.0))).galley(egui::pos2(rect.left(), cover.bottom() + 6.0), g, p.text);
        let foot = egui::pos2(rect.left(), rect.bottom() - 6.0);
        match progress.filter(|x| *x > 0.0) {
            Some(x) => {
                painter.hline(foot.x..=foot.x + w, foot.y, Stroke::new(2.0_f32, p.border));
                painter.hline(foot.x..=foot.x + w * x.clamp(0.0, 1.0), foot.y, Stroke::new(2.0_f32, p.accent_strong));
            }
            None => {
                painter.text(foot, egui::Align2::LEFT_CENTER, ext, egui::FontId::proportional(10.0), p.faint);
            }
        }
        resp.on_hover_text(file).clicked()
    }
}
