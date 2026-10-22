//! Mundo, native (continued): Tempo (calendar, date converter, eras),
//! Genealogias (family trees), Línguas (dictionary) and the dialogs.

use super::mundo_ui::{vault_folders, CalEdit, EraEdit, MundoView, NotePick};
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::Book;
use ateliet_core::scriptorium::enredo::{self as en, str_of};
use ateliet_core::scriptorium::mundo::{self as mu, Calendar};
use ateliet_core::scriptorium::fmt_n;
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::path::Path;

const ERA_CORES: [&str; 8] = ["#8a6d3b", "#5d7a3a", "#2f6b6b", "#4b5bb3", "#7a2e3b", "#a8641b", "#6b4f8a", "#3f9f98"];

fn card(ui: &mut egui::Ui, p: Palette, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().stroke(Stroke::new(1.0_f32, p.border)).corner_radius(12.0).inner_margin(18.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        body(ui);
    });
}

fn card_head(ui: &mut egui::Ui, p: Palette, title: &str, actions: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(kit::medium(16.0)).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
    });
    ui.add_space(8.0);
}

impl MundoView {
    // ── tempo ─────────────────────────────────────────────────────────────

    pub(super) fn tempo(&mut self, ui: &mut egui::Ui, p: Palette, _root: &Path, _ix: &Index, _b: &Book) {
        let Some(w) = self.world().cloned() else { return };
        egui::ScrollArea::vertical().id_salt("mu-tempo").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let width = (avail - 48.0).min(980.0);
            ui.add_space(20.0);
            ui.horizontal_top(|ui| {
                ui.add_space(((avail - width) / 2.0).max(16.0));
                ui.vertical(|ui| {
                    ui.set_width(width);
                    self.calendar_card(ui, p, &w);
                    ui.add_space(16.0);
                    self.eras_card(ui, p, &w);
                    ui.add_space(40.0);
                });
            });
        });
    }

    fn calendar_card(&mut self, ui: &mut egui::Ui, p: Palette, w: &Json) {
        let c = w["calendario"].clone();
        card(ui, p, |ui| {
            card_head(ui, p, c["nome"].as_str().filter(|s| !s.is_empty()).unwrap_or("Calendário"), |ui| {
                if kit::button(ui, Some(icons::PENCIL), if c.is_object() { "Editar" } else { "Criar" }, p).clicked() {
                    self.cal_edit = Some(cal_edit_of(&c));
                }
                if kit::quiet(ui, Some(icons::FILE_SEARCH), "Ler de uma nota…", p).clicked() {
                    self.note_pick = Some((NotePick::Calendar, String::new()));
                }
            });
            if !c.is_object() {
                ui.label(RichText::new("Sem calendário ainda. Leia de uma nota (procuro tabelas de meses e de dias da semana) ou crie um simples.").color(p.muted));
                return;
            }
            let cal = Calendar { c: &c };
            let months = cal.months();
            let ic = &c["intercalar"];
            // the months, as a grid of small cards
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                let month = |ui: &mut egui::Ui, n: &str, nome: &str, local: &str, dias: i64, inter: bool, tip: &str| {
                    let (r, resp) = ui.allocate_exact_size(vec2(150.0, 60.0), Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(r, 8.0, if inter { p.accent_soft } else { p.bg_secondary });
                    painter.text(r.left_top() + vec2(10.0, 8.0), egui::Align2::LEFT_TOP, n, egui::FontId::proportional(11.0), p.faint);
                    painter.text(r.right_top() + vec2(-10.0, 8.0), egui::Align2::RIGHT_TOP, format!("{dias} d"), egui::FontId::proportional(11.0), p.faint);
                    let g = super::enredo_ui::truncated(ui, nome, kit::medium(13.0), p.text, r.width() - 20.0);
                    painter.galley(r.left_top() + vec2(10.0, 22.0), g, p.text);
                    if !local.is_empty() {
                        let g = super::enredo_ui::truncated(ui, local, egui::FontId::new(11.5, egui::FontFamily::Name("italic".into())), p.muted, r.width() - 20.0);
                        painter.galley(r.left_top() + vec2(10.0, 40.0), g, p.muted);
                    }
                    if !tip.is_empty() {
                        resp.on_hover_text(tip);
                    }
                };
                for (i, m) in months.iter().enumerate() {
                    month(ui, &(i + 1).to_string(), &m.nome, &m.local, m.dias, false, &m.ancora);
                    if ic.is_object() && ic["depois"].as_i64() == Some(i as i64 + 1) {
                        let anos: Vec<String> = ic["anos"].as_array().into_iter().flatten().map(|x| x.to_string()).collect();
                        month(ui, "+", str_of(ic, "nome"), str_of(ic, "local"), ic["dias"].as_i64().unwrap_or(30), true, &format!("Intercalar: anos {} de cada ciclo de {}", anos.join(", "), ic["ciclo"]));
                    }
                }
            });
            ui.add_space(12.0);
            let fact = |ui: &mut egui::Ui, k: &str, v: &str| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(vec2(170.0, 18.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.label(RichText::new(k).size(12.5).color(p.muted));
                    });
                    ui.add(egui::Label::new(RichText::new(v).size(13.0).color(p.text)).wrap());
                });
            };
            let sem: Vec<String> = c["semana"].as_array().into_iter().flatten().map(|s| format!("{}{}", str_of(s, "nome"), if str_of(s, "local").is_empty() { String::new() } else { format!(" ({})", str_of(s, "local")) })).collect();
            if !sem.is_empty() {
                let por_mes = c["semanaPorMes"].as_bool().unwrap_or(false);
                let k = if por_mes { format!("{} (recomeça a cada mês)", capitalize(c["semanaNome"].as_str().filter(|s| !s.is_empty()).unwrap_or("semana"))) } else { "Semana".into() };
                let extra = c["diaExtra"].as_str().filter(|s| !s.is_empty()).map(|x| format!(" · 30º dia: {x}")).unwrap_or_default();
                fact(ui, &k, &format!("{}{extra}", sem.join(" · ")));
            }
            let (cy_years, cy_days) = cal.cycle();
            if ic.is_object() && cy_years > 1 {
                let leap_y = ic["anos"].get(0).and_then(|x| x.as_i64()).map(|a| a - 1).unwrap_or(0);
                let common = (0..cy_years).find(|&y| !cal.is_leap(y)).unwrap_or(0);
                fact(ui, "Ano", &format!("{} dias (comum) ou {} dias · ciclo de {cy_years} anos = {} dias", cal.year_len(common), cal.year_len(leap_y), fmt_n(cy_days)));
            } else {
                fact(ui, "Ano", &format!("{} dias", cal.year_len(0)));
            }
            if let Some(pr) = c["presente"].as_f64() {
                fact(ui, "Presente da narrativa", &mu::year(pr, &c));
            }
            if let Some(d) = c["dia"].as_str().filter(|s| !s.is_empty()) {
                fact(ui, "O dia", d);
            }
            if !months.is_empty() {
                ui.add_space(14.0);
                kit::hairline(ui, p);
                ui.add_space(12.0);
                self.converter(ui, p, &c);
            }
        });
    }

    fn converter(&mut self, ui: &mut egui::Ui, p: Palette, c: &Json) {
        let cal = Calendar { c };
        if self.conv.2 == 1 && self.conv.0 == 1 {
            if let Some(pr) = c["presente"].as_i64() {
                self.conv.2 = pr;
            }
        }
        let (mut d, mut mi, mut y, mut add, mut unit) = self.conv;
        ui.label(RichText::new("CONVERSOR DE DATAS").font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let ms = cal.months_of(y);
            mi = mi.min(ms.len().saturating_sub(1));
            ui.add(egui::DragValue::new(&mut d).range(1..=ms.get(mi).map(|m| m.dias).unwrap_or(30)));
            let items: Vec<(String, bool)> = ms.iter().enumerate().map(|(i, m)| (format!("{}{}", m.nome, if m.intercalar { " (intercalar)" } else { "" }), i == mi)).collect();
            if let Some(k) = pick(ui, p, "conv-mes", &ms.get(mi).map(|m| m.nome.clone()).unwrap_or_default(), &items) {
                mi = k;
            }
            let mut ay = y.abs();
            if ui.add(egui::DragValue::new(&mut ay).range(0..=10_000_000)).changed() {
                y = if y < 0 { -ay } else { ay };
            }
            let antes = c["eraAntes"].as_str().unwrap_or("AC").to_string();
            let depois = c["eraDepois"].as_str().unwrap_or("DC").to_string();
            let cur = if y < 0 { 1 } else { 0 };
            if let Some(k) = kit::segmented(ui, &[("", &depois), ("", &antes)], cur, p) {
                y = if k == 1 { -y.abs() } else { y.abs() };
            }
        });
        let ms = cal.months_of(y);
        mi = mi.min(ms.len().saturating_sub(1));
        d = d.clamp(1, ms.get(mi).map(|m| m.dias).unwrap_or(30));
        ui.add_space(6.0);
        ui.label(RichText::new(cal.describe(y, mi, d)).font(kit::medium(15.0)).color(p.text));
        let mut bits = vec![format!("dia {} de {} do ano", cal.to_abs(y, mi, d) - cal.days_before_year(y) + 1, cal.year_len(y))];
        let ic = &c["intercalar"];
        if let Some(cy) = ic["ciclo"].as_i64().filter(|c| *c > 0) {
            bits.push(format!("ano {} do ciclo, {}", y.rem_euclid(cy) + 1, if cal.is_leap(y) { "com mês intercalar" } else { "comum" }));
        }
        if let Some(pr) = c["presente"].as_i64().filter(|pr| *pr != y) {
            let dy = pr - y;
            bits.push(if dy > 0 { format!("{} anos antes do presente", fmt_n(dy)) } else { format!("{} anos depois do presente", fmt_n(-dy)) });
        }
        ui.label(RichText::new(bits.join(" · ")).size(12.0).color(p.faint));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Somar").size(12.5).color(p.muted));
            ui.add(egui::DragValue::new(&mut add));
            let cur = match unit { 'm' => 1, 'y' => 2, _ => 0 };
            if let Some(k) = kit::segmented(ui, &[("", "dias"), ("", "meses"), ("", "anos")], cur, p) {
                unit = ['d', 'm', 'y'][k];
            }
            if add != 0 {
                let (y2, m2, d2) = cal.add(y, mi, d, add, unit);
                ui.label(RichText::new(format!("→ {}", cal.describe(y2, m2, d2))).font(kit::medium(13.0)).color(p.accent_strong));
            }
        });
        self.conv = (d, mi, y, add, unit);
    }

    fn eras_card(&mut self, ui: &mut egui::Ui, p: Palette, w: &Json) {
        let cal = w["calendario"].clone();
        let mut eras: Vec<Json> = w["eras"].as_array().cloned().unwrap_or_default();
        eras.sort_by(|a, b| a["de"].as_f64().unwrap_or(0.0).partial_cmp(&b["de"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
        let mut to_enredo = false;
        card(ui, p, |ui| {
            card_head(ui, p, "Eras", |ui| {
                if kit::button(ui, Some(icons::PLUS), "Nova era", p).clicked() {
                    self.era = Some(EraEdit { id: None, nome: String::new(), de: String::new(), ate: String::new(), nativo: String::new(), desc: String::new(), cor: ERA_CORES[eras.len() % ERA_CORES.len()].into() });
                }
                if !eras.is_empty() && kit::quiet(ui, Some(icons::ARROW_RIGHT_LEFT), "Levar à Cronologia do Enredo", p).clicked() {
                    to_enredo = true;
                }
                if kit::quiet(ui, Some(icons::FILE_SEARCH), "Ler de uma nota…", p).clicked() {
                    self.note_pick = Some((NotePick::Eras, String::new()));
                }
            });
            if eras.is_empty() {
                ui.label(RichText::new("Sem eras ainda.").color(p.muted));
                return;
            }
            // widths by the square root of the length: long prehistory doesn't crush recent eras
            let wts: Vec<f32> = eras.iter().map(|e| ((e["ate"].as_f64().unwrap_or(0.0) - e["de"].as_f64().unwrap_or(0.0)).max(50.0) as f32).sqrt()).collect();
            let tot: f32 = wts.iter().sum::<f32>().max(1e-3);
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(width, 74.0), Sense::hover());
            let mut x = rect.left();
            let codex = self.codex.clone();
            let mut entries_by_era: Vec<Vec<(String, String)>> = vec![Vec::new(); eras.len()];
            if let Some(cx) = &codex {
                static PAREN: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"[(（]([^)]*\d[^)]*)[)）]").unwrap());
                for e in cx.entries.iter().filter(|e| !e.legado) {
                    let Some(r) = PAREN.captures(&e.titulo).and_then(|c| mu::parse_range(&c[1], cal["eraAntes"].as_str().unwrap_or(""), cal["eraDepois"].as_str().unwrap_or(""))) else { continue };
                    let mid = (r.0 + r.1) / 2.0;
                    if let Some(k) = eras.iter().position(|er| mid >= er["de"].as_f64().unwrap_or(0.0) && mid <= er["ate"].as_f64().unwrap_or(0.0)) {
                        let t = PAREN.replace(&e.titulo, "").trim().to_string();
                        entries_by_era[k].push((e.key.clone(), t));
                    }
                }
            }
            for (i, er) in eras.iter().enumerate() {
                let wpx = width * wts[i] / tot;
                let r = Rect::from_min_size(pos2(x, rect.top()), vec2(wpx - 3.0, rect.height()));
                x += wpx;
                let color = super::color(er["cor"].as_str().unwrap_or("#c9a45c"));
                let resp = ui.interact(r, ui.id().with(("era", i)), Sense::click()).on_hover_text(str_of(er, "desc"));
                ui.painter().rect_filled(r, 8.0, color.gamma_multiply(if resp.hovered() { 0.28 } else { 0.18 }));
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 4.0)), egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, color);
                let clip = ui.painter().with_clip_rect(r.shrink(6.0));
                clip.text(r.left_top() + vec2(8.0, 12.0), egui::Align2::LEFT_TOP, str_of(er, "nome"), kit::medium(12.5), p.text);
                clip.text(r.left_top() + vec2(8.0, 32.0), egui::Align2::LEFT_TOP, format!("{} – {}", mu::year(er["de"].as_f64().unwrap_or(0.0), &cal), mu::year(er["ate"].as_f64().unwrap_or(0.0), &cal)), egui::FontId::proportional(11.0), p.muted);
                if !str_of(er, "nativo").is_empty() {
                    clip.text(r.left_top() + vec2(8.0, 50.0), egui::Align2::LEFT_TOP, str_of(er, "nativo"), egui::FontId::new(11.0, egui::FontFamily::Name("italic".into())), p.faint);
                }
                if resp.clicked() {
                    self.era = Some(era_edit_of(er, &cal));
                }
            }
            ui.add_space(10.0);
            for (i, er) in eras.iter().enumerate() {
                let desc = str_of(er, "desc");
                if desc.is_empty() && entries_by_era[i].is_empty() {
                    continue;
                }
                ui.add_space(6.0);
                ui.horizontal_top(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, super::color(er["cor"].as_str().unwrap_or("#c9a45c")));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(str_of(er, "nome")).font(kit::medium(13.0)).color(p.text));
                        if !desc.is_empty() {
                            ui.label(RichText::new(desc).size(12.5).color(p.muted));
                        }
                        if !entries_by_era[i].is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                                for (k, t) in &entries_by_era[i] {
                                    if kit::chip(ui, Some(icons::LIBRARY), t, false, p).clicked() {
                                        self.sel = Some(k.clone());
                                        self.aba = super::mundo_ui::Aba::Codice;
                                    }
                                }
                            });
                        }
                    });
                });
            }
        });
        if to_enredo {
            self.toast = Some("Abra a Cronologia do Enredo para ver as eras.".into());
            self.eras_to_enredo(&eras);
        }
    }

    fn eras_to_enredo(&mut self, eras: &[Json]) {
        let pending = eras.to_vec();
        self.pending_eras = Some(pending);
    }

    // ── genealogias ───────────────────────────────────────────────────────

    pub(super) fn genealogias(&mut self, ui: &mut egui::Ui, p: Palette, acts: &mut Vec<Action>) {
        let Some(w) = self.world().cloned() else { return };
        let wid = str_of(&w, "id").to_string();
        let trees: Vec<(usize, Json)> = self.data["arvores"].as_array().into_iter().flatten().enumerate().filter(|(_, t)| str_of(t, "mundo").is_empty() || t["mundo"] == wid.as_str()).map(|(i, t)| (i, t.clone())).collect();
        egui::SidePanel::left(ui.id().with("mu-trees")).resizable(false).exact_width(210.0).show_separator_line(false).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 14, right: 8, top: 14, bottom: 8 })).show_inside(ui, |ui| {
            ui.label(RichText::new("ÁRVORES").font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.y = 1.0;
            let mut del = None;
            for (i, t) in &trees {
                let id = str_of(t, "id").to_string();
                let on = self.tree.as_deref() == Some(id.as_str()) || (self.tree.is_none() && trees.first().map(|x| x.0) == Some(*i));
                let r = kit::Row::new(str_of(t, "nome")).icon(icons::GIT_FORK).selected(on).trailing(t["pessoas"].as_array().map(|a| a.len()).unwrap_or(0).to_string()).show(ui, p);
                if r.clicked() {
                    self.tree = Some(id.clone());
                    self.person = None;
                }
                r.context_menu(|ui| {
                    if kit::menu_item(ui, icons::PENCIL, "Renomear…", p) {
                        self.new_tree = Some((str_of(t, "nome").to_string(), id.clone()));
                    }
                    if kit::menu_danger(ui, icons::TRASH_2, "Excluir árvore", p) {
                        del = Some(*i);
                    }
                });
            }
            if let Some(i) = del {
                self.data["arvores"].as_array_mut().unwrap().remove(i);
                self.tree = None;
                self.changed();
            }
            ui.add_space(6.0);
            if kit::quiet(ui, Some(icons::PLUS), "Nova árvore", p).clicked() {
                self.new_tree = Some((String::new(), String::new()));
            }
        });
        let cur = trees.iter().find(|(_, t)| self.tree.as_deref() == Some(str_of(t, "id"))).or(trees.first()).map(|x| x.0);
        let Some(ti) = cur else {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show_inside(ui, |ui| {
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.label(kit::icon(icons::GIT_FORK, 32.0).color(p.faint));
                    ui.add_space(12.0);
                    ui.label(RichText::new("Genealogias").font(kit::medium(17.0)).color(p.text));
                    ui.add_space(6.0);
                    ui.allocate_ui(vec2(440.0, 60.0), |ui| {
                        ui.label(RichText::new("Árvores das Casas e dinastias, ou de qualquer família do livro. Cada pessoa pode apontar para uma ficha de personagem ou uma nota.").color(p.muted));
                    });
                    ui.add_space(12.0);
                    if kit::primary_icon(ui, icons::PLUS, "Criar a primeira árvore", p).clicked() {
                        self.new_tree = Some((String::new(), String::new()));
                    }
                });
            });
            return;
        };
        self.tree = Some(str_of(&self.data["arvores"][ti], "id").to_string());
        if !self.data["arvores"][ti]["pessoas"].is_array() {
            self.data["arvores"][ti]["pessoas"] = json!([]);
        }
        if self.person.is_some() {
            egui::SidePanel::right(ui.id().with("mu-person")).resizable(false).exact_width(300.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 18, right: 14, top: 14, bottom: 10 })).show_inside(ui, |ui| {
                self.person_drawer(ui, p, ti, &w, acts);
            });
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg_secondary)).show_inside(ui, |ui| {
            let t = self.data["arvores"][ti].clone();
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.label(RichText::new(str_of(&t, "nome")).font(kit::bold(20.0)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(14.0);
                    if kit::button(ui, Some(icons::USER_PLUS), "Pessoa", p).clicked() {
                        self.add_person(ti, None, vec![], vec![]);
                    }
                });
            });
            let pessoas: Vec<Json> = t["pessoas"].as_array().cloned().unwrap_or_default();
            if pessoas.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Comece pela pessoa mais antiga que você conhece. Depois use filho(a), cônjuge e pai/mãe no painel da pessoa.").color(p.muted));
                });
                return;
            }
            self.draw_tree(ui, p, &t, &pessoas, &w);
        });
    }

    fn draw_tree(&mut self, ui: &mut egui::Ui, p: Palette, t: &Json, pessoas: &[Json], w: &Json) {
        let (pos, tw, th) = mu::family_layout(pessoas);
        const PAD: f32 = 40.0;
        const CW: f32 = 160.0;
        const CH: f32 = 92.0;
        let cal = w["calendario"].clone();
        let tree_color = super::color(t["cor"].as_str().or(w["cor"].as_str()).filter(|s| !s.is_empty()).unwrap_or("#8a6d3b"));
        egui::ScrollArea::both().id_salt(("mu-tree", str_of(t, "id"))).auto_shrink([false, false]).show(ui, |ui| {
            let size = vec2(tw + PAD * 2.0, th + PAD * 2.0).max(ui.available_size());
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let off = rect.min + vec2(PAD + ((size.x - tw - PAD * 2.0) / 2.0).max(0.0), PAD);
            let at = |id: &str| pos.get(id).map(|v| off + vec2(v.0, v.1));
            let painter = ui.painter();
            let line = Stroke::new(1.4_f32, p.border_strong);
            let ids: Vec<String> = pessoas.iter().map(|x| str_of(x, "id").to_string()).collect();
            let list = |x: &Json, k: &str| -> Vec<String> { x[k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).filter(|v| ids.contains(v)).collect() };
            // couples
            let mut seen = Vec::new();
            for x in pessoas {
                for s in list(x, "conjuges") {
                    let mut k = [str_of(x, "id").to_string(), s.clone()];
                    k.sort();
                    if seen.contains(&k) {
                        continue;
                    }
                    seen.push(k);
                    if let (Some(a), Some(b)) = (at(str_of(x, "id")), at(&s)) {
                        if (a.y - b.y).abs() < 0.5 {
                            painter.hline((a.x.min(b.x) + CW)..=a.x.max(b.x), a.y + CH / 2.0, line);
                        }
                    }
                }
            }
            // children: from the middle of the couple (or the one parent) down to each child
            let mut fams: Vec<(Vec<String>, Vec<String>)> = Vec::new();
            for x in pessoas {
                let mut ps = list(x, "pais");
                if ps.is_empty() {
                    continue;
                }
                ps.sort();
                match fams.iter_mut().find(|f| f.0 == ps) {
                    Some(f) => f.1.push(str_of(x, "id").to_string()),
                    None => fams.push((ps, vec![str_of(x, "id").to_string()])),
                }
            }
            for (ps, kids) in fams {
                let pp: Vec<egui::Pos2> = ps.iter().filter_map(|i| at(i)).collect();
                let ks: Vec<egui::Pos2> = kids.iter().filter_map(|i| at(i)).collect();
                if pp.is_empty() || ks.is_empty() {
                    continue;
                }
                let (px, py) = if pp.len() == 2 { ((pp[0].x.min(pp[1].x) + CW + pp[0].x.max(pp[1].x)) / 2.0, pp[0].y + CH / 2.0) } else { (pp[0].x + CW / 2.0, pp[0].y + CH) };
                let mid = ks[0].y - (mu::TREE_H - CH) / 2.0;
                painter.vline(px, py..=mid, line);
                let xs: Vec<f32> = ks.iter().map(|c| c.x + CW / 2.0).chain(std::iter::once(px)).collect();
                painter.hline(xs.iter().cloned().fold(f32::INFINITY, f32::min)..=xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max), mid, line);
                for c in ks {
                    painter.vline(c.x + CW / 2.0, mid..=c.y, line);
                }
            }
            // people
            for x in pessoas {
                let id = str_of(x, "id").to_string();
                let Some(o) = at(&id) else { continue };
                let r = Rect::from_min_size(o, vec2(CW, CH));
                let resp = ui.interact(r, ui.id().with(("person", &id)), Sense::click());
                let sel = self.person.as_deref() == Some(id.as_str());
                let color = x["cor"].as_str().filter(|s| !s.is_empty()).map(super::color).unwrap_or(tree_color);
                painter.rect_filled(r.translate(vec2(0.0, 1.0)), 10.0, Color32::from_black_alpha(if p.dark { 40 } else { 10 }));
                painter.rect(r, 10.0, p.bg, Stroke::new(if sel { 1.5_f32 } else { 1.0 }, if sel { p.accent_strong } else if resp.hovered() { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
                painter.rect_filled(Rect::from_min_size(r.min, vec2(CW, 4.0)), egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, color);
                let nome = str_of(x, "nome");
                let g = super::enredo_ui::truncated(ui, if nome.is_empty() { "Sem nome" } else { nome }, kit::medium(13.0), p.text, CW - 20.0);
                painter.galley(r.left_top() + vec2(10.0, 14.0), g, p.text);
                let titulo = str_of(x, "titulo");
                if !titulo.is_empty() {
                    let g = super::enredo_ui::truncated(ui, titulo, egui::FontId::proportional(11.5), p.muted, CW - 20.0);
                    painter.galley(r.left_top() + vec2(10.0, 34.0), g, p.muted);
                }
                let yr = |k: &str| -> String {
                    let v = x[k].as_str().map(String::from).or_else(|| x[k].as_i64().map(|n| n.to_string())).unwrap_or_default();
                    match v.trim().parse::<f64>() {
                        Ok(n) => mu::year(n, &cal),
                        Err(_) => v,
                    }
                };
                let (a, b) = (yr("nasc"), yr("morte"));
                if !a.is_empty() || !b.is_empty() {
                    painter.text(r.left_bottom() + vec2(10.0, -12.0), egui::Align2::LEFT_CENTER, format!("{} – {}", if a.is_empty() { "?" } else { &a }, b), egui::FontId::proportional(11.0), p.faint);
                }
                if !str_of(x, "nota").is_empty() {
                    painter.text(r.right_bottom() + vec2(-12.0, -12.0), egui::Align2::CENTER_CENTER, icons::LINK, kit::icon_font(11.0), p.faint);
                }
                if resp.clicked() {
                    self.person = Some(id.clone());
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
        });
    }

    fn add_person(&mut self, ti: usize, child_of: Option<String>, pais: Vec<String>, conjuges: Vec<String>) {
        let id = en::uid("p");
        let t = &mut self.data["arvores"][ti];
        let ps = t["pessoas"].as_array_mut().unwrap();
        ps.push(json!({ "id": id, "nome": "", "titulo": "", "nasc": "", "morte": "", "nota": "", "pais": pais, "conjuges": conjuges }));
        for q in ps.iter_mut() {
            let qid = str_of(q, "id").to_string();
            if conjuges.contains(&qid) {
                push_unique(q, "conjuges", &id);
            }
            if child_of.as_deref() == Some(qid.as_str()) {
                push_unique(q, "pais", &id);
                if let Some(a) = q["pais"].as_array_mut() {
                    while a.len() > 2 {
                        a.remove(0);
                    }
                }
            }
        }
        self.person = Some(id);
        self.focus_name = true;
        self.changed();
    }

    fn person_drawer(&mut self, ui: &mut egui::Ui, p: Palette, ti: usize, w: &Json, acts: &mut Vec<Action>) {
        let Some(pid) = self.person.clone() else { return };
        let Some(pi) = self.data["arvores"][ti]["pessoas"].as_array().and_then(|a| a.iter().position(|x| x["id"] == pid.as_str())) else {
            self.person = None;
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("PESSOA").font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button_sized(ui, icons::X, "Fechar", false, p, 24.0, 14.0).clicked() {
                    self.person = None;
                }
            });
        });
        let antes = w["calendario"]["eraAntes"].as_str().unwrap_or("AC").to_string();
        let mut ch = false;
        let focus = self.focus_name;
        {
            let x = &mut self.data["arvores"][ti]["pessoas"][pi];
            for (label, key, hint) in [("Nome", "nome", "Nome"), ("Título ou papel", "titulo", "Rainha de Sadaka, fundador…"), ("Nascimento (ano)", "nasc", ""), ("Morte (ano)", "morte", ""), ("Nota ou ficha ligada", "nota", "Nome da nota")] {
                ui.add_space(4.0);
                ui.label(RichText::new(label).size(12.0).color(p.muted));
                let mut v = x[key].as_str().map(String::from).or_else(|| x[key].as_i64().map(|n| n.to_string())).unwrap_or_default();
                let hint = if key == "nasc" { format!("ex.: 2790 (ou -300 para {antes})") } else { hint.to_string() };
                let r = ui.add(egui::TextEdit::singleline(&mut v).hint_text(RichText::new(hint).color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                if key == "nome" && focus {
                    // keep asking until it holds (the click that made the person may still be going on)
                    if r.has_focus() {
                        self.focus_name = false;
                    } else {
                        r.request_focus();
                    }
                }
                if r.changed() {
                    x[key] = json!(v);
                    ch = true;
                }
            }
        }
        let x = self.data["arvores"][ti]["pessoas"][pi].clone();
        let nota = str_of(&x, "nota").to_string();
        if !nota.is_empty() && kit::quiet(ui, Some(icons::EXTERNAL_LINK), "Abrir a nota", p).clicked() {
            acts.push(Action::OpenLink(nota.trim_start_matches("[[").trim_end_matches("]]").to_string(), String::new(), true));
        }
        ui.add_space(10.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
        let pais: Vec<String> = x["pais"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
        let conj: Vec<String> = x["conjuges"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
        let mut add: Option<(Option<String>, Vec<String>, Vec<String>)> = None;
        ui.horizontal_wrapped(|ui| {
            if kit::button(ui, Some(icons::ARROW_DOWN), "Filho(a)", p).clicked() {
                let mut ps = vec![pid.clone()];
                ps.extend(conj.iter().take(1).cloned());
                add = Some((None, ps, vec![]));
            }
            if kit::button(ui, Some(icons::HEART), "Cônjuge", p).clicked() {
                add = Some((None, vec![], vec![pid.clone()]));
            }
            if pais.len() < 2 && kit::button(ui, Some(icons::ARROW_UP), "Pai/Mãe", p).clicked() {
                add = Some((Some(pid.clone()), vec![], pais.iter().take(1).cloned().collect()));
            }
        });
        // link to someone already in the tree
        let others: Vec<(String, String)> = self.data["arvores"][ti]["pessoas"].as_array().into_iter().flatten().filter(|o| o["id"] != pid.as_str()).map(|o| (str_of(o, "id").to_string(), str_of(o, "nome").to_string())).collect();
        let mut link: Option<(String, &str)> = None;
        if !others.is_empty() {
            let mut items = Vec::new();
            for (oid, n) in &others {
                let n = if n.is_empty() { "Sem nome" } else { n };
                items.push((format!("{n} é pai/mãe"), oid.clone(), "pai"));
                items.push((format!("{n} é cônjuge"), oid.clone(), "conj"));
                items.push((format!("{n} é filho(a)"), oid.clone(), "filho"));
            }
            let labels: Vec<(String, bool)> = items.iter().map(|x| (x.0.clone(), false)).collect();
            ui.add_space(4.0);
            if let Some(k) = pick(ui, p, "link-person", "Ligar a alguém da árvore…", &labels) {
                link = Some((items[k].1.clone(), items[k].2));
            }
        }
        ui.add_space(10.0);
        let mut del = false;
        if kit::quiet(ui, Some(icons::TRASH_2), "Excluir da árvore", p).clicked() {
            del = true;
        }
        if let Some((oid, kind)) = link {
            let ps = self.data["arvores"][ti]["pessoas"].as_array_mut().unwrap();
            let oi = ps.iter().position(|o| o["id"] == oid.as_str()).unwrap();
            match kind {
                "pai" => {
                    push_unique(&mut ps[pi], "pais", &oid);
                    trim2(&mut ps[pi]);
                }
                "filho" => {
                    push_unique(&mut ps[oi], "pais", &pid);
                    trim2(&mut ps[oi]);
                }
                _ => {
                    push_unique(&mut ps[pi], "conjuges", &oid);
                    push_unique(&mut ps[oi], "conjuges", &pid);
                }
            }
            ch = true;
        }
        if del {
            let ps = self.data["arvores"][ti]["pessoas"].as_array_mut().unwrap();
            ps.remove(pi);
            for o in ps.iter_mut() {
                for k in ["pais", "conjuges"] {
                    if let Some(a) = o[k].as_array_mut() {
                        a.retain(|v| v != pid.as_str());
                    }
                }
            }
            self.person = None;
            ch = true;
        }
        if ch {
            self.changed();
        }
        if let Some((child_of, ps, cj)) = add {
            self.add_person(ti, child_of, ps, cj);
        }
    }

    // ── línguas ───────────────────────────────────────────────────────────

    pub(super) fn linguas(&mut self, ui: &mut egui::Ui, p: Palette, acts: &mut Vec<Action>) {
        let Some(lex) = self.lex.clone() else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(18.0).color(p.muted));
                ui.label(RichText::new("Lendo os léxicos…").color(p.muted));
            });
            return;
        };
        if lex.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(kit::icon(icons::LANGUAGES, 30.0).color(p.faint));
                ui.add_space(10.0);
                ui.allocate_ui(vec2(460.0, 60.0), |ui| {
                    ui.label(RichText::new("Nenhuma tabela de léxico encontrada. Nas fontes do mundo, marque a pasta das línguas com a categoria \"Línguas e escritas\".").color(p.muted));
                });
            });
            return;
        }
        let mut langs: Vec<(String, usize)> = Vec::new();
        for r in lex.iter() {
            match langs.iter_mut().find(|l| l.0 == r.lang) {
                Some(l) => l.1 += 1,
                None => langs.push((r.lang.clone(), 1)),
            }
        }
        langs.sort_by(|a, b| a.0.cmp(&b.0));
        if self.lang != "tudo" && !langs.iter().any(|l| l.0 == self.lang) {
            self.lang = "tudo".into();
        }
        egui::SidePanel::left(ui.id().with("mu-langs")).resizable(false).exact_width(210.0).show_separator_line(false).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 14, right: 8, top: 14, bottom: 8 })).show_inside(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if kit::Row::new("Todas").icon(icons::LANGUAGES).selected(self.lang == "tudo").trailing(lex.len().to_string()).show(ui, p).clicked() {
                self.lang = "tudo".into();
                self.lex_max = 120;
            }
            ui.add_space(6.0);
            for (l, n) in &langs {
                if kit::Row::new(l).selected(&self.lang == l).trailing(n.to_string()).show(ui, p).clicked() {
                    self.lang = l.clone();
                    self.lex_max = 120;
                }
            }
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 24, right: 18, top: 14, bottom: 8 })).show_inside(ui, |ui| {
            let q = mu::deaccent(self.q.trim());
            let rows: Vec<&mu::LexRow> = lex.iter().filter(|r| self.lang == "tudo" || r.lang == self.lang).filter(|r| q.is_empty() || r.text.contains(&q)).collect();
            ui.label(RichText::new(if q.is_empty() { if self.lang == "tudo" { "Todas as línguas".to_string() } else { self.lang.clone() } } else { format!("{} linha{} com “{}”", rows.len(), if rows.len() == 1 { "" } else { "s" }, self.q.trim()) }).font(kit::medium(15.0)).color(p.text));
            if q.is_empty() {
                ui.label(RichText::new("Busque uma palavra, um sentido ou um som. Acentos não importam.").size(12.0).color(p.faint));
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("mu-lex").auto_shrink([false, false]).show(ui, |ui| {
                let mut last = String::new();
                let mut grid_rows: Vec<&mu::LexRow> = Vec::new();
                let flush = |ui: &mut egui::Ui, rs: &mut Vec<&mu::LexRow>, acts: &mut Vec<Action>| {
                    if rs.is_empty() {
                        return;
                    }
                    let r0 = rs[0];
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&r0.lang).font(kit::medium(11.5)).color(p.accent_strong));
                        let src = format!("{}{}", r0.path.rsplit('/').next().unwrap_or(&r0.path).trim_end_matches(".md").replace('_', " "), if r0.head.is_empty() { String::new() } else { format!(" › {}", r0.head) });
                        if ui.add(egui::Label::new(RichText::new(src).size(11.5).color(p.faint)).sense(Sense::click())).on_hover_text("Abrir a nota").clicked() {
                            acts.push(Action::Open(r0.path.clone(), true));
                        }
                    });
                    ui.add_space(4.0);
                    egui::Frame::new().stroke(Stroke::new(1.0_f32, p.border)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        egui::Grid::new(("lex", &r0.path, r0.line)).striped(true).spacing(vec2(18.0, 6.0)).show(ui, |ui| {
                            for c in &r0.cols {
                                ui.label(RichText::new(c).font(kit::medium(12.0)).color(p.muted));
                            }
                            ui.end_row();
                            for r in rs.iter() {
                                for c in &r.cells {
                                    ui.label(rich_cell(c, &self.q, p));
                                }
                                ui.end_row();
                            }
                        });
                    });
                    rs.clear();
                };
                for r in rows.iter().take(self.lex_max) {
                    let k = format!("{}#{}#{}", r.path, r.head, r.cols.join("|"));
                    if k != last {
                        flush(ui, &mut grid_rows, acts);
                        last = k;
                    }
                    grid_rows.push(r);
                }
                flush(ui, &mut grid_rows, acts);
                if rows.len() > self.lex_max {
                    ui.add_space(10.0);
                    if kit::quiet(ui, None, &format!("Mostrar mais ({} restantes)", rows.len() - self.lex_max), p).clicked() {
                        self.lex_max += 200;
                    }
                }
                if rows.is_empty() {
                    ui.label(RichText::new("Nada encontrado.").color(p.faint));
                }
                ui.add_space(30.0);
            });
        });
    }

    // ── dialogs ───────────────────────────────────────────────────────────

    pub(super) fn dialogs(&mut self, ctx: &egui::Context, p: Palette, root: &Path, ix: &Index, b: &Book, _author: &str) {
        if let Some(eras) = self.pending_eras.take() {
            let mut data = en::load(root, b);
            let mut n = 0;
            for er in eras {
                if data["eras"].as_array().is_some_and(|a| a.iter().any(|x| x["nome"] == er["nome"])) {
                    continue;
                }
                data["eras"].as_array_mut().unwrap().push(json!({ "id": en::uid("r"), "nome": er["nome"], "de": er["de"], "ate": er["ate"], "cor": er["cor"].as_str().unwrap_or("#c9a45c"), "mundo": self.world().map(|w| w["id"].clone()).unwrap_or(Json::Null) }));
                n += 1;
            }
            let _ = en::save(root, ix, b, &data);
            self.toast = Some(if n > 0 { format!("{n} eras levadas para a Cronologia do Enredo.") } else { "A Cronologia já tem estas eras.".into() });
        }
        if self.any_dialog() {
            kit::scrim(ctx, p);
        }
        self.world_dialog(ctx, p, root);
        self.sources_dialog(ctx, p, root);
        self.era_dialog(ctx, p);
        self.calendar_dialog(ctx, p);
        self.note_dialog(ctx, p, root, ix);
        self.tree_dialog(ctx, p);
        self.recat_dialog(ctx, p);
        self.terms_dialog(ctx, p);
    }

    fn window<R>(ctx: &egui::Context, id: &str, width: f32, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
        egui::Window::new(id).title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
            ui.set_width(width);
            body(ui)
        }).and_then(|r| r.inner)
    }

    fn footer(ui: &mut egui::Ui, p: Palette, ok: &str) -> (bool, bool) {
        let (mut save, mut cancel) = (false, false);
        ui.add_space(12.0);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            save = kit::primary(ui, ok, p).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command));
            cancel = kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
        (save, cancel)
    }

    fn world_dialog(&mut self, ctx: &egui::Context, p: Palette, root: &Path) {
        let Some(nw) = self.new_world.as_mut() else { return };
        let editing = nw.id.is_some();
        let folders = if editing { vec![] } else { vault_folders(root) };
        let res = Self::window(ctx, "mu-new-world", 440.0, |ui| {
            ui.label(RichText::new(if editing { "Renomear mundo" } else { "Novo mundo" }).font(kit::medium(16.0)).color(p.text));
            ui.add_space(10.0);
            ui.label(RichText::new("Nome").size(12.0).color(p.muted));
            let r = ui.add(egui::TextEdit::singleline(&mut nw.nome).hint_text(RichText::new("Álfheim").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
            if nw.nome.is_empty() {
                r.request_focus();
            }
            if !editing {
                ui.add_space(8.0);
                ui.label(RichText::new("Pasta com as notas deste mundo (opcional)").size(12.0).color(p.muted));
                ui.add(egui::TextEdit::singleline(&mut nw.filtro).hint_text(RichText::new("Filtrar pastas").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
                let f = mu::deaccent(&nw.filtro);
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for d in folders.iter().filter(|d| f.is_empty() || mu::deaccent(d).contains(&f)).take(200) {
                        if kit::Row::new(d).icon(icons::FOLDER).selected(nw.pasta == *d).show(ui, p).clicked() {
                            nw.pasta = d.clone();
                        }
                    }
                });
            }
            Self::footer(ui, p, if editing { "Salvar" } else { "Criar" })
        });
        let Some((save, cancel)) = res else { return };
        if cancel {
            self.new_world = None;
        } else if save && !nw.nome.trim().is_empty() {
            let nw = self.new_world.take().unwrap();
            match nw.id {
                Some(id) => {
                    if let Some(w) = self.data["mundos"].as_array_mut().and_then(|a| a.iter_mut().find(|w| w["id"] == id.as_str())) {
                        w["nome"] = json!(nw.nome.trim());
                    }
                }
                None => {
                    let n = self.data["mundos"].as_array().map(|a| a.len()).unwrap_or(0);
                    let id = en::uid("w");
                    let mut w = json!({ "id": id, "nome": nw.nome.trim(), "cor": en::TRAMA_CORES[n % en::TRAMA_CORES.len()], "fontes": [] });
                    if !nw.pasta.is_empty() {
                        w["fontes"] = json!([{ "id": en::uid("f"), "tipo": "pasta", "caminho": nw.pasta, "rotulo": nw.pasta.rsplit('/').next().unwrap_or(&nw.pasta), "cat": "" }]);
                    }
                    self.data["mundos"].as_array_mut().unwrap().push(w);
                    self.mundo = Some(id);
                    self.codex = None;
                    self.lex = None;
                }
            }
            self.changed();
        }
    }

    fn sources_dialog(&mut self, ctx: &egui::Context, p: Palette, _root: &Path) {
        if !self.sources || self.note_pick.is_some() {
            return;
        }
        let Some(w) = self.world().cloned() else {
            self.sources = false;
            return;
        };
        let mut fontes: Vec<Json> = w["fontes"].as_array().cloned().unwrap_or_default();
        let mut changed = false;
        let mut add: Option<NotePick> = None;
        let res = Self::window(ctx, "mu-sources", 600.0, |ui| {
            ui.label(RichText::new(format!("Fontes de {}", str_of(&w, "nome"))).font(kit::medium(16.0)).color(p.text));
            ui.label(RichText::new("Pastas e notas que formam este mundo. Nada é movido nem copiado. Numa nota longa, cada título vira um verbete; numa pasta, cada nota vira um.").size(12.5).color(p.muted));
            ui.add_space(10.0);
            let mut remove = None;
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                for (i, s) in fontes.iter_mut().enumerate() {
                    egui::Frame::new().stroke(Stroke::new(1.0_f32, p.border)).corner_radius(10.0).inner_margin(10.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(kit::icon(if s["tipo"] == "pasta" { icons::FOLDER } else { icons::FILE_TEXT }, 14.0).color(p.muted));
                            let mut lab = str_of(s, "rotulo").to_string();
                            if ui.add(egui::TextEdit::singleline(&mut lab).hint_text(RichText::new("Rótulo").color(p.faint)).desired_width(170.0).margin(egui::Margin::symmetric(6, 4))).changed() {
                                s["rotulo"] = json!(lab);
                                changed = true;
                            }
                            let cur = str_of(s, "cat").to_string();
                            let mut items = vec![("Automática (pelo título)".to_string(), cur.is_empty())];
                            items.extend(mu::CATS.iter().map(|c| (c.1.to_string(), cur == c.0)));
                            let lab = if cur.is_empty() { "Categoria automática".to_string() } else { mu::cat_name(&cur).to_string() };
                            if let Some(k) = pick(ui, p, &format!("src-cat-{i}"), &lab, &items) {
                                s["cat"] = json!(if k == 0 { "" } else { mu::CATS[k - 1].0 });
                                changed = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if kit::icon_button_sized(ui, icons::X, "Tirar esta fonte", false, p, 22.0, 12.0).clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            let mut lg = s["legado"].as_bool().unwrap_or(false);
                            if kit::toggle(ui, &mut lg, p).changed() {
                                s["legado"] = json!(lg);
                                changed = true;
                            }
                            ui.label(RichText::new("versão anterior").size(12.0).color(p.muted));
                            ui.label(RichText::new(format!("{}{}", str_of(s, "caminho"), s["secao"].as_str().map(|x| format!(" › {x}")).unwrap_or_default())).size(11.5).color(p.faint));
                        });
                    });
                    ui.add_space(6.0);
                }
                if fontes.is_empty() {
                    ui.label(RichText::new("Nenhuma fonte ainda.").color(p.faint));
                }
            });
            if let Some(i) = remove {
                fontes.remove(i);
                changed = true;
            }
            ui.horizontal(|ui| {
                if kit::button(ui, Some(icons::FOLDER_PLUS), "Pasta…", p).clicked() {
                    add = Some(NotePick::SourceFolder);
                }
                if kit::button(ui, Some(icons::FILE_PLUS), "Nota…", p).clicked() {
                    add = Some(NotePick::Source);
                }
            });
            let mut done = false;
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                done = kit::primary(ui, "Pronto", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
            done
        });
        if changed {
            if let Some(wm) = self.world_mut() {
                wm["fontes"] = json!(fontes);
            }
            self.changed();
            self.last_check = None;
        }
        if let Some(k) = add {
            self.note_pick = Some((k, String::new()));
        }
        if res == Some(true) {
            self.sources = false;
            self.last_check = None;
        }
    }

    fn note_dialog(&mut self, ctx: &egui::Context, p: Palette, root: &Path, ix: &Index) {
        let Some((kind, filter)) = self.note_pick.as_mut() else { return };
        let kind = *kind;
        let items: Vec<String> = if kind == NotePick::SourceFolder {
            vault_folders(root)
        } else {
            let mut v: Vec<String> = ix.page_paths().map(String::from).collect();
            v.sort();
            v
        };
        let title = match kind {
            NotePick::Calendar => "Nota com as tabelas do calendário (meses, semana)",
            NotePick::Eras => "Nota com a tabela de periodização (Era · Nomes · Datas · Resumo)",
            NotePick::Source => "Nota do mundo (cada título vira verbete)",
            NotePick::SourceFolder => "Pasta do mundo",
        };
        let mut chosen = None;
        let res = Self::window(ctx, "mu-note-pick", 520.0, |ui| {
            ui.label(RichText::new(title).font(kit::medium(15.0)).color(p.text));
            ui.add_space(8.0);
            let r = ui.add(egui::TextEdit::singleline(filter).hint_text(RichText::new("Filtrar").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
            r.request_focus();
            let f = mu::deaccent(filter);
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for it in items.iter().filter(|x| f.is_empty() || mu::deaccent(x).contains(&f)).take(300) {
                    if kit::Row::new(it).icon(if kind == NotePick::SourceFolder { icons::FOLDER } else { icons::FILE_TEXT }).show(ui, p).clicked() {
                        chosen = Some(it.clone());
                    }
                }
            });
            let mut cancel = false;
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
            cancel
        });
        if let Some(path) = chosen {
            self.note_pick = None;
            match kind {
                NotePick::Calendar => {
                    let md = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
                    let mut cal = mu::calendar_from_md(&md);
                    let n = cal["meses"].as_array().map(|a| a.len()).unwrap_or(0);
                    if n == 0 {
                        self.toast = Some("Não achei uma tabela de meses nessa nota.".into());
                        return;
                    }
                    cal["fonte"] = json!(path);
                    let sem = cal["semana"].as_array().map(|a| a.len()).unwrap_or(0);
                    let inter = cal["intercalar"].is_object();
                    if let Some(w) = self.world_mut() {
                        let mut cur = w["calendario"].clone();
                        if !cur.is_object() {
                            cur = json!({});
                        }
                        for (k, v) in cal.as_object().unwrap() {
                            cur[k] = v.clone();
                        }
                        w["calendario"] = cur;
                    }
                    self.toast = Some(format!("Calendário lido: {n} meses{}, {sem} dias na semana.", if inter { " + intercalar" } else { "" }));
                    self.changed();
                }
                NotePick::Eras => {
                    let md = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
                    let eras = mu::eras_from_md(&md);
                    if eras.is_empty() {
                        self.toast = Some("Não achei uma tabela de periodização nessa nota.".into());
                        return;
                    }
                    let n = eras.len();
                    if let Some(w) = self.world_mut() {
                        w["eras"] = json!(eras);
                    }
                    self.toast = Some(format!("{n} eras lidas."));
                    self.changed();
                }
                NotePick::Source | NotePick::SourceFolder => {
                    let folder = kind == NotePick::SourceFolder;
                    let rotulo = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".md").to_string();
                    if let Some(w) = self.world_mut() {
                        if !w["fontes"].is_array() {
                            w["fontes"] = json!([]);
                        }
                        w["fontes"].as_array_mut().unwrap().push(json!({ "id": en::uid("f"), "tipo": if folder { "pasta" } else { "nota" }, "caminho": path, "rotulo": rotulo, "cat": "" }));
                    }
                    self.changed();
                    self.last_check = None;
                }
            }
        } else if res == Some(true) {
            self.note_pick = None;
        }
    }

    fn era_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(e) = self.era.as_mut() else { return };
        let cal = self.data["mundos"].as_array().and_then(|ws| ws.iter().find(|w| Some(str_of(w, "id")) == self.mundo.as_deref()).or(ws.first())).map(|w| w["calendario"].clone()).unwrap_or(Json::Null);
        let antes = cal["eraAntes"].as_str().unwrap_or("AC").to_string();
        let depois = cal["eraDepois"].as_str().unwrap_or("DC").to_string();
        let mut delete = false;
        let res = Self::window(ctx, "mu-era", 440.0, |ui| {
            ui.label(RichText::new(if e.id.is_some() { "Editar era" } else { "Nova era" }).font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            for (l, v, h) in [("Nome", &mut e.nome, String::new()), ("Começa em", &mut e.de, format!("ex.: 3000 {antes}")), ("Termina em", &mut e.ate, format!("ex.: 150 {depois}")), ("Nomes nativos", &mut e.nativo, String::new())] {
                ui.label(RichText::new(l).size(12.0).color(p.muted));
                ui.add(egui::TextEdit::singleline(v).hint_text(RichText::new(h).color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                ui.add_space(4.0);
            }
            ui.label(RichText::new("Em uma linha").size(12.0).color(p.muted));
            ui.add(egui::TextEdit::multiline(&mut e.desc).desired_rows(2).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Cor").size(12.0).color(p.muted));
                for c in ERA_CORES {
                    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 7.5, super::color(c));
                    if e.cor.eq_ignore_ascii_case(c) {
                        ui.painter().circle_stroke(r.center(), 10.0, Stroke::new(1.5_f32, p.text));
                    }
                    if resp.clicked() {
                        e.cor = c.into();
                    }
                }
            });
            if e.id.is_some() {
                ui.add_space(6.0);
                delete = kit::quiet(ui, Some(icons::TRASH_2), "Excluir esta era", p).clicked();
            }
            Self::footer(ui, p, "Salvar")
        });
        let Some((save, cancel)) = res else { return };
        if delete || (save && !e.nome.trim().is_empty()) {
            let e = self.era.take().unwrap();
            let a = mu::parse_range(&e.de, &antes, &depois);
            let z = mu::parse_range(&e.ate, &antes, &depois);
            if let Some(w) = self.world_mut() {
                if !w["eras"].is_array() {
                    w["eras"] = json!([]);
                }
                let eras = w["eras"].as_array_mut().unwrap();
                if delete {
                    eras.retain(|x| Some(str_of(x, "id")) != e.id.as_deref());
                } else {
                    let v = json!({ "nome": e.nome.trim(), "de": a.map(|r| r.0).unwrap_or(0.0), "ate": z.map(|r| r.1).or(a.map(|r| r.1)).unwrap_or(0.0), "nativo": e.nativo, "desc": e.desc, "cor": e.cor });
                    match e.id.as_ref().and_then(|id| eras.iter_mut().find(|x| x["id"] == id.as_str())) {
                        Some(x) => {
                            for (k, val) in v.as_object().unwrap() {
                                x[k] = val.clone();
                            }
                        }
                        None => {
                            let mut v = v;
                            v["id"] = json!(en::uid("era"));
                            eras.push(v);
                        }
                    }
                }
            }
            self.changed();
        } else if cancel {
            self.era = None;
        }
    }

    fn calendar_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(c) = self.cal_edit.as_mut() else { return };
        let res = Self::window(ctx, "mu-cal", 560.0, |ui| {
            ui.label(RichText::new("Calendário").font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(ctx.screen_rect().height() - 300.0).show(ui, |ui| {
                let field = |ui: &mut egui::Ui, l: &str, d: &str, v: &mut String, rows: usize| {
                    ui.label(RichText::new(l).font(kit::medium(12.5)).color(p.text));
                    if !d.is_empty() {
                        ui.label(RichText::new(d).size(11.5).color(p.faint));
                    }
                    if rows > 1 {
                        ui.add(egui::TextEdit::multiline(v).desired_rows(rows).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                    } else {
                        ui.add(egui::TextEdit::singleline(v).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                    }
                    ui.add_space(8.0);
                };
                field(ui, "Nome", "", &mut c.nome, 1);
                field(ui, "Meses", "Um por linha: Nome | nome local | dias", &mut c.meses, 8);
                field(ui, "Dias da semana", "Um por linha: Nome | nome local", &mut c.semana, 4);
                ui.horizontal(|ui| {
                    kit::toggle(ui, &mut c.por_mes, p);
                    ui.label(RichText::new("A semana recomeça a cada mês").color(p.text));
                });
                ui.add_space(8.0);
                field(ui, "Nome da semana", "Ex.: seisseno", &mut c.semana_nome, 1);
                field(ui, "Nome do 30º dia", "Ex.: Dia da Lua", &mut c.dia_extra, 1);
                field(ui, "Mês intercalar", "Nome | nome local | dias (vazio = sem intercalar)", &mut c.inter, 1);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("depois do mês nº").size(12.0).color(p.muted));
                    ui.add(egui::TextEdit::singleline(&mut c.depois).desired_width(40.0));
                    ui.label(RichText::new("ciclo de anos").size(12.0).color(p.muted));
                    ui.add(egui::TextEdit::singleline(&mut c.ciclo).desired_width(40.0));
                    ui.label(RichText::new("anos com intercalar").size(12.0).color(p.muted));
                    ui.add(egui::TextEdit::singleline(&mut c.anos).hint_text(RichText::new("3, 6, 8, 11, 14, 17, 19").color(p.faint)).desired_width(f32::INFINITY));
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Eras: antes").size(12.0).color(p.muted));
                    ui.add(egui::TextEdit::singleline(&mut c.antes).desired_width(60.0));
                    ui.label(RichText::new("depois").size(12.0).color(p.muted));
                    ui.add(egui::TextEdit::singleline(&mut c.dep).desired_width(60.0));
                    ui.label(RichText::new("ano do presente").size(12.0).color(p.muted));
                    ui.add(egui::TextEdit::singleline(&mut c.presente).desired_width(80.0));
                });
                ui.add_space(8.0);
                field(ui, "O dia", "Divisões do dia, em texto livre", &mut c.dia, 3);
            });
            Self::footer(ui, p, "Salvar")
        });
        let Some((save, cancel)) = res else { return };
        if save {
            let c = self.cal_edit.take().unwrap();
            let split = |l: &str| -> Vec<String> { l.split('|').map(|x| x.trim().to_string()).collect() };
            let meses: Vec<Json> = c.meses.lines().map(split).filter(|r| !r[0].is_empty()).map(|r| json!({ "nome": r[0], "local": r.get(1).cloned().unwrap_or_default(), "dias": r.get(2).and_then(|x| x.parse::<i64>().ok()).unwrap_or(30) })).collect();
            let semana: Vec<Json> = c.semana.lines().map(split).filter(|r| !r[0].is_empty()).map(|r| json!({ "nome": r[0], "local": r.get(1).cloned().unwrap_or_default() })).collect();
            let it = split(&c.inter);
            let inter = if it[0].is_empty() {
                Json::Null
            } else {
                json!({ "nome": it[0], "local": it.get(1).cloned().unwrap_or_default(), "dias": it.get(2).and_then(|x| x.parse::<i64>().ok()).unwrap_or(30), "depois": c.depois.trim().parse::<i64>().unwrap_or(6), "ciclo": c.ciclo.trim().parse::<i64>().unwrap_or(0), "anos": c.anos.split([',', ' ']).filter_map(|x| x.trim().parse::<i64>().ok()).filter(|x| *x > 0).collect::<Vec<_>>() })
            };
            if let Some(w) = self.world_mut() {
                let mut cal = if w["calendario"].is_object() { w["calendario"].clone() } else { json!({}) };
                cal["nome"] = json!(c.nome);
                cal["meses"] = json!(meses);
                cal["semana"] = json!(semana);
                cal["semanaPorMes"] = json!(c.por_mes);
                cal["semanaNome"] = json!(c.semana_nome);
                cal["diaExtra"] = json!(c.dia_extra);
                cal["eraAntes"] = json!(if c.antes.trim().is_empty() { "AC" } else { c.antes.trim() });
                cal["eraDepois"] = json!(if c.dep.trim().is_empty() { "DC" } else { c.dep.trim() });
                cal["presente"] = json!(c.presente.trim().parse::<i64>().ok());
                cal["dia"] = json!(c.dia);
                cal["intercalar"] = inter;
                w["calendario"] = cal;
            }
            self.changed();
        } else if cancel {
            self.cal_edit = None;
        }
    }

    fn tree_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some((nome, id)) = self.new_tree.as_mut() else { return };
        let renaming = !id.is_empty();
        let res = Self::window(ctx, "mu-tree-new", 400.0, |ui| {
            ui.label(RichText::new(if renaming { "Renomear árvore" } else { "Nova árvore" }).font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            let r = ui.add(egui::TextEdit::singleline(nome).hint_text(RichText::new("Casa Vanasbern · Família Minerva").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
            r.request_focus();
            let (mut s, c) = Self::footer(ui, p, if renaming { "Salvar" } else { "Criar" });
            s |= ui.input(|i| i.key_pressed(egui::Key::Enter));
            (s, c)
        });
        let Some((save, cancel)) = res else { return };
        if save && !nome.trim().is_empty() {
            let (nome, id) = self.new_tree.take().unwrap();
            if id.is_empty() {
                let w = self.world().cloned().unwrap_or(Json::Null);
                let nid = en::uid("tree");
                self.data["arvores"].as_array_mut().unwrap().push(json!({ "id": nid, "nome": nome.trim(), "mundo": w["id"], "casa": "", "cor": w["cor"], "pessoas": [] }));
                self.tree = Some(nid);
            } else if let Some(t) = self.data["arvores"].as_array_mut().and_then(|a| a.iter_mut().find(|t| t["id"] == id.as_str())) {
                t["nome"] = json!(nome.trim());
            }
            self.changed();
        } else if cancel {
            self.new_tree = None;
        }
    }

    fn recat_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(key) = self.recat.clone() else { return };
        let mut chosen = None;
        let res = Self::window(ctx, "mu-recat", 340.0, |ui| {
            ui.label(RichText::new("Categoria deste verbete").font(kit::medium(15.0)).color(p.text));
            ui.add_space(6.0);
            for (k, n, _) in mu::CATS {
                if kit::Row::new(n).icon(super::mundo_ui::cat_icon(k)).show(ui, p).clicked() {
                    chosen = Some(k);
                }
            }
            ui.input(|i| i.key_pressed(egui::Key::Escape))
        });
        if let Some(k) = chosen {
            self.data["classif"][&key] = json!(k);
            self.recat = None;
            self.last_check = None;
            self.changed();
        } else if res == Some(true) {
            self.recat = None;
        }
    }

    fn terms_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some((key, t)) = self.retermos.as_mut() else { return };
        let res = Self::window(ctx, "mu-terms", 440.0, |ui| {
            ui.label(RichText::new("Nomes procurados nos capítulos").font(kit::medium(15.0)).color(p.text));
            ui.label(RichText::new("Separe por vírgula. O Códice mostra em quais capítulos estes nomes aparecem.").size(12.0).color(p.muted));
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(t).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
            Self::footer(ui, p, "Salvar")
        });
        let Some((save, cancel)) = res else { return };
        if save {
            let list: Vec<String> = t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let key = key.clone();
            self.data["termos"][&key] = json!(list);
            self.retermos = None;
            self.last_check = None;
            self.apps = None;
            self.changed();
        } else if cancel {
            self.retermos = None;
        }
    }
}

