//! Drawing a spread: the paper and its ruling, the spine, and the items in
//! their layers. Lengths in the model are millimetres; a page on screen is
//! `k` pixels a millimetre.

use super::{Book, Cx, Journal};
use crate::theme::{mix, Palette};
use ateliet_core::journal::{self, Item, Kind, Layout, Ruling};
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Shape, Stroke};

/// One page as it is on screen.
#[derive(Clone, Debug)]
pub struct PageView {
    pub n: usize,
    pub rect: Rect,
    pub k: f32,
    pub lay: Layout,
    /// Where its items may show: the page and a little past its outer edges
    /// (a sticker hangs over the edge), never over the other page (unless
    /// the item lies across both).
    pub clip: Rect,
    pub clip_across: Rect,
}

impl PageView {
    pub fn s(&self, p: (f64, f64)) -> Pos2 {
        pos2(self.rect.min.x + p.0 as f32 * self.k, self.rect.min.y + p.1 as f32 * self.k)
    }
    pub fn w(&self, q: Pos2) -> (f64, f64) {
        (((q.x - self.rect.min.x) / self.k) as f64, ((q.y - self.rect.min.y) / self.k) as f64)
    }
    pub fn len(&self, mm: f64) -> f32 {
        mm as f32 * self.k
    }
}

/// The paper's colour: a warm white over a light theme, a grey a little
/// lighter than the window over a dark one.
pub fn paper_color(p: Palette) -> Color32 {
    if p.dark {
        mix(p.bg, Color32::WHITE, 0.07)
    } else {
        mix(Color32::WHITE, Color32::from_rgb(0xf4, 0xec, 0xdc), 0.4)
    }
}

/// Behind the notebook.
pub fn desk_color(p: Palette) -> Color32 {
    if p.dark {
        mix(p.bg, Color32::BLACK, 0.25)
    } else {
        mix(p.bg_secondary, Color32::from_rgb(0x80, 0x80, 0x84), 0.06)
    }
}

pub fn ink_color(p: Palette) -> Color32 {
    if p.dark {
        Color32::from_rgb(0xe6, 0xe4, 0xde)
    } else {
        Color32::from_rgb(0x26, 0x25, 0x23)
    }
}

/// A `#rrggbb` colour.
pub fn hex(s: &str) -> Option<Color32> {
    let c = ateliet_core::moodboard::parse_hex(s)?;
    Some(Color32::from_rgb(c[0], c[1], c[2]))
}

