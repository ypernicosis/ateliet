//! Enredo: the Escaleta, a step outline made for writing fast. Every
//! chapter ends in an open line: type and press Enter and the scene is
//! there, the cursor still on the line for the next one; paste a list and
//! each item becomes a scene. A scene is edited where it is (the title, then
//! as much text as it needs below), from the keyboard: Enter opens the next
//! scene, Tab goes into the text, the arrows walk the outline and Alt+arrows
//! carry the scene up and down, across chapters. Scenes whose chapter is
//! not known yet can sit "somewhere between" two chapters, in a band of
//! their own. The side drawer is there for the rest, only when asked for.

use super::enredo_ui::{note_name, status_color, EnredoView, Sel, Zone};
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::{self, Book};
use ateliet_core::scriptorium::enredo::{self as en, Col, Draft};
use ateliet_core::scriptorium::mundo::deaccent;
use eframe::egui::{self, pos2, vec2, Color32, Key, Modifiers, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::Path;

/// Where a scene sits in the outline.
#[derive(Clone, PartialEq, Debug)]
pub enum Place {
    Ideas,
    Cap(String),
    /// Somewhere between two chapters (in book order).
    Range(String, String),
}

impl Place {
    pub fn key(&self) -> String {
        match self {
            Place::Ideas => String::new(),
            Place::Cap(c) => c.clone(),
            Place::Range(a, b) => format!("≈{a}|{b}"),
        }
    }
    fn cap(&self) -> Option<&str> {
        match self {
            Place::Cap(c) => Some(c),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Title,
    Body,
}

/// What the outline remembers between frames.
pub struct Esc {
    /// The scene and field being typed in.
    pub edit: Option<(String, Field)>,
    /// Give the field the keyboard (cursor at the end) when it shows.
    pub focus: bool,
    /// What is typed in each group's open line, by `Place::key`.
    pub drafts: HashMap<String, String>,
    /// Put the keyboard on this group's open line.
    pub focus_add: Option<String>,
    /// The group written in last (where N goes).
    pub last: Option<Place>,
    /// 0 titles, 1 titles and the start of the text, 2 everything.
    pub density: usize,
    /// Snapshots before structural changes (Ctrl+Z).
    pub undo: Vec<Json>,
    /// The groups as last drawn, in order (empty chapters too).
    pub order: Vec<Place>,
}

impl Default for Esc {
    fn default() -> Esc {
        Esc { edit: None, focus: false, drafts: HashMap::new(), focus_add: None, last: None, density: 1, undo: Vec::new(), order: Vec::new() }
    }
}

enum Ev {
    Edit(String, Field),
    /// A field lost the keyboard.
    Blur(String, Field),
    /// Enter on a title: the next scene, right after this one.
    NewAfter(String, Place),
    /// The open line of a group was sent.
    Capture(Place, String),
    /// Text with line breaks landed in a title: it becomes scenes.
    Split(String, Place, String),
    /// Backspace on an empty scene.
    Delete(String),
    Move(String, i32),
    Nav(String, i32),
    Details(String),
    FocusAdd(String),
    ToChapter(String),
    Open(String),
    Select(Sel),
    ToggleArc(String),
}

/// Take one press of `key` with modifiers `want` out of this frame's
/// events, so the text field does not see it.
fn take_key(ui: &egui::Ui, key: Key, want: impl Fn(Modifiers) -> bool) -> bool {
    ui.input_mut(|i| {
        let mut hit = false;
        i.events.retain(|e| match e {
            egui::Event::Key { key: k, pressed: true, modifiers, .. } if !hit && *k == key && want(*modifiers) => {
                hit = true;
                false
            }
            _ => true,
        });
        hit
    })
}

fn plain(m: Modifiers) -> bool {
    !m.shift && !m.alt && !m.command && !m.ctrl
}

fn cmd(m: Modifiers) -> bool {
    m.command || m.ctrl
}

/// Focus a text field with the cursor at the end.
fn focus_end(ctx: &egui::Context, id: egui::Id, len: usize) {
    let mut st = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(len))));
    st.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

fn dashed_rect(painter: &egui::Painter, r: Rect, stroke: Stroke) {
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    painter.extend(egui::Shape::dashed_line(&pts, stroke, 5.0, 4.0));
}

/// A chapter by its label ("3", "P") or the start of its title.
fn find_col<'a>(all: &'a [Col], key: &str) -> Option<&'a Col> {
    let k = deaccent(key.trim());
    if k.is_empty() {
        return None;
    }
    all.iter().find(|c| deaccent(&c.label) == k).or_else(|| all.iter().find(|c| deaccent(&c.title).starts_with(&k)))
}

/// A name typed after `@` or `#`, matched to a known one: same name, else
/// the first that starts with it.
fn best<'a>(names: impl Iterator<Item = (&'a str, &'a str)> + Clone, typed: &str) -> Option<&'a str> {
    let t = deaccent(typed.trim());
    if t.is_empty() {
        return None;
    }
    names.clone().find(|(_, n)| deaccent(n) == t).or_else(|| names.clone().find(|(_, n)| deaccent(n).starts_with(&t))).or_else(|| names.clone().find(|(_, n)| deaccent(n).split_whitespace().any(|w| w.starts_with(&t)))).map(|x| x.0)
}

impl EnredoView {
    // ── outline: placing scenes ──────────────────────────────────────────

    fn snapshot(&mut self) {
        self.esc.undo.push(self.data.clone());
        if self.esc.undo.len() > 60 {
            self.esc.undo.remove(0);
        }
    }

