//! Tracing: a picture under (or over) the drawing, shown the way that helps
//! the most — photo, greyscale, lines (Sobel on a slightly blurred
//! luminance), a few flat values, or light non-photo blue.

use image::RgbaImage;
use serde_json::Value;

fn luminance(img: &RgbaImage) -> Vec<f32> {
    img.pixels()
        .map(|p| {
            // transparent parts count as white paper
            let a = p[3] as f32 / 255.0;
            ((0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0) * a + (1.0 - a)
        })
        .collect()
}

fn box_blur(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; src.len()];
    let mut out = vec![0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let l = if x > 0 { src[i - 1] } else { src[i] };
            let r = if x + 1 < w { src[i + 1] } else { src[i] };
            tmp[i] = (l + src[i] + r) / 3.0;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let u = if y > 0 { tmp[i - w] } else { tmp[i] };
            let d = if y + 1 < h { tmp[i + w] } else { tmp[i] };
            out[i] = (u + tmp[i] + d) / 3.0;
        }
    }
    out
}

/// Turn a picture into its trace form, in place.
pub fn apply(img: &mut RgbaImage, settings: &Value) {
    let mode = settings["mode"].as_str().unwrap_or("photo");
    if mode == "photo" {
        return;
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let lum = luminance(img);
    match mode {
        "grey" => {
            for (p, l) in img.pixels_mut().zip(&lum) {
                let v = (l * 255.0) as u8;
                *p = image::Rgba([v, v, v, 255]);
            }
        }
        "values" => {
            let n = settings["levels"].as_f64().unwrap_or(3.0).round().clamp(2.0, 6.0) as usize;
            for (p, l) in img.pixels_mut().zip(&lum) {
                let step = ((l * n as f32).floor() as usize).min(n - 1);
                let v = (step as f32 / (n - 1) as f32 * 225.0 + 20.0) as u8;
                *p = image::Rgba([v, v, v, 255]);
            }
        }
        "blue" => {
            for (p, l) in img.pixels_mut().zip(&lum) {
                let d = 1.0 - l;
                *p = image::Rgba([90, 169, 230, (d * 1.15 * 255.0).min(255.0) as u8]);
            }
        }
        "lines" => {
            let blur = box_blur(&lum, w, h);
            let mut mag = vec![0f32; w * h];
            let mut max = 0f32;
            for y in 1..h.saturating_sub(1) {
                for x in 1..w.saturating_sub(1) {
                    let i = y * w + x;
                    let gx = -blur[i - w - 1] - 2.0 * blur[i - 1] - blur[i + w - 1] + blur[i - w + 1] + 2.0 * blur[i + 1] + blur[i + w + 1];
                    let gy = -blur[i - w - 1] - 2.0 * blur[i - w] - blur[i - w + 1] + blur[i + w - 1] + 2.0 * blur[i + w] + blur[i + w + 1];
                    let m = (gx * gx + gy * gy).sqrt();
                    mag[i] = m;
                    max = max.max(m);
                }
            }
            let t = settings["threshold"].as_f64().unwrap_or(0.5).clamp(0.02, 0.98) as f32;
            let lo = max * (0.42 - t * 0.38);
            let hi = lo * 2.2 + 1e-6;
            for (p, m) in img.pixels_mut().zip(&mag) {
                let a = ((m - lo) / (hi - lo)).clamp(0.0, 1.0);
                *p = image::Rgba([35, 34, 31, (a * 255.0) as u8]);
            }
        }
        _ => {}
    }
}
