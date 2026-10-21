//! Clip Studio Paint `.clip` files: the layers, as pictures, for the
//! storyboard.
//!
//! A `.clip` file is a sequence of chunks after a 24-byte `CSFCHUNK` header.
//! One chunk (`SQLi`) is a SQLite database that describes the canvas and the
//! layer tree; the `Exta` chunks hold the pixels, in 256×256 zlib tiles,
//! referenced from the database by an external id. The format is not
//! documented; this follows what clip_to_psd (MIT, dobrokot) found out.
//!
//! Every layer that has rendered pixels comes out as a picture: raster
//! layers, and vector, text and gradient layers as Clip Studio last drew
//! them. Folders are flattened (their visibility and opacity carried down to
//! the layers inside), layer masks are applied, the paper becomes a plain
//! colour, and correction layers, which cannot be represented, are listed
//! as skipped.

use image::{GrayImage, Rgba, RgbaImage};
use rusqlite::types::Value;
use std::collections::HashMap;
use std::io::Read;

const TILE: usize = 256;

#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    /// Names of the folders the layer was in, outermost first.
    pub folders: Vec<String>,
    pub visible: bool,
    /// 0..1, folders included.
    pub opacity: f32,
    /// Canvas 2D composite operation that renders the blend mode.
    pub blend: &'static str,
    /// Clip Studio's own name for the blend mode, when `blend` only
    /// approximates it.
    pub blend_approx: Option<&'static str>,
    /// Clipped to the layer below.
    pub clip: bool,
    /// "raster", "vector" (vector, text, gradient… as rendered) or "paper".
    pub kind: &'static str,
    /// Position of `image` on the canvas.
    pub x: i32,
    pub y: i32,
    pub image: RgbaImage,
}

#[derive(Debug)]
pub struct Document {
    pub width: u32,
    pub height: u32,
    pub dpi: f64,
    /// Bottom to top.
    pub layers: Vec<Layer>,
    /// The composite Clip Studio saved with the file (PNG).
    pub preview: Option<Vec<u8>>,
    /// Layers left out, with the reason.
    pub skipped: Vec<String>,
}

struct Chunks<'a> {
    sqlite: &'a [u8],
    external: HashMap<Vec<u8>, &'a [u8]>,
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be64(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8).map(|b| u64::from_be_bytes(b.try_into().unwrap()))
}

fn split_chunks(data: &[u8]) -> Result<Chunks<'_>, String> {
    if data.len() < 28 || &data[..8] != b"CSFCHUNK" {
        return Err("not a Clip Studio file".into());
    }
    let mut at = 24;
    let mut sqlite: &[u8] = &[];
    let mut external = HashMap::new();
    while at + 16 <= data.len() {
        if &data[at..at + 4] != b"CHNK" {
            return Err(format!("damaged file: no chunk at byte {at}"));
        }
        let name = &data[at + 4..at + 8];
        let size = be32(data, at + 12).unwrap() as usize;
        let body = data.get(at + 16..at + 16 + size).ok_or("damaged file: a chunk runs past the end")?;
        match name {
            b"SQLi" => sqlite = body,
            b"Exta" => {
                let id_len = be64(body, 0).unwrap_or(0) as usize;
                if let (Some(id), Some(_)) = (body.get(8..8 + id_len), be64(body, 8 + id_len)) {
                    external.insert(id.to_vec(), &body[16 + id_len..]);
                }
            }
            _ => {}
        }
        at += 16 + size;
    }
    if sqlite.is_empty() {
        return Err("damaged file: no layer database".into());
    }
    Ok(Chunks { sqlite, external })
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

