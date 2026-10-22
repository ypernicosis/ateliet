//! The export menu: this frame, every frame as PNG files in a folder next
//! to the storyboard, a contact sheet, and the timeline as an animated PNG.

use super::*;
use ateliet_core::board::export as bx;

pub(super) struct ExportOpts {
    scale: f32,
    with_trace: bool,
    transparent: bool,
    columns: usize,
    notes: bool,
}

impl Default for ExportOpts {
    fn default() -> Self {
        ExportOpts { scale: 1.0, with_trace: false, transparent: false, columns: 4, notes: true }
    }
}

impl BoardEditor {
    fn stem(&self) -> String {
        self.abs.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn written(&mut self, what: &str, out: &std::path::Path, r: std::io::Result<()>) {
        self.error = Some(match r {
            Ok(()) => format!("{what}: {}", out.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            Err(e) => format!("Não foi possível exportar: {e}"),
        });
    }

    fn export_frame_png(&mut self) {
        let o = &self.xo;
        let (scale, trace, transparent) = (o.scale, o.with_trace, o.transparent);
        let (w, h, px) = bx::frame_rgba(&mut self.doc, &mut self.pics, self.frame, scale, trace, transparent);
        let name = self.doc.frame(self.frame)["name"].as_str().unwrap_or("").to_string();
        let out = self.abs.with_file_name(bx::frame_file_name(&self.stem(), self.frame, &name));
        let r = std::fs::write(&out, ateliet_core::board::tile::encode_png(&px, w, h));
        self.written("Exportado", &out, r);
    }

    fn export_all_frames(&mut self) {
        let stem = self.stem();
        let dir = self.abs.with_file_name(format!("{stem} - quadros"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.error = Some(format!("Não foi possível criar a pasta: {e}"));
            return;
        }
        let o = &self.xo;
        let (scale, trace, transparent) = (o.scale, o.with_trace, o.transparent);
        let mut n = 0;
        for i in 0..self.doc.frame_count() {
            let (w, h, px) = bx::frame_rgba(&mut self.doc, &mut self.pics, i, scale, trace, transparent);
            let name = self.doc.frame(i)["name"].as_str().unwrap_or("").to_string();
            if std::fs::write(dir.join(bx::frame_file_name(&stem, i, &name)), ateliet_core::board::tile::encode_png(&px, w, h)).is_ok() {
                n += 1;
            }
        }
        self.error = Some(format!("{n} {} em {}", if n == 1 { "PNG" } else { "PNGs" }, dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
    }

    fn export_contact_sheet(&mut self) {
        let db = crate::fonts::book_fonts();
        let stem = self.stem();
        let (cols, notes) = (self.xo.columns, self.xo.notes);
        let (w, h, px) = bx::contact_sheet(&mut self.doc, &mut self.pics, &db, &stem, cols, notes, 2400);
        let out = self.abs.with_file_name(format!("{stem} - folha de contato.png"));
        let r = std::fs::write(&out, ateliet_core::board::tile::encode_png(&px, w, h));
        self.written("Folha de contato", &out, r);
    }

    fn export_animated_png(&mut self) {
        let stem = self.stem();
        let max = (1280.0 * self.xo.scale).round() as u32;
        let bytes = bx::animated_png(&mut self.doc, &mut self.pics, max);
        let out = self.abs.with_file_name(format!("{stem} - animação.png"));
        let r = std::fs::write(&out, bytes);
        self.written("PNG animado (toca nas notas)", &out, r);
    }

    /// A Clip Studio file into this frame (on top, centred) or a new frame after it.
    pub(super) fn import_clip(&mut self, new_frame: bool) {
        let Some(f) = rfd::FileDialog::new().add_filter("Clip Studio", &["clip"]).pick_file() else { return };
        let clip = match std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| ateliet_core::clipstudio::read(&b)) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("Não foi possível ler o arquivo: {e}"));
                return;
            }
        };
        let dx = (self.doc.width() as i32 - clip.width as i32) / 2;
        let dy = (self.doc.height() as i32 - clip.height as i32) / 2;
        let at_frame = self.frame;
        let mut approx = Vec::new();
        let mut target = at_frame;
        self.structure(|d| {
            target = if new_frame { d.add_frame(at_frame + 1) } else { at_frame };
            let at = d.frame_layers(target).len();
            approx = ateliet_core::board::clip::add_layers(d, &clip, target, at, false, dx, dy);
        });
        self.frame = target;
        self.invalidate();
        self.error = Some(format!("{} camadas importadas.{}", clip.layers.len(), ateliet_core::board::clip::summary(&clip, &approx)));
    }

    /// The export button and its menu.
    pub(super) fn export_menu(&mut self, ui: &mut egui::Ui, p: crate::theme::Palette) {
        let r = kit::icon_button(ui, icons::DOWNLOAD, "Exportar imagens", false, p);
        let id = ui.make_persistent_id("board-export");
        if r.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        let pages = self.is_pages();
        let timeline = self.has_timeline();
        let mut act: Option<u8> = None;
        egui::popup_below_widget(ui, id, &r, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(250.0);
            if kit::menu_item(ui, icons::IMAGE, if pages { "Esta página (PNG)" } else { "Este quadro (PNG)" }, p) {
                act = Some(0);
            }
            if kit::menu_item(ui, icons::FOLDER_CLOSED, if pages { "Todas as páginas (PNG, numa pasta)" } else { "Todos os quadros (PNG, numa pasta)" }, p) {
                act = Some(1);
            }
            if kit::menu_item(ui, icons::LAYOUT_GRID, "Folha de contato (PNG)", p) {
                act = Some(2);
            }
            if timeline && kit::menu_item(ui, icons::PLAY, "PNG animado da linha do tempo", p) {
                act = Some(3);
            }
            ui.add_space(4.0);
            kit::hairline(ui, p);
            ui.add_space(4.0);
            let o = &mut self.xo;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tamanho").size(12.0).color(p.muted));
                for (k, l) in [(0.5, "50%"), (1.0, "100%"), (2.0, "200%")] {
                    if ui.selectable_label((o.scale - k).abs() < 1e-3, RichText::new(l).size(12.0)).clicked() {
                        o.scale = k;
                    }
                }
            });
            for (v, l) in [(&mut o.with_trace, "Com o decalque"), (&mut o.transparent, "Fundo transparente")] {
                ui.horizontal(|ui| {
                    kit::toggle(ui, v, p);
                    ui.label(RichText::new(l).size(12.0).color(p.muted));
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Folha: colunas").size(12.0).color(p.muted));
                ui.add(egui::DragValue::new(&mut o.columns).range(1..=8));
                kit::toggle(ui, &mut o.notes, p);
                ui.label(RichText::new("notas").size(12.0).color(p.muted));
            });
        });
        if let Some(a) = act {
            ui.memory_mut(|m| m.close_popup());
            match a {
                0 if pages => self.export_page_png(),
                0 => self.export_frame_png(),
                1 => self.export_all_frames(),
                2 => self.export_contact_sheet(),
                _ => self.export_animated_png(),
            }
        }
    }
}
