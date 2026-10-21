//! The library: the selected book up close (cover, numbers, progress, the
//! book's screens), the last fortnight of writing, and the shelf.

use super::{color, fmt_date, Action, Sc};
use crate::host::{self, Caches};
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use crate::vault::Vault;
use ateliet_core::scriptorium::book::{self, Book};
use ateliet_core::scriptorium::{fmt_n, link_text};
use eframe::egui::{self, Color32, RichText, Sense};

#[derive(Default)]
pub struct Library {
    pub selected: Option<String>,
}

pub struct Cx<'a> {
    pub vault: &'a Vault,
    pub sc: &'a mut Sc,
    pub caches: &'a mut Caches,
    pub palette: Palette,
}

/// The cover: the book's picture (`capa`) or a plain coloured board with the title.
pub fn cover(ui: &mut egui::Ui, cx: &mut Cx, b: &Book, size: egui::Vec2) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect.expand(6.0));
    for (i, a) in [(1.0, 18u8), (3.0, 14), (7.0, 10), (12.0, 6)] {
        painter.rect_filled(rect.translate(egui::vec2(0.0, i * 0.6)).expand(i * 0.3), 6.0, Color32::from_black_alpha(a));
    }
    let capa = link_text(b.fm.get("capa").unwrap_or(&serde_json::Value::Null));
    let tex = if capa.is_empty() { None } else { host::image(ui.ctx(), cx.vault, cx.caches, &capa, &b.path) };
    if let Some((id, sz)) = tex {
        // fill the board, cropping what overflows
        let k = (rect.width() / sz.x).max(rect.height() / sz.y);
        let (w, h) = (rect.width() / (sz.x * k), rect.height() / (sz.y * k));
        let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(w, h));
        painter.add(egui::epaint::RectShape::filled(rect, 4.0, Color32::WHITE).with_texture(id, uv));
    } else {
        let c = color(&b.color);
        painter.rect_filled(rect, 4.0, c);
        painter.rect_stroke(rect.shrink(rect.width() * 0.07), 1.0, egui::Stroke::new(0.8_f32, Color32::from_white_alpha(60)), egui::StrokeKind::Inside);
        let fs = (rect.width() / 9.0).clamp(10.0, 24.0);
        let galley = painter.layout(b.title.clone(), egui::FontId::new(fs, egui::FontFamily::Name("bold".into())), Color32::WHITE, rect.width() * 0.8);
        painter.galley(egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + rect.height() * 0.28), galley, Color32::WHITE);
        if !b.author.is_empty() {
            let g = painter.layout(b.author.clone(), egui::FontId::proportional(fs * 0.55), Color32::from_white_alpha(220), rect.width() * 0.8);
            painter.galley(egui::pos2(rect.center().x - g.size().x / 2.0, rect.bottom() - rect.height() * 0.14 - g.size().y), g, Color32::WHITE);
        }
    }
    // spine shadow
    painter.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * 0.03, rect.height())), egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, Color32::from_black_alpha(30));
    resp
}

fn progress(ui: &mut egui::Ui, p: Palette, frac: f32, w: f32, h: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    ui.painter().rect_filled(r, h / 2.0, p.bg_active);
    let mut f = r;
    f.set_width((r.width() * frac.clamp(0.0, 1.0)).max(if frac > 0.0 { h } else { 0.0 }));
    ui.painter().rect_filled(f, h / 2.0, p.accent_strong);
}

