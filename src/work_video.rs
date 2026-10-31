//! Video accompanying any workspace, without occupying a document tab.
mod native;

use crate::{config, icons, kit, theme::Palette};
use ateliet_core::work_video::{self as model, History, Recent, Source};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub struct Video {
    pub open: bool,
    history: History,
    current: Option<Recent>,
    backend: Option<native::Backend>,
    snapshot: native::Snapshot,
    url: String,
    error: String,
    failed_source: Option<Source>,
    focus_url: bool,
    picker: Option<Receiver<Option<PathBuf>>>,
    last_poll: Instant,
    last_save: Instant,
    saved: String,
    rect: Option<egui::Rect>,
}

fn state_path() -> PathBuf { config::app_config_path().with_file_name("video-session.json") }

impl Default for Video {
    fn default() -> Self {
        let saved = std::fs::read_to_string(state_path()).unwrap_or_default();
        let mut history: History = serde_json::from_str(&saved).unwrap_or_default();
        history.sanitize();
        Self {
            open: false, history, current: None, backend: None, snapshot: Default::default(),
            url: String::new(), error: String::new(), focus_url: false, picker: None,
            failed_source: None,
            last_poll: Instant::now(), last_save: Instant::now(), saved,
            rect: None,
        }
    }
}

impl Video {
    pub fn active(&self) -> bool { self.snapshot.visible }

    pub fn captures_drop(&self, ctx: &egui::Context) -> bool {
        self.open && self.rect.is_some_and(|r| ctx.input(|i| i.pointer.latest_pos().is_some_and(|pos| r.contains(pos))))
    }

    pub fn open_dialog(&mut self) {
        self.open = true;
        self.focus_url = true;
    }

    pub fn play_local(&mut self, path: PathBuf) {
        self.play(Source::Local { path }, None);
    }

    fn collect(&mut self) {
        let Some(backend) = &self.backend else { return };
        self.snapshot = backend.snapshot();
        self.history.pinned = self.snapshot.pinned;
        if let Some(current) = &mut self.current {
            if matches!(current.source, Source::Youtube { .. }) && !self.snapshot.id.is_empty() {
                let source = Source::Youtube { id: self.snapshot.id.clone() };
                if source.valid() { current.source = source; }
            }
            if self.snapshot.position.is_finite() { current.position = self.snapshot.position.max(0.0); }
            if !self.snapshot.title.is_empty() { current.title = self.snapshot.title.clone(); }
            self.history.remember(current.source.clone(), current.title.clone(), current.position);
        }
        if self.snapshot.change { self.open_dialog(); }
    }

    fn play(&mut self, source: Source, position: Option<f64>) {
        if !source.valid() { self.error = "Fonte de vídeo inválida.".into(); self.open = true; return; }
        if let Source::Local { path } = &source {
            if !path.is_file() {
                self.error = "O arquivo foi movido ou não está disponível. Escolha-o novamente.".into();
                self.open = true;
                return;
            }
        }
        self.collect();
        let previous = self.history.recent.iter().find(|r| r.source == source);
        let at = position.unwrap_or_else(|| previous.map(|r| r.position).unwrap_or(0.0));
        let title = previous.map(|r| r.title.clone()).unwrap_or_else(|| source.label());
        let result = (|| {
            if self.backend.is_none() { self.backend = Some(native::Backend::new()?); }
            let json = serde_json::to_string(&serde_json::json!({"source":source, "position":at, "title":title, "pinned":self.history.pinned})).map_err(|e| e.to_string())?;
            self.backend.as_mut().unwrap().open(&json)
        })();
        if let Err(error) = result {
            self.error = error; self.failed_source = Some(source); self.open = true; return;
        }
        self.current = Some(Recent { source: source.clone(), title: title.clone(), position: at });
        self.history.remember(source, title, at);
        self.snapshot.visible = true;
        self.error.clear(); self.open = false;
        self.failed_source = None;
        self.last_poll = Instant::now() - Duration::from_secs(2);
        self.save();
    }

