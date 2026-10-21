//! Expression evaluation and Dataview's function library.

use super::index::{canonical, Index};
use super::parse::Expr;
use super::value::{compare, compare_text, parse_duration, parse_string_value, Dur, Lambda, Link, Obj, Settings, Value};
use crate::datefmt;
use chrono::{Datelike, NaiveDateTime, NaiveTime, Timelike};
use regex::Regex;
use std::cmp::Ordering;
use std::sync::Arc;

pub struct Ctx<'a> {
    pub index: &'a Index,
    pub settings: &'a Settings,
    pub origin: String,
    pub this: Value,
}

impl<'a> Ctx<'a> {
    pub fn new(index: &'a Index, settings: &'a Settings, origin: &str) -> Ctx<'a> {
        let this = index.page_row(origin).unwrap_or(Value::Null);
        Ctx { index, settings, origin: origin.to_string(), this }
    }

    pub fn eval(&self, e: &Expr, row: &Value) -> Result<Value, String> {
        let mut scope = Vec::new();
        self.ev(e, row, &mut scope)
    }

    fn lookup(&self, name: &str, row: &Value, scope: &[(String, Value)]) -> Value {
        if let Some((_, v)) = scope.iter().rev().find(|(k, _)| k == name) {
            return v.clone();
        }
        get_field(row, name, self.index)
    }

    fn ev(&self, e: &Expr, row: &Value, scope: &mut Vec<(String, Value)>) -> Result<Value, String> {
        Ok(match e {
            Expr::Lit(v) => match v {
                Value::Link(l) => Value::Link(self.index.link_from(&l.target(), l.display.clone(), l.embed, &self.origin)),
                other => other.clone(),
            },
            Expr::Field(name) => self.lookup(name, row, scope),
            Expr::This => self.this.clone(),
            Expr::Row => row.clone(),
            Expr::DateKeyword(k) => date_keyword(k).unwrap_or(Value::Null),
            Expr::Index(obj, idx) => {
                let o = self.ev(obj, row, scope)?;
                let i = self.ev(idx, row, scope)?;
                index_value(&o, &i, self.index)
            }
            Expr::List(items) => Value::List(items.iter().map(|x| self.ev(x, row, scope)).collect::<Result<_, _>>()?),
            Expr::Object(fields) => {
                let mut o = Obj::default();
                for (k, x) in fields {
                    o.insert(k.clone(), self.ev(x, row, scope)?);
                }
                Value::Object(o)
            }
            Expr::Lambda(params, body) => Value::Lambda(Arc::new(Lambda { params: params.clone(), body: (**body).clone() })),
            Expr::Unary(op, x) => {
                let v = self.ev(x, row, scope)?;
                match (op, v) {
                    ('!', v) => Value::Bool(!v.truthy()),
                    ('-', Value::Number(n)) => Value::Number(-n),
                    ('-', Value::Duration(d)) => Value::Duration(d.neg()),
                    ('-', Value::Null) => Value::Null,
                    (_, v) => return Err(format!("cannot negate a {}", v.type_name())),
                }
            }
            Expr::Binary(op, a, b) => {
                if op == "and" {
                    let l = self.ev(a, row, scope)?;
                    return Ok(Value::Bool(l.truthy() && self.ev(b, row, scope)?.truthy()));
                }
                if op == "or" {
                    let l = self.ev(a, row, scope)?;
                    return Ok(Value::Bool(l.truthy() || self.ev(b, row, scope)?.truthy()));
                }
                let l = self.ev(a, row, scope)?;
                let r = self.ev(b, row, scope)?;
                binary(op, &l, &r, self.settings)?
            }
            Expr::Call(f, args) => {
                if let Expr::Field(name) = &**f {
                    let shadowed = scope.iter().any(|(k, _)| k == name);
                    if !shadowed && is_function(name) {
                        let vals: Vec<Value> = args.iter().map(|x| self.ev(x, row, scope)).collect::<Result<_, _>>()?;
                        return self.call(&name.to_lowercase(), vals, row, scope);
                    }
                }
                let fv = self.ev(f, row, scope)?;
                let vals: Vec<Value> = args.iter().map(|x| self.ev(x, row, scope)).collect::<Result<_, _>>()?;
                match fv {
                    Value::Lambda(l) => self.apply(&l, vals, row, scope)?,
                    other => return Err(format!("{} is not a function", other.type_name())),
                }
            }
        })
    }

    pub fn apply(&self, l: &Lambda, args: Vec<Value>, row: &Value, scope: &mut Vec<(String, Value)>) -> Result<Value, String> {
        let n = scope.len();
        for (i, p) in l.params.iter().enumerate() {
            scope.push((p.clone(), args.get(i).cloned().unwrap_or(Value::Null)));
        }
        let r = self.ev(&l.body, row, scope);
        scope.truncate(n);
        r
    }

    fn call_fn(&self, f: &Value, args: Vec<Value>, row: &Value, scope: &mut Vec<(String, Value)>) -> Result<Value, String> {
        match f {
            Value::Lambda(l) => self.apply(l, args, row, scope),
            Value::String(op) => {
                let a = args.first().cloned().unwrap_or(Value::Null);
                let b = args.get(1).cloned().unwrap_or(Value::Null);
                binary(op, &a, &b, self.settings)
            }
            other => Err(format!("expected a function, got {}", other.type_name())),
        }
    }

    fn call(&self, name: &str, args: Vec<Value>, row: &Value, scope: &mut Vec<(String, Value)>) -> Result<Value, String> {
        let s = self.settings;
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        // functions applied element-wise when their first argument is a list
        const VECTOR: &[&str] = &[
            "lower", "upper", "round", "trunc", "floor", "ceil", "number", "string", "striptime", "regextest", "regexmatch", "regexreplace", "replace",
            "startswith", "endswith", "padleft", "padright", "substring", "truncate", "split", "dateformat", "durationformat", "localtime", "default",
            "containsword", "currencyformat", "typeof", "display", "link", "embed", "meta", "date", "dur",
        ];
        if let Some(Value::List(items)) = args.first() {
            if VECTOR.contains(&name) && !(name == "regextest" || name == "regexmatch") {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    let mut a = args.clone();
                    a[0] = it.clone();
                    out.push(self.call(name, a, row, scope)?);
                }
                return Ok(Value::List(out));
            }
        }
        if (name == "regextest" || name == "regexmatch") && matches!(args.get(1), Some(Value::List(_))) {
            let Some(Value::List(items)) = args.get(1) else { unreachable!() };
            let mut out = Vec::new();
            for it in items {
                out.push(self.call(name, vec![arg(0), it.clone()], row, scope)?);
            }
            return Ok(Value::List(out));
        }
        let list_arg = |i: usize| -> Vec<Value> {
            match args.get(i) {
                Some(Value::List(l)) => l.clone(),
                Some(Value::Null) | None => Vec::new(),
                Some(v) => vec![v.clone()],
            }
        };
        let text = |i: usize| -> String {
            match args.get(i) {
                Some(Value::String(t)) => t.clone(),
                Some(Value::Null) | None => String::new(),
                Some(v) => v.to_text(s),
            }
        };
        let num = |i: usize| -> Option<f64> { args.get(i).and_then(Value::as_f64) };
        Ok(match name {
            // ── constructors ───────────────────────────────────────────────
            "object" => {
                let mut o = Obj::default();
                for pair in args.chunks(2) {
                    let k = match &pair[0] {
                        Value::String(k) => k.clone(),
                        v => v.to_text(s),
                    };
                    o.insert(k, pair.get(1).cloned().unwrap_or(Value::Null));
                }
                Value::Object(o)
            }
            "list" | "array" => Value::List(args),
            "link" => match arg(0) {
                Value::Null => Value::Null,
                Value::Link(mut l) => {
                    if let Some(Value::String(d)) = args.get(1) {
                        l.display = Some(d.clone());
                    }
                    Value::Link(l)
                }
                v => {
                    let t = v.to_text(s);
                    let display = match args.get(1) {
                        Some(Value::String(d)) => Some(d.clone()),
                        _ => None,
                    };
                    Value::Link(self.index.link_from(&t, display, false, &self.origin))
                }
            },
            "embed" => match arg(0) {
                Value::Link(mut l) => {
                    l.embed = args.get(1).map(Value::truthy).unwrap_or(true);
                    Value::Link(l)
                }
                Value::Null => Value::Null,
                v => return Err(format!("embed() expects a link, got {}", v.type_name())),
            },
            "elink" => {
                let url = text(0);
                let d = if args.len() > 1 { text(1) } else { url.clone() };
                Value::String(format!("[{d}]({url})"))
            }
            "date" => match arg(0) {
                Value::Date(d) => Value::Date(d),
                Value::Link(l) => {
                    let name = l.path.rsplit('/').next().unwrap_or(&l.path).trim_end_matches(".md").to_string();
                    datefmt::parse_iso(&name).map(Value::Date).unwrap_or(Value::Null)
                }
                Value::String(t) => {
                    if let Some(Value::String(fmt)) = args.get(1) {
                        datefmt::parse_moment(&t, &luxon_to_moment(fmt), s.loc()).map(Value::Date).unwrap_or(Value::Null)
                    } else {
                        date_keyword(&t.to_lowercase()).or_else(|| datefmt::parse_iso(&t).map(Value::Date)).unwrap_or(Value::Null)
                    }
                }
                Value::Number(ms) => Value::Date(datefmt::from_millis(ms as i64)),
                _ => Value::Null,
            },
            "dur" => match arg(0) {
                Value::Duration(d) => Value::Duration(d),
                Value::String(t) => parse_duration(&t).map(Value::Duration).unwrap_or(Value::Null),
                Value::Number(ms) => Value::Duration(Dur::from_millis(ms)),
                _ => Value::Null,
            },
            "number" => match arg(0) {
                Value::Number(n) => Value::Number(n),
                Value::String(t) => {
                    static NUM: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"-?[0-9]+(?:\.[0-9]+)?").unwrap());
                    NUM.find(&t).and_then(|m| m.as_str().parse().ok()).map(Value::Number).unwrap_or(Value::Null)
                }
                Value::Bool(b) => Value::Number(b as i32 as f64),
                _ => Value::Null,
            },
            "string" => Value::String(arg(0).to_text(s)),
            "display" => Value::String(display_text(&arg(0), s)),
            "typeof" => Value::str(arg(0).type_name()),
            "striptime" => match arg(0) {
                Value::Date(d) => Value::Date(d.date().and_time(NaiveTime::MIN)),
                v => v,
            },
            "localtime" => arg(0),
            "meta" => match arg(0) {
                Value::Link(l) => {
                    let mut o = Obj::default();
                    o.insert("display", l.display.clone().map(Value::String).unwrap_or(Value::Null));
                    o.insert("embed", Value::Bool(l.embed));
                    o.insert("path", Value::str(&l.path));
                    o.insert("subpath", l.subpath.clone().map(|x| Value::String(x.trim_start_matches('^').to_string())).unwrap_or(Value::Null));
                    o.insert(
                        "type",
                        Value::str(match &l.subpath {
                            Some(x) if x.starts_with('^') => "block",
                            Some(_) => "header",
                            None => "file",
                        }),
                    );
                    Value::Object(o)
                }
                _ => Value::Null,
            },

            // ── numbers ────────────────────────────────────────────────────
            "round" => match arg(0) {
                Value::Number(n) => {
                    let digits = num(1).unwrap_or(0.0).max(0.0) as i32;
                    let k = 10f64.powi(digits);
                    Value::Number((n * k).round() / k)
                }
                v => v,
            },
            "trunc" => num(0).map(|n| Value::Number(n.trunc())).unwrap_or(arg(0)),
            "floor" => num(0).map(|n| Value::Number(n.floor())).unwrap_or(arg(0)),
            "ceil" => num(0).map(|n| Value::Number(n.ceil())).unwrap_or(arg(0)),
            "abs" => num(0).map(|n| Value::Number(n.abs())).unwrap_or(arg(0)),
            "min" | "max" => {
                let items = if args.len() == 1 { list_arg(0) } else { args.clone() };
                let items: Vec<Value> = items.into_iter().filter(|v| !v.is_null()).collect();
                let pick = if name == "min" { Ordering::Less } else { Ordering::Greater };
                items.into_iter().reduce(|a, b| if compare(&b, &a) == pick { b } else { a }).unwrap_or(Value::Null)
            }
            "sum" | "product" => {
                let items = list_arg(0);
                let mut acc: Option<Value> = None;
                let op = if name == "sum" { "+" } else { "*" };
                for v in items.into_iter().filter(|v| !v.is_null()) {
                    acc = Some(match acc {
                        None => v,
                        Some(a) => binary(op, &a, &v, s)?,
                    });
                }
                acc.unwrap_or(if name == "sum" { Value::Number(0.0) } else { Value::Null })
            }
            "average" => {
                let items: Vec<Value> = list_arg(0).into_iter().filter(|v| !v.is_null()).collect();
                if items.is_empty() {
                    Value::Null
                } else {
                    let n = items.len() as f64;
                    let mut acc = items[0].clone();
                    for v in &items[1..] {
                        acc = binary("+", &acc, v, s)?;
                    }
                    binary("/", &acc, &Value::Number(n), s)?
                }
            }
            "reduce" => {
                let items = list_arg(0);
                let f = arg(1);
                let mut it = items.into_iter();
                let mut acc = it.next().unwrap_or(Value::Null);
                for v in it {
                    acc = self.call_fn(&f, vec![acc, v], row, scope)?;
                }
                acc
            }
            "minby" | "maxby" => {
                let items = list_arg(0);
                let f = arg(1);
                let pick = if name == "minby" { Ordering::Less } else { Ordering::Greater };
                let mut best: Option<(Value, Value)> = None;
                for v in items {
                    let k = self.call_fn(&f, vec![v.clone()], row, scope)?;
                    best = match best {
                        Some((bk, bv)) if compare(&k, &bk) != pick => Some((bk, bv)),
                        _ => Some((k, v)),
                    };
                }
                best.map(|(_, v)| v).unwrap_or(Value::Null)
            }
            "currencyformat" => match arg(0) {
                Value::Number(n) => Value::String(currency(n, &text(1), &s.locale)),
                v => v,
            },
            "hash" => {
                use sha2::{Digest, Sha256};
                let h = Sha256::digest(format!("{}{}{}", text(0), text(1), text(2)).as_bytes());
                Value::Number(u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as f64)
            }

            // ── lists and objects ──────────────────────────────────────────
            "length" => match arg(0) {
                Value::List(l) => Value::Number(l.len() as f64),
                Value::Object(o) => Value::Number(o.0.len() as f64),
                Value::String(t) => Value::Number(t.chars().count() as f64),
                Value::Null => Value::Number(0.0),
                _ => Value::Number(1.0),
            },
            "contains" | "icontains" | "econtains" => {
                let needle = arg(1);
                let ci = name == "icontains";
                let exact = name == "econtains";
                fn has(h: &Value, n: &Value, ci: bool, exact: bool, s: &Settings) -> bool {
                    match h {
                        Value::String(t) => match n {
                            Value::String(x) => {
                                if exact {
                                    t == x
                                } else if ci {
                                    t.to_lowercase().contains(&x.to_lowercase())
                                } else {
                                    t.contains(x.as_str())
                                }
                            }
                            other => t.contains(&other.to_text(s)),
                        },
                        Value::List(l) => l.iter().any(|x| if exact { compare(x, n) == Ordering::Equal } else { has(x, n, ci, false, s) }),
                        Value::Object(o) => match n {
                            Value::String(k) => o.get(k).is_some() || (ci && o.get_ci(k).is_some()),
                            _ => false,
                        },
                        Value::Link(l) => match n {
                            Value::Link(m) => l.path == m.path,
                            Value::String(x) => {
                                if ci {
                                    l.path.to_lowercase().contains(&x.to_lowercase())
                                } else {
                                    l.path.contains(x.as_str())
                                }
                            }
                            _ => false,
                        },
                        other => compare(other, n) == Ordering::Equal,
                    }
                }
                Value::Bool(has(&arg(0), &needle, ci, exact, s))
            }
            "containsword" => {
                let hay = text(0).to_lowercase();
                let w = text(1).to_lowercase();
                Value::Bool(hay.split(|c: char| !c.is_alphanumeric()).any(|x| x == w))
            }
            "extract" => match arg(0) {
                Value::Object(o) => {
                    let mut out = Obj::default();
                    for k in &args[1..] {
                        let k = k.to_text(s);
                        out.insert(k.clone(), o.get(&k).cloned().unwrap_or(Value::Null));
                    }
                    Value::Object(out)
                }
                _ => Value::Null,
            },
            "sort" => {
                let mut items = list_arg(0);
                match args.get(1) {
                    Some(f @ Value::Lambda(_)) => {
                        let mut keyed: Vec<(Value, Value)> = Vec::new();
                        for v in items {
                            let k = self.call_fn(f, vec![v.clone()], row, scope)?;
                            keyed.push((k, v));
                        }
                        keyed.sort_by(|a, b| compare(&a.0, &b.0));
                        items = keyed.into_iter().map(|x| x.1).collect();
                    }
                    _ => items.sort_by(compare),
                }
                Value::List(items)
            }
            "reverse" => {
                let mut l = list_arg(0);
                l.reverse();
                Value::List(l)
            }
            "nonnull" => Value::List(list_arg(0).into_iter().filter(|v| !v.is_null()).collect()),
            "all" | "any" | "none" => {
                let (items, f) = match (args.first(), args.get(1)) {
                    (Some(Value::List(l)), Some(f)) => (l.clone(), Some(f.clone())),
                    (Some(Value::List(l)), None) => (l.clone(), None),
                    _ => (args.clone(), None),
                };
                let mut results = Vec::new();
                for v in items {
                    results.push(match &f {
                        Some(f) => self.call_fn(f, vec![v], row, scope)?.truthy(),
                        None => v.truthy(),
                    });
                }
                Value::Bool(match name {
                    "all" => results.iter().all(|x| *x),
                    "any" => results.iter().any(|x| *x),
                    _ => !results.iter().any(|x| *x),
                })
            }
            "join" => {
                let sep = if args.len() > 1 { text(1) } else { ", ".to_string() };
                Value::String(list_arg(0).iter().map(|v| display_text(v, s)).collect::<Vec<_>>().join(&sep))
            }
            "filter" => {
                let f = arg(1);
                let mut out = Vec::new();
                for v in list_arg(0) {
                    if self.call_fn(&f, vec![v.clone()], row, scope)?.truthy() {
                        out.push(v);
                    }
                }
                Value::List(out)
            }
            "map" => {
                let f = arg(1);
                let mut out = Vec::new();
                for v in list_arg(0) {
                    out.push(self.call_fn(&f, vec![v], row, scope)?);
                }
                Value::List(out)
            }
            "unique" => {
                let mut out: Vec<Value> = Vec::new();
                for v in list_arg(0) {
                    if !out.iter().any(|x| compare(x, &v) == Ordering::Equal) {
                        out.push(v);
                    }
                }
                Value::List(out)
            }
            "flat" => {
                let depth = num(1).unwrap_or(1.0) as usize;
                fn flat(l: Vec<Value>, d: usize, out: &mut Vec<Value>) {
                    for v in l {
                        match v {
                            Value::List(inner) if d > 0 => flat(inner, d - 1, out),
                            other => out.push(other),
                        }
                    }
                }
                let mut out = Vec::new();
                flat(list_arg(0), depth, &mut out);
                Value::List(out)
            }
            "slice" => {
                let l = list_arg(0);
                let n = l.len() as i64;
                let norm = |x: f64| -> usize {
                    let x = x as i64;
                    (if x < 0 { (n + x).max(0) } else { x.min(n) }) as usize
                };
                let a = num(1).map(norm).unwrap_or(0);
                let b = num(2).map(norm).unwrap_or(n as usize);
                Value::List(if a < b { l[a..b].to_vec() } else { Vec::new() })
            }
            "firstvalue" => list_arg(0).into_iter().find(|v| !v.is_null()).unwrap_or(Value::Null),
            "choice" => {
                if arg(0).truthy() {
                    arg(1)
                } else {
                    arg(2)
                }
            }
            "default" | "ldefault" => {
                if arg(0).is_null() {
                    arg(1)
                } else {
                    arg(0)
                }
            }

            // ── strings ────────────────────────────────────────────────────
            "lower" => match arg(0) {
                Value::String(t) => Value::String(t.to_lowercase()),
                v => v,
            },
            "upper" => match arg(0) {
                Value::String(t) => Value::String(t.to_uppercase()),
                v => v,
            },
            "regextest" | "regexmatch" => {
                let pat = if name == "regexmatch" { format!("^(?:{})$", text(0)) } else { text(0) };
                let re = Regex::new(&pat).map_err(|e| format!("bad regex: {e}"))?;
                Value::Bool(re.is_match(&text(1)))
            }
            "regexreplace" => {
                let re = Regex::new(&text(1)).map_err(|e| format!("bad regex: {e}"))?;
                Value::String(re.replace_all(&text(0), js_replacement(&text(2)).as_str()).into_owned())
            }
            "replace" => Value::String(text(0).replace(&text(1), &text(2))),
            "split" => {
                let re = Regex::new(&text(1)).map_err(|e| format!("bad regex: {e}"))?;
                let t = text(0);
                let mut parts: Vec<Value> = re.split(&t).map(Value::str).collect();
                if let Some(n) = num(2) {
                    parts.truncate(n as usize);
                }
                Value::List(parts)
            }
            "startswith" => Value::Bool(text(0).starts_with(&text(1))),
            "endswith" => Value::Bool(text(0).ends_with(&text(1))),
            "padleft" | "padright" => {
                let t = text(0);
                let len = num(1).unwrap_or(0.0) as usize;
                let pad = if args.len() > 2 { text(2) } else { " ".into() };
                let have = t.chars().count();
                if have >= len || pad.is_empty() {
                    Value::String(t)
                } else {
                    let fill: String = pad.chars().cycle().take(len - have).collect();
                    Value::String(if name == "padleft" { fill + &t } else { t + &fill })
                }
            }
            "substring" => {
                let chars: Vec<char> = text(0).chars().collect();
                let a = (num(1).unwrap_or(0.0).max(0.0) as usize).min(chars.len());
                let b = num(2).map(|x| (x.max(0.0) as usize).min(chars.len())).unwrap_or(chars.len());
                Value::String(if a < b { chars[a..b].iter().collect() } else { String::new() })
            }
            "truncate" => {
                let t = text(0);
                let len = num(1).unwrap_or(t.len() as f64) as usize;
                let suffix = if args.len() > 2 { text(2) } else { "...".into() };
                if t.chars().count() <= len {
                    Value::String(t)
                } else {
                    let keep = len.saturating_sub(suffix.chars().count());
                    Value::String(t.chars().take(keep).collect::<String>() + &suffix)
                }
            }

            // ── dates ──────────────────────────────────────────────────────
            "dateformat" => match arg(0) {
                Value::Date(d) => Value::String(datefmt::format_luxon(d, &text(1), s.loc())),
                v => v,
            },
            "durationformat" => match arg(0) {
                Value::Duration(d) => Value::String(duration_format(&d, &text(1))),
                v => v,
            },
            other => return Err(format!("unknown function '{other}'")),
        })
    }
}

