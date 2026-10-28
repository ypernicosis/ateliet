//! The toolbar, the style panel, the menus (card, column, drawing, the
//! empty canvas), the vault picker and the PNG export.

use super::{Cx, Out, QuadroEditor, Sel, Tool};
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::quadro::card;
use ateliet_core::quadro::Format;
use eframe::egui::{self, pos2, vec2, Color32, RichText, Sense};
use serde_json::{json, Value};

const STROKES: [&str; 5] = ["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00"];
const FILLS: [&str; 5] = ["transparent", "#ffc9c9", "#b2f2bb", "#a5d8ff", "#ffec99"];
const CARD_COLORS: [(&str, &str); 6] = [("Vermelho", "#e03131"), ("Laranja", "#f08c00"), ("Amarelo", "#f5c400"), ("Verde", "#2f9e44"), ("Azul", "#1971c2"), ("Roxo", "#9c36b5")];

fn swatch(ui: &mut egui::Ui, hex: &str, on: bool, p: Palette) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    let c = ateliet_core::quadro::rough::color(hex);
    match c {
        Some(c) => {
            ui.painter().rect_filled(rect.shrink(2.0), 4.0, super::draw::ink(c, p.dark));
        }
        None => {
            // transparent: a crossed box
            ui.painter().rect_stroke(rect.shrink(2.0), 4.0, egui::Stroke::new(1.0_f32, p.border_strong), egui::StrokeKind::Inside);
            ui.painter().line_segment([rect.shrink(4.0).left_bottom(), rect.shrink(4.0).right_top()], egui::Stroke::new(1.0_f32, p.error));
        }
    }
    if on {
        ui.painter().rect_stroke(rect, 5.0, egui::Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Inside);
    }
    r.on_hover_text(hex).clicked()
}

/// A row of small choices; returns the one clicked.
fn choices<T: Clone + PartialEq>(ui: &mut egui::Ui, items: &[(T, &str, &str)], cur: &T, p: Palette) -> Option<T> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (v, glyph, tip) in items {
            let on = v == cur;
            let (rect, r) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::click());
            let fill = if on { p.accent_soft } else if r.hovered() { p.bg_hover } else { p.bg_secondary };
            ui.painter().rect_filled(rect, 6.0, fill);
            let font = if glyph.chars().next().is_some_and(|c| c as u32 >= 0xe000) { kit::icon_font(14.0) } else { egui::FontId::proportional(12.0) };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, font, if on { p.accent_strong } else { p.text });
            if r.on_hover_text(*tip).clicked() {
                out = Some(v.clone());
            }
        }
    });
    out
}

fn label(ui: &mut egui::Ui, t: &str, p: Palette) {
    ui.add_space(4.0);
    ui.label(RichText::new(t).size(11.5).color(p.faint));
}

