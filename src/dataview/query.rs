//! Running a parsed query against the index.

use super::eval::{file_link, Ctx};
use super::index::Index;
use super::parse::{parse_expr, parse_query, Expr, Header, Op, Query};
use super::value::{compare, Obj, Settings, Value};
use serde_json::{json, Value as Json};
use std::cmp::Ordering;

struct Row {
    /// The value shown in the id column (`file.link`, or the group key).
    id: Value,
    data: Value,
}

pub enum QueryResult {
    List { rows: Vec<(Option<Value>, Value)>, without_id: bool, grouped: bool },
    Table { headers: Vec<String>, rows: Vec<Vec<Value>> },
    Task { groups: Vec<(Option<Value>, Vec<Value>)> },
    Calendar { days: Vec<(Value, Value)> },
}

fn rows_for(index: &Index, q: &Query, origin: &str) -> Result<Vec<Row>, String> {
    let paths = index.source_paths(&q.source, origin)?;
    let mut rows = Vec::new();
    for p in paths {
        let Some(page) = index.page_row(&p) else { continue };
        if matches!(q.header, Header::Task) {
            let Value::Object(page_obj) = &page else { continue };
            let file = page_obj.get("file").cloned().unwrap_or(Value::Null);
            let Value::List(tasks) = super::eval::index_value(&file, &Value::str("tasks"), index) else { continue };
            for t in tasks {
                // a task sees the page's fields under its own
                let mut o = page_obj.clone();
                if let Value::Object(to) = &t {
                    for (k, v) in &to.0 {
                        o.insert(k.clone(), v.clone());
                    }
                }
                rows.push(Row { id: file_link(&p), data: Value::Object(o) });
            }
        } else {
            rows.push(Row { id: file_link(&p), data: page });
        }
    }
    Ok(rows)
}

fn sort_rows(cx: &Ctx, rows: &mut Vec<Row>, keys: &[(Expr, bool)]) -> Result<(), String> {
    let mut keyed: Vec<(Vec<Value>, Row)> = Vec::with_capacity(rows.len());
    for r in rows.drain(..) {
        let mut k = Vec::new();
        for (e, _) in keys {
            k.push(cx.eval(e, &r.data)?);
        }
        keyed.push((k, r));
    }
    keyed.sort_by(|a, b| {
        for (i, (_, desc)) in keys.iter().enumerate() {
            let o = compare(&a.0[i], &b.0[i]);
            if o != Ordering::Equal {
                return if *desc { o.reverse() } else { o };
            }
        }
        Ordering::Equal
    });
    rows.extend(keyed.into_iter().map(|x| x.1));
    Ok(())
}

pub fn execute(index: &Index, settings: &Settings, origin: &str, text: &str) -> Result<QueryResult, String> {
    let q = parse_query(text)?;
    let cx = Ctx::new(index, settings, origin);
    let mut rows = rows_for(index, &q, origin)?;
    let mut grouped = false;
    for op in &q.ops {
        match op {
            Op::Where(e) => {
                let mut keep = Vec::with_capacity(rows.len());
                for r in rows {
                    if cx.eval(e, &r.data)?.truthy() {
                        keep.push(r);
                    }
                }
                rows = keep;
            }
            Op::Sort(keys) => sort_rows(&cx, &mut rows, keys)?,
            Op::Limit(n) => rows.truncate(*n),
            Op::GroupBy(e, name) => {
                let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                for r in rows {
                    let k = cx.eval(e, &r.data)?;
                    match groups.iter_mut().find(|(gk, _)| compare(gk, &k) == Ordering::Equal) {
                        Some((_, v)) => v.push(r.data),
                        None => groups.push((k, vec![r.data])),
                    }
                }
                groups.sort_by(|a, b| compare(&a.0, &b.0));
                rows = groups
                    .into_iter()
                    .map(|(k, members)| {
                        let mut o = Obj::default();
                        o.insert("key", k.clone());
                        o.insert("rows", Value::List(members));
                        if name != "key" && !name.contains(['.', '(', '[', ' ']) {
                            o.insert(name.clone(), k.clone());
                        }
                        Row { id: k, data: Value::Object(o) }
                    })
                    .collect();
                grouped = true;
            }
            Op::Flatten(e, name) => {
                let mut out = Vec::new();
                for r in rows {
                    let v = cx.eval(e, &r.data)?;
                    let items = match v {
                        Value::List(l) => l,
                        other => vec![other],
                    };
                    for it in items {
                        let mut data = r.data.clone();
                        if let Value::Object(o) = &mut data {
                            o.insert(name.clone(), it);
                        }
                        out.push(Row { id: r.id.clone(), data });
                    }
                }
                rows = out;
            }
        }
    }
    Ok(match &q.header {
        Header::List { without_id, expr } => {
            let mut out = Vec::new();
            for r in rows {
                let v = match expr {
                    Some(e) => Some(cx.eval(e, &r.data)?),
                    None => None,
                };
                out.push(match v {
                    Some(v) if *without_id => (None, v),
                    Some(v) => (Some(r.id), v),
                    None if grouped => {
                        let members = super::eval::index_value(&r.data, &Value::str("rows"), index);
                        let links = super::eval::index_value(&members, &Value::str("file"), index);
                        (Some(r.id), super::eval::index_value(&links, &Value::str("link"), index))
                    }
                    None => (None, r.id),
                });
            }
            QueryResult::List { rows: out, without_id: *without_id, grouped }
        }
        Header::Table { without_id, columns } => {
            let mut headers: Vec<String> = Vec::new();
            if !without_id {
                headers.push(if grouped { settings.group_column.clone() } else { settings.id_column.clone() });
            }
            headers.extend(columns.iter().map(|(_, n)| n.clone()));
            let mut out = Vec::new();
            for r in rows {
                let mut cells = Vec::new();
                if !without_id {
                    cells.push(r.id.clone());
                }
                for (e, _) in columns {
                    cells.push(cx.eval(e, &r.data)?);
                }
                out.push(cells);
            }
            QueryResult::Table { headers, rows: out }
        }
        Header::Task => {
            let mut groups: Vec<(Option<Value>, Vec<Value>)> = Vec::new();
            if grouped {
                for r in rows {
                    let tasks = match &r.data {
                        Value::Object(o) => match o.get("rows") {
                            Some(Value::List(l)) => l.clone(),
                            _ => Vec::new(),
                        },
                        _ => Vec::new(),
                    };
                    groups.push((Some(r.id), drop_nested(tasks)));
                }
            } else {
                for r in rows {
                    match groups.last_mut() {
                        Some((Some(k), v)) if compare(k, &r.id) == Ordering::Equal => v.push(r.data),
                        _ => groups.push((Some(r.id), vec![r.data])),
                    }
                }
                for g in &mut groups {
                    g.1 = drop_nested(std::mem::take(&mut g.1));
                }
            }
            QueryResult::Task { groups }
        }
        Header::Calendar(e) => {
            let mut days = Vec::new();
            for r in rows {
                let v = cx.eval(e, &r.data)?;
                if matches!(v, Value::Date(_)) {
                    days.push((v, r.id));
                }
            }
            QueryResult::Calendar { days }
        }
    })
}

