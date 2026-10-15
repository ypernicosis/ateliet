// Global search, quick switcher, command palette and folder picker.

import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { FuzzySuggestModal, Keymap, SuggestModal, type FuzzyMatch } from "../obsidian/ui.ts";
import { TFile, TFolder } from "../obsidian/vault.ts";
import { debounce, parseFrontMatterAliases, prepareFuzzySearch, renderMatches } from "../obsidian/util.ts";
import { formatHotkey } from "../obsidian/plugin.ts";
import { invoke } from "../tauri.ts";

// ── global search view ────────────────────────────────────────────────────

export class SearchView extends ItemView {
  inputEl!: HTMLInputElement;
  resultsEl!: HTMLElement;
  summaryEl!: HTMLElement;
  caseSensitive = false;
  private token = 0;
  private run = debounce(() => this.search(), 250, true);

  getViewType() {
    return "search";
  }
  getDisplayText() {
    return "Search";
  }
  getIcon() {
    return "search";
  }

  async onOpen() {
    this.headerEl.hide();
    const row = this.contentEl.createDiv("search-row");
    const wrap = row.createDiv("search-input-container global-search-input-container");
    this.inputEl = wrap.createEl("input", { type: "search", attr: { placeholder: "Search...", spellcheck: "false", enterkeyhint: "search" } });
    const clear = wrap.createDiv("search-input-clear-button");
    clear.addEventListener("click", () => {
      this.inputEl.value = "";
      this.search();
    });
    const caseBtn = row.createDiv({ cls: "clickable-icon", attr: { "aria-label": "Match case" } });
    setIcon(caseBtn, "case-sensitive");
    caseBtn.addEventListener("click", () => {
      this.caseSensitive = !this.caseSensitive;
      caseBtn.toggleClass("is-active", this.caseSensitive);
      this.search();
    });
    this.inputEl.addEventListener("input", () => this.run());
    this.inputEl.addEventListener("keydown", (e) => e.key === "Enter" && this.search());
    this.summaryEl = this.contentEl.createDiv("search-result-container-summary search-results-info");
    this.resultsEl = this.contentEl.createDiv("search-result-container");
  }

  getState() {
    return { query: this.inputEl?.value || "", matchingCase: this.caseSensitive };
  }
  async setState(state: any) {
    if (typeof state?.query === "string" && this.inputEl) {
      this.inputEl.value = state.query;
      this.caseSensitive = !!state.matchingCase;
      this.search();
    }
  }

  setQuery(q: string) {
    this.inputEl.value = q;
    this.search();
    this.inputEl.focus();
  }

  async search() {
    const q = this.inputEl.value.trim();
    const token = ++this.token;
    this.resultsEl.empty();
    this.summaryEl.empty();
    if (!q) return;
    this.summaryEl.setText("Searching…");
    const started = performance.now();
    const results: any[] = await invoke("search_vault", { root: this.app.vault.adapter.basePath, query: q, caseSensitive: this.caseSensitive });
    if (token !== this.token) return;
    const total = results.reduce((n, r) => n + r.total, 0);
    this.summaryEl.setText(`${results.length} files · ${total} matches · ${Math.round(performance.now() - started)} ms`);
    for (const r of results) {
      const file = this.app.vault.getAbstractFileByPath(r.path);
      if (!(file instanceof TFile)) continue;
      const item = this.resultsEl.createDiv("tree-item search-result");
      const title = item.createDiv("tree-item-self search-result-file-title is-clickable");
      const collapse = title.createDiv("tree-item-icon collapse-icon");
      setIcon(collapse, "right-triangle");
      title.createDiv({ cls: "tree-item-inner", text: file.extension === "md" ? file.basename : file.name });
      title.createDiv({ cls: "tree-item-flair-outer" }).createSpan({ cls: "tree-item-flair", text: String(r.total || "") });
      const matches = item.createDiv("search-result-file-matches");
      title.addEventListener("click", (e) => {
        if ((e.target as HTMLElement).closest(".collapse-icon")) {
          const c = !item.hasClass("is-collapsed");
          item.toggleClass("is-collapsed", c);
          matches.toggle(!c);
          return;
        }
        this.app.workspace.getLeaf(Keymap.isModEvent(e)).openFile(file);
      });
      for (const m of r.matches) {
        const el = matches.createDiv("search-result-file-match tappable");
        // keep some context around the first match
        const [s0] = m.ranges[0] || [0, 0];
        const offset = Math.max(0, s0 - 60);
        const text = (offset ? "…" : "") + m.text.slice(offset);
        renderMatches(el, text, m.ranges.map(([a, b]: [number, number]) => [a - offset + (offset ? 1 : 0), b - offset + (offset ? 1 : 0)]));
        el.addEventListener("click", (e) => {
          this.app.workspace.getLeaf(Keymap.isModEvent(e)).openFile(file, { eState: { line: m.line, cursor: { from: { line: m.line, ch: m.ranges[0]?.[0] || 0 }, to: { line: m.line, ch: m.ranges[0]?.[1] || 0 } } } });
        });
      }
    }
  }
}

