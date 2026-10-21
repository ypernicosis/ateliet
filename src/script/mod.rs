//! The JavaScript engine (QuickJS) for the scripts people already have in
//! their notes: `dataviewjs` blocks and inline `$=` queries, and Templater's
//! `<% %>` / `<%* %>` commands. Everything the scripts reach for (pages,
//! queries, files, date formats, prompts) is answered here in Rust; the
//! prelude only shapes it into the `dv` and `tp` APIs.

use crate::dataview::eval::Ctx;
use crate::dataview::index::Index;
use crate::dataview::parse::{parse_expr, parse_source};
use crate::dataview::query::{self, QueryResult};
use crate::dataview::value::{parse_duration, parse_inline_value, Dur, Settings, Value};
use crate::datefmt;
use rquickjs::{Context, Function, Promise, Runtime};
use serde_json::{json, Value as Json};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

const PRELUDE: &str = include_str!("prelude.js");

pub type PromptFn = Arc<dyn Fn(Json) -> Option<Json> + Send + Sync>;
pub type LogFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// What a script may touch.
#[derive(Clone)]
pub struct Host {
    pub index: Arc<RwLock<Index>>,
    pub settings: Settings,
    pub root: PathBuf,
    pub prompt: Option<PromptFn>,
    pub log: Option<LogFn>,
    /// Seconds a script may run (prompts excluded).
    pub timeout_secs: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// A vault-relative path that stays inside the vault.
fn safe_rel(p: &str) -> Option<String> {
    let p = p.trim().trim_start_matches('/');
    if p.is_empty() || Path::new(p).components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(p.to_string())
}

struct Call<'a> {
    host: &'a Host,
    deadline: &'a AtomicI64,
}

