//! The book in pages, with the plugin's rules: body text on a baseline
//! grid; chapters opening on a recto with a sunk head, label, title,
//! ornament, art and epigraph, a drop cap and small caps on the first
//! line; widows and orphans kept away; headings kept with their text;
//! half-title, title page, credits, dedication, epigraph, contents, part
//! pages, back matter; running heads and folios.

use super::fonts::FontDb;
use super::lines::{self, Ctx, Faces, Set, SetLine, K};
use super::model::{Align, Blk, BookModel, Para, Run, Style};
use super::{hex, Item, Page, PageKind, PT_MM};
use serde_json::Value as Json;

#[derive(Clone, Debug)]
pub struct Geo {
    pub w: f32,
    pub h: f32,
    pub bleed: f32,
    pub mirror: bool,
    pub inner: f32,
    pub outer: f32,
    pub top: f32,
    pub bottom: f32,
    pub lh: f32,
    pub body_h: f32,
    pub body_w: f32,
}

impl Geo {
    pub fn of(cfg: &Json, print: bool) -> Geo {
        let f = |k: &str, d: f64| cfg[k].as_f64().unwrap_or(d) as f32;
        let m = &cfg["margens"];
        let g = |k: &str, d: f64| m[k].as_f64().unwrap_or(d) as f32;
        let (w, h) = (f("largura", 140.0), f("altura", 210.0));
        let (inner, outer, top, bottom) = (g("interna", 22.0), g("externa", 16.0), g("superior", 18.0), g("inferior", 20.0));
        let lh = (f("corpo", 10.5) * PT_MM * f("entrelinha", 1.45) * 100.0).round() / 100.0;
        let body_h = ((h - top - bottom) / lh + 0.001).floor() * lh;
        Geo { w, h, bleed: if print { f("sangria", 3.0) } else { 0.0 }, mirror: print, inner, outer, top, bottom, lh, body_h, body_w: w - inner - outer }
    }

    /// Left of the text block on a page.
    pub fn body_left(&self, recto: bool) -> f32 {
        if self.mirror {
            if recto {
                self.inner
            } else {
                self.outer
            }
        } else {
            (self.inner + self.outer) / 2.0
        }
    }

    /// Offset of the trim box inside the page (bleed only outside the spine).
    pub fn trim_x(&self, recto: bool) -> f32 {
        if self.bleed > 0.0 && self.mirror {
            if recto {
                0.0
            } else {
                self.bleed
            }
        } else {
            self.bleed
        }
    }

    pub fn page_w(&self) -> f32 {
        if self.mirror && self.bleed > 0.0 {
            self.w + self.bleed
        } else {
            self.w + 2.0 * self.bleed
        }
    }
}

pub struct Options {
    pub print: bool,
}

/// Something the flow of a section holds, top to bottom.
#[derive(Clone)]
enum Flow {
    /// A line of text: items relative to (body left, baseline).
    Line { h: f32, base: f32, items: Vec<Item>, para: Option<(usize, usize, usize)>, deco: Option<usize>, keep: bool },
    /// A contents line: its page number is filled at the end.
    Toc { h: f32, base: f32, items: Vec<Item>, section: String, x_text: f32, size: f32 },
    /// Space that disappears at the top of a page.
    Gap(f32, Option<usize>),
    /// An unbreakable piece (figure, opening): items relative to its top left.
    Block { h: f32, items: Vec<Item>, keep: bool, opening: bool },
    Break { recto: bool },
}

#[derive(Clone, Copy)]
enum DecoKind {
    Fill([u8; 3], Option<[u8; 3]>),
    Left([u8; 3]),
    Rules([u8; 3]),
    Frame([u8; 3]),
}

#[derive(Clone, Copy)]
struct Deco {
    kind: DecoKind,
    x0: f32,
    x1: f32,
}

/// Where to put a chapter's page number in the contents, once known.
struct TocFill {
    page: usize,
    y: f32,
    x_text: f32,
    x_right: f32,
    section: String,
    size: f32,
}

pub struct Laid {
    pub pages: Vec<Page>,
    /// section id → page number
    pub toc: Vec<(String, usize)>,
}

struct L<'a> {
    cx: Ctx<'a>,
    cfg: &'a Json,
    g: Geo,
    opts: &'a Options,
    pages: Vec<Page>,
    decos: Vec<Deco>,
    toc: Vec<(String, usize)>,
    toc_fill: Vec<TocFill>,
    accent: Option<[u8; 3]>,
    pic_sizes: Vec<(f32, f32)>,
    info: Vec<PInfo>,
    spans: Vec<(usize, usize, f32, f32)>,
    meta_title: String,
    meta_author: String,
}

/// What a page is for, beyond its items.
#[derive(Clone, Default)]
struct PInfo {
    run_title: String,
    opening: bool,
    no_head: bool,
    no_folio: bool,
}

fn s<'a>(cfg: &'a Json, k: &str) -> &'a str {
    cfg[k].as_str().unwrap_or("")
}

fn num(cfg: &Json, k: &str, d: f32) -> f32 {
    cfg[k].as_f64().map(|v| v as f32).unwrap_or(d)
}

fn on(cfg: &Json, k: &str, d: bool) -> bool {
    cfg[k].as_bool().unwrap_or(d)
}

/// The book's faces: its fonts when installed, the bundled serifs otherwise.
pub fn faces(db: &FontDb, cfg: &Json) -> Faces {
    let find = |fam: &str, fallback: &str, italic: bool, weight: u16| db.find(fam, italic, weight).or_else(|| db.find(fallback, italic, weight)).or_else(|| (!db.faces.is_empty()).then_some(0)).unwrap_or(0);
    let body = s(cfg, "fonte");
    let title = s(cfg, "fonteTitulos");
    let bw = 400;
    let tw = num(cfg, "tituloPeso", 500.0) as u16;
    let b = [find(body, "EB Garamond", false, bw), find(body, "EB Garamond", true, bw), find(body, "EB Garamond", false, 700), find(body, "EB Garamond", true, 700)];
    let t = [find(title, "Cormorant Garamond", false, tw.min(600)), find(title, "Cormorant Garamond", true, tw.min(600)), find(title, "Cormorant Garamond", false, 700), find(title, "Cormorant Garamond", true, 700)];
    let mono = ["Menlo", "SF Mono", "Consolas", "DejaVu Sans Mono", "Courier New", "Liberation Mono"].iter().find_map(|f| db.find(f, false, 400)).unwrap_or(b[0]);
    let has = |id: usize| {
        db.face(id).data().is_some_and(|d| {
            ttf_parser::Face::parse(&d, db.face(id).index).ok().and_then(|f| f.tables().gsub.map(|g| g.features.into_iter().any(|x| &x.tag.to_bytes() == b"smcp"))).unwrap_or(false)
        })
    };
    Faces { smcp_body: has(b[0]), smcp_title: has(t[0]), body: b, title: t, mono }
}

impl<'a> L<'a> {
    fn lh(&self) -> f32 {
        self.g.lh
    }

    /// Baseline inside a line box of height `h` for a size in pt.
    fn base(&self, h: f32, size: f32, title: bool) -> f32 {
        let f = self.cx.db.face(if title { self.cx.faces.title[0] } else { self.cx.faces.body[0] });
        let asc = f.ascender / f.upem * size * PT_MM;
        let desc = -f.descender / f.upem * size * PT_MM;
        (h - asc - desc) / 2.0 + asc
    }

    fn new_page(&mut self, kind: PageKind) -> usize {
        let n = self.pages.len() + 1 + self.start_offset();
        let mut p = Page::new(self.g.page_w(), self.g.h + 2.0 * self.g.bleed, n, kind, hex(s(self.cfg, "corPapel")).unwrap_or([255, 255, 255]));
        p.trim = (self.g.trim_x(p.recto), self.g.bleed, self.g.w, self.g.h);
        self.pages.push(p);
        self.pages.len() - 1
    }

    fn start_offset(&self) -> usize {
        0
    }

    fn next_is_recto(&self) -> bool {
        !(self.pages.len() + 1).is_multiple_of(2)
    }

    fn blank(&mut self) {
        self.new_page(PageKind::Blank);
    }