/// The zlib tiles of a bitmap, in grid order (`None` for an empty tile).
fn tiles(d: &[u8]) -> Result<Vec<Option<&[u8]>>, String> {
    let begin = utf16("BlockDataBeginChunk");
    let end = utf16("BlockDataEndChunk");
    let status = [&[0, 0, 0, 0x0b][..], &utf16("BlockStatus")].concat();
    let checksum = [&[0, 0, 0, 0x0d][..], &utf16("BlockCheckSum")].concat();
    let mut out = Vec::new();
    let mut at = 0;
    while at < d.len() {
        let size = if d[at..].starts_with(&status) {
            let count = be32(d, at + 30).ok_or("short block status")? as usize;
            count * 4 + 12 + status.len()
        } else if d[at..].starts_with(&checksum) {
            checksum.len() + 12 + out.len() * 4
        } else if d.get(at + 8..at + 8 + begin.len()) == Some(&begin[..]) {
            let size = be32(d, at).unwrap() as usize;
            let tail = 4 + end.len();
            if size < 8 + begin.len() + tail || at + size > d.len() {
                return Err("damaged pixel block".into());
            }
            let block = &d[at + 8 + begin.len()..at + size - tail];
            match be32(block, 16) {
                Some(0) => out.push(None),
                Some(1) => {
                    let len = be32(block, 20).ok_or("short pixel block")? as usize;
                    if block.len() != len + 24 {
                        return Err("damaged pixel block size".into());
                    }
                    out.push(Some(&block[28..]));
                }
                _ => return Err("unknown pixel block".into()),
            }
            size
        } else {
            return Err(format!("unknown data in a bitmap at {at}"));
        };
        if size == 0 {
            break;
        }
        at += size;
    }
    Ok(out)
}

/// What the `Offscreen.Attribute` blob says about a bitmap.
#[derive(Debug, Clone, Copy)]
struct Offscreen {
    width: usize,
    height: usize,
    grid_w: usize,
    grid_h: usize,
    /// Outside the drawn tiles: white (true) or transparent/black.
    fill_white: bool,
    /// RGBA (alpha plane + BGRx) or a single channel.
    rgba: bool,
}

struct Cursor<'a> {
    d: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn int(&mut self) -> Result<u32, String> {
        let v = be32(self.d, self.at).ok_or("short bitmap description")?;
        self.at += 4;
        Ok(v)
    }
    fn expect(&mut self, s: &str) -> Result<(), String> {
        let n = self.int()? as usize;
        let got = self.d.get(self.at..self.at + 2 * n).ok_or("short bitmap description")?;
        self.at += 2 * n;
        if got != utf16(s).as_slice() {
            return Err(format!("expected {s} in the bitmap description"));
        }
        Ok(())
    }
}

fn parse_offscreen(a: &[u8]) -> Result<Offscreen, String> {
    let mut c = Cursor { d: a, at: 0 };
    let (header, info, extra) = (c.int()?, c.int()?, c.int()?);
    if header != 16 || info != 102 || !(extra == 42 || extra == 58) {
        return Err("unknown bitmap description".into());
    }
    c.int()?;
    c.expect("Parameter")?;
    let width = c.int()? as usize;
    let height = c.int()? as usize;
    let grid_w = c.int()? as usize;
    let grid_h = c.int()? as usize;
    let mut packing = [0u32; 16];
    for p in packing.iter_mut() {
        *p = c.int()?;
    }
    c.expect("InitColor")?;
    c.int()?;
    let fill = c.int()?;
    let rgba = (packing[1], packing[2]) == (1, 4);
    if !rgba && packing[1] + packing[2] != 1 {
        return Err(format!("unsupported pixel packing {}+{}", packing[1], packing[2]));
    }
    if width > 30000 || height > 30000 || grid_w * TILE < width || grid_h * TILE < height {
        return Err("bitmap size out of range".into());
    }
    Ok(Offscreen { width, height, grid_w, grid_h, fill_white: fill != 0, rgba })
}

fn inflate(z: &[u8], expect: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(expect);
    flate2::read::ZlibDecoder::new(z).read_to_end(&mut out).ok()?;
    (out.len() == expect).then_some(out)
}

fn decode_rgba(info: &Offscreen, blocks: &[Option<&[u8]>]) -> Result<RgbaImage, String> {
    if blocks.len() != info.grid_w * info.grid_h {
        return Err("bitmap tile count does not match its size".into());
    }
    let fill = if info.fill_white { Rgba([255, 255, 255, 255]) } else { Rgba([0, 0, 0, 0]) };
    let mut img = RgbaImage::from_pixel(info.width as u32, info.height as u32, fill);
    let k = TILE * TILE;
    for (i, block) in blocks.iter().enumerate() {
        let Some(z) = block else { continue };
        let (tx, ty) = ((i % info.grid_w) * TILE, (i / info.grid_w) * TILE);
        if info.rgba {
            let Some(px) = inflate(z, 5 * k) else { continue };
            for y in 0..TILE.min(info.height.saturating_sub(ty)) {
                for x in 0..TILE.min(info.width.saturating_sub(tx)) {
                    let p = y * TILE + x;
                    let c = k + p * 4;
                    img.put_pixel((tx + x) as u32, (ty + y) as u32, Rgba([px[c + 2], px[c + 1], px[c], px[p]]));
                }
            }
        } else {
            let Some(px) = inflate(z, k) else { continue };
            for y in 0..TILE.min(info.height.saturating_sub(ty)) {
                for x in 0..TILE.min(info.width.saturating_sub(tx)) {
                    let v = px[y * TILE + x];
                    img.put_pixel((tx + x) as u32, (ty + y) as u32, Rgba([v, v, v, 255]));
                }
            }
        }
    }
    Ok(img)
}

