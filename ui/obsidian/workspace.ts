// Workspace: splits, tab groups, leaves, sidedocks and the ribbon. The layout
// serialises to the same JSON shape as Obsidian's workspace.json.

import { Events } from "./events.ts";
import { setIcon } from "./icons.ts";
import { Menu, Keymap } from "./ui.ts";
import { debounce, parseLinktext } from "./util.ts";
import { TFile } from "./vault.ts";
import { FileView, ItemView, MarkdownView, View } from "./views.ts";
import { EditorState } from "@codemirror/state";
import { language } from "@codemirror/language";
import { obsidianMarkdown } from "../editor/syntax.ts";

let idCounter = 0;
const newId = () => (Date.now().toString(16) + (idCounter++).toString(16)).slice(-16).padStart(16, "0");

export abstract class WorkspaceItem extends Events {
  app: any;
  id = newId();
  parent: WorkspaceParent | null = null;
  containerEl!: HTMLElement;
  dimension: number | null = null;

  constructor(app: any) {
    super();
    this.app = app;
  }

  getRoot(): WorkspaceItem {
    let n: WorkspaceItem = this;
    while (n.parent) n = n.parent;
    return n;
  }
  getContainer(): WorkspaceContainer {
    return this.getRoot() as any;
  }
  abstract serialize(): any;
  /** @internal */ onResize() {}
}

export abstract class WorkspaceParent extends WorkspaceItem {
  children: WorkspaceItem[] = [];
  insertChild(index: number, child: WorkspaceItem, _resize?: boolean): void {
    child.parent?.removeChild(child);
    child.parent = this;
    if (index < 0 || index > this.children.length) index = this.children.length;
    this.children.splice(index, 0, child);
    this.renderChildren();
  }
  removeChild(child: WorkspaceItem, _resize?: boolean): void {
    const i = this.children.indexOf(child);
    if (i === -1) return;
    this.children.splice(i, 1);
    child.parent = null;
    child.containerEl.detach();
    this.renderChildren();
    this.onChildRemoved();
  }
  replaceChild(index: number, child: WorkspaceItem): void {
    const old = this.children[index];
    if (old) {
      old.parent = null;
      old.containerEl.detach();
    }
    child.parent?.removeChild(child);
    child.parent = this;
    this.children[index] = child;
    this.renderChildren();
  }
  /** @internal */ abstract renderChildren(): void;
  /** @internal */ onChildRemoved(): void {
    if (this.children.length === 0 && this.parent && !(this instanceof WorkspaceRoot) && !(this instanceof WorkspaceSidedock)) {
      this.parent.removeChild(this);
    } else if (this.children.length === 1 && this instanceof WorkspaceSplit && !(this instanceof WorkspaceRoot) && !(this instanceof WorkspaceSidedock) && this.parent) {
      // collapse a split that only holds one child
      const only = this.children[0];
      const p = this.parent;
      const idx = p.children.indexOf(this);
      p.replaceChild(idx, only);
    }
  }
  onResize(): void {
    for (const c of this.children) c.onResize();
  }
}

export class WorkspaceSplit extends WorkspaceParent {
  direction: "vertical" | "horizontal";

  constructor(app: any, direction: "vertical" | "horizontal" = "vertical") {
    super(app);
    this.direction = direction;
    this.containerEl = createDiv(`workspace-split mod-${direction}`);
  }

  renderChildren(): void {
    this.containerEl.querySelectorAll(":scope > .workspace-leaf-resize-handle").forEach((e) => e.remove());
    const els = this.children.map((c) => c.containerEl);
    this.containerEl.setChildrenInPlace(els);
    this.children.forEach((c, i) => {
      c.containerEl.style.flexGrow = String(c.dimension ?? 50);
      if (i < this.children.length - 1) {
        const handle = createDiv("workspace-leaf-resize-handle");
        c.containerEl.after(handle);
        this.attachResize(handle, i);
      }
    });
  }

  private attachResize(handle: HTMLElement, i: number) {
    handle.addEventListener("mousedown", (e) => {
      e.preventDefault();
      const a = this.children[i], b = this.children[i + 1];
      const vertical = this.direction === "vertical";
      const ra = a.containerEl.getBoundingClientRect(), rb = b.containerEl.getBoundingClientRect();
      const total = vertical ? ra.width + rb.width : ra.height + rb.height;
      const growTotal = (a.dimension ?? 50) + (b.dimension ?? 50);
      const start = vertical ? e.clientX : e.clientY;
      const startA = vertical ? ra.width : ra.height;
      document.body.addClass("is-grabbing");
      const move = (ev: MouseEvent) => {
        const d = (vertical ? ev.clientX : ev.clientY) - start;
        const sizeA = Math.max(60, Math.min(total - 60, startA + d));
        a.dimension = (sizeA / total) * growTotal;
        b.dimension = growTotal - a.dimension;
        a.containerEl.style.flexGrow = String(a.dimension);
        b.containerEl.style.flexGrow = String(b.dimension);
        this.onResize();
      };
      const up = () => {
        document.body.removeClass("is-grabbing");
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
        this.app.workspace.requestSaveLayout();
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
    });
  }

  serialize(): any {
    return { id: this.id, type: "split", children: this.children.map((c) => c.serialize()), direction: this.direction, ...(this.dimension != null ? { dimension: this.dimension } : {}) };
  }
}

export class WorkspaceRoot extends WorkspaceSplit {
  win = window;
  doc = document;
  constructor(app: any) {
    super(app, "vertical");
    this.containerEl.addClass("mod-root");
  }
}

export class WorkspaceSidedock extends WorkspaceSplit {
  side: "left" | "right";
  collapsed = false;
  size = 300;
  private resizeHandle: HTMLElement;

  constructor(app: any, side: "left" | "right") {
    super(app, "horizontal");
    this.side = side;
    this.containerEl.addClass("mod-sidedock", `mod-${side}-split`);
    this.resizeHandle = createDiv("workspace-leaf-resize-handle");
    this.resizeHandle.addEventListener("mousedown", (e) => this.startResize(e));
    this.setSize(this.size);
  }

  renderChildren(): void {
    super.renderChildren();
    this.containerEl.appendChild(this.resizeHandle);
  }

  setSize(px: number) {
    this.size = Math.max(160, Math.min(800, px));
    this.containerEl.style.width = this.collapsed ? "0px" : this.size + "px";
  }

