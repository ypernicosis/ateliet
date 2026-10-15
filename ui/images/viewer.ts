// The picture viewer: one picture large, with what connects it to the rest
// of the vault: the notes that use it and the pictures that look like it.

import { ImageItem, api, folderName, formatBytes, fullUrl, insertImages, notesUsing, thumbnailUrl } from "./api.ts";
import { ImagesHost, imageMenu, newNoteWithImages } from "./actions.ts";
import { ImageGrid } from "./grid.ts";
import { setIcon } from "../obsidian/icons.ts";
import { moment } from "../obsidian/util.ts";
import { currentWindow, invoke } from "../tauri.ts";
import { PanZoom } from "./zoom.ts";

export class ImageViewer {
  el: HTMLElement;
  private list: ImageItem[] = [];
  private index = 0;
  private stageEl!: HTMLElement;
  private imgEl!: HTMLImageElement;
  private infoEl!: HTMLElement;
  private similar: ImageGrid | null = null;
  private zoom: PanZoom | null = null;
  private zoomLabel: HTMLElement | null = null;
  private home: { parent: HTMLElement; next: Node | null } | null = null;
  /** Whether the window was already fullscreen before the viewer asked for it. */
  private windowWasFullscreen = false;
  private idleTimer = 0;
  private onKey = (e: KeyboardEvent) => this.key(e);
  private onIdleMove = () => this.wake();

  constructor(private host: ImagesHost, parent: HTMLElement, private opts: { onClose?: () => void; more?: () => Promise<ImageItem[]>; onFolder?: (folder: string) => void } = {}) {
    this.el = parent.createDiv({ cls: "image-viewer", attr: { tabindex: "-1" } });
    this.el.hide();
  }

  get isOpen() {
    return this.el.isShown();
  }

  open(list: ImageItem[], index: number) {
    this.list = list;
    this.index = index;
    this.el.show();
    this.el.focus();
    document.addEventListener("keydown", this.onKey, true);
    this.render();
  }

  close() {
    if (!this.isOpen) return;
    if (this.fullscreen) this.setFullscreen(false);
    document.removeEventListener("keydown", this.onKey, true);
    this.zoom?.destroy();
    this.zoom = null;
    this.similar?.destroy();
    this.similar = null;
    this.el.empty();
    this.el.hide();
    this.opts.onClose?.();
  }

  private key(e: KeyboardEvent) {
    if (!this.isOpen || document.querySelector(".modal-container, .menu")) return;
    const target = e.target as HTMLElement;
    if (target?.closest?.("input, textarea, .cm-editor")) return;
    if (e.key === "Escape") {
      if (this.fullscreen) this.setFullscreen(false);
      else this.close();
    } else if (e.key === "ArrowRight") this.step(1);
    else if (e.key === "ArrowLeft") this.step(-1);
    else if ((e.key === "f" || e.key === "F" || e.key === "F11") && !e.ctrlKey && !e.metaKey) this.setFullscreen(!this.fullscreen);
    else if (!this.zoom?.key(e)) return;
    e.preventDefault();
    e.stopPropagation();
  }

  get fullscreen() {
    return this.el.hasClass("is-fullscreen");
  }

  /** Fullscreen: the picture alone on black, over the whole window, and the
   * window itself fullscreen. The bar comes back when the mouse moves. */
  async setFullscreen(on: boolean) {
    if (on === this.fullscreen) return;
    const win = currentWindow();
    if (on) {
      this.home = { parent: this.el.parentElement!, next: this.el.nextSibling };
      document.body.appendChild(this.el);
      this.el.addClass("is-fullscreen");
      this.el.addEventListener("pointermove", this.onIdleMove);
      this.wake();
      this.windowWasFullscreen = await win.isFullscreen().catch(() => false);
      if (!this.windowWasFullscreen) await win.setFullscreen(true).catch(() => {});
    } else {
      this.el.removeClass("is-fullscreen", "is-idle");
      this.el.removeEventListener("pointermove", this.onIdleMove);
      clearTimeout(this.idleTimer);
      if (this.home?.parent.isConnected) this.home.parent.insertBefore(this.el, this.home.next?.parentNode === this.home.parent ? this.home.next : null);
      this.home = null;
      if (!this.windowWasFullscreen) await win.setFullscreen(false).catch(() => {});
    }
    if (this.zoom) {
      this.zoom.upscale = on;
      if (this.zoom.fitted) this.zoom.fit();
    }
    this.el.focus();
    this.updateFullscreenButton();
  }

