//! The inbox: a folder (`<config>/ateliet/inbox/`) where anything dropped
//! becomes an excerpt. A `.json` in the shape of [`Snippet`] (or a list of
//! them), a `.txt` or `.md`, or a picture (the photo of a handwritten page
//! taken on the phone, arriving through the vault's sync). Each file is
//! imported and then removed.

use super::{free_name, save, Snippet, SourceRef};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What arrived: the excerpt and the note written for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrival {
    pub note: String,
    pub snippet: Snippet,
}

pub fn dir(config_dir: &Path) -> PathBuf {
    config_dir.join("ateliet").join("inbox")
}

const PICTURES: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "heic"];

/// Imports what is in the inbox into `folder` of the vault (`root`).
/// Files changed in the last two seconds are left for the next time: they
/// may still be arriving.
pub fn import(root: &Path, inbox: &Path, folder: &str) -> Vec<Arrival> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(inbox) else { return out };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort();
    for f in files {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        let fresh = std::fs::metadata(&f).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(2));
        if fresh {
            continue;
        }
        let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let snippets: Vec<Snippet> = match ext.as_str() {
            "json" => {
                let Ok(t) = std::fs::read_to_string(&f) else { continue };
                match serde_json::from_str::<serde_json::Value>(&t) {
                    Ok(serde_json::Value::Array(a)) => a.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect(),
                    Ok(v) => serde_json::from_value(v).ok().into_iter().collect(),
                    Err(_) => continue,
                }
            }
            "txt" | "md" => {
                let Ok(t) = std::fs::read_to_string(&f) else { continue };
                vec![Snippet::new(&t, SourceRef { title: stem.clone(), ..Default::default() }, "manual")]
            }
            e if PICTURES.contains(&e) => {
                let pics = if folder.is_empty() { "Imagens".to_string() } else { format!("{folder}/Imagens") };
                let dest_dir = root.join(&pics);
                if std::fs::create_dir_all(&dest_dir).is_err() {
                    continue;
                }
                let pic = free_name(&dest_dir, &name);
                if std::fs::copy(&f, dest_dir.join(&pic)).is_err() {
                    continue;
                }
                let mut s = Snippet::new("", SourceRef { title: stem.clone(), ..Default::default() }, "digitalizacao");
                s.image = Some(format!("{pics}/{pic}"));
                vec![s]
            }
            _ => continue,
        };
        let mut ok = true;
        for mut s in snippets {
            if s.id.is_empty() {
                s.id = crate::journal::short_id();
            }
            if s.captured.is_empty() {
                s.captured = super::now();
            }
            match save(root, folder, &s) {
                Ok(note) => out.push(Arrival { note, snippet: s }),
                Err(_) => ok = false,
            }
        }
        if ok {
            let _ = std::fs::remove_file(&f);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_and_empties() {
        let root = std::env::temp_dir().join(format!("ateliet-inbox-{}", crate::journal::short_id()));
        let inbox = dir(&root.join(".obsidian"));
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("a.json"), r#"{"text":"Um trecho.","source":{"title":"Livro","author":"Autora"},"origin":"leitor"}"#).unwrap();
        std::fs::write(inbox.join("nota.txt"), "Texto solto").unwrap();
        std::fs::write(inbox.join("foto.jpg"), [0xff, 0xd8, 0xff]).unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(10);
        for n in ["a.json", "nota.txt", "foto.jpg"] {
            std::fs::File::options().write(true).open(inbox.join(n)).unwrap().set_modified(old).unwrap();
        }
        let got = import(&root, &inbox, "Trechos");
        assert_eq!(got.len(), 3);
        assert!(std::fs::read_dir(&inbox).unwrap().next().is_none());
        let a = got.iter().find(|a| a.snippet.origin == "leitor").unwrap();
        assert!(a.note.starts_with("Trechos/Livro – Um trecho"));
        let text = std::fs::read_to_string(root.join(&a.note)).unwrap();
        assert_eq!(Snippet::from_note(&text).unwrap().source.author.as_deref(), Some("Autora"));
        let pic = got.iter().find(|a| a.snippet.image.is_some()).unwrap();
        assert!(root.join(pic.snippet.image.as_ref().unwrap()).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