  private startResize(e: MouseEvent) {
    e.preventDefault();
    const start = e.clientX;
    const startSize = this.size;
    document.body.addClass("is-grabbing");
    const move = (ev: MouseEvent) => {
      const d = ev.clientX - start;
      this.setSize(this.side === "left" ? startSize + d : startSize - d);
      this.onResize();
    };
    const up = () => {
      document.body.removeClass("is-grabbing");
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      this.app.workspace.requestSaveLayout();
      this.app.workspace.trigger("resize");
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  toggle(): void {
    this.collapsed ? this.expand() : this.collapse();
  }
  collapse(): void {
    this.collapsed = true;
    this.containerEl.addClass("is-sidedock-collapsed");
    this.setSize(this.size);
    this.app.workspace.onSidedockToggle();
  }
  expand(): void {
    this.collapsed = false;
    this.containerEl.removeClass("is-sidedock-collapsed");
    this.setSize(this.size);
    this.app.workspace.onSidedockToggle();
  }

  serialize(): any {
    return { ...super.serialize(), width: this.size, collapsed: this.collapsed };
  }
}

export class WorkspaceMobileDrawer extends WorkspaceParent {
  renderChildren(): void {}
  serialize() {
    return {};
  }
}
export class WorkspaceFloating extends WorkspaceParent {
  renderChildren(): void {}
  serialize() {
    return {};
  }
}
export abstract class WorkspaceContainer extends WorkspaceSplit {}
export class WorkspaceWindow extends WorkspaceContainer {}

export class WorkspaceTabs extends WorkspaceParent {
  currentTab = 0;
  tabHeaderContainerEl: HTMLElement;
  tabHeaderEl: HTMLElement;
  tabsInnerEl: HTMLElement;
  tabsContainerEl: HTMLElement;
  newTabButtonEl: HTMLElement;
  isStacked = false;

  constructor(app: any) {
    super(app);
    this.containerEl = createDiv("workspace-tabs");
    this.tabHeaderContainerEl = this.containerEl.createDiv({ cls: "workspace-tab-header-container", attr: { "data-tauri-drag-region": "" } });
    this.tabsInnerEl = this.tabHeaderContainerEl.createDiv({ cls: "workspace-tab-header-container-inner", attr: { "data-tauri-drag-region": "" } });
    this.tabHeaderEl = this.tabsInnerEl;
    this.newTabButtonEl = this.tabHeaderContainerEl.createDiv({ cls: "workspace-tab-header-new-tab" });
    const btn = this.newTabButtonEl.createSpan({ cls: "clickable-icon", attr: { "aria-label": "New tab" } });
    setIcon(btn, "plus");
    btn.addEventListener("click", () => {
      const leaf = new WorkspaceLeaf(this.app);
      this.insertChild(this.currentTab + 1, leaf);
      leaf.setViewState({ type: "empty" }).then(() => this.app.workspace.setActiveLeaf(leaf, { focus: true }));
    });
    this.tabHeaderContainerEl.createDiv({ cls: "workspace-tab-header-spacer", attr: { "data-tauri-drag-region": "" } });
    const list = this.tabHeaderContainerEl.createDiv("workspace-tab-header-tab-list");
    const listBtn = list.createSpan({ cls: "clickable-icon", attr: { "aria-label": "Tab list" } });
    setIcon(listBtn, "chevron-down");
    listBtn.addEventListener("click", (e) => {
      const menu = new Menu();
      for (const c of this.children) {
        if (!(c instanceof WorkspaceLeaf)) continue;
        menu.addItem((i) => i.setTitle(c.getDisplayText()).setIcon(c.getIcon()).setChecked(this.children[this.currentTab] === c).onClick(() => this.app.workspace.setActiveLeaf(c, { focus: true })));
      }
      menu.showAtMouseEvent(e);
    });
    this.tabsContainerEl = this.containerEl.createDiv("workspace-tab-container");
    this.tabsInnerEl.addEventListener("dragover", (e) => {
      if (this.app.workspace.draggedLeaf) {
        e.preventDefault();
        this.tabsInnerEl.addClass("is-being-dragged-over");
      }
    });
    this.tabsInnerEl.addEventListener("dragleave", () => this.tabsInnerEl.removeClass("is-being-dragged-over"));
    this.tabsInnerEl.addEventListener("drop", (e) => {
      this.tabsInnerEl.removeClass("is-being-dragged-over");
      const leaf: WorkspaceLeaf | null = this.app.workspace.draggedLeaf;
      if (!leaf) return;
      e.preventDefault();
      // drop index from x position
      let idx = this.children.length;
      for (let i = 0; i < this.children.length; i++) {
        const c = this.children[i] as WorkspaceLeaf;
        const r = c.tabHeaderEl.getBoundingClientRect();
        if (e.clientX < r.left + r.width / 2) { idx = i; break; }
      }
      if (leaf.parent === this && this.children.indexOf(leaf) < idx) idx--;
      this.insertChild(idx, leaf);
      this.app.workspace.setActiveLeaf(leaf, { focus: true });
      this.app.workspace.requestSaveLayout();
    });
  }

  renderChildren(): void {
    const leaves = this.children as WorkspaceLeaf[];
    this.tabsInnerEl.setChildrenInPlace(leaves.map((l) => l.tabHeaderEl));
    this.tabsContainerEl.setChildrenInPlace(leaves.map((l) => l.containerEl));
    if (this.currentTab >= leaves.length) this.currentTab = Math.max(0, leaves.length - 1);
    this.updateVisibility();
  }

  /** @internal */ updateVisibility() {
    const leaves = this.children as WorkspaceLeaf[];
    leaves.forEach((l, i) => {
      const active = i === this.currentTab;
      l.containerEl.toggle(active);
      l.tabHeaderEl.toggleClass("is-active", active);
    });
    this.containerEl.toggleClass("mod-single-tab", leaves.length <= 1);
  }

  selectTab(leaf: WorkspaceLeaf): void {
    const i = this.children.indexOf(leaf);
    if (i === -1) return;
    this.currentTab = i;
    this.updateVisibility();
    leaf.onResize();
  }
  selectTabIndex(i: number): void {
    const leaf = this.children[i] as WorkspaceLeaf | undefined;
    if (leaf) this.selectTab(leaf);
  }

  serialize(): any {
    const out: any = { id: this.id, type: "tabs", children: this.children.map((c) => c.serialize()) };
    if (this.currentTab) out.currentTab = this.currentTab;
    if (this.dimension != null) out.dimension = this.dimension;
    if (this.isStacked) out.stacked = true;
    return out;
  }
}

// ── leaves ────────────────────────────────────────────────────────────────

class LeafHistory {
  backHistory: any[] = [];
  forwardHistory: any[] = [];
  constructor(private leaf: WorkspaceLeaf) {}
  push(entry: any) {
    this.backHistory.push(entry);
    if (this.backHistory.length > 50) this.backHistory.shift();
    this.forwardHistory = [];
  }
  async back() {
    const e = this.backHistory.pop();
    if (!e) return;
    this.forwardHistory.push(this.leaf.snapshot());
    await this.leaf.setViewState(e.state, e.eState, true);
  }
  async forward() {
    const e = this.forwardHistory.pop();
    if (!e) return;
    this.backHistory.push(this.leaf.snapshot());
    await this.leaf.setViewState(e.state, e.eState, true);
  }
}

export class EmptyView extends ItemView {
  constructor(leaf: any) {
    super(leaf);
    this.navigation = true;
  }
  getViewType() {
    return "empty";
  }
  getDisplayText() {
    return "New tab";
  }
  async onOpen() {
    const c = this.contentEl.createDiv("empty-state");
    const inner = c.createDiv("empty-state-container");
    inner.createDiv({ cls: "empty-state-title", text: "No file is open" });
    const actions = inner.createDiv("empty-state-action-list");
    const act = (text: string, cmd: string) => {
      const a = actions.createDiv({ cls: "empty-state-action tappable", text });
      a.addEventListener("click", () => this.app.commands.executeCommandById(cmd));
    };
    act("Create new note (⌘ N)", "file-explorer:new-file");
    act("Go to file (⌘ O)", "switcher:open");
    act("Close", "workspace:close");
  }
}

/** Keeps the state of a view whose type is not (yet) registered. */
export class PlaceholderView extends ItemView {
  constructor(leaf: any, private type: string, private state: any) {
    super(leaf);
  }
  getViewType() {
    return this.type;
  }
  getDisplayText() {
    return this.state?.file ? String(this.state.file).split("/").pop()!.replace(/\.md$/, "") : this.type;
  }
  getState() {
    return this.state || {};
  }
  async setState(s: any) {
    this.state = s;
  }
  async onOpen() {
    const c = this.contentEl.createDiv("empty-state");
    c.createDiv({ cls: "empty-state-title", text: `"${this.type}" view is not available.` });
    c.createDiv({ cls: "empty-state-message", text: "The plugin that provides it is disabled or not supported in Ateliet." });
  }
}

export class WorkspaceLeaf extends WorkspaceItem {
  view!: View;
  tabHeaderEl: HTMLElement;
  tabHeaderInnerIconEl: HTMLElement;
  tabHeaderInnerTitleEl: HTMLElement;
  tabHeaderStatusContainerEl: HTMLElement;
  tabHeaderCloseEl: HTMLElement;
  pinned = false;
  group: string | null = null;
  history: LeafHistory;
  working = false;
  isDeferred = false;
  /** @internal */ activeTime = 0;
  hoverPopover: any = null;

  constructor(app: any) {
    super(app);
    this.history = new LeafHistory(this);
    this.containerEl = createDiv("workspace-leaf");
    this.tabHeaderEl = createDiv({ cls: "workspace-tab-header tappable", attr: { draggable: "true" } });
    const inner = this.tabHeaderEl.createDiv("workspace-tab-header-inner");
    this.tabHeaderInnerIconEl = inner.createDiv("workspace-tab-header-inner-icon");
    this.tabHeaderInnerTitleEl = inner.createDiv("workspace-tab-header-inner-title");
    this.tabHeaderStatusContainerEl = inner.createDiv("workspace-tab-header-status-container");
    this.tabHeaderCloseEl = inner.createDiv({ cls: "workspace-tab-header-inner-close-button", attr: { "aria-label": "Close" } });
    setIcon(this.tabHeaderCloseEl, "x");
    this.tabHeaderCloseEl.addEventListener("click", (e) => {
      e.stopPropagation();
      this.detach();
    });
    this.tabHeaderEl.addEventListener("mousedown", (e) => {
      if (e.button === 1) {
        e.preventDefault();
        this.detach();
        return;
      }
      if (e.button === 0 && !(e.target as HTMLElement).closest(".workspace-tab-header-inner-close-button")) this.app.workspace.setActiveLeaf(this, { focus: true });
    });
    this.tabHeaderEl.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const menu = new Menu();
      menu.addItem((i) => i.setSection("close").setTitle("Close").setIcon("x").onClick(() => this.detach()));
      menu.addItem((i) => i.setSection("close").setTitle("Close others").onClick(() => {
        for (const c of (this.parent?.children || []).slice()) if (c !== this && c instanceof WorkspaceLeaf) c.detach();
      }));
      menu.addItem((i) => i.setSection("pane").setTitle(this.pinned ? "Unpin" : "Pin").setIcon("pin").onClick(() => this.togglePinned()));
      menu.addItem((i) => i.setSection("pane").setTitle("Split right").setIcon("separator-vertical").onClick(() => this.app.workspace.duplicateLeaf(this, "split", "vertical")));
      menu.addItem((i) => i.setSection("pane").setTitle("Split down").setIcon("separator-horizontal").onClick(() => this.app.workspace.duplicateLeaf(this, "split", "horizontal")));
      this.view?.onTabMenu?.(menu);
      this.view?.onPaneMenu(menu, "tab-header");
      menu.showAtMouseEvent(e);
    });
    this.tabHeaderEl.addEventListener("dragstart", (e) => {
      this.app.workspace.draggedLeaf = this;
      e.dataTransfer?.setData("text/plain", this.getDisplayText());
      this.tabHeaderEl.addClass("is-being-dragged");
    });
    this.tabHeaderEl.addEventListener("dragend", () => {
      this.app.workspace.draggedLeaf = null;
      this.tabHeaderEl.removeClass("is-being-dragged");
    });
    this.containerEl.addEventListener("mousedown", () => this.app.workspace.setActiveLeaf(this, { focus: false }), true);
    this.containerEl.addEventListener("focusin", () => this.app.workspace.setActiveLeaf(this, { focus: false }));
    // Plugins iterate leaves and expect `leaf.view` to exist at all times.
    this.view = new EmptyView(this);
    this.containerEl.appendChild(this.view.containerEl);
  }

