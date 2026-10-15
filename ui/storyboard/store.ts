// Loading and saving a storyboard, and the pixels of its raster layers.
//
// Pixels live in 512×512 tiles. A tile is either decoded (a canvas, while its
// frame is on screen or recently was) or only encoded (the PNG bytes from the
// file). Drawing marks tiles dirty; saving encodes the dirty ones and writes
// the zip again from the cached bytes, so an unchanged tile costs nothing.

import { Board, FORMAT, Frame, Layer, Reference, TILE, normalizeBoard, unusedLayers } from "./model.ts";
import { ZipEntry, readZip, writeZip } from "./zip.ts";
import { fileUrl, readBytes, writeBytes } from "../tauri.ts";

export type TileKey = string; // `${layer}/${tx}_${ty}`

export function tileKey(layer: string, tx: number, ty: number): TileKey {
  return `${layer}/${tx}_${ty}`;
}

export function parseKey(key: TileKey): { layer: string; tx: number; ty: number } {
  const slash = key.indexOf("/");
  const [tx, ty] = key.slice(slash + 1).split("_").map(Number);
  return { layer: key.slice(0, slash), tx, ty };
}

export function newTile(): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = TILE;
  c.height = TILE;
  return c;
}

export function context(c: HTMLCanvasElement): CanvasRenderingContext2D {
  return c.getContext("2d", { willReadFrequently: false })!;
}

async function canvasToPng(c: HTMLCanvasElement): Promise<Uint8Array> {
  const blob: Blob | null = await new Promise((res) => c.toBlob(res, "image/png"));
  if (!blob) throw new Error("could not encode a tile");
  return new Uint8Array(await blob.arrayBuffer());
}

export async function decodeImage(bytes: Uint8Array, type = "image/png"): Promise<ImageBitmap> {
  return createImageBitmap(new Blob([bytes as BlobPart], { type }));
}

function isEmpty(c: HTMLCanvasElement): boolean {
  const d = context(c).getImageData(0, 0, c.width, c.height).data;
  for (let i = 3; i < d.length; i += 4) if (d[i]) return false;
  return true;
}

export class TileStore {
  /** PNG bytes of every tile that is not dirty. */
  private encoded = new Map<TileKey, ZipEntry>();
  private live = new Map<TileKey, HTMLCanvasElement>();
  private dirty = new Set<TileKey>();
  /** Tiles of each layer ("tx_ty"). */
  private index = new Map<string, Set<string>>();
  /** Most recently used last. */
  private lru: TileKey[] = [];
  private pinned = new Set<string>();
  private loading = new Map<TileKey, Promise<void>>();
  /** Decoded tiles kept (about 1 MB each); tiles on screen or not yet saved stay regardless. */
  limit = 400;

  addEncoded(key: TileKey, data: Uint8Array) {
    this.encoded.set(key, { name: `tiles/${key}.png`, data });
    this.indexAdd(key);
  }

  private indexAdd(key: TileKey) {
    const { layer, tx, ty } = parseKey(key);
    let s = this.index.get(layer);
    if (!s) this.index.set(layer, (s = new Set()));
    s.add(`${tx}_${ty}`);
  }

  private indexRemove(key: TileKey) {
    const { layer, tx, ty } = parseKey(key);
    this.index.get(layer)?.delete(`${tx}_${ty}`);
  }

  /** [tx, ty] of every tile of a layer. */
  tilesOf(layer: string): [number, number][] {
    return [...(this.index.get(layer) || [])].map((s) => s.split("_").map(Number) as [number, number]);
  }

  has(layer: string, tx: number, ty: number) {
    return !!this.index.get(layer)?.has(`${tx}_${ty}`);
  }

  /** The decoded tile, if it is loaded. */
  get(layer: string, tx: number, ty: number): HTMLCanvasElement | undefined {
    const key = tileKey(layer, tx, ty);
    const c = this.live.get(key);
    if (c) this.touch(key);
    return c;
  }

  /** A tile to draw on: decoded, or created empty. Must be loaded first. */
  ensure(layer: string, tx: number, ty: number): HTMLCanvasElement {
    const key = tileKey(layer, tx, ty);
    let c = this.live.get(key);
    if (!c) {
      c = newTile();
      this.live.set(key, c);
      this.indexAdd(key);
    }
    this.touch(key);
    return c;
  }

