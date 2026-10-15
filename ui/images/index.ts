// Images: the picture side of Ateliet. Two modes share one vault:
//
// - writing mode: notes in the middle, a picture search panel in the sidebar,
//   "Insert picture" from the command palette, drag-and-drop into notes;
// - image mode: the picture library in the middle, collections on the left,
//   the note you were writing on the right.
//
// Each mode keeps its own layout (`.obsidian/ateliet/workspace.json` and
// `workspace-images.json`); switching swaps them.

import { DEFAULT_SETTINGS, ImageItem, ImageSettings, LibraryStatus, api, insertImages } from "./api.ts";
import { ImagesHost } from "./actions.ts";
import { COLLECTIONS_VIEW, ImageCollectionsView } from "./collections.ts";
import { ImageLibraryView, LIBRARY_VIEW } from "./library.ts";
import { ImageSearchView, InsertImageModal, PANEL_VIEW } from "./panel.ts";
import { PLUGIN_VIEW, SharedPluginView } from "./plugins.ts";
import { PinterestModal, describeJob, renderFollowed } from "./pinterest.ts";
import { MarkdownView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Notice, Setting } from "../obsidian/ui.ts";
import { TFile } from "../obsidian/vault.ts";
import { invoke, listen } from "../tauri.ts";

const SETTINGS_FILE = "ateliet/images.json";
export const WRITE_LAYOUT = "ateliet/workspace.json";
export const IMAGES_LAYOUT = "ateliet/workspace-images.json";
const IMAGE_EXT = /\.(jpe?g|png|gif|webp)$/i;

export class ImagesPlugin implements ImagesHost {
  app: any;
  plugin: any;
  settings: ImageSettings = { ...DEFAULT_SETTINGS };
  status: LibraryStatus | null = null;
  private switchEl: HTMLElement | null = null;
  private statusEl: HTMLElement | null = null;
  private unlisten: (() => void)[] = [];
  private pending = new Set<string>();
  private flushTimer = 0;
  private viewsRegistered = false;
  private switching = false;

  /** `defaultWriteLayout`: the layout of a vault opened for the first time. */
  constructor(private defaultWriteLayout: any) {}

  async onEnable(app: any) {
    this.app = app;
    this.settings = { ...DEFAULT_SETTINGS, ...(await app.readJson(SETTINGS_FILE, {})) };
    app.layoutFile = this.settings.mode === "images" ? IMAGES_LAYOUT : WRITE_LAYOUT;
    app.layoutFallback = () => (app.layoutFile === IMAGES_LAYOUT ? this.defaultImagesLayout() : null);
    document.body.toggleClass("mode-images", this.settings.mode === "images");
    app.imageDropHandler = (e: DragEvent, view: MarkdownView) => this.onEditorDrop(e, view);

    if (!this.viewsRegistered) {
      const r = app.workspace.viewRegistry;
      r.registerView(LIBRARY_VIEW, (leaf: any) => new ImageLibraryView(leaf, this));
      r.registerView(COLLECTIONS_VIEW, (leaf: any) => new ImageCollectionsView(leaf, this));
      r.registerView(PANEL_VIEW, (leaf: any) => new ImageSearchView(leaf, this));
      r.registerView(PLUGIN_VIEW, (leaf: any) => new SharedPluginView(leaf));
      this.viewsRegistered = true;
    }
    this.registerCommands();
    this.buildSwitch();
    this.statusEl = app.statusBar.createDiv("status-bar-item images-status mod-clickable");
    this.statusEl.addEventListener("click", () => this.openSettings());
    this.statusEl.hide();

    this.unlisten.push(await listen("images-progress", (s: LibraryStatus) => this.onStatus(s)));
    this.unlisten.push(await listen("images-changed", () => app.workspace.trigger("images:changed")));
    this.unlisten.push(
      await listen("pinterest-progress", (job: any) => {
        app.workspace.trigger("pinterest:progress", job);
        this.onDownload(job);
      }),
    );
    this.unlisten.push(
      await listen("images-model", (m: any) => {
        if (this.status) this.onStatus({ ...this.status, model: m });
        if (m.ready && !m.downloading && !m.error) new Notice("Search by description is ready. Pictures are being analysed in the background.");
        if (m.error && !m.downloading) new Notice("Could not download the image model: " + m.error);
      }),
    );
    for (const ev of ["create", "modify", "delete"]) {
      this.listenVault(ev, (f: any) => this.touched(f?.path));
    }
    this.listenVault("rename", (f: any, old: string) => {
      this.touched(old);
      this.touched(f?.path);
    });
    app.workspace.on("file-menu", this.fileMenu);
    app.workspace.onLayoutReady(() => this.afterLayout());

    try {
      const st = await api.open(app.vault.adapter.getBasePath(), app.vault.configDir, this.settings.external, this.settings.auto, this.settings.shared);
      this.onStatus(st);
    } catch (e) {
      console.error("images", e);
    }
  }

