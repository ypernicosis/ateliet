//! Colours and text styles, close to Obsidian's default theme.

use crate::config::Appearance;
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub bg_secondary: Color32,
    pub bg_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub code_bg: Color32,
    pub highlight: Color32,
    pub error: Color32,
    pub dark: bool,
}

impl Palette {
    pub fn new(a: &Appearance) -> Palette {
        let accent = Color32::from_rgb(a.accent[0], a.accent[1], a.accent[2]);
        if a.dark {
            Palette {
                bg: Color32::from_rgb(0x1e, 0x1e, 0x1e),
                bg_secondary: Color32::from_rgb(0x26, 0x26, 0x26),
                bg_hover: Color32::from_rgb(0x33, 0x33, 0x33),
                border: Color32::from_rgb(0x36, 0x36, 0x36),
                text: Color32::from_rgb(0xda, 0xda, 0xda),
                muted: Color32::from_rgb(0xb3, 0xb3, 0xb3),
                faint: Color32::from_rgb(0x66, 0x66, 0x66),
                accent,
                code_bg: Color32::from_rgb(0x2a, 0x2a, 0x2a),
                highlight: Color32::from_rgba_unmultiplied(0xff, 0xd0, 0x00, 0x50),
                error: Color32::from_rgb(0xfb, 0x46, 0x4c),
                dark: true,
            }
        } else {
            Palette {
                bg: Color32::from_rgb(0xff, 0xff, 0xff),
                bg_secondary: Color32::from_rgb(0xf6, 0xf6, 0xf6),
                bg_hover: Color32::from_rgb(0xea, 0xea, 0xea),
                border: Color32::from_rgb(0xe0, 0xe0, 0xe0),
                text: Color32::from_rgb(0x22, 0x22, 0x22),
                muted: Color32::from_rgb(0x5c, 0x5c, 0x5c),
                faint: Color32::from_rgb(0xab, 0xab, 0xab),
                accent,
                code_bg: Color32::from_rgb(0xf3, 0xf3, 0xf3),
                highlight: Color32::from_rgba_unmultiplied(0xff, 0xd0, 0x00, 0x66),
                error: Color32::from_rgb(0xe9, 0x31, 0x47),
                dark: false,
            }
        }
    }

    /// Accent colour readable as text on the background.
    pub fn link(&self) -> Color32 {
        self.accent
    }
}

pub fn apply(ctx: &egui::Context, a: &Appearance) -> Palette {
    let p = Palette::new(a);
    let mut v = if a.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg_secondary;
    v.window_fill = p.bg;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.bg_secondary;
    v.code_bg_color = p.code_bg;
    v.hyperlink_color = p.link();
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.override_text_color = Some(p.text);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.bg_fill = p.bg_secondary;
    v.widgets.inactive.bg_fill = p.bg_hover;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.hovered.weak_bg_fill = p.bg_hover;
    v.widgets.hovered.bg_fill = p.bg_hover;
    v.widgets.active.weak_bg_fill = p.accent.gamma_multiply(0.25);
    v.widgets.active.bg_fill = p.accent.gamma_multiply(0.35);
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(4);
    }
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    let base = a.font_size;
    style.text_styles = [
        (TextStyle::Small, FontId::new(base * 0.75, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(base * 0.85, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(base * 0.85, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(base * 1.25, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(base * 0.8, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    style.interaction.selectable_labels = false;
    ctx.set_style(style);
    p
}
