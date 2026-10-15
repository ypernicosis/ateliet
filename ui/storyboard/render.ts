// Drawing a frame: paper, onion skins, layers (blend modes, opacity,
// clipping to the layer below), the stroke being drawn, lifted pixels being
// transformed, and the trace picture. The same code draws the screen, the
// thumbnails, playback and the exported pictures; only the target and its
// transform change.

import { Board, Frame, Layer, TILE, Transform } from "./model.ts";
import { StoryDoc, context } from "./store.ts";
import { traced } from "./trace.ts";

export interface ViewState {
  /** Screen position (CSS pixels) of the canvas origin. */
  x: number;
  y: number;
  scale: number;
  rotation: number;
  mirror: boolean;
}

/** The stroke in progress, drawn over its layer until it is committed. */
export interface StrokeOverlay {
  layer: string;
  tiles: Map<string, HTMLCanvasElement>;
  op: GlobalCompositeOperation;
  alpha: number;
  /** Clip to the layer's own pixels (alpha lock). */
  atop?: boolean;
}

/** Pixels lifted from a layer, being moved, scaled or rotated. */
export interface Floating {
  layer: string;
  canvas: HTMLCanvasElement;
  t: Transform;
}

export interface DrawOptions {
  /** Doc rectangle that needs drawing (tiles outside are skipped). */
  region: { x: number; y: number; w: number; h: number };
  background: boolean;
  /** Export: trace layers only when they say so, never temporary state. */
  exporting?: boolean;
  stroke?: StrokeOverlay | null;
  floating?: Floating | null;
  hidden?: Set<string>;
  onion?: boolean;
}

const pools = new Map<string, HTMLCanvasElement[]>();

/** Scratch canvases of a given size, reused between frames. */
export class Scratch {
  private used: HTMLCanvasElement[] = [];
  constructor(private w: number, private h: number) {}
  take(m: DOMMatrix): CanvasRenderingContext2D {
    const key = `${this.w}x${this.h}`;
    let pool = pools.get(key);
    if (!pool) pools.set(key, (pool = []));
    const c = pool.pop() || Object.assign(document.createElement("canvas"), { width: this.w, height: this.h });
    this.used.push(c);
    const ctx = c.getContext("2d")!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = "source-over";
    ctx.clearRect(0, 0, this.w, this.h);
    ctx.setTransform(m);
    ctx.imageSmoothingQuality = "high";
    return ctx;
  }
  release() {
    const key = `${this.w}x${this.h}`;
    const pool = pools.get(key)!;
    for (const c of this.used) if (pool.length < 8) pool.push(c);
    this.used = [];
  }
}

let checker: CanvasPattern | null = null;
function checkerPattern(ctx: CanvasRenderingContext2D): CanvasPattern | string {
  if (!checker) {
    const c = document.createElement("canvas");
    c.width = c.height = 16;
    const g = c.getContext("2d")!;
    g.fillStyle = "#ffffff";
    g.fillRect(0, 0, 16, 16);
    g.fillStyle = "#d8d8d8";
    g.fillRect(0, 0, 8, 8);
    g.fillRect(8, 8, 8, 8);
    checker = ctx.createPattern(c, "repeat");
  }
  return checker || "#fff";
}

export function applyTransform(ctx: CanvasRenderingContext2D, t: Transform) {
  ctx.translate(t.x, t.y);
  ctx.rotate(t.rotation);
  ctx.scale(t.sx, t.sy);
}

/** Corners of a transformed w×h picture, on the canvas. */
export function corners(t: Transform, w: number, h: number): { x: number; y: number }[] {
  const c = Math.cos(t.rotation);
  const s = Math.sin(t.rotation);
  return [
    [-w / 2, -h / 2],
    [w / 2, -h / 2],
    [w / 2, h / 2],
    [-w / 2, h / 2],
  ].map(([x, y]) => ({ x: t.x + c * x * t.sx - s * y * t.sy, y: t.y + s * x * t.sx + c * y * t.sy }));
}

/** Natural size of a picture source. */
export function sourceSize(img: CanvasImageSource): [number, number] {
  const i = img as any;
  return [i.naturalWidth || i.width, i.naturalHeight || i.height];
}

