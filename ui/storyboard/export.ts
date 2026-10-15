// Getting frames out: PNG files in the vault, a contact sheet (the frames in
// a grid with their numbers, durations and notes, ready to print or embed),
// Pictogrep's storyboards folder, and a video when the system can record one.

import { Editor } from "./editor.ts";
import { Frame, aspectOf, frameStarts, timecode } from "./model.ts";
import { invoke } from "../tauri.ts";
import { crc32 } from "./zip.ts";
import { TFile } from "../obsidian/vault.ts";

export async function canvasPng(c: HTMLCanvasElement): Promise<Uint8Array> {
  const blob: Blob | null = await new Promise((r) => c.toBlob(r, "image/png"));
  if (!blob) throw new Error("could not encode the picture");
  return new Uint8Array(await blob.arrayBuffer());
}

async function ensureFolder(app: any, folder: string) {
  if (folder && !app.vault.getAbstractFileByPath(folder)) await app.vault.createFolder(folder).catch(() => {});
}

async function writeVault(app: any, path: string, bytes: Uint8Array): Promise<TFile> {
  const existing = app.vault.getAbstractFileByPath(path);
  const buf = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
  if (existing instanceof TFile) {
    await app.vault.modifyBinary(existing, buf);
    return existing;
  }
  return app.vault.createBinary(path, buf);
}