    pub fn reveal(&mut self) {
        if self.snapshot.visible {
            if let Some(backend) = &mut self.backend { backend.reveal(); }
        } else if let Some(current) = &self.current {
            self.play(current.source.clone(), None);
        } else { self.open_dialog(); }
    }

    pub fn toggle(&mut self) {
        if self.snapshot.visible {
            if let Some(backend) = &mut self.backend { backend.toggle(); }
            self.last_poll = Instant::now() - Duration::from_secs(2);
        } else { self.reveal(); }
    }

    fn choose_file(&mut self, ctx: &egui::Context) {
        if self.picker.is_some() { return; }
        let (tx, rx) = mpsc::channel();
        self.picker = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let path = rfd::FileDialog::new().set_title("Vídeo para acompanhar o trabalho").add_filter("Vídeos", model::EXTENSIONS).pick_file();
            let _ = tx.send(path);
            ctx.request_repaint();
        });
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.picker {
            match rx.try_recv() {
                Ok(path) => { self.picker = None; if let Some(path) = path { self.play_local(path); } }
                Err(mpsc::TryRecvError::Disconnected) => { self.picker = None; }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.backend.is_some() && self.last_poll.elapsed() >= Duration::from_secs(1) {
            let was_visible = self.snapshot.visible;
            self.collect();
            self.last_poll = Instant::now();
            if (was_visible && !self.snapshot.visible) || self.last_save.elapsed() >= Duration::from_secs(15) { self.save(); }
        }
        if self.snapshot.visible { ctx.request_repaint_after(Duration::from_secs(1)); }
    }

    pub fn save(&mut self) {
        let Ok(json) = serde_json::to_string_pretty(&self.history) else { return };
        if json == self.saved { return; }
        let path = state_path();
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, &json)?;
            std::fs::rename(tmp, path)
        })();
        match result {
            Ok(()) => { self.saved = json; self.last_save = Instant::now(); }
            Err(_) => { self.error = "Não foi possível guardar a posição deste vídeo.".into(); self.last_save = Instant::now(); }
        }
    }

    pub fn shutdown(&mut self) { self.collect(); self.save(); self.backend = None; }

    pub fn status(&mut self, ui: &mut egui::Ui, p: Palette) {
        let Some(current) = &self.current else { return };
        let title = current.title.clone();
        if self.snapshot.visible && kit::icon_button_sized(ui, if self.snapshot.playing { icons::PAUSE } else { icons::PLAY }, "Pausar / continuar vídeo", false, p, 18.0, 12.0).clicked() { self.toggle(); }
        if kit::icon_button_sized(ui, icons::FILM, &format!("Mostrar vídeo: {title}"), self.snapshot.visible, p, 18.0, 12.0).clicked() { self.reveal(); }
    }

    pub fn ui(&mut self, ctx: &egui::Context, p: Palette) {
        if !self.open { return; }
        let mut open = true;
        let mut play = None;
        let mut pick = false;
        let mut submitted = false;
        let window = egui::Window::new("Vídeo enquanto trabalho").id(egui::Id::new("work-video-picker")).open(&mut open)
            .collapsible(false).resizable(false).default_width(440.0)
            .max_height((ctx.screen_rect().height() - 80.0).max(180.0)).vscroll(true).show(ctx, |ui| {
                ui.label(egui::RichText::new("Um vídeo ao lado do que você está criando.").color(p.muted));
                ui.add_space(10.0);
                ui.add_enabled_ui(self.picker.is_none(), |ui| {
                    if ui.button("Abrir vídeo do computador…").clicked() { pick = true; }
                });
                ui.add_space(8.0);
                ui.label("Link do YouTube");
                let input = ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("Cole o link do vídeo").desired_width(f32::INFINITY));
                if self.focus_url { input.request_focus(); self.focus_url = false; }
                let valid = model::youtube(&self.url);
                if input.changed() { self.error.clear(); }
                ui.horizontal(|ui| {
                    if ui.add_enabled(valid.is_ok(), egui::Button::new("Assistir")).clicked() { submitted = true; }
                    if input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { submitted = true; }
                    if !self.url.trim().is_empty() && ui.button("Abrir fora").clicked() {
                        match &valid { Ok((source, _)) => { let _ = open::that(source.external()); }, Err(error) => self.error = error.clone() }
                    }
                });
                if submitted {
                    match valid {
                        Ok((source, at)) => play = Some((source, if at > 0.0 { Some(at) } else { None })),
                        Err(error) => self.error = error,
                    }
                }
                if !self.url.trim().is_empty() && model::youtube(&self.url).is_err() && self.error.is_empty() {
                    ui.label(egui::RichText::new("Use o link de um vídeo do YouTube, incluindo Shorts ou transmissões.").size(12.0).color(p.muted));
                }
                if !self.error.is_empty() { ui.add_space(6.0); ui.colored_label(p.error, &self.error); }
                if let Some(source) = &self.failed_source {
                    if ui.button("Abrir este vídeo no aplicativo padrão").clicked() { let _ = open::that(source.external()); }
                }
                if !self.snapshot.error.is_empty() { ui.label(&self.snapshot.error); }
                ui.add_space(8.0);
                if self.current.is_some() && ui.button("Mostrar vídeo atual").clicked() { self.reveal(); }
                if !self.history.recent.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("Retomar").color(p.muted));
                    for recent in &self.history.recent {
                        ui.horizontal(|ui| {
                            let name = if recent.title.is_empty() { recent.source.label() } else { recent.title.clone() };
                            let secs = recent.position.max(0.0) as u64;
                            let label = format!("{} · {}:{:02}", name, secs / 60, secs % 60);
                            let width = (ui.available_width() - 36.0).max(100.0);
                            if ui.add_sized([width, 28.0], egui::Button::new(egui::RichText::new(label).size(13.0)).truncate()).on_hover_text(recent.source.external()).clicked() { play = Some((recent.source.clone(), None)); }
                            if kit::icon_button(ui, icons::EXTERNAL_LINK, "Abrir no navegador ou app padrão", false, p).clicked() { let _ = open::that(recent.source.external()); }
                        });
                    }
                    if ui.small_button(if self.current.is_some() { "Limpar anteriores" } else { "Limpar recentes" }).clicked() {
                        self.history.recent.retain(|r| self.current.as_ref().is_some_and(|c| c.source == r.source));
                        self.save();
                    }
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Arraste um vídeo para esta janela. Os arquivos ficam onde estão.").size(12.0).color(p.muted));
                if !cfg!(target_os = "macos") { ui.label("O player integrado está disponível no macOS. Neste sistema, use Abrir fora."); }
            });
        self.open = open && self.open;
        self.rect = window.as_ref().map(|w| w.response.rect);
        // Drop is scoped to this window; notes retain their attachment behavior.
        if self.captures_drop(ctx) {
            let files = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>());
            if let Some(path) = files.into_iter().find(|p| model::is_video(p)) { play = Some((Source::Local { path }, None)); }
        }
        if pick { self.choose_file(ctx); }
        if let Some((source, at)) = play { self.play(source, at); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_fits_small_workspace_and_limits_drop_target() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let mut video = Video::default();
        video.history.recent.clear();
        for i in 0..8 {
            video.history.remember(Source::Local { path: PathBuf::from(format!("/example/movie-{i}.mov")) }, "Um título muito longo para uma referência de vídeo durante a escrita".repeat(3), 42.0);
        }
        video.open_dialog();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 400.0));
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ctx| {
                video.ui(ctx, Palette::new(&Default::default()));
            });
        }
        let rect = video.rect.unwrap();
        assert!(rect.width() <= bounds.width(), "{rect:?}");
        assert!(rect.height() <= bounds.height(), "{rect:?}");
        video.open = false;
        assert!(!video.captures_drop(&ctx));
        assert!(video.backend.is_none(), "opening the selector must not start a media process");
    }
}
