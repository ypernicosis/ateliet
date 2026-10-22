//! Enredo, the saga: the books of a series side by side. The Grade lays
//! the series' plot lines (tramas with the same name are one line) across
//! the books; Cronologia, Pistas and Arcos read every book. Clicking
//! something of another book opens that book's Enredo on it.

use super::enredo_ui::{truncated, EnredoView, Sel};
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::Book;
use ateliet_core::scriptorium::enredo::{self as en, Col};
use eframe::egui::{self, pos2, vec2, Rect, RichText, Sense, Stroke};
use serde_json::Value as Json;
use std::path::Path;

/// One book of what is shown: the current one reads the live data.
pub struct Src {
    pub book: Book,
    pub data: Option<Json>,
    pub cols: Vec<Col>,
}

impl Src {
    pub fn d<'a>(&'a self, current: &'a Json) -> &'a Json {
        self.data.as_ref().unwrap_or(current)
    }

    pub fn current(&self) -> bool {
        self.data.is_none()
    }
}

impl EnredoView {
    /// The books shown: this one, or every book of the series.
    pub(super) fn sources(&mut self, root: &Path, ix: &Index, b: &Book, cols: &[Col], saga: &[Book]) -> Vec<Src> {
        if !self.saga_on(saga) {
            return vec![Src { book: b.clone(), data: None, cols: cols.to_vec() }];
        }
        saga.iter()
            .map(|sb| {
                if sb.path == b.path {
                    return Src { book: sb.clone(), data: None, cols: cols.to_vec() };
                }
                let m = std::fs::metadata(root.join(en::path(sb))).and_then(|m| m.modified()).ok();
                let fresh = self.saga_cache.get(&sb.path).is_some_and(|c| c.0 == m);
                if !fresh {
                    self.saga_cache.insert(sb.path.clone(), (m, en::load(root, sb)));
                }
                let data = self.saga_cache[&sb.path].1.clone();
                let cols = en::chapters(ix, sb, &data);
                Src { book: sb.clone(), data: Some(data), cols }
            })
            .collect()
    }

    pub(super) fn saga_on(&self, saga: &[Book]) -> bool {
        self.saga && saga.len() > 1 && self.mode.has_saga()
    }

    /// Open another book's Enredo (on `sel`), keeping the mode.
    pub(super) fn go_book(&mut self, path: &str, sel: Option<Sel>) {
        self.pending_book = Some((path.to_string(), sel));
    }

