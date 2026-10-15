// Pages: what the page kinds (thumbnails, name, comic, webtoon) add over
// frames. Panels are polygons on the page; balloons are boxes with text.
// Here: their geometry (point inside, cutting a panel in two along a line
// with a gutter, layouts, reading order) and how they are drawn.

import { Balloon, BalloonKind, Board, Frame, PageSetup, Panel, uid } from "./model.ts";

export type Pt = { x: number; y: number };
export type Rect = { x: number; y: number; w: number; h: number };

export function rectPanel(r: Rect): Panel {
  return { id: uid(), points: [{ x: r.x, y: r.y }, { x: r.x + r.w, y: r.y }, { x: r.x + r.w, y: r.y + r.h }, { x: r.x, y: r.y + r.h }] };
}

export function inside(poly: Pt[], p: Pt): boolean {
  let c = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i], b = poly[j];
    if ((a.y > p.y) !== (b.y > p.y) && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) c = !c;
  }
  return c;
}

export function bounds(poly: Pt[]): Rect {
  const xs = poly.map((p) => p.x), ys = poly.map((p) => p.y);
  const x = Math.min(...xs), y = Math.min(...ys);
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
}

export function polyPath(poly: Pt[]): Path2D {
  const p = new Path2D();
  poly.forEach((q, i) => (i ? p.lineTo(q.x, q.y) : p.moveTo(q.x, q.y)));
  p.closePath();
  return p;
}

/** Keeps the part of a (convex) polygon where n·(p − a) ≥ d (Sutherland–Hodgman). */
function clipHalf(poly: Pt[], a: Pt, n: Pt, d: number): Pt[] {
  const side = (p: Pt) => n.x * (p.x - a.x) + n.y * (p.y - a.y) - d;
  const out: Pt[] = [];
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i], q = poly[(i + 1) % poly.length];
    const sp = side(p), sq = side(q);
    if (sp >= 0) out.push(p);
    if ((sp >= 0) !== (sq >= 0)) {
      const t = sp / (sp - sq);
      out.push({ x: p.x + (q.x - p.x) * t, y: p.y + (q.y - p.y) * t });
    }
  }
  return out;
}

function area(poly: Pt[]): number {
  let s = 0;
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i], q = poly[(i + 1) % poly.length];
    s += p.x * q.y - q.x * p.y;
  }
  return Math.abs(s / 2);
}

/** True when the line a→b passes through the polygon (it has vertices on both sides). */
export function lineCuts(poly: Pt[], a: Pt, b: Pt): boolean {
  const n = { x: -(b.y - a.y), y: b.x - a.x };
  let pos = false, neg = false;
  for (const p of poly) {
    const s = n.x * (p.x - a.x) + n.y * (p.y - a.y);
    if (s > 1e-6) pos = true;
    if (s < -1e-6) neg = true;
  }
  if (!pos || !neg) return false;
  // The stroke must actually cross the panel, not just its extension.
  const r = bounds(poly);
  const segMinX = Math.min(a.x, b.x), segMaxX = Math.max(a.x, b.x), segMinY = Math.min(a.y, b.y), segMaxY = Math.max(a.y, b.y);
  return segMaxX >= r.x && segMinX <= r.x + r.w && segMaxY >= r.y && segMinY <= r.y + r.h;
}

/** Cuts a panel along the line a→b, leaving `gap` between the two halves. */
export function cutPanel(panel: Panel, a: Pt, b: Pt, gap: number): Panel[] {
  const len = Math.hypot(b.x - a.x, b.y - a.y) || 1;
  const n = { x: -(b.y - a.y) / len, y: (b.x - a.x) / len };
  const one = clipHalf(panel.points, a, n, gap / 2);
  const two = clipHalf(panel.points, a, { x: -n.x, y: -n.y }, gap / 2);
  const out: Panel[] = [];
  for (const pts of [one, two]) if (pts.length >= 3 && area(pts) > 100) out.push({ id: uid(), points: pts.map((p) => ({ x: Math.round(p.x * 10) / 10, y: Math.round(p.y * 10) / 10 })) });
  return out.length === 2 ? out : [panel];
}

