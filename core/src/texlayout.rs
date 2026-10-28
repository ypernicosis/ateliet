//! TeX math laid out as boxes, as a typesetter would: stacked fractions,
//! roots whose sign grows with what is under it, scripts at script size,
//! limits above and below big operators in display style, delimiters
//! stretched by `\left … \right`, matrices and `cases`/`aligned`, accents,
//! and TeX's spaces around operators and relations.
//!
//! Heights are estimates of the ink (x-height, cap height, descenders), not
//! the font's line metrics, so the pieces sit close as in print; the app
//! only measures widths and paints glyphs on their baselines.

use crate::texmath::symbol;

/// The face a glyph is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Roman,
    Italic,
    Bold,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// Text whose baseline is at `y`.
    Glyphs { x: f32, y: f32, text: String, size: f32, face: Face },
    /// A filled rectangle (fraction bars, overlines), `y` its top.
    Rule { x: f32, y: f32, w: f32, h: f32 },
    /// A stroked line (root signs, tall delimiters, accents).
    Path { pts: Vec<(f32, f32)>, width: f32 },
}

/// A laid-out piece: its width, how far its ink rises above the baseline
/// and falls below it, and what to paint (y grows downward, baseline 0).
#[derive(Clone, Debug, Default)]
pub struct MathBox {
    pub w: f32,
    pub asc: f32,
    pub desc: f32,
    pub items: Vec<Item>,
}

impl MathBox {
    fn add(&mut self, b: MathBox, dx: f32, dy: f32) {
        self.asc = self.asc.max(b.asc - dy);
        self.desc = self.desc.max(b.desc + dy);
        self.w = self.w.max(dx + b.w);
        for it in b.items {
            self.items.push(match it {
                Item::Glyphs { x, y, text, size, face } => Item::Glyphs { x: x + dx, y: y + dy, text, size, face },
                Item::Rule { x, y, w, h } => Item::Rule { x: x + dx, y: y + dy, w, h },
                Item::Path { pts, width } => Item::Path { pts: pts.into_iter().map(|(x, y)| (x + dx, y + dy)).collect(), width },
            });
        }
    }
}

/// The width of `text` at `size` in `face`.
pub type Measure<'a> = &'a dyn Fn(&str, f32, Face) -> f32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Accent {
    Hat,
    Bar,
    Vec,
    Dot,
    Ddot,
    Tilde,
    Under,
}

#[derive(Clone, Debug)]
enum Node {
    Sym { text: String, face: Face, class: Class },
    /// ∑, ∫, lim…: `limits` puts scripts above and below in display style.
    BigOp { text: String, face: Face, big: bool, limits: bool },
    Group(Vec<Node>),
    Frac { num: Vec<Node>, den: Vec<Node>, bar: bool },
    Sqrt { index: Option<Vec<Node>>, body: Vec<Node> },
    Scripts { base: Box<Node>, sup: Option<Vec<Node>>, sub: Option<Vec<Node>> },
    Delim { left: String, body: Vec<Node>, right: String },
    Matrix { rows: Vec<Vec<Vec<Node>>>, left: String, right: String, align: Align },
    Accent { kind: Accent, body: Vec<Node> },
    Space(f32),
}

/// How a matrix's columns line up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Center,
    Left,
    /// `aligned`: right, left, right, left…
    Pairs,
}

#[derive(Clone, Debug, PartialEq)]
enum End {
    Eof,
    Brace,
    Amp,
    Row,
    Right(String),
    Env,
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

const BINS: &str = "+−±∓×÷·∗∘•∪∩∧∨⊕⊗∖⋅";
const RELS: &str = "=<>≤≥≠≈≡∼≃≅∝≪≫∈∉∋⊂⊃⊆⊇→←↔⇒⇐⇔↦↑↓∣∥⊥:";
const OPEN: &str = "([{⟨⌊⌈";
const CLOSE: &str = ")]}⟩⌋⌉!";

fn class_of(text: &str) -> Class {
    let mut cs = text.chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) if BINS.contains(c) => Class::Bin,
        (Some(c), None) if RELS.contains(c) => Class::Rel,
        (Some(c), None) if OPEN.contains(c) => Class::Open,
        (Some(c), None) if CLOSE.contains(c) => Class::Close,
        (Some(',' | ';'), None) => Class::Punct,
        _ => Class::Ord,
    }
}

/// Greek lowercase is italic in TeX; capitals and symbols are upright.
fn face_of_symbol(s: &str) -> Face {
    match s.chars().next() {
        Some(c) if ('α'..='ω').contains(&c) || "ϑϖϕς".contains(c) => Face::Italic,
        _ => Face::Roman,
    }
}

fn double_struck(c: char) -> char {
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32)).unwrap_or(c),
        'a'..='z' => char::from_u32(0x1D552 + (c as u32 - 'a' as u32)).unwrap_or(c),
        '0'..='9' => char::from_u32(0x1D7D8 + (c as u32 - '0' as u32)).unwrap_or(c),
        _ => c,
    }
}

