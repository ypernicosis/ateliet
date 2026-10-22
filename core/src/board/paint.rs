//! Painting by regions: flood fill, masks from polygons and shapes, and
//! pictures or masks laid on a layer's tiles (returning what undo needs).

use super::doc::BoardDoc;
use super::tile::{mul255, Tile, TILE};
use crate::scriptorium::mapa::Raster;

/// A tile before and after a change.
pub type TileChange = ((i32, i32), Option<Tile>, Option<Tile>);

/// Scanline flood fill over premultiplied RGBA; a mask (1 = filled), or
/// `None` when the seed is outside.
pub fn flood(d: &[u8], w: usize, h: usize, sx: i64, sy: i64, tol: u32) -> Option<Vec<u8>> {
    if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
        return None;
    }
    let (sx, sy) = (sx as usize, sy as usize);
    let mut mask = vec![0u8; w * h];
    let i0 = (sy * w + sx) * 4;
    let seed = [d[i0] as i32, d[i0 + 1] as i32, d[i0 + 2] as i32, d[i0 + 3] as i32];
    let t = (tol * tol * 4) as i32;
    let same = |i: usize| {
        let j = i * 4;
        // transparent pixels match each other whatever their colour bytes
        if seed[3] == 0 && d[j + 3] == 0 {
            return true;
        }
        let (dr, dg, db, da) = (d[j] as i32 - seed[0], d[j + 1] as i32 - seed[1], d[j + 2] as i32 - seed[2], d[j + 3] as i32 - seed[3]);
        dr * dr + dg * dg + db * db + da * da <= t
    };
    let mut stack = vec![(sx, sy)];
    while let Some((x0, y)) = stack.pop() {
        let row = y * w;
        let mut x = x0;
        if mask[row + x] != 0 {
            continue;
        }
        while x > 0 && mask[row + x - 1] == 0 && same(row + x - 1) {
            x -= 1;
        }
        let (mut up, mut down) = (false, false);
        while x < w && mask[row + x] == 0 && same(row + x) {
            mask[row + x] = 1;
            if y > 0 {
                let u = row - w + x;
                if mask[u] == 0 && same(u) {
                    if !up {
                        stack.push((x, y - 1));
                        up = true;
                    }
                } else {
                    up = false;
                }
            }
            if y + 1 < h {
                let dn = row + w + x;
                if mask[dn] == 0 && same(dn) {
                    if !down {
                        stack.push((x, y + 1));
                        down = true;
                    }
                } else {
                    down = false;
                }
            }
            x += 1;
        }
    }
    Some(mask)
}

/// Grow a mask by one pixel (4-neighbours), so fills tuck under the lines.
pub fn dilate(m: &mut [u8], w: usize, h: usize) {
    let src = m.to_vec();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if src[i] != 0 {
                continue;
            }
            if (x > 0 && src[i - 1] != 0) || (x + 1 < w && src[i + 1] != 0) || (y > 0 && src[i - w] != 0) || (y + 1 < h && src[i + w] != 0) {
                m[i] = 1;
            }
        }
    }
}

fn coverage(r: Raster) -> Vec<u8> {
    r.px.chunks_exact(4).map(|p| p[3]).collect()
}

fn shift(polys: &[Vec<(f64, f64)>], x0: f64, y0: f64) -> Vec<Vec<(f64, f64)>> {
    polys.iter().map(|p| p.iter().map(|q| (q.0 - x0, q.1 - y0)).collect()).collect()
}

/// Antialiased coverage (0–255) of polygons, over the box at (`x0`, `y0`).
pub fn polygon_mask(polys: &[Vec<(f64, f64)>], x0: f64, y0: f64, w: usize, h: usize) -> Vec<u8> {
    let mut r = Raster::new(w, h, 4);
    r.fill(&shift(polys, x0, y0), [255.0; 3], 1.0);
    coverage(r)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind {
    Line,
    Rect,
    Ellipse,
    /// A camera-move arrow: a line with a filled head.
    Arrow,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 4] = [ShapeKind::Line, ShapeKind::Rect, ShapeKind::Ellipse, ShapeKind::Arrow];
}

