//! The Journal: notebooks of fixed pages over the core's model and format
//! (`ateliet_core::journal`). The shelf of notebooks, a notebook open at a
//! spread, the pages turned one by one, text on the ruling that goes on to
//! the next page, pictures cut and pasted over it.
//!
//! - `draw`   — paper, ruling, spine and the items.
//! - `input`  — pointer and keyboard on the spread.
//! - `typeset` — text as it shows and as it is set in rows on the ruling.
//! - `typing` — the text being typed: keys, cursor, selection.
//! - `chrome` — the toolbar that hides, the balloon by the selection, and
//!   the material panels (pictures, notes, excerpts).
//! - `shelf`  — the shelf, the new notebook, the page thumbnails.
//! - `boards` — storyboards brought in: the panel, a frame, all of them.

mod boards;
mod chrome;
mod draw;
mod input;
mod shelf;
mod typeset;
mod typing;

use crate::host::Caches;
use crate::vault::Vault;
use ateliet_core::capture::{self, Snippet};
use ateliet_core::journal::wrap::{self, PageSet};
use ateliet_core::journal::{self, format, Item, Kind, Layout, Notebook, Page, PageRef};
use eframe::egui;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// Something dragged onto a page: a picture (its library key), a note, an
/// excerpt's note.
#[derive(Clone, Debug)]
pub enum Payload {
    Picture(String),
    Note(String),
    Snippet(String),
    /// A storyboard, whole (it goes on the page as an animation).
    Board(String),
}

/// What the Journal asks the app to do.
pub enum Out {
    Toast(String),
    /// Files were added to the vault.
    Rescan,
    /// Open a note in the writing mode.
    Open(String),
    /// Open a book in the reader beside the notebook (at a place); with
    /// no book, the reader's shelf.
    Read(Option<(String, Option<ateliet_core::reader::Locator>)>),
    /// Pages reordered: rename the files (their links follow), then set
    /// each page's number.
    Reorder { renames: Vec<(String, String)>, numbers: Vec<(String, usize)> },
}

/// What the app lends the Journal for a frame.
pub struct Cx<'a> {
    pub vault: &'a Vault,
    pub caches: &'a mut Caches,
    pub palette: crate::theme::Palette,
    pub images: &'a mut crate::images_ui::ImagesState,
    /// The vault's config folder (`.obsidian`).
    pub config_dir: Option<PathBuf>,
    /// Where pictures from outside the vault are copied.
    pub attach: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Write,
    Pen,
    Line,
    Tape,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Pictures,
    Notes,
    Snippets,
    Stickers,
    Storyboards,
}

/// A cover's texture: being made in the background, or made.
pub enum CoverTex {
    Making(std::sync::mpsc::Receiver<egui::ColorImage>),
    Ready(egui::TextureHandle),
}

/// A storyboard's frames, drawn small in the background for the panel.
pub enum BoardThumbs {
    Loading(std::sync::mpsc::Receiver<Result<Vec<(journal::storyboard::Frame, egui::ColorImage)>, String>>),
    Ready(Vec<(journal::storyboard::Frame, egui::TextureHandle)>),
    Failed(String),
}

/// A page note as it is here: the page, the page as last read from disk,
/// and whether it waits to be saved.
pub struct Loaded {
    pub path: String,
    abs: PathBuf,
    pub page: Page,
    base: Page,
    dirty: bool,
    changed_at: Option<Instant>,
    mtime: Option<SystemTime>,
}

/// The pages a change can touch, as they were: what undo brings back.
#[derive(Clone)]
struct Snapshot {
    pages: Vec<(String, Page)>,
}

/// A pointer gesture in progress (page numbers are 1-based).
#[derive(Clone, Debug)]
enum Drag {
    Move { page: usize, start: (f64, f64), orig: Vec<(String, f64, f64)>, moved: bool },
    Resize { page: usize, id: String, handle: (i8, i8), orig: Box<Item> },
    Rotate { page: usize, id: String, orig: f64, a0: f64 },
    Ink { page: usize, id: String, last: (f64, f64), at: Instant },
    Shape { page: usize, id: String, start: (f64, f64) },
    Marquee { page: usize, start: (f64, f64), now: (f64, f64) },
    /// Letters of the text being typed, swept over: the selection.
    Select,
    Crop { k: String, r0: ateliet_core::moodboard::Rect, start: (f64, f64) },
    Pan,
}

/// The text being typed. It goes into its box as it is typed (the box
/// keeps it without the line breaks at its end, which only the typing has).
pub struct Editing {
    pub page: usize,
    pub id: String,
    pub buf: String,
    /// Begun here: gone if nothing is typed in it.
    pub new: bool,
    /// The cursor, and the other end of the selection (chars of `buf`).
    pub cursor: usize,
    pub anchor: usize,
    /// What the input method is composing (chars of `buf`), not typed yet.
    pub ime: Option<(usize, usize)>,
    /// Where up and down keep to (screen x) while they are pressed.
    pub goal: Option<f32>,
    /// When the last change was typed (a pause starts a new undo step).
    pub typed: Option<Instant>,
}

impl Editing {
    pub fn new(page: usize, id: &str, buf: String, new: bool, cursor: Option<usize>) -> Editing {
        let c = cursor.unwrap_or(usize::MAX).min(buf.chars().count());
        Editing { page, id: id.to_string(), buf, new, cursor: c, anchor: c, ime: None, goal: None, typed: None }
    }

    /// The selection, first to last (empty: the cursor).
    pub fn range(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }
}

/// A page as it is on the screen this frame: its texts set, and as they show.
pub struct Shown {
    pub path: String,
    pub set: PageSet,
    pub styled: HashMap<String, typeset::Styled>,
}

/// What the spread shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Spread,
    Thumbs,
    /// The cover, alone: edited like a page (page 0, kept in the index note).
    Cover,
}

pub struct Book {
    pub nb: Notebook,
    root: PathBuf,
    loaded: HashMap<String, Loaded>,
    /// Spread 0 is the first page alone, on the right; spread `s` shows
    /// pages `2s` and `2s + 1`.
    pub spread: usize,
    pub look: Look,
    /// 1 shows the whole spread; more is a closer look, moved by `pan`.
    pub zoom: f32,
    pub pan: egui::Vec2,
    pub sel_page: usize,
    pub sel: Vec<String>,
    drag: Option<Drag>,
    pub editing: Option<Editing>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    guides: Vec<(usize, ateliet_core::canvas::Guide)>,
    crop: Option<(usize, String, ateliet_core::moodboard::CropStage)>,
    marker_written: usize,
    /// Bodies changed here since each page was read (for merging).
    edited: HashMap<String, HashSet<String>>,
    turn_cooldown: Option<Instant>,
    goto: Option<String>,
    backup_dir: Option<PathBuf>,
    /// The cover as a page: the index note.
    cover_ref: PageRef,
    /// Pages to set again even when they are not on the screen (a text went
    /// on to them, they were read from disk).
    pub unsettled: BTreeSet<usize>,
    /// Texts whose next box may have room to give back (they got shorter).
    pub repull: HashSet<String>,
    /// The typing went on to another page: it is shown.
    pub follow: Option<usize>,
    /// The ink item strokes are being added to, and when the last ended.
    last_ink: Option<(usize, String, Instant)>,
    /// Horizontal scrolling gathered toward a page turn.
    swipe: f32,
    /// A page turn being shown: when, and which way (1 forward).
    turned: Option<(Instant, f32)>,
    /// Where the cursor of the text being typed is on screen.
    caret: Option<egui::Pos2>,
    /// The view was brought close by the immersive mode (and goes back).
    auto_zoom: bool,
}

