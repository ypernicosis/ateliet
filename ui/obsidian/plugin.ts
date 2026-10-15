// Plugin, setting tabs, the community plugin manager, commands and hotkeys.

import { Component, Events } from "./events.ts";
import { MarkdownPreviewRenderer } from "./renderer.ts";
import { Notice, Scope, normalizeModifiers } from "./ui.ts";
import { normalizePath } from "./util.ts";
import { isMac } from "../tauri.ts";

// ── Plugin ────────────────────────────────────────────────────────────────

export abstract class Plugin extends Component {
  app: any;
  manifest: any;
  /** @internal */ _ribbonIcons: HTMLElement[] = [];
  /** @internal */ _statusBarItems: HTMLElement[] = [];

  constructor(app: any, manifest: any) {
    super();
    this.app = app;
    this.manifest = manifest;
  }

  addRibbonIcon(icon: string, title: string, callback: (evt: MouseEvent) => any): HTMLElement {
    const el = this.app.workspace.leftRibbon.addRibbonItemButton(`${this.manifest.id}:${title}`, icon, title, callback);
    this._ribbonIcons.push(el);
    this.register(() => this.app.workspace.leftRibbon.removeRibbonAction(el));
    return el;
  }

  addStatusBarItem(): HTMLElement {
    const el = this.app.statusBar.createDiv({ cls: `status-bar-item plugin-${this.manifest.id.replace(/[^\w-]/g, "-")}` });
    this.app.statusBar.insertBefore(el, this.app.statusBar.firstChild);
    this.register(() => el.detach());
    return el;
  }

  addCommand(command: any): any {
    const id = `${this.manifest.id}:${command.id}`;
    // keep identity so plugins mutating their command object still work
    Object.assign(command, { id, name: `${this.manifest.name}: ${command.name}` });
    this.app.commands.addCommand(command);
    this.register(() => this.app.commands.removeCommand(id));
    return command;
  }

  removeCommand(commandId: string): void {
    this.app.commands.removeCommand(`${this.manifest.id}:${commandId}`);
  }

  addSettingTab(settingTab: PluginSettingTab): void {
    this.app.setting.addSettingTab(settingTab);
    this.register(() => this.app.setting.removeSettingTab(settingTab));
  }

  registerView(type: string, viewCreator: (leaf: any) => any): void {
    this.app.workspace.viewRegistry.registerView(type, viewCreator);
    this.register(() => {
      this.app.workspace.detachLeavesOfType(type);
      this.app.workspace.viewRegistry.unregisterView(type);
    });
  }

  registerHoverLinkSource(id: string, info: any): void {
    this.app.workspace.registerHoverLinkSource(id, info);
    this.register(() => this.app.workspace.unregisterHoverLinkSource(id));
  }

  registerExtensions(extensions: string[], viewType: string): void {
    this.app.workspace.viewRegistry.registerExtensions(extensions, viewType);
    this.register(() => this.app.workspace.viewRegistry.unregisterExtensions(extensions));
  }

  registerMarkdownPostProcessor(postProcessor: any, sortOrder?: number): any {
    MarkdownPreviewRenderer.registerPostProcessor(postProcessor, sortOrder);
    this.register(() => MarkdownPreviewRenderer.unregisterPostProcessor(postProcessor));
    this.app.rerenderPreviews?.();
    return postProcessor;
  }

  registerMarkdownCodeBlockProcessor(language: string, handler: any, sortOrder?: number): any {
    const pp = MarkdownPreviewRenderer.createCodeBlockPostProcessor(language, handler);
    if (sortOrder !== undefined) pp.sortOrder = sortOrder;
    MarkdownPreviewRenderer.registerCodeBlock(language, pp);
    this.register(() => MarkdownPreviewRenderer.unregisterCodeBlock(language));
    this.app.rerenderPreviews?.();
    return pp;
  }

  registerCodeMirror(_cb: (cm: any) => any): void {}

  registerEditorExtension(extension: any): void {
    this.app.workspace.registerEditorExtension(extension);
    this.register(() => this.app.workspace.unregisterEditorExtension(extension));
  }

  registerObsidianProtocolHandler(action: string, handler: any): void {
    this.app.workspace.registerObsidianProtocolHandler(action, handler);
    this.register(() => delete this.app.workspace.protocolHandlers[action]);
  }

  registerEditorSuggest(editorSuggest: any): void {
    this.app.workspace.editorSuggest.addSuggest(editorSuggest);
    this.register(() => this.app.workspace.editorSuggest.removeSuggest(editorSuggest));
  }

  registerBasesView(_id: string, _reg: any): boolean {
    return false;
  }

