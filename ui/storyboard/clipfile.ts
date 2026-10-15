// Clip Studio files: the Rust core reads the layers (src/clipstudio.rs) and
// hands them over as PNGs; here they become layers of a frame, with their
// names (folder / layer), visibility, opacity, blend mode and clipping.

import { Board, Layer, newBoard, newLayer, PAPER } from "./model.ts";
import { StoryDoc, decodeImage } from "./store.ts";
import { invoke } from "../tauri.ts";

export interface ClipLayer {
  name: string;
  folders: string[];
  visible: boolean;
  opacity: number;
  blend: string;
  blendApprox: string | null;
  clip: boolean;
  kind: "raster" | "vector" | "paper";
  x: number;
  y: number;
  width: number;
  height: number;
  color?: string;
  png?: { offset: number; length: number };
}

export interface ClipFile {
  width: number;
  height: number;
  dpi: number;
  layers: ClipLayer[];
  skipped: string[];
  preview: Uint8Array | null;
  blobs: Uint8Array;
}

export async function readClip(path: string): Promise<ClipFile> {
  const raw = await invoke<any>("clip_import", { path });
  const buf: Uint8Array = raw instanceof ArrayBuffer ? new Uint8Array(raw) : raw instanceof Uint8Array ? raw : new Uint8Array(raw);
  if (String.fromCharCode(...buf.subarray(0, 4)) !== "ACLP") throw new Error("unexpected answer from the Clip Studio reader");
  const n = new DataView(buf.buffer, buf.byteOffset).getUint32(4, true);
  const head = JSON.parse(new TextDecoder().decode(buf.subarray(8, 8 + n)));
  const blobs = buf.subarray(8 + n);
  const preview = head.preview ? blobs.subarray(head.preview.offset, head.preview.offset + head.preview.length) : null;
  return { ...head, preview, blobs };
}

/** Puts the file's layers into `frameId` of the document (bottom to top), above index `at`. */
export async function addClipLayers(doc: StoryDoc, clip: ClipFile, frameId: string, at: number, opts: { paperAsBackground: boolean; dx?: number; dy?: number }): Promise<{ layers: Layer[]; approximated: string[] }> {
  const dx = Math.round(opts.dx || 0), dy = Math.round(opts.dy || 0);
  const frame = doc.frame(frameId)!;
  const made: Layer[] = [];
  const approximated: string[] = [];
  let paperDone = !opts.paperAsBackground;
  for (const cl of clip.layers) {
    const name = [...cl.folders, cl.name].filter(Boolean).join(" / ") || "Layer";
    if (cl.kind === "paper" && !paperDone && cl.visible && cl.color) {
      doc.board.background = cl.color;
      paperDone = true;
      continue;
    }
    const l = newLayer(name, "raster", { visible: cl.visible, opacity: cl.opacity, blend: cl.blend, clip: cl.clip, x: dx, y: dy });
    doc.board.layers[l.id] = l;
    if (cl.kind === "paper" && cl.color) {
      doc.tiles.paint(l.id, 0, 0, clip.width, clip.height, (ctx) => {
        ctx.fillStyle = cl.color!;
        ctx.fillRect(0, 0, clip.width, clip.height);
      });
    } else if (cl.png) {
      const bmp = await decodeImage(clip.blobs.subarray(cl.png.offset, cl.png.offset + cl.png.length));
      doc.tiles.paint(l.id, cl.x, cl.y, cl.width, cl.height, (ctx) => ctx.drawImage(bmp, 0, 0));
      bmp.close?.();
    }
    if (cl.blendApprox) approximated.push(cl.blendApprox);
    made.push(l);
  }
  frame.layers.splice(at, 0, ...made.map((l) => l.id));
  if (made.length) frame.active = made[made.length - 1].id;
  return { layers: made, approximated };
}

/** A new storyboard holding the .clip file as its first frame. */
export async function boardFromClip(abs: string, clip: ClipFile, fps = 24): Promise<{ doc: StoryDoc; approximated: string[] }> {
  const board: Board = newBoard({ width: clip.width, height: clip.height, infinite: false, background: PAPER, fps });
  const doc = StoryDoc.create(abs, board);
  const frame = board.frames[0];
  // The empty drawing layer a new board starts with goes on top.
  const { approximated } = await addClipLayers(doc, clip, frame.id, 0, { paperAsBackground: true });
  if (clip.preview) doc.preview = clip.preview;
  return { doc, approximated };
}

/** A short account of what an import could not carry over exactly. */
export function importSummary(clip: ClipFile, approximated: string[]): string {
  const parts: string[] = [];
  const corrections = clip.skipped.filter((s) => s.endsWith(": correction layer")).length;
  const other = clip.skipped.filter((s) => !s.endsWith(": correction layer"));
  if (corrections) parts.push(`${corrections} correction ${corrections === 1 ? "layer" : "layers"} left out (they have no pixels of their own)`);
  if (other.length) parts.push(`left out: ${other.slice(0, 3).join("; ")}${other.length > 3 ? ` and ${other.length - 3} more` : ""}`);
  if (approximated.length) {
    const kinds = [...new Set(approximated)];
    parts.push(`${approximated.length} ${approximated.length === 1 ? "blend mode" : "blend modes"} approximated (${kinds.join(", ")})`);
  }
  return parts.length ? ` ${parts.join("; ")}.` : "";
}
