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
            "cast" | "enredo" | "mundo" | "trilha" | "layout" | "capa" => {
                if let Some(b) = book {
                    self.open(&format!("scriptorium:{view}:{b}"), true);
                }
            }
            "mapas" => {
                if let Some(b) = book {
                    self.open(&format!("scriptorium:mundo:{b}"), true);
                    if let Some(crate::sc::View::Mundo(m)) = self.tabs.get_mut(self.active).and_then(|t| t.view.as_mut()) {
                        m.aba = crate::sc::mundo_ui::Aba::Mapas;
                    }
                }
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
            Action::Export(b) => self.open_export(&b),
            Action::Pick(p) => self.picker = Some(*p),
            Action::Saved(p) => {
                if let Some(v) = &self.vault {
                    v.index.write().unwrap().update(&[p]);
                }
            }
            Action::Rename(p, name) => {
                self.save_all();
                self.rename(&p, &name);
                let to = format!("{}{name}.md", p.rsplit_once('/').map(|(d, _)| format!("{d}/")).unwrap_or_default());
                if let Some(t) = self.tabs.iter_mut().find(|t| t.path == to) {
                    if let Some(crate::sc::View::Sheet(s)) = t.view.as_mut() {
                        s.path = to.clone();
                    }
                }
            }
            Action::AsMarkdown(p) => {
                self.save_all();
                if let Some(i) = self.tabs.iter().position(|t| t.path == p) {
                    let text = self.vault.as_ref().and_then(|v| std::fs::read_to_string(v.abs(&p)).ok()).unwrap_or_default();
                    let t = &mut self.tabs[i];
                    t.view = None;
                    t.text = text.clone();
                    t.saved = text;
                    t.mode = Mode::Edit;
                }
            }
            Action::CreateSheetFor { sheet, list, idx, name } => {
                let made = {
                    let (Some(v), Some(sc)) = (self.vault.as_ref(), self.sc.as_ref()) else { return };
                    let ix = v.index.read().unwrap();
                    book::book_for(&ix, &sheet, &sc.author()).map(|b| book::create_character(&v.root, &ix, &b, &name))
                };
                match made {
                    Some(Ok(path)) => {
                        self.written(std::slice::from_ref(&path));
                        let base = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".md").to_string();
                        if let Some(crate::sc::View::Sheet(s)) = self.tabs.iter_mut().find(|t| t.path == sheet).and_then(|t| t.view.as_mut()) {
                            let mut l = s.sheet.list(&list);
                            if idx < l.len() {
                                l[idx] = json!(format!("[[{base}]]"));
                            }
                            s.sheet.fm.insert(list, Json::Array(l));
                            s.changed();
                        }
                        self.open(&path, true);
                    }
                    Some(Err(e)) => self.toast(format!("Não foi possível criar a ficha: {e}")),
                    None => self.toast("Esta ficha não pertence a um livro."),
                }
            }
            Action::Toast(t) => self.toast(t),
            Action::Music(cmd) => {
                let (Some(v), Some(sc)) = (&self.vault, &self.sc) else { return };
                let ix = v.index.read().unwrap();
                let resolve = |t: &str, s: &str| v.resolve(t, s);
                self.music.run(cmd, &v.root, &ix, &sc.author(), &resolve);
            }
            Action::OpenLink(t, source, nt) => {
                let target = self.vault.as_ref().and_then(|v| v.resolve(t.split('#').next().unwrap_or(&t), &source));
                match target {
                    Some(p) => self.open(&p, nt),
                    None => self.open_link(&t, nt),
                }
            }
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

    /// Today's words (for sprints and the daily goal).
    fn words_today(&self) -> i64 {
        self.sc.as_ref().map(|s| s.settings.today_words(None)).unwrap_or(0)
    }

    pub fn toggle_focus(&mut self) {
        if self.focus.take().is_some() {
            (self.left_open, self.right_open) = self.focus_restore;
            crate::sc::focus::DIM.with(|d| d.set(false));
            crate::sc::focus::TYPEWRITER.with(|d| d.set(false));
            crate::sc::focus::ON.with(|d| d.set(false));
            return;
        }
        self.focus_restore = (self.left_open, self.right_open);
        self.focus = Some(crate::sc::focus::Focus::new());
        crate::sc::focus::ON.with(|d| d.set(true));
    }

    pub fn focus_hud(&mut self, ctx: &eframe::egui::Context) {
        let p = self.palette;
        let (dim, tw, goal) = match self.sc.as_ref() {
            Some(sc) => (sc.settings.data["esmaecer"].as_bool().unwrap_or(false), sc.settings.data["maquinaDeEscrever"].as_bool().unwrap_or(true), sc.settings.daily_goal()),
            None => (false, true, 0),
        };
        crate::sc::focus::DIM.with(|d| d.set(dim));
        crate::sc::focus::TYPEWRITER.with(|d| d.set(tw));
        let active = self.tabs.get(self.active).filter(|t| t.view.is_none() && t.board.is_none()).map(|t| (t.path.clone(), ateliet_core::scriptorium::count_words(&t.text) as i64));
        let is_chapter = active.as_ref().is_some_and(|(p, _)| self.vault.as_ref().is_some_and(|v| ateliet_core::scriptorium::book::is_chapter(&v.index.read().unwrap(), p)));
        let tabs: Vec<(String, i64)> = self.tabs.iter().map(|t| (t.path.clone(), ateliet_core::scriptorium::count_words(&t.text) as i64)).collect();
        let root = self.vault.as_ref().map(|v| v.root.clone());
        let today = self.words_today();
        let words_now = today;
        let Some(f) = self.focus.as_mut() else { return };
        if let Some((path, w)) = &active {
            f.see(path, *w);
        }
        let live = |path: &str| -> Option<i64> {
            tabs.iter().find(|(p, _)| p == path).map(|(_, w)| *w).or_else(|| root.as_ref().and_then(|r| std::fs::read_to_string(r.join(path)).ok()).map(|t| ateliet_core::scriptorium::count_words(&t) as i64))
        };
        let session = f.session_words(&live);
        let chapter = if is_chapter { active.as_ref().map(|(_, w)| *w) } else { None };
        let act = crate::sc::focus::hud(ctx, p, session, chapter, today, goal, dim, tw, self.sprint.as_ref(), words_now, f);
        match act {
            Some(crate::sc::focus::HudAction::Exit) => {
                let mins = f.started.elapsed().as_secs() / 60;
                self.toggle_focus();
                if session > 0 || mins >= 5 {
                    self.toast(format!("Sessão de foco: {} palavras em {mins} min.", ateliet_core::scriptorium::fmt_n(session)));
                }
            }
            Some(crate::sc::focus::HudAction::ToggleDim) => {
                if let Some(sc) = self.sc.as_mut() {
                    sc.settings.set("esmaecer", json!(!dim));
                }
            }
            Some(crate::sc::focus::HudAction::ToggleTypewriter) => {
                if let Some(sc) = self.sc.as_mut() {
                    sc.settings.set("maquinaDeEscrever", json!(!tw));
                }
            }
            Some(crate::sc::focus::HudAction::StartSprint(m, g)) => self.sprint = Some(crate::sc::focus::Sprint::new(m, g, words_now)),
            Some(crate::sc::focus::HudAction::PauseSprint) => {
                if let Some(s) = self.sprint.as_mut() {
                    s.toggle_pause();
                }
            }
            Some(crate::sc::focus::HudAction::StopSprint) => self.finish_sprint(true),
            None => {}
        }
    }

    pub fn finish_sprint(&mut self, early: bool) {
        let words = self.words_today();
        let Some(s) = self.sprint.take() else { return };
        let rec = s.record(words, early);
        let gained = s.gained(words);
        if let Some(sc) = self.sc.as_mut() {
            let mut list = sc.settings.data["sprints"].as_array().cloned().unwrap_or_default();
            list.push(rec);
            let n = list.len();
            sc.settings.set("sprints", json!(list[n.saturating_sub(200)..].to_vec()));
        }
        let goal = if s.goal > 0 { format!(" de {}", ateliet_core::scriptorium::fmt_n(s.goal)) } else { String::new() };
        let hit = if s.goal > 0 && gained >= s.goal { " — meta batida!" } else { "" };
        self.toast(format!("Sprint {}: {} palavras{goal}{hit}", if early { "encerrado" } else { "concluído" }, ateliet_core::scriptorium::fmt_n(gained)));
    }

    pub fn sprint_tick(&mut self) {
        if self.sprint.as_ref().is_some_and(|s| s.done()) {
            self.finish_sprint(false);
        }
    }

    /// The music: what plays, the diary of the chapter being written, the app's answers.
    pub fn music_tick(&mut self, ctx: &eframe::egui::Context) {
        if let Some(sc) = self.sc.as_mut() {
            if !self.music.app_ok && sc.settings.data["musicaApp"] == "ok" {
                self.music.app_ok = true;
            } else if self.music.app_ok && sc.settings.data["musicaApp"] != "ok" {
                sc.settings.set("musicaApp", json!("ok"));
            }
            self.music.diary = sc.settings.data["diarioTrilha"] != false;
        }
        let (Some(v), Some(sc)) = (&self.vault, &self.sc) else { return };
        // a chapter being written
        if let Some(t) = self.tabs.get(self.active).filter(|t| t.view.is_none()) {
            if let Some(e) = t.last_edit.filter(|e| self.music.last_edit.is_none_or(|m| *e > m)) {
                let is_ch = v.index.read().unwrap().page(&t.path).is_some_and(|p| book::kind(p) == "capitulo");
                if is_ch {
                    self.music.last_edit = Some(e);
                    self.music.last_chapter = Some(t.path.clone());
                }
            }
        }
        {
            let ix = v.index.read().unwrap();
            self.music.tick(ctx, &v.root, &ix, &sc.author());
        }
        let written = std::mem::take(&mut self.music.written);
        if !written.is_empty() {
            v.index.write().unwrap().update(&written);
        }
        for t in std::mem::take(&mut self.music.toasts) {
            self.toast(t);
        }
        if let Some((b, list)) = self.music.playlists.take() {
            let items = list.into_iter().map(|(id, name)| (name.clone(), format!("{id}\t{name}"), String::new())).collect();
            self.picker = Some(crate::sc::picker::Picker::new("Qual playlist é a trilha deste livro?", items, crate::sc::picker::Purpose::Playlist(b), crate::icons::MUSIC));
        }
    }

    /// "Escrever ouvindo…": pick a track of the current book (themes of this chapter first).
    pub fn write_with(&mut self) {
        let Some(b) = self.current_book() else {
            self.toast("Abra um capítulo ou escolha um livro na Biblioteca.");
            return;
        };
        let (Some(v), Some(_)) = (&self.vault, &self.sc) else { return };
        let data = ateliet_core::scriptorium::trilha::load(&v.root, &b);
        let ch = self.active_path().filter(|p| v.index.read().unwrap().page(p).is_some_and(|pg| book::kind(pg) == "capitulo"));
        let fm = ch.as_ref().and_then(|c| v.index.read().unwrap().page(c).map(|pg| pg.yaml.clone())).unwrap_or(Json::Null);
        let themes = ch.as_ref().map(|c| ateliet_core::scriptorium::trilha::theme_keys_for_chapter(&data, c, &fm)).unwrap_or_default();
        let mut items: Vec<(String, String, String)> = data["faixas"].as_array().into_iter().flatten().map(|t| {
            let k = t["k"].as_str().unwrap_or("").to_string();
            let star = if themes.contains(&k) { "★ " } else { "" };
            (format!("{star}{}", t["nome"].as_str().unwrap_or("")), k, t["artista"].as_str().unwrap_or("").to_string())
        }).collect();
        if items.is_empty() {
            self.toast("A trilha deste livro está vazia: adicione faixas na tela Trilha.");
            self.open_view("trilha", Some(b.path));
            return;
        }
        items.sort_by_key(|x| !x.0.starts_with('★'));
        let title = if ch.is_some() { "Escrever este capítulo ouvindo… (★ = tema dele)" } else { "Tocar…" };
        self.picker = Some(crate::sc::picker::Picker::new(title, items, crate::sc::picker::Purpose::WriteWith(b.path, ch), crate::icons::MUSIC));
    }

    pub fn picker_ui(&mut self, ctx: &eframe::egui::Context) {
        let p = self.palette;
        let Some(pk) = self.picker.as_mut() else { return };
        let (chosen, close) = pk.show(ctx, p);
        if close {
            let pk = self.picker.take().unwrap();
            if let Some(v) = chosen {
                let target = match &pk.purpose {
                    crate::sc::picker::Purpose::Portrait(s) | crate::sc::picker::Purpose::Gallery(s) | crate::sc::picker::Purpose::Person(s, _) => s.clone(),
                    crate::sc::picker::Purpose::Playlist(b) => {
                        let (pid, name) = v.split_once('\t').unwrap_or((v.as_str(), ""));
                        if let (Some(vault), Some(sc)) = (&self.vault, &self.sc) {
                            let ix = vault.index.read().unwrap();
                            if let Some(book) = book::book_by_path(&ix, b, &sc.author()) {
                                self.music.import(&vault.root, &ix, &book, pid, name);
                            }
                        }
                        return;
                    }
                    crate::sc::picker::Purpose::WriteWith(b, ch) => {
                        let (b, ch) = (b.clone(), ch.clone());
                        self.sc_apply(Action::Music(crate::music::Cmd::Play(b, v, ch)));
                        return;
                    }
                };
                if let Some(crate::sc::View::Sheet(s)) = self.tabs.iter_mut().find(|t| t.path == target).and_then(|t| t.view.as_mut()) {
                    s.picked(&pk.purpose, &v);
                }
            }
        }
    }

    pub fn open_export(&mut self, b: &str) {
        let (Some(v), Some(sc)) = (self.vault.as_ref(), self.sc.as_ref()) else { return };
        let title = book::book_by_path(&v.index.read().unwrap(), b, &sc.author()).map(|x| x.title).unwrap_or_default();
        let last: Vec<String> = sc.settings.data["ultimosFormatos"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).map(|x| if x == "epub" { "epub".to_string() } else { x.to_string() }).collect()).unwrap_or_default();
        self.export = Some(crate::sc::export_ui::ExportDialog::new(b, &title, &last));
    }

    pub fn export_ui(&mut self, ctx: &eframe::egui::Context) {
        let p = self.palette;
        let (Some(d), Some(v), Some(sc)) = (self.export.as_mut(), self.vault.as_ref(), self.sc.as_mut()) else { return };
        let opts = ateliet_core::scriptorium::export::Options {
            typography: sc.settings.data["tipografia"].as_bool().unwrap_or(true),
            soft_break: sc.settings.str("quebraSimples", "paragrafo"),
            author: sc.author(),
        };
        let (open, remember) = d.show(ctx, p, v.root.clone(), v.index.clone(), opts);
        if let Some(f) = remember {
            sc.settings.set("ultimosFormatos", json!(f));
            if let Some(v) = self.vault.as_mut() {
                v.rescan();
            }
        }
        if !open {
            self.export = None;
        }
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
