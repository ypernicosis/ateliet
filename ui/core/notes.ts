// Daily notes, templates, random note, word count.

import { FuzzySuggestModal } from "../obsidian/ui.ts";
import { TFile, TFolder, Vault } from "../obsidian/vault.ts";
import { moment, normalizePath } from "../obsidian/util.ts";
import { MarkdownView } from "../obsidian/views.ts";

export function applyTemplateVars(text: string, title: string, opts: { dateFormat?: string; timeFormat?: string } = {}): string {
  const now = moment();
  return text
    .replace(/{{\s*title\s*}}/gi, title)
    .replace(/{{\s*(date|time)\s*(?::([^}]*))?}}/gi, (_m, kind: string, fmt?: string) => {
      if (fmt) return now.format(fmt.trim());
      return kind.toLowerCase() === "date" ? now.format(opts.dateFormat || "YYYY-MM-DD") : now.format(opts.timeFormat || "HH:mm");
    });
}

export class DailyNotesPlugin {
  app: any;
  plugin: any;
  options: { format?: string; folder?: string; template?: string; autorun?: boolean } = {};

  async onEnable(app: any) {
    this.app = app;
    this.options = await app.readJson("daily-notes.json", {});
    app.commands.addCommand({ id: "daily-notes", name: "Daily notes: Open today's daily note", icon: "calendar-days", callback: () => this.openDay(moment()) });
    app.commands.addCommand({ id: "daily-notes:goto-prev", name: "Daily notes: Open previous daily note", checkCallback: (c: boolean) => this.adjacent(-1, c) });
    app.commands.addCommand({ id: "daily-notes:goto-next", name: "Daily notes: Open next daily note", checkCallback: (c: boolean) => this.adjacent(1, c) });
    this.plugin.ribbonEl = app.workspace.leftRibbon.addRibbonItemButton("daily-notes", "calendar-days", "Open today's daily note", () => this.openDay(moment()));
    if (this.options.autorun) app.workspace.onLayoutReady(() => this.openDay(moment()));
  }

  onDisable() {
    this.plugin.ribbonEl?.remove();
  }

  getFormat() {
    return this.options.format || "YYYY-MM-DD";
  }

  pathFor(date: any): string {
    const folder = normalizePath(this.options.folder || "/");
    const name = date.format(this.getFormat());
    return normalizePath((folder === "/" ? "" : folder + "/") + name + ".md");
  }

  async getDailyNote(date: any, create = true): Promise<TFile | null> {
    const path = this.pathFor(date);
    const existing = this.app.vault.getAbstractFileByPath(path);
    if (existing instanceof TFile) return existing;
    if (!create) return null;
    let content = "";
    if (this.options.template) {
      const t = this.app.metadataCache.getFirstLinkpathDest(this.options.template, "");
      if (t instanceof TFile) content = applyTemplateVars(await this.app.vault.read(t), date.format(this.getFormat()), { dateFormat: this.getFormat() });
    }
    const dir = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
    if (dir && !this.app.vault.getAbstractFileByPath(dir)) await this.app.vault.createFolder(dir).catch(() => {});
    return this.app.vault.create(path, content);
  }

  async openDay(date: any) {
    const f = await this.getDailyNote(date);
    if (f) await this.app.workspace.getLeaf(false).openFile(f);
  }

  private adjacent(dir: number, checking: boolean): boolean {
    const f = this.app.workspace.getActiveFile();
    if (!f) return false;
    const d = moment(f.basename, this.getFormat(), true);
    if (!d.isValid()) return false;
    if (checking) return true;
    const notes = this.app.vault.getMarkdownFiles()
      .map((x: TFile) => ({ x, d: moment(x.basename, this.getFormat(), true) }))
      .filter((o: any) => o.d.isValid())
      .sort((a: any, b: any) => a.d.valueOf() - b.d.valueOf());
    const i = notes.findIndex((o: any) => o.x === f);
    const target = notes[i + dir];
    if (target) this.app.workspace.getLeaf(false).openFile(target.x);
    return true;
  }
}

export class TemplatesPlugin {
  app: any;
  plugin: any;
  options: { folder?: string; dateFormat?: string; timeFormat?: string } = {};

