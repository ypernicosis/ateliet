//! Enredo, native: the plot of a book. Grade (plot lines × chapters, scenes
//! at the crossings), Escaleta (the step outline), and a side drawer with
//! the selected scene, plot line or planned chapter.

use super::library::Cx;
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
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Grade,
    Escaleta,
    Cronologia,
    Pistas,
    Arcos,
    Mural,
    Relacoes,
}

pub const MODES: [(Mode, &str, &str); 7] = [
    (Mode::Grade, icons::LAYOUT_GRID, "Grade"),
    (Mode::Escaleta, icons::LIST_ORDERED, "Escaleta"),
    (Mode::Cronologia, icons::CALENDAR_RANGE, "Cronologia"),
    (Mode::Pistas, icons::KEY_ROUND, "Pistas"),
    (Mode::Arcos, icons::TRENDING_UP, "Arcos"),
    (Mode::Mural, icons::STICKY_NOTE, "Mural"),
    (Mode::Relacoes, icons::NETWORK, "Relações"),
];

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Sel {
    Cena(String),
    Trama(String),
    Plan(String),
    Pista(String),
    Arco(String),
    Evento(String),
    /// `application:beat`
    Batida(String),
}

pub enum CardCmd {
    Open(String),
    Duplicate,
    ToIdeas,
    Status(String),
    Delete,
}

/// The scene being dragged (and the plot line row it left).
#[derive(Clone)]
struct Drag {
    id: String,
    from: Option<String>,
}

/// Where a dragged scene can land.
pub(super) struct Zone {
    rect: Rect,
    cap: Option<String>,
    trama: Option<String>,
    cards: Vec<(Rect, String)>,
}

pub struct EnredoView {
    pub book: String,
    pub mode: Mode,
    pub(super) data: Json,
    loaded: bool,
    mtime: Option<SystemTime>,
    dirty: bool,
    last_edit: Option<Instant>,
    pub(super) sel: Option<Sel>,
    filter_tramas: Option<Vec<String>>,
    filter_pov: String,
    filter_status: String,
    arco: Option<String>,
    pub(super) focus_title: bool,
    pub(super) new_trama: Option<(String, Option<String>)>,
    pub(super) era: Option<super::enredo_more::EraEdit>,
    pub(super) arc_pick: bool,
    pub(super) crono_zoom: Option<f32>,
    pub(super) reading_order: bool,
    pub(super) confirm: Option<String>,
    /// The structures applied to the book, with where their beats fall.
    pub(super) apps: Vec<super::enredo_st::App>,
    pub(super) st_cache: std::collections::HashMap<String, (Option<SystemTime>, ateliet_core::scriptorium::estruturas::Structure)>,
    pub(super) st_dialog: Option<super::enredo_st::StDialog>,
    pub(super) st_editor: Option<Box<super::enredo_st::StEditor>>,
    pub(super) st_folder: String,
    pub(super) board_state: super::enredo_board::Board,
    pub(super) pending_open: Option<String>,
}

pub fn status_color(k: &str) -> Color32 {
    super::color(book::status_color(if k.is_empty() { "ideia" } else { k }))
}

pub fn str_field(ui: &mut egui::Ui, obj: &mut Json, key: &str, edit: impl FnOnce(&mut egui::Ui, &mut String) -> egui::Response) -> (bool, egui::Response) {
    let mut t = obj[key].as_str().unwrap_or("").to_string();
    let r = edit(ui, &mut t);
    if r.changed() {
        obj[key] = json!(t);
        return (true, r);
    }
    (false, r)
}

pub fn note_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string()
}

pub(super) fn truncated(ui: &egui::Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(8.0));
    ui.fonts(|f| f.layout_job(job))
}

impl EnredoView {
    pub fn new(book: &str) -> EnredoView {
        EnredoView {
            book: book.into(),
            mode: Mode::Grade,
            data: en::blank(),
            loaded: false,
            mtime: None,
            dirty: false,
            last_edit: None,
            sel: None,
            filter_tramas: None,
            filter_pov: String::new(),
            filter_status: String::new(),
            arco: None,
            focus_title: false,
            new_trama: None,
            era: None,
            arc_pick: false,
            crono_zoom: None,
            reading_order: false,
            confirm: None,
            apps: Vec::new(),
            st_cache: Default::default(),
            st_dialog: None,
            st_editor: None,
            st_folder: String::new(),
            board_state: Default::default(),
            pending_open: None,
        }
    }

    pub fn changed(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
    }

    fn file_mtime(root: &Path, b: &Book) -> Option<SystemTime> {
        std::fs::metadata(root.join(en::path(b))).and_then(|m| m.modified()).ok()
    }

    /// Load the first time, and again when the file changed on disk.
    fn ensure(&mut self, root: &Path, b: &Book) {
        let m = Self::file_mtime(root, b);
        if !self.loaded || (!self.dirty && m != self.mtime) {
            self.data = en::load(root, b);
            self.mtime = m;
            self.loaded = true;
        }
    }

    pub fn save(&mut self, root: &Path, ix: &Index, author: &str) -> Option<String> {
        if !self.dirty {
            return None;
        }
        let b = book::book_by_path(ix, &self.book, author)?;
        if en::save(root, ix, &b, &self.data).is_ok() {
            self.dirty = false;
            self.mtime = Self::file_mtime(root, &b);
            return Some(en::path(&b));
        }
        None
    }

    pub fn title(&self) -> String {
        "Enredo".into()
    }

    pub(super) fn passes(&self, s: &Json) -> bool {
        if !self.filter_pov.is_empty() && en::str_of(s, "pov") != self.filter_pov {
            return false;
        }
        if !self.filter_status.is_empty() && en::str_of(s, "status").is_empty().then_some("ideia").unwrap_or(en::str_of(s, "status")) != self.filter_status {
            return false;
        }
        if let Some(f) = &self.filter_tramas {
            let tr = en::tramas_of(s);
            if !tr.iter().any(|t| f.iter().any(|x| x == t)) && !(tr.is_empty() && f.iter().any(|x| x == "__none")) {
                return false;
            }
        }
        true
    }

    fn filtered(&self) -> bool {
        self.filter_tramas.is_some() || !self.filter_pov.is_empty() || !self.filter_status.is_empty()
    }

    /// The visible chapters: the chosen arc, or all.
    fn window(&mut self, all: &[Col]) -> (usize, usize) {
        if let Some(a) = &self.arco {
            let idx: Vec<usize> = all.iter().enumerate().filter(|(_, c)| &c.part == a).map(|(i, _)| i).collect();
            if let (Some(lo), Some(hi)) = (idx.first(), idx.last()) {
                return (*lo, *hi + 1);
            }
            self.arco = None;
        }
        (0, all.len())
    }

