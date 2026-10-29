//! Cover textures, made from the cover's colour, at the cover's real size.
//!
//! Each texture is a surface, not a pattern laid on a colour: a height map
//! (the threads of a cloth, the grain of a leather, the fibres of a paper)
//! lit from the top left, so it reads as relief, with a little change of
//! tone where the material changes (a slub in a thread, a fleck in kraft).
//! The relief is measured in millimetres of the cover, so a linen thread is
//! as thin on an A6 as on an A4, and the contrast stays low: a few per cent
//! round the colour. Marbling is the exception: veins of two or three inks
//! drawn from the cover's colour, combed and swirled. Every cover gets a
//! soft light across it and slightly darker edges, as a bound board has.

use image::{Rgba, RgbaImage};

/// The textures a cover can have: key (as written in `capa-textura`) and
/// the name shown.
pub const TEXTURES: &[(&str, &str)] = &[("", "Lisa"), ("linho", "Linho"), ("tecido", "Tecido"), ("kraft", "Papel kraft"), ("couro", "Couro"), ("marmore", "Marmorizada"), ("cortica", "Cortiça"), ("pontos", "Pontilhada")];

/// Pixels a millimetre the textures are made at.
pub const PX_MM: f32 = 6.0;

fn hash(a: i32, b: i32, c: u32) -> f32 {
    let mut x = (a as u32).wrapping_mul(0x9e37_79b9) ^ (b as u32).wrapping_mul(0x85eb_ca6b) ^ c.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x & 0xffff) as f32 / 65535.0
}

/// Smooth value noise, one cell per unit.
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (x - x.floor(), y - y.floor());
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash(ix, iy, seed);
    let b = hash(ix + 1, iy, seed);
    let c = hash(ix, iy + 1, seed);
    let d = hash(ix + 1, iy + 1, seed);
    let top = a + (b - a) * tx;
    top + (c + (d - c) * tx - top) * ty
}

/// Noise with finer octaves over it, in 0..1.
fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let (mut v, mut amp, mut f, mut sum) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        v += amp * noise(x * f, y * f, seed + o * 31);
        sum += amp;
        amp *= 0.5;
        f *= 2.03;
    }
    v / sum
}

/// Distance to the nearest and second nearest of points scattered one a
/// cell (Worley), and a value proper to the nearest one.
fn cells(x: f32, y: f32, seed: u32) -> (f32, f32, f32) {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (mut d1, mut d2, mut id) = (9.0f32, 9.0f32, 0.0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            let px = cx as f32 + 0.15 + 0.7 * hash(cx, cy, seed);
            let py = cy as f32 + 0.15 + 0.7 * hash(cx, cy, seed + 1);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = hash(cx, cy, seed + 2);
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, id)
}