pub struct Journal {
    /// Notebooks of the vault (for the shelf), found in the background.
    pub shelf: Option<Vec<Notebook>>,
    shelf_rx: Option<std::sync::mpsc::Receiver<Vec<Notebook>>>,
    shelf_gen: u64,
    pub book: Option<Book>,
    pub new_dlg: Option<shelf::NewNotebook>,
    pub tool: Tool,
    pub panel: Option<Panel>,
    pub query: String,
    pub color: egui::Color32,
    pub out: Vec<Out>,
    /// When the pointer last moved: the toolbar shows for a while after.
    seen: Instant,
    inbox_at: Instant,
    /// An excerpt that just arrived, offered for the open page.
    pub arrived: Option<(String, Snippet)>,
    /// Pictures with a filter, made once: (path, filter) → texture.
    filtered: HashMap<(String, String), Option<(egui::TextureHandle, egui::Vec2)>>,
    /// Excerpts' notes as read: path → (modified, excerpt).
    snippets: HashMap<String, (u64, Option<Snippet>)>,
    /// Live notes shown on pages: path → (modified, text).
    notes: HashMap<String, (u64, String)>,
    /// The toolbar asked for the shelf.
    pub close_requested: bool,
    /// The reader is open beside (set by the app each frame).
    pub reading_beside: bool,
    /// Only the notebook on the screen: the app's ribbon, the panels and
    /// the bar go, the view comes close to what is being written.
    pub immersive: bool,
    /// Pictures sent from image mode, for the open page.
    pub pending_pictures: Vec<String>,
    /// Excerpts from the reader beside, for the open page: (note, excerpt).
    pub pending_snippets: Vec<(String, Snippet)>,
    /// Storyboards opened in the panel: path → their frames.
    pub boards: HashMap<String, (u64, BoardThumbs)>,
    /// The window cutting a storyboard, and the animations playing.
    pub board_dlg: Option<boards::BoardDialog>,
    anims: HashMap<String, boards::Anim>,
    /// Cover textures made: (texture, colour) → picture.
    cover_tex: HashMap<String, CoverTex>,
    /// Covers of the shelf's notebooks: index note → (modified, cover page).
    covers: HashMap<String, (u64, Page)>,
    /// Each page's last setting (by its note): where its texts' rows are.
    sets: HashMap<String, PageSet>,
    /// The pages on the screen this frame, set: what is drawn, hit and typed
    /// in is this, and only this (a text is where its letters are).
    pub shown: HashMap<usize, Shown>,
    /// Where the cursor was last frame (it stays lit after it moves).
    caret_key: Option<(usize, String, usize, usize)>,
    /// A notebook to be deleted, waiting for a yes.
    pub delete_dlg: Option<Notebook>,
}

impl Default for Journal {
    fn default() -> Journal {
        Journal {
            shelf: None,
            shelf_rx: None,
            shelf_gen: u64::MAX,
            book: None,
            new_dlg: None,
            tool: Tool::Select,
            panel: None,
            query: String::new(),
            color: egui::Color32::from_rgb(0x2b, 0x2b, 0x30),
            out: Vec::new(),
            seen: Instant::now(),
            inbox_at: Instant::now(),
            arrived: None,
            filtered: HashMap::new(),
            snippets: HashMap::new(),
            notes: HashMap::new(),
            close_requested: false,
            reading_beside: false,
            immersive: false,
            pending_pictures: Vec::new(),
            pending_snippets: Vec::new(),
            boards: HashMap::new(),
            board_dlg: None,
            anims: HashMap::new(),
            cover_tex: HashMap::new(),
            sets: HashMap::new(),
            shown: HashMap::new(),
            caret_key: None,
            delete_dlg: None,
            covers: HashMap::new(),
        }
    }
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

impl Loaded {
    fn read(root: &std::path::Path, path: &str) -> Loaded {
        let abs = root.join(path);
        let text = std::fs::read_to_string(&abs).unwrap_or_default();
        let page = Page::parse(&text);
        let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        Loaded { path: path.to_string(), abs, base: page.clone(), page, dirty: false, changed_at: None, mtime }
    }
}

/// What was written on the page from the notes mode, outside the Journal's
/// sections (at the top of the note, or in a section with no item), goes
/// on the page as text: the first free lines after what is there. A page
/// still marked the old way is written again the new way.
fn adopt_notes_text(l: &mut Loaded, lay: &Layout) {
    let mut changed = false;
    if let Ok(text) = std::fs::read_to_string(&l.abs) {
        changed |= format::is_legacy(&text);
    }
    let mut pieces: Vec<(Option<String>, String)> = Vec::new();
    // the top of the note, after the frontmatter
    let fm_end = ateliet_core::syntax::frontmatter(&l.page.head).map(|f| f.block.end).unwrap_or(0);
    let rest = l.page.head[fm_end..].to_string();
    if !rest.trim().is_empty() {
        pieces.push((None, rest.trim().to_string()));
        l.page.head = format!("{}\n\n", l.page.head[..fm_end].trim_end());
    }
    for (id, body) in std::mem::take(&mut l.page.loose) {
        if body.trim().is_empty() {
            continue;
        }
        pieces.push((Some(id), body.trim_end().to_string()));
    }
    let (c0, c1) = lay.main_column();
    let s = lay.paper.line();
    for (id, text) in pieces {
        let low = l.page.items.iter().filter(|i| i.x < c1 && i.x + i.w > c0).map(|i| i.bbox().y + i.bbox().h).fold(0.0_f64, f64::max);
        let top = lay.text_top(low.max(lay.first_baseline() - s) + 0.01, 1.0);
        let rows = text.lines().count().max(1) as f64;
        let mut it = Item::new(Kind::Text, c0, top, c1 - c0, rows * s);
        if let Some(id) = id {
            it.id = id;
        }
        it.set_content(&text);
        l.page.items.push(it);
        changed = true;
    }
    if changed {
        l.dirty = true;
        l.changed_at = Some(Instant::now());
    }
}

impl Book {
    pub fn open(root: PathBuf, mut nb: Notebook, backup_dir: Option<PathBuf>) -> Book {
        nb.scan(&root);
        let marker = nb.marker.clamp(1, nb.pages.len().max(1));
        let mut b = Book {
            nb,
            root,
            loaded: HashMap::new(),
            spread: marker / 2,
            look: Look::Spread,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            sel_page: 0,
            sel: Vec::new(),
            drag: None,
            editing: None,
            undo: Vec::new(),
            redo: Vec::new(),
            guides: Vec::new(),
            crop: None,
            marker_written: marker,
            edited: HashMap::new(),
            turn_cooldown: None,
            goto: None,
            backup_dir,
            cover_ref: PageRef { path: String::new(), number: 0, title: String::new(), date: None },
            unsettled: BTreeSet::new(),
            repull: HashSet::new(),
            follow: None,
            last_ink: None,
            swipe: 0.0,
            turned: None,
            caret: None,
            auto_zoom: false,
        };
        b.cover_ref.path = b.nb.path.clone();
        if b.nb.pages.is_empty() {
            b.append_page("");
        }
        b
    }

