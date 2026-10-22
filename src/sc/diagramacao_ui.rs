//! Diagramação: the book's look (ready styles, format and margins,
//! text, chapter title and number, opening, first words, running heads and
//! folios, scene breaks and parts, colours, front matter) beside its pages,
//! typeset in the background exactly as the PDF will be.

use super::library::Cx;
use super::Action;
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::scriptorium::book::{self, Book, LAYOUT_PRESETS, PAPERS, TRIM_PRESETS};
use ateliet_core::scriptorium::export::Options;
use ateliet_core::typeset::fonts::FontDb;
use ateliet_core::typeset::layout::{self, Laid};
use ateliet_core::typeset::{model, raster, PageKind, Picture};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense, Stroke};
use serde_json::{json, Value as Json};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ORNAMENTS: [(&str, &str); 8] = [("⁂", "⁂"), ("✦ ✦ ✦", "✦ ✦ ✦"), ("* * *", "* * *"), ("❦", "❦"), ("◆", "◆"), ("— ✧ —", "— ✧ —"), ("§", "§"), ("", "(espaço em branco)")];

struct Done {
    gen: u64,
    laid: Arc<Laid>,
    pics: Arc<Vec<Picture>>,
    took: Duration,
}

struct Px {
    gen: u64,
    page: usize,
    ppm: f32,
    img: egui::ColorImage,
}

pub struct DiagramacaoView {
    pub book: String,
    cfg: Json,
    loaded: bool,
    save_at: Option<Instant>,
    print: bool,
    /// pixels per mm; None fits a spread to the window
    zoom: Option<f32>,
    gen: u64,
    want: Option<Instant>,
    job: Option<Receiver<Done>>,
    laid: Option<(Arc<Laid>, Arc<Vec<Picture>>, Duration)>,
    /// page → (typesetting it shows, pixels per mm, picture); an old one stays up until the new arrives
    tex: HashMap<usize, (u64, f32, egui::TextureHandle)>,
    asked: HashSet<(usize, u32)>,
    px_tx: Sender<Px>,
    px_rx: Receiver<Px>,
    pdf: Option<Receiver<Result<PathBuf, String>>>,
    open: HashSet<&'static str>,
    goto: Option<usize>,
    toast: Option<String>,
}

impl DiagramacaoView {
    pub fn new(book: &str) -> DiagramacaoView {
        let (px_tx, px_rx) = mpsc::channel();
        DiagramacaoView { book: book.into(), cfg: Json::Null, loaded: false, save_at: None, print: true, zoom: None, gen: 0, want: Some(Instant::now()), job: None, laid: None, tex: HashMap::new(), asked: HashSet::new(), px_tx, px_rx, pdf: None, open: ["fmt"].into_iter().collect(), goto: None, toast: None }
    }

    pub fn title(&self) -> String {
        "Diagramação".into()
    }

    fn changed(&mut self) {
        self.save_at = Some(Instant::now());
        self.want = Some(Instant::now());
    }

