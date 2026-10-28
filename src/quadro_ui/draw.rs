//! Drawing a quadro: the dotted ground, drawings (from the core's rough
//! geometry), columns and cards, the selection; also the small picture of
//! a quadro embedded in a note.

use super::{Geo, View};
use crate::icons;
use crate::kit;
use crate::theme::Palette;
use ateliet_core::quadro::card::Seg;
use ateliet_core::quadro::element::{rotate, Element};
use ateliet_core::quadro::layout::{self, CardBox, LaneBox, Measure};
use ateliet_core::quadro::{rough, Board, Card, Format, Lane};
use eframe::egui::{self, pos2, vec2, Color32, FontId, Pos2, Rect, Stroke};
use std::collections::HashMap;

/// Pictures for the canvas: a vault target, or `file:<id>` for an image
/// element's own data.
pub trait Pics {
    fn tex(&mut self, ctx: &egui::Context, key: &str) -> Option<(egui::TextureId, egui::Vec2)>;
}

/// The columns and loose cards laid out (world coordinates).
#[derive(Default)]
pub struct Scene {
    pub lanes: Vec<LaneBox>,
    pub loose: Vec<CardBox>,
}

impl Scene {
    pub fn card(&self, id: &str) -> Option<&CardBox> {
        self.loose.iter().chain(self.lanes.iter().flat_map(|l| l.cards.iter())).find(|c| c.id == id)
    }
}

struct EguiMeasure<'a> {
    ctx: &'a egui::Context,
    pics: &'a mut dyn Pics,
}

impl Measure for EguiMeasure<'_> {
    fn text(&mut self, text: &str, size: f32, wrap: f32) -> (f32, f32) {
        let g = self.ctx.fonts(|f| f.layout(text.to_string(), FontId::proportional(size), Color32::WHITE, wrap));
        (g.size().x, g.size().y)
    }
    fn picture(&mut self, target: &str) -> Option<(f32, f32)> {
        self.pics.tex(self.ctx, target).map(|(_, s)| (s.x, s.y))
    }
}

pub fn layout(ctx: &egui::Context, b: &Board, pics: &mut dyn Pics) -> Scene {
    let trig = layout::triggers(b);
    let mut m = EguiMeasure { ctx, pics };
    Scene {
        lanes: b.lanes.iter().map(|l| layout::lane_box(l, &trig, &mut m)).collect(),
        loose: b.cards.iter().map(|c| layout::card_box(c, c.x, c.y, c.w, &trig, &mut m)).collect(),
    }
}

/// A font size on screen, in steps (every size is a new set of glyphs).
pub fn font_px(world: f32, zoom: f32) -> Option<f32> {
    let s = world * zoom;
    if s < 3.0 {
        None
    } else if s < 14.0 {
        Some((s * 2.0).round() / 2.0)
    } else {
        Some(s.round())
    }
}

/// Excalidraw's dark mode: `invert(93%) hue-rotate(180deg)` on the drawing.
pub fn ink(c: [u8; 4], dark: bool) -> Color32 {
    if !dark {
        return Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
    }
    let inv = |v: u8| 237.15 - 0.86 * v as f32;
    let (r, g, b) = (inv(c[0]), inv(c[1]), inv(c[2]));
    let m = [[-0.574, 1.43, 0.144], [0.426, 0.43, 0.144], [0.426, 1.43, -0.856]];
    let f = |row: [f32; 3]| (row[0] * r + row[1] * g + row[2] * b).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(f(m[0]), f(m[1]), f(m[2]), c[3])
}

pub fn ground(painter: &egui::Painter, rect: Rect, v: &View, p: Palette) {
    painter.rect_filled(rect, 0.0, p.bg);
    let step = 24.0 * v.zoom;
    if step < 8.0 {
        return;
    }
    let ox = (v.origin.x - rect.left()).rem_euclid(step);
    let oy = (v.origin.y - rect.top()).rem_euclid(step);
    let dot = p.border_strong.gamma_multiply(0.55);
    let mut y = rect.top() + oy;
    while y < rect.bottom() {
        let mut x = rect.left() + ox;
        while x < rect.right() {
            painter.circle_filled(pos2(x, y), 0.9, dot);
            x += step;
        }
        y += step;
    }
}