    pub fn count(&self) -> usize {
        self.nb.pages.len()
    }

    /// The pages of the spread shown: left and right (numbers, 1-based).
    pub fn spread_pages(&self) -> (Option<usize>, Option<usize>) {
        if self.look == Look::Cover {
            return (None, Some(0));
        }
        let l = 2 * self.spread;
        let r = l + 1;
        ((l >= 1 && l <= self.count()).then_some(l), (r <= self.count()).then_some(r))
    }

    pub fn spreads(&self) -> usize {
        self.count() / 2 + 1
    }

    pub fn go_to(&mut self, page: usize) {
        let to = (page.clamp(1, self.count().max(1)) / 2).min(self.spreads() - 1);
        // the typing goes on when its page is still shown (it went on there)
        if self.look == Look::Cover || self.editing.as_ref().is_some_and(|e| e.page / 2 != to) {
            self.commit_edit();
        }
        if self.look == Look::Cover {
            self.look = Look::Spread;
        }
        let before = self.spread;
        self.spread = to;
        if self.spread != before {
            self.turned = Some((Instant::now(), if self.spread > before { 1.0 } else { -1.0 }));
        }
        if self.editing.is_none() {
            self.sel.clear();
        }
        self.crop = None;
        // the immersive mode's close view follows the writing to the new page
        if !self.auto_zoom {
            self.zoom = 1.0;
            self.pan = egui::Vec2::ZERO;
        }
    }

    pub fn turn(&mut self, forward: bool) {
        // the cover opens onto the first page, and is before it
        if self.look == Look::Cover {
            if forward {
                self.commit_edit();
                self.look = Look::Spread;
                self.spread = 0;
                self.sel.clear();
                self.turned = Some((Instant::now(), 1.0));
            }
            return;
        }
        if !forward && self.spread == 0 {
            self.open_cover();
            return;
        }
        let (l, r) = self.spread_pages();
        if forward && self.spread + 1 < self.spreads() {
            self.go_to(r.unwrap_or(l.unwrap_or(1)) + 1);
        } else if forward && r.is_some() {
            // past the last page: a new one, unless the last is still blank
            let last = self.count();
            if self.page(last).is_some_and(|p| !p.items.is_empty()) {
                let n = self.append_page("");
                self.go_to(n);
            }
        } else if !forward && self.spread > 0 {
            self.go_to(l.unwrap_or(1).saturating_sub(1).max(1));
        }
    }

    /// A new page after the one shown on the right (or at the end), opened.
    pub fn new_page_here(&mut self, out: &mut Vec<Out>) {
        self.commit_edit();
        let (l, r) = self.spread_pages();
        let cur = r.or(l).unwrap_or(self.count());
        if cur >= self.count() {
            let n = self.append_page("");
            self.go_to(n);
        } else {
            self.insert_after(cur, out);
            self.go_to(cur + 1);
        }
        out.push(Out::Rescan);
    }

    pub fn page_ref(&self, n: usize) -> Option<&PageRef> {
        if n == 0 {
            return Some(&self.cover_ref);
        }
        self.nb.pages.get(n - 1)
    }

    /// The cover, to look at and change.
    pub fn open_cover(&mut self) {
        self.commit_edit();
        self.look = Look::Cover;
        self.sel.clear();
        self.crop = None;
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
        self.turned = Some((Instant::now(), -1.0));
    }

    /// A page by number, read from disk the first time.
    pub fn loaded(&mut self, n: usize) -> Option<&mut Loaded> {
        let path = self.page_ref(n)?.path.clone();
        if !self.loaded.contains_key(&path) {
            let mut l = Loaded::read(&self.root, &path);
            self.restore_backup(&mut l);
            if n > 0 {
                let lay = self.layout_for(n, &l.page);
                adopt_notes_text(&mut l, &lay);
                // its texts are set again (written in the notes mode while the
                // Journal was closed, a box may not match its text)
                self.unsettled.insert(n);
                for it in l.page.items.iter().filter(|i| i.flow.as_ref().is_some_and(|f| f.next.is_some())) {
                    self.repull.insert(it.id.clone());
                }
            }
            self.loaded.insert(path.clone(), l);
        }
        self.loaded.get_mut(&path)
    }

    fn layout_for(&self, n: usize, page: &Page) -> Layout {
        Layout::new(&self.nb.size, page.paper().unwrap_or(self.nb.paper), self.nb.columns, n % 2 == 1)
    }

    pub fn page(&mut self, n: usize) -> Option<&mut Page> {
        self.loaded(n).map(|l| &mut l.page)
    }

    pub fn page_ro(&self, n: usize) -> Option<&Page> {
        let path = &self.page_ref(n)?.path;
        self.loaded.get(path).map(|l| &l.page)
    }

    /// Where things fall on page `n`.
    pub fn layout(&self, n: usize) -> Layout {
        if n == 0 {
            // the cover: no ruling, no columns (text still has lines to sit on)
            return Layout::new(&self.nb.size, journal::Paper { ruling: journal::Ruling::Plain, spacing: 7.0 }, journal::Columns::None, true);
        }
        let paper = self.page_ro(n).and_then(|p| p.paper()).unwrap_or(self.nb.paper);
        Layout::new(&self.nb.size, paper, self.nb.columns, n % 2 == 1)
    }

    fn backup_path(&self, path: &str) -> Option<PathBuf> {
        self.backup_dir.as_ref().map(|d| d.join(format!("{}.json", path.replace('/', "__"))))
    }

