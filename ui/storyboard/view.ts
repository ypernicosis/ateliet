// The storyboard view: a `.storyboard` file open for drawing.
//
//   ┌ toolbar: undo · tool options · zoom · view · trace · import/export ┐
//   │ tools │            canvas                     │ references        │
//   │       │                                       │ layers            │
//   └ timeline: transport · frames · notes ─────────────────────────────┘

import { Editor, ToolHandler, selectionFrom } from "./editor.ts";
import { BrushKind, BRUSH_NAMES } from "./brush.ts";
import { BoardOptionsModal } from "./dialogs.ts";
import { ColorPanel } from "./color.ts";
import { LayersPanel } from "./layers.ts";
import { ReferencePanel } from "./references.ts";
import { Timeline } from "./timeline.ts";
import { BrushTool, EyedropperTool, FillTool, HandTool, LassoFillTool, MoveTool, SelectTool, ShapeTool, TOOL_INFO, ToolSettings, defaultToolSettings, fitTransform, toolsFor } from "./tools.ts";
import { BalloonTool, PanelTool } from "./pagetools.ts";
import { BALLOON_KINDS, LAYOUTS, layoutPanels, readingOrder, rectPanel } from "./pages.ts";
import { THUMB_ASPECTS, kindInfo, startPanels } from "./kinds.ts";
import { isPageKind } from "./model.ts";
import { Layer, Reference, TraceMode, addFrame, newLayer, uid } from "./model.ts";
import { cloneBoard } from "./history.ts";
import { StoryDoc } from "./store.ts";
import { TRACE_MODES, defaultTrace } from "./trace.ts";
import { addClipLayers, importSummary, readClip } from "./clipfile.ts";
import { animatedPng, boardPdf, canRecordVideo, canvasPng, exportFrames, exportToPictogrep, pagesPdf, recordVideo, saveContactSheet, webtoonSlices, writeVault } from "./export.ts";
import { FileView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Menu, Modal, Notice, Setting } from "../obsidian/ui.ts";
import { TFile } from "../obsidian/vault.ts";
import { currentWindow, fileUrl, invoke } from "../tauri.ts";
import { closePopover } from "./popover.ts";

export const STORYBOARD_VIEW = "storyboard";
const TOOLS_KEY = "ateliet-storyboard-tools";

function loadToolSettings(): ToolSettings {
  const d = defaultToolSettings();
  try {
    const raw = JSON.parse(localStorage.getItem(TOOLS_KEY) || "{}");
    return { ...d, ...raw, brushes: { ...d.brushes, ...(raw.brushes || {}) }, fill: { ...d.fill, ...(raw.fill || {}) }, shape: { ...d.shape, ...(raw.shape || {}) }, select: { ...d.select, ...(raw.select || {}) }, lasso: { ...d.lasso, ...(raw.lasso || {}) }, balloon: { ...d.balloon, ...(raw.balloon || {}) } };
  } catch {
    return d;
  }
}

export class StoryboardView extends FileView {
  ed: Editor | null = null;
  private settings = loadToolSettings();
  private tools = new Map<string, ToolHandler>();
  private toolId = "pencil";
  private root!: HTMLElement;
  private optionsEl!: HTMLElement;
  private toolsEl!: HTMLElement;
  private zoomLabel!: HTMLElement;
  private statusEl!: HTMLElement;
  private undoBtn!: HTMLElement;
  private redoBtn!: HTMLElement;
  private traceBtn!: HTMLElement;
  private colorBtn!: HTMLElement;
  private stage!: HTMLElement;
  private refs: ReferencePanel | null = null;
  private layers: LayersPanel | null = null;
  private timeline: Timeline | null = null;
  private colorPop: { el: HTMLElement; panel: ColorPanel } | null = null;
  private keys = new Set<string>();
  private panning: { x: number; y: number } | null = null;
  private rotating: { a0: number; r0: number } | null = null;
  private toolBeforeAlt: string | null = null;
  private activePointer: number | null = null;
  private clipboard: { canvas: HTMLCanvasElement; x: number; y: number } | null = null;
  private lastTraceMode: TraceMode = "photo";
  private focus: { parent: HTMLElement; next: Node | null; osFullscreen: boolean } | null = null;
  private focusBar!: HTMLElement;
  private focusBtn!: HTMLElement;
  private onionBtn!: HTMLElement;
  private joinBtn: HTMLElement | null = null;
  private side!: HTMLElement;

  getViewType() {
    return STORYBOARD_VIEW;
  }
  getIcon() {
    return "clapperboard";
  }
  canAcceptExtension(ext: string) {
    return ext === "storyboard";
  }

  async onOpen() {
    this.contentEl.addClass("sb-view");
    this.registerDomEvent(document, "keydown", (e: KeyboardEvent) => this.onKey(e), true);
    this.registerDomEvent(document, "keyup", (e: KeyboardEvent) => this.onKeyUp(e), true);
    this.registerDomEvent(window, "blur", () => this.keys.clear());
    this.registerDomEvent(document, "paste", (e: ClipboardEvent) => this.onPaste(e));
  }

  async onLoadFile(file: TFile) {
    this.contentEl.empty();
    const abs = this.app.vault.adapter.getFullPath(file.path);
    let doc: StoryDoc;
    try {
      doc = await StoryDoc.open(abs);
    } catch (e: any) {
      this.contentEl.createDiv({ cls: "sb-error", text: `This storyboard could not be opened: ${e?.message || e}` });
      return;
    }
    this.build(doc);
  }

  async onUnloadFile() {
    await this.close();
  }

  async onRename(file: TFile) {
    if (this.ed) this.ed.doc.abs = this.app.vault.adapter.getFullPath(file.path);
  }

  async onClose() {
    await super.onClose();
    await this.close();
  }

  private async close() {
    const ed = this.ed;
    if (!ed) return;
    if (this.focus) await this.setFocus(false);
    ed.commitFloating();
    await ed.save();
    this.refs?.destroy();
    this.layers?.destroy();
    this.timeline?.destroy();
    this.colorPop?.panel.destroy();
    ed.destroy();
    this.ed = null;
  }

  // ── building the view ───────────────────────────────────────────────────

  private build(doc: StoryDoc) {
    const root = (this.root = this.contentEl.createDiv({ cls: "sb-root", attr: { "data-tauri-drag-region": "" } }));
    const bar = root.createDiv("sb-toolbar");
    const main = root.createDiv("sb-main");
    this.toolsEl = main.createDiv("sb-tools");
    const center = main.createDiv("sb-center");
    this.stage = center.createDiv({ cls: "sb-stage", attr: { tabindex: "0" } });
    const handle = main.createDiv({ cls: "sb-side-handle", attr: { "aria-label": "Drag to resize" } });
    const side = (this.side = main.createDiv("sb-side"));
    this.dragWidth(handle, side);
    this.focusBar = center.createDiv("sb-focusbar");
    const ed = (this.ed = new Editor(doc, this.stage));
    ed.app = this.app;

    // Manga balloons read vertically by default; comics and the rest horizontally.
    this.settings.balloon.vertical = doc.board.kind === "name";
    this.buildToolbar(bar);
    this.buildTools();
    const host = { trace: (r: Reference) => this.trace(r), place: (r: Reference) => this.layers?.placePicture(r.id) };
    ed.frame = doc.board.frames[0];
    this.refs = new ReferencePanel(this.app, ed, side, host);
    const resizer = side.createDiv("sb-side-resizer");
    this.layers = new LayersPanel(ed, side, { chooseReference: (cb) => this.chooseReference(cb), trace: () => this.trace() });
    this.dragSplit(resizer, side);
    this.timeline = new Timeline(this.app, ed, root);

    ed.on("history", () => this.updateHistory());
    ed.on("view", () => this.zoomLabel.setText(`${Math.round(ed.view.scale * 100)}%`));
    ed.on("dirty", () => this.setStatus("Edited"));
    ed.on("saved", () => this.setStatus("Saved"));
    ed.on("save-error", (e: any) => {
      this.setStatus("Not saved");
      new Notice(`The storyboard could not be saved: ${e?.message || e}`);
    });
    ed.on("notice", (t: string) => new Notice(t));
    ed.on("color", () => this.updateColorButton());
    ed.on("board", () => this.updateTraceButton());
    ed.on("frame", () => this.updateTraceButton());
    ed.on("selection", () => this.renderOptions());
    ed.on("panel", () => this.renderOptions());
    ed.on("balloon", () => this.renderOptions());
    ed.on("onion", () => this.updateOnion());
    ed.on("board", () => this.updateOnion());
    ed.on("frame", () => this.renderFocusBar());
    ed.on("board", () => this.updateJoin());
    ed.on("frame", () => this.updateJoin());
    ed.on("history", () => this.renderFocusBar());
    // Another tab coming forward leaves focus mode (it covers the whole window).
    this.registerEvent(this.app.workspace.on("active-leaf-change", (leaf: any) => {
      if (this.focus && leaf && leaf !== this.leaf) this.setFocus(false);
    }));

    this.bindStage();
    this.setTool(this.toolId);
    this.updateHistory();
    this.updateColorButton();
    this.setStatus("Saved");
    ed.view.mirror = false;
    ed.setFrame(0, false).then(() => {
      ed.fit();
      if (!doc.thumbs.size) ed.scheduleThumb();
      this.updateTraceButton();
    });
  }

  private dragWidth(handle: HTMLElement, side: HTMLElement) {
    try {
      const w = Number(localStorage.getItem("ateliet-storyboard-side") || 0);
      if (w) side.style.width = `${w}px`;
    } catch {
      /* private mode */
    }
    handle.addEventListener("pointerdown", (e) => {
      try { handle.setPointerCapture(e.pointerId); } catch { /* synthetic */ }
      const x0 = e.clientX, w0 = side.getBoundingClientRect().width;
      // In focus mode the reference can sit on either side and is sized in percent.
      const dir = this.focus && this.root.hasClass("ref-left") ? 1 : -1;
      const total = side.parentElement!.getBoundingClientRect().width;
      const move = (ev: PointerEvent) => {
        const w = w0 + dir * (ev.clientX - x0);
        if (this.focus) side.style.width = `${Math.max(15, Math.min(80, (w / total) * 100))}%`;
        else side.style.width = `${Math.max(200, Math.min(900, w))}px`;
      };
      const up = () => {
        handle.removeEventListener("pointermove", move);
        handle.removeEventListener("pointerup", up);
        try {
          if (this.focus) localStorage.setItem("ateliet-storyboard-focus-ref", side.style.width);
          else localStorage.setItem("ateliet-storyboard-side", String(Math.round(side.getBoundingClientRect().width)));
        } catch {
          /* private mode */
        }
      };
      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", up);
    });
  }

