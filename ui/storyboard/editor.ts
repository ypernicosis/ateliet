// The storyboard editor without its panels: the document, the frame on
// screen, the view (zoom, pan, rotation, mirror), the selection, colours,
// undo, autosave and the canvas that shows it all. Panels and tools talk to
// it; it tells them what changed through events:
//
//   "frame"    another frame is on screen
//   "board"    frames, layers or settings changed
//   "view"     zoom, pan, rotation or mirror changed
//   "history"  undo/redo availability
//   "color"    the colour changed
//   "tool"     the tool or its settings changed
//   "selection"
//   "saved" / "dirty"

import { Board, Frame, Layer, Panel, Transform, frameLabel, isPageKind, newLayer } from "./model.ts";
import { addPage } from "./kinds.ts";
import { facingIndex, inside, isJoined, isLeftPage, spreadId, trimRect } from "./pages.ts";
import { StoryDoc, context } from "./store.ts";
import { Compositor, DrawOptions, Floating, Scratch, StrokeOverlay, ViewState, corners, pixelBounds, sourceSize, viewMatrix } from "./render.ts";
import { BoardStep, History, Step, TileStep, cloneBoard, GroupStep, SpreadTarget } from "./history.ts";
import { Events } from "../obsidian/events.ts";

/** Room left around the frame when it is fitted in view. */
export const FIT_MARGIN = 32;

export interface Selection {
  /** Polygon in canvas coordinates. */
  points: { x: number; y: number }[];
  path: Path2D;
  box: { x: number; y: number; w: number; h: number };
}

export function selectionFrom(points: { x: number; y: number }[]): Selection | null {
  if (points.length < 3) return null;
  const path = new Path2D();
  path.moveTo(points[0].x, points[0].y);
  for (const p of points.slice(1)) path.lineTo(p.x, p.y);
  path.closePath();
  const xs = points.map((p) => p.x), ys = points.map((p) => p.y);
  const box = { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) };
  if (box.w < 1 || box.h < 1) return null;
  return { points, path, box };
}

export interface ToolHandler {
  id: string;
  cursor?: string;
  down?(e: PointerEvent, p: { x: number; y: number }): void;
  move?(e: PointerEvent, p: { x: number; y: number }): void;
  up?(e: PointerEvent, p: { x: number; y: number }): void;
  hover?(e: PointerEvent, p: { x: number; y: number }): void;
  /** Extra drawing on the overlay, in canvas coordinates. */
  overlay?(ctx: CanvasRenderingContext2D): void;
  key?(e: KeyboardEvent): boolean;
  cancel?(): void;
  activate?(): void;
  deactivate?(): void;
}

export class Editor extends Events {
  app: any = null;
  doc: StoryDoc;
  comp: Compositor;
  history = new History();
  frame!: Frame;
  view: ViewState = { x: 0, y: 0, scale: 1, rotation: 0, mirror: false };
  stage: HTMLElement;
  canvas: HTMLCanvasElement;
  overlay: HTMLCanvasElement;
  stroke: StrokeOverlay | null = null;
  floating: Floating | null = null;
  selection: Selection | null = null;
  /** Layers hidden for a moment (holding Q hides the trace). */
  hidden = new Set<string>();
  color = "#23221f";
  secondary = "#ffffff";
  tool: ToolHandler | null = null;
  pointer: { x: number; y: number } | null = null;
  /** The picture shown instead of the frame during playback. */
  playing: { pic: HTMLCanvasElement; area: { x: number; y: number; w: number; h: number } } | null = null;
  private dpr = 1;
  private raf = 0;
  private cache: { key: string; below: HTMLCanvasElement | null; above: HTMLCanvasElement | null; active: Layer | null } = { key: "", below: null, above: null, active: null };
  private version = 0;
  private saveTimer = 0;
  private thumbTimers = new Map<string, number>();
  private antsTimer = 0;
  private antsOffset = 0;
  private resizeObserver: ResizeObserver;
  private outside = "#2a2a2a";
  destroyed = false;

  constructor(doc: StoryDoc, stage: HTMLElement) {
    super();
    this.doc = doc;
    doc.retain = () => this.history.layersInUse();
    this.comp = new Compositor(doc);
    this.comp.onImage = () => this.invalidate();
    this.stage = stage;
    this.canvas = stage.createEl("canvas", { cls: "sb-canvas" });
    this.overlay = stage.createEl("canvas", { cls: "sb-overlay" });
    this.history.onChange = () => this.trigger("history");
    this.resizeObserver = new ResizeObserver(() => requestAnimationFrame(() => this.resize()));
    this.resizeObserver.observe(stage);
    this.outside = getComputedStyle(stage).getPropertyValue("--sb-outside").trim() || "#3a3936";
    this.antsTimer = window.setInterval(() => {
      if (!this.selection) return;
      this.antsOffset = (this.antsOffset + 1) % 16;
      this.drawOverlay();
    }, 120);
  }

  destroy() {
    this.destroyed = true;
    this.resizeObserver.disconnect();
    clearInterval(this.antsTimer);
    cancelAnimationFrame(this.raf);
    clearTimeout(this.saveTimer);
    for (const t of this.thumbTimers.values()) clearTimeout(t);
  }

  get board(): Board {
    return this.doc.board;
  }

  get layers(): Layer[] {
    return this.frame.layers.map((id) => this.board.layers[id]).filter(Boolean);
  }

  get active(): Layer | undefined {
    return this.board.layers[this.frame.active];
  }

  get frameIndex() {
    return this.board.frames.indexOf(this.frame);
  }

  // ── frames ──────────────────────────────────────────────────────────────

  async setFrame(target: Frame | number, keepView = true) {
    const f = typeof target === "number" ? this.board.frames[Math.max(0, Math.min(this.board.frames.length - 1, target))] : target;
    if (!f) return;
    this.commitFloating();
    const changed = f !== this.frame;
    this.frame = f;
    this.facingCache.key = "";
    this.wantPictures();
    const need = this.needed();
    this.doc.tiles.pin(need);
    await this.doc.tiles.load(need);
    if (this.frame !== f) return;
    if (!keepView) this.fit();
    this.invalidate();
    if (changed) this.trigger("frame", f);
  }

