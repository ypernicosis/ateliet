//! Mermaid diagrams. Flowcharts (`graph` / `flowchart`), state diagrams and
//! class diagrams are graphs: nodes and edges laid out in ranks along the
//! chart's direction. Sequence diagrams, pie charts and Gantt charts have
//! layouts of their own (`sequence`, `pie`, `gantt`). Every kind ends as a
//! `Drawing`: shapes, lines and text with semantic inks the app colours.

use std::collections::HashMap;

pub mod gantt;
pub mod pie;
pub mod sequence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Circle,
    Diamond,
    Hexagon,
    Subroutine,
    Cylinder,
    Flag,
    /// A state diagram's start (`[*]` before an arrow): a filled dot.
    Start,
    /// Its end (`[*]` after an arrow): a dot in a ring.
    End,
    /// A class: its name over its attributes and methods.
    Class,
    /// A fork or join bar of a state diagram.
    Bar,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
    /// A class's members, or a state's description lines.
    pub members: Vec<String>,
}

impl Node {
    fn new(id: &str) -> Node {
        Node { id: id.to_string(), label: id.to_string(), shape: Shape::Rect, members: Vec::new() }
    }
}

/// The mark at one end of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    None,
    Arrow,
    /// Inheritance or realization: a hollow triangle.
    Triangle,
    /// Composition: a filled diamond.
    Diamond,
    /// Aggregation: a hollow diamond.
    HollowDiamond,
    /// A message lost: a cross.
    Cross,
    /// An open arrow (asynchronous message).
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Solid,
    Dotted,
    Thick,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub line: Line,
    /// The mark where the edge arrives, and where it leaves.
    pub head: Head,
    pub tail: Head,
}

