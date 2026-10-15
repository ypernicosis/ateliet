// Sidebar panes: backlinks, outgoing links, tags, outline, bookmarks.

import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Keymap, Menu } from "../obsidian/ui.ts";
import { TFile } from "../obsidian/vault.ts";
import { debounce, getAllTags, getLinkpath, iterateCacheRefs, parseLinktext } from "../obsidian/util.ts";

abstract class ActiveFilePane extends ItemView {
  // View declares `file` as a read-only getter; panes track the note they describe.
  private shownFile: TFile | null = null;
  override get file(): TFile | null {
    return this.shownFile;
  }
  override set file(f: TFile | null) {
    this.shownFile = f;
  }
  protected update = debounce(() => this.render(), 200, true);

  async onOpen() {
    this.headerEl.hide();
    this.contentEl.addClass("ateliet-pane");
    this.registerEvent(this.app.workspace.on("file-open", () => this.update()));
    this.registerEvent(this.app.metadataCache.on("resolved", () => this.update()));
    this.registerEvent(this.app.metadataCache.on("changed", () => this.update()));
    this.render();
  }

  protected heading(text: string, count?: number): HTMLElement {
    const h = this.contentEl.createDiv("tree-item-self is-clickable pane-heading");
    h.createDiv({ cls: "tree-item-inner", text });
    if (count !== undefined) h.createDiv("tree-item-flair-outer").createSpan({ cls: "tree-item-flair", text: String(count) });
    return h;
  }

  protected empty(text: string) {
    this.contentEl.createDiv({ cls: "search-empty-state pane-empty", text });
  }

  abstract render(): void;
}

// ── backlinks ─────────────────────────────────────────────────────────────

export class BacklinkView extends ActiveFilePane {
  getViewType() {
    return "backlink";
  }
  getDisplayText() {
    return this.file ? `Backlinks for ${this.file.basename}` : "Backlinks";
  }
  getIcon() {
    return "links-coming-in";
  }

  async render() {
    const file = (this.file = this.app.workspace.getActiveFile());
    this.contentEl.empty();
    if (!file) return this.empty("No file is open.");
    const bl = this.app.metadataCache.getBacklinksForFile(file);
    const sources = bl.keys().sort();
    this.heading("Linked mentions", sources.length);
    const list = this.contentEl.createDiv("search-result-container backlink-pane");
    if (!sources.length) list.createDiv({ cls: "search-empty-state", text: "No backlinks found." });
    for (const src of sources) {
      const f = this.app.vault.getAbstractFileByPath(src);
      if (!(f instanceof TFile)) continue;
      const refs = bl.get(src) || [];
      const item = list.createDiv("tree-item search-result");
      const t = item.createDiv("tree-item-self search-result-file-title is-clickable");
      const collapse = t.createDiv("tree-item-icon collapse-icon");
      setIcon(collapse, "right-triangle");
      t.createDiv({ cls: "tree-item-inner", text: f.basename });
      t.createDiv("tree-item-flair-outer").createSpan({ cls: "tree-item-flair", text: String(refs.length) });
      t.addEventListener("click", (e) => this.app.workspace.getLeaf(Keymap.isModEvent(e)).openFile(f));
      const matches = item.createDiv("search-result-file-matches");
      const text = await this.app.vault.cachedRead(f);
      const lines = text.split("\n");
      for (const r of refs) {
        if (!r.position) continue;
        const line = lines[r.position.start.line] ?? "";
        const m = matches.createDiv("search-result-file-match tappable");
        const s = r.position.start.col, e = r.position.end.line === r.position.start.line ? r.position.end.col : line.length;
        m.appendText(line.slice(0, s).trimStart());
        m.createSpan({ cls: "search-result-file-matched-text", text: line.slice(s, e) });
        m.appendText(line.slice(e));
        m.addEventListener("click", (ev) => this.app.workspace.getLeaf(Keymap.isModEvent(ev)).openFile(f, { eState: { line: r.position.start.line } }));
      }
    }
    // unlinked mentions: plain-text occurrences of the note name
    const unlinked = this.heading("Unlinked mentions");
    unlinked.addClass("is-collapsed");
    const ul = this.contentEl.createDiv("search-result-container");
    ul.hide();
    unlinked.addEventListener("click", async () => {
      const show = unlinked.hasClass("is-collapsed");
      unlinked.toggleClass("is-collapsed", !show);
      ul.toggle(show);
      if (!show || ul.childElementCount) return;
      const name = file.basename;
      const re = new RegExp(`(^|[^\\w\\[])${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![\\w\\]])`, "i");
      for (const f of this.app.vault.getMarkdownFiles()) {
        if (f === file || sources.includes(f.path)) continue;
        const text = await this.app.vault.cachedRead(f);
        const lines = text.split("\n");
        const hits = lines.map((l: string, i: number) => [l, i] as const).filter(([l]: readonly [string, number]) => re.test(l) && !l.includes("[[" + name));
        if (!hits.length) continue;
        const item = ul.createDiv("tree-item search-result");
        const t = item.createDiv("tree-item-self search-result-file-title is-clickable");
        t.createDiv({ cls: "tree-item-inner", text: f.basename });
        t.addEventListener("click", () => this.app.workspace.getLeaf(false).openFile(f));
        for (const [l, i] of hits.slice(0, 5)) {
          const m = item.createDiv("search-result-file-match tappable");
          m.setText(l.trim());
          const link = m.createEl("button", { text: "Link", cls: "search-result-hover-button mod-small" });
          link.addEventListener("click", async (e) => {
            e.stopPropagation();
            await this.app.vault.process(f, (c: string) => {
              const ls = c.split("\n");
              ls[i] = ls[i].replace(re, (_m: string, pre: string) => `${pre}[[${name}]]`);
              return ls.join("\n");
            });
          });
          m.addEventListener("click", () => this.app.workspace.getLeaf(false).openFile(f, { eState: { line: i } }));
        }
      }
      if (!ul.childElementCount) ul.createDiv({ cls: "search-empty-state", text: "No unlinked mentions found." });
    });
  }
}

