//! Where a page's texts go, row by row: the page's typesetter.
//!
//! A text is not a box: it is rows of letters sitting on the ruling. Each
//! row is written in the room its line has at that height, from where the
//! row starts to whatever comes next on that line (another text's letters,
//! a picture glued there) or the end of the text's measure. So:
//!
//! - texts are set in reading order (by the line they were put on, then
//!   left to right), and what comes first has the line: a text whose first
//!   letters would fall under a row of one set before goes down a line, and
//!   what that pushes goes down too, as it is set after;
//! - a row with no room where it would start (a picture across the column)
//!   is written on the next line that has room: text goes round pictures,
//!   which stay where they were glued; beside a picture, rows are shorter;
//! - a text set first still leaves room for one written later beside it
//!   (begun on a line it already had a row on): its rows stop before the
//!   later text's own rows on the same lines (when a real column is left),
//!   so writing beside something never runs into it; a later text begun
//!   under it is in its way instead, and goes down when it grows;
//! - a text pushed down remembers where it was put (`home`) and goes back
//!   up when there is room again.
//!
//! How many letters fit in so much room is the caller's to say: a
//! [`Measure`] lays the text out in its font. Setting a page again with the
//! last setting at hand ([`set_page`]) goes over it until nothing moves.

use super::{Item, Kind, Layout};
use std::collections::HashMap;

/// Room kept between a text and what is beside it (mm).
pub const GAP: f64 = 3.0;
/// Room kept over and under a picture (mm): letters do not touch it.
pub const GAP_Y: f64 = 1.2;
/// The narrowest room a row is written in (mm).
pub const MIN_ROW: f64 = 6.0;
/// A text set first stops before a later one beside it only when this much
/// room is left to it; nearer than that, the later one is in its way (under
/// it) and goes down.
pub const MIN_COL: f64 = 15.0;

/// A row as the caller laid it out: where it ends (exclusive, in the
/// text's display chars), whether a line break ends it (the break is the
/// char at `end`), and how wide its letters are (mm, without the spaces
/// after the last one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub end: usize,
    pub newline: bool,
    pub width: f64,
}

/// Lays out the texts being set.
pub trait Measure {
    /// How many display chars text `id` has.
    fn len(&mut self, id: &str) -> usize;
    /// The row of text `id` that starts at char `from`, in `room` mm.
    fn row(&mut self, id: &str, from: usize, room: f64) -> Fit;
}

/// One row of a text: on the line `base` (its baseline, mm), from `x`, in
/// `room` mm; chars `from..to` of the text.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub base: f64,
    pub x: f64,
    pub room: f64,
    pub from: usize,
    pub to: usize,
    /// A line break follows (the char at `to`).
    pub newline: bool,
    pub width: f64,
    /// It starts where the text's rows start, not moved along by something
    /// in the way.
    pub natural: bool,
}

/// A text set on its page.
#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    pub id: String,
    pub rows: Vec<Row>,
    /// The box round it: its top (the first row's, or the source's row on
    /// top of an excerpt) and its height.
    pub top: f64,
    pub height: f64,
    /// How many chars are on the page (the rest goes on in the next box).
    pub placed: usize,
    pub len: usize,
    /// Not even its first row has room on the page.
    pub off: bool,
    /// The height of one of its rows (mm).
    pub row_h: f64,
}

impl Set {
    /// More text than fits on the page.
    pub fn overflows(&self) -> bool {
        !self.off && self.placed < self.len
    }

    /// The row char `c` is on: a char at a soft break belongs to the row it
    /// starts; the end of the text, to the last row.
    pub fn row_of(&self, c: usize) -> usize {
        self.rows.iter().position(|r| c >= r.from && (c < r.to || (c == r.to && r.newline))).unwrap_or_else(|| {
            self.rows.iter().rposition(|r| c >= r.from).unwrap_or(0)
        })
    }