#[derive(Clone, Debug)]
pub struct Chart {
    /// "TD" (top-down), "BT", "LR" or "RL".
    pub dir: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Where everything goes: node boxes (x, y, w, h) and the size of the whole.
#[derive(Clone, Debug)]
pub struct Layout {
    pub boxes: Vec<(f32, f32, f32, f32)>,
    pub width: f32,
    pub height: f32,
}

const SHAPES: [(&str, &str, Shape); 10] = [
    ("([", "])", Shape::Stadium),
    ("((", "))", Shape::Circle),
    ("[[", "]]", Shape::Subroutine),
    ("[(", ")]", Shape::Cylinder),
    ("{{", "}}", Shape::Hexagon),
    ("[", "]", Shape::Rect),
    ("(", ")", Shape::Round),
    ("{", "}", Shape::Diamond),
    (">", "]", Shape::Flag),
    ("[/", "/]", Shape::Rect),
];

fn clean_label(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(s);
    s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n")
}

/// A node reference at the start of `s`: (id, label and shape if given, rest).
fn node_ref(s: &str) -> Option<(String, Option<(String, Shape)>, &str)> {
    let s = s.trim_start();
    let id_len = s.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.')).unwrap_or(s.len());
    // a dash run starts an edge, not an id
    let mut id_len = id_len;
    if let Some(k) = s[..id_len].find("--").or_else(|| s[..id_len].find("-.")) {
        id_len = k;
    }
    let id = s[..id_len].trim_end_matches('-').to_string();
    if id.is_empty() {
        return None;
    }
    let rest = &s[id.len()..];
    for (open, close, shape) in SHAPES {
        if let Some(after) = rest.strip_prefix(open) {
            if let Some(end) = after.find(close) {
                return Some((id, Some((clean_label(&after[..end]), shape)), &after[end + close.len()..]));
            }
        }
    }
    Some((id, None, rest))
}

/// An edge at the start of `s`: (label, line, arrow, rest).
fn edge(s: &str) -> Option<(String, Line, bool, &str)> {
    let s = s.trim_start();
    // `-- text -->` and `-. text .->` and `== text ==>`
    for (open, closes, line) in [("--", ["-->", "---"], Line::Solid), ("-.", [".->", ".-"], Line::Dotted), ("==", ["==>", "==="], Line::Thick)] {
        if let Some(after) = s.strip_prefix(open) {
            let after_t = after.trim_start();
            if !after_t.starts_with(['-', '>', '.', '=']) {
                for c in closes {
                    if let Some(k) = after.find(c) {
                        let label = clean_label(&after[..k]);
                        return Some((label, line, c.ends_with('>'), &after[k + c.len()..]));
                    }
                }
            }
        }
    }
    let ops: [(&str, Line, bool); 9] = [("-.->", Line::Dotted, true), ("-.-", Line::Dotted, false), ("==>", Line::Thick, true), ("===", Line::Thick, false), ("--->", Line::Solid, true), ("-->", Line::Solid, true), ("---", Line::Solid, false), ("--o", Line::Solid, false), ("--x", Line::Solid, false)];
    for (op, line, arrow) in ops {
        if let Some(mut rest) = s.strip_prefix(op) {
            // longer arrows (`---->`) are the same edge
            rest = rest.trim_start_matches(['-', '=', '.']).trim_start_matches('>');
            let mut label = String::new();
            let r = rest.trim_start();
            if let Some(after) = r.strip_prefix('|') {
                if let Some(k) = after.find('|') {
                    label = clean_label(&after[..k]);
                    return Some((label, line, arrow, &after[k + 1..]));
                }
            }
            return Some((std::mem::take(&mut label), line, arrow, rest));
        }
    }
    None
}

/// Any diagram Mermaid draws that the app knows.
#[derive(Clone, Debug)]
pub enum Diagram {
    /// Flowcharts, state and class diagrams.
    Chart(Chart),
    Sequence(sequence::Sequence),
    Pie(pie::Pie),
    Gantt(gantt::Gantt),
}

/// The source's lines without blanks and `%%` comments.
fn lines(src: &str) -> impl Iterator<Item = &str> {
    src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"))
}

/// The first word of the source: the kind of diagram (front matter and
/// `%%{init}%%` directives skipped).
fn kind_of(src: &str) -> Option<&str> {
    let mut it = lines(src).peekable();
    if it.peek() == Some(&"---") {
        it.next();
        for l in it.by_ref() {
            if l == "---" {
                break;
            }
        }
    }
    it.next()?.split_whitespace().next()
}

/// A diagram, or `None` when its kind is not one the app draws.
pub fn diagram(src: &str) -> Option<Diagram> {
    match kind_of(src)? {
        "graph" | "flowchart" => parse(src).map(Diagram::Chart),
        "stateDiagram" | "stateDiagram-v2" => state(src).map(Diagram::Chart),
        "classDiagram" | "classDiagram-v2" => class(src).map(Diagram::Chart),
        "sequenceDiagram" => sequence::parse(src).map(Diagram::Sequence),
        "pie" => pie::parse(src).map(Diagram::Pie),
        "gantt" => gantt::parse(src).map(Diagram::Gantt),
        _ => None,
    }
}

/// A diagram drawn: `measure` gives the size of a line of text, `width`
/// the room there is (charts wider than that are scaled down by the app).
pub fn draw(d: &Diagram, measure: &dyn Fn(&str) -> (f32, f32), width: f32) -> Drawing {
    match d {
        Diagram::Chart(c) => chart_drawing(c, measure),
        Diagram::Sequence(s) => sequence::draw(s, measure),
        Diagram::Pie(p) => pie::draw(p, measure),
        Diagram::Gantt(g) => gantt::draw(g, measure, width),
    }
}

/// The source after its header line (front matter skipped).
fn body(src: &str) -> Vec<&str> {
    let mut it = lines(src).peekable();
    if it.peek() == Some(&"---") {
        it.next();
        for l in it.by_ref() {
            if l == "---" {
                break;
            }
        }
    }
    it.next();
    it.collect()
}

/// The direction set by a `direction XX` line, or `dflt`.
fn direction(lines: &[&str], dflt: &str) -> String {
    lines.iter().find_map(|l| l.strip_prefix("direction ")).map(|d| d.trim()).filter(|d| ["LR", "RL", "TB", "TD", "BT"].contains(d)).map(|d| if d == "TB" { "TD" } else { d }).unwrap_or(dflt).to_string()
}

/// A state diagram as a chart: states as rounded boxes, `[*]` as the start
/// and end dots, transitions with their labels. Composite states are
/// flattened into their states.
pub fn state(src: &str) -> Option<Chart> {
    let body = body(src);
    let mut chart = Chart { dir: direction(&body, "TD"), nodes: Vec::new(), edges: Vec::new() };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut get = |chart: &mut Chart, id: &str| -> usize {
        *index.entry(id.to_string()).or_insert_with(|| {
            let mut n = Node::new(id);
            n.shape = match id {
                "[*]start" => Shape::Start,
                "[*]end" => Shape::End,
                _ => Shape::Round,
            };
            if n.shape != Shape::Round {
                n.label.clear();
            }
            chart.nodes.push(n);
            chart.nodes.len() - 1
        })
    };
    let mut in_note = false;
    for l in body {
        if in_note {
            in_note = !l.starts_with("end note");
            continue;
        }
        if l.starts_with("note ") {
            in_note = !l.contains(':');
            continue;
        }
        if l == "}" || l.starts_with("direction ") || l.starts_with("classDef ") || l.starts_with("class ") || l == "--" {
            continue;
        }
        if let Some(rest) = l.strip_prefix("state ") {
            let rest = rest.trim_end_matches('{').trim();
            // state "A long name" as S  /  state S <<choice>>
            if let Some(q) = rest.strip_prefix('"') {
                if let Some((label, after)) = q.split_once('"') {
                    if let Some(id) = after.trim().strip_prefix("as ") {
                        let i = get(&mut chart, id.trim());
                        chart.nodes[i].label = label.to_string();
                    }
                }
                continue;
            }
            let mut words = rest.split_whitespace();
            if let Some(id) = words.next() {
                let i = get(&mut chart, id);
                match words.next() {
                    Some("<<choice>>") => {
                        chart.nodes[i].shape = Shape::Diamond;
                        chart.nodes[i].label.clear();
                    }
                    Some("<<fork>>") | Some("<<join>>") => {
                        chart.nodes[i].shape = Shape::Bar;
                        chart.nodes[i].label.clear();
                    }
                    _ => {}
                }
            }
            continue;
        }
        if let Some((a, rest)) = l.split_once("-->") {
            let (b, label) = match rest.split_once(':') {
                Some((b, t)) => (b.trim(), t.trim().to_string()),
                None => (rest.trim(), String::new()),
            };
            let a = if a.trim() == "[*]" { "[*]start" } else { a.trim() };
            let b = if b == "[*]" { "[*]end" } else { b };
            let (x, y) = (get(&mut chart, a), get(&mut chart, b));
            chart.edges.push(Edge { from: x, to: y, label, line: Line::Solid, head: Head::Arrow, tail: Head::None });
            continue;
        }
        // S : a description
        if let Some((id, text)) = l.split_once(':') {
            let id = id.trim();
            if !id.is_empty() && !id.contains(' ') {
                let i = get(&mut chart, id);
                chart.nodes[i].members.push(text.trim().to_string());
            }
            continue;
        }
        let id = l.trim_end_matches('{').trim();
        if !id.is_empty() && !id.contains(' ') {
            get(&mut chart, id);
        }
    }
    (!chart.nodes.is_empty()).then_some(chart)
}

static CLASS_REL: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r#"^([A-Za-z_][\w~]*)\s*(?:"([^"]*)"\s*)?(<\||\*|o|<)?(--|\.\.)(\|>|\*|o|>)?\s*(?:"([^"]*)"\s*)?([A-Za-z_][\w~]*)\s*(?::\s*(.*))?$"#).unwrap()
});

