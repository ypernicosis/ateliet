//! Typesetting a book into pages: fonts and shaping, Portuguese
//! hyphenation, the book's blocks, line breaking and pagination with the
//! plugin's rules (chapter openings, drop caps, running heads, folios,
//! front matter, contents), then the pages as PDF or as pictures. The
//! preview and the PDF come from the same pages, so they break alike.

pub mod compose;
pub mod fonts;
pub mod hyph;
pub mod layout;
pub mod lines;
pub mod model;
pub mod pdf;
pub mod raster;

use fonts::FontId;

/// Millimetres in a typographic point.
pub const PT_MM: f32 = 25.4 / 72.0;

#[derive(Clone, Debug)]
pub struct Glyph {
    pub id: u16,
    /// From the run's origin, in mm.
    pub x: f32,
    pub y: f32,
}

/// A picture placed on pages (decoded RGBA, or a JPEG kept as it is).
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub jpeg: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub enum Item {
    /// Glyphs on a baseline at (x, y) mm from the page's top left.
    Text { font: FontId, size: f32, x: f32, y: f32, color: [u8; 3], glyphs: Vec<Glyph>, text: String, map: Vec<(u16, String)> },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: [u8; 3], dash: Option<(f32, f32)> },
    Rect { x: f32, y: f32, w: f32, h: f32, color: [u8; 3] },
    Image { pic: usize, x: f32, y: f32, w: f32, h: f32 },
    /// Polygons (curves already flattened), filled (nonzero) and/or stroked.
    Path { polys: Vec<Vec<(f32, f32)>>, fill: Option<[u8; 3]>, fill_alpha: f32, stroke: Option<Pen> },
    /// Children seen through an affine map (theirs → the page's), clipped
    /// to polygons in their own space, at an opacity.
    Group { m: [f32; 6], clip: Option<Vec<Vec<(f32, f32)>>>, alpha: f32, items: Vec<Item> },
    /// A colour fading along a box (down, or across when `across`).
    Gradient { x: f32, y: f32, w: f32, h: f32, color: [u8; 3], a0: f32, a1: f32, across: bool },
    /// A text item's glyph outlines stroked: a halo drawn under the text.
    Outline { text: Box<Item>, width: f32, color: [u8; 3] },
}

#[derive(Clone, Copy, Debug)]
pub struct Pen {
    pub width: f32,
    pub color: [u8; 3],
    pub dash: Option<(f32, f32)>,
}

/// `m` after `n` (apply `n` first).
pub fn mul(m: &[f32; 6], n: &[f32; 6]) -> [f32; 6] {
    [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3], m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5]]
}

pub const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn apply(m: &[f32; 6], p: (f32, f32)) -> (f32, f32) {
    (m[0] * p.0 + m[2] * p.1 + m[4], m[1] * p.0 + m[3] * p.1 + m[5])
}