  registerCliHandler(..._args: any[]): void {}

  async loadData(): Promise<any> {
    const path = normalizePath(`${this.manifest.dir}/data.json`);
    try {
      if (!(await this.app.vault.adapter.exists(path))) return null;
      const text = await this.app.vault.adapter.read(path);
      return text.trim() ? JSON.parse(text) : null;
    } catch (e) {
      console.error(`[${this.manifest.id}] loadData`, e);
      return null;
    }
  }

  async saveData(data: any): Promise<void> {
    const path = normalizePath(`${this.manifest.dir}/data.json`);
    this.app.plugins.markSelfWrite(path);
    await this.app.vault.adapter.write(path, JSON.stringify(data, null, 2));
  }

  onExternalSettingsChange?(): any;
  onUserEnable(): void {}

  /** @internal */ async loadCSS(): Promise<void> {
    const path = normalizePath(`${this.manifest.dir}/styles.css`);
    if (!(await this.app.vault.adapter.exists(path))) return;
    const css = await this.app.vault.adapter.read(path);
    const style = createEl("style", { attr: { "data-plugin": this.manifest.id } });
    style.textContent = css;
    // Plugin CSS goes before the theme and snippets, as in Obsidian.
    document.head.insertBefore(style, document.getElementById("ateliet-theme"));
    this.register(() => style.remove());
    this.app.workspace.trigger("css-change");
  }
}

// ── setting tabs ──────────────────────────────────────────────────────────

export abstract class SettingTab {
  app: any;
  containerEl: HTMLElement;
  id = "";
  name = "";
  icon = "";
  navEl?: HTMLElement;

  constructor(app: any) {
    this.app = app;
    this.containerEl = createDiv("vertical-tab-content");
  }
  abstract display(): void;
  hide(): void {
    this.containerEl.empty();
  }
}

export abstract class PluginSettingTab extends SettingTab {
  plugin: Plugin;
  constructor(app: any, plugin: Plugin) {
    super(app);
    this.plugin = plugin;
    this.id = plugin.manifest.id;
    this.name = plugin.manifest.name;
  }
}

export abstract class SettingPage extends SettingTab {}

// ── commands ──────────────────────────────────────────────────────────────

export class Commands {
  app: any;
  commands: Record<string, any> = {};
  editorCommands: Record<string, any> = {};

  constructor(app: any) {
    this.app = app;
  }

  addCommand(cmd: any) {
    this.commands[cmd.id] = cmd;
    if (cmd.editorCallback || cmd.editorCheckCallback) this.editorCommands[cmd.id] = cmd;
    this.app.hotkeyManager?.bake();
  }
  removeCommand(id: string) {
    delete this.commands[id];
    delete this.editorCommands[id];
    this.app.hotkeyManager?.bake();
  }
  findCommand(id: string) {
    return this.commands[id];
  }
  listCommands(): any[] {
    return Object.values(this.commands).filter((c) => this.isAvailable(c));
  }

  private editorContext(): { editor: any; view: any } | null {
    const view = this.app.workspace.activeEditor;
    if (!view || !view.editor) return null;
    if (view.getMode?.() === "preview") return { editor: view.editor, view };
    return { editor: view.editor, view };
  }

  isAvailable(cmd: any): boolean {
    try {
      if (cmd.checkCallback) return !!cmd.checkCallback(true);
      if (cmd.editorCheckCallback) {
        const c = this.editorContext();
        return !!c && !!cmd.editorCheckCallback(true, c.editor, c.view);
      }
      if (cmd.editorCallback) return !!this.editorContext();
    } catch (e) {
      console.error(e);
      return false;
    }
    return true;
  }

  executeCommand(cmd: any, evt?: Event): boolean {
    if (!cmd) return false;
    try {
      if (cmd.editorCheckCallback) {
        const c = this.editorContext();
        if (!c || !cmd.editorCheckCallback(true, c.editor, c.view)) return false;
        cmd.editorCheckCallback(false, c.editor, c.view);
      } else if (cmd.editorCallback) {
        const c = this.editorContext();
        if (!c) return false;
        cmd.editorCallback(c.editor, c.view);
      } else if (cmd.checkCallback) {
        if (!cmd.checkCallback(true)) return false;
        cmd.checkCallback(false);
      } else if (cmd.callback) {
        cmd.callback(evt);
      } else return false;
    } catch (e) {
      console.error(`Command ${cmd.id} failed`, e);
      new Notice(`Command failed: ${cmd.name}`);
    }
    this.app.workspace.trigger("command-executed", cmd);
    return true;
  }

