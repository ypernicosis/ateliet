// Built-in app and editor commands (same ids as Obsidian, so hotkeys.json
// customisations carry over) and the [[link]] / #tag editor suggestions.

import { EditorSuggest, type Editor, type EditorPosition } from "../obsidian/editor.ts";
import { MarkdownView } from "../obsidian/views.ts";
import { TFile } from "../obsidian/vault.ts";
import { parseFrontMatterAliases, prepareFuzzySearch, renderMatches } from "../obsidian/util.ts";
import { setIcon } from "../obsidian/icons.ts";
import { openMoveModal } from "./search.ts";
import { invoke } from "../tauri.ts";

const M = (key: string, ...mods: string[]) => [{ modifiers: ["Mod", ...mods], key }];

function wrap(editor: Editor, before: string, after = before) {
  const sels = editor.listSelections();
  const cm = editor.cm;
  const changes: any[] = [];
  const ranges: any[] = [];
  for (const s of cm.state.selection.ranges) {
    const text = cm.state.sliceDoc(s.from, s.to);
    const pre = cm.state.sliceDoc(s.from - before.length, s.from);
    const post = cm.state.sliceDoc(s.to, s.to + after.length);
    if (pre === before && post === after) {
      changes.push({ from: s.from - before.length, to: s.from, insert: "" }, { from: s.to, to: s.to + after.length, insert: "" });
      ranges.push({ anchor: s.from - before.length, head: s.to - before.length });
    } else if (text.startsWith(before) && text.endsWith(after) && text.length >= before.length + after.length) {
      changes.push({ from: s.from, to: s.to, insert: text.slice(before.length, text.length - after.length) });
      ranges.push({ anchor: s.from, head: s.to - before.length - after.length });
    } else {
      changes.push({ from: s.from, to: s.to, insert: before + text + after });
      ranges.push({ anchor: s.from + before.length, head: s.to + before.length });
    }
  }
  void sels;
  cm.dispatch({ changes, selection: { anchor: ranges[0].anchor, head: ranges[0].head }, userEvent: "input" });
}

function mapLines(editor: Editor, fn: (line: string) => string) {
  const cm = editor.cm;
  const s = cm.state.selection.main;
  const first = cm.state.doc.lineAt(s.from).number;
  const last = cm.state.doc.lineAt(s.to).number;
  const changes = [];
  for (let n = first; n <= last; n++) {
    const l = cm.state.doc.line(n);
    changes.push({ from: l.from, to: l.to, insert: fn(l.text) });
  }
  cm.dispatch({ changes, userEvent: "input" });
}

function setHeading(editor: Editor, level: number) {
  mapLines(editor, (l) => {
    const stripped = l.replace(/^#{1,6}\s+/, "");
    return level ? "#".repeat(level) + " " + stripped : stripped;
  });
}

function swapLine(editor: Editor, dir: -1 | 1) {
  const cm = editor.cm;
  const s = cm.state.selection.main;
  const a = cm.state.doc.lineAt(s.from), b = cm.state.doc.lineAt(s.to);
  if (dir === -1 && a.number === 1) return;
  if (dir === 1 && b.number === cm.state.doc.lines) return;
  const other = cm.state.doc.line(dir === -1 ? a.number - 1 : b.number + 1);
  const block = cm.state.sliceDoc(a.from, b.to);
  if (dir === -1) {
    cm.dispatch({ changes: { from: other.from, to: b.to, insert: block + "\n" + other.text }, selection: { anchor: s.anchor - other.length - 1, head: s.head - other.length - 1 } });
  } else {
    cm.dispatch({ changes: { from: a.from, to: other.to, insert: other.text + "\n" + block }, selection: { anchor: s.anchor + other.length + 1, head: s.head + other.length + 1 } });
  }
}

function linkAtCursor(editor: Editor): string | null {
  const cur = editor.getCursor();
  const line = editor.getLine(cur.line);
  const re = /!?\[\[([^\]]+?)\]\]|\[[^\]]*\]\(([^)]+)\)/g;
  let m;
  while ((m = re.exec(line))) {
    if (cur.ch >= m.index && cur.ch <= m.index + m[0].length) {
      if (m[1]) return m[1].split("|")[0];
      return decodeURI(m[2]);
    }
  }
  return null;
}