fn script_letter(c: char) -> char {
    match c {
        'A'..='Z' => char::from_u32(0x1D4D0 + (c as u32 - 'A' as u32)).unwrap_or(c),
        _ => c,
    }
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }

    /// A command's name after its backslash (letters, or one other character).
    fn name(&mut self) -> String {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i == start && self.i < self.s.len() {
            self.i += 1;
        }
        self.s[start..self.i].iter().collect()
    }

    /// The raw text of a `{…}` argument (or of one character).
    fn raw(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') {
            return self.peek().map(|c| {
                self.i += 1;
                c.to_string()
            }).unwrap_or_default();
        }
        self.i += 1;
        let start = self.i;
        let mut depth = 1;
        while let Some(c) = self.peek() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                '\\' => self.i += 1,
                _ => {}
            }
            self.i += 1;
        }
        let out: String = self.s[start..self.i.min(self.s.len())].iter().collect();
        self.i += 1;
        out
    }

    /// One argument: a group, or a single atom.
    fn arg(&mut self) -> Vec<Node> {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                self.row().0
            }
            Some(_) => {
                let mut out = Vec::new();
                self.atom(&mut out);
                out
            }
            None => Vec::new(),
        }
    }

    /// A delimiter after `\left`, `\right`, `\big`…
    fn delim(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('\\') => {
                self.i += 1;
                let n = self.name();
                match n.as_str() {
                    "{" => "{".into(),
                    "}" => "}".into(),
                    "|" => "‖".into(),
                    "langle" => "⟨".into(),
                    "rangle" => "⟩".into(),
                    "lfloor" => "⌊".into(),
                    "rfloor" => "⌋".into(),
                    "lceil" => "⌈".into(),
                    "rceil" => "⌉".into(),
                    "vert" => "|".into(),
                    "Vert" => "‖".into(),
                    _ => String::new(),
                }
            }
            Some(c) => {
                self.i += 1;
                if c == '.' { String::new() } else { c.to_string() }
            }
            None => String::new(),
        }
    }

    /// Nodes up to the end of the group, cell or formula.
    fn row(&mut self) -> (Vec<Node>, End) {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else { return (out, End::Eof) };
            match c {
                '}' => {
                    self.i += 1;
                    return (out, End::Brace);
                }
                '&' => {
                    self.i += 1;
                    return (out, End::Amp);
                }
                '^' | '_' => {
                    self.i += 1;
                    let arg = self.arg();
                    attach(&mut out, c == '^', arg);
                }
                '\'' => {
                    self.i += 1;
                    attach(&mut out, true, vec![Node::Sym { text: "′".into(), face: Face::Roman, class: Class::Ord }]);
                }
                '\\' => {
                    let save = self.i;
                    self.i += 1;
                    let n = self.name();
                    match n.as_str() {
                        "\\" | "cr" => return (out, End::Row),
                        "right" => {
                            let d = self.delim();
                            return (out, End::Right(d));
                        }
                        "end" => {
                            self.raw();
                            return (out, End::Env);
                        }
                        "limits" | "nolimits" => {
                            if let Some(Node::BigOp { limits, .. }) = out.last_mut() {
                                *limits = n == "limits";
                            }
                            if let Some(Node::Scripts { base, .. }) = out.last_mut() {
                                if let Node::BigOp { limits, .. } = base.as_mut() {
                                    *limits = n == "limits";
                                }
                            }
                        }
                        _ => {
                            self.i = save;
                            self.atom(&mut out);
                        }
                    }
                }
                _ => self.atom(&mut out),
            }
        }
    }

    /// One atom onto `out`.
    fn atom(&mut self, out: &mut Vec<Node>) {
        self.skip_ws();
        let Some(c) = self.peek() else { return };
        self.i += 1;
        if c == '{' {
            out.push(Node::Group(self.row().0));
            return;
        }
        if c != '\\' {
            if c.is_ascii_digit() || c == '.' {
                // a number as one piece
                let mut t = c.to_string();
                while let Some(d) = self.peek().filter(|d| d.is_ascii_digit() || (*d == '.' && self.s.get(self.i + 1).is_some_and(|x| x.is_ascii_digit()))) {
                    t.push(d);
                    self.i += 1;
                }
                out.push(Node::Sym { text: t, face: Face::Roman, class: Class::Ord });
                return;
            }
            let (text, face) = match c {
                '-' => ("−".to_string(), Face::Roman),
                '*' => ("∗".to_string(), Face::Roman),
                c if c.is_alphabetic() => (c.to_string(), Face::Italic),
                c => (c.to_string(), Face::Roman),
            };
            let class = class_of(&text);
            out.push(Node::Sym { text, face, class });
            return;
        }
        let n = self.name();
        let sym = |text: &str, class: Class| Node::Sym { text: text.to_string(), face: Face::Roman, class };
        match n.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let (num, den) = (self.arg(), self.arg());
                out.push(Node::Frac { num, den, bar: true });
            }
            "binom" | "dbinom" | "tbinom" => {
                let (num, den) = (self.arg(), self.arg());
                out.push(Node::Delim { left: "(".into(), body: vec![Node::Frac { num, den, bar: false }], right: ")".into() });
            }
            "sqrt" => {
                self.skip_ws();
                let index = if self.peek() == Some('[') {
                    self.i += 1;
                    let start = self.i;
                    while self.peek().is_some_and(|c| c != ']') {
                        self.i += 1;
                    }
                    let src: String = self.s[start..self.i].iter().collect();
                    self.i += 1;
                    Some(parse_nodes(&src))
                } else {
                    None
                };
                let body = self.arg();
                out.push(Node::Sqrt { index, body });
            }
            "left" => {
                let left = self.delim();
                let (body, end) = self.row();
                let right = if let End::Right(r) = end { r } else { String::new() };
                out.push(Node::Delim { left, body, right });
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" | "bigm" | "Bigm" => {
                let d = self.delim();
                if !d.is_empty() {
                    let class = class_of(&d);
                    out.push(Node::Sym { text: d, face: Face::Roman, class });
                }
            }
            "begin" => {
                let env = self.raw();
                if env == "array" {
                    self.raw();
                }
                out.push(self.matrix(&env));
            }
            "text" | "textrm" | "mbox" | "textnormal" | "mathrm" | "operatorname" | "textup" | "textsf" | "mathsf" | "texttt" | "mathtt" => {
                let t = self.raw();
                let text = if n.starts_with("math") || n == "operatorname" { crate::texmath::convert(&t) } else { t };
                out.push(if n == "operatorname" { Node::BigOp { text, face: Face::Roman, big: false, limits: false } } else { Node::Sym { text, face: Face::Roman, class: Class::Ord } });
            }
            "textit" | "mathit" | "emph" => {
                let t = self.raw();
                out.push(Node::Sym { text: crate::texmath::convert(&t), face: Face::Italic, class: Class::Ord });
            }
            "textbf" | "mathbf" | "boldsymbol" | "bm" => {
                let t = self.raw();
                out.push(Node::Sym { text: crate::texmath::convert(&t), face: Face::Bold, class: Class::Ord });
            }
            "mathbb" | "Bbb" => {
                let t = self.raw();
                out.push(sym(&t.chars().map(double_struck).collect::<String>(), Class::Ord));
            }
            "mathcal" | "mathscr" => {
                let t = self.raw();
                out.push(sym(&t.chars().map(script_letter).collect::<String>(), Class::Ord));
            }
            "mathfrak" => {
                let t = self.raw();
                out.push(Node::Sym { text: t, face: Face::Italic, class: Class::Ord });
            }
            "hat" | "widehat" | "bar" | "overline" | "vec" | "overrightarrow" | "dot" | "ddot" | "tilde" | "widetilde" | "underline" | "overbrace" | "underbrace" => {
                let kind = match n.as_str() {
                    "hat" | "widehat" => Accent::Hat,
                    "bar" | "overline" | "overbrace" => Accent::Bar,
                    "vec" | "overrightarrow" => Accent::Vec,
                    "dot" => Accent::Dot,
                    "ddot" => Accent::Ddot,
                    "tilde" | "widetilde" => Accent::Tilde,
                    _ => Accent::Under,
                };
                let body = self.arg();
                out.push(Node::Accent { kind, body });
            }
            "," | "thinspace" => out.push(Node::Space(3.0 / 18.0)),
            ":" | ">" | "medspace" => out.push(Node::Space(4.0 / 18.0)),
            ";" | "thickspace" => out.push(Node::Space(5.0 / 18.0)),
            "!" | "negthinspace" => out.push(Node::Space(-3.0 / 18.0)),
            " " => out.push(Node::Space(0.3)),
            "quad" => out.push(Node::Space(1.0)),
            "qquad" => out.push(Node::Space(2.0)),
            "displaystyle" | "textstyle" | "scriptstyle" | "left." | "right." | "nonumber" | "notag" | "label" | "tag" => {
                if n == "label" || n == "tag" {
                    self.raw();
                }
            }
            "not" => {
                let mut next = Vec::new();
                self.atom(&mut next);
                let negated = match next.first() {
                    Some(Node::Sym { text, .. }) => match text.as_str() {
                        "=" => "≠",
                        "∈" => "∉",
                        "<" => "≮",
                        ">" => "≯",
                        "≤" => "≰",
                        "≥" => "≱",
                        "⊂" => "⊄",
                        "≡" => "≢",
                        _ => "",
                    },
                    _ => "",
                };
                if negated.is_empty() {
                    out.extend(next);
                } else {
                    out.push(sym(negated, Class::Rel));
                }
            }
            "sum" | "prod" | "coprod" | "bigcup" | "bigcap" | "bigoplus" | "bigotimes" | "bigvee" | "bigwedge" | "bigsqcup" => {
                let text = match n.as_str() {
                    "sum" => "∑",
                    "prod" => "∏",
                    "coprod" => "∐",
                    "bigcup" => "⋃",
                    "bigcap" => "⋂",
                    "bigoplus" => "⨁",
                    "bigotimes" => "⨂",
                    "bigvee" => "⋁",
                    "bigwedge" => "⋀",
                    _ => "⨆",
                };
                out.push(Node::BigOp { text: text.into(), face: Face::Roman, big: true, limits: true });
            }
            "int" | "iint" | "iiint" | "oint" => {
                let text = match n.as_str() {
                    "iint" => "∬",
                    "iiint" => "∭",
                    "oint" => "∮",
                    _ => "∫",
                };
                out.push(Node::BigOp { text: text.into(), face: Face::Roman, big: true, limits: false });
            }
            "lim" | "max" | "min" | "sup" | "inf" | "det" | "liminf" | "limsup" | "argmax" | "argmin" | "Pr" | "gcd" => {
                let text = match n.as_str() {
                    "liminf" => "lim inf",
                    "limsup" => "lim sup",
                    "argmax" => "arg max",
                    "argmin" => "arg min",
                    t => t,
                };
                out.push(Node::BigOp { text: text.into(), face: Face::Roman, big: false, limits: true });
            }
            "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "sinh" | "cosh" | "tanh" | "coth" | "arcsin" | "arccos" | "arctan" | "log" | "ln" | "lg" | "exp" | "arg" | "dim" | "ker" | "deg" | "hom" | "mod" | "bmod" => {
                out.push(Node::BigOp { text: n.trim_start_matches('b').to_string(), face: Face::Roman, big: false, limits: false });
            }
            "pmod" => {
                let arg = self.arg();
                out.push(Node::Space(0.5));
                out.push(Node::Delim { left: "(".into(), body: std::iter::once(Node::Sym { text: "mod".into(), face: Face::Roman, class: Class::Ord }).chain(std::iter::once(Node::Space(0.3))).chain(arg).collect(), right: ")".into() });
            }
            "{" => out.push(sym("{", Class::Open)),
            "}" => out.push(sym("}", Class::Close)),
            "|" => out.push(sym("‖", Class::Ord)),
            "\\" => {}
            _ => match symbol(&n) {
                Some(s) if !s.trim().is_empty() => {
                    let class = class_of(s);
                    out.push(Node::Sym { text: s.to_string(), face: face_of_symbol(s), class });
                }
                Some(_) => out.push(Node::Space(0.25)),
                None => out.push(Node::Sym { text: n, face: Face::Roman, class: Class::Ord }),
            },
        }
    }

    /// The body of `\begin{env}` up to its `\end`.
    fn matrix(&mut self, env: &str) -> Node {
        let mut rows: Vec<Vec<Vec<Node>>> = vec![Vec::new()];
        loop {
            let (cell, end) = self.row();
            rows.last_mut().unwrap().push(cell);
            match end {
                End::Amp => {}
                End::Row => rows.push(Vec::new()),
                _ => break,
            }
        }
        // a trailing \\ leaves an empty row
        if rows.len() > 1 && rows.last().is_some_and(|r| r.iter().all(|c| c.is_empty())) {
            rows.pop();
        }
        let (left, right, align) = match env.trim_end_matches('*') {
            "pmatrix" => ("(", ")", Align::Center),
            "bmatrix" => ("[", "]", Align::Center),
            "Bmatrix" => ("{", "}", Align::Center),
            "vmatrix" => ("|", "|", Align::Center),
            "Vmatrix" => ("‖", "‖", Align::Center),
            "cases" | "dcases" => ("{", "", Align::Left),
            "rcases" => ("", "}", Align::Left),
            "aligned" | "align" | "split" | "alignat" | "eqnarray" | "gathered" | "gather" => ("", "", if env.starts_with("gather") { Align::Center } else { Align::Pairs }),
            _ => ("", "", Align::Center),
        };
        Node::Matrix { rows, left: left.into(), right: right.into(), align }
    }
}

