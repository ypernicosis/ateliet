//! Excalidraw drawings: the plugin's `.excalidraw.md` (a markdown head,
//! `# Excalidraw Data` with the text elements, element links and embedded
//! files, and `## Drawing` with the scene as `json` or LZString
//! `compressed-json`) and plain `.excalidraw` JSON. The scene is kept as
//! read, with its keys in order; only the elements and the sections the
//! plugin derives from them are written anew, the way the plugin writes them.

use super::element::Element;
use super::{lz, Board, FileRef, Format};
use regex::Regex;
use serde_json::{Map, Value};

static DRAWING: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"\n##? Drawing\n[^`]*```(compressed-json|json)\n([\s\S]*?)```\n").unwrap());
static DATA: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"(?m)^(%%\n+)?# Excalidraw Data(?:\n|$)").unwrap());
static TEXTS: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"(?m)^(%%\n+)?##? Text Elements(?:\n|$)").unwrap());
static BLOCKREF: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"\s\^(.{8})\n+").unwrap());
static LINK_LINE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"(?m)^(.{8}):\s*(.*)$").unwrap());
static FILE_LINE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| Regex::new(r"(?m)^([\w\d]+):\s*(.*)$").unwrap());

/// Scene JSON as the plugin writes it (`JSON.stringify(scene, null, "\t")`).
fn pretty(v: &Value, indent: &str) -> String {
    use serde::Serialize;
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    let _ = v.serialize(&mut ser);
    String::from_utf8(out).unwrap_or_default()
}

fn take_scene(b: &mut Board, json: &str, files_in_scene: bool) -> Result<(), String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("desenho ilegível: {e}"))?;
    let Value::Object(mut m) = v else { return Err("desenho ilegível".into()) };
    b.elements = m.get("elements").and_then(|e| e.as_array()).into_iter().flatten().filter_map(|e| Element::from_value(e.clone())).collect();
    // placeholders keep the keys' order
    if m.contains_key("elements") {
        m.insert("elements".into(), Value::Null);
    }
    if files_in_scene {
        if let Some(Value::Object(files)) = m.get("files") {
            b.files = files.iter().map(|(k, f)| (k.clone(), FileRef::Data(f.clone()))).collect();
        }
        if m.contains_key("files") {
            m.insert("files".into(), Value::Null);
        }
    }
    b.kept.scene = m;
    Ok(())
}

fn scene_json(b: &Board, files_in_scene: bool, indent: &str) -> String {
    let mut m = b.kept.scene.clone();
    let els = Value::Array(b.elements.iter().map(|e| e.to_value()).collect());
    if m.contains_key("elements") {
        m.insert("elements".into(), els);
    } else {
        m.insert("elements".into(), els);
    }
    if files_in_scene {
        let files: Map<String, Value> = b.files.iter().filter_map(|(k, f)| if let FileRef::Data(v) = f { Some((k.clone(), v.clone())) } else { None }).collect();
        m.insert("files".into(), Value::Object(files));
    }
    pretty(&Value::Object(m), indent)
}

/// A new, empty scene (what Excalidraw writes for a blank drawing).
fn blank_scene(b: &mut Board, source: &str) {
    let v = serde_json::json!({
        "type": "excalidraw",
        "version": 2,
        "source": source,
        "elements": null,
        "appState": { "gridSize": null, "viewBackgroundColor": "#ffffff" },
        "files": {}
    });
    b.kept.scene = v.as_object().cloned().unwrap_or_default();
}