/// A class diagram as a chart: classes as boxes with their members,
/// relations with UML's marks (inheritance, composition, aggregation…).
pub fn class(src: &str) -> Option<Chart> {
    let body = body(src);
    let mut chart = Chart { dir: direction(&body, "TD"), nodes: Vec::new(), edges: Vec::new() };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut get = |chart: &mut Chart, id: &str| -> usize {
        *index.entry(id.to_string()).or_insert_with(|| {
            let mut n = Node::new(id);
            n.shape = Shape::Class;
            // List~int~ is List<int>
            if let Some((name, t)) = id.split_once('~') {
                n.label = format!("{name}<{}>", t.trim_end_matches('~'));
            }
            chart.nodes.push(n);
            chart.nodes.len() - 1
        })
    };
    let mut open: Option<usize> = None;
    for l in body {
        if let Some(i) = open {
            if l == "}" {
                open = None;
            } else {
                chart.nodes[i].members.push(l.to_string());
            }
            continue;
        }
        if l.starts_with("direction ") || l.starts_with("note") || l.starts_with("classDef ") || l.starts_with("style ") || l.starts_with("click ") || l.starts_with("link ") || l.starts_with("callback ") || l.starts_with("cssClass ") {
            continue;
        }
        if let Some(rest) = l.strip_prefix("class ") {
            let braced = rest.trim_end().ends_with('{');
            let rest = rest.trim_end_matches('{').trim();
            let (id, label) = match rest.split_once('[') {
                Some((id, l)) => (id.trim(), Some(l.trim_end_matches(']').trim_matches('"').to_string())),
                None => (rest.split_whitespace().next().unwrap_or(""), None),
            };
            if id.is_empty() {
                continue;
            }
            let i = get(&mut chart, id);
            if let Some(label) = label {
                chart.nodes[i].label = label;
            }
            if let Some(ann) = rest.split_once("<<").map(|(_, a)| a.trim_end_matches(">>")) {
                chart.nodes[i].members.insert(0, format!("«{ann}»"));
            }
            if braced {
                open = Some(i);
            }
            continue;
        }
        // <<interface>> Shape
        if let Some(rest) = l.strip_prefix("<<") {
            if let Some((ann, id)) = rest.split_once(">>") {
                let i = get(&mut chart, id.trim());
                chart.nodes[i].members.insert(0, format!("«{ann}»"));
            }
            continue;
        }
        if let Some(c) = CLASS_REL.captures(l) {
            let (a, b) = (get(&mut chart, &c[1]), get(&mut chart, &c[7]));
            let mark = |m: Option<regex::Match>| match m.map(|m| m.as_str()) {
                Some("<|") | Some("|>") => Head::Triangle,
                Some("*") => Head::Diamond,
                Some("o") => Head::HollowDiamond,
                Some("<") | Some(">") => Head::Arrow,
                _ => Head::None,
            };
            let mut label = c.get(8).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
            // cardinalities read as part of the label: "1 … *"
            let card = match (c.get(2), c.get(6)) {
                (Some(x), Some(y)) => format!("{} … {}", x.as_str(), y.as_str()),
                (Some(x), None) | (None, Some(x)) => x.as_str().to_string(),
                _ => String::new(),
            };
            if !card.is_empty() {
                label = if label.is_empty() { card } else { format!("{label} ({card})") };
            }
            let line = if &c[4] == ".." { Line::Dotted } else { Line::Solid };
            chart.edges.push(Edge { from: a, to: b, label, line, head: mark(c.get(5)), tail: mark(c.get(3)) });
            continue;
        }
        // Name : +member
        if let Some((id, m)) = l.split_once(" : ").or_else(|| l.split_once(':')) {
            let id = id.trim();
            if !id.is_empty() && !id.contains(' ') {
                let i = get(&mut chart, id);
                chart.nodes[i].members.push(m.trim().to_string());
            }
        }
    }
    (!chart.nodes.is_empty()).then_some(chart)
}