// ── outgoing links ────────────────────────────────────────────────────────

export class OutgoingLinkView extends ActiveFilePane {
  getViewType() {
    return "outgoing-link";
  }
  getDisplayText() {
    return this.file ? `Outgoing links from ${this.file.basename}` : "Outgoing links";
  }
  getIcon() {
    return "links-going-out";
  }
  render() {
    const file = (this.file = this.app.workspace.getActiveFile());
    this.contentEl.empty();
    if (!file) return this.empty("No file is open.");
    const cache = this.app.metadataCache.getFileCache(file);
    const resolved = new Map<string, TFile>();
    const unresolved = new Set<string>();
    iterateCacheRefs(cache, (ref: any) => {
      const lp = getLinkpath(ref.link);
      if (!lp) return;
      const dest = this.app.metadataCache.getFirstLinkpathDest(lp, file.path);
      if (dest) resolved.set(dest.path, dest);
      else unresolved.add(lp);
    });
    this.heading("Links", resolved.size);
    const a = this.contentEl.createDiv("search-result-container");
    for (const f of resolved.values()) {
      const t = a.createDiv("tree-item-self is-clickable outgoing-link-item");
      setIcon(t.createDiv("tree-item-icon"), "file");
      t.createDiv({ cls: "tree-item-inner", text: f.extension === "md" ? f.basename : f.name });
      t.addEventListener("click", (e) => this.app.workspace.getLeaf(Keymap.isModEvent(e)).openFile(f));
    }
    this.heading("Unresolved links", unresolved.size);
    const b = this.contentEl.createDiv("search-result-container");
    for (const l of unresolved) {
      const t = b.createDiv("tree-item-self is-clickable outgoing-link-item is-unresolved");
      setIcon(t.createDiv("tree-item-icon"), "file-question");
      t.createDiv({ cls: "tree-item-inner", text: l });
      t.addEventListener("click", () => this.app.workspace.openLinkText(l, file.path, false));
    }
  }
}

// ── tags ──────────────────────────────────────────────────────────────────

interface TagNode {
  name: string;
  full: string;
  count: number;
  children: Map<string, TagNode>;
}

