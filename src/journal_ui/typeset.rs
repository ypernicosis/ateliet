//! Text on a page, as it shows and as it is set: the Markdown of a text
//! turned into what is drawn (its marks hidden, except on the line being
//! typed), with a map from every drawn char back to the char of the text;
//! the rows of the page's typesetter (`journal::wrap`) measured in the
//! text's font; and each row drawn, hit and walked through by the cursor.
//!
//! Rows are broken once, at a fixed scale (`K_REF` px a millimetre), never
//! at the scale the page is shown at: zooming never moves a word to another
//! line, and what is typed and what is drawn always agree.

use super::draw::{self, PageView};
use ateliet_core::journal::wrap::{self, Fit, Measure, PageSet, Set};
use ateliet_core::journal::{Item, Kind, Layout};
use eframe::egui::{self, epaint::Fonts, text::LayoutJob, Color32, Pos2, Stroke};
use std::collections::HashMap;
use std::sync::Arc;

/// The scale rows are broken at (px a millimetre).
pub const K_REF: f32 = 6.0;

/// A text as it shows: what is drawn, and where each drawn char comes from.
#[derive(Clone)]
pub struct Styled {
    /// The whole text, not wrapped.
    pub job: LayoutJob,
    pub chars: Vec<char>,
    /// Drawn char → char of the text (one more at the end: the text's length).
    to_buf: Vec<usize>,
    /// Drawn char → byte of `job.text` (one more at the end).
    bytes: Vec<usize>,
    /// The type of the text (for an empty row).
    pub base: egui::TextFormat,
}

impl Styled {
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// The char of the text a drawn place is at.
    pub fn buf_of(&self, d: usize) -> usize {
        self.to_buf[d.min(self.chars.len())]
    }

    /// The drawn place of a char of the text (a hidden mark: the next char shown).
    pub fn disp_of(&self, c: usize) -> usize {
        self.to_buf[..self.chars.len()].partition_point(|&b| b < c)
    }

    /// Drawn chars `a..b`, as a job of their own (not wrapped).
    pub fn slice(&self, a: usize, b: usize) -> LayoutJob {
        let n = self.chars.len();
        let (a, b) = (a.min(n), b.min(n).max(a.min(n)));
        let (ba, bb) = (self.bytes[a], self.bytes[b]);
        let mut job = LayoutJob { text: self.job.text[ba..bb].to_string(), ..Default::default() };
        job.wrap = egui::text::TextWrapping { max_width: f32::INFINITY, max_rows: usize::MAX, break_anywhere: false, overflow_character: None };
        for s in &self.job.sections {
            let (s0, s1) = (s.byte_range.start.max(ba), s.byte_range.end.min(bb));
            if s0 < s1 {
                job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: s0 - ba..s1 - ba, format: s.format.clone() });
            }
        }
        if job.sections.is_empty() {
            job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: 0..0, format: self.base.clone() });
        }
        job
    }

    /// The next line break at or after drawn char `d` (or the end).
    fn para_end(&self, d: usize) -> usize {
        self.chars[d.min(self.chars.len())..].iter().position(|c| *c == '\n').map(|p| d + p).unwrap_or(self.chars.len())
    }
}

struct Builder {
    job: LayoutJob,
    chars: Vec<char>,
    to_buf: Vec<usize>,
    bytes: Vec<usize>,
}

impl Builder {
    /// Drawn text whose chars are the text's, from char `at` on.
    fn mapped(&mut self, s: &str, at: usize, f: egui::TextFormat) {
        self.put(s, f, |k| at + k);
    }

    /// Drawn text standing for the text's char `at` (and the last one for `last`).
    fn standing(&mut self, s: &str, at: usize, last: usize, f: egui::TextFormat) {
        let n = s.chars().count();
        self.put(s, f, |k| if k + 1 == n && n > 1 { last } else { at });
    }