    /// Put a scene in a place, before `before` (None = at the end).
    pub(super) fn place_scene(&mut self, id: &str, place: &Place, before: Option<&str>) {
        match place {
            Place::Cap(c) => en::move_scene(&mut self.data, id, Some(c), before, None),
            Place::Ideas => {
                en::move_scene(&mut self.data, id, None, before, None);
                en::set_range(&mut self.data, id, None);
            }
            Place::Range(a, b) => {
                en::move_scene(&mut self.data, id, None, before, None);
                en::set_range(&mut self.data, id, Some((a, b)));
            }
        }
    }

    /// The scene after `id` among those that share its chapter (or its lack of one).
    fn next_sibling(&self, place: &Place, id: &str) -> Option<String> {
        let list = en::scenes_in(&self.data, place.cap());
        let k = list.iter().position(|&i| self.data["cenas"][i]["id"] == id)?;
        list.get(k + 1).map(|&i| en::str_of(&self.data["cenas"][i], "id").to_string())
    }

    /// Where a draft's `^3` or `^3-6` sends it, if anywhere.
    fn draft_place(d: &Draft, all: &[Col]) -> Option<Place> {
        let (a, b) = d.cap.as_ref()?;
        let ca = find_col(all, a)?;
        match b.as_deref().and_then(|b| find_col(all, b)) {
            Some(cb) if cb.path != ca.path => {
                let ia = all.iter().position(|c| c.path == ca.path).unwrap_or(0);
                let ib = all.iter().position(|c| c.path == cb.path).unwrap_or(0);
                let (x, y) = if ia <= ib { (ca, cb) } else { (cb, ca) };
                Some(Place::Range(x.path.clone(), y.path.clone()))
            }
            _ => Some(Place::Cap(ca.path.clone())),
        }
    }

    /// Point of view and plot lines from a draft (plot lines not found are created).
    fn fill_tokens(&mut self, id: &str, d: &Draft, chars: &[String]) {
        let Some(i) = en::scene_index(&self.data, id) else { return };
        if let Some(pov) = &d.pov {
            let names: Vec<String> = chars.iter().map(|c| note_name(c)).collect();
            let found = best(names.iter().map(|n| (n.as_str(), n.as_str())), pov).map(String::from);
            self.data["cenas"][i]["pov"] = json!(found.unwrap_or_else(|| pov.clone()));
        }
        for t in &d.tramas {
            let known: Vec<(String, String)> = self.data["tramas"].as_array().into_iter().flatten().map(|x| (en::str_of(x, "id").to_string(), en::str_of(x, "nome").to_string())).collect();
            let tid = match best(known.iter().map(|(a, b)| (a.as_str(), b.as_str())), t) {
                Some(x) => x.to_string(),
                None => en::new_trama(&mut self.data, t),
            };
            let mut l: Vec<String> = en::tramas_of(&self.data["cenas"][i]).into_iter().map(String::from).collect();
            if !l.contains(&tid) {
                l.push(tid);
            }
            self.data["cenas"][i]["tramas"] = json!(l);
        }
    }

    /// A new scene from a draft, in `place` (or where the draft says),
    /// right after `after` when it lands in the same place.
    fn create_draft(&mut self, place: &Place, d: &Draft, all: &[Col], chars: &[String], after: Option<&str>) -> String {
        let target = Self::draft_place(d, all).unwrap_or_else(|| place.clone());
        let id = en::new_scene(&mut self.data, target.cap(), &[]);
        if let Some(i) = en::scene_index(&self.data, &id) {
            self.data["cenas"][i]["titulo"] = json!(d.titulo);
            self.data["cenas"][i]["resumo"] = json!(d.resumo);
        }
        self.fill_tokens(&id, d, chars);
        let before = match after {
            Some(a) if &target == place => self.next_sibling(place, a),
            _ => None,
        };
        if before.is_some() || matches!(target, Place::Range(..)) {
            self.place_scene(&id, &target, before.as_deref());
        }
        id
    }

    /// `@pov #trama ^cap` typed in a title take effect when the title is left.
    fn apply_tokens(&mut self, id: &str, place: &Place, all: &[Col], chars: &[String]) {
        let Some(i) = en::scene_index(&self.data, id) else { return };
        let t = en::str_of(&self.data["cenas"][i], "titulo").to_string();
        let (titulo, pov, tramas, cap) = en::take_tokens(&t);
        if pov.is_none() && tramas.is_empty() && cap.is_none() {
            if t.trim() != t {
                self.data["cenas"][i]["titulo"] = json!(t.trim());
                self.changed();
            }
            return;
        }
        self.snapshot();
        self.data["cenas"][i]["titulo"] = json!(titulo);
        let d = Draft { titulo, resumo: String::new(), pov, tramas, cap };
        self.fill_tokens(id, &d, chars);
        if let Some(to) = Self::draft_place(&d, all).filter(|to| to != place) {
            self.place_scene(id, &to, None);
        }
        self.changed();
    }

