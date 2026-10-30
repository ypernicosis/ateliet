//! The text being typed on a page: where writing starts, the keys (and
//! the input method, the clipboard), the cursor and the selection.
//!
//! What is typed goes straight into its box, and the page is set again
//! every frame (`Book::settle`): there is no editor over the page with
//! a layout of its own, the letters typed are the page's letters. The
//! cursor is a char of the text; where it shows, and where a click or the
//! arrow keys take it, comes from the rows the page was set in.

use super::draw::{self, PageView};
use super::typeset::{self, Placed, Styled};
use super::{Book, Editing, Journal, Shown};
use ateliet_core::journal::wrap::{self, Set};
use ateliet_core::journal::{Item, Kind};
use eframe::egui::{self, pos2, Color32, Pos2, Rect, Stroke};
use std::time::Duration;

/// The keyboard's focus while typing.
pub fn typing_id() -> egui::Id {
    egui::Id::new("journal-typing")
}

/// A pause this long starts a new undo step.
const PAUSE: Duration = Duration::from_millis(1200);

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_left(t: &[char], c: usize) -> usize {
    let mut i = c.min(t.len());
    while i > 0 && !is_word(t[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word(t[i - 1]) {
        i -= 1;
    }
    i
}

fn word_right(t: &[char], c: usize) -> usize {
    let mut i = c.min(t.len());
    while i < t.len() && !is_word(t[i]) {
        i += 1;
    }
    while i < t.len() && is_word(t[i]) {
        i += 1;
    }
    i
}

/// The word round char `c` (or the run of spaces, or the mark, it is in).
fn word_at(t: &[char], c: usize) -> (usize, usize) {
    let c = c.min(t.len());
    let kind = |ch: char| if is_word(ch) { 0 } else if ch.is_whitespace() { 1 } else { 2 };
    let k = t.get(c).or_else(|| c.checked_sub(1).and_then(|p| t.get(p))).map(|ch| kind(*ch));
    let Some(k) = k else { return (c, c) };
    let (mut a, mut z) = (c, c);
    while a > 0 && kind(t[a - 1]) == k {
        a -= 1;
    }
    while z < t.len() && kind(t[z]) == k {
        z += 1;
    }
    (a, z)
}

/// The line (between line breaks) round char `c`.
fn line_at(t: &[char], c: usize) -> (usize, usize) {
    let c = c.min(t.len());
    let a = t[..c].iter().rposition(|ch| *ch == '\n').map(|p| p + 1).unwrap_or(0);
    let z = t[c..].iter().position(|ch| *ch == '\n').map(|p| c + p).unwrap_or(t.len());
    (a, z)
}

fn byte(s: &str, c: usize) -> usize {
    s.char_indices().nth(c).map(|(b, _)| b).unwrap_or(s.len())
}

impl Journal {
    // ── the pages as they are set ─────────────────────────────────────────

    /// Set the pages on the screen (and those waiting to be set), and lay
    /// their texts out at the scale they are shown at.
    pub fn settle_shown(&mut self, ctx: &egui::Context, b: &mut Book, views: &[PageView], p: crate::theme::Palette) {
        self.shown.clear();
        for pv in views {
            b.unsettled.insert(pv.n);
        }
        let mut guard = 0;
        while let Some(n) = b.unsettled.pop_first() {
            guard += 1;
            if guard > 24 {
                break;
            }
            let Some(path) = b.page_ref(n).map(|r| r.path.clone()) else { continue };
            if let Some(set) = b.settle(ctx, n, self.sets.get(&path)) {
                self.sets.insert(path, set);
            }
        }
        b.unsettled.clear();
        let typing = b.editing.as_ref().map(|e| (e.page, e.id.clone(), e.buf.clone(), e.cursor));
        for pv in views {
            let Some(path) = b.page_ref(pv.n).map(|r| r.path.clone()) else { continue };
            let Some(set) = self.sets.get(&path).cloned() else { continue };
            let items = b.page(pv.n).map(|pg| pg.items.clone()).unwrap_or_default();
            let t = typing.as_ref().filter(|t| t.0 == pv.n).map(|t| (t.1.as_str(), t.2.as_str(), t.3));
            let styled = typeset::styled_page(&items, &pv.lay, pv.k, t, draw::ink_color(p), p.accent_strong);
            self.shown.insert(pv.n, Shown { path, set, styled });
        }
    }

    /// A text's rows on the screen: as its page was set this frame, or (a
    /// cover on the shelf, a thumbnail) set alone.
    pub fn rows_of(&self, ctx: &egui::Context, it: &Item, pv: &PageView, source: &str, p: crate::theme::Palette) -> Vec<Placed> {
        if let Some(sh) = self.shown.get(&pv.n).filter(|s| s.path == source) {
            if let (Some(set), Some(st)) = (sh.set.get(&it.id), sh.styled.get(&it.id)) {
                return ctx.fonts(|f| typeset::place_all(f, it, st, set, pv));
            }
        }
        let set = typeset::set_alone(ctx, it, &pv.lay);
        let ink = draw::ink_color(p);
        let color = it.style.color.as_deref().and_then(draw::hex).unwrap_or(ink);
        let foot = typeset::foot_of(it);
        let st = typeset::styled(it, &super::flow_text(it), &pv.lay, pv.k, color, p.accent_strong, None, Some((&foot, ink.gamma_multiply(0.55))));
        ctx.fonts(|f| typeset::place_all(f, it, &st, &set, pv))
    }

    /// A text of a shown page: its setting, how it shows, its rows on the screen.
    fn text_view(&self, ctx: &egui::Context, b: &mut Book, pv: &PageView, id: &str) -> Option<(Set, Styled, Vec<Placed>)> {
        let sh = self.shown.get(&pv.n)?;
        let set = sh.set.get(id)?.clone();
        let st = sh.styled.get(id)?.clone();
        let it = b.page(pv.n)?.item(id)?.clone();
        let rows = ctx.fonts(|f| typeset::place_all(f, &it, &st, &set, pv));
        Some((set, st, rows))
    }

    /// The char of a text nearest a point of its page.
    pub fn char_near_point(&self, ctx: &egui::Context, b: &mut Book, pv: &PageView, id: &str, at: (f64, f64)) -> Option<usize> {
        let (set, st, rows) = self.text_view(ctx, b, pv, id)?;
        typeset::char_near(&set, &rows, pv, at).map(|d| st.buf_of(d))
    }

    /// Paper beside a row's letters, on its line, where a new text has no
    /// room: just past their end or just before their start. The nearest
    /// such row: its text, the char at that edge, and whether it is the
    /// row's start.
    pub fn row_edge_near(&self, b: &mut Book, n: usize, at: (f64, f64)) -> Option<(String, usize, bool)> {
        let sh = self.shown.get(&n)?;
        let mut best: Option<(f64, String, usize, bool)> = None;
        for t in &sh.set.texts {
            if b.page(n).and_then(|p| p.item(&t.id)).is_none_or(|i| i.locked) {
                continue;
            }
            let Some(st) = sh.styled.get(&t.id) else { continue };
            for (r, row) in t.rows.iter().enumerate() {
                if at.1 < row.base - t.row_h || at.1 > row.base + 0.4 {
                    continue;
                }
                let end = row.x + row.width;
                let edge = if at.0 >= end && at.0 <= end + wrap::GAP + 3.0 {
                    let mut d = row.to;
                    if !row.newline && r + 1 < t.rows.len() && d > row.from && st.chars.get(d - 1).is_some_and(|c| c.is_whitespace()) {
                        d -= 1;
                    }
                    Some((at.0 - end, d, false))
                } else if at.0 < row.x && at.0 > row.x - wrap::GAP - wrap::MIN_ROW {
                    Some((row.x - at.0, row.from, true))
                } else {
                    None
                };
                if let Some((dist, d, start)) = edge.filter(|e| best.as_ref().is_none_or(|b| e.0 < b.0)) {
                    best = Some((dist, t.id.clone(), st.buf_of(d), start));
                }
            }
        }
        best.map(|(_, id, c, start)| (id, c, start))
    }

    // ── where writing starts ──────────────────────────────────────────────

    /// Start typing in a text, the cursor at `cursor` (its end when none).
    pub fn edit(&mut self, b: &mut Book, n: usize, id: &str, cursor: Option<usize>) {
        if b.editing.as_ref().is_some_and(|e| e.page == n && e.id == id) {
            if let (Some(e), Some(c)) = (b.editing.as_mut(), cursor) {
                e.cursor = c.min(e.buf.chars().count());
                e.anchor = e.cursor;
            }
            return;
        }
        b.commit_edit();
        let Some(it) = b.page(n).and_then(|p| p.item(id)).cloned() else { return };
        if !it.kind.is_text() || it.locked {
            return;
        }
        b.sel_page = n;
        b.sel = vec![id.to_string()];
        b.editing = Some(Editing::new(n, id, super::flow_text(&it), false, cursor));
    }

    /// A new text on the line under the point, starting right there. Its
    /// measure runs to what is next on that line (another text's letters,
    /// 1 mm short of them, so it fits in a gap between two words; a
    /// picture) or the column's end; its rows go round whatever is in their
    /// way below. Begun mid-line with nothing to its left (and the notebook
    /// set so), its next rows go back to the column's margin. False when
    /// there is no room for one there.
    pub fn write_at(&mut self, b: &mut Book, pv: &PageView, at: (f64, f64)) -> bool {
        let lay = &pv.lay;
        if at.1 > lay.last_baseline() + 1.0 || at.1 < 0.0 || at.0 < 0.0 || at.0 > lay.w {
            return false;
        }
        let (c0, c1) = lay.column_at(at.0);
        let s = lay.paper.line();
        let x = if at.0 - c0 < 3.0 { c0 } else { at.0.min(c1 - wrap::MIN_ROW) };
        let y = lay.text_top(at.1 - s * 0.5, 1.0);
        let (y0, y1) = (y, y + s);
        let meets = |r0: f64, r1: f64| r0 < y1 - 0.3 && r1 > y0 + 0.3;
        let mut right = c1;
        let mut left_ink = false;
        if let Some(sh) = self.shown.get(&pv.n) {
            // (as set this frame: a text just let go of is gone already)
            for t in sh.set.texts.iter().filter(|t| b.page(pv.n).is_some_and(|p| p.item(&t.id).is_some())) {
                for r in t.rows.iter().filter(|r| meets(r.base - t.row_h, r.base)) {
                    if r.x >= x {
                        right = right.min(r.x - wrap::NEAR);
                    } else if r.x + r.width > c0 {
                        left_ink = true;
                    }
                }
            }
        }
        if let Some(page) = b.page(pv.n) {
            for o in page.items.iter().filter(|i| wrap::in_the_way(i)) {
                let bb = o.bbox();
                if meets(bb.y, bb.y + bb.h) && bb.x >= x {
                    right = right.min(bb.x - wrap::GAP);
                }
            }
        }
        if right - x < wrap::NARROW {
            return false;
        }
        b.checkpoint();
        let mut it = if b.nb.wrap_to_margin && x > c0 && !left_ink && right >= c1 - 0.01 {
            let mut it = Item::new(Kind::Text, c0, y, c1 - c0, s);
            it.style.indent = ((x - c0) * 100.0).round() / 100.0;
            it
        } else {
            Item::new(Kind::Text, x, y, right - x, s)
        };
        if (b.nb.text_size - 1.0).abs() > 1e-6 {
            it.style.size = b.nb.text_size;
        }
        it.style.align = b.nb.align.clone();
        // the notebook's font; in the margin, its hand
        let margin = lay.margin.is_some_and(|m| lay.columns[m] == (c0, c1));
        it.style.font = if margin { b.nb.margin_font.clone().unwrap_or_else(|| "mao".into()) } else { b.nb.font.clone().unwrap_or_default() };
        if pv.n == 0 {
            it.style.color = Some(super::input::cover_ink(&b.nb.cover));
        }
        it.set_content("");
        let id = it.id.clone();
        if let Some(p) = b.page(pv.n) {
            p.items.push(it);
        }
        b.sel_page = pv.n;
        b.sel = vec![id.clone()];
        b.editing = Some(Editing::new(pv.n, &id, String::new(), true, Some(0)));
        true
    }

    /// A new text on the first free lines of the page's main column,
    /// starting with what was typed.
    pub fn write_free(&mut self, b: &mut Book, pv: &PageView, typed: &str) {
        let lay = pv.lay.clone();
        let col = lay.main_column();
        let top = super::input::free_top(b, pv.n, &lay, col, 1.0);
        b.checkpoint();
        let mut it = Item::new(Kind::Text, col.0, top, col.1 - col.0, lay.paper.line());
        it.style.font = b.nb.font.clone().unwrap_or_default();
        if (b.nb.text_size - 1.0).abs() > 1e-6 {
            it.style.size = b.nb.text_size;
        }
        it.style.align = b.nb.align.clone();
        if pv.n == 0 {
            it.style.color = Some(super::input::cover_ink(&b.nb.cover));
        }
        it.set_content(typed);
        let id = it.id.clone();
        if let Some(p) = b.page(pv.n) {
            p.items.push(it);
        }
        b.sel_page = pv.n;
        b.sel = vec![id.clone()];
        let mut e = Editing::new(pv.n, &id, typed.to_string(), true, None);
        if !typed.is_empty() {
            e.typed = Some(std::time::Instant::now());
        }
        b.editing = Some(e);
    }

    /// A click in the text being typed (or one to type in): the cursor
    /// there; two clicks pick a word, three a line; with shift, the
    /// selection goes to there.
    pub fn click_text(&mut self, b: &mut Book, n: usize, id: &str, c: Option<usize>, count: u8, shift: bool) {
        let typing_it = b.editing.as_ref().is_some_and(|e| e.page == n && e.id == id);
        if !typing_it {
            self.edit(b, n, id, c);
        }
        let Some(e) = b.editing.as_mut() else { return };
        let c = c.unwrap_or(e.cursor).min(e.buf.chars().count());
        let t: Vec<char> = e.buf.chars().collect();
        match count {
            2 => {
                let (a, z) = word_at(&t, c);
                e.anchor = a;
                e.cursor = z;
            }
            3 => {
                let (a, z) = line_at(&t, c);
                e.anchor = a;
                e.cursor = z;
            }
            _ => {
                e.cursor = c;
                if !shift || !typing_it {
                    e.anchor = c;
                }
            }
        }
        e.goal = None;
        e.ime = None;
    }

    /// The pointer sweeping the text being typed: the selection runs to it.
    pub fn sweep_to(&mut self, ctx: &egui::Context, b: &mut Book, views: &[PageView], q: Pos2) {
        let Some((n, id)) = b.editing.as_ref().map(|e| (e.page, e.id.clone())) else { return };
        let Some(pv) = views.iter().find(|v| v.n == n).cloned() else { return };
        if let Some(c) = self.char_near_point(ctx, b, &pv, &id, pv.w(q)) {
            if let Some(e) = b.editing.as_mut() {
                e.cursor = c;
                e.goal = None;
            }
        }
    }

    // ── the keys ──────────────────────────────────────────────────────────

    /// The keys, the input method and the clipboard, while a text is being
    /// typed. The keyboard is the text's while it is typed (but a text field
    /// elsewhere, open over the page, keeps it).
    pub fn type_keys(&mut self, ui: &egui::Ui, b: &mut Book, views: &[PageView]) {
        let ctx = ui.ctx().clone();
        let fid = typing_id();
        if b.editing.is_none() {
            if ctx.memory(|m| m.has_focus(fid)) {
                ctx.memory_mut(|m| m.surrender_focus(fid));
            }
            return;
        }
        let other = ctx.memory(|m| m.focused()).filter(|f| *f != fid);
        if other.is_some_and(|f| egui::text_edit::TextEditState::load(&ctx, f).is_some()) {
            return;
        }
        ctx.memory_mut(|m| {
            m.request_focus(fid);
            m.set_focus_lock_filter(fid, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true });
        });
        let events = ctx.input(|i| i.events.clone());
        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        for ev in events {
            let Some(composing) = b.editing.as_ref().map(|e| e.ime.is_some()) else { break };
            match ev {
                egui::Event::Text(t) if t != "\n" && t != "\r" => self.replace_sel(b, &t),
                egui::Event::Paste(t) if !t.is_empty() => self.replace_sel(b, &t.replace("\r\n", "\n").replace('\r', "\n")),
                egui::Event::Copy => {
                    if let Some(s) = selected(b) {
                        ctx.copy_text(s);
                    }
                }
                egui::Event::Cut => {
                    if let Some(s) = selected(b) {
                        ctx.copy_text(s);
                        self.replace_sel(b, "");
                    }
                }
                egui::Event::Ime(ime) => self.ime(b, ime),
                egui::Event::Key { key, pressed: true, modifiers, repeat, .. } => {
                    use egui::Key::*;
                    if composing && (repeat || matches!(key, Backspace | ArrowUp | ArrowDown | ArrowLeft | ArrowRight)) {
                        continue;
                    }
                    self.key(&ctx, b, views, key, modifiers, mac);
                }
                _ => {}
            }
        }
    }

    /// Change what is typed: chars `a..z` become `with`, the cursor after
    /// them. A pause before it starts a new undo step.
    fn change(&mut self, b: &mut Book, a: usize, z: usize, with: &str) {
        let Some(e) = b.editing.as_ref() else { return };
        let step = e.typed.map_or(!e.new, |t| t.elapsed() > PAUSE);
        if step {
            b.checkpoint();
        }
        let Some(e) = b.editing.as_mut() else { return };
        let len = e.buf.chars().count();
        let (a, z) = (a.min(len), z.min(len).max(a.min(len)));
        let (ba, bz) = (byte(&e.buf, a), byte(&e.buf, z));
        e.buf.replace_range(ba..bz, with);
        e.cursor = a + with.chars().count();
        e.anchor = e.cursor;
        e.goal = None;
        e.typed = Some(std::time::Instant::now());
        b.store_typing();
    }

    fn replace_sel(&mut self, b: &mut Book, with: &str) {
        let Some(e) = b.editing.as_ref() else { return };
        let (a, z) = e.ime.unwrap_or(e.range());
        self.change(b, a, z, with);
        if let Some(e) = b.editing.as_mut() {
            e.ime = None;
        }
    }

    fn ime(&mut self, b: &mut Book, ev: egui::ImeEvent) {
        let Some(e) = b.editing.as_ref() else { return };
        match ev {
            egui::ImeEvent::Preedit(t) if t != "\n" && t != "\r" => {
                let (a, z) = e.ime.unwrap_or(e.range());
                self.change(b, a, z, &t);
                if let Some(e) = b.editing.as_mut() {
                    let n = t.chars().count();
                    e.ime = (n > 0).then_some((a, a + n));
                }
            }
            egui::ImeEvent::Commit(t) if t != "\n" && t != "\r" => self.replace_sel(b, &t),
            egui::ImeEvent::Disabled => {
                if let Some(e) = b.editing.as_mut() {
                    e.ime = None;
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn key(&mut self, ctx: &egui::Context, b: &mut Book, views: &[PageView], key: egui::Key, m: egui::Modifiers, mac: bool) {
        use egui::Key::*;
        let Some(e) = b.editing.as_ref() else { return };
        let (n, id) = (e.page, e.id.clone());
        let t: Vec<char> = e.buf.chars().collect();
        let (c, (a, z)) = (e.cursor, e.range());
        let word = if mac { m.alt } else { m.ctrl };
        let line = mac && m.mac_cmd;
        let goto = |b: &mut Book, to: usize, keep: bool| {
            if let Some(e) = b.editing.as_mut() {
                e.cursor = to.min(e.buf.chars().count());
                if !keep {
                    e.anchor = e.cursor;
                }
                e.goal = None;
            }
        };
        match key {
            Escape => {
                b.commit_edit();
                if b.page(n).and_then(|p| p.item(&id)).is_some() {
                    b.sel_page = n;
                    b.sel = vec![id];
                }
            }
            Enter if m.command && m.shift => self.immersive = !self.immersive,
            Enter if m.command => {
                b.commit_edit();
                b.new_page_here(&mut self.out);
            }
            Enter => {
                // a list goes on: a new item (an empty one ends the list)
                let (l0, _) = line_at(&t, c);
                let this: String = t[l0..c].iter().collect();
                let lead: String = this.chars().take_while(|ch| *ch == ' ' || *ch == '\t').collect();
                let rest = &this[lead.len()..];
                let mark = ["- [ ] ", "- [x] ", "- ", "* "].into_iter().find(|mk| rest.starts_with(mk));
                match mark {
                    Some(mk) if rest.len() == mk.len() && a == z => self.change(b, l0, c, ""),
                    Some(mk) => {
                        let mk = if mk.starts_with("- [") { "- [ ] " } else { mk };
                        self.change(b, a, z, &format!("\n{lead}{mk}"));
                    }
                    None => self.change(b, a, z, "\n"),
                }
            }
            Backspace => {
                if a != z {
                    self.change(b, a, z, "");
                } else if c > 0 {
                    let from = if line {
                        self.row_edge(ctx, b, views, false).filter(|s| *s < c).unwrap_or_else(|| line_at(&t, c).0.min(c - 1))
                    } else if word {
                        word_left(&t, c)
                    } else {
                        c - 1
                    };
                    self.change(b, from, c, "");
                }
            }
            Delete => {
                if a != z {
                    self.change(b, a, z, "");
                } else if c < t.len() {
                    let to = if word { word_right(&t, c) } else { c + 1 };
                    self.change(b, c, to, "");
                }
            }
            ArrowLeft | ArrowRight => {
                let right = key == ArrowRight;
                let to = if a != z && !m.shift {
                    if right { z } else { a }
                } else if line {
                    self.row_edge(ctx, b, views, right).unwrap_or(c)
                } else if word {
                    if right { word_right(&t, c) } else { word_left(&t, c) }
                } else if right {
                    (c + 1).min(t.len())
                } else {
                    c.saturating_sub(1)
                };
                goto(b, to, m.shift);
            }
            Home | End => {
                let end = key == End;
                let to = if m.ctrl && !mac { if end { t.len() } else { 0 } } else { self.row_edge(ctx, b, views, end).unwrap_or(c) };
                goto(b, to, m.shift);
            }
            ArrowUp | ArrowDown => {
                let down = key == ArrowDown;
                if m.command {
                    goto(b, if down { t.len() } else { 0 }, m.shift);
                } else if let Some((to, goal)) = self.row_step(ctx, b, views, down) {
                    goto(b, to, m.shift);
                    if let Some(e) = b.editing.as_mut() {
                        e.goal = Some(goal);
                    }
                }
            }
            A if m.command => {
                if let Some(e) = b.editing.as_mut() {
                    e.anchor = 0;
                    e.cursor = t.len();
                }
            }
            Z if m.command => self.undo_typing(b, m.shift),
            Y if m.command => self.undo_typing(b, true),
            B if m.command => self.toggle_mark(b, "**"),
            I if m.command => self.toggle_mark(b, "*"),
            Equals | Plus | Minus if m.command => {
                let sizes: Vec<f64> = super::chrome::SIZES.iter().map(|s| s.0).collect();
                let now = b.page(n).and_then(|p| p.item(&id)).map(|i| i.style.size()).unwrap_or(1.0);
                let next = if key == Minus { sizes.iter().rev().copied().find(|s| *s < now - 0.01) } else { sizes.iter().copied().find(|s| *s > now + 0.01) };
                if let Some(sz) = next {
                    b.checkpoint();
                    if let Some(slot) = b.page(n).and_then(|p| p.item_mut(&id)) {
                        slot.style.size = if (sz - 1.0).abs() < 1e-6 { 0.0 } else { sz };
                    }
                    b.touch(n);
                }
            }
            _ => {}
        }
    }

    /// `**` or `*` round the selection (off again when it is there already);
    /// with nothing selected, a pair to type in.
    fn toggle_mark(&mut self, b: &mut Book, mk: &str) {
        let Some(e) = b.editing.as_ref() else { return };
        let t: Vec<char> = e.buf.chars().collect();
        let (a, z) = e.range();
        let k = mk.chars().count();
        let has = a >= k && z + k <= t.len() && t[a - k..a].iter().collect::<String>() == mk && t[z..z + k].iter().collect::<String>() == mk;
        let inner: String = t[a..z].iter().collect();
        if has {
            self.change(b, a - k, z + k, &inner);
            if let Some(e) = b.editing.as_mut() {
                e.anchor = a - k;
                e.cursor = z - k;
            }
        } else {
            self.change(b, a, z, &format!("{mk}{inner}{mk}"));
            if let Some(e) = b.editing.as_mut() {
                e.anchor = a + k;
                e.cursor = z + k;
            }
        }
    }

    /// Undo (or redo) while typing: the typing goes on where it was.
    fn undo_typing(&mut self, b: &mut Book, redo: bool) {
        let Some((n, id, c)) = b.editing.as_ref().map(|e| (e.page, e.id.clone(), e.cursor)) else { return };
        b.commit_edit();
        if redo {
            b.redo();
        } else {
            b.undo();
        }
        if b.page(n).and_then(|p| p.item(&id)).is_some() {
            self.edit(b, n, &id, Some(c));
        }
    }

    /// The text being typed, as set on its page, with its rows on the screen.
    fn typed_view(&self, ctx: &egui::Context, b: &mut Book, views: &[PageView]) -> Option<(PageView, Set, Styled, Vec<Placed>)> {
        let (n, id) = b.editing.as_ref().map(|e| (e.page, e.id.clone()))?;
        let pv = views.iter().find(|v| v.n == n)?.clone();
        let (set, st, rows) = self.text_view(ctx, b, &pv, &id)?;
        Some((pv, set, st, rows))
    }

    /// The start or the end of the row the cursor is on (a row broken at a
    /// space ends before the space).
    fn row_edge(&self, ctx: &egui::Context, b: &mut Book, views: &[PageView], end: bool) -> Option<usize> {
        let (_, set, st, _) = self.typed_view(ctx, b, views)?;
        let c = b.editing.as_ref()?.cursor;
        let r = set.row_of(st.disp_of(c));
        let row = set.rows.get(r)?;
        if !end {
            return Some(st.buf_of(row.from));
        }
        let mut d = row.to;
        if !row.newline && r + 1 < set.rows.len() && d > row.from && st.chars.get(d - 1).is_some_and(|ch| ch.is_whitespace()) {
            d -= 1;
        }
        Some(st.buf_of(d))
    }

    /// The char one row up or down, nearest to where the cursor keeps to
    /// (and that place).
    fn row_step(&self, ctx: &egui::Context, b: &mut Book, views: &[PageView], down: bool) -> Option<(usize, f32)> {
        let (pv, set, st, rows) = self.typed_view(ctx, b, views)?;
        let e = b.editing.as_ref()?;
        let len = e.buf.chars().count();
        let (r, x) = typeset::caret_at(&set, &rows, st.disp_of(e.cursor))?;
        let goal = e.goal.unwrap_or(x);
        let to = if down { r + 1 } else { r.checked_sub(1).unwrap_or(usize::MAX) };
        let Some(row) = set.rows.get(to) else { return Some((if down { len } else { 0 }, goal)) };
        let base = row.base - set.row_h * 0.5;
        let at = (pv.w(pos2(goal, 0.0)).0, base);
        let d = typeset::char_near(&set, &rows, &pv, at).unwrap_or(row.from);
        Some((st.buf_of(d), goal))
    }

    // ── the cursor and the selection ──────────────────────────────────────

    /// The selection of the text being typed: a soft wash, as a marker on
    /// paper, under its letters.
    pub fn draw_typed_selection(&self, ui: &egui::Ui, painter: &egui::Painter, b: &Book, it: &Item, pv: &PageView, wash: Color32) {
        let Some(e) = b.editing.as_ref().filter(|e| e.page == pv.n && e.id == it.id) else { return };
        let (a, z) = e.range();
        if a == z {
            return;
        }
        let Some(sh) = self.shown.get(&pv.n) else { return };
        let (Some(set), Some(st)) = (sh.set.get(&it.id), sh.styled.get(&it.id)) else { return };
        let rows = ui.ctx().fonts(|f| typeset::place_all(f, it, st, set, pv));
        let (da, dz) = (st.disp_of(a), st.disp_of(z));
        let lh = set.row_h as f32 * pv.k;
        for (r, row) in set.rows.iter().enumerate() {
            let (lo, hi) = (da.max(row.from), dz.min(row.to));
            let past = dz > row.to && da <= row.to;
            if lo > hi || (lo == hi && !past) {
                continue;
            }
            let Some(p) = rows.get(r) else { continue };
            let x0 = p.x_of(row.from, lo);
            let x1 = p.x_of(row.from, hi) + if past && row.newline { lh * 0.25 } else { 0.0 };
            let base = pv.s((0.0, row.base)).y;
            painter.rect_filled(Rect::from_min_max(pos2(x0, base - lh * 0.86), pos2(x1.max(x0 + 1.0), base + lh * 0.1)), 2.0, wash);
        }
    }

    /// The cursor of the text being typed, the input method's place, and
    /// what it is composing (underlined).
    pub fn typing_overlay(&mut self, ui: &mut egui::Ui, b: &mut Book, views: &[PageView], p: crate::theme::Palette) {
        let ctx = ui.ctx().clone();
        let Some(e) = b.editing.as_ref() else {
            b.caret = None;
            return;
        };
        let (n, id, cursor, ime, len) = (e.page, e.id.clone(), e.cursor, e.ime, e.buf.len());
        if !views.iter().any(|v| v.n == n) {
            // the typing went on to a page about to be shown
            if b.follow.is_none() {
                b.commit_edit();
            }
            return;
        }
        let Some((pv, set, st, rows)) = self.typed_view(&ctx, b, views) else {
            // begun this frame: set on the next
            ctx.request_repaint();
            return;
        };
        let Some(it) = b.page(n).and_then(|pg| pg.item(&id)).cloned() else { return };
        let ink = it.style.color.as_deref().and_then(draw::hex).unwrap_or(draw::ink_color(p));
        let Some((r, x)) = typeset::caret_at(&set, &rows, st.disp_of(cursor)) else { return };
        let row = &set.rows[r];
        let lh = draw::line_px(&it, &pv.lay, pv.k);
        let base = pv.s((0.0, row.base)).y - lh * 0.06;
        let size = lh * draw::font_of(&it).3;
        let caret_rect = Rect::from_min_max(pos2(x - 1.0, base - size * 0.82), pos2(x + 1.0, base + size * 0.14));
        let text_rect = set.ink().iter().map(|r| Rect::from_min_max(pv.s((r.x, r.y)), pv.s((r.x + r.w, r.y + r.h)))).reduce(|a, c| a.union(c)).unwrap_or(caret_rect);
        // the keyboard's focus lives here (egui drops a focus no widget holds)
        ui.interact(caret_rect, typing_id(), egui::Sense::focusable_noninteractive());
        ctx.output_mut(|o| o.ime = Some(egui::output::IMEOutput { rect: text_rect, cursor_rect: caret_rect }));
        // what the input method is composing, underlined
        if let Some((a, z)) = ime {
            let (da, dz) = (st.disp_of(a), st.disp_of(z));
            for (k, rw) in set.rows.iter().enumerate() {
                let (lo, hi) = (da.max(rw.from), dz.min(rw.to));
                if lo >= hi {
                    continue;
                }
                let Some(pl) = rows.get(k) else { continue };
                let y = pv.s((0.0, rw.base)).y + 1.5;
                ui.painter_at(pv.clip).hline(pl.x_of(rw.from, lo)..=pl.x_of(rw.from, hi), y, Stroke::new(1.0_f32, ink));
            }
        }
        let key = (n, id.clone(), cursor, len);
        let moved = self.caret_key.as_ref() != Some(&key);
        self.caret_key = Some(key);
        self.caret(ui, &pv, pos2(x, base), size, lh, ink, moved);
        b.caret = Some(pos2(x, base - lh * 0.5));
    }

    /// The cursor: a thin stroke of the ink, from the line up to the height
    /// of the capitals, that glides along a line as letters come and fades
    /// in and out instead of blinking.
    #[allow(clippy::too_many_arguments)]
    fn caret(&mut self, ui: &egui::Ui, pv: &PageView, base: Pos2, size: f32, lh: f32, ink: Color32, moved: bool) {
        let ctx = ui.ctx();
        let root = egui::Id::new("journal-caret");
        // a jump (another line, a click elsewhere, another text) puts it
        // there at once: a fresh animation starts where it is going
        let last: Option<Pos2> = ctx.data(|d| d.get_temp(root.with("at")));
        let mut gen: u64 = ctx.data(|d| d.get_temp(root.with("gen"))).unwrap_or(0);
        if last.is_none_or(|l| (l.y - base.y).abs() > lh * 0.5 || (l.x - base.x).abs() > size * 2.5) {
            gen += 1;
            ctx.data_mut(|d| d.insert_temp(root.with("gen"), gen));
        }
        ctx.data_mut(|d| d.insert_temp(root.with("at"), base));
        let x = ctx.animate_value_with_time(root.with(gen).with("x"), base.x, 0.05);
        let now = ctx.input(|i| i.time);
        if moved {
            ctx.data_mut(|d| d.insert_temp(root.with("t"), now));
        }
        let since = now - ctx.data(|d| d.get_temp::<f64>(root.with("t"))).unwrap_or(now);
        // steady while typing, then a slow breath
        let a = if since < 0.6 { 1.0 } else { 0.55 + 0.45 * ((since - 0.6) * std::f64::consts::PI / 0.7).cos() as f32 };
        let w = (size * 0.075).clamp(1.2, 3.0);
        ui.painter_at(pv.clip).line_segment([pos2(x, base.y - size * 0.82), pos2(x, base.y + size * 0.14)], Stroke::new(w, ink.gamma_multiply(a.clamp(0.1, 1.0))));
        ctx.request_repaint_after(Duration::from_millis(33));
    }

    /// Is the writing quiet: a text typed in and the pointer still since?
    /// (The toolbar and the balloon stay away then.)
    pub fn quiet(&self, b: &Book) -> bool {
        b.editing.as_ref().is_some_and(|e| e.typed.is_some_and(|t| t > self.seen) || (e.typed.is_none() && e.new))
    }
}

/// The selected text, when there is a selection.
fn selected(b: &Book) -> Option<String> {
    let e = b.editing.as_ref()?;
    let (a, z) = e.range();
    (a < z).then(|| e.buf.chars().skip(a).take(z - a).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_lines() {
        let t: Vec<char> = "um dois, três\nquatro".chars().collect();
        assert_eq!(word_left(&t, 7), 3);
        assert_eq!(word_right(&t, 3), 7);
        assert_eq!(word_at(&t, 4), (3, 7));
        assert_eq!(word_at(&t, 2), (2, 3));
        assert_eq!(line_at(&t, 16), (14, 20));
        assert_eq!(line_at(&t, 3), (0, 13));
        assert_eq!(byte("três", 3), 4);
    }
}
