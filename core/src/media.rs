//! PDF pages and video frames for the notes that embed them.
//!
//! PDF pages are drawn by hayro (pure Rust). Videos go through `ffmpeg`
//! when it is installed: their size and length, a poster frame, and a
//! stream of frames (and of sound, as raw samples) read from its output
//! while the video plays in the note.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// The number of pages of a PDF.
pub fn pdf_pages(bytes: &[u8]) -> Result<usize, String> {
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map_err(|e| format!("{e:?}"))?;
    Ok(pdf.pages().len())
}

/// One page of a PDF drawn `width` pixels wide on white: (w, h, RGBA).
pub fn pdf_page(bytes: &[u8], page: usize, width: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map_err(|e| format!("{e:?}"))?;
    let pages = pdf.pages();
    let p = pages.get(page).ok_or("página fora do documento")?;
    let (pw, _) = p.render_dimensions();
    let scale = (width.max(1) as f32 / pw.max(1.0)).min(8.0);
    let cache = hayro::RenderCache::new();
    let pix = hayro::render(
        p,
        &cache,
        &hayro::hayro_interpret::InterpreterSettings::default(),
        &hayro::RenderSettings::default(),
        &hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE },
    );
    let (w, h) = (pix.width() as u32, pix.height() as u32);
    Ok((w, h, pix.data_as_u8_slice().to_vec()))
}

// ── video ─────────────────────────────────────────────────────────────────

/// What `ffprobe` tells of a video: its picture size and length (seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub seconds: f64,
    pub fps: f64,
    pub audio: bool,
}

fn ffprobe() -> Option<std::path::PathBuf> {
    let ff = crate::board::export::ffmpeg_path()?;
    let p = ff.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    p.is_file().then_some(p)
}

pub fn video_info(path: &Path) -> Option<VideoInfo> {
    let o = Command::new(ffprobe()?).args(["-v", "error", "-show_entries", "stream=codec_type,width,height,r_frame_rate:format=duration", "-of", "default=nw=1"]).arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&o.stdout);
    let mut info = VideoInfo { width: 0, height: 0, seconds: 0.0, fps: 24.0, audio: false };
    let mut video = false;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "codec_type" => {
                video = v == "video";
                if v == "audio" {
                    info.audio = true;
                }
            }
            "width" if video && info.width == 0 => info.width = v.parse().unwrap_or(0),
            "height" if video && info.height == 0 => info.height = v.parse().unwrap_or(0),
            "r_frame_rate" if video => {
                if let Some((a, b)) = v.split_once('/') {
                    let (a, b): (f64, f64) = (a.parse().unwrap_or(24.0), b.parse().unwrap_or(1.0));
                    if a > 0.0 && b > 0.0 {
                        info.fps = (a / b).clamp(1.0, 120.0);
                    }
                }
            }
            "duration" => info.seconds = v.parse().unwrap_or(0.0),
            _ => {}
        }
    }
    (info.width > 0 && info.height > 0).then_some(info)
}

/// The size a video is decoded at to show `width` pixels wide (even sides).
pub fn fit(info: &VideoInfo, width: u32) -> (u32, u32) {
    let w = width.min(info.width).max(2) & !1;
    let h = ((info.height as f64 * w as f64 / info.width as f64).round() as u32).max(2) & !1;
    (w, h)
}