    /// A plugin rewrote the note without the block: the items come back
    /// from the copy kept beside it.
    fn restore_backup(&self, l: &mut Loaded) {
        if l.page.has_block() {
            return;
        }
        let Some(v) = self.backup_path(&l.path).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return };
        l.page.restore(&v);
        l.dirty = true;
        l.changed_at = Some(Instant::now());
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { pages: self.loaded.iter().map(|(k, l)| (k.clone(), l.page.clone())).collect() }
    }

    /// Before a change: the state to come back to.
    pub fn checkpoint(&mut self) {
        let s = self.snapshot();
        self.undo.push(s);
        if self.undo.len() > 150 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn uncheckpoint(&mut self) {
        self.undo.pop();
    }

    /// Page `n` changed (it will be saved shortly).
    pub fn touch(&mut self, n: usize) {
        if let Some(l) = self.loaded(n) {
            l.dirty = true;
            l.changed_at = Some(Instant::now());
        }
    }

    /// A body was edited here (it wins over the disk's unless the disk's
    /// changed too).
    fn edited_body(&mut self, n: usize, id: &str) {
        if let Some(p) = self.page_ref(n).map(|r| r.path.clone()) {
            self.edited.entry(p).or_default().insert(id.to_string());
        }
    }

    fn restore(&mut self, s: Snapshot) {
        for (path, page) in s.pages {
            if let Some(l) = self.loaded.get_mut(&path) {
                if l.page != page {
                    l.page = page;
                    l.dirty = true;
                    l.changed_at = Some(Instant::now());
                }
            }
        }
        let n = self.sel_page;
        let ids: HashSet<String> = self.page_ro(n).map(|p| p.items.iter().map(|i| i.id.clone()).collect()).unwrap_or_default();
        self.sel.retain(|id| ids.contains(id));
    }

    pub fn undo(&mut self) {
        self.commit_edit();
        if let Some(s) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        self.commit_edit();
        if let Some(s) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            self.restore(s);
        }
    }

    /// Save what waits (all of it with `now`), reading the disk first: what
    /// changed there meanwhile is merged.
    pub fn save(&mut self, now: bool, out: &mut Vec<Out>) {
        let busy = self.drag.is_some();
        let due: Vec<String> = self.loaded.iter().filter(|(_, l)| l.dirty && (now || (!busy && l.changed_at.is_some_and(|t| t.elapsed() > Duration::from_millis(1500))))).map(|(k, _)| k.clone()).collect();
        for path in due {
            let backup = self.backup_path(&path);
            let edited = self.edited.remove(&path).unwrap_or_default();
            let Some(l) = self.loaded.get_mut(&path) else { continue };
            let disk_mtime = std::fs::metadata(&l.abs).and_then(|m| m.modified()).ok();
            if disk_mtime.is_some() && disk_mtime != l.mtime {
                if let Ok(text) = std::fs::read_to_string(&l.abs) {
                    let disk = Page::parse(&text);
                    let mut ours = l.page.clone();
                    // a body is "ours" only when it was typed here
                    for it in ours.items.iter_mut() {
                        if !edited.contains(&it.id) {
                            it.body = l.base.item(&it.id).and_then(|b| b.body.clone()).or(it.body.take());
                        }
                    }
                    let (merged, conflicts) = format::merge(&ours, &l.base, &disk);
                    if !conflicts.is_empty() {
                        out.push(Out::Toast(format!("{} foi mudada também no disco; ficou o texto do disco em {} {}.", vault_name(&path), conflicts.len(), if conflicts.len() == 1 { "item" } else { "itens" })));
                    }
                    l.page = merged;
                }
            }
            for it in l.page.items.iter_mut() {
                it.tidy();
            }
            let text = l.page.write();
            match ateliet_core::quadro::write_atomic(&l.abs, &text) {
                Ok(()) => {
                    l.dirty = false;
                    l.changed_at = None;
                    l.mtime = std::fs::metadata(&l.abs).and_then(|m| m.modified()).ok();
                    l.base = Page::parse(&text);
                    if let Some(b) = backup {
                        let _ = ateliet_core::quadro::write_atomic(&b, &l.page.block_value().to_string());
                    }
                }
                Err(e) => out.push(Out::Toast(format!("Não foi possível salvar {}: {e}", vault_name(&path)))),
            }
        }
        self.write_marker();
    }

    /// The bookmark: the notebook opens where it was left.
    fn write_marker(&mut self) {
        let (l, r) = self.spread_pages();
        let m = r.or(l).unwrap_or(1);
        if m == self.marker_written || m == 0 {
            return;
        }
        self.edit_index(|t| format::set_marker(t, m));
        self.marker_written = m;
        self.nb.marker = m;
    }

    /// Change the index note's text (its frontmatter): through the cover's
    /// page when it is loaded (the cover lives in that note), else on disk.
    fn edit_index(&mut self, f: impl FnOnce(&str) -> String) {
        let path = self.nb.path.clone();
        if let Some(l) = self.loaded.get_mut(&path) {
            l.page.head = f(&l.page.head);
            l.dirty = true;
            l.changed_at = Some(Instant::now());
            return;
        }
        let abs = self.root.join(&path);
        if let Ok(text) = std::fs::read_to_string(&abs) {
            let _ = ateliet_core::quadro::write_atomic(&abs, &f(&text));
        }
    }

    /// Pages changed on disk (by Obsidian) and not here: read again.
    pub fn watch_disk(&mut self, out: &mut Vec<Out>) {
        if self.drag.is_some() || self.editing.is_some() {
            return;
        }
        let mut reread = Vec::new();
        for (k, l) in &self.loaded {
            if l.dirty {
                continue;
            }
            let now = std::fs::metadata(&l.abs).and_then(|m| m.modified()).ok();
            if now.is_some() && now != l.mtime {
                reread.push(k.clone());
            }
        }
        for k in reread {
            let mut l = Loaded::read(&self.root, &k);
            self.restore_backup(&mut l);
            let n = if k == self.nb.path { 0 } else { self.nb.pages.iter().position(|p| p.path == k).map(|i| i + 1).unwrap_or(0) };
            if n > 0 {
                let lay = self.layout_for(n, &l.page);
                adopt_notes_text(&mut l, &lay);
                // texts changed in the note are set again: their boxes grow
                // (or shrink) to fit, and push down what is under them
                self.unsettled.insert(n);
                let old = self.loaded.get(&k).map(|o| o.page.clone());
                for it in l.page.items.iter().filter(|i| i.flow.as_ref().is_some_and(|f| f.next.is_some())) {
                    let before = old.as_ref().and_then(|o| o.item(&it.id)).map(|o| o.content().to_string());
                    if before.as_deref() != Some(it.content()) {
                        self.repull.insert(it.id.clone());
                    }
                }
            }
            out.push(Out::Toast(format!("{} mudou no disco e foi relida.", vault_name(&k))));
            self.loaded.insert(k, l);
        }
    }

