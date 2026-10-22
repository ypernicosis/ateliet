//! Text alignment of a note (left, justified, centre, right), kept in its
//! frontmatter as `alinhamento` and applied to the editor, Live Preview and
//! the reading view.

use eframe::egui::{self, text::LayoutJob, Galley};
use std::cell::Cell;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum TextAlign {
    #[default]
    Left,
    Justify,
    Center,
    Right,
}

pub const ALL: [TextAlign; 4] = [TextAlign::Left, TextAlign::Justify, TextAlign::Center, TextAlign::Right];

/// The frontmatter key.
pub const KEY: &str = "alinhamento";

impl TextAlign {
    /// From a frontmatter value (Portuguese or English spellings).
    pub fn parse(v: &str) -> TextAlign {
        match v.trim().to_lowercase().as_str() {
            "justificado" | "justificar" | "justify" | "justified" => TextAlign::Justify,
            "centro" | "centralizado" | "center" | "centre" => TextAlign::Center,
            "direita" | "right" => TextAlign::Right,
            _ => TextAlign::Left,
        }
    }

    /// The value written to the frontmatter (`None` for the default).
    pub fn value(self) -> Option<&'static str> {
        match self {
            TextAlign::Left => None,
            TextAlign::Justify => Some("justificado"),
            TextAlign::Center => Some("centro"),
            TextAlign::Right => Some("direita"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TextAlign::Left => "Alinhar à esquerda",
            TextAlign::Justify => "Justificar",
            TextAlign::Center => "Centralizar",
            TextAlign::Right => "Alinhar à direita",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            TextAlign::Left => crate::icons::ALIGN_LEFT,
            TextAlign::Justify => crate::icons::ALIGN_JUSTIFY,
            TextAlign::Center => crate::icons::ALIGN_CENTER,
            TextAlign::Right => crate::icons::ALIGN_RIGHT,
        }
    }

    /// The alignment a note asks for in its frontmatter.
    pub fn of_note(text: &str) -> TextAlign {
        let Some(fm) = ateliet_core::syntax::frontmatter(text) else { return TextAlign::Left };
        let yaml = &text[fm.yaml];
        for line in yaml.lines() {
            if let Some(rest) = line.strip_prefix(KEY) {
                if let Some(v) = rest.trim_start().strip_prefix(':') {
                    return TextAlign::parse(v.trim().trim_matches(['"', '\'']));
                }
            }
        }
        TextAlign::Left
    }

    /// The note's text with this alignment written to (or, for the default,
    /// removed from) its frontmatter.
    pub fn set_on(self, text: &str) -> String {
        let v = self.value().map(|s| serde_json::Value::String(s.into()));
        ateliet_core::scriptorium::set_frontmatter(text, KEY, v.as_ref())
    }
}

thread_local! {
    /// The alignment of the note being drawn (set around the note's view).
    static CURRENT: Cell<TextAlign> = const { Cell::new(TextAlign::Left) };
}

pub fn set_current(a: TextAlign) {
    CURRENT.with(|c| c.set(a));
}

pub fn current() -> TextAlign {
    CURRENT.with(|c| c.get())
}

/// Lays out `job` within `width` with the alignment: a galley whose rows sit
/// between 0 and `width`, as the editor and the reading view expect (egui
/// centres rows around 0, which a text editor would draw half outside).
pub fn layout(fonts: &egui::text::Fonts, mut job: LayoutJob, width: f32, a: TextAlign) -> Arc<Galley> {
    if a == TextAlign::Left || !width.is_finite() {
        return fonts.layout_job(job);
    }
    job.wrap.max_width = width;
    match a {
        TextAlign::Justify => job.justify = true,
        TextAlign::Center => job.halign = egui::Align::Center,
        TextAlign::Right => job.halign = egui::Align::RIGHT,
        TextAlign::Left => {}
    }
    let galley = fonts.layout_job(job);
    let dx = match a {
        TextAlign::Center => width / 2.0,
        TextAlign::Right => width,
        _ => 0.0,
    };
    let mut g = (*galley).clone();
    if dx != 0.0 {
        let d = egui::vec2(dx, 0.0);
        for row in &mut g.rows {
            for glyph in &mut row.glyphs {
                glyph.pos.x += dx;
            }
            row.rect = row.rect.translate(d);
            row.visuals.mesh.translate(d);
            row.visuals.mesh_bounds = row.visuals.mesh_bounds.translate(d);
        }
        g.mesh_bounds = g.mesh_bounds.translate(d);
    }
    // the whole width, so the space beside short rows belongs to the text
    g.rect = egui::Rect::from_min_max(egui::pos2(0.0, g.rect.min.y), egui::pos2(width.max(g.rect.max.x), g.rect.max.y));
    Arc::new(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_frontmatter() {
        let t = "---\ntitulo: x\nalinhamento: centro\n---\ntexto";
        assert_eq!(TextAlign::of_note(t), TextAlign::Center);
        let j = TextAlign::Justify.set_on(t);
        assert_eq!(TextAlign::of_note(&j), TextAlign::Justify);
        let l = TextAlign::Left.set_on(&j);
        assert_eq!(TextAlign::of_note(&l), TextAlign::Left);
        assert!(!l.contains("alinhamento"));
        assert!(l.contains("titulo: x") && l.ends_with("texto"));
        assert_eq!(TextAlign::of_note("sem propriedades"), TextAlign::Left);
        let n = TextAlign::Right.set_on("sem propriedades");
        assert!(n.starts_with("---\nalinhamento: direita\n---\n"), "{n}");
    }
}
