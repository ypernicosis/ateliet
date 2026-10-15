// Views: View → ItemView → FileView → EditableFileView → TextFileView →
// MarkdownView, with Obsidian's DOM structure.

import { Component } from "./events.ts";
import { setIcon } from "./icons.ts";
import { Menu, Scope } from "./ui.ts";
import { debounce, resolveSubpath } from "./util.ts";
import { TFile } from "./vault.ts";
import { Editor, editorEditorField, infoExtension } from "./editor.ts";
import { renderMarkdownInto, renderProperties } from "./renderer.ts";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, drawSelection, dropCursor, highlightSpecialChars, lineNumbers, rectangularSelection, crosshairCursor } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, foldGutter, foldKeymap, indentOnInput, indentUnit } from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { highlightSelectionMatches, search, searchKeymap, openSearchPanel } from "@codemirror/search";
import { obsidianMarkdown, obsidianSyntaxHighlighting } from "../editor/syntax.ts";
import { lineClasses, livePreview, livePreviewHost, sourceMode } from "../editor/livepreview.ts";
import { embedKind } from "./renderer.ts";

export abstract class View extends Component {
  app: any;
  icon = "";
  navigation = false;
  leaf: any;
  containerEl: HTMLElement;
  scope: Scope | null = null;

  constructor(leaf: any) {
    super();
    this.leaf = leaf;
    this.app = leaf.app;
    this.containerEl = createDiv("workspace-leaf-content");
  }

  async onOpen(): Promise<void> {}
  async onClose(): Promise<void> {}
  abstract getViewType(): string;
  getState(): Record<string, unknown> {
    return {};
  }
  async setState(_state: any, _result: any): Promise<void> {}
  getEphemeralState(): Record<string, unknown> {
    return {};
  }
  setEphemeralState(_state: any): void {}
  getIcon(): string {
    return this.icon;
  }
  onResize(): void {}
  abstract getDisplayText(): string;
  onPaneMenu(menu: Menu, _source: string): void {
    menu.addItem((i) => i.setSection("pane").setTitle("Close").setIcon("x").onClick(() => this.leaf.detach()));
  }
  onTabMenu(menu: Menu): void {
    void menu;
  }
  /** @internal */ get file(): TFile | null {
    return null;
  }
}

export abstract class ItemView extends View {
  contentEl: HTMLElement;
  headerEl: HTMLElement;
  titleContainerEl: HTMLElement;
  titleParentEl: HTMLElement;
  titleEl: HTMLElement;
  backButtonEl: HTMLElement;
  forwardButtonEl: HTMLElement;
  actionsEl: HTMLElement;
  moreOptionsButtonEl: HTMLElement;
  navButtonsEl: HTMLElement;

  constructor(leaf: any) {
    super(leaf);
    this.headerEl = this.containerEl.createDiv("view-header");
    const left = this.headerEl.createDiv("view-header-left");
    this.navButtonsEl = left.createDiv("view-header-nav-buttons");
    this.backButtonEl = this.navButtonsEl.createEl("button", { cls: "clickable-icon", attr: { "aria-label": "Navigate back" } });
    setIcon(this.backButtonEl, "arrow-left");
    this.forwardButtonEl = this.navButtonsEl.createEl("button", { cls: "clickable-icon", attr: { "aria-label": "Navigate forward" } });
    setIcon(this.forwardButtonEl, "arrow-right");
    this.backButtonEl.addEventListener("click", () => this.leaf.history?.back());
    this.forwardButtonEl.addEventListener("click", () => this.leaf.history?.forward());
    this.titleContainerEl = this.headerEl.createDiv("view-header-title-container mod-at-start");
    this.titleParentEl = this.titleContainerEl.createDiv("view-header-title-parent");
    this.titleEl = this.titleContainerEl.createDiv("view-header-title");
    this.actionsEl = this.headerEl.createDiv("view-actions");
    this.moreOptionsButtonEl = this.actionsEl.createEl("button", { cls: "clickable-icon view-action", attr: { "aria-label": "More options" } });
    setIcon(this.moreOptionsButtonEl, "more-vertical");
    this.moreOptionsButtonEl.addEventListener("click", (e) => {
      const menu = new Menu();
      this.onPaneMenu(menu, "more-options");
      this.app.workspace.trigger("leaf-menu", menu, this.leaf);
      const r = this.moreOptionsButtonEl.getBoundingClientRect();
      menu.showAtPosition({ x: r.right, y: r.bottom, left: true });
      e.stopPropagation();
    });
    this.contentEl = this.containerEl.createDiv("view-content");
  }