  getViewState(): any {
    return { type: this.view?.getViewType() || "empty", state: this.view?.getState() || {}, ...(this.pinned ? { pinned: true } : {}), ...(this.group ? { group: this.group } : {}) };
  }

  getEphemeralState(): any {
    return this.view?.getEphemeralState() || {};
  }

  /** @internal */ snapshot() {
    return { state: this.getViewState(), eState: this.getEphemeralState(), title: this.getDisplayText(), icon: this.getIcon() };
  }

  async setViewState(viewState: any, eState?: any, fromHistory = false): Promise<void> {
    const ws = this.app.workspace;
    const type = viewState?.type || "empty";
    if (viewState?.pinned !== undefined) this.setPinned(!!viewState.pinned);
    if (viewState?.group !== undefined) this.group = viewState.group;
    const current = this.view;
    const navigates = current && current.navigation && !fromHistory && current.getViewType() !== "empty";
    const sameType = current && current._loaded && current.getViewType() === type && !(current instanceof PlaceholderView);
    if (navigates) {
      const before = JSON.stringify(current.getState());
      if (!sameType || before !== JSON.stringify(viewState?.state || {})) this.history.push(this.snapshot());
    }
    this.working = true;
    try {
      if (sameType) {
        await current.setState(viewState.state || {}, { history: !fromHistory });
      } else {
        const creator = ws.viewRegistry.viewByType[type];
        const view: View = creator ? creator(this) : type === "empty" ? new EmptyView(this) : new PlaceholderView(this, type, viewState.state);
        if (current) await this.closeView();
        this.view = view;
        view.containerEl.setAttr("data-type", type);
        this.containerEl.appendChild(view.containerEl);
        view.load();
        try {
          await view.onOpen();
        } catch (e) {
          console.error("onOpen", type, e);
        }
        try {
          await view.setState(viewState.state || {}, { history: false });
        } catch (e) {
          console.error("setState", type, e);
        }
      }
    } finally {
      this.working = false;
    }
    if (eState) this.setEphemeralState(eState);
    this.updateHeader();
    if (viewState?.active) ws.setActiveLeaf(this, { focus: true });
    ws.onLeafStateChanged(this);
  }

