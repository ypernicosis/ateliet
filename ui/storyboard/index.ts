// Storyboards: Pictogrep's drawing-from-references, grown into a storyboard
// tool. A storyboard is a `.storyboard` file in the vault (see model.ts),
// opened in its own view, listed in "Storyboards", embedded in notes with
// `![[name.storyboard]]`, reachable from both writing and image mode.

import { BoardOptionsModal } from "./dialogs.ts";
import { LIST_VIEW, StoryboardEmbed, StoryboardListView, confirmDelete } from "./list.ts";
import { STORYBOARD_VIEW, StoryboardView } from "./view.ts";
import { newBoard, PAPER } from "./model.ts";
import { StoryDoc } from "./store.ts";
import { boardFromClip, importSummary, readClip } from "./clipfile.ts";
import { FileModal } from "./references.ts";
import { askName } from "../images/collections.ts";
import { ImageItem } from "../images/api.ts";
import { MarkdownView } from "../obsidian/views.ts";
import { Menu, Notice } from "../obsidian/ui.ts";
import { TFile, TFolder } from "../obsidian/vault.ts";
import { invoke } from "../tauri.ts";

const SETTINGS_FILE = "ateliet/storyboard.json";

export class StoryboardPlugin {
  app: any;
  description = "";
  settings = { folder: "Storyboards", fps: 24, width: 1920, height: 1080, infinite: false, background: PAPER };
  private registered = false;
  private ribbonEl: HTMLElement | null = null;
  private off: (() => void)[] = [];

  async onEnable(app: any) {
    this.app = app;
    this.settings = { ...this.settings, ...(await app.readJson(SETTINGS_FILE, {})) };
    const r = app.workspace.viewRegistry;
    if (!this.registered) {
      r.registerView(STORYBOARD_VIEW, (leaf: any) => new StoryboardView(leaf));
      r.registerView(LIST_VIEW, (leaf: any) => new StoryboardListView(leaf, this));
      this.registered = true;
    }
    r.registerExtensions(["storyboard"], STORYBOARD_VIEW);
    app.embedRegistry.registerExtension("storyboard", (ctx: any, file: TFile) => new StoryboardEmbed(ctx, file, (f, n) => this.open(f, n)));
    this.ribbonEl = app.workspace.leftRibbon.addRibbonItemButton("storyboard:list", "clapperboard", "Storyboards", () => this.openList());
    this.registerCommands();
    const fileMenu = (menu: Menu, file: any) => this.fileMenu(menu, file);
    app.workspace.on("file-menu", fileMenu);
    this.off.push(() => app.workspace.off("file-menu", fileMenu));
    const hook = (menu: Menu, items: ImageItem[]) => this.imageMenu(menu, items);
    (app.imageMenuHooks ||= []).push(hook);
    this.off.push(() => (app.imageMenuHooks = app.imageMenuHooks.filter((h: any) => h !== hook)));
  }

  onDisable() {
    this.off.forEach((f) => f());
    this.off = [];
    if (this.ribbonEl) this.app.workspace.leftRibbon.removeRibbonAction(this.ribbonEl);
    this.app.workspace.viewRegistry.unregisterExtensions(["storyboard"]);
    this.app.embedRegistry.unregisterExtension("storyboard");
    this.app.workspace.detachLeavesOfType(STORYBOARD_VIEW);
    this.app.workspace.detachLeavesOfType(LIST_VIEW);
  }

  private async saveSettings() {
    await this.app.writeJson(SETTINGS_FILE, this.settings);
  }