const FUNCTIONS: &[&str] = &[
    "object", "list", "array", "link", "embed", "elink", "date", "dur", "number", "string", "display", "typeof", "striptime", "localtime", "meta",
    "round", "trunc", "floor", "ceil", "abs", "min", "max", "sum", "product", "average", "reduce", "minby", "maxby", "currencyformat", "hash", "length",
    "contains", "icontains", "econtains", "containsword", "extract", "sort", "reverse", "nonnull", "all", "any", "none", "join", "filter", "map", "unique",
    "flat", "slice", "firstvalue", "choice", "default", "ldefault", "lower", "upper", "regextest", "regexmatch", "regexreplace", "replace", "split",
    "startswith", "endswith", "padleft", "padright", "substring", "truncate", "dateformat", "durationformat",
];

fn is_function(name: &str) -> bool {
    FUNCTIONS.contains(&name.to_lowercase().as_str())
}

/// `$1` / `$<name>` in JavaScript replacement strings → the regex crate's `${1}` / `${name}`.
fn js_replacement(r: &str) -> String {
    let mut out = String::new();
    let mut chars = r.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            match chars.peek() {
                Some(d) if d.is_ascii_digit() => {
                    let mut n = String::new();
                    while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                        n.push(*d);
                        chars.next();
                    }
                    out.push_str(&format!("${{{n}}}"));
                }
                Some('<') => {
                    chars.next();
                    let name: String = chars.by_ref().take_while(|c| *c != '>').collect();
                    out.push_str(&format!("${{{name}}}"));
                }
                Some('$') => {
                    chars.next();
                    out.push_str("$$");
                }
                Some('&') => {
                    chars.next();
                    out.push_str("${0}");
                }
                _ => out.push_str("$$"),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn luxon_to_moment(fmt: &str) -> String {
    fmt.replace("yyyy", "YYYY").replace("yy", "YY").replace("dd", "DD").replace('d', "D").replace("EEEE", "dddd").replace("EEE", "ddd")
}

fn date_keyword(k: &str) -> Option<Value> {
    let now = datefmt::now();
    let today = datefmt::today();
    let d = match k {
        "now" => now,
        "today" => today,
        "tomorrow" => today + chrono::Duration::days(1),
        "yesterday" => today - chrono::Duration::days(1),
        "sow" => datefmt::start_of(now, "isoWeek", 1),
        "eow" => datefmt::end_of(now, "isoWeek", 1),
        "som" => datefmt::start_of(now, "month", 0),
        "eom" => datefmt::end_of(now, "month", 0),
        "soy" => datefmt::start_of(now, "year", 0),
        "eoy" => datefmt::end_of(now, "year", 0),
        _ => return None,
    };
    Some(Value::Date(d))
}

/// What a value looks like as text in a list or a table cell.
pub fn display_text(v: &Value, s: &Settings) -> String {
    match v {
        Value::Link(l) => l.label(),
        Value::Null => String::new(),
        Value::List(l) => l.iter().map(|x| display_text(x, s)).collect::<Vec<_>>().join(", "),
        other => other.to_text(s),
    }
}

fn currency(n: f64, code: &str, locale: &str) -> String {
    let code = if code.is_empty() { if locale.to_lowercase().starts_with("pt") { "BRL" } else { "USD" } } else { code };
    let pt = locale.to_lowercase().starts_with("pt");
    let neg = n < 0.0;
    let cents = (n.abs() * 100.0).round() as i64;
    let int = (cents / 100).to_string();
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            grouped.push(if pt { '.' } else { ',' });
        }
        grouped.push(c);
    }
    let body = format!("{grouped}{}{:02}", if pt { ',' } else { '.' }, cents % 100);
    let sym = match code {
        "BRL" => "R$",
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        other => other,
    };
    let sign = if neg { "-" } else { "" };
    if pt {
        format!("{sign}{sym}\u{a0}{body}")
    } else {
        format!("{sign}{sym}{body}")
    }
}