    /// Text block origin on a page (trim offsets included).
    fn origin(&self, page: usize) -> (f32, f32) {
        let p = &self.pages[page];
        (p.trim.0 + self.g.body_left(p.recto), p.trim.1 + self.g.top)
    }

    fn place(&mut self, page: usize, items: &[Item], dx: f32, dy: f32) {
        let p = &mut self.pages[page];
        for it in items {
            p.items.push(shift(it, dx, dy));
        }
    }

    // ── paragraphs ───────────────────────────────────────────────────

    /// A paragraph as lines (and its drop cap, if any).
    fn para_lines(&self, p: &Para, measure: f32, indent: f32, how: Set, base_st: &Style) -> Vec<SetLine> {
        let mut runs: Vec<Run> = p.runs.iter().map(|r| Run { text: r.text.clone(), st: merge_style(base_st, &r.st) }).collect();
        // drop cap: the first letter (with an opening quote) set apart
        let mut cap: Option<(Vec<Item>, f32, usize)> = None;
        if p.dropcap {
            if let Some(r0) = runs.first_mut() {
                let t = r0.text.trim_start().to_string();
                let mut chars = t.chars();
                let mut letter = String::new();
                for c in chars.by_ref() {
                    letter.push(c);
                    if c.is_alphanumeric() {
                        break;
                    }
                }
                if letter.chars().any(|c| c.is_alphanumeric()) {
                    r0.text = chars.collect();
                    let n = num(self.cfg, "capitularLinhas", 3.0).clamp(2.0, 6.0) as usize;
                    cap = Some(self.dropcap(&letter, n));
                }
            }
        }
        let cap_w = cap.as_ref().map(|c| c.1).unwrap_or(0.0);
        let cap_n = cap.as_ref().map(|c| c.2).unwrap_or(0);
        let width = |n: usize| measure - if n < cap_n { cap_w } else if n == 0 { indent } else { 0.0 };
        let justify = how == Set::Justify;
        let mut items = lines::items(&self.cx, &runs, self.cx.hyphenate);
        let mut brk = lines::breaks(&items, &width, justify);
        // small caps on the first line: break again until the first line holds exactly them
        if p.opener && self.cfg["versaleteAlcance"].as_str() != Some("palavras") {
            let mut chars = first_line_chars(&items, &brk);
            for _ in 0..4 {
                let sc = smallcap_chars(&runs, chars);
                let it2 = lines::items(&self.cx, &sc, self.cx.hyphenate);
                let b2 = lines::breaks(&it2, &width, justify);
                let c2 = first_line_chars(&it2, &b2);
                items = it2;
                brk = b2;
                if c2 == chars {
                    break;
                }
                chars = c2;
            }
        } else if p.opener {
            let sc = smallcap_words(&runs, 4);
            items = lines::items(&self.cx, &sc, self.cx.hyphenate);
            brk = lines::breaks(&items, &width, justify);
        }
        let mut set = lines::set(&items, &brk, &width, how);
        for (i, l) in set.iter_mut().enumerate() {
            let dx = if i < cap_n { cap_w } else if i == 0 { indent } else { 0.0 };
            if dx != 0.0 {
                l.items = l.items.iter().map(|it| shift(it, dx, 0.0)).collect();
            }
        }
        if let (Some((items, _, _)), Some(l0)) = (cap, set.first_mut()) {
            l0.items.extend(items);
        }
        set
    }

    /// The drop cap's items (relative to the first baseline), width and lines spanned.
    fn dropcap(&self, letter: &str, n: usize) -> (Vec<Item>, f32, usize) {
        let mut st = Style { title_font: true, ..Style::plain() };
        let fam = s(self.cfg, "capitularFonte");
        let _ = fam;
        let cf = self.cx.db.face(self.cx.faces.title[0]);
        let body = self.cx.db.face(self.cx.faces.body[0]);
        let body_cap = body.cap_height / body.upem * self.cx.size * PT_MM;
        // from the cap height of the first line down to the baseline of line n
        let span = (n as f32 - 1.0) * self.lh() + body_cap;
        let size = span / (cf.cap_height / cf.upem * PT_MM);
        st.scale = size / self.cx.size;
        st.color = hex(s(self.cfg, "capitularCor")).or(self.accent_or([0x3d, 0x2b, 0x1a]).into());
        let p = self.cx.piece(letter, &st);
        let y = (n as f32 - 1.0) * self.lh();
        let elevated = s(self.cfg, "capitularEstilo") == "elevada";
        let item = Item::Text { font: p.font, size: p.size, x: 0.0, y: if elevated { 0.0 } else { y }, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map };
        let gap = self.cx.size * PT_MM * 0.35;
        if elevated {
            return (vec![item], p.w + gap, 1);
        }
        (vec![item], p.w + gap, n)
    }

    fn accent_or(&self, d: [u8; 3]) -> [u8; 3] {
        self.accent.unwrap_or(d)
    }

    /// Lines of a paragraph into the flow.
    #[allow(clippy::too_many_arguments)]
    fn push_para(&self, flow: &mut Vec<Flow>, pid: &mut usize, p: &Para, measure: f32, x0: f32, st: &Style, align: Option<Align>, deco: Option<usize>) {
        let justify = on(self.cfg, "justificar", true);
        let how = match p.align.or(align) {
            Some(Align::Center) => Set::Center,
            Some(Align::Right) => Set::Right,
            Some(Align::Left) => Set::Left,
            Some(Align::Justify) => Set::Justify,
            None if justify => Set::Justify,
            None => Set::Left,
        };
        let indent = if p.first || num(self.cfg, "recuo", 1.3) == 0.0 { 0.0 } else { num(self.cfg, "recuo", 1.3) * self.cx.size * st.scale * PT_MM };
        let set = self.para_lines(p, measure, indent, how, st);
        let n = set.len();
        *pid += 1;
        let lh = self.lh();
        let base = self.base(lh, self.cx.size, false);
        for (i, l) in set.into_iter().enumerate() {
            flow.push(Flow::Line { h: lh, base, items: l.items.iter().map(|it| shift(it, x0, 0.0)).collect(), para: Some((*pid, i, n)), deco, keep: false });
        }
        // a drop cap taller than its paragraph keeps the room it takes
        if p.dropcap && s(self.cfg, "capitularEstilo") != "elevada" {
            let cap_n = num(self.cfg, "capitularLinhas", 3.0).clamp(2.0, 6.0) as usize;
            if n < cap_n {
                flow.push(Flow::Gap((cap_n - n) as f32 * lh, deco));
            }
        }
    }