  private registerCommands() {
    const c = (cmd: any) => this.app.commands.addCommand(cmd);
    c({ id: "storyboard:new", name: "New storyboard…", icon: "clapperboard", callback: () => this.create() });
    c({ id: "storyboard:list", name: "Open storyboards", icon: "clapperboard", callback: () => this.openList() });
    c({ id: "storyboard:from-clip", name: "New storyboard from a Clip Studio file…", icon: "layers", callback: () => this.createFromClip() });
    c({ id: "storyboard:from-pictogrep", name: "New storyboard from Pictogrep's drawings", icon: "images", callback: () => this.createFromPictogrep() });
    c({
      id: "storyboard:insert",
      name: "Insert storyboard into note…",
      icon: "clapperboard",
      editorCallback: (editor: any) => new FileModal(this.app, "storyboard", (f) => editor.replaceSelection(`![[${f.path}]]\n`), "Choose a storyboard").open(),
    });
    c({
      id: "storyboard:import-clip",
      name: "Import a Clip Studio file into this storyboard…",
      icon: "layers",
      checkCallback: (checking: boolean) => {
        const v = this.activeView();
        if (!v) return false;
        if (!checking) v.importClip(false);
        return true;
      },
    });
  }

  private activeView(): StoryboardView | null {
    const v = this.app.workspace.activeLeaf?.view;
    return v instanceof StoryboardView ? v : null;
  }

  /** The storyboard most recently in front (for "add to references"). */
  private lastView(): StoryboardView | null {
    const leaves = this.app.workspace.getLeavesOfType(STORYBOARD_VIEW);
    if (!leaves.length) return null;
    const active = this.activeView();
    if (active) return active;
    return leaves.sort((a: any, b: any) => (b.activeTime || 0) - (a.activeTime || 0))[0].view as StoryboardView;
  }

  async openList() {
    const ws = this.app.workspace;
    const leaf = ws.getLeavesOfType(LIST_VIEW)[0] || ws.getLeaf("tab");
    await leaf.setViewState({ type: LIST_VIEW, state: {}, active: true });
    ws.revealLeaf(leaf);
  }

  async open(file: TFile, newLeaf = false) {
    const ws = this.app.workspace;
    const existing = ws.getLeavesOfType(STORYBOARD_VIEW).find((l: any) => l.view.file === file);
    if (existing && !newLeaf) {
      ws.setActiveLeaf(existing, { focus: true });
      ws.revealLeaf(existing);
      return;
    }
    // A note in the way is not replaced: storyboards get their own tab.
    const current = ws.activeLeaf?.view;
    const leaf = newLeaf || current instanceof MarkdownView ? ws.getLeaf("tab") : ws.getLeaf(false);
    await leaf.openFile(file);
    ws.setActiveLeaf(leaf, { focus: true });
  }

