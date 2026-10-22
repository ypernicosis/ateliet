//! Trilha: a book's soundtrack. Faixas (covers, play, themes and
//! the minutes written to each), Temas (the leitmotifs: tracks of a
//! character, chapter, scene, plot line, act or place) and Diário (what
//! played while each chapter was written). A track's panel ties it to
//! themes, keeps why it matters and plays it.

use super::library::Cx;
use super::Action;
use crate::icons;
use crate::kit;
use crate::music::{self, Cmd};
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::{self, Book};
use ateliet_core::scriptorium::enredo as en;
use ateliet_core::scriptorium::estruturas as es;
use ateliet_core::scriptorium::trilha::{self as tr, str_of};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Faixas,
    Temas,
    Diario,
}

struct AddTrack {
    nome: String,
    artista: String,
    link: String,
    arquivo: String,
}

/// Choosing what a track is the theme of.
struct ThemePick {
    kind: &'static str,
    filter: String,
    lugar: String,
}

enum Confirm {
    Remove(Vec<String>),
    Dedupe,
    Delete,
}

pub struct TrilhaView {
    pub book: String,
    tab: Tab,
    data: Json,
    loaded: bool,
    mtime: Option<SystemTime>,
    dirty: bool,
    last_edit: Option<Instant>,
    sel: Option<String>,
    q: String,
    picking: Option<HashSet<String>>,
    add: Option<AddTrack>,
    theme: Option<ThemePick>,
    confirm: Option<Confirm>,
    copy_from: bool,
    toast: Option<String>,
}

impl TrilhaView {
    pub fn new(book: &str) -> TrilhaView {
        TrilhaView { book: book.into(), tab: Tab::Faixas, data: tr::blank(), loaded: false, mtime: None, dirty: false, last_edit: None, sel: None, q: String::new(), picking: None, add: None, theme: None, confirm: None, copy_from: false, toast: None }
    }

    pub fn title(&self) -> String {
        "Trilha".into()
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
    }

    fn mtime(root: &Path, b: &Book) -> Option<SystemTime> {
        std::fs::metadata(root.join(tr::path(b))).and_then(|m| m.modified()).ok()
    }

    pub fn save(&mut self, root: &Path, ix: &Index, author: &str) -> Option<String> {
        if !self.dirty {
            return None;
        }
        let b = book::book_by_path(ix, &self.book, author)?;
        let r = tr::save(root, ix, &b, &self.data).ok();
        self.dirty = false;
        self.mtime = Self::mtime(root, &b);
        r
    }

    fn faixas(&self) -> Vec<Json> {
        self.data["faixas"].as_array().cloned().unwrap_or_default()
    }