/// A flowchart, or `None` when the source is another kind of diagram.
pub fn parse(src: &str) -> Option<Chart> {
    let mut lines = lines(src);
    let head = lines.next()?;
    let mut words = head.split_whitespace();
    let kind = words.next()?;
    if kind != "graph" && kind != "flowchart" {
        return None;
    }
    let dir = match words.next().unwrap_or("TD").trim_end_matches(';') {
        "LR" => "LR",
        "RL" => "RL",
        "BT" => "BT",
        _ => "TD",
    }
    .to_string();
    let mut chart = Chart { dir, nodes: Vec::new(), edges: Vec::new() };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut get = |chart: &mut Chart, id: String, def: Option<(String, Shape)>| -> usize {
        let i = *index.entry(id.clone()).or_insert_with(|| {
            chart.nodes.push(Node::new(&id));
            chart.nodes.len() - 1
        });
        if let Some((label, shape)) = def {
            chart.nodes[i].label = label;
            chart.nodes[i].shape = shape;
        }
        i
    };
    for line in lines {
        for stmt in line.split(';') {
            let stmt = stmt.trim();
            if stmt.is_empty() || ["subgraph", "end", "style", "classDef", "class", "click", "linkStyle", "direction"].iter().any(|k| stmt == *k || stmt.starts_with(&format!("{k} "))) {
                continue;
            }
            // a chain: A --> B -- text --> C, with `&` for several at once
            let mut rest = stmt;
            let mut prev: Vec<usize> = Vec::new();
            let mut pending: Option<(String, Line, bool)> = None;
            loop {
                let mut group = Vec::new();
                loop {
                    let Some((id, def, r)) = node_ref(rest) else { break };
                    group.push(get(&mut chart, id, def));
                    rest = r.trim_start();
                    match rest.strip_prefix('&') {
                        Some(r) => rest = r,
                        None => break,
                    }
                }
                if group.is_empty() {
                    break;
                }
                if let Some((label, line, arrow)) = pending.take() {
                    for &a in &prev {
                        for &b in &group {
                            chart.edges.push(Edge { from: a, to: b, label: label.clone(), line, head: if arrow { Head::Arrow } else { Head::None }, tail: Head::None });
                        }
                    }
                }
                prev = group;
                match edge(rest) {
                    Some((label, line, arrow, r)) => {
                        pending = Some((label, line, arrow));
                        rest = r;
                    }
                    None => break,
                }
            }
        }
    }
    (!chart.nodes.is_empty()).then_some(chart)
}

