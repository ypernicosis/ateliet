//! Enredo: the Mural (ideas and research: notes, images, links,
//! loose scenes) and Relações (characters, events and notes tied by
//! threads). Cards on an infinite surface, joined by labelled threads; the
//! data is the plugin's (`mural` / `relacoes` in Enredo.md).

use super::enredo_ui::{note_name, status_color, EnredoView, Sel};
use super::library::Cx;
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::scriptorium::book::{self, Book};
use ateliet_core::scriptorium::enredo::{self as en, str_of};
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{json, Value as Json};
use std::collections::HashMap;

const CARD_COLORS: [(&str, &str); 6] = [("Ouro", "#c9a45c"), ("Petróleo", "#2f6b6b"), ("Vinho", "#7a2e3b"), ("Índigo", "#4b5bb3"), ("Musgo", "#5d7a3a"), ("Cinza", "#6b7280")];
const LINK_COLORS: [(&str, &str); 4] = [("Vermelho (conflito)", "#c0392b"), ("Verde (aliança)", "#3f9f6f"), ("Ouro", "#c9a45c"), ("Azul", "#4f8fbf")];

enum Grab {
    Pan(Vec2),
    Card(String, Vec2),
    Link(String),
}

/// A card being edited in the dialog.
pub struct CardEdit {
    id: String,
    titulo: String,
    texto: String,
    data: String,
    url: String,
    link_label: Option<String>,
}

#[derive(Default)]
pub struct Board {
    sel: Option<String>,
    sel_link: Option<String>,
    grab: Option<Grab>,
    locked: bool,
    edit: Option<CardEdit>,
    /// Adding a character, scene or image: the list to pick from.
    adding: Option<(u8, String)>,
    fitted: Option<String>,
}

fn bezier(a: Pos2, b: Pos2) -> [Pos2; 4] {
    let dx = b.x - a.x;
    [a, pos2(a.x + dx * 0.4, a.y), pos2(b.x - dx * 0.4, b.y), b]
}

fn bez_point(c: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let v = c[0].to_vec2() * (u * u * u) + c[1].to_vec2() * (3.0 * u * u * t) + c[2].to_vec2() * (3.0 * u * t * t) + c[3].to_vec2() * (t * t * t);
    v.to_pos2()
}

