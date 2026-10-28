//! Audio, video and PDF embedded in a note (`![[file]]`), shown in place:
//! a player for audio, a player for video (through ffmpeg, when installed),
//! and a page viewer for PDF. Their state lives here, one per app, since
//! the reading view is drawn again every frame.

use crate::theme::Palette;
use crate::{icons, kit};
use ateliet_core::media;
use eframe::egui::{self, Color32, Sense, Stroke};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Audio,
    Video,
    Pdf,
}

pub fn kind(name: &str) -> Option<Kind> {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "mp3" | "wav" | "m4a" | "ogg" | "flac" | "opus" | "aac" => Some(Kind::Audio),
        "mp4" | "mov" | "mkv" | "webm" | "ogv" | "avi" | "m4v" => Some(Kind::Video),
        "pdf" => Some(Kind::Pdf),
        _ => None,
    }
}

/// Sound out of the app, for the embedded players.
#[cfg(feature = "audio")]
struct Out {
    stream: rodio::OutputStream,
}

/// The audio player of one embed.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
struct AudioPlayer {
    path: PathBuf,
    #[cfg(feature = "audio")]
    sink: rodio::Sink,
    dur: f64,
}

/// A video being shown: its info, the frame on screen, and while it plays
/// the decoder's frames and the clock.
struct VideoPlayer {
    path: PathBuf,
    info: media::VideoInfo,
    size: (u32, u32),
    tex: Option<egui::TextureHandle>,
    frames: Option<Receiver<Vec<u8>>>,
    /// Where it started playing (in the video) and when.
    from: f64,
    started: Option<Instant>,
    /// The position while paused.
    at: f64,
    #[cfg(feature = "audio")]
    sink: Option<rodio::Sink>,
    shown: f64,
}

/// A PDF's pages: their count, and the pages drawn (page, width) → texture.
struct PdfDoc {
    pages: Option<Result<usize, String>>,
    page: usize,
    tex: HashMap<(usize, u32), Option<egui::TextureHandle>>,
    loading: Option<(usize, u32, Receiver<Result<(u32, u32, Vec<u8>), String>>)>,
    count_rx: Option<Receiver<Result<usize, String>>>,
}

#[derive(Default)]
struct State {
    #[cfg(feature = "audio")]
    out: Option<Out>,
    audio: Option<AudioPlayer>,
    video: Option<VideoPlayer>,
    /// Posters and info of videos, read in the background.
    posters: HashMap<PathBuf, Option<(media::VideoInfo, (u32, u32), Vec<u8>)>>,
    poster_rx: HashMap<PathBuf, Receiver<Option<(media::VideoInfo, (u32, u32), Vec<u8>)>>>,
    poster_tex: HashMap<PathBuf, egui::TextureHandle>,
    pdfs: HashMap<PathBuf, PdfDoc>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A seek bar: the part played, and a click or drag that asks for a time.
fn seek_bar(ui: &mut egui::Ui, pos: f64, dur: f64, p: Palette) -> Option<f64> {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(40.0), 18.0), Sense::click_and_drag());
    let bar = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
    ui.painter().rect_filled(bar, 2.0, p.border);
    let f = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
    ui.painter().rect_filled(egui::Rect::from_min_max(bar.min, egui::pos2(bar.left() + bar.width() * f, bar.bottom())), 2.0, p.accent);
    if r.hovered() || r.dragged() {
        ui.painter().circle_filled(egui::pos2(bar.left() + bar.width() * f, bar.center().y), 6.0, p.accent);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if (r.clicked() || r.drag_stopped()) && dur > 0.0 {
        let x = r.interact_pointer_pos()?.x;
        return Some((((x - bar.left()) / bar.width()).clamp(0.0, 1.0) as f64) * dur);
    }
    None
}

fn frame<R>(ui: &mut egui::Ui, p: Palette, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new().fill(p.bg_secondary).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    }).inner
}

/// The embed of `abs` (named `name`): true when it asks to open the file in
/// the system's app.
pub fn show(ui: &mut egui::Ui, abs: &Path, name: &str, k: Kind, page: Option<usize>, p: Palette) -> bool {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        match k {
            Kind::Audio => audio(ui, &mut s, abs, name, p),
            Kind::Video => video(ui, &mut s, abs, name, p),
            Kind::Pdf => pdf(ui, &mut s, abs, name, page, p),
        }
    })
}