export class TagView extends ItemView {
  sortByCount = true;
  collapsed = new Set<string>();
  private update = debounce(() => this.render(), 300, true);
  getViewType() {
    return "tag";
  }
  getDisplayText() {
    return "Tags";
  }
  getIcon() {
    return "tags";
  }
  async onOpen() {
    this.headerEl.hide();
    this.registerEvent(this.app.metadataCache.on("resolved", () => this.update()));
    this.render();
  }
  render() {
    this.contentEl.empty();
    const header = this.contentEl.createDiv("nav-header");
    const btns = header.createDiv("nav-buttons-container");
    const sort = btns.createDiv({ cls: "clickable-icon nav-action-button", attr: { "aria-label": "Change sort order" } });
    setIcon(sort, "arrow-up-narrow-wide");
    sort.addEventListener("click", () => {
      this.sortByCount = !this.sortByCount;
      this.render();
    });
    const counts: Record<string, number> = {};
    for (const f of this.app.vault.getMarkdownFiles()) {
      for (const t of new Set(getAllTags(this.app.metadataCache.getFileCache(f)) || [])) counts[t.toLowerCase()] = (counts[t.toLowerCase()] || 0) + 1;
    }
    const root: TagNode = { name: "", full: "", count: 0, children: new Map() };
    for (const tag in counts) {
      const parts = tag.slice(1).split("/");
      let node = root;
      let full = "#";
      for (const p of parts) {
        full += (full === "#" ? "" : "/") + p;
        if (!node.children.has(p)) node.children.set(p, { name: p, full, count: 0, children: new Map() });
        node = node.children.get(p)!;
        node.count += counts[tag];
      }
    }
    const container = this.contentEl.createDiv("tag-container");
    const walk = (node: TagNode, el: HTMLElement) => {
      const kids = Array.from(node.children.values()).sort((a, b) => (this.sortByCount ? b.count - a.count : 0) || a.name.localeCompare(b.name));
      for (const k of kids) {
        const item = el.createDiv("tree-item");
        const self = item.createDiv("tree-item-self tag-pane-tag is-clickable");
        if (k.children.size) {
          const ci = self.createDiv("tree-item-icon collapse-icon");
          setIcon(ci, "right-triangle");
          const isC = this.collapsed.has(k.full);
          ci.toggleClass("is-collapsed", isC);
          ci.addEventListener("click", (e) => {
            e.stopPropagation();
            isC ? this.collapsed.delete(k.full) : this.collapsed.add(k.full);
            this.render();
          });
        }
        const inner = self.createDiv("tree-item-inner");
        inner.createSpan({ cls: "tag-pane-tag-text", text: k.name });
        self.createDiv("tree-item-flair-outer").createSpan({ cls: "tag-pane-tag-count tree-item-flair", text: String(k.count) });
        self.addEventListener("click", () => this.app.internalPlugins.getPluginById("global-search")?.instance?.openGlobalSearch(`tag:${k.full}`));
        if (k.children.size && !this.collapsed.has(k.full)) walk(k, item.createDiv("tree-item-children"));
      }
    };
    walk(root, container);
    if (!Object.keys(counts).length) container.createDiv({ cls: "pane-empty", text: "No tags found." });
  }
}

// ── outline ───────────────────────────────────────────────────────────────

export class OutlineView extends ActiveFilePane {
  getViewType() {
    return "outline";
  }
  getDisplayText() {
    return this.file ? `Outline of ${this.file.basename}` : "Outline";
  }
  getIcon() {
    return "list";
  }
  render() {
    const file = (this.file = this.app.workspace.getActiveFile());
    this.contentEl.empty();
    if (!file) return this.empty("No file is open.");
    const headings = this.app.metadataCache.getFileCache(file)?.headings || [];
    if (!headings.length) return this.empty("No headings found.");
    const root = this.contentEl.createDiv("outline");
    const min = Math.min(...headings.map((h: any) => h.level));
    for (const h of headings) {
      const item = root.createDiv("tree-item");
      const self = item.createDiv("tree-item-self is-clickable");
      self.style.paddingInlineStart = `${(h.level - min) * 16 + 8}px`;
      self.createDiv({ cls: "tree-item-inner", text: h.heading });
      self.addEventListener("click", async () => {
        const leaf = this.app.workspace.getLeavesOfType("markdown").find((l: any) => l.view.file === file) || this.app.workspace.getLeaf(false);
        if (leaf.view?.file !== file) await leaf.openFile(file);
        this.app.workspace.setActiveLeaf(leaf, { focus: true });
        leaf.view.setEphemeralState({ line: h.position.start.line, focus: true });
        if (leaf.view.getMode?.() === "preview") {
          const el = leaf.view.previewMode.containerEl.querySelector(`[data-line-start="${h.position.start.line}"]`) as HTMLElement;
          el?.scrollIntoView({ block: "start" });
        }
      });
    }
  }
}

// ── bookmarks (shared with Obsidian via bookmarks.json) ───────────────────

export class BookmarksPlugin {
  app: any;
  plugin: any;
  items: any[] = [];
  async onEnable(app: any) {
    this.app = app;
    await this.load();
    app.workspace.viewRegistry.registerView("bookmarks", (leaf: any) => new BookmarksView(leaf, this));
    app.commands.addCommand({ id: "bookmarks:bookmark-current-view", name: "Bookmarks: Bookmark current file", checkCallback: (c: boolean) => {
      const f = app.workspace.getActiveFile();
      if (!f) return false;
      if (!c) this.toggle(f);
      return true;
    } });
    app.commands.addCommand({ id: "bookmarks:open", name: "Bookmarks: Show bookmarks", callback: () => app.workspace.ensureSideLeaf("bookmarks", "left", { reveal: true }) });
    app.workspace.on("file-menu", (menu: Menu, file: any) => {
      if (!(file instanceof TFile)) return;
      const has = this.find(file.path);
      menu.addItem((i) => i.setSection("action").setTitle(has ? "Remove bookmark" : "Bookmark...").setIcon("bookmark").onClick(() => this.toggle(file)));
    });
  }
  async load() {
    const data = await this.app.readJson("bookmarks.json", { items: [] });
    this.items = data.items || [];
    this.app.workspace.trigger("bookmarks-changed");
  }
  async save() {
    const data = await this.app.readJson("bookmarks.json", {});
    data.items = this.items;
    await this.app.writeJson("bookmarks.json", data);
    this.app.workspace.trigger("bookmarks-changed");
  }
  find(path: string, items = this.items): any {
    for (const it of items) {
      if (it.type === "file" && it.path === path) return it;
      if (it.type === "group") {
        const r = this.find(path, it.items || []);
        if (r) return r;
      }
    }
    return null;
  }
  getBookmarks() {
    return this.items;
  }
  async toggle(file: TFile) {
    const existing = this.find(file.path);
    const remove = (items: any[]): any[] => items.filter((i) => i !== existing).map((i) => (i.type === "group" ? { ...i, items: remove(i.items || []) } : i));
    if (existing) this.items = remove(this.items);
    else this.items.push({ type: "file", ctime: Date.now(), path: file.path });
    await this.save();
  }
  addItem(item: any) {
    this.items.push(item);
    this.save();
  }
}