export function registerCoreCommands(app: any) {
  const c = (cmd: any) => app.commands.addCommand(cmd);
  const ws = app.workspace;
  const activeMd = () => ws.getActiveViewOfType(MarkdownView) as MarkdownView | null;

  // ── app ──
  c({ id: "app:open-settings", name: "Open settings", icon: "settings", hotkeys: M(","), callback: () => app.setting.open() });
  c({ id: "app:toggle-left-sidebar", name: "Toggle left sidebar", icon: "panel-left", hotkeys: M("\\"), callback: () => ws.leftSplit.toggle() });
  c({ id: "app:toggle-right-sidebar", name: "Toggle right sidebar", icon: "panel-right", hotkeys: M("\\", "Shift"), callback: () => ws.rightSplit.toggle() });
  c({ id: "app:go-back", name: "Navigate back", icon: "arrow-left", hotkeys: M("ArrowLeft", "Alt"), callback: () => ws.activeLeaf?.history.back() });
  c({ id: "app:go-forward", name: "Navigate forward", icon: "arrow-right", hotkeys: M("ArrowRight", "Alt"), callback: () => ws.activeLeaf?.history.forward() });
  c({ id: "app:reload", name: "Reload app without saving", hotkeys: M("R"), callback: () => location.reload() });
  c({ id: "app:open-vault", name: "Open another vault", icon: "vault", callback: () => app.openVaultChooser() });
  c({ id: "app:delete-file", name: "Delete current file", icon: "trash-2", checkCallback: (k: boolean) => {
    const f = ws.getActiveFile();
    if (!f) return false;
    if (!k) app.fileManager.promptForDeletion(f);
    return true;
  } });
  c({ id: "app:toggle-default-new-pane-mode", name: "Toggle reading view by default", callback: () => app.vault.setConfig("defaultViewMode", app.vault.getConfig("defaultViewMode") === "preview" ? "source" : "preview") });
  c({ id: "theme:toggle-light-dark", name: "Toggle light/dark mode", icon: "sun-moon", callback: () => app.vault.setConfig("theme", document.body.hasClass("theme-dark") ? "moonstone" : "obsidian") });
  c({ id: "app:show-debug-info", name: "Show debug info", callback: () => {
    const info = `Ateliet ${app.version}\nPlugins: ${Object.keys(app.plugins.plugins).join(", ")}\nFailed: ${Object.keys(app.plugins.failed).join(", ") || "none"}`;
    navigator.clipboard.writeText(info);
    new app.Notice("Debug info copied to clipboard");
  } });
  c({ id: "app:toggle-devtools", name: "Toggle developer tools", hotkeys: [{ modifiers: ["Mod", "Alt"], key: "I" }], callback: () => invoke("plugin:webview|internal_toggle_devtools").catch(() => new app.Notice("Right-click → Inspect element to open the developer tools.")) });

  // ── workspace ──
  c({ id: "workspace:close", name: "Close current tab", icon: "x", hotkeys: M("W"), callback: () => ws.activeLeaf?.detach() });
  c({ id: "workspace:close-others", name: "Close all other tabs", callback: () => {
    const a = ws.activeLeaf;
    for (const l of (a?.parent?.children || []).slice()) if (l !== a) (l as any).detach();
  } });
  c({ id: "workspace:new-tab", name: "New tab", icon: "plus", hotkeys: M("T"), callback: async () => {
    const leaf = ws.getLeaf("tab");
    await leaf.setViewState({ type: "empty", active: true });
  } });
  c({ id: "workspace:split-vertical", name: "Split right", icon: "separator-vertical", callback: () => ws.activeLeaf && ws.duplicateLeaf(ws.activeLeaf, "split", "vertical") });
  c({ id: "workspace:split-horizontal", name: "Split down", icon: "separator-horizontal", callback: () => ws.activeLeaf && ws.duplicateLeaf(ws.activeLeaf, "split", "horizontal") });
  c({ id: "workspace:toggle-pin", name: "Toggle pin", icon: "pin", callback: () => ws.activeLeaf?.togglePinned() });
  const cycle = (d: number) => () => {
    const leaf = ws.activeLeaf;
    const tabs = leaf?.parent;
    if (!tabs?.children) return;
    const i = tabs.children.indexOf(leaf);
    const next = tabs.children[(i + d + tabs.children.length) % tabs.children.length];
    if (next) ws.setActiveLeaf(next, { focus: true });
  };
  c({ id: "workspace:next-tab", name: "Go to next tab", hotkeys: [{ modifiers: ["Ctrl"], key: "Tab" }], callback: cycle(1) });
  c({ id: "workspace:previous-tab", name: "Go to previous tab", hotkeys: [{ modifiers: ["Ctrl", "Shift"], key: "Tab" }], callback: cycle(-1) });
  for (let i = 1; i <= 8; i++) {
    c({ id: `workspace:goto-tab-${i}`, name: `Go to tab #${i}`, hotkeys: M(String(i)), callback: () => {
      const tabs = ws.activeLeaf?.parent;
      const l = tabs?.children?.[i - 1];
      if (l) ws.setActiveLeaf(l, { focus: true });
    } });
  }
  c({ id: "workspace:goto-last-tab", name: "Go to last tab", hotkeys: M("9"), callback: () => {
    const tabs = ws.activeLeaf?.parent;
    const l = tabs?.children?.[tabs.children.length - 1];
    if (l) ws.setActiveLeaf(l, { focus: true });
  } });
  c({ id: "workspace:copy-path", name: "Copy file path", checkCallback: (k: boolean) => {
    const f = ws.getActiveFile();
    if (!f) return false;
    if (!k) navigator.clipboard.writeText(f.path);
    return true;
  } });
  c({ id: "workspace:copy-url", name: "Copy Obsidian URL", checkCallback: (k: boolean) => {
    const f = ws.getActiveFile();
    if (!f) return false;
    if (!k) navigator.clipboard.writeText(`obsidian://open?vault=${encodeURIComponent(app.vault.getName())}&file=${encodeURIComponent(f.path)}`);
    return true;
  } });
  c({ id: "workspace:edit-file-title", name: "Rename file", hotkeys: [{ modifiers: [], key: "F2" }], checkCallback: (k: boolean) => {
    const v = activeMd();
    if (!v) return false;
    if (!k) {
      v.inlineTitleEl.focus();
      document.getSelection()?.selectAllChildren(v.inlineTitleEl);
    }
    return true;
  } });
  c({ id: "workspace:move-file", name: "Move current file to another folder", checkCallback: (k: boolean) => {
    const f = ws.getActiveFile();
    if (!f) return false;
    if (!k) openMoveModal(app, f);
    return true;
  } });
  c({ id: "workspace:show-trash", name: "Reveal file in system explorer", checkCallback: (k: boolean) => {
    const f = ws.getActiveFile();
    if (!f) return false;
    if (!k) app.showInFolder(f.path);
    return true;
  } });
  c({ id: "open-with-default-app:open", name: "Open in default app", checkCallback: (k: boolean) => {
    const f = ws.getActiveFile();
    if (!f) return false;
    if (!k) app.openWithDefaultApp(f.path);
    return true;
  } });

  // ── markdown view ──
  c({ id: "markdown:toggle-preview", name: "Toggle reading view", icon: "book-open", hotkeys: M("E"), checkCallback: (k: boolean) => {
    const v = activeMd();
    if (!v) return false;
    if (!k) v.setMode(v.getMode() === "preview" ? "source" : "preview");
    return true;
  } });
  c({ id: "editor:toggle-source", name: "Toggle Live Preview/Source mode", icon: "code-2", checkCallback: (k: boolean) => {
    const v = activeMd();
    if (!v) return false;
    if (!k) v.toggleSourceLivePreview();
    return true;
  } });
  c({ id: "editor:save-file", name: "Save current file", hotkeys: M("S"), checkCallback: (k: boolean) => {
    const v = activeMd();
    if (!v) return false;
    if (!k) v.save();
    return true;
  } });
  c({ id: "editor:open-search", name: "Search current file", icon: "search", hotkeys: M("F"), checkCallback: (k: boolean) => {
    const v = activeMd();
    if (!v) return false;
    if (!k) v.showSearch(false);
    return true;
  } });
  c({ id: "editor:open-search-replace", name: "Search & replace in current file", hotkeys: M("H"), checkCallback: (k: boolean) => {
    const v = activeMd();
    if (!v || v.getMode() !== "source") return false;
    if (!k) v.showSearch(true);
    return true;
  } });

  // ── editor ──
  const e = (id: string, name: string, fn: (ed: Editor, v: MarkdownView) => any, hotkeys?: any, icon?: string) =>
    c({ id: `editor:${id}`, name, icon, hotkeys, editorCheckCallback: (k: boolean, ed: Editor, v: MarkdownView) => {
      if (v.getMode?.() === "preview") return false;
      if (!k) fn(ed, v);
      return true;
    } });
  e("toggle-bold", "Toggle bold", (ed) => wrap(ed, "**"), M("B"), "bold");
  e("toggle-italics", "Toggle italic", (ed) => wrap(ed, "*"), M("I"), "italic");
  e("toggle-strikethrough", "Toggle strikethrough", (ed) => wrap(ed, "~~"), undefined, "strikethrough");
  e("toggle-highlight", "Toggle highlight", (ed) => wrap(ed, "=="), undefined, "highlighter");
  e("toggle-code", "Toggle code", (ed) => wrap(ed, "`"), undefined, "code");
  e("toggle-inline-math", "Toggle math", (ed) => wrap(ed, "$"), undefined, "sigma");
  e("toggle-comments", "Toggle comment", (ed) => wrap(ed, "%%"), M("/"), "percent");
  e("insert-link", "Insert Markdown link", (ed) => {
    const sel = ed.getSelection();
    if (/^https?:\/\//.test(sel)) ed.replaceSelection(`[](${sel})`);
    else {
      ed.replaceSelection(`[${sel}]()`);
      const cur = ed.getCursor();
      ed.setCursor({ line: cur.line, ch: cur.ch - 1 });
    }
  }, M("K"), "link");
  e("insert-wikilink", "Add internal link", (ed) => {
    const sel = ed.getSelection();
    ed.replaceSelection(`[[${sel}]]`);
    if (!sel) {
      const cur = ed.getCursor();
      ed.setCursor({ line: cur.line, ch: cur.ch - 2 });
      app.workspace.editorSuggest.trigger(ed, ed.owner?.file);
    }
  }, undefined, "link-2");
  e("insert-embed", "Add embed", (ed) => {
    ed.replaceSelection(`![[${ed.getSelection()}]]`);
  }, undefined, "image");
  e("insert-callout", "Insert callout", (ed) => {
    const sel = ed.getSelection();
    ed.replaceSelection(`> [!note]\n> ${sel.split("\n").join("\n> ")}`);
  }, undefined, "quote");
  e("insert-codeblock", "Insert code block", (ed) => {
    const sel = ed.getSelection();
    ed.replaceSelection("```\n" + sel + "\n```");
  }, undefined, "code-2");
  e("insert-horizontal-rule", "Insert horizontal rule", (ed) => ed.replaceSelection("\n---\n"), undefined, "minus");
  e("insert-table", "Insert table", (ed) => ed.replaceSelection("| Column 1 | Column 2 |\n| -------- | -------- |\n|          |          |\n"), undefined, "table");
  e("toggle-checklist-status", "Toggle checkbox status", (ed) => mapLines(ed, (l) => {
    const m = /^(\s*)([-*+]|\d+[.)])\s+\[(.)\]\s?(.*)$/.exec(l);
    if (m) return `${m[1]}${m[2]} [${m[3] === " " ? "x" : " "}] ${m[4]}`;
    const b = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(l);
    if (b) return `${b[1]}${b[2]} [ ] ${b[3]}`;
    return `- [ ] ${l}`;
  }), M("L"), "check-square");
  e("toggle-bullet-list", "Toggle bullet list", (ed) => mapLines(ed, (l) => (/^\s*[-*+]\s/.test(l) ? l.replace(/^(\s*)[-*+]\s(\[.\]\s)?/, "$1") : l.replace(/^(\s*)(\d+[.)]\s)?/, "$1- "))), undefined, "list");
  e("toggle-numbered-list", "Toggle numbered list", (ed) => {
    let n = 0;
    mapLines(ed, (l) => (/^\s*\d+[.)]\s/.test(l) ? l.replace(/^(\s*)\d+[.)]\s/, "$1") : l.replace(/^(\s*)([-*+]\s)?/, (_m, s) => `${s}${++n}. `)));
  }, undefined, "list-ordered");
  e("toggle-blockquote", "Toggle blockquote", (ed) => mapLines(ed, (l) => (l.startsWith("> ") ? l.slice(2) : l.startsWith(">") ? l.slice(1) : "> " + l)), undefined, "quote");
  for (let i = 0; i <= 6; i++) e(`set-heading${i ? "-" + i : "-0"}`, i ? `Set as heading ${i}` : "Remove heading", (ed) => setHeading(ed, i), undefined, "heading");
  e("swap-line-up", "Move line up", (ed) => swapLine(ed, -1), [{ modifiers: ["Mod", "Shift"], key: "ArrowUp" }]);
  e("swap-line-down", "Move line down", (ed) => swapLine(ed, 1), [{ modifiers: ["Mod", "Shift"], key: "ArrowDown" }]);
  e("delete-paragraph", "Delete paragraph", (ed) => ed.exec("deleteLine"), M("D"));
  e("toggle-fold", "Toggle fold on the current line", (ed) => ed.exec("toggleFold"));
  e("fold-all", "Fold all headings and lists", (ed) => ed.exec("foldAll"));
  e("unfold-all", "Unfold all headings and lists", (ed) => ed.exec("unfoldAll"));
  e("indent-list", "Indent list", (ed) => ed.exec("indentMore"));
  e("unindent-list", "Unindent list", (ed) => ed.exec("indentLess"));
  e("follow-link", "Follow link under cursor", (ed, v) => {
    const l = linkAtCursor(ed);
    if (l) ws.openLinkText(l, v.file?.path || "", false);
  }, [{ modifiers: ["Alt"], key: "Enter" }]);
  e("open-link-in-new-leaf", "Open link under cursor in new tab", (ed, v) => {
    const l = linkAtCursor(ed);
    if (l) ws.openLinkText(l, v.file?.path || "", "tab");
  }, M("Enter"));
  e("add-cursor-above", "Add cursor above", (ed) => {
    const sels = ed.listSelections();
    const top = sels[0].head;
    if (top.line > 0) ed.setSelections([...sels, { anchor: { line: top.line - 1, ch: top.ch } }]);
  });
  e("add-cursor-below", "Add cursor below", (ed) => {
    const sels = ed.listSelections();
    const bottom = sels[sels.length - 1].head;
    if (bottom.line < ed.lastLine()) ed.setSelections([...sels, { anchor: { line: bottom.line + 1, ch: bottom.ch } }]);
  });
  e("cycle-list-checklist", "Cycle bullet/checkbox", (ed) => mapLines(ed, (l) => {
    if (/^\s*[-*+]\s\[.\]\s/.test(l)) return l.replace(/^(\s*)[-*+]\s\[.\]\s/, "$1");
    if (/^\s*[-*+]\s/.test(l)) return l.replace(/^(\s*)([-*+])\s/, "$1$2 [ ] ");
    return l.replace(/^(\s*)/, "$1- ");
  }));
  e("attach-file", "Insert attachment", async (_ed, v) => {
    const input = createEl("input", { type: "file", attr: { multiple: true } }) as HTMLInputElement;
    input.onchange = () => app.insertFiles(Array.from(input.files || []), v);
    input.click();
  }, undefined, "paperclip");
}