    pub fn dirty(&self) -> bool {
        self.loaded.values().any(|l| l.dirty)
    }

    /// A new page at the end of the notebook.
    pub fn append_page(&mut self, date: &str) -> usize {
        let n = self.count() + 1;
        let path = self.nb.page_path(n, "");
        let date = if date.is_empty() { today() } else { date.to_string() };
        let text = Page::new_text(&self.nb.name, n, &date);
        let abs = self.root.join(&path);
        let _ = ateliet_core::quadro::write_atomic(&abs, &text);
        self.nb.pages.push(PageRef { path, number: n, title: String::new(), date: Some(date) });
        n
    }

    /// The page of a day in a diary notebook: the one dated so, or a new one.
    pub fn day_page(&mut self, date: &str) -> usize {
        if let Some(i) = self.nb.pages.iter().position(|p| p.date.as_deref() == Some(date)) {
            return i + 1;
        }
        self.append_page(date)
    }

    /// A new page right after `after`: the pages after it move one on
    /// (their files renamed, their numbers set by the app).
    pub fn insert_after(&mut self, after: usize, out: &mut Vec<Out>) {
        self.save(true, out);
        let n = self.append_page("");
        if after >= n - 1 {
            return;
        }
        let new = self.nb.pages.remove(n - 1);
        self.nb.pages.insert(after, new);
        self.reorder_files(out);
    }

    /// Bring the files in line with the order of `nb.pages`.
    pub fn reorder_files(&mut self, out: &mut Vec<Out>) {
        self.save(true, out);
        let renames = self.nb.renames();
        let numbers: Vec<(String, usize)> = self.nb.pages.iter().enumerate().map(|(i, p)| {
            let to = renames.iter().find(|(o, _)| o == &p.path).map(|(_, n)| n.clone()).unwrap_or_else(|| p.path.clone());
            (to, i + 1)
        }).collect();
        self.loaded.clear();
        self.undo.clear();
        self.redo.clear();
        self.sel.clear();
        out.push(Out::Reorder { renames, numbers });
    }

    /// Change a setting of the notebook in its index note.
    pub fn set_setting(&mut self, key: &str, value: Option<&str>) {
        self.edit_index(|t| format::set_key(t, key, value));
    }

    /// Read the pages again (after the app renamed them).
    pub fn rescan(&mut self) {
        self.loaded.clear();
        self.nb.scan(&self.root);
        self.spread = self.spread.min(self.spreads() - 1);
    }

    pub fn delete_page(&mut self, n: usize, out: &mut Vec<Out>) {
        let Some(r) = self.page_ref(n).cloned() else { return };
        self.save(true, out);
        if trash::delete(self.root.join(&r.path)).is_err() {
            let _ = std::fs::remove_file(self.root.join(&r.path));
        }
        self.nb.pages.remove(n - 1);
        if self.nb.pages.is_empty() {
            self.append_page("");
        }
        self.reorder_files(out);
        self.spread = self.spread.min(self.spreads() - 1);
        out.push(Out::Toast(format!("Página {n} apagada (está na lixeira).")));
    }

    // ── text that goes on ─────────────────────────────────────────────────

    /// Where an item is, among the loaded pages: (page number, index).
    fn find(&mut self, id: &str) -> Option<(usize, usize)> {
        for n in 1..=self.count() {
            let path = &self.nb.pages[n - 1].path;
            if let Some(l) = self.loaded.get(path) {
                if let Some(i) = l.page.items.iter().position(|it| it.id == id) {
                    return Some((n, i));
                }
            }
        }
        // the neighbours of loaded pages may hold the rest of a chain
        for n in 1..=self.count() {
            if let Some(p) = self.page(n) {
                if let Some(i) = p.items.iter().position(|it| it.id == id) {
                    return Some((n, i));
                }
            }
        }
        None
    }

    /// The text of a box as it is now (the one being typed: as typed).
    pub fn text_now(&mut self, n: usize, id: &str) -> Option<String> {
        if let Some(e) = self.editing.as_ref().filter(|e| e.page == n && e.id == id) {
            return Some(e.buf.clone());
        }
        self.page(n).and_then(|p| p.item(id)).map(flow_text)
    }

    /// What is being typed, into its box (it goes in as it is typed).
    pub fn store_typing(&mut self) {
        let Some((n, id, buf)) = self.editing.as_ref().map(|e| (e.page, e.id.clone(), e.buf.clone())) else { return };
        let mut more = false;
        if let Some(slot) = self.page(n).and_then(|p| p.item_mut(&id)) {
            let ex = (slot.kind == Kind::Excerpt).then(|| journal::excerpt_parts(slot.content()));
            let last = slot.flow.as_ref().is_none_or(|f| f.next.is_none());
            let first = slot.flow.as_ref().is_none_or(|f| f.prev.is_none());
            set_flow_text(slot, &buf, ex.as_ref(), first, last);
            more = !last;
        }
        self.edited_body(n, &id);
        self.touch(n);
        // shorter, it may take text back from the next box
        if more {
            self.repull.insert(id);
        }
    }

    /// Put text in a box (and in what is being typed, when it is that box).
    pub fn set_text(&mut self, n: usize, id: &str, text: &str) {
        if let Some(e) = self.editing.as_mut().filter(|e| e.page == n && e.id == id) {
            let len = text.chars().count();
            e.buf = text.to_string();
            e.cursor = e.cursor.min(len);
            e.anchor = e.anchor.min(len);
            e.ime = None;
        }
        if let Some(slot) = self.page(n).and_then(|p| p.item_mut(id)) {
            let ex = (slot.kind == Kind::Excerpt).then(|| journal::excerpt_parts(slot.content()));
            let last = slot.flow.as_ref().is_none_or(|f| f.next.is_none());
            let first = slot.flow.as_ref().is_none_or(|f| f.prev.is_none());
            set_flow_text(slot, text, ex.as_ref(), first, last);
        }
        self.edited_body(n, id);
        self.touch(n);
    }