  private async closeView() {
    const v = this.view;
    if (!v) return;
    try {
      await v.onClose();
    } catch (e) {
      console.error(e);
    }
    v.unload();
    v.containerEl.detach();
  }

  setEphemeralState(state: any): void {
    try {
      this.view?.setEphemeralState(state);
    } catch (e) {
      console.error(e);
    }
  }

  async openFile(file: TFile, openState?: any): Promise<void> {
    const ws = this.app.workspace;
    const type = ws.viewRegistry.getTypeByExtension(file.extension) || "unsupported";
    const state = { file: file.path, ...(openState?.state || {}) };
    await this.setViewState({ type, state, active: openState?.active ?? true, group: openState?.group }, openState?.eState);
    ws.recordFileOpen(file);
  }

  async open(view: View): Promise<View> {
    await this.closeView();
    this.view = view;
    view.leaf = this;
    this.containerEl.appendChild(view.containerEl);
    view.load();
    await view.onOpen();
    this.updateHeader();
    return view;
  }

  getDisplayText(): string {
    return this.view?.getDisplayText() || "New tab";
  }
  getIcon(): string {
    return this.view?.getIcon() || "file";
  }

  /** @internal */ updateHeader() {
    this.tabHeaderInnerTitleEl.setText(this.getDisplayText());
    this.tabHeaderEl.setAttr("aria-label", this.getDisplayText());
    this.tabHeaderEl.setAttr("data-type", this.view?.getViewType() || "empty");
    setIcon(this.tabHeaderInnerIconEl, this.getIcon());
    this.tabHeaderEl.toggleClass("mod-pinned", this.pinned);
    if (this.view instanceof ItemView) this.view.updateHeader();
    this.app.workspace.updateTitle?.();
  }

  setPinned(pinned: boolean): void {
    this.pinned = pinned;
    this.tabHeaderEl.toggleClass("mod-pinned", pinned);
    this.trigger("pinned-change", pinned);
  }
  togglePinned(): void {
    this.setPinned(!this.pinned);
    this.app.workspace.requestSaveLayout();
  }
  setGroup(group: string): void {
    this.group = group;
  }
  setGroupMember(other: WorkspaceLeaf): void {
    this.group = other.group;
  }

  async loadIfDeferred(): Promise<void> {}

  onResize(): void {
    try {
      this.view?.onResize();
    } catch (e) {
      console.error(e);
    }
  }

  detach(): void {
    const ws = this.app.workspace;
    const parent = this.parent;
    const wasActive = ws.activeLeaf === this;
    this.closeView();
    if (parent) {
      const idx = parent.children.indexOf(this);
      parent.removeChild(this);
      if (parent instanceof WorkspaceTabs && parent.children.length) {
        parent.currentTab = Math.min(idx, parent.children.length - 1);
        parent.updateVisibility();
      }
    }
    this.trigger("detach");
    ws.onLeafDetached(this, wasActive);
  }

  serialize(): any {
    const st = this.getViewState();
    return { id: this.id, type: "leaf", ...(this.pinned ? { pinned: true } : {}), state: { type: st.type, state: st.state, icon: this.getIcon(), title: this.getDisplayText() } };
  }
}

// ── ribbon ────────────────────────────────────────────────────────────────

export class WorkspaceRibbon {
  containerEl: HTMLElement;
  actionsEl: HTMLElement;
  settingsEl: HTMLElement;
  items: { id: string; icon: string; title: string; el: HTMLElement; hidden: boolean }[] = [];
  app: any;

  constructor(app: any, side: "left" | "right") {
    this.app = app;
    this.containerEl = createDiv(`workspace-ribbon side-dock-ribbon mod-${side}`);
    this.containerEl.setAttr("data-tauri-drag-region", "");
    this.actionsEl = this.containerEl.createDiv({ cls: "side-dock-actions", attr: { "data-tauri-drag-region": "" } });
    this.settingsEl = this.containerEl.createDiv("side-dock-settings");
    this.actionsEl.addEventListener("contextmenu", (e) => {
      const menu = new Menu();
      for (const it of this.items) {
        menu.addItem((i) => i.setTitle(it.title).setIcon(it.icon).setChecked(!it.hidden).onClick(() => {
          it.hidden = !it.hidden;
          it.el.toggle(!it.hidden);
          this.app.workspace.requestSaveLayout();
        }));
      }
      menu.showAtMouseEvent(e);
    });
  }

