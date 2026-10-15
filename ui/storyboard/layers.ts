// Layers of the frame on screen, top first. Each can be hidden, locked,
// clipped to the layer below, alpha-locked, shared by every frame, renamed,
// duplicated, merged down, reordered by dragging. Under the list: opacity
// and blend mode of the active layer, and for a trace layer how the picture
// is shown.

import { Editor } from "./editor.ts";
import { BLEND_MODES, Layer, newLayer } from "./model.ts";
import { Scratch, corners, pixelBounds, sourceSize } from "./render.ts";
import { TRACE_MODES, defaultTrace } from "./trace.ts";
import { fitTransform } from "./tools.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Menu, Notice } from "../obsidian/ui.ts";

export class LayersPanel {
  el: HTMLElement;
  private list!: HTMLElement;
  private props!: HTMLElement;
  private dragging: string | null = null;
  private onBoard = () => this.render();

  constructor(private ed: Editor, parent: HTMLElement, private hooks: { chooseReference: (cb: (refId: string) => void) => void; trace: () => void }) {
    this.el = parent.createDiv("sb-layers");
    const head = this.el.createDiv("sb-panel-head");
    head.createDiv({ cls: "sb-panel-title", text: "Layers" });
    const add = (icon: string, label: string, fn: () => void) => {
      const b = head.createDiv({ cls: "clickable-icon", attr: { "aria-label": label } });
      setIcon(b, icon);
      b.addEventListener("click", fn);
    };
    add("plus", "New drawing layer (Ctrl/Cmd+Shift+N)", () => this.ed.addLayer("raster"));
    add("scan-line", "Trace a reference (adds a trace layer and a layer to draw on)", () => this.hooks.trace());
    add("image-plus", "Place a reference as a picture layer", () => this.hooks.chooseReference((id) => this.placePicture(id)));
    add("more-horizontal", "More", (e?: any) => this.layerMenu(this.ed.active!).showAtMouseEvent(e || window.event));
    this.list = this.el.createDiv("sb-layer-list");
    this.props = this.el.createDiv("sb-layer-props");
    ed.on("board", this.onBoard);
    ed.on("frame", this.onBoard);
    this.render();
  }

  destroy() {
    this.ed.off("board", this.onBoard);
    this.ed.off("frame", this.onBoard);
  }