  onDisable() {
    if (this.settings.mode === "images") this.setMode("write");
    this.unlisten.forEach((u) => u());
    this.unlisten = [];
    this.switchEl?.remove();
    this.statusEl?.remove();
    this.app.workspace.off("file-menu", this.fileMenu);
    this.app.imageDropHandler = null;
    this.app.layoutFallback = null;
    this.app.workspace.detachLeavesOfType(PANEL_VIEW);
    this.app.workspace.detachLeavesOfType(COLLECTIONS_VIEW);
    this.app.workspace.detachLeavesOfType(LIBRARY_VIEW);
  }

  private listenVault(name: string, cb: (...a: any[]) => void) {
    const ref = this.app.vault.on(name, cb);
    this.unlisten.push(() => this.app.vault.offref(ref));
  }

  // ── keeping the library in step with the vault ──────────────────────────

  private touched(path?: string) {
    if (!path) return;
    // Folders matter too: a renamed folder moves every picture inside it.
    if (!IMAGE_EXT.test(path) && path.includes(".")) return;
    this.pending.add(path);
    clearTimeout(this.flushTimer);
    this.flushTimer = window.setTimeout(() => {
      const paths = [...this.pending];
      this.pending.clear();
      api.refresh(paths).catch(() => {});
    }, 300);
  }

  private downloadText = "";

  /** Pinterest downloads in the status bar, and a notice when one ends. */
  private onDownload(job: any) {
    this.downloadText = job.state === "running" ? describeJob(job) : "";
    if ((job.state === "done" || job.state === "error" || job.state === "cancelled") && !job.automatic) new Notice(describeJob(job));
    if (this.status) this.onStatus(this.status);
  }

  private onStatus(s: LibraryStatus) {
    this.status = s;
    this.app.workspace.trigger("images:status", s);
    const el = this.statusEl;
    if (!el) return;
    if (this.downloadText) {
      el.setText(this.downloadText);
      el.show();
    } else if (s.model.downloading) {
      const pct = s.model.total ? Math.round((s.model.received / s.model.total) * 100) : 0;
      el.setText(`Downloading image model ${pct}%`);
      el.show();
    } else if (s.working && s.total) {
      el.setText(`Analysing pictures ${s.indexed.toLocaleString()}/${s.total.toLocaleString()}`);
      el.show();
    } else {
      el.hide();
    }
  }

  async saveSettings() {
    await this.app.writeJson(SETTINGS_FILE, this.settings);
  }

  async updateSettings(patch: Partial<ImageSettings>) {
    const before = this.settings;
    this.settings = { ...this.settings, ...patch };
    await this.saveSettings();
    if (JSON.stringify(before.external) !== JSON.stringify(this.settings.external) || before.auto !== this.settings.auto || before.shared !== this.settings.shared) {
      await api.configure(this.settings.external, this.settings.auto, this.settings.shared).catch(() => {});
    }
    this.app.workspace.trigger("images:settings");
    if (patch.tile || patch.showNames !== undefined) this.app.workspace.getLeavesOfType(LIBRARY_VIEW).forEach((l: any) => l.view.reload?.());
  }

