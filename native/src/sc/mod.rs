//! Scriptorium, native: the library of books, the manuscript panel, the
//! cast… The model lives in `ateliet_core::scriptorium`; this keeps the
//! caches (word counts, book statistics) and the views.

pub mod actions;
pub mod binder;
pub mod capa_ui;
pub mod cast_ui;
pub mod compositor;
pub mod diagramacao_ui;
pub mod enredo_board;
pub mod enredo_more;
pub mod enredo_saga;
pub mod enredo_st;
pub mod enredo_ui;
pub mod picker;
pub mod sheet_ui;
pub mod trilha_ui;
pub mod export_ui;
pub mod focus;
pub mod library;
pub mod mapa_export;
pub mod mapa_ui;
pub mod mundo_mapas;
pub mod mundo_more;
pub mod mundo_ui;
pub mod pagina_ui;

use crate::vault::Vault;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::{self, Book, Chapter};
use ateliet_core::scriptorium::stats::ScSettings;
use ateliet_core::scriptorium::{count_scenes, count_words, strip_frontmatter};
use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub struct ChapterStats {
    pub info: Chapter,
    pub words: usize,
}

pub struct BookStats {
    pub words: usize,
    pub scenes: usize,
    pub chapters: usize,
    pub per: Vec<ChapterStats>,
    pub characters: usize,
    pub last_edited: Option<String>,
}

pub struct Sc {
    pub settings: ScSettings,
    words: HashMap<String, (i64, usize, usize)>,
    stats: HashMap<String, (u64, Arc<BookStats>)>,
    last_save: Instant,
}

/// What a view asks the app to do.
#[derive(Clone, Debug)]
pub enum Action {
    Open(String, bool),
    NewBook,
    NewChapter(String),
    NewCharacter(String),
    /// Open a Scriptorium view (`library`, …) for a book.
    View(String, Option<String>),
    SelectBook(String),
    SetFm(String, String, Option<Json>),
    Reorder(Vec<String>),
    RenameChapter(String, String),
    ChapterPart(String, String),
    Trash(String),
    Continue(String),
    Export(String),
    Pick(Box<picker::Picker>),
    /// A sheet wrote its note.
    Saved(String),
    Rename(String, String),
    AsMarkdown(String),
    CreateSheetFor { sheet: String, list: String, idx: usize, name: String },
    /// (target, source note, new tab)
    OpenLink(String, String, bool),
    Toast(String),
    Music(crate::music::Cmd),
}

impl Sc {
    pub fn load(config_dir: &std::path::Path) -> Sc {
        Sc { settings: ScSettings::load(config_dir), words: HashMap::new(), stats: HashMap::new(), last_save: Instant::now() }
    }

    pub fn author(&self) -> String {
        self.settings.default_author()
    }

    fn words_of(&mut self, root: &std::path::Path, path: &str, mtime: i64) -> (usize, usize) {
        if let Some((m, w, s)) = self.words.get(path) {
            if *m == mtime {
                return (*w, *s);
            }
        }
        let text = std::fs::read_to_string(root.join(path)).unwrap_or_default();
        let body = strip_frontmatter(&text);
        let r = (count_words(body), count_scenes(body));
        self.words.insert(path.to_string(), (mtime, r.0, r.1));
        r
    }

    pub fn stats(&mut self, ix: &Index, book: &Book) -> Arc<BookStats> {
        if let Some((g, s)) = self.stats.get(&book.path) {
            if *g == ix.generation {
                return s.clone();
            }
        }
        let chapters = book::chapters(ix, book);
        let (mut words, mut scenes) = (0, 0);
        let mut per = Vec::new();
        let mut last: Option<(i64, String)> = None;
        for c in chapters {
            let (w, s) = self.words_of(&ix.root, &c.path, c.mtime);
            if c.include {
                words += w;
                scenes += s;
            }
            if last.as_ref().is_none_or(|(m, _)| c.mtime > *m) {
                last = Some((c.mtime, c.path.clone()));
            }
            per.push(ChapterStats { info: c, words: w });
        }
        let s = Arc::new(BookStats {
            words,
            scenes,
            chapters: per.len(),
            per,
            characters: book::characters(ix, book).len(),
            last_edited: last.map(|(_, p)| p),
        });
        self.stats.insert(book.path.clone(), (ix.generation, s.clone()));
        s
    }

