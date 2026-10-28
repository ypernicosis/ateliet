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
        let out = ateliet_core::scriptorium::set_frontmatter(text, KEY, v.as_ref());
        // a frontmatter that only held the alignment goes with it
        for empty in ["---\n---\n", "---\n---"] {
            if let Some(rest) = out.strip_prefix(empty) {
                return rest.to_string();
            }
        }
        out
    }
}

/// Sets the alignment of an open note. The frontmatter at the top grows or
/// shrinks, so the block being edited in Live Preview and the source
/// editor's cursor move down or up with the text below it.
pub fn apply(ctx: &egui::Context, tab: &mut crate::app::Tab, a: TextAlign) {
    if TextAlign::of_note(&tab.text) == a {
        return;
    }
    let old = std::mem::take(&mut tab.text);
    let new = a.set_on(&old);
    let lines = new.split('\n').count() as isize - old.split('\n').count() as isize;
    let chars = new.chars().count() as isize - old.chars().count() as isize;
    // where the frontmatter ended before: the cursor only moves past it
    let fm_end = ateliet_core::syntax::frontmatter(&old).map(|f| old[..f.block.end.min(old.len())].chars().count()).unwrap_or(0);
    let shift = |x: usize| (x as isize + lines).max(0) as usize;
    if let Some(start) = tab.live.active {
        tab.live.active = Some(shift(start));
        tab.live.buf_end = shift(tab.live.buf_end);
    }
    tab.text = new;
    tab.cursor = if tab.cursor >= fm_end { (tab.cursor as isize + chars).max(0) as usize } else { tab.cursor };
    let id = tab.editor_id();
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        if let Some(r) = state.cursor.char_range() {
            let mv = |c: egui::text::CCursor| if c.index >= fm_end { egui::text::CCursor::new((c.index as isize + chars).max(0) as usize) } else { c };
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(mv(r.secondary), mv(r.primary))));
            state.store(ctx, id);
        }
    }
    tab.last_edit = Some(std::time::Instant::now());
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

/// A paragraph whose first section carries this letter spacing (too small
/// to see) stays flush left when the note is centred or set to the right:
/// list items, whose marker would otherwise drift away from the margin.
const FLUSH_LEFT: f32 = 1e-4;

pub fn flush_left(f: &mut egui::TextFormat) {
    f.extra_letter_spacing = FLUSH_LEFT;
}

/// The alignment lists take in a note aligned with `a`.
pub fn for_lists(a: TextAlign) -> TextAlign {
    if a == TextAlign::Justify { a } else { TextAlign::Left }
}

/// Lays out `job` within `width` with the alignment: a galley whose rows sit
/// between 0 and `width`, as the editor and the reading view expect.
///
/// The rows are laid out flush left and then moved here, not by egui:
/// justified rows get the extra room in their spaces only (egui also spreads
/// it between letters), the last row of a paragraph keeps its trailing
/// spaces in view (so the caret stays inside the column while typing), and
/// rows set entirely in monospace (code, frontmatter) stay flush left.
pub fn layout(fonts: &egui::text::Fonts, mut job: LayoutJob, width: f32, a: TextAlign) -> Arc<Galley> {
    if a == TextAlign::Left || !width.is_finite() {
        return fonts.layout_job(job);
    }
    job.wrap.max_width = width;
    job.halign = egui::Align::LEFT;
    job.justify = false;
    let galley = fonts.layout_job(job);
    let mut g = (*galley).clone();
    let n_rows = g.rows.len();
    let mut bounds = egui::Rect::NOTHING;
    let mut para_align = a;
    let mut para_start = true;
    for (i, row) in g.rows.iter_mut().enumerate() {
        if para_start {
            let first = row.glyphs.first().map(|gl| gl.section_index).unwrap_or(row.section_index_at_start);
            let flush = galley.job.sections.get(first as usize).is_some_and(|s| s.format.extra_letter_spacing == FLUSH_LEFT);
            para_align = if flush { for_lists(a) } else { a };
        }
        let para_end = row.ends_with_newline || i + 1 == n_rows;
        para_start = row.ends_with_newline;
        align_row(row, &galley.job, width, para_align, para_end);
        bounds = bounds.union(row.visuals.mesh_bounds);
    }
    g.mesh_bounds = bounds;
    // the whole width, so the space beside short rows belongs to the text
    g.rect = egui::Rect::from_min_max(egui::pos2(0.0, g.rect.min.y), egui::pos2(width.max(g.rect.max.x), g.rect.max.y));
    Arc::new(g)
}