/// `^`/`_` onto the last node (an empty base when there is none).
fn attach(out: &mut Vec<Node>, up: bool, arg: Vec<Node>) {
    let last = out.pop().unwrap_or(Node::Group(Vec::new()));
    let node = match last {
        Node::Scripts { base, mut sup, mut sub } if (up && sup.is_none()) || (!up && sub.is_none()) => {
            if up {
                sup = Some(arg);
            } else {
                sub = Some(arg);
            }
            Node::Scripts { base, sup, sub }
        }
        other => {
            let (sup, sub) = if up { (Some(arg), None) } else { (None, Some(arg)) };
            Node::Scripts { base: Box::new(other), sup, sub }
        }
    };
    out.push(node);
}

fn parse_nodes(src: &str) -> Vec<Node> {
    let mut p = Parser { s: src.chars().collect(), i: 0 };
    let mut out = Vec::new();
    loop {
        let (mut nodes, end) = p.row();
        out.append(&mut nodes);
        // a stray `}` or `&` at the top: keep going
        if end == End::Eof {
            break;
        }
    }
    out
}

// ── layout ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Style {
    size: f32,
    /// The formula's own size (scripts never get smaller than half of it).
    base: f32,
    display: bool,
}

impl Style {
    fn script(self) -> Style {
        Style { size: (self.size * 0.7).max(self.base * 0.5), base: self.base, display: false }
    }
    fn text(self) -> Style {
        Style { display: false, ..self }
    }
    /// The height of the math axis (where fraction bars sit) above the baseline.
    fn axis(self) -> f32 {
        self.size * 0.25
    }
    fn rule(self) -> f32 {
        (self.size * 0.055).max(1.0)
    }
}

