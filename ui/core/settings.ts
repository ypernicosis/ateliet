// Settings modal: built-in tabs + plugin tabs, Obsidian's vertical layout.

import { Modal, Setting, SuggestModal, Notice } from "../obsidian/ui.ts";
import { SettingTab } from "../obsidian/plugin.ts";
import { formatHotkey } from "../obsidian/plugin.ts";
import { setIcon } from "../obsidian/icons.ts";
import { requestUrl } from "../obsidian/util.ts";
import { eventModifiers, normalizeKey } from "../obsidian/ui.ts";
import { invoke } from "../tauri.ts";
import { BUILT_IN_PLUGINS } from "../tools/common.ts";

class FnTab extends SettingTab {
  constructor(app: any, id: string, name: string, icon: string, private fn: (el: HTMLElement, tab: FnTab) => any) {
    super(app);
    this.id = id;
    this.name = name;
    this.icon = icon;
  }
  display() {
    this.containerEl.empty();
    this.fn(this.containerEl, this);
  }
}

export class SettingsModal extends Modal {
  settingTabs: SettingTab[] = [];
  pluginTabs: SettingTab[] = [];
  activeTab: SettingTab | null = null;
  tabHeadersEl: HTMLElement;
  tabContentContainer: HTMLElement;
  private navEls = new Map<SettingTab, HTMLElement>();

  constructor(app: any) {
    super(app);
    this.modalEl.addClass("mod-settings", "mod-sidebar-layout");
    this.headerEl.remove();
    this.contentEl.addClass("vertical-tabs-container");
    this.tabHeadersEl = this.contentEl.createDiv("vertical-tab-header");
    this.tabContentContainer = this.contentEl.createDiv("vertical-tab-content-container");
    this.buildCoreTabs();
  }

  addSettingTab(tab: SettingTab) {
    this.pluginTabs.push(tab);
    if (this.containerEl.isConnected) this.renderNav();
  }
  removeSettingTab(tab: SettingTab) {
    this.pluginTabs.remove(tab);
    if (this.activeTab === tab) this.openTab(this.settingTabs[0]);
    if (this.containerEl.isConnected) this.renderNav();
  }

  onOpen() {
    this.renderNav();
    this.openTab(this.activeTab && (this.settingTabs.includes(this.activeTab) || this.pluginTabs.includes(this.activeTab)) ? this.activeTab : this.settingTabs[0]);
  }

  onClose() {
    try {
      this.activeTab?.hide();
    } catch (e) {
      console.error(e);
    }
  }

  openTabById(id: string): SettingTab | undefined {
    const t = [...this.settingTabs, ...this.pluginTabs].find((x) => x.id === id);
    if (t) this.openTab(t);
    return t;
  }

  openTab(tab: SettingTab) {
    if (!tab) return;
    if (this.activeTab && this.activeTab !== tab) {
      try {
        this.activeTab.hide();
      } catch (e) {
        console.error(e);
      }
      this.activeTab.containerEl.detach();
    }
    this.activeTab = tab;
    this.navEls.forEach((el, t) => el.toggleClass("is-active", t === tab));
    this.tabContentContainer.empty();
    this.tabContentContainer.appendChild(tab.containerEl);
    tab.containerEl.scrollTop = 0;
    try {
      tab.display();
    } catch (e) {
      console.error(e);
      tab.containerEl.createDiv({ cls: "mod-warning", text: "This settings tab failed to render: " + e });
    }
  }

  private renderNav() {
    this.tabHeadersEl.empty();
    this.navEls.clear();
    const group = (title: string, tabs: SettingTab[]) => {
      if (!tabs.length) return;
      const g = this.tabHeadersEl.createDiv("vertical-tab-header-group");
      g.createDiv({ cls: "vertical-tab-header-group-title", text: title });
      const items = g.createDiv("vertical-tab-header-group-items");
      for (const t of tabs) {
        const el = items.createDiv("vertical-tab-nav-item tappable");
        if (t.icon) setIcon(el.createDiv("vertical-tab-nav-item-icon"), t.icon);
        el.createSpan({ text: t.name });
        el.addEventListener("click", () => this.openTab(t));
        t.navEl = el;
        this.navEls.set(t, el);
      }
    };
    group("Options", this.settingTabs.filter((t) => !(t as any).isCorePluginTab));
    const core = this.settingTabs.filter((t) => (t as any).isCorePluginTab && this.app.internalPlugins.getPluginById((t as any).pluginId)?.enabled);
    group("Core plugins", core);
    group("Community plugins", this.pluginTabs.slice().sort((a, b) => a.name.localeCompare(b.name)));
    this.navEls.forEach((el, t) => el.toggleClass("is-active", t === this.activeTab));
  }

