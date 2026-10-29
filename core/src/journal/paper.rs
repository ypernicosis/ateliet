//! A notebook's size, paper and columns, and where the ruling, the margins
//! and the columns fall on one of its pages.

/// The page size, in millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct PageSize {
    /// As written in the index note: `A5`, `B5`, `A4`, `A6`, `bolso` or `120x180`.
    pub name: String,
    pub w: f64,
    pub h: f64,
}

pub const SIZES: &[(&str, f64, f64)] = &[("A6", 105.0, 148.0), ("A5", 148.0, 210.0), ("B5", 176.0, 250.0), ("A4", 210.0, 297.0), ("bolso", 90.0, 140.0)];

impl PageSize {
    pub fn parse(s: &str) -> PageSize {
        let t = s.trim();
        if let Some((_, w, h)) = SIZES.iter().find(|(n, ..)| n.eq_ignore_ascii_case(t)) {
            return PageSize { name: t.to_string(), w: *w, h: *h };
        }
        let nums: Vec<f64> = t.trim_end_matches("mm").split(['x', 'X', '×']).filter_map(|n| n.trim().replace(',', ".").parse().ok()).collect();
        if let [w, h] = nums[..] {
            if (40.0..=600.0).contains(&w) && (40.0..=600.0).contains(&h) {
                return PageSize { name: t.to_string(), w, h };
            }
        }
        PageSize::parse("A5")
    }
}

impl Default for PageSize {
    fn default() -> PageSize {
        PageSize::parse("A5")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ruling {
    Plain,
    Lined,
    Grid,
    Dot,
}

/// The paper: its ruling and the space between lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paper {
    pub ruling: Ruling,
    pub spacing: f64,
}

impl Default for Paper {
    fn default() -> Paper {
        Paper { ruling: Ruling::Grid, spacing: 5.0 }
    }
}

pub const PAPERS: &[(&str, &str)] = &[("liso", "Liso"), ("pautado-7mm", "Pautado 7 mm"), ("pautado-6mm", "Pautado 6 mm"), ("grade-5mm", "Quadriculado 5 mm"), ("grade-4mm", "Quadriculado 4 mm"), ("pontilhado-5mm", "Pontilhado 5 mm")];

impl Paper {
    /// `liso`, `pautado-7mm`, `grade-5mm`, `pontilhado-5mm` (the spacing is optional).
    pub fn parse(s: &str) -> Paper {
        let t = s.trim().to_lowercase();
        let (kind, rest) = t.split_once('-').unwrap_or((t.as_str(), ""));
        let ruling = match kind {
            "liso" | "plain" => Ruling::Plain,
            "pautado" | "lined" => Ruling::Lined,
            "pontilhado" | "dot" | "dots" => Ruling::Dot,
            _ => Ruling::Grid,
        };
        let default = if matches!(ruling, Ruling::Lined | Ruling::Plain) { 7.0 } else { 5.0 };
        let spacing = rest.trim_end_matches("mm").replace(',', ".").parse::<f64>().ok().filter(|s| (2.0..=20.0).contains(s)).unwrap_or(default);
        Paper { ruling, spacing }
    }

    pub fn name(&self) -> String {
        let kind = match self.ruling {
            Ruling::Plain => return "liso".into(),
            Ruling::Lined => "pautado",
            Ruling::Grid => "grade",
            Ruling::Dot => "pontilhado",
        };
        format!("{kind}-{}mm", fmt_num(self.spacing))
    }

    /// The distance between the lines text sits on: the ruling's, or 7 mm
    /// on plain paper (where the lines are there without being drawn).
    pub fn line(&self) -> f64 {
        self.spacing
    }
}

/// `5` for 5.0, `5.5` for 5.5.
pub fn fmt_num(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// How a page is divided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Columns {
    #[default]
    None,
    /// Cornell: a narrow margin column for comments beside the text, under a
    /// line across the top. The margin is on the left, on the right, on the
    /// outer side of each page (left on the left page) or the inner side.
    CornellLeft,
    CornellRight,
    CornellOuter,
    CornellInner,
    Two,
}

pub const COLUMNS: &[(&str, &str)] = &[("nenhuma", "Sem colunas"), ("cornell-externa", "Cornell, margem por fora"), ("cornell-interna", "Cornell, margem por dentro"), ("cornell-esquerda", "Cornell, margem à esquerda"), ("cornell-direita", "Cornell, margem à direita"), ("duas", "Duas colunas")];

impl Columns {
    pub fn parse(s: &str) -> Columns {
        match s.trim().to_lowercase().as_str() {
            "cornell-esquerda" | "cornell" => Columns::CornellLeft,
            "cornell-direita" => Columns::CornellRight,
            "cornell-externa" => Columns::CornellOuter,
            "cornell-interna" => Columns::CornellInner,
            "duas" => Columns::Two,
            _ => Columns::None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Columns::None => "nenhuma",
            Columns::CornellLeft => "cornell-esquerda",
            Columns::CornellRight => "cornell-direita",
            Columns::CornellOuter => "cornell-externa",
            Columns::CornellInner => "cornell-interna",
            Columns::Two => "duas",
        }
    }