  addRibbonItemButton(id: string, icon: string, title: string, callback: (evt: MouseEvent) => any): HTMLElement {
    const el = this.actionsEl.createDiv({ cls: "clickable-icon side-dock-ribbon-action", attr: { "aria-label": title, "data-tooltip-position": "right" } });
    setIcon(el, icon);
    el.addEventListener("click", callback);
    const hidden = (this.app.workspace.hiddenRibbonItems || []).includes(id);
    el.toggle(!hidden);
    this.items.push({ id, icon, title, el, hidden });
    return el;
  }

  removeRibbonAction(el: HTMLElement) {
    el.remove();
    this.items = this.items.filter((i) => i.el !== el);
  }

  addSettingsButton(icon: string, title: string, cb: (e: MouseEvent) => any): HTMLElement {
    const el = this.settingsEl.createDiv({ cls: "clickable-icon side-dock-ribbon-action", attr: { "aria-label": title, "data-tooltip-position": "right" } });
    setIcon(el, icon);
    el.addEventListener("click", cb);
    return el;
  }
}

// ── view registry ─────────────────────────────────────────────────────────

export class ViewRegistry extends Events {
  viewByType: Record<string, (leaf: WorkspaceLeaf) => View> = {};
  typeByExtension: Record<string, string> = {};

  registerView(type: string, creator: (leaf: WorkspaceLeaf) => View) {
    if (this.viewByType[type]) throw new Error(`Attempting to register an existing view type "${type}"`);
    this.viewByType[type] = creator;
    this.trigger("view-registered", type);
  }
  unregisterView(type: string) {
    delete this.viewByType[type];
    this.trigger("view-unregistered", type);
  }
  registerExtensions(exts: string[], type: string) {
    for (const e of exts) this.typeByExtension[e.toLowerCase()] = type;
    this.trigger("extensions-updated");
  }
  unregisterExtensions(exts: string[]) {
    for (const e of exts) delete this.typeByExtension[e.toLowerCase()];
    this.trigger("extensions-updated");
  }
  getTypeByExtension(ext: string): string | undefined {
    return this.typeByExtension[ext.toLowerCase()];
  }
  isExtensionRegistered(ext: string) {
    return ext.toLowerCase() in this.typeByExtension;
  }
}

// ── workspace ─────────────────────────────────────────────────────────────

export class Workspace extends Events {
  app: any;
  containerEl: HTMLElement;
  rootSplit: WorkspaceRoot;
  leftSplit: WorkspaceSidedock;
  rightSplit: WorkspaceSidedock;
  leftRibbon: WorkspaceRibbon;
  rightRibbon: WorkspaceRibbon;
  floatingSplit: WorkspaceFloating;
  activeLeaf: WorkspaceLeaf | null = null;
  activeEditor: any = null;
  layoutReady = false;
  requestSaveLayout: any;
  viewRegistry = new ViewRegistry();
  editorSuggest: any;
  hiddenRibbonItems: string[] = [];
  /** @internal */ draggedLeaf: WorkspaceLeaf | null = null;
  /** @internal */ lastOpenFiles: string[] = [];
  /** @internal */ editorExtensions: any[] = [];
  /** @internal */ hoverLinkSources: Record<string, any> = {};
  private layoutReadyCallbacks: (() => any)[] = [];
  private lastActiveFile: TFile | null = null;
  protocolHandlers: Record<string, any> = {};

  constructor(app: any) {
    super();
    this.app = app;
    this.containerEl = createDiv("workspace");
    this.leftRibbon = new WorkspaceRibbon(app, "left");
    this.rightRibbon = new WorkspaceRibbon(app, "right");
    this.leftSplit = new WorkspaceSidedock(app, "left");
    this.rootSplit = new WorkspaceRoot(app);
    this.rightSplit = new WorkspaceSidedock(app, "right");
    this.floatingSplit = new WorkspaceFloating(app);
    this.containerEl.append(this.leftRibbon.containerEl, this.leftSplit.containerEl, this.rootSplit.containerEl, this.rightSplit.containerEl);
    this.requestSaveLayout = debounce(() => this.saveLayout(), 1000, true);
    this.editorSuggest = new EditorSuggestManager();
    this.viewRegistry.on("view-registered", (type: string) => this.reloadPlaceholders(type));
    window.addEventListener("resize", debounce(() => {
      this.rootSplit.onResize();
      this.leftSplit.onResize();
      this.rightSplit.onResize();
      this.trigger("resize");
    }, 50));
  }

  // ── layout readiness ─────────────────────────────────────────────────────
  onLayoutReady(cb: () => any): void {
    if (this.layoutReady) setTimeout(cb, 0);
    else this.layoutReadyCallbacks.push(cb);
  }
  /** @internal */ setLayoutReady() {
    this.layoutReady = true;
    for (const cb of this.layoutReadyCallbacks.splice(0)) {
      try {
        cb();
      } catch (e) {
        console.error(e);
      }
    }
    this.trigger("layout-ready");
    this.trigger("layout-change");
  }

