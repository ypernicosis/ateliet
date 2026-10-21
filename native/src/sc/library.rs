//! The library: the selected book up close (cover, numbers, progress, the
//! book's screens), the last fortnight of writing, and the shelf.

use super::{color, fmt_date, Action, Sc};
use crate::host::{self, Caches};
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
    painter.rect_filled(rect.translate(egui::vec2(3.0, 4.0)), 4.0, Color32::from_black_alpha(40));
    let capa = link_text(b.fm.get("capa").unwrap_or(&serde_json::Value::Null));
    let tex = if capa.is_empty() { None } else { host::image(ui.ctx(), cx.vault, cx.caches, &capa, &b.path) };
    if let Some((id, sz)) = tex {
        // fill the board, cropping what overflows
        let k = (rect.width() / sz.x).max(rect.height() / sz.y);
        let (w, h) = (rect.width() / (sz.x * k), rect.height() / (sz.y * k));
        let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(w, h));
        painter.image(id, rect, uv, Color32::WHITE);
    } else {
        let c = color(&b.color);
        painter.rect_filled(rect, 4.0, c);
        painter.rect_stroke(rect.shrink(rect.width() * 0.06), 2.0, egui::Stroke::new(1.0_f32, Color32::from_white_alpha(70)), egui::StrokeKind::Inside);
        let fs = (rect.width() / 9.0).clamp(10.0, 24.0);
        let galley = painter.layout(b.title.clone(), egui::FontId::new(fs, egui::FontFamily::Name("bold".into())), Color32::WHITE, rect.width() * 0.8);
        painter.galley(egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + rect.height() * 0.28), galley, Color32::WHITE);
        if !b.author.is_empty() {
            let g = painter.layout(b.author.clone(), egui::FontId::proportional(fs * 0.55), Color32::from_white_alpha(220), rect.width() * 0.8);
            painter.galley(egui::pos2(rect.center().x - g.size().x / 2.0, rect.bottom() - rect.height() * 0.14 - g.size().y), g, Color32::WHITE);
        }
    }
    // spine shadow
    painter.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * 0.035, rect.height())), 0.0, Color32::from_black_alpha(35));
    resp
}

