//! Dataview results (from the core) drawn natively: tables, lists, task
//! lists, calendars, the element tree a dataviewjs block builds, errors and
//! inline values.

use crate::markdown::{self, Action, Renderer, Runs, Style};
use eframe::egui::{self, FontFamily, FontId, RichText, Stroke, TextFormat};
use serde_json::Value;

fn text_fmt(r: &Renderer, size: f32) -> TextFormat {
    let p = r.host.palette();
    TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color: p.text, ..Default::default() }
}

/// Markdown text inside a value (strings may hold links, emphasis…).
fn markdown_runs(r: &mut Renderer, runs: &mut Runs, s: &str, size: f32) {
    if !s.contains(['[', '*', '_', '`', '#', '=', '~', '<', '$']) && !s.contains("http") {
        let f = text_fmt(r, size);
        runs.push(s, f);
        return;
    }
    let doc = markdown::parse(s);
    let mut first = true;
    for b in &doc.blocks {
        if !first {
            runs.push(" ", text_fmt(r, size));
        }
        first = false;
        match b {
            markdown::Block::Para(inl) | markdown::Block::Heading { inlines: inl, .. } => r.runs(runs, inl, size, Style::default(), None),
            markdown::Block::Embed { target, .. } => {
                let p = r.host.palette();
                let f = TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color: p.link(), ..Default::default() };
                runs.link(crate::vault::basename(target), f, Action::Open(target.clone(), false));
            }
            _ => {
                let f = text_fmt(r, size);
                runs.push(s, f);
            }
        }
    }
}

/// One value appended to a run list (lists as comma-separated text).
pub fn value_runs(r: &mut Renderer, runs: &mut Runs, v: &Value, size: f32, _inline: bool) {
    let p = r.host.palette();
    match v {
        Value::Null => {
            let mut f = text_fmt(r, size);
            f.color = p.faint;
            runs.push("-", f);
        }
        Value::Bool(b) => runs.push(&b.to_string(), text_fmt(r, size)),
        Value::Number(n) => {
            let t = match n.as_f64() {
                Some(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{}", x as i64),
                Some(x) => format!("{x}"),
                None => n.to_string(),
            };
            runs.push(&t, text_fmt(r, size));
        }
        Value::String(s) => markdown_runs(r, runs, s, size),
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    runs.push(", ", text_fmt(r, size));
                }
                value_runs(r, runs, x, size, true);
            }
        }
        Value::Object(o) => match o.get("t").and_then(Value::as_str) {
            Some("link") => {
                let path = o.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
                let sub = o.get("subpath").and_then(Value::as_str);
                let target = match sub {
                    Some(s) => format!("{path}#{s}"),
                    None => path.clone(),
                };
                let label = o.get("display").and_then(Value::as_str).unwrap_or(&path).to_string();
                let ok = r.host.resolved(&path);
                let mut f = text_fmt(r, size);
                f.color = if ok { p.link() } else { p.link().gamma_multiply(0.6) };
                f.underline = Stroke::new(1.0_f32, f.color.gamma_multiply(0.5));
                runs.link(&label, f, Action::Open(target, false));
            }
            Some("date") | Some("dur") => runs.push(o.get("text").and_then(Value::as_str).unwrap_or_default(), text_fmt(r, size)),
            Some("obj") => {
                let entries = o.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
                for (i, e) in entries.iter().enumerate() {
                    if i > 0 {
                        runs.push(", ", text_fmt(r, size));
                    }
                    let k = e.get(0).and_then(Value::as_str).unwrap_or_default();
                    runs.push(&format!("{k}: "), text_fmt(r, size));
                    value_runs(r, runs, e.get(1).unwrap_or(&Value::Null), size, true);
                }
            }
            _ => {
                // an inline query result
                if let Some(t) = o.get("type").and_then(Value::as_str) {
                    match t {
                        "value" => value_runs(r, runs, o.get("value").unwrap_or(&Value::Null), size, true),
                        "error" => {
                            let mut f = text_fmt(r, size);
                            f.color = p.error;
                            let e = o.get("error").and_then(Value::as_str).unwrap_or_default();
                            runs.push(&format!("Dataview: {}", e.lines().next().unwrap_or_default()), f);
                        }
                        "js" => js_inline(r, runs, o.get("root").unwrap_or(&Value::Null), size),
                        _ => {}
                    }
                }
            }
        },
    }
}

