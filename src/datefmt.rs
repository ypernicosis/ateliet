//! Date formatting and parsing with the two token dialects the tools speak:
//! moment's (`YYYY-MM-DD`, used by daily notes, the calendar and Templater)
//! and Luxon's (`yyyy-MM-dd`, used by Dataview). Month and weekday names come
//! from a small locale table (English and Portuguese); other locales fall back
//! to English.

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};

pub struct Locale {
    pub months: [&'static str; 12],
    pub months_short: [&'static str; 12],
    /// Sunday first.
    pub weekdays: [&'static str; 7],
    pub weekdays_short: [&'static str; 7],
    pub weekdays_min: [&'static str; 7],
    pub meridiem: [&'static str; 2],
    /// First day of the week, 0 = Sunday.
    pub week_start: u32,
    pt: bool,
}

static EN: Locale = Locale {
    months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    months_short: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    weekdays: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
    weekdays_short: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    weekdays_min: ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"],
    meridiem: ["AM", "PM"],
    week_start: 0,
    pt: false,
};

static PT: Locale = Locale {
    months: ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
    months_short: ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
    weekdays: ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"],
    weekdays_short: ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"],
    weekdays_min: ["do", "2ª", "3ª", "4ª", "5ª", "6ª", "sá"],
    meridiem: ["AM", "PM"],
    week_start: 0,
    pt: true,
};

pub fn locale(name: &str) -> &'static Locale {
    if name.to_ascii_lowercase().starts_with("pt") {
        &PT
    } else {
        &EN
    }
}

pub fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

pub fn today() -> NaiveDateTime {
    now().date().and_time(NaiveTime::MIN)
}

pub fn to_millis(dt: NaiveDateTime) -> i64 {
    match Local.from_local_datetime(&dt) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => t.timestamp_millis(),
        chrono::LocalResult::None => dt.and_utc().timestamp_millis(),
    }
}

pub fn from_millis(ms: i64) -> NaiveDateTime {
    Local.timestamp_millis_opt(ms).single().map(|t| t.naive_local()).unwrap_or_default()
}

fn offset_minutes(dt: NaiveDateTime) -> i64 {
    (to_millis(dt) - dt.and_utc().timestamp_millis()) / -60_000
}

fn zone(dt: NaiveDateTime, colon: bool) -> String {
    let m = offset_minutes(dt);
    let sign = if m < 0 { '-' } else { '+' };
    let m = m.abs();
    if colon {
        format!("{sign}{:02}:{:02}", m / 60, m % 60)
    } else {
        format!("{sign}{:02}{:02}", m / 60, m % 60)
    }
}

fn hour12(h: u32) -> u32 {
    match h % 12 {
        0 => 12,
        x => x,
    }
}

fn ordinal(n: u32, loc: &Locale) -> String {
    if loc.pt {
        return format!("{n}º");
    }
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Week of year where weeks start on `start` and week 1 holds January 1st
/// (moment's `w` for English and Portuguese).
fn locale_week(d: NaiveDate, start: u32) -> u32 {
    let jan1 = NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap();
    let off = (jan1.weekday().num_days_from_sunday() + 7 - start) % 7;
    (d.ordinal0() + off) / 7 + 1
}

enum Tok<'a> {
    Lit(String),
    Field(&'a str),
}

const MOMENT_TOKENS: &[&str] = &[
    "YYYYYY", "YYYY", "gggg", "GGGG", "MMMM", "dddd", "DDDD", "SSSS", "LLLL", "llll", "SSS", "MMM", "ddd", "DDD", "LLL", "lll", "YY", "gg", "GG",
    "MM", "DD", "Do", "dd", "ww", "WW", "HH", "hh", "kk", "mm", "ss", "SS", "ZZ", "LTS", "LT", "LL", "ll", "Y", "M", "D", "d", "e", "E", "w",
    "W", "H", "h", "k", "m", "s", "S", "A", "a", "Q", "X", "x", "Z", "L", "l",
];

const LUXON_TOKENS: &[&str] = &[
    "yyyyyy", "yyyy", "kkkk", "MMMMM", "LLLLL", "EEEEE", "ccccc", "MMMM", "LLLL", "EEEE", "cccc", "DDDD", "TTTT", "tttt", "ffff", "FFFF",
    "MMM", "LLL", "EEE", "ccc", "DDD", "TTT", "ttt", "fff", "FFF", "ooo", "SSS", "ZZZ", "yy", "kk", "MM", "LL", "dd", "HH", "hh", "mm", "ss",
    "WW", "DD", "TT", "tt", "ff", "FF", "ZZ", "qq", "y", "M", "L", "d", "E", "c", "H", "h", "m", "s", "S", "a", "W", "D", "T", "t", "f", "F",
    "o", "q", "Z", "X", "x", "u",
];

fn tokenize(fmt: &str, luxon: bool) -> Vec<Tok<'_>> {
    let table = if luxon { LUXON_TOKENS } else { MOMENT_TOKENS };
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    let b = fmt.as_bytes();
    while i < fmt.len() {
        let c = b[i];
        if !luxon && c == b'[' {
            if let Some(end) = fmt[i + 1..].find(']') {
                lit.push_str(&fmt[i + 1..i + 1 + end]);
                i += end + 2;
                continue;
            }
        }
        if luxon && c == b'\'' {
            match fmt[i + 1..].find('\'') {
                Some(0) => {
                    lit.push('\'');
                    i += 2;
                }
                Some(end) => {
                    lit.push_str(&fmt[i + 1..i + 1 + end]);
                    i += end + 2;
                }
                None => {
                    lit.push_str(&fmt[i + 1..]);
                    i = fmt.len();
                }
            }
            continue;
        }
        if let Some(t) = table.iter().find(|t| fmt[i..].starts_with(**t)) {
            if !lit.is_empty() {
                out.push(Tok::Lit(std::mem::take(&mut lit)));
            }
            out.push(Tok::Field(&fmt[i..i + t.len()]));
            i += t.len();
            continue;
        }
        let ch = fmt[i..].chars().next().unwrap();
        lit.push(ch);
        i += ch.len_utf8();
    }
    if !lit.is_empty() {
        out.push(Tok::Lit(lit));
    }
    out
}

/// Format with moment tokens.
pub fn format_moment(dt: NaiveDateTime, fmt: &str, loc: &Locale) -> String {
    let mut s = String::new();
    for t in tokenize(fmt, false) {
        match t {
            Tok::Lit(l) => s.push_str(&l),
            Tok::Field(f) => s.push_str(&moment_field(dt, f, loc)),
        }
    }
    s
}

fn moment_field(dt: NaiveDateTime, f: &str, loc: &Locale) -> String {
    let d = dt.date();
    let wd = d.weekday().num_days_from_sunday() as usize;
    let iso = d.iso_week();
    match f {
        "YYYYYY" => format!("{:+07}", d.year()),
        "YYYY" | "Y" => format!("{:04}", d.year()),
        "YY" => format!("{:02}", d.year().rem_euclid(100)),
        "gggg" => format!("{:04}", d.year()),
        "gg" => format!("{:02}", d.year().rem_euclid(100)),
        "GGGG" => format!("{:04}", iso.year()),
        "GG" => format!("{:02}", iso.year().rem_euclid(100)),
        "M" => d.month().to_string(),
        "MM" => format!("{:02}", d.month()),
        "MMM" => loc.months_short[d.month0() as usize].to_string(),
        "MMMM" => loc.months[d.month0() as usize].to_string(),
        "Q" => ((d.month0() / 3) + 1).to_string(),
        "D" => d.day().to_string(),
        "DD" => format!("{:02}", d.day()),
        "Do" => ordinal(d.day(), loc),
        "DDD" => d.ordinal().to_string(),
        "DDDD" => format!("{:03}", d.ordinal()),
        "d" => wd.to_string(),
        "dd" => loc.weekdays_min[wd].to_string(),
        "ddd" => loc.weekdays_short[wd].to_string(),
        "dddd" => loc.weekdays[wd].to_string(),
        "e" => ((wd as u32 + 7 - loc.week_start) % 7).to_string(),
        "E" => d.weekday().number_from_monday().to_string(),
        "w" => locale_week(d, loc.week_start).to_string(),
        "ww" => format!("{:02}", locale_week(d, loc.week_start)),
        "W" => iso.week().to_string(),
        "WW" => format!("{:02}", iso.week()),
        "H" => dt.hour().to_string(),
        "HH" => format!("{:02}", dt.hour()),
        "h" => hour12(dt.hour()).to_string(),
        "hh" => format!("{:02}", hour12(dt.hour())),
        "k" => (if dt.hour() == 0 { 24 } else { dt.hour() }).to_string(),
        "kk" => format!("{:02}", if dt.hour() == 0 { 24 } else { dt.hour() }),
        "m" => dt.minute().to_string(),
        "mm" => format!("{:02}", dt.minute()),
        "s" => dt.second().to_string(),
        "ss" => format!("{:02}", dt.second()),
        "S" => (dt.and_utc().timestamp_subsec_millis() / 100).to_string(),
        "SS" => format!("{:02}", dt.and_utc().timestamp_subsec_millis() / 10),
        "SSS" | "SSSS" => format!("{:03}", dt.and_utc().timestamp_subsec_millis()),
        "A" => loc.meridiem[(dt.hour() >= 12) as usize].to_string(),
        "a" => loc.meridiem[(dt.hour() >= 12) as usize].to_lowercase(),
        "X" => (to_millis(dt) / 1000).to_string(),
        "x" => to_millis(dt).to_string(),
        "Z" => zone(dt, true),
        "ZZ" => zone(dt, false),
        "LT" => format_moment(dt, if loc.pt { "HH:mm" } else { "h:mm A" }, loc),
        "LTS" => format_moment(dt, if loc.pt { "HH:mm:ss" } else { "h:mm:ss A" }, loc),
        "L" => format_moment(dt, if loc.pt { "DD/MM/YYYY" } else { "MM/DD/YYYY" }, loc),
        "l" => format_moment(dt, if loc.pt { "D/M/YYYY" } else { "M/D/YYYY" }, loc),
        "LL" => format_moment(dt, if loc.pt { "D [de] MMMM [de] YYYY" } else { "MMMM D, YYYY" }, loc),
        "ll" => format_moment(dt, if loc.pt { "D [de] MMM [de] YYYY" } else { "MMM D, YYYY" }, loc),
        "LLL" => format_moment(dt, if loc.pt { "D [de] MMMM [de] YYYY [às] HH:mm" } else { "MMMM D, YYYY h:mm A" }, loc),
        "lll" => format_moment(dt, if loc.pt { "D [de] MMM [de] YYYY [às] HH:mm" } else { "MMM D, YYYY h:mm A" }, loc),
        "LLLL" => format_moment(dt, if loc.pt { "dddd, D [de] MMMM [de] YYYY [às] HH:mm" } else { "dddd, MMMM D, YYYY h:mm A" }, loc),
        "llll" => format_moment(dt, if loc.pt { "ddd, D [de] MMM [de] YYYY [às] HH:mm" } else { "ddd, MMM D, YYYY h:mm A" }, loc),
        other => other.to_string(),
    }
}

/// Format with Luxon tokens.
pub fn format_luxon(dt: NaiveDateTime, fmt: &str, loc: &Locale) -> String {
    let mut s = String::new();
    for t in tokenize(fmt, true) {
        match t {
            Tok::Lit(l) => s.push_str(&l),
            Tok::Field(f) => s.push_str(&luxon_field(dt, f, loc)),
        }
    }
    s
}

fn luxon_field(dt: NaiveDateTime, f: &str, loc: &Locale) -> String {
    let d = dt.date();
    let wd = d.weekday().num_days_from_sunday() as usize;
    let iso = d.iso_week();
    match f {
        "yyyyyy" => format!("{:06}", d.year()),
        "yyyy" => format!("{:04}", d.year()),
        "yy" => format!("{:02}", d.year().rem_euclid(100)),
        "y" => d.year().to_string(),
        "kkkk" => format!("{:04}", iso.year()),
        "kk" => format!("{:02}", iso.year().rem_euclid(100)),
        "M" | "L" => d.month().to_string(),
        "MM" | "LL" => format!("{:02}", d.month()),
        "MMM" | "LLL" => loc.months_short[d.month0() as usize].to_string(),
        "MMMM" | "LLLL" => loc.months[d.month0() as usize].to_string(),
        "MMMMM" | "LLLLL" => loc.months[d.month0() as usize].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
        "q" => ((d.month0() / 3) + 1).to_string(),
        "qq" => format!("{:02}", (d.month0() / 3) + 1),
        "d" => d.day().to_string(),
        "dd" => format!("{:02}", d.day()),
        "o" => d.ordinal().to_string(),
        "ooo" => format!("{:03}", d.ordinal()),
        "E" | "c" => d.weekday().number_from_monday().to_string(),
        "EEE" | "ccc" => loc.weekdays_short[wd].to_string(),
        "EEEE" | "cccc" => loc.weekdays[wd].to_string(),
        "EEEEE" | "ccccc" => loc.weekdays[wd].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
        "W" => iso.week().to_string(),
        "WW" => format!("{:02}", iso.week()),
        "H" => dt.hour().to_string(),
        "HH" => format!("{:02}", dt.hour()),
        "h" => hour12(dt.hour()).to_string(),
        "hh" => format!("{:02}", hour12(dt.hour())),
        "m" => dt.minute().to_string(),
        "mm" => format!("{:02}", dt.minute()),
        "s" => dt.second().to_string(),
        "ss" => format!("{:02}", dt.second()),
        "S" => dt.and_utc().timestamp_subsec_millis().to_string(),
        "SSS" => format!("{:03}", dt.and_utc().timestamp_subsec_millis()),
        "u" => format!("{:03}", dt.and_utc().timestamp_subsec_millis()),
        "a" => loc.meridiem[(dt.hour() >= 12) as usize].to_string(),
        "X" => (to_millis(dt) / 1000).to_string(),
        "x" => to_millis(dt).to_string(),
        "Z" => {
            let m = offset_minutes(dt);
            if m % 60 == 0 {
                format!("{:+}", m / 60)
            } else {
                zone(dt, true)
            }
        }
        "ZZ" => zone(dt, true),
        "ZZZ" => zone(dt, false),
        "D" => format_luxon(dt, if loc.pt { "dd/MM/yyyy" } else { "M/d/yyyy" }, loc),
        "DD" => format_luxon(dt, if loc.pt { "d 'de' MMM 'de' yyyy" } else { "MMM d, yyyy" }, loc),
        "DDD" => format_luxon(dt, if loc.pt { "d 'de' MMMM 'de' yyyy" } else { "MMMM d, yyyy" }, loc),
        "DDDD" => format_luxon(dt, if loc.pt { "EEEE, d 'de' MMMM 'de' yyyy" } else { "EEEE, MMMM d, yyyy" }, loc),
        "t" => format_luxon(dt, if loc.pt { "HH:mm" } else { "h:mm a" }, loc),
        "tt" => format_luxon(dt, if loc.pt { "HH:mm:ss" } else { "h:mm:ss a" }, loc),
        "T" | "TTT" | "TTTT" | "ttt" | "tttt" => format_luxon(dt, "HH:mm", loc),
        "TT" => format_luxon(dt, "HH:mm:ss", loc),
        "f" => format!("{}, {}", luxon_field(dt, "D", loc), luxon_field(dt, "t", loc)),
        "ff" | "fff" | "ffff" => format!("{}, {}", luxon_field(dt, "DD", loc), luxon_field(dt, "t", loc)),
        "F" | "FF" | "FFF" | "FFFF" => format!("{}, {}", luxon_field(dt, "DD", loc), luxon_field(dt, "tt", loc)),
        other => other.to_string(),
    }
}

/// Parse `text` with a moment format. Returns None when it does not match
/// (strict: the whole text must be consumed).
pub fn parse_moment(text: &str, fmt: &str, loc: &Locale) -> Option<NaiveDateTime> {
    let (mut y, mut mo, mut d, mut h, mut mi, mut s) = (1970i32, 1u32, 1u32, 0u32, 0u32, 0u32);
    let mut pm: Option<bool> = None;
    let mut rest = text;
    fn num(r: &mut &str, min: usize, max: usize) -> Option<i64> {
        let n = r.bytes().take(max).take_while(|b| b.is_ascii_digit()).count();
        if n < min {
            return None;
        }
        let v = r[..n].parse().ok()?;
        *r = &r[n..];
        Some(v)
    }
    /// The longest name `r` starts with (ignoring case); consumes it.
    fn name(r: &mut &str, names: &[&str]) -> Option<usize> {
        let lower = r.to_lowercase();
        let i = (0..names.len()).filter(|&i| lower.starts_with(&names[i].to_lowercase())).max_by_key(|&i| names[i].len())?;
        let take: usize = r.chars().take(names[i].chars().count()).map(char::len_utf8).sum();
        *r = &r[take..];
        Some(i)
    }
    for t in tokenize(fmt, false) {
        match t {
            Tok::Lit(l) => rest = rest.strip_prefix(l.as_str())?,
            Tok::Field(f) => match f {
                "YYYY" | "Y" | "gggg" | "GGGG" => y = num(&mut rest, 4, 4)? as i32,
                "YY" | "gg" | "GG" => {
                    let v = num(&mut rest, 2, 2)? as i32;
                    y = if v > 68 { 1900 + v } else { 2000 + v };
                }
                "M" => mo = num(&mut rest, 1, 2)? as u32,
                "MM" => mo = num(&mut rest, 2, 2)? as u32,
                "MMM" => mo = name(&mut rest, &loc.months_short)? as u32 + 1,
                "MMMM" => mo = name(&mut rest, &loc.months).or_else(|| name(&mut rest, &loc.months_short))? as u32 + 1,
                "D" => d = num(&mut rest, 1, 2)? as u32,
                "DD" => d = num(&mut rest, 2, 2)? as u32,
                "Do" => {
                    d = num(&mut rest, 1, 2)? as u32;
                    let suffix: usize = rest.chars().take_while(|c| c.is_alphabetic() || *c == 'º').map(|c| c.len_utf8()).sum();
                    rest = &rest[suffix..];
                }
                "H" | "h" | "k" => h = num(&mut rest, 1, 2)? as u32,
                "HH" | "hh" | "kk" => h = num(&mut rest, 2, 2)? as u32,
                "m" => mi = num(&mut rest, 1, 2)? as u32,
                "mm" => mi = num(&mut rest, 2, 2)? as u32,
                "s" => s = num(&mut rest, 1, 2)? as u32,
                "ss" => s = num(&mut rest, 2, 2)? as u32,
                "SSS" | "SS" | "S" => {
                    num(&mut rest, 1, 3)?;
                }
                "A" | "a" => {
                    let l = rest.to_lowercase();
                    if l.starts_with("pm") {
                        pm = Some(true)
                    } else if l.starts_with("am") {
                        pm = Some(false)
                    } else {
                        return None;
                    }
                    rest = &rest[2..];
                }
                "dddd" => {
                    name(&mut rest, &loc.weekdays)?;
                }
                "ddd" => {
                    name(&mut rest, &loc.weekdays_short)?;
                }
                "X" => return Some(from_millis(num(&mut rest, 1, 12)? * 1000)),
                "x" => return Some(from_millis(num(&mut rest, 1, 15)?)),
                _ => return None,
            },
        }
    }
    if !rest.is_empty() {
        return None;
    }
    if let Some(p) = pm {
        h = h % 12 + if p { 12 } else { 0 };
    }
    NaiveDate::from_ymd_opt(y, mo, d)?.and_hms_opt(h, mi, s)
}

/// ISO 8601 date or date-time (`2024-01-02`, `2024-01-02T10:00`, with
/// optional seconds, fraction and zone, which is converted to local time).
/// Also accepts `2024-01` and a bare year-month-day with spaces before the time.
pub fn parse_iso(text: &str) -> Option<NaiveDateTime> {
    let t = text.trim();
    let b = t.as_bytes();
    if b.len() < 7 || !b[..4].iter().all(u8::is_ascii_digit) || b[4] != b'-' {
        return None;
    }
    let y: i32 = t[..4].parse().ok()?;
    let mo: u32 = t.get(5..7)?.parse().ok()?;
    if b.len() == 7 {
        return NaiveDate::from_ymd_opt(y, mo, 1).map(|d| d.and_time(NaiveTime::MIN));
    }
    if b.get(7) != Some(&b'-') || b.len() < 10 {
        return None;
    }
    let d: u32 = t.get(8..10)?.parse().ok()?;
    let date = NaiveDate::from_ymd_opt(y, mo, d)?;
    if b.len() == 10 {
        return Some(date.and_time(NaiveTime::MIN));
    }
    if !matches!(b[10], b'T' | b' ') {
        return None;
    }
    let rest = &t[11..];
    let (time_part, zone_part) = match rest.find(['Z', '+', '-']) {
        Some(i) => (&rest[..i], Some(&rest[i..])),
        None => (rest, None),
    };
    let mut it = time_part.split(':');
    let h: u32 = it.next()?.parse().ok()?;
    let mi: u32 = it.next().unwrap_or("0").parse().ok()?;
    let sec = it.next().unwrap_or("0");
    let (s, frac) = sec.split_once('.').unwrap_or((sec, "0"));
    let s: u32 = s.parse().ok()?;
    let ms: u32 = format!("{:0<3}", &frac[..frac.len().min(3)]).parse().ok()?;
    let naive = date.and_hms_milli_opt(h, mi, s, ms)?;
    match zone_part {
        None => Some(naive),
        Some(z) => {
            let off_min = if z == "Z" {
                0
            } else {
                let sign = if z.starts_with('-') { -1 } else { 1 };
                let digits: String = z[1..].chars().filter(|c| c.is_ascii_digit()).collect();
                let hh: i64 = digits.get(..2)?.parse().ok()?;
                let mm: i64 = digits.get(2..4).unwrap_or("0").parse().ok()?;
                sign * (hh * 60 + mm)
            };
            let utc_ms = naive.and_utc().timestamp_millis() - off_min * 60_000;
            Some(from_millis(utc_ms))
        }
    }
}

pub fn add_months(dt: NaiveDateTime, months: i64) -> NaiveDateTime {
    let total = dt.year() as i64 * 12 + dt.month0() as i64 + months;
    let y = total.div_euclid(12) as i32;
    let m = total.rem_euclid(12) as u32 + 1;
    let last = days_in_month(y, m);
    let d = dt.day().min(last);
    NaiveDate::from_ymd_opt(y, m, d).unwrap_or(dt.date()).and_time(dt.time())
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (NaiveDate::from_ymd_opt(ny, nm, 1).unwrap() - NaiveDate::from_ymd_opt(y, m, 1).unwrap()).num_days() as u32
}

/// Start of a calendar unit (`year`, `quarter`, `month`, `week`, `isoWeek`, `day`, `hour`, `minute`, `second`).
pub fn start_of(dt: NaiveDateTime, unit: &str, week_start: u32) -> NaiveDateTime {
    let d = dt.date();
    let midnight = |d: NaiveDate| d.and_time(NaiveTime::MIN);
    match unit.trim_end_matches('s') {
        "year" | "y" => midnight(NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap()),
        "quarter" | "Q" => midnight(NaiveDate::from_ymd_opt(d.year(), d.month0() / 3 * 3 + 1, 1).unwrap()),
        "month" | "M" => midnight(NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap()),
        "week" | "w" => {
            let back = (d.weekday().num_days_from_sunday() + 7 - week_start) % 7;
            midnight(d - Duration::days(back as i64))
        }
        "isoWeek" | "W" => midnight(d - Duration::days(d.weekday().num_days_from_monday() as i64)),
        "day" | "d" | "date" | "D" => midnight(d),
        "hour" | "h" => d.and_hms_opt(dt.hour(), 0, 0).unwrap(),
        "minute" | "m" => d.and_hms_opt(dt.hour(), dt.minute(), 0).unwrap(),
        "second" => d.and_hms_opt(dt.hour(), dt.minute(), dt.second()).unwrap(),
        _ => dt,
    }
}

/// Last millisecond of a calendar unit.
pub fn end_of(dt: NaiveDateTime, unit: &str, week_start: u32) -> NaiveDateTime {
    let s = start_of(dt, unit, week_start);
    let next = match unit.trim_end_matches('s') {
        "year" | "y" => add_months(s, 12),
        "quarter" | "Q" => add_months(s, 3),
        "month" | "M" => add_months(s, 1),
        "week" | "w" | "isoWeek" | "W" => s + Duration::days(7),
        "day" | "d" | "date" | "D" => s + Duration::days(1),
        "hour" | "h" => s + Duration::hours(1),
        "minute" | "m" => s + Duration::minutes(1),
        "second" => s + Duration::seconds(1),
        _ => return dt,
    };
    next - Duration::milliseconds(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        parse_iso(s).unwrap()
    }

    #[test]
    fn moment_tokens() {
        let d = dt("2026-10-21T09:05:07");
        assert_eq!(format_moment(d, "YYYY/MM/DD", &EN), "2026/10/21");
        assert_eq!(format_moment(d, "dddd, D [de] MMMM", &PT), "quarta-feira, 21 de outubro");
        assert_eq!(format_moment(d, "Do MMM YY h:mm A", &EN), "21st Oct 26 9:05 AM");
        assert_eq!(format_moment(d, "GGGG-[W]WW", &EN), "2026-W43");
        assert_eq!(format_moment(d, "LL", &PT), "21 de outubro de 2026");
    }

    #[test]
    fn luxon_tokens() {
        let d = dt("2026-10-21T15:30:00");
        assert_eq!(format_luxon(d, "MMMM dd, yyyy", &EN), "October 21, 2026");
        assert_eq!(format_luxon(d, "h:mm a - MMMM dd, yyyy", &EN), "3:30 PM - October 21, 2026");
        assert_eq!(format_luxon(d, "yyyy-MM-dd 'at' HH:mm", &EN), "2026-10-21 at 15:30");
        assert_eq!(format_luxon(d, "EEEE", &PT), "quarta-feira");
    }

    #[test]
    fn parsing() {
        assert_eq!(parse_moment("2026/10/21", "YYYY/MM/DD", &EN), Some(dt("2026-10-21")));
        assert_eq!(parse_moment("21 de outubro de 2026", "D [de] MMMM [de] YYYY", &PT), Some(dt("2026-10-21")));
        assert_eq!(parse_moment("2026-10-21x", "YYYY-MM-DD", &EN), None);
        assert_eq!(parse_iso("2026-10-21T10:00"), Some(dt("2026-10-21T10:00:00")));
        assert!(parse_iso("2026-13-01").is_none());
        assert!(parse_iso("hello").is_none());
    }

    #[test]
    fn units() {
        let d = dt("2026-10-21T15:30:00");
        assert_eq!(start_of(d, "month", 0), dt("2026-10-01"));
        assert_eq!(start_of(d, "week", 0), dt("2026-10-18"));
        assert_eq!(start_of(d, "isoWeek", 0), dt("2026-10-19"));
        assert_eq!(end_of(d, "month", 0), dt("2026-10-31T23:59:59.999"));
        assert_eq!(add_months(dt("2026-01-31"), 1), dt("2026-02-28"));
    }
}