/// The outline of an ellipse in the box `a`–`b`.
pub fn ellipse_points(a: (f64, f64), b: (f64, f64), n: usize) -> Vec<(f64, f64)> {
    let (cx, cy, rx, ry) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, (b.0 - a.0).abs() / 2.0, (b.1 - a.1).abs() / 2.0);
    (0..n).map(|i| {
        let t = i as f64 / n as f64 * std::f64::consts::TAU;
        (cx + t.cos() * rx, cy + t.sin() * ry)
    }).collect()
}

/// The head of a camera arrow ending at `b`.
pub fn arrow_head(a: (f64, f64), b: (f64, f64), width: f64) -> Vec<(f64, f64)> {
    let ang = (b.1 - a.1).atan2(b.0 - a.0);
    let k = (width * 4.0).max(10.0);
    vec![
        (b.0 + ang.cos() * width, b.1 + ang.sin() * width),
        (b.0 - (ang - 0.45).cos() * k, b.1 - (ang - 0.45).sin() * k),
        (b.0 - (ang + 0.45).cos() * k, b.1 - (ang + 0.45).sin() * k),
    ]
}

/// Coverage of a shape from `a` to `b`, over the box at (`x0`, `y0`).
#[allow(clippy::too_many_arguments)]
pub fn shape_mask(kind: ShapeKind, a: (f64, f64), b: (f64, f64), width: f64, fill: bool, x0: f64, y0: f64, w: usize, h: usize) -> Vec<u8> {
    let mut r = Raster::new(w, h, 4);
    let white = [255.0; 3];
    match kind {
        ShapeKind::Line | ShapeKind::Arrow => {
            r.stroke(&shift(&[vec![a, b]], x0, y0), width, white, 1.0);
            if kind == ShapeKind::Arrow {
                r.fill(&shift(&[arrow_head(a, b, width)], x0, y0), white, 1.0);
            }
        }
        ShapeKind::Rect | ShapeKind::Ellipse => {
            let mut poly = if kind == ShapeKind::Rect { vec![a, (b.0, a.1), b, (a.0, b.1)] } else { ellipse_points(a, b, 96) };
            if fill {
                r.fill(&shift(&[poly], x0, y0), white, 1.0);
            } else {
                poly.push(poly[0]);
                r.stroke(&shift(&[poly], x0, y0), width, white, 1.0);
            }
        }
    }
    coverage(r)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Over,
    /// Under what is already there.
    Behind,
    Erase,
    /// Only where the layer has pixels (alpha lock).
    Atop,
}

/// A layer's pixels in a box (layer coordinates), premultiplied RGBA.
pub fn read_layer(doc: &mut BoardDoc, layer: &str, x0: i32, y0: i32, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let t = TILE as i32;
    for ty in y0.div_euclid(t)..=(y0 + h as i32 - 1).div_euclid(t) {
        for tx in x0.div_euclid(t)..=(x0 + w as i32 - 1).div_euclid(t) {
            let Some(tile) = doc.tile(layer, tx, ty) else { continue };
            let (bx, by) = (tx * t, ty * t);
            let (sx0, sy0) = ((x0 - bx).max(0), (y0 - by).max(0));
            let (sx1, sy1) = ((x0 + w as i32 - bx).min(t), (y0 + h as i32 - by).min(t));
            for y in sy0..sy1 {
                let src = (y as usize * TILE + sx0 as usize) * 4;
                let dst = (((by + y - y0) as usize) * w + (bx + sx0 - x0) as usize) * 4;
                let n = (sx1 - sx0) as usize * 4;
                out[dst..dst + n].copy_from_slice(&tile.px[src..src + n]);
            }
        }
    }
    out
}

