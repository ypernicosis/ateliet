// Brushes. A stroke is a run of dabs placed along the smoothed pointer path,
// spaced by a fraction of their size and shaped by pressure.
//
// - pencil: Pictogrep's graphite: a small chisel that builds up where lines
//           cross, drawn straight onto the layer;
// - pen:    an even ink line; width follows pressure;
// - brush:  soft and translucent, for tone; opacity follows pressure;
// - marker: flat, translucent, does not build up within one stroke;
// - eraser: hard or soft.
//
// Pen, brush, marker and eraser draw into a separate stroke buffer and are
// put onto the layer when the stroke ends, so a stroke never darkens where
// it overlaps itself and the stroke opacity is exact.

import { TILE } from "./model.ts";
import { newTile } from "./store.ts";

export type BrushKind = "pencil" | "pen" | "brush" | "marker" | "eraser";

export interface BrushSettings {
  size: number;
  /** 0..1 */
  opacity: number;
  /** 0 (raw) .. 0.95 (very steady) */
  smoothing: number;
  /** 0 (soft) .. 1 (hard) */
  hardness: number;
  pressureSize: boolean;
  pressureOpacity: boolean;
}

export const BRUSH_DEFAULTS: Record<BrushKind, BrushSettings> = {
  pencil: { size: 4, opacity: 1, smoothing: 0.36, hardness: 1, pressureSize: true, pressureOpacity: true },
  pen: { size: 5, opacity: 1, smoothing: 0.5, hardness: 1, pressureSize: true, pressureOpacity: false },
  brush: { size: 40, opacity: 0.6, smoothing: 0.3, hardness: 0.15, pressureSize: false, pressureOpacity: true },
  marker: { size: 24, opacity: 0.5, smoothing: 0.3, hardness: 0.85, pressureSize: false, pressureOpacity: false },
  eraser: { size: 24, opacity: 1, smoothing: 0.2, hardness: 0.9, pressureSize: false, pressureOpacity: false },
};

export const BRUSH_NAMES: Record<BrushKind, string> = { pencil: "Pencil", pen: "Pen", brush: "Brush", marker: "Marker", eraser: "Eraser" };

export interface Point {
  x: number;
  y: number;
  p: number;
}

/** Where dabs go: a function that paints a rectangle in layer coordinates. */
export type Painter = (x: number, y: number, w: number, h: number, draw: (ctx: CanvasRenderingContext2D) => void) => void;

/** Tiles of the stroke being drawn (layer coordinates, same grid as layers). */
export class StrokeBuffer {
  tiles = new Map<string, HTMLCanvasElement>();
  paint: Painter = (x, y, w, h, draw) => {
    const x0 = Math.floor(x / TILE), y0 = Math.floor(y / TILE);
    const x1 = Math.floor((x + w - 1) / TILE), y1 = Math.floor((y + h - 1) / TILE);
    for (let ty = y0; ty <= y1; ty++) {
      for (let tx = x0; tx <= x1; tx++) {
        const k = `${tx}_${ty}`;
        let c = this.tiles.get(k);
        if (!c) this.tiles.set(k, (c = newTile()));
        const ctx = c.getContext("2d")!;
        ctx.save();
        ctx.translate(-tx * TILE, -ty * TILE);
        draw(ctx);
        ctx.restore();
      }
    }
  };
}

function rgba(hex: string, a: number): string {
  const n = parseInt(hex.slice(1, 7), 16);
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
}