  addAction(icon: string, title: string, callback: (evt: MouseEvent) => any): HTMLElement {
    const el = createEl("button", { cls: "clickable-icon view-action", attr: { "aria-label": title } });
    setIcon(el, icon);
    el.addEventListener("click", callback);
    this.actionsEl.insertBefore(el, this.actionsEl.firstChild);
    return el;
  }

  /** @internal */ updateHeader() {
    this.titleEl.setText(this.getDisplayText());
    const hist = this.leaf.history;
    this.backButtonEl.toggleClass("mod-disabled", !hist || hist.backHistory.length === 0);
    this.forwardButtonEl.toggleClass("mod-disabled", !hist || hist.forwardHistory.length === 0);
  }
}

export abstract class FileView extends ItemView {
  allowNoFile = false;
  navigation = true;
  _file: TFile | null = null;

  get file(): TFile | null {
    return this._file;
  }
  set file(f: TFile | null) {
    this._file = f;
  }

  getDisplayText(): string {
    return this.file ? this.file.basename : "No file";
  }

  onload(): void {
    super.onload();
    this.registerEvent(
      this.app.vault.on("rename", (f: any) => {
        if (f === this.file) {
          this.onRename(f);
          this.leaf.updateHeader?.();
        }
      }),
    );
    this.registerEvent(
      this.app.vault.on("delete", (f: any) => {
        if (f === this.file) this.onDeleteFile();
      }),
    );
  }

  /** @internal */ onDeleteFile() {
    this.leaf.detach();
  }

  getState(): Record<string, unknown> {
    return { file: this.file?.path ?? null };
  }

  async setState(state: any, result: any): Promise<void> {
    const path = state?.file;
    if (typeof path === "string") {
      const f = this.app.vault.getAbstractFileByPath(path);
      if (f instanceof TFile) await this.loadFile(f);
    }
    void result;
  }

