//! Dataview values: what fields hold and what expressions compute.

use crate::datefmt::{self, Locale};
use chrono::{Datelike, Duration as ChronoDuration, NaiveDateTime, Timelike};
use serde_json::{json, Map as JsonMap, Value as Json};
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// Resolved vault path when the target exists, otherwise the link text.
    pub path: String,
    pub display: Option<String>,
    pub subpath: Option<String>,
    pub embed: bool,
}

impl Link {
    pub fn file(path: &str) -> Link {
        Link { path: path.to_string(), display: None, subpath: None, embed: false }
    }

    /// What Dataview shows: the alias, or the file name without `.md`
    /// (followed by ` > heading` for section links).
    pub fn label(&self) -> String {
        if let Some(d) = &self.display {
            return d.clone();
        }
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        let name = name.strip_suffix(".md").unwrap_or(name);
        match &self.subpath {
            Some(s) => format!("{name} > {}", s.trim_start_matches('^')),
            None => name.to_string(),
        }
    }

    /// The text Obsidian would resolve: `path#subpath`.
    pub fn target(&self) -> String {
        match &self.subpath {
            Some(s) => format!("{}#{}", self.path, s),
            None => self.path.clone(),
        }
    }

    pub fn markdown(&self) -> String {
        let t = self.target();
        let t = t.strip_suffix(".md").unwrap_or(&t);
        let bang = if self.embed { "!" } else { "" };
        match &self.display {
            Some(d) => format!("{bang}[[{t}|{d}]]"),
            None => format!("{bang}[[{t}]]"),
        }
    }
}

/// A Luxon-style duration: calendar parts are kept apart so that adding
/// "1 month" to a date moves to the same day of the next month.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dur {
    pub years: f64,
    pub months: f64,
    pub weeks: f64,
    pub days: f64,
    pub hours: f64,
    pub minutes: f64,
    pub seconds: f64,
    pub millis: f64,
}

