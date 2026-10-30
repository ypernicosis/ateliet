//! Comics: the pictures of a CBZ (read from the zip one at a time) or of a
//! CBR / CB7 (unpacked once into a cache folder), in natural order (`2`
//! before `10`). `ComicInfo.xml`, when there is one, gives the title, the
//! series, the writers and the reading direction.

use super::archive::{self, Magic, Zip};
use super::{Document, Format, Hit, Locator, Meta, Rgba, TextLayer, TocEntry, View};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PICTURES: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "avif", "tif", "tiff"];

enum Source {
    Zip(Zip),
    Dir(PathBuf),
}

pub struct Comic {
    src: Source,
    pages: Vec<String>,
    meta: Meta,
    toc: Vec<TocEntry>,
    /// Decoded pages kept: index → picture (a few, the last used).
    cache: Vec<(usize, std::sync::Arc<image::RgbaImage>)>,
    sizes: HashMap<usize, (u32, u32)>,
}

/// Natural order: runs of digits compare as numbers.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(c) = x.peek().copied().filter(|c| c.is_ascii_digit()) {
                    n.push(c);
                    x.next();
                }
                let mut m = String::new();
                while let Some(d) = y.peek().copied().filter(|c| c.is_ascii_digit()) {
                    m.push(d);
                    y.next();
                }
                let (n2, m2) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n2.len().cmp(&m2.len()).then(n2.cmp(m2));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