pub fn parse_md(text: &str) -> Result<Board, String> {
    let text = text.replace("\r\n", "\n");
    let mut b = Board::empty(Format::ExcalidrawMd);
    let (data, json) = match DRAWING.captures(&text) {
        Some(c) => {
            let whole = c.get(0).unwrap();
            b.kept.compressed = &c[1] == "compressed-json";
            let raw = c[2].to_string();
            let json = if b.kept.compressed { lz::decompress_from_base64(&raw).ok_or("desenho comprimido ilegível")? } else { raw };
            let json = json[..json.rfind('}').map(|i| i + 1).unwrap_or(json.len())].to_string();
            b.kept.tail = text[whole.end()..].to_string();
            (text[..whole.start()].to_string(), json)
        }
        None => {
            // a note marked as a drawing with nothing drawn yet
            b.kept.compressed = true;
            b.kept.tail = "%%".into();
            blank_scene(&mut b, "https://github.com/zsviczian/obsidian-excalidraw-plugin");
            let head = text.trim_end_matches(['\n', '%']).to_string();
            b.kept.head = if head.is_empty() { String::new() } else { format!("{head}\n\n") };
            return Ok(b);
        }
    };
    take_scene(&mut b, &json, false)?;
    // the head ends where the data sections start
    let (start, rest_at, commented) = if let Some(c) = DATA.captures(&data) {
        let m = c.get(0).unwrap();
        (m.start(), m.end(), c.get(1).is_some())
    } else if let Some(c) = TEXTS.captures(&data) {
        let m = c.get(0).unwrap();
        (m.start(), m.start(), c.get(1).is_some())
    } else {
        // no sections: the head is everything, less the comment opening the drawing
        let head = data.strip_suffix("%%").unwrap_or(&data).to_string();
        (head.len(), data.len(), false)
    };
    b.kept.head = data[..start].to_string();
    b.kept.commented = commented;
    let rest = &data[rest_at.min(data.len())..];
    let after_texts = match TEXTS.find(rest) {
        Some(m) => &rest[m.end()..],
        None => rest,
    };
    let links_at = after_texts.find("## Element Links\n");
    let files_at = after_texts.find("## Embedded Files\n");
    let texts_end = [links_at, files_at].into_iter().flatten().min().unwrap_or(after_texts.len());
    let texts = &after_texts[..texts_end];
    b.kept.dummy = texts.contains("^_dummy!_");
    // text elements: the markdown wins over the JSON, as in the plugin
    let mut pos = 0;
    for c in BLOCKREF.captures_iter(texts) {
        let m = c.get(0).unwrap();
        let raw = texts[pos..m.start()].to_string();
        pos = m.end();
        let id = c[1].to_string();
        if id == "_dummy!_" {
            continue;
        }
        let raw = raw.trim_start_matches('\n').to_string();
        if let Some(e) = b.elements.iter_mut().find(|e| e.id() == id) {
            if e.kind() == "text" {
                e.v.insert("rawText".into(), Value::String(raw));
            } else {
                e.v.insert("link".into(), Value::String(raw));
            }
        }
        b.kept.text_order.push(id);
    }
    if let Some(l) = links_at {
        let end = files_at.filter(|f| *f > l).unwrap_or(after_texts.len());
        for c in LINK_LINE.captures_iter(&after_texts[l + 17..end]) {
            b.kept.links.push((c[1].to_string(), c[2].to_string()));
        }
    }
    if let Some(f) = files_at {
        let end = links_at.filter(|l| *l > f).unwrap_or(after_texts.len());
        for c in FILE_LINE.captures_iter(&after_texts[f + 18..end]) {
            let rest = c[2].trim_end().to_string();
            let simple = rest.strip_prefix("[[").and_then(|r| r.strip_suffix("]]")).filter(|p| !p.contains("]]"));
            let r = match simple {
                Some(p) => FileRef::Vault(p.to_string()),
                None => FileRef::Raw(rest),
            };
            b.files.push((c[1].to_string(), r));
        }
    }
    Ok(b)
}

pub fn write_md(b: &mut Board) -> String {
    super::element::fix_indices(&mut b.elements);
    let mut out = b.kept.head.clone();
    if b.kept.commented {
        out.push_str("%%\n");
    }
    out.push_str("# Excalidraw Data\n\n## Text Elements\n");
    if b.kept.dummy {
        out.push_str("\n^_dummy!_\n\n");
    }
    // text elements in the order the file had them, new ones after
    let live: Vec<&Element> = b.elements.iter().filter(|e| e.kind() == "text" && !e.deleted()).collect();
    let mut order: Vec<&str> = b.kept.text_order.iter().map(|s| s.as_str()).filter(|id| live.iter().any(|e| e.id() == *id)).collect();
    for e in &live {
        if !order.contains(&e.id()) {
            order.push(e.id());
        }
    }
    for id in &order {
        let e = live.iter().find(|e| e.id() == *id).unwrap();
        out.push_str(&format!("{} ^{}\n\n", e.raw_text(), id));
    }
    b.kept.text_order = order.iter().map(|s| s.to_string()).collect();
    let links: Vec<(String, String)> = b
        .kept
        .links
        .iter()
        .filter_map(|(id, old)| {
            let e = b.element(id)?;
            let link = e.get("link").as_str().map(str::to_string).unwrap_or_else(|| old.clone());
            Some((id.clone(), link))
        })
        .collect();
    if !links.is_empty() {
        out.push_str("## Element Links\n");
        for (id, l) in &links {
            out.push_str(&format!("{id}: {l}\n\n"));
        }
    }
    let used: std::collections::HashSet<&str> = b.elements.iter().filter_map(|e| e.get("fileId").as_str()).collect();
    let files: Vec<&(String, FileRef)> = b.files.iter().filter(|(k, _)| used.contains(k.as_str())).collect();
    if !files.is_empty() {
        out.push_str("## Embedded Files\n");
        for (k, f) in files {
            let rest = match f {
                FileRef::Vault(p) => format!("[[{p}]]"),
                FileRef::Raw(r) => r.clone(),
                FileRef::Data(_) => continue,
            };
            out.push_str(&format!("{k}: {rest}\n\n"));
        }
    }
    if !b.kept.commented {
        out.push_str("%%\n");
    }
    let json = scene_json(b, false, "\t");
    if b.kept.compressed {
        let packed = lz::compress_to_base64(&json);
        let chunks: Vec<&str> = packed.as_bytes().chunks(256).map(|c| std::str::from_utf8(c).unwrap_or("")).collect();
        out.push_str(&format!("## Drawing\n```compressed-json\n{}\n```\n", chunks.join("\n\n")));
    } else {
        out.push_str(&format!("## Drawing\n```json\n{json}\n```\n"));
    }
    out.push_str(&b.kept.tail);
    out
}