/// A premultiplied picture `w`×`h` laid on a layer at (`x0`, `y0`) (layer
/// coordinates), at an opacity.
#[allow(clippy::too_many_arguments)]
pub fn apply_image(doc: &mut BoardDoc, layer: &str, x0: i32, y0: i32, w: usize, h: usize, px: &[u8], alpha: f32, mode: Mode) -> Vec<TileChange> {
    let mut changes = Vec::new();
    if w == 0 || h == 0 {
        return changes;
    }
    let o = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    let t = TILE as i32;
    for ty in y0.div_euclid(t)..=(y0 + h as i32 - 1).div_euclid(t) {
        for tx in x0.div_euclid(t)..=(x0 + w as i32 - 1).div_euclid(t) {
            let (bx, by) = (tx * t, ty * t);
            let (sx0, sy0) = ((x0 - bx).max(0), (y0 - by).max(0));
            let (sx1, sy1) = ((x0 + w as i32 - bx).min(t), (y0 + h as i32 - by).min(t));
            // nothing to do where the picture is empty here
            let any = (sy0..sy1).any(|y| (sx0..sx1).any(|x| px[((((by + y - y0) as usize) * w) + (bx + x - x0) as usize) * 4 + 3] > 0));
            if !any {
                continue;
            }
            let before = doc.tile(layer, tx, ty).cloned();
            if before.is_none() && matches!(mode, Mode::Erase | Mode::Atop) {
                continue;
            }
            let tile = doc.tile_mut(layer, tx, ty);
            for y in sy0..sy1 {
                for x in sx0..sx1 {
                    let s = ((((by + y - y0) as usize) * w) + (bx + x - x0) as usize) * 4;
                    let sa = mul255(px[s + 3] as u32, o);
                    if sa == 0 {
                        continue;
                    }
                    let sc = [mul255(px[s] as u32, o), mul255(px[s + 1] as u32, o), mul255(px[s + 2] as u32, o)];
                    let d = (y as usize * TILE + x as usize) * 4;
                    let da = tile.px[d + 3] as u32;
                    match mode {
                        Mode::Over => {
                            for c in 0..3 {
                                tile.px[d + c] = (sc[c] + mul255(tile.px[d + c] as u32, 255 - sa)).min(255) as u8;
                            }
                            tile.px[d + 3] = (sa + mul255(da, 255 - sa)).min(255) as u8;
                        }
                        Mode::Behind => {
                            for c in 0..3 {
                                tile.px[d + c] = (tile.px[d + c] as u32 + mul255(sc[c], 255 - da)).min(255) as u8;
                            }
                            tile.px[d + 3] = (da + mul255(sa, 255 - da)).min(255) as u8;
                        }
                        Mode::Erase => {
                            for c in 0..4 {
                                tile.px[d + c] = mul255(tile.px[d + c] as u32, 255 - sa) as u8;
                            }
                        }
                        Mode::Atop => {
                            for c in 0..3 {
                                tile.px[d + c] = (mul255(sc[c], da) + mul255(tile.px[d + c] as u32, 255 - sa)).min(255) as u8;
                            }
                        }
                    }
                }
            }
            let empty = tile.is_empty();
            if empty {
                doc.set_tile(layer, tx, ty, None);
            }
            let after = if empty { None } else { doc.tile(layer, tx, ty).cloned() };
            changes.push(((tx, ty), before, after));
        }
    }
    changes
}

/// A colour through a coverage mask, laid on a layer.
#[allow(clippy::too_many_arguments)]
pub fn apply_mask(doc: &mut BoardDoc, layer: &str, x0: i32, y0: i32, w: usize, h: usize, cov: &[u8], color: [u8; 3], alpha: f32, mode: Mode) -> Vec<TileChange> {
    let mut px = vec![0u8; w * h * 4];
    for (i, &k) in cov.iter().enumerate() {
        if k == 0 {
            continue;
        }
        let k = k as u32;
        px[i * 4] = mul255(color[0] as u32, k) as u8;
        px[i * 4 + 1] = mul255(color[1] as u32, k) as u8;
        px[i * 4 + 2] = mul255(color[2] as u32, k) as u8;
        px[i * 4 + 3] = k as u8;
    }
    apply_image(doc, layer, x0, y0, w, h, &px, alpha, mode)
}