    pub(super) fn trama_color(&self, id: &str, p: Palette) -> Color32 {
        en::trama(&self.data, id).map(|t| super::color(en::str_of(t, "cor"))).unwrap_or(p.faint)
    }

    pub(super) fn new_scene(&mut self, cap: Option<&str>, tramas: &[&str]) {
        let id = en::new_scene(&mut self.data, cap, tramas);
        self.sel = Some(Sel::Cena(id));
        self.focus_title = true;
        self.changed();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let root = cx.vault.root.clone();
        let author = cx.sc.author();
        let ix_lock = cx.vault.index.clone();
        let ix = ix_lock.read().unwrap();
        let Some(b) = book::book_by_path(&ix, &self.book, &author) else {
            ui.label("Livro não encontrado.");
            return acts;
        };
        self.ensure(&root, &b);
        if self.dirty && self.last_edit.is_some_and(|t| t.elapsed() > Duration::from_millis(700)) {
            if let Some(path) = self.save(&root, &ix, &author) {
                acts.push(Action::Saved(path));
            }
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(400));
        }
        let cols = en::chapters(&ix, &b, &self.data);
        let chars: Vec<String> = book::characters(&ix, &b);
        let words: Vec<usize> = {
            let stats = cx.sc.stats(&ix, &b);
            cols.iter().map(|c| stats.per.iter().find(|x| x.info.path == c.path).map(|x| x.words).unwrap_or(0)).collect()
        };
        self.st_folder = ateliet_core::scriptorium::estruturas::folder(&cx.sc.settings.str("pastaEstruturas", ""));
        self.resolve(&root, &cols, &words);
        self.keys(ui);