fn mesh_fill(painter: &egui::Painter, v: &View, pts: &[(f32, f32)], tris: &[[usize; 3]], color: Color32) {
    let mut mesh = egui::Mesh::default();
    for p in pts {
        mesh.colored_vertex(v.s(*p), color);
    }
    for t in tris {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// An image as a quad (turned with the element).
fn quad(painter: &egui::Painter, v: &View, tex: egui::TextureId, corners: [(f32, f32); 4], tint: Color32) {
    let mut mesh = egui::Mesh::with_texture(tex);
    let uv = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    for (c, u) in corners.iter().zip(uv) {
        mesh.vertices.push(egui::epaint::Vertex { pos: v.s(*c), uv: u, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

pub fn text_galley(ctx: &egui::Context, text: &str, px: f32, mono: bool, color: Color32, wrap: f32, align: &str) -> std::sync::Arc<egui::Galley> {
    let font = if mono { FontId::monospace(px) } else { FontId::proportional(px) };
    let mut job = egui::text::LayoutJob::simple(text.to_string(), font, color, wrap);
    job.halign = match align {
        "center" => egui::Align::Center,
        "right" => egui::Align::RIGHT,
        _ => egui::Align::LEFT,
    };
    ctx.fonts(|f| f.layout_job(job))
}

pub fn element(painter: &egui::Painter, ctx: &egui::Context, e: &Element, geo: &Geo, v: &View, p: Palette, pics: &mut dyn Pics, editing: bool) {
    if e.deleted() {
        return;
    }
    for (f, tris) in geo.g.fills.iter().zip(&geo.tris) {
        mesh_fill(painter, v, &f.pts, tris, ink(f.color, p.dark));
    }
    for s in &geo.g.strokes {
        let w = (s.width * v.zoom).max(0.6);
        let c = ink(s.color, p.dark);
        let pts: Vec<Pos2> = s.pts.iter().map(|q| v.s(*q)).collect();
        if s.round && w >= 2.5 {
            for q in &pts {
                painter.circle_filled(*q, w / 2.0, c);
            }
        }
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, Stroke::new(w, c)));
        }
    }
    match e.kind() {
        "text" if !editing => {
            let size = e.fo("fontSize", 20.0);
            let Some(px) = font_px(size, v.zoom) else {
                // too small to read: a hint of where the text is
                let (x0, y0, x1, y1) = e.bounds();
                painter.rect_filled(Rect::from_min_max(v.s((x0, y0 + (y1 - y0) * 0.4)), v.s((x1, y0 + (y1 - y0) * 0.6))), 1.0, ink([128, 128, 128, 90], false));
                return;
            };
            let color = ink(rough::color(e.s("strokeColor")).map(|c| rough::with_alpha(c, e.fo("opacity", 100.0) / 100.0)).unwrap_or([30, 30, 30, 255]), p.dark);
            let text = if e.s("text").is_empty() { e.raw_text() } else { e.s("text") };
            let align = e.s("textAlign");
            let wrap = if e.container().is_some() { e.w() * v.zoom + 2.0 } else { f32::INFINITY };
            let g = text_galley(ctx, text, px, e.get("fontFamily").as_i64() == Some(3), color, wrap, align);
            let anchor_x = match align {
                "center" => e.x() + e.w() / 2.0,
                "right" => e.x() + e.w(),
                _ => e.x(),
            };
            // the galley's rows are spaced by the font; Excalidraw's by lineHeight
            let top = (anchor_x, e.y());
            let a = e.angle();
            let pos = v.s(rotate(top, e.center(), a));
            painter.add(egui::Shape::Text(egui::epaint::TextShape::new(pos, g, color).with_angle(a)));
        }
        "image" => {
            let (x0, y0, x1, y1) = e.bounds();
            let a = e.angle();
            let c = e.center();
            let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|q| rotate(q, c, a));
            let key = format!("file:{}", e.s("fileId"));
            match pics.tex(ctx, &key) {
                Some((t, _)) => quad(painter, v, t, corners, Color32::WHITE.gamma_multiply(e.fo("opacity", 100.0) / 100.0)),
                None => {
                    let r = Rect::from_min_max(v.s((x0, y0)), v.s((x1, y1)));
                    painter.rect(r, 4.0, p.bg_secondary, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
                    painter.text(r.center(), egui::Align2::CENTER_CENTER, icons::IMAGE, kit::icon_font((18.0 * v.zoom).clamp(8.0, 40.0)), p.faint);
                }
            }
        }
        "frame" | "magicframe" => {
            if let Some(px) = font_px(13.0, v.zoom.max(0.6)) {
                let name = e.get("name").as_str().unwrap_or("Frame");
                painter.text(v.s((e.x(), e.y())) - vec2(0.0, 4.0), egui::Align2::LEFT_BOTTOM, name, FontId::proportional(px), p.faint);
            }
        }
        "embeddable" | "iframe" => {
            let r = Rect::from_min_max(v.s((e.bounds().0, e.bounds().1)), v.s((e.bounds().2, e.bounds().3)));
            let link = e.get("link").as_str().unwrap_or("");
            painter.text(r.center(), egui::Align2::CENTER_CENTER, format!("{} {}", icons::EXTERNAL_LINK, link.chars().take(40).collect::<String>()), FontId::proportional(12.0), p.muted);
        }
        _ => {}
    }
    // a link on the element
    if e.get("link").as_str().is_some_and(|l| !l.is_empty()) && !matches!(e.kind(), "embeddable" | "iframe") {
        let (_, y0, x1, _) = e.aabb();
        painter.text(v.s((x1, y0)) + vec2(4.0, -4.0), egui::Align2::LEFT_BOTTOM, icons::LINK, kit::icon_font(12.0), p.accent_strong);
    }
}

pub fn lane_colors(p: Palette) -> (Color32, Color32) {
    if p.dark {
        (Color32::from_rgb(0x24, 0x24, 0x26), Color32::from_rgb(0x2e, 0x2e, 0x31))
    } else {
        (Color32::from_rgb(0xf2, 0xf1, 0xee), Color32::WHITE)
    }
}

/// The header's buttons (world): collapse, add a card, menu.
pub fn lane_buttons(lb: &LaneBox) -> [Rect; 3] {
    let (x, y, w, _) = lb.rect;
    let cy = y + layout::HEADER_H / 2.0;
    let r = |i: f32| Rect::from_center_size(pos2(x + w - 18.0 - i * 26.0, cy), vec2(22.0, 22.0));
    [r(2.0), r(1.0), r(0.0)]
}

/// The add-a-card row (world).
pub fn lane_foot(lb: &LaneBox) -> Rect {
    let (x, y, w, h) = lb.rect;
    Rect::from_min_max(pos2(x + layout::LANE_PAD, y + h - layout::FOOT_H + 2.0), pos2(x + w - layout::LANE_PAD, y + h - 6.0))
}

pub fn wr(r: Rect, v: &View) -> Rect {
    Rect::from_min_max(v.s((r.min.x, r.min.y)), v.s((r.max.x, r.max.y)))
}

pub fn rr(r: layout::R) -> Rect {
    Rect::from_min_size(pos2(r.0, r.1), vec2(r.2, r.3))
}

#[allow(clippy::too_many_arguments)]
pub fn lane(painter: &egui::Painter, l: &Lane, lb: &LaneBox, v: &View, p: Palette, selected: bool, hover: Option<Pos2>, editing_title: bool) {
    let (bg, _) = lane_colors(p);
    let r = wr(rr(lb.rect), v);
    painter.rect(r, 10.0 * v.zoom.min(1.0), bg, Stroke::new(if selected { 2.0_f32 } else { 1.0 }, if selected { p.accent_strong } else { p.border }), egui::StrokeKind::Inside);
    let Some(px) = font_px(14.0, v.zoom) else { return };
    let cy = v.s((0.0, lb.rect.1 + layout::HEADER_H / 2.0)).y;
    let x = v.s((lb.rect.0 + 12.0, 0.0)).x;
    let mut tx = x;
    if !editing_title {
        let g = painter.layout_no_wrap(l.title.clone(), kit::bold(px), p.text);
        let maxw = (lb.rect.2 - 110.0).max(30.0) * v.zoom;
        let g = if g.size().x > maxw {
            let mut job = egui::text::LayoutJob::single_section(l.title.clone(), egui::TextFormat { font_id: kit::bold(px), color: p.text, ..Default::default() });
            job.wrap = egui::text::TextWrapping::truncate_at_width(maxw);
            painter.ctx().fonts(|f| f.layout_job(job))
        } else {
            g
        };
        let w = g.size().x;
        painter.galley(pos2(x, cy - g.size().y / 2.0), g, p.text);
        tx += w + 8.0 * v.zoom;
    }
    let n = l.cards.len();
    let over = l.max > 0 && n > l.max as usize;
    let count = if l.max > 0 { format!("{n}/{}", l.max) } else { n.to_string() };
    if !editing_title {
        let cr = painter.text(pos2(tx, cy), egui::Align2::LEFT_CENTER, count, FontId::proportional(px * 0.85), if over { p.error } else { p.faint });
        if l.complete.is_some() {
            painter.text(pos2(cr.right() + 6.0 * v.zoom, cy), egui::Align2::LEFT_CENTER, icons::CHECK, kit::icon_font(px * 0.85), p.faint);
        }
    }
    let [col, add, menu] = lane_buttons(lb);
    for (rect, glyph) in [(col, if l.collapsed { icons::CHEVRON_RIGHT } else { icons::CHEVRON_DOWN }), (add, icons::PLUS), (menu, icons::ELLIPSIS)] {
        let sr = wr(rect, v);
        if hover.is_some_and(|h| sr.contains(h)) {
            painter.rect_filled(sr, 5.0, p.bg_hover);
        }
        painter.text(sr.center(), egui::Align2::CENTER_CENTER, glyph, kit::icon_font(px), p.muted);
    }
    if !l.collapsed {
        let fr = wr(lane_foot(lb), v);
        let hov = hover.is_some_and(|h| fr.contains(h));
        if hov {
            painter.rect_filled(fr, 6.0, p.bg_hover);
        }
        let full = l.max > 0 && n >= l.max as usize;
        let label = if full { format!("{}  Coluna cheia", icons::LOCK) } else { format!("{}  Adicionar cartão", icons::PLUS) };
        let mut job = egui::text::LayoutJob::default();
        let (glyph, text) = label.split_once("  ").unwrap();
        job.append(glyph, 0.0, egui::TextFormat { font_id: kit::icon_font(px * 0.9), color: if hov { p.text } else { p.faint }, valign: egui::Align::Center, ..Default::default() });
        job.append(text, 6.0 * v.zoom, egui::TextFormat { font_id: FontId::proportional(px * 0.9), color: if hov { p.text } else { p.faint }, valign: egui::Align::Center, ..Default::default() });
        let g = painter.ctx().fonts(|f| f.layout_job(job));
        painter.galley(pos2(fr.left() + 6.0 * v.zoom, fr.center().y - g.size().y / 2.0), g, p.faint);
    }
}

/// The checkbox of a card (world).
pub fn check_rect(cb: &CardBox) -> Rect {
    let (x, y, w, _) = cb.rect;
    Rect::from_min_size(pos2(x + w - layout::CARD_PAD - 15.0, y + layout::CARD_PAD + 1.0), vec2(15.0, 15.0))
}

#[allow(clippy::too_many_arguments)]
pub fn card(painter: &egui::Painter, ctx: &egui::Context, b: &Board, c: &Card, cb: &CardBox, v: &View, p: Palette, pics: &mut dyn Pics, selected: bool, hovered: bool, ghost: bool) {
    let (_, bg) = lane_colors(p);
    let r = wr(rr(cb.rect), v);
    let fade = if ghost { 0.55 } else { 1.0 };
    if !ghost {
        painter.rect_filled(r.translate(vec2(0.0, 1.5 * v.zoom.min(1.0))), 8.0 * v.zoom.min(1.0), Color32::from_black_alpha(if p.dark { 60 } else { 14 }));
    }
    painter.rect(r, 8.0 * v.zoom.min(1.0), bg.gamma_multiply(fade), Stroke::new(if selected { 2.0_f32 } else { 1.0 }, if selected { p.accent_strong } else if hovered { p.border_strong } else { p.border }), egui::StrokeKind::Inside);
    if let Some(col) = c.color.as_deref().and_then(rough::color) {
        let s = Rect::from_min_size(r.min + vec2(0.0, 6.0 * v.zoom), vec2(3.0 * v.zoom.max(0.5), (r.height() - 12.0 * v.zoom).max(1.0)));
        painter.rect_filled(s, 1.5, Color32::from_rgb(col[0], col[1], col[2]));
    }
    if let Some((target, pr)) = &cb.picture {
        let rect = wr(rr(*pr), v);
        match pics.tex(ctx, target) {
            Some((t, _)) => {
                painter.image(t, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE.gamma_multiply(fade));
            }
            None => {
                painter.rect_filled(rect, 4.0, p.bg_secondary);
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, icons::IMAGE, kit::icon_font((16.0 * v.zoom).clamp(8.0, 32.0)), p.faint);
            }
        }
    }
    let Some(px) = font_px(layout::TITLE_SIZE, v.zoom) else {
        // a line where the title is
        if cb.title_rect.3 > 0.0 {
            let tr = wr(rr(cb.title_rect), v);
            painter.rect_filled(Rect::from_min_size(tr.min, vec2(tr.width() * 0.7, (tr.height() * 0.5).max(1.0))), 1.0, p.faint.gamma_multiply(0.5));
        }
        return;
    };
    let done = c.check != ' ';
    let note_card = cb.info.is_note_card();
    if !cb.title.is_empty() {
        let base = if done { p.muted } else { p.text };
        let mut job = egui::text::LayoutJob::default();
        let mut picture_done = false;
        for s in &cb.info.segs {
            let (text, link) = match s {
                Seg::Text(t) => (t.clone(), false),
                Seg::Link(_, a) | Seg::Url(_, a) => (a.clone(), true),
                Seg::Embed(t) if ateliet_core::quadro::card::is_picture(t) && !picture_done => {
                    picture_done = true;
                    continue;
                }
                Seg::Embed(t) => (ateliet_core::quadro::card::link_target(t).1, true),
            };
            let fmt = egui::TextFormat { font_id: if note_card { kit::medium(px) } else { FontId::proportional(px) }, color: if link { p.accent_strong } else { base }, underline: if link && !note_card { Stroke::new(1.0_f32, p.accent_strong.gamma_multiply(0.5)) } else { Stroke::NONE }, ..Default::default() };
            job.append(&text, 0.0, fmt);
        }
        job.wrap.max_width = cb.title_rect.2 * v.zoom + 1.0;
        let g = ctx.fonts(|f| f.layout_job(job));
        let tr = wr(rr(cb.title_rect), v);
        painter.galley(tr.min, g, base);
    }
    for (tag, cr) in &cb.chips {
        let (fg, bgc) = layout::tag_colors(b, tag);
        let rect = wr(rr(*cr), v);
        let (fill, text) = if p.dark { (Color32::from_rgb(bgc[0], bgc[1], bgc[2]), Color32::from_rgb(fg[0], fg[1], fg[2])) } else {
            let mix = |a: u8, t: f32| (a as f32 + (255.0 - a as f32) * t) as u8;
            (Color32::from_rgb(mix(fg[0], 0.82), mix(fg[1], 0.82), mix(fg[2], 0.82)), Color32::from_rgb((fg[0] as f32 * 0.75) as u8, (fg[1] as f32 * 0.75) as u8, (fg[2] as f32 * 0.75) as u8))
        };
        painter.rect_filled(rect, 6.0 * v.zoom.min(1.0), fill.gamma_multiply(fade));
        if let Some(cpx) = font_px(layout::CHIP_SIZE, v.zoom) {
            painter.text(pos2(rect.left() + 7.0 * v.zoom, rect.center().y), egui::Align2::LEFT_CENTER, tag, kit::medium(cpx), text);
        }
    }
    if let Some((when, mr)) = &cb.meta {
        if let Some(mpx) = font_px(layout::META_SIZE, v.zoom) {
            let rect = wr(rr(*mr), v);
            let overdue = cb.info.date.as_deref().is_some_and(|d| d < ateliet_core::datefmt::today().format("%Y-%m-%d").to_string().as_str()) && !done;
            let col = if overdue { p.error } else { p.muted };
            painter.text(rect.min + vec2(0.0, rect.height() / 2.0), egui::Align2::LEFT_CENTER, icons::CALENDAR, kit::icon_font(mpx), col);
            painter.text(rect.min + vec2(mpx + 5.0 * v.zoom, rect.height() / 2.0), egui::Align2::LEFT_CENTER, when, FontId::proportional(mpx), col);
        }
    }
    let show_checks = b.kept.settings.get("show-checkboxes").and_then(|v| v.as_bool()).unwrap_or(false);
    if hovered || done || show_checks {
        let cr = wr(check_rect(cb), v);
        if done {
            painter.rect_filled(cr, 3.0, p.accent_strong);
            painter.text(cr.center(), egui::Align2::CENTER_CENTER, icons::CHECK, kit::icon_font(cr.height() * 0.8), p.on_accent);
        } else {
            painter.rect_stroke(cr, 3.0, Stroke::new(1.2_f32, p.border_strong), egui::StrokeKind::Inside);
        }
    }
}

/// The whole quadro, scaled into `rect` (embeds in notes).
pub fn preview(ui: &mut egui::Ui, b: &Board, p: Palette, pics: &mut dyn Pics, geos: &mut HashMap<String, (u64, Geo)>, max_h: f32) -> egui::Response {
    let ctx = ui.ctx().clone();
    let scene = layout(&ctx, b, pics);
    let mut bounds: Option<Rect> = None;
    let mut add = |r: Rect| bounds = Some(bounds.map_or(r, |b| b.union(r)));
    for l in &scene.lanes {
        add(rr(l.rect));
    }
    for c in &scene.loose {
        add(rr(c.rect));
    }
    for e in b.elements.iter().filter(|e| !e.deleted()) {
        let (x0, y0, x1, y1) = e.aabb();
        add(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)));
    }
    let w = ui.available_width();
    let Some(bx) = bounds else {
        let (r, resp) = ui.allocate_exact_size(vec2(w, 60.0), egui::Sense::click());
        ui.painter().rect(r, 8.0, p.bg_secondary, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Quadro vazio", FontId::proportional(13.0), p.faint);
        return resp;
    };
    let bx = bx.expand(24.0);
    let zoom = (w / bx.width()).min(max_h / bx.height()).min(1.0);
    let h = bx.height() * zoom;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let v = View { origin: pos2(rect.center().x - bx.center().x * zoom, rect.top() - bx.top() * zoom), zoom };
    painter.rect_filled(rect, 8.0, p.bg);
    for e in b.elements.iter().filter(|e| !e.deleted()) {
        let g = super::geo_of(geos, e);
        element(&painter, &ctx, e, g, &v, p, pics, false);
    }
    for (l, lb) in b.lanes.iter().zip(&scene.lanes) {
        lane(&painter, l, lb, &v, p, false, None, false);
        for (c, cb) in l.cards.iter().zip(&lb.cards) {
            card(&painter, &ctx, b, c, cb, &v, p, pics, false, false, false);
        }
    }
    for (c, cb) in b.cards.iter().zip(&scene.loose) {
        card(&painter, &ctx, b, c, cb, &v, p, pics, false, false, false);
    }
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
    let label = match b.format {
        Format::Kanban => icons::SQUARE_KANBAN,
        _ => icons::PEN_TOOL,
    };
    painter.text(rect.right_top() + vec2(-10.0, 10.0), egui::Align2::RIGHT_TOP, label, kit::icon_font(13.0), p.faint);
    resp
}
