//! A composed page of a note (a `scriptorium-pagina` block) open in the
//! compositor; saving writes the block back into the note.

use super::compositor::{Compositor, Event};
use super::library::Cx;
use super::Action;
use ateliet_core::scriptorium::book;
use ateliet_core::typeset::compose;

pub struct PaginaView {
    pub note: String,
    pub id: String,
    edit: Option<Compositor>,
    missing: bool,
}

impl PaginaView {
    /// `path` is "<note>#<page id>".
    pub fn new(path: &str) -> PaginaView {
        let (note, id) = path.rsplit_once('#').unwrap_or((path, ""));
        PaginaView { note: note.into(), id: id.into(), edit: None, missing: false }
    }

    pub fn title(&self) -> String {
        format!("Página — {}", crate::vault::basename(&self.note))
    }

    pub fn dirty(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.dirty())
    }

    pub fn ui(&mut self, ui: &mut eframe::egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let mut acts = Vec::new();
        if self.edit.is_none() && !self.missing {
            let text = std::fs::read_to_string(cx.vault.root.join(&self.note)).unwrap_or_default();
            match compose::page_blocks(&text).into_iter().find(|(_, m)| m["id"].as_str() == Some(self.id.as_str())) {
                Some((_, m)) => {
                    let cfg = {
                        let ix = cx.vault.index.read().unwrap();
                        book::book_for(&ix, &self.note, &cx.sc.author()).map(|b| book::layout(&b)).unwrap_or_else(|| book::DEFAULT_LAYOUT.clone())
                    };
                    self.edit = Some(Compositor::new(m, false, cfg, 0, &self.note, &self.title()));
                }
                None => self.missing = true,
            }
        }
        let Some(ed) = self.edit.as_mut() else {
            ui.centered_and_justified(|ui| ui.label("Esta página composta não está mais na nota."));
            return acts;
        };
        match ed.ui(ui, cx) {
            Some(Event::Save(m)) => {
                let full = cx.vault.root.join(&self.note);
                let text = std::fs::read_to_string(&full).unwrap_or_default();
                match compose::replace_page(&text, &self.id, &m) {
                    Some(t) if std::fs::write(&full, &t).is_ok() => {
                        ed.mark_saved();
                        acts.push(Action::Saved(self.note.clone()));
                        acts.push(Action::Toast("Página salva.".into()));
                    }
                    _ => acts.push(Action::Toast("Não consegui salvar a página: o bloco sumiu da nota.".into())),
                }
            }
            Some(Event::Close) => acts.push(Action::Open(self.note.clone(), false)),
            None => {}
        }
        acts
    }
}