fn decode_gray(info: &Offscreen, blocks: &[Option<&[u8]>]) -> Result<GrayImage, String> {
    if blocks.len() != info.grid_w * info.grid_h {
        return Err("mask tile count does not match its size".into());
    }
    let mut img = GrayImage::from_pixel(info.width as u32, info.height as u32, image::Luma([if info.fill_white { 255 } else { 0 }]));
    let k = TILE * TILE;
    for (i, block) in blocks.iter().enumerate() {
        let Some(z) = block else { continue };
        let (tx, ty) = ((i % info.grid_w) * TILE, (i / info.grid_w) * TILE);
        // A mask is normally one channel; take the alpha plane of an RGBA one.
        let Some(px) = inflate(z, if info.rgba { 5 * k } else { k }) else { continue };
        for y in 0..TILE.min(info.height.saturating_sub(ty)) {
            for x in 0..TILE.min(info.width.saturating_sub(tx)) {
                img.put_pixel((tx + x) as u32, (ty + y) as u32, image::Luma([px[y * TILE + x]]));
            }
        }
    }
    Ok(img)
}

type Row = HashMap<String, Value>;

fn rows(conn: &rusqlite::Connection, table: &str) -> Result<Vec<Row>, String> {
    let exists: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\"")).map_err(|e| e.to_string())?;
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let out = stmt
        .query_map([], |r| {
            let mut row = Row::new();
            for (i, n) in names.iter().enumerate() {
                row.insert(n.clone(), r.get::<_, Value>(i)?);
            }
            Ok(row)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn int(r: &Row, k: &str) -> i64 {
    match r.get(k) {
        Some(Value::Integer(i)) => *i,
        Some(Value::Real(f)) => *f as i64,
        _ => 0,
    }
}

fn real(r: &Row, k: &str) -> f64 {
    match r.get(k) {
        Some(Value::Integer(i)) => *i as f64,
        Some(Value::Real(f)) => *f,
        _ => 0.0,
    }
}

fn bytes(r: &Row, k: &str) -> Option<Vec<u8>> {
    match r.get(k) {
        Some(Value::Blob(b)) if !b.is_empty() => Some(b.clone()),
        Some(Value::Text(t)) if !t.is_empty() => Some(t.as_bytes().to_vec()),
        _ => None,
    }
}

fn text(r: &Row, k: &str) -> String {
    match r.get(k) {
        Some(Value::Text(t)) => t.clone(),
        Some(Value::Blob(b)) => String::from_utf8_lossy(b).into_owned(),
        _ => String::new(),
    }
}

/// Clip Studio blend mode → (canvas composite operation, approximated name).
fn blend(mode: i64) -> (&'static str, Option<&'static str>) {
    match mode {
        0 | 30 => ("source-over", None),
        1 => ("darken", None),
        2 => ("multiply", None),
        3 => ("color-burn", None),
        4 => ("multiply", Some("linear burn")),
        5 => ("difference", Some("subtract")),
        6 => ("darken", Some("darker color")),
        7 => ("lighten", None),
        8 => ("screen", None),
        9 => ("color-dodge", None),
        10 => ("color-dodge", Some("glow dodge")),
        11 => ("lighter", None),
        12 => ("lighter", Some("add (glow)")),
        13 => ("lighten", Some("lighter color")),
        14 => ("overlay", None),
        15 => ("soft-light", None),
        16 => ("hard-light", None),
        17 => ("hard-light", Some("vivid light")),
        18 => ("hard-light", Some("linear light")),
        19 => ("hard-light", Some("pin light")),
        20 => ("hard-light", Some("hard mix")),
        21 => ("difference", None),
        22 => ("exclusion", None),
        23 => ("hue", None),
        24 => ("saturation", None),
        25 => ("color", None),
        26 => ("luminosity", None),
        36 => ("color-dodge", Some("divide")),
        _ => ("source-over", Some("unknown")),
    }
}

struct Db {
    layers: HashMap<i64, Row>,
    /// Mipmap id → external id and bitmap description of its full-size level.
    bitmaps: HashMap<i64, (Vec<u8>, Vec<u8>)>,
    canvas: Row,
    preview: Option<Vec<u8>>,
}

fn open_db(sqlite: &[u8]) -> Result<Db, String> {
    // rusqlite opens files; the database goes to a private temporary one.
    let dir = std::env::temp_dir().join(format!("ateliet-clip-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("canvas.sqlite");
    let result = (|| {
        std::fs::write(&path, sqlite).map_err(|e| e.to_string())?;
        let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
        let layers = rows(&conn, "Layer")?.into_iter().map(|r| (int(&r, "MainId"), r)).collect();
        let offscreen: HashMap<i64, Row> = rows(&conn, "Offscreen")?.into_iter().map(|r| (int(&r, "MainId"), r)).collect();
        let info: HashMap<i64, i64> = rows(&conn, "MipmapInfo")?.iter().map(|r| (int(r, "MainId"), int(r, "Offscreen"))).collect();
        let mut bitmaps = HashMap::new();
        for m in rows(&conn, "Mipmap")? {
            let Some(off) = info.get(&int(&m, "BaseMipmapInfo")).and_then(|o| offscreen.get(o)) else { continue };
            if let (Some(id), Some(attr)) = (bytes(off, "BlockData"), bytes(off, "Attribute")) {
                bitmaps.insert(int(&m, "MainId"), (id, attr));
            }
        }
        let canvas = rows(&conn, "Canvas")?.into_iter().next().ok_or("no canvas in the file")?;
        let preview = rows(&conn, "CanvasPreview")?.into_iter().find_map(|r| bytes(&r, "ImageData"));
        Ok(Db { layers, bitmaps, canvas, preview })
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn bitmap<'a>(chunks: &'a Chunks, db: &Db, mipmap: i64) -> Result<Option<(Offscreen, Vec<Option<&'a [u8]>>)>, String> {
    if mipmap == 0 {
        return Ok(None);
    }
    let Some((id, attr)) = db.bitmaps.get(&mipmap) else { return Ok(None) };
    let Some(data) = chunks.external.get(id) else { return Ok(None) };
    let info = parse_offscreen(attr)?;
    Ok(Some((info, tiles(data)?)))
}

/// The smallest rectangle holding every pixel that is not transparent.
fn bounds(img: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] != 0 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    (x1 > x0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

/// A layer mask, placed on the canvas.
struct Mask {
    image: GrayImage,
    x: i32,
    y: i32,
    /// Value outside the mask's bitmap.
    outside: u8,
}

impl Mask {
    fn at(&self, x: i32, y: i32) -> u16 {
        let (mx, my) = (x - self.x, y - self.y);
        if mx >= 0 && my >= 0 && (mx as u32) < self.image.width() && (my as u32) < self.image.height() {
            self.image.get_pixel(mx as u32, my as u32)[0] as u16
        } else {
            self.outside as u16
        }
    }
}

/// The enabled layer mask of a layer or folder, if it has one.
fn mask(chunks: &Chunks, db: &Db, l: &Row) -> Result<Option<Mask>, String> {
    if int(l, "LayerVisibility") & 2 == 0 {
        return Ok(None);
    }
    let Some((info, blocks)) = bitmap(chunks, db, int(l, "LayerLayerMaskMipmap"))? else { return Ok(None) };
    let image = decode_gray(&info, &blocks)?;
    let x = (int(l, "LayerMaskOffsetX") + int(l, "LayerOffsetX") + int(l, "LayerMaskOffscrOffsetX")) as i32;
    let y = (int(l, "LayerMaskOffsetY") + int(l, "LayerOffsetY") + int(l, "LayerMaskOffscrOffsetY")) as i32;
    Ok(Some(Mask { image, x, y, outside: if info.fill_white { 255 } else { 0 } }))
}

fn apply_mask(img: &mut RgbaImage, ox: i32, oy: i32, m: &Mask) {
    for (x, y, p) in img.enumerate_pixels_mut() {
        if p[3] != 0 {
            p[3] = ((p[3] as u16 * m.at(x as i32 + ox, y as i32 + oy) + 127) / 255) as u8;
        }
    }
}

/// The colour of a flat fill layer ("GradationSettingAdd0001"), which has no
/// pixels of its own.
fn flat_color(info: &[u8]) -> Option<[u8; 3]> {
    let mut at = 8;
    while let Some(n) = be32(info, at) {
        let n = n as usize;
        let name = info.get(at + 4..at + 4 + 2 * n)?;
        at += 4 + 2 * n;
        if name == utf16("GradationSetting").as_slice() {
            at += 3 * 4 + 7 * 8;
            continue;
        }
        let size = be32(info, at)? as usize;
        let section = info.get(at + 4..at + 4 + size)?;
        at += 4 + size;
        if name == utf16("GradationSettingAdd0001").as_slice() {
            if be32(section, 0)? == 0 {
                return None;
            }
            let c = |i: usize| be32(section, 4 + 4 * i).map(|v| (v >> 24) as u8);
            return Some([c(0)?, c(1)?, c(2)?]);
        }
    }
    None
}

struct Context<'a> {
    folders: Vec<String>,
    visible: bool,
    opacity: f32,
    /// Masks of the enclosing folders.
    masks: Vec<&'a Mask>,
}

pub fn read(data: &[u8]) -> Result<Document, String> {
    let chunks = split_chunks(data)?;
    let db = open_db(chunks.sqlite)?;
    let width = real(&db.canvas, "CanvasWidth").round() as u32;
    let height = real(&db.canvas, "CanvasHeight").round() as u32;
    if width == 0 || height == 0 || width > 30000 || height > 30000 {
        return Err("canvas size out of range".into());
    }
    let mut doc = Document { width, height, dpi: real(&db.canvas, "CanvasResolution"), layers: Vec::new(), preview: db.preview.clone(), skipped: Vec::new() };
    let root = int(&db.canvas, "CanvasRootFolder");
    let ctx = Context { folders: Vec::new(), visible: true, opacity: 1.0, masks: Vec::new() };
    walk(&chunks, &db, root, &ctx, &mut doc, 0)?;
    Ok(doc)
}

fn walk(chunks: &Chunks, db: &Db, folder: i64, ctx: &Context, doc: &mut Document, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("layer folders nested too deep".into());
    }
    let Some(f) = db.layers.get(&folder) else { return Ok(()) };
    let mut id = int(f, "LayerFirstChildIndex");
    let mut seen = 0;
    while id != 0 && seen < 100_000 {
        seen += 1;
        let Some(l) = db.layers.get(&id) else { break };
        let name = text(l, "LayerName");
        let visible = ctx.visible && int(l, "LayerVisibility") & 1 != 0;
        let opacity = ctx.opacity * (int(l, "LayerOpacity") as f32 / 256.0).clamp(0.0, 1.0);
        if int(l, "LayerFolder") != 0 {
            let mut folders = ctx.folders.clone();
            folders.push(name);
            let own = mask(chunks, db, l)?;
            let mut masks = ctx.masks.clone();
            masks.extend(own.as_ref());
            walk(chunks, db, id, &Context { folders, visible, opacity, masks }, doc, depth + 1)?;
        } else if let Err(e) = layer(chunks, db, l, name.clone(), ctx, visible, opacity, doc) {
            doc.skipped.push(format!("{name}: {e}"));
        }
        id = int(l, "LayerNextIndex");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn layer(chunks: &Chunks, db: &Db, l: &Row, name: String, ctx: &Context, visible: bool, opacity: f32, doc: &mut Document) -> Result<(), String> {
    let (blend_op, approx) = blend(int(l, "LayerComposite"));
    let base = Layer { name: name.clone(), folders: ctx.folders.clone(), visible, opacity, blend: blend_op, blend_approx: approx, clip: int(l, "LayerClip") != 0, kind: "raster", x: 0, y: 0, image: RgbaImage::new(1, 1) };
    if int(l, "DrawColorEnable") != 0 {
        let c = |k: &str| (int(l, k) as u64 >> 24).min(255) as u8;
        let color = Rgba([c("DrawColorMainRed"), c("DrawColorMainGreen"), c("DrawColorMainBlue"), 255]);
        doc.layers.push(Layer { kind: "paper", image: RgbaImage::from_pixel(doc.width, doc.height, color), ..base });
        return Ok(());
    }
    if bytes(l, "FilterLayerInfo").is_some() {
        doc.skipped.push(format!("{name}: correction layer"));
        return Ok(());
    }
    let (mut img, ox, oy) = match bytes(l, "GradationFillInfo").and_then(|g| flat_color(&g)) {
        // A flat colour fill covers the canvas; its mask gives it its shape.
        Some([r, g, b]) => (RgbaImage::from_pixel(doc.width, doc.height, Rgba([r, g, b, 255])), 0, 0),
        None => {
            let Some((info, blocks)) = bitmap(chunks, db, int(l, "LayerRenderMipmap"))? else {
                return Ok(());
            };
            let ox = (int(l, "LayerOffsetX") + int(l, "LayerRenderOffscrOffsetX")) as i32;
            let oy = (int(l, "LayerOffsetY") + int(l, "LayerRenderOffscrOffsetY")) as i32;
            (decode_rgba(&info, &blocks)?, ox, oy)
        }
    };
    if let Some(m) = mask(chunks, db, l)? {
        apply_mask(&mut img, ox, oy, &m);
    }
    for m in &ctx.masks {
        apply_mask(&mut img, ox, oy, m);
    }
    let Some((bx, by, bw, bh)) = bounds(&img) else { return Ok(()) };
    let image = image::imageops::crop_imm(&img, bx, by, bw, bh).to_image();
    let kind = if int(l, "LayerType") & 1 != 0 { "raster" } else { "vector" };
    doc.layers.push(Layer { kind, x: ox + bx as i32, y: oy + by as i32, image, ..base });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Option<Vec<u8>> {
        let p = std::env::var("ATELIET_CLIP_SAMPLE").ok()?;
        std::fs::read(p).ok()
    }

    #[test]
    fn rejects_other_files() {
        assert!(read(b"PNG not a clip file at all, nope").is_err());
        let mut bad = b"CSFCHUNK".to_vec();
        bad.extend([0u8; 16]);
        bad.extend(b"CHNKHead\0\0\0\0\0\0\0\x04abcd");
        assert!(read(&bad).is_err(), "no database");
    }

    #[test]
    fn blend_modes() {
        assert_eq!(blend(2), ("multiply", None));
        assert_eq!(blend(30).0, "source-over");
        assert!(blend(17).1.is_some());
    }

    /// With ATELIET_CLIP_SAMPLE pointing at clip_to_psd's
    /// tests/test_export_all_features.clip (and ATELIET_CLIP_REFERENCE at a
    /// folder of its per-layer PNGs, made with `--output-dir`).
    #[test]
    fn reads_sample() {
        let Some(data) = sample() else { return };
        let doc = read(&data).unwrap();
        assert_eq!((doc.width, doc.height), (928, 647));
        assert!(doc.preview.as_ref().is_some_and(|p| p.starts_with(b"\x89PNG")));
        let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(doc.layers[0].kind, "paper");
        assert!(names.contains(&"Layer 1") && names.contains(&"МОЙ СЛОЙ!"));
        assert!(doc.skipped.iter().any(|s| s.starts_with("Level Correction 1")));
        let l2 = doc.layers.iter().find(|l| l.name == "Layer 2 opacity").unwrap();
        assert_eq!(l2.folders, vec!["Folder 1"]);
        assert!((l2.opacity - 207.0 / 256.0).abs() < 1e-4);
        assert_eq!(doc.layers.iter().find(|l| l.name == "multiply Copy").unwrap().blend, "multiply");

        // Pixel-exact against clip_to_psd's decoding of the same bitmap.
        let Ok(dir) = std::env::var("ATELIET_CLIP_REFERENCE") else { return };
        let chunks = split_chunks(&data).unwrap();
        let db = open_db(chunks.sqlite).unwrap();
        let l1 = db.layers.get(&5).unwrap();
        let (info, blocks) = bitmap(&chunks, &db, int(l1, "LayerRenderMipmap")).unwrap().unwrap();
        let ours = decode_rgba(&info, &blocks).unwrap();
        let file = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().starts_with("chunk_005_")).unwrap();
        let theirs = image::open(file.path()).unwrap().to_rgba8();
        assert_eq!(ours.dimensions(), theirs.dimensions());
        assert!(ours.pixels().zip(theirs.pixels()).all(|(a, b)| a == b));
    }
}
