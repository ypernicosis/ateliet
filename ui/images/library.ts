// The image library: the main view of image mode. Every picture of the vault
// in a masonry wall, search by description, folders, similar pictures, and
// dropping pictures in from the desktop or a browser.

import { ImageItem, ImagePage, ImageQuery, TILE_WIDTH, api, folderName } from "./api.ts";
import { ImagesHost, imageMenu } from "./actions.ts";
import { ImageGrid } from "./grid.ts";
import { ImageViewer } from "./viewer.ts";
import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Menu, Notice } from "../obsidian/ui.ts";
import { debounce, requestUrl } from "../obsidian/util.ts";

export const LIBRARY_VIEW = "image-library";
const PAGE = 120;

export interface LibraryState {
  search: string;
  folder: string | null;
  recursive: boolean;
  sort: "random" | "recent" | "oldest" | "name";
  similarTo: string | null;
  similarName: string;
  /** A collection of the shared library ("__shared__": all of it, "__unsorted__": in none). */
  collection: string | null;
}

function collectionLabel(c: string): string {
  return c === "__shared__" ? "Shared library" : c === "__unsorted__" ? "Not in a collection" : c;
}

const SORTS: [LibraryState["sort"], string][] = [
  ["random", "Shuffled"],
  ["recent", "Newest first"],
  ["oldest", "Oldest first"],
  ["name", "By name"],
];

export class ImageLibraryView extends ItemView {
  state: LibraryState = { search: "", folder: null, recursive: true, sort: "random", similarTo: null, similarName: "", collection: null };
  private seed = 0;
  private total = 0;
  private mode: ImagePage["mode"] = "browse";
  private loading = false;
  private generation = 0;
  private toolbarEl!: HTMLElement;
  private inputEl!: HTMLInputElement;
  private chipsEl!: HTMLElement;
  private statusEl!: HTMLElement;
  private noticeEl!: HTMLElement;
  private scrollEl!: HTMLElement;
  private grid!: ImageGrid;
  private viewer!: ImageViewer;
  private sortEl!: HTMLElement;

  constructor(leaf: any, private host: ImagesHost) {
    super(leaf);
  }

  getViewType() {
    return LIBRARY_VIEW;
  }
  getIcon() {
    return "images";
  }
  getDisplayText() {
    if (this.state.similarTo) return `Similar to ${this.state.similarName || "picture"}`;
    if (this.state.search) return this.state.search;
    if (this.state.collection) return collectionLabel(this.state.collection);
    if (this.state.folder !== null) return folderName(this.app, this.state.folder);
    return "Pictures";
  }

  getState(): any {
    return { ...this.state };
  }

  async setState(state: any, result: any) {
    const next = { ...this.state, ...(state || {}) };
    const changed = JSON.stringify(next) !== JSON.stringify(this.state);
    this.state = next;
    if (this.inputEl) {
      this.inputEl.value = this.state.search;
      if (changed || !this.grid.items.length) this.reload();
    }
    if (result) result.history = changed;
  }