/** Panels in reading order: rows top to bottom, each row left→right (or right→left). */
export function readingOrder(panels: Panel[], rtl: boolean): Panel[] {
  const items = panels.map((p) => ({ p, r: bounds(p.points) }));
  items.sort((a, b) => a.r.y - b.r.y);
  const rows: (typeof items)[] = [];
  for (const it of items) {
    const row = rows[rows.length - 1];
    // Same row when it starts above the middle of the row's first panel.
    if (row && it.r.y < row[0].r.y + row[0].r.h * 0.5) row.push(it);
    else rows.push([it]);
  }
  return rows.flatMap((row) => row.sort((a, b) => (rtl ? b.r.x + b.r.w - (a.r.x + a.r.w) : a.r.x - b.r.x)).map((it) => it.p));
}

// ── layouts ───────────────────────────────────────────────────────────────

/** Rows, each with a relative height and relative column widths. */
type RowSpec = { h: number; cols: number[] };

export const LAYOUTS: { id: string; name: string; rows: RowSpec[] }[] = [
  { id: "one", name: "One panel", rows: [{ h: 1, cols: [1] }] },
  { id: "two", name: "Two tiers", rows: [{ h: 1, cols: [1] }, { h: 1, cols: [1] }] },
  { id: "three", name: "Three tiers", rows: [{ h: 1, cols: [1] }, { h: 1, cols: [1] }, { h: 1, cols: [1] }] },
  { id: "three-split", name: "Three tiers, middle split", rows: [{ h: 1, cols: [1] }, { h: 1, cols: [1, 1] }, { h: 1, cols: [1] }] },
  { id: "classic", name: "Six panels (2 × 3)", rows: [{ h: 1, cols: [1, 1] }, { h: 1, cols: [1, 1] }, { h: 1, cols: [1, 1] }] },
  { id: "nine", name: "Nine panels (3 × 3)", rows: [{ h: 1, cols: [1, 1, 1] }, { h: 1, cols: [1, 1, 1] }, { h: 1, cols: [1, 1, 1] }] },
  { id: "big-top", name: "Big panel, two below", rows: [{ h: 1.6, cols: [1] }, { h: 1, cols: [1, 1] }] },
  { id: "big-bottom", name: "Two above, big panel", rows: [{ h: 1, cols: [1, 1] }, { h: 1.6, cols: [1] }] },
  { id: "action", name: "Wide, three narrow, wide", rows: [{ h: 1, cols: [1] }, { h: 1.2, cols: [1, 1, 1] }, { h: 1, cols: [1] }] },
  { id: "yonkoma", name: "4-koma (four stacked)", rows: [{ h: 1, cols: [1] }, { h: 1, cols: [1] }, { h: 1, cols: [1] }, { h: 1, cols: [1] }] },
];

export function layoutPanels(inner: Rect, rows: RowSpec[], gx: number, gy: number, rtl: boolean): Panel[] {
  const out: Panel[] = [];
  const totalH = rows.reduce((n, r) => n + r.h, 0);
  const usableH = inner.h - gy * (rows.length - 1);
  let y = inner.y;
  for (const row of rows) {
    const h = (usableH * row.h) / totalH;
    const totalW = row.cols.reduce((n, c) => n + c, 0);
    const usableW = inner.w - gx * (row.cols.length - 1);
    const cols = rtl ? [...row.cols].reverse() : row.cols;
    let x = inner.x;
    for (const c of cols) {
      const w = (usableW * c) / totalW;
      out.push(rectPanel({ x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) }));
      x += w + gx;
    }
    y += h + gy;
  }
  return out;
}

/** A grid of panels of one aspect (thumbnails), centred in the inner frame, with room for numbers. */
export function gridPanels(inner: Rect, cols: number, rows: number, aspect: number, gap: number, label: number): Panel[] {
  const cw = (inner.w - gap * (cols - 1)) / cols;
  const ch = (inner.h - gap * (rows - 1)) / rows - label;
  let w = cw, h = cw / aspect;
  if (h > ch) {
    h = ch;
    w = ch * aspect;
  }
  const totalW = cols * w + (cols - 1) * gap;
  const totalH = rows * (h + label) + (rows - 1) * gap;
  const x0 = inner.x + (inner.w - totalW) / 2, y0 = inner.y + (inner.h - totalH) / 2;
  const out: Panel[] = [];
  for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) out.push(rectPanel({ x: Math.round(x0 + c * (w + gap)), y: Math.round(y0 + r * (h + label + gap)), w: Math.round(w), h: Math.round(h) }));
  return out;
}