pub fn hex_of(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Grey of a red pencil, for `==highlights==`.
const PENCIL_RED: Color32 = Color32::from_rgb(0xc2, 0x3b, 0x2e);

/// The paper of a page: its colour, the ruling, the Cornell lines, the
/// number at the foot.
pub fn paper(painter: &egui::Painter, pv: &PageView, p: Palette) {
    let paper = paper_color(p);
    painter.rect_filled(pv.rect, 0.0, paper);
    let line = ink_color(p).gamma_multiply(if p.dark { 0.09 } else { 0.075 });
    let w = (0.6_f32).max(pv.k * 0.12);
    let lay = &pv.lay;
    match lay.paper.ruling {
        Ruling::Grid => {
            for &y in &lay.rows {
                painter.hline(pv.rect.x_range(), pv.s((0.0, y)).y, Stroke::new(w, line));
            }
            for &x in &lay.cols {
                painter.vline(pv.s((x, 0.0)).x, pv.rect.y_range(), Stroke::new(w, line));
            }
        }
        Ruling::Lined => {
            for &y in &lay.rows {
                painter.hline(pv.rect.x_range(), pv.s((0.0, y)).y, Stroke::new(w, line));
            }
        }
        Ruling::Dot => {
            let r = (pv.k * 0.22).max(0.7);
            for &y in &lay.rows {
                for &x in &lay.cols {
                    painter.circle_filled(pv.s((x, y)), r, line.gamma_multiply(1.8));
                }
            }
        }
        Ruling::Plain => {}
    }
    let strong = Stroke::new((pv.k * 0.2).max(0.9), ink_color(p).gamma_multiply(0.22));
    if let Some(h) = lay.header {
        painter.hline((pv.rect.left() + pv.len(4.0))..=(pv.rect.right() - pv.len(4.0)), pv.s((0.0, h)).y, strong);
    }
    if let (Some(d), Some(h)) = (lay.divider, lay.header) {
        painter.vline(pv.s((d, 0.0)).x, pv.s((0.0, h)).y..=(pv.rect.bottom() - pv.len(6.0)), strong);
    }
    // the number, at the outer corner
    let at = if lay.right { pv.rect.right_bottom() + vec2(-pv.len(6.0), -pv.len(4.0)) } else { pv.rect.left_bottom() + vec2(pv.len(6.0), -pv.len(4.0)) };
    let align = if lay.right { egui::Align2::RIGHT_BOTTOM } else { egui::Align2::LEFT_BOTTOM };
    painter.text(at, align, pv.n.to_string(), egui::FontId::proportional((pv.k * 2.6).clamp(8.0, 13.0)), ink_color(p).gamma_multiply(0.35));
}

/// The spine between the pages, and the thickness of what is left on each
/// side: a few thin lines at the outer edges.
pub fn spine(painter: &egui::Painter, spread: Rect, left_pages: usize, right_pages: usize, p: Palette) {
    let x = spread.center().x;
    let shade = if p.dark { Color32::BLACK } else { Color32::from_rgb(0x30, 0x28, 0x20) };
    let wide = spread.width() * 0.035;
    let steps = 10;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let a = (1.0 - t).powi(2) * if p.dark { 0.22 } else { 0.07 };
        let w = wide / steps as f32;
        painter.rect_filled(Rect::from_min_max(pos2(x - (i + 1) as f32 * w, spread.top()), pos2(x - i as f32 * w, spread.bottom())), 0.0, shade.gamma_multiply(a));
        painter.rect_filled(Rect::from_min_max(pos2(x + i as f32 * w, spread.top()), pos2(x + (i + 1) as f32 * w, spread.bottom())), 0.0, shade.gamma_multiply(a));
    }
    painter.vline(x, spread.y_range(), Stroke::new(1.0_f32, shade.gamma_multiply(if p.dark { 0.5 } else { 0.18 })));
    let edge = Stroke::new(1.0_f32, ink_color(p).gamma_multiply(0.16));
    let lines = |n: usize| -> usize { if n == 0 { 0 } else { (1 + n / 24).min(3) } };
    for k in 0..lines(left_pages) {
        let d = 2.0 + k as f32 * 2.0;
        painter.vline(spread.left() - d, (spread.top() + d)..=(spread.bottom() - d), edge);
    }
    for k in 0..lines(right_pages) {
        let d = 2.0 + k as f32 * 2.0;
        painter.vline(spread.right() + d, (spread.top() + d)..=(spread.bottom() - d), edge);
    }
}

// ── text ──────────────────────────────────────────────────────────────────

/// The family and size factor of a text style (`tema`, `serifa`, `mao`).
pub fn font_of(it: &Item) -> (egui::FontFamily, egui::FontFamily, egui::FontFamily, f32) {
    let font = if it.style.font.is_empty() && it.kind == Kind::Excerpt { "serifa" } else { it.style.font.as_str() };
    let n = |s: &str| egui::FontFamily::Name(s.into());
    match font {
        "mao" => (n("hand"), n("hand"), n("hand"), 0.95),
        "serifa" => (n("garamond"), n("garamond-bold"), n("garamond-italic"), 0.66),
        _ => (n("theme-text"), n("theme-text-bold"), n("theme-text-italic"), 0.58),
    }
}

/// The height of one line of a text, in px.
pub fn line_px(it: &Item, lay: &Layout, k: f32) -> f32 {
    (lay.paper.line() * it.style.lines()) as f32 * k
}

fn align_of(it: &Item) -> egui::Align {
    match it.style.align.as_str() {
        "centro" => egui::Align::Center,
        "direita" => egui::Align::RIGHT,
        _ => egui::Align::LEFT,
    }
}

/// The plain text as it is typed (the editor and the flow measure this).
pub fn plain_job(it: &Item, text: &str, lay: &Layout, k: f32, color: Color32) -> egui::text::LayoutJob {
    let (reg, _, _, f) = font_of(it);
    let lh = line_px(it, lay, k);
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: egui::FontId::new(lh * f, reg), color, line_height: Some(lh), ..Default::default() });
    job.wrap.max_width = (it.w as f32 * k).max(4.0);
    job.halign = align_of(it);
    job
}

