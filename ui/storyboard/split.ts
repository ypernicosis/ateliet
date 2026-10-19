// How the window is shared between the drawing and the reference.
//
// In focus mode the two sit side by side or one above the other. Automatic
// picks the arrangement and the share from the shape of the frame and the
// shape of the picture on show: both come out about the same size on screen
// (sight-size: the eye compares them at a glance), and the reference panel
// hugs its picture instead of framing it with empty space. A wide picture
// next to a wide frame goes below it; a tall one goes beside it. A side can
// also be chosen by hand. Dragging the divider fixes the share (it snaps to
// the fit, a third, a half and two thirds); a double click fits it again.
//
// Outside focus, the reference shares the sidebar with the layers: its height
// follows the picture's shape at the sidebar's width until its divider is
// dragged (a double click gives it back to the picture).
//
// In both, the reference can float instead: a small window over the drawing,
// moved by its title (it sticks to the edges), resized by its corner, shaped
// like its picture until it is resized (a double click on the title fits it
// again).

export type Arrange = "auto" | "right" | "left" | "below" | "above";
type Side = Exclude<Arrange, "auto">;

export interface FloatBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface SplitState {
  arrange: Arrange;
  /** Share of the reference; null fits it to the picture on show. */
  share: number | null;
  /** A window over the drawing instead of a panel beside it. */
  floating: boolean;
  /** Where the window is, in pixels of the work area. */
  float: FloatBox | null;
  /** The window was resized by hand (otherwise it takes the picture's shape). */
  floatSized: boolean;
}

/** What the share is computed from (pixels). */
export interface SplitInput {
  /** The room both share along each axis (divider excluded). */
  w: number;
  h: number;
  /** The reference panel around its picture (title, buttons, strip). */
  chrome: { w: number; h: number };
  /** Room kept around a fitted frame. */
  margin: number;
  /** Frame aspect (w / h); null when it has no edges. */
  canvas: number | null;
  /** Picture aspect; null without a picture. */
  ref: number | null;
}

const KEY = "ateliet-storyboard-split";
const AUTO_MIN = 0.2, AUTO_MAX = 0.65;
const DRAG_MIN = 0.12, DRAG_MAX = 0.8;
/** The other arrangement must be this much better before automatic flips to it. */
const FLIP = 1.15;
const SNAP_PX = 10;
/** Room kept between a floating window and the edges; closer than STICK it sticks to them. */
const EDGE = 10, STICK = 24;
const FLOAT_MIN_W = 200, FLOAT_MIN_H = 150;

/** Area of a picture of `aspect` fitted in a box (the whole box without one). */
function fittedArea(bw: number, bh: number, aspect: number | null): number {
  if (bw <= 0 || bh <= 0) return 0;
  if (!aspect) return bw * bh;
  return bw / bh > aspect ? bh * bh * aspect : (bw * bw) / aspect;
}

/** How well a share works: the smaller of the two pictures on screen. */
export function splitScore(i: SplitInput, stacked: boolean, f: number): number {
  const ref = stacked ? fittedArea(i.w - i.chrome.w, f * i.h - i.chrome.h, i.ref) : fittedArea(f * i.w - i.chrome.w, i.h - i.chrome.h, i.ref);
  const cw = stacked ? i.w : (1 - f) * i.w, ch = stacked ? (1 - f) * i.h : i.h;
  // Without edges the drawing can be anywhere; most of the room is still the drawing's.
  const canvas = i.canvas ? fittedArea(cw - 2 * i.margin, ch - 2 * i.margin, i.canvas) : 0.6 * cw * ch;
  return Math.min(ref, canvas);
}

/** The best share for an arrangement; on a tie the smaller one, so the panel hugs its picture. */
export function bestShare(i: SplitInput, stacked: boolean): { f: number; score: number } {
  if (!i.ref) return { f: stacked ? 0.35 : 0.3, score: 0 };
  let best = { f: AUTO_MIN, score: -1 };
  for (let f = AUTO_MIN; f <= AUTO_MAX + 1e-9; f += 0.005) {
    const s = splitScore(i, stacked, f);
    if (s > best.score * 1.0005) best = { f, score: s };
  }
  return best;
}