function safe(name: string): string {
  return name.replace(/[\\/:*?"<>|#^[\]]/g, "-").trim() || "frame";
}

export interface FrameExport {
  scale: number;
  withTrace: boolean;
  transparent: boolean;
}

/** One PNG per frame into a vault folder; returns the files. */
export async function exportFrames(ed: Editor, folder: string, o: FrameExport, progress?: (i: number, n: number) => void): Promise<TFile[]> {
  const app = (ed as any).app;
  await ensureFolder(app, folder);
  const out: TFile[] = [];
  const base = safe(ed.doc.abs.split(/[\\/]/).pop()!.replace(/\.storyboard$/i, ""));
  for (const [i, f] of ed.board.frames.entries()) {
    progress?.(i, ed.board.frames.length);
    const pic = await framePicture(ed, f, o);
    const name = `${base} ${String(i + 1).padStart(3, "0")}${f.name ? " " + safe(f.name) : ""}.png`;
    out.push(await writeVault(app, folder ? `${folder}/${name}` : name, await canvasPng(pic)));
  }
  return out;
}

export async function framePicture(ed: Editor, f: Frame, o: FrameExport): Promise<HTMLCanvasElement> {
  const traces = f.layers.map((id) => ed.board.layers[id]).filter((l) => l?.kind === "trace");
  const saved = traces.map((l) => l.trace!.export);
  if (o.withTrace) traces.forEach((l) => (l.trace!.export = true));
  try {
    return await ed.comp.picture(f, o.scale, { exporting: true, background: !o.transparent });
  } finally {
    traces.forEach((l, i) => (l.trace!.export = saved[i]));
  }
}

/** All frames in a grid, each with its number, name, duration, shot, camera and notes. */
export async function contactSheet(ed: Editor, o: { columns: number; notes: boolean; width: number }): Promise<HTMLCanvasElement> {
  const b = ed.board;
  const frames = b.frames;
  const pics: HTMLCanvasElement[] = [];
  for (const f of frames) pics.push(await ed.comp.picture(f, 1, { exporting: true }));
  const cols = Math.max(1, Math.min(o.columns, frames.length));
  const pad = 32, gap = 28;
  const cellW = Math.floor((o.width - 2 * pad - (cols - 1) * gap) / cols);
  const aspect = pics.reduce((m, p) => Math.max(m, p.height / p.width), 0) || 9 / 16;
  const imgH = Math.round(cellW * aspect);
  const lineH = Math.max(14, Math.round(cellW / 26));
  const textLines = o.notes ? 5 : 1;
  const cellH = imgH + 10 + lineH * (textLines + 0.5);
  const rows = Math.ceil(frames.length / cols);
  const title = ed.doc.abs.split(/[\\/]/).pop()!.replace(/\.storyboard$/i, "");
  const headH = lineH * 3;
  const c = document.createElement("canvas");
  c.width = o.width;
  c.height = Math.round(pad * 2 + headH + rows * cellH + (rows - 1) * gap);
  const ctx = c.getContext("2d")!;
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, c.width, c.height);
  ctx.fillStyle = "#23221f";
  ctx.font = `600 ${Math.round(lineH * 1.5)}px sans-serif`;
  ctx.fillText(title, pad, pad + lineH * 1.4);
  ctx.font = `${lineH}px sans-serif`;
  ctx.fillStyle = "#6b6862";
  const total = frames.reduce((n, f) => n + f.duration, 0);
  ctx.fillText(`${frames.length} frames · ${timecode(total, b.fps)} at ${b.fps} fps · ${b.infinite ? "infinite canvas" : `${b.width} × ${b.height} (${aspectOf(b.width, b.height)})`}`, pad, pad + lineH * 2.7);
  const starts = frameStarts(b);
  const wrap = (text: string, maxW: number, maxLines: number): string[] => {
    const words = text.replace(/\s+/g, " ").trim().split(" ");
    const lines: string[] = [];
    let cur = "";
    for (const w of words) {
      const t = cur ? cur + " " + w : w;
      if (ctx.measureText(t).width > maxW && cur) {
        lines.push(cur);
        cur = w;
        if (lines.length === maxLines) break;
      } else cur = t;
    }
    if (cur && lines.length < maxLines) lines.push(cur);
    return lines;
  };
  frames.forEach((f, i) => {
    const col = i % cols, row = Math.floor(i / cols);
    const x = pad + col * (cellW + gap);
    const y = pad + headH + row * (cellH + gap);
    const p = pics[i];
    const k = Math.min(cellW / p.width, imgH / p.height);
    const w = p.width * k, h = p.height * k;
    ctx.drawImage(p, x + (cellW - w) / 2, y + (imgH - h) / 2, w, h);
    ctx.strokeStyle = f.color || "#23221f";
    ctx.lineWidth = f.color ? 3 : 1;
    ctx.strokeRect(x + (cellW - w) / 2 + 0.5, y + (imgH - h) / 2 + 0.5, w - 1, h - 1);
    let ty = y + imgH + 10 + lineH;
    ctx.fillStyle = "#23221f";
    ctx.font = `600 ${lineH}px sans-serif`;
    const head = `${i + 1}${f.name ? " · " + f.name : ""}`;
    ctx.fillText(head, x, ty);
    ctx.font = `${lineH}px sans-serif`;
    ctx.fillStyle = "#6b6862";
    const meta = `${timecode(starts[i], b.fps)} · ${(f.duration / b.fps).toFixed(1)}s${f.notes.shot ? " · " + f.notes.shot : ""}${f.notes.camera ? " · " + f.notes.camera : ""}`;
    const mw = ctx.measureText(meta).width;
    ctx.fillText(meta, Math.max(x + ctx.measureText(head).width + 12, x + cellW - mw), ty);
    if (!o.notes) return;
    ctx.fillStyle = "#23221f";
    const lines = [...(f.notes.action ? wrap(f.notes.action, cellW, 2) : []), ...(f.notes.dialogue ? wrap(`“${f.notes.dialogue}”`, cellW, 2).map((l) => l) : []), ...(f.notes.sound ? wrap(`♪ ${f.notes.sound}`, cellW, 1) : [])].slice(0, 4);
    for (const l of lines) {
      ty += lineH * 1.05;
      ctx.fillText(l, x, ty);
    }
  });
  return c;
}

export async function saveContactSheet(ed: Editor, path: string, o: { columns: number; notes: boolean; width: number }): Promise<TFile> {
  const app = (ed as any).app;
  const folder = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
  await ensureFolder(app, folder);
  return writeVault(app, path, await canvasPng(await contactSheet(ed, o)));
}

/** Every frame into Pictogrep's storyboards folder (replacing earlier exports of the same frames). */
export async function exportToPictogrep(ed: Editor, progress?: (i: number, n: number) => void): Promise<number> {
  const b = ed.board;
  const name = ed.doc.abs.split(/[\\/]/).pop()!.replace(/\.storyboard$/i, "");
  let n = 0;
  for (const [i, f] of b.frames.entries()) {
    progress?.(i, b.frames.length);
    const pic = await framePicture(ed, f, { scale: 1, withTrace: false, transparent: false });
    const ref = ed.doc.refs.ref(f.reference);
    const area = ed.comp.area(f);
    await invoke("pictogrep_board_save", await canvasPng(pic), {
      headers: {
        "x-index": String(i + 1),
        "x-aspect": aspectOf(Math.round(area.w), Math.round(area.h)),
        "x-name": encodeURIComponent(`${name} ${f.name || i + 1}`),
        "x-key": encodeURIComponent(`${ed.doc.abs}#${f.id}`),
        "x-source": encodeURIComponent(ref && !ref.path.startsWith("refs/") ? ref.path : ""),
        "x-query": encodeURIComponent(name),
      },
    });
    n++;
  }
  return n;
}

export function canRecordVideo(): string | null {
  const MR = (window as any).MediaRecorder;
  if (!MR || !HTMLCanvasElement.prototype.captureStream) return null;
  for (const t of ["video/webm;codecs=vp9", "video/webm;codecs=vp8", "video/webm", "video/mp4"]) if (MR.isTypeSupported?.(t)) return t;
  return null;
}

/** Plays the frames into a canvas stream and records it. */
export async function recordVideo(ed: Editor, scale: number, progress?: (s: string) => void): Promise<{ bytes: Uint8Array; ext: string }> {
  const type = canRecordVideo();
  if (!type) throw new Error("This system cannot record video here. Export the frames as PNG instead.");
  const b = ed.board;
  const pics: HTMLCanvasElement[] = [];
  for (const [i, f] of b.frames.entries()) {
    progress?.(`Rendering ${i + 1}/${b.frames.length}`);
    pics.push(await framePicture(ed, f, { scale, withTrace: false, transparent: false }));
  }
  const w = Math.max(...pics.map((p) => p.width)) & ~1, h = Math.max(...pics.map((p) => p.height)) & ~1;
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d")!;
  const stream = (c as any).captureStream(b.fps);
  const rec = new (window as any).MediaRecorder(stream, { mimeType: type, videoBitsPerSecond: 8_000_000 });
  const chunks: Blob[] = [];
  rec.ondataavailable = (e: any) => e.data.size && chunks.push(e.data);
  const done = new Promise<void>((r) => (rec.onstop = () => r()));
  const draw = (p: HTMLCanvasElement) => {
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, w, h);
    const k = Math.min(w / p.width, h / p.height);
    ctx.drawImage(p, (w - p.width * k) / 2, (h - p.height * k) / 2, p.width * k, p.height * k);
  };
  draw(pics[0]);
  rec.start();
  for (const [i, f] of b.frames.entries()) {
    progress?.(`Recording ${i + 1}/${b.frames.length}`);
    draw(pics[i]);
    await new Promise((r) => setTimeout(r, (f.duration / b.fps) * 1000));
  }
  rec.stop();
  await done;
  const blob = new Blob(chunks, { type });
  return { bytes: new Uint8Array(await blob.arrayBuffer()), ext: type.includes("mp4") ? "mp4" : "webm" };
}

