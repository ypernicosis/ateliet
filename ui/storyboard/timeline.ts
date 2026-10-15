// The timeline: frames as clips whose width is their duration, a time ruler,
// playback at the storyboard's frame rate, onion skin, and the notes of the
// selected frame (shot, camera, action, dialogue, sound), which is what a
// film storyboard is read by. For animation, frames are drawings held for a
// number of frames; for film, shots held for seconds. Same thing.

import { Editor } from "./editor.ts";
import { Frame, Layer, frameStarts, newLayer, timecode, totalFrames, uid } from "./model.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Menu, Modal, Notice, Setting } from "../obsidian/ui.ts";

export const SHOTS = ["", "Extreme wide", "Wide", "Full", "Medium wide", "Medium", "Medium close-up", "Close-up", "Extreme close-up", "Over the shoulder", "Point of view", "Insert", "Two shot", "Establishing"];
export const CAMERA = ["", "Static", "Pan left", "Pan right", "Tilt up", "Tilt down", "Dolly in", "Dolly out", "Truck left", "Truck right", "Crane up", "Crane down", "Zoom in", "Zoom out", "Handheld", "Tracking", "Whip pan", "Rack focus"];
const LABELS = ["", "#e5534b", "#f0883e", "#d4a72c", "#57ab5a", "#539bf5", "#986ee2", "#e275ad"];

export class Timeline {
  el: HTMLElement;
  private track!: HTMLElement;
  private ruler!: HTMLCanvasElement;
  private playhead!: HTMLElement;
  private timeEl!: HTMLElement;
  private playBtn!: HTMLElement;
  private onionBtn!: HTMLElement;
  private notesEl!: HTMLElement;
  private scroller!: HTMLElement;
  private pxPerFrame = 0;
  private urls = new Map<string, string>();
  private loop = true;
  private time = 0;
  private raf = 0;
  private pictures = new Map<string, { pic: HTMLCanvasElement; area: any }>();
  private picturesVersion = -1;
  private version = 0;
  private off: (() => void)[] = [];