  markDirty(layer: string, tx: number, ty: number) {
    const key = tileKey(layer, tx, ty);
    this.dirty.add(key);
    this.encoded.delete(key);
  }

  get dirtyCount() {
    return this.dirty.size;
  }

  private touch(key: TileKey) {
    const i = this.lru.lastIndexOf(key);
    if (i !== this.lru.length - 1) {
      if (i >= 0) this.lru.splice(i, 1);
      this.lru.push(key);
    }
  }

  /** Whether every tile of a layer is decoded. */
  isLoaded(layer: string): boolean {
    for (const k of this.index.get(layer) || []) if (!this.live.has(`${layer}/${k}`) && this.encoded.has(`${layer}/${k}`)) return false;
    return true;
  }

  /** Decodes every tile of these layers. */
  async load(layers: string[]) {
    const jobs: Promise<void>[] = [];
    for (const layer of layers) {
      for (const [tx, ty] of this.tilesOf(layer)) {
        const key = tileKey(layer, tx, ty);
        if (this.live.has(key)) continue;
        let job = this.loading.get(key);
        if (!job) {
          const entry = this.encoded.get(key);
          if (!entry) continue;
          job = decodeImage(entry.data)
            .then((bmp) => {
              if (this.live.has(key)) return;
              const c = newTile();
              context(c).drawImage(bmp, 0, 0);
              bmp.close?.();
              this.live.set(key, c);
              this.touch(key);
            })
            .catch((e) => console.warn("storyboard tile", key, e))
            .finally(() => this.loading.delete(key));
          this.loading.set(key, job);
        }
        jobs.push(job);
      }
    }
    await Promise.all(jobs);
  }

  /** Layers whose tiles must stay decoded (the frames on screen). */
  pin(layers: string[]) {
    this.pinned = new Set(layers);
    this.evict();
  }

  private evict() {
    if (this.live.size <= this.limit) return;
    for (let i = 0; i < this.lru.length && this.live.size > this.limit; ) {
      const key = this.lru[i];
      const { layer } = parseKey(key);
      if (this.pinned.has(layer) || this.dirty.has(key) || !this.encoded.has(key)) {
        i++;
        continue;
      }
      this.live.delete(key);
      this.lru.splice(i, 1);
    }
  }

  /** A copy of a tile (for undo); null when the tile does not exist. */
  snapshot(layer: string, tx: number, ty: number): HTMLCanvasElement | null {
    const c = this.live.get(tileKey(layer, tx, ty));
    if (!c) return null;
    const copy = newTile();
    context(copy).drawImage(c, 0, 0);
    return copy;
  }

  /** Puts a tile back from a snapshot (null removes it). */
  restore(layer: string, tx: number, ty: number, snap: HTMLCanvasElement | null) {
    const key = tileKey(layer, tx, ty);
    if (!snap) {
      this.live.delete(key);
      this.encoded.delete(key);
      this.dirty.delete(key);
      this.indexRemove(key);
      return;
    }
    const c = this.ensure(layer, tx, ty);
    const ctx = context(c);
    ctx.clearRect(0, 0, TILE, TILE);
    ctx.drawImage(snap, 0, 0);
    this.markDirty(layer, tx, ty);
  }

  /** Copies every tile of a layer to another (duplicating a layer). */
  async copyLayer(from: string, to: string, dx = 0, dy = 0) {
    await this.load([from]);
    for (const [tx, ty] of this.tilesOf(from)) {
      const src = this.live.get(tileKey(from, tx, ty));
      if (!src) continue;
      if (dx % TILE === 0 && dy % TILE === 0) {
        const c = this.ensure(to, tx + dx / TILE, ty + dy / TILE);
        context(c).drawImage(src, 0, 0);
        this.markDirty(to, tx + dx / TILE, ty + dy / TILE);
      } else {
        this.paint(to, (tx * TILE + dx), (ty * TILE + dy), TILE, TILE, (ctx) => ctx.drawImage(src, 0, 0));
      }
    }
  }

  deleteLayer(layer: string) {
    for (const [tx, ty] of this.tilesOf(layer)) {
      const key = tileKey(layer, tx, ty);
      this.live.delete(key);
      this.encoded.delete(key);
      this.dirty.delete(key);
    }
    this.index.delete(layer);
    this.lru = this.lru.filter((k) => !k.startsWith(layer + "/"));
  }