class BookmarksView extends ItemView {
  constructor(leaf: any, private bm: BookmarksPlugin) {
    super(leaf);
  }
  getViewType() {
    return "bookmarks";
  }
  getDisplayText() {
    return "Bookmarks";
  }
  getIcon() {
    return "bookmark";
  }
  async onOpen() {
    this.headerEl.hide();
    this.registerEvent(this.app.workspace.on("bookmarks-changed", () => this.render()));
    this.render();
  }
  render() {
    this.contentEl.empty();
    const list = this.contentEl.createDiv("bookmarks-container");
    const walk = (items: any[], el: HTMLElement) => {
      for (const it of items) {
        const item = el.createDiv("tree-item");
        const self = item.createDiv("tree-item-self is-clickable bookmark");
        const icon = self.createDiv("tree-item-icon");
        if (it.type === "group") {
          setIcon(icon, "folder");
          self.createDiv({ cls: "tree-item-inner", text: it.title || "Group" });
          walk(it.items || [], item.createDiv("tree-item-children"));
        } else if (it.type === "file") {
          const { path, subpath } = parseLinktext(it.path + (it.subpath || ""));
          setIcon(icon, "file");
          const f = this.app.vault.getAbstractFileByPath(path);
          self.createDiv({ cls: "tree-item-inner", text: it.title || (f instanceof TFile ? f.basename : path) + (subpath || "") });
          self.addEventListener("click", (e) => f instanceof TFile && this.app.workspace.getLeaf(Keymap.isModEvent(e)).openFile(f, { eState: { subpath } }));
        } else if (it.type === "search") {
          setIcon(icon, "search");
          self.createDiv({ cls: "tree-item-inner", text: it.title || it.query });
          self.addEventListener("click", () => this.app.internalPlugins.getPluginById("global-search")?.instance?.openGlobalSearch(it.query));
        } else if (it.type === "folder") {
          setIcon(icon, "folder-open");
          self.createDiv({ cls: "tree-item-inner", text: it.title || it.path });
          self.addEventListener("click", () => {
            const f = this.app.vault.getAbstractFileByPath(it.path);
            if (f) this.app.internalPlugins.getPluginById("file-explorer")?.instance?.revealInFolder(f);
          });
        } else if (it.type === "url") {
          setIcon(icon, "globe");
          self.createDiv({ cls: "tree-item-inner", text: it.title || it.url });
          self.addEventListener("click", () => this.app.openExternal(it.url));
        } else {
          setIcon(icon, "bookmark");
          self.createDiv({ cls: "tree-item-inner", text: it.title || it.type });
        }
        self.addEventListener("contextmenu", (e) => {
          const m = new Menu();
          m.addItem((i) => i.setTitle("Remove").setIcon("trash-2").onClick(() => {
            const rm = (xs: any[]): any[] => xs.filter((x) => x !== it).map((x) => (x.type === "group" ? { ...x, items: rm(x.items || []) } : x));
            this.bm.items = rm(this.bm.items);
            this.bm.save();
          }));
          m.showAtMouseEvent(e);
        });
      }
    };
    walk(this.bm.items, list);
    if (!this.bm.items.length) list.createDiv({ cls: "pane-empty", text: "No bookmarks yet." });
  }
}

export function simplePanePlugin(id: string, type: string, name: string, side: "left" | "right", icon: string, View: any) {
  return {
    app: null as any,
    plugin: null as any,
    onEnable(app: any) {
      this.app = app;
      app.workspace.viewRegistry.registerView(type, (leaf: any) => new View(leaf));
      app.commands.addCommand({ id: `${id}:open`, name: `${name}: Show ${name.toLowerCase()}`, icon, callback: () => app.workspace.ensureSideLeaf(type, side, { reveal: true }) });
    },
  };
}
