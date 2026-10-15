// The drawing tools. Each one gets pointer events in canvas coordinates and
// works through the editor (pixels, history, redraw).

import { BRUSH_DEFAULTS, BrushKind, BrushSettings, Point, Stroke, StrokeBuffer, cursorRadius } from "./brush.ts";
import { Editor, ToolHandler, selectionFrom, translated } from "./editor.ts";
import { Layer, TILE, Transform } from "./model.ts";
import { context } from "./store.ts";
import { Scratch, corners, sourceSize } from "./render.ts";
import { polyPath } from "./pages.ts";
import { cloneBoard, BoardStep, TileStep } from "./history.ts";

export type ShapeKind = "line" | "rect" | "ellipse" | "arrow";

export interface ToolSettings {
  brushes: Record<BrushKind, BrushSettings>;
  fill: { tolerance: number; expand: number; sample: "all" | "layer"; behind: boolean };
  lasso: { opacity: number };
  shape: { kind: ShapeKind; width: number; fill: boolean };
  select: { mode: "rect" | "lasso" | "ellipse" };
  balloon: { kind: import("./model.ts").BalloonKind; vertical: boolean; size: number };
}

export function defaultToolSettings(): ToolSettings {
  return {
    brushes: JSON.parse(JSON.stringify(BRUSH_DEFAULTS)),
    fill: { tolerance: 40, expand: 2, sample: "all", behind: false },
    lasso: { opacity: 1 },
    shape: { kind: "rect", width: 4, fill: false },
    select: { mode: "rect" },
    balloon: { kind: "speech", vertical: false, size: 28 },
  };
}

export const TOOL_INFO: { id: string; name: string; icon: string; key: string }[] = [
  { id: "pencil", name: "Pencil", icon: "pencil", key: "P" },
  { id: "pen", name: "Pen", icon: "pen-tool", key: "N" },
  { id: "brush", name: "Brush", icon: "brush", key: "B" },
  { id: "marker", name: "Marker", icon: "highlighter", key: "K" },
  { id: "eraser", name: "Eraser", icon: "eraser", key: "E" },
  { id: "fill", name: "Fill", icon: "paint-bucket", key: "G" },
  { id: "lasso", name: "Lasso fill", icon: "lasso", key: "L" },
  { id: "shape", name: "Shapes and camera arrows", icon: "shapes", key: "U" },
  { id: "select", name: "Select", icon: "square-dashed", key: "M" },
  { id: "move", name: "Move and transform", icon: "move", key: "V" },
  { id: "panel", name: "Panels: cut, draw, move", icon: "panels-top-left", key: "J" },
  { id: "balloon", name: "Balloons and captions", icon: "message-square-text", key: "W" },
  { id: "eyedropper", name: "Pick colour", icon: "pipette", key: "I" },
  { id: "hand", name: "Hand", icon: "hand", key: "H" },
];

/** The tools a kind of storyboard offers (panels and balloons only on pages). */
export function toolsFor(kind: string): typeof TOOL_INFO {
  const pages = ["thumbnails", "name", "comic", "webtoon"].includes(kind);
  return TOOL_INFO.filter((t) => pages || (t.id !== "panel" && t.id !== "balloon"));
}

/** The active layer, if drawing on it is possible; otherwise tells why. */
function drawable(ed: Editor, raster = true): Layer | null {
  const l = ed.active;
  if (!l) return null;
  if (!l.visible) {
    ed.trigger("notice", "This layer is hidden.");
    return null;
  }
  if (l.locked) {
    ed.trigger("notice", "This layer is locked.");
    return null;
  }
  if (raster && l.kind !== "raster") {
    ed.trigger("notice", l.kind === "trace" ? "This is the trace layer: draw on a layer above it (T toggles the trace)." : "This is a picture layer: add a drawing layer to draw.");
    return null;
  }
  return l;
}

/** Clips drawing in layer coordinates to the canvas (bounded) and the selection. */
function clipper(ed: Editor, l: Layer, at?: { x: number; y: number }): (ctx: CanvasRenderingContext2D) => void {
  const b = ed.board;
  const joined = !!ed.joinedFacing();
  let sel = ed.selection ? translated(ed.selection.path, -l.x, -l.y) : null;
  // Pages: a stroke stays in the panel it starts in (like drawing inside a frame), except on a spread drawn as one.
  if (!sel && at && b.page?.clipToPanel && !joined) {
    const panel = ed.panelAt(at.x, at.y);
    if (panel) sel = translated(polyPath(panel.points), -l.x, -l.y);
  }
  const r = ed.drawArea();
  return (ctx) => {
    if (!b.infinite) {
      ctx.beginPath();
      ctx.rect(r.x - l.x, r.y - l.y, r.w, r.h);
      ctx.clip();
    }
    if (sel) ctx.clip(sel);
  };
}