impl EnredoView {
    fn board_data(&mut self, which: &str) -> &mut Json {
        let d = &mut self.data[which];
        if !d.is_object() {
            *d = json!({ "cards": [], "links": [], "view": null });
        }
        for k in ["cards", "links"] {
            if !d[k].is_array() {
                d[k] = json!([]);
            }
        }
        d
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn board(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, b: &Book, which: &str, chars: &[String], acts: &mut Vec<Action>) {
        let d = self.board_data(which).clone();
        let mut bs = std::mem::take(&mut self.board_state);
        let canvas = ui.available_rect_before_wrap();
        let (_, resp) = ui.allocate_exact_size(canvas.size(), Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 0.0, p.bg_secondary);
        // view: screen = canvas.min + (x, y) + world * z
        let mut view = (d["view"]["x"].as_f64().unwrap_or(0.0) as f32, d["view"]["y"].as_f64().unwrap_or(0.0) as f32, d["view"]["z"].as_f64().unwrap_or(1.0) as f32);
        let cards: Vec<Json> = d["cards"].as_array().cloned().unwrap_or_default();
        let links: Vec<Json> = d["links"].as_array().cloned().unwrap_or_default();
        // dotted background that moves with the view
        let step = 24.0 * view.2;
        if step > 8.0 {
            let ox = (view.0).rem_euclid(step);
            let oy = (view.1).rem_euclid(step);
            let mut y = canvas.top() + oy;
            while y < canvas.bottom() {
                let mut x = canvas.left() + ox;
                while x < canvas.right() {
                    painter.circle_filled(pos2(x, y), 0.9, p.border_strong.gamma_multiply(0.6));
                    x += step;
                }
                y += step;
            }
        }
        // the first time, frame everything
        if bs.fitted.as_deref() != Some(which) {
            bs.fitted = Some(which.to_string());
            if d["view"].is_null() && !cards.is_empty() {
                view = fit(&cards, canvas.size());
            }
        }
        let to_screen = |w: Pos2, v: (f32, f32, f32)| canvas.min + vec2(v.0, v.1) + w.to_vec2() * v.2;
        let to_world = |s: Pos2, v: (f32, f32, f32)| ((s - canvas.min - vec2(v.0, v.1)) / v.2).to_pos2();
        let z = view.2;
        let book_chars: HashMap<String, Json> = chars.iter().filter_map(|c| cx.vault.index.read().ok().and_then(|ix| ix.page(c).map(|pg| (c.clone(), pg.yaml.clone())))).collect();
        // lay the cards out (their height depends on their text)
        let mut rects: HashMap<String, Rect> = HashMap::new();
        struct Paint {
            id: String,
            rect: Rect,
            color: Option<Color32>,
            kind: String,
            lines: Vec<(String, egui::FontId, Color32)>,
            tag: Option<(String, Color32)>,
            image: Option<String>,
            initials: Option<String>,
            icon: Option<&'static str>,
        }
        let mut paints: Vec<Paint> = Vec::new();
        for c in &cards {
            let id = str_of(c, "id").to_string();
            let w = c["w"].as_f64().unwrap_or(210.0) as f32;
            let kind = str_of(c, "t").to_string();
            let link = str_of(c, "link").to_string();
            let fm = book_chars.get(&link).cloned().or_else(|| if link.is_empty() { None } else { cx.vault.index.read().ok().and_then(|ix| ix.page(&link).map(|pg| pg.yaml.clone())) }).unwrap_or(Json::Null);
            let color = c["cor"].as_str().or(fm["cor"].as_str()).filter(|s| !s.is_empty()).map(super::color);
            let mut lines: Vec<(String, egui::FontId, Color32)> = Vec::new();
            let mut tag = None;
            let mut image = None;
            let mut initials = None;
            let mut icon = None;
            let texto = str_of(c, "texto").to_string();
            match kind.as_str() {
                "personagem" => {
                    let name = if link.is_empty() { "(nota não encontrada)".into() } else { note_name(&link) };
                    initials = Some(ateliet_core::scriptorium::sheet::initials(&name));
                    image = Some(ateliet_core::scriptorium::link_text(&fm["retrato"])).filter(|s| !s.is_empty());
                    lines.push((name, kit::medium(13.5), p.text));
                    let sub = fm["frase"].as_str().filter(|s| !s.is_empty()).map(|f| format!("“{f}”")).or(fm["papel"].as_str().map(String::from)).unwrap_or_default();
                    if !sub.is_empty() {
                        lines.push((sub, egui::FontId::proportional(12.0), p.muted));
                    }
                    if !texto.is_empty() {
                        lines.push((texto, egui::FontId::proportional(12.0), p.muted));
                    }
                    tag = Some((format!("● {}", fm["papel"].as_str().filter(|s| !s.is_empty()).unwrap_or("personagem")), p.faint));
                }
                "cena" => {
                    icon = Some(icons::CLAPPERBOARD);
                    match en::scene_index(&self.data, str_of(c, "ref")) {
                        Some(i) => {
                            let s = &self.data["cenas"][i];
                            let t = str_of(s, "titulo");
                            lines.push((if t.is_empty() { "Cena sem título".into() } else { t.into() }, kit::medium(13.0), p.text));
                            if !str_of(s, "resumo").is_empty() {
                                lines.push((str_of(s, "resumo").into(), egui::FontId::proportional(12.0), p.muted));
                            }
                            let st = str_of(s, "status");
                            tag = Some((format!("● {}", book::status_label(if st.is_empty() { "ideia" } else { st })), status_color(st)));
                        }
                        None => lines.push(("(cena removida)".into(), kit::medium(13.0), p.faint)),
                    }
                }
                "capitulo" => {
                    icon = Some(icons::BOOK);
                    lines.push((if link.is_empty() { "(capítulo)".into() } else { fm["titulo"].as_str().map(String::from).unwrap_or_else(|| note_name(&link)) }, kit::medium(13.0), p.text));
                    if !texto.is_empty() {
                        lines.push((texto, egui::FontId::proportional(12.0), p.muted));
                    }
                }
                "imagem" => {
                    image = Some(link.clone()).filter(|s| !s.is_empty());
                    if !texto.is_empty() {
                        lines.push((texto, egui::FontId::proportional(12.0), p.muted));
                    }
                }
                "link" => {
                    let url = str_of(c, "url");
                    icon = Some(if url.starts_with("[[") { icons::FILE_TEXT } else { icons::LINK });
                    lines.push((c["titulo"].as_str().filter(|s| !s.is_empty()).unwrap_or(url).to_string(), kit::medium(13.0), p.text));
                    if !texto.is_empty() {
                        lines.push((texto, egui::FontId::proportional(12.0), p.muted));
                    }
                    tag = Some((url.trim_start_matches("https://").trim_start_matches("http://").chars().take(40).collect(), p.faint));
                }
                "evento" => {
                    icon = Some(icons::CALENDAR_CLOCK);
                    if !str_of(c, "data").is_empty() {
                        lines.push((str_of(c, "data").into(), kit::medium(11.0), p.accent_strong));
                    }
                    lines.push((c["titulo"].as_str().filter(|s| !s.is_empty()).unwrap_or("Evento").into(), kit::medium(13.0), p.text));
                    if !texto.is_empty() {
                        lines.push((texto, egui::FontId::proportional(12.0), p.muted));
                    }
                }
                _ => {
                    lines.push((if texto.is_empty() { "Nota vazia — duplo clique para escrever".into() } else { texto }, egui::FontId::proportional(13.0), if str_of(c, "texto").is_empty() { p.faint } else { p.text }));
                }
            }
            // height in world units
            let inner = w - 24.0;
            let mut h = 20.0;
            if kind == "personagem" {
                h += 56.0;
            }
            if kind == "imagem" {
                h += w * 0.62;
            }
            for (t, f, _) in &lines {
                let g = ui.fonts(|fo| fo.layout(t.clone(), f.clone(), Color32::WHITE, inner));
                h += g.size().y.min(160.0) + 4.0;
            }
            if tag.is_some() {
                h += 18.0;
            }
            let wr = Rect::from_min_size(pos2(c["x"].as_f64().unwrap_or(0.0) as f32, c["y"].as_f64().unwrap_or(0.0) as f32), vec2(w, h.max(46.0)));
            rects.insert(id.clone(), wr);
            paints.push(Paint { id, rect: wr, color, kind, lines, tag, image, initials, icon });
        }
        // input: what was grabbed
        let pointer = ui.ctx().pointer_latest_pos();
        let hit_card = |pos: Pos2, v: (f32, f32, f32)| -> Option<String> {
            let w = to_world(pos, v);
            paints.iter().rev().find(|c| c.rect.contains(w)).map(|c| c.id.clone())
        };
        let hit_port = |pos: Pos2, v: (f32, f32, f32)| -> Option<String> {
            paints.iter().rev().find(|c| to_screen(pos2(c.rect.right(), c.rect.center().y), v).distance(pos) < 9.0).map(|c| c.id.clone())
        };
        let hit_link = |pos: Pos2, v: (f32, f32, f32)| -> Option<String> {
            for l in &links {
                let (Some(a), Some(bb)) = (rects.get(str_of(l, "a")), rects.get(str_of(l, "b"))) else { continue };
                let cv = bezier(to_screen(a.center(), v), to_screen(bb.center(), v));
                if (0..=30).any(|i| bez_point(&cv, i as f32 / 30.0).distance(pos) < 6.0) {
                    return Some(str_of(l, "id").to_string());
                }
            }
            None
        };
        let mut save = false;
        let mut cmds: Vec<BoardCmd> = Vec::new();
        if resp.drag_started() {
            // where the button went down, not where the drag was recognised
            if let Some(pos) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()) {
                bs.grab = if let Some(id) = hit_port(pos, view).filter(|_| !bs.locked) {
                    Some(Grab::Link(id))
                } else if let Some(id) = hit_card(pos, view) {
                    bs.sel = Some(id.clone());
                    bs.sel_link = None;
                    let r = rects[&id];
                    if !bs.locked {
                        cmds.push(BoardCmd::Raise(id.clone()));
                    }
                    if bs.locked { None } else { Some(Grab::Card(id, to_world(pos, view) - r.min)) }
                } else {
                    Some(Grab::Pan(vec2(view.0, view.1) - pos.to_vec2()))
                };
            }
        }
        if resp.dragged() {
            if let Some(pos) = pointer {
                match &bs.grab {
                    Some(Grab::Pan(o)) => {
                        view.0 = o.x + pos.x;
                        view.1 = o.y + pos.y;
                    }
                    Some(Grab::Card(id, off)) => {
                        let w = to_world(pos, view) - *off;
                        let snap = |v: f32| (v / 10.0).round() * 10.0;
                        cmds.push(BoardCmd::Move(id.clone(), snap(w.x), snap(w.y)));
                    }
                    _ => {}
                }
            }
        }
        if resp.drag_stopped() {
            if let (Some(Grab::Link(from)), Some(pos)) = (&bs.grab, pointer) {
                if let Some(to) = hit_card(pos, view).filter(|t| t != from) {
                    cmds.push(BoardCmd::Link(from.clone(), to));
                }
            }
            if matches!(bs.grab, Some(Grab::Card(..)) | Some(Grab::Pan(_))) {
                save = true;
            }
            bs.grab = None;
        }
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if let Some(l) = hit_link(pos, view) {
                    bs.sel_link = Some(l);
                    bs.sel = None;
                } else {
                    bs.sel = hit_card(pos, view);
                    bs.sel_link = None;
                }
            }
        }
        if resp.double_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                match hit_card(pos, view) {
                    Some(id) => {
                        let c = cards.iter().find(|c| c["id"] == id.as_str()).cloned().unwrap_or(Json::Null);
                        match str_of(&c, "t") {
                            "cena" => cmds.push(BoardCmd::OpenScene(str_of(&c, "ref").into())),
                            "link" => cmds.push(BoardCmd::OpenUrl(str_of(&c, "url").into())),
                            "personagem" | "capitulo" => acts.push(Action::Open(str_of(&c, "link").into(), true)),
                            _ => bs.edit = Some(card_edit(&c)),
                        }
                    }
                    None if !bs.locked => {
                        let w = to_world(pos, view);
                        let id = en::uid("c");
                        cmds.push(BoardCmd::Add(json!({ "id": id, "t": "nota", "texto": "", "cor": "#c9a45c", "x": (w.x - 100.0).round(), "y": (w.y - 30.0).round(), "w": 210 })));
                        bs.edit = Some(CardEdit { id, titulo: String::new(), texto: String::new(), data: String::new(), url: String::new(), link_label: None });
                    }
                    None => {}
                }
            }
        }
        // zoom with ctrl+wheel (or pinch), pan with the wheel
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0 {
                if let Some(pos) = pointer {
                    let nz = (view.2 * zoom).clamp(0.15, 3.0);
                    let rel = pos - canvas.min;
                    view.0 = rel.x - (rel.x - view.0) * nz / view.2;
                    view.1 = rel.y - (rel.y - view.1) * nz / view.2;
                    view.2 = nz;
                    save = true;
                }
            } else if scroll != Vec2::ZERO {
                view.0 += scroll.x;
                view.1 += scroll.y;
            }
        }
        if bs.grab.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if let Some(pos) = pointer.filter(|p| canvas.contains(*p)) {
            if hit_port(pos, view).is_some() && !bs.locked {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            } else if hit_card(pos, view).is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        // right-click menus
        let menu_target = ui.ctx().memory(|m| m.data.get_temp::<(String, bool)>(egui::Id::new(("board-menu", which))));
        if resp.secondary_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = hit_link(pos, view).map(|l| (l, true)).or_else(|| hit_card(pos, view).map(|c| (c, false)));
                ui.ctx().memory_mut(|m| match &t {
                    Some(t) => m.data.insert_temp(egui::Id::new(("board-menu", which)), t.clone()),
                    None => m.data.remove::<(String, bool)>(egui::Id::new(("board-menu", which))),
                });
                if let Some((id, is_link)) = t {
                    if is_link {
                        bs.sel_link = Some(id);
                        bs.sel = None;
                    } else {
                        bs.sel = Some(id);
                        bs.sel_link = None;
                    }
                }
            }
        }
        let menu_target = menu_target.or_else(|| ui.ctx().memory(|m| m.data.get_temp::<(String, bool)>(egui::Id::new(("board-menu", which)))));
        resp.context_menu(|ui| {
            let Some((id, is_link)) = menu_target.clone() else {
                ui.label(RichText::new("Duplo clique cria uma nota").color(p.faint));
                return;
            };
            if is_link {
                let l = links.iter().find(|l| l["id"] == id.as_str()).cloned().unwrap_or(Json::Null);
                if kit::menu_item(ui, icons::TAG, if str_of(&l, "rotulo").is_empty() { "Adicionar rótulo…" } else { "Mudar rótulo…" }, p) {
                    bs.edit = Some(CardEdit { id: id.clone(), titulo: String::new(), texto: String::new(), data: String::new(), url: String::new(), link_label: Some(str_of(&l, "rotulo").into()) });
                }
                if kit::menu_item(ui, icons::MINUS, if l["estilo"] == "tracejado" { "Linha contínua" } else { "Linha tracejada" }, p) {
                    cmds.push(BoardCmd::LinkSet(id.clone(), "estilo", if l["estilo"] == "tracejado" { json!("") } else { json!("tracejado") }));
                }
                if kit::menu_item(ui, icons::ARROW_RIGHT, if l["seta"] == false { "Mostrar seta" } else { "Sem seta" }, p) {
                    cmds.push(BoardCmd::LinkSet(id.clone(), "seta", json!(l["seta"] == false)));
                }
                if kit::menu_item(ui, icons::ARROW_RIGHT_LEFT, "Inverter direção", p) {
                    cmds.push(BoardCmd::Flip(id.clone()));
                }
                kit::menu_sep(ui, p);
                for (n, col) in LINK_COLORS {
                    if kit::menu_check(ui, l["cor"] == col, n, p) {
                        cmds.push(BoardCmd::LinkSet(id.clone(), "cor", json!(col)));
                    }
                }
                if kit::menu_check(ui, l["cor"].as_str().is_none_or(|s| s.is_empty()), "Cor padrão", p) {
                    cmds.push(BoardCmd::LinkSet(id.clone(), "cor", Json::Null));
                }
                kit::menu_sep(ui, p);
                if kit::menu_danger(ui, icons::TRASH_2, "Remover ligação", p) {
                    cmds.push(BoardCmd::Unlink(id.clone()));
                }
                return;
            }
            let c = cards.iter().find(|c| c["id"] == id.as_str()).cloned().unwrap_or(Json::Null);
            let t = str_of(&c, "t");
            if (t == "personagem" || t == "capitulo" || t == "imagem") && kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir", p) {
                acts.push(Action::Open(str_of(&c, "link").into(), true));
            }
            if t == "link" && kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir link", p) {
                cmds.push(BoardCmd::OpenUrl(str_of(&c, "url").into()));
            }
            if t == "cena" && kit::menu_item(ui, icons::PENCIL, "Editar cena", p) {
                cmds.push(BoardCmd::OpenScene(str_of(&c, "ref").into()));
            }
            if t == "nota" && kit::menu_item(ui, icons::CLAPPERBOARD, "Transformar em cena", p) {
                cmds.push(BoardCmd::ToScene(id.clone()));
            }
            if kit::menu_item(ui, icons::PENCIL, "Editar texto", p) {
                bs.edit = Some(card_edit(&c));
            }
            kit::menu_sep(ui, p);
            for (n, col) in CARD_COLORS {
                if kit::menu_check(ui, c["cor"] == col, n, p) {
                    cmds.push(BoardCmd::CardSet(id.clone(), "cor", json!(col)));
                }
            }
            if kit::menu_check(ui, c["cor"].as_str().is_none_or(|s| s.is_empty()), "Sem cor", p) {
                cmds.push(BoardCmd::CardSet(id.clone(), "cor", Json::Null));
            }
            kit::menu_sep(ui, p);
            if kit::menu_item(ui, icons::MOVE, if c["w"].as_f64().unwrap_or(210.0) > 230.0 { "Estreitar" } else { "Alargar" }, p) {
                cmds.push(BoardCmd::CardSet(id.clone(), "w", json!(if c["w"].as_f64().unwrap_or(210.0) > 230.0 { 180 } else { 300 })));
            }
            if kit::menu_item(ui, icons::COPY, "Duplicar", p) {
                let mut dd = c.clone();
                dd["id"] = json!(en::uid("c"));
                dd["x"] = json!(c["x"].as_f64().unwrap_or(0.0) + 30.0);
                dd["y"] = json!(c["y"].as_f64().unwrap_or(0.0) + 30.0);
                cmds.push(BoardCmd::Add(dd));
            }
            kit::menu_sep(ui, p);
            if kit::menu_danger(ui, icons::TRASH_2, "Remover do quadro", p) {
                cmds.push(BoardCmd::Remove(id.clone()));
            }
        });
        // delete key
        if ui.input(|i| i.key_pressed(egui::Key::Delete)) && !ui.ctx().memory(|m| m.focused().is_some()) {
            if let Some(id) = bs.sel.take() {
                cmds.push(BoardCmd::Remove(id));
            } else if let Some(id) = bs.sel_link.take() {
                cmds.push(BoardCmd::Unlink(id));
            }
        }
        // threads
        for l in &links {
            let (Some(a), Some(bb)) = (rects.get(str_of(l, "a")), rects.get(str_of(l, "b"))) else { continue };
            let (sa, sb) = (to_screen(a.center(), view), to_screen(bb.center(), view));
            let sel = bs.sel_link.as_deref() == Some(str_of(l, "id"));
            let col = l["cor"].as_str().filter(|s| !s.is_empty()).map(super::color).unwrap_or(p.border_strong);
            let stroke = Stroke::new(if sel { 2.6 } else { 1.6 } * z.sqrt(), if sel { p.accent_strong } else { col });
            let cv = bezier(sa, sb);
            let pts: Vec<Pos2> = (0..=40).map(|i| bez_point(&cv, i as f32 / 40.0)).collect();
            if l["estilo"] == "tracejado" {
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 7.0 * z, 6.0 * z));
            } else {
                painter.add(egui::Shape::line(pts.clone(), stroke));
            }
            if l["seta"] != false {
                // the arrow sits where the thread enters the target card
                let tr = Rect::from_min_max(to_screen(bb.min, view), to_screen(bb.max, view));
                if let Some(k) = pts.iter().position(|q| tr.contains(*q)) {
                    let tip = pts[k];
                    let dir = (pts[k] - pts[k.saturating_sub(2)]).normalized();
                    let n = vec2(-dir.y, dir.x);
                    let s = 7.0 * z.sqrt();
                    painter.add(egui::Shape::convex_polygon(vec![tip, tip - dir * s * 1.6 + n * s * 0.8, tip - dir * s * 1.6 - n * s * 0.8], stroke.color, Stroke::NONE));
                }
            }
            let label = str_of(l, "rotulo");
            if !label.is_empty() {
                let m = bez_point(&cv, 0.5);
                let g = painter.layout_no_wrap(label.to_string(), egui::FontId::proportional(11.5 * z.sqrt()), p.text);
                let r = Rect::from_center_size(m, g.size() + vec2(10.0, 4.0));
                painter.rect(r, 6.0, p.bg, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                painter.galley(r.min + vec2(5.0, 2.0), g, p.text);
            }
        }
        if let (Some(Grab::Link(from)), Some(pos)) = (&bs.grab, pointer) {
            if let Some(a) = rects.get(from) {
                let cv = bezier(to_screen(a.center(), view), pos);
                let pts: Vec<Pos2> = (0..=30).map(|i| bez_point(&cv, i as f32 / 30.0)).collect();
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.6_f32, p.accent_strong), 6.0, 5.0));
            }
        }
        // cards
        for c in &paints {
            let r = Rect::from_min_max(to_screen(c.rect.min, view), to_screen(c.rect.max, view));
            if !r.intersects(canvas) {
                continue;
            }
            let sel = bs.sel.as_deref() == Some(c.id.as_str());
            let note = c.kind == "nota";
            let fill = match (note, c.color) {
                (true, Some(col)) => blend(p.bg, col, if p.dark { 0.22 } else { 0.16 }),
                _ => p.bg,
            };
            painter.rect_filled(r.translate(vec2(0.0, 2.0)), 10.0 * z, Color32::from_black_alpha(if p.dark { 60 } else { 14 }));
            painter.rect(r, 10.0 * z, fill, Stroke::new(if sel { 2.0_f32 } else { 1.0 }, if sel { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
            if let (false, Some(col)) = (note, c.color) {
                painter.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 4.0 * z)), egui::CornerRadius { nw: (10.0 * z) as u8, ne: (10.0 * z) as u8, sw: 0, se: 0 }, col);
            }
            let pad = 12.0 * z;
            let inner_w = r.width() - 2.0 * pad;
            let mut y = r.top() + 10.0 * z;
            if c.kind == "personagem" {
                let ac = pos2(r.left() + pad + 22.0 * z, y + 24.0 * z);
                let shown = c.image.as_ref().and_then(|im| crate::host::image(ui.ctx(), cx.vault, cx.caches, im, "")).map(|(tid, sz)| {
                    let ar = Rect::from_center_size(ac, vec2(44.0 * z, 44.0 * z));
                    let k = (ar.width() / sz.x).max(ar.height() / sz.y);
                    let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(ar.width() / (sz.x * k), ar.height() / (sz.y * k)));
                    painter.add(egui::epaint::RectShape::filled(ar, 22.0 * z, Color32::WHITE).with_texture(tid, uv));
                });
                if shown.is_none() {
                    painter.circle_filled(ac, 22.0 * z, c.color.unwrap_or(p.bg_active));
                    painter.text(ac, egui::Align2::CENTER_CENTER, c.initials.clone().unwrap_or_default(), kit::medium(14.0 * z), if c.color.is_some() { Color32::WHITE } else { p.muted });
                }
                y += 56.0 * z;
            }
            if c.kind == "imagem" {
                let ir = Rect::from_min_size(pos2(r.left() + pad, y), vec2(inner_w, c.rect.width() * 0.62 * z - 6.0 * z));
                match c.image.as_ref().and_then(|im| crate::host::image(ui.ctx(), cx.vault, cx.caches, im, "")) {
                    Some((tid, sz)) => {
                        let k = (ir.width() / sz.x).max(ir.height() / sz.y);
                        let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(ir.width() / (sz.x * k), ir.height() / (sz.y * k)));
                        painter.add(egui::epaint::RectShape::filled(ir, 6.0 * z, Color32::WHITE).with_texture(tid, uv));
                    }
                    None => {
                        painter.rect_filled(ir, 6.0 * z, p.bg_active);
                    }
                }
                y += c.rect.width() * 0.62 * z;
            }
            for (k, (t, f, col)) in c.lines.iter().enumerate() {
                let font = egui::FontId::new(f.size * z, f.family.clone());
                let mut job = egui::text::LayoutJob::single_section(t.clone(), egui::TextFormat { font_id: font, color: *col, ..Default::default() });
                job.wrap = egui::text::TextWrapping { max_width: inner_w - if k == 0 && c.icon.is_some() { 20.0 * z } else { 0.0 }, max_rows: 8, break_anywhere: false, overflow_character: Some('…') };
                let g = ui.fonts(|fo| fo.layout_job(job));
                let mut x = r.left() + pad;
                if k == 0 {
                    if let Some(ic) = c.icon {
                        painter.text(pos2(x + 7.0 * z, y + g.size().y / 2.0), egui::Align2::CENTER_CENTER, ic, kit::icon_font(13.0 * z), p.muted);
                        x += 20.0 * z;
                    }
                }
                let h = g.size().y;
                painter.galley(pos2(x, y), g, *col);
                y += h + 4.0 * z;
            }
            if let Some((t, col)) = &c.tag {
                painter.text(pos2(r.left() + pad, y + 6.0 * z), egui::Align2::LEFT_CENTER, t, egui::FontId::proportional(11.0 * z), *col);
            }
            // the port: drag from it to another card to tie them
            let port = pos2(r.right(), r.center().y);
            let hov = pointer.is_some_and(|q| q.distance(port) < 9.0);
            if (sel || hov) && !bs.locked {
                painter.circle(port, if hov { 6.0 } else { 4.5 }, p.bg, Stroke::new(1.5_f32, p.accent_strong));
            }
        }
        // apply what was asked
        for cmd in cmds {
            self.board_cmd(which, cmd, &mut bs, acts, b);
            save = true;
        }
        // keep the view
        let dv = &mut self.board_data(which)["view"];
        let nv = json!({ "x": view.0, "y": view.1, "z": view.2 });
        if *dv != nv {
            *dv = nv;
            if save || bs.grab.is_none() {
                save = true;
            }
        }
        if save {
            self.changed();
        }
        // toolbar
        let mural = which == "mural";
        egui::Area::new(egui::Id::new(("board-bar", which))).order(egui::Order::Middle).fixed_pos(canvas.left_top() + vec2(16.0, 14.0)).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(4, 3)).corner_radius(9.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let center = to_world(canvas.center(), view) - vec2(105.0, 40.0);
                    let add_at = |t: Json| -> Json {
                        let mut t = t;
                        t["id"] = json!(en::uid("c"));
                        t["x"] = json!(center.x.round());
                        t["y"] = json!(center.y.round());
                        if t["w"].is_null() {
                            t["w"] = json!(210);
                        }
                        t
                    };
                    let mut new: Option<Json> = None;
                    if mural {
                        if kit::icon_button(ui, icons::STICKY_NOTE, "Nota", false, p).clicked() {
                            new = Some(add_at(json!({ "t": "nota", "texto": "", "cor": "#c9a45c" })));
                        }
                        if kit::icon_button(ui, icons::IMAGE, "Imagem", false, p).clicked() {
                            bs.adding = Some((3, String::new()));
                        }
                        if kit::icon_button(ui, icons::LINK, "Link / referência", false, p).clicked() {
                            let c = add_at(json!({ "t": "link", "url": "", "titulo": "", "texto": "", "w": 230 }));
                            bs.edit = Some(card_edit(&c));
                            new = Some(c);
                        }
                        if kit::icon_button(ui, icons::CLAPPERBOARD, "Cena do enredo", false, p).clicked() {
                            bs.adding = Some((2, String::new()));
                        }
                    } else {
                        if kit::icon_button(ui, icons::USER_PLUS, "Adicionar personagem", false, p).clicked() {
                            bs.adding = Some((1, String::new()));
                        }
                        if kit::icon_button(ui, icons::STICKY_NOTE, "Nova nota", false, p).clicked() {
                            new = Some(add_at(json!({ "t": "nota", "texto": "Nova nota", "cor": "#c9a45c" })));
                        }
                        if kit::icon_button(ui, icons::CALENDAR_CLOCK, "Novo evento", false, p).clicked() {
                            let c = add_at(json!({ "t": "evento", "titulo": "Evento", "data": "", "texto": "" }));
                            bs.edit = Some(card_edit(&c));
                            new = Some(c);
                        }
                        if kit::icon_button(ui, icons::IMAGE, "Imagem", false, p).clicked() {
                            bs.adding = Some((3, String::new()));
                        }
                    }
                    if let Some(c) = new {
                        bs.sel = c["id"].as_str().map(String::from);
                        self.board_data(which)["cards"].as_array_mut().unwrap().push(c);
                        self.changed();
                    }
                    ui.add_space(4.0);
                    if kit::icon_button(ui, icons::ZOOM_OUT, "Afastar", false, p).clicked() {
                        self.board_zoom(which, 1.0 / 1.2, canvas);
                    }
                    if kit::icon_button(ui, icons::ZOOM_IN, "Aproximar", false, p).clicked() {
                        self.board_zoom(which, 1.2, canvas);
                    }
                    if kit::icon_button(ui, icons::MAXIMIZE, "Enquadrar tudo", false, p).clicked() {
                        let cs = self.board_data(which)["cards"].as_array().cloned().unwrap_or_default();
                        if !cs.is_empty() {
                            let v = fit(&cs, canvas.size());
                            self.board_data(which)["view"] = json!({ "x": v.0, "y": v.1, "z": v.2 });
                            self.changed();
                        }
                    }
                    if kit::icon_button(ui, icons::ORBIT, "Organizar em círculo", false, p).clicked() {
                        let d = self.board_data(which);
                        let n = d["cards"].as_array().map(|a| a.len()).unwrap_or(0);
                        let rr = (n as f32 * 48.0).max(260.0);
                        for (i, c) in d["cards"].as_array_mut().into_iter().flatten().enumerate() {
                            let a = i as f32 / n as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                            c["x"] = json!((a.cos() * rr).round());
                            c["y"] = json!((a.sin() * rr * 0.7).round());
                        }
                        let cs = d["cards"].as_array().cloned().unwrap_or_default();
                        let v = fit(&cs, canvas.size());
                        d["view"] = json!({ "x": v.0, "y": v.1, "z": v.2 });
                        self.changed();
                    }
                    if kit::icon_button(ui, if bs.locked { icons::LOCK } else { icons::LOCK_OPEN }, "Travar edição", bs.locked, p).clicked() {
                        bs.locked = !bs.locked;
                    }
                });
            });
        });
        if mural && cards.is_empty() {
            painter.text(canvas.center(), egui::Align2::CENTER_CENTER, "Mural de ideias e pesquisa: notas, imagens, links e cenas soltas.\nDuplo clique cria uma nota; uma nota pode virar cena.", egui::FontId::proportional(13.0), p.faint);
        }
        if !mural && cards.is_empty() {
            painter.text(canvas.center(), egui::Align2::CENTER_CENTER, "Personagens, eventos e notas ligados por fios.\nArraste do ponto à direita de um cartão até outro para ligá-los.", egui::FontId::proportional(13.0), p.faint);
        }
        self.board_dialogs(ui.ctx(), p, which, &mut bs, chars, canvas, view, cx);
        self.board_state = bs;
    }

    fn board_zoom(&mut self, which: &str, f: f32, canvas: Rect) {
        let d = &mut self.board_data(which)["view"];
        let (x, y, z) = (d["x"].as_f64().unwrap_or(0.0) as f32, d["y"].as_f64().unwrap_or(0.0) as f32, d["z"].as_f64().unwrap_or(1.0) as f32);
        let nz = (z * f).clamp(0.15, 3.0);
        let c = canvas.size() / 2.0;
        *d = json!({ "x": c.x - (c.x - x) * nz / z, "y": c.y - (c.y - y) * nz / z, "z": nz });
        self.changed();
    }

    fn board_cmd(&mut self, which: &str, cmd: BoardCmd, bs: &mut Board, _acts: &mut [Action], b: &Book) {
        let _ = b;
        match cmd {
            BoardCmd::OpenScene(id) => self.sel = Some(Sel::Cena(id)),
            BoardCmd::OpenUrl(u) => {
                let target = u.trim_start_matches("[[").trim_end_matches("]]").to_string();
                if u.starts_with("[[") {
                    self.pending_open = Some(target);
                } else if !u.is_empty() {
                    let _ = open::that(u);
                }
            }
            BoardCmd::ToScene(id) => {
                let d = self.board_data(which);
                let Some(c) = d["cards"].as_array().and_then(|a| a.iter().find(|c| c["id"] == id.as_str())).cloned() else { return };
                let texto = str_of(&c, "texto").to_string();
                let mut lines = texto.lines();
                let titulo: String = lines.next().unwrap_or("").chars().take(80).collect();
                let resumo = lines.collect::<Vec<_>>().join("\n").trim().to_string();
                let sid = en::new_scene(&mut self.data, None, &[]);
                if let Some(i) = en::scene_index(&self.data, &sid) {
                    self.data["cenas"][i]["titulo"] = json!(titulo);
                    self.data["cenas"][i]["resumo"] = json!(resumo);
                }
                if let Some(c) = self.board_data(which)["cards"].as_array_mut().and_then(|a| a.iter_mut().find(|c| c["id"] == id.as_str())) {
                    c["t"] = json!("cena");
                    c["ref"] = json!(sid);
                    if let Some(o) = c.as_object_mut() {
                        o.remove("texto");
                    }
                }
                self.sel = Some(Sel::Cena(sid));
            }
            other => {
                let d = self.board_data(which);
                match other {
                    BoardCmd::Move(id, x, y) => {
                        if let Some(c) = d["cards"].as_array_mut().and_then(|a| a.iter_mut().find(|c| c["id"] == id.as_str())) {
                            c["x"] = json!(x);
                            c["y"] = json!(y);
                        }
                    }
                    BoardCmd::Link(a, bb) => {
                        d["links"].as_array_mut().unwrap().push(json!({ "id": en::uid("l"), "a": a, "b": bb, "seta": true }));
                    }
                    BoardCmd::Unlink(id) => d["links"].as_array_mut().unwrap().retain(|l| l["id"] != id.as_str()),
                    BoardCmd::Flip(id) => {
                        if let Some(l) = d["links"].as_array_mut().and_then(|a| a.iter_mut().find(|l| l["id"] == id.as_str())) {
                            let (a, bb) = (l["a"].clone(), l["b"].clone());
                            l["a"] = bb;
                            l["b"] = a;
                        }
                    }
                    BoardCmd::LinkSet(id, k, v) => {
                        if let Some(l) = d["links"].as_array_mut().and_then(|a| a.iter_mut().find(|l| l["id"] == id.as_str())) {
                            set_or_remove(l, k, v);
                        }
                    }
                    BoardCmd::CardSet(id, k, v) => {
                        if let Some(c) = d["cards"].as_array_mut().and_then(|a| a.iter_mut().find(|c| c["id"] == id.as_str())) {
                            set_or_remove(c, k, v);
                        }
                    }
                    BoardCmd::Raise(id) => {
                        let a = d["cards"].as_array_mut().unwrap();
                        if let Some(i) = a.iter().position(|c| c["id"] == id.as_str()) {
                            let c = a.remove(i);
                            a.push(c);
                        }
                    }
                    BoardCmd::Add(c) => {
                        bs.sel = c["id"].as_str().map(String::from);
                        d["cards"].as_array_mut().unwrap().push(c);
                    }
                    BoardCmd::Remove(id) => {
                        d["cards"].as_array_mut().unwrap().retain(|c| c["id"] != id.as_str());
                        d["links"].as_array_mut().unwrap().retain(|l| l["a"] != id.as_str() && l["b"] != id.as_str());
                        bs.sel = None;
                    }
                    _ => {}
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn board_dialogs(&mut self, ctx: &egui::Context, p: Palette, which: &str, bs: &mut Board, chars: &[String], canvas: Rect, view: (f32, f32, f32), cx: &mut Cx) {
        let center = ((canvas.center() - canvas.min - vec2(view.0, view.1)) / view.2).to_pos2() - vec2(105.0, 40.0);
        // editing a card's text, or a thread's label
        if let Some(e) = bs.edit.as_mut() {
            let (kind, is_link) = match &e.link_label {
                Some(_) => ("rotulo".to_string(), true),
                None => (self.board_data(which)["cards"].as_array().and_then(|a| a.iter().find(|c| c["id"] == e.id.as_str())).map(|c| str_of(c, "t").to_string()).unwrap_or_default(), false),
            };
            let mut done = None;
            kit::scrim(ctx, p);
            egui::Window::new("board-edit").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 120.0]).show(ctx, |ui| {
                ui.set_width(400.0);
                if kit::cmd_enter(ui.ctx()) {
                    done = Some(true);
                }
                ui.label(RichText::new(if is_link { "Rótulo da ligação" } else { "Editar cartão" }).font(kit::medium(16.0)).color(p.text));
                ui.add_space(8.0);
                let field = |ui: &mut egui::Ui, l: &str, v: &mut String, hint: &str, rows: usize, focus: bool| {
                    ui.label(RichText::new(l).size(12.0).color(p.muted));
                    let r = if rows > 1 { ui.add(egui::TextEdit::multiline(v).desired_rows(rows).desired_width(f32::INFINITY).hint_text(RichText::new(hint).color(p.faint)).margin(egui::Margin::symmetric(8, 6))) } else { ui.add(egui::TextEdit::singleline(v).desired_width(f32::INFINITY).hint_text(RichText::new(hint).color(p.faint)).margin(egui::Margin::symmetric(8, 6))) };
                    if focus && !ui.memory(|m| m.focused().is_some()) {
                        r.request_focus();
                    }
                    ui.add_space(6.0);
                };
                if let Some(lab) = e.link_label.as_mut() {
                    field(ui, "Rótulo", lab, "Ex.: irmãos, traiu, ama, deve a vida", 1, true);
                } else {
                    match kind.as_str() {
                        "link" => {
                            field(ui, "Endereço", &mut e.url, "https://… ou [[nota]]", 1, true);
                            field(ui, "Título", &mut e.titulo, "", 1, false);
                            field(ui, "Por que guardar?", &mut e.texto, "", 3, false);
                        }
                        "evento" => {
                            field(ui, "Título", &mut e.titulo, "", 1, true);
                            field(ui, "Data / época", &mut e.data, "", 1, false);
                            field(ui, "Texto", &mut e.texto, "", 3, false);
                        }
                        "nota" => field(ui, "Nota", &mut e.texto, "Escreva…", 5, true),
                        _ => field(ui, "Anotação no cartão", &mut e.texto, "", 3, true),
                    }
                }
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::primary(ui, "Salvar", p).clicked() {
                        done = Some(true);
                    }
                    if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = Some(false);
                    }
                });
            });
            match done {
                Some(true) => {
                    let e = bs.edit.take().unwrap();
                    let d = self.board_data(which);
                    if let Some(l) = e.link_label {
                        if let Some(x) = d["links"].as_array_mut().and_then(|a| a.iter_mut().find(|x| x["id"] == e.id.as_str())) {
                            x["rotulo"] = json!(l);
                        }
                    } else if let Some(c) = d["cards"].as_array_mut().and_then(|a| a.iter_mut().find(|c| c["id"] == e.id.as_str())) {
                        c["texto"] = json!(e.texto);
                        match kind.as_str() {
                            "link" => {
                                c["url"] = json!(e.url);
                                c["titulo"] = json!(if e.titulo.is_empty() { e.url.clone() } else { e.titulo });
                            }
                            "evento" => {
                                c["titulo"] = json!(e.titulo);
                                c["data"] = json!(e.data);
                            }
                            _ => {}
                        }
                    }
                    self.changed();
                }
                Some(false) => {
                    bs.edit = None;
                }
                None => {}
            }
            return;
        }
        // adding a character (1), a scene (2) or an image (3)
        let Some((kind, filter)) = bs.adding.as_mut() else { return };
        let kind = *kind;
        let mut items: Vec<(String, String)> = match kind {
            1 => {
                let present: Vec<String> = self.board_data(which)["cards"].as_array().into_iter().flatten().filter_map(|c| c["link"].as_str().map(String::from)).collect();
                let mut v = vec![("__all".to_string(), "★ Todos os que ainda não estão no quadro".to_string())];
                v.extend(chars.iter().filter(|c| !present.contains(c)).map(|c| (c.clone(), note_name(c))));
                v
            }
            2 => {
                let mut v = vec![("__new".to_string(), "＋ Nova cena".to_string())];
                v.extend(self.data["cenas"].as_array().into_iter().flatten().map(|s| (str_of(s, "id").to_string(), if str_of(s, "titulo").is_empty() { "Cena sem título".to_string() } else { str_of(s, "titulo").to_string() })));
                v
            }
            _ => {
                let ix = cx.vault.index.read().unwrap();
                let mut v: Vec<(String, String)> = ix.file_paths().filter(|f| {
                    let l = f.to_lowercase();
                    [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|e| l.ends_with(e))
                }).map(|f| (f.to_string(), f.to_string())).collect();
                v.sort();
                v
            }
        };
        let f = ateliet_core::scriptorium::mundo::deaccent(filter);
        items.retain(|x| f.is_empty() || x.0.starts_with("__") || ateliet_core::scriptorium::mundo::deaccent(&x.1).contains(&f));
        let mut chosen = None;
        let mut close = false;
        kit::scrim(ctx, p);
        egui::Window::new("board-add").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 100.0]).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(match kind { 1 => "Personagem", 2 => "Cena", _ => "Imagem" }).font(kit::medium(16.0)).color(p.text));
            ui.add_space(6.0);
            ui.add(egui::TextEdit::singleline(filter).hint_text(RichText::new("Filtrar").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6))).request_focus();
            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                for (k, l) in items.iter().take(300) {
                    if kit::Row::new(l).icon(match kind { 1 => icons::USER, 2 => icons::CLAPPERBOARD, _ => icons::IMAGE }).show(ui, p).clicked() {
                        chosen = Some(k.clone());
                    }
                }
            });
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if let Some(k) = chosen {
            bs.adding = None;
            let base = |t: Json, i: usize| -> Json {
                let mut t = t;
                t["id"] = json!(en::uid("c"));
                t["x"] = json!((center.x + i as f32 * 30.0).round());
                t["y"] = json!((center.y + i as f32 * 20.0).round());
                if t["w"].is_null() {
                    t["w"] = json!(210);
                }
                t
            };
            let new: Vec<Json> = match (kind, k.as_str()) {
                (1, "__all") => items.iter().filter(|x| !x.0.starts_with("__")).enumerate().map(|(i, x)| base(json!({ "t": "personagem", "link": x.0 }), i)).collect(),
                (1, _) => vec![base(json!({ "t": "personagem", "link": k }), 0)],
                (2, "__new") => {
                    let sid = en::new_scene(&mut self.data, None, &[]);
                    self.sel = Some(Sel::Cena(sid.clone()));
                    self.focus_title = true;
                    vec![base(json!({ "t": "cena", "ref": sid, "w": 220 }), 0)]
                }
                (2, _) => vec![base(json!({ "t": "cena", "ref": k, "w": 220 }), 0)],
                _ => vec![base(json!({ "t": "imagem", "link": k, "w": 240 }), 0)],
            };
            let d = self.board_data(which);
            for c in new {
                d["cards"].as_array_mut().unwrap().push(c);
            }
            self.changed();
        } else if close {
            bs.adding = None;
        }
    }
}