impl Dur {
    pub const PARTS: [&'static str; 8] = ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds"];

    pub fn get(&self, part: &str) -> f64 {
        match part {
            "years" => self.years,
            "months" => self.months,
            "weeks" => self.weeks,
            "days" => self.days,
            "hours" => self.hours,
            "minutes" => self.minutes,
            "seconds" => self.seconds,
            "milliseconds" => self.millis,
            _ => 0.0,
        }
    }

    pub fn set(&mut self, part: &str, v: f64) {
        match part {
            "years" => self.years = v,
            "months" => self.months = v,
            "weeks" => self.weeks = v,
            "days" => self.days = v,
            "hours" => self.hours = v,
            "minutes" => self.minutes = v,
            "seconds" => self.seconds = v,
            "milliseconds" => self.millis = v,
            _ => {}
        }
    }

    /// Luxon's casual conversion (a year is 365 days, a month 30).
    pub fn to_millis(&self) -> f64 {
        ((((self.years * 365.0 + self.months * 30.0 + self.weeks * 7.0 + self.days) * 24.0 + self.hours) * 60.0 + self.minutes) * 60.0 + self.seconds)
            * 1000.0
            + self.millis
    }

    pub fn from_millis(ms: f64) -> Dur {
        Dur { millis: ms, ..Default::default() }.normalized()
    }

    /// Spread milliseconds into days, hours, minutes, seconds and milliseconds.
    pub fn normalized(&self) -> Dur {
        let mut ms = ((self.weeks * 7.0 + self.days) * 86_400_000.0 + self.hours * 3_600_000.0 + self.minutes * 60_000.0 + self.seconds * 1000.0 + self.millis).round();
        let sign = if ms < 0.0 { -1.0 } else { 1.0 };
        ms = ms.abs();
        let days = (ms / 86_400_000.0).floor();
        ms -= days * 86_400_000.0;
        let hours = (ms / 3_600_000.0).floor();
        ms -= hours * 3_600_000.0;
        let minutes = (ms / 60_000.0).floor();
        ms -= minutes * 60_000.0;
        let seconds = (ms / 1000.0).floor();
        ms -= seconds * 1000.0;
        Dur { years: self.years, months: self.months, weeks: 0.0, days: sign * days, hours: sign * hours, minutes: sign * minutes, seconds: sign * seconds, millis: sign * ms }
    }

    pub fn neg(&self) -> Dur {
        Dur {
            years: -self.years,
            months: -self.months,
            weeks: -self.weeks,
            days: -self.days,
            hours: -self.hours,
            minutes: -self.minutes,
            seconds: -self.seconds,
            millis: -self.millis,
        }
    }

    pub fn add(&self, o: &Dur) -> Dur {
        Dur {
            years: self.years + o.years,
            months: self.months + o.months,
            weeks: self.weeks + o.weeks,
            days: self.days + o.days,
            hours: self.hours + o.hours,
            minutes: self.minutes + o.minutes,
            seconds: self.seconds + o.seconds,
            millis: self.millis + o.millis,
        }
    }

    pub fn scale(&self, k: f64) -> Dur {
        Dur {
            years: self.years * k,
            months: self.months * k,
            weeks: self.weeks * k,
            days: self.days * k,
            hours: self.hours * k,
            minutes: self.minutes * k,
            seconds: self.seconds * k,
            millis: self.millis * k,
        }
    }

    pub fn apply(&self, dt: NaiveDateTime) -> NaiveDateTime {
        let whole_months = self.years * 12.0 + self.months;
        let mut d = datefmt::add_months(dt, whole_months.trunc() as i64);
        let rest = Dur { years: 0.0, months: 0.0, ..*self };
        let mut ms = rest.to_millis() + whole_months.fract() * 30.0 * 86_400_000.0;
        // Whole days move the calendar date, so a DST change keeps the clock time.
        let days = (ms / 86_400_000.0).trunc();
        d += ChronoDuration::days(days as i64);
        ms -= days * 86_400_000.0;
        d + ChronoDuration::milliseconds(ms.round() as i64)
    }

    /// Calendar-aware difference `a - b` in years, months, days, hours, minutes, seconds.
    pub fn between(a: NaiveDateTime, b: NaiveDateTime) -> Dur {
        if a < b {
            return Dur::between(b, a).neg();
        }
        let mut months = (a.year() - b.year()) as i64 * 12 + a.month() as i64 - b.month() as i64;
        while months > 0 && datefmt::add_months(b, months) > a {
            months -= 1;
        }
        let anchor = datefmt::add_months(b, months);
        let rest = (a - anchor).num_milliseconds() as f64;
        let mut d = Dur::from_millis(rest);
        d.years = (months / 12) as f64;
        d.months = (months % 12) as f64;
        d
    }

    /// "1 year, 2 months, 3 days" (zero parts omitted).
    pub fn human(&self, loc: &Locale) -> String {
        let pt = loc.months[0] == "janeiro";
        let names: [(&str, &str); 8] = if pt {
            [("ano", "anos"), ("mês", "meses"), ("semana", "semanas"), ("dia", "dias"), ("hora", "horas"), ("minuto", "minutos"), ("segundo", "segundos"), ("milissegundo", "milissegundos")]
        } else {
            [("year", "years"), ("month", "months"), ("week", "weeks"), ("day", "days"), ("hour", "hours"), ("minute", "minutes"), ("second", "seconds"), ("millisecond", "milliseconds")]
        };
        let mut parts = Vec::new();
        for (i, p) in Self::PARTS.iter().enumerate() {
            let v = self.get(p);
            if v != 0.0 {
                let n = if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{:.2}", v).trim_end_matches('0').to_string() };
                parts.push(format!("{n} {}", if v.abs() == 1.0 { names[i].0 } else { names[i].1 }));
            }
        }
        if parts.is_empty() {
            return format!("0 {}", names[7].1);
        }
        parts.join(", ")
    }
}

#[derive(Clone, Debug)]
pub struct Lambda {
    pub params: Vec<String>,
    pub body: super::parse::Expr,
}

#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Date(NaiveDateTime),
    Duration(Dur),
    Link(Link),
    List(Vec<Value>),
    Object(Obj),
    Lambda(Arc<Lambda>),
}

/// An insertion-ordered map (Dataview keeps field order for display).
#[derive(Clone, Debug, Default)]
pub struct Obj(pub Vec<(String, Value)>);

impl Obj {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }

    pub fn get_ci(&self, k: &str) -> Option<&Value> {
        self.get(k).or_else(|| {
            let lower = k.to_lowercase();
            self.0.iter().find(|(key, _)| key.to_lowercase() == lower).map(|(_, v)| v)
        })
    }

    pub fn insert(&mut self, k: impl Into<String>, v: Value) {
        let k = k.into();
        if let Some(slot) = self.0.iter_mut().find(|(key, _)| *key == k) {
            slot.1 = v;
        } else {
            self.0.push((k, v));
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(k, _)| k)
    }
}

impl PartialEq for Value {
    fn eq(&self, o: &Value) -> bool {
        compare(self, o) == Ordering::Equal
    }
}

