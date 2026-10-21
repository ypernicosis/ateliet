//! Enredo, native: narrative structures. Each structure applied to a stretch
//! of the book gets a band of beats over the Grade; scenes tie to beats and
//! answer the structure's questions; a dialog chooses where each structure
//! applies, and an editor changes a structure without touching Markdown.

use super::enredo_ui::{truncated, EnredoView, Sel};
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book;
use ateliet_core::scriptorium::enredo::{self as en, Col};
use ateliet_core::scriptorium::estruturas::{self as es, Beat, Group, Layout, Structure};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A structure applied to the book: the application, the structure, and
/// where its beats fall on the chapters.
pub struct App {
    pub a: Json,
    pub st: Structure,
    pub lay: Layout,
}

impl App {
    pub fn id(&self) -> String {
        self.a["id"].as_str().unwrap_or("").to_string()
    }
}

pub fn band_color(k: usize, p: Palette) -> Color32 {
    match k % 4 {
        0 => p.accent_strong,
        1 => Color32::from_rgb(0xb0, 0x7a, 0x36),
        2 => Color32::from_rgb(0x2f, 0x6b, 0x6b),
        _ => Color32::from_rgb(0x8b, 0x5f, 0xbf),
    }
}

pub struct BandBeat {
    pub key: String,
    pub x0: f32,
    pub x1: f32,
    pub lane: usize,
    pub label: String,
    pub tip: String,
    pub scenes: usize,
}

/// A structure's row over the Grade (x in pixels from the first chapter column).
pub struct Band {
    pub app: usize,
    pub h: f32,
    pub range: Option<(f32, f32)>,
    pub beats: Vec<BandBeat>,
    pub groups: Vec<(f32, f32, String)>,
    pub fora: usize,
}

#[derive(Default)]
pub struct StDialog {
    pub focus: Option<String>,
}

pub struct StEditor {
    pub r: Option<String>,
    pub pronta: bool,
    pub m: Structure,
    open: HashSet<String>,
    new_campo: String,
    new_group: String,
    error: Option<String>,
}

fn beat_label(b: &Beat) -> String {
    b.label()
}

impl EnredoView {
    /// A structure by reference, re-read when its note changes.
    pub(super) fn structure(&mut self, root: &Path, r: &str) -> Option<Structure> {
        let mtime = r.strip_prefix("nota:").and_then(|p| std::fs::metadata(root.join(p)).and_then(|m| m.modified()).ok());
        if let Some((m, st)) = self.st_cache.get(r) {
            if *m == mtime {
                return Some(st.clone());
            }
        }
        let st = es::get(root, r)?;
        self.st_cache.insert(r.to_string(), (mtime, st.clone()));
        Some(st)
    }

    pub(super) fn resolve(&mut self, root: &Path, cols: &[Col], words: &[usize]) {
        let by_words = self.data["posModo"] == "palavras";
        let apps: Vec<Json> = self.data["estruturas"].as_array().cloned().unwrap_or_default();
        let mut out = Vec::new();
        for a in apps {
            let Some(st) = self.structure(root, a["ref"].as_str().unwrap_or("")) else { continue };
            let lay = es::layout(&a, &st, cols, words, by_words);
            out.push(App { a, st, lay });
        }
        self.apps = out;
    }

    fn beat_hit(&self, key: &str) -> Option<(usize, &Beat)> {
        let (aid, bid) = key.split_once(':')?;
        let k = self.apps.iter().position(|x| x.id() == aid)?;
        let b = self.apps[k].st.batidas.iter().find(|b| b.id == bid)?;
        Some((k, b))
    }

    fn scenes_of_beat(&self, key: &str) -> Vec<usize> {
        self.data["cenas"].as_array().into_iter().flatten().enumerate().filter(|(_, s)| s["batidas"].as_array().is_some_and(|l| l.iter().any(|x| x == key))).map(|(i, _)| i).collect()
    }

    /// Beats that start at a chapter column (global index).
    pub(super) fn beats_at(&self, col: usize) -> Vec<(String, String, usize)> {
        let mut out = Vec::new();
        for (k, x) in self.apps.iter().enumerate() {
            for b in &x.st.batidas {
                if let Some(m) = x.lay.beats.get(&b.id) {
                    if !m.fora && m.caps.first() == Some(&col) {
                        out.push((format!("{}:{}", x.id(), b.id), beat_label(b), k));
                    }
                }
            }
        }
        out
    }

    /// Bands for the Grade over the columns `lo..lo+n`.
    pub(super) fn bands(&self, ui: &egui::Ui, lo: usize, n: usize, colw: f32) -> Vec<Band> {
        let mut out = Vec::new();
        if n == 0 {
            return out;
        }
        for (k, x) in self.apps.iter().enumerate() {
            let Some(from) = x.lay.from else {
                out.push(Band { app: k, h: 40.0, range: None, beats: vec![], groups: vec![], fora: 0 });
                continue;
            };
            let to = x.lay.to;
            if to < lo || from >= lo + n {
                continue; // not in the arc being shown
            }
            let base = (from as f32 - lo as f32) * colw;
            let span = (to - from + 1) as f32 * colw;
            let px = |f: f64| base + f as f32 * span;
            // compact labels when the beats don't fit with their names
            let need: f32 = x.st.batidas.iter().filter_map(|b| x.lay.beats.get(&b.id)).map(|m| (px(m.x1) - px(m.x0)).max(104.0)).sum();
            let compact = need > span * 1.6;
            let min_w = if compact { 22.0 } else { 104.0 };
            let mut lanes: Vec<f32> = Vec::new();
            let mut beats = Vec::new();
            let mut fora = 0;
            for b in &x.st.batidas {
                let Some(m) = x.lay.beats.get(&b.id) else { continue };
                if m.fora {
                    fora += 1;
                    continue;
                }
                let mut x0 = px(m.x0);
                let mut x1 = px(m.x1).max(x0 + min_w);
                if x1 > base + span {
                    x0 = (x0 - (x1 - base - span)).max(base);
                    x1 = base + span;
                }
                let lane = lanes.iter().position(|end| *end <= x0 + 0.5).unwrap_or_else(|| {
                    lanes.push(0.0);
                    lanes.len() - 1
                });
                lanes[lane] = x1;
                let key = format!("{}:{}", x.id(), b.id);
                let label = if compact { b.num.map(|n| n.to_string()).unwrap_or_else(|| b.nome.chars().take(2).collect()) } else { beat_label(b) };
                let mut tip = b.nome.clone();
                if let Some(f) = b.info("Função") {
                    tip.push_str(&format!("\n{f}"));
                }
                let scenes = self.scenes_of_beat(&key).len();
                beats.push(BandBeat { key, x0, x1, lane, label, tip, scenes });
            }
            let groups = x.st.grupos.iter().filter_map(|g| x.lay.groups.get(&g.nome).map(|m| (px(m.x0), px(m.x1), g.nome.clone()))).collect();
            let _ = ui;
            out.push(Band { app: k, h: 30.0 + lanes.len().max(1) as f32 * 24.0, range: Some((base, base + span)), beats, groups, fora });
        }
        out
    }