fn js_inline(r: &mut Renderer, runs: &mut Runs, node: &Value, size: f32) {
    for c in node.get("children").and_then(Value::as_array).cloned().unwrap_or_default() {
        if let Some(s) = c.as_str() {
            runs.push(s, text_fmt(r, size));
        } else if let Some(el) = c.get("el") {
            if let Some(m) = el.get("markdown").filter(|m| !m.is_null()) {
                value_runs(r, runs, m, size, true);
            }
            if let Some(t) = el.get("text").filter(|m| !m.is_null()) {
                value_runs(r, runs, t, size, true);
            }
            js_inline(r, runs, el, size);
        }
    }
}

/// A value in its own cell: lists become bullet lines.
fn value_cell(r: &mut Renderer, ui: &mut egui::Ui, v: &Value, size: f32) {
    if let Value::Array(a) = v {
        if a.len() > 1 {
            for x in a {
                let mut runs = Runs::new();
                runs.push("• ", text_fmt(r, size));
                value_runs(r, &mut runs, x, size, true);
                if let Some(act) = runs.show_cell(ui) {
                    r.actions.push(act);
                }
            }
            return;
        }
    }
    let mut runs = Runs::new();
    value_runs(r, &mut runs, v, size, false);
    if let Some(act) = runs.show_cell(ui) {
        r.actions.push(act);
    }
}

fn empty(r: &Renderer, ui: &mut egui::Ui, kind: &str) {
    let p = r.host.palette();
    egui::Frame::new().stroke(Stroke::new(1.5_f32, p.border)).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| ui.label(RichText::new(format!("Dataview: No results to show for {kind} query.")).color(p.muted)));
    });
}

