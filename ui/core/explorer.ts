// File explorer core plugin.

import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Keymap, Menu } from "../obsidian/ui.ts";
import { TAbstractFile, TFile, TFolder } from "../obsidian/vault.ts";
import { debounce } from "../obsidian/util.ts";
import { openMoveModal } from "./search.ts";

type SortOrder = "alphabetical" | "alphabeticalReverse" | "byModifiedTime" | "byModifiedTimeReverse" | "byCreatedTime" | "byCreatedTimeReverse";

interface FileItem {
  el: HTMLElement;
  selfEl: HTMLElement;
  innerEl: HTMLElement;
  file: TAbstractFile;
  childrenEl?: HTMLElement;
  collapsed?: boolean;
  setCollapsed?: (c: boolean) => void;
}

export class FileExplorerView extends ItemView {
  fileItems: Record<string, FileItem> = {};
  sortOrder: SortOrder = "alphabetical";
  expanded = new Set<string>();
  navFilesContainer!: HTMLElement;
  private activeEl: HTMLElement | null = null;
  private refresh = debounce(() => this.render(), 50, true);
  autoReveal = false;

  getViewType() {
    return "file-explorer";
  }
  getDisplayText() {
    return "Files";
  }
  getIcon() {
    return "folder-closed";
  }

  async onOpen() {
    this.containerEl.addClass("mod-file-explorer");
    this.headerEl.hide();
    const header = this.contentEl.createDiv("nav-header");
    const buttons = header.createDiv("nav-buttons-container");
    const btn = (icon: string, label: string, cb: (e: MouseEvent) => any) => {
      const b = buttons.createDiv({ cls: "clickable-icon nav-action-button", attr: { "aria-label": label } });
      setIcon(b, icon);
      b.addEventListener("click", cb);
      return b;
    };
    btn("square-pen", "New note", () => this.newNote(this.app.fileManager.getNewFileParent(this.app.workspace.getActiveFile()?.path || "")));
    btn("folder-plus", "New folder", () => this.newFolder(this.app.vault.getRoot()));
    btn("arrow-up-narrow-wide", "Change sort order", (e) => this.sortMenu(e));
    btn("chevrons-down-up", "Collapse all", () => {
      this.expanded.clear();
      this.render();
      this.app.workspace.requestSaveLayout();
    });
    this.navFilesContainer = this.contentEl.createDiv({ cls: "nav-files-container node-insert-event", attr: { tabindex: "-1" } });
    this.navFilesContainer.addEventListener("contextmenu", (e) => {
      if ((e.target as HTMLElement).closest(".tree-item-self")) return;
      e.preventDefault();
      this.folderMenu(this.app.vault.getRoot(), e);
    });
    this.setupRootDrop();

    const v = this.app.vault;
    this.registerEvent(v.on("create", () => this.refresh()));
    this.registerEvent(v.on("delete", () => this.refresh()));
    this.registerEvent(v.on("rename", (f: TAbstractFile, old: string) => {
      if (this.expanded.delete(old)) this.expanded.add(f.path);
      this.refresh();
    }));
    this.registerEvent(v.on("modify", () => this.sortOrder.startsWith("by") && this.refresh()));
    this.registerEvent(this.app.workspace.on("file-open", () => this.updateActive()));
    this.render();
  }

  getState() {
    return { sortOrder: this.sortOrder, expanded: Array.from(this.expanded) };
  }
  async setState(state: any) {
    if (state?.sortOrder) this.sortOrder = state.sortOrder;
    if (Array.isArray(state?.expanded)) this.expanded = new Set(state.expanded);
    this.render();
  }

  private sorted(folder: TFolder): TAbstractFile[] {
    const showAll = this.app.vault.getConfig("showUnsupportedFiles");
    const known = (f: TFile) => showAll || f.extension === "md" || this.app.workspace.viewRegistry.isExtensionRegistered(f.extension) || ["png", "jpg", "jpeg", "gif", "svg", "webp", "pdf", "mp3", "mp4", "webm", "wav", "m4a", "ogg", "mov", "canvas", "bmp", "avif", "flac"].includes(f.extension);
    const items = folder.children.filter((c) => !(c instanceof TFile) || known(c));
    const folders = items.filter((c) => c instanceof TFolder);
    const files = items.filter((c) => c instanceof TFile) as TFile[];
    const name = (a: TAbstractFile, b: TAbstractFile) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
    folders.sort(name);
    const o = this.sortOrder;
    files.sort((a, b) => {
      switch (o) {
        case "alphabeticalReverse": return -name(a, b);
        case "byModifiedTime": return b.stat.mtime - a.stat.mtime;
        case "byModifiedTimeReverse": return a.stat.mtime - b.stat.mtime;
        case "byCreatedTime": return b.stat.ctime - a.stat.ctime;
        case "byCreatedTimeReverse": return a.stat.ctime - b.stat.ctime;
        default: return name(a, b);
      }
    });
    if (o === "alphabeticalReverse") folders.reverse();
    return [...folders, ...files];
  }