/** How many cells of an aspect fit a sheet nicely. */
export function gridFor(inner: Rect, aspect: number): { cols: number; rows: number } {
  let best = { cols: 3, rows: 3, score: -1 };
  for (let cols = 2; cols <= 6; cols++) {
    for (let rows = 2; rows <= 6; rows++) {
      const cw = inner.w / cols, ch = inner.h / rows;
      const w = Math.min(cw, ch * aspect);
      // Prefer about 9–16 cells that fill the sheet.
      const fill = (w * (w / aspect) * cols * rows) / (inner.w * inner.h);
      const n = cols * rows;
      const score = fill - Math.abs(n - 12) * 0.02;
      if (score > best.score) best = { cols, rows, score };
    }
  }
  return { cols: best.cols, rows: best.rows };
}

// ── drawing ───────────────────────────────────────────────────────────────

export const BALLOON_KINDS: [BalloonKind, string, string][] = [
  ["speech", "message-circle", "Speech"],
  ["thought", "cloud", "Thought"],
  ["shout", "zap", "Shout"],
  ["whisper", "message-circle-dashed", "Whisper"],
  ["narration", "rectangle-horizontal", "Caption box"],
  ["sfx", "type", "Sound effect"],
];

export function newBalloon(kind: BalloonKind, r: Rect, vertical: boolean, size: number): Balloon {
  return { id: uid(), kind, x: r.x, y: r.y, w: Math.max(40, r.w), h: Math.max(30, r.h), text: "", vertical, size, tail: kind === "speech" || kind === "thought" || kind === "shout" || kind === "whisper" ? { x: r.x + r.w * 0.3, y: r.y + r.h + Math.max(30, r.h * 0.4) } : null };
}

const FONT = `"Hiragino Sans", "Hiragino Kaku Gothic ProN", "Noto Sans CJK JP", "Yu Gothic", "Meiryo", system-ui, sans-serif`;

/** Panels (paper outside them, borders), balloons, and reading-order numbers. */
export function drawPage(ctx: CanvasRenderingContext2D, b: Board, f: Frame, o: { numbers: boolean; px: number }) {
  const page = b.page;
  if (!page) return;
  const panels = f.panels || [];
  if (panels.length && page.mask) {
    ctx.save();
    const p = new Path2D();
    p.rect(-1, -1, b.width + 2, b.height + 2);
    for (const pn of panels) p.addPath(polyPath(pn.points));
    if (b.background === "transparent") {
      ctx.globalCompositeOperation = "destination-out";
      ctx.fillStyle = "#000";
    } else ctx.fillStyle = b.background;
    ctx.fill(p, "evenodd");
    ctx.restore();
  }
  if (panels.length && page.border > 0) {
    ctx.save();
    ctx.lineWidth = page.border;
    ctx.lineJoin = "miter";
    ctx.strokeStyle = "#111";
    for (const pn of panels) ctx.stroke(polyPath(pn.points));
    ctx.restore();
  }
  for (const bl of f.balloons || []) drawBalloon(ctx, bl);
  if (o.numbers && panels.length > 1) {
    const order = readingOrder(panels, page.rtl);
    ctx.save();
    const s = Math.max(12, Math.min(b.width, b.height) / 60);
    ctx.font = `600 ${s}px system-ui, sans-serif`;
    ctx.textBaseline = "top";
    order.forEach((pn, i) => {
      const r = bounds(pn.points);
      const label = String(i + 1) + (pn.star ? " ★" : "");
      // Thumbnails: under the panel; pages: inside its corner, on a white chip.
      if (b.kind === "thumbnails") {
        ctx.fillStyle = pn.star ? "#c77d00" : "#555";
        ctx.fillText(label, r.x, r.y + r.h + s * 0.35);
      } else {
        const w = ctx.measureText(label).width + s * 0.6;
        const x = page.rtl ? r.x + r.w - w - s * 0.3 : r.x + s * 0.3;
        ctx.fillStyle = "rgba(255,255,255,.85)";
        ctx.fillRect(x, r.y + s * 0.3, w, s * 1.3);
        ctx.fillStyle = "#2f6fd6";
        ctx.fillText(label, x + s * 0.3, r.y + s * 0.45);
      }
    });
    ctx.restore();
  }
  void o.px;
}