  async addExternalFolder() {
    const path: string | null = await invoke("pick_folder");
    if (!path) return;
    const base = this.app.vault.adapter.getBasePath();
    if (path === base || path.startsWith(base + "/") || path.startsWith(base + "\\")) {
      new Notice("That folder is already part of the vault.");
      return;
    }
    if (this.settings.external.includes(path)) return;
    await this.updateSettings({ external: [...this.settings.external, path] });
    new Notice("Folder added. Its pictures stay where they are; they are copied into the vault only when you use them in a note.");
  }

  async removeExternalFolder(path: string) {
    await this.updateSettings({ external: this.settings.external.filter((p) => p !== path) });
  }

  // ── the two modes ───────────────────────────────────────────────────────

  isImageMode() {
    return this.settings.mode === "images";
  }

  private defaultImagesLayout() {
    const last = this.app.workspace.getActiveFile?.()?.path || this.app.workspace.getLastOpenFiles?.()[0];
    const lastIsNote = last && last.endsWith(".md") && this.app.vault.getAbstractFileByPath(last);
    return {
      main: { type: "split", direction: "vertical", children: [{ type: "tabs", children: [{ type: "leaf", pinned: true, state: { type: LIBRARY_VIEW, state: {} } }] }] },
      left: {
        type: "split",
        direction: "horizontal",
        width: 270,
        children: [{ type: "tabs", children: [{ type: "leaf", state: { type: COLLECTIONS_VIEW, state: {} } }, { type: "leaf", state: { type: "file-explorer", state: {} } }] }],
      },
      right: {
        type: "split",
        direction: "horizontal",
        width: 400,
        collapsed: !lastIsNote,
        children: [{ type: "tabs", children: [lastIsNote ? { type: "leaf", state: { type: "markdown", state: { file: last, mode: "source" } } } : { type: "leaf", state: { type: "empty", state: {} } }] }],
      },
    };
  }

  async setMode(mode: "write" | "images") {
    if (mode === this.settings.mode || this.switching) return;
    this.switching = true;
    const app = this.app;
    const ws = app.workspace;
    try {
      // Unsaved edits first, then the layout of the mode being left.
      const saves: Promise<any>[] = [];
      ws.iterateAllLeaves((l: any) => {
        if (l.view instanceof MarkdownView) saves.push(l.view.save());
      });
      await Promise.all(saves);
      await ws.saveLayout();
      const fromNote = ws.getActiveFile?.();
      this.settings.mode = mode;
      await this.saveSettings();
      app.layoutFile = mode === "images" ? IMAGES_LAYOUT : WRITE_LAYOUT;
      const layout = (await app.loadWorkspaceLayout()) || this.defaultWriteLayout;
      await ws.loadLayout(layout);
      document.body.toggleClass("mode-images", mode === "images");
      // Image mode: the note you were writing stays at hand on the right.
      if (mode === "images" && fromNote?.extension === "md") {
        const open = ws.getLeavesOfType("markdown").some((l: any) => l.view.file?.path === fromNote.path);
        if (!open) await this.openNote(fromNote, false, false);
      }
      if (mode === "write") this.ensurePanel();
      this.updateSwitch();
      ws.trigger("layout-change");
      ws.requestSaveLayout();
    } finally {
      this.switching = false;
    }
  }

  private buildSwitch() {
    const ribbon = this.app.workspace.leftRibbon;
    const sw = (this.switchEl = createDiv("ateliet-mode-switch"));
    const button = (mode: "write" | "images", icon: string, label: string) => {
      const b = sw.createDiv({ cls: "clickable-icon side-dock-ribbon-action", attr: { "aria-label": label, "data-tooltip-position": "right", "data-mode": mode } });
      setIcon(b, icon);
      b.addEventListener("click", () => this.setMode(mode));
    };
    button("write", "pen-line", "Writing mode");
    button("images", "images", "Image mode");
    ribbon.actionsEl.insertBefore(sw, ribbon.actionsEl.firstChild);
    this.updateSwitch();
  }

  private updateSwitch() {
    this.switchEl?.querySelectorAll("[data-mode]").forEach((b: any) => b.toggleClass("is-active", b.dataset.mode === this.settings.mode));
  }