/// The text as it shows: `**bold**`, `*italic*`, `==highlight==` (a red
/// pencil line under it), `[[links]]` and `#tags` in the accent.
pub fn rich_job(it: &Item, text: &str, lay: &Layout, k: f32, color: Color32, accent: Color32) -> egui::text::LayoutJob {
    let (reg, bold, ital, f) = font_of(it);
    let lh = line_px(it, lay, k);
    let size = lh * f;
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (it.w as f32 * k).max(4.0);
    job.halign = align_of(it);
    let fmt = |fam: &egui::FontFamily, c: Color32, under: bool| egui::TextFormat {
        font_id: egui::FontId::new(size, fam.clone()),
        color: c,
        line_height: Some(lh),
        underline: if under { Stroke::new((k * 0.25).max(1.0), PENCIL_RED.gamma_multiply(0.85)) } else { Stroke::NONE },
        ..Default::default()
    };
    for (li, raw) in text.split('\n').enumerate() {
        if li > 0 {
            job.append("\n", 0.0, fmt(&reg, color, false));
        }
        let mut line = raw.to_string();
        let mut heading = false;
        let t = line.trim_start();
        if t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') {
            heading = true;
            line = t.trim_start_matches('#').trim_start().to_string();
        } else if let Some(r) = t.strip_prefix("- [ ] ") {
            line = format!("☐ {r}");
        } else if let Some(r) = t.strip_prefix("- [x] ") {
            line = format!("☑ {r}");
        } else if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            line = format!("• {r}");
        }
        let (mut b, mut i, mut m) = (heading, false, false);
        let mut buf = String::new();
        let chars: Vec<char> = line.chars().collect();
        let mut j = 0;
        let flush = |job: &mut egui::text::LayoutJob, buf: &mut String, b: bool, i: bool, m: bool, c: Color32| {
            if !buf.is_empty() {
                let fam = if b { &bold } else if i { &ital } else { &reg };
                job.append(buf, 0.0, fmt(fam, c, m));
                buf.clear();
            }
        };
        while j < chars.len() {
            let c = chars[j];
            let next = chars.get(j + 1).copied();
            if c == '*' && next == Some('*') {
                flush(&mut job, &mut buf, b, i, m, color);
                b = !b;
                j += 2;
                continue;
            }
            if c == '=' && next == Some('=') {
                flush(&mut job, &mut buf, b, i, m, color);
                m = !m;
                j += 2;
                continue;
            }
            if c == '*' || c == '_' {
                let prev_ws = j == 0 || chars[j - 1].is_whitespace() || chars[j - 1].is_ascii_punctuation();
                let opens = !i && prev_ws && next.is_some_and(|n| !n.is_whitespace());
                let closes = i && j > 0 && !chars[j - 1].is_whitespace();
                if opens || closes {
                    flush(&mut job, &mut buf, b, i, m, color);
                    i = !i;
                    j += 1;
                    continue;
                }
            }
            if c == '[' && next == Some('[') {
                if let Some(end) = chars[j + 2..].windows(2).position(|w| w == [']', ']']) {
                    flush(&mut job, &mut buf, b, i, m, color);
                    let inner: String = chars[j + 2..j + 2 + end].iter().collect();
                    let shown = inner.split_once('|').map(|(_, s)| s.to_string()).unwrap_or_else(|| inner.split('#').next().unwrap_or("").to_string());
                    let mut s = shown;
                    flush(&mut job, &mut s, b, i, m, accent);
                    j += end + 4;
                    continue;
                }
            }
            if c == '#' && next.is_some_and(|n| n.is_alphanumeric()) && (j == 0 || chars[j - 1].is_whitespace()) {
                flush(&mut job, &mut buf, b, i, m, color);
                let mut tag = String::from("#");
                j += 1;
                while j < chars.len() && (chars[j].is_alphanumeric() || matches!(chars[j], '_' | '-' | '/')) {
                    tag.push(chars[j]);
                    j += 1;
                }
                job.append(&tag, 0.0, fmt(&reg, accent, m));
                continue;
            }
            buf.push(c);
            j += 1;
        }
        flush(&mut job, &mut buf, b, i, m, color);
    }
    job
}

/// Paint a galley so its rows sit on the ruling: the first baseline on
/// the line one row below the box's top.
pub fn galley_on_lines(painter: &egui::Painter, galley: std::sync::Arc<egui::Galley>, pv: &PageView, it: &Item, color: Color32) {
    let at = text_origin(&galley, pv, it);
    painter.galley(at, galley, color);
}