pub fn parse_json(text: &str) -> Result<Board, String> {
    let mut b = Board::empty(Format::Excalidraw);
    let trimmed = text.trim_end();
    b.kept.tail = text[trimmed.len()..].to_string();
    // the indentation the file was written with (2 spaces from excalidraw.com)
    b.kept.indent = text.lines().nth(1).map(|l| l.chars().take_while(|c| c.is_whitespace()).collect::<String>()).filter(|s| !s.is_empty()).unwrap_or_else(|| "  ".into());
    if trimmed.is_empty() {
        blank_scene(&mut b, "https://excalidraw.com");
        return Ok(b);
    }
    take_scene(&mut b, trimmed, true)?;
    Ok(b)
}

pub fn write_json(b: &mut Board) -> String {
    super::element::fix_indices(&mut b.elements);
    let indent = if b.kept.indent.is_empty() { "  ".to_string() } else { b.kept.indent.clone() };
    format!("{}{}", scene_json(b, true, &indent), b.kept.tail)
}

/// A new `.excalidraw.md` the plugin opens (compressed, as it saves by default).
pub fn new_md() -> String {
    let mut b = Board::empty(Format::ExcalidrawMd);
    b.kept.head = "---\n\nexcalidraw-plugin: parsed\ntags: [excalidraw]\n\n---\n==⚠  Switch to EXCALIDRAW VIEW in the MORE OPTIONS menu of this document. ⚠== You can decompress Drawing data with the command palette: 'Decompress current Excalidraw file'. For more info check in plugin settings under 'Saving'\n\n\n".into();
    b.kept.compressed = true;
    b.kept.tail = "%%".into();
    blank_scene(&mut b, "https://github.com/zsviczian/obsidian-excalidraw-plugin");
    b.write()
}

#[cfg(test)]
mod tests {
    use super::super::element::Style;
    use super::*;

