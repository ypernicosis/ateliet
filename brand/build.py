#!/usr/bin/env python3
"""Builds the ateliet mark, wordmark and app icons from geometry.

    pip install fonttools uharfbuzz skia-pathops cairosvg pillow
    python3 brand/build.py            # writes brand/*.svg, icons/icon.png, icons/icon.icns

The mark is a manga page (the inner frame, inked) with two gutters cut into it:
an inset round panel and one slanted cut that runs out through the bottom edge.
Together they draw a single-storey "a"; the round panel is still in non-photo blue.
The wordmark is Newsreader Italic (SIL OFL 1.1), outlined, so no font ships with the app.
"""
import io, math, os, re, sys, urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "brand"
CACHE = Path(os.environ.get("ATELIET_FONT_CACHE", Path.home() / ".cache" / "ateliet-brand"))

PAPER = "#F4F2EC"
INK = "#18181C"
BLUE = "#9BD3EA"   # non-photo blue, the colour of underdrawing
GUIDE = "#86C6E3"  # the printed guides of manuscript paper

# ── geometry (1024 canvas; macOS tile 824 at 100,100) ─────────────────────────

FRAME = (232, 232, 792, 792)  # inner frame (基本枠): the inked page
SLANT = 13.0                  # stem angle, the stem angle of Newsreader Italic's "l"


def squircle(cx=512, cy=512, r=412, n=5.0, steps=240):
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        pts.append((cx + r * math.copysign(abs(c) ** (2 / n), c), cy + r * math.copysign(abs(s) ** (2 / n), s)))
    return "M" + " L".join(f"{x:.2f},{y:.2f}" for x, y in pts) + "Z"


def a_geometry(frame, r, g, gs, slant=SLANT, cy_f=0.56):
    """Ring (centre, radius) and stem polygon. The stem's inner edge is tangent to the
    counter; its top is cut level with the top of the ring; it runs out through the
    bottom of the frame. The glyph is centred horizontally on the frame."""
    x0, y0, x1, y1 = frame
    th = math.radians(slant)
    n = (math.cos(th), math.sin(th))   # normal, pointing right
    d = (-math.sin(th), math.cos(th))  # down the stem
    cy = y0 + cy_f * (y1 - y0)
    k = r - g / 2 + gs / 2
    ytop = cy - r - g / 2
    ybot = y1 + 300

    def stem(cx):
        p = (cx + k * n[0], cy + k * n[1])

        def at(y, side):
            q = (p[0] + side * n[0] * gs / 2, p[1] + side * n[1] * gs / 2)
            t = (y - q[1]) / d[1]
            return (q[0] + t * d[0], y)
        return [at(ytop, -1), at(ytop, 1), at(ybot, 1), at(ybot, -1)]

    cx = (x0 + x1) / 2
    for _ in range(40):
        right = max(stem(cx)[1][0], cx + r + g / 2)
        cx += ((x0 + x1) / 2 - (cx - r - g / 2 + right) / 2) * 0.8
    return cx, cy, stem(cx)


def _circle(pen, cx, cy, r):
    k = 0.5522847498 * r
    pen.moveTo((cx + r, cy))
    pen.curveTo((cx + r, cy + k), (cx + k, cy + r), (cx, cy + r))
    pen.curveTo((cx - k, cy + r), (cx - r, cy + k), (cx - r, cy))
    pen.curveTo((cx - r, cy - k), (cx - k, cy - r), (cx, cy - r))
    pen.curveTo((cx + k, cy - r), (cx + r, cy - k), (cx + r, cy))
    pen.closePath()


def _poly(pen, pts):
    pen.moveTo(pts[0])
    for q in pts[1:]:
        pen.lineTo(q)
    pen.closePath()


def _num(v):
    return f"{v:.2f}".rstrip("0").rstrip(".")


def _d(path):
    from fontTools.pens.svgPathPen import SVGPathPen
    pen = SVGPathPen(None, ntos=_num)
    path.draw(pen)
    return pen.getCommands()


