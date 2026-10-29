//! Storyboards brought into a notebook: the panel with each storyboard's
//! frames (drawn small in the background), a frame dragged onto a page, a
//! whole storyboard laid out on new pages, and a frame drawn again when the
//! storyboard changed. A frame is saved as a picture of the vault, so the
//! page reads as a note in Obsidian too; the item remembers where it came
//! from (`storyboard`, `quadro`).

use super::{BoardThumbs, Book, Cx, Journal, Out, Payload};
use crate::icons;
use crate::kit;
use ateliet_core::journal::storyboard::{self, Frame};
use ateliet_core::journal::{Item, Kind};
use eframe::egui::{self, vec2, Color32, RichText, Sense, Stroke};
use serde_json::json;

/// Where frames are saved: `Storyboards/` in the attachment folder.
fn folder(cx: &Cx) -> String {
    if cx.attach.is_empty() {
        "Storyboards".into()
    } else {
        format!("{}/Storyboards", cx.attach)
    }
}

fn write_png(cx: &Cx, board: &str, f: &Frame, img: &image::RgbaImage) -> Result<String, String> {
    let rel = format!("{}/{}", folder(cx), storyboard::file_name(board, f));
    let abs = cx.vault.abs(&rel);
    if let Some(d) = abs.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    img.save(&abs).map_err(|e| e.to_string())?;
    Ok(rel)
}

/// One frame drawn and saved: its picture's path, the frame, its size.
pub fn save_frame(cx: &mut Cx, board: &str, id: &str, index: usize) -> Result<(String, Frame, (u32, u32)), String> {
    let (f, img) = storyboard::render(&cx.vault.abs(board), id, index, 1600)?;
    let rel = write_png(cx, board, &f, &img)?;
    cx.caches.images.remove(&rel);
    Ok((rel, f, img.dimensions()))
}

/// A picture item of a frame, remembering its storyboard.
pub fn frame_item(rel: &str, board: &str, f: &Frame, x: f64, y: f64, w: f64, h: f64) -> Item {
    let mut it = Item::new(Kind::Image, x, y, w, h);
    it.set_picture(rel, "");
    it.extra.insert("storyboard".into(), json!(board));
    it.extra.insert("quadro".into(), json!(f.id));
    it
}

impl Journal {
    pub fn boards_panel(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let p = cx.palette;
        ui.label(RichText::new("Abra um storyboard e arraste um quadro para a página, ou traga todos para páginas novas.").size(11.5).color(p.faint));
        ui.add_space(6.0);
        let mut list: Vec<(String, u64)> = cx.vault.files.iter().filter(|e| !e.folder && e.path.to_lowercase().ends_with(".storyboard")).map(|e| (e.path.clone(), e.mtime)).collect();
        list.sort_by_key(|b| std::cmp::Reverse(b.1));
        if list.is_empty() {
            ui.label(RichText::new("Nenhum storyboard no cofre.").size(12.5).color(p.faint));
            return;
        }
        let mut import: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("journal-boards").auto_shrink([false, false]).show(ui, |ui| {
            for (path, mtime) in &list {
                let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".storyboard");
                let open = self.open_board.as_deref() == Some(path.as_str());
                let r = kit::Row::new(name).icon(icons::CLAPPERBOARD).chevron(open).selected(open).show(ui, p);
                if r.clicked() {
                    self.open_board = if open { None } else { Some(path.clone()) };
                }
                if !open {
                    continue;
                }
                self.load_thumbs(ui.ctx(), cx, path, *mtime);
                ui.horizontal(|ui| {
                    if kit::quiet(ui, Some(icons::FILE_PLUS), "Trazer todos para páginas novas", p).clicked() {
                        import = Some(path.clone());
                    }
                    if kit::quiet(ui, Some(icons::EXTERNAL_LINK), "Abrir", p).clicked() {
                        self.out.push(Out::Open(path.clone()));
                    }
                });
                match self.boards.get(path).map(|b| &b.1) {
                    Some(BoardThumbs::Loading(_)) | None => {
                        ui.label(RichText::new("Desenhando os quadros…").size(12.0).color(p.faint));
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
                    }
                    Some(BoardThumbs::Failed(e)) => {
                        ui.label(RichText::new(e).size(12.0).color(p.error));
                    }
                    Some(BoardThumbs::Ready(frames)) => {
                        let cell = ((ui.available_width() - 8.0) / 2.0).floor();
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                            for (f, tex) in frames {
                                let s = tex.size_vec2();
                                let h = (cell * s.y / s.x.max(1.0)).min(cell * 1.5);
                                let (rect, resp) = ui.allocate_exact_size(vec2(cell, h + 16.0), Sense::click_and_drag());
                                let pic = egui::Rect::from_min_size(rect.min, vec2(cell, h));
                                ui.painter().image(tex.id(), pic, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                                ui.painter().rect_stroke(pic, 2.0, Stroke::new(1.0_f32, if resp.hovered() { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
                                let label = if f.name.trim().is_empty() { format!("{}", f.index + 1) } else { format!("{} · {}", f.index + 1, f.name.trim()) };
                                ui.painter().with_clip_rect(rect).text(egui::pos2(rect.left(), pic.bottom() + 3.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(11.0), p.muted);
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                }
                                if resp.drag_started() {
                                    resp.dnd_set_drag_payload(Payload::Frame(path.clone(), f.id.clone(), f.index));
                                }
                            }
                        });
                    }
                }
                ui.add_space(10.0);
            }
        });
        if let Some(path) = import {
            if let Some(mut b) = self.book.take() {
                self.import_board(ui.ctx(), &mut b, cx, &path);
                self.book = Some(b);
            }
        }
    }

    /// The frames of a storyboard, drawn small once per change of the file.
    fn load_thumbs(&mut self, ctx: &egui::Context, cx: &Cx, path: &str, mtime: u64) {
        if self.boards.get(path).is_some_and(|(m, _)| *m == mtime) {
            if let Some((_, BoardThumbs::Loading(rx))) = self.boards.get(path) {
                if let Ok(res) = rx.try_recv() {
                    let st = match res {
                        Ok(list) => BoardThumbs::Ready(list.into_iter().map(|(f, img)| {
                            let t = ctx.load_texture(format!("journal-board:{path}:{}", f.id), img, egui::TextureOptions::LINEAR);
                            (f, t)
                        }).collect()),
                        Err(e) => BoardThumbs::Failed(e),
                    };
                    self.boards.insert(path.to_string(), (mtime, st));
                }
            }
            return;
        }
        let abs = cx.vault.abs(path);
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let res = storyboard::render_all(&abs, 320).map(|list| list.into_iter().map(|(f, img)| {
                let size = [img.width() as usize, img.height() as usize];
                (f, egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
            }).collect());
            let _ = tx.send(res);
            repaint.request_repaint();
        });
        self.boards.insert(path.to_string(), (mtime, BoardThumbs::Loading(rx)));
    }

