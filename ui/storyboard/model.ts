// The storyboard document: frames on a timeline, each with a stack of layers.
//
// A `.storyboard` file is a zip holding `board.json` (this model), the pixels
// of every raster layer as 512×512 PNG tiles (`tiles/<layer>/<x>_<y>.png`),
// pictures copied into the file (`refs/…`), a small picture of each frame
// (`thumbs/<frame>.png`) and `preview.png` (the first frame) for lists and
// embeds. Tiles make an infinite canvas the same thing as a bounded one: only
// the parts that were drawn on exist.

export const TILE = 512;
export const FORMAT = 1;

export interface Transform {
  /** Centre of the picture on the canvas. */
  x: number;
  y: number;
  /** Scale; negative mirrors. */
  sx: number;
  sy: number;
  /** Radians. */
  rotation: number;
}

export type LayerKind = "raster" | "image" | "trace";
export type TraceMode = "photo" | "grey" | "lines" | "values" | "blue";

export interface TraceSettings {
  mode: TraceMode;
  /** Shown over the drawing (a light table) instead of under it. */
  above: boolean;
  /** Included when frames are exported (off: it is only a guide). */
  export: boolean;
  /** Line detection sensitivity, 0..1. */
  threshold: number;
  /** Number of values for "values" mode. */
  levels: number;
}

export interface Layer {
  id: string;
  name: string;
  kind: LayerKind;
  visible: boolean;
  /** 0..1 */
  opacity: number;
  /** Canvas composite operation. */
  blend: string;
  locked: boolean;
  /** Draws only where the layer below has pixels. */
  clip: boolean;
  /** Painting keeps the transparent parts transparent. */
  alphaLock?: boolean;
  /** Offset of a raster layer's tiles (moving the layer moves this). */
  x: number;
  y: number;
  /** In every frame (a background, a model sheet). */
  shared?: boolean;
  /** Image and trace layers: which reference, and where. */
  src?: string;
  transform?: Transform;
  trace?: TraceSettings;
}

export interface FrameNotes {
  action: string;
  dialogue: string;
  camera: string;
  shot: string;
  sound: string;
}

export interface Frame {
  id: string;
  name: string;
  /** Length on the timeline, in frames at the board's fps. */
  duration: number;
  /** Layer ids, bottom to top. */
  layers: string[];
  active: string;
  notes: FrameNotes;
  /** The reference shown beside this frame. */
  reference?: string;
  /** A colour label on the timeline. */
  color?: string;
}

export interface Reference {
  id: string;
  name: string;
  /** Absolute path, or `refs/<file>` for a picture kept inside the storyboard. */
  path: string;
  /** Where it came from: a collection, a folder, a note, a canvas. */
  from?: string;
}

export interface Guides {
  thirds: boolean;
  center: boolean;
  /** Title and action safe areas. */
  safe: boolean;
  /** Squares per side of the grid; 0 is off. The same grid shows over the reference. */
  grid: number;
  /** Mirror the view horizontally (a check, not a change). */
  mirror: boolean;
}

export interface Onion {
  enabled: boolean;
  before: number;
  after: number;
  opacity: number;
  /** Earlier frames reddish, later ones greenish. */
  tint: boolean;
  colorBefore: string;
  colorAfter: string;
  /** Farther frames fainter. */
  fade: boolean;
  /** Picture layers too (not only drawing). */
  pictures: boolean;
}

export interface Board {
  format: number;
  width: number;
  height: number;
  infinite: boolean;
  /** Paper colour, or "transparent". */
  background: string;
  fps: number;
  frames: Frame[];
  layers: Record<string, Layer>;
  references: Reference[];
  palette: string[];
  recent: string[];
  guides: Guides;
  onion: Onion;
  created: number;
  modified: number;
}

export interface SizePreset {
  id: string;
  name: string;
  width: number;
  height: number;
}