    // ── outline: the view ────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub(super) fn escaleta(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, all: &[Col], chars: &[String], root: &Path, ix: &Index, acts: &mut Vec<Action>) {
        let (lo, hi) = self.window(all);
        let cenas: Vec<Json> = self.data["cenas"].as_array().cloned().unwrap_or_default();
        let pos = |path: &str| all.iter().position(|c| c.path == path);
        // scenes without a chapter: between two chapters, or loose ideas
        let mut ranged: Vec<(usize, usize, usize)> = Vec::new();
        let mut loose: Vec<usize> = Vec::new();
        for i in en::scenes_in(&self.data, None) {
            match en::range_of(&cenas[i]).and_then(|(x, y)| Some((pos(x)?, pos(y)?))) {
                Some((x, y)) => ranged.push((i, x.min(y), x.max(y))),
                None => loose.push(i),
            }
        }
        let mut groups: Vec<(Place, Option<usize>, Vec<usize>)> = Vec::new();
        for ci in lo..hi {
            let mut spans: Vec<(usize, usize)> = ranged.iter().filter(|r| r.1.max(lo) == ci && r.2 >= lo).map(|r| (r.1, r.2)).collect();
            spans.sort();
            spans.dedup();
            for (x, y) in spans {
                let list: Vec<usize> = ranged.iter().filter(|r| r.1 == x && r.2 == y).map(|r| r.0).filter(|&i| self.passes(&cenas[i])).collect();
                if !list.is_empty() {
                    groups.push((Place::Range(all[x].path.clone(), all[y].path.clone()), None, list));
                }
            }
            let list = en::scenes_in(&self.data, Some(&all[ci].path)).into_iter().filter(|&i| self.passes(&cenas[i])).collect();
            groups.push((Place::Cap(all[ci].path.clone()), Some(ci), list));
        }
        groups.push((Place::Ideas, None, loose.into_iter().filter(|&i| self.passes(&cenas[i])).collect()));
        self.esc.order = groups.iter().map(|g| g.0.clone()).collect();
        let cref = &cenas;
        let flat: Vec<(String, Place)> = groups.iter().flat_map(|(pl, _, l)| l.iter().map(move |&i| (en::str_of(&cref[i], "id").to_string(), pl.clone()))).collect();