/// One frame at `at` seconds, `size` pixels: RGBA.
pub fn video_frame(path: &Path, at: f64, size: (u32, u32)) -> Option<Vec<u8>> {
    let ff = crate::board::export::ffmpeg_path()?;
    let o = Command::new(ff)
        .args(["-v", "error", "-ss", &format!("{at:.3}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={}:{}", size.0, size.1), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    (o.stdout.len() == (size.0 * size.1 * 4) as usize).then_some(o.stdout)
}

/// A video decoding from `at` seconds: frames of `size` pixels at the
/// video's rate, read one by one.
pub struct VideoStream {
    child: Child,
    pub size: (u32, u32),
}

impl VideoStream {
    pub fn open(path: &Path, at: f64, size: (u32, u32), fps: f64) -> Option<VideoStream> {
        let ff = crate::board::export::ffmpeg_path()?;
        let child = Command::new(ff)
            .args(["-v", "error", "-ss", &format!("{at:.3}"), "-i"])
            .arg(path)
            .args(["-an", "-vf", &format!("scale={}:{},fps={fps:.3}", size.0, size.1), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(VideoStream { child, size })
    }

    /// The next frame, or `None` at the end.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; (self.size.0 * self.size.1 * 4) as usize];
        self.child.stdout.as_mut()?.read_exact(&mut buf).ok()?;
        Some(buf)
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A video's sound from `at` seconds as 16-bit stereo samples at 44.1 kHz.
pub struct AudioStream {
    child: Child,
}

pub const AUDIO_RATE: u32 = 44100;

impl AudioStream {
    pub fn open(path: &Path, at: f64) -> Option<AudioStream> {
        let ff = crate::board::export::ffmpeg_path()?;
        let child = Command::new(ff)
            .args(["-v", "error", "-ss", &format!("{at:.3}"), "-i"])
            .arg(path)
            .args(["-vn", "-ac", "2", "-ar", &AUDIO_RATE.to_string(), "-f", "s16le", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(AudioStream { child })
    }

    /// Up to `n` samples (fewer at the end).
    pub fn read(&mut self, n: usize) -> Vec<i16> {
        let mut bytes = vec![0u8; n * 2];
        let Some(out) = self.child.stdout.as_mut() else { return Vec::new() };
        let mut got = 0;
        while got < bytes.len() {
            match out.read(&mut bytes[got..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => got += k,
            }
        }
        bytes.truncate(got - got % 2);
        bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with a red square, written with pdf-writer.
    fn sample_pdf() -> Vec<u8> {
        use pdf_writer::{Content, Pdf, Rect, Ref};
        let mut pdf = Pdf::new();
        let (catalog, tree, page, content) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
        pdf.catalog(catalog).pages(tree);
        pdf.pages(tree).kids([page]).count(1);
        pdf.page(page).parent(tree).media_box(Rect::new(0.0, 0.0, 200.0, 100.0)).contents(content);
        let mut c = Content::new();
        c.set_fill_rgb(1.0, 0.0, 0.0);
        c.rect(50.0, 25.0, 100.0, 50.0);
        c.fill_nonzero();
        pdf.stream(content, &c.finish());
        pdf.finish()
    }

    #[test]
    fn pdf_pages_are_drawn() {
        let bytes = sample_pdf();
        assert_eq!(pdf_pages(&bytes).unwrap(), 1);
        let (w, h, px) = pdf_page(&bytes, 0, 400).unwrap();
        assert_eq!((w, h), (400, 200));
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        assert_eq!(at(200, 100), [255, 0, 0, 255], "the square, in the middle");
        assert_eq!(at(10, 10), [255, 255, 255, 255], "white paper around it");
        assert!(pdf_page(&bytes, 3, 400).is_err());
        assert!(pdf_pages(b"not a pdf").is_err());
    }

    /// With ffmpeg installed: a test video is probed, a frame read, and streamed.
    #[test]
    fn video_through_ffmpeg() {
        let Some(ff) = crate::board::export::ffmpeg_path() else { return };
        let dir = std::env::temp_dir().join(format!("ateliet-media-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let v = dir.join("v.mp4");
        let ok = Command::new(ff).args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x90:rate=10:duration=2", "-f", "lavfi", "-i", "sine=duration=2", "-shortest", "-pix_fmt", "yuv420p"]).arg(&v).status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            return;
        }
        let info = video_info(&v).unwrap();
        assert_eq!((info.width, info.height), (160, 90));
        assert!((info.seconds - 2.0).abs() < 0.2 && info.audio);
        let size = fit(&info, 80);
        assert_eq!(size, (80, 44));
        assert_eq!(video_frame(&v, 1.0, size).unwrap().len(), 80 * 44 * 4);
        let mut s = VideoStream::open(&v, 0.5, size, info.fps).unwrap();
        let mut n = 0;
        while s.next_frame().is_some() {
            n += 1;
        }
        assert!((14..=16).contains(&n), "{n} frames for 1.5 s at 10 fps");
        let mut a = AudioStream::open(&v, 1.0).unwrap();
        assert_eq!(a.read(2000).len(), 2000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