  executeCommandById(id: string, evt?: Event): boolean {
    return this.executeCommand(this.commands[id], evt);
  }
}

// ── hotkeys ───────────────────────────────────────────────────────────────

export interface Hotkey {
  modifiers: string[];
  key: string;
}

export class HotkeyManager {
  app: any;
  customKeys: Record<string, Hotkey[]> = {};
  bakedHotkeys: { id: string; modifiers: string; key: string }[] = [];

  constructor(app: any) {
    this.app = app;
  }

  getDefaultHotkeys(id: string): Hotkey[] {
    return this.app.commands.commands[id]?.hotkeys || [];
  }
  getHotkeys(id: string): Hotkey[] | undefined {
    return this.customKeys[id];
  }
  setHotkeys(id: string, keys: Hotkey[]) {
    this.customKeys[id] = keys;
    this.bake();
    this.save();
  }
  removeHotkeys(id: string) {
    delete this.customKeys[id];
    this.bake();
    this.save();
  }
  /** Effective hotkeys for a command. */
  effective(id: string): Hotkey[] {
    return this.customKeys[id] ?? this.getDefaultHotkeys(id);
  }

  bake() {
    const out: { id: string; modifiers: string; key: string }[] = [];
    for (const id in this.app.commands.commands) {
      for (const h of this.effective(id)) out.push({ id, modifiers: normalizeModifiers(h.modifiers || []), key: (h.key || "").length === 1 ? h.key.toUpperCase() : h.key });
    }
    this.bakedHotkeys = out;
  }

  /** Called on keydown (capture phase); returns true if a command ran. */
  onKeyDown(evt: KeyboardEvent, info: { modifiers: string; key: string; vkey: string }): boolean {
    for (const h of this.bakedHotkeys) {
      if (h.modifiers !== info.modifiers) continue;
      if (h.key !== info.key && h.key.toLowerCase() !== info.vkey.toLowerCase()) continue;
      if (this.app.commands.executeCommandById(h.id, evt)) return true;
    }
    return false;
  }

  printHotkeyForCommand(id: string): string {
    const h = this.effective(id)[0];
    return h ? formatHotkey(h) : "";
  }

  async load() {
    try {
      const path = `${this.app.vault.configDir}/hotkeys.json`;
      if (await this.app.vault.adapter.exists(path)) this.customKeys = JSON.parse((await this.app.vault.adapter.read(path)) || "{}");
    } catch (e) {
      console.error("hotkeys", e);
    }
    this.bake();
  }

  async save() {
    await this.app.vault.adapter.write(`${this.app.vault.configDir}/hotkeys.json`, JSON.stringify(this.customKeys, null, 2));
  }
}

export function formatHotkey(h: Hotkey): string {
  const sym: Record<string, string> = isMac ? { Mod: "⌘", Ctrl: "⌃", Meta: "⌘", Alt: "⌥", Shift: "⇧" } : { Mod: "Ctrl", Ctrl: "Ctrl", Meta: "Win", Alt: "Alt", Shift: "Shift" };
  const keys: Record<string, string> = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→", Enter: "↵", " ": "Space", Escape: "Esc", Backspace: "⌫", Delete: "Del", Tab: "Tab" };
  const mods = (h.modifiers || []).map((m) => sym[m] || m);
  const k = keys[h.key] || (h.key.length === 1 ? h.key.toUpperCase() : h.key);
  return isMac ? mods.join("") + k : [...mods, k].join(" + ");
}

// ── community plugin manager ──────────────────────────────────────────────

export class Plugins extends Events {
  app: any;
  manifests: Record<string, any> = {};
  plugins: Record<string, Plugin> = {};
  enabledPlugins = new Set<string>();
  failed: Record<string, string> = {};
  private selfWrites = new Map<string, number>();
  private requireFn: (id: string) => (name: string) => any;

  constructor(app: any, requireFn: (id: string) => (name: string) => any) {
    super();
    this.app = app;
    this.requireFn = requireFn;
  }

  get dir() {
    return `${this.app.vault.configDir}/plugins`;
  }

  async loadManifests(): Promise<void> {
    const adapter = this.app.vault.adapter;
    this.manifests = {};
    if (!(await adapter.exists(this.dir))) return;
    const { folders } = await adapter.list(this.dir);
    for (const folder of folders) {
      try {
        const mpath = `${folder}/manifest.json`;
        if (!(await adapter.exists(mpath))) continue;
        const manifest = JSON.parse(await adapter.read(mpath));
        manifest.dir = folder;
        if (manifest.id) this.manifests[manifest.id] = manifest;
      } catch (e) {
        console.warn("bad manifest in", folder, e);
      }
    }
  }