  // ── built-in tabs ────────────────────────────────────────────────────────
  private cfg(k: string) {
    return this.app.vault.getConfig(k);
  }
  private set(k: string, v: any) {
    this.app.vault.setConfig(k, v);
  }
  private toggle(el: HTMLElement, key: string, name: string, desc = "", invert = false) {
    new Setting(el).setName(name).setDesc(desc).addToggle((t) =>
      t.setValue(invert ? !this.cfg(key) : !!this.cfg(key)).onChange((v) => this.set(key, invert ? !v : v)),
    );
  }

  private buildCoreTabs() {
    const app = this.app;
    const add = (id: string, name: string, icon: string, fn: (el: HTMLElement, tab: FnTab) => any) => {
      const t = new FnTab(app, id, name, icon, fn);
      this.settingTabs.push(t);
      return t;
    };

    add("editor", "Editor", "pencil", (el) => {
      new Setting(el).setName("Default view for new tabs").addDropdown((d) => d.addOptions({ source: "Editing view", preview: "Reading view" }).setValue(this.cfg("defaultViewMode")).onChange((v) => this.set("defaultViewMode", v)));
      new Setting(el).setName("Default editing mode").addDropdown((d) => d.addOptions({ live: "Live Preview", source: "Source mode" }).setValue(this.cfg("livePreview") !== false ? "live" : "source").onChange((v) => {
        this.set("livePreview", v === "live");
        app.workspace.iterateAllLeaves((l: any) => l.view?.applyLivePreview?.(v === "live"));
      }));
      new Setting(el).setName("Properties in document").addDropdown((d) => d.addOptions({ visible: "Visible", hidden: "Hidden", source: "Source" }).setValue(this.cfg("propertiesInDocument")).onChange((v) => this.set("propertiesInDocument", v)));
      this.toggle(el, "readableLineLength", "Readable line length", "Limit maximum line length.");
      this.toggle(el, "showInlineTitle", "Show inline title", "Displays the filename as an editable title inline with the file contents.");
      this.toggle(el, "showLineNumber", "Show line numbers");
      this.toggle(el, "spellcheck", "Spellcheck");
      this.toggle(el, "autoPairBrackets", "Auto-pair brackets");
      this.toggle(el, "useTab", "Indent using tabs");
      new Setting(el).setName("Tab indent size").addSlider((s) => s.setLimits(2, 8, 1).setValue(this.cfg("tabSize")).setDynamicTooltip().onChange((v) => this.set("tabSize", v)));
    });

    add("file", "Files and links", "folder", (el) => {
      new Setting(el).setName("Default location for new notes").addDropdown((d) => d.addOptions({ root: "Vault folder", current: "Same folder as current file", folder: "In the folder specified below" }).setValue(this.cfg("newFileLocation")).onChange((v) => {
        this.set("newFileLocation", v);
        this.display();
      }));
      if (this.cfg("newFileLocation") === "folder") new Setting(el).setName("Folder to create new notes in").addText((t) => t.setValue(this.cfg("newFileFolderPath")).onChange((v) => this.set("newFileFolderPath", v)));
      new Setting(el).setName("New link format").addDropdown((d) => d.addOptions({ shortest: "Shortest path when possible", relative: "Relative path to file", absolute: "Absolute path in vault" }).setValue(this.cfg("newLinkFormat")).onChange((v) => this.set("newLinkFormat", v)));
      this.toggle(el, "useMarkdownLinks", "Use [[Wikilinks]]", "Generate [[Wikilinks]] instead of Markdown links.", true);
      this.toggle(el, "alwaysUpdateLinks", "Automatically update internal links", "Rename links when a file is renamed or moved.");
      new Setting(el).setName("Default location for new attachments").setDesc("'/' = vault root, './' = same folder as the note, './name' = subfolder, or a folder path").addText((t) => t.setValue(this.cfg("attachmentFolderPath")).onChange((v) => this.set("attachmentFolderPath", v)));
      this.toggle(el, "promptDelete", "Confirm file deletion");
      new Setting(el).setName("Deleted files").addDropdown((d) => d.addOptions({ system: "Move to system trash", local: "Move to vault trash (.trash folder)", none: "Permanently delete" }).setValue(this.cfg("trashOption")).onChange((v) => this.set("trashOption", v)));
      this.toggle(el, "showUnsupportedFiles", "Detect all file extensions", "Show files with any extension in the file explorer.");
    });

    add("appearance", "Appearance", "palette", async (el, tab) => {
      new Setting(el).setName("Base color scheme").addDropdown((d) => d.addOptions({ system: "Adapt to system", moonstone: "Light", obsidian: "Dark" }).setValue(this.cfg("theme")).onChange((v) => this.set("theme", v)));
      new Setting(el).setName("Accent color").addColorPicker((c) => c.setValue(this.cfg("accentColor") || "#8a5cf5").onChange((v) => this.set("accentColor", v))).addExtraButton((b) => b.setIcon("rotate-ccw").setTooltip("Restore default").onClick(() => {
        this.set("accentColor", "");
        tab.display();
      }));
      await app.customCss.readThemes();
      const themes = Object.keys(app.customCss.themes).sort();
      new Setting(el).setName("Themes").setDesc("Themes installed in .obsidian/themes — shared with Obsidian.").addDropdown((d) => {
        d.addOption("", "Default");
        for (const t of themes) d.addOption(t, t);
        d.setValue(this.cfg("cssTheme") || "").onChange((v) => app.customCss.setTheme(v));
      }).addButton((b) => b.setButtonText("Browse").onClick(() => new ThemeBrowser(app, () => tab.display()).open()));
      new Setting(el).setName("Font size").addSlider((s) => s.setLimits(10, 30, 1).setValue(this.cfg("baseFontSize") || 16).setDynamicTooltip().onChange((v) => this.set("baseFontSize", v)));
      new Setting(el).setName("Text alignment").setDesc("How paragraphs and list items line up in the editor and in reading view. Ateliet only; Obsidian ignores it.").addDropdown((d) => d.addOptions({ justify: "Justified", start: "Left", center: "Centered", end: "Right" }).setValue(this.cfg("atelietTextAlign") || "justify").onChange((v) => this.set("atelietTextAlign", v)));
      new Setting(el).setName("Interface font").addText((t) => t.setPlaceholder("System default").setValue(this.cfg("interfaceFontFamily")).onChange((v) => this.set("interfaceFontFamily", v)));
      new Setting(el).setName("Text font").addText((t) => t.setPlaceholder("System default").setValue(this.cfg("textFontFamily")).onChange((v) => this.set("textFontFamily", v)));
      new Setting(el).setName("Monospace font").addText((t) => t.setPlaceholder("System default").setValue(this.cfg("monospaceFontFamily")).onChange((v) => this.set("monospaceFontFamily", v)));
      new Setting(el).setName("Show ribbon").addToggle((t) => t.setValue(this.cfg("showRibbon") !== false).onChange((v) => this.set("showRibbon", v)));
      new Setting(el).setName("CSS snippets").setHeading().addExtraButton((b) => b.setIcon("refresh-cw").setTooltip("Reload snippets").onClick(async () => {
        await app.customCss.reload();
        tab.display();
      })).addExtraButton((b) => b.setIcon("folder-open").setTooltip("Open snippets folder").onClick(async () => {
        await app.vault.adapter.mkdir(app.customCss.getSnippetsFolder());
        app.openWithDefaultApp(app.customCss.getSnippetsFolder());
      }));
      await app.customCss.readSnippets();
      for (const s of app.customCss.snippets) {
        new Setting(el).setName(s).setDesc(`${app.customCss.getSnippetPath(s)}`).addToggle((t) => t.setValue(app.customCss.enabledSnippets.has(s)).onChange((v) => app.customCss.setCssEnabledStatus(s, v)));
      }
    });

    add("hotkeys", "Hotkeys", "keyboard", (el) => {
      let filter = "";
      new Setting(el).addSearch((s) => s.setPlaceholder("Filter...").onChange((v) => {
        filter = v.toLowerCase();
        render();
      }));
      const list = el.createDiv("hotkey-list-container");
      const render = () => {
        list.empty();
        const cmds = Object.values(app.commands.commands).filter((c: any) => c.name.toLowerCase().includes(filter)).sort((a: any, b: any) => a.name.localeCompare(b.name));
        for (const cmd of cmds as any[]) {
          const s = new Setting(list).setName(cmd.name).setClass("hotkey-setting-row");
          const keys = app.hotkeyManager.effective(cmd.id);
          const ctl = s.controlEl.createDiv("setting-command-hotkeys");
          for (const h of keys) {
            const k = ctl.createSpan({ cls: "setting-hotkey", text: formatHotkey(h) });
            const x = k.createSpan("setting-hotkey-icon setting-delete-hotkey");
            setIcon(x, "x");
            x.addEventListener("click", () => {
              app.hotkeyManager.setHotkeys(cmd.id, keys.filter((y: any) => y !== h));
              render();
            });
          }
          if (app.hotkeyManager.getHotkeys(cmd.id)) {
            s.addExtraButton((b) => b.setIcon("rotate-ccw").setTooltip("Restore default").onClick(() => {
              app.hotkeyManager.removeHotkeys(cmd.id);
              render();
            }));
          }
          s.addExtraButton((b) => b.setIcon("plus-circle").setTooltip("Customize this command").onClick(() => {
            const rec = ctl.createSpan({ cls: "setting-hotkey mod-active", text: "Press hotkey..." });
            const onKey = (e: KeyboardEvent) => {
              if (["Shift", "Control", "Alt", "Meta"].includes(e.key)) return;
              e.preventDefault();
              e.stopPropagation();
              window.removeEventListener("keydown", onKey, true);
              rec.remove();
              if (e.key === "Escape") return;
              const mods = eventModifiers(e).split(",").filter(Boolean);
              app.hotkeyManager.setHotkeys(cmd.id, [...keys, { modifiers: mods, key: normalizeKey(e) }]);
              render();
            };
            window.addEventListener("keydown", onKey, true);
          }));
        }
      };
      render();
    });

    add("plugins", "Core plugins", "box", (el) => {
      for (const p of Object.values(app.internalPlugins.plugins) as any[]) {
        if (p.instance.hidden) continue;
        new Setting(el).setName(p.name).setDesc(p.instance.description || "").addToggle((t) => t.setValue(p.enabled).onChange(async (v) => {
          v ? await p.enable(true) : await p.disable(true);
          this.renderNav();
          if (!v) new Notice("Reload Ateliet to fully disable this core plugin.");
        }));
      }
    });

    add("community-plugins", "Community plugins", "puzzle", async (el, tab) => {
      const pm = app.plugins;
      new Setting(el).setName("Community plugins").setDesc("Plugins are read from .obsidian/plugins — the same ones Obsidian uses.").addButton((b) => b.setButtonText("Browse").setCta().onClick(() => new PluginBrowser(app, () => tab.display()).open()));
      new Setting(el).setName("Installed plugins").setHeading().addExtraButton((b) => b.setIcon("refresh-cw").setTooltip("Reload plugins").onClick(async () => {
        await pm.loadManifests();
        tab.display();
      })).addExtraButton((b) => b.setIcon("folder-open").setTooltip("Open plugins folder").onClick(() => app.openWithDefaultApp(pm.dir)));
      const ids = Object.keys(pm.manifests).sort((a, b) => pm.manifests[a].name.localeCompare(pm.manifests[b].name));
      for (const id of ids) {
        const m = pm.manifests[id];
        const s = new Setting(el).setName(m.name);
        const desc = createFragment((f) => {
          f.createDiv({ text: `Version ${m.version}${m.author ? " by " + m.author : ""}` });
          f.createDiv({ text: m.description || "" });
          if (pm.failed[id]) f.createDiv({ cls: "mod-warning", text: "Failed to load — see developer console." });
        });
        s.setDesc(desc);
        if (BUILT_IN_PLUGINS[id]) {
          s.descEl.createDiv({ cls: "mod-warning", text: `${BUILT_IN_PLUGINS[id]} is built into Ateliet now, so this plugin is not loaded. Its settings were carried over.` });
          continue;
        }
        if (pm.plugins[id] && pm.plugins[id] && this.pluginTabs.some((t) => t.id === id)) {
          s.addExtraButton((b) => b.setIcon("settings").setTooltip("Options").onClick(() => this.openTabById(id)));
        }
        s.addExtraButton((b) => b.setIcon("trash-2").setTooltip("Uninstall").onClick(async () => {
          if (!(await app.confirm(`Uninstall "${m.name}"?`, "This removes the plugin folder — also for Obsidian.", "Uninstall"))) return;
          await pm.disablePluginAndSave(id);
          await app.vault.adapter.rmdir(m.dir, true);
          delete pm.manifests[id];
          tab.display();
        }));
        s.addToggle((t) => t.setValue(pm.isEnabled(id)).onChange(async (v) => {
          if (v) await pm.enablePluginAndSave(id);
          else await pm.disablePluginAndSave(id);
          this.renderNav();
          tab.display();
        }));
      }
      if (!ids.length) el.createDiv({ cls: "setting-item-description", text: "No community plugins installed." });
    });

    add("about", "About", "info", async (el) => {
      const info = await invoke<any>("app_info");
      el.createEl("h2", { text: "Ateliet" });
      el.createEl("p", { text: `Version ${info.version} — a minimal, Obsidian-compatible markdown workshop built with Rust.` });
      new Setting(el).setName("Vault").setDesc(app.vault.adapter.basePath).addButton((b) => b.setButtonText("Open another vault").onClick(() => app.openVaultChooser()));
      new Setting(el).setName("Obsidian API version").setDesc((window as any).obsidianApiVersion || "1.12.7");
    });
  }

