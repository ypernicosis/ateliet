// Folders as collections: the left pane of image mode. Only folders that hold
// pictures (directly or below) are listed, each with a cover and a count.
// Pictures dragged onto a folder move into it; every folder can have a note.

import { FolderInfo, ImageItem, SharedInfo, api, ensureInVault, thumbnailUrl, vaultFile } from "./api.ts";
import { ImagesHost } from "./actions.ts";
import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Menu, Modal, Notice, Setting } from "../obsidian/ui.ts";
import { TFile, TFolder } from "../obsidian/vault.ts";
import { debounce } from "../obsidian/util.ts";
import { fileUrl, invoke } from "../tauri.ts";

const thumbnailUrlOriginal = (it: ImageItem) => fileUrl(it.abs);

export const COLLECTIONS_VIEW = "image-collections";

/** Asks for a name; resolves to null when cancelled. */
export function askName(app: any, title: string, value = ""): Promise<string | null> {
  return new Promise((resolve) => {
    const m = new Modal(app);
    let result: string | null = null;
    m.setTitle(title);
    new Setting(m.contentEl).addText((t) => {
      t.setValue(value).onChange((v) => (value = v));
      t.inputEl.style.width = "100%";
      t.inputEl.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
          result = value.trim() || null;
          m.close();
        }
      });
      setTimeout(() => t.inputEl.select(), 0);
    });
    const buttons = m.modalEl.createDiv("modal-button-container");
    buttons.createEl("button", { text: "OK", cls: "mod-cta" }).addEventListener("click", () => {
      result = value.trim() || null;
      m.close();
    });
    buttons.createEl("button", { text: "Cancel" }).addEventListener("click", () => m.close());
    m.onClose = () => resolve(result);
    m.open();
  });
}

interface Node {
  key: string;
  name: string;
  own: number;
  total: number;
  cover: string;
  latest: number;
  children: Map<string, Node>;
  external: boolean;
}

function node(key: string, name: string, external: boolean): Node {
  return { key, name, own: 0, total: 0, cover: "", latest: -Infinity, children: new Map(), external };
}

/** Builds a tree from per-folder counts. `root` is "" for the vault. */
function buildTree(folders: FolderInfo[], rootKey: string, rootName: string, sep: string, external: boolean): Node {
  const root = node(rootKey, rootName, external);
  for (const f of folders) {
    const rel = rootKey ? f.folder.slice(rootKey.length).replace(/^[\\/]/, "") : f.folder;
    let cur = root;
    let key = rootKey;
    for (const part of rel ? rel.split(/[\\/]/) : []) {
      key = key ? key + sep + part : part;
      if (!cur.children.has(part)) cur.children.set(part, node(key, part, external));
      cur = cur.children.get(part)!;
    }
    cur.own += f.count;
    if (f.latest > cur.latest) {
      cur.latest = f.latest;
      cur.cover = f.cover;
    }
  }
  const total = (n: Node): number => {
    n.total = n.own;
    for (const c of n.children.values()) {
      n.total += total(c);
      if (c.latest > n.latest) {
        n.latest = c.latest;
        n.cover = c.cover;
      }
    }
    return n.total;
  };
  total(root);
  return root;
}

export class ImageCollectionsView extends ItemView {
  private collapsed = new Set<string>();
  private listEl!: HTMLElement;

  constructor(leaf: any, private host: ImagesHost) {
    super(leaf);
  }

  getViewType() {
    return COLLECTIONS_VIEW;
  }
  getIcon() {
    return "gallery-vertical-end";
  }
  getDisplayText() {
    return "Collections";
  }
  getState(): any {
    return { collapsed: [...this.collapsed] };
  }
  async setState(state: any) {
    this.collapsed = new Set(state?.collapsed || []);
    if (this.listEl) this.render();
  }

  async onOpen() {
    this.headerEl.hide();
    this.contentEl.empty();
    this.contentEl.addClass("image-collections");
    const bar = this.contentEl.createDiv("nav-header").createDiv("nav-buttons-container");
    const btn = (icon: string, label: string, cb: (e: MouseEvent) => any) => {
      const b = bar.createDiv({ cls: "clickable-icon nav-action-button", attr: { "aria-label": label } });
      setIcon(b, icon);
      b.addEventListener("click", cb);
    };
    btn("folder-plus", "New folder", () => this.newFolder(""));
    btn("hard-drive", "Add a folder from this computer", () => this.host.addExternalFolder());
    btn("refresh-cw", "Look for new pictures", () => api.rescan());
    this.listEl = this.contentEl.createDiv("image-collections-list");
    this.registerEvent(this.app.workspace.on("images:changed" as any, () => this.update()));
    this.registerEvent(this.app.workspace.on("active-leaf-change", () => this.markActive()));
    this.registerEvent(this.app.workspace.on("images:library-state" as any, () => this.markActive()));
    this.render();
  }