impl Value {
    pub fn str(s: impl Into<String>) -> Value {
        Value::String(s.into())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Date(_) => "date",
            Value::Duration(_) => "duration",
            Value::Link(_) => "link",
            Value::List(_) => "array",
            Value::Object(_) => "object",
            Value::Lambda(_) => "function",
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Date(_) | Value::Link(_) | Value::Lambda(_) => true,
            Value::Duration(d) => d.to_millis() != 0.0,
            Value::List(l) => !l.is_empty(),
            Value::Object(o) => !o.0.is_empty(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Plain-text form used by `string()`, `join()` and string concatenation.
    pub fn to_text(&self, s: &Settings) -> String {
        match self {
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => fmt_number(*n),
            Value::String(t) => t.clone(),
            Value::Date(d) => fmt_date(*d, s),
            Value::Duration(d) => d.human(s.loc()),
            Value::Link(l) => l.markdown(),
            Value::List(l) => l.iter().map(|v| v.to_text(s)).collect::<Vec<_>>().join(", "),
            Value::Object(o) => format!("{{ {} }}", o.0.iter().map(|(k, v)| format!("{k}: {}", v.to_text(s))).collect::<Vec<_>>().join(", ")),
            Value::Lambda(_) => "<function>".into(),
        }
    }

    /// The tagged JSON the interface renders (dates and durations already
    /// formatted with the user's settings).
    pub fn to_display(&self, s: &Settings) -> Json {
        match self {
            Value::Null => Json::Null,
            Value::Bool(b) => json!(b),
            Value::Number(n) => {
                if n.is_finite() {
                    json!(n)
                } else {
                    json!(fmt_number(*n))
                }
            }
            Value::String(t) => json!(t),
            Value::Date(d) => json!({ "t": "date", "text": fmt_date(*d, s), "iso": d.format("%Y-%m-%dT%H:%M:%S").to_string() }),
            Value::Duration(d) => json!({ "t": "dur", "text": d.human(s.loc()) }),
            Value::Link(l) => json!({ "t": "link", "path": l.path, "subpath": l.subpath, "display": l.label(), "embed": l.embed }),
            Value::List(l) => Json::Array(l.iter().map(|v| v.to_display(s)).collect()),
            Value::Object(o) => json!({ "t": "obj", "entries": o.0.iter().map(|(k, v)| json!([k, v.to_display(s)])).collect::<Vec<_>>() }),
            Value::Lambda(_) => json!("<function>"),
        }
    }

    /// The JSON shape passed to the embedded JavaScript engine, which turns the
    /// tagged objects back into dates, durations and links.
    pub fn to_wire(&self) -> Json {
        match self {
            Value::Null | Value::Lambda(_) => Json::Null,
            Value::Bool(b) => json!(b),
            Value::Number(n) => json!(n),
            Value::String(t) => json!(t),
            Value::Date(d) => json!({ "$date": datefmt::to_millis(*d) }),
            Value::Duration(d) => {
                let mut m = JsonMap::new();
                for p in Dur::PARTS {
                    let v = d.get(p);
                    if v != 0.0 {
                        m.insert(p.to_string(), json!(v));
                    }
                }
                json!({ "$dur": m })
            }
            Value::Link(l) => json!({ "$link": { "path": l.path, "display": l.display, "subpath": l.subpath, "embed": l.embed } }),
            Value::List(l) => Json::Array(l.iter().map(Value::to_wire).collect()),
            Value::Object(o) => Json::Object(o.0.iter().map(|(k, v)| (k.clone(), v.to_wire())).collect()),
        }
    }

    pub fn from_wire(j: &Json) -> Value {
        match j {
            Json::Null => Value::Null,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
            Json::String(s) => Value::String(s.clone()),
            Json::Array(a) => Value::List(a.iter().map(Value::from_wire).collect()),
            Json::Object(o) => {
                if o.len() == 1 {
                    if let Some(ms) = o.get("$date").and_then(Json::as_f64) {
                        return Value::Date(datefmt::from_millis(ms as i64));
                    }
                    if let Some(Json::Object(parts)) = o.get("$dur") {
                        let mut d = Dur::default();
                        for (k, v) in parts {
                            d.set(k, v.as_f64().unwrap_or(0.0));
                        }
                        return Value::Duration(d);
                    }
                    if let Some(Json::Object(l)) = o.get("$link") {
                        let s = |k: &str| l.get(k).and_then(Json::as_str).map(str::to_string);
                        return Value::Link(Link {
                            path: s("path").unwrap_or_default(),
                            display: s("display"),
                            subpath: s("subpath"),
                            embed: l.get("embed").and_then(Json::as_bool).unwrap_or(false),
                        });
                    }
                }
                Value::Object(Obj(o.iter().map(|(k, v)| (k.clone(), Value::from_wire(v))).collect()))
            }
        }
    }

    /// YAML/JSON from frontmatter: strings that look like dates, durations or
    /// links become those (as Dataview does).
    pub fn from_frontmatter(j: &Json) -> Value {
        match j {
            Json::String(s) => parse_string_value(s),
            Json::Array(a) => Value::List(a.iter().map(Value::from_frontmatter).collect()),
            Json::Object(o) => Value::Object(Obj(o.iter().map(|(k, v)| (k.clone(), Value::from_frontmatter(v))).collect())),
            other => Value::from_wire(other),
        }
    }
}

fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Number(_) => 1,
        Value::String(_) => 2,
        Value::Bool(_) => 3,
        Value::Date(_) => 4,
        Value::Duration(_) => 5,
        Value::Link(_) => 6,
        Value::List(_) => 7,
        Value::Object(_) => 8,
        Value::Lambda(_) => 9,
    }
}