    /// Set page `n`: its texts in rows on the ruling (`journal::wrap`), each
    /// box where its rows are. A text pushed past the last line goes to the
    /// next page; what does not fit in a box goes on in the next one; a box
    /// that got shorter takes its text back from the next. `prev` is the
    /// page's last setting; the new one comes back.
    pub fn settle(&mut self, ctx: &egui::Context, n: usize, prev: Option<&PageSet>) -> Option<PageSet> {
        let lay = self.layout(n);
        let typing = self.editing.as_ref().filter(|e| e.page == n).map(|e| (e.id.clone(), e.buf.clone(), e.cursor));
        let skip: Vec<String> = match &self.drag {
            Some(Drag::Move { page, orig, moved: true, .. }) if *page == n => orig.iter().map(|o| o.0.clone()).collect(),
            _ => Vec::new(),
        };
        let items = self.page(n)?.items.clone();
        let t = typing.as_ref().map(|(a, b, c)| (a.as_str(), b.as_str(), *c));
        let texts = typeset::styled_page(&items, &lay, typeset::K_REF, t, egui::Color32::BLACK, egui::Color32::BLACK);
        let set = typeset::set_page(ctx, &items, &lay, &texts, prev, &skip);
        let mut changed = false;
        let mut off = Vec::new();
        let mut over = Vec::new();
        let page = self.page(n)?;
        for t in &set.texts {
            let Some(it) = page.item_mut(&t.id) else { continue };
            if t.off {
                off.push(t.id.clone());
                continue;
            }
            if (it.y - t.top).abs() > 0.005 {
                // pushed: where it was put is kept, to go back to
                if it.home.is_none() {
                    it.home = Some(it.y);
                }
                it.y = t.top;
                changed = true;
            }
            if it.home.is_some_and(|h| (h - it.y).abs() < 0.01) {
                it.home = None;
                changed = true;
            }
            if (it.h - t.height).abs() > 0.005 {
                it.h = t.height;
                changed = true;
            }
            if t.overflows() {
                over.push((t.id.clone(), texts[&t.id].buf_of(t.placed)));
            }
        }
        if changed {
            self.touch(n);
        }
        for id in off {
            self.move_on(n, &id);
        }
        for (id, at) in over {
            self.overflow(n, &id, at);
        }
        let pulls: Vec<String> = set.texts.iter().filter(|t| !t.off && !t.overflows() && self.repull.contains(&t.id)).map(|t| t.id.clone()).collect();
        for id in pulls {
            self.repull.remove(&id);
            self.pull(n, &id);
        }
        Some(set)
    }

    /// A text with no room left on page `n` goes to the top of the next page
    /// (and pushes down what is there, as it is set first).
    fn move_on(&mut self, n: usize, id: &str) {
        if n == 0 || self.editing.as_ref().is_some_and(|e| e.id == id) {
            return;
        }
        let Some(mut it) = self.page(n).and_then(|p| p.item(id)).cloned() else { return };
        // alone on its page, there is nowhere better
        if it.locked || self.page_ro(n).is_some_and(|p| p.items.iter().filter(|i| wrap::is_set(i)).count() <= 1) {
            return;
        }
        let next = if n >= self.count() { self.append_page("") } else { n + 1 };
        let nl = self.layout(next);
        it.y = nl.first_baseline() - nl.paper.line() * it.style.lines();
        it.home = None;
        if let Some(p) = self.page(n) {
            p.items.retain(|i| i.id != id);
        }
        if let Some(p) = self.page(next) {
            p.items.push(it);
        }
        self.sel.retain(|s| s != id);
        self.touch(n);
        self.touch(next);
        self.unsettled.insert(next);
    }

    /// Text `id` of page `n` holds more than fits: what is from char `at`
    /// on goes on in the next box (one is made on the next page, in the same
    /// place of its column, when there is none). Typing past the end goes
    /// on there too.
    fn overflow(&mut self, n: usize, id: &str, at: usize) {
        let Some(text) = self.text_now(n, id) else { return };
        let cut = char_byte(&text, at);
        if cut >= text.len() {
            return;
        }
        let (head, tail) = text.split_at(cut);
        let para = head.ends_with('\n');
        let head = head.strip_suffix('\n').unwrap_or(head).to_string();
        let tail = tail.to_string();
        let Some(it) = self.page(n).and_then(|p| p.item(id)).cloned() else { return };
        let ex = (it.kind == Kind::Excerpt).then(|| journal::excerpt_parts(it.content()));
        let next = it.flow.as_ref().and_then(|f| f.next.clone()).and_then(|nx| self.find(&nx).map(|(nn, _)| (nn, nx)));
        let (nn, nid, joined) = match next {
            Some((nn, nid)) => {
                let before = self.text_now(nn, &nid).unwrap_or_default();
                let was_para = self.page(nn).and_then(|p| p.item(&nid)).and_then(|i| i.flow.as_ref()).is_some_and(|f| f.para);
                let joined = if before.is_empty() { tail.clone() } else { format!("{tail}{}{before}", if was_para { "\n" } else { "" }) };
                if let Some(f) = self.page(nn).and_then(|p| p.item_mut(&nid)).and_then(|i| i.flow.as_mut()) {
                    f.para = para;
                }
                (nn, nid, joined)
            }
            None => {
                let nn = if n >= self.count() { self.append_page("") } else { n + 1 };
                // the same column on the next page (the margin goes to the
                // margin even when the pages mirror it), at the same place in it
                let lay = self.layout(n);
                let nl = self.layout(nn);
                let here = lay.column_at(it.x + it.w / 2.0);
                let in_margin = lay.margin.is_some_and(|m| lay.columns[m] == here);
                let idx = lay.columns.iter().position(|c| *c == here).unwrap_or(0);
                let (c0, c1) = if in_margin {
                    nl.margin.map(|m| nl.columns[m]).unwrap_or_else(|| nl.main_column())
                } else if lay.margin.is_some() {
                    nl.main_column()
                } else {
                    nl.columns.get(idx).copied().unwrap_or_else(|| nl.main_column())
                };
                let off = (it.x - here.0).max(0.0);
                let x = (c0 + off).min(c1 - 10.0);
                let w = if (it.x + it.w - here.1).abs() < 1.0 { c1 - x } else { it.w.min(c1 - x) };
                let mut nx = Item::new(it.kind.clone(), x, nl.first_baseline() - nl.paper.line() * it.style.lines(), w, nl.paper.line());
                nx.style = it.style.clone();
                nx.style.indent = 0.0;
                nx.flow = Some(journal::Flow { prev: Some(id.to_string()), next: None, para });
                set_flow_text(&mut nx, "", ex.as_ref(), false, true);
                let nid = nx.id.clone();
                if let Some(p) = self.page(nn) {
                    p.items.push(nx);
                }
                if let Some(slot) = self.page(n).and_then(|p| p.item_mut(id)) {
                    let mut f = slot.flow.clone().unwrap_or_default();
                    f.next = Some(nid.clone());
                    slot.flow = Some(f);
                }
                (nn, nid, tail.clone())
            }
        };
        // the cursor in what went on: the typing goes on there
        let follow = self.editing.as_ref().filter(|e| e.page == n && e.id == id && e.cursor > head.chars().count()).map(|e| (e.cursor.saturating_sub(at), e.typed));
        self.set_text(n, id, &head);
        self.set_text(nn, &nid, &joined);
        if let Some((c, typed)) = follow {
            let mut e = Editing::new(nn, &nid, joined, false, Some(c));
            e.typed = typed;
            self.editing = Some(e);
            self.sel_page = nn;
            self.sel = vec![nid.clone()];
            self.follow = Some(nn);
        }
        self.unsettled.insert(nn);
    }

