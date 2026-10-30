//! The notebook open at a spread: where the pages are on screen, what the
//! pointer and the keys do on them, the text being typed in place, and the
//! picture being cropped.

use super::draw::{self, PageView};
use super::{typeset, Book, Cx, Drag, Journal, Look, Payload, Tool};
use ateliet_core::canvas::{self, Rect as MRect};
use ateliet_core::journal::{self, Item, Kind, Mask};
use ateliet_core::moodboard as mb;
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Stroke};
use std::time::{Duration, Instant};

/// How far stickers may hang past a page's outer edges.
const OVERHANG: f32 = 8.0;

impl Book {
    /// The pages of the spread on screen.
    pub fn views(&self, canvas: Rect) -> (Vec<PageView>, Rect) {
        let (w, h) = (self.nb.size.w as f32, self.nb.size.h as f32);
        let room = Rect::from_min_max(canvas.min + vec2(40.0, 30.0), canvas.max - vec2(40.0, 76.0));
        if self.look == Look::Cover {
            // the closed notebook, alone in the middle
            let k = (room.width() / w).min((room.height() - 40.0) / h).max(0.5) * self.zoom;
            let rect = Rect::from_center_size(room.center() + vec2(0.0, 20.0) + self.pan, vec2(w * k, h * k));
            let o = OVERHANG * k;
            let clip = rect.expand(o).intersect(canvas);
            return (vec![PageView { n: 0, rect, k, lay: self.layout(0), clip, clip_across: clip }], rect);
        }
        let fit = (room.width() / (2.0 * w)).min(room.height() / h).max(0.5);
        let k = fit * self.zoom;
        let center = room.center() + self.pan;
        let spread = Rect::from_center_size(center, vec2(2.0 * w * k, h * k));
        let (l, r) = self.spread_pages();
        let mut out = Vec::new();
        let left_rect = Rect::from_min_max(spread.min, pos2(center.x, spread.max.y));
        let right_rect = Rect::from_min_max(pos2(center.x, spread.min.y), spread.max);
        let o = OVERHANG * k;
        if let Some(n) = l {
            let clip = Rect::from_min_max(left_rect.min - vec2(o, o), pos2(left_rect.max.x, left_rect.max.y + o)).intersect(canvas);
            let clip_across = Rect::from_min_max(left_rect.min - vec2(o, o), right_rect.max + vec2(o, o)).intersect(canvas);
            out.push(PageView { n, rect: left_rect, k, lay: self.layout(n), clip, clip_across });
        }
        if let Some(n) = r {
            let clip = Rect::from_min_max(pos2(right_rect.min.x, right_rect.min.y - o), right_rect.max + vec2(o, o)).intersect(canvas);
            out.push(PageView { n, rect: right_rect, k, lay: self.layout(n), clip, clip_across: clip });
        }
        (out, spread)
    }

    /// Is page `n` on the spread shown?
    pub fn views_show(&self, n: usize) -> bool {
        let (l, r) = self.spread_pages();
        l == Some(n) || r == Some(n)
    }

    fn sel_items(&mut self) -> Vec<Item> {
        let ids = self.sel.clone();
        let n = self.sel_page;
        self.page(n).map(|p| p.items.iter().filter(|i| ids.contains(&i.id)).cloned().collect()).unwrap_or_default()
    }

    pub fn delete_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint();
        let n = self.sel_page;
        let ids = std::mem::take(&mut self.sel);
        // a box in the middle of a chain: its neighbours join round it
        let mut relink = Vec::new();
        if let Some(p) = self.page(n) {
            for id in &ids {
                if let Some(f) = p.item(id).and_then(|i| i.flow.clone()) {
                    relink.push(f);
                }
            }
            p.items.retain(|i| !ids.contains(&i.id));
            for it in p.items.iter_mut().filter(|i| i.anchor.as_ref().is_some_and(|a| ids.contains(a))) {
                it.anchor = None;
            }
        }
        for f in relink {
            if let Some((pn, _)) = f.prev.as_ref().and_then(|p| self.find(p)) {
                let pid = f.prev.clone().unwrap();
                if let Some(it) = self.page(pn).and_then(|pg| pg.item_mut(&pid)) {
                    if let Some(fl) = it.flow.as_mut() {
                        fl.next = f.next.clone();
                    }
                }
                self.touch(pn);
                self.unsettled.insert(pn);
                self.repull.insert(pid);
            }
            if let Some((nn, _)) = f.next.as_ref().and_then(|p| self.find(p)) {
                let nid = f.next.clone().unwrap();
                if let Some(it) = self.page(nn).and_then(|pg| pg.item_mut(&nid)) {
                    if let Some(fl) = it.flow.as_mut() {
                        fl.prev = f.prev.clone();
                        fl.para = fl.para && fl.prev.is_some();
                    }
                    if it.flow.as_ref().is_some_and(|fl| fl.prev.is_none() && fl.next.is_none()) {
                        it.flow = None;
                    }
                }
                self.touch(nn);
            }
        }
        self.touch(n);
    }

    pub fn duplicate_selection(&mut self) {
        let items = self.sel_items();
        if items.is_empty() {
            return;
        }
        self.checkpoint();
        let n = self.sel_page;
        let mut new_ids = Vec::new();
        if let Some(p) = self.page(n) {
            for it in items {
                let mut c = it.clone();
                c.id = Item::new(c.kind.clone(), 0.0, 0.0, 0.0, 0.0).id;
                c.x += 5.0;
                c.y += 5.0;
                c.flow = None;
                c.anchor = None;
                new_ids.push(c.id.clone());
                p.items.push(c);
            }
        }
        self.sel = new_ids;
        self.touch(n);
    }

    /// Up (`front`) or down one layer, past the next item it overlaps;
    /// to the top or the bottom with `all`.
    pub fn layer(&mut self, front: bool, all: bool) {
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint();
        let n = self.sel_page;
        let ids = self.sel.clone();
        if let Some(p) = self.page(n) {
            let items = &mut p.items;
            if all {
                let (sel, rest): (Vec<Item>, Vec<Item>) = items.drain(..).partition(|i| ids.contains(&i.id));
                *items = if front { [rest, sel].concat() } else { [sel, rest].concat() };
            } else if front {
                for idx in (0..items.len()).rev() {
                    if !ids.contains(&items[idx].id) {
                        continue;
                    }
                    let bb = items[idx].bbox();
                    if let Some(j) = (idx + 1..items.len()).find(|&j| !ids.contains(&items[j].id) && items[j].bbox().intersects(&bb)) {
                        let it = items.remove(idx);
                        items.insert(j, it);
                    }
                }
            } else {
                for idx in 0..items.len() {
                    if !ids.contains(&items[idx].id) {
                        continue;
                    }
                    let bb = items[idx].bbox();
                    if let Some(j) = (0..idx).rev().find(|&j| !ids.contains(&items[j].id) && items[j].bbox().intersects(&bb)) {
                        let it = items.remove(idx);
                        items.insert(j, it);
                    }
                }
            }
        }
        self.touch(n);
    }
}

/// Is a point inside an item (its box, turned)?
fn hit_item(it: &Item, at: (f64, f64)) -> bool {
    let pad = if matches!(it.kind, Kind::Ink | Kind::Line) { 1.5 } else { 0.0 };
    canvas::contains(it.x - pad, it.y - pad, it.w + 2.0 * pad, it.h + 2.0 * pad, it.r, at.0, at.1)
}

/// Which handle of an item: the side it moves, as (sx, sy) in −1..1.
type Handle = (i8, i8);

/// The handles of a selected item on screen: corners and sides, and the
/// rotation knob above it.
fn handles(it: &Item, pv: &PageView) -> (Vec<(Handle, Pos2)>, Pos2) {
    let (cx, cy) = (it.x + it.w / 2.0, it.y + it.h / 2.0);
    let at = |sx: f64, sy: f64| pv.s(canvas::rot_pt(cx + sx * it.w / 2.0, cy + sy * it.h / 2.0, cx, cy, it.r));
    let text = it.kind.is_text();
    let mut v = Vec::new();
    if !text {
        for (sx, sy) in [(-1, -1), (1, -1), (1, 1), (-1, 1)] {
            v.push(((sx, sy), at(sx as f64, sy as f64)));
        }
    }
    if text || matches!(it.kind, Kind::Note | Kind::Tape) {
        v.push(((-1, 0), at(-1.0, 0.0)));
        v.push(((1, 0), at(1.0, 0.0)));
    }
    if matches!(it.kind, Kind::Note) {
        v.push(((0, 1), at(0.0, 1.0)));
    }
    let top = at(0.0, -1.0);
    let (s, c) = it.r.to_radians().sin_cos();
    let knob = top + vec2(s as f32, -c as f32) * 22.0;
    (v, knob)
}