    fn cornell(&self) -> bool {
        matches!(self, Columns::CornellLeft | Columns::CornellRight | Columns::CornellOuter | Columns::CornellInner)
    }
}

/// Space left free round the page's edges.
pub const MARGIN_TOP: f64 = 12.0;
pub const MARGIN_BOTTOM: f64 = 10.0;
pub const MARGIN_SIDE: f64 = 9.0;
/// Room between a column line and the text beside it.
pub const GUTTER: f64 = 3.0;

/// Where things fall on one page.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub w: f64,
    pub h: f64,
    /// A right-hand page (the odd ones).
    pub right: bool,
    pub paper: Paper,
    /// The drawn horizontal lines (lined and grid paper), top to bottom.
    pub rows: Vec<f64>,
    /// The drawn vertical lines (grid), or the dots' columns (dot paper).
    pub cols: Vec<f64>,
    /// The lines text sits on, top to bottom (drawn or not).
    pub baselines: Vec<f64>,
    /// Cornell: the line across the top, and the column line under it.
    pub header: Option<f64>,
    pub divider: Option<f64>,
    /// The text columns, left to right (x0, x1); the margin column is one.
    pub columns: Vec<(f64, f64)>,
    /// Which of `columns` is the margin (Cornell).
    pub margin: Option<usize>,
}

/// Lines `s` apart filling `0..len`, centred so the leftover is shared.
fn even(len: f64, s: f64) -> Vec<f64> {
    let n = (len / s).floor() as i64;
    let o = (len - n as f64 * s) / 2.0;
    (0..=n).map(|k| o + k as f64 * s).collect()
}

