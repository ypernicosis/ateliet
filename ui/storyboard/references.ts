// References: the pictures you draw from, beside the frame. One is shown
// large (zoom with the wheel, drag to move, double click for real size), the
// rest wait in a strip below; ← and → go through them. A whole moodboard
// comes in at once: a collection of the shared library, a vault folder, the
// pictures of a note, an Obsidian canvas, Pictogrep's storyboard references,
// files from the computer, the clipboard, or a search by description.
//
// References point at the pictures where they are; pasted or dropped files
// are kept inside the storyboard (refs/…).

import { Editor } from "./editor.ts";
import { Reference, uid } from "./model.ts";
import { PanZoom } from "../images/zoom.ts";
import { ImageItem, api, thumbnailUrl } from "../images/api.ts";
import { setIcon } from "../obsidian/icons.ts";
import { FuzzySuggestModal, Menu, Modal, Notice } from "../obsidian/ui.ts";
import { TFile, TFolder } from "../obsidian/vault.ts";
import { invoke, readBytes } from "../tauri.ts";

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|bmp|avif)$/i;

export interface ReferenceHost {
  trace(ref: Reference): void;
  place(ref: Reference): void;
}

export class ReferencePanel {
  el: HTMLElement;
  private stage!: HTMLElement;
  private content!: HTMLElement;
  private img!: HTMLImageElement;
  private grid!: HTMLCanvasElement;
  private strip!: HTMLElement;
  private title!: HTMLElement;
  private counter!: HTMLElement;
  private zoom: PanZoom | null = null;
  private empty!: HTMLElement;
  current: Reference | null = null;
  private mirrored = false;
  private actions!: HTMLElement;
  private mirrorBtn!: HTMLElement;
  private stripKey = "";
  private off: (() => void)[] = [];

  constructor(private app: any, private ed: Editor, parent: HTMLElement, private host: ReferenceHost) {
    this.el = parent.createDiv("sb-refs");
    const head = this.el.createDiv("sb-panel-head");
    head.createDiv({ cls: "sb-panel-title", text: "References" });
    this.counter = head.createDiv("sb-refs-counter");
    const add = head.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Add references (moodboard, folder, note, search…)" } });
    setIcon(add, "image-plus");
    add.addEventListener("click", (e) => this.addMenu(e));
    const more = head.createDiv({ cls: "clickable-icon", attr: { "aria-label": "More" } });
    setIcon(more, "more-horizontal");
    more.addEventListener("click", (e) => this.currentMenu(e));

    this.stage = this.el.createDiv("sb-refs-stage");
    this.content = this.stage.createDiv();
    this.img = this.content.createEl("img", { attr: { draggable: "false", alt: "" } });
    this.img.crossOrigin = "anonymous";
    this.grid = this.content.createEl("canvas", { cls: "sb-refs-grid" });
    this.empty = this.stage.createDiv({ cls: "sb-refs-empty" });
    this.empty.createDiv({ text: "No references yet." });
    const b = this.empty.createEl("button", { text: "Add references…", cls: "mod-cta" });
    b.addEventListener("click", (e) => this.addMenu(e));
    this.empty.createDiv({ cls: "sb-hint", text: "Bring a collection, a folder, a note's pictures or a canvas; or drop and paste pictures here." });
    this.zoom = new PanZoom(this.stage, this.content, { margin: 8 });
    // A reference is there to be looked at: small pictures are enlarged to the panel.
    this.zoom.upscale = true;
    // Previous / next over the picture, where the eye already is.
    for (const [cls, icon, label, d] of [["sb-refs-prev", "chevron-left", "Previous reference (←)", -1], ["sb-refs-next", "chevron-right", "Next reference (→)", 1]] as const) {
      const b = this.stage.createDiv({ cls: `sb-refs-nav ${cls} pz-ignore`, attr: { "aria-label": label } });
      setIcon(b, icon);
      b.addEventListener("click", () => this.step(d));
      b.addEventListener("dblclick", (e) => e.stopPropagation());
    }
    this.actions = this.el.createDiv("sb-refs-actions");
    const act = (icon: string, label: string, fn: () => void) => {
      const b = this.actions.createDiv({ cls: "clickable-icon", attr: { "aria-label": label } });
      setIcon(b, icon);
      b.addEventListener("click", fn);
      return b;
    };
    act("scan-line", "Trace this picture (adds a trace layer and a layer to draw on)", () => this.current && this.host.trace(this.current));
    act("image-plus", "Place it on the frame as a picture layer", () => this.current && this.host.place(this.current));
    this.mirrorBtn = act("flip-horizontal", "Mirror the reference", () => this.toggleMirror());
    act("maximize", "Whole picture (double click: real size)", () => this.zoom?.fit());
    act("pipette", "Alt+click the picture takes its colour", () => this.ed.trigger("notice", "Alt+click the reference (or use the colour picker tool, I) to take a colour from it."));
    this.title = this.actions.createDiv("sb-refs-title");
    this.strip = this.el.createDiv("sb-refs-strip");

    // Alt+click (or the colour picker tool) takes a colour from the reference.
    this.stage.addEventListener("pointerdown", (e) => {
      if (!(e.altKey || this.ed.tool?.id === "eyedropper") || !this.current) return;
      e.stopImmediatePropagation();
      e.preventDefault();
      const c = this.sample(e);
      if (c) this.ed.setColor(c);
    }, true);
    this.stage.addEventListener("dragover", (e) => e.preventDefault());
    this.stage.addEventListener("drop", (e) => this.drop(e));
    this.strip.addEventListener("dragover", (e) => e.preventDefault());
    this.strip.addEventListener("drop", (e) => this.drop(e));
    this.el.addEventListener("paste", (e) => this.paste(e as ClipboardEvent));
    this.el.tabIndex = -1;

    const onFrame = () => {
      const r = this.ed.doc.refs.ref(this.ed.frame.reference);
      if (r && r !== this.current) this.show(r, false);
    };
    const onBoard = () => this.render();
    const onGuides = () => this.drawGrid();
    this.ed.on("frame", onFrame);
    this.ed.on("board", onBoard);
    this.ed.on("guides", onGuides);
    this.off.push(() => this.ed.off("frame", onFrame), () => this.ed.off("board", onBoard), () => this.ed.off("guides", onGuides));
    const first = this.ed.doc.refs.ref(this.ed.frame?.reference) || this.ed.board.references[0];
    this.render();
    if (first) this.show(first, false);
  }

