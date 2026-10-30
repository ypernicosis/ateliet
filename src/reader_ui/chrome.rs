//! The frame that hides: a thin bar at the top when the pointer goes
//! there (the book, the contents, search, highlights, appearance, the cut
//! tool, reading beside the Journal), a 1 px line of progress at the
//! foot; the panels that slide over the page and go when an item is
//! chosen; the small menus over a selection and over a cut.

use super::{mark_swatch, Book, Cx, Out, Panel, Reader, Tool};
use crate::{icons, kit};
use ateliet_core::reader::marks::COLORS;
use ateliet_core::reader::{Format, Locator};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Stroke};
use std::time::Duration;

const BAR: f32 = 40.0;

impl Reader {
    /// The top bar; true when the book is to be closed.
    pub(super) fn chrome_bar(&mut self, ui: &mut egui::Ui, b: &mut Book, cx: &Cx, full: Rect, always: bool) -> bool {
        let p = cx.palette;
        let ctx = ui.ctx().clone();
        let hover = ctx.input(|i| i.pointer.hover_pos());
        let near_top = hover.is_some_and(|q| full.contains(q) && q.y < full.top() + BAR + 24.0);
        let show = always || near_top || b.panel.is_some() || b.tool == Tool::Crop || (!self.immersive && b.seen.elapsed() < Duration::from_millis(1600) && hover.is_some_and(|q| q.y < full.top() + 160.0));
        let t = ctx.animate_bool_with_time(egui::Id::new(("leitor-bar", &b.file, self.side)), show, 0.18);
        let mut close = false;
        if t > 0.01 {
            let bar = Rect::from_min_size(full.min - vec2(0.0, (1.0 - t) * BAR), vec2(full.width(), BAR));
            let (paper, ink) = b.prefs.colors(p);
            ui.painter().rect_filled(bar, 0.0, paper.gamma_multiply(0.97));
            ui.painter().hline(bar.x_range(), bar.bottom(), Stroke::new(1.0_f32, ink.gamma_multiply(0.08)));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 5.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
            let ui = &mut child;
            let (glyph, tip) = if self.side { (icons::X, "Fechar o leitor ao lado") } else { (icons::LIBRARY, "Estante") };
            if kit::icon_button(ui, glyph, tip, false, p).clicked() {
                close = true;
            }
            let meta = b.opened.as_ref().map(|o| o.meta.clone()).unwrap_or_default();
            let mut title = RichText::new(if meta.title.is_empty() { crate::vault::basename(&b.file).to_string() } else { meta.title.clone() }).font(kit::medium(13.0)).color(ink);
            if b.error.is_some() {
                title = title.color(ink.gamma_multiply(0.6));
            }
            ui.add(egui::Label::new(title).truncate());
            if !meta.author.is_empty() && ui.available_width() > 420.0 {
                ui.label(RichText::new(&meta.author).size(12.0).color(ink.gamma_multiply(0.55)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if b.error.is_some() {
                    return;
                }
                if !self.side {
                    let (g, tip) = if self.immersive { (icons::MINIMIZE, "Sair do modo imersivo (Esc)") } else { (icons::MAXIMIZE, "Só a página (F)") };
                    if kit::icon_button(ui, g, tip, self.immersive, p).clicked() {
                        self.immersive = !self.immersive;
                    }
                    if kit::icon_button(ui, icons::NOTEBOOK_PEN, "Ler ao lado do Journal", false, p).clicked() {
                        let mut at = b.info(b.chap, b.page).map(|i| i.loc.clone());
                        if let Some(l) = at.as_mut() {
                            l.file = b.file.clone();
                            if b.reflows() {
                                l.chap = Some(b.chap);
                            }
                        }
                        self.out.push(Out::Beside { file: b.file.clone(), at });
                    }
                }
                if kit::icon_button(ui, icons::CROP, "Recortar uma região (R)", b.tool == Tool::Crop, p).clicked() {
                    b.tool = if b.tool == Tool::Crop { Tool::Read } else { Tool::Crop };
                    b.cut = None;
                }
                for (panel, g, tip) in [(Panel::Look, icons::TYPE, "Aparência"), (Panel::Marks, icons::HIGHLIGHTER, "Grifos"), (Panel::Search, icons::SEARCH, "Buscar (Ctrl/Cmd+F)"), (Panel::Toc, icons::LIST_TREE, "Sumário (T)")] {
                    if kit::icon_button(ui, g, tip, b.panel == Some(panel), p).clicked() {
                        b.panel = if b.panel == Some(panel) { None } else { Some(panel) };
                    }
                }
            });
        }
        if b.error.is_none() && b.opened.is_some() {
            self.progress_line(ui, b, cx, full);
        }
        close
    }

    fn progress_line(&self, ui: &mut egui::Ui, b: &Book, cx: &Cx, full: Rect) {
        let p = cx.palette;
        let (_, ink) = b.prefs.colors(p);
        let y = full.bottom() - 1.0;
        let x1 = full.left() + full.width() * b.progress();
        ui.painter().hline(full.left()..=full.right(), y, Stroke::new(1.0_f32, ink.gamma_multiply(0.06)));
        ui.painter().hline(full.left()..=x1, y, Stroke::new(1.0_f32, ink.gamma_multiply(0.28)));
        let hover = ui.ctx().input(|i| i.pointer.hover_pos()).is_some_and(|q| full.contains(q) && q.y > full.bottom() - 26.0);
        if hover {
            let mut parts = Vec::new();
            if b.reflows() {
                parts.push(b.place_label(b.chap, b.page));
                if let Some(n) = b.count(b.chap) {
                    let left = n.saturating_sub(b.page + b.step());
                    parts.push(if left == 0 { "fim do capítulo".into() } else { format!("{left} {} até o fim do capítulo", if left == 1 { "página" } else { "páginas" }) });
                }
            } else if let Some(n) = b.count(b.chap) {
                parts.push(format!("página {} de {n}", b.page + 1));
            }
            parts.push(format!("{:.0}%", b.progress() * 100.0));
            ui.painter().text(pos2(full.center().x, full.bottom() - 12.0), egui::Align2::CENTER_CENTER, parts.join("  ·  "), egui::FontId::proportional(11.5), ink.gamma_multiply(0.55));
        }
    }
}

impl Book {
    /// Panels, menus and balloons over the page.
    pub(super) fn overlays(&mut self, ui: &mut egui::Ui, cx: &Cx, out: &mut Vec<Out>) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        if let Some(panel) = self.panel {
            let pos = pos2(full.right() - 12.0, full.top() + BAR + 6.0);
            let mut keep = true;
            egui::Area::new(egui::Id::new(("leitor-panel", &self.file))).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos).order(egui::Order::Foreground).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(330.0);
                    ui.set_max_height((full.height() - BAR - 40.0).max(200.0));
                    match panel {
                        Panel::Toc => self.toc_panel(ui, cx, &mut keep),
                        Panel::Search => self.search_panel(ui, cx, &mut keep),
                        Panel::Marks => self.marks_panel(ui, cx, out, &mut keep),
                        Panel::Look => self.look_panel(ui, cx),
                    }
                });
            });
            if !keep {
                self.panel = None;
            }
        }
        self.selection_menu(ui, cx, out);
        self.cut_menu(ui, cx, out);
        self.note_editor(ui, cx, out);
        // a footnote, in a balloon
        if let Some((at, text)) = self.popup.clone() {
            egui::Area::new(egui::Id::new(("leitor-note", &self.file))).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(at - vec2(0.0, 10.0)).order(egui::Order::Foreground).constrain(true).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_max_width(380.0);
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.label(RichText::new(text).size(13.0).color(cx.palette.text));
                    });
                });
            });
        }
        // a picture of the book: send it to the Journal
        let menu_id = egui::Id::new("leitor-img-menu");
        if let Some((at, name)) = ctx.memory(|m| m.data.get_temp::<(egui::Pos2, String)>(menu_id)) {
            let mut done = false;
            let r = egui::Area::new(egui::Id::new(("leitor-img", &self.file))).fixed_pos(at).order(egui::Order::Foreground).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    if kit::menu_item(ui, icons::NOTEBOOK_PEN, "Mandar a imagem ao Journal", cx.palette) {
                        let id = self.id();
                        self.pending.insert(id, super::Pend::Picture { name: name.clone() });
                        let _ = self.w.tx.send(super::worker::Req::Resource { name: name.clone(), id });
                        done = true;
                    }
                });
            });
            if done || ctx.input(|i| i.pointer.any_pressed()) && !r.response.contains_pointer() {
                ctx.memory_mut(|m| m.data.remove::<(egui::Pos2, String)>(menu_id));
            }
        }
    }

    fn toc_panel(&mut self, ui: &mut egui::Ui, cx: &Cx, keep: &mut bool) {
        let p = cx.palette;
        let toc = self.opened.as_ref().map(|o| o.toc.clone()).unwrap_or_default();
        kit::section(ui, "Sumário", p);
        if toc.is_empty() {
            ui.label(RichText::new(if self.reflows() { "O livro não traz sumário." } else { "O documento não tem marcadores." }).size(12.0).color(p.faint));
            if !self.reflows() {
                if let Some(n) = self.count(0) {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Ir à página").size(12.0).color(p.muted));
                        let id = egui::Id::new(("leitor-goto", &self.file));
                        let mut s: String = ui.memory(|m| m.data.get_temp(id)).unwrap_or_default();
                        let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(60.0));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if let Ok(k) = s.trim().parse::<usize>() {
                                self.set_page(0, k.clamp(1, n) - 1);
                                *keep = false;
                            }
                        }
                        ui.memory_mut(|m| m.data.insert_temp(id, s));
                        ui.label(RichText::new(format!("de {n}")).size(12.0).color(p.faint));
                    });
                }
            }
            return;
        }
        // the entry for where the reading is
        let here = self.info(self.chap, self.page).and_then(|i| i.loc.offset);
        let current = toc.iter().rposition(|e| e.chap < self.chap || e.chap == self.chap && (e.offset.unwrap_or(0) <= here.unwrap_or(0)) && (self.reflows() || e.page.unwrap_or(0) <= self.page));
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            for (i, e) in toc.iter().enumerate() {
                let r = kit::Row::new(&e.title).depth(e.level).selected(Some(i) == current).show(ui, p);
                if r.clicked() {
                    if self.reflows() {
                        self.go(Locator { chap: Some(e.chap), offset: e.offset, ..Default::default() });
                        if e.offset.is_none() {
                            self.set_page(e.chap, 0);
                        }
                    } else if let Some(pg) = e.page {
                        self.set_page(0, pg);
                    }
                    *keep = false;
                }
            }
        });
    }

    fn search_panel(&mut self, ui: &mut egui::Ui, cx: &Cx, keep: &mut bool) {
        let p = cx.palette;
        kit::section(ui, "Buscar no livro", p);
        let r = kit::search_field(ui, &mut self.query, "Palavras…", p);
        if !r.has_focus() && self.hits.is_empty() && !self.searching && self.query.is_empty() {
            r.request_focus();
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && self.query.trim().chars().count() >= 2 {
            self.search_id = self.id();
            self.searching = true;
            self.hits.clear();
            let _ = self.w.tx.send(super::worker::Req::Search { q: self.query.trim().to_string(), view: self.view.clone(), id: self.search_id });
        }
        ui.add_space(6.0);
        if self.searching {
            ui.label(RichText::new("Buscando…").size(12.0).color(p.faint));
            return;
        }
        if self.hits.is_empty() {
            if self.search_id > 0 && !self.query.is_empty() {
                ui.label(RichText::new("Nada encontrado.").size(12.0).color(p.faint));
            }
            return;
        }
        ui.label(RichText::new(format!("{} {}", self.hits.len(), if self.hits.len() == 1 { "ocorrência" } else { "ocorrências" })).size(11.5).color(p.faint));
        let hits = self.hits.clone();
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            for h in hits {
                let label = match h.page {
                    Some(pg) => format!("p. {}", pg + 1),
                    None => self.place_label(h.chap, 0),
                };
                let r = ui.add(egui::Button::new(RichText::new(format!("{label}\n{}", h.context)).size(12.0).color(p.text)).frame(false).wrap());
                if r.clicked() {
                    match h.page {
                        Some(pg) => self.set_page(h.chap, pg),
                        None => {
                            self.go(Locator { chap: Some(h.chap), offset: h.offset, ..Default::default() });
                        }
                    }
                }
                ui.add_space(4.0);
            }
        });
        let _ = keep;
    }

    fn marks_panel(&mut self, ui: &mut egui::Ui, cx: &Cx, out: &mut Vec<Out>, keep: &mut bool) {
        let p = cx.palette;
        ui.horizontal(|ui| {
            kit::section(ui, "Grifos", p);
            if let Some(n) = self.note.clone() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::icon_button(ui, icons::FILE_TEXT, &format!("Abrir a nota do livro ({n})"), false, p).clicked() {
                        out.push(Out::Open(n.clone()));
                        *keep = false;
                    }
                });
            }
        });
        if self.marks.is_empty() {
            ui.label(RichText::new("Selecione um trecho e escolha uma cor para grifar. Os grifos ficam na nota do livro, em Livros/.").size(12.0).color(p.faint));
            return;
        }
        let marks = self.marks.clone();
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            for m in marks {
                let preview: String = m.text.chars().take(160).collect();
                let r = ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 16.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.5, mark_swatch(&m.color));
                    ui.vertical(|ui| {
                        if !m.label.is_empty() {
                            ui.label(RichText::new(&m.label).size(11.0).color(p.faint));
                        }
                        let r = ui.add(egui::Button::new(RichText::new(preview).size(12.5).color(p.text)).frame(false).wrap());
                        if !m.note.is_empty() {
                            ui.label(RichText::new(format!("{}  {}", icons::PENCIL, m.note)).size(11.5).color(p.muted));
                        }
                        r
                    })
                    .inner
                });
                if r.inner.clicked() {
                    self.go(m.loc.clone());
                    *keep = false;
                }
                ui.add_space(6.0);
            }
        });
    }

    fn look_panel(&mut self, ui: &mut egui::Ui, cx: &Cx) {
        let p = cx.palette;
        let before = self.prefs.clone();
        kit::section(ui, "Aparência", p);
        let papers = [("claro", "Claro"), ("sepia", "Sépia"), ("escuro", "Escuro"), ("tema", "Tema")];
        let sel = papers.iter().position(|x| x.0 == self.prefs.paper).unwrap_or(0);
        if let Some(i) = kit::segmented(ui, &papers.iter().map(|x| ("", x.1)).collect::<Vec<_>>(), sel, p) {
            self.prefs.paper = papers[i].0.to_string();
        }
        ui.add_space(8.0);
        if self.reflows() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Fonte").size(12.0).color(p.muted));
                let shown = if self.prefs.font.is_empty() { "EB Garamond (padrão)".to_string() } else { self.prefs.font.clone() };
                egui::ComboBox::from_id_salt(("leitor-font", &self.file)).selected_text(shown).width(200.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.prefs.font, String::new(), "EB Garamond (padrão)");
                    for f in ["Cormorant Garamond", "Libre Baskerville", "Playfair Display", "Inter"] {
                        ui.selectable_value(&mut self.prefs.font, f.to_string(), f);
                    }
                    ui.separator();
                    for f in crate::fonts::book_fonts().families() {
                        if !["EB Garamond", "Cormorant Garamond", "Libre Baskerville", "Playfair Display", "Inter"].contains(&f.as_str()) {
                            ui.selectable_value(&mut self.prefs.font, f.clone(), f);
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tamanho").size(12.0).color(p.muted));
                if kit::icon_button(ui, icons::MINUS, "Menor", false, p).clicked() {
                    self.prefs.size = (self.prefs.size - 1.0).max(9.0);
                }
                ui.label(RichText::new(format!("{:.0}", self.prefs.size)).size(12.5).color(p.text));
                if kit::icon_button(ui, icons::PLUS, "Maior", false, p).clicked() {
                    self.prefs.size = (self.prefs.size + 1.0).min(30.0);
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Entrelinha").size(12.0).color(p.muted));
                ui.add(egui::Slider::new(&mut self.prefs.leading, 1.1..=2.2).step_by(0.05).show_value(false));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Margens").size(12.0).color(p.muted));
                ui.add(egui::Slider::new(&mut self.prefs.margin, 4.0..=40.0).step_by(1.0).show_value(false));
            });
            kit::setting(ui, "Justificado", "", p, |ui| kit::toggle(ui, &mut self.prefs.justify, p));
            kit::setting(ui, "Hifenização", "", p, |ui| kit::toggle(ui, &mut self.prefs.hyphenate, p));
        } else {
            let fits = [("pagina", "Página inteira"), ("largura", "Largura")];
            let sel = fits.iter().position(|x| x.0 == self.prefs.fit).unwrap_or(0);
            if let Some(i) = kit::segmented(ui, &fits.iter().map(|x| ("", x.1)).collect::<Vec<_>>(), sel, p) {
                self.prefs.fit = fits[i].0.to_string();
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
            }
            if self.format() == Some(Format::Pdf) {
                kit::setting(ui, "Cortar margens", "Amplia o texto no espaço da tela.", p, |ui| kit::toggle(ui, &mut self.prefs.trim, p));
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Zoom {:.0}%", self.zoom * 100.0)).size(12.0).color(p.muted));
                if kit::icon_button(ui, icons::ZOOM_OUT, "Menos", false, p).clicked() {
                    self.zoom = (self.zoom / 1.2).max(0.5);
                }
                if kit::icon_button(ui, icons::ZOOM_IN, "Mais", false, p).clicked() {
                    self.zoom = (self.zoom * 1.2).min(6.0);
                }
                if kit::icon_button(ui, icons::REFRESH_CW, "Ajustar", false, p).clicked() {
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                }
            });
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Páginas").size(12.0).color(p.muted));
        let spreads = [("auto", "Automático"), ("uma", "Uma"), ("duas", "Duas")];
        let sel = spreads.iter().position(|x| x.0 == self.prefs.spread).unwrap_or(0);
        if let Some(i) = kit::segmented(ui, &spreads.iter().map(|x| ("", x.1)).collect::<Vec<_>>(), sel, p) {
            self.prefs.spread = spreads[i].0.to_string();
        }
        ui.add_space(6.0);
        kit::setting(ui, "Copiar com a citação", "“…” — Autor, Título, página.", p, |ui| kit::toggle(ui, &mut self.prefs.cite, p));
        if self.prefs != before {
            self.state_dirty = true;
            if self.prefs.trim != before.trim {
                self.infos.clear();
                self.asked_infos.clear();
            }
        }
    }

    /// Where a selection's menu goes: above its first line.
    fn sel_anchor(&self) -> Option<egui::Pos2> {
        let s = self.sel.as_ref()?;
        let v = self.views.iter().find(|v| v.chap == s.chap && v.page == s.page)?;
        let t = self.info(s.chap, s.page)?.text.clone()?;
        let r = t.rects(s.a, s.b);
        let first = r.first()?;
        let sr = v.frac_rect(*first);
        Some(pos2(sr.center().x.clamp(v.rect.left() + 120.0, v.rect.right() - 120.0), sr.top() - 8.0))
    }

    fn selection_menu(&mut self, ui: &mut egui::Ui, cx: &Cx, out: &mut Vec<Out>) {
        let p = cx.palette;
        if !self.sel.as_ref().is_some_and(|s| s.done) || self.note_edit.is_some() {
            return;
        }
        let Some(at) = self.sel_anchor() else { return };
        let ctx = ui.ctx().clone();
        let mark = self.sel.as_ref().and_then(|s| s.mark.clone());
        egui::Area::new(egui::Id::new(("leitor-sel", &self.file))).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(at).order(egui::Order::Foreground).constrain(true).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for c in COLORS {
                        let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
                        let current = mark.as_ref().and_then(|id| self.marks.iter().find(|m| &m.id == id)).is_some_and(|m| m.color == c);
                        ui.painter().circle_filled(r.center(), if resp.hovered() || current { 8.0 } else { 7.0 }, mark_swatch(c));
                        if current {
                            ui.painter().circle_stroke(r.center(), 10.0, Stroke::new(1.5_f32, p.text));
                        }
                        if resp.on_hover_text(format!("Grifar ({c})")).clicked() {
                            self.highlight(cx, c, out);
                        }
                    }
                    ui.add_space(4.0);
                    if kit::icon_button(ui, icons::PENCIL, "Nota: grifa e abre um campo para escrever", false, p).clicked() {
                        let id = match mark.clone() {
                            Some(id) => Some(id),
                            None => self.highlight(cx, "amarelo", out),
                        };
                        if let Some(id) = id {
                            let note = self.marks.iter().find(|m| m.id == id).map(|m| m.note.clone()).unwrap_or_default();
                            self.note_edit = Some((id, note));
                        }
                    }
                    if kit::icon_button(ui, icons::NOTEBOOK_PEN, "Mandar ao Journal (guarda em Trechos/)", false, p).clicked() {
                        if let Some((note, snippet)) = self.keep_selection(cx, out) {
                            out.push(Out::Snippet { note, snippet });
                            self.sel = None;
                        }
                    }
                    let copy_tip = if self.prefs.cite { "Copiar com a citação" } else { "Copiar" };
                    if kit::icon_button(ui, icons::COPY, copy_tip, false, p).clicked() {
                        if let Some(t) = self.selection_text() {
                            let t = if self.prefs.cite { self.citation(&t) } else { t };
                            ui.ctx().copy_text(t);
                            out.push(Out::Toast("Copiado.".into()));
                        }
                        self.sel = None;
                    }
                    if let Some(id) = mark.clone() {
                        if kit::icon_button(ui, icons::TRASH_2, "Remover o grifo", false, p).clicked() {
                            self.unmark(cx, &id, out);
                        }
                    }
                    if cx.journal_open {
                        // dragged to a page of the Journal beside
                        let (r, resp) = ui.allocate_exact_size(vec2(22.0, 26.0), egui::Sense::drag());
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, icons::GRIP_VERTICAL, kit::icon_font(15.0), if resp.hovered() { p.text } else { p.faint });
                        let resp = resp.on_hover_text("Arraste para a página do Journal");
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if resp.drag_started() {
                            if let Some((note, _)) = self.keep_selection(cx, out) {
                                egui::DragAndDrop::set_payload(ui.ctx(), crate::journal_ui::Payload::Snippet(note));
                                self.sel = None;
                            }
                        }
                    }
                });
            });
        });
    }

    fn cut_menu(&mut self, ui: &mut egui::Ui, cx: &Cx, out: &mut Vec<Out>) {
        let p = cx.palette;
        let Some(c) = self.cut.clone().filter(|c| c.done) else { return };
        let Some(v) = self.views.iter().find(|v| v.chap == c.chap && v.page == c.page).copied() else { return };
        let r = v.frac_rect(c.rect());
        let ctx = ui.ctx().clone();
        let ready = c.png.is_some();
        let inside = self.info(c.chap, c.page).and_then(|i| i.text.clone()).and_then(|t| t.inside(c.rect()).map(|(a, b)| t.text(a, b))).filter(|t| !t.trim().is_empty());
        egui::Area::new(egui::Id::new(("leitor-cut", &self.file))).pivot(egui::Align2::CENTER_TOP).fixed_pos(pos2(r.center().x, r.bottom() + 8.0)).order(egui::Order::Foreground).constrain(true).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled_ui(ready, |ui| kit::button(ui, Some(icons::NOTEBOOK_PEN), if ready { "Mandar ao Journal" } else { "Preparando…" }, p)).inner.clicked() {
                        if let Some((note, snippet)) = self.keep_cut(cx, out) {
                            out.push(Out::Snippet { note, snippet });
                            self.cut = None;
                        }
                    }
                    if let Some(t) = inside.clone() {
                        if kit::icon_button(ui, icons::COPY, "Copiar o texto da região", false, p).clicked() {
                            ui.ctx().copy_text(t);
                            out.push(Out::Toast("Copiado.".into()));
                        }
                    }
                    if cx.journal_open && ready {
                        let (gr, resp) = ui.allocate_exact_size(vec2(22.0, 26.0), egui::Sense::drag());
                        ui.painter().text(gr.center(), egui::Align2::CENTER_CENTER, icons::GRIP_VERTICAL, kit::icon_font(15.0), if resp.hovered() { p.text } else { p.faint });
                        let resp = resp.on_hover_text("Arraste para a página do Journal");
                        if resp.drag_started() {
                            if let Some((note, _)) = self.keep_cut(cx, out) {
                                egui::DragAndDrop::set_payload(ui.ctx(), crate::journal_ui::Payload::Snippet(note));
                                self.cut = None;
                            }
                        }
                    }
                    if kit::icon_button(ui, icons::X, "Cancelar", false, p).clicked() {
                        self.cut = None;
                    }
                });
            });
        });
    }

    fn note_editor(&mut self, ui: &mut egui::Ui, cx: &Cx, out: &mut Vec<Out>) {
        let p = cx.palette;
        let Some((id, mut text)) = self.note_edit.clone() else { return };
        let at = self.sel_anchor().unwrap_or_else(|| ui.max_rect().center());
        let ctx = ui.ctx().clone();
        let mut done: Option<bool> = None;
        egui::Area::new(egui::Id::new(("leitor-note-edit", &self.file))).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(at).order(egui::Order::Foreground).constrain(true).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(300.0);
                ui.label(RichText::new("Nota do grifo").size(12.0).color(p.muted));
                let r = ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3).desired_width(f32::INFINITY));
                if !r.has_focus() && text.is_empty() {
                    r.request_focus();
                }
                ui.horizontal(|ui| {
                    if kit::primary(ui, "Guardar", p).clicked() || kit::cmd_enter(ui.ctx()) {
                        done = Some(true);
                    }
                    if kit::button(ui, None, "Cancelar", p).clicked() {
                        done = Some(false);
                    }
                });
            });
        });
        match done {
            Some(true) => {
                self.set_mark_note(cx, &id, &text, out);
                self.note_edit = None;
                self.sel = None;
            }
            Some(false) => self.note_edit = None,
            None => self.note_edit = Some((id, text)),
        }
        let _ = Color32::WHITE;
    }
}