impl QuadroEditor {
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let p = cx.palette;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            egui::Frame::new().fill(p.bg_secondary).corner_radius(9.0).inner_margin(3.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut tools = vec![
                    (Tool::Select, icons::MOUSE_POINTER_2, "Seleção (V ou 1)"),
                    (Tool::Hand, icons::HAND, "Mão (H, ou segure Espaço)"),
                ];
                if self.board.format == Format::Kanban {
                    tools.push((Tool::Card, icons::STICKY_NOTE, "Cartão (C): clique numa coluna ou no quadro"));
                    tools.push((Tool::Lane, icons::COLUMNS_3, "Coluna (K)"));
                }
                tools.extend([
                    (Tool::Rect, icons::SQUARE, "Retângulo (R ou 2)"),
                    (Tool::Diamond, icons::DIAMOND, "Losango (D ou 3)"),
                    (Tool::Ellipse, icons::CIRCLE, "Elipse (O ou 4)"),
                    (Tool::Arrow, icons::ARROW_UP_RIGHT, "Seta (A ou 5): prende-se a formas e cartões"),
                    (Tool::Line, icons::MINUS, "Linha (L ou 6)"),
                    (Tool::Draw, icons::PENCIL, "Traço à mão (P ou 7)"),
                    (Tool::Text, icons::TYPE, "Texto (T ou 8)"),
                    (Tool::Eraser, icons::ERASER, "Borracha (E ou 0)"),
                ]);
                for (t, glyph, tip) in tools {
                    let sel = self.tool == t;
                    let (rect, r) = ui.allocate_exact_size(vec2(30.0, 28.0), Sense::click());
                    if sel {
                        ui.painter().rect(rect, 7.0, p.bg, egui::Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                    } else if r.hovered() {
                        ui.painter().rect_filled(rect, 7.0, p.bg_hover);
                    }
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(15.0), if sel { p.text } else { p.muted });
                    if r.on_hover_text(tip).clicked() {
                        self.tool = t;
                    }
                }
            });
            if kit::icon_button(ui, if self.lock { icons::LOCK } else { icons::LOCK_OPEN }, "Manter a ferramenta depois de desenhar (Q)", self.lock, p).clicked() {
                self.lock = !self.lock;
            }
            if kit::icon_button(ui, icons::FILE_PLUS, "Inserir nota ou imagem do cofre (I) · também dá para arrastar do explorador", self.insert.is_some(), p).clicked() {
                self.insert = if self.insert.is_some() { None } else { Some(String::new()) };
            }
            if self.board.format == Format::Kanban && kit::icon_button(ui, icons::PLUS, "Nova coluna à direita", false, p).clicked() {
                let id = self.add_lane(None, "Nova coluna");
                self.sel = vec![Sel::Lane(id.clone())];
                self.edit_lane(&id, true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if kit::icon_button(ui, icons::IMAGE_DOWN, "Exportar PNG: a seleção, ou tudo", self.export.is_some(), p).clicked() {
                    self.export_png(cx);
                }
                if kit::icon_button(ui, icons::MAXIMIZE, "Ajustar à tela (Shift+1)", false, p).clicked() {
                    self.fit = true;
                }
                ui.add_space(6.0);
                ui.add_enabled_ui(!self.redo.is_empty(), |ui| {
                    if kit::icon_button(ui, icons::REDO_2, "Refazer (Ctrl+Shift+Z)", false, p).clicked() {
                        self.redo();
                    }
                });
                ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                    if kit::icon_button(ui, icons::UNDO_2, "Desfazer (Ctrl+Z)", false, p).clicked() {
                        self.undo();
                    }
                });
            });
        });
    }

    /// The look of the selection (or of what the tool will draw).
    pub(super) fn props(&mut self, ui: &mut egui::Ui, canvas: egui::Rect, p: Palette) {
        let drawing_tool = matches!(self.tool, Tool::Rect | Tool::Diamond | Tool::Ellipse | Tool::Arrow | Tool::Line | Tool::Draw | Tool::Text);
        let els = self.sel_els();
        if !drawing_tool && els.is_empty() {
            return;
        }
        // what the panel shows: the first selected element, else the style
        let first = els.first().and_then(|id| self.board.element(id)).cloned();
        let kinds: Vec<String> = els.iter().filter_map(|id| self.board.element(id).map(|e| e.kind().to_string())).collect();
        let tool_kind = match self.tool {
            Tool::Text => "text",
            Tool::Arrow => "arrow",
            Tool::Line => "line",
            Tool::Draw => "freedraw",
            _ => "rectangle",
        };
        let has = |k: &str| if kinds.is_empty() { tool_kind == k } else { kinds.iter().any(|x| x == k) };
        let st = &self.style;
        let get_s = |k: &str, d: &str| first.as_ref().map(|e| e.s(k).to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| d.to_string());
        let get_f = |k: &str, d: f32| first.as_ref().map(|e| e.fo(k, d)).unwrap_or(d);
        let stroke = get_s("strokeColor", &st.stroke);
        let bg = get_s("backgroundColor", &st.background);
        let fill = get_s("fillStyle", &st.fill);
        let width = get_f("strokeWidth", st.width);
        let dash = get_s("strokeStyle", &st.dash);
        let rough = get_f("roughness", st.roughness);
        let opacity = get_f("opacity", st.opacity);
        let font = get_f("fontSize", st.font_size);
        let family = first.as_ref().and_then(|e| e.get("fontFamily").as_i64()).unwrap_or(st.font_family as i64);
        let align = get_s("textAlign", &st.align);
        let round = first.as_ref().map(|e| !e.get("roundness").is_null()).unwrap_or(st.round);
        let start_head = first.as_ref().map(|e| e.get("startArrowhead").as_str().map(str::to_string)).unwrap_or(st.start_head.clone());
        let end_head = first.as_ref().map(|e| e.get("endArrowhead").as_str().map(str::to_string)).unwrap_or(st.end_head.clone());
        let mut change: Vec<(&str, Value)> = Vec::new();
        let mut act: Option<&str> = None;
        let pos = canvas.left_top() + vec2(12.0, 12.0);
        egui::Area::new(egui::Id::new(("quadro-props", &self.path))).fixed_pos(pos).order(egui::Order::Middle).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(p.bg).stroke(egui::Stroke::new(1.0_f32, p.border)).corner_radius(10.0).inner_margin(10.0).shadow(egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(if p.dark { 80 } else { 22 }) }).show(ui, |ui| {
                ui.set_width(172.0);
                ui.spacing_mut().item_spacing.y = 3.0;
                let (hr, hresp) = ui.allocate_exact_size(vec2(172.0, 18.0), Sense::click());
                ui.painter().text(hr.left_center(), egui::Align2::LEFT_CENTER, "Estilo", kit::medium(12.0), p.muted);
                ui.painter().text(hr.right_center(), egui::Align2::RIGHT_CENTER, if self.props_open { icons::CHEVRON_UP } else { icons::CHEVRON_DOWN }, kit::icon_font(13.0), p.faint);
                if hresp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.props_open = !self.props_open;
                }
                if !self.props_open {
                    return;
                }
                label(ui, "Traço", p);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for c in STROKES {
                        if swatch(ui, c, stroke.eq_ignore_ascii_case(c), p) {
                            change.push(("strokeColor", json!(c)));
                        }
                    }
                    let mut cc = ateliet_core::quadro::rough::color(&stroke).map(|c| Color32::from_rgb(c[0], c[1], c[2])).unwrap_or(Color32::BLACK);
                    if egui::color_picker::color_edit_button_srgba(ui, &mut cc, egui::color_picker::Alpha::Opaque).changed() {
                        change.push(("strokeColor", json!(format!("#{:02x}{:02x}{:02x}", cc.r(), cc.g(), cc.b()))));
                    }
                });
                if !has("text") || kinds.len() > 1 || (kinds.is_empty() && tool_kind != "text") {
                    if !(kinds.iter().all(|k| k == "arrow" || k == "line" || k == "freedraw") && !kinds.is_empty()) && !matches!(tool_kind, "arrow" | "line" | "freedraw") || kinds.iter().any(|k| matches!(k.as_str(), "rectangle" | "ellipse" | "diamond")) {
                        label(ui, "Fundo", p);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for c in FILLS {
                                if swatch(ui, c, bg.eq_ignore_ascii_case(c), p) {
                                    change.push(("backgroundColor", json!(c)));
                                }
                            }
                            let mut cc = ateliet_core::quadro::rough::color(&bg).map(|c| Color32::from_rgb(c[0], c[1], c[2])).unwrap_or(Color32::WHITE);
                            if egui::color_picker::color_edit_button_srgba(ui, &mut cc, egui::color_picker::Alpha::Opaque).changed() {
                                change.push(("backgroundColor", json!(format!("#{:02x}{:02x}{:02x}", cc.r(), cc.g(), cc.b()))));
                            }
                        });
                        if bg != "transparent" {
                            label(ui, "Preenchimento", p);
                            if let Some(v) = choices(ui, &[("hachure".to_string(), "///", "Hachura"), ("cross-hatch".to_string(), "XX", "Hachura cruzada"), ("solid".to_string(), "■", "Sólido")], &fill, p) {
                                change.push(("fillStyle", json!(v)));
                            }
                        }
                    }
                    label(ui, "Espessura", p);
                    if let Some(v) = choices(ui, &[(1.0f32, "─", "Fina"), (2.0, "━", "Média"), (4.0, "▬", "Grossa")], &width, p) {
                        change.push(("strokeWidth", json!(v)));
                    }
                    if !has("freedraw") || kinds.len() > 1 {
                        label(ui, "Traçado", p);
                        if let Some(v) = choices(ui, &[("solid".to_string(), "—", "Contínuo"), ("dashed".to_string(), "- -", "Tracejado"), ("dotted".to_string(), "···", "Pontilhado")], &dash, p) {
                            change.push(("strokeStyle", json!(v)));
                        }
                        label(ui, "Rabisco", p);
                        if let Some(v) = choices(ui, &[(0.0f32, "∿0", "Liso"), (1.0, "∿1", "Artista"), (2.0, "∿2", "Cartunista")], &rough, p) {
                            change.push(("roughness", json!(v)));
                        }
                    }
                    if has("rectangle") || has("diamond") || has("arrow") || has("line") {
                        label(ui, if has("arrow") && kinds.len() <= 1 { "Seta" } else { "Cantos" }, p);
                        if let Some(v) = choices(ui, &[(false, if has("arrow") { "⌐" } else { "□" }, "Retos"), (true, if has("arrow") { "⌒" } else { "▢" }, "Arredondados / curva")], &round, p) {
                            change.push(("roundness", json!(v)));
                        }
                    }
                }
                if has("arrow") {
                    let heads = [(None, "—", "Sem ponta"), (Some("arrow".to_string()), "›", "Seta"), (Some("triangle".to_string()), "▶", "Triângulo"), (Some("dot".to_string()), "●", "Ponto"), (Some("bar".to_string()), "|", "Barra")];
                    label(ui, "Ponta inicial", p);
                    if let Some(v) = choices(ui, &heads, &start_head, p) {
                        change.push(("startArrowhead", json!(v)));
                    }
                    label(ui, "Ponta final", p);
                    if let Some(v) = choices(ui, &heads, &end_head, p) {
                        change.push(("endArrowhead", json!(v)));
                    }
                }
                if has("text") {
                    label(ui, "Tamanho do texto", p);
                    if let Some(v) = choices(ui, &[(16.0f32, "P", "Pequeno"), (20.0, "M", "Médio"), (28.0, "G", "Grande"), (36.0, "GG", "Muito grande")], &font, p) {
                        change.push(("fontSize", json!(v)));
                    }
                    label(ui, "Fonte e alinhamento", p);
                    ui.horizontal(|ui| {
                        if let Some(v) = choices(ui, &[(5i64, "✎", "À mão"), (2, "Aa", "Normal"), (3, "</>", "Código")], &family, p) {
                            change.push(("fontFamily", json!(v)));
                        }
                    });
                    if let Some(v) = choices(ui, &[("left".to_string(), icons::ALIGN_LEFT, "À esquerda"), ("center".to_string(), icons::ALIGN_CENTER, "Ao centro"), ("right".to_string(), icons::ALIGN_RIGHT, "À direita")], &align, p) {
                        change.push(("textAlign", json!(v)));
                    }
                }
                label(ui, "Opacidade", p);
                let mut o = opacity;
                ui.spacing_mut().slider_width = 130.0;
                let r = ui.add(egui::Slider::new(&mut o, 0.0..=100.0).show_value(false));
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    change.push(("opacity", json!(o.round())));
                }
                if !els.is_empty() {
                    label(ui, "Camadas e ações", p);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (glyph, tip, a) in [(icons::BRING_TO_FRONT, "Trazer para a frente (Ctrl+Shift+])", "front"), (icons::SEND_TO_BACK, "Enviar para trás (Ctrl+Shift+[)", "back"), (icons::GROUP, "Agrupar (Ctrl+G)", "group"), (icons::UNGROUP, "Desagrupar (Ctrl+Shift+G)", "ungroup"), (icons::COPY_PLUS, "Duplicar (Ctrl+D)", "dup"), (icons::TRASH_2, "Excluir (Delete)", "del")] {
                            if kit::icon_button_sized(ui, glyph, tip, false, p, 26.0, 14.0).clicked() {
                                act = Some(a);
                            }
                        }
                    });
                }
            });
        });
        for (k, v) in change {
            // the style of new elements follows
            match k {
                "strokeColor" => self.style.stroke = v.as_str().unwrap_or("#1e1e1e").into(),
                "backgroundColor" => self.style.background = v.as_str().unwrap_or("transparent").into(),
                "fillStyle" => self.style.fill = v.as_str().unwrap_or("hachure").into(),
                "strokeWidth" => self.style.width = v.as_f64().unwrap_or(2.0) as f32,
                "strokeStyle" => self.style.dash = v.as_str().unwrap_or("solid").into(),
                "roughness" => self.style.roughness = v.as_f64().unwrap_or(1.0) as f32,
                "opacity" => self.style.opacity = v.as_f64().unwrap_or(100.0) as f32,
                "fontSize" => self.style.font_size = v.as_f64().unwrap_or(20.0) as f32,
                "fontFamily" => self.style.font_family = v.as_u64().unwrap_or(5) as u32,
                "textAlign" => self.style.align = v.as_str().unwrap_or("left").into(),
                "roundness" => self.style.round = v.as_bool().unwrap_or(true),
                "startArrowhead" => self.style.start_head = v.as_str().map(str::to_string),
                "endArrowhead" => self.style.end_head = v.as_str().map(str::to_string),
                _ => {}
            }
            self.restyle(k, v.clone());
            if k == "fontSize" || k == "fontFamily" {
                let ctx = ui.ctx().clone();
                for id in self.sel_els() {
                    let texts: Vec<String> = std::iter::once(id.clone()).chain(self.board.element(&id).map(|e| e.bound().into_iter().filter(|(_, k)| k == "text").map(|(i, _)| i).collect::<Vec<_>>()).unwrap_or_default()).collect();
                    for t in texts {
                        if self.board.element(&t).is_some_and(|e| e.kind() == "text") {
                            if k == "fontSize" {
                                if let Some(e) = self.board.element_mut(&t) {
                                    e.set("fontSize", v.clone());
                                }
                            }
                            self.fit_text(&ctx, &t);
                        }
                    }
                }
            }
        }
        match act {
            Some("front") => self.reorder(true),
            Some("back") => self.reorder(false),
            Some("group") => self.group(true),
            Some("ungroup") => self.group(false),
            Some("del") => self.delete_sel(),
            Some("dup") => self.pending_dup = true,
            _ => {}
        }
    }

    /// The month grid for a card's date; Some(date) when one is picked.
    fn calendar(&mut self, ui: &mut egui::Ui, cur: Option<&str>, p: Palette) -> Option<String> {
        use chrono::Datelike;
        let today = chrono::Local::now().date_naive();
        let cur_d = cur.and_then(|c| chrono::NaiveDate::parse_from_str(c, "%Y-%m-%d").ok());
        let (y, m) = self.menu_month.unwrap_or_else(|| {
            let d = cur_d.unwrap_or(today);
            (d.year(), d.month())
        });
        let mut out = None;
        ui.horizontal(|ui| {
            if kit::icon_button_sized(ui, icons::CHEVRON_LEFT, "Mês anterior", false, p, 22.0, 13.0).clicked() {
                self.menu_month = Some(if m == 1 { (y - 1, 12) } else { (y, m - 1) });
            }
            const MONTHS: [&str; 12] = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
            ui.label(RichText::new(format!("{} {y}", MONTHS[(m - 1) as usize])).font(kit::medium(12.5)).color(p.text));
            if kit::icon_button_sized(ui, icons::CHEVRON_RIGHT, "Próximo mês", false, p, 22.0, 13.0).clicked() {
                self.menu_month = Some(if m == 12 { (y + 1, 1) } else { (y, m + 1) });
            }
        });
        let first = chrono::NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(today);
        let offset = first.weekday().num_days_from_sunday() as i64;
        egui::Grid::new("quadro-cal").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
            for d in ["D", "S", "T", "Q", "Q", "S", "S"] {
                ui.label(RichText::new(d).size(10.5).color(p.faint));
            }
            ui.end_row();
            for i in 0..42i64 {
                let day = first + chrono::Duration::days(i - offset);
                let (rect, r) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
                let picked = cur_d == Some(day);
                let fill = if picked { p.accent_strong } else if r.hovered() { p.bg_hover } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(rect, 5.0, fill);
                let col = if picked { p.on_accent } else if day.month() != m { p.faint } else if day == today { p.accent_strong } else { p.text };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, day.day().to_string(), egui::FontId::proportional(11.5), col);
                if r.clicked() {
                    out = Some(day.format("%Y-%m-%d").to_string());
                }
                if i % 7 == 6 {
                    ui.end_row();
                }
            }
        });
        out
    }

    /// The menu open on the canvas, if any.
    pub(super) fn menus(&mut self, ui: &mut egui::Ui, cx: &mut Cx, v: &super::View) {
        if self.pending_dup {
            self.pending_dup = false;
            self.duplicate(cx);
        }
        let Some((target, at, w)) = self.menu.clone() else { return };
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        let mut close = false;
        let area = egui::Area::new(egui::Id::new(("quadro-menu", &self.path))).fixed_pos(at).order(egui::Order::Foreground).constrain(true).show(&ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(220.0);
                match &target {
                    Some(Sel::Card(id)) => close = self.card_menu(ui, cx, id, p),
                    Some(Sel::Lane(id)) => close = self.lane_menu(ui, id, p),
                    Some(Sel::El(id)) => close = self.element_menu(ui, cx, id, p),
                    None => close = self.ground_menu(ui, cx, w, p),
                }
            });
        });
        let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|q| !area.response.rect.contains(q)));
        if close || outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.menu = None;
        }
        let _ = v;
    }

    fn card_menu(&mut self, ui: &mut egui::Ui, cx: &mut Cx, id: &str, p: Palette) -> bool {
        let Some(c) = self.board.card(id).cloned() else { return true };
        let trig = ateliet_core::quadro::layout::triggers(&self.board);
        let info = card::parse(&c.text, &trig.0, &trig.1);
        let loose = self.board.cards.iter().any(|x| x.id == id);
        if kit::menu_item(ui, icons::PENCIL, "Editar", p) {
            self.edit_card(id, false);
            return true;
        }
        if let Some(n) = info.note() {
            if kit::menu_item(ui, icons::FILE_TEXT, "Abrir nota", p) {
                self.out.push(Out::Open(n.to_string()));
                return true;
            }
        }
        if kit::menu_item(ui, icons::SQUARE_CHECK, if c.check == ' ' { "Marcar como concluído" } else { "Reabrir" }, p) {
            self.checkpoint();
            if let Some(c) = self.board.card_mut(id) {
                c.check = if c.check == ' ' { 'x' } else { ' ' };
            }
            self.changed();
            return true;
        }
        kit::menu_sep(ui, p);
        ui.label(RichText::new(format!("{}  Data", icons::CALENDAR)).size(12.0).color(p.muted));
        if let Some(d) = self.calendar(ui, info.date.as_deref(), p) {
            self.checkpoint();
            if let Some(c) = self.board.card_mut(id) {
                c.text = card::with_date(&c.text, Some(&d), &trig.0);
            }
            self.changed();
            return true;
        }
        if info.date.is_some() && kit::menu_item(ui, icons::X, "Tirar a data", p) {
            self.checkpoint();
            if let Some(c) = self.board.card_mut(id) {
                c.text = card::with_date(&c.text, None, &trig.0);
            }
            self.changed();
            return true;
        }
        kit::menu_sep(ui, p);
        ui.horizontal(|ui| {
            ui.label(kit::icon(icons::TAG, 13.0).color(p.muted));
            let r = ui.add(egui::TextEdit::singleline(&mut self.menu_text).hint_text("nova tag").desired_width(130.0));
            let go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (go || kit::icon_button_sized(ui, icons::PLUS, "Adicionar a tag", false, p, 22.0, 13.0).clicked()) && !self.menu_text.trim().is_empty() {
                let tag = self.menu_text.trim().replace(' ', "-");
                self.checkpoint();
                if let Some(c) = self.board.card_mut(id) {
                    c.text = card::with_tag(&c.text, &tag);
                }
                self.menu_text.clear();
                self.changed();
            }
        });
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for t in &info.tags {
                if kit::chip(ui, Some(icons::X), t, false, p).on_hover_text("Tirar a tag").clicked() {
                    remove = Some(t.clone());
                }
            }
        });
        if let Some(t) = remove {
            self.checkpoint();
            if let Some(c) = self.board.card_mut(id) {
                c.text = card::without_tag(&c.text, &t);
            }
            self.changed();
        }
        if loose {
            kit::menu_sep(ui, p);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if swatch(ui, "transparent", c.color.is_none(), p) {
                    self.checkpoint();
                    if let Some(c) = self.board.card_mut(id) {
                        c.color = None;
                    }
                    self.changed();
                }
                for (name, hex) in CARD_COLORS {
                    if swatch(ui, hex, c.color.as_deref() == Some(hex), p) {
                        self.checkpoint();
                        if let Some(c) = self.board.card_mut(id) {
                            c.color = Some(hex.to_string());
                        }
                        self.changed();
                    }
                    let _ = name; // the hex shows on hover
                }
            });
        }
        if !self.board.lanes.is_empty() {
            kit::menu_sep(ui, p);
            let mut to: Option<Option<usize>> = None;
            ui.menu_button(format!("{}  Mover para", icons::CORNER_DOWN_RIGHT), |ui| {
                for (i, l) in self.board.lanes.iter().enumerate() {
                    if kit::menu_item(ui, icons::COLUMNS_3, &l.title, p) {
                        to = Some(Some(i));
                    }
                }
                if !loose && kit::menu_item(ui, icons::STICKY_NOTE, "Soltar no quadro", p) {
                    to = Some(None);
                }
            });
            if let Some(dest) = to {
                self.checkpoint();
                match dest {
                    Some(li) if self.lane_full(li) && !self.board.lanes[li].cards.iter().any(|x| x.id == id) => {
                        self.uncheckpoint();
                        self.out.push(Out::Toast("A coluna está cheia.".into()));
                    }
                    Some(li) => {
                        let lb = &self.scene.lanes[li];
                        let at = (lb.rect.0 + 10.0, lb.rect.1 + lb.rect.3 + 100.0);
                        let _ = self.drop_card(id, at, at);
                    }
                    None => {
                        // beside the column it leaves
                        let cb = self.scene.card(id).map(|c| (c.rect.0, c.rect.1)).unwrap_or((0.0, 0.0));
                        if let Some((mut card, _)) = self.take_card(id) {
                            card.x = cb.0 + 300.0;
                            card.y = cb.1;
                            self.board.cards.push(card);
                            self.changed();
                        }
                    }
                }
                return true;
            }
        }
        kit::menu_sep(ui, p);
        if kit::menu_item(ui, icons::COPY_PLUS, "Duplicar", p) {
            self.duplicate(cx);
            return true;
        }
        if kit::menu_item(ui, icons::COPY, "Copiar", p) {
            if let Some(j) = self.copy_json() {
                ui.ctx().copy_text(j);
            }
            return true;
        }
        if kit::menu_danger(ui, icons::TRASH_2, "Excluir", p) {
            self.sel = vec![Sel::Card(id.to_string())];
            self.delete_sel();
            return true;
        }
        false
    }

    fn lane_menu(&mut self, ui: &mut egui::Ui, id: &str, p: Palette) -> bool {
        let Some(li) = self.board.lanes.iter().position(|l| l.id == id) else { return true };
        let l = self.board.lanes[li].clone();
        if kit::menu_item(ui, icons::PENCIL, "Renomear", p) {
            self.edit_lane(id, false);
            return true;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Limite de cartões").color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button_sized(ui, icons::PLUS, "Mais", false, p, 22.0, 13.0).clicked() {
                    self.checkpoint();
                    let n = self.board.lanes[li].max;
                    self.board.lanes[li].max = if n == 0 { l.cards.len().max(1) as u32 } else { n + 1 };
                    self.changed();
                }
                ui.label(RichText::new(if l.max == 0 { "sem".to_string() } else { l.max.to_string() }).color(p.muted));
                if kit::icon_button_sized(ui, icons::MINUS, "Menos (0 tira o limite)", false, p, 22.0, 13.0).clicked() && l.max > 0 {
                    self.checkpoint();
                    self.board.lanes[li].max -= 1;
                    self.changed();
                }
            });
        });
        if kit::menu_check(ui, l.complete.is_some(), "Cartões aqui ficam concluídos", p) {
            self.checkpoint();
            self.board.lanes[li].complete = if l.complete.is_some() { None } else { Some("Complete".into()) };
            if self.board.lanes[li].complete.is_some() {
                for c in self.board.lanes[li].cards.iter_mut() {
                    c.check = 'x';
                }
            }
            self.changed();
            return true;
        }
        if kit::menu_item(ui, if l.collapsed { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }, if l.collapsed { "Expandir" } else { "Recolher" }, p) {
            self.checkpoint();
            self.board.lanes[li].collapsed = !l.collapsed;
            self.changed();
            return true;
        }
        kit::menu_sep(ui, p);
        if li > 0 && kit::menu_item(ui, icons::ARROW_LEFT, "Mover para a esquerda", p) {
            self.shift_lane(li, false);
            return true;
        }
        if li + 1 < self.board.lanes.len() && kit::menu_item(ui, icons::ARROW_RIGHT, "Mover para a direita", p) {
            self.shift_lane(li, true);
            return true;
        }
        if l.cards.iter().any(|c| c.check != ' ') && kit::menu_item(ui, icons::ARCHIVE, "Arquivar os concluídos", p) {
            self.checkpoint();
            let (done, open): (Vec<_>, Vec<_>) = self.board.lanes[li].cards.drain(..).partition(|c| c.check != ' ');
            self.board.lanes[li].cards = open;
            self.board.archive.extend(done);
            self.changed();
            return true;
        }
        kit::menu_sep(ui, p);
        if kit::menu_danger(ui, icons::TRASH_2, if l.cards.is_empty() { "Excluir coluna" } else { "Excluir coluna e cartões" }, p) {
            self.sel = vec![Sel::Lane(id.to_string())];
            self.delete_sel();
            return true;
        }
        false
    }

    fn element_menu(&mut self, ui: &mut egui::Ui, cx: &mut Cx, id: &str, p: Palette) -> bool {
        let Some(e) = self.board.element(id).cloned() else { return true };
        let ctx = ui.ctx().clone();
        if e.kind() == "text" && kit::menu_item(ui, icons::PENCIL, "Editar texto", p) {
            self.edit_text(id, false);
            return true;
        }
        if matches!(e.kind(), "rectangle" | "ellipse" | "diamond") && kit::menu_item(ui, icons::TYPE, "Texto dentro", p) {
            self.sel = vec![Sel::El(id.to_string())];
            self.edit_inside(&ctx, id);
            return true;
        }
        if let Some(l) = e.get("link").as_str().filter(|l| !l.is_empty()) {
            if kit::menu_item(ui, icons::EXTERNAL_LINK, "Abrir link", p) {
                let target = l.trim_start_matches("[[").trim_end_matches("]]").split('|').next().unwrap_or("").to_string();
                if target.starts_with("http") {
                    let _ = open::that(&target);
                } else {
                    self.out.push(Out::Open(target));
                }
                return true;
            }
        }
        if kit::menu_item(ui, icons::BRING_TO_FRONT, "Trazer para a frente", p) {
            self.reorder(true);
            return true;
        }
        if kit::menu_item(ui, icons::SEND_TO_BACK, "Enviar para trás", p) {
            self.reorder(false);
            return true;
        }
        if self.sel_els().len() > 1 && kit::menu_item(ui, icons::GROUP, "Agrupar", p) {
            self.group(true);
            return true;
        }
        if !e.group_ids().is_empty() && kit::menu_item(ui, icons::UNGROUP, "Desagrupar", p) {
            self.group(false);
            return true;
        }
        kit::menu_sep(ui, p);
        if kit::menu_item(ui, icons::COPY_PLUS, "Duplicar", p) {
            self.duplicate(cx);
            return true;
        }
        if kit::menu_item(ui, icons::COPY, "Copiar", p) {
            if let Some(j) = self.copy_json() {
                ctx.copy_text(j);
            }
            return true;
        }
        if kit::menu_danger(ui, icons::TRASH_2, "Excluir", p) {
            self.delete_sel();
            return true;
        }
        false
    }

    fn ground_menu(&mut self, ui: &mut egui::Ui, cx: &mut Cx, w: (f32, f32), p: Palette) -> bool {
        let ctx = ui.ctx().clone();
        if kit::menu_item(ui, icons::CLIPBOARD, "Colar", p) {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                if let Some(t) = cb.get_text().ok().filter(|t| !t.is_empty()) {
                    self.paste_text(cx, &ctx, &t, w);
                } else if let Ok(img) = cb.get_image() {
                    let png = ateliet_core::board::tile::encode_png(&img.bytes, img.width as u32, img.height as u32);
                    self.paste_picture(cx, &ctx, png, w);
                }
            }
            return true;
        }
        kit::menu_sep(ui, p);
        if self.is_kanban() {
            if kit::menu_item(ui, icons::STICKY_NOTE, "Novo cartão aqui", p) {
                let id = self.add_loose_card((w.0, w.1), "");
                self.sel = vec![Sel::Card(id.clone())];
                self.edit_card(&id, true);
                return true;
            }
            if kit::menu_item(ui, icons::COLUMNS_3, "Nova coluna aqui", p) {
                let id = self.add_lane(Some(w), "Nova coluna");
                self.sel = vec![Sel::Lane(id.clone())];
                self.edit_lane(&id, true);
                return true;
            }
        }
        if kit::menu_item(ui, icons::TYPE, "Texto aqui", p) {
            self.new_text(&ctx, w);
            return true;
        }
        if kit::menu_item(ui, icons::FILE_PLUS, "Inserir nota ou imagem…", p) {
            self.insert = Some(String::new());
            return true;
        }
        kit::menu_sep(ui, p);
        if kit::menu_item(ui, icons::LIST_CHECKS, "Selecionar tudo", p) {
            self.select_all();
            return true;
        }
        if kit::menu_item(ui, icons::MAXIMIZE, "Ajustar à tela", p) {
            self.fit = true;
            return true;
        }
        if kit::menu_item(ui, icons::IMAGE_DOWN, "Exportar PNG", p) {
            self.export_png(cx);
            return true;
        }
        false
    }

    /// Find a note or picture of the vault and put it on the canvas.
    pub(super) fn insert_popup(&mut self, ui: &mut egui::Ui, cx: &mut Cx, canvas: egui::Rect) {
        let Some(mut q) = self.insert.take() else { return };
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        let mut chosen = None;
        let mut keep = true;
        egui::Area::new(egui::Id::new(("quadro-insert", &self.path))).fixed_pos(pos2(canvas.center().x - 190.0, canvas.top() + 16.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::menu(ui.style()).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(380.0);
                let r = kit::search_field(ui, &mut q, "Nota ou imagem do cofre…", p);
                if !r.has_focus() && q.is_empty() {
                    r.request_focus();
                }
                let needle = q.to_lowercase();
                let hits: Vec<&ateliet_core::vault::Entry> = cx
                    .vault
                    .files
                    .iter()
                    .filter(|e| !e.folder && e.path != self.path)
                    .filter(|e| {
                        let l = e.path.to_lowercase();
                        (l.ends_with(".md") || card::is_picture(&l)) && (needle.is_empty() || l.contains(&needle))
                    })
                    .take(40)
                    .collect();
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for e in &hits {
                        let icon = if card::is_picture(&e.path) { icons::IMAGE } else { icons::FILE_TEXT };
                        if kit::Row::new(&e.path).icon(icon).show(ui, p).clicked() {
                            chosen = Some(e.path.clone());
                        }
                    }
                    if hits.is_empty() {
                        ui.label(RichText::new("Nada encontrado.").color(p.faint));
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    chosen = hits.first().map(|e| e.path.clone());
                }
            });
        });
        if let Some(path) = chosen {
            let v = self.view(canvas);
            let at = v.w(canvas.center());
            self.insert_file(cx, &ctx, &path, at);
            return;
        }
        if keep {
            self.insert = Some(q);
        }
    }

    /// PNG of the selection (or everything) beside the quadro, drawn in the background.
    pub(super) fn export_png(&mut self, cx: &mut Cx) {
        if self.export.is_some() {
            return;
        }
        let v = cx.vault;
        // the pictures it needs, found in the vault now
        let mut paths = std::collections::HashMap::new();
        let trig = ateliet_core::quadro::layout::triggers(&self.board);
        let cards = self.board.cards.iter().chain(self.board.lanes.iter().flat_map(|l| l.cards.iter()));
        for c in cards {
            if let Some(pic) = card::parse(&c.text, &trig.0, &trig.1).picture() {
                if let Some(r) = v.resolve(pic, &self.path) {
                    paths.insert(pic.to_string(), v.abs(&r));
                }
            }
        }
        for (_, f) in &self.board.files {
            if let Some(pth) = f.vault_path() {
                if let Some(r) = v.resolve(&pth, &self.path) {
                    paths.insert(pth.clone(), v.abs(&r));
                }
            }
        }
        let only: Option<std::collections::HashSet<String>> = if self.sel.is_empty() {
            None
        } else {
            Some(self.sel.iter().map(|s| match s {
                Sel::El(i) | Sel::Card(i) | Sel::Lane(i) => i.clone(),
            }).collect())
        };
        let board = self.board.clone();
        let folder = crate::vault::parent(&self.path).to_string();
        let name = self.title();
        let base = if folder.is_empty() { name } else { format!("{folder}/{name}") };
        let rel = v.available_path(&base, "png");
        let abs = v.abs(&rel);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let db = crate::fonts::book_fonts();
            let regular = db.find("Inter", false, 400).unwrap_or(0);
            let bold = db.find("Inter", false, 600).unwrap_or(regular);
            let fonts = ateliet_core::quadro::export::Fonts { db: &db, regular, bold, mono: None };
            let mut load = |k: &str| -> Option<ateliet_core::typeset::Picture> {
                let img = image::open(paths.get(k)?).ok()?.to_rgba8();
                Some(ateliet_core::typeset::Picture { w: img.width(), h: img.height(), rgba: img.into_raw(), jpeg: None })
            };
            let r = ateliet_core::quadro::export::png(&board, &fonts, &mut load, only.as_ref(), 2.0).and_then(|png| std::fs::write(&abs, png).map_err(|e| e.to_string())).map(|_| rel);
            let _ = tx.send(r);
        });
        self.export = Some(rx);
    }
}