def page_paths(frame=FRAME, r=160, g=44, gs=58):
    """Boolean geometry of the page: (ink, counter, gutters), each as SVG path data.
    ink = frame minus the gutters and the round panel; counter = the round panel;
    gutters = the paper inside the frame (ring and stem)."""
    import pathops
    x0, y0, x1, y1 = frame
    cx, cy, st = a_geometry(frame, r, g, gs)

    def shape(fn, *a):
        p = pathops.Path()
        fn(p.getPen(), *a)
        return p
    rect = shape(_poly, [(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
    outer = shape(_circle, cx, cy, r + g / 2)
    inner = shape(_circle, cx, cy, r - g / 2)
    stem = shape(_poly, st)
    op, Op = pathops.op, pathops.PathOp
    cut = op(outer, stem, Op.UNION)
    ink = op(rect, cut, Op.DIFFERENCE)
    counter = op(inner, stem, Op.DIFFERENCE)
    gutters = op(op(cut, inner, Op.DIFFERENCE), rect, Op.INTERSECTION)
    return _d(ink), _d(counter), _d(gutters)


def page(frame=FRAME, r=160, g=44, gs=58, ink=INK, blue=BLUE, gutter=PAPER, mono=False):
    """The inked page with the 'a' cut into it. mono: one colour, the counter inked
    and the gutters left open."""
    i, c, gt = page_paths(frame, r, g, gs)
    if mono:
        return f'<path d="{i}{c}" fill="{ink}"/>'
    return (f'<path d="{gt}" fill="{gutter}"/>' if gutter else "") + f'<path d="{i}" fill="{ink}"/><path d="{c}" fill="{blue}"/>'


def guides(frame=FRAME, trim=58, w=3.5, color=GUIDE):
    x0, y0, x1, y1 = frame
    return (f'<rect x="{x0 - trim}" y="{y0 - trim}" width="{x1 - x0 + 2 * trim}" height="{y1 - y0 + 2 * trim}" '
            f'fill="none" stroke="{color}" stroke-width="{w}"/>')


def tile(uid):
    return (f'<defs><linearGradient id="p{uid}" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="#F9F7F2"/><stop offset="1" stop-color="#EFECE5"/></linearGradient></defs>'
            f'<path d="{squircle()}" fill="url(#p{uid})"/>')


def svg(inner, w=1024, h=1024, vb=None):
    vb = vb or f"0 0 {w} {h}"
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" width="{w}" height="{h}">{inner}</svg>\n'


def icon_large():
    return svg(tile("L") + guides() + page(gutter=None))


def icon_small():
    # optical size for 16–64 px: bigger page, heavier gutters, no guides
    fr = (184, 184, 840, 840)
    return svg(tile("S") + page(frame=fr, r=186, g=68, gs=88, gutter=None))


def mark(mono=False):
    x0, y0, x1, y1 = FRAME
    return svg(page(mono=mono, ink="currentColor" if mono else INK), w=560, h=560,
               vb=f"{x0} {y0} {x1 - x0} {y1 - y0}")


# ── wordmark ─────────────────────────────────────────────────────────────────

FONTS = {
    # Newsreader Italic at two optical sizes: display for large settings, text for small ones
    "display": "Newsreader:ital,opsz,wght@1,72,400",
    "text": "Newsreader:ital,opsz,wght@1,16,400",
}


def font_file(key):
    CACHE.mkdir(parents=True, exist_ok=True)
    path = CACHE / f"newsreader-{key}.ttf"
    if not path.exists():
        css = urllib.request.urlopen(f"https://fonts.googleapis.com/css2?family={FONTS[key]}").read().decode()
        url = re.search(r"url\((https://[^)]+)\)", css).group(1)
        path.write_bytes(urllib.request.urlopen(url).read())
    return path


def text_path(key, text, size, x=0.0, y=0.0, tracking=0.0):
    """Shaped (kerned) text as SVG path data, baseline at (x, y). Returns (d, advance, metrics)."""
    from fontTools.ttLib import TTFont
    from fontTools.pens.svgPathPen import SVGPathPen
    from fontTools.pens.transformPen import TransformPen
    import uharfbuzz as hb
    path = font_file(key)
    f = TTFont(path)
    gs, upm, order = f.getGlyphSet(), f["head"].unitsPerEm, f.getGlyphOrder()
    font = hb.Font(hb.Face(hb.Blob.from_file_path(str(path))))
    buf = hb.Buffer()
    buf.add_str(text)
    buf.guess_segment_properties()
    hb.shape(font, buf, {"kern": True})
    from fontTools.pens.boundsPen import BoundsPen
    s, pen, bp, adv = size / upm, SVGPathPen(gs, ntos=_num), BoundsPen(gs), 0
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        t = (s, 0, 0, -s, x + (adv + pos.x_offset) * s, y - pos.y_offset * s)
        gs[order[info.codepoint]].draw(TransformPen(pen, t))
        gs[order[info.codepoint]].draw(TransformPen(bp, t))
        adv += pos.x_advance + tracking * upm / 1000
    adv -= tracking * upm / 1000
    os2 = f["OS/2"]
    return pen.getCommands(), adv * s, {"cap": os2.sCapHeight * s, "xh": os2.sxHeight * s, "bounds": bp.bounds}


def _fit(inner, bounds, pad):
    x0, y0, x1, y1 = bounds
    x0, y0, x1, y1 = x0 - pad, y0 - pad, x1 + pad, y1 + pad
    return svg(inner, w=round(x1 - x0), h=round(y1 - y0), vb=f"{x0:.1f} {y0:.1f} {x1 - x0:.1f} {y1 - y0:.1f}")


def wordmark(key="display", color="currentColor"):
    d, w, m = text_path(key, "ateliet", 200, tracking=-5 if key == "display" else 10)
    return _fit(f'<path d="{d}" fill="{color}"/>', m["bounds"], 4)


def lockup(dark=False):
    """Mark + wordmark: the page as tall as the wordmark's capitals, sitting on the baseline.
    On dark grounds the page keeps a margin of paper around it, the sheet it is drawn on."""
    d, w, m = text_path("display", "ateliet", 200)
    side = m["cap"] * 1.04
    gap = side * 0.42
    k = side / 560
    mk = f'<g transform="translate(0,{-side:.2f}) scale({k:.5f}) translate(-232,-232)">{page()}</g>'
    sheet = ""
    if dark:
        e = side * 0.1
        sheet = f'<rect x="{-e:.2f}" y="{-side - e:.2f}" width="{side + 2 * e:.2f}" height="{side + 2 * e:.2f}" rx="{e * 0.5:.2f}" fill="{PAPER}"/>'
    tx = side + gap + (side * 0.1 if dark else 0)
    text = f'<path transform="translate({tx:.2f},0)" d="{d}" fill="{PAPER if dark else INK}"/>'
    x0, y0, x1, y1 = m["bounds"]
    e = side * 0.1 if dark else 0
    bounds = (-e, min(y0, -side - e), tx + x1, max(y1, e))
    return _fit(sheet + mk + text, bounds, 4)


# ── raster ───────────────────────────────────────────────────────────────────

def raster(svg_text, size, shadow=True):
    import cairosvg
    from PIL import Image, ImageFilter
    scale = 4 if size <= 64 else 1  # supersample small sizes, then downsample
    big = size * scale
    im = Image.open(io.BytesIO(cairosvg.svg2png(bytestring=svg_text.encode(), output_width=big, output_height=big))).convert("RGBA")
    if shadow:
        a = im.getchannel("A")
        sh = Image.new("RGBA", im.size, (0, 0, 0, 0))
        k = big / 1024
        blur = a.filter(ImageFilter.GaussianBlur(14 * k)).point(lambda v: int(v * 0.30))
        sh.paste((0, 0, 0, 255), (0, round(8 * k)), blur)
        sh.alpha_composite(im)
        im = sh
    return im.resize((size, size), Image.LANCZOS) if scale > 1 else im


def main():
    from PIL import Image
    files = {
        "icon.svg": icon_large(),
        "icon-small.svg": icon_small(),
        "mark.svg": mark(),
        "mark-mono.svg": mark(mono=True),
        "wordmark.svg": wordmark("display"),
        "wordmark-text.svg": wordmark("text"),
        "lockup.svg": lockup(),
        "lockup-dark.svg": lockup(dark=True),
    }
    for name, text in files.items():
        (BRAND / name).write_text(text)
    large, small = files["icon.svg"], files["icon-small.svg"]
    png = raster(large, 1024)
    png.save(ROOT / "icons" / "icon.png", optimize=True)
    sizes = {16: small, 32: small, 64: small, 128: large, 256: large, 512: large, 1024: large}
    imgs = [raster(s, n) for n, s in sizes.items()]
    imgs[-1].save(ROOT / "icons" / "icon.icns", append_images=imgs[:-1])
    print("wrote", ", ".join(files), "icons/icon.png", "icons/icon.icns")


if __name__ == "__main__":
    sys.exit(main())