    // ── blocks → flow ────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn blocks(&mut self, blocks: &[Blk], flow: &mut Vec<Flow>, pid: &mut usize, x0: f32, measure: f32, st: &Style, align: Option<Align>, deco: Option<usize>) {
        let lh = self.lh();
        let gap = crate::scriptorium::css::para_gap(self.cfg) as f32 * lh;
        let mut prev_p = false;
        for b in blocks {
            match b {
                Blk::P(p) => {
                    if prev_p && gap > 0.0 {
                        flow.push(Flow::Gap(gap, deco));
                    }
                    self.push_para(flow, pid, p, measure, x0, st, align, deco);
                    prev_p = true;
                    continue;
                }
                Blk::H { level, runs } => {
                    let scale = if *level <= 1 { 1.3 } else { 1.0 };
                    let hs = Style { title_font: true, bold: true, scale: st.scale * scale, ..st.clone() };
                    flow.push(Flow::Gap(lh, deco));
                    let p = Para { runs: runs.clone(), first: true, align: Some(Align::Center), dropcap: false, opener: false };
                    let set = self.para_lines(&p, measure, 0.0, Set::Center, &hs);
                    let base = self.base(lh, self.cx.size * hs.scale, true);
                    for l in set {
                        flow.push(Flow::Line { h: lh, base, items: l.items.iter().map(|it| shift(it, x0, 0.0)).collect(), para: None, deco, keep: true });
                    }
                    flow.push(Flow::Gap(lh / 2.0, deco));
                }
                Blk::Scene => {
                    let orn = s(self.cfg, "ornamento");
                    let orn = if orn.is_empty() { "⁂" } else { orn };
                    let ost = Style { title_font: true, track: 0.35, color: Some(self.accent_or([0x6d, 0x5a, 0x40])), ..Style::plain() };
                    let p = self.cx.piece(orn, &ost);
                    let x = x0 + (measure - p.w + 0.35 * self.cx.size * PT_MM) / 2.0;
                    let it = Item::Text { font: p.font, size: p.size, x, y: 0.0, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map };
                    flow.push(Flow::Line { h: 2.0 * lh, base: self.base(2.0 * lh, self.cx.size, true), items: vec![it], para: None, deco, keep: false });
                }
                Blk::Figure { pic, frac, caption } => {
                    let Some(pc) = self.pics_size(*pic) else { continue };
                    let mut w = measure * frac;
                    let mut h = w * pc.1 / pc.0;
                    let maxh = self.g.body_h - 3.0 * lh;
                    if h > maxh {
                        h = maxh;
                        w = h * pc.0 / pc.1;
                    }
                    let mut items = vec![Item::Image { pic: *pic, x: x0 + (measure - w) / 2.0, y: lh / 2.0, w, h }];
                    let mut total = h + lh;
                    if !caption.is_empty() {
                        let cst = Style { italic: true, scale: 0.8, ..st.clone() };
                        let p = Para { runs: caption.clone(), first: true, align: Some(Align::Center), dropcap: false, opener: false };
                        let set = self.para_lines(&p, measure, 0.0, Set::Center, &cst);
                        let ch = self.cx.size * 0.8 * 1.3 * PT_MM;
                        for (i, l) in set.iter().enumerate() {
                            let y = lh / 2.0 + h + 1.5 + ch * (i as f32 + 0.8);
                            items.extend(l.items.iter().map(|it| shift(it, x0, y)));
                        }
                        total += 1.5 + ch * set.len() as f32;
                    }
                    let total = (total / lh).ceil() * lh;
                    flow.push(Flow::Block { h: total, items, keep: false, opening: false });
                }
                Blk::Boxed { kind, title, blocks } => {
                    self.boxed(kind, title, blocks, flow, pid, x0, measure, st, deco);
                }
                Blk::List { ordered, start, items } => {
                    flow.push(Flow::Gap(lh / 2.0, deco));
                    let ind = 1.4 * self.cx.size * PT_MM;
                    for (i, it) in items.iter().enumerate() {
                        let mark = if *ordered { format!("{}.", *start as usize + i) } else { "•".into() };
                        let mp = self.cx.piece(&mark, st);
                        let at = flow.len();
                        let paras: Vec<Blk> = it.iter().map(|b| match b {
                            Blk::P(p) => Blk::P(Para { first: true, align: Some(Align::Left), ..p.clone() }),
                            o => o.clone(),
                        }).collect();
                        self.blocks(&paras, flow, pid, x0 + ind, measure - ind, st, Some(Align::Left), deco);
                        if let Some(Flow::Line { items, .. }) = flow.get_mut(at) {
                            items.push(Item::Text { font: mp.font, size: mp.size, x: x0 + ind - mp.w - 1.2, y: 0.0, color: mp.color, glyphs: mp.glyphs, text: mp.text, map: mp.map });
                        }
                    }
                    flow.push(Flow::Gap(lh / 2.0, deco));
                }
                Blk::Code(t) => {
                    let cst = Style { mono: true, scale: st.scale * 0.8, ..st.clone() };
                    for line in t.lines() {
                        let p = self.cx.piece(line, &cst);
                        let it = Item::Text { font: p.font, size: p.size, x: x0, y: 0.0, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map };
                        flow.push(Flow::Line { h: lh, base: self.base(lh, self.cx.size, false), items: vec![it], para: None, deco, keep: false });
                    }
                }
                Blk::Notes(list) => {
                    flow.push(Flow::Gap(lh, deco));
                    let nst = Style { scale: st.scale * 0.82, ..st.clone() };
                    let nh = self.cx.size * 0.82 * 1.35 * PT_MM;
                    let rule = Item::Line { x0, y0: -nh * 0.9, x1: x0 + measure * 0.3, y1: -nh * 0.9, width: 0.2, color: [0x99, 0x99, 0x99], dash: None };
                    let mut first = true;
                    for (i, note) in list.iter().enumerate() {
                        let mut runs = vec![Run { text: format!("{}. ", i + 1), st: Style::plain() }];
                        runs.extend(note.iter().cloned());
                        let p = Para { runs, first: true, align: Some(Align::Left), dropcap: false, opener: false };
                        let set = self.para_lines(&p, measure, 0.0, Set::Left, &nst);
                        let n = set.len();
                        *pid += 1;
                        for (k, l) in set.into_iter().enumerate() {
                            let mut items: Vec<Item> = l.items.iter().map(|it| shift(it, x0, 0.0)).collect();
                            if first {
                                items.push(rule.clone());
                                first = false;
                            }
                            flow.push(Flow::Line { h: nh, base: nh * 0.78, items, para: Some((*pid, k, n)), deco, keep: false });
                        }
                    }
                }
                Blk::PageBreak { recto } => flow.push(Flow::Break { recto: *recto }),
                Blk::Missing(t) => {
                    let p = self.cx.piece(t, &Style { scale: 0.8, color: Some([0xbb, 0, 0]), ..Style::plain() });
                    flow.push(Flow::Line { h: lh, base: self.base(lh, self.cx.size, false), items: vec![Item::Text { font: p.font, size: p.size, x: x0, y: 0.0, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map }], para: None, deco, keep: false });
                }
            }
            prev_p = false;
        }
    }

    fn pics_size(&self, pic: usize) -> Option<(f32, f32)> {
        self.pic_sizes.get(pic).copied()
    }

    /// Quotes and callouts: margins, type and decoration by kind.
    #[allow(clippy::too_many_arguments)]
    fn boxed(&mut self, kind: &str, title: &[Run], blocks: &[Blk], flow: &mut Vec<Flow>, pid: &mut usize, x0: f32, measure: f32, st: &Style, outer: Option<usize>) {
        let lh = self.lh();
        let mut st2 = st.clone();
        let (mut l, mut r, mut align, mut deco_kind, mut pad) = (0.0f32, 0.0f32, None, None, 0.0f32);
        match kind {
            "" => {
                l = measure * 0.06;
                r = measure * 0.06;
                st2.scale *= 0.95;
            }
            "carta" => {
                st2.italic = true;
                deco_kind = Some(DecoKind::Fill([0xf6, 0xef, 0xdf], Some([0xd8, 0xc9, 0xa6])));
                pad = 4.0;
                l = 5.0;
                r = 5.0;
            }
            "poema" => {
                st2.italic = true;
                l = measure * 0.12;
                align = Some(Align::Left);
            }
            "epigrafe" => {
                st2.italic = true;
                st2.scale *= 0.9;
                l = measure * 0.28;
                align = Some(Align::Right);
            }
            "destaque" => {
                st2.italic = true;
                st2.scale *= 1.3;
                align = Some(Align::Center);
                deco_kind = Some(DecoKind::Rules(self.cx.color));
                pad = 2.0;
                l = measure * 0.04;
                r = measure * 0.04;
            }
            "sonho" => {
                st2.italic = true;
                st2.opacity *= 0.75;
                l = measure * 0.08;
                r = measure * 0.08;
            }
            "flashback" | "note" | "info" | "nota" => {
                if kind == "flashback" {
                    st2.italic = true;
                }
                deco_kind = Some(DecoKind::Left([0x99, 0x99, 0x99]));
                l = 4.0;
            }
            "tela" => {
                st2.mono = true;
                st2.scale *= 0.84;
                st2.color = Some([0x7f, 0xe0, 0xc4]);
                deco_kind = Some(DecoKind::Fill([0x0f, 0x1a, 0x1c], None));
                pad = 3.0;
                l = 4.0;
                r = 4.0;
                align = Some(Align::Left);
            }
            "inscricao" | "placa" => {
                st2.track += 0.08;
                if kind == "placa" {
                    st2.upper = true;
                }
                align = Some(Align::Center);
                deco_kind = Some(DecoKind::Frame(self.cx.color));
                pad = 3.0;
                l = 3.0;
                r = 3.0;
            }
            "mensagem" | "mensagem-eu" => {
                st2.scale *= 0.86;
                let me = kind == "mensagem-eu";
                if me {
                    st2.color = Some([255, 255, 255]);
                    l = measure * 0.26;
                } else {
                    r = measure * 0.26;
                }
                deco_kind = Some(DecoKind::Fill(if me { [0x2f, 0x7b, 0xf6] } else { [0xe9, 0xe9, 0xee] }, None));
                pad = 2.0;
                align = Some(Align::Left);
                l += 3.5;
                r += 3.5;
            }
            "centro" => align = Some(Align::Center),
            "direita" => align = Some(Align::Right),
            "diario" => {
                st2.italic = true;
                deco_kind = Some(DecoKind::Fill([0xf4, 0xf7, 0xfa], None));
                pad = 2.0;
                l = 5.0;
                r = 5.0;
            }
            _ => {
                deco_kind = Some(DecoKind::Left([0x99, 0x99, 0x99]));
                l = 3.0;
            }
        }
        let deco = deco_kind.map(|k| {
            self.decos.push(Deco { kind: k, x0: x0 + if matches!(k, DecoKind::Left(_)) { 0.0 } else { l - if pad > 0.0 { l.min(5.0) } else { 0.0 } }, x1: x0 + measure - r + if pad > 0.0 { r.min(5.0) } else { 0.0 } });
            self.decos.len() - 1
        }).or(outer);
        let space = if kind.is_empty() { lh / 2.0 } else { lh };
        flow.push(Flow::Gap(space, outer));
        if pad > 0.0 {
            flow.push(Flow::Gap(pad, deco));
        }
        if !title.is_empty() && kind != "epigrafe" {
            let tst = Style { title_font: true, smallcaps: true, track: 0.06, ..st2.clone() };
            let p = Para { runs: title.to_vec(), first: true, align: Some(Align::Left), dropcap: false, opener: false };
            let set = self.para_lines(&p, measure - l - r, 0.0, if align == Some(Align::Center) { Set::Center } else { Set::Left }, &tst);
            for line in set {
                flow.push(Flow::Line { h: lh, base: self.base(lh, self.cx.size, true), items: line.items.iter().map(|it| shift(it, x0 + l, 0.0)).collect(), para: None, deco, keep: true });
            }
        }
        let inner: Vec<Blk> = blocks.iter().map(|b| match b {
            Blk::P(p) => Blk::P(Para { first: true, ..p.clone() }),
            o => o.clone(),
        }).collect();
        self.blocks(&inner, flow, pid, x0 + l, measure - l - r, &st2, align, deco);
        if kind == "epigrafe" && !title.is_empty() {
            let tst = Style { smallcaps: true, ..st2.clone() };
            let mut runs = vec![Run { text: "— ".into(), st: Style::plain() }];
            runs.extend(title.iter().cloned());
            let p = Para { runs, first: true, align: Some(Align::Right), dropcap: false, opener: false };
            let set = self.para_lines(&p, measure - l - r, 0.0, Set::Right, &tst);
            for line in set {
                flow.push(Flow::Line { h: lh, base: self.base(lh, self.cx.size, false), items: line.items.iter().map(|it| shift(it, x0 + l, 0.0)).collect(), para: None, deco, keep: false });
            }
        }
        if pad > 0.0 {
            flow.push(Flow::Gap(pad, deco));
        }
        flow.push(Flow::Gap(space, outer));
    }
}