impl Call<'_> {
    fn index(&self) -> std::sync::RwLockReadGuard<'_, Index> {
        self.host.index.read().unwrap_or_else(|e| e.into_inner())
    }

    fn resolve_page(&self, ix: &Index, path: &str, origin: &str) -> Option<String> {
        if ix.page(path).is_some() {
            return Some(path.to_string());
        }
        let with_md = format!("{path}.md");
        if ix.page(&with_md).is_some() {
            return Some(with_md);
        }
        ix.resolve(path, origin)
    }

    fn query_wire(&self, ix: &Index, q: &str, origin: &str) -> Json {
        match query::execute(ix, &self.host.settings, origin, q) {
            Ok(r) => json!({ "successful": true, "value": result_wire(&r) }),
            Err(e) => json!({ "successful": false, "error": e }),
        }
    }

    fn run(&self, name: &str, a: &[Json]) -> Result<Json, String> {
        let s = |i: usize| a.get(i).and_then(Json::as_str).unwrap_or_default().to_string();
        let st = &self.host.settings;
        Ok(match name {
            "page" => {
                let ix = self.index();
                match self.resolve_page(&ix, &s(0), &s(1)) {
                    Some(p) => ix.page_row(&p).map(|v| v.to_wire()).unwrap_or(Json::Null),
                    None => Json::Null,
                }
            }
            "pages" | "page_paths" => {
                let ix = self.index();
                let src = parse_source(&s(0))?;
                let paths = ix.source_paths(&src, &s(1))?;
                if name == "page_paths" {
                    json!(paths)
                } else {
                    Json::Array(paths.iter().filter_map(|p| ix.page_row(p)).map(|v| v.to_wire()).collect())
                }
            }
            "resolve" => {
                let ix = self.index();
                let t = s(0);
                let t = t.split('#').next().unwrap_or_default();
                if ix.exists(t) {
                    json!(t)
                } else {
                    ix.resolve(t, &s(1)).map(Json::String).unwrap_or(Json::Null)
                }
            }
            "exists" => {
                let ix = self.index();
                json!(safe_rel(&s(0)).is_some_and(|p| ix.exists(&p) || self.host.root.join(&p).exists()))
            }
            "load" => {
                let path = {
                    let ix = self.index();
                    let raw = s(0);
                    match safe_rel(&raw) {
                        Some(p) if self.host.root.join(&p).is_file() => Some(p),
                        _ => ix.resolve(&raw, &s(1)),
                    }
                };
                match path.and_then(|p| std::fs::read_to_string(self.host.root.join(p)).ok()) {
                    Some(t) => json!(t),
                    None => Json::Null,
                }
            }
            "date" => {
                let t = s(0);
                let k = t.trim().to_lowercase();
                let ix = self.index();
                let cx = Ctx::new(&ix, st, "");
                let v = parse_expr(&format!("date({k})")).ok().and_then(|e| cx.eval(&e, &Value::Null).ok());
                match v {
                    Some(Value::Date(d)) => json!(datefmt::to_millis(d)),
                    _ => datefmt::parse_iso(&t).map(|d| json!(datefmt::to_millis(d))).unwrap_or(Json::Null),
                }
            }
            "duration" => match parse_duration(&s(0)) {
                Some(d) => Value::Duration(d).to_wire()["$dur"].clone(),
                None => Json::Null,
            },
            "parse_value" => parse_inline_value(&s(0)).to_wire(),
            "query" => {
                let ix = self.index();
                self.query_wire(&ix, &s(0), &s(1))
            }
            "query_markdown" => {
                let ix = self.index();
                match query::execute(&ix, st, &s(1), &s(0)) {
                    Ok(r) => json!({ "successful": true, "value": result_markdown(&r, st) }),
                    Err(e) => json!({ "successful": false, "error": e }),
                }
            }
            "evaluate" => {
                let ix = self.index();
                let cx = Ctx::new(&ix, st, &s(1));
                match parse_expr(&s(0)).and_then(|e| cx.eval(&e, &cx.this)) {
                    Ok(v) => json!({ "successful": true, "value": v.to_wire() }),
                    Err(e) => json!({ "successful": false, "error": e }),
                }
            }
            "markdown_table" => {
                let headers: Vec<String> = a.first().and_then(Json::as_array).map(|h| h.iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default();
                let rows: Vec<Vec<Value>> = a
                    .get(1)
                    .and_then(Json::as_array)
                    .map(|r| r.iter().map(|row| row.as_array().map(|c| c.iter().map(Value::from_wire).collect()).unwrap_or_default()).collect())
                    .unwrap_or_default();
                json!(markdown_table(&headers, &rows, st))
            }
            "markdown_list" => {
                let items: Vec<Value> = a.first().and_then(Json::as_array).map(|l| l.iter().map(Value::from_wire).collect()).unwrap_or_default();
                json!(items.iter().map(|v| format!("- {}", md_cell(v, st))).collect::<Vec<_>>().join("\n"))
            }
            "markdown_tasks" => {
                let items: Vec<Value> = a.first().and_then(Json::as_array).map(|l| l.iter().map(Value::from_wire).collect()).unwrap_or_default();
                json!(items.iter().map(|t| markdown_task(t, 0)).collect::<Vec<_>>().join("\n"))
            }
            "format" => {
                let ms = a.first().and_then(Json::as_f64).unwrap_or(0.0) as i64;
                let dt = datefmt::from_millis(ms);
                let fmt = s(1);
                json!(if s(2) == "luxon" { datefmt::format_luxon(dt, &fmt, st.loc()) } else { datefmt::format_moment(dt, &fmt, st.loc()) })
            }
            "parse_format" => {
                let fmt = s(1);
                let fmt = if s(2) == "luxon" { luxon_to_moment(&fmt) } else { fmt };
                datefmt::parse_moment(&s(0), &fmt, st.loc()).map(|d| json!(datefmt::to_millis(d))).unwrap_or(Json::Null)
            }
            "parse_iso" => datefmt::parse_iso(&s(0)).map(|d| json!(datefmt::to_millis(d))).unwrap_or(Json::Null),
            "dur_human" => json!(dur_from(a.first()).human(st.loc())),
            "dur_format" => {
                let d = dur_from(a.first());
                let ix = self.index();
                let cx = Ctx::new(&ix, st, "");
                let e = crate::dataview::parse::Expr::Call(
                    Box::new(crate::dataview::parse::Expr::Field("durationformat".into())),
                    vec![crate::dataview::parse::Expr::Lit(Value::Duration(d)), crate::dataview::parse::Expr::Lit(Value::String(s(1)))],
                );
                match cx.eval(&e, &Value::Null)? {
                    Value::String(t) => json!(t),
                    _ => json!(""),
                }
            }
            "locale" => json!(st.locale.to_lowercase()),
            "names" => {
                let l = st.loc();
                match s(0).as_str() {
                    "months" => json!(l.months),
                    "months_short" => json!(l.months_short),
                    "weekdays" => json!(l.weekdays),
                    "weekdays_short" => json!(l.weekdays_short),
                    _ => json!(l.weekdays_min),
                }
            }
            "tp_tags" => {
                let ix = self.index();
                json!(ix.page(&s(0)).map(|p| p.tags.clone()).unwrap_or_default())
            }
            "unique_path" => {
                let base = safe_rel(&s(0)).ok_or("invalid path")?;
                let ext = s(1);
                let ix = self.index();
                let mut candidate = format!("{base}.{ext}");
                let mut n = 1;
                while ix.exists(&candidate) || self.host.root.join(&candidate).exists() {
                    candidate = format!("{base} {n}.{ext}");
                    n += 1;
                }
                json!(candidate)
            }
            "create" => {
                let p = safe_rel(&s(0)).ok_or("invalid path")?;
                let abs = self.host.root.join(&p);
                if abs.exists() {
                    return Err(format!("{p} already exists"));
                }
                if let Some(dir) = abs.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&abs, s(1)).map_err(|e| e.to_string())?;
                self.host.index.write().unwrap_or_else(|e| e.into_inner()).update(&[p]);
                Json::Null
            }
            "prompt" => {
                let Some(prompt) = &self.host.prompt else { return Err("prompts are not available here".into()) };
                // waiting for the person does not count against the time limit
                let started = now_ms();
                let answer = prompt(a.first().cloned().unwrap_or(Json::Null));
                self.deadline.fetch_add(now_ms() - started, Ordering::SeqCst);
                answer.unwrap_or(Json::Null)
            }
            "compile_template" => json!(compile_template(&s(0))),
            "log" => {
                if let Some(log) = &self.host.log {
                    log(&s(0), &s(1));
                }
                Json::Null
            }
            other => return Err(format!("unknown host call {other}")),
        })
    }
}