  async onOpen() {
    this.contentEl.empty();
    this.contentEl.addClass("image-library");
    const tb = (this.toolbarEl = this.contentEl.createDiv("image-library-toolbar"));
    const search = tb.createDiv("image-search-box");
    setIcon(search.createDiv("image-search-icon"), "search");
    this.inputEl = search.createEl("input", { type: "search", attr: { placeholder: "Describe the picture you're looking for…", spellcheck: "false" } });
    this.inputEl.value = this.state.search;
    const run = debounce(() => this.applySearch(), 450, true);
    this.inputEl.addEventListener("input", () => run());
    this.inputEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") this.applySearch();
      if (e.key === "Escape" && this.inputEl.value) {
        this.inputEl.value = "";
        this.applySearch();
        e.stopPropagation();
      }
    });
    this.chipsEl = tb.createDiv("image-chips");
    this.sortEl = tb.createDiv({ cls: "clickable-icon image-sort", attr: { "aria-label": "Order" } });
    setIcon(this.sortEl, "arrow-up-down");
    this.sortEl.addEventListener("click", (e) => this.sortMenu(e));
    const pin = tb.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Import from Pinterest" } });
    setIcon(pin, "download");
    pin.addEventListener("click", () => this.host.openPinterest(this.state.collection && !this.state.collection.startsWith("__") ? { collection: this.state.collection } : {}));
    const tiles = tb.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Preview size" } });
    setIcon(tiles, "layout-grid");
    tiles.addEventListener("click", (e) => this.tileMenu(e));
    this.statusEl = this.contentEl.createDiv("image-library-status");
    this.noticeEl = this.contentEl.createDiv("image-library-notice");
    this.scrollEl = this.contentEl.createDiv("image-library-scroll");
    this.grid = new ImageGrid(this.scrollEl, {
      tileWidth: () => TILE_WIDTH[this.host.settings.tile as keyof typeof TILE_WIDTH] || 220,
      showNames: () => this.host.settings.showNames,
      onOpen: (_item, index) => this.viewer.open(this.grid.items, index),
      onMenu: (e, items) =>
        imageMenu(this.host, items, (menu) => {
          const c = this.state.collection;
          if (c && !c.startsWith("__")) {
            menu.addItem((i) =>
              i.setTitle(`Remove from ${c}`).setIcon("minus-circle").onClick(async () => {
                await api.sharedCollection("remove", c, { paths: items.map((it) => it.abs) });
                this.reload();
              }),
            );
          }
        }).showAtMouseEvent(e),
    });
    this.scrollEl.addEventListener("scroll", () => {
      if (this.scrollEl.scrollTop + this.scrollEl.clientHeight > this.scrollEl.scrollHeight - 1200) this.loadMore();
    });
    this.viewer = new ImageViewer(this.host, this.contentEl, {
      more: () => this.loadMore(),
      onFolder: (folder) => this.setState({ folder, recursive: false, search: "", similarTo: null }, {}),
    });
    this.setupDrop();
    this.registerEvent(this.app.workspace.on("images:changed" as any, () => this.softRefresh()));
    this.registerEvent(this.app.workspace.on("images:status" as any, () => this.renderStatus()));
    this.registerEvent(this.app.workspace.on("images:settings" as any, () => this.grid.relayout()));
    this.registerDomEvent(this.contentEl, "paste", (e: ClipboardEvent) => this.onPaste(e));
    // The first query runs from setState, which always follows onOpen.
  }

  async onClose() {
    this.viewer?.close();
    this.grid?.destroy();
  }

  private applySearch() {
    const search = this.inputEl.value.trim();
    if (search === this.state.search && !this.state.similarTo) return;
    this.state = { ...this.state, search, similarTo: search ? null : this.state.similarTo };
    this.updateHeader();
    this.app.workspace.requestSaveLayout();
    this.reload();
  }

  private query(offset: number, count = PAGE): ImageQuery {
    const s = this.state;
    return { search: s.search, folder: s.folder, recursive: s.recursive, sort: s.sort, seed: this.seed, offset, count, similarTo: s.similarTo, collection: s.collection };
  }

  /** New query from the top. */
  async reload() {
    const gen = ++this.generation;
    if (this.state.sort === "random") this.seed = 0;
    this.renderChips();
    this.app.workspace.trigger("images:library-state");
    this.viewer?.close();
    this.loading = true;
    try {
      const page = await api.query(this.query(0));
      if (gen !== this.generation) return;
      this.seed = page.seed;
      this.total = page.total;
      this.mode = page.mode;
      this.grid.setItems(page.items);
      this.scrollEl.scrollTop = 0;
    } catch (e) {
      if (gen === this.generation) this.grid.clear();
      console.error(e);
    } finally {
      if (gen === this.generation) this.loading = false;
    }
    this.renderStatus();
    this.updateHeader();
  }

  /** The library changed on disk: refresh what is shown, keeping the order. */
  private softRefresh = debounce(async () => {
    if (this.viewer.isOpen || this.loading) return;
    const gen = ++this.generation;
    const count = Math.max(PAGE, this.grid.items.length);
    const top = this.scrollEl.scrollTop;
    const page = await api.query(this.query(0, count)).catch(() => null);
    if (!page || gen !== this.generation) return;
    const same = page.items.length === this.grid.items.length && page.items.every((it, i) => it.key === this.grid.items[i].key && it.mtime === this.grid.items[i].mtime);
    this.total = page.total;
    this.mode = page.mode;
    if (!same) {
      this.grid.setItems(page.items);
      this.scrollEl.scrollTop = top;
    }
    this.renderStatus();
  }, 800, true);

  private async loadMore(): Promise<ImageItem[]> {
    if (this.loading || this.grid.items.length >= this.total) return [];
    const gen = this.generation;
    this.loading = true;
    try {
      const page = await api.query(this.query(this.grid.items.length));
      if (gen !== this.generation) return [];
      this.grid.append(page.items);
      return page.items;
    } finally {
      if (gen === this.generation) this.loading = false;
    }
  }

  private renderChips() {
    this.chipsEl.empty();
    const chip = (icon: string, text: string, clear: () => void) => {
      const c = this.chipsEl.createDiv("image-chip");
      setIcon(c.createSpan("image-chip-icon"), icon);
      c.createSpan({ text });
      const x = c.createSpan({ cls: "image-chip-clear", attr: { "aria-label": "Remove" } });
      setIcon(x, "x");
      x.addEventListener("click", clear);
    };
    if (this.state.collection) chip("library", collectionLabel(this.state.collection), () => this.setState({ collection: null }, {}));
    if (this.state.folder !== null) {
      chip("folder", folderName(this.app, this.state.folder) + (this.state.recursive ? "" : " (only this folder)"), () => this.setState({ folder: null }, {}));
    }
    if (this.state.similarTo) chip("sparkles", `Similar to ${this.state.similarName || "picture"}`, () => this.setState({ similarTo: null }, {}));
    this.sortEl.toggle(!this.state.search && !this.state.similarTo);
  }

  private renderStatus() {
    const st = this.host.status;
    const n = this.total;
    const parts: string[] = [];
    if (this.mode === "semantic") parts.push(`${n.toLocaleString()} pictures, most similar to the description first`);
    else if (this.mode === "name") parts.push(`${n.toLocaleString()} ${n === 1 ? "picture" : "pictures"} with matching names`);
    else if (this.mode === "similar") parts.push(`${n.toLocaleString()} pictures, most similar first`);
    else parts.push(`${n.toLocaleString()} ${n === 1 ? "picture" : "pictures"}`);
    if (st?.working && st.total) parts.push(`analysing ${st.indexed.toLocaleString()} of ${st.total.toLocaleString()}`);
    this.statusEl.setText(parts.join(" · "));

    this.noticeEl.empty();
    this.noticeEl.hide();
    if (!st || !st.scanned) return;
    if (!st.model.ready && (this.state.search || st.total > 0)) {
      this.noticeEl.show();
      if (st.model.downloading) {
        const pct = st.model.total ? Math.round((st.model.received / st.model.total) * 100) : 0;
        this.noticeEl.createSpan({ text: `Downloading the image model… ${pct}%` });
      } else {
        this.noticeEl.createSpan({ text: this.state.search ? "Only file names are being searched. " : "" });
        this.noticeEl.createSpan({ text: `Search by description needs a one-time download of the image model (about ${Math.round(st.model.size / 1e6)} MB). Pictures are analysed on this computer and never leave it.` });
        const b = this.noticeEl.createEl("button", { text: "Download", cls: "mod-cta" });
        b.addEventListener("click", () => api.downloadModel());
        if (st.model.error) this.noticeEl.createDiv({ cls: "image-library-error", text: st.model.error });
      }
    } else if (this.state.search && this.mode === "semantic" && st.indexed < st.total) {
      this.noticeEl.show();
      this.noticeEl.setText(`${(st.total - st.indexed).toLocaleString()} pictures are still being analysed and appear at the end until they are ready.`);
    }
    if (!this.total && !this.loading) {
      this.noticeEl.show();
      this.noticeEl.createDiv({ cls: "image-library-empty", text: this.state.search || this.state.similarTo ? "No pictures found." : this.state.folder !== null ? "No pictures in this folder." : "No pictures yet. Drop images here, or add a folder of pictures in Settings → Images." });
    }
  }

  private sortMenu(e: MouseEvent) {
    const menu = new Menu();
    for (const [id, label] of SORTS) {
      menu.addItem((i) => i.setTitle(label).setChecked(this.state.sort === id).onClick(() => this.setState({ sort: id }, {})));
    }
    if (this.state.folder !== null || (this.state.collection && !this.state.collection.startsWith("__"))) {
      menu.addSeparator();
      menu.addItem((i) => i.setTitle("Include subfolders").setChecked(this.state.recursive).onClick(() => this.setState({ recursive: !this.state.recursive }, {})));
    }
    menu.showAtMouseEvent(e);
  }

  private tileMenu(e: MouseEvent) {
    const menu = new Menu();
    for (const [id, label] of [["small", "Small"], ["medium", "Medium"], ["large", "Large"]] as const) {
      menu.addItem((i) => i.setTitle(label).setChecked(this.host.settings.tile === id).onClick(() => this.host.updateSettings({ tile: id })));
    }
    menu.addSeparator();
    menu.addItem((i) => i.setTitle("Show file names").setChecked(this.host.settings.showNames).onClick(() => this.host.updateSettings({ showNames: !this.host.settings.showNames })));
    menu.showAtMouseEvent(e);
  }

  // ── bringing pictures in ─────────────────────────────────────────────────

  /** Where dropped pictures go: the folder on screen, else the import folder. */
  private destination(): string | undefined {
    const f = this.state.folder;
    if (f !== null && !/^([a-zA-Z]:)?[\\/]/.test(f)) return f || "/";
    return this.host.settings.importFolder || undefined;
  }

  private setupDrop() {
    const el = this.contentEl;
    const isImport = (e: DragEvent) => {
      const types = Array.from(e.dataTransfer?.types || []);
      return types.includes("Files") || (types.includes("text/uri-list") && !types.includes("text/x-ateliet-images"));
    };
    this.registerDomEvent(el, "dragover", (e: DragEvent) => {
      if (!isImport(e)) return;
      e.preventDefault();
      el.addClass("is-drop-target");
    });
    this.registerDomEvent(el, "dragleave", (e: DragEvent) => {
      if (!el.contains(e.relatedTarget as Node)) el.removeClass("is-drop-target");
    });
    this.registerDomEvent(el, "drop", async (e: DragEvent) => {
      el.removeClass("is-drop-target");
      if (!isImport(e)) return;
      e.preventDefault();
      const files = Array.from(e.dataTransfer?.files || []).filter((f) => /^image\//.test(f.type) || /\.(jpe?g|png|gif|webp)$/i.test(f.name));
      if (files.length) return this.importFiles(files);
      const urls = (e.dataTransfer?.getData("text/uri-list") || "").split(/\r?\n/).filter((u) => /^https?:\/\//.test(u));
      if (urls.length) this.importUrls(urls);
    });
  }

  private onPaste(e: ClipboardEvent) {
    if ((e.target as HTMLElement)?.closest?.("input, textarea")) return;
    const files = Array.from(e.clipboardData?.files || []).filter((f) => /^image\//.test(f.type));
    if (files.length) {
      e.preventDefault();
      this.importFiles(files);
      return;
    }
    const text = e.clipboardData?.getData("text/plain")?.trim() || "";
    if (/^https?:\/\/\S+$/.test(text)) {
      e.preventDefault();
      this.importUrls([text]);
    }
  }

  /** The shared collection dropped pictures join, or "" for the library itself; null when they go to the vault. */
  private sharedDestination(): string | null {
    const st = this.host.status;
    if (!st?.shared) return null;
    const c = this.state.collection;
    if (c) return c.startsWith("__") ? "" : c;
    if (this.state.folder !== null) return null;
    return this.host.settings.importTarget === "shared" ? "" : null;
  }

  private async save(name: string, data: ArrayBuffer) {
    const shared = this.sharedDestination();
    if (shared !== null) {
      await api.sharedImport(name, data, shared);
      return;
    }
    const dest = this.destination();
    const app = this.app;
    let path: string;
    if (dest !== undefined) {
      const dot = name.lastIndexOf(".");
      const base = (dest === "/" ? "" : dest + "/") + (dot > 0 ? name.slice(0, dot) : name);
      if (dest !== "/" && !app.vault.getAbstractFileByPath(dest)) await app.vault.createFolder(dest).catch(() => {});
      path = app.vault.getAvailablePath(base, dot > 0 ? name.slice(dot + 1) : "png");
    } else {
      path = await app.fileManager.getAvailablePathForAttachment(name, "");
    }
    await app.vault.createBinary(path, data);
  }

  private async importFiles(files: File[]) {
    for (const f of files) await this.save(f.name || "Pasted image.png", await f.arrayBuffer());
    new Notice(`${files.length} ${files.length === 1 ? "picture" : "pictures"} added.`);
  }

  private async importUrls(urls: string[]) {
    let ok = 0;
    for (const url of urls) {
      try {
        const r = await requestUrl({ url, method: "GET" });
        const type = (r.headers?.["content-type"] || r.headers?.["Content-Type"] || "").split(";")[0];
        if (r.status >= 400 || !/^image\/(jpeg|png|gif|webp)/.test(type)) throw new Error(type ? `the link is ${type}, not a picture` : `HTTP ${r.status}`);
        const ext = type.split("/")[1].replace("jpeg", "jpg");
        let name = decodeURIComponent(new URL(url).pathname.split("/").pop() || "").replace(/[\\/:*?"<>|#^[\]]/g, " ").trim() || "Image";
        if (!/\.(jpe?g|png|gif|webp)$/i.test(name)) name = `${name}.${ext}`;
        await this.save(name, r.arrayBuffer);
        ok++;
      } catch (err) {
        new Notice(`Could not download ${url}: ${(err as any)?.message || err}`);
      }
    }
    if (ok) new Notice(`${ok} ${ok === 1 ? "picture" : "pictures"} added from the web.`);
  }
}