  render() {
    if (!this.navFilesContainer) return;
    const scroll = this.navFilesContainer.scrollTop;
    this.navFilesContainer.empty();
    this.fileItems = {};
    const rootWrap = this.navFilesContainer.createDiv();
    const root = rootWrap.createDiv("tree-item nav-folder mod-root");
    const rootSelf = root.createDiv("tree-item-self nav-folder-title");
    rootSelf.createDiv({ cls: "tree-item-inner nav-folder-title-content", text: this.app.vault.getName() });
    const rootChildren = root.createDiv("tree-item-children nav-folder-children");
    this.fileItems["/"] = { el: root, selfEl: rootSelf, innerEl: rootSelf, file: this.app.vault.getRoot(), childrenEl: rootChildren };
    this.renderChildren(this.app.vault.getRoot(), rootChildren);
    this.navFilesContainer.scrollTop = scroll;
    this.updateActive();
  }

  private renderChildren(folder: TFolder, el: HTMLElement) {
    for (const child of this.sorted(folder)) {
      if (child instanceof TFolder) this.renderFolder(child, el);
      else this.renderFile(child as TFile, el);
    }
  }

  private renderFolder(folder: TFolder, parentEl: HTMLElement) {
    const el = parentEl.createDiv("tree-item nav-folder");
    const self = el.createDiv({ cls: "tree-item-self nav-folder-title is-clickable mod-collapsible", attr: { "data-path": folder.path, draggable: "true" } });
    const icon = self.createDiv("tree-item-icon collapse-icon nav-folder-collapse-indicator");
    setIcon(icon, "right-triangle");
    const inner = self.createDiv({ cls: "tree-item-inner nav-folder-title-content", text: folder.name });
    const collapsed = !this.expanded.has(folder.path);
    el.toggleClass("is-collapsed", collapsed);
    icon.toggleClass("is-collapsed", collapsed);
    const item: FileItem = { el, selfEl: self, innerEl: inner, file: folder, collapsed };
    let childrenEl: HTMLElement | null = null;
    const setCollapsed = (c: boolean) => {
      item.collapsed = c;
      el.toggleClass("is-collapsed", c);
      icon.toggleClass("is-collapsed", c);
      if (c) {
        this.expanded.delete(folder.path);
        childrenEl?.remove();
        childrenEl = null;
      } else {
        this.expanded.add(folder.path);
        if (!childrenEl) {
          childrenEl = el.createDiv("tree-item-children nav-folder-children");
          item.childrenEl = childrenEl;
          this.renderChildren(folder, childrenEl);
          this.updateActive();
        }
      }
      this.app.workspace.requestSaveLayout();
    };
    item.setCollapsed = setCollapsed;
    this.fileItems[folder.path] = item;
    if (!collapsed) {
      childrenEl = el.createDiv("tree-item-children nav-folder-children");
      item.childrenEl = childrenEl;
      this.renderChildren(folder, childrenEl);
    }
    self.addEventListener("click", (e) => {
      if (inner.isContentEditable) return;
      e.preventDefault();
      setCollapsed(!item.collapsed);
    });
    self.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      e.stopPropagation();
      this.folderMenu(folder, e);
    });
    this.setupDrag(self, folder);
    this.setupDrop(self, folder);
  }

  private renderFile(file: TFile, parentEl: HTMLElement) {
    const el = parentEl.createDiv("tree-item nav-file");
    const self = el.createDiv({ cls: "tree-item-self nav-file-title tappable is-clickable", attr: { "data-path": file.path, draggable: "true" } });
    const inner = self.createDiv({ cls: "tree-item-inner nav-file-title-content", text: file.extension === "md" ? file.basename : file.basename });
    if (file.extension !== "md") self.createDiv({ cls: "nav-file-tag", text: file.extension });
    this.fileItems[file.path] = { el, selfEl: self, innerEl: inner, file };
    self.addEventListener("click", (e) => {
      if (inner.isContentEditable) return;
      e.preventDefault();
      this.app.workspace.getLeaf(Keymap.isModEvent(e)).openFile(file);
    });
    self.addEventListener("auxclick", (e) => {
      if (e.button === 1) this.app.workspace.getLeaf("tab").openFile(file);
    });
    self.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      e.stopPropagation();
      this.fileMenu(file, e);
    });
    self.addEventListener("mouseover", (e) => {
      this.app.workspace.trigger("hover-link", { event: e, source: "file-explorer", hoverParent: this, targetEl: self, linktext: file.path, sourcePath: "" });
    });
    this.setupDrag(self, file);
    if (file.parent) this.setupDrop(self, file.parent);
  }

  private updateActive() {
    this.activeEl?.removeClass("is-active");
    const f = this.app.workspace.getActiveFile();
    const item = f && this.fileItems[f.path];
    if (item) {
      item.selfEl.addClass("is-active");
      this.activeEl = item.selfEl;
    }
  }

  revealInFolder(file: TAbstractFile) {
    let p = file.parent;
    while (p && !p.isRoot()) {
      this.expanded.add(p.path);
      p = p.parent;
    }
    this.render();
    const item = this.fileItems[file.path];
    item?.selfEl.scrollIntoView({ block: "center" });
    item?.selfEl.addClass("has-focus");
    setTimeout(() => item?.selfEl.removeClass("has-focus"), 1500);
  }

  // ── creating & renaming ──────────────────────────────────────────────────
  async newNote(folder: TFolder) {
    const f = await this.app.fileManager.createNewMarkdownFile(folder, "Untitled");
    await this.app.workspace.getLeaf(false).openFile(f, { state: { mode: "source" }, eState: { rename: "all" } });
    const view: any = this.app.workspace.activeLeaf?.view;
    if (view?.inlineTitleEl) {
      view.inlineTitleEl.focus();
      document.getSelection()?.selectAllChildren(view.inlineTitleEl);
    }
  }

  async newFolder(parent: TFolder) {
    const folder = await this.app.fileManager.createNewFolder(parent);
    if (!parent.isRoot()) this.expanded.add(parent.path);
    this.render();
    this.startRename(folder);
  }

  startRename(file: TAbstractFile) {
    const item = this.fileItems[file.path];
    if (!item) return;
    const inner = item.innerEl;
    const original = file instanceof TFile ? file.basename : file.name;
    inner.setText(original);
    inner.setAttr("contenteditable", "true");
    inner.addClass("is-being-renamed");
    inner.focus();
    document.getSelection()?.selectAllChildren(inner);
    const finish = async (commit: boolean) => {
      inner.removeAttribute("contenteditable");
      inner.removeClass("is-being-renamed");
      inner.removeEventListener("blur", onBlur);
      inner.removeEventListener("keydown", onKey);
      const name = (inner.textContent || "").trim();
      if (!commit || !name || name === original) {
        inner.setText(original);
        return;
      }
      const parent = file.parent && !file.parent.isRoot() ? file.parent.path + "/" : "";
      const newPath = parent + name + (file instanceof TFile && file.extension ? "." + file.extension : "");
      try {
        await this.app.fileManager.renameFile(file, newPath);
      } catch (e: any) {
        new this.app.Notice(String(e?.message || e));
        inner.setText(original);
      }
    };
    const onBlur = () => finish(true);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        finish(true);
      } else if (e.key === "Escape") {
        e.preventDefault();
        finish(false);
      }
    };
    inner.addEventListener("blur", onBlur);
    inner.addEventListener("keydown", onKey);
  }

  // ── menus ────────────────────────────────────────────────────────────────
  private sortMenu(e: MouseEvent) {
    const m = new Menu();
    const opts: [SortOrder, string][] = [
      ["alphabetical", "File name (A to Z)"], ["alphabeticalReverse", "File name (Z to A)"],
      ["byModifiedTime", "Modified time (new to old)"], ["byModifiedTimeReverse", "Modified time (old to new)"],
      ["byCreatedTime", "Created time (new to old)"], ["byCreatedTimeReverse", "Created time (old to new)"],
    ];
    for (const [k, label] of opts) m.addItem((i) => i.setTitle(label).setChecked(this.sortOrder === k).onClick(() => {
      this.sortOrder = k;
      this.render();
      this.app.workspace.requestSaveLayout();
    }));
    m.showAtMouseEvent(e);
  }

  private fileMenu(file: TFile, e: MouseEvent) {
    const m = new Menu();
    m.addItem((i) => i.setSection("open").setTitle("Open in new tab").setIcon("file-plus").onClick(() => this.app.workspace.getLeaf("tab").openFile(file)));
    m.addItem((i) => i.setSection("open").setTitle("Open to the right").setIcon("separator-vertical").onClick(() => this.app.workspace.getLeaf("split").openFile(file)));
    m.addItem((i) => i.setSection("action").setTitle("Make a copy").setIcon("files").onClick(async () => {
      const base = (file.parent && !file.parent.isRoot() ? file.parent.path + "/" : "") + file.basename;
      await this.app.vault.copy(file, this.app.vault.getAvailablePath(base, file.extension));
    }));
    m.addItem((i) => i.setSection("action").setTitle("Rename...").setIcon("pencil").onClick(() => this.startRename(file)));
    m.addItem((i) => i.setSection("action").setTitle("Move file to...").setIcon("folder-tree").onClick(() => openMoveModal(this.app, file)));
    m.addItem((i) => i.setSection("info").setTitle("Copy path").setIcon("copy").onClick(() => navigator.clipboard.writeText(file.path)));
    m.addItem((i) => i.setSection("system").setTitle("Reveal in Finder").setIcon("arrow-up-right").onClick(() => this.app.showInFolder(file.path)));
    m.addItem((i) => i.setSection("system").setTitle("Open in default app").setIcon("arrow-up-right").onClick(() => this.app.openWithDefaultApp(file.path)));
    m.addItem((i) => i.setSection("danger").setTitle("Delete").setIcon("trash-2").setWarning(true).onClick(() => this.app.fileManager.promptForDeletion(file)));
    this.app.workspace.trigger("file-menu", m, file, "file-explorer-context-menu", null);
    m.showAtMouseEvent(e);
  }

  private folderMenu(folder: TFolder, e: MouseEvent) {
    const m = new Menu();
    m.addItem((i) => i.setSection("action-primary").setTitle("New note").setIcon("square-pen").onClick(() => this.newNote(folder)));
    m.addItem((i) => i.setSection("action-primary").setTitle("New folder").setIcon("folder-plus").onClick(() => this.newFolder(folder)));
    if (!folder.isRoot()) {
      m.addItem((i) => i.setSection("action").setTitle("Rename...").setIcon("pencil").onClick(() => this.startRename(folder)));
      m.addItem((i) => i.setSection("danger").setTitle("Delete").setIcon("trash-2").setWarning(true).onClick(() => this.app.fileManager.promptForDeletion(folder)));
    }
    m.addItem((i) => i.setSection("info").setTitle("Copy path").setIcon("copy").onClick(() => navigator.clipboard.writeText(folder.path)));
    m.addItem((i) => i.setSection("system").setTitle("Reveal in Finder").setIcon("arrow-up-right").onClick(() => this.app.showInFolder(folder.path)));
    this.app.workspace.trigger("file-menu", m, folder, "file-explorer-context-menu", null);
    m.showAtMouseEvent(e);
  }

  // ── drag & drop ──────────────────────────────────────────────────────────
  private setupDrag(el: HTMLElement, file: TAbstractFile) {
    el.addEventListener("dragstart", (e) => {
      e.dataTransfer!.setData("text/x-ateliet-path", file.path);
      e.dataTransfer!.setData("text/plain", file instanceof TFile ? this.app.fileManager.generateMarkdownLink(file, "") : file.path);
      e.dataTransfer!.effectAllowed = "all";
      (this.app as any).dragManager = { draggable: { type: file instanceof TFile ? "file" : "folder", file } };
    });
  }

  private setupDrop(el: HTMLElement, target: TFolder) {
    el.addEventListener("dragover", (e) => {
      if (!e.dataTransfer?.types.includes("text/x-ateliet-path") && !e.dataTransfer?.types.includes("Files")) return;
      e.preventDefault();
      e.stopPropagation();
      const item = this.fileItems[target.path];
      this.navFilesContainer.querySelectorAll(".is-being-dragged-over").forEach((x) => x.removeClass("is-being-dragged-over"));
      (item?.el || el).addClass("is-being-dragged-over");
    });
    el.addEventListener("dragleave", () => (this.fileItems[target.path]?.el || el).removeClass("is-being-dragged-over"));
    el.addEventListener("drop", (e) => this.onDrop(e, target));
  }

  private setupRootDrop() {
    this.navFilesContainer.addEventListener("dragover", (e) => {
      if (e.dataTransfer?.types.includes("text/x-ateliet-path") || e.dataTransfer?.types.includes("Files")) e.preventDefault();
    });
    this.navFilesContainer.addEventListener("drop", (e) => this.onDrop(e, this.app.vault.getRoot()));
  }

  private async onDrop(e: DragEvent, target: TFolder) {
    e.preventDefault();
    e.stopPropagation();
    this.navFilesContainer.querySelectorAll(".is-being-dragged-over").forEach((x) => x.removeClass("is-being-dragged-over"));
    const path = e.dataTransfer?.getData("text/x-ateliet-path");
    if (path) {
      const file = this.app.vault.getAbstractFileByPath(path);
      if (!file || file === target || file.parent === target) return;
      if (file instanceof TFolder && (target.path + "/").startsWith(file.path + "/")) return;
      const dest = (target.isRoot() ? "" : target.path + "/") + file.name;
      try {
        await this.app.fileManager.renameFile(file, dest);
      } catch (err: any) {
        new this.app.Notice(String(err?.message || err));
      }
      return;
    }
    // Files from the OS: copy into the folder.
    for (const f of Array.from(e.dataTransfer?.files || [])) {
      const dot = f.name.lastIndexOf(".");
      const base = (target.isRoot() ? "" : target.path + "/") + (dot > 0 ? f.name.slice(0, dot) : f.name);
      const ext = dot > 0 ? f.name.slice(dot + 1) : "";
      await this.app.vault.createBinary(this.app.vault.getAvailablePath(base, ext), await f.arrayBuffer());
    }
  }
}