  constructor(private app: any, private ed: Editor, parent: HTMLElement) {
    this.el = parent.createDiv("sb-timeline");
    const bar = this.el.createDiv("sb-timeline-bar");
    const btn = (icon: string, label: string, fn: (e: MouseEvent) => void) => {
      const b = bar.createDiv({ cls: "clickable-icon", attr: { "aria-label": label } });
      setIcon(b, icon);
      b.addEventListener("click", fn);
      return b;
    };
    btn("skip-back", "First frame (Home)", () => this.go(0));
    btn("step-back", "Previous frame (↑ or ,)", () => this.go(this.ed.frameIndex - 1));
    this.playBtn = btn("play", "Play / pause (Enter)", () => this.toggle());
    btn("step-forward", "Next frame (↓ or .)", () => this.go(this.ed.frameIndex + 1));
    btn("skip-forward", "Last frame (End)", () => this.go(this.ed.board.frames.length - 1));
    const loopBtn = btn("repeat", "Loop", () => {
      this.loop = !this.loop;
      loopBtn.toggleClass("is-active", this.loop);
    });
    loopBtn.addClass("is-active");
    this.timeEl = bar.createDiv("sb-timecode");
    const fps = bar.createEl("select", { cls: "dropdown sb-fps", attr: { "aria-label": "Frames per second" } });
    for (const v of [8, 12, 15, 24, 25, 30, 48, 50, 60]) fps.createEl("option", { value: String(v), text: `${v} fps` });
    fps.value = String(this.ed.board.fps);
    if (!fps.value) {
      fps.createEl("option", { value: String(this.ed.board.fps), text: `${this.ed.board.fps} fps` });
      fps.value = String(this.ed.board.fps);
    }
    fps.addEventListener("change", () => this.ed.mutate("Frame rate", (b) => (b.fps = Number(fps.value))));
    bar.createDiv("sb-spacer");
    this.onionBtn = btn("layers-3", "Onion skin (O): see the frames before and after", () => this.toggleOnion());
    btn("settings-2", "Onion skin settings", (e) => this.onionMenu(e));
    btn("plus", "New frame (Ctrl/Cmd+Enter)", () => this.ed.newFrame());
    btn("copy-plus", "Duplicate frame", () => this.duplicate(this.ed.frame));
    const zoom = bar.createEl("input", { type: "range", cls: "sb-timeline-zoom", attr: { min: "0", max: "100", value: "0", "aria-label": "Timeline zoom" } });
    zoom.addEventListener("input", () => {
      const v = Number(zoom.value);
      this.pxPerFrame = v === 0 ? 0 : 2 + (v / 100) ** 2 * 60;
      this.render();
    });

    const body = this.el.createDiv("sb-timeline-body");
    this.scroller = body.createDiv("sb-timeline-scroll");
    this.ruler = this.scroller.createEl("canvas", { cls: "sb-ruler" });
    this.track = this.scroller.createDiv("sb-track");
    this.playhead = this.scroller.createDiv("sb-playhead");
    this.ruler.addEventListener("pointerdown", (e) => this.scrub(e));
    this.ruler.addEventListener("pointermove", (e) => e.buttons && this.scrub(e));
    this.notesEl = body.createDiv("sb-notes");
    new ResizeObserver(() => requestAnimationFrame(() => this.render())).observe(this.scroller);

    const onFrame = () => {
      if (!this.raf) this.time = frameStarts(this.ed.board)[this.ed.frameIndex] || 0;
      this.render();
      this.renderNotes();
    };
    const onBoard = () => {
      this.version++;
      this.render();
      this.renderNotes();
      fps.value = String(this.ed.board.fps);
    };
    const onThumb = (f: Frame) => {
      const u = this.urls.get(f.id);
      if (u) URL.revokeObjectURL(u);
      this.urls.delete(f.id);
      this.version++;
      this.render();
    };
    const onDirty = () => this.version++;
    this.ed.on("frame", onFrame);
    this.ed.on("board", onBoard);
    this.ed.on("thumb", onThumb);
    this.ed.on("dirty", onDirty);
    this.off.push(() => this.ed.off("frame", onFrame), () => this.ed.off("board", onBoard), () => this.ed.off("thumb", onThumb), () => this.ed.off("dirty", onDirty));
    this.render();
    this.renderNotes();
  }

  destroy() {
    this.stop();
    this.off.forEach((f) => f());
    for (const u of this.urls.values()) URL.revokeObjectURL(u);
  }