  // ── iteration & lookup ───────────────────────────────────────────────────
  iterateLeaves(item: WorkspaceItem, cb: (leaf: WorkspaceLeaf) => any): boolean;
  iterateLeaves(cb: (leaf: WorkspaceLeaf) => any, item: WorkspaceItem): boolean;
  iterateLeaves(a: any, b: any): boolean {
    const [item, cb] = typeof a === "function" ? [b, a] : [a, b];
    const walk = (n: WorkspaceItem): boolean => {
      if (n instanceof WorkspaceLeaf) return cb(n) === true;
      if (n instanceof WorkspaceParent) for (const c of n.children.slice()) if (walk(c)) return true;
      return false;
    };
    return walk(item);
  }
  iterateRootLeaves(cb: (leaf: WorkspaceLeaf) => any): void {
    this.iterateLeaves(this.rootSplit, cb);
  }
  iterateAllLeaves(cb: (leaf: WorkspaceLeaf) => any): void {
    this.iterateLeaves(this.rootSplit, cb);
    this.iterateLeaves(this.leftSplit, cb);
    this.iterateLeaves(this.rightSplit, cb);
  }
  getLeavesOfType(type: string): WorkspaceLeaf[] {
    const out: WorkspaceLeaf[] = [];
    this.iterateAllLeaves((l) => {
      if (l.view?.getViewType() === type) out.push(l);
    });
    return out;
  }
  getLeafById(id: string): WorkspaceLeaf | null {
    let found: WorkspaceLeaf | null = null;
    this.iterateAllLeaves((l) => {
      if (l.id === id) found = l;
    });
    return found;
  }
  getActiveViewOfType<T extends View>(type: abstract new (...a: any[]) => T): T | null {
    const v = this.activeLeaf?.view;
    return v instanceof type ? (v as T) : null;
  }
  getActiveFile(): TFile | null {
    const v: any = this.activeLeaf?.view;
    if (v && v.file instanceof TFile) return v.file;
    return this.lastActiveFile && !this.lastActiveFile.deleted ? this.lastActiveFile : null;
  }
  getMostRecentLeaf(root: WorkspaceItem = this.rootSplit): WorkspaceLeaf | null {
    let best: WorkspaceLeaf | null = null;
    this.iterateLeaves(root, (l) => {
      if (!best || l.activeTime > best.activeTime) best = l;
    });
    return best;
  }
  getLastOpenFiles(): string[] {
    return this.lastOpenFiles.slice(0, 10);
  }
  getGroupLeaves(group: string): WorkspaceLeaf[] {
    const out: WorkspaceLeaf[] = [];
    this.iterateAllLeaves((l) => l.group === group && out.push(l));
    return out;
  }

  // ── leaf creation ────────────────────────────────────────────────────────
  private rootTabsOf(leaf: WorkspaceLeaf | null): WorkspaceTabs {
    if (leaf && leaf.parent instanceof WorkspaceTabs && leaf.getRoot() === this.rootSplit) return leaf.parent;
    let tabs: WorkspaceTabs | null = null;
    const walk = (n: WorkspaceItem) => {
      if (tabs) return;
      if (n instanceof WorkspaceTabs) tabs = n;
      else if (n instanceof WorkspaceParent) n.children.forEach(walk);
    };
    walk(this.rootSplit);
    if (!tabs) {
      tabs = new WorkspaceTabs(this.app);
      this.rootSplit.insertChild(0, tabs);
    }
    return tabs;
  }

  getLeaf(newLeaf?: boolean | "tab" | "split" | "window", direction: "vertical" | "horizontal" = "vertical"): WorkspaceLeaf {
    const active = this.activeLeaf && this.activeLeaf.getRoot() === this.rootSplit ? this.activeLeaf : this.getMostRecentLeaf();
    if (newLeaf === "split") return this.createLeafBySplit(active || this.getLeaf(true), direction);
    if (newLeaf === true || newLeaf === "tab" || newLeaf === "window") {
      const tabs = this.rootTabsOf(active);
      const leaf = new WorkspaceLeaf(this.app);
      const idx = active && active.parent === tabs ? tabs.children.indexOf(active) + 1 : tabs.children.length;
      tabs.insertChild(idx, leaf);
      return leaf;
    }
    if (active && !active.pinned) return active;
    return this.getLeaf(true);
  }

  getUnpinnedLeaf(): WorkspaceLeaf {
    return this.getLeaf(false);
  }

  createLeafInParent(parent: WorkspaceSplit, index: number): WorkspaceLeaf {
    const tabs = new WorkspaceTabs(this.app);
    const leaf = new WorkspaceLeaf(this.app);
    tabs.insertChild(0, leaf);
    parent.insertChild(index, tabs);
    return leaf;
  }

  createLeafBySplit(leaf: WorkspaceLeaf, direction: "vertical" | "horizontal" = "vertical", before = false): WorkspaceLeaf {
    const tabs = leaf.parent as WorkspaceTabs;
    const container = tabs?.parent as WorkspaceSplit;
    const newLeaf = new WorkspaceLeaf(this.app);
    const newTabs = new WorkspaceTabs(this.app);
    newTabs.insertChild(0, newLeaf);
    if (!container) {
      this.rootSplit.insertChild(-1, newTabs);
      return newLeaf;
    }
    const idx = container.children.indexOf(tabs);
    if (container.direction === direction) {
      container.insertChild(before ? idx : idx + 1, newTabs);
    } else {
      const split = new WorkspaceSplit(this.app, direction);
      split.dimension = tabs.dimension;
      tabs.dimension = null;
      container.replaceChild(idx, split);
      split.insertChild(0, tabs);
      split.insertChild(before ? 0 : 1, newTabs);
    }
    return newLeaf;
  }

  splitActiveLeaf(direction: "vertical" | "horizontal" = "vertical"): WorkspaceLeaf {
    return this.createLeafBySplit(this.activeLeaf || this.getLeaf(), direction);
  }

  async duplicateLeaf(leaf: WorkspaceLeaf, leafType?: boolean | "tab" | "split" | "window", direction?: "vertical" | "horizontal"): Promise<WorkspaceLeaf> {
    const target = leafType === "split" ? this.createLeafBySplit(leaf, direction || "vertical") : this.getLeaf(leafType === undefined ? true : leafType);
    await target.setViewState({ ...leaf.getViewState(), active: true }, leaf.getEphemeralState());
    return target;
  }

  private sideLeaf(dock: WorkspaceSidedock, split: boolean): WorkspaceLeaf {
    let tabs = dock.children.find((c) => c instanceof WorkspaceTabs) as WorkspaceTabs | undefined;
    if (!tabs || split) {
      tabs = new WorkspaceTabs(this.app);
      dock.insertChild(-1, tabs);
    }
    const leaf = new WorkspaceLeaf(this.app);
    tabs.insertChild(-1, leaf);
    return leaf;
  }
  getLeftLeaf(split: boolean): WorkspaceLeaf {
    return this.sideLeaf(this.leftSplit, split);
  }
  getRightLeaf(split: boolean): WorkspaceLeaf {
    return this.sideLeaf(this.rightSplit, split);
  }
  async ensureSideLeaf(type: string, side: "left" | "right", options: any = {}): Promise<WorkspaceLeaf> {
    const existing = this.getLeavesOfType(type)[0];
    if (existing) {
      if (options.reveal) await this.revealLeaf(existing);
      return existing;
    }
    const leaf = side === "left" ? this.getLeftLeaf(!!options.split) : this.getRightLeaf(!!options.split);
    await leaf.setViewState({ type, state: options.state, active: options.active });
    if (options.reveal !== false) await this.revealLeaf(leaf);
    return leaf;
  }