fn merge_style(base: &Style, s: &Style) -> Style {
    Style {
        italic: base.italic ^ s.italic,
        bold: base.bold || s.bold,
        smallcaps: base.smallcaps || s.smallcaps,
        allcaps_small: base.allcaps_small || s.allcaps_small,
        upper: base.upper || s.upper,
        sup: s.sup,
        scale: base.scale * s.scale,
        track: base.track + s.track,
        color: s.color.or(base.color),
        opacity: base.opacity * s.opacity,
        strike: s.strike || base.strike,
        mono: s.mono || base.mono,
        title_font: base.title_font || s.title_font,
    }
}

/// How many characters of the text the first line holds (spaces count one).
fn first_line_chars(items: &[K], brk: &[usize]) -> usize {
    let end = brk.first().copied().unwrap_or(0).min(items.len());
    let mut n = 0;
    for k in &items[..end] {
        match k {
            K::Box { piece, .. } => n += piece.text.chars().count(),
            K::Glue { .. } => n += 1,
            _ => {}
        }
    }
    // spaces at the line's end are not on it
    let mut j = end;
    while j > 0 && matches!(items[j - 1], K::Glue { .. }) {
        n -= 1;
        j -= 1;
    }
    n
}

/// The first `n` characters (leading spaces skipped) in small caps.
fn smallcap_chars(runs: &[Run], n: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut left = n;
    let mut started = false;
    for r in runs {
        if left == 0 {
            out.push(r.clone());
            continue;
        }
        let sc = Style { smallcaps: true, track: r.st.track + 0.03, ..r.st.clone() };
        let mut cut = r.text.len();
        for (i, c) in r.text.char_indices() {
            if !started && c == ' ' {
                continue;
            }
            started = true;
            if left == 0 {
                cut = i;
                break;
            }
            left -= 1;
        }
        out.push(Run { text: r.text[..cut].to_string(), st: sc });
        if cut < r.text.len() {
            out.push(Run { text: r.text[cut..].to_string(), st: r.st.clone() });
        }
    }
    out
}

/// The first `n` words in small caps.
fn smallcap_words(runs: &[Run], n: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut words = 0;
    for r in runs {
        if words >= n {
            out.push(r.clone());
            continue;
        }
        // split the run where the n-th word ends
        let mut cut = None;
        let mut in_word = false;
        for (i, c) in r.text.char_indices() {
            let sp = c == ' ' || c == '\n';
            if !sp && !in_word {
                in_word = true;
            } else if sp && in_word {
                in_word = false;
                words += 1;
                if words >= n {
                    cut = Some(i);
                    break;
                }
            }
        }
        if in_word && cut.is_none() {
            // the run ends inside a word; counted when the next space comes
        }
        let sc = Style { smallcaps: true, track: r.st.track + 0.03, ..r.st.clone() };
        match cut {
            Some(i) => {
                out.push(Run { text: r.text[..i].to_string(), st: sc });
                out.push(Run { text: r.text[i..].to_string(), st: r.st.clone() });
            }
            None => out.push(Run { text: r.text.clone(), st: sc }),
        }
    }
    out
}

pub fn shift(it: &Item, dx: f32, dy: f32) -> Item {
    match it {
        Item::Text { font, size, x, y, color, glyphs, text, map } => Item::Text { font: *font, size: *size, x: x + dx, y: y + dy, color: *color, glyphs: glyphs.clone(), text: text.clone(), map: map.clone() },
        Item::Line { x0, y0, x1, y1, width, color, dash } => Item::Line { x0: x0 + dx, y0: y0 + dy, x1: x1 + dx, y1: y1 + dy, width: *width, color: *color, dash: *dash },
        Item::Rect { x, y, w, h, color } => Item::Rect { x: x + dx, y: y + dy, w: *w, h: *h, color: *color },
        Item::Image { pic, x, y, w, h } => Item::Image { pic: *pic, x: x + dx, y: y + dy, w: *w, h: *h },
    }
}


/// A paginated section's settings.
struct Sec {
    id: String,
    run_title: String,
    no_head: bool,
    no_folio: bool,
}

impl<'a> L<'a> {
    fn page_info(&mut self, page: usize) -> &mut PInfo {
        while self.info.len() <= page {
            self.info.push(PInfo::default());
        }
        &mut self.info[page]
    }

    fn flow_page(&mut self, sec: &Sec) -> usize {
        let p = self.new_page(PageKind::Flow);
        self.pages[p].section = sec.id.clone();
        let i = self.page_info(p);
        i.run_title = sec.run_title.clone();
        i.no_head = sec.no_head;
        i.no_folio = sec.no_folio;
        p
    }