  async onEnable(app: any) {
    this.app = app;
    this.options = await app.readJson("templates.json", {});
    app.commands.addCommand({ id: "insert-template", name: "Templates: Insert template", icon: "files", editorCallback: (editor: any, view: any) => this.pick(editor, view) });
    app.commands.addCommand({ id: "insert-current-date", name: "Templates: Insert current date", editorCallback: (e: any) => e.replaceSelection(moment().format(this.options.dateFormat || "YYYY-MM-DD")) });
    app.commands.addCommand({ id: "insert-current-time", name: "Templates: Insert current time", editorCallback: (e: any) => e.replaceSelection(moment().format(this.options.timeFormat || "HH:mm")) });
  }

  templates(): TFile[] {
    const folder = this.app.vault.getAbstractFileByPath(normalizePath(this.options.folder || ""));
    const out: TFile[] = [];
    if (folder instanceof TFolder) Vault.recurseChildren(folder, (f) => f instanceof TFile && f.extension === "md" && out.push(f));
    return out;
  }

  pick(editor: any, view: MarkdownView) {
    const list = this.templates();
    if (!list.length) {
      new this.app.Notice(this.options.folder ? `No templates in "${this.options.folder}".` : "Set a template folder in Settings → Templates.");
      return;
    }
    const plugin = this;
    new (class extends FuzzySuggestModal<TFile> {
      getItems() {
        return list;
      }
      getItemText(f: TFile) {
        return f.path.slice((plugin.options.folder || "").length).replace(/^\//, "").replace(/\.md$/, "");
      }
      async onChooseItem(f: TFile) {
        const text = applyTemplateVars(await plugin.app.vault.read(f), view.file?.basename || "", plugin.options);
        editor.replaceSelection(text);
      }
    })(this.app).open();
  }
}

export class RandomNotePlugin {
  app: any;
  plugin: any;
  onEnable(app: any) {
    this.app = app;
    app.commands.addCommand({ id: "random-note", name: "Random note: Open random note", icon: "dices", callback: () => {
      const files = app.vault.getMarkdownFiles();
      if (files.length) app.workspace.getLeaf(false).openFile(files[Math.floor(Math.random() * files.length)]);
    } });
  }
}

export class WordCountPlugin {
  app: any;
  plugin: any;
  el: HTMLElement | null = null;
  onEnable(app: any) {
    this.app = app;
    this.el = app.statusBar.createDiv("status-bar-item plugin-word-count");
    const update = () => {
      const v = app.workspace.getActiveViewOfType(MarkdownView);
      if (!v || !this.el) {
        this.el?.setText("");
        return;
      }
      const sel = v.editor.getSelection();
      const text = (sel || v.getViewData()).replace(/^---\r?\n[\s\S]*?\r?\n---/, "");
      const words = (text.match(/[\p{L}\p{N}][\p{L}\p{N}'’_-]*/gu) || []).length;
      const chars = text.length;
      this.el.setText(`${words.toLocaleString()} ${words === 1 ? "word" : "words"} ${chars.toLocaleString()} ${chars === 1 ? "character" : "characters"}`);
    };
    app.on("status-bar-update", update);
    app.workspace.on("active-leaf-change", update);
    app.workspace.on("editor-selection-change", () => app.updateStatusBar());
  }
  onDisable() {
    this.el?.remove();
  }
}

export class BacklinkCountStatus {
  app: any;
  plugin: any;
  el: HTMLElement | null = null;
  onEnable(app: any) {
    this.app = app;
    this.el = app.statusBar.createDiv("status-bar-item plugin-backlink mod-clickable");
    const update = () => {
      const f = app.workspace.getActiveFile();
      if (!f || !this.el) return this.el?.setText("");
      const n = app.metadataCache.getBacklinksForFile(f).count();
      this.el.setText(`${n} backlink${n === 1 ? "" : "s"}`);
    };
    this.el.addEventListener("click", () => app.workspace.ensureSideLeaf("backlink", "right", { reveal: true }));
    app.workspace.on("file-open", update);
    app.metadataCache.on("resolved", update);
  }
  onDisable() {
    this.el?.remove();
  }
}