/// Replace a layer's pixels in a box (layer coordinates).
pub fn write_layer(doc: &mut BoardDoc, layer: &str, x0: i32, y0: i32, w: usize, h: usize, px: &[u8]) -> Vec<TileChange> {
    let mut changes = Vec::new();
    let t = TILE as i32;
    for ty in y0.div_euclid(t)..=(y0 + h as i32 - 1).div_euclid(t) {
        for tx in x0.div_euclid(t)..=(x0 + w as i32 - 1).div_euclid(t) {
            let (bx, by) = (tx * t, ty * t);
            let (sx0, sy0) = ((x0 - bx).max(0), (y0 - by).max(0));
            let (sx1, sy1) = ((x0 + w as i32 - bx).min(t), (y0 + h as i32 - by).min(t));
            let before = doc.tile(layer, tx, ty).cloned();
            let mut tile = before.clone().unwrap_or_default();
            for y in sy0..sy1 {
                let d = (y as usize * TILE + sx0 as usize) * 4;
                let s = (((by + y - y0) as usize) * w + (bx + sx0 - x0) as usize) * 4;
                let n = (sx1 - sx0) as usize * 4;
                tile.px[d..d + n].copy_from_slice(&px[s..s + n]);
            }
            let after = if tile.is_empty() { None } else { Some(tile) };
            if before.as_ref().map(|b| &b.px) == after.as_ref().map(|a| &a.px) {
                continue;
            }
            doc.set_tile(layer, tx, ty, after.clone());
            changes.push(((tx, ty), before, after));
        }
    }
    changes
}

/// The canvas box a layer's pixels cover (raster layers: its tiles).
pub fn layer_bounds(doc: &BoardDoc, id: &str) -> Option<(i32, i32, usize, usize)> {
    let l = doc.layer(id);
    if l["kind"].as_str().unwrap_or("raster") != "raster" {
        if doc.infinite() {
            let (x, y) = (l["transform"]["x"].as_f64().unwrap_or(0.0) as i32, l["transform"]["y"].as_f64().unwrap_or(0.0) as i32);
            return Some((x - 4096, y - 4096, 8192, 8192));
        }
        return Some((0, 0, doc.width() as usize, doc.height() as usize));
    }
    let tiles = doc.layer_tiles(id);
    if tiles.is_empty() {
        return None;
    }
    let (lx, ly) = (l["x"].as_f64().unwrap_or(0.0).round() as i32, l["y"].as_f64().unwrap_or(0.0).round() as i32);
    let t = TILE as i32;
    let x0 = tiles.iter().map(|k| k.0).min().unwrap() * t + lx;
    let y0 = tiles.iter().map(|k| k.1).min().unwrap() * t + ly;
    let x1 = (tiles.iter().map(|k| k.0).max().unwrap() + 1) * t + lx;
    let y1 = (tiles.iter().map(|k| k.1).max().unwrap() + 1) * t + ly;
    let (mut x0, mut y0, mut x1, mut y1) = (x0, y0, x1, y1);
    if !doc.infinite() {
        x0 = x0.max(0);
        y0 = y0.max(0);
        x1 = x1.min(doc.width() as i32);
        y1 = y1.min(doc.height() as i32);
    }
    (x1 > x0 && y1 > y0).then(|| (x0, y0, (x1 - x0) as usize, (y1 - y0) as usize))
}