fn push_unique(x: &mut Json, k: &str, v: &str) {
    if !x[k].is_array() {
        x[k] = json!([]);
    }
    let a = x[k].as_array_mut().unwrap();
    if !a.iter().any(|y| y == v) {
        a.push(json!(v));
    }
}

fn trim2(x: &mut Json) {
    if let Some(a) = x["pais"].as_array_mut() {
        while a.len() > 2 {
            a.remove(0);
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn cal_edit_of(c: &Json) -> CalEdit {
    let ms: Vec<String> = c["meses"].as_array().map(|a| a.iter().map(|m| format!("{} | {} | {}", str_of(m, "nome"), str_of(m, "local"), m["dias"].as_i64().unwrap_or(30))).collect()).unwrap_or_else(|| (1..=12).map(|i| format!("Mês {i} |  | 30")).collect());
    let sem: Vec<String> = c["semana"].as_array().map(|a| a.iter().map(|s| if str_of(s, "local").is_empty() { str_of(s, "nome").to_string() } else { format!("{} | {}", str_of(s, "nome"), str_of(s, "local")) }).collect()).unwrap_or_else(|| ["Um", "Dois", "Três", "Quatro", "Cinco", "Seis", "Sete"].iter().map(|x| x.to_string()).collect());
    let ic = &c["intercalar"];
    CalEdit {
        nome: c["nome"].as_str().unwrap_or("Calendário").into(),
        meses: ms.join("\n"),
        semana: sem.join("\n"),
        por_mes: c["semanaPorMes"].as_bool().unwrap_or(false),
        semana_nome: str_of(c, "semanaNome").into(),
        dia_extra: str_of(c, "diaExtra").into(),
        inter: if ic.is_object() { format!("{} | {} | {}", str_of(ic, "nome"), str_of(ic, "local"), ic["dias"].as_i64().unwrap_or(30)) } else { String::new() },
        depois: ic["depois"].as_i64().unwrap_or(6).to_string(),
        ciclo: ic["ciclo"].as_i64().unwrap_or(0).to_string(),
        anos: ic["anos"].as_array().map(|a| a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        antes: c["eraAntes"].as_str().unwrap_or("AC").into(),
        dep: c["eraDepois"].as_str().unwrap_or("DC").into(),
        presente: c["presente"].as_i64().map(|x| x.to_string()).unwrap_or_else(|| if c.is_object() { String::new() } else { "1".into() }),
        dia: str_of(c, "dia").into(),
    }
}

fn era_edit_of(er: &Json, cal: &Json) -> EraEdit {
    EraEdit {
        id: er["id"].as_str().map(String::from),
        nome: str_of(er, "nome").into(),
        de: er["de"].as_f64().map(|v| mu::year(v, cal)).unwrap_or_default(),
        ate: er["ate"].as_f64().map(|v| mu::year(v, cal)).unwrap_or_default(),
        nativo: str_of(er, "nativo").into(),
        desc: str_of(er, "desc").into(),
        cor: er["cor"].as_str().unwrap_or("#8a6d3b").into(),
    }
}

/// A table cell with its bold/italic/code marks, and the search hit marked.
fn rich_cell(c: &str, q: &str, p: Palette) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = c.replace("**", "").replace('`', "");
    let bold = c.contains("**");
    let base = egui::TextFormat { font_id: if bold { kit::medium(13.0) } else { egui::FontId::proportional(13.0) }, color: p.text, ..Default::default() };
    let q = q.trim();
    if q.is_empty() {
        job.append(&plain, 0.0, base);
        return job;
    }
    // match char by char without accents, so "agua" marks "água"
    let chars: Vec<char> = plain.chars().collect();
    let folded: Vec<String> = chars.iter().map(|c| mu::deaccent(&c.to_string())).collect();
    let ql: Vec<char> = mu::deaccent(q).chars().collect();
    let mut hit = None;
    'outer: for i in 0..chars.len() {
        let mut k = 0;
        let mut j = i;
        while k < ql.len() {
            let Some(f) = folded.get(j) else { continue 'outer };
            for fc in f.chars() {
                if k >= ql.len() || fc != ql[k] {
                    continue 'outer;
                }
                k += 1;
            }
            j += 1;
        }
        hit = Some((i, j));
        break;
    }
    match hit {
        Some((i, j)) => {
            let s: String = chars[..i].iter().collect();
            let m: String = chars[i..j].iter().collect();
            let e: String = chars[j..].iter().collect();
            job.append(&s, 0.0, base.clone());
            let mut hl = base.clone();
            hl.background = p.highlight;
            job.append(&m, 0.0, hl);
            job.append(&e, 0.0, base);
        }
        None => job.append(&plain, 0.0, base),
    }
    job
}

/// A chip that opens a list.
pub(super) fn pick(ui: &mut egui::Ui, p: Palette, id: &str, text: &str, items: &[(String, bool)]) -> Option<usize> {
    let r = kit::chip(ui, None, text, false, p);
    let popup = ui.make_persistent_id(("mu-pick", id));
    if r.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    let mut out = None;
    egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(220.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, (l, on)) in items.iter().enumerate() {
                if kit::menu_check(ui, *on, l, p) {
                    out = Some(i);
                }
            }
        });
    });
    out
}
