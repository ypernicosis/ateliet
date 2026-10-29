//! Cutting a picture out as it shows on the page (its crop, its mirror and
//! its mask), into a picture with transparency: what the sticker drawer
//! keeps, so a cut-out can be used again.

use super::{Crop, Mask};
use image::{imageops, Rgba, RgbaImage};

/// Coverage (0..1) of a closed polygon over a `w`×`h` grid of pixels, with
/// 4×4 samples a pixel (even-odd rule).
pub fn coverage(poly: &[(f64, f64)], w: u32, h: u32) -> Vec<f32> {
    const S: u32 = 4;
    let mut cov = vec![0f32; (w * h) as usize];
    if poly.len() < 3 {
        return cov;
    }
    let mut xs: Vec<f64> = Vec::new();
    for sy in 0..h * S {
        let y = (sy as f64 + 0.5) / S as f64;
        xs.clear();
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.1 <= y) != (b.1 <= y) {
                xs.push(a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        let row = (sy / S) as usize;
        for pair in xs.chunks(2) {
            let [x0, x1] = pair else { continue };
            let s0 = ((x0 * S as f64) - 0.5).ceil().max(0.0) as i64;
            let s1 = ((x1 * S as f64) - 0.5).floor().min((w * S) as f64 - 1.0) as i64;
            for sx in s0..=s1 {
                cov[row * w as usize + (sx as u32 / S) as usize] += 1.0 / (S * S) as f32;
            }
        }
    }
    cov
}

/// The picture of an item cut out: `max_side` bounds the result.
pub fn cut(img: &RgbaImage, crop: Option<Crop>, flip: bool, mask: Option<&Mask>, max_side: u32) -> RgbaImage {
    let (iw, ih) = (img.width().max(1), img.height().max(1));
    let c = crop.unwrap_or(Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
    let x0 = ((c.x * iw as f64).round() as u32).min(iw - 1);
    let y0 = ((c.y * ih as f64).round() as u32).min(ih - 1);
    let cw = ((c.w * iw as f64).round() as u32).clamp(1, iw - x0);
    let ch = ((c.h * ih as f64).round() as u32).clamp(1, ih - y0);
    let mut pic = imageops::crop_imm(img, x0, y0, cw, ch).to_image();
    if flip {
        imageops::flip_horizontal_in_place(&mut pic);
    }
    let k = (max_side as f64 / cw.max(ch) as f64).min(1.0);
    if k < 1.0 {
        pic = imageops::resize(&pic, ((cw as f64 * k).round() as u32).max(1), ((ch as f64 * k).round() as u32).max(1), imageops::FilterType::Triangle);
    }
    let Some(m) = mask else { return pic };
    let (w, h) = pic.dimensions();
    // a polaroid: the picture inside a white frame
    if m.shape == "polaroide" {
        let (wx, wy, ww, wh) = m.window();
        let fw = (w as f64 / ww).round() as u32;
        let fh = (h as f64 / wh).round() as u32;
        let mut out = RgbaImage::from_pixel(fw, fh, Rgba([250, 249, 246, 255]));
        imageops::overlay(&mut out, &pic, (wx * fw as f64).round() as i64, (wy * fh as f64).round() as i64);
        return out;
    }
    let poly: Vec<(f64, f64)> = m.outline(96).iter().map(|p| (p.0 * w as f64, p.1 * h as f64)).collect();
    let cov = coverage(&poly, w, h);
    for (i, px) in pic.pixels_mut().enumerate() {
        px[3] = (px[3] as f32 * cov[i]).round() as u8;
    }
    pic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_cut_is_clear_at_the_corners() {
        let img = RgbaImage::from_pixel(100, 60, Rgba([200, 10, 10, 255]));
        let m = Mask { shape: "circulo".into(), pts: vec![] };
        let out = cut(&img, Some(Crop { x: 0.2, y: 0.0, w: 0.6, h: 1.0 }), true, Some(&m), 1000);
        assert_eq!(out.dimensions(), (60, 60));
        assert_eq!(out.get_pixel(0, 0)[3], 0);
        assert_eq!(out.get_pixel(30, 30)[3], 255);
        let full = coverage(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], 10, 10);
        assert!(full.iter().all(|c| (*c - 1.0).abs() < 1e-6));
        let p = cut(&img, None, false, Some(&Mask { shape: "polaroide".into(), pts: vec![] }), 1000);
        assert!(p.width() > 100 && p.height() > 60);
    }
}
