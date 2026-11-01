//! Video accompanying any workspace, without occupying a document tab.
mod native;

use crate::{config, icons, kit, theme::Palette};
use ateliet_core::work_video::{self as model, History, Recent, Source};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Presentation { #[default] Closed, Docked, Collapsed, Detached }

pub struct Video {
    presentation: Presentation,
    switcher: bool,
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
    player_rect: Option<egui::Rect>,
    recent_expanded: bool,
}

fn state_path() -> PathBuf { config::app_config_path().with_file_name("video-session.json") }

impl Default for Video {
    fn default() -> Self {
        let saved = std::fs::read_to_string(state_path()).unwrap_or_default();
        let mut history: History = serde_json::from_str(&saved).unwrap_or_default();
        history.sanitize();
        Self {
            presentation: Presentation::Closed, switcher: true, history, current: None, backend: None, snapshot: Default::default(),
            url: String::new(), error: String::new(), focus_url: false, picker: None,
            failed_source: None,
            last_poll: Instant::now(), last_save: Instant::now(), saved,
            rect: None, player_rect: None, recent_expanded: false,
        }
    }
}

impl Video {
    pub fn active(&self) -> bool { self.presentation != Presentation::Closed }
    pub fn docked(&self) -> bool { self.presentation == Presentation::Docked }

    pub fn captures_drop(&self, ctx: &egui::Context) -> bool {
        self.docked() && self.rect.is_some_and(|r| ctx.input(|i| {
            i.pointer.latest_pos().is_some_and(|pos| r.contains(pos)) &&
                i.raw.dropped_files.iter().any(|f| f.path.as_ref().is_some_and(|p| model::is_video(p)))
        }))
    }

    pub fn reveal(&mut self) {
        let resume = self.presentation == Presentation::Collapsed;
        self.presentation = Presentation::Docked;
        self.switcher = self.current.is_none();
        if resume { if let Some(backend) = &mut self.backend { backend.play(); self.snapshot.intent = true; } }
        if self.current.is_none() { self.focus_url = true; }
    }

    pub fn choose_source(&mut self) {
        self.presentation = Presentation::Docked;
        self.switcher = true; self.focus_url = true;
    }

    pub fn toggle_dock(&mut self) {
        if self.docked() { self.collapse(); } else { self.reveal(); }
    }

    fn collapse(&mut self) {
        if let Some(backend) = &mut self.backend { backend.pause(); }
        self.presentation = if self.current.is_some() { Presentation::Collapsed } else { Presentation::Closed };
        self.snapshot.playing = false; self.snapshot.intent = false;
        self.rect = None; self.player_rect = None;
    }

    fn close(&mut self) {
        self.collect(); self.save(); self.backend = None; self.current = None;
        self.snapshot = Default::default(); self.presentation = Presentation::Closed;
        self.rect = None; self.player_rect = None;
    }

    pub fn play_local(&mut self, path: PathBuf) {
        self.play(Source::Local { path }, None);
    }

    fn collect(&mut self) {
        let Some(backend) = &self.backend else { return };
        let Some(snapshot) = backend.snapshot() else { return };
        self.snapshot = snapshot;
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
        match self.snapshot.presentation.as_str() {
            "docked" => { self.presentation = Presentation::Docked; self.switcher = false; }
            "collapsed" => self.presentation = Presentation::Collapsed,
            _ => {}
        }
    }

    fn play(&mut self, source: Source, position: Option<f64>) {
        if !source.valid() { self.error = "Fonte de vídeo inválida.".into(); self.presentation = Presentation::Docked; self.switcher = true; return; }
        if let Source::Local { path } = &source {
            if !path.is_file() {
                self.error = "O arquivo foi movido ou não está disponível. Escolha-o novamente.".into();
                self.presentation = Presentation::Docked; self.switcher = true;
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
            self.error = error; self.failed_source = Some(source); self.presentation = Presentation::Docked; self.switcher = true; return;
        }
        self.current = Some(Recent { source: source.clone(), title: title.clone(), position: at });
        self.history.remember(source, title, at);
        self.snapshot = native::Snapshot { position: at, intent: true, ..Default::default() };
        self.error.clear(); self.presentation = Presentation::Docked; self.switcher = false;
        self.failed_source = None;
        self.last_poll = Instant::now() - Duration::from_secs(2);
        self.save();
    }

    fn retry(&mut self) {
        self.collect();
        if let Some(current) = &self.current {
            let source = current.source.clone(); let at = current.position;
            self.play(source, Some(at));
        }
    }

    pub fn toggle(&mut self) {
        if !self.snapshot.error.is_empty() && self.current.is_some() { self.retry(); return; }
        if matches!(self.presentation, Presentation::Collapsed | Presentation::Closed) { self.reveal(); }
        else if let Some(backend) = &mut self.backend {
            backend.toggle();
            self.snapshot.intent = !(self.snapshot.playing || self.snapshot.intent);
            self.snapshot.playing = self.snapshot.intent;
        }
        self.last_poll = Instant::now() - Duration::from_secs(2);
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
        // The previous frame's card is also the drop target used by App.
        if self.captures_drop(ctx) {
            let path = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).find(|p| model::is_video(p)));
            if let Some(path) = path { self.play_local(path); }
        }
        self.rect = None; self.player_rect = None;
        if let Some(rx) = &self.picker {
            match rx.try_recv() {
                Ok(path) => { self.picker = None; if let Some(path) = path { self.play_local(path); } }
                Err(mpsc::TryRecvError::Disconnected) => { self.picker = None; }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.backend.as_ref().is_some_and(|b| b.has_event() || self.last_poll.elapsed() >= Duration::from_secs(1)) {
            let was_visible = self.snapshot.visible;
            self.collect();
            self.last_poll = Instant::now();
            if (was_visible && !self.snapshot.visible) || self.last_save.elapsed() >= Duration::from_secs(15) { self.save(); }
        }
        if self.backend.is_some() { ctx.request_repaint_after(if self.presentation == Presentation::Detached { Duration::from_millis(250) } else { Duration::from_secs(1) }); }
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

    /// A temporary rail is needed only when the workspace has no support panel.
    pub fn rail(&mut self, ctx: &egui::Context, p: Palette) {
        if !self.docked() { return; }
        let max = (ctx.available_rect().width() - 320.0).clamp(240.0, 520.0);
        egui::SidePanel::right("work-video-rail").resizable(true)
            .default_width(340.0_f32.min(max)).width_range(240.0..=max)
            .frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(10))
            .show(ctx, |ui| self.dock(ui, p));
    }

    fn action_menu(&mut self, ui: &mut egui::Ui, p: Palette, panel_left: Option<f32>) {
        let id = ui.make_persistent_id("video-actions");
        let response = kit::icon_button(ui, icons::ELLIPSIS, "Opções do vídeo", false, p);
        if response.clicked() { ui.memory_mut(|m| m.toggle_popup(id)); }
        let mut anchor = response.clone();
        let placement = if let Some(left) = panel_left {
            // Open beside the support column, leaving the official player
            // unobscured and playing while its options are being used.
            anchor.rect = egui::Rect::from_min_size(egui::pos2((left - 240.0).max(ui.ctx().screen_rect().left()), response.rect.top()), egui::vec2(232.0, 0.0));
            egui::AboveOrBelow::Below
        } else { egui::AboveOrBelow::Above };
        egui::popup::popup_above_or_below_widget(ui, id, &anchor, placement, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(180.0);
            if ui.button("Trocar vídeo…").clicked() {
                self.choose_source(); ui.memory_mut(|m| m.close_popup());
            }
            if self.current.is_some() {
                if ui.button(if self.presentation == Presentation::Detached { "Encaixar no painel" } else { "Destacar vídeo" }).clicked() {
                    self.presentation = if self.presentation == Presentation::Detached { Presentation::Docked } else { Presentation::Detached };
                    ui.memory_mut(|m| m.close_popup());
                }
                if ui.button("Abrir no aplicativo padrão").clicked() {
                    if let Some(backend) = &mut self.backend { backend.pause(); }
                    let _ = open::that(self.current.as_ref().unwrap().source.external()); ui.memory_mut(|m| m.close_popup());
                }
                ui.separator();
            }
            if ui.button("Encerrar vídeo").clicked() { self.close(); ui.memory_mut(|m| m.close_popup()); }
        });
    }

    fn title(&self) -> String {
        self.current.as_ref().map(|r| if r.title.is_empty() { r.source.label() } else { r.title.clone() }).unwrap_or_else(|| "Vídeo".into())
    }

    fn playback_label(&self) -> &'static str {
        if self.current.is_none() { "Arquivo do computador ou YouTube" }
        else if !self.snapshot.error.is_empty() { "Não foi possível reproduzir" }
        else if self.presentation == Presentation::Collapsed { "Pausado · recolhido" }
        else if !self.snapshot.ready { "Conectando" }
        else if self.snapshot.suspended { "Em pausa temporária" }
        else if self.snapshot.buffering { "Aguardando dados" }
        else if self.snapshot.playing { "Reproduzindo" }
        else { "Pausado" }
    }

    fn title_ui(&mut self, ui: &mut egui::Ui, width: f32, p: Palette) -> egui::Response {
        let title = self.title();
        let detail = format!("{}\n{}", title, self.playback_label());
        ui.allocate_ui_with_layout(egui::vec2(width.max(20.0), 28.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(egui::RichText::new(&title).font(kit::medium(12.5)).color(p.text)).truncate().sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(detail)
        }).inner
    }

    /// One quiet strip; restoring never creates a second player.
    pub fn compact_bar(&mut self, ctx: &egui::Context, p: Palette) {
        if !matches!(self.presentation, Presentation::Collapsed | Presentation::Detached) { return; }
        egui::TopBottomPanel::bottom("work-video-compact").exact_height(36.0)
            .frame(egui::Frame::new().fill(p.bg_secondary).inner_margin(egui::Margin::symmetric(10, 3)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if kit::icon_button(ui, if self.snapshot.playing || self.snapshot.intent { icons::PAUSE } else { icons::PLAY }, "Reproduzir / pausar", false, p).clicked() { self.toggle(); }
                    let width = ui.available_width() - 72.0;
                    if self.title_ui(ui, width, p).clicked() { self.reveal(); }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.action_menu(ui, p, None);
                        if kit::icon_button(ui, icons::PANEL_RIGHT_OPEN, "Mostrar vídeo no painel", false, p).clicked() { self.reveal(); }
                    });
                });
            });
    }

    pub fn dock(&mut self, ui: &mut egui::Ui, p: Palette) {
        if !self.docked() { return; }
        let start = ui.cursor().min;
        let playing = self.snapshot.error.is_empty() && (self.snapshot.playing || self.snapshot.intent);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if self.current.is_some() {
                if kit::icon_button(ui, if playing { icons::PAUSE } else { icons::PLAY }, "Reproduzir / pausar", false, p).clicked() { self.toggle(); }
            } else { ui.label(kit::icon(icons::FILM, 16.0).color(p.faint)); }
            let width = ui.available_width() - 72.0;
            if self.title_ui(ui, width, p).clicked() { self.switcher = !self.switcher; self.focus_url = self.switcher; }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, icons::MINUS, "Recolher e pausar", false, p).clicked() { self.collapse(); }
                self.action_menu(ui, p, Some(start.x));
            });
        });
        if !self.docked() { return; }
        if self.current.is_some() {
            ui.add_space(4.0);
            let minimum = 200.0 / ui.ctx().zoom_factor();
            let width = ui.available_width();
            let max = (ui.available_height() - 160.0).min(720.0).max(minimum);
            let preferred = if self.history.height > 0.0 { self.history.height } else { (width * 9.0 / 16.0).max(minimum) };
            let height = preferred.clamp(minimum, max);
            if width >= minimum && ui.available_height() >= height + 24.0 {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(18, 18, 18));
                // AVKit/WebKit own their loading indicator. An egui label
                // behind a ready native view would look like a reload on hide.
                self.player_rect = ui.clip_rect().contains_rect(rect).then_some(rect);
                let (divider, response) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::drag());
                let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text("Arraste para ajustar a altura · dois cliques para restaurar");
                if response.hovered() || response.dragged() {
                    ui.painter().hline(egui::Rangef::new(divider.center().x - 18.0, divider.center().x + 18.0), divider.center().y, egui::Stroke::new(2.0_f32, p.border_strong));
                }
                if response.dragged() { self.history.height = (height + ui.input(|i| i.pointer.delta().y)).clamp(minimum, max); }
                if response.double_clicked() { self.history.height = 0.0; self.save(); }
                if response.drag_stopped() { self.save(); }
            } else {
                ui.label(egui::RichText::new("Amplie o painel para ver o vídeo.").size(12.0).color(p.muted));
            }
        }
        if !self.error.is_empty() { ui.colored_label(p.error, &self.error); }
        if !self.snapshot.error.is_empty() {
            ui.label(egui::RichText::new(&self.snapshot.error).size(12.0).color(p.muted));
            ui.horizontal(|ui| {
                if kit::primary(ui, "Tentar novamente", p).clicked() {
                    self.retry();
                }
                if ui.add(egui::Button::new("Trocar vídeo").frame(false)).clicked() { self.switcher = true; self.focus_url = true; }
            });
        }
        if self.switcher || self.current.is_none() {
            let max = ui.available_height().min(300.0).max(60.0);
            egui::ScrollArea::vertical().id_salt("work-video-sources").max_height(max).auto_shrink([false, true])
                .show(ui, |ui| self.sources(ui, p));
        }
        self.rect = Some(egui::Rect::from_min_max(start, egui::pos2(ui.max_rect().right(), ui.cursor().top())));
        ui.add_space(4.0); ui.separator(); ui.add_space(6.0);
    }

    fn sources(&mut self, ui: &mut egui::Ui, p: Palette) {
        let mut play = None;
        let mut pick = false;
        ui.spacing_mut().item_spacing.y = 7.0;
        if ui.add_enabled(self.picker.is_none(), egui::Button::new("Abrir arquivo…").min_size(egui::vec2(ui.available_width(), 32.0))).clicked() { pick = true; }
        ui.label(egui::RichText::new("YouTube").size(12.0).color(p.muted));
        let valid = model::youtube(&self.url);
        let mut submitted = false;
        ui.horizontal(|ui| {
            let input = ui.add_sized([(ui.available_width() - 28.0 - ui.spacing().item_spacing.x).max(40.0), 28.0], egui::TextEdit::singleline(&mut self.url).hint_text("Cole um link do YouTube"));
            if self.focus_url {
                input.request_focus(); self.focus_url = false;
                let mut state = egui::TextEdit::load_state(ui.ctx(), input.id).unwrap_or_default();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.url.chars().count()))));
                state.store(ui.ctx(), input.id);
            }
            if input.changed() { self.error.clear(); }
            submitted = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            submitted |= ui.add_enabled_ui(valid.is_ok(), |ui| kit::icon_button(ui, icons::PLAY, "Reproduzir vídeo", false, p)).inner.clicked();
        });
        if submitted {
            match model::youtube(&self.url) {
                Ok((source, at)) => play = Some((source, if at > 0.0 { Some(at) } else { None })),
                Err(error) => self.error = error,
            }
        }
        if !self.url.trim().is_empty() && valid.is_err() {
            ui.label(egui::RichText::new("Use o link de um vídeo, Short ou transmissão do YouTube.").size(11.0).color(p.faint));
        }
        if let Some(source) = &self.failed_source {
            if ui.small_button("Abrir no aplicativo padrão").clicked() { let _ = open::that(source.external()); }
        }
        ui.label(egui::RichText::new("Ou arraste um vídeo para cá.").size(11.0).color(p.faint));
        if !self.history.recent.is_empty() {
            ui.add_space(4.0);
            kit::section(ui, "Retomar", p);
            let recent_limit = if self.recent_expanded { model::RECENT_LIMIT } else { 3 };
            for recent in self.history.recent.iter().filter(|r| !self.current.as_ref().is_some_and(|c| c.source == r.source)).take(recent_limit) {
                let name = if recent.title.is_empty() { recent.source.label() } else { recent.title.clone() };
                let secs = recent.position.max(0.0) as u64;
                let label = format!("{} · {}:{:02}", name, secs / 60, secs % 60);
                if ui.add_sized([ui.available_width(), 30.0], egui::Button::new(egui::RichText::new(label).size(12.0)).frame(false).truncate()).on_hover_text(format!("{}\n{}", name, recent.source.external())).clicked() { play = Some((recent.source.clone(), None)); }
            }
            if self.history.recent.len() > 3 && ui.small_button(if self.recent_expanded { "Mostrar menos" } else { "Ver todos" }).clicked() { self.recent_expanded = !self.recent_expanded; }
            if self.recent_expanded && ui.small_button("Limpar recentes").clicked() {
                self.history.recent.retain(|r| self.current.as_ref().is_some_and(|c| c.source == r.source)); self.save();
            }
        }
        if !cfg!(target_os = "macos") { ui.label("Player integrado disponível no macOS. Use Abrir fora neste sistema."); }
        if pick { self.choose_file(ui.ctx()); }
        if let Some((source, at)) = play { self.play(source, at); }
    }

    /// Tooltips never occlude the media. A translated paint layer lasts only
    /// this pass, so no persistent hit-test transform or stale position leaks.
    fn presentation_rect(&self, ctx: &egui::Context) -> Option<egui::Rect> {
        let rect = self.player_rect?;
        if !self.docked() || !ctx.screen_rect().contains_rect(rect) { return None; }
        let layers = ctx.memory(|m| m.areas().visible_layer_ids().iter().filter_map(|layer| {
            if layer.order == egui::Order::Background { None } else { m.area_rect(layer.id).map(|r| (*layer, r)) }
        }).collect::<Vec<_>>());
        let mut obscured = false;
        for (layer, area) in layers {
            if !area.intersects(rect) { continue; }
            if layer.order == egui::Order::Tooltip {
                let target = tooltip_position(ctx.screen_rect(), rect, area);
                ctx.transform_layer_shapes(layer, egui::emath::TSTransform::from_translation(target.min - area.min));
            } else { obscured = true; }
        }
        if obscured { None } else { Some(rect) }
    }

    /// Run after all egui dialogs/menus so a native child never covers them.
    pub fn sync(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if ctx.will_discard() { return; }
        let rect = self.presentation_rect(ctx);
        let mode = if self.presentation == Presentation::Detached { 2 } else if rect.is_some() { 1 } else { 0 };
        // Supply stable geometry even while a modal suspends presentation.
        if let Some(backend) = &mut self.backend { backend.place(frame, ctx.screen_rect(), self.player_rect, mode); }
    }
}

fn tooltip_position(screen: egui::Rect, video: egui::Rect, tooltip: egui::Rect) -> egui::Rect {
    let size = tooltip.size().min(screen.size());
    let candidates = [
        egui::pos2(tooltip.left(), video.top() - size.y - 8.0),
        egui::pos2(video.left() - size.x - 8.0, tooltip.top()),
        egui::pos2(tooltip.left(), video.bottom() + 8.0),
    ];
    for min in candidates {
        let min = min.clamp(screen.min, screen.max - size);
        let candidate = egui::Rect::from_min_size(min, size);
        if !candidate.intersects(video) { return candidate; }
    }
    // At extreme zoom there may be no free side; pin the tip to the far
    // corner. It still must not suspend or recreate the media session.
    egui::Rect::from_min_size(screen.min, size)
}

#[cfg(test)]
mod tests;
