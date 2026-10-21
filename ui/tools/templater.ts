// Templater, built in. Templates run in the script engine inside the Rust
// core (src/script): `<% %>` commands, `<%* %>` JavaScript and the `tp` API.
// This file picks templates, answers the prompts a template asks for and
// applies what it produced (text, cursor, renames, new notes).

import { Modal, Notice, Setting, SuggestModal } from "../obsidian/ui.ts";
import { TFile } from "../obsidian/vault.ts";
import { MarkdownView } from "../obsidian/views.ts";
import { normalizePath } from "../obsidian/util.ts";
import { invoke, listen } from "../tauri.ts";
import { loadToolSettings, locale, saveToolSettings, vaultPath } from "./common.ts";

export const TEMPLATER_DEFAULTS = {
  templates_folder: "",
  enabled_templates_hotkeys: [] as string[],
  trigger_on_file_creation: false,
  folder_templates: [] as { folder: string; template: string }[],
  auto_jump_to_cursor: false,
};

type Options = typeof TEMPLATER_DEFAULTS;

const enum RunMode {
  CreateNewFromTemplate = 0,
  AppendActiveFile = 1,
  OverwriteFile = 2,
  OverwriteActiveFile = 3,
}

class PromptModal extends Modal {
  private done = false;
  constructor(app: any, private title: string, private value: string, private multiline: boolean, private resolve: (v: string | null) => void) {
    super(app);
  }
  onOpen() {
    this.titleEl.setText(this.title || "Templater");
    const input = this.multiline ? this.contentEl.createEl("textarea", { cls: "templater-prompt-input" }) : this.contentEl.createEl("input", { cls: "templater-prompt-input", attr: { type: "text" } });
    (input as any).value = this.value;
    input.style.width = "100%";
    const submit = () => {
      this.done = true;
      this.resolve((input as any).value);
      this.close();
    };
    input.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter" && (!this.multiline || e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        submit();
      }
    });
    new Setting(this.contentEl).addButton((b) => b.setButtonText("OK").setCta().onClick(submit));
    setTimeout(() => (input as HTMLElement).focus(), 0);
  }
  onClose() {
    if (!this.done) this.resolve(null);
  }
}

class ChoiceModal extends SuggestModal<number> {
  private done = false;
  constructor(app: any, private labels: string[], placeholder: string, private resolve: (i: number | null) => void, limit?: number) {
    super(app);
    if (placeholder) this.setPlaceholder(placeholder);
    if (limit) this.limit = limit;
  }
  getSuggestions(q: string): number[] {
    const needle = q.toLowerCase();
    return this.labels.map((_, i) => i).filter((i) => this.labels[i].toLowerCase().includes(needle));
  }
  renderSuggestion(i: number, el: HTMLElement) {
    el.setText(this.labels[i]);
  }
  onChooseSuggestion(i: number) {
    this.done = true;
    this.resolve(i);
  }
  onClose() {
    // a choice closes the modal before onChooseSuggestion runs
    setTimeout(() => {
      if (!this.done) this.resolve(null);
    }, 0);
  }
}

class MultiChoiceModal extends Modal {
  private done = false;
  constructor(app: any, private labels: string[], private title: string, private resolve: (v: number[] | null) => void) {
    super(app);
  }
  onOpen() {
    this.titleEl.setText(this.title || "Templater");
    const picked = new Set<number>();
    this.labels.forEach((l, i) => {
      new Setting(this.contentEl).setName(l).addToggle((t) => t.onChange((v) => (v ? picked.add(i) : picked.delete(i))));
    });
    new Setting(this.contentEl).addButton((b) =>
      b.setButtonText("OK").setCta().onClick(() => {
        this.done = true;
        this.resolve(Array.from(picked).sort((a, b) => a - b));
        this.close();
      }),
    );
  }
  onClose() {
    if (!this.done) this.resolve(null);
  }
}

