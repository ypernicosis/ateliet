//! Daily writing statistics and the plugin's settings file
//! (`.obsidian/plugins/scriptorium-omena/data.json`), shared with Obsidian.

use chrono::{Duration, Local, NaiveDate};
use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

pub struct ScSettings {
    path: PathBuf,
    pub data: Json,
    dirty: bool,
}

fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn today() -> String {
    day_key(Local::now().date_naive())
}

impl ScSettings {
    pub fn load(config_dir: &Path) -> ScSettings {
        let path = config_dir.join("plugins/scriptorium-omena/data.json");
        let data = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Json>(&t).ok()).filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
        ScSettings { path, data, dirty: false }
    }

    pub fn str(&self, k: &str, default: &str) -> String {
        self.data.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(default).to_string()
    }

    pub fn set(&mut self, k: &str, v: Json) {
        if self.data.get(k) != Some(&v) {
            self.data[k] = v;
            self.dirty = true;
        }
    }

    pub fn books_folder(&self) -> String {
        self.str("pastaLivros", "Livros")
    }

    pub fn default_author(&self) -> String {
        self.str("autorPadrao", "")
    }

    pub fn daily_goal(&self) -> i64 {
        self.data.get("metaDiaria").and_then(|v| v.as_i64()).unwrap_or(1000)
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        if let Some(d) = self.path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if std::fs::write(&self.path, serde_json::to_string_pretty(&self.data).unwrap_or_default()).is_ok() {
            self.dirty = false;
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn days(&self) -> Option<&serde_json::Map<String, Json>> {
        self.data.get("stats")?.get("days")?.as_object()
    }

    /// Note the book's current total; the day's words are `end - start`.
    pub fn record(&mut self, book: &str, total: i64) {
        let day = today();
        if self.data.get("stats").and_then(|s| s.get("days")).and_then(|d| d.as_object()).is_none() {
            self.data["stats"] = json!({ "days": {} });
        }
        let start = {
            let days = self.days().unwrap();
            if let Some(r) = days.get(&day).and_then(|d| d.get(book)) {
                if r["end"].as_i64() == Some(total) {
                    return;
                }
                None
            } else {
                let mut prev: Vec<&String> = days.keys().filter(|k| k.as_str() < day.as_str()).collect();
                prev.sort();
                Some(prev.iter().rev().find_map(|k| days[*k].get(book).and_then(|r| r["end"].as_i64())).unwrap_or(total))
            }
        };
        let days = self.data["stats"]["days"].as_object_mut().unwrap();
        let d = days.entry(day).or_insert_with(|| json!({}));
        match start {
            Some(s) => d[book] = json!({ "start": s, "end": total }),
            None => d[book]["end"] = json!(total),
        }
        self.dirty = true;
    }

    fn words_on(&self, day: &str, book: Option<&str>) -> i64 {
        let Some(d) = self.days().and_then(|d| d.get(day)).and_then(|d| d.as_object()) else { return 0 };
        let net = |r: &Json| r["end"].as_i64().unwrap_or(0) - r["start"].as_i64().unwrap_or(0);
        match book {
            Some(b) => d.get(b).map(net).unwrap_or(0),
            None => d.values().map(net).sum(),
        }
    }

    pub fn today_words(&self, book: Option<&str>) -> i64 {
        self.words_on(&today(), book)
    }

    /// Days in a row with words written (today may still be empty).
    pub fn streak(&self) -> usize {
        let mut d = Local::now().date_naive();
        let mut n = 0;
        for i in 0..3650 {
            if self.words_on(&day_key(d), None) > 0 {
                n += 1;
            } else if i > 0 {
                break;
            }
            d -= Duration::days(1);
        }
        n
    }

    pub fn last_days(&self, n: i64) -> Vec<(String, i64)> {
        let start = Local::now().date_naive() - Duration::days(n - 1);
        (0..n)
            .map(|i| {
                let k = day_key(start + Duration::days(i));
                let Some(d) = self.days().and_then(|d| d.get(&k)).and_then(|d| d.as_object()) else { return (k, 0) };
                let w = d.values().map(|r| (r["end"].as_i64().unwrap_or(0) - r["start"].as_i64().unwrap_or(0)).max(0)).sum();
                (k, w)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_words() {
        let dir = std::env::temp_dir().join(format!("sc-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = ScSettings::load(&dir);
        s.record("L.md", 100);
        s.record("L.md", 250);
        assert_eq!(s.today_words(Some("L.md")), 150);
        assert_eq!(s.today_words(None), 150);
        assert_eq!(s.streak(), 1);
        s.save();
        let s2 = ScSettings::load(&dir);
        assert_eq!(s2.today_words(None), 150);
        assert_eq!(s2.last_days(3).len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
