//! The reader (Leitor): EPUB, PDF and comics read in a tab, immersive and
//! minimal — while reading only the page shows; the frame comes back when
//! the pointer goes to the top. Every passage can be highlighted, copied
//! or sent to the Journal; every region of a page cut out and sent there.
//! Beside the Journal, passages and cuts are dragged onto the page.
//!
//! - `worker` — the book's thread: layout, pictures, text.
//! - `chrome` — the frame that hides, the panels (contents, search,
//!   highlights, appearance) and the menus over a selection or a cut.
//! - `keep`   — what goes to the vault: highlights in the book's note,
//!   excerpts in `Trechos/`, cuts in `Anexos/Recortes/`.
//! - `shelf`  — the books of the vault, "continuar lendo" first.

mod chrome;
mod keep;
mod shelf;
mod worker;

use crate::theme::Palette;
use crate::vault::Vault;
use ateliet_core::capture::Snippet;
use ateliet_core::reader::marks::Mark;
use ateliet_core::reader::state::{self, BookState, State};
use ateliet_core::reader::{Format, Hit, Locator, Target, TextLayer, View};
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Sense, Stroke};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use worker::{Opened, PageInfo, Req, Resp, Worker};

pub const SHELF: &str = "leitor:estante";

/// Points in a millimetre (a point of type is a point on the screen).
const PT_MM: f32 = 72.0 / 25.4;

/// What the reader asks of the app.
pub enum Out {
    Toast(String),
    /// Files were added to the vault.
    Rescan,
    /// Open a note.
    Open(String),
    /// An excerpt was kept: for the Journal's open page (or the note beside).
    Snippet { note: String, snippet: Snippet },
    /// Read this book beside the Journal, or beside the notes.
    Beside { file: String, at: Option<Locator>, journal: bool },
    /// Close the reader beside.
    CloseSide,
}

/// What the app lends the reader for a frame.
pub struct Cx<'a> {
    pub vault: &'a Vault,
    pub palette: Palette,
    pub config_dir: Option<PathBuf>,
    /// A notebook is open in the Journal, beside the reader.
    pub journal_open: bool,
    /// The reader is beside the notes: what it keeps goes into the note in use.
    pub notes_beside: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Toc,
    Search,
    Marks,
    Look,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Read,
    Crop,
}

/// Text selected on a page: glyphs `a..=b` (in the order chosen).
#[derive(Clone, Debug)]
pub struct Sel {
    chap: usize,
    page: usize,
    a: usize,
    b: usize,
    done: bool,
    /// The highlight it is (clicked on one).
    mark: Option<String>,
}

#[derive(Clone, Debug)]
struct Cut {
    chap: usize,
    page: usize,
    a: (f32, f32),
    b: (f32, f32),
    done: bool,
    /// The region drawn at print resolution, once it came (PNG).
    png: Option<Vec<u8>>,
    asked: Option<u64>,
}

impl Cut {
    fn rect(&self) -> [f32; 4] {
        [self.a.0.min(self.b.0), self.a.1.min(self.b.1), self.a.0.max(self.b.0), self.a.1.max(self.b.1)]
    }
}

/// What an answer from the book's thread is for.
enum Pend {
    Cut,
    Picture { name: String },
    Note { at: Pos2 },
}

/// A page on the screen: which, where, and the part of it shown (fractions).
#[derive(Clone, Copy, Debug)]
struct PageView {
    chap: usize,
    page: usize,
    rect: Rect,
    uv: [f32; 4],
}

impl PageView {
    fn frac_of(&self, p: Pos2) -> (f32, f32) {
        let u = (p.x - self.rect.left()) / self.rect.width().max(1.0);
        let v = (p.y - self.rect.top()) / self.rect.height().max(1.0);
        (self.uv[0] + u * (self.uv[2] - self.uv[0]), self.uv[1] + v * (self.uv[3] - self.uv[1]))
    }

    fn screen_of(&self, f: (f32, f32)) -> Pos2 {
        let u = (f.0 - self.uv[0]) / (self.uv[2] - self.uv[0]).max(1e-6);
        let v = (f.1 - self.uv[1]) / (self.uv[3] - self.uv[1]).max(1e-6);
        pos2(self.rect.left() + u * self.rect.width(), self.rect.top() + v * self.rect.height())
    }

    fn frac_rect(&self, r: [f32; 4]) -> Rect {
        Rect::from_min_max(self.screen_of((r[0], r[1])), self.screen_of((r[2], r[3])))
    }
}

/// Reading preferences (by format, kept in `leitor.json`).
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// "claro", "sepia", "escuro", "tema".
    pub paper: String,
    pub font: String,
    pub size: f32,
    pub leading: f32,
    pub margin: f32,
    pub justify: bool,
    pub hyphenate: bool,
    /// "auto", "uma", "duas".
    pub spread: String,
    /// Fixed pages: "pagina" (whole) or "largura" (the width, scrolled).
    pub fit: String,
    /// Cut the white margins of PDF pages.
    pub trim: bool,
    /// Copy with the citation ("…" — Autor, *Título*, p.).
    pub cite: bool,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { paper: "claro".into(), font: String::new(), size: 14.0, leading: 1.45, margin: 16.0, justify: true, hyphenate: true, spread: "auto".into(), fit: "pagina".into(), trim: false, cite: false }
    }
}

impl Prefs {
    fn from_json(v: &serde_json::Value) -> Prefs {
        let d = Prefs::default();
        let s = |k: &str, d: &str| v[k].as_str().unwrap_or(d).to_string();
        let f = |k: &str, d: f32| v[k].as_f64().map(|x| x as f32).unwrap_or(d);
        let b = |k: &str, d: bool| v[k].as_bool().unwrap_or(d);
        Prefs { paper: s("papel", &d.paper), font: s("fonte", &d.font), size: f("tamanho", d.size).clamp(9.0, 30.0), leading: f("entrelinha", d.leading).clamp(1.1, 2.2), margin: f("margem", d.margin).clamp(4.0, 40.0), justify: b("justificar", d.justify), hyphenate: b("hifenizar", d.hyphenate), spread: s("paginas", &d.spread), fit: s("ajuste", &d.fit), trim: b("cortarMargens", d.trim), cite: b("copiarComCitacao", d.cite) }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "papel": self.paper, "fonte": self.font, "tamanho": self.size, "entrelinha": self.leading, "margem": self.margin, "justificar": self.justify, "hifenizar": self.hyphenate, "paginas": self.spread, "ajuste": self.fit, "cortarMargens": self.trim, "copiarComCitacao": self.cite })
    }

    /// Paper and ink.
    fn colors(&self, p: Palette) -> (Color32, Color32) {
        match self.paper.as_str() {
            "sepia" => (Color32::from_rgb(244, 236, 218), Color32::from_rgb(70, 52, 34)),
            "escuro" => (Color32::from_rgb(28, 27, 26), Color32::from_rgb(206, 200, 190)),
            "tema" => (p.bg, p.text),
            _ => (Color32::from_rgb(253, 251, 246), Color32::from_rgb(34, 30, 26)),
        }
    }
}