// ── audio ─────────────────────────────────────────────────────────────────

#[cfg(feature = "audio")]
fn out(s: &mut State) -> Option<&rodio::OutputStream> {
    if s.out.is_none() {
        let mut stream = rodio::OutputStreamBuilder::open_default_stream().ok()?;
        stream.log_on_drop(false);
        s.out = Some(Out { stream });
    }
    s.out.as_ref().map(|o| &o.stream)
}

/// Stop whatever plays in the notes (one player at a time).
fn stop_all(s: &mut State) {
    s.audio = None;
    if let Some(v) = &mut s.video {
        let at = video_pos(v);
        pause_video(v, at);
    }
}

#[cfg(feature = "audio")]
fn play_audio(s: &mut State, abs: &Path) -> Result<(), String> {
    use rodio::Source as _;
    stop_all(s);
    let stream = out(s).ok_or("sem saída de áudio")?;
    let f = std::fs::File::open(abs).map_err(|e| e.to_string())?;
    let dec = rodio::Decoder::try_from(f).map_err(|e| format!("não consegui ler o áudio ({e})"))?;
    let mut dur = dec.total_duration().map(|d| d.as_secs_f64()).unwrap_or(0.0);
    if dur == 0.0 {
        dur = media::video_info(abs).map(|i| i.seconds).unwrap_or(0.0);
    }
    let sink = rodio::Sink::connect_new(stream.mixer());
    sink.append(dec);
    s.audio = Some(AudioPlayer { path: abs.to_path_buf(), sink, dur });
    Ok(())
}

fn audio(ui: &mut egui::Ui, s: &mut State, abs: &Path, name: &str, p: Palette) -> bool {
    let mut open = false;
    #[cfg(feature = "audio")]
    let mine = s.audio.as_ref().is_some_and(|a| a.path == abs);
    #[cfg(not(feature = "audio"))]
    let _ = (&s.audio, abs);
    frame(ui, p, |ui| {
        ui.horizontal(|ui| {
            // built without sound: the system's app plays it
            #[cfg(not(feature = "audio"))]
            if kit::icon_button(ui, icons::PLAY, "Tocar no app padrão", false, p).clicked() {
                open = true;
            }
            #[cfg(feature = "audio")]
            {
                let playing = mine && s.audio.as_ref().is_some_and(|a| !a.sink.is_paused() && !a.sink.empty());
                if kit::icon_button(ui, if playing { icons::PAUSE } else { icons::PLAY }, if playing { "Pausar" } else { "Tocar" }, false, p).clicked() {
                    match (&s.audio, mine) {
                        (Some(a), true) if !a.sink.empty() => {
                            if a.sink.is_paused() {
                                a.sink.play();
                            } else {
                                a.sink.pause();
                            }
                        }
                        _ => {
                            if let Err(e) = play_audio(s, abs) {
                                ui.ctx().debug_text(e);
                            }
                        }
                    }
                }
                if playing {
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
            }
            ui.label(egui::RichText::new(name).size(13.0).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", false, p).clicked() {
                    open = true;
                }
                #[cfg(feature = "audio")]
                if let Some(a) = s.audio.as_ref().filter(|_| mine) {
                    let pos = a.sink.get_pos().as_secs_f64();
                    ui.label(egui::RichText::new(format!("{} / {}", clock(pos), clock(a.dur))).size(12.0).color(p.muted));
                    if let Some(t) = seek_bar(ui, pos, a.dur, p) {
                        let _ = a.sink.try_seek(Duration::from_secs_f64(t));
                    }
                }
            });
        });
    });
    open
}

// ── video ─────────────────────────────────────────────────────────────────

/// A video's sound read from ffmpeg, as a source rodio plays.
#[cfg(feature = "audio")]
struct PcmSource {
    stream: media::AudioStream,
    buf: Vec<i16>,
    at: usize,
    done: bool,
}