    /// Typeset again in the background with the settings as they are now.
    fn start(&mut self, cx: &Cx, b: &Book) {
        self.gen += 1;
        let gen = self.gen;
        let (tx, rx) = mpsc::channel();
        let root = cx.vault.root.clone();
        let index = cx.vault.index.clone();
        let mut b = b.clone();
        if let Json::Object(m) = &mut b.fm {
            m.insert("diagramacao".into(), book::layout_diff(&self.cfg));
        }
        let print = self.print;
        let author = cx.sc.author();
        let ctx_typo = cx.sc.settings.data["tipografia"] != false;
        let soft = cx.sc.settings.str("quebraSimples", "paragrafo");
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let db = crate::fonts::book_fonts();
            let ix = index.read().unwrap();
            let m = model::build(&root, &ix, &b, &Options { typography: ctx_typo, soft_break: soft, author });
            drop(ix);
            let laid = layout::typeset(&m, &db, &layout::Options { print });
            let _ = tx.send(Done { gen, laid: Arc::new(laid), pics: Arc::new(m.pics), took: t0.elapsed() });
        });
        self.job = Some(rx);
    }

    fn export_pdf(&mut self, cx: &Cx, b: &Book) {
        let (tx, rx) = mpsc::channel();
        let root = cx.vault.root.clone();
        let index = cx.vault.index.clone();
        let mut b = b.clone();
        if let Json::Object(m) = &mut b.fm {
            m.insert("diagramacao".into(), book::layout_diff(&self.cfg));
        }
        let print = self.print;
        let opts = Options { typography: cx.sc.settings.data["tipografia"] != false, soft_break: cx.sc.settings.str("quebraSimples", "paragrafo"), author: cx.sc.author() };
        std::thread::spawn(move || {
            let db = crate::fonts::book_fonts();
            let ix = index.read().unwrap();
            let _ = tx.send(ateliet_core::typeset::book_pdf(&root, &ix, &b, &opts, &db, print));
        });
        self.pdf = Some(rx);
        self.toast = Some("Gerando o PDF…".into());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) -> Vec<Action> {
        let p = cx.palette;
        let mut acts = Vec::new();
        let author = cx.sc.author();
        let b = {
            let ix = cx.vault.index.read().unwrap();
            book::book_by_path(&ix, &self.book, &author)
        };
        let Some(b) = b else {
            ui.label("Livro não encontrado.");
            return acts;
        };
        if !self.loaded {
            self.cfg = book::layout(&b);
            self.loaded = true;
        }
        // save a moment after the last change
        if self.save_at.is_some_and(|t| t.elapsed() > Duration::from_millis(700)) {
            self.save_at = None;
            acts.push(Action::SetFm(b.path.clone(), "diagramacao".into(), Some(book::layout_diff(&self.cfg))));
        }
        if self.want.is_some_and(|t| t.elapsed() > Duration::from_millis(250)) && self.job.is_none() {
            self.want = None;
            self.start(cx, &b);
        }
        if self.want.is_some() || self.save_at.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
        if let Some(rx) = &self.job {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            if let Ok(d) = rx.try_recv() {
                self.job = None;
                if d.gen == self.gen {
                    if self.print {
                        // the cover's spine follows the real page count
                        let mut pc = cx.sc.settings.data["pageCounts"].clone();
                        if !pc.is_object() {
                            pc = json!({});
                        }
                        pc[&b.path] = json!(d.laid.pages.len());
                        cx.sc.settings.set("pageCounts", pc);
                    }
                    self.laid = Some((d.laid, d.pics, d.took));
                    self.asked.clear();
                }
            }
        }
        while let Ok(px) = self.px_rx.try_recv() {
            if px.gen == self.gen {
                let t = ui.ctx().load_texture(format!("page:{}:{}", px.gen, px.page), px.img, egui::TextureOptions::LINEAR);
                self.tex.insert(px.page, (px.gen, px.ppm, t));
            }
        }
        if let Some(rx) = &self.pdf {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
            if let Ok(r) = rx.try_recv() {
                self.pdf = None;
                match r {
                    Ok(path) => {
                        let rel = path.strip_prefix(&cx.vault.root).map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                        self.toast = Some(format!("PDF pronto: {rel}"));
                        acts.push(Action::Saved(rel));
                    }
                    Err(e) => self.toast = Some(format!("Não foi possível gerar o PDF: {e}")),
                }
            }
        }
        let side = egui::SidePanel::left(ui.id().with("dg-side")).resizable(false).exact_width(330.0).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 18, right: 12, top: 14, bottom: 12 })).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("dg-side-s").auto_shrink([false, false]).show(ui, |ui| self.side(ui, p, &b));
        });
        ui.painter().vline(side.response.rect.right(), side.response.rect.y_range(), Stroke::new(1.0_f32, p.border));
        egui::TopBottomPanel::top(ui.id().with("dg-bar")).frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin { left: 16, right: 14, top: 10, bottom: 8 })).show_inside(ui, |ui| {
            self.bar(ui, p, cx, &b, &mut acts);
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg_secondary)).show_inside(ui, |ui| self.pages(ui, p));
        if let Some(t) = self.toast.take() {
            acts.push(Action::Toast(t));
        }
        acts
    }

    fn bar(&mut self, ui: &mut egui::Ui, p: Palette, cx: &Cx, b: &Book, acts: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            let room = (ui.available_width() - 250.0).max(120.0);
            ui.allocate_ui_with_layout(vec2(room, 40.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Diagramação").font(kit::bold(17.0)).color(p.text));
                let mut sub = b.title.clone();
                if let Some((laid, _, _)) = &self.laid {
                    let n = laid.pages.len();
                    let spine = book::spine_mm(n, self.cfg["papel"].as_str().unwrap_or("polen80"), false);
                    sub += &format!(" · {n} páginas · lombada {:.1} mm", spine).replace('.', ",");
                }
                let r = ui.add(egui::Label::new(RichText::new(sub).size(11.5).color(p.faint)).truncate());
                if let Some((_, _, took)) = &self.laid {
                    r.on_hover_text(format!("Paginado em {} ms", took.as_millis()));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let other = if ui.available_width() > 300.0 { kit::button(ui, Some(icons::DOWNLOAD), "Outros formatos", p) } else { kit::icon_button(ui, icons::DOWNLOAD, "Outros formatos", false, p) };
                if other.clicked() {
                    acts.push(Action::Export(b.path.clone()));
                }
                let r = ui.add_enabled_ui(self.pdf.is_none(), |ui| kit::primary_icon(ui, icons::FILE_TEXT, "PDF", p)).inner;
                if r.on_hover_text(if self.print { "PDF para impressão: sangria, páginas espelhadas" } else { "PDF de tela" }).clicked() {
                    self.export_pdf(cx, b);
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if let Some(i) = kit::segmented(ui, &[(icons::PRINTER, "Impressão"), (icons::MONITOR, "Tela")], if self.print { 0 } else { 1 }, p) {
                let print = i == 0;
                if print != self.print {
                    self.print = print;
                    self.want = Some(Instant::now() - Duration::from_secs(1));
                    self.tex.clear();
                }
            }
            if self.job.is_some() {
                ui.add(egui::Spinner::new().size(12.0).color(p.faint));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let z = self.zoom.unwrap_or(2.0);
                if kit::icon_button(ui, icons::ZOOM_IN, "Aproximar", false, p).clicked() {
                    self.zoom = Some((z * 1.2).min(8.0));
                }
                if kit::icon_button(ui, icons::ZOOM_OUT, "Afastar", false, p).clicked() {
                    self.zoom = Some((z / 1.2).max(0.5));
                }
                if kit::icon_button(ui, icons::MAXIMIZE, "Ajustar à janela", self.zoom.is_none(), p).clicked() {
                    self.zoom = None;
                }
                ui.add_space(6.0);
                if let Some((laid, _, _)) = &self.laid {
                    let n = laid.pages.len();
                    egui::ComboBox::from_id_salt("dg-goto").width(110.0).selected_text(RichText::new("Ir para…").size(12.5)).show_ui(ui, |ui| {
                        for t in &laid.toc {
                            let lab = match t.0.as_str() {
                                "toc" => "Sumário".to_string(),
                                id if id.starts_with("ch") => format!("Capítulo {}", &id[2..]),
                                id => id.trim_start_matches("bm-").to_string(),
                            };
                            if ui.selectable_label(false, format!("{lab} · p. {}", t.1)).clicked() {
                                self.goto = Some(t.1.min(n));
                            }
                        }
                    });
                }
            });
        });
    }

    /// The pages in spreads (print) or one after another (screen).
    fn pages(&mut self, ui: &mut egui::Ui, p: Palette) {
        let Some((laid, pics, _)) = self.laid.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Diagramando…").color(p.faint));
            });
            return;
        };
        let ppp = ui.ctx().pixels_per_point();
        let goto = self.goto.take();
        egui::ScrollArea::both().id_salt("dg-pages").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(20.0);
            let n = laid.pages.len();
            // rows of pages: [1], [2,3], [4,5]… in print; pairs in screen too
            let mut rows: Vec<Vec<usize>> = Vec::new();
            if self.print {
                rows.push(vec![0]);
                let mut i = 1;
                while i < n {
                    rows.push((i..(i + 2).min(n)).collect());
                    i += 2;
                }
            } else {
                let mut i = 0;
                while i < n {
                    rows.push((i..(i + 2).min(n)).collect());
                    i += 2;
                }
            }
            let (tw, th) = laid.pages.first().map(|pg| (pg.trim.2, pg.trim.3)).unwrap_or((140.0, 210.0));
            // fit a spread in the window unless zoomed by hand
            let z = self.zoom.unwrap_or_else(|| ((ui.available_width() - 72.0) / (2.0 * tw + 12.0)).min((ui.available_height() - 60.0) / th).clamp(0.5, 8.0));
            let pw = tw * z;
            let ph = th * z;
            let full_w = (2.0 * pw + 12.0).max(ui.available_width() - 40.0);
            for row in rows {
                let (rect, _) = ui.allocate_exact_size(vec2(full_w + 40.0, ph + 34.0), Sense::hover());
                let cx = rect.center().x;
                for &i in &row {
                    let pg = &laid.pages[i];
                    let left = if self.print { pg.recto } else { i % 2 == 1 };
                    let x = if self.print { if left { cx } else { cx - pw } } else if i % 2 == 0 { cx - pw - 6.0 } else { cx + 6.0 };
                    let pr = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(pw, ph));
                    if goto == Some(pg.n) {
                        ui.scroll_to_rect(pr, Some(egui::Align::Center));
                    }
                    ui.painter().rect_filled(pr.translate(vec2(0.0, 2.0)), 1.0, Color32::from_black_alpha(if p.dark { 80 } else { 26 }));
                    let paper = Color32::from_rgb(pg.paper[0], pg.paper[1], pg.paper[2]);
                    ui.painter().rect_filled(pr, 0.0, paper);
                    if ui.is_rect_visible(pr) {
                        let ppm = (z * ppp * 4.0).round() / 4.0;
                        match self.tex.get(&i) {
                            Some((g, m, t)) => {
                                // the picture covers the trim box of the page
                                let full = vec2(pg.w, pg.h);
                                let uv = Rect::from_min_size(pos2(pg.trim.0 / full.x, pg.trim.1 / full.y), vec2(pg.trim.2 / full.x, pg.trim.3 / full.y));
                                ui.painter().image(t.id(), pr, uv, Color32::WHITE);
                                if (*g != self.gen || (*m - ppm).abs() > 0.01) && self.asked.insert((i, (ppm * 100.0) as u32)) {
                                    self.raster(&laid, &pics, i, ppm);
                                }
                            }
                            None => {
                                if self.asked.insert((i, (ppm * 100.0) as u32)) {
                                    self.raster(&laid, &pics, i, ppm);
                                }
                            }
                        }
                    }
                    if pg.kind == PageKind::Blank {
                        ui.painter().text(pr.center(), egui::Align2::CENTER_CENTER, "em branco", egui::FontId::proportional(11.0), p.faint);
                    }
                    let lab = pg.n.to_string();
                    ui.painter().text(pos2(pr.center().x, pr.bottom() + 12.0), egui::Align2::CENTER_CENTER, lab, egui::FontId::proportional(11.0), p.faint);
                }
                if self.print && row.len() == 2 {
                    // the spine between facing pages
                    ui.painter().vline(cx, (rect.top() + 4.0)..=(rect.top() + 4.0 + ph), Stroke::new(1.0_f32, Color32::from_black_alpha(30)));
                }
            }
            ui.add_space(30.0);
        });
    }

    fn raster(&self, laid: &Arc<Laid>, pics: &Arc<Vec<Picture>>, i: usize, ppm: f32) {
        let (laid, pics, tx, gen) = (laid.clone(), pics.clone(), self.px_tx.clone(), self.gen);
        rayon_spawn(move || {
            let db: Arc<FontDb> = crate::fonts::book_fonts();
            let pg = &laid.pages[i];
            let (w, h, px) = raster::render(pg, &db, &pics, ppm);
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            let _ = tx.send(Px { gen, page: i, ppm, img });
        });
    }

    // ── settings ────────────────────────────────────────────────────────

    fn group(&mut self, ui: &mut egui::Ui, p: Palette, key: &'static str, title: &str, body: impl FnOnce(&mut Self, &mut egui::Ui)) {
        let open = self.open.contains(key);
        let r = ui.add(egui::Label::new(RichText::new(format!("{}  {title}", if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT })).font(kit::medium(13.5)).color(p.text)).sense(Sense::click()));
        if r.clicked() {
            if open {
                self.open.remove(key);
            } else {
                self.open.insert(key);
            }
        }
        if open {
            ui.add_space(4.0);
            egui::Frame::new().inner_margin(egui::Margin { left: 6, right: 0, top: 0, bottom: 6 }).show(ui, |ui| body(self, ui));
        }
        ui.add_space(6.0);
        kit::hairline(ui, p);
        ui.add_space(6.0);
    }

    fn side(&mut self, ui: &mut egui::Ui, p: Palette, b: &Book) {
        let _ = b;
        ui.label(RichText::new("ESTILO DO LIVRO").size(10.5).color(p.faint));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (key, label, desc, set) in LAYOUT_PRESETS.iter() {
                let on = matches_style(&self.cfg, set);
                let (r, resp) = ui.allocate_exact_size(vec2(140.0, 54.0), Sense::click());
                ui.painter().rect(r, 8.0, if on { p.accent_soft } else if resp.hovered() { p.bg_hover } else { p.bg }, Stroke::new(1.0_f32, if on { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
                ui.painter().text(r.min + vec2(10.0, 9.0), egui::Align2::LEFT_TOP, *label, kit::medium(12.5), p.text);
                let g = crate::sc::enredo_ui::truncated(ui, desc, egui::FontId::proportional(10.5), p.faint, 124.0);
                ui.painter().galley(r.min + vec2(10.0, 29.0), g, p.faint);
                if resp.on_hover_text(*desc).clicked() {
                    apply_style(&mut self.cfg, set);
                    let _ = key;
                    self.changed();
                    self.tex.clear();
                }
            }
        });
        ui.add_space(12.0);
        kit::hairline(ui, p);
        ui.add_space(8.0);
        self.group(ui, p, "fmt", "Formato e margens", |me, ui| {
            let cur = me.cfg["formato"].as_str().unwrap_or("14x21").to_string();
            row(ui, p, "Tamanho do livro", |ui| {
                egui::ComboBox::from_id_salt("dg-fmt").width(170.0).selected_text(TRIM_PRESETS.iter().find(|t| t.0 == cur).map(|t| t.1).unwrap_or("Personalizado")).show_ui(ui, |ui| {
                    for t in TRIM_PRESETS {
                        if ui.selectable_label(cur == t.0, t.1).clicked() {
                            me.cfg["formato"] = json!(t.0);
                            if t.0 != "custom" {
                                me.cfg["largura"] = json!(t.2);
                                me.cfg["altura"] = json!(t.3);
                            }
                            me.changed();
                        }
                    }
                });
            });
            if cur == "custom" {
                me.number(ui, p, "Largura (mm)", "largura", 80.0, 300.0, 1.0);
                me.number(ui, p, "Altura (mm)", "altura", 100.0, 400.0, 1.0);
            }
            ui.label(RichText::new("Margens (mm)").size(12.0).color(p.muted));
            for (k, lab) in [("superior", "Topo"), ("externa", "Fora"), ("inferior", "Pé"), ("interna", "Lombada")] {
                let mut v = me.cfg["margens"][k].as_f64().unwrap_or(15.0) as f32;
                row(ui, p, lab, |ui| {
                    if ui.add(egui::DragValue::new(&mut v).range(3.0..=60.0).speed(0.25).suffix(" mm")).changed() {
                        me.cfg["margens"][k] = json!(v);
                        me.changed();
                    }
                });
            }
            me.number(ui, p, "Sangria (mm)", "sangria", 0.0, 10.0, 0.1);
            let paper = me.cfg["papel"].as_str().unwrap_or("polen80").to_string();
            row(ui, p, "Papel (define a lombada)", |ui| {
                egui::ComboBox::from_id_salt("dg-paper").width(150.0).selected_text(PAPERS.iter().find(|x| x.0 == paper).map(|x| x.1).unwrap_or("")).show_ui(ui, |ui| {
                    for x in PAPERS {
                        if ui.selectable_label(paper == x.0, x.1).clicked() {
                            me.cfg["papel"] = json!(x.0);
                            me.changed();
                        }
                    }
                });
            });
        });
        self.group(ui, p, "texto", "Texto", |me, ui| {
            me.font(ui, p, "Fonte do texto", "fonte", None);
            me.slider(ui, p, "Corpo (pt)", "corpo", 8.0, 14.0, 0.25);
            me.slider(ui, p, "Entrelinha", "entrelinha", 1.1, 2.0, 0.05);
            me.slider(ui, p, "Recuo de parágrafo (em)", "recuo", 0.0, 3.0, 0.1);
            me.choice(ui, p, "Espaço entre parágrafos", "espacoParagrafo", &[("auto", "Automático"), ("nenhum", "Nenhum"), ("meia", "Meia linha"), ("linha", "Uma linha")]);
            me.toggle(ui, p, "Justificar", "justificar");
            me.toggle(ui, p, "Hifenização automática", "hifenizar");
        });
        self.group(ui, p, "titulo", "Título do capítulo", |me, ui| {
            me.font(ui, p, "Fonte dos títulos", "fonteTitulos", None);
            me.slider(ui, p, "Tamanho (em)", "tituloTamanho", 1.0, 4.5, 0.05);
            me.choice(ui, p, "Peso", "tituloPeso", &[("300", "Leve"), ("400", "Normal"), ("500", "Médio"), ("600", "Seminegrito"), ("700", "Negrito"), ("900", "Black")]);
            me.choice(ui, p, "Caixa", "tituloCaixa", &[("normal", "Como escrito"), ("alta", "MAIÚSCULAS"), ("versalete", "Versalete"), ("versalete-total", "VERSALETE TOTAL")]);
            me.toggle(ui, p, "Itálico", "tituloItalico");
            me.slider(ui, p, "Espaço entre letras (em)", "tituloEspacamento", 0.0, 0.6, 0.01);
            me.choice(ui, p, "Alinhamento da abertura", "tituloAlinhamento", &[("centro", "Centralizado"), ("esquerda", "À esquerda"), ("direita", "À direita")]);
        });
        self.group(ui, p, "rotulo", "Numeração do capítulo", |me, ui| {
            me.choice(ui, p, "Número", "numeracaoCapitulo", &[("extenso", "Por extenso (Um)"), ("romano", "Romano (I)"), ("numero", "Algarismo (1)"), ("so-extenso", "Só por extenso"), ("so-numero", "Só o algarismo"), ("so-romano", "Só o romano"), ("nenhum", "Sem número")]);
            let mode = me.cfg["numeracaoCapitulo"].as_str().unwrap_or("extenso").to_string();
            if mode != "nenhum" {
                if !mode.starts_with("so-") {
                    me.text(ui, p, "Palavra antes do número", "rotuloPrefixo", "Capítulo");
                }
                me.choice(ui, p, "Posição", "rotuloPosicao", &[("acima", "Acima do título"), ("abaixo", "Abaixo do título")]);
                me.slider(ui, p, "Tamanho (em)", "rotuloTamanho", 0.6, 6.0, 0.05);
                me.choice(ui, p, "Caixa", "rotuloCaixa", &[("versalete", "Versalete"), ("versalete-total", "VERSALETE TOTAL"), ("alta", "MAIÚSCULAS"), ("normal", "Como escrito")]);
            }
        });
        self.group(ui, p, "abertura", "Abertura do capítulo", |me, ui| {
            me.toggle(ui, p, "Começar na página ímpar", "aberturaImpar");
            me.slider(ui, p, "Descida da abertura (%)", "descida", 0.0, 60.0, 1.0);
            me.choice(ui, p, "Fio decorativo", "fio", &[("nenhum", "Nenhum"), ("fino", "Fio fino"), ("curto", "Fio curto"), ("duplo", "Fio duplo"), ("pontilhado", "Pontilhado"), ("largo", "De margem a margem")]);
            if me.cfg["fio"].as_str().is_some_and(|f| f != "nenhum") {
                me.choice(ui, p, "Lugar do fio", "fioLugar", &[("abaixo", "Depois do título"), ("entre", "Entre o número e o título")]);
            }
            me.pick_text(ui, p, "Ornamento", "ornamentoAbertura", &ORNAMENTS);
        });
        self.group(ui, p, "inicio", "Início do texto", |me, ui| {
            me.toggle(ui, p, "Capitular (letra grande)", "capitular");
            if me.cfg["capitular"].as_bool().unwrap_or(false) {
                me.slider(ui, p, "Altura (linhas)", "capitularLinhas", 2.0, 6.0, 1.0);
                me.choice(ui, p, "Tipo", "capitularEstilo", &[("afundada", "Afundada no texto"), ("elevada", "Elevada")]);
            }
            me.toggle(ui, p, "Versalete no começo", "versaleteInicio");
            if me.cfg["versaleteInicio"].as_bool().unwrap_or(false) {
                me.choice(ui, p, "Alcance", "versaleteAlcance", &[("linha", "Primeira linha inteira"), ("palavras", "Primeiras palavras")]);
            }
        });
        self.group(ui, p, "pagina", "Cabeçalho e número de página", |me, ui| {
            me.toggle(ui, p, "Cabeçalho corrente", "cabecalho");
            if me.cfg["cabecalho"].as_bool().unwrap_or(true) {
                let what = [("livro", "Título do livro"), ("autor", "Autor"), ("capitulo", "Título do capítulo"), ("nada", "Nada")];
                me.choice(ui, p, "Página da esquerda", "cabecalhoVerso", &what);
                me.choice(ui, p, "Página da direita", "cabecalhoRecto", &what);
                me.choice(ui, p, "Estilo", "cabecalhoEstilo", &[("versalete", "Versalete"), ("italico", "Itálico"), ("alta", "MAIÚSCULAS pequenas"), ("normal", "Normal")]);
                me.choice(ui, p, "Fonte", "cabecalhoFonte", &[("texto", "Do texto"), ("titulos", "Dos títulos")]);
            }
            me.choice(ui, p, "Número de página", "folio", &[("externo", "Lado externo"), ("centro", "Centro"), ("interno", "Lado interno"), ("nenhum", "Sem número")]);
            if me.cfg["folio"].as_str() != Some("nenhum") {
                me.choice(ui, p, "Posição", "folioPosicao", &[("pe", "No pé da página"), ("topo", "No topo")]);
                me.pick_text(ui, p, "Formato", "folioFormato", &[("{n}", "12"), ("— {n} —", "— 12 —"), ("· {n} ·", "· 12 ·"), ("[ {n} ]", "[ 12 ]"), ("{n} ❧", "12 ❧"), ("~ {n} ~", "~ 12 ~")]);
                me.choice(ui, p, "Algarismos", "folioAlgarismos", &[("antigo", "Antigos"), ("alinhado", "Alinhados")]);
                me.toggle(ui, p, "Mostrar na abertura de capítulo", "folioAbertura");
            }
        });
        self.group(ui, p, "cena", "Quebras de cena e partes", |me, ui| {
            me.pick_text(ui, p, "Quebra de cena", "ornamento", &ORNAMENTS);
            me.text(ui, p, "Rótulo das partes", "rotuloParte", "Parte");
        });
        self.group(ui, p, "cores", "Cores", |me, ui| {
            me.color(ui, p, "Texto", "corTexto", "#1a1410");
            me.color(ui, p, "Títulos", "corTitulos", "#1a1410");
            me.color(ui, p, "Destaque", "corDestaque", "#8a6d3b");
            me.color(ui, p, "Capitular", "capitularCor", "#3d2b1a");
            me.color(ui, p, "Papel", "corPapel", "#ffffff");
        });
        self.group(ui, p, "iniciais", "Páginas iniciais", |me, ui| {
            for (k, l) in [("falsaFolha", "Falsa folha de rosto"), ("rosto", "Folha de rosto"), ("creditos", "Créditos / copyright"), ("dedicatoria", "Dedicatória"), ("sumario", "Sumário")] {
                let mut v = me.cfg["paginasIniciais"][k].as_bool().unwrap_or(true);
                row(ui, p, l, |ui| {
                    if kit::toggle(ui, &mut v, p).changed() {
                        me.cfg["paginasIniciais"][k] = json!(v);
                        me.changed();
                    }
                });
            }
            ui.label(RichText::new("Dedicatória, epígrafe, ISBN e editora ficam nas propriedades da nota do livro; \"Agradecimentos\" e \"Sobre o autor\" vêm das seções com esses nomes.").size(11.5).color(p.faint));
        });
    }

    fn number(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str, min: f32, max: f32, step: f32) {
        let mut v = self.cfg[key].as_f64().unwrap_or(0.0) as f32;
        row(ui, p, label, |ui| {
            if ui.add(egui::DragValue::new(&mut v).range(min..=max).speed(step)).changed() {
                self.cfg[key] = json!(v);
                self.changed();
            }
        });
    }

    fn slider(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str, min: f32, max: f32, step: f32) {
        let mut v = self.cfg[key].as_f64().unwrap_or(min as f64) as f32;
        ui.label(RichText::new(label).size(12.5).color(p.text));
        let r = ui.add(egui::Slider::new(&mut v, min..=max).step_by(step as f64));
        if r.changed() {
            self.cfg[key] = json!((v / step).round() * step);
            self.changed();
        }
        ui.add_space(4.0);
    }

    fn toggle(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str) {
        let mut v = self.cfg[key].as_bool().unwrap_or(false);
        row(ui, p, label, |ui| {
            if kit::toggle(ui, &mut v, p).changed() {
                self.cfg[key] = json!(v);
                self.changed();
            }
        });
    }

    fn choice(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str, opts: &[(&str, &str)]) {
        let cur = match &self.cfg[key] {
            Json::String(s) => s.clone(),
            Json::Number(n) => n.to_string(),
            _ => String::new(),
        };
        let shown = opts.iter().find(|o| o.0 == cur).map(|o| o.1.to_string()).unwrap_or(cur.clone());
        row(ui, p, label, |ui| {
            egui::ComboBox::from_id_salt(("dg", key)).width(150.0).selected_text(shown).show_ui(ui, |ui| {
                for (k, l) in opts {
                    if ui.selectable_label(cur == *k, *l).clicked() {
                        self.cfg[key] = if self.cfg[key].is_number() || key == "tituloPeso" { json!(k.parse::<f64>().unwrap_or(0.0)) } else { json!(k) };
                        self.changed();
                    }
                }
            });
        });
    }

    fn text(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str, hint: &str) {
        let mut v = self.cfg[key].as_str().unwrap_or(hint).to_string();
        row(ui, p, label, |ui| {
            if ui.add(egui::TextEdit::singleline(&mut v).desired_width(120.0).hint_text(RichText::new(hint).color(p.faint))).changed() {
                self.cfg[key] = json!(v);
                self.changed();
            }
        });
    }

    fn pick_text(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str, opts: &[(&str, &str)]) {
        let mut v = self.cfg[key].as_str().unwrap_or("").to_string();
        row(ui, p, label, |ui| {
            ui.menu_button(kit::icon(icons::CHEVRON_DOWN, 13.0).color(p.muted), |ui| {
                for (k, l) in opts {
                    if ui.selectable_label(v == *k, *l).clicked() {
                        self.cfg[key] = json!(k);
                        self.changed();
                        ui.close_menu();
                    }
                }
            });
            if ui.add(egui::TextEdit::singleline(&mut v).desired_width(90.0)).changed() {
                self.cfg[key] = json!(v);
                self.changed();
            }
        });
    }

    fn color(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str, auto: &str) {
        let cur = self.cfg[key].as_str().filter(|s| !s.is_empty()).unwrap_or(auto).to_string();
        let c = super::color(&cur);
        let mut rgb = [c.r(), c.g(), c.b()];
        row(ui, p, label, |ui| {
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                self.cfg[key] = json!(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
                self.changed();
            }
            let custom = self.cfg[key].as_str().is_some_and(|s| !s.is_empty());
            if custom && key != "corTexto" && key != "corPapel" && kit::icon_button(ui, icons::ROTATE_CCW, "Voltar ao automático", false, p).clicked() {
                self.cfg[key] = json!("");
                self.changed();
            }
        });
    }

    fn font(&mut self, ui: &mut egui::Ui, p: Palette, label: &str, key: &str, empty: Option<&str>) {
        let cur = self.cfg[key].as_str().unwrap_or("").to_string();
        let families = font_families();
        let shown = if cur.is_empty() { empty.unwrap_or("").to_string() } else { cur.clone() };
        let missing = !cur.is_empty() && !families.iter().any(|f| f.eq_ignore_ascii_case(&cur)) && !cur.eq_ignore_ascii_case("cormorant");
        row(ui, p, label, |ui| {
            egui::ComboBox::from_id_salt(("dg-font", key)).width(170.0).selected_text(shown).height(360.0).show_ui(ui, |ui| {
                if let Some(e) = empty {
                    if ui.selectable_label(cur.is_empty(), e).clicked() {
                        self.cfg[key] = json!("");
                        self.changed();
                    }
                }
                for f in &families {
                    if ui.selectable_label(cur.eq_ignore_ascii_case(f), f.as_str()).clicked() {
                        self.cfg[key] = json!(f);
                        self.changed();
                    }
                }
            });
        });
        if missing {
            ui.label(RichText::new(format!("\"{cur}\" não está instalada: o livro usa {} no lugar.", if key == "fonte" { "EB Garamond" } else { "Cormorant Garamond" })).size(11.5).color(p.faint));
        }
    }
}

fn row(ui: &mut egui::Ui, p: Palette, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.5).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

pub fn font_families() -> Vec<String> {
    static CACHE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| crate::fonts::book_fonts().families()).clone()
}

/// A ready style over the settings (format and margins only when it sets them).
fn apply_style(cfg: &mut Json, set: &Json) {
    for (k, v) in set.as_object().into_iter().flatten() {
        if k == "margens" {
            for (a, b) in v.as_object().into_iter().flatten() {
                cfg["margens"][a] = b.clone();
            }
        } else {
            cfg[k] = v.clone();
        }
    }
}

fn matches_style(cfg: &Json, set: &Json) -> bool {
    set.as_object().into_iter().flatten().all(|(k, v)| match (v, &cfg[k]) {
        (Json::Number(a), Json::Number(b)) => (a.as_f64().unwrap_or(0.0) - b.as_f64().unwrap_or(0.0)).abs() < 1e-6,
        (Json::Object(a), _) => a.iter().all(|(x, y)| cfg[k][x] == *y || (y.as_f64().is_some() && (y.as_f64() == cfg[k][x].as_f64()))),
        (a, b) => a == b,
    })
}

/// Background work on a small pool (pages are drawn in parallel).
fn rayon_spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}