/** Draws a picture source centred on its transform, at its natural size. */
function drawPlaced(ctx: CanvasRenderingContext2D, img: CanvasImageSource, t: Transform, natural: [number, number]) {
  ctx.save();
  applyTransform(ctx, t);
  ctx.drawImage(img, -natural[0] / 2, -natural[1] / 2, natural[0], natural[1]);
  ctx.restore();
}

export class Compositor {
  /** Decoded pictures for image and trace layers, filled in asynchronously. */
  images = new Map<string, HTMLImageElement>();
  onImage: () => void = () => {};

  constructor(public doc: StoryDoc) {}

  get board(): Board {
    return this.doc.board;
  }

  /** Starts loading the pictures a frame uses. */
  want(frame: Frame) {
    for (const id of frame.layers) {
      const l = this.board.layers[id];
      if (!l?.src || this.images.has(l.src)) continue;
      const ref = this.doc.refs.ref(l.src);
      this.doc.refs.image(ref).then((img) => {
        if (img && !this.images.has(l.src!)) {
          this.images.set(l.src!, img);
          this.onImage();
        }
      });
    }
  }

  /**
   * Draws `frame` into `ctx`, whose transform maps the canvas to its pixels.
   * Layers are drawn bottom to top; trace layers set "above" come last.
   */
  drawFrame(ctx: CanvasRenderingContext2D, frame: Frame, o: DrawOptions) {
    const m = ctx.getTransform();
    const scratch = new Scratch(ctx.canvas.width, ctx.canvas.height);
    const b = this.board;
    ctx.save();
    if (!b.infinite) {
      ctx.beginPath();
      ctx.rect(0, 0, b.width, b.height);
      ctx.clip();
    }
    if (o.background) this.paper(ctx, o.region);
    if (o.onion && b.onion.enabled && !o.exporting) this.onion(ctx, frame, o, scratch, m);
    const layers = this.visibleLayers(frame, o);
    const below = layers.filter((l) => !(l.kind === "trace" && l.trace?.above));
    const above = layers.filter((l) => l.kind === "trace" && l.trace?.above);
    this.drawLayers(ctx, below, o, scratch, m);
    this.drawLayers(ctx, above, o, scratch, m);
    ctx.restore();
    scratch.release();
  }

  visibleLayers(frame: Frame, o: DrawOptions): Layer[] {
    return frame.layers
      .map((id) => this.board.layers[id])
      .filter((l) => l && l.visible && !o.hidden?.has(l.id) && !(o.exporting && l.kind === "trace" && !l.trace?.export));
  }

  paper(ctx: CanvasRenderingContext2D, r: DrawOptions["region"]) {
    const b = this.board;
    const rect = b.infinite ? r : { x: 0, y: 0, w: b.width, h: b.height };
    ctx.save();
    ctx.fillStyle = b.background === "transparent" ? checkerPattern(ctx) : b.background;
    ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
    ctx.restore();
  }

  /** Draws a run of layers, isolating the ones that need it. */
  drawLayers(ctx: CanvasRenderingContext2D, layers: Layer[], o: DrawOptions, scratch: Scratch, m: DOMMatrix) {
    for (let i = 0; i < layers.length; i++) {
      const l = layers[i];
      if (l.clip) {
        // Find the base: the nearest layer below that does not clip.
        let j = i - 1;
        while (j >= 0 && layers[j].clip) j--;
        if (j < 0) {
          this.drawLayer(ctx, l, o, scratch, m);
          continue;
        }
        const base = scratch.take(m);
        this.content(base, layers[j], o, scratch, m);
        const own = scratch.take(m);
        this.content(own, l, o, scratch, m);
        own.save();
        own.setTransform(1, 0, 0, 1, 0, 0);
        own.globalCompositeOperation = "destination-in";
        own.drawImage(base.canvas, 0, 0);
        own.restore();
        this.blit(ctx, own.canvas, l);
        continue;
      }
      this.drawLayer(ctx, l, o, scratch, m);
    }
  }