#[cfg(feature = "audio")]
impl Iterator for PcmSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.at >= self.buf.len() {
            if self.done {
                return None;
            }
            self.buf = self.stream.read(8192);
            self.at = 0;
            if self.buf.is_empty() {
                self.done = true;
                return None;
            }
        }
        let v = self.buf[self.at] as f32 / 32768.0;
        self.at += 1;
        Some(v)
    }
}

#[cfg(feature = "audio")]
impl rodio::Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        2
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        media::AUDIO_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn video_pos(v: &VideoPlayer) -> f64 {
    match v.started {
        #[cfg(feature = "audio")]
        Some(_) if v.sink.is_some() => v.from + v.sink.as_ref().unwrap().get_pos().as_secs_f64(),
        Some(t) => v.from + t.elapsed().as_secs_f64(),
        None => v.at,
    }
}

fn pause_video(v: &mut VideoPlayer, at: f64) {
    v.frames = None;
    v.started = None;
    v.at = at;
    #[cfg(feature = "audio")]
    {
        v.sink = None;
    }
}

fn play_video(s: &mut State, at: f64, ctx: &egui::Context) {
    stop_all(s);
    #[cfg(feature = "audio")]
    let sink = {
        let has_audio = s.video.as_ref().is_some_and(|v| v.info.audio);
        let path = s.video.as_ref().map(|v| v.path.clone());
        match (has_audio, path) {
            (true, Some(path)) => out(s).and_then(|o| {
                let stream = media::AudioStream::open(&path, at)?;
                let sink = rodio::Sink::connect_new(o.mixer());
                sink.append(PcmSource { stream, buf: Vec::new(), at: 0, done: false });
                Some(sink)
            }),
            _ => None,
        }
    };
    let Some(v) = s.video.as_mut() else { return };
    let (tx, rx) = mpsc::sync_channel(4);
    let (path, size, fps, c) = (v.path.clone(), v.size, v.info.fps, ctx.clone());
    std::thread::spawn(move || {
        let Some(mut st) = media::VideoStream::open(&path, at, size, fps) else { return };
        while let Some(f) = st.next_frame() {
            if tx.send(f).is_err() {
                break;
            }
            c.request_repaint();
        }
    });
    v.frames = Some(rx);
    v.from = at;
    v.shown = at - 1.0;
    v.started = Some(Instant::now());
    #[cfg(feature = "audio")]
    {
        v.sink = sink;
    }
}