fn progress(ui: &mut egui::Ui, p: Palette, frac: f32, w: f32, h: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    ui.painter().rect_filled(r, h / 2.0, p.bg_secondary);
    let mut f = r;
    f.set_width(r.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(f, h / 2.0, p.accent);
}

fn chip(ui: &mut egui::Ui, p: Palette, text: String) {
    egui::Frame::new().fill(p.bg_secondary).corner_radius(12.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
        ui.label(RichText::new(text).color(p.muted));
    });
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
            egui::Frame::new().inner_margin(egui::Margin::symmetric(36, 26)).show(ui, |ui| {
                ui.set_width(ui.available_width().min(1100.0));
                // header
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Biblioteca").size(28.0).family(egui::FontFamily::Name("bold".into())).color(p.text));
                        ui.label(RichText::new("Seus mundos. Suas histórias. Seu legado.").color(p.muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("+  Novo livro").color(Color32::WHITE)).fill(p.accent)).clicked() {
                            acts.push(Action::NewBook);
                        }
                        let st = cx.sc.settings.streak();
                        if st > 0 {
                            chip(ui, p, format!("🔥 {st} {} seguidos", if st == 1 { "dia" } else { "dias" }));
                        }
                        chip(ui, p, format!("✒ {} palavras hoje", fmt_n(cx.sc.settings.today_words(None))));
                    });
                });
                ui.add_space(22.0);
                if books.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.label(RichText::new("Sua estante está vazia").size(22.0).color(p.text));
                        ui.label(RichText::new("Crie um livro para ganhar a estrutura completa: manuscrito em capítulos, personagens, quadro de enredo, mapas, artes e exportações.").color(p.muted));
                        ui.add_space(12.0);
                        if ui.button("Criar meu primeiro livro").clicked() {
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
                    ui.vertical(|ui| {
                        if cover(ui, cx, b, egui::vec2(190.0, 285.0)).on_hover_text("Abrir a nota do livro").clicked() {
                            acts.push(Action::Open(b.path.clone(), false));
                        }
                    });
                    ui.add_space(28.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("LIVRO SELECIONADO").small().color(p.faint));
                        ui.label(RichText::new(&b.title).size(30.0).family(egui::FontFamily::Name("bold".into())).color(p.text));
                        let genre = b.fm.get("genero").and_then(|g| g.as_str()).unwrap_or("");
                        let serie = if b.serie.is_empty() { String::new() } else { format!("{}{}", b.serie, if b.volume.is_empty() { String::new() } else { format!(" · vol. {}", b.volume) }) };
                        let by: Vec<&str> = [b.author.as_str(), serie.as_str(), genre].into_iter().filter(|s| !s.is_empty()).collect();
                        if !by.is_empty() {
                            ui.label(RichText::new(by.join(" — ")).color(p.muted));
                        }
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            for (n, l) in [(stats.chapters, "Capítulos"), (stats.scenes, "Cenas"), (stats.words, "Palavras"), (stats.characters, "Personagens"), (stats.maps, "Mapas")] {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(fmt_n(n as i64)).size(22.0).color(p.text));
                                    ui.label(RichText::new(l).small().color(p.muted));
                                });
                                ui.add_space(18.0);
                            }
                        });
                        if b.goal > 0 {
                            ui.add_space(10.0);
                            let frac = stats.words as f32 / b.goal as f32;
                            progress(ui, p, frac, 420.0, 8.0);
                            let mut extra = String::new();
                            if let Ok(d) = chrono::NaiveDate::parse_from_str(b.deadline.get(..10).unwrap_or(""), "%Y-%m-%d") {
                                let days = (d - chrono::Local::now().date_naive()).num_days();
                                if days > 0 {
                                    extra = format!(" · faltam {days} dias · {} palavras/dia", fmt_n((b.goal - stats.words as i64).max(0) / days));
                                } else {
                                    extra = " · prazo vencido".into();
                                }
                            }
                            ui.label(RichText::new(format!("{:.0}% de {} palavras{extra}", (frac * 100.0).min(100.0), fmt_n(b.goal))).small().color(p.muted));
                        }
                        ui.label(RichText::new(format!("Criado {} · Modificado {}", fmt_date(b.created), fmt_date(b.modified))).small().color(p.faint));
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add(egui::Button::new(RichText::new("✒  Continuar escrevendo").color(Color32::WHITE)).fill(p.accent)).clicked() {
                                acts.push(Action::Continue(b.path.clone()));
                            }
                            if ui.button("Editar nota do livro").clicked() {
                                acts.push(Action::Open(b.path.clone(), false));
                            }
                            if ui.button("Novo capítulo").clicked() {
                                acts.push(Action::NewChapter(b.path.clone()));
                            }
                            if ui.button("Novo personagem").clicked() {
                                acts.push(Action::NewCharacter(b.path.clone()));
                            }
                        });
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            for (icon, label, view) in [("☰", "Manuscrito", "binder")] {
                                let r = ui.add(egui::Button::new(RichText::new(format!("{icon}  {label}")).size(15.0)).min_size(egui::vec2(140.0, 40.0)));
                                if r.clicked() {
                                    acts.push(Action::View(view.into(), Some(b.path.clone())));
                                }
                            }
                        });
                    });
                });

                // the last fortnight
                ui.add_space(26.0);
                ui.label(RichText::new("ÚLTIMOS 14 DIAS").small().color(p.faint));
                let days = cx.sc.settings.last_days(14);
                let max = days.iter().map(|d| d.1).max().unwrap_or(0).max(1) as f32;
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0 * 34.0, 90.0), Sense::hover());
                let pointer = ui.ctx().pointer_hover_pos();
                for (i, (day, w)) in days.iter().enumerate() {
                    let x = r.left() + i as f32 * 34.0;
                    let h = (*w as f32 / max * 64.0).max(2.0);
                    let bar = egui::Rect::from_min_max(egui::pos2(x + 6.0, r.top() + 70.0 - h), egui::pos2(x + 28.0, r.top() + 70.0));
                    ui.painter().rect_filled(bar, 3.0, if *w > 0 { p.accent } else { p.border });
                    ui.painter().text(egui::pos2(x + 17.0, r.top() + 82.0), egui::Align2::CENTER_CENTER, &day[8..], egui::FontId::proportional(11.0), p.faint);
                    if pointer.is_some_and(|pt| bar.expand2(egui::vec2(4.0, 70.0)).contains(pt)) {
                        let d: Vec<&str> = day.split('-').rev().collect();
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new(("day", i)), |ui| {
                            ui.label(format!("{}: {} palavras", d.join("/"), fmt_n(*w)));
                        });
                    }
                }

                // the shelf: one row per series, single books last
                ui.add_space(24.0);
                ui.label(RichText::new(format!("ESTANTE · {} {}", books.len(), if books.len() == 1 { "LIVRO" } else { "LIVROS" })).small().color(p.faint));
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
                            ui.label(RichText::new(if k.is_empty() { "Livros avulsos".to_string() } else { k.clone() }).strong().color(p.text));
                            ui.label(RichText::new(format!("{} {}", bs.len(), if bs.len() == 1 { "livro" } else { "livros" })).small().color(p.muted));
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(22.0, 18.0);
                        for b in bs {
                            let st = cx.sc.stats(&ix, b);
                            let selected = b.path == sel;
                            let r = egui::Frame::new().inner_margin(6.0).corner_radius(6.0).fill(if selected { p.bg_secondary } else { Color32::TRANSPARENT }).show(ui, |ui| {
                                ui.set_width(130.0);
                                ui.vertical(|ui| {
                                    let c = cover(ui, cx, b, egui::vec2(130.0, 195.0));
                                    let vol = if !k.is_empty() && !b.volume.is_empty() { format!("{} · ", b.volume) } else { String::new() };
                                    ui.add(egui::Label::new(RichText::new(format!("{vol}{}", b.title)).strong().color(p.text)).truncate());
                                    ui.add(egui::Label::new(RichText::new(format!("{} · {} cap. · {} pal.", if b.author.is_empty() { "—" } else { &b.author }, st.chapters, fmt_n(st.words as i64))).small().color(p.muted)).truncate());
                                    if b.goal > 0 && st.words > 0 {
                                        progress(ui, p, st.words as f32 / b.goal as f32, 130.0, 4.0);
                                    }
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
        acts
    }
}
