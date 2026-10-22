//! Mundo: the codex over the world's notes (categories, entries,
//! a reader), the world's time (calendar, date converter, eras), family
//! trees and the dictionary over the language notes. The notes are read in
//! place; `Mundo/Mundo.md` keeps only the organisation.

use super::library::Cx;
use super::Action;
use crate::host::RenderHost;
use crate::icons;
use crate::kit;
use crate::markdown::{self, Renderer};
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::{self, Book};
use ateliet_core::scriptorium::enredo::{self as en, str_of};
use ateliet_core::scriptorium::mundo::{self as mu, Codex, LexRow};
use ateliet_core::scriptorium::fmt_n;
use eframe::egui::{self, vec2, Color32, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Aba {
    Codice,
    Tempo,
    Mapas,
    Genealogias,
    Linguas,
}

const ABAS: [(Aba, &str, &str); 5] = [(Aba::Codice, icons::LIBRARY, "Códice"), (Aba::Tempo, icons::HOURGLASS, "Tempo"), (Aba::Mapas, icons::MAP, "Mapas"), (Aba::Genealogias, icons::GIT_FORK, "Genealogias"), (Aba::Linguas, icons::LANGUAGES, "Línguas")];

pub fn cat_icon(k: &str) -> &'static str {
    match k {
        "visao" => icons::GLOBE,
        "geografia" => icons::MOUNTAIN,
        "povos" => icons::USERS,
        "casas" => icons::SHIELD,
        "organizacoes" => icons::LANDMARK,
        "religiao" => icons::FLAME,
        "magia" => icons::ORBIT,
        "bestiario" => icons::PAW_PRINT,
        "linguas" => icons::LANGUAGES,
        "tempo" => icons::CALENDAR_CLOCK,
        "historia" => icons::HOURGLASS,
        "arte" => icons::PALETTE,
        "itens" => icons::GEM,
        "entidades" => icons::GHOST,
        "conceitos" => icons::LIGHTBULB,
        "personagens" => icons::USER,
        "pesquisa" => icons::BOOK_OPEN,
        "tudo" => icons::LAYERS,
        "fixados" => icons::STAR,
        "imagens" => icons::IMAGE,
        "legado" => icons::ARCHIVE,
        _ => icons::FILE_TEXT,
    }
}

/// What a background job hands back.
enum Job {
    Codex(Codex),
    Apps(HashMap<String, Vec<usize>>),
    Lex(Vec<LexRow>),
    Done,
}

pub struct NewWorld {
    pub nome: String,
    pub pasta: String,
    pub filtro: String,
    /// Editing an existing world (rename) instead of creating one.
    pub id: Option<String>,
}

pub struct EraEdit {
    pub id: Option<String>,
    pub nome: String,
    pub de: String,
    pub ate: String,
    pub nativo: String,
    pub desc: String,
    pub cor: String,
}

pub struct CalEdit {
    pub nome: String,
    pub meses: String,
    pub semana: String,
    pub por_mes: bool,
    pub semana_nome: String,
    pub dia_extra: String,
    pub inter: String,
    pub depois: String,
    pub ciclo: String,
    pub anos: String,
    pub antes: String,
    pub dep: String,
    pub presente: String,
    pub dia: String,
}

/// Choosing a note (to read a calendar or eras from).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NotePick {
    Calendar,
    Eras,
    Source,
    SourceFolder,
}

pub struct MundoView {
    pub book: String,
    pub aba: Aba,
    pub mundo: Option<String>,
    pub(super) data: Json,
    pub(super) home: Option<Book>,
    loaded: bool,
    mtime: Option<SystemTime>,
    dirty: bool,
    last_edit: Option<Instant>,
    // codex
    pub(super) cat: String,
    pub(super) sel: Option<String>,
    pub(super) q: String,
    deep: bool,
    hits: Option<(String, HashMap<String, String>)>,
    pub(super) codex: Option<Arc<Codex>>,
    codex_for: String,
    pub(super) chapters: Vec<(String, String)>,
    pub(super) apps: Option<HashMap<String, Vec<usize>>>,
    job: Option<Receiver<Job>>,
    jobs_running: usize,
    pub(super) last_check: Option<Instant>,
    // languages
    pub(super) lex: Option<Arc<Vec<LexRow>>>,
    pub(super) lang: String,
    pub(super) lex_max: usize,
    // trees
    pub(super) tree: Option<String>,
    pub(super) person: Option<String>,
    // tempo
    pub(super) conv: (i64, usize, i64, i64, char),
    // dialogs
    pub(super) new_world: Option<NewWorld>,
    pub(super) sources: bool,
    pub(super) era: Option<EraEdit>,
    pub(super) cal_edit: Option<CalEdit>,
    pub(super) note_pick: Option<(NotePick, String)>,
    pub(super) new_tree: Option<(String, String)>,
    pub(super) recat: Option<String>,
    pub(super) retermos: Option<(String, String)>,
    pub(super) toast: Option<String>,
    pub(super) focus_name: bool,
    pub(super) pending_eras: Option<Vec<Json>>,
    // maps
    pub(super) maps: Option<(Instant, Vec<(String, Json)>)>,
    pub(super) new_map: Option<super::mundo_mapas::NewMap>,
}