// ── [[link]] and #tag suggestions ─────────────────────────────────────────

interface LinkItem {
  file: TFile | null;
  path: string;
  alias?: string;
  heading?: string;
  match: any;
}

export class LinkSuggest extends EditorSuggest<LinkItem> {
  onTrigger(cursor: EditorPosition, editor: Editor): any {
    const line = editor.getLine(cursor.line).slice(0, cursor.ch);
    const m = /(!?)\[\[([^\[\]]*)$/.exec(line);
    if (!m) return null;
    if (editor.getLine(cursor.line).slice(cursor.ch).startsWith("]]") === false && /\]\]/.test(m[2])) return null;
    return { start: { line: cursor.line, ch: m.index + m[1].length + 2 }, end: cursor, query: m[2] };
  }

  getSuggestions(ctx: any): LinkItem[] {
    const q: string = ctx.query;
    const src = ctx.file?.path || "";
    const hashIdx = q.indexOf("#");
    if (hashIdx !== -1) {
      const target = q.slice(0, hashIdx);
      const sub = q.slice(hashIdx + 1).toLowerCase();
      const f = target ? this.app.metadataCache.getFirstLinkpathDest(target, src) : ctx.file;
      if (!f) return [];
      const hs = this.app.metadataCache.getFileCache(f)?.headings || [];
      return hs.filter((h: any) => h.heading.toLowerCase().includes(sub)).map((h: any) => ({ file: f, path: f.path, heading: h.heading, match: { score: 0, matches: [] } }));
    }
    const pipe = q.indexOf("|");
    const query = pipe === -1 ? q : q.slice(0, pipe);
    const items: LinkItem[] = [];
    const fuzzy = query ? prepareFuzzySearch(query) : null;
    for (const f of this.app.vault.getFiles()) {
      const text = f.extension === "md" ? f.path.slice(0, -3) : f.path;
      const m = fuzzy ? fuzzy(text) : { score: -f.path.length * 0.01, matches: [] };
      if (m) {
        const bm = fuzzy ? fuzzy(f.basename) : null;
        items.push({ file: f, path: f.path, match: { score: m.score + (bm ? 3 : 0) + (f.extension === "md" ? 0.5 : 0), matches: m.matches } });
      }
      if (f.extension === "md") {
        for (const a of parseFrontMatterAliases(this.app.metadataCache.getFileCache(f)?.frontmatter) || []) {
          const am = fuzzy ? fuzzy(a) : null;
          if (am) items.push({ file: f, path: f.path, alias: a, match: am });
        }
      }
    }
    if (query) {
      for (const srcPath in this.app.metadataCache.unresolvedLinks) {
        for (const l in this.app.metadataCache.unresolvedLinks[srcPath]) {
          const m = fuzzy!(l);
          if (m && !items.some((i) => !i.file && i.path === l)) items.push({ file: null, path: l, match: { score: m.score - 5, matches: m.matches } });
        }
      }
    }
    items.sort((a, b) => b.match.score - a.match.score);
    return items.slice(0, 50);
  }