fn dur_from(j: Option<&Json>) -> Dur {
    let mut d = Dur::default();
    if let Some(Json::Object(o)) = j {
        for (k, v) in o {
            d.set(k, v.as_f64().unwrap_or(0.0));
        }
    }
    d
}

fn luxon_to_moment(fmt: &str) -> String {
    fmt.replace("yyyy", "YYYY").replace("yy", "YY").replace("dd", "DD").replace('d', "D").replace("EEEE", "dddd").replace("EEE", "ddd")
}

/// A query result in the shape `dv.query` returns.
fn result_wire(r: &QueryResult) -> Json {
    match r {
        QueryResult::List { rows, .. } => json!({
            "type": "list",
            "values": rows.iter().map(|(id, v)| match id {
                Some(id) if !v.is_null() => json!({ "key": id.to_wire(), "value": v.to_wire() }),
                Some(id) => id.to_wire(),
                None => v.to_wire(),
            }).collect::<Vec<_>>(),
        }),
        QueryResult::Table { headers, rows } => json!({
            "type": "table",
            "headers": headers,
            "values": rows.iter().map(|r| r.iter().map(Value::to_wire).collect::<Vec<_>>()).collect::<Vec<_>>(),
        }),
        QueryResult::Task { groups } => json!({
            "type": "task",
            "values": groups.iter().flat_map(|(_, t)| t.iter().map(Value::to_wire)).collect::<Vec<_>>(),
        }),
        QueryResult::Calendar { days } => json!({
            "type": "calendar",
            "values": days.iter().map(|(d, l)| json!({ "date": d.to_wire(), "link": l.to_wire() })).collect::<Vec<_>>(),
        }),
    }
}

fn md_cell(v: &Value, s: &Settings) -> String {
    match v {
        Value::Null => s.null_as.clone(),
        Value::Link(l) => l.markdown(),
        Value::List(l) => l.iter().map(|x| md_cell(x, s)).collect::<Vec<_>>().join(", "),
        other => other.to_text(s).replace('|', "\\|").replace('\n', " "),
    }
}

fn markdown_table(headers: &[String], rows: &[Vec<Value>], s: &Settings) -> String {
    let mut out = format!("| {} |\n|{}|\n", headers.join(" | "), headers.iter().map(|_| " --- ").collect::<Vec<_>>().join("|"));
    for r in rows {
        out.push_str(&format!("| {} |\n", r.iter().map(|c| md_cell(c, s)).collect::<Vec<_>>().join(" | ")));
    }
    out
}

