//! Pages → PDF: fonts embedded as subsets (CID-keyed, Identity-H, with a
//! ToUnicode map so the text can be searched and copied), pictures as
//! JPEG or deflated RGB with an alpha mask, trim and bleed boxes.

use super::fonts::{FontDb, FontId};
use super::{Item, Page, Picture, PT_MM};
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;

pub struct Meta {
    pub title: String,
    pub author: String,
    pub lang: String,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

fn pt(mm: f32) -> f32 {
    mm / PT_MM
}

struct Alloc(i32);

impl Alloc {
    fn next(&mut self) -> Ref {
        self.0 += 1;
        Ref::new(self.0)
    }
}

pub fn write(pages: &[Page], db: &FontDb, pics: &[Picture], meta: &Meta) -> Result<Vec<u8>, String> {
    let mut pdf = Pdf::new();
    let mut ids = Alloc(0);
    let catalog = ids.next();
    let tree = ids.next();
    let info = ids.next();
    // glyphs used by each font, with their text
    let mut used: BTreeMap<FontId, BTreeMap<u16, String>> = BTreeMap::new();
    for p in pages {
        for it in &p.items {
            if let Item::Text { font, glyphs, map, .. } = it {
                let e = used.entry(*font).or_default();
                for g in glyphs {
                    e.entry(g.id).or_default();
                }
                for (g, s) in map {
                    let v = e.entry(*g).or_default();
                    if v.is_empty() {
                        *v = s.clone();
                    }
                }
            }
        }
    }
    // fonts
    struct Embedded {
        name: String,
        r: Ref,
        remap: HashMap<u16, u16>,
        upem: f32,
        adv: HashMap<u16, f32>,
    }
    let mut fonts: HashMap<FontId, Embedded> = HashMap::new();
    for (k, (fid, glyphs)) in used.iter().enumerate() {
        let face = db.face(*fid);
        let data = face.data().ok_or("fonte ilegível")?;
        let tf = ttf_parser::Face::parse(&data, face.index).map_err(|e| e.to_string())?;
        let mut mapper = subsetter::GlyphRemapper::new();
        for g in glyphs.keys() {
            mapper.remap(*g);
        }
        let sub = subsetter::subset(&data, face.index, &mapper).map_err(|e| format!("{e:?}"))?;
        let remap: HashMap<u16, u16> = glyphs.keys().map(|g| (*g, mapper.get(*g).unwrap_or(*g))).collect();
        let adv: HashMap<u16, f32> = glyphs.keys().map(|g| (*g, tf.glyph_hor_advance(ttf_parser::GlyphId(*g)).unwrap_or(0) as f32)).collect();
        let base = format!("{}+{}", ["AAAAAA", "BAAAAA", "CAAAAA", "DAAAAA", "EAAAAA", "FAAAAA", "GAAAAA", "HAAAAA"][k % 8], face.family.replace(|c: char| !c.is_ascii_alphanumeric(), ""));
        let (t0, cid, desc, file, cmap) = (ids.next(), ids.next(), ids.next(), ids.next(), ids.next());
        pdf.type0_font(t0).base_font(Name(base.as_bytes())).encoding_predefined(Name(b"Identity-H")).descendant_font(cid).to_unicode(cmap);
        let scale = 1000.0 / face.upem;
        {
            let mut c = pdf.cid_font(cid);
            c.subtype(if face.cff { CidFontType::Type0 } else { CidFontType::Type2 }).base_font(Name(base.as_bytes())).system_info(SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 }).font_descriptor(desc).default_width(0.0);
            if !face.cff {
                c.cid_to_gid_map_predefined(Name(b"Identity"));
            }
            let mut w = c.widths();
            for (g, a) in &adv {
                w.consecutive(remap[g], [a * scale]);
            }
        }
        let bb = tf.global_bounding_box();
        let mut flags = FontFlags::NON_SYMBOLIC;
        if face.italic {
            flags |= FontFlags::ITALIC;
        }
        let mut fd = pdf.font_descriptor(desc);
        fd.name(Name(base.as_bytes())).flags(flags).bbox(Rect::new(bb.x_min as f32 * scale, bb.y_min as f32 * scale, bb.x_max as f32 * scale, bb.y_max as f32 * scale)).italic_angle(tf.italic_angle()).ascent(face.ascender * scale).descent(face.descender * scale).cap_height(face.cap_height * scale).stem_v(80.0);
        if face.cff {
            fd.font_file3(file);
        } else {
            fd.font_file2(file);
        }
        fd.finish();
        // a CFF subset goes in as its bare CFF table
        let program: Vec<u8> = if face.cff {
            ttf_parser::RawFace::parse(&sub, 0).ok().and_then(|r| r.table(ttf_parser::Tag::from_bytes(b"CFF ")).map(|t| t.to_vec())).unwrap_or(sub.clone())
        } else {
            sub
        };
        let z = deflate(&program);
        let mut s = pdf.stream(file, &z);
        s.filter(Filter::FlateDecode);
        if face.cff {
            s.pair(Name(b"Subtype"), Name(b"CIDFontType0C"));
        }
        s.finish();
        let mut uc = UnicodeCmap::new(Name(b"Custom"), SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"UCS"), supplement: 0 });
        for (g, t) in glyphs {
            if !t.is_empty() {
                uc.pair_with_multiple(remap[g], t.chars());
            }
        }
        pdf.cmap(cmap, &uc.finish());
        fonts.insert(*fid, Embedded { name: format!("F{k}"), r: t0, remap, upem: face.upem, adv });
    }
    // pictures
    let mut pic_refs: HashMap<usize, Ref> = HashMap::new();
    for p in pages {
        for it in &p.items {
            if let Item::Image { pic, .. } = it {
                if pic_refs.contains_key(pic) || *pic >= pics.len() {
                    continue;
                }
                let im = &pics[*pic];
                let r = ids.next();
                if let Some(j) = &im.jpeg {
                    let mut x = pdf.image_xobject(r, j);
                    x.width(im.w as i32);
                    x.height(im.h as i32);
                    x.color_space().device_rgb();
                    x.filter(Filter::DctDecode);
                    x.bits_per_component(8);
                } else {
                    let rgb: Vec<u8> = im.rgba.chunks(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
                    let alpha: Vec<u8> = im.rgba.chunks(4).map(|c| c[3]).collect();
                    let mask = alpha.iter().any(|a| *a < 255).then(|| ids.next());
                    let z = deflate(&rgb);
                    let mut x = pdf.image_xobject(r, &z);
                    x.width(im.w as i32);
                    x.height(im.h as i32);
                    x.color_space().device_rgb();
                    x.filter(Filter::FlateDecode);
                    x.bits_per_component(8);
                    if let Some(m) = mask {
                        x.s_mask(m);
                    }
                    x.finish();
                    if let Some(m) = mask {
                        let za = deflate(&alpha);
                        let mut a = pdf.image_xobject(m, &za);
                        a.width(im.w as i32);
                        a.height(im.h as i32);
                        a.color_space().device_gray();
                        a.filter(Filter::FlateDecode);
                        a.bits_per_component(8);
                    }
                }
                pic_refs.insert(*pic, r);
            }
        }
    }
    // pages
    let mut kids = Vec::new();
    for p in pages {
        let (pr, cr) = (ids.next(), ids.next());
        kids.push(pr);
        let (w, h) = (pt(p.w), pt(p.h));
        let mut c = Content::new();
        if p.paper != [255, 255, 255] {
            c.set_fill_rgb(p.paper[0] as f32 / 255.0, p.paper[1] as f32 / 255.0, p.paper[2] as f32 / 255.0);
            c.rect(0.0, 0.0, w, h);
            c.fill_nonzero();
        }
        for it in &p.items {
            match it {
                Item::Rect { x, y, w: rw, h: rh, color } => {
                    c.set_fill_rgb(color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0);
                    c.rect(pt(*x), h - pt(y + rh), pt(*rw), pt(*rh));
                    c.fill_nonzero();
                }
                Item::Line { x0, y0, x1, y1, width, color, dash } => {
                    c.set_stroke_rgb(color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0);
                    c.set_line_width(pt(*width));
                    match dash {
                        Some((a, b)) => c.set_dash_pattern([pt(*a), pt(*b)], 0.0),
                        None => c.set_dash_pattern([], 0.0),
                    };
                    c.move_to(pt(*x0), h - pt(*y0));
                    c.line_to(pt(*x1), h - pt(*y1));
                    c.stroke();
                }
                Item::Image { pic, x, y, w: iw, h: ih } => {
                    if let Some(r) = pic_refs.get(pic) {
                        let _ = r;
                        c.save_state();
                        c.transform([pt(*iw), 0.0, 0.0, pt(*ih), pt(*x), h - pt(y + ih)]);
                        c.x_object(Name(format!("I{pic}").as_bytes()));
                        c.restore_state();
                    }
                }
                Item::Text { font, size, x, y, color, glyphs, .. } => {
                    let Some(f) = fonts.get(font) else { continue };
                    c.set_fill_rgb(color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0);
                    c.begin_text();
                    c.set_font(Name(f.name.as_bytes()), *size);
                    // runs of glyphs on the same baseline, spaced with TJ adjustments
                    let mut i = 0;
                    while i < glyphs.len() {
                        let y0 = glyphs[i].y;
                        c.set_text_matrix([1.0, 0.0, 0.0, 1.0, pt(x + glyphs[i].x), h - pt(y + y0)]);
                        let mut pen = glyphs[i].x;
                        let mut tj = c.show_positioned();
                        let mut items = tj.items();
                        while i < glyphs.len() && glyphs[i].y == y0 {
                            let g = &glyphs[i];
                            let gap = g.x - pen;
                            if gap.abs() > 1e-4 {
                                items.adjust(-gap / PT_MM / size * 1000.0);
                            }
                            let cid = f.remap.get(&g.id).copied().unwrap_or(0);
                            items.show(Str(&cid.to_be_bytes()));
                            pen = g.x + f.adv.get(&g.id).copied().unwrap_or(0.0) / f.upem * size * PT_MM;
                            i += 1;
                        }
                        items.finish();
                        tj.finish();
                    }
                    c.end_text();
                }
            }
        }
        let data = deflate(&c.finish());
        pdf.stream(cr, &data).filter(Filter::FlateDecode);
        let mut page = pdf.page(pr);
        page.parent(tree).media_box(Rect::new(0.0, 0.0, w, h)).contents(cr);
        let (tx, ty, tw, th) = p.trim;
        if (tw, th) != (p.w, p.h) {
            page.trim_box(Rect::new(pt(tx), h - pt(ty + th), pt(tx + tw), h - pt(ty)));
            page.bleed_box(Rect::new(0.0, 0.0, w, h));
        }
        let mut res = page.resources();
        let mut fd = res.fonts();
        for f in fonts.values() {
            fd.pair(Name(f.name.as_bytes()), f.r);
        }
        fd.finish();
        let mut xo = res.x_objects();
        for (k, r) in &pic_refs {
            xo.pair(Name(format!("I{k}").as_bytes()), *r);
        }
        xo.finish();
        res.finish();
        page.finish();
    }
    let n = kids.len() as i32;
    pdf.pages(tree).kids(kids).count(n);
    let mut cat = pdf.catalog(catalog);
    cat.pages(tree);
    if !meta.lang.is_empty() {
        cat.lang(TextStr(&meta.lang));
    }
    cat.finish();
    pdf.document_info(info).title(TextStr(&meta.title)).author(TextStr(&meta.author)).creator(TextStr("Ateliet"));
    Ok(pdf.finish())
}