fn video(ui: &mut egui::Ui, s: &mut State, abs: &Path, name: &str, p: Palette) -> bool {
    let ctx = ui.ctx().clone();
    // the poster and the info, read once in the background
    if !s.posters.contains_key(abs) {
        if let Some(rx) = s.poster_rx.get(abs) {
            if let Ok(r) = rx.try_recv() {
                s.poster_rx.remove(abs);
                s.posters.insert(abs.to_path_buf(), r);
            }
        } else if ateliet_core::board::export::ffmpeg_path().is_some() {
            let (tx, rx) = mpsc::channel();
            let (path, c) = (abs.to_path_buf(), ctx.clone());
            std::thread::spawn(move || {
                let r = media::video_info(&path).and_then(|info| {
                    let size = media::fit(&info, 1280);
                    let px = media::video_frame(&path, (info.seconds * 0.1).min(2.0), size)?;
                    Some((info, size, px))
                });
                let _ = tx.send(r);
                c.request_repaint();
            });
            s.poster_rx.insert(abs.to_path_buf(), rx);
        } else {
            s.posters.insert(abs.to_path_buf(), None);
        }
    }
    let loaded = s.posters.get(abs).cloned();
    let Some(Some((info, size, poster))) = loaded else {
        // no ffmpeg, an unreadable video, or still reading: a card
        let mut open = false;
        frame(ui, p, |ui| {
            let reading = s.poster_rx.contains_key(abs);
            let r = kit::Row::new(name).icon(icons::FILM).trailing(if reading { "lendo…".to_string() } else if ateliet_core::board::export::ffmpeg_path().is_none() { "vídeo · abrir (instale o ffmpeg para ver aqui)".to_string() } else { "vídeo · abrir".to_string() }).show(ui, p);
            open = r.clicked();
        });
        return open;
    };
    if !s.poster_tex.contains_key(abs) {
        let img = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], &poster);
        s.poster_tex.insert(abs.to_path_buf(), ctx.load_texture(format!("poster:{}", abs.display()), img, egui::TextureOptions::LINEAR));
    }
    let is_current = s.video.as_ref().is_some_and(|v| v.path == abs);
    let mut open = false;
    let mut action: Option<Option<f64>> = None; // Some(None): play/pause; Some(Some(t)): seek
    let pos = s.video.as_ref().filter(|_| is_current).map(video_pos).unwrap_or(0.0);
    // a new frame due?
    if let Some(v) = s.video.as_mut().filter(|_| is_current) {
        if let Some(rx) = &v.frames {
            let want = video_pos(v);
            let mut latest = None;
            // frames behind the clock are skipped
            while v.shown < want - 0.5 / v.info.fps {
                match rx.try_recv() {
                    Ok(f) => {
                        latest = Some(f);
                        v.shown += 1.0 / v.info.fps;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let end = v.info.seconds;
                        pause_video(v, if want >= end - 0.5 { 0.0 } else { want });
                        break;
                    }
                }
            }
            if v.shown < v.from {
                v.shown = v.from;
            }
            if let Some(f) = latest {
                let img = egui::ColorImage::from_rgba_unmultiplied([v.size.0 as usize, v.size.1 as usize], &f);
                match &mut v.tex {
                    Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                    None => v.tex = Some(ctx.load_texture("video-frame", img, egui::TextureOptions::LINEAR)),
                }
            }
            ctx.request_repaint_after(Duration::from_millis((500.0 / v.info.fps) as u64));
        }
    }
    let playing = s.video.as_ref().is_some_and(|v| is_current && v.started.is_some());
    frame(ui, p, |ui| {
        let w = ui.available_width().min(info.width as f32 * 1.5);
        let h = w * info.height as f32 / info.width as f32;
        let tex = s.video.as_ref().filter(|v| is_current && v.tex.is_some()).and_then(|v| v.tex.as_ref().map(|t| t.id())).unwrap_or_else(|| s.poster_tex[abs].id());
        let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
        let img = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h));
        ui.painter().rect_filled(img, 4.0, Color32::BLACK);
        ui.painter().image(tex, img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        if !playing {
            let c = img.center();
            ui.painter().circle_filled(c, 26.0, Color32::from_black_alpha(140));
            ui.painter().text(c + egui::vec2(2.0, 0.0), egui::Align2::CENTER_CENTER, icons::PLAY, kit::icon_font(22.0), Color32::WHITE);
        }
        if r.clicked() {
            action = Some(None);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if kit::icon_button(ui, if playing { icons::PAUSE } else { icons::PLAY }, if playing { "Pausar" } else { "Tocar" }, false, p).clicked() {
                action = Some(None);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", false, p).clicked() {
                    open = true;
                }
                ui.label(egui::RichText::new(format!("{} / {}", clock(pos), clock(info.seconds))).size(12.0).color(p.muted));
                if let Some(t) = seek_bar(ui, pos, info.seconds, p) {
                    action = Some(Some(t));
                }
            });
        });
    });
    if let Some(a) = action {
        if !is_current {
            stop_all(s);
            s.video = Some(VideoPlayer { path: abs.to_path_buf(), info, size, tex: None, frames: None, from: 0.0, started: None, at: 0.0, #[cfg(feature = "audio")] sink: None, shown: 0.0 });
        }
        let v = s.video.as_mut().unwrap();
        match a {
            None if v.started.is_some() => {
                let at = video_pos(v);
                pause_video(v, at);
            }
            None => {
                let at = v.at;
                play_video(s, at, &ctx);
            }
            Some(t) => {
                if v.started.is_some() {
                    play_video(s, t, &ctx);
                } else {
                    // paused: show that frame
                    v.at = t;
                    let (path, size) = (v.path.clone(), v.size);
                    if let Some(px) = media::video_frame(&path, t, size) {
                        let img = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], &px);
                        match &mut v.tex {
                            Some(tx) => tx.set(img, egui::TextureOptions::LINEAR),
                            None => v.tex = Some(ctx.load_texture("video-frame", img, egui::TextureOptions::LINEAR)),
                        }
                    }
                }
            }
        }
    }
    let _ = name;
    open
}

