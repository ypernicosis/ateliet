//! Focus mode: only the text, a small HUD (words this session, in the
//! chapter, the daily goal), dimmed paragraphs, typewriter scrolling and
//! timed sprints. Sprints keep running outside focus mode.

use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::scriptorium::fmt_n;
use eframe::egui::{self, Color32, RichText, Sense};
use serde_json::json;
use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

thread_local! {
    /// Read by the Live Preview while drawing: dim the other blocks, keep the caret centred.
    pub static DIM: Cell<bool> = const { Cell::new(false) };
    pub static TYPEWRITER: Cell<bool> = const { Cell::new(false) };
    /// Focus mode is on (the note shows without its properties).
    pub static ON: Cell<bool> = const { Cell::new(false) };
}

pub struct Focus {
    pub started: Instant,
    /// Words of each chapter the first time it was seen in the session.
    base: HashMap<String, i64>,
    last_input: Instant,
}

pub struct Sprint {
    pub minutes: u32,
    pub goal: i64,
    end: Instant,
    paused_left: Option<Duration>,
    words0: i64,
}

impl Focus {
    pub fn new() -> Focus {
        Focus { started: Instant::now(), base: HashMap::new(), last_input: Instant::now() - Duration::from_secs(10) }
    }

    /// Note the chapter's words the first time it shows up.
    pub fn see(&mut self, path: &str, words: i64) {
        self.base.entry(path.to_string()).or_insert(words);
    }

    pub fn session_words(&self, live: &dyn Fn(&str) -> Option<i64>) -> i64 {
        self.base.iter().filter_map(|(p, w0)| live(p).map(|w| w - w0)).sum()
    }
}

impl Default for Focus {
    fn default() -> Self {
        Focus::new()
    }
}

impl Sprint {
    pub fn new(minutes: u32, goal: i64, words_now: i64) -> Sprint {
        Sprint { minutes, goal, end: Instant::now() + Duration::from_secs(minutes as u64 * 60), paused_left: None, words0: words_now }
    }

    pub fn left(&self) -> Duration {
        self.paused_left.unwrap_or_else(|| self.end.saturating_duration_since(Instant::now()))
    }

    pub fn toggle_pause(&mut self) {
        match self.paused_left.take() {
            Some(l) => self.end = Instant::now() + l,
            None => self.paused_left = Some(self.left()),
        }
    }

    pub fn paused(&self) -> bool {
        self.paused_left.is_some()
    }

    pub fn done(&self) -> bool {
        !self.paused() && self.left().is_zero()
    }

    pub fn gained(&self, words_now: i64) -> i64 {
        (words_now - self.words0).max(0)
    }

    pub fn clock(&self) -> String {
        let s = self.left().as_secs_f32().ceil() as u64;
        format!("{}:{:02}", s / 60, s % 60)
    }

    /// The record kept in the plugin's settings.
    pub fn record(&self, words_now: i64, early: bool) -> serde_json::Value {
        let spent = ((self.minutes as f32 * 60.0 - self.left().as_secs_f32()) / 60.0).round() as u32;
        json!({ "d": ateliet_core::scriptorium::stats::today(), "min": if early { spent } else { self.minutes }, "w": self.gained(words_now) })
    }
}

pub enum HudAction {
    Exit,
    ToggleDim,
    ToggleTypewriter,
    StartSprint(u32, i64),
    PauseSprint,
    StopSprint,
}

/// A ring that fills with `frac`.
fn ring(ui: &mut egui::Ui, frac: f32, p: Palette, label: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), Sense::hover());
    let c = rect.center();
    let rad = 14.0;
    ui.painter().circle_stroke(c, rad, egui::Stroke::new(3.0_f32, p.bg_active));
    let n = 48;
    let end = (frac.clamp(0.0, 1.0) * n as f32) as usize;
    if end > 0 {
        let pts: Vec<egui::Pos2> = (0..=end).map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / n as f32 * std::f32::consts::TAU;
            c + egui::vec2(a.cos(), a.sin()) * rad
        }).collect();
        ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(3.0_f32, p.accent_strong)));
    }
    ui.painter().text(c, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(if label.chars().count() > 3 { 9.5 } else { 10.5 }), p.muted);
    r
}