    pub(super) fn paint_band(&self, ui: &egui::Ui, band: &Band, o: egui::Pos2, clip: Rect, p: Palette) -> Option<Sel> {
        let painter = ui.painter().with_clip_rect(clip);
        let color = band_color(band.app, p);
        let mut out = None;
        let Some((r0, r1)) = band.range else {
            painter.text(pos2(o.x + 12.0, o.y + band.h / 2.0), egui::Align2::LEFT_CENTER, "O trecho desta estrutura não tem capítulos: clique no nome para escolher outro", egui::FontId::proportional(12.0), p.faint);
            return None;
        };
        painter.rect_filled(Rect::from_min_max(pos2(o.x + r0 + 2.0, o.y + 3.0), pos2(o.x + r1 - 2.0, o.y + band.h - 3.0)), 6.0, color.gamma_multiply(0.06));
        for (x0, x1, name) in &band.groups {
            let r = Rect::from_min_max(pos2(o.x + x0 + 4.0, o.y + 6.0), pos2(o.x + x1 - 4.0, o.y + 22.0));
            painter.line_segment([pos2(r.left(), r.bottom() - 1.0), pos2(r.right(), r.bottom() - 1.0)], Stroke::new(1.0_f32, color.gamma_multiply(0.35)));
            let g = truncated(ui, name, kit::medium(10.5), color.gamma_multiply(0.85), r.width() - 4.0);
            painter.galley(pos2(r.left() + 2.0, r.top()), g, color);
        }
        for b in &band.beats {
            let r = Rect::from_min_max(pos2(o.x + b.x0 + 2.0, o.y + 26.0 + b.lane as f32 * 24.0), pos2(o.x + b.x1 - 2.0, o.y + 46.0 + b.lane as f32 * 24.0));
            let vis = r.intersect(clip);
            let sel = matches!(&self.sel, Some(Sel::Batida(k)) if k == &b.key);
            let mut hov = false;
            if vis.is_positive() {
                let resp = ui.interact(vis, ui.id().with(("beat", &b.key)), Sense::click()).on_hover_text(&b.tip);
                hov = resp.hovered();
                if resp.clicked() {
                    out = Some(Sel::Batida(b.key.clone()));
                }
            }
            let fill = if sel { color } else if b.scenes > 0 { color.gamma_multiply(if hov { 0.32 } else { 0.22 }) } else { color.gamma_multiply(if hov { 0.16 } else { 0.09 }) };
            painter.rect(r, 5.0, fill, Stroke::new(1.0_f32, color.gamma_multiply(if sel { 1.0 } else { 0.35 })), egui::StrokeKind::Inside);
            let fg = if sel { Color32::WHITE } else { p.text };
            let badge = if b.scenes > 0 { 18.0 } else { 0.0 };
            let g = truncated(ui, &b.label, egui::FontId::proportional(11.5), fg, r.width() - 10.0 - badge);
            painter.galley(pos2(r.left() + 5.0, r.center().y - g.size().y / 2.0), g, fg);
            if b.scenes > 0 && r.width() > 40.0 {
                painter.text(pos2(r.right() - 8.0, r.center().y), egui::Align2::CENTER_CENTER, b.scenes.to_string(), kit::medium(10.5), if sel { Color32::WHITE } else { color });
            }
        }
        if band.fora > 0 {
            painter.text(pos2(o.x + r1 - 6.0, o.y + 8.0), egui::Align2::RIGHT_TOP, format!("+{} sem capítulo", band.fora), egui::FontId::proportional(10.5), p.faint);
        }
        out
    }