    /// Livro | Saga, for the views that read the whole series.
    pub(super) fn scope_switch(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, saga: &[Book]) {
        if !self.mode.has_saga() {
            return;
        }
        let enabled = saga.len() > 1;
        let tip = if enabled {
            format!("A série {}: {} livros", b.serie, saga.len())
        } else if b.serie.trim().is_empty() {
            "Defina a \"serie\" do livro (na nota do livro) para ver a saga.".to_string()
        } else {
            "Só há um livro nesta série.".to_string()
        };
        ui.add_enabled_ui(enabled, |ui| {
            let cur = if self.saga && enabled { 1 } else { 0 };
            // the bar lays out right to left: keep Livro before Saga
            let r = ui.allocate_ui_with_layout(vec2(112.0, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| kit::segmented(ui, &[("", "Livro"), ("", "Saga")], cur, p));
            if let Some(i) = r.inner {
                self.saga = i == 1;
            }
            r.response.on_hover_text(tip.clone()).on_disabled_hover_text(tip);
        });
    }

    /// Tramas × books.
    pub(super) fn grade_saga(&mut self, ui: &mut egui::Ui, p: Palette, srcs: &[Src]) {
        let mut names: Vec<(String, egui::Color32)> = Vec::new();
        for s in srcs {
            for t in s.d(&self.data)["tramas"].as_array().into_iter().flatten() {
                let n = en::str_of(t, "nome").to_string();
                if !n.is_empty() && !names.iter().any(|x| x.0 == n) {
                    names.push((n, super::color(en::str_of(t, "cor"))));
                }
            }
        }
        let mut go: Option<(String, Option<Sel>)> = None;
        egui::ScrollArea::both().id_salt("en-saga").auto_shrink([false, false]).show(ui, |ui| {
            if names.is_empty() {
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.label(kit::icon(icons::LAYERS, 32.0).color(p.faint));
                    ui.add_space(12.0);
                    ui.label(RichText::new("As tramas da saga").font(kit::medium(17.0)).color(p.text));
                    ui.add_space(6.0);
                    ui.allocate_ui(vec2(460.0, 60.0), |ui| {
                        ui.label(RichText::new("Nenhuma trama nos livros desta série ainda. Tramas com o mesmo nome em livros diferentes viram uma linha só aqui.").color(p.muted));
                    });
                });
                return;
            }
            const LAB: f32 = 176.0;
            const COLW: f32 = 232.0;
            const HEADH: f32 = 52.0;
            const CARD: f32 = 46.0;
            const GAP: f32 = 6.0;
            const PAD: f32 = 8.0;
            const MAXN: usize = 12;
            // the scenes of each line in each book, in reading order
            let cells: Vec<Vec<Vec<Json>>> = names
                .iter()
                .map(|(nome, _)| {
                    srcs.iter()
                        .map(|s| {
                            let d = s.d(&self.data);
                            let Some(tid) = d["tramas"].as_array().into_iter().flatten().find(|t| en::str_of(t, "nome") == nome).map(|t| en::str_of(t, "id").to_string()) else { return Vec::new() };
                            en::ordered(&s.cols, d).into_iter().map(|i| &d["cenas"][i]).filter(|c| en::tramas_of(c).contains(&tid.as_str())).cloned().collect()
                        })
                        .collect()
                })
                .collect();
            let row_h: Vec<f32> = cells.iter().map(|r| r.iter().map(|c| c.len().min(MAXN) as f32 * (CARD + GAP) + if c.len() > MAXN { 22.0 } else { 0.0 }).fold(CARD, f32::max) + PAD * 2.0).collect();
            let total = vec2(LAB + COLW * srcs.len() as f32 + 24.0, HEADH + row_h.iter().sum::<f32>() + 24.0);
            let (rect, _) = ui.allocate_exact_size(total.max(ui.available_size()), Sense::hover());
            let o = rect.min + vec2(16.0, 8.0);
            let painter = ui.painter().clone();
            let clip = ui.clip_rect();
            let mut y = o.y + HEADH;
            for (ri, (nome, color)) in names.iter().enumerate() {
                let h = row_h[ri];
                painter.line_segment([pos2(o.x, y), pos2(o.x + LAB + COLW * srcs.len() as f32, y)], Stroke::new(1.0_f32, p.border));
                for (ci, s) in srcs.iter().enumerate() {
                    let x = o.x + LAB + COLW * ci as f32;
                    if s.current() {
                        painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(COLW, h)), 0.0, p.accent_soft.gamma_multiply(0.35));
                    }
                    let list = &cells[ri][ci];
                    for (k, sc) in list.iter().take(MAXN).enumerate() {
                        let r = Rect::from_min_size(pos2(x + PAD, y + PAD + k as f32 * (CARD + GAP)), vec2(COLW - PAD * 2.0, CARD));
                        if !r.intersects(clip) {
                            continue;
                        }
                        let resp = ui.interact(r, ui.id().with(("saga-card", ri, &s.book.path, en::str_of(sc, "id"))), Sense::click());
                        self.paint_card(ui, r, sc, p, resp.hovered(), false);
                        let cap = en::cap_of(sc).map(|c| s.cols.iter().find(|x| x.path == c).map(|x| x.title.clone()).unwrap_or_default()).unwrap_or_else(|| "ideia".into());
                        let g = truncated(ui, &cap, egui::FontId::proportional(10.5), p.faint, 110.0);
                        painter.galley(pos2(r.right() - g.size().x - 8.0, r.bottom() - g.size().y - 6.0), g, p.faint);
                        if resp.clicked() {
                            go = Some((s.book.path.clone(), Some(Sel::Cena(en::str_of(sc, "id").into()))));
                        }
                    }
                    if list.len() > MAXN {
                        painter.text(pos2(x + PAD, y + PAD + MAXN as f32 * (CARD + GAP) + 4.0), egui::Align2::LEFT_TOP, format!("+{} cenas", list.len() - MAXN), egui::FontId::proportional(11.5), p.faint);
                    }
                }
                // the line's name, sticky on the left
                let lx = o.x.max(clip.left());
                let lr = Rect::from_min_size(pos2(lx, y + PAD), vec2(LAB - 12.0, 26.0));
                painter.rect_filled(lr.expand2(vec2(4.0, 2.0)), 6.0, p.bg.gamma_multiply(0.94));
                painter.circle_filled(pos2(lr.left() + 8.0, lr.center().y), 4.5, *color);
                let g = truncated(ui, nome, kit::medium(13.0), p.text, LAB - 36.0);
                painter.galley(pos2(lr.left() + 20.0, lr.center().y - g.size().y / 2.0), g, p.text);
                y += h;
            }
            // the books, sticky on top
            let ty = o.y.max(clip.top());
            painter.rect_filled(Rect::from_min_size(pos2(o.x - 16.0, ty - 8.0), vec2(total.x + 16.0, HEADH + 8.0)), 0.0, p.bg);
            painter.text(pos2(o.x.max(clip.left()), ty + HEADH / 2.0), egui::Align2::LEFT_CENTER, "Tramas da saga", kit::medium(12.0), p.faint);
            for (ci, s) in srcs.iter().enumerate() {
                let r = Rect::from_min_size(pos2(o.x + LAB + COLW * ci as f32 + PAD, ty + 4.0), vec2(COLW - PAD * 2.0, HEADH - 10.0));
                let resp = ui.interact(r, ui.id().with(("saga-book", &s.book.path)), Sense::click()).on_hover_text(if s.current() { "Este livro" } else { "Abrir o Enredo deste livro" });
                if resp.hovered() && !s.current() {
                    painter.rect_filled(r, 8.0, p.bg_hover);
                }
                let vol = if s.book.volume.trim().is_empty() { format!("{}", ci + 1) } else { format!("Vol. {}", s.book.volume) };
                painter.text(pos2(r.left() + 8.0, r.top() + 12.0), egui::Align2::LEFT_CENTER, vol.to_uppercase(), kit::medium(10.0), if s.current() { p.accent_strong } else { p.faint });
                let g = truncated(ui, &s.book.title, kit::medium(13.5), p.text, r.width() - 16.0);
                painter.galley(pos2(r.left() + 8.0, r.top() + 20.0), g, p.text);
                if resp.clicked() && !s.current() {
                    go = Some((s.book.path.clone(), None));
                }
            }
        });
        if let Some((path, sel)) = go {
            self.go_book(&path, sel);
        }
    }
}