class TemplatePicker extends SuggestModal<TFile> {
  constructor(app: any, private files: TFile[], private choose: (f: TFile) => void) {
    super(app);
    this.setPlaceholder("Choose a template…");
  }
  getSuggestions(q: string): TFile[] {
    const n = q.toLowerCase();
    return this.files.filter((f) => f.path.toLowerCase().includes(n));
  }
  renderSuggestion(f: TFile, el: HTMLElement) {
    el.setText(f.basename);
    el.createDiv({ cls: "suggestion-note", text: f.parent?.path && f.parent.path !== "/" ? f.parent.path : "" });
  }
  onChooseSuggestion(f: TFile) {
    this.choose(f);
  }
}

export class TemplaterTool {
  app: any;
  plugin: any;
  options: Options = { ...TEMPLATER_DEFAULTS };
  private unlisten: (() => void) | null = null;
  private hotkeyIds: string[] = [];
  private createRef: any = null;

  async onEnable(app: any) {
    this.app = app;
    this.options = await loadToolSettings(app, "templater", "templater-obsidian", TEMPLATER_DEFAULTS);
    this.unlisten = await listen("tp-prompt", (p: any) => this.answer(p));
    const cmd = (id: string, name: string, extra: any) => app.commands.addCommand({ id: `templater-obsidian:${id}`, name: `Templater: ${name}`, ...extra });
    cmd("insert-templater", "Open insert template modal", {
      icon: "layout-template",
      hotkeys: [{ modifiers: ["Alt"], key: "e" }],
      checkCallback: (checking: boolean) => {
        if (!this.activeMarkdown()) return false;
        if (!checking) this.pick((f) => this.insert(f));
        return true;
      },
    });
    cmd("create-new-note-from-template", "Create new note from template", {
      icon: "file-plus",
      hotkeys: [{ modifiers: ["Alt"], key: "n" }],
      callback: () => this.pick((f) => this.createFromTemplate(f)),
    });
    cmd("replace-in-file-templater", "Replace templates in the active file", {
      hotkeys: [{ modifiers: ["Alt"], key: "r" }],
      checkCallback: (checking: boolean) => {
        if (!this.activeMarkdown()) return false;
        if (!checking) this.replaceInActive();
        return true;
      },
    });
    this.registerHotkeyTemplates();
    this.plugin.ribbonEl = app.workspace.leftRibbon.addRibbonItemButton("templater", "layout-template", "Templater", () => this.pick((f) => (this.activeMarkdown() ? this.insert(f) : this.createFromTemplate(f))));
    app.workspace.onLayoutReady(() => {
      this.createRef = app.vault.on("create", (f: any) => this.onCreate(f));
    });
  }

  onDisable() {
    this.unlisten?.();
    this.plugin.ribbonEl?.remove();
    if (this.createRef) this.app.vault.offref(this.createRef);
    for (const id of this.hotkeyIds) this.app.commands.removeCommand(id);
  }

  private registerHotkeyTemplates() {
    for (const id of this.hotkeyIds) this.app.commands.removeCommand(id);
    this.hotkeyIds = [];
    for (const path of this.options.enabled_templates_hotkeys || []) {
      if (!path) continue;
      const id = `templater-obsidian:${path}`;
      this.hotkeyIds.push(id);
      this.app.commands.addCommand({
        id,
        name: `Templater: Insert ${path}`,
        callback: () => {
          const f = this.app.vault.getAbstractFileByPath(path);
          if (!(f instanceof TFile)) return new Notice(`Templater: template not found: ${path}`);
          if (this.activeMarkdown()) this.insert(f);
          else this.createFromTemplate(f);
        },
      });
    }
  }

  private activeMarkdown(): MarkdownView | null {
    const v = this.app.workspace.getActiveViewOfType(MarkdownView);
    return v && v.file ? v : null;
  }

  templates(): TFile[] {
    const folder = normalizePath(this.options.templates_folder || "/");
    const all: TFile[] = this.app.vault.getMarkdownFiles();
    const inFolder = folder === "/" ? all : all.filter((f) => f.path.startsWith(folder + "/"));
    return inFolder.sort((a, b) => a.path.localeCompare(b.path));
  }

  private pick(choose: (f: TFile) => void) {
    const list = this.templates();
    if (!list.length) return new Notice("Templater: no templates found. Set the template folder in the Templater settings.");
    new TemplatePicker(this.app, list, choose).open();
  }

  // ── running ──────────────────────────────────────────────────────────────