  private wake() {
    this.el.removeClass("is-idle");
    clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => {
      if (this.fullscreen && !this.el.querySelector(".image-viewer-bar:hover")) this.el.addClass("is-idle");
    }, 1800);
  }

  private fullscreenButton: HTMLElement | null = null;
  private updateFullscreenButton() {
    if (!this.fullscreenButton) return;
    setIcon(this.fullscreenButton, this.fullscreen ? "minimize" : "maximize");
    this.fullscreenButton.setAttr("aria-label", this.fullscreen ? "Leave fullscreen (F or Esc)" : "Fullscreen (F)");
  }

  private async step(delta: number) {
    let next = this.index + delta;
    if (next >= this.list.length && this.opts.more) {
      const more = await this.opts.more();
      if (more.length) this.list = this.list.concat(more);
    }
    next = Math.max(0, Math.min(this.list.length - 1, next));
    if (next !== this.index) {
      this.index = next;
      this.render();
    }
  }

  private render() {
    const item = this.list[this.index];
    if (!item) return this.close();
    const app = this.host.app;
    this.similar?.destroy();
    this.el.empty();

    const bar = this.el.createDiv("image-viewer-bar");
    const back = bar.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Back to the pictures (Esc)" } });
    setIcon(back, "arrow-left");
    back.addEventListener("click", () => this.close());
    bar.createDiv({ cls: "image-viewer-title", text: item.name });
    bar.createDiv({ cls: "image-viewer-counter", text: this.list.length > 1 ? `${this.index + 1} / ${this.list.length}` : "" });
    const prev = bar.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Previous (←)" } });
    setIcon(prev, "chevron-left");
    prev.toggleClass("mod-disabled", this.index === 0);
    prev.addEventListener("click", () => this.step(-1));
    const next = bar.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Next (→)" } });
    setIcon(next, "chevron-right");
    next.addEventListener("click", () => this.step(1));
    bar.createDiv("image-viewer-sep");
    const zoomOut = bar.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Zoom out (−)" } });
    setIcon(zoomOut, "zoom-out");
    zoomOut.addEventListener("click", () => this.zoom?.zoomBy(0.8));
    const label = (this.zoomLabel = bar.createDiv({ cls: "image-viewer-zoom", attr: { "aria-label": "Whole picture (0) · real size (1)" } }));
    label.addEventListener("click", () => (this.zoom?.fitted ? this.zoom.actual() : this.zoom?.fit()));
    const zoomIn = bar.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Zoom in (+)" } });
    setIcon(zoomIn, "zoom-in");
    zoomIn.addEventListener("click", () => this.zoom?.zoomBy(1.25));
    // Dragging the picture moves it now, so dragging it into a note starts here.
    const grip = bar.createDiv({ cls: "clickable-icon image-viewer-grip", attr: { "aria-label": "Drag into a note", draggable: "true" } });
    setIcon(grip, "grip-vertical");
    grip.addEventListener("dragstart", (e) => {
      e.dataTransfer?.setData("text/x-ateliet-images", JSON.stringify([item]));
      if (!item.external) e.dataTransfer?.setData("text/x-ateliet-path", item.key);
      if (this.imgEl.complete) e.dataTransfer?.setDragImage(this.imgEl, 16, 16);
    });
    const fsButton = (this.fullscreenButton = bar.createDiv({ cls: "clickable-icon" }));
    fsButton.addEventListener("click", () => this.setFullscreen(!this.fullscreen));
    this.updateFullscreenButton();

    const body = this.el.createDiv("image-viewer-body");
    this.stageEl = body.createDiv("image-viewer-stage");
    const content = this.stageEl.createDiv();
    // The cached preview shows at once; the original replaces it when loaded.
    this.imgEl = content.createEl("img", { attr: { src: thumbnailUrl(item.abs, 960), alt: item.name, draggable: "false" } });
    this.zoom?.destroy();
    const zoom = (this.zoom = new PanZoom(this.stageEl, content, { onChange: (z) => label.setText(`${z.percent}%`) }));
    zoom.upscale = this.fullscreen;
    const sized = !!(item.width && item.height);
    if (sized) zoom.setSize(item.width!, item.height!);
    this.imgEl.addEventListener("load", () => {
      if (!sized && this.imgEl.src === full.src) zoom.setSize(this.imgEl.naturalWidth, this.imgEl.naturalHeight);
      else if (!sized && !zoom.width) zoom.setSize(this.imgEl.naturalWidth, this.imgEl.naturalHeight);
    });
    const full = new Image();
    full.onload = () => {
      if (this.list[this.index] === item) this.imgEl.src = full.src;
    };
    full.src = fullUrl(item);
    this.imgEl.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      imageMenu(this.host, [item]).showAtMouseEvent(e);
    });

    const info = (this.infoEl = body.createDiv("image-viewer-info"));
    info.createDiv({ cls: "image-viewer-name", text: item.name });
    const where = info.createDiv("image-viewer-where");
    const folderLink = where.createEl("a", { text: item.external ? item.folder : folderName(app, item.folder), cls: "image-viewer-folder" });
    folderLink.addEventListener("click", () => {
      this.close();
      this.opts.onFolder?.(item.folder);
    });
    const meta: string[] = [];
    if (item.width) meta.push(`${item.width} × ${item.height}`);
    meta.push(formatBytes(item.size));
    meta.push(moment(item.mtime).format("ll"));
    info.createDiv({ cls: "image-viewer-meta", text: meta.join(" · ") });

    const actions = info.createDiv("image-viewer-actions");
    const action = (icon: string, label: string, cb: () => any, cta = false) => {
      const b = actions.createEl("button", { cls: cta ? "mod-cta" : "", attr: { "aria-label": label } });
      setIcon(b.createSpan(), icon);
      b.createSpan({ text: label });
      b.addEventListener("click", cb);
      return b;
    };
    action("file-input", "Insert into note", () => insertImages(app, [item]), true);
    action("file-plus", "New note", () => newNoteWithImages(this.host, [item]));
    action("folder-open", "Show in folder", () => invoke("show_in_folder", { path: item.abs }));
    const more = actions.createEl("button", { cls: "clickable-icon", attr: { "aria-label": "More" } });
    setIcon(more, "more-horizontal");
    more.addEventListener("click", (e) => imageMenu(this.host, [item]).showAtMouseEvent(e));

    const notes = notesUsing(app, item);
    const notesEl = info.createDiv("image-viewer-section");
    notesEl.createDiv({ cls: "image-viewer-heading", text: notes.length ? `Used in ${notes.length} ${notes.length === 1 ? "note" : "notes"}` : "Not used in any note yet" });
    for (const n of notes.slice(0, 30)) {
      const row = notesEl.createDiv("image-viewer-note tree-item-self is-clickable");
      setIcon(row.createDiv("tree-item-icon"), "file-text");
      row.createDiv({ cls: "tree-item-inner", text: n.basename });
      row.addEventListener("click", (e) => this.host.openNote(n, e.metaKey || e.ctrlKey));
    }

    const simEl = info.createDiv("image-viewer-section");
    const simHead = simEl.createDiv({ cls: "image-viewer-heading", text: "Similar pictures" });
    if (!item.indexed) {
      simEl.createDiv({ cls: "image-viewer-hint", text: "This picture has not been analysed yet." });
      return;
    }
    const grid = (this.similar = new ImageGrid(simEl, {
      tileWidth: () => 96,
      gap: 6,
      onOpen: (it, i) => this.open(grid.items, i),
      onMenu: (e, items) => imageMenu(this.host, items).showAtMouseEvent(e),
    }));
    api.query({ similarTo: item.key, count: 24 }).then((page) => {
      if (this.similar !== grid) return;
      if (!page.items.length) simHead.setText("No similar pictures");
      grid.setItems(page.items);
    });
  }
}
