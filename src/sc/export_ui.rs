//! "Exportar livro": pick formats, run in the background, show where the
//! files went.

use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book;
use ateliet_core::scriptorium::export::{self, Options};
use eframe::egui::{self, RichText, Sense};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, RwLock};

pub const FORMATS: [(&str, &str, &str, &str); 6] = [
    ("pdf-impressao", "PDF para impressão", "Com sangria e páginas espelhadas, para a gráfica ou a KDP", icons::PRINTER),
    ("pdf-tela", "PDF de tela", "Para ler no computador ou mandar a leitores beta", icons::MONITOR),
    ("epub", "EPUB", "Leitores de e-book, Kindle (via Send to Kindle), Apple Books", icons::TABLET),
    ("docx", "Word (DOCX)", "Para revisão, editoras e concursos", icons::FILE_TEXT),
    ("md", "Markdown", "Um arquivo portátil com as imagens ao lado", icons::HASH),
    ("web", "Leitor web", "Uma página HTML que se lê bem no celular", icons::GLOBE),
];

pub struct ExportDialog {
    pub book: String,
    pub title: String,
    pub picked: [bool; 6],
    running: Option<Receiver<Vec<(String, Result<PathBuf, String>)>>>,
    results: Vec<(String, Result<PathBuf, String>)>,
}

impl ExportDialog {
    pub fn new(book: &str, title: &str, last: &[String]) -> ExportDialog {
        let mut picked = [false; 6];
        for (i, f) in FORMATS.iter().enumerate() {
            picked[i] = last.iter().any(|l| l == f.0 || (f.0 == "epub" && l == "epub"));
        }
        if !picked.iter().any(|p| *p) {
            picked[2] = true;
        }
        ExportDialog { book: book.into(), title: title.into(), picked, running: None, results: Vec::new() }
    }

    fn start(&mut self, ctx: &egui::Context, root: PathBuf, index: Arc<RwLock<Index>>, opts: Options) {
        let (tx, rx) = mpsc::channel();
        let formats: Vec<&'static str> = FORMATS.iter().zip(self.picked).filter(|(_, p)| *p).map(|(f, _)| f.0).collect();
        let path = self.book.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let ix = index.read().unwrap();
            let mut out = Vec::new();
            let Some(b) = book::book_by_path(&ix, &path, &opts.author) else {
                let _ = tx.send(vec![("livro".into(), Err("o livro não foi encontrado".into()))]);
                return;
            };
            for f in formats {
                let r = match f {
                    "epub" => export::epub(&root, &ix, &b, &opts),
                    "docx" => ateliet_core::scriptorium::docx::docx(&root, &ix, &b, &opts),
                    "md" => export::markdown(&root, &ix, &b, &opts),
                    "pdf-impressao" | "pdf-tela" => ateliet_core::typeset::book_pdf(&root, &ix, &b, &opts, &crate::fonts::book_fonts(), f == "pdf-impressao"),
                    _ => export::web(&root, &ix, &b, &opts),
                };
                out.push((f.to_string(), r));
            }
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        self.running = Some(rx);
        self.results.clear();
    }