function ellipsePath(bl: Balloon): Path2D {
  const p = new Path2D();
  p.ellipse(bl.x + bl.w / 2, bl.y + bl.h / 2, bl.w / 2, bl.h / 2, 0, 0, Math.PI * 2);
  return p;
}

function tailPath(bl: Balloon, width: number): Path2D | null {
  if (!bl.tail) return null;
  const cx = bl.x + bl.w / 2, cy = bl.y + bl.h / 2;
  const ang = Math.atan2(bl.tail.y - cy, bl.tail.x - cx);
  const base = Math.min(bl.w, bl.h) * width;
  const nx = -Math.sin(ang), ny = Math.cos(ang);
  // The base sits a little inside the balloon so the fill hides the seam.
  const bx = cx + Math.cos(ang) * (bl.w / 2) * 0.7, by = cy + Math.sin(ang) * (bl.h / 2) * 0.7;
  const p = new Path2D();
  p.moveTo(bx + nx * base, by + ny * base);
  p.lineTo(bl.tail.x, bl.tail.y);
  p.lineTo(bx - nx * base, by - ny * base);
  p.closePath();
  return p;
}

export function drawBalloon(ctx: CanvasRenderingContext2D, bl: Balloon) {
  ctx.save();
  const lw = Math.max(1.5, bl.size / 9);
  ctx.lineJoin = "round";
  ctx.strokeStyle = "#111";
  ctx.fillStyle = "#fff";
  const outline = (shapes: Path2D[], dashed = false) => {
    // Stroke everything, then fill everything: the inside seams disappear.
    ctx.lineWidth = lw * 2;
    if (dashed) ctx.setLineDash([lw * 3, lw * 2.5]);
    for (const s of shapes) ctx.stroke(s);
    ctx.setLineDash([]);
    for (const s of shapes) ctx.fill(s);
  };
  if (bl.kind === "speech" || bl.kind === "whisper") {
    const shapes = [ellipsePath(bl)];
    const t = tailPath(bl, 0.12);
    if (t) shapes.unshift(t);
    outline(shapes, bl.kind === "whisper");
  } else if (bl.kind === "thought") {
    const shapes: Path2D[] = [];
    const cx = bl.x + bl.w / 2, cy = bl.y + bl.h / 2, n = 11;
    for (let i = 0; i < n; i++) {
      const a = (i / n) * Math.PI * 2;
      const p = new Path2D();
      const r = Math.min(bl.w, bl.h) * 0.2;
      p.ellipse(cx + Math.cos(a) * (bl.w / 2 - r * 0.6), cy + Math.sin(a) * (bl.h / 2 - r * 0.6), r, r * 0.85, 0, 0, Math.PI * 2);
      shapes.push(p);
    }
    const core = new Path2D();
    core.ellipse(cx, cy, bl.w * 0.42, bl.h * 0.4, 0, 0, Math.PI * 2);
    shapes.push(core);
    if (bl.tail) {
      for (let k = 1; k <= 3; k++) {
        const t = k / 4, r = Math.min(bl.w, bl.h) * (0.09 - k * 0.018);
        const p = new Path2D();
        p.ellipse(cx + (bl.tail.x - cx) * (0.55 + t * 0.45), cy + (bl.tail.y - cy) * (0.55 + t * 0.45), Math.max(3, r), Math.max(3, r * 0.8), 0, 0, Math.PI * 2);
        shapes.push(p);
      }
    }
    outline(shapes);
  } else if (bl.kind === "shout") {
    const p = new Path2D();
    const cx = bl.x + bl.w / 2, cy = bl.y + bl.h / 2, n = 18;
    for (let i = 0; i <= n * 2; i++) {
      const a = (i / (n * 2)) * Math.PI * 2;
      const k = i % 2 ? 0.78 : 1.08;
      const x = cx + Math.cos(a) * (bl.w / 2) * k, y = cy + Math.sin(a) * (bl.h / 2) * k;
      i ? p.lineTo(x, y) : p.moveTo(x, y);
    }
    p.closePath();
    const shapes = [p];
    const t = tailPath(bl, 0.1);
    if (t) shapes.unshift(t);
    outline(shapes);
  } else if (bl.kind === "narration") {
    const p = new Path2D();
    p.rect(bl.x, bl.y, bl.w, bl.h);
    ctx.lineWidth = lw;
    ctx.fill(p);
    ctx.stroke(p);
  }
  drawText(ctx, bl);
  ctx.restore();
}