impl Layout {
    pub fn new(size: &PageSize, paper: Paper, columns: Columns, right: bool) -> Layout {
        let (w, h) = (size.w, size.h);
        let s = paper.line();
        let (rows, cols, mut baselines) = match paper.ruling {
            Ruling::Grid | Ruling::Dot => {
                let ys = even(h, s);
                let xs = even(w, s);
                let base: Vec<f64> = ys.iter().copied().filter(|&y| y >= MARGIN_TOP + s && y <= h - MARGIN_BOTTOM).collect();
                (ys, xs, base)
            }
            Ruling::Lined | Ruling::Plain => {
                let top = MARGIN_TOP + 2.0 * s;
                let n = ((h - MARGIN_BOTTOM - top) / s).floor().max(0.0) as i64;
                let base: Vec<f64> = (0..=n).map(|k| top + k as f64 * s).collect();
                let drawn = if paper.ruling == Ruling::Lined { base.clone() } else { Vec::new() };
                (drawn, Vec::new(), base)
            }
        };
        let near_col = |x: f64| -> f64 {
            if paper.ruling == Ruling::Grid || paper.ruling == Ruling::Dot {
                cols.iter().copied().min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs())).unwrap_or(x)
            } else {
                x
            }
        };
        let (x0, x1) = (MARGIN_SIDE, w - MARGIN_SIDE);
        let mut header = None;
        let mut divider = None;
        let mut out_cols = vec![(x0, x1)];
        let mut margin = None;
        if columns.cornell() && !baselines.is_empty() {
            header = Some(baselines.remove(0));
            let left = match columns {
                Columns::CornellLeft => true,
                Columns::CornellRight => false,
                Columns::CornellOuter => !right,
                _ => right,
            };
            let mw = w * 0.3;
            if left {
                let d = near_col(x0 + mw);
                divider = Some(d);
                out_cols = vec![(x0, d - GUTTER), (d + GUTTER, x1)];
                margin = Some(0);
            } else {
                let d = near_col(x1 - mw);
                divider = Some(d);
                out_cols = vec![(x0, d - GUTTER), (d + GUTTER, x1)];
                margin = Some(1);
            }
        } else if columns == Columns::Two {
            let mid = near_col(w / 2.0);
            out_cols = vec![(x0, mid - GUTTER * 1.5), (mid + GUTTER * 1.5, x1)];
        }
        Layout { w, h, right, paper, rows, cols, baselines, header, divider, columns: out_cols, margin }
    }

    /// The column under `x` (the nearest when `x` falls between two).
    pub fn column_at(&self, x: f64) -> (f64, f64) {
        self.columns.iter().copied().min_by(|a, b| {
            let d = |c: &(f64, f64)| if x < c.0 { c.0 - x } else if x > c.1 { x - c.1 } else { 0.0 };
            d(a).total_cmp(&d(b))
        }).unwrap_or((MARGIN_SIDE, self.w - MARGIN_SIDE))
    }

    /// The main column (the widest).
    pub fn main_column(&self) -> (f64, f64) {
        self.columns.iter().copied().max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0))).unwrap_or((MARGIN_SIDE, self.w - MARGIN_SIDE))
    }

    /// The first line text can sit on.
    pub fn first_baseline(&self) -> f64 {
        self.baselines.first().copied().unwrap_or(MARGIN_TOP + self.paper.line())
    }

    /// The lowest line text can sit on.
    pub fn last_baseline(&self) -> f64 {
        self.baselines.last().copied().unwrap_or(self.h - MARGIN_BOTTOM)
    }

    /// The line a point is nearest to, among those text can sit on.
    pub fn nearest_baseline(&self, y: f64) -> f64 {
        self.baselines.iter().copied().min_by(|a, b| (a - y).abs().total_cmp(&(b - y).abs())).unwrap_or(y)
    }

    /// The top of a text box whose first line sits on the line nearest
    /// below `y` (a click lands on the line under the pointer).
    pub fn text_top(&self, y: f64, lines: f64) -> f64 {
        let s = self.paper.line() * lines;
        let b = self.baselines.iter().copied().find(|&b| b >= y + 0.2 * s).unwrap_or_else(|| self.last_baseline());
        b - s
    }

    /// How many lines of `lines` ruling lines fit from `top` to the last line.
    pub fn rows_from(&self, top: f64, lines: f64) -> usize {
        let s = self.paper.line() * lines;
        (((self.last_baseline() - top) / s) + 1e-6).floor().max(0.0) as usize
    }

    /// Lines an item may snap to: margins, columns and the column line
    /// (vertical), and the ruling (horizontal).
    pub fn snap_lines(&self) -> (Vec<f64>, Vec<f64>) {
        let mut xs: Vec<f64> = self.columns.iter().flat_map(|c| [c.0, c.1]).collect();
        xs.extend(self.divider);
        xs.push(self.w / 2.0);
        let mut ys = self.baselines.clone();
        ys.extend(self.header);
        ys.push(MARGIN_TOP);
        ys.push(self.h - MARGIN_BOTTOM);
        (xs, ys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for (n, _) in PAPERS {
            assert_eq!(Paper::parse(n).name(), *n);
        }
        for (n, _) in COLUMNS {
            assert_eq!(Columns::parse(n).name(), *n);
        }
        assert_eq!(PageSize::parse("120x180").w, 120.0);
        assert_eq!(PageSize::parse("b5").h, 250.0);
        assert_eq!(PageSize::parse("nada").name, "A5");
    }

    #[test]
    fn cornell_margins_face_out() {
        let size = PageSize::parse("A5");
        let paper = Paper::parse("grade-5mm");
        let l = Layout::new(&size, paper, Columns::CornellOuter, false);
        let r = Layout::new(&size, paper, Columns::CornellOuter, true);
        assert_eq!(l.margin, Some(0));
        assert_eq!(r.margin, Some(1));
        // the column line sits on the grid
        let d = l.divider.unwrap();
        assert!(l.cols.iter().any(|c| (c - d).abs() < 1e-9));
        assert!(l.header.unwrap() < l.first_baseline());
        // text boxes start a line above their first baseline
        let top = l.text_top(l.first_baseline() - 1.0, 1.0);
        assert!((top + 5.0 - l.first_baseline()).abs() < 1e-9);
        assert!(l.rows_from(top, 1.0) == l.baselines.len());
    }
}
