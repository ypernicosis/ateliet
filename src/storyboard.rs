//! The storyboard's needs from the core: Clip Studio files turned into layers,
//! and the drawings Pictogrep keeps in its `storyboards/` folder (read, so
//! they can be opened and edited here, and written, so frames exported from
//! Ateliet show up in Pictogrep). The storyboard itself is a zip the
//! front-end reads and writes through the ordinary file commands.

use crate::gallery::Galleries;
use crate::shared::shared_of;
use ateliet_core::clipstudio;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::ImageEncoder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::ipc::Response;
use tauri::State;

type Res<T> = Result<T, String>;

fn png(img: &image::RgbaImage) -> Res<Vec<u8>> {
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// A `.clip` file as `ACLP` + u32 LE length + JSON + the PNGs the JSON
/// points into (`{offset, length}` after the JSON).
pub fn encode_clip(doc: &clipstudio::Document) -> Res<Vec<u8>> {
    let mut blobs: Vec<u8> = Vec::new();
    let mut put = |bytes: &[u8]| {
        let at = blobs.len();
        blobs.extend_from_slice(bytes);
        json!({ "offset": at, "length": bytes.len() })
    };
    let preview = doc.preview.as_deref().map(&mut put);
    let mut layers = Vec::new();
    for l in &doc.layers {
        let mut v = json!({
            "name": l.name, "folders": l.folders, "visible": l.visible, "opacity": l.opacity,
            "blend": l.blend, "blendApprox": l.blend_approx, "clip": l.clip, "kind": l.kind,
            "x": l.x, "y": l.y, "width": l.image.width(), "height": l.image.height(),
        });
        if l.kind == "paper" {
            let p = l.image.get_pixel(0, 0);
            v["color"] = json!(format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2]));
        } else {
            v["png"] = put(&png(&l.image)?);
        }
        layers.push(v);
    }
    let head = serde_json::to_vec(&json!({
        "width": doc.width, "height": doc.height, "dpi": doc.dpi,
        "preview": preview, "layers": layers, "skipped": doc.skipped,
    }))
    .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(8 + head.len() + blobs.len());
    out.extend_from_slice(b"ACLP");
    out.extend_from_slice(&(head.len() as u32).to_le_bytes());
    out.extend_from_slice(&head);
    out.extend_from_slice(&blobs);
    Ok(out)
}

#[tauri::command]
pub async fn clip_import(path: String) -> Res<Response> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = fs::read(&path).map_err(|e| format!("{e}: {path}"))?;
        let doc = clipstudio::read(&data)?;
        encode_clip(&doc).map(Response::new)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Pictogrep's storyboards/ folder ───────────────────────────────────────

fn boards_dir(window: &tauri::Window, state: &State<Galleries>) -> Res<PathBuf> {
    let g = state.get(window.label())?;
    Ok(shared_of(&g)?.root.join("storyboards"))
}

/// Pictogrep's file-name stem: lowercase ASCII letters and digits, runs of
/// anything else as one dash, at most 60 characters.
pub fn clean_stem(value: &str) -> String {
    let stem = Path::new(value).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut out = String::new();
    let mut dash = false;
    for c in stem.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_matches('-').chars().take(60).collect::<String>();
    if out.is_empty() {
        "image".into()
    } else {
        out
    }
}

fn is_image(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("png" | "jpg" | "jpeg" | "webp" | "gif"))
}

/// The drawings in Pictogrep's storyboards folder, with what their JSON says.
#[tauri::command]
pub fn pictogrep_boards(window: tauri::Window, state: State<Galleries>) -> Res<Value> {
    let dir = boards_dir(&window, &state)?;
    let mut boards = Vec::new();
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if !p.is_file() || !is_image(&p) {
            continue;
        }
        let meta: Value = fs::read(p.with_extension("json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        let mtime = entry.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
        boards.push(json!({
            "name": entry.file_name().to_string_lossy(), "abs": p, "mtime": mtime,
            "source": meta.get("source").cloned().unwrap_or(Value::Null),
            "aspect": meta.get("aspect").cloned().unwrap_or(Value::Null),
            "query": meta.get("query").cloned().unwrap_or(Value::Null),
        }));
    }
    boards.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let refs: Vec<PathBuf> = fs::read_dir(dir.join("references")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && is_image(p)).collect();
    Ok(json!({ "dir": dir, "boards": boards, "references": refs }))
}

fn write_atomically(target: &Path, data: &[u8]) -> Res<()> {
    let tmp = target.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, data).map_err(|e| e.to_string())?;
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Writes one frame into Pictogrep's storyboards folder, named the way
/// Pictogrep names its own (`0001_16x9_<stem>_<id>.png` and a JSON beside it).
/// The id comes from the storyboard and the frame, so exporting again
/// replaces the earlier copy.
#[tauri::command]
pub fn pictogrep_board_save(window: tauri::Window, state: State<Galleries>, request: tauri::ipc::Request<'_>) -> Res<Value> {
    let dir = boards_dir(&window, &state)?;
    let header = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()).unwrap_or_default();
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else { return Err("expected raw body".into()) };
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("expected a PNG".into());
    }
    let index: u32 = header("x-index").parse().unwrap_or(1).clamp(1, 9999);
    let aspect = header("x-aspect");
    let aspect_file = if aspect.chars().all(|c| c.is_ascii_digit() || c == ':') && aspect.contains(':') { aspect.replace(':', "x") } else { "free".into() };
    let key = header("x-key");
    let id: String = Sha256::digest(key.as_bytes())[..8].iter().map(|b| format!("{b:02x}")).collect();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // An earlier export of the same frame, possibly at another position.
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(&format!("_{id}.png")) {
            let _ = fs::remove_file(entry.path());
            let _ = fs::remove_file(entry.path().with_extension("json"));
        }
    }
    let file = format!("{index:04}_{aspect_file}_{}_{id}.png", clean_stem(&header("x-name")));
    let target = dir.join(&file);
    write_atomically(&target, data)?;
    let meta = json!({ "file": file, "source": header("x-source"), "aspect": aspect, "tags": [], "query": header("x-query") });
    write_atomically(&target.with_extension("json"), format!("{}\n", serde_json::to_string_pretty(&meta).unwrap()).as_bytes())?;
    Ok(json!({ "path": target }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_like_pictogrep() {
        assert_eq!(clean_stem("My Board — shot 1.png"), "my-board-shot-1");
        assert_eq!(clean_stem("¡¡¡.png"), "image");
        assert_eq!(clean_stem(&"a".repeat(80)).len(), 60);
    }

    #[test]
    fn clip_container() {
        let doc = clipstudio::Document {
            width: 4,
            height: 2,
            dpi: 72.0,
            layers: vec![clipstudio::Layer { name: "L".into(), folders: vec![], visible: true, opacity: 1.0, blend: "source-over", blend_approx: None, clip: false, kind: "raster", x: 1, y: 0, image: image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])) }],
            preview: Some(b"\x89PNGxx".to_vec()),
            skipped: vec![],
        };
        let out = encode_clip(&doc).unwrap();
        assert_eq!(&out[..4], b"ACLP");
        let n = u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize;
        let head: Value = serde_json::from_slice(&out[8..8 + n]).unwrap();
        let blobs = &out[8 + n..];
        let p = &head["layers"][0]["png"];
        let at = p["offset"].as_u64().unwrap() as usize;
        let len = p["length"].as_u64().unwrap() as usize;
        let img = image::load_from_memory(&blobs[at..at + len]).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(1, 1).0, [1, 2, 3, 255]);
        assert_eq!(head["preview"]["offset"], 0);
    }
}
