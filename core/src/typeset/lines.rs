//! Lines of text: styled runs shaped into pieces, paragraphs broken with
//! Knuth and Plass's total-fit algorithm (words may hyphenate), lines set
//! justified, ragged or centred as page items.

use super::fonts::{FontDb, FontId, Shaped};
use super::model::{Run, Style};
use super::{hyph, Glyph, Item, PT_MM};
use std::sync::Arc;

/// The faces a book uses.
#[derive(Clone, Debug)]
pub struct Faces {
    pub body: [FontId; 4],
    pub title: [FontId; 4],
    pub mono: FontId,
    /// Small caps by the fonts' own glyphs.
    pub smcp_body: bool,
    pub smcp_title: bool,
}

impl Faces {
    pub fn pick(&self, st: &Style) -> FontId {
        if st.mono {
            return self.mono;
        }
        let set = if st.title_font { &self.title } else { &self.body };
        set[(st.italic as usize) | ((st.bold as usize) << 1)]
    }
}

/// Typesetting context: fonts and the body size.
pub struct Ctx<'a> {
    pub db: &'a FontDb,
    pub faces: Faces,
    /// Body size in pt.
    pub size: f32,
    pub color: [u8; 3],
    pub paper: [u8; 3],
    pub hyphenate: bool,
    /// Extra OpenType features for everything (e.g. old-style figures).
    pub figures: Option<&'static str>,
}

/// A shaped bit of text in one style.
#[derive(Clone, Debug)]
pub struct Piece {
    pub font: FontId,
    pub size: f32,
    pub color: [u8; 3],
    pub glyphs: Vec<Glyph>,
    pub map: Vec<(u16, String)>,
    pub text: String,
    pub w: f32,
    pub rise: f32,
    pub strike: bool,
}

impl Ctx<'_> {
    pub fn color_of(&self, st: &Style) -> [u8; 3] {
        let c = st.color.unwrap_or(self.color);
        if st.opacity >= 0.999 {
            return c;
        }
        let a = st.opacity.clamp(0.0, 1.0);
        [0, 1, 2].map(|i| (c[i] as f32 * a + self.paper[i] as f32 * (1.0 - a)).round() as u8)
    }

    /// Shape text in a style (size in pt already includes the style's scale).
    pub fn piece(&self, text: &str, st: &Style) -> Piece {
        let mut size = self.size * st.scale;
        let mut rise = 0.0;
        if st.sup {
            size *= 0.68;
            rise = -self.size * 0.33 * PT_MM;
        }
        let font = self.faces.pick(st);
        let has_smcp = if st.title_font { self.faces.smcp_title } else { self.faces.smcp_body };
        let mut feats: Vec<&str> = vec!["kern", "liga"];
        if let Some(f) = self.figures {
            feats.push(f);
        }
        let mut t = if st.upper { text.to_uppercase() } else { text.to_string() };
        if st.smallcaps || st.allcaps_small {
            if has_smcp {
                feats.push("smcp");
                if st.allcaps_small {
                    feats.push("c2sc");
                }
            } else {
                // no small caps in the font: capitals a little smaller
                t = t.to_uppercase();
                size *= 0.8;
            }
        }
        let shaped: Arc<Vec<Shaped>> = self.db.shape(font, &t, &feats);
        let face = self.db.face(font);
        let k = size * PT_MM / face.upem;
        let track = st.track * size * PT_MM;
        let mut pen = 0.0;
        let mut glyphs = Vec::with_capacity(shaped.len());
        let mut map = Vec::with_capacity(shaped.len());
        for (i, g) in shaped.iter().enumerate() {
            glyphs.push(Glyph { id: g.id, x: pen + g.dx as f32 * k, y: rise - g.dy as f32 * k });
            let end = shaped.iter().skip(i + 1).map(|o| o.cluster).find(|c| *c != g.cluster).unwrap_or(t.len() as u32);
            let (a, b) = (g.cluster.min(end) as usize, g.cluster.max(end) as usize);
            map.push((g.id, t.get(a..b).unwrap_or("").to_string()));
            pen += g.adv as f32 * k + track;
        }
        Piece { font, size, color: self.color_of(st), glyphs, map, text: t, w: pen, rise, strike: st.strike }
    }

    pub fn space(&self, st: &Style) -> f32 {
        let p = self.piece(" ", st);
        if p.w > 0.0 {
            p.w
        } else {
            self.size * st.scale * PT_MM * 0.25
        }
    }
}

/// Items of a paragraph for line breaking.
#[derive(Clone, Debug)]
pub enum K {
    Box { w: f32, piece: Piece },
    Glue { w: f32, y: f32, z: f32 },
    Pen { w: f32, p: f32, flagged: bool, hyphen: Option<Piece> },
    /// A forced line break inside the paragraph (`<br>`).
    Break,
}