  private update = debounce(() => this.render(), 600, true);

  /** The row key of what the library shows. */
  private current(): string | undefined {
    const leaf = this.app.workspace.getLeavesOfType("image-library")[0];
    const st = leaf ? (leaf.view as any).state : null;
    if (!st) return undefined;
    if (st.collection) return "shared:" + st.collection;
    return st.folder === null ? "__all__" : st.folder;
  }

  private markActive() {
    const cur = this.current();
    this.listEl.querySelectorAll(".image-collection").forEach((el: any) => el.toggleClass("is-active", el.dataset.key === cur));
  }

  private renderGen = 0;

  async render() {
    // Fetch first, draw once: overlapping renders must not both append.
    const gen = ++this.renderGen;
    let folders: FolderInfo[] = [];
    try {
      folders = await api.folders();
    } catch {
      return;
    }
    const info = await api.sharedInfo().catch(() => null);
    if (gen !== this.renderGen) return;
    const top = this.listEl.scrollTop;
    this.listEl.empty();
    const total = folders.reduce((n, f) => n + f.count, 0);
    const all = this.row(this.listEl, { key: "__all__", name: "All pictures", own: total, total, cover: "", latest: 0, children: new Map(), external: false }, 0, "images");
    all.addEventListener("click", () => this.host.openLibrary({ folder: null, collection: null, search: "", similarTo: null }));
    if (info?.home) this.renderShared(info);

    const vaultFolders = folders.filter((f) => !f.external);
    // Empty folders too, so a new folder can receive pictures.
    for (const f of this.app.vault.getAllLoadedFiles()) {
      if (f instanceof TFolder && !f.isRoot() && f.children.length === 0) vaultFolders.push({ folder: f.path, count: 0, cover: "", external: false, latest: -Infinity });
    }
    if (vaultFolders.length) {
      const tree = buildTree(vaultFolders, "", this.app.vault.getName(), "/", false);
      this.section("This vault");
      this.renderNode(tree, 0, true);
    }
    const external = this.host.settings.external as string[];
    if (external.length) {
      this.section("On this computer");
      for (const root of external) {
        const sep = root.includes("\\") && !root.includes("/") ? "\\" : "/";
        const mine = folders.filter((f) => f.external && (f.folder === root || f.folder.startsWith(root + sep)));
        const name = root.split(/[\\/]/).filter(Boolean).pop() || root;
        const tree = buildTree(mine, root, name, sep, true);
        this.renderNode(tree, 0, true);
      }
    }
    this.listEl.scrollTop = top;
    this.markActive();
  }

  // ── the shared library (Pictogrep's collections and plugins) ───────────

  private renderShared(info: SharedInfo) {
    const head = this.listEl.createDiv("image-collections-section");
    head.setText("Shared library");
    if (info.running) head.createSpan({ cls: "image-collections-badge", text: "Pictogrep open" });
    const node = (key: string, name: string, count: number, cover: string): Node => ({ key, name, own: count, total: count, cover, latest: 0, children: new Map(), external: true });
    const all = this.row(this.listEl, node("shared:__shared__", "All shared pictures", info.total || 0, ""), 0, "library");
    all.addEventListener("click", () => this.host.openLibrary({ collection: "__shared__", folder: null, search: "", similarTo: null }));
    all.addEventListener("contextmenu", (e) => this.sharedMenu(e, null));
    this.dropShared(all, null);
    if (info.unsorted) {
      const u = this.row(this.listEl, node("shared:__unsorted__", "Not in a collection", info.unsorted, ""), 0, "inbox");
      u.addEventListener("click", () => this.host.openLibrary({ collection: "__unsorted__", folder: null, search: "", similarTo: null }));
    }
    // Collections nest with "/" in their names.
    const byName = new Map((info.collections || []).map((c) => [c.name, c]));
    const kids = (parent: string) => (info.collections || []).map((c) => c.name).filter((n) => (parent ? n.startsWith(parent + "/") && !n.slice(parent.length + 1).includes("/") : !n.includes("/")));
    const draw = (name: string, depth: number) => {
      const c = byName.get(name)!;
      const n = node("shared:" + name, name.split("/").pop()!, c.count, c.cover);
      for (const k of kids(name)) n.children.set(k, node(k, k, 0, ""));
      const el = this.row(this.listEl, n, depth, "folder");
      el.addEventListener("click", () => this.host.openLibrary({ collection: name, folder: null, recursive: true, search: "", similarTo: null }));
      el.addEventListener("contextmenu", (e) => this.sharedMenu(e, name));
      this.dropShared(el, name);
      if (!this.collapsed.has(n.key)) for (const k of kids(name)) draw(k, depth + 1);
    };
    for (const name of kids("")) draw(name, 1);
    const plugins = info.plugins || [];
    if (plugins.length) {
      this.section("Plugins");
      for (const p of plugins) {
        const el = this.listEl.createDiv({ cls: "image-collection tree-item-self is-clickable", attr: { "data-key": "plugin:" + p.id } });
        el.style.paddingInlineStart = "8px";
        el.createDiv("image-collection-toggle");
        setIcon(el.createDiv("image-collection-cover"), "puzzle");
        el.createDiv({ cls: "image-collection-name", text: p.name });
        el.createDiv({ cls: "image-collection-count", text: p.version });
        el.addEventListener("click", (e) => this.host.openPlugin(p.id, e.metaKey || e.ctrlKey));
      }
    }
  }