    /// The band's name in the label column; returns true when clicked.
    pub(super) fn paint_band_label(&self, ui: &egui::Ui, band: &Band, r: Rect, vr: Rect, p: Palette) -> bool {
        let x = &self.apps[band.app];
        let lp = ui.painter().with_clip_rect(vr);
        let resp = ui.interact(vr, ui.id().with(("band-lab", band.app)), Sense::click()).on_hover_text("Mudar o trecho, o modo ou editar a estrutura");
        lp.rect_filled(r, 0.0, if resp.hovered() { p.bg_hover } else { p.bg });
        lp.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0_f32, p.border_strong));
        lp.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, p.border));
        let color = band_color(band.app, p);
        lp.text(pos2(r.left() + 20.0, r.top() + 18.0), egui::Align2::CENTER_CENTER, icons::MILESTONE, kit::icon_font(13.0), color);
        let g = truncated(ui, &x.st.nome, kit::medium(12.5), p.text, r.width() - 46.0);
        lp.galley(pos2(r.left() + 32.0, r.top() + 18.0 - g.size().y / 2.0), g, p.text);
        let cols_label = match x.a["escopo"].as_str().unwrap_or("livro") {
            "livro" => "livro inteiro".to_string(),
            "parte" => x.a["parte"].as_str().unwrap_or("").to_string(),
            _ => format!("{} capítulos", x.lay.list.len()),
        };
        lp.text(pos2(r.left() + 32.0, r.top() + 34.0), egui::Align2::LEFT_CENTER, cols_label, egui::FontId::proportional(11.0), p.faint);
        resp.clicked()
    }

    // ── drawer ────────────────────────────────────────────────────────────

    pub(super) fn draw_beat(&mut self, ui: &mut egui::Ui, p: Palette, key: &str, cols: &[Col]) -> bool {
        let Some((k, b)) = self.beat_hit(key).map(|(k, b)| (k, b.clone())) else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("BATIDA").font(kit::medium(10.5)).color(p.faint));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::icon_button_sized(ui, icons::X, "Fechar", false, p, 24.0, 14.0).clicked() {
                        self.sel = None;
                    }
                });
            });
            ui.label(RichText::new("Batida não encontrada (a estrutura mudou?).").color(p.faint));
            return false;
        };
        let x = &self.apps[k];
        let color = band_color(k, p);
        let mut close = false;
        ui.horizontal(|ui| {
            let kick = format!("{} · {}{}", x.st.nome, es::scope_label(&x.a, cols), if b.grupo.is_empty() { String::new() } else { format!(" · {}", b.grupo) });
            ui.allocate_ui(vec2(ui.available_width() - 32.0, 16.0), |ui| {
                ui.add(egui::Label::new(RichText::new(kick.to_uppercase()).font(kit::medium(10.5)).color(color).extra_letter_spacing(0.8)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button_sized(ui, icons::X, "Fechar (Esc)", false, p, 24.0, 14.0).clicked() {
                    close = true;
                }
            });
        });
        ui.label(RichText::new(beat_label(&b)).font(kit::bold(20.0)).color(p.text));
        let m = x.lay.beats.get(&b.id).cloned();
        let cap_txt = |caps: &[usize]| match caps {
            [] => "entre capítulos".to_string(),
            [c] => format!("no capítulo {} · {}", cols[*c].label, cols[*c].title),
            [a, .., z] => format!("nos capítulos {}–{}", cols[*a].label, cols[*z].label),
        };
        match &m {
            Some(m) if !m.fora => {
                let how = if x.a["modo"] == "sequencial" { " (uma batida por capítulo)".to_string() } else { format!(" · {}–{}% do trecho", b.de().round(), b.ate().round()) };
                ui.label(RichText::new(format!("Cai {}{how}", cap_txt(&m.caps))).size(12.0).color(p.muted));
            }
            _ => {
                ui.label(RichText::new("Não cabe no trecho: são mais batidas do que capítulos. Aumente o trecho ou use as porcentagens.").size(12.0).color(p.error));
            }
        }
        ui.add_space(6.0);
        if !b.desc.is_empty() {
            ui.label(RichText::new(&b.desc).color(p.text));
        }
        for (kk, v) in &b.info {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(kk).size(12.0).color(p.faint));
                ui.label(RichText::new(v).size(13.0).color(p.text));
            });
        }
        if let Some(g) = x.st.grupos.iter().find(|g| g.nome == b.grupo).filter(|g| !g.desc.is_empty()) {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{}: {}", g.nome, g.desc)).size(12.0).italics().color(p.muted));
        }
        // scenes in the beat, and whether they sit where the beat falls
        let scenes = self.scenes_of_beat(key);
        if let Some(m) = m.as_ref().filter(|m| !m.fora && !m.caps.is_empty()) {
            let (lo, hi) = (m.caps[0], *m.caps.last().unwrap());
            let off: Vec<usize> = scenes.iter().filter_map(|&i| en::cap_of(&self.data["cenas"][i])).filter_map(|c| cols.iter().position(|x| x.path == c)).filter(|&i| i < lo || i > hi).collect();
            let placed = scenes.iter().any(|&i| en::cap_of(&self.data["cenas"][i]).is_some());
            if placed {
                ui.add_space(4.0);
                if off.is_empty() {
                    ui.label(RichText::new("✓ As cenas estão no lugar certo").size(12.0).color(p.accent_strong));
                } else {
                    let w = if off.iter().all(|&i| i < lo) { "antes" } else if off.iter().all(|&i| i > hi) { "depois" } else { "fora" };
                    ui.label(RichText::new(format!("{} {} {w} do trecho desta batida", off.len(), if off.len() == 1 { "cena está" } else { "cenas estão" })).size(12.0).color(p.error));
                }
            }
        }
        ui.add_space(10.0);
        kit::section(ui, &format!("Cenas nesta batida · {}", scenes.len()), p);
        let mut goto = None;
        for &i in &scenes {
            let s = self.data["cenas"][i].clone();
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(320.0), 50.0), Sense::click());
            self.paint_card(ui, r, &s, p, resp.hovered(), false);
            if resp.clicked() {
                goto = Some(Sel::Cena(en::str_of(&s, "id").to_string()));
            }
        }
        let mut ch = false;
        if kit::quiet(ui, Some(icons::PLUS), "Nova cena nesta batida", p).clicked() {
            let cap = m.as_ref().and_then(|m| m.caps.first()).map(|&c| cols[c].path.clone());
            let id = en::new_scene(&mut self.data, cap.as_deref(), &[]);
            if let Some(i) = en::scene_index(&self.data, &id) {
                self.data["cenas"][i]["batidas"] = json!([key]);
            }
            goto = Some(Sel::Cena(id));
            self.focus_title = true;
            ch = true;
        }
        let x = &self.apps[k];
        if let Some(mb) = b.espelho.as_ref().and_then(|e| x.st.batidas.iter().find(|y| &y.id == e)).filter(|mb| mb.id != b.id) {
            if kit::Row::new(&format!("Espelho: {}", beat_label(mb))).icon(icons::ARROW_RIGHT_LEFT).show(ui, p).clicked() {
                goto = Some(Sel::Batida(format!("{}:{}", x.id(), mb.id)));
            }
        }
        ui.add_space(10.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
        let r = x.a["ref"].as_str().unwrap_or("").to_string();
        let aid = x.id();
        ui.horizontal_wrapped(|ui| {
            if kit::quiet(ui, Some(icons::PENCIL), "Editar a estrutura", p).clicked() {
                self.st_editor = Some(Box::new(StEditor::open(&r, self.apps[k].st.clone())));
            }
            if kit::quiet(ui, Some(icons::SLIDERS_HORIZONTAL), "Mudar o trecho", p).clicked() {
                self.st_dialog = Some(StDialog { focus: Some(aid.clone()) });
            }
        });
        if close {
            self.sel = None;
        }
        if goto.is_some() {
            self.sel = goto;
        }
        ch
    }

    /// The scene's beats and the structure's questions about it.
    pub(super) fn scene_beats(&mut self, ui: &mut egui::Ui, p: Palette, i: usize, id: &str, cols: &[Col]) -> bool {
        if self.apps.is_empty() {
            return false;
        }
        let mut ch = false;
        let mine: Vec<String> = self.data["cenas"][i]["batidas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let cap_col = en::cap_of(&self.data["cenas"][i]).and_then(|c| cols.iter().position(|x| x.path == c));
        // every beat, the ones around the scene's chapter first
        let mut near = Vec::new();
        let mut rest = Vec::new();
        let many = self.apps.len() > 1;
        for x in &self.apps {
            for b in &x.st.batidas {
                let key = format!("{}:{}", x.id(), b.id);
                let label = if many { format!("{} › {}", x.st.nome, beat_label(b)) } else { beat_label(b) };
                let is_near = cap_col.is_some_and(|c| x.lay.beats.get(&b.id).is_some_and(|m| m.caps.contains(&c)));
                if is_near { near.push((key, label)) } else { rest.push((key, label)) }
            }
        }
        let text = if mine.is_empty() {
            "Batida".to_string()
        } else {
            mine.iter().filter_map(|k| self.beat_hit(k).map(|(_, b)| b.num.map(|n| n.to_string()).unwrap_or_else(|| b.nome.clone()))).collect::<Vec<_>>().join(" · ")
        };
        ui.add_space(4.0);
        let r = kit::chip(ui, Some(icons::MILESTONE), &text, !mine.is_empty(), p).on_hover_text("Uma cena pode ser batida de mais de uma estrutura");
        let popup = ui.make_persistent_id(("scene-beats", id));
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        let mut toggle: Option<String> = None;
        let mut clear = false;
        egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(260.0);
            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                if !mine.is_empty() && kit::menu_item(ui, icons::ERASER, "Tirar de todas as batidas", p) {
                    clear = true;
                }
                if !near.is_empty() {
                    kit::section(ui, "Perto deste capítulo", p);
                    for (k, l) in &near {
                        if kit::menu_check(ui, mine.contains(k), l, p) {
                            toggle = Some(k.clone());
                        }
                    }
                    kit::menu_sep(ui, p);
                }
                for (k, l) in &rest {
                    if kit::menu_check(ui, mine.contains(k), l, p) {
                        toggle = Some(k.clone());
                    }
                }
            });
        });
        if clear {
            self.data["cenas"][i]["batidas"] = json!([]);
            ch = true;
        }
        if let Some(k) = toggle {
            let mut l = mine.clone();
            if let Some(pos) = l.iter().position(|x| *x == k) {
                l.remove(pos);
            } else {
                l.push(k);
            }
            self.data["cenas"][i]["batidas"] = json!(l);
            ch = true;
        }
        // one card per beat: its info and the structure's questions (asked once)
        let mut asked: HashSet<String> = HashSet::new();
        let hits: Vec<(Beat, Vec<String>)> = mine.iter().filter_map(|k| self.beat_hit(k).map(|(a, b)| (b.clone(), self.apps[a].st.campos.clone()))).collect();
        for (b, campos) in hits {
            ui.add_space(6.0);
            egui::Frame::new().fill(p.bg_secondary).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(format!("Ficha · {}", b.nome)).font(kit::medium(12.5)).color(p.text));
                for (kk, v) in &b.info {
                    ui.label(RichText::new(format!("{kk}: {v}")).size(12.0).color(p.muted));
                }
                let s = &mut self.data["cenas"][i];
                if !s["campos"].is_object() {
                    s["campos"] = json!({});
                }
                for c in campos {
                    if !asked.insert(c.clone()) {
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new(&c).size(12.0).color(p.muted));
                    let mut t = s["campos"][&c].as_str().unwrap_or("").to_string();
                    if ui.add(egui::TextEdit::multiline(&mut t).desired_rows(2).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 5))).changed() {
                        s["campos"][&c] = json!(t);
                        ch = true;
                    }
                }
            });
        }
        ch
    }

    // ── which structures apply where ──────────────────────────────────────

    pub(super) fn structures_dialog(&mut self, ctx: &egui::Context, p: Palette, cols: &[Col], ix: &Index, root: &Path) {
        if self.st_dialog.is_none() || self.st_editor.is_some() {
            return;
        }
        let lib = es::list(ix);
        let mut close = false;
        let mut ch = false;
        let mut edit: Option<(Option<String>, Option<Structure>)> = None;
        let mut delete_note: Option<String> = None;
        let focus = self.st_dialog.as_ref().and_then(|d| d.focus.clone());
        let lib_st: Vec<Option<Structure>> = lib.iter().map(|e| self.structure(root, &e.r#ref)).collect();
        kit::scrim(ctx, p);
        egui::Window::new("estruturas").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 60.0]).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Estruturas narrativas").font(kit::medium(16.0)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::icon_button(ui, icons::X, "Fechar (Esc)", false, p).clicked() {
                        close = true;
                    }
                });
            });
            ui.label(RichText::new("Use quantas quiser: uma para o livro, outra para um arco, outra dentro de um capítulo só. Cada uma ganha a própria faixa na Grade, sobre os capítulos em que vale.").size(12.5).color(p.muted));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height(ctx.screen_rect().height() - 220.0).show(ui, |ui| {
                ui.set_width(600.0);
                let n = self.data["estruturas"].as_array().map(|a| a.len()).unwrap_or(0);
                if n == 0 {
                    ui.label(RichText::new("Nenhuma estrutura neste livro ainda.").color(p.faint));
                }
                let mut remove = None;
                let mut up = None;
                for i in 0..n {
                    let a = self.data["estruturas"][i].clone();
                    let aid = en::str_of(&a, "id").to_string();
                    let st = lib.iter().position(|e| a["ref"] == e.r#ref.as_str()).and_then(|k| lib_st[k].clone());
                    let color = band_color(i, p);
                    let frame = egui::Frame::new().stroke(Stroke::new(if focus.as_deref() == Some(aid.as_str()) { 1.5_f32 } else { 1.0 }, if focus.as_deref() == Some(aid.as_str()) { color } else { p.border })).corner_radius(10.0).inner_margin(12.0);
                    let r = frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(10.0, 20.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 5.0, color);
                            let items: Vec<(String, bool)> = lib.iter().map(|e| (format!("{}{}", e.nome, if e.pronta { "" } else { " · sua" }), a["ref"] == e.r#ref.as_str())).collect();
                            let name = st.as_ref().map(|s| s.nome.clone()).unwrap_or_else(|| "(estrutura apagada)".into());
                            if let Some(k) = pick(ui, p, &format!("st-ref-{i}"), &name, &items) {
                                self.data["estruturas"][i]["ref"] = json!(lib[k].r#ref);
                                ch = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if kit::icon_button_sized(ui, icons::X, "Tirar do livro (as cenas perdem só a ligação com estas batidas)", false, p, 24.0, 13.0).clicked() {
                                    remove = Some(i);
                                }
                                if i > 0 && kit::icon_button_sized(ui, icons::ARROW_UP, "Subir (ordem das faixas)", false, p, 24.0, 13.0).clicked() {
                                    up = Some(i);
                                }
                                if st.is_some() && kit::icon_button_sized(ui, icons::PENCIL, "Editar esta estrutura", false, p, 24.0, 13.0).clicked() {
                                    edit = Some((a["ref"].as_str().map(String::from), st.clone()));
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ch |= self.scope_editor(ui, p, i, cols, st.as_ref());
                    });
                    if focus.as_deref() == Some(aid.as_str()) {
                        r.response.scroll_to_me(Some(egui::Align::Center));
                    }
                    ui.add_space(8.0);
                }
                if let Some(i) = remove {
                    let aid = en::str_of(&self.data["estruturas"][i], "id").to_string();
                    self.data["estruturas"].as_array_mut().unwrap().remove(i);
                    for s in self.data["cenas"].as_array_mut().into_iter().flatten() {
                        if let Some(l) = s["batidas"].as_array_mut() {
                            l.retain(|k| !k.as_str().unwrap_or("").starts_with(&format!("{aid}:")));
                        }
                    }
                    ch = true;
                }
                if let Some(i) = up {
                    self.data["estruturas"].as_array_mut().unwrap().swap(i, i - 1);
                    ch = true;
                }
                // add one
                let items: Vec<(String, bool)> = lib.iter().map(|e| (format!("{}{}{}", e.nome, if e.pronta { "" } else { "  ·  sua" }, e.capitulos.map(|c| format!("  ·  {c} capítulos")).unwrap_or_default()), false)).collect();
                ui.horizontal(|ui| {
                    let r = kit::primary_icon(ui, icons::PLUS, if n > 0 { "Usar outra estrutura" } else { "Usar uma estrutura" }, p);
                    let popup = ui.make_persistent_id("st-add");
                    if r.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(popup));
                    }
                    let mut chosen = None;
                    egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(280.0);
                        for (k, (l, _)) in items.iter().enumerate() {
                            if kit::menu_item(ui, icons::MILESTONE, l, p) {
                                chosen = Some(k);
                            }
                        }
                    });
                    if let Some(k) = chosen {
                        self.add_app(&lib[k].r#ref, lib_st[k].as_ref(), cols);
                        ch = true;
                    }
                });
                if self.data["estruturas"].as_array().is_some_and(|a| a.iter().any(|x| x["modo"] != "sequencial")) {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Nas porcentagens, contar o tamanho do trecho").size(12.0).color(p.muted));
                        let cur = if self.data["posModo"] == "palavras" { 1 } else { 0 };
                        if let Some(k) = kit::segmented(ui, &[("", "por capítulos"), ("", "por palavras escritas")], cur, p) {
                            self.data["posModo"] = json!(if k == 1 { "palavras" } else { "capitulos" });
                            ch = true;
                        }
                    });
                }
                // library
                ui.add_space(16.0);
                ui.label(RichText::new("BIBLIOTECA DE ESTRUTURAS").font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
                ui.add_space(4.0);
                for (k, e) in lib.iter().enumerate() {
                    let Some(st) = &lib_st[k] else { continue };
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(&st.nome).font(kit::medium(13.0)).color(p.text));
                            let meta: Vec<String> = [format!("{} batidas", st.batidas.len()), if st.grupos.is_empty() { String::new() } else { format!("{} grupos", st.grupos.len()) }, st.capitulos.map(|c| format!("dura {c} capítulos")).unwrap_or_default(), if e.pronta { "pronta".into() } else { "sua".into() }].into_iter().filter(|x| !x.is_empty()).collect();
                            ui.label(RichText::new(meta.join(" · ")).size(11.5).color(p.faint));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !e.pronta && kit::icon_button_sized(ui, icons::TRASH_2, "Apagar estrutura (vai para a lixeira)", false, p, 24.0, 13.0).clicked() {
                                delete_note = e.r#ref.strip_prefix("nota:").map(String::from);
                            }
                            if kit::button(ui, Some(icons::PENCIL), if e.pronta { "Personalizar" } else { "Editar" }, p).on_hover_text(if e.pronta { "Abre uma cópia sua para editar" } else { "Editar batidas, grupos, duração e perguntas" }).clicked() {
                                edit = Some((Some(e.r#ref.clone()), Some(st.clone())));
                            }
                            let used = self.data["estruturas"].as_array().map(|a| a.iter().filter(|x| x["ref"] == e.r#ref.as_str()).count()).unwrap_or(0);
                            if used > 0 {
                                ui.label(RichText::new(if used == 1 { "em uso".to_string() } else { format!("em uso ×{used}") }).size(11.5).color(p.accent_strong));
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(6.0);
                if kit::quiet(ui, Some(icons::PLUS), "Nova estrutura", p).clicked() {
                    edit = Some((None, None));
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.memory(|m| m.any_popup_open()) {
                close = true;
            }
        });
        if let Some(d) = self.st_dialog.as_mut() {
            d.focus = None;
        }
        if ch {
            self.changed();
        }
        if let Some(p) = delete_note {
            let _ = trash::delete(root.join(&p));
            self.st_cache.remove(&format!("nota:{p}"));
        }
        if let Some((r, st)) = edit {
            self.st_editor = Some(Box::new(match (r, st) {
                (Some(r), Some(st)) => StEditor::open(&r, st),
                _ => StEditor::open("", es::parse(&es::TEMPLATE.replace("Minha estrutura", "Nova estrutura"), "")),
            }));
        }
        if close {
            self.st_dialog = None;
        }
    }

    fn add_app(&mut self, r: &str, st: Option<&Structure>, cols: &[Col]) {
        let mut a = json!({ "id": en::uid("e"), "ref": r, "escopo": "livro", "modo": "proporcional", "numerados": false });
        if let Some(n) = st.and_then(|s| s.capitulos) {
            // a structure with its own length starts at the first numbered chapter not taken yet
            let mut used = HashSet::new();
            for b in self.data["estruturas"].as_array().into_iter().flatten() {
                used.extend(es::scope_of(b, cols));
            }
            let first = (0..cols.len()).find(|&i| cols[i].include && es::is_numbered(&cols[i]) && !used.contains(&i)).or_else(|| (0..cols.len()).find(|&i| es::is_numbered(&cols[i])));
            if let Some(f) = first {
                a["escopo"] = json!("capitulos");
                a["de"] = json!(es::ch_key(&cols[f]));
                a["n"] = json!(n);
                a["numerados"] = json!(true);
                if st.is_some_and(|s| s.batidas.len() as i64 == n) {
                    a["modo"] = json!("sequencial");
                }
            }
        }
        if !self.data["estruturas"].is_array() {
            self.data["estruturas"] = json!([]);
        }
        let id = a["id"].as_str().unwrap_or("").to_string();
        self.data["estruturas"].as_array_mut().unwrap().push(a);
        self.st_dialog = Some(StDialog { focus: Some(id) });
    }

    /// "Vale em" and the stretch; "Batidas" mode; a summary of the result.
    fn scope_editor(&mut self, ui: &mut egui::Ui, p: Palette, i: usize, cols: &[Col], st: Option<&Structure>) -> bool {
        let mut ch = false;
        let a = &mut self.data["estruturas"][i];
        let label_w = 78.0;
        let row_label = |ui: &mut egui::Ui, t: &str| {
            ui.allocate_ui_with_layout(vec2(label_w, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(t).size(12.5).color(p.muted));
            });
        };
        let escopo = a["escopo"].as_str().unwrap_or("livro").to_string();
        let first_num = (0..cols.len()).find(|&k| cols[k].include && es::is_numbered(&cols[k])).or(if cols.is_empty() { None } else { Some(0) });
        ui.horizontal(|ui| {
            row_label(ui, "Vale em");
            let cur = es::ESCOPOS.iter().position(|x| x.0 == escopo).unwrap_or(0);
            let items: Vec<(&str, &str)> = es::ESCOPOS.iter().map(|x| ("", x.1)).collect();
            if let Some(k) = kit::segmented(ui, &items, cur, p) {
                let nk = es::ESCOPOS[k].0;
                if nk != escopo {
                    a["escopo"] = json!(nk);
                    if nk == "parte" && a["parte"].as_str().unwrap_or("").is_empty() {
                        a["parte"] = json!(cols.iter().find(|c| !c.part.is_empty()).map(|c| c.part.clone()).unwrap_or_default());
                    }
                    if (nk == "capitulos" || nk == "capitulo") && a["de"].as_str().unwrap_or("").is_empty() {
                        if let Some(f) = first_num {
                            a["de"] = json!(es::ch_key(&cols[f]));
                        }
                    }
                    if nk == "capitulos" && a["ate"].is_null() && a["n"].is_null() {
                        match st.and_then(|s| s.capitulos) {
                            Some(n) => a["n"] = json!(n),
                            None => a["ate"] = a["de"].clone(),
                        }
                    }
                    if nk == "capitulo" && a["modo"] == "sequencial" {
                        a["modo"] = json!("proporcional");
                    }
                    ch = true;
                }
            }
        });
        let ch_items = |sel: &str| -> Vec<(String, bool)> { cols.iter().map(|c| (format!("{} · {}", c.label, c.title), es::ch_key(c) == sel || c.path == sel)).collect() };
        let ch_name = |sel: &str| cols.iter().find(|c| es::ch_key(c) == sel || c.path == sel).map(|c| format!("{} · {}", c.label, c.title)).unwrap_or_else(|| "escolher".into());
        match escopo.as_str() {
            "parte" => {
                ui.horizontal(|ui| {
                    row_label(ui, "");
                    let parts = en::parts(cols);
                    if parts.is_empty() {
                        ui.label(RichText::new("Nenhum capítulo tem arco/parte definido.").size(12.0).color(p.faint));
                    } else {
                        let cur = a["parte"].as_str().unwrap_or("").to_string();
                        let items: Vec<(String, bool)> = parts.iter().map(|x| (x.clone(), *x == cur)).collect();
                        if let Some(k) = pick(ui, p, &format!("st-part-{i}"), if cur.is_empty() { "escolher" } else { &cur }, &items) {
                            a["parte"] = json!(parts[k]);
                            ch = true;
                        }
                    }
                });
            }
            "capitulos" => {
                ui.horizontal_wrapped(|ui| {
                    row_label(ui, "");
                    ui.label(RichText::new("de").size(12.5).color(p.muted));
                    let de = a["de"].as_str().unwrap_or("").to_string();
                    if let Some(k) = pick(ui, p, &format!("st-de-{i}"), &ch_name(&de), &ch_items(&de)) {
                        a["de"] = json!(es::ch_key(&cols[k]));
                        ch = true;
                    }
                    ui.label(RichText::new("até").size(12.5).color(p.muted));
                    let counting = a["n"].is_u64();
                    let ate = a["ate"].as_str().unwrap_or("").to_string();
                    let mut items = vec![("contar capítulos".to_string(), counting)];
                    items.extend(ch_items(&ate).into_iter().map(|(l, on)| (l, on && !counting)));
                    let text = if counting { "contar capítulos".to_string() } else { ch_name(&ate) };
                    if let Some(k) = pick(ui, p, &format!("st-ate-{i}"), &text, &items) {
                        if k == 0 {
                            a["n"] = json!(st.and_then(|s| s.capitulos).unwrap_or(10));
                            if let Some(o) = a.as_object_mut() {
                                o.remove("ate");
                            }
                        } else {
                            a["ate"] = json!(es::ch_key(&cols[k - 1]));
                            if let Some(o) = a.as_object_mut() {
                                o.remove("n");
                            }
                        }
                        ch = true;
                    }
                    if let Some(mut n) = a["n"].as_u64() {
                        if ui.add(egui::DragValue::new(&mut n).range(1..=400).suffix(" capítulos")).changed() {
                            a["n"] = json!(n);
                            ch = true;
                        }
                    }
                });
            }
            "capitulo" => {
                ui.horizontal_wrapped(|ui| {
                    row_label(ui, "");
                    let de = a["de"].as_str().unwrap_or("").to_string();
                    if let Some(k) = pick(ui, p, &format!("st-cap-{i}"), &ch_name(&de), &ch_items(&de)) {
                        a["de"] = json!(es::ch_key(&cols[k]));
                        ch = true;
                    }
                    ui.label(RichText::new("a estrutura inteira acontece dentro deste capítulo").size(12.0).color(p.faint));
                });
            }
            _ => {}
        }
        if escopo != "capitulo" {
            ui.horizontal(|ui| {
                row_label(ui, "");
                let mut on = a["numerados"].as_bool().unwrap_or(false);
                if kit::toggle(ui, &mut on, p).changed() {
                    a["numerados"] = json!(on);
                    ch = true;
                }
                ui.label(RichText::new("Pular prólogo, interlúdios e epílogo").size(12.5).color(p.text));
            });
            ui.horizontal(|ui| {
                row_label(ui, "Batidas");
                let cur = if a["modo"] == "sequencial" { 1 } else { 0 };
                let items: Vec<(&str, &str)> = es::MODOS.iter().map(|x| ("", x.1)).collect();
                if let Some(k) = kit::segmented(ui, &items, cur, p) {
                    a["modo"] = json!(es::MODOS[k].0);
                    ch = true;
                }
            });
        }
        // what comes out of it
        if let Some(st) = st {
            let list = es::scope_of(a, cols);
            let nb = st.batidas.len();
            let (msg, bad) = if list.is_empty() {
                ("Nenhum capítulo neste trecho.".to_string(), true)
            } else if escopo == "capitulo" {
                (format!("{nb} batidas dentro de \"{}\".", cols[list[0]].title), false)
            } else {
                let (f, l) = (&cols[list[0]], &cols[*list.last().unwrap()]);
                let ends = format!("{} · {} → {} · {}", f.label, f.title, l.label, l.title);
                let (mut msg, mut bad) = if a["modo"] == "sequencial" {
                    if nb > list.len() {
                        (format!("{nb} batidas e {} capítulos: {} ficam sem capítulo. {ends}", list.len(), nb - list.len()), true)
                    } else {
                        (format!("{nb} batidas em {} capítulos{}: {ends}", list.len(), if nb < list.len() { format!(" ({} sobram no fim)", list.len() - nb) } else { String::new() }), false)
                    }
                } else {
                    (format!("{nb} batidas espalhadas por {} {}: {ends}", list.len(), if list.len() == 1 { "capítulo" } else { "capítulos" }), false)
                };
                if let Some(c) = st.capitulos.filter(|c| *c as usize != list.len()) {
                    msg.push_str(&format!(" · esta estrutura foi pensada para {c} capítulos"));
                    bad = true;
                }
                (msg, bad)
            };
            ui.add_space(4.0);
            ui.label(RichText::new(msg).size(12.0).color(if bad { p.error } else { p.faint }));
        }
        ch
    }

    // ── the structure editor ──────────────────────────────────────────────

    pub(super) fn structure_editor(&mut self, ctx: &egui::Context, p: Palette, ix: &Index, root: &Path, author: &str, acts: &mut Vec<Action>) {
        let Some(ed) = self.st_editor.as_mut() else { return };
        let mut close = false;
        let mut save = false;
        let computed = es::parse(&es::to_md(&ed.m), "");
        kit::scrim(ctx, p);
        egui::Window::new("estrutura-editor").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 40.0]).show(ctx, |ui| {
            ui.set_width(660.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(if ed.r.is_some() { "Editar estrutura" } else { "Nova estrutura" }).font(kit::medium(16.0)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::icon_button(ui, icons::X, "Fechar sem salvar", false, p).clicked() {
                        close = true;
                    }
                });
            });
            if ed.pronta {
                ui.label(RichText::new("Esta é uma estrutura pronta. Ao salvar, ela vira uma cópia sua, e a original continua disponível.").size(12.0).color(p.muted));
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(ctx.screen_rect().height() - 330.0).show(ui, |ui| {
                ui.set_width(640.0);
                let m = &mut ed.m;
                ui.add(egui::TextEdit::singleline(&mut m.nome).font(kit::bold(19.0)).frame(false).hint_text("Nome da estrutura").desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Duração").size(12.5).color(p.muted));
                    let mut has = m.capitulos.is_some();
                    if kit::toggle(ui, &mut has, p).changed() {
                        m.capitulos = if has { Some(m.batidas.len().max(1) as i64) } else { None };
                    }
                    if let Some(c) = m.capitulos.as_mut() {
                        ui.add(egui::DragValue::new(c).range(1..=400).suffix(" capítulos"));
                    } else {
                        ui.label(RichText::new("se estica ao trecho escolhido").size(12.0).color(p.faint));
                    }
                });
                ui.add(egui::TextEdit::multiline(&mut m.desc).desired_rows(2).desired_width(f32::INFINITY).hint_text("A ideia da estrutura (opcional)").margin(egui::Margin::symmetric(8, 5)));
                ui.add_space(10.0);
                ui.label(RichText::new("PERGUNTAS DA FICHA DE CADA CENA").font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
                ui.horizontal_wrapped(|ui| {
                    let mut drop = None;
                    for (k, c) in m.campos.iter().enumerate() {
                        egui::Frame::new().fill(p.bg_secondary).corner_radius(13.0).inner_margin(egui::Margin { left: 9, right: 4, top: 3, bottom: 3 }).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(c).size(12.5));
                                if kit::icon_button_sized(ui, icons::X, "", false, p, 18.0, 11.0).clicked() {
                                    drop = Some(k);
                                }
                            });
                        });
                    }
                    if let Some(k) = drop {
                        m.campos.remove(k);
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut ed.new_campo).hint_text("+ pergunta (Enter)").desired_width(170.0).margin(egui::Margin::symmetric(8, 4)));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ed.new_campo.trim().is_empty() {
                        m.campos.push(ed.new_campo.trim().to_string());
                        ed.new_campo.clear();
                        r.request_focus();
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("BATIDAS · {}", m.batidas.len())).font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let more = kit::icon_button_sized(ui, icons::ELLIPSIS, "Mais", false, p, 24.0, 14.0);
                        more.context_menu(|_| {});
                        let popup = ui.make_persistent_id("ste-more");
                        if more.clicked() {
                            ui.memory_mut(|mm| mm.toggle_popup(popup));
                        }
                        egui::popup_below_widget(ui, popup, &more, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            let order = es::ordered(m);
                            if kit::menu_item(ui, icons::LIST_ORDERED, "Numerar as batidas em ordem", p) {
                                for (k, &i) in order.iter().enumerate() {
                                    m.batidas[i].num = Some(k as i64 + 1);
                                }
                            }
                            if kit::menu_item(ui, icons::LIST, "Tirar os números", p) {
                                m.batidas.iter_mut().for_each(|b| b.num = None);
                            }
                            if kit::menu_item(ui, icons::SLIDERS_HORIZONTAL, "Distribuir as porcentagens por igual", p) {
                                m.batidas.iter_mut().for_each(|b| b.auto = true);
                                m.grupos.iter_mut().for_each(|g| g.auto = true);
                            }
                            if kit::menu_item(ui, icons::ARROW_RIGHT_LEFT, "Espelhar (1 ↔ último, 2 ↔ penúltimo…)", p) {
                                let ids: Vec<String> = order.iter().map(|&i| m.batidas[i].id.clone()).collect();
                                for (k, &i) in order.iter().enumerate() {
                                    let t = &ids[ids.len() - 1 - k];
                                    m.batidas[i].espelho = if *t != m.batidas[i].id { Some(t.clone()) } else { None };
                                }
                            }
                        });
                        let r = ui.add(egui::TextEdit::singleline(&mut ed.new_group).hint_text("+ grupo (Enter)").desired_width(130.0).margin(egui::Margin::symmetric(8, 4)));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let g = ed.new_group.trim().to_string();
                            if !g.is_empty() && !m.grupos.iter().any(|x| x.nome == g) {
                                m.grupos.push(Group { nome: g, auto: true, ..Default::default() });
                            }
                            ed.new_group.clear();
                        }
                        if kit::quiet(ui, Some(icons::PLUS), "Batida", p).clicked() {
                            let num = m.batidas.iter().filter_map(|b| b.num).max().map(|n| n + 1);
                            let grupo = m.grupos.last().map(|g| g.nome.clone()).unwrap_or_default();
                            m.batidas.push(Beat { id: en::uid("nb"), num, auto: true, grupo, ..Default::default() });
                        }
                    });
                });
                ui.add_space(4.0);
                // groups and their beats
                let groups: Vec<Option<String>> = std::iter::once(None).chain(m.grupos.iter().map(|g| Some(g.nome.clone()))).collect();
                let mut ops: Vec<(String, &'static str)> = Vec::new();
                for g in groups {
                    let idxs: Vec<usize> = match &g {
                        None => (0..m.batidas.len()).filter(|&i| m.batidas[i].grupo.is_empty() || !m.grupos.iter().any(|x| x.nome == m.batidas[i].grupo)).collect(),
                        Some(name) => (0..m.batidas.len()).filter(|&i| &m.batidas[i].grupo == name).collect(),
                    };
                    if g.is_none() && idxs.is_empty() {
                        continue;
                    }
                    if let Some(name) = &g {
                        let gi = m.grupos.iter().position(|x| &x.nome == name).unwrap();
                        let auto = computed.grupos.iter().find(|x| &x.nome == name).cloned();
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let mut nm = m.grupos[gi].nome.clone();
                            let r = ui.add(egui::TextEdit::singleline(&mut nm).font(kit::medium(13.5)).desired_width(220.0).margin(egui::Margin::symmetric(6, 3)));
                            if r.changed() && !nm.trim().is_empty() && !m.grupos.iter().any(|x| x.nome == nm) {
                                let old = m.grupos[gi].nome.clone();
                                for b in m.batidas.iter_mut().filter(|b| b.grupo == old) {
                                    b.grupo = nm.clone();
                                }
                                m.grupos[gi].nome = nm;
                            }
                            let g = &mut m.grupos[gi];
                            pct_inputs(ui, p, &mut g.auto, &mut g.de, &mut g.ate, auto.as_ref().map(|x| (x.de, x.ate)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if kit::icon_button_sized(ui, icons::X, "Apagar grupo (as batidas ficam, sem grupo)", false, p, 22.0, 12.0).clicked() {
                                    ops.push((name.clone(), "del-group"));
                                }
                                if gi > 0 && kit::icon_button_sized(ui, icons::ARROW_UP, "Subir grupo", false, p, 22.0, 12.0).clicked() {
                                    ops.push((name.clone(), "up-group"));
                                }
                            });
                        });
                    }
                    let order = es::ordered(m);
                    for (k, &i) in idxs.iter().enumerate() {
                        let auto = order.iter().position(|&x| x == i).and_then(|pos| computed.batidas.get(pos)).map(|b| (b.de, b.ate));
                        let id = m.batidas[i].id.clone();
                        let open = ed.open.contains(&id);
                        egui::Frame::new().fill(if open { p.bg_secondary } else { Color32::TRANSPARENT }).corner_radius(8.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let b = &mut m.batidas[i];
                                let mut num = b.num.map(|n| n.to_string()).unwrap_or_default();
                                if ui.add(egui::TextEdit::singleline(&mut num).hint_text("·").desired_width(28.0).margin(egui::Margin::symmetric(4, 3))).changed() {
                                    b.num = num.trim().parse().ok();
                                }
                                ui.add(egui::TextEdit::singleline(&mut b.nome).hint_text("Nome da batida").desired_width(250.0).margin(egui::Margin::symmetric(6, 3)));
                                pct_inputs(ui, p, &mut b.auto, &mut b.de, &mut b.ate, auto);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    if kit::icon_button_sized(ui, icons::X, "Apagar batida", false, p, 22.0, 12.0).clicked() {
                                        ops.push((id.clone(), "del"));
                                    }
                                    if k + 1 < idxs.len() && kit::icon_button_sized(ui, icons::ARROW_DOWN, "Descer", false, p, 22.0, 12.0).clicked() {
                                        ops.push((id.clone(), "down"));
                                    }
                                    if k > 0 && kit::icon_button_sized(ui, icons::ARROW_UP, "Subir", false, p, 22.0, 12.0).clicked() {
                                        ops.push((id.clone(), "up"));
                                    }
                                    if kit::icon_button_sized(ui, if open { icons::CHEVRON_UP } else { icons::CHEVRON_DOWN }, "Detalhes: função, espelho, grupo, descrição", open, p, 22.0, 12.0).clicked() {
                                        ops.push((id.clone(), "toggle"));
                                    }
                                });
                            });
                            if !open {
                                if let Some(f) = m.batidas[i].info("Função") {
                                    ui.label(RichText::new(f).size(11.5).color(p.faint));
                                }
                                return;
                            }
                            let others: Vec<(String, String)> = es::ordered(m).into_iter().filter(|&j| j != i).map(|j| (m.batidas[j].id.clone(), beat_label(&m.batidas[j]))).collect();
                            let gnames: Vec<String> = m.grupos.iter().map(|g| g.nome.clone()).collect();
                            let b = &mut m.batidas[i];
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Grupo").size(12.0).color(p.muted));
                                let mut items = vec![("(nenhum)".to_string(), b.grupo.is_empty())];
                                items.extend(gnames.iter().map(|g| (g.clone(), *g == b.grupo)));
                                if let Some(k) = pick(ui, p, &format!("ste-g-{id}"), if b.grupo.is_empty() { "(nenhum)" } else { &b.grupo }, &items) {
                                    b.grupo = if k == 0 { String::new() } else { gnames[k - 1].clone() };
                                }
                                ui.label(RichText::new("Espelho").size(12.0).color(p.muted));
                                let cur = b.espelho.clone().unwrap_or_default();
                                let mut items = vec![("(nenhum)".to_string(), cur.is_empty())];
                                items.extend(others.iter().map(|o| (o.1.clone(), o.0 == cur)));
                                let lab = others.iter().find(|o| o.0 == cur).map(|o| o.1.clone()).unwrap_or_else(|| "(nenhum)".into());
                                if let Some(k) = pick(ui, p, &format!("ste-m-{id}"), &lab, &items) {
                                    b.espelho = if k == 0 { None } else { Some(others[k - 1].0.clone()) };
                                }
                            });
                            ui.label(RichText::new("Informações do cartão — uma por linha, \"Chave: valor\"").size(12.0).color(p.muted));
                            let mut info = b.info.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n");
                            if ui.add(egui::TextEdit::multiline(&mut info).desired_rows(2).desired_width(f32::INFINITY).hint_text("Função: o que esta batida faz na história").margin(egui::Margin::symmetric(8, 5))).changed() {
                                b.info = info.lines().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).filter(|(k, v)| !k.is_empty() && !v.is_empty() && !k.eq_ignore_ascii_case("espelho")).collect();
                            }
                            ui.add(egui::TextEdit::multiline(&mut b.desc).desired_rows(2).desired_width(f32::INFINITY).hint_text("Descrição (opcional)").margin(egui::Margin::symmetric(8, 5)));
                        });
                    }
                }
                for (id, op) in ops {
                    match op {
                        "toggle" => {
                            if !ed.open.remove(&id) {
                                ed.open.insert(id);
                            }
                        }
                        "del" => {
                            m.batidas.retain(|b| b.id != id);
                            for b in m.batidas.iter_mut() {
                                if b.espelho.as_deref() == Some(id.as_str()) {
                                    b.espelho = None;
                                }
                            }
                        }
                        "up" | "down" => {
                            let Some(a) = m.batidas.iter().position(|b| b.id == id) else { continue };
                            let g = m.batidas[a].grupo.clone();
                            let sibs: Vec<usize> = (0..m.batidas.len()).filter(|&j| m.batidas[j].grupo == g).collect();
                            let k = sibs.iter().position(|&j| j == a).unwrap();
                            let t = if op == "up" { k.checked_sub(1) } else { Some(k + 1).filter(|&x| x < sibs.len()) };
                            if let Some(t) = t {
                                m.batidas.swap(a, sibs[t]);
                            }
                        }
                        "del-group" => {
                            m.grupos.retain(|g| g.nome != id);
                            for b in m.batidas.iter_mut().filter(|b| b.grupo == id) {
                                b.grupo.clear();
                            }
                        }
                        "up-group" => {
                            if let Some(gi) = m.grupos.iter().position(|g| g.nome == id).filter(|&g| g > 0) {
                                m.grupos.swap(gi, gi - 1);
                            }
                        }
                        _ => {}
                    }
                }
                if m.batidas.is_empty() {
                    ui.label(RichText::new("Nenhuma batida. Comece com \"+ Batida\".").color(p.faint));
                }
            });
            if let Some(e) = &ed.error {
                ui.add_space(6.0);
                ui.label(RichText::new(e).color(p.error));
            }
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary_icon(ui, icons::CHECK, if ed.pronta { "Salvar como minha" } else { "Salvar" }, p).clicked() {
                    save = true;
                }
                if kit::button(ui, None, "Cancelar", p).clicked() {
                    close = true;
                }
            });
        });
        if save {
            match self.save_structure(root, ix, author) {
                Ok(path) => {
                    acts.push(Action::Saved(path));
                    self.st_editor = None;
                }
                Err(e) => {
                    if let Some(ed) = self.st_editor.as_mut() {
                        ed.error = Some(e);
                    }
                }
            }
        } else if close {
            self.st_editor = None;
        }
    }

    /// Write the edited structure; books that use it keep their scenes tied
    /// to the same beats even when the beats' ids change.
    fn save_structure(&mut self, root: &Path, ix: &Index, author: &str) -> Result<String, String> {
        let (pronta, old, m) = {
            let ed = self.st_editor.as_ref().unwrap();
            (ed.pronta, ed.r.clone(), ed.m.clone())
        };
        let m = &m;
        if m.nome.trim().is_empty() {
            return Err("Dê um nome à estrutura.".into());
        }
        let order = es::ordered(m);
        let new_ids = es::stable_ids(m)?;
        let mut st = m.clone();
        st.nome = m.nome.trim().to_string();
        st.batidas = order.iter().map(|&i| {
            let mut b = m.batidas[i].clone();
            if b.nome.trim().is_empty() {
                b.nome = "Batida".into();
            }
            b
        }).collect();
        if pronta {
            if let Some(o) = st.fm.as_object_mut() {
                o.remove("gerador");
            }
        }
        let new_ref = es::write(root, &self.st_folder, if pronta { None } else { old.as_deref() }, &st)?;
        let id_map: HashMap<String, String> = order.iter().zip(&new_ids).filter(|(&i, n)| m.batidas[i].id != **n).map(|(&i, n)| (m.batidas[i].id.clone(), n.clone())).collect();
        let remap = |data: &mut Json, swap_ref: bool| -> bool {
            let Some(old) = old.as_deref() else { return false };
            let mut dirty = false;
            let apps: Vec<String> = data["estruturas"].as_array().into_iter().flatten().filter(|a| a["ref"] == old).filter_map(|a| a["id"].as_str().map(String::from)).collect();
            if apps.is_empty() {
                return false;
            }
            if swap_ref && old != new_ref {
                for a in data["estruturas"].as_array_mut().into_iter().flatten() {
                    if a["ref"] == old {
                        a["ref"] = json!(new_ref);
                        dirty = true;
                    }
                }
            }
            for s in data["cenas"].as_array_mut().into_iter().flatten() {
                if let Some(l) = s["batidas"].as_array_mut() {
                    for k in l.iter_mut() {
                        let Some((ap, bid)) = k.as_str().and_then(|x| x.split_once(':')) else { continue };
                        if let (true, Some(nb)) = (apps.iter().any(|a| a == ap), id_map.get(bid)) {
                            *k = json!(format!("{ap}:{nb}"));
                            dirty = true;
                        }
                    }
                }
            }
            dirty
        };
        // this book (a copy of a built-in one replaces it here only)
        let mut data = std::mem::take(&mut self.data);
        if remap(&mut data, true) {
            self.changed();
        }
        self.data = data;
        // the writer's own structure: every other book that uses it
        if !pronta {
            for b in book::books(ix, author) {
                if b.path == self.book || !root.join(en::path(&b)).exists() {
                    continue;
                }
                let mut d = en::load(root, &b);
                if remap(&mut d, true) {
                    let _ = en::save(root, ix, &b, &d);
                }
            }
        }
        self.st_cache.clear();
        Ok(new_ref.strip_prefix("nota:").unwrap_or(&new_ref).to_string())
    }
}

