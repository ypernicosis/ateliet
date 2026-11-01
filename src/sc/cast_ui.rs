//! The cast: every character of a book as a card (portrait, name, role, line).

use super::library::Cx;
use super::sheet_ui::parse_color;
use super::Action;
use crate::host;
use crate::icons;
use crate::kit;
use ateliet_core::scriptorium::book;
use ateliet_core::scriptorium::sheet::initials;
use eframe::egui::{self, Color32, RichText, Sense};
use serde_json::Value as Json;

pub struct CastView {
    pub book: String,
    query: String,
}

impl CastView {
    pub fn new(book: &str) -> CastView {
        CastView { book: book.into(), query: String::new() }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let ix_lock = cx.vault.index.clone();
        let ix = ix_lock.read().unwrap();
        let author = cx.sc.author();
        let Some(b) = book::book_by_path(&ix, &self.book, &author) else {
            ui.label("Livro não encontrado.");
            return acts;
        };
        let chars = book::characters(&ix, &b);
        egui::ScrollArea::vertical().id_salt("sc-cast").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let w = avail.min(1080.0);
            let side = ((avail - w) / 2.0).max(0.0) + 40.0;
            ui.add_space(36.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width((w - 80.0).max(300.0));
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Elenco").font(kit::bold(26.0)).color(p.text));
                            ui.label(RichText::new(format!("{} · {} {}", b.title, chars.len(), if chars.len() == 1 { "personagem" } else { "personagens" })).color(p.muted));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if kit::primary_icon(ui, icons::USER_PLUS, "Personagem", p).clicked() {
                                acts.push(Action::NewCharacter(b.path.clone()));
                            }
                            if kit::button(ui, Some(icons::LAYOUT_TEMPLATE), "Modelo de ficha", p).on_hover_text("As fichas novas copiam as propriedades e as seções deste modelo").clicked() {
                                acts.push(Action::CharacterModel(b.path.clone()));
                            }
                            ui.allocate_ui(egui::vec2(220.0, 30.0), |ui| {
                                kit::search_field(ui, &mut self.query, "Buscar", p);
                            });
                        });
                    });
                    ui.add_space(26.0);
                    let q = self.query.to_lowercase();
                    let shown: Vec<&String> = chars.iter().filter(|c| q.is_empty() || c.to_lowercase().contains(&q)).collect();
                    if shown.is_empty() {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(kit::icon(icons::USERS, 30.0).color(p.faint));
                            ui.add_space(10.0);
                            ui.label(RichText::new(if chars.is_empty() { "Nenhum personagem ainda" } else { "Ninguém com esse nome" }).font(kit::medium(15.0)).color(p.muted));
                        });
                        return;
                    }
                    let card_w = 180.0;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(20.0, 22.0);
                        for c in shown {
                            let Some(pg) = ix.page(c) else { continue };
                            let name = c.rsplit('/').next().unwrap_or(c).trim_end_matches(".md").to_string();
                            let fm = &pg.yaml;
                            let s = |k: &str| fm.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let color = parse_color(&s("cor"));
                            let resp = ui.allocate_ui_with_layout(egui::vec2(card_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.set_width(card_w);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                let (rect, r) = ui.allocate_exact_size(egui::vec2(card_w, 230.0), Sense::click());
                                let retrato = ateliet_core::scriptorium::link_text(fm.get("retrato").unwrap_or(&Json::Null));
                                match if retrato.is_empty() { None } else { host::image(ui.ctx(), cx.vault, cx.caches, &retrato, c) } {
                                    Some((id, sz)) => {
                                        let k = (rect.width() / sz.x).max(rect.height() / sz.y);
                                        let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(rect.width() / (sz.x * k), rect.height() / (sz.y * k)));
                                        ui.painter().add(egui::epaint::RectShape::filled(rect, 12.0, Color32::WHITE).with_texture(id, uv));
                                    }
                                    None => {
                                        ui.painter().rect_filled(rect, 12.0, color.map(|c| c.gamma_multiply(0.85)).unwrap_or(p.bg_secondary));
                                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initials(&name), kit::medium(44.0), if color.is_some() { Color32::WHITE } else { p.faint });
                                    }
                                }
                                if r.hovered() {
                                    ui.painter().rect_stroke(rect, 12.0, egui::Stroke::new(2.0_f32, color.unwrap_or(p.accent_strong)), egui::StrokeKind::Inside);
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                ui.add_space(8.0);
                                ui.add(egui::Label::new(RichText::new(&name).font(kit::medium(14.0)).color(p.text)).truncate());
                                let role = s("papel");
                                if !role.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(role).size(12.5).color(p.muted)).truncate());
                                }
                                let frase = s("frase");
                                if !frase.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(format!("“{frase}”")).font(egui::FontId::new(12.5, egui::FontFamily::Name("italic".into()))).color(p.faint)).wrap());
                                }
                                r
                            });
                            if resp.inner.clicked() {
                                acts.push(Action::Open(c.clone(), ui.input(|i| i.modifiers.command)));
                            }
                        }
                    });
                });
            });
            ui.add_space(40.0);
        });
        acts
    }
}
