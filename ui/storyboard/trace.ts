// Tracing: a picture under (or over) the drawing, shown the way that helps
// the most for what is being drawn.
//
// - photo:  the picture itself;
// - grey:   without colour, so values read clearly;
// - lines:  its edges as thin dark lines (Sobel on a slightly blurred
//           luminance), the closest thing to a drawing to trace over;
// - values: two to six flat tones, for blocking in light and shadow;
// - blue:   a light "non-photo blue" print, the colour animators sketch in,
//           which stays out of the way of dark lines drawn over it.
//
// Results are cached per picture and settings; large pictures are worked on
// at most 2048 pixels wide and drawn back at full size.

import { TraceSettings } from "./model.ts";

const MAX = 2048;
type Source = HTMLImageElement | ImageBitmap | HTMLCanvasElement;
const cache = new WeakMap<object, Map<string, HTMLCanvasElement>>();

function sizeOf(img: Source): [number, number] {
  const w = (img as HTMLImageElement).naturalWidth || img.width;
  const h = (img as HTMLImageElement).naturalHeight || img.height;
  return [w, h];
}

export function traceKey(s: TraceSettings): string {
  return s.mode === "lines" ? `lines:${s.threshold.toFixed(2)}` : s.mode === "values" ? `values:${s.levels}` : s.mode;
}

/** The picture as the trace settings show it (same aspect, maybe smaller). */
export function traced(img: Source, s: TraceSettings): Source {
  if (s.mode === "photo") return img;
  const key = traceKey(s);
  let m = cache.get(img);
  if (!m) cache.set(img, (m = new Map()));
  const hit = m.get(key);
  if (hit) return hit;
  const [w0, h0] = sizeOf(img);
  const k = Math.min(1, MAX / Math.max(w0, h0));
  const w = Math.max(1, Math.round(w0 * k));
  const h = Math.max(1, Math.round(h0 * k));
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d", { willReadFrequently: true })!;
  ctx.drawImage(img, 0, 0, w, h);
  const data = ctx.getImageData(0, 0, w, h);
  const px = data.data;
  const lum = new Float32Array(w * h);
  for (let i = 0, j = 0; j < lum.length; i += 4, j++) {
    // Transparent parts count as white paper.
    const a = px[i + 3] / 255;
    lum[j] = ((0.2126 * px[i] + 0.7152 * px[i + 1] + 0.0722 * px[i + 2]) / 255) * a + (1 - a);
  }
  if (s.mode === "grey") {
    for (let j = 0, i = 0; j < lum.length; j++, i += 4) {
      const v = lum[j] * 255;
      px[i] = px[i + 1] = px[i + 2] = v;
      px[i + 3] = 255;
    }
  } else if (s.mode === "values") {
    const n = Math.max(2, Math.min(6, Math.round(s.levels || 3)));
    for (let j = 0, i = 0; j < lum.length; j++, i += 4) {
      const step = Math.min(n - 1, Math.floor(lum[j] * n));
      const v = (step / (n - 1)) * 225 + 20;
      px[i] = px[i + 1] = px[i + 2] = v;
      px[i + 3] = 255;
    }
  } else if (s.mode === "blue") {
    // Dark becomes a light cyan-blue; white becomes transparent.
    for (let j = 0, i = 0; j < lum.length; j++, i += 4) {
      const d = 1 - lum[j];
      px[i] = 90;
      px[i + 1] = 169;
      px[i + 2] = 230;
      px[i + 3] = Math.min(255, d * 1.15 * 255);
    }
  } else if (s.mode === "lines") {
    const blur = boxBlur(lum, w, h);
    const mag = new Float32Array(w * h);
    let max = 0;
    for (let y = 1; y < h - 1; y++) {
      for (let x = 1; x < w - 1; x++) {
        const i = y * w + x;
        const gx = -blur[i - w - 1] - 2 * blur[i - 1] - blur[i + w - 1] + blur[i - w + 1] + 2 * blur[i + 1] + blur[i + w + 1];
        const gy = -blur[i - w - 1] - 2 * blur[i - w] - blur[i - w + 1] + blur[i + w - 1] + 2 * blur[i + w] + blur[i + w + 1];
        const m = Math.hypot(gx, gy);
        mag[i] = m;
        if (m > max) max = m;
      }
    }
    // Sensitivity: 0 keeps only the strongest edges, 1 nearly every change.
    const t = Math.max(0.02, Math.min(0.98, s.threshold ?? 0.5));
    const lo = max * (0.42 - t * 0.38);
    const hi = lo * 2.2 + 1e-6;
    for (let j = 0, i = 0; j < mag.length; j++, i += 4) {
      const a = Math.max(0, Math.min(1, (mag[j] - lo) / (hi - lo)));
      px[i] = 35;
      px[i + 1] = 34;
      px[i + 2] = 31;
      px[i + 3] = a * 255;
    }
  }
  ctx.putImageData(data, 0, 0);
  m.set(key, c);
  return c;
}

function boxBlur(src: Float32Array, w: number, h: number): Float32Array {
  const tmp = new Float32Array(src.length);
  const out = new Float32Array(src.length);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const l = x > 0 ? src[i - 1] : src[i];
      const r = x < w - 1 ? src[i + 1] : src[i];
      tmp[i] = (l + src[i] + r) / 3;
    }
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const u = y > 0 ? tmp[i - w] : tmp[i];
      const d = y < h - 1 ? tmp[i + w] : tmp[i];
      out[i] = (u + tmp[i] + d) / 3;
    }
  }
  return out;
}

export const TRACE_MODES: [string, string, string][] = [
  ["photo", "Photo", "The picture as it is"],
  ["grey", "Greyscale", "Without colour, so the values read clearly"],
  ["lines", "Lines", "Its edges as thin lines, to trace over"],
  ["values", "Values", "A few flat tones, for light and shadow"],
  ["blue", "Blue pencil", "A light non-photo blue, out of the way of dark lines"],
];

export function defaultTrace(): TraceSettings {
  return { mode: "photo", above: false, export: false, threshold: 0.5, levels: 3 };
}