  async loadFile(file: TFile): Promise<void> {
    if (this.file === file) return;
    if (this.file) await this.onUnloadFile(this.file);
    this.file = file;
    this.titleParentEl.setText(file.parent && !file.parent.isRoot() ? file.parent.path.replace(/\//g, " / ") + " / " : "");
    await this.onLoadFile(file);
    this.leaf.updateHeader?.();
  }

  async onLoadFile(_file: TFile): Promise<void> {}
  async onUnloadFile(_file: TFile): Promise<void> {}
  async onRename(_file: TFile): Promise<void> {}
  canAcceptExtension(_ext: string): boolean {
    return false;
  }

  async onClose(): Promise<void> {
    if (this.file) await this.onUnloadFile(this.file);
  }

  onPaneMenu(menu: Menu, source: string): void {
    if (this.file) this.app.workspace.trigger("file-menu", menu, this.file, source, this.leaf);
    super.onPaneMenu(menu, source);
  }
}

export abstract class EditableFileView extends FileView {}

export abstract class TextFileView extends EditableFileView {
  data = "";
  requestSave: () => void;
  /** @internal */ lastSavedData: string | null = null;
  /** @internal */ dirty = false;
  /** @internal true once the file's content is in the editor. */ loaded = false;

  constructor(leaf: any) {
    super(leaf);
    this.requestSave = debounce(() => this.save(), 1000, true);
  }

  async onLoadFile(file: TFile): Promise<void> {
    this.loaded = false;
    this.dirty = false;
    const data = await this.app.vault.read(file);
    if (this.file !== file) return; // another file was opened meanwhile
    this.data = data;
    this.lastSavedData = data;
    this.setViewData(data, true);
    this.dirty = false;
    this.loaded = true;
    this.registerEvent(
      this.app.vault.on("modify", async (f: any) => {
        if (f !== this.file || this.dirty) return;
        const fresh = await this.app.vault.read(f);
        if (fresh === this.lastSavedData || fresh === this.getViewData()) return;
        this.lastSavedData = fresh;
        this.data = fresh;
        this.setViewData(fresh, false);
      }),
    );
  }

  async onUnloadFile(file: TFile): Promise<void> {
    await this.save();
    void file;
    this.loaded = false;
    this.clear();
  }

  async save(clear?: boolean): Promise<void> {
    const file = this.file;
    // Never write before the file's content is loaded: that would overwrite
    // the note with whatever the (still empty) editor holds.
    if (!file || file.deleted || !this.loaded) {
      if (clear) this.clear();
      return;
    }
    const data = this.getViewData();
    this.data = data;
    if (data !== this.lastSavedData) {
      this.lastSavedData = data;
      this.dirty = false;
      await this.app.vault.modify(file, data);
    }
    this.dirty = false;
    if (clear) this.clear();
  }

  abstract getViewData(): string;
  abstract setViewData(data: string, clear: boolean): void;
  abstract clear(): void;
}

// ── markdown ──────────────────────────────────────────────────────────────

export class MarkdownEditView extends Component {
  view: MarkdownView;
  containerEl: HTMLElement;
  cm!: EditorView;
  editorEl: HTMLElement;
  sizerEl: HTMLElement;
  inlineTitleEl: HTMLElement;
  type = "source";

  constructor(view: MarkdownView) {
    super();
    this.view = view;
    this.containerEl = createDiv("markdown-source-view cm-s-obsidian mod-cm6 node-insert-event");
    this.editorEl = this.containerEl;
    this.sizerEl = createDiv("cm-sizer");
    this.inlineTitleEl = createDiv({ cls: "inline-title", attr: { contenteditable: "true", spellcheck: "false", tabindex: "-1" } });
  }

  get editor() {
    return this.view.editor;
  }
  get file() {
    return this.view.file;
  }
  getSelection(): string {
    return this.view.editor.getSelection();
  }
  get(): string {
    return this.cm.state.doc.toString();
  }
  set(data: string, clear: boolean) {
    const cm = this.cm;
    if (clear) cm.setState(this.view.createState(data));
    else if (data !== cm.state.doc.toString()) {
      const sel = cm.state.selection.main;
      cm.dispatch({ changes: { from: 0, to: cm.state.doc.length, insert: data }, selection: { anchor: Math.min(sel.anchor, data.length) } });
    }
  }
  clear() {
    this.cm.setState(this.view.createState(""));
  }
  applyScroll(top: number) {
    this.cm.scrollDOM.scrollTop = top;
  }
  getScroll(): number {
    return this.cm.scrollDOM.scrollTop;
  }
}

export class MarkdownPreviewView extends Component {
  view: MarkdownView;
  containerEl: HTMLElement;
  sizerEl: HTMLElement;
  type = "preview";
  private renderToken = 0;
  private renderChild = new Component();

  constructor(view: MarkdownView) {
    super();
    this.view = view;
    this.containerEl = createDiv("markdown-reading-view");
    const pv = this.containerEl.createDiv("markdown-preview-view markdown-rendered node-insert-event");
    this.sizerEl = pv.createDiv("markdown-preview-sizer markdown-preview-section");
  }

  get file() {
    return this.view.file;
  }

  get renderer() {
    return this;
  }

  async rerender(full?: boolean) {
    void full;
    await this.render(this.view.data);
  }

  async render(data: string) {
    const token = ++this.renderToken;
    const scroll = this.getScroll();
    this.renderChild.unload();
    this.renderChild = new Component();
    this.renderChild.load();
    const target = createDiv();
    const pv = this.containerEl.firstElementChild as HTMLElement;
    pv.toggleClass("is-readable-line-width", !!this.view.app.vault.getConfig("readableLineLength"));
    pv.toggleClass("show-properties", this.view.app.vault.getConfig("propertiesInDocument") !== "hidden");
    // render off-DOM, then swap (avoids flicker)
    const sizer = createDiv("markdown-preview-sizer markdown-preview-section");
    if (this.view.app.vault.getConfig("showInlineTitle") && this.view.file) {
      sizer.createDiv({ cls: "mod-header mod-ui" }).createDiv({ cls: "inline-title", text: this.view.file.basename });
    }
    sizer.appendChild(target);
    await renderMarkdownInto(this.view.app, data, sizer, this.view.file?.path || "", this.renderChild, { sections: true, frontmatter: this.view.app.vault.getConfig("propertiesInDocument") !== "hidden" });
    target.remove();
    if (token !== this.renderToken) return;
    sizer.createDiv("markdown-preview-pusher");
    this.sizerEl.replaceWith(sizer);
    this.sizerEl = sizer;
    this.applyScroll(scroll);
  }

  set(data: string, _clear: boolean) {
    if (this.view.getMode() === "preview") this.render(data);
  }
  clear() {
    this.sizerEl.empty();
  }
  getScroll(): number {
    return (this.containerEl.firstElementChild as HTMLElement)?.scrollTop || 0;
  }
  applyScroll(top: number) {
    const el = this.containerEl.firstElementChild as HTMLElement;
    if (el) el.scrollTop = top;
  }
  showSearch() {
    this.view.app.internalPlugins?.getPluginById("global-search")?.instance?.openGlobalSearch?.("path:\"" + (this.view.file?.path || "") + "\" ");
  }
}

export class MarkdownView extends TextFileView {
  editor: Editor;
  editMode: MarkdownEditView;
  sourceMode: MarkdownEditView;
  previewMode: MarkdownPreviewView;
  currentMode: MarkdownEditView | MarkdownPreviewView;
  hoverPopover: any = null;
  inlineTitleEl: HTMLElement;
  modeButtonEl: HTMLElement;
  /** @internal */ livePreviewCompartment = new Compartment();
  /** @internal */ pluginCompartment = new Compartment();
  /** @internal */ optionsCompartment = new Compartment();
  private mode: "source" | "preview" = "source";
  private suppressChange = false;

  constructor(leaf: any) {
    super(leaf);
    this.icon = "file-text";
    this.containerEl.addClass("markdown-view-content");
    this.contentEl.addClass("markdown-view");
    this.editMode = new MarkdownEditView(this);
    this.sourceMode = this.editMode;
    this.previewMode = new MarkdownPreviewView(this);
    this.inlineTitleEl = this.editMode.inlineTitleEl;
    this.contentEl.appendChild(this.editMode.containerEl);
    this.contentEl.appendChild(this.previewMode.containerEl);
    this.addChild(this.previewMode);
    this.addChild(this.editMode);

    const cm = new EditorView({ state: EditorState.create({ doc: "" }), parent: this.editMode.containerEl });
    this.editMode.cm = cm;
    this.editor = new Editor(cm, this);
    cm.setState(this.createState(""));
    // Obsidian's editor DOM: .cm-scroller > .cm-sizer > (.inline-title, .cm-contentContainer > (.cm-gutters, .cm-content))
    this.editMode.sizerEl = buildSizer(cm, [this.inlineTitleEl]);
    this.inlineTitleEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        this.inlineTitleEl.blur();
        cm.focus();
      }
      if (e.key === "Escape") {
        this.inlineTitleEl.setText(this.file?.basename || "");
        this.inlineTitleEl.blur();
      }
    });
    this.inlineTitleEl.addEventListener("blur", () => this.renameFromTitle(this.inlineTitleEl.textContent || ""));
    this.titleEl.addEventListener("click", () => {
      if (!this.file) return;
      this.titleEl.setAttr("contenteditable", "true");
      this.titleEl.focus();
      document.getSelection()?.selectAllChildren(this.titleEl);
    });
    this.titleEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        this.titleEl.blur();
      }
    });
    this.titleEl.addEventListener("blur", () => {
      this.titleEl.removeAttribute("contenteditable");
      this.renameFromTitle(this.titleEl.textContent || "");
    });

    this.modeButtonEl = this.addAction("book-open", "Current view: editing. Click to read", (e) => {
      if (e.metaKey || e.ctrlKey) this.toggleSourceLivePreview();
      else this.setMode(this.mode === "preview" ? "source" : "preview");
    });
    this.currentMode = this.editMode;
    this.updateModeDom();
  }

  getViewType(): string {
    return "markdown";
  }

  getDisplayText(): string {
    return this.file ? this.file.basename : "No file";
  }

  canAcceptExtension(ext: string): boolean {
    return ext === "md";
  }

  /** @internal */ createState(doc: string): EditorState {
    const app = this.app;
    const view = this;
    const cfg = (k: string) => app.vault.getConfig(k);
    const host = liveHost(app, () => view.file?.path || "", this);
    const ext: Extension[] = [
      infoExtension(this),
      editorEditorField.init(() => this.editMode.cm),
      livePreviewHost.of(host),
      highlightSpecialChars(),
      history(),
      drawSelection(),
      dropCursor(),
      EditorState.allowMultipleSelections.of(true),
      indentOnInput(),
      obsidianMarkdown(),
      obsidianSyntaxHighlighting,
      bracketMatching(),
      rectangularSelection(),
      crosshairCursor(),
      highlightSelectionMatches(),
      search({ top: true }),
      lineClasses,
      foldGutter({
        markerDOM: (open) => {
          const d = createDiv("cm-fold-indicator" + (open ? "" : " is-collapsed"));
          setIcon(d.createDiv("collapse-indicator collapse-icon"), "chevron-down");
          return d;
        },
      }),
      this.optionsCompartment.of(this.optionExtensions()),
      this.livePreviewCompartment.of(cfg("livePreview") !== false ? livePreview() : sourceMode()),
      keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap, ...foldKeymap, indentWithTab]),
      EditorView.lineWrapping,
      EditorView.contentAttributes.of({ spellcheck: cfg("spellcheck") ? "true" : "false", autocorrect: "off", autocapitalize: "off" }),
      EditorView.updateListener.of((u) => {
        if (u.docChanged && !this.suppressChange) this.onEditorChange(u);
        if (u.selectionSet || u.docChanged) app.workspace.trigger("editor-selection-change", this.editor, this);
      }),
      EditorView.domEventHandlers({
        paste: (e) => {
          app.workspace.trigger("editor-paste", e, this.editor, this);
          if (e.defaultPrevented) return true;
          return app.handlePaste?.(e, this) || false;
        },
        drop: (e) => {
          app.workspace.trigger("editor-drop", e, this.editor, this);
          if (e.defaultPrevented) return true;
          return app.handleDrop?.(e, this) || false;
        },
        contextmenu: (e) => {
          const menu = new Menu();
          app.workspace.trigger("editor-menu", menu, this.editor, this);
          for (const [title, icon, cmd] of [["Cut", "scissors", "cut"], ["Copy", "copy", "copy"], ["Paste", "clipboard-paste", "paste"]] as const) {
            menu.addItem((i) => i.setSection("clipboard").setTitle(title).setIcon(icon).onClick(() => app.clipboardAction(cmd, this.editor)));
          }
          menu.showAtMouseEvent(e);
          return true;
        },
        mouseover: (e) => {
          const el = (e.target as HTMLElement).closest?.("[data-href]") as HTMLElement | null;
          if (el && !el.dataset.external) app.workspace.trigger("hover-link", { event: e, source: "editor", hoverParent: this, targetEl: el, linktext: el.dataset.href, sourcePath: this.file?.path || "" });
          return false;
        },
      }),
    ];
    try {
      return EditorState.create({ doc, extensions: [...ext, this.pluginCompartment.of(app.workspace.getEditorExtensions())] });
    } catch (e) {
      // A plugin's editor extension failed; keep the editor usable without them.
      console.error("[ateliet] editor extensions from plugins failed, starting without them", e);
      return EditorState.create({ doc, extensions: [...ext, this.pluginCompartment.of([])] });
    }
  }

  /** @internal */ optionExtensions(): Extension[] {
    const cfg = (k: string) => this.app.vault.getConfig(k);
    const out: Extension[] = [];
    if (cfg("showLineNumber")) out.push(lineNumbers());
    if (cfg("autoPairBrackets") !== false) out.push(closeBrackets());
    const tab = cfg("tabSize") || 4;
    out.push(EditorState.tabSize.of(tab));
    out.push(indentUnit.of(cfg("useTab") !== false ? "\t" : " ".repeat(tab)));
    return out;
  }

  /** @internal */ reconfigure() {
    const cm = this.editMode.cm;
    cm.dispatch({
      effects: [
        this.pluginCompartment.reconfigure(this.app.workspace.getEditorExtensions()),
        this.optionsCompartment.reconfigure(this.optionExtensions()),
      ],
    });
    this.updateModeDom();
  }

  private onEditorChange(u: any) {
    this.dirty = true;
    this.data = u.state.doc.toString();
    this.requestSave();
    this.app.workspace.trigger("editor-change", this.editor, this);
    const typed = u.transactions.some((tr: any) => tr.isUserEvent("input") || tr.isUserEvent("delete"));
    if (typed) this.app.workspace.editorSuggest?.trigger(this.editor, this.file);
    this.app.updateStatusBar?.();
  }

  private async renameFromTitle(name: string) {
    name = name.trim().replace(/[\\/:]/g, "-");
    const file = this.file;
    if (!file || !name || name === file.basename) {
      this.inlineTitleEl.setText(file?.basename || "");
      this.titleEl.setText(file?.basename || "");
      return;
    }
    const parent = file.parent && !file.parent.isRoot() ? file.parent.path + "/" : "";
    try {
      await this.app.fileManager.renameFile(file, `${parent}${name}.${file.extension}`);
    } catch (e: any) {
      new (this.app.Notice)(String(e?.message || e));
      this.inlineTitleEl.setText(file.basename);
    }
  }

  async onLoadFile(file: TFile): Promise<void> {
    this.inlineTitleEl.setText(file.basename);
    await super.onLoadFile(file);
    this.app.workspace.trigger("file-open", file);
  }

  async onRename(file: TFile): Promise<void> {
    this.inlineTitleEl.setText(file.basename);
  }

  getViewData(): string {
    return this.editMode.cm.state.doc.toString();
  }

  setViewData(data: string, clear: boolean): void {
    this.suppressChange = true;
    try {
      this.editMode.set(data, clear);
      if (clear) {
        // start below the frontmatter so Live Preview shows it as properties
        const m = /^---\r?\n[\s\S]*?\r?\n---[ \t]*(\r?\n|$)/.exec(data);
        if (m) this.editMode.cm.dispatch({ selection: { anchor: Math.min(m[0].length, data.length) } });
      }
    } finally {
      this.suppressChange = false;
    }
    this.data = data;
    if (this.mode === "preview") this.previewMode.render(data);
  }

  clear(): void {
    this.suppressChange = true;
    this.editMode.clear();
    this.suppressChange = false;
    this.previewMode.clear();
  }

  getMode(): "source" | "preview" {
    return this.mode;
  }

  /** @internal */ async setMode(mode: "source" | "preview" | MarkdownEditView | MarkdownPreviewView) {
    const m = typeof mode === "string" ? mode : mode === this.previewMode ? "preview" : "source";
    if (m === this.mode) return;
    if (this.dirty) await this.save();
    this.mode = m;
    this.currentMode = m === "preview" ? this.previewMode : this.editMode;
    if (m === "preview") await this.previewMode.render(this.getViewData());
    this.updateModeDom();
    this.app.workspace.trigger("layout-change");
    this.app.workspace.requestSaveLayout();
    if (m === "source") this.editMode.cm.focus();
  }

  /** @internal */ toggleSourceLivePreview() {
    const live = !this.isLivePreview();
    this.app.vault.setConfig("livePreview", live);
    this.app.workspace.iterateAllLeaves((l: any) => l.view instanceof MarkdownView && l.view.applyLivePreview(live));
  }

  /** @internal */ isLivePreview(): boolean {
    return this.app.vault.getConfig("livePreview") !== false;
  }

  /** @internal */ applyLivePreview(live: boolean) {
    this.editMode.cm.dispatch({ effects: this.livePreviewCompartment.reconfigure(live ? livePreview() : sourceMode()) });
    this.updateModeDom();
  }

  private updateModeDom() {
    const preview = this.mode === "preview";
    this.editMode.containerEl.toggle(!preview);
    this.previewMode.containerEl.toggle(preview);
    this.editMode.containerEl.toggleClass("is-live-preview", this.isLivePreview());
    this.editMode.containerEl.toggleClass("is-readable-line-width", !!this.app.vault.getConfig("readableLineLength"));
    this.editMode.containerEl.toggleClass("show-properties", this.app.vault.getConfig("propertiesInDocument") !== "hidden");
    this.inlineTitleEl.toggle(!!this.app.vault.getConfig("showInlineTitle"));
    setIcon(this.modeButtonEl, preview ? "pencil" : "book-open");
    this.modeButtonEl.setAttr("aria-label", preview ? "Current view: reading. Click to edit" : "Current view: editing. Click to read");
  }

  getState(): Record<string, unknown> {
    return { file: this.file?.path ?? null, mode: this.mode, source: !this.isLivePreview() };
  }

  async setState(state: any, result: any): Promise<void> {
    await super.setState(state, result);
    const def = this.app.vault.getConfig("defaultViewMode") === "preview" ? "preview" : "source";
    const mode = state?.mode === "preview" || state?.mode === "source" ? state.mode : def;
    await this.setMode(mode);
  }

  getEphemeralState(): Record<string, unknown> {
    const sel = this.editMode.cm.state.selection.main;
    return { cursor: { from: this.editor.offsetToPos(sel.from), to: this.editor.offsetToPos(sel.to) }, scroll: this.currentMode === this.previewMode ? this.previewMode.getScroll() : this.editMode.getScroll() };
  }

  setEphemeralState(state: any): void {
    if (!state) return;
    if (state.line !== undefined) {
      const pos = { line: state.line, ch: 0 };
      this.editor.setCursor(pos);
      this.editor.scrollIntoView({ from: pos, to: pos }, true);
    }
    if (state.cursor) {
      this.editor.setSelection(state.cursor.from, state.cursor.to);
      this.editor.scrollIntoView({ from: state.cursor.from, to: state.cursor.to }, true);
    }
    if (state.subpath && this.file) {
      const r = resolveSubpath(this.app.metadataCache.getFileCache(this.file), state.subpath);
      if (r) {
        const pos = { line: r.start.line, ch: 0 };
        if (this.mode === "preview") {
          setTimeout(() => {
            const el = this.previewMode.sizerEl.querySelector(`[data-line-start="${r.start.line}"]`) as HTMLElement | null;
            el?.scrollIntoView();
          }, 50);
        }
        this.editor.setCursor(pos);
        this.editor.scrollIntoView({ from: pos, to: pos }, true);
      }
    }
    if (typeof state.scroll === "number") {
      if (this.mode === "preview") this.previewMode.applyScroll(state.scroll);
    }
    if (state.focus && this.mode === "source") this.editor.focus();
  }

  showSearch(replace = false): void {
    if (this.mode === "preview") return this.previewMode.showSearch();
    openSearchPanel(this.editMode.cm);
    void replace;
  }

  onPaneMenu(menu: Menu, source: string): void {
    menu.addItem((i) =>
      i.setSection("view").setTitle(this.mode === "preview" ? "Editing view" : "Reading view").setIcon(this.mode === "preview" ? "pencil" : "book-open").onClick(() => this.setMode(this.mode === "preview" ? "source" : "preview")),
    );
    menu.addItem((i) =>
      i.setSection("view").setTitle(this.isLivePreview() ? "Source mode" : "Live Preview").setIcon("code-2").onClick(() => this.toggleSourceLivePreview()),
    );
    super.onPaneMenu(menu, source);
  }

  async onClose(): Promise<void> {
    await super.onClose();
    this.editMode.cm.destroy();
  }

  onResize(): void {
    this.editMode.cm.requestMeasure();
  }
}