export class GlobalSearchPlugin {
  app: any;
  plugin: any;
  onEnable(app: any) {
    this.app = app;
    app.workspace.viewRegistry.registerView("search", (leaf: any) => new SearchView(leaf));
    app.commands.addCommand({ id: "global-search:open", name: "Search: Search in all files", icon: "search", hotkeys: [{ modifiers: ["Mod", "Shift"], key: "F" }], callback: () => {
      const sel = app.workspace.activeEditor?.editor?.getSelection();
      this.openGlobalSearch(sel || "");
    } });
  }
  async openGlobalSearch(query: string) {
    const leaf = await this.app.workspace.ensureSideLeaf("search", "left", { reveal: true });
    await this.app.workspace.revealLeaf(leaf);
    (leaf.view as SearchView).setQuery(query);
  }
  getGlobalSearchQuery() {
    const leaf = this.app.workspace.getLeavesOfType("search")[0];
    return leaf ? (leaf.view as SearchView).inputEl.value : "";
  }
}

// ── quick switcher ────────────────────────────────────────────────────────

interface SwitcherItem {
  file: TFile | null;
  path: string;
  alias?: string;
  create?: boolean;
}

export class QuickSwitcher extends SuggestModal<{ item: SwitcherItem; match: any }> {
  constructor(app: any) {
    super(app);
    this.setPlaceholder("Find or create a note...");
    this.setInstructions([
      { command: "↑↓", purpose: "to navigate" },
      { command: "↵", purpose: "to open" },
      { command: "⌘ ↵", purpose: "to open in new tab" },
      { command: "⇧ ↵", purpose: "to create" },
      { command: "esc", purpose: "to dismiss" },
    ]);
    this.scope.register(["Shift"], "Enter", (e) => {
      this.create(this.inputEl.value, Keymap.isModEvent(e));
      return false;
    });
    this.limit = 50;
  }

  getSuggestions(query: string) {
    const files: SwitcherItem[] = [];
    const recent = this.app.workspace.getLastOpenFiles();
    for (const f of this.app.vault.getFiles()) {
      files.push({ file: f, path: f.path });
      if (f.extension === "md") for (const a of parseFrontMatterAliases(this.app.metadataCache.getFileCache(f)?.frontmatter) || []) files.push({ file: f, path: f.path, alias: a });
    }
    if (!query.trim()) {
      const byPath = new Map(files.filter((x) => !x.alias).map((x) => [x.path, x]));
      const out = recent.map((p: string) => byPath.get(p)).filter(Boolean).map((item: any) => ({ item, match: { score: 0, matches: [] } }));
      return out.length ? out : files.filter((x) => !x.alias).slice(0, 50).map((item) => ({ item, match: { score: 0, matches: [] } }));
    }
    const fuzzy = prepareFuzzySearch(query);
    const out: { item: SwitcherItem; match: any }[] = [];
    for (const it of files) {
      const text = it.alias ?? (it.file!.extension === "md" ? it.path.slice(0, -3) : it.path);
      const m = fuzzy(text);
      if (m) {
        // prefer basename matches
        const base = it.alias ?? it.file!.basename;
        const bm = fuzzy(base);
        out.push({ item: it, match: { score: m.score + (bm ? 2 + bm.score * 0.1 : 0), matches: m.matches } });
      }
    }
    out.sort((a, b) => b.match.score - a.match.score);
    const exact = out.some((x) => (x.item.alias ?? x.item.file!.basename).toLowerCase() === query.toLowerCase());
    if (!exact) out.push({ item: { file: null, path: query, create: true }, match: { score: -1e9, matches: [] } });
    return out.slice(0, this.limit);
  }

  renderSuggestion(v: { item: SwitcherItem; match: any }, el: HTMLElement) {
    el.addClass("mod-complex");
    const content = el.createDiv("suggestion-content");
    const title = content.createDiv("suggestion-title");
    const it = v.item;
    if (it.create) {
      title.setText(it.path);
      el.createDiv("suggestion-aux").createSpan({ cls: "suggestion-hotkey", text: "Enter to create" });
      return;
    }
    const f = it.file!;
    const text = it.alias ?? (f.extension === "md" ? f.path.slice(0, -3) : f.path);
    renderMatches(title, text, v.match.matches);
    if (it.alias) {
      content.createDiv({ cls: "suggestion-note", text: f.path });
      const aux = el.createDiv("suggestion-aux");
      setIcon(aux.createSpan("suggestion-flair"), "forward");
    }
    if (f.extension !== "md") el.createDiv("suggestion-aux").createSpan({ cls: "suggestion-flair", text: f.extension });
  }