impl Library {
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let ix_lock = cx.vault.index.clone();
        let ix = ix_lock.read().unwrap();
        let author = cx.sc.author();
        let books = book::books(&ix, &author);
        egui::ScrollArea::vertical().id_salt("sc-library").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let w = avail.min(980.0);
            let side = ((avail - w) / 2.0).max(0.0) + 40.0;
            ui.add_space(36.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                ui.set_width((w - 80.0).max(300.0));
                // header
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Biblioteca").font(kit::bold(26.0)).color(p.text));
                        ui.add_space(2.0);
                        let st = cx.sc.settings.streak();
                        let today = fmt_n(cx.sc.settings.today_words(None));
                        let line = if st > 0 { format!("{today} palavras hoje · {st} {} seguidos", if st == 1 { "dia" } else { "dias" }) } else { format!("{today} palavras hoje") };
                        ui.label(RichText::new(line).color(p.muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::primary_icon(ui, icons::PLUS, "Novo livro", p).clicked() {
                            acts.push(Action::NewBook);
                        }
                    });
                });
                ui.add_space(30.0);
                if books.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.label(kit::icon(icons::BOOK_PLUS, 34.0).color(p.faint));
                        ui.add_space(14.0);
                        ui.label(RichText::new("Sua estante está vazia").font(kit::medium(18.0)).color(p.text));
                        ui.add_space(4.0);
                        ui.label(RichText::new("Um livro traz manuscrito em capítulos, personagens, enredo, mapas e exportações.").color(p.muted));
                        ui.add_space(18.0);
                        if kit::primary(ui, "Criar o primeiro livro", p).clicked() {
                            acts.push(Action::NewBook);
                        }
                    });
                    return;
                }
                let last = cx.sc.settings.str("ultimoLivro", "");
                if self.selected.as_ref().is_none_or(|s| !books.iter().any(|b| &b.path == s)) {
                    self.selected = Some(if books.iter().any(|b| b.path == last) { last.clone() } else { books[0].path.clone() });
                }
                let sel = self.selected.clone().unwrap();
                let b = books.iter().find(|b| b.path == sel).unwrap();
                let stats = cx.sc.stats(&ix, b);

                // the selected book
                ui.horizontal_top(|ui| {
                    if cover(ui, cx, b, egui::vec2(196.0, 294.0)).on_hover_text("Abrir a nota do livro").clicked() {
                        acts.push(Action::Open(b.path.clone(), false));
                    }
                    ui.add_space(36.0);
                    ui.vertical(|ui| {
                        ui.add_space(6.0);
                        let genre = b.fm.get("genero").and_then(|g| g.as_str()).unwrap_or("");
                        let serie = if b.serie.is_empty() { String::new() } else { format!("{}{}", b.serie, if b.volume.is_empty() { String::new() } else { format!(" · volume {}", b.volume) }) };
                        if !serie.is_empty() {
                            ui.label(RichText::new(serie).font(kit::medium(12.0)).color(p.accent_strong));
                            ui.add_space(2.0);
                        }
                        ui.label(RichText::new(&b.title).font(kit::bold(30.0)).color(p.text));
                        let by: Vec<&str> = [b.author.as_str(), genre].into_iter().filter(|s| !s.is_empty()).collect();
                        if !by.is_empty() {
                            ui.label(RichText::new(by.join(" · ")).size(15.0).color(p.muted));
                        }
                        ui.add_space(22.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 30.0;
                            for (n, l) in [(stats.words, "palavras"), (stats.chapters, "capítulos"), (stats.scenes, "cenas"), (stats.characters, "personagens")] {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(fmt_n(n as i64)).font(kit::medium(22.0)).color(p.text));
                                    ui.label(RichText::new(l).size(12.0).color(p.faint));
                                });
                            }
                        });
                        if b.goal > 0 {
                            ui.add_space(18.0);
                            let frac = stats.words as f32 / b.goal as f32;
                            progress(ui, p, frac, 360.0, 4.0);
                            ui.add_space(4.0);
                            let mut extra = String::new();
                            if let Ok(d) = chrono::NaiveDate::parse_from_str(b.deadline.get(..10).unwrap_or(""), "%Y-%m-%d") {
                                let days = (d - chrono::Local::now().date_naive()).num_days();
                                if days > 0 {
                                    extra = format!(" · faltam {days} dias, {} palavras por dia", fmt_n((b.goal - stats.words as i64).max(0) / days));
                                } else {
                                    extra = " · prazo vencido".into();
                                }
                            }
                            ui.label(RichText::new(format!("{:.0}% de {} palavras{extra}", (frac * 100.0).min(100.0), fmt_n(b.goal))).size(12.0).color(p.faint));
                        }
                        ui.add_space(24.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                            if kit::primary_icon(ui, icons::FEATHER, "Continuar escrevendo", p).clicked() {
                                acts.push(Action::Continue(b.path.clone()));
                            }
                            if kit::button(ui, Some(icons::LIST_TREE), "Manuscrito", p).clicked() {
                                acts.push(Action::View("binder".into(), Some(b.path.clone())));
                            }
                            if kit::button(ui, Some(icons::FILE_PLUS), "Capítulo", p).clicked() {
                                acts.push(Action::NewChapter(b.path.clone()));
                            }
                            if kit::button(ui, Some(icons::USER_PLUS), "Personagem", p).clicked() {
                                acts.push(Action::NewCharacter(b.path.clone()));
                            }
                            if kit::button(ui, Some(icons::DOWNLOAD), "Exportar", p).clicked() {
                                acts.push(Action::Export(b.path.clone()));
                            }
                        });
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if kit::quiet(ui, Some(icons::FILE_TEXT), "Nota do livro", p).clicked() {
                                acts.push(Action::Open(b.path.clone(), false));
                            }
                            ui.label(RichText::new(format!("· editado em {}", fmt_date(b.modified))).size(12.0).color(p.faint));
                        });
                    });
                });

                // the last fortnight
                ui.add_space(44.0);
                kit::section(ui, "Últimos 14 dias", p);
                ui.add_space(6.0);
                let days = cx.sc.settings.last_days(14);
                let max = days.iter().map(|d| d.1).max().unwrap_or(0).max(1) as f32;
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0 * 34.0, 90.0), Sense::hover());
                let pointer = ui.ctx().pointer_hover_pos();
                for (i, (day, w)) in days.iter().enumerate() {
                    let x = r.left() + i as f32 * 34.0;
                    let h = (*w as f32 / max * 64.0).max(2.0);
                    let h = if *w > 0 { h.max(4.0) } else { 3.0 };
                    let bar = egui::Rect::from_min_max(egui::pos2(x + 9.0, r.top() + 66.0 - h), egui::pos2(x + 25.0, r.top() + 66.0));
                    ui.painter().rect_filled(bar, 4.0, if *w > 0 { p.accent_strong } else { p.bg_active });
                    ui.painter().text(egui::pos2(x + 17.0, r.top() + 82.0), egui::Align2::CENTER_CENTER, &day[8..], egui::FontId::proportional(11.0), p.faint);
                    if pointer.is_some_and(|pt| bar.expand2(egui::vec2(4.0, 70.0)).contains(pt)) {
                        let d: Vec<&str> = day.split('-').rev().collect();
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new(("day", i)), |ui| {
                            ui.label(format!("{}: {} palavras", d.join("/"), fmt_n(*w)));
                        });
                    }
                }

                // the shelf: one row per series, single books last
                ui.add_space(36.0);
                kit::section(ui, &format!("Estante · {} {}", books.len(), if books.len() == 1 { "livro" } else { "livros" }), p);
                let mut groups: Vec<(String, Vec<&Book>)> = Vec::new();
                for b in &books {
                    let k = b.serie.trim().to_string();
                    match groups.iter_mut().find(|g| g.0 == k) {
                        Some(g) => g.1.push(b),
                        None => groups.push((k, vec![b])),
                    }
                }
                let many = groups.len() > 1;
                for (k, bs) in groups {
                    if many || !k.is_empty() {
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(if k.is_empty() { "Livros avulsos".to_string() } else { k.clone() }).font(kit::medium(14.0)).color(p.text));
                            ui.label(RichText::new(format!("{} {}", bs.len(), if bs.len() == 1 { "livro" } else { "livros" })).small().color(p.muted));
                        });
                    }
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(26.0, 22.0);
                        for b in bs {
                            let st = cx.sc.stats(&ix, b);
                            let selected = b.path == sel;
                            let r = egui::Frame::new().show(ui, |ui| {
                                ui.set_width(132.0);
                                ui.vertical(|ui| {
                                    let c = cover(ui, cx, b, egui::vec2(132.0, 198.0));
                                    if selected {
                                        let rr = c.rect.expand(5.0);
                                        ui.painter().rect_stroke(rr, 8.0, egui::Stroke::new(2.0_f32, p.accent_strong), egui::StrokeKind::Outside);
                                    }
                                    ui.add_space(8.0);
                                    let vol = if !k.is_empty() && !b.volume.is_empty() { format!("{} · ", b.volume) } else { String::new() };
                                    ui.add(egui::Label::new(RichText::new(format!("{vol}{}", b.title)).font(kit::medium(13.0)).color(p.text)).truncate());
                                    ui.add(egui::Label::new(RichText::new(format!("{} palavras", fmt_n(st.words as i64))).size(12.0).color(p.faint)).truncate());
                                    c
                                })
                                .inner
                            });
                            let c = r.inner;
                            if c.clicked() {
                                self.selected = Some(b.path.clone());
                                acts.push(Action::SelectBook(b.path.clone()));
                            }
                            if c.double_clicked() {
                                acts.push(Action::Continue(b.path.clone()));
                            }
                        }
                    });
                }
                });
            });
            ui.add_space(40.0);
        });
        acts
    }
}