    fn put(&mut self, s: &str, f: egui::TextFormat, map: impl Fn(usize) -> usize) {
        if s.is_empty() {
            return;
        }
        for (k, (b, c)) in s.char_indices().enumerate() {
            self.chars.push(c);
            self.to_buf.push(map(k));
            self.bytes.push(self.job.text.len() + b);
        }
        self.job.append(s, 0.0, f);
    }
}

/// What a text shows: `**bold**`, `*italic*`, `==highlight==` (a red pencil
/// line under it), `[[links]]` and `#tags` in the accent, a list's dash as a
/// bullet, a heading in bold. The marks are hidden, except on the text's
/// line where `reveal` (a char of the text: the cursor) is: there they show,
/// faint, so every typed char is where the cursor expects it.
#[allow(clippy::too_many_arguments)]
pub fn styled(it: &Item, text: &str, lay: &Layout, k: f32, color: Color32, accent: Color32, reveal: Option<usize>, foot: Option<(&str, Color32)>) -> Styled {
    let (reg, bold, ital, f) = draw::font_of(it);
    let lh = draw::line_px(it, lay, k);
    let size = lh * f;
    let mark = color.gamma_multiply(0.32);
    let fmt = |fam: &egui::FontFamily, c: Color32, under: bool| egui::TextFormat {
        font_id: egui::FontId::new(size, fam.clone()),
        color: c,
        line_height: Some(lh),
        valign: egui::Align::BOTTOM,
        underline: if under { Stroke::new((k * 0.25).max(1.0), draw::PENCIL_RED.gamma_multiply(0.85)) } else { Stroke::NONE },
        ..Default::default()
    };
    let mut out = Builder { job: LayoutJob::default(), chars: Vec::new(), to_buf: Vec::new(), bytes: Vec::new() };
    let mut line_start = 0usize;
    for (li, raw) in text.split('\n').enumerate() {
        if li > 0 {
            out.mapped("\n", line_start - 1, fmt(&reg, color, false));
        }
        let chars: Vec<char> = raw.chars().collect();
        let n_chars = chars.len();
        let live = reveal.is_some_and(|c| c >= line_start && c <= line_start + n_chars);
        let s_of = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };
        let lead = chars.iter().take_while(|c| c.is_whitespace()).count();
        let t: String = s_of(lead, n_chars);
        let mut heading = false;
        // the line's own mark: a heading's hashes, a list's dash
        let prefix: Option<(usize, &str)> = if t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') {
            heading = true;
            let hashes = t.chars().take_while(|c| *c == '#').count();
            Some((lead + hashes + 1, ""))
        } else if t.starts_with("- [ ] ") {
            Some((lead + 6, "☐ "))
        } else if t.starts_with("- [x] ") || t.starts_with("- [X] ") {
            Some((lead + 6, "☑ "))
        } else if t.starts_with("- ") || t.starts_with("* ") {
            Some((lead + 2, "• "))
        } else {
            None
        };
        let mut j = 0;
        if let Some((n, shown)) = prefix {
            if live {
                out.mapped(&s_of(0, n), line_start, fmt(&reg, mark, false));
            } else {
                out.mapped(&s_of(0, lead), line_start, fmt(&reg, color, false));
                out.standing(shown, line_start + lead, line_start + n - 1, fmt(&reg, color, false));
            }
            j = n;
        }
        let (mut b, mut i, mut m) = (heading, false, false);
        let mut run_from = j;
        let mut run = String::new();
        let fam_of = |b: bool, i: bool| if b { &bold } else if i { &ital } else { &reg };
        macro_rules! flush {
            ($c:expr) => {
                if !run.is_empty() {
                    out.mapped(&run, line_start + run_from, fmt(fam_of(b, i), $c, m));
                    run.clear();
                }
            };
        }
        // a mark: shown faint on the line being typed, else hidden
        macro_rules! mark {
            ($at:expr, $len:expr) => {
                if live {
                    out.mapped(&s_of($at, $at + $len), line_start + $at, fmt(fam_of(b, i), mark, false));
                }
            };
        }
        while j < n_chars {
            let c = chars[j];
            let next = chars.get(j + 1).copied();
            if c == '*' && next == Some('*') {
                flush!(color);
                b = !b;
                mark!(j, 2);
                j += 2;
                run_from = j;
                continue;
            }
            if c == '=' && next == Some('=') {
                flush!(color);
                m = !m;
                mark!(j, 2);
                j += 2;
                run_from = j;
                continue;
            }
            if c == '*' || c == '_' {
                let prev_ws = j == 0 || chars[j - 1].is_whitespace() || chars[j - 1].is_ascii_punctuation();
                let opens = !i && prev_ws && next.is_some_and(|n| !n.is_whitespace());
                let closes = i && j > 0 && !chars[j - 1].is_whitespace();
                if opens || closes {
                    flush!(color);
                    i = !i;
                    mark!(j, 1);
                    j += 1;
                    run_from = j;
                    continue;
                }
            }
            if c == '[' && next == Some('[') {
                if let Some(end) = chars[j + 2..].windows(2).position(|w| w == [']', ']']) {
                    flush!(color);
                    let inner = j + 2;
                    if live {
                        mark!(j, 2);
                        out.mapped(&s_of(inner, inner + end), line_start + inner, fmt(fam_of(b, i), accent, m));
                        mark!(inner + end, 2);
                    } else {
                        let body = &chars[inner..inner + end];
                        let (from, to) = match body.iter().position(|c| *c == '|') {
                            Some(p) => (inner + p + 1, inner + end),
                            None => (inner, inner + body.iter().position(|c| *c == '#').unwrap_or(end)),
                        };
                        out.mapped(&s_of(from, to), line_start + from, fmt(fam_of(b, i), accent, m));
                    }
                    j += end + 4;
                    run_from = j;
                    continue;
                }
            }
            if c == '#' && next.is_some_and(|n| n.is_alphanumeric()) && (j == 0 || chars[j - 1].is_whitespace()) {
                flush!(color);
                let from = j;
                j += 1;
                while j < n_chars && (chars[j].is_alphanumeric() || matches!(chars[j], '_' | '-' | '/')) {
                    j += 1;
                }
                out.mapped(&s_of(from, j), line_start + from, fmt(&reg, accent, m));
                run_from = j;
                continue;
            }
            if run.is_empty() {
                run_from = j;
            }
            run.push(c);
            j += 1;
        }
        flush!(color);
        line_start += n_chars + 1;
    }
    let len = text.chars().count();
    // an excerpt's source link, after its passage (not part of the text)
    if let Some((f_text, f_color)) = foot.filter(|f| !f.0.is_empty()) {
        let small = egui::TextFormat { font_id: egui::FontId::new((lh * 0.5).clamp(1.0, 400.0), egui::FontFamily::Name("italic".into())), color: f_color, line_height: Some(lh), valign: egui::Align::BOTTOM, ..Default::default() };
        out.put(&format!("  — {f_text}"), small, |_| len);
    }
    out.to_buf.push(len);
    out.bytes.push(out.job.text.len());
    Styled { job: out.job, chars: out.chars, to_buf: out.to_buf, bytes: out.bytes, base: fmt(&reg, color, false) }
}