// ── PDF ───────────────────────────────────────────────────────────────────

fn pdf(ui: &mut egui::Ui, s: &mut State, abs: &Path, name: &str, page: Option<usize>, p: Palette) -> bool {
    let ctx = ui.ctx().clone();
    let doc = s.pdfs.entry(abs.to_path_buf()).or_insert_with(|| {
        let (tx, rx) = mpsc::channel();
        let (path, c) = (abs.to_path_buf(), ctx.clone());
        std::thread::spawn(move || {
            let r = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| media::pdf_pages(&b));
            let _ = tx.send(r);
            c.request_repaint();
        });
        PdfDoc { pages: None, page: page.unwrap_or(0), tex: HashMap::new(), loading: None, count_rx: Some(rx) }
    });
    if let Some(rx) = &doc.count_rx {
        if let Ok(r) = rx.try_recv() {
            doc.pages = Some(r);
            doc.count_rx = None;
        }
    }
    let mut open = false;
    frame(ui, p, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(icons::FILE_TEXT).font(kit::icon_font(14.0)).color(p.muted));
            ui.label(egui::RichText::new(name).size(13.0).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::icon_button(ui, icons::EXTERNAL_LINK, "Abrir no app padrão", false, p).clicked() {
                    open = true;
                }
                if let Some(Ok(n)) = doc.pages {
                    if kit::icon_button(ui, icons::CHEVRON_RIGHT, "Próxima página", false, p).clicked() && doc.page + 1 < n {
                        doc.page += 1;
                    }
                    ui.label(egui::RichText::new(format!("{} / {n}", doc.page + 1)).size(12.0).color(p.muted));
                    if kit::icon_button(ui, icons::CHEVRON_LEFT, "Página anterior", false, p).clicked() && doc.page > 0 {
                        doc.page -= 1;
                    }
                }
            });
        });
        ui.add_space(4.0);
        match &doc.pages {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new(format!("Não foi possível ler o PDF: {e}")).size(12.5).color(p.faint));
            }
            Some(Ok(_)) => {
                let w = ui.available_width();
                // drawn at the screen's pixels, in steps so a resize does not redraw at every pixel
                let px = (((w * ui.ctx().pixels_per_point()) / 160.0).ceil() * 160.0) as u32;
                let key = (doc.page, px);
                if !doc.tex.contains_key(&key) && doc.loading.as_ref().is_none_or(|l| (l.0, l.1) != key) {
                    let (tx, rx) = mpsc::channel();
                    let (path, c, pg) = (abs.to_path_buf(), ctx.clone(), doc.page);
                    std::thread::spawn(move || {
                        let r = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| media::pdf_page(&b, pg, px));
                        let _ = tx.send(r);
                        c.request_repaint();
                    });
                    doc.loading = Some((key.0, key.1, rx));
                }
                if let Some((pg, wpx, rx)) = &doc.loading {
                    if let Ok(r) = rx.try_recv() {
                        let k = (*pg, *wpx);
                        let tex = r.ok().map(|(w, h, rgba)| ctx.load_texture(format!("pdf:{}:{}:{}", abs.display(), k.0, k.1), egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), egui::TextureOptions::LINEAR));
                        doc.tex.insert(k, tex);
                        doc.loading = None;
                    }
                }
                // the page drawn at this width, or another drawing of it meanwhile
                let tex = doc.tex.get(&key).cloned().flatten().or_else(|| doc.tex.iter().find(|((pg, _), t)| *pg == doc.page && t.is_some()).and_then(|(_, t)| t.clone()));
                match tex {
                    Some(t) => {
                        let sz = t.size_vec2();
                        let h = w * sz.y / sz.x;
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
                        ui.painter().image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0_f32, p.border), egui::StrokeKind::Outside);
                    }
                    None if doc.tex.get(&key).is_some_and(|t| t.is_none()) => {
                        ui.label(egui::RichText::new("Não foi possível desenhar esta página.").size(12.5).color(p.faint));
                    }
                    None => {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, w * 1.3), Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                        ui.put(rect, egui::Spinner::new());
                    }
                }
            }
        }
    });
    open
}