  display() {
    if (this.activeTab) this.openTab(this.activeTab);
  }

  /** core plugins can contribute their own option tabs */
  addCorePluginTab(pluginId: string, name: string, icon: string, fn: (el: HTMLElement) => any) {
    const t = new FnTab(this.app, pluginId, name, icon, fn) as any;
    t.isCorePluginTab = true;
    t.pluginId = pluginId;
    this.settingTabs.push(t);
  }
}

// ── community plugin browser ──────────────────────────────────────────────

const RELEASES = "https://raw.githubusercontent.com/obsidianmd/obsidian-releases/HEAD";

async function downloadRelease(repo: string, files: string[], required: string[]): Promise<Record<string, string>> {
  const rel = (await requestUrl({ url: `https://api.github.com/repos/${repo}/releases/latest`, headers: { Accept: "application/vnd.github+json", "User-Agent": "Ateliet" } })).json;
  const out: Record<string, string> = {};
  for (const name of files) {
    const asset = (rel.assets || []).find((a: any) => a.name === name);
    if (!asset) {
      if (required.includes(name)) throw new Error(`Release is missing ${name}`);
      continue;
    }
    out[name] = (await requestUrl({ url: asset.browser_download_url })).text;
  }
  return out;
}

class PluginBrowser extends SuggestModal<any> {
  private list: any[] | null = null;
  constructor(app: any, private done: () => void) {
    super(app);
    this.setPlaceholder("Search community plugins...");
    this.emptyStateText = "Loading plugin list…";
    requestUrl(`${RELEASES}/community-plugins.json`).json.then((l: any[]) => {
      this.list = l;
      this.emptyStateText = "No plugins found.";
      this.onInput();
    }).catch((e: any) => {
      this.emptyStateText = "Could not load the plugin list: " + e;
      this.onInput();
    });
  }
  getSuggestions(q: string) {
    if (!this.list) return [];
    const s = q.toLowerCase();
    return this.list.filter((p) => !s || p.name.toLowerCase().includes(s) || p.description?.toLowerCase().includes(s) || p.author?.toLowerCase().includes(s)).slice(0, 80);
  }
  renderSuggestion(p: any, el: HTMLElement) {
    el.addClass("mod-complex");
    const c = el.createDiv("suggestion-content");
    c.createDiv({ cls: "suggestion-title", text: p.name });
    c.createDiv({ cls: "suggestion-note", text: `${p.author} — ${p.description}` });
    if (this.app.plugins.manifests[p.id]) el.createDiv("suggestion-aux").createSpan({ cls: "suggestion-flair", text: "installed" });
  }
  async onChooseSuggestion(p: any) {
    const notice = new Notice(`Installing ${p.name}…`, 0);
    try {
      const files = await downloadRelease(p.repo, ["manifest.json", "main.js", "styles.css"], ["manifest.json", "main.js"]);
      const manifest = JSON.parse(files["manifest.json"]);
      const dir = `${this.app.plugins.dir}/${manifest.id}`;
      await this.app.vault.adapter.mkdir(dir);
      for (const name in files) await this.app.vault.adapter.write(`${dir}/${name}`, files[name]);
      await this.app.plugins.loadManifests();
      notice.setMessage(`Installed ${p.name}. Enable it in the list.`);
      setTimeout(() => notice.hide(), 3000);
      this.done();
    } catch (e: any) {
      notice.setMessage(`Failed to install ${p.name}: ${e?.message || e}`);
      setTimeout(() => notice.hide(), 6000);
    }
  }
}