    /// The letters, row by row (mm): from the row's start to its last letter.
    pub fn ink(&self) -> Vec<crate::canvas::Rect> {
        self.rows.iter().map(|r| crate::canvas::Rect { x: r.x, y: r.base - self.row_h, w: r.width.max(0.5), h: self.row_h }).collect()
    }
}

/// The texts of a page, set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageSet {
    pub texts: Vec<Set>,
}

impl PageSet {
    pub fn get(&self, id: &str) -> Option<&Set> {
        self.texts.iter().find(|t| t.id == id)
    }
}

/// Does text go round this item? Pictures, scans and notes: yes. Stickers,
/// tape, ink and lines lie over the page and text goes on under them,
/// unless the item says otherwise (`wrap`).
pub fn in_the_way(it: &Item) -> bool {
    match it.wrap.as_deref() {
        Some("nao") | Some("nenhum") => false,
        Some(_) => true,
        None => matches!(it.kind, Kind::Image | Kind::Scan | Kind::Note),
    }
}

/// Is this item set as text (in rows on the ruling)?
pub fn is_set(it: &Item) -> bool {
    it.kind.is_text()
}

/// Where a text is from the reader's point of view: the top it was put at.
pub fn home(it: &Item) -> f64 {
    it.home.unwrap_or(it.y)
}

#[derive(Clone, Copy, Debug)]
struct Band {
    y0: f64,
    y1: f64,
    x0: f64,
    x1: f64,
}

impl Band {
    fn meets(&self, y0: f64, y1: f64) -> bool {
        self.y0 < y1 - 0.3 && self.y1 > y0 + 0.3
    }
}

/// Set the texts of a page. `prev` is the last setting (it says where the
/// texts set later are, for those set first to leave them room); the page
/// is set again until nothing moves. Items in `skip` are left out (they are
/// being carried by the pointer).
pub fn set_page(items: &[Item], lay: &Layout, prev: Option<&PageSet>, m: &mut dyn Measure, skip: &[String]) -> PageSet {
    let mut last = prev.cloned().unwrap_or_default();
    for _ in 0..4 {
        let now = set_once(items, lay, &last, m, skip);
        if now == last {
            return now;
        }
        last = now;
    }
    last
}