/// The new box of an item resized from handle (sx, sy) toward `p` (page mm).
fn resize(orig: &Item, h: (i8, i8), p: (f64, f64), free: bool) -> (f64, f64, f64, f64) {
    let (sx, sy) = (h.0 as f64, h.1 as f64);
    let (cx, cy) = (orig.x + orig.w / 2.0, orig.y + orig.h / 2.0);
    let a = canvas::rot_pt(cx - sx * orig.w / 2.0, cy - sy * orig.h / 2.0, cx, cy, orig.r);
    let (s, c) = orig.r.to_radians().sin_cos();
    let (vx, vy) = (p.0 - a.0, p.1 - a.1);
    let (lx, ly) = (vx * c + vy * s, -vx * s + vy * c);
    let min = 3.0;
    let (nw, nh) = if sy == 0.0 {
        ((sx * lx).max(min * 3.0), orig.h)
    } else if sx == 0.0 {
        (orig.w, (sy * ly).max(min * 3.0))
    } else if free || orig.kind == Kind::Tape {
        ((sx * lx).max(min), (sy * ly).max(min))
    } else {
        let k = (min / orig.w.min(orig.h)).max(((sx * lx) / orig.w).max((sy * ly) / orig.h));
        (orig.w * k, orig.h * k)
    };
    let (ox, oy) = (if sx == 0.0 { 0.0 } else { sx * nw / 2.0 }, if sy == 0.0 { 0.0 } else { sy * nh / 2.0 });
    let (ax, ay) = if sx == 0.0 || sy == 0.0 {
        // a side: the anchor is the middle of the opposite side
        canvas::rot_pt(cx - sx * orig.w / 2.0, cy - sy * orig.h / 2.0, cx, cy, orig.r)
    } else {
        a
    };
    let (ncx, ncy) = (ax + ox * c - oy * s, ay + ox * s + oy * c);
    (ncx - nw / 2.0, ncy - nh / 2.0, nw, nh)
}

fn mb_item(it: &Item) -> mb::Item {
    let mut m = mb::Item::image("x", it.x, it.y, it.w, it.h);
    m.r = it.r;
    m.crop = it.crop;
    m.flip = it.flip;
    m
}