    /// The book of the open note, else the last one used, else the first.
    pub fn current_book(&mut self, ix: &Index, active: Option<&str>) -> Option<Book> {
        let author = self.author();
        if let Some(b) = active.and_then(|p| book::book_for(ix, p, &author)) {
            self.settings.set("ultimoLivro", Json::String(b.path.clone()));
            return Some(b);
        }
        let last = self.settings.str("ultimoLivro", "");
        book::book_by_path(ix, &last, &author).or_else(|| book::books(ix, &author).into_iter().next())
    }

    /// After the index changed: count the words of the open chapter's book
    /// for today's statistics; save the settings now and then.
    pub fn tick(&mut self, v: &Vault, active: Option<&str>) {
        if let Some(p) = active {
            let ix = v.index.read().unwrap();
            let author = self.author();
            if let Some(b) = book::book_for(&ix, p, &author) {
                let s = self.stats(&ix, &b);
                self.settings.record(&b.path, s.words as i64);
            }
        }
        if self.settings.is_dirty() && self.last_save.elapsed().as_secs() >= 2 {
            self.settings.save();
            self.last_save = Instant::now();
        }
    }
}

/// A Scriptorium screen open in a tab.
pub enum View {
    Library(library::Library),
    Cast(cast_ui::CastView),
    Sheet(Box<sheet_ui::SheetView>),
    Enredo(Box<enredo_ui::EnredoView>),
    Mundo(Box<mundo_ui::MundoView>),
    Mapa(Box<mapa_ui::MapaView>),
    Trilha(Box<trilha_ui::TrilhaView>),
    Diagramacao(Box<diagramacao_ui::DiagramacaoView>),
    Capa(Box<capa_ui::CapaView>),
    Pagina(Box<pagina_ui::PaginaView>),
}

impl View {
    pub fn title(&self) -> String {
        match self {
            View::Library(_) => "Biblioteca".into(),
            View::Cast(_) => "Elenco".into(),
            View::Sheet(s) => s.title(),
            View::Enredo(e) => e.title(),
            View::Mundo(m) => m.title(),
            View::Mapa(m) => m.title(),
            View::Trilha(t) => t.title(),
            View::Diagramacao(d) => d.title(),
            View::Capa(c) => c.title(),
            View::Pagina(c) => c.title(),
        }
    }

    pub fn from_path(path: &str) -> Option<View> {
        match path {
            "scriptorium:library" => Some(View::Library(library::Library::default())),
            _ => {
                if let Some(b) = path.strip_prefix("scriptorium:mundo:") {
                    return Some(View::Mundo(Box::new(mundo_ui::MundoView::new(b))));
                }
                if let Some(b) = path.strip_prefix("scriptorium:pagina:") {
                    return Some(View::Pagina(Box::new(pagina_ui::PaginaView::new(b))));
                }
                if let Some(b) = path.strip_prefix("scriptorium:capa:") {
                    return Some(View::Capa(Box::new(capa_ui::CapaView::new(b))));
                }
                if let Some(b) = path.strip_prefix("scriptorium:layout:") {
                    return Some(View::Diagramacao(Box::new(diagramacao_ui::DiagramacaoView::new(b))));
                }
                if let Some(b) = path.strip_prefix("scriptorium:trilha:") {
                    return Some(View::Trilha(Box::new(trilha_ui::TrilhaView::new(b))));
                }
                if let Some(b) = path.strip_prefix("scriptorium:enredo:") {
                    return Some(View::Enredo(Box::new(enredo_ui::EnredoView::new(b))));
                }
                path.strip_prefix("scriptorium:cast:").map(|b| View::Cast(cast_ui::CastView::new(b)))
            }
        }
    }
}

pub fn color(hex: &str) -> eframe::egui::Color32 {
    let c = ateliet_core::board::tile::parse_color(hex);
    eframe::egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// "Seg 03/10/2026" style date from milliseconds.
pub fn fmt_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y").to_string()).unwrap_or_default()
}
