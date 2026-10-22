//! Daily notes: the note of a date, created from the template of the daily
//! notes settings (`daily-notes.json`, shared with Obsidian).

use crate::config::VaultConfig;
use crate::vault::Vault;
use ateliet_core::{calendar, datefmt};
use chrono::{NaiveDate, NaiveTime};

#[derive(Clone, Debug, Default)]
pub struct DailySettings {
    pub format: String,
    pub folder: String,
    pub template: String,
}

impl DailySettings {
    pub fn load(cfg: &VaultConfig) -> Self {
        let v = cfg.read("daily-notes.json");
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        DailySettings { format: s("format"), folder: s("folder"), template: s("template") }
    }

    pub fn save(&self, cfg: &VaultConfig) {
        cfg.merge("daily-notes.json", serde_json::json!({ "format": self.format, "folder": self.folder, "template": self.template }));
    }

    pub fn path(&self, date: NaiveDate, locale: &str) -> String {
        calendar::daily_note_path(date, &self.format, &self.folder, datefmt::locale(locale))
    }
}

/// `{{date}}`, `{{date:FORMAT}}`, `{{time}}` and `{{title}}` (Obsidian's core templates).
pub fn apply_template_vars(text: &str, title: &str, date_fmt: &str, locale: &str, when: chrono::NaiveDateTime) -> String {
    let loc = datefmt::locale(locale);
    let now = datefmt::now();
    let re = regex::Regex::new(r"\{\{\s*(date|time|title)\s*(?::([^}]*))?\}\}").unwrap();
    re.replace_all(text, |c: &regex::Captures| {
        let kind = c[1].to_lowercase();
        if kind == "title" {
            return title.to_string();
        }
        let fmt = c.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| if kind == "date" { date_fmt.to_string() } else { "HH:mm".into() });
        datefmt::format_moment(if kind == "date" { when } else { now }, &fmt, loc)
    })
    .into_owned()
}

/// The daily note for a date, created if missing. Returns its path.
pub fn ensure(vault: &Vault, s: &DailySettings, date: NaiveDate, locale: &str) -> std::io::Result<String> {
    let path = s.path(date, locale);
    if vault.abs(&path).exists() {
        return Ok(path);
    }
    let mut content = String::new();
    if !s.template.trim().is_empty() {
        if let Some(t) = vault.resolve(s.template.trim(), "") {
            if let Ok(text) = std::fs::read_to_string(vault.abs(&t)) {
                let fmt = if s.format.is_empty() { "YYYY-MM-DD" } else { &s.format };
                let title = datefmt::format_moment(date.and_time(NaiveTime::MIN), fmt, datefmt::locale(locale));
                content = apply_template_vars(&text, title.rsplit('/').next().unwrap_or(&title), fmt, locale, date.and_time(NaiveTime::MIN));
            }
        }
    }
    vault.write(&path, &content)?;
    Ok(path)
}
