// The kinds of storyboard, what each one starts with, and how a new page
// or frame of each kind looks.
//
// Sizes for pages are in millimetres (trim, bleed and inner frame) and
// become pixels at the chosen resolution. Publishers differ; these are the
// common starting points, and every number can be changed in the settings.

import { Board, BoardKind, Frame, PageSetup, addFrame, isPageKind, newBoard } from "./model.ts";
import { LAYOUTS, gridFor, gridPanels, layoutPanels, pageSetupDefaults, rectPanel } from "./pages.ts";

export interface KindInfo {
  id: BoardKind;
  name: string;
  icon: string;
  /** One line under the name when choosing. */
  blurb: string;
  /** What one unit is called. */
  unit: string;
  units: string;
}

export const KINDS: KindInfo[] = [
  { id: "animatic", name: "Animatic", icon: "clapperboard", blurb: "Timed frames for film and animation: playback, onion skin, shot notes.", unit: "frame", units: "frames" },
  { id: "presentation", name: "Presentation board", icon: "presentation", blurb: "Shots as cards with their notes, printed or sent as pages (PDF).", unit: "shot", units: "shots" },
  { id: "thumbnails", name: "Thumbnails", icon: "layout-grid", blurb: "Sheets of small panels to explore fast; star the keepers and send them to an animatic.", unit: "sheet", units: "sheets" },
  { id: "name", name: "Name (manga)", icon: "book-open", blurb: "Manga pages: panels, vertical balloons, right to left, spreads.", unit: "page", units: "pages" },
  { id: "comic", name: "Comic pages", icon: "book-open-text", blurb: "Comic pages: panels, balloons and captions, left to right, spreads.", unit: "page", units: "pages" },
  { id: "webtoon", name: "Webtoon", icon: "smartphone", blurb: "A vertical strip read by scrolling, exported in upload-sized slices.", unit: "part", units: "parts" },
  { id: "canvas", name: "Infinite canvas", icon: "infinity", blurb: "One endless canvas: no frames, no timeline, no edges. Sketch, study, plan.", unit: "canvas", units: "canvases" },
];

export function kindInfo(k: BoardKind | undefined): KindInfo {
  return KINDS.find((x) => x.id === k) || KINDS[0];
}

export interface PageSize {
  id: string;
  name: string;
  /** Trim size, mm. */
  w: number;
  h: number;
  /** Bleed, mm. */
  bleed: number;
  /** Inner frame, mm (centred on the trim). */
  iw: number;
  ih: number;
}

export const PAGE_SIZES: Record<"name" | "comic", PageSize[]> = {
  name: [
    { id: "b5", name: "B5 · 182 × 257 mm (magazine, tankōbon)", w: 182, h: 257, bleed: 5, iw: 150, ih: 220 },
    { id: "a5", name: "A5 · 148 × 210 mm", w: 148, h: 210, bleed: 5, iw: 122, ih: 180 },
    { id: "b6", name: "B6 · 128 × 182 mm (small tankōbon)", w: 128, h: 182, bleed: 5, iw: 105, ih: 155 },
    { id: "a4", name: "A4 · 210 × 297 mm", w: 210, h: 297, bleed: 5, iw: 180, ih: 270 },
  ],
  comic: [
    { id: "us", name: "US comic · 6.625 × 10.25 in", w: 168.3, h: 260.4, bleed: 3.2, iw: 152.4, ih: 241.3 },
    { id: "album", name: "European album · 225 × 300 mm", w: 225, h: 300, bleed: 5, iw: 200, ih: 270 },
    { id: "a4", name: "A4 · 210 × 297 mm", w: 210, h: 297, bleed: 3, iw: 186, ih: 273 },
    { id: "letter", name: "US Letter · 8.5 × 11 in", w: 215.9, h: 279.4, bleed: 3.2, iw: 190.5, ih: 254 },
    { id: "digest", name: "Digest · 5.5 × 8.5 in", w: 139.7, h: 215.9, bleed: 3.2, iw: 120, ih: 196 },
  ],
};

export const THUMB_ASPECTS: [string, string, number][] = [
  ["16:9", "16:9 (film, video)", 16 / 9],
  ["2.39", "2.39:1 (scope)", 2.39],
  ["4:3", "4:3", 4 / 3],
  ["1:1", "1:1", 1],
  ["9:16", "9:16 (vertical video)", 9 / 16],
  ["page", "Manga / comic page", 182 / 257],
];

export const WEBTOON_WIDTHS: [number, string][] = [
  [800, "800 px (Webtoon)"],
  [690, "690 px"],
  [940, "940 px (Tapas)"],
];

const mmToPx = (mm: number, dpi: number) => Math.round((mm / 25.4) * dpi);

/** Everything a new storyboard of a kind needs. */
export interface KindOptions {
  kind: BoardKind;
  // animatic / presentation
  width: number;
  height: number;
  infinite: boolean;
  fps: number;
  background: string;
  // name / comic
  pageSize: string;
  dpi: number;
  rtl: boolean;
  spreads: boolean;
  layout: string;
  pages: number;
  // thumbnails
  aspect: string;
  landscape: boolean;
  // webtoon
  stripWidth: number;
  slices: number;
}