function outsideCanvas(ed: Editor, l: Layer, x: number, y: number, w: number, h: number) {
  const r = ed.drawArea();
  return !ed.board.infinite && (x + w < r.x - l.x || y + h < r.y - l.y || x > r.x + r.w - l.x || y > r.y + r.h - l.y);
}

/**
 * On a spread drawn as one, paints the same marks into the facing page's
 * layer, clipped to that page. (x, y, w, h) and `draw` are in the coordinates
 * of `l`, the layer being drawn on.
 */
function paintFacing(ed: Editor, l: Layer, step: TileStep, x: number, y: number, w: number, h: number, draw: (ctx: CanvasRenderingContext2D) => void) {
  const sp = step.spread;
  if (!sp) return;
  const fl = sp.target.layer;
  const page = { x: sp.target.dx - l.x, y: -l.y, w: ed.board.width, h: ed.board.height };
  if (x + w < page.x || x > page.x + page.w || y + h < page.y || y > page.y + page.h) return;
  const ox = l.x - sp.target.dx - fl.x, oy = l.y - fl.y;
  ed.eachTile(x + ox, y + oy, w, h, (tx, ty) => sp.step.touch(tx, ty));
  ed.doc.tiles.paint(fl.id, x + ox, y + oy, w, h, (ctx) => {
    ctx.translate(-x, -y);
    ctx.beginPath();
    ctx.rect(page.x, page.y, page.w, page.h);
    ctx.clip();
    draw(ctx);
  });
}

// ── brushes ───────────────────────────────────────────────────────────────

export class BrushTool implements ToolHandler {
  cursor = "none";
  private stroke: Stroke | null = null;
  private layer: Layer | null = null;
  private step: TileStep | null = null;
  private buffer: StrokeBuffer | null = null;
  private last = new Map<string, Point>();

  constructor(public id: BrushKind, private ed: Editor, private settings: () => ToolSettings) {}

  get s(): BrushSettings {
    return this.settings().brushes[this.id];
  }

  down(e: PointerEvent, p: { x: number; y: number }) {
    const ed = this.ed;
    const l = drawable(ed);
    if (!l) return;
    this.layer = l;
    this.step = ed.pixels(this.id === "eraser" ? "Erase" : "Draw", l.id);
    const clip = clipper(ed, l, p);
    const color = this.id === "eraser" ? "#000000" : ed.color;
    const direct = Stroke.direct(this.id);
    if (direct) {
      this.stroke = new Stroke(this.id, this.s, color, (x, y, w, h, draw) => {
        if (outsideCanvas(ed, l, x, y, w, h)) return;
        ed.eachTile(x, y, w, h, (tx, ty) => this.step!.touch(tx, ty));
        ed.doc.tiles.paint(l.id, x, y, w, h, (ctx) => {
          ctx.translate(-x, -y);
          clip(ctx);
          if (l.alphaLock) ctx.globalCompositeOperation = "source-atop";
          draw(ctx);
        });
        paintFacing(ed, l, this.step!, x, y, w, h, (ctx) => {
          if (this.step!.spread!.target.layer.alphaLock) ctx.globalCompositeOperation = "source-atop";
          draw(ctx);
        });
      });
    } else {
      const buf = (this.buffer = new StrokeBuffer());
      this.stroke = new Stroke(this.id, this.s, color, (x, y, w, h, draw) => {
        if (outsideCanvas(ed, l, x, y, w, h)) return;
        buf.paint(x, y, w, h, (ctx) => {
          clip(ctx);
          draw(ctx);
        });
      });
      const c = this.stroke.composite;
      ed.stroke = { layer: l.id, tiles: buf.tiles, op: c.op, alpha: c.alpha, atop: !!l.alphaLock && this.id !== "eraser" };
    }
    const pt = { x: p.x - l.x, y: p.y - l.y, p: Stroke.pressure(e) };
    const prev = this.last.get(l.id);
    if (e.shiftKey && prev) this.stroke.line(prev, pt);
    else this.stroke.add(pt);
    this.last.set(l.id, pt);
    ed.requestRender();
  }

  move(e: PointerEvent, p: { x: number; y: number }) {
    this.ed.pointer = p;
    if (!this.stroke || !this.layer) {
      this.ed.drawOverlay();
      return;
    }
    const l = this.layer;
    const events = (e as any).getCoalescedEvents?.() as PointerEvent[] | undefined;
    for (const ev of events?.length ? events : [e]) {
      const q = this.ed.eventPoint(ev);
      const pt = { x: q.x - l.x, y: q.y - l.y, p: Stroke.pressure(ev) };
      this.stroke.add(pt);
      this.last.set(l.id, pt);
    }
    this.ed.requestRender();
  }

  hover(_e: PointerEvent, p: { x: number; y: number }) {
    this.ed.pointer = p;
    this.ed.drawOverlay();
  }

