//! Sequence diagrams: participants side by side with their lifelines, the
//! messages between them from top to bottom, notes, and the frames of
//! loops and alternatives (`loop`, `alt`/`else`, `opt`, `par`/`and`…).

use super::{body, Anchor, Drawing, Head, Ink, Prim};
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Clone, Debug)]
pub struct Participant {
    pub id: String,
    pub label: String,
    pub actor: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Message { from: usize, to: usize, text: String, dashed: bool, head: Head },
    /// A note beside one participant (`left`: on its left) or over one or two.
    Note { a: usize, b: usize, text: String, side: Side },
    /// A frame opens (`loop`, `alt`…), changes part (`else`, `and`) or ends.
    Open { kind: String, label: String },
    Part { label: String },
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Over,
}

#[derive(Clone, Debug)]
pub struct Sequence {
    pub title: String,
    pub participants: Vec<Participant>,
    pub items: Vec<Item>,
    pub autonumber: bool,
}

static MESSAGE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(.+?)\s*(-->>|->>|-->|->|--x|-x|--\)|-\))\s*[+-]?\s*(.+?)\s*:\s*(.*)$").unwrap());
static NOTE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^note\s+(left of|right of|over)\s+([^:]+?)\s*:\s*(.*)$").unwrap());
const BLOCKS: [&str; 6] = ["loop", "alt", "opt", "par", "critical", "break"];

fn text(s: &str) -> String {
    s.trim().replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n")
}

pub fn parse(src: &str) -> Option<Sequence> {
    let mut seq = Sequence { title: String::new(), participants: Vec::new(), items: Vec::new(), autonumber: false };
    fn get(seq: &mut Sequence, id: &str) -> usize {
        let id = id.trim();
        match seq.participants.iter().position(|p| p.id == id) {
            Some(i) => i,
            None => {
                seq.participants.push(Participant { id: id.to_string(), label: id.to_string(), actor: false });
                seq.participants.len() - 1
            }
        }
    }
    for l in body(src) {
        let first = l.split_whitespace().next().unwrap_or("");
        if first == "participant" || first == "actor" {
            let rest = l[first.len()..].trim();
            let (id, label) = match rest.split_once(" as ") {
                Some((id, label)) => (id.trim(), text(label)),
                None => (rest, rest.to_string()),
            };
            let i = get(&mut seq, id);
            seq.participants[i].label = label;
            seq.participants[i].actor = first == "actor";
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            seq.title = t.trim_start_matches(':').trim().to_string();
            continue;
        }
        if first == "autonumber" {
            seq.autonumber = true;
            continue;
        }
        if ["activate", "deactivate", "box", "links", "link", "properties", "details", "create", "destroy"].contains(&first) {
            continue;
        }
        if BLOCKS.contains(&first) || first == "rect" {
            seq.items.push(Item::Open { kind: first.to_string(), label: if first == "rect" { String::new() } else { text(&l[first.len()..]) } });
            continue;
        }
        if first == "else" || first == "and" || first == "option" {
            seq.items.push(Item::Part { label: text(&l[first.len()..]) });
            continue;
        }
        if l == "end" {
            seq.items.push(Item::End);
            continue;
        }
        if let Some(c) = NOTE.captures(l) {
            let (a, b) = match c[2].split_once(',') {
                Some((a, b)) => (get(&mut seq, a), get(&mut seq, b)),
                None => {
                    let a = get(&mut seq, &c[2]);
                    (a, a)
                }
            };
            let side = match c[1].to_lowercase().as_str() {
                "left of" => Side::Left,
                "right of" => Side::Right,
                _ => Side::Over,
            };
            seq.items.push(Item::Note { a, b, text: text(&c[3]), side });
            continue;
        }
        if let Some(c) = MESSAGE.captures(l) {
            let (from, to) = (get(&mut seq, &c[1]), get(&mut seq, c[3].trim_start_matches(['+', '-'])));
            let op = &c[2];
            let head = if op.ends_with("x") {
                Head::Cross
            } else if op.ends_with(')') {
                Head::Open
            } else if op.ends_with(">>") {
                Head::Arrow
            } else {
                Head::None
            };
            seq.items.push(Item::Message { from, to, text: text(&c[4]), dashed: op.starts_with("--"), head });
        }
    }
    (!seq.participants.is_empty()).then_some(seq)
}

/// Height of a block of text (it may have line breaks).
fn text_size(measure: &dyn Fn(&str) -> (f32, f32), t: &str) -> (f32, f32) {
    let mut w: f32 = 0.0;
    let mut h = 0.0;
    for line in t.split('\n') {
        let (lw, lh) = measure(line);
        w = w.max(lw);
        h += lh;
    }
    (w, h)
}