/// The text a box sets: a text's Markdown (the one being typed: as typed),
/// an excerpt's passage.
pub fn text_of(it: &Item, typing: Option<&str>) -> String {
    typing.map(str::to_string).unwrap_or_else(|| super::flow_text(it))
}

/// An excerpt's source link (drawn after its passage, in its last box).
pub fn foot_of(it: &Item) -> String {
    if it.kind != Kind::Excerpt || it.flow.as_ref().is_some_and(|f| f.next.is_some()) {
        return String::new();
    }
    draw::plain_link(&ateliet_core::journal::excerpt_parts(it.content()).foot)
}

/// The text being typed on a page: its id, what is typed and where the cursor is.
pub type Typing<'a> = Option<(&'a str, &'a str, usize)>;

/// The texts of a page as they show at scale `k` (the one typed: as typed,
/// with the marks of the cursor's line showing).
pub fn styled_page(items: &[Item], lay: &Layout, k: f32, typing: Typing, ink: Color32, accent: Color32) -> HashMap<String, Styled> {
    items.iter().filter(|i| wrap::is_set(i)).map(|it| {
        let (typed, reveal) = match typing {
            Some((id, buf, c)) if id == it.id => (Some(buf), Some(c)),
            _ => (None, None),
        };
        let color = it.style.color.as_deref().and_then(draw::hex).unwrap_or(ink);
        let foot = foot_of(it);
        let st = styled(it, &text_of(it, typed), lay, k, color, accent, reveal, Some((&foot, ink.gamma_multiply(0.55))));
        (it.id.clone(), st)
    }).collect()
}