fn rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// A book open in the reader.
pub struct Book {
    pub file: String,
    fp: String,
    w: Worker,
    opened: Option<Opened>,
    error: Option<String>,
    chap: usize,
    /// `usize::MAX`: the chapter's last page, once its count is known.
    page: usize,
    counts: HashMap<(usize, String), usize>,
    asked_counts: HashSet<(usize, String)>,
    infos: HashMap<(usize, usize, String), PageInfo>,
    asked_infos: HashSet<(usize, usize, String)>,
    tex: HashMap<(usize, usize, String), (u32, egui::TextureHandle)>,
    asked_px: HashMap<(usize, usize, String), u32>,
    lru: VecDeque<(usize, usize, String)>,
    view: View,
    view_key: String,
    pending_view: Option<(View, Instant)>,
    /// A place to go to once the book is laid out.
    want: Option<Locator>,
    locating: Option<u64>,
    next_id: u64,
    pub panel: Option<Panel>,
    query: String,
    hits: Vec<Hit>,
    search_id: u64,
    searching: bool,
    sel: Option<Sel>,
    tool: Tool,
    cut: Option<Cut>,
    marks: Vec<Mark>,
    /// The book's note in the vault, once there is one.
    note: Option<String>,
    note_mtime: u64,
    note_checked: Option<Instant>,
    note_edit: Option<(String, String)>,
    popup: Option<(Pos2, String)>,
    pending: HashMap<u64, Pend>,
    /// The page moved inside the window (zoomed or scrolled), points.
    pan: egui::Vec2,
    zoom: f32,
    wheel: f32,
    swipe: f32,
    turned: Option<Instant>,
    seen: Instant,
    prefs: Prefs,
    state_dirty: bool,
    saved_at: Instant,
    /// Pages shown last frame.
    views: Vec<PageView>,
    /// Where the pointer pressed on a page: (view index, fraction, time).
    press: Option<(usize, (f32, f32), Instant)>,
    clicks: (Instant, u8),
}

pub struct Reader {
    pub path: String,
    pub book: Option<Book>,
    shelf: shelf::Shelf,
    /// Only the page on the screen: the app's ribbon, sidebars, tabs and
    /// status bar go.
    pub immersive: bool,
    /// Shown beside the Journal.
    pub side: bool,
    out: Vec<Out>,
    state: Option<State>,
    /// A book to open on the next frame (and where).
    asked: Option<(String, Option<Locator>)>,
}

impl Reader {
    /// The shelf, or a book (`path` a book of the vault).
    pub fn new(path: &str, side: bool) -> Reader {
        let mut r = Reader { path: path.to_string(), book: None, shelf: shelf::Shelf::default(), immersive: false, side, out: Vec::new(), state: None, asked: None };
        if path != SHELF {
            r.asked = Some((path.to_string(), None));
        }
        r
    }

    /// The book open and the place shown.
    pub fn here(&self) -> Option<(String, Option<Locator>)> {
        let b = self.book.as_ref()?;
        let mut at = b.info(b.chap, b.page).map(|i| i.loc.clone());
        if let Some(l) = at.as_mut() {
            l.file = b.file.clone();
            if b.reflows() {
                l.chap = Some(b.chap);
            }
        }
        Some((b.file.clone(), at))
    }

    /// Open a book (at a place) on the next frame.
    pub fn request(&mut self, file: &str, at: Option<Locator>) {
        self.path = file.to_string();
        self.asked = Some((file.to_string(), at));
    }

    pub fn title(&self) -> String {
        match &self.book {
            Some(b) => b.opened.as_ref().map(|o| o.meta.title.clone()).unwrap_or_else(|| crate::vault::basename(&b.file).to_string()),
            None => match &self.asked {
                Some((f, _)) => crate::vault::basename(f).to_string(),
                None => "Estante".into(),
            },
        }
    }

    fn state(&mut self, cx: &Cx) -> &mut State {
        if self.state.is_none() {
            self.state = Some(cx.config_dir.as_ref().map(|d| State::load(d)).unwrap_or_default());
        }
        self.state.as_mut().unwrap()
    }

    /// Opens a book of the vault (at a place, or where it was left).
    pub fn open(&mut self, cx: &Cx, ctx: &egui::Context, file: &str, at: Option<Locator>) {
        if self.book.as_ref().is_some_and(|b| b.file == file) {
            if let (Some(l), Some(b)) = (at, self.book.as_mut()) {
                b.go(l);
            }
            self.path = file.to_string();
            return;
        }
        self.save_state(cx);
        let abs = cx.vault.abs(file);
        let fp = ateliet_core::reader::fingerprint(&abs);
        let st = self.state(cx);
        let left = st.books.get(&fp).or_else(|| st.by_file(file).map(|x| x.1)).and_then(|b| b.at.clone());
        let fmt_key = ateliet_core::reader::format_of(file).map(|f| f.name().to_lowercase()).unwrap_or_default();
        let prefs = {
            let mut base = st.prefs.get(&fmt_key).cloned().unwrap_or(serde_json::Value::Null);
            if let Some(over) = st.books.get(&fp).map(|b| b.prefs.clone()).filter(|v| v.is_object()) {
                if let (Some(o), Some(b)) = (over.as_object(), base.as_object_mut()) {
                    for (k, v) in o {
                        b.insert(k.clone(), v.clone());
                    }
                } else {
                    base = over;
                }
            }
            Prefs::from_json(&base)
        };
        let w = worker::spawn(abs, ctx.clone());
        let mut want = at.or(left);
        if let Some(l) = want.as_mut() {
            l.file = file.to_string();
        }
        self.book = Some(Book {
            file: file.to_string(),
            fp,
            w,
            opened: None,
            error: None,
            chap: 0,
            page: 0,
            counts: HashMap::new(),
            asked_counts: HashSet::new(),
            infos: HashMap::new(),
            asked_infos: HashSet::new(),
            tex: HashMap::new(),
            asked_px: HashMap::new(),
            lru: VecDeque::new(),
            view: View::default(),
            view_key: String::new(),
            pending_view: None,
            want,
            locating: None,
            next_id: 1,
            panel: None,
            query: String::new(),
            hits: Vec::new(),
            search_id: 0,
            searching: false,
            sel: None,
            tool: Tool::Read,
            cut: None,
            marks: Vec::new(),
            note: None,
            note_mtime: 0,
            note_checked: None,
            note_edit: None,
            popup: None,
            pending: HashMap::new(),
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            wheel: 0.0,
            swipe: 0.0,
            turned: None,
            seen: Instant::now(),
            prefs,
            state_dirty: false,
            saved_at: Instant::now(),
            views: Vec::new(),
            press: None,
            clicks: (Instant::now(), 0),
        });
        self.path = file.to_string();
    }

