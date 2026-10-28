//! Gantt charts: sections of tasks on a time axis. Tasks start on a date or
//! after others and last until a date or for a duration; `done`, `active`,
//! `crit` and `milestone` change how they are drawn.

use super::{lines, Anchor, Drawing, Ink, Prim};
use chrono::{NaiveDate, NaiveDateTime};

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub name: String,
    pub id: String,
    pub section: usize,
    /// Days since 1970-01-01 (fractions are hours and minutes).
    pub start: f64,
    pub end: f64,
    pub done: bool,
    pub active: bool,
    pub crit: bool,
    pub milestone: bool,
}

#[derive(Clone, Debug)]
pub struct Gantt {
    pub title: String,
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
    /// The axis labels' format (strftime, as Mermaid's `axisFormat`).
    pub axis: Option<String>,
}

/// Mermaid's `dateFormat` (Moment tokens) as a strftime pattern.
fn strftime(moment: &str) -> String {
    let mut out = moment.to_string();
    for (a, b) in [("YYYY", "%Y"), ("YY", "%y"), ("MM", "%m"), ("DD", "%d"), ("HH", "%H"), ("mm", "%M"), ("ss", "%S")] {
        out = out.replace(a, b);
    }
    out
}

fn epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(1970, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap()
}

/// A date in the chart's format (or ISO), as days since the epoch.
fn date(s: &str, fmt: &str) -> Option<f64> {
    let s = s.trim();
    let t = NaiveDateTime::parse_from_str(s, fmt).ok().or_else(|| NaiveDate::parse_from_str(s, fmt).ok().and_then(|d| d.and_hms_opt(0, 0, 0))).or_else(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)))?;
    Some((t - epoch()).num_seconds() as f64 / 86400.0)
}

/// `3d`, `2w`, `12h`, `1.5d`… in days.
fn duration(s: &str) -> Option<f64> {
    let s = s.trim();
    let unit_at = s.find(|c: char| c.is_alphabetic())?;
    let n: f64 = s[..unit_at].trim().parse().ok()?;
    let k = match &s[unit_at..] {
        "ms" => 1.0 / 86_400_000.0,
        "s" => 1.0 / 86400.0,
        "m" => 1.0 / 1440.0,
        "h" => 1.0 / 24.0,
        "d" => 1.0,
        "w" => 7.0,
        "M" => 30.0,
        "y" => 365.0,
        _ => return None,
    };
    Some(n * k)
}

pub fn parse(src: &str) -> Option<Gantt> {
    let mut g = Gantt { title: String::new(), sections: Vec::new(), tasks: Vec::new(), axis: None };
    let mut fmt = "%Y-%m-%d".to_string();
    let mut prev_end: Option<f64> = None;
    for l in lines(src).skip(1) {
        let word = l.split_whitespace().next().unwrap_or("");
        let rest = l[word.len()..].trim();
        match word {
            "title" => g.title = rest.to_string(),
            "dateFormat" => fmt = strftime(rest),
            "axisFormat" => g.axis = Some(rest.to_string()),
            "section" => g.sections.push(rest.to_string()),
            "excludes" | "includes" | "todayMarker" | "tickInterval" | "weekday" | "inclusiveEndDates" | "topAxis" | "displayMode" | "click" => {}
            _ => {
                let Some((name, meta)) = l.split_once(':') else { continue };
                let mut t = Task { name: name.trim().to_string(), id: String::new(), section: g.sections.len().saturating_sub(1), start: 0.0, end: 0.0, done: false, active: false, crit: false, milestone: false };
                let mut items: Vec<&str> = meta.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
                while let Some(&tag) = items.first() {
                    match tag {
                        "done" => t.done = true,
                        "active" => t.active = true,
                        "crit" => t.crit = true,
                        "milestone" => t.milestone = true,
                        _ => break,
                    }
                    items.remove(0);
                }
                let start_of = |s: &str, g: &Gantt| -> Option<f64> {
                    if let Some(ids) = s.strip_prefix("after ") {
                        return ids.split_whitespace().filter_map(|id| g.tasks.iter().find(|t| t.id == id).map(|t| t.end)).reduce(f64::max);
                    }
                    date(s, &fmt)
                };
                let (start, end_s) = match items.len() {
                    0 => continue,
                    1 => (prev_end, items[0]),
                    2 => match start_of(items[0], &g) {
                        Some(s) => (Some(s), items[1]),
                        None => {
                            t.id = items[0].to_string();
                            (prev_end, items[1])
                        }
                    },
                    _ => {
                        t.id = items[0].to_string();
                        (start_of(items[1], &g), items[2])
                    }
                };
                let Some(start) = start.or(prev_end) else { continue };
                t.start = start;
                t.end = duration(end_s).map(|d| start + d).or_else(|| date(end_s, &fmt)).unwrap_or(start + 1.0);
                if t.milestone {
                    t.end = t.start;
                }
                prev_end = Some(t.end);
                g.tasks.push(t);
            }
        }
    }
    (!g.tasks.is_empty()).then_some(g)
}