  /** Reloads the frame after its layers changed (undo, duplicate, import…). */
  async refreshFrame() {
    if (!this.board.frames.includes(this.frame)) this.frame = this.board.frames.find((f) => f.id === this.frame?.id) || this.board.frames[0];
    else this.frame = this.board.frames.find((f) => f.id === this.frame.id)!;
    if (!this.frame.layers.includes(this.frame.active)) this.frame.active = this.frame.layers[this.frame.layers.length - 1];
    this.wantPictures();
    const need = this.needed();
    this.doc.tiles.pin(need);
    await this.doc.tiles.load(need);
    this.invalidate();
    this.trigger("frame", this.frame);
    this.trigger("board");
  }

  // ── pages ───────────────────────────────────────────────────────────────

  /** The facing page of a spread and its offset from this page, or null. */
  facing(): { frame: Frame; dx: number } | null {
    const b = this.board;
    if (!isPageKind(b.kind) || !this.frame) return null;
    const j = facingIndex(b, this.frameIndex);
    if (j < 0) return null;
    // Pages meet at their trim: the bleed of the inner edges overlaps.
    const step = b.width - 2 * (b.page?.bleed || 0);
    return { frame: b.frames[j], dx: isLeftPage(b, this.frameIndex) ? step : -step };
  }

  /** The facing page as drawn on screen, kept until something changes. */
  private facingCache: { key: string; canvas: HTMLCanvasElement | null } = { key: "", canvas: null };

  /** Layers to keep loaded: this frame, its onion-skin neighbours and the facing page. */
  private needed(): string[] {
    const need = this.comp.layersFor(this.frame, true);
    const f = this.facing();
    if (f) need.push(...this.comp.layersFor(f.frame, false));
    return need;
  }

  /** The facing page drawn at screen resolution (null while its tiles load). */
  private facingCanvas(f: { frame: Frame; dx: number }, m: DOMMatrix): HTMLCanvasElement | null {
    const ids = this.comp.layersFor(f.frame, false);
    if (!ids.every((id) => this.doc.tiles.isLoaded(id))) {
      this.doc.tiles.load(ids).then(() => this.invalidate());
      return null;
    }
    const key = `${this.version}|${this.canvas.width}x${this.canvas.height}|${f.frame.id}|${f.dx}`;
    if (this.facingCache.key === key) return this.facingCache.canvas;
    const r = this.visibleRegion();
    this.facingCache.canvas = this.buildCache(this.facingCache.canvas, (c) => {
      c.setTransform(m.translate(f.dx, 0));
      this.comp.drawFrame(c, f.frame, { region: { x: r.x - f.dx, y: r.y, w: r.w, h: r.h }, background: true, onion: false, hidden: this.hidden });
    });
    this.facingCache.key = key;
    return this.facingCache.canvas;
  }

  // ── spreads drawn as one ────────────────────────────────────────────────

  /** The facing page when this spread is drawn as one. */
  joinedFacing(): { frame: Frame; dx: number } | null {
    const f = this.facing();
    return f && isJoined(this.board, this.frameIndex) ? f : null;
  }

  /** Where drawing may reach: the page, or both pages of a spread drawn as one. */
  drawArea(): { x: number; y: number; w: number; h: number } {
    const b = this.board;
    const f = this.joinedFacing();
    const t = f && this.spreadTarget();
    if (!f || !t || !this.doc.tiles.isLoaded(t.layer.id)) return { x: 0, y: 0, w: b.width, h: b.height };
    return { x: Math.min(0, f.dx), y: 0, w: b.width + Math.abs(f.dx), h: b.height };
  }

  /**
   * The layer on the facing page that receives what is drawn on `l` across
   * the spine: the one with the same name, else that page's active layer,
   * else its top drawing layer. None for a layer shown on every page.
   */
  spreadTarget(l: Layer | null | undefined = this.active): SpreadTarget | null {
    const f = this.joinedFacing();
    if (!f || !l || l.kind !== "raster") return null;
    const layers = f.frame.layers.map((id) => this.board.layers[id]).filter((x): x is Layer => !!x && x.kind === "raster");
    if (layers.some((x) => x.id === l.id)) return null;
    const target = layers.find((x) => x.name === l.name) || layers.find((x) => x.id === f.frame.active) || layers[layers.length - 1];
    if (!target || target.locked) return null;
    return { frame: f.frame, dx: f.dx, layer: target };
  }

  /** Joins this spread into one drawing surface, or separates it again. Nothing drawn is lost either way. */
  toggleJoined() {
    const id = spreadId(this.board, this.frameIndex);
    if (!id || !this.board.page) return;
    const on = isJoined(this.board, this.frameIndex);
    this.mutate(on ? "Separate pages" : "Join pages", (b) => {
      const list = new Set(b.page!.joined || []);
      if (on) list.delete(id);
      else list.add(id);
      b.page!.joined = [...list];
    });
    this.doc.tiles.pin(this.needed());
  }

  /**
   * After a stroke on a spread drawn as one: what went past this page's edge
   * moves to the facing page (tools that already painted both pages only
   * clear it), so each page holds exactly its own part.
   */
  private settleSpread(step: TileStep) {
    const sp = step.spread!;
    const l = this.board.layers[step.layer];
    const fl = sp.target.layer;
    if (!l) return;
    const b = this.board;
    const dx = sp.target.dx;
    // Past this page's edge, in this layer's coordinates.
    const over = { x: (dx < 0 ? dx : b.width) - l.x, y: -l.y, w: Math.abs(dx), h: b.height };
    const T = 512;
    for (const k of [...step.before.keys()]) {
      const [tx, ty] = k.split("_").map(Number);
      const x0 = Math.max(tx * T, over.x), y0 = Math.max(ty * T, over.y);
      const x1 = Math.min((tx + 1) * T, over.x + over.w), y1 = Math.min((ty + 1) * T, over.y + over.h);
      if (x1 <= x0 || y1 <= y0) continue;
      const tile = this.doc.tiles.get(l.id, tx, ty);
      if (!tile) continue;
      if (!sp.handled) {
        // Into the facing layer (its coordinates: + this layer's offset − dx − its offset).
        const ox = l.x - dx - fl.x, oy = l.y - fl.y;
        this.eachTile(x0 + ox, y0 + oy, x1 - x0, y1 - y0, (fx, fy) => sp.step.touch(fx, fy));
        this.doc.tiles.paint(fl.id, x0 + ox, y0 + oy, x1 - x0, y1 - y0, (ctx) => {
          ctx.beginPath();
          ctx.rect(0, 0, x1 - x0, y1 - y0);
          ctx.clip();
          if (fl.alphaLock) ctx.globalCompositeOperation = "source-atop";
          ctx.drawImage(tile, tx * T - x0, ty * T - y0);
        });
      }
      const ctx = context(tile);
      ctx.clearRect(x0 - tx * T, y0 - ty * T, x1 - x0, y1 - y0);
      this.doc.tiles.markDirty(l.id, tx, ty);
    }
  }