    /// Back to the shelf.
    pub fn close_book(&mut self, cx: &Cx) {
        self.save_state(cx);
        self.book = None;
        self.path = SHELF.into();
        self.immersive = false;
    }

    /// Where each book was left, written to `leitor.json`.
    pub fn save_state(&mut self, cx: &Cx) {
        let Some(dir) = cx.config_dir.clone() else { return };
        let Some(b) = self.book.as_mut() else {
            if let Some(s) = &self.state {
                let _ = s.save(&dir);
            }
            return;
        };
        if !b.state_dirty {
            return;
        }
        b.state_dirty = false;
        b.saved_at = Instant::now();
        let (fp, entry, fmt, prefs) = (b.fp.clone(), b.book_state(), b.format().map(|f| f.name().to_lowercase()).unwrap_or_default(), b.prefs.to_json());
        let st = self.state(cx);
        st.books.insert(fp, entry);
        if !fmt.is_empty() {
            if !st.prefs.is_object() {
                st.prefs = serde_json::json!({});
            }
            st.prefs[fmt] = prefs;
        }
        let _ = st.save(&dir);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &Cx) -> Vec<Out> {
        let p = cx.palette;
        if let Some((file, at)) = self.asked.take() {
            let ctx = ui.ctx().clone();
            self.open(cx, &ctx, &file, at);
        }
        if self.book.is_none() {
            if let Some((file, at)) = self.shelf.ui(ui, cx, self.state.get_or_insert_with(|| cx.config_dir.as_ref().map(|d| State::load(d)).unwrap_or_default()), self.side) {
                let ctx = ui.ctx().clone();
                self.open(cx, &ctx, &file, at);
            }
            if self.side && self.shelf.close_side {
                self.shelf.close_side = false;
                self.out.push(Out::CloseSide);
            }
            return std::mem::take(&mut self.out);
        }
        let mut b = self.book.take().unwrap();
        b.poll(ui.ctx(), cx, &mut self.out);
        b.watch_note(cx);
        let full = ui.max_rect();
        let (paper, ink) = b.prefs.colors(p);
        ui.painter().rect_filled(full, 0.0, paper);
        if let Some(e) = b.error.clone() {
            ui.painter().text(full.center(), egui::Align2::CENTER_CENTER, format!("Não foi possível abrir o livro: {e}"), egui::FontId::proportional(14.0), ink);
            let close = self.chrome_bar(ui, &mut b, cx, full, true);
            if close {
                self.book = Some(b);
                self.close_book(cx);
                return std::mem::take(&mut self.out);
            }
            self.book = Some(b);
            return std::mem::take(&mut self.out);
        }
        if b.opened.is_none() {
            ui.painter().text(full.center(), egui::Align2::CENTER_CENTER, "Abrindo…", egui::FontId::proportional(13.0), ink.gamma_multiply(0.5));
            self.book = Some(b);
            return std::mem::take(&mut self.out);
        }
        let area = Rect::from_min_max(full.min + vec2(18.0, 30.0), full.max - vec2(18.0, 26.0));
        b.set_view(ui, area, p);
        b.views = b.layout(ui, area);
        let resp = ui.interact(full, ui.id().with(("leitor", &b.file)), Sense::click_and_drag());
        b.draw(ui, cx, area);
        b.input(ui, cx, &resp, area, &mut self.out, &mut self.immersive);
        let close = self.chrome_bar(ui, &mut b, cx, full, false);
        b.overlays(ui, cx, &mut self.out);
        if b.state_dirty && b.saved_at.elapsed() > Duration::from_secs(3) {
            self.book = Some(b);
            self.save_state(cx);
            b = self.book.take().unwrap();
        }
        self.book = Some(b);
        if close {
            if self.side {
                self.save_state(cx);
                self.out.push(Out::CloseSide);
            } else {
                self.close_book(cx);
            }
        }
        std::mem::take(&mut self.out)
    }
}

impl Book {
    pub fn format(&self) -> Option<Format> {
        self.opened.as_ref().map(|o| o.format)
    }

    fn reflows(&self) -> bool {
        self.opened.as_ref().is_some_and(|o| o.reflows)
    }

