// The `app` object plugins receive. Wires vault, metadata, workspace,
// commands, plugins, appearance and persistence together.

import { Vault, TFile, TFolder } from "./obsidian/vault.ts";
import { MetadataCache } from "./obsidian/metadata.ts";
import { FileManager } from "./obsidian/filemanager.ts";
import { Workspace } from "./obsidian/workspace.ts";
import { MarkdownView } from "./obsidian/views.ts";
import { Commands, HotkeyManager, Plugins } from "./obsidian/plugin.ts";
import { ConfirmationModal, Keymap, Notice, Scope, eventModifiers, normalizeKey } from "./obsidian/ui.ts";
import { Events } from "./obsidian/events.ts";
import { debounce, moment, normalizePath } from "./obsidian/util.ts";
import { invoke, isMac } from "./tauri.ts";

const APPEARANCE_KEYS = new Set([
  "accentColor", "baseFontSize", "baseFontSizeAction", "cssTheme", "enabledCssSnippets", "interfaceFontFamily",
  "monospaceFontFamily", "nativeMenus", "showRibbon", "showViewHeader", "textFontFamily", "theme", "translucency",
]);

export class App extends Events {
  vault: Vault;
  metadataCache: MetadataCache;
  fileManager: FileManager;
  workspace: Workspace;
  scope: Scope;
  keymap: Keymap;
  commands: Commands;
  hotkeyManager: HotkeyManager;
  plugins: Plugins;
  internalPlugins: InternalPlugins;
  setting: any;
  customCss: CustomCss;
  statusBar: HTMLElement;
  appId: string;
  isMobile = false;
  lastEvent: UserEvent | null = null;
  dom: { appContainerEl: HTMLElement; workspaceEl: HTMLElement; statusBarEl: HTMLElement };
  Notice = Notice;
  viewRegistry: any;
  embedRegistry = { embedByExtension: {} as Record<string, any>, registerExtension(ext: string, cb: any) { this.embedByExtension[ext] = cb; }, unregisterExtension(ext: string) { delete this.embedByExtension[ext]; }, isExtensionRegistered(ext: string) { return ext in this.embedByExtension; } };
  /** @internal */ changedConfigKeys = new Set<string>();
  /** @internal */ statusItems: Record<string, HTMLElement> = {};

  constructor(vaultPath: string, requireFn: (id: string) => (name: string) => any) {
    super();
    this.appId = hash(vaultPath);
    this.vault = new Vault(vaultPath);
    this.vault.app = this;
    this.metadataCache = new MetadataCache(this, this.vault);
    this.fileManager = new FileManager(this);
    this.scope = new Scope();
    this.keymap = new Keymap(this.scope);
    this.workspace = new Workspace(this);
    this.viewRegistry = this.workspace.viewRegistry;
    this.commands = new Commands(this);
    this.hotkeyManager = new HotkeyManager(this);
    this.plugins = new Plugins(this, requireFn);
    this.internalPlugins = new InternalPlugins(this);
    this.customCss = new CustomCss(this);
    const container = document.body.createDiv("app-container");
    container.appendChild(this.workspace.containerEl);
    this.statusBar = container.createDiv("status-bar");
    this.dom = { appContainerEl: container, workspaceEl: this.workspace.containerEl, statusBarEl: this.statusBar };
    this.installGlobalHandlers();
  }

  // ── configuration (.obsidian/app.json + appearance.json) ─────────────────
  private configPath(name: string) {
    return `${this.vault.configDir}/${name}`;
  }

  async readJson(name: string, fallback: any = {}): Promise<any> {
    try {
      const p = this.configPath(name);
      if (!(await this.vault.adapter.exists(p))) return fallback;
      const t = await this.vault.adapter.read(p);
      return t.trim() ? JSON.parse(t) : fallback;
    } catch (e) {
      console.warn("config", name, e);
      return fallback;
    }
  }

  async writeJson(name: string, data: any) {
    const p = this.configPath(name);
    this.plugins.markSelfWrite(p);
    await this.vault.adapter.write(p, JSON.stringify(data, null, 2));
  }

  async loadConfig() {
    const app = await this.readJson("app.json");
    const appearance = await this.readJson("appearance.json");
    this.vault.config = { ...app, ...appearance };
  }

