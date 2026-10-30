//! What sits round the notebook, and goes away while one writes: the small
//! toolbar at the foot, the balloon by the selected item, the material
//! panels that slide in from the left, the go-to-page box and the notice of
//! an excerpt that just arrived.

use super::draw::{self, PageView};
use super::{Book, Cx, Journal, Look, Out, Panel, Payload, Tool};
use crate::icons;
use crate::kit;
use ateliet_core::capture;
use ateliet_core::journal::{self, Item, Kind, SHAPES};
use eframe::egui::{self, vec2, Color32, Rect, RichText, Sense, Stroke};
use std::time::Duration;

/// Inks for the pen, lines and type; tapes' colours.
const INKS: &[&str] = &["#26252399", "#262523", "#3a5a8c", "#b23a2e", "#3d6b48", "#7a5a3a"];
const TAPES: &[&str] = &["#e8d9a8", "#cfe0d2", "#e7c9c4", "#c9d6e8", "#d9d2c3"];
/// The sticker drawer's folder in the vault.
pub const STICKERS: &str = "Adesivos";

impl Journal {
    pub fn chrome(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &mut Cx, canvas: Rect, views: &[PageView]) {
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        // shown while the pointer moves, a tool is on or something is picked
        let recent = self.seen.elapsed() < Duration::from_millis(2200);
        let near_foot = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|h| h.y > canvas.bottom() - 90.0 && canvas.contains(h));
        let show = recent || near_foot || self.tool != Tool::Select || b.drag.is_none() && !b.sel.is_empty() || self.panel.is_some() || b.goto.is_some();
        let writing = b.editing.is_some();
        let t = ctx.animate_bool_with_time(egui::Id::new("journal-chrome"), show && !writing, 0.12);
        if t < 1.0 && show {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if recent {
            ctx.request_repaint_after(Duration::from_millis(2300));
        }
        if t > 0.01 {
            self.toolbar(ui, b, cx, canvas, t);
        }
        if b.drag.is_none() && !(self.immersive && writing) {
            self.balloon(ui, b, cx, views);
        }
        if b.look == Look::Cover {
            self.cover_bar(ui, b, cx, canvas);
        }
        self.goto_box(ui, b, canvas, p);
        self.arrived_notice(ui, b, cx, canvas, views);
        // what is being carried from a panel
        if let Some(pl) = egui::DragAndDrop::payload::<Payload>(&ctx) {
            if let Some(h) = ctx.input(|i| i.pointer.hover_pos()) {
                let label = match &*pl {
                    Payload::Picture(k) => format!("{}  {}", icons::IMAGE, k.rsplit('/').next().unwrap_or(k)),
                    Payload::Note(n) => format!("{}  {}", icons::FILE_TEXT, n.rsplit('/').next().unwrap_or(n).trim_end_matches(".md")),
                    Payload::Snippet(n) => format!("{}  {}", icons::QUOTE, n.rsplit('/').next().unwrap_or(n).trim_end_matches(".md")),
                    Payload::Board(b) => format!("{}  {}", icons::CLAPPERBOARD, b.rsplit('/').next().unwrap_or(b).trim_end_matches(".storyboard")),
                };
                egui::Area::new(egui::Id::new("journal-carry")).fixed_pos(h + vec2(14.0, 10.0)).order(egui::Order::Tooltip).interactable(false).show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(RichText::new(label).size(12.0)));
                });
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &mut Cx, canvas: Rect, t: f32) {
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        let at = egui::pos2(canvas.center().x, canvas.bottom() - 16.0);
        egui::Area::new(egui::Id::new("journal-toolbar")).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(at).order(egui::Order::Foreground).show(&ctx, |ui| {
            ui.set_opacity(t);
            egui::Frame::new().fill(p.bg).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(8, 5)).shadow(egui::epaint::Shadow { offset: [0, 3], blur: 14, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 26 }) }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if kit::icon_button(ui, icons::LIBRARY_BIG, "Estante", false, p).clicked() {
                        self.out_close(b);
                    }
                    if kit::icon_button(ui, icons::NOTEBOOK, "Capa: cor, textura, imagem e adesivos", b.look == Look::Cover, p).clicked() {
                        if b.look == Look::Cover { b.turn(true) } else { b.open_cover() }
                    }
                    sep(ui, p);
                    for (tool, glyph, tip) in [(Tool::Select, icons::MOUSE_POINTER_2, "Selecionar (V)"), (Tool::Write, icons::TYPE, "Escrever (T)"), (Tool::Pen, icons::PEN_LINE, "Caneta (P)"), (Tool::Line, icons::SLASH, "Linha (L)"), (Tool::Tape, icons::STICKY_NOTE, "Fita (F)")] {
                        if kit::icon_button(ui, glyph, tip, self.tool == tool, p).clicked() {
                            self.tool = tool;
                        }
                    }
                    if self.tool == Tool::Write {
                        let cur = b.nb.font.clone().unwrap_or_else(|| "tema".into());
                        if let Some(f) = font_menu(ui, "journal-nb-font", &cur, true, p).filter(|f| *f != cur) {
                            b.set_setting("fonte", (f != "tema").then_some(f.as_str()));
                            b.nb.font = (f != "tema").then_some(f);
                        }
                        if let Some(sz) = size_menu(ui, "journal-nb-size", b.nb.text_size, true, p) {
                            b.nb.text_size = sz;
                            let v = format!("{sz}");
                            b.set_setting("tamanho-letra", ((sz - 1.0).abs() > 1e-6).then_some(v.as_str()));
                        }
                        let wm = b.nb.wrap_to_margin;
                        let tip = if wm { "Começado no meio da linha, o texto volta à margem na linha seguinte (clique: manter onde começou)" } else { "Começado no meio da linha, o texto segue alinhado onde começou (clique: voltar à margem)" };
                        if kit::icon_button(ui, if wm { icons::CORNER_DOWN_LEFT } else { icons::ALIGN_LEFT }, tip, wm, p).clicked() {
                            b.nb.wrap_to_margin = !wm;
                            b.set_setting("quebra", Some(if wm { "bloco" } else { "margem" }));
                        }
                    }
                    if matches!(self.tool, Tool::Pen | Tool::Line) {
                        for c in INKS.iter().take(5) {
                            let col = draw::hex(&c[..7]).unwrap_or(Color32::BLACK);
                            let (r, resp) = ui.allocate_exact_size(vec2(18.0, 26.0), Sense::click());
                            ui.painter().circle_filled(r.center(), 6.0, col);
                            if self.color == col {
                                ui.painter().circle_stroke(r.center(), 8.0, Stroke::new(1.5_f32, p.accent_strong));
                            }
                            if resp.clicked() {
                                self.color = col;
                            }
                        }
                    }
                    sep(ui, p);
                    for (panel, glyph, tip) in [(Panel::Pictures, icons::IMAGES, "Imagens (I)"), (Panel::Notes, icons::FILE_TEXT, "Notas (N)"), (Panel::Snippets, icons::QUOTE, "Trechos (E)"), (Panel::Stickers, icons::STAR, "Adesivos"), (Panel::Storyboards, icons::CLAPPERBOARD, "Storyboards")] {
                        if kit::icon_button(ui, glyph, tip, self.panel == Some(panel), p).clicked() {
                            self.toggle_panel(panel);
                        }
                    }
                    if kit::icon_button(ui, icons::BOOK_OPEN, "Ler ao lado: um livro do Leitor ao lado do caderno", self.reading_beside, p).clicked() {
                        self.out.push(Out::Read(None));
                    }
                    sep(ui, p);
                    if kit::icon_button(ui, icons::CHEVRON_LEFT, "Página anterior (←)", false, p).clicked() {
                        b.turn(false);
                    }
                    let (l, r) = b.spread_pages();
                    let label = match (l, r) {
                        _ if b.look == Look::Cover => "Capa".to_string(),
                        (Some(a), Some(c)) => format!("{a}–{c} de {}", b.count()),
                        (Some(a), None) | (None, Some(a)) => format!("{a} de {}", b.count()),
                        _ => String::new(),
                    };
                    if ui.add(egui::Label::new(RichText::new(label).size(12.0).color(p.muted)).sense(Sense::click())).on_hover_text("Ir para a página (Ctrl+G)").clicked() {
                        b.goto = Some(String::new());
                    }
                    if kit::icon_button(ui, icons::CHEVRON_RIGHT, "Próxima página (→)", false, p).clicked() {
                        b.turn(true);
                    }
                    if kit::icon_button(ui, icons::FOCUS, "Modo imersivo: só a página; digitar já escreve (Ctrl/Cmd+Shift+Enter, Esc sai)", self.immersive, p).clicked() {
                        self.immersive = !self.immersive;
                    }
                    if kit::icon_button(ui, icons::LAYOUT_GRID, "Todas as páginas", b.look == Look::Thumbs, p).clicked() {
                        b.commit_edit();
                        b.look = if b.look == Look::Thumbs { Look::Spread } else { Look::Thumbs };
                    }
                    if kit::icon_button(ui, icons::PLUS, "Página nova depois desta (Ctrl/Cmd+Enter)", false, p).clicked() {
                        b.new_page_here(&mut self.out);
                    }
                    let r = kit::icon_button_sized(ui, icons::CHEVRON_UP, "Mais opções de página", false, p, 18.0, 12.0);
                    let id = ui.make_persistent_id("journal-page-menu");
                    if r.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(id));
                    }
                    egui::popup::popup_above_or_below_widget(ui, id, &r, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(230.0);
                        if kit::menu_item(ui, icons::FILE_PLUS, "Página nova no fim", p) {
                            b.commit_edit();
                            let n = b.append_page("");
                            b.go_to(n);
                            self.out.push(Out::Rescan);
                        }
                        let cur = r_or_l(b);
                        if kit::menu_item(ui, icons::CORNER_DOWN_RIGHT, &format!("Inserir depois da página {cur}"), p) {
                            b.commit_edit();
                            b.insert_after(cur, &mut self.out);
                            b.go_to(cur + 1);
                        }
                        if b.nb.daily && kit::menu_item(ui, icons::CALENDAR_DAYS, "Página de hoje", p) {
                            b.commit_edit();
                            let n = b.day_page(&super::today());
                            b.go_to(n);
                            self.out.push(Out::Rescan);
                        }
                        kit::menu_sep(ui, p);
                        if let Some(path) = b.page_ref(cur).map(|r| r.path.clone()) {
                            if kit::menu_item(ui, icons::EXTERNAL_LINK, &format!("Abrir a página {cur} como nota"), p) {
                                b.save(true, &mut self.out);
                                self.out.push(Out::Open(path));
                            }
                        }
                        if b.count() > 1 && kit::menu_danger(ui, icons::TRASH_2, &format!("Apagar a página {cur}"), p) {
                            b.commit_edit();
                            b.delete_page(cur, &mut self.out);
                        }
                    });
                });
            });
        });
    }

    fn out_close(&mut self, b: &mut Book) {
        b.commit_edit();
        b.save(true, &mut self.out);
        self.close_requested = true;
    }

    fn balloon(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &mut Cx, views: &[PageView]) {
        if b.sel.is_empty() {
            return;
        }
        let p = cx.palette;
        let Some(pv) = views.iter().find(|v| v.n == b.sel_page).cloned() else { return };
        let n = pv.n;
        let sel = b.sel.clone();
        let items: Vec<Item> = b.page(n).map(|pg| pg.items.iter().filter(|i| sel.contains(&i.id)).cloned().collect()).unwrap_or_default();
        if items.is_empty() {
            return;
        }
        let bb = items.iter().map(|i| i.bbox()).reduce(|a, c| {
            let x0 = a.x.min(c.x);
            let y0 = a.y.min(c.y);
            ateliet_core::canvas::Rect { x: x0, y: y0, w: (a.x + a.w).max(c.x + c.w) - x0, h: (a.y + a.h).max(c.y + c.h) - y0 }
        }).unwrap();
        let top = pv.s((bb.x + bb.w / 2.0, bb.y));
        let below = pv.s((bb.x + bb.w / 2.0, bb.y + bb.h));
        let (at, pivot) = if top.y > pv.rect.top() - 10.0 && top.y > 90.0 { (top - vec2(0.0, 30.0), egui::Align2::CENTER_BOTTOM) } else { (below + vec2(0.0, 14.0), egui::Align2::CENTER_TOP) };
        let one = (items.len() == 1).then(|| items[0].clone());
        let ctx = ui.ctx().clone();
        egui::Area::new(egui::Id::new("journal-balloon")).pivot(pivot).fixed_pos(at).order(egui::Order::Foreground).constrain(true).show(&ctx, |ui| {
            egui::Frame::new().fill(p.bg).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(8.0).inner_margin(egui::Margin::symmetric(6, 3)).shadow(egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(if p.dark { 80 } else { 22 }) }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    if let Some(it) = &one {
                        self.balloon_for(ui, b, cx, n, it);
                    }
                    if kit::icon_button(ui, icons::BRING_TO_FRONT, "Trazer para frente (Ctrl+])", false, p).clicked() {
                        b.layer(true, false);
                    }
                    if kit::icon_button(ui, icons::SEND_TO_BACK, "Mandar para trás (Ctrl+[)", false, p).clicked() {
                        b.layer(false, false);
                    }
                    if kit::icon_button(ui, icons::COPY_PLUS, "Duplicar (Ctrl+D)", false, p).clicked() {
                        b.duplicate_selection();
                    }
                    if let Some(it) = &one {
                        let (g, tip) = if it.locked { (icons::LOCK, "Destravar") } else { (icons::LOCK_OPEN, "Travar no lugar") };
                        if kit::icon_button(ui, g, tip, it.locked, p).clicked() {
                            edit_item(b, n, &it.id, |i| i.locked = !i.locked);
                        }
                    }
                    if kit::icon_button(ui, icons::TRASH_2, "Apagar (Delete)", false, p).clicked() {
                        b.delete_selection();
                    }
                });
            });
        });
    }

    /// The part of the balloon that depends on what the item is.
    fn balloon_for(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &mut Cx, n: usize, it: &Item) {
        let p = cx.palette;
        let id = it.id.clone();
        if b.crop.as_ref().is_some_and(|c| c.1 == id) {
            if kit::button(ui, Some(icons::CHECK), "Concluir recorte", p).clicked() {
                self.finish_crop(b);
            }
            if kit::icon_button(ui, icons::REFRESH_CW, "Imagem inteira", false, p).clicked() {
                if let Some((_, _, st)) = b.crop.as_mut() {
                    st.reset();
                }
            }
            sep(ui, p);
            return;
        }
        match &it.kind {
            Kind::Text | Kind::Excerpt => {
                if b.editing.is_none() && kit::icon_button(ui, icons::PENCIL, "Editar (Enter)", false, p).clicked() {
                    self.edit(b, n, &id, false);
                }
                let current = super::draw::effective_font(it).to_string();
                if let Some(f) = font_menu(ui, ("journal-font", &id), &current, false, p) {
                    edit_item(b, n, &id, |i| i.style.font = f);
                    b.pending_reflow = Some((n, id.clone()));
                }
                if let Some(sz) = size_menu(ui, ("journal-size", &id), it.style.size(), false, p) {
                    edit_item(b, n, &id, |i| i.style.size = if (sz - 1.0).abs() < 1e-6 { 0.0 } else { sz });
                    b.pending_reflow = Some((n, id.clone()));
                }
                // writing begun mid-line: back to the margin, or kept there
                let lay = b.layout(n);
                let (c0, c1) = lay.column_at(it.x + it.w / 2.0);
                if it.style.indent > 0.0 || it.x > c0 + 0.5 {
                    let to_margin = it.style.indent > 0.0;
                    let tip = if to_margin { "As linhas seguintes voltam à margem da coluna (clique: manter onde o texto começa)" } else { "As linhas seguintes começam onde o texto começa (clique: voltar à margem da coluna)" };
                    if kit::icon_button(ui, if to_margin { icons::CORNER_DOWN_LEFT } else { icons::ALIGN_LEFT }, tip, to_margin, p).clicked() {
                        edit_item(b, n, &id, |i| {
                            if to_margin {
                                i.x += i.style.indent;
                                i.w -= i.style.indent;
                                i.style.indent = 0.0;
                            } else {
                                i.style.indent = ((i.x - c0) * 100.0).round() / 100.0;
                                i.w = c1 - c0;
                                i.x = c0;
                            }
                        });
                        b.pending_reflow = Some((n, id.clone()));
                    }
                }
                let big = it.style.lines() > 1.0;
                if kit::icon_button(ui, icons::HEADING, if big { "Uma linha da pauta" } else { "Duas linhas da pauta (maior)" }, big, p).clicked() {
                    edit_item(b, n, &id, |i| i.style.lines = if big { 0.0 } else { 2.0 });
                    b.pending_reflow = Some((n, id.clone()));
                }
                let (g, next) = match it.style.align.as_str() {
                    "centro" => (icons::ALIGN_CENTER, "direita"),
                    "direita" => (icons::ALIGN_RIGHT, ""),
                    _ => (icons::ALIGN_LEFT, "centro"),
                };
                if kit::icon_button(ui, g, "Alinhamento", false, p).clicked() {
                    edit_item(b, n, &id, |i| i.style.align = next.to_string());
                }
                self.swatches(ui, b, n, &id, INKS, |i, c| i.style.color = c, it.style.color.clone(), p);
                if it.kind == Kind::Excerpt {
                    if let Some(loc) = self.reader_origin(cx.vault, b, n, &it) {
                        if kit::icon_button(ui, icons::BOOK_OPEN, "Abrir no Leitor, no ponto do trecho", false, p).clicked() {
                            self.out.push(Out::Read(Some((loc.file.clone(), Some(loc)))));
                        }
                    }
                }
            }
            Kind::Image | Kind::Sticker | Kind::Scan => {
                if it.kind == Kind::Image && kit::icon_button(ui, icons::CROP, "Recortar (C)", false, p).clicked() {
                    self.start_crop(b, n, &id);
                }
                let r = kit::icon_button(ui, icons::SHAPES, "Forma do recorte", it.mask.is_some(), p);
                let mid = ui.make_persistent_id(("journal-shape", &id));
                if r.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(mid));
                }
                egui::popup::popup_below_widget(ui, mid, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(180.0);
                    if kit::menu_check(ui, it.mask.is_none(), "Retângulo", p) {
                        edit_item(b, n, &id, |i| super::input::set_mask(i, None));
                    }
                    for (key, label) in SHAPES {
                        let on = it.mask.as_ref().is_some_and(|m| m.shape == *key);
                        if kit::menu_check(ui, on, label, p) {
                            edit_item(b, n, &id, |i| {
                                super::input::set_mask(i, Some(key));
                                // the round shapes look right square
                                if matches!(*key, "circulo" | "estrela" | "coracao") && (i.w - i.h).abs() > 0.5 {
                                    let s = i.w.min(i.h);
                                    let (cx0, cy0) = (i.x + i.w / 2.0, i.y + i.h / 2.0);
                                    i.w = s;
                                    i.h = s;
                                    i.x = cx0 - s / 2.0;
                                    i.y = cy0 - s / 2.0;
                                }
                            });
                        }
                    }
                });
                let sticker = it.look.border > 0.0;
                if kit::icon_button(ui, icons::STAR, if sticker { "Sem borda de adesivo" } else { "Borda de adesivo" }, sticker, p).clicked() {
                    edit_item(b, n, &id, |i| {
                        i.look.border = if sticker { 0.0 } else { 1.8 };
                        i.look.shadow = !sticker;
                    });
                }
                let r = kit::icon_button(ui, icons::SLIDERS_HORIZONTAL, "Filtro", it.look.filter.is_some(), p);
                let fid = ui.make_persistent_id(("journal-filter", &id));
                if r.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(fid));
                }
                egui::popup::popup_below_widget(ui, fid, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(190.0);
                    for (key, label) in ateliet_core::typeset::compose::FILTERS.iter().filter(|(k, _)| *k != "noite") {
                        let on = it.look.filter.as_deref().unwrap_or("") == *key;
                        if kit::menu_check(ui, on, label, p) {
                            edit_item(b, n, &id, |i| i.look.filter = (!key.is_empty()).then(|| key.to_string()));
                        }
                    }
                    kit::menu_sep(ui, p);
                    if kit::menu_check(ui, it.look.multiply, "Deixar a pauta aparecer", p) {
                        edit_item(b, n, &id, |i| i.look.multiply = !i.look.multiply);
                    }
                    if kit::menu_check(ui, it.look.shadow, "Sombra", p) {
                        edit_item(b, n, &id, |i| i.look.shadow = !i.look.shadow);
                    }
                });
                if kit::icon_button(ui, icons::FLIP_HORIZONTAL_2, "Espelhar", it.flip, p).clicked() {
                    edit_item(b, n, &id, |i| i.flip = !i.flip);
                }
                if kit::icon_button(ui, icons::TEXT, "Legenda", false, p).clicked() {
                    self.caption(b, n, it);
                }
                if let Some((board, de, ate, anim)) = super::boards::board_of(it) {
                    if it.extra.contains_key("quadro") {
                        if kit::icon_button(ui, icons::REFRESH_CW, "Desenhar de novo do storyboard", false, p).clicked() {
                            self.refresh_frame(ui.ctx(), cx, it);
                        }
                    } else {
                        if kit::icon_button(ui, icons::SCISSORS, "Trecho do storyboard: de que quadro a que quadro, ou quanto tempo", false, p).clicked() {
                            let ctx = ui.ctx().clone();
                            self.open_board_dialog(&ctx, cx, &board, anim, Some((de, ate)), super::boards::Target::Edit(n, id.clone()));
                        }
                        if anim {
                            let paused = it.extra.get("pausado").and_then(|v| v.as_bool()).unwrap_or(false);
                            if kit::icon_button(ui, if paused { icons::PLAY } else { icons::PAUSE }, if paused { "Tocar" } else { "Pausar" }, false, p).clicked() {
                                edit_item(b, n, &id, |i| {
                                    if paused {
                                        i.extra.remove("pausado");
                                    } else {
                                        i.extra.insert("pausado".into(), serde_json::json!(true));
                                    }
                                });
                            }
                        }
                        if kit::icon_button(ui, icons::REFRESH_CW, "Desenhar de novo do storyboard", false, p).clicked() {
                            self.place_board(b, cx, &board, de, ate, anim, super::boards::Target::Edit(n, id.clone()));
                        }
                    }
                }
                if kit::icon_button(ui, icons::BOOKMARK_PLUS, "Guardar como adesivo", false, p).clicked() {
                    self.save_sticker(cx, b, n, it);
                }
                if let Some(loc) = self.reader_origin(cx.vault, b, n, &it) {
                    if kit::icon_button(ui, icons::BOOK_OPEN, "Abrir no Leitor, no ponto do recorte", false, p).clicked() {
                        self.out.push(Out::Read(Some((loc.file.clone(), Some(loc)))));
                    }
                }
                if let Some(t) = it.picture() {
                    if kit::icon_button(ui, icons::EXTERNAL_LINK, &format!("Origem: {t}"), false, p).clicked() {
                        let path = b.page_ref(n).map(|r| r.path.clone()).unwrap_or_default();
                        if let Some(abs) = cx.vault.resolve(&t, &path).map(|r| cx.vault.abs(&r)) {
                            let _ = open::that(abs);
                        }
                    }
                }
            }
            Kind::Note => {
                if let Some(t) = journal::embed_target(it.content()) {
                    if kit::icon_button(ui, icons::EXTERNAL_LINK, "Abrir a nota", false, p).clicked() {
                        self.out.push(Out::Open(t.split('#').next().unwrap_or("").to_string()));
                    }
                }
            }
            Kind::Tape => self.swatches(ui, b, n, &id, TAPES, |i, c| i.color = c, it.color.clone(), p),
            Kind::Ink | Kind::Line => {
                self.swatches(ui, b, n, &id, INKS, |i, c| {
                    i.color = c.clone();
                    for s in i.strokes.iter_mut() {
                        s.color = c.clone();
                    }
                }, it.color.clone().or_else(|| it.strokes.first().and_then(|s| s.color.clone())), p);
                if it.kind == Kind::Line {
                    let thick = it.style.lines() > 1.0;
                    if kit::icon_button(ui, icons::MINUS, if thick { "Traço fino" } else { "Traço grosso" }, thick, p).clicked() {
                        edit_item(b, n, &id, |i| i.style.lines = if thick { 0.0 } else { 3.0 });
                    }
                }
            }
            Kind::Other(_) => {}
        }
        if it.r != 0.0 && kit::icon_button(ui, icons::ROTATE_CCW, "Endireitar", false, p).clicked() {
            edit_item(b, n, &id, |i| i.r = 0.0);
        }
        sep(ui, p);
    }

    #[allow(clippy::too_many_arguments)]
    fn swatches(&mut self, ui: &mut egui::Ui, b: &mut Book, n: usize, id: &str, list: &[&str], set: impl Fn(&mut Item, Option<String>), now: Option<String>, p: crate::theme::Palette) {
        for (k, c) in list.iter().enumerate() {
            let hex = &c[..7];
            let col = draw::hex(hex).unwrap_or(Color32::BLACK);
            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 24.0), Sense::click());
            ui.painter().circle_filled(r.center(), 5.5, col);
            let on = now.as_deref() == Some(hex) || (k == 0 && now.is_none());
            if on {
                ui.painter().circle_stroke(r.center(), 7.5, Stroke::new(1.3_f32, p.accent_strong));
            }
            if resp.clicked() {
                let v = if k == 0 && list == INKS { None } else { Some(hex.to_string()) };
                edit_item(b, n, id, |i| set(i, v.clone()));
            }
        }
    }

    /// A caption under a picture, attached to it (it moves with it).
    fn caption(&mut self, b: &mut Book, n: usize, it: &Item) {
        let lay = b.layout(n);
        let bb = it.bbox();
        let y = lay.text_top(bb.y + bb.h + 0.5, 1.0);
        let mut c = Item::new(Kind::Text, bb.x, y, bb.w.max(30.0), lay.paper.line());
        c.anchor = Some(it.id.clone());
        c.style.font = "mao".into();
        let cap = it.caption();
        c.set_content(&cap);
        let id = c.id.clone();
        b.checkpoint();
        if let Some(p) = b.page(n) {
            p.items.push(c);
        }
        b.touch(n);
        self.edit(b, n, &id, true);
    }

    /// The picture cut out as it shows, kept in the drawer (`Adesivos/`).
    fn save_sticker(&mut self, cx: &mut Cx, b: &mut Book, n: usize, it: &Item) {
        let path = b.page_ref(n).map(|r| r.path.clone()).unwrap_or_default();
        let Some(src) = it.picture().and_then(|t| cx.vault.resolve(&t, &path)) else { return };
        let Ok(img) = image::open(cx.vault.abs(&src)) else {
            self.out.push(Out::Toast("Não foi possível ler a imagem.".into()));
            return;
        };
        let cut = journal::cut::cut(&img.to_rgba8(), it.crop, it.flip, it.mask.as_ref(), 1600);
        let dir = cx.vault.root.join(STICKERS);
        let _ = std::fs::create_dir_all(&dir);
        let stem = src.rsplit('/').next().unwrap_or("adesivo").rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_else(|| "adesivo".into());
        let name = capture::free_name(&dir, &format!("{stem}.png"));
        match cut.save(dir.join(&name)) {
            Ok(()) => {
                let rel = format!("{STICKERS}/{name}");
                if let Some(cfg) = &cx.config_dir {
                    save_sticker_mask(cfg, &rel, it.mask.as_ref());
                }
                self.out.push(Out::Rescan);
                self.out.push(Out::Toast(format!("Adesivo guardado em {rel}.")));
            }
            Err(e) => self.out.push(Out::Toast(format!("Não foi possível guardar o adesivo: {e}"))),
        }
    }

    fn goto_box(&mut self, ui: &mut egui::Ui, b: &mut Book, canvas: Rect, p: crate::theme::Palette) {
        let Some(mut text) = b.goto.take() else { return };
        let ctx = ui.ctx().clone();
        let mut done = false;
        let mut keep = true;
        egui::Area::new(egui::Id::new("journal-goto")).pivot(egui::Align2::CENTER_TOP).fixed_pos(canvas.center_top() + vec2(0.0, 24.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ir para a página").size(12.5).color(p.muted));
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(60.0));
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        done = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        keep = false;
                    }
                });
            });
        });
        if done {
            if let Ok(n) = text.trim().parse::<usize>() {
                b.go_to(n);
            }
            keep = false;
        }
        if keep {
            b.goto = Some(text);
        }
    }

    fn arrived_notice(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &mut Cx, canvas: Rect, _views: &[PageView]) {
        let Some((note, s)) = self.arrived.clone() else { return };
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        egui::Area::new(egui::Id::new("journal-arrived")).pivot(egui::Align2::RIGHT_TOP).fixed_pos(canvas.right_top() + vec2(-16.0, 16.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.label(RichText::new(format!("{}  Trecho recebido", icons::INBOX)).font(kit::medium(12.5)).color(p.text));
                let preview: String = if s.text.is_empty() { s.source.line() } else { s.text.chars().take(140).collect() };
                ui.label(RichText::new(preview).size(12.0).color(p.muted));
                ui.horizontal(|ui| {
                    if kit::primary(ui, "Pôr na página aberta", p).clicked() {
                        let n = r_or_l(b);
                        let lay = b.layout(n);
                        self.place_snippet(b, n, &lay, None, &note, &s);
                        self.arrived = None;
                    }
                    if kit::button(ui, None, "Guardar", p).clicked() {
                        self.arrived = None;
                    }
                });
            });
        });
    }

    // ── material ──────────────────────────────────────────────────────────

    pub fn material(&mut self, ui: &mut egui::Ui, cx: &mut Cx, panel: &mut Option<Panel>) {
        let p = cx.palette;
        let Some(which) = *panel else { return };
        ui.horizontal(|ui| {
            let items = [(icons::IMAGES, ""), (icons::FILE_TEXT, ""), (icons::QUOTE, ""), (icons::STAR, ""), (icons::CLAPPERBOARD, "")];
            let sel = match which {
                Panel::Pictures => 0,
                Panel::Notes => 1,
                Panel::Snippets => 2,
                Panel::Stickers => 3,
                Panel::Storyboards => 4,
            };
            if let Some(k) = kit::segmented(ui, &items, sel, p) {
                *panel = Some([Panel::Pictures, Panel::Notes, Panel::Snippets, Panel::Stickers, Panel::Storyboards][k]);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, icons::X, "Fechar", false, p).clicked() {
                    *panel = None;
                }
            });
        });
        ui.add_space(8.0);
        let title = ["Imagens", "Notas", "Trechos", "Adesivos", "Storyboards"][match which {
            Panel::Pictures => 0,
            Panel::Notes => 1,
            Panel::Snippets => 2,
            Panel::Stickers => 3,
            Panel::Storyboards => 4,
        }];
        ui.label(RichText::new(title).font(kit::medium(14.0)).color(p.text));
        ui.add_space(4.0);
        match which {
            Panel::Pictures => {
                ui.label(RichText::new("Arraste uma imagem para a página.").size(11.5).color(p.faint));
                ui.add_space(4.0);
                cx.images.material_ui(ui, p);
            }
            Panel::Notes => self.notes_panel(ui, cx),
            Panel::Snippets => self.snippets_panel(ui, cx),
            Panel::Stickers => self.stickers_panel(ui, cx),
            Panel::Storyboards => self.boards_panel(ui, cx),
        }
    }

    fn notes_panel(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let p = cx.palette;
        kit::search_field(ui, &mut self.query, "Buscar notas", p);
        ui.add_space(4.0);
        ui.label(RichText::new("Arraste para a página: a nota aparece ao vivo. Com Alt, o texto é copiado para a pauta.").size(11.5).color(p.faint));
        ui.add_space(6.0);
        let q = self.query.to_lowercase();
        // not the notebook's own pages
        let own = self.book.as_ref().map(|b| format!("{}/", b.nb.folder)).filter(|f| f.len() > 1);
        let mut notes: Vec<(&str, u64)> = cx.vault.markdown_files().filter(|e| q.is_empty() || e.path.to_lowercase().contains(&q)).filter(|e| !e.path.starts_with(capture::FOLDER) && own.as_ref().is_none_or(|o| !e.path.starts_with(o.as_str()))).map(|e| (e.path.as_str(), e.mtime)).collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n.1));
        egui::ScrollArea::vertical().id_salt("journal-notes").auto_shrink([false, false]).show(ui, |ui| {
            for (path, _) in notes.iter().take(300) {
                let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md");
                let folder = path.rsplit_once('/').map(|(f, _)| f).unwrap_or("");
                let r = row(ui, icons::FILE_TEXT, name, folder, p);
                if r.drag_started() {
                    r.dnd_set_drag_payload(Payload::Note(path.to_string()));
                }
                if r.double_clicked() {
                    self.out.push(Out::Open(path.to_string()));
                }
            }
        });
    }

    fn snippets_panel(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let p = cx.palette;
        kit::search_field(ui, &mut self.query, "Buscar trechos", p);
        ui.add_space(4.0);
        ui.label(RichText::new(format!("Os trechos do Leitor, da caixa de entrada e de “Mandar ao Journal”, guardados em {}/.", capture::FOLDER)).size(11.5).color(p.faint));
        ui.add_space(6.0);
        let prefix = format!("{}/", capture::FOLDER);
        let mut list: Vec<(String, u64)> = cx.vault.markdown_files().filter(|e| e.path.starts_with(&prefix)).map(|e| (e.path.clone(), e.mtime)).collect();
        list.sort_by_key(|n| std::cmp::Reverse(n.1));
        let used: std::collections::HashSet<String> = cx.vault.index.read().map(|ix| list.iter().filter(|(path, _)| !ix.inlinks(path).is_empty()).map(|(p, _)| p.clone()).collect()).unwrap_or_default();
        let q = self.query.to_lowercase();
        let mut put: Option<(String, capture::Snippet)> = None;
        egui::ScrollArea::vertical().id_salt("journal-snippets").auto_shrink([false, false]).show(ui, |ui| {
            if list.is_empty() {
                ui.label(RichText::new("Nenhum trecho ainda.").size(12.5).color(p.faint));
            }
            for (path, _) in list.iter().take(300) {
                let Some(s) = self.snippet(cx.vault, path) else { continue };
                if !q.is_empty() && !s.text.to_lowercase().contains(&q) && !s.source.line().to_lowercase().contains(&q) {
                    continue;
                }
                egui::Frame::new().fill(p.bg).corner_radius(6.0).stroke(Stroke::new(1.0_f32, p.border)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let head = if s.source.line().is_empty() { "Sem fonte".to_string() } else { s.source.line() };
                    // the source and the text are the handle to drag it by
                    let mut grips = Vec::new();
                    ui.horizontal(|ui| {
                        grips.push(ui.add(egui::Label::new(RichText::new(head).font(kit::medium(12.0)).color(p.text)).sense(Sense::drag())));
                        if used.contains(path) {
                            ui.label(RichText::new("usado").size(10.5).color(p.faint));
                        }
                    });
                    let preview: String = if s.text.is_empty() { s.image.clone().unwrap_or_default() } else { s.text.chars().take(220).collect() };
                    grips.push(ui.add(egui::Label::new(RichText::new(preview).size(12.0).color(p.muted)).sense(Sense::drag())));
                    for g in grips {
                        if g.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if g.drag_started() {
                            g.dnd_set_drag_payload(Payload::Snippet(path.clone()));
                        }
                    }
                    ui.horizontal(|ui| {
                        if kit::quiet(ui, Some(icons::CORNER_DOWN_RIGHT), "Pôr na página", p).clicked() {
                            put = Some((path.clone(), s.clone()));
                        }
                        if kit::quiet(ui, Some(icons::EXTERNAL_LINK), "Abrir", p).clicked() {
                            self.out.push(Out::Open(path.clone()));
                        }
                    });
                });
                ui.add_space(6.0);
            }
        });
        if let Some((path, s)) = put {
            if let Some(mut b) = self.book.take() {
                let n = r_or_l(&b);
                let lay = b.layout(n);
                self.place_snippet(&mut b, n, &lay, None, &path, &s);
                self.book = Some(b);
            }
        }
    }

    fn stickers_panel(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let p = cx.palette;
        ui.label(RichText::new(format!("Recortes guardados com “Guardar como adesivo”, em {STICKERS}/. Arraste para a página.")).size(11.5).color(p.faint));
        ui.add_space(6.0);
        let prefix = format!("{STICKERS}/");
        let list: Vec<String> = cx.vault.files.iter().filter(|e| !e.folder && e.path.starts_with(&prefix) && ateliet_core::images::is_image(&e.path)).map(|e| e.path.clone()).collect();
        if list.is_empty() {
            ui.label(RichText::new("A gaveta está vazia.").size(12.5).color(p.faint));
            return;
        }
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().id_salt("journal-stickers").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for path in &list {
                    let (r, resp) = ui.allocate_exact_size(vec2(80.0, 80.0), Sense::click_and_drag());
                    ui.painter().rect_filled(r, 6.0, p.bg_hover);
                    if let Some((t, s)) = crate::host::image(&ctx, cx.vault, cx.caches, path, "") {
                        let k = (r.width() - 8.0) / s.x.max(s.y);
                        ui.painter().image(t, Rect::from_center_size(r.center(), s * k), Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    if resp.drag_started() {
                        resp.dnd_set_drag_payload(Payload::Picture(path.clone()));
                    }
                    resp.on_hover_text(path.rsplit('/').next().unwrap_or(path));
                }
            });
        });
    }
}