  destroy() {
    this.off.forEach((f) => f());
    this.zoom?.destroy();
  }

  get list(): Reference[] {
    return this.ed.board.references;
  }

  render() {
    const list = this.list;
    if (this.current && !list.includes(this.current)) this.current = list.find((r) => r.id === this.current!.id) || null;
    this.empty.toggle(!list.length);
    this.content.toggle(!!list.length);
    if (!this.current && list.length) this.show(list[0], false);
    this.counter.setText(list.length ? `${this.current ? list.indexOf(this.current) + 1 : 0} / ${list.length}` : "");
    this.actions.toggle(!!list.length);
    this.stage.toggleClass("has-many", list.length > 1);
    // The strip is rebuilt only when the list changes (it can hold hundreds).
    const key = list.map((r) => r.id + r.path).join("|");
    if (key === this.stripKey) return;
    this.stripKey = key;
    this.strip.empty();
    for (const r of list) {
      const t = this.strip.createDiv({ cls: "sb-refs-thumb", attr: { draggable: "true", "aria-label": r.name } });
      t.toggleClass("is-active", r === this.current);
      const im = t.createEl("img", { attr: { src: r.path.startsWith("refs/") ? this.ed.doc.refs.url(r) : thumbnailUrl(r.path, 160), loading: "lazy", draggable: "false" } });
      im.onerror = () => t.addClass("is-missing");
      // Looked up when used: undo replaces the board's objects.
      const now = () => this.list.find((x) => x.id === r.id);
      t.addEventListener("click", () => {
        const x = now();
        if (x) this.show(x);
      });
      t.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        const x = now();
        if (x) this.refMenu(x).showAtMouseEvent(e);
      });
      t.addEventListener("dragstart", (e) => e.dataTransfer?.setData("text/x-ateliet-ref", r.id));
    }
  }

  async show(r: Reference, remember = true) {
    this.current = r;
    if (remember && this.ed.frame && this.ed.frame.reference !== r.id) {
      this.ed.frame.reference = r.id;
      this.ed.changed();
    }
    this.title.setText(displayName(r, this.list.indexOf(r)));
    this.title.setAttr("aria-label", r.name + (r.from ? ` · ${r.from}` : ""));
    this.strip.querySelectorAll(".sb-refs-thumb").forEach((el, i) => {
      const on = this.list[i]?.id === r.id;
      el.toggleClass("is-active", on);
      // Scroll the strip only (scrollIntoView would also move clipped ancestors).
      if (on) {
        const t = el as HTMLElement, s = this.strip;
        if (t.offsetLeft < s.scrollLeft) s.scrollLeft = t.offsetLeft - 8;
        else if (t.offsetLeft + t.offsetWidth > s.scrollLeft + s.clientWidth) s.scrollLeft = t.offsetLeft + t.offsetWidth - s.clientWidth + 8;
      }
    });
    this.counter.setText(`${this.list.indexOf(r) + 1} / ${this.list.length}`);
    const img = await this.ed.doc.refs.image(r);
    if (this.current !== r) return;
    if (!img) {
      this.img.removeAttribute("src");
      this.title.setText(`${r.name} · not found (${r.path})`);
      return;
    }
    this.img.src = img.src;
    const w = img.naturalWidth, h = img.naturalHeight;
    this.zoom!.setSize(w, h);
    this.drawGrid();
  }

  step(delta: number) {
    const list = this.list;
    if (!list.length) return;
    const i = this.current ? list.findIndex((x) => x.id === this.current!.id) : -1;
    const next = list[(i + delta + list.length) % list.length];
    this.show(next);
  }

  toggleMirror() {
    this.mirrored = !this.mirrored;
    this.zoom?.setFlip(this.mirrored);
    this.mirrorBtn.toggleClass("is-active", this.mirrored);
  }

  /** The canvas grid over the reference too (the grid method of drawing from a picture). */
  drawGrid() {
    const n = this.ed.board.guides.grid;
    const w = this.zoom?.width || 0, h = this.zoom?.height || 0;
    this.grid.toggle(n > 0 && w > 0);
    if (!n || !w) return;
    this.grid.width = w;
    this.grid.height = h;
    this.grid.style.width = `${w}px`;
    this.grid.style.height = `${h}px`;
    const ctx = this.grid.getContext("2d")!;
    ctx.clearRect(0, 0, w, h);
    const step = Math.max(w, h) / n;
    ctx.strokeStyle = "rgba(40,120,220,.7)";
    ctx.lineWidth = Math.max(1, Math.max(w, h) / 900);
    ctx.beginPath();
    for (let x = step; x < w - 0.5; x += step) {
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
    }
    for (let y = step; y < h - 0.5; y += step) {
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
    }
    ctx.stroke();
  }

  private sample(e: PointerEvent): string | null {
    const r = this.img.getBoundingClientRect();
    const w = this.zoom?.width || 0, h = this.zoom?.height || 0;
    if (!w || !r.width) return null;
    let x = ((e.clientX - r.left) / r.width) * w;
    const y = ((e.clientY - r.top) / r.height) * h;
    if (this.mirrored) x = w - x;
    if (x < 0 || y < 0 || x >= w || y >= h) return null;
    const c = document.createElement("canvas");
    c.width = c.height = 1;
    const ctx = c.getContext("2d")!;
    try {
      ctx.drawImage(this.img, Math.floor(x), Math.floor(y), 1, 1, 0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      return "#" + [d[0], d[1], d[2]].map((v) => v.toString(16).padStart(2, "0")).join("");
    } catch {
      return null;
    }
  }

  // ── adding ──────────────────────────────────────────────────────────────

  /** Adds references (skipping pictures already there); shows the first new one. */
  add(items: { name: string; path: string; from?: string }[], label = "Add references") {
    const have = new Set(this.list.map((r) => r.path));
    const fresh: Reference[] = [];
    for (const it of items) {
      if (have.has(it.path)) continue;
      have.add(it.path);
      fresh.push({ id: uid(), name: it.name, path: it.path, from: it.from });
    }
    if (!fresh.length) {
      new Notice(items.length ? "Those pictures are already among the references." : "No pictures found there.");
      return;
    }
    this.ed.mutate(label, (b) => b.references.push(...fresh));
    this.render();
    this.show(fresh[0]);
    new Notice(`${fresh.length} ${fresh.length === 1 ? "reference" : "references"} added.`);
  }

  /** A picture's bytes kept inside the storyboard. */
  embed(name: string, bytes: Uint8Array) {
    const ext = (name.match(/\.(png|jpe?g|gif|webp)$/i)?.[1] || "png").toLowerCase();
    const path = `refs/${uid()}.${ext}`;
    this.ed.doc.refs.embedded.set(path, bytes);
    this.add([{ name, path, from: "pasted" }]);
  }

  private addMenu(e: MouseEvent) {
    const m = new Menu();
    m.addItem((i) => i.setTitle("Search pictures by description…").setIcon("sparkles").onClick(() => new SearchRefsModal(this.app, (items, q) => this.add(items.map((it) => ({ name: it.name, path: it.abs, from: `search "${q}"` })))).open()));
    m.addItem((i) => i.setTitle("A collection (moodboard)…").setIcon("gallery-vertical-end").onClick(() => this.fromCollection()));
    m.addItem((i) => i.setTitle("A vault folder…").setIcon("folder").onClick(() => new FolderModal(this.app, (f) => this.fromFolder(f)).open()));
    m.addItem((i) => i.setTitle("The pictures of a note…").setIcon("file-text").onClick(() => new FileModal(this.app, "md", (f) => this.fromNote(f)).open()));
    m.addItem((i) => i.setTitle("An Obsidian canvas…").setIcon("layout-dashboard").onClick(() => new FileModal(this.app, "canvas", (f) => this.fromCanvas(f)).open()));
    m.addItem((i) => i.setTitle("Pictogrep's storyboard references").setIcon("image").onClick(() => this.fromPictogrep()));
    m.addItem((i) => i.setTitle("Files on this computer…").setIcon("hard-drive").onClick(() => this.fromDisk()));
    m.addSeparator();
    m.addItem((i) => i.setTitle("Paste a picture (Ctrl/Cmd+V here)").setIcon("clipboard").setDisabled(true));
    m.showAtMouseEvent(e);
  }

  private async fromCollection() {
    const info = await api.sharedInfo().catch(() => null);
    const names = (info?.collections || []).map((c: any) => c.name);
    if (!info?.home || !names.length) {
      new Notice(info?.home ? "The shared library has no collections yet." : "No shared library is in use (Settings → Images).");
      return;
    }
    new ListModal(this.app, names, "Choose a collection", async (name) => {
      const page = await api.query({ collection: name, count: 2000, sort: "name" });
      this.add(page.items.map((it) => ({ name: it.name, path: it.abs, from: name })), `Add moodboard "${name}"`);
    }).open();
  }

  private fromFolder(folder: TFolder) {
    const out: { name: string; path: string; from: string }[] = [];
    const walk = (f: TFolder) => {
      for (const c of f.children) {
        if (c instanceof TFolder) walk(c);
        else if (c instanceof TFile && IMAGE_EXT.test(c.name)) out.push({ name: c.name, path: this.app.vault.adapter.getFullPath(c.path), from: folder.path || "vault" });
      }
    };
    walk(folder);
    out.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
    this.add(out, `Add folder "${folder.name || "vault"}"`);
  }

  private fromNote(note: TFile) {
    const cache = this.app.metadataCache.getFileCache(note);
    const out: { name: string; path: string; from: string }[] = [];
    for (const e of [...(cache?.embeds || []), ...(cache?.links || [])]) {
      const f = this.app.metadataCache.getFirstLinkpathDest(e.link.split("#")[0], note.path);
      if (f instanceof TFile && IMAGE_EXT.test(f.name)) out.push({ name: f.name, path: this.app.vault.adapter.getFullPath(f.path), from: note.basename });
    }
    this.add(out, `Add pictures of "${note.basename}"`);
  }

  private async fromCanvas(file: TFile) {
    try {
      const data = JSON.parse(await this.app.vault.read(file));
      const nodes: any[] = (data.nodes || []).filter((n: any) => n.type === "file" && IMAGE_EXT.test(n.file || ""));
      // Left to right, top to bottom: the order the board reads in.
      nodes.sort((a, b) => (Math.abs(a.y - b.y) > 80 ? a.y - b.y : a.x - b.x));
      const out = nodes
        .map((n) => this.app.vault.getAbstractFileByPath(n.file))
        .filter((f: any) => f instanceof TFile)
        .map((f: TFile) => ({ name: f.name, path: this.app.vault.adapter.getFullPath(f.path), from: file.basename }));
      this.add(out, `Add canvas "${file.basename}"`);
    } catch (e) {
      new Notice("Could not read the canvas: " + e);
    }
  }

  private async fromPictogrep() {
    try {
      const r: any = await invoke("pictogrep_boards");
      const refs: string[] = r.references || [];
      this.add(refs.map((p) => ({ name: p.split(/[\\/]/).pop()!, path: p, from: "Pictogrep" })), "Add Pictogrep references");
    } catch (e) {
      new Notice(String(e));
    }
  }

  private async fromDisk() {
    const paths: string[] = await invoke("pick_files", { name: "Pictures", extensions: ["png", "jpg", "jpeg", "webp", "gif"], multiple: true });
    this.add(paths.map((p) => ({ name: p.split(/[\\/]/).pop()!, path: p, from: "computer" })));
  }

  async drop(e: DragEvent) {
    e.preventDefault();
    const dt = e.dataTransfer;
    if (!dt) return;
    const raw = dt.getData("text/x-ateliet-images");
    if (raw) {
      try {
        const items: ImageItem[] = JSON.parse(raw);
        return this.add(items.map((it) => ({ name: it.name, path: it.abs, from: it.folder || undefined })));
      } catch {
        /* fall through */
      }
    }
    const vps = dt.getData("text/x-ateliet-path").split("\n").filter((vp) => IMAGE_EXT.test(vp));
    if (vps.length) return this.add(vps.map((vp) => ({ name: vp.split("/").pop()!, path: this.app.vault.adapter.getFullPath(vp) })));
    for (const f of Array.from(dt.files)) {
      if (!f.type.startsWith("image/")) continue;
      this.embed(f.name, new Uint8Array(await f.arrayBuffer()));
    }
  }

  async paste(e: ClipboardEvent) {
    for (const item of Array.from(e.clipboardData?.items || [])) {
      if (item.type.startsWith("image/")) {
        const f = item.getAsFile();
        if (!f) continue;
        e.preventDefault();
        this.embed(`pasted.${item.type.split("/")[1] || "png"}`, new Uint8Array(await f.arrayBuffer()));
        return;
      }
    }
  }

  // ── menus ───────────────────────────────────────────────────────────────

  private currentMenu(e: MouseEvent) {
    if (this.current) this.refMenu(this.current, true).showAtMouseEvent(e);
    else this.addMenu(e);
  }

  refMenu(r: Reference, current = false): Menu {
    const m = new Menu();
    m.addItem((i) => i.setTitle("Trace this picture").setIcon("scan-line").onClick(() => this.host.trace(r)));
    m.addItem((i) => i.setTitle("Place it on the frame as a picture layer").setIcon("image-plus").onClick(() => this.host.place(r)));
    if (current) {
      m.addItem((i) => i.setTitle(this.mirrored ? "Show unmirrored" : "Mirror the reference").setIcon("flip-horizontal").onClick(() => this.toggleMirror()));
      m.addItem((i) => i.setTitle("Whole picture").setIcon("maximize").onClick(() => this.zoom?.fit()));
    }
    if (!r.path.startsWith("refs/")) {
      m.addItem((i) => i.setTitle("Keep a copy inside the storyboard").setIcon("package").onClick(async () => {
        const data = new Uint8Array(await readBytes(r.path));
        const ext = (r.path.match(/\.(png|jpe?g|gif|webp)$/i)?.[1] || "png").toLowerCase();
        const path = `refs/${uid()}.${ext}`;
        this.ed.doc.refs.embedded.set(path, data);
        this.ed.mutate("Keep reference inside", (b) => {
          const x = b.references.find((y) => y.id === r.id);
          if (x) x.path = path;
        });
        this.ed.doc.refs.forget(r.id);
        new Notice("The picture now travels with the storyboard.");
      }));
    }
    m.addSeparator();
    m.addItem((i) => i.setTitle("Remove from references").setIcon("x").onClick(() => {
      this.ed.mutate("Remove reference", (b) => {
        b.references = b.references.filter((x) => x.id !== r.id);
        for (const f of b.frames) if (f.reference === r.id) delete f.reference;
      });
      if (this.current?.id === r.id) this.current = null;
      this.render();
    }));
    m.addItem((i) => i.setTitle("Remove all references").setIcon("trash-2").setWarning(true).onClick(() => {
      this.ed.mutate("Remove all references", (b) => {
        const used = new Set(Object.values(b.layers).map((l) => l.src).filter(Boolean));
        b.references = b.references.filter((x) => used.has(x.id));
        for (const f of b.frames) delete f.reference;
      });
      this.current = null;
      this.render();
    }));
    return m;
  }
}