  private afterLayout() {
    if (this.settings.mode === "write") this.ensurePanel();
  }

  /** The picture panel joins the writing sidebar once; closing it is respected. */
  private ensurePanel() {
    if (this.settings.panelAdded) return;
    const ws = this.app.workspace;
    if (!ws.getLeavesOfType(PANEL_VIEW).length) ws.getRightLeaf(false).setViewState({ type: PANEL_VIEW, state: {} });
    this.settings.panelAdded = true;
    this.saveSettings();
  }

  // ── opening things ──────────────────────────────────────────────────────

  async openLibrary(state: any, newLeaf = false) {
    const ws = this.app.workspace;
    let leaf = newLeaf ? null : ws.getLeavesOfType(LIBRARY_VIEW).find((l: any) => l.getRoot() === ws.rootSplit) || ws.getLeavesOfType(LIBRARY_VIEW)[0];
    if (!leaf) leaf = ws.getLeaf("tab");
    const base = { search: "", folder: null, recursive: true, similarTo: null, similarName: "", collection: null };
    await leaf.setViewState({ type: LIBRARY_VIEW, state: { ...base, ...state }, active: true });
    ws.revealLeaf(leaf);
  }

  async openNote(file: TFile, newLeaf = false, focus = true) {
    const ws = this.app.workspace;
    if (this.isImageMode() && !newLeaf) {
      let leaf: any = null;
      ws.iterateLeaves(ws.rightSplit, (l: any) => {
        if (!leaf && (l.view instanceof MarkdownView || l.view?.getViewType() === "empty")) leaf = l;
      });
      leaf ||= ws.getRightLeaf(false);
      await leaf.openFile(file);
      ws.rightSplit.expand();
      ws.revealLeaf(leaf);
      if (focus) ws.setActiveLeaf(leaf, { focus: true });
      return;
    }
    await ws.getLeaf(newLeaf ? "tab" : false).openFile(file);
  }

  async openPlugin(id: string, newLeaf = false) {
    const ws = this.app.workspace;
    let leaf = newLeaf ? null : ws.getLeavesOfType(PLUGIN_VIEW).find((l: any) => l.view.plugin?.id === id);
    if (!leaf) leaf = ws.getLeaf("tab");
    await leaf.setViewState({ type: PLUGIN_VIEW, state: { id }, active: true });
    ws.revealLeaf(leaf);
  }

  openPinterest(initial: { url?: string; collection?: string } = {}) {
    new PinterestModal(this.app, this, initial).open();
  }

  openSettings() {
    this.app.setting.open();
    this.app.setting.openTabById?.("images");
  }

  // ── commands, menus, drops ──────────────────────────────────────────────

  private registerCommands() {
    const c = (cmd: any) => this.app.commands.addCommand(cmd);
    c({ id: "images:toggle-mode", name: "Switch between writing and image mode", icon: "images", hotkeys: [{ modifiers: ["Mod", "Shift"], key: "i" }], callback: () => this.setMode(this.isImageMode() ? "write" : "images") });
    c({ id: "images:writing-mode", name: "Writing mode", icon: "pen-line", callback: () => this.setMode("write") });
    c({ id: "images:image-mode", name: "Image mode", icon: "images", callback: () => this.setMode("images") });
    c({ id: "images:open-library", name: "Open picture library in a new tab", icon: "images", callback: () => this.openLibrary({}, true) });
    c({ id: "images:show-panel", name: "Show picture panel", icon: "image", callback: () => this.app.workspace.ensureSideLeaf(PANEL_VIEW, "right", { reveal: true }) });
    c({ id: "images:show-collections", name: "Show collections", icon: "gallery-vertical-end", callback: () => this.app.workspace.ensureSideLeaf(COLLECTIONS_VIEW, "left", { reveal: true }) });
    c({ id: "images:insert", name: "Insert picture…", icon: "image-plus", editorCallback: () => new InsertImageModal(this.app, this).open() });
    c({
      id: "images:similar-to-current",
      name: "Find pictures similar to this one",
      icon: "sparkles",
      checkCallback: (checking: boolean) => {
        const f = this.app.workspace.getActiveFile();
        if (!f || !IMAGE_EXT.test(f.path)) return false;
        if (!checking) this.openLibrary({ similarTo: f.path, similarName: f.name });
        return true;
      },
    });
    c({ id: "images:pinterest", name: "Import from Pinterest…", icon: "download", callback: () => this.openPinterest() });
    c({ id: "images:download-model", name: "Turn on search by description (downloads the image model)", icon: "download", callback: () => api.downloadModel() });
  }