export class Stroke {
  private last: Point | null = null;
  private smoothed: Point | null = null;
  /** Distance left over from the previous segment, so spacing is even. */
  private carry = 0;
  bounds = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity };
  dabs = 0;

  constructor(
    public kind: BrushKind,
    public s: BrushSettings,
    public color: string,
    private paint: Painter,
    /** Clips every dab (a selection), in layer coordinates. */
    private clip: Path2D | null = null,
  ) {}

  /** Pressure from a pointer event: a mouse counts as a firm, even press. */
  static pressure(e: PointerEvent): number {
    if (e.pointerType === "mouse" || !e.pressure) return 1;
    return Math.max(0.05, Math.min(1, e.pressure));
  }

  add(pt: Point) {
    const k = this.s.smoothing;
    if (!this.smoothed) this.smoothed = { ...pt };
    else {
      const a = 1 - Math.min(0.95, k);
      this.smoothed = {
        x: this.smoothed.x + (pt.x - this.smoothed.x) * a,
        y: this.smoothed.y + (pt.y - this.smoothed.y) * a,
        p: this.smoothed.p + (pt.p - this.smoothed.p) * Math.max(a, 0.35),
      };
    }
    this.segment(this.smoothed);
  }

  /** Catches up with the pointer, so the line ends where the pen lifted. */
  end(pt?: Point) {
    if (pt && this.smoothed) {
      const steps = 6;
      const from = { ...this.smoothed };
      for (let i = 1; i <= steps; i++) {
        const t = i / steps;
        this.segment({ x: from.x + (pt.x - from.x) * t, y: from.y + (pt.y - from.y) * t, p: from.p + (pt.p - from.p) * t });
      }
    }
  }

  /** A straight line from a to b (shift-click). */
  line(a: Point, b: Point) {
    this.last = null;
    this.carry = 0;
    this.segment(a);
    this.segment(b);
  }

  private radius(p: number): number {
    const r = this.s.size / 2;
    return Math.max(0.5, this.s.pressureSize ? r * (0.2 + 0.8 * p) : r);
  }

  private spacing(r: number): number {
    if (this.kind === "pencil") return Math.max(0.45, r * 0.42);
    if (this.kind === "brush") return Math.max(0.5, r * 0.18);
    return Math.max(0.4, r * 0.12);
  }

  private segment(pt: Point) {
    if (!this.last) {
      this.dab(pt);
      this.last = pt;
      return;
    }
    const a = this.last;
    const dx = pt.x - a.x, dy = pt.y - a.y;
    const dist = Math.hypot(dx, dy);
    if (dist < 1e-3) return;
    let t = this.carry;
    while (true) {
      const p = a.p + (pt.p - a.p) * Math.min(1, t / dist);
      const step = this.spacing(this.radius(p));
      t += step;
      if (t > dist) {
        this.carry = t - step - dist;
        break;
      }
      const u = t / dist;
      this.dab({ x: a.x + dx * u, y: a.y + dy * u, p: a.p + (pt.p - a.p) * u });
    }
    if (this.carry < 0) this.carry = 0;
    this.last = pt;
  }

  private dab(pt: Point) {
    const r = this.radius(pt.p);
    const pad = r * (this.kind === "pencil" ? 1.6 : 1.2) + 2;
    const x = Math.floor(pt.x - pad), y = Math.floor(pt.y - pad);
    const w = Math.ceil(pad * 2) + 1;
    this.bounds.x0 = Math.min(this.bounds.x0, x);
    this.bounds.y0 = Math.min(this.bounds.y0, y);
    this.bounds.x1 = Math.max(this.bounds.x1, x + w);
    this.bounds.y1 = Math.max(this.bounds.y1, y + w);
    this.dabs++;
    this.paint(x, y, w, w, (ctx) => {
      if (this.clip) ctx.clip(this.clip);
      this.shape(ctx, pt, r);
    });
  }

  private shape(ctx: CanvasRenderingContext2D, pt: Point, r: number) {
    const color = this.kind === "eraser" ? "#000000" : this.color;
    const pa = this.s.pressureOpacity ? 0.25 + 0.75 * pt.p : 1;
    switch (this.kind) {
      case "pencil": {
        // Pictogrep's graphite chisel, in any colour.
        const p = Math.max(0.05, Math.min(1, 0.5 + (pt.p - 0.5) * 1.2));
        const width = Math.max(0.7, this.s.size * 0.62 * (0.45 + p * 0.9));
        const height = width * 2.15;
        ctx.fillStyle = rgba(color, (0.38 + p * 0.32) * this.s.opacity);
        ctx.beginPath();
        ctx.roundRect(pt.x - width / 2, pt.y - height / 2, width, height, Math.min(width / 2, 1.5));
        ctx.fill();
        return;
      }
      case "pen":
      case "marker":
      case "eraser":
      case "brush": {
        const hard = this.s.hardness;
        const flow = this.kind === "brush" ? 0.22 * pa : this.kind === "marker" ? 1 : pa;
        if (hard >= 0.98) {
          ctx.fillStyle = rgba(color, flow);
          ctx.beginPath();
          ctx.arc(pt.x, pt.y, r, 0, Math.PI * 2);
          ctx.fill();
          return;
        }
        const g = ctx.createRadialGradient(pt.x, pt.y, 0, pt.x, pt.y, r);
        g.addColorStop(0, rgba(color, flow));
        g.addColorStop(Math.max(0.01, Math.min(0.99, hard)), rgba(color, flow));
        g.addColorStop(1, rgba(color, 0));
        ctx.fillStyle = g;
        ctx.beginPath();
        ctx.arc(pt.x, pt.y, r, 0, Math.PI * 2);
        ctx.fill();
        return;
      }
    }
  }

  /** How the finished stroke goes onto the layer. */
  get composite(): { op: GlobalCompositeOperation; alpha: number } {
    if (this.kind === "eraser") return { op: "destination-out", alpha: this.s.opacity };
    if (this.kind === "pencil") return { op: "source-over", alpha: 1 };
    return { op: "source-over", alpha: this.s.opacity };
  }

  /** Pencil draws straight onto the layer; the others use a stroke buffer. */
  static direct(kind: BrushKind) {
    return kind === "pencil";
  }
}

/** The outline a brush leaves, for the cursor. */
export function cursorRadius(kind: BrushKind, s: BrushSettings): number {
  if (kind === "pencil") return Math.max(1, s.size * 0.62);
  return Math.max(0.5, s.size / 2);
}
