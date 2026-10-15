// A masonry grid of picture previews: shortest-column placement from the
// known aspect ratios, so tiles never jump while their previews load.

import { ImageItem, setDragData, thumbnailUrl } from "./api.ts";
import { Keymap } from "../obsidian/ui.ts";

export interface GridOptions {
  tileWidth: () => number;
  showNames?: () => boolean;
  gap?: number;
  onOpen: (item: ImageItem, index: number, e: MouseEvent) => void;
  onMenu?: (e: MouseEvent, items: ImageItem[], index: number) => void;
}

export class ImageGrid {
  el: HTMLElement;
  items: ImageItem[] = [];
  private tiles: HTMLElement[] = [];
  private heights: number[] = [];
  private columns = 1;
  private colWidth = 200;
  selected = new Set<number>();
  private anchor = -1;
  private resize: ResizeObserver;
  private lastWidth = 0;

  constructor(parent: HTMLElement, private opts: GridOptions) {
    this.el = parent.createDiv("image-grid");
    // Relayout in the next frame: changing the grid's height inside the
    // observer callback would trigger "ResizeObserver loop" warnings.
    let frame = 0;
    this.resize = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const w = this.el.clientWidth;
        if (w && Math.abs(w - this.lastWidth) > 1) this.relayout();
      });
    });
    this.resize.observe(this.el);
  }

  destroy() {
    this.resize.disconnect();
    this.el.remove();
  }

  get gap() {
    return this.opts.gap ?? 8;
  }

  clear() {
    this.items = [];
    this.tiles = [];
    this.selected.clear();
    this.anchor = -1;
    this.el.empty();
    this.el.style.height = "0px";
  }

  setItems(items: ImageItem[]) {
    this.clear();
    this.append(items);
  }

  append(items: ImageItem[]) {
    if (!this.tiles.length) this.measure();
    for (const item of items) {
      const index = this.items.length;
      this.items.push(item);
      const tile = this.makeTile(item, index);
      this.tiles.push(tile);
      this.place(tile, item);
    }
    this.el.style.height = `${Math.max(0, ...this.heights)}px`;
  }

  /** Removes a picture (deleted, moved out of the current folder). */
  remove(key: string) {
    const keep = this.items.filter((i) => i.key !== key);
    if (keep.length !== this.items.length) this.setItems(keep);
  }

  selectedItems(): ImageItem[] {
    return [...this.selected].sort((a, b) => a - b).map((i) => this.items[i]).filter(Boolean);
  }

  private measure() {
    const width = this.el.clientWidth || this.lastWidth || 800;
    this.lastWidth = width;
    const target = this.opts.tileWidth();
    this.columns = Math.max(1, Math.floor((width + this.gap) / (target + this.gap)));
    this.colWidth = (width - this.gap * (this.columns - 1)) / this.columns;
    this.heights = new Array(this.columns).fill(0);
  }

  relayout() {
    this.measure();
    this.tiles.forEach((tile, i) => this.place(tile, this.items[i]));
    this.el.style.height = `${Math.max(0, ...this.heights)}px`;
  }

  private place(tile: HTMLElement, item: ImageItem) {
    let col = 0;
    for (let c = 1; c < this.columns; c++) if (this.heights[c] < this.heights[col] - 0.5) col = c;
    const ratio = item.width && item.height ? item.height / item.width : 1;
    const label = this.opts.showNames?.() ? 22 : 0;
    const h = Math.max(40, Math.min(this.colWidth * 3, this.colWidth * ratio));
    tile.style.width = `${this.colWidth}px`;
    tile.style.height = `${h + label}px`;
    tile.style.transform = `translate(${col * (this.colWidth + this.gap)}px, ${this.heights[col]}px)`;
    this.heights[col] += h + label + this.gap;
    const img = tile.querySelector("img") as HTMLImageElement | null;
    if (img) {
      const want = this.colWidth * (window.devicePixelRatio || 1) > 480 ? 960 : this.colWidth * (window.devicePixelRatio || 1) > 240 ? 480 : 240;
      const src = thumbnailUrl(item.abs, want);
      if (img.dataset.src !== src) {
        img.dataset.src = src;
        img.src = src;
      }
    }
  }

  private makeTile(item: ImageItem, index: number): HTMLElement {
    const tile = this.el.createDiv({ cls: "image-tile", attr: { draggable: "true", "aria-label": item.name } });
    const img = tile.createEl("img", { attr: { loading: "lazy", decoding: "async", alt: item.name, draggable: "false" } });
    img.addEventListener("load", () => tile.addClass("is-loaded"));
    img.addEventListener("error", () => tile.addClass("is-broken"));
    if (this.opts.showNames?.()) tile.createDiv({ cls: "image-tile-name", text: item.name });
    if (item.external) tile.addClass("is-external");
    tile.addEventListener("click", (e) => {
      if (Keymap.isModEvent(e) || e.shiftKey) {
        this.toggleSelect(index, e.shiftKey);
        return;
      }
      if (this.selected.size) this.clearSelection();
      this.opts.onOpen(item, index, e);
    });
    tile.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      if (!this.selected.has(index)) {
        this.clearSelection();
        this.toggleSelect(index, false);
      }
      this.opts.onMenu?.(e, this.selectedItems(), index);
    });
    tile.addEventListener("dragstart", (e) => {
      const items = this.selected.has(index) ? this.selectedItems() : [item];
      setDragData(e, items);
      if (e.dataTransfer && img.complete) e.dataTransfer.setDragImage(img, 20, 20);
    });
    return tile;
  }

  toggleSelect(index: number, range: boolean) {
    if (range && this.anchor >= 0) {
      const [a, b] = [Math.min(this.anchor, index), Math.max(this.anchor, index)];
      for (let i = a; i <= b; i++) this.selected.add(i);
    } else if (this.selected.has(index)) {
      this.selected.delete(index);
    } else {
      this.selected.add(index);
    }
    this.anchor = index;
    this.tiles.forEach((t, i) => t.toggleClass("is-selected", this.selected.has(i)));
    this.el.toggleClass("has-selection", this.selected.size > 0);
  }

  clearSelection() {
    this.selected.clear();
    this.anchor = -1;
    this.tiles.forEach((t) => t.removeClass("is-selected"));
    this.el.removeClass("has-selection");
  }
}