const stackedSide = (s: Side) => s === "below" || s === "above";

export interface SplitHost {
  root: HTMLElement;
  main: HTMLElement;
  tools: HTMLElement;
  center: HTMLElement;
  side: HTMLElement;
  /** The divider between the drawing and the side. */
  handle: HTMLElement;
  /** Outside focus: the reference panel, its picture area, and the divider under it. */
  refs: HTMLElement;
  refStage: HTMLElement;
  resizer: HTMLElement;
  focus(): boolean;
  /** The frame's shape as it is fitted in view, or null. */
  canvasAspect(): number | null;
  canvasMargin: number;
  /** Floating on or off (for the buttons that show it). */
  floated?(on: boolean): void;
}

export class Split {
  state: SplitState = { arrange: "auto", share: null, floating: false, float: null, floatSized: false };
  /** The arrangement automatic chose last (it only flips for a clearly better one). */
  private autoSide: Side = "right";
  private picture: { w: number; h: number } | null = null;
  /** Outside focus: the reference's height, once its divider was dragged. */
  private refsHeight: number | null = null;
  private resize: ResizeObserver;
  private raf = 0;

  constructor(private h: SplitHost) {
    this.load();
    this.resize = new ResizeObserver(() => this.schedule());
    this.resize.observe(h.main);
    // The sidebar coming back from hidden, or the toolbar wrapping, resizes it alone.
    this.resize.observe(h.side);
    this.bindHandle();
    this.bindResizer();
    this.bindFloat();
    this.loadSideWidth();
  }

  destroy() {
    this.resize.disconnect();
    cancelAnimationFrame(this.raf);
  }

  /** The picture on show changed (null: none). */
  setPicture(size: { w: number; h: number } | null) {
    this.picture = size && size.w > 0 && size.h > 0 ? size : null;
    this.schedule();
  }

  /** The side the reference is on now. */
  get current(): Side {
    return this.state.arrange === "auto" ? this.autoSide : this.state.arrange;
  }

  setArrange(a: Arrange) {
    Object.assign(this.state, { arrange: a, share: null, floating: false });
    this.save();
    this.layout();
  }

  setFloating(on: boolean) {
    this.state.floating = on;
    this.save();
    this.layout();
  }

  /** Back to fitting the share (or the floating window) to the picture. */
  fitAgain() {
    if (this.state.floating) {
      this.state.floatSized = false;
      this.save();
    } else if (this.h.focus()) {
      this.state.share = null;
      this.save();
    } else this.refsHeight = null;
    this.layout();
  }

  schedule() {
    cancelAnimationFrame(this.raf);
    this.raf = requestAnimationFrame(() => this.layout());
  }

  layout() {
    const h = this.h;
    const focus = h.focus();
    for (const s of ["right", "left", "below", "above"]) h.root.removeClass(`ref-${s}`);
    h.root.toggleClass("ref-float", this.state.floating);
    h.floated?.(this.state.floating);
    if (this.state.floating) {
      h.main.style.removeProperty("--sb-ref");
      this.layoutFloat();
      return;
    }
    for (const p of ["left", "top", "width", "height"]) h.refs.style.removeProperty(p);
    if (!focus) {
      h.main.style.removeProperty("--sb-ref");
      this.layoutSidebar();
      return;
    }
    h.refs.style.flex = "";
    const input = this.input();
    let side = this.current;
    let f = this.state.share;
    if (this.state.arrange === "auto") {
      const across = bestShare(input.across, false), down = bestShare(input.down, true);
      const now = stackedSide(this.autoSide) ? down : across, other = stackedSide(this.autoSide) ? across : down;
      if (input.across.ref && other.score > now.score * FLIP) this.autoSide = stackedSide(this.autoSide) ? "right" : "below";
      side = this.autoSide;
    }
    if (f == null) f = bestShare(stackedSide(side) ? input.down : input.across, stackedSide(side)).f;
    h.root.addClass(`ref-${side}`);
    const px = `${Math.round(f * (stackedSide(side) ? input.down.h : input.across.w))}px`;
    if (h.main.style.getPropertyValue("--sb-ref") !== px) h.main.style.setProperty("--sb-ref", px);
  }

