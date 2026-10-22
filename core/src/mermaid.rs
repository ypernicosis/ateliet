//! Mermaid flowcharts (`graph` / `flowchart`): the nodes, their shapes and
//! the edges between them, laid out in ranks along the chart's direction.
//! Other kinds of diagram are not parsed (the app shows their source).

use std::collections::HashMap;

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
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
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
    pub arrow: bool,
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

/// A flowchart, or `None` when the source is another kind of diagram.
pub fn parse(src: &str) -> Option<Chart> {
    let mut lines = src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("%%"));
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
            chart.nodes.push(Node { id: id.clone(), label: id.clone(), shape: Shape::Rect });
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
                            chart.edges.push(Edge { from: a, to: b, label: label.clone(), line, arrow });
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
            let (tw, th) = measure(&nd.label);
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
        let l = layout(&c, &|s| (s.chars().count() as f32 * 7.0, 16.0));
        // A above B above C; D below C
        assert!(l.boxes[0].1 < l.boxes[1].1 && l.boxes[1].1 < l.boxes[2].1 && l.boxes[2].1 < l.boxes[3].1);
        assert!(parse("sequenceDiagram\nA->>B: oi").is_none());
        // a cycle does not hang
        let cy = parse("flowchart LR\nA-->B-->C-->A").unwrap();
        let l = layout(&cy, &|_| (20.0, 10.0));
        assert!(l.boxes[0].0 < l.boxes[1].0 && l.boxes[1].0 < l.boxes[2].0);
    }
}