  up(e: PointerEvent, p: { x: number; y: number }) {
    const s = this.stroke, l = this.layer, ed = this.ed;
    if (!s || !l) return;
    const end = { x: p.x - l.x, y: p.y - l.y, p: Stroke.pressure(e) };
    s.end(end);
    if (this.buffer) {
      const c = s.composite;
      const atop = !!l.alphaLock && this.id !== "eraser";
      const fl = this.step!.spread?.target.layer;
      for (const [k, tile] of this.buffer.tiles) {
        const [tx, ty] = k.split("_").map(Number);
        // The same stroke on the facing page of a spread drawn as one.
        if (fl) {
          paintFacing(ed, l, this.step!, tx * TILE, ty * TILE, TILE, TILE, (fctx) => {
            fctx.globalAlpha = c.alpha;
            fctx.globalCompositeOperation = fl.alphaLock && this.id !== "eraser" ? "source-atop" : c.op;
            fctx.drawImage(tile, tx * TILE, ty * TILE);
          });
        }
        if (c.op === "destination-out" && !ed.doc.tiles.has(l.id, tx, ty)) continue;
        this.step!.touch(tx, ty);
        const ctx = context(ed.doc.tiles.ensure(l.id, tx, ty));
        ctx.save();
        ctx.globalAlpha = c.alpha;
        ctx.globalCompositeOperation = atop ? "source-atop" : c.op;
        ctx.drawImage(tile, 0, 0);
        ctx.restore();
        ed.doc.tiles.markDirty(l.id, tx, ty);
      }
    }
    if (this.step!.spread) this.step!.spread.handled = true;
    ed.stroke = null;
    ed.commitPixels(this.step!);
    this.stroke = null;
    this.buffer = null;
    this.step = null;
    this.layer = null;
    ed.invalidate();
  }

  cancel() {
    if (this.step) {
      this.step.finish();
      this.step.undo();
    }
    this.ed.stroke = null;
    this.stroke = null;
    this.buffer = null;
    this.ed.invalidate();
  }

  overlay(ctx: CanvasRenderingContext2D) {
    const p = this.ed.pointer;
    if (!p) return;
    const r = cursorRadius(this.id, this.s);
    const px = 1 / this.ed.view.scale;
    ctx.save();
    ctx.lineWidth = px;
    ctx.strokeStyle = "rgba(255,255,255,.9)";
    ctx.beginPath();
    ctx.arc(p.x, p.y, Math.max(r, 2 * px), 0, Math.PI * 2);
    ctx.stroke();
    ctx.strokeStyle = "rgba(0,0,0,.8)";
    ctx.beginPath();
    ctx.arc(p.x, p.y, Math.max(r, 2 * px) + px, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }
}

// ── fill ──────────────────────────────────────────────────────────────────

export class FillTool implements ToolHandler {
  id = "fill";
  cursor = "crosshair";
  constructor(private ed: Editor, private settings: () => ToolSettings) {}