  render() {
    const ed = this.ed;
    if (!ed.frame) return;
    this.list.empty();
    const layers = [...ed.layers].reverse();
    for (const l of layers) {
      const row = this.list.createDiv({ cls: "sb-layer", attr: { draggable: "true" } });
      row.toggleClass("is-active", l.id === ed.frame.active);
      row.toggleClass("is-hidden", !l.visible);
      row.toggleClass("is-clipped", l.clip);
      const eye = row.createDiv({ cls: "sb-layer-eye clickable-icon", attr: { "aria-label": l.visible ? "Hide" : "Show" } });
      setIcon(eye, l.visible ? "eye" : "eye-off");
      eye.addEventListener("click", (e) => {
        e.stopPropagation();
        ed.mutate(l.visible ? "Hide layer" : "Show layer", () => (l.visible = !l.visible));
      });
      const kind = row.createDiv("sb-layer-kind");
      setIcon(kind, l.kind === "trace" ? "scan-line" : l.kind === "image" ? "image" : l.clip ? "corner-left-down" : "pen-line");
      const name = row.createDiv({ cls: "sb-layer-name", text: l.name });
      const badges = row.createDiv("sb-layer-badges");
      const badge = (icon: string, label: string) => setIcon(badges.createSpan({ cls: "sb-badge", attr: { "aria-label": label } }), icon);
      if (l.shared) badge("layers", "In every frame");
      if (l.alphaLock) badge("droplet", "Transparency locked");
      if (l.locked) badge("lock", "Locked");
      if (l.blend !== "source-over") badges.createSpan({ cls: "sb-badge-text", text: BLEND_MODES.find((b) => b[0] === l.blend)?.[1] || l.blend });
      if (l.opacity < 1) badges.createSpan({ cls: "sb-badge-text", text: `${Math.round(l.opacity * 100)}%` });
      row.addEventListener("click", () => ed.setActive(l.id));
      name.addEventListener("dblclick", () => this.rename(l, name));
      row.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        ed.setActive(l.id);
        this.layerMenu(l).showAtMouseEvent(e);
      });
      row.addEventListener("dragstart", (e) => {
        this.dragging = l.id;
        e.dataTransfer?.setData("text/x-sb-layer", l.id);
      });
      row.addEventListener("dragover", (e) => {
        if (!this.dragging) return;
        e.preventDefault();
        const r = row.getBoundingClientRect();
        row.toggleClass("drop-above", e.clientY < r.top + r.height / 2);
        row.toggleClass("drop-below", e.clientY >= r.top + r.height / 2);
      });
      row.addEventListener("dragleave", () => row.removeClass("drop-above", "drop-below"));
      row.addEventListener("drop", (e) => {
        e.preventDefault();
        const r = row.getBoundingClientRect();
        const above = e.clientY < r.top + r.height / 2;
        row.removeClass("drop-above", "drop-below");
        if (this.dragging && this.dragging !== l.id) this.reorder(this.dragging, l.id, above);
        this.dragging = null;
      });
    }
    this.renderProps();
  }

  private rename(l: Layer, el: HTMLElement) {
    const input = createEl("input", { type: "text", value: l.name, cls: "sb-rename" }) as HTMLInputElement;
    el.empty();
    el.appendChild(input);
    input.focus();
    input.select();
    const done = (save: boolean) => {
      const v = input.value.trim();
      if (save && v && v !== l.name) this.ed.mutate("Rename layer", () => (l.name = v));
      else this.render();
    };
    input.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") done(true);
      if (e.key === "Escape") done(false);
    });
    input.addEventListener("blur", () => done(true));
  }

  /** Moves layer `id` above or below `target` (in the frame's bottom-to-top order). */
  private reorder(id: string, target: string, above: boolean) {
    this.ed.mutate("Reorder layers", () => {
      const frames = this.ed.board.layers[id].shared ? this.ed.board.frames : [this.ed.frame];
      for (const f of frames) {
        if (!f.layers.includes(id) || !f.layers.includes(target)) continue;
        f.layers.splice(f.layers.indexOf(id), 1);
        const t = f.layers.indexOf(target);
        f.layers.splice(above ? t + 1 : t, 0, id);
      }
    });
  }

  layerMenu(l: Layer): Menu {
    const ed = this.ed;
    const m = new Menu();
    if (!l) return m;
    m.addItem((i) => i.setTitle("Rename…").setIcon("pencil").onClick(() => {
      const row = this.list.querySelector(".sb-layer.is-active .sb-layer-name") as HTMLElement;
      if (row) this.rename(l, row);
    }));
    m.addItem((i) => i.setTitle("Duplicate").setIcon("copy").onClick(() => this.duplicate(l)));
    m.addItem((i) => i.setTitle("Merge down").setIcon("merge").setDisabled(!this.below(l)).onClick(() => this.mergeDown(l)));
    m.addSeparator();
    m.addItem((i) => i.setTitle("Clip to the layer below").setIcon("corner-left-down").setChecked(l.clip).onClick(() => ed.mutate("Clip layer", () => (l.clip = !l.clip))));
    if (l.kind === "raster") m.addItem((i) => i.setTitle("Lock transparency").setIcon("droplet").setChecked(!!l.alphaLock).onClick(() => ed.mutate("Lock transparency", () => (l.alphaLock = !l.alphaLock))));
    m.addItem((i) => i.setTitle("Lock").setIcon("lock").setChecked(l.locked).onClick(() => ed.mutate(l.locked ? "Unlock layer" : "Lock layer", () => (l.locked = !l.locked))));
    m.addItem((i) => i.setTitle(l.shared ? "In every frame (turn off: keep only in this frame)" : "Show in every frame").setIcon("layers").setChecked(!!l.shared).onClick(() => this.toggleShared(l)));
    if (l.kind === "raster") {
      m.addSeparator();
      m.addItem((i) => i.setTitle("Move to the frame's centre").setIcon("crosshair").onClick(() => this.center(l)));
      m.addItem((i) => i.setTitle("Clear").setIcon("eraser").onClick(() => this.clear(l)));
    }
    if (l.kind !== "raster") {
      m.addSeparator();
      m.addItem((i) => i.setTitle("Fit the picture in the frame").setIcon("minimize-2").onClick(() => this.fit(l, false)));
      m.addItem((i) => i.setTitle("Fill the frame with the picture").setIcon("maximize-2").onClick(() => this.fit(l, true)));
      m.addItem((i) => i.setTitle("Change picture…").setIcon("image").onClick(() => this.hooks.chooseReference((id) => ed.mutate("Change picture", () => {
        l.src = id;
        ed.comp.want(ed.frame);
      }))));
      m.addItem((i) => i.setTitle("Turn into a drawing layer (rasterize)").setIcon("pen-line").onClick(() => this.rasterize(l)));
    }
    m.addSeparator();
    m.addItem((i) => i.setTitle("Delete layer").setIcon("trash-2").setWarning(true).onClick(() => this.remove(l)));
    return m;
  }

  private below(l: Layer): Layer | null {
    const ids = this.ed.frame.layers;
    const i = ids.indexOf(l.id);
    const b = i > 0 ? this.ed.board.layers[ids[i - 1]] : null;
    return b && b.kind === "raster" && !b.locked ? b : null;
  }

  async duplicate(l: Layer) {
    const ed = this.ed;
    const copy: Layer = { ...JSON.parse(JSON.stringify(l)), id: newLayer("").id, name: `${l.name} copy`, shared: false };
    if (l.kind === "raster") await ed.doc.tiles.copyLayer(l.id, copy.id);
    ed.mutate("Duplicate layer", (b) => {
      b.layers[copy.id] = copy;
      ed.frame.layers.splice(ed.frame.layers.indexOf(l.id) + 1, 0, copy.id);
      ed.frame.active = copy.id;
    });
    ed.changed(true);
  }

  remove(l: Layer) {
    const ed = this.ed;
    ed.mutate("Delete layer", (b) => {
      const frames = l.shared ? b.frames : [ed.frame];
      for (const f of frames) {
        f.layers = f.layers.filter((id) => id !== l.id);
        if (!f.layers.length) {
          const n = newLayer("Drawing");
          b.layers[n.id] = n;
          f.layers.push(n.id);
        }
        if (!f.layers.includes(f.active)) f.active = f.layers[f.layers.length - 1];
      }
    });
    ed.changed(true);
  }

  toggleShared(l: Layer) {
    const ed = this.ed;
    ed.mutate(l.shared ? "Keep layer in this frame only" : "Show layer in every frame", (b) => {
      if (l.shared) {
        l.shared = false;
        for (const f of b.frames) if (f !== ed.frame) f.layers = f.layers.filter((id) => id !== l.id);
        for (const f of b.frames) if (!f.layers.length) {
          const n = newLayer("Drawing");
          b.layers[n.id] = n;
          f.layers.push(n.id);
          f.active = n.id;
        }
      } else {
        l.shared = true;
        const at = ed.frame.layers.indexOf(l.id);
        for (const f of b.frames) if (!f.layers.includes(l.id)) f.layers.splice(Math.min(at, f.layers.length), 0, l.id);
      }
    });
    ed.changed(true);
  }

  private async mergeDown(l: Layer) {
    const ed = this.ed;
    const b = this.below(l);
    if (!b) return;
    await ed.doc.tiles.load([l.id, b.id]);
    // The upper layer alone, as a picture of the area it covers.
    let area = l.kind === "raster" ? pixelBounds(ed.doc, l) : null;
    if (l.kind !== "raster" && l.transform && l.src) {
      const size = ed.pictureSize(l.src);
      if (size) {
        const cs = corners(l.transform, size[0], size[1]);
        const xs = cs.map((p) => p.x), ys = cs.map((p) => p.y);
        area = { x: Math.floor(Math.min(...xs)), y: Math.floor(Math.min(...ys)), w: Math.ceil(Math.max(...xs) - Math.min(...xs)) + 1, h: Math.ceil(Math.max(...ys) - Math.min(...ys)) + 1 };
      }
    }
    if (!area) return this.remove(l);
    if (!ed.board.infinite) {
      const x0 = Math.max(0, area.x), y0 = Math.max(0, area.y);
      const x1 = Math.min(ed.board.width, area.x + area.w), y1 = Math.min(ed.board.height, area.y + area.h);
      if (x1 <= x0 || y1 <= y0) return this.remove(l);
      area = { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    }
    const tmp = document.createElement("canvas");
    tmp.width = Math.ceil(area.w);
    tmp.height = Math.ceil(area.h);
    const tctx = tmp.getContext("2d")!;
    tctx.translate(-area.x, -area.y);
    const scratch = new Scratch(tmp.width, tmp.height);
    ed.comp.content(tctx, l, { region: area, background: false }, scratch, tctx.getTransform());
    scratch.release();
    const step = ed.pixels("Merge down", b.id);
    const lx = Math.floor(area.x - b.x), ly = Math.floor(area.y - b.y);
    ed.eachTile(lx, ly, tmp.width, tmp.height, (tx, ty) => step.touch(tx, ty));
    ed.doc.tiles.paint(b.id, lx, ly, tmp.width, tmp.height, (ctx) => {
      ctx.globalAlpha = l.opacity;
      ctx.globalCompositeOperation = l.clip ? "source-atop" : (l.blend as GlobalCompositeOperation);
      ctx.drawImage(tmp, 0, 0);
    });
    step.finish();
    ed.mutate("Merge down", (bd) => {
      ed.frame.layers = ed.frame.layers.filter((id) => id !== l.id);
      ed.frame.active = b.id;
      void bd;
    }, [step]);
    ed.changed(true);
  }

  private async rasterize(l: Layer) {
    const ed = this.ed;
    const n = newLayer(l.name);
    ed.mutate("Add layer", (b) => {
      b.layers[n.id] = n;
      ed.frame.layers.splice(ed.frame.layers.indexOf(l.id), 0, n.id);
    });
    ed.frame.active = l.id;
    await this.mergeDown(l);
  }

  private clear(l: Layer) {
    const ed = this.ed;
    const step = ed.pixels("Clear layer", l.id);
    for (const [tx, ty] of ed.doc.tiles.tilesOf(l.id)) {
      step.touch(tx, ty);
      ed.doc.tiles.restore(l.id, tx, ty, null);
    }
    ed.commitPixels(step);
  }

  private center(l: Layer) {
    const ed = this.ed;
    const r = pixelBounds(ed.doc, l);
    if (!r) return;
    const target = ed.board.infinite ? ed.visibleRegion() : { x: 0, y: 0, w: ed.board.width, h: ed.board.height };
    ed.mutate("Centre layer", () => {
      l.x += Math.round(target.x + target.w / 2 - (r.x + r.w / 2));
      l.y += Math.round(target.y + target.h / 2 - (r.y + r.h / 2));
    });
  }

  private fit(l: Layer, cover: boolean) {
    const size = l.src ? this.ed.pictureSize(l.src) : null;
    if (!size) return;
    this.ed.mutate(cover ? "Fill frame" : "Fit picture", () => (l.transform = fitTransform(this.ed, size[0], size[1], cover)));
  }

  async placePicture(refId: string, kind: "image" | "trace" = "image"): Promise<Layer | null> {
    const ed = this.ed;
    const ref = ed.doc.refs.ref(refId);
    const img = await ed.doc.refs.image(ref);
    if (!img || !ref) {
      new Notice("That picture could not be found.");
      return null;
    }
    ed.comp.images.set(refId, img);
    const [w, h] = sourceSize(img);
    const t = fitTransform(ed, w, h, kind === "trace");
    if (kind === "trace") return ed.addLayer("trace", { name: `Trace · ${ref.name}`, src: refId, transform: t, opacity: 0.5, locked: true, trace: defaultTrace() }, 0);
    return ed.addLayer("image", { name: ref.name, src: refId, transform: t });
  }

  // ── properties of the active layer ──────────────────────────────────────

  private renderProps() {
    const ed = this.ed;
    const l = ed.active;
    this.props.empty();
    if (!l) return;
    const row = (label: string) => {
      const r = this.props.createDiv("sb-prop");
      r.createSpan({ cls: "sb-prop-label", text: label });
      return r;
    };
    // Opacity applies live while dragging; one undo step when released.
    const op = row("Opacity").createEl("input", { type: "range", attr: { min: "0", max: "100", value: String(Math.round(l.opacity * 100)) } });
    const opv = op.parentElement!.createSpan({ cls: "sb-prop-value", text: `${Math.round(l.opacity * 100)}%` });
    let start = l.opacity;
    op.addEventListener("pointerdown", () => (start = l.opacity));
    op.addEventListener("input", () => {
      l.opacity = Number(op.value) / 100;
      opv.setText(`${op.value}%`);
      ed.invalidate();
    });
    op.addEventListener("change", () => {
      const v = l.opacity;
      l.opacity = start;
      ed.mutate("Layer opacity", () => (l.opacity = v));
    });
    const blend = row("Blend").createEl("select", { cls: "dropdown" });
    for (const [v, n] of BLEND_MODES) blend.createEl("option", { value: v, text: n });
    blend.value = l.blend;
    blend.addEventListener("change", () => ed.mutate("Blend mode", () => (l.blend = blend.value)));
    if (l.kind === "trace") this.traceProps(l, row);
  }

  private traceProps(l: Layer, row: (label: string) => HTMLElement) {
    const ed = this.ed;
    const t = (l.trace ||= defaultTrace());
    const mode = row("Show as").createEl("select", { cls: "dropdown" });
    for (const [v, n, d] of TRACE_MODES) mode.createEl("option", { value: v, text: n, attr: { title: d } });
    mode.value = t.mode;
    mode.addEventListener("change", () => ed.mutate("Trace display", () => (t.mode = mode.value as any)));
    if (t.mode === "lines") {
      const s = row("Detail").createEl("input", { type: "range", attr: { min: "5", max: "95", value: String(Math.round(t.threshold * 100)) } });
      s.addEventListener("change", () => ed.mutate("Line detail", () => (t.threshold = Number(s.value) / 100)));
    }
    if (t.mode === "values") {
      const s = row("Tones").createEl("input", { type: "range", attr: { min: "2", max: "6", value: String(t.levels) } });
      s.addEventListener("change", () => ed.mutate("Tones", () => (t.levels = Number(s.value))));
    }
    const toggles = this.props.createDiv("sb-prop-toggles");
    const tog = (label: string, get: () => boolean, set: (v: boolean) => void, title: string) => {
      const lab = toggles.createEl("label", { attr: { title } });
      const cb = lab.createEl("input", { type: "checkbox" });
      cb.checked = get();
      lab.appendText(" " + label);
      cb.addEventListener("change", () => ed.mutate(label, () => set(cb.checked)));
    };
    tog("Over the drawing", () => t.above, (v) => (t.above = v), "A light table: the picture over your lines instead of under them");
    tog("In exports", () => t.export, (v) => (t.export = v), "Off: the trace is only a guide and is left out of exported frames");
    tog("Locked", () => l.locked, (v) => (l.locked = v), "Unlock to move, scale or rotate it with the Move tool (V)");
    this.props.createDiv({ cls: "sb-hint", text: "T shows or hides the trace · hold Q to look without it · Shift+T changes how it shows" });
  }
}