  private needsIsolation(l: Layer, o: DrawOptions): boolean {
    return (o.stroke?.layer === l.id) || (o.floating?.layer === l.id) || l.blend !== "source-over" || l.kind === "trace";
  }

  drawLayer(ctx: CanvasRenderingContext2D, l: Layer, o: DrawOptions, scratch: Scratch, m: DOMMatrix) {
    if (!this.needsIsolation(l, o) && l.kind === "raster") {
      ctx.save();
      ctx.globalAlpha = l.opacity;
      this.content(ctx, l, o, scratch, m);
      ctx.restore();
      return;
    }
    if (!this.needsIsolation(l, o) && l.kind === "image") {
      ctx.save();
      ctx.globalAlpha = l.opacity;
      this.content(ctx, l, o, scratch, m);
      ctx.restore();
      return;
    }
    const buf = scratch.take(m);
    this.content(buf, l, o, scratch, m);
    this.blit(ctx, buf.canvas, l);
  }

  /** An isolated layer onto the target, with its blend mode and opacity. */
  private blit(ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement, l: Layer) {
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = l.opacity;
    ctx.globalCompositeOperation = l.blend as GlobalCompositeOperation;
    ctx.drawImage(canvas, 0, 0);
    ctx.restore();
  }

  /** A layer's own pixels (and the stroke or lifted pixels on it). */
  content(ctx: CanvasRenderingContext2D, l: Layer, o: DrawOptions, _scratch: Scratch, _m: DOMMatrix) {
    if (l.kind === "raster") {
      this.tiles(ctx, l.id, l.x, l.y, o.region);
      const s = o.stroke;
      if (s && s.layer === l.id) {
        ctx.save();
        ctx.globalAlpha *= s.alpha;
        ctx.globalCompositeOperation = s.atop ? "source-atop" : s.op;
        this.tileMap(ctx, s.tiles, l.x, l.y, o.region);
        ctx.restore();
      }
    } else if (l.src && l.transform) {
      const img = this.images.get(l.src);
      if (img) {
        const natural = sourceSize(img);
        let src: CanvasImageSource = img;
        try {
          if (l.kind === "trace" && l.trace) src = traced(img, l.trace);
        } catch (e) {
          console.warn("trace", e);
        }
        ctx.save();
        ctx.imageSmoothingQuality = "high";
        drawPlaced(ctx, src, l.transform, natural);
        ctx.restore();
      }
    }
    const f = o.floating;
    if (f && f.layer === l.id) {
      ctx.save();
      drawPlaced(ctx, f.canvas, f.t, [f.canvas.width, f.canvas.height]);
      ctx.restore();
    }
  }

  /** Draws a raster layer's tiles that fall in the region. */
  tiles(ctx: CanvasRenderingContext2D, layer: string, ox: number, oy: number, r: DrawOptions["region"]) {
    const store = this.doc.tiles;
    const x0 = Math.floor((r.x - ox) / TILE);
    const y0 = Math.floor((r.y - oy) / TILE);
    const x1 = Math.floor((r.x + r.w - ox) / TILE);
    const y1 = Math.floor((r.y + r.h - oy) / TILE);
    const list: [HTMLCanvasElement, number, number][] = [];
    for (const [tx, ty] of store.tilesOf(layer)) {
      if (tx < x0 || tx > x1 || ty < y0 || ty > y1) continue;
      const c = store.get(layer, tx, ty);
      if (c) list.push([c, tx, ty]);
    }
    drawTiles(ctx, list, ox, oy);
  }

  private tileMap(ctx: CanvasRenderingContext2D, tiles: Map<string, HTMLCanvasElement>, ox: number, oy: number, r: DrawOptions["region"]) {
    const list: [HTMLCanvasElement, number, number][] = [];
    for (const [k, c] of tiles) {
      const [tx, ty] = k.split("_").map(Number);
      if ((tx + 1) * TILE + ox < r.x || tx * TILE + ox > r.x + r.w || (ty + 1) * TILE + oy < r.y || ty * TILE + oy > r.y + r.h) continue;
      list.push([c, tx, ty]);
    }
    drawTiles(ctx, list, ox, oy);
  }

