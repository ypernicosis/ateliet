//! What the reader keeps in the vault. Highlights go in the book's note
//! (made with the first highlight or excerpt, in `Livros/`); excerpts are
//! notes in `Trechos/` with the place in the book; cuts are PNGs in
//! `Anexos/Recortes/<Livro>/` with an excerpt that embeds them.

use super::{Book, Cx, Out};
use ateliet_core::capture::{self, Snippet, SourceRef};
use ateliet_core::reader::marks::{self, Mark};
use ateliet_core::reader::Locator;
use std::time::{Duration, Instant};

pub const CUTS: &str = "Anexos/Recortes";

impl Book {
    fn meta(&self) -> ateliet_core::reader::Meta {
        self.opened.as_ref().map(|o| o.meta.clone()).unwrap_or_default()
    }

    /// The book's note, looked for among the vault's notes (its `arquivo`
    /// links to the book).
    fn find_note(&self, cx: &Cx) -> Option<String> {
        let prefix = format!("{}/", marks::FOLDER);
        let mut cands: Vec<&str> = cx.vault.markdown_files().filter(|e| e.path.starts_with(&prefix)).map(|e| e.path.as_str()).collect();
        // the note named after the book first
        let title = marks::clean_name(&self.meta().title);
        cands.sort_by_key(|p| !p.ends_with(&format!("/{title}.md")));
        cands.into_iter().find(|p| std::fs::read_to_string(cx.vault.abs(p)).is_ok_and(|t| marks::is_note_of(&t, &self.file))).map(str::to_string)
    }

    pub(super) fn load_marks(&mut self, cx: &Cx) {
        self.note = self.find_note(cx);
        self.read_marks(cx);
    }

    fn read_marks(&mut self, cx: &Cx) {
        self.marks.clear();
        if let Some(n) = &self.note {
            let abs = cx.vault.abs(n);
            self.note_mtime = mtime(&abs);
            if let Ok(t) = std::fs::read_to_string(abs) {
                self.marks = marks::read(&t);
            }
        }
    }

    /// The note changed on disk (edited in Obsidian): the highlights again.
    pub(super) fn watch_note(&mut self, cx: &Cx) {
        if self.note_checked.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        self.note_checked = Some(Instant::now());
        let Some(n) = self.note.clone() else { return };
        if mtime(&cx.vault.abs(&n)) != self.note_mtime {
            self.read_marks(cx);
        }
    }

    /// The book's note, made now if there is none.
    pub(super) fn ensure_note(&mut self, cx: &Cx, out: &mut Vec<Out>) -> Option<String> {
        if let Some(n) = &self.note {
            if cx.vault.abs(n).exists() {
                return Some(n.clone());
            }
        }
        if let Some(n) = self.find_note(cx) {
            self.note = Some(n.clone());
            return Some(n);
        }
        let meta = self.meta();
        let name = marks::clean_name(&meta.title);
        let mut path = format!("{}/{name}.md", marks::FOLDER);
        let mut k = 2;
        while cx.vault.abs(&path).exists() {
            path = format!("{}/{name} {k}.md", marks::FOLDER);
            k += 1;
        }
        match cx.vault.write(&path, &marks::new_note(&meta, &self.file)) {
            Ok(()) => {
                out.push(Out::Rescan);
                self.note = Some(path.clone());
                self.note_mtime = mtime(&cx.vault.abs(&path));
                Some(path)
            }
            Err(e) => {
                out.push(Out::Toast(format!("Não foi possível criar a nota do livro: {e}")));
                None
            }
        }
    }

    fn write_marks(&mut self, cx: &Cx, out: &mut Vec<Out>) {
        let Some(n) = self.ensure_note(cx, out) else { return };
        let abs = cx.vault.abs(&n);
        let text = std::fs::read_to_string(&abs).unwrap_or_default();
        let new = marks::write(&text, &self.marks);
        if new != text {
            if let Err(e) = cx.vault.write(&n, &new) {
                out.push(Out::Toast(format!("Não foi possível guardar o grifo: {e}")));
            }
            self.note_mtime = mtime(&abs);
        }
    }

    /// The selection's place: its start (with the quote) and its end.
    fn sel_place(&self) -> Option<(Locator, usize, String)> {
        let s = self.sel.as_ref()?;
        let info = self.infos.get(&(s.chap, s.page, self.view_key.clone()))?;
        let t = info.text.clone()?;
        let (a, b) = (s.a.min(s.b), s.a.max(s.b));
        let text = t.text(a, b);
        let quote = t.quote(a, b);
        let loc = if self.reflows() {
            let ga = t.glyphs.get(a)?;
            let gb = t.glyphs.get(b)?;
            (Locator { file: self.file.clone(), chap: Some(s.chap), offset: Some(ga.off), quote: Some(quote), ..Default::default() }, gb.off)
        } else {
            (Locator { file: self.file.clone(), page: Some(s.page + 1), offset: Some(a), quote: Some(quote), ..Default::default() }, b)
        };
        Some((loc.0, loc.1, text))
    }

    /// The selection highlighted in a colour (or a highlight recoloured).
    pub(super) fn highlight(&mut self, cx: &Cx, color: &str, out: &mut Vec<Out>) -> Option<String> {
        if let Some(id) = self.sel.as_ref().and_then(|s| s.mark.clone()) {
            if let Some(m) = self.marks.iter_mut().find(|m| m.id == id) {
                m.color = color.to_string();
            }
            self.write_marks(cx, out);
            return Some(id);
        }
        let (loc, end, text) = self.sel_place()?;
        let label = {
            let s = self.sel.as_ref()?;
            self.place_label(s.chap, s.page)
        };
        let m = Mark::new(loc, end, color, &text, &label);
        let id = m.id.clone();
        self.marks.push(m);
        self.write_marks(cx, out);
        if let Some(s) = self.sel.as_mut() {
            s.mark = Some(id.clone());
        }
        Some(id)
    }