  private async run(template: TFile | null, templateText: string | null, target: string, mode: RunMode, content?: string, selection?: string): Promise<any | null> {
    try {
      const res = await invoke("tp_run", {
        args: {
          vault: vaultPath(this.app),
          template: template?.path || "",
          templateText,
          target,
          content: content ?? null,
          selection: selection ?? "",
          runMode: mode,
          locale: locale(),
          weekStart: 0,
        },
      });
      return res.cancelled ? null : res;
    } catch (e: any) {
      console.error("Templater", e);
      new Notice(`Templater: ${String(e?.message || e).split("\n")[0]}`, 8000);
      return null;
    }
  }

  async insert(template: TFile) {
    const view = this.activeMarkdown();
    if (!view) return;
    const editor = view.editor;
    const res = await this.run(template, null, view.file!.path, RunMode.AppendActiveFile, editor.getValue(), editor.getSelection());
    if (!res) return;
    const start = editor.posToOffset(editor.getCursor("from"));
    editor.replaceSelection(res.text);
    this.placeCursor(editor, start, res.cursors);
    await this.applyOps(view.file!, res.ops, editor);
  }

  async createFromTemplate(template: TFile, folder?: string) {
    const active = this.app.workspace.getActiveFile();
    const dir = folder ?? this.app.fileManager.getNewFileParent?.(active?.path || "")?.path ?? (active?.parent?.path || "/");
    const base = normalizePath((dir && dir !== "/" ? dir + "/" : "") + "Untitled");
    const path = this.app.vault.getAvailablePath(base, "md");
    const file: TFile = await this.app.vault.create(path, "");
    const res = await this.run(template, null, path, RunMode.CreateNewFromTemplate, "");
    if (!res) return;
    await this.app.vault.modify(file, res.text);
    const leaf = this.app.workspace.getLeaf(false);
    await leaf.openFile(file);
    const view = leaf.view instanceof MarkdownView ? leaf.view : null;
    if (view) this.placeCursor(view.editor, 0, res.cursors);
    await this.applyOps(file, res.ops, view?.editor);
  }

  async replaceInActive() {
    const view = this.activeMarkdown();
    if (!view) return;
    const editor = view.editor;
    const text = editor.getValue();
    const res = await this.run(null, text, view.file!.path, RunMode.OverwriteActiveFile, text);
    if (!res) return;
    editor.setValue(res.text);
    this.placeCursor(editor, 0, res.cursors);
    await this.applyOps(view.file!, res.ops, editor);
  }

  /** The first `tp.file.cursor()` position (Templater jumps there after inserting). */
  private placeCursor(editor: any, start: number, cursors: number[]) {
    if (!cursors?.length) return;
    editor.setCursor(editor.offsetToPos(start + cursors[0]));
    editor.focus?.();
  }

  private async applyOps(file: TFile, ops: any[], editor?: any) {
    for (const op of ops || []) {
      try {
        if (op.op === "rename" && op.path && op.path !== file.path) {
          const dir = op.path.includes("/") ? op.path.slice(0, op.path.lastIndexOf("/")) : "";
          if (dir && !this.app.vault.getAbstractFileByPath(dir)) await this.app.vault.createFolder(dir);
          const target = this.app.vault.getAbstractFileByPath(op.path) ? this.app.vault.getAvailablePath(op.path.replace(/\.md$/, ""), "md") : op.path;
          await this.app.fileManager.renameFile(file, target);
        } else if (op.op === "open" && op.path) {
          const f = this.app.vault.getAbstractFileByPath(op.path);
          if (f instanceof TFile) await this.app.workspace.getLeaf("tab").openFile(f);
        } else if (op.op === "cursor_append" && editor) {
          editor.replaceSelection(op.content || "");
        }
      } catch (e) {
        console.error("Templater", op, e);
      }
    }
  }