    fn track_mut(&mut self, k: &str) -> Option<&mut Json> {
        self.data["faixas"].as_array_mut()?.iter_mut().find(|t| t["k"] == k)
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
        let m = Self::mtime(&root, &b);
        if !self.loaded || (!self.dirty && m != self.mtime) {
            self.data = tr::load(&root, &b);
            self.mtime = m;
            self.loaded = true;
        }
        if self.dirty && self.last_edit.is_some_and(|t| t.elapsed() > Duration::from_millis(600)) {
            if let Some(path) = self.save(&root, &ix, &author) {
                acts.push(Action::Saved(path));
            }
        }
        if self.dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        egui::TopBottomPanel::top(ui.id().with("tr-bar")).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 20, right: 16, top: 12, bottom: 10 })).show_separator_line(true).show_inside(ui, |ui| {
            self.bar(ui, p, &b, cx, &mut acts);
        });
        let sel = self.sel.clone().and_then(|k| tr::find(&self.data, &k).map(|i| self.data["faixas"][i].clone()));
        if let Some(t) = sel {
            let r = egui::SidePanel::right(ui.id().with("tr-drawer")).resizable(false).exact_width(340.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 22, right: 14, top: 16, bottom: 12 })).show_inside(ui, |ui| {
                self.drawer(ui, p, cx, &b, &ix, &t, &mut acts);
            });
            let rr = r.response.rect;
            ui.painter().vline(rr.left(), rr.y_range(), Stroke::new(1.0_f32, p.border));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show_inside(ui, |ui| {
            if self.data["faixas"].as_array().is_none_or(|a| a.is_empty()) && self.tab == Tab::Faixas {
                self.empty(ui, p, &b, cx);
                return;
            }
            match self.tab {
                Tab::Faixas => self.tracks(ui, p, cx, &b, &mut acts),
                Tab::Temas => self.themes(ui, p, cx, &b, &ix, &root, &mut acts),
                Tab::Diario => self.diary(ui, p, cx, &b, &ix, &mut acts),
            }
        });
        self.dialogs(ui.ctx(), p, &b, &ix, &root, &author, cx, &mut acts);
        if let Some(t) = self.toast.take() {
            acts.push(Action::Toast(t));
        }
        acts
    }

    fn bar(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, cx: &Cx, acts: &mut Vec<Action>) {
        let pl = self.data["fonte"]["playlist"].clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Trilha").font(kit::bold(17.0)).color(p.text));
                let sub = match pl["nome"].as_str() {
                    Some(n) => format!("{} · playlist \"{n}\"", b.title),
                    None => b.title.clone(),
                };
                ui.label(RichText::new(sub).size(11.5).color(p.faint));
            });
            ui.add_space(14.0);
            let tabs = [(icons::MUSIC, "Faixas"), (icons::SPARKLES, "Temas"), (icons::FEATHER, "Diário")];
            let cur = match self.tab {
                Tab::Faixas => 0,
                Tab::Temas => 1,
                Tab::Diario => 2,
            };
            if let Some(i) = kit::segmented(ui, &tabs, cur, p) {
                self.tab = [Tab::Faixas, Tab::Temas, Tab::Diario][i];
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(kit::icon(icons::ELLIPSIS, 15.0).color(p.muted), |ui| self.manage_menu(ui, p, b, cx, acts));
                if kit::icon_button(ui, icons::PLUS, "Adicionar faixa (link ou áudio do cofre)", false, p).clicked() {
                    self.add = Some(AddTrack { nome: String::new(), artista: String::new(), link: String::new(), arquivo: String::new() });
                }
                if music::mac() {
                    let connected = pl.is_object();
                    if let (true, Some(pid)) = (connected, pl["id"].as_str()) {
                        if kit::icon_button(ui, icons::PLAY, "Tocar a playlist no app Música", false, p).clicked() {
                            acts.push(Action::Music(Cmd::PlayPlaylist(pid.to_string())));
                        }
                    }
                    if connected && kit::icon_button(ui, icons::REFRESH_CW, &format!("Atualizar do app Música (última: {})", self.data["fonte"]["atualizado"].as_str().unwrap_or("—")), false, p).clicked() {
                        acts.push(Action::Music(Cmd::Refresh(b.path.clone())));
                    }
                    let lab = if connected { "Trocar playlist" } else { "Conectar app Música" };
                    let r = if self.faixas().is_empty() { kit::primary_icon(ui, icons::PLUG, lab, p) } else { kit::button(ui, Some(icons::PLUG), lab, p) };
                    if r.clicked() {
                        acts.push(Action::Music(Cmd::Connect(b.path.clone())));
                    }
                }
                if let Some(busy) = &cx.music.busy {
                    ui.add(egui::Spinner::new().size(12.0).color(p.faint));
                    ui.label(RichText::new(busy).size(12.0).color(p.muted));
                }
            });
        });
    }

    fn manage_menu(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, cx: &Cx, acts: &mut Vec<Action>) {
        let pl = self.data["fonte"]["playlist"].clone();
        if music::mac() {
            if pl.is_object() && kit::menu_item(ui, icons::REFRESH_CW, &format!("Atualizar do app Música (\"{}\")", pl["nome"].as_str().unwrap_or("")), p) {
                acts.push(Action::Music(Cmd::Refresh(b.path.clone())));
                ui.close_menu();
            }
            if kit::menu_item(ui, icons::PLUG, if pl.is_object() { "Trocar a playlist…" } else { "Conectar uma playlist do app Música…" }, p) {
                acts.push(Action::Music(Cmd::Connect(b.path.clone())));
                ui.close_menu();
            }
        }
        if kit::menu_item(ui, icons::PLUS, "Adicionar faixa (link ou áudio)…", p) {
            self.add = Some(AddTrack { nome: String::new(), artista: String::new(), link: String::new(), arquivo: String::new() });
            ui.close_menu();
        }
        if kit::menu_item(ui, icons::COPY, "Copiar faixas de outro livro…", p) {
            self.copy_from = true;
            ui.close_menu();
        }
        kit::menu_sep(ui, p);
        if !self.faixas().is_empty() && kit::menu_item(ui, icons::SQUARE_CHECK, "Selecionar faixas…", p) {
            self.tab = Tab::Faixas;
            self.sel = None;
            self.picking = Some(HashSet::new());
            ui.close_menu();
        }
        let dups = tr::duplicates(&self.data).len();
        if dups > 0 && kit::menu_item(ui, icons::COPY, &format!("Remover repetidas ({dups})"), p) {
            self.confirm = Some(Confirm::Dedupe);
            ui.close_menu();
        }
        let gone = self.data["removidas"].as_array().map(|a| a.len()).unwrap_or(0);
        if gone > 0 && kit::menu_item(ui, icons::UNDO_2, &format!("Trazer de volta as removidas ({gone})"), p) {
            self.data["removidas"] = json!([]);
            self.changed();
            self.toast = Some(if pl.is_object() { "Pronto. Elas voltam na próxima atualização do app Música.".into() } else { "Pronto.".into() });
            ui.close_menu();
        }
        if pl.is_object() && kit::menu_item(ui, icons::UNLINK, "Desligar da playlist (mantém as faixas)", p) {
            self.data["fonte"] = Json::Null;
            self.changed();
            ui.close_menu();
        }
        let _ = cx;
        kit::menu_sep(ui, p);
        if kit::menu_danger(ui, icons::TRASH_2, "Apagar esta trilha…", p) {
            self.confirm = Some(Confirm::Delete);
            ui.close_menu();
        }
    }

    fn empty(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book, cx: &Cx) {
        ui.add_space(90.0);
        ui.vertical_centered(|ui| {
            ui.label(kit::icon(icons::MUSIC, 34.0).color(p.faint));
            ui.add_space(14.0);
            ui.label(RichText::new("Nenhuma faixa ainda").font(kit::medium(17.0)).color(p.text));
            ui.add_space(6.0);
            ui.allocate_ui(vec2(470.0, 80.0), |ui| {
                ui.label(RichText::new(if music::mac() { "Conecte a playlist do livro no app Música: o Ateliet lê nome, artista, álbum, ano e capa de cada faixa. Você também pode adicionar links ou áudios do cofre." } else { "Adicione links (Spotify, YouTube, SoundCloud, Apple Music) ou áudios do cofre. No Mac, a playlist do livro pode vir do app Música." }).color(p.muted));
            });
            ui.add_space(14.0);
            if kit::primary_icon(ui, icons::PLUS, "Adicionar faixa", p).clicked() {
                self.add = Some(AddTrack { nome: String::new(), artista: String::new(), link: String::new(), arquivo: String::new() });
            }
            let _ = (b, cx);
        });
        self.legacy(ui, p, b);
    }

    /// The links of the book's `playlist` property.
    fn legacy(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book) {
        let items: Vec<String> = match &b.fm["playlist"] {
            Json::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
            Json::String(s) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        };
        let list = tr::playlist_items(&items);
        if list.is_empty() {
            return;
        }
        ui.add_space(24.0);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 0)).show(ui, |ui| {
            ui.label(RichText::new("LINKS DA PROPRIEDADE \"PLAYLIST\"").size(10.5).color(p.faint));
            ui.add_space(4.0);
            for (title, src) in list {
                let lab = if title.is_empty() { src.clone() } else { title };
                if kit::Row::new(&lab).icon(icons::LINK).trailing(tr::service(&src).unwrap_or("abrir")).show(ui, p).clicked() {
                    let _ = open::that(&src);
                }
            }
        });
    }

    fn minutes(&self, k: &str) -> f64 {
        self.data["diario"].as_object().map(|d| d.values().map(|ch| ch[k].as_f64().unwrap_or(0.0)).sum()).unwrap_or(0.0)
    }

    /// One track as a card; returns its response.
    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, b: &Book, t: &Json, w: f32, acts: &mut Vec<Action>) {
        let k = str_of(t, "k").to_string();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 72.0), Sense::click());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter().clone();
        let sel = self.sel.as_deref() == Some(k.as_str());
        let now = cx.music.now.key == k && cx.music.now.some();
        let picked = self.picking.as_ref().map(|s| s.contains(&k));
        painter.rect(rect, 10.0, if picked == Some(true) { p.accent_soft } else { p.bg }, Stroke::new(if sel { 1.5_f32 } else { 1.0 }, if sel || now { p.accent_strong } else if resp.hovered() { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
        let art = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(56.0, 56.0));
        cover(ui, cx, str_of(t, "capa"), art, p);
        // play over the cover
        let play = ui.interact(art, ui.id().with(("tr-play", &k)), Sense::click());
        if play.hovered() || now {
            painter.rect_filled(art, 7.0, Color32::from_black_alpha(90));
            let g = if now && cx.music.now.playing { icons::PAUSE } else { icons::PLAY };
            painter.text(art.center(), egui::Align2::CENTER_CENTER, g, kit::icon_font(18.0), Color32::WHITE);
        }
        if play.clicked() {
            if now {
                acts.push(Action::Music(Cmd::PlayPause));
            } else {
                acts.push(Action::Music(Cmd::Play(b.path.clone(), k.clone(), None)));
            }
        }
        let x = art.right() + 12.0;
        let right = rect.right() - 12.0;
        let dur = t["dur"].as_f64().filter(|d| *d > 0.0).map(tr::fmt_dur);
        let dw = if dur.is_some() { 40.0 } else { 0.0 };
        let g = super::enredo_ui::truncated(ui, str_of(t, "nome"), kit::medium(13.5), p.text, right - x - dw);
        painter.galley(pos2(x, rect.top() + 12.0), g, p.text);
        let meta = [str_of(t, "artista"), str_of(t, "album")].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" · ");
        let g = super::enredo_ui::truncated(ui, &meta, egui::FontId::proportional(12.0), p.muted, right - x);
        painter.galley(pos2(x, rect.top() + 31.0), g, p.muted);
        let mut bx = x;
        let nt = t["temas"].as_array().map(|a| a.len()).unwrap_or(0);
        let secs = self.minutes(&k);
        for (lab, strong) in [(if nt > 0 { format!("{nt} {}", if nt == 1 { "tema" } else { "temas" }) } else { String::new() }, true), (if secs >= 60.0 { format!("{} {}", icons::FEATHER, tr::fmt_min(secs)) } else { String::new() }, false)] {
            if lab.is_empty() {
                continue;
            }
            let g = painter.layout_no_wrap(lab, egui::FontId::proportional(10.5), if strong { p.accent_strong } else { p.faint });
            let r = Rect::from_min_size(pos2(bx, rect.top() + 49.0), g.size() + vec2(10.0, 4.0));
            painter.rect_filled(r, 5.0, if strong { p.accent_soft } else { p.bg_secondary });
            painter.galley(r.min + vec2(5.0, 2.0), g, p.text);
            bx = r.right() + 5.0;
        }
        if let Some(d) = dur {
            painter.text(pos2(right, rect.top() + 14.0), egui::Align2::RIGHT_TOP, d, egui::FontId::proportional(11.0), p.faint);
        }
        if let Some(on) = picked {
            painter.text(pos2(right, rect.bottom() - 14.0), egui::Align2::RIGHT_CENTER, if on { icons::SQUARE_CHECK } else { icons::SQUARE }, kit::icon_font(15.0), if on { p.accent_strong } else { p.faint });
        }
        if resp.clicked() && !play.clicked() {
            match self.picking.as_mut() {
                Some(set) => {
                    if !set.remove(&k) {
                        set.insert(k.clone());
                    }
                }
                None => self.sel = if sel { None } else { Some(k.clone()) },
            }
        }
    }

    fn grid(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, b: &Book, list: &[Json], acts: &mut Vec<Action>) {
        let avail = ui.available_width();
        let per = ((avail + 10.0) / 290.0).floor().max(1.0);
        let w = (avail - 10.0 * (per - 1.0)) / per;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            for t in list {
                self.card(ui, p, cx, b, t, w, acts);
            }
        });
    }

    fn tracks(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, b: &Book, acts: &mut Vec<Action>) {
        let all = self.faixas();
        egui::ScrollArea::vertical().id_salt("tr-tracks").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 16, bottom: 24 }).show(ui, |ui| {
                if let Some(set) = self.picking.clone() {
                    ui.horizontal(|ui| {
                        let n = set.len();
                        ui.label(RichText::new(if n == 0 { "Clique nas faixas para selecionar".to_string() } else { format!("{n} {}", if n == 1 { "selecionada" } else { "selecionadas" }) }).color(p.muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if kit::primary(ui, "Concluir", p).clicked() {
                                self.picking = None;
                            }
                            if ui.add_enabled_ui(n > 0, |ui| kit::danger(ui, "Remover", p)).inner.clicked() {
                                self.confirm = Some(Confirm::Remove(set.iter().cloned().collect()));
                            }
                            if kit::quiet(ui, None, if n == all.len() { "Nenhuma" } else { "Todas" }, p).clicked() {
                                self.picking = Some(if n == all.len() { HashSet::new() } else { all.iter().map(|t| str_of(t, "k").to_string()).collect() });
                            }
                        });
                    });
                    ui.add_space(10.0);
                } else {
                    let dups = tr::duplicates(&self.data).len();
                    if dups > 0 {
                        ui.horizontal(|ui| {
                            ui.label(kit::icon(icons::COPY, 13.0).color(p.muted));
                            ui.label(RichText::new(format!("{dups} {} nesta trilha.", if dups == 1 { "faixa aparece repetida" } else { "faixas aparecem repetidas" })).color(p.muted));
                            if kit::quiet(ui, None, "Remover repetidas", p).clicked() {
                                self.confirm = Some(Confirm::Dedupe);
                            }
                        });
                        ui.add_space(8.0);
                    }
                }
                ui.allocate_ui(vec2(ui.available_width().min(420.0), 30.0), |ui| {
                    kit::search_field(ui, &mut self.q, "Buscar faixa, artista, álbum…", p);
                });
                ui.add_space(12.0);
                let q = tr::uni_slug(&self.q).replace('-', " ");
                let list: Vec<Json> = all.into_iter().filter(|t| self.q.trim().is_empty() || tr::uni_slug(&format!("{} {} {}", str_of(t, "nome"), str_of(t, "artista"), str_of(t, "album"))).replace('-', " ").contains(&q)).collect();
                self.grid(ui, p, cx, b, &list, acts);
                self.legacy(ui, p, b);
            });
        });
    }

    fn ref_label(&self, b: &Book, ix: &Index, root: &Path, m: &Json, en_data: &Json) -> String {
        let r = str_of(m, "ref");
        match str_of(m, "t") {
            "personagem" => super::enredo_ui::note_name(r),
            "capitulo" => ix.page(r).and_then(|pg| pg.yaml["titulo"].as_str().map(String::from)).unwrap_or_else(|| super::enredo_ui::note_name(r)),
            "cena" => en::scene_index(en_data, r).map(|i| str_of(&en_data["cenas"][i], "titulo").to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Cena sem título".into()),
            "trama" => en::trama(en_data, r).map(|t| str_of(t, "nome").to_string()).unwrap_or_else(|| r.to_string()),
            "batida" => beats(root, en_data).into_iter().find(|x| x.1 == r).map(|x| x.0).unwrap_or_else(|| r.to_string()),
            "livro" => b.title.clone(),
            _ => r.to_string(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn themes(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, b: &Book, ix: &Index, root: &Path, acts: &mut Vec<Action>) {
        let en_data = en::load(root, b);
        type Group = (String, Vec<(String, Vec<Json>)>);
        let mut groups: Vec<Group> = Vec::new();
        for (t, _) in tr::TEMA_TIPOS {
            let mut items: Vec<(String, Vec<Json>)> = Vec::new();
            for f in self.faixas() {
                for m in f["temas"].as_array().into_iter().flatten().filter(|m| m["t"] == t) {
                    let lab = self.ref_label(b, ix, root, m, &en_data);
                    match items.iter_mut().find(|x| x.0 == lab) {
                        Some(x) => x.1.push(f.clone()),
                        None => items.push((lab, vec![f.clone()])),
                    }
                }
            }
            if !items.is_empty() {
                items.sort_by(|a, b| a.0.cmp(&b.0));
                groups.push((t.to_string(), items));
            }
        }
        egui::ScrollArea::vertical().id_salt("tr-themes").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 18, bottom: 24 }).show(ui, |ui| {
                if groups.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(kit::icon(icons::SPARKLES, 30.0).color(p.faint));
                        ui.add_space(10.0);
                        ui.allocate_ui(vec2(460.0, 60.0), |ui| {
                            ui.label(RichText::new("Nenhum tema ainda. Abra uma faixa e ligue-a a um personagem, capítulo, cena, trama, ato ou lugar: ela vira o leitmotiv dele.").color(p.muted));
                        });
                    });
                    return;
                }
                for (tipo, items) in groups {
                    ui.label(RichText::new(tr::tema_label(&tipo).to_uppercase()).font(kit::medium(10.5)).color(p.faint).extra_letter_spacing(0.8));
                    ui.add_space(6.0);
                    for (lab, tracks) in items {
                        ui.label(RichText::new(lab).font(kit::medium(14.0)).color(p.text));
                        ui.add_space(4.0);
                        self.grid(ui, p, cx, b, &tracks, acts);
                        ui.add_space(12.0);
                    }
                    ui.add_space(10.0);
                }
            });
        });
    }

    fn diary(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, b: &Book, ix: &Index, acts: &mut Vec<Action>) {
        let pending = cx.music.pending(&b.path).cloned();
        egui::ScrollArea::vertical().id_salt("tr-diary").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 18, bottom: 24 }).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(760.0));
                ui.label(RichText::new("O diário conta só o tempo em que a música tocava enquanto você escrevia num capítulo (áudio do cofre ou app Música).").size(12.0).color(p.faint));
                ui.add_space(12.0);
                let mut any = false;
                for c in book::chapters(ix, b) {
                    let rows = tr::diary_for(&self.data, &c.path, pending.as_ref());
                    let insp = self.data["inspiracao"][&c.path].clone();
                    if rows.is_empty() && !insp.is_object() {
                        continue;
                    }
                    any = true;
                    let total: f64 = rows.iter().map(|r| r.1).sum();
                    ui.horizontal(|ui| {
                        if ui.add(egui::Label::new(RichText::new(&c.title).font(kit::medium(14.5)).color(p.text)).sense(Sense::click())).on_hover_text("Abrir o capítulo").clicked() {
                            acts.push(Action::Open(c.path.clone(), false));
                        }
                        ui.label(RichText::new(tr::fmt_min(total)).size(12.0).color(p.faint));
                    });
                    if let Some(m) = insp["k"].as_str().and_then(|k| tr::meta(&self.data, k)) {
                        ui.label(RichText::new(format!("{} escrito ouvindo: {} — {}", icons::FEATHER, str_of(&m, "nome"), str_of(&m, "artista"))).italics().size(12.5).color(p.accent_strong));
                    }
                    ui.add_space(4.0);
                    let max = rows.first().map(|r| r.1).unwrap_or(1.0).max(1.0);
                    for (k, s) in rows.iter().take(8) {
                        let m = tr::meta(&self.data, k).unwrap_or(json!({ "nome": k }));
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                        let bar = Rect::from_min_size(r.min + vec2(0.0, 3.0), vec2((s / max) as f32 * r.width().min(520.0), 20.0));
                        ui.painter().rect_filled(bar, 5.0, p.accent_soft);
                        let lab = format!("{} — {}", str_of(&m, "nome"), str_of(&m, "artista"));
                        let g = super::enredo_ui::truncated(ui, &lab, egui::FontId::proportional(12.5), p.text, r.width() - 80.0);
                        ui.painter().galley(pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, p.text);
                        ui.painter().text(pos2(r.right(), r.center().y), egui::Align2::RIGHT_CENTER, tr::fmt_min(*s), egui::FontId::proportional(11.5), p.faint);
                        if resp.clicked() && tr::find(&self.data, k).is_some() {
                            self.tab = Tab::Faixas;
                            self.sel = Some(k.clone());
                        }
                    }
                    ui.add_space(16.0);
                }
                if !any {
                    ui.add_space(30.0);
                    ui.label(RichText::new("Nada registrado ainda. Escreva com música tocando e o diário aparece aqui.").color(p.muted));
                }
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn drawer(&mut self, ui: &mut egui::Ui, p: Palette, cx: &mut Cx, b: &Book, ix: &Index, t: &Json, acts: &mut Vec<Action>) {
        let k = str_of(t, "k").to_string();
        let root = cx.vault.root.clone();
        egui::ScrollArea::vertical().id_salt("tr-drawer-s").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("FAIXA").font(kit::medium(10.5)).color(p.faint));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::icon_button(ui, icons::X, "Fechar", false, p).clicked() {
                        self.sel = None;
                    }
                });
            });
            ui.add_space(6.0);
            let (art, _) = ui.allocate_exact_size(vec2(200.0, 200.0), Sense::hover());
            cover(ui, cx, str_of(t, "capa"), art, p);
            ui.add_space(10.0);
            ui.label(RichText::new(str_of(t, "nome")).font(kit::medium(17.0)).color(p.text));
            let meta = [str_of(t, "artista").to_string(), str_of(t, "album").to_string(), t["ano"].as_i64().map(|y| y.to_string()).or(t["ano"].as_str().map(String::from)).unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            ui.label(RichText::new(meta).color(p.muted));
            let extra = [str_of(t, "genero").to_string(), t["dur"].as_f64().filter(|d| *d > 0.0).map(tr::fmt_dur).unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            if !extra.is_empty() {
                ui.label(RichText::new(extra).size(12.0).color(p.faint));
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if kit::primary_icon(ui, icons::PLAY, "Tocar", p).clicked() {
                    acts.push(Action::Music(Cmd::Play(b.path.clone(), k.clone(), None)));
                }
                if let Some(ch) = cx.music.last_chapter.clone().filter(|c| c.starts_with(&format!("{}/", b.folder))) {
                    if kit::button(ui, Some(icons::PEN_LINE), "Escrever ouvindo", p).on_hover_text(format!("Toca e anota que {} foi escrito ouvindo esta faixa", super::enredo_ui::note_name(&ch))).clicked() {
                        acts.push(Action::Music(Cmd::Play(b.path.clone(), k.clone(), Some(ch))));
                    }
                }
                if music::mac() {
                    if let (Some(pid), Some(tid)) = (t["app"]["pid"].as_str(), t["app"]["tid"].as_str()) {
                        if kit::quiet(ui, Some(icons::QUOTE), "Ver no app", p).clicked() {
                            acts.push(Action::Music(Cmd::Reveal(pid.into(), tid.into())));
                        }
                    }
                }
                if !str_of(t, "link").is_empty() && kit::quiet(ui, Some(icons::EXTERNAL_LINK), &format!("Abrir no {}", tr::service(str_of(t, "link")).unwrap_or("navegador")), p).clicked() {
                    let _ = open::that(str_of(t, "link"));
                }
            });
            ui.add_space(14.0);
            kit::hairline(ui, p);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(kit::icon(icons::SPARKLES, 13.0).color(p.muted));
                ui.label(RichText::new("Tema de…").font(kit::medium(13.0)).color(p.text));
            });
            ui.add_space(6.0);
            let en_data = en::load(&root, b);
            let temas: Vec<Json> = t["temas"].as_array().cloned().unwrap_or_default();
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (i, m) in temas.iter().enumerate() {
                    let lab = format!("{}: {}", tr::tema_label(str_of(m, "t")), self.ref_label(b, ix, &root, m, &en_data));
                    if kit::chip(ui, Some(icons::X), &lab, true, p).on_hover_text("Tirar este tema").clicked() {
                        remove = Some(i);
                    }
                }
                let add = kit::chip(ui, Some(icons::PLUS), "tema", false, p);
                let popup = ui.make_persistent_id(("tr-theme", &k));
                if add.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(popup));
                }
                egui::popup_below_widget(ui, popup, &add, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(200.0);
                    for (kind, glyph, lab) in [("personagem", icons::USER, "Personagem…"), ("capitulo", icons::BOOK, "Capítulo…"), ("cena", icons::CLAPPERBOARD, "Cena…"), ("trama", icons::GIT_FORK, "Trama…"), ("batida", icons::MILESTONE, "Ato / batida da estrutura…"), ("lugar", icons::MAP_PIN, "Lugar…")] {
                        if kit::menu_item(ui, glyph, lab, p) {
                            self.theme = Some(ThemePick { kind, filter: String::new(), lugar: String::new() });
                        }
                    }
                    if kit::menu_item(ui, icons::BOOK_OPEN, "O livro inteiro", p) {
                        add_theme(&mut self.data, &k, "livro", &b.path);
                        self.changed();
                    }
                });
            });
            if let Some(i) = remove {
                if let Some(tt) = self.track_mut(&k) {
                    if let Some(a) = tt["temas"].as_array_mut() {
                        a.remove(i);
                    }
                }
                self.changed();
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.label(kit::icon(icons::STICKY_NOTE, 13.0).color(p.muted));
                ui.label(RichText::new("Por que esta música?").font(kit::medium(13.0)).color(p.text));
            });
            ui.add_space(4.0);
            let mut notas = str_of(t, "notas").to_string();
            if ui.add(egui::TextEdit::multiline(&mut notas).desired_rows(4).desired_width(f32::INFINITY).hint_text(RichText::new("O clima, o personagem, a cena que ela evoca…").color(p.faint)).margin(egui::Margin::symmetric(8, 6))).changed() {
                if let Some(tt) = self.track_mut(&k) {
                    tt["notas"] = json!(notas);
                }
                self.changed();
            }
            // where it played while writing
            let pending = cx.music.pending(&b.path).cloned();
            let mut rows: Vec<(String, f64)> = Vec::new();
            for c in book::chapters(ix, b) {
                let s: f64 = tr::diary_for(&self.data, &c.path, pending.as_ref()).into_iter().filter(|r| r.0 == k).map(|r| r.1).sum();
                if s > 0.0 {
                    rows.push((c.path.clone(), s));
                }
            }
            if !rows.is_empty() {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(kit::icon(icons::FEATHER, 13.0).color(p.muted));
                    ui.label(RichText::new("Tocou enquanto você escrevia").font(kit::medium(13.0)).color(p.text));
                });
                rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (ch, s) in rows {
                    let title = ix.page(&ch).and_then(|pg| pg.yaml["titulo"].as_str().map(String::from)).unwrap_or_else(|| super::enredo_ui::note_name(&ch));
                    if kit::Row::new(&title).icon(icons::FILE_TEXT).trailing(tr::fmt_min(s)).show(ui, p).clicked() {
                        acts.push(Action::Open(ch.clone(), false));
                    }
                }
            }
            ui.add_space(18.0);
            if kit::quiet(ui, Some(icons::TRASH_2), "Remover da trilha", p).clicked() {
                self.confirm = Some(Confirm::Remove(vec![k.clone()]));
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn dialogs(&mut self, ctx: &egui::Context, p: Palette, b: &Book, ix: &Index, root: &Path, author: &str, cx: &mut Cx, acts: &mut Vec<Action>) {
        if self.add.is_some() {
            self.add_dialog(ctx, p, ix);
        } else if self.theme.is_some() {
            self.theme_dialog(ctx, p, b, ix, root);
        } else if self.confirm.is_some() {
            self.confirm_dialog(ctx, p, b, root, cx, acts);
        } else if self.copy_from {
            self.copy_dialog(ctx, p, b, ix, root, author);
        }
    }

    fn add_dialog(&mut self, ctx: &egui::Context, p: Palette, ix: &Index) {
        let Some(a) = self.add.as_mut() else { return };
        let mut done = None;
        kit::scrim(ctx, p);
        let audios: Vec<String> = ix.file_paths().filter(|f| tr::is_audio(f)).cloned().collect();
        egui::Window::new("tr-add").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 100.0]).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new("Adicionar faixa").font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            let field = |ui: &mut egui::Ui, l: &str, v: &mut String, hint: &str| {
                ui.label(RichText::new(l).size(12.0).color(p.muted));
                ui.add(egui::TextEdit::singleline(v).desired_width(f32::INFINITY).hint_text(RichText::new(hint).color(p.faint)).margin(egui::Margin::symmetric(8, 6)));
                ui.add_space(6.0);
            };
            field(ui, "Nome da faixa", &mut a.nome, "");
            field(ui, "Artista", &mut a.artista, "");
            field(ui, "Link (Spotify, YouTube, SoundCloud, Apple Music), opcional", &mut a.link, "https://…");
            ui.label(RichText::new("Ou um áudio do cofre").size(12.0).color(p.muted));
            egui::ComboBox::from_id_salt("tr-audio").width(400.0).selected_text(if a.arquivo.is_empty() { "nenhum".to_string() } else { a.arquivo.clone() }).show_ui(ui, |ui| {
                ui.selectable_value(&mut a.arquivo, String::new(), "nenhum");
                for f in &audios {
                    ui.selectable_value(&mut a.arquivo, f.clone(), f.as_str());
                }
            });
            if audios.is_empty() {
                ui.label(RichText::new("Nenhum áudio no cofre (mp3, m4a, ogg, wav, flac).").size(11.5).color(p.faint));
            }
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled_ui(!a.nome.trim().is_empty() || !a.arquivo.is_empty(), |ui| kit::primary(ui, "Adicionar", p)).inner.clicked() {
                    done = Some(true);
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                let a = self.add.take().unwrap();
                let nome = if a.nome.trim().is_empty() { super::enredo_ui::note_name(&a.arquivo).rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_default() } else { a.nome.trim().to_string() };
                let k = tr::track_key(a.artista.trim(), &nome);
                if tr::find(&self.data, &k).is_none() {
                    self.data["faixas"].as_array_mut().unwrap().push(json!({ "k": k, "nome": nome, "artista": a.artista.trim(), "link": a.link.trim(), "arquivo": a.arquivo, "temas": [], "notas": "" }));
                    self.changed();
                }
                self.sel = Some(k);
            }
            Some(false) => self.add = None,
            None => {}
        }
    }

    fn theme_dialog(&mut self, ctx: &egui::Context, p: Palette, b: &Book, ix: &Index, root: &Path) {
        let Some(k) = self.sel.clone() else {
            self.theme = None;
            return;
        };
        let Some(tp) = self.theme.as_mut() else { return };
        let kind = tp.kind;
        let items: Vec<(String, String)> = match kind {
            "personagem" => book::characters(ix, b).into_iter().map(|c| (super::enredo_ui::note_name(&c), c)).collect(),
            "capitulo" => book::chapters(ix, b).into_iter().map(|c| (c.title.clone(), c.path)).collect(),
            "cena" => {
                let d = en::load(root, b);
                let cols = en::chapters(ix, b, &d);
                en::ordered(&cols, &d).into_iter().map(|i| {
                    let s = &d["cenas"][i];
                    let t = str_of(s, "titulo");
                    let cap = en::cap_of(s).map(|c| format!(" — {}", super::enredo_ui::note_name(c))).unwrap_or_default();
                    (format!("{}{cap}", if t.is_empty() { "Cena sem título" } else { t }), str_of(s, "id").to_string())
                }).collect()
            }
            "trama" => en::load(root, b)["tramas"].as_array().into_iter().flatten().map(|t| (str_of(t, "nome").to_string(), str_of(t, "id").to_string())).collect(),
            "batida" => beats(root, &en::load(root, b)),
            _ => Vec::new(),
        };
        let f = ateliet_core::scriptorium::mundo::deaccent(&tp.filter);
        let shown: Vec<&(String, String)> = items.iter().filter(|x| f.is_empty() || ateliet_core::scriptorium::mundo::deaccent(&x.0).contains(&f)).collect();
        let mut chosen = None;
        let mut close = false;
        kit::scrim(ctx, p);
        egui::Window::new("tr-theme").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 100.0]).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(format!("Tema de {}", match kind { "personagem" => "qual personagem?", "capitulo" => "qual capítulo?", "cena" => "qual cena?", "trama" => "qual trama?", "batida" => "qual ato?", _ => "um lugar" })).font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            if kind == "lugar" {
                let r = ui.add(egui::TextEdit::singleline(&mut tp.lugar).desired_width(f32::INFINITY).hint_text(RichText::new("Ex.: Düsseldorf, a torre, o deserto").color(p.faint)).margin(egui::Margin::symmetric(8, 6)));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !tp.lugar.trim().is_empty() {
                    chosen = Some(tp.lugar.trim().to_string());
                }
            } else {
                ui.add(egui::TextEdit::singleline(&mut tp.filter).hint_text(RichText::new("Filtrar").color(p.faint)).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(8, 6))).request_focus();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    if shown.is_empty() {
                        ui.label(RichText::new("Nada aqui ainda.").color(p.faint));
                    }
                    for (lab, id) in shown {
                        if kit::Row::new(lab).show(ui, p).clicked() {
                            chosen = Some(id.clone());
                        }
                    }
                });
            }
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kind == "lugar" && kit::primary(ui, "Ligar", p).clicked() && !tp.lugar.trim().is_empty() {
                    chosen = Some(tp.lugar.trim().to_string());
                }
                close = kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if let Some(r) = chosen {
            add_theme(&mut self.data, &k, kind, &r);
            self.theme = None;
            self.changed();
        } else if close {
            self.theme = None;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, p: Palette, b: &Book, root: &Path, cx: &mut Cx, acts: &mut Vec<Action>) {
        let Some(c) = self.confirm.as_ref() else { return };
        let (title, msg, ok) = match c {
            Confirm::Remove(keys) => {
                let from_app = keys.iter().filter(|k| tr::find(&self.data, k).is_some_and(|i| self.data["faixas"][i]["app"].is_object())).count();
                let first = keys.first().and_then(|k| tr::meta(&self.data, k)).map(|m| str_of(&m, "nome").to_string()).unwrap_or_default();
                let mut msg = if keys.len() == 1 { format!("\"{first}\" sai da trilha de {}, com temas e notas.", b.title) } else { format!("{} faixas saem da trilha de {}, com temas e notas.", keys.len(), b.title) };
                if from_app > 0 {
                    msg += " As que vêm da playlist do app Música não voltam quando você atualizar (dá para trazê-las de volta no menu ⋯); a playlist não é alterada.";
                }
                (if keys.len() == 1 { "Remover faixa?".to_string() } else { format!("Remover {} faixas?", keys.len()) }, msg, "Remover")
            }
            Confirm::Dedupe => {
                let n = tr::duplicates(&self.data).len();
                (format!("Remover {n} {}?", if n == 1 { "repetida" } else { "repetidas" }), "Fica uma cópia de cada; temas, notas e o diário das cópias passam para ela.".to_string(), "Remover")
            }
            Confirm::Delete => (format!("Apagar a trilha de {}?", b.title), "As faixas, os temas, as notas e o diário desta trilha vão para a lixeira, com as capas baixadas (pasta Trilha). A playlist no app Música não é alterada.".to_string(), "Apagar"),
        };
        let mut done = None;
        kit::scrim(ctx, p);
        egui::Window::new("tr-confirm").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 140.0]).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new(&title).font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            ui.label(RichText::new(&msg).color(p.muted));
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::danger(ui, ok, p).clicked() {
                    done = Some(true);
                }
                if kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        let Some(go) = done else { return };
        let c = self.confirm.take().unwrap();
        if !go {
            return;
        }
        match c {
            Confirm::Remove(keys) => {
                let mut rem: Vec<Json> = self.data["removidas"].as_array().cloned().unwrap_or_default();
                let gone: Vec<Json> = self.faixas().into_iter().filter(|t| keys.iter().any(|k| t["k"] == k.as_str())).collect();
                for t in &gone {
                    if t["app"].is_object() && !rem.contains(&t["k"]) {
                        rem.push(t["k"].clone());
                    }
                }
                self.data["removidas"] = json!(rem);
                self.data["faixas"].as_array_mut().unwrap().retain(|t| !keys.iter().any(|k| t["k"] == k.as_str()));
                // covers no one uses any more go to the trash
                let mine = format!("{}/Trilha/capas/", b.folder);
                for t in gone {
                    let c = str_of(&t, "capa");
                    if c.starts_with(&mine) && !self.faixas().iter().any(|x| x["capa"] == c) {
                        let _ = trash::delete(root.join(c));
                    }
                }
                if self.sel.as_ref().is_some_and(|s| keys.contains(s)) {
                    self.sel = None;
                }
                self.picking = None;
                self.changed();
            }
            Confirm::Dedupe => {
                let (removed, rekeyed) = tr::tidy(&mut self.data);
                self.changed();
                self.toast = Some(format!("{removed} {}.{}", if removed == 1 { "repetida removida" } else { "repetidas removidas" }, if rekeyed > 0 { format!(" {rekeyed} {} a capa de outra música: atualize do app Música para buscar as certas.", if rekeyed == 1 { "faixa tinha" } else { "faixas tinham" }) } else { String::new() }));
            }
            Confirm::Delete => {
                let _ = trash::delete(root.join(format!("{}/Trilha", b.folder)));
                let _ = trash::delete(root.join(tr::path(b)));
                self.data = tr::blank();
                self.dirty = false;
                self.sel = None;
                self.picking = None;
                acts.push(Action::Saved(tr::path(b)));
                self.toast = Some(format!("Trilha de {} apagada.", b.title));
            }
        }
        let _ = cx;
    }

    fn copy_dialog(&mut self, ctx: &egui::Context, p: Palette, b: &Book, ix: &Index, root: &Path, author: &str) {
        let others: Vec<Book> = book::books(ix, author).into_iter().filter(|x| x.path != b.path && root.join(tr::path(x)).exists()).collect();
        let mut chosen = None;
        let mut close = false;
        kit::scrim(ctx, p);
        egui::Window::new("tr-copy").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 110.0]).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new("Copiar as faixas de qual livro?").font(kit::medium(16.0)).color(p.text));
            ui.add_space(8.0);
            if others.is_empty() {
                ui.label(RichText::new("Nenhum outro livro tem trilha ainda.").color(p.faint));
            }
            for o in &others {
                let lab = if o.serie.is_empty() { o.title.clone() } else { format!("{} · {}", o.title, o.serie) };
                if kit::Row::new(&lab).icon(icons::BOOK).show(ui, p).clicked() {
                    chosen = Some(o.clone());
                }
            }
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = kit::button(ui, None, "Cancelar", p).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if let Some(o) = chosen {
            self.copy_from = false;
            let src = tr::load(root, &o);
            let mut n = 0;
            for t in src["faixas"].as_array().into_iter().flatten() {
                let dup = self.faixas().iter().any(|x| x["k"] == t["k"] || (x["app"]["tid"].is_string() && x["app"]["tid"] == t["app"]["tid"]));
                if dup {
                    continue;
                }
                let mut c = t.clone();
                c["temas"] = json!([]);
                self.data["faixas"].as_array_mut().unwrap().push(c);
                n += 1;
            }
            self.changed();
            self.toast = Some(if n > 0 { format!("{n} {} de {} (sem os temas, que são de lá).", if n == 1 { "faixa copiada" } else { "faixas copiadas" }, o.title) } else { format!("Todas as faixas de {} já estavam aqui.", o.title) });
        } else if close {
            self.copy_from = false;
        }
    }
}