        let dragging = Self::dragging(ui);
        let mut zones = Vec::new();
        let mut evs: Vec<Ev> = Vec::new();
        let mut card_cmd = None;
        let mut edited = false;
        let mut seen_edit = false;
        egui::ScrollArea::vertical().id_salt("en-esc").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let w = (avail - 80.0).clamp(320.0, 820.0);
            let side = ((avail - w) / 2.0).max(16.0);
            ui.add_space(16.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    self.esc_toolbar(ui, p, w);
                    let mut last_part = String::new();
                    let mut n = 0;
                    for (place, col, list) in &groups {
                        let col = col.map(|k| &all[k]);
                        if let Some(c) = col {
                            if !c.part.is_empty() && c.part != last_part {
                                ui.add_space(14.0);
                                let on = self.arco.as_ref() == Some(&c.part);
                                let r = ui.add(egui::Label::new(RichText::new(c.part.to_uppercase()).font(kit::medium(11.0)).color(if on { p.accent_strong } else { p.faint }).extra_letter_spacing(1.2)).sense(Sense::click())).on_hover_text("Clique: mostrar só este arco");
                                if r.clicked() {
                                    evs.push(Ev::ToggleArc(c.part.clone()));
                                }
                                last_part = c.part.clone();
                                ui.add_space(4.0);
                            }
                        }
                        ui.add_space(if matches!(place, Place::Range(..)) { 8.0 } else { 10.0 });
                        self.esc_head(ui, p, place, col, all, list.len(), &mut evs);
                        if let Some(pl) = col.and_then(|c| c.planned.as_ref().or(c.plan.as_ref())) {
                            let notas: Vec<&str> = en::str_of(pl, "notas").lines().filter(|l| !l.trim().is_empty()).take(2).collect();
                            if !notas.is_empty() {
                                ui.add(egui::Label::new(RichText::new(notas.join(" · ")).size(12.0).italics().color(p.faint)).truncate());
                            }
                        }
                        ui.add_space(6.0);
                        let ranged = matches!(place, Place::Range(..));
                        let frame = egui::Frame::new().stroke(if ranged { Stroke::NONE } else { Stroke::new(1.0_f32, p.border) }).corner_radius(10.0).fill(match place {
                            Place::Ideas => p.bg_secondary,
                            Place::Range(..) => Color32::TRANSPARENT,
                            Place::Cap(_) => p.bg,
                        });
                        let mut cards = Vec::new();
                        let fr = frame.show(ui, |ui| {
                            ui.set_width(w);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (k, &si) in list.iter().enumerate() {
                                let num = if matches!(place, Place::Cap(_)) {
                                    n += 1;
                                    n.to_string()
                                } else if ranged {
                                    "≈".into()
                                } else {
                                    "·".into()
                                };
                                let (rr, sid, ch, seen) = self.esc_row(ui, p, &cenas[si], si, &num, k, w, place, dragging.as_deref(), &mut evs, &mut card_cmd);
                                edited |= ch;
                                seen_edit |= seen;
                                cards.push((rr, sid));
                            }
                            self.esc_add(ui, p, place, w, !list.is_empty(), &flat, &mut evs);
                        });
                        let fr_rect = fr.response.rect;
                        if ranged {
                            dashed_rect(ui.painter(), fr_rect.shrink(0.5), Stroke::new(1.0_f32, p.border_strong));
                        }
                        let range = match place {
                            Place::Cap(_) => None,
                            Place::Ideas => Some(None),
                            Place::Range(a, b) => Some(Some((a.clone(), b.clone()))),
                        };
                        zones.push(Zone { rect: fr_rect.intersect(ui.clip_rect()), cap: place.cap().map(String::from), trama: None, range, cards });
                    }
                    ui.add_space(80.0);
                });
            });
        });
        if edited {
            self.changed();
        }
        self.drops(ui, p, &zones);
        // Ctrl+Z, outside the text fields (they keep their own)
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if !typing && !self.esc.undo.is_empty() && ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Z))) {
            if let Some(d) = self.esc.undo.pop() {
                self.data = d;
                self.esc.edit = None;
                self.changed();
            }
        }
        self.card_cmd(card_cmd, acts);
        self.esc_events(evs, &flat, all, chars, b, root, ix, acts);
        // the scene being typed in is gone (deleted, filtered out)
        if !seen_edit && !self.esc.focus {
            self.esc.edit = None;
        }
        if let Some((id, _)) = &self.esc.edit {
            if en::scene_index(&self.data, id).is_none() {
                self.esc.edit = None;
            }
        }
    }

    fn esc_toolbar(&mut self, ui: &mut egui::Ui, p: Palette, w: f32) {
        ui.allocate_ui_with_layout(vec2(w, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let items = [("", "Títulos"), ("", "Resumo"), ("", "Tudo")];
            ui.add_space((w - kit::segmented_width(ui, &items) - 30.0).max(0.0));
            ui.label(kit::icon(icons::CIRCLE_HELP, 14.0).color(p.faint)).on_hover_text(
                "Escrever rápido\n\n\
                 Na linha aberta no fim de cada capítulo:\n\
                 Enter cria a cena · Shift+Enter quebra a linha\n\
                 Uma lista colada (-, *, 1.) vira uma cena por item; o que fica embaixo de cada item vira o texto dela\n\n\
                 No título de uma cena:\n\
                 Enter abre a próxima cena · Tab ou Shift+Enter escreve o texto\n\
                 ↑ ↓ andam pelas cenas · Alt+↑ ↓ levam a cena (até para outro capítulo)\n\
                 Backspace numa cena vazia apaga · Ctrl+Enter abre o painel · Esc sai\n\n\
                 No título: @Ana (ponto de vista) · #Mistério (trama; cria se não existe) · ^3 (capítulo 3) · ^3-6 (em algum ponto entre o 3 e o 6)\n\n\
                 Ctrl+Z desfaz criar, apagar e mover; N leva à última linha aberta usada",
            );
            if let Some(k) = kit::segmented(ui, &items, self.esc.density, p) {
                self.esc.density = k;
            }
        });
        ui.add_space(2.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn esc_head(&mut self, ui: &mut egui::Ui, p: Palette, place: &Place, col: Option<&Col>, all: &[Col], count: usize, evs: &mut Vec<Ev>) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            match (place, col) {
                (Place::Cap(_), Some(c)) => {
                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 18.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, if c.planned.is_some() { p.faint } else { status_color(&c.status) });
                    ui.label(RichText::new(&c.label).font(kit::medium(14.0)).color(p.faint));
                    let t = ui.add(egui::Label::new(RichText::new(&c.title).font(if c.planned.is_some() { egui::FontId::new(15.0, egui::FontFamily::Name("italic".into())) } else { kit::medium(15.0) }).color(p.text)).sense(Sense::click()).truncate());
                    if t.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let t = if c.planned.is_none() { t.on_hover_text("Clique para editar, duplo clique para abrir") } else { t };
                    if t.double_clicked() && c.planned.is_none() {
                        evs.push(Ev::Open(c.path.clone()));
                    } else if t.clicked() {
                        evs.push(Ev::Select(match &c.planned {
                            Some(pl) => Sel::Plan(en::str_of(pl, "id").to_string()),
                            None => Sel::Cap(c.path.clone()),
                        }));
                    }
                    let gi = all.iter().position(|x| x.path == c.path).unwrap_or(0);
                    for (key, label, k) in self.beats_at(gi) {
                        let colr = super::enredo_st::band_color(k, p);
                        let r = ui.add(egui::Label::new(RichText::new(format!("◆ {label}")).size(11.5).color(colr)).sense(Sense::click()).truncate());
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if r.clicked() {
                            evs.push(Ev::Select(Sel::Batida(key)));
                        }
                    }
                }
                (Place::Range(a, b), _) => {
                    let name = |path: &str| all.iter().find(|c| c.path == path).map(|c| if c.label == "·" { c.title.clone() } else { c.label.clone() }).unwrap_or_default();
                    ui.add_space(2.0);
                    ui.label(RichText::new("≈").font(kit::medium(14.0)).color(p.faint));
                    ui.label(RichText::new(format!("Em algum ponto entre {} e {}", name(a), name(b))).italics().color(p.muted)).on_hover_text(
                        "Cenas que ainda não têm capítulo certo. Arraste uma para um capítulo quando souber; mude a faixa no painel da cena ou com ^3-6 no título.",
                    );
                }
                _ => {
                    ui.label(kit::icon(icons::INBOX, 14.0).color(p.faint));
                    ui.label(RichText::new("Ideias").font(kit::medium(15.0)).color(p.text));
                    ui.label(RichText::new("cenas sem lugar ainda").color(p.faint)).on_hover_text("Para uma cena que acontece em algum ponto entre dois capítulos, escreva ^3-6 no título, ou escolha \"Entre capítulos\" no painel da cena.");
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !matches!(place, Place::Range(..)) && kit::icon_button_sized(ui, icons::PLUS, "Nova cena aqui", false, p, 24.0, 14.0).clicked() {
                    evs.push(Ev::FocusAdd(place.key()));
                }
                if count > 0 {
                    ui.label(RichText::new(format!("{count} {}", if count == 1 { "cena" } else { "cenas" })).size(11.5).color(p.faint));
                }
            });
        });
    }

    /// One scene of the outline. Returns its rect, its id, whether its text
    /// changed and whether it is the one being typed in.
    #[allow(clippy::too_many_arguments)]
    fn esc_row(&mut self, ui: &mut egui::Ui, p: Palette, s: &Json, si: usize, num: &str, k: usize, w: f32, place: &Place, dragging: Option<&str>, evs: &mut Vec<Ev>, card_cmd: &mut Option<(String, super::enredo_ui::CardCmd)>) -> (Rect, String, bool, bool) {
        let sid = en::str_of(s, "id").to_string();
        let editing = self.esc.edit.as_ref().filter(|e| e.0 == sid).map(|e| e.1);
        let want_focus = editing.is_some() && self.esc.focus;
        let title = en::str_of(s, "titulo").to_string();
        let resumo = en::str_of(s, "resumo").to_string();
        let mut changed = false;
        let mut body_rect: Option<Rect> = None;
        let mut extra_hits: Vec<(Rect, &'static str)> = Vec::new();
        let bg = ui.painter().add(egui::Shape::Noop);
        let density = self.esc.density;
        let meta_w = 170.0_f32.min(w * 0.3);
        let main_w = w - 26.0 - 14.0 - meta_w - 16.0;
        let inner = ui.horizontal_top(|ui| {
            ui.set_width(w);
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(vec2(26.0, 20.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(num).size(12.0).color(p.faint));
            });
            ui.allocate_ui_with_layout(vec2(main_w, 20.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(main_w);
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add_space(9.0);
                // the title
                if editing == Some(Field::Title) {
                    let id = egui::Id::new(("esc-t", &sid));
                    if ui.memory(|m| m.has_focus(id)) {
                        self.title_keys(ui, &sid, place, title.is_empty() && resumo.is_empty(), evs);
                    }
                    if want_focus {
                        focus_end(ui.ctx(), id, title.chars().count());
                        self.esc.focus = false;
                    }
                    let mut t = title.clone();
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut t)
                            .id(id)
                            .frame(false)
                            .margin(egui::Margin::ZERO)
                            .desired_width(f32::INFINITY)
                            .font(kit::medium(13.5))
                            .hint_text(RichText::new("Título  ·  Enter: próxima cena  ·  Tab: escrever mais").color(p.faint)),
                    );
                    if want_focus {
                        r.scroll_to_me(None);
                    }
                    if r.changed() {
                        if t.contains('\n') {
                            evs.push(Ev::Split(sid.clone(), place.clone(), t));
                        } else {
                            self.data["cenas"][si]["titulo"] = json!(t);
                            changed = true;
                        }
                    }
                    if r.lost_focus() {
                        evs.push(Ev::Blur(sid.clone(), Field::Title));
                    }
                } else if !title.is_empty() || resumo.is_empty() {
                    let (txt, c) = if title.is_empty() { ("Cena sem título", p.faint) } else { (title.as_str(), p.text) };
                    ui.add(egui::Label::new(RichText::new(txt).font(kit::medium(13.5)).color(c)).wrap().selectable(false));
                }
                // the text
                if editing == Some(Field::Body) {
                    let id = egui::Id::new(("esc-b", &sid));
                    if ui.memory(|m| m.has_focus(id)) {
                        self.body_keys(ui, &sid, resumo.is_empty(), evs);
                    }
                    if want_focus {
                        focus_end(ui.ctx(), id, resumo.chars().count());
                        self.esc.focus = false;
                    }
                    let mut t = resumo.clone();
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut t)
                            .id(id)
                            .frame(false)
                            .margin(egui::Margin::ZERO)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::proportional(13.0))
                            .text_color(p.text)
                            .hint_text(RichText::new("O que acontece — uma frase ou a cena inteira. Esc sai · Tab vai para a próxima cena").color(p.faint)),
                    );
                    if want_focus {
                        r.scroll_to_me(None);
                    }
                    if r.changed() {
                        self.data["cenas"][si]["resumo"] = json!(t);
                        changed = true;
                    }
                    if r.lost_focus() {
                        evs.push(Ev::Blur(sid.clone(), Field::Body));
                    }
                    body_rect = Some(r.rect);
                } else if !resumo.is_empty() && (density > 0 || title.is_empty()) {
                    let rows = match (density, title.is_empty()) {
                        (0, _) => 1,
                        (1, true) => 4,
                        (1, false) => 3,
                        _ => usize::MAX,
                    };
                    let color = if title.is_empty() { p.text } else { p.muted };
                    let mut job = egui::text::LayoutJob::single_section(resumo.clone(), egui::TextFormat { font_id: egui::FontId::proportional(if title.is_empty() { 13.0 } else { 12.5 }), color, ..Default::default() });
                    job.wrap = egui::text::TextWrapping { max_width: main_w, max_rows: rows, break_anywhere: false, overflow_character: Some('…') };
                    let g = ui.fonts(|f| f.layout_job(job));
                    let (r, _) = ui.allocate_exact_size(vec2(main_w, g.size().y), Sense::hover());
                    ui.painter().galley(r.min, g, color);
                    body_rect = Some(r);
                }
                ui.add_space(9.0);
            });
            ui.vertical(|ui| {
                ui.add_space(9.0);
                ui.allocate_ui_with_layout(vec2(meta_w, 20.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (ir, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                    extra_hits.push((ir, "details"));
                    for t in en::tramas_of(s).iter().rev() {
                        let (r, resp) = ui.allocate_exact_size(vec2(9.0, 16.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, self.trama_color(t, p));
                        if let Some(tt) = en::trama(&self.data, t) {
                            resp.on_hover_text(en::str_of(tt, "nome"));
                        }
                    }
                    let pov = en::str_of(s, "pov");
                    if !pov.is_empty() {
                        ui.add(egui::Label::new(RichText::new(pov).size(11.5).color(p.muted)).truncate());
                    }
                    let q = en::str_of(s, "quando");
                    if !q.is_empty() {
                        ui.add(egui::Label::new(RichText::new(q).size(11.5).color(p.faint)).truncate());
                    }
                    if !matches!(place, Place::Cap(_)) {
                        let r = kit::quiet(ui, Some(icons::ARROW_RIGHT), "capítulo", p).on_hover_text("Criar um capítulo a partir desta cena");
                        extra_hits.push((r.rect, "chapter"));
                    }
                });
            });
        });
        let rr = inner.response.rect;
        let sel = matches!(&self.sel, Some(Sel::Cena(id)) if id == &sid);
        let ghost = dragging == Some(sid.as_str());
        let mut hovered = false;
        if editing.is_none() {
            let resp = ui.interact(rr, ui.id().with(("esc", &sid)), Sense::click_and_drag());
            hovered = resp.hovered() && dragging.is_none();
            if resp.clicked() {
                let at = resp.interact_pointer_pos().unwrap_or(rr.min);
                let field = if body_rect.is_some_and(|b| at.y >= b.top() - 2.0) { Field::Body } else { Field::Title };
                evs.push(Ev::Edit(sid.clone(), field));
            }
            if resp.double_clicked() {
                evs.push(Ev::Details(sid.clone()));
            }
            Self::drag_source(ui, &resp, &sid, None);
            Self::card_menu(&resp, &sid, place.cap(), en::str_of(s, "status"), p, card_cmd);
        }
        // small targets over the row (registered last, so they get the click)
        for (r, what) in extra_hits {
            let h = ui.interact(r, ui.id().with(("esc-hit", &sid, what)), Sense::click());
            if what == "details" {
                if hovered || h.hovered() || editing.is_some() || sel {
                    let c = if h.hovered() || sel { p.text } else { p.faint };
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, icons::PANEL_RIGHT_OPEN, kit::icon_font(13.0), c);
                }
                if h.clicked() {
                    evs.push(Ev::Details(sid.clone()));
                }
                h.on_hover_text("Detalhes da cena (Ctrl+Enter)");
            } else if h.clicked() {
                evs.push(Ev::ToChapter(sid.clone()));
            }
        }
        let fill = if editing.is_some() || sel {
            p.accent_soft
        } else if hovered {
            p.bg_hover
        } else {
            Color32::TRANSPARENT
        };
        let mut shapes = vec![egui::Shape::rect_filled(rr.shrink(1.0), 8.0, if ghost { p.bg_secondary } else { fill })];
        shapes.push(egui::Shape::rect_filled(Rect::from_min_size(rr.min + vec2(0.0, 10.0), vec2(3.0, (rr.height() - 20.0).max(4.0))), 2.0, status_color(en::str_of(s, "status"))));
        if k > 0 {
            shapes.push(egui::Shape::line_segment([rr.left_top() + vec2(14.0, 0.0), rr.right_top() - vec2(14.0, 0.0)], Stroke::new(1.0_f32, p.border)));
        }
        ui.painter().set(bg, egui::Shape::Vec(shapes));
        (rr, sid, changed, editing.is_some())
    }

    fn title_keys(&mut self, ui: &egui::Ui, sid: &str, place: &Place, empty: bool, evs: &mut Vec<Ev>) {
        if take_key(ui, Key::Enter, cmd) {
            evs.push(Ev::Details(sid.into()));
        } else if take_key(ui, Key::Enter, plain) {
            evs.push(Ev::NewAfter(sid.into(), place.clone()));
        } else if take_key(ui, Key::Enter, |m| m.shift) || take_key(ui, Key::Tab, |m| !m.shift) {
            evs.push(Ev::Edit(sid.into(), Field::Body));
        }
        if take_key(ui, Key::ArrowUp, |m| m.alt) {
            evs.push(Ev::Move(sid.into(), -1));
        } else if take_key(ui, Key::ArrowDown, |m| m.alt) {
            evs.push(Ev::Move(sid.into(), 1));
        } else if take_key(ui, Key::ArrowUp, plain) || take_key(ui, Key::Tab, |m| m.shift) {
            evs.push(Ev::Nav(sid.into(), -1));
        } else if take_key(ui, Key::ArrowDown, plain) {
            evs.push(Ev::Nav(sid.into(), 1));
        }
        if empty && take_key(ui, Key::Backspace, plain) {
            evs.push(Ev::Delete(sid.into()));
        }
    }

    fn body_keys(&mut self, ui: &egui::Ui, sid: &str, empty: bool, evs: &mut Vec<Ev>) {
        if take_key(ui, Key::Enter, cmd) {
            evs.push(Ev::Details(sid.into()));
        }
        if take_key(ui, Key::Tab, |m| m.shift) || (empty && take_key(ui, Key::Backspace, plain)) {
            evs.push(Ev::Edit(sid.into(), Field::Title));
        } else if take_key(ui, Key::Tab, |m| !m.shift) {
            evs.push(Ev::Nav(sid.into(), 1));
        }
        if take_key(ui, Key::ArrowUp, |m| m.alt) {
            evs.push(Ev::Move(sid.into(), -1));
        } else if take_key(ui, Key::ArrowDown, |m| m.alt) {
            evs.push(Ev::Move(sid.into(), 1));
        }
    }

    /// The open line at the end of a group.
    #[allow(clippy::too_many_arguments)]
    fn esc_add(&mut self, ui: &mut egui::Ui, p: Palette, place: &Place, w: f32, has_rows: bool, flat: &[(String, Place)], evs: &mut Vec<Ev>) {
        let key = place.key();
        let id = egui::Id::new(("esc-add", &key));
        let focused = ui.memory(|m| m.has_focus(id));
        let mut text = self.esc.drafts.get(&key).cloned().unwrap_or_default();
        if focused {
            if take_key(ui, Key::Enter, plain) {
                if !text.trim().is_empty() {
                    evs.push(Ev::Capture(place.clone(), std::mem::take(&mut text)));
                }
            } else if text.is_empty() && take_key(ui, Key::ArrowUp, plain) {
                // to the last scene of this group
                if let Some(t) = flat.iter().rposition(|x| &x.1 == place).and_then(|t| flat.get(t)) {
                    evs.push(Ev::Edit(t.0.clone(), Field::Title));
                }
            }
        }
        if has_rows {
            let r = ui.painter().add(egui::Shape::Noop);
            let y = ui.cursor().top();
            ui.painter().set(r, egui::Shape::line_segment([pos2(ui.max_rect().left() + 14.0, y), pos2(ui.max_rect().right() - 14.0, y)], Stroke::new(1.0_f32, p.border)));
        }
        ui.horizontal_top(|ui| {
            ui.set_width(w);
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(vec2(26.0, 20.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add_space(9.0);
                ui.label(kit::icon(icons::PLUS, 13.0).color(if focused { p.accent_strong } else { p.faint }));
            });
            ui.vertical(|ui| {
                ui.set_width(w - 26.0 - 14.0 - 16.0);
                ui.add_space(8.0);
                let want = self.esc.focus_add.as_deref() == Some(key.as_str());
                if want {
                    focus_end(ui.ctx(), id, text.chars().count());
                    self.esc.focus_add = None;
                }
                let hint = match place {
                    Place::Range(..) => "Outra cena desta faixa…",
                    _ if has_rows => "Nova cena…",
                    Place::Ideas => "Uma ideia de cena, ou cole uma lista…",
                    _ => "Escreva a primeira cena, ou cole uma lista…",
                };
                let r = ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .id(id)
                        .frame(false)
                        .margin(egui::Margin::ZERO)
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .font(kit::medium(13.5))
                        .return_key(egui::KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter))
                        .hint_text(RichText::new(hint).color(p.faint)),
                );
                if want {
                    r.scroll_to_me(Some(egui::Align::Center));
                }
                if r.has_focus() || focused {
                    ui.add_space(2.0);
                    ui.label(RichText::new("Enter cria  ·  Shift+Enter quebra a linha  ·  lista colada = várias cenas  ·  @pov  #trama  ^capítulo  ^3-6").size(11.0).color(p.faint));
                }
                ui.add_space(8.0);
            });
        });
        if text.is_empty() {
            self.esc.drafts.remove(&key);
        } else {
            self.esc.drafts.insert(key, text);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn esc_events(&mut self, evs: Vec<Ev>, flat: &[(String, Place)], all: &[Col], chars: &[String], b: &Book, root: &Path, ix: &Index, acts: &mut Vec<Action>) {
        let place_of = |id: &str| flat.iter().find(|x| x.0 == id).map(|x| x.1.clone());
        for ev in evs {
            match ev {
                Ev::Edit(id, f) => {
                    if let Some(pl) = place_of(&id) {
                        if self.esc.edit.as_ref().is_some_and(|e| e.0 != id || e.1 != f) {
                            if let Some((old, Field::Title)) = self.esc.edit.clone() {
                                if let Some(op) = place_of(&old) {
                                    self.apply_tokens(&old, &op, all, chars);
                                }
                            }
                        }
                        self.esc.last = Some(pl);
                    }
                    if matches!(self.sel, Some(Sel::Cena(_))) {
                        self.sel = Some(Sel::Cena(id.clone()));
                    }
                    self.esc.edit = Some((id, f));
                    self.esc.focus = true;
                }
                Ev::Blur(id, f) => {
                    if self.esc.edit.as_ref() == Some(&(id.clone(), f)) && !self.esc.focus {
                        if f == Field::Title {
                            if let Some(pl) = place_of(&id) {
                                self.apply_tokens(&id, &pl, all, chars);
                            }
                        }
                        self.esc.edit = None;
                    }
                }
                Ev::NewAfter(id, pl) => {
                    let empty = en::scene_index(&self.data, &id).is_some_and(|i| en::str_of(&self.data["cenas"][i], "titulo").trim().is_empty() && en::str_of(&self.data["cenas"][i], "resumo").trim().is_empty());
                    if empty {
                        // Enter on an empty scene ends the run: back to the open line
                        en::delete_scene(&mut self.data, &id);
                        self.esc.edit = None;
                        self.esc.focus_add = Some(pl.key());
                        self.changed();
                        continue;
                    }
                    self.apply_tokens(&id, &pl, all, chars);
                    let now = en::scene_index(&self.data, &id).map(|i| {
                        let s = &self.data["cenas"][i];
                        match (en::cap_of(s), en::range_of(s)) {
                            (Some(c), _) => Place::Cap(c.to_string()),
                            (None, Some((a, b))) => Place::Range(a.to_string(), b.to_string()),
                            _ => Place::Ideas,
                        }
                    });
                    let pl = now.unwrap_or(pl);
                    self.snapshot();
                    let nid = self.create_draft(&pl, &Draft::default(), all, chars, Some(&id));
                    self.esc.edit = Some((nid, Field::Title));
                    self.esc.focus = true;
                    self.esc.last = Some(pl);
                    self.changed();
                }
                Ev::Capture(pl, text) => {
                    let drafts = en::parse_capture(&text);
                    if drafts.is_empty() {
                        continue;
                    }
                    self.snapshot();
                    for d in &drafts {
                        self.create_draft(&pl, d, all, chars, None);
                    }
                    self.esc.focus_add = Some(pl.key());
                    self.esc.last = Some(pl);
                    self.changed();
                }
                Ev::Split(id, pl, text) => {
                    let drafts = en::parse_capture(&text);
                    let Some((first, rest)) = drafts.split_first() else { continue };
                    self.snapshot();
                    if let Some(i) = en::scene_index(&self.data, &id) {
                        self.data["cenas"][i]["titulo"] = json!(first.titulo);
                        if !first.resumo.is_empty() {
                            let old = en::str_of(&self.data["cenas"][i], "resumo").to_string();
                            self.data["cenas"][i]["resumo"] = json!(if old.is_empty() { first.resumo.clone() } else { format!("{old}\n\n{}", first.resumo) });
                        }
                    }
                    self.fill_tokens(&id, first, chars);
                    let mut after = id.clone();
                    for d in rest {
                        after = self.create_draft(&pl, d, all, chars, Some(&after));
                    }
                    self.esc.edit = Some((after, Field::Title));
                    self.esc.focus = true;
                    self.changed();
                }
                Ev::Delete(id) => {
                    let k = flat.iter().position(|x| x.0 == id);
                    let pl = place_of(&id).unwrap_or(Place::Ideas);
                    let prev = k.and_then(|k| k.checked_sub(1)).map(|k| flat[k].clone()).filter(|x| x.1 == pl);
                    self.snapshot();
                    en::delete_scene(&mut self.data, &id);
                    match prev {
                        Some((pid, _)) => {
                            self.esc.edit = Some((pid, Field::Title));
                            self.esc.focus = true;
                        }
                        None => {
                            self.esc.edit = None;
                            self.esc.focus_add = Some(pl.key());
                        }
                    }
                    if self.sel == Some(Sel::Cena(id)) {
                        self.sel = None;
                    }
                    self.changed();
                }
                Ev::Move(id, dir) => {
                    self.move_in_outline(&id, dir, flat);
                    self.esc.focus = true;
                }
                Ev::Nav(id, dir) => {
                    let Some(k) = flat.iter().position(|x| x.0 == id) else { continue };
                    let t = k as i64 + dir as i64;
                    if let Some(pl) = place_of(&id) {
                        self.apply_tokens(&id, &pl, all, chars);
                    }
                    if t >= 0 && (t as usize) < flat.len() {
                        self.esc.edit = Some((flat[t as usize].0.clone(), Field::Title));
                        self.esc.focus = true;
                    } else if dir > 0 {
                        self.esc.edit = None;
                        self.esc.focus_add = Some(flat[k].1.key());
                    }
                }
                Ev::Details(id) => {
                    self.sel = Some(Sel::Cena(id));
                    self.confirm = None;
                }
                Ev::FocusAdd(key) => self.esc.focus_add = Some(key),
                Ev::Select(s) => {
                    self.sel = Some(s);
                    self.confirm = None;
                }
                Ev::Open(c) => acts.push(Action::Open(c, true)),
                Ev::ToggleArc(a) => self.arco = if self.arco.as_ref() == Some(&a) { None } else { Some(a) },
                Ev::ToChapter(sid) => {
                    let Some(i) = en::scene_index(&self.data, &sid) else { continue };
                    let t = en::str_of(&self.data["cenas"][i], "titulo");
                    let title = if t.is_empty() { "Novo capítulo".to_string() } else { t.to_string() };
                    match book::create_chapter(root, ix, b, &title, "", "capitulo") {
                        Ok(path) => {
                            en::move_scene(&mut self.data, &sid, Some(&path), None, None);
                            self.changed();
                            acts.push(Action::Saved(path));
                        }
                        Err(e) => eprintln!("capítulo: {e}"),
                    }
                }
            }
        }
    }

    /// Alt+↑/↓: one step up or down the outline; past the first or last
    /// scene of a group, into the next group.
    fn move_in_outline(&mut self, id: &str, dir: i32, flat: &[(String, Place)]) {
        let mut groups: Vec<(Place, Vec<String>)> = Vec::new();
        for (sid, pl) in flat {
            match groups.last_mut() {
                Some(g) if &g.0 == pl => g.1.push(sid.clone()),
                _ => groups.push((pl.clone(), vec![sid.clone()])),
            }
        }
        // empty chapters count too: moving down from the last scene of a
        // chapter lands in the next chapter even when it has no scenes
        let Some(gi) = groups.iter().position(|g| g.1.iter().any(|x| x == id)) else { return };
        let k = groups[gi].1.iter().position(|x| x == id).unwrap_or(0);
        let all_places: Vec<Place> = self.esc_places();
        let here = groups[gi].0.clone();
        let pi = all_places.iter().position(|x| x == &here);
        self.snapshot();
        if dir < 0 {
            if k > 0 {
                let before = groups[gi].1[k - 1].clone();
                self.place_scene(id, &here, Some(&before));
            } else if let Some(prev) = pi.and_then(|pi| pi.checked_sub(1)).map(|i| all_places[i].clone()) {
                self.place_scene(id, &prev, None);
            } else {
                self.esc.undo.pop();
                return;
            }
        } else if k + 1 < groups[gi].1.len() {
            let before = groups[gi].1.get(k + 2).cloned();
            self.place_scene(id, &here, before.as_deref());
        } else if let Some(next) = pi.and_then(|pi| all_places.get(pi + 1)).cloned() {
            let first = groups.iter().find(|g| g.0 == next).and_then(|g| g.1.first().cloned());
            self.place_scene(id, &next, first.as_deref());
        } else {
            self.esc.undo.pop();
            return;
        }
        self.changed();
    }

    /// The groups of the outline in order, as last drawn (chapters and
    /// bands with scenes, then ideas), so a scene can move into an empty chapter.
    fn esc_places(&self) -> Vec<Place> {
        self.esc.order.clone()
    }
}
