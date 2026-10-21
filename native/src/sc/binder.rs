//! The manuscript panel: the chapters of the current book in reading order,
//! with status, part, word counts; drag to reorder, right-click for more.

use super::library::Cx;
use super::{color, Action};
use ateliet_core::scriptorium::book::{self, STATUS};
use ateliet_core::scriptorium::fmt_n;
use eframe::egui::{self, RichText, Sense};
use serde_json::{json, Value as Json};

#[derive(Default)]
pub struct Binder {
    /// A book picked in the panel (else the book of the open note).
    pub book: Option<String>,
}

impl Binder {
    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx, active: Option<&str>) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let ix_lock = cx.vault.index.clone();
        let ix = ix_lock.read().unwrap();
        let author = cx.sc.author();
        let from_active = active.and_then(|a| book::book_for(&ix, a, &author));
        if let Some(b) = &from_active {
            self.book = Some(b.path.clone());
        }
        let book = from_active.or_else(|| self.book.as_deref().and_then(|b| book::book_by_path(&ix, b, &author))).or_else(|| cx.sc.current_book(&ix, None));
        let Some(book) = book else {
            ui.label(RichText::new("Nenhum livro ainda.").color(p.muted));
            if ui.button("Novo livro").clicked() {
                acts.push(Action::NewBook);
            }
            return acts;
        };
        let books = book::books(&ix, &author);
        if books.len() > 1 {
            let mut pick = book.path.clone();
            egui::ComboBox::from_id_salt("binder-book").selected_text(book.title.clone()).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                for b in &books {
                    ui.selectable_value(&mut pick, b.path.clone(), &b.title);
                }
            });
            if pick != book.path {
                self.book = Some(pick.clone());
                acts.push(Action::SelectBook(pick));
            }
        } else {
            ui.label(RichText::new(&book.title).strong().size(16.0).color(p.text));
        }
        let stats = cx.sc.stats(&ix, &book);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} palavras", fmt_n(stats.words as i64))).color(p.text));
            ui.label(RichText::new(format!("· hoje {}", fmt_n(cx.sc.settings.today_words(Some(&book.path))))).color(p.muted));
        });
        if book.goal > 0 {
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, p.bg);
            let mut f = r;
            f.set_width(r.width() * (stats.words as f32 / book.goal as f32).min(1.0));
            ui.painter().rect_filled(f, 2.0, p.accent);
        }
        ui.horizontal(|ui| {
            if ui.small_button("+ Capítulo").on_hover_text("Novo capítulo").clicked() {
                acts.push(Action::NewChapter(book.path.clone()));
            }
            if ui.small_button("📚").on_hover_text("Biblioteca").clicked() {
                acts.push(Action::View("library".into(), Some(book.path.clone())));
            }
            if ui.small_button("👤").on_hover_text("Novo personagem").clicked() {
                acts.push(Action::NewCharacter(book.path.clone()));
            }
        });
        ui.separator();

        let files: Vec<String> = stats.per.iter().map(|c| c.info.path.clone()).collect();
        let mut last_part = String::new();
        let mut n = 0;
        let mut drop: Option<(usize, usize)> = None;
        egui::ScrollArea::vertical().id_salt("binder").auto_shrink([false, false]).show(ui, |ui| {
            for (idx, c) in stats.per.iter().enumerate() {
                let info = &c.info;
                if !info.part.is_empty() && info.part != last_part {
                    ui.add_space(6.0);
                    ui.label(RichText::new(info.part.to_uppercase()).small().strong().color(p.faint));
                }
                last_part = info.part.clone();
                if info.numbered && info.include {
                    n += 1;
                }
                let is_active = active == Some(info.path.as_str());
                let id = egui::Id::new(("binder-row", &info.path));
                let row = ui.dnd_drag_source(id, idx, |ui| {
                    egui::Frame::new().fill(if is_active { p.bg } else { egui::Color32::TRANSPARENT }).corner_radius(4.0).inner_margin(egui::Margin::symmetric(4, 3)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (dot, dr) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::click());
                            ui.painter().circle_filled(dot.center(), 4.5, color(book::status_color(&info.status)));
                            if dr.on_hover_text(format!("{} (clique para mudar)", book::status_label(&info.status))).clicked() {
                                let i = STATUS.iter().position(|s| s.0 == info.status).unwrap_or(1);
                                acts.push(Action::SetFm(info.path.clone(), "status".into(), Some(json!(STATUS[(i + 1) % STATUS.len()].0))));
                            }
                            let num = if info.numbered { n.to_string() } else { match info.tipo.as_str() { "prologo" => "P", "epilogo" => "E", "interludio" => "I", _ => "·" }.to_string() };
                            ui.label(RichText::new(num).small().color(p.faint));
                            let words = if info.goal > 0 { format!("{}/{}", fmt_n(c.words as i64), fmt_n(info.goal)) } else { fmt_n(c.words as i64) };
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(words).small().color(p.faint));
                                let t = RichText::new(&info.title).color(if !info.include { p.faint } else if is_active { p.text } else { p.muted });
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.add(egui::Label::new(if info.include { t } else { t.strikethrough() }).truncate());
                                });
                            });
                        });
                    });
                });
                let r = ui.interact(row.response.rect, id.with("click"), Sense::click());
                if r.clicked() {
                    acts.push(Action::Open(info.path.clone(), ui.input(|i| i.modifiers.command)));
                }
                if let Some(from) = row.response.dnd_release_payload::<usize>() {
                    drop = Some((*from, idx));
                }
                if row.response.dnd_hover_payload::<usize>().is_some() {
                    let y = row.response.rect.top();
                    ui.painter().hline(row.response.rect.x_range(), y, egui::Stroke::new(2.0_f32, p.accent));
                }
                r.context_menu(|ui| {
                    if ui.button("Abrir em nova aba").clicked() {
                        acts.push(Action::Open(info.path.clone(), true));
                        ui.close_menu();
                    }
                    if ui.button("Renomear…").clicked() {
                        acts.push(Action::RenameChapter(info.path.clone(), info.title.clone()));
                        ui.close_menu();
                    }
                    ui.separator();
                    for (k, label, _) in STATUS {
                        if ui.radio(info.status == k, format!("Status: {label}")).clicked() {
                            acts.push(Action::SetFm(info.path.clone(), "status".into(), Some(json!(k))));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Definir parte / arco…").clicked() {
                        acts.push(Action::ChapterPart(info.path.clone(), info.part.clone()));
                        ui.close_menu();
                    }
                    if ui.button(if info.include { "Excluir da exportação" } else { "Incluir na exportação" }).clicked() {
                        acts.push(Action::SetFm(info.path.clone(), "incluir".into(), if info.include { Some(Json::Bool(false)) } else { None }));
                        ui.close_menu();
                    }
                    if ui.add_enabled(idx > 0, egui::Button::new("Subir")).clicked() {
                        let mut a = files.clone();
                        a.swap(idx - 1, idx);
                        acts.push(Action::Reorder(a));
                        ui.close_menu();
                    }
                    if ui.add_enabled(idx + 1 < files.len(), egui::Button::new("Descer")).clicked() {
                        let mut a = files.clone();
                        a.swap(idx + 1, idx);
                        acts.push(Action::Reorder(a));
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Mover para a lixeira").clicked() {
                        acts.push(Action::Trash(info.path.clone()));
                        ui.close_menu();
                    }
                });
            }
            ui.add_space(8.0);
            if ui.add(egui::Button::new(RichText::new("+ Novo capítulo").color(p.muted)).frame(false)).clicked() {
                acts.push(Action::NewChapter(book.path.clone()));
            }
        });
        if let Some((from, to)) = drop {
            if from != to {
                let mut a = files.clone();
                let f = a.remove(from);
                a.insert(to, f);
                acts.push(Action::Reorder(a));
            }
        }
        acts
    }
}
