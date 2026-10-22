//! Pages → PDF: fonts embedded as subsets (CID-keyed, Identity-H, with a
//! ToUnicode map so the text can be searched and copied), pictures as
//! JPEG or deflated RGB with an alpha mask, trim and bleed boxes.

use super::fonts::{FontDb, FontId};
use super::{mul, Item, Page, Picture, PT_MM};
use pdf_writer::types::{CidFontType, FontFlags, FunctionShadingType, LineJoinStyle, MaskType, SystemInfo, TextRenderingMode, UnicodeCmap};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

struct Embedded {
    name: String,
    r: Ref,
    remap: HashMap<u16, u16>,
    upem: f32,
    adv: HashMap<u16, f32>,
}

fn rgb(c: &[u8; 3]) -> (f32, f32, f32) {
    (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
}

/// An opacity as a graphics state, when not opaque.
fn opacity(c: &mut Content, a: f32, used: &mut BTreeSet<u16>) -> bool {
    let k = (a.clamp(0.0, 1.0) * 100.0).round() as u16;
    if k >= 100 {
        return false;
    }
    used.insert(k);
    c.save_state();
    c.set_parameters(Name(format!("G{k}").as_bytes()));
    true
}

fn polys(c: &mut Content, ps: &[Vec<(f32, f32)>], h: f32) {
    for p in ps {
        if p.len() < 2 {
            continue;
        }
        c.move_to(pt(p[0].0), h - pt(p[0].1));
        for q in &p[1..] {
            c.line_to(pt(q.0), h - pt(q.1));
        }
        c.close_path();
    }
}

fn text(c: &mut Content, it: &Item, h: f32, fonts: &HashMap<FontId, Embedded>) {
    let Item::Text { font, size, x, y, glyphs, .. } = it else { return };
    let Some(f) = fonts.get(font) else { return };
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

fn draw(c: &mut Content, items: &[Item], h: f32, fonts: &HashMap<FontId, Embedded>, pics: &HashMap<usize, Ref>, gs: &mut BTreeSet<u16>, grads: &mut Vec<Grad>, alpha: f32) {
    for it in items {
        match it {
            Item::Rect { x, y, w: rw, h: rh, color } => {
                let o = opacity(c, alpha, gs);
                let (r, g, b) = rgb(color);
                c.set_fill_rgb(r, g, b);
                c.rect(pt(*x), h - pt(y + rh), pt(*rw), pt(*rh));
                c.fill_nonzero();
                if o {
                    c.restore_state();
                }
            }
            Item::Line { x0, y0, x1, y1, width, color, dash } => {
                let o = opacity(c, alpha, gs);
                let (r, g, b) = rgb(color);
                c.set_stroke_rgb(r, g, b);
                c.set_line_width(pt(*width));
                match dash {
                    Some((a, b)) => c.set_dash_pattern([pt(*a), pt(*b)], 0.0),
                    None => c.set_dash_pattern([], 0.0),
                };
                c.move_to(pt(*x0), h - pt(*y0));
                c.line_to(pt(*x1), h - pt(*y1));
                c.stroke();
                if o {
                    c.restore_state();
                }
            }
            Item::Image { pic, x, y, w: iw, h: ih } => {
                if pics.contains_key(pic) {
                    let o = opacity(c, alpha, gs);
                    c.save_state();
                    c.transform([pt(*iw), 0.0, 0.0, pt(*ih), pt(*x), h - pt(y + ih)]);
                    c.x_object(Name(format!("I{pic}").as_bytes()));
                    c.restore_state();
                    if o {
                        c.restore_state();
                    }
                }
            }
            Item::Text { color, .. } => {
                let o = opacity(c, alpha, gs);
                let (r, g, b) = rgb(color);
                c.set_fill_rgb(r, g, b);
                text(c, it, h, fonts);
                if o {
                    c.restore_state();
                }
            }
            Item::Outline { text: t, width, color } => {
                let o = opacity(c, alpha, gs);
                c.save_state();
                let (r, g, b) = rgb(color);
                c.set_stroke_rgb(r, g, b);
                c.set_line_width(pt(*width));
                c.set_line_join(LineJoinStyle::RoundJoin);
                c.set_text_rendering_mode(TextRenderingMode::Stroke);
                text(c, t, h, fonts);
                c.restore_state();
                if o {
                    c.restore_state();
                }
            }
            Item::Path { polys: ps, fill, fill_alpha, stroke } => {
                if let Some(f) = fill {
                    let o = opacity(c, alpha * fill_alpha, gs);
                    let (r, g, b) = rgb(f);
                    c.set_fill_rgb(r, g, b);
                    polys(c, ps, h);
                    c.fill_nonzero();
                    if o {
                        c.restore_state();
                    }
                }
                if let Some(s) = stroke {
                    let o = opacity(c, alpha, gs);
                    c.save_state();
                    let (r, g, b) = rgb(&s.color);
                    c.set_stroke_rgb(r, g, b);
                    c.set_line_width(pt(s.width));
                    c.set_line_join(LineJoinStyle::RoundJoin);
                    if let Some((a, b)) = s.dash {
                        c.set_dash_pattern([pt(a), pt(b)], 0.0);
                    }
                    polys(c, ps, h);
                    c.stroke();
                    c.restore_state();
                    if o {
                        c.restore_state();
                    }
                }
            }
            Item::Gradient { x, y, w: gw, h: gh, color, a0, a1, across } => {
                // the colour through a soft mask: a grey ramp from a0 to a1
                let (x0, y0, x1, y1) = (pt(*x), h - pt(y + gh), pt(x + gw), h - pt(*y));
                let coords = if *across { [x0, y1, x1, y1] } else { [x0, y1, x0, y0] };
                grads.push(Grad { bbox: [x0, y0, x1, y1], coords, a0: *a0, a1: *a1 });
                let o = opacity(c, alpha, gs);
                c.save_state();
                c.set_parameters(Name(format!("M{}", grads.len() - 1).as_bytes()));
                let (r, g, b) = rgb(color);
                c.set_fill_rgb(r, g, b);
                c.rect(x0, y0, x1 - x0, y1 - y0);
                c.fill_nonzero();
                c.restore_state();
                if o {
                    c.restore_state();
                }
            }
            Item::Group { m, clip, alpha: ga, items } => {
                c.save_state();
                // the map in PDF space: flip in, map, flip out
                let k = 1.0 / PT_MM;
                let f = [k, 0.0, 0.0, -k, 0.0, h];
                let finv = [1.0 / k, 0.0, 0.0, -1.0 / k, 0.0, h / k];
                c.transform(mul(&f, &mul(m, &finv)));
                if let Some(cl) = clip {
                    polys(c, cl, h);
                    c.clip_nonzero();
                    c.end_path();
                }
                draw(c, items, h, fonts, pics, gs, grads, alpha * ga);
                c.restore_state();
            }
        }
    }
}

/// A gradient's soft mask, in the page's user space (pt).
struct Grad {
    bbox: [f32; 4],
    coords: [f32; 4],
    a0: f32,
    a1: f32,
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
        super::walk(&p.items, &mut |it| {
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
        });
    }
    // fonts
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
    let mut wanted: Vec<usize> = Vec::new();
    for p in pages {
        super::walk(&p.items, &mut |it| {
            if let Item::Image { pic, .. } = it {
                wanted.push(*pic);
            }
        });
    }
    {
        for pic in &wanted {
            {
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
    let mut all_gs: BTreeSet<u16> = BTreeSet::new();
    let mut gs_refs: BTreeMap<u16, Ref> = BTreeMap::new();
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
        let mut used_gs: BTreeSet<u16> = BTreeSet::new();
        let mut grads: Vec<Grad> = Vec::new();
        draw(&mut c, &p.items, h, &fonts, &pic_refs, &mut used_gs, &mut grads, 1.0);
        let mut masks: Vec<Ref> = Vec::new();
        for g in &grads {
            let (gr, fr, sr, fnr) = (ids.next(), ids.next(), ids.next(), ids.next());
            let mut fc = Content::new();
            fc.shading(Name(b"Sh"));
            let bytes = fc.finish();
            let mut fx = pdf.form_xobject(fr, &bytes);
            fx.bbox(Rect::new(g.bbox[0], g.bbox[1], g.bbox[2], g.bbox[3]));
            fx.group().transparency().color_space().device_gray();
            fx.resources().shadings().pair(Name(b"Sh"), sr);
            fx.finish();
            let mut sh = pdf.function_shading(sr);
            sh.shading_type(FunctionShadingType::Axial);
            sh.color_space().device_gray();
            sh.function(fnr);
            sh.coords(g.coords);
            sh.extend([true, true]);
            sh.finish();
            pdf.exponential_function(fnr).domain([0.0, 1.0]).c0([g.a0]).c1([g.a1]).n(1.0);
            pdf.ext_graphics(gr).soft_mask().subtype(MaskType::Luminosity).group(fr);
            masks.push(gr);
        }
        all_gs.extend(used_gs.iter().copied());
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
        if !used_gs.is_empty() || !masks.is_empty() {
            let mut eg = res.ext_g_states();
            for a in &used_gs {
                let r = *gs_refs.entry(*a).or_insert_with(|| ids.next());
                eg.pair(Name(format!("G{a}").as_bytes()), r);
            }
            for (k, r) in masks.iter().enumerate() {
                eg.pair(Name(format!("M{k}").as_bytes()), *r);
            }
            eg.finish();
        }
        res.finish();
        page.finish();
    }
    for (a, r) in &gs_refs {
        let v = *a as f32 / 100.0;
        pdf.ext_graphics(*r).non_stroking_alpha(v).stroking_alpha(v);
    }
    let _ = all_gs;
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