fn duration_format(d: &Dur, fmt: &str) -> String {
    // Luxon's Duration#toFormat with the units the format asks for.
    let units = [('y', "years"), ('M', "months"), ('w', "weeks"), ('d', "days"), ('h', "hours"), ('m', "minutes"), ('s', "seconds"), ('S', "milliseconds")];
    let used: Vec<char> = {
        let mut v = Vec::new();
        let mut quoted = false;
        for c in fmt.chars() {
            if c == '\'' {
                quoted = !quoted;
            } else if !quoted && units.iter().any(|(u, _)| *u == c) && !v.contains(&c) {
                v.push(c);
            }
        }
        v
    };
    let mut ms = d.to_millis();
    let size = |u: char| -> f64 {
        match u {
            'y' => 365.0 * 86_400_000.0,
            'M' => 30.0 * 86_400_000.0,
            'w' => 7.0 * 86_400_000.0,
            'd' => 86_400_000.0,
            'h' => 3_600_000.0,
            'm' => 60_000.0,
            's' => 1000.0,
            _ => 1.0,
        }
    };
    let mut vals = std::collections::HashMap::new();
    for (u, _) in units.iter().filter(|(u, _)| used.contains(u)) {
        let v = (ms / size(*u)).trunc();
        ms -= v * size(*u);
        vals.insert(*u, v as i64);
    }
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            for q in chars.by_ref() {
                if q == '\'' {
                    break;
                }
                out.push(q);
            }
            continue;
        }
        if let Some(v) = vals.get(&c) {
            let mut width = 1;
            while chars.peek() == Some(&c) {
                chars.next();
                width += 1;
            }
            out.push_str(&format!("{:0width$}", v, width = width));
        } else {
            out.push(c);
        }
    }
    out
}