/// Every item, groups and outlines opened.
pub fn walk<'a>(items: &'a [Item], f: &mut impl FnMut(&'a Item)) {
    for it in items {
        f(it);
        match it {
            Item::Group { items, .. } => walk(items, f),
            Item::Outline { text, .. } => walk(std::slice::from_ref(text), f),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKind {
    Flow,
    Fixed,
    Front,
    Blank,
}

#[derive(Clone, Debug)]
pub struct Page {
    /// Size in mm (trim plus bleed).
    pub w: f32,
    pub h: f32,
    pub n: usize,
    pub recto: bool,
    pub kind: PageKind,
    pub paper: [u8; 3],
    pub items: Vec<Item>,
    /// The section (chapter id…) that starts or runs on it.
    pub section: String,
    /// Where the trim box sits inside the page (bleed), mm.
    pub trim: (f32, f32, f32, f32),
}

impl Page {
    pub fn new(w: f32, h: f32, n: usize, kind: PageKind, paper: [u8; 3]) -> Page {
        Page { w, h, n, recto: n % 2 == 1, kind, paper, items: Vec::new(), section: String::new(), trim: (0.0, 0.0, w, h) }
    }
}

/// The book as a PDF in `Exportações/` ("— impressão" with bleed and
/// facing pages, or "— tela").
pub fn book_pdf(root: &std::path::Path, ix: &crate::dataview::index::Index, b: &crate::scriptorium::book::Book, opts: &crate::scriptorium::export::Options, db: &fonts::FontDb, print: bool) -> Result<std::path::PathBuf, String> {
    let m = model::build(root, ix, b, opts);
    let laid = layout::typeset(&m, db, &layout::Options { print });
    let lang = if m.meta.lang.is_empty() { "pt-BR".to_string() } else { m.meta.lang.clone() };
    let bytes = pdf::write(&laid.pages, db, &m.pics, &pdf::Meta { title: m.meta.title.clone(), author: m.meta.author.clone(), lang })?;
    let rel = crate::scriptorium::export::out_path(b, if print { "impressão" } else { "tela" }, "pdf");
    let abs = root.join(&rel);
    if let Some(d) = abs.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&abs, bytes).map_err(|e| e.to_string())?;
    Ok(abs)
}

pub fn hex(c: &str) -> Option<[u8; 3]> {
    let s = c.trim().trim_start_matches('#');
    let s: String = if s.len() == 3 { s.chars().flat_map(|c| [c, c]).collect() } else { s.to_string() };
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(&s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

#[cfg(test)]
pub(crate) mod testfonts {
    pub const BUNDLED: [&[u8]; 4] = [
        include_bytes!("../../../native/assets/fonts/EBGaramond-Medium.ttf"),
        include_bytes!("../../../native/assets/fonts/EBGaramond-MediumItalic.ttf"),
        include_bytes!("../../../native/assets/fonts/CormorantGaramond-Medium.ttf"),
        include_bytes!("../../../native/assets/fonts/CormorantGaramond-MediumItalic.ttf"),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_to_pdf_and_picture() {
        let db = fonts::FontDb::new(&testfonts::BUNDLED);
        let f = db.find("EB Garamond", false, 500).expect("EB Garamond");
        let t = db.find("Cormorant Garamond", false, 500).expect("Cormorant");
        assert!(db.width(f, "Olá", 10.5, &[]) > 2.0);
        let mut page = Page::new(140.0, 210.0, 1, PageKind::Flow, [255, 253, 248]);
        let mut put = |font: FontId, size: f32, x: f32, y: f32, text: &str, feats: &[&str]| {
            let s = db.shape(font, text, feats);
            let k = size * PT_MM / db.face(font).upem;
            let mut pen = 0.0;
            let mut glyphs = Vec::new();
            let mut map = Vec::new();
            for g in s.iter() {
                glyphs.push(Glyph { id: g.id, x: pen + g.dx as f32 * k, y: -g.dy as f32 * k });
                let end = s.iter().map(|o| o.cluster).filter(|c| *c > g.cluster).min().unwrap_or(text.len() as u32);
                map.push((g.id, text[g.cluster as usize..end as usize].to_string()));
                pen += g.adv as f32 * k;
            }
            page.items.push(Item::Text { font, size, x, y, color: [26, 20, 16], glyphs, text: text.into(), map });
        };
        put(t, 24.0, 30.0, 60.0, "Capítulo Um", &["smcp"]);
        put(f, 10.5, 22.0, 80.0, "Era uma vez uma torre de vidro — fi, ffl.", &[]);
        put(f, 10.5, 22.0, 86.0, "1234567890", &["onum"]);
        page.items.push(Item::Line { x0: 50.0, y0: 66.0, x1: 90.0, y1: 66.0, width: 0.3, color: [138, 109, 59], dash: None });
        let bytes = pdf::write(&[page.clone()], &db, &[], &pdf::Meta { title: "Teste".into(), author: "A".into(), lang: "pt-BR".into() }).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        let (w, h, px) = raster::render(&page, &db, &[], 4.0);
        assert_eq!((w, h), (560, 840));
        // ink somewhere on the text line
        assert!((300..340).any(|y| (88..300).any(|x| px[((y * w + x) * 4) as usize] < 120)));
        if let Ok(dir) = std::env::var("TYPESET_OUT") {
            std::fs::write(format!("{dir}/t.pdf"), &bytes).unwrap();
            image::RgbaImage::from_raw(w, h, px).unwrap().save(format!("{dir}/t.png")).unwrap();
        }
    }
}

#[cfg(test)]
mod book_tests {
    use super::*;

    /// TYPESET_VAULT=<vault> TYPESET_BOOK=<book note> TYPESET_OUT=<dir> cargo test book_to_pdf -- --nocapture
    #[test]
    fn book_to_pdf() {
        let (Ok(vault), Ok(book_path), Ok(out)) = (std::env::var("TYPESET_VAULT"), std::env::var("TYPESET_BOOK"), std::env::var("TYPESET_OUT")) else { return };
        let root = std::path::Path::new(&vault);
        let ix = crate::dataview::index::Index::build(root);
        let b = crate::scriptorium::book::book_by_path(&ix, &book_path, "").expect("book");
        let m = model::build(root, &ix, &b, &crate::scriptorium::export::Options::default());
        let db = fonts::FontDb::new(&[
            include_bytes!("../../../native/assets/fonts/EBGaramond-Medium.ttf"),
            include_bytes!("../../../native/assets/fonts/EBGaramond-MediumItalic.ttf"),
            include_bytes!("../../../native/assets/fonts/EBGaramond-Bold.ttf"),
            include_bytes!("../../../native/assets/fonts/EBGaramond-BoldItalic.ttf"),
            include_bytes!("../../../native/assets/fonts/CormorantGaramond-Medium.ttf"),
            include_bytes!("../../../native/assets/fonts/CormorantGaramond-MediumItalic.ttf"),
            include_bytes!("../../../native/assets/fonts/CormorantGaramond-Bold.ttf"),
            include_bytes!("../../../native/assets/fonts/Cinzel-Bold.ttf"),
            include_bytes!("../../../native/assets/fonts/Inter-SemiBold.ttf"),
        ]);
        let t0 = std::time::Instant::now();
        let laid = layout::typeset(&m, &db, &layout::Options { print: true });
        eprintln!("{} pages in {:?}", laid.pages.len(), t0.elapsed());
        let bytes = pdf::write(&laid.pages, &db, &m.pics, &pdf::Meta { title: m.meta.title.clone(), author: m.meta.author.clone(), lang: "pt-BR".into() }).unwrap();
        std::fs::write(format!("{out}/book.pdf"), bytes).unwrap();
        for (i, p) in laid.pages.iter().enumerate().take(40) {
            let (w, h, px) = raster::render(p, &db, &m.pics, 4.0);
            image::RgbaImage::from_raw(w, h, px).unwrap().save(format!("{out}/p{:02}.png", i + 1)).unwrap();
        }
    }

    #[test]
    fn cover_to_pdf() {
        let (Ok(vault), Ok(book_path), Ok(out)) = (std::env::var("TYPESET_VAULT"), std::env::var("TYPESET_BOOK"), std::env::var("TYPESET_OUT")) else { return };
        let root = std::path::Path::new(&vault);
        let ix = crate::dataview::index::Index::build(root);
        let b = crate::scriptorium::book::book_by_path(&ix, &book_path, "").expect("book");
        let db = fonts::FontDb::new(&[
            include_bytes!("../../../native/assets/fonts/EBGaramond-Medium.ttf"),
            include_bytes!("../../../native/assets/fonts/CormorantGaramond-Medium.ttf"),
            include_bytes!("../../../native/assets/fonts/Cinzel-Bold.ttf"),
            include_bytes!("../../../native/assets/fonts/Inter-Regular.ttf"),
        ]);
        let m = crate::scriptorium::capa::load_or_default(root, &b);
        let cfg = crate::scriptorium::book::layout(&b);
        let env = compose::cover_env(&m, &cfg, 240);
        let pics = crate::scriptorium::capa::CoverPics::load(root, &ix, &b.path, &m);
        let pg = crate::scriptorium::capa::page(&m, &env, &db, &pics);
        let bytes = pdf::write(&[pg.clone()], &db, &pics.pics.list, &pdf::Meta { title: "capa".into(), author: String::new(), lang: "pt-BR".into() }).unwrap();
        std::fs::write(format!("{out}/capa.pdf"), bytes).unwrap();
        let (w, h, px) = raster::render(&pg, &db, &pics.pics.list, 3.0);
        image::RgbaImage::from_raw(w, h, px).unwrap().save(format!("{out}/capa.png")).unwrap();
    }
}
