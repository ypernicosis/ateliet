//! Colours and text styles. A quiet, paper-like look: white page, warm
//! off-white chrome, hairline borders, one accent used sparingly. The accent
//! comes from the vault's settings; a darker shade of it is derived for text
//! and filled buttons so they stay readable whatever colour was picked.

use crate::config::Appearance;
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// The page (notes, the centre).
    pub bg: Color32,
    /// Sidebars, ribbon, tab strip.
    pub bg_secondary: Color32,
    pub bg_hover: Color32,
    /// Selected rows and tabs.
    pub bg_active: Color32,
    pub border: Color32,
    /// Outlines of buttons and fields.
    pub border_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    /// The vault's accent as picked.
    pub accent: Color32,
    /// The accent, dark (or light) enough for text and filled buttons.
    pub accent_strong: Color32,
    /// A wash of the accent for selections and highlights.
    pub accent_soft: Color32,
    /// Text on an `accent_strong` fill.
    pub on_accent: Color32,
    pub code_bg: Color32,
    pub highlight: Color32,
    pub error: Color32,
    pub dark: bool,
}

fn lum(c: Color32) -> f32 {
    let f = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Move `c` towards `to` until it reaches `ratio` against `against`.
fn readable(c: Color32, against: Color32, to: Color32, ratio: f32) -> Color32 {
    let mut out = c;
    let mut t = 0.0;
    while contrast(out, against) < ratio && t < 1.0 {
        t += 0.04;
        out = mix(c, to, t);
    }
    out
}

impl Palette {
    pub fn new(a: &Appearance) -> Palette {
        let accent = Color32::from_rgb(a.accent[0], a.accent[1], a.accent[2]);
        if a.dark {
            let bg = Color32::from_rgb(0x19, 0x19, 0x19);
            let strong = readable(accent, bg, Color32::WHITE, 4.5);
            Palette {
                bg,
                bg_secondary: Color32::from_rgb(0x20, 0x20, 0x20),
                bg_hover: Color32::from_rgb(0x2a, 0x2a, 0x2a),
                bg_active: Color32::from_rgb(0x31, 0x31, 0x31),
                border: Color32::from_rgb(0x2e, 0x2e, 0x2e),
                border_strong: Color32::from_rgb(0x3a, 0x3a, 0x3a),
                text: Color32::from_rgb(0xe6, 0xe6, 0xe3),
                muted: Color32::from_rgb(0x9b, 0x9b, 0x97),
                faint: Color32::from_rgb(0x62, 0x62, 0x5f),
                accent,
                accent_strong: strong,
                accent_soft: mix(bg, accent, 0.22),
                on_accent: if contrast(strong, Color32::BLACK) > contrast(strong, Color32::WHITE) { Color32::from_rgb(0x12, 0x12, 0x12) } else { Color32::WHITE },
                code_bg: Color32::from_rgb(0x23, 0x23, 0x23),
                highlight: Color32::from_rgba_unmultiplied(0xff, 0xd0, 0x00, 0x40),
                error: Color32::from_rgb(0xf2, 0x6d, 0x6d),
                dark: true,
            }
        } else {
            let bg = Color32::WHITE;
            let strong = readable(accent, bg, Color32::from_rgb(0x10, 0x10, 0x18), 4.5);
            Palette {
                bg,
                bg_secondary: Color32::from_rgb(0xf7, 0xf7, 0xf5),
                bg_hover: Color32::from_rgb(0xee, 0xee, 0xeb),
                bg_active: Color32::from_rgb(0xe8, 0xe8, 0xe4),
                border: Color32::from_rgb(0xe9, 0xe9, 0xe6),
                border_strong: Color32::from_rgb(0xdc, 0xdc, 0xd8),
                text: Color32::from_rgb(0x1f, 0x1f, 0x1e),
                muted: Color32::from_rgb(0x6b, 0x6b, 0x67),
                faint: Color32::from_rgb(0xa5, 0xa5, 0xa0),
                accent,
                accent_strong: strong,
                accent_soft: mix(bg, accent, 0.16),
                on_accent: Color32::WHITE,
                code_bg: Color32::from_rgb(0xf5, 0xf5, 0xf3),
                highlight: Color32::from_rgba_unmultiplied(0xff, 0xd0, 0x00, 0x55),
                error: Color32::from_rgb(0xd9, 0x3f, 0x3f),
                dark: false,
            }
        }
    }

    /// Links in notes.
    pub fn link(&self) -> Color32 {
        self.accent_strong
    }
}

/// Interface text sizes (the notes follow the vault's own font size).
pub const UI: f32 = 13.5;
pub const UI_SMALL: f32 = 11.5;

pub fn apply(ctx: &egui::Context, a: &Appearance) -> Palette {
    let p = Palette::new(a);
    let mut v = if a.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg_secondary;
    v.window_fill = p.bg;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if a.dark { 90 } else { 28 }) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if a.dark { 80 } else { 22 }) };
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.extreme_bg_color = if a.dark { Color32::from_rgb(0x15, 0x15, 0x15) } else { p.bg };
    v.faint_bg_color = p.bg_secondary;
    v.code_bg_color = p.code_bg;
    v.hyperlink_color = p.link();
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0_f32, p.accent_strong);
    v.override_text_color = Some(p.text);
    v.striped = false;
    v.indent_has_left_vline = false;
    v.button_frame = true;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.resize_corner_size = 10.0;
    v.clip_rect_margin = 3.0;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg_secondary;
    w.noninteractive.weak_bg_fill = p.bg_secondary;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    // buttons are quiet: no fill until hovered
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_fill = p.bg_active;
    // secondary buttons and fields: a hairline, no fill
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.hovered.weak_bg_fill = p.bg_hover;
    w.hovered.bg_fill = p.bg_hover;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.hovered.expansion = 0.0;
    w.active.weak_bg_fill = p.bg_active;
    w.active.bg_fill = p.bg_active;
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.active.expansion = 0.0;
    w.open.weak_bg_fill = p.bg_hover;
    w.open.bg_fill = p.bg_hover;
    w.open.bg_stroke = Stroke::NONE;
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::same(6);
    }
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(UI_SMALL, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(UI, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(UI, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(20.0, FontFamily::Name("bold".into()))),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(10.0, 5.0);
    s.interact_size = egui::vec2(28.0, 26.0);
    s.menu_margin = Margin::same(6);
    s.window_margin = Margin::same(18);
    s.indent = 16.0;
    s.icon_width = 14.0;
    s.icon_width_inner = 8.0;
    s.icon_spacing = 6.0;
    s.combo_height = 280.0;
    s.text_edit_width = 240.0;
    s.slider_width = 120.0;
    s.scroll = egui::style::ScrollStyle::floating();
    s.scroll.bar_width = 8.0;
    s.scroll.floating_width = 3.0;
    s.scroll.floating_allocated_width = 0.0;
    s.scroll.foreground_color = false;
    s.scroll.dormant_handle_opacity = 0.0;
    s.scroll.active_handle_opacity = 0.5;
    s.scroll.interact_handle_opacity = 0.8;
    s.menu_spacing = 2.0;
    style.interaction.selectable_labels = false;
    style.interaction.tooltip_delay = 0.35;
    style.visuals.menu_corner_radius = CornerRadius::same(8);
    ctx.set_style(style);
    p
}