impl Journal {
    pub fn notebook_ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let Some(mut b) = self.book.take() else { return };
        let ctx = ui.ctx().clone();
        // the typing went on to another page last frame: that page is shown
        if let Some(n) = b.follow.take() {
            if !b.views_show(n) {
                b.go_to(n);
            }
        }
        // what is typed this frame shows this frame
        if b.look != Look::Thumbs {
            let views = b.views(ui.available_rect_before_wrap()).0;
            self.type_keys(ui, &mut b, &views);
        }
        let canvas = ui.available_rect_before_wrap();
        let p = cx.palette;
        let desk = if self.immersive { crate::theme::mix(draw::desk_color(p), Color32::BLACK, if p.dark { 0.35 } else { 0.12 }) } else { draw::desk_color(p) };
        ui.painter().rect_filled(canvas, 0.0, desk);
        if b.look == Look::Thumbs {
            self.thumbs_ui(ui, &mut b, cx, canvas);
            self.book = Some(b);
            return;
        }
        let resp = ui.interact(canvas, ui.id().with("journal-spread"), egui::Sense::click_and_drag());
        self.zoom_and_swipe(ui, &mut b, canvas, &resp);
        self.follow_writing(ui, &mut b, canvas);
        // a page turning: the spread slides in from the side it comes from
        let slide = match b.turned {
            Some((t, dir)) if t.elapsed() < Duration::from_millis(260) => {
                ctx.request_repaint();
                let x = 1.0 - t.elapsed().as_secs_f32() / 0.26;
                dir * 70.0 * x * x
            }
            _ => {
                b.turned = None;
                0.0
            }
        };
        b.pan.x += slide;
        let (views, spread) = b.views(canvas);
        b.pan.x -= slide;
        // the pages set: every text where its rows are
        self.settle_shown(&ctx, &mut b, &views, cx.palette);
        let painter = ui.painter_at(canvas);
        // the shadow under the open notebook, the paper, the spine
        let pages_rect = views.iter().map(|v| v.rect).reduce(|a, c| a.union(c)).unwrap_or(spread);
        painter.add(egui::epaint::RectShape::filled(pages_rect, 2.0, Color32::from_black_alpha(if p.dark { 90 } else { 28 })).with_blur_width(18.0));
        for pv in &views {
            if pv.n == 0 {
                let page = b.page(0).cloned();
                let nb = b.nb.clone();
                self.draw_cover(ui.ctx(), &painter, &nb, page.as_ref(), pv.rect, cx);
            } else {
                draw::paper(&painter, pv, p);
            }
        }
        if views.len() == 2 {
            let left_n = views[0].n;
            draw::spine(&painter, spread, left_n.saturating_sub(1), b.count().saturating_sub(views[1].n), p);
        } else if let Some(v) = views.first() {
            let edge = Stroke::new(1.0_f32, draw::ink_color(p).gamma_multiply(0.16));
            let left = v.n.saturating_sub(1);
            let right = b.count().saturating_sub(v.n);
            for k in 0..(if v.lay.right { (1 + right / 24).min(3) * usize::from(right > 0) } else { (1 + left / 24).min(3) * usize::from(left > 0) }) {
                let d = 2.0 + k as f32 * 2.0;
                let x = if v.lay.right { v.rect.right() + d } else { v.rect.left() - d };
                painter.vline(x, (v.rect.top() + d)..=(v.rect.bottom() - d), edge);
            }
        }
        for pv in &views {
            self.draw_page_items(ui, &mut b, cx, pv);
        }
        // the empty side after the last page: a page waiting to be made
        if let (Look::Spread, (Some(_), None)) = (b.look, b.spread_pages()) {
            let slot = Rect::from_min_max(pos2(spread.center().x, spread.top()), spread.max).shrink(14.0);
            let r = ui.interact(slot, ui.id().with("journal-new-page"), egui::Sense::click());
            let c = if r.hovered() { p.accent_strong } else { draw::ink_color(p).gamma_multiply(0.25) };
            let pts = [slot.left_top(), slot.right_top(), slot.right_bottom(), slot.left_bottom(), slot.left_top()];
            painter.add(egui::Shape::dashed_line(&pts, Stroke::new(1.2_f32, c), 6.0, 5.0));
            painter.text(slot.center(), egui::Align2::CENTER_CENTER, format!("{}  Página nova", crate::icons::PLUS), egui::FontId::proportional(14.0), c);
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.clicked() {
                b.new_page_here(&mut self.out);
            }
        }
        // pictures sent from image mode: on the page shown on the right
        let sent = std::mem::take(&mut self.pending_pictures);
        if let Some(pv) = views.last().cloned().filter(|_| !sent.is_empty()) {
            for (k, key) in sent.into_iter().enumerate() {
                let at = (pv.lay.w / 2.0 + k as f64 * 6.0, pv.lay.h / 2.5 + k as f64 * 6.0);
                self.place(&ctx, &mut b, cx, &pv, at, Payload::Picture(key), false);
            }
        }
        // excerpts and cuts from the reader beside: on the page on the right
        let came = std::mem::take(&mut self.pending_snippets);
        if let Some(pv) = views.last().cloned().filter(|_| !came.is_empty()) {
            for (note, sn) in came {
                self.place_snippet(&mut b, pv.n, &pv.lay, None, &note, &sn);
            }
        }
        self.pointer(ui, &mut b, cx, &views, &resp);
        self.drops(ui, &mut b, cx, &views);
        self.draw_selection(ui, &mut b, &views, p);
        self.crop_overlay(ui, &mut b, cx, &views);
        self.typing_overlay(ui, &mut b, &views, p);
        if !ctx.wants_keyboard_input() && b.editing.is_none() {
            self.keys(ui, &mut b, &views);
        }
        self.chrome(ui, &mut b, cx, canvas, &views);
        self.book = Some(b);
    }

    /// The immersive mode follows what is being written: the view comes
    /// close until the letters are a comfortable size and keeps the line
    /// being typed at the same height, like a typewriter; when typing
    /// stops, it goes back to the whole spread.
    fn follow_writing(&mut self, ui: &egui::Ui, b: &mut Book, canvas: Rect) {
        let room_c = Rect::from_min_max(canvas.min + vec2(40.0, 30.0), canvas.max - vec2(40.0, 76.0)).center();
        let zoom_to = |b: &mut Book, z: f32, at: Pos2| {
            let old = b.zoom;
            b.zoom = z;
            let rel = at - (room_c + b.pan);
            b.pan += rel - rel * (b.zoom / old);
        };
        let target = b.editing.as_ref().map(|e| (e.page, e.id.clone())).filter(|_| self.immersive);
        match (target, b.caret) {
            (Some((n, id)), Some(caret)) => {
                let Some(it) = b.page(n).and_then(|p| p.item(&id)).cloned() else { return };
                let (views, _) = b.views(canvas);
                let Some(pv) = views.iter().find(|v| v.n == n) else { return };
                let lh = draw::line_px(&it, &pv.lay, pv.k);
                let want = (b.zoom * 34.0 / lh.max(1.0)).clamp(1.0, 5.0);
                let z = b.zoom + (want - b.zoom) * 0.18;
                zoom_to(b, z, caret);
                // the line typed at two fifths of the height; the box centred
                let box_x = pv.s((it.x + it.w / 2.0, it.y)).x;
                let goal = egui::vec2(canvas.center().x - box_x, canvas.top() + canvas.height() * 0.42 - caret.y);
                b.pan += goal * 0.18;
                b.auto_zoom = true;
                if goal.length() > 1.0 || (want - b.zoom).abs() > 0.01 {
                    ui.ctx().request_repaint();
                }
            }
            _ if b.auto_zoom && b.editing.is_none() => {
                let z = b.zoom + (1.0 - b.zoom) * 0.2;
                zoom_to(b, z, canvas.center());
                b.pan *= 0.8;
                if (b.zoom - 1.0).abs() < 0.01 && b.pan.length() < 1.0 {
                    b.zoom = 1.0;
                    b.pan = egui::Vec2::ZERO;
                    b.auto_zoom = false;
                }
                ui.ctx().request_repaint();
            }
            _ => {}
        }
    }

    fn zoom_and_swipe(&mut self, ui: &egui::Ui, b: &mut Book, canvas: Rect, resp: &egui::Response) {
        let (zoom, scroll, hover) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
        let Some(h) = hover.filter(|h| canvas.contains(*h)) else { return };
        if !resp.hovered() && !resp.dragged() {
            return;
        }
        if zoom != 1.0 {
            let old = b.zoom;
            b.zoom = (b.zoom * zoom).clamp(1.0, 6.0);
            // keep the point under the pointer where it is
            let room_c = Rect::from_min_max(canvas.min + vec2(40.0, 30.0), canvas.max - vec2(40.0, 76.0)).center();
            let rel = h - (room_c + b.pan);
            b.pan += rel - rel * (b.zoom / old);
            if b.zoom <= 1.0 {
                b.pan = egui::Vec2::ZERO;
            }
            return;
        }
        if b.zoom > 1.0 {
            b.pan += scroll;
            return;
        }
        // a sideways swipe turns the page
        if b.turn_cooldown.is_some_and(|t| t.elapsed() < Duration::from_millis(550)) {
            b.swipe = 0.0;
            return;
        }
        b.swipe += scroll.x;
        if b.swipe.abs() > 90.0 {
            let fwd = b.swipe < 0.0;
            b.swipe = 0.0;
            b.turn(fwd);
            b.turn_cooldown = Some(Instant::now());
        }
        if scroll.x == 0.0 {
            b.swipe *= 0.9;
        }
    }

    /// The page under a screen point: its view and the point on it.
    fn at(views: &[PageView], q: Pos2) -> Option<(&PageView, (f64, f64))> {
        views.iter().find(|v| v.rect.contains(q)).or_else(|| views.iter().find(|v| v.clip.contains(q))).map(|v| (v, v.w(q)))
    }

    fn pointer(&mut self, ui: &egui::Ui, b: &mut Book, _cx: &mut Cx, views: &[PageView], resp: &egui::Response) {
        let Some(q) = resp.interact_pointer_pos().or(resp.hover_pos()) else { return };
        let (shift, alt, cmd) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt, i.modifiers.command));
        let thr = |pv: &PageView| (6.0 / pv.k) as f64;
        if b.crop.is_some() {
            self.crop_pointer(ui, b, views, resp);
            return;
        }
        // the cursor
        if resp.hovered() && b.drag.is_none() {
            let icon = match self.tool {
                Tool::Write => egui::CursorIcon::Text,
                Tool::Pen | Tool::Line | Tool::Tape => egui::CursorIcon::Crosshair,
                // letters and paper take typing (the I-beam); other things move
                Tool::Select => {
                    let over_handle = self.handle_at(b, views, q).is_some();
                    if over_handle {
                        egui::CursorIcon::Grab
                    } else if let Some((pv, at)) = Self::at(views, q).filter(|(pv, _)| pv.rect.contains(q)) {
                        match self.hit_at(b, pv.n, at).and_then(|id| b.page(pv.n).and_then(|p| p.item(&id)).map(|i| i.kind.is_text())) {
                            Some(false) => egui::CursorIcon::Move,
                            _ => egui::CursorIcon::Text,
                        }
                    } else {
                        egui::CursorIcon::Default
                    }
                }
            };
            ui.ctx().set_cursor_icon(icon);
        }
        // the middle button or space drags the view when it is zoomed
        if resp.drag_started_by(egui::PointerButton::Middle) || (resp.drag_started() && ui.input(|i| i.key_down(egui::Key::Space))) {
            b.drag = Some(Drag::Pan);
        }
        if resp.drag_started_by(egui::PointerButton::Primary) && b.drag.is_none() {
            let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(q);
            self.press(ui, b, views, start, shift, alt);
        }
        if resp.dragged() {
            if let Some(d) = b.drag.clone() {
                self.drag_to(ui, b, views, q, d, shift, cmd, thr);
            }
        }
        if resp.drag_stopped() {
            self.release(ui.ctx(), b, views, shift);
        }
        if resp.clicked() {
            let count = if resp.triple_clicked() { 3 } else if resp.double_clicked() { 2 } else { 1 };
            self.click(ui.ctx(), b, views, q, shift, count);
        }
        if resp.double_clicked() {
            if let Some((pv, at)) = Self::at(views, q) {
                if let Some(id) = self.hit_at(b, pv.n, at) {
                    let kind = b.page(pv.n).and_then(|p| p.item(&id)).map(|i| i.kind.clone());
                    match kind {
                        // (a text's double click picks a word: done with the click)
                        Some(Kind::Text) | Some(Kind::Excerpt) => {}
                        Some(Kind::Image) => self.start_crop(b, pv.n, &id),
                        Some(Kind::Note) => {
                            let target = b.page(pv.n).and_then(|p| p.item(&id)).and_then(|i| journal::embed_target(i.content()));
                            if let Some(t) = target {
                                self.out.push(super::Out::Open(t.split('#').next().unwrap_or("").to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if resp.secondary_clicked() {
            if let Some((pv, at)) = Self::at(views, q) {
                if let Some(id) = self.hit_at(b, pv.n, at) {
                    if !b.sel.contains(&id) || b.sel_page != pv.n {
                        b.sel_page = pv.n;
                        b.sel = vec![id];
                    }
                }
            }
        }
    }

    /// The selected item's handle under the pointer: its page, its id, and
    /// which handle (None for the rotation knob).
    fn handle_at(&self, b: &mut Book, views: &[PageView], q: Pos2) -> Option<(usize, String, Option<Handle>)> {
        if b.sel.len() != 1 {
            return None;
        }
        let pv = views.iter().find(|v| v.n == b.sel_page)?;
        let id = b.sel[0].clone();
        let it = b.page(pv.n)?.item(&id)?.clone();
        if it.locked {
            return None;
        }
        let (hs, knob) = handles(&it, pv);
        if knob.distance(q) < 8.0 {
            return Some((pv.n, id, None));
        }
        hs.into_iter().find(|(_, at)| at.distance(q) < 7.0).map(|(h, _)| (pv.n, id, Some(h)))
    }

    fn press(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView], q: Pos2, shift: bool, alt: bool) {
        // on the letters of a text: the press puts the cursor there and
        // starts a selection, as in any editor. A text moves when it was
        // picked first (`Esc`, or with others), or with `Alt`.
        if !alt && matches!(self.tool, Tool::Select | Tool::Write) {
            if let Some((pv, at)) = Self::at(views, q).map(|(pv, at)| (pv.clone(), at)) {
                let n = pv.n;
                let typing = b.editing.as_ref().map(|e| (e.page, e.id.clone()));
                let id = typing.clone().filter(|(en, id)| *en == n && self.shown.get(&n).and_then(|s| s.set.get(id)).is_some_and(|set| typeset::on_letters(set, at))).map(|t| t.1).or_else(|| self.hit_at(b, n, at));
                let text = id.filter(|id| b.page(n).and_then(|p| p.item(id)).is_some_and(|i| i.kind.is_text() && !i.locked));
                let picked = |id: &str| b.editing.is_none() && b.sel_page == n && b.sel.iter().any(|s| s == id);
                let same = |id: &str| typing.as_ref().is_some_and(|(en, eid)| *en == n && eid == id);
                // (shift on another text adds it to the picked things)
                if let Some(id) = text.filter(|id| !picked(id) && (!shift || same(id))) {
                    let c = self.char_near_point(ui.ctx(), b, &pv, &id, at);
                    self.click_text(b, n, &id, c, 1, shift);
                    b.drag = Some(Drag::Select);
                    return;
                }
            }
        }
        b.commit_edit();
        if self.tool == Tool::Select {
            if let Some((n, id, h)) = self.handle_at(b, views, q) {
                let Some(orig) = b.page(n).and_then(|p| p.item(&id)).cloned() else { return };
                b.checkpoint();
                match h {
                    Some(handle) => b.drag = Some(Drag::Resize { page: n, id, handle, orig: Box::new(orig) }),
                    None => {
                        let pv = views.iter().find(|v| v.n == n).unwrap();
                        let c = pv.s((orig.x + orig.w / 2.0, orig.y + orig.h / 2.0));
                        let a0 = ((q.y - c.y) as f64).atan2((q.x - c.x) as f64).to_degrees();
                        b.drag = Some(Drag::Rotate { page: n, id, orig: orig.r, a0 });
                    }
                }
                return;
            }
        }
        let Some((pv, at)) = Self::at(views, q) else { return };
        let n = pv.n;
        match self.tool {
            Tool::Select | Tool::Write => {
                // a left page's item that lies across, grabbed on the right
                let mut hit = self.hit_at(b, n, at).map(|id| (n, id));
                if hit.is_none() && pv.lay.right && n > 1 {
                    let left_at = (at.0 + pv.lay.w, at.1);
                    if let Some(id) = self.hit_at(b, n - 1, left_at) {
                        if b.page(n - 1).and_then(|p| p.item(&id)).is_some_and(|i| i.across) && views.iter().any(|v| v.n == n - 1) {
                            hit = Some((n - 1, id));
                        }
                    }
                }
                match hit {
                    Some((hn, id)) => {
                        if b.sel_page != hn || !b.sel.contains(&id) {
                            if !shift || b.sel_page != hn {
                                b.sel.clear();
                            }
                            b.sel_page = hn;
                            b.sel.push(id.clone());
                        }
                        if b.page(hn).and_then(|p| p.item(&id)).is_some_and(|i| i.locked) {
                            return;
                        }
                        let ids = b.sel.clone();
                        let orig: Vec<(String, f64, f64)> = b.page(hn).map(|p| p.items.iter().filter(|i| ids.contains(&i.id) || i.anchor.as_ref().is_some_and(|a| ids.contains(a))).map(|i| (i.id.clone(), i.x, i.y)).collect()).unwrap_or_default();
                        let start = if hn != n { (at.0 + pv.lay.w, at.1) } else { at };
                        b.checkpoint();
                        b.drag = Some(Drag::Move { page: hn, start, orig, moved: false });
                    }
                    None => {
                        if !shift {
                            b.sel.clear();
                        }
                        b.sel_page = n;
                        b.drag = Some(Drag::Marquee { page: n, start: at, now: at });
                    }
                }
            }
            Tool::Pen => {
                b.checkpoint();
                let w = 0.45;
                let reuse = b.last_ink.as_ref().filter(|(pn, _, t)| *pn == n && t.elapsed() < Duration::from_millis(1400)).map(|(_, id, _)| id.clone());
                let lay_w = pv.lay.w;
                let lay_h = pv.lay.h;
                let color = draw::hex_of(self.color);
                let id = match reuse.and_then(|id| b.page(n).and_then(|p| p.item_mut(&id)).map(|it| (id, it))) {
                    Some((id, it)) => {
                        // back to page coordinates while drawing
                        for s in it.strokes.iter_mut() {
                            for q in s.pts.iter_mut() {
                                q[0] = (it.x + q[0] * it.w) / lay_w;
                                q[1] = (it.y + q[1] * it.h) / lay_h;
                            }
                        }
                        it.x = 0.0;
                        it.y = 0.0;
                        it.w = lay_w;
                        it.h = lay_h;
                        it.r = 0.0;
                        it.strokes.push(journal::Stroke { pts: vec![[at.0 / lay_w, at.1 / lay_h, w]], color: Some(color) });
                        b.uncheckpoint();
                        id
                    }
                    None => {
                        let mut it = Item::new(Kind::Ink, 0.0, 0.0, lay_w, lay_h);
                        it.strokes.push(journal::Stroke { pts: vec![[at.0 / lay_w, at.1 / lay_h, w]], color: Some(color) });
                        let id = it.id.clone();
                        if let Some(p) = b.page(n) {
                            p.items.push(it);
                        }
                        id
                    }
                };
                b.drag = Some(Drag::Ink { page: n, id, last: at, at: Instant::now() });
                b.touch(n);
            }
            Tool::Line | Tool::Tape => {
                b.checkpoint();
                let kind = if self.tool == Tool::Line { Kind::Line } else { Kind::Tape };
                let mut it = Item::new(kind, at.0, at.1, 0.1, 0.1);
                if self.tool == Tool::Line {
                    it.color = Some(draw::hex_of(self.color));
                    it.pts = vec![[0.0, 0.0], [1.0, 1.0]];
                } else {
                    it.h = 7.0;
                }
                let id = it.id.clone();
                if let Some(p) = b.page(n) {
                    p.items.push(it);
                }
                b.drag = Some(Drag::Shape { page: n, id, start: at });
            }
        }
        let _ = ui;
    }

    #[allow(clippy::too_many_arguments)]
    fn drag_to(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView], q: Pos2, d: Drag, shift: bool, _cmd: bool, thr: impl Fn(&PageView) -> f64) {
        match d {
            Drag::Pan => {
                b.pan += ui.input(|i| i.pointer.delta());
            }
            Drag::Move { page, start, orig, .. } => {
                let Some(pv) = views.iter().find(|v| v.n == page) else { return };
                let at = pv.w(q);
                let (mut dx, mut dy) = (at.0 - start.0, at.1 - start.1);
                // snap the moving group to the page's lines and the other items
                let ids: Vec<String> = orig.iter().map(|o| o.0.clone()).collect();
                let lay = pv.lay.clone();
                let th = thr(pv);
                b.guides.clear();
                let mut guides = Vec::new();
                if let Some(pg) = b.page(page) {
                    let moving: Vec<&Item> = pg.items.iter().filter(|i| ids.contains(&i.id)).collect();
                    let bb0 = moving.iter().map(|i| {
                        let o = orig.iter().find(|o| o.0 == i.id).unwrap();
                        let mut c = (*i).clone();
                        c.x = o.1;
                        c.y = o.2;
                        c.bbox()
                    }).reduce(union);
                    if let Some(bb0) = bb0 {
                        let others: Vec<MRect> = pg.items.iter().filter(|i| !ids.contains(&i.id)).map(|i| i.bbox()).collect();
                        let (xs, ys) = lay.snap_lines();
                        if !ui.input(|i| i.modifiers.alt) {
                            let (sx, sy, g) = canvas::snap_lines(bb0, dx, dy, &others, &xs, &ys, th);
                            dx = sx;
                            dy = sy;
                            guides = g.into_iter().map(|g| (page, g)).collect();
                        }
                    }
                    for (id, x0, y0) in &orig {
                        if let Some(it) = pg.item_mut(id) {
                            it.x = x0 + dx;
                            it.y = y0 + dy;
                            // put here by hand: here is where it goes back to
                            it.home = None;
                        }
                    }
                }
                b.guides = guides;
                if let Some(Drag::Move { moved, .. }) = b.drag.as_mut() {
                    *moved = true;
                }
                b.touch(page);
            }
            Drag::Resize { page, id, handle, orig } => {
                let Some(pv) = views.iter().find(|v| v.n == page) else { return };
                let at = pv.w(q);
                let (x, y, w, h) = resize(&orig, handle, at, shift);
                if let Some(it) = b.page(page).and_then(|p| p.item_mut(&id)) {
                    it.x = x;
                    it.y = y;
                    it.w = w;
                    if !orig.kind.is_text() {
                        it.h = h;
                    }
                }
                b.touch(page);
            }
            Drag::Rotate { page, id, orig, a0 } => {
                let Some(pv) = views.iter().find(|v| v.n == page) else { return };
                let Some(it) = b.page(page).and_then(|p| p.item(&id)).cloned() else { return };
                let c = pv.s((it.x + it.w / 2.0, it.y + it.h / 2.0));
                let a = ((q.y - c.y) as f64).atan2((q.x - c.x) as f64).to_degrees();
                let mut r = orig + a - a0;
                r = ((r + 180.0).rem_euclid(360.0)) - 180.0;
                if shift {
                    r = (r / 15.0).round() * 15.0;
                } else if r.abs() < 2.0 {
                    r = 0.0;
                }
                if let Some(it) = b.page(page).and_then(|p| p.item_mut(&id)) {
                    it.r = r;
                }
                b.touch(page);
            }
            Drag::Ink { page, id, last, at: t0 } => {
                let Some(pv) = views.iter().find(|v| v.n == page) else { return };
                let at = pv.w(q);
                let dist = ((at.0 - last.0).powi(2) + (at.1 - last.1).powi(2)).sqrt();
                if dist < 0.25 {
                    return;
                }
                let dt = t0.elapsed().as_secs_f64().max(1e-3);
                let speed = dist / dt;
                // faster is thinner, as a pen on paper
                let w = (0.55 - (speed / 400.0).min(0.3)).max(0.22);
                let (lw, lh) = (pv.lay.w, pv.lay.h);
                if let Some(it) = b.page(page).and_then(|p| p.item_mut(&id)) {
                    if let Some(s) = it.strokes.last_mut() {
                        let prev_w = s.pts.last().map(|q| q[2]).unwrap_or(w);
                        s.pts.push([at.0 / lw, at.1 / lh, prev_w * 0.6 + w * 0.4]);
                    }
                }
                b.drag = Some(Drag::Ink { page, id, last: at, at: Instant::now() });
                b.touch(page);
            }
            Drag::Shape { page, id, start } => {
                let Some(pv) = views.iter().find(|v| v.n == page) else { return };
                let mut at = pv.w(q);
                if shift {
                    // straight across or down, or at 45°
                    let (dx, dy) = (at.0 - start.0, at.1 - start.1);
                    let a = (dy.atan2(dx).to_degrees() / 45.0).round() * 45.0;
                    let len = (dx * dx + dy * dy).sqrt();
                    at = (start.0 + len * a.to_radians().cos(), start.1 + len * a.to_radians().sin());
                }
                if let Some(it) = b.page(page).and_then(|p| p.item_mut(&id)) {
                    match it.kind {
                        Kind::Line => {
                            let (x0, y0) = (start.0.min(at.0), start.1.min(at.1));
                            let (w, h) = ((at.0 - start.0).abs().max(0.1), (at.1 - start.1).abs().max(0.1));
                            it.x = x0;
                            it.y = y0;
                            it.w = w;
                            it.h = h;
                            let u = |v: f64, o: f64, l: f64| ((v - o) / l).clamp(0.0, 1.0);
                            it.pts = vec![[u(start.0, x0, w), u(start.1, y0, h)], [u(at.0, x0, w), u(at.1, y0, h)]];
                        }
                        _ => {
                            let (dx, dy) = (at.0 - start.0, at.1 - start.1);
                            let len = (dx * dx + dy * dy).sqrt().max(4.0);
                            let (mx, my) = ((start.0 + at.0) / 2.0, (start.1 + at.1) / 2.0);
                            it.w = len;
                            it.x = mx - len / 2.0;
                            it.y = my - it.h / 2.0;
                            it.r = dy.atan2(dx).to_degrees();
                        }
                    }
                }
                b.touch(page);
            }
            Drag::Marquee { page, start, .. } => {
                let Some(pv) = views.iter().find(|v| v.n == page) else { return };
                let now = pv.w(q);
                b.drag = Some(Drag::Marquee { page, start, now });
                let r = MRect { x: start.0.min(now.0), y: start.1.min(now.1), w: (now.0 - start.0).abs(), h: (now.1 - start.1).abs() };
                let ids: Vec<String> = b.page(page).map(|p| p.items.iter().filter(|i| r.contains_rect(&i.bbox()) || (r.w > 2.0 && r.intersects(&i.bbox()) && r.contains_rect(&centre_box(i)))).map(|i| i.id.clone()).collect()).unwrap_or_default();
                b.sel_page = page;
                b.sel = ids;
            }
            Drag::Select => self.sweep_to(ui.ctx(), b, views, q),
            Drag::Crop { .. } => {}
        }
    }

    fn release(&mut self, ctx: &egui::Context, b: &mut Book, views: &[PageView], shift: bool) {
        b.guides.clear();
        let Some(d) = b.drag.take() else { return };
        match d {
            // a press held still on the paper (egui makes it a drag after a
            // while): a click
            Drag::Marquee { page, start, now } if (now.0 - start.0).hypot(now.1 - start.1) < 1.0 => {
                if let Some(q) = views.iter().find(|v| v.n == page).map(|pv| pv.s(start)) {
                    self.click(ctx, b, views, q, shift, 1);
                }
            }
            Drag::Move { page, moved, .. } => {
                if !moved {
                    b.uncheckpoint();
                    return;
                }
                self.settle(b, views, page);
            }
            Drag::Resize { page, .. } => {
                b.unsettled.insert(page);
                b.touch(page);
            }
            Drag::Ink { page, id, .. } => {
                // the box round the strokes; points within it (0..1)
                let Some(pv) = views.iter().find(|v| v.n == page) else { return };
                let (lw, lh) = (pv.lay.w, pv.lay.h);
                if let Some(it) = b.page(page).and_then(|p| p.item_mut(&id)) {
                    let pad = 1.0;
                    let all: Vec<(f64, f64)> = it.strokes.iter().flat_map(|s| s.pts.iter().map(|q| (q[0] * lw, q[1] * lh))).collect();
                    let x0 = all.iter().map(|p| p.0).fold(f64::INFINITY, f64::min) - pad;
                    let y0 = all.iter().map(|p| p.1).fold(f64::INFINITY, f64::min) - pad;
                    let x1 = all.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max) + pad;
                    let y1 = all.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max) + pad;
                    let (w, h) = ((x1 - x0).max(0.5), (y1 - y0).max(0.5));
                    for s in it.strokes.iter_mut() {
                        for q in s.pts.iter_mut() {
                            q[0] = (q[0] * lw - x0) / w;
                            q[1] = (q[1] * lh - y0) / h;
                        }
                    }
                    it.x = x0;
                    it.y = y0;
                    it.w = w;
                    it.h = h;
                }
                b.last_ink = Some((page, id, Instant::now()));
                b.touch(page);
            }
            Drag::Shape { page, id, .. } => {
                let tiny = b.page(page).and_then(|p| p.item(&id)).is_some_and(|i| i.w < 1.0 && i.h < 1.0);
                if tiny {
                    if let Some(p) = b.page(page) {
                        p.items.retain(|i| i.id != id);
                    }
                    b.uncheckpoint();
                } else {
                    b.sel_page = page;
                    b.sel = vec![id];
                    b.touch(page);
                }
            }
            _ => {}
        }
    }

    /// After a move: an item carried over the spine goes to the other page,
    /// one left over both pages lies across them.
    fn settle(&mut self, b: &mut Book, views: &[PageView], page: usize) {
        let Some(pv) = views.iter().find(|v| v.n == page) else { return };
        let other = views.iter().find(|v| v.n != page).map(|v| v.n);
        let w = pv.lay.w;
        let ids = b.sel.clone();
        let mut carried: Vec<Item> = Vec::new();
        if let Some(pg) = b.page(page) {
            let anchored: Vec<String> = pg.items.iter().filter(|i| i.anchor.as_ref().is_some_and(|a| ids.contains(a))).map(|i| i.id.clone()).collect();
            for it in pg.items.iter_mut().filter(|i| ids.contains(&i.id) || anchored.contains(&i.id)) {
                let bb = it.bbox();
                let c = bb.x + bb.w / 2.0;
                if pv.lay.right {
                    it.across = false;
                    if c < 0.0 && other.is_some() {
                        carried.push(it.clone());
                    } else if bb.x < -2.0 && other.is_some() {
                        // over the spine from the right: kept on the left, across
                        carried.push(it.clone());
                    }
                } else {
                    let over = bb.x + bb.w > w + 2.0;
                    if c > w && other.is_some() {
                        carried.push(it.clone());
                    } else {
                        it.across = over && other.is_some();
                    }
                }
            }
            let gone: Vec<String> = carried.iter().map(|i| i.id.clone()).collect();
            pg.items.retain(|i| !gone.contains(&i.id));
        }
        if carried.is_empty() || other.is_none() {
            b.touch(page);
            return;
        }
        let to = other.unwrap();
        for mut it in carried {
            if pv.lay.right {
                it.x += w;
                let bb = it.bbox();
                it.across = bb.x + bb.w > w + 2.0;
            } else {
                it.x -= w;
                it.across = false;
            }
            if let Some(p) = b.page(to) {
                p.items.push(it);
            }
        }
        b.sel_page = to;
        b.touch(page);
        b.touch(to);
    }

    /// A click: on letters, the cursor goes there (no picking first; two
    /// clicks pick a word, three a line); on a picture or anything else, it
    /// is picked; on the paper, writing starts right there.
    fn click(&mut self, ctx: &egui::Context, b: &mut Book, views: &[PageView], q: Pos2, shift: bool, count: u8) {
        let Some((pv, at)) = Self::at(views, q) else {
            b.commit_edit();
            b.sel.clear();
            return;
        };
        let pv = pv.clone();
        let n = pv.n;
        if matches!(self.tool, Tool::Pen | Tool::Line | Tool::Tape) {
            return;
        }
        if let Some(id) = self.hit_at(b, n, at) {
            let kind = b.page(n).and_then(|p| p.item(&id)).map(|i| i.kind.clone());
            let typing_it = b.editing.as_ref().is_some_and(|e| e.page == n && e.id == id);
            if kind.as_ref().is_some_and(|k| k.is_text()) && (!shift || typing_it) {
                let c = self.char_near_point(ctx, b, &pv, &id, at);
                self.click_text(b, n, &id, c, count, shift);
                return;
            }
            b.commit_edit();
            if !shift {
                b.sel = vec![id];
            } else if let Some(i) = b.sel.iter().position(|s| *s == id) {
                b.sel.remove(i);
            } else {
                b.sel.push(id);
            }
            b.sel_page = n;
            return;
        }
        // the typing is done (a text left empty goes away, out of the way)
        b.commit_edit();
        b.sel.clear();
        // the paper takes a new text right where it is clicked; only where
        // one has no room (too near the next letters), the click goes into
        // the nearest row on that line
        if !self.write_at(b, &pv, at) {
            if let Some((id, c, _)) = self.row_edge_near(b, n, at) {
                self.click_text(b, n, &id, Some(c), count.min(1), shift);
            }
        }
    }

    /// What is under a point of page `n`, topmost first: a text where its
    /// letters are (a little round them), anything else in its box.
    pub fn hit_at(&self, b: &mut Book, n: usize, at: (f64, f64)) -> Option<String> {
        let shown = self.shown.get(&n);
        let page = b.page(n)?;
        page.items.iter().rev().find(|it| match (it.kind.is_text(), shown.and_then(|s| s.set.get(&it.id))) {
            (true, Some(set)) => typeset::on_letters(set, at),
            _ => hit_item(it, at),
        }).map(|it| it.id.clone())
    }

    fn draw_selection(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView], p: crate::theme::Palette) {
        let Some(pv) = views.iter().find(|v| v.n == b.sel_page) else { return };
        let painter = ui.painter_at(pv.clip_across.union(pv.clip));
        let items = b.sel_items();
        let editing = b.editing.as_ref().map(|e| e.id.clone());
        for it in &items {
            // the text being typed has no frame: it is writing on the page
            if editing.as_deref() == Some(it.id.as_str()) {
                continue;
            }
            let c: Vec<Pos2> = it.corners().iter().map(|q| pv.s(*q)).collect();
            // (a picked text is washed under its letters, as it is drawn)
            if !it.kind.is_text() {
                painter.add(egui::Shape::closed_line(c, Stroke::new(1.5_f32, p.accent_strong)));
            }
        }
        if items.len() == 1 && b.editing.is_none() && b.crop.is_none() {
            let it = &items[0];
            if !it.locked {
                let (hs, knob) = handles(it, pv);
                let top = pv.s(canvas::rot_pt(it.x + it.w / 2.0, it.y, it.x + it.w / 2.0, it.y + it.h / 2.0, it.r));
                painter.line_segment([top, knob], Stroke::new(1.0_f32, p.accent_strong));
                painter.circle(knob, 5.0, p.bg, Stroke::new(1.5_f32, p.accent_strong));
                for (_, at) in hs {
                    painter.rect(Rect::from_center_size(at, vec2(8.0, 8.0)), 1.5, p.bg, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Middle);
                }
            } else {
                let c = pv.s((it.x + it.w, it.y));
                painter.text(c + vec2(4.0, -4.0), egui::Align2::LEFT_BOTTOM, crate::icons::LOCK, crate::kit::icon_font(12.0), p.faint);
            }
        }
        if let Some(Drag::Marquee { page, start, now }) = &b.drag {
            if let Some(pv) = views.iter().find(|v| v.n == *page) {
                let r = Rect::from_two_pos(pv.s(*start), pv.s(*now));
                ui.painter().rect(r, 0.0, p.accent_soft.gamma_multiply(0.35), Stroke::new(1.0_f32, p.accent_strong), egui::StrokeKind::Inside);
            }
        }
        for (n, g) in &b.guides {
            let Some(pv) = views.iter().find(|v| v.n == *n) else { continue };
            let stroke = Stroke::new(1.0_f32, Color32::from_rgb(0xe0, 0x4f, 0x8a));
            if g.v {
                let x = pv.s((g.at, 0.0)).x;
                let (y0, y1) = if g.from.is_finite() { (pv.s((0.0, g.from)).y, pv.s((0.0, g.to)).y) } else { (pv.rect.top(), pv.rect.bottom()) };
                ui.painter().vline(x, y0..=y1, stroke);
            } else {
                let y = pv.s((0.0, g.at)).y;
                let (x0, x1) = if g.from.is_finite() { (pv.s((g.from, 0.0)).x, pv.s((g.to, 0.0)).x) } else { (pv.rect.left(), pv.rect.right()) };
                ui.painter().hline(x0..=x1, y, stroke);
            }
        }
    }

    fn keys(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView]) {
        use egui::Key;
        let m = ui.input(|i| i.modifiers);
        let press = |k: Key| ui.input(|i| i.key_pressed(k));
        // in the immersive mode, typing writes: on the next free lines
        if self.immersive && b.sel.is_empty() && !m.command {
            let typed: String = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
            if !typed.is_empty() {
                if let Some(pv) = views.last() {
                    self.write_free(b, pv, &typed);
                }
                return;
            }
        }
        if cmd_shift_enter(ui) {
            self.immersive = !self.immersive;
            return;
        }
        let cmd = m.command;
        if cmd && press(Key::Z) {
            if m.shift { b.redo() } else { b.undo() }
            return;
        }
        if cmd && press(Key::Y) {
            b.redo();
            return;
        }
        if cmd && press(Key::Enter) {
            b.new_page_here(&mut self.out);
            return;
        }
        if cmd && press(Key::D) {
            b.duplicate_selection();
            return;
        }
        if cmd && press(Key::G) {
            b.goto = Some(String::new());
            return;
        }
        if cmd && press(Key::Num0) {
            b.zoom = 1.0;
            b.pan = egui::Vec2::ZERO;
            return;
        }
        if cmd && press(Key::CloseBracket) {
            b.layer(true, m.shift);
            return;
        }
        if cmd && press(Key::OpenBracket) {
            b.layer(false, m.shift);
            return;
        }
        if press(Key::Delete) || press(Key::Backspace) {
            b.delete_selection();
            return;
        }
        if press(Key::Escape) {
            if b.crop.is_some() {
                b.crop = None;
            } else if !b.sel.is_empty() {
                b.sel.clear();
            } else if b.zoom > 1.0 {
                b.zoom = 1.0;
                b.pan = egui::Vec2::ZERO;
            } else if self.tool != Tool::Select {
                self.tool = Tool::Select;
            } else if self.immersive {
                self.immersive = false;
            }
            return;
        }
        if press(Key::Enter) {
            if b.crop.is_some() {
                self.finish_crop(b);
                return;
            }
            if b.sel.len() == 1 {
                let (n, id) = (b.sel_page, b.sel[0].clone());
                self.edit(b, n, &id, None);
                return;
            }
            // nothing picked: write on the next free lines of the page
            if b.sel.is_empty() {
                if let Some(pv) = views.last() {
                    self.write_free(b, pv, "");
                }
                return;
            }
        }
        if press(Key::PageDown) {
            b.turn(true);
            return;
        }
        if press(Key::PageUp) {
            b.turn(false);
            return;
        }
        if b.sel.is_empty() {
            if press(Key::ArrowRight) {
                b.turn(true);
            }
            if press(Key::ArrowLeft) {
                b.turn(false);
            }
        } else {
            let step = if m.shift { 5.0 } else { 0.5 };
            let (dx, dy) = if press(Key::ArrowRight) { (step, 0.0) } else if press(Key::ArrowLeft) { (-step, 0.0) } else if press(Key::ArrowUp) { (0.0, -step) } else if press(Key::ArrowDown) { (0.0, step) } else { (0.0, 0.0) };
            if dx != 0.0 || dy != 0.0 {
                b.checkpoint();
                let ids = b.sel.clone();
                let n = b.sel_page;
                if let Some(p) = b.page(n) {
                    for it in p.items.iter_mut().filter(|i| (ids.contains(&i.id) || i.anchor.as_ref().is_some_and(|a| ids.contains(a))) && !i.locked) {
                        it.x += dx;
                        it.y += dy;
                        it.home = None;
                    }
                }
                b.touch(n);
            }
        }
        if m.any() {
            return;
        }
        if press(Key::V) {
            self.tool = Tool::Select;
        } else if press(Key::T) {
            self.tool = Tool::Write;
        } else if press(Key::P) {
            self.tool = Tool::Pen;
        } else if press(Key::L) {
            self.tool = Tool::Line;
        } else if press(Key::F) {
            self.tool = Tool::Tape;
        } else if press(Key::I) {
            self.toggle_panel(super::Panel::Pictures);
        } else if press(Key::N) {
            self.toggle_panel(super::Panel::Notes);
        } else if press(Key::E) {
            self.toggle_panel(super::Panel::Snippets);
        } else if press(Key::C) && b.sel.len() == 1 {
            let (n, id) = (b.sel_page, b.sel[0].clone());
            self.start_crop(b, n, &id);
        }
    }

    pub fn toggle_panel(&mut self, p: super::Panel) {
        self.panel = if self.panel == Some(p) { None } else { Some(p) };
    }

    // ── cropping ──────────────────────────────────────────────────────────

    pub fn start_crop(&mut self, b: &mut Book, n: usize, id: &str) {
        let Some(it) = b.page(n).and_then(|p| p.item(id)).cloned() else { return };
        if it.kind != Kind::Image {
            return;
        }
        b.commit_edit();
        b.crop = Some((n, id.to_string(), mb::CropStage::start(&mb_item(&it))));
        b.sel_page = n;
        b.sel = vec![id.to_string()];
    }

    pub fn finish_crop(&mut self, b: &mut Book) {
        let Some((n, id, stage)) = b.crop.take() else { return };
        let Some(it) = b.page(n).and_then(|p| p.item(&id)).cloned() else { return };
        let mut m = mb_item(&it);
        if stage.apply(&mut m) {
            b.checkpoint();
            if let Some(slot) = b.page(n).and_then(|p| p.item_mut(&id)) {
                slot.crop = m.crop;
                slot.x = m.x;
                slot.y = m.y;
                slot.w = m.w;
                slot.h = m.h;
            }
            b.touch(n);
        }
    }

    fn crop_pointer(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView], resp: &egui::Response) {
        let Some((n, _, stage)) = b.crop.as_ref() else { return };
        let Some(pv) = views.iter().find(|v| v.n == *n).cloned() else { return };
        let Some(q) = resp.interact_pointer_pos() else { return };
        let local = stage.local(pv.w(q));
        if resp.drag_started() {
            let r = stage.rect;
            let th = 8.0 / pv.k as f64;
            let near = |a: f64, v: f64| (a - v).abs() < th;
            let mut k = String::new();
            if near(local.1, r.y) {
                k.push('n');
            } else if near(local.1, r.y + r.h) {
                k.push('s');
            }
            if near(local.0, r.x) {
                k.push('w');
            } else if near(local.0, r.x + r.w) {
                k.push('e');
            }
            let inside = local.0 > r.x && local.0 < r.x + r.w && local.1 > r.y && local.1 < r.y + r.h;
            if k.is_empty() && inside {
                k = "move".into();
            }
            if k.is_empty() {
                // a press outside the window ends cropping
                self.finish_crop(b);
                return;
            }
            b.drag = Some(Drag::Crop { k, r0: r, start: local });
        }
        if let (true, Some(Drag::Crop { k, r0, start, .. })) = (resp.dragged(), b.drag.clone()) {
            if let Some((_, _, st)) = b.crop.as_mut() {
                st.drag(&k, r0, (local.0 - start.0, local.1 - start.1), ui.input(|i| i.modifiers.shift), 3.0);
            }
        }
        if resp.drag_stopped() {
            b.drag = None;
        }
    }

    /// The picture being cropped: all of it, dimmed, and the window over it.
    fn crop_overlay(&mut self, ui: &egui::Ui, b: &mut Book, cx: &mut Cx, views: &[PageView]) {
        let Some((n, id, stage)) = b.crop.clone() else { return };
        let Some(pv) = views.iter().find(|v| v.n == n) else { return };
        let Some(it) = b.page(n).and_then(|p| p.item(&id)).cloned() else { return };
        let path = b.page_ref(n).map(|r| r.path.clone()).unwrap_or_default();
        let Some(target) = it.picture() else { return };
        let ctx = ui.ctx().clone();
        let Some((t, _)) = self.picture(&ctx, cx, &target, &path, it.look.filter.as_deref()) else { return };
        let painter = ui.painter();
        let quad = |x0: f64, y0: f64, x1: f64, y1: f64| -> Vec<Pos2> { [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|q| pv.s(stage.world(*q))).collect() };
        let full = quad(0.0, 0.0, stage.fw, stage.fh);
        let (u0, u1) = if it.flip { (1.0, 0.0) } else { (0.0, 1.0) };
        let mesh = |pts: &[Pos2], uv: [(f32, f32); 4], tint: Color32| {
            let mut m = egui::Mesh::with_texture(t);
            for (i, q) in pts.iter().enumerate() {
                m.vertices.push(egui::epaint::Vertex { pos: *q, uv: pos2(uv[i].0, uv[i].1), color: tint });
            }
            m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
            m
        };
        painter.add(mesh(&full, [(u0, 0.0), (u1, 0.0), (u1, 1.0), (u0, 1.0)], Color32::from_white_alpha(110)));
        let r = stage.rect;
        let win = quad(r.x, r.y, r.x + r.w, r.y + r.h);
        let fu = |x: f64| { let u = x / stage.fw; (if it.flip { 1.0 - u } else { u }) as f32 };
        let fv = |y: f64| (y / stage.fh) as f32;
        painter.add(mesh(&win, [(fu(r.x), fv(r.y)), (fu(r.x + r.w), fv(r.y)), (fu(r.x + r.w), fv(r.y + r.h)), (fu(r.x), fv(r.y + r.h))], Color32::WHITE));
        let p = cx.palette;
        painter.add(egui::Shape::closed_line(win.clone(), Stroke::new(1.5_f32, p.accent_strong)));
        for q in &win {
            painter.rect(Rect::from_center_size(*q, vec2(9.0, 9.0)), 1.5, p.bg, Stroke::new(1.5_f32, p.accent_strong), egui::StrokeKind::Middle);
        }
    }

    // ── what is dropped on a page ─────────────────────────────────────────

    fn drops(&mut self, ui: &egui::Ui, b: &mut Book, cx: &mut Cx, views: &[PageView]) {
        let ctx = ui.ctx().clone();
        if !egui::DragAndDrop::has_payload_of_type::<Payload>(&ctx) {
            return;
        }
        let Some(q) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let Some((pv, at)) = Self::at(views, q) else { return };
        ui.painter().rect_stroke(pv.rect.shrink(2.0), 2.0, Stroke::new(2.0_f32, cx.palette.accent_strong), egui::StrokeKind::Inside);
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        let Some(payload) = egui::DragAndDrop::take_payload::<Payload>(&ctx) else { return };
        let alt = ctx.input(|i| i.modifiers.alt);
        let pv = pv.clone();
        self.place(&ctx, b, cx, &pv, at, (*payload).clone(), alt);
        // the material steps aside once something is on the page
        self.panel = None;
    }

    /// Put something on page `pv.n` at `at`.
    #[allow(clippy::too_many_arguments)]
    pub fn place(&mut self, ctx: &egui::Context, b: &mut Book, cx: &mut Cx, pv: &PageView, at: (f64, f64), what: Payload, alt: bool) {
        let n = pv.n;
        let lay = pv.lay.clone();
        match what {
            Payload::Picture(key) => {
                let rel = copy_into_vault(cx.vault, &cx.attach, &key);
                if rel != key {
                    self.out.push(super::Out::Rescan);
                }
                let path = b.page_ref(n).map(|r| r.path.clone()).unwrap_or_default();
                let size = crate::host::image(ctx, cx.vault, cx.caches, &rel, &path).map(|(_, s)| s);
                let aspect = size.map(|s| (s.y / s.x.max(1.0)) as f64).unwrap_or(0.75);
                let (c0, c1) = lay.main_column();
                let mut w = ((c1 - c0) * 0.6).max(20.0);
                let mut h = w * aspect;
                if h > lay.h * 0.6 {
                    h = lay.h * 0.6;
                    w = h / aspect;
                }
                let mut it = Item::new(Kind::Image, at.0 - w / 2.0, at.1 - h / 2.0, w, h);
                it.set_picture(&rel, "");
                // from the sticker drawer: cut already, with its edge
                if rel.starts_with(&format!("{}/", super::chrome::STICKERS)) {
                    it.kind = Kind::Sticker;
                    it.mask = cx.config_dir.as_ref().and_then(|c| super::chrome::sticker_mask(c, &rel)).filter(|m| m.shape != "polaroide");
                    it.look.border = 1.8;
                    it.w = it.w.min(50.0);
                    it.h = it.w * aspect;
                }
                b.checkpoint();
                let id = it.id.clone();
                if let Some(p) = b.page(n) {
                    p.items.push(it);
                }
                b.sel_page = n;
                b.sel = vec![id];
                b.touch(n);
            }
            Payload::Note(path) => {
                b.checkpoint();
                let link = path.trim_end_matches(".md").to_string();
                if alt {
                    // a copy of its text, on the ruling
                    let text = std::fs::read_to_string(cx.vault.abs(&path)).unwrap_or_default();
                    let body = ateliet_core::scriptorium::strip_frontmatter(&text).trim().to_string();
                    let (c0, c1) = lay.column_at(at.0);
                    let mut it = Item::new(Kind::Text, c0, lay.text_top(at.1, 1.0), c1 - c0, lay.paper.line());
                    it.set_content(&body);
                    let id = it.id.clone();
                    if let Some(p) = b.page(n) {
                        p.items.push(it);
                    }
                    b.unsettled.insert(n);
                    b.sel_page = n;
                    b.sel = vec![id];
                } else {
                    let (w, h) = (60.0_f64.min(lay.w - 12.0), 45.0);
                    let mut it = Item::new(Kind::Note, at.0 - w / 2.0, at.1 - h / 2.0, w, h);
                    it.set_content(&format!("![[{link}]]"));
                    let id = it.id.clone();
                    if let Some(p) = b.page(n) {
                        p.items.push(it);
                    }
                    b.sel_page = n;
                    b.sel = vec![id];
                }
                b.touch(n);
            }
            Payload::Snippet(path) => {
                let Some(s) = self.snippet(cx.vault, &path) else { return };
                self.place_snippet(b, n, &lay, Some(at), &path, &s);
            }
            Payload::Board(board) => {
                // the whole storyboard, playing; cut it from the balloon
                let last = ateliet_core::journal::storyboard::clip(&cx.vault.abs(&board)).map(|c| c.len().saturating_sub(1)).unwrap_or(0);
                self.place_board(b, cx, &board, 0, last, true, super::boards::Target::New(n, at));
            }
        }
    }

    /// An excerpt on a page: in the column under the point (or the main
    /// column's first free line), its note linked from the page.
    pub fn place_snippet(&mut self, b: &mut Book, n: usize, lay: &journal::Layout, at: Option<(f64, f64)>, note: &str, s: &ateliet_core::capture::Snippet) {
        b.checkpoint();
        let (c0, c1) = match at {
            Some(a) => lay.column_at(a.0),
            None => lay.main_column(),
        };
        let y = match at {
            Some(a) => lay.text_top(a.1 - lay.paper.line() * 0.5, 1.0),
            None => free_top(b, n, lay, (c0, c1), 3.0),
        };
        let id;
        if let Some(img) = s.image.as_ref().filter(|_| s.text.trim().is_empty()) {
            let w = (c1 - c0) * 0.7;
            let mut it = Item::new(Kind::Image, c0, y, w, w * 0.75);
            it.set_picture(img, &s.source.line());
            id = it.id.clone();
            if let Some(p) = b.page(n) {
                p.items.push(it);
            }
        } else {
            let mut it = Item::new(Kind::Excerpt, c0, y, c1 - c0, lay.paper.line());
            it.set_content(&s.section());
            id = it.id.clone();
            if let Some(p) = b.page(n) {
                p.items.push(it);
            }
            b.unsettled.insert(n);
        }
        if let Some(p) = b.page(n) {
            p.add_source(note.trim_end_matches(".md"));
        }
        b.sel_page = n;
        b.sel = vec![id];
        b.touch(n);
    }
}

