//! The Journal as the app's third mode (beside writing and pictures): what
//! the app lends it each frame, and what it asks of the app back — a note
//! to open, pages to rename with their links, a toast.

use crate::app::App;
use crate::journal_ui::{Cx, Out};
use eframe::egui;

impl App {
    pub fn journal_mode_ui(&mut self, ctx: &egui::Context) {
        let attach = self.attachment_folder();
        let Some(v) = self.vault.as_ref() else { return };
        let root = v.root.clone();
        self.images.open(&root, ctx);
        // the reader beside takes the right of the window
        self.journal.reading_beside = self.side_reader.is_some();
        if self.side_reader.is_none() { self.video.rail(ctx, self.palette); }
        self.side_reader_ui(ctx);
        let Some(v) = self.vault.as_ref() else { return };
        let outs = {
            let mut cx = Cx { vault: v, caches: &mut self.caches, palette: self.palette, images: &mut self.images, config_dir: self.cfg.as_ref().map(|c| c.dir.clone()), attach };
            self.journal.ui(ctx, &mut cx)
        };
        for o in outs {
            match o {
                Out::Toast(t) => self.toast(t),
                Out::Read(book) => {
                    if book.is_none() && self.side_reader.is_some() {
                        self.side_reader = None;
                    } else {
                        self.read_beside(book);
                    }
                }
                Out::Rescan => {
                    if let Some(v) = self.vault.as_mut() {
                        v.rescan();
                    }
                }
                Out::Open(path) => {
                    let Some(v) = self.vault.as_ref() else { continue };
                    let target = v.resolve(&path, "").unwrap_or(path);
                    self.journal.save_all();
                    self.journal_mode = false;
                    self.open(&target, false);
                }
                Out::Reorder { renames, numbers } => {
                    self.move_files(&renames);
                    if let Some(v) = self.vault.as_ref() {
                        for (path, n) in numbers {
                            let abs = v.abs(&path);
                            if let Ok(text) = std::fs::read_to_string(&abs) {
                                let new = ateliet_core::journal::format::set_number(&text, n);
                                if new != text {
                                    let _ = ateliet_core::quadro::write_atomic(&abs, &new);
                                }
                            }
                        }
                    }
                    if let Some(b) = self.journal.book.as_mut() {
                        b.rescan();
                    }
                }
            }
        }
    }

    /// Several files renamed at once, the links to them rewritten in one
    /// pass (so names may be swapped: they go through temporary names).
    pub fn move_files(&mut self, moves: &[(String, String)]) {
        if moves.is_empty() {
            return;
        }
        self.save_all();
        let Some(v) = &self.vault else { return };
        let tmp: Vec<String> = moves.iter().enumerate().map(|(i, (from, _))| format!("{from}.ateliet-move-{i}")).collect();
        for ((from, _), t) in moves.iter().zip(&tmp) {
            if let Err(e) = std::fs::rename(v.abs(from), v.abs(t)) {
                self.toast(format!("Não foi possível renomear {from}: {e}"));
                return;
            }
        }
        for ((_, to), t) in moves.iter().zip(&tmp) {
            if let Some(d) = v.abs(to).parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::rename(v.abs(t), v.abs(to));
        }
        let update_links = self.cfg.as_ref().map(|c| c.read("app.json")["alwaysUpdateLinks"].as_bool().unwrap_or(true)).unwrap_or(true);
        if let Some(ix) = v.index.read().ok().filter(|_| update_links) {
            let mut sources: Vec<String> = moves.iter().flat_map(|(old, _)| ix.inlinks(old)).collect();
            sources.sort();
            sources.dedup();
            for src in sources {
                let now = moves.iter().find(|(o, _)| *o == src).map(|(_, n)| n.clone()).unwrap_or_else(|| src.clone());
                let Ok(text) = std::fs::read_to_string(v.abs(&now)) else { continue };
                let moved = |t: &str| -> Option<String> {
                    let r = ix.resolve(t, &src)?;
                    moves.iter().find(|(o, _)| *o == r).map(|(_, n)| n.clone())
                };
                if let Some(new_text) = ateliet_core::relink::rewrite(&text, &moved) {
                    let _ = v.write(&now, &new_text);
                }
            }
        }
        for t in &mut self.tabs {
            if let Some((_, to)) = moves.iter().find(|(f, _)| *f == t.path) {
                t.path = to.clone();
            }
        }
        let touched: Vec<String> = moves.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect();
        v.index.write().unwrap().update(&touched);
        self.vault.as_mut().unwrap().rescan();
        self.save_session();
    }

    /// "Mandar ao Journal": the selection of the open note becomes an
    /// excerpt (a note in `Trechos/`), and goes on the open page when a
    /// notebook is open.
    pub fn send_to_journal(&mut self, ctx: &egui::Context) {
        let Some(t) = self.tabs.get(self.active) else { return };
        let Some((a, b)) = crate::editor::selection(ctx, t.editor_id()).filter(|(a, b)| a < b) else {
            self.toast("Selecione um trecho da nota para mandar ao Journal.");
            return;
        };
        let text: String = t.text.chars().skip(a).take(b - a).collect();
        let source = ateliet_core::capture::SourceRef { title: t.title(), book: Some(t.path.trim_end_matches(".md").to_string()), ..Default::default() };
        let origin = if self.current_book().is_some() { "scriptorium" } else { "manual" };
        let s = ateliet_core::capture::Snippet::new(&text, source, origin);
        let Some(v) = self.vault.as_ref() else { return };
        match ateliet_core::capture::save(&v.root, ateliet_core::capture::FOLDER, &s) {
            Ok(note) => {
                self.vault.as_mut().unwrap().rescan();
                if self.journal.book.is_some() {
                    self.journal.arrived = Some((note, s));
                    self.journal_mode = true;
                    self.images_mode = false;
                } else {
                    self.toast(format!("Trecho guardado em {note}."));
                }
            }
            Err(e) => self.toast(format!("Não foi possível guardar o trecho: {e}")),
        }
    }
}
