//! The notebook open at a spread: where the pages are on screen, what the
//! pointer and the keys do on them, the text being typed in place, and the
//! picture being cropped.

use super::draw::{self, PageView};
use super::{Book, Cx, Drag, Editing, Journal, Look, Payload, Tool};
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

    /// The topmost item under a point of page `n`.
    fn hit(&mut self, n: usize, at: (f64, f64)) -> Option<String> {
        let page = self.page(n)?;
        page.items.iter().rev().find(|it| hit_item(it, at)).map(|it| it.id.clone())
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
                self.pending_reflow = Some((pn, pid));
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
        // not while typing: the text in the box is not the one being typed
        if let Some((n, id)) = b.pending_reflow.take_if(|_| b.editing.is_none()) {
            let k = b.views(ui.available_rect_before_wrap()).0.first().map(|v| v.k).unwrap_or(4.0).max(2.0);
            let mut m = |it: &Item, text: &str, lay: &journal::Layout| draw::row_starts(&ctx, it, text, lay, k);
            b.reflow(n, &id, &mut m);
            // typing went past the last line: go on in the next box
            if let Some((typed, before)) = b.continue_edit.take() {
                let next = b.find(&typed).and_then(|(pn, _)| b.page(pn).and_then(|p| p.item(&typed)).and_then(|i| i.flow.as_ref()).and_then(|f| f.next.clone()));
                if let Some((nn, _)) = next.as_ref().and_then(|nx| b.find(nx)) {
                    let nid = next.unwrap();
                    let len = b.page(nn).and_then(|p| p.item(&nid)).map(|i| super::flow_text(i).chars().count()).unwrap_or(0);
                    if !b.views_show(nn) {
                        b.go_to(nn);
                    }
                    self.edit(&mut b, nn, &nid, false);
                    b.edit_cursor = Some(len.saturating_sub(before));
                } else if let Some((pn, _)) = b.find(&typed) {
                    // it fit after all: back to the same box, at its end
                    let len = b.page(pn).and_then(|p| p.item(&typed)).map(|i| super::flow_text(i).chars().count()).unwrap_or(0);
                    self.edit(&mut b, pn, &typed, false);
                    b.edit_cursor = Some(len);
                }
            }
        }
        let canvas = ui.available_rect_before_wrap();
        let p = cx.palette;
        ui.painter().rect_filled(canvas, 0.0, draw::desk_color(p));
        if b.look == Look::Thumbs {
            self.thumbs_ui(ui, &mut b, cx, canvas);
            self.book = Some(b);
            return;
        }
        let resp = ui.interact(canvas, ui.id().with("journal-spread"), egui::Sense::click_and_drag());
        self.zoom_and_swipe(ui, &mut b, canvas, &resp);
        let (views, spread) = b.views(canvas);
        let painter = ui.painter_at(canvas);
        // the shadow under the open notebook, the paper, the spine
        let pages_rect = views.iter().map(|v| v.rect).reduce(|a, c| a.union(c)).unwrap_or(spread);
        painter.add(egui::epaint::RectShape::filled(pages_rect, 2.0, Color32::from_black_alpha(if p.dark { 90 } else { 28 })).with_blur_width(18.0));
        for pv in &views {
            draw::paper(&painter, pv, p);
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
        if let (Some(_), None) = b.spread_pages() {
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
        self.pointer(ui, &mut b, cx, &views, &resp);
        self.drops(ui, &mut b, cx, &views);
        self.draw_selection(ui, &mut b, &views, p);
        self.crop_overlay(ui, &mut b, cx, &views);
        self.edit_overlay(ui, &mut b, &views, p);
        if !ctx.wants_keyboard_input() && b.editing.is_none() {
            self.keys(ui, &mut b);
        }
        self.chrome(ui, &mut b, cx, canvas, &views);
        self.book = Some(b);
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
                Tool::Select => {
                    let over_handle = self.handle_at(b, views, q).is_some();
                    if over_handle {
                        egui::CursorIcon::Grab
                    } else if let Some((pv, at)) = Self::at(views, q) {
                        if b.hit(pv.n, at).is_some() { egui::CursorIcon::Move } else { egui::CursorIcon::Default }
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
            self.release(b, views);
        }
        if resp.clicked() {
            self.click(ui.ctx(), b, views, q, shift);
        }
        if resp.double_clicked() {
            if let Some((pv, at)) = Self::at(views, q) {
                if let Some(id) = b.hit(pv.n, at) {
                    let kind = b.page(pv.n).and_then(|p| p.item(&id)).map(|i| i.kind.clone());
                    match kind {
                        Some(Kind::Text) | Some(Kind::Excerpt) => self.edit(b, pv.n, &id, false),
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
                if let Some(id) = b.hit(pv.n, at) {
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

    fn press(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView], q: Pos2, shift: bool, _alt: bool) {
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
                let mut hit = b.hit(n, at).map(|id| (n, id));
                if hit.is_none() && pv.lay.right && n > 1 {
                    let left_at = (at.0 + pv.lay.w, at.1);
                    if let Some(id) = b.hit(n - 1, left_at) {
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
            Drag::Crop { .. } => {}
        }
    }

    fn release(&mut self, b: &mut Book, views: &[PageView]) {
        b.guides.clear();
        let Some(d) = b.drag.take() else { return };
        match d {
            Drag::Move { page, moved, .. } => {
                if !moved {
                    b.uncheckpoint();
                    return;
                }
                self.settle(b, views, page);
            }
            Drag::Resize { page, id, orig, .. } => {
                if orig.kind.is_text() {
                    b.pending_reflow = Some((page, id));
                }
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

    fn click(&mut self, ctx: &egui::Context, b: &mut Book, views: &[PageView], q: Pos2, shift: bool) {
        let Some((pv, at)) = Self::at(views, q) else {
            b.commit_edit();
            b.sel.clear();
            return;
        };
        let n = pv.n;
        if matches!(self.tool, Tool::Pen | Tool::Line | Tool::Tape) {
            return;
        }
        if let Some(id) = b.hit(n, at) {
            let kind = b.page(n).and_then(|p| p.item(&id)).map(|i| i.kind.clone());
            // a text: typing starts where it was clicked (with the writing
            // tool, or on a text already picked)
            let picked = b.sel_page == n && b.sel == [id.clone()];
            if kind.as_ref().is_some_and(|k| k.is_text()) && !shift && (self.tool == Tool::Write || picked) {
                let at_char = self.char_at(ctx, b, pv, &id, at);
                self.edit(b, n, &id, false);
                b.edit_cursor = at_char;
                return;
            }
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
        let had = !b.sel.is_empty() || b.editing.is_some();
        b.commit_edit();
        b.sel.clear();
        // a click on an empty spot starts writing there (the second click,
        // when something was selected)
        if self.tool == Tool::Write || !had {
            self.write_at(b, pv, at);
        }
    }

    /// A new text whose first line sits on the line under the point, as
    /// wide as the column there.
    fn write_at(&mut self, b: &mut Book, pv: &PageView, at: (f64, f64)) {
        let lay = &pv.lay;
        if at.1 > lay.last_baseline() + 1.0 || at.1 < 0.0 || at.0 < 0.0 || at.0 > lay.w {
            return;
        }
        let (c0, c1) = lay.column_at(at.0);
        let x = if at.0 - c0 > 8.0 && at.0 < c1 - 10.0 { at.0 } else { c0 };
        let y = lay.text_top(at.1 - lay.paper.line() * 0.5, 1.0);
        b.checkpoint();
        let mut it = Item::new(Kind::Text, x, y, (c1 - x).max(10.0), lay.paper.line());
        // the notebook's font; in the margin, its hand
        let margin = lay.margin.is_some_and(|m| lay.columns[m] == (c0, c1));
        it.style.font = if margin { b.nb.margin_font.clone().unwrap_or_else(|| "mao".into()) } else { b.nb.font.clone().unwrap_or_default() };
        it.set_content("");
        let id = it.id.clone();
        if let Some(p) = b.page(pv.n) {
            p.items.push(it);
        }
        b.sel_page = pv.n;
        b.sel = vec![id.clone()];
        b.editing = Some(Editing { page: pv.n, id, buf: String::new(), new: true });
        b.focus_edit = true;
    }

    pub fn edit(&mut self, b: &mut Book, n: usize, id: &str, new: bool) {
        b.commit_edit();
        let Some(it) = b.page(n).and_then(|p| p.item(id)).cloned() else { return };
        if !it.kind.is_text() || it.locked {
            return;
        }
        b.checkpoint();
        b.sel_page = n;
        b.sel = vec![id.to_string()];
        b.editing = Some(Editing { page: n, id: id.to_string(), buf: super::flow_text(&it), new });
        b.focus_edit = true;
    }

    /// The char of a text under a point of its page.
    fn char_at(&self, ctx: &egui::Context, b: &mut Book, pv: &PageView, id: &str, at: (f64, f64)) -> Option<usize> {
        let mut it = b.page(pv.n)?.item(id)?.clone();
        it.style.align = String::new();
        if it.kind == Kind::Excerpt && !journal::excerpt_parts(it.content()).title.is_empty() {
            it.y += pv.lay.paper.line() * it.style.lines();
        }
        let text = super::flow_text(&it);
        let g = ctx.fonts(|f| f.layout_job(draw::plain_job(&it, &text, &pv.lay, pv.k, Color32::BLACK)));
        let origin = draw::text_origin(&g, pv, &it);
        let c = g.cursor_from_pos(pv.s(at) - origin);
        Some(c.ccursor.index)
    }

    /// The text being typed, over its box: the same font and lines as when
    /// it is shown.
    fn edit_overlay(&mut self, ui: &mut egui::Ui, b: &mut Book, views: &[PageView], p: crate::theme::Palette) {
        let Some(e) = b.editing.as_ref() else { return };
        let (n, id) = (e.page, e.id.clone());
        let Some(pv) = views.iter().find(|v| v.n == n).cloned() else {
            b.commit_edit();
            return;
        };
        let Some(mut it) = b.page(n).and_then(|pg| pg.item(&id)).cloned() else {
            b.editing = None;
            return;
        };
        it.style.align = String::new();
        if it.kind == Kind::Excerpt && !journal::excerpt_parts(it.content()).title.is_empty() {
            it.y += pv.lay.paper.line() * it.style.lines();
        }
        let lay = pv.lay.clone();
        let k = pv.k;
        let ink = it.style.color.as_deref().and_then(draw::hex).unwrap_or(draw::ink_color(p));
        let buf = b.editing.as_ref().map(|e| e.buf.clone()).unwrap_or_default();
        let now = ui.fonts(|f| f.layout_job(draw::plain_job(&it, if buf.is_empty() { " " } else { &buf }, &lay, k, ink)));
        let origin = draw::text_origin(&now, &pv, &it);
        let cap = lay.rows_from(it.y, it.style.lines()).max(1);
        let rows = now.rows.len().max(1);
        let lh = draw::line_px(&it, &lay, k);
        let line_mm = lay.paper.line() * it.style.lines();
        // the box grows with the text as it is typed
        let head = if it.y != b.page(n).and_then(|pg| pg.item(&id)).map(|i| i.y).unwrap_or(it.y) { 1 } else { 0 };
        if let Some(slot) = b.page(n).and_then(|pg| pg.item_mut(&id)) {
            slot.h = (rows.min(cap) + head) as f64 * line_mm;
        }
        let rect = Rect::from_min_size(origin, vec2(it.w as f32 * k, (cap as f32 + 1.0) * lh));
        let edit_id = egui::Id::new(("journal-edit", &id));
        if let Some(c) = b.edit_cursor.take() {
            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(c))));
            st.store(ui.ctx(), edit_id);
        }
        let it2 = it.clone();
        let mut layouter = move |ui: &egui::Ui, text: &str, _w: f32| ui.fonts(|f| f.layout_job(draw::plain_job(&it2, text, &lay, k, ink)));
        let Some(e) = b.editing.as_mut() else { return };
        let te = egui::TextEdit::multiline(&mut e.buf).frame(false).margin(egui::Margin::ZERO).desired_width(it.w as f32 * k).text_color(ink).layouter(&mut layouter).id(edit_id);
        // typed text never shows past the page
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(pv.clip.intersect(ui.clip_rect()));
        let out = te.show(&mut child);
        let buf_len = e.buf.chars().count();
        let resp = out.response;
        if resp.changed() {
            // saved a moment after the last key, typing or not
            b.touch(n);
        }
        if b.focus_edit {
            resp.request_focus();
            b.focus_edit = false;
        }
        // past the last line, with the cursor at the end: on to the next page
        let at_end = out.cursor_range.is_some_and(|c| c.primary.ccursor.index >= buf_len);
        if out.galley.rows.len() > cap && at_end && resp.changed() {
            let before = b.page(n).and_then(|pg| pg.item(&id)).and_then(|i| i.flow.as_ref()).and_then(|f| f.next.clone()).and_then(|nx| b.find(&nx).and_then(|(pn, _)| b.page(pn).and_then(|pg| pg.item(&nx)).map(|i| super::flow_text(i).chars().count()))).unwrap_or(0);
            b.continue_edit = Some((id.clone(), before));
            b.commit_edit();
            return;
        }
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        // a click on the balloon (font, size, colour) keeps the typing going
        let on_balloon = ui.ctx().input(|i| i.pointer.interact_pos()).and_then(|q| ui.ctx().layer_id_at(q)).is_some_and(|l| l.order == egui::Order::Foreground);
        if esc || (resp.lost_focus() && !on_balloon && !b.focus_edit) {
            b.commit_edit();
            return;
        }
        // focus comes back from a button of the balloon (else a space
        // typed next would press it)
        if !resp.has_focus() && !ui.ctx().wants_keyboard_input() && !ui.ctx().input(|i| i.pointer.any_down()) {
            resp.request_focus();
        }
    }

    fn draw_selection(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView], p: crate::theme::Palette) {
        let Some(pv) = views.iter().find(|v| v.n == b.sel_page) else { return };
        let painter = ui.painter_at(pv.clip_across.union(pv.clip));
        let items = b.sel_items();
        for it in &items {
            let c: Vec<Pos2> = it.corners().iter().map(|q| pv.s(*q)).collect();
            painter.add(egui::Shape::closed_line(c, Stroke::new(1.5_f32, p.accent_strong)));
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

    fn keys(&mut self, ui: &egui::Ui, b: &mut Book) {
        use egui::Key;
        let m = ui.input(|i| i.modifiers);
        let press = |k: Key| ui.input(|i| i.key_pressed(k));
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
                self.edit(b, n, &id, false);
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
                    it.look.shadow = true;
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
                    b.pending_reflow = Some((n, id.clone()));
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
            Payload::Frame(board, id, index) => {
                match super::boards::save_frame(cx, &board, &id, index) {
                    Ok((rel, frame, (w, h))) => {
                        self.out.push(super::Out::Rescan);
                        let (c0, c1) = lay.main_column();
                        let fw = (c1 - c0).min(lay.w * 0.8);
                        let fh = fw * h as f64 / w.max(1) as f64;
                        b.checkpoint();
                        let it = super::boards::frame_item(&rel, &board, &frame, at.0 - fw / 2.0, at.1 - fh / 2.0, fw, fh);
                        let id = it.id.clone();
                        if let Some(p) = b.page(n) {
                            p.items.push(it);
                        }
                        b.sel_page = n;
                        b.sel = vec![id];
                        b.touch(n);
                    }
                    Err(e) => self.out.push(super::Out::Toast(format!("Não foi possível trazer o quadro: {e}"))),
                }
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
            None => {
                // the first lines of the column with nothing on them
                let s = lay.paper.line();
                let boxes: Vec<ateliet_core::canvas::Rect> = b.page(n).map(|p| p.items.iter().map(|i| i.bbox()).collect()).unwrap_or_default();
                lay.baselines.iter().map(|b| b - s).find(|&top| {
                    let want = MRect { x: c0, y: top, w: c1 - c0, h: s * 3.0 };
                    !boxes.iter().any(|o| o.intersects(&want))
                }).unwrap_or(lay.first_baseline() - s)
            }
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
            b.pending_reflow = Some((n, id.clone()));
        }
        if let Some(p) = b.page(n) {
            p.add_source(note.trim_end_matches(".md"));
        }
        b.sel_page = n;
        b.sel = vec![id];
        b.touch(n);
    }
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