export function defaultKindOptions(kind: BoardKind): KindOptions {
  return {
    kind,
    width: 1920,
    height: 1080,
    infinite: false,
    fps: kind === "animatic" ? 24 : 24,
    background: kind === "name" || kind === "comic" || kind === "webtoon" || kind === "thumbnails" ? "#ffffff" : "#e0dedb",
    pageSize: kind === "comic" ? "us" : "b5",
    dpi: 150,
    rtl: kind === "name",
    spreads: kind === "name" || kind === "comic",
    layout: "one",
    pages: kind === "name" || kind === "comic" ? 4 : 1,
    aspect: "16:9",
    landscape: true,
    stripWidth: 800,
    slices: 10,
  };
}

/** Page geometry and canvas size for a page kind. */
export function pageGeometry(o: KindOptions): { width: number; height: number; page: PageSetup } {
  const page = pageSetupDefaults();
  if (o.kind === "name" || o.kind === "comic") {
    const size = PAGE_SIZES[o.kind].find((s) => s.id === o.pageSize) || PAGE_SIZES[o.kind][0];
    const dpi = o.dpi || 150;
    const bleed = mmToPx(size.bleed, dpi);
    const width = mmToPx(size.w, dpi) + 2 * bleed, height = mmToPx(size.h, dpi) + 2 * bleed;
    const iw = mmToPx(size.iw, dpi), ih = mmToPx(size.ih, dpi);
    Object.assign(page, {
      dpi, bleed, inner: { x: Math.round((width - iw) / 2), y: Math.round((height - ih) / 2), w: iw, h: ih },
      rtl: o.rtl, spreads: o.spreads,
      // Manga: narrow gutters side by side, wider between tiers.
      gutterX: mmToPx(o.kind === "name" ? 2 : 3, dpi), gutterY: mmToPx(o.kind === "name" ? 5 : 3, dpi),
      border: Math.max(2, mmToPx(0.6, dpi)), mask: true, clipToPanel: true,
    });
    return { width, height, page };
  }
  if (o.kind === "thumbnails") {
    const dpi = 150;
    const width = o.landscape ? mmToPx(297, dpi) : mmToPx(210, dpi), height = o.landscape ? mmToPx(210, dpi) : mmToPx(297, dpi);
    const m = mmToPx(12, dpi);
    Object.assign(page, { dpi, bleed: 0, inner: { x: m, y: m, w: width - 2 * m, h: height - 2 * m }, rtl: false, spreads: false, gutterX: mmToPx(6, dpi), gutterY: mmToPx(6, dpi), border: 3, mask: true, clipToPanel: true });
    return { width, height, page };
  }
  // webtoon
  const width = o.stripWidth || 800;
  const height = 1280 * Math.max(1, o.slices || 10);
  const m = Math.round(width * 0.05);
  Object.assign(page, { dpi: 72, bleed: 0, inner: { x: m, y: 0, w: width - 2 * m, h: height }, rtl: false, spreads: false, gutterX: 30, gutterY: 200, border: 3, mask: false, clipToPanel: false, slice: 1280 });
  return { width, height, page };
}

export function thumbAspect(o: { aspect: string }): number {
  return THUMB_ASPECTS.find((a) => a[0] === o.aspect)?.[2] || 16 / 9;
}

/** The panels a fresh page of this board starts with. */
export function startPanels(b: Board, layout?: string, aspect?: number) {
  const p = b.page;
  if (!p) return [];
  if (b.kind === "webtoon") return [];
  if (b.kind === "thumbnails") {
    const a = aspect || b.thumbAspect || 16 / 9;
    const { cols, rows } = gridFor(p.inner, a);
    const label = Math.round(Math.min(b.width, b.height) / 40);
    return gridPanels(p.inner, cols, rows, a, p.gutterX, label);
  }
  const spec = LAYOUTS.find((l) => l.id === (layout || "one")) || LAYOUTS[0];
  if (spec.id === "one") return [rectPanel(p.inner)];
  return layoutPanels(p.inner, spec.rows, p.gutterX, p.gutterY, p.rtl);
}

/** A new storyboard of a kind. */
export function boardForKind(o: KindOptions): Board {
  if (o.kind === "canvas") {
    // The size is only where the view starts.
    return newBoard({ kind: "canvas", width: o.width, height: o.height, infinite: true, background: o.background, fps: o.fps });
  }
  if (!isPageKind(o.kind)) {
    return newBoard({ kind: o.kind, width: o.width, height: o.height, infinite: o.infinite, background: o.background, fps: o.fps });
  }
  const g = pageGeometry(o);
  const b = newBoard({ kind: o.kind, page: g.page, width: g.width, height: g.height, infinite: false, background: o.background, fps: o.fps });
  if (o.kind === "thumbnails") b.thumbAspect = thumbAspect(o);
  b.frames[0].panels = startPanels(b, o.layout);
  b.frames[0].balloons = [];
  for (let i = 1; i < Math.max(1, o.pages); i++) addPage(b, i, o.layout);
  return b;
}

/** A new frame, or for page kinds a new page with its starting panels. */
export function addPage(b: Board, index: number, layout?: string): Frame {
  const f = addFrame(b, index);
  if (isPageKind(b.kind)) {
    // Thumbnail sheets repeat the previous sheet's grid; pages start as one panel to cut.
    const prev = b.frames[index - 1];
    f.panels = b.kind === "thumbnails" && prev?.panels?.length ? prev.panels.map((p) => ({ ...p, id: p.id + "-" + f.id.slice(0, 4), star: false, points: p.points.map((q) => ({ ...q })) })) : startPanels(b, layout);
    f.balloons = [];
  }
  return f;
}
