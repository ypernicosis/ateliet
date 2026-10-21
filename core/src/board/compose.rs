//! Drawing a frame region: paper, onion skins, layers (opacity, blend modes,
//! clipping to the layer below, the stroke being drawn) and picture layers.
//! The same code draws the screen tiles, thumbnails and exports.

use super::doc::BoardDoc;
use super::tile::{mul255, parse_color, Tile, TILE};
use serde_json::Value;
use std::collections::HashMap;

/// The stroke in progress, drawn over its layer until it is committed.
pub struct Overlay<'a> {
    pub layer: &'a str,
    pub tiles: &'a HashMap<(i32, i32), Tile>,
    pub opacity: f32,
    pub erase: bool,
    pub alpha_lock: bool,
}

/// Decoded pictures of image and trace layers (premultiplied RGBA).
#[derive(Default)]
pub struct Pictures {
    map: HashMap<String, Option<(u32, u32, Vec<u8>)>>,
}

impl Pictures {
    fn get(&mut self, doc: &BoardDoc, ref_id: &str, trace: Option<&Value>) -> Option<&(u32, u32, Vec<u8>)> {
        let key = format!("{ref_id}|{}", trace.map(|t| t.to_string()).unwrap_or_default());
        if !self.map.contains_key(&key) {
            let r = doc.board["references"].as_array().and_then(|a| a.iter().find(|r| r["id"].as_str() == Some(ref_id))).cloned();
            let img = r.and_then(|r| doc.reference_bytes(r["path"].as_str().unwrap_or_default())).and_then(|b| image::load_from_memory(&b).ok());
            let decoded = img.map(|i| {
                let i = if i.width() > 4096 || i.height() > 4096 { i.thumbnail(4096, 4096) } else { i };
                let mut rgba = i.to_rgba8();
                if let Some(t) = trace {
                    super::trace::apply(&mut rgba, t);
                }
                let (w, h) = (rgba.width(), rgba.height());
                let mut px = rgba.into_raw();
                for p in px.chunks_exact_mut(4) {
                    let a = p[3] as u32;
                    for c in p.iter_mut().take(3) {
                        *c = mul255(*c as u32, a) as u8;
                    }
                }
                (w, h, px)
            });
            self.map.insert(key.clone(), decoded);
        }
        self.map.get(&key).and_then(|x| x.as_ref())
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

pub struct Options<'a> {
    pub background: bool,
    pub onion: bool,
    pub overlay: Option<Overlay<'a>>,
    /// Export: trace layers only when their settings say so.
    pub exporting: bool,
}

#[inline]
fn blend_channel(mode: &str, s: f32, d: f32) -> f32 {
    match mode {
        "multiply" => s * d,
        "screen" => s + d - s * d,
        "darken" => s.min(d),
        "lighten" => s.max(d),
        "difference" => (s - d).abs(),
        "exclusion" => s + d - 2.0 * s * d,
        "overlay" => {
            if d <= 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }
        "hard-light" => {
            if s <= 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }
        "soft-light" => {
            if s <= 0.5 {
                d - (1.0 - 2.0 * s) * d * (1.0 - d)
            } else {
                let g = if d <= 0.25 { ((16.0 * d - 12.0) * d + 4.0) * d } else { d.sqrt() };
                d + (2.0 * s - 1.0) * (g - d)
            }
        }
        "color-dodge" => {
            if d == 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (d / (1.0 - s)).min(1.0)
            }
        }
        "color-burn" => {
            if d >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - d) / s).min(1.0)
            }
        }
        _ => s,
    }
}

