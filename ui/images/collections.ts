// Folders as collections: the left pane of image mode. Only folders that hold
// pictures (directly or below) are listed, each with a cover and a count.
// Pictures dragged onto a folder move into it; every folder can have a note.

import { FolderInfo, ImageItem, api, ensureInVault, thumbnailUrl, vaultFile } from "./api.ts";
import { ImagesHost } from "./actions.ts";
import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Menu, Notice } from "../obsidian/ui.ts";
import { TFile, TFolder } from "../obsidian/vault.ts";
import { debounce } from "../obsidian/util.ts";
import { invoke } from "../tauri.ts";

export const COLLECTIONS_VIEW = "image-collections";

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
    this.render();
  }

  private update = debounce(() => this.render(), 600, true);

  private current(): string | null | undefined {
    const leaf = this.app.workspace.getLeavesOfType("image-library")[0];
    return leaf ? (leaf.view as any).state?.folder : undefined;
  }

  private markActive() {
    const cur = this.current();
    this.listEl.querySelectorAll(".image-collection").forEach((el: any) => el.toggleClass("is-active", el.dataset.key === (cur === null ? "__all__" : cur)));
  }

  async render() {
    let folders: FolderInfo[] = [];
    try {
      folders = await api.folders();
    } catch {
      return;
    }
    const top = this.listEl.scrollTop;
    this.listEl.empty();
    const total = folders.reduce((n, f) => n + f.count, 0);
    const all = this.row(this.listEl, { key: "__all__", name: "All pictures", own: total, total, cover: "", latest: 0, children: new Map(), external: false }, 0, "images");
    all.addEventListener("click", () => this.host.openLibrary({ folder: null, search: "", similarTo: null }));

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