  async down(_e: PointerEvent, p: { x: number; y: number }) {
    const ed = this.ed;
    const l = drawable(ed);
    if (!l) return;
    if (!ed.inBounds(p.x, p.y)) return;
    const s = this.settings().fill;
    const b = ed.board;
    const vis = ed.visibleRegion();
    const region = b.infinite ? vis : { x: 0, y: 0, w: b.width, h: b.height };
    const rx = Math.floor(region.x), ry = Math.floor(region.y);
    const w = Math.ceil(region.w), h = Math.ceil(region.h);
    if (w * h > 40e6) return ed.trigger("notice", "Zoom in to fill: the area on screen is too large.");
    const src = document.createElement("canvas");
    src.width = w;
    src.height = h;
    const sctx = src.getContext("2d", { willReadFrequently: true })!;
    sctx.translate(-rx, -ry);
    if (s.sample === "layer") ed.comp.tiles(sctx, l.id, l.x, l.y, { x: rx, y: ry, w, h });
    else {
      const hidden = new Set(ed.layers.filter((x) => x.kind === "trace").map((x) => x.id));
      ed.comp.drawLayers(sctx, ed.layers.filter((x) => x.visible && !hidden.has(x.id)), { region: { x: rx, y: ry, w, h }, background: false }, new Scratch(w, h), sctx.getTransform());
    }
    const sx = Math.floor(p.x - rx), sy = Math.floor(p.y - ry);
    const data = sctx.getImageData(0, 0, w, h).data;
    const mask = flood(data, w, h, sx, sy, s.tolerance);
    if (!mask) return;
    for (let i = 0; i < s.expand; i++) dilate(mask, w, h);
    const fillC = document.createElement("canvas");
    fillC.width = w;
    fillC.height = h;
    const fctx = fillC.getContext("2d")!;
    const img = fctx.createImageData(w, h);
    const n = parseInt(ed.color.slice(1), 16);
    const [r, g, bl] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    for (let i = 0; i < mask.length; i++) {
      if (!mask[i]) continue;
      const j = i * 4;
      img.data[j] = r;
      img.data[j + 1] = g;
      img.data[j + 2] = bl;
      img.data[j + 3] = 255;
    }
    fctx.putImageData(img, 0, 0);
    await ed.doc.tiles.load([l.id]);
    const step = ed.pixels("Fill", l.id);
    const lx = rx - l.x, ly = ry - l.y;
    const clip = clipper(ed, l, p);
    ed.eachTile(lx, ly, w, h, (tx, ty) => step.touch(tx, ty));
    ed.doc.tiles.paint(l.id, lx, ly, w, h, (ctx) => {
      ctx.translate(-lx, -ly);
      clip(ctx);
      ctx.globalCompositeOperation = l.alphaLock ? "source-atop" : s.behind ? "destination-over" : "source-over";
      ctx.drawImage(fillC, lx, ly);
    });
    ed.commitPixels(step);
  }
}

/** Scanline flood fill; returns a mask (1 = filled) or null when the seed is outside. */
export function flood(d: Uint8ClampedArray, w: number, h: number, sx: number, sy: number, tol: number): Uint8Array | null {
  if (sx < 0 || sy < 0 || sx >= w || sy >= h) return null;
  const mask = new Uint8Array(w * h);
  const i0 = (sy * w + sx) * 4;
  const [r0, g0, b0, a0] = [d[i0], d[i0 + 1], d[i0 + 2], d[i0 + 3]];
  const t = tol * tol * 4;
  const same = (i: number) => {
    const j = i * 4;
    // Transparent pixels match each other whatever their colour bytes.
    if (a0 === 0 && d[j + 3] === 0) return true;
    const dr = d[j] - r0, dg = d[j + 1] - g0, db = d[j + 2] - b0, da = d[j + 3] - a0;
    return dr * dr + dg * dg + db * db + da * da <= t;
  };
  const stack = [sx, sy];
  while (stack.length) {
    const y = stack.pop()!, x0 = stack.pop()!;
    let x = x0;
    const row = y * w;
    while (x > 0 && !mask[row + x - 1] && same(row + x - 1)) x--;
    let up = false, down = false;
    for (; x < w && !mask[row + x] && same(row + x); x++) {
      mask[row + x] = 1;
      if (y > 0) {
        const u = row - w + x;
        if (!mask[u] && same(u)) {
          if (!up) {
            stack.push(x, y - 1);
            up = true;
          }
        } else up = false;
      }
      if (y < h - 1) {
        const dn = row + w + x;
        if (!mask[dn] && same(dn)) {
          if (!down) {
            stack.push(x, y + 1);
            down = true;
          }
        } else down = false;
      }
    }
  }
  return mask;
}

export function dilate(m: Uint8Array, w: number, h: number) {
  const src = m.slice();
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      if (src[i]) continue;
      if ((x > 0 && src[i - 1]) || (x < w - 1 && src[i + 1]) || (y > 0 && src[i - w]) || (y < h - 1 && src[i + w])) m[i] = 1;
    }
  }
}

// ── lasso fill ────────────────────────────────────────────────────────────

export class LassoFillTool implements ToolHandler {
  id = "lasso";
  cursor = "crosshair";
  private pts: { x: number; y: number }[] = [];
  private erase = false;
  constructor(private ed: Editor, private settings: () => ToolSettings) {}

  down(e: PointerEvent, p: { x: number; y: number }) {
    if (!drawable(this.ed)) return;
    this.pts = [p];
    this.erase = e.altKey;
  }
  move(_e: PointerEvent, p: { x: number; y: number }) {
    if (!this.pts.length) return;
    const last = this.pts[this.pts.length - 1];
    if (Math.hypot(p.x - last.x, p.y - last.y) * this.ed.view.scale < 2) return;
    this.pts.push(p);
    this.ed.drawOverlay();
  }
  async up() {
    const ed = this.ed;
    const pts = this.pts;
    this.pts = [];
    ed.drawOverlay();
    const l = ed.active;
    const sel = selectionFrom(pts);
    if (!l || !sel) return;
    await ed.doc.tiles.load([l.id]);
    const step = ed.pixels(this.erase ? "Lasso erase" : "Lasso fill", l.id);
    const box = sel.box;
    const x = Math.floor(box.x - l.x) - 1, y = Math.floor(box.y - l.y) - 1;
    const w = Math.ceil(box.w) + 3, h = Math.ceil(box.h) + 3;
    if (outsideCanvas(ed, l, x, y, w, h)) return;
    const path = translated(sel.path, -l.x, -l.y);
    const clip = clipper(ed, l, pts[0]);
    ed.eachTile(x, y, w, h, (tx, ty) => step.touch(tx, ty));
    ed.doc.tiles.paint(l.id, x, y, w, h, (ctx) => {
      ctx.translate(-x, -y);
      clip(ctx);
      ctx.globalAlpha = this.settings().lasso.opacity;
      ctx.globalCompositeOperation = this.erase ? "destination-out" : l.alphaLock ? "source-atop" : "source-over";
      ctx.fillStyle = ed.color;
      ctx.fill(path);
    });
    ed.commitPixels(step);
  }
  overlay(ctx: CanvasRenderingContext2D) {
    if (this.pts.length < 2) return;
    const px = 1 / this.ed.view.scale;
    ctx.save();
    ctx.beginPath();
    this.pts.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
    ctx.closePath();
    ctx.globalAlpha = 0.35;
    ctx.fillStyle = this.erase ? "#ffffff" : this.ed.color;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.lineWidth = px;
    ctx.setLineDash([4 * px, 3 * px]);
    ctx.strokeStyle = "#000";
    ctx.stroke();
    ctx.restore();
  }
}