/// How high the ink of `text` rises and how far it falls, at `size`.
fn ink(text: &str, size: f32) -> (f32, f32) {
    let mut asc: f32 = 0.0;
    let mut desc: f32 = 0.0;
    for c in text.chars() {
        let a = if "acemnorsuvwxzαεικνοπστυωγημρχ+−=×÷·∗∘<>~∼≈≃−,.:;".contains(c) { 0.47 } else { 0.72 };
        asc = asc.max(a);
        if "gjpqyβγζημξρφχψ()[]{}|‖/⟨⟩⌊⌋⌈⌉,;∫∬∭∮∑∏".contains(c) {
            desc = desc.max(if "()[]{}|‖⟨⟩⌊⌋⌈⌉∫∬∭∮".contains(c) { 0.26 } else { 0.22 });
        }
        if "()[]{}|‖⟨⟩⌊⌋⌈⌉".contains(c) {
            asc = asc.max(0.76);
        }
    }
    if text.is_empty() {
        asc = 0.47;
    }
    (asc * size, desc * size)
}

fn glyphs(text: &str, st: Style, face: Face, m: Measure) -> MathBox {
    let (asc, desc) = ink(text, st.size);
    let mut w = m(text, st.size, face);
    // italic letters lean past their advance
    if face == Face::Italic {
        w += st.size * 0.03;
    }
    MathBox { w, asc, desc, items: vec![Item::Glyphs { x: 0.0, y: 0.0, text: text.to_string(), size: st.size, face }] }
}

/// TeX's spaces between neighbours, in em (thin, medium, thick); none in
/// scripts except around operators.
fn space(a: Class, b: Class, st: Style) -> f32 {
    use Class::*;
    let mu = match (a, b) {
        (Ord, Op) | (Op, Ord) | (Op, Op) | (Close, Op) => 3.0,
        (Bin, _) | (_, Bin) => 4.0,
        (Rel, _) | (_, Rel) if a != Rel || b != Rel => 5.0,
        (Punct, _) => 3.0,
        _ => 0.0,
    };
    if st.size < st.base * 0.9 && mu > 3.0 {
        return 0.0;
    }
    mu / 18.0 * st.size
}

