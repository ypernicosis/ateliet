//! The storyboards screen: what can be made (each kind, a click away) and
//! the vault's storyboards as cards with their first frame, most recently
//! changed first, filtered by name or folder.

use crate::icons;
use crate::kit;
use crate::sc::library::Cx;
use crate::sc::{fmt_date, Action};
use crate::vault;
use ateliet_core::board::doc;
use ateliet_core::board::pages as pg;
use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

/// The tab's path.
pub const PATH: &str = "scriptorium:storyboards";

/// Files read at once in the background.
const READERS: usize = 4;

const CARD_W: f32 = 228.0;
const THUMB_H: f32 = 136.0;
const GAP: f32 = 18.0;

struct Info {
    kind: String,
    frames: usize,
    tex: Option<egui::TextureHandle>,
}

enum Slot {
    Loading,
    Ready(Info),
    Failed,
}

type Loaded = (String, u64, Result<(doc::Summary, Option<egui::ColorImage>), String>);

pub struct Home {
    filter: String,
    /// path → (modified, what was read of it)
    cards: HashMap<String, (u64, Slot)>,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
    busy: usize,
}

impl Default for Home {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Home { filter: String::new(), cards: HashMap::new(), tx, rx, busy: 0 }
    }
}

fn kind_icon(k: &str) -> &'static str {
    match k {
        "animatic" => icons::CLAPPERBOARD,
        "presentation" => icons::PRESENTATION,
        "thumbnails" => icons::LAYOUT_GRID,
        "name" => icons::BOOK_OPEN,
        "comic" => icons::BOOK_OPEN_TEXT,
        "webtoon" => icons::SMARTPHONE,
        _ => icons::INFINITY,
    }
}

fn name_of(path: &str) -> &str {
    vault::basename(path).trim_end_matches(".storyboard")
}

/// Reads one file's summary and decodes its preview, small.
fn load(abs: std::path::PathBuf) -> Result<(doc::Summary, Option<egui::ColorImage>), String> {
    let bytes = std::fs::read(&abs).map_err(|e| e.to_string())?;
    let mut s = doc::summary(&bytes)?;
    let img = s.preview.take().and_then(|png| image::load_from_memory(&png).ok()).map(|img| {
        let img = img.thumbnail(CARD_W as u32 * 2, THUMB_H as u32 * 2).to_rgba8();
        egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
    });
    Ok((s, img))
}

impl Home {
    /// (path, frames, has a picture) of the files read so far.
    #[cfg(test)]
    pub fn read(&self) -> Vec<(String, usize, bool)> {
        self.cards.iter().filter_map(|(k, (_, s))| match s {
            Slot::Ready(i) => Some((k.clone(), i.frames, i.tex.is_some())),
            _ => None,
        }).collect()
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok((path, mtime, res)) = self.rx.try_recv() {
            self.busy = self.busy.saturating_sub(1);
            let slot = match res {
                Ok((s, img)) => Slot::Ready(Info { kind: s.kind, frames: s.frames, tex: img.map(|i| ctx.load_texture(format!("sb-home-{path}"), i, egui::TextureOptions::LINEAR)) }),
                Err(_) => Slot::Failed,
            };
            // a reply for an older version of the file is dropped
            if self.cards.get(&path).is_some_and(|(m, _)| *m == mtime) {
                self.cards.insert(path, (mtime, slot));
            }
        }
    }