impl StEditor {
    pub fn open(r: &str, st: Structure) -> StEditor {
        let pronta = !r.is_empty() && !r.starts_with("nota:");
        let mut m = st;
        if pronta {
            m.nome = format!("{} (minha)", m.nome);
        }
        StEditor { r: if r.is_empty() { None } else { Some(r.to_string()) }, pronta, m, open: HashSet::new(), new_campo: String::new(), new_group: String::new(), error: None }
    }
}

/// From–to percentages; empty (auto) shows the computed value faintly.
fn pct_inputs(ui: &mut egui::Ui, p: Palette, auto: &mut bool, de: &mut Option<f64>, ate: &mut Option<f64>, computed: Option<(Option<f64>, Option<f64>)>) {
    let (cd, ca) = computed.unwrap_or((None, None));
    let mut a = if *auto { cd.unwrap_or(0.0) } else { de.unwrap_or(0.0) };
    let mut z = if *auto { ca.or(cd).unwrap_or(0.0) } else { ate.or(*de).unwrap_or(0.0) };
    let tip = if *auto { "Automático (distribuído por igual). Arraste ou digite para fixar." } else { "Porcentagem do trecho" };
    let is_auto = *auto;
    ui.scope(|ui| {
        if is_auto {
            ui.visuals_mut().override_text_color = Some(p.faint);
        }
        let r1 = ui.add(egui::DragValue::new(&mut a).range(0.0..=100.0).speed(0.5).max_decimals(1)).on_hover_text(tip);
        ui.label(RichText::new("–").color(p.faint));
        let r2 = ui.add(egui::DragValue::new(&mut z).range(0.0..=100.0).speed(0.5).max_decimals(1).suffix("%")).on_hover_text(tip);
        if r1.changed() || r2.changed() {
            *auto = false;
            if r1.changed() && z < a {
                z = a;
            }
            if r2.changed() && a > z {
                a = z;
            }
            *de = Some(a);
            *ate = Some(z);
        }
        if !*auto && kit::icon_button_sized(ui, icons::ROTATE_CCW, "Voltar ao automático", false, p, 20.0, 11.0).clicked() {
            *auto = true;
            *de = None;
            *ate = None;
        }
    });
}

/// A chip that opens a list (same as the drawer's).
fn pick(ui: &mut egui::Ui, p: Palette, id: &str, text: &str, items: &[(String, bool)]) -> Option<usize> {
    let r = kit::chip(ui, None, text, false, p);
    let popup = ui.make_persistent_id(("st-pick", id));
    if r.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    let mut out = None;
    egui::popup_below_widget(ui, popup, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(240.0);
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