// ── shapes ────────────────────────────────────────────────────────────────

export class ShapeTool implements ToolHandler {
  id = "shape";
  cursor = "crosshair";
  private a: { x: number; y: number } | null = null;
  private b: { x: number; y: number } | null = null;
  private mods = { shift: false, alt: false };
  constructor(private ed: Editor, private settings: () => ToolSettings) {}

  down(e: PointerEvent, p: { x: number; y: number }) {
    if (!drawable(this.ed)) return;
    this.a = p;
    this.b = p;
    this.mods = { shift: e.shiftKey, alt: e.altKey };
  }
  move(e: PointerEvent, p: { x: number; y: number }) {
    if (!this.a) return;
    this.b = p;
    this.mods = { shift: e.shiftKey, alt: e.altKey };
    this.ed.drawOverlay();
  }

  /** Start and end after Shift (squares, 45° lines) and Alt (from the centre). */
  private geometry() {
    let a = { ...this.a! }, b = { ...this.b! };
    const kind = this.settings().shape.kind;
    if (this.mods.shift) {
      if (kind === "line" || kind === "arrow") {
        const ang = Math.round(Math.atan2(b.y - a.y, b.x - a.x) / (Math.PI / 4)) * (Math.PI / 4);
        const d = Math.hypot(b.x - a.x, b.y - a.y);
        b = { x: a.x + Math.cos(ang) * d, y: a.y + Math.sin(ang) * d };
      } else {
        const d = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y));
        b = { x: a.x + Math.sign(b.x - a.x || 1) * d, y: a.y + Math.sign(b.y - a.y || 1) * d };
      }
    }
    if (this.mods.alt && kind !== "line" && kind !== "arrow") a = { x: 2 * a.x - b.x, y: 2 * a.y - b.y };
    return { a, b, kind };
  }

  private draw(ctx: CanvasRenderingContext2D, ox: number, oy: number) {
    const { a, b, kind } = this.geometry();
    const s = this.settings().shape;
    ctx.lineWidth = s.width;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = ctx.fillStyle = this.ed.color;
    const A = { x: a.x - ox, y: a.y - oy }, B = { x: b.x - ox, y: b.y - oy };
    ctx.beginPath();
    if (kind === "line" || kind === "arrow") {
      ctx.moveTo(A.x, A.y);
      ctx.lineTo(B.x, B.y);
      ctx.stroke();
      if (kind === "arrow") {
        // A camera-move arrow: a filled head sized to the line width.
        const ang = Math.atan2(B.y - A.y, B.x - A.x);
        const k = Math.max(10, s.width * 4);
        ctx.beginPath();
        ctx.moveTo(B.x + Math.cos(ang) * s.width, B.y + Math.sin(ang) * s.width);
        ctx.lineTo(B.x - Math.cos(ang - 0.45) * k, B.y - Math.sin(ang - 0.45) * k);
        ctx.lineTo(B.x - Math.cos(ang + 0.45) * k, B.y - Math.sin(ang + 0.45) * k);
        ctx.closePath();
        ctx.fill();
      }
      return;
    }
    const x = Math.min(A.x, B.x), y = Math.min(A.y, B.y), w = Math.abs(B.x - A.x), h = Math.abs(B.y - A.y);
    if (kind === "rect") ctx.rect(x, y, w, h);
    else ctx.ellipse(x + w / 2, y + h / 2, w / 2, h / 2, 0, 0, Math.PI * 2);
    if (s.fill) ctx.fill();
    else ctx.stroke();
  }

  async up() {
    const ed = this.ed;
    if (!this.a || !this.b) return;
    const l = ed.active;
    const moved = Math.hypot(this.b.x - this.a.x, this.b.y - this.a.y) * ed.view.scale > 2;
    if (!l || !moved) {
      this.a = this.b = null;
      ed.drawOverlay();
      return;
    }
    await ed.doc.tiles.load([l.id]);
    const { a, b } = this.geometry();
    const pad = this.settings().shape.width * 4 + 12;
    const x = Math.floor(Math.min(a.x, b.x) - l.x - pad), y = Math.floor(Math.min(a.y, b.y) - l.y - pad);
    const w = Math.ceil(Math.abs(b.x - a.x) + 2 * pad), h = Math.ceil(Math.abs(b.y - a.y) + 2 * pad);
    const step = ed.pixels("Shape", l.id);
    const clip = clipper(ed, l, a);
    ed.eachTile(x, y, w, h, (tx, ty) => step.touch(tx, ty));
    ed.doc.tiles.paint(l.id, x, y, w, h, (ctx) => {
      ctx.translate(-x, -y);
      clip(ctx);
      if (l.alphaLock) ctx.globalCompositeOperation = "source-atop";
      this.draw(ctx, l.x, l.y);
    });
    this.a = this.b = null;
    ed.commitPixels(step);
    ed.drawOverlay();
  }

  overlay(ctx: CanvasRenderingContext2D) {
    if (!this.a || !this.b) return;
    ctx.save();
    ctx.globalAlpha = 0.8;
    this.draw(ctx, 0, 0);
    ctx.restore();
  }
}

