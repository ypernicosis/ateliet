//! Enredo, native (continued): the side drawer for whatever is selected,
//! and the Cronologia, Pistas and Arcos views.

use super::enredo_ui::{note_name, str_field, truncated, EnredoView, Sel};
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::{self, Book, STATUS};
use ateliet_core::scriptorium::enredo::{self as en, Col};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::path::Path;

/// An era being created or edited.
pub struct EraEdit {
    pub id: Option<String>,
    pub nome: String,
    pub de: String,
    pub ate: String,
    pub cor: String,
}

const ERA_CORES: [&str; 6] = ["#c9a45c", "#7a8fbf", "#8fae7a", "#bf7a8f", "#9a7abf", "#7abfb4"];

fn kicker(ui: &mut egui::Ui, p: Palette, text: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(text.to_uppercase()).font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::icon_button_sized(ui, icons::X, "Fechar (Esc)", false, p, 24.0, 14.0).clicked() {
                close = true;
            }
        });
    });
    close
}

fn title_edit(ui: &mut egui::Ui, p: Palette, obj: &mut Json, key: &str, hint: &str, focus: &mut bool) -> bool {
    let (ch, r) = str_field(ui, obj, key, |ui, t| ui.add(egui::TextEdit::multiline(t).frame(false).desired_rows(1).desired_width(f32::INFINITY).font(kit::bold(20.0)).hint_text(RichText::new(hint).color(p.faint))));
    if std::mem::take(focus) {
        r.request_focus();
    }
    // Enter ends the title instead of breaking the line
    if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Some(s) = obj[key].as_str() {
            obj[key] = json!(s.replace('\n', ""));
        }
        r.surrender_focus();
        let n = ui.ctx().cumulative_pass_nr();
        ui.data_mut(|d| d.insert_temp(egui::Id::new("en-focus-next"), n));
    }
    ch
}

fn area(ui: &mut egui::Ui, p: Palette, obj: &mut Json, key: &str, hint: &str, rows: usize) -> bool {
    str_field(ui, obj, key, |ui, t| ui.add(egui::TextEdit::multiline(t).desired_rows(rows).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)).hint_text(RichText::new(hint).color(p.faint)))).0
}

/// The summary under a title (takes the focus when Enter ends the title).
fn plain_area(ui: &mut egui::Ui, p: Palette, obj: &mut Json, key: &str, hint: &str, rows: usize) -> bool {
    let flag = egui::Id::new("en-focus-next");
    let id = ui.make_persistent_id(("en-area", key));
    if ui.data(|d| d.get_temp::<u64>(flag)).is_some_and(|n| n < ui.ctx().cumulative_pass_nr()) {
        ui.data_mut(|d| d.remove_temp::<u64>(flag));
        ui.memory_mut(|m| m.request_focus(id));
    }
    str_field(ui, obj, key, |ui, t| ui.add(egui::TextEdit::multiline(t).id(id).frame(false).desired_rows(rows).desired_width(f32::INFINITY).font(egui::FontId::proportional(14.0)).text_color(p.muted).hint_text(RichText::new(hint).color(p.faint)))).0
}

fn line(ui: &mut egui::Ui, p: Palette, label: &str, obj: &mut Json, key: &str, hint: &str) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(96.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(RichText::new(label).color(p.muted));
        });
        ch = str_field(ui, obj, key, |ui, t| ui.add(egui::TextEdit::singleline(t).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 4)).hint_text(RichText::new(hint).color(p.faint)))).0;
    });
    ch
}

fn label(ui: &mut egui::Ui, p: Palette, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(12.0).color(p.muted));
}

/// A section that folds; open state kept per title.
fn sec(ui: &mut egui::Ui, p: Palette, title: &str, glyph: &str, default_open: bool, body: impl FnOnce(&mut egui::Ui)) {
    let id = ui.make_persistent_id(("en-sec", title.split(" ·").next().unwrap_or(title)));
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    ui.add_space(8.0);
    kit::hairline(ui, p);
    ui.add_space(4.0);
    if kit::Row::new(title).icon(glyph).chevron(open).show(ui, p).clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        ui.add_space(4.0);
        ui.indent(id, body);
    }
}

/// A chip that opens a list; returns the index picked.
fn pick(ui: &mut egui::Ui, p: Palette, id: &str, glyph: &str, text: &str, active: bool, items: &[(String, bool)]) -> Option<usize> {
    let r = kit::chip(ui, Some(glyph), text, active, p);
    let popup = ui.make_persistent_id(("en-pick", id));
    if r.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    let mut out = None;
    egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(220.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, (l, on)) in items.iter().enumerate() {
                if kit::Row::new(l).selected(*on).show(ui, p).clicked() {
                    out = Some(i);
                }
            }
        });
    });
    out
}

fn scene_label(data: &Json, id: &str) -> String {
    match en::scene_index(data, id) {
        Some(i) => {
            let t = en::str_of(&data["cenas"][i], "titulo");
            if t.is_empty() { "Cena sem título".into() } else { t.to_string() }
        }
        None => "(cena removida)".into(),
    }
}

fn nice_step(raw: f32) -> f32 {
    let e = 10f32.powf(raw.max(1e-6).log10().floor());
    let f = raw / e;
    (if f < 1.5 { 1.0 } else if f < 3.5 { 2.0 } else if f < 7.5 { 5.0 } else { 10.0 }) * e
}