/// The page an action without a place goes to: the right one shown (or
/// the left).
/// The sizes of type, within the line of the ruling.
pub const SIZES: &[(f64, &str)] = &[(0.8, "Pequena"), (1.0, "Normal"), (1.2, "Média"), (1.45, "Grande")];

/// A menu of the sizes; the size picked, if one was.
pub fn size_menu(ui: &mut egui::Ui, salt: impl std::hash::Hash, current: f64, above: bool, p: crate::theme::Palette) -> Option<f64> {
    let name = SIZES.iter().find(|s| (s.0 - current).abs() < 0.01).map(|s| s.1.to_string()).unwrap_or_else(|| format!("{:.0}%", current * 100.0));
    let r = kit::chip(ui, Some(icons::HEADING), &name, false, p).on_hover_text("Tamanho da letra (Ctrl/Cmd+= e Ctrl/Cmd+- ao escrever)");
    let id = ui.make_persistent_id(salt);
    if r.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    let mut out = None;
    let side = if above { egui::AboveOrBelow::Above } else { egui::AboveOrBelow::Below };
    egui::popup::popup_above_or_below_widget(ui, id, &r, side, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(150.0);
        for (v, label) in SIZES {
            let on = (v - current).abs() < 0.01;
            let text = RichText::new(*label).size(13.0 * *v as f32).color(if on { p.accent_strong } else { p.text });
            if ui.add(egui::Button::new(text).frame(false).min_size(vec2(140.0, 26.0))).clicked() {
                out = Some(*v);
            }
        }
    });
    out
}