/// Text comparison close to `localeCompare`: letters first ignoring case and
/// accents, then the exact text to break ties.
pub fn compare_text(a: &str, b: &str) -> Ordering {
    use unicode_normalization::UnicodeNormalization;
    let fold = |s: &str| s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).flat_map(char::to_lowercase).collect::<String>();
    fold(a).cmp(&fold(b)).then_with(|| a.cmp(b))
}

/// Total order over values (Dataview's `compareValue`).
pub fn compare(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => compare_text(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Date(x), Value::Date(y)) => x.cmp(y),
        (Value::Duration(x), Value::Duration(y)) => x.to_millis().partial_cmp(&y.to_millis()).unwrap_or(Ordering::Equal),
        (Value::Link(x), Value::Link(y)) => {
            let px = x.path.to_lowercase();
            let py = y.path.to_lowercase();
            px.cmp(&py).then_with(|| x.subpath.cmp(&y.subpath))
        }
        (Value::List(x), Value::List(y)) => {
            for (p, q) in x.iter().zip(y) {
                let c = compare(p, q);
                if c != Ordering::Equal {
                    return c;
                }
            }
            x.len().cmp(&y.len())
        }
        (Value::Object(x), Value::Object(y)) => {
            let mut kx: Vec<&String> = x.keys().collect();
            let mut ky: Vec<&String> = y.keys().collect();
            kx.sort();
            ky.sort();
            let c = kx.cmp(&ky);
            if c != Ordering::Equal {
                return c;
            }
            for k in kx {
                let c = compare(x.get(k).unwrap(), y.get(k).unwrap());
                if c != Ordering::Equal {
                    return c;
                }
            }
            Ordering::Equal
        }
        _ => type_rank(a).cmp(&type_rank(b)),
    }
}

pub fn fmt_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let s = format!("{n}");
    s
}

pub fn fmt_date(d: NaiveDateTime, s: &Settings) -> String {
    let fmt = if d.time().num_seconds_from_midnight() == 0 && d.and_utc().timestamp_subsec_millis() == 0 { &s.date_format } else { &s.datetime_format };
    datefmt::format_luxon(d, fmt, s.loc())
}

/// Display settings shared by queries (from the Dataview settings).
#[derive(Clone, Debug)]
pub struct Settings {
    pub date_format: String,
    pub datetime_format: String,
    pub locale: String,
    pub null_as: String,
    pub id_column: String,
    pub group_column: String,
    pub show_count: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            date_format: "MMMM dd, yyyy".into(),
            datetime_format: "h:mm a - MMMM dd, yyyy".into(),
            locale: "en".into(),
            null_as: "\\-".into(),
            id_column: "File".into(),
            group_column: "Group".into(),
            show_count: true,
        }
    }
}

impl Settings {
    pub fn loc(&self) -> &'static Locale {
        datefmt::locale(&self.locale)
    }
}

// ── literal parsing shared by fields and expressions ───────────────────────

/// `[[target#sub|alias]]` (optionally `![[…]]`) as an unresolved link.
pub fn parse_link_literal(s: &str) -> Option<Link> {
    let t = s.trim();
    let (embed, t) = match t.strip_prefix('!') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let inner = t.strip_prefix("[[")?.strip_suffix("]]")?;
    if inner.contains("[[") || inner.contains("]]") {
        return None;
    }
    let (target, alias) = crate::syntax::split_wikilink(inner);
    let (path, sub) = match target.split_once('#') {
        Some((p, s)) => (p.to_string(), Some(s.to_string())),
        None => (target.to_string(), None),
    };
    Some(Link { path, display: alias.map(str::to_string), subpath: sub, embed })
}