  /** Merge-write only the keys we changed, so Obsidian-only keys survive. */
  saveAppConfig = debounce(async () => {
    const keys = Array.from(this.changedConfigKeys);
    this.changedConfigKeys.clear();
    if (!keys.length) return;
    const app = await this.readJson("app.json");
    const appearance = await this.readJson("appearance.json");
    let appDirty = false, appearanceDirty = false;
    for (const k of keys) {
      const target = APPEARANCE_KEYS.has(k) ? appearance : app;
      target[k] = this.vault.config[k];
      if (target === app) appDirty = true;
      else appearanceDirty = true;
    }
    if (appDirty) await this.writeJson("app.json", app);
    if (appearanceDirty) await this.writeJson("appearance.json", appearance);
  }, 300, true);

  /** Called by vault.setConfig. */
  onConfigChanged(key: string) {
    this.changedConfigKeys.add(key);
    if (APPEARANCE_KEYS.has(key)) this.customCss.applyAppearance();
    this.workspace.updateOptions();
    if (["readableLineLength", "showInlineTitle", "propertiesInDocument"].includes(key)) this.rerenderPreviews();
    document.body.toggleClass("show-inline-title", !!this.vault.getConfig("showInlineTitle"));
    document.body.toggleClass("show-view-header", this.vault.getConfig("showViewHeader") !== false);
  }

  // ── workspace layout ─────────────────────────────────────────────────────
  /** Where the layout on screen is saved (writing and image mode each have one). */
  layoutFile = "ateliet/workspace.json";
  /** Layout for a mode that has none saved yet. */
  layoutFallback: (() => any) | null = null;
  /** Lets the image library handle pictures dropped into a note. */
  imageDropHandler: ((e: DragEvent, view: MarkdownView) => boolean) | null = null;

  async loadWorkspaceLayout(): Promise<any> {
    if (this.layoutFile !== "ateliet/workspace.json") return (await this.readJson(this.layoutFile, null)) || this.layoutFallback?.() || null;
    // Ateliet keeps its own layout so the two apps don't fight over
    // workspace.json; on first run, start from Obsidian's last session.
    const own = await this.readJson("ateliet/workspace.json", null);
    if (own) return own;
    const obsidian = await this.readJson("workspace.json", null);
    if (obsidian) return obsidian;
    return null;
  }

  async saveWorkspaceLayout(layout: any) {
    await this.writeJson(this.layoutFile, layout);
  }

  // ── shell helpers ────────────────────────────────────────────────────────
  openExternal(url: string) {
    invoke("open_external", { url });
  }
  openWithDefaultApp(path: string) {
    invoke("open_external", { url: this.vault.adapter.getFullPath(path) });
  }
  showInFolder(path: string) {
    invoke("show_in_folder", { path: this.vault.adapter.getFullPath(path) });
  }
  async confirm(title: string, message = "", cta = "Delete"): Promise<boolean> {
    const m = new ConfirmationModal(this, title, message, cta);
    m.open();
    return m.result;
  }
  loadLocalStorage(key: string): any {
    try {
      const v = localStorage.getItem(`${this.appId}-${key}`);
      return v === null ? null : JSON.parse(v);
    } catch {
      return null;
    }
  }
  saveLocalStorage(key: string, data: any) {
    const k = `${this.appId}-${key}`;
    if (data == null) localStorage.removeItem(k);
    else localStorage.setItem(k, JSON.stringify(data));
  }
  getTheme(): string {
    return document.body.hasClass("theme-dark") ? "obsidian" : "moonstone";
  }
  changeTheme(theme: string) {
    this.vault.setConfig("theme", theme);
  }
  getAccentColor(): string {
    return this.vault.getConfig("accentColor") || "";
  }
  openVaultChooser() {
    invoke("open_launcher");
  }
  getAppTitle(): string {
    return document.title;
  }
  emulateMobile(_on: boolean) {}
  async clipboardAction(cmd: "cut" | "copy" | "paste", editor: any) {
    if (cmd === "paste") {
      const t = await navigator.clipboard.readText();
      editor.replaceSelection(t);
      return;
    }
    const text = editor.getSelection();
    await navigator.clipboard.writeText(text);
    if (cmd === "cut") editor.replaceSelection("");
  }