// ── pickers ───────────────────────────────────────────────────────────────

class ListModal extends FuzzySuggestModal<string> {
  constructor(app: any, private items: string[], placeholder: string, private cb: (s: string) => void) {
    super(app);
    this.setPlaceholder(placeholder);
  }
  getItems() {
    return this.items;
  }
  getItemText(s: string) {
    return s;
  }
  onChooseItem(s: string) {
    this.cb(s);
  }
}

class FolderModal extends FuzzySuggestModal<TFolder> {
  constructor(app: any, private cb: (f: TFolder) => void) {
    super(app);
    this.setPlaceholder("Choose a folder of pictures");
  }
  getItems(): TFolder[] {
    return this.app.vault.getAllLoadedFiles().filter((f: any) => f instanceof TFolder);
  }
  getItemText(f: TFolder) {
    return f.path || "/";
  }
  onChooseItem(f: TFolder) {
    this.cb(f);
  }
}

export class FileModal extends FuzzySuggestModal<TFile> {
  constructor(app: any, private ext: string, private cb: (f: TFile) => void, placeholder?: string) {
    super(app);
    this.setPlaceholder(placeholder || (ext === "md" ? "Choose a note" : `Choose a .${ext} file`));
  }
  getItems(): TFile[] {
    return this.app.vault.getFiles().filter((f: TFile) => f.extension === this.ext);
  }
  getItemText(f: TFile) {
    return f.path;
  }
  onChooseItem(f: TFile) {
    this.cb(f);
  }
}