fn class_of_node(n: &Node) -> Class {
    match n {
        Node::Sym { class, .. } => *class,
        Node::BigOp { .. } => Class::Op,
        Node::Scripts { base, .. } => match class_of_node(base) {
            Class::Op => Class::Op,
            _ => Class::Ord,
        },
        Node::Space(_) => Class::Ord,
        _ => Class::Ord,
    }
}

fn hlist(nodes: &[Node], st: Style, m: Measure) -> MathBox {
    let mut b = MathBox::default();
    let mut x = 0.0;
    let mut prev: Option<Class> = None;
    for n in nodes {
        if let Node::Space(em) = n {
            x += em * st.size;
            b.w = b.w.max(x);
            continue;
        }
        let mut c = class_of_node(n);
        // a sign with nothing to work on is just a sign (unary minus)
        if c == Class::Bin && matches!(prev, None | Some(Class::Bin | Class::Rel | Class::Open | Class::Op | Class::Punct)) {
            c = Class::Ord;
        }
        if let Some(p) = prev {
            x += space(p, c, st);
        }
        let nb = node(n, st, m);
        let w = nb.w;
        b.add(nb, x, 0.0);
        x += w;
        b.w = b.w.max(x);
        prev = Some(c);
    }
    // a trailing binary sign was really an ordinary one; no matter for widths
    if nodes.is_empty() {
        b.asc = st.size * 0.47;
    }
    b
}

fn node(n: &Node, st: Style, m: Measure) -> MathBox {
    match n {
        Node::Sym { text, face, .. } => glyphs(text, st, *face, m),
        Node::BigOp { text, face, big, .. } => {
            if !*big {
                return glyphs(text, st, *face, m);
            }
            // bigger in display, centred on the axis
            let k = if st.display { 1.55 } else { 1.15 };
            let s = Style { size: st.size * k, ..st };
            let mut g = glyphs(text, s, *face, m);
            let integral = "∫∬∭∮".contains(text.as_str());
            let (asc, desc) = if integral { (0.78 * s.size, 0.26 * s.size) } else { (0.76 * s.size, 0.2 * s.size) };
            // where the glyph's ink centre should be: on the axis
            let shift = (asc - desc) / 2.0 - st.axis();
            let mut b = MathBox::default();
            g.asc = asc;
            g.desc = desc;
            b.add(g, 0.0, shift);
            b
        }
        Node::Space(em) => MathBox { w: em * st.size, ..Default::default() },
        Node::Group(v) => hlist(v, st, m),
        Node::Frac { num, den, bar } => frac(num, den, *bar, st, m),
        Node::Sqrt { index, body } => sqrt(index.as_deref(), body, st, m),
        Node::Scripts { base, sup, sub } => scripts(base, sup.as_deref(), sub.as_deref(), st, m),
        Node::Delim { left, body, right } => {
            let inner = hlist(body, st, m);
            fenced(inner, left, right, st, m)
        }
        Node::Matrix { rows, left, right, align } => {
            let inner = matrix(rows, *align, st, m);
            fenced(inner, left, right, st, m)
        }
        Node::Accent { kind, body } => accent(*kind, body, st, m),
    }
}

fn frac(num: &[Node], den: &[Node], bar: bool, st: Style, m: Measure) -> MathBox {
    let inner = if st.display { st.text() } else { st.script() };
    let (n, d) = (hlist(num, inner, m), hlist(den, inner, m));
    let pad = st.size * 0.12;
    let w = n.w.max(d.w) + 2.0 * pad;
    let (a, t) = (st.axis(), st.rule());
    let gap = st.size * if st.display { 0.16 } else { 0.1 };
    let mut b = MathBox::default();
    let num_base = -(a + t / 2.0 + gap + n.desc);
    let den_base = -a + t / 2.0 + gap + d.asc;
    let (nw, dw) = (n.w, d.w);
    b.add(n, (w - nw) / 2.0, num_base);
    b.add(d, (w - dw) / 2.0, den_base);
    if bar {
        b.items.push(Item::Rule { x: pad * 0.4, y: -a - t / 2.0, w: w - pad * 0.8, h: t });
    }
    b.w = w;
    b
}

fn sqrt(index: Option<&[Node]>, body: &[Node], st: Style, m: Measure) -> MathBox {
    let inner = hlist(body, st, m);
    let t = st.rule();
    let gap = st.size * 0.12;
    let top = -(inner.asc + gap + t / 2.0);
    let bottom = inner.desc.max(st.size * 0.05) + t;
    let h = bottom - top;
    let sw = (st.size * 0.42 + h * 0.12).min(st.size * 0.9);
    let mut b = MathBox::default();
    let mut ox = 0.0;
    let mut idx_box = None;
    if let Some(ix) = index {
        let s = Style { size: (st.size * 0.5).max(st.base * 0.4), base: st.base, display: false };
        let ib = hlist(ix, s, m);
        ox = (ib.w - sw * 0.45).max(0.0);
        idx_box = Some(ib);
    }
    // the sign: a small tick, down to the bottom, up to the top
    let knee_y = bottom - h * 0.42;
    let pts = vec![(ox, knee_y + st.size * 0.04), (ox + sw * 0.22, knee_y - st.size * 0.03), (ox + sw * 0.52, bottom), (ox + sw, top), (ox + sw + inner.w + st.size * 0.12, top)];
    b.items.push(Item::Path { pts, width: t });
    let iw = inner.w;
    b.add(inner, ox + sw + st.size * 0.06, 0.0);
    if let Some(ib) = idx_box {
        let y = knee_y - st.size * 0.12 - ib.desc;
        b.add(ib, 0.0, y);
    }
    b.asc = b.asc.max(-top + t);
    b.desc = b.desc.max(bottom);
    b.w = ox + sw + iw + st.size * 0.16;
    b
}