  async loadEnabledList(): Promise<string[]> {
    try {
      const p = `${this.app.vault.configDir}/community-plugins.json`;
      if (!(await this.app.vault.adapter.exists(p))) return [];
      const list = JSON.parse(await this.app.vault.adapter.read(p));
      return Array.isArray(list) ? list : [];
    } catch {
      return [];
    }
  }

  async saveEnabledList(): Promise<void> {
    const p = `${this.app.vault.configDir}/community-plugins.json`;
    // Preserve Obsidian's ordering and entries of plugins unknown to us.
    const prev = await this.loadEnabledList();
    const keep = prev.filter((id) => this.enabledPlugins.has(id) || !this.manifests[id]);
    for (const id of this.enabledPlugins) if (!keep.includes(id)) keep.push(id);
    this.markSelfWrite(p);
    await this.app.vault.adapter.write(p, JSON.stringify(keep, null, 2));
  }

  async initialize(): Promise<void> {
    await this.loadManifests();
    const enabled = await this.loadEnabledList();
    for (const id of enabled) if (this.manifests[id]) this.enabledPlugins.add(id);
    for (const id of enabled) {
      if (!this.manifests[id]) continue;
      await this.loadPlugin(id);
    }
  }

  async loadPlugin(id: string): Promise<Plugin | null> {
    const manifest = this.manifests[id];
    if (!manifest || this.plugins[id]) return this.plugins[id] || null;
    const started = performance.now();
    try {
      const code = await this.app.vault.adapter.read(`${manifest.dir}/main.js`);
      const module: any = { exports: {} };
      const fn = new Function("require", "module", "exports", `${code}\n//# sourceURL=plugin:${encodeURIComponent(id)}`);
      fn(this.requireFn(id), module, module.exports);
      const exp = module.exports;
      const PluginClass = exp.default || exp;
      if (typeof PluginClass !== "function") throw new Error("No plugin class exported");
      const instance: Plugin = new PluginClass(this.app, manifest);
      if (!(instance instanceof Plugin)) throw new Error("Exported class does not extend Plugin");
      this.plugins[id] = instance;
      await (instance as any).load();
      await instance.loadCSS();
      delete this.failed[id];
      console.info(`[ateliet] plugin ${id} loaded in ${Math.round(performance.now() - started)}ms`);
      this.trigger("plugin-loaded", id);
      return instance;
    } catch (e: any) {
      console.error(`[ateliet] failed to load plugin ${id}`, e);
      this.failed[id] = String(e?.stack || e);
      delete this.plugins[id];
      new Notice(`Failed to load plugin "${manifest.name}". See the developer console.`);
      return null;
    }
  }

  async unloadPlugin(id: string): Promise<void> {
    const p = this.plugins[id];
    if (!p) return;
    try {
      p.unload();
    } catch (e) {
      console.error(e);
    }
    delete this.plugins[id];
  }

  getPlugin(id: string): Plugin | null {
    return this.plugins[id] || null;
  }

  isEnabled(id: string): boolean {
    return this.enabledPlugins.has(id);
  }

  async enablePlugin(id: string): Promise<boolean> {
    this.enabledPlugins.add(id);
    const p = await this.loadPlugin(id);
    p?.onUserEnable();
    return !!p;
  }
  async disablePlugin(id: string): Promise<void> {
    this.enabledPlugins.delete(id);
    await this.unloadPlugin(id);
  }
  async enablePluginAndSave(id: string): Promise<boolean> {
    const ok = await this.enablePlugin(id);
    await this.saveEnabledList();
    return ok;
  }
  async disablePluginAndSave(id: string): Promise<void> {
    await this.disablePlugin(id);
    await this.saveEnabledList();
  }

  /** @internal */ markSelfWrite(path: string) {
    this.selfWrites.set(normalizePath(path), Date.now());
  }

  /** Watcher hook: an Obsidian instance (or anything) changed plugin data on disk. */
  onConfigFileChange(path: string) {
    const t = this.selfWrites.get(path);
    if (t && Date.now() - t < 2000) return;
    const m = new RegExp(`^${this.dir.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/([^/]+)/data\\.json$`).exec(path);
    if (!m) return;
    for (const id in this.plugins) {
      if (this.manifests[id]?.dir === `${this.dir}/${m[1]}`) {
        try {
          this.plugins[id].onExternalSettingsChange?.();
        } catch (e) {
          console.error(e);
        }
      }
    }
  }

  requestSaveConfig() {
    this.saveEnabledList();
  }
}

export { Scope };