  private fileMenu = (menu: any, file: any) => {
    if (file instanceof TFile && IMAGE_EXT.test(file.path)) {
      menu.addItem((i: any) => i.setTitle("Find similar pictures").setIcon("sparkles").setSection?.("view").onClick(() => this.openLibrary({ similarTo: file.path, similarName: file.name })));
      menu.addItem((i: any) => i.setTitle("Insert into note").setIcon("file-input").onClick(() => insertImages(this.app, [this.itemForFile(file)])));
    } else if (file && file.children) {
      menu.addItem((i: any) => i.setTitle("Show pictures in this folder").setIcon("images").onClick(() => this.openLibrary({ folder: file.isRoot() ? "" : file.path, recursive: true })));
    }
  };

  private itemForFile(file: TFile): ImageItem {
    const folder = file.parent && !file.parent.isRoot() ? file.parent.path : "";
    return { key: file.path, abs: this.app.vault.adapter.getFullPath(file.path), name: file.name, folder, mtime: file.stat.mtime, size: file.stat.size, width: 0, height: 0, external: false, origin: "vault", indexed: true };
  }

  /** Pictures dragged from an image view into a note. */
  private onEditorDrop(e: DragEvent, view: MarkdownView): boolean {
    const raw = e.dataTransfer?.getData("text/x-ateliet-images");
    if (!raw) return false;
    e.preventDefault();
    let items: ImageItem[] = [];
    try {
      items = JSON.parse(raw);
    } catch {
      return true;
    }
    const pos = view.editMode?.cm?.posAtCoords({ x: e.clientX, y: e.clientY });
    insertImages(this.app, items, view, pos ?? undefined);
    return true;
  }
}

// ── settings tab ──────────────────────────────────────────────────────────

