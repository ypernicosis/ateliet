//! Storyboards on a page, each as one thing: a span of its timeline as an
//! animation (an animated PNG, which Obsidian plays too, and the Journal
//! plays on the page) or one of its frames as a picture. The span is cut by
//! frame or by time in a small window with the storyboard playing in it. The
//! item remembers where it came from (`storyboard`, `de`, `ate`, `animado`),
//! so it can be cut again or drawn again when the storyboard changes.

use super::{BoardThumbs, Book, Cx, Journal, Out};
use crate::icons;
use crate::kit;
use ateliet_core::journal::storyboard::{self, Clip};
use ateliet_core::journal::{Item, Kind};
use eframe::egui::{self, vec2, Color32, RichText, Sense, Stroke};
use serde_json::json;

/// Where storyboards are saved as pictures: `Storyboards/` in the
/// attachment folder.
fn folder(cx: &Cx) -> String {
    if cx.attach.is_empty() {
        "Storyboards".into()
    } else {
        format!("{}/Storyboards", cx.attach)
    }
}

/// Where the cut goes.
#[derive(Clone, Debug)]
pub enum Target {
    /// A new item on a page, centred at a point.
    New(usize, (f64, f64)),
    /// An item already on a page, cut again.
    Edit(usize, String),
}

/// The window that cuts a storyboard.
pub struct BoardDialog {
    pub path: String,
    clip: Clip,
    pub animated: bool,
    pub from: usize,
    pub to: usize,
    pub target: Target,
    opened: f64,
}

/// What an item says of its storyboard: (file, first, last, animated).
pub fn board_of(it: &Item) -> Option<(String, usize, usize, bool)> {
    let b = it.extra.get("storyboard")?.as_str()?.to_string();
    let de = it.extra.get("de").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let ate = it.extra.get("ate").and_then(|v| v.as_u64()).unwrap_or(de as u64) as usize;
    let anim = it.extra.get("animado").and_then(|v| v.as_bool()).unwrap_or(false);
    Some((b, de, ate, anim))
}

/// A storyboard's animations being played: (file, first, last, modified) →
/// its frames, drawn in the background.
pub enum Anim {
    Loading(std::sync::mpsc::Receiver<Vec<(egui::ColorImage, f32)>>),
    Ready(Vec<(egui::TextureHandle, f32)>),
}

