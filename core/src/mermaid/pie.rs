//! Pie charts: `"label" : value` slices, with a title and, with `showData`,
//! the values in the legend.

use super::{lines, Anchor, Drawing, Ink, Prim};

#[derive(Clone, Debug)]
pub struct Pie {
    pub title: String,
    pub show_data: bool,
    pub slices: Vec<(String, f64)>,
}

pub fn parse(src: &str) -> Option<Pie> {
    let mut pie = Pie { title: String::new(), show_data: false, slices: Vec::new() };
    for (i, l) in lines(src).enumerate() {
        let mut l = l;
        if i == 0 || l.starts_with("pie") {
            let Some(rest) = l.strip_prefix("pie") else { continue };
            l = rest.trim();
            if let Some(r) = l.strip_prefix("showData") {
                pie.show_data = true;
                l = r.trim();
            }
            if l.is_empty() {
                continue;
            }
        }
        if let Some(t) = l.strip_prefix("title") {
            pie.title = t.trim().to_string();
            continue;
        }
        if let Some((label, value)) = l.rsplit_once(':') {
            let label = label.trim().trim_matches('"').to_string();
            if let Ok(v) = value.trim().parse::<f64>() {
                if v > 0.0 {
                    pie.slices.push((label, v));
                }
            }
        }
    }
    (!pie.slices.is_empty()).then_some(pie)
}

pub fn draw(pie: &Pie, measure: &dyn Fn(&str) -> (f32, f32)) -> Drawing {
    let total: f64 = pie.slices.iter().map(|s| s.1).sum();
    let (_, lh) = measure("Ag");
    let r = 110.0;
    let title_h = if pie.title.is_empty() { 0.0 } else { lh * 1.4 + 8.0 };
    let (cx, cy) = (r + 10.0, title_h + r + 10.0);
    let mut d = Drawing::default();
    let mut a = 0.0_f32;
    let full = std::f32::consts::TAU;
    for (i, (_, v)) in pie.slices.iter().enumerate() {
        let sweep = (*v / total) as f32 * full;
        d.prims.push(Prim::Slice { x: cx, y: cy, r, a0: a, a1: a + sweep, ink: Ink::Series(i) });
        a += sweep;
    }
    // the percentages inside the slices big enough to hold them
    let mut a = 0.0_f32;
    for (v_i, (_, v)) in pie.slices.iter().enumerate() {
        let sweep = (*v / total) as f32 * full;
        if sweep > 0.3 {
            let mid = a + sweep / 2.0;
            let (x, y) = (cx + mid.sin() * r * 0.66, cy - mid.cos() * r * 0.66);
            let pct = (*v / total * 100.0).round();
            d.prims.push(Prim::Text { x, y, text: format!("{pct}%"), anchor: Anchor::Center, ink: Ink::Solid, bold: true, small: true });
        }
        let _ = v_i;
        a += sweep;
    }
    d.prims.push(Prim::Circle { x: cx, y: cy, r, fill: None, stroke: Some(Ink::Back) });
    // the legend
    let lx = cx + r + 30.0;
    let n = pie.slices.len() as f32;
    let row = lh * 1.5;
    let mut ly = cy - row * n / 2.0;
    let mut width: f32 = lx;
    for (i, (label, v)) in pie.slices.iter().enumerate() {
        d.prims.push(Prim::Rect { x: lx, y: ly + row / 2.0 - 7.0, w: 14.0, h: 14.0, round: 3.0, fill: Some(Ink::Series(i)), stroke: None, dashed: false });
        let t = if pie.show_data { format!("{label} [{}]", trim_num(*v)) } else { label.clone() };
        width = width.max(lx + 24.0 + measure(&t).0);
        d.text(lx + 24.0, ly + row / 2.0, &t, Anchor::Left, Ink::Text);
        ly += row;
    }
    if !pie.title.is_empty() {
        width = width.max(measure(&pie.title).0 + 20.0);
        d.prims.push(Prim::Text { x: width / 2.0, y: lh * 0.7, text: pie.title.clone(), anchor: Anchor::Center, ink: Ink::Text, bold: true, small: false });
    }
    d.width = width + 10.0;
    d.height = (cy + r + 10.0).max(ly + 10.0);
    d
}

fn trim_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_add_to_a_turn() {
        let p = parse("pie showData\n    title Animais\n    \"Cães\" : 386\n    \"Gatos\" : 85.5\n    \"Ratos\" : 15\n").unwrap();
        assert!(p.show_data);
        assert_eq!(p.title, "Animais");
        assert_eq!(p.slices.len(), 3);
        let d = draw(&p, &|t| (t.chars().count() as f32 * 7.0, 16.0));
        let sweep: f32 = d.prims.iter().filter_map(|x| if let Prim::Slice { a0, a1, .. } = x { Some(a1 - a0) } else { None }).sum();
        assert!((sweep - std::f32::consts::TAU).abs() < 1e-3);
        assert!(parse("pie title Só título").is_none());
        assert_eq!(parse("pie title Pets\n\"A\": 1").unwrap().title, "Pets");
    }
}