export function imagesSettingsTab(app: any) {
  return (el: HTMLElement) => {
    const plugin: ImagesPlugin | null = app.internalPlugins.getEnabledPluginById("images");
    if (!plugin) {
      el.createEl("p", { text: "Turn on the Images core plugin to use the picture library." });
      return;
    }
    const render = () => {
      el.empty();
      const s = plugin.settings;
      const st = plugin.status;
      new Setting(el).setName("Search by description").setHeading();
      const model = st?.model;
      const desc = !model
        ? "…"
        : model.downloading
          ? `Downloading the image model: ${Math.round((model.received / Math.max(1, model.total)) * 100)}%`
          : model.ready
            ? `Ready. ${st!.indexed.toLocaleString()} of ${st!.total.toLocaleString()} pictures analysed${st!.failed ? `, ${st!.failed} could not be read` : ""}.`
            : `Find pictures by describing them ("a red bicycle in the rain"). Needs a one-time download of the image model (about ${Math.round(model.size / 1e6)} MB). Everything runs on this computer.`;
      const modelSetting = new Setting(el).setName("Image model").setDesc(desc);
      if (model && !model.ready && !model.downloading) modelSetting.addButton((b) => b.setButtonText("Download").setCta().onClick(() => api.downloadModel()));
      if (model?.error) el.createDiv({ cls: "setting-item-description mod-warning", text: model.error });
      new Setting(el)
        .setName("Analyse new pictures automatically")
        .setDesc("Pictures are analysed in the background, a few at a time, as they arrive.")
        .addToggle((t) => t.setValue(s.auto).onChange((v) => plugin.updateSettings({ auto: v })));
      new Setting(el)
        .setName("Analyse every picture again")
        .setDesc("Only needed if search results look wrong after the model changed.")
        .addButton((b) => b.setButtonText("Start").onClick(() => api.reindex()));

      new Setting(el).setName("Shared library (Pictogrep)").setHeading();
      const sharedDesc = s.shared === "" ? "Off: image mode shows only the vault and the folders below." : st?.shared ? `In use: ${st.shared}${st.sharedRunning ? " (Pictogrep is open)" : ""}. Pictures, collections, followed boards and plugins are the same in both apps.` : s.shared === "auto" ? "Pictogrep's folder was not found on this computer. Choose it below if it is somewhere else." : `Folder not found: ${s.shared}`;
      new Setting(el)
        .setName("Pictogrep folder")
        .setDesc(sharedDesc)
        .addDropdown((d) =>
          d
            .addOptions({ auto: "Find automatically", custom: "Choose a folder…", off: "Do not share" })
            .setValue(s.shared === "auto" ? "auto" : s.shared === "" ? "off" : "custom")
            .onChange(async (v) => {
              if (v === "auto") await plugin.updateSettings({ shared: "auto" });
              else if (v === "off") await plugin.updateSettings({ shared: "" });
              else {
                const path: string | null = await invoke("pick_folder");
                if (path) await plugin.updateSettings({ shared: path });
              }
              setTimeout(render, 800);
            }),
        );
      new Setting(el)
        .setName("Pictures added to \"All pictures\" go to")
        .setDesc("Dropping or pasting into a collection or a folder always saves there.")
        .addDropdown((d) => d.addOptions({ shared: "The shared library", vault: "The vault" }).setValue(s.importTarget).onChange((v) => plugin.updateSettings({ importTarget: v as any })));
      new Setting(el).setName("Pinterest").setHeading();
      new Setting(el).setName("Import a board").addButton((b) => b.setButtonText("Import from Pinterest…").onClick(() => plugin.openPinterest()));
      if (!st?.shared) {
        new Setting(el).setName("Vault folder for Pinterest imports").setDesc("Used when no shared library is in use.").addText((t) => t.setValue(s.pinterestFolder).onChange((v) => plugin.updateSettings({ pinterestFolder: v.trim() || "Pinterest" })));
      }
      el.createDiv({ cls: "setting-item-heading-sub", text: "Followed boards" });
      renderFollowed(el.createDiv("pinterest-followed"));

      new Setting(el).setName("Library").setHeading();
      new Setting(el)
        .setName("Folders on this computer")
        .setDesc("Pictures outside the vault that belong to the library. They stay in place and are copied into the vault only when used in a note.")
        .addButton((b) => b.setButtonText("Add folder").onClick(async () => {
          await plugin.addExternalFolder();
          render();
        }));
      for (const p of s.external) {
        new Setting(el).setName(p.split(/[\\/]/).filter(Boolean).pop() || p).setDesc(p).addExtraButton((b) => b.setIcon("x").setTooltip("Remove from library").onClick(async () => {
          await plugin.removeExternalFolder(p);
          render();
        }));
      }
      new Setting(el)
        .setName("Folder for added pictures")
        .setDesc("Where pictures dropped or pasted into the library are saved when no folder is open. Empty uses the attachment folder.")
        .addText((t) => t.setPlaceholder("Attachment folder").setValue(s.importFolder).onChange((v) => plugin.updateSettings({ importFolder: v.trim().replace(/^\/+|\/+$/g, "") })));

      new Setting(el).setName("Gallery").setHeading();
      new Setting(el).setName("Preview size").addDropdown((d) => d.addOptions({ small: "Small", medium: "Medium", large: "Large" }).setValue(s.tile).onChange((v) => plugin.updateSettings({ tile: v as any })));
      new Setting(el).setName("Show file names").addToggle((t) => t.setValue(s.showNames).onChange((v) => plugin.updateSettings({ showNames: v })));
      new Setting(el).setName("Modes").setDesc("Switch between writing and image mode with the two buttons at the top of the left ribbon, or with Ctrl/Cmd + Shift + I.");
    };
    render();
    const ref = app.workspace.on("images:status", () => {
      if (!el.isConnected) return app.workspace.offref(ref);
      if (!el.querySelector("input:focus")) render();
    });
  };
}