    /// A whole storyboard on new pages after the one shown: two frames a
    /// page, down the main column, each with its number, name, action and
    /// dialogue under it.
    pub fn import_board(&mut self, ctx: &egui::Context, b: &mut Book, cx: &mut Cx, path: &str) {
        let frames = match storyboard::render_all(&cx.vault.abs(path), 1600) {
            Ok(f) if !f.is_empty() => f,
            Ok(_) => return,
            Err(e) => {
                self.out.push(Out::Toast(format!("Não foi possível abrir o storyboard: {e}")));
                return;
            }
        };
        let total = frames.len();
        for pair in frames.chunks(2) {
            b.new_page_here(&mut self.out);
            let (l, r) = b.spread_pages();
            let n = r.or(l).unwrap_or(1);
            let lay = b.layout(n);
            let s = lay.paper.line();
            let (c0, c1) = lay.main_column();
            let top = lay.first_baseline() - s;
            let room = (lay.last_baseline() - top) / 2.0;
            for (k, (f, img)) in pair.iter().enumerate() {
                let Ok(rel) = write_png(cx, path, f, img) else { continue };
                cx.caches.images.remove(&rel);
                let (w, h) = img.dimensions();
                let max_h = room - 3.0 * s;
                let mut fw = c1 - c0;
                let mut fh = fw * h as f64 / w.max(1) as f64;
                if fh > max_h {
                    fh = max_h;
                    fw = fh * w as f64 / h.max(1) as f64;
                }
                let y = top + k as f64 * room;
                let pic = frame_item(&rel, path, f, c0, y, fw, fh);
                let mut cap = Item::new(Kind::Text, c0, lay.text_top(y + fh + 0.5, 1.0), c1 - c0, s);
                cap.anchor = Some(pic.id.clone());
                cap.style.font = b.nb.font.clone().unwrap_or_default();
                cap.set_content(&f.caption());
                // the caption as tall as its lines (a few: it does not flow)
                let rows = super::draw::row_starts(ctx, &cap, cap.content(), &lay, 4.0).len().clamp(1, 3);
                cap.h = rows as f64 * s;
                if let Some(pg) = b.page(n) {
                    pg.items.push(pic);
                    pg.items.push(cap);
                }
                b.touch(n);
            }
        }
        self.out.push(Out::Rescan);
        self.out.push(Out::Toast(format!("{total} {} do storyboard em páginas novas.", if total == 1 { "quadro" } else { "quadros" })));
    }

    /// A frame drawn again from its storyboard, over its picture.
    pub fn refresh_frame(&mut self, ctx: &egui::Context, cx: &mut Cx, it: &Item) {
        let (Some(board), Some(id)) = (it.extra.get("storyboard").and_then(|v| v.as_str()), it.extra.get("quadro").and_then(|v| v.as_str())) else { return };
        let Some(target) = it.picture() else { return };
        let res = storyboard::render(&cx.vault.abs(board), id, 0, 1600).and_then(|(_, img)| img.save(cx.vault.abs(&target)).map_err(|e| e.to_string()));
        match res {
            Ok(()) => {
                cx.caches.images.remove(&target);
                self.filtered.retain(|(p, _), _| *p != target);
                ctx.request_repaint();
                self.out.push(Out::Toast("Quadro desenhado de novo do storyboard.".into()));
            }
            Err(e) => self.out.push(Out::Toast(format!("Não foi possível atualizar o quadro: {e}"))),
        }
    }
}
