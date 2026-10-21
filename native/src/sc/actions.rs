//! What the Scriptorium views ask for, carried out by the app.

use super::Action;
use crate::app::{App, Mode};
use crate::form::{Field, Form, Purpose};
use ateliet_core::scriptorium::book::{self, CHAPTER_TYPES};
use ateliet_core::scriptorium::{safe_name, set_frontmatter};
use serde_json::{json, Value as Json};
use std::collections::HashMap;

impl App {
    /// Change a note's text: in its tab when it is open, else on disk.
    pub fn edit_file(&mut self, path: &str, f: impl FnOnce(&str) -> String) {
        if let Some(i) = self.tabs.iter().position(|t| t.path == path && t.view.is_none()) {
            let t = &mut self.tabs[i];
            t.text = f(&t.text);
            t.live.close();
            self.save_tab(i);
        } else if let Some(v) = &self.vault {
            match std::fs::read_to_string(v.abs(path)) {
                Ok(text) => {
                    if let Err(e) = v.write(path, &f(&text)) {
                        self.toast(format!("Não foi possível salvar {path}: {e}"));
                    }
                }
                Err(e) => self.toast(format!("Não foi possível ler {path}: {e}")),
            }
        }
        if let Some(v) = &self.vault {
            v.index.write().unwrap().update(&[path.to_string()]);
        }
    }

    fn set_fm(&mut self, path: &str, key: &str, value: Option<Json>) {
        self.edit_file(path, |t| set_frontmatter(t, key, value.as_ref()));
    }

    fn written(&mut self, paths: &[String]) {
        if let Some(v) = &self.vault {
            v.index.write().unwrap().update(paths);
        }
        if let Some(v) = self.vault.as_mut() {
            v.rescan();
        }
    }

    pub fn open_view(&mut self, view: &str, book: Option<String>) {
        if let (Some(b), Some(sc)) = (&book, self.sc.as_mut()) {
            sc.settings.set("ultimoLivro", json!(b));
        }
        match view {
            "binder" => {
                self.right_open = true;
                self.right_tab = crate::app::RightTab::Manuscript;
                self.binder.book = book;
            }
            _ => self.open(&format!("scriptorium:{view}"), true),
        }
    }

    pub fn current_book(&mut self) -> Option<book::Book> {
        let active = self.active_path();
        let v = self.vault.as_ref()?;
        let ix = v.index.read().unwrap();
        self.sc.as_mut()?.current_book(&ix, active.as_deref())
    }

    pub fn new_book_form(&mut self) {
        let author = self.sc.as_ref().map(|s| s.author()).unwrap_or_default();
        let folder = self.sc.as_ref().map(|s| s.settings.books_folder()).unwrap_or_else(|| "Livros".into());
        self.form = Some(Form::new(
            "Novo livro",
            Purpose::NewBook,
            "Criar",
            vec![
                Field::text("titulo", "Título", "").hint("Ex.: A Torre de Vidro"),
                Field::text("subtitulo", "Subtítulo", ""),
                Field::text("autor", "Autor", &author),
                Field::text("serie", "Série (opcional)", ""),
                Field::text("volume", "Volume", ""),
                Field::text("genero", "Gênero", "").hint("Ex.: Fantasia"),
                Field::text("meta", "Meta de palavras", "80000"),
                Field::text("prazo", "Prazo (AAAA-MM-DD)", ""),
                Field::area("sinopse", "Sinopse", ""),
                Field::text("pasta", "Pasta", &folder),
            ],
        ));
    }

    fn chapter_form(&mut self, book: &str) {
        let parts: Vec<String> = {
            let Some(v) = &self.vault else { return };
            let ix = v.index.read().unwrap();
            let author = self.sc.as_ref().map(|s| s.author()).unwrap_or_default();
            match book::book_by_path(&ix, book, &author) {
                Some(b) => book::chapters(&ix, &b).into_iter().map(|c| c.part).filter(|p| !p.is_empty()).collect(),
                None => return,
            }
        };
        self.form = Some(Form::new(
            "Novo capítulo",
            Purpose::NewChapter { book: book.to_string() },
            "Criar",
            vec![
                Field::text("titulo", "Título", "").hint("Ex.: Preparativos"),
                Field::text("parte", "Parte / arco (opcional)", parts.last().map(String::as_str).unwrap_or("")).hint("Ex.: Ato Um"),
                Field::select("tipo", "Tipo", "capitulo", &CHAPTER_TYPES),
            ],
        ));
    }

    pub fn sc_apply(&mut self, a: Action) {
        match a {
            Action::Open(p, new_tab) => self.open(&p, new_tab),
            Action::NewBook => self.new_book_form(),
            Action::NewChapter(b) => self.chapter_form(&b),
            Action::NewCharacter(b) => self.form = Some(Form::new("Novo personagem", Purpose::NewCharacter { book: b }, "Criar", vec![Field::text("nome", "Nome", "")])),
            Action::View(v, b) => self.open_view(&v, b),
            Action::SelectBook(b) => {
                if let Some(sc) = self.sc.as_mut() {
                    sc.settings.set("ultimoLivro", json!(b));
                }
            }
            Action::SetFm(p, k, v) => self.set_fm(&p, &k, v),
            Action::Reorder(files) => {
                for (i, f) in files.iter().enumerate() {
                    let n = i as i64 + 1;
                    let current = self.vault.as_ref().and_then(|v| v.index.read().unwrap().page(f).and_then(|p| p.yaml.get("ordem").and_then(|o| o.as_f64())));
                    if current != Some(n as f64) {
                        self.set_fm(f, "ordem", Some(json!(n)));
                    }
                }
            }
            Action::RenameChapter(p, title) => self.form = Some(Form::new("Renomear capítulo", Purpose::RenameChapter { path: p }, "Salvar", vec![Field::text("t", "Título", &title)])),
            Action::ChapterPart(p, part) => self.form = Some(Form::new("Parte / arco", Purpose::ChapterPart { path: p }, "Salvar", vec![Field::text("p", "Nome da parte (vazio para nenhuma)", &part)])),
            Action::Trash(p) => {
                let name = crate::vault::basename(&p).to_string();
                self.form = Some(Form::new("Mover capítulo para a lixeira?", Purpose::ConfirmTrash { path: p }, "Mover", vec![]).with_note(&format!("\"{name}\" vai para a lixeira (pode ser recuperado).")));
            }
            Action::Continue(b) => self.continue_book(&b),
        }
    }