export const SIZE_PRESETS: SizePreset[] = [
  { id: "hd", name: "16:9 · 1920 × 1080 (HD)", width: 1920, height: 1080 },
  { id: "hd720", name: "16:9 · 1280 × 720", width: 1280, height: 720 },
  { id: "4k", name: "16:9 · 3840 × 2160 (4K)", width: 3840, height: 2160 },
  { id: "scope", name: "2.39:1 · 2048 × 858 (scope)", width: 2048, height: 858 },
  { id: "flat", name: "1.85:1 · 1998 × 1080 (flat)", width: 1998, height: 1080 },
  { id: "univisium", name: "2:1 · 2000 × 1000", width: 2000, height: 1000 },
  { id: "43", name: "4:3 · 1440 × 1080", width: 1440, height: 1080 },
  { id: "34", name: "3:4 · 1080 × 1440", width: 1080, height: 1440 },
  { id: "vertical", name: "9:16 · 1080 × 1920 (vertical)", width: 1080, height: 1920 },
  { id: "square", name: "1:1 · 1080 × 1080", width: 1080, height: 1080 },
  { id: "a4", name: "A4 landscape · 1754 × 1240 (150 dpi)", width: 1754, height: 1240 },
  { id: "a4p", name: "A4 portrait · 1240 × 1754 (150 dpi)", width: 1240, height: 1754 },
];

export const PAPER = "#e0dedb";

export const BLEND_MODES: [string, string][] = [
  ["source-over", "Normal"],
  ["multiply", "Multiply"],
  ["darken", "Darken"],
  ["color-burn", "Color burn"],
  ["screen", "Screen"],
  ["lighten", "Lighten"],
  ["color-dodge", "Color dodge"],
  ["lighter", "Add"],
  ["overlay", "Overlay"],
  ["soft-light", "Soft light"],
  ["hard-light", "Hard light"],
  ["difference", "Difference"],
  ["exclusion", "Exclusion"],
  ["hue", "Hue"],
  ["saturation", "Saturation"],
  ["color", "Color"],
  ["luminosity", "Luminosity"],
];

export const DEFAULT_PALETTE = ["#23221f", "#5b5853", "#9a968f", "#ffffff", "#c0392b", "#e67e22", "#f1c40f", "#27ae60", "#2980b9", "#8e44ad", "#6fa8dc", "#f4b6c2"];

export function uid(): string {
  const a = new Uint8Array(6);
  crypto.getRandomValues(a);
  return Array.from(a, (b) => b.toString(36).padStart(2, "0")).join("").slice(0, 10);
}

export function emptyNotes(): FrameNotes {
  return { action: "", dialogue: "", camera: "", shot: "", sound: "" };
}

export function newLayer(name: string, kind: LayerKind = "raster", extra: Partial<Layer> = {}): Layer {
  return { id: uid(), name, kind, visible: true, opacity: 1, blend: "source-over", locked: false, clip: false, x: 0, y: 0, ...extra };
}

export interface NewBoardOptions {
  width: number;
  height: number;
  infinite: boolean;
  background: string;
  fps: number;
}

export function newBoard(o: NewBoardOptions): Board {
  const now = Date.now();
  const board: Board = {
    format: FORMAT,
    width: o.width,
    height: o.height,
    infinite: o.infinite,
    background: o.background,
    fps: o.fps,
    frames: [],
    layers: {},
    references: [],
    palette: [...DEFAULT_PALETTE],
    recent: [],
    guides: { thirds: false, center: false, safe: false, grid: 0, mirror: false },
    onion: { enabled: false, before: 1, after: 1, opacity: 0.3, tint: true, colorBefore: "#d0453b", colorAfter: "#2f9e5a", fade: true, pictures: false },
    created: now,
    modified: now,
  };
  addFrame(board, 0);
  return board;
}

