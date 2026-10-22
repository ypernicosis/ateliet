//! Exports of a whole storyboard: one PNG per frame, a contact sheet (the
//! frames in a grid with their notes) and an animated PNG of the timeline.

use super::compose::{self as bc, Options, Pictures};
use super::doc::BoardDoc;
use crate::typeset::compose;
use crate::typeset::fonts::FontDb;
use crate::typeset::{Item, Pen, PT_MM};
use serde_json::{json, Value as Json};

/// A name safe for a file (no separators or characters vaults refuse).
pub fn safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if r#"\/:*?"<>|#^[]"#.contains(c) { '-' } else { c }).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "quadro".into() } else { s }
}

/// One frame as straight RGBA: scaled, with the trace layers or not, on
/// the paper or transparent.
pub fn frame_rgba(doc: &mut BoardDoc, pics: &mut Pictures, frame: usize, scale: f32, with_trace: bool, transparent: bool) -> (u32, u32, Vec<u8>) {
    let (w, h) = (doc.width() as usize, doc.height() as usize);
    // trace layers count as exported for this picture only
    let traces: Vec<(String, Json)> = doc.frame_layers(frame).into_iter().filter(|id| doc.layer(id)["kind"] == "trace").map(|id| { let v = doc.layer(&id)["trace"]["export"].clone(); (id, v) }).collect();
    if with_trace {
        for (id, _) in &traces {
            doc.layer_mut(id)["trace"]["export"] = json!(true);
        }
    }
    let opts = Options { background: !transparent, onion: false, overlay: None, exporting: true };
    let px = bc::region(doc, pics, frame, 0, 0, w, h, &opts);
    if with_trace {
        for (id, v) in traces {
            doc.layer_mut(&id)["trace"]["export"] = v;
        }
    }
    let straight = super::tile::unpremultiply(&px);
    let img = image::RgbaImage::from_raw(w as u32, h as u32, straight).unwrap_or_default();
    let img = if (scale - 1.0).abs() > 1e-3 {
        let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    (img.width(), img.height(), img.into_raw())
}

/// File names of the frames: "<storyboard> 001 <name>.png".
pub fn frame_file_name(base: &str, i: usize, name: &str) -> String {
    let name = name.trim();
    format!("{} {:03}{}.png", safe(base), i + 1, if name.is_empty() { String::new() } else { format!(" {}", safe(name)) })
}

fn timecode(frames: f32, fps: u32) -> String {
    let s = frames / fps.max(1) as f32;
    let m = (s / 60.0).floor();
    format!("{}:{:04.1}", m as u32, s - m * 60.0).replace('.', ",")
}

/// All frames in a grid, each with its number, name, start, duration, shot
/// and camera, and (with `notes`) the action, dialogue and sound.
pub fn contact_sheet(doc: &mut BoardDoc, pics: &mut Pictures, db: &FontDb, title: &str, columns: usize, notes: bool, width: u32) -> (u32, u32, Vec<u8>) {
    let n = doc.frame_count();
    let cols = columns.clamp(1, 8).min(n.max(1));
    let wf = width as f32;
    let (pad, gap) = (32.0f32, 28.0f32);
    let cell_w = ((wf - 2.0 * pad - (cols as f32 - 1.0) * gap) / cols as f32).floor();
    let aspect = doc.height() as f32 / doc.width().max(1) as f32;
    let img_h = (cell_w * aspect).round();
    let line_h = (cell_w / 26.0).round().clamp(14.0, 30.0);
    let text_lines = if notes { 5.0 } else { 1.0 };
    let cell_h = img_h + 10.0 + line_h * (text_lines + 0.5);
    let rows = n.div_ceil(cols);
    let head_h = line_h * 3.0;
    let hf = (pad * 2.0 + head_h + rows as f32 * cell_h + (rows as f32 - 1.0).max(0.0) * gap).round();
    let ink = [0x23, 0x22, 0x1f];
    let muted = [0x6b, 0x68, 0x62];
    let sans = compose::first_font(db, &["Inter", "Helvetica Neue", "Arial"], false, 400);
    let sans_b = compose::first_font(db, &["Inter", "Helvetica Neue", "Arial"], false, 600);
    let label = |text: &str, font, size: f32, x: f32, y: f32, color: [u8; 3]| -> Item {
        // sizes in pixels; the typesetter measures type in points over millimetres
        let size = size / PT_MM;
        let (f, glyphs, map, _) = compose::run(db, font, text, size, 0.0);
        Item::Text { font: f, size, x, y, color, glyphs, text: text.to_string(), map }
    };
    let width_of = |text: &str, font, size: f32| compose::run(db, font, text, size / PT_MM, 0.0).3;
    let mut page = crate::typeset::Page::new(wf, hf, 1, crate::typeset::PageKind::Fixed, [255, 255, 255]);
    let mut pictures = Vec::new();
    page.items.push(label(title, sans_b, line_h * 1.5, pad, pad + line_h * 1.4, ink));
    let fps = doc.fps();
    let durations: Vec<f32> = (0..n).map(|i| doc.frame(i)["duration"].as_f64().unwrap_or(24.0) as f32).collect();
    let total: f32 = durations.iter().sum();
    let info = super::pages::kind_info(doc.kind());
    let size = if doc.infinite() { "tela infinita".to_string() } else { format!("{} × {}", doc.width(), doc.height()) };
    let summary = format!("{} {} · {} a {} qps · {}", n, if n == 1 { info.unit } else { info.units }, timecode(total, fps), fps, size);
    page.items.push(label(&summary, sans, line_h, pad, pad + line_h * 2.7, muted));
    let mut start = 0.0f32;
    for i in 0..n {
        let (col, row) = (i % cols, i / cols);
        let x = pad + col as f32 * (cell_w + gap);
        let y = pad + head_h + row as f32 * (cell_h + gap);
        let (pw, ph, px) = bc::frame_image(doc, pics, i, (cell_w * 2.0) as u32, true);
        let k = (cell_w / pw as f32).min(img_h / ph as f32);
        let (w, h) = (pw as f32 * k, ph as f32 * k);
        let (ix, iy) = (x + (cell_w - w) / 2.0, y + (img_h - h) / 2.0);
        pictures.push(crate::typeset::Picture { w: pw, h: ph, rgba: px, jpeg: None });
        page.items.push(Item::Image { pic: pictures.len() - 1, x: ix, y: iy, w, h });
        let f = doc.frame(i).clone();
        let color = f["color"].as_str().map(super::tile::parse_color);
        page.items.push(Item::Path { polys: vec![vec![(ix, iy), (ix + w, iy), (ix + w, iy + h), (ix, iy + h)]], fill: None, fill_alpha: 1.0, stroke: Some(Pen { width: if color.is_some() { 3.0 } else { 1.0 }, color: color.unwrap_or(ink), dash: None }) });
        let nt = &f["notes"];
        let name = f["name"].as_str().unwrap_or("").trim().to_string();
        let head = if name.is_empty() { (i + 1).to_string() } else { format!("{} · {name}", i + 1) };
        let mut ty = y + img_h + 10.0 + line_h;
        page.items.push(label(&head, sans_b, line_h, x, ty, ink));
        let mut meta = format!("{} · {:.1} s", timecode(start, fps), durations[i] / fps.max(1) as f32).replace('.', ",");
        for k in ["shot", "camera"] {
            if let Some(v) = nt[k].as_str().filter(|v| !v.trim().is_empty()) {
                meta.push_str(" · ");
                meta.push_str(v.trim());
            }
        }
        let mw = width_of(&meta, sans, line_h);
        let hw = width_of(&head, sans_b, line_h);
        page.items.push(label(&meta, sans, line_h, (x + hw + 12.0).max(x + cell_w - mw), ty, muted));
        start += durations[i];
        if !notes {
            continue;
        }
        let mut body = Vec::new();
        for (k, pre, post) in [("action", "", ""), ("dialogue", "“", "”"), ("sound", "♪ ", "")] {
            if let Some(v) = nt[k].as_str().filter(|v| !v.trim().is_empty()) {
                body.push(format!("{pre}{}{post}", v.trim()));
            }
        }
        if !body.is_empty() {
            ty += line_h * 0.3;
            page.items.extend(compose::text_box(
                &compose::TextBox { x, y: ty, w: cell_w, h: line_h * 4.4, text: &body.join("\n"), font: "Inter", kind: "sans", size: line_h / PT_MM, color: ink, align: "left", valign: "top", lh: 1.05, weight: 400, italic: false, spacing: 0.0, stroke: 0.0, stroke_color: [255; 3], vertical: false, upper: false },
                db,
            ));
        }
    }
    crate::typeset::raster::render(&page, db, &pictures, 1.0)
}

fn png_chunks(png: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 8;
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
        let mut t = [0u8; 4];
        t.copy_from_slice(&png[at + 4..at + 8]);
        if at + 8 + len > png.len() {
            break;
        }
        out.push((t, &png[at + 8..at + 8 + len]));
        at += 12 + len;
    }
    out
}