impl Journal {
    pub fn boards_panel(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let p = cx.palette;
        ui.label(RichText::new("Arraste um storyboard para a página (entra como animação), ou escolha o trecho e o jeito.").size(11.5).color(p.faint));
        ui.add_space(6.0);
        let mut list: Vec<(String, u64)> = cx.vault.files.iter().filter(|e| !e.folder && e.path.to_lowercase().ends_with(".storyboard")).map(|e| (e.path.clone(), e.mtime)).collect();
        list.sort_by_key(|b| std::cmp::Reverse(b.1));
        if list.is_empty() {
            ui.label(RichText::new("Nenhum storyboard no cofre.").size(12.5).color(p.faint));
            return;
        }
        let mut open: Option<(String, bool)> = None;
        egui::ScrollArea::vertical().id_salt("journal-boards").auto_shrink([false, false]).show(ui, |ui| {
            for (path, mtime) in &list {
                self.load_thumbs(ui.ctx(), cx, path, *mtime);
                let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".storyboard").to_string();
                egui::Frame::new().fill(p.bg).corner_radius(6.0).stroke(Stroke::new(1.0_f32, p.border)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, resp) = ui.allocate_exact_size(vec2(96.0, 60.0), Sense::click_and_drag());
                        ui.painter().rect_filled(r, 3.0, p.bg_hover);
                        let frames = match self.boards.get(path).map(|b| &b.1) {
                            Some(BoardThumbs::Ready(f)) => f.len(),
                            _ => 0,
                        };
                        if let Some((_, BoardThumbs::Ready(f))) = self.boards.get(path) {
                            // the poster plays while the pointer is on it
                            let i = if resp.hovered() && !f.is_empty() { ((ui.input(|i| i.time) * 4.0) as usize) % f.len() } else { 0 };
                            if let Some((_, t)) = f.get(i) {
                                let s = t.size_vec2();
                                let k = (r.width() / s.x).min(r.height() / s.y);
                                ui.painter().image(t.id(), egui::Rect::from_center_size(r.center(), s * k), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            if resp.hovered() {
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                            }
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if resp.drag_started() {
                            resp.dnd_set_drag_payload(super::Payload::Board(path.clone()));
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&name).font(kit::medium(12.5)).color(p.text));
                            if frames > 0 {
                                ui.label(RichText::new(format!("{frames} {}", if frames == 1 { "quadro" } else { "quadros" })).size(11.0).color(p.faint));
                            }
                            ui.horizontal(|ui| {
                                if kit::quiet(ui, Some(icons::FILM), "Animação…", p).clicked() {
                                    open = Some((path.clone(), true));
                                }
                                if kit::quiet(ui, Some(icons::IMAGE), "Imagem…", p).clicked() {
                                    open = Some((path.clone(), false));
                                }
                            });
                        });
                    });
                });
                ui.add_space(6.0);
            }
        });
        if let Some((path, animated)) = open {
            let target = self.book.as_mut().map(|b| {
                let (l, r) = b.spread_pages();
                let n = r.or(l).unwrap_or(1);
                let lay = b.layout(n);
                let (c0, c1) = lay.main_column();
                Target::New(n, ((c0 + c1) / 2.0, lay.h * 0.35))
            });
            if let Some(t) = target {
                self.open_board_dialog(ui.ctx(), cx, &path, animated, None, t);
            }
        }
    }

    /// The frames of a storyboard, drawn small once per change of the file
    /// (for the panel's posters and the window's preview).
    pub fn load_thumbs(&mut self, ctx: &egui::Context, cx: &Cx, path: &str, mtime: u64) {
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

    pub fn open_board_dialog(&mut self, ctx: &egui::Context, cx: &Cx, path: &str, animated: bool, span: Option<(usize, usize)>, target: Target) {
        match storyboard::clip(&cx.vault.abs(path)) {
            Ok(clip) if !clip.is_empty() => {
                let last = clip.len() - 1;
                let (from, to) = span.unwrap_or((0, if animated { last } else { 0 }));
                self.board_dlg = Some(BoardDialog { path: path.to_string(), clip, animated, from: from.min(last), to: to.min(last).max(from.min(last)), target, opened: ctx.input(|i| i.time) });
            }
            Ok(_) => self.out.push(Out::Toast("O storyboard não tem quadros.".into())),
            Err(e) => self.out.push(Out::Toast(format!("Não foi possível abrir o storyboard: {e}"))),
        }
    }

    /// The window: the storyboard playing the span chosen, the way it goes
    /// in (animation or picture), from which frame to which, or for how long.
    pub fn board_dialog_ui(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        let Some(mut d) = self.board_dlg.take() else { return };
        let p = cx.palette;
        let mtime = cx.vault.files.iter().find(|e| e.path == d.path).map(|e| e.mtime).unwrap_or(0);
        self.load_thumbs(ctx, cx, &d.path, mtime);
        let mut keep = true;
        let mut go = false;
        kit::scrim(ctx, p);
        let name = d.path.rsplit('/').next().unwrap_or(&d.path).trim_end_matches(".storyboard").to_string();
        egui::Window::new("journal-board-dialog").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).fixed_size(vec2(460.0, 0.0)).frame(egui::Frame::window(&ctx.style()).fill(p.bg).inner_margin(egui::Margin::same(18))).show(ctx, |ui| {
            ui.label(RichText::new(&name).font(kit::bold(16.0)).color(p.text));
            ui.add_space(10.0);
            // the preview, playing the span (or showing the frame)
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 230.0), Sense::hover());
            ui.painter().rect_filled(r, 4.0, p.bg_secondary);
            if let Some((_, BoardThumbs::Ready(frames))) = self.boards.get(&d.path) {
                let i = if d.animated {
                    let total = d.clip.span(d.from, d.to).max(0.05);
                    let t = ((ui.input(|i| i.time) - d.opened) as f32).rem_euclid(total);
                    let mut acc = 0.0;
                    let mut at = d.from;
                    for k in d.from..=d.to {
                        acc += d.clip.span(k, k);
                        at = k;
                        if t < acc {
                            break;
                        }
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    at
                } else {
                    d.from
                };
                if let Some((_, tex)) = frames.get(i) {
                    let s = tex.size_vec2();
                    let k = ((r.width() - 12.0) / s.x).min((r.height() - 12.0) / s.y);
                    ui.painter().image(tex.id(), egui::Rect::from_center_size(r.center(), s * k), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                ui.painter().text(r.right_bottom() + vec2(-8.0, -6.0), egui::Align2::RIGHT_BOTTOM, format!("{} / {}", i + 1, d.clip.len()), egui::FontId::proportional(11.0), p.faint);
            } else if let Some((_, BoardThumbs::Failed(e))) = self.boards.get(&d.path) {
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(12.0), p.error);
            } else {
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Desenhando os quadros…", egui::FontId::proportional(12.0), p.faint);
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            ui.add_space(12.0);
            if let Some(k) = kit::segmented(ui, &[(icons::FILM, "Animação"), (icons::IMAGE, "Imagem")], if d.animated { 0 } else { 1 }, p) {
                d.animated = k == 0;
                if !d.animated {
                    d.to = d.from;
                } else if d.to == d.from {
                    d.to = d.clip.len() - 1;
                }
            }
            ui.add_space(10.0);
            let n = d.clip.len();
            let fmt = |s: f32| format!("{:.1} s", s).replace('.', ",");
            if d.animated {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Do quadro").size(12.5).color(p.muted));
                    let mut a = d.from + 1;
                    if ui.add(egui::Slider::new(&mut a, 1..=n)).changed() {
                        d.from = a - 1;
                        d.to = d.to.max(d.from);
                    }
                    ui.label(RichText::new(format!("começa em {}", fmt(d.clip.start(d.from)))).size(11.5).color(p.faint));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Até o quadro").size(12.5).color(p.muted));
                    let mut b = d.to + 1;
                    if ui.add(egui::Slider::new(&mut b, (d.from + 1)..=n)).changed() {
                        d.to = b - 1;
                    }
                });
                // or by time: the last frame that starts within it
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Duração").size(12.5).color(p.muted));
                    let whole = d.clip.span(d.from, n - 1);
                    let mut dur = d.clip.span(d.from, d.to);
                    if ui.add(egui::Slider::new(&mut dur, 0.0..=whole.max(0.1)).custom_formatter(|v, _| fmt(v as f32))).changed() {
                        let end = d.clip.start(d.from) + dur;
                        d.to = (d.from..n).take_while(|&k| d.clip.start(k) < end - 1e-3).last().unwrap_or(d.from);
                    }
                });
                ui.label(RichText::new(format!("{} {}, {}", d.to - d.from + 1, if d.to == d.from { "quadro" } else { "quadros" }, fmt(d.clip.span(d.from, d.to)))).size(12.0).color(p.faint));
            } else {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Quadro").size(12.5).color(p.muted));
                    let mut a = d.from + 1;
                    if ui.add(egui::Slider::new(&mut a, 1..=n)).changed() {
                        d.from = a - 1;
                        d.to = d.from;
                    }
                });
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if matches!(d.target, Target::Edit(..)) { "Aplicar" } else { "Pôr na página" };
                    if kit::primary(ui, label, p).clicked() {
                        go = true;
                    }
                    if kit::button(ui, None, "Cancelar", p).clicked() {
                        keep = false;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                keep = false;
            }
        });
        if go {
            if let Some(mut b) = self.book.take() {
                self.place_board(&mut b, cx, &d.path, d.from, d.to, d.animated, d.target.clone());
                self.book = Some(b);
            }
            keep = false;
        }
        if keep {
            self.board_dlg = Some(d);
        }
    }

    /// Draw the span, save it as a picture of the vault, and put it on the
    /// page (or in place of the item it was cut from).
    #[allow(clippy::too_many_arguments)]
    pub fn place_board(&mut self, b: &mut Book, cx: &mut Cx, path: &str, from: usize, to: usize, animated: bool, target: Target) {
        let to = if animated { to } else { from };
        let frames = match storyboard::render_span(&cx.vault.abs(path), from, to, 1200) {
            Ok(f) if !f.is_empty() => f,
            Ok(_) => return,
            Err(e) => {
                self.out.push(Out::Toast(format!("Não foi possível desenhar o storyboard: {e}")));
                return;
            }
        };
        let rel = format!("{}/{}", folder(cx), storyboard::span_file_name(path, from, to, animated));
        let abs = cx.vault.abs(&rel);
        if let Some(d) = abs.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let res = if animated && frames.len() > 1 { std::fs::write(&abs, storyboard::apng(&frames)).map_err(|e| e.to_string()) } else { frames[0].0.save(&abs).map_err(|e| e.to_string()) };
        if let Err(e) = res {
            self.out.push(Out::Toast(format!("Não foi possível gravar a imagem: {e}")));
            return;
        }
        cx.caches.images.remove(&rel);
        self.filtered.retain(|(p, _), _| *p != rel);
        self.anims.retain(|k, _| !k.starts_with(&format!("{path}|")));
        self.out.push(Out::Rescan);
        let (w, h) = frames[0].0.dimensions();
        let aspect = h as f64 / w.max(1) as f64;
        let mark = |it: &mut Item| {
            it.set_picture(&rel, "");
            it.extra.insert("storyboard".into(), json!(path));
            it.extra.insert("de".into(), json!(from));
            it.extra.insert("ate".into(), json!(to));
            it.extra.insert("animado".into(), json!(animated && frames.len() > 1));
            it.extra.remove("quadro");
        };
        b.checkpoint();
        match target {
            Target::New(n, at) => {
                let lay = b.layout(n);
                let (c0, c1) = lay.main_column();
                let fw = (c1 - c0) * 0.85;
                let fh = fw * aspect;
                let mut it = Item::new(Kind::Image, at.0 - fw / 2.0, at.1 - fh / 2.0, fw, fh);
                mark(&mut it);
                let id = it.id.clone();
                if let Some(pg) = b.page(n) {
                    pg.items.push(it);
                }
                b.sel_page = n;
                b.sel = vec![id];
                b.touch(n);
            }
            Target::Edit(n, id) => {
                if let Some(it) = b.page(n).and_then(|pg| pg.item_mut(&id)) {
                    mark(it);
                    // the same width; the height of the new picture
                    it.h = it.w * aspect;
                    it.crop = None;
                }
                b.touch(n);
            }
        }
    }

    /// A storyboard item's picture now, when it is an animation that plays:
    /// its frames are drawn once in the background, then shown in time.
    pub fn anim_texture(&mut self, ctx: &egui::Context, cx: &Cx, it: &Item) -> Option<(egui::TextureId, egui::Vec2)> {
        let (board, from, to, animated) = board_of(it)?;
        if !animated || it.extra.get("pausado").and_then(|v| v.as_bool()).unwrap_or(false) {
            return None;
        }
        let mtime = cx.vault.files.iter().find(|e| e.path == board).map(|e| e.mtime).unwrap_or(0);
        let key = format!("{board}|{from}|{to}|{mtime}");
        if !self.anims.contains_key(&key) {
            let abs = cx.vault.abs(&board);
            let (tx, rx) = std::sync::mpsc::channel();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let frames = storyboard::render_span(&abs, from, to, 800).unwrap_or_default();
                let out = frames.into_iter().map(|(img, d)| (egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), d)).collect();
                let _ = tx.send(out);
                repaint.request_repaint();
            });
            self.anims.insert(key.clone(), Anim::Loading(rx));
        }
        if let Some(Anim::Loading(rx)) = self.anims.get(&key) {
            if let Ok(frames) = rx.try_recv() {
                let ready = frames.into_iter().enumerate().map(|(i, (img, d))| (ctx.load_texture(format!("journal-anim:{key}:{i}"), img, egui::TextureOptions::LINEAR), d)).collect();
                self.anims.insert(key.clone(), Anim::Ready(ready));
            }
        }
        let Some(Anim::Ready(frames)) = self.anims.get(&key) else { return None };
        let total: f32 = frames.iter().map(|f| f.1).sum();
        if frames.is_empty() || total <= 0.0 {
            return None;
        }
        let t = (ctx.input(|i| i.time) as f32).rem_euclid(total);
        let mut acc = 0.0;
        for (tex, d) in frames {
            acc += d;
            if t < acc {
                ctx.request_repaint_after(std::time::Duration::from_secs_f32((acc - t).max(0.01)));
                return Some((tex.id(), tex.size_vec2()));
            }
        }
        frames.last().map(|(t, _)| (t.id(), t.size_vec2()))
    }

    /// A frame placed earlier (one frame of a storyboard), drawn again.
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
