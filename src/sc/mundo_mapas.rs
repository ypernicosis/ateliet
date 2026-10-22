//! Mundo › Mapas: the world's interactive maps (each a note that opens in
//! the map view) and its pictures, any of which can become a new map.

use super::library::Cx;
use super::mapa_ui::{is_image, name_of};
use super::mundo_ui::MundoView;
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::Book;
use ateliet_core::scriptorium::enredo::{self as en, str_of};
use ateliet_core::scriptorium::mapa as ma;
use ateliet_core::scriptorium::mundo as mu;
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense};
use serde_json::Value as Json;
use std::path::Path;
use std::time::{Duration, Instant};

pub struct NewMap {
    pub name: String,
    pub image: Option<String>,
    pub filter: String,
    focus: bool,
}

impl NewMap {
    pub fn new(image: Option<String>) -> NewMap {
        let name = image.as_deref().map(|i| name_of(i).rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| name_of(i)).replace(['_', '-'], " ")).unwrap_or_default();
        NewMap { name, image, filter: String::new(), focus: true }
    }
}

impl MundoView {
    fn refresh_maps(&mut self, root: &Path, ix: &Index, b: &Book, author: &str) {
        if self.maps.as_ref().is_some_and(|m| m.0.elapsed() < Duration::from_secs(4)) {
            return;
        }
        let saga = en::saga_books(ix, b, author);
        let list = ma::list(ix, &saga).into_iter().map(|p| {
            let d = ma::load(root, &p);
            (p, d)
        });
        self.maps = Some((Instant::now(), list.collect()));
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn mapas(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, root: &Path, ix: &Index, b: &Book, author: &str, acts: &mut Vec<Action>) {
        self.refresh_maps(root, ix, b, author);
        let maps = self.maps.as_ref().map(|m| m.1.clone()).unwrap_or_default();
        let images: Vec<String> = self.codex.as_ref().map(|c| c.images.iter().filter(|i| !i.legado).map(|i| i.path.clone()).collect()).unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("mu-maps").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 18, bottom: 24 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Mapas interativos").font(kit::medium(15.0)).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::button(ui, Some(icons::PLUS), "Novo mapa", p).clicked() {
                            self.new_map = Some(NewMap::new(None));
                        }
                    });
                });
                ui.add_space(10.0);
                let current: Vec<&(String, Json)> = maps.iter().filter(|m| m.1["legado"] != true).collect();
                let old: Vec<&(String, Json)> = maps.iter().filter(|m| m.1["legado"] == true).collect();
                if current.is_empty() {
                    ui.label(RichText::new("Nenhum mapa ainda. Escolha uma imagem do mundo abaixo, ou crie um com qualquer imagem do cofre.").color(p.muted));
                }
                self.map_grid(ui, p, cx, &current, acts);
                if !old.is_empty() {
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Versões anteriores").font(kit::medium(14.0)).color(p.text));
                        ui.label(RichText::new("guardadas para consulta; o cânone prevalece").size(12.0).color(p.faint));
                    });
                    ui.add_space(8.0);
                    self.map_grid(ui, p, cx, &old, acts);
                }
                if !images.is_empty() {
                    ui.add_space(22.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Imagens do mundo").font(kit::medium(14.0)).color(p.text));
                        ui.label(RichText::new("clique para fazer de uma delas um mapa interativo").size(12.0).color(p.faint));
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                        for im in &images {
                            let (rect, r) = ui.allocate_exact_size(vec2(150.0, 124.0), Sense::click());
                            let th = Rect::from_min_size(rect.min, vec2(150.0, 98.0));
                            thumb(ui, cx, im, th, p);
                            if r.hovered() {
                                ui.painter().rect_stroke(th, 8.0, egui::Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Inside);
                            }
                            let name = name_of(im);
                            let g = super::enredo_ui::truncated(ui, &name, egui::FontId::proportional(11.5), p.muted, 150.0);
                            ui.painter().galley(pos2(rect.left(), th.bottom() + 6.0), g, p.muted);
                            if r.on_hover_text(format!("{name}\nCriar um mapa interativo com esta imagem")).clicked() {
                                self.new_map = Some(NewMap::new(Some(im.clone())));
                            }
                        }
                    });
                }
            });
        });
    }

    fn map_grid(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, maps: &[&(String, Json)], acts: &mut Vec<Action>) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
            for (path, d) in maps {
                let (rect, r) = ui.allocate_exact_size(vec2(236.0, 202.0), Sense::click());
                let hov = r.hovered();
                ui.painter().rect(rect, 12.0, p.bg, egui::Stroke::new(1.0_f32, if hov { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
                let th = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(rect.width() - 12.0, 136.0));
                let img = str_of(d, "imagem");
                let shown = cx.vault.resolve(img, path).unwrap_or_else(|| img.to_string());
                thumb(ui, cx, &shown, th, p);
                let pins = d["camadas"].as_array().map(|a| a.iter().filter(|l| l["t"] == "pino").count()).unwrap_or(0);
                let subs = d["camadas"].as_array().map(|a| a.iter().filter(|l| !str_of(l, "submapa").is_empty()).count()).unwrap_or(0);
                let mut meta = vec![format!("{pins} {}", if pins == 1 { "pino" } else { "pinos" })];
                if subs > 0 {
                    meta.push(format!("{subs} {}", if subs == 1 { "submapa" } else { "submapas" }));
                }
                if d["escala"].is_object() {
                    meta.push(if d["escala"]["proj"] == "mollweide" { "escala esférica".into() } else { "com escala".into() });
                }
                let name = name_of(path);
                let g = super::enredo_ui::truncated(ui, &name, kit::medium(13.5), p.text, rect.width() - 24.0);
                ui.painter().galley(pos2(rect.left() + 12.0, th.bottom() + 10.0), g, p.text);
                ui.painter().text(pos2(rect.left() + 12.0, th.bottom() + 40.0), egui::Align2::LEFT_CENTER, meta.join(" · "), egui::FontId::proportional(11.5), p.faint);
                if r.clicked() {
                    acts.push(Action::Open(path.clone(), true));
                }
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn new_map_dialog(&mut self, ctx: &egui::Context, p: Palette, root: &Path, ix: &Index, b: &Book, author: &str, acts: &mut Vec<Action>) {
        let Some(nm) = self.new_map.as_mut() else { return };
        let mut done = None;
        let images: Vec<String> = if nm.image.is_none() {
            let f = mu::deaccent(&nm.filter);
            let mut v: Vec<String> = ix.file_paths().filter(|x| is_image(x)).filter(|x| f.is_empty() || mu::deaccent(x).contains(&f)).cloned().collect();
            v.sort();
            v
        } else {
            Vec::new()
        };
        kit::scrim(ctx, p);
        egui::Window::new("mu-new-map").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Novo mapa interativo").font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            ui.label(RichText::new("Nome do mapa").size(12.0).color(p.muted));
            let r = ui.add(egui::TextEdit::singleline(&mut nm.name).desired_width(f32::INFINITY).hint_text(RichText::new("Ex.: Continente de Álfheim").color(p.faint)).margin(egui::Margin::symmetric(8, 6)));
            if nm.focus {
                r.request_focus();
                if r.has_focus() {
                    nm.focus = false;
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Imagem base").size(12.0).color(p.muted));
            match nm.image.clone() {
                Some(im) => {
                    ui.horizontal(|ui| {
                        ui.label(kit::icon(icons::IMAGE, 14.0).color(p.muted));
                        ui.label(RichText::new(name_of(&im)).color(p.text));
                        if kit::quiet(ui, None, "Trocar", p).clicked() {
                            nm.image = None;
                        }
                    });
                }
                None => {
                    ui.add(egui::TextEdit::singleline(&mut nm.filter).hint_text(RichText::new("Filtrar imagens").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        if images.is_empty() {
                            ui.label(RichText::new("Nenhuma imagem no cofre.").color(p.faint));
                        }
                        for im in images.iter().take(300) {
                            if kit::Row::new(im).icon(icons::IMAGE).show(ui, p).clicked() {
                                if nm.name.trim().is_empty() {
                                    nm.name = NewMap::new(Some(im.clone())).name;
                                }
                                nm.image = Some(im.clone());
                            }
                        }
                    });
                }
            }
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = nm.image.is_some();
                if ui.add_enabled_ui(ok, |ui| kit::primary(ui, "Criar mapa", p)).inner.clicked() || (ok && kit::cmd_enter(ui.ctx())) {
                    done = Some(true);
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                let nm = self.new_map.take().unwrap();
                let home = mu::home(root, ix, b, author);
                let w = self.world().map(|w| (str_of(w, "id").to_string(), str_of(w, "nome").to_string()));
                match ma::create(root, ix, &home, &nm.name, nm.image.as_deref().unwrap_or(""), w.as_ref().map(|w| (w.0.as_str(), w.1.as_str()))) {
                    Ok(path) => {
                        self.maps = None;
                        acts.push(Action::Saved(path.clone()));
                        acts.push(Action::Open(path, true));
                    }
                    Err(e) => self.toast = Some(format!("Não foi possível criar o mapa: {e}")),
                }
            }
            Some(false) => self.new_map = None,
            None => {}
        }
    }
}

fn thumb(ui: &mut egui::Ui, cx: &mut Cx, path: &str, r: Rect, p: Palette) {
    match crate::host::image(ui.ctx(), cx.vault, cx.caches, path, "") {
        Some((id, sz)) => {
            let k = (r.width() / sz.x).max(r.height() / sz.y);
            let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(r.width() / (sz.x * k), r.height() / (sz.y * k)));
            ui.painter().add(egui::epaint::RectShape::filled(r, 8.0, Color32::WHITE).with_texture(id, uv));
        }
        None => {
            ui.painter().rect_filled(r, 8.0, p.bg_active);
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, icons::MAP, kit::icon_font(22.0), p.faint);
        }
    }
}