/// Ranks along the direction (longest path from the sources, cycles cut),
/// nodes in a rank ordered to keep edges short, sizes from `measure`.
pub fn layout(chart: &Chart, measure: &dyn Fn(&str) -> (f32, f32)) -> Layout {
    let n = chart.nodes.len();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for e in &chart.edges {
        if e.from != e.to {
            out[e.from].push(e.to);
        }
    }
    // cut cycles: an edge back to a node on the current path is ignored
    let mut state = vec![0u8; n];
    let mut keep: Vec<Vec<usize>> = vec![Vec::new(); n];
    fn dfs(v: usize, out: &[Vec<usize>], state: &mut [u8], keep: &mut [Vec<usize>]) {
        state[v] = 1;
        for &w in &out[v] {
            if state[w] == 1 {
                continue;
            }
            keep[v].push(w);
            if state[w] == 0 {
                dfs(w, out, state, keep);
            }
        }
        state[v] = 2;
    }
    for v in 0..n {
        if state[v] == 0 {
            dfs(v, &out, &mut state, &mut keep);
        }
    }
    for v in 0..n {
        for &w in &keep[v] {
            indeg[w] += 1;
        }
    }
    // longest path ranks (Kahn's order)
    let mut rank = vec![0usize; n];
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut deg = indeg.clone();
    let mut qi = 0;
    while qi < queue.len() {
        let v = queue[qi];
        qi += 1;
        for &w in &keep[v] {
            rank[w] = rank[w].max(rank[v] + 1);
            deg[w] -= 1;
            if deg[w] == 0 {
                queue.push(w);
            }
        }
    }
    let ranks = rank.iter().copied().max().unwrap_or(0) + 1;
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    for v in 0..n {
        layers[rank[v]].push(v);
    }
    // order by the average place of the parents, two sweeps
    let mut pos = vec![0.0f32; n];
    for l in &layers {
        for (k, &v) in l.iter().enumerate() {
            pos[v] = k as f32;
        }
    }
    let parents: Vec<Vec<usize>> = (0..n).map(|w| (0..n).filter(|&v| keep[v].contains(&w)).collect()).collect();
    for _ in 0..2 {
        for l in layers.iter_mut().skip(1) {
            l.sort_by(|&a, &b| {
                let ba = if parents[a].is_empty() { pos[a] } else { parents[a].iter().map(|&p| pos[p]).sum::<f32>() / parents[a].len() as f32 };
                let bb = if parents[b].is_empty() { pos[b] } else { parents[b].iter().map(|&p| pos[p]).sum::<f32>() / parents[b].len() as f32 };
                ba.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
            });
            for (k, &v) in l.iter().enumerate() {
                pos[v] = k as f32;
            }
        }
    }
    // sizes
    let sizes: Vec<(f32, f32)> = chart
        .nodes
        .iter()
        .map(|nd| {
            match nd.shape {
                Shape::Start | Shape::End => return (18.0, 18.0),
                Shape::Bar => return if chart.dir == "LR" || chart.dir == "RL" { (8.0, 64.0) } else { (64.0, 8.0) },
                Shape::Class => {
                    let (tw, th) = measure(&nd.label);
                    let mw = nd.members.iter().map(|m| measure(m).0).fold(0.0, f32::max);
                    let rows = nd.members.len() as f32;
                    let attrs = nd.members.iter().any(|m| !m.contains('(') && !m.starts_with('«'));
                    let methods = nd.members.iter().any(|m| m.contains('('));
                    let sections = attrs as u8 as f32 + methods as u8 as f32;
                    return ((tw.max(mw) + 28.0).max(90.0), th + 16.0 + rows * th * 1.15 + sections * 10.0);
                }
                _ => {}
            }
            let (tw, mut th) = measure(&nd.label);
            let mut tw = tw;
            for m in &nd.members {
                let (w, h) = measure(m);
                tw = tw.max(w);
                th += h * 1.1;
            }
            let (w, h) = (tw + 28.0, th + 18.0);
            match nd.shape {
                Shape::Diamond => (w * 1.5, h * 1.6),
                Shape::Circle => {
                    let d = w.max(h) + 6.0;
                    (d, d)
                }
                Shape::Hexagon => (w + 20.0, h),
                _ => (w.max(48.0), h),
            }
        })
        .collect();
    let horizontal = chart.dir == "LR" || chart.dir == "RL";
    let (gap_main, gap_cross) = (56.0f32, 28.0f32);
    // along the direction: each rank as thick as its biggest node
    let thick: Vec<f32> = layers.iter().map(|l| l.iter().map(|&v| if horizontal { sizes[v].0 } else { sizes[v].1 }).fold(0.0, f32::max)).collect();
    let widths: Vec<f32> = layers.iter().map(|l| l.iter().map(|&v| if horizontal { sizes[v].1 } else { sizes[v].0 }).sum::<f32>() + gap_cross * (l.len().saturating_sub(1)) as f32).collect();
    let cross_total = widths.iter().copied().fold(0.0, f32::max);
    let mut boxes = vec![(0.0, 0.0, 0.0, 0.0); n];
    let mut main = 0.0;
    for (r, l) in layers.iter().enumerate() {
        let mut cross = (cross_total - widths[r]) / 2.0;
        for &v in l {
            let (w, h) = sizes[v];
            let (along, across) = if horizontal { (w, h) } else { (h, w) };
            let m = main + (thick[r] - along) / 2.0;
            boxes[v] = if horizontal { (m, cross, w, h) } else { (cross, m, w, h) };
            cross += across + gap_cross;
        }
        main += thick[r] + gap_main;
    }
    let main_total = (main - gap_main).max(0.0);
    let (width, height) = if horizontal { (main_total, cross_total) } else { (cross_total, main_total) };
    // reversed directions
    if chart.dir == "BT" {
        for b in &mut boxes {
            b.1 = height - b.1 - b.3;
        }
    }
    if chart.dir == "RL" {
        for b in &mut boxes {
            b.0 = width - b.0 - b.2;
        }
    }
    Layout { boxes, width, height }
}

// ── drawing ────────────────────────────────────────────────────────────────

/// What a mark is, for the app to colour it from its theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// Edges, lifelines, axes.
    Line,
    /// Boxes: their fill and their outline.
    Fill,
    Stroke,
    /// The background of a label sitting on a line.
    Back,
    Text,
    Muted,
    /// Notes (sequence diagrams).
    Note,
    /// The frames of loops and alternatives, grid lines.
    Faint,
    /// A colour of a series (pie slices, Gantt sections).
    Series(usize),
    /// Gantt tasks: done, active, critical.
    Done,
    Active,
    Crit,
    /// Filled marks (start dots, arrowheads that are solid).
    Solid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Rect { x: f32, y: f32, w: f32, h: f32, round: f32, fill: Option<Ink>, stroke: Option<Ink>, dashed: bool },
    /// A closed shape (diamonds, hexagons…).
    Poly { pts: Vec<(f32, f32)>, fill: Option<Ink>, stroke: Option<Ink> },
    Circle { x: f32, y: f32, r: f32, fill: Option<Ink>, stroke: Option<Ink> },
    Line { pts: Vec<(f32, f32)>, ink: Ink, width: f32, dashed: bool },
    /// A mark at `at`, pointing along `dir`.
    Head { at: (f32, f32), dir: (f32, f32), kind: Head, ink: Ink },
    /// Text centred vertically on `y`.
    Text { x: f32, y: f32, text: String, anchor: Anchor, ink: Ink, bold: bool, small: bool },
    /// A pie slice between two angles (radians, clockwise from twelve o'clock).
    Slice { x: f32, y: f32, r: f32, a0: f32, a1: f32, ink: Ink },
}

