//! Where a column's header and cards go, and what is inside a card (title,
//! picture, tags, date). The screen and the exported picture lay out with
//! this, each measuring text with its own fonts.

use super::card::{self, Info, Seg};
use super::{Board, Card, Lane};

pub const HEADER_H: f32 = 44.0;
pub const LANE_PAD: f32 = 8.0;
pub const CARD_GAP: f32 = 8.0;
pub const CARD_PAD: f32 = 10.0;
pub const TITLE_SIZE: f32 = 14.0;
pub const META_SIZE: f32 = 11.5;
pub const CHIP_H: f32 = 20.0;
pub const CHIP_SIZE: f32 = 11.0;
/// Kept free at the right of a card's title for its checkbox.
pub const CHECK_W: f32 = 20.0;
/// The add-a-card row at the bottom of a column.
pub const FOOT_H: f32 = 34.0;

pub type R = (f32, f32, f32, f32);

/// Text measuring: (text, font size, wrap width) → (width, height).
pub trait Measure {
    fn text(&mut self, text: &str, size: f32, wrap: f32) -> (f32, f32);
    /// A picture's size, when known.
    fn picture(&mut self, target: &str) -> Option<(f32, f32)>;
}

#[derive(Clone, Debug)]
pub struct CardBox {
    pub id: String,
    pub rect: R,
    /// The title's text as shown, and its box.
    pub title: String,
    pub title_rect: R,
    pub picture: Option<(String, R)>,
    /// Tags: (tag, box).
    pub chips: Vec<(String, R)>,
    /// Date and time line.
    pub meta: Option<(String, R)>,
    pub info: Info,
}

#[derive(Clone, Debug)]
pub struct LaneBox {
    pub id: String,
    pub rect: R,
    pub cards: Vec<CardBox>,
}

/// The title as shown: links by their name, picture embeds left out (the
/// picture is drawn), note embeds as their name.
pub fn title_of(info: &Info) -> String {
    let mut out = String::new();
    let mut picture_done = false;
    for s in &info.segs {
        match s {
            Seg::Text(t) => out.push_str(t),
            Seg::Link(_, a) | Seg::Url(_, a) => out.push_str(a),
            Seg::Embed(t) if card::is_picture(t) && !picture_done => picture_done = true,
            Seg::Embed(t) => out.push_str(&card::link_target(t).1),
        }
    }
    out.trim().to_string()
}

pub fn triggers(b: &Board) -> (String, String) {
    let s = &b.kept.settings;
    (s.get("date-trigger").and_then(|v| v.as_str()).unwrap_or("@").to_string(), s.get("time-trigger").and_then(|v| v.as_str()).unwrap_or("@@").to_string())
}

pub fn card_box(c: &Card, x: f32, y: f32, w: f32, trig: &(String, String), m: &mut dyn Measure) -> CardBox {
    let info = card::parse(&c.text, &trig.0, &trig.1);
    let inner = w - 2.0 * CARD_PAD;
    let mut cy = y + CARD_PAD;
    let mut picture = None;
    if let Some(p) = info.picture() {
        let (pw, ph) = m.picture(p).unwrap_or((4.0, 3.0));
        let h = (inner * ph / pw.max(1.0)).min(inner * 1.6);
        picture = Some((p.to_string(), (x + CARD_PAD, cy, inner, h)));
        cy += h + 6.0;
    }
    let title = title_of(&info);
    // room on the right for the checkbox
    let tw = inner - CHECK_W;
    let (_, th) = if title.is_empty() { (0.0, 0.0) } else { m.text(&title, TITLE_SIZE, tw) };
    let title_rect = (x + CARD_PAD, cy, tw, th);
    if th > 0.0 {
        cy += th + 4.0;
    }
    let mut chips = Vec::new();
    if !info.tags.is_empty() {
        let mut cx = x + CARD_PAD;
        cy += 2.0;
        for t in &info.tags {
            let (tw, _) = m.text(t, CHIP_SIZE, f32::INFINITY);
            let cw = (tw + 14.0).min(inner);
            if cx + cw > x + CARD_PAD + inner && cx > x + CARD_PAD {
                cx = x + CARD_PAD;
                cy += CHIP_H + 4.0;
            }
            chips.push((t.clone(), (cx, cy, cw, CHIP_H)));
            cx += cw + 4.0;
        }
        cy += CHIP_H + 4.0;
    }
    let mut meta = None;
    let when = match (&info.date, &info.time) {
        (Some(d), Some(t)) => Some(format!("{d} {t}")),
        (Some(d), None) => Some(d.clone()),
        (None, Some(t)) => Some(t.clone()),
        _ => None,
    };
    if let Some(wn) = when {
        let (_, mh) = m.text(&wn, META_SIZE, inner);
        meta = Some((wn, (x + CARD_PAD, cy, inner, mh)));
        cy += mh + 4.0;
    }
    let h = (cy - y + CARD_PAD - 4.0).max(38.0);
    CardBox { id: c.id.clone(), rect: (x, y, w, h), title, title_rect, picture, chips, meta, info }
}

pub fn lane_box(l: &Lane, trig: &(String, String), m: &mut dyn Measure) -> LaneBox {
    let mut y = l.y + HEADER_H;
    let mut cards = Vec::new();
    if !l.collapsed {
        for c in &l.cards {
            let b = card_box(c, l.x + LANE_PAD, y, l.w - 2.0 * LANE_PAD, trig, m);
            y += b.rect.3 + CARD_GAP;
            cards.push(b);
        }
        y += FOOT_H;
    }
    LaneBox { id: l.id.clone(), rect: (l.x, l.y, l.w, (y - l.y).max(HEADER_H)), cards }
}

/// A tag's colours: the board's `tag-colors` setting, else one picked from
/// the tag's name (text, background).
pub fn tag_colors(b: &Board, tag: &str) -> ([u8; 3], [u8; 3]) {
    if let Some(list) = b.kept.settings.get("tag-colors").and_then(|v| v.as_array()) {
        for t in list {
            if t["tagKey"].as_str().is_some_and(|k| k.eq_ignore_ascii_case(tag)) {
                let fg = t["color"].as_str().and_then(super::rough::color);
                let bg = t["backgroundColor"].as_str().and_then(super::rough::color);
                if let (Some(f), Some(bg)) = (fg, bg) {
                    return ([f[0], f[1], f[2]], [bg[0], bg[1], bg[2]]);
                }
            }
        }
    }
    const HUES: [([u8; 3], [u8; 3]); 8] = [
        ([0xd9, 0x4a, 0xd0], [0x5a, 0x2a, 0x56]),
        ([0x3f, 0xb9, 0xa8], [0x1f, 0x4a, 0x45]),
        ([0xe0, 0x4a, 0x4a], [0x5c, 0x26, 0x26]),
        ([0xc8, 0xb4, 0x3c], [0x4f, 0x48, 0x1e]),
        ([0x9b, 0x6b, 0xe8], [0x3d, 0x2e, 0x5e]),
        ([0x3c, 0x8c, 0xe8], [0x1d, 0x3a, 0x5e]),
        ([0xe0, 0x8a, 0x3c], [0x5a, 0x38, 0x1c]),
        ([0x5d, 0xb8, 0x5d], [0x25, 0x4a, 0x25]),
    ];
    let h = tag.to_lowercase().bytes().fold(5381u32, |a, c| a.wrapping_mul(33) ^ c as u32);
    HUES[(h % HUES.len() as u32) as usize]
}