    /// Text `id` of page `n` has room again: the next box's text comes back
    /// into it (what does not fit goes on again when it is set); a next box
    /// left empty with none after it goes.
    fn pull(&mut self, n: usize, id: &str) {
        let Some(nid) = self.page(n).and_then(|p| p.item(id)).and_then(|i| i.flow.as_ref()).and_then(|f| f.next.clone()) else { return };
        let Some((nn, _)) = self.find(&nid) else { return };
        if self.editing.as_ref().is_some_and(|e| e.id == nid) {
            return;
        }
        let Some(nx) = self.page(nn).and_then(|p| p.item(&nid)).cloned() else { return };
        let more = flow_text(&nx);
        let para = nx.flow.as_ref().is_some_and(|f| f.para);
        let mine = self.text_now(n, id).unwrap_or_default();
        let joined = if more.is_empty() { mine } else { format!("{mine}{}{more}", if para { "\n" } else { "" }) };
        let after = nx.flow.as_ref().and_then(|f| f.next.clone());
        if after.is_none() {
            if let Some(p) = self.page(nn) {
                p.items.retain(|i| i.id != nid);
            }
            if let Some(slot) = self.page(n).and_then(|p| p.item_mut(id)) {
                if let Some(f) = slot.flow.as_mut() {
                    f.next = None;
                }
                if slot.flow.as_ref().is_some_and(|f| f.prev.is_none()) {
                    slot.flow = None;
                }
                // an excerpt's source link comes back to its last box
                if slot.kind == Kind::Excerpt {
                    let mut ex = journal::excerpt_parts(slot.content());
                    ex.foot = journal::excerpt_parts(nx.content()).foot;
                    let first = slot.flow.as_ref().is_none_or(|f| f.prev.is_none());
                    set_flow_text(slot, &ex.text.clone(), Some(&ex), first, true);
                }
            }
            self.touch(nn);
        } else {
            self.set_text(nn, &nid, "");
            if let Some(f) = self.page(nn).and_then(|p| p.item_mut(&nid)).and_then(|i| i.flow.as_mut()) {
                f.para = false;
            }
        }
        self.set_text(n, id, &joined);
        self.unsettled.insert(nn);
    }

    /// End typing. The text is in its box already (it went in as it was
    /// typed); a new text left empty goes away.
    pub fn commit_edit(&mut self) {
        let Some(e) = self.editing.take() else { return };
        let Some(it) = self.page(e.page).and_then(|p| p.item(&e.id)).cloned() else { return };
        if e.buf.trim().is_empty() && it.flow.is_none() && it.kind == Kind::Text {
            if let Some(p) = self.page(e.page) {
                p.items.retain(|i| i.id != e.id);
            }
            if e.new && e.typed.is_none() {
                self.uncheckpoint();
            } else {
                self.touch(e.page);
            }
            self.sel.retain(|s| s != &e.id);
            return;
        }
        // set again without the line breaks only the typing had
        self.unsettled.insert(e.page);
        if it.flow.as_ref().is_some_and(|f| f.next.is_some()) {
            self.repull.insert(e.id);
        }
    }
}

/// The text of a box that flows: a text's Markdown, an excerpt's passage.
pub fn flow_text(it: &Item) -> String {
    match it.kind {
        Kind::Excerpt => journal::excerpt_parts(it.content()).text,
        _ => it.content().to_string(),
    }
}

/// Put a piece of a chain's text in a box. An excerpt's piece keeps the
/// source on top (first box) and the link at the end (last box).
pub fn set_flow_text(it: &mut Item, text: &str, ex: Option<&journal::ExcerptParts>, first: bool, last: bool) {
    match (&it.kind, ex) {
        (Kind::Excerpt, Some(p)) => {
            let title = if first { p.title.as_str() } else { "" };
            let foot = if last { p.foot.as_str() } else { "" };
            it.set_content(&journal::excerpt_section(title, text, foot));
        }
        _ => it.set_content(text),
    }
}

/// The byte offset of the `n`th char.
pub fn char_byte(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(b, _)| b).unwrap_or(s.len())
}

fn vault_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string()
}

impl Journal {
    /// The notebooks of the vault, looked for in the background (again when
    /// the vault's files change).
    pub fn scan_shelf(&mut self, vault: &Vault) {
        let gen = vault.files.len() as u64 ^ vault.files.iter().map(|e| e.mtime).max().unwrap_or(0);
        if let Some(rx) = &self.shelf_rx {
            if let Ok(list) = rx.try_recv() {
                self.shelf = Some(list);
                self.shelf_rx = None;
            }
            return;
        }
        if gen == self.shelf_gen {
            return;
        }
        self.shelf_gen = gen;
        let root = vault.root.clone();
        let files: Vec<String> = vault.files.iter().filter(|e| !e.folder && e.path.to_lowercase().ends_with(".md")).map(|e| e.path.clone()).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        self.shelf_rx = Some(rx);
        std::thread::spawn(move || {
            let mut out: Vec<Notebook> = files.iter().filter(|p| format::is_notebook_file(&root.join(p))).filter_map(|p| {
                let text = std::fs::read_to_string(root.join(p)).ok()?;
                let mut nb = Notebook::parse(p, &text)?;
                nb.scan(&root);
                Some(nb)
            }).collect();
            out.sort_by_key(|a| a.name.to_lowercase());
            let _ = tx.send(out);
        });
    }

    pub fn open_notebook(&mut self, vault: &Vault, path: &str, backup: Option<PathBuf>) {
        self.close(vault);
        let Ok(text) = std::fs::read_to_string(vault.abs(path)) else { return };
        let Some(nb) = Notebook::parse(path, &text) else {
            self.out.push(Out::Toast(format!("{path} não é um caderno.")));
            return;
        };
        self.book = Some(Book::open(vault.root.clone(), nb, backup));
        self.out.push(Out::Rescan);
    }

    /// Back to the shelf (everything saved).
    pub fn close(&mut self, _vault: &Vault) {
        if let Some(mut b) = self.book.take() {
            b.commit_edit();
            b.save(true, &mut self.out);
        }
        self.shelf_gen = u64::MAX;
    }

    pub fn save_all(&mut self) {
        if let Some(b) = self.book.as_mut() {
            b.commit_edit();
            b.save(true, &mut self.out);
        }
    }