fn add_theme(data: &mut Json, k: &str, kind: &str, r: &str) {
    let Some(t) = data["faixas"].as_array_mut().and_then(|a| a.iter_mut().find(|t| t["k"] == k)) else { return };
    if !t["temas"].is_array() {
        t["temas"] = json!([]);
    }
    let a = t["temas"].as_array_mut().unwrap();
    a.retain(|m| !(m["t"] == kind && m["ref"] == r));
    a.push(json!({ "t": kind, "ref": r }));
}

/// The beats of the structures applied to the book: (label, "app:beat").
fn beats(root: &Path, en_data: &Json) -> Vec<(String, String)> {
    let apps: Vec<Json> = en_data["estruturas"].as_array().cloned().unwrap_or_default();
    let many = apps.len() > 1;
    let mut out = Vec::new();
    for a in &apps {
        let Some(st) = es::get(root, a["ref"].as_str().unwrap_or("")) else { continue };
        for bt in &st.batidas {
            out.push((if many { format!("{} › {}", st.nome, bt.label()) } else { bt.label() }, format!("{}:{}", a["id"].as_str().unwrap_or(""), bt.id)));
        }
    }
    out
}

/// A track's cover (or a note glyph).
fn cover(ui: &mut egui::Ui, cx: &mut Cx, capa: &str, r: Rect, p: Palette) {
    let shown = (!capa.is_empty()).then(|| crate::host::image(ui.ctx(), cx.vault, cx.caches, capa, "")).flatten();
    match shown {
        Some((id, sz)) => {
            let k = (r.width() / sz.x).max(r.height() / sz.y);
            let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(r.width() / (sz.x * k), r.height() / (sz.y * k)));
            ui.painter().add(egui::epaint::RectShape::filled(r, 7.0, Color32::WHITE).with_texture(id, uv));
        }
        None => {
            ui.painter().rect_filled(r, 7.0, p.bg_active);
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, icons::MUSIC, kit::icon_font(r.width() * 0.32), p.faint);
        }
    }
}