/** Search the picture library (by description when the model is ready) and pick references. */
class SearchRefsModal extends Modal {
  private chosen = new Map<string, ImageItem>();
  constructor(app: any, private cb: (items: ImageItem[], q: string) => void) {
    super(app);
  }
  onOpen() {
    this.setTitle("Search pictures");
    this.modalEl.addClass("sb-search-modal");
    const input = this.contentEl.createEl("input", { type: "text", cls: "sb-search-input", attr: { placeholder: "Describe what you need: \"a man running in the rain\"" } });
    const grid = this.contentEl.createDiv("sb-search-grid");
    const buttons = this.modalEl.createDiv("modal-button-container");
    const ok = buttons.createEl("button", { text: "Add", cls: "mod-cta" });
    buttons.createEl("button", { text: "Cancel" }).addEventListener("click", () => this.close());
    let q = "";
    let timer = 0;
    const run = async () => {
      q = input.value.trim();
      const page = await api.query({ search: q, count: 60 }).catch(() => null);
      grid.empty();
      for (const it of page?.items || []) {
        const t = grid.createDiv("sb-search-item");
        t.createEl("img", { attr: { src: thumbnailUrl(it.abs, 240), loading: "lazy" } });
        t.toggleClass("is-selected", this.chosen.has(it.abs));
        t.addEventListener("click", () => {
          if (this.chosen.has(it.abs)) this.chosen.delete(it.abs);
          else this.chosen.set(it.abs, it);
          t.toggleClass("is-selected", this.chosen.has(it.abs));
          ok.setText(this.chosen.size ? `Add ${this.chosen.size}` : "Add");
        });
      }
      if (!page?.items.length) grid.createDiv({ cls: "sb-hint", text: "Nothing found." });
    };
    input.addEventListener("input", () => {
      clearTimeout(timer);
      timer = window.setTimeout(run, 280);
    });
    ok.addEventListener("click", () => {
      this.cb([...this.chosen.values()], q);
      this.close();
    });
    setTimeout(() => input.focus(), 0);
    run();
  }
  onClose() {
    this.contentEl.empty();
  }
}

/** A readable name: pictures named by a hash (Pinterest, the shared library) show where they came from instead. */
export function displayName(r: Reference, index: number): string {
  const stem = r.name.replace(/\.[a-z0-9]+$/i, "");
  if (/^[0-9a-f_-]{16,}$/i.test(stem) || /^pin[-_]?\d+$/i.test(stem)) return `${r.from || "Picture"} · ${index + 1}`;
  return r.name + (r.from ? ` · ${r.from}` : "");
}