    /// Lay a section's flow on pages.
    fn paginate(&mut self, flow: Vec<Flow>, sec: &Sec, recto: bool) {
        if recto && !self.next_is_recto() {
            self.blank();
        }
        let cap = self.g.body_h + 0.01;
        let mut page: Option<usize> = None;
        let mut y = 0.0f32;
        let mut placed = 0usize;
        let mut i = 0;
        let min_h = |f: &Flow| -> f32 {
            match f {
                Flow::Line { h, para: Some((_, _, n)), .. } => h * (*n).min(2) as f32,
                Flow::Line { h, .. } | Flow::Toc { h, .. } => *h,
                Flow::Block { h, .. } => *h,
                _ => 0.0,
            }
        };
        if !self.pages.is_empty() || page.is_none() {
            self.toc.push((sec.id.clone(), self.pages.len() + 1));
        }
        while i < flow.len() {
            if page.is_none() {
                page = Some(self.flow_page(sec));
                y = 0.0;
                placed = 0;
            }
            let pg = page.unwrap();
            match &flow[i] {
                Flow::Break { recto } => {
                    if placed > 0 {
                        page = None;
                    }
                    if *recto && !self.next_is_recto() && page.is_none() {
                        self.blank();
                    }
                    i += 1;
                }
                Flow::Gap(h, deco) => {
                    if placed > 0 {
                        if y + h > cap {
                            page = None;
                        } else {
                            if let Some(d) = deco {
                                self.spans.push((pg, *d, y, y + h));
                            }
                            y += h;
                        }
                    }
                    i += 1;
                }
                Flow::Block { h, items, keep, opening } => {
                    let need = h + if *keep { flow.get(i + 1).map(min_h).unwrap_or(0.0) } else { 0.0 };
                    if placed > 0 && y + need > cap {
                        page = None;
                        continue;
                    }
                    let (ox, oy) = self.origin(pg);
                    self.place(pg, items, ox, oy + y);
                    if *opening {
                        let inf = self.page_info(pg);
                        inf.opening = true;
                    }
                    y += h;
                    placed += 1;
                    i += 1;
                }
                Flow::Toc { h, base, items, section, x_text, size } => {
                    if placed > 0 && y + h > cap {
                        page = None;
                        continue;
                    }
                    let (ox, oy) = self.origin(pg);
                    self.place(pg, items, ox, oy + y + base);
                    self.toc_fill.push(TocFill { page: pg, y: oy + y + base, x_text: ox + x_text, x_right: ox + self.g.body_w, section: section.clone(), size: *size });
                    y += h;
                    placed += 1;
                    i += 1;
                }
                Flow::Line { para: Some((_, 0, n)), .. } => {
                    // a whole paragraph: split it with no widows or orphans
                    let n = *n;
                    let mut done = 0;
                    while done < n {
                        if page.is_none() {
                            page = Some(self.flow_page(sec));
                            y = 0.0;
                            placed = 0;
                        }
                        let pg = page.unwrap();
                        let Flow::Line { h, .. } = &flow[i + done] else { break };
                        let h = *h;
                        let k = ((cap - y) / h).floor().max(0.0) as usize;
                        let r = n - done;
                        let mut take = k.min(r);
                        if take < r {
                            if done == 0 && take == 1 && placed > 0 {
                                take = 0;
                            }
                            if r - take == 1 && take >= 2 {
                                take -= 1;
                            }
                            if done == 0 && take == 1 && placed > 0 {
                                take = 0;
                            }
                            if take == 0 && placed == 0 {
                                take = k.max(1).min(r);
                            }
                        }
                        for j in 0..take {
                            if let Flow::Line { h, base, items, deco, .. } = &flow[i + done + j] {
                                let (ox, oy) = self.origin(pg);
                                let mut its: Vec<Item> = items.clone();
                                // a paragraph's last line on this page, continued on the next: justified to the end
                                let _ = &mut its;
                                self.place(pg, &its, ox, oy + y + base);
                                if let Some(d) = deco {
                                    self.spans.push((pg, *d, y, y + h));
                                }
                                y += h;
                                placed += 1;
                            }
                        }
                        done += take;
                        if done < n {
                            page = None;
                        }
                    }
                    i += n;
                }
                Flow::Line { h, base, items, deco, keep, .. } => {
                    let need = h + if *keep { flow.get(i + 1).map(min_h).unwrap_or(0.0) } else { 0.0 };
                    if placed > 0 && y + need > cap {
                        page = None;
                        continue;
                    }
                    let (ox, oy) = self.origin(pg);
                    self.place(pg, items, ox, oy + y + base);
                    if let Some(d) = deco {
                        self.spans.push((pg, *d, y, y + h));
                    }
                    y += h;
                    placed += 1;
                    i += 1;
                }
            }
        }
    }