/// Where a text's galley goes for its rows to sit on the ruling.
pub fn text_origin(galley: &egui::Galley, pv: &PageView, it: &Item) -> Pos2 {
    let lh = line_px(it, &pv.lay, pv.k);
    let base = galley.rows.iter().find_map(|r| r.glyphs.first().map(|g| g.pos.y - r.rect.min.y)).unwrap_or(lh * 0.78);
    let top = pv.s((it.x, it.y));
    // the first row's baseline on the line under the box's top
    let mut x = top.x;
    if it.style.align == "centro" {
        x += it.w as f32 * pv.k / 2.0;
    } else if it.style.align == "direita" {
        x += it.w as f32 * pv.k;
    }
    pos2(x, top.y + lh - base - lh * 0.06)
}

/// Where the rows of a text start (char indices), laid out at a fine
/// scale: the flow measures with this.
pub fn row_starts(ctx: &egui::Context, it: &Item, text: &str, lay: &Layout) -> Vec<usize> {
    const K: f32 = 6.0;
    let job = plain_job(it, text, lay, K, Color32::BLACK);
    let galley = ctx.fonts(|f| f.layout_job(job));
    let mut out = Vec::with_capacity(galley.rows.len());
    let mut n = 0;
    for r in &galley.rows {
        out.push(n);
        n += r.glyphs.len() + usize::from(r.ends_with_newline);
    }
    if out.is_empty() {
        out.push(0);
    }
    out
}

// ── items ─────────────────────────────────────────────────────────────────