  rerenderPreviews() {
    this.workspace?.iterateAllLeaves((l: any) => {
      if (l.view instanceof MarkdownView && l.view.getMode() === "preview") l.view.previewMode.rerender(true);
    });
  }

  // ── attachments ──────────────────────────────────────────────────────────
  async saveAttachment(name: string, ext: string, data: ArrayBuffer, sourcePath: string): Promise<TFile> {
    const path = await this.fileManager.getAvailablePathForAttachment(`${name}.${ext}`, sourcePath);
    return this.vault.createBinary(path, data);
  }

  async insertFiles(files: File[], view: MarkdownView) {
    const src = view.file?.path || "";
    const links: string[] = [];
    for (const f of files) {
      const dot = f.name.lastIndexOf(".");
      let base = dot > 0 ? f.name.slice(0, dot) : f.name;
      let ext = dot > 0 ? f.name.slice(dot + 1) : (f.type.split("/")[1] || "bin");
      if (/^image\.(png|jpe?g|gif)$/i.test(f.name) || !base) base = "Pasted image " + moment().format("YYYYMMDDHHmmss");
      if (ext === "jpeg") ext = "jpg";
      const file = await this.saveAttachment(base, ext, await f.arrayBuffer(), src);
      const link = this.fileManager.generateMarkdownLink(file, src);
      links.push(link.startsWith("!") || file.extension === "md" ? link : "!" + link);
    }
    view.editor.replaceSelection(links.join("\n"));
  }

  handlePaste(e: ClipboardEvent, view: MarkdownView): boolean {
    const files = Array.from(e.clipboardData?.files || []);
    if (!files.length) return false;
    e.preventDefault();
    this.insertFiles(files, view);
    return true;
  }

  handleDrop(e: DragEvent, view: MarkdownView): boolean {
    if (this.imageDropHandler?.(e, view)) return true;
    const files = Array.from(e.dataTransfer?.files || []);
    const internal = e.dataTransfer?.getData("text/x-ateliet-path");
    if (internal) {
      e.preventDefault();
      const f = this.vault.getAbstractFileByPath(internal);
      if (f instanceof TFile) {
        const pos = view.editMode.cm.posAtCoords({ x: e.clientX, y: e.clientY });
        let link = this.fileManager.generateMarkdownLink(f, view.file?.path || "");
        // Pictures are embedded, like Obsidian does when dragging them in.
        if (/^(png|jpe?g|gif|webp|bmp|svg|avif)$/i.test(f.extension) && !link.startsWith("!")) link = "!" + link;
        view.editMode.cm.dispatch({ changes: { from: pos ?? view.editMode.cm.state.selection.main.head, insert: link } });
      }
      return true;
    }
    if (!files.length) return false;
    e.preventDefault();
    const pos = view.editMode.cm.posAtCoords({ x: e.clientX, y: e.clientY });
    if (pos != null) view.editMode.cm.dispatch({ selection: { anchor: pos } });
    this.insertFiles(files, view);
    return true;
  }

  // ── status bar ───────────────────────────────────────────────────────────
  updateStatusBar = debounce(() => this.trigger("status-bar-update"), 150, true);