/// Field access on a row: exact name, then the canonical name.
pub fn get_field(row: &Value, name: &str, index: &Index) -> Value {
    match row {
        Value::Object(o) => o.get(name).or_else(|| o.get(&canonical(name))).cloned().unwrap_or(Value::Null),
        Value::Link(l) => index.deref(l).map(|r| get_field(&r, name, index)).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

pub fn index_value(o: &Value, i: &Value, index: &Index) -> Value {
    match (o, i) {
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        (Value::List(l), Value::Number(n)) => {
            let n = *n as i64;
            let idx = if n < 0 { l.len() as i64 + n } else { n };
            l.get(idx as usize).cloned().unwrap_or(Value::Null)
        }
        (Value::List(l), Value::String(k)) => {
            if k == "length" {
                return Value::Number(l.len() as f64);
            }
            let mut out = Vec::new();
            for x in l {
                match index_value(x, i, index) {
                    Value::List(inner) => out.extend(inner),
                    v => out.push(v),
                }
            }
            Value::List(out)
        }
        (Value::Object(_), Value::String(k)) | (Value::Link(_), Value::String(k)) => get_field(o, k, index),
        (Value::String(t), Value::String(k)) if k == "length" => Value::Number(t.chars().count() as f64),
        (Value::String(t), Value::Number(n)) => t.chars().nth(*n as usize).map(|c| Value::String(c.to_string())).unwrap_or(Value::Null),
        (Value::Date(d), Value::String(k)) => date_part(*d, k),
        (Value::Duration(d), Value::String(k)) => {
            let key = match k.as_str() {
                "years" | "months" | "weeks" | "days" | "hours" | "minutes" | "seconds" | "milliseconds" => k.as_str(),
                "year" => "years",
                "month" => "months",
                "week" => "weeks",
                "day" => "days",
                "hour" => "hours",
                "minute" => "minutes",
                "second" => "seconds",
                "millisecond" => "milliseconds",
                _ => return Value::Null,
            };
            Value::Number(d.get(key))
        }
        _ => Value::Null,
    }
}

fn date_part(d: NaiveDateTime, k: &str) -> Value {
    let n = |x: u32| Value::Number(x as f64);
    match k {
        "year" => Value::Number(d.year() as f64),
        "month" => n(d.month()),
        "day" => n(d.day()),
        "hour" => n(d.hour()),
        "minute" => n(d.minute()),
        "second" => n(d.second()),
        "millisecond" => n(d.and_utc().timestamp_subsec_millis()),
        "week" | "weekNumber" => n(d.iso_week().week()),
        "weekyear" | "weekYear" => Value::Number(d.iso_week().year() as f64),
        "weekday" => n(d.weekday().number_from_monday()),
        "quarter" => n(d.month0() / 3 + 1),
        "ordinal" => n(d.ordinal()),
        "ts" => Value::Number(datefmt::to_millis(d) as f64),
        "daysInMonth" => n(datefmt::days_in_month(d.year(), d.month())),
        _ => Value::Null,
    }
}

pub fn binary(op: &str, l: &Value, r: &Value, s: &Settings) -> Result<Value, String> {
    use Value::*;
    let cmp = |o: Ordering| -> bool {
        match op {
            "=" => o == Ordering::Equal,
            "!=" => o != Ordering::Equal,
            "<" => o == Ordering::Less,
            ">" => o == Ordering::Greater,
            "<=" => o != Ordering::Greater,
            ">=" => o != Ordering::Less,
            _ => false,
        }
    };
    if matches!(op, "=" | "!=" | "<" | ">" | "<=" | ">=") {
        // dates compare with the date strings Dataview users often write
        let o = match (l, r) {
            (Date(_), String(t)) => compare(l, &parse_string_value(t)),
            (String(t), Date(_)) => compare(&parse_string_value(t), r),
            (Link(a), String(t)) | (String(t), Link(a)) => {
                let o = compare_text(&a.label(), t);
                if matches!(l, String(_)) {
                    o.reverse()
                } else {
                    o
                }
            }
            _ => compare(l, r),
        };
        return Ok(Bool(cmp(o)));
    }
    Ok(match (op, l, r) {
        (_, Null, _) | (_, _, Null) => {
            if op == "+" {
                match (l, r) {
                    (String(a), Null) => String(a.clone() + "null"),
                    (Null, String(b)) => String("null".to_string() + b),
                    _ => Null,
                }
            } else {
                Null
            }
        }
        ("+", Number(a), Number(b)) => Number(a + b),
        ("-", Number(a), Number(b)) => Number(a - b),
        ("*", Number(a), Number(b)) => Number(a * b),
        ("/", Number(a), Number(b)) => Number(a / b),
        ("%", Number(a), Number(b)) => Number(a % b),
        ("+", String(a), b) => String(a.clone() + &display_text(b, s)),
        ("+", a, String(b)) => String(display_text(a, s) + b),
        ("*", String(a), Number(n)) | ("*", Number(n), String(a)) => String(a.repeat(n.max(0.0) as usize)),
        ("+", Date(d), Duration(x)) | ("+", Duration(x), Date(d)) => Date(x.apply(*d)),
        ("-", Date(d), Duration(x)) => Date(x.neg().apply(*d)),
        ("-", Date(a), Date(b)) => Duration(Dur::between(*a, *b)),
        ("+", Duration(a), Duration(b)) => Duration(a.add(b)),
        ("-", Duration(a), Duration(b)) => Duration(a.add(&b.neg())),
        ("*", Duration(a), Number(n)) | ("*", Number(n), Duration(a)) => Duration(a.scale(*n)),
        ("/", Duration(a), Number(n)) => Duration(a.scale(1.0 / n)),
        ("/", Duration(a), Duration(b)) => Number(a.to_millis() / b.to_millis()),
        ("+", List(a), List(b)) => List(a.iter().chain(b).cloned().collect()),
        ("+", List(a), b) => List(a.iter().cloned().chain(std::iter::once(b.clone())).collect()),
        ("+", Object(a), Object(b)) => {
            let mut o = a.clone();
            for (k, v) in &b.0 {
                o.insert(k.clone(), v.clone());
            }
            Object(o)
        }
        (op, a, b) => return Err(format!("cannot apply '{op}' to {} and {}", a.type_name(), b.type_name())),
    })
}

/// Shortcut used by the query runner for `file.link` of a path.
pub fn file_link(path: &str) -> Value {
    Value::Link(Link::file(path))
}