  /** Clicking the facing page makes it the page being drawn, without the view jumping. */
  async switchToFacing() {
    const f = this.facing();
    if (!f) return;
    const at = this.toStage(f.dx, 0);
    await this.setFrame(f.frame);
    const now = this.toStage(0, 0);
    this.view.x += at.x - now.x;
    this.view.y += at.y - now.y;
    this.viewChanged();
  }

  /** The panel under a canvas point (the last drawn wins). */
  panelAt(x: number, y: number, frame: Frame = this.frame): Panel | undefined {
    const panels = frame?.panels || [];
    for (let i = panels.length - 1; i >= 0; i--) if (inside(panels[i].points, { x, y })) return panels[i];
    return undefined;
  }

  /** Starts loading the pictures of the frame and of its onion-skin neighbours. */
  private wantPictures() {
    const b = this.board, i = this.frameIndex;
    this.comp.want(this.frame);
    if (!b.onion.enabled || !b.onion.pictures) return;
    for (let k = -b.onion.before; k <= b.onion.after; k++) if (k && b.frames[i + k]) this.comp.want(b.frames[i + k]);
  }

  frameName(f: Frame = this.frame) {
    return frameLabel(this.board, f);
  }

  // ── changes and undo ────────────────────────────────────────────────────

  /** A change to the structure (frames, layers, settings), undoable. */
  mutate(label: string, fn: (b: Board) => void, extra: Step[] = []) {
    const before = cloneBoard(this.board);
    fn(this.board);
    this.facingCache.key = "";
    const after = cloneBoard(this.board);
    this.history.push(new BoardStep(label, this.doc, before, after, extra));
    this.changed(true);
    this.trigger("board");
  }

  /** Records, as one undo step, everything that changed since `before` (imports). */
  recordSince(label: string, before: Board) {
    this.history.push(new BoardStep(label, this.doc, before, cloneBoard(this.board)));
    this.changed(true);
    this.trigger("board");
  }

  /** Records a pixel change; call `done()` when it is finished. */
  pixels(label: string, layer: string): TileStep {
    const step = new TileStep(label, this.doc, layer);
    let target = layer === this.active?.id ? this.spreadTarget() : null;
    // Drawing into tiles that are not decoded yet would replace them.
    if (target && !this.doc.tiles.isLoaded(target.layer.id)) {
      this.doc.tiles.load([target.layer.id]);
      target = null;
    }
    if (target) step.spread = { target, step: new TileStep(label, this.doc, target.layer.id), handled: false };
    return step;
  }

  commitPixels(step: TileStep) {
    if (step.spread) this.settleSpread(step);
    const done = [step, step.spread?.step].filter((s): s is TileStep => !!s && s.finish());
    if (!done.length) return;
    this.history.push(done.length > 1 ? new GroupStep(step.label, done) : done[0]);
    this.changed(true);
    if (step.spread) this.scheduleThumb(step.spread.target.frame);
  }

  async undo() {
    if (this.floating) return this.cancelFloating();
    const s = this.history.undo();
    if (s) await this.afterHistory(s);
  }

  async redo() {
    const s = this.history.redo();
    if (s) await this.afterHistory(s);
  }

  private async afterHistory(s: Step) {
    if (s instanceof BoardStep) await this.refreshFrame();
    else this.invalidate();
    this.changed(true);
  }