  async create(name: string, newTab: boolean) {
    this.close();
    if (!name.trim()) return;
    const parent = this.app.fileManager.getNewFileParent(this.app.workspace.getActiveFile()?.path || "");
    const path = (parent.isRoot() ? "" : parent.path + "/") + name.trim() + (name.endsWith(".md") ? "" : ".md");
    const existing = this.app.vault.getAbstractFileByPath(path);
    const file = existing instanceof TFile ? existing : await this.app.vault.create(path, "");
    await this.app.workspace.getLeaf(newTab ? "tab" : false).openFile(file);
  }

  onChooseSuggestion(v: { item: SwitcherItem; match: any }, evt: MouseEvent | KeyboardEvent) {
    if (v.item.create) return this.create(v.item.path, !!Keymap.isModEvent(evt));
    this.app.workspace.getLeaf(Keymap.isModEvent(evt)).openFile(v.item.file!);
  }
}

// ── command palette ───────────────────────────────────────────────────────

export class CommandPalette extends FuzzySuggestModal<any> {
  plugin: CommandPalettePlugin;
  constructor(app: any, plugin: CommandPalettePlugin) {
    super(app);
    this.plugin = plugin;
    this.setPlaceholder("Select a command...");
    this.setInstructions([
      { command: "↑↓", purpose: "to navigate" },
      { command: "↵", purpose: "to use" },
      { command: "esc", purpose: "to dismiss" },
    ]);
  }
  getItems() {
    const cmds = this.app.commands.listCommands();
    const pinned: string[] = this.plugin.options.pinned || [];
    const recent = this.plugin.recent;
    const rank = (c: any) => {
      const p = pinned.indexOf(c.id);
      if (p !== -1) return -1000 + p;
      const r = recent.indexOf(c.id);
      return r === -1 ? 0 : -500 + r;
    };
    return cmds.slice().sort((a: any, b: any) => rank(a) - rank(b) || a.name.localeCompare(b.name));
  }
  getItemText(c: any) {
    return c.name;
  }
  renderSuggestion(m: FuzzyMatch<any>, el: HTMLElement) {
    el.addClass("mod-complex");
    const content = el.createDiv("suggestion-content");
    renderMatches(content.createDiv("suggestion-title"), m.item.name, m.match.matches);
    const aux = el.createDiv("suggestion-aux");
    if ((this.plugin.options.pinned || []).includes(m.item.id)) setIcon(aux.createSpan("suggestion-flair"), "pin");
    for (const h of this.app.hotkeyManager.effective(m.item.id)) aux.createEl("kbd", { cls: "suggestion-hotkey", text: formatHotkey(h) });
  }
  onChooseItem(cmd: any) {
    this.plugin.recent = [cmd.id, ...this.plugin.recent.filter((x: string) => x !== cmd.id)].slice(0, 20);
    this.app.commands.executeCommand(cmd);
  }
}

export class CommandPalettePlugin {
  app: any;
  plugin: any;
  options: any = { pinned: [] };
  recent: string[] = [];
  async onEnable(app: any) {
    this.app = app;
    this.options = await app.readJson("command-palette.json", { pinned: [] });
    app.commands.addCommand({ id: "command-palette:open", name: "Command palette: Open command palette", icon: "terminal-square", hotkeys: [{ modifiers: ["Mod"], key: "P" }], callback: () => new CommandPalette(app, this).open() });
  }
}

export class SwitcherPlugin {
  app: any;
  plugin: any;
  onEnable(app: any) {
    this.app = app;
    app.commands.addCommand({ id: "switcher:open", name: "Quick switcher: Open quick switcher", icon: "file-search", hotkeys: [{ modifiers: ["Mod"], key: "O" }], callback: () => new QuickSwitcher(app).open() });
  }
}

// ── folder picker ─────────────────────────────────────────────────────────

export class FolderSuggestModal extends FuzzySuggestModal<TFolder> {
  constructor(app: any, private onPick: (f: TFolder) => any, placeholder = "Choose a folder...") {
    super(app);
    this.setPlaceholder(placeholder);
  }
  getItems() {
    return this.app.vault.getAllFolders(true);
  }
  getItemText(f: TFolder) {
    return f.isRoot() ? "/" : f.path;
  }
  onChooseItem(f: TFolder) {
    this.onPick(f);
  }
}

export function openMoveModal(app: any, file: TFile | TFolder | (TFile | TFolder)[]) {
  const files = Array.isArray(file) ? file : [file];
  new FolderSuggestModal(app, async (folder) => {
    for (const f of files) {
      if (f === folder || (f instanceof TFolder && (folder.path + "/").startsWith(f.path + "/"))) continue;
      const dest = (folder.isRoot() ? "" : folder.path + "/") + f.name;
      try {
        await app.fileManager.renameFile(f, dest);
      } catch (e: any) {
        new app.Notice(String(e?.message || e));
      }
    }
  }, files.length === 1 ? `Move "${files[0].name}" to...` : `Move ${files.length} items to...`).open();
}