// ── selection ─────────────────────────────────────────────────────────────

export class SelectTool implements ToolHandler {
  id = "select";
  cursor = "crosshair";
  private pts: { x: number; y: number }[] = [];
  private start: { x: number; y: number } | null = null;
  private shift = false;
  constructor(private ed: Editor, private settings: () => ToolSettings) {}

  down(e: PointerEvent, p: { x: number; y: number }) {
    this.start = p;
    this.pts = [p];
    this.shift = e.shiftKey;
  }
  move(e: PointerEvent, p: { x: number; y: number }) {
    if (!this.start) return;
    this.shift = e.shiftKey;
    if (this.settings().select.mode === "lasso") this.pts.push(p);
    else this.pts = [this.start, p];
    this.ed.drawOverlay();
  }
  private polygon(): { x: number; y: number }[] {
    const mode = this.settings().select.mode;
    if (mode === "lasso") return this.pts;
    if (this.pts.length < 2) return [];
    const [a, b0] = this.pts;
    let b = b0;
    if (this.shift) {
      const d = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y));
      b = { x: a.x + Math.sign(b.x - a.x || 1) * d, y: a.y + Math.sign(b.y - a.y || 1) * d };
    }
    if (mode === "rect") return [a, { x: b.x, y: a.y }, b, { x: a.x, y: b.y }];
    const cx = (a.x + b.x) / 2, cy = (a.y + b.y) / 2, rx = Math.abs(b.x - a.x) / 2, ry = Math.abs(b.y - a.y) / 2;
    return Array.from({ length: 64 }, (_, i) => ({ x: cx + Math.cos((i / 64) * Math.PI * 2) * rx, y: cy + Math.sin((i / 64) * Math.PI * 2) * ry }));
  }
  up() {
    const poly = this.polygon();
    this.start = null;
    this.pts = [];
    this.ed.commitFloating();
    this.ed.setSelection(selectionFrom(poly));
  }
  overlay(ctx: CanvasRenderingContext2D) {
    const poly = this.start ? this.polygon() : [];
    if (poly.length < 2) return;
    const px = 1 / this.ed.view.scale;
    ctx.save();
    ctx.lineWidth = px;
    ctx.setLineDash([4 * px, 4 * px]);
    ctx.strokeStyle = "#2f80ed";
    ctx.beginPath();
    poly.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
    ctx.closePath();
    ctx.stroke();
    ctx.restore();
  }
}

// ── move and transform ────────────────────────────────────────────────────

type Handle = "move" | "rotate" | "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
const HANDLE_SIGNS: Record<string, [number, number]> = { nw: [-1, -1], n: [0, -1], ne: [1, -1], e: [1, 0], se: [1, 1], s: [0, 1], sw: [-1, 1], w: [-1, 0] };

export class MoveTool implements ToolHandler {
  id = "move";
  cursor = "move";
  private drag: { handle: Handle; t0: Transform; p0: { x: number; y: number }; w: number; h: number; before?: any; layer0?: { x: number; y: number } } | null = null;
  constructor(private ed: Editor) {}

  /** What is being transformed: lifted pixels, or a picture/trace layer. */
  private target(): { t: Transform; w: number; h: number; set: (t: Transform) => void } | null {
    const ed = this.ed;
    if (ed.floating) {
      const f = ed.floating;
      return { t: f.t, w: f.canvas.width, h: f.canvas.height, set: (t) => (f.t = t) };
    }
    const l = ed.active;
    if (l && (l.kind === "image" || l.kind === "trace") && l.transform && l.src) {
      const size = ed.pictureSize(l.src);
      if (!size) return null;
      return { t: l.transform, w: size[0], h: size[1], set: (t) => (l.transform = t) };
    }
    return null;
  }