  /** Something changed: redraw, save soon, update the frame's picture. */
  changed(pixels = false) {
    this.doc.changed = true;
    this.invalidate();
    this.trigger("dirty");
    clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => this.save(), 2500);
    if (pixels) this.scheduleThumb();
  }

  scheduleThumb(frame: Frame = this.frame) {
    clearTimeout(this.thumbTimers.get(frame.id));
    this.thumbTimers.set(frame.id, window.setTimeout(() => {
      this.thumbTimers.delete(frame.id);
      this.updateThumb(frame);
    }, 700));
  }

  async updateThumb(frame: Frame) {
    if (this.destroyed || !this.board.frames.includes(frame)) return;
    try {
      const area = this.comp.area(frame);
      // A webtoon strip is summed up by its top, the way a reader first meets it.
      if (this.board.kind === "webtoon") area.h = Math.min(area.h, Math.round(area.w * 1.6));
      const scale = Math.min(1, 320 / Math.max(area.w, area.h));
      const pic = await this.comp.picture(frame, scale, { exporting: false, area });
      const blob: Blob | null = await new Promise((r) => pic.toBlob(r, "image/png"));
      if (!blob) return;
      this.doc.thumbs.set(frame.id, new Uint8Array(await blob.arrayBuffer()));
      if (frame === this.board.frames[0]) {
        const big = await this.comp.picture(frame, Math.min(1, 720 / Math.max(area.w, area.h)), { exporting: true, area });
        const b2: Blob | null = await new Promise((r) => big.toBlob(r, "image/png"));
        if (b2) this.doc.preview = new Uint8Array(await b2.arrayBuffer());
      }
      this.trigger("thumb", frame);
    } catch (e) {
      console.warn("storyboard thumbnail", e);
    }
  }

  async save() {
    clearTimeout(this.saveTimer);
    if (!this.doc.changed) return;
    if (!this.doc.preview || !this.doc.thumbs.size) await this.updateThumb(this.board.frames[0]);
    try {
      await this.doc.save();
      this.trigger("saved");
    } catch (e) {
      this.trigger("save-error", e);
    }
  }

  // ── layers ──────────────────────────────────────────────────────────────

  setActive(id: string) {
    if (this.frame.active === id) return;
    this.commitFloating();
    this.frame.active = id;
    this.invalidate();
    this.trigger("board");
  }

  addLayer(kind: "raster" | "image" | "trace" = "raster", extra: Partial<Layer> = {}, at?: number): Layer {
    const count = this.layers.filter((l) => l.kind === kind).length + 1;
    const name = kind === "trace" ? "Trace" : kind === "image" ? "Picture" : `Layer ${count}`;
    const l = newLayer(name, kind, extra);
    this.mutate(`Add ${kind === "raster" ? "layer" : name.toLowerCase()}`, (b) => {
      b.layers[l.id] = l;
      const i = at ?? this.frame.layers.indexOf(this.frame.active) + 1;
      if (l.shared) for (const f of b.frames) f.layers.splice(Math.min(i, f.layers.length), 0, l.id);
      else this.frame.layers.splice(i, 0, l.id);
      this.frame.active = l.id;
    });
    return l;
  }

  // ── view ────────────────────────────────────────────────────────────────

  resize() {
    const r = this.stage.getBoundingClientRect();
    this.dpr = window.devicePixelRatio || 1;
    for (const c of [this.canvas, this.overlay]) {
      c.width = Math.max(1, Math.round(r.width * this.dpr));
      c.height = Math.max(1, Math.round(r.height * this.dpr));
      c.style.width = `${r.width}px`;
      c.style.height = `${r.height}px`;
    }
    if (this.autoFit) this.fit();
    this.invalidate();
  }

  /** True until the user zooms or pans: the frame then follows the stage's size. */
  private autoFit = true;

  /** What "fit" puts in view: the frame, both pages of a spread, or (infinite canvas) the drawn area. */
  private fitArea(): { x: number; y: number; w: number; h: number } {
    const area = this.board.infinite ? this.comp.area(this.frame, false) : { x: 0, y: 0, w: this.board.width, h: this.board.height };
    // A spread: both pages in view.
    const f = this.facing();
    if (f) {
      area.x = Math.min(0, f.dx);
      area.w = this.board.width + Math.abs(f.dx);
    }
    return area;
  }

  /** The shape of what is fitted in view; null when it has no edges or is read by scrolling. */
  fitAspect(): number | null {
    if (this.board.infinite || !this.frame) return null;
    const area = this.fitArea();
    if (this.board.kind === "webtoon" && area.h > area.w * 2) return null;
    return area.w / area.h;
  }

  /** The whole frame in view (infinite canvas: the drawn area, or the start size). */
  fit() {
    const r = this.stage.getBoundingClientRect();
    if (!r.width || !r.height || !this.frame) return;
    const area = this.fitArea();
    const margin = FIT_MARGIN;
    this.view.rotation = 0;
    if (this.board.kind === "webtoon" && area.h > area.w * 2) {
      // A long strip is read by scrolling: fit its width (up to the phone-ish size) and start at the top.
      this.view.scale = Math.max(0.02, Math.min(1, (r.width - 2 * margin) / area.w));
      this.centerOn(area.w / 2, (r.height / 2 - margin) / this.view.scale);
      this.autoFit = true;
      return;
    }
    const s = Math.min((r.width - 2 * margin) / area.w, (r.height - 2 * margin) / area.h);
    this.view.scale = Math.max(0.02, Math.min(8, s));
    this.centerOn(area.x + area.w / 2, area.y + area.h / 2);
    this.autoFit = true;
  }

  /** Puts canvas point (x, y) at the middle of the stage. */
  centerOn(x: number, y: number) {
    const r = this.stage.getBoundingClientRect();
    const m = viewMatrix({ ...this.view, x: 0, y: 0 });
    const p = m.transformPoint(new DOMPoint(x, y));
    this.view.x = r.width / 2 - p.x;
    this.view.y = r.height / 2 - p.y;
    this.viewChanged();
  }

  zoomAt(factor: number, sx?: number, sy?: number) {
    const r = this.stage.getBoundingClientRect();
    sx ??= r.width / 2;
    sy ??= r.height / 2;
    const p = this.toCanvas(sx, sy);
    this.view.scale = Math.max(0.02, Math.min(32, this.view.scale * factor));
    const q = viewMatrix(this.view).transformPoint(new DOMPoint(p.x, p.y));
    this.view.x += sx - q.x;
    this.view.y += sy - q.y;
    this.viewChanged();
  }

  setZoom(scale: number) {
    this.zoomAt(scale / this.view.scale);
  }

  panBy(dx: number, dy: number) {
    this.view.x += dx;
    this.view.y += dy;
    this.viewChanged();
  }

  /** Rotates the view around a stage point (default: its centre). */
  rotateTo(rad: number, sx?: number, sy?: number) {
    const r = this.stage.getBoundingClientRect();
    sx ??= r.width / 2;
    sy ??= r.height / 2;
    const p = this.toCanvas(sx, sy);
    this.view.rotation = rad;
    const q = viewMatrix(this.view).transformPoint(new DOMPoint(p.x, p.y));
    this.view.x += sx - q.x;
    this.view.y += sy - q.y;
    this.viewChanged();
  }

  /** Mirrors the view around the stage centre (a check: the drawing is not changed). */
  setMirror(on: boolean) {
    if (on === this.view.mirror) return;
    const r = this.stage.getBoundingClientRect();
    const p = this.toCanvas(r.width / 2, r.height / 2);
    this.view.mirror = on;
    this.board.guides.mirror = on;
    const q = viewMatrix(this.view).transformPoint(new DOMPoint(p.x, p.y));
    this.view.x += r.width / 2 - q.x;
    this.view.y += r.height / 2 - q.y;
    this.viewChanged();
  }

  viewChanged() {
    this.autoFit = false;
    this.invalidate();
    this.trigger("view");
  }

  /** Stage (CSS pixel) point → canvas coordinates. */
  toCanvas(sx: number, sy: number): { x: number; y: number } {
    const p = viewMatrix(this.view).inverse().transformPoint(new DOMPoint(sx, sy));
    return { x: p.x, y: p.y };
  }

  /** Client point (from an event) → canvas coordinates. */
  eventPoint(e: { clientX: number; clientY: number }): { x: number; y: number } {
    const r = this.stage.getBoundingClientRect();
    return this.toCanvas(e.clientX - r.left, e.clientY - r.top);
  }

  toStage(x: number, y: number): { x: number; y: number } {
    const p = viewMatrix(this.view).transformPoint(new DOMPoint(x, y));
    return { x: p.x, y: p.y };
  }

  /** The part of the canvas that is on screen. */
  visibleRegion(): { x: number; y: number; w: number; h: number } {
    const r = this.stage.getBoundingClientRect();
    const pts = [this.toCanvas(0, 0), this.toCanvas(r.width, 0), this.toCanvas(0, r.height), this.toCanvas(r.width, r.height)];
    const xs = pts.map((p) => p.x), ys = pts.map((p) => p.y);
    return { x: Math.min(...xs) - 2, y: Math.min(...ys) - 2, w: Math.max(...xs) - Math.min(...xs) + 4, h: Math.max(...ys) - Math.min(...ys) + 4 };
  }

  /** True when a canvas point is where drawing is allowed. */
  inBounds(x: number, y: number) {
    return this.board.infinite || (x >= 0 && y >= 0 && x <= this.board.width && y <= this.board.height);
  }

  // ── drawing the screen ──────────────────────────────────────────────────

  /** Something that is not the active layer changed: rebuild everything. */
  invalidate() {
    this.version++;
    this.requestRender();
  }

  requestRender() {
    if (this.raf || this.destroyed) return;
    this.raf = requestAnimationFrame(() => {
      this.raf = 0;
      this.renderNow();
    });
  }

  private matrix(): DOMMatrix {
    return new DOMMatrix().scaleSelf(this.dpr, this.dpr).multiplySelf(viewMatrix(this.view));
  }

  private options(): DrawOptions {
    return { region: this.visibleRegion(), background: true, stroke: this.stroke, floating: this.floating, hidden: this.hidden, onion: !this.playing };
  }

  /** Layers in drawing order (trace layers shown above go last). */
  private ordered(o: DrawOptions): Layer[] {
    const vis = this.comp.visibleLayers(this.frame, o);
    return [...vis.filter((l) => !(l.kind === "trace" && l.trace?.above)), ...vis.filter((l) => l.kind === "trace" && l.trace?.above)];
  }

  renderNow() {
    if (!this.frame || this.destroyed) return;
    const ctx = this.canvas.getContext("2d")!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = "source-over";
    ctx.fillStyle = this.outside;
    ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    const m = this.matrix();
    const facing = this.playing ? null : this.facing();
    if (facing) {
      const pic = this.facingCanvas(facing, m);
      if (pic) ctx.drawImage(pic, 0, 0);
      else {
        ctx.setTransform(m);
        ctx.fillStyle = this.board.background === "transparent" ? "#fff" : this.board.background;
        ctx.fillRect(facing.dx, 0, this.board.width, this.board.height);
        ctx.setTransform(1, 0, 0, 1, 0, 0);
      }
    }
    const finish = () => {
      if (facing) this.spine(ctx, m, facing);
      this.drawOverlay();
    };
    if (this.playing) {
      // Playback shows prepared pictures of whole frames.
      const area = this.playing.area;
      ctx.setTransform(m);
      if (!this.board.infinite) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(0, 0, this.board.width, this.board.height);
        ctx.clip();
      }
      ctx.drawImage(this.playing.pic, area.x, area.y, area.w, area.h);
      if (!this.board.infinite) ctx.restore();
      finish();
      return;
    }
    const o = this.options();
    const order = this.ordered(o);
    const active = this.active;
    const i = active ? order.indexOf(active) : -1;
    const above = i >= 0 ? order.slice(i + 1) : [];
    const cacheable = i >= 0 && active!.kind === "raster" && !active!.clip && !(above[0]?.clip) && above.every((l) => l.blend === "source-over" && !l.clip);
    if (!cacheable) {
      ctx.setTransform(m);
      this.comp.drawFrame(ctx, this.frame, o);
      this.cache.key = "";
      finish();
      return;
    }
    const key = `${this.version}|${this.canvas.width}x${this.canvas.height}|${active!.id}|${active!.opacity}|${active!.blend}|${active!.visible}`;
    const scratch = new Scratch(this.canvas.width, this.canvas.height);
    if (this.cache.key !== key) {
      this.cache.below = this.buildCache(this.cache.below, (c) => {
        c.setTransform(m);
        this.clipToCanvas(c);
        this.comp.paper(c, o.region);
        if (o.onion && this.board.onion.enabled) this.comp.onion(c, this.frame, o, scratch, m);
        this.comp.drawLayers(c, order.slice(0, i), { ...o, stroke: null, floating: null }, scratch, m);
      });
      this.cache.above = above.length
        ? this.buildCache(this.cache.above, (c) => {
            c.setTransform(m);
            this.clipToCanvas(c);
            this.comp.drawLayers(c, above, { ...o, stroke: null, floating: null }, scratch, m);
          })
        : null;
      this.cache.key = key;
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.drawImage(this.cache.below!, 0, 0);
    ctx.setTransform(m);
    ctx.save();
    this.clipToCanvas(ctx);
    this.comp.drawLayers(ctx, [active!], o, scratch, m);
    ctx.restore();
    scratch.release();
    if (this.cache.above) {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.drawImage(this.cache.above, 0, 0);
    }
    ctx.setTransform(m);
    ctx.save();
    this.clipToCanvas(ctx);
    this.comp.decor(ctx, this.frame, true);
    ctx.restore();
    finish();
  }

  /** Pages meet at the trim: the facing page covers this page's bleed on the spine side, which would be cut away. */
  private spine(ctx: CanvasRenderingContext2D, m: DOMMatrix, f: { frame: Frame; dx: number }) {
    const pic = this.facingCanvas(f, m);
    const bleed = this.board.page?.bleed || 0;
    if (!pic || !bleed) return;
    const w = this.board.width, h = this.board.height;
    const x = f.dx < 0 ? 0 : w - bleed;
    ctx.save();
    ctx.setTransform(m);
    ctx.beginPath();
    ctx.rect(x, 0, bleed, h);
    ctx.clip();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.drawImage(pic, 0, 0);
    ctx.restore();
  }

  private clipToCanvas(c: CanvasRenderingContext2D) {
    if (this.board.infinite) return;
    // A spread drawn as one: a stroke in progress shows across both pages.
    const r = this.drawArea();
    c.beginPath();
    c.rect(r.x, r.y, r.w, r.h);
    c.clip();
  }

  private buildCache(prev: HTMLCanvasElement | null, draw: (c: CanvasRenderingContext2D) => void): HTMLCanvasElement {
    const c = prev && prev.width === this.canvas.width && prev.height === this.canvas.height ? prev : Object.assign(document.createElement("canvas"), { width: this.canvas.width, height: this.canvas.height });
    const ctx = c.getContext("2d")!;
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, c.width, c.height);
    draw(ctx);
    ctx.restore();
    return c;
  }

  /** Guides, frame edge, selection, handles and the tool's own marks. */
  drawOverlay() {
    const ctx = this.overlay.getContext("2d")!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, this.overlay.width, this.overlay.height);
    const m = this.matrix();
    ctx.setTransform(m);
    const px = 1 / (this.view.scale * this.dpr);
    const b = this.board;
    if (!b.infinite) {
      ctx.lineWidth = px;
      ctx.strokeStyle = "rgba(0,0,0,.35)";
      ctx.strokeRect(0, 0, b.width, b.height);
      if (isPageKind(b.kind)) this.pageGuides(ctx, px);
      this.guides(ctx, isPageKind(b.kind) ? trimRect(b) : { x: 0, y: 0, w: b.width, h: b.height }, px);
    } else if (b.guides.thirds || b.guides.center || b.guides.safe || b.guides.grid) {
      // Infinite canvas: guides frame the starting size, centred on the origin area.
      ctx.lineWidth = px;
      ctx.setLineDash([6 * px, 6 * px]);
      ctx.strokeStyle = "rgba(0,0,0,.3)";
      ctx.strokeRect(0, 0, b.width, b.height);
      ctx.setLineDash([]);
      this.guides(ctx, { x: 0, y: 0, w: b.width, h: b.height }, px);
    }
    if (this.selection) {
      ctx.lineWidth = px;
      ctx.setLineDash([4 * px, 4 * px]);
      ctx.lineDashOffset = -this.antsOffset * px;
      ctx.strokeStyle = "#000";
      ctx.stroke(this.selection.path);
      ctx.lineDashOffset = -this.antsOffset * px + 4 * px;
      ctx.strokeStyle = "#fff";
      ctx.stroke(this.selection.path);
      ctx.setLineDash([]);
    }
    if (this.floating) this.handles(ctx, this.floating.t, this.floating.canvas.width, this.floating.canvas.height, px);
    this.tool?.overlay?.(ctx);
  }

  /** Trim, inner frame, the facing page's edge and (webtoon) the upload slices. */
  private pageGuides(ctx: CanvasRenderingContext2D, px: number) {
    const b = this.board, p = b.page!;
    ctx.save();
    ctx.lineWidth = px;
    if (p.bleed) {
      const t = trimRect(b);
      ctx.strokeStyle = "rgba(0,0,0,.55)";
      ctx.strokeRect(t.x, t.y, t.w, t.h);
    }
    if (b.kind !== "webtoon" && p.inner.w) {
      ctx.setLineDash([6 * px, 4 * px]);
      ctx.strokeStyle = "rgba(47,128,237,.7)";
      ctx.strokeRect(p.inner.x, p.inner.y, p.inner.w, p.inner.h);
      ctx.setLineDash([]);
    }
    const f = this.facing();
    if (f) {
      const t = trimRect(b);
      ctx.strokeStyle = "rgba(0,0,0,.35)";
      ctx.strokeRect(t.x + f.dx, t.y, t.w, t.h);
    }
    if (b.kind === "webtoon" && p.slice) {
      // Where the platform cuts the strip: keep faces and balloons off these lines.
      ctx.setLineDash([10 * px, 6 * px]);
      ctx.strokeStyle = "rgba(220,90,40,.55)";
      ctx.font = `${11 * px}px system-ui, sans-serif`;
      ctx.fillStyle = "rgba(220,90,40,.8)";
      for (let y = p.slice, n = 1; y < b.height; y += p.slice, n++) {
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(b.width, y);
        ctx.stroke();
        ctx.fillText(String(n + 1), b.width + 6 * px, y + 12 * px);
      }
      ctx.setLineDash([]);
      ctx.strokeStyle = "rgba(47,128,237,.4)";
      ctx.strokeRect(p.inner.x, 0, p.inner.w, b.height);
    }
    ctx.restore();
  }

  /** Thirds, centre, safe areas and the drawing grid over a rectangle. */
  guides(ctx: CanvasRenderingContext2D, r: { x: number; y: number; w: number; h: number }, px: number) {
    const g = this.board.guides;
    ctx.save();
    ctx.lineWidth = px;
    if (g.grid > 0) {
      ctx.strokeStyle = "rgba(40,120,220,.45)";
      const n = g.grid;
      const step = Math.max(r.w, r.h) / n;
      ctx.beginPath();
      for (let x = r.x + step; x < r.x + r.w - 0.5; x += step) {
        ctx.moveTo(x, r.y);
        ctx.lineTo(x, r.y + r.h);
      }
      for (let y = r.y + step; y < r.y + r.h - 0.5; y += step) {
        ctx.moveTo(r.x, y);
        ctx.lineTo(r.x + r.w, y);
      }
      ctx.stroke();
    }
    if (g.thirds) {
      ctx.strokeStyle = "rgba(255,255,255,.7)";
      ctx.setLineDash([6 * px, 4 * px]);
      ctx.beginPath();
      for (const k of [1, 2]) {
        ctx.moveTo(r.x + (r.w * k) / 3, r.y);
        ctx.lineTo(r.x + (r.w * k) / 3, r.y + r.h);
        ctx.moveTo(r.x, r.y + (r.h * k) / 3);
        ctx.lineTo(r.x + r.w, r.y + (r.h * k) / 3);
      }
      ctx.stroke();
      ctx.setLineDash([]);
    }
    if (g.center) {
      ctx.strokeStyle = "rgba(220,60,60,.75)";
      const cx = r.x + r.w / 2, cy = r.y + r.h / 2, k = Math.min(r.w, r.h) * 0.04;
      ctx.beginPath();
      ctx.moveTo(cx - k, cy);
      ctx.lineTo(cx + k, cy);
      ctx.moveTo(cx, cy - k);
      ctx.lineTo(cx, cy + k);
      ctx.stroke();
    }
    if (g.safe) {
      ctx.strokeStyle = "rgba(240,180,40,.8)";
      for (const k of [0.05, 0.1]) ctx.strokeRect(r.x + r.w * k, r.y + r.h * k, r.w * (1 - 2 * k), r.h * (1 - 2 * k));
    }
    ctx.restore();
  }

  /** Transform handles around a placed picture. */
  handles(ctx: CanvasRenderingContext2D, t: Transform, w: number, h: number, px: number) {
    const cs = corners(t, w, h);
    ctx.save();
    ctx.lineWidth = px * 1.5;
    ctx.strokeStyle = "#2f80ed";
    ctx.beginPath();
    cs.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
    ctx.closePath();
    ctx.stroke();
    ctx.fillStyle = "#fff";
    const k = 5 * px;
    for (const p of [...cs, ...cs.map((p, i) => ({ x: (p.x + cs[(i + 1) % 4].x) / 2, y: (p.y + cs[(i + 1) % 4].y) / 2 }))]) {
      ctx.fillRect(p.x - k, p.y - k, 2 * k, 2 * k);
      ctx.strokeRect(p.x - k, p.y - k, 2 * k, 2 * k);
    }
    // Rotation handle above the top edge.
    const top = { x: (cs[0].x + cs[1].x) / 2, y: (cs[0].y + cs[1].y) / 2 };
    const dir = Math.atan2(cs[1].y - cs[0].y, cs[1].x - cs[0].x) - Math.PI / 2;
    const rh = { x: top.x + Math.cos(dir) * 28 * px * Math.sign(t.sy || 1), y: top.y + Math.sin(dir) * 28 * px * Math.sign(t.sy || 1) };
    ctx.beginPath();
    ctx.moveTo(top.x, top.y);
    ctx.lineTo(rh.x, rh.y);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(rh.x, rh.y, k, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
    ctx.restore();
  }

  // ── lifted pixels (move / transform a selection or a whole layer) ───────

  private floatStep: TileStep | null = null;
  private floatBefore: Board | null = null;

  /** Lifts the selection (or the whole layer) off the active raster layer. */
  async liftSelection(): Promise<boolean> {
    const l = this.active;
    if (!l || l.kind !== "raster" || l.locked || this.floating) return false;
    await this.doc.tiles.load([l.id]);
    const sel = this.selection;
    const box = sel ? sel.box : pixelBounds(this.doc, l);
    if (!box) return false;
    const x0 = Math.floor(box.x), y0 = Math.floor(box.y);
    const w = Math.ceil(box.x + box.w) - x0, h = Math.ceil(box.y + box.h) - y0;
    if (w < 1 || h < 1) return false;
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const ctx = c.getContext("2d")!;
    ctx.translate(-x0, -y0);
    if (sel) ctx.clip(sel.path);
    this.comp.tiles(ctx, l.id, l.x, l.y, { x: x0, y: y0, w, h });
    const step = this.pixels("Transform", l.id);
    const lx = x0 - l.x, ly = y0 - l.y;
    const clipPath = sel ? translated(sel.path, -l.x, -l.y) : null;
    this.eachTile(lx, ly, w, h, (tx, ty) => step.touch(tx, ty));
    this.doc.tiles.paint(l.id, lx, ly, w, h, (t) => {
      t.globalCompositeOperation = "destination-out";
      t.translate(-lx, -ly);
      if (clipPath) t.fill(clipPath);
      else t.fillRect(lx, ly, w, h);
    });
    this.floatStep = step;
    this.floatBefore = cloneBoard(this.board);
    this.floating = { layer: l.id, canvas: c, t: { x: x0 + w / 2, y: y0 + h / 2, sx: 1, sy: 1, rotation: 0 } };
    this.selection = null;
    this.invalidate();
    this.trigger("selection");
    return true;
  }

  /** Puts lifted pixels down where they are now. */
  commitFloating() {
    const f = this.floating;
    if (!f) return;
    const l = this.board.layers[f.layer];
    this.floating = null;
    if (l) {
      const step = this.floatStep!;
      const cs = corners(f.t, f.canvas.width, f.canvas.height);
      const xs = cs.map((p) => p.x - l.x), ys = cs.map((p) => p.y - l.y);
      const x0 = Math.floor(Math.min(...xs)) - 1, y0 = Math.floor(Math.min(...ys)) - 1;
      const w = Math.ceil(Math.max(...xs)) - x0 + 2, h = Math.ceil(Math.max(...ys)) - y0 + 2;
      this.eachTile(x0, y0, w, h, (tx, ty) => step.touch(tx, ty));
      this.doc.tiles.paint(
        l.id, x0, y0, w, h,
        (ctx) => {
          ctx.translate(-x0 - l.x, -y0 - l.y);
          if (!this.board.infinite) {
            ctx.beginPath();
            ctx.rect(0, 0, this.board.width, this.board.height);
            ctx.clip();
          }
          ctx.imageSmoothingQuality = "high";
          ctx.translate(f.t.x, f.t.y);
          ctx.rotate(f.t.rotation);
          ctx.scale(f.t.sx, f.t.sy);
          ctx.drawImage(f.canvas, -f.canvas.width / 2, -f.canvas.height / 2);
        },
      );
      if (step.finish()) this.history.push(step);
    }
    this.floatStep = null;
    this.floatBefore = null;
    this.changed(true);
    this.trigger("selection");
  }

  /** Pixels placed over a layer to be moved before they go down (paste). */
  startFloating(layer: string, canvas: HTMLCanvasElement, t: Transform) {
    this.commitFloating();
    this.floatStep = this.pixels("Paste", layer);
    this.floating = { layer, canvas, t };
    this.invalidate();
    this.trigger("selection");
  }

  /** The selected pixels of the active layer (or all of it) as a picture. */
  copyPixels(): { canvas: HTMLCanvasElement; x: number; y: number } | null {
    const l = this.active;
    if (!l || l.kind !== "raster") return null;
    const sel = this.selection;
    const box = sel ? sel.box : pixelBounds(this.doc, l);
    if (!box) return null;
    const x0 = Math.floor(box.x), y0 = Math.floor(box.y);
    const w = Math.ceil(box.x + box.w) - x0, h = Math.ceil(box.y + box.h) - y0;
    if (w < 1 || h < 1) return null;
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const ctx = c.getContext("2d")!;
    ctx.translate(-x0, -y0);
    if (sel) ctx.clip(sel.path);
    this.comp.tiles(ctx, l.id, l.x, l.y, { x: x0, y: y0, w, h });
    return { canvas: c, x: x0, y: y0 };
  }

  /** What is seen (every visible layer and the paper) inside the selection, or the whole frame. */
  async copyMerged(): Promise<{ canvas: HTMLCanvasElement; x: number; y: number } | null> {
    if (!this.frame) return null;
    const sel = this.selection;
    let area = this.comp.area(this.frame);
    if (sel) {
      const x0 = Math.floor(sel.box.x), y0 = Math.floor(sel.box.y);
      area = { x: x0, y: y0, w: Math.ceil(sel.box.x + sel.box.w) - x0, h: Math.ceil(sel.box.y + sel.box.h) - y0 };
    }
    if (area.w < 1 || area.h < 1) return null;
    const c = await this.comp.picture(this.frame, 1, { exporting: true, area });
    if (sel) {
      const ctx = c.getContext("2d")!;
      ctx.setTransform(1, 0, 0, 1, -area.x, -area.y);
      ctx.globalCompositeOperation = "destination-in";
      ctx.fill(sel.path);
    }
    return { canvas: c, x: area.x, y: area.y };
  }

  /** Clears the selected pixels of the active layer. */
  clearSelection() {
    const l = this.active;
    const sel = this.selection;
    if (!l || l.kind !== "raster" || l.locked || !sel) return;
    const step = this.pixels("Clear", l.id);
    const lx = Math.floor(sel.box.x - l.x) - 1, ly = Math.floor(sel.box.y - l.y) - 1;
    const w = Math.ceil(sel.box.w) + 3, h = Math.ceil(sel.box.h) + 3;
    const path = translated(sel.path, -l.x, -l.y);
    this.eachTile(lx, ly, w, h, (tx, ty) => {
      if (this.doc.tiles.has(l.id, tx, ty)) step.touch(tx, ty);
    });
    for (const k of step.before.keys()) {
      const [tx, ty] = k.split("_").map(Number);
      const ctx = this.doc.tiles.ensure(l.id, tx, ty).getContext("2d")!;
      ctx.save();
      ctx.translate(-tx * 512, -ty * 512);
      ctx.globalCompositeOperation = "destination-out";
      ctx.fill(path);
      ctx.restore();
      this.doc.tiles.markDirty(l.id, tx, ty);
    }
    this.commitPixels(step);
  }

  /** Fills the selection with the colour on the active layer. */
  fillSelection() {
    const l = this.active;
    const sel = this.selection;
    if (!l || l.kind !== "raster" || l.locked || !sel) return;
    const step = this.pixels("Fill selection", l.id);
    const lx = Math.floor(sel.box.x - l.x) - 1, ly = Math.floor(sel.box.y - l.y) - 1;
    const w = Math.ceil(sel.box.w) + 3, h = Math.ceil(sel.box.h) + 3;
    const path = translated(sel.path, -l.x, -l.y);
    this.eachTile(lx, ly, w, h, (tx, ty) => step.touch(tx, ty));
    this.doc.tiles.paint(l.id, lx, ly, w, h, (ctx) => {
      ctx.translate(-lx, -ly);
      if (l.alphaLock) ctx.globalCompositeOperation = "source-atop";
      ctx.fillStyle = this.color;
      ctx.fill(path);
    });
    this.commitPixels(step);
  }

  cancelFloating() {
    const f = this.floating;
    if (!f) return;
    this.floating = null;
    this.floatStep?.finish();
    this.floatStep?.undo();
    this.floatStep = null;
    this.invalidate();
    this.trigger("selection");
  }

  /** Calls `fn` for each tile a layer rectangle touches. */
  eachTile(x: number, y: number, w: number, h: number, fn: (tx: number, ty: number) => void) {
    const T = 512;
    for (let ty = Math.floor(y / T); ty <= Math.floor((y + h - 1) / T); ty++) for (let tx = Math.floor(x / T); tx <= Math.floor((x + w - 1) / T); tx++) fn(tx, ty);
  }

  setSelection(sel: Selection | null) {
    this.selection = sel;
    this.drawOverlay();
    this.trigger("selection");
  }

  // ── colours ─────────────────────────────────────────────────────────────

  setColor(c: string, remember = true) {
    this.color = c;
    if (remember) {
      const r = this.board.recent.filter((x) => x !== c);
      r.unshift(c);
      this.board.recent = r.slice(0, 16);
    }
    this.trigger("color");
  }

  swapColors() {
    [this.color, this.secondary] = [this.secondary, this.color];
    this.trigger("color");
  }

  /** The colour on screen at a stage point (what you see, without guides). */
  sampleScreen(sx: number, sy: number): string | null {
    const ctx = this.canvas.getContext("2d")!;
    const d = ctx.getImageData(Math.round(sx * this.dpr), Math.round(sy * this.dpr), 1, 1).data;
    return "#" + [d[0], d[1], d[2]].map((v) => v.toString(16).padStart(2, "0")).join("");
  }

  // ── adding frames ───────────────────────────────────────────────────────

  async newFrame(after = true) {
    let created: Frame | null = null;
    this.mutate("New frame", (b) => {
      created = addPage(b, this.frameIndex + (after ? 1 : 0));
    });
    await this.setFrame(created!);
    this.trigger("board");
  }

  pictureSize(src: string): [number, number] | null {
    const img = this.comp.images.get(src);
    return img ? sourceSize(img) : null;
  }
}

/** Translates a Path2D (selection in canvas coordinates → layer coordinates). */
export function translated(p: Path2D, dx: number, dy: number): Path2D {
  const out = new Path2D();
  out.addPath(p, new DOMMatrix().translateSelf(dx, dy));
  return out;
}