/// Composite a premultiplied layer buffer onto the destination.
fn blend_into(dst: &mut [u8], src: &[u8], opacity: f32, mode: &str) {
    let o = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    if mode == "source-over" || mode.is_empty() || matches!(mode, "hue" | "saturation" | "color" | "luminosity") {
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            let sa = mul255(s[3] as u32, o);
            if sa == 0 {
                continue;
            }
            let inv = 255 - sa;
            for k in 0..3 {
                d[k] = (mul255(s[k] as u32, o) + mul255(d[k] as u32, inv)).min(255) as u8;
            }
            d[3] = (sa + mul255(d[3] as u32, inv)).min(255) as u8;
        }
        return;
    }
    if mode == "lighter" {
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            for k in 0..4 {
                d[k] = (d[k] as u32 + mul255(s[k] as u32, o)).min(255) as u8;
            }
        }
        return;
    }
    let of = o as f32 / 255.0;
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let sa = s[3] as f32 / 255.0 * of;
        if sa <= 0.0 {
            continue;
        }
        let da = d[3] as f32 / 255.0;
        let mut out = [0f32; 4];
        for k in 0..3 {
            let sc = s[k] as f32 / 255.0 * of; // premultiplied
            let dc = d[k] as f32 / 255.0;
            let su = if sa > 0.0 { sc / sa } else { 0.0 };
            let du = if da > 0.0 { dc / da } else { 0.0 };
            out[k] = sc * (1.0 - da) + dc * (1.0 - sa) + sa * da * blend_channel(mode, su, du);
        }
        out[3] = sa + da - sa * da;
        for k in 0..4 {
            d[k] = (out[k].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Pixels of a raster layer over a region (canvas coordinates), premultiplied.
fn raster_region(doc: &mut BoardDoc, layer: &str, x0: i32, y0: i32, w: usize, h: usize, out: &mut [u8]) -> bool {
    let l = doc.layer(layer);
    let lx = l["x"].as_f64().unwrap_or(0.0).round() as i32;
    let ly = l["y"].as_f64().unwrap_or(0.0).round() as i32;
    let t = TILE as i32;
    // layer-space rectangle
    let (ax, ay) = (x0 - lx, y0 - ly);
    let (tx0, ty0) = (ax.div_euclid(t), ay.div_euclid(t));
    let (tx1, ty1) = ((ax + w as i32 - 1).div_euclid(t), (ay + h as i32 - 1).div_euclid(t));
    let mut any = false;
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let Some(tile) = doc.tile(layer, tx, ty) else { continue };
            any = true;
            // overlap between the tile and the region, in layer space
            let (sx0, sy0) = ((tx * t).max(ax), (ty * t).max(ay));
            let (sx1, sy1) = (((tx + 1) * t).min(ax + w as i32), ((ty + 1) * t).min(ay + h as i32));
            for y in sy0..sy1 {
                let src = ((y - ty * t) as usize * TILE + (sx0 - tx * t) as usize) * 4;
                let dst = ((y - ay) as usize * w + (sx0 - ax) as usize) * 4;
                let n = (sx1 - sx0) as usize * 4;
                out[dst..dst + n].copy_from_slice(&tile.px[src..src + n]);
            }
        }
    }
    any
}

/// A picture layer (image or trace) placed by its transform, sampled bilinearly.
fn picture_region(pic: &(u32, u32, Vec<u8>), t: &Value, x0: i32, y0: i32, w: usize, h: usize, out: &mut [u8]) {
    let (iw, ih, px) = (pic.0 as f32, pic.1 as f32, &pic.2);
    let cx = t["x"].as_f64().unwrap_or(0.0) as f32;
    let cy = t["y"].as_f64().unwrap_or(0.0) as f32;
    let sx = t["sx"].as_f64().unwrap_or(1.0) as f32;
    let sy = t["sy"].as_f64().unwrap_or(1.0) as f32;
    let rot = t["rotation"].as_f64().unwrap_or(0.0) as f32;
    let (c, s) = (rot.cos(), rot.sin());
    if sx == 0.0 || sy == 0.0 {
        return;
    }
    let stride = pic.0 as usize;
    for y in 0..h {
        for x in 0..w {
            // canvas → picture coordinates (inverse of translate·rotate·scale)
            let dx = x0 as f32 + x as f32 + 0.5 - cx;
            let dy = y0 as f32 + y as f32 + 0.5 - cy;
            let u = (c * dx + s * dy) / sx + iw / 2.0 - 0.5;
            let v = (-s * dx + c * dy) / sy + ih / 2.0 - 0.5;
            if u < -0.5 || v < -0.5 || u > iw - 0.5 || v > ih - 0.5 {
                continue;
            }
            let (u0, v0) = (u.floor().max(0.0) as usize, v.floor().max(0.0) as usize);
            let (u1, v1) = ((u0 + 1).min(pic.0 as usize - 1), (v0 + 1).min(pic.1 as usize - 1));
            let (fu, fv) = ((u - u0 as f32).clamp(0.0, 1.0), (v - v0 as f32).clamp(0.0, 1.0));
            let o = (y * w + x) * 4;
            for k in 0..4 {
                let p = |xx: usize, yy: usize| px[(yy * stride + xx) * 4 + k] as f32;
                let top = p(u0, v0) * (1.0 - fu) + p(u1, v0) * fu;
                let bot = p(u0, v1) * (1.0 - fu) + p(u1, v1) * fu;
                out[o + k] = (top * (1.0 - fv) + bot * fv).round() as u8;
            }
        }
    }
}