pub fn draw(seq: &Sequence, measure: &dyn Fn(&str) -> (f32, f32)) -> Drawing {
    let n = seq.participants.len();
    let (_, line_h) = measure("Ag");
    let pad = 12.0;
    let box_w: Vec<f32> = seq.participants.iter().map(|p| (text_size(measure, &p.label).0 + 2.0 * pad + 8.0).max(90.0)).collect();
    let mut box_h = line_h * seq.participants.iter().map(|p| p.label.split('\n').count()).max().unwrap_or(1) as f32 + 2.0 * pad;
    if seq.participants.iter().any(|p| p.actor) {
        // room for a stick figure over the name
        box_h = box_h.max(line_h + 44.0);
    }
    // the room between neighbouring lifelines: their boxes, and the labels
    // of the messages and notes between them
    let mut gap: Vec<f32> = (0..n.saturating_sub(1)).map(|i| box_w[i] / 2.0 + box_w[i + 1] / 2.0 + 30.0).collect();
    let mut number = 0;
    for it in &seq.items {
        let (a, b, w) = match it {
            Item::Message { from, to, text, .. } => {
                number += 1;
                let label = if seq.autonumber { format!("{number}. {text}") } else { text.clone() };
                let w = text_size(measure, &label).0 * 0.9;
                if from == to {
                    (*from, *from + 1, w + 50.0)
                } else {
                    ((*from).min(*to), (*from).max(*to), w + 40.0)
                }
            }
            Item::Note { a, b, text, side } => {
                let w = text_size(measure, text).0 * 0.9 + 2.0 * pad;
                match side {
                    Side::Right => (*a, *a + 1, w + 20.0 + box_w[*a] / 2.0),
                    Side::Left if *a > 0 => (*a - 1, *a, w + 20.0 + box_w[*a] / 2.0),
                    _ => ((*a).min(*b), (*a).max(*b), w),
                }
            }
            _ => continue,
        };
        if b >= n || a == b {
            continue;
        }
        let have: f32 = gap[a..b].iter().sum();
        if have < w {
            gap[b - 1] += w - have;
        }
    }
    let left_room = seq.items.iter().filter_map(|it| match it {
        Item::Note { a: 0, side: Side::Left, text, .. } => Some(text_size(measure, text).0 * 0.9 + 2.0 * pad + 20.0),
        _ => None,
    });
    let mut x0 = box_w.first().copied().unwrap_or(90.0) / 2.0 + 10.0;
    for w in left_room {
        x0 = x0.max(w + box_w[0] / 2.0 + 10.0);
    }
    let mut xs = vec![x0];
    for g in &gap {
        xs.push(xs.last().unwrap() + g);
    }
    let right_room = seq.items.iter().filter_map(|it| match it {
        Item::Message { from, to, text, .. } if from == to && *from + 1 == n => Some(text_size(measure, text).0 * 0.9 + 60.0),
        Item::Note { a, side: Side::Right, text, .. } if *a + 1 == n => Some(text_size(measure, text).0 * 0.9 + 2.0 * pad + 20.0),
        _ => None,
    });
    let mut width = xs.last().unwrap() + box_w.last().copied().unwrap_or(90.0) / 2.0 + 10.0;
    for w in right_room {
        width = width.max(xs[n - 1] + w);
    }
    let mut d = Drawing::default();
    let title_h = if seq.title.is_empty() { 0.0 } else { line_h * 1.3 + 10.0 };
    if !seq.title.is_empty() {
        d.prims.push(Prim::Text { x: width / 2.0, y: line_h * 0.65, text: seq.title.clone(), anchor: Anchor::Center, ink: Ink::Text, bold: true, small: false });
    }
    let top = title_h;
    let mut y = top + box_h + 20.0;
    let mut body: Vec<Prim> = Vec::new();
    // open frames: kind, label, top, and the parts' dividers
    let mut frames: Vec<(String, String, f32, Vec<(f32, String)>)> = Vec::new();
    let mut frame_prims: Vec<Prim> = Vec::new();
    let mut number = 0;
    let depth_pad = |k: usize| 10.0 + 8.0 * k as f32;
    for it in &seq.items {
        match it {
            Item::Message { from, to, text, dashed, head } => {
                number += 1;
                let label = if seq.autonumber { format!("{number}. {text}") } else { text.clone() };
                let (_, th) = text_size(measure, &label);
                let th = th * 0.9;
                let (xa, xb) = (xs[*from], xs[*to]);
                if from == to {
                    let ly = y + th / 2.0;
                    body.push(Prim::Text { x: xa + 10.0, y: ly, text: label, anchor: Anchor::Left, ink: Ink::Text, bold: false, small: true });
                    let y1 = y + th + 6.0;
                    let y2 = y1 + 22.0;
                    body.push(Prim::Line { pts: vec![(xa, y1), (xa + 36.0, y1), (xa + 36.0, y2), (xa + 3.0, y2)], ink: Ink::Line, width: 1.3, dashed: *dashed });
                    if *head != Head::None {
                        body.push(Prim::Head { at: (xa, y2), dir: (-1.0, 0.0), kind: *head, ink: Ink::Line });
                    }
                    y = y2 + 18.0;
                } else {
                    body.push(Prim::Text { x: (xa + xb) / 2.0, y: y + th / 2.0, text: label, anchor: Anchor::Center, ink: Ink::Text, bold: false, small: true });
                    let ly = y + th + 6.0;
                    let dir = if xb > xa { 1.0 } else { -1.0 };
                    body.push(Prim::Line { pts: vec![(xa, ly), (xb - dir * 2.0, ly)], ink: Ink::Line, width: 1.3, dashed: *dashed });
                    if *head != Head::None {
                        body.push(Prim::Head { at: (xb, ly), dir: (dir, 0.0), kind: *head, ink: Ink::Line });
                    }
                    y = ly + 20.0;
                }
            }
            Item::Note { a, b, text, side } => {
                let (tw, th) = text_size(measure, text);
                let (tw, th) = (tw * 0.9, th * 0.9);
                let w = tw + 2.0 * pad;
                let h = th + 2.0 * pad * 0.7;
                let x = match side {
                    Side::Right => xs[*a] + 10.0,
                    Side::Left => xs[*a] - 10.0 - w,
                    Side::Over => {
                        let (l, r) = (xs[(*a).min(*b)], xs[(*a).max(*b)]);
                        let w2 = w.max(r - l + 40.0);
                        (l + r) / 2.0 - w2 / 2.0
                    }
                };
                let w = if *side == Side::Over { w.max(xs[(*a).max(*b)] - xs[(*a).min(*b)] + 40.0) } else { w };
                body.push(Prim::Rect { x, y, w, h, round: 3.0, fill: Some(Ink::Note), stroke: Some(Ink::Faint), dashed: false });
                let mut ty = y + pad * 0.7;
                for line in text.split('\n') {
                    let lh = measure(line).1 * 0.9;
                    body.push(Prim::Text { x: x + w / 2.0, y: ty + lh / 2.0, text: line.to_string(), anchor: Anchor::Center, ink: Ink::Text, bold: false, small: true });
                    ty += lh;
                }
                y += h + 14.0;
            }
            Item::Open { kind, label } => {
                frames.push((kind.clone(), label.clone(), y, Vec::new()));
                y += line_h + 16.0;
            }
            Item::Part { label } => {
                if let Some(f) = frames.last_mut() {
                    f.3.push((y, label.clone()));
                    y += line_h + 12.0;
                }
            }
            Item::End => {
                let Some((kind, label, ft, parts)) = frames.pop() else { continue };
                let k = depth_pad(frames.len());
                let (fx, fw) = (xs[0] - box_w[0] / 2.0 + k - 10.0, xs[n - 1] + box_w[n - 1] / 2.0 - k + 10.0);
                let (fx, fw) = (fx.max(2.0), fw.min(width - 2.0) - fx.max(2.0));
                let fb = y + 2.0;
                if kind == "rect" {
                    frame_prims.push(Prim::Rect { x: fx, y: ft, w: fw, h: fb - ft, round: 4.0, fill: Some(Ink::Back), stroke: None, dashed: false });
                } else {
                    frame_prims.push(Prim::Rect { x: fx, y: ft, w: fw, h: fb - ft, round: 2.0, fill: None, stroke: Some(Ink::Faint), dashed: false });
                    let (kw, _) = measure(&kind);
                    let tag_w = kw * 0.85 + 18.0;
                    let tag_h = line_h * 0.9 + 6.0;
                    frame_prims.push(Prim::Poly { pts: vec![(fx, ft), (fx + tag_w, ft), (fx + tag_w, ft + tag_h - 6.0), (fx + tag_w - 6.0, ft + tag_h), (fx, ft + tag_h)], fill: Some(Ink::Faint), stroke: None });
                    frame_prims.push(Prim::Text { x: fx + 8.0, y: ft + tag_h / 2.0, text: kind.clone(), anchor: Anchor::Left, ink: Ink::Text, bold: true, small: true });
                    if !label.is_empty() {
                        frame_prims.push(Prim::Text { x: fx + tag_w + 10.0, y: ft + tag_h / 2.0, text: format!("[{label}]"), anchor: Anchor::Left, ink: Ink::Muted, bold: false, small: true });
                    }
                    for (py, pl) in parts {
                        frame_prims.push(Prim::Line { pts: vec![(fx, py), (fx + fw, py)], ink: Ink::Faint, width: 1.0, dashed: true });
                        if !pl.is_empty() {
                            frame_prims.push(Prim::Text { x: fx + fw / 2.0, y: py + line_h * 0.5 + 4.0, text: format!("[{pl}]"), anchor: Anchor::Center, ink: Ink::Muted, bold: false, small: true });
                        }
                    }
                }
                y += 12.0;
            }
        }
    }
    let bottom = y + 6.0;
    // frames under everything; lifelines; the participants above and below
    d.prims.extend(frame_prims);
    for (i, p) in seq.participants.iter().enumerate() {
        d.prims.push(Prim::Line { pts: vec![(xs[i], top + box_h), (xs[i], bottom)], ink: Ink::Faint, width: 1.0, dashed: true });
        for by in [top, bottom] {
            participant(&mut d, p, xs[i], by, box_w[i], box_h, measure);
        }
    }
    d.prims.extend(body);
    d.width = width;
    d.height = bottom + box_h + 2.0;
    d
}

