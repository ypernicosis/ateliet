//! Pixels: 512×512 tiles in premultiplied RGBA8, and the dabs brushes put
//! on them.

pub const TILE: usize = 512;

#[derive(Clone)]
pub struct Tile {
    /// Premultiplied RGBA, row-major, TILE×TILE.
    pub px: Vec<u8>,
}

impl Default for Tile {
    fn default() -> Self {
        Tile { px: vec![0; TILE * TILE * 4] }
    }
}

#[inline]
pub fn mul255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

impl Tile {
    pub fn is_empty(&self) -> bool {
        self.px.chunks_exact(4).all(|p| p[3] == 0)
    }

    /// From a PNG (straight alpha, as canvases save it).
    pub fn from_png(bytes: &[u8]) -> Option<Tile> {
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        let mut t = Tile::default();
        let (w, h) = (img.width().min(TILE as u32) as usize, img.height().min(TILE as u32) as usize);
        let raw = img.as_raw();
        for y in 0..h {
            for x in 0..w {
                let s = (y * img.width() as usize + x) * 4;
                let d = (y * TILE + x) * 4;
                let a = raw[s + 3] as u32;
                t.px[d] = mul255(raw[s] as u32, a) as u8;
                t.px[d + 1] = mul255(raw[s + 1] as u32, a) as u8;
                t.px[d + 2] = mul255(raw[s + 2] as u32, a) as u8;
                t.px[d + 3] = a as u8;
            }
        }
        Some(t)
    }

    /// Straight-alpha RGBA (for PNG and for display APIs that want it).
    pub fn straight(&self) -> Vec<u8> {
        unpremultiply(&self.px)
    }

    pub fn to_png(&self) -> Vec<u8> {
        encode_png(&self.straight(), TILE as u32, TILE as u32)
    }

    /// Paint a round dab centred at (`cx`, `cy`) (tile coordinates).
    #[allow(clippy::too_many_arguments)]
    pub fn dab(&mut self, cx: f32, cy: f32, radius: f32, hardness: f32, color: [u8; 3], alpha: f32, mode: DabMode) {
        let r = radius.max(0.5);
        let x0 = ((cx - r).floor().max(0.0)) as usize;
        let y0 = ((cy - r).floor().max(0.0)) as usize;
        let x1 = ((cx + r).ceil().min(TILE as f32 - 1.0)).max(0.0) as usize;
        let y1 = ((cy + r).ceil().min(TILE as f32 - 1.0)).max(0.0) as usize;
        if cx + r < 0.0 || cy + r < 0.0 || cx - r >= TILE as f32 || cy - r >= TILE as f32 {
            return;
        }
        let hard = hardness.clamp(0.0, 1.0);
        let inner = r * hard;
        let fall = (r - inner).max(0.75);
        for y in y0..=y1 {
            let dy = y as f32 + 0.5 - cy;
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let d = (dx * dx + dy * dy).sqrt();
                // antialiased edge: coverage from the distance to the rim
                let cov = if d <= inner { 1.0 } else { (1.0 - (d - inner) / fall).clamp(0.0, 1.0) };
                let cov = if hard >= 0.99 { (r + 0.5 - d).clamp(0.0, 1.0) } else { cov * cov * (3.0 - 2.0 * cov) };
                if cov <= 0.0 {
                    continue;
                }
                let a = (alpha * cov).clamp(0.0, 1.0);
                let i = (y * TILE + x) * 4;
                let p = &mut self.px[i..i + 4];
                match mode {
                    DabMode::Over => {
                        let sa = (a * 255.0) as u32;
                        let inv = 255 - sa;
                        p[0] = (mul255(color[0] as u32, sa) + mul255(p[0] as u32, inv)) as u8;
                        p[1] = (mul255(color[1] as u32, sa) + mul255(p[1] as u32, inv)) as u8;
                        p[2] = (mul255(color[2] as u32, sa) + mul255(p[2] as u32, inv)) as u8;
                        p[3] = (sa + mul255(p[3] as u32, inv)) as u8;
                    }
                    DabMode::Max => {
                        // no build-up inside one stroke (markers, stroke buffers)
                        let sa = (a * 255.0) as u32;
                        if sa > p[3] as u32 {
                            p[0] = mul255(color[0] as u32, sa) as u8;
                            p[1] = mul255(color[1] as u32, sa) as u8;
                            p[2] = mul255(color[2] as u32, sa) as u8;
                            p[3] = sa as u8;
                        }
                    }
                    DabMode::Erase => {
                        let keep = 255 - (a * 255.0) as u32;
                        for c in p.iter_mut() {
                            *c = mul255(*c as u32, keep) as u8;
                        }
                    }
                }
            }
        }
    }

    /// Put `src` over this tile with an opacity (`erase`: remove where `src` has alpha).
    pub fn merge(&mut self, src: &Tile, opacity: f32, erase: bool, alpha_lock: bool) {
        let o = (opacity.clamp(0.0, 1.0) * 255.0) as u32;
        for (d, s) in self.px.chunks_exact_mut(4).zip(src.px.chunks_exact(4)) {
            let sa = mul255(s[3] as u32, o);
            if sa == 0 {
                continue;
            }
            if erase {
                let keep = 255 - sa;
                for c in d.iter_mut() {
                    *c = mul255(*c as u32, keep) as u8;
                }
                continue;
            }
            let inv = 255 - sa;
            let da = d[3] as u32;
            for k in 0..3 {
                d[k] = (mul255(s[k] as u32, o) + mul255(d[k] as u32, inv)) as u8;
            }
            if alpha_lock {
                // colour changes, coverage stays: rescale to the old alpha
                let na = sa + mul255(da, inv);
                if na > 0 {
                    for k in 0..3 {
                        d[k] = ((d[k] as u32 * da) / na).min(255) as u8;
                    }
                }
                d[3] = da as u8;
            } else {
                d[3] = (sa + mul255(da, inv)) as u8;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DabMode {
    Over,
    Max,
    Erase,
}

pub fn unpremultiply(px: &[u8]) -> Vec<u8> {
    let mut out = px.to_vec();
    for p in out.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in p.iter_mut().take(3) {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

pub fn encode_png(straight: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive);
    let _ = image::ImageEncoder::write_image(enc, straight, w, h, image::ExtendedColorType::Rgba8);
    out
}

pub fn parse_color(s: &str) -> [u8; 3] {
    let h = s.trim().trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    if h.len() >= 6 {
        [p(0), p(2), p(4)]
    } else if h.len() == 3 {
        let q = |i: usize| u8::from_str_radix(&h[i..i + 1].repeat(2), 16).unwrap_or(0);
        [q(0), q(1), q(2)]
    } else {
        [0, 0, 0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dabs_and_png() {
        let mut t = Tile::default();
        assert!(t.is_empty());
        t.dab(100.0, 100.0, 10.0, 1.0, [255, 0, 0], 1.0, DabMode::Over);
        let i = (100 * TILE + 100) * 4;
        assert_eq!(&t.px[i..i + 4], &[255, 0, 0, 255]);
        let back = Tile::from_png(&t.to_png()).unwrap();
        assert_eq!(&back.px[i..i + 4], &[255, 0, 0, 255]);
        t.dab(100.0, 100.0, 4.0, 1.0, [0, 0, 0], 1.0, DabMode::Erase);
        assert_eq!(t.px[i + 3], 0);
        assert_eq!(parse_color("#23221f"), [0x23, 0x22, 0x1f]);
    }
}