/// Layers of a frame drawn into `dst` (no paper).
fn layers_into(doc: &mut BoardDoc, pics: &mut Pictures, frame: usize, x0: i32, y0: i32, w: usize, h: usize, dst: &mut [u8], opts: &Options, only_raster: bool) {
    let ids = doc.frame_layers(frame);
    let mut buf = vec![0u8; w * h * 4];
    let mut base_alpha: Option<Vec<u8>> = None;
    let mut deferred: Vec<String> = Vec::new();
    let mut order: Vec<String> = Vec::new();
    // trace layers "above" go last (a light table)
    for id in &ids {
        let l = doc.layer(id);
        if l["kind"] == "trace" && l["trace"]["above"].as_bool() == Some(true) {
            deferred.push(id.clone());
        } else {
            order.push(id.clone());
        }
    }
    order.extend(deferred);
    for id in order {
        let l = doc.layer(&id).clone();
        if l["visible"].as_bool() == Some(false) {
            continue;
        }
        let kind = l["kind"].as_str().unwrap_or("raster");
        if only_raster && kind != "raster" {
            continue;
        }
        if opts.exporting && kind == "trace" && l["trace"]["export"].as_bool() != Some(true) {
            continue;
        }
        buf.iter_mut().for_each(|b| *b = 0);
        let mut drew = match kind {
            "raster" => raster_region(doc, &id, x0, y0, w, h, &mut buf),
            "image" | "trace" => {
                let rid = l["src"].as_str().unwrap_or_default().to_string();
                let trace = if kind == "trace" { Some(&l["trace"]) } else { None };
                match pics.get(doc, &rid, trace) {
                    Some(p) => {
                        let p = p.clone();
                        picture_region(&p, &l["transform"], x0, y0, w, h, &mut buf);
                        true
                    }
                    None => false,
                }
            }
            _ => false,
        };
        if let Some(ov) = opts.overlay.as_ref().filter(|o| o.layer == id) {
            // the stroke being drawn, merged into this layer's pixels
            let lx = l["x"].as_f64().unwrap_or(0.0).round() as i32;
            let ly = l["y"].as_f64().unwrap_or(0.0).round() as i32;
            let mut stroke = vec![0u8; w * h * 4];
            let t = TILE as i32;
            for (&(tx, ty), tile) in ov.tiles {
                let (sx0, sy0) = ((tx * t + lx).max(x0), (ty * t + ly).max(y0));
                let (sx1, sy1) = (((tx + 1) * t + lx).min(x0 + w as i32), ((ty + 1) * t + ly).min(y0 + h as i32));
                if sx0 >= sx1 || sy0 >= sy1 {
                    continue;
                }
                for y in sy0..sy1 {
                    let src = ((y - ly - ty * t) as usize * TILE + (sx0 - lx - tx * t) as usize) * 4;
                    let dsti = ((y - y0) as usize * w + (sx0 - x0) as usize) * 4;
                    let n = (sx1 - sx0) as usize * 4;
                    stroke[dsti..dsti + n].copy_from_slice(&tile.px[src..src + n]);
                }
            }
            let mut tmp = Tile { px: buf.clone() };
            let st = Tile { px: stroke };
            if tmp.px.len() == st.px.len() {
                tmp.merge(&st, ov.opacity, ov.erase, ov.alpha_lock);
            }
            buf = tmp.px;
            drew = true;
        }
        if !drew {
            if !l["clip"].as_bool().unwrap_or(false) {
                base_alpha = Some(vec![0u8; w * h]);
            }
            continue;
        }
        if l["clip"].as_bool().unwrap_or(false) {
            if let Some(base) = &base_alpha {
                for (p, &a) in buf.chunks_exact_mut(4).zip(base.iter()) {
                    for c in p.iter_mut() {
                        *c = mul255(*c as u32, a as u32) as u8;
                    }
                }
            }
        } else {
            base_alpha = Some(buf.chunks_exact(4).map(|p| p[3]).collect());
        }
        let opacity = l["opacity"].as_f64().unwrap_or(1.0) as f32;
        let mode = l["blend"].as_str().unwrap_or("source-over");
        blend_into(dst, &buf, opacity, mode);
    }
}