    /// Backgrounds, rules and frames of callouts, under their lines.
    fn draw_decos(&mut self) {
        let mut spans = std::mem::take(&mut self.spans);
        spans.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)));
        let mut merged: Vec<(usize, usize, f32, f32)> = Vec::new();
        for s in spans {
            match merged.last_mut() {
                Some(m) if m.0 == s.0 && m.1 == s.1 && s.2 <= m.3 + 0.05 => m.3 = m.3.max(s.3),
                _ => merged.push(s),
            }
        }
        for (pg, d, y0, y1) in merged {
            let deco = self.decos[d];
            let (ox, oy) = self.origin(pg);
            let (x0, x1, a, b) = (ox + deco.x0, ox + deco.x1, oy + y0, oy + y1);
            let mut under = Vec::new();
            match deco.kind {
                DecoKind::Fill(c, border) => {
                    under.push(Item::Rect { x: x0, y: a, w: x1 - x0, h: b - a, color: c });
                    if let Some(bc) = border {
                        for (p, q) in [((x0, a), (x1, a)), ((x1, a), (x1, b)), ((x1, b), (x0, b)), ((x0, b), (x0, a))] {
                            under.push(Item::Line { x0: p.0, y0: p.1, x1: q.0, y1: q.1, width: 0.3, color: bc, dash: None });
                        }
                    }
                }
                DecoKind::Left(c) => under.push(Item::Line { x0, y0: a, x1: x0, y1: b, width: 0.5, color: c, dash: None }),
                DecoKind::Rules(c) => {
                    under.push(Item::Line { x0, y0: a, x1, y1: a, width: 0.3, color: c, dash: None });
                    under.push(Item::Line { x0, y0: b, x1, y1: b, width: 0.3, color: c, dash: None });
                }
                DecoKind::Frame(c) => {
                    for (p, q) in [((x0, a), (x1, a)), ((x1, a), (x1, b)), ((x1, b), (x0, b)), ((x0, b), (x0, a))] {
                        under.push(Item::Line { x0: p.0, y0: p.1, x1: q.0, y1: q.1, width: 0.6, color: c, dash: None });
                    }
                }
            }
            let items = &mut self.pages[pg].items;
            for (k, it) in under.into_iter().enumerate() {
                items.insert(k, it);
            }
        }
    }

    // ── openings and fixed pages ─────────────────────────────────────

    /// Lines of styled text, centred (or aligned) in `measure`, as items from y = 0 (first baseline at `first_base`).
    fn text_block(&self, runs: &[Run], st: &Style, measure: f32, how: Set, line_h: f32) -> (Vec<Item>, usize) {
        let p = Para { runs: runs.to_vec(), first: true, align: None, dropcap: false, opener: false };
        let set = self.para_lines(&p, measure, 0.0, how, st);
        let n = set.len();
        let mut out = Vec::new();
        for (i, l) in set.into_iter().enumerate() {
            out.extend(l.items.iter().map(|it| shift(it, 0.0, i as f32 * line_h)));
        }
        (out, n)
    }

    fn case_style(&self, k: &str, st: &mut Style) {
        match k {
            "alta" => st.upper = true,
            "versalete" => st.smallcaps = true,
            "versalete-total" => {
                st.smallcaps = true;
                st.allcaps_small = true;
            }
            _ => {}
        }
    }

    fn how(&self) -> Set {
        match s(self.cfg, "tituloAlinhamento") {
            "esquerda" => Set::Left,
            "direita" => Set::Right,
            _ => Set::Center,
        }
    }

    /// The chapter opening as one block.
    fn opening(&self, ch: &super::model::Chapter) -> Flow {
        let lh = self.lh();
        let em = self.cx.size * PT_MM;
        let w = self.g.body_w;
        let how = self.how();
        let mut items = Vec::new();
        let mut y = (self.g.body_h * num(self.cfg, "descida", 28.0) / 100.0 * 10.0).round() / 10.0;
        let acc = self.accent;
        // label
        let lab_size = num(self.cfg, "rotuloTamanho", 0.95);
        let mut lab_st = Style { title_font: true, scale: lab_size, track: if s(self.cfg, "rotuloCaixa") == "normal" { 0.04 } else { 0.2 }, color: Some(acc.unwrap_or([0x6b, 0x5a, 0x45])), ..Style::plain() };
        let caixa = s(self.cfg, "rotuloCaixa");
        self.case_style(if caixa.is_empty() { "versalete" } else { caixa }, &mut lab_st);
        let lab_lh = if lab_size > 1.4 { lab_size * em * 1.1 } else { lh };
        // title
        let t_size = num(self.cfg, "tituloTamanho", 2.0);
        let mut t_st = Style { title_font: true, scale: t_size, track: num(self.cfg, "tituloEspacamento", 0.0), italic: on(self.cfg, "tituloItalico", false), bold: num(self.cfg, "tituloPeso", 500.0) >= 650.0, color: hex(s(self.cfg, "corTitulos")), ..Style::plain() };
        self.case_style(s(self.cfg, "tituloCaixa"), &mut t_st);
        let t_lh = t_size * em * 1.15;
        let mut parts: Vec<&str> = Vec::new();
        let has_lab = !ch.head.label.is_empty();
        let has_tit = ch.head.show_title && !ch.head.title.is_empty();
        let fio = s(self.cfg, "fio");
        let has_rule = !fio.is_empty() && fio != "nenhum";
        let below = s(self.cfg, "rotuloPosicao") == "abaixo";
        let between = s(self.cfg, "fioLugar") == "entre" && has_lab && has_tit;
        let order: [&str; 3] = match (below, between) {
            (true, true) => ["t", "r", "l"],
            (true, false) => ["t", "l", "r"],
            (false, true) => ["l", "r", "t"],
            (false, false) => ["l", "t", "r"],
        };
        for k in order {
            match k {
                "l" if has_lab => parts.push("l"),
                "t" if has_tit => parts.push("t"),
                "r" if has_rule => parts.push("r"),
                _ => {}
            }
        }
        for k in parts {
            match k {
                "l" => {
                    let (its, n) = self.text_block(&[Run { text: ch.head.label.clone(), st: Style::plain() }], &lab_st, w, how, lab_lh);
                    let base = self.base(lab_lh, self.cx.size * lab_size, true);
                    items.extend(its.iter().map(|it| shift(it, 0.0, y + base)));
                    y += n as f32 * lab_lh;
                }
                "t" => {
                    y += 0.3 * t_size * em;
                    let (its, n) = self.text_block(&ch.head.title, &t_st, w, how, t_lh);
                    let base = self.base(t_lh, self.cx.size * t_size, true);
                    items.extend(its.iter().map(|it| shift(it, 0.0, y + base)));
                    y += n as f32 * t_lh + 0.2 * t_size * em;
                }
                _ => {
                    y += 0.55 * em;
                    let rw = match fio {
                        "curto" => 0.09,
                        "largo" => 1.0,
                        _ => 0.28,
                    } * w;
                    let x0 = match how {
                        Set::Left => 0.0,
                        Set::Right => w - rw,
                        _ => (w - rw) / 2.0,
                    };
                    let c = acc.unwrap_or([0x8a, 0x6d, 0x3b]);
                    let (lw, dash) = match fio {
                        "curto" => (0.45, None),
                        "pontilhado" => (0.6, Some((0.6, 0.9))),
                        _ => (0.3, None),
                    };
                    items.push(Item::Line { x0, y0: y, x1: x0 + rw, y1: y, width: lw, color: c, dash });
                    if fio == "duplo" {
                        items.push(Item::Line { x0, y0: y + 0.7, x1: x0 + rw, y1: y + 0.7, width: 0.25, color: c, dash: None });
                        y += 0.7;
                    }
                    y += 0.55 * em;
                }
            }
        }
        let orn = s(self.cfg, "ornamentoAbertura");
        if !orn.is_empty() {
            let ost = Style { title_font: true, scale: 1.1, track: 0.3, color: Some(acc.unwrap_or([0x8a, 0x6d, 0x3b])), ..Style::plain() };
            let (its, _) = self.text_block(&[Run { text: orn.to_string(), st: Style::plain() }], &ost, w, how, lh);
            let base = self.base(lh, self.cx.size * 1.1, true);
            items.extend(its.iter().map(|it| shift(it, 0.15 * em, y + base)));
            y += lh;
        }
        if let Some(pic) = ch.head.art {
            if let Some((pw, ph)) = self.pics_size(pic) {
                y += lh;
                let mut h = 45.0f32.min(w * ph / pw);
                let mut aw = h * pw / ph;
                if aw > w {
                    aw = w;
                    h = aw * ph / pw;
                }
                let x = match how {
                    Set::Left => 0.0,
                    Set::Right => w - aw,
                    _ => (w - aw) / 2.0,
                };
                items.push(Item::Image { pic, x, y, w: aw, h });
                y += h;
            }
        }
        if let Some((epi, who)) = &ch.head.epigraph {
            y += lh;
            let est = Style { italic: true, scale: 0.88, ..Style::plain() };
            let (l, r) = (w * 0.26, w * 0.06);
            let line = 0.88 * em * 1.35;
            let (its, n) = self.text_block(epi, &est, w - l - r, Set::Right, line);
            items.extend(its.iter().map(|it| shift(it, l, y + line * 0.8)));
            y += n as f32 * line;
            if !who.is_empty() {
                let cst = Style { smallcaps: true, scale: 0.88, ..Style::plain() };
                let (its, _) = self.text_block(&[Run { text: who.clone(), st: Style::plain() }], &cst, w - l - r, Set::Right, line);
                y += 0.3 * em;
                items.extend(its.iter().map(|it| shift(it, l, y + line * 0.8)));
                y += line;
            }
        }
        y += 2.0 * lh;
        let h = (y / lh - 0.001).ceil() * lh;
        Flow::Block { h, items, keep: true, opening: true }
    }

    /// A page that is not flowing text: its items relative to the trim box.
    fn fixed(&mut self, recto: bool, verso: bool, items: Vec<Item>, section: &str) -> usize {
        if recto && !self.next_is_recto() {
            self.blank();
        }
        if verso && self.next_is_recto() {
            self.blank();
        }
        let p = self.new_page(PageKind::Fixed);
        self.pages[p].section = section.to_string();
        let (tx, ty) = (self.pages[p].trim.0, self.pages[p].trim.1);
        self.place(p, &items, tx, ty);
        let inf = self.page_info(p);
        inf.no_head = true;
        inf.no_folio = true;
        p
    }

    fn centered(&self, runs: &[Run], st: &Style, y: f32, line: f32) -> (Vec<Item>, f32) {
        let w = self.g.w - 2.0 * self.g.outer;
        let (its, n) = self.text_block(runs, st, w, Set::Center, line);
        let base = self.base(line, self.cx.size * st.scale, st.title_font);
        (its.iter().map(|it| shift(it, self.g.outer, y + base)).collect(), y + n as f32 * line)
    }

    fn front(&mut self, m: &BookModel) {
        let g = self.g.clone();
        let em = self.cx.size * PT_MM;
        let print = self.opts.print;
        let pi = &m.cfg["paginasIniciais"];
        let plain = |t: &str| vec![Run { text: t.to_string(), st: Style::plain() }];
        if print && pi["falsaFolha"].as_bool().unwrap_or(true) {
            let st = Style { title_font: true, scale: 1.5, upper: true, track: 0.12, ..Style::plain() };
            let (its, _) = self.centered(&plain(&m.meta.title), &st, g.top + g.body_h * 0.3, 1.5 * em * 1.3);
            self.fixed(true, false, its, "halftitle");
        }
        if pi["rosto"].as_bool().unwrap_or(true) {
            let mut items = Vec::new();
            let top = g.top;
            let mut y = top + (g.h - g.top - g.bottom) * 0.12;
            if !m.meta.author.is_empty() {
                let st = Style { title_font: true, scale: 1.1, upper: true, track: 0.25, ..Style::plain() };
                let (its, y2) = self.centered(&plain(&m.meta.author), &st, y, 1.1 * em * 1.4);
                items.extend(its);
                y = y2;
            }
            y += (g.h - g.top - g.bottom) * 0.18;
            let st = Style { title_font: true, scale: 2.4, ..Style::plain() };
            let (its, y2) = self.centered(&plain(&m.meta.title), &st, y, 2.4 * em * 1.1);
            items.extend(its);
            y = y2;
            if !m.meta.subtitle.is_empty() {
                let st = Style { title_font: true, italic: true, scale: 1.1, ..Style::plain() };
                let (its, y2) = self.centered(&plain(&m.meta.subtitle), &st, y + 0.6 * 1.1 * em, 1.1 * em * 1.3);
                items.extend(its);
                y = y2;
            }
            let orn = s(self.cfg, "ornamentoAbertura");
            let orn = if orn.is_empty() { "❦" } else { orn };
            let st = Style { title_font: true, color: Some([0x8a, 0x6d, 0x3b]), ..Style::plain() };
            let (its, _) = self.centered(&plain(orn), &st, y + 1.2 * em, em * 1.4);
            items.extend(its);
            if !m.meta.publisher.is_empty() {
                let st = Style { title_font: true, smallcaps: true, track: 0.2, scale: 0.85, ..Style::plain() };
                let (its, _) = self.centered(&plain(&m.meta.publisher), &st, g.h - g.bottom - 0.04 * (g.h - g.top - g.bottom) - em * 1.4, em * 1.4);
                items.extend(its);
            }
            self.fixed(print, false, items, "title");
        }
        if print && pi["creditos"].as_bool().unwrap_or(true) {
            let mut paras: Vec<Vec<Run>> = Vec::new();
            if !m.credits.is_empty() {
                paras.push(m.credits.clone());
            }
            paras.push(plain(&format!("© {} {}. Todos os direitos reservados.", m.meta.year, m.meta.author)));
            if !m.meta.isbn.is_empty() {
                paras.push(plain(&format!("ISBN {}", m.meta.isbn)));
            }
            if !m.meta.publisher.is_empty() {
                paras.push(plain(&m.meta.publisher));
            }
            paras.push(plain("Nenhuma parte desta obra pode ser reproduzida sem autorização."));
            let st = Style { scale: 0.72, ..Style::plain() };
            let line = 0.72 * em * 1.45;
            let mut blocks: Vec<(Vec<Item>, usize)> = paras.iter().map(|p| self.text_block(p, &st, g.body_w, Set::Left, line)).collect();
            let total: f32 = blocks.iter().map(|b| b.1 as f32 * line + 0.6 * 0.72 * em).sum();
            let recto = !self.next_is_recto();
            let left = g.body_left(!recto) ;
            let mut y = g.top + g.body_h - total;
            let mut items = Vec::new();
            for (its, n) in blocks.drain(..) {
                items.extend(its.iter().map(|it| shift(it, left, y + line * 0.8)));
                y += n as f32 * line + 0.6 * 0.72 * em;
            }
            self.fixed(false, true, items, "credits");
        }
        if pi["dedicatoria"].as_bool().unwrap_or(true) && !m.dedication.is_empty() {
            let st = Style { italic: true, ..Style::plain() };
            let (its, _) = self.centered(&m.dedication, &st, g.top + g.body_h * 0.28, self.lh());
            self.fixed(print, false, its, "dedication");
        }
        if !m.epigraph.is_empty() {
            let st = Style { italic: true, ..Style::plain() };
            let l = g.body_w * 0.22;
            let (its, n) = self.text_block(&m.epigraph, &st, g.body_w - l, Set::Right, self.lh());
            let x = g.body_left(true) + l;
            let y = g.top + g.body_h * 0.28;
            let mut items: Vec<Item> = its.iter().map(|it| shift(it, x, y + self.lh() * 0.75)).collect();
            if !m.meta.epigraph_author.is_empty() {
                let cst = Style { smallcaps: true, ..Style::plain() };
                let (c, _) = self.text_block(&plain(&m.meta.epigraph_author), &cst, g.body_w - l, Set::Right, self.lh());
                let cy = y + (n as f32 + 0.5) * self.lh();
                items.extend(c.iter().map(|it| shift(it, x, cy + self.lh() * 0.75)));
            }
            self.fixed(print, false, items, "epigraph");
        }
        if pi["sumario"].as_bool().unwrap_or(true) && m.chapters.len() > 1 {
            self.contents(m);
        }
    }

    fn contents(&mut self, m: &BookModel) {
        let lh = self.lh();
        let em = self.cx.size * PT_MM;
        let w = self.g.body_w;
        let mut flow = Vec::new();
        let tst = Style { title_font: true, scale: 1.6, track: 0.1, color: hex(s(self.cfg, "corTitulos")), ..Style::plain() };
        let (its, n) = self.text_block(&[Run { text: "Sumário".into(), st: Style::plain() }], &tst, w, Set::Center, 1.6 * em * 1.2);
        let base = self.base(1.6 * em * 1.2, self.cx.size * 1.6, true);
        let h = ((n as f32 * 1.6 * em * 1.2 + 2.0 * lh) / lh).ceil() * lh;
        flow.push(Flow::Block { h, items: its.iter().map(|it| shift(it, 0.0, base)).collect(), keep: true, opening: false });
        let mut last_part = String::new();
        let base = self.base(lh, self.cx.size, false);
        for ch in &m.chapters {
            if !ch.part.is_empty() && ch.part != last_part {
                last_part = ch.part.clone();
                flow.push(Flow::Gap(lh, None));
                let pst = Style { smallcaps: true, track: 0.12, ..Style::plain() };
                let (its, _) = self.text_block(&[Run { text: ch.part.clone(), st: Style::plain() }], &pst, w, Set::Center, lh);
                flow.push(Flow::Line { h: lh, base, items: its, para: None, deco: None, keep: true });
            }
            let mut runs = Vec::new();
            if !ch.label.is_empty() {
                let core = crate::scriptorium::export::label_core(&m.cfg, &ch.label);
                runs.push(Run { text: format!("{core}  "), st: Style { smallcaps: true, color: Some([0x6b, 0x5a, 0x45]), ..Style::plain() } });
            }
            runs.extend(ch.head.title.iter().cloned());
            let p = Para { runs, first: true, align: Some(Align::Left), dropcap: false, opener: false };
            let set = self.para_lines(&p, w - 12.0, 0.0, Set::Left, &Style::plain());
            let n = set.len();
            for (k, l) in set.into_iter().enumerate() {
                if k + 1 == n {
                    flow.push(Flow::Toc { h: lh, base, items: l.items, section: ch.id.clone(), x_text: l.w, size: self.cx.size });
                } else {
                    flow.push(Flow::Line { h: lh, base, items: l.items, para: None, deco: None, keep: true });
                }
            }
        }
        let sec = Sec { id: "toc".into(), run_title: String::new(), no_head: true, no_folio: true };
        let print = self.opts.print;
        self.paginate(flow, &sec, print);
    }

    fn part_page(&mut self, part: &str) {
        let em = self.cx.size * PT_MM;
        let label = self.cfg.get("rotuloParte").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| "Parte".into());
        let mut items = Vec::new();
        let mid = self.g.h * 0.42;
        let plain = |t: &str| vec![Run { text: t.to_string(), st: Style::plain() }];
        let mut y = mid - em * 2.0;
        if !label.is_empty() {
            let st = Style { title_font: true, smallcaps: true, track: 0.3, color: Some(self.accent.unwrap_or([0x6b, 0x5a, 0x45])), ..Style::plain() };
            let (its, y2) = self.centered(&plain(&label), &st, y, em * 1.4);
            items.extend(its);
            y = y2 + 0.3 * 2.2 * em;
        }
        let st = Style { title_font: true, scale: 2.2, color: hex(s(self.cfg, "corTitulos")), ..Style::plain() };
        let (its, _) = self.centered(&plain(part), &st, y, 2.2 * em * 1.15);
        items.extend(its);
        let print = self.opts.print;
        self.fixed(print, false, items, "part");
    }

    /// Running heads, folios and the contents' page numbers.
    fn finish(&mut self) {
        let em = self.cx.size * PT_MM;
        let size = self.cx.size * 0.78;
        let head_on = on(self.cfg, "cabecalho", true);
        let folio = s(self.cfg, "folio");
        let folio = if folio.is_empty() { "externo" } else { folio };
        let at_top = s(self.cfg, "folioPosicao") == "topo";
        let fmt = s(self.cfg, "folioFormato");
        let fmt = if fmt.is_empty() { "{n}" } else { fmt };
        let on_open = on(self.cfg, "folioAbertura", true);
        let color = self.accent.unwrap_or([0x5b, 0x4c, 0x3a]);
        let mut hst = Style { scale: 0.78, color: Some(color), title_font: s(self.cfg, "cabecalhoFonte") == "titulos", ..Style::plain() };
        match s(self.cfg, "cabecalhoEstilo") {
            "italico" => {
                hst.italic = true;
                hst.track = 0.02;
            }
            "alta" => {
                hst.upper = true;
                hst.track = 0.2;
                hst.scale *= 0.66;
            }
            "normal" => hst.track = 0.02,
            _ => {
                hst.smallcaps = true;
                hst.track = 0.12;
            }
        }
        let head_y = self.g.top * 0.42 + size * PT_MM * 0.95;
        let foot_y = self.g.h - self.g.bottom * 0.38 - size * PT_MM * 0.3;
        let fst = Style { scale: 0.78, track: 0.04, color: Some(color), ..Style::plain() };
        let figures = if s(self.cfg, "folioAlgarismos") == "alinhado" { "lnum" } else { "onum" };
        for i in 0..self.pages.len() {
            let inf = self.info.get(i).cloned().unwrap_or_default();
            if self.pages[i].kind != PageKind::Flow {
                continue;
            }
            let recto = self.pages[i].recto;
            let left = self.pages[i].trim.0 + self.g.body_left(recto);
            let top = self.pages[i].trim.1;
            if head_on && !inf.no_head && !inf.opening {
                let mode = if recto { s(self.cfg, "cabecalhoRecto") } else { s(self.cfg, "cabecalhoVerso") };
                let mode = if mode.is_empty() { if recto { "capitulo" } else { "livro" } } else { mode };
                let text = match mode {
                    "nada" => String::new(),
                    "autor" => self.meta_author.clone(),
                    "capitulo" => if inf.run_title.is_empty() { self.meta_title.clone() } else { inf.run_title.clone() },
                    _ => self.meta_title.clone(),
                };
                if !text.is_empty() {
                    let mut p = self.cx.piece(&text, &hst);
                    // too long for the measure: cut with an ellipsis
                    let mut t = text.clone();
                    while p.w > self.g.body_w * (if at_top { 0.8 } else { 1.0 }) && t.chars().count() > 4 {
                        t = t.chars().take(t.chars().count() - 2).collect::<String>().trim_end().to_string() + "…";
                        p = self.cx.piece(&t, &hst);
                    }
                    let x = left + (self.g.body_w - p.w) / 2.0;
                    self.pages[i].items.push(Item::Text { font: p.font, size: p.size, x, y: top + head_y, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map });
                }
            }
            if folio != "nenhum" && !inf.no_folio && (!inf.opening || on_open) {
                let n = self.pages[i].n;
                let t = fmt.replace("{n}", &n.to_string());
                let mut cx2 = Ctx { db: self.cx.db, faces: self.cx.faces.clone(), size: self.cx.size, color: self.cx.color, paper: self.cx.paper, hyphenate: false, figures: Some(figures) };
                cx2.figures = Some(figures);
                let p = cx2.piece(&t, &fst);
                let pos = if inf.opening { "centro" } else { folio };
                let right = match pos {
                    "externo" => recto,
                    "interno" => !recto,
                    _ => false,
                };
                let x = if pos == "centro" { left + (self.g.body_w - p.w) / 2.0 } else if right { left + self.g.body_w - p.w } else { left };
                let y = if at_top && !inf.opening { head_y } else { foot_y };
                self.pages[i].items.push(Item::Text { font: p.font, size: p.size, x, y: top + y, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map });
            }
        }
        // the contents' numbers and leaders
        let fills = std::mem::take(&mut self.toc_fill);
        for f in fills {
            let Some(n) = self.toc.iter().find(|t| t.0 == f.section).map(|t| t.1) else { continue };
            let cx2 = Ctx { db: self.cx.db, faces: self.cx.faces.clone(), size: f.size, color: self.cx.color, paper: self.cx.paper, hyphenate: false, figures: Some("onum") };
            let p = cx2.piece(&n.to_string(), &Style::plain());
            let x = f.x_right - p.w;
            let dot_y = f.y - em * 0.08;
            let a = f.x_text + 1.6;
            let b = x - 1.6;
            if b > a + 4.0 {
                self.pages[f.page].items.push(Item::Line { x0: a, y0: dot_y, x1: b, y1: dot_y, width: 0.25, color: [0x8a, 0x7a, 0x66], dash: Some((0.25, 1.0)) });
            }
            self.pages[f.page].items.push(Item::Text { font: p.font, size: p.size, x, y: f.y, color: p.color, glyphs: p.glyphs, text: p.text, map: p.map });
        }
    }
}