/// The polygon a picture's cut makes on screen (or its box), and where each
/// point is in the box (0..1).
pub fn outline(it: &Item, pv: &PageView) -> Vec<(Pos2, (f64, f64))> {
    let pts = it.mask.as_ref().filter(|m| m.shape != "polaroide").map(|m| m.outline(64)).unwrap_or_else(|| vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    let (cx, cy) = (it.x + it.w / 2.0, it.y + it.h / 2.0);
    pts.into_iter().map(|(u, v)| {
        let (x, y) = ateliet_core::canvas::rot_pt(it.x + u * it.w, it.y + v * it.h, cx, cy, it.r);
        (pv.s((x, y)), (u, v))
    }).collect()
}

/// A filled polygon (ear-clipped), with a texture when given.
fn polygon(painter: &egui::Painter, pts: &[Pos2], uv: Option<(&[egui::Pos2], egui::TextureId)>, color: Color32) {
    if pts.len() < 3 {
        return;
    }
    let p: Vec<(f32, f32)> = pts.iter().map(|q| (q.x, q.y)).collect();
    let tris = ateliet_core::quadro::rough::triangulate(&p);
    let mut mesh = match uv {
        Some((_, t)) => egui::Mesh::with_texture(t),
        None => egui::Mesh::default(),
    };
    for (i, q) in pts.iter().enumerate() {
        let u = uv.map(|(u, _)| u[i]).unwrap_or(egui::epaint::WHITE_UV);
        mesh.vertices.push(egui::epaint::Vertex { pos: *q, uv: u, color });
    }
    for t in tris {
        mesh.indices.extend(t.iter().map(|&i| i as u32));
    }
    painter.add(mesh);
}

/// The rounded margin of a sticker round its cut, and its shadow.
fn sticker_edge(painter: &egui::Painter, pts: &[Pos2], r: f32, color: Color32, shadow: bool) {
    let draw = |off: egui::Vec2, c: Color32, painter: &egui::Painter| {
        let moved: Vec<Pos2> = pts.iter().map(|p| *p + off).collect();
        polygon(painter, &moved, None, c);
        if r > 0.3 {
            for i in 0..moved.len() {
                let (a, b) = (moved[i], moved[(i + 1) % moved.len()]);
                painter.line_segment([a, b], Stroke::new(2.0 * r, c));
                painter.circle_filled(a, r, c);
            }
        }
    };
    if shadow {
        draw(vec2(r * 0.25 + 1.0, r * 0.35 + 1.5), Color32::from_black_alpha(40), painter);
    }
    draw(egui::Vec2::ZERO, color, painter);
}

impl Journal {
    /// Every item of a page, bottom to top (and the left page's items that
    /// lie across onto this one, drawn by the left page).
    pub fn draw_page_items(&mut self, ui: &egui::Ui, b: &mut Book, cx: &mut Cx, pv: &PageView) {
        let Some(page) = b.page(pv.n).cloned() else { return };
        let path = b.page_ref(pv.n).map(|r| r.path.clone()).unwrap_or_default();
        let editing = b.editing.as_ref().filter(|e| e.page == pv.n).map(|e| e.id.clone());
        let cropping = b.crop.as_ref().filter(|c| c.0 == pv.n).map(|c| c.1.clone());
        for it in &page.items {
            let clip = if it.across { pv.clip_across } else { pv.clip };
            let painter = ui.painter_at(clip);
            if editing.as_deref() == Some(it.id.as_str()) || cropping.as_deref() == Some(it.id.as_str()) {
                continue;
            }
            self.draw_item(ui, &painter, it, pv, cx, &path);
        }
    }

    pub fn draw_item(&mut self, ui: &egui::Ui, painter: &egui::Painter, it: &Item, pv: &PageView, cx: &mut Cx, source: &str) {
        let p = cx.palette;
        let ink = it.color.as_deref().and_then(hex).unwrap_or(ink_color(p));
        let ctx = ui.ctx().clone();
        match &it.kind {
            Kind::Text => {
                let job = rich_job(it, it.content(), &pv.lay, pv.k, it.style.color.as_deref().and_then(hex).unwrap_or(ink_color(p)), p.accent_strong);
                let g = ctx.fonts(|f| f.layout_job(job));
                galley_on_lines(painter, g, pv, it, ink);
            }
            Kind::Excerpt => {
                let parts = journal::excerpt_parts(it.content());
                let slip = if p.dark { mix(paper_color(p), Color32::WHITE, 0.05) } else { Color32::from_rgb(0xfd, 0xfc, 0xf9) };
                let pad = pv.len(1.2);
                let corners: Vec<Pos2> = it.corners().iter().map(|c| pv.s(*c)).collect();
                if it.r == 0.0 {
                    let r = Rect::from_min_max(corners[0], corners[2]).expand(pad);
                    painter.rect_filled(r.translate(vec2(0.6, 0.9)), 1.0, Color32::from_black_alpha(18));
                    painter.rect_filled(r, 1.0, slip);
                } else {
                    polygon(painter, &corners, None, slip);
                }
                let color = it.style.color.as_deref().and_then(hex).unwrap_or(ink_color(p));
                let small = egui::FontId::new((line_px(it, &pv.lay, pv.k) * 0.5).clamp(5.0, 15.0), egui::FontFamily::Name("italic".into()));
                // the source on the first row, the passage under it
                let mut body = it.clone();
                if !parts.title.is_empty() {
                    let lh = pv.lay.paper.line() * it.style.lines();
                    body.y += lh;
                    let base = pv.s((it.x, it.y + lh)).y - pv.len(lh * 0.22);
                    painter.text(pos2(pv.s((it.x, 0.0)).x, base), egui::Align2::LEFT_BOTTOM, &parts.title, small.clone(), ink_color(p).gamma_multiply(0.55));
                }
                let mut job = rich_job(&body, &parts.text, &pv.lay, pv.k, color, p.accent_strong);
                if !parts.foot.is_empty() {
                    let lh = line_px(it, &pv.lay, pv.k);
                    job.append(&format!("  — {}", plain_link(&parts.foot)), 0.0, egui::TextFormat { font_id: small, color: ink_color(p).gamma_multiply(0.55), line_height: Some(lh), ..Default::default() });
                }
                let g = ctx.fonts(|f| f.layout_job(job));
                let at = text_origin(&g, pv, &body);
                painter.galley(at, g, color);
            }
            Kind::Image | Kind::Sticker | Kind::Scan => self.draw_picture(painter, &ctx, it, pv, cx, source),
            Kind::Note => {
                let target = journal::embed_target(it.content()).unwrap_or_default();
                let corners: Vec<Pos2> = it.corners().iter().map(|c| pv.s(*c)).collect();
                let slip = if p.dark { mix(paper_color(p), Color32::WHITE, 0.05) } else { Color32::from_rgb(0xfd, 0xfc, 0xf9) };
                polygon(painter, &corners, None, slip);
                painter.add(Shape::closed_line(corners.clone(), Stroke::new(1.0_f32, ink_color(p).gamma_multiply(0.12))));
                let name = target.split('#').next().unwrap_or("").rsplit('/').next().unwrap_or("").trim_end_matches(".md").to_string();
                let r = Rect::from_min_max(corners[0], corners[2]);
                let text = self.note_text(cx.vault, &target, source).unwrap_or_else(|| "Nota não encontrada".into());
                let head_h = pv.len(4.0);
                painter.text(r.left_top() + vec2(pv.len(1.5), pv.len(1.0)), egui::Align2::LEFT_TOP, format!("{}  {name}", crate::icons::FILE_TEXT), egui::FontId::new((pv.k * 2.4).clamp(6.0, 15.0), egui::FontFamily::Name("medium".into())), p.accent_strong);
                let mut inner = it.clone();
                inner.x += 1.5;
                inner.w -= 3.0;
                inner.y += 4.0;
                let job = rich_job(&inner, &text, &pv.lay, pv.k, ink_color(p), p.accent_strong);
                let g = ctx.fonts(|f| f.layout_job(job));
                let at = text_origin(&g, pv, &inner);
                let clip = painter.with_clip_rect(painter.clip_rect().intersect(Rect::from_min_max(r.min + vec2(0.0, head_h), r.max)));
                clip.galley(at, g, ink_color(p));
            }
            Kind::Ink => {
                for s in &it.strokes {
                    let c = s.color.as_deref().and_then(hex).unwrap_or(ink);
                    let pts: Vec<(Pos2, f32)> = s.pts.iter().map(|q| {
                        let (cx0, cy0) = (it.x + it.w / 2.0, it.y + it.h / 2.0);
                        let (x, y) = ateliet_core::canvas::rot_pt(it.x + q[0] * it.w, it.y + q[1] * it.h, cx0, cy0, it.r);
                        (pv.s((x, y)), (q[2] as f32 * pv.k).max(0.6))
                    }).collect();
                    stroke_path(painter, &pts, c);
                }
            }
            Kind::Line => {
                let w = it.style.lines().max(0.2) as f32;
                let pts: Vec<Pos2> = it.pts.iter().map(|q| {
                    let (cx0, cy0) = (it.x + it.w / 2.0, it.y + it.h / 2.0);
                    pv.s(ateliet_core::canvas::rot_pt(it.x + q[0] * it.w, it.y + q[1] * it.h, cx0, cy0, it.r))
                }).collect();
                painter.add(Shape::line(pts, Stroke::new((w * 0.35 * pv.k).max(0.8), ink)));
            }
            Kind::Tape => {
                let c = it.color.as_deref().and_then(hex).unwrap_or(Color32::from_rgb(0xe8, 0xd9, 0xa8));
                // serrated short ends
                let teeth = 5;
                let mut local: Vec<(f64, f64)> = vec![(0.0, 0.0), (1.0, 0.0)];
                for t in 1..=teeth * 2 {
                    let v = t as f64 / (teeth * 2) as f64;
                    local.push((if t % 2 == 1 { 0.985 } else { 1.0 }, v));
                }
                local.push((0.0, 1.0));
                for t in (1..teeth * 2).rev() {
                    let v = t as f64 / (teeth * 2) as f64;
                    local.push((if t % 2 == 1 { 0.015 } else { 0.0 }, v));
                }
                let (cx0, cy0) = (it.x + it.w / 2.0, it.y + it.h / 2.0);
                let pts: Vec<Pos2> = local.iter().map(|(u, v)| pv.s(ateliet_core::canvas::rot_pt(it.x + u * it.w, it.y + v * it.h, cx0, cy0, it.r))).collect();
                polygon(painter, &pts, None, c.gamma_multiply(0.62));
            }
            Kind::Other(_) => {
                let corners: Vec<Pos2> = it.corners().iter().map(|c| pv.s(*c)).collect();
                painter.add(Shape::dashed_line(&[corners.clone(), vec![corners[0]]].concat(), Stroke::new(1.0_f32, p.faint), 4.0, 3.0));
            }
        }
        if it.kind.has_section() && it.body.is_none() {
            let corners: Vec<Pos2> = it.corners().iter().map(|c| pv.s(*c)).collect();
            painter.add(Shape::dashed_line(&[corners.clone(), vec![corners[0]]].concat(), Stroke::new(1.2_f32, p.error), 5.0, 3.0));
            painter.text(corners[0] + vec2(4.0, 4.0), egui::Align2::LEFT_TOP, "Seção apagada no Obsidian", egui::FontId::proportional(11.0), p.error);
        }
    }

    fn draw_picture(&mut self, painter: &egui::Painter, ctx: &egui::Context, it: &Item, pv: &PageView, cx: &mut Cx, source: &str) {
        let p = cx.palette;
        let pts = outline(it, pv);
        let scr: Vec<Pos2> = pts.iter().map(|(q, _)| *q).collect();
        let border = it.look.border as f32 * pv.k;
        let edge = it.look.border_color.as_deref().and_then(hex).unwrap_or(Color32::from_rgb(0xfb, 0xfb, 0xf8));
        if border > 0.0 || it.look.shadow {
            sticker_edge(painter, &scr, border, if border > 0.0 { edge } else { Color32::TRANSPARENT }, it.look.shadow);
        }
        let target = it.picture().unwrap_or_default();
        let polaroid = it.mask.as_ref().is_some_and(|m| m.shape == "polaroide");
        let tex = self.picture(ctx, cx, &target, source, it.look.filter.as_deref());
        let Some((t, _)) = tex else {
            polygon(painter, &scr, None, ink_color(p).gamma_multiply(0.07));
            painter.text(Rect::from_points(&scr).center(), egui::Align2::CENTER_CENTER, if target.is_empty() { "Sem imagem".to_string() } else { format!("{} não encontrada", target.rsplit('/').next().unwrap_or(&target)) }, egui::FontId::proportional((pv.k * 2.2).clamp(8.0, 12.0)), p.faint);
            return;
        };
        let c = it.crop.unwrap_or(journal::Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        let tint = if it.look.multiply { Color32::from_white_alpha(225) } else { Color32::WHITE };
        if polaroid {
            polygon(painter, &scr, None, Color32::from_rgb(0xfa, 0xf9, 0xf6));
            let (wx, wy, ww, wh) = it.mask.as_ref().unwrap().window();
            let (cx0, cy0) = (it.x + it.w / 2.0, it.y + it.h / 2.0);
            let quad: Vec<(f64, f64)> = vec![(wx, wy), (wx + ww, wy), (wx + ww, wy + wh), (wx, wy + wh)];
            let s: Vec<Pos2> = quad.iter().map(|(u, v)| pv.s(ateliet_core::canvas::rot_pt(it.x + u * it.w, it.y + v * it.h, cx0, cy0, it.r))).collect();
            let uv: Vec<Pos2> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().map(|(u, v)| uv_of(&c, it.flip, *u, *v)).collect();
            polygon(painter, &s, Some((&uv, t)), tint);
            return;
        }
        let uv: Vec<Pos2> = pts.iter().map(|(_, (u, v))| uv_of(&c, it.flip, *u, *v)).collect();
        polygon(painter, &scr, Some((&uv, t)), tint);
    }
}

fn uv_of(c: &journal::Crop, flip: bool, u: f64, v: f64) -> Pos2 {
    let u = if flip { 1.0 - u } else { u };
    pos2((c.x + u * c.w) as f32, (c.y + v * c.h) as f32)
}

/// `[fonte](url)` → `fonte`; `[[a|b]]` → `b`.
pub fn plain_link(s: &str) -> String {
    if let Some(r) = s.strip_prefix("[[") {
        let inner = r.trim_end_matches("]]");
        return inner.split_once('|').map(|(_, b)| b.to_string()).unwrap_or_else(|| inner.rsplit('/').next().unwrap_or(inner).to_string());
    }
    if let (Some(a), Some(b)) = (s.find('['), s.find("](")) {
        if a < b {
            return s[a + 1..b].to_string();
        }
    }
    s.to_string()
}

/// A stroke whose width changes along it: segments with round joints.
pub fn stroke_path(painter: &egui::Painter, pts: &[(Pos2, f32)], c: Color32) {
    if pts.len() == 1 {
        painter.circle_filled(pts[0].0, pts[0].1 / 2.0, c);
        return;
    }
    for w in pts.windows(2) {
        let width = (w[0].1 + w[1].1) / 2.0;
        painter.line_segment([w[0].0, w[1].0], Stroke::new(width, c));
        painter.circle_filled(w[1].0, width / 2.0, c);
    }
}