class ThemeBrowser extends SuggestModal<any> {
  private list: any[] | null = null;
  constructor(app: any, private done: () => void) {
    super(app);
    this.setPlaceholder("Search community themes...");
    this.emptyStateText = "Loading theme list…";
    requestUrl(`${RELEASES}/community-css-themes.json`).json.then((l: any[]) => {
      this.list = l;
      this.emptyStateText = "No themes found.";
      this.onInput();
    });
  }
  getSuggestions(q: string) {
    if (!this.list) return [];
    const s = q.toLowerCase();
    return this.list.filter((t) => !s || t.name.toLowerCase().includes(s) || t.author?.toLowerCase().includes(s)).slice(0, 80);
  }
  renderSuggestion(t: any, el: HTMLElement) {
    el.addClass("mod-complex");
    const c = el.createDiv("suggestion-content");
    c.createDiv({ cls: "suggestion-title", text: t.name });
    c.createDiv({ cls: "suggestion-note", text: `${t.author} · ${(t.modes || []).join(", ")}` });
  }
  async onChooseSuggestion(t: any) {
    const notice = new Notice(`Installing ${t.name}…`, 0);
    try {
      const files = await downloadRelease(t.repo, ["manifest.json", "theme.css"], ["theme.css"]).catch(async () => {
        // older themes ship theme.css on the default branch only
        const css = (await requestUrl(`https://raw.githubusercontent.com/${t.repo}/HEAD/theme.css`)).text;
        const manifest = (await requestUrl(`https://raw.githubusercontent.com/${t.repo}/HEAD/manifest.json`)).text;
        return { "theme.css": css, "manifest.json": manifest } as Record<string, string>;
      });
      const dir = `${this.app.customCss.getThemeFolder()}/${t.name}`;
      await this.app.vault.adapter.mkdir(dir);
      for (const name in files) await this.app.vault.adapter.write(`${dir}/${name}`, files[name]);
      await this.app.customCss.readThemes();
      this.app.customCss.setTheme(t.name);
      notice.setMessage(`Installed and enabled ${t.name}.`);
      setTimeout(() => notice.hide(), 3000);
      this.done();
    } catch (e: any) {
      notice.setMessage(`Failed to install ${t.name}: ${e?.message || e}`);
      setTimeout(() => notice.hide(), 6000);
    }
  }
}