fn markdown_task(t: &Value, depth: usize) -> String {
    let Value::Object(o) = t else { return String::new() };
    let text = o.get("text").and_then(Value::as_str).unwrap_or_default();
    let status = o.get("status").and_then(Value::as_str);
    let indent = "    ".repeat(depth);
    let mut line = match status {
        Some(st) => format!("{indent}- [{st}] {text}"),
        None => format!("{indent}- {text}"),
    };
    if let Some(Value::List(children)) = o.get("children") {
        for c in children {
            line.push('\n');
            line.push_str(&markdown_task(c, depth + 1));
        }
    }
    line
}

fn result_markdown(r: &QueryResult, s: &Settings) -> String {
    match r {
        QueryResult::List { rows, .. } => rows
            .iter()
            .map(|(id, v)| match id {
                Some(id) if !v.is_null() => format!("- {}: {}", md_cell(id, s), md_cell(v, s)),
                Some(id) => format!("- {}", md_cell(id, s)),
                None => format!("- {}", md_cell(v, s)),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        QueryResult::Table { headers, rows } => markdown_table(headers, rows, s),
        QueryResult::Task { groups } => groups.iter().flat_map(|(_, t)| t.iter().map(|x| markdown_task(x, 0))).collect::<Vec<_>>().join("\n"),
        QueryResult::Calendar { days } => days.iter().map(|(d, l)| format!("- {}: {}", md_cell(d, s), md_cell(l, s))).collect::<Vec<_>>().join("\n"),
    }
}

/// Templater's syntax compiled to the body of an async function: text is
/// appended to `tR`, `<% expr %>` appends its value, `<%* code %>` runs.
/// `-`/`_` next to the delimiters trim one newline / all whitespace.
pub fn compile_template(text: &str) -> String {
    let mut out = String::from("let tR = \"\";\n");
    let mut rest = text;
    let mut pending_text = String::new();
    let mut trim_next: Option<char> = None;
    loop {
        let Some(open) = rest.find("<%") else {
            let mut t = rest.to_string();
            apply_right_trim(&mut t, trim_next);
            pending_text.push_str(&t);
            break;
        };
        let mut before = rest[..open].to_string();
        apply_right_trim(&mut before, trim_next.take());
        let after_open = &rest[open + 2..];
        let mut chars = after_open.char_indices().peekable();
        let mut left_trim = None;
        let mut exec = false;
        let mut skip = 0;
        if let Some(&(_, c)) = chars.peek() {
            if c == '-' || c == '_' {
                left_trim = Some(c);
                chars.next();
                skip += 1;
            }
        }
        if let Some(&(_, c)) = chars.peek() {
            if c == '*' {
                exec = true;
                skip += 1;
            } else if c == '+' || c == '~' {
                skip += 1;
            }
        }
        match left_trim {
            Some('-') => {
                if before.ends_with("\r\n") {
                    before.truncate(before.len() - 2);
                } else if before.ends_with('\n') {
                    before.pop();
                }
            }
            Some('_') => {
                let t = before.trim_end().len();
                before.truncate(t);
            }
            _ => {}
        }
        pending_text.push_str(&before);
        let body_start = &after_open[skip..];
        let Some(close) = body_start.find("%>") else {
            // unterminated: keep the rest as text
            pending_text.push_str(&rest[open..]);
            break;
        };
        let mut code = &body_start[..close];
        let mut right = None;
        if code.ends_with('-') || code.ends_with('_') {
            right = code.chars().last();
            code = &code[..code.len() - 1];
        }
        if !pending_text.is_empty() {
            out.push_str(&format!("tR += {};\n", serde_json::to_string(&pending_text).unwrap()));
            pending_text.clear();
        }
        if exec {
            out.push_str(code);
            out.push_str("\n;\n");
        } else {
            let expr = code.trim().trim_end_matches(';');
            if !expr.is_empty() {
                out.push_str(&format!("tR += __out(await ({expr}\n));\n"));
            }
        }
        trim_next = right;
        rest = &body_start[close + 2..];
    }
    if !pending_text.is_empty() {
        out.push_str(&format!("tR += {};\n", serde_json::to_string(&pending_text).unwrap()));
    }
    out.push_str("return tR;\n");
    out
}

fn apply_right_trim(t: &mut String, mode: Option<char>) {
    match mode {
        Some('-') => {
            if t.starts_with("\r\n") {
                t.drain(..2);
            } else if t.starts_with('\n') {
                t.remove(0);
            }
        }
        Some('_') => {
            let n = t.len() - t.trim_start().len();
            t.drain(..n);
        }
        _ => {}
    }
}

fn exception_text(ctx: &rquickjs::Ctx, e: rquickjs::Error) -> String {
    if matches!(e, rquickjs::Error::Exception) {
        let v = ctx.catch();
        if let Some(ex) = v.as_exception() {
            let msg = ex.message().unwrap_or_default();
            let stack = ex.stack().unwrap_or_default();
            return if stack.is_empty() { msg } else { format!("{msg}\n{stack}") };
        }
        if let Some(s) = v.as_string().and_then(|s| s.to_string().ok()) {
            return s;
        }
        return format!("{v:?}");
    }
    e.to_string()
}

/// Run `entry(arg1, arg2)` (an async global defined by the prelude) and return its string result.
fn run_entry(host: &Host, entry: &str, args: &[String]) -> Result<String, String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_memory_limit(512 * 1024 * 1024);
    rt.set_max_stack_size(4 * 1024 * 1024);
    let deadline = Arc::new(AtomicI64::new(now_ms() + host.timeout_secs * 1000));
    {
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || now_ms() > d.load(Ordering::SeqCst))));
    }
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    let host = Arc::new(host.clone());
    let errors: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    ctx.with(|ctx| -> Result<String, String> {
        let h = host.clone();
        let dl = deadline.clone();
        let errs = errors.clone();
        let f = Function::new(ctx.clone(), move |name: String, args: String| -> String {
            let parsed: Vec<Json> = serde_json::from_str(&args).unwrap_or_default();
            let call = Call { host: &h, deadline: &dl };
            match call.run(&name, &parsed) {
                Ok(Json::Null) => String::new(),
                Ok(v) => v.to_string(),
                Err(e) => {
                    *errs.lock().unwrap() = Some(e);
                    String::new()
                }
            }
        })
        .map_err(|e| e.to_string())?;
        ctx.globals().set("__host", f).map_err(|e| e.to_string())?;
        ctx.eval::<(), _>(PRELUDE).map_err(|e| exception_text(&ctx, e))?;
        let entry_fn: Function = ctx.globals().get(entry).map_err(|e| e.to_string())?;
        let res: Result<String, rquickjs::Error> = match args {
            [a] => entry_fn.call::<_, Promise>((a.clone(),)).and_then(|p| p.finish::<String>()),
            [a, b] => entry_fn.call::<_, Promise>((a.clone(), b.clone())).and_then(|p| p.finish::<String>()),
            _ => return Err("bad entry arguments".into()),
        };
        match res {
            Ok(s) => Ok(s),
            Err(e) => {
                let mut msg = exception_text(&ctx, e);
                if now_ms() > deadline.load(Ordering::SeqCst) {
                    msg = format!("The script took longer than {} seconds and was stopped.", host.timeout_secs);
                }
                if let Some(h) = errors.lock().unwrap().take() {
                    msg = format!("{msg}\n({h})");
                }
                Err(msg)
            }
        }
    })
}