/// Axis ticks: a step in days that leaves a handful of them.
fn tick_step(span: f64) -> f64 {
    for s in [1.0 / 24.0, 0.25, 1.0, 2.0, 7.0, 14.0, 30.0, 91.0, 182.0, 365.0, 730.0, 1825.0] {
        if span / s <= 9.0 {
            return s;
        }
    }
    3650.0
}

pub fn draw(g: &Gantt, measure: &dyn Fn(&str) -> (f32, f32), width: f32) -> Drawing {
    let (_, lh) = measure("Ag");
    let width = width.max(320.0);
    let label_w = g.sections.iter().map(|s| measure(s).0 * 0.9).fold(0.0, f32::max);
    let left = if g.sections.is_empty() { 8.0 } else { label_w + 20.0 };
    let right = width - 10.0;
    let t0 = g.tasks.iter().map(|t| t.start).fold(f64::INFINITY, f64::min);
    let t1 = g.tasks.iter().map(|t| t.end).fold(f64::NEG_INFINITY, f64::max).max(t0 + 1.0);
    let x = |t: f64| left + ((t - t0) / (t1 - t0)) as f32 * (right - left);
    let row = (lh * 1.5).max(24.0);
    let bar = row * 0.72;
    let mut d = Drawing::default();
    let top = if g.title.is_empty() { 4.0 } else { lh * 1.4 + 10.0 };
    if !g.title.is_empty() {
        d.prims.push(Prim::Text { x: width / 2.0, y: lh * 0.7, text: g.title.clone(), anchor: Anchor::Center, ink: Ink::Text, bold: true, small: false });
    }
    let bottom = top + row * g.tasks.len() as f32;
    // section bands under everything, the grid above them
    let mut i = 0;
    while i < g.tasks.len() {
        let s = g.tasks[i].section;
        let n = g.tasks[i..].iter().take_while(|t| t.section == s).count();
        let (y0, y1) = (top + row * i as f32, top + row * (i + n) as f32);
        if !g.sections.is_empty() {
            d.prims.push(Prim::Rect { x: 0.0, y: y0, w: width, h: y1 - y0, round: 0.0, fill: Some(if s % 2 == 0 { Ink::Back } else { Ink::Fill }), stroke: None, dashed: false });
            if let Some(name) = g.sections.get(s) {
                d.prims.push(Prim::Text { x: 8.0, y: (y0 + y1) / 2.0, text: name.clone(), anchor: Anchor::Left, ink: Ink::Muted, bold: true, small: true });
            }
        }
        i += n;
    }
    let step = tick_step(t1 - t0);
    let mut t = (t0 / step).ceil() * step;
    let fmt = g.axis.clone().unwrap_or_else(|| if step < 1.0 { "%H:%M".into() } else if t1 - t0 > 400.0 { "%Y".into() } else if step >= 28.0 { "%m/%Y".into() } else { "%d/%m".into() });
    while t <= t1 + 1e-9 {
        let xx = x(t);
        d.prims.push(Prim::Line { pts: vec![(xx, top), (xx, bottom)], ink: Ink::Faint, width: 1.0, dashed: false });
        let when = epoch() + chrono::Duration::seconds((t * 86400.0).round() as i64);
        let label = when.format(&fmt).to_string();
        d.prims.push(Prim::Text { x: xx, y: bottom + lh * 0.75, text: label, anchor: Anchor::Center, ink: Ink::Muted, bold: false, small: true });
        t += step;
    }
    for (i, task) in g.tasks.iter().enumerate() {
        let cy = top + row * (i as f32 + 0.5);
        let ink = if task.crit { Ink::Crit } else if task.done { Ink::Done } else if task.active { Ink::Active } else { Ink::Series(task.section) };
        if task.milestone {
            let (cx, r) = (x(task.start), bar * 0.5);
            d.prims.push(Prim::Poly { pts: vec![(cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy)], fill: Some(ink), stroke: Some(Ink::Stroke) });
            let tw = measure(&task.name).0 * 0.9;
            if cx + r + 6.0 + tw < right {
                d.prims.push(Prim::Text { x: cx + r + 6.0, y: cy, text: task.name.clone(), anchor: Anchor::Left, ink: Ink::Text, bold: false, small: true });
            } else {
                d.prims.push(Prim::Text { x: cx - r - 6.0, y: cy, text: task.name.clone(), anchor: Anchor::Right, ink: Ink::Text, bold: false, small: true });
            }
            continue;
        }
        let (x0, x1) = (x(task.start), x(task.end).max(x(task.start) + 2.0));
        d.prims.push(Prim::Rect { x: x0, y: cy - bar / 2.0, w: x1 - x0, h: bar, round: 3.0, fill: Some(ink), stroke: if task.active || task.crit { Some(Ink::Stroke) } else { None }, dashed: false });
        // the name inside the bar when it fits, else beside it
        let tw = measure(&task.name).0 * 0.9;
        if tw + 12.0 < x1 - x0 {
            d.prims.push(Prim::Text { x: (x0 + x1) / 2.0, y: cy, text: task.name.clone(), anchor: Anchor::Center, ink: Ink::Solid, bold: false, small: true });
        } else if x1 + 6.0 + tw < right {
            d.prims.push(Prim::Text { x: x1 + 6.0, y: cy, text: task.name.clone(), anchor: Anchor::Left, ink: Ink::Text, bold: false, small: true });
        } else {
            d.prims.push(Prim::Text { x: x0 - 6.0, y: cy, text: task.name.clone(), anchor: Anchor::Right, ink: Ink::Text, bold: false, small: true });
        }
    }
    d.width = width;
    d.height = bottom + lh * 1.6;
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_follow_each_other() {
        let src = "gantt\n    title Projeto\n    dateFormat YYYY-MM-DD\n    section A\n    Pesquisa :done, a1, 2024-01-01, 10d\n    Escrita :active, a2, after a1, 2w\n    section B\n    Revisão :crit, 5d\n    Entrega :milestone, m1, 2024-02-01, 0d\n    Fim :2024-02-02, 2024-02-05\n";
        let g = parse(src).unwrap();
        assert_eq!(g.title, "Projeto");
        assert_eq!(g.sections, ["A", "B"]);
        let t = &g.tasks;
        assert_eq!(t.len(), 5);
        assert!(t[0].done && t[1].active && t[2].crit && t[3].milestone);
        assert_eq!(t[1].start, t[0].end, "after a1");
        assert_eq!(t[1].end - t[1].start, 14.0);
        assert_eq!(t[2].start, t[1].end, "no start: after the previous task");
        assert_eq!(t[2].section, 1);
        assert_eq!(t[4].end - t[4].start, 3.0);
        assert_eq!(date("2024-01-01", "%Y-%m-%d"), Some(19723.0));
        let d = draw(&g, &|s| (s.chars().count() as f32 * 7.0, 16.0), 600.0);
        assert_eq!(d.width, 600.0);
        assert!(d.prims.iter().any(|p| matches!(p, Prim::Poly { .. })), "the milestone");
        let fmt = parse("gantt\ndateFormat DD/MM/YYYY\nx :a, 05/03/2024, 1d").unwrap();
        assert_eq!(fmt.tasks[0].start, date("2024-03-05", "%Y-%m-%d").unwrap());
    }
}