    fn chapters(&self) -> usize {
        self.opened.as_ref().map(|o| o.chapters).unwrap_or(1)
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// The pages of a chapter for the current view, once known.
    fn count(&self, chap: usize) -> Option<usize> {
        self.counts.get(&(chap, self.view_key.clone())).copied()
    }

    fn ask_count(&mut self, chap: usize) {
        let k = (chap, self.view_key.clone());
        if !self.counts.contains_key(&k) && self.asked_counts.insert(k) {
            let _ = self.w.tx.send(Req::Pages { chap, view: self.view.clone() });
        }
    }

    fn info(&self, chap: usize, page: usize) -> Option<&PageInfo> {
        self.infos.get(&(chap, page, self.view_key.clone()))
    }

    fn ask_info(&mut self, chap: usize, page: usize) {
        let k = (chap, page, self.view_key.clone());
        if !self.infos.contains_key(&k) && self.asked_infos.insert(k) {
            let trim = self.prefs.trim && self.format() == Some(Format::Pdf);
            let _ = self.w.tx.send(Req::Page { chap, page, view: self.view.clone(), trim });
        }
    }

    fn ask_tex(&mut self, chap: usize, page: usize, px: u32) {
        let k = (chap, page, self.view_key.clone());
        let have = self.tex.get(&k).map(|t| t.0).unwrap_or(0).max(self.asked_px.get(&k).copied().unwrap_or(0));
        if have >= px || (have > 0 && have as f32 >= px as f32 * 0.92) {
            return;
        }
        self.asked_px.insert(k, px);
        let gen = self.w.gen.load(Ordering::Relaxed);
        let _ = self.w.tx.send(Req::Render { chap, page, view: self.view.clone(), px, gen });
    }

    /// Go to a place (answered by the book's thread).
    fn go(&mut self, mut loc: Locator) -> u64 {
        loc.file = self.file.clone();
        if !self.reflows() {
            if let Some(pg) = loc.page {
                self.set_page(0, pg.saturating_sub(1));
                if loc.quote.is_none() {
                    return 0;
                }
            }
        }
        let id = self.id();
        self.locating = Some(id);
        let _ = self.w.tx.send(Req::Locate { loc, view: self.view.clone(), id });
        id
    }

    fn set_page(&mut self, chap: usize, page: usize) {
        if (chap, page) != (self.chap, self.page) {
            self.chap = chap;
            self.page = page;
            self.sel = None;
            self.cut = None;
            self.popup = None;
            self.pan = egui::Vec2::ZERO;
            self.state_dirty = true;
            self.w.gen.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Two pages side by side?
    fn spread(&self, area: Rect) -> bool {
        match self.prefs.spread.as_str() {
            "duas" => true,
            "uma" => false,
            _ => area.width() > area.height() * 1.25 && area.width() > 900.0 && (self.reflows() || self.prefs.fit != "largura"),
        }
    }

    fn step(&self) -> usize {
        if self.views.len() > 1 { 2 } else { 1 }
    }

    fn next(&mut self) {
        let step = self.step();
        let last = self.page_last();
        if self.page == usize::MAX {
            return;
        }
        match self.count(self.chap) {
            Some(n) if self.page + step < n => self.set_page(self.chap, self.page + step),
            Some(_) | None if self.chap + 1 < self.chapters() && self.reflows() => self.set_page(self.chap + 1, 0),
            _ => {
                let _ = last;
            }
        }
        self.turned = Some(Instant::now());
    }

    fn prev(&mut self) {
        let step = self.step();
        if self.page >= step {
            self.set_page(self.chap, self.page - step);
        } else if self.page > 0 {
            self.set_page(self.chap, 0);
        } else if self.chap > 0 && self.reflows() {
            self.set_page(self.chap - 1, usize::MAX);
        }
        self.turned = Some(Instant::now());
    }

    fn page_last(&self) -> bool {
        self.count(self.chap).is_some_and(|n| self.page + self.step() >= n) && self.chap + 1 >= self.chapters()
    }

    /// 0–1 through the book.
    fn progress(&self) -> f32 {
        let n = self.count(self.chap).unwrap_or(1).max(1) as f32;
        let pg = if self.page == usize::MAX { n - 1.0 } else { self.page as f32 };
        let c = self.chapters().max(1) as f32;
        ((self.chap as f32 + (pg + 1.0) / n) / c).clamp(0.0, 1.0)
    }

    fn book_state(&self) -> BookState {
        let meta = self.opened.as_ref().map(|o| o.meta.clone()).unwrap_or_default();
        let mut at = self.info(self.chap, self.page).map(|i| i.loc.clone());
        if let Some(l) = at.as_mut() {
            l.file = self.file.clone();
            if l.chap.is_none() && self.reflows() {
                l.chap = Some(self.chap);
            }
        }
        BookState { file: self.file.clone(), title: meta.title, author: meta.author, at, progress: self.progress(), seen: state::now(), prefs: serde_json::Value::Null }
    }

    /// Answers from the book's thread.
    fn poll(&mut self, ctx: &egui::Context, cx: &Cx, out: &mut Vec<Out>) {
        while let Ok(r) = self.w.rx.try_recv() {
            match r {
                Resp::Opened(o) => {
                    self.opened = Some(*o);
                    self.load_marks(cx);
                }
                Resp::Failed(e) => {
                    if self.opened.is_none() {
                        self.error = Some(e);
                    } else {
                        out.push(Out::Toast(e));
                    }
                }
                Resp::Pages { chap, key, n } => {
                    if key == self.view_key && chap == self.chap && self.page == usize::MAX {
                        let step = if self.views.len() > 1 { 2 } else { 1 };
                        let last = n.saturating_sub(1);
                        self.page = last - last % step;
                    }
                    self.counts.insert((chap, key), n);
                }
                Resp::Page { chap, page, key, info } => {
                    self.infos.insert((chap, page, key), *info);
                }
                Resp::Image { chap, page, key, px, img } => {
                    let k = (chap, page, key);
                    let c = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.px);
                    let t = ctx.load_texture(format!("leitor:{}:{chap}:{page}:{px}", self.file), c, egui::TextureOptions::LINEAR);
                    self.tex.insert(k.clone(), (px, t));
                    self.lru.retain(|x| *x != k);
                    self.lru.push_back(k);
                    while self.lru.len() > 18 {
                        if let Some(old) = self.lru.pop_front() {
                            self.tex.remove(&old);
                            self.asked_px.remove(&old);
                        }
                    }
                }
                Resp::Located { id, chap, page, key } => {
                    if self.locating == Some(id) && key == self.view_key {
                        self.locating = None;
                        self.want = None;
                        let step = if self.views.len() > 1 { 2 } else { 1 };
                        self.set_page(chap, page - page % step);
                    }
                }
                Resp::Search { id, hits } => {
                    if id == self.search_id {
                        self.hits = hits;
                        self.searching = false;
                    }
                }
                Resp::Crop { id, img } => {
                    if let Some(Pend::Cut) = self.pending.remove(&id) {
                        match img.and_then(|i| i.png()) {
                            Ok(png) => {
                                if let Some(c) = self.cut.as_mut().filter(|c| c.asked == Some(id)) {
                                    c.png = Some(png);
                                }
                            }
                            Err(e) => out.push(Out::Toast(format!("Não foi possível recortar: {e}"))),
                        }
                    }
                }
                Resp::Resource { id, bytes } => {
                    if let Some(Pend::Picture { name }) = self.pending.remove(&id) {
                        match bytes {
                            Some(b) => self.keep_picture(cx, &name, &b, out),
                            None => out.push(Out::Toast(format!("{name}: não foi possível ler a imagem"))),
                        }
                    }
                }
                Resp::Note { id, text } => {
                    if let Some(Pend::Note { at }) = self.pending.remove(&id) {
                        if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
                            self.popup = Some((at, t));
                        }
                    }
                }
            }
        }
    }

    /// The view (for an EPUB: the page's size from the window, and the
    /// type); a change waits until the window stops moving.
    fn set_view(&mut self, ui: &egui::Ui, area: Rect, p: Palette) {
        let (paper, ink) = self.prefs.colors(p);
        let spread = self.spread(area);
        let mut v = View { font: self.prefs.font.clone(), size: self.prefs.size, leading: self.prefs.leading, margin: self.prefs.margin, justify: self.prefs.justify, hyphenate: self.prefs.hyphenate, paper: rgb(paper), ink: rgb(ink), link: rgb(if p.dark || self.prefs.paper == "escuro" { Color32::from_rgb(160, 150, 210) } else { Color32::from_rgb(92, 78, 140) }), ..View::default() };
        if self.reflows() {
            let pw = if spread { (area.width() - 24.0) / 2.0 } else { area.width().min(area.height() * 0.95).max(area.width().min(760.0)) };
            v.w = (pw / PT_MM).round().max(60.0);
            v.h = (area.height() / PT_MM).round().max(80.0);
        } else {
            // fixed pages do not depend on the view
            v = View { paper: rgb(paper), ink: rgb(ink), ..View::default() };
        }
        let key = v.key();
        if key == self.view_key {
            self.pending_view = None;
            return;
        }
        let fresh = self.view_key.is_empty() || !self.tex.keys().any(|k| k.2 == self.view_key);
        let ready = match &self.pending_view {
            Some((pv, at)) if pv.key() == key => at.elapsed() > Duration::from_millis(220),
            _ => {
                self.pending_view = Some((v.clone(), Instant::now()));
                false
            }
        };
        if !(fresh || ready) {
            ui.ctx().request_repaint_after(Duration::from_millis(240));
            return;
        }
        // keep the place: the start of the page shown
        let here = self.info(self.chap, self.page).map(|i| i.loc.clone());
        self.view = v;
        self.view_key = key;
        self.pending_view = None;
        if self.reflows() {
            if self.want.is_none() {
                self.want = here.map(|mut l| {
                    l.chap = Some(self.chap);
                    l
                });
            }
            self.page = 0;
        }
        if let Some(w) = self.want.clone() {
            self.go(w);
        }
    }

    /// Where the pages go on the screen.
    fn layout(&mut self, ui: &egui::Ui, area: Rect) -> Vec<PageView> {
        let spread = self.spread(area);
        if self.page == usize::MAX {
            self.ask_count(self.chap);
            return Vec::new();
        }
        self.ask_count(self.chap);
        let rtl = self.opened.as_ref().is_some_and(|o| o.meta.rtl);
        let n = self.count(self.chap);
        let mut pages = vec![self.page];
        if spread && n.is_none_or(|n| self.page + 1 < n) {
            pages.push(self.page + 1);
        }
        if let Some(n) = n {
            if self.page >= n && n > 0 {
                let last = n - 1;
                self.page = if spread { last - last % 2 } else { last };
                pages = vec![self.page];
                if spread && self.page + 1 < n {
                    pages.push(self.page + 1);
                }
            }
        }
        let mut out = Vec::new();
        if self.reflows() {
            let (w, h) = (self.view.w * PT_MM, self.view.h * PT_MM);
            let total = if spread { w * 2.0 + 24.0 } else { w };
            let x0 = area.center().x - total / 2.0;
            let y0 = area.center().y - h / 2.0;
            for (i, pg) in pages.iter().enumerate() {
                let slot = if rtl && spread { 1 - i } else { i };
                let r = Rect::from_min_size(pos2(x0 + slot as f32 * (w + 24.0), y0), vec2(w, h));
                out.push(PageView { chap: self.chap, page: *pg, rect: r, uv: [0.0, 0.0, 1.0, 1.0] });
            }
            return out;
        }
        // fixed pages: the whole page, or the width (scrolled), zoomed
        let sizes: Vec<(f32, f32)> = pages.iter().map(|pg| self.info(self.chap, *pg).map(|i| i.size).or_else(|| self.infos.iter().find(|(k, _)| k.0 == self.chap).map(|(_, i)| i.size)).unwrap_or((210.0, 297.0))).collect();
        let uvs: Vec<[f32; 4]> = pages.iter().map(|pg| self.info(self.chap, *pg).and_then(|i| i.content).filter(|_| self.prefs.trim).unwrap_or([0.0, 0.0, 1.0, 1.0])).collect();
        let shown: Vec<(f32, f32)> = sizes.iter().zip(&uvs).map(|(s, uv)| (s.0 * (uv[2] - uv[0]), s.1 * (uv[3] - uv[1]))).collect();
        let gap = 0.0;
        let total_w: f32 = shown.iter().map(|s| s.0).sum::<f32>() + gap;
        let max_h = shown.iter().map(|s| s.1).fold(0.0, f32::max).max(1.0);
        let fit = if self.prefs.fit == "largura" && !spread { area.width() / total_w.max(1.0) } else { (area.width() / total_w.max(1.0)).min(area.height() / max_h) };
        let s = fit * self.zoom;
        let (tw, th) = (total_w * s, max_h * s);
        // the page larger than the window moves with the pan
        let max_pan = vec2(((tw - area.width()) / 2.0).max(0.0), ((th - area.height()) / 2.0).max(0.0));
        self.pan = vec2(self.pan.x.clamp(-max_pan.x, max_pan.x), self.pan.y.clamp(-max_pan.y, max_pan.y));
        let origin = pos2(area.center().x - tw / 2.0, area.center().y - th / 2.0) - self.pan;
        let order: Vec<usize> = if rtl { (0..pages.len()).rev().collect() } else { (0..pages.len()).collect() };
        let mut x = origin.x;
        for i in order {
            let (w, h) = (shown[i].0 * s, shown[i].1 * s);
            let r = Rect::from_min_size(pos2(x, origin.y + (th - h) / 2.0), vec2(w, h));
            out.push(PageView { chap: self.chap, page: pages[i], rect: r, uv: uvs[i] });
            x += w + gap;
        }
        let _ = ui;
        out
    }

    /// The pages, their highlights, the selection, the search hits.
    fn draw(&mut self, ui: &mut egui::Ui, cx: &Cx, area: Rect) {
        let p = cx.palette;
        let ppp = ui.ctx().pixels_per_point();
        let painter = ui.painter().with_clip_rect(ui.max_rect());
        let views = self.views.clone();
        let (paper, ink) = self.prefs.colors(p);
        // what comes next first (the thread answers the newest ask first)
        let mut ahead: Vec<(usize, usize)> = Vec::new();
        if let Some(last) = views.iter().map(|v| v.page).max() {
            for d in 1..=views.len().max(1) {
                ahead.push((self.chap, last + d));
            }
            if let Some(first) = views.iter().map(|v| v.page).min().filter(|f| *f > 0) {
                ahead.push((self.chap, first - 1));
            }
        }
        let n = self.count(self.chap);
        for (c, pg) in ahead {
            if n.is_some_and(|n| pg >= n) {
                continue;
            }
            self.ask_info(c, pg);
            if let Some(v) = views.first() {
                let px = ((v.rect.width() / (v.uv[2] - v.uv[0]).max(0.1)) * ppp).ceil() as u32;
                self.ask_tex(c, pg, round_px(px));
            }
        }
        for v in &views {
            self.ask_info(v.chap, v.page);
            let full_w = v.rect.width() / (v.uv[2] - v.uv[0]).max(0.05);
            let px = round_px((full_w * ppp).ceil() as u32).min(if self.reflows() { 4000 } else { 5000 });
            self.ask_tex(v.chap, v.page, px);
            let k = (v.chap, v.page, self.view_key.clone());
            if !self.reflows() {
                painter.rect_filled(v.rect, 0.0, Color32::WHITE);
                if !(p.dark || self.prefs.paper == "escuro") {
                    painter.rect_stroke(v.rect.expand(0.5), 0.0, Stroke::new(1.0_f32, ink.gamma_multiply(0.08)), egui::StrokeKind::Outside);
                }
            }
            match self.tex.get(&k) {
                Some((_, t)) => {
                    let uv = Rect::from_min_max(pos2(v.uv[0], v.uv[1]), pos2(v.uv[2], v.uv[3]));
                    painter.image(t.id(), v.rect, uv, Color32::WHITE);
                }
                None => {
                    if self.reflows() {
                        painter.rect_filled(v.rect, 0.0, paper);
                    }
                }
            }
        }
        // highlights, search hits and the selection, over the ink
        let q = self.query.trim().to_string();
        for v in &views {
            let Some(info) = self.info(v.chap, v.page) else { continue };
            let Some(t) = info.text.clone() else { continue };
            for m in &self.marks {
                if let Some((a, b)) = self.mark_range(m, v.chap, v.page, &t) {
                    let c = mark_color(&m.color);
                    for r in t.rects(a, b) {
                        painter.rect_filled(v.frac_rect(r).expand2(vec2(0.5, 0.0)), 1.5, c);
                    }
                }
            }
            if self.panel == Some(Panel::Search) && q.chars().count() >= 2 {
                for (a, b) in t.find(&q) {
                    for r in t.rects(a, b) {
                        painter.rect_stroke(v.frac_rect(r).expand(1.0), 2.0, Stroke::new(1.5_f32, Color32::from_rgb(230, 140, 40)), egui::StrokeKind::Outside);
                    }
                }
            }
            if let Some(s) = self.sel.as_ref().filter(|s| s.chap == v.chap && s.page == v.page && s.mark.is_none()) {
                let sc = if p.dark || self.prefs.paper == "escuro" { Color32::from_rgba_unmultiplied(110, 140, 220, 90) } else { Color32::from_rgba_unmultiplied(70, 110, 220, 60) };
                for r in t.rects(s.a, s.b) {
                    painter.rect_filled(v.frac_rect(r).expand2(vec2(0.5, 0.5)), 1.0, sc);
                }
            }
        }
        // the cut being drawn
        if let Some(c) = &self.cut {
            if let Some(v) = views.iter().find(|v| v.chap == c.chap && v.page == c.page) {
                let r = v.frac_rect(c.rect());
                let dim = Color32::from_black_alpha(70);
                let page = v.rect;
                for band in [Rect::from_min_max(page.min, pos2(page.max.x, r.min.y)), Rect::from_min_max(pos2(page.min.x, r.max.y), page.max), Rect::from_min_max(pos2(page.min.x, r.min.y), pos2(r.min.x, r.max.y)), Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(page.max.x, r.max.y))] {
                    if band.is_positive() {
                        painter.rect_filled(band, 0.0, dim);
                    }
                }
                painter.rect_stroke(r, 0.0, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Outside);
            }
        } else if self.tool == Tool::Crop {
            for v in &views {
                painter.rect_filled(v.rect, 0.0, Color32::from_black_alpha(22));
            }
        }
        let _ = area;
    }