/// A menu of the Journal's fonts, each name in its own face; the font
/// picked, if one was.
pub fn font_menu(ui: &mut egui::Ui, salt: impl std::hash::Hash, current: &str, above: bool, p: crate::theme::Palette) -> Option<String> {
    let name = crate::fonts::JOURNAL_FONTS.iter().find(|f| f.0 == current).map(|f| f.1).unwrap_or("Fonte do tema");
    let short: String = name.split(" (").next().unwrap_or(name).to_string();
    let r = kit::chip(ui, Some(icons::TYPE), &short, false, p);
    let id = ui.make_persistent_id(salt);
    if r.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    let mut out = None;
    let side = if above { egui::AboveOrBelow::Above } else { egui::AboveOrBelow::Below };
    egui::popup::popup_above_or_below_widget(ui, id, &r, side, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(230.0);
        for (key, label, k) in crate::fonts::JOURNAL_FONTS {
            let probe = Item { style: ateliet_core::journal::TextStyle { font: key.to_string(), ..Default::default() }, ..Item::new(Kind::Text, 0.0, 0.0, 1.0, 1.0) };
            let (fam, ..) = super::draw::font_of(&probe);
            let on = *key == current;
            let text = RichText::new(*label).font(egui::FontId::new(15.0 * k / 0.6, fam)).color(if on { p.accent_strong } else { p.text });
            if ui.add(egui::Button::new(text).frame(false).min_size(vec2(220.0, 26.0))).clicked() {
                out = Some(key.to_string());
            }
        }
    });
    out
}

