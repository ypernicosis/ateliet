// The picture viewer: one picture large, with what connects it to the rest
// of the vault: the notes that use it and the pictures that look like it.

import { ImageItem, api, folderName, formatBytes, fullUrl, insertImages, notesUsing, thumbnailUrl } from "./api.ts";
import { ImagesHost, imageMenu, newNoteWithImages } from "./actions.ts";
import { ImageGrid } from "./grid.ts";
import { setIcon } from "../obsidian/icons.ts";
import { moment } from "../obsidian/util.ts";
import { invoke } from "../tauri.ts";

export class ImageViewer {
  el: HTMLElement;
  private list: ImageItem[] = [];
  private index = 0;
  private stageEl!: HTMLElement;
  private imgEl!: HTMLImageElement;
  private infoEl!: HTMLElement;
  private similar: ImageGrid | null = null;
  private onKey = (e: KeyboardEvent) => this.key(e);

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
    document.removeEventListener("keydown", this.onKey, true);
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
    if (e.key === "Escape") this.close();
    else if (e.key === "ArrowRight") this.step(1);
    else if (e.key === "ArrowLeft") this.step(-1);
    else return;
    e.preventDefault();
    e.stopPropagation();
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

    const body = this.el.createDiv("image-viewer-body");
    this.stageEl = body.createDiv("image-viewer-stage");
    // The cached preview shows at once; the original replaces it when loaded.
    this.imgEl = this.stageEl.createEl("img", { attr: { src: thumbnailUrl(item.abs, 960), alt: item.name, draggable: "true" } });
    const full = new Image();
    full.onload = () => {
      if (this.list[this.index] === item) this.imgEl.src = full.src;
    };
    full.src = fullUrl(item);
    this.imgEl.addEventListener("click", () => this.stageEl.toggleClass("is-zoomed", !this.stageEl.hasClass("is-zoomed")));
    this.imgEl.addEventListener("dragstart", (e) => {
      e.dataTransfer?.setData("text/x-ateliet-images", JSON.stringify([item]));
      if (!item.external) e.dataTransfer?.setData("text/x-ateliet-path", item.key);
    });
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