fn chunk(out: &mut Vec<u8>, t: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(t);
    out.extend_from_slice(data);
    let crc = super::zip::crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Frames of one size as an animated PNG, each held `(num, den)` seconds.
pub fn apng(frames: &[(u32, u32, Vec<u8>)], delays: &[(u16, u16)]) -> Vec<u8> {
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut seq = 0u32;
    for (i, (w, h, px)) in frames.iter().enumerate() {
        let png = super::tile::encode_png(px, *w, *h);
        let cs = png_chunks(&png);
        if i == 0 {
            if let Some((_, ihdr)) = cs.iter().find(|c| &c.0 == b"IHDR") {
                chunk(&mut out, b"IHDR", ihdr);
            }
            let mut actl = Vec::new();
            actl.extend_from_slice(&(frames.len() as u32).to_be_bytes());
            actl.extend_from_slice(&0u32.to_be_bytes());
            chunk(&mut out, b"acTL", &actl);
        }
        let mut fctl = Vec::with_capacity(26);
        fctl.extend_from_slice(&seq.to_be_bytes());
        seq += 1;
        fctl.extend_from_slice(&w.to_be_bytes());
        fctl.extend_from_slice(&h.to_be_bytes());
        fctl.extend_from_slice(&0u32.to_be_bytes());
        fctl.extend_from_slice(&0u32.to_be_bytes());
        let (num, den) = delays.get(i).copied().unwrap_or((1, 1));
        fctl.extend_from_slice(&num.to_be_bytes());
        fctl.extend_from_slice(&den.to_be_bytes());
        fctl.extend_from_slice(&[0, 0]);
        chunk(&mut out, b"fcTL", &fctl);
        for (t, data) in cs.iter().filter(|c| &c.0 == b"IDAT") {
            if i == 0 {
                chunk(&mut out, t, data);
            } else {
                let mut d = Vec::with_capacity(4 + data.len());
                d.extend_from_slice(&seq.to_be_bytes());
                seq += 1;
                d.extend_from_slice(data);
                chunk(&mut out, b"fdAT", &d);
            }
        }
    }
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The timeline as an animated PNG, frames scaled to fit `max` pixels.
pub fn animated_png(doc: &mut BoardDoc, pics: &mut Pictures, max: u32) -> Vec<u8> {
    let n = doc.frame_count();
    let fps = doc.fps().clamp(1, 65535) as u16;
    let mut frames = Vec::with_capacity(n);
    let mut delays = Vec::with_capacity(n);
    for i in 0..n {
        frames.push(bc::frame_image(doc, pics, i, max, true));
        let d = doc.frame(i)["duration"].as_f64().unwrap_or(24.0).round().clamp(1.0, 65535.0) as u16;
        delays.push((d, fps));
    }
    apng(&frames, &delays)
}

/// A proportion written the way Pictogrep names it ("16:9"), or the ratio.
pub fn aspect_of(w: u32, h: u32) -> String {
    fn g(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { g(b, a % b) }
    }
    let d = g(w, h).max(1);
    let (a, b) = (w / d, h / d);
    if a <= 32 && b <= 32 {
        return format!("{a}:{b}");
    }
    let r = w as f64 / h.max(1) as f64;
    for (name, v) in [("16:9", 16.0 / 9.0), ("4:3", 4.0 / 3.0), ("2:1", 2.0), ("3:4", 0.75), ("1:1", 1.0), ("9:16", 9.0 / 16.0)] {
        if (r - v).abs() < 0.01 {
            return name.into();
        }
    }
    format!("{w}:{h}")
}

/// Pictogrep's file-name stem: lowercase ASCII letters and digits, runs of
/// anything else as one dash, at most 60 characters.
pub fn clean_stem(value: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in value.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out: String = out.trim_matches('-').chars().take(60).collect();
    if out.is_empty() { "image".into() } else { out }
}

/// Every frame into Pictogrep's storyboards folder, named the way
/// Pictogrep names its own (`0001_16x9_<stem>_<id>.png` and a JSON beside
/// it). The id comes from the storyboard and the frame, so exporting again
/// replaces the earlier copy. Returns how many were written.
pub fn to_pictogrep(doc: &mut BoardDoc, pics: &mut Pictures, dir: &std::path::Path, abs: &str, name: &str) -> Result<usize, String> {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut n = 0;
    for i in 0..doc.frame_count() {
        let (w, h, px) = frame_rgba(doc, pics, i, 1.0, false, false);
        let png = super::tile::encode_png(&px, w, h);
        let f = doc.frame(i).clone();
        let aspect = aspect_of(w, h);
        let aspect_file = if aspect.chars().all(|c| c.is_ascii_digit() || c == ':') && aspect.contains(':') { aspect.replace(':', "x") } else { "free".into() };
        let key = format!("{abs}#{}", f["id"].as_str().unwrap_or(""));
        let id: String = Sha256::digest(key.as_bytes())[..8].iter().map(|b| format!("{b:02x}")).collect();
        // an earlier export of the same frame, maybe at another position
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().ends_with(&format!("_{id}.png")) {
                let _ = std::fs::remove_file(e.path());
                let _ = std::fs::remove_file(e.path().with_extension("json"));
            }
        }
        let fname = f["name"].as_str().filter(|s| !s.trim().is_empty()).map(String::from).unwrap_or_else(|| (i + 1).to_string());
        let file = format!("{:04}_{aspect_file}_{}_{id}.png", (i + 1).min(9999), clean_stem(&format!("{name} {fname}")));
        let target = dir.join(&file);
        std::fs::write(&target, png).map_err(|e| e.to_string())?;
        // the reference the frame was drawn from, when it is a file of its own
        let source = f["reference"].as_str().and_then(|rid| doc.board["references"].as_array().and_then(|a| a.iter().find(|r| r["id"].as_str() == Some(rid))).and_then(|r| r["path"].as_str()).filter(|p| !p.starts_with("refs/")).map(String::from)).unwrap_or_default();
        let meta = json!({ "file": file, "source": source, "aspect": aspect, "tags": [], "query": name });
        std::fs::write(target.with_extension("json"), format!("{}\n", serde_json::to_string_pretty(&meta).unwrap_or_default())).map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apng_has_frames() {
        let f = (2u32, 2u32, vec![255u8; 16]);
        let a = apng(&[f.clone(), f], &[(1, 2), (1, 2)]);
        let cs = png_chunks(&a);
        let names: Vec<&[u8; 4]> = cs.iter().map(|c| &c.0).collect();
        assert_eq!(names.first().copied(), Some(b"IHDR"));
        assert!(names.contains(&b"acTL") && names.contains(&b"fdAT"));
        assert_eq!(names.iter().filter(|n| **n == b"fcTL").count(), 2);
        assert!(image::load_from_memory(&a).is_ok());
        assert_eq!(frame_file_name("A/B", 0, " cena "), "A-B 001 cena.png");
        assert_eq!(aspect_of(1920, 1080), "16:9");
        assert_eq!(clean_stem("My Board — shot 1"), "my-board-shot-1");
    }
}