  renderSuggestion(it: LinkItem, el: HTMLElement) {
    el.addClass("mod-complex");
    const content = el.createDiv("suggestion-content");
    const title = content.createDiv("suggestion-title");
    if (it.heading) {
      title.setText(it.heading);
      return;
    }
    if (!it.file) {
      title.setText(it.path);
      content.createDiv({ cls: "suggestion-note", text: "Not created yet" });
      return;
    }
    if (it.alias) {
      renderMatches(title, it.alias, it.match.matches);
      content.createDiv({ cls: "suggestion-note", text: it.file.path });
      setIcon(el.createDiv("suggestion-aux").createSpan("suggestion-flair"), "forward");
      return;
    }
    const text = it.file.extension === "md" ? it.file.path.slice(0, -3) : it.file.path;
    renderMatches(title, it.file.basename, it.match.matches, -(text.length - it.file.basename.length));
    if (it.file.parent && !it.file.parent.isRoot()) content.createDiv({ cls: "suggestion-note", text: it.file.parent.path });
    if (it.file.extension !== "md") el.createDiv("suggestion-aux").createSpan({ cls: "suggestion-flair", text: it.file.extension });
  }

  selectSuggestion(it: LinkItem) {
    const ctx = this.context;
    if (!ctx) return;
    const editor: Editor = ctx.editor;
    const src = ctx.file?.path || "";
    let text: string;
    if (it.heading) {
      const q: string = ctx.query;
      text = `${q.slice(0, q.indexOf("#"))}#${it.heading}`;
    } else if (it.file) {
      text = this.app.metadataCache.fileToLinktext(it.file, src, true);
      if (it.alias) text += "|" + it.alias;
    } else text = it.path;
    const end = editor.getLine(ctx.end.line).slice(ctx.end.ch).startsWith("]]") ? { line: ctx.end.line, ch: ctx.end.ch + 2 } : ctx.end;
    editor.replaceRange(text + "]]", ctx.start, end);
    editor.setCursor({ line: ctx.start.line, ch: ctx.start.ch + text.length + 2 });
    this.close();
  }
}