fn scripts(base: &Node, sup: Option<&[Node]>, sub: Option<&[Node]>, st: Style, m: Measure) -> MathBox {
    let small = st.script();
    if let Node::BigOp { limits: true, .. } = base {
        if st.display {
            // limits above and below, centred
            let bb = node(base, st, m);
            let up = sup.map(|s| hlist(s, small, m));
            let dn = sub.map(|s| hlist(s, small, m));
            let w = bb.w.max(up.as_ref().map_or(0.0, |u| u.w)).max(dn.as_ref().map_or(0.0, |d| d.w));
            let gap = st.size * 0.12;
            let mut b = MathBox::default();
            let (bw, basc, bdesc) = (bb.w, bb.asc, bb.desc);
            b.add(bb, (w - bw) / 2.0, 0.0);
            if let Some(u) = up {
                let y = -(basc + gap + u.desc);
                let uw = u.w;
                b.add(u, (w - uw) / 2.0, y);
            }
            if let Some(d) = dn {
                let y = bdesc + gap + d.asc;
                let dw = d.w;
                b.add(d, (w - dw) / 2.0, y);
            }
            b.w = w;
            return b;
        }
    }
    let bb = node(base, st, m);
    let italic = matches!(base, Node::Sym { face: Face::Italic, .. });
    let integral = matches!(base, Node::BigOp { text, .. } if "∫∬∭∮".contains(text.as_str()));
    let up = sup.map(|s| hlist(s, small, m));
    let dn = sub.map(|s| hlist(s, small, m));
    let mut u_shift = (bb.asc - small.size * 0.38).max(st.size * 0.36);
    let mut d_shift = (bb.desc + small.size * 0.12).max(st.size * 0.16);
    if let (Some(u), Some(d)) = (&up, &dn) {
        // room between them
        let gap = (u_shift - u.desc) - (d.asc - d_shift);
        let need = st.size * 0.12;
        if gap < need {
            d_shift += need - gap;
        }
        if sub.is_some() {
            d_shift = d_shift.max(st.size * 0.22);
        }
        let _ = &mut u_shift;
    }
    let mut b = MathBox::default();
    let bw = bb.w;
    b.add(bb, 0.0, 0.0);
    let x_up = bw + if italic { st.size * 0.04 } else { 0.0 } + if integral { -st.size * 0.1 } else { 0.0 };
    let x_dn = bw - if italic { st.size * 0.03 } else { 0.0 } - if integral { st.size * 0.3 } else { 0.0 };
    let mut w = bw;
    if let Some(u) = up {
        w = w.max(x_up + u.w);
        b.add(u, x_up, -u_shift);
    }
    if let Some(d) = dn {
        w = w.max(x_dn + d.w);
        b.add(d, x_dn, d_shift);
    }
    b.w = w + st.size * 0.04;
    b
}

/// A delimiter `h` tall, centred on the axis: the glyph when that is tall
/// enough, else drawn.
fn delimiter(d: &str, asc: f32, desc: f32, st: Style, m: Measure) -> MathBox {
    if d.is_empty() {
        return MathBox { w: st.size * 0.06, ..Default::default() };
    }
    let a = st.axis();
    // symmetric around the axis
    let half = (asc - a).max(desc + a).max(st.size * 0.45);
    if half <= st.size * 0.55 {
        return glyphs(d, st, Face::Roman, m);
    }
    // the axis is at y = -a
    let top = -(a + half);
    let h = 2.0 * half;
    let t = st.rule() * 1.15;
    let w = (st.size * 0.32 + h * 0.03).min(st.size * 0.6);
    let (y0, y1) = (top, top + h);
    let mid = (y0 + y1) / 2.0;
    let pad = st.size * 0.06;
    let curve = |x0: f32, xm: f32| -> Vec<(f32, f32)> {
        (0..=20).map(|i| {
            let t = i as f32 / 20.0;
            let y = y0 + (y1 - y0) * t;
            // a flattened arc: the middle bulges, the ends come back
            let k = 1.0 - (2.0 * t - 1.0).powi(2);
            (x0 + (xm - x0) * k, y)
        }).collect()
    };
    let pts: Vec<Vec<(f32, f32)>> = match d {
        "(" => vec![curve(pad + w * 0.85, pad + w * 0.1)],
        ")" => vec![curve(pad + w * 0.15, pad + w * 0.9)],
        "[" | "⌊" | "⌈" => {
            let (x0, x1) = (pad + w * 0.25, pad + w * 0.85);
            let mut v = vec![(x0, y0), (x0, y1)];
            if d != "⌊" {
                v.insert(0, (x1, y0));
            }
            if d != "⌈" {
                v.push((x1, y1));
            }
            vec![v]
        }
        "]" | "⌋" | "⌉" => {
            let (x0, x1) = (pad + w * 0.15, pad + w * 0.75);
            let mut v = vec![(x1, y0), (x1, y1)];
            if d != "⌋" {
                v.insert(0, (x0, y0));
            }
            if d != "⌉" {
                v.push((x0, y1));
            }
            vec![v]
        }
        "{" | "}" => {
            let s = if d == "{" { 1.0 } else { -1.0 };
            let (xa, xb) = if d == "{" { (pad + w * 0.9, pad + w * 0.45) } else { (pad + w * 0.1, pad + w * 0.55) };
            let xc = xb - s * w * 0.4;
            let r = w * 0.35;
            vec![vec![(xa, y0), (xb + s * r * 0.3, y0 + r * 0.4), (xb, y0 + r), (xb, mid - r), (xc, mid), (xb, mid + r), (xb, y1 - r), (xb + s * r * 0.3, y1 - r * 0.4), (xa, y1)]]
        }
        "|" => vec![vec![(pad + w * 0.5, y0), (pad + w * 0.5, y1)]],
        "‖" => vec![vec![(pad + w * 0.35, y0), (pad + w * 0.35, y1)], vec![(pad + w * 0.75, y0), (pad + w * 0.75, y1)]],
        "⟨" => vec![vec![(pad + w * 0.85, y0), (pad + w * 0.2, mid), (pad + w * 0.85, y1)]],
        "⟩" => vec![vec![(pad + w * 0.15, y0), (pad + w * 0.8, mid), (pad + w * 0.15, y1)]],
        "/" => vec![vec![(pad + w, y0), (pad, y1)]],
        _ => return glyphs(d, st, Face::Roman, m),
    };
    MathBox { w: w + 2.0 * pad, asc: -y0, desc: y1, items: pts.into_iter().map(|p| Item::Path { pts: p, width: t }).collect() }
}