  private hit(t: Transform, w: number, h: number, p: { x: number; y: number }): Handle | null {
    const ed = this.ed;
    const cs = corners(t, w, h).map((c) => ed.toStage(c.x, c.y));
    const s = ed.toStage(p.x, p.y);
    const near = (q: { x: number; y: number }) => Math.hypot(q.x - s.x, q.y - s.y) < 9;
    const names: Handle[] = ["nw", "ne", "se", "sw"];
    for (let i = 0; i < 4; i++) if (near(cs[i])) return names[i];
    const mids: Handle[] = ["n", "e", "s", "w"];
    for (let i = 0; i < 4; i++) if (near({ x: (cs[i].x + cs[(i + 1) % 4].x) / 2, y: (cs[i].y + cs[(i + 1) % 4].y) / 2 })) return mids[i];
    const top = { x: (cs[0].x + cs[1].x) / 2, y: (cs[0].y + cs[1].y) / 2 };
    const dir = Math.atan2(cs[1].y - cs[0].y, cs[1].x - cs[0].x) - Math.PI / 2;
    const rh = { x: top.x + Math.cos(dir) * 28 * Math.sign(t.sy || 1), y: top.y + Math.sin(dir) * 28 * Math.sign(t.sy || 1) };
    if (near(rh)) return "rotate";
    // Inside the quadrilateral?
    let inside = false;
    for (let i = 0, j = 3; i < 4; j = i++) {
      if ((cs[i].y > s.y) !== (cs[j].y > s.y) && s.x < ((cs[j].x - cs[i].x) * (s.y - cs[i].y)) / (cs[j].y - cs[i].y) + cs[i].x) inside = !inside;
    }
    return inside ? "move" : null;
  }

  async down(e: PointerEvent, p: { x: number; y: number }) {
    const ed = this.ed;
    let target = this.target();
    if (!target) {
      const l = drawable(ed, false);
      if (!l) return;
      if (l.kind === "raster" && ed.selection) {
        await ed.liftSelection();
        target = this.target();
      } else if (l.kind === "raster") {
        // Dragging a whole raster layer moves its offset: instant and lossless.
        this.drag = { handle: "move", t0: { x: 0, y: 0, sx: 1, sy: 1, rotation: 0 }, p0: p, w: 0, h: 0, before: cloneBoard(ed.board), layer0: { x: l.x, y: l.y } };
        return;
      }
    }
    if (!target) return;
    const handle = this.hit(target.t, target.w, target.h, p);
    if (!handle) {
      // Outside: put lifted pixels down.
      if (ed.floating) ed.commitFloating();
      return;
    }
    this.drag = { handle, t0: { ...target.t }, p0: p, w: target.w, h: target.h, before: ed.floating ? undefined : cloneBoard(ed.board) };
    void e;
  }

  move(e: PointerEvent, p: { x: number; y: number }) {
    const d = this.drag, ed = this.ed;
    if (!d) return this.hover(e, p);
    if (d.layer0) {
      const l = ed.active!;
      l.x = Math.round(d.layer0.x + p.x - d.p0.x);
      l.y = Math.round(d.layer0.y + p.y - d.p0.y);
      ed.invalidate();
      return;
    }
    const target = this.target();
    if (!target) return;
    const t0 = d.t0;
    let t: Transform = { ...t0 };
    if (d.handle === "move") {
      t.x = t0.x + p.x - d.p0.x;
      t.y = t0.y + p.y - d.p0.y;
    } else if (d.handle === "rotate") {
      const a0 = Math.atan2(d.p0.y - t0.y, d.p0.x - t0.x);
      const a1 = Math.atan2(p.y - t0.y, p.x - t0.x);
      let r = t0.rotation + a1 - a0;
      if (e.shiftKey) r = Math.round(r / (Math.PI / 12)) * (Math.PI / 12);
      t.rotation = r;
    } else {
      const [hx, hy] = HANDLE_SIGNS[d.handle];
      const cos = Math.cos(t0.rotation), sin = Math.sin(t0.rotation);
      const local = (q: { x: number; y: number }) => ({ x: cos * (q.x - t0.x) + sin * (q.y - t0.y), y: -sin * (q.x - t0.x) + cos * (q.y - t0.y) });
      const hw = (d.w * Math.abs(t0.sx)) / 2, hh = (d.h * Math.abs(t0.sy)) / 2;
      const anchor = { x: -hx * hw, y: -hy * hh };
      const u = local(p);
      let nw = hx ? Math.max(1, Math.abs(u.x - anchor.x)) : 2 * hw;
      let nh = hy ? Math.max(1, Math.abs(u.y - anchor.y)) : 2 * hh;
      // Corners keep the proportions unless Shift is held.
      if (hx && hy && !e.shiftKey) {
        const k = Math.max(nw / (2 * hw), nh / (2 * hh));
        nw = 2 * hw * k;
        nh = 2 * hh * k;
      }
      const cx = hx ? anchor.x + (hx * nw) / 2 : 0;
      const cy = hy ? anchor.y + (hy * nh) / 2 : 0;
      t = { ...t0, x: t0.x + cos * cx - sin * cy, y: t0.y + sin * cx + cos * cy, sx: Math.sign(t0.sx || 1) * (nw / d.w), sy: Math.sign(t0.sy || 1) * (nh / d.h) };
    }
    target.set(t);
    ed.invalidate();
  }