    /// The excerpts that arrived in the inbox (every few seconds).
    fn poll_inbox(&mut self, cx: &Cx) {
        if self.inbox_at.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.inbox_at = Instant::now();
        let Some(dir) = cx.config_dir.as_ref().map(|d| capture::inbox::dir(d)) else { return };
        if !dir.is_dir() {
            return;
        }
        let got = capture::inbox::import(&cx.vault.root, &dir, capture::FOLDER);
        if got.is_empty() {
            return;
        }
        self.out.push(Out::Rescan);
        let n = got.len();
        if let Some(a) = got.into_iter().last() {
            self.arrived = Some((a.note, a.snippet));
        }
        self.out.push(Out::Toast(if n == 1 { "Um trecho chegou pela caixa de entrada.".into() } else { format!("{n} trechos chegaram pela caixa de entrada.") }));
    }

    /// The whole mode; what it asks of the app comes back.
    pub fn ui(&mut self, ctx: &egui::Context, cx: &mut Cx) -> Vec<Out> {
        self.poll_inbox(cx);
        if ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            self.seen = Instant::now();
        }
        if let Some(b) = self.book.as_mut() {
            b.watch_disk(&mut self.out);
            b.save(false, &mut self.out);
            if b.dirty() {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
        let p = cx.palette;
        // the material slides in from the left
        let mut panel = self.panel;
        if self.book.is_some() {
            egui::SidePanel::left("journal-material").resizable(true).show_separator_line(false).default_width(300.0).width_range(220.0..=520.0).frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin { left: 12, right: 10, top: 12, bottom: 10 })).show_animated(ctx, panel.is_some() && !self.immersive, |ui| {
                let clip = ui.clip_rect();
                ui.painter().vline(clip.right() - 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
                self.material(ui, cx, &mut panel);
            });
        }
        self.panel = panel;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::ZERO)).show(ctx, |ui| {
            if self.book.is_some() {
                self.notebook_ui(ui, cx);
            } else {
                self.scan_shelf(cx.vault);
                self.shelf_ui(ui, cx);
            }
        });
        self.new_notebook_dialog(ctx, cx);
        self.board_dialog_ui(ctx, cx);
        self.delete_dialog(ctx, cx);
        if std::mem::take(&mut self.close_requested) {
            self.close(cx.vault);
        }
        std::mem::take(&mut self.out)
    }

    /// A picture's texture, through the item's filter when it has one.
    pub fn picture(&mut self, ctx: &egui::Context, cx: &mut Cx, target: &str, source: &str, filter: Option<&str>) -> Option<(egui::TextureId, egui::Vec2)> {
        let Some(f) = filter.filter(|f| !f.is_empty()) else {
            return crate::host::image(ctx, cx.vault, cx.caches, target, source);
        };
        let path = cx.vault.resolve(target, source)?;
        let key = (path.clone(), f.to_string());
        if !self.filtered.contains_key(&key) {
            let made = image::open(cx.vault.abs(&path)).ok().map(|img| {
                let img = if img.width() > 1600 || img.height() > 1600 { img.thumbnail(1600, 1600) } else { img };
                let mut rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                ateliet_core::typeset::compose::filter(&mut rgba, w, h, f);
                let size = [w as usize, h as usize];
                let c = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                (ctx.load_texture(format!("journal:{path}:{f}"), c, egui::TextureOptions::LINEAR), egui::vec2(w as f32, h as f32))
            });
            self.filtered.insert(key.clone(), made);
        }
        self.filtered[&key].as_ref().map(|(t, s)| (t.id(), *s))
    }

    /// Pictures changed on disk: their filtered copies are made again.
    pub fn changed(&mut self, paths: &[String]) {
        self.filtered.retain(|(p, _), _| !paths.contains(p));
        self.notes.retain(|p, _| !paths.contains(p));
        self.snippets.retain(|p, _| !paths.contains(p));
        if paths.iter().any(|p| p.to_lowercase().ends_with(".md")) {
            self.shelf_gen = u64::MAX;
        }
    }

    /// An excerpt's note, read once per change.
    /// The place in a book an item came from (an excerpt or a cut kept by
    /// the reader), found through the page's sources.
    pub fn reader_origin(&mut self, vault: &Vault, b: &mut Book, n: usize, it: &Item) -> Option<ateliet_core::reader::Locator> {
        let page_path = b.page_ref(n).map(|r| r.path.clone()).unwrap_or_default();
        let sources: Vec<String> = b.page(n)?.frontmatter().get("fontes").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim_start_matches("[[").trim_end_matches("]]").to_string())).collect()).unwrap_or_default();
        let picture = it.picture().map(|p| p.rsplit('/').next().unwrap_or(&p).to_string());
        for link in sources {
            let Some(path) = vault.resolve(&link, &page_path) else { continue };
            let Some(s) = self.snippet(vault, &path) else { continue };
            if s.origin != "leitor" {
                continue;
            }
            let same = match it.kind {
                Kind::Excerpt => s.section().trim() == it.content().trim() || (!s.text.is_empty() && it.content().contains(s.text.lines().next().unwrap_or(""))),
                _ => picture.is_some() && s.image.as_ref().and_then(|i| i.rsplit('/').next()) == picture.as_deref(),
            };
            if same {
                return s.source.locator.as_ref().and_then(ateliet_core::reader::Locator::from_json);
            }
        }
        None
    }

    pub fn snippet(&mut self, vault: &Vault, path: &str) -> Option<Snippet> {
        let mtime = vault.files.iter().find(|e| e.path == path).map(|e| e.mtime).unwrap_or(0);
        if self.snippets.get(path).is_none_or(|(m, _)| *m != mtime) {
            let s = std::fs::read_to_string(vault.abs(path)).ok().and_then(|t| Snippet::from_note(&t));
            self.snippets.insert(path.to_string(), (mtime, s));
        }
        self.snippets[path].1.clone()
    }

    /// A note's text, for a live note on a page.
    pub fn note_text(&mut self, vault: &Vault, target: &str, source: &str) -> Option<String> {
        let (link, section) = target.split_once('#').map(|(a, b)| (a, Some(b))).unwrap_or((target, None));
        let path = vault.resolve(link, source)?;
        let mtime = vault.files.iter().find(|e| e.path == path).map(|e| e.mtime).unwrap_or(0);
        let key = format!("{path}#{}", section.unwrap_or(""));
        if self.notes.get(&key).is_none_or(|(m, _)| *m != mtime) {
            let text = std::fs::read_to_string(vault.abs(&path)).unwrap_or_default();
            let body = ateliet_core::scriptorium::strip_frontmatter(&text).to_string();
            let body = match section {
                Some(s) => ateliet_core::scriptorium::body_section(&text, s),
                None => body,
            };
            self.notes.insert(key.clone(), (mtime, body.trim().to_string()));
        }
        Some(self.notes[&key].1.clone())
    }
}