/// The rows of the last paragraph laid out: (text, room, rows by start).
type Memo = (String, u64, Vec<(usize, Fit)>);

/// Lays rows out in the texts' fonts, at `K_REF`.
pub struct Meter<'a> {
    fonts: &'a Fonts,
    texts: &'a HashMap<String, Styled>,
    memo: Option<Memo>,
}

impl<'a> Meter<'a> {
    pub fn new(fonts: &'a Fonts, texts: &'a HashMap<String, Styled>) -> Meter<'a> {
        Meter { fonts, texts, memo: None }
    }
}

/// How wide a row's letters are (px): to its last letter that is not a space.
pub fn ink_width(row: &egui::epaint::text::Row) -> f32 {
    row.glyphs.iter().rev().find(|g| !g.chr.is_whitespace()).map(|g| g.max_x()).unwrap_or(0.0)
}

impl Measure for Meter<'_> {
    fn len(&mut self, id: &str) -> usize {
        self.texts.get(id).map(|t| t.len()).unwrap_or(0)
    }

    fn row(&mut self, id: &str, from: usize, room: f64) -> Fit {
        let bits = room.to_bits();
        if let Some((mid, mroom, rows)) = &self.memo {
            if mid == id && *mroom == bits {
                if let Some((_, f)) = rows.iter().find(|(s, _)| *s == from) {
                    return *f;
                }
            }
        }
        let Some(st) = self.texts.get(id) else { return Fit { end: from, newline: false, width: 0.0 } };
        let para = st.para_end(from);
        // a long paragraph is laid out a piece at a time
        let cap = para.min(from + 600);
        let mut job = st.slice(from, cap);
        job.wrap.max_width = (room as f32 * K_REF).max(1.0);
        let g = self.fonts.layout_job(job);
        let mut rows = Vec::with_capacity(g.rows.len());
        let mut at = from;
        let n = g.rows.len();
        for (k, r) in g.rows.iter().enumerate() {
            let end = (at + r.glyphs.len()).min(cap);
            let last = k + 1 == n;
            if last && cap < para && n > 1 {
                // cut short with the paragraph: laid out again from here
                break;
            }
            let newline = last && end == para && para < st.len();
            rows.push((at, Fit { end, newline, width: (ink_width(r) / K_REF) as f64 }));
            at = end;
        }
        let fit = rows.first().map(|r| r.1).unwrap_or(Fit { end: from, newline: from == para && para < st.len(), width: 0.0 });
        self.memo = Some((id.to_string(), bits, rows));
        fit
    }
}

/// Set a page's texts (`texts`: as they show at `K_REF`).
pub fn set_page(ctx: &egui::Context, items: &[Item], lay: &Layout, texts: &HashMap<String, Styled>, prev: Option<&PageSet>, skip: &[String]) -> PageSet {
    ctx.fonts(|f| wrap::set_page(items, lay, prev, &mut Meter::new(f, texts), skip))
}

/// One text set alone (a cover on the shelf, a page's thumbnail).
pub fn set_alone(ctx: &egui::Context, it: &Item, lay: &Layout) -> Set {
    let texts = styled_page(std::slice::from_ref(it), lay, K_REF, None, Color32::BLACK, Color32::BLACK);
    let mut lone = it.clone();
    lone.home = None;
    let set = set_page(ctx, std::slice::from_ref(&lone), lay, &texts, None, &[]);
    set.texts.into_iter().next().unwrap_or(Set { id: it.id.clone(), rows: Vec::new(), top: it.y, height: it.h, placed: 0, len: 0, off: true, row_h: lay.paper.line() })
}