#[derive(Clone, Debug, Default)]
pub struct Drawing {
    pub prims: Vec<Prim>,
    pub width: f32,
    pub height: f32,
}

impl Drawing {
    pub fn text(&mut self, x: f32, y: f32, text: &str, anchor: Anchor, ink: Ink) {
        self.prims.push(Prim::Text { x, y, text: text.to_string(), anchor, ink, bold: false, small: false });
    }
}

/// Where the line from a box's centre toward `toward` leaves the box.
fn exit(b: (f32, f32, f32, f32), toward: (f32, f32)) -> (f32, f32) {
    let c = (b.0 + b.2 / 2.0, b.1 + b.3 / 2.0);
    let d = (toward.0 - c.0, toward.1 - c.1);
    if d.0.abs() < 1e-3 && d.1.abs() < 1e-3 {
        return c;
    }
    let tx = if d.0.abs() > 1e-3 { (b.2 / 2.0) / d.0.abs() } else { f32::INFINITY };
    let ty = if d.1.abs() > 1e-3 { (b.3 / 2.0) / d.1.abs() } else { f32::INFINITY };
    let t = tx.min(ty);
    (c.0 + d.0 * t, c.1 + d.1 * t)
}

fn unit(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = (dx * dx + dy * dy).sqrt().max(1e-3);
    (dx / l, dy / l)
}