  /** Neighbouring frames, faint (and tinted: earlier red, later green). */
  onion(ctx: CanvasRenderingContext2D, frame: Frame, o: DrawOptions, scratch: Scratch, m: DOMMatrix) {
    const b = this.board;
    const i = b.frames.indexOf(frame);
    const sides: [number, string][] = [];
    for (let k = b.onion.before; k >= 1; k--) sides.push([i - k, b.onion.colorBefore || "#d0453b"]);
    for (let k = 1; k <= b.onion.after; k++) sides.push([i + k, b.onion.colorAfter || "#2f9e5a"]);
    for (const [j, tint] of sides) {
      const f = b.frames[j];
      if (!f) continue;
      const buf = scratch.take(m);
      // Shared layers are the same in every frame: skinning them only blurs.
      const layers = f.layers.map((id) => b.layers[id]).filter((l) => l && l.visible && !l.shared && (l.kind === "raster" || (l.kind === "image" && b.onion.pictures)));
      this.drawLayers(buf, layers, { ...o, stroke: null, floating: null }, scratch, m);
      if (b.onion.tint) {
        buf.save();
        buf.setTransform(1, 0, 0, 1, 0, 0);
        buf.globalCompositeOperation = "source-atop";
        buf.globalAlpha = 0.65;
        buf.fillStyle = tint;
        buf.fillRect(0, 0, buf.canvas.width, buf.canvas.height);
        buf.restore();
      }
      const distance = Math.abs(j - i);
      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.globalAlpha = b.onion.fade === false ? b.onion.opacity : b.onion.opacity / distance;
      ctx.drawImage(buf.canvas, 0, 0);
      ctx.restore();
    }
  }

  /** Layers a frame needs decoded (its own and, with onion skin, its neighbours'). */
  layersFor(frame: Frame, onion: boolean): string[] {
    const b = this.board;
    const ids = new Set(frame.layers);
    if (onion && b.onion.enabled) {
      const i = b.frames.indexOf(frame);
      for (let k = -b.onion.before; k <= b.onion.after; k++) b.frames[i + k]?.layers.forEach((id) => ids.add(id));
    }
    return [...ids].filter((id) => b.layers[id]?.kind === "raster");
  }

  /**
   * The frame as a picture: `scale` × the canvas size (bounded canvas) or
   * the drawn area plus a margin (infinite canvas).
   */
  async picture(frame: Frame, scale = 1, opts: { exporting?: boolean; background?: boolean; area?: { x: number; y: number; w: number; h: number } } = {}): Promise<HTMLCanvasElement> {
    await this.doc.tiles.load(this.layersFor(frame, false));
    for (const id of frame.layers) {
      const l = this.board.layers[id];
      if (l?.src && !this.images.has(l.src)) {
        const img = await this.doc.refs.image(this.doc.refs.ref(l.src));
        if (img) this.images.set(l.src, img);
      }
    }
    const area = opts.area || this.area(frame, opts.exporting ?? true);
    const c = document.createElement("canvas");
    c.width = Math.max(1, Math.round(area.w * scale));
    c.height = Math.max(1, Math.round(area.h * scale));
    const ctx = c.getContext("2d")!;
    ctx.imageSmoothingQuality = "high";
    ctx.setTransform(scale, 0, 0, scale, -area.x * scale, -area.y * scale);
    this.drawFrame(ctx, frame, { region: area, background: opts.background ?? true, exporting: opts.exporting ?? true });
    return c;
  }