/// Moves one flush-left row into place: an offset per glyph, applied to the
/// glyphs and to the vertices of the row's mesh.
fn align_row(row: &mut egui::epaint::text::Row, job: &LayoutJob, width: f32, a: TextAlign, para_end: bool) {
    if a == TextAlign::Left {
        return;
    }
    let mono = |gl: &egui::epaint::text::Glyph| job.sections.get(gl.section_index as usize).is_some_and(|s| s.format.font_id.family == egui::FontFamily::Monospace);
    let content: Vec<usize> = (0..row.glyphs.len()).filter(|&k| !row.glyphs[k].chr.is_whitespace()).collect();
    if !content.is_empty() && content.iter().all(|&k| mono(&row.glyphs[k])) {
        return;
    }
    let left = row.glyphs.first().map(|gl| gl.pos.x).unwrap_or(0.0);
    let last_ink = content.last().map(|&k| row.glyphs[k].max_x()).unwrap_or(left);
    let last_any = row.glyphs.last().map(|gl| gl.max_x()).unwrap_or(left);
    let mut offsets = vec![0.0_f32; row.glyphs.len()];
    let uniform = match a {
        TextAlign::Justify => {
            if para_end || content.is_empty() {
                None
            } else {
                // the spaces between words (not the ones the row starts or ends with)
                let (first, last) = (content[0], *content.last().unwrap());
                let spaces = (first..last).filter(|&k| row.glyphs[k].chr.is_whitespace()).count();
                let room = width - (last_ink - left);
                if spaces == 0 || room <= 0.0 {
                    None
                } else {
                    let per = room / spaces as f32;
                    let mut acc: f32 = 0.0;
                    for (k, gl) in row.glyphs.iter().enumerate() {
                        offsets[k] = acc.round();
                        if k >= first && k < last && gl.chr.is_whitespace() {
                            acc += per;
                        }
                    }
                    Some(None)
                }
            }
        }
        TextAlign::Center | TextAlign::Right => {
            // the paragraph's last row keeps its trailing spaces: the caret follows the typing
            let w = if para_end { last_any } else { last_ink } - left;
            let free = (width - w).max(0.0);
            Some(Some(if a == TextAlign::Center { (free / 2.0).round() } else { free.floor() }))
        }
        TextAlign::Left => None,
    };
    let Some(uniform) = uniform else { return };
    if let Some(dx) = uniform {
        offsets.iter_mut().for_each(|o| *o = dx);
    }
    let orig: Vec<f32> = row.glyphs.iter().map(|gl| gl.pos.x).collect();
    // the glyph a vertex belongs to, by its x (backgrounds, underlines)
    let at = |x: f32| -> f32 {
        if orig.is_empty() {
            return uniform.unwrap_or(0.0);
        }
        let k = orig.partition_point(|&gx| gx <= x + 0.01);
        offsets[k.saturating_sub(1)]
    };
    let glyph_vertices = row.visuals.glyph_vertex_range.clone();
    // glyph quads come four vertices per inked glyph, in order
    let mut quad_owner = Vec::new();
    for (k, gl) in row.glyphs.iter().enumerate() {
        if !gl.uv_rect.is_nothing() {
            quad_owner.push(k);
        }
    }
    for (vi, v) in row.visuals.mesh.vertices.iter_mut().enumerate() {
        let dx = if glyph_vertices.contains(&vi) {
            quad_owner.get((vi - glyph_vertices.start) / 4).map(|&k| offsets[k]).unwrap_or_else(|| at(v.pos.x))
        } else {
            at(v.pos.x)
        };
        v.pos.x += dx;
    }
    for (gl, o) in row.glyphs.iter_mut().zip(&offsets) {
        gl.pos.x += o;
    }
    match uniform {
        Some(dx) => row.rect = row.rect.translate(egui::vec2(dx, 0.0)),
        None => row.rect.max.x = row.rect.max.x.max(width),
    }
    if !row.visuals.mesh.vertices.is_empty() {
        row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> egui::text::Fonts {
        egui::text::Fonts::new(1.0, 2048, egui::FontDefinitions::default())
    }

    fn job(text: &str, family: egui::FontFamily) -> LayoutJob {
        let mut j = LayoutJob::default();
        j.append(text, 0.0, egui::TextFormat { font_id: egui::FontId::new(14.0, family), ..Default::default() });
        j
    }

    #[test]
    fn justified_rows_reach_the_margin_through_their_spaces() {
        let f = fonts();
        let text = "uma frase razoavelmente longa para quebrar em várias linhas dentro de uma coluna estreita\nfim";
        let g = layout(&f, job(text, egui::FontFamily::Proportional), 200.0, TextAlign::Justify);
        assert!(g.rows.len() > 2);
        let first = &g.rows[0];
        let ink = first.glyphs.iter().rev().find(|gl| !gl.chr.is_whitespace()).unwrap();
        assert!((ink.max_x() - 200.0).abs() < 1.0, "the row ends at the margin: {}", ink.max_x());
        // letters of one word keep their spacing
        let plain = f.layout_job(job(text, egui::FontFamily::Proportional).tap_wrap(200.0));
        let (a, b) = (&first.glyphs[0], &first.glyphs[1]);
        let (pa, pb) = (&plain.rows[0].glyphs[0], &plain.rows[0].glyphs[1]);
        assert!(((b.pos.x - a.pos.x) - (pb.pos.x - pa.pos.x)).abs() < 0.01);
        // the paragraph's last row is not stretched
        let last = g.rows.iter().find(|r| r.ends_with_newline).unwrap();
        assert!(last.glyphs.last().unwrap().max_x() < 199.0);
        assert_eq!(g.rows.iter().map(|r| r.char_count_including_newline()).sum::<usize>(), text.chars().count());
    }

    #[test]
    fn centred_and_right_rows_but_code_stays_left() {
        let f = fonts();
        let g = layout(&f, job("abc", egui::FontFamily::Proportional), 300.0, TextAlign::Center);
        let w = g.rows[0].glyphs.last().unwrap().max_x() - g.rows[0].glyphs[0].pos.x;
        assert!((g.rows[0].glyphs[0].pos.x - (300.0 - w) / 2.0).abs() <= 1.0);
        let g = layout(&f, job("abc ", egui::FontFamily::Proportional), 300.0, TextAlign::Right);
        assert!(g.rows[0].glyphs.last().unwrap().max_x() <= 300.5, "the trailing space stays in the column");
        let g = layout(&f, job("let x = 1;", egui::FontFamily::Monospace), 300.0, TextAlign::Right);
        assert_eq!(g.rows[0].glyphs[0].pos.x, 0.0);
    }

    trait TapWrap {
        fn tap_wrap(self, w: f32) -> Self;
    }
    impl TapWrap for LayoutJob {
        fn tap_wrap(mut self, w: f32) -> Self {
            self.wrap.max_width = w;
            self
        }
    }

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
        assert_eq!(TextAlign::Left.set_on(&n), "sem propriedades", "an emptied frontmatter goes away");
    }
}