  async revealLeaf(leaf: WorkspaceLeaf): Promise<void> {
    const root = leaf.getRoot();
    if (root instanceof WorkspaceSidedock && root.collapsed) root.expand();
    if (leaf.parent instanceof WorkspaceTabs) leaf.parent.selectTab(leaf);
    leaf.onResize();
  }

  detachLeavesOfType(type: string): void {
    for (const l of this.getLeavesOfType(type)) l.detach();
  }

  moveLeafToPopout(leaf: WorkspaceLeaf): WorkspaceWindow {
    void leaf;
    return null as any;
  }
  openPopoutLeaf(): WorkspaceLeaf {
    return this.getLeaf(true);
  }

  // ── activity ─────────────────────────────────────────────────────────────
  setActiveLeaf(leaf: WorkspaceLeaf, params?: { focus?: boolean } | boolean, focus?: boolean): void {
    const doFocus = typeof params === "boolean" ? focus : params?.focus;
    if (leaf.parent instanceof WorkspaceTabs) leaf.parent.selectTab(leaf);
    if (this.activeLeaf === leaf) {
      if (doFocus) this.focusLeaf(leaf);
      return;
    }
    // Sidebar leaves become active only if they host a file view (Obsidian semantics).
    const inSidebar = leaf.getRoot() !== this.rootSplit;
    if (inSidebar && !(leaf.view instanceof FileView)) {
      if (doFocus) this.focusLeaf(leaf);
      return;
    }
    this.activeLeaf?.containerEl.removeClass("mod-active");
    this.activeLeaf?.tabHeaderEl.removeClass("is-active-leaf");
    this.activeLeaf = leaf;
    leaf.activeTime = Date.now();
    leaf.containerEl.addClass("mod-active");
    this.activeEditor = leaf.view instanceof MarkdownView ? leaf.view : null;
    this.updateFocusClasses();
    if (doFocus) this.focusLeaf(leaf);
    this.trigger("active-leaf-change", leaf);
    this.checkFileOpen();
    this.requestSaveLayout();
  }

  private focusLeaf(leaf: WorkspaceLeaf) {
    const v: any = leaf.view;
    if (v instanceof MarkdownView) {
      if (v.getMode() === "source") v.editor.focus();
    } else (v?.containerEl as HTMLElement)?.focus?.();
  }

  private updateFocusClasses() {
    this.rootSplit.containerEl.querySelectorAll(".workspace-tabs.mod-active").forEach((e) => e.removeClass("mod-active"));
    if (this.activeLeaf?.parent instanceof WorkspaceTabs) this.activeLeaf.parent.containerEl.addClass("mod-active");
  }

  private checkFileOpen() {
    const f = this.getActiveFile();
    if (f !== this.lastActiveFile) {
      this.lastActiveFile = f;
      this.trigger("file-open", f);
      this.app.updateStatusBar?.();
    }
    this.updateTitle();
  }

  /** @internal */ updateTitle() {
    const f = this.getActiveFile();
    const t = (f ? f.basename + " - " : "") + this.app.vault.getName() + " - Ateliet";
    document.title = t;
  }

  /** @internal */ onLeafStateChanged(leaf: WorkspaceLeaf) {
    if (leaf === this.activeLeaf) {
      this.activeEditor = leaf.view instanceof MarkdownView ? leaf.view : null;
      this.checkFileOpen();
    }
    // Like Obsidian: no layout-change while the saved layout is being restored.
    if (!this.layoutReady) return;
    this.trigger("layout-change");
    this.requestSaveLayout();
  }

  /** @internal */ onLeafDetached(leaf: WorkspaceLeaf, wasActive: boolean) {
    if (wasActive) {
      this.activeLeaf = null;
      const next = this.getMostRecentLeaf();
      if (next) this.setActiveLeaf(next, { focus: true });
      else {
        // keep at least one tab in the root
        const l = this.getLeaf(true);
        l.setViewState({ type: "empty" }).then(() => this.setActiveLeaf(l, { focus: true }));
      }
    }
    this.trigger("layout-change");
    this.requestSaveLayout();
    void leaf;
  }

  /** @internal */ onSidedockToggle() {
    document.body.toggleClass("is-left-sidedock-collapsed", this.leftSplit.collapsed);
    document.body.toggleClass("is-right-sidedock-collapsed", this.rightSplit.collapsed);
    this.rootSplit.onResize();
    this.trigger("resize");
    this.requestSaveLayout();
  }

  /** @internal */ recordFileOpen(file: TFile) {
    this.lastOpenFiles = [file.path, ...this.lastOpenFiles.filter((p) => p !== file.path)].slice(0, 20);
  }

  // ── links ────────────────────────────────────────────────────────────────
  async openLinkText(linktext: string, sourcePath: string, newLeaf?: boolean | "tab" | "split" | "window", openViewState?: any): Promise<void> {
    const { path, subpath } = parseLinktext(linktext);
    let file: TFile | null = path ? this.app.metadataCache.getFirstLinkpathDest(path, sourcePath) : this.app.vault.getAbstractFileByPath(sourcePath);
    if (!file) {
      if (!path) return;
      // Clicking an unresolved link creates the note, like Obsidian.
      const parent = this.app.fileManager.getNewFileParent(sourcePath);
      const target = (parent.isRoot() ? "" : parent.path + "/") + (path.endsWith(".md") ? path : path + ".md");
      try {
        file = await this.app.vault.create(target, "");
      } catch (e) {
        console.error(e);
        return;
      }
    }
    const leaf = this.getLeaf(newLeaf);
    await leaf.openFile(file!, { ...openViewState, eState: { ...(openViewState?.eState || {}), subpath: subpath || undefined } });
    this.setActiveLeaf(leaf, { focus: true });
  }