fn fenced(inner: MathBox, left: &str, right: &str, st: Style, m: Measure) -> MathBox {
    let l = delimiter(left, inner.asc, inner.desc, st, m);
    let r = delimiter(right, inner.asc, inner.desc, st, m);
    let mut b = MathBox::default();
    let (lw, iw) = (l.w, inner.w);
    b.add(l, 0.0, 0.0);
    b.add(inner, lw, 0.0);
    b.add(r, lw + iw, 0.0);
    b
}

fn matrix(rows: &[Vec<Vec<Node>>], align: Align, st: Style, m: Measure) -> MathBox {
    let cell_st = st.text();
    let cells: Vec<Vec<MathBox>> = rows.iter().map(|r| r.iter().map(|c| hlist(c, cell_st, m)).collect()).collect();
    let cols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
    let widths: Vec<f32> = (0..cols).map(|j| cells.iter().filter_map(|r| r.get(j)).map(|c| c.w).fold(0.0, f32::max)).collect();
    let row_gap = st.size * if align == Align::Center { 0.45 } else { 0.35 };
    let col_gap = |j: usize| match align {
        Align::Pairs if j % 2 == 1 => 0.0,
        Align::Pairs => st.size * 1.0,
        _ => st.size * 0.9,
    };
    // the rows' heights
    let heights: Vec<(f32, f32)> = cells.iter().map(|r| (r.iter().map(|c| c.asc).fold(st.size * 0.72, f32::max), r.iter().map(|c| c.desc).fold(st.size * 0.22, f32::max))).collect();
    let total: f32 = heights.iter().map(|h| h.0 + h.1).sum::<f32>() + row_gap * heights.len().saturating_sub(1) as f32;
    // centred on the axis
    let mut y = -st.axis() - total / 2.0;
    let mut b = MathBox::default();
    let pad = st.size * 0.12;
    for (i, row) in cells.into_iter().enumerate() {
        let base = y + heights[i].0;
        let mut x = pad;
        for (j, c) in row.into_iter().enumerate() {
            let cw = c.w;
            let dx = match align {
                Align::Center => (widths[j] - cw) / 2.0,
                Align::Left => 0.0,
                Align::Pairs => if j % 2 == 0 { widths[j] - cw } else { 0.0 },
            };
            b.add(c, x + dx, base);
            x += widths[j];
            if j + 1 < cols {
                x += col_gap(j + 1);
            }
        }
        y = base + heights[i].1 + row_gap;
    }
    b.w = pad * 2.0 + widths.iter().sum::<f32>() + (1..cols).map(col_gap).sum::<f32>();
    b.asc = b.asc.max(st.axis() + total / 2.0);
    b.desc = b.desc.max(total / 2.0 - st.axis());
    b
}

fn accent(kind: Accent, body: &[Node], st: Style, m: Measure) -> MathBox {
    let inner = hlist(body, st, m);
    let t = st.rule();
    let single_italic = matches!(body, [Node::Sym { face: Face::Italic, text, .. }] if text.chars().count() == 1);
    let lean = if single_italic { st.size * 0.06 } else { 0.0 };
    let (w, asc, desc) = (inner.w, inner.asc, inner.desc);
    let mut b = MathBox::default();
    b.add(inner, 0.0, 0.0);
    let cx = w / 2.0 + lean;
    let y = -(asc + st.size * 0.1);
    let aw = (w * 0.9).clamp(st.size * 0.35, st.size * 3.0);
    match kind {
        Accent::Bar => b.items.push(Item::Rule { x: if single_italic { lean } else { 0.0 }, y: y - t, w, h: t }),
        Accent::Under => b.items.push(Item::Rule { x: 0.0, y: desc + st.size * 0.08, w, h: t }),
        Accent::Hat => b.items.push(Item::Path { pts: vec![(cx - aw / 2.0, y), (cx, y - st.size * 0.16), (cx + aw / 2.0, y)], width: t }),
        Accent::Vec => {
            let (x0, x1) = (cx - aw / 2.0, cx + aw / 2.0);
            let yy = y - st.size * 0.06;
            b.items.push(Item::Path { pts: vec![(x0, yy), (x1, yy)], width: t });
            b.items.push(Item::Path { pts: vec![(x1 - st.size * 0.12, yy - st.size * 0.08), (x1, yy), (x1 - st.size * 0.12, yy + st.size * 0.08)], width: t });
        }
        Accent::Tilde => {
            let pts = (0..=12).map(|i| {
                let f = i as f32 / 12.0;
                (cx - aw / 2.0 + aw * f, y - st.size * 0.07 - (f * std::f32::consts::TAU).sin() * st.size * 0.05)
            }).collect();
            b.items.push(Item::Path { pts, width: t });
        }
        Accent::Dot | Accent::Ddot => {
            let r = st.size * 0.045;
            let xs: Vec<f32> = if kind == Accent::Dot { vec![cx] } else { vec![cx - st.size * 0.1, cx + st.size * 0.1] };
            for x in xs {
                b.items.push(Item::Path { pts: vec![(x - r * 0.5, y - st.size * 0.06), (x + r * 0.5, y - st.size * 0.06)], width: r * 2.0 });
            }
        }
    }
    b.asc = b.asc.max(-(y - st.size * 0.2));
    b.w = w;
    b
}