const INF: f32 = 10_000.0;

/// Runs → boxes, glue and hyphenation points.
pub fn items(cx: &Ctx, runs: &[Run], hyphenate: bool) -> Vec<K> {
    let mut out: Vec<K> = Vec::new();
    for r in runs {
        let mut first = true;
        for (li, line) in r.text.split('\n').enumerate() {
            if li > 0 {
                out.push(K::Break);
            }
            for (wi, word) in line.split([' ', '\t']).enumerate() {
                if wi > 0 || (!first && word.is_empty()) {
                    // a space: glue (several spaces count as one)
                    if !matches!(out.last(), Some(K::Glue { .. }) | None | Some(K::Break)) {
                        let w = cx.space(&r.st);
                        out.push(K::Glue { w, y: w * 0.5, z: w * 0.33 });
                    }
                }
                first = false;
                if word.is_empty() {
                    continue;
                }
                // hyphenation points of the letters' part
                let core: String = word.chars().filter(|c| c.is_alphabetic()).collect();
                let lead = word.chars().take_while(|c| !c.is_alphabetic()).count();
                let pts = if hyphenate && core.chars().count() >= 5 && word.chars().skip(lead).take(core.chars().count()).all(|c| c.is_alphabetic()) { hyph::points(&core) } else { Vec::new() };
                if pts.is_empty() {
                    let piece = cx.piece(word, &r.st);
                    out.push(K::Box { w: piece.w, piece });
                    continue;
                }
                let chars: Vec<char> = word.chars().collect();
                let mut cut: Vec<usize> = pts.iter().map(|p| p + lead).collect();
                cut.push(chars.len());
                let mut from = 0;
                for (k, c) in cut.iter().enumerate() {
                    let part: String = chars[from..*c].iter().collect();
                    let piece = cx.piece(&part, &r.st);
                    out.push(K::Box { w: piece.w, piece });
                    if k + 1 < cut.len() {
                        let hy = cx.piece("-", &r.st);
                        out.push(K::Pen { w: hy.w, p: 50.0, flagged: true, hyphen: Some(hy) });
                    }
                    from = *c;
                }
            }
        }
    }
    while matches!(out.last(), Some(K::Glue { .. })) {
        out.pop();
    }
    out
}

#[derive(Clone, Copy)]
struct Node {
    pos: usize,
    line: usize,
    fit: u8,
    w: f32,
    y: f32,
    z: f32,
    dem: f32,
    prev: Option<usize>,
}

/// Where lines break (item indices of the breaks), by total fit; `width(n)`
/// is the measure of line n. Falls back to first fit when nothing fits.
pub fn breaks(items: &[K], width: &dyn Fn(usize) -> f32, justify: bool) -> Vec<usize> {
    // forced breaks split the paragraph into independent parts
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, it) in items.iter().enumerate() {
        if matches!(it, K::Break) {
            parts.push((start, i));
            start = i + 1;
        }
    }
    parts.push((start, items.len()));
    let mut out = Vec::new();
    let mut line0 = 0;
    for (a, b) in parts {
        let seg = &items[a..b];
        let mut br = if justify { total_fit(seg, &|n| width(line0 + n), 2.5).or_else(|| total_fit(seg, &|n| width(line0 + n), 8.0)) } else { None };
        if br.is_none() {
            br = Some(first_fit(seg, &|n| width(line0 + n)));
        }
        let br = br.unwrap();
        line0 += br.len();
        out.extend(br.into_iter().map(|x| x + a));
    }
    out
}

fn first_fit(items: &[K], width: &dyn Fn(usize) -> f32) -> Vec<usize> {
    let mut out = Vec::new();
    let mut w = 0.0;
    let mut last_ok: Option<usize> = None;
    let mut i = 0;
    let mut line_start = 0;
    while i < items.len() {
        match &items[i] {
            K::Box { w: bw, .. } => {
                w += bw;
                if w > width(out.len()) && last_ok.is_some() {
                    let b = last_ok.take().unwrap();
                    out.push(b);
                    i = b + 1;
                    while i < items.len() && matches!(items[i], K::Glue { .. }) {
                        i += 1;
                    }
                    line_start = i;
                    w = 0.0;
                    continue;
                }
            }
            K::Glue { w: gw, .. } => {
                if i > line_start {
                    last_ok = Some(i);
                }
                w += gw;
            }
            K::Pen { w: pw, .. } => {
                if w + pw <= width(out.len()) {
                    last_ok = Some(i);
                }
            }
            K::Break => {}
        }
        i += 1;
    }
    out.push(items.len());
    out
}