  /** Pictures dropped on a shared collection join it (copied into the library if needed). */
  private dropShared(el: HTMLElement, name: string | null) {
    el.addEventListener("dragover", (e) => {
      if (!e.dataTransfer?.types.includes("text/x-ateliet-images")) return;
      e.preventDefault();
      el.addClass("is-being-dragged-over");
    });
    el.addEventListener("dragleave", () => el.removeClass("is-being-dragged-over"));
    el.addEventListener("drop", async (e) => {
      el.removeClass("is-being-dragged-over");
      const raw = e.dataTransfer?.getData("text/x-ateliet-images");
      if (!raw) return;
      e.preventDefault();
      e.stopPropagation();
      const items: ImageItem[] = JSON.parse(raw);
      try {
        if (name) {
          const r = await api.sharedCollection("add", name, { paths: items.map((i) => i.abs) });
          new Notice(`${r.added} ${r.added === 1 ? "picture" : "pictures"} added to ${name}.`);
        } else {
          for (const it of items.filter((i) => i.origin !== "shared")) await api.sharedImport(it.name, await (await fetch(thumbnailUrlOriginal(it))).arrayBuffer());
          new Notice("Copied into the shared library.");
        }
      } catch (err) {
        new Notice(String(err));
      }
    });
  }

  private sharedMenu(e: MouseEvent, name: string | null) {
    const menu = new Menu();
    menu.addItem((i) => i.setTitle("New collection…").setIcon("folder-plus").onClick(async () => {
      const n = await askName(this.app, "New collection", name ? name + "/" : "");
      if (n) await api.sharedCollection("create", n).catch((err) => new Notice(String(err)));
    }));
    menu.addItem((i) => i.setTitle("Import from Pinterest…").setIcon("download").onClick(() => this.host.openPinterest(name ? { collection: name } : {})));
    if (name) {
      menu.addItem((i) => i.setTitle("Open in new tab").setIcon("images").onClick(() => this.host.openLibrary({ collection: name, folder: null, recursive: true, search: "", similarTo: null }, true)));
      menu.addItem((i) => i.setTitle("Rename…").setIcon("pencil").onClick(async () => {
        const n = await askName(this.app, "Rename collection", name);
        if (n && n !== name) await api.sharedCollection("rename", name, { into: n }).catch((err) => new Notice(String(err)));
      }));
      menu.addSeparator();
      menu.addItem((i) => i.setTitle("Delete collection").setIcon("trash-2").setWarning(true).onClick(async () => {
        // Only the collection goes away; its pictures stay in the library.
        await api.sharedCollection("delete", name).catch((err) => new Notice(String(err)));
      }));
    }
    menu.showAtMouseEvent(e);
  }

  private section(text: string) {
    this.listEl.createDiv({ cls: "image-collections-section", text });
  }

  private row(parent: HTMLElement, n: Node, depth: number, icon?: string): HTMLElement {
    const el = parent.createDiv({ cls: "image-collection tree-item-self is-clickable", attr: { "data-key": n.key } });
    el.style.paddingInlineStart = `${8 + depth * 14}px`;
    const toggle = el.createDiv("image-collection-toggle");
    if (n.children.size) {
      setIcon(toggle, "right-triangle");
      toggle.toggleClass("is-collapsed", this.collapsed.has(n.key));
      toggle.addEventListener("click", (e) => {
        e.stopPropagation();
        this.collapsed.has(n.key) ? this.collapsed.delete(n.key) : this.collapsed.add(n.key);
        this.app.workspace.requestSaveLayout();
        this.render();
      });
    }
    const cover = el.createDiv("image-collection-cover");
    if (n.cover) cover.createEl("img", { attr: { src: thumbnailUrl(n.cover, 240), loading: "lazy" } });
    else setIcon(cover, icon || "folder");
    el.createDiv({ cls: "image-collection-name", text: n.name });
    el.createDiv({ cls: "image-collection-count", text: n.total.toLocaleString() });
    return el;
  }

