//! The manuscript panel: the chapters of the current book in reading order,
//! with status, part, word counts; drag to reorder, right-click for more.

use super::library::Cx;
use super::{color, Action};
use crate::icons;
use crate::kit;
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
            if kit::menu_item(ui, icons::BOOK_PLUS, "Novo livro", p) {
                acts.push(Action::NewBook);
            }
            return acts;
        };
        let books = book::books(&ix, &author);
        ui.add_space(2.0);
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
            ui.label(RichText::new(&book.title).font(kit::medium(15.0)).color(p.text));
        }
        let stats = cx.sc.stats(&ix, &book);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} palavras · {} hoje", fmt_n(stats.words as i64), fmt_n(cx.sc.settings.today_words(Some(&book.path))))).size(12.0).color(p.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if kit::icon_button_sized(ui, icons::DOWNLOAD, "Exportar", false, p, 24.0, 14.0).clicked() {
                    acts.push(Action::Export(book.path.clone()));
                }
                if kit::icon_button_sized(ui, icons::WAYPOINTS, "Enredo", false, p, 24.0, 14.0).clicked() {
                    acts.push(Action::View("enredo".into(), Some(book.path.clone())));
                }
                if kit::icon_button_sized(ui, icons::USERS, "Elenco", false, p, 24.0, 14.0).clicked() {
                    acts.push(Action::View("cast".into(), Some(book.path.clone())));
                }
                if kit::icon_button_sized(ui, icons::USER_PLUS, "Novo personagem", false, p, 24.0, 14.0).clicked() {
                    acts.push(Action::NewCharacter(book.path.clone()));
                }
                if kit::icon_button_sized(ui, icons::LIBRARY_BIG, "Biblioteca", false, p, 24.0, 14.0).clicked() {
                    acts.push(Action::View("library".into(), Some(book.path.clone())));
                }
                if kit::icon_button_sized(ui, icons::FILE_PLUS, "Novo capítulo", false, p, 24.0, 14.0).clicked() {
                    acts.push(Action::NewChapter(book.path.clone()));
                }
            });
        });
        if book.goal > 0 {
            ui.add_space(4.0);
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), Sense::hover());
            ui.painter().rect_filled(r, 1.5, p.bg_active);
            let mut f = r;
            f.set_width((r.width() * (stats.words as f32 / book.goal as f32).min(1.0)).max(3.0));
            ui.painter().rect_filled(f, 1.5, p.accent_strong);
        }
        ui.add_space(12.0);

        let files: Vec<String> = stats.per.iter().map(|c| c.info.path.clone()).collect();
        let mut last_part = String::new();
        let mut n = 0;
        let mut drop: Option<(usize, usize)> = None;
        egui::ScrollArea::vertical().id_salt("binder").auto_shrink([false, false]).show(ui, |ui| {
            for (idx, c) in stats.per.iter().enumerate() {
                let info = &c.info;
                if !info.part.is_empty() && info.part != last_part {
                    ui.add_space(6.0);
                    kit::section(ui, &info.part, p);
                }
                last_part = info.part.clone();
                if info.numbered && info.include {
                    n += 1;
                }
                let is_active = active == Some(info.path.as_str());
                let id = egui::Id::new(("binder-row", &info.path));
                let words = if info.goal > 0 { format!("{}/{}", fmt_n(c.words as i64), fmt_n(info.goal)) } else { fmt_n(c.words as i64) };
                let num = if info.numbered { n.to_string() } else { match info.tipo.as_str() { "prologo" => "P", "epilogo" => "E", "interludio" => "I", _ => "·" }.to_string() };
                let label = format!("{num}   {}", info.title);
                let row = ui.dnd_drag_source(id, idx, |ui| {
                    kit::Row::new(&label).dot(color(book::status_color(&info.status))).selected(is_active).dim(!info.include).trailing(words.clone()).show(ui, p)
                });
                let r = ui.interact(row.response.rect, id.with("click"), Sense::click());
                // the status dot cycles the status
                let dot = egui::Rect::from_center_size(egui::pos2(row.response.rect.left() + 12.0, row.response.rect.center().y), egui::vec2(16.0, 16.0));
                let dr = ui.interact(dot, id.with("dot"), Sense::click()).on_hover_text(format!("{} (clique para mudar)", book::status_label(&info.status)));
                if dr.clicked() {
                    let i = STATUS.iter().position(|s| s.0 == info.status).unwrap_or(1);
                    acts.push(Action::SetFm(info.path.clone(), "status".into(), Some(json!(STATUS[(i + 1) % STATUS.len()].0))));
                }
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
                    if kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir em nova aba", p) {
                        acts.push(Action::Open(info.path.clone(), true));
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, icons::PENCIL, "Renomear…", p) {
                        acts.push(Action::RenameChapter(info.path.clone(), info.title.clone()));
                        ui.close_menu();
                    }
                    kit::menu_sep(ui, p);
                    for (k, label, _) in STATUS {
                        if kit::menu_check(ui, info.status == k, &format!("Status: {label}"), p) {
                            acts.push(Action::SetFm(info.path.clone(), "status".into(), Some(json!(k))));
                            ui.close_menu();
                        }
                    }
                    kit::menu_sep(ui, p);
                    if kit::menu_item(ui, icons::BOOKMARK, "Definir parte / arco…", p) {
                        acts.push(Action::ChapterPart(info.path.clone(), info.part.clone()));
                        ui.close_menu();
                    }
                    if kit::menu_item(ui, if info.include { icons::EYE_OFF } else { icons::EYE }, if info.include { "Excluir da exportação" } else { "Incluir na exportação" }, p) {
                        acts.push(Action::SetFm(info.path.clone(), "incluir".into(), if info.include { Some(Json::Bool(false)) } else { None }));
                        ui.close_menu();
                    }
                    if kit::menu_entry(ui, icons::ARROW_UP, "Subir", "", idx > 0, false, p) {
                        let mut a = files.clone();
                        a.swap(idx - 1, idx);
                        acts.push(Action::Reorder(a));
                        ui.close_menu();
                    }
                    if kit::menu_entry(ui, icons::ARROW_DOWN, "Descer", "", idx + 1 < files.len(), false, p) {
                        let mut a = files.clone();
                        a.swap(idx + 1, idx);
                        acts.push(Action::Reorder(a));
                        ui.close_menu();
                    }
                    kit::menu_sep(ui, p);
                    if kit::menu_danger(ui, icons::TRASH_2, "Mover para a lixeira", p) {
                        acts.push(Action::Trash(info.path.clone()));
                        ui.close_menu();
                    }
                });
            }
            ui.add_space(8.0);
            if kit::quiet(ui, Some(icons::PLUS), "Novo capítulo", p).clicked() {
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