/** Text inside a balloon: wrapped lines, or vertical columns right to left. */
function drawText(ctx: CanvasRenderingContext2D, bl: Balloon) {
  const text = bl.text.trim();
  if (!text) return;
  const sfx = bl.kind === "sfx";
  const size = bl.size;
  ctx.font = `${sfx || bl.kind === "shout" ? "800" : "500"} ${size}px ${FONT}`;
  ctx.fillStyle = "#111";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  // Room inside the shape (an ellipse leaves about 70% of its box).
  const k = bl.kind === "narration" || sfx ? 0.9 : 0.72;
  const iw = bl.w * k, ih = bl.h * k;
  const cx = bl.x + bl.w / 2, cy = bl.y + bl.h / 2;
  const paragraphs = text.split("\n");
  if (bl.vertical) {
    const lineH = size * 1.08, colW = size * 1.3;
    const perCol = Math.max(1, Math.floor(ih / lineH));
    const cols: string[] = [];
    for (const para of paragraphs) {
      const chars = [...para];
      if (!chars.length) cols.push("");
      for (let i = 0; i < chars.length; i += perCol) cols.push(chars.slice(i, i + perCol).join(""));
    }
    const totalW = cols.length * colW;
    cols.forEach((col, i) => {
      const x = cx + totalW / 2 - colW * (i + 0.5);
      const chars = [...col];
      const top = cy - (chars.length * lineH) / 2 + lineH / 2;
      chars.forEach((ch, j) => {
        const y = top + j * lineH;
        // Long vowel marks and dashes turn upright in vertical text.
        if ("ー—–-〜~…".includes(ch)) {
          ctx.save();
          ctx.translate(x, y);
          ctx.rotate(Math.PI / 2);
          stroked(ctx, ch, 0, 0, sfx, size);
          ctx.restore();
        } else stroked(ctx, ch, x, y, sfx, size);
      });
    });
    return;
  }
  const lines: string[] = [];
  for (const para of paragraphs) {
    // Words, or single characters for scripts without spaces (CJK).
    const tokens = /\s/.test(para) ? para.split(/(\s+)/) : [...para];
    let cur = "";
    for (const t of tokens) {
      const next = cur + t;
      if (ctx.measureText(next.trim()).width > iw && cur.trim()) {
        lines.push(cur.trim());
        cur = t.trimStart();
      } else cur = next;
    }
    lines.push(cur.trim());
  }
  const lineH = size * 1.2;
  const top = cy - (lines.length * lineH) / 2 + lineH / 2;
  lines.forEach((l, i) => stroked(ctx, l, cx, top + i * lineH, sfx, size));
  void ih;
}

function stroked(ctx: CanvasRenderingContext2D, t: string, x: number, y: number, outline: boolean, size: number) {
  if (outline) {
    ctx.save();
    ctx.lineWidth = Math.max(3, size / 5);
    ctx.strokeStyle = "#fff";
    ctx.lineJoin = "round";
    ctx.strokeText(t, x, y);
    ctx.restore();
  }
  ctx.fillText(t, x, y);
}

/** The page's trim box (canvas minus bleed). */
export function trimRect(b: Board): Rect {
  const bl = b.page?.bleed || 0;
  return { x: bl, y: bl, w: b.width - 2 * bl, h: b.height - 2 * bl };
}

export function innerRect(b: Board): Rect {
  return b.page?.inner || trimRect(b);
}

/** Facing page index in a spread (page 1 alone, then pairs), or -1. */
export function facingIndex(b: Board, i: number): number {
  if (!b.page?.spreads || i < 0) return -1;
  const j = i === 0 ? -1 : i % 2 === 1 ? i + 1 : i - 1;
  return j >= 0 && j < b.frames.length ? j : -1;
}

/** Whether page i sits on the left of its spread. */
export function isLeftPage(b: Board, i: number): boolean {
  // Left to right: even page numbers (1-based) are on the left. Manga mirrors it.
  const evenPage = (i + 1) % 2 === 0;
  return b.page?.rtl ? !evenPage : evenPage;
}

export function pageSetupDefaults(): PageSetup {
  return { dpi: 150, bleed: 0, inner: { x: 0, y: 0, w: 0, h: 0 }, rtl: false, spreads: false, gutterX: 12, gutterY: 24, border: 4, mask: true, clipToPanel: true, slice: 1280 };
}
