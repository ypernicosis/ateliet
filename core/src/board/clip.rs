//! Clip Studio files into storyboards: each layer with pixels becomes a
//! drawing layer (named "folder / layer") with its visibility, opacity,
//! blend mode and clipping; the paper becomes the background colour.

use super::doc::{new_layer, BoardDoc};
use super::paint::{apply_image, Mode};
use super::tile::mul255;
use crate::clipstudio::Document;
use serde_json::json;

/// Put the file's layers into `frame` above position `at` (bottom to top),
/// moved by (`dx`, `dy`). Returns the Clip Studio blend modes that could
/// only be approximated.
pub fn add_layers(doc: &mut BoardDoc, clip: &Document, frame: usize, at: usize, paper_as_background: bool, dx: i32, dy: i32) -> Vec<String> {
    let mut made = Vec::new();
    let mut approximated = Vec::new();
    let mut paper_done = !paper_as_background;
    for cl in &clip.layers {
        let name = cl.folders.iter().chain(std::iter::once(&cl.name)).filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" / ");
        let name = if name.is_empty() { "Camada".to_string() } else { name };
        if cl.kind == "paper" && !paper_done && cl.visible && cl.image.width() > 0 {
            let p = cl.image.get_pixel(0, 0);
            doc.board["background"] = json!(format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2]));
            paper_done = true;
            continue;
        }
        let mut l = new_layer(&name, "raster");
        l["visible"] = json!(cl.visible);
        l["opacity"] = json!(cl.opacity);
        l["blend"] = json!(cl.blend);
        l["clip"] = json!(cl.clip);
        l["x"] = json!(dx);
        l["y"] = json!(dy);
        let id = l["id"].as_str().unwrap_or_default().to_string();
        doc.board["layers"][&id] = l;
        let (w, h) = (cl.image.width() as usize, cl.image.height() as usize);
        if cl.kind == "paper" {
            // a paper kept as a layer covers the canvas
            let p = cl.image.get_pixel(0, 0);
            let (cw, ch) = (clip.width as usize, clip.height as usize);
            let px: Vec<u8> = (0..cw * ch).flat_map(|_| [p[0], p[1], p[2], 255]).collect();
            apply_image(doc, &id, 0, 0, cw, ch, &px, 1.0, Mode::Over);
        } else if w > 0 && h > 0 {
            let mut px = cl.image.as_raw().clone();
            for c in px.chunks_exact_mut(4) {
                let a = c[3] as u32;
                for v in c.iter_mut().take(3) {
                    *v = mul255(*v as u32, a) as u8;
                }
            }
            apply_image(doc, &id, cl.x, cl.y, w, h, &px, 1.0, Mode::Over);
        }
        if let Some(a) = cl.blend_approx {
            approximated.push(a.to_string());
        }
        made.push(id);
    }
    let mut layers = doc.frame_layers(frame);
    let at = at.min(layers.len());
    for (k, id) in made.iter().enumerate() {
        layers.insert(at + k, id.clone());
    }
    doc.board["frames"][frame]["layers"] = json!(layers);
    if let Some(last) = made.last() {
        doc.board["frames"][frame]["active"] = json!(last);
    }
    doc.mark_frame(frame);
    doc.touch();
    approximated
}

/// A new storyboard holding the file as its first frame (the empty drawing
/// layer a new storyboard starts with stays on top).
pub fn board_from_clip(clip: &Document, fps: u32) -> (BoardDoc, Vec<String>) {
    let mut doc = BoardDoc::new("animatic", clip.width, clip.height, super::doc::PAPER, fps);
    let approximated = add_layers(&mut doc, clip, 0, 0, true, 0, 0);
    if let Some(p) = &clip.preview {
        doc.preview = Some(p.clone());
    }
    (doc, approximated)
}

/// What an import could not carry over exactly, in a sentence (or empty).
pub fn summary(clip: &Document, approximated: &[String]) -> String {
    let mut parts = Vec::new();
    let corrections = clip.skipped.iter().filter(|s| s.ends_with(": correction layer")).count();
    let other: Vec<&String> = clip.skipped.iter().filter(|s| !s.ends_with(": correction layer")).collect();
    if corrections > 0 {
        parts.push(format!("{corrections} {} de fora (não têm pixels)", if corrections == 1 { "camada de correção ficou" } else { "camadas de correção ficaram" }));
    }
    if !other.is_empty() {
        let shown: Vec<&str> = other.iter().take(3).map(|s| s.as_str()).collect();
        let more = if other.len() > 3 { format!(" e mais {}", other.len() - 3) } else { String::new() };
        parts.push(format!("de fora: {}{more}", shown.join("; ")));
    }
    if !approximated.is_empty() {
        let mut kinds: Vec<&str> = approximated.iter().map(|s| s.as_str()).collect();
        kinds.sort();
        kinds.dedup();
        parts.push(format!("{} {} ({})", approximated.len(), if approximated.len() == 1 { "modo de mesclagem aproximado" } else { "modos de mesclagem aproximados" }, kinds.join(", ")));
    }
    if parts.is_empty() { String::new() } else { format!(" {}.", parts.join("; ")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipstudio::Layer;

    fn layer(name: &str, kind: &'static str, px: [u8; 4], w: u32, h: u32) -> Layer {
        Layer { name: name.into(), folders: vec!["Pasta".into()], visible: true, opacity: 0.5, blend: "multiply", blend_approx: if kind == "raster" { Some("Color burn") } else { None }, clip: false, kind, x: 10, y: 20, image: image::RgbaImage::from_pixel(w, h, image::Rgba(px)) }
    }

    #[test]
    fn layers_and_paper() {
        let doc = Document { width: 300, height: 200, dpi: 300.0, layers: vec![layer("Papel", "paper", [250, 240, 230, 255], 1, 1), layer("Tinta", "raster", [255, 0, 0, 255], 40, 30)], preview: None, skipped: vec!["X: correction layer".into()] };
        let (mut b, approx) = board_from_clip(&doc, 24);
        assert_eq!(b.background(), "#faf0e6");
        let ids = b.frame_layers(0);
        assert_eq!(ids.len(), 2);
        let l = b.layer(&ids[0]).clone();
        assert_eq!(l["name"], "Pasta / Tinta");
        assert_eq!(l["blend"], "multiply");
        let px = crate::board::paint::read_layer(&mut b, &ids[0], 15, 25, 1, 1);
        assert_eq!(px, vec![255, 0, 0, 255]);
        assert!(summary(&doc, &approx).contains("correção"));
    }
}