pub fn render(r: &mut Renderer, ui: &mut egui::Ui, res: &Value) {
    let size = r.host.size();
    let p = r.host.palette();
    match res.get("type").and_then(Value::as_str).unwrap_or_default() {
        "error" => {
            let e = res.get("error").and_then(Value::as_str).unwrap_or_default();
            egui::Frame::new().fill(p.error.gamma_multiply(0.08)).corner_radius(egui::CornerRadius::same(4)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(format!("Dataview: {e}")).monospace().size(size * 0.8).color(p.error));
            });
        }
        "table" => {
            let headers: Vec<String> = res["headers"].as_array().map(|h| h.iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default();
            let rows = res["rows"].as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                return empty(r, ui, "table");
            }
            r.in_cell = true;
            egui::ScrollArea::horizontal().id_salt(("dvtable", headers.join("|"), rows.len())).show(ui, |ui| {
                let mut ys: Vec<f32> = Vec::new();
                let g = egui::Grid::new(ui.next_auto_id()).striped(false).spacing([28.0, 9.0]).show(ui, |ui| {
                    for (i, h) in headers.iter().enumerate() {
                        let mut runs = Runs::new();
                        runs.push(h, TextFormat { font_id: FontId::new(size * 0.88, FontFamily::Name("bold".into())), color: p.muted, ..Default::default() });
                        if i == 0 {
                            runs.push(&format!("  {}", rows.len()), TextFormat { font_id: FontId::new(size * 0.75, FontFamily::Proportional), color: p.faint, ..Default::default() });
                        }
                        runs.show_cell(ui);
                    }
                    ui.end_row();
                    ys.push(ui.cursor().top() - 4.5);
                    for row in &rows {
                        for cell in row.as_array().cloned().unwrap_or_default() {
                            ui.vertical(|ui| value_cell(r, ui, &cell, size));
                        }
                        ui.end_row();
                        ys.push(ui.cursor().top() - 4.5);
                    }
                });
                let x = g.response.rect.x_range();
                let last = ys.len().saturating_sub(1);
                for (k, y) in ys.into_iter().enumerate() {
                    if k == last && k > 0 {
                        break;
                    }
                    ui.painter().hline(x, y, egui::Stroke::new(1.0_f32, if k == 0 { p.border_strong } else { p.border }));
                }
            });
            r.in_cell = false;
        }
        "list" => {
            let items = res["items"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                return empty(r, ui, "list");
            }
            for it in items {
                ui.horizontal_top(|ui| {
                    markdown::bullet(ui, size, p.faint, false);
                    ui.vertical(|ui| {
                        let id = &it["id"];
                        let val = &it["value"];
                        let mut runs = Runs::new();
                        if !id.is_null() {
                            value_runs(r, &mut runs, id, size, true);
                            if !val.is_null() && !val.is_array() {
                                runs.push(": ", text_fmt(r, size));
                                value_runs(r, &mut runs, val, size, true);
                            }
                        } else {
                            value_runs(r, &mut runs, val, size, true);
                        }
                        if let Some(a) = runs.show(ui) {
                            r.actions.push(a);
                        }
                        if !id.is_null() {
                            if let Value::Array(sub) = val {
                                for x in sub {
                                    ui.horizontal_top(|ui| {
                                        markdown::bullet(ui, size, p.faint, true);
                                        let mut runs = Runs::new();
                                        value_runs(r, &mut runs, x, size, true);
                                        if let Some(a) = runs.show(ui) {
                                            r.actions.push(a);
                                        }
                                    });
                                }
                            }
                        }
                    });
                });
            }
        }
        "task" => {
            let groups = res["groups"].as_array().cloned().unwrap_or_default();
            let total: usize = groups.iter().map(|g| g["tasks"].as_array().map(|t| t.len()).unwrap_or(0)).sum();
            if total == 0 {
                return empty(r, ui, "task");
            }
            for g in groups {
                let tasks = g["tasks"].as_array().cloned().unwrap_or_default();
                if tasks.is_empty() {
                    continue;
                }
                if !g["key"].is_null() {
                    ui.add_space(4.0);
                    let mut runs = Runs::new();
                    let bold = TextFormat { font_id: FontId::new(size * 1.05, FontFamily::Name("bold".into())), color: p.text, ..Default::default() };
                    let mut sub = Runs::new();
                    value_runs(r, &mut sub, &g["key"], size * 1.05, true);
                    for s in sub.job.sections.iter() {
                        let mut f = s.format.clone();
                        f.font_id = bold.font_id.clone();
                        let text = &sub.job.text[s.byte_range.clone()];
                        if let Some((_, a)) = sub.links.first() {
                            runs.link(text, f, a.clone());
                        } else {
                            runs.push(text, f);
                        }
                    }
                    runs.push(&format!(" ({})", tasks.len()), TextFormat { font_id: FontId::new(size * 0.8, FontFamily::Proportional), color: p.muted, ..Default::default() });
                    if let Some(a) = runs.show(ui) {
                        r.actions.push(a);
                    }
                }
                task_list(r, ui, &tasks, size);
            }
        }
        "calendar" => {
            let days = res["days"].as_array().cloned().unwrap_or_default();
            if days.is_empty() {
                return empty(r, ui, "calendar");
            }
            for d in days {
                let mut runs = Runs::new();
                value_runs(r, &mut runs, &d["date"], size, true);
                runs.push(" — ", text_fmt(r, size));
                value_runs(r, &mut runs, &d["link"], size, true);
                if let Some(a) = runs.show(ui) {
                    r.actions.push(a);
                }
            }
        }
        "js" => js_node(r, ui, &res["root"], size),
        "css" => {}
        "value" => {
            let mut runs = Runs::new();
            value_runs(r, &mut runs, &res["value"], size, true);
            if let Some(a) = runs.show(ui) {
                r.actions.push(a);
            }
        }
        _ => {}
    }
}

fn task_list(r: &mut Renderer, ui: &mut egui::Ui, tasks: &[Value], size: f32) {
    let p = r.host.palette();
    for t in tasks {
        ui.horizontal_top(|ui| {
            let status = t["status"].as_str().and_then(|s| s.chars().next());
            match status {
                Some(c) if t["task"].as_bool().unwrap_or(false) => {
                    let mut checked = c != ' ';
                    if ui.add(egui::Checkbox::without_text(&mut checked)).clicked() {
                        r.actions.push(Action::ToggleTaskIn(t["path"].as_str().unwrap_or_default().to_string(), t["line"].as_f64().unwrap_or(0.0) as usize, c));
                    }
                }
                _ => {
                    markdown::bullet(ui, size, p.faint, false);
                }
            }
            ui.vertical(|ui| {
                let text = t["text"].as_str().unwrap_or_default();
                let doc = markdown::parse(text);
                let done = matches!(status, Some(c) if c != ' ');
                for b in &doc.blocks {
                    if let markdown::Block::Para(inl) = b {
                        let mut runs = Runs::new();
                        r.runs(&mut runs, inl, size, Style { strike: done, ..Default::default() }, done.then_some(p.faint));
                        if let Some(a) = runs.show(ui) {
                            r.actions.push(a);
                        }
                    }
                }
                if let Some(children) = t["children"].as_array() {
                    if !children.is_empty() {
                        task_list(r, ui, children, size);
                    }
                }
            });
        });
    }
}