#[allow(clippy::too_many_arguments)]
pub fn hud(ctx: &egui::Context, p: Palette, session: i64, chapter: Option<i64>, today: i64, goal: i64, dim: bool, typewriter: bool, sprint: Option<&Sprint>, words_now: i64, f: &mut Focus) -> Option<HudAction> {
    let mut act = None;
    // the HUD fades while typing and comes back when the mouse moves
    if ctx.input(|i| !i.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_))) && i.events.iter().any(|e| matches!(e, egui::Event::Text(_) | egui::Event::Key { .. }))) {
        f.last_input = Instant::now();
    }
    if ctx.input(|i| i.pointer.delta().length() > 0.5) {
        f.last_input = Instant::now() - Duration::from_secs(10);
    }
    let typing = f.last_input.elapsed() < Duration::from_secs(3);
    let alpha = ctx.animate_bool_with_time(egui::Id::new("hud-visible"), !typing, 0.25);
    if typing {
        ctx.request_repaint_after(Duration::from_millis(500));
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.memory(|m| m.focused().is_some() || m.any_popup_open()) {
        act = Some(HudAction::Exit);
    }
    let work = ctx.available_rect();
    egui::Area::new(egui::Id::new("focus-hud")).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(egui::pos2(work.center().x, work.bottom() - 22.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        ui.set_opacity(0.25 + 0.75 * alpha);
        egui::Frame::new().fill(p.bg).stroke(egui::Stroke::new(1.0_f32, p.border)).corner_radius(22.0).inner_margin(egui::Margin { left: 14, right: 8, top: 6, bottom: 6 }).shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if p.dark { 80 } else { 22 }) }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(format!("{}{} nesta sessão", if session >= 0 { "+" } else { "" }, fmt_n(session))).font(kit::medium(12.5)).color(p.text));
                    if let Some(c) = chapter {
                        ui.label(RichText::new(format!("{} no capítulo", fmt_n(c))).size(11.5).color(p.faint));
                    }
                });
                if goal > 0 {
                    let frac = today as f32 / goal as f32;
                    let label = if frac >= 1.0 { "✓".to_string() } else { format!("{:.0}%", frac * 100.0) };
                    ring(ui, frac, p, &label).on_hover_text(format!("{} de {} palavras hoje", fmt_n(today), fmt_n(goal)));
                }
                if let Some(s) = sprint {
                    let r = egui::Frame::new().fill(if s.paused() { p.bg_active } else { p.accent_soft }).corner_radius(14.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(kit::icon(if s.paused() { icons::PAUSE } else { icons::TIMER }, 12.0).color(p.accent_strong));
                            ui.label(RichText::new(s.clock()).font(kit::medium(12.5)).color(p.text));
                            let g = s.gained(words_now);
                            ui.label(RichText::new(if s.goal > 0 { format!("{}/{}", fmt_n(g), fmt_n(s.goal)) } else { format!("+{}", fmt_n(g)) }).size(11.5).color(p.muted));
                        });
                    });
                    let r = ui.interact(r.response.rect, egui::Id::new("sprint-pill"), Sense::click()).on_hover_text("Pausar ou encerrar");
                    r.context_menu(|ui| {
                        if kit::menu_item(ui, if s.paused() { icons::PLAY } else { icons::PAUSE }, if s.paused() { "Retomar" } else { "Pausar" }, p) {
                            act = Some(HudAction::PauseSprint);
                            ui.close_menu();
                        }
                        if kit::menu_item(ui, icons::CIRCLE_STOP, "Encerrar agora", p) {
                            act = Some(HudAction::StopSprint);
                            ui.close_menu();
                        }
                    });
                    if r.clicked() {
                        act = Some(HudAction::PauseSprint);
                    }
                }
                ui.add_space(2.0);
                ui.spacing_mut().item_spacing.x = 0.0;
                if kit::icon_button(ui, icons::EYE, "Esmaecer os outros parágrafos", dim, p).clicked() {
                    act = Some(HudAction::ToggleDim);
                }
                if kit::icon_button(ui, icons::ALIGN_CENTER, "Máquina de escrever (linha sempre no centro)", typewriter, p).clicked() {
                    act = Some(HudAction::ToggleTypewriter);
                }
                let tr = kit::icon_button(ui, icons::TIMER, "Sprint", false, p);
                let popup = ui.make_persistent_id("sprint-menu");
                if tr.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(popup));
                }
                egui::popup_above_or_below_widget(ui, popup, &tr, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    ui.set_min_width(170.0);
                    for m in [10, 15, 25, 45, 60] {
                        if kit::Row::new(&format!("{m} minutos")).icon(icons::TIMER).show(ui, p).clicked() {
                            act = Some(HudAction::StartSprint(m, 0));
                        }
                    }
                    if kit::Row::new("25 min · meta de 500").icon(icons::TARGET).show(ui, p).clicked() {
                        act = Some(HudAction::StartSprint(25, 500));
                    }
                });
                if kit::icon_button(ui, icons::X, "Sair do foco (Esc)", false, p).clicked() {
                    act = Some(HudAction::Exit);
                }
            });
        });
    });
    act
}

/// The running sprint in the status bar (outside focus mode).
pub fn status_pill(ui: &mut egui::Ui, p: Palette, s: &Sprint, words_now: i64) -> egui::Response {
    ui.label(RichText::new(format!("{}{} · +{}", s.clock(), if s.paused() { " (pausa)" } else { "" }, fmt_n(s.gained(words_now)))).size(11.5).color(p.accent_strong)).interact(Sense::click()).on_hover_text("Sprint: clique para pausar")
}
