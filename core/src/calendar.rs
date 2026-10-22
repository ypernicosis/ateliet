//! The calendar: a month of daily notes, with dots for how much was written
//! and for open tasks. Daily note paths follow the daily notes settings
//! (folder + moment format), so the calendar opens the same files.

use crate::dataview::index::Index;
use crate::datefmt::{self, Locale};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use serde::Deserialize;
use serde_json::{json, Value as Json};
use std::path::Path;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MonthRequest {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// Daily note format (moment tokens), e.g. `YYYY-MM-DD` or `YYYY/MM/DD`.
    pub format: String,
    pub folder: String,
    /// 0 = Sunday … 6 = Saturday; `None` uses the locale's first day.
    pub week_start: Option<u32>,
    pub words_per_dot: u32,
    pub locale: String,
}

/// Vault-relative path of the daily note for a date.
pub fn daily_note_path(date: NaiveDate, format: &str, folder: &str, loc: &Locale) -> String {
    let fmt = if format.trim().is_empty() { "YYYY-MM-DD" } else { format };
    let name = datefmt::format_moment(date.and_time(NaiveTime::MIN), fmt, loc);
    let folder = folder.trim().trim_matches('/');
    if folder.is_empty() {
        format!("{name}.md")
    } else {
        format!("{folder}/{name}.md")
    }
}

/// Words as Obsidian counts them: runs of letters/digits (CJK characters one by one), frontmatter excluded.
pub fn count_words(text: &str) -> usize {
    let body = match crate::syntax::frontmatter(text) {
        Some(f) => &text[f.block.end.min(text.len())..],
        None => text,
    };
    let mut n = 0;
    let mut in_word = false;
    for c in body.chars() {
        let cjk = matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7af);
        if cjk {
            n += 1;
            in_word = false;
        } else if c.is_alphanumeric() || c == '\'' || c == '’' || (in_word && (c == '-' || c == '_')) {
            if !in_word {
                n += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    n
}

pub fn month(root: &Path, index: &Index, req: &MonthRequest) -> Json {
    let loc = datefmt::locale(&req.locale);
    let week_start = req.week_start.unwrap_or(loc.week_start) % 7;
    let first = NaiveDate::from_ymd_opt(req.year, req.month.clamp(1, 12), 1).unwrap_or_else(|| datefmt::today().date());
    let back = (first.weekday().num_days_from_sunday() + 7 - week_start) % 7;
    let start = first - Duration::days(back as i64);
    let today = datefmt::today().date();
    let per_dot = req.words_per_dot.max(1) as usize;
    let mut weeks = Vec::new();
    let mut d = start;
    for _ in 0..6 {
        let mut days = Vec::new();
        // the ISO week of the row's Thursday
        let thursday = d + Duration::days(((4 + 7 - d.weekday().num_days_from_sunday()) % 7) as i64);
        let week_number = thursday.iso_week().week();
        for _ in 0..7 {
            let path = daily_note_path(d, &req.format, &req.folder, loc);
            let exists = index.exists(&path);
            let (dots, open_tasks) = if exists {
                let words = std::fs::read_to_string(root.join(&path)).map(|t| count_words(&t)).unwrap_or(0);
                let open = index.page(&path).map(|p| p.lists.iter().filter(|i| i.status == Some(' ')).count()).unwrap_or(0);
                (words.div_ceil(per_dot).clamp(if words > 0 { 1 } else { 0 }, 5), open)
            } else {
                (0, 0)
            };
            days.push(json!({
                "date": d.format("%Y-%m-%d").to_string(),
                "day": d.day(),
                "inMonth": d.month() == first.month(),
                "today": d == today,
                "weekend": matches!(d.weekday().num_days_from_sunday(), 0 | 6),
                "path": path,
                "exists": exists,
                "dots": dots,
                "openTasks": open_tasks,
            }));
            d += Duration::days(1);
        }
        weeks.push(json!({ "week": week_number, "days": days }));
        if d.month() != first.month() && d > first {
            break;
        }
    }
    let weekdays: Vec<&str> = (0..7).map(|i| loc.weekdays_short[((week_start + i) % 7) as usize]).collect();
    json!({
        "month": loc.months[first.month0() as usize],
        "year": first.year(),
        "weekdays": weekdays,
        "weeks": weeks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_words() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        assert_eq!(daily_note_path(d, "YYYY/MM/DD", "sistema de projetos/calendario/dias/", datefmt::locale("pt")), "sistema de projetos/calendario/dias/2026/10/21.md");
        assert_eq!(daily_note_path(d, "", "", datefmt::locale("en")), "2026-10-21.md");
        assert_eq!(count_words("---\ntags: [a]\n---\nOlá, mundo! d'água e bem-vindo 123"), 6);
    }

    #[test]
    fn month_grid() {
        let dir = std::env::temp_dir().join(format!("ateliet-cal-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dias/2026/10")).unwrap();
        std::fs::write(dir.join("dias/2026/10/21.md"), "um dois três\n- [ ] tarefa\n").unwrap();
        let ix = Index::build(&dir);
        let req = MonthRequest { year: 2026, month: 10, format: "YYYY/MM/DD".into(), folder: "dias".into(), week_start: None, words_per_dot: 2, locale: "pt-BR".into() };
        let m = month(&dir, &ix, &req);
        assert_eq!(m["month"], "outubro");
        assert_eq!(m["weekdays"][0], "dom");
        let days: Vec<&Json> = m["weeks"].as_array().unwrap().iter().flat_map(|w| w["days"].as_array().unwrap()).collect();
        assert_eq!(days[0]["date"], "2026-09-27");
        let d21 = days.iter().find(|d| d["date"] == "2026-10-21").unwrap();
        assert_eq!(d21["exists"], true);
        assert_eq!(d21["dots"], 2);
        assert_eq!(d21["openTasks"], 1);
        assert_eq!(days.len() % 7, 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