pub(super) enum BoardCmd {
    Raise(String),
    Move(String, f32, f32),
    Link(String, String),
    Unlink(String),
    Flip(String),
    LinkSet(String, &'static str, Json),
    CardSet(String, &'static str, Json),
    Add(Json),
    Remove(String),
    OpenScene(String),
    OpenUrl(String),
    ToScene(String),
}

fn set_or_remove(o: &mut Json, k: &str, v: Json) {
    if v.is_null() || v == "" {
        if let Some(m) = o.as_object_mut() {
            m.remove(k);
        }
    } else {
        o[k] = v;
    }
}

fn card_edit(c: &Json) -> CardEdit {
    CardEdit { id: str_of(c, "id").into(), titulo: str_of(c, "titulo").into(), texto: str_of(c, "texto").into(), data: str_of(c, "data").into(), url: str_of(c, "url").into(), link_label: None }
}

fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// A view that frames every card.
fn fit(cards: &[Json], size: Vec2) -> (f32, f32, f32) {
    let f = |c: &Json, k: &str| c[k].as_f64().unwrap_or(0.0) as f32;
    let x0 = cards.iter().map(|c| f(c, "x")).fold(f32::INFINITY, f32::min) - 40.0;
    let y0 = cards.iter().map(|c| f(c, "y")).fold(f32::INFINITY, f32::min) - 40.0;
    let x1 = cards.iter().map(|c| f(c, "x") + c["w"].as_f64().unwrap_or(210.0) as f32).fold(f32::NEG_INFINITY, f32::max) + 40.0;
    let y1 = cards.iter().map(|c| f(c, "y") + 200.0).fold(f32::NEG_INFINITY, f32::max) + 40.0;
    let z = (size.x / (x1 - x0)).min(size.y / (y1 - y0)).clamp(0.15, 1.5);
    ((size.x - (x1 - x0) * z) / 2.0 - x0 * z, (size.y - (y1 - y0) * z) / 2.0 - y0 * z, z)
}