export { writeVault, ensureFolder, safe };

// ── animated PNG ───────────────────────────────────────────────────────────
// APNG: every frame as a full-colour PNG frame, each held for its duration.
// No codec needed, and notes, browsers and most viewers play it.

function chunks(png: Uint8Array): { type: string; data: Uint8Array }[] {
  const out: { type: string; data: Uint8Array }[] = [];
  const v = new DataView(png.buffer, png.byteOffset, png.byteLength);
  let at = 8;
  while (at + 8 <= png.length) {
    const len = v.getUint32(at);
    const type = String.fromCharCode(...png.subarray(at + 4, at + 8));
    out.push({ type, data: png.subarray(at + 8, at + 8 + len) });
    at += 12 + len;
  }
  return out;
}

function chunk(type: string, data: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + data.length);
  const v = new DataView(out.buffer);
  v.setUint32(0, data.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  v.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

export async function animatedPng(ed: Editor, scale: number, progress?: (s: string) => void): Promise<Uint8Array> {
  const b = ed.board;
  // One size for every frame: the union of their areas.
  const areas = b.frames.map((f) => ed.comp.area(f));
  const x0 = Math.min(...areas.map((a) => a.x)), y0 = Math.min(...areas.map((a) => a.y));
  const x1 = Math.max(...areas.map((a) => a.x + a.w)), y1 = Math.max(...areas.map((a) => a.y + a.h));
  const area = { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
  const parts: Uint8Array[] = [new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])];
  let seq = 0;
  for (const [i, f] of b.frames.entries()) {
    progress?.(`Rendering ${i + 1}/${b.frames.length}`);
    const pic = await ed.comp.picture(f, scale, { exporting: true, area });
    const cs = chunks(await canvasPng(pic));
    if (i === 0) {
      parts.push(chunk("IHDR", cs.find((c) => c.type === "IHDR")!.data));
      const actl = new Uint8Array(8);
      new DataView(actl.buffer).setUint32(0, b.frames.length);
      parts.push(chunk("acTL", actl));
    }
    const fctl = new Uint8Array(26);
    const fv = new DataView(fctl.buffer);
    fv.setUint32(0, seq++);
    fv.setUint32(4, pic.width);
    fv.setUint32(8, pic.height);
    fv.setUint16(20, Math.min(65535, f.duration));
    fv.setUint16(22, Math.min(65535, b.fps));
    parts.push(chunk("fcTL", fctl));
    for (const c of cs.filter((c) => c.type === "IDAT")) {
      if (i === 0) parts.push(chunk("IDAT", c.data));
      else {
        const d = new Uint8Array(4 + c.data.length);
        new DataView(d.buffer).setUint32(0, seq++);
        d.set(c.data, 4);
        parts.push(chunk("fdAT", d));
      }
    }
  }
  parts.push(chunk("IEND", new Uint8Array()));
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}