    pub(super) fn unmark(&mut self, cx: &Cx, id: &str, out: &mut Vec<Out>) {
        self.marks.retain(|m| m.id != id);
        self.write_marks(cx, out);
        self.sel = None;
    }

    pub(super) fn set_mark_note(&mut self, cx: &Cx, id: &str, note: &str, out: &mut Vec<Out>) {
        if let Some(m) = self.marks.iter_mut().find(|m| m.id == id) {
            m.note = note.trim().to_string();
        }
        self.write_marks(cx, out);
    }

    fn source(&mut self, cx: &Cx, loc: Locator, out: &mut Vec<Out>) -> SourceRef {
        let meta = self.meta();
        let note = self.ensure_note(cx, out);
        SourceRef { title: meta.title, author: (!meta.author.is_empty()).then_some(meta.author), book: note.map(|n| n.trim_end_matches(".md").to_string()), url: None, locator: Some(loc.to_json()) }
    }

    /// The selection as an excerpt in `Trechos/`: (its note, the excerpt).
    pub(super) fn keep_selection(&mut self, cx: &Cx, out: &mut Vec<Out>) -> Option<(String, Snippet)> {
        let (loc, _, text) = self.sel_place()?;
        let source = self.source(cx, loc, out);
        let s = Snippet::new(&text, source, "leitor");
        match capture::save(&cx.vault.root, capture::FOLDER, &s) {
            Ok(note) => {
                out.push(Out::Rescan);
                Some((note, s))
            }
            Err(e) => {
                out.push(Out::Toast(format!("Não foi possível guardar o trecho: {e}")));
                None
            }
        }
    }

    /// The folder of this book's cuts, and the cut's file name.
    fn cut_path(&self, cx: &Cx, page: usize, ext: &str) -> String {
        let title = marks::clean_name(&self.meta().title);
        let dir = format!("{CUTS}/{title}");
        let stem = format!("{} p{}", title.to_lowercase(), page + 1);
        let free = capture::free_name(&cx.vault.abs(&dir), &format!("{stem}-1.{ext}"));
        format!("{dir}/{free}")
    }

    /// The cut (its PNG at print resolution) as a picture and an excerpt;
    /// the text inside the region goes with it as a transcription.
    pub(super) fn keep_cut(&mut self, cx: &Cx, out: &mut Vec<Out>) -> Option<(String, Snippet)> {
        let c = self.cut.clone()?;
        let png = c.png.clone()?;
        let rel = self.cut_path(cx, c.page, "png");
        let abs = cx.vault.abs(&rel);
        if let Some(d) = abs.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&abs, &png) {
            out.push(Out::Toast(format!("Não foi possível guardar o recorte: {e}")));
            return None;
        }
        let rect = c.rect();
        let inside = self.infos.get(&(c.chap, c.page, self.view_key.clone())).and_then(|i| i.text.clone()).and_then(|t| t.inside(rect).map(|(a, b)| t.text(a, b)));
        let mut loc = if self.reflows() {
            let start = self.infos.get(&(c.chap, c.page, self.view_key.clone())).and_then(|i| i.text.clone()).and_then(|t| t.inside(rect).and_then(|(a, _)| t.glyphs.get(a).map(|g| g.off)));
            let base = self.infos.get(&(c.chap, c.page, self.view_key.clone())).map(|i| i.loc.clone()).unwrap_or_default();
            Locator { chap: Some(c.chap), offset: start.or(base.offset), quote: base.quote, ..Default::default() }
        } else {
            Locator { page: Some(c.page + 1), ..Default::default() }
        };
        loc.file = self.file.clone();
        loc.rect = Some(rect);
        let source = self.source(cx, loc, out);
        let mut s = Snippet::new("", source, "leitor");
        s.image = Some(rel);
        if let Some(t) = inside.filter(|t| !t.trim().is_empty()) {
            s.note = Some(format!("Transcrição: {t}"));
        }
        match capture::save(&cx.vault.root, capture::FOLDER, &s) {
            Ok(note) => {
                out.push(Out::Rescan);
                Some((note, s))
            }
            Err(e) => {
                out.push(Out::Toast(format!("Não foi possível guardar o recorte: {e}")));
                None
            }
        }
    }

    /// A picture of the book sent whole (its original file).
    pub(super) fn keep_picture(&mut self, cx: &Cx, name: &str, bytes: &[u8], out: &mut Vec<Out>) {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")).unwrap_or_else(|| "png".into());
        let rel = self.cut_path(cx, self.page, &ext);
        let abs = cx.vault.abs(&rel);
        if let Some(d) = abs.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&abs, bytes) {
            out.push(Out::Toast(format!("Não foi possível guardar a imagem: {e}")));
            return;
        }
        let mut loc = self.infos.get(&(self.chap, self.page, self.view_key.clone())).map(|i| i.loc.clone()).unwrap_or_default();
        loc.file = self.file.clone();
        if self.reflows() {
            loc.chap = Some(self.chap);
        }
        let source = self.source(cx, loc, out);
        let mut s = Snippet::new("", source, "leitor");
        s.image = Some(rel);
        match capture::save(&cx.vault.root, capture::FOLDER, &s) {
            Ok(note) => {
                out.push(Out::Rescan);
                out.push(Out::Snippet { note, snippet: s });
            }
            Err(e) => out.push(Out::Toast(format!("Não foi possível guardar o trecho: {e}"))),
        }
    }
}

fn mtime(p: &std::path::Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}