/// The whole book in pages.
pub fn typeset(m: &BookModel, db: &FontDb, opts: &Options) -> Laid {
    let cfg = &m.cfg;
    let g = Geo::of(cfg, opts.print);
    let paper = hex(s(cfg, "corPapel")).unwrap_or([255, 255, 255]);
    let cx = Ctx { db, faces: faces(db, cfg), size: num(cfg, "corpo", 10.5), color: hex(s(cfg, "corTexto")).unwrap_or([0x1a, 0x14, 0x10]), paper, hyphenate: on(cfg, "hifenizar", true), figures: None };
    let mut l = L { cx, cfg, g, opts, pages: Vec::new(), decos: Vec::new(), toc: Vec::new(), toc_fill: Vec::new(), accent: hex(s(cfg, "corDestaque")), pic_sizes: m.pics.iter().map(|p| (p.w as f32, p.h as f32)).collect(), info: Vec::new(), spans: Vec::new(), meta_title: m.meta.title.clone(), meta_author: m.meta.author.clone() };
    l.front(m);
    let mut last_part = String::new();
    let mut pid = 0;
    for ch in &m.chapters {
        if !ch.part.is_empty() && ch.part != last_part {
            l.part_page(&ch.part);
            last_part = ch.part.clone();
        }
        let mut flow = vec![l.opening(ch)];
        let w = l.g.body_w;
        l.blocks(&ch.blocks, &mut flow, &mut pid, 0.0, w, &Style::plain(), None, None);
        let sec = Sec { id: ch.id.clone(), run_title: ch.title.clone(), no_head: false, no_folio: false };
        let recto = opts.print && on(cfg, "aberturaImpar", true);
        l.paginate(flow, &sec, recto);
    }
    for (title, blocks) in &m.back {
        let em = l.cx.size * PT_MM;
        let lh = l.lh();
        let tst = Style { title_font: true, scale: 1.5, color: hex(s(cfg, "corTitulos")), ..Style::plain() };
        let (its, n) = l.text_block(&[Run { text: title.clone(), st: Style::plain() }], &tst, l.g.body_w, Set::Center, 1.5 * em * 1.2);
        let base = l.base(1.5 * em * 1.2, l.cx.size * 1.5, true);
        let h = ((n as f32 * 1.5 * em * 1.2 + 2.0 * lh) / lh).ceil() * lh;
        let mut flow = vec![Flow::Block { h, items: its.iter().map(|it| shift(it, 0.0, base)).collect(), keep: true, opening: true }];
        let w = l.g.body_w;
        l.blocks(blocks, &mut flow, &mut pid, 0.0, w, &Style::plain(), None, None);
        let sec = Sec { id: format!("bm-{title}"), run_title: title.clone(), no_head: true, no_folio: false };
        l.paginate(flow, &sec, opts.print);
    }
    l.draw_decos();
    l.finish();
    // a printed book ends on a verso
    if opts.print && l.pages.len() % 2 == 1 {
        l.blank();
    }
    Laid { toc: l.toc.clone(), pages: l.pages }
}