impl MundoView {
    pub fn new(book: &str) -> MundoView {
        MundoView {
            book: book.into(),
            aba: Aba::Codice,
            mundo: None,
            data: mu::blank(),
            home: None,
            loaded: false,
            mtime: None,
            dirty: false,
            last_edit: None,
            cat: "tudo".into(),
            sel: None,
            q: String::new(),
            deep: false,
            hits: None,
            codex: None,
            codex_for: String::new(),
            chapters: Vec::new(),
            apps: None,
            job: None,
            jobs_running: 0,
            last_check: None,
            lex: None,
            lang: "tudo".into(),
            lex_max: 120,
            tree: None,
            person: None,
            conv: (1, 0, 1, 0, 'd'),
            new_world: None,
            sources: false,
            era: None,
            cal_edit: None,
            note_pick: None,
            new_tree: None,
            recat: None,
            retermos: None,
            toast: None,
            focus_name: false,
            pending_eras: None,
            maps: None,
            new_map: None,
        }
    }

    pub fn title(&self) -> String {
        "Mundo".into()
    }

    pub fn changed(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
    }

    pub(super) fn world(&self) -> Option<&Json> {
        let ws = self.data["mundos"].as_array()?;
        self.mundo.as_ref().and_then(|id| ws.iter().find(|w| w["id"] == id.as_str())).or(ws.first())
    }

    pub(super) fn world_mut(&mut self) -> Option<&mut Json> {
        let id = self.world().map(|w| str_of(w, "id").to_string())?;
        self.data["mundos"].as_array_mut()?.iter_mut().find(|w| w["id"] == id.as_str())
    }

    fn file_mtime(root: &Path, home: &Book) -> Option<SystemTime> {
        std::fs::metadata(root.join(mu::path(home))).and_then(|m| m.modified()).ok()
    }

    fn ensure(&mut self, root: &Path, home: &Book) {
        let m = Self::file_mtime(root, home);
        if !self.loaded || (!self.dirty && m != self.mtime) {
            self.data = mu::load(root, home);
            self.mtime = m;
            self.loaded = true;
            self.codex_for.clear();
        }
    }

    pub fn save(&mut self, root: &Path, ix: &Index) -> Option<String> {
        if !self.dirty {
            return None;
        }
        let home = self.home.clone()?;
        let r = mu::save(root, ix, &home, &self.data).ok();
        self.dirty = false;
        self.mtime = Self::file_mtime(root, &home);
        r
    }