/// The material at a point (mm): its height (0..1) and a change of tone
/// (around 0) where the material itself is lighter or darker.
fn surface(kind: &str, x: f32, y: f32, seed: u32) -> (f32, f32) {
    use std::f32::consts::TAU;
    match kind {
        // fine threads both ways (about 0.45 mm), each a little uneven, with
        // slubs; the weft a touch higher than the warp
        "linho" => {
            let p = 0.75;
            let wobble_x = 0.08 * noise(y * 0.6, x * 0.05, seed);
            let wobble_y = 0.08 * noise(x * 0.6, y * 0.05, seed + 3);
            // each thread its own thickness and tone
            let (tx, ty) = ((x / p + wobble_x).round() as i32, (y / p + wobble_y).round() as i32);
            let (kx, ky) = (0.6 + 0.4 * hash(tx, 0, seed + 13), 0.6 + 0.4 * hash(0, ty, seed + 17));
            let warp = (0.5 + 0.5 * ((x / p + wobble_x) * TAU).cos()).powf(1.0 / kx) * (0.7 + 0.3 * noise(x / p, y * 0.12, seed + 5));
            let weft = (0.5 + 0.5 * ((y / p + wobble_y) * TAU).cos()).powf(1.0 / ky) * (0.7 + 0.3 * noise(y / p, x * 0.12, seed + 7));
            let slub = (noise(x * 0.2, y * 2.5, seed + 9) - 0.5) * 0.12 + (noise(y * 0.2, x * 2.5, seed + 11) - 0.5) * 0.12;
            let tone = (hash(tx, 1, seed + 19) - 0.5) * 0.05 + (hash(1, ty, seed + 23) - 0.5) * 0.05 + slub;
            (warp.max(weft * 1.05) * 0.8 + 0.2 * warp * weft, tone)
        }
        // a coarser plain weave (1.2 mm): the threads go over and under
        "tecido" => {
            let p = 1.8;
            let (u, v) = (x / p, y / p);
            let (cu, cv) = (u.floor() as i32, v.floor() as i32);
            let (fu, fv) = (u - u.floor(), v - v.floor());
            let over = (cu + cv).rem_euclid(2) == 0;
            // a thread is a rounded ridge across its cell, pinched at the ends
            let ridge = |t: f32, along: f32| (t * std::f32::consts::PI).sin().max(0.0).powf(0.6) * (0.55 + 0.45 * (along * std::f32::consts::PI).sin().max(0.0));
            // threads of a hand loom: not all as thick, not all as straight
            let jit = 0.8 + 0.2 * hash(cu, cv, seed + 3);
            let (fu, fv) = ((fu + 0.08 * (noise(v * 0.7, u * 0.1, seed + 5) - 0.5)).clamp(0.0, 1.0), (fv + 0.08 * (noise(u * 0.7, v * 0.1, seed + 6) - 0.5)).clamp(0.0, 1.0));
            let h = jit * if over { ridge(fv, fu) } else { ridge(fu, fv) * 0.92 };
            let thread = if over { hash(0, cv, seed + 7) } else { hash(cu, 0, seed + 8) };
            let tone = (thread - 0.5) * 0.07 + (fbm(x * 0.06, y * 0.06, 3, seed) - 0.5) * 0.08;
            (h, tone)
        }
        // pulp mottling, fibres along the sheet, a few dark flecks
        "kraft" => {
            let mottle = fbm(x * 0.06, y * 0.06, 4, seed);
            let fibres = noise(x * 0.35, y * 4.5, seed + 4) * 0.6 + noise(x * 2.5, y * 0.4, seed + 6) * 0.4;
            let fleck = if hash((x * 1.6) as i32, (y * 1.6) as i32, seed + 8) > 0.9975 { -0.22 } else { 0.0 };
            let streak = (noise(x * 0.9, y * 9.0, seed + 12) - 0.5) * 0.08;
            (fibres * 0.5 + mottle * 0.5, (mottle - 0.5) * 0.26 + streak + fleck)
        }
        // pebbled grain (cells of 1.4 mm, raised in the middle), soft folds
        "couro" => {
            let s = 1.6;
            let (d1, d2, id) = cells(x / s + 0.4 * fbm(x * 0.05, y * 0.05, 2, seed + 3), y / s, seed);
            let pebble = ((d2 - d1) * 2.2).min(1.0).powf(0.7);
            let folds = fbm(x * 0.02, y * 0.03, 3, seed + 7);
            (pebble * 0.75 + folds * 0.25, (id - 0.5) * 0.04 + (folds - 0.5) * 0.10)
        }
        // granules of 0.6–1.5 mm, each its own tone
        "cortica" => {
            let (d1, d2, id) = cells(x / 0.9, y / 0.9, seed);
            let (e1, _, id2) = cells(x / 0.45 + 7.0, y / 0.45, seed + 5);
            let h = ((d2 - d1) * 2.0).min(1.0) * 0.7 + (1.0 - e1.min(1.0)) * 0.3;
            let pore = if d1 < 0.06 && id > 0.8 { -0.25 } else { 0.0 };
            (h, (id - 0.5) * 0.22 + (id2 - 0.5) * 0.10 + pore)
        }
        // small dimples pressed into a smooth board, 3.5 mm apart, in staggered rows
        "pontos" => {
            let g = 3.5;
            let row = (y / g).floor();
            let ox = if row as i32 % 2 == 0 { 0.0 } else { g / 2.0 };
            let (u, v) = ((x + ox).rem_euclid(g) - g / 2.0, y.rem_euclid(g) - g / 2.0);
            let r = (u * u + v * v).sqrt();
            let dimple = if r < 0.55 { -(1.0 - r / 0.55).powf(0.8) } else { 0.0 };
            (0.5 + 0.5 * dimple, 0.0)
        }
        _ => (0.5 + 0.06 * fbm(x * 0.5, y * 0.5, 3, seed), 0.0),
    }
}