        egui::TopBottomPanel::top(ui.id().with("en-bar")).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 20, right: 16, top: 12, bottom: 10 })).show_separator_line(true).show_inside(ui, |ui| {
            self.bar(ui, p, &b, &cols);
        });
        if self.sel.is_some() {
            let r = egui::SidePanel::right(ui.id().with("en-drawer")).resizable(false).exact_width(372.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 22, right: 10, top: 16, bottom: 12 })).show_inside(ui, |ui| {
                self.drawer(ui, p, &b, &cols, &chars, &root, &ix, &mut acts);
            });
            let rr = r.response.rect;
            ui.painter().vline(rr.left(), rr.y_range(), Stroke::new(1.0_f32, p.border));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show_inside(ui, |ui| match self.mode {
            Mode::Mural => self.board(ui, p, cx, &b, "mural", &chars, &mut acts),
            Mode::Relacoes => self.board(ui, p, cx, &b, "relacoes", &chars, &mut acts),
            Mode::Grade => self.grade(ui, p, &b, &cols, &chars, &mut acts),
            Mode::Escaleta => self.escaleta(ui, p, &b, &cols, &root, &ix, &mut acts),
            Mode::Cronologia => self.cronologia(ui, p, &b, &cols),
            Mode::Pistas => self.pistas(ui, p, &b, &cols),
            Mode::Arcos => self.arcos(ui, p, &b, &cols, &ix),
        });
        self.trama_dialog(ui.ctx(), p);
        self.era_dialog(ui.ctx(), p);
        self.arc_dialog(ui.ctx(), p, &ix);
        self.structures_dialog(ui.ctx(), p, &cols, &ix, &root);
        self.structure_editor(ui.ctx(), p, &ix, &root, &author, &mut acts);
        if let Some(t) = self.pending_open.take() {
            acts.push(Action::OpenLink(t, b.path.clone(), true));
        }
        acts
    }

    fn keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().memory(|m| m.focused().is_some() || m.any_popup_open()) || self.new_trama.is_some() || self.era.is_some() || self.arc_pick || self.st_dialog.is_some() || self.st_editor.is_some() {
            return;
        }
        let (esc, n, num) = ui.input(|i| {
            let num = [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7].iter().position(|k| i.key_pressed(*k) && !i.modifiers.command);
            (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::N) && !i.modifiers.command && !i.modifiers.ctrl, num)
        });
        if esc {
            self.sel = None;
            self.confirm = None;
        }
        if n {
            self.primary();
        }
        if let Some(k) = num {
            self.mode = MODES[k].0;
        }
    }

    fn primary(&mut self) {
        match self.mode {
            Mode::Grade | Mode::Escaleta => self.new_scene(None, &[]),
            Mode::Cronologia => self.new_event(),
            Mode::Pistas => self.new_pista(),
            Mode::Arcos => self.new_arc(),
            Mode::Mural | Mode::Relacoes => {}
        }
    }

    // ── bar ───────────────────────────────────────────────────────────────

    fn bar(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, cols: &[Col]) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Enredo").font(kit::bold(17.0)).color(p.text));
                ui.label(RichText::new(&b.title).size(11.5).color(p.faint));
            });
            ui.add_space(14.0);
            // narrow tabs: icons only, so the chips on the right keep their room
            let wide = ui.available_width() > 900.0;
            let roomy = ui.available_width() > 1040.0;
            let short = |t: &str| if roomy { t.to_string() } else { String::new() };
            let items: Vec<(&str, &str)> = MODES.iter().map(|m| (m.1, if wide { m.2 } else { "" })).collect();
            let cur = MODES.iter().position(|m| m.0 == self.mode).unwrap_or(0);
            if let Some(i) = kit::segmented(ui, &items, cur, p) {
                self.mode = MODES[i].0;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let lab = match self.mode {
                    Mode::Grade | Mode::Escaleta => "Cena",
                    Mode::Cronologia => "Evento",
                    Mode::Pistas => "Pista",
                    Mode::Arcos => "Arco",
                    Mode::Mural | Mode::Relacoes => "",
                };
                if !lab.is_empty() && kit::primary_icon(ui, icons::PLUS, lab, p).on_hover_text(format!("Nova {} (N)", lab.to_lowercase())).clicked() {
                    self.primary();
                }
                if matches!(self.mode, Mode::Grade | Mode::Escaleta | Mode::Arcos) {
                    let lab = match self.apps.len() {
                        0 => "Sem estrutura".to_string(),
                        1 => self.apps[0].st.nome.clone(),
                        n => format!("{n} estruturas"),
                    };
                    let tip = if self.apps.is_empty() { "Escolher estruturas narrativas".to_string() } else { self.apps.iter().map(|x| format!("{} — {}", x.st.nome, ateliet_core::scriptorium::estruturas::scope_label(&x.a, cols))).collect::<Vec<_>>().join("\n") };
                    if kit::chip(ui, Some(icons::MILESTONE), &short(&lab), !self.apps.is_empty(), p).on_hover_text(format!("{lab}\n{tip}")).clicked() {
                        self.st_dialog = Some(super::enredo_st::StDialog::default());
                    }
                }
                if matches!(self.mode, Mode::Grade | Mode::Escaleta | Mode::Cronologia) {
                    let fr = kit::chip(ui, Some(icons::LIST_FILTER), &short(if self.filtered() { "Filtrado" } else { "Filtrar" }), self.filtered(), p).on_hover_text("Filtrar");
                    let popup = ui.make_persistent_id("en-filter");
                    if fr.clicked() {
                        ui.memory_mut(|m| m.toggle_popup(popup));
                    }
                    egui::popup_below_widget(ui, popup, &fr, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                        ui.set_min_width(220.0);
                        self.filter_menu(ui, p);
                    });
                }
                if matches!(self.mode, Mode::Grade | Mode::Escaleta) {
                    let parts = en::parts(cols);
                    if !parts.is_empty() {
                        let ar = kit::chip(ui, Some(icons::BOOKMARK), &short(self.arco.as_deref().unwrap_or("Todos os arcos")), self.arco.is_some(), p).on_hover_text(self.arco.as_deref().unwrap_or("Todos os arcos").to_string());
                        let popup = ui.make_persistent_id("en-arc");
                        if ar.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(popup));
                        }
                        egui::popup_below_widget(ui, popup, &ar, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(200.0);
                            if kit::Row::new("Todos os arcos").selected(self.arco.is_none()).show(ui, p).clicked() {
                                self.arco = None;
                            }
                            for part in parts {
                                if kit::Row::new(&part).icon(icons::BOOKMARK).selected(self.arco.as_ref() == Some(&part)).show(ui, p).clicked() {
                                    self.arco = Some(part.clone());
                                }
                            }
                        });
                    }
                }
            });
        });
    }

    fn filter_menu(&mut self, ui: &mut egui::Ui, p: Palette) {
        if kit::Row::new("Mostrar tudo").icon(icons::ERASER).show(ui, p).clicked() {
            self.filter_tramas = None;
            self.filter_pov.clear();
            self.filter_status.clear();
        }
        let tramas: Vec<(String, String, String)> = self.data["tramas"].as_array().into_iter().flatten().map(|t| (en::str_of(t, "id").to_string(), en::str_of(t, "nome").to_string(), en::str_of(t, "cor").to_string())).collect();
        if !tramas.is_empty() {
            kit::section(ui, "Tramas", p);
            for (id, nome, cor) in &tramas {
                let on = self.filter_tramas.as_ref().is_none_or(|f| f.contains(id));
                if kit::Row::new(nome).dot(super::color(cor)).dim(!on).trailing(if on { "✓" } else { "" }).show(ui, p).clicked() {
                    let all: Vec<String> = tramas.iter().map(|t| t.0.clone()).collect();
                    let mut cur = self.filter_tramas.clone().unwrap_or_else(|| all.clone());
                    if on {
                        cur.retain(|x| x != id);
                    } else {
                        cur.push(id.clone());
                    }
                    self.filter_tramas = if cur.len() == all.len() { None } else { Some(cur) };
                }
            }
        }
        let mut povs: Vec<String> = self.data["cenas"].as_array().into_iter().flatten().map(|s| en::str_of(s, "pov").to_string()).filter(|s| !s.is_empty()).collect();
        povs.sort();
        povs.dedup();
        if !povs.is_empty() {
            kit::section(ui, "Ponto de vista", p);
            for pov in povs {
                let on = self.filter_pov == pov;
                if kit::Row::new(&pov).icon(icons::EYE).selected(on).show(ui, p).clicked() {
                    self.filter_pov = if on { String::new() } else { pov.clone() };
                }
            }
        }
        kit::section(ui, "Status", p);
        for (k, label, c) in STATUS {
            let on = self.filter_status == k;
            if kit::Row::new(label).dot(super::color(c)).selected(on).show(ui, p).clicked() {
                self.filter_status = if on { String::new() } else { k.to_string() };
            }
        }
    }

    // ── scene card ────────────────────────────────────────────────────────

    pub(super) fn paint_card(&self, ui: &egui::Ui, rect: Rect, s: &Json, p: Palette, hovered: bool, ghost: bool) {
        let painter = ui.painter();
        let sel = matches!(&self.sel, Some(Sel::Cena(id)) if s["id"] == id.as_str());
        let alpha = if ghost { 0.35 } else { 1.0 };
        if !ghost {
            painter.rect_filled(rect.translate(vec2(0.0, 1.0)), 8.0, Color32::from_black_alpha(if p.dark { 40 } else { 10 }));
        }
        painter.rect(rect, 8.0, p.bg, Stroke::new(if sel { 1.5_f32 } else { 1.0 }, if sel { p.accent_strong } else if hovered { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
        let tr = en::tramas_of(s);
        let bar = tr.first().map(|t| self.trama_color(t, p)).unwrap_or(p.border_strong);
        painter.rect_filled(Rect::from_min_size(rect.min + vec2(0.0, 0.0), vec2(3.0, rect.height())), egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, bar.gamma_multiply(alpha));
        let title = en::str_of(s, "titulo");
        let (t, c) = if title.is_empty() { ("Cena sem título", p.faint) } else { (title, p.text) };
        let g = truncated(ui, t, kit::medium(12.5), c.gamma_multiply(alpha), rect.width() - 22.0);
        painter.galley(pos2(rect.left() + 12.0, rect.top() + 8.0), g, c);
        let y = rect.bottom() - 13.0;
        painter.circle_filled(pos2(rect.left() + 15.5, y), 3.5, status_color(en::str_of(s, "status")).gamma_multiply(alpha));
        let mut x = rect.left() + 25.0;
        for t in tr.iter().skip(1).take(4) {
            painter.circle_filled(pos2(x + 2.5, y), 2.5, self.trama_color(t, p).gamma_multiply(alpha));
            x += 8.0;
        }
        let pov = en::str_of(s, "pov");
        if !pov.is_empty() {
            let g = truncated(ui, pov.split(' ').next().unwrap_or(pov), egui::FontId::proportional(11.0), p.muted.gamma_multiply(alpha), rect.right() - x - 10.0);
            painter.galley(pos2(x + 2.0, y - g.size().y / 2.0), g, p.muted);
        }
    }

    /// Right-click on a scene card.
    pub(super) fn card_menu(r: &egui::Response, sid: &str, cap: Option<&str>, status: &str, p: Palette, out: &mut Option<(String, CardCmd)>) {
        r.context_menu(|ui| {
            if let Some(c) = cap.filter(|c| !c.starts_with("plan:")) {
                if kit::menu_item(ui, icons::BOOK_OPEN, "Abrir capítulo", p) {
                    *out = Some((sid.into(), CardCmd::Open(c.into())));
                }
            }
            if kit::menu_item(ui, icons::COPY, "Duplicar", p) {
                *out = Some((sid.into(), CardCmd::Duplicate));
            }
            if cap.is_some() && kit::menu_item(ui, icons::INBOX, "Mover para Ideias", p) {
                *out = Some((sid.into(), CardCmd::ToIdeas));
            }
            kit::menu_sep(ui, p);
            for (k, label, _) in STATUS {
                if kit::menu_check(ui, status == k || (status.is_empty() && k == "ideia"), label, p) {
                    *out = Some((sid.into(), CardCmd::Status(k.into())));
                }
            }
            kit::menu_sep(ui, p);
            if kit::menu_danger(ui, icons::TRASH_2, "Excluir…", p) {
                *out = Some((sid.into(), CardCmd::Delete));
            }
        });
    }

    pub(super) fn card_cmd(&mut self, cmd: Option<(String, CardCmd)>, acts: &mut Vec<Action>) {
        let Some((sid, c)) = cmd else { return };
        match c {
            CardCmd::Open(path) => acts.push(Action::Open(path, true)),
            CardCmd::Duplicate => {
                if let Some(n) = en::duplicate_scene(&mut self.data, &sid) {
                    self.sel = Some(Sel::Cena(n));
                    self.changed();
                }
            }
            CardCmd::ToIdeas => {
                en::move_scene(&mut self.data, &sid, None, None, None);
                self.changed();
            }
            CardCmd::Status(k) => {
                if let Some(i) = en::scene_index(&self.data, &sid) {
                    self.data["cenas"][i]["status"] = json!(k);
                    self.changed();
                }
            }
            CardCmd::Delete => {
                self.sel = Some(Sel::Cena(sid.clone()));
                self.confirm = Some(sid);
            }
        }
    }

    /// Drag a card: set the payload when the drag starts.
    pub(super) fn drag_source(ui: &egui::Ui, r: &egui::Response, id: &str, from: Option<&str>) {
        if r.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), Drag { id: id.into(), from: from.map(String::from) });
        }
    }

    pub(super) fn dragging(ui: &egui::Ui) -> Option<String> {
        egui::DragAndDrop::payload::<Drag>(ui.ctx()).map(|d| d.id.clone())
    }

    /// While a scene is dragged: a ghost under the pointer, the drop spot
    /// marked, the move done on release.
    pub(super) fn drops(&mut self, ui: &egui::Ui, p: Palette, zones: &[Zone]) {
        let Some(d) = egui::DragAndDrop::payload::<Drag>(ui.ctx()) else { return };
        let Some(pos) = ui.ctx().pointer_latest_pos() else { return };
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("en-drop")));
        let target = zones.iter().find(|z| z.rect.contains(pos));
        let mut before = None;
        if let Some(z) = target {
            fg.rect_stroke(z.rect.shrink(2.0), 8.0, Stroke::new(1.5_f32, p.accent_strong.gamma_multiply(0.6)), egui::StrokeKind::Inside);
            let rest: Vec<&(Rect, String)> = z.cards.iter().filter(|c| c.1 != d.id).collect();
            let at = rest.iter().position(|c| c.0.center().y > pos.y);
            before = at.map(|i| rest[i].1.clone());
            let y = match at {
                Some(i) => rest[i].0.top() - 3.0,
                None => rest.last().map(|c| c.0.bottom() + 3.0).unwrap_or(z.rect.top() + 6.0),
            };
            let (l, r) = rest.first().map(|c| (c.0.left(), c.0.right())).unwrap_or((z.rect.left() + 8.0, z.rect.right() - 8.0));
            fg.line_segment([pos2(l, y), pos2(r, y)], Stroke::new(2.0_f32, p.accent_strong));
        }
        if let Some(i) = en::scene_index(&self.data, &d.id) {
            let s = &self.data["cenas"][i];
            let t = en::str_of(s, "titulo");
            let g = fg.layout_no_wrap(if t.is_empty() { "Cena sem título".into() } else { t.to_string() }, kit::medium(12.5), p.text);
            let r = Rect::from_min_size(pos + vec2(10.0, 8.0), vec2(g.size().x + 24.0, 30.0));
            fg.rect(r, 8.0, p.bg, Stroke::new(1.0_f32, p.border_strong), egui::StrokeKind::Inside);
            fg.galley(pos2(r.left() + 12.0, r.center().y - g.size().y / 2.0), g, p.text);
        }
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            if let Some(z) = target {
                let trama = z.trama.as_deref().map(|t| (d.from.as_deref(), t));
                en::move_scene(&mut self.data, &d.id, z.cap.as_deref(), before.as_deref(), trama);
                self.changed();
            }
        }
    }

    // ── Grade ─────────────────────────────────────────────────────────────

    fn empty_grade(&mut self, ui: &mut egui::Ui, p: Palette, chars: &[String]) {
        ui.add_space(90.0);
        ui.vertical_centered(|ui| {
            ui.label(kit::icon(icons::WAYPOINTS, 34.0).color(p.faint));
            ui.add_space(14.0);
            ui.label(RichText::new("Comece pelas tramas").font(kit::medium(17.0)).color(p.text));
            ui.add_space(6.0);
            ui.allocate_ui(vec2(440.0, 80.0), |ui| {
                ui.label(RichText::new("Uma trama é um fio da história: um personagem, um mistério, um conflito. Cada linha da grade é uma trama; cada coluna, um capítulo. As cenas ficam nos cruzamentos.").color(p.muted));
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                let w = if chars.is_empty() { 130.0 } else { 330.0 };
                ui.add_space((ui.available_width() - w) / 2.0);
                if kit::primary_icon(ui, icons::PLUS, "Criar trama", p).clicked() {
                    self.new_trama = Some((String::new(), None));
                }
                if !chars.is_empty() && kit::button(ui, Some(icons::USERS), "Uma trama por personagem", p).clicked() {
                    for c in chars.iter().take(8) {
                        en::new_trama(&mut self.data, &note_name(c));
                    }
                    self.changed();
                }
            });
        });
    }

    fn grade(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, all: &[Col], chars: &[String], acts: &mut Vec<Action>) {
        let empty = |k: &str| self.data[k].as_array().is_none_or(|a| a.is_empty());
        if empty("tramas") && empty("cenas") && empty("planejados") {
            self.empty_grade(ui, p, chars);
            return;
        }
        let (lo, hi) = self.window(all);
        let cols: Vec<&Col> = all[lo..hi].iter().collect();
        let mut rows: Vec<(String, String, Color32)> = self.data["tramas"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| (en::str_of(t, "id").to_string(), en::str_of(t, "nome").to_string(), super::color(en::str_of(t, "cor"))))
            .filter(|t| self.filter_tramas.as_ref().is_none_or(|f| f.contains(&t.0)))
            .collect();
        rows.push(("__none".into(), "Sem trama".into(), p.faint));
        let n = cols.len();
        const LAB: f32 = 176.0;
        const COLW: f32 = 196.0;
        const ADD: f32 = 48.0;
        const PARTH: f32 = 28.0;
        const HEADH: f32 = 48.0;
        const CARD: f32 = 50.0;
        const GAP: f32 = 6.0;
        const PAD: f32 = 8.0;
        let has_parts = cols.iter().any(|c| !c.part.is_empty());
        let caps: Vec<Option<String>> = std::iter::once(None).chain(cols.iter().map(|c| Some(c.path.clone()))).collect();
        let cenas: Vec<Json> = self.data["cenas"].as_array().cloned().unwrap_or_default();
        let grid: Vec<Vec<Vec<usize>>> = rows
            .iter()
            .map(|(tid, _, _)| {
                caps.iter()
                    .map(|cap| {
                        en::scenes_in(&self.data, cap.as_deref())
                            .into_iter()
                            .filter(|&i| {
                                let tr = en::tramas_of(&cenas[i]);
                                self.passes(&cenas[i]) && if tid == "__none" { tr.is_empty() } else { tr.contains(&tid.as_str()) }
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        let row_h: Vec<f32> = grid
            .iter()
            .map(|cells| {
                let m = cells.iter().map(|c| c.len()).max().unwrap_or(0) as f32;
                (PAD * 2.0 + m * CARD + (m - 1.0).max(0.0) * GAP + 24.0).max(80.0)
            })
            .collect();
        let head_h = if has_parts { PARTH } else { 0.0 } + HEADH;
        let bands = self.bands(ui, lo, n, COLW);
        let top_h = head_h + bands.iter().map(|b| b.h).sum::<f32>();
        let total = vec2(LAB + COLW * (n + 1) as f32 + ADD, top_h + row_h.iter().sum::<f32>());
        let mut zones: Vec<Zone> = Vec::new();
        let mut new_scene: Option<(Option<String>, String)> = None;
        let mut select: Option<Sel> = None;
        let mut new_chapter = false;
        let mut open_ch: Option<String> = None;
        let mut toggle_arc: Option<String> = None;
        let mut add_trama = false;
        let mut card_cmd = None;
        let mut open_structures: Option<usize> = None;
        let dragging = Self::dragging(ui);
        egui::ScrollArea::both().id_salt("en-grade").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_size();
            let (rect, _) = ui.allocate_exact_size(total.max(avail), Sense::hover());
            let o = rect.min;
            let clip = ui.clip_rect();
            let painter = ui.painter().clone();
            let hair = Stroke::new(1.0_f32, p.border);
            let sx = |i: usize| o.x + LAB + COLW * i as f32;
            let lab_x = o.x.max(clip.left());
            let head_y = o.y.max(clip.top());
            let content = Rect::from_min_max(pos2(lab_x + LAB, head_y + top_h), clip.max);
            // cells
            let mut y = o.y + top_h;
            for (ri, (tid, _, _)) in rows.iter().enumerate() {
                let h = row_h[ri];
                for ci in 0..=n {
                    let cell = Rect::from_min_size(pos2(sx(ci), y), vec2(COLW, h));
                    if !cell.intersects(clip) {
                        continue;
                    }
                    if ci == 0 {
                        painter.rect_filled(cell, 0.0, p.bg_secondary);
                    } else if cols[ci - 1].planned.is_some() {
                        painter.rect_filled(cell, 0.0, p.bg_secondary.gamma_multiply(0.45));
                    }
                    painter.line_segment([cell.right_top(), cell.right_bottom()], hair);
                    let vis = cell.intersect(content);
                    if vis.is_positive() {
                        let r = ui.interact(vis, ui.id().with(("cell", ri, ci)), Sense::click());
                        if r.double_clicked() {
                            new_scene = Some((caps[ci].clone(), tid.clone()));
                        }
                        if r.hovered() && dragging.is_none() && grid[ri][ci].is_empty() {
                            painter.text(cell.center(), egui::Align2::CENTER_CENTER, icons::PLUS, kit::icon_font(14.0), p.faint.gamma_multiply(0.7));
                        }
                    }
                    let mut cards = Vec::new();
                    let mut cy = y + PAD;
                    for &si in &grid[ri][ci] {
                        let cr = Rect::from_min_size(pos2(cell.left() + PAD, cy), vec2(COLW - 2.0 * PAD, CARD));
                        cy += CARD + GAP;
                        let sid = en::str_of(&cenas[si], "id").to_string();
                        let vis = cr.intersect(content);
                        let mut hovered = false;
                        if vis.is_positive() {
                            let r = ui.interact(vis, ui.id().with(("card", &sid, ri)), Sense::click_and_drag());
                            hovered = r.hovered();
                            if r.clicked() {
                                select = Some(Sel::Cena(sid.clone()));
                            }
                            Self::drag_source(ui, &r, &sid, Some(tid));
                            Self::card_menu(&r, &sid, en::cap_of(&cenas[si]), en::str_of(&cenas[si], "status"), p, &mut card_cmd);
                            if hovered && dragging.is_none() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                        }
                        self.paint_card(ui, cr, &cenas[si], p, hovered, dragging.as_deref() == Some(sid.as_str()));
                        cards.push((cr, sid));
                    }
                    zones.push(Zone { rect: vis, cap: caps[ci].clone(), trama: Some(tid.clone()), cards });
                }
                painter.line_segment([pos2(o.x, y + h), pos2(o.x + total.x.max(avail.x), y + h)], hair);
                y += h;
            }
            // structure bands, over the chapters they cover
            let band_clip = Rect::from_min_max(pos2((lab_x + LAB).max(sx(1)), head_y + head_h), pos2(sx(n + 1).min(clip.right()), clip.bottom()));
            let mut by = o.y + head_h;
            for band in &bands {
                if let Some(s) = self.paint_band(ui, band, pos2(sx(1), by), band_clip, p) {
                    select = Some(s);
                }
                painter.rect_filled(Rect::from_min_size(pos2(sx(0), by), vec2(COLW, band.h)), 0.0, p.bg_secondary);
                painter.line_segment([pos2(o.x, by + band.h), pos2(o.x + total.x.max(avail.x), by + band.h)], hair);
                by += band.h;
            }
            // header (sticky on top)
            let head = Rect::from_min_size(pos2(o.x, head_y), vec2(total.x.max(avail.x), head_h));
            painter.rect_filled(head, 0.0, p.bg);
            painter.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0_f32, p.border_strong));
            let hy = head_y + if has_parts { PARTH } else { 0.0 };
            let visible_x = |r: Rect| r.intersect(Rect::from_min_max(pos2(lab_x + LAB, clip.top()), clip.max));
            if has_parts {
                let mut i = 0;
                while i < n {
                    let mut j = i;
                    while j + 1 < n && cols[j + 1].part == cols[i].part {
                        j += 1;
                    }
                    let part = &cols[i].part;
                    if !part.is_empty() {
                        let r = Rect::from_min_max(pos2(sx(i + 1) + 6.0, head_y + 5.0), pos2(sx(j + 2) - 6.0, head_y + PARTH - 3.0));
                        let on = self.arco.as_ref() == Some(part);
                        let vr = visible_x(r);
                        let hov = vr.is_positive() && {
                            let resp = ui.interact(vr, ui.id().with(("part", i)), Sense::click()).on_hover_text("Clique: mostrar só este arco");
                            if resp.clicked() {
                                toggle_arc = Some(part.clone());
                            }
                            resp.hovered()
                        };
                        painter.rect_filled(r, 6.0, if on { p.accent_soft } else if hov { p.bg_hover } else { p.bg_secondary });
                        let g = truncated(ui, part, kit::medium(11.5), if on { p.accent_strong } else { p.muted }, r.width() - 16.0);
                        painter.galley(pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, p.muted);
                    }
                    i = j + 1;
                }
            }
            let ideas = Rect::from_min_size(pos2(sx(0), hy), vec2(COLW, HEADH));
            painter.text(pos2(ideas.left() + 14.0, ideas.center().y), egui::Align2::LEFT_CENTER, "Ideias", kit::medium(12.5), p.muted);
            let vr = visible_x(ideas);
            if vr.is_positive() {
                ui.interact(vr, ui.id().with("ideas-h"), Sense::hover()).on_hover_text("Cenas que ainda não estão em nenhum capítulo");
            }
            for (i, c) in cols.iter().enumerate() {
                let r = Rect::from_min_size(pos2(sx(i + 1), hy), vec2(COLW, HEADH));
                let vr = visible_x(r);
                let mut hov = false;
                if vr.is_positive() {
                    let resp = ui.interact(vr, ui.id().with(("ch", i)), Sense::click()).on_hover_text(if c.planned.is_some() { format!("{} — planejado no roteiro", c.title) } else { format!("{} — clique para abrir", c.title) });
                    hov = resp.hovered();
                    if resp.clicked() {
                        match &c.planned {
                            Some(pl) => select = Some(Sel::Plan(en::str_of(pl, "id").to_string())),
                            None => open_ch = Some(c.path.clone()),
                        }
                    }
                }
                if hov {
                    painter.rect_filled(r.shrink2(vec2(4.0, 6.0)), 6.0, p.bg_hover);
                }
                let dim = if c.include { 1.0 } else { 0.5 };
                painter.circle_filled(pos2(r.left() + 16.0, r.center().y), 3.5, if c.planned.is_some() { p.faint } else { status_color(&c.status) });
                let ng = ui.fonts(|f| f.layout_no_wrap(c.label.clone(), kit::medium(12.5), p.faint));
                let nw = ng.size().x;
                painter.galley(pos2(r.left() + 26.0, r.center().y - ng.size().y / 2.0), ng, p.faint);
                let tg = truncated(ui, &c.title, if c.planned.is_some() { egui::FontId::new(12.5, egui::FontFamily::Name("italic".into())) } else { kit::medium(12.5) }, p.text.gamma_multiply(dim), r.width() - nw - 44.0);
                painter.galley(pos2(r.left() + 32.0 + nw, r.center().y - tg.size().y / 2.0), tg, p.text);
            }
            let add = Rect::from_min_size(pos2(sx(n + 1), hy), vec2(ADD, HEADH));
            let vr = visible_x(add);
            if vr.is_positive() {
                let resp = ui.interact(vr, ui.id().with("add-ch"), Sense::click()).on_hover_text("Novo capítulo");
                if resp.hovered() {
                    painter.rect_filled(Rect::from_center_size(add.center(), vec2(28.0, 28.0)), 6.0, p.bg_hover);
                }
                painter.text(add.center(), egui::Align2::CENTER_CENTER, icons::PLUS, kit::icon_font(15.0), if resp.hovered() { p.text } else { p.muted });
                if resp.clicked() {
                    new_chapter = true;
                }
            }
            // plot line labels (sticky on the left)
            let mut y = o.y + top_h;
            for (ri, (tid, nome, color)) in rows.iter().enumerate() {
                let h = row_h[ri];
                let r = Rect::from_min_size(pos2(lab_x, y), vec2(LAB, h));
                y += h;
                let vr = r.intersect(Rect::from_min_max(pos2(clip.left(), head_y + top_h), clip.max));
                if !vr.is_positive() {
                    continue;
                }
                let lp = ui.painter().with_clip_rect(vr);
                lp.rect_filled(r, 0.0, p.bg);
                lp.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0_f32, p.border_strong));
                lp.line_segment([r.left_bottom(), r.right_bottom()], hair);
                let resp = ui.interact(vr, ui.id().with(("row", ri)), Sense::click());
                let sel = matches!(&self.sel, Some(Sel::Trama(t)) if t == tid);
                if (resp.hovered() && tid != "__none") || sel {
                    lp.rect_filled(Rect::from_min_size(r.min + vec2(6.0, 8.0), vec2(LAB - 12.0, 30.0)), 6.0, if sel { p.bg_active } else { p.bg_hover });
                }
                lp.circle_filled(pos2(r.left() + 20.0, r.top() + 23.0), 4.5, *color);
                let g = truncated(ui, nome, if tid == "__none" { egui::FontId::proportional(12.5) } else { kit::medium(13.0) }, if tid == "__none" { p.faint } else { p.text }, LAB - 46.0);
                lp.galley(pos2(r.left() + 32.0, r.top() + 23.0 - g.size().y / 2.0), g, p.text);
                let count: usize = grid[ri].iter().map(|c| c.len()).sum();
                if count > 0 {
                    lp.text(pos2(r.left() + 32.0, r.top() + 42.0), egui::Align2::LEFT_CENTER, format!("{count} {}", if count == 1 { "cena" } else { "cenas" }), egui::FontId::proportional(11.0), p.faint);
                }
                if resp.clicked() && tid != "__none" {
                    select = Some(Sel::Trama(tid.clone()));
                }
            }
            let mut by = o.y + head_h;
            for band in &bands {
                let r = Rect::from_min_size(pos2(lab_x, by), vec2(LAB, band.h));
                by += band.h;
                let vr = r.intersect(Rect::from_min_max(pos2(clip.left(), head_y + head_h), clip.max));
                if vr.is_positive() && self.paint_band_label(ui, band, r, vr, p) {
                    open_structures = Some(band.app);
                }
            }
            // corner
            let corner = Rect::from_min_size(pos2(lab_x, head_y), vec2(LAB, head_h));
            painter.rect_filled(corner, 0.0, p.bg);
            painter.line_segment([corner.right_top(), corner.right_bottom()], Stroke::new(1.0_f32, p.border_strong));
            painter.line_segment([corner.left_bottom(), corner.right_bottom()], Stroke::new(1.0_f32, p.border_strong));
            let cy = hy + HEADH / 2.0;
            painter.text(pos2(corner.left() + 16.0, cy), egui::Align2::LEFT_CENTER, "Tramas", kit::medium(12.5), p.muted);
            let pb = Rect::from_center_size(pos2(corner.right() - 22.0, cy), vec2(26.0, 26.0));
            let resp = ui.interact(pb, ui.id().with("add-trama"), Sense::click()).on_hover_text("Nova trama");
            if resp.hovered() {
                painter.rect_filled(pb, 6.0, p.bg_hover);
            }
            painter.text(pb.center(), egui::Align2::CENTER_CENTER, icons::PLUS, kit::icon_font(14.0), if resp.hovered() { p.text } else { p.muted });
            if resp.clicked() {
                add_trama = true;
            }
        });
        self.drops(ui, p, &zones);
        if let Some((cap, tid)) = new_scene {
            let tr: Vec<&str> = if tid == "__none" { vec![] } else { vec![tid.as_str()] };
            self.new_scene(cap.as_deref(), &tr);
        }
        if let Some(s) = select {
            self.sel = Some(s);
            self.confirm = None;
        }
        if new_chapter {
            acts.push(Action::NewChapter(b.path.clone()));
        }
        if let Some(c) = open_ch {
            acts.push(Action::Open(c, true));
        }
        if let Some(a) = toggle_arc {
            self.arco = if self.arco.as_ref() == Some(&a) { None } else { Some(a) };
        }
        if add_trama {
            self.new_trama = Some((String::new(), None));
        }
        self.card_cmd(card_cmd, acts);
        if let Some(i) = open_structures {
            self.st_dialog = Some(super::enredo_st::StDialog { focus: self.apps.get(i).map(|x| x.id()) });
        }
    }

    fn trama_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some((name, for_scene)) = self.new_trama.as_mut() else { return };
        let mut done = None;
        let mut close = false;
        kit::scrim(ctx, p);
        egui::Window::new("nova-trama").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 140.0]).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new("Nova trama").font(kit::medium(16.0)).color(p.text));
            ui.label(RichText::new("Um personagem, um mistério, um conflito.").color(p.muted));
            ui.add_space(12.0);
            let r = ui.add(egui::TextEdit::singleline(name).hint_text(RichText::new("Ex.: Soren, IGA, O mistério da ponte").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6)));
            r.request_focus();
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary(ui, "Criar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some((name.trim().to_string(), for_scene.clone()));
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        if let Some((n, scene)) = done {
            self.new_trama = None;
            if !n.is_empty() {
                let id = en::new_trama(&mut self.data, &n);
                match scene.and_then(|s| en::scene_index(&self.data, &s)) {
                    Some(i) => {
                        if let Some(a) = self.data["cenas"][i]["tramas"].as_array_mut() {
                            a.push(json!(id));
                        } else {
                            self.data["cenas"][i]["tramas"] = json!([id]);
                        }
                    }
                    None => self.sel = Some(Sel::Trama(id)),
                }
                self.changed();
            }
        } else if close {
            self.new_trama = None;
        }
    }

    // ── Escaleta ──────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn escaleta(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, all: &[Col], root: &Path, ix: &Index, acts: &mut Vec<Action>) {
        let (lo, hi) = self.window(all);
        let groups: Vec<Option<&Col>> = all[lo..hi].iter().map(Some).chain(std::iter::once(None)).collect();
        let cenas: Vec<Json> = self.data["cenas"].as_array().cloned().unwrap_or_default();
        let dragging = Self::dragging(ui);
        let mut zones = Vec::new();
        let mut select = None;
        let mut new_in: Option<Option<String>> = None;
        let mut open_ch = None;
        let mut to_chapter: Option<String> = None;
        let mut toggle_arc = None;
        let mut card_cmd = None;
        egui::ScrollArea::vertical().id_salt("en-esc").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let w = (avail - 80.0).clamp(320.0, 820.0);
            let side = ((avail - w) / 2.0).max(16.0);
            ui.add_space(24.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    let mut last_part = String::new();
                    let mut n = 0;
                    for g in &groups {
                        let cap = g.map(|c| c.path.clone());
                        let list: Vec<usize> = en::scenes_in(&self.data, cap.as_deref()).into_iter().filter(|&i| self.passes(&cenas[i])).collect();
                        if let Some(c) = g {
                            if !c.part.is_empty() && c.part != last_part {
                                ui.add_space(14.0);
                                let on = self.arco.as_ref() == Some(&c.part);
                                let r = ui.add(egui::Label::new(RichText::new(c.part.to_uppercase()).font(kit::medium(11.0)).color(if on { p.accent_strong } else { p.faint }).extra_letter_spacing(1.2)).sense(Sense::click())).on_hover_text("Clique: mostrar só este arco");
                                if r.clicked() {
                                    toggle_arc = Some(c.part.clone());
                                }
                                last_part = c.part.clone();
                                ui.add_space(4.0);
                            }
                        }
                        // chapter head
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            match g {
                                Some(c) => {
                                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 18.0), Sense::hover());
                                    ui.painter().circle_filled(r.center(), 3.5, if c.planned.is_some() { p.faint } else { status_color(&c.status) });
                                    ui.label(RichText::new(&c.label).font(kit::medium(14.0)).color(p.faint));
                                    let t = ui.add(egui::Label::new(RichText::new(&c.title).font(if c.planned.is_some() { egui::FontId::new(15.0, egui::FontFamily::Name("italic".into())) } else { kit::medium(15.0) }).color(p.text)).sense(Sense::click()).truncate());
                                    if t.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if t.clicked() {
                                        match &c.planned {
                                            Some(pl) => select = Some(Sel::Plan(en::str_of(pl, "id").to_string())),
                                            None => open_ch = Some(c.path.clone()),
                                        }
                                    }
                                    let gi = all.iter().position(|x| x.path == c.path).unwrap_or(0);
                                    for (key, label, k) in self.beats_at(gi) {
                                        let col = super::enredo_st::band_color(k, p);
                                        let r = ui.add(egui::Label::new(RichText::new(format!("◆ {label}")).size(11.5).color(col)).sense(Sense::click()).truncate());
                                        if r.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        }
                                        if r.clicked() {
                                            select = Some(Sel::Batida(key));
                                        }
                                    }
                                }
                                None => {
                                    ui.label(kit::icon(icons::INBOX, 14.0).color(p.faint));
                                    ui.label(RichText::new("Ideias").font(kit::medium(15.0)).color(p.text));
                                    ui.label(RichText::new("cenas sem capítulo").color(p.faint));
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if kit::icon_button_sized(ui, icons::PLUS, "Nova cena aqui", false, p, 24.0, 14.0).clicked() {
                                    new_in = Some(cap.clone());
                                }
                                ui.label(RichText::new(format!("{} {}", list.len(), if list.len() == 1 { "cena" } else { "cenas" })).size(11.5).color(p.faint));
                            });
                        });
                        if let Some(pl) = g.and_then(|c| c.planned.as_ref().or(c.plan.as_ref())) {
                            let notas: Vec<&str> = en::str_of(pl, "notas").lines().filter(|l| !l.trim().is_empty()).take(2).collect();
                            if !notas.is_empty() {
                                ui.add(egui::Label::new(RichText::new(notas.join(" · ")).size(12.0).italics().color(p.faint)).truncate());
                            }
                        }
                        ui.add_space(6.0);
                        // the scenes
                        let frame = egui::Frame::new().stroke(Stroke::new(1.0_f32, p.border)).corner_radius(10.0).fill(if cap.is_none() { p.bg_secondary } else { p.bg });
                        let mut cards = Vec::new();
                        let fr = frame.show(ui, |ui| {
                            ui.set_width(w);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            if list.is_empty() {
                                ui.add_space(14.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(16.0);
                                    ui.label(RichText::new(if cap.is_some() { "Nenhuma cena planejada" } else { "Arraste para cá o que ainda não tem lugar" }).size(12.5).color(p.faint));
                                });
                                ui.add_space(14.0);
                            }
                            for (k, &si) in list.iter().enumerate() {
                                n += 1;
                                let s = &cenas[si];
                                let sid = en::str_of(s, "id").to_string();
                                let bg = ui.painter().add(egui::Shape::Noop);
                                let inner = ui.horizontal_top(|ui| {
                                    ui.set_width(w);
                                    ui.add_space(14.0);
                                    ui.allocate_ui_with_layout(vec2(26.0, 20.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                        ui.add_space(12.0);
                                        ui.label(RichText::new(n.to_string()).size(12.0).color(p.faint));
                                    });
                                    let meta_w = 190.0_f32.min(w * 0.32);
                                    ui.allocate_ui_with_layout(vec2(w - 26.0 - 14.0 - meta_w - 16.0, 20.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                        ui.add_space(11.0);
                                        let t = en::str_of(s, "titulo");
                                        ui.add(egui::Label::new(RichText::new(if t.is_empty() { "Cena sem título" } else { t }).font(kit::medium(13.5)).color(if t.is_empty() { p.faint } else { p.text })).truncate());
                                        let r = en::str_of(s, "resumo");
                                        if !r.is_empty() {
                                            ui.add_space(2.0);
                                            ui.add(egui::Label::new(RichText::new(r).size(12.5).color(p.muted)).wrap());
                                        }
                                        ui.add_space(11.0);
                                    });
                                    ui.vertical(|ui| {
                                        ui.add_space(11.0);
                                        ui.allocate_ui_with_layout(vec2(meta_w, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            ui.add_space(6.0);
                                            for t in en::tramas_of(s).iter().rev() {
                                                let (r, resp) = ui.allocate_exact_size(vec2(9.0, 16.0), Sense::hover());
                                                ui.painter().circle_filled(r.center(), 4.0, self.trama_color(t, p));
                                                if let Some(tt) = en::trama(&self.data, t) {
                                                    resp.on_hover_text(en::str_of(tt, "nome"));
                                                }
                                            }
                                            let pov = en::str_of(s, "pov");
                                            if !pov.is_empty() {
                                                ui.label(RichText::new(pov).size(11.5).color(p.muted));
                                            }
                                            let q = en::str_of(s, "quando");
                                            if !q.is_empty() {
                                                ui.label(RichText::new(q).size(11.5).color(p.faint));
                                            }
                                            if cap.is_none() && kit::quiet(ui, Some(icons::ARROW_RIGHT), "capítulo", p).on_hover_text("Criar um capítulo a partir desta cena").clicked() {
                                                to_chapter = Some(sid.clone());
                                            }
                                        });
                                    });
                                });
                                let rr = inner.response.rect;
                                let resp = ui.interact(rr, ui.id().with(("esc", &sid)), Sense::click_and_drag());
                                let sel = matches!(&self.sel, Some(Sel::Cena(id)) if id == &sid);
                                let ghost = dragging.as_deref() == Some(sid.as_str());
                                let fill = if sel { p.accent_soft } else if resp.hovered() && dragging.is_none() { p.bg_hover } else { Color32::TRANSPARENT };
                                let mut shapes = vec![egui::Shape::rect_filled(rr.shrink(1.0), 8.0, if ghost { p.bg_secondary } else { fill })];
                                shapes.push(egui::Shape::rect_filled(Rect::from_min_size(rr.min + vec2(0.0, 10.0), vec2(3.0, rr.height() - 20.0)), 2.0, status_color(en::str_of(s, "status"))));
                                if k > 0 {
                                    shapes.push(egui::Shape::line_segment([rr.left_top() + vec2(14.0, 0.0), rr.right_top() - vec2(14.0, 0.0)], Stroke::new(1.0_f32, p.border)));
                                }
                                ui.painter().set(bg, egui::Shape::Vec(shapes));
                                if resp.clicked() {
                                    select = Some(Sel::Cena(sid.clone()));
                                }
                                Self::drag_source(ui, &resp, &sid, None);
                                Self::card_menu(&resp, &sid, cap.as_deref(), en::str_of(s, "status"), p, &mut card_cmd);
                                cards.push((rr, sid));
                            }
                        });
                        zones.push(Zone { rect: fr.response.rect.intersect(ui.clip_rect()), cap, trama: None, cards });
                    }
                    ui.add_space(60.0);
                });
            });
        });
        self.drops(ui, p, &zones);
        if let Some(s) = select {
            self.sel = Some(s);
            self.confirm = None;
        }
        if let Some(cap) = new_in {
            self.new_scene(cap.as_deref(), &[]);
        }
        if let Some(c) = open_ch {
            acts.push(Action::Open(c, true));
        }
        if let Some(a) = toggle_arc {
            self.arco = if self.arco.as_ref() == Some(&a) { None } else { Some(a) };
        }
        self.card_cmd(card_cmd, acts);
        if let Some(sid) = to_chapter {
            if let Some(i) = en::scene_index(&self.data, &sid) {
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
