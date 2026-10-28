//! The notes' typography as the theme sets it: heading sizes, weights and
//! colours, line height, paragraph spacing, the readable line width, the
//! colours of bold, italic, links, quote borders and bullets, and the
//! fonts (looked up on the system, or taken from the theme's CSS, in the
//! background). Read by the reading view, Live Preview and the editor.

use crate::config::Appearance;
use crate::fonts::ThemeFonts;
use ateliet_core::css_theme::{self as ct, Typography};
use eframe::egui::{self, Color32};
use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone, Debug, PartialEq)]
pub struct Typo {
    /// Heading sizes as multiples of the base size.
    pub heading: [f32; 6],
    pub heading_bold: [bool; 6],
    pub heading_color: [Option<Color32>; 6],
    /// Line height of body text, as a multiple of its size.
    pub line: f32,
    /// Space between blocks, as a multiple of the base size.
    pub gap: f32,
    /// The readable line width in pixels (the app's own when `None`).
    pub line_width: Option<f32>,
    pub bold: Option<Color32>,
    pub italic: Option<Color32>,
    pub link: Option<Color32>,
    pub quote: Option<Color32>,
    pub quote_width: f32,
    pub bullet: Option<Color32>,
    pub highlight: Option<Color32>,
    /// Code size, as a multiple of the base size.
    pub code: f32,
}

impl Default for Typo {
    fn default() -> Self {
        Typo {
            heading: [1.6, 1.32, 1.16, 1.05, 1.0, 0.92],
            heading_bold: [true; 6],
            heading_color: [None; 6],
            line: 1.62,
            gap: 0.7,
            line_width: None,
            bold: None,
            italic: None,
            link: None,
            quote: None,
            quote_width: 2.0,
            bullet: None,
            highlight: None,
            code: 0.88,
        }
    }
}

static TYPO: Lazy<RwLock<Typo>> = Lazy::new(|| RwLock::new(Typo::default()));

/// The typography in use.
pub fn get() -> Typo {
    TYPO.read().map(|t| t.clone()).unwrap_or_default()
}

fn color(c: Option<[u8; 4]>) -> Option<Color32> {
    c.filter(|c| c[3] > 0).map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
}

pub fn from_theme(t: &Typography) -> Typo {
    let d = Typo::default();
    let mut out = d.clone();
    for i in 0..6 {
        out.heading[i] = t.heading_size[i].unwrap_or(d.heading[i]);
        out.heading_bold[i] = t.heading_weight[i].is_none_or(|w| w >= 550);
        out.heading_color[i] = color(t.heading_color[i]);
    }
    out.line = t.line_height.unwrap_or(d.line);
    // the paragraph spacing of CSS is a margin; the reading view's gap is a bit less than one line
    out.gap = t.paragraph_spacing.map(|p| p * 0.7).unwrap_or(d.gap);
    out.line_width = t.line_width;
    out.bold = color(t.bold_color);
    out.italic = color(t.italic_color);
    out.link = color(t.link_color);
    out.quote = color(t.quote_border);
    out.quote_width = t.quote_border_width.unwrap_or(d.quote_width);
    out.bullet = color(t.bullet_color);
    out.highlight = color(t.highlight);
    out.code = t.code_size.unwrap_or(d.code);
    out
}

/// The fonts last asked for (their names), not to look them up again.
static FONTS_KEY: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

/// Take the appearance's typography; its fonts load in the background and
/// replace the families' fonts when found.
pub fn apply(ctx: &egui::Context, a: &Appearance) {
    let t = a.theme_vars.as_ref().map(|v| ct::typography(if a.dark { &v.1 } else { &v.0 }, a.font_size)).unwrap_or_default();
    if let Ok(mut w) = TYPO.write() {
        *w = from_theme(&t);
    }
    let key = format!("{:?}|{:?}|{:?}|{}", t.text_font, t.mono_font, t.interface_font, a.theme_fonts.len());
    {
        let Ok(mut k) = FONTS_KEY.lock() else { return };
        if *k == key {
            return;
        }
        *k = key;
    }
    let embedded = a.theme_fonts.clone();
    let c = ctx.clone();
    std::thread::spawn(move || {
        let found = find_fonts(&t, &embedded);
        // without a theme font, the bundled families as they were
        c.set_fonts(crate::fonts::definitions_with(&found));
        c.request_repaint();
    });
}

/// A face of the first family of `list` there is: in the theme's CSS, or on
/// the system (or among the bundled fonts).
fn face(list: &[String], italic: bool, weight: u16, embedded: &[ct::EmbeddedFont]) -> Option<(Arc<Vec<u8>>, u32)> {
    for fam in list {
        let mine: Vec<&ct::EmbeddedFont> = embedded.iter().filter(|f| f.family.eq_ignore_ascii_case(fam)).collect();
        if let Some(f) = mine.iter().min_by_key(|f| (if f.italic != italic { 10_000 } else { 0 }) + (f.weight as i32 - weight as i32).unsigned_abs()) {
            return Some((Arc::new(f.data.clone()), 0));
        }
        // Inter is what the app uses already
        if fam.eq_ignore_ascii_case("inter") {
            return None;
        }
        let db = crate::fonts::book_fonts();
        if let Some(id) = db.find(fam, italic, weight) {
            let f = db.face(id);
            if let Some(data) = f.data() {
                return Some((data, f.index));
            }
        }
    }
    None
}

fn find_fonts(t: &Typography, embedded: &[ct::EmbeddedFont]) -> ThemeFonts {
    let mut out = ThemeFonts::default();
    if !t.text_font.is_empty() {
        out.text = [face(&t.text_font, false, 400, embedded), face(&t.text_font, false, 700, embedded), face(&t.text_font, true, 400, embedded)];
    }
    if !t.mono_font.is_empty() {
        out.mono = face(&t.mono_font, false, 400, embedded);
    }
    if !t.interface_font.is_empty() {
        out.interface = [face(&t.interface_font, false, 400, embedded), face(&t.interface_font, false, 600, embedded), face(&t.interface_font, true, 400, embedded)];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_values_or_the_defaults() {
        assert_eq!(from_theme(&Typography::default()), Typo::default());
        let t = Typography { heading_size: [Some(2.0), None, None, None, None, None], heading_weight: [Some(400), Some(700), None, None, None, None], line_height: Some(1.8), paragraph_spacing: Some(1.0), ..Default::default() };
        let x = from_theme(&t);
        assert_eq!(x.heading[0], 2.0);
        assert_eq!(x.heading[1], 1.32);
        assert_eq!(x.heading_bold[..2], [false, true]);
        assert_eq!(x.line, 1.8);
        assert!((x.gap - 0.7).abs() < 1e-6);
    }
}