/** A new frame at `index`, with the shared layers and one drawing layer. */
export function addFrame(board: Board, index: number, duration?: number): Frame {
  const drawing = newLayer("Drawing");
  board.layers[drawing.id] = drawing;
  const shared = sharedLayers(board);
  const frame: Frame = {
    id: uid(),
    name: "",
    duration: duration ?? (board.frames[index - 1]?.duration || Math.max(1, Math.round(board.fps * 2))),
    layers: [...shared.map((l) => l.id), drawing.id],
    active: drawing.id,
    notes: emptyNotes(),
  };
  board.frames.splice(index, 0, frame);
  return frame;
}

/** Shared layers in the order of the first frame that has them. */
export function sharedLayers(board: Board): Layer[] {
  const seen = new Set<string>();
  const out: Layer[] = [];
  for (const f of board.frames) {
    for (const id of f.layers) {
      const l = board.layers[id];
      if (l?.shared && !seen.has(id)) {
        seen.add(id);
        out.push(l);
      }
    }
  }
  return out;
}

export function frameLabel(board: Board, frame: Frame): string {
  return frame.name || `Frame ${board.frames.indexOf(frame) + 1}`;
}

/** Where each frame starts on the timeline, in frames. */
export function frameStarts(board: Board): number[] {
  const out: number[] = [];
  let t = 0;
  for (const f of board.frames) {
    out.push(t);
    t += f.duration;
  }
  return out;
}

export function totalFrames(board: Board): number {
  return board.frames.reduce((n, f) => n + f.duration, 0);
}

export function timecode(frames: number, fps: number): string {
  const s = Math.floor(frames / fps);
  const f = Math.round(frames - s * fps);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}:${String(f).padStart(2, "0")}`;
}

/** Layers that no frame uses any more (after deletions). */
export function unusedLayers(board: Board): string[] {
  const used = new Set(board.frames.flatMap((f) => f.layers));
  return Object.keys(board.layers).filter((id) => !used.has(id));
}

/** Fills in what an older or hand-edited board.json lacks. */
export function normalizeBoard(raw: any): Board {
  const base = newBoard({ width: raw?.width || 1920, height: raw?.height || 1080, infinite: !!raw?.infinite, background: raw?.background ?? PAPER, fps: raw?.fps || 24 });
  const board: Board = { ...base, ...raw, guides: { ...base.guides, ...(raw?.guides || {}) }, onion: { ...base.onion, ...(raw?.onion || {}) } };
  if (!Array.isArray(board.frames) || !board.frames.length) {
    board.frames = [];
    board.layers = board.layers || {};
    addFrame(board, 0);
  }
  for (const f of board.frames) {
    f.notes = { ...emptyNotes(), ...(f.notes || {}) };
    f.layers = (f.layers || []).filter((id) => board.layers[id]);
    if (!f.layers.length) {
      const l = newLayer("Drawing");
      board.layers[l.id] = l;
      f.layers.push(l.id);
    }
    if (!f.layers.includes(f.active)) f.active = f.layers[f.layers.length - 1];
    f.duration = Math.max(1, Math.round(f.duration || board.fps));
  }
  for (const l of Object.values(board.layers)) {
    l.x ||= 0;
    l.y ||= 0;
    l.opacity ??= 1;
    l.blend ||= "source-over";
  }
  board.references ||= [];
  board.palette ||= [...DEFAULT_PALETTE];
  board.recent ||= [];
  return board;
}

/** Reduced aspect ratio ("16:9"), or the plain numbers when they do not reduce nicely. */
export function aspectOf(w: number, h: number): string {
  const g = (a: number, b: number): number => (b ? g(b, a % b) : a);
  const d = g(w, h);
  const a = w / d;
  const b = h / d;
  if (a <= 32 && b <= 32) return `${a}:${b}`;
  const r = w / h;
  for (const [name, v] of [["16:9", 16 / 9], ["4:3", 4 / 3], ["2:1", 2], ["3:4", 3 / 4], ["1:1", 1], ["9:16", 9 / 16]] as [string, number][]) {
    if (Math.abs(r - v) < 0.01) return name;
  }
  return `${w}:${h}`;
}