/// A graph chart drawn: boxes in their shapes, edges with their marks and
/// labels.
pub fn chart_drawing(chart: &Chart, measure: &dyn Fn(&str) -> (f32, f32)) -> Drawing {
    let lay = layout(chart, measure);
    let mut d = Drawing { prims: Vec::new(), width: lay.width, height: lay.height };
    let center = |b: (f32, f32, f32, f32)| (b.0 + b.2 / 2.0, b.1 + b.3 / 2.0);
    for (ei, e) in chart.edges.iter().enumerate() {
        let (a, b) = (lay.boxes[e.from], lay.boxes[e.to]);
        let (ca, cb) = (center(a), center(b));
        let dir = unit(ca, cb);
        let perp = (-dir.1, dir.0);
        // two edges between the same boxes (there and back) side by side
        // (the side is in the edge's own frame: the edge back has the opposite one)
        let twin = chart.edges.iter().enumerate().any(|(j, f)| j != ei && f.from == e.to && f.to == e.from);
        let side = if twin { 7.0 } else { 0.0 };
        // boxes in the way bend the edge around them
        let mut bend: f32 = 0.0;
        for (i, bx) in lay.boxes.iter().enumerate() {
            if i == e.from || i == e.to {
                continue;
            }
            let c = center(*bx);
            let t = ((c.0 - ca.0) * dir.0 + (c.1 - ca.1) * dir.1) / ((cb.0 - ca.0) * dir.0 + (cb.1 - ca.1) * dir.1).max(1e-3);
            let off = (c.0 - ca.0) * perp.0 + (c.1 - ca.1) * perp.1;
            let half = (bx.2 * perp.0.abs() + bx.3 * perp.1.abs()) / 2.0;
            if t > 0.0 && t < 1.0 && off.abs() < half + 6.0 {
                bend = bend.max(half + off.abs() + 24.0);
            }
        }
        let shift = |q: (f32, f32), k: f32| (q.0 + perp.0 * k, q.1 + perp.1 * k);
        let w = if e.line == Line::Thick { 2.6 } else { 1.3 };
        let (pts, label_at) = if bend > 0.0 {
            // out to the side and back: a curve through a point beside the boxes
            let k = bend;
            let ctrl = shift(((ca.0 + cb.0) / 2.0, (ca.1 + cb.1) / 2.0), k * 2.0);
            let pa = exit(a, ctrl);
            let pb = exit(b, ctrl);
            let pts: Vec<(f32, f32)> = (0..=16).map(|i| {
                let t = i as f32 / 16.0;
                let u = 1.0 - t;
                (u * u * pa.0 + 2.0 * u * t * ctrl.0 + t * t * pb.0, u * u * pa.1 + 2.0 * u * t * ctrl.1 + t * t * pb.1)
            }).collect();
            let mid = pts[8];
            (pts, mid)
        } else {
            let (pa, pb) = (shift(exit(a, cb), side), shift(exit(b, ca), side));
            (vec![pa, pb], ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0))
        };
        let (pa, pb) = (pts[0], pts[pts.len() - 1]);
        let (before_b, after_a) = (pts[pts.len() - 2], pts[1]);
        d.prims.push(Prim::Line { pts: pts.clone(), ink: Ink::Line, width: w, dashed: e.line == Line::Dotted });
        if e.head != Head::None {
            d.prims.push(Prim::Head { at: pb, dir: unit(before_b, pb), kind: e.head, ink: Ink::Line });
        }
        if e.tail != Head::None {
            d.prims.push(Prim::Head { at: pa, dir: unit(after_a, pa), kind: e.tail, ink: Ink::Line });
        }
        if !e.label.is_empty() {
            let mid = if twin && bend == 0.0 { shift(label_at, side * 3.0) } else { label_at };
            let (tw, th) = measure(&e.label);
            let (tw, th) = (tw * 0.9, th * 0.9);
            d.prims.push(Prim::Rect { x: mid.0 - tw / 2.0 - 4.0, y: mid.1 - th / 2.0 - 2.0, w: tw + 8.0, h: th + 4.0, round: 4.0, fill: Some(Ink::Back), stroke: None, dashed: false });
            d.prims.push(Prim::Text { x: mid.0, y: mid.1, text: e.label.clone(), anchor: Anchor::Center, ink: Ink::Muted, bold: false, small: true });
        }
    }
    for (i, nd) in chart.nodes.iter().enumerate() {
        let (x, y, w, h) = lay.boxes[i];
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (fill, stroke) = (Some(Ink::Fill), Some(Ink::Stroke));
        match nd.shape {
            Shape::Diamond => d.prims.push(Prim::Poly { pts: vec![(cx, y), (x + w, cy), (cx, y + h), (x, cy)], fill, stroke }),
            Shape::Circle => d.prims.push(Prim::Circle { x: cx, y: cy, r: w.min(h) / 2.0, fill, stroke }),
            Shape::Hexagon => {
                let k = h * 0.3;
                d.prims.push(Prim::Poly { pts: vec![(x + k, y), (x + w - k, y), (x + w, cy), (x + w - k, y + h), (x + k, y + h), (x, cy)], fill, stroke });
            }
            Shape::Flag => {
                let k = h * 0.3;
                d.prims.push(Prim::Poly { pts: vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x + k, cy)], fill, stroke });
            }
            Shape::Round => d.prims.push(Prim::Rect { x, y, w, h, round: 8.0, fill, stroke, dashed: false }),
            Shape::Stadium | Shape::Cylinder => d.prims.push(Prim::Rect { x, y, w, h, round: h / 2.0, fill, stroke, dashed: false }),
            Shape::Subroutine => {
                d.prims.push(Prim::Rect { x, y, w, h, round: 2.0, fill, stroke, dashed: false });
                for vx in [x + 6.0, x + w - 6.0] {
                    d.prims.push(Prim::Line { pts: vec![(vx, y), (vx, y + h)], ink: Ink::Stroke, width: 1.3, dashed: false });
                }
            }
            Shape::Rect => d.prims.push(Prim::Rect { x, y, w, h, round: 3.0, fill, stroke, dashed: false }),
            Shape::Start => d.prims.push(Prim::Circle { x: cx, y: cy, r: 7.0, fill: Some(Ink::Solid), stroke: None }),
            Shape::End => {
                d.prims.push(Prim::Circle { x: cx, y: cy, r: 8.5, fill: None, stroke: Some(Ink::Solid) });
                d.prims.push(Prim::Circle { x: cx, y: cy, r: 5.0, fill: Some(Ink::Solid), stroke: None });
            }
            Shape::Bar => d.prims.push(Prim::Rect { x, y, w, h, round: 2.0, fill: Some(Ink::Solid), stroke: None, dashed: false }),
            Shape::Class => {
                d.prims.push(Prim::Rect { x, y, w, h, round: 3.0, fill, stroke, dashed: false });
                let (_, th) = measure(&nd.label);
                let row = th * 1.15;
                let mut at = y + 8.0;
                let notes: Vec<&String> = nd.members.iter().filter(|m| m.starts_with('«')).collect();
                for n in &notes {
                    d.prims.push(Prim::Text { x: cx, y: at + row / 2.0, text: (*n).clone(), anchor: Anchor::Center, ink: Ink::Muted, bold: false, small: true });
                    at += row;
                }
                d.prims.push(Prim::Text { x: cx, y: at + th / 2.0, text: nd.label.clone(), anchor: Anchor::Center, ink: Ink::Text, bold: true, small: false });
                at += th + 8.0;
                let attrs: Vec<&String> = nd.members.iter().filter(|m| !m.starts_with('«') && !m.contains('(')).collect();
                let methods: Vec<&String> = nd.members.iter().filter(|m| m.contains('(')).collect();
                for group in [attrs, methods] {
                    if group.is_empty() {
                        continue;
                    }
                    d.prims.push(Prim::Line { pts: vec![(x, at - 4.0), (x + w, at - 4.0)], ink: Ink::Stroke, width: 1.0, dashed: false });
                    for m in group {
                        d.prims.push(Prim::Text { x: x + 12.0, y: at + row / 2.0, text: m.clone(), anchor: Anchor::Left, ink: Ink::Text, bold: false, small: false });
                        at += row;
                    }
                    at += 10.0;
                }
                continue;
            }
        }
        if !nd.label.is_empty() {
            let (_, th) = measure(&nd.label);
            let extra: f32 = nd.members.iter().map(|m| measure(m).1 * 1.1).sum();
            let top = cy - (th + extra) / 2.0;
            d.prims.push(Prim::Text { x: cx, y: top + th / 2.0, text: nd.label.clone(), anchor: Anchor::Center, ink: Ink::Text, bold: false, small: false });
            let mut at = top + th;
            for m in &nd.members {
                let mh = measure(m).1 * 1.1;
                d.prims.push(Prim::Text { x: cx, y: at + mh / 2.0, text: m.clone(), anchor: Anchor::Center, ink: Ink::Muted, bold: false, small: true });
                at += mh;
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_parses_and_lays_out() {
        let src = "graph TD\n  A[Início] --> B{Decide?}\n  B -->|sim| C(Faz)\n  B -- não --> D((Fim))\n  C -.-> D\n  A & C --> E\n%% comentário\n";
        let c = parse(src).unwrap();
        assert_eq!(c.nodes.len(), 5);
        assert_eq!(c.nodes[1].shape, Shape::Diamond);
        assert_eq!(c.nodes[3].shape, Shape::Circle);
        assert_eq!(c.edges.len(), 6);
        assert_eq!(c.edges[1].label, "sim");
        assert_eq!(c.edges[2].label, "não");
        assert_eq!(c.edges[3].line, Line::Dotted);
        assert!(!chart_drawing(&c, &|s| (s.chars().count() as f32 * 7.0, 16.0)).prims.is_empty());
        let l = layout(&c, &|s| (s.chars().count() as f32 * 7.0, 16.0));
        // A above B above C; D below C
        assert!(l.boxes[0].1 < l.boxes[1].1 && l.boxes[1].1 < l.boxes[2].1 && l.boxes[2].1 < l.boxes[3].1);
        assert!(parse("sequenceDiagram\nA->>B: oi").is_none());
        assert_eq!(c.edges[0].head, Head::Arrow);
        // a cycle does not hang
        let cy = parse("flowchart LR\nA-->B-->C-->A").unwrap();
        let l = layout(&cy, &|_| (20.0, 10.0));
        assert!(l.boxes[0].0 < l.boxes[1].0 && l.boxes[1].0 < l.boxes[2].0);
    }

    #[test]
    fn state_diagrams() {
        let src = "stateDiagram-v2\n    [*] --> Still\n    Still --> [*]\n    Still --> Moving : empurrão\n    Moving --> Crash\n    state \"Batida feia\" as Crash\n    Crash : ai\n    Crash --> [*]\n    state c <<choice>>\n    note right of Still : uma nota\n";
        let Some(Diagram::Chart(c)) = diagram(src) else { panic!() };
        let names: Vec<&str> = c.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(names, ["[*]start", "Still", "[*]end", "Moving", "Crash", "c"]);
        assert_eq!(c.nodes[0].shape, Shape::Start);
        assert_eq!(c.nodes[2].shape, Shape::End);
        assert_eq!(c.nodes[4].label, "Batida feia");
        assert_eq!(c.nodes[4].members, ["ai"]);
        assert_eq!(c.nodes[5].shape, Shape::Diamond);
        assert_eq!(c.edges.len(), 5);
        assert_eq!(c.edges[2].label, "empurrão");
    }

    #[test]
    fn class_diagrams() {
        let src = "classDiagram\n    Animal <|-- Duck\n    Animal *-- \"1\" Leg : tem\n    Zoo o-- Animal\n    Animal : +int age\n    Animal: +isMammal() bool\n    class Duck{\n        +String beakColor\n        +swim()\n    }\n    class Shape\n    <<interface>> Shape\n    Duck ..|> Shape\n    class List~int~\n";
        let Some(Diagram::Chart(c)) = diagram(src) else { panic!() };
        let animal = c.nodes.iter().position(|n| n.id == "Animal").unwrap();
        assert_eq!(c.nodes[animal].members, ["+int age", "+isMammal() bool"]);
        let duck = c.nodes.iter().find(|n| n.id == "Duck").unwrap();
        assert_eq!(duck.members, ["+String beakColor", "+swim()"]);
        let shape = c.nodes.iter().find(|n| n.id == "Shape").unwrap();
        assert_eq!(shape.members, ["«interface»"]);
        assert_eq!(c.nodes.iter().find(|n| n.id == "List~int~").unwrap().label, "List<int>");
        assert_eq!(c.edges[0].tail, Head::Triangle);
        assert_eq!(c.edges[1].tail, Head::Diamond);
        assert!(c.edges[1].label.contains('1') && c.edges[1].label.contains("tem"));
        assert_eq!(c.edges[2].tail, Head::HollowDiamond);
        assert_eq!((c.edges[3].line, c.edges[3].head), (Line::Dotted, Head::Triangle));
        // the parent is drawn above its child
        let l = layout(&c, &|s| (s.chars().count() as f32 * 7.0, 16.0));
        let duck_i = c.nodes.iter().position(|n| n.id == "Duck").unwrap();
        assert!(l.boxes[animal].1 < l.boxes[duck_i].1);
    }
}