  // ── global input handling ────────────────────────────────────────────────
  private installGlobalHandlers() {
    document.addEventListener(
      "keydown",
      (e) => {
        this.lastEvent = e;
        if (e.isComposing || e.key === "Process") return;
        const info = { modifiers: eventModifiers(e), key: normalizeKey(e), vkey: e.key };
        // modal / popover scopes first
        if (this.keymap.scope !== this.scope) {
          if (this.keymap.onKeyDown(e)) {
            e.stopPropagation();
            return;
          }
        }
        if (info.modifiers || /^F\d+$/.test(info.key) || (info.key === "Escape" && false)) {
          if (this.hotkeyManager.onKeyDown(e, info)) {
            e.preventDefault();
            e.stopPropagation();
            return;
          }
        }
        if (this.keymap.scope === this.scope && this.scope.handleKey(e, info) === false) e.stopPropagation();
      },
      true,
    );
    document.addEventListener("mousedown", (e) => (this.lastEvent = e), true);

    // Links in rendered markdown (reading view, embeds, plugin views).
    document.addEventListener("click", (e) => {
      const a = (e.target as HTMLElement).closest?.("a") as HTMLAnchorElement | null;
      if (!a) return;
      if (a.closest(".cm-editor") && !a.closest(".cm-embed-block, .markdown-rendered")) return;
      if (e.defaultPrevented) {
        // handled by a plugin; still never let the webview navigate
        if (a.getAttribute("href") && !a.getAttribute("href")!.startsWith("#")) e.preventDefault();
        return;
      }
      const href = a.getAttribute("href") || "";
      if (a.hasClass("internal-link")) {
        e.preventDefault();
        const linktext = a.getAttribute("data-href") || href;
        const source = (a.closest("[data-source-path]") as HTMLElement)?.dataset.sourcePath || this.workspace.getActiveFile()?.path || "";
        this.workspace.openLinkText(decodeURIComponent(linktext), source, Keymap.isModEvent(e));
        return;
      }
      if (a.hasClass("tag")) {
        e.preventDefault();
        this.internalPlugins.getPluginById("global-search")?.instance?.openGlobalSearch("tag:" + (a.textContent || href));
        return;
      }
      if (/^(https?|mailto|obsidian|zotero|file):/i.test(href)) {
        e.preventDefault();
        if (href.startsWith("obsidian://")) this.handleObsidianUrl(href);
        else this.openExternal(href);
        return;
      }
      if (href && !href.startsWith("#") && !href.startsWith("javascript:")) e.preventDefault();
    });
    document.addEventListener("auxclick", (e) => {
      if (e.button === 1 && (e.target as HTMLElement).closest?.("a")) e.preventDefault();
    });
    document.addEventListener("mouseover", (e) => {
      const a = (e.target as HTMLElement).closest?.("a.internal-link") as HTMLElement | null;
      if (!a || a.closest(".cm-editor")) return;
      const source = (a.closest("[data-source-path]") as HTMLElement)?.dataset.sourcePath || this.workspace.getActiveFile()?.path || "";
      this.workspace.trigger("hover-link", { event: e, source: "preview", hoverParent: a.closest(".popover") ? {} : this.workspace.activeLeaf?.view || {}, targetEl: a, linktext: a.getAttribute("data-href"), sourcePath: source });
    });
    // Never let the webview navigate away on file drops outside editors.
    window.addEventListener("dragover", (e) => e.preventDefault());
    window.addEventListener("drop", (e) => e.preventDefault());
    window.addEventListener("focus", () => document.body.addClass("is-focused"));
    window.addEventListener("blur", () => document.body.removeClass("is-focused"));
  }

  handleObsidianUrl(url: string) {
    const u = new URL(url);
    const action = u.hostname || u.pathname.replace(/^\/+/, "");
    const params: any = { action };
    u.searchParams.forEach((v, k) => (params[k] = v));
    const h = this.workspace.protocolHandlers[action];
    if (h) return h(params);
    if (action === "open" && params.file) this.workspace.openLinkText(params.file, "", false);
    else if (action === "search") this.internalPlugins.getPluginById("global-search")?.instance?.openGlobalSearch(params.query || "");
    else if (action === "new" && params.name) this.vault.create(normalizePath(params.name + ".md"), params.content || "").then((f) => this.workspace.getLeaf(false).openFile(f));
  }
}

type UserEvent = MouseEvent | KeyboardEvent | TouchEvent | PointerEvent;

function hash(s: string): string {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return (h >>> 0).toString(16).padStart(8, "0") + (s.length.toString(16).padStart(8, "0"));
}

// ── core (internal) plugins registry ──────────────────────────────────────

export class InternalPlugin {
  id: string;
  name: string;
  enabled = false;
  _loaded = false;
  instance: any;
  app: App;
  constructor(app: App, id: string, name: string, instance: any) {
    this.app = app;
    this.id = id;
    this.name = name;
    this.instance = instance;
    instance.plugin = this;
  }
  async enable(save = false) {
    if (this.enabled) return;
    this.enabled = true;
    this._loaded = true;
    await this.instance.onEnable?.(this.app, this);
    if (save) this.app.internalPlugins.saveConfig();
  }
  async disable(save = false) {
    if (!this.enabled) return;
    this.enabled = false;
    this._loaded = false;
    this.instance.onDisable?.(this.app, this);
    if (save) this.app.internalPlugins.saveConfig();
  }
}