fn total_fit(items: &[K], width: &dyn Fn(usize) -> f32, tol: f32) -> Option<Vec<usize>> {
    let n = items.len();
    if n == 0 {
        return Some(vec![0]);
    }
    let mut nodes: Vec<Node> = vec![Node { pos: 0, line: 0, fit: 1, w: 0.0, y: 0.0, z: 0.0, dem: 0.0, prev: None }];
    let mut active: Vec<usize> = vec![0];
    let (mut tw, mut ty, mut tz) = (0.0f32, 0.0f32, 0.0f32);
    let is_break = |i: usize| -> Option<(f32, f32, bool)> {
        match &items[i] {
            K::Glue { .. } if i > 0 && matches!(items[i - 1], K::Box { .. }) => Some((0.0, 0.0, false)),
            K::Pen { p, w, flagged, .. } if *p < INF => Some((*p, *w, *flagged)),
            _ => None,
        }
    };
    // the last line: a break after everything
    for i in 0..=n {
        let at_end = i == n;
        let cand = if at_end { Some((-INF, 0.0, false)) } else { is_break(i) };
        if let Some((p, pw, flagged)) = cand {
            let mut best: [Option<(f32, usize)>; 4] = [None; 4];
            let mut keep = Vec::with_capacity(active.len());
            for &a in &active {
                let nd = nodes[a];
                let lw = width(nd.line);
                let l = tw - nd.w + pw;
                let r = if l < lw {
                    let y = ty - nd.y + if at_end { INF } else { 0.0 };
                    if y > 0.0 {
                        (lw - l) / y
                    } else {
                        INF
                    }
                } else if l > lw {
                    let z = tz - nd.z;
                    if z > 0.0 {
                        (lw - l) / z
                    } else {
                        -INF
                    }
                } else {
                    0.0
                };
                if r < -1.0 || p <= -INF {
                    // too tight from here on (or forced): drop the node
                    if r >= -1.0 {
                        // still a valid break for the forced end
                        let bad = 100.0 * r.abs().powi(3);
                        let dem = (10.0 + bad).powi(2) + nd.dem;
                        let fit = fitness(r);
                        if best[fit as usize].is_none_or(|b| dem < b.0) {
                            best[fit as usize] = Some((dem, a));
                        }
                    }
                    continue;
                }
                keep.push(a);
                if r <= tol {
                    let bad = 100.0 * r.abs().powi(3);
                    let mut dem = if p >= 0.0 { (10.0 + bad + p).powi(2) } else if p > -INF { (10.0 + bad).powi(2) - p * p } else { (10.0 + bad).powi(2) };
                    if flagged && nd.pos > 0 && matches!(items.get(nd.pos), Some(K::Pen { flagged: true, .. })) {
                        dem += 3000.0;
                    }
                    let fit = fitness(r);
                    if (fit as i32 - nd.fit as i32).abs() > 1 {
                        dem += 3000.0;
                    }
                    dem += nd.dem;
                    if best[fit as usize].is_none_or(|b| dem < b.0) {
                        best[fit as usize] = Some((dem, a));
                    }
                }
            }
            active = keep;
            // totals after the break (skip the glue that follows)
            let (mut aw, mut ay, mut az) = (tw, ty, tz);
            let mut j = i;
            while j < n {
                match &items[j] {
                    K::Glue { w, y, z } => {
                        aw += w;
                        ay += y;
                        az += z;
                    }
                    K::Pen { p, .. } if *p > -INF && j > i => break,
                    K::Box { .. } => break,
                    _ => {}
                }
                j += 1;
            }
            for (fit, b) in best.iter().enumerate() {
                if let Some((dem, a)) = b {
                    nodes.push(Node { pos: i, line: nodes[*a].line + 1, fit: fit as u8, w: aw, y: ay, z: az, dem: *dem, prev: Some(*a) });
                    active.push(nodes.len() - 1);
                }
            }
            if active.is_empty() {
                return None;
            }
        }
        if !at_end {
            match &items[i] {
                K::Box { w, .. } => tw += w,
                K::Glue { w, y, z } => {
                    tw += w;
                    ty += y;
                    tz += z;
                }
                _ => {}
            }
        }
    }
    let end = active.iter().filter(|&&a| nodes[a].pos == n).min_by(|&&a, &&b| nodes[a].dem.partial_cmp(&nodes[b].dem).unwrap_or(std::cmp::Ordering::Equal))?;
    let mut out = Vec::new();
    let mut c = Some(*end);
    while let Some(k) = c {
        if nodes[k].prev.is_some() {
            out.push(nodes[k].pos);
        }
        c = nodes[k].prev;
    }
    out.reverse();
    Some(out)
}