/// Lay `upper` (any kind) onto the drawing layer `lower` with its opacity,
/// blend mode and clipping; the tiles of `lower` that changed.
pub fn merge_down(doc: &mut BoardDoc, pics: &mut super::compose::Pictures, upper: &str, lower: &str) -> Vec<TileChange> {
    let Some((x0, y0, w, h)) = layer_bounds(doc, upper) else { return Vec::new() };
    let Some(mut up) = super::compose::layer_region(doc, pics, upper, x0, y0, w, h) else { return Vec::new() };
    let u = doc.layer(upper).clone();
    let lo = doc.layer(lower).clone();
    let (lx, ly) = (lo["x"].as_f64().unwrap_or(0.0).round() as i32, lo["y"].as_f64().unwrap_or(0.0).round() as i32);
    let mut low = read_layer(doc, lower, x0 - lx, y0 - ly, w, h);
    if u["clip"].as_bool() == Some(true) {
        for (s, d) in up.chunks_exact_mut(4).zip(low.chunks_exact(4)) {
            for c in s.iter_mut() {
                *c = mul255(*c as u32, d[3] as u32) as u8;
            }
        }
    }
    let opacity = u["opacity"].as_f64().unwrap_or(1.0) as f32;
    super::compose::blend(&mut low, &up, opacity, u["blend"].as_str().unwrap_or("source-over"));
    write_layer(doc, lower, x0 - lx, y0 - ly, w, h, &low)
}

/// A premultiplied picture `sw`×`sh` drawn `size` big around `center`,
/// turned by `angle` (radians) and maybe mirrored, into the box `w`×`h` at
/// (`x0`, `y0`).
#[allow(clippy::too_many_arguments)]
pub fn place_image(src: &[u8], sw: usize, sh: usize, center: (f64, f64), size: (f64, f64), angle: f64, flip: bool, x0: f64, y0: f64, w: usize, h: usize) -> Vec<u8> {
    let mut r = Raster::new(w, h, 1);
    let kx = size.0 / sw.max(1) as f64 * if flip { -1.0 } else { 1.0 };
    let ky = size.1 / sh.max(1) as f64;
    let (s, c) = angle.sin_cos();
    let (hx, hy) = (-(sw as f64) / 2.0 * kx, -(sh as f64) / 2.0 * ky);
    let m = [c * kx, s * kx, -s * ky, c * ky, center.0 - x0 + c * hx - s * hy, center.1 - y0 + s * hx + c * hy];
    r.image(src, sw, sh, m);
    r.px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_stops_at_lines() {
        // a 10×10 picture with a vertical line at x = 5
        let (w, h) = (10, 10);
        let mut d = vec![0u8; w * h * 4];
        for y in 0..h {
            d[(y * w + 5) * 4 + 3] = 255;
        }
        let m = flood(&d, w, h, 1, 1, 10).unwrap();
        assert_eq!(m.iter().filter(|&&v| v != 0).count(), 50);
        let mut m2 = m.clone();
        dilate(&mut m2, w, h);
        assert_eq!(m2.iter().filter(|&&v| v != 0).count(), 60);
    }

    #[test]
    fn masks_paint_and_erase() {
        let mut doc = BoardDoc::new("animatic", 800, 600, "#ffffff", 24);
        let l = doc.add_layer(0, "x");
        let cov = polygon_mask(&[vec![(10.0, 10.0), (600.0, 10.0), (600.0, 40.0), (10.0, 40.0)]], 0.0, 0.0, 700, 50);
        let ch = apply_mask(&mut doc, &l, 0, 0, 700, 50, &cov, [255, 0, 0], 1.0, Mode::Over);
        assert_eq!(ch.len(), 2);
        let px = read_layer(&mut doc, &l, 520, 20, 1, 1);
        assert_eq!(px, vec![255, 0, 0, 255]);
        let ch = apply_mask(&mut doc, &l, 0, 0, 700, 50, &cov, [0, 0, 0], 1.0, Mode::Erase);
        assert!(ch.iter().all(|c| c.2.is_none()));
        let s = shape_mask(ShapeKind::Arrow, (5.0, 25.0), (90.0, 25.0), 4.0, false, 0.0, 0.0, 100, 50);
        assert!(s[25 * 100 + 50] > 200 && s[5 * 100 + 50] == 0);
    }
}
