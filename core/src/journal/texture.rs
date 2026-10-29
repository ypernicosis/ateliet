//! Cover textures, made from the cover's colour: no pictures shipped, the
//! same noise every time for the same texture and colour.

use image::{Rgba, RgbaImage};

/// The textures a cover can have: key (as written in `capa-textura`) and
/// the name shown.
pub const TEXTURES: &[(&str, &str)] = &[("", "Lisa"), ("linho", "Linho"), ("tecido", "Tecido"), ("kraft", "Papel kraft"), ("couro", "Couro"), ("marmore", "Marmorizada"), ("cortica", "Cortiça"), ("pontos", "Pontilhada")];

/// A hash of a few numbers, in 0..1.
fn hash(a: u32, b: u32, c: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b) ^ c.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x & 0xffff) as f32 / 65535.0
}

/// Smooth value noise at a scale of `s` px.
fn noise(x: f32, y: f32, s: f32, seed: u32) -> f32 {
    let (fx, fy) = (x / s, y / s);
    let (ix, iy) = (fx.floor() as i32 as u32, fy.floor() as i32 as u32);
    let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash(ix, iy, seed);
    let b = hash(ix + 1, iy, seed);
    let c = hash(ix, iy + 1, seed);
    let d = hash(ix + 1, iy + 1, seed);
    (a + (b - a) * tx) + ((c + (d - c) * tx) - (a + (b - a) * tx)) * ty
}

fn fbm(x: f32, y: f32, s: f32, seed: u32) -> f32 {
    let mut v = 0.0;
    let mut amp = 0.5;
    let mut sc = s;
    for o in 0..4 {
        v += amp * noise(x, y, sc, seed + o);
        amp *= 0.5;
        sc *= 0.5;
    }
    v / 0.9375
}

/// A texture `w`×`h` in the colour `base`: each pixel is the colour made
/// lighter or darker by the texture's pattern.
pub fn render(kind: &str, base: [u8; 3], w: u32, h: u32) -> RgbaImage {
    let seed = base[0] as u32 * 65536 + base[1] as u32 * 256 + base[2] as u32;
    let shade = |v: f32| -> f32 { v.clamp(-1.0, 1.0) };
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32, y as f32);
            let d = match kind {
                // threads both ways, uneven
                "linho" => {
                    let a = ((fx * 0.9).sin() * 0.5 + 0.5) * (0.6 + 0.4 * noise(fx, fy, 40.0, seed));
                    let b = ((fy * 0.9).sin() * 0.5 + 0.5) * (0.6 + 0.4 * noise(fy, fx, 40.0, seed + 7));
                    (a * 0.5 + b * 0.5 - 0.5) * 0.22 + (hash(x, y, seed) - 0.5) * 0.06
                }
                // a weave: over and under in a checker of threads
                "tecido" => {
                    let cell = 6.0;
                    let (cx, cy) = ((fx / cell).floor() as i32, (fy / cell).floor() as i32);
                    let over = (cx + cy).rem_euclid(2) == 0;
                    let (u, v) = (fx / cell - cx as f32, fy / cell - cy as f32);
                    let t = if over { (u * std::f32::consts::PI).sin() } else { (v * std::f32::consts::PI).sin() };
                    (t - 0.6) * 0.16 + (hash(x, y, seed) - 0.5) * 0.05
                }
                // fibres and specks
                "kraft" => {
                    let fib = noise(fx * 0.3, fy * 4.0, 6.0, seed) - 0.5;
                    let speck = if hash(x, y, seed + 3) > 0.995 { -0.25 } else { 0.0 };
                    fib * 0.14 + (fbm(fx, fy, 60.0, seed) - 0.5) * 0.12 + speck
                }
                // grain and creases
                "couro" => {
                    let cells = (fbm(fx, fy, 9.0, seed) - 0.5) * 0.22;
                    let crease = (1.0 - ((fbm(fx, fy, 70.0, seed + 5) - 0.5).abs() * 30.0).min(1.0)) * -0.12;
                    cells + crease
                }
                // veins pulled through
                "marmore" => {
                    let t = fy * 0.025 + fbm(fx, fy, 90.0, seed) * 6.0;
                    let vein = 1.0 - (t.sin().abs()).powf(0.18);
                    (fbm(fx, fy, 140.0, seed + 9) - 0.5) * 0.18 + vein * 0.35
                }
                // granules
                "cortica" => (fbm(fx, fy, 5.0, seed) - 0.5) * 0.45 + if hash(x, y, seed + 1) > 0.97 { -0.2 } else { 0.0 },
                "pontos" => {
                    let cell = 14.0;
                    let (u, v) = (fx.rem_euclid(cell) - cell / 2.0, fy.rem_euclid(cell) - cell / 2.0);
                    if u * u + v * v < 3.2 { -0.18 } else { 0.0 }
                }
                _ => (hash(x, y, seed) - 0.5) * 0.02,
            };
            let d = shade(d);
            let px = |c: u8| -> u8 {
                let c = c as f32;
                (if d >= 0.0 { c + (255.0 - c) * d } else { c * (1.0 + d) }).round().clamp(0.0, 255.0) as u8
            };
            img.put_pixel(x, y, Rgba([px(base[0]), px(base[1]), px(base[2]), 255]));
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_vary_and_repeat() {
        for (k, _) in TEXTURES {
            let a = render(k, [60, 80, 120], 64, 64);
            let b = render(k, [60, 80, 120], 64, 64);
            assert_eq!(a, b, "{k}");
            if !k.is_empty() {
                let first = a.get_pixel(0, 0);
                assert!(a.pixels().any(|p| p != first), "{k} is flat");
            }
        }
    }
}
