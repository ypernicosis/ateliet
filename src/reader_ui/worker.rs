//! The book's own thread: it opens the file, lays out chapters, draws
//! pages and reads their text, away from the window's thread. The window
//! asks; the answers come back when they are ready, and the window draws
//! what it has meanwhile (the page before, or the paper).

use ateliet_core::reader::{self, Document, Format, Hit, Link, Locator, Meta, Rgba, TextLayer, TocEntry, View};
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

pub enum Req {
    Pages { chap: usize, view: View },
    Page { chap: usize, page: usize, view: View, trim: bool },
    Render { chap: usize, page: usize, view: View, px: u32, gen: u64 },
    Locate { loc: Locator, view: View, id: u64 },
    Search { q: String, view: View, id: u64 },
    /// A region drawn at print resolution.
    Crop { chap: usize, page: usize, view: View, rect: [f32; 4], id: u64 },
    Resource { name: String, id: u64 },
    Note { chap: usize, off: usize, id: u64 },
}

/// What a page holds besides its picture.
#[derive(Clone)]
pub struct PageInfo {
    pub size: (f32, f32),
    pub text: Option<Arc<TextLayer>>,
    pub links: Vec<Link>,
    pub images: Vec<([f32; 4], String)>,
    /// The page's start.
    pub loc: Locator,
    /// The part with ink, when the margins are cut (fractions).
    pub content: Option<[f32; 4]>,
}

pub struct Opened {
    pub meta: Meta,
    pub toc: Vec<TocEntry>,
    pub chapters: usize,
    pub reflows: bool,
    pub format: Format,
}

pub enum Resp {
    Opened(Box<Opened>),
    Failed(String),
    Pages { chap: usize, key: String, n: usize },
    Page { chap: usize, page: usize, key: String, info: Box<PageInfo> },
    Image { chap: usize, page: usize, key: String, px: u32, img: Rgba },
    Located { id: u64, chap: usize, page: usize, key: String },
    Search { id: u64, hits: Vec<Hit> },
    Crop { id: u64, img: Result<Rgba, String> },
    Resource { id: u64, bytes: Option<Vec<u8>> },
    Note { id: u64, text: Option<String> },
}

pub struct Worker {
    pub tx: Sender<Req>,
    pub rx: Receiver<Resp>,
    /// Renders older than this are dropped (the page was turned).
    pub gen: Arc<AtomicU64>,
}

/// The box of what is not paper on a small picture of the page.
fn ink_box(img: &Rgba) -> Option<[f32; 4]> {
    let (w, h) = (img.w as usize, img.h as usize);
    if w < 8 || h < 8 {
        return None;
    }
    let bg = [img.px[0], img.px[1], img.px[2]];
    let ink = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        (0..3).any(|k| (img.px[i + k] as i32 - bg[k] as i32).abs() > 28)
    };
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if ink(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let pad = 0.02;
    Some([(x0 as f32 / w as f32 - pad).max(0.0), (y0 as f32 / h as f32 - pad).max(0.0), (x1 as f32 / w as f32 + pad).min(1.0), (y1 as f32 / h as f32 + pad).min(1.0)])
}

pub fn spawn(abs: PathBuf, ctx: egui::Context) -> Worker {
    let (tx, req_rx) = mpsc::channel::<Req>();
    let (resp_tx, rx) = mpsc::channel::<Resp>();
    let gen = Arc::new(AtomicU64::new(0));
    let g = gen.clone();
    std::thread::spawn(move || {
        let send = |r: Resp| {
            let _ = resp_tx.send(r);
            ctx.request_repaint();
        };
        let mut doc: Box<dyn Document> = match reader::open(&abs, crate::fonts::book_fonts()) {
            Ok(d) => d,
            Err(e) => {
                send(Resp::Failed(e));
                return;
            }
        };
        send(Resp::Opened(Box::new(Opened { meta: doc.meta().clone(), toc: doc.toc().to_vec(), chapters: doc.chapters(), reflows: doc.reflows(), format: doc.format() })));
        let mut queue: Vec<Req> = Vec::new();
        loop {
            if queue.is_empty() {
                match req_rx.recv() {
                    Ok(r) => queue.push(r),
                    Err(_) => return,
                }
            }
            while let Ok(r) = req_rx.try_recv() {
                queue.push(r);
            }
            // the newest asks first: what is on the screen now
            let r = queue.pop().unwrap();
            match r {
                Req::Pages { chap, view } => {
                    let n = doc.pages(chap, &view);
                    send(Resp::Pages { chap, key: view.key(), n });
                }
                Req::Page { chap, page, view, trim } => {
                    let size = doc.size(chap, page, &view);
                    let text = doc.text(chap, page, &view).map(Arc::new);
                    let links = doc.links(chap, page, &view);
                    let images = doc.images(chap, page, &view);
                    let loc = doc.locator(chap, page, &view);
                    let content = if trim { doc.render(chap, page, &view, 160).ok().and_then(|i| ink_box(&i)) } else { None };
                    send(Resp::Page { chap, page, key: view.key(), info: Box::new(PageInfo { size, text, links, images, loc, content }) });
                }
                Req::Render { chap, page, view, px, gen } => {
                    if gen + 2 < g.load(Ordering::Relaxed) {
                        continue;
                    }
                    match doc.render(chap, page, &view, px) {
                        Ok(img) => send(Resp::Image { chap, page, key: view.key(), px, img }),
                        Err(e) => send(Resp::Failed(e)),
                    }
                }
                Req::Locate { loc, view, id } => {
                    let (chap, page) = doc.locate(&loc, &view);
                    send(Resp::Located { id, chap, page, key: view.key() });
                }
                Req::Search { q, view, id } => {
                    let hits = doc.search(&q, &view);
                    send(Resp::Search { id, hits });
                }
                Req::Crop { chap, page, view, rect, id } => {
                    // about 300 ppi: a PDF page from its points, an EPUB page
                    // from its millimetres, a comic page at its own size
                    let (w, _) = doc.size(chap, page, &view);
                    let px = match doc.format() {
                        Format::Pdf => (w * 300.0 / 72.0) as u32,
                        Format::Epub if doc.reflows() => (view.w * 12.0) as u32,
                        _ => 0,
                    };
                    let img = doc.render(chap, page, &view, px.min(6000)).map(|full| full.crop(rect));
                    send(Resp::Crop { id, img });
                }
                Req::Resource { name, id } => send(Resp::Resource { id, bytes: doc.resource(&name) }),
                Req::Note { chap, off, id } => send(Resp::Note { id, text: doc.note_text(chap, off) }),
            }
        }
    });
    Worker { tx, rx, gen }
}