impl EnredoView {
    // ── drawer ────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub(super) fn drawer(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, cols: &[Col], chars: &[String], root: &Path, ix: &Index, acts: &mut Vec<Action>) {
        let Some(sel) = self.sel.clone() else { return };
        egui::ScrollArea::vertical().id_salt("en-drawer-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.set_max_width(ui.available_width() - 10.0);
            let changed = match &sel {
                Sel::Cena(id) => self.draw_scene(ui, p, id, cols, chars, acts),
                Sel::Trama(id) => self.draw_trama(ui, p, id),
                Sel::Plan(id) => self.draw_plan(ui, p, id, b, root, ix, acts),
                Sel::Pista(id) => self.draw_pista(ui, p, id, cols),
                Sel::Arco(id) => self.draw_arc(ui, p, id, cols, acts),
                Sel::Evento(id) => self.draw_event(ui, p, id),
            };
            if changed {
                self.changed();
            }
            ui.add_space(30.0);
        });
    }

    fn missing(&mut self, ui: &mut egui::Ui, p: Palette, what: &str) -> bool {
        if kicker(ui, p, what) {
            self.sel = None;
        }
        ui.label(RichText::new("Não encontrado (foi excluído?).").color(p.faint));
        false
    }

    fn delete_button(&mut self, ui: &mut egui::Ui, p: Palette, key: &str, label: &str) -> bool {
        if self.confirm.as_deref() == Some(key) {
            let mut yes = false;
            ui.horizontal(|ui| {
                if kit::danger(ui, "Excluir de vez", p).clicked() {
                    yes = true;
                }
                if kit::button(ui, None, "Cancelar", p).clicked() {
                    self.confirm = None;
                }
            });
            if yes {
                self.confirm = None;
            }
            return yes;
        }
        if kit::quiet(ui, Some(icons::TRASH_2), label, p).clicked() {
            self.confirm = Some(key.to_string());
        }
        false
    }

    fn draw_scene(&mut self, ui: &mut egui::Ui, p: Palette, id: &str, cols: &[Col], chars: &[String], acts: &mut Vec<Action>) -> bool {
        let Some(i) = en::scene_index(&self.data, id) else { return self.missing(ui, p, "Cena") };
        if kicker(ui, p, "Cena") {
            self.sel = None;
            return false;
        }
        let mut ch = false;
        let mut focus = std::mem::take(&mut self.focus_title);
        let tramas: Vec<(String, String, String)> = self.data["tramas"].as_array().into_iter().flatten().map(|t| (en::str_of(t, "id").to_string(), en::str_of(t, "nome").to_string(), en::str_of(t, "cor").to_string())).collect();
        let s = &mut self.data["cenas"][i];
        ch |= title_edit(ui, p, s, "titulo", "Título da cena", &mut focus);
        ch |= plain_area(ui, p, s, "resumo", "O que acontece? (uma ou duas frases)", 2);
        ui.add_space(6.0);
        // chapter and point of view
        let cap = en::cap_of(s).map(String::from);
        let mut new_trama_for = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let cur = cols.iter().find(|c| Some(&c.path) == cap.as_ref());
            let text = cur.map(|c| format!("{} · {}", c.label, c.title)).unwrap_or_else(|| "Ideias".into());
            let mut items = vec![("Ideias (sem capítulo)".to_string(), cap.is_none())];
            items.extend(cols.iter().map(|c| (format!("{} · {}", c.label, c.title), Some(&c.path) == cap.as_ref())));
            if let Some(k) = pick(ui, p, "cap", icons::BOOK_OPEN, &text, false, &items) {
                s["cap"] = if k == 0 { Json::Null } else { json!(cols[k - 1].path) };
                s["ord"] = json!(9999);
                ch = true;
            }
            let pov = en::str_of(s, "pov").to_string();
            let names: Vec<String> = chars.iter().map(|c| note_name(c)).collect();
            let mut items = vec![("Sem POV".to_string(), pov.is_empty())];
            items.extend(names.iter().map(|n| (n.clone(), *n == pov)));
            if let Some(k) = pick(ui, p, "pov", icons::EYE, if pov.is_empty() { "POV" } else { &pov }, false, &items) {
                s["pov"] = json!(if k == 0 { String::new() } else { names[k - 1].clone() });
                ch = true;
            }
        });
        // plot lines
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let mine: Vec<String> = en::tramas_of(s).into_iter().map(String::from).collect();
            for t in &mine {
                let Some((_, nome, cor)) = tramas.iter().find(|x| &x.0 == t) else { continue };
                egui::Frame::new().fill(p.bg_secondary).corner_radius(13.0).inner_margin(egui::Margin { left: 9, right: 4, top: 3, bottom: 3 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (r, _) = ui.allocate_exact_size(vec2(8.0, 18.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, super::color(cor));
                        ui.label(RichText::new(nome).size(12.5).color(p.text));
                        if kit::icon_button_sized(ui, icons::X, "Tirar desta trama", false, p, 18.0, 11.0).clicked() {
                            let keep: Vec<&String> = mine.iter().filter(|x| *x != t).collect();
                            s["tramas"] = json!(keep);
                            ch = true;
                        }
                    });
                });
            }
            let mut items: Vec<(String, bool)> = tramas.iter().filter(|t| !mine.contains(&t.0)).map(|t| (t.1.clone(), false)).collect();
            items.push(("Nova trama…".into(), false));
            if let Some(k) = pick(ui, p, "trama", icons::PLUS, "trama", false, &items) {
                if k + 1 == items.len() {
                    new_trama_for = Some(id.to_string());
                } else {
                    let tid = tramas.iter().filter(|t| !mine.contains(&t.0)).nth(k).map(|t| t.0.clone()).unwrap_or_default();
                    let mut l = mine.clone();
                    l.push(tid);
                    s["tramas"] = json!(l);
                    ch = true;
                }
            }
        });
        // status
        ui.add_space(4.0);
        let st = en::str_of(s, "status").to_string();
        let cur = STATUS.iter().position(|x| x.0 == st).unwrap_or(0);
        let items: Vec<(&str, &str)> = STATUS.iter().map(|x| ("", x.1)).collect();
        if let Some(k) = kit::segmented(ui, &items, cur, p) {
            s["status"] = json!(STATUS[k].0);
            ch = true;
        }
        // details
        let has_details = ["local", "quando", "objetivo", "conflito", "desfecho", "valor"].iter().any(|k| !en::str_of(s, k).is_empty());
        sec(ui, p, "Detalhes", icons::LIST, has_details, |ui| {
            ch |= line(ui, p, "Local", s, "local", "Onde?");
            ch |= line(ui, p, "Quando", s, "quando", "Ex.: Ano 3, inverno");
            let q = en::str_of(s, "quando");
            if !q.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(100.0);
                    ui.label(RichText::new(match en::parse_quando(q) {
                        Some(k) => format!("na cronologia: {}", en::fmt_quando(k)),
                        None => "sem data na cronologia".into(),
                    }).size(11.5).color(p.faint));
                });
            }
            for (k, l) in [("objetivo", "Objetivo"), ("conflito", "Conflito"), ("desfecho", "Desfecho")] {
                label(ui, p, l);
                ch |= area(ui, p, s, k, "", 2);
            }
            ch |= line(ui, p, "Valor em jogo", s, "valor", "Ex.: confiança → traição");
        });
        if let Some(sid) = new_trama_for {
            self.new_trama = Some((String::new(), Some(sid)));
        }
        // clues
        let in_pistas: Vec<(String, String, bool)> = self.data["pistas"].as_array().into_iter().flatten().filter_map(|pi| {
            let pl = pi["plantios"].as_array().is_some_and(|l| l.iter().any(|x| x["cena"] == id));
            let co = pi["colheitas"].as_array().is_some_and(|l| l.iter().any(|x| x["cena"] == id));
            (pl || co).then(|| (en::str_of(pi, "id").to_string(), en::str_of(pi, "nome").to_string(), pl))
        }).collect();
        let all_pistas: Vec<(String, String)> = self.data["pistas"].as_array().into_iter().flatten().map(|pi| (en::str_of(pi, "id").to_string(), en::str_of(pi, "nome").to_string())).collect();
        let mut link: Option<(&str, Option<String>)> = None;
        let mut goto = None;
        sec(ui, p, "Pistas", icons::KEY_ROUND, !in_pistas.is_empty(), |ui| {
            for (pid, nome, planted) in &in_pistas {
                if kit::Row::new(&format!("{}: {}", if *planted { "planta" } else { "colhe" }, if nome.is_empty() { "pista" } else { nome })).icon(if *planted { icons::SPROUT } else { icons::WHEAT }).show(ui, p).clicked() {
                    goto = Some(Sel::Pista(pid.clone()));
                }
            }
            ui.horizontal(|ui| {
                let mut items: Vec<(String, bool)> = all_pistas.iter().map(|x| (if x.1.is_empty() { "Pista sem nome".into() } else { x.1.clone() }, false)).collect();
                items.push(("Nova pista".into(), false));
                if let Some(k) = pick(ui, p, "plantar", icons::SPROUT, "Plantar aqui", false, &items) {
                    link = Some(("plantios", all_pistas.get(k).map(|x| x.0.clone())));
                }
                if let Some(k) = pick(ui, p, "colher", icons::WHEAT, "Colher aqui", false, &items) {
                    link = Some(("colheitas", all_pistas.get(k).map(|x| x.0.clone())));
                }
            });
        });
        if let Some((kind, pid)) = link {
            let pid = pid.unwrap_or_else(|| {
                let nid = en::uid("p");
                self.data["pistas"].as_array_mut().unwrap().push(json!({ "id": nid, "nome": "", "tipo": "pista", "estado": "aberta", "plantios": [], "colheitas": [], "notas": "" }));
                goto = Some(Sel::Pista(nid.clone()));
                self.focus_title = true;
                nid
            });
            if let Some(pi) = self.data["pistas"].as_array_mut().and_then(|a| a.iter_mut().find(|x| x["id"] == pid.as_str())) {
                if !pi[kind].is_array() {
                    pi[kind] = json!([]);
                }
                pi[kind].as_array_mut().unwrap().push(json!({ "cena": id }));
                if kind == "colheitas" && pi["estado"] == "aberta" {
                    pi["estado"] = json!("paga");
                }
            }
            ch = true;
        }
        // character arcs
        let marked = self.data["arcos"].as_array().is_some_and(|a| a.iter().any(|x| x["pontos"].as_array().is_some_and(|l| l.iter().any(|pt| pt["cena"] == id))));
        let mut arcs = std::mem::take(&mut self.data["arcos"]);
        sec(ui, p, "Arcos", icons::TRENDING_UP, marked, |ui| {
            let list = arcs.as_array_mut();
            if list.as_ref().is_none_or(|l| l.is_empty()) {
                ui.label(RichText::new("Crie arcos na visão Arcos.").size(12.0).color(p.faint));
            }
            for a in list.into_iter().flatten() {
                let name = note_name(en::str_of(a, "personagem"));
                if !a["pontos"].is_array() {
                    a["pontos"] = json!([]);
                }
                let pts = a["pontos"].as_array_mut().unwrap();
                let at = pts.iter().position(|x| x["cena"] == id);
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(vec2(110.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&name).color(p.text)).truncate());
                    });
                    match at {
                        Some(k) => {
                            let mut n = pts[k]["nivel"].as_i64().unwrap_or(0) as i32;
                            if ui.add(egui::Slider::new(&mut n, -3..=3).show_value(true)).on_hover_text("−3 = preso à mentira · +3 = vive a verdade").changed() {
                                pts[k]["nivel"] = json!(n);
                                ch = true;
                            }
                            if kit::icon_button_sized(ui, icons::X, "Desmarcar", false, p, 20.0, 12.0).clicked() {
                                pts.remove(k);
                                ch = true;
                            }
                        }
                        None => {
                            if kit::quiet(ui, Some(icons::PLUS), "marcar", p).clicked() {
                                pts.push(json!({ "cena": id, "nivel": 0, "nota": "" }));
                                ch = true;
                            }
                        }
                    }
                });
            }
        });
        self.data["arcos"] = arcs;
        // notes
        let s = &mut self.data["cenas"][i];
        let has_notes = !en::str_of(s, "notas").is_empty();
        sec(ui, p, "Notas", icons::STICKY_NOTE, has_notes, |ui| {
            ch |= area(ui, p, s, "notas", "Pesquisa, dúvidas, alternativas…", 4);
        });
        ui.add_space(10.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
        let cap_file = cap.filter(|c| !c.starts_with("plan:"));
        let mut dup = false;
        let mut del = false;
        ui.horizontal_wrapped(|ui| {
            if let Some(c) = &cap_file {
                if kit::quiet(ui, Some(icons::BOOK_OPEN), "Abrir capítulo", p).clicked() {
                    acts.push(Action::Open(c.clone(), true));
                }
            }
            if kit::quiet(ui, Some(icons::COPY), "Duplicar", p).clicked() {
                dup = true;
            }
            del = self.delete_button(ui, p, id, "Excluir");
        });
        if del {
            ui.label(RichText::new("O texto dos capítulos não é tocado.").size(11.5).color(p.faint));
        }
        if dup {
            if let Some(n) = en::duplicate_scene(&mut self.data, id) {
                self.sel = Some(Sel::Cena(n));
                ch = true;
            }
        }
        if del {
            en::delete_scene(&mut self.data, id);
            self.sel = None;
            ch = true;
        }
        if goto.is_some() {
            self.sel = goto;
        }
        ch
    }

    fn draw_trama(&mut self, ui: &mut egui::Ui, p: Palette, id: &str) -> bool {
        let Some(i) = self.data["tramas"].as_array().and_then(|a| a.iter().position(|t| t["id"] == id)) else { return self.missing(ui, p, "Trama") };
        if kicker(ui, p, "Trama") {
            self.sel = None;
            return false;
        }
        let mut ch = false;
        let mut focus = std::mem::take(&mut self.focus_title);
        let n_scenes: Vec<Option<String>> = self.data["cenas"].as_array().into_iter().flatten().filter(|s| en::tramas_of(s).contains(&id)).map(|s| en::cap_of(s).map(String::from)).collect();
        let t = &mut self.data["tramas"][i];
        ch |= title_edit(ui, p, t, "nome", "Nome da trama", &mut focus);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let cur = en::str_of(t, "cor").to_lowercase();
            for c in en::TRAMA_CORES {
                let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                ui.painter().circle_filled(r.center(), if resp.hovered() { 8.5 } else { 7.5 }, super::color(c));
                if cur == c {
                    ui.painter().circle_stroke(r.center(), 10.5, Stroke::new(1.5_f32, p.text));
                }
                if resp.clicked() {
                    t["cor"] = json!(c);
                    ch = true;
                }
            }
        });
        ui.add_space(4.0);
        let tipo = en::str_of(t, "tipo").to_string();
        let tipo = if tipo.is_empty() { "subtrama".to_string() } else { tipo };
        let items: Vec<(String, bool)> = en::TRAMA_TIPOS.iter().map(|x| (x.1.to_string(), x.0 == tipo)).collect();
        let lab = en::TRAMA_TIPOS.iter().find(|x| x.0 == tipo).map(|x| x.1).unwrap_or("Subtrama");
        if let Some(k) = pick(ui, p, "tipo", icons::TAG, lab, false, &items) {
            t["tipo"] = json!(en::TRAMA_TIPOS[k].0);
            ch = true;
        }
        label(ui, p, "Pergunta central");
        ch |= area(ui, p, t, "desc", "Pergunta central desta trama, como começa e como termina…", 4);
        let mut caps: Vec<String> = n_scenes.iter().flatten().cloned().collect();
        caps.sort();
        caps.dedup();
        ui.label(RichText::new(format!("{} cenas em {} capítulos", n_scenes.len(), caps.len())).size(12.0).color(p.faint));
        ui.add_space(10.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
        let mut up = false;
        let mut del = false;
        ui.horizontal_wrapped(|ui| {
            if i > 0 && kit::quiet(ui, Some(icons::ARROW_UP), "Subir", p).clicked() {
                up = true;
            }
            del = self.delete_button(ui, p, id, "Excluir trama");
        });
        if up {
            self.data["tramas"].as_array_mut().unwrap().swap(i, i - 1);
            ch = true;
        }
        if del {
            en::delete_trama(&mut self.data, id);
            self.sel = None;
            ch = true;
        }
        ch
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_plan(&mut self, ui: &mut egui::Ui, p: Palette, id: &str, b: &Book, root: &Path, ix: &Index, acts: &mut Vec<Action>) -> bool {
        let Some(i) = self.data["planejados"].as_array().and_then(|a| a.iter().position(|x| x["id"] == id)) else { return self.missing(ui, p, "Capítulo planejado") };
        let num = self.data["planejados"][i]["num"].as_f64();
        if kicker(ui, p, &match num { Some(n) => format!("Capítulo planejado · {n}"), None => "Capítulo planejado".into() }) {
            self.sel = None;
            return false;
        }
        let mut ch = false;
        let mut focus = false;
        let key = format!("plan:{id}");
        let pl = &mut self.data["planejados"][i];
        let files: Vec<String> = match pl["arquivos"].as_array() {
            Some(a) if !a.is_empty() => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            _ => pl["arquivo"].as_str().filter(|s| !s.is_empty()).map(|s| vec![s.to_string()]).unwrap_or_default(),
        };
        ch |= title_edit(ui, p, pl, "titulo", &num.map(|n| format!("Capítulo {n}")).unwrap_or_else(|| "Título do capítulo".into()), &mut focus);
        ch |= line(ui, p, "Arco", pl, "parte", "Arco / parte");
        label(ui, p, "Notas do roteiro");
        ch |= area(ui, p, pl, "notas", "Notas do roteiro para este capítulo…", 8);
        let notes = en::str_of(pl, "notas").trim().to_string();
        let title = en::str_of(pl, "titulo").to_string();
        let parte = en::str_of(pl, "parte").to_string();
        let n = self.data["cenas"].as_array().into_iter().flatten().filter(|s| en::cap_of(s).is_some_and(|c| c == key || files.iter().any(|f| f == c))).count();
        ui.label(RichText::new(format!("{n} {} planejadas aqui", if n == 1 { "cena" } else { "cenas" })).size(12.0).color(p.faint));
        for f in &files {
            if kit::Row::new(&note_name(f)).icon(icons::FILE_TEXT).show(ui, p).clicked() {
                acts.push(Action::Open(f.clone(), true));
            }
        }
        ui.add_space(10.0);
        let mut del = false;
        let mut write = false;
        ui.horizontal_wrapped(|ui| {
            if files.is_empty() && kit::primary_icon(ui, icons::FEATHER, "Escrever este capítulo", p).clicked() {
                write = true;
            }
            del = self.delete_button(ui, p, id, "Excluir do plano");
        });
        if write {
            let t = if title.is_empty() { num.map(|n| format!("Capítulo {n}")).unwrap_or_else(|| "Capítulo".into()) } else { title };
            match book::create_chapter(root, ix, b, &t, &parte, "capitulo") {
                Ok(path) => {
                    if !notes.is_empty() {
                        let full = root.join(&path);
                        if let Ok(text) = std::fs::read_to_string(&full) {
                            let _ = std::fs::write(&full, format!("{}\n\n%%\nRoteiro (capítulo {}):\n{notes}\n%%\n\n", text.trim_end(), num.map(|n| n.to_string()).unwrap_or_default()));
                        }
                    }
                    en::link_planned(&mut self.data, id, &path);
                    ch = true;
                    acts.push(Action::Saved(path.clone()));
                    acts.push(Action::Open(path, true));
                }
                Err(e) => eprintln!("capítulo planejado: {e}"),
            }
        }
        if del {
            en::delete_planned(&mut self.data, id);
            self.sel = None;
            ch = true;
        }
        ch
    }

    fn scene_items(&self, cols: &[Col]) -> Vec<(String, String)> {
        en::ordered(cols, &self.data)
            .into_iter()
            .map(|i| {
                let s = &self.data["cenas"][i];
                let cap = en::cap_of(s).and_then(|c| cols.iter().find(|x| x.path == c)).map(|c| format!(" — {}", c.title)).unwrap_or_default();
                (en::str_of(s, "id").to_string(), format!("{}{cap}", scene_label(&self.data, en::str_of(s, "id"))))
            })
            .collect()
    }

    fn draw_pista(&mut self, ui: &mut egui::Ui, p: Palette, id: &str, cols: &[Col]) -> bool {
        let Some(i) = self.data["pistas"].as_array().and_then(|a| a.iter().position(|x| x["id"] == id)) else { return self.missing(ui, p, "Pista") };
        if kicker(ui, p, "Pista") {
            self.sel = None;
            return false;
        }
        let mut ch = false;
        let mut focus = std::mem::take(&mut self.focus_title);
        let scenes = self.scene_items(cols);
        let labels: Vec<(String, String)> = ["plantios", "colheitas"]
            .iter()
            .flat_map(|k| self.data["pistas"][i][*k].as_array().cloned().unwrap_or_default())
            .map(|x| (en::str_of(&x, "cena").to_string(), scene_label(&self.data, en::str_of(&x, "cena"))))
            .collect();
        let pi = &mut self.data["pistas"][i];
        ch |= title_edit(ui, p, pi, "nome", "O que é plantado?", &mut focus);
        ui.add_space(4.0);
        let tipo = en::str_of(pi, "tipo").to_string();
        let items: Vec<(String, bool)> = en::PISTA_TIPOS.iter().map(|x| (x.1.to_string(), x.0 == tipo)).collect();
        if let Some(k) = pick(ui, p, "ptipo", icons::TAG, en::PISTA_TIPOS.iter().find(|x| x.0 == tipo).map(|x| x.1).unwrap_or("Pista"), false, &items) {
            pi["tipo"] = json!(en::PISTA_TIPOS[k].0);
            ch = true;
        }
        let est = ["aberta", "paga", "abandonada"];
        let cur = est.iter().position(|x| pi["estado"] == *x).unwrap_or(0);
        if let Some(k) = kit::segmented(ui, &[("", "Aberta"), ("", "Paga"), ("", "Abandonada")], cur, p) {
            pi["estado"] = json!(est[k]);
            ch = true;
        }
        let mut goto = None;
        for (kind, title, glyph) in [("plantios", "Plantada em", icons::SPROUT), ("colheitas", "Paga em", icons::WHEAT)] {
            sec(ui, p, title, glyph, true, |ui| {
                if !pi[kind].is_array() {
                    pi[kind] = json!([]);
                }
                let list = pi[kind].as_array_mut().unwrap();
                let mut drop = None;
                for (k, x) in list.iter().enumerate() {
                    let sid = en::str_of(x, "cena").to_string();
                    let l = labels.iter().find(|y| y.0 == sid).map(|y| y.1.clone()).unwrap_or_default();
                    ui.horizontal(|ui| {
                        let w = ui.available_width() - 26.0;
                        ui.allocate_ui_with_layout(vec2(w, 28.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            if kit::Row::new(&l).icon(icons::CLAPPERBOARD).show(ui, p).clicked() {
                                goto = Some(Sel::Cena(sid.clone()));
                            }
                        });
                        if kit::icon_button_sized(ui, icons::X, "Tirar", false, p, 20.0, 12.0).clicked() {
                            drop = Some(k);
                        }
                    });
                }
                if let Some(k) = drop {
                    list.remove(k);
                    ch = true;
                }
                let items: Vec<(String, bool)> = scenes.iter().map(|s| (s.1.clone(), false)).collect();
                if let Some(k) = pick(ui, p, kind, icons::PLUS, "cena", false, &items) {
                    list.push(json!({ "cena": scenes[k].0 }));
                    if kind == "colheitas" && pi["estado"] == "aberta" {
                        pi["estado"] = json!("paga");
                    }
                    ch = true;
                }
            });
        }
        label(ui, p, "Notas");
        let pi = &mut self.data["pistas"][i];
        ch |= area(ui, p, pi, "notas", "Como o leitor deve perceber (ou não perceber) esta pista?", 3);
        ui.add_space(10.0);
        if self.delete_button(ui, p, id, "Excluir pista") {
            self.data["pistas"].as_array_mut().unwrap().remove(i);
            self.sel = None;
            ch = true;
        }
        if goto.is_some() {
            self.sel = goto;
        }
        ch
    }

    fn draw_arc(&mut self, ui: &mut egui::Ui, p: Palette, id: &str, cols: &[Col], acts: &mut Vec<Action>) -> bool {
        let Some(i) = self.data["arcos"].as_array().and_then(|a| a.iter().position(|x| x["id"] == id)) else { return self.missing(ui, p, "Arco") };
        if kicker(ui, p, "Arco de personagem") {
            self.sel = None;
            return false;
        }
        let mut ch = false;
        let order: Vec<String> = en::ordered(cols, &self.data).into_iter().map(|k| en::str_of(&self.data["cenas"][k], "id").to_string()).collect();
        let labels: Vec<(String, String)> = order.iter().map(|s| (s.clone(), scene_label(&self.data, s))).collect();
        let a = &mut self.data["arcos"][i];
        let who = en::str_of(a, "personagem").to_string();
        ui.label(RichText::new(note_name(&who)).font(kit::bold(20.0)).color(p.text));
        let tipo = en::str_of(a, "tipo").to_string();
        let items: Vec<(String, bool)> = en::ARCO_TIPOS.iter().map(|x| (x.1.to_string(), x.0 == tipo)).collect();
        if let Some(k) = pick(ui, p, "atipo", icons::TRENDING_UP, en::ARCO_TIPOS.iter().find(|x| x.0 == tipo).map(|x| x.1).unwrap_or(en::ARCO_TIPOS[0].1), false, &items) {
            a["tipo"] = json!(en::ARCO_TIPOS[k].0);
            ch = true;
        }
        for (k, l, ph) in [
            ("mentira", "A mentira em que acredita", "Ex.: deve haver uma razão para a dor existir"),
            ("verdade", "A verdade que precisa aprender", "Ex.: o amor não precisa de motivo"),
            ("desejo", "O que deseja", ""),
            ("necessidade", "Do que precisa", ""),
            ("ferida", "A ferida (o porquê da mentira)", ""),
        ] {
            label(ui, p, l);
            ch |= area(ui, p, a, k, ph, 2);
        }
        let mut pts: Vec<(usize, String, i64)> = a["pontos"].as_array().into_iter().flatten().map(|pt| {
            let sid = en::str_of(pt, "cena").to_string();
            (order.iter().position(|x| *x == sid).unwrap_or(usize::MAX), sid, pt["nivel"].as_i64().unwrap_or(0))
        }).collect();
        pts.sort();
        let mut goto = None;
        sec(ui, p, &format!("Pontos do arco · {}", pts.len()), icons::TRENDING_UP, true, |ui| {
            for (_, sid, n) in &pts {
                let l = labels.iter().find(|x| &x.0 == sid).map(|x| x.1.clone()).unwrap_or_else(|| "(removida)".into());
                if kit::Row::new(&l).trailing(format!("{}{n}", if *n > 0 { "+" } else { "" })).show(ui, p).clicked() {
                    goto = Some(Sel::Cena(sid.clone()));
                }
            }
            ui.label(RichText::new("Marque pontos no painel de cada cena (seção Arcos).").size(11.5).color(p.faint));
        });
        ui.add_space(10.0);
        let mut del = false;
        ui.horizontal_wrapped(|ui| {
            if kit::quiet(ui, Some(icons::CONTACT), "Abrir ficha", p).clicked() {
                acts.push(Action::Open(who.clone(), true));
            }
            del = self.delete_button(ui, p, id, "Excluir arco");
        });
        if del {
            self.data["arcos"].as_array_mut().unwrap().remove(i);
            self.sel = None;
            ch = true;
        }
        if goto.is_some() {
            self.sel = goto;
        }
        ch
    }

    fn draw_event(&mut self, ui: &mut egui::Ui, p: Palette, id: &str) -> bool {
        let Some(i) = self.data["eventos"].as_array().and_then(|a| a.iter().position(|x| x["id"] == id)) else { return self.missing(ui, p, "Evento") };
        if kicker(ui, p, "Evento do mundo") {
            self.sel = None;
            return false;
        }
        let mut ch = false;
        let mut focus = std::mem::take(&mut self.focus_title);
        let tramas: Vec<(String, String)> = self.data["tramas"].as_array().into_iter().flatten().map(|t| (en::str_of(t, "id").to_string(), en::str_of(t, "nome").to_string())).collect();
        let ev = &mut self.data["eventos"][i];
        ch |= title_edit(ui, p, ev, "titulo", "O que aconteceu?", &mut focus);
        ch |= line(ui, p, "Quando", ev, "quando", "Ex.: 1288, Ano 3 inverno");
        ui.horizontal(|ui| {
            ui.add_space(100.0);
            ui.label(RichText::new(match en::parse_quando(en::str_of(ev, "quando")) {
                Some(k) => format!("na cronologia: {}", en::fmt_quando(k)),
                None => "sem data reconhecível".into(),
            }).size(11.5).color(p.faint));
        });
        let t0 = ev["tramas"].get(0).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mut items = vec![("Mundo (sem trama)".to_string(), t0.is_empty())];
        items.extend(tramas.iter().map(|t| (t.1.clone(), t.0 == t0)));
        let lab = tramas.iter().find(|t| t.0 == t0).map(|t| t.1.clone()).unwrap_or_else(|| "Mundo".into());
        if let Some(k) = pick(ui, p, "evtrama", icons::WAYPOINTS, &lab, false, &items) {
            ev["tramas"] = if k == 0 { json!([]) } else { json!([tramas[k - 1].0]) };
            ch = true;
        }
        label(ui, p, "Descrição");
        ch |= area(ui, p, ev, "desc", "Contexto, consequências, quem estava lá…", 5);
        ui.add_space(10.0);
        if self.delete_button(ui, p, id, "Excluir evento") {
            self.data["eventos"].as_array_mut().unwrap().remove(i);
            self.sel = None;
            ch = true;
        }
        ch
    }

    // ── creating ──────────────────────────────────────────────────────────

    pub(super) fn new_event(&mut self) {
        let id = en::uid("e");
        self.data["eventos"].as_array_mut().unwrap().push(json!({ "id": id, "titulo": "", "quando": "", "desc": "", "tramas": [] }));
        self.sel = Some(Sel::Evento(id));
        self.focus_title = true;
        self.changed();
    }

    pub(super) fn new_pista(&mut self) {
        let id = en::uid("p");
        self.data["pistas"].as_array_mut().unwrap().push(json!({ "id": id, "nome": "", "tipo": "pista", "estado": "aberta", "plantios": [], "colheitas": [], "notas": "" }));
        self.sel = Some(Sel::Pista(id));
        self.focus_title = true;
        self.changed();
    }

    pub(super) fn new_arc(&mut self) {
        self.arc_pick = true;
    }

    pub(super) fn arc_dialog(&mut self, ctx: &egui::Context, p: Palette, ix: &Index) {
        if !self.arc_pick {
            return;
        }
        let author = String::new();
        let chars: Vec<String> = book::book_by_path(ix, &self.book, &author).map(|b| book::characters(ix, &b)).unwrap_or_default();
        let free: Vec<String> = chars.into_iter().filter(|c| !self.data["arcos"].as_array().is_some_and(|a| a.iter().any(|x| x["personagem"] == c.as_str()))).collect();
        let mut close = false;
        let mut chosen = None;
        kit::scrim(ctx, p);
        egui::Window::new("novo-arco").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 120.0]).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Arco de qual personagem?").font(kit::medium(16.0)).color(p.text));
            ui.add_space(10.0);
            if free.is_empty() {
                ui.label(RichText::new("Todos os personagens deste livro já têm arco (ou o livro não tem personagens).").color(p.muted));
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for c in &free {
                    if kit::Row::new(&note_name(c)).icon(icons::USER).show(ui, p).clicked() {
                        chosen = Some(c.clone());
                    }
                }
            });
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        if let Some(c) = chosen {
            let id = en::uid("a");
            self.data["arcos"].as_array_mut().unwrap().push(json!({ "id": id, "personagem": c, "tipo": "positivo", "mentira": "", "verdade": "", "desejo": "", "necessidade": "", "ferida": "", "pontos": [] }));
            self.sel = Some(Sel::Arco(id));
            self.arc_pick = false;
            self.changed();
        } else if close {
            self.arc_pick = false;
        }
    }

    pub(super) fn era_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(e) = self.era.as_mut() else { return };
        let mut save = false;
        let mut close = false;
        let mut delete = false;
        kit::scrim(ctx, p);
        egui::Window::new("era").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 120.0]).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(if e.id.is_some() { "Era" } else { "Nova era" }).font(kit::medium(16.0)).color(p.text));
            ui.add_space(12.0);
            for (l, v, h) in [("Nome", &mut e.nome, "Ex.: Era dos Estelares"), ("Começa em", &mut e.de, "Ex.: 1200, Ano 3"), ("Termina em", &mut e.ate, "Ex.: 1350")] {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(vec2(90.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new(l).color(p.muted));
                    });
                    ui.add(egui::TextEdit::singleline(v).hint_text(h).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5)));
                });
            }
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(vec2(90.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Cor").color(p.muted));
                });
                for c in ERA_CORES {
                    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 7.5, super::color(c));
                    if e.cor.eq_ignore_ascii_case(c) {
                        ui.painter().circle_stroke(r.center(), 10.0, Stroke::new(1.5_f32, p.text));
                    }
                    if resp.clicked() {
                        e.cor = c.to_string();
                    }
                }
            });
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary(ui, "Salvar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                if e.id.is_some() {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if kit::quiet(ui, Some(icons::TRASH_2), "Excluir", p).clicked() {
                            delete = true;
                        }
                    });
                }
            });
        });
        if save && !e.nome.trim().is_empty() {
            let v = json!({ "nome": e.nome.trim(), "de": en::parse_quando(&e.de), "ate": en::parse_quando(&e.ate), "cor": e.cor });
            let eras = self.data["eras"].as_array_mut().unwrap();
            match e.id.clone().and_then(|id| eras.iter_mut().find(|x| x["id"] == id.as_str())) {
                Some(x) => {
                    for k in ["nome", "de", "ate", "cor"] {
                        x[k] = v[k].clone();
                    }
                }
                None => {
                    let mut v = v;
                    v["id"] = json!(en::uid("r"));
                    eras.push(v);
                }
            }
            self.era = None;
            self.changed();
        } else if delete {
            let id = e.id.clone().unwrap_or_default();
            self.data["eras"].as_array_mut().unwrap().retain(|x| x["id"] != id.as_str());
            self.era = None;
            self.changed();
        } else if close {
            self.era = None;
        }
    }

    // ── Cronologia ────────────────────────────────────────────────────────

    pub(super) fn cronologia(&mut self, ui: &mut egui::Ui, p: Palette, _b: &Book, cols: &[Col]) {
        struct Item {
            scene: bool,
            id: String,
            title: String,
            quando: String,
            qk: f64,
            lane: String,
            color: Color32,
            read: usize,
            row: usize,
        }
        let mut items: Vec<Item> = Vec::new();
        let mut undated: Vec<usize> = Vec::new();
        let tname = |d: &Json, id: Option<&str>| id.and_then(|i| en::trama(d, i)).map(|t| (en::str_of(t, "nome").to_string(), super::color(en::str_of(t, "cor"))));
        for (read, i) in en::ordered(cols, &self.data).into_iter().enumerate() {
            let s = &self.data["cenas"][i];
            if !self.passes(s) {
                continue;
            }
            let (lane, color) = tname(&self.data, en::tramas_of(s).first().copied()).unwrap_or(("Sem trama".into(), p.faint));
            match en::parse_quando(en::str_of(s, "quando")) {
                Some(qk) => items.push(Item { scene: true, id: en::str_of(s, "id").into(), title: en::str_of(s, "titulo").into(), quando: en::str_of(s, "quando").into(), qk, lane, color, read: read + 1, row: 0 }),
                None => undated.push(i),
            }
        }
        for ev in self.data["eventos"].as_array().into_iter().flatten() {
            if let Some(qk) = en::parse_quando(en::str_of(ev, "quando")) {
                let (lane, color) = tname(&self.data, ev["tramas"].get(0).and_then(|x| x.as_str())).unwrap_or(("Mundo".into(), p.muted));
                items.push(Item { scene: false, id: en::str_of(ev, "id").into(), title: en::str_of(ev, "titulo").into(), quando: en::str_of(ev, "quando").into(), qk, lane, color, read: 0, row: 0 });
            }
        }
        let eras: Vec<Json> = self.data["eras"].as_array().cloned().unwrap_or_default();
        // tools
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            let auto = ui.data(|d| d.get_temp::<f32>(egui::Id::new("crono-auto"))).unwrap_or(1.0);
            if kit::icon_button(ui, icons::ZOOM_OUT, "Afastar", false, p).clicked() {
                self.crono_zoom = Some(self.crono_zoom.unwrap_or(auto) / 1.4);
            }
            if kit::icon_button(ui, icons::ZOOM_IN, "Aproximar", false, p).clicked() {
                self.crono_zoom = Some(self.crono_zoom.unwrap_or(auto) * 1.4);
            }
            if kit::icon_button(ui, icons::MAXIMIZE, "Ajustar", false, p).clicked() {
                self.crono_zoom = None;
            }
            ui.add_space(8.0);
            if kit::chip(ui, Some(icons::ROUTE), "Ordem de leitura", self.reading_order, p).on_hover_text("Liga as cenas na ordem em que o leitor as lê: mostra flashbacks e saltos no tempo").clicked() {
                self.reading_order = !self.reading_order;
            }
            ui.add_space(4.0);
            if kit::quiet(ui, Some(icons::PLUS), "Era", p).clicked() {
                self.era = Some(EraEdit { id: None, nome: String::new(), de: String::new(), ate: String::new(), cor: ERA_CORES[0].into() });
            }
        });
        ui.add_space(8.0);
        let mut select = None;
        let mut edit_era = None;
        egui::ScrollArea::both().id_salt("en-crono").auto_shrink([false, false]).show(ui, |ui| {
            if items.is_empty() {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.label(kit::icon(icons::CALENDAR_RANGE, 32.0).color(p.faint));
                    ui.add_space(12.0);
                    ui.label(RichText::new("A linha do tempo do mundo").font(kit::medium(17.0)).color(p.text));
                    ui.add_space(6.0);
                    ui.allocate_ui(vec2(460.0, 80.0), |ui| {
                        ui.label(RichText::new("Preencha \"Quando\" nas cenas (por exemplo \"Ano 3, inverno\", \"2016-03-04\", \"1288\", \"500 a.C.\") ou crie eventos do mundo que não aparecem no livro. A ordem aqui é a da história, não a da narração.").color(p.muted));
                    });
                    ui.add_space(14.0);
                    if kit::primary_icon(ui, icons::PLUS, "Novo evento", p).clicked() {
                        select = Some(Sel::Evento(String::new()));
                    }
                });
            } else {
                let mut min = items.iter().map(|i| i.qk).fold(f64::INFINITY, f64::min);
                let mut max = items.iter().map(|i| i.qk).fold(f64::NEG_INFINITY, f64::max);
                for e in &eras {
                    if let Some(v) = e["de"].as_f64() {
                        min = min.min(v);
                    }
                    if let Some(v) = e["ate"].as_f64() {
                        max = max.max(v);
                    }
                }
                if max - min < 1.0 {
                    min -= 1.0;
                    max += 1.0;
                }
                let pad = (max - min) * 0.04;
                min -= pad;
                max += pad;
                const LAB: f32 = 150.0;
                const CW: f32 = 156.0;
                const RH: f32 = 42.0;
                let w = ui.available_width().max(400.0);
                let auto = ((w - LAB - CW - 30.0) as f64 / (max - min)).max(1e-4) as f32;
                ui.data_mut(|d| d.insert_temp(egui::Id::new("crono-auto"), auto));
                let ppu = self.crono_zoom.unwrap_or(auto) as f64;
                let mut lanes: Vec<String> = self.data["tramas"].as_array().into_iter().flatten().map(|t| en::str_of(t, "nome").to_string()).collect();
                for it in &items {
                    if !lanes.contains(&it.lane) {
                        lanes.push(it.lane.clone());
                    }
                }
                lanes.retain(|l| items.iter().any(|i| &i.lane == l));
                items.sort_by(|a, b| a.qk.partial_cmp(&b.qk).unwrap_or(std::cmp::Ordering::Equal));
                let x_of = |v: f64| LAB + ((v - min) * ppu) as f32;
                let mut rows_end: std::collections::HashMap<String, Vec<f32>> = Default::default();
                for it in items.iter_mut() {
                    let rows = rows_end.entry(it.lane.clone()).or_default();
                    let x = x_of(it.qk);
                    let r = rows.iter().position(|end| *end < x - 4.0).unwrap_or_else(|| {
                        rows.push(0.0);
                        rows.len() - 1
                    });
                    rows[r] = x + CW;
                    it.row = r;
                }
                let mut lane_top = std::collections::HashMap::new();
                let mut y = 34.0;
                for l in &lanes {
                    lane_top.insert(l.clone(), y);
                    y += 14.0 + rows_end.get(l).map(|r| r.len()).unwrap_or(1).max(1) as f32 * RH;
                }
                let h = y + 10.0;
                let total_w = x_of(max) + CW + 30.0;
                let (rect, _) = ui.allocate_exact_size(vec2(total_w.max(w), h), Sense::hover());
                let o = rect.min;
                let painter = ui.painter().clone();
                let clip = ui.clip_rect();
                // eras
                for e in &eras {
                    let Some(de) = e["de"].as_f64() else { continue };
                    let ate = e["ate"].as_f64().unwrap_or(de);
                    let r = Rect::from_min_max(pos2(o.x + x_of(de), o.y + 22.0), pos2(o.x + x_of(ate).max(x_of(de) + 2.0), o.y + h));
                    let c = super::color(e["cor"].as_str().unwrap_or("#c9a45c"));
                    painter.rect_filled(r, 4.0, c.gamma_multiply(0.12));
                    painter.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0_f32, c.gamma_multiply(0.6)));
                    painter.text(r.left_top() + vec2(6.0, 4.0), egui::Align2::LEFT_TOP, en::str_of(e, "nome"), kit::medium(11.0), c);
                    let resp = ui.interact(Rect::from_min_size(r.left_top(), vec2(r.width().max(60.0), 18.0)), ui.id().with(("era", en::str_of(e, "id"))), Sense::click()).on_hover_text("Editar era");
                    if resp.clicked() {
                        edit_era = Some(e.clone());
                    }
                }
                // ticks
                let mut step = nice_step(((max - min) / ((total_w - LAB) / 110.0).max(4.0) as f64) as f32) as f64;
                if max - min >= 2.0 {
                    step = step.max(1.0);
                }
                let mut v = (min / step).ceil() * step;
                while v <= max {
                    let x = o.x + x_of(v);
                    painter.line_segment([pos2(x, o.y + 18.0), pos2(x, o.y + h)], Stroke::new(1.0_f32, p.border.gamma_multiply(0.7)));
                    painter.text(pos2(x + 4.0, o.y + 4.0), egui::Align2::LEFT_TOP, en::fmt_quando(v + 1e-9), egui::FontId::proportional(11.0), p.faint);
                    v += step;
                }
                // lanes
                for l in &lanes {
                    let top = o.y + lane_top[l];
                    let lh = 14.0 + rows_end.get(l).map(|r| r.len()).unwrap_or(1).max(1) as f32 * RH - 6.0;
                    painter.line_segment([pos2(o.x, top + lh + 3.0), pos2(o.x + total_w.max(w), top + lh + 3.0)], Stroke::new(1.0_f32, p.border));
                }
                // reading order
                let mut centers: Vec<(usize, egui::Pos2)> = Vec::new();
                for it in &items {
                    let top = o.y + lane_top[&it.lane] + 8.0 + it.row as f32 * RH;
                    let r = Rect::from_min_size(pos2(o.x + x_of(it.qk), top), vec2(CW - 6.0, 32.0));
                    if it.scene {
                        centers.push((it.read, pos2(r.left() + 4.0, r.center().y)));
                    }
                    if !r.intersects(clip) {
                        continue;
                    }
                    let resp = ui.interact(r, ui.id().with(("crono", &it.id)), Sense::click()).on_hover_text(&it.quando);
                    let sel = match &self.sel {
                        Some(Sel::Cena(id)) | Some(Sel::Evento(id)) => id == &it.id,
                        _ => false,
                    };
                    painter.rect(r, 7.0, if it.scene { p.bg } else { p.bg_secondary }, Stroke::new(if sel { 1.5_f32 } else { 1.0 }, if sel { p.accent_strong } else if resp.hovered() { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
                    painter.rect_filled(Rect::from_min_size(r.min, vec2(3.0, r.height())), egui::CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 }, it.color);
                    let mut x = r.left() + 10.0;
                    if it.scene {
                        let g = painter.layout_no_wrap(format!("#{}", it.read), egui::FontId::proportional(10.5), p.faint);
                        let gw = g.size().x;
                        painter.galley(pos2(x, r.center().y - g.size().y / 2.0), g, p.faint);
                        x += gw + 6.0;
                    } else {
                        painter.text(pos2(x + 5.0, r.center().y), egui::Align2::CENTER_CENTER, icons::MAP_PIN, kit::icon_font(12.0), p.muted);
                        x += 16.0;
                    }
                    let t = if it.title.is_empty() { if it.scene { "Cena sem título" } else { "Evento" } } else { &it.title };
                    let g = truncated(ui, t, kit::medium(12.0), p.text, r.right() - x - 6.0);
                    painter.galley(pos2(x, r.center().y - g.size().y / 2.0), g, p.text);
                    if resp.clicked() {
                        select = Some(if it.scene { Sel::Cena(it.id.clone()) } else { Sel::Evento(it.id.clone()) });
                    }
                }
                if self.reading_order && centers.len() > 1 {
                    centers.sort_by_key(|c| c.0);
                    let pts: Vec<egui::Pos2> = centers.iter().map(|c| c.1).collect();
                    for w in pts.windows(2) {
                        let shapes = egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.5_f32, p.accent_strong.gamma_multiply(0.8)), 5.0, 4.0);
                        painter.extend(shapes);
                        let d = (w[1] - w[0]).normalized();
                        let m = w[0] + (w[1] - w[0]) * 0.5;
                        let n = vec2(-d.y, d.x);
                        painter.add(egui::Shape::convex_polygon(vec![m + d * 5.0, m - d * 4.0 + n * 4.0, m - d * 4.0 - n * 4.0], p.accent_strong.gamma_multiply(0.8), Stroke::NONE));
                    }
                }
                // lane labels, sticky on the left
                let lx = o.x.max(clip.left());
                for l in &lanes {
                    let top = o.y + lane_top[l];
                    let lr = Rect::from_min_size(pos2(lx, top + 4.0), vec2(LAB - 14.0, 26.0));
                    painter.rect_filled(lr.expand2(vec2(6.0, 2.0)), 6.0, p.bg.gamma_multiply(0.92));
                    let c = items.iter().find(|i| &i.lane == l).map(|i| i.color).unwrap_or(p.faint);
                    painter.circle_filled(pos2(lr.left() + 8.0, lr.center().y), 4.0, c);
                    let g = truncated(ui, l, kit::medium(12.5), p.text, LAB - 40.0);
                    painter.galley(pos2(lr.left() + 18.0, lr.center().y - g.size().y / 2.0), g, p.text);
                }
            }
            if !undated.is_empty() {
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new(format!("SEM DATA · {}", undated.len())).font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
                });
                ui.add_space(6.0);
                egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 0)).show(ui, |ui| ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(ui.clip_rect().width() - 40.0);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for &i in undated.iter().take(80) {
                        let s = self.data["cenas"][i].clone();
                        let (r, resp) = ui.allocate_exact_size(vec2(180.0, 50.0), Sense::click());
                        self.paint_card(ui, r, &s, p, resp.hovered(), false);
                        if resp.clicked() {
                            select = Some(Sel::Cena(en::str_of(&s, "id").into()));
                        }
                    }
                }));
                ui.add_space(30.0);
            }
        });
        if let Some(e) = edit_era {
            let f = |v: &Json| v.as_f64().map(|x| if x.fract() == 0.0 { format!("{x}") } else { format!("{x:.2}") }).unwrap_or_default();
            self.era = Some(EraEdit { id: Some(en::str_of(&e, "id").into()), nome: en::str_of(&e, "nome").into(), de: f(&e["de"]), ate: f(&e["ate"]), cor: e["cor"].as_str().unwrap_or(ERA_CORES[0]).into() });
        }
        match select {
            Some(Sel::Evento(id)) if id.is_empty() => self.new_event(),
            Some(s) => {
                self.sel = Some(s);
                self.confirm = None;
            }
            None => {}
        }
    }

    // ── Pistas ────────────────────────────────────────────────────────────

    pub(super) fn pistas(&mut self, ui: &mut egui::Ui, p: Palette, _b: &Book, cols: &[Col]) {
        let all: Vec<Json> = self.data["pistas"].as_array().cloned().unwrap_or_default();
        let abertas = all.iter().filter(|x| x["estado"] == "aberta").count();
        let pagas = all.iter().filter(|x| x["estado"] == "paga").count();
        let sem: Vec<String> = all.iter().filter(|x| x["estado"] == "aberta" && x["plantios"].as_array().is_some_and(|l| !l.is_empty()) && x["colheitas"].as_array().is_none_or(|l| l.is_empty())).map(|x| en::str_of(x, "nome").to_string()).collect();
        let mut select = None;
        if all.is_empty() {
            ui.add_space(90.0);
            ui.vertical_centered(|ui| {
                ui.label(kit::icon(icons::KEY_ROUND, 32.0).color(p.faint));
                ui.add_space(12.0);
                ui.label(RichText::new("Plantar e colher").font(kit::medium(17.0)).color(p.text));
                ui.add_space(6.0);
                ui.allocate_ui(vec2(460.0, 80.0), |ui| {
                    ui.label(RichText::new("Registre cada pista, mistério, promessa ou objeto que precisa voltar depois. Marque a cena onde é plantado e a cena onde é pago: o que ficou sem resposta aparece aqui.").color(p.muted));
                });
                ui.add_space(14.0);
                if kit::primary_icon(ui, icons::PLUS, "Nova pista", p).clicked() {
                    select = Some(());
                }
            });
            if select.is_some() {
                self.new_pista();
            }
            return;
        }
        // columns: ideas, then each chapter
        let col_keys: Vec<Option<&str>> = std::iter::once(None).chain(cols.iter().map(|c| Some(c.path.as_str()))).collect();
        let col_of = |sid: &str| en::scene_index(&self.data, sid).map(|i| en::cap_of(&self.data["cenas"][i])).and_then(|cap| col_keys.iter().position(|k| *k == cap));
        let mut sel = None;
        egui::ScrollArea::both().id_salt("en-pistas").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.spacing_mut().item_spacing.x = 6.0;
                for (t, warn) in [(format!("{abertas} abertas"), false), (format!("{pagas} pagas"), false)] {
                    kit::chip(ui, None, &t, warn, p);
                }
                if !sem.is_empty() {
                    kit::chip(ui, Some(icons::TRIANGLE_ALERT), &format!("{} plantadas sem colheita", sem.len()), true, p).on_hover_text(sem.join(", "));
                }
            });
            ui.add_space(14.0);
            const NAME: f32 = 260.0;
            const CW: f32 = 30.0;
            const RH: f32 = 48.0;
            let n = col_keys.len();
            let w = 20.0 + NAME + CW * n as f32 + 20.0;
            let mut order: Vec<&Json> = all.iter().collect();
            order.sort_by_key(|x| if x["estado"] == "aberta" { 0 } else { 1 });
            let (rect, _) = ui.allocate_exact_size(vec2(w.max(ui.available_width()), 30.0 + RH * order.len() as f32), Sense::hover());
            let o = rect.min + vec2(20.0, 0.0);
            let painter = ui.painter().clone();
            for (i, k) in col_keys.iter().enumerate() {
                let x = o.x + NAME + CW * i as f32 + CW / 2.0;
                let lab = match k {
                    None => "·".to_string(),
                    Some(path) => cols.iter().find(|c| c.path == *path).map(|c| c.label.clone()).unwrap_or_default(),
                };
                painter.text(pos2(x, o.y + 12.0), egui::Align2::CENTER_CENTER, lab, egui::FontId::proportional(11.0), p.faint);
            }
            for (ri, pi) in order.iter().enumerate() {
                let y = o.y + 30.0 + RH * ri as f32;
                let row = Rect::from_min_size(pos2(o.x - 8.0, y), vec2(NAME + CW * n as f32 + 16.0, RH - 4.0));
                let pid = en::str_of(pi, "id").to_string();
                let resp = ui.interact(row, ui.id().with(("pista", &pid)), Sense::click());
                let is_sel = matches!(&self.sel, Some(Sel::Pista(id)) if id == &pid);
                if is_sel || resp.hovered() {
                    painter.rect_filled(row, 8.0, if is_sel { p.accent_soft } else { p.bg_hover });
                }
                let estado = en::str_of(pi, "estado");
                let (glyph, gc) = match estado {
                    "paga" => (icons::CHECK, p.accent_strong),
                    "abandonada" => (icons::X, p.faint),
                    _ => (icons::CIRCLE_DOT, p.muted),
                };
                painter.text(pos2(o.x + 8.0, row.top() + 15.0), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(13.0), gc);
                let nome = en::str_of(pi, "nome");
                let tipo = en::PISTA_TIPOS.iter().find(|x| pi["tipo"] == x.0).map(|x| x.1).unwrap_or("Pista");
                let g = truncated(ui, if nome.is_empty() { "Pista sem nome" } else { nome }, kit::medium(13.0), if estado == "abandonada" { p.faint } else { p.text }, NAME - 40.0);
                painter.galley(pos2(o.x + 22.0, row.top() + 6.0), g, p.text);
                painter.text(pos2(o.x + 22.0, row.top() + 26.0), egui::Align2::LEFT_TOP, tipo, egui::FontId::proportional(11.0), p.faint);
                let pl: Vec<usize> = pi["plantios"].as_array().into_iter().flatten().filter_map(|x| col_of(en::str_of(x, "cena"))).collect();
                let co: Vec<usize> = pi["colheitas"].as_array().into_iter().flatten().filter_map(|x| col_of(en::str_of(x, "cena"))).collect();
                let cx = |i: usize| o.x + NAME + CW * i as f32 + CW / 2.0;
                let cy = row.center().y;
                for i in 0..n {
                    painter.circle_filled(pos2(cx(i), cy), 1.5, p.border_strong);
                }
                if let Some(&a) = pl.iter().min() {
                    let end = co.iter().max().copied();
                    let bx = cx(end.unwrap_or(n - 1));
                    let c = if end.is_some() { p.accent_strong.gamma_multiply(0.5) } else { p.error.gamma_multiply(0.45) };
                    if end.is_some() {
                        painter.line_segment([pos2(cx(a), cy), pos2(bx, cy)], Stroke::new(3.0_f32, c));
                    } else {
                        painter.extend(egui::Shape::dashed_line(&[pos2(cx(a), cy), pos2(bx + CW / 2.0, cy)], Stroke::new(2.0_f32, c), 4.0, 3.0));
                    }
                }
                for &i in &pl {
                    painter.circle(pos2(cx(i), cy), 6.0, p.bg, Stroke::new(2.0_f32, p.accent_strong));
                }
                for &i in &co {
                    painter.circle_filled(pos2(cx(i), cy), 6.0, p.accent_strong);
                }
                if resp.clicked() {
                    sel = Some(Sel::Pista(pid));
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label(RichText::new("○ plantada  ● paga  ┄ ainda sem colheita").size(11.5).color(p.faint));
            });
        });
        if sel.is_some() {
            self.sel = sel;
            self.confirm = None;
        }
    }

    // ── Arcos ─────────────────────────────────────────────────────────────

    pub(super) fn arcos(&mut self, ui: &mut egui::Ui, p: Palette, _b: &Book, cols: &[Col], ix: &Index) {
        let arcs: Vec<Json> = self.data["arcos"].as_array().cloned().unwrap_or_default();
        if arcs.is_empty() {
            let mut go = false;
            ui.add_space(90.0);
            ui.vertical_centered(|ui| {
                ui.label(kit::icon(icons::TRENDING_UP, 32.0).color(p.faint));
                ui.add_space(12.0);
                ui.label(RichText::new("Arcos de personagem").font(kit::medium(17.0)).color(p.text));
                ui.add_space(6.0);
                ui.allocate_ui(vec2(460.0, 80.0), |ui| {
                    ui.label(RichText::new("A curva interna de cada personagem: a mentira em que acredita, a verdade que precisa aprender, e onde, cena a cena, ela avança ou recua (de −3 a +3).").color(p.muted));
                });
                ui.add_space(14.0);
                go = kit::primary_icon(ui, icons::PLUS, "Novo arco", p).clicked();
            });
            if go {
                self.new_arc();
            }
            return;
        }
        let seq: Vec<String> = en::ordered(cols, &self.data).into_iter().map(|i| en::str_of(&self.data["cenas"][i], "id").to_string()).collect();
        let nmax = (seq.len().max(2) - 1) as f32;
        let mut sel = None;
        egui::ScrollArea::vertical().id_salt("en-arcos").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(20.0);
            let avail = ui.available_width() - 40.0;
            let per_row = ((avail + 16.0) / 436.0).floor().max(1.0);
            let card_w = (avail - 16.0 * (per_row - 1.0)) / per_row;
            egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 0)).show(ui, |ui| ui.horizontal_wrapped(|ui| {
                ui.set_max_width(avail);
                ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
                for a in &arcs {
                    let who = en::str_of(a, "personagem");
                    let name = note_name(who);
                    let fm = ix.page(who).map(|pg| pg.yaml.clone()).unwrap_or(Json::Null);
                    let color = super::sheet_ui::parse_color(fm["cor"].as_str().unwrap_or("")).unwrap_or(p.accent_strong);
                    let aid = en::str_of(a, "id").to_string();
                    let (rect, resp) = ui.allocate_exact_size(vec2(card_w, 214.0), Sense::click());
                    let is_sel = matches!(&self.sel, Some(Sel::Arco(id)) if id == &aid);
                    let painter = ui.painter();
                    painter.rect(rect, 12.0, p.bg, Stroke::new(if is_sel { 1.5_f32 } else { 1.0 }, if is_sel { p.accent_strong } else if resp.hovered() { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
                    let av = pos2(rect.left() + 32.0, rect.top() + 32.0);
                    painter.circle_filled(av, 15.0, color.gamma_multiply(0.85));
                    painter.text(av, egui::Align2::CENTER_CENTER, ateliet_core::scriptorium::sheet::initials(&name), kit::medium(12.0), Color32::WHITE);
                    painter.text(pos2(rect.left() + 56.0, rect.top() + 25.0), egui::Align2::LEFT_CENTER, &name, kit::medium(14.5), p.text);
                    let tipo = en::ARCO_TIPOS.iter().find(|x| a["tipo"] == x.0).map(|x| x.1.split(" (").next().unwrap_or(x.1)).unwrap_or("");
                    painter.text(pos2(rect.left() + 56.0, rect.top() + 42.0), egui::Align2::LEFT_CENTER, tipo, egui::FontId::proportional(11.5), p.faint);
                    let (m, v) = (en::str_of(a, "mentira"), en::str_of(a, "verdade"));
                    if !m.is_empty() || !v.is_empty() {
                        let mut job = egui::text::LayoutJob::default();
                        job.append(if m.is_empty() { "—" } else { m }, 0.0, egui::TextFormat { font_id: egui::FontId::new(12.0, egui::FontFamily::Name("italic".into())), color: p.muted, ..Default::default() });
                        job.append("  →  ", 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: p.faint, ..Default::default() });
                        job.append(if v.is_empty() { "—" } else { v }, 0.0, egui::TextFormat { font_id: kit::medium(12.0), color: p.text, ..Default::default() });
                        job.wrap = egui::text::TextWrapping::truncate_at_width(card_w - 40.0);
                        let g = ui.fonts(|f| f.layout_job(job));
                        painter.galley(pos2(rect.left() + 20.0, rect.top() + 62.0), g, p.text);
                    }
                    let chart = Rect::from_min_max(pos2(rect.left() + 20.0, rect.top() + 88.0), pos2(rect.right() - 20.0, rect.bottom() - 18.0));
                    painter.rect_filled(chart, 8.0, p.bg_secondary);
                    let zero = chart.center().y;
                    painter.line_segment([pos2(chart.left() + 8.0, zero), pos2(chart.right() - 8.0, zero)], Stroke::new(1.0_f32, p.border_strong));
                    let mut pts: Vec<(f32, f32)> = a["pontos"].as_array().into_iter().flatten().filter_map(|pt| {
                        let i = seq.iter().position(|s| pt["cena"] == s.as_str())?;
                        Some((i as f32, pt["nivel"].as_f64().unwrap_or(0.0).clamp(-3.0, 3.0) as f32))
                    }).collect();
                    pts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
                    let px = |i: f32| chart.left() + 14.0 + i / nmax * (chart.width() - 28.0);
                    let py = |n: f32| zero - n * (chart.height() / 2.0 - 12.0) / 3.0;
                    let line: Vec<egui::Pos2> = pts.iter().map(|(i, n)| pos2(px(*i), py(*n))).collect();
                    if line.len() > 1 {
                        painter.add(egui::Shape::line(line.clone(), Stroke::new(2.0_f32, color)));
                    }
                    for q in &line {
                        painter.circle(*q, 4.0, p.bg, Stroke::new(2.0_f32, color));
                    }
                    if pts.is_empty() {
                        painter.text(chart.center(), egui::Align2::CENTER_CENTER, "Marque pontos no painel da cena", egui::FontId::proportional(11.5), p.faint);
                    }
                    painter.text(pos2(chart.left() + 6.0, chart.top() + 4.0), egui::Align2::LEFT_TOP, "verdade", egui::FontId::proportional(9.5), p.faint);
                    painter.text(pos2(chart.left() + 6.0, chart.bottom() - 4.0), egui::Align2::LEFT_BOTTOM, "mentira", egui::FontId::proportional(9.5), p.faint);
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        sel = Some(Sel::Arco(aid));
                    }
                }
            }));
            ui.add_space(30.0);
        });
        if sel.is_some() {
            self.sel = sel;
            self.confirm = None;
        }
    }
}