/// A `dataviewjs` block: the rendered element tree, ready for the interface.
pub fn run_dataviewjs(host: &Host, code: &str, origin: &str) -> Json {
    match run_entry(host, "__runDataviewJs", &[code.to_string(), origin.to_string()]) {
        Ok(out) => {
            let tree: Json = serde_json::from_str(&out).unwrap_or(Json::Null);
            let ix = host.index.read().unwrap_or_else(|e| e.into_inner());
            json!({ "type": "js", "root": display_tree(&tree, &ix, &host.settings, origin) })
        }
        Err(e) => json!({ "type": "error", "error": e }),
    }
}

/// An inline `$= expression` query.
pub fn run_inline_js(host: &Host, code: &str, origin: &str) -> Json {
    let wrapped = format!("const __v = await (async () => {{ return ({code}\n); }})(); if (__v !== undefined) dv.span(__v);");
    run_dataviewjs(host, &wrapped, origin)
}

/// Convert wire values inside the recorded element tree to display values.
fn display_tree(t: &Json, ix: &Index, s: &Settings, origin: &str) -> Json {
    match t {
        Json::Object(o) if o.contains_key("$el") => json!({ "el": display_tree(&o["$el"], ix, s, origin) }),
        Json::Object(o) if o.contains_key("$op") => json!({ "op": display_op(&o["$op"], ix, s, origin) }),
        Json::Object(o) => {
            let mut out = serde_json::Map::new();
            for (k, v) in o {
                let nv = match k.as_str() {
                    "text" | "markdown" if !v.is_null() && !v.is_string() => Value::from_wire(v).to_display(s),
                    "children" => Json::Array(v.as_array().map(|c| c.iter().map(|x| display_tree(x, ix, s, origin)).collect()).unwrap_or_default()),
                    _ => v.clone(),
                };
                out.insert(k.clone(), nv);
            }
            Json::Object(out)
        }
        other => other.clone(),
    }
}