  /**
   * Draws into a layer over the rectangle (x, y, w, h) in layer coordinates:
   * `draw` gets a context translated so that (x, y) is the origin, once per
   * tile the rectangle touches. Tiles must be loaded. Returns the tiles drawn.
   */
  paint(layer: string, x: number, y: number, w: number, h: number, draw: (ctx: CanvasRenderingContext2D) => void, bounds?: { w: number; h: number } | null): [number, number][] {
    let x0 = Math.floor(x / TILE);
    let y0 = Math.floor(y / TILE);
    let x1 = Math.floor((x + w - 1) / TILE);
    let y1 = Math.floor((y + h - 1) / TILE);
    if (bounds) {
      x0 = Math.max(x0, 0);
      y0 = Math.max(y0, 0);
      x1 = Math.min(x1, Math.ceil(bounds.w / TILE) - 1);
      y1 = Math.min(y1, Math.ceil(bounds.h / TILE) - 1);
    }
    const out: [number, number][] = [];
    for (let ty = y0; ty <= y1; ty++) {
      for (let tx = x0; tx <= x1; tx++) {
        const ctx = context(this.ensure(layer, tx, ty));
        ctx.save();
        ctx.translate(x - tx * TILE, y - ty * TILE);
        draw(ctx);
        ctx.restore();
        this.markDirty(layer, tx, ty);
        out.push([tx, ty]);
      }
    }
    return out;
  }

  /** Bounding box of a layer's tiles, in layer coordinates. */
  bounds(layer: string): { x: number; y: number; w: number; h: number } | null {
    const t = this.tilesOf(layer);
    if (!t.length) return null;
    const xs = t.map((p) => p[0]);
    const ys = t.map((p) => p[1]);
    const x0 = Math.min(...xs) * TILE;
    const y0 = Math.min(...ys) * TILE;
    return { x: x0, y: y0, w: (Math.max(...xs) + 1) * TILE - x0, h: (Math.max(...ys) + 1) * TILE - y0 };
  }

  /** Encodes dirty tiles (dropping the ones that became empty). */
  async flush() {
    const keys = [...this.dirty];
    for (const key of keys) {
      const c = this.live.get(key);
      this.dirty.delete(key);
      if (!c) continue;
      if (isEmpty(c)) {
        this.live.delete(key);
        this.encoded.delete(key);
        this.indexRemove(key);
        continue;
      }
      try {
        this.encoded.set(key, { name: `tiles/${key}.png`, data: await canvasToPng(c) });
      } catch (e) {
        this.dirty.add(key);
        throw e;
      }
    }
    this.evict();
  }

  /** Encoded entries for the layers still in use. */
  entries(keep: Set<string>): ZipEntry[] {
    const out: ZipEntry[] = [];
    for (const [key, e] of this.encoded) if (keep.has(parseKey(key).layer)) out.push(e);
    return out;
  }
}

/** Pictures inside the storyboard (refs/…) and decoded references. */
export class RefStore {
  embedded = new Map<string, Uint8Array>();
  private images = new Map<string, Promise<HTMLImageElement | null>>();

  constructor(private doc: StoryDoc) {}

  url(ref: Reference): string {
    if (ref.path.startsWith("refs/")) {
      const bytes = this.embedded.get(ref.path);
      if (!bytes) return "";
      return URL.createObjectURL(new Blob([bytes as BlobPart], { type: mimeOf(ref.path) }));
    }
    return fileUrl(ref.path);
  }

  /** The decoded picture (cached), or null when it cannot be found. */
  image(ref: Reference | undefined): Promise<HTMLImageElement | null> {
    if (!ref) return Promise.resolve(null);
    let p = this.images.get(ref.id);
    if (!p) {
      p = new Promise((resolve) => {
        const img = new Image();
        // Served with CORS headers: without this the canvas would be tainted
        // (no thumbnails, export, colour picking or trace processing).
        img.crossOrigin = "anonymous";
        img.decoding = "async";
        img.onload = () => resolve(img);
        img.onerror = () => resolve(null);
        img.src = this.url(ref);
      });
      this.images.set(ref.id, p);
    }
    return p;
  }

  forget(id: string) {
    this.images.delete(id);
  }

  ref(id: string | undefined): Reference | undefined {
    return id ? this.doc.board.references.find((r) => r.id === id) : undefined;
  }
}