impl Journal {
    /// Over the cover: its colour, its texture, a picture to fill it (or
    /// the picture picked on it), and the way into the notebook.
    fn cover_bar(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &mut Cx, canvas: Rect) {
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        egui::Area::new(egui::Id::new("journal-cover-bar")).pivot(egui::Align2::CENTER_TOP).fixed_pos(canvas.center_top() + vec2(0.0, 12.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
            egui::Frame::new().fill(p.bg).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("Capa").font(kit::medium(13.0)).color(p.text));
                    sep(ui, p);
                    for c in ateliet_core::journal::format::COVERS {
                        let col = draw::hex(c).unwrap_or(Color32::GRAY);
                        let (r, resp) = ui.allocate_exact_size(vec2(18.0, 26.0), Sense::click());
                        ui.painter().rect_filled(r.shrink2(vec2(2.0, 5.0)), 2.0, col);
                        if b.nb.cover == *c {
                            ui.painter().rect_stroke(r.shrink2(vec2(0.0, 3.0)), 3.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Inside);
                        }
                        if resp.on_hover_text("Cor da capa").clicked() {
                            b.nb.cover = c.to_string();
                            b.set_setting("capa", Some(c));
                        }
                    }
                    sep(ui, p);
                    let cur = b.nb.cover_texture.clone().unwrap_or_default();
                    let name = ateliet_core::journal::texture::TEXTURES.iter().find(|t| t.0 == cur).map(|t| t.1).unwrap_or("Lisa");
                    let r = kit::chip(ui, Some(icons::LAYERS), name, false, p);
                    let id = ui.make_persistent_id("journal-cover-texture");
                    if r.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(id));
                    }
                    egui::popup::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(170.0);
                        for (key, label) in ateliet_core::journal::texture::TEXTURES {
                            if kit::menu_check(ui, cur == *key, label, p) {
                                b.nb.cover_texture = (!key.is_empty()).then(|| key.to_string());
                                b.set_setting("capa-textura", (!key.is_empty()).then_some(*key));
                            }
                        }
                    });
                    // a picture picked on the cover becomes its background
                    let one = (b.sel.len() == 1).then(|| b.sel[0].clone());
                    let picked = one.and_then(|sid| b.page(0).and_then(|pg| pg.item(&sid)).filter(|i| i.kind == Kind::Image).and_then(|i| i.picture().map(|t| (i.id.clone(), t))));
                    if let Some((iid, target)) = picked {
                        if kit::quiet(ui, Some(icons::IMAGE), "Usar como fundo", p).clicked() {
                            b.checkpoint();
                            if let Some(pg) = b.page(0) {
                                pg.items.retain(|i| i.id != iid);
                            }
                            b.touch(0);
                            b.sel.clear();
                            b.nb.cover_image = Some(target.clone());
                            b.set_setting("capa-imagem", Some(&format!("[[{target}]]")));
                        }
                    } else if kit::quiet(ui, Some(icons::IMAGE), "Imagem de fundo…", p).clicked() {
                        if let Some(f) = rfd::FileDialog::new().set_directory(&cx.vault.root).add_filter("Imagens", &["png", "jpg", "jpeg", "webp", "gif"]).pick_file() {
                            let rel = super::input::copy_into_vault(cx.vault, &cx.attach, &f.to_string_lossy());
                            b.nb.cover_image = Some(rel.clone());
                            b.set_setting("capa-imagem", Some(&format!("[[{rel}]]")));
                            self.out.push(Out::Rescan);
                        }
                    }
                    if b.nb.cover_image.is_some() && kit::icon_button(ui, icons::X, "Tirar a imagem de fundo", false, p).clicked() {
                        b.nb.cover_image = None;
                        b.set_setting("capa-imagem", None);
                    }
                    sep(ui, p);
                    if kit::quiet(ui, Some(icons::BOOK_OPEN), "Abrir o caderno", p).clicked() {
                        b.turn(true);
                    }
                });
            });
        });
    }
}