/// Where writing goes on in a column: the first free lines (`rows` of
/// them) after what is already in it, else the first free ones anywhere.
pub(super) fn free_top(b: &mut Book, n: usize, lay: &journal::Layout, (c0, c1): (f64, f64), rows: f64) -> f64 {
    let s = lay.paper.line();
    let boxes: Vec<MRect> = b.page(n).map(|p| p.items.iter().map(|i| i.bbox()).collect()).unwrap_or_default();
    let low = boxes.iter().filter(|o| o.x < c1 && o.x + o.w > c0).map(|o| o.y + o.h).fold(f64::NEG_INFINITY, f64::max);
    let free = |top: &f64| {
        let want = MRect { x: c0, y: *top, w: c1 - c0, h: s * rows };
        !boxes.iter().any(|o| o.intersects(&want))
    };
    let tops: Vec<f64> = lay.baselines.iter().map(|b| b - s).collect();
    tops.iter().copied().filter(|t| *t >= low).find(free).or_else(|| tops.iter().copied().find(free)).unwrap_or(lay.first_baseline() - s)
}

/// Text written on a cover: light on a dark cover, dark on a light one.
pub(super) fn cover_ink(cover: &str) -> String {
    let c = draw::hex(cover).unwrap_or(Color32::BLACK);
    if crate::theme::contrast(c, Color32::WHITE) > 3.0 { "#f6f3ec".into() } else { "#222224".into() }
}