    /// Open the chapter written last, cursor at its end, with the manuscript panel.
    pub fn continue_book(&mut self, b: &str) {
        let target = {
            let (Some(v), Some(sc)) = (self.vault.as_ref(), self.sc.as_mut()) else { return };
            let ix = v.index.read().unwrap();
            let author = sc.author();
            let Some(book) = book::book_by_path(&ix, b, &author) else { return };
            sc.settings.set("ultimoLivro", json!(b));
            let st = sc.stats(&ix, &book);
            st.last_edited.clone().or_else(|| st.per.first().map(|c| c.info.path.clone())).unwrap_or_else(|| b.to_string())
        };
        self.open(&target, false);
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.mode = Mode::Edit;
            t.cursor = t.text.chars().count();
        }
        self.open_view("binder", Some(b.to_string()));
    }

    pub fn form_ui(&mut self, ctx: &eframe::egui::Context) {
        let p = self.palette;
        let Some(form) = self.form.as_mut() else { return };
        form.palette = p;
        let (values, close) = form.show(ctx);
        if close {
            let form = self.form.take().unwrap();
            if let Some(v) = values {
                self.form_done(form.purpose, v);
            }
        }
    }

    fn form_done(&mut self, purpose: Purpose, v: HashMap<String, String>) {
        let g = |k: &str| v.get(k).map(|s| s.trim().to_string()).unwrap_or_default();
        let Some(root) = self.vault.as_ref().map(|x| x.root.clone()) else { return };
        let author = self.sc.as_ref().map(|s| s.author()).unwrap_or_default();
        match purpose {
            Purpose::NewBook => {
                let nb = book::NewBook { title: g("titulo"), subtitle: g("subtitulo"), author: g("autor"), serie: g("serie"), volume: g("volume"), genre: g("genero"), goal: g("meta").parse().unwrap_or(80000), deadline: g("prazo"), synopsis: g("sinopse"), folder: g("pasta") };
                match book::create_book(&root, &nb) {
                    Ok(paths) => {
                        self.written(&paths);
                        if let Some(sc) = self.sc.as_mut() {
                            sc.settings.set("ultimoLivro", json!(paths[0]));
                        }
                        self.open("scriptorium:library", true);
                        if let Some(crate::sc::View::Library(l)) = self.tabs.get_mut(self.active).and_then(|t| t.view.as_mut()) {
                            l.selected = Some(paths[0].clone());
                        }
                        self.toast(format!("Livro criado em {}", crate::vault::parent(&paths[0])));
                    }
                    Err(e) => self.toast(format!("Não foi possível criar o livro: {e}")),
                }
            }
            Purpose::NewChapter { book: b } => {
                let res = {
                    let Some(vt) = &self.vault else { return };
                    let ix = vt.index.read().unwrap();
                    match book::book_by_path(&ix, &b, &author) {
                        Some(bk) => book::create_chapter(&root, &ix, &bk, &g("titulo"), &g("parte"), &g("tipo")),
                        None => Err("livro não encontrado".into()),
                    }
                };
                match res {
                    Ok(p) => {
                        self.written(std::slice::from_ref(&p));
                        self.open(&p, false);
                        if let Some(t) = self.tabs.get_mut(self.active) {
                            t.mode = Mode::Edit;
                        }
                    }
                    Err(e) => self.toast(format!("Não foi possível criar o capítulo: {e}")),
                }
            }
            Purpose::NewCharacter { book: b } => {
                if g("nome").is_empty() {
                    return;
                }
                let res = {
                    let Some(vt) = &self.vault else { return };
                    let ix = vt.index.read().unwrap();
                    match book::book_by_path(&ix, &b, &author) {
                        Some(bk) => book::create_character(&root, &ix, &bk, &g("nome")),
                        None => Err("livro não encontrado".into()),
                    }
                };
                match res {
                    Ok(p) => {
                        self.written(std::slice::from_ref(&p));
                        self.open(&p, true);
                    }
                    Err(e) => self.toast(format!("Não foi possível criar o personagem: {e}")),
                }
            }
            Purpose::RenameChapter { path } => {
                let t = g("t");
                if t.is_empty() {
                    return;
                }
                self.set_fm(&path, "titulo", Some(json!(t)));
                let name = crate::vault::basename(&path).to_string();
                let num: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
                let new = format!("{}{}", if num.is_empty() { String::new() } else { format!("{num} — ") }, safe_name(&t));
                if new != name {
                    self.rename(&path, &new);
                }
            }
            Purpose::ChapterPart { path } => {
                let p = g("p");
                self.set_fm(&path, "parte", if p.is_empty() { None } else { Some(json!(p)) });
            }
            Purpose::ConfirmTrash { path } => self.delete(&path),
        }
    }
}