/// Lay out `src` at `size`: `display` for a formula of its own (big
/// operators, limits above and below), else the style of running text.
pub fn layout(src: &str, size: f32, display: bool, measure: Measure) -> MathBox {
    let nodes = parse_nodes(src);
    let st = Style { size, base: size, display };
    let mut b = hlist(&nodes, st, measure);
    b.asc = b.asc.max(size * 0.72);
    b.desc = b.desc.max(size * 0.22);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(t: &str, s: f32, _: Face) -> f32 {
        t.chars().count() as f32 * s * 0.5
    }

    fn texts(b: &MathBox) -> Vec<(String, f32, f32)> {
        b.items.iter().filter_map(|i| if let Item::Glyphs { text, x, y, .. } = i { Some((text.clone(), *x, *y)) } else { None }).collect()
    }

    #[test]
    fn fraction_stacks_over_a_bar() {
        let b = layout(r"\frac{a+b}{c}", 20.0, true, &m);
        let t = texts(&b);
        let a = t.iter().find(|x| x.0 == "a").unwrap();
        let c = t.iter().find(|x| x.0 == "c").unwrap();
        assert!(a.2 < 0.0 && c.2 > a.2 + 10.0, "numerator above the denominator: {t:?}");
        let rule = b.items.iter().find_map(|i| if let Item::Rule { y, w, .. } = i { Some((*y, *w)) } else { None }).unwrap();
        assert!(rule.0 < 0.0 && rule.0 > a.2, "the bar between them");
        assert!(t.iter().any(|x| x.0 == "+"));
        // the minus sign of `a-b` is a real minus, with room around it
        let b = layout("a-b", 20.0, false, &m);
        let t = texts(&b);
        assert_eq!(t[1].0, "−");
        assert!(t[1].1 > t[0].1 + 10.0 + 1.0);
        // unary minus: no room after
        let t = texts(&layout("-x", 20.0, false, &m));
        assert!((t[1].1 - 10.0).abs() < 0.01, "{t:?}");
    }

    #[test]
    fn scripts_limits_and_roots() {
        let t = texts(&layout("x^2_i", 20.0, false, &m));
        let (two, i) = (t.iter().find(|x| x.0 == "2").unwrap(), t.iter().find(|x| x.0 == "i").unwrap());
        assert!(two.2 < 0.0 && i.2 > 0.0 && two.1 > 0.0);
        // display ∑ has its limits above and below
        let t = texts(&layout(r"\sum_{k=1}^{n} k", 20.0, true, &m));
        let (n, k1) = (t.iter().find(|x| x.0 == "n").unwrap(), t.iter().find(|x| x.0 == "k").unwrap());
        let sum = t.iter().find(|x| x.0 == "∑").unwrap();
        assert!(n.2 < sum.2 - 15.0 && k1.2 > sum.2 + 5.0, "{t:?}");
        // inline, beside
        let t = texts(&layout(r"\sum_{k=1}^{n} k", 20.0, false, &m));
        let (n, sum) = (t.iter().find(|x| x.0 == "n").unwrap(), t.iter().find(|x| x.0 == "∑").unwrap());
        assert!(n.1 > sum.1);
        let b = layout(r"\sqrt[3]{\frac{1}{x}}", 20.0, true, &m);
        assert!(b.items.iter().any(|i| matches!(i, Item::Path { .. })));
        assert!(texts(&b).iter().any(|x| x.0 == "3"));
    }

    #[test]
    fn matrices_and_tall_delimiters() {
        let b = layout(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", 20.0, true, &m);
        let t = texts(&b);
        let (a, d) = (t.iter().find(|x| x.0 == "a").unwrap(), t.iter().find(|x| x.0 == "d").unwrap());
        assert!(d.1 > a.1 && d.2 > a.2);
        assert_eq!(b.items.iter().filter(|i| matches!(i, Item::Path { .. })).count(), 2, "drawn parentheses");
        let b = layout(r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{otherwise} \end{cases}", 20.0, true, &m);
        assert!(texts(&b).iter().any(|x| x.0 == "otherwise"));
        let b = layout(r"\left( \frac{a}{b} \right)", 20.0, true, &m);
        assert_eq!(b.items.iter().filter(|i| matches!(i, Item::Path { .. })).count(), 2);
        let b = layout(r"\hat{x} + \vec{v} + \overline{AB} + \mathbb{R}", 20.0, false, &m);
        assert!(texts(&b).iter().any(|x| x.0 == "ℝ"));
        let b = layout(r"\begin{aligned} a &= b \\ &= c \end{aligned}", 20.0, true, &m);
        let t = texts(&b);
        let eqs: Vec<f32> = t.iter().filter(|x| x.0 == "=").map(|x| x.1).collect();
        assert_eq!(eqs.len(), 2);
        assert!((eqs[0] - eqs[1]).abs() < 0.01, "the = line up: {t:?}");
        // nothing breaks on odd input
        for src in ["", "}", "{", r"\frac", r"\left(", "x^", r"\begin{matrix}", "a & b", r"\sqrt[", r"\\"] {
            let _ = layout(src, 16.0, true, &m);
        }
    }
}