const BLOCK_TAGS: &[&str] = &["div", "p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "table", "thead", "tbody", "tr", "pre", "blockquote", "details", "summary", "section", "article", "header", "footer", "hr", "figure", "nav"];

/// The element tree recorded by `dv.el`, `dv.header`, `dv.paragraph`… with
/// tables and lists placed where the script made them.
fn js_node(r: &mut Renderer, ui: &mut egui::Ui, node: &Value, size: f32) {
    let p = r.host.palette();
    let mut runs = Runs::new();
    let flush = |r: &mut Renderer, ui: &mut egui::Ui, runs: &mut Runs| {
        let done = std::mem::take(runs);
        if let Some(a) = done.show(ui) {
            r.actions.push(a);
        }
    };
    for c in node.get("children").and_then(Value::as_array).cloned().unwrap_or_default() {
        if let Some(s) = c.as_str() {
            runs.push(s, text_fmt(r, size));
            continue;
        }
        if let Some(op) = c.get("op") {
            flush(r, ui, &mut runs);
            render(r, ui, op);
            continue;
        }
        let Some(el) = c.get("el") else { continue };
        let tag = el["tag"].as_str().unwrap_or("div").to_lowercase();
        let heading = tag.len() == 2 && tag.starts_with('h') && tag[1..].parse::<u8>().is_ok();
        if BLOCK_TAGS.contains(&tag.as_str()) {
            flush(r, ui, &mut runs);
            if tag == "hr" {
                ui.add_space(size * 0.6);
                let (rr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().hline(rr.x_range(), rr.center().y, egui::Stroke::new(1.0_f32, p.border_strong));
                ui.add_space(size * 0.6);
                continue;
            }
            let sz = if heading { size * [1.6, 1.32, 1.16, 1.05, 1.0, 0.92][(tag[1..].parse::<usize>().unwrap_or(1)).clamp(1, 6) - 1] } else { size };
            let mut inner = Runs::new();
            if heading {
                ui.add_space(size * 0.8);
            }
            for key in ["text", "markdown"] {
                if let Some(v) = el.get(key).filter(|v| !v.is_null()) {
                    match v {
                        Value::String(s) if key == "markdown" && (tag == "p" || tag == "div") => {
                            // block markdown (dv.paragraph)
                            let doc = markdown::parse(s);
                            flush(r, ui, &mut inner);
                            r.blocks(ui, &doc.blocks);
                        }
                        other => {
                            value_runs(r, &mut inner, other, sz, true);
                            if heading {
                                for s in inner.job.sections.iter_mut() {
                                    s.format.font_id = FontId::new(sz, FontFamily::Name("bold".into()));
                                }
                            }
                        }
                    }
                }
            }
            if let Some(h) = el.get("html").and_then(Value::as_str) {
                inner.push(&h.replace("<br>", "\n"), text_fmt(r, sz));
            }
            if tag == "li" {
                ui.horizontal_top(|ui| {
                    markdown::bullet(ui, size, p.faint, false);
                    ui.vertical(|ui| {
                        flush(r, ui, &mut inner);
                        js_node(r, ui, el, size);
                    });
                });
            } else {
                flush(r, ui, &mut inner);
                js_node(r, ui, el, size);
            }
        } else {
            // inline element: text joins the current line
            for key in ["text", "markdown"] {
                if let Some(v) = el.get(key).filter(|v| !v.is_null()) {
                    value_runs(r, &mut runs, v, size, true);
                }
            }
            if tag == "br" {
                runs.push("\n", text_fmt(r, size));
            }
            let has_children = el["children"].as_array().is_some_and(|c| !c.is_empty());
            if has_children {
                flush(r, ui, &mut runs);
                js_node(r, ui, el, size);
            }
        }
    }
    flush(r, ui, &mut runs);
}