// ── reusable embedded editor (Obsidian's internal ScrollableMarkdownEditor) ──
// Plugins such as Kanban obtain this class through
// `embedRegistry.embedByExtension.md(...).editMode` and subclass it to build
// inline editors, overriding `buildLocalExtensions()` and `onUpdate()`.

/** Re-parent CM's content into Obsidian's `.cm-sizer > .cm-contentContainer`, which themes target. */
export function buildSizer(cm: EditorView, before: HTMLElement[]): HTMLElement {
  const scroller = cm.scrollDOM as any;
  cm.contentDOM.detach();
  const sizer = scroller.createDiv("cm-sizer");
  for (const el of before) sizer.appendChild(el);
  const container = sizer.createDiv("cm-contentContainer");
  const gutters = scroller.querySelector(":scope > .cm-gutters");
  if (gutters) container.appendChild(gutters);
  container.appendChild(cm.contentDOM);
  // CM inserts gutters/layers with scrollDOM.insertBefore(x, contentDOM): redirect.
  const native = Node.prototype.insertBefore;
  scroller.insertBefore = function (node: Node, ref: Node | null) {
    if (ref === cm.contentDOM) return native.call(container, node, ref);
    if (ref && ref.parentNode !== scroller) return native.call(scroller, node, sizer);
    return native.call(scroller, node, ref);
  };
  return sizer;
}