    fn scene() -> String {
        let s: Value = serde_json::from_str(r##"{
            "type": "excalidraw",
            "version": 2,
            "source": "https://github.com/zsviczian/obsidian-excalidraw-plugin/releases/tag/2.1.4",
            "elements": [
                { "id": "Rk3b9Xq1", "type": "rectangle", "x": 10, "y": 20.5, "width": 200, "height": 100, "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "#a5d8ff", "fillStyle": "hachure", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100, "groupIds": [], "frameId": null, "index": "a0", "roundness": { "type": 3 }, "seed": 1966, "version": 12, "versionNonce": 77, "isDeleted": false, "boundElements": [{ "type": "text", "id": "Tx7aaaaa" }], "updated": 1700000000000, "link": null, "locked": false, "customData": { "mine": [1, 2] } },
                { "id": "Tx7aaaaa", "type": "text", "x": 60, "y": 55, "width": 100, "height": 25, "angle": 0, "strokeColor": "#1e1e1e", "backgroundColor": "transparent", "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100, "groupIds": [], "frameId": null, "index": "a1", "roundness": null, "seed": 1, "version": 3, "versionNonce": 5, "isDeleted": false, "boundElements": null, "updated": 1, "link": null, "locked": false, "text": "Olá", "rawText": "Olá", "fontSize": 20, "fontFamily": 5, "textAlign": "center", "verticalAlign": "middle", "containerId": "Rk3b9Xq1", "originalText": "Olá", "autoResize": true, "lineHeight": 1.25 },
                { "id": "Im9bbbbb", "type": "image", "x": 300, "y": 0, "width": 64, "height": 64, "angle": 0, "strokeColor": "transparent", "backgroundColor": "transparent", "fillStyle": "solid", "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100, "groupIds": [], "frameId": null, "index": "a2", "roundness": null, "seed": 2, "version": 1, "versionNonce": 1, "isDeleted": false, "boundElements": null, "updated": 1, "link": "[[Nota]]", "locked": false, "status": "saved", "fileId": "f00dfeed11", "scale": [1, 1] }
            ],
            "appState": { "theme": "light", "viewBackgroundColor": "#ffffff", "gridSize": null },
            "files": {}
        }"##)
        .unwrap();
        pretty(&s, "\t")
    }

    fn md(compressed: bool) -> String {
        let json = scene();
        let drawing = if compressed {
            let p = lz::compress_to_base64(&json);
            let chunks: Vec<&str> = p.as_bytes().chunks(256).map(|c| std::str::from_utf8(c).unwrap()).collect();
            format!("## Drawing\n```compressed-json\n{}\n```\n%%", chunks.join("\n\n"))
        } else {
            format!("## Drawing\n```json\n{json}\n```\n%%")
        };
        format!("---\n\nexcalidraw-plugin: parsed\ntags: [excalidraw]\n\n---\n==⚠  Switch to EXCALIDRAW VIEW in the MORE OPTIONS menu of this document. ⚠== You can decompress Drawing data with the command palette: 'Decompress current Excalidraw file'. For more info check in plugin settings under 'Saving'\n\nNotas no verso.\n\n# Excalidraw Data\n\n## Text Elements\nOlá ^Tx7aaaaa\n\n## Element Links\nIm9bbbbb: [[Nota]]\n\n## Embedded Files\nf00dfeed11: [[pasta/foto.png]]\n\n%%\n{drawing}")
    }

    #[test]
    fn round_trips_the_plugin_format() {
        for compressed in [true, false] {
            let text = md(compressed);
            let mut b = parse_md(&text).unwrap();
            assert_eq!(b.elements.len(), 3);
            assert_eq!(b.kept.compressed, compressed);
            assert_eq!(b.file("f00dfeed11"), Some(&FileRef::Vault("pasta/foto.png".into())));
            assert_eq!(b.kept.links, vec![("Im9bbbbb".to_string(), "[[Nota]]".to_string())]);
            assert!(b.kept.head.ends_with("Notas no verso.\n\n"));
            assert_eq!(b.write(), text, "compressed: {compressed}");
        }
    }

    #[test]
    fn markdown_text_wins_and_edits_are_written() {
        let text = md(true).replace("Olá ^Tx7aaaaa", "Olá [[mundo]] ^Tx7aaaaa");
        let mut b = parse_md(&text).unwrap();
        assert_eq!(b.element("Tx7aaaaa").unwrap().raw_text(), "Olá [[mundo]]");
        b.element_mut("Tx7aaaaa").unwrap().set_text("Novo texto");
        let mut n = Element::new("ellipse", 0.0, 0.0, 50.0, 50.0, &Style::default());
        n.set("index", serde_json::json!("a3"));
        b.elements.push(n);
        let out = b.write();
        assert!(out.contains("## Text Elements\nNovo texto ^Tx7aaaaa\n\n## Element Links"));
        let back = parse_md(&out).unwrap();
        let t = back.element("Tx7aaaaa").unwrap();
        assert_eq!(t.s("text"), "Novo texto");
        assert_eq!(t.get("version").as_i64(), Some(4), "a changed element gets a new version");
        assert_eq!(back.elements[0].get("customData"), &serde_json::json!({ "mine": [1, 2] }), "unknown fields survive");
        assert_eq!(back.elements.len(), 4);
    }

    #[test]
    fn plain_excalidraw_json() {
        let json = scene().replace('\t', "  ") + "\n";
        let mut b = parse_json(&json).unwrap();
        assert_eq!(b.kept.indent, "  ");
        assert_eq!(b.write(), json);
        let with_file = json.replace("\"files\": {}", "\"files\": {\n    \"f00dfeed11\": {\n      \"mimeType\": \"image/png\",\n      \"id\": \"f00dfeed11\",\n      \"dataURL\": \"data:image/png;base64,iVBORw0KGgo=\",\n      \"created\": 1\n    }\n  }");
        let mut b = parse_json(&with_file).unwrap();
        assert!(matches!(b.file("f00dfeed11"), Some(FileRef::Data(_))));
        assert_eq!(b.write(), with_file);
    }

    #[test]
    fn new_drawing_opens_again() {
        let t = new_md();
        assert!(t.contains("excalidraw-plugin: parsed"));
        assert!(t.contains("```compressed-json\n"));
        let b = parse_md(&t).unwrap();
        assert!(b.elements.is_empty());
        assert_eq!(super::super::detect("Desenho.excalidraw.md", &t), Some(Format::ExcalidrawMd));
    }
}