fn display_op(op: &Json, ix: &Index, s: &Settings, origin: &str) -> Json {
    let wire_list = |k: &str| -> Vec<Value> { op[k].as_array().map(|a| a.iter().map(Value::from_wire).collect()).unwrap_or_default() };
    match op["kind"].as_str().unwrap_or_default() {
        "list" => json!({ "type": "list", "items": wire_list("items").iter().map(|v| json!({ "id": null, "value": v.to_display(s) })).collect::<Vec<_>>() }),
        "table" => json!({
            "type": "table",
            "headers": op["headers"],
            "rows": op["rows"].as_array().map(|r| r.iter().map(|row| Json::Array(row.as_array().map(|c| c.iter().map(|x| Value::from_wire(x).to_display(s)).collect()).unwrap_or_default())).collect::<Vec<_>>()).unwrap_or_default(),
        }),
        "tasks" => {
            let tasks = wire_list("tasks");
            let group = op["group"].as_bool().unwrap_or(true);
            let mut groups: Vec<(Option<Value>, Vec<Json>)> = Vec::new();
            for t in &tasks {
                let path = match t {
                    Value::Object(o) => o.get("path").and_then(Value::as_str).unwrap_or_default().to_string(),
                    _ => String::new(),
                };
                let key = if group { Some(Value::Link(crate::dataview::value::Link::file(&path))) } else { None };
                match groups.last_mut() {
                    Some((k, v)) if group && k.as_ref().map(|x| x == key.as_ref().unwrap()).unwrap_or(false) => v.push(query::task_json(t)),
                    Some((None, v)) if !group => v.push(query::task_json(t)),
                    _ => groups.push((key, vec![query::task_json(t)])),
                }
            }
            json!({ "type": "task", "groups": groups.iter().map(|(k, t)| json!({ "key": k.as_ref().map(|x| x.to_display(s)), "tasks": t })).collect::<Vec<_>>() })
        }
        "query" => query::run(ix, s, origin, op["query"].as_str().unwrap_or_default()),
        "css" => json!({ "type": "css", "css": op["css"] }),
        other => json!({ "type": "error", "error": format!("unknown output {other}") }),
    }
}

/// Input for one Templater run.
pub struct TemplateRun {
    pub template_path: String,
    pub template_text: String,
    pub target_path: String,
    pub target_content: String,
    pub selection: String,
    pub run_mode: u32,
    pub week_start: u32,
}

pub struct TemplateOutput {
    pub text: String,
    /// UTF-16 offsets of `tp.file.cursor()` markers, in cursor order.
    pub cursors: Vec<usize>,
    pub ops: Vec<Json>,
    pub cancelled: bool,
}