    /// Starts reading the files not read yet (or changed), a few at a time.
    fn request(&mut self, ctx: &egui::Context, cx: &Cx, boards: &[(String, u64)]) {
        for (path, mtime) in boards {
            if self.busy >= READERS {
                break;
            }
            if self.cards.get(path).is_some_and(|(m, _)| m == mtime) {
                continue;
            }
            self.cards.insert(path.clone(), (*mtime, Slot::Loading));
            self.busy += 1;
            let (tx, abs, path, mtime, ctx) = (self.tx.clone(), cx.vault.abs(path), path.clone(), *mtime, ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send((path, mtime, load(abs)));
                ctx.request_repaint();
            });
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let ctx = ui.ctx().clone();
        self.receive(&ctx);
        let mut boards: Vec<(String, u64)> = cx.vault.files.iter().filter(|e| !e.folder && e.path.to_lowercase().ends_with(".storyboard")).map(|e| (e.path.clone(), e.mtime)).collect();
        boards.sort_by_key(|b| std::cmp::Reverse(b.1));
        self.cards.retain(|k, _| boards.iter().any(|b| &b.0 == k));
        self.request(&ctx, cx, &boards);

        egui::ScrollArea::vertical().id_salt("sb-home").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let w = avail.min(1100.0);
            let side = ((avail - w) / 2.0).max(0.0) + 40.0;
            ui.add_space(36.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    let inner = (w - 80.0).max(300.0);
                    ui.set_width(inner);
                    ui.label(RichText::new("Storyboards").font(kit::bold(26.0)).color(p.text));
                    ui.add_space(2.0);
                    let count = match boards.len() {
                        0 => "Nenhum storyboard no cofre ainda".to_string(),
                        1 => "1 storyboard no cofre".to_string(),
                        n => format!("{n} storyboards no cofre"),
                    };
                    ui.label(RichText::new(count).color(p.muted));
                    ui.add_space(26.0);

                    kit::section(ui, "Criar", p);
                    ui.add_space(8.0);
                    self.kinds(ui, inner, p, &mut acts);
                    ui.add_space(30.0);

                    ui.horizontal(|ui| {
                        kit::section(ui, "Abrir", p);
                        if !boards.is_empty() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.allocate_ui(egui::vec2(260.0, 26.0), |ui| kit::search_field(ui, &mut self.filter, "Filtrar por nome ou pasta", p));
                            });
                        }
                    });
                    ui.add_space(10.0);
                    if boards.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(kit::icon(icons::CLAPPERBOARD, 30.0).color(p.faint));
                            ui.add_space(10.0);
                            ui.label(RichText::new("Os storyboards que você criar aparecem aqui.").color(p.muted));
                        });
                        return;
                    }
                    let q = self.filter.trim().to_lowercase();
                    let shown: Vec<&(String, u64)> = boards.iter().filter(|(path, _)| q.is_empty() || path.to_lowercase().contains(&q)).collect();
                    if shown.is_empty() {
                        ui.label(RichText::new("Nenhum storyboard com esse nome.").color(p.muted));
                        return;
                    }
                    let cols = (((inner + GAP) / (CARD_W + GAP)).floor() as usize).max(1);
                    for row in shown.chunks(cols) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = GAP;
                            for (path, mtime) in row {
                                self.card(ui, path, *mtime, p, &mut acts);
                            }
                        });
                        ui.add_space(GAP);
                    }
                });
            });
            ui.add_space(40.0);
        });
        acts
    }

    /// One tile per kind; a click asks for its options (or makes the canvas).
    fn kinds(&mut self, ui: &mut egui::Ui, inner: f32, p: crate::theme::Palette, acts: &mut Vec<Action>) {
        let gap = 10.0;
        let cols = if inner > 760.0 { 4 } else if inner > 520.0 { 3 } else { 2 };
        let tw = (inner - gap * (cols as f32 - 1.0)) / cols as f32;
        for row in pg::KINDS.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for k in row {
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(tw, 74.0), Sense::click());
                    let hover = r.hovered();
                    ui.painter().rect(rect, 8.0, if hover { p.bg_hover } else { p.bg }, Stroke::new(1.0_f32, if hover { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
                    ui.painter().text(rect.left_top() + egui::vec2(20.0, 22.0), egui::Align2::CENTER_CENTER, kind_icon(k.id), kit::icon_font(16.0), if hover { p.accent_strong } else { p.muted });
                    ui.painter().text(rect.left_top() + egui::vec2(38.0, 14.0), egui::Align2::LEFT_TOP, k.name, kit::medium(13.0), p.text);
                    let g = ui.painter().layout(k.blurb.to_string(), egui::FontId::proportional(11.0), p.faint, tw - 50.0);
                    let clip = egui::Rect::from_min_max(rect.left_top() + egui::vec2(38.0, 32.0), rect.right_bottom() - egui::vec2(10.0, 6.0));
                    ui.painter().with_clip_rect(clip).galley(clip.min, g, p.faint);
                    if r.on_hover_text(k.blurb).clicked() {
                        acts.push(Action::NewStoryboard(k.id.to_string()));
                    }
                }
            });
            ui.add_space(gap);
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, path: &str, mtime: u64, p: crate::theme::Palette, acts: &mut Vec<Action>) {
        let (rect, r) = ui.allocate_exact_size(egui::vec2(CARD_W, THUMB_H + 58.0), Sense::click());
        let hover = r.hovered();
        let painter = ui.painter();
        let thumb = egui::Rect::from_min_size(rect.min, egui::vec2(CARD_W, THUMB_H));
        painter.rect(thumb, 8.0, if p.dark { Color32::from_gray(28) } else { p.bg_secondary }, Stroke::new(if hover { 1.5_f32 } else { 1.0 }, if hover { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
        let info = match self.cards.get(path) {
            Some((_, Slot::Ready(i))) => Some(i),
            _ => None,
        };
        match self.cards.get(path).map(|c| &c.1) {
            Some(Slot::Ready(Info { tex: Some(t), .. })) => {
                let sz = t.size_vec2();
                let area = thumb.shrink(8.0);
                let k = (area.width() / sz.x).min(area.height() / sz.y);
                let img = egui::Rect::from_center_size(area.center(), sz * k);
                painter.image(t.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            Some(Slot::Loading) | None => {
                painter.text(thumb.center(), egui::Align2::CENTER_CENTER, icons::CLAPPERBOARD, kit::icon_font(22.0), p.border_strong);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
            }
            _ => {
                let glyph = info.map(|i| kind_icon(&i.kind)).unwrap_or(icons::CLAPPERBOARD);
                painter.text(thumb.center(), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(22.0), p.faint);
            }
        }
        let name = painter.layout(name_of(path).to_string(), kit::medium(13.5), p.text, CARD_W - 4.0);
        let name_rect = egui::Rect::from_min_size(thumb.left_bottom() + egui::vec2(2.0, 8.0), egui::vec2(CARD_W - 4.0, 18.0));
        painter.with_clip_rect(name_rect).galley(name_rect.min, name, p.text);
        let meta = match info {
            Some(i) if i.kind == "canvas" => format!("Tela infinita · {}", fmt_date(mtime as i64)),
            Some(i) => {
                let k = pg::kind_info(&i.kind);
                format!("{} · {} {} · {}", k.name, i.frames, if i.frames == 1 { k.unit } else { k.units }, fmt_date(mtime as i64))
            }
            None => fmt_date(mtime as i64),
        };
        let folder = vault::parent(path);
        let line2 = if folder.is_empty() { meta } else { format!("{meta}\n{folder}") };
        let g = painter.layout(line2, egui::FontId::proportional(11.5), p.faint, CARD_W - 4.0);
        let meta_rect = egui::Rect::from_min_max(name_rect.left_bottom() + egui::vec2(0.0, 2.0), rect.right_bottom());
        painter.with_clip_rect(meta_rect).galley(meta_rect.min, g, p.faint);
        if matches!(self.cards.get(path), Some((_, Slot::Failed))) {
            r.clone().on_hover_text("Não foi possível ler a miniatura deste arquivo");
        }
        if r.clicked() {
            acts.push(Action::Open(path.to_string(), false));
        }
        r.context_menu(|ui| {
            if kit::menu_item(ui, icons::CLAPPERBOARD, "Abrir", p) {
                acts.push(Action::Open(path.to_string(), false));
            }
            if kit::menu_item(ui, icons::PLUS, "Abrir em nova aba", p) {
                acts.push(Action::Open(path.to_string(), true));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;
    use eframe::App as _;

    fn workspace(name: &str) -> (crate::app::App, egui::Context, std::path::PathBuf) {
        let ctx = egui::Context::default();
        let mut app = crate::app::App::new(&eframe::CreationContext::_new_kittest(ctx.clone()), None);
        let root = std::env::temp_dir().join(format!("ateliet-storyboards-{}-{name}", std::process::id()));
        std::fs::create_dir_all(root.join("img")).unwrap();
        std::fs::write(root.join("Nota.md"), "# Nota\n").unwrap();
        image::RgbaImage::from_pixel(32, 32, image::Rgba([200, 30, 30, 255])).save(root.join("img/a.png")).unwrap();
        app.vault = Some(crate::vault::Vault::open(&root, ctx.clone()));
        app.sc = Some(crate::sc::Sc::load(&root.join(".obsidian")));
        (app, ctx, root)
    }

    fn frame(app: &mut crate::app::App, ctx: &egui::Context, n: f64) {
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
        let _ = ctx.run(egui::RawInput { screen_rect: Some(bounds), time: Some(n), ..Default::default() }, |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()));
    }

    #[test]
    fn the_screen_lists_the_vaults_storyboards_with_their_first_frame() {
        let (mut app, ctx, root) = workspace("list");
        app.new_board("animatic");
        app.save_all();
        app.open(super::PATH, true);
        let mut read = Vec::new();
        for n in 0..100 {
            frame(&mut app, &ctx, n as f64);
            if let Some(crate::sc::View::Storyboards(h)) = app.tabs[app.active].view.as_ref() {
                read = h.read();
            }
            if !read.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(read, vec![("Storyboard.storyboard".to_string(), 1, true)]);
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_picture_from_the_side_panel_always_lands_or_says_why() {
        let (mut app, _ctx, root) = workspace("pictures");
        app.open("Nota.md", true);
        app.insert_pictures(&["img/a.png".into()]);
        assert!(app.tabs[app.active].text.contains("![[a.png]]"));
        app.new_board("animatic");
        app.toasts.clear();
        app.insert_pictures(&["img/a.png".into()]);
        assert!(app.tabs[app.active].board.as_ref().unwrap().doc.board["references"].as_array().is_some_and(|r| r.len() == 1));
        app.open(super::PATH, true);
        app.toasts.clear();
        app.insert_pictures(&["img/a.png".into()]);
        assert_eq!(app.toasts.len(), 1, "a tab that takes no pictures must say so");
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}