/// Ctrl/Cmd+Shift+Enter: the immersive mode.
fn cmd_shift_enter(ui: &egui::Ui) -> bool {
    ui.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Enter))
}

fn union(a: MRect, c: MRect) -> MRect {
    let x0 = a.x.min(c.x);
    let y0 = a.y.min(c.y);
    let x1 = (a.x + a.w).max(c.x + c.w);
    let y1 = (a.y + a.h).max(c.y + c.h);
    MRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

fn centre_box(i: &Item) -> MRect {
    let b = i.bbox();
    MRect { x: b.x + b.w / 2.0 - 0.1, y: b.y + b.h / 2.0 - 0.1, w: 0.2, h: 0.2 }
}

/// A picture from outside the vault (the shared library) copied into
/// `folder`; a vault picture keeps its path.
pub fn copy_into_vault(v: &crate::vault::Vault, folder: &str, key: &str) -> String {
    let p = std::path::Path::new(key);
    if !p.is_absolute() {
        return key.to_string();
    }
    if let Ok(rel) = p.strip_prefix(&v.root) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "imagem.png".into());
    let dir = if folder.is_empty() { v.root.clone() } else { v.root.join(folder) };
    let _ = std::fs::create_dir_all(&dir);
    let free = ateliet_core::capture::free_name(&dir, &name);
    let rel = if folder.is_empty() { free.clone() } else { format!("{folder}/{free}") };
    match std::fs::copy(p, v.abs(&rel)) {
        Ok(_) => rel,
        Err(_) => key.to_string(),
    }
}

/// A stock shape for a picture's cut (or none).
pub fn set_mask(it: &mut Item, shape: Option<&str>) {
    it.mask = shape.map(|s| Mask { shape: s.to_string(), pts: Vec::new() });
}