/// Marbling: the cover's colour, a darker and a lighter ink and pale
/// veins, swirled the way inks spread on size (noise warped by noise, twice).
fn marble(x: f32, y: f32, base: [f32; 3], seed: u32) -> [f32; 3] {
    let (px, py) = (x * 0.022, y * 0.022);
    let q = (fbm(px, py, 4, seed), fbm(px + 5.2, py + 1.3, 4, seed + 1));
    let r = (fbm(px + 4.0 * q.0 + 1.7, py + 4.0 * q.1 + 9.2, 4, seed + 2), fbm(px + 4.0 * q.0 + 8.3, py + 4.0 * q.1 + 2.8, 4, seed + 3));
    let f = fbm(px + 4.0 * r.0, py + 4.0 * r.1, 5, seed + 4);
    let dark = base.map(|c| c * 0.7);
    let light = base.map(|c| c + (1.0 - c) * 0.34);
    let pale = [0.86, 0.83, 0.78];
    let mix = |a: [f32; 3], b: [f32; 3], k: f32| [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
    let smooth = |e0: f32, e1: f32, v: f32| {
        let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // the inks by where the swirl lands, then thin pale veins where they meet
    let mut c = mix(dark, base, smooth(0.32, 0.52, f));
    c = mix(c, light, smooth(0.58, 0.74, f) * (0.6 + 0.4 * r.1));
    let vein = (1.0 - ((f - 0.53).abs() * 55.0).min(1.0)) * smooth(0.3, 0.6, q.1) + (1.0 - ((f - 0.71).abs() * 80.0).min(1.0)) * 0.6;
    mix(c, pale, (vein * 0.7).min(0.7))
}

fn srgb_to_lin(c: u8) -> f32 {
    let s = c as f32 / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

fn lin_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// A cover `w_mm`×`h_mm` in the colour `base`, `PX_MM` pixels a mm.
pub fn render(kind: &str, base: [u8; 3], w_mm: f32, h_mm: f32) -> RgbaImage {
    let seed = base[0] as u32 * 65536 + base[1] as u32 * 256 + base[2] as u32 + kind.len() as u32 * 7919;
    let (w, h) = ((w_mm * PX_MM).round().max(1.0) as u32, (h_mm * PX_MM).round().max(1.0) as u32);
    let lin = [srgb_to_lin(base[0]), srgb_to_lin(base[1]), srgb_to_lin(base[2])];
    let step = 1.0 / PX_MM;
    // how deep the relief is, in mm, per material
    let depth = match kind {
        "linho" => 0.10,
        "tecido" => 0.22,
        "kraft" => 0.08,
        "couro" => 0.18,
        "cortica" => 0.22,
        "pontos" => 0.25,
        _ => 0.03,
    };
    // the light: from the top left, rather low, as across a desk
    let (lx, ly, lz) = (-0.5f32, -0.62f32, 0.6f32);
    let ln = (lx * lx + ly * ly + lz * lz).sqrt();
    let (lx, ly, lz) = (lx / ln, ly / ln, lz / ln);
    let mut img = RgbaImage::new(w, h);
    for py in 0..h {
        for px in 0..w {
            let (x, y) = (px as f32 * step, py as f32 * step);
            let mut c = if kind == "marmore" {
                marble(x, y, lin, seed)
            } else {
                // the slope of the height, lit from the top left
                let (hc, tone) = surface(kind, x, y, seed);
                let (hx, _) = surface(kind, x + step, y, seed);
                let (hy, _) = surface(kind, x, y + step, seed);
                let (gx, gy) = ((hx - hc) / step * depth, (hy - hc) / step * depth);
                let nl = (gx * gx + gy * gy + 1.0).sqrt();
                // Lambert against a flat board's light, so flat stays the colour
                let shade = (-gx * lx - gy * ly + lz) / nl / lz - 1.0;
                // and the hollows a little darker (where dust and shadow sit)
                let k = 1.0 + shade.clamp(-0.35, 0.35) * 0.55 + tone + (hc - 0.5) * depth * 0.6;
                lin.map(|v| v * k)
            };
            // the board: a broad light from the top left, darker edges
            let (u, v) = (x / w_mm, y / h_mm);
            let light = 1.0 + 0.05 * (0.5 - (u * 0.6 + v * 0.4));
            let edge = (u.min(1.0 - u) * w_mm).min(v.min(1.0 - v) * h_mm);
            let rim = 1.0 - 0.10 * (1.0 - (edge / 3.0).min(1.0)).powi(2);
            c = c.map(|v| v * light * rim);
            img.put_pixel(px, py, Rgba([lin_to_srgb(c[0]), lin_to_srgb(c[1]), lin_to_srgb(c[2]), 255]));
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_vary_repeat_and_keep_the_colour() {
        for (k, _) in TEXTURES {
            let a = render(k, [60, 80, 120], 20.0, 20.0);
            let b = render(k, [60, 80, 120], 20.0, 20.0);
            assert_eq!(a, b, "{k}");
            assert_eq!(a.dimensions(), (120, 120));
            let first = a.get_pixel(40, 40);
            assert!(a.pixels().any(|p| p != first), "{k} is flat");
            // the mean stays near the cover's colour (marbling aside)
            if *k != "marmore" {
                let n = (a.width() * a.height()) as f32;
                let mean: f32 = a.pixels().map(|p| p[2] as f32).sum::<f32>() / n;
                assert!((mean - 120.0).abs() < 18.0, "{k}: {mean}");
            }
        }
    }
}
