// Pictures while writing: a search panel for the sidebar of writing mode and
// a quick "insert picture" dialog. Clicking a result puts it at the cursor;
// dragging drops it anywhere in a note.

import { ImageItem, api, insertImages, targetNote } from "./api.ts";
import { ImagesHost, imageMenu } from "./actions.ts";
import { ImageGrid } from "./grid.ts";
import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Modal } from "../obsidian/ui.ts";
import { debounce } from "../obsidian/util.ts";

export const PANEL_VIEW = "image-search";

/** Search box + results grid, shared by the panel and the dialog. */
class SearchBlock {
  inputEl: HTMLInputElement;
  grid: ImageGrid;
  hintEl: HTMLElement;
  private total = 0;
  private loading = false;
  private gen = 0;

  constructor(private host: ImagesHost, parent: HTMLElement, scrollEl: HTMLElement | null, tile: number, private onPick: (item: ImageItem) => void, placeholder: string) {
    const box = parent.createDiv("image-search-box");
    setIcon(box.createDiv("image-search-icon"), "search");
    this.inputEl = box.createEl("input", { type: "search", attr: { placeholder, spellcheck: "false" } });
    this.hintEl = parent.createDiv("image-search-hint");
    const scroller = scrollEl || parent.createDiv("image-search-results");
    this.grid = new ImageGrid(scroller, {
      tileWidth: () => tile,
      gap: 6,
      onOpen: (item) => this.onPick(item),
      onMenu: (e, items) => imageMenu(host, items).showAtMouseEvent(e),
    });
    const run = debounce(() => this.run(), 400, true);
    this.inputEl.addEventListener("input", () => run());
    scroller.addEventListener("scroll", () => {
      if (scroller.scrollTop + scroller.clientHeight > scroller.scrollHeight - 600) this.more();
    });
  }

  async run() {
    const gen = ++this.gen;
    const search = this.inputEl.value.trim();
    const page = await api.query({ search, sort: "recent", count: 60 }).catch(() => null);
    if (!page || gen !== this.gen) return;
    this.total = page.total;
    this.grid.setItems(page.items);
    const st = this.host.status;
    this.hintEl.empty();
    if (search && page.mode === "name" && st && !st.model.ready) {
      this.hintEl.setText("Searching file names only. Search by description can be turned on in Settings → Images.");
    } else if (!search) {
      this.hintEl.setText(page.total ? "Recent pictures. Click to insert, or drag into the note." : "No pictures in this vault yet.");
    } else if (!page.total) {
      this.hintEl.setText("No pictures found.");
    }
  }

  private async more() {
    if (this.loading || this.grid.items.length >= this.total) return;
    this.loading = true;
    const gen = this.gen;
    try {
      const page = await api.query({ search: this.inputEl.value.trim(), sort: "recent", offset: this.grid.items.length, count: 60 });
      if (gen === this.gen) this.grid.append(page.items);
    } finally {
      this.loading = false;
    }
  }
}

export class ImageSearchView extends ItemView {
  private block!: SearchBlock;

  constructor(leaf: any, private host: ImagesHost) {
    super(leaf);
  }
  getViewType() {
    return PANEL_VIEW;
  }
  getIcon() {
    return "image";
  }
  getDisplayText() {
    return "Pictures";
  }

  async onOpen() {
    this.headerEl.hide();
    this.contentEl.empty();
    this.contentEl.addClass("image-search-panel");
    this.block = new SearchBlock(this.host, this.contentEl, null, 104, (item) => insertImages(this.app, [item], targetNote(this.app)), "Find a picture…");
    const footer = this.contentEl.createDiv("image-search-footer");
    const open = footer.createEl("button", { text: "Open image mode" });
    open.addEventListener("click", () => this.host.setMode("images"));
    this.registerEvent(this.app.workspace.on("images:changed" as any, debounce(() => this.block.run(), 1000, true)));
    this.block.run();
  }

  async onClose() {
    this.block?.grid.destroy();
  }
}

export class InsertImageModal extends Modal {
  constructor(app: any, private host: ImagesHost) {
    super(app);
  }

  onOpen() {
    this.modalEl.addClass("image-insert-modal");
    this.setTitle("Insert picture");
    const view = targetNote(this.app);
    const block = new SearchBlock(
      this.host,
      this.contentEl,
      null,
      150,
      async (item) => {
        this.close();
        await insertImages(this.app, [item], view);
      },
      "Describe the picture…",
    );
    block.inputEl.addEventListener("keydown", async (e) => {
      if (e.key === "Enter" && block.grid.items[0]) {
        e.preventDefault();
        this.close();
        await insertImages(this.app, [block.grid.items[0]], view);
      }
    });
    block.run();
    setTimeout(() => block.inputEl.focus(), 0);
  }

  onClose() {
    this.contentEl.empty();
  }
}