    /// Returns (keep open, formats to remember when an export finished).
    pub fn show(&mut self, ctx: &egui::Context, p: Palette, root: PathBuf, index: Arc<RwLock<Index>>, opts: Options) -> (bool, Option<Vec<String>>) {
        let mut open = true;
        let mut remember = None;
        if let Some(rx) = &self.running {
            if let Ok(r) = rx.try_recv() {
                self.results = r;
                self.running = None;
                remember = Some(FORMATS.iter().zip(self.picked).filter(|(_, p)| *p).map(|(f, _)| f.0.to_string()).collect());
            }
        }
        kit::scrim(ctx, p);
        let mut go = false;
        egui::Window::new("export-dialog").title_bar(false).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 100.0]).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Exportar livro").font(kit::medium(16.0)).color(p.text));
                    ui.label(RichText::new(&self.title).color(p.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if kit::icon_button(ui, icons::X, "Fechar (Esc)", false, p).clicked() {
                        open = false;
                    }
                });
            });
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            for (i, (_, name, desc, glyph)) in FORMATS.iter().enumerate() {
                let w = ui.available_width();
                let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 54.0), Sense::click());
                let on = self.picked[i];
                ui.painter().rect(rect, 10.0, if on { p.accent_soft } else if r.hovered() { p.bg_hover } else { p.bg }, egui::Stroke::new(1.0_f32, if on { p.accent_strong } else { p.border_strong }), egui::StrokeKind::Inside);
                ui.painter().text(egui::pos2(rect.left() + 26.0, rect.center().y), egui::Align2::CENTER_CENTER, *glyph, kit::icon_font(17.0), if on { p.accent_strong } else { p.muted });
                ui.painter().text(egui::pos2(rect.left() + 50.0, rect.top() + 18.0), egui::Align2::LEFT_CENTER, *name, kit::medium(13.5), p.text);
                ui.painter().text(egui::pos2(rect.left() + 50.0, rect.top() + 36.0), egui::Align2::LEFT_CENTER, *desc, egui::FontId::proportional(12.0), p.faint);
                let c = egui::pos2(rect.right() - 22.0, rect.center().y);
                if on {
                    ui.painter().circle_filled(c, 9.0, p.accent_strong);
                    ui.painter().text(c, egui::Align2::CENTER_CENTER, icons::CHECK, kit::icon_font(11.0), p.on_accent);
                } else {
                    ui.painter().circle_stroke(c, 8.5, egui::Stroke::new(1.0_f32, p.border_strong));
                }
                if r.clicked() && self.running.is_none() {
                    self.picked[i] = !on;
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new("PDF para impressão e capa ainda saem pela versão web do Ateliet.").size(11.5).color(p.faint));
            if !self.results.is_empty() {
                ui.add_space(10.0);
                kit::hairline(ui, p);
                ui.add_space(10.0);
                for (f, r) in &self.results {
                    let name = FORMATS.iter().find(|x| x.0 == f).map(|x| x.1).unwrap_or(f);
                    ui.horizontal(|ui| match r {
                        Ok(path) => {
                            ui.label(kit::icon(icons::CHECK, 13.0).color(p.accent_strong));
                            ui.label(RichText::new(name).font(kit::medium(13.0)).color(p.text));
                            ui.label(RichText::new(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).size(12.0).color(p.faint));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if kit::icon_button_sized(ui, icons::FOLDER_OPEN, "Mostrar na pasta", false, p, 24.0, 13.0).clicked() {
                                    let _ = open::that(path.parent().unwrap_or(path));
                                }
                                if kit::icon_button_sized(ui, icons::EXTERNAL_LINK, "Abrir", false, p, 24.0, 13.0).clicked() {
                                    let _ = open::that(path);
                                }
                            });
                        }
                        Err(e) => {
                            ui.label(kit::icon(icons::TRIANGLE_ALERT, 13.0).color(p.error));
                            ui.label(RichText::new(format!("{name}: {e}")).size(12.5).color(p.error));
                        }
                    });
                }
            }
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let n = self.picked.iter().filter(|x| **x).count();
                if self.running.is_some() {
                    ui.add(egui::Spinner::new().size(16.0).color(p.muted));
                    ui.label(RichText::new("Exportando…").color(p.muted));
                } else if ui.add_enabled_ui(n > 0, |ui| kit::primary_icon(ui, icons::DOWNLOAD, if n > 1 { "Exportar formatos" } else { "Exportar" }, p)).inner.clicked() || (n > 0 && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    go = true;
                }
                if kit::button(ui, None, if self.results.is_empty() { "Cancelar" } else { "Fechar" }, p).clicked() {
                    open = false;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                open = false;
            }
        });
        if go {
            self.start(ctx, root, index, opts);
        }
        (open, remember)
    }
}