  private uniquePath(folder: string, name: string): string {
    const base = (folder ? folder + "/" : "") + name.replace(/[\\/:*?"<>|]/g, "-");
    let p = `${base}.storyboard`;
    for (let i = 2; this.app.vault.getAbstractFileByPath(p); i++) p = `${base} ${i}.storyboard`;
    return p;
  }

  private async ensureFolder(folder: string) {
    if (folder && !this.app.vault.getAbstractFileByPath(folder)) await this.app.vault.createFolder(folder).catch(() => {});
  }

  private async write(path: string, doc: StoryDoc): Promise<TFile> {
    const bytes = await doc.encode();
    return this.app.vault.createBinary(path, bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
  }

  async create(opts: { folder?: string } = {}): Promise<TFile | null> {
    const s = this.settings;
    const modal = new BoardOptionsModal(this.app, { name: "Storyboard", folder: opts.folder ?? s.folder, width: s.width, height: s.height, infinite: s.infinite, background: s.background, fps: s.fps }, "new");
    modal.open();
    const o = await modal.result;
    if (!o) return null;
    Object.assign(this.settings, { folder: o.folder, width: o.width, height: o.height, infinite: o.infinite, background: o.background, fps: o.fps });
    await this.saveSettings();
    await this.ensureFolder(o.folder);
    const path = this.uniquePath(o.folder, o.name);
    const doc = StoryDoc.create(this.app.vault.adapter.getFullPath(path), newBoard(o));
    const file = await this.write(path, doc);
    await this.open(file, true);
    return file;
  }

  async createFromClip(clipPath?: string): Promise<TFile | null> {
    if (!clipPath) {
      const picked: string[] = await invoke("pick_files", { name: "Clip Studio", extensions: ["clip"], multiple: false });
      clipPath = picked[0];
    }
    if (!clipPath) return null;
    const notice = new Notice("Reading the Clip Studio file…", 0);
    try {
      const clip = await readClip(clipPath);
      const name = clipPath.split(/[\\/]/).pop()!.replace(/\.clip$/i, "");
      await this.ensureFolder(this.settings.folder);
      const path = this.uniquePath(this.settings.folder, name);
      const { doc, approximated } = await boardFromClip(this.app.vault.adapter.getFullPath(path), clip, this.settings.fps);
      const file = await this.write(path, doc);
      notice.hide();
      new Notice(`${clip.layers.length} layers imported (${clip.width} × ${clip.height}).${importSummary(clip, approximated)}`, 10000);
      await this.open(file, true);
      return file;
    } catch (e: any) {
      notice.hide();
      new Notice(`Could not import the Clip Studio file: ${e?.message || e}`);
      return null;
    }
  }

  async createFromPictogrep(): Promise<TFile | null> {
    const file = await this.create();
    if (!file) return null;
    const v = this.lastView();
    // The view builds asynchronously; wait for its editor.
    for (let i = 0; i < 50 && v && !v.ed?.frame; i++) await new Promise((r) => setTimeout(r, 100));
    await (v as any)?.importPictogrep?.();
    return file;
  }

  async rename(file: TFile) {
    const name = await askName(this.app, "Rename storyboard", file.basename);
    if (!name || name === file.basename) return;
    const parent = file.parent && !file.parent.isRoot() ? file.parent.path + "/" : "";
    await this.app.fileManager.renameFile(file, `${parent}${name}.storyboard`).catch((e: any) => new Notice(String(e)));
  }

  async duplicate(file: TFile) {
    const parent = file.parent && !file.parent.isRoot() ? file.parent.path : "";
    const path = this.uniquePath(parent, `${file.basename} copy`);
    await this.app.vault.copy(file, path);
  }

  private fileMenu(menu: Menu, file: any) {
    if (file instanceof TFolder) {
      menu.addItem((i) => i.setTitle("New storyboard").setIcon("clapperboard").setSection?.("action").onClick(() => this.create({ folder: file.isRoot() ? "" : file.path })));
    } else if (file instanceof TFile && file.extension === "storyboard") {
      menu.addItem((i) => i.setTitle("Duplicate storyboard").setIcon("copy").onClick(() => this.duplicate(file)));
      menu.addItem((i) => i.setTitle("Delete storyboard").setIcon("trash-2").setWarning(true).onClick(() => confirmDelete(this.app, file)));
    } else if (file instanceof TFile && file.extension === "clip") {
      menu.addItem((i) => i.setTitle("Open as a new storyboard").setIcon("clapperboard").onClick(() => this.createFromClip(this.app.vault.adapter.getFullPath(file.path))));
      const v = this.lastView();
      if (v) menu.addItem((i) => i.setTitle(`Import into "${v.file?.basename}"`).setIcon("layers").onClick(() => v.importClip(false, this.app.vault.adapter.getFullPath(file.path))));
    }
  }

  /** "Add to storyboard references" in the picture library's menus. */
  private imageMenu(menu: Menu, items: ImageItem[]) {
    const v = this.lastView();
    if (!v?.ed) return;
    const refs = (v as any).refs;
    menu.addItem((i) =>
      i.setTitle(`Add to the references of "${v.file?.basename}"`).setIcon("clapperboard").onClick(() => refs?.add(items.map((it) => ({ name: it.name, path: it.abs, from: it.folder || undefined })))),
    );
  }
}