/// Draw `w`×`h` canvas pixels from (`x0`, `y0`) of a frame; premultiplied RGBA.
pub fn region(doc: &mut BoardDoc, pics: &mut Pictures, frame: usize, x0: i32, y0: i32, w: usize, h: usize, opts: &Options) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    if opts.background {
        let bg = doc.background();
        if bg != "transparent" {
            let c = parse_color(&bg);
            let (bw, bh) = (doc.width() as i32, doc.height() as i32);
            let infinite = doc.infinite();
            for y in 0..h {
                let cy = y0 + y as i32;
                for x in 0..w {
                    let cx = x0 + x as i32;
                    if infinite || (cx >= 0 && cy >= 0 && cx < bw && cy < bh) {
                        let i = (y * w + x) * 4;
                        out[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                    }
                }
            }
        }
    }
    if opts.onion && doc.board["onion"]["enabled"].as_bool() == Some(true) {
        let onion = doc.board["onion"].clone();
        let before = onion["before"].as_u64().unwrap_or(1) as i64;
        let after = onion["after"].as_u64().unwrap_or(1) as i64;
        let base = onion["opacity"].as_f64().unwrap_or(0.3) as f32;
        let tint = onion["tint"].as_bool().unwrap_or(true);
        let fade = onion["fade"].as_bool().unwrap_or(true);
        let pictures = onion["pictures"].as_bool().unwrap_or(false);
        let n = doc.frame_count() as i64;
        let mut sides: Vec<(i64, [u8; 3], f32)> = Vec::new();
        for k in (1..=before).rev() {
            let o = if fade { base * (1.0 - (k - 1) as f32 / (before as f32 + 1.0)) } else { base };
            sides.push((frame as i64 - k, parse_color(onion["colorBefore"].as_str().unwrap_or("#d0453b")), o));
        }
        for k in 1..=after {
            let o = if fade { base * (1.0 - (k - 1) as f32 / (after as f32 + 1.0)) } else { base };
            sides.push((frame as i64 + k, parse_color(onion["colorAfter"].as_str().unwrap_or("#2f9e5a")), o));
        }
        for (f, color, o) in sides {
            if f < 0 || f >= n {
                continue;
            }
            let mut ghost = vec![0u8; w * h * 4];
            let plain = Options { background: false, onion: false, overlay: None, exporting: false };
            layers_into(doc, pics, f as usize, x0, y0, w, h, &mut ghost, &plain, !pictures);
            if tint {
                for p in ghost.chunks_exact_mut(4) {
                    let a = p[3] as u32;
                    p[0] = mul255(color[0] as u32, a) as u8;
                    p[1] = mul255(color[1] as u32, a) as u8;
                    p[2] = mul255(color[2] as u32, a) as u8;
                }
            }
            blend_into(&mut out, &ghost, o, "source-over");
        }
    }
    layers_into(doc, pics, frame, x0, y0, w, h, &mut out, opts, false);
    out
}

/// A whole frame as straight RGBA, scaled to fit `max` (thumbnails, exports).
pub fn frame_image(doc: &mut BoardDoc, pics: &mut Pictures, frame: usize, max: u32, exporting: bool) -> (u32, u32, Vec<u8>) {
    let (w, h) = (doc.width() as usize, doc.height() as usize);
    let opts = Options { background: true, onion: false, overlay: None, exporting };
    let px = region(doc, pics, frame, 0, 0, w, h, &opts);
    let straight = super::tile::unpremultiply(&px);
    let img = image::RgbaImage::from_raw(w as u32, h as u32, straight).unwrap_or_default();
    let img = if max > 0 && (img.width() > max || img.height() > max) { image::DynamicImage::ImageRgba8(img).thumbnail(max, max).to_rgba8() } else { img };
    (img.width(), img.height(), img.into_raw())
}

/// The thumbnail PNG of a frame (what `thumbs/<frame>.png` holds).
pub fn thumbnail_png(doc: &mut BoardDoc, pics: &mut Pictures, frame: usize) -> Option<Vec<u8>> {
    let (w, h, px) = frame_image(doc, pics, frame, 320, true);
    Some(super::tile::encode_png(&px, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::doc::PAPER;
    use crate::board::tile::DabMode;

    #[test]
    fn layers_paper_and_offsets() {
        let mut d = BoardDoc::new("animatic", 600, 400, PAPER, 24);
        let layer = d.active_layer(0);
        d.tile_mut(&layer, 0, 0).dab(10.0, 10.0, 4.0, 1.0, [0, 0, 255], 1.0, DabMode::Over);
        let mut pics = Pictures::default();
        let opts = Options { background: true, onion: false, overlay: None, exporting: false };
        let px = region(&mut d, &mut pics, 0, 0, 0, 20, 20, &opts);
        assert_eq!(&px[(10 * 20 + 10) * 4..(10 * 20 + 10) * 4 + 4], &[0, 0, 255, 255]);
        assert_eq!(&px[0..4], &[0xe0, 0xde, 0xdb, 255]);
        // outside the page: no paper
        let out = region(&mut d, &mut pics, 0, 700, 0, 4, 4, &opts);
        assert_eq!(out[3], 0);
        // a layer moved by (100, 50) shows its pixels there
        d.layer_mut(&layer)["x"] = serde_json::json!(100);
        d.layer_mut(&layer)["y"] = serde_json::json!(50);
        let moved = region(&mut d, &mut pics, 0, 100, 50, 20, 20, &opts);
        assert_eq!(&moved[(10 * 20 + 10) * 4..(10 * 20 + 10) * 4 + 4], &[0, 0, 255, 255]);
        // multiply over paper darkens
        d.layer_mut(&layer)["blend"] = serde_json::json!("multiply");
        let m = region(&mut d, &mut pics, 0, 100, 50, 20, 20, &opts);
        assert!(m[(10 * 20 + 10) * 4] < 10);
        let png = thumbnail_png(&mut d, &mut pics, 0).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}