export function mimeOf(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase();
  return ext === "jpg" || ext === "jpeg" ? "image/jpeg" : ext === "webp" ? "image/webp" : ext === "gif" ? "image/gif" : "image/png";
}

/** An open storyboard. */
export class StoryDoc {
  board!: Board;
  tiles = new TileStore();
  refs = new RefStore(this);
  thumbs = new Map<string, Uint8Array>();
  preview: Uint8Array | null = null;
  private saving: Promise<void> | null = null;
  private again = false;
  changed = false;
  /** Layers the undo history still needs, kept even when no frame uses them. */
  retain: () => Set<string> = () => new Set();

  constructor(public abs: string) {}

  static async open(abs: string): Promise<StoryDoc> {
    const doc = new StoryDoc(abs);
    const files = await readZip(await readBytes(abs));
    const json = files.get("board.json");
    if (!json) throw new Error("not a storyboard file (no board.json)");
    const raw = JSON.parse(new TextDecoder().decode(json));
    if (raw.format > FORMAT) throw new Error("This storyboard was made by a newer version of Ateliet.");
    doc.board = normalizeBoard(raw);
    for (const [name, data] of files) {
      if (name.startsWith("tiles/") && name.endsWith(".png")) doc.tiles.addEncoded(name.slice(6, -4), data);
      else if (name.startsWith("refs/")) doc.refs.embedded.set(name, data);
      else if (name.startsWith("thumbs/") && name.endsWith(".png")) doc.thumbs.set(name.slice(7, -4), data);
      else if (name === "preview.png") doc.preview = data;
    }
    return doc;
  }

  static create(abs: string, board: Board): StoryDoc {
    const doc = new StoryDoc(abs);
    doc.board = board;
    doc.changed = true;
    return doc;
  }

  frame(id: string): Frame | undefined {
    return this.board.frames.find((f) => f.id === id);
  }

  layer(id: string): Layer | undefined {
    return this.board.layers[id];
  }

  /** Saves now (or right after a save in progress). */
  save(): Promise<void> {
    if (this.saving) {
      this.again = true;
      return this.saving;
    }
    this.saving = (async () => {
      try {
        do {
          this.again = false;
          await this.write();
        } while (this.again);
      } finally {
        this.saving = null;
      }
    })();
    return this.saving;
  }

  private async write() {
    await writeBytes(this.abs, await this.encode(), true);
    this.changed = false;
  }

  /** The whole file, as bytes. */
  async encode(): Promise<Uint8Array> {
    // Layers no frame uses any more go now, with their pixels.
    const retained = this.retain();
    for (const id of unusedLayers(this.board)) {
      if (retained.has(id)) continue;
      this.tiles.deleteLayer(id);
      delete this.board.layers[id];
    }
    const frames = new Set(this.board.frames.map((f) => f.id));
    for (const id of [...this.thumbs.keys()]) if (!frames.has(id)) this.thumbs.delete(id);
    const usedRefs = new Set(this.board.references.map((r) => r.path));
    for (const name of [...this.refs.embedded.keys()]) if (!usedRefs.has(name)) this.refs.embedded.delete(name);
    await this.tiles.flush();
    this.board.modified = Date.now();
    const entries: ZipEntry[] = [{ name: "board.json", data: new TextEncoder().encode(JSON.stringify(this.board)) }];
    if (this.preview) entries.push({ name: "preview.png", data: this.preview });
    for (const [id, data] of this.thumbs) entries.push({ name: `thumbs/${id}.png`, data });
    for (const [name, data] of this.refs.embedded) entries.push({ name, data });
    entries.push(...this.tiles.entries(new Set(Object.keys(this.board.layers))));
    return writeZip(entries);
  }
}

/** Reads only what a list or an embed needs: board.json and preview.png. */
export async function peek(abs: string): Promise<{ board: Board; preview: Uint8Array | null; thumbs: Map<string, Uint8Array> }> {
  const files = await readZip(await readBytes(abs));
  const json = files.get("board.json");
  if (!json) throw new Error("not a storyboard file");
  const thumbs = new Map<string, Uint8Array>();
  for (const [name, data] of files) if (name.startsWith("thumbs/") && name.endsWith(".png")) thumbs.set(name.slice(7, -4), data);
  return { board: normalizeBoard(JSON.parse(new TextDecoder().decode(json))), preview: files.get("preview.png") || null, thumbs };
}