  hover(_e: PointerEvent, p: { x: number; y: number }) {
    const t = this.target();
    const h = t ? this.hit(t.t, t.w, t.h, p) : null;
    const cur: Record<string, string> = { move: "move", rotate: "grab", nw: "nwse-resize", se: "nwse-resize", ne: "nesw-resize", sw: "nesw-resize", n: "ns-resize", s: "ns-resize", e: "ew-resize", w: "ew-resize" };
    this.ed.stage.style.cursor = h ? cur[h] : t ? "default" : "move";
  }

  up() {
    const d = this.drag, ed = this.ed;
    this.drag = null;
    if (!d) return;
    if (d.before) {
      const after = cloneBoard(ed.board);
      if (JSON.stringify(after) !== JSON.stringify(d.before)) {
        ed.history.push(new BoardStep(d.layer0 ? "Move layer" : "Transform picture", ed.doc, d.before, after));
        ed.changed(true);
        ed.trigger("board");
      }
    }
  }

  key(e: KeyboardEvent): boolean {
    const ed = this.ed;
    if (e.key === "Enter" && ed.floating) {
      ed.commitFloating();
      return true;
    }
    if (e.key === "Escape" && ed.floating) {
      ed.cancelFloating();
      return true;
    }
    // Arrow keys nudge (Shift: 10 px). Without Alt they switch references, so nudging needs Alt.
    if (e.altKey && e.key.startsWith("Arrow")) {
      const k = e.shiftKey ? 10 : 1;
      const dx = e.key === "ArrowLeft" ? -k : e.key === "ArrowRight" ? k : 0;
      const dy = e.key === "ArrowUp" ? -k : e.key === "ArrowDown" ? k : 0;
      const t = this.target();
      if (t) t.set({ ...t.t, x: t.t.x + dx, y: t.t.y + dy });
      else if (ed.active?.kind === "raster" && !ed.active.locked) {
        const l = ed.active;
        ed.mutate("Move layer", () => {
          l.x += dx;
          l.y += dy;
        });
      }
      ed.invalidate();
      return true;
    }
    return false;
  }

  cancel() {
    this.drag = null;
  }

  deactivate() {
    this.ed.stage.style.cursor = "";
    this.ed.commitFloating();
  }

  /** Flips or resets what is being transformed. */
  flip(axis: "x" | "y") {
    const t = this.target();
    if (!t) return;
    const before = this.ed.floating ? null : cloneBoard(this.ed.board);
    t.set({ ...t.t, sx: axis === "x" ? -t.t.sx : t.t.sx, sy: axis === "y" ? -t.t.sy : t.t.sy });
    if (before) this.ed.history.push(new BoardStep("Flip", this.ed.doc, before, cloneBoard(this.ed.board)));
    this.ed.changed(true);
  }
}

// ── colour picker and hand ────────────────────────────────────────────────

export class EyedropperTool implements ToolHandler {
  id = "eyedropper";
  cursor = "crosshair";
  constructor(private ed: Editor) {}
  down(e: PointerEvent) {
    this.pick(e);
  }
  move(e: PointerEvent) {
    if (e.buttons) this.pick(e);
  }
  private pick(e: PointerEvent) {
    const r = this.ed.stage.getBoundingClientRect();
    const c = this.ed.sampleScreen(e.clientX - r.left, e.clientY - r.top);
    if (c) this.ed.setColor(c, false);
  }
  up() {
    this.ed.setColor(this.ed.color);
  }
}

export class HandTool implements ToolHandler {
  id = "hand";
  cursor = "grab";
  private last: { x: number; y: number } | null = null;
  constructor(private ed: Editor) {}
  down(e: PointerEvent) {
    this.last = { x: e.clientX, y: e.clientY };
    this.ed.stage.style.cursor = "grabbing";
  }
  move(e: PointerEvent) {
    if (!this.last) return;
    this.ed.panBy(e.clientX - this.last.x, e.clientY - this.last.y);
    this.last = { x: e.clientX, y: e.clientY };
  }
  up() {
    this.last = null;
    this.ed.stage.style.cursor = "";
  }
}

/** Fits a picture inside the frame (or the view, on an infinite canvas). */
export function fitTransform(ed: Editor, w: number, h: number, cover = false): Transform {
  const b = ed.board;
  const area = b.infinite ? ed.visibleRegion() : { x: 0, y: 0, w: b.width, h: b.height };
  const k = cover ? Math.max(area.w / w, area.h / h) : Math.min(area.w / w, area.h / h);
  return { x: area.x + area.w / 2, y: area.y + area.h / 2, sx: k, sy: k, rotation: 0 };
}

export function pictureSize(img: HTMLImageElement): [number, number] {
  return sourceSize(img);
}

export { TILE };