  /** A note created inside a folder with a folder template gets that template. */
  private async onCreate(f: any) {
    if (!this.options.trigger_on_file_creation || !(f instanceof TFile) || f.extension !== "md") return;
    const rules = (this.options.folder_templates || []).filter((r) => r.folder && r.template);
    let best: { folder: string; template: string } | null = null;
    for (const r of rules) {
      const folder = normalizePath(r.folder);
      if ((folder === "/" || f.path.startsWith(folder + "/")) && (!best || folder.length > normalizePath(best.folder).length)) best = r;
    }
    if (!best) return;
    // let whoever created it write its content first; only empty notes are filled
    await new Promise((r) => setTimeout(r, 300));
    const text = await this.app.vault.read(f);
    if (text.trim()) return;
    const tpl = this.app.vault.getAbstractFileByPath(normalizePath(best.template));
    if (!(tpl instanceof TFile)) return;
    const res = await this.run(tpl, null, f.path, RunMode.CreateNewFromTemplate, "");
    if (!res) return;
    await this.app.vault.modify(f, res.text);
    await this.applyOps(f, res.ops);
  }

  // ── prompts from a running template ──────────────────────────────────────

  private async answer(p: any) {
    const reply = (value: any) => invoke("tp_answer", { id: p.id, value });
    switch (p.kind) {
      case "prompt":
        new PromptModal(this.app, p.text, p.value || "", !!p.multiline, (v) => reply(v)).open();
        break;
      case "suggester":
        new ChoiceModal(this.app, p.labels || [], p.placeholder || "", (i) => reply(i), p.limit || undefined).open();
        break;
      case "multi_suggester":
        new MultiChoiceModal(this.app, p.labels || [], p.placeholder || "", (v) => reply(v)).open();
        break;
      case "clipboard":
        reply(await navigator.clipboard.readText().catch(() => ""));
        break;
      default:
        reply(null);
    }
  }

  // ── settings tab ─────────────────────────────────────────────────────────

  settingsTab() {
    return (el: HTMLElement) => {
      const o = this.options;
      const save = () => saveToolSettings(this.app, "templater", o);
      el.createDiv({ cls: "setting-item-description", text: "Templater is built into Ateliet: <% %> commands and <%* %> scripts run in the app's core. Settings are read from the old plugin the first time." });
      new Setting(el).setName("Template folder location").setDesc("Templates are the notes in this folder.").addText((t) => t.setPlaceholder("Templates").setValue(o.templates_folder).onChange((v) => { o.templates_folder = v; save(); }));
      new Setting(el).setName("Trigger on new file creation").setDesc("Fill empty notes created in a folder with that folder's template.").addToggle((t) => t.setValue(!!o.trigger_on_file_creation).onChange((v) => { o.trigger_on_file_creation = v; save(); }));
      new Setting(el).setName("Folder templates").setHeading();
      const list = el.createDiv();
      const draw = () => {
        list.empty();
        (o.folder_templates || []).forEach((r, i) => {
          new Setting(list)
            .addText((t) => t.setPlaceholder("Folder").setValue(r.folder).onChange((v) => { r.folder = v; save(); }))
            .addText((t) => t.setPlaceholder("Template path").setValue(r.template).onChange((v) => { r.template = v; save(); }))
            .addExtraButton((b) => b.setIcon("trash-2").onClick(() => { o.folder_templates.splice(i, 1); save(); draw(); }));
        });
        new Setting(list).addButton((b) => b.setButtonText("Add folder template").onClick(() => { (o.folder_templates ||= []).push({ folder: "", template: "" }); save(); draw(); }));
      };
      draw();
      new Setting(el).setName("Template hotkeys").setDesc("Each template listed here becomes a command you can bind in Hotkeys.").setHeading();
      const hk = el.createDiv();
      const drawHk = () => {
        hk.empty();
        (o.enabled_templates_hotkeys || []).forEach((p, i) => {
          new Setting(hk)
            .addText((t) => t.setPlaceholder("Template path").setValue(p).onChange((v) => { o.enabled_templates_hotkeys[i] = v; save(); this.registerHotkeyTemplates(); }))
            .addExtraButton((b) => b.setIcon("trash-2").onClick(() => { o.enabled_templates_hotkeys.splice(i, 1); save(); this.registerHotkeyTemplates(); drawHk(); }));
        });
        new Setting(hk).addButton((b) => b.setButtonText("Add template hotkey").onClick(() => { (o.enabled_templates_hotkeys ||= []).push(""); save(); drawHk(); }));
      };
      drawHk();
    };
  }
}