fn is_picture(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    if base.starts_with('.') || name.contains("__MACOSX") {
        return false;
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    PICTURES.contains(&ext.as_str())
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, base, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

impl Comic {
    pub fn open(path: &Path) -> Result<Comic, String> {
        let src = if path.is_dir() {
            Source::Dir(path.to_path_buf())
        } else if archive::magic(path) == Magic::Zip {
            Source::Zip(Zip::open(path)?)
        } else {
            let dest = archive::cache_dir(&super::fingerprint(path));
            archive::unpack(path, &dest)?;
            Source::Dir(dest)
        };
        let mut names: Vec<String> = match &src {
            Source::Zip(z) => z.names().map(str::to_string).collect(),
            Source::Dir(d) => {
                let mut v = Vec::new();
                walk(d, d, &mut v);
                v
            }
        };
        let info = names.iter().find(|n| n.rsplit('/').next().is_some_and(|b| b.eq_ignore_ascii_case("ComicInfo.xml"))).cloned();
        names.retain(|n| is_picture(n));
        names.sort_by(|a, b| natural_cmp(a, b));
        if names.is_empty() {
            return Err("nenhuma imagem no arquivo".into());
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut meta = Meta { title: stem, ..Default::default() };
        let mut c = Comic { src, pages: names, meta: Meta::default(), toc: Vec::new(), cache: Vec::new(), sizes: HashMap::new() };
        if let Some(info) = info {
            if let Some(text) = c.bytes(&info).map(|b| String::from_utf8_lossy(&b).into_owned()) {
                let t = super::xml::tree(&text);
                let get = |k: &str| t.first(&k.to_ascii_lowercase()).map(|n| n.all_text()).filter(|s| !s.is_empty());
                let series = get("Series");
                let number = get("Number");
                match (get("Title"), series.clone()) {
                    (Some(title), Some(s)) => meta.title = format!("{s} {} — {title}", number.clone().unwrap_or_default()).replace("  ", " "),
                    (Some(title), None) => meta.title = title,
                    (None, Some(s)) => meta.title = format!("{s} {}", number.unwrap_or_default()).trim().to_string(),
                    _ => {}
                }
                meta.author = [get("Writer"), get("Penciller")].into_iter().flatten().collect::<Vec<_>>().join(", ");
                meta.lang = get("LanguageISO").unwrap_or_default();
                meta.publisher = get("Publisher").unwrap_or_default();
                meta.rtl = get("Manga").is_some_and(|m| m.eq_ignore_ascii_case("YesAndRightToLeft"));
            }
        }
        // chapters: the folders inside the archive, when there are several
        let mut last = String::new();
        let folders: Vec<(usize, String)> = c.pages.iter().enumerate().filter_map(|(i, p)| {
            let dir = p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
            (dir != last).then(|| {
                last = dir.clone();
                (i, dir)
            })
        }).collect();
        if folders.len() > 1 {
            c.toc = folders.into_iter().map(|(i, d)| TocEntry { title: d.rsplit('/').next().unwrap_or(&d).to_string(), level: 0, chap: 0, page: Some(i), offset: None }).collect();
        }
        c.meta = meta;
        Ok(c)
    }

    fn bytes(&self, name: &str) -> Option<Vec<u8>> {
        match &self.src {
            Source::Zip(z) => z.read(name).ok(),
            Source::Dir(d) => std::fs::read(d.join(name)).ok(),
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// A page decoded, at its own size.
    pub fn picture(&mut self, page: usize) -> Result<std::sync::Arc<image::RgbaImage>, String> {
        if let Some(i) = self.cache.iter().position(|(p, _)| *p == page) {
            let hit = self.cache.remove(i);
            self.cache.push(hit.clone());
            return Ok(hit.1);
        }
        let name = self.pages.get(page).ok_or("página fora do livro")?.clone();
        let b = self.bytes(&name).ok_or_else(|| format!("{name}: não foi possível ler"))?;
        let img = std::sync::Arc::new(image::load_from_memory(&b).map_err(|e| format!("{name}: {e}"))?.to_rgba8());
        self.sizes.insert(page, img.dimensions());
        self.cache.push((page, img.clone()));
        if self.cache.len() > 6 {
            self.cache.remove(0);
        }
        Ok(img)
    }

    /// The original file of a page (for cutting without resampling).
    pub fn page_name(&self, page: usize) -> Option<&str> {
        self.pages.get(page).map(String::as_str)
    }
}

impl Document for Comic {
    fn format(&self) -> Format {
        Format::Comic
    }

    fn meta(&self) -> &Meta {
        &self.meta
    }

    fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    fn chapters(&self) -> usize {
        1
    }

    fn pages(&mut self, _chap: usize, _view: &View) -> usize {
        self.pages.len()
    }

    fn size(&mut self, _chap: usize, page: usize, _view: &View) -> (f32, f32) {
        if let Some((w, h)) = self.sizes.get(&page) {
            return (*w as f32, *h as f32);
        }
        // the header is enough for the size
        if let Some(name) = self.pages.get(page).cloned() {
            if let Some(b) = self.bytes(&name) {
                if let Ok((w, h)) = image::ImageReader::new(std::io::Cursor::new(&b)).with_guessed_format().map_err(|e| e.to_string()).and_then(|r| r.into_dimensions().map_err(|e| e.to_string())) {
                    self.sizes.insert(page, (w, h));
                    return (w as f32, h as f32);
                }
            }
        }
        (2.0, 3.0)
    }

    fn render(&mut self, _chap: usize, page: usize, _view: &View, px_w: u32) -> Result<Rgba, String> {
        let img = self.picture(page)?;
        let (w, h) = img.dimensions();
        if px_w >= w || px_w == 0 {
            return Ok(Rgba { w, h, px: img.as_raw().clone() });
        }
        let nh = ((h as f64) * px_w as f64 / w as f64).round().max(1.0) as u32;
        let small = image::imageops::resize(img.as_ref(), px_w, nh, image::imageops::FilterType::Triangle);
        Ok(Rgba { w: px_w, h: nh, px: small.into_raw() })
    }

    fn text(&mut self, _chap: usize, _page: usize, _view: &View) -> Option<TextLayer> {
        None
    }

    fn locate(&mut self, loc: &Locator, _view: &View) -> (usize, usize) {
        (0, loc.page.unwrap_or(1).saturating_sub(1).min(self.pages.len().saturating_sub(1)))
    }

    fn locator(&mut self, _chap: usize, page: usize, _view: &View) -> Locator {
        Locator { page: Some(page + 1), ..Default::default() }
    }

    fn search(&mut self, _query: &str, _view: &View) -> Vec<Hit> {
        Vec::new()
    }

    fn cover(&mut self, px_w: u32) -> Option<Rgba> {
        self.render(0, 0, &View::default(), px_w).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural() {
        let mut v = vec!["p10.jpg", "p2.jpg", "p1.jpg", "a/p01.png"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a/p01.png", "p1.jpg", "p2.jpg", "p10.jpg"]);
    }

    #[test]
    fn cbz() {
        let dir = std::env::temp_dir().join(format!("ateliet-cbz-{}", crate::journal::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(20, 30, image::Rgba([200, 10, 10, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        let info = b"<ComicInfo><Series>Akira</Series><Number>1</Number><Manga>YesAndRightToLeft</Manga><Writer>Otomo</Writer></ComicInfo>";
        let z = crate::board::zip::write(&[("10.png".into(), &png[..]), ("2.png".into(), &png[..]), ("ComicInfo.xml".into(), &info[..])]);
        let p = dir.join("a.cbz");
        std::fs::write(&p, z).unwrap();
        let mut c = Comic::open(&p).unwrap();
        assert_eq!(c.pages, vec!["2.png", "10.png"]);
        assert!(c.meta().rtl);
        assert_eq!(c.meta().title, "Akira 1");
        let v = View::default();
        assert_eq!(c.size(0, 1, &v), (20.0, 30.0));
        let r = c.render(0, 0, &v, 10).unwrap();
        assert_eq!((r.w, r.h), (10, 15));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