fn r_or_l(b: &Book) -> usize {
    let (l, r) = b.spread_pages();
    r.or(l).unwrap_or(1)
}

/// Change one item (a step to undo).
pub fn edit_item(b: &mut Book, n: usize, id: &str, f: impl FnOnce(&mut Item)) {
    b.checkpoint();
    if let Some(it) = b.page(n).and_then(|p| p.item_mut(id)) {
        f(it);
    }
    b.touch(n);
}

fn sep(ui: &mut egui::Ui, p: crate::theme::Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0_f32, p.border));
}

/// A row of a panel list: icon, name, a faint second part; draggable.
fn row(ui: &mut egui::Ui, glyph: &str, name: &str, faint: &str, p: crate::theme::Palette) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click_and_drag());
    if resp.hovered() {
        ui.painter().rect_filled(r, 5.0, p.bg_hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    ui.painter().text(r.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, glyph, kit::icon_font(13.0), p.faint);
    let g = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(13.0), p.text);
    let w = g.size().x;
    ui.painter().galley(r.left_center() + vec2(26.0, -g.size().y / 2.0), g, p.text);
    if !faint.is_empty() && 26.0 + w + 12.0 < r.width() {
        let clip = ui.painter().with_clip_rect(r);
        clip.text(r.left_center() + vec2(26.0 + w + 8.0, 0.0), egui::Align2::LEFT_CENTER, faint, egui::FontId::proportional(11.5), p.faint);
    }
    resp
}

/// Stickers' cuts, kept beside the vault's config: the border follows them.
fn masks_file(cfg: &std::path::Path) -> std::path::PathBuf {
    cfg.join("ateliet").join("journal").join("adesivos.json")
}

fn save_sticker_mask(cfg: &std::path::Path, rel: &str, mask: Option<&journal::Mask>) {
    let f = masks_file(cfg);
    let mut v: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    match mask {
        Some(m) => {
            v.insert(rel.to_string(), serde_json::to_value(m).unwrap_or_default());
        }
        None => {
            v.remove(rel);
        }
    }
    let _ = ateliet_core::quadro::write_atomic(&f, &serde_json::Value::Object(v).to_string());
}

/// The cut a sticker of the drawer was made with.
pub fn sticker_mask(cfg: &std::path::Path, rel: &str) -> Option<journal::Mask> {
    let t = std::fs::read_to_string(masks_file(cfg)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&t).ok()?;
    serde_json::from_value(v.get(rel)?.clone()).ok()
}