export class TagSuggest extends EditorSuggest<{ tag: string; count: number; match: any }> {
  onTrigger(cursor: EditorPosition, editor: Editor): any {
    const line = editor.getLine(cursor.line).slice(0, cursor.ch);
    const m = /(?:^|\s)#([^\s#!-,.:-@\[-^`{-~]*)$/u.exec(line);
    if (!m) return null;
    if (/^#{1,6}$/.test(line.trim())) return null;
    return { start: { line: cursor.line, ch: cursor.ch - m[1].length - 1 }, end: cursor, query: m[1] };
  }
  getSuggestions(ctx: any) {
    const tags = this.app.metadataCache.getTags();
    const f = ctx.query ? prepareFuzzySearch(ctx.query) : null;
    const out = [];
    for (const t in tags) {
      const m = f ? f(t.slice(1)) : { score: tags[t], matches: [] };
      if (m) out.push({ tag: t, count: tags[t], match: m });
    }
    out.sort((a, b) => b.match.score - a.match.score || b.count - a.count);
    return out.slice(0, 30);
  }
  renderSuggestion(it: any, el: HTMLElement) {
    renderMatches(el.createDiv("suggestion-content"), it.tag.slice(1), it.match.matches);
    el.createDiv("suggestion-aux").createSpan({ cls: "suggestion-flair", text: String(it.count) });
  }
  selectSuggestion(it: any) {
    const ctx = this.context;
    if (!ctx) return;
    ctx.editor.replaceRange(it.tag + " ", ctx.start, ctx.end);
    this.close();
  }
}
