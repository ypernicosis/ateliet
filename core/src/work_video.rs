//! A work-session video is independent of documents and vaults.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

pub const RECENT_LIMIT: usize = 8;
pub const EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "webm", "mkv", "avi", "ogv"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    Local { path: PathBuf },
    Youtube { id: String },
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Self::Local { path } => path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            Self::Youtube { .. } => "YouTube".into(),
        }
    }

    pub fn external(&self) -> String {
        match self {
            Self::Local { path } => path.to_string_lossy().into_owned(),
            Self::Youtube { id } => format!("https://www.youtube.com/watch?v={id}"),
        }
    }

    pub fn valid(&self) -> bool {
        match self {
            Self::Local { path } => path.is_absolute(),
            Self::Youtube { id } => valid_id(id),
        }
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub fn is_video(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Only a recognised YouTube URL can cross into the embedded player.
pub fn youtube(input: &str) -> Result<(Source, f64), String> {
    let input = input.trim();
    let normal = if input.starts_with("youtube.com/") || input.starts_with("www.youtube.com/") || input.starts_with("youtu.be/") { format!("https://{input}") } else { input.to_owned() };
    let url = Url::parse(&normal).map_err(|_| "Cole um link de vídeo do YouTube.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err("Use um link HTTP ou HTTPS do YouTube.".into());
    }
    let segments: Vec<_> = url.path_segments().into_iter().flatten().filter(|p| !p.is_empty()).collect();
    let id = match (url.host_str().unwrap_or(""), segments.as_slice()) {
        ("youtu.be", [id]) => Some((*id).to_string()),
        ("youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com", ["watch"]) => url.query_pairs().find(|(k, _)| k == "v").map(|(_, v)| v.into_owned()),
        ("youtube.com" | "www.youtube.com" | "m.youtube.com" | "www.youtube-nocookie.com" | "youtube-nocookie.com", ["embed" | "shorts" | "live", id]) => Some((*id).to_string()),
        _ => None,
    }.filter(|id| valid_id(id)).ok_or_else(|| "Esse link não identifica um vídeo do YouTube. Use o link de um vídeo, não de uma playlist ou canal.".to_string())?;
    let at = url.query_pairs().find(|(k, _)| k == "t" || k == "start").map(|(_, v)| v.into_owned()).or_else(|| url.fragment().and_then(|f| f.strip_prefix("t=")).map(str::to_owned));
    Ok((Source::Youtube { id }, at.as_deref().and_then(timestamp).unwrap_or(0.0)))
}

fn timestamp(s: &str) -> Option<f64> {
    if let Ok(n) = s.parse::<u32>() { return Some(n as f64); }
    let re = regex::Regex::new(r"^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?$").ok()?;
    let c = re.captures(s)?;
    if s.is_empty() { return None; }
    let mut n = 0u32;
    for (i, factor) in [(1, 3600u32), (2, 60), (3, 1)] {
        if let Some(v) = c.get(i) { n = n.checked_add(v.as_str().parse::<u32>().ok()?.checked_mul(factor)?)?; }
    }
    Some(n as f64)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recent {
    pub source: Source,
    pub title: String,
    pub position: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub recent: Vec<Recent>,
    pub pinned: bool,
}

impl Default for History {
    fn default() -> Self { Self { recent: Vec::new(), pinned: true } }
}

impl History {
    pub fn remember(&mut self, source: Source, title: String, position: f64) {
        if !source.valid() { return; }
        self.recent.retain(|r| r.source != source);
        self.recent.insert(0, Recent { source, title: title.chars().take(200).collect(), position: if position.is_finite() { position.max(0.0) } else { 0.0 } });
        self.recent.truncate(RECENT_LIMIT);
    }

    pub fn sanitize(&mut self) {
        let recent = std::mem::take(&mut self.recent);
        for r in recent.into_iter().rev() { self.remember(r.source, r.title, r.position); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "dQw4w9WgXcQ";

    #[test]
    fn shared_links_and_times() {
        for (url, at) in [
            (format!("https://youtu.be/{ID}?t=1h2m3s&si=share"), 3723.0),
            (format!("https://www.youtube.com/watch?v={ID}&t=92"), 92.0),
            (format!("https://m.youtube.com/shorts/{ID}"), 0.0),
            (format!("https://www.youtube.com/live/{ID}?start=50"), 50.0),
            (format!("https://www.youtube-nocookie.com/embed/{ID}#t=2m"), 120.0),
            (format!("youtu.be/{ID}"), 0.0),
        ] { assert_eq!(youtube(&url), Ok((Source::Youtube { id: ID.into() }, at)), "{url}"); }
    }

    #[test]
    fn rejects_untrusted_or_ambiguous_input() {
        for url in ["javascript:alert(1)", "file:///etc/passwd", "https://youtube.com.evil.org/watch?v=dQw4w9WgXcQ", "https://youtube.com@evil.org/watch?v=dQw4w9WgXcQ", "https://evil.org@youtube.com/watch?v=dQw4w9WgXcQ", "https://youtube.com/playlist?list=foo", "https://youtu.be/short", "https://youtu.be/dQw4w9WgXcQ/extra", "https://youtube.com/watch?v=%3Cscript%3E", "https://youtube.com:444/watch?v=dQw4w9WgXcQ"] {
            assert!(youtube(url).is_err(), "{url}");
        }
        assert_eq!(timestamp("999999999999999999h"), None);
        assert_eq!(timestamp("1mgarbage"), None);
    }

    #[test]
    fn history_is_bounded_deduplicated_and_sanitized() {
        let mut h = History::default();
        for i in 0..12 { h.remember(Source::Local { path: PathBuf::from(format!("/movie-{i}.mp4")) }, format!("Movie {i}"), 20.0); }
        assert_eq!(h.recent.len(), RECENT_LIMIT);
        let source = h.recent[3].source.clone();
        h.remember(source.clone(), "Resumed".into(), f64::NAN);
        assert_eq!(h.recent[0].source, source);
        assert_eq!(h.recent[0].position, 0.0);
        assert_eq!(h.recent.len(), RECENT_LIMIT);
        let json = serde_json::to_string(&h).unwrap();
        let mut loaded: History = serde_json::from_str(&json).unwrap();
        loaded.sanitize();
        assert_eq!(loaded.recent[0].title, "Resumed");
    }
}