export class InternalPlugins extends Events {
  app: App;
  plugins: Record<string, InternalPlugin> = {};
  config: Record<string, boolean> = {};

  constructor(app: App) {
    super();
    this.app = app;
  }

  register(id: string, name: string, instance: any, defaultOn = true) {
    this.plugins[id] = new InternalPlugin(this.app, id, name, instance);
    if (!(id in this.config)) this.config[id] = defaultOn;
  }

  getPluginById(id: string): InternalPlugin | null {
    return this.plugins[id] || null;
  }
  getEnabledPluginById(id: string): any {
    const p = this.plugins[id];
    return p && p.enabled ? p.instance : null;
  }
  getEnabledPlugins(): InternalPlugin[] {
    return Object.values(this.plugins).filter((p) => p.enabled);
  }

  async loadConfig() {
    const raw = await this.app.readJson("core-plugins.json", null);
    if (Array.isArray(raw)) {
      for (const id in this.config) this.config[id] = false;
      for (const id of raw) this.config[id] = true;
    } else if (raw && typeof raw === "object") {
      Object.assign(this.config, raw);
    }
  }

  async enableAll() {
    for (const id in this.plugins) if (this.config[id] !== false) await this.plugins[id].enable();
  }

  async saveConfig() {
    const raw = await this.app.readJson("core-plugins.json", null);
    const out: Record<string, boolean> = Array.isArray(raw) ? Object.fromEntries(raw.map((x: string) => [x, true])) : raw || {};
    for (const id in this.plugins) out[id] = this.plugins[id].enabled;
    await this.app.writeJson("core-plugins.json", out);
  }
}

// ── appearance: themes, snippets, CSS variables ──────────────────────────

export class CustomCss extends Events {
  app: App;
  theme = "";
  themes: Record<string, any> = {};
  snippets: string[] = [];
  enabledSnippets = new Set<string>();
  styleEl: HTMLStyleElement;
  extraStyleEls: HTMLStyleElement[] = [];
  private snippetEls: Record<string, HTMLStyleElement> = {};
  /** @internal */ csscache = new Map<string, string>();

  constructor(app: App) {
    super();
    this.app = app;
    document.head.createEl("meta", { attr: { name: "ateliet-plugin-styles" } });
    this.styleEl = document.head.createEl("style", { attr: { id: "ateliet-theme" } });
  }

  get configDir() {
    return this.app.vault.configDir;
  }
  getThemeFolder() {
    return `${this.configDir}/themes`;
  }
  getSnippetsFolder() {
    return `${this.configDir}/snippets`;
  }
  getSnippetPath(name?: string) {
    return name ? `${this.getSnippetsFolder()}/${name}.css` : this.getSnippetsFolder();
  }
  getThemePath(name: string) {
    return `${this.getThemeFolder()}/${name}/theme.css`;
  }

  async readThemes() {
    const a = this.app.vault.adapter;
    this.themes = {};
    if (!(await a.exists(this.getThemeFolder()))) return;
    const list = await a.list(this.getThemeFolder());
    for (const folder of list.folders) {
      const name = folder.split("/").pop()!;
      let manifest: any = { name };
      try {
        if (await a.exists(`${folder}/manifest.json`)) manifest = JSON.parse(await a.read(`${folder}/manifest.json`));
      } catch { /* ignore */ }
      this.themes[manifest.name || name] = { ...manifest, dir: folder };
    }
    for (const f of list.files) if (f.endsWith(".css")) this.themes[f.split("/").pop()!.slice(0, -4)] = { name: f.split("/").pop()!.slice(0, -4), legacy: f };
  }

  async readSnippets() {
    const a = this.app.vault.adapter;
    this.snippets = [];
    if (!(await a.exists(this.getSnippetsFolder()))) return;
    const list = await a.list(this.getSnippetsFolder());
    this.snippets = list.files.filter((f: string) => f.endsWith(".css")).map((f: string) => f.split("/").pop()!.slice(0, -4)).sort();
  }