    /// Index the world (and find its names in the chapters) in the background.
    fn jobs(&mut self, ctx: &egui::Context, root: &Path) {
        if let Some(rx) = &self.job {
            while let Ok(j) = rx.try_recv() {
                match j {
                    Job::Codex(c) => {
                        self.codex = Some(Arc::new(c));
                        self.apps = None;
                        self.hits = None;
                    }
                    Job::Apps(a) => self.apps = Some(a),
                    Job::Lex(l) => self.lex = Some(Arc::new(l)),
                    Job::Done => self.jobs_running = 0,
                }
            }
        }
        let Some(w) = self.world().cloned() else { return };
        let wid = str_of(&w, "id").to_string();
        let due = self.last_check.is_none_or(|t| t.elapsed() > Duration::from_secs(4));
        if self.jobs_running > 0 || !(due || self.codex_for != wid) {
            return;
        }
        self.last_check = Some(Instant::now());
        let (tx, rx) = mpsc::channel();
        let root = root.to_path_buf();
        let data = self.data.clone();
        let old_sig = if self.codex_for == wid { self.codex.as_ref().map(|c| c.sig.clone()) } else { None };
        let need_apps = self.apps.is_none();
        let need_lex = self.lex.is_none() || old_sig.is_none();
        let chapters: Vec<String> = self.chapters.iter().map(|c| c.1.clone()).collect();
        let current = self.codex.clone();
        self.codex_for = wid;
        self.jobs_running = 1;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let sig = mu::signature(&root, &w, &data);
            let fresh = old_sig.as_deref() != Some(sig.as_str());
            let codex = if fresh { Some(Arc::new(mu::index(&root, &w, &data))) } else { current };
            if fresh {
                if let Some(c) = &codex {
                    let _ = tx.send(Job::Codex(Codex { entries: c.entries.clone(), by_key: c.by_key.clone(), images: c.images.clone(), sig: c.sig.clone() }));
                }
            }
            if fresh || need_lex {
                let _ = tx.send(Job::Lex(mu::lexicon(&root, &w)));
            }
            if fresh || need_apps {
                if let Some(c) = &codex {
                    let _ = tx.send(Job::Apps(mu::appearances(&root, &chapters, c)));
                }
            }
            let _ = tx.send(Job::Done);
            ctx.request_repaint();
        });
        self.job = Some(rx);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let root: PathBuf = cx.vault.root.clone();
        let author = cx.sc.author();
        let ix_lock = cx.vault.index.clone();
        let ix = ix_lock.read().unwrap();
        let Some(b) = book::book_by_path(&ix, &self.book, &author) else {
            ui.label("Livro não encontrado.");
            return acts;
        };
        let home = mu::home(&root, &ix, &b, &author);
        self.ensure(&root, &home);
        self.home = Some(home);
        if self.chapters.is_empty() {
            for sb in en::saga_books(&ix, &b, &author) {
                for c in book::chapters(&ix, &sb) {
                    self.chapters.push((if sb.path == b.path { c.title.clone() } else { format!("{} · {}", sb.title, c.title) }, c.path));
                }
            }
        }
        if self.dirty && self.last_edit.is_some_and(|t| t.elapsed() > Duration::from_millis(600)) {
            if let Some(path) = self.save(&root, &ix) {
                acts.push(Action::Saved(path));
            }
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        self.jobs(ui.ctx(), &root);
        ui.ctx().request_repaint_after(Duration::from_secs(4));
        self.keys(ui);

        egui::TopBottomPanel::top(ui.id().with("mu-bar")).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 20, right: 16, top: 12, bottom: 10 })).show_inside(ui, |ui| {
            self.bar(ui, p, &b);
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show_inside(ui, |ui| {
            if self.world().is_none() {
                self.empty(ui, p);
                return;
            }
            match self.aba {
                Aba::Codice => self.codice(ui, p, cx, &root, &mut acts),
                Aba::Tempo => self.tempo(ui, p, &root, &ix, &b),
                Aba::Mapas => self.mapas(ui, p, cx, &root, &ix, &b, &author, &mut acts),
                Aba::Genealogias => self.genealogias(ui, p, &mut acts),
                Aba::Linguas => self.linguas(ui, p, &mut acts),
            }
        });
        self.dialogs(ui.ctx(), p, &root, &ix, &b, &author);
        self.new_map_dialog(ui.ctx(), p, &root, &ix, &b, &author, &mut acts);
        if let Some(t) = self.toast.take() {
            acts.push(Action::Toast(t));
        }
        acts
    }

    fn keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().memory(|m| m.focused().is_some() || m.any_popup_open()) || self.any_dialog() {
            return;
        }
        let k = ui.input(|i| [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5].iter().position(|k| i.key_pressed(*k) && !i.modifiers.command));
        if let Some(k) = k {
            self.aba = ABAS[k].0;
        }
    }

    pub(super) fn any_dialog(&self) -> bool {
        self.new_world.is_some() || self.new_map.is_some() || self.sources || self.era.is_some() || self.cal_edit.is_some() || self.note_pick.is_some() || self.new_tree.is_some() || self.recat.is_some() || self.retermos.is_some()
    }

    // ── bar ───────────────────────────────────────────────────────────────

    fn bar(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Mundo").font(kit::bold(17.0)).color(p.text));
                ui.label(RichText::new(if b.serie.is_empty() { &b.title } else { &b.serie }).size(11.5).color(p.faint));
            });
            ui.add_space(10.0);
            let worlds: Vec<(String, String, String)> = self.data["mundos"].as_array().into_iter().flatten().map(|w| (str_of(w, "id").to_string(), str_of(w, "nome").to_string(), str_of(w, "cor").to_string())).collect();
            let cur = self.world().map(|w| str_of(w, "id").to_string());
            for (id, nome, cor) in &worlds {
                let on = cur.as_ref() == Some(id);
                let r = kit::chip(ui, None, &format!("●  {nome}"), on, p);
                let _ = cor;
                if r.clicked() {
                    self.mundo = Some(id.clone());
                    self.sel = None;
                    self.cat = "tudo".into();
                    self.lex = None;
                    self.apps = None;
                }
                r.context_menu(|ui| {
                    if kit::menu_item(ui, icons::SETTINGS_2, "Fontes e ajustes…", p) {
                        self.mundo = Some(id.clone());
                        self.sources = true;
                    }
                    if kit::menu_item(ui, icons::PENCIL, "Renomear…", p) {
                        self.new_world = Some(NewWorld { nome: nome.clone(), pasta: String::new(), filtro: String::new(), id: Some(id.clone()) });
                    }
                    kit::menu_sep(ui, p);
                    if kit::menu_danger(ui, icons::TRASH_2, "Remover do Scriptorium", p) {
                        if let Some(a) = self.data["mundos"].as_array_mut() {
                            a.retain(|w| w["id"] != id.as_str());
                        }
                        self.mundo = None;
                        self.changed();
                    }
                });
            }
            if kit::icon_button_sized(ui, icons::PLUS, "Novo mundo", false, p, 24.0, 14.0).clicked() {
                self.new_world = Some(NewWorld { nome: String::new(), pasta: String::new(), filtro: String::new(), id: None });
            }
            if self.world().is_none() {
                return;
            }
            ui.add_space(10.0);
            let wide = ui.available_width() > 640.0;
            let items: Vec<(&str, &str)> = ABAS.iter().map(|a| (a.1, if wide { a.2 } else { "" })).collect();
            let curi = ABAS.iter().position(|a| a.0 == self.aba).unwrap_or(0);
            if let Some(i) = kit::segmented(ui, &items, curi, p) {
                self.aba = ABAS[i].0;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, icons::SETTINGS_2, "Fontes e ajustes deste mundo", false, p).clicked() {
                    self.sources = true;
                }
                if matches!(self.aba, Aba::Codice | Aba::Linguas) {
                    ui.allocate_ui(vec2(ui.available_width().min(260.0), 30.0), |ui| {
                        let r = kit::search_field(ui, &mut self.q, if self.aba == Aba::Codice { "Buscar no mundo" } else { "Palavra ou sentido" }, p);
                        if r.changed() {
                            self.deep = false;
                            self.lex_max = 120;
                        }
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.deep = true;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Slash)) && !ui.memory(|m| m.focused().is_some()) {
                            r.request_focus();
                        }
                    });
                }
            });
        });
    }

    fn empty(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.add_space(90.0);
        ui.vertical_centered(|ui| {
            ui.label(kit::icon(icons::GLOBE, 34.0).color(p.faint));
            ui.add_space(14.0);
            ui.label(RichText::new("Um mundo para este livro").font(kit::medium(17.0)).color(p.text));
            ui.add_space(6.0);
            ui.allocate_ui(vec2(460.0, 80.0), |ui| {
                ui.label(RichText::new("O Mundo organiza as notas que você já escreveu (geografia, povos, religião, línguas, história) sem copiá-las, e acrescenta calendário, eras e genealogias.").color(p.muted));
            });
            ui.add_space(14.0);
            if kit::primary_icon(ui, icons::PLUS, "Criar mundo", p).clicked() {
                self.new_world = Some(NewWorld { nome: String::new(), pasta: String::new(), filtro: String::new(), id: None });
            }
        });
    }

    // ── códice ────────────────────────────────────────────────────────────

    fn cat_counts(cx: &Codex) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for e in &cx.entries {
            if let Some(par) = e.parent.as_ref().and_then(|k| cx.get(k)) {
                if par.cat == e.cat && par.legado == e.legado {
                    continue;
                }
            }
            if e.legado {
                *counts.entry("legado".into()).or_default() += 1;
                continue;
            }
            *counts.entry("tudo".into()).or_default() += 1;
            *counts.entry(e.cat.clone()).or_default() += 1;
        }
        counts
    }

    pub(super) fn apps_of(&self, e: &mu::Entry) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        if let Some(a) = &self.apps {
            for t in &e.terms {
                for &i in a.get(t).into_iter().flatten() {
                    if !out.contains(&i) {
                        out.push(i);
                    }
                }
            }
        }
        out.sort();
        out
    }

    fn codice(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, root: &Path, acts: &mut Vec<Action>) {
        let Some(codex) = self.codex.clone().filter(|_| self.world().is_some_and(|w| w["id"] == self.codex_for.as_str())) else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(18.0).color(p.muted));
                ui.label(RichText::new("Lendo as notas do mundo…").color(p.muted));
            });
            return;
        };
        let counts = Self::cat_counts(&codex);
        let fixados: Vec<String> = self.data["fixados"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).filter(|k| codex.by_key.contains_key(k)).collect();
        // narrow: the list and the reader take turns
        let avail = ui.available_width();
        let narrow = avail < 960.0;
        let reading = narrow && self.sel.is_some();
        let cats_w = if narrow { 190.0 } else { 210.0 };
        let list_w = if narrow { (avail - cats_w).max(240.0) } else { 330.0 };
        // categories
        if !reading {
        egui::SidePanel::left(ui.id().with("mu-cats")).resizable(false).exact_width(cats_w).show_separator_line(false).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 14, right: 8, top: 14, bottom: 8 })).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("mu-cats-s").show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let mut row = |ui: &mut egui::Ui, k: &str, label: &str, n: usize| {
                    if kit::Row::new(label).icon(cat_icon(k)).selected(self.cat == k).trailing(n.to_string()).show(ui, p).clicked() {
                        self.cat = k.to_string();
                        self.q.clear();
                    }
                };
                row(ui, "tudo", "Tudo", counts.get("tudo").copied().unwrap_or(0));
                if !fixados.is_empty() {
                    row(ui, "fixados", "Fixados", fixados.len());
                }
                ui.add_space(6.0);
                for (k, nome, _) in mu::CATS {
                    if let Some(&n) = counts.get(k) {
                        row(ui, k, nome, n);
                    }
                }
                if !codex.images.is_empty() || counts.contains_key("legado") {
                    ui.add_space(6.0);
                }
                if !codex.images.is_empty() {
                    row(ui, "imagens", "Imagens", codex.images.len());
                }
                if let Some(&n) = counts.get("legado") {
                    row(ui, "legado", "Versões anteriores", n);
                }
            });
        });
        }
        // entries
        let q = self.q.trim().to_lowercase();
        if self.deep && !q.is_empty() && self.hits.as_ref().is_none_or(|h| h.0 != q) {
            self.hits = Some((q.clone(), mu::full_search(root, &codex, &q)));
        }
        let hits = self.hits.as_ref().filter(|h| self.deep && h.0 == q).map(|h| h.1.clone());
        let items: Vec<usize> = if !q.is_empty() {
            let mut v: Vec<usize> = (0..codex.entries.len()).filter(|&i| {
                let e = &codex.entries[i];
                e.titulo.to_lowercase().contains(&q) || e.terms.iter().any(|t| t.to_lowercase().contains(&q)) || hits.as_ref().is_some_and(|h| h.contains_key(&e.key))
            }).collect();
            v.sort_by_key(|&i| codex.entries[i].legado);
            v
        } else if self.cat == "fixados" {
            fixados.iter().filter_map(|k| codex.by_key.get(k).copied()).collect()
        } else {
            (0..codex.entries.len()).filter(|&i| {
                let e = &codex.entries[i];
                if let Some(par) = e.parent.as_ref().and_then(|k| codex.get(k)) {
                    if par.cat == e.cat && par.legado == e.legado {
                        return false;
                    }
                }
                if self.cat == "legado" {
                    return e.legado;
                }
                !e.legado && (self.cat == "tudo" || e.cat == self.cat)
            }).collect()
        };
        if !reading {
        egui::SidePanel::left(ui.id().with("mu-list")).resizable(false).exact_width(list_w).show_separator_line(false).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 10, right: 6, top: 12, bottom: 8 })).show_inside(ui, |ui| {
            if self.cat == "imagens" && q.is_empty() {
                self.images(ui, p, cx, &codex, acts);
                return;
            }
            let title = if !q.is_empty() { format!("{} resultado{}", items.len(), if items.len() == 1 { "" } else { "s" }) } else { match self.cat.as_str() { "tudo" => "Tudo".into(), "fixados" => "Fixados".into(), "legado" => "Versões anteriores".into(), k => mu::cat_name(k).into() } };
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(title).font(kit::medium(13.0)).color(p.text));
                if !q.is_empty() && !self.deep {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::quiet(ui, None, "Buscar no texto ↵", p).clicked() {
                            self.deep = true;
                        }
                    });
                }
            });
            if self.cat == "legado" && q.is_empty() {
                ui.label(RichText::new("Textos guardados que o cânone substituiu. Quando divergem, vale a fonte canônica.").size(11.5).color(p.faint));
            }
            ui.add_space(4.0);
            if items.is_empty() {
                ui.label(RichText::new(if q.is_empty() { "Nada aqui ainda." } else { "Nada com esse nome. Enter procura dentro dos textos." }).color(p.faint));
                return;
            }
            egui::ScrollArea::vertical().id_salt("mu-list-s").auto_shrink([false, false]).show(ui, |ui| {
                let mut last_fonte: Option<String> = None;
                for &i in &items {
                    let e = &codex.entries[i];
                    if q.is_empty() && last_fonte.as_deref() != Some(e.fonte.as_str()) {
                        ui.add_space(6.0);
                        ui.label(RichText::new(e.fonte.to_uppercase()).font(kit::medium(10.0)).color(p.faint).extra_letter_spacing(0.8));
                        last_fonte = Some(e.fonte.clone());
                    }
                    let sel = self.sel.as_deref() == Some(e.key.as_str());
                    let bg = ui.painter().add(egui::Shape::Noop);
                    let inner = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&e.titulo).font(kit::medium(13.0)).color(if e.legado { p.muted } else { p.text })).truncate());
                            if fixados.contains(&e.key) {
                                ui.label(kit::icon(icons::STAR, 11.0).color(p.accent_strong));
                            }
                        });
                        let mut bits: Vec<String> = Vec::new();
                        if !q.is_empty() {
                            bits.push(e.parent.as_ref().and_then(|k| codex.get(k)).map(|x| x.titulo.clone()).unwrap_or_else(|| e.fonte.clone()));
                        }
                        if e.legado && self.cat != "legado" {
                            bits.push("versão anterior".into());
                        }
                        if !e.children.is_empty() {
                            bits.push(format!("{} seções", e.children.len()));
                        }
                        bits.push(format!("{} palavras", fmt_n(e.words as i64)));
                        let n = self.apps_of(e).len();
                        if n > 0 {
                            bits.push(format!("em {n} cap."));
                        }
                        ui.add(egui::Label::new(RichText::new(bits.join(" · ")).size(11.0).color(p.faint)).truncate());
                        let sn = hits.as_ref().and_then(|h| h.get(&e.key)).cloned().unwrap_or_else(|| e.snippet.clone());
                        if !sn.is_empty() {
                            let mut job = egui::text::LayoutJob::single_section(sn, egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: p.muted, ..Default::default() });
                            job.wrap = egui::text::TextWrapping { max_rows: 2, break_anywhere: false, overflow_character: Some('…'), max_width: ui.available_width() };
                            ui.label(job);
                        }
                    });
                    let r = ui.interact(inner.response.rect, ui.id().with(("mu-row", &e.key)), Sense::click());
                    let fill = if sel { p.bg } else if r.hovered() { p.bg_hover } else { Color32::TRANSPARENT };
                    ui.painter().set(bg, egui::Shape::rect_filled(inner.response.rect, 8.0, fill));
                    if sel {
                        ui.painter().rect_stroke(inner.response.rect, 8.0, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                    }
                    if r.clicked() {
                        self.sel = Some(e.key.clone());
                    }
                }
            });
        });
        }
        if narrow && !reading {
            return;
        }
        // reader
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 34, right: 24, top: 20, bottom: 12 })).show_inside(ui, |ui| {
            if reading && kit::quiet(ui, Some(icons::ARROW_LEFT), "Voltar à lista", p).clicked() {
                self.sel = None;
                return;
            }
            self.reader(ui, p, cx, root, &codex, acts);
        });
    }

    fn images(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, codex: &Codex, acts: &mut Vec<Action>) {
        ui.label(RichText::new("Imagens do mundo").font(kit::medium(13.0)).color(p.text));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().id_salt("mu-imgs").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for im in &codex.images {
                    let (rect, r) = ui.allocate_exact_size(vec2(146.0, 120.0), Sense::click());
                    let thumb = egui::Rect::from_min_size(rect.min, vec2(146.0, 96.0));
                    match crate::host::image(ui.ctx(), cx.vault, cx.caches, &im.path, "") {
                        Some((id, sz)) => {
                            let k = (thumb.width() / sz.x).max(thumb.height() / sz.y);
                            let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), vec2(thumb.width() / (sz.x * k), thumb.height() / (sz.y * k)));
                            ui.painter().add(egui::epaint::RectShape::filled(thumb, 8.0, Color32::WHITE).with_texture(id, uv));
                        }
                        None => {
                            ui.painter().rect_filled(thumb, 8.0, p.bg_active);
                        }
                    }
                    let name = im.path.rsplit('/').next().unwrap_or(&im.path);
                    let g = super::enredo_ui::truncated(ui, name, egui::FontId::proportional(11.5), p.muted, 146.0);
                    ui.painter().galley(egui::pos2(rect.left(), thumb.bottom() + 6.0), g, p.muted);
                    if r.on_hover_text(name).clicked() {
                        acts.push(Action::Open(im.path.clone(), true));
                    }
                }
            });
        });
    }

    fn reader(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, root: &Path, codex: &Codex, acts: &mut Vec<Action>) {
        let Some(e) = self.sel.as_ref().and_then(|k| codex.get(k)).cloned() else {
            let w = self.world().cloned().unwrap_or(Json::Null);
            let canon: Vec<&mu::Entry> = codex.entries.iter().filter(|x| !x.legado).collect();
            let words: usize = canon.iter().filter(|x| x.parent.is_none()).map(|x| x.words).sum();
            let mut notes: Vec<&str> = canon.iter().map(|x| x.path.as_str()).collect();
            notes.sort();
            notes.dedup();
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(str_of(&w, "nome")).font(kit::bold(28.0)).color(p.text));
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{} verbetes · {} palavras em {} notas", fmt_n(canon.len() as i64), fmt_n(words as i64), notes.len())).color(p.muted));
                ui.add_space(4.0);
                if w["fontes"].as_array().is_none_or(|a| a.is_empty()) {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Este mundo ainda não tem fontes: escolha a pasta ou as notas onde ele está escrito.").color(p.faint));
                    ui.add_space(10.0);
                    if kit::primary_icon(ui, icons::FOLDER_PLUS, "Escolher fontes", p).clicked() {
                        self.sources = true;
                    }
                } else {
                    ui.label(RichText::new("Escolha um verbete à esquerda. As notas continuam onde estão; o Códice só as organiza.").size(12.0).color(p.faint));
                }
            });
            return;
        };
        let fixed = self.data["fixados"].as_array().is_some_and(|a| a.iter().any(|x| x == e.key.as_str()));
        ui.horizontal(|ui| {
            ui.label(kit::icon(cat_icon(&e.cat), 12.0).color(p.faint));
            ui.label(RichText::new(mu::cat_name(&e.cat)).size(12.0).color(p.faint));
            ui.label(RichText::new("·").color(p.faint));
            match e.parent.as_ref().and_then(|k| codex.get(k)) {
                Some(par) => {
                    if ui.add(egui::Label::new(RichText::new(&par.titulo).size(12.0).color(p.accent_strong)).sense(Sense::click())).clicked() {
                        self.sel = Some(par.key.clone());
                    }
                }
                None => {
                    ui.label(RichText::new(&e.fonte).size(12.0).color(p.faint));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let more = kit::icon_button_sized(ui, icons::ELLIPSIS, "Mais", false, p, 26.0, 14.0);
                let popup = ui.make_persistent_id("mu-more");
                if more.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(popup));
                }
                egui::popup_below_widget(ui, popup, &more, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    if kit::menu_item(ui, icons::TAGS, "Mudar categoria…", p) {
                        self.recat = Some(e.key.clone());
                    }
                    if kit::menu_item(ui, icons::TEXT_SEARCH, "Nomes procurados nos capítulos…", p) {
                        self.retermos = Some((e.key.clone(), e.terms.join(", ")));
                    }
                    if kit::menu_item(ui, icons::LINK, "Copiar link", p) {
                        ui.ctx().copy_text(format!("[[{}{}]]", e.path.trim_end_matches(".md"), e.heading.as_ref().map(|h| format!("#{h}")).unwrap_or_default()));
                    }
                });
                if kit::icon_button_sized(ui, icons::FILE_TEXT, "Abrir a nota nesta seção", false, p, 26.0, 14.0).clicked() {
                    acts.push(Action::OpenLink(format!("{}{}", e.path, e.heading.as_ref().map(|h| format!("#{h}")).unwrap_or_default()), e.path.clone(), true));
                }
                if kit::icon_button_sized(ui, icons::STAR, if fixed { "Desafixar" } else { "Fixar" }, fixed, p, 26.0, 14.0).clicked() {
                    let a = self.data["fixados"].as_array_mut().unwrap();
                    match a.iter().position(|x| x == e.key.as_str()) {
                        Some(i) => {
                            a.remove(i);
                        }
                        None => a.push(json!(e.key)),
                    }
                    self.changed();
                }
            });
        });
        egui::ScrollArea::vertical().id_salt(("mu-reader", &e.key)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(760.0));
            if e.legado {
                ui.add_space(6.0);
                egui::Frame::new().fill(p.bg_secondary).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(kit::icon(icons::ARCHIVE, 13.0).color(p.muted));
                        ui.label(RichText::new("Versão anterior. O cânone deste mundo prevalece quando os textos divergem.").size(12.5).color(p.muted));
                    });
                });
            }
            ui.add_space(8.0);
            ui.label(RichText::new(&e.titulo).font(kit::bold(28.0)).color(p.text));
            ui.label(RichText::new(format!("{} palavras · {}", fmt_n(e.words as i64), e.path.rsplit('/').next().unwrap_or(&e.path).trim_end_matches(".md"))).size(12.0).color(p.faint));
            let apps = self.apps_of(&e);
            if !apps.is_empty() {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Aparece em").size(12.0).color(p.muted));
                    for &i in apps.iter().take(12) {
                        if let Some((t, path)) = self.chapters.get(i) {
                            if kit::chip(ui, Some(icons::BOOK_OPEN), t, false, p).clicked() {
                                acts.push(Action::Open(path.clone(), true));
                            }
                        }
                    }
                });
            }
            ui.add_space(12.0);
            let kids: Vec<mu::Entry> = e.children.iter().filter_map(|k| codex.get(k).cloned()).collect();
            let all = mu::section_text(root, &e);
            let lines: Vec<&str> = all.lines().collect();
            let stop = kids.first().map(|k| k.line.saturating_sub(e.line)).unwrap_or(lines.len()).min(lines.len());
            let from = if e.heading.is_some() { 1 } else { 0 };
            // a note's own title (its single "# ") is already the entry's title
            let from = if from == 0 && lines.iter().find(|l| !l.trim().is_empty()).is_some_and(|l| l.starts_with("# ")) { lines.iter().position(|l| l.starts_with("# ")).map(|i| i + 1).unwrap_or(0) } else { from };
            let intro = lines[from.min(stop)..stop].join("\n");
            let intro = intro.trim();
            if !intro.is_empty() {
                let doc = markdown::parse(intro);
                let r = {
                    let mut h = RenderHost { vault: cx.vault, palette: p, size: 15.0, source: &e.path, caches: cx.caches, settings: cx.dv };
                    let mut r = Renderer::new(&mut h);
                    r.blocks(ui, &doc.blocks);
                    std::mem::take(&mut r.actions)
                };
                for a in r {
                    if let markdown::Action::Open(t, nt) = a {
                        acts.push(Action::OpenLink(t, e.path.clone(), nt));
                    }
                }
            }
            if !kids.is_empty() {
                ui.add_space(14.0);
                ui.label(RichText::new("SEÇÕES").font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(1.0));
                ui.add_space(4.0);
                for k in kids {
                    let r = egui::Frame::new().stroke(Stroke::new(1.0_f32, p.border)).corner_radius(10.0).inner_margin(12.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&k.titulo).font(kit::medium(14.0)).color(p.text));
                        if !k.snippet.is_empty() {
                            ui.label(RichText::new(&k.snippet).size(12.5).color(p.muted));
                        }
                        ui.label(RichText::new(format!("{} palavras", fmt_n(k.words as i64))).size(11.0).color(p.faint));
                    });
                    let resp = ui.interact(r.response.rect, ui.id().with(("kid", &k.key)), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_stroke(r.response.rect, 10.0, Stroke::new(1.0_f32, p.border_strong), egui::StrokeKind::Inside);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.sel = Some(k.key.clone());
                    }
                    ui.add_space(6.0);
                }
            }
            ui.add_space(40.0);
        });
    }
}

/// Folders of the vault (for choosing sources).
pub(super) fn vault_folders(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { return };
        let mut v: Vec<String> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
        v.sort();
        for n in v {
            let rel = if dir.is_empty() { n } else { format!("{dir}/{n}") };
            out.push(rel.clone());
            walk(root, &rel, out);
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}