pub fn run_template(host: &Host, run: &TemplateRun) -> Result<TemplateOutput, String> {
    let (frontmatter, ctime, mtime) = {
        let ix = host.index.read().unwrap_or_else(|e| e.into_inner());
        match ix.page(&run.target_path) {
            Some(p) => (Value::Object(p.frontmatter.clone()).to_wire(), p.ctime, p.mtime),
            None => (json!({}), now_ms(), now_ms()),
        }
    };
    let name = run.target_path.rsplit('/').next().unwrap_or(&run.target_path);
    let (basename, extension) = match name.rsplit_once('.') {
        Some((b, e)) => (b.to_string(), e.to_string()),
        None => (name.to_string(), String::new()),
    };
    // plain JSON for the frontmatter: dates as their ISO text
    let fm_plain = strip_wire(&frontmatter);
    let cfg = json!({
        "templateText": run.template_text,
        "templatePath": run.template_path,
        "target": { "path": run.target_path, "basename": basename, "name": name, "extension": extension, "ctime": ctime, "mtime": mtime },
        "targetContent": run.target_content,
        "frontmatter": fm_plain,
        "selection": run.selection,
        "runMode": run.run_mode,
        "vaultPath": host.root.to_string_lossy(),
        "weekStart": run.week_start,
    });
    let out = run_entry(host, "__runTemplate", &[cfg.to_string()])?;
    let v: Json = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    if v["cancelled"].as_bool() == Some(true) {
        return Ok(TemplateOutput { text: String::new(), cursors: Vec::new(), ops: Vec::new(), cancelled: true });
    }
    let raw = v["text"].as_str().unwrap_or_default();
    let (text, cursors) = take_cursors(raw);
    Ok(TemplateOutput { text, cursors, ops: v["ops"].as_array().cloned().unwrap_or_default(), cancelled: false })
}

fn strip_wire(j: &Json) -> Json {
    match j {
        Json::Object(o) if o.len() == 1 && o.contains_key("$date") => {
            let d = datefmt::from_millis(o["$date"].as_f64().unwrap_or(0.0) as i64);
            json!(d.format("%Y-%m-%d").to_string())
        }
        Json::Object(o) if o.len() == 1 && o.contains_key("$link") => {
            let l = &o["$link"];
            json!(format!("[[{}]]", l["path"].as_str().unwrap_or_default().trim_end_matches(".md")))
        }
        Json::Object(o) if o.len() == 1 && o.contains_key("$dur") => json!(dur_from(o.get("$dur")).human(datefmt::locale("en"))),
        Json::Object(o) => Json::Object(o.iter().map(|(k, v)| (k.clone(), strip_wire(v))).collect()),
        Json::Array(a) => Json::Array(a.iter().map(strip_wire).collect()),
        other => other.clone(),
    }
}