fn set_once(items: &[Item], lay: &Layout, prev: &PageSet, m: &mut dyn Measure, skip: &[String]) -> PageSet {
    let s = lay.paper.line();
    let last_base = lay.last_baseline() + 0.01;
    let fixed: Vec<(&str, Band)> = items.iter().filter(|i| !skip.contains(&i.id) && in_the_way(i)).map(|i| {
        let b = i.bbox();
        (i.id.as_str(), Band { y0: b.y - GAP_Y, y1: b.y + b.h + GAP_Y, x0: b.x, x1: b.x + b.w })
    }).collect();
    let mut order: Vec<&Item> = items.iter().filter(|i| is_set(i) && !skip.contains(&i.id)).collect();
    let line_of = |i: &Item| ((home(i) + 0.3) / s).floor();
    order.sort_by(|a, b| line_of(a).total_cmp(&line_of(b)).then(a.x.total_cmp(&b.x)).then(a.id.cmp(&b.id)));
    let rank: HashMap<&str, usize> = order.iter().enumerate().map(|(k, i)| (i.id.as_str(), k)).collect();
    let mut hard: Vec<Band> = Vec::new();
    let mut out = PageSet::default();
    for (k, it) in order.iter().enumerate() {
        let rh = s * it.style.lines();
        let rigid = it.kind == Kind::Excerpt;
        let head = if rigid && !super::excerpt_parts(it.content()).title.is_empty() { 1.0 } else { 0.0 };
        let end = it.x + it.w.max(MIN_ROW);
        let start0 = if rigid { it.x } else { it.x + it.style.indent.clamp(0.0, (it.w - MIN_ROW).max(0.0)) };
        let len = m.len(&it.id);
        let top0 = home(it);
        // a picture the text was written on is not in its way
        let probe = (start0 + 0.5, top0 + rh * (0.5 + head));
        let fixed_here: Vec<Band> = fixed.iter().filter(|(id, b)| *id != it.id && !(probe.0 > b.x0 && probe.0 < b.x1 && probe.1 > b.y0 && probe.1 < b.y1)).map(|(_, b)| *b).collect();
        // the rows of texts set after this one and begun beside it (on a line
        // it had a row on), as they were last set
        let mine: Vec<f64> = prev.get(&it.id).map(|t| t.rows.iter().map(|r| r.base).collect()).unwrap_or_default();
        let beside = |t: &Set| t.rows.first().is_some_and(|r0| mine.iter().any(|b| (b - r0.base).abs() < 0.01));
        let soft: Vec<Band> = prev.texts.iter().filter(|t| rank.get(t.id.as_str()).is_some_and(|r| *r > k) && beside(t)).flat_map(|t| {
            t.rows.iter().filter(|r| r.natural).map(|r| Band { y0: r.base - t.row_h, y1: r.base, x0: r.x, x1: r.x + r.width.max(0.5) }).collect::<Vec<_>>()
        }).collect();
        let try_at = |base: f64, force: bool, m: &mut dyn Measure| -> Option<(Vec<Row>, usize)> {
            let mut rows: Vec<Row> = Vec::new();
            let mut from = 0;
            let mut b = base;
            let mut trailing = false;
            while rows.is_empty() || from < len || trailing {
                if b > last_base {
                    break;
                }
                let (y0, y1) = (b - rh, b);
                let first = rows.is_empty();
                let natural = if first { start0 } else { it.x };
                let blocks: Vec<(f64, f64)> = fixed_here.iter().chain(hard.iter()).filter(|o| o.meets(y0, y1)).map(|o| (o.x0, o.x1)).collect();
                let start = if first || rigid {
                    let blocked = blocks.iter().any(|&(a, z)| natural > a - 0.5 && natural < z + 0.5);
                    if blocked && !(first && force) {
                        return None;
                    }
                    natural
                } else {
                    // the first free place along the line
                    let mut x = natural;
                    while let Some(&(_, z)) = blocks.iter().find(|&&(a, z)| x > a - GAP && x < z + GAP - 0.01) {
                        x = z + GAP;
                    }
                    x
                };
                let mut right = end;
                if rigid {
                    if blocks.iter().any(|&(a, z)| a < end && z > it.x) && !(first && force) {
                        return None;
                    }
                } else {
                    for &(a, _) in &blocks {
                        if a > start {
                            right = right.min(a - GAP);
                        }
                    }
                    for o in soft.iter().filter(|o| o.meets(y0, y1)) {
                        if o.x0 > start + 0.5 && o.x0 - GAP - start >= MIN_COL {
                            right = right.min(o.x0 - GAP);
                        }
                    }
                }
                let room = right - start;
                if room < MIN_ROW.min(it.w) - 1e-6 {
                    if first || rigid {
                        if !(first && force) {
                            return None;
                        }
                    } else {
                        // no room on this line: on to the next
                        b += s;
                        continue;
                    }
                }
                let mut f = m.row(&it.id, from, room.max(1.0));
                if f.end < from || (f.end == from && !f.newline && from < len) {
                    f.end = (from + 1).min(len);
                }
                f.end = f.end.min(len);
                rows.push(Row { base: b, x: start, room: room.max(1.0), from, to: f.end, newline: f.newline, width: f.width, natural: (start - natural).abs() < 0.01 });
                from = (f.end + usize::from(f.newline)).min(len);
                trailing = f.newline && from >= len;
                b += rh;
            }
            Some((rows, from))
        };
        let mut base = top0 + rh * (1.0 + head);
        let mut got = None;
        while base <= last_base {
            if let Some(r) = try_at(base, false, m) {
                got = Some(r);
                break;
            }
            if it.locked {
                got = try_at(base, true, m);
                break;
            }
            base += s;
        }
        let set = match got {
            Some((rows, placed)) if !rows.is_empty() => {
                let top = rows[0].base - rh * (1.0 + head);
                let height = rows.last().map(|r| r.base).unwrap_or(top + rh) - top;
                for r in &rows {
                    let (x0, x1) = if rigid { (it.x, end) } else { (r.x, r.x + r.width.max(0.5)) };
                    hard.push(Band { y0: r.base - rh, y1: r.base, x0, x1 });
                }
                if head > 0.0 {
                    hard.push(Band { y0: top, y1: top + rh, x0: it.x, x1: end });
                }
                Set { id: it.id.clone(), rows, top, height, placed, len, off: false, row_h: rh }
            }
            _ => Set { id: it.id.clone(), rows: Vec::new(), top: top0, height: it.h, placed: 0, len, off: true, row_h: rh },
        };
        out.texts.push(set);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{Columns, PageSize, Paper, Ruling};

    /// Letters 2 mm wide; rows break after a space, or anywhere when a
    /// word is longer than the row.
    struct Mono(HashMap<String, Vec<char>>);

    impl Measure for Mono {
        fn len(&mut self, id: &str) -> usize {
            self.0[id].len()
        }
        fn row(&mut self, id: &str, from: usize, room: f64) -> Fit {
            let t = &self.0[id];
            let fit = ((room / 2.0).floor() as usize).max(1);
            let para = t[from..].iter().position(|c| *c == '\n').map(|p| from + p).unwrap_or(t.len());
            let (end, newline) = if para - from <= fit {
                (para, para < t.len())
            } else {
                let cut = (from..from + fit).rev().find(|&i| t[i] == ' ').map(|i| i + 1).unwrap_or(from + fit);
                (cut, false)
            };
            let ink = t[from..end].iter().rposition(|c| *c != ' ').map(|p| p + 1).unwrap_or(0);
            Fit { end, newline, width: ink as f64 * 2.0 }
        }
    }

    fn lay() -> Layout {
        Layout::new(&PageSize::parse("A5"), Paper { ruling: Ruling::Lined, spacing: 6.0 }, Columns::None, true)
    }

    fn text(id: &str, x: f64, line: usize, w: f64, lay: &Layout) -> Item {
        let s = lay.paper.line();
        let mut it = Item::new(Kind::Text, x, lay.baselines[line] - s, w, s);
        it.id = id.into();
        it
    }

    thread_local!(static LAST: std::cell::RefCell<Option<PageSet>> = const { std::cell::RefCell::new(None) });

    /// Set the page as the app does, frame after frame: set it, put the
    /// boxes where the setting says, again (the last setting kept).
    fn run(items: &mut [Item], words: &[(&str, &str)], lay: &Layout) -> PageSet {
        let mut m = Mono(words.iter().map(|(i, t)| (i.to_string(), t.chars().collect())).collect());
        let mut prev: Option<PageSet> = LAST.with(|l| l.borrow().clone());
        for _ in 0..3 {
            let set = set_page(items, lay, prev.as_ref(), &mut m, &[]);
            for t in &set.texts {
                let it = items.iter_mut().find(|i| i.id == t.id).unwrap();
                if (it.y - t.top).abs() > 0.01 && it.home.is_none() {
                    it.home = Some(it.y);
                }
                it.y = t.top;
                it.h = t.height;
                if it.home.is_some_and(|h| (h - it.y).abs() < 0.01) {
                    it.home = None;
                }
            }
            prev = Some(set);
        }
        LAST.with(|l| *l.borrow_mut() = prev.clone());
        prev.unwrap()
    }

    fn line(set: &PageSet, id: &str, lay: &Layout) -> usize {
        let b = set.get(id).unwrap().rows[0].base;
        lay.baselines.iter().position(|l| (l - b).abs() < 0.01).unwrap()
    }

    #[test]
    fn a_text_growing_pushes_the_one_under_it_and_lets_it_back() {
        let lay = lay();
        let mut items = vec![text("a", 10.0, 0, 60.0, &lay), text("b", 10.0, 2, 60.0, &lay)];
        let long = "um dois tres quatro cinco seis sete oito nove dez onze doze treze";
        let set = run(&mut items, &[("a", long), ("b", "embaixo")], &lay);
        let rows_a = set.get("a").unwrap().rows.len();
        assert!(rows_a >= 3, "{rows_a}");
        assert_eq!(line(&set, "b", &lay), rows_a, "b goes just under a");
        assert!(items[1].home.is_some());
        // a shrinks: b goes back where it was put
        let set = run(&mut items, &[("a", "um"), ("b", "embaixo")], &lay);
        assert_eq!(line(&set, "b", &lay), 2);
        assert!(items[1].home.is_none());
    }

    #[test]
    fn a_text_beside_is_not_pushed() {
        // the video's case: a short text that wraps to the margin (its box
        // the whole column) and a narrow one growing down at the right
        let lay = lay();
        let mut c = text("c", 9.0, 3, 130.0, &lay);
        c.style.indent = 30.0;
        let b = text("b", 100.0, 0, 39.0, &lay);
        let mut items = vec![c, b];
        let set = run(&mut items, &[("c", "e assim tambem"), ("b", "mas ai quando voce ta digitando nesse aquele mesmo ali do lado desse")], &lay);
        assert_eq!(line(&set, "c", &lay), 3, "c stays on its line");
        assert!(set.get("b").unwrap().rows.len() >= 4, "{:?}", set.get("b"));
        // and c, typed on, stops before b's letters on that line
        let set = run(&mut items, &[("c", "e assim tambem e mais e mais e mais e mais"), ("b", "mas ai quando voce ta digitando nesse aquele mesmo ali do lado desse")], &lay);
        let c = set.get("c").unwrap();
        assert!(c.rows[0].x + c.rows[0].width <= 100.0 - GAP + 0.01, "{:?}", c.rows[0]);
        assert_eq!(line(&set, "c", &lay), 3);
    }

    #[test]
    fn a_text_begun_under_another_is_pushed_even_when_indented() {
        // "e assim tambem", begun mid-line two lines under a short text: when
        // that text grows down to its line, it goes down (it is not beside)
        let lay = lay();
        let a = text("a", 9.0, 0, 130.0, &lay);
        let mut c = text("c", 9.0, 2, 130.0, &lay);
        c.style.indent = 25.0;
        let mut items = vec![a, c];
        run(&mut items, &[("a", "tum como"), ("c", "e assim tambem")], &lay);
        let long = "tum como digitar assim e mais um pouco de texto que cresce e cresce ainda mais para empurrar quem esta embaixo dele na coluna e passa da terceira linha";
        let set = run(&mut items, &[("a", long), ("c", "e assim tambem")], &lay);
        let rows_a = set.get("a").unwrap().rows.len();
        assert!(rows_a >= 3);
        assert_eq!(line(&set, "c", &lay), rows_a);
        // and back when it is short again
        let set = run(&mut items, &[("a", "tum como"), ("c", "e assim tambem")], &lay);
        assert_eq!(line(&set, "c", &lay), 2);
    }

    #[test]
    fn a_text_set_first_leaves_room_for_one_written_beside_it_later() {
        let lay = lay();
        // d written on the paper beside a's short first line; then a grows
        let mut items = vec![text("a", 10.0, 0, 120.0, &lay), text("d", 80.0, 0, 50.0, &lay)];
        run(&mut items, &[("a", "tum como"), ("d", "do lado")], &lay);
        let set = run(&mut items, &[("a", "tum como digitar assim e mais um pouco de texto aqui"), ("d", "do lado")], &lay);
        let a = set.get("a").unwrap();
        assert!(a.rows[0].x + a.rows[0].width <= 80.0 - GAP + 0.01);
        assert_eq!(line(&set, "d", &lay), 0, "d stays where it was written");
    }

    #[test]
    fn text_goes_round_a_picture_and_the_picture_stays() {
        let lay = lay();
        let s = lay.paper.line();
        let a = text("a", 10.0, 0, 120.0, &lay);
        // a picture across the column, two lines down, two lines tall
        let mut pic = Item::new(Kind::Image, 5.0, lay.baselines[2] - s, 140.0, 2.0 * s - 0.5);
        pic.id = "pic".into();
        let mut items = vec![a, pic.clone()];
        let long = "um dois tres quatro cinco seis sete oito nove dez onze doze treze quatorze quinze dezesseis dezessete dezoito dezenove vinte vinte e um vinte e dois";
        let set = run(&mut items, &[("a", long)], &lay);
        let a = set.get("a").unwrap();
        let bases: Vec<f64> = a.rows.iter().map(|r| r.base).collect();
        assert!(!bases.iter().any(|b| (b - lay.baselines[2]).abs() < 0.01 || (b - lay.baselines[3]).abs() < 0.01), "{bases:?}");
        assert_eq!(items[1], pic);
        // beside a picture on the right, rows are shorter
        let mut pic2 = Item::new(Kind::Image, 80.0, lay.baselines[2] - s, 50.0, 3.0 * s);
        pic2.id = "pic2".into();
        let mut items = vec![text("a", 10.0, 0, 120.0, &lay), pic2];
        let set = run(&mut items, &[("a", long)], &lay);
        let a = set.get("a").unwrap();
        assert!(a.rows[2].x + a.rows[2].room <= 80.0 - GAP + 0.01);
        // (a row whose letters would touch it is shorter too)
        assert!(a.rows[1].x + a.rows[1].room <= 80.0 - GAP + 0.01);
        assert!(a.rows[0].room > 100.0);
    }

    #[test]
    fn what_does_not_fit_overflows_and_what_has_no_room_is_off() {
        let lay = lay();
        let n = lay.baselines.len();
        let long: String = "palavra ".repeat(400);
        let mut items = vec![text("a", 10.0, n - 3, 120.0, &lay)];
        let set = run(&mut items, &[("a", &long)], &lay);
        let a = set.get("a").unwrap();
        assert!(a.overflows());
        assert_eq!(a.rows.len(), 3);
        // a text pushed below the last line
        let mut items = vec![text("a", 10.0, n - 2, 120.0, &lay), text("b", 10.0, n - 1, 120.0, &lay)];
        let set = run(&mut items, &[("a", "um dois tres quatro cinco seis sete oito nove dez onze doze treze quatorze quinze dezesseis dezessete dezoito"), ("b", "fim")], &lay);
        assert!(set.get("b").unwrap().off);
    }

    #[test]
    fn line_breaks_and_an_empty_text_have_rows() {
        let lay = lay();
        let mut items = vec![text("a", 10.0, 0, 120.0, &lay), text("e", 10.0, 6, 120.0, &lay)];
        let set = run(&mut items, &[("a", "um\n\ndois\n"), ("e", "")], &lay);
        let a = set.get("a").unwrap();
        assert_eq!(a.rows.len(), 4);
        assert_eq!((a.rows[0].from, a.rows[0].to, a.rows[0].newline), (0, 2, true));
        assert_eq!((a.rows[1].from, a.rows[1].to), (3, 3));
        assert_eq!(a.rows[3].from, 9);
        assert_eq!(a.row_of(9), 3);
        assert_eq!(a.row_of(2), 0);
        assert_eq!(set.get("e").unwrap().rows.len(), 1);
    }

    #[test]
    fn an_excerpt_moves_as_one_block() {
        let lay = lay();
        let a = text("a", 10.0, 0, 120.0, &lay);
        let mut ex = text("x", 10.0, 2, 120.0, &lay);
        ex.kind = Kind::Excerpt;
        ex.set_content("passagem");
        let mut items = vec![a, ex];
        let long = "um dois tres quatro cinco seis sete oito nove dez onze doze treze quatorze quinze dezesseis dezessete dezoito dezenove vinte vinte e um vinte e dois";
        let set = run(&mut items, &[("a", long), ("x", "uma passagem")], &lay);
        let rows_a = set.get("a").unwrap().rows.len();
        assert_eq!(line(&set, "x", &lay), rows_a);
    }
}