  // ── editor extensions ────────────────────────────────────────────────────
  registerEditorExtension(ext: any) {
    this.editorExtensions.push(ext);
    this.updateOptions();
  }
  unregisterEditorExtension(ext: any) {
    this.editorExtensions.remove(ext);
    this.updateOptions();
  }
  /** @internal */ getEditorExtensions(): any[] {
    const flat = (x: any): any[] => (Array.isArray(x) ? x.flatMap(flat) : x ? [x] : []);
    // Obsidian's editor language is a CM5 "hypermd" stream mode; extensions that
    // replace the language (e.g. Templater's overlay) would wipe out Ateliet's
    // markdown parser, so they are left out.
    return flat(this.editorExtensions).filter((e) => !replacesLanguage(e));
  }
  updateOptions(): void {
    this.iterateAllLeaves((l) => {
      if (l.view instanceof MarkdownView) l.view.reconfigure();
    });
  }

  registerHoverLinkSource(id: string, info: any) {
    this.hoverLinkSources[id] = info;
  }
  unregisterHoverLinkSource(id: string) {
    delete this.hoverLinkSources[id];
  }

  registerObsidianProtocolHandler(action: string, handler: any) {
    this.protocolHandlers[action] = handler;
  }

  // ── layout persistence ───────────────────────────────────────────────────
  getLayout(): any {
    return {
      main: this.rootSplit.serialize(),
      left: this.leftSplit.serialize(),
      right: this.rightSplit.serialize(),
      "left-ribbon": { hiddenItems: Object.fromEntries(this.leftRibbon.items.map((i) => [i.id, i.hidden])) },
      active: this.activeLeaf?.id,
      lastOpenFiles: this.lastOpenFiles,
    };
  }

  /** @internal */ async saveLayout() {
    if (!this.layoutReady) return;
    try {
      await this.app.saveWorkspaceLayout(this.getLayout());
    } catch (e) {
      console.error("save layout", e);
    }
  }

  async changeLayout(layout: any): Promise<void> {
    await this.loadLayout(layout);
  }

  /** @internal */ async loadLayout(layout: any): Promise<void> {
    const build = async (data: any, parent: WorkspaceParent): Promise<void> => {
      if (!data) return;
      if (data.type === "leaf") {
        const leaf = new WorkspaceLeaf(this.app);
        if (data.id) leaf.id = data.id;
        parent.insertChild(-1, leaf);
        const st = data.state || { type: "empty" };
        await leaf.setViewState({ type: st.type, state: st.state, pinned: data.pinned, group: data.group });
        return;
      }
      if (data.type === "tabs") {
        const tabs = new WorkspaceTabs(this.app);
        if (data.id) tabs.id = data.id;
        if (data.dimension != null) tabs.dimension = data.dimension;
        parent.insertChild(-1, tabs);
        for (const c of data.children || []) await build(c, tabs);
        tabs.currentTab = Math.min(data.currentTab || 0, Math.max(0, tabs.children.length - 1));
        tabs.updateVisibility();
        if (tabs.children.length === 0) parent.removeChild(tabs);
        return;
      }
      if (data.type === "split") {
        const split = new WorkspaceSplit(this.app, data.direction || "vertical");
        if (data.dimension != null) split.dimension = data.dimension;
        parent.insertChild(-1, split);
        for (const c of data.children || []) await build(c, split);
        return;
      }
    };
    const clear = (p: WorkspaceParent) => {
      for (const c of p.children.slice()) {
        this.iterateLeaves(c, (l) => {
          l.view?.unload();
        });
        p.removeChild(c);
      }
    };
    clear(this.rootSplit);
    clear(this.leftSplit);
    clear(this.rightSplit);
    this.hiddenRibbonItems = Object.entries(layout?.["left-ribbon"]?.hiddenItems || {}).filter(([, v]) => v).map(([k]) => k);
    for (const it of this.leftRibbon.items) {
      it.hidden = this.hiddenRibbonItems.includes(it.id);
      it.el.toggle(!it.hidden);
    }
    const fill = async (dock: WorkspaceSplit, data: any) => {
      if (!data) return;
      for (const c of data.children || []) await build(c, dock);
    };
    await fill(this.rootSplit, layout?.main);
    await fill(this.leftSplit, layout?.left);
    await fill(this.rightSplit, layout?.right);
    if (layout?.left) {
      this.leftSplit.setSize(layout.left.width || 300);
      layout.left.collapsed ? this.leftSplit.collapse() : this.leftSplit.expand();
    }
    if (layout?.right) {
      this.rightSplit.setSize(layout.right.width || 300);
      layout.right.collapsed ? this.rightSplit.collapse() : this.rightSplit.expand();
    }
    this.lastOpenFiles = layout?.lastOpenFiles || [];
    if (this.rootSplit.children.length === 0) {
      const leaf = this.getLeaf(true);
      await leaf.setViewState({ type: "empty" });
    }
    const active = (layout?.active && this.getLeafById(layout.active)) || this.getMostRecentLeaf() || this.getLeaf(false);
    if (active) this.setActiveLeaf(active, { focus: true });
  }

  private reloadPlaceholders(type: string) {
    this.iterateAllLeaves((l) => {
      if (l.view instanceof PlaceholderView && l.view.getViewType() === type) {
        const st = l.view.getState();
        l.setViewState({ type, state: st });
      }
    });
  }
}

const languageCheck = new WeakMap<object, boolean>();
let baseLanguage: any = null;
function replacesLanguage(ext: any): boolean {
  if (!ext || (typeof ext !== "object" && typeof ext !== "function")) return false;
  const known = languageCheck.get(ext);
  if (known !== undefined) return known;
  let r = false;
  try {
    baseLanguage ||= EditorState.create({ extensions: obsidianMarkdown() }).facet(language);
    r = EditorState.create({ extensions: [obsidianMarkdown(), ext] }).facet(language) !== baseLanguage;
  } catch {
    r = false;
  }
  if (r) console.warn("[ateliet] ignoring a plugin editor extension that replaces the markdown language");
  languageCheck.set(ext, r);
  return r;
}

/** Runs registered EditorSuggests after each user edit. */
class EditorSuggestManager {
  suggests: any[] = [];
  current: any = null;
  addSuggest(s: any) {
    this.suggests.push(s);
  }
  removeSuggest(s: any) {
    this.suggests.remove(s);
  }
  trigger(editor: any, file: any) {
    if (this.current && this.current.isOpen) {
      if (this.current.trigger(editor, file, false)) return;
    }
    this.current = null;
    for (const s of this.suggests) {
      if (s.trigger(editor, file, false)) {
        this.current = s;
        return;
      }
    }
  }
  close() {
    this.current?.close();
    this.current = null;
  }
}

export { Keymap };