  async loadTheme() {
    const name = this.app.vault.getConfig("cssTheme") || "";
    this.theme = name;
    let css = "";
    if (name) {
      const t = this.themes[name];
      const path = t?.legacy || (t?.dir ? `${t.dir}/theme.css` : this.getThemePath(name));
      try {
        if (await this.app.vault.adapter.exists(path)) css = await this.app.vault.adapter.read(path);
      } catch (e) {
        console.warn("theme", e);
      }
    }
    this.styleEl.textContent = css;
    document.body.toggleClass("is-themed", !!css);
  }

  async loadSnippets() {
    const enabled: string[] = this.app.vault.getConfig("enabledCssSnippets") || [];
    this.enabledSnippets = new Set(enabled);
    for (const k in this.snippetEls) {
      this.snippetEls[k].remove();
      delete this.snippetEls[k];
    }
    for (const name of this.snippets) {
      if (!this.enabledSnippets.has(name)) continue;
      try {
        const css = await this.app.vault.adapter.read(this.getSnippetPath(name));
        const el = document.head.createEl("style", { attr: { "data-snippet": name } });
        el.textContent = css;
        this.snippetEls[name] = el;
      } catch (e) {
        console.warn("snippet", name, e);
      }
    }
    this.extraStyleEls = Object.values(this.snippetEls);
  }

  async reload() {
    await this.readThemes();
    await this.readSnippets();
    await this.loadTheme();
    await this.loadSnippets();
    this.applyAppearance();
    this.app.workspace.trigger("css-change");
  }

  setTheme(name: string) {
    this.app.vault.setConfig("cssTheme", name);
    this.loadTheme().then(() => this.app.workspace.trigger("css-change"));
  }

  setCssEnabledStatus(snippet: string, enabled: boolean) {
    const set = new Set<string>(this.app.vault.getConfig("enabledCssSnippets") || []);
    enabled ? set.add(snippet) : set.delete(snippet);
    this.app.vault.setConfig("enabledCssSnippets", Array.from(set));
    this.loadSnippets().then(() => this.app.workspace.trigger("css-change"));
  }

  applyAppearance() {
    const cfg = (k: string) => this.app.vault.getConfig(k);
    const body = document.body;
    const mode = cfg("theme");
    const dark = mode === "system" ? matchMedia("(prefers-color-scheme: dark)").matches : mode !== "moonstone";
    body.toggleClass("theme-dark", dark);
    body.toggleClass("theme-light", !dark);
    body.style.setProperty("--font-text-size", `${cfg("baseFontSize") || 16}px`);
    const fonts: [string, string][] = [["interfaceFontFamily", "--font-interface-override"], ["textFontFamily", "--font-text-override"], ["monospaceFontFamily", "--font-monospace-override"]];
    for (const [k, v] of fonts) {
      const f = cfg(k);
      if (f) body.style.setProperty(v, f.split(",").map((x: string) => (x.trim().includes(" ") && !/^["']/.test(x.trim()) ? `'${x.trim()}'` : x.trim())).join(", "));
      else body.style.removeProperty(v);
    }
    const accent = cfg("accentColor");
    if (accent && /^#[0-9a-f]{6}$/i.test(accent)) {
      const [h, s, l] = hexToHsl(accent);
      body.style.setProperty("--accent-h", String(h));
      body.style.setProperty("--accent-s", s + "%");
      body.style.setProperty("--accent-l", l + "%");
    } else {
      body.style.removeProperty("--accent-h");
      body.style.removeProperty("--accent-s");
      body.style.removeProperty("--accent-l");
    }
    body.toggleClass("is-translucent", !!cfg("translucency"));
    body.toggleClass("show-ribbon", cfg("showRibbon") !== false);
    this.app.workspace.leftRibbon.containerEl.toggle(cfg("showRibbon") !== false);
  }
}

function hexToHsl(hex: string): [number, number, number] {
  const r = parseInt(hex.slice(1, 3), 16) / 255, g = parseInt(hex.slice(3, 5), 16) / 255, b = parseInt(hex.slice(5, 7), 16) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0, s = 0;
  const l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h *= 60;
  }
  return [Math.round(h), Math.round(s * 100), Math.round(l * 100)];
}

export { TFile, TFolder, isMac };
