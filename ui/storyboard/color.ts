// Colour: a saturation/value square, a hue strip, the hex value, the
// storyboard's palette (kept in the file) and the colours used recently.

import { Editor } from "./editor.ts";
import { setIcon } from "../obsidian/icons.ts";

export function hexToHsv(hex: string): [number, number, number] {
  const n = parseInt(hex.replace("#", "").slice(0, 6).padEnd(6, "0"), 16);
  const r = ((n >> 16) & 255) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  let h = 0;
  if (d) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return [h, max ? d / max : 0, max];
}

export function hsvToHex(h: number, s: number, v: number): string {
  const c = v * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = v - c;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return "#" + [r, g, b].map((k) => Math.round((k + m) * 255).toString(16).padStart(2, "0")).join("");
}

export function normalizeHex(v: string): string | null {
  let s = v.trim().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/i.test(s)) s = s.split("").map((c) => c + c).join("");
  return /^[0-9a-f]{6}$/i.test(s) ? "#" + s.toLowerCase() : null;
}

/** The colour panel; lives in a popover opened from the colour swatch. */
export class ColorPanel {
  el: HTMLElement;
  private h = 0;
  private s = 0;
  private v = 0;
  private sv!: HTMLCanvasElement;
  private hue!: HTMLCanvasElement;
  private hex!: HTMLInputElement;
  private paletteEl!: HTMLElement;
  private recentEl!: HTMLElement;
  private svKnob!: HTMLElement;
  private hueKnob!: HTMLElement;
  private onColor = () => this.sync();
  private onBoard = () => this.renderSwatches();

  constructor(private ed: Editor, parent: HTMLElement) {
    this.el = parent.createDiv("sb-color");
    const svWrap = this.el.createDiv("sb-color-sv");
    this.sv = svWrap.createEl("canvas", { attr: { width: "200", height: "140" } });
    this.svKnob = svWrap.createDiv("sb-color-knob");
    const hueWrap = this.el.createDiv("sb-color-hue");
    this.hue = hueWrap.createEl("canvas", { attr: { width: "200", height: "12" } });
    this.hueKnob = hueWrap.createDiv("sb-color-hue-knob");
    this.drawHue();
    const row = this.el.createDiv("sb-color-row");
    this.hex = row.createEl("input", { type: "text", cls: "sb-color-hex", attr: { spellcheck: "false", maxlength: "7" } });
    this.hex.addEventListener("change", () => {
      const c = normalizeHex(this.hex.value);
      if (c) this.ed.setColor(c);
      else this.sync();
    });
    const add = row.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Add to the palette" } });
    setIcon(add, "plus");
    add.addEventListener("click", () => {
      const p = this.ed.board.palette;
      if (!p.includes(this.ed.color)) {
        this.ed.mutate("Add colour to palette", (b) => b.palette.push(this.ed.color));
        this.renderSwatches();
      }
    });
    this.el.createDiv({ cls: "sb-color-label", text: "Palette (right-click removes)" });
    this.paletteEl = this.el.createDiv("sb-swatches");
    this.el.createDiv({ cls: "sb-color-label", text: "Recent" });
    this.recentEl = this.el.createDiv("sb-swatches");
    this.drag(this.sv, (x, y) => {
      this.s = x;
      this.v = 1 - y;
      this.emit();
    });
    this.drag(this.hue, (x) => {
      this.h = x * 359.9;
      this.drawSv();
      this.emit();
    });
    ed.on("color", this.onColor);
    ed.on("board", this.onBoard);
    [this.h, this.s, this.v] = hexToHsv(ed.color);
    this.drawSv();
    this.sync();
    this.renderSwatches();
  }

  destroy() {
    this.ed.off("color", this.onColor);
    this.ed.off("board", this.onBoard);
    this.el.remove();
  }

  private drag(el: HTMLElement, cb: (x: number, y: number) => void) {
    const at = (e: PointerEvent) => {
      const r = el.getBoundingClientRect();
      cb(Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)), Math.max(0, Math.min(1, (e.clientY - r.top) / r.height)));
    };
    el.addEventListener("pointerdown", (e) => {
      try { el.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
      at(e);
    });
    el.addEventListener("pointermove", (e) => {
      if (e.buttons) at(e);
    });
    el.addEventListener("pointerup", () => this.ed.setColor(this.ed.color));
  }

  private emitting = false;
  private emit() {
    this.emitting = true;
    this.ed.setColor(hsvToHex(this.h, this.s, this.v), false);
    this.emitting = false;
    this.place();
  }

  private sync() {
    if (!this.emitting) {
      const [h, s, v] = hexToHsv(this.ed.color);
      // Greys have no hue: keep the strip where it was.
      if (s > 0.001 && v > 0.001) this.h = h;
      this.s = s;
      this.v = v;
      this.drawSv();
      this.place();
    }
    this.hex.value = this.ed.color;
    this.renderRecent();
  }

  private place() {
    this.svKnob.style.left = `${this.s * 100}%`;
    this.svKnob.style.top = `${(1 - this.v) * 100}%`;
    this.svKnob.style.background = this.ed.color;
    this.hueKnob.style.left = `${(this.h / 360) * 100}%`;
  }

  private drawHue() {
    const ctx = this.hue.getContext("2d")!;
    const g = ctx.createLinearGradient(0, 0, this.hue.width, 0);
    for (let i = 0; i <= 6; i++) g.addColorStop(i / 6, hsvToHex((i * 60) % 360, 1, 1));
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, this.hue.width, this.hue.height);
  }

  private drawSv() {
    const ctx = this.sv.getContext("2d")!;
    const w = this.sv.width, h = this.sv.height;
    ctx.fillStyle = hsvToHex(this.h, 1, 1);
    ctx.fillRect(0, 0, w, h);
    const white = ctx.createLinearGradient(0, 0, w, 0);
    white.addColorStop(0, "#fff");
    white.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = white;
    ctx.fillRect(0, 0, w, h);
    const black = ctx.createLinearGradient(0, 0, 0, h);
    black.addColorStop(0, "rgba(0,0,0,0)");
    black.addColorStop(1, "#000");
    ctx.fillStyle = black;
    ctx.fillRect(0, 0, w, h);
  }

  private swatch(parent: HTMLElement, c: string, onRemove?: () => void) {
    const s = parent.createDiv({ cls: "sb-swatch", attr: { "aria-label": c } });
    s.style.background = c;
    s.toggleClass("is-active", c === this.ed.color);
    s.addEventListener("click", () => this.ed.setColor(c));
    if (onRemove)
      s.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        onRemove();
      });
  }

  private renderSwatches() {
    this.paletteEl.empty();
    for (const c of this.ed.board.palette) {
      this.swatch(this.paletteEl, c, () => {
        this.ed.mutate("Remove colour from palette", (b) => (b.palette = b.palette.filter((x) => x !== c)));
        this.renderSwatches();
      });
    }
  }

  private renderRecent() {
    this.recentEl.empty();
    for (const c of this.ed.board.recent) this.swatch(this.recentEl, c);
  }
}