// ── rows on the screen ──────────────────────────────────────────────────

/// A row laid out on the screen: its galley and where the galley goes.
pub struct Placed {
    pub galley: Arc<egui::Galley>,
    pub origin: Pos2,
}

impl Placed {
    fn glyphs(&self) -> &[egui::epaint::text::Glyph] {
        self.galley.rows.first().map(|r| r.glyphs.as_slice()).unwrap_or(&[])
    }

    /// Where drawn char `d` of the row (`from` its first) is, on the screen.
    pub fn x_of(&self, from: usize, d: usize) -> f32 {
        let g = self.glyphs();
        let k = d.saturating_sub(from);
        let x = match g.get(k) {
            Some(gl) => gl.pos.x,
            None => g.last().map(|gl| gl.max_x()).unwrap_or(0.0),
        };
        self.origin.x + x
    }

    /// The drawn char of the row nearest to screen `x` (`from`..=`to`).
    pub fn char_at(&self, from: usize, to: usize, x: f32) -> usize {
        let lx = x - self.origin.x;
        for (k, g) in self.glyphs().iter().enumerate() {
            if lx < (g.pos.x + g.max_x()) / 2.0 {
                return (from + k).min(to);
            }
        }
        to
    }
}

/// Where the letters of a row sit (from the galley's top to the line), px.
pub fn baseline_off(fonts: &Fonts, st: &Styled) -> f32 {
    let mut job = LayoutJob::default();
    job.append("Ag", 0.0, st.base.clone());
    let g = fonts.layout_job(job);
    g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.pos.y).unwrap_or(st.base.line_height.unwrap_or(12.0) * 0.8)
}

/// Row `r` of a text laid out on the screen: justified (the room left
/// shared among its spaces, when the row is not a paragraph's last and the
/// spaces would not open wider than three), centred or to the right in its
/// room, on its line.
pub fn place_row(fonts: &Fonts, it: &Item, st: &Styled, set: &Set, r: usize, pv: &PageView, base_off: f32) -> Placed {
    let row = &set.rows[r];
    let k = pv.k;
    let room = row.room as f32 * k;
    let mut job = st.slice(row.from, row.to);
    let last_of_para = row.newline || (r + 1 == set.rows.len() && !set.overflows());
    let mut plain = fonts.layout_job(job.clone());
    // the type is set in whole pixels: at this scale a row comes out a hair
    // wider or narrower than where it was broken; the letters are spaced so
    // that it is exactly as wide (what is drawn is what was set)
    let ink = |g: &egui::Galley| g.rows.first().map(ink_width).unwrap_or(0.0);
    let target = row.width as f32 * k;
    let gaps = plain.rows.first().and_then(|r| r.glyphs.iter().rposition(|g| !g.chr.is_whitespace())).unwrap_or(0);
    if gaps > 0 && (ink(&plain) - target).abs() > 0.3 {
        let extra = (target - ink(&plain)) / gaps as f32;
        for s in job.sections.iter_mut() {
            s.format.extra_letter_spacing += extra;
        }
        plain = fonts.layout_job(job.clone());
    }
    let mut width = ink(&plain);
    let galley = if it.style.align == "justificado" && !last_of_para {
        let gl = plain.rows.first().map(|r| r.glyphs.clone()).unwrap_or_default();
        let words: Vec<usize> = (1..gl.len()).filter(|&i| !gl[i].chr.is_whitespace() && gl[i - 1].chr.is_whitespace() && gl[..i].iter().any(|g| !g.chr.is_whitespace())).collect();
        let space = gl.iter().find(|g| g.chr == ' ').map(|g| g.advance_width).unwrap_or(4.0).max(0.5);
        let free = room - width - 0.5;
        if !words.is_empty() && free > 0.0 && free / words.len() as f32 <= space * 3.0 {
            let per = free / words.len() as f32;
            let starts: Vec<usize> = words.iter().map(|&i| st.bytes_of(row.from + i) - st.bytes_of(row.from)).collect();
            let mut wide = job.clone();
            wide.sections.clear();
            for s in &job.sections {
                let mut from = s.byte_range.start;
                let mut lead = 0.0;
                for &b in starts.iter().filter(|b| **b >= s.byte_range.start && **b < s.byte_range.end) {
                    if b == from {
                        lead += per;
                        continue;
                    }
                    wide.sections.push(egui::text::LayoutSection { leading_space: lead, byte_range: from..b, format: s.format.clone() });
                    from = b;
                    lead = per;
                }
                wide.sections.push(egui::text::LayoutSection { leading_space: lead, byte_range: from..s.byte_range.end, format: s.format.clone() });
            }
            job = wide;
            width = room;
            fonts.layout_job(job)
        } else {
            plain
        }
    } else {
        plain
    };
    let dx = match it.style.align.as_str() {
        "centro" => (room - width) / 2.0,
        "direita" => room - width,
        _ => 0.0,
    }
    .max(0.0);
    let lh = draw::line_px(it, &pv.lay, k);
    let base_y = pv.s((0.0, row.base)).y - lh * 0.06;
    Placed { galley, origin: egui::pos2(pv.s((row.x, 0.0)).x + dx, base_y - base_off) }
}