  /** The room in both arrangements, and what the reference panel takes around its picture. */
  private input(): { across: SplitInput; down: SplitInput } {
    const h = this.h;
    const m = h.main.getBoundingClientRect();
    const tools = h.tools.offsetWidth;
    const divider = h.handle.offsetWidth || 1;
    // Measured when the panel is showing; its bars hardly change with its size.
    const sw = h.side.clientWidth, sh = h.side.clientHeight;
    const stw = h.refStage.clientWidth, sth = h.refStage.clientHeight;
    const chrome = sw > 0 && sh > 0 && sth > 0 ? { w: Math.max(0, sw - stw), h: Math.max(0, sh - sth) } : { w: 0, h: 130 };
    // The picture area keeps a small margin of its own.
    chrome.w += 16;
    chrome.h += 16;
    const base = { chrome, margin: h.canvasMargin, canvas: h.canvasAspect(), ref: this.picture ? this.picture.w / this.picture.h : null };
    return {
      across: { ...base, w: Math.max(1, m.width - tools - divider), h: Math.max(1, m.height) },
      down: { ...base, w: Math.max(1, m.width - tools), h: Math.max(1, m.height - divider) },
    };
  }

  /** Outside focus: the reference as tall as its picture is at the sidebar's width. */
  private layoutSidebar() {
    const h = this.h;
    const total = h.side.clientHeight;
    if (!total) return;
    const max = Math.max(120, total - 160);
    if (this.refsHeight != null) {
      h.refs.style.flex = `0 0 ${Math.min(max, this.refsHeight)}px`;
      return;
    }
    if (!this.picture) {
      h.refs.style.flex = "";
      return;
    }
    const chrome = Math.max(0, h.refs.offsetHeight - h.refStage.clientHeight);
    const width = h.refStage.clientWidth - 16;
    if (width <= 0) return;
    const want = chrome + 16 + Math.max(90, (width * this.picture.h) / this.picture.w);
    h.refs.style.flex = `0 0 ${Math.round(Math.max(120, Math.min(max, want)))}px`;
  }

  // ── floating ────────────────────────────────────────────────────────────

  /** The window placed (and kept inside the work area), shaped like its picture unless resized. */
  private layoutFloat() {
    const h = this.h;
    h.refs.style.flex = "";
    const m = h.main.getBoundingClientRect();
    if (!m.width || !m.height) return;
    let box = this.state.float;
    if (!box) {
      // First time: the top right corner of the drawing, under the focus bar.
      const w = Math.round(Math.max(FLOAT_MIN_W + 60, Math.min(440, m.width * 0.3)));
      const c = h.center.getBoundingClientRect();
      box = { x: c.right - m.left - w - EDGE - 6, y: h.focus() ? 56 : EDGE + 6, w, h: Math.round(w * 0.9) };
    }
    box = { ...box };
    if (!this.state.floatSized && this.picture) {
      const chrome = this.floatChrome();
      const maxH = m.height - 2 * EDGE;
      box.h = Math.round(chrome + (box.w - 16) * this.picture.h / this.picture.w);
      if (box.h > maxH) {
        box.h = maxH;
        box.w = Math.round(Math.max(FLOAT_MIN_W, 16 + (maxH - chrome) * this.picture.w / this.picture.h));
      }
    }
    this.state.float = this.clampFloat(box);
    this.placeFloat();
  }

  /** Title, buttons and strip around the picture, with the picture's margin. */
  private floatChrome(): number {
    const h = this.h;
    const c = h.refs.offsetHeight - h.refStage.clientHeight;
    return (c > 0 && h.refStage.clientHeight > 0 ? c : 130) + 16;
  }

