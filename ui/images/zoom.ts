// Zoom and pan for a picture inside a frame: the wheel zooms around the
// pointer, dragging moves the picture, a double click switches between the
// whole picture and its real size, two fingers pinch. Used by the picture
// viewer and by the storyboard's reference panel.

export interface PanZoomOptions {
  /** Largest zoom, as a multiple of the real size. */
  max?: number;
  /** Called after every change (zoom label, minimap…). */
  onChange?: (pz: PanZoom) => void;
  /** Margin left around the picture when it is fitted, in pixels. */
  margin?: number;
}

export class PanZoom {
  scale = 1;
  x = 0;
  y = 0;
  /** True while the picture follows the frame's size (the "whole picture" view). */
  fitted = true;
  private natW = 0;
  private natH = 0;
  private pointers = new Map<number, { x: number; y: number }>();
  private pinch: { dist: number; scale: number; cx: number; cy: number; x: number; y: number } | null = null;
  private drag: { x: number; y: number; px: number; py: number; moved: boolean } | null = null;
  private resize: ResizeObserver;
  private flipX = false;
  /** Lets "whole picture" enlarge pictures smaller than the frame (fullscreen). */
  upscale = false;

  constructor(public frame: HTMLElement, public content: HTMLElement, private opts: PanZoomOptions = {}) {
    frame.addClass("pz-frame");
    content.addClass("pz-content");
    frame.addEventListener("wheel", this.onWheel, { passive: false });
    frame.addEventListener("pointerdown", this.onDown);
    frame.addEventListener("pointermove", this.onMove);
    frame.addEventListener("pointerup", this.onUp);
    frame.addEventListener("pointercancel", this.onUp);
    frame.addEventListener("dblclick", this.onDouble);
    this.resize = new ResizeObserver(() =>
      requestAnimationFrame(() => {
        if (this.fitted) this.fit();
        else this.clamp();
      }),
    );
    this.resize.observe(frame);
  }

  destroy() {
    this.resize.disconnect();
    this.frame.removeEventListener("wheel", this.onWheel);
    this.frame.removeEventListener("pointerdown", this.onDown);
    this.frame.removeEventListener("pointermove", this.onMove);
    this.frame.removeEventListener("pointerup", this.onUp);
    this.frame.removeEventListener("pointercancel", this.onUp);
    this.frame.removeEventListener("dblclick", this.onDouble);
  }

  /** The picture's real size; call when it is known (or changes). */
  setSize(w: number, h: number, keep = false) {
    const changed = w !== this.natW || h !== this.natH;
    this.natW = w;
    this.natH = h;
    this.content.style.width = `${w}px`;
    this.content.style.height = `${h}px`;
    if (!keep || changed) this.fit();
    else this.apply();
  }

  get width() {
    return this.natW;
  }
  get height() {
    return this.natH;
  }

  /** The scale at which the whole picture fits the frame (above 1 only with `upscale`). */
  fitScale() {
    const m = this.opts.margin ?? 16;
    const fw = Math.max(1, this.frame.clientWidth - 2 * m);
    const fh = Math.max(1, this.frame.clientHeight - 2 * m);
    if (!this.natW || !this.natH) return 1;
    return Math.min(this.upscale ? Infinity : 1, fw / this.natW, fh / this.natH);
  }

  fit() {
    this.fitted = true;
    this.scale = this.fitScale();
    this.x = (this.frame.clientWidth - this.natW * this.scale) / 2;
    this.y = (this.frame.clientHeight - this.natH * this.scale) / 2;
    this.apply();
  }

  /** Zoom to `scale`, keeping the frame point (cx, cy) still (default: centre). */
  zoomTo(scale: number, cx = this.frame.clientWidth / 2, cy = this.frame.clientHeight / 2) {
    const min = Math.min(this.fitScale(), 0.05);
    const s = Math.max(min, Math.min(this.opts.max ?? 32, scale));
    this.x = cx - ((cx - this.x) * s) / this.scale;
    this.y = cy - ((cy - this.y) * s) / this.scale;
    this.scale = s;
    this.fitted = Math.abs(s - this.fitScale()) < 1e-3;
    if (this.fitted) return this.fit();
    this.clamp();
  }

  zoomBy(factor: number, cx?: number, cy?: number) {
    this.zoomTo(this.scale * factor, cx, cy);
  }

  /** Real size, centred on (cx, cy) or the frame's centre. */
  actual(cx?: number, cy?: number) {
    this.zoomTo(1, cx, cy);
  }

  panBy(dx: number, dy: number) {
    this.x += dx;
    this.y += dy;
    this.fitted = false;
    this.clamp();
  }

  /** Mirror the picture horizontally (a check for drawing). */
  setFlip(flip: boolean) {
    this.flipX = flip;
    this.apply();
  }