  private dragSplit(handle: HTMLElement, side: HTMLElement) {
    handle.addEventListener("pointerdown", (e) => {
      try { handle.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
      const refs = this.refs!.el;
      const y0 = e.clientY, h0 = refs.getBoundingClientRect().height;
      const move = (ev: PointerEvent) => {
        const total = side.getBoundingClientRect().height;
        refs.style.flex = `0 0 ${Math.max(120, Math.min(total - 140, h0 + ev.clientY - y0))}px`;
      };
      const up = () => {
        handle.removeEventListener("pointermove", move);
        handle.removeEventListener("pointerup", up);
      };
      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", up);
    });
  }

  private button(parent: HTMLElement, icon: string, label: string, fn: (e: MouseEvent) => void): HTMLElement {
    const b = parent.createDiv({ cls: "clickable-icon", attr: { "aria-label": label } });
    setIcon(b, icon);
    b.addEventListener("click", fn);
    return b;
  }

  private buildToolbar(bar: HTMLElement) {
    const ed = this.ed!;
    this.undoBtn = this.button(bar, "undo-2", "Undo (Ctrl/Cmd+Z)", () => ed.undo());
    this.redoBtn = this.button(bar, "redo-2", "Redo (Ctrl/Cmd+Shift+Z)", () => ed.redo());
    this.optionsEl = bar.createDiv("sb-options");
    bar.createDiv("sb-spacer");
    this.button(bar, "zoom-out", "Zoom out (−)", () => ed.zoomAt(0.8));
    this.zoomLabel = bar.createDiv({ cls: "sb-zoom", text: "100%", attr: { "aria-label": "Fit (0) · 100% (1)" } });
    this.zoomLabel.addEventListener("click", () => (Math.abs(ed.view.scale - 1) < 0.01 ? ed.fit() : ed.setZoom(1)));
    this.button(bar, "zoom-in", "Zoom in (+)", () => ed.zoomAt(1.25));
    this.button(bar, "maximize", "Fit the frame (0)", () => ed.fit());
    this.button(bar, "rotate-ccw", "Rotate view (hold R and drag) · click to reset", () => ed.rotateTo(0));
    this.button(bar, "flip-horizontal-2", "Mirror the view (F): check proportions, the drawing is not changed", () => ed.setMirror(!ed.view.mirror));
    this.button(bar, "grid-3x3", "Guides and grid", (e) => this.guidesMenu(e));
    bar.createDiv("sb-sep");
    this.traceBtn = this.button(bar, "scan-line", "Trace (T shows/hides · hold Q to peek · Shift+T display)", (e) => this.traceMenu(e));
    if (!isPageKind(ed.board.kind)) {
      this.onionBtn = this.button(bar, "layers-3", "Onion skin (O): the frames before and after, faint", () => this.timeline?.toggleOnion());
      const onionMore = this.button(bar, "chevron-down", "Onion skin settings", () => this.timeline?.onionPanel(onionMore));
      onionMore.addClass("sb-caret");
    }
    if (isPageKind(ed.board.kind) && ed.board.kind !== "webtoon" && ed.board.kind !== "thumbnails") {
      this.joinBtn = this.button(bar, "book-open", "Join the two pages: draw across the spine and the margins (separate again any time; nothing drawn is lost)", () => this.toggleJoined());
    }
    bar.createDiv("sb-sep");
    this.focusBtn = this.button(bar, "focus", "Focus (Tab): only the drawing and the reference", () => this.setFocus(!this.focus));
    this.button(bar, "import", "Import (Clip Studio, pictures, Pictogrep drawings)", (e) => this.importMenu(e));
    this.button(bar, "share", "Export (PNG frames, contact sheet, Pictogrep, video)", (e) => this.exportMenu(e));
    this.button(bar, "settings", "Storyboard settings (size, infinite canvas, paper, fps)", () => this.editSettings());
    this.button(bar, "panel-right", "Show or hide references and layers · timeline: Shift+Tab", () => this.root.toggleClass("hide-side", !this.root.hasClass("hide-side")));
    this.statusEl = bar.createDiv("sb-status");
  }

  private buildTools() {
    const ed = this.ed!;
    const s = () => this.settings;
    for (const k of ["pencil", "pen", "brush", "marker", "eraser"] as BrushKind[]) this.tools.set(k, new BrushTool(k, ed, s));
    this.tools.set("fill", new FillTool(ed, s));
    this.tools.set("lasso", new LassoFillTool(ed, s));
    this.tools.set("shape", new ShapeTool(ed, s));
    this.tools.set("select", new SelectTool(ed, s));
    this.tools.set("move", new MoveTool(ed));
    this.tools.set("eyedropper", new EyedropperTool(ed));
    this.tools.set("hand", new HandTool(ed));
    this.tools.set("panel", new PanelTool(ed));
    this.tools.set("balloon", new BalloonTool(ed, s));
    this.toolsEl.empty();
    for (const t of toolsFor(ed.board.kind)) {
      const b = this.toolsEl.createDiv({ cls: "sb-tool clickable-icon", attr: { "aria-label": `${t.name} (${t.key})`, "data-tool": t.id, "data-tooltip-position": "right" } });
      setIcon(b, t.icon);
      b.addEventListener("click", () => this.setTool(t.id));
    }
    this.toolsEl.createDiv("sb-tools-spacer");
    const colors = this.toolsEl.createDiv("sb-colors");
    this.colorBtn = colors.createDiv({ cls: "sb-color-main", attr: { "aria-label": "Colour" } });
    const second = colors.createDiv({ cls: "sb-color-second", attr: { "aria-label": "Second colour (X swaps)" } });
    this.colorBtn.addEventListener("click", () => this.toggleColor());
    second.addEventListener("click", () => ed.swapColors());
  }

  private updateColorButton() {
    const ed = this.ed!;
    this.colorBtn.style.background = ed.color;
    (this.colorBtn.nextElementSibling as HTMLElement).style.background = ed.secondary;
  }

  private toggleColor() {
    if (this.colorPop) {
      this.colorPop.panel.destroy();
      this.colorPop.el.remove();
      this.colorPop = null;
      return;
    }
    const el = this.root.createDiv("sb-color-pop");
    const panel = new ColorPanel(this.ed!, el);
    const close = el.createDiv({ cls: "clickable-icon sb-color-close", attr: { "aria-label": "Close" } });
    setIcon(close, "x");
    close.addEventListener("click", () => this.toggleColor());
    this.colorPop = { el, panel };
  }

  // ── focus mode ──────────────────────────────────────────────────────────
  //
  // Only the drawing and the reference, side by side, over the whole window:
  // Ateliet's tabs and sidebars, the toolbar, the timeline and the layers go
  // away; the slim tool column and a small floating bar stay. Tab or Esc
  // come back. The reference can sit left or right and the divider moves.

  get inFocus() {
    return !!this.focus;
  }

  async setFocus(on: boolean) {
    if (on === !!this.focus || !this.ed) return;
    closePopover();
    if (on) {
      this.focus = { parent: this.root.parentElement!, next: this.root.nextSibling, osFullscreen: false };
      document.body.appendChild(this.root);
      this.root.addClass("sb-focus");
      let left = false, width = "42%";
      try {
        left = localStorage.getItem("ateliet-storyboard-focus-left") === "1";
        width = localStorage.getItem("ateliet-storyboard-focus-ref") || width;
      } catch {
        /* private mode */
      }
      this.root.toggleClass("ref-left", left);
      this.side.style.width = width;
    } else {
      const f = this.focus!;
      this.focus = null;
      if (f.osFullscreen) await currentWindow().setFullscreen(false).catch(() => {});
      this.root.removeClass("sb-focus", "ref-left");
      if (f.parent.isConnected) f.parent.insertBefore(this.root, f.next && f.next.parentNode === f.parent ? f.next : null);
      else this.contentEl.appendChild(this.root);
      let w = 0;
      try {
        w = Number(localStorage.getItem("ateliet-storyboard-side") || 0);
      } catch {
        /* private mode */
      }
      this.side.style.width = w ? `${w}px` : "";
    }
    this.focusBtn?.toggleClass("is-active", !!this.focus);
    this.renderFocusBar();
    this.stage.focus({ preventScroll: true });
  }

  private async toggleOsFullscreen() {
    if (!this.focus) return;
    const win = currentWindow();
    const now = await win.isFullscreen().catch(() => false);
    await win.setFullscreen(!now).catch(() => {});
    this.focus.osFullscreen = !now;
    this.renderFocusBar();
  }

  private renderFocusBar() {
    const bar = this.focusBar;
    if (!bar || !this.ed) return;
    bar.empty();
    if (!this.focus) return;
    const ed = this.ed;
    const b = (icon: string, label: string, fn: () => void, active = false, disabled = false) => {
      const x = bar.createDiv({ cls: "clickable-icon", attr: { "aria-label": label } });
      setIcon(x, icon);
      x.toggleClass("is-active", active);
      x.toggleClass("mod-disabled", disabled);
      x.addEventListener("click", () => !disabled && fn());
      return x;
    };
    b("minimize-2", "Leave focus (Tab or Esc)", () => this.setFocus(false));
    bar.createDiv("sb-sep");
    b("undo-2", "Undo (Ctrl/Cmd+Z)", () => ed.undo(), false, !ed.history.canUndo);
    b("redo-2", "Redo", () => ed.redo(), false, !ed.history.canRedo);
    const brush = this.settings.brushes[this.toolId as BrushKind];
    if (brush) {
      bar.createDiv("sb-sep");
      this.slider(bar, "", 0.5, 500, brush.size, (v) => (v < 10 ? v.toFixed(1) : String(Math.round(v))), (v) => (brush.size = v), true);
    }
    bar.createDiv("sb-sep");
    const traces = ed.layers.filter((l) => l.kind === "trace");
    b("scan-line", "Trace (T)", () => this.toggleTrace(), traces.some((l) => l.visible));
    if (!isPageKind(ed.board.kind)) b("layers-3", "Onion skin (O)", () => this.timeline?.toggleOnion(), ed.board.onion.enabled);
    b("flip-horizontal-2", "Mirror the view (F)", () => ed.setMirror(!ed.view.mirror), ed.view.mirror);
    bar.createDiv("sb-sep");
    b("chevron-left", "Previous frame (↑ or ,)", () => this.timeline?.go(ed.frameIndex - 1), false, ed.frameIndex === 0);
    bar.createDiv({ cls: "sb-focus-frame", text: `${ed.frameIndex + 1} / ${ed.board.frames.length}` });
    b("chevron-right", "Next frame (↓ or .)", () => this.timeline?.go(ed.frameIndex + 1), false, ed.frameIndex >= ed.board.frames.length - 1);
    b("plus", "New frame (Ctrl/Cmd+Enter)", () => ed.newFrame());
    bar.createDiv("sb-sep");
    b("arrow-left-right", "Reference on the other side", () => {
      const left = !this.root.hasClass("ref-left");
      this.root.toggleClass("ref-left", left);
      try {
        localStorage.setItem("ateliet-storyboard-focus-left", left ? "1" : "0");
      } catch {
        /* private mode */
      }
    });
    b(this.focus.osFullscreen ? "shrink" : "expand", this.focus.osFullscreen ? "Leave full screen" : "Full screen", () => this.toggleOsFullscreen(), this.focus.osFullscreen);
  }

  private toggleJoined() {
    const ed = this.ed!;
    if (!ed.facing()) {
      new Notice(ed.board.page?.spreads ? "This page has no facing page (the first page stands alone)." : "Turn on spreads in the storyboard settings to see two pages side by side.");
      return;
    }
    ed.toggleJoined();
  }

  private updateJoin() {
    const ed = this.ed;
    if (!ed || !this.joinBtn) return;
    const joined = !!ed.joinedFacing();
    this.joinBtn.toggleClass("is-active", joined);
    this.joinBtn.toggleClass("is-disabled", !ed.facing());
    this.joinBtn.setAttr("aria-label", joined ? "Separate the pages: panels and margins come back (what was drawn across them stays, hidden under the paper)" : "Join the two pages: draw across the spine and the margins (separate again any time; nothing drawn is lost)");
    this.root.toggleClass("is-joined", joined);
  }

  private updateOnion() {
    if (!this.ed || !this.onionBtn) return;
    this.onionBtn.toggleClass("is-active", this.ed.board.onion.enabled);
    this.renderFocusBar();
  }

  private setStatus(s: string) {
    this.statusEl?.setText(s);
    this.statusEl?.toggleClass("is-dirty", s !== "Saved");
  }

  private updateHistory() {
    const h = this.ed!.history;
    this.undoBtn.toggleClass("mod-disabled", !h.canUndo);
    this.redoBtn.toggleClass("mod-disabled", !h.canRedo);
    this.undoBtn.setAttr("aria-label", h.canUndo ? `Undo ${h.peekUndo()!.label.toLowerCase()} (Ctrl/Cmd+Z)` : "Nothing to undo");
    this.redoBtn.setAttr("aria-label", h.canRedo ? `Redo ${h.peekRedo()!.label.toLowerCase()} (Ctrl/Cmd+Shift+Z)` : "Nothing to redo");
  }

  private saveToolSettings() {
    try {
      localStorage.setItem(TOOLS_KEY, JSON.stringify(this.settings));
    } catch {
      /* private mode */
    }
  }

  // ── tools and their options ─────────────────────────────────────────────

  setTool(id: string) {
    const ed = this.ed!;
    const t = this.tools.get(id);
    if (!t) return;
    ed.tool?.deactivate?.();
    this.toolId = id;
    ed.tool = t;
    t.activate?.();
    this.stage.style.cursor = t.cursor || "";
    this.toolsEl.querySelectorAll(".sb-tool").forEach((b: any) => b.toggleClass("is-active", b.dataset.tool === id));
    this.renderOptions();
    this.renderFocusBar();
    ed.drawOverlay();
  }

  private slider(parent: HTMLElement, label: string, min: number, max: number, value: number, fmt: (v: number) => string, set: (v: number) => void, log = false) {
    const wrap = parent.createDiv("sb-opt");
    wrap.createSpan({ cls: "sb-opt-label", text: label });
    const toPos = (v: number) => (log ? (Math.log(v) - Math.log(min)) / (Math.log(max) - Math.log(min)) * 1000 : ((v - min) / (max - min)) * 1000);
    const fromPos = (p: number) => (log ? Math.exp(Math.log(min) + (p / 1000) * (Math.log(max) - Math.log(min))) : min + (p / 1000) * (max - min));
    const input = wrap.createEl("input", { type: "range", attr: { min: "0", max: "1000", value: String(toPos(value)) } });
    const out = wrap.createSpan({ cls: "sb-opt-value", text: fmt(value) });
    input.addEventListener("input", () => {
      const v = fromPos(Number(input.value));
      out.setText(fmt(v));
      set(v);
      this.ed!.drawOverlay();
    });
    input.addEventListener("change", () => this.saveToolSettings());
    return input;
  }

  private check(parent: HTMLElement, label: string, value: boolean, set: (v: boolean) => void, title = "") {
    const lab = parent.createEl("label", { cls: "sb-opt sb-opt-check", attr: { title } });
    const cb = lab.createEl("input", { type: "checkbox" });
    cb.checked = value;
    lab.appendText(label);
    cb.addEventListener("change", () => {
      set(cb.checked);
      this.saveToolSettings();
    });
  }

  private segmented<T extends string>(parent: HTMLElement, options: [T, string, string][], value: T, set: (v: T) => void) {
    const wrap = parent.createDiv("sb-seg");
    for (const [v, icon, label] of options) {
      const b = wrap.createDiv({ cls: "clickable-icon", attr: { "aria-label": label } });
      setIcon(b, icon);
      b.toggleClass("is-active", v === value);
      b.addEventListener("click", () => {
        set(v);
        this.saveToolSettings();
        this.renderOptions();
      });
    }
  }

  private renderOptions() {
    const el = this.optionsEl;
    if (!el || !this.ed) return;
    el.empty();
    const id = this.toolId;
    const ed = this.ed;
    const tool = TOOL_INFO.find((t) => t.id === id)!;
    el.createSpan({ cls: "sb-opt-tool", text: tool.name });
    if (id in BRUSH_NAMES) {
      const b = this.settings.brushes[id as BrushKind];
      this.slider(el, "Size", 0.5, 500, b.size, (v) => (v < 10 ? v.toFixed(1) : String(Math.round(v))), (v) => (b.size = v), true);
      this.slider(el, "Opacity", 0.02, 1, b.opacity, (v) => `${Math.round(v * 100)}%`, (v) => (b.opacity = v));
      this.slider(el, "Steady", 0, 0.95, b.smoothing, (v) => `${Math.round(v * 100)}`, (v) => (b.smoothing = v));
      if (id !== "pencil") this.slider(el, "Hardness", 0, 1, b.hardness, (v) => `${Math.round(v * 100)}`, (v) => (b.hardness = v));
      this.check(el, "Pressure → size", b.pressureSize, (v) => (b.pressureSize = v), "Pen pressure changes the width");
      this.check(el, "→ opacity", b.pressureOpacity, (v) => (b.pressureOpacity = v), "Pen pressure changes the opacity");
    } else if (id === "fill") {
      const f = this.settings.fill;
      this.slider(el, "Tolerance", 0, 255, f.tolerance, (v) => String(Math.round(v)), (v) => (f.tolerance = Math.round(v)));
      this.slider(el, "Grow", 0, 8, f.expand, (v) => `${Math.round(v)} px`, (v) => (f.expand = Math.round(v)));
      this.segmented(el, [["all", "layers", "Look at all layers (fill inside lines drawn on other layers)"], ["layer", "square", "Look at this layer only"]], f.sample, (v) => (f.sample = v));
      this.check(el, "Behind lines", f.behind, (v) => (f.behind = v), "Paint under what is already on the layer");
    } else if (id === "lasso") {
      this.slider(el, "Opacity", 0.02, 1, this.settings.lasso.opacity, (v) => `${Math.round(v * 100)}%`, (v) => (this.settings.lasso.opacity = v));
      el.createSpan({ cls: "sb-hint", text: "Draw a shape to fill it · Alt: erase it" });
    } else if (id === "shape") {
      const s = this.settings.shape;
      this.segmented(el, [["line", "minus", "Line"], ["arrow", "move-right", "Camera arrow"], ["rect", "square", "Rectangle (panel)"], ["ellipse", "circle", "Ellipse"]], s.kind, (v) => (s.kind = v));
      this.slider(el, "Width", 0.5, 80, s.width, (v) => v.toFixed(v < 10 ? 1 : 0), (v) => (s.width = v), true);
      this.check(el, "Filled", s.fill, (v) => (s.fill = v));
      el.createSpan({ cls: "sb-hint", text: "Shift: straight / square · Alt: from the centre" });
    } else if (id === "select") {
      const s = this.settings.select;
      this.segmented(el, [["rect", "square-dashed", "Rectangle"], ["ellipse", "circle-dashed", "Ellipse"], ["lasso", "lasso", "Free shape"]], s.mode, (v) => (s.mode = v));
      const act = (icon: string, label: string, fn: () => void, enabled = true) => {
        const b = this.button(el, icon, label, fn);
        b.toggleClass("mod-disabled", !enabled);
      };
      const has = !!ed.selection;
      act("square-dashed-mouse-pointer", "Select all (Ctrl/Cmd+A)", () => this.selectAll());
      act("x", "Deselect (Ctrl/Cmd+D)", () => ed.setSelection(null), has);
      act("move", "Move or transform the selection (Ctrl/Cmd+T)", () => this.transform(), has);
      act("paint-bucket", "Fill with the colour", () => ed.fillSelection(), has);
      act("eraser", "Clear (Delete)", () => ed.clearSelection(), has);
      act("copy", "Copy (Ctrl/Cmd+C)", () => this.copy(false), has);
    } else if (id === "move") {
      const mt = this.tools.get("move") as MoveTool;
      if (ed.floating) {
        this.button(el, "check", "Apply (Enter)", () => ed.commitFloating());
        this.button(el, "x", "Cancel (Esc)", () => ed.cancelFloating());
      } else {
        this.button(el, "scaling", "Transform the layer: scale and rotate (Ctrl/Cmd+T)", () => this.transform());
      }
      this.button(el, "flip-horizontal", "Flip horizontally", () => mt.flip("x"));
      this.button(el, "flip-vertical", "Flip vertically", () => mt.flip("y"));
      el.createSpan({ cls: "sb-hint", text: "Drag to move · corners scale (Shift: free) · round handle rotates (Shift: 15°) · Alt+arrows nudge" });
    } else if (id === "eyedropper") {
      el.createSpan({ cls: "sb-hint", text: "Click the canvas or a reference · Alt+click does this with any brush" });
    } else if (id === "hand") {
      el.createSpan({ cls: "sb-hint", text: "Drag to move around · Space does this with any tool · wheel zooms" });
    } else if (id === "panel") {
      this.panelOptions(el);
    } else if (id === "balloon") {
      this.balloonOptions(el);
    }
  }

  /** Changes the page setup (one undo step). */
  private setPage(label: string, fn: (p: NonNullable<import("./model.ts").Board["page"]>) => void) {
    const ed = this.ed!;
    if (!ed.board.page) return;
    ed.mutate(label, (b) => fn(b.page!));
  }

  private panelOptions(el: HTMLElement) {
    const ed = this.ed!;
    const b = ed.board, page = b.page!;
    const mt = this.tools.get("panel") as PanelTool;
    if (b.kind === "name" || b.kind === "comic") {
      const sel = el.createEl("select", { cls: "dropdown sb-opt-select", attr: { "aria-label": "Page layout" } });
      sel.createEl("option", { value: "", text: "Lay out this page…" });
      for (const l of LAYOUTS) sel.createEl("option", { value: l.id, text: l.name });
      sel.addEventListener("change", () => {
        const spec = LAYOUTS.find((l) => l.id === sel.value);
        if (!spec) return;
        ed.mutate(`Layout: ${spec.name}`, () => (ed.frame.panels = spec.id === "one" ? [rectPanel(page.inner)] : layoutPanels(page.inner, spec.rows, page.gutterX, page.gutterY, page.rtl)));
        mt.selected = null;
        sel.value = "";
      });
    }
    if (b.kind === "thumbnails") {
      const sel = el.createEl("select", { cls: "dropdown sb-opt-select", attr: { "aria-label": "Cell shape" } });
      sel.createEl("option", { value: "", text: "New grid on this sheet…" });
      for (const [v, n] of THUMB_ASPECTS) sel.createEl("option", { value: v, text: n });
      sel.addEventListener("change", () => {
        const a = THUMB_ASPECTS.find((x) => x[0] === sel.value);
        if (!a) return;
        ed.mutate("New thumbnail grid", (bd) => {
          bd.thumbAspect = a[2];
          ed.frame.panels = startPanels(bd, undefined, a[2]);
        });
        sel.value = "";
      });
    }
    if (b.kind === "webtoon") {
      this.button(el, "square-plus", "Add a panel below the last one", () => this.webtoonPanel());
      this.button(el, "move-vertical", `Make the strip longer (+${page.slice} px)`, () => ed.mutate("Longer strip", (bd) => (bd.height += page.slice)));
      this.button(el, "smartphone", "Read it on a phone-sized screen", () => this.phonePreview());
    }
    this.slider(el, b.kind === "webtoon" ? "Gap" : "Gutters", 0, b.kind === "webtoon" ? 1200 : 120, b.kind === "webtoon" ? page.gutterY : page.gutterX, (v) => `${Math.round(v)}`, (v) => {
      if (b.kind === "webtoon") page.gutterY = Math.round(v);
      else {
        const ratio = page.gutterX ? page.gutterY / page.gutterX : 2;
        page.gutterX = Math.round(v);
        page.gutterY = Math.round(v * ratio);
      }
      ed.changed();
    });
    this.slider(el, "Border", 0, 16, page.border, (v) => v.toFixed(1), (v) => {
      page.border = v;
      ed.changed();
    });
    if (this.joinBtn) {
      const joined = !!ed.joinedFacing();
      const jb = this.button(el, "book-open", joined ? "Separate the pages" : "Join the two pages", () => this.toggleJoined());
      jb.toggleClass("is-active", joined);
      el.createSpan({ cls: "sb-opt-label", text: joined ? "Joined: cuts go through both pages" : "Join pages" });
    }
    this.check(el, "Strokes stay in their panel", page.clipToPanel, (v) => this.setPage("Strokes in panels", (p) => (p.clipToPanel = v)), "A stroke that starts in a panel cannot leave it (like drawing in a frame)");
    this.check(el, "Paper outside panels", page.mask, (v) => this.setPage("Paper outside panels", (p) => (p.mask = v)), "Hide what is drawn in the gutters and margins");
    if (mt.selected) {
      this.button(el, "zoom-in", "Zoom to the panel (double click)", () => {
        const pn = ed.frame.panels?.find((x) => x.id === mt.selected);
        if (pn) mt.zoomTo(pn);
      });
      if (b.kind === "thumbnails") this.button(el, "star", "Star this panel as a keeper (*)", () => mt.toggleStar());
      this.button(el, "trash-2", "Delete the panel (Delete)", () => mt.deleteSelected());
    }
    if (b.kind === "thumbnails") this.button(el, "send", "Send the panels to an animatic…", () => this.sendToAnimatic());
    el.createSpan({ cls: "sb-hint", text: b.kind === "webtoon" ? "Drag in empty space for a panel · drag across one to cut it" : "Drag across a panel to cut it (Shift: straight) · drag in empty space for a new one · click, then drag to move" });
  }

  private balloonOptions(el: HTMLElement) {
    const bs = this.settings.balloon;
    const bt = this.tools.get("balloon") as BalloonTool;
    this.segmented(el, BALLOON_KINDS.map(([k, icon, name]) => [k, icon, name] as [any, string, string]), bs.kind, (v) => {
      bs.kind = v;
      bt.applyToSelected({ kind: v });
    });
    this.check(el, "Vertical text", bs.vertical, (v) => {
      bs.vertical = v;
      bt.applyToSelected({ vertical: v });
    }, "Japanese style: columns read top to bottom, right to left");
    this.slider(el, "Text", 8, 140, bs.size, (v) => String(Math.round(v)), (v) => (bs.size = Math.round(v)));
    const sizeInput = el.lastElementChild?.querySelector("input");
    sizeInput?.addEventListener("change", () => bt.applyToSelected({ size: bs.size }));
    el.createSpan({ cls: "sb-hint", text: "Drag or click to place · type · drag the round handle to aim the tail · double click to edit" });
  }

  /** Webtoon: a full-width panel under the last one (the strip grows when needed). */
  private webtoonPanel() {
    const ed = this.ed!;
    const page = ed.board.page!;
    const panels = ed.frame.panels || [];
    const bottom = panels.reduce((m, p) => Math.max(m, ...p.points.map((q) => q.y)), 0);
    const y = panels.length ? bottom + page.gutterY : 200;
    const h = Math.round(page.inner.w * 1.1);
    ed.mutate("Add panel", (b) => {
      while (y + h + 200 > b.height) b.height += page.slice;
      (ed.frame.panels ||= []).push(rectPanel({ x: page.inner.x, y, w: page.inner.w, h }));
    });
    ed.centerOn(ed.board.width / 2, y + h / 2);
  }

  /** Webtoon: the strip at phone width, scrolled like a reader would. */
  private async phonePreview() {
    const ed = this.ed!;
    const m = new Modal(this.app);
    m.setTitle("Phone preview");
    m.modalEl.addClass("sb-phone-modal");
    const phone = m.contentEl.createDiv("sb-phone");
    const screen = phone.createDiv("sb-phone-screen");
    screen.createDiv({ cls: "sb-hint", text: "Rendering…" });
    m.open();
    const pics: HTMLCanvasElement[] = [];
    for (const f of ed.board.frames) pics.push(await ed.comp.picture(f, Math.min(1, 780 / ed.board.width), { exporting: true }));
    screen.empty();
    for (const p of pics) {
      p.addClass("sb-phone-part");
      screen.appendChild(p);
    }
  }

  /** Thumbnails → animatic: each panel (starred ones, or all) becomes a frame to draw over. */
  async sendToAnimatic() {
    const ed = this.ed!;
    const b = ed.board;
    const all: { frame: import("./model.ts").Frame; panel: import("./model.ts").Panel }[] = [];
    for (const f of b.frames) for (const pn of readingOrder(f.panels || [], false)) all.push({ frame: f, panel: pn });
    if (!all.length) return new Notice("These sheets have no panels.");
    const starred = all.filter((x) => x.panel.star);
    const targets: TFile[] = this.app.vault.getFiles().filter((f: TFile) => f.extension === "storyboard" && f !== this.file);
    const o = { which: starred.length ? "starred" : "all", target: "" };
    const modal = new Modal(this.app);
    modal.setTitle("Send panels to an animatic");
    new Setting(modal.contentEl).setName("Panels").addDropdown((d) => {
      if (starred.length) d.addOption("starred", `Starred (${starred.length})`);
      d.addOption("all", `All (${all.length})`);
      d.setValue(o.which).onChange((v) => (o.which = v));
    });
    new Setting(modal.contentEl).setName("Into").setDesc("Each panel becomes a frame: the thumbnail on a faint layer, and an empty layer above to draw the clean version.").addDropdown((d) => {
      d.addOption("", "A new animatic");
      for (const f of targets) d.addOption(f.path, f.path.replace(/\.storyboard$/, ""));
      d.onChange((v) => (o.target = v));
    });
    const buttons = modal.modalEl.createDiv("modal-button-container");
    const go = buttons.createEl("button", { text: "Send", cls: "mod-cta" });
    go.addEventListener("click", async () => {
      modal.close();
      const list = o.which === "starred" ? starred : all;
      const notice = new Notice(`Sending ${list.length} panels…`, 0);
      try {
        await this.panelsToAnimatic(list, o.target);
        notice.hide();
      } catch (e) {
        notice.hide();
        new Notice(`Could not send the panels: ${e}`);
      }
    });
    modal.open();
  }

  private async panelsToAnimatic(list: { frame: import("./model.ts").Frame; panel: import("./model.ts").Panel }[], target: string) {
    const ed = this.ed!;
    const a = ed.board.thumbAspect || 16 / 9;
    const size = a > 2 ? [2048, 858] : a > 1.5 ? [1920, 1080] : a > 1.2 ? [1440, 1080] : a > 0.9 ? [1080, 1080] : a > 0.65 ? [1240, 1754] : [1080, 1920];
    // The target storyboard: a new file, or one opened in its own tab.
    let file: TFile;
    if (!target) {
      const parent = this.file?.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
      let path = `${parent}${this.baseName()} animatic.storyboard`;
      for (let i = 2; this.app.vault.getAbstractFileByPath(path); i++) path = `${parent}${this.baseName()} animatic ${i}.storyboard`;
      const { boardForKind, defaultKindOptions } = await import("./kinds.ts");
      const board = boardForKind({ ...defaultKindOptions("animatic"), width: size[0], height: size[1] });
      const doc = StoryDoc.create(this.app.vault.adapter.getFullPath(path), board);
      const bytes = await doc.encode();
      file = await this.app.vault.createBinary(path, bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
    } else file = this.app.vault.getAbstractFileByPath(target);
    const leaf = this.app.workspace.getLeaf("tab");
    await leaf.openFile(file);
    const view = leaf.view as StoryboardView;
    for (let i = 0; i < 60 && !view.ed?.frame; i++) await new Promise((r) => setTimeout(r, 100));
    const ted = view.ed;
    if (!ted) throw new Error("the animatic did not open");
    const tb = ted.board;
    const before = (await import("./history.ts")).cloneBoard(tb);
    const fresh = !target;
    for (const [n, it] of list.entries()) {
      const r = (await import("./pages.ts")).bounds(it.panel.points);
      const k = Math.min(tb.width / r.w, tb.height / r.h);
      const pic = await ed.comp.picture(it.frame, k, { exporting: true, area: r, background: false });
      // The first frame of a new animatic is reused.
      const f = fresh && n === 0 ? tb.frames[0] : (await import("./kinds.ts")).addPage(tb, tb.frames.length);
      const thumb = (await import("./model.ts")).newLayer("Thumbnail", "raster", { opacity: 0.45 });
      tb.layers[thumb.id] = thumb;
      f.layers.unshift(thumb.id);
      f.name = `${ed.frameName(it.frame)} · ${n + 1}`;
      const x = Math.round((tb.width - pic.width) / 2), y = Math.round((tb.height - pic.height) / 2);
      ted.doc.tiles.paint(thumb.id, x, y, pic.width, pic.height, (ctx) => ctx.drawImage(pic, 0, 0));
      ted.scheduleThumb(f);
    }
    ted.recordSince(`Panels from ${this.baseName()}`, before);
    await ted.refreshFrame();
    ted.fit();
    new Notice(`${list.length} panels are now frames of "${file.basename}", ready to draw over.`);
  }

  private exportPages(kind: "pdf" | "png" | "slices") {
    const ed = this.ed!;
    const parent = this.file?.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
    const n = new Notice("Exporting…", 0);
    (async () => {
      try {
        if (kind === "pdf") {
          const bytes = isPageKind(ed.board.kind) ? await pagesPdf(ed, (i, t) => n.setMessage(`Page ${i + 1}/${t}…`)) : await boardPdf(ed, 6, true, (i, t) => n.setMessage(`Page ${i + 1}/${t}…`));
          const f = await writeVault(this.app, `${parent}${this.baseName()}.pdf`, bytes);
          n.hide();
          new Notice(`Saved ${f.path}.`);
        } else if (kind === "slices") {
          const count = await webtoonSlices(ed, `${parent}${this.baseName()} slices`, (s) => n.setMessage(s));
          n.hide();
          new Notice(`${count} slices saved in "${this.baseName()} slices".`);
        }
      } catch (err) {
        n.hide();
        new Notice(`Export failed: ${err}`);
      }
    })();
  }

  private selectAll() {
    const ed = this.ed!;
    const b = ed.board;
    const r = b.infinite ? ed.visibleRegion() : { x: 0, y: 0, w: b.width, h: b.height };
    ed.setSelection(selectionFrom([{ x: r.x, y: r.y }, { x: r.x + r.w, y: r.y }, { x: r.x + r.w, y: r.y + r.h }, { x: r.x, y: r.y + r.h }]));
  }

  private async transform() {
    const ed = this.ed!;
    const l = ed.active;
    if (!l) return;
    if (l.kind === "raster") {
      if (l.locked) return new Notice("This layer is locked.");
      if (!(await ed.liftSelection())) return new Notice("Nothing to transform on this layer.");
    } else if (l.locked) {
      ed.mutate("Unlock layer", () => (l.locked = false));
    }
    this.setTool("move");
  }

  // ── pointer and wheel ───────────────────────────────────────────────────

  private bindStage() {
    const ed = this.ed!;
    const st = this.stage;
    st.addEventListener("pointerdown", (e) => {
      st.focus({ preventScroll: true });
      this.closeColorOnCanvas();
      if (this.timeline?.isPlaying) this.timeline.stop();
      const r = st.getBoundingClientRect();
      if (e.button === 1 || this.keys.has(" ") || (e.button === 0 && this.toolId === "hand")) {
        this.panning = { x: e.clientX, y: e.clientY };
        try { st.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
        st.style.cursor = "grabbing";
        e.preventDefault();
        return;
      }
      if (this.keys.has("r") && e.button === 0) {
        const c = { x: r.width / 2, y: r.height / 2 };
        this.rotating = { a0: Math.atan2(e.clientY - r.top - c.y, e.clientX - r.left - c.x), r0: ed.view.rotation };
        try { st.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
        e.preventDefault();
        return;
      }
      if (e.button !== 0 && !(e.pointerType === "pen" && e.buttons)) return;
      // A click on the facing page of a spread goes to that page.
      const facing = ed.facing();
      if (facing && !["hand", "eyedropper"].includes(this.toolId)) {
        const q = ed.eventPoint(e);
        const inCurrent = q.x >= 0 && q.x <= ed.board.width && q.y >= 0 && q.y <= ed.board.height;
        const inFacing = q.x >= facing.dx && q.x <= facing.dx + ed.board.width && q.y >= 0 && q.y <= ed.board.height;
        // A spread drawn as one: drawing tools draw there too; panels and balloons belong to their page.
        const drawsAcross = !!ed.joinedFacing() && !["panel", "balloon", "move", "select"].includes(this.toolId);
        if (inFacing && !inCurrent && !drawsAcross && !(this.toolId === "panel" && ed.joinedFacing() && !ed.panelAt(q.x - facing.dx, q.y, facing.frame))) {
          ed.switchToFacing();
          e.preventDefault();
          return;
        }
      }
      // A pen's eraser end erases.
      if (e.pointerType === "pen" && (e.buttons & 32)) this.useTemporary("eraser");
      else if (e.altKey && ["pencil", "pen", "brush", "marker", "fill", "shape"].includes(this.toolId)) {
        const c = ed.sampleScreen(e.clientX - r.left, e.clientY - r.top);
        if (c) ed.setColor(c);
        return;
      }
      this.activePointer = e.pointerId;
      this.root.addClass("is-drawing");
      try { st.setPointerCapture(e.pointerId); } catch { /* synthetic or finished pointer */ }
      const p = ed.eventPoint(e);
      ed.pointer = p;
      ed.tool?.down?.(e, p);
      e.preventDefault();
    });
    st.addEventListener("pointermove", (e) => {
      if (this.panning) {
        ed.panBy(e.clientX - this.panning.x, e.clientY - this.panning.y);
        this.panning = { x: e.clientX, y: e.clientY };
        return;
      }
      if (this.rotating) {
        const r = st.getBoundingClientRect();
        const a = Math.atan2(e.clientY - r.top - r.height / 2, e.clientX - r.left - r.width / 2);
        let rot = this.rotating.r0 + a - this.rotating.a0;
        if (e.shiftKey) rot = Math.round(rot / (Math.PI / 12)) * (Math.PI / 12);
        ed.rotateTo(rot);
        return;
      }
      const p = ed.eventPoint(e);
      if (this.activePointer === e.pointerId) ed.tool?.move?.(e, p);
      else {
        ed.pointer = p;
        if (ed.tool?.hover) ed.tool.hover(e, p);
        else ed.drawOverlay();
      }
    });
    const end = (e: PointerEvent) => {
      if (this.panning || this.rotating) {
        this.panning = null;
        this.rotating = null;
        st.style.cursor = ed.tool?.cursor || "";
        return;
      }
      if (this.activePointer !== e.pointerId) return;
      this.activePointer = null;
      this.root.removeClass("is-drawing");
      if (e.type === "pointercancel") ed.tool?.cancel?.();
      else ed.tool?.up?.(e, ed.eventPoint(e));
      if (this.toolBeforeAlt) {
        this.setTool(this.toolBeforeAlt);
        this.toolBeforeAlt = null;
      }
    };
    st.addEventListener("pointerup", end);
    st.addEventListener("pointercancel", end);
    st.addEventListener("pointerleave", () => {
      if (this.activePointer === null) {
        ed.pointer = null;
        ed.drawOverlay();
      }
    });
    st.addEventListener("wheel", (e) => {
      e.preventDefault();
      const r = st.getBoundingClientRect();
      const px = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
      if (e.altKey) {
        ed.rotateTo(ed.view.rotation + (e.deltaY * px) / 600, e.clientX - r.left, e.clientY - r.top);
        return;
      }
      // A webtoon strip scrolls like it is read; Ctrl zooms.
      if (!e.ctrlKey && ed.board.kind === "webtoon") {
        ed.panBy(-(e.shiftKey ? e.deltaY : e.deltaX) * px, e.shiftKey ? 0 : -e.deltaY * px);
        return;
      }
      if (!e.ctrlKey && (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY))) {
        ed.panBy(-(e.deltaX || e.deltaY) * px, e.shiftKey ? 0 : -e.deltaY * px);
        return;
      }
      ed.zoomAt(Math.exp(-e.deltaY * px * (e.ctrlKey ? 0.01 : 0.0018)), e.clientX - r.left, e.clientY - r.top);
    }, { passive: false });
    st.addEventListener("contextmenu", (e) => e.preventDefault());
    // Pictures dropped on the canvas become picture layers (or references).
    st.addEventListener("dragover", (e) => e.preventDefault());
    st.addEventListener("drop", (e) => this.onDrop(e));
  }

  private useTemporary(id: string) {
    if (this.toolId === id) return;
    this.toolBeforeAlt = this.toolId;
    this.setTool(id);
  }

  private closeColorOnCanvas() {
    if (this.colorPop) this.toggleColor();
  }

  // ── keyboard ────────────────────────────────────────────────────────────

  private isActive() {
    return this.app.workspace.activeLeaf === this.leaf && this.ed && this.root?.isConnected;
  }

  private onKey(e: KeyboardEvent) {
    if (!this.isActive()) return;
    const target = e.target as HTMLElement;
    if (target?.closest?.("input, textarea, select, .cm-editor, [contenteditable]") || document.querySelector(".modal-container, .menu, .prompt")) return;
    const ed = this.ed!;
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key;
    const lower = k.toLowerCase();
    let used = true;
    if (!mod) this.keys.add(lower === " " ? " " : lower);
    if (ed.tool?.key?.(e)) {
      /* the tool used it */
    } else if (mod && lower === "z") e.shiftKey ? ed.redo() : ed.undo();
    else if (mod && lower === "y") ed.redo();
    else if (mod && lower === "s") ed.save();
    else if (mod && lower === "a") this.selectAll();
    else if (mod && lower === "d") ed.setSelection(null);
    else if (mod && lower === "t") this.transform();
    else if (mod && lower === "c") this.copy(false);
    else if (mod && lower === "x") this.copy(true);
    else if (mod && lower === "v") used = false; // the paste event handles it
    else if (mod && k === "Enter") ed.newFrame();
    else if (mod && e.shiftKey && lower === "n") ed.addLayer("raster");
    else if (mod) used = false;
    else if (k === " ") {
      if (!this.panning) this.stage.style.cursor = "grab";
    } else if (k === "ArrowLeft") this.refs?.step(-1);
    else if (k === "ArrowRight") this.refs?.step(1);
    else if (k === "ArrowUp" || k === ",") this.timeline?.go(ed.frameIndex - 1);
    else if (k === "ArrowDown" || k === ".") this.timeline?.go(ed.frameIndex + 1);
    else if (k === "Tab" && e.shiftKey) this.root.toggleClass("hide-timeline", !this.root.hasClass("hide-timeline"));
    else if (k === "Tab") this.setFocus(!this.focus);
    else if (k === "Home") this.timeline?.go(0);
    else if (k === "End") this.timeline?.go(ed.board.frames.length - 1);
    else if (k === "Enter") this.timeline?.toggle();
    else if (k === "Escape") {
      if (ed.floating) ed.cancelFloating();
      else if (ed.selection) ed.setSelection(null);
      else if (this.focus) this.setFocus(false);
      else used = false;
    } else if (k === "Delete" || k === "Backspace") ed.clearSelection();
    else if (k === "[" || k === "]" || k === "{" || k === "}") this.adjustBrush(k);
    else if (lower === "x") ed.swapColors();
    else if (lower === "d") {
      ed.setColor("#23221f");
      ed.secondary = "#ffffff";
      ed.trigger("color");
    } else if (lower === "f") ed.setMirror(!ed.view.mirror);
    else if (lower === "o" && !isPageKind(ed.board.kind)) this.timeline?.toggleOnion();
    else if (lower === "t" && e.shiftKey) this.cycleTraceMode();
    else if (lower === "t") this.toggleTrace();
    else if (lower === "q") this.peek(true);
    else if (k === "+" || k === "=") ed.zoomAt(1.25);
    else if (k === "-" || k === "_") ed.zoomAt(0.8);
    else if (k === "0") ed.fit();
    else if (k === "1") ed.setZoom(1);
    else if (lower === "r" && e.shiftKey) ed.rotateTo(0);
    else if (lower === "r") {
      /* held for rotating */
    } else {
      const t = toolsFor(ed.board.kind).find((x) => x.key.toLowerCase() === lower);
      if (t) this.setTool(t.id);
      else used = false;
    }
    if (used) {
      e.preventDefault();
      e.stopPropagation();
    }
  }

  private onKeyUp(e: KeyboardEvent) {
    const lower = e.key.toLowerCase();
    this.keys.delete(lower === " " ? " " : lower);
    if (!this.ed) return;
    if (lower === "q") this.peek(false);
    if (e.key === " " && !this.panning) this.stage.style.cursor = this.ed.tool?.cursor || "";
  }

  private adjustBrush(k: string) {
    const b = this.settings.brushes[this.toolId as BrushKind];
    if (!b) return;
    if (k === "[") b.size = Math.max(0.5, b.size / 1.15);
    if (k === "]") b.size = Math.min(500, b.size * 1.15);
    if (k === "{") b.opacity = Math.max(0.02, b.opacity - 0.1);
    if (k === "}") b.opacity = Math.min(1, b.opacity + 0.1);
    this.saveToolSettings();
    this.renderOptions();
    this.ed!.drawOverlay();
  }

  // ── clipboard and drops ─────────────────────────────────────────────────

  private async copy(cut: boolean) {
    const ed = this.ed!;
    const c = ed.copyPixels();
    if (!c) return;
    this.clipboard = c;
    try {
      const blob: Blob | null = await new Promise((r) => c.canvas.toBlob(r, "image/png"));
      const CI = (window as any).ClipboardItem;
      if (blob && CI && navigator.clipboard?.write) await navigator.clipboard.write([new CI({ "image/png": blob })]);
    } catch {
      /* the internal clipboard still works */
    }
    if (cut) {
      if (!ed.selection) this.selectAll();
      ed.clearSelection();
    }
  }

  private async onPaste(e: ClipboardEvent) {
    if (!this.isActive()) return;
    const target = e.target as HTMLElement;
    if (target?.closest?.("input, textarea, .cm-editor, [contenteditable]")) return;
    if (this.refs && this.refs.el.contains(document.activeElement)) return; // the reference panel takes it
    const ed = this.ed!;
    let canvas: HTMLCanvasElement | null = null;
    let at: { x: number; y: number } | null = null;
    for (const item of Array.from(e.clipboardData?.items || [])) {
      if (!item.type.startsWith("image/")) continue;
      const f = item.getAsFile();
      if (!f) continue;
      const bmp = await createImageBitmap(f);
      canvas = document.createElement("canvas");
      canvas.width = bmp.width;
      canvas.height = bmp.height;
      canvas.getContext("2d")!.drawImage(bmp, 0, 0);
      break;
    }
    if (!canvas && this.clipboard) {
      canvas = this.clipboard.canvas;
      at = { x: this.clipboard.x + canvas.width / 2, y: this.clipboard.y + canvas.height / 2 };
    }
    if (!canvas) return;
    e.preventDefault();
    const l = ed.addLayer("raster", { name: "Pasted" });
    const r = ed.visibleRegion();
    const k = Math.min(1, (r.w * 0.8) / canvas.width, (r.h * 0.8) / canvas.height);
    ed.startFloating(l.id, canvas, { x: at?.x ?? r.x + r.w / 2, y: at?.y ?? r.y + r.h / 2, sx: k, sy: k, rotation: 0 });
    this.setTool("move");
  }

  private async onDrop(e: DragEvent) {
    e.preventDefault();
    const ed = this.ed!;
    const dt = e.dataTransfer;
    if (!dt) return;
    const refId = dt.getData("text/x-ateliet-ref");
    if (refId) {
      const l = await this.layers!.placePicture(refId);
      if (l?.transform) {
        const p = ed.eventPoint(e);
        ed.mutate("Place picture", () => {
          l.transform!.x = p.x;
          l.transform!.y = p.y;
        });
      }
      return;
    }
    // Anything else becomes a reference first, then a picture layer.
    const before = ed.board.references.length;
    await this.refs!.drop(e);
    const added = ed.board.references.slice(before);
    for (const r of added) await this.layers!.placePicture(r.id);
  }

  // ── tracing ─────────────────────────────────────────────────────────────

  private traceLayers(): Layer[] {
    return this.ed!.layers.filter((l) => l.kind === "trace");
  }

  private updateTraceButton() {
    if (!this.traceBtn || !this.ed?.frame) return;
    const t = this.traceLayers();
    this.traceBtn.toggleClass("is-active", t.some((l) => l.visible));
    this.traceBtn.toggleClass("has-trace", t.length > 0);
  }

  /**
   * Traces a reference: one trace layer at the bottom of the frame (replaced
   * if there is one), fitted to the frame, half transparent and locked, and
   * a drawing layer above it, selected, so the next stroke lands on it.
   */
  async trace(ref?: Reference) {
    const ed = this.ed!;
    if (!ref) {
      ref = this.refs?.current || undefined;
      if (!ref) return this.chooseReference((id) => this.trace(ed.doc.refs.ref(id)));
    }
    const img = await ed.doc.refs.image(ref);
    if (!img) return new Notice("That picture could not be found.");
    ed.comp.images.set(ref.id, img);
    const t = fitTransform(ed, img.naturalWidth, img.naturalHeight, false);
    const existing = this.traceLayers()[0];
    let drawOn: string | null = null;
    ed.mutate("Trace picture", (b) => {
      if (existing) {
        existing.src = ref!.id;
        existing.transform = t;
        existing.visible = true;
        existing.name = `Trace · ${ref!.name}`;
      } else {
        const l = newLayer(`Trace · ${ref!.name}`, "trace", { src: ref!.id, transform: t, opacity: 0.45, locked: true, trace: { ...defaultTrace(), mode: this.lastTraceMode } });
        b.layers[l.id] = l;
        ed.frame.layers.unshift(l.id);
      }
      // Draw on the lowest empty drawing layer above the trace, or a new one.
      const empty = ed.frame.layers.map((id) => b.layers[id]).find((l) => l.kind === "raster" && !l.locked && !ed.doc.tiles.tilesOf(l.id).length);
      if (empty) drawOn = empty.id;
      else {
        const n = newLayer("Tracing");
        b.layers[n.id] = n;
        ed.frame.layers.push(n.id);
        drawOn = n.id;
      }
      ed.frame.active = drawOn!;
      ed.frame.reference = ref!.id;
    });
    ed.comp.want(ed.frame);
    if (!["pencil", "pen", "brush", "marker"].includes(this.toolId)) this.setTool("pencil");
    this.updateTraceButton();
    new Notice("Tracing. T hides or shows the picture, hold Q to look without it, Shift+T switches photo / lines / values / blue pencil.", 6000);
  }

  private toggleTrace() {
    const ed = this.ed!;
    const t = this.traceLayers();
    if (!t.length) return this.trace();
    const show = !t.some((l) => l.visible);
    ed.mutate(show ? "Show trace" : "Hide trace", () => t.forEach((l) => (l.visible = show)));
  }

  private cycleTraceMode() {
    const ed = this.ed!;
    const t = this.traceLayers();
    if (!t.length) return;
    const modes = TRACE_MODES.map((m) => m[0]) as TraceMode[];
    const next = modes[(modes.indexOf(t[0].trace?.mode || "photo") + 1) % modes.length];
    this.lastTraceMode = next;
    ed.mutate("Trace display", () => t.forEach((l) => ((l.trace ||= defaultTrace()).mode = next)));
    new Notice(`Trace: ${TRACE_MODES.find((m) => m[0] === next)![1]}`, 1500);
  }

  /** Holding Q: the drawing alone, without the trace. */
  private peek(on: boolean) {
    const ed = this.ed!;
    const ids = this.traceLayers().map((l) => l.id);
    if (on) ids.forEach((id) => ed.hidden.add(id));
    else ids.forEach((id) => ed.hidden.delete(id));
    ed.invalidate();
  }

  private traceMenu(e: MouseEvent) {
    const ed = this.ed!;
    const t = this.traceLayers();
    const m = new Menu();
    m.addItem((i) => i.setTitle(this.refs?.current ? `Trace "${this.refs.current.name}"` : "Trace a reference…").setIcon("scan-line").onClick(() => this.trace()));
    m.addItem((i) => i.setTitle("Trace another reference…").setIcon("images").onClick(() => this.chooseReference((id) => this.trace(ed.doc.refs.ref(id)))));
    if (t.length) {
      const l = t[0];
      m.addSeparator();
      m.addItem((i) => i.setTitle(l.visible ? "Hide the trace (T)" : "Show the trace (T)").setIcon(l.visible ? "eye-off" : "eye").onClick(() => this.toggleTrace()));
      for (const [mode, name, desc] of TRACE_MODES) m.addItem((i) => i.setTitle(`${name} — ${desc}`).setChecked(l.trace?.mode === mode).onClick(() => {
        this.lastTraceMode = mode as TraceMode;
        ed.mutate("Trace display", () => ((l.trace ||= defaultTrace()).mode = mode as TraceMode));
      }));
      m.addSeparator();
      for (const v of [0.25, 0.45, 0.7, 1]) m.addItem((i) => i.setTitle(`Opacity ${Math.round(v * 100)}%`).setChecked(Math.abs(l.opacity - v) < 0.02).onClick(() => ed.mutate("Trace opacity", () => (l.opacity = v))));
      m.addSeparator();
      m.addItem((i) => i.setTitle("Light table: show over the drawing").setChecked(!!l.trace?.above).onClick(() => ed.mutate("Trace over drawing", () => ((l.trace ||= defaultTrace()).above = !l.trace.above))));
      m.addItem((i) => i.setTitle(l.locked ? "Unlock to move / scale it (then V)" : "Lock in place").setIcon(l.locked ? "unlock" : "lock").onClick(() => {
        ed.mutate(l.locked ? "Unlock trace" : "Lock trace", () => (l.locked = !l.locked));
        if (!l.locked) {
          ed.setActive(l.id);
          this.setTool("move");
        }
      }));
      m.addItem((i) => i.setTitle("Fit to the frame").setIcon("minimize-2").onClick(() => {
        const s = ed.pictureSize(l.src!);
        if (s) ed.mutate("Fit trace", () => (l.transform = fitTransform(ed, s[0], s[1], false)));
      }));
      m.addItem((i) => i.setTitle("Fill the frame").setIcon("maximize-2").onClick(() => {
        const s = ed.pictureSize(l.src!);
        if (s) ed.mutate("Fill frame with trace", () => (l.transform = fitTransform(ed, s[0], s[1], true)));
      }));
      m.addItem((i) => i.setTitle("Include in exported frames").setChecked(!!l.trace?.export).onClick(() => ed.mutate("Trace in exports", () => ((l.trace ||= defaultTrace()).export = !l.trace.export))));
      m.addItem((i) => i.setTitle("Same trace on every frame").setIcon("layers").setChecked(!!l.shared).onClick(() => this.layers!.toggleShared(l)));
      m.addSeparator();
      m.addItem((i) => i.setTitle("Remove the trace").setIcon("trash-2").onClick(() => this.layers!.remove(l)));
    }
    m.showAtMouseEvent(e);
  }

  /** Picks one of the references (or adds pictures first when there are none). */
  chooseReference(cb: (id: string) => void) {
    const ed = this.ed!;
    const list = ed.board.references;
    if (!list.length) return new Notice("Add references first (the picture button in the References panel).");
    const m = new Modal(this.app);
    m.setTitle("Choose a reference");
    m.modalEl.addClass("sb-choose-modal");
    const grid = m.contentEl.createDiv("sb-search-grid");
    for (const r of list) {
      const t = grid.createDiv({ cls: "sb-search-item", attr: { "aria-label": r.name } });
      t.createEl("img", { attr: { src: ed.doc.refs.url(r) || "", loading: "lazy" } });
      t.addEventListener("click", () => {
        m.close();
        cb(r.id);
      });
    }
    m.open();
  }

  // ── menus ───────────────────────────────────────────────────────────────

  private guidesMenu(e: MouseEvent) {
    const ed = this.ed!;
    const g = ed.board.guides;
    const set = (fn: () => void) => {
      fn();
      ed.changed();
      ed.drawOverlay();
      ed.trigger("guides");
    };
    const m = new Menu();
    m.addItem((i) => i.setTitle("Rule of thirds").setChecked(g.thirds).onClick(() => set(() => (g.thirds = !g.thirds))));
    m.addItem((i) => i.setTitle("Centre").setChecked(g.center).onClick(() => set(() => (g.center = !g.center))));
    m.addItem((i) => i.setTitle("Safe areas (title and action)").setChecked(g.safe).onClick(() => set(() => (g.safe = !g.safe))));
    m.addSeparator();
    m.addItem((i) => i.setTitle("Grid (also over the reference)").setIsLabel(true));
    for (const n of [0, 3, 4, 6, 8, 12, 16]) m.addItem((i) => i.setTitle(n ? `${n} squares across` : "No grid").setChecked(g.grid === n).onClick(() => set(() => (g.grid = n))));
    m.addSeparator();
    m.addItem((i) => i.setTitle("Mirror the view (F)").setChecked(ed.view.mirror).onClick(() => ed.setMirror(!ed.view.mirror)));
    m.addItem((i) => i.setTitle("Mirror the reference").onClick(() => this.refs?.toggleMirror()));
    m.addItem((i) => i.setTitle("Reset rotation (Shift+R)").onClick(() => ed.rotateTo(0)));
    m.showAtMouseEvent(e);
  }

  private importMenu(e: MouseEvent) {
    const m = new Menu();
    m.addItem((i) => i.setTitle("Clip Studio file (.clip) as layers of this frame…").setIcon("layers").onClick(() => this.importClip(false)));
    m.addItem((i) => i.setTitle("Clip Studio file (.clip) as a new frame…").setIcon("square-plus").onClick(() => this.importClip(true)));
    m.addSeparator();
    m.addItem((i) => i.setTitle("Pictures as layers of this frame…").setIcon("image-plus").onClick(() => this.importPictures(false)));
    m.addItem((i) => i.setTitle("Pictures as new frames (one each)…").setIcon("images").onClick(() => this.importPictures(true)));
    m.addItem((i) => i.setTitle("Pictogrep's storyboard drawings as frames").setIcon("clapperboard").onClick(() => this.importPictogrep()));
    m.showAtMouseEvent(e);
  }

  async importClip(asFrame: boolean, path?: string) {
    const ed = this.ed!;
    if (!path) {
      const picked: string[] = await invoke("pick_files", { name: "Clip Studio", extensions: ["clip"], multiple: false });
      path = picked[0];
    }
    if (!path) return;
    const notice = new Notice("Reading the Clip Studio file…", 0);
    try {
      const clip = await readClip(path);
      const before = cloneBoard(ed.board);
      let frameId = ed.frame.id;
      let at = ed.frame.layers.indexOf(ed.frame.active) + 1;
      if (asFrame) {
        const f = addFrame(ed.board, ed.frameIndex + 1);
        frameId = f.id;
        at = 0;
      }
      const name = path.split(/[\\/]/).pop()!;
      // A file of another size is centred on the frame.
      const b = ed.board;
      const dx = b.infinite ? 0 : (b.width - clip.width) / 2, dy = b.infinite ? 0 : (b.height - clip.height) / 2;
      const res = await addClipLayers(ed.doc, clip, frameId, at, { paperAsBackground: false, dx, dy });
      if (asFrame) ed.doc.frame(frameId)!.name = name.replace(/\.clip$/i, "");
      ed.recordSince("Import Clip Studio file", before);
      await ed.setFrame(ed.doc.frame(frameId)!);
      await ed.refreshFrame();
      ed.changed(true);
      notice.hide();
      const sizeNote = !ed.board.infinite && (clip.width !== ed.board.width || clip.height !== ed.board.height) ? ` The file is ${clip.width} × ${clip.height}, centred on the ${ed.board.width} × ${ed.board.height} frame.` : "";
      new Notice(`${res.layers.length} layers imported from ${name}.${sizeNote}${importSummary(clip, res.approximated)}`, 10000);
    } catch (err: any) {
      notice.hide();
      new Notice(`Could not import the Clip Studio file: ${err?.message || err}`);
    }
  }

  private async importPictures(asFrames: boolean) {
    const ed = this.ed!;
    const paths: string[] = await invoke("pick_files", { name: "Pictures", extensions: ["png", "jpg", "jpeg", "webp", "gif"], multiple: true });
    if (!paths.length) return;
    const have = new Map(ed.board.references.map((r) => [r.path, r.id]));
    const refs: Reference[] = paths.map((p) => ({ id: have.get(p) || uid(), name: p.split(/[\\/]/).pop()!, path: p, from: "computer" }));
    ed.mutate("Add pictures", (b) => {
      for (const r of refs) if (!have.has(r.path)) b.references.push(r);
    });
    if (!asFrames) {
      for (const r of refs) await this.layers!.placePicture(r.id);
      return;
    }
    for (const r of refs) {
      await ed.newFrame(true);
      await this.layers!.placePicture(r.id);
      ed.frame.reference = r.id;
    }
    this.refs?.render();
  }

  /** Pictogrep's drawings become frames: the drawing on a layer, its source as the reference. */
  async importPictogrep() {
    const ed = this.ed!;
    let data: any;
    try {
      data = await invoke("pictogrep_boards");
    } catch (e) {
      return new Notice(String(e));
    }
    const boards: any[] = data.boards || [];
    if (!boards.length) return new Notice("Pictogrep's storyboards folder has no drawings.");
    const notice = new Notice(`Importing ${boards.length} drawings…`, 0);
    const before = cloneBoard(ed.board);
    for (const b of boards) {
      const img = await new Promise<HTMLImageElement | null>((res) => {
        const i = new Image();
        i.crossOrigin = "anonymous";
        i.onload = () => res(i);
        i.onerror = () => res(null);
        i.src = fileUrl(b.abs);
      });
      if (!img) continue;
      await ed.newFrame(true);
      const l = ed.active!;
      const k = ed.board.infinite ? 1 : Math.min(ed.board.width / img.naturalWidth, ed.board.height / img.naturalHeight);
      const w = img.naturalWidth * k, h = img.naturalHeight * k;
      const x = ed.board.infinite ? 0 : (ed.board.width - w) / 2, y = ed.board.infinite ? 0 : (ed.board.height - h) / 2;
      ed.doc.tiles.paint(l.id, Math.floor(x), Math.floor(y), Math.ceil(w), Math.ceil(h), (ctx) => ctx.drawImage(img, x - Math.floor(x), y - Math.floor(y), w, h));
      ed.frame.name = String(b.name).replace(/\.[a-z]+$/i, "");
      if (b.source) {
        const id = ed.board.references.find((r) => r.path === b.source)?.id || uid();
        if (!ed.board.references.some((r) => r.id === id)) ed.board.references.push({ id, name: String(b.source).split(/[\\/]/).pop()!, path: b.source, from: "Pictogrep" });
        ed.frame.reference = id;
      }
      ed.scheduleThumb(ed.frame);
    }
    notice.hide();
    ed.recordSince("Import Pictogrep drawings", before);
    await ed.refreshFrame();
    ed.changed(true);
    this.refs?.render();
  }

  private exportMenu(e: MouseEvent) {
    const kind = this.ed!.board.kind;
    if (isPageKind(kind)) return this.pageExportMenu(e);
    const m = new Menu();
    m.addItem((i) => i.setTitle("Board pages to print or send (PDF, six shots per page)").setIcon("file-text").onClick(() => this.exportPages("pdf")));
    m.addItem((i) => i.setTitle("Frames as PNG pictures in the vault…").setIcon("images").onClick(() => this.exportPng()));
    m.addItem((i) => i.setTitle("Contact sheet (all frames with notes)…").setIcon("layout-grid").onClick(() => this.exportSheet()));
    m.addItem((i) => i.setTitle("Into Pictogrep's storyboards folder").setIcon("clapperboard").onClick(() => this.exportPictogrep()));
    m.addItem((i) => i.setTitle("Animation as an animated PNG (plays in notes)").setIcon("film").onClick(() => this.exportAnimation()));
    m.addItem((i) => i.setTitle(canRecordVideo() ? "Video (WebM)" : "Video (this system has no video encoder)").setIcon("clapperboard").setDisabled(!canRecordVideo()).onClick(() => this.exportVideo()));
    m.addSeparator();
    m.addItem((i) => i.setTitle("This frame as a picture in the vault").setIcon("image").onClick(() => this.exportCurrent()));
    m.addItem((i) => i.setTitle("Copy this frame").setIcon("copy").onClick(() => this.copyFrame()));
    m.showAtMouseEvent(e);
  }

  private pageExportMenu(e: MouseEvent) {
    const kind = this.ed!.board.kind;
    const unit = kindInfo(kind).units;
    const m = new Menu();
    if (kind === "thumbnails") m.addItem((i) => i.setTitle("Send the panels to an animatic…").setIcon("send").onClick(() => this.sendToAnimatic()));
    if (kind === "webtoon") {
      m.addItem((i) => i.setTitle("Slices for upload (PNG, 1280 px high)").setIcon("scissors").onClick(() => this.exportPages("slices")));
      m.addItem((i) => i.setTitle("Phone preview").setIcon("smartphone").onClick(() => this.phonePreview()));
    }
    m.addItem((i) => i.setTitle(`${unit[0].toUpperCase() + unit.slice(1)} as PDF (print size)`).setIcon("file-text").onClick(() => this.exportPages("pdf")));
    m.addItem((i) => i.setTitle(`${unit[0].toUpperCase() + unit.slice(1)} as PNG pictures in the vault…`).setIcon("images").onClick(() => this.exportPng()));
    m.addItem((i) => i.setTitle("Into Pictogrep's storyboards folder").setIcon("clapperboard").onClick(() => this.exportPictogrep()));
    m.addSeparator();
    m.addItem((i) => i.setTitle(`This ${kindInfo(kind).unit} as a picture in the vault`).setIcon("image").onClick(() => this.exportCurrent()));
    m.addItem((i) => i.setTitle(`Copy this ${kindInfo(kind).unit}`).setIcon("copy").onClick(() => this.copyFrame()));
    m.showAtMouseEvent(e);
  }

  private baseName() {
    return this.file?.basename || "Storyboard";
  }

  private exportFolder() {
    const parent = this.file?.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
    return `${parent}${this.baseName()} frames`;
  }

  private async exportPng() {
    const ed = this.ed!;
    const o = { folder: this.exportFolder(), scale: 1, withTrace: false, transparent: false };
    const m = new Modal(this.app);
    m.setTitle("Export frames");
    new Setting(m.contentEl).setName("Vault folder").addText((t) => t.setValue(o.folder).onChange((v) => (o.folder = v.trim().replace(/^\/+|\/+$/g, ""))));
    new Setting(m.contentEl).setName("Size").addDropdown((d) => d.addOptions({ "0.5": "Half", "1": "Full", "2": "Double" }).setValue("1").onChange((v) => (o.scale = Number(v))));
    new Setting(m.contentEl).setName("Include trace pictures").addToggle((t) => t.setValue(false).onChange((v) => (o.withTrace = v)));
    new Setting(m.contentEl).setName("Transparent instead of paper").addToggle((t) => t.setValue(false).onChange((v) => (o.transparent = v)));
    const b = m.modalEl.createDiv("modal-button-container");
    b.createEl("button", { text: "Export", cls: "mod-cta" }).addEventListener("click", async () => {
      m.close();
      const n = new Notice("Exporting…", 0);
      try {
        const files = await exportFrames(ed, o.folder, o, (i, t) => n.setMessage(`Exporting ${i + 1}/${t}…`));
        n.hide();
        new Notice(`${files.length} frames saved in ${o.folder || "the vault"}.`);
      } catch (err) {
        n.hide();
        new Notice(`Export failed: ${err}`);
      }
    });
    m.open();
  }

  private async exportSheet() {
    const ed = this.ed!;
    const parent = this.file?.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
    const o = { path: `${parent}${this.baseName()} contact sheet.png`, columns: 3, notes: true, width: 2400, insert: false };
    const m = new Modal(this.app);
    m.setTitle("Contact sheet");
    new Setting(m.contentEl).setName("Save as").addText((t) => t.setValue(o.path).onChange((v) => (o.path = v.trim())));
    new Setting(m.contentEl).setName("Frames per row").addDropdown((d) => d.addOptions({ "2": "2", "3": "3", "4": "4", "5": "5", "6": "6" }).setValue("3").onChange((v) => (o.columns = Number(v))));
    new Setting(m.contentEl).setName("With notes (action, dialogue, sound)").addToggle((t) => t.setValue(true).onChange((v) => (o.notes = v)));
    new Setting(m.contentEl).setName("Width").addDropdown((d) => d.addOptions({ "1600": "1600 px", "2400": "2400 px", "3508": "3508 px (A4 at 300 dpi)" }).setValue("2400").onChange((v) => (o.width = Number(v))));
    const b = m.modalEl.createDiv("modal-button-container");
    b.createEl("button", { text: "Save", cls: "mod-cta" }).addEventListener("click", async () => {
      m.close();
      const n = new Notice("Making the contact sheet…", 0);
      try {
        const f = await saveContactSheet(ed, o.path.endsWith(".png") ? o.path : o.path + ".png", o);
        n.hide();
        new Notice(`Saved ${f.path}.`);
        this.app.workspace.getLeaf("tab").openFile(f);
      } catch (err) {
        n.hide();
        new Notice(`Could not make the contact sheet: ${err}`);
      }
    });
    m.open();
  }

  private async exportPictogrep() {
    const n = new Notice("Exporting to Pictogrep…", 0);
    try {
      const count = await exportToPictogrep(this.ed!, (i, t) => n.setMessage(`Exporting to Pictogrep ${i + 1}/${t}…`));
      n.hide();
      new Notice(`${count} frames are now in Pictogrep's storyboards folder.`);
    } catch (err) {
      n.hide();
      new Notice(`Could not export to Pictogrep: ${err}`);
    }
  }

  private async exportAnimation() {
    const n = new Notice("Preparing the animation…", 0);
    try {
      const bytes = await animatedPng(this.ed!, 1, (s) => n.setMessage(s));
      const parent = this.file?.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
      const f = await writeVault(this.app, `${parent}${this.baseName()} animation.png`, bytes);
      n.hide();
      new Notice(`Saved ${f.path}. Embed it in a note to watch it play.`);
    } catch (err) {
      n.hide();
      new Notice(`Could not make the animation: ${err}`);
    }
  }

  private async exportVideo() {
    const ed = this.ed!;
    const n = new Notice("Preparing the video…", 0);
    try {
      const { bytes, ext } = await recordVideo(ed, 1, (s) => n.setMessage(s));
      const parent = this.file?.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
      const f = await writeVault(this.app, `${parent}${this.baseName()}.${ext}`, bytes);
      n.hide();
      new Notice(`Saved ${f.path}.`);
    } catch (err) {
      n.hide();
      new Notice(String(err));
    }
  }

  private async exportCurrent() {
    const ed = this.ed!;
    const pic = await ed.comp.picture(ed.frame, 1, { exporting: true });
    const parent = this.file?.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
    const f = await writeVault(this.app, `${parent}${this.baseName()} ${ed.frameIndex + 1}.png`, await canvasPng(pic));
    new Notice(`Saved ${f.path}.`);
  }

  private async copyFrame() {
    const ed = this.ed!;
    const pic = await ed.comp.picture(ed.frame, 1, { exporting: true });
    try {
      const blob: Blob | null = await new Promise((r) => pic.toBlob(r, "image/png"));
      await navigator.clipboard.write([new (window as any).ClipboardItem({ "image/png": blob })]);
      new Notice("Frame copied.");
    } catch {
      this.clipboard = { canvas: pic, x: 0, y: 0 };
      new Notice("Copied inside Ateliet (the system clipboard refused the picture).");
    }
  }

  private async editSettings() {
    const ed = this.ed!;
    const b = ed.board;
    const modal = new BoardOptionsModal(this.app, { kind: b.kind, name: this.baseName(), width: b.width, height: b.height, infinite: b.infinite, background: b.background, fps: b.fps, rtl: !!b.page?.rtl, spreads: !!b.page?.spreads }, "edit");
    modal.open();
    const res = await modal.result;
    if (!res) return;
    const kindChanged = res.kind !== b.kind;
    const timed = res.kind === "animatic" || res.kind === "presentation";
    const dw = timed ? res.width - b.width : 0, dh = timed ? res.height - b.height : 0;
    const shift = !timed || b.infinite || res.infinite ? { x: 0, y: 0 } : res.anchor === "center" ? { x: Math.round(dw / 2), y: Math.round(dh / 2) } : res.anchor === "top" ? { x: Math.round(dw / 2), y: 0 } : res.anchor === "left" ? { x: 0, y: Math.round(dh / 2) } : { x: 0, y: 0 };
    ed.mutate("Storyboard settings", (bd) => {
      bd.kind = res.kind;
      bd.background = res.background;
      if (timed) {
        bd.width = res.width;
        bd.height = res.height;
        bd.infinite = res.infinite;
        bd.fps = res.fps;
      } else {
        bd.infinite = false;
        // Becoming pages: a page setup from the current canvas.
        if (!bd.page) {
          const m = Math.round(Math.min(bd.width, bd.height) * 0.06);
          bd.page = { dpi: 150, bleed: 0, inner: { x: m, y: m, w: bd.width - 2 * m, h: bd.height - 2 * m }, rtl: res.kind === "name", spreads: res.kind === "name" || res.kind === "comic", gutterX: 12, gutterY: 24, border: 4, mask: true, clipToPanel: true, slice: 1280 };
        }
        bd.page.rtl = res.rtl;
        bd.page.spreads = res.spreads && res.kind !== "webtoon" && res.kind !== "thumbnails";
        // Frames that become pages start as one panel over the inner frame (a webtoon starts empty).
        for (const f of bd.frames) {
          if (!f.panels?.length) f.panels = startPanels(bd);
          f.balloons ||= [];
        }
      }
      if (shift.x || shift.y) {
        for (const l of Object.values(bd.layers)) {
          if (l.kind === "raster") {
            l.x += shift.x;
            l.y += shift.y;
          } else if (l.transform) {
            l.transform.x += shift.x;
            l.transform.y += shift.y;
          }
        }
      }
    });
    if (res.name !== this.baseName() && this.file) {
      const parent = this.file.parent && !this.file.parent.isRoot() ? this.file.parent.path + "/" : "";
      await this.app.fileManager.renameFile(this.file, `${parent}${res.name}.storyboard`).catch((e: any) => new Notice(String(e)));
    }
    if (kindChanged) {
      // Another kind brings another workspace (tools, timeline, exports).
      await this.reload();
      return;
    }
    await ed.refreshFrame();
    ed.fit();
    ed.scheduleThumb();
  }

  /** Saves and builds the view again from the file. */
  private async reload() {
    if (!this.file) return;
    await this.close();
    this.contentEl.empty();
    const doc = await StoryDoc.open(this.app.vault.adapter.getFullPath(this.file.path));
    this.build(doc);
  }
}

export { STORYBOARD_VIEW as VIEW_TYPE };