export function liveHost(app: any, sourcePath: () => string, owner?: Component) {
  return {
    renderMarkdown: (md: string, el: HTMLElement) => {
      renderMarkdownInto(app, md, el, sourcePath(), owner || new Component(), { sections: true });
    },
    propertiesMode: () => app.vault.getConfig("propertiesInDocument"),
    renderProperties: (el: HTMLElement) => {
      if (app.vault.getConfig("propertiesInDocument") === "hidden") return true;
      renderProperties(app, el, "", sourcePath());
      return true;
    },
    openLink: (linktext: string, evt: MouseEvent) => app.workspace.openLinkText(linktext, sourcePath(), (window as any).Keymap?.isModEvent?.(evt) || false),
    resolveEmbed: (linktext: string) => {
      const f = app.metadataCache.getFirstLinkpathDest(linktext.split("#")[0], sourcePath());
      return f ? { url: app.vault.getResourcePath(f), kind: embedKind(f.extension) } : null;
    },
    isLinkResolved: (linktext: string) => {
      const p = linktext.split("#")[0];
      return !p || !!app.metadataCache.getFirstLinkpathDest(p, sourcePath());
    },
  };
}

export class ScrollableMarkdownEditor extends Component {
  app: any;
  containerEl: HTMLElement;
  editorEl: HTMLElement;
  owner: any;
  cm: EditorView;
  editor: Editor;
  scope: Scope;
  /** @internal */ private settingValue = false;