  /** Keep at least part of the picture on screen; centre an axis that fits. */
  private clamp() {
    const fw = this.frame.clientWidth;
    const fh = this.frame.clientHeight;
    const w = this.natW * this.scale;
    const h = this.natH * this.scale;
    if (w <= fw) this.x = (fw - w) / 2;
    else this.x = Math.min(0, Math.max(fw - w, this.x));
    if (h <= fh) this.y = (fh - h) / 2;
    else this.y = Math.min(0, Math.max(fh - h, this.y));
    this.apply();
  }

  private apply() {
    const flip = this.flipX ? ` translateX(${this.natW}px) scaleX(-1)` : "";
    this.content.style.transform = `translate(${this.x}px, ${this.y}px) scale(${this.scale})${flip}`;
    this.frame.toggleClass("is-zoomed", !this.fitted);
    this.frame.toggleClass("can-pan", this.natW * this.scale > this.frame.clientWidth + 1 || this.natH * this.scale > this.frame.clientHeight + 1);
    this.opts.onChange?.(this);
  }

  private local(e: { clientX: number; clientY: number }) {
    const r = this.frame.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const p = this.local(e);
    // A trackpad pinch arrives as a wheel with ctrlKey; a sideways scroll
    // (trackpad) or shift+wheel pans; the plain wheel zooms.
    const pixels = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaMode === 2 ? e.deltaY * 400 : e.deltaY;
    if (!e.ctrlKey && (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY))) {
      const dx = e.shiftKey && !e.deltaX ? pixels : e.deltaX;
      return this.panBy(-dx, e.shiftKey ? 0 : -pixels);
    }
    const k = e.ctrlKey ? 0.01 : 0.0018;
    this.zoomBy(Math.exp(-pixels * k), p.x, p.y);
  };

  private onDown = (e: PointerEvent) => {
    if (e.button !== 0 && e.button !== 1) return;
    if ((e.target as HTMLElement).closest?.(".pz-ignore")) return;
    try { this.frame.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
    const p = this.local(e);
    this.pointers.set(e.pointerId, p);
    if (this.pointers.size === 2) {
      const [a, b] = [...this.pointers.values()];
      this.pinch = { dist: Math.hypot(a.x - b.x, a.y - b.y) || 1, scale: this.scale, cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2, x: this.x, y: this.y };
      this.drag = null;
    } else {
      this.drag = { x: this.x, y: this.y, px: p.x, py: p.y, moved: false };
    }
    e.preventDefault();
  };

  private onMove = (e: PointerEvent) => {
    if (!this.pointers.has(e.pointerId)) return;
    const p = this.local(e);
    this.pointers.set(e.pointerId, p);
    if (this.pinch && this.pointers.size === 2) {
      const [a, b] = [...this.pointers.values()];
      const dist = Math.hypot(a.x - b.x, a.y - b.y) || 1;
      const cx = (a.x + b.x) / 2;
      const cy = (a.y + b.y) / 2;
      this.x = this.pinch.x + (cx - this.pinch.cx);
      this.y = this.pinch.y + (cy - this.pinch.cy);
      this.scale = this.pinch.scale;
      this.zoomTo(this.pinch.scale * (dist / this.pinch.dist), cx, cy);
      return;
    }
    if (!this.drag) return;
    const dx = p.x - this.drag.px;
    const dy = p.y - this.drag.py;
    if (!this.drag.moved && Math.hypot(dx, dy) < 3) return;
    this.drag.moved = true;
    this.frame.addClass("is-panning");
    this.x = this.drag.x + dx;
    this.y = this.drag.y + dy;
    this.fitted = false;
    this.clamp();
  };

  private onUp = (e: PointerEvent) => {
    this.pointers.delete(e.pointerId);
    if (this.pointers.size < 2) this.pinch = null;
    if (!this.pointers.size) {
      this.drag = null;
      this.frame.removeClass("is-panning");
    }
  };

  private onDouble = (e: MouseEvent) => {
    if ((e.target as HTMLElement).closest?.(".pz-ignore")) return;
    const p = this.local(e);
    if (this.fitted && this.fitScale() < 1) this.actual(p.x, p.y);
    else if (this.fitted) this.zoomTo(2, p.x, p.y);
    else this.fit();
  };

  /** Shared keyboard shortcuts; returns true when the key was used. */
  key(e: KeyboardEvent): boolean {
    if (e.metaKey || e.altKey) return false;
    const k = e.key;
    if (k === "+" || k === "=") this.zoomBy(1.25);
    else if (k === "-" || k === "_") this.zoomBy(0.8);
    else if (k === "0") this.fit();
    else if (k === "1" && !e.ctrlKey) this.actual();
    else return false;
    return true;
  }

  get percent() {
    return Math.round(this.scale * 100);
  }
}