impl Styled {
    fn bytes_of(&self, d: usize) -> usize {
        self.bytes[d.min(self.chars.len())]
    }
}

/// All rows of a text laid out on the screen.
pub fn place_all(fonts: &Fonts, it: &Item, st: &Styled, set: &Set, pv: &PageView) -> Vec<Placed> {
    let off = baseline_off(fonts, st);
    (0..set.rows.len()).map(|r| place_row(fonts, it, st, set, r, pv, off)).collect()
}

/// Where the cursor at drawn char `d` is on the screen: its row and x.
pub fn caret_at(set: &Set, rows: &[Placed], d: usize) -> Option<(usize, f32)> {
    if set.rows.is_empty() {
        return None;
    }
    let r = set.row_of(d);
    let row = &set.rows[r];
    Some((r, rows.get(r)?.x_of(row.from, d.clamp(row.from, row.to))))
}

/// The drawn char nearest a point of the page (mm) in a text's rows: on
/// its row (the nearest row up or down), the nearest place along it. A row
/// broken at a space keeps the cursor on it (before the space).
pub fn char_near(set: &Set, rows: &[Placed], pv: &PageView, at: (f64, f64)) -> Option<usize> {
    if set.rows.is_empty() {
        return None;
    }
    let r = set.rows.iter().enumerate().min_by(|(_, a), (_, b)| {
        let d = |r: &wrap::Row| if at.1 < r.base - set.row_h { r.base - set.row_h - at.1 } else if at.1 > r.base { at.1 - r.base } else { 0.0 };
        d(a).total_cmp(&d(b)).then(a.base.total_cmp(&b.base))
    })?.0;
    let row = &set.rows[r];
    let p = rows.get(r)?;
    let mut d = p.char_at(row.from, row.to, pv.s(at).x);
    if d == row.to && !row.newline && r + 1 < set.rows.len() && row.to > row.from {
        d = row.to - 1;
    }
    Some(d)
}

/// Is a point of the page (mm) on a text's letters (a little round them)?
pub fn on_letters(set: &Set, at: (f64, f64)) -> bool {
    set.rows.iter().any(|r| at.1 >= r.base - set.row_h && at.1 <= r.base + 0.4 && at.0 >= r.x - 1.5 && at.0 <= r.x + r.width + 2.5)
}

/// The text a set belongs to, drawn with its rows: the page's setting.
pub fn draw_rows(painter: &egui::Painter, rows: Vec<Placed>, ink: Color32) {
    for p in rows {
        painter.galley(p.origin, p.galley, ink);
    }
}