fn participant(d: &mut Drawing, p: &Participant, x: f32, y: f32, w: f32, h: f32, measure: &dyn Fn(&str) -> (f32, f32)) {
    if p.actor {
        // a stick figure over the name
        let (_, lh) = measure("Ag");
        let fig = h - lh - 4.0;
        let (cy, s) = (y + fig * 0.18, fig * 0.16);
        d.prims.push(Prim::Circle { x, y: cy, r: s, fill: Some(Ink::Fill), stroke: Some(Ink::Stroke) });
        let body_top = cy + s;
        let hip = y + fig * 0.62;
        d.prims.push(Prim::Line { pts: vec![(x, body_top), (x, hip)], ink: Ink::Stroke, width: 1.3, dashed: false });
        d.prims.push(Prim::Line { pts: vec![(x - fig * 0.25, body_top + fig * 0.12), (x + fig * 0.25, body_top + fig * 0.12)], ink: Ink::Stroke, width: 1.3, dashed: false });
        d.prims.push(Prim::Line { pts: vec![(x - fig * 0.22, y + fig), (x, hip), (x + fig * 0.22, y + fig)], ink: Ink::Stroke, width: 1.3, dashed: false });
        d.prims.push(Prim::Text { x, y: y + fig + 2.0 + lh / 2.0, text: p.label.clone(), anchor: Anchor::Center, ink: Ink::Text, bold: false, small: false });
        return;
    }
    d.prims.push(Prim::Rect { x: x - w / 2.0, y, w, h, round: 4.0, fill: Some(Ink::Fill), stroke: Some(Ink::Stroke), dashed: false });
    let lines: Vec<&str> = p.label.split('\n').collect();
    let lh = measure("Ag").1;
    let mut ty = y + h / 2.0 - lh * lines.len() as f32 / 2.0;
    for l in lines {
        d.prims.push(Prim::Text { x, y: ty + lh / 2.0, text: l.to_string(), anchor: Anchor::Center, ink: Ink::Text, bold: false, small: false });
        ty += lh;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_draws() {
        let src = "sequenceDiagram\n    title Olá\n    autonumber\n    actor A as Alice\n    participant J as John\n    A->>J: Hello John, how are you?\n    loop Every minute\n        J-->>A: Great!\n    end\n    alt doente\n        A->>A: tosse\n    else bem\n        A-xJ: perdido\n    end\n    Note right of J: Rational thoughts\n    Note over A,J: A typical interaction\n    J-)A: async\n";
        let s = parse(src).unwrap();
        assert_eq!(s.participants.len(), 2);
        assert!(s.participants[0].actor);
        assert_eq!(s.participants[1].label, "John");
        assert_eq!(s.title, "Olá");
        assert!(s.autonumber);
        let msgs: Vec<&Item> = s.items.iter().filter(|i| matches!(i, Item::Message { .. })).collect();
        assert_eq!(msgs.len(), 5);
        assert!(matches!(msgs[1], Item::Message { dashed: true, head: Head::Arrow, .. }));
        assert!(matches!(msgs[2], Item::Message { from: 0, to: 0, .. }));
        assert!(matches!(msgs[3], Item::Message { head: Head::Cross, .. }));
        assert!(matches!(msgs[4], Item::Message { head: Head::Open, .. }));
        assert_eq!(s.items.iter().filter(|i| matches!(i, Item::Note { .. })).count(), 2);
        let d = draw(&s, &|t| (t.chars().count() as f32 * 7.0, 16.0));
        assert!(d.width > 200.0 && d.height > 200.0);
        // every mark inside the drawing
        for p in &d.prims {
            if let Prim::Text { x, y, .. } = p {
                assert!(*x >= 0.0 && *x <= d.width && *y >= 0.0 && *y <= d.height, "{p:?}");
            }
        }
    }
}