    /// The glyphs of a highlight on a page.
    fn mark_range(&self, m: &Mark, chap: usize, page: usize, t: &TextLayer) -> Option<(usize, usize)> {
        if self.reflows() {
            if m.loc.chap != Some(chap) {
                return None;
            }
            let start = m.loc.offset?;
            let a = t.glyphs.iter().position(|g| g.off >= start && g.off <= m.end && !g.ch.trim().is_empty())?;
            let b = t.glyphs.iter().rposition(|g| g.off >= start && g.off <= m.end && !g.ch.trim().is_empty())?;
            Some((a, b))
        } else {
            if m.loc.page != Some(page + 1) {
                return None;
            }
            let a = m.loc.offset?.min(t.glyphs.len().saturating_sub(1));
            Some((a, m.end.min(t.glyphs.len().saturating_sub(1))))
        }
    }

    /// The page under a point, and the point in it.
    fn hit(&self, q: Pos2) -> Option<(usize, (f32, f32))> {
        self.views.iter().enumerate().find(|(_, v)| v.rect.contains(q)).map(|(i, v)| (i, v.frac_of(q)))
    }

    fn input(&mut self, ui: &mut egui::Ui, cx: &Cx, resp: &egui::Response, area: Rect, out: &mut Vec<Out>, immersive: &mut bool) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            self.seen = Instant::now();
        }
        let typing = ctx.wants_keyboard_input();
        // keys
        if !typing {
            let (next, prev, esc, home, end) = ctx.input(|i| {
                let shift = i.modifiers.shift;
                (
                    i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::PageDown) || (i.key_pressed(egui::Key::Space) && !shift) || i.key_pressed(egui::Key::ArrowDown) && self.fits(area),
                    i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::PageUp) || (i.key_pressed(egui::Key::Space) && shift) || i.key_pressed(egui::Key::ArrowUp) && self.fits(area),
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Home),
                    i.key_pressed(egui::Key::End),
                )
            });
            let rtl = self.opened.as_ref().is_some_and(|o| o.meta.rtl) && !self.reflows();
            let arrows_swap = rtl && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowRight));
            if next {
                if arrows_swap { self.prev() } else { self.next() }
            } else if prev {
                if arrows_swap { self.next() } else { self.prev() }
            }
            if home {
                self.set_page(0, 0);
            }
            if end {
                let last = self.chapters().saturating_sub(1);
                self.set_page(last, usize::MAX);
            }
            if esc {
                if self.popup.is_some() || self.sel.is_some() || self.cut.is_some() || self.panel.is_some() || self.note_edit.is_some() {
                    self.popup = None;
                    self.sel = None;
                    self.cut = None;
                    self.panel = None;
                    self.note_edit = None;
                } else if self.tool == Tool::Crop {
                    self.tool = Tool::Read;
                } else if *immersive {
                    *immersive = false;
                }
            }
            let cmd_enter = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
            if cmd_enter || ctx.input(|i| i.key_pressed(egui::Key::F) && !i.modifiers.any()) {
                *immersive = !*immersive;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::R) && !i.modifiers.any()) {
                self.tool = if self.tool == Tool::Crop { Tool::Read } else { Tool::Crop };
                self.cut = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::T) && !i.modifiers.any()) {
                self.panel = if self.panel == Some(Panel::Toc) { None } else { Some(Panel::Toc) };
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                self.panel = Some(Panel::Search);
            }
            let copy = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
            if copy {
                if let Some(text) = self.selection_text() {
                    let t = if self.prefs.cite { self.citation(&text) } else { text };
                    ctx.copy_text(t);
                }
            }
        }
        // the wheel: scrolls a page larger than the window, else turns
        if resp.hovered() || resp.contains_pointer() {
            let (scroll, zoom) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0 && !self.reflows() {
                self.zoom = (self.zoom * zoom).clamp(0.5, 6.0);
            } else if zoom != 1.0 {
                self.prefs.size = (self.prefs.size * zoom.powf(0.5)).clamp(9.0, 30.0);
                self.state_dirty = true;
            }
            if scroll.x.abs() > scroll.y.abs() {
                self.swipe += scroll.x;
                if self.turned.is_none_or(|t| t.elapsed() > Duration::from_millis(450)) && self.swipe.abs() > 60.0 {
                    let rtl = self.opened.as_ref().is_some_and(|o| o.meta.rtl) && !self.reflows();
                    if (self.swipe < 0.0) != rtl { self.next() } else { self.prev() }
                    self.swipe = 0.0;
                }
            } else if scroll.y != 0.0 {
                self.swipe = 0.0;
                let can_scroll = !self.fits(area);
                if can_scroll {
                    let before = self.pan.y;
                    self.pan.y -= scroll.y;
                    let max = self.views.iter().map(|v| v.rect.height()).fold(0.0, f32::max);
                    let lim = ((max - area.height()) / 2.0).max(0.0);
                    let clamped = self.pan.y.clamp(-lim, lim);
                    if (clamped - self.pan.y).abs() > 0.5 && (before - clamped).abs() < 0.5 {
                        self.wheel += -scroll.y;
                    } else {
                        self.wheel = 0.0;
                    }
                    self.pan.y = clamped;
                    if self.wheel.abs() > 160.0 {
                        let down = self.wheel > 0.0;
                        self.wheel = 0.0;
                        if down {
                            self.next();
                            self.pan.y = -lim;
                        } else {
                            self.prev();
                            self.pan.y = lim;
                        }
                    }
                } else {
                    self.wheel += -scroll.y;
                    if self.turned.is_none_or(|t| t.elapsed() > Duration::from_millis(350)) && self.wheel.abs() > 90.0 {
                        if self.wheel > 0.0 { self.next() } else { self.prev() }
                        self.wheel = 0.0;
                    }
                }
            }
        }
        if self.page == usize::MAX {
            return;
        }
        let shift = ctx.input(|i| i.modifiers.shift);
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        // a drag starts where the button went down, not where it was noticed
        let origin = ctx.input(|i| i.pointer.press_origin()).or(pointer);
        // the cut
        if self.tool == Tool::Crop {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            if resp.drag_started() {
                if let Some((vi, f)) = origin.and_then(|q| self.hit(q)) {
                    let v = self.views[vi];
                    self.cut = Some(Cut { chap: v.chap, page: v.page, a: f, b: f, done: false, png: None, asked: None });
                }
            }
            if resp.dragged() {
                if let (Some(c), Some(q)) = (self.cut.as_mut().filter(|c| !c.done), pointer) {
                    if let Some(v) = self.views.iter().find(|v| v.chap == c.chap && v.page == c.page) {
                        let f = v.frac_of(q);
                        c.b = (f.0.clamp(0.0, 1.0), f.1.clamp(0.0, 1.0));
                        if shift {
                            // a square (on the page)
                            let (w, h) = (v.rect.width() / (v.uv[2] - v.uv[0]), v.rect.height() / (v.uv[3] - v.uv[1]));
                            let dx = (c.b.0 - c.a.0) * w;
                            let dy = (c.b.1 - c.a.1) * h;
                            let s = dx.abs().max(dy.abs());
                            c.b = (c.a.0 + s.copysign(dx) / w, c.a.1 + s.copysign(dy) / h);
                        }
                    }
                }
            }
            if resp.drag_stopped() {
                let mut ask = None;
                if let Some(c) = self.cut.as_mut() {
                    let r = c.rect();
                    if (r[2] - r[0]) < 0.01 || (r[3] - r[1]) < 0.01 {
                        self.cut = None;
                    } else {
                        c.done = true;
                        ask = Some((c.chap, c.page, r));
                    }
                }
                if let Some((chap, page, rect)) = ask {
                    let id = self.id();
                    self.pending.insert(id, Pend::Cut);
                    if let Some(c) = self.cut.as_mut() {
                        c.asked = Some(id);
                    }
                    let _ = self.w.tx.send(Req::Crop { chap, page, view: self.view.clone(), rect, id });
                }
            }
            return;
        }
        // the text: press, drag, release
        let over_text = pointer.and_then(|q| self.hit(q)).and_then(|(vi, f)| {
            let v = self.views[vi];
            let t = self.info(v.chap, v.page)?.text.clone()?;
            t.hit(f.0, f.1, false).map(|_| ())
        });
        let over_link = pointer.and_then(|q| self.link_at(q));
        if over_link.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if over_text.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
        if resp.drag_started() {
            if let Some((vi, f)) = origin.and_then(|q| self.hit(q)) {
                let v = self.views[vi];
                if let Some(t) = self.info(v.chap, v.page).and_then(|i| i.text.clone()) {
                    if let Some(g) = t.hit(f.0, f.1, true) {
                        let a = match self.sel.as_ref().filter(|s| shift && s.chap == v.chap && s.page == v.page) {
                            Some(s) => s.a,
                            None => g,
                        };
                        self.sel = Some(Sel { chap: v.chap, page: v.page, a, b: g, done: false, mark: None });
                        self.popup = None;
                    }
                } else if !self.fits(area) {
                    self.press = Some((vi, f, Instant::now()));
                }
            }
        }
        if resp.dragged() {
            if let (Some(s), Some(q)) = (self.sel.clone().filter(|s| !s.done), pointer) {
                if let Some(v) = self.views.iter().find(|v| v.chap == s.chap && v.page == s.page) {
                    let f = v.frac_of(q);
                    if let Some(t) = self.info(s.chap, s.page).and_then(|i| i.text.clone()) {
                        if let Some(g) = t.hit(f.0, f.1, true) {
                            if let Some(s) = self.sel.as_mut() {
                                s.b = g;
                            }
                        }
                    }
                }
            } else if self.press.is_some() {
                // a page larger than the window, dragged
                self.pan -= resp.drag_delta();
            }
        }
        if resp.drag_stopped() {
            self.press = None;
            if let Some(s) = self.sel.as_mut() {
                s.done = true;
                // the words, whole
                if let Some(t) = self.infos.get(&(s.chap, s.page, self.view_key.clone())).and_then(|i| i.text.clone()) {
                    let (a, b) = (s.a.min(s.b), s.a.max(s.b));
                    let (wa, _) = t.word(a);
                    let (_, wb) = t.word(b);
                    s.a = wa;
                    s.b = wb;
                }
            }
        }
        if resp.clicked() {
            let q = pointer.unwrap_or_default();
            // clicks in a row: a word, then the paragraph
            let now = Instant::now();
            let n = if now.duration_since(self.clicks.0) < Duration::from_millis(420) { self.clicks.1 + 1 } else { 1 };
            self.clicks = (now, n);
            if let Some(l) = self.link_at(q) {
                self.follow(l, q);
                return;
            }
            let on = self.hit(q).and_then(|(vi, f)| {
                let v = self.views[vi];
                let t = self.info(v.chap, v.page)?.text.clone()?;
                Some((v, t.clone(), t.hit(f.0, f.1, false)?))
            });
            match (on, n) {
                (Some((v, t, g)), 2) => {
                    let (a, b) = t.word(g);
                    self.sel = Some(Sel { chap: v.chap, page: v.page, a, b, done: true, mark: None });
                }
                (Some((v, t, g)), k) if k >= 3 => {
                    let (a, b) = t.para(g);
                    self.sel = Some(Sel { chap: v.chap, page: v.page, a, b, done: true, mark: None });
                }
                (Some((v, t, g)), _) => {
                    // a click on a highlight selects it
                    let hit_mark = self.marks.iter().find(|m| self.mark_range(m, v.chap, v.page, &t).is_some_and(|(a, b)| g >= a && g <= b)).map(|m| m.id.clone());
                    match hit_mark {
                        Some(id) => {
                            let (a, b) = self.marks.iter().find(|m| m.id == id).and_then(|m| self.mark_range(m, v.chap, v.page, &t)).unwrap_or((g, g));
                            self.sel = Some(Sel { chap: v.chap, page: v.page, a, b, done: true, mark: Some(id) });
                        }
                        None => {
                            if self.sel.is_some() || self.popup.is_some() {
                                self.sel = None;
                                self.popup = None;
                            } else {
                                self.turn_by_margin(q, area);
                            }
                        }
                    }
                }
                (None, _) => {
                    if self.sel.is_some() || self.popup.is_some() || self.cut.is_some() {
                        self.sel = None;
                        self.popup = None;
                        self.cut = None;
                    } else {
                        self.turn_by_margin(q, area);
                    }
                }
            }
        }
        // a picture of the book: the right button sends it to the Journal
        if resp.secondary_clicked() {
            if let Some(q) = pointer {
                if let Some((vi, f)) = self.hit(q) {
                    let v = self.views[vi];
                    let name = self.info(v.chap, v.page).and_then(|i| i.images.iter().find(|(r, _)| f.0 >= r[0] && f.0 <= r[2] && f.1 >= r[1] && f.1 <= r[3]).map(|x| x.1.clone()));
                    if let Some(name) = name {
                        ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new("leitor-img-menu"), (q, name)));
                    }
                }
            }
        }
        let _ = (cx, out);
    }

    /// Does the page fit in the window (nothing to scroll)?
    fn fits(&self, area: Rect) -> bool {
        self.views.iter().all(|v| v.rect.height() <= area.height() + 1.0 && v.rect.width() <= area.width() + 1.0)
    }

    fn turn_by_margin(&mut self, q: Pos2, area: Rect) {
        let rtl = self.opened.as_ref().is_some_and(|o| o.meta.rtl) && !self.reflows();
        let x = (q.x - area.left()) / area.width().max(1.0);
        if x > 0.78 {
            if rtl { self.prev() } else { self.next() }
        } else if x < 0.22 {
            if rtl { self.next() } else { self.prev() }
        }
    }

    fn link_at(&self, q: Pos2) -> Option<ateliet_core::reader::Link> {
        let (vi, f) = self.hit(q)?;
        let v = self.views[vi];
        self.info(v.chap, v.page)?.links.iter().find(|l| f.0 >= l.r[0] - 0.004 && f.0 <= l.r[2] + 0.004 && f.1 >= l.r[1] - 0.004 && f.1 <= l.r[3] + 0.004).cloned()
    }

    fn follow(&mut self, l: ateliet_core::reader::Link, at: Pos2) {
        match l.target {
            Target::Url(u) => {
                let _ = open::that(u);
            }
            Target::At { chap, offset, page } => {
                if l.note && self.reflows() {
                    let id = self.id();
                    self.pending.insert(id, Pend::Note { at });
                    let _ = self.w.tx.send(Req::Note { chap, off: offset.unwrap_or(0), id });
                    return;
                }
                if let Some(pg) = page {
                    self.set_page(chap, pg);
                } else {
                    self.go(Locator { chap: Some(chap), offset, ..Default::default() });
                }
            }
        }
    }

    /// The selected text.
    fn selection_text(&self) -> Option<String> {
        let s = self.sel.as_ref()?;
        let t = self.infos.get(&(s.chap, s.page, self.view_key.clone()))?.text.clone()?;
        let text = t.text(s.a, s.b);
        (!text.is_empty()).then_some(text)
    }

    /// "…" — Autor, *Título*, cap./p.
    fn citation(&self, text: &str) -> String {
        let meta = self.opened.as_ref().map(|o| o.meta.clone()).unwrap_or_default();
        let place = self.place_label(self.chap, self.page);
        let mut s = format!("“{}” — ", text.trim());
        if !meta.author.is_empty() {
            s.push_str(&meta.author);
            s.push_str(", ");
        }
        s.push_str(&format!("*{}*", meta.title));
        if !place.is_empty() {
            s.push_str(", ");
            s.push_str(&place);
        }
        s
    }

    /// "cap. 3 — Título" in an EPUB; "p. 58" in a PDF.
    fn place_label(&self, chap: usize, page: usize) -> String {
        if self.reflows() {
            let title = self.opened.as_ref().and_then(|o| o.toc.iter().filter(|e| e.chap == chap).min_by_key(|e| e.level).map(|e| e.title.clone())).unwrap_or_default();
            if title.is_empty() { format!("cap. {}", chap + 1) } else { title }
        } else {
            format!("p. {}", page + 1)
        }
    }
}

fn round_px(px: u32) -> u32 {
    px.div_ceil(64) * 64
}

pub fn mark_color(name: &str) -> Color32 {
    match name {
        "azul" => Color32::from_rgba_unmultiplied(110, 170, 255, 80),
        "verde" => Color32::from_rgba_unmultiplied(120, 210, 120, 80),
        "rosa" => Color32::from_rgba_unmultiplied(250, 130, 170, 80),
        _ => Color32::from_rgba_unmultiplied(255, 215, 60, 95),
    }
}

/// The swatch of a highlight colour (solid).
pub fn mark_swatch(name: &str) -> Color32 {
    match name {
        "azul" => Color32::from_rgb(110, 170, 255),
        "verde" => Color32::from_rgb(120, 200, 120),
        "rosa" => Color32::from_rgb(245, 130, 170),
        _ => Color32::from_rgb(250, 205, 50),
    }
}
