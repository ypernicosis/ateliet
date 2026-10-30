//! The reader in the app: a book (or the shelf) open in a tab, and the
//! reader beside the Journal ("Ler ao lado"), whose passages and cuts go
//! straight onto the open page. What the reader asks of the app comes
//! back here.

use crate::app::{App, Mode, Tab};
use crate::reader_ui::{Cx, Out, Reader, SHELF};
use ateliet_core::reader::Locator;
use eframe::egui;

impl App {
    fn reader_cx(&self) -> Option<Cx<'_>> {
        let v = self.vault.as_ref()?;
        Some(Cx { vault: v, palette: self.palette, config_dir: self.cfg.as_ref().map(|c| c.dir.clone()), journal_open: self.journal.book.is_some() })
    }

    /// A book of the vault (or the shelf) in the reader, at a place.
    pub fn open_book(&mut self, path: &str, at: Option<Locator>, new_tab: bool) {
        if let Some(i) = self.tabs.iter().position(|t| t.reader.is_some() && t.path == path) {
            self.active = i;
            if let (Some(l), Some(r)) = (at, self.tabs[i].reader.as_mut()) {
                r.request(path, Some(l));
            }
            return;
        }
        let mut reader = Reader::new(path, false);
        if path != SHELF {
            reader.request(path, at);
        }
        let tab = Tab { path: path.to_string(), text: String::new(), saved: String::new(), mode: Mode::Read, doc: None, last_edit: None, cursor: 0, highlight: Default::default(), scroll_to: None, live: Default::default(), board: None, quadro: None, view: None, reader: Some(Box::new(reader)), back: Vec::new(), fwd: Vec::new() };
        if new_tab || self.tabs.is_empty() || self.tabs.get(self.active).is_some_and(|t| t.dirty()) {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        } else {
            self.replace_active(tab);
        }
        if path != SHELF {
            self.recent.retain(|p| p != path);
            self.recent.insert(0, path.to_string());
            self.recent.truncate(30);
        }
        self.save_session();
    }

    /// The active tab's reader.
    pub fn reader_pane(&mut self, ui: &mut egui::Ui) {
        let mut reader = match self.tabs.get_mut(self.active).and_then(|t| t.reader.take()) {
            Some(r) => r,
            None => return,
        };
        let outs = match self.reader_cx() {
            Some(cx) => reader.ui(ui, &cx),
            None => Vec::new(),
        };
        // the tab follows the book open in it
        let path = reader.path.clone();
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.reader = Some(reader);
            if t.path != path {
                t.path = path;
                self.save_session();
            }
        }
        self.reader_outs(outs, false);
    }

    /// What a reader asks for.
    fn reader_outs(&mut self, outs: Vec<Out>, side: bool) {
        for o in outs {
            match o {
                Out::Toast(t) => self.toast(t),
                Out::Rescan => {
                    if let Some(v) = self.vault.as_mut() {
                        v.rescan();
                    }
                }
                Out::Open(note) => {
                    if side {
                        self.journal.save_all();
                        self.journal_mode = false;
                    }
                    self.open(&note, true);
                }
                Out::Snippet { note, snippet } => {
                    if side && self.journal.book.is_some() {
                        // beside the Journal: on the open page at once
                        self.journal.pending_snippets.push((note, snippet));
                    } else if self.journal.book.is_some() {
                        self.journal.arrived = Some((note.clone(), snippet));
                        self.toast(format!("Trecho guardado em {note}. No Journal, ele espera para entrar na página aberta."));
                    } else {
                        self.toast(format!("Trecho guardado em {note}."));
                    }
                }
                Out::Beside { file, at } => self.read_beside(Some((file, at))),
                Out::CloseSide => {
                    if let Some(mut r) = self.side_reader.take() {
                        if let Some(cx) = self.reader_cx() {
                            r.save_state(&cx);
                        }
                    }
                }
            }
        }
    }

    /// The Journal with the reader beside it (a book at a place, or the shelf).
    pub fn read_beside(&mut self, book: Option<(String, Option<Locator>)>) {
        match (book, self.side_reader.as_mut()) {
            (Some((file, at)), Some(r)) => r.request(&file, at),
            (Some((file, at)), None) => {
                let mut r = Reader::new(SHELF, true);
                r.request(&file, at);
                self.side_reader = Some(Box::new(r));
            }
            (None, Some(_)) => {}
            (None, None) => self.side_reader = Some(Box::new(Reader::new(SHELF, true))),
        }
        self.images_mode = false;
        self.journal_mode = true;
        if self.journal.book.is_none() {
            self.toast("Abra um caderno para pôr nele os trechos e recortes do livro.");
        }
    }

    /// The reader beside the Journal, on the right of the window.
    pub fn side_reader_ui(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.side_reader.take() else { return };
        let p = self.palette;
        let width = ctx.screen_rect().width();
        let mut outs = Vec::new();
        egui::SidePanel::right("leitor-ao-lado").resizable(true).show_separator_line(false).default_width((width * 0.46).max(360.0)).width_range((width * 0.36).max(320.0)..=(width * 0.7).max(400.0)).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::ZERO)).show(ctx, |ui| {
            let clip = ui.clip_rect();
            ui.painter().vline(clip.left() + 0.5, clip.y_range(), egui::Stroke::new(1.0_f32, p.border));
            if let Some(cx) = self.reader_cx() {
                outs = r.ui(ui, &cx);
            }
        });
        self.side_reader = Some(r);
        self.reader_outs(outs, true);
    }

    /// Where each open book was left, saved.
    pub fn save_readers(&mut self) {
        let Some(v) = self.vault.as_ref() else { return };
        let cx = Cx { vault: v, palette: self.palette, config_dir: self.cfg.as_ref().map(|c| c.dir.clone()), journal_open: false };
        for t in &mut self.tabs {
            if let Some(r) = t.reader.as_mut() {
                r.save_state(&cx);
            }
        }
        if let Some(r) = self.side_reader.as_mut() {
            r.save_state(&cx);
        }
    }
}