  constructor(app: any, containerEl: HTMLElement, owner: any) {
    super();
    this.app = app;
    this.containerEl = containerEl;
    this.owner = owner || {};
    this.scope = new Scope(app.scope);
    this.editorEl = containerEl.createDiv("markdown-source-view cm-s-obsidian mod-cm6 is-live-preview");
    // create the view first so state fields can reference it (editorEditorField)
    this.cm = new EditorView({ parent: this.editorEl });
    buildSizer(this.cm, []);
    this.editor = new Editor(this.cm, this);
    this.cm.setState(this.createState(""));
  }

  get file() {
    return this.owner?.file ?? null;
  }
  get activeCM() {
    return this.cm;
  }

  /** @internal */ createState(doc: string): EditorState {
    try {
      return EditorState.create({ doc, extensions: [...this.buildLocalExtensions(), this.app.workspace.getEditorExtensions()] });
    } catch (e) {
      console.error("[ateliet] embedded editor: plugin extensions failed", e);
      return EditorState.create({ doc, extensions: this.buildLocalExtensions() });
    }
  }

  buildLocalExtensions(): Extension[] {
    const app = this.app;
    return [
      infoExtension(this),
      editorEditorField.init(() => this.cm),
      livePreviewHost.of(liveHost(app, () => this.file?.path || "")),
      highlightSpecialChars(),
      history(),
      drawSelection(),
      EditorState.allowMultipleSelections.of(true),
      indentOnInput(),
      obsidianMarkdown(),
      obsidianSyntaxHighlighting,
      closeBrackets(),
      lineClasses,
      livePreview(),
      keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap, indentWithTab]),
      EditorView.lineWrapping,
      EditorView.updateListener.of((u) => {
        if (this.settingValue) return;
        const changed = u.docChanged;
        this.onUpdate(u, changed);
        if (changed && u.transactions.some((tr: any) => tr.isUserEvent("input") || tr.isUserEvent("delete"))) {
          app.workspace.editorSuggest?.trigger(this.editor, this.file);
        }
      }),
      EditorView.domEventHandlers({
        focus: () => {
          app.keymap.pushScope(this.scope);
          app.workspace.activeEditor = this;
        },
        blur: () => app.keymap.popScope(this.scope),
      }),
    ];
  }

  onUpdate(_update: any, _changed: boolean): void {}
  getMode() {
    return "source";
  }
  set(data: string, clear = false) {
    this.settingValue = true;
    try {
      if (clear) this.cm.setState(this.createState(data));
      else this.cm.dispatch({ changes: { from: 0, to: this.cm.state.doc.length, insert: data } });
    } finally {
      this.settingValue = false;
    }
  }
  get(): string {
    return this.cm.state.doc.toString();
  }
  getValue(): string {
    return this.get();
  }
  clear() {
    this.set("", true);
  }
  focus() {
    this.cm.focus();
  }
  blur() {
    this.cm.contentDOM.blur();
  }
  hasFocus() {
    return this.cm.hasFocus;
  }
  updateBottomPadding(_h?: number) {}
  onResize() {
    this.cm.requestMeasure();
  }
  destroy() {
    if (this.app.workspace.activeEditor === this) this.app.workspace.activeEditor = null;
    this.cm.destroy();
    this.editorEl.remove();
  }
  onunload() {
    this.destroy();
  }
}

/** The concrete class of `embed.editMode` (Kanban reads its grandparent). */
export class EmbedEditView extends ScrollableMarkdownEditor {}