  private clampFloat(b: FloatBox): FloatBox {
    const m = this.h.main.getBoundingClientRect();
    const w = Math.max(FLOAT_MIN_W, Math.min(m.width - 2 * EDGE, b.w));
    const hh = Math.max(FLOAT_MIN_H, Math.min(m.height - 2 * EDGE, b.h));
    const x = Math.max(EDGE, Math.min(m.width - w - EDGE, b.x));
    const y = Math.max(EDGE, Math.min(m.height - hh - EDGE, b.y));
    return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(hh) };
  }

  private placeFloat() {
    const b = this.state.float!;
    Object.assign(this.h.refs.style, { left: `${b.x}px`, top: `${b.y}px`, width: `${b.w}px`, height: `${b.h}px` });
  }

  /** Moving by the title, resizing by the corner grip. */
  private bindFloat() {
    const refs = this.h.refs;
    const head = refs.querySelector(".sb-panel-head") as HTMLElement | null;
    const grip = refs.createDiv({ cls: "sb-refs-grip", attr: { "aria-label": "Resize" } });
    const drag = (el: HTMLElement, move: (dx: number, dy: number, b: FloatBox) => FloatBox, ignore?: string) => {
      el.addEventListener("pointerdown", (e) => {
        if (!this.state.floating || e.button !== 0 || !this.state.float) return;
        if (ignore && (e.target as HTMLElement).closest(ignore)) return;
        e.preventDefault();
        e.stopPropagation();
        try { el.setPointerCapture(e.pointerId); } catch { /* synthetic */ }
        refs.addClass("is-moving");
        const x0 = e.clientX, y0 = e.clientY, b0 = { ...this.state.float };
        const onMove = (ev: PointerEvent) => {
          this.state.float = move(ev.clientX - x0, ev.clientY - y0, b0);
          this.placeFloat();
        };
        const up = () => {
          el.removeEventListener("pointermove", onMove);
          el.removeEventListener("pointerup", up);
          el.removeEventListener("pointercancel", up);
          refs.removeClass("is-moving");
          this.save();
        };
        el.addEventListener("pointermove", onMove);
        el.addEventListener("pointerup", up);
        el.addEventListener("pointercancel", up);
      });
    };
    if (head) {
      drag(head, (dx, dy, b) => this.stick(this.clampFloat({ ...b, x: b.x + dx, y: b.y + dy })), ".clickable-icon");
      head.addEventListener("dblclick", (e) => {
        if (!this.state.floating || (e.target as HTMLElement).closest(".clickable-icon")) return;
        this.fitAgain();
      });
    }
    drag(grip, (dx, dy, b) => {
      this.state.floatSized = true;
      return this.clampFloat({ ...b, w: b.w + dx, h: b.h + dy });
    });
  }

  /** Close to an edge of the work area, the window sticks to it. */
  private stick(b: FloatBox): FloatBox {
    const m = this.h.main.getBoundingClientRect();
    const right = m.width - b.w - EDGE, bottom = m.height - b.h - EDGE;
    if (b.x - EDGE < STICK) b.x = EDGE;
    else if (right - b.x < STICK) b.x = right;
    if (b.y - EDGE < STICK) b.y = EDGE;
    else if (bottom - b.y < STICK) b.y = bottom;
    return b;
  }

  // ── dragging ────────────────────────────────────────────────────────────

  private bindHandle() {
    const handle = this.h.handle;
    handle.addEventListener("dblclick", () => {
      if (this.h.focus()) this.fitAgain();
    });
    handle.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      try { handle.setPointerCapture(e.pointerId); } catch { /* synthetic */ }
      handle.addClass("is-dragging");
      const move = this.h.focus() ? this.focusDrag() : this.sidebarDrag(e);
      const onMove = (ev: PointerEvent) => move(ev);
      const up = () => {
        handle.removeEventListener("pointermove", onMove);
        handle.removeEventListener("pointerup", up);
        handle.removeEventListener("pointercancel", up);
        handle.removeClass("is-dragging", "is-snapped");
        if (this.h.focus()) this.save();
        else this.saveSideWidth();
      };
      handle.addEventListener("pointermove", onMove);
      handle.addEventListener("pointerup", up);
      handle.addEventListener("pointercancel", up);
    });
  }

  /** Focus: the divider follows the pointer, in shares of the room, snapping to useful places. */
  private focusDrag(): (e: PointerEvent) => void {
    const side = this.current;
    const stacked = stackedSide(side);
    const input = this.input();
    const fit = bestShare(stacked ? input.down : input.across, stacked).f;
    const room = stacked ? input.down.h : input.across.w;
    const m = this.h.main.getBoundingClientRect();
    // Automatic keeps the side it is on once the divider is moved.
    this.state.arrange = side;
    return (e) => {
      const at = side === "right" ? m.right - e.clientX : side === "left" ? e.clientX - m.left : side === "below" ? m.bottom - e.clientY : e.clientY - m.top;
      let f = Math.max(DRAG_MIN, Math.min(DRAG_MAX, at / room));
      const snap = [fit, 1 / 3, 1 / 2, 2 / 3].find((c) => Math.abs(c - f) * room < SNAP_PX);
      if (snap != null) f = snap;
      this.h.handle.toggleClass("is-snapped", snap != null);
      this.state.share = snap === fit ? null : f;
      this.layout();
    };
  }

  private sidebarDrag(e: PointerEvent): (e: PointerEvent) => void {
    const side = this.h.side;
    const x0 = e.clientX, w0 = side.getBoundingClientRect().width;
    return (ev) => {
      side.style.width = `${Math.max(200, Math.min(900, w0 - (ev.clientX - x0)))}px`;
      this.layoutSidebar();
    };
  }

  private bindResizer() {
    const r = this.h.resizer;
    r.setAttr("aria-label", "Drag to resize · double click: fit the picture");
    r.addEventListener("dblclick", () => this.fitAgain());
    r.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      try { r.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
      r.addClass("is-dragging");
      const y0 = e.clientY, h0 = this.h.refs.getBoundingClientRect().height;
      const move = (ev: PointerEvent) => {
        const total = this.h.side.getBoundingClientRect().height;
        this.refsHeight = Math.max(120, Math.min(total - 140, h0 + ev.clientY - y0));
        this.layoutSidebar();
      };
      const up = () => {
        r.removeEventListener("pointermove", move);
        r.removeEventListener("pointerup", up);
        r.removeEventListener("pointercancel", up);
        r.removeClass("is-dragging");
      };
      r.addEventListener("pointermove", move);
      r.addEventListener("pointerup", up);
      r.addEventListener("pointercancel", up);
    });
  }

  // ── remembering ─────────────────────────────────────────────────────────

  private load() {
    try {
      const raw = localStorage.getItem(KEY);
      if (raw) {
        const s = JSON.parse(raw);
        if (["auto", "right", "left", "below", "above"].includes(s.arrange)) this.state.arrange = s.arrange;
        if (typeof s.share === "number") this.state.share = Math.max(DRAG_MIN, Math.min(DRAG_MAX, s.share));
        this.state.floating = !!s.floating;
        this.state.floatSized = !!s.floatSized;
        const f = s.float;
        if (f && [f.x, f.y, f.w, f.h].every((v) => typeof v === "number")) this.state.float = { x: f.x, y: f.y, w: f.w, h: f.h };
      } else if (localStorage.getItem("ateliet-storyboard-focus-left") === "1") {
        // The reference was kept on the left before there was a choice of four.
        this.state.arrange = "left";
      }
    } catch {
      /* private mode */
    }
  }

  private save() {
    try {
      localStorage.setItem(KEY, JSON.stringify(this.state));
    } catch {
      /* private mode */
    }
  }

  private loadSideWidth() {
    try {
      const w = Number(localStorage.getItem("ateliet-storyboard-side") || 0);
      if (w) this.h.side.style.width = `${w}px`;
    } catch {
      /* private mode */
    }
  }

  private saveSideWidth() {
    try {
      localStorage.setItem("ateliet-storyboard-side", String(Math.round(this.h.side.getBoundingClientRect().width)));
    } catch {
      /* private mode */
    }
  }
}