/// `1 day`, `2h 30m`, `3 weeks, 2 days`, `1yr`.
pub fn parse_duration(s: &str) -> Option<Dur> {
    let t = s.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    let mut d = Dur::default();
    let mut any = false;
    let mut rest = t.as_str();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if let Some(r) = rest.strip_prefix("and ") {
            rest = r;
            continue;
        }
        if rest.is_empty() {
            break;
        }
        let n_len = rest.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.' || *b == b'-').count();
        if n_len == 0 {
            return None;
        }
        let n: f64 = rest[..n_len].parse().ok()?;
        rest = rest[n_len..].trim_start();
        let u_len = rest.chars().take_while(|c| c.is_alphabetic()).map(char::len_utf8).sum::<usize>();
        let unit = &rest[..u_len];
        rest = &rest[u_len..];
        let part = match unit {
            "y" | "yr" | "yrs" | "year" | "years" | "ano" | "anos" => "years",
            "mo" | "mos" | "month" | "months" | "mês" | "mes" | "meses" => "months",
            "w" | "wk" | "wks" | "week" | "weeks" | "semana" | "semanas" => "weeks",
            "d" | "day" | "days" | "dia" | "dias" => "days",
            "h" | "hr" | "hrs" | "hour" | "hours" | "hora" | "horas" => "hours",
            "m" | "min" | "mins" | "minute" | "minutes" | "minuto" | "minutos" => "minutes",
            "s" | "sec" | "secs" | "second" | "seconds" | "segundo" | "segundos" => "seconds",
            "ms" | "millisecond" | "milliseconds" => "milliseconds",
            _ => return None,
        };
        d.set(part, d.get(part) + n);
        any = true;
    }
    any.then_some(d)
}

/// Values written as text in frontmatter or inline fields.
pub fn parse_string_value(s: &str) -> Value {
    let t = s.trim();
    if let Some(l) = parse_link_literal(t) {
        return Value::Link(l);
    }
    if let Some(d) = datefmt::parse_iso(t) {
        // a bare year-month (`2024-05`) reads as a date only with a day
        if t.len() >= 10 {
            return Value::Date(d);
        }
    }
    if t.chars().any(|c| c.is_ascii_digit()) && t.chars().any(|c| c.is_alphabetic()) {
        if let Some(d) = parse_duration(t) {
            return Value::Duration(d);
        }
    }
    Value::String(s.to_string())
}

/// Inline field values (`key:: value`): numbers and booleans too, and a
/// comma-separated list when every item is a link.
pub fn parse_inline_value(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        return Value::Null;
    }
    if let Ok(n) = t.parse::<f64>() {
        if t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') && n.is_finite() {
            return Value::Number(n);
        }
    }
    match t.to_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if t.contains(',') && t.contains("[[") {
        let parts: Vec<Option<Link>> = t.split(',').map(parse_link_literal).collect();
        if parts.len() > 1 && parts.iter().all(Option::is_some) {
            return Value::List(parts.into_iter().map(|l| Value::Link(l.unwrap())).collect());
        }
    }
    parse_string_value(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        let d = parse_duration("1 day, 2h 30 minutes").unwrap();
        assert_eq!(d.days, 1.0);
        assert_eq!(d.hours, 2.0);
        assert_eq!(d.minutes, 30.0);
        assert!(parse_duration("tomorrow").is_none());
        let a = datefmt::parse_iso("2026-03-15T10:00").unwrap();
        let b = datefmt::parse_iso("2026-01-10T08:30").unwrap();
        let diff = Dur::between(a, b);
        assert_eq!((diff.months, diff.days, diff.hours, diff.minutes), (2.0, 5.0, 1.0, 30.0));
        assert_eq!(diff.apply(b), a);
    }

    #[test]
    fn inline_values() {
        assert!(matches!(parse_inline_value("42"), Value::Number(n) if n == 42.0));
        assert!(matches!(parse_inline_value("2026-10-21"), Value::Date(_)));
        assert!(matches!(parse_inline_value("[[A]], [[B]]"), Value::List(l) if l.len() == 2));
        assert!(matches!(parse_inline_value("3 days"), Value::Duration(_)));
        assert!(matches!(parse_inline_value("hello, world"), Value::String(_)));
        assert!(matches!(parse_inline_value("Rua 7"), Value::String(_)));
    }

    #[test]
    fn ordering() {
        assert_eq!(compare(&Value::str("b"), &Value::str("Á")), Ordering::Greater);
        assert_eq!(compare(&Value::Null, &Value::Number(1.0)), Ordering::Less);
        assert_eq!(compare(&Value::Number(2.0), &Value::Number(10.0)), Ordering::Less);
    }
}
