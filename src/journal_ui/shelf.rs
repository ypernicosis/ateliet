//! The shelf of notebooks, the new notebook, and all the pages of one seen
//! from above (where they are put in another order).

use super::draw::{self, PageView};
use super::{Book, Cx, Journal, Look, Out};
use crate::icons;
use crate::kit;
use ateliet_core::journal::paper::{COLUMNS, PAPERS, SIZES};
use ateliet_core::journal::{format::COVERS, Item, Kind, Layout, Notebook, Page};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense, Stroke};

/// The folder new notebooks go in.
pub const FOLDER: &str = "Cadernos";

pub struct NewNotebook {
    name: String,
    size: usize,
    paper: usize,
    columns: usize,
    cover: usize,
    daily: bool,
    error: Option<String>,
    focus: bool,
}

impl NewNotebook {
    pub fn new() -> NewNotebook {
        NewNotebook { name: String::new(), size: 1, paper: 3, columns: 1, cover: 0, daily: false, error: None, focus: true }
    }
}

impl Journal {
    pub fn shelf_ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let p = cx.palette;
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, draw::desk_color(p));
        let mut open: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("journal-shelf").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 48, right: 48, top: 36, bottom: 36 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Cadernos").font(kit::bold(22.0)).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::primary_icon(ui, icons::PLUS, "Novo caderno", p).clicked() {
                            self.new_dlg = Some(NewNotebook::new());
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Cada caderno é uma pasta de notas: uma por página, legíveis no Obsidian.").size(12.5).color(p.faint));
                ui.add_space(26.0);
                let Some(list) = self.shelf.clone() else {
                    ui.label(RichText::new("Procurando cadernos…").size(13.0).color(p.faint));
                    return;
                };
                if list.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(kit::icon(icons::NOTEBOOK_PEN, 34.0).color(p.faint));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Nenhum caderno ainda").font(kit::medium(15.0)).color(p.text));
                        ui.label(RichText::new("Um caderno tem capa, páginas de tamanho fixo e papel quadriculado, pautado ou liso.").size(12.5).color(p.muted));
                        ui.add_space(12.0);
                        if kit::primary_icon(ui, icons::PLUS, "Novo caderno", p).clicked() {
                            self.new_dlg = Some(NewNotebook::new());
                        }
                    });
                    return;
                }
                // the one used last first, a little larger
                let mtime = |nb: &Notebook| cx.vault.files.iter().find(|e| e.path == nb.path).map(|e| e.mtime).unwrap_or(0);
                let mut list = list;
                list.sort_by_key(|nb| std::cmp::Reverse(mtime(nb)));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(26.0, 26.0);
                    for (i, nb) in list.iter().enumerate() {
                        let h = if i == 0 { 250.0 } else { 200.0 };
                        let w = h * (nb.size.w / nb.size.h) as f32;
                        let (r, resp) = ui.allocate_exact_size(vec2(w, h + 34.0), Sense::click());
                        let cover = Rect::from_min_size(r.min, vec2(w, h));
                        let lift = if resp.hovered() { -3.0 } else { 0.0 };
                        let cover = cover.translate(vec2(0.0, lift));
                        ui.painter().add(egui::epaint::RectShape::filled(cover, 4.0, Color32::from_black_alpha(if p.dark { 110 } else { 40 })).with_blur_width(14.0));
                        let page = self.cover_page(cx, &nb.path, mtime(nb));
                        let painter = ui.painter_at(cover);
                        self.draw_cover(ui.ctx(), &painter, nb, page.as_ref(), cover, cx);
                        if nb.daily {
                            let col = draw::hex(&nb.cover).unwrap_or(Color32::GRAY);
                            let on = if crate::theme::contrast(col, Color32::WHITE) > 3.0 { Color32::from_rgb(0xf6, 0xf3, 0xec) } else { Color32::from_rgb(0x22, 0x22, 0x24) };
                            ui.painter().text(cover.right_top() + vec2(-12.0, 14.0), egui::Align2::RIGHT_TOP, icons::CALENDAR_DAYS, kit::icon_font(13.0), on.gamma_multiply(0.7));
                        }
                        let pages = nb.pages.len();
                        let foot = format!("{pages} {}{}", if pages == 1 { "página" } else { "páginas" }, nb.start.as_deref().map(|s| format!(" · desde {s}")).unwrap_or_default());
                        ui.painter().text(pos2(r.left(), cover.bottom() + 10.0 - lift), egui::Align2::LEFT_TOP, foot, egui::FontId::proportional(11.5), p.faint);
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            open = Some(nb.path.clone());
                        }
                        resp.context_menu(|ui| {
                            if kit::menu_item(ui, icons::BOOK_OPEN, "Abrir", p) {
                                open = Some(nb.path.clone());
                                ui.close_menu();
                            }
                            if kit::menu_item(ui, icons::FILE_TEXT, "Abrir a nota-índice", p) {
                                self.out.push(Out::Open(nb.path.clone()));
                                ui.close_menu();
                            }
                            kit::menu_sep(ui, p);
                            if kit::menu_danger(ui, icons::TRASH_2, "Apagar o caderno…", p) {
                                self.delete_dlg = Some(nb.clone());
                                ui.close_menu();
                            }
                        });
                    }
                });
            });
        });
        if let Some(path) = open {
            let backup = cx.config_dir.as_ref().map(|d| d.join("ateliet").join("journal"));
            self.open_notebook(cx.vault, &path, backup);
        }
    }

    /// Asked before a notebook goes: its index note and its pages go to the
    /// trash; anything else in its folder (pictures, storyboards drawn for
    /// it) stays, and the folder goes only when nothing is left in it.
    pub fn delete_dialog(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        let Some(nb) = self.delete_dlg.take() else { return };
        let p = cx.palette;
        let mut keep = true;
        let mut go = false;
        kit::scrim(ctx, p);
        egui::Window::new("journal-delete").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).fixed_size(vec2(400.0, 0.0)).frame(egui::Frame::window(&ctx.style()).fill(p.bg).inner_margin(egui::Margin::same(18))).show(ctx, |ui| {
            ui.label(RichText::new(format!("Apagar o caderno “{}”?", nb.name)).font(kit::bold(16.0)).color(p.text));
            ui.add_space(8.0);
            let mut full = nb.clone();
            full.scan(&cx.vault.root);
            let n = full.pages.len();
            ui.label(RichText::new(format!("A nota-índice e {n} {} vão para a lixeira do sistema, de onde podem ser recuperadas. Imagens e outros arquivos da pasta ficam.", if n == 1 { "página" } else { "páginas" })).size(12.5).color(p.muted));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::danger(ui, "Apagar", p).clicked() {
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
            // the open notebook closes first (all saved)
            if self.book.as_ref().is_some_and(|b| b.nb.path == nb.path) {
                self.close(cx.vault);
                self.book = None;
            }
            let mut full = nb.clone();
            full.scan(&cx.vault.root);
            let mut files: Vec<String> = full.pages.iter().map(|r| r.path.clone()).collect();
            files.push(nb.path.clone());
            let mut failed = 0;
            for f in &files {
                let abs = cx.vault.abs(f);
                if trash::delete(&abs).is_err() && std::fs::remove_file(&abs).is_err() {
                    failed += 1;
                }
                if let Some(d) = &cx.config_dir {
                    let _ = std::fs::remove_file(d.join("ateliet").join("journal").join(format!("{}.json", f.replace('/', "__"))));
                }
            }
            if !nb.folder.is_empty() {
                let dir = cx.vault.abs(&nb.folder);
                if std::fs::read_dir(&dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
                    let _ = std::fs::remove_dir(&dir);
                }
            }
            self.covers.remove(&nb.path);
            self.shelf = self.shelf.take().map(|l| l.into_iter().filter(|x| x.path != nb.path).collect());
            self.out.push(Out::Rescan);
            self.out.push(Out::Toast(if failed == 0 { format!("Caderno “{}” apagado.", nb.name) } else { format!("{failed} arquivos do caderno não puderam ser apagados.") }));
            keep = false;
        }
        if keep {
            self.delete_dlg = Some(nb);
        }
    }

    /// A notebook's cover page (from its index note), read once per change.
    fn cover_page(&mut self, cx: &Cx, path: &str, mtime: u64) -> Option<Page> {
        if self.covers.get(path).is_none_or(|(m, _)| *m != mtime) {
            let text = std::fs::read_to_string(cx.vault.abs(path)).ok()?;
            self.covers.insert(path.to_string(), (mtime, Page::parse(&text)));
        }
        self.covers.get(path).map(|(_, p)| p.clone())
    }

    pub fn new_notebook_dialog(&mut self, ctx: &egui::Context, cx: &mut Cx) {
        let Some(mut d) = self.new_dlg.take() else { return };
        let p = cx.palette;
        let mut keep = true;
        let mut create = false;
        kit::scrim(ctx, p);
        egui::Window::new("Novo caderno").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).fixed_size(vec2(440.0, 0.0)).frame(egui::Frame::window(&ctx.style()).fill(p.bg).inner_margin(egui::Margin::same(18))).title_bar(false).show(ctx, |ui| {
            ui.label(RichText::new("Novo caderno").font(kit::bold(17.0)).color(p.text));
            ui.add_space(12.0);
            ui.label(RichText::new("Nome").size(12.5).color(p.muted));
            let r = ui.add(egui::TextEdit::singleline(&mut d.name).hint_text("Natureza, Diário de 2026…").desired_width(f32::INFINITY));
            if d.focus {
                r.request_focus();
                d.focus = false;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Tamanho").size(12.5).color(p.muted));
            let sizes: Vec<(&str, &str)> = SIZES.iter().map(|(n, ..)| ("", *n)).collect();
            if let Some(k) = kit::segmented(ui, &sizes, d.size, p) {
                d.size = k;
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Papel").size(12.5).color(p.muted));
                    egui::ComboBox::from_id_salt("journal-paper").width(190.0).selected_text(PAPERS[d.paper].1).show_ui(ui, |ui| {
                        for (i, (_, label)) in PAPERS.iter().enumerate() {
                            ui.selectable_value(&mut d.paper, i, *label);
                        }
                    });
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new("Colunas").size(12.5).color(p.muted));
                    egui::ComboBox::from_id_salt("journal-columns").width(200.0).selected_text(COLUMNS[d.columns].1).show_ui(ui, |ui| {
                        for (i, (_, label)) in COLUMNS.iter().enumerate() {
                            ui.selectable_value(&mut d.columns, i, *label);
                        }
                    });
                });
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Capa").size(12.5).color(p.muted));
            ui.horizontal(|ui| {
                for (i, c) in COVERS.iter().enumerate() {
                    let col = draw::hex(c).unwrap_or(Color32::GRAY);
                    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 34.0), Sense::click());
                    ui.painter().rect_filled(r.shrink(2.0), 3.0, col);
                    if d.cover == i {
                        ui.painter().rect_stroke(r, 4.0, Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Inside);
                    }
                    if resp.clicked() {
                        d.cover = i;
                    }
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                kit::toggle(ui, &mut d.daily, p);
                ui.label(RichText::new("Diário: “Página de hoje” acrescenta uma página com a data").size(12.5).color(p.text));
            });
            if let Some(e) = &d.error {
                ui.add_space(8.0);
                ui.label(RichText::new(e).size(12.5).color(p.error));
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::primary(ui, "Criar", p).clicked() {
                        create = true;
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
        if create {
            match create_notebook(cx, &d) {
                Ok(path) => {
                    let backup = cx.config_dir.as_ref().map(|c| c.join("ateliet").join("journal"));
                    self.open_notebook(cx.vault, &path, backup);
                    self.out.push(Out::Rescan);
                    keep = false;
                }
                Err(e) => d.error = Some(e),
            }
        }
        if keep {
            self.new_dlg = Some(d);
        }
    }

    /// All the pages, in pairs as the notebook opens; a page dragged
    /// elsewhere takes that place.
    pub fn thumbs_ui(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &mut Cx, canvas: Rect) {
        let p = cx.palette;
        let (w, h) = (b.nb.size.w as f32, b.nb.size.h as f32);
        let ph = 150.0;
        let k = ph / h;
        let pw = w * k;
        let mut rects: Vec<(usize, Rect)> = Vec::new();
        let mut go: Option<usize> = None;
        let drag_id = egui::Id::new(("journal-thumb-drag", &b.nb.path));
        let dragging: Option<usize> = ui.ctx().data(|d| d.get_temp(drag_id));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(canvas), |ui| {
            egui::ScrollArea::vertical().id_salt("journal-thumbs").auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { left: 40, right: 40, top: 30, bottom: 90 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&b.nb.name).font(kit::bold(18.0)).color(p.text));
                        ui.label(RichText::new(format!("{} {} · arraste para mudar a ordem", b.count(), if b.count() == 1 { "página" } else { "páginas" })).size(12.0).color(p.faint));
                    });
                    ui.add_space(18.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(30.0, 28.0);
                        for s in 0..b.spreads() {
                            let (r, _) = ui.allocate_exact_size(vec2(2.0 * pw, ph + 18.0), Sense::hover());
                            let left = Rect::from_min_size(r.min, vec2(pw, ph));
                            let right = Rect::from_min_size(r.min + vec2(pw, 0.0), vec2(pw, ph));
                            for (n, rect) in [(2 * s, left), (2 * s + 1, right)] {
                                if n == 0 || n > b.count() {
                                    continue;
                                }
                                let lay = b.layout(n);
                                let pv = PageView { n, rect, k, lay, clip: rect, clip_across: rect.union(if n % 2 == 0 { right } else { left }) };
                                self.thumb_page(ui, b, cx, &pv);
                                rects.push((n, rect));
                                let resp = ui.interact(rect, egui::Id::new(("journal-thumb", n)), Sense::click_and_drag());
                                if resp.hovered() {
                                    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.5_f32, p.accent_soft), egui::StrokeKind::Outside);
                                }
                                if resp.clicked() {
                                    go = Some(n);
                                }
                                if resp.drag_started() {
                                    ui.ctx().data_mut(|d| d.insert_temp(drag_id, n));
                                }
                                if dragging == Some(n) {
                                    ui.painter().rect_filled(rect, 0.0, p.accent_soft.gamma_multiply(0.5));
                                }
                            }
                            if s > 0 && 2 * s <= b.count() && 2 * s < b.count() {
                                ui.painter().vline(r.min.x + pw, left.y_range(), Stroke::new(1.0_f32, Color32::from_black_alpha(40)));
                            }
                        }
                    });
                });
            });
        });
        // a page carried to another place
        if let Some(from) = dragging {
            let hover = ui.ctx().input(|i| i.pointer.hover_pos());
            let target = hover.and_then(|h| rects.iter().find(|(_, r)| r.contains(h)).map(|(n, r)| if h.x < r.center().x { *n } else { *n + 1 }));
            if let (Some(t), Some(h)) = (target, hover) {
                if let Some((_, r)) = rects.iter().find(|(n, _)| *n == t.min(b.count())) {
                    let x = if t > b.count() { r.right() } else { r.left() };
                    ui.painter().vline(x, r.y_range(), Stroke::new(3.0_f32, p.accent_strong));
                }
                let _ = h;
            }
            if ui.ctx().input(|i| i.pointer.any_released()) {
                ui.ctx().data_mut(|d| d.remove::<usize>(drag_id));
                if let Some(t) = target {
                    // before page `t` (1-based); after the last with count+1
                    if t != from && t != from + 1 {
                        let pg = b.nb.pages.remove(from - 1);
                        let at = if t > from { t - 2 } else { t - 1 };
                        b.nb.pages.insert(at.min(b.nb.pages.len()), pg);
                        b.reorder_files(&mut self.out);
                    }
                }
            }
        }
        if let Some(n) = go {
            b.go_to(n);
            b.look = Look::Spread;
        }
        // the toolbar is the way back
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            b.look = Look::Spread;
        }
        let back = egui::Area::new(egui::Id::new("journal-thumbs-back")).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(canvas.center_bottom() - vec2(0.0, 16.0)).order(egui::Order::Foreground);
        back.show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if kit::button(ui, Some(icons::BOOK_OPEN), "Voltar ao caderno", p).clicked() {
                    b.look = Look::Spread;
                }
            });
        });
    }

    /// A page small: the paper and its items (text as grey lines).
    fn thumb_page(&mut self, ui: &egui::Ui, b: &mut Book, cx: &mut Cx, pv: &PageView) {
        let p = cx.palette;
        let painter = ui.painter_at(pv.rect);
        painter.rect_filled(pv.rect, 0.0, draw::paper_color(p));
        painter.rect_stroke(pv.rect, 0.0, Stroke::new(1.0_f32, Color32::from_black_alpha(25)), egui::StrokeKind::Inside);
        let Some(page) = b.page(pv.n).cloned() else { return };
        let path = b.page_ref(pv.n).map(|r| r.path.clone()).unwrap_or_default();
        for it in &page.items {
            if it.kind.is_text() || it.kind == Kind::Note {
                let lh = pv.lay.paper.line() * it.style.lines();
                let rows = ((it.h / lh).round() as usize).max(1);
                for r in 0..rows {
                    let y = it.y + (r as f64 + 0.75) * lh;
                    let w = if r + 1 == rows { it.w * 0.6 } else { it.w };
                    painter.line_segment([pv.s((it.x, y)), pv.s((it.x + w, y))], Stroke::new((pv.k * 0.9).max(1.0), draw::ink_color(p).gamma_multiply(0.25)));
                }
            } else {
                self.draw_item(ui, &painter, it, pv, cx, &path);
            }
        }
        painter.text(pv.rect.center_bottom() + vec2(0.0, 12.0), egui::Align2::CENTER_CENTER, pv.n.to_string(), egui::FontId::proportional(11.0), p.faint);
    }
}