  private renderNode(n: Node, depth: number, isRoot: boolean) {
    const el = this.row(this.listEl, n, depth, isRoot ? (n.external ? "hard-drive" : "vault") : "folder");
    el.addEventListener("click", () => this.host.openLibrary({ folder: n.key, recursive: true, search: "", similarTo: null }));
    el.addEventListener("contextmenu", (e) => this.menu(e, n, isRoot));
    this.dropTarget(el, n);
    if (this.collapsed.has(n.key)) return;
    const kids = [...n.children.values()].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
    for (const c of kids) this.renderNode(c, depth + 1, false);
  }

  private dropTarget(el: HTMLElement, n: Node) {
    el.addEventListener("dragover", (e) => {
      if (!e.dataTransfer?.types.includes("text/x-ateliet-images")) return;
      e.preventDefault();
      el.addClass("is-being-dragged-over");
    });
    el.addEventListener("dragleave", () => el.removeClass("is-being-dragged-over"));
    el.addEventListener("drop", async (e) => {
      el.removeClass("is-being-dragged-over");
      const raw = e.dataTransfer?.getData("text/x-ateliet-images");
      if (!raw) return;
      e.preventDefault();
      if (n.external) return new Notice("Pictures can only be moved into folders of the vault.");
      const items: ImageItem[] = JSON.parse(raw);
      let moved = 0;
      for (const it of items) {
        if (it.folder === n.key && !it.external) continue;
        const f = vaultFile(this.app, it);
        if (f) {
          const dest = this.app.vault.getAvailablePath((n.key ? n.key + "/" : "") + f.basename, f.extension);
          await this.app.fileManager.renameFile(f, dest);
        } else {
          await ensureInVault(this.app, it, "", n.key || "/");
        }
        moved++;
      }
      if (moved) new Notice(`${moved} ${moved === 1 ? "picture" : "pictures"} moved to ${n.name}.`);
    });
  }

  private menu(e: MouseEvent, n: Node, isRoot: boolean) {
    const menu = new Menu();
    menu.addItem((i) => i.setTitle("Open in new tab").setIcon("images").onClick(() => this.host.openLibrary({ folder: n.key, recursive: true, search: "", similarTo: null }, true)));
    menu.addItem((i) => i.setTitle("Only this folder, without subfolders").setIcon("folder").onClick(() => this.host.openLibrary({ folder: n.key, recursive: false, search: "", similarTo: null })));
    if (!n.external) {
      menu.addItem((i) => i.setTitle("Note for this collection").setIcon("file-text").onClick(() => this.folderNote(n)));
      menu.addItem((i) => i.setTitle("New folder inside").setIcon("folder-plus").onClick(() => this.newFolder(n.key)));
    }
    menu.addItem((i) =>
      i.setTitle("Show in system explorer").setIcon("folder-open").onClick(() => invoke("show_in_folder", { path: n.external ? n.key : this.app.vault.adapter.getFullPath(n.key) })),
    );
    if (n.external && isRoot) {
      menu.addSeparator();
      menu.addItem((i) => i.setTitle("Remove from library").setIcon("x").onClick(() => this.host.removeExternalFolder(n.key)));
    }
    menu.showAtMouseEvent(e);
  }

  /** `<folder>/<folder name>.md`: notes about a collection live with it. */
  private async folderNote(n: Node) {
    const name = n.key ? n.name : this.app.vault.getName();
    const path = (n.key ? n.key + "/" : "") + name + ".md";
    let file = this.app.vault.getAbstractFileByPath(path);
    if (!(file instanceof TFile)) file = await this.app.vault.create(path, `# ${name}\n\n`);
    await this.host.openNote(file as TFile);
  }

  private async newFolder(parent: string) {
    const path = this.app.vault.getAvailablePath((parent ? parent + "/" : "") + "Untitled", "");
    const folder: TFolder = await this.app.vault.createFolder(path);
    new Notice(`Created ${folder.path}. Drag pictures onto it, or rename it in the file explorer.`);
    this.render();
  }
}