/// Remove `\u{E000}CURSOR<n>\u{E000}` markers, returning UTF-16 offsets sorted by n.
fn take_cursors(raw: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(raw.len());
    let mut found: Vec<(i64, usize)> = Vec::new();
    let mut u16 = 0usize;
    let mut rest = raw;
    while let Some(i) = rest.find("\u{E000}CURSOR") {
        let before = &rest[..i];
        out.push_str(before);
        u16 += before.encode_utf16().count();
        let after = &rest[i + "\u{E000}CURSOR".len()..];
        match after.find('\u{E000}') {
            Some(end) => {
                let n: i64 = after[..end].parse().unwrap_or(0);
                found.push((n, u16));
                rest = &after[end + '\u{E000}'.len_utf8()..];
            }
            None => {
                rest = after;
            }
        }
    }
    out.push_str(rest);
    found.sort_by_key(|(n, _)| *n);
    (out, found.into_iter().map(|(_, o)| o).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn host() -> (PathBuf, Host) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("ateliet-js-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(dir.join("Fin")).unwrap();
        fs::write(dir.join("Fin/2026-09.md"), "---\ntipo: mensal\nmes: Setembro\n---\n- aluguel [valor:: 1200]\n- luz [valor:: 150]\n").unwrap();
        fs::write(dir.join("Fin/2026-10.md"), "---\ntipo: mensal\nmes: Outubro\n---\n- aluguel [valor:: 1300]\n- [ ] pagar [valor:: 10]\n").unwrap();
        fs::write(dir.join("Home.md"), "status:: ok\n").unwrap();
        let ix = Index::build(&dir);
        let h = Host { index: Arc::new(RwLock::new(ix)), settings: Settings::default(), root: dir.clone(), prompt: None, log: None, timeout_secs: 10 };
        (dir, h)
    }

    #[test]
    fn dataviewjs_table_and_sums() {
        let (dir, h) = host();
        let code = r#"
            const pages = dv.pages('"Fin"').where(p => p.tipo == "mensal").sort(p => p.file.name);
            dv.header(2, "Totais");
            dv.table(["Mês", "Total"], pages.map(p => [p.mes, p.file.lists.valor.sum()]));
            dv.paragraph(`Geral: ${pages.file.lists.valor.array().reduce((a, b) => a + b, 0)}`);
            dv.list(pages.file.link);
        "#;
        let r = run_dataviewjs(&h, code, "Home.md");
        assert_eq!(r["type"], "js", "{r}");
        let kids = r["root"]["children"].as_array().unwrap();
        assert_eq!(kids.len(), 4);
        assert_eq!(kids[0]["el"]["tag"], "h2");
        let table = &kids[1]["op"];
        assert_eq!(table["type"], "table");
        assert_eq!(table["rows"][0][0], "Setembro");
        assert_eq!(table["rows"][0][1], 1350.0);
        assert_eq!(table["rows"][1][1], 1310.0);
        assert_eq!(kids[2]["el"]["markdown"], "Geral: 2660");
        assert_eq!(kids[3]["op"]["items"][0]["value"]["t"], "link");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dataviewjs_dates_queries_and_errors() {
        let (dir, h) = host();
        let code = r#"
            const d = dv.date("2026-10-21").plus({ months: 1 });
            dv.span(d.toFormat("dd/MM/yyyy"));
            const q = await dv.query('TABLE mes FROM "Fin" SORT file.name');
            dv.span(q.value.values.length + " " + q.value.headers.join(","));
            dv.span(moment("2026-01-31").add(1, "month").format("YYYY-MM-DD"));
            dv.span(dv.current().status);
        "#;
        let r = run_dataviewjs(&h, code, "Home.md");
        let kids = r["root"]["children"].as_array().unwrap();
        assert_eq!(kids[0]["el"]["markdown"], "21/11/2026");
        assert_eq!(kids[1]["el"]["markdown"], "2 File,mes");
        assert_eq!(kids[2]["el"]["markdown"], "2026-02-28");
        assert_eq!(kids[3]["el"]["markdown"], "ok");
        let r = run_dataviewjs(&h, "throw new Error('boom')", "Home.md");
        assert_eq!(r["type"], "error");
        assert!(r["error"].as_str().unwrap().contains("boom"));
        let r = run_dataviewjs(&Host { timeout_secs: 1, ..h.clone() }, "while (true) {}", "Home.md");
        assert!(r["error"].as_str().unwrap().contains("seconds"));
        let r = run_inline_js(&h, "dv.current().file.name", "Home.md");
        assert_eq!(r["root"]["children"][0]["el"]["markdown"], "Home");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn templates() {
        let (dir, mut h) = host();
        h.prompt = Some(Arc::new(|q: Json| match q["kind"].as_str() {
            Some("suggester") => Some(json!(1)),
            Some("prompt") => Some(json!("2027")),
            _ => None,
        }));
        let tpl = "<%*\nconst mes = await tp.system.suggester([\"Janeiro\", \"Fevereiro\"], [\"Janeiro\", \"Fevereiro\"]);\nconst ano = await tp.system.prompt(\"Ano\");\nawait tp.file.rename(`${ano}-${mes}`);\n-%>\n---\ntipo: mensal\nmes: <% mes %>\n---\n# <% mes %> <% ano %>\nCriado em <% tp.date.now(\"YYYY\", 0, \"2026-10-21\", \"YYYY-MM-DD\") %> por <% tp.file.title %>\n<% tp.file.cursor(1) %>fim <%_ \"x\" _%>   z";
        let run = TemplateRun {
            template_path: "T.md".into(),
            template_text: tpl.into(),
            target_path: "Fin/Untitled.md".into(),
            target_content: String::new(),
            selection: String::new(),
            run_mode: 0,
            week_start: 0,
        };
        let out = run_template(&h, &run).unwrap();
        assert!(out.text.starts_with("---\ntipo: mensal\nmes: Fevereiro\n---\n# Fevereiro 2027\nCriado em 2026 por 2027-Fevereiro\n"), "{}", out.text);
        assert!(out.text.ends_with("fimxz"), "{}", out.text);
        assert_eq!(out.cursors.len(), 1);
        assert_eq!(out.ops[0]["op"], "rename");
        assert_eq!(out.ops[0]["path"], "Fin/2027-Fevereiro.md");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn compile_trims() {
        let js = compile_template("a\n<%- 1 -%>\nb");
        assert!(js.contains("tR += \"a\""));
        assert!(js.contains("tR += \"b\""));
    }
}