fn fitness(r: f32) -> u8 {
    if r < -0.5 {
        0
    } else if r <= 0.5 {
        1
    } else if r <= 1.0 {
        2
    } else {
        3
    }
}

/// One set line: page items relative to the line's left edge and baseline, and its natural width.
pub struct SetLine {
    pub items: Vec<Item>,
    pub w: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Set {
    Justify,
    Left,
    Center,
    Right,
}

/// Set the lines of a paragraph: `breaks` from `breaks()`, `width(n)` the
/// measure, `last_justify` for a line that continues on the next page.
pub fn set(items: &[K], brk: &[usize], width: &dyn Fn(usize) -> f32, how: Set) -> Vec<SetLine> {
    let mut out = Vec::new();
    let mut start = 0;
    for (ln, &end) in brk.iter().enumerate() {
        // skip glue and breaks at the start of a line
        while start < end && matches!(items[start], K::Glue { .. } | K::Break | K::Pen { .. }) {
            start += 1;
        }
        let last = ln + 1 == brk.len() || matches!(items.get(end), Some(K::Break)) || end >= items.len();
        let hyphen = match items.get(end) {
            Some(K::Pen { hyphen: Some(h), .. }) => Some(h.clone()),
            _ => None,
        };
        let seg: Vec<&K> = items[start..end.min(items.len())].iter().filter(|k| !matches!(k, K::Pen { .. } | K::Break)).collect();
        let mut seg: Vec<&K> = seg;
        while matches!(seg.last(), Some(K::Glue { .. })) {
            seg.pop();
        }
        let natural: f32 = seg.iter().map(|k| match k {
            K::Box { w, .. } | K::Glue { w, .. } => *w,
            _ => 0.0,
        }).sum::<f32>() + hyphen.as_ref().map(|h| h.w).unwrap_or(0.0);
        let stretch: f32 = seg.iter().map(|k| if let K::Glue { y, .. } = k { *y } else { 0.0 }).sum();
        let shrink: f32 = seg.iter().map(|k| if let K::Glue { z, .. } = k { *z } else { 0.0 }).sum();
        let lw = width(ln);
        let justify = how == Set::Justify && !last;
        let ratio = if justify {
            if natural < lw && stretch > 0.0 {
                ((lw - natural) / stretch).min(6.0)
            } else if natural > lw && shrink > 0.0 {
                ((lw - natural) / shrink).max(-1.0)
            } else {
                0.0
            }
        } else if natural > lw && shrink > 0.0 {
            ((lw - natural) / shrink).max(-1.0)
        } else {
            0.0
        };
        let used = natural + if ratio > 0.0 { ratio * stretch } else { ratio * shrink };
        let mut x = match how {
            Set::Center => ((lw - used) / 2.0).max(0.0),
            Set::Right => (lw - used).max(0.0),
            _ => 0.0,
        };
        let mut line_items = Vec::new();
        let put = |p: &Piece, x: f32, line_items: &mut Vec<Item>| {
            if p.glyphs.is_empty() {
                return;
            }
            line_items.push(Item::Text { font: p.font, size: p.size, x, y: 0.0, color: p.color, glyphs: p.glyphs.clone(), text: p.text.clone(), map: p.map.clone() });
            if p.strike {
                let y = p.rise - p.size * PT_MM * 0.28;
                line_items.push(Item::Line { x0: x, y0: y, x1: x + p.w, y1: y, width: p.size * PT_MM * 0.06, color: p.color, dash: None });
            }
        };
        for k in &seg {
            match k {
                K::Box { piece, w } => {
                    put(piece, x, &mut line_items);
                    x += w;
                }
                K::Glue { w, y, z } => x += w + if ratio > 0.0 { ratio * y } else { ratio * z },
                _ => {}
            }
        }
        if let Some(h) = &hyphen {
            put(h, x, &mut line_items);
        }
        out.push(SetLine { items: merge(line_items), w: used });
        start = end;
    }
    out
}

/// Neighbouring text items in the same font, size and colour become one.
fn merge(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for it in items {
        if let (Some(Item::Text { font: f0, size: s0, color: c0, x: x0, glyphs: g0, text: t0, map: m0, y: y0 }), Item::Text { font, size, color, x, glyphs, text, map, y }) = (out.last_mut(), &it) {
            if f0 == font && s0 == size && c0 == color && y0 == y {
                let dx = x - *x0;
                g0.extend(glyphs.iter().map(|g| Glyph { id: g.id, x: g.x + dx, y: g.y }));
                t0.push(' ');
                t0.push_str(text);
                m0.extend(map.iter().cloned());
                continue;
            }
        }
        out.push(it);
    }
    out
}