  /** The area a frame occupies: the canvas, or (infinite) what was drawn. */
  area(frame: Frame, exporting = true): { x: number; y: number; w: number; h: number } {
    const b = this.board;
    if (!b.infinite) return { x: 0, y: 0, w: b.width, h: b.height };
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const id of frame.layers) {
      const l = b.layers[id];
      if (!l || !l.visible || (exporting && l.kind === "trace" && !l.trace?.export)) continue;
      let r: { x: number; y: number; w: number; h: number } | null = null;
      if (l.kind === "raster") {
        const t = pixelBounds(this.doc, l);
        if (t) r = t;
      } else if (l.transform && l.src) {
        const img = this.images.get(l.src);
        if (img) {
          const [w, h] = sourceSize(img);
          const cs = corners(l.transform, w, h);
          const xs = cs.map((p) => p.x);
          const ys = cs.map((p) => p.y);
          r = { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) };
        }
      }
      if (!r) continue;
      x0 = Math.min(x0, r.x);
      y0 = Math.min(y0, r.y);
      x1 = Math.max(x1, r.x + r.w);
      y1 = Math.max(y1, r.y + r.h);
    }
    if (!isFinite(x0)) return { x: 0, y: 0, w: b.width, h: b.height };
    const pad = 32;
    return { x: Math.floor(x0 - pad), y: Math.floor(y0 - pad), w: Math.ceil(x1 - x0 + 2 * pad), h: Math.ceil(y1 - y0 + 2 * pad) };
  }
}

/** The exact rectangle of a raster layer's non-transparent pixels (canvas coordinates). */
export function pixelBounds(doc: StoryDoc, l: Layer): { x: number; y: number; w: number; h: number } | null {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const [tx, ty] of doc.tiles.tilesOf(l.id)) {
    const c = doc.tiles.get(l.id, tx, ty);
    if (!c) {
      // Not decoded: count the whole tile.
      x0 = Math.min(x0, tx * TILE);
      y0 = Math.min(y0, ty * TILE);
      x1 = Math.max(x1, (tx + 1) * TILE);
      y1 = Math.max(y1, (ty + 1) * TILE);
      continue;
    }
    const d = context(c).getImageData(0, 0, TILE, TILE).data;
    let found = false;
    let lx0 = TILE, ly0 = TILE, lx1 = -1, ly1 = -1;
    for (let y = 0; y < TILE; y++) {
      for (let x = 0; x < TILE; x++) {
        if (d[(y * TILE + x) * 4 + 3]) {
          found = true;
          if (x < lx0) lx0 = x;
          if (x > lx1) lx1 = x;
          if (y < ly0) ly0 = y;
          if (y > ly1) ly1 = y;
        }
      }
    }
    if (!found) continue;
    x0 = Math.min(x0, tx * TILE + lx0);
    y0 = Math.min(y0, ty * TILE + ly0);
    x1 = Math.max(x1, tx * TILE + lx1 + 1);
    y1 = Math.max(y1, ty * TILE + ly1 + 1);
  }
  if (!isFinite(x0)) return null;
  return { x: x0 + l.x, y: y0 + l.y, w: x1 - x0, h: y1 - y0 };
}

/**
 * Draws tiles at their place. When the transform is a plain scale and move,
 * edges are rounded to whole device pixels so neighbouring tiles meet
 * without hairline seams.
 */
function drawTiles(ctx: CanvasRenderingContext2D, list: [HTMLCanvasElement, number, number][], ox: number, oy: number) {
  if (!list.length) return;
  const m = ctx.getTransform();
  if (Math.abs(m.b) < 1e-9 && Math.abs(m.c) < 1e-9 && m.a > 0 && m.d > 0) {
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    for (const [c, tx, ty] of list) {
      const X0 = Math.round(m.a * (tx * TILE + ox) + m.e);
      const Y0 = Math.round(m.d * (ty * TILE + oy) + m.f);
      const X1 = Math.round(m.a * ((tx + 1) * TILE + ox) + m.e);
      const Y1 = Math.round(m.d * ((ty + 1) * TILE + oy) + m.f);
      ctx.drawImage(c, X0, Y0, X1 - X0, Y1 - Y0);
    }
    ctx.restore();
    return;
  }
  for (const [c, tx, ty] of list) ctx.drawImage(c, tx * TILE + ox, ty * TILE + oy, TILE, TILE);
}

/** The view's transform: canvas → CSS pixels of the stage. */
export function viewMatrix(v: ViewState): DOMMatrix {
  const m = new DOMMatrix();
  m.translateSelf(v.x, v.y);
  m.rotateSelf((v.rotation * 180) / Math.PI);
  m.scaleSelf(v.mirror ? -v.scale : v.scale, v.scale);
  return m;
}