/// Write a new notebook: its folder, the index note, the title page.
fn create_notebook(cx: &mut Cx, d: &NewNotebook) -> Result<String, String> {
    let name: String = d.name.trim().chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']')).collect();
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Dê um nome ao caderno.".into());
    }
    let folder = format!("{FOLDER}/{name}");
    if cx.vault.abs(&folder).exists() {
        return Err(format!("Já existe uma pasta {folder}."));
    }
    let today = super::today();
    let size = SIZES[d.size].0;
    let paper = PAPERS[d.paper].0;
    let columns = COLUMNS[d.columns].0;
    let index = Notebook::new_text(&folder, size, paper, columns, COVERS[d.cover], &today, d.daily);
    let path = format!("{folder}/{name}.md");
    ateliet_core::quadro::write_atomic(&cx.vault.abs(&path), &index).map_err(|e| e.to_string())?;
    // the title page: the name and the day it began, in the middle
    let nb = Notebook::parse(&path, &index).ok_or("caderno inválido")?;
    let lay = Layout::new(&nb.size, nb.paper, nb.columns, true);
    let mut page = Page::parse(&Page::new_text(&name, 1, &today));
    let (c0, c1) = (ateliet_core::journal::paper::MARGIN_SIDE, lay.w - ateliet_core::journal::paper::MARGIN_SIDE);
    let mid = lay.text_top(lay.h * 0.36, 2.0);
    let mut title = Item::new(Kind::Text, c0, mid, c1 - c0, lay.paper.line() * 2.0);
    title.style.font = "mao".into();
    title.style.lines = 2.0;
    title.style.align = "centro".into();
    title.set_content(&name);
    let mut date = Item::new(Kind::Text, c0, mid + lay.paper.line() * 3.0, c1 - c0, lay.paper.line());
    date.style.align = "centro".into();
    date.style.font = "mao".into();
    date.set_content(&format!("começado em {today}"));
    page.items.push(title);
    page.items.push(date);
    let first = nb.page_path(1, "Folha de rosto");
    ateliet_core::quadro::write_atomic(&cx.vault.abs(&first), &page.write()).map_err(|e| e.to_string())?;
    Ok(path)
}