/// Tasks whose parent task is also in the list are shown under it, not twice.
fn drop_nested(tasks: Vec<Value>) -> Vec<Value> {
    let key = |t: &Value| -> Option<(String, f64)> {
        let Value::Object(o) = t else { return None };
        Some((o.get("path")?.as_str()?.to_string(), o.get("line")?.as_f64()?))
    };
    let mut lines = std::collections::HashSet::new();
    for t in &tasks {
        if let Some((p, l)) = key(t) {
            lines.insert((p, l as i64));
        }
    }
    let ancestor_in = |t: &Value| -> bool {
        let Value::Object(o) = t else { return false };
        let path = o.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
        matches!(o.get("parent"), Some(Value::Number(p)) if lines.contains(&(path, *p as i64)))
    };
    tasks.iter().filter(|t| !ancestor_in(t)).cloned().collect()
}

/// The JSON the interface renders.
pub fn result_json(r: &QueryResult, s: &Settings) -> Json {
    let d = |v: &Value| v.to_display(s);
    match r {
        QueryResult::List { rows, .. } => json!({
            "type": "list",
            "items": rows.iter().map(|(id, v)| json!({ "id": id.as_ref().map(d), "value": d(v) })).collect::<Vec<_>>(),
        }),
        QueryResult::Table { headers, rows } => json!({
            "type": "table",
            "headers": headers,
            "rows": rows.iter().map(|r| r.iter().map(d).collect::<Vec<_>>()).collect::<Vec<_>>(),
        }),
        QueryResult::Task { groups } => json!({
            "type": "task",
            "groups": groups.iter().map(|(k, tasks)| json!({ "key": k.as_ref().map(d), "tasks": tasks.iter().map(task_json).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        }),
        QueryResult::Calendar { days } => json!({
            "type": "calendar",
            "days": days.iter().map(|(date, link)| json!({ "date": d(date), "link": d(link) })).collect::<Vec<_>>(),
        }),
    }
}

pub fn task_json(t: &Value) -> Json {
    let Value::Object(o) = t else { return Json::Null };
    let get = |k: &str| o.get(k).cloned().unwrap_or(Value::Null);
    let children = match get("children") {
        Value::List(l) => l.iter().map(task_json).collect(),
        _ => Vec::new(),
    };
    json!({
        "text": get("text").as_str().unwrap_or_default(),
        "status": get("status").as_str(),
        "task": get("task").truthy(),
        "path": get("path").as_str().unwrap_or_default(),
        "line": get("line").as_f64().unwrap_or(0.0),
        "children": children,
    })
}

pub fn run(index: &Index, settings: &Settings, origin: &str, text: &str) -> Json {
    match execute(index, settings, origin, text) {
        Ok(r) => result_json(&r, settings),
        Err(e) => json!({ "type": "error", "error": e }),
    }
}

/// An inline query: `= expression`.
pub fn inline(index: &Index, settings: &Settings, origin: &str, text: &str) -> Json {
    let cx = Ctx::new(index, settings, origin);
    let res = parse_expr(text.trim()).and_then(|e| cx.eval(&e, &cx.this));
    match res {
        Ok(v) => json!({ "type": "value", "value": v.to_display(settings) }),
        Err(e) => json!({ "type": "error", "error": e }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataview::index::Index;
    use std::fs;

    fn vault() -> (std::path::PathBuf, Index) {
        let dir = std::env::temp_dir().join(format!("ateliet-dv-{}-{}", std::process::id(), rand_suffix()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Fin/2026")).unwrap();
        fs::create_dir_all(dir.join("Proj")).unwrap();
        fs::write(dir.join("Fin/2026/2026-09.md"), "---\ntipo: mensal\nmes: Setembro\nano: 2026\n---\n- aluguel [valor:: 1200]\n- luz [valor:: 150]\n").unwrap();
        fs::write(dir.join("Fin/2026/2026-10.md"), "---\ntipo: mensal\nmes: Outubro\nano: 2026\nstatus: aberto\n---\n- aluguel [valor:: 1200]\n- [ ] pagar luz [valor:: 180] 📅 2026-10-30\n- [x] internet\n").unwrap();
        fs::write(dir.join("Proj/A.md"), "---\ntags: [proj]\nprogresso: 80\ndeadline: 2026-12-01\n---\nVer [[2026-10]].\n").unwrap();
        fs::write(dir.join("Proj/B.md"), "---\ntags: [proj/sub]\nprogresso: 20\n---\n#proj\n").unwrap();
        fs::write(dir.join("Home.md"), "status:: ok\n[[A]] [[B]]\n").unwrap();
        let ix = Index::build(&dir);
        (dir, ix)
    }

    fn rand_suffix() -> u64 {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    #[test]
    fn table_where_sort() {
        let (dir, ix) = vault();
        let s = Settings::default();
        let r = execute(&ix, &s, "Home.md", "TABLE progresso, deadline FROM #proj WHERE progresso > 10 SORT progresso DESC").unwrap();
        let QueryResult::Table { headers, rows } = r else { panic!() };
        assert_eq!(headers, vec!["File", "progresso", "deadline"]);
        assert_eq!(rows.len(), 2);
        assert!(matches!(&rows[0][0], Value::Link(l) if l.path == "Proj/A.md"));
        assert!(matches!(rows[1][1], Value::Number(n) if n == 20.0));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sums_over_list_fields() {
        let (dir, ix) = vault();
        let s = Settings::default();
        let r = execute(&ix, &s, "Home.md", "TABLE WITHOUT ID mes, sum(file.lists.valor) AS total FROM \"Fin\" WHERE tipo = \"mensal\" SORT ano, file.name").unwrap();
        let QueryResult::Table { rows, .. } = r else { panic!() };
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0][1], Value::Number(n) if n == 1350.0));
        assert!(matches!(rows[1][1], Value::Number(n) if n == 1380.0));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn group_tasks_inline() {
        let (dir, ix) = vault();
        let s = Settings::default();
        let r = execute(&ix, &s, "Home.md", "LIST rows.file.link FROM #proj GROUP BY tags").unwrap();
        let QueryResult::List { rows, grouped, .. } = r else { panic!() };
        assert!(grouped);
        assert_eq!(rows.len(), 2);
        let r = execute(&ix, &s, "Home.md", "TASK FROM \"Fin\" WHERE !completed").unwrap();
        let QueryResult::Task { groups } = r else { panic!() };
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1.len(), 1);
        let r = execute(&ix, &s, "Home.md", "LIST FROM [[]]").unwrap();
        let QueryResult::List { rows, .. } = r else { panic!() };
        assert!(rows.is_empty());
        let r = execute(&ix, &s, "Proj/A.md", "LIST FROM [[2026-10]]").unwrap();
        let QueryResult::List { rows, .. } = r else { panic!() };
        assert_eq!(rows.len(), 1);
        let v = inline(&ix, &s, "Home.md", "this.status + \" / \" + length(this.file.outlinks)");
        assert_eq!(v["value"], "ok / 2");
        let v = inline(&ix, &s, "Home.md", "[[A]].progresso * 2");
        assert_eq!(v["value"], 160.0);
        let v = inline(&ix, &s, "Home.md", "date(2026-10-21) + dur(1 month)");
        assert_eq!(v["value"]["text"], "November 21, 2026");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_are_reported() {
        let (dir, ix) = vault();
        let s = Settings::default();
        assert_eq!(run(&ix, &s, "Home.md", "TABLE FROM")["type"], "error");
        assert_eq!(run(&ix, &s, "Home.md", "LIST WHERE nosuchfn(1)")["type"], "error");
        let _ = fs::remove_dir_all(dir);
    }
}