  private thumb(f: Frame): string | null {
    let u = this.urls.get(f.id);
    if (u) return u;
    const bytes = this.ed.doc.thumbs.get(f.id);
    if (!bytes) return null;
    u = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "image/png" }));
    this.urls.set(f.id, u);
    return u;
  }

  private scale(): number {
    const total = Math.max(1, totalFrames(this.ed.board));
    const fit = Math.max(2, (this.scroller.clientWidth - 24) / total);
    return this.pxPerFrame || Math.min(fit, 40);
  }

  render() {
    const b = this.ed.board;
    if (!this.ed.frame) return;
    const k = this.scale();
    const starts = frameStarts(b);
    const total = totalFrames(b);
    const width = Math.max(this.scroller.clientWidth, total * k + 80);
    this.track.style.width = `${width}px`;
    this.track.empty();
    b.frames.forEach((f, i) => {
      const clip = this.track.createDiv({ cls: "sb-clip", attr: { draggable: "true" } });
      clip.style.left = `${starts[i] * k}px`;
      clip.style.width = `${Math.max(8, f.duration * k - 2)}px`;
      clip.toggleClass("is-active", f === this.ed.frame);
      if (f.color) clip.style.setProperty("--sb-label", f.color);
      const url = this.thumb(f);
      if (url) clip.createEl("img", { attr: { src: url, draggable: "false" } });
      const info = clip.createDiv("sb-clip-info");
      info.createSpan({ cls: "sb-clip-num", text: String(i + 1) });
      if (f.name) info.createSpan({ cls: "sb-clip-name", text: f.name });
      info.createSpan({ cls: "sb-clip-dur", text: `${(f.duration / b.fps).toFixed(f.duration % b.fps ? 2 : 0)}s` });
      clip.addEventListener("click", () => this.ed.setFrame(f));
      clip.addEventListener("dblclick", () => this.rename(f));
      clip.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        this.frameMenu(f).showAtMouseEvent(e);
      });
      // Reordering by dragging a clip onto another.
      clip.addEventListener("dragstart", (e) => e.dataTransfer?.setData("text/x-sb-frame", f.id));
      clip.addEventListener("dragover", (e) => {
        if (e.dataTransfer?.types.includes("text/x-sb-frame")) e.preventDefault();
      });
      clip.addEventListener("drop", (e) => {
        const id = e.dataTransfer?.getData("text/x-sb-frame");
        if (!id || id === f.id) return;
        e.preventDefault();
        const r = clip.getBoundingClientRect();
        this.move(id, f.id, e.clientX > r.left + r.width / 2);
      });
      // The right edge sets the duration.
      const edge = clip.createDiv({ cls: "sb-clip-edge", attr: { "aria-label": "Drag to change how long the frame lasts" } });
      edge.addEventListener("pointerdown", (e) => this.resize(e, f, edge));
      edge.addEventListener("click", (e) => e.stopPropagation());
    });
    this.drawRuler(width, k);
    this.placePlayhead();
    this.playBtn && setIcon(this.playBtn, this.raf ? "pause" : "play");
    this.onionBtn.toggleClass("is-active", b.onion.enabled);
  }

  private drawRuler(width: number, k: number) {
    const dpr = window.devicePixelRatio || 1;
    this.ruler.width = Math.round(width * dpr);
    this.ruler.height = Math.round(18 * dpr);
    this.ruler.style.width = `${width}px`;
    const ctx = this.ruler.getContext("2d")!;
    ctx.scale(dpr, dpr);
    const fps = this.ed.board.fps;
    const color = getComputedStyle(this.el).getPropertyValue("--text-faint") || "#888";
    ctx.fillStyle = ctx.strokeStyle = color;
    ctx.font = "10px var(--font-interface, sans-serif)";
    const secPx = fps * k;
    const every = secPx > 120 ? 1 : secPx > 40 ? 2 : secPx > 15 ? 5 : 10;
    for (let s = 0; s * secPx < width; s++) {
      const x = s * secPx + 0.5;
      ctx.beginPath();
      ctx.moveTo(x, s % every ? 12 : 6);
      ctx.lineTo(x, 18);
      ctx.stroke();
      if (s % every === 0) ctx.fillText(`${s}s`, x + 3, 10);
      if (k >= 6) for (let f = 1; f < fps; f++) {
        const fx = x + f * k;
        ctx.beginPath();
        ctx.moveTo(fx, 15);
        ctx.lineTo(fx, 18);
        ctx.stroke();
      }
    }
  }

  private placePlayhead() {
    const k = this.scale();
    this.playhead.style.left = `${this.time * k}px`;
    this.timeEl.setText(`${timecode(this.time, this.ed.board.fps)} / ${timecode(totalFrames(this.ed.board), this.ed.board.fps)}`);
  }

  private scrub(e: PointerEvent) {
    const r = this.track.getBoundingClientRect();
    const t = Math.max(0, (e.clientX - r.left) / this.scale());
    this.time = Math.min(t, totalFrames(this.ed.board) - 0.001);
    const i = this.indexAt(this.time);
    this.placePlayhead();
    if (this.ed.frameIndex !== i) this.ed.setFrame(i);
  }

  private indexAt(t: number): number {
    const starts = frameStarts(this.ed.board);
    let i = 0;
    while (i + 1 < starts.length && starts[i + 1] <= t) i++;
    return i;
  }

  go(i: number) {
    const n = this.ed.board.frames.length;
    if (!n) return;
    this.ed.setFrame(Math.max(0, Math.min(n - 1, i)));
  }

  // ── playback ────────────────────────────────────────────────────────────

  get isPlaying() {
    return !!this.raf;
  }

  async toggle() {
    if (this.raf) return this.stop();
    await this.play();
  }

  /** Renders every frame once (until something changes), then plays. */
  async play() {
    const ed = this.ed;
    ed.commitFloating();
    if (this.picturesVersion !== this.version) {
      this.pictures.clear();
      const notice = new Notice("Preparing playback…", 0);
      try {
        const dpr = window.devicePixelRatio || 1;
        for (const [i, f] of ed.board.frames.entries()) {
          const area = ed.comp.area(f, false);
          const scale = Math.min(1, ed.view.scale * dpr, 2048 / Math.max(area.w, area.h));
          const pic = await ed.comp.picture(f, scale, { exporting: false, area });
          this.pictures.set(f.id, { pic, area });
          notice.setMessage(`Preparing playback… ${i + 1}/${ed.board.frames.length}`);
        }
      } finally {
        notice.hide();
      }
      this.picturesVersion = this.version;
    }
    const total = totalFrames(ed.board);
    if (this.time >= total - 0.01) this.time = 0;
    let last = performance.now();
    let shown = "";
    const tick = (now: number) => {
      const dt = (now - last) / 1000;
      last = now;
      this.time += dt * ed.board.fps;
      if (this.time >= total) {
        if (!this.loop) {
          this.time = total - 0.001;
          this.stop();
          return;
        }
        this.time %= total;
      }
      const f = ed.board.frames[this.indexAt(this.time)];
      if (f && f.id !== shown) {
        shown = f.id;
        ed.playing = this.pictures.get(f.id) || null;
        ed.frame = f;
        ed.requestRender();
        this.track.querySelectorAll(".sb-clip").forEach((c, i) => c.toggleClass("is-active", ed.board.frames[i] === f));
      }
      this.placePlayhead();
      this.raf = requestAnimationFrame(tick);
    };
    this.raf = requestAnimationFrame(tick);
    setIcon(this.playBtn, "pause");
  }

  stop() {
    if (!this.raf) return;
    cancelAnimationFrame(this.raf);
    this.raf = 0;
    const ed = this.ed;
    ed.playing = null;
    const f = ed.frame;
    ed.frame = null as any;
    ed.setFrame(f);
    setIcon(this.playBtn, "play");
    this.render();
  }

  // ── onion skin ──────────────────────────────────────────────────────────

  toggleOnion() {
    this.ed.board.onion.enabled = !this.ed.board.onion.enabled;
    this.ed.changed();
    this.ed.refreshFrame();
  }

  private onionMenu(e: MouseEvent) {
    const o = this.ed.board.onion;
    const m = new Menu();
    for (const n of [1, 2, 3]) m.addItem((i) => i.setTitle(`${n} before`).setChecked(o.before === n).onClick(() => this.setOnion({ before: n })));
    m.addItem((i) => i.setTitle("None before").setChecked(o.before === 0).onClick(() => this.setOnion({ before: 0 })));
    m.addSeparator();
    for (const n of [1, 2, 3]) m.addItem((i) => i.setTitle(`${n} after`).setChecked(o.after === n).onClick(() => this.setOnion({ after: n })));
    m.addItem((i) => i.setTitle("None after").setChecked(o.after === 0).onClick(() => this.setOnion({ after: 0 })));
    m.addSeparator();
    for (const v of [0.15, 0.3, 0.5]) m.addItem((i) => i.setTitle(`Opacity ${Math.round(v * 100)}%`).setChecked(Math.abs(o.opacity - v) < 0.01).onClick(() => this.setOnion({ opacity: v })));
    m.addItem((i) => i.setTitle("Tint (earlier red, later green)").setChecked(o.tint).onClick(() => this.setOnion({ tint: !o.tint })));
    m.showAtMouseEvent(e);
  }

  private setOnion(p: Partial<typeof this.ed.board.onion>) {
    Object.assign(this.ed.board.onion, p, { enabled: true });
    this.ed.changed();
    this.ed.refreshFrame();
  }

  // ── editing frames ──────────────────────────────────────────────────────

  private resize(e: PointerEvent, f: Frame, edge: HTMLElement) {
    e.stopPropagation();
    e.preventDefault();
    try { edge.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
    const k = this.scale();
    const x0 = e.clientX;
    const d0 = f.duration;
    const clip = edge.parentElement!;
    const move = (ev: PointerEvent) => {
      const d = Math.max(1, Math.round(d0 + (ev.clientX - x0) / k));
      clip.style.width = `${Math.max(8, d * k - 2)}px`;
      clip.querySelector(".sb-clip-dur")?.setText(`${(d / this.ed.board.fps).toFixed(2)}s · ${d}f`);
    };
    const up = (ev: PointerEvent) => {
      edge.removeEventListener("pointermove", move);
      edge.removeEventListener("pointerup", up);
      const d = Math.max(1, Math.round(d0 + (ev.clientX - x0) / k));
      if (d !== d0) this.ed.mutate("Frame duration", () => (f.duration = d));
      else this.render();
    };
    edge.addEventListener("pointermove", move);
    edge.addEventListener("pointerup", up);
  }

  private move(id: string, target: string, after: boolean) {
    this.ed.mutate("Reorder frames", (b) => {
      const f = b.frames.find((x) => x.id === id)!;
      b.frames.splice(b.frames.indexOf(f), 1);
      const t = b.frames.findIndex((x) => x.id === target);
      b.frames.splice(after ? t + 1 : t, 0, f);
    });
    this.ed.refreshFrame();
  }

  private rename(f: Frame) {
    const m = new Modal(this.app);
    m.setTitle("Frame name");
    let v = f.name;
    new Setting(m.contentEl).addText((t) => {
      t.setPlaceholder(this.ed.frameName(f)).setValue(f.name).onChange((x) => (v = x));
      t.inputEl.style.width = "100%";
      t.inputEl.addEventListener("keydown", (e) => e.key === "Enter" && m.close());
      setTimeout(() => t.inputEl.select(), 0);
    });
    m.onClose = () => {
      if (v.trim() !== f.name) this.ed.mutate("Rename frame", () => (f.name = v.trim()));
    };
    m.open();
  }

  async duplicate(f: Frame) {
    const ed = this.ed;
    const copies: [string, Layer][] = [];
    for (const id of f.layers) {
      const l = ed.board.layers[id];
      if (!l || l.shared) continue;
      const c: Layer = { ...JSON.parse(JSON.stringify(l)), id: newLayer("").id };
      if (l.kind === "raster") await ed.doc.tiles.copyLayer(l.id, c.id);
      copies.push([id, c]);
    }
    let created: Frame | null = null;
    ed.mutate("Duplicate frame", (b) => {
      const map = new Map(copies.map(([from, c]) => [from, c.id]));
      for (const [, c] of copies) b.layers[c.id] = c;
      created = { ...JSON.parse(JSON.stringify(f)), id: uid(), layers: f.layers.map((id) => map.get(id) || id), active: map.get(f.active) || f.active };
      b.frames.splice(b.frames.indexOf(f) + 1, 0, created!);
    });
    const thumb = ed.doc.thumbs.get(f.id);
    if (thumb) ed.doc.thumbs.set(created!.id, thumb);
    await ed.setFrame(created!);
    ed.changed(true);
  }

  remove(f: Frame) {
    const ed = this.ed;
    if (ed.board.frames.length === 1) return new Notice("A storyboard keeps at least one frame.");
    const i = ed.board.frames.indexOf(f);
    ed.mutate("Delete frame", (b) => {
      b.frames.splice(i, 1);
    });
    ed.setFrame(Math.min(i, ed.board.frames.length - 1));
    ed.changed();
  }

  private setDuration(f: Frame) {
    const m = new Modal(this.app);
    m.setTitle("Duration");
    let secs = f.duration / this.ed.board.fps;
    let frames = f.duration;
    const s = new Setting(m.contentEl).setName("Seconds").addText((t) => t.setValue(secs.toFixed(2)).onChange((v) => {
      secs = Number(v.replace(",", "."));
      frames = Math.max(1, Math.round(secs * this.ed.board.fps));
    }));
    new Setting(m.contentEl).setName("Or frames").addText((t) => t.setValue(String(frames)).onChange((v) => (frames = Math.max(1, Math.round(Number(v)) || 1))));
    void s;
    const buttons = m.modalEl.createDiv("modal-button-container");
    buttons.createEl("button", { text: "OK", cls: "mod-cta" }).addEventListener("click", () => {
      if (frames !== f.duration) this.ed.mutate("Frame duration", () => (f.duration = frames));
      m.close();
    });
    m.open();
  }

  frameMenu(f: Frame): Menu {
    const ed = this.ed;
    const m = new Menu();
    m.addItem((i) => i.setTitle("New frame after").setIcon("plus").onClick(async () => {
      await ed.setFrame(f);
      ed.newFrame(true);
    }));
    m.addItem((i) => i.setTitle("New frame before").setIcon("plus").onClick(async () => {
      await ed.setFrame(f);
      ed.newFrame(false);
    }));
    m.addItem((i) => i.setTitle("Duplicate").setIcon("copy-plus").onClick(() => this.duplicate(f)));
    m.addItem((i) => i.setTitle("Rename…").setIcon("pencil").onClick(() => this.rename(f)));
    m.addItem((i) => i.setTitle("Duration…").setIcon("timer").onClick(() => this.setDuration(f)));
    m.addItem((i) => {
      i.setTitle("Colour label").setIcon("tag");
      const sm = i.setSubmenu();
      const names: Record<string, string> = { "": "None", "#e5534b": "Red", "#f0883e": "Orange", "#d4a72c": "Yellow", "#57ab5a": "Green", "#539bf5": "Blue", "#986ee2": "Purple", "#e275ad": "Pink" };
      for (const c of LABELS) sm.addItem((j) => j.setTitle(names[c]).setChecked((f.color || "") === c).onClick(() => ed.mutate("Frame label", () => (c ? (f.color = c) : delete f.color))));
    });
    m.addSeparator();
    m.addItem((i) => i.setTitle("Delete frame").setIcon("trash-2").setWarning(true).onClick(() => this.remove(f)));
    return m;
  }

  // ── notes ───────────────────────────────────────────────────────────────

  renderNotes() {
    const ed = this.ed;
    const f = ed.frame;
    const el = this.notesEl;
    if (!f) return;
    // Keep what is being typed when the frame did not change.
    if (el.dataset.frame === f.id && el.contains(document.activeElement)) return;
    el.dataset.frame = f.id;
    el.empty();
    const head = el.createDiv("sb-notes-head");
    head.createSpan({ cls: "sb-notes-title", text: ed.frameName(f) });
    head.createSpan({ cls: "sb-notes-dur", text: `${(f.duration / ed.board.fps).toFixed(2)} s · ${f.duration} frames` });
    const grid = el.createDiv("sb-notes-grid");
    const select = (label: string, values: string[], key: "shot" | "camera") => {
      const wrap = grid.createDiv("sb-note");
      wrap.createDiv({ cls: "sb-note-label", text: label });
      const s = wrap.createEl("select", { cls: "dropdown" });
      const all = values.includes(f.notes[key]) ? values : [...values, f.notes[key]];
      for (const v of all) s.createEl("option", { value: v, text: v || "—" });
      s.value = f.notes[key];
      s.addEventListener("change", () => ed.mutate(label, () => (f.notes[key] = s.value)));
    };
    select("Shot", SHOTS, "shot");
    select("Camera", CAMERA, "camera");
    const text = (label: string, key: "action" | "dialogue" | "sound", rows: number) => {
      const wrap = el.createDiv("sb-note");
      wrap.createDiv({ cls: "sb-note-label", text: label });
      const t = wrap.createEl("textarea", { attr: { rows: String(rows), placeholder: label } });
      t.value = f.notes[key];
      t.addEventListener("keydown", (e) => e.stopPropagation());
      t.addEventListener("change", () => ed.mutate(label, () => (f.notes[key] = t.value)));
    };
    text("Action", "action", 2);
    text("Dialogue", "dialogue", 2);
    text("Sound", "sound", 1);
  }
}