export class FileExplorerPlugin {
  app: any;
  plugin: any;
  onEnable(app: any) {
    this.app = app;
    app.workspace.viewRegistry.registerView("file-explorer", (leaf: any) => new FileExplorerView(leaf));
    app.commands.addCommand({ id: "file-explorer:open", name: "Files: Show file explorer", callback: () => this.reveal() });
    app.commands.addCommand({ id: "file-explorer:reveal-active-file", name: "Files: Reveal current file in navigation", checkCallback: (c: boolean) => {
      const f = app.workspace.getActiveFile();
      if (!f) return false;
      if (!c) this.revealInFolder(f);
      return true;
    } });
    app.commands.addCommand({ id: "file-explorer:new-file", name: "Create new note", icon: "square-pen", hotkeys: [{ modifiers: ["Mod"], key: "N" }], callback: () => this.newNote(false) });
    app.commands.addCommand({ id: "file-explorer:new-file-in-new-pane", name: "Create note in new tab", hotkeys: [{ modifiers: ["Mod", "Shift"], key: "N" }], callback: () => this.newNote(true) });
    app.commands.addCommand({ id: "file-explorer:new-folder", name: "Create new folder", callback: () => this.view()?.newFolder(app.vault.getRoot()) });
  }
  view(): FileExplorerView | null {
    return this.app.workspace.getLeavesOfType("file-explorer")[0]?.view || null;
  }
  async reveal() {
    const leaf = await this.app.workspace.ensureSideLeaf("file-explorer", "left", { reveal: true });
    return leaf.view as FileExplorerView;
  }
  async revealInFolder(file: TAbstractFile) {
    const v = await this.reveal();
    v.revealInFolder(file);
  }
  async newNote(newTab: boolean) {
    const parent = this.app.fileManager.getNewFileParent(this.app.workspace.getActiveFile()?.path || "");
    const f = await this.app.fileManager.createNewMarkdownFile(parent, "Untitled");
    await this.app.workspace.getLeaf(newTab ? "tab" : false).openFile(f, { state: { mode: "source" } });
    const view: any = this.app.workspace.activeLeaf?.view;
    if (view?.inlineTitleEl && this.app.vault.getConfig("showInlineTitle")) {
      view.inlineTitleEl.focus();
      document.getSelection()?.selectAllChildren(view.inlineTitleEl);
    } else view?.editor?.focus();
  }
}
