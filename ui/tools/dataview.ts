// Dataview, built in. The index, the query language and dataviewjs run in
// Rust (src/dataview, src/script); this file only renders their results with
// the plugin's DOM and classes, so themes and snippets keep working.

import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder, StateEffect } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { editorInfoField } from "../obsidian/editor.ts";
import { Component } from "../obsidian/events.ts";
import { MarkdownPreviewRenderer, MarkdownRenderer } from "../obsidian/renderer.ts";
import { Setting } from "../obsidian/ui.ts";
import { debounce } from "../obsidian/util.ts";
import { TFile } from "../obsidian/vault.ts";
import { invoke } from "../tauri.ts";
import { loadToolSettings, locale, saveToolSettings, vaultPath } from "./common.ts";

export const DATAVIEW_DEFAULTS = {
  renderNullAs: "\\-",
  warnOnEmptyResult: true,
  refreshEnabled: true,
  refreshInterval: 2500,
  defaultDateFormat: "MMMM dd, yyyy",
  defaultDateTimeFormat: "h:mm a - MMMM dd, yyyy",
  tableIdColumnName: "File",
  tableGroupColumnName: "Group",
  showResultCount: true,
  allowHtml: true,
  inlineQueryPrefix: "=",
  inlineJsQueryPrefix: "$=",
  inlineQueriesInCodeblocks: true,
  prettyRenderInlineFields: true,
  prettyRenderInlineFieldsInLivePreview: true,
  enableInlineDataview: true,
  enableDataviewJs: true,
  enableInlineDataviewJs: true,
  dataviewJsKeyword: "dataviewjs",
};

type Options = typeof DATAVIEW_DEFAULTS;

interface LiveBlock {
  el: HTMLElement;
  run: () => Promise<void>;
}

const refreshEffect = StateEffect.define<null>();
const SAFE_TAGS = new Set(["div", "span", "p", "a", "b", "i", "em", "strong", "u", "s", "small", "mark", "code", "pre", "blockquote", "ul", "ol", "li", "table", "thead", "tbody", "tfoot", "tr", "td", "th", "h1", "h2", "h3", "h4", "h5", "h6", "img", "br", "hr", "details", "summary", "progress", "meter", "label", "sup", "sub", "figure", "figcaption", "button", "input", "svg", "path", "kbd", "abbr", "time", "caption", "colgroup", "col", "section", "article", "header", "footer", "nav"]);
const IMAGE = /\.(png|jpe?g|gif|bmp|svg|webp|avif)$/i;

/** `[key:: value]` and `(key:: value)` in a line of text (the rules of src/dataview/index.rs). */
export function findInlineFields(line: string): { start: number; end: number; key: string; value: string; paren: boolean }[] {
  const out: { start: number; end: number; key: string; value: string; paren: boolean }[] = [];
  let i = 0;
  while (i < line.length) {
    const open = line[i];
    if ((open === "[" || open === "(") && !(open === "[" && (line[i + 1] === "[" || line[i - 1] === "["))) {
      const close = open === "[" ? "]" : ")";
      let depth = 0;
      let end = -1;
      for (let j = i; j < line.length; j++) {
        if (line[j] === open) depth++;
        else if (line[j] === close && --depth === 0) {
          end = j;
          break;
        }
      }
      if (end > 0) {
        const inner = line.slice(i + 1, end);
        const sep = inner.indexOf("::");
        if (sep > 0) {
          const key = inner.slice(0, sep).trim().replace(/^[*_~=]+|[*_~=]+$/g, "").trim();
          if (key && key.length <= 150 && !/[\[\]()`|#]/.test(key) && !key.includes("://")) {
            out.push({ start: i, end: end + 1, key, value: inner.slice(sep + 2).trim(), paren: open === "(" });
            i = end + 1;
            continue;
          }
        }
      }
    }
    i++;
  }
  return out;
}

export class DataviewTool {
  app: any;
  plugin: any;
  options: Options = { ...DATAVIEW_DEFAULTS };
  private live = new Set<LiveBlock>();
  private views = new Set<EditorView>();
  private cleanups: (() => void)[] = [];

  async onEnable(app: any) {
    this.app = app;
    this.options = await loadToolSettings(app, "dataview", "dataview", DATAVIEW_DEFAULTS);
    const block = (lang: string, handler: (src: string, el: HTMLElement, ctx: any) => Promise<void>) => {
      MarkdownPreviewRenderer.registerCodeBlock(lang, MarkdownPreviewRenderer.createCodeBlockPostProcessor(lang, handler));
      this.cleanups.push(() => MarkdownPreviewRenderer.unregisterCodeBlock(lang));
    };
    block("dataview", (src, el, ctx) => this.liveBlock(el, () => this.query(src, ctx.sourcePath), ctx.sourcePath));
    if (this.options.enableDataviewJs) {
      block(this.options.dataviewJsKeyword || "dataviewjs", (src, el, ctx) => this.liveBlock(el, () => this.js(src, ctx.sourcePath, false), ctx.sourcePath));
    }
    const pp = (el: HTMLElement, ctx: any) => this.inlineInReading(el, ctx.sourcePath);
    MarkdownPreviewRenderer.registerPostProcessor(pp, 100);
    this.cleanups.push(() => MarkdownPreviewRenderer.unregisterPostProcessor(pp));
    if (this.options.enableInlineDataview) {
      const ext = this.editorExtension();
      app.workspace.registerEditorExtension(ext);
      this.cleanups.push(() => app.workspace.unregisterEditorExtension(ext));
    }
    const refresh = debounce(() => this.refresh(), Math.max(500, Number(this.options.refreshInterval) || 2500), true);
    const onChange = () => this.options.refreshEnabled && refresh();
    for (const [src, ev] of [[app.metadataCache, "changed"], [app.vault, "create"], [app.vault, "delete"], [app.vault, "rename"]] as const) {
      const ref = (src as any).on(ev, onChange);
      this.cleanups.push(() => (src as any).offref(ref));
    }
    app.commands.addCommand({ id: "dataview:dataview-force-refresh-views", name: "Dataview: Force refresh all views and blocks", icon: "refresh-cw", callback: () => this.refresh() });
  }

  onDisable() {
    this.cleanups.forEach((f) => f());
    this.cleanups = [];
    this.live.clear();
  }

  private settings() {
    const o = this.options;
    return {
      dateFormat: o.defaultDateFormat,
      dateTimeFormat: o.defaultDateTimeFormat,
      renderNullAs: o.renderNullAs,
      tableIdColumnName: o.tableIdColumnName,
      tableGroupColumnName: o.tableGroupColumnName,
      locale: locale(),
    };
  }

  private nullText(): string {
    return (this.options.renderNullAs ?? "\\-").replace(/\\(.)/g, "$1");
  }

  // ── running ──────────────────────────────────────────────────────────────

  query(source: string, origin: string): Promise<any> {
    return invoke("dv_query", { vault: vaultPath(this.app), origin, source, settings: this.settings() });
  }

  js(code: string, origin: string, inline: boolean): Promise<any> {
    return invoke("dv_js", { vault: vaultPath(this.app), origin, code, inline, settings: this.settings() });
  }

  inline(expr: string, origin: string): Promise<any> {
    return invoke("dv_inline", { vault: vaultPath(this.app), origin, expr, settings: this.settings() });
  }

  private async liveBlock(el: HTMLElement, run: () => Promise<any>, sourcePath: string) {
    el.addClass("dataview-container");
    let component = new Component();
    component.load();
    const entry: LiveBlock = {
      el,
      run: async () => {
        let res: any;
        try {
          res = await run();
        } catch (e: any) {
          res = { type: "error", error: String(e?.message || e) };
        }
        const next = new Component();
        next.load();
        const holder = createDiv();
        await this.render(res, holder, sourcePath, next);
        el.empty();
        while (holder.firstChild) el.appendChild(holder.firstChild);
        component.unload();
        component = next;
      },
    };
    this.live.add(entry);
    await entry.run();
  }

  refresh() {
    for (const b of Array.from(this.live)) {
      if (!b.el.isConnected) {
        this.live.delete(b);
        continue;
      }
      b.run();
    }
    for (const v of this.views) v.dispatch({ effects: refreshEffect.of(null) });
  }

  // ── rendering ────────────────────────────────────────────────────────────

  async render(res: any, el: HTMLElement, sourcePath: string, component: Component): Promise<void> {
    if (!res) return;
    switch (res.type) {
      case "error": {
        el.createEl("pre", { cls: "dataview dataview-error", text: `Dataview: ${res.error}` });
        return;
      }
      case "table":
        return this.renderTable(res, el, sourcePath, component);
      case "list":
        return this.renderList(res, el, sourcePath, component);
      case "task":
        return this.renderTasks(res, el, sourcePath, component);
      case "calendar":
        return this.renderCalendar(res, el);
      case "js":
        for (const c of res.root?.children || []) await this.renderNode(c, el, sourcePath, component);
        return;
      case "css": {
        el.createEl("style", { text: res.css });
        return;
      }
      case "value":
        return this.renderValue(res.value, el, sourcePath, component, true);
    }
  }

  private empty(el: HTMLElement, kind: string): boolean {
    if (!this.options.warnOnEmptyResult) return false;
    el.createDiv("dataview dataview-error-box").createEl("p", { cls: "dataview dataview-error-message", text: `Dataview: No results to show for ${kind} query.` });
    return true;
  }

  private async renderTable(res: any, el: HTMLElement, sourcePath: string, component: Component) {
    if (!res.rows.length && this.empty(el, "table")) return;
    const table = el.createEl("table", { cls: "dataview table-view-table" });
    const head = table.createEl("thead", { cls: "table-view-thead" }).createEl("tr", { cls: "table-view-tr-header" });
    res.headers.forEach((h: string, i: number) => {
      const th = head.createEl("th", { cls: "table-view-th" });
      const span = th.createSpan({ cls: "dataview table-view-header", text: h });
      if (i === 0 && this.options.showResultCount) span.createSpan({ cls: "dataview small-text", text: String(res.rows.length) });
    });
    const body = table.createEl("tbody", { cls: "table-view-tbody" });
    for (const row of res.rows) {
      const tr = body.createEl("tr");
      for (const cell of row) await this.renderValue(cell, tr.createEl("td"), sourcePath, component, false);
    }
  }

  private async renderList(res: any, el: HTMLElement, sourcePath: string, component: Component) {
    if (!res.items.length && this.empty(el, "list")) return;
    const ul = el.createEl("ul", { cls: "dataview list-view-ul" });
    for (const it of res.items) {
      const li = ul.createEl("li");
      if (it.id !== null && it.id !== undefined) {
        await this.renderValue(it.id, li, sourcePath, component, true);
        if (it.value !== null && it.value !== undefined) {
          if (Array.isArray(it.value)) await this.renderValue(it.value, li, sourcePath, component, false);
          else {
            li.appendText(": ");
            await this.renderValue(it.value, li, sourcePath, component, true);
          }
        }
      } else await this.renderValue(it.value, li, sourcePath, component, true);
    }
  }

  private async renderTasks(res: any, el: HTMLElement, sourcePath: string, component: Component) {
    const total = res.groups.reduce((n: number, g: any) => n + g.tasks.length, 0);
    if (!total && this.empty(el, "task")) return;
    for (const g of res.groups) {
      if (!g.tasks.length) continue;
      if (g.key !== null && g.key !== undefined) {
        const h = el.createEl("h4", { cls: "dataview-result-group" });
        await this.renderValue(g.key, h, sourcePath, component, true);
        if (this.options.showResultCount) h.createSpan({ cls: "dataview small-text", text: String(g.tasks.length) });
      }
      await this.renderTaskList(g.tasks, el, sourcePath, component);
    }
  }

  private async renderTaskList(tasks: any[], el: HTMLElement, sourcePath: string, component: Component) {
    const ul = el.createEl("ul", { cls: "contains-task-list dataview" });
    for (const t of tasks) {
      const li = ul.createEl("li", { cls: t.task ? "dataview task-list-item" : "dataview" });
      if (t.task) {
        const checked = t.status && t.status !== " ";
        li.setAttribute("data-task", t.status || " ");
        if (checked) li.addClass("is-checked");
        const box = li.createEl("input", { cls: "task-list-item-checkbox", attr: { type: "checkbox" } });
        box.checked = !!checked;
        box.addEventListener("click", (e) => {
          e.preventDefault();
          this.toggleTask(t.path, t.line, !!checked);
        });
      }
      const text = li.createSpan();
      await this.renderMarkdown(String(t.text || ""), text, t.path || sourcePath, component, true);
      if (t.children?.length) await this.renderTaskList(t.children, li, sourcePath, component);
    }
  }

  private async toggleTask(path: string, line: number, checked: boolean) {
    const file = this.app.vault.getAbstractFileByPath(path);
    if (!(file instanceof TFile)) return;
    await this.app.vault.process(file, (text: string) => {
      const lines = text.split("\n");
      const l = lines[line];
      if (l === undefined) return text;
      lines[line] = l.replace(/^(\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+\[)(.)(\])/, (_m, a, _c, b) => a + (checked ? " " : "x") + b);
      return lines.join("\n");
    });
  }

  private renderCalendar(res: any, el: HTMLElement) {
    if (!res.days.length && this.empty(el, "calendar")) return;
    const byDay = new Map<string, any[]>();
    for (const d of res.days) {
      const k = String(d.date?.iso || "").slice(0, 10);
      (byDay.get(k) || byDay.set(k, []).get(k)!).push(d.link);
    }
    const months = Array.from(new Set(Array.from(byDay.keys()).map((k) => k.slice(0, 7)))).sort();
    for (const ym of months) {
      const [y, m] = ym.split("-").map(Number);
      const box = el.createDiv("dataview dataview-calendar");
      box.createEl("h4", { text: new Date(y, m - 1, 1).toLocaleDateString(locale(), { month: "long", year: "numeric" }) });
      const grid = box.createEl("table", { cls: "dataview-calendar-grid" });
      const first = new Date(y, m - 1, 1);
      const days = new Date(y, m, 0).getDate();
      let row = grid.createEl("tr");
      for (let i = 0; i < first.getDay(); i++) row.createEl("td");
      for (let d = 1; d <= days; d++) {
        if ((first.getDay() + d - 1) % 7 === 0 && d > 1) row = grid.createEl("tr");
        const key = `${ym}-${String(d).padStart(2, "0")}`;
        const td = row.createEl("td", { text: String(d) });
        const links = byDay.get(key);
        if (links) {
          td.addClass("has-entries");
          for (const l of links) {
            const a = td.createEl("a", { cls: "internal-link dataview-calendar-dot", attr: { "data-href": l.path, href: l.path, "aria-label": l.display } });
            a.setText("•");
          }
        }
      }
    }
  }

  private async renderMarkdown(text: string, el: HTMLElement, sourcePath: string, component: Component, inline: boolean) {
    if (!/[\[\]*_`#=~<>$|\\]|https?:|\n/.test(text)) {
      el.appendText(text);
      return;
    }
    const holder = el.createSpan();
    await MarkdownRenderer.render(this.app, text, holder, sourcePath, component);
    holder.removeClass("markdown-rendered");
    if (inline && holder.childElementCount === 1 && holder.firstElementChild?.tagName === "P") {
      const p = holder.firstElementChild;
      while (p.firstChild) holder.insertBefore(p.firstChild, p);
      p.remove();
    }
  }

  async renderValue(v: any, el: HTMLElement, sourcePath: string, component: Component, inline: boolean): Promise<void> {
    if (v === null || v === undefined) {
      el.appendText(this.nullText());
      return;
    }
    if (typeof v === "string") return this.renderMarkdown(v, el, sourcePath, component, true);
    if (typeof v === "number" || typeof v === "boolean") {
      el.appendText(String(v));
      return;
    }
    if (Array.isArray(v)) {
      if (!v.length) return;
      if (inline) {
        for (let i = 0; i < v.length; i++) {
          if (i) el.appendText(", ");
          await this.renderValue(v[i], el, sourcePath, component, true);
        }
        return;
      }
      const ul = el.createEl("ul", { cls: "dataview dataview-ul dataview-result-list-ul" });
      for (const x of v) await this.renderValue(x, ul.createEl("li", { cls: "dataview-result-list-li" }), sourcePath, component, true);
      return;
    }
    switch (v.t) {
      case "link": {
        const target = v.path + (v.subpath ? "#" + v.subpath : "");
        if (v.embed && IMAGE.test(v.path)) {
          const f = this.app.vault.getAbstractFileByPath(v.path);
          if (f instanceof TFile) {
            el.createEl("img", { attr: { src: this.app.vault.getResourcePath(f), alt: v.display || "" } });
            return;
          }
        }
        const a = el.createEl("a", { cls: "internal-link", text: v.display, attr: { "data-href": target, href: target, target: "_blank", rel: "noopener" } });
        if (!this.app.vault.getAbstractFileByPath(v.path)) a.addClass("is-unresolved");
        return;
      }
      case "date":
      case "dur":
        el.appendText(v.text);
        return;
      case "obj": {
        const ul = el.createEl("ul", { cls: "dataview dataview-ul dataview-result-object-ul" });
        for (const [k, x] of v.entries) {
          const li = ul.createEl("li", { cls: "dataview dataview-li dataview-result-object-li" });
          li.appendText(k + ": ");
          await this.renderValue(x, li, sourcePath, component, true);
        }
        return;
      }
    }
    el.appendText(String(v));
  }

  /** An element recorded by dv.el / dv.container in the script engine. */
  private async renderNode(node: any, parent: HTMLElement, sourcePath: string, component: Component): Promise<void> {
    if (typeof node === "string") {
      parent.appendText(node);
      return;
    }
    if (node?.op) {
      await this.render(node.op, parent, sourcePath, component);
      return;
    }
    const n = node?.el;
    if (!n) return;
    const tag = SAFE_TAGS.has(String(n.tag).toLowerCase()) ? String(n.tag).toLowerCase() : "div";
    const el = parent.createEl(tag as any);
    for (const c of n.cls || []) el.addClass(c);
    for (const [k, val] of Object.entries(n.attrs || {})) {
      if (/^on/i.test(k) || (/^(href|src)$/i.test(k) && /^\s*javascript:/i.test(String(val)))) continue;
      el.setAttribute(k, String(val));
    }
    for (const [k, val] of Object.entries(n.style || {})) (el.style as any)[k] = val;
    if (n.html !== undefined && n.html !== null && this.options.allowHtml) el.innerHTML = n.html;
    if (n.text !== undefined && n.text !== null) {
      if (typeof n.text === "string") el.setText(n.text);
      else await this.renderValue(n.text, el, sourcePath, component, true);
    }
    if (n.markdown !== undefined && n.markdown !== null) {
      if (typeof n.markdown === "string") await this.renderMarkdown(n.markdown, el, sourcePath, component, tag !== "p" && tag !== "div");
      else await this.renderValue(n.markdown, el, sourcePath, component, tag === "span");
    }
    for (const c of n.children || []) await this.renderNode(c, el, sourcePath, component);
  }

  // ── inline queries ───────────────────────────────────────────────────────

  private inlineKind(text: string): { js: boolean; expr: string } | null {
    const js = this.options.inlineJsQueryPrefix || "$=";
    const dql = this.options.inlineQueryPrefix || "=";
    if (this.options.enableInlineDataviewJs && this.options.enableDataviewJs && text.startsWith(js)) return { js: true, expr: text.slice(js.length).trim() };
    if (this.options.enableInlineDataview && text.startsWith(dql)) return { js: false, expr: text.slice(dql.length).trim() };
    return null;
  }

  private async renderInline(kind: { js: boolean; expr: string }, el: HTMLElement, sourcePath: string, component: Component) {
    let res: any;
    try {
      res = kind.js ? await this.js(kind.expr, sourcePath, true) : await this.inline(kind.expr, sourcePath);
    } catch (e: any) {
      res = { type: "error", error: String(e?.message || e) };
    }
    el.empty();
    if (res.type === "error") {
      el.addClass("dataview-error");
      el.setText(`Dataview (inline field '${kind.expr}'): ${res.error.split("\n")[0]}`);
      return;
    }
    if (res.type === "js") {
      for (const c of res.root?.children || []) {
        const child = c?.el;
        // inline results are spans: render their content without the wrapper
        if (child && child.tag === "span" && !child.children?.length && child.markdown !== undefined) {
          if (typeof child.markdown === "string") await this.renderMarkdown(child.markdown, el, sourcePath, component, true);
          else await this.renderValue(child.markdown, el, sourcePath, component, true);
        } else await this.renderNode(c, el, sourcePath, component);
      }
      return;
    }
    await this.renderValue(res.value, el, sourcePath, component, true);
  }

  /** Inline fields in reading view: key and value in their own spans, as the plugin shows them. */
  private prettyFields(el: HTMLElement) {
    if (!this.options.prettyRenderInlineFields) return;
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => ((n as Text).data.includes("::") && !(n.parentElement?.closest("code, pre, .dataview.inline-field")) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
    });
    const nodes: Text[] = [];
    while (walker.nextNode()) nodes.push(walker.currentNode as Text);
    for (const node of nodes) {
      const text = node.data;
      const found = findInlineFields(text);
      if (!found.length) continue;
      const frag = document.createDocumentFragment();
      let at = 0;
      for (const f of found) {
        frag.append(text.slice(at, f.start));
        frag.append(this.fieldEl(f.key, f.value, f.paren));
        at = f.end;
      }
      frag.append(text.slice(at));
      node.replaceWith(frag);
    }
  }

  fieldEl(key: string, value: string, paren: boolean): HTMLElement {
    const span = createSpan({ cls: "dataview inline-field" });
    if (paren) span.createSpan({ cls: "dataview inline-field-standalone-value", text: value });
    else {
      span.createSpan({ cls: "dataview inline-field-key", text: key, attr: { "data-dv-key": key, "data-dv-norm": key.toLowerCase().replace(/\s+/g, "-") } });
      span.createSpan({ cls: "dataview inline-field-value", text: value, attr: { id: "dataview-inline-field-" + key } });
    }
    return span;
  }

  private inlineInReading(el: HTMLElement, sourcePath: string) {
    this.prettyFields(el);
    for (const code of Array.from(el.querySelectorAll<HTMLElement>("code"))) {
      if (code.closest("pre") && !this.options.inlineQueriesInCodeblocks) continue;
      if (code.parentElement?.tagName === "PRE") continue;
      const kind = this.inlineKind((code.textContent || "").trim());
      if (!kind) continue;
      const span = createSpan({ cls: "dataview dataview-inline-query" });
      code.replaceWith(span);
      const component = new Component();
      component.load();
      const entry: LiveBlock = { el: span, run: () => this.renderInline(kind, span, sourcePath, component) };
      this.live.add(entry);
      entry.run();
    }
  }

  private editorExtension() {
    const tool = this;
    class InlineWidget extends WidgetType {
      constructor(readonly kind: { js: boolean; expr: string }, readonly sourcePath: string, readonly gen: number) {
        super();
      }
      eq(o: InlineWidget) {
        return o.kind.expr === this.kind.expr && o.kind.js === this.kind.js && o.gen === this.gen && o.sourcePath === this.sourcePath;
      }
      toDOM() {
        const span = document.createElement("span");
        span.className = "dataview dataview-inline-query cm-dataview-inline";
        const c = new Component();
        c.load();
        tool.renderInline(this.kind, span, this.sourcePath, c);
        return span;
      }
      ignoreEvent() {
        return false;
      }
    }
    class FieldWidget extends WidgetType {
      constructor(readonly key: string, readonly value: string, readonly paren: boolean) {
        super();
      }
      eq(o: FieldWidget) {
        return o.key === this.key && o.value === this.value && o.paren === this.paren;
      }
      toDOM() {
        return tool.fieldEl(this.key, this.value, this.paren);
      }
      ignoreEvent() {
        return false;
      }
    }
    let gen = 0;
    const build = (view: EditorView): DecorationSet => {
      const b = new RangeSetBuilder<Decoration>();
      const info = view.state.field(editorInfoField, false);
      const sourcePath = info?.file?.path || "";
      const sel = view.state.selection;
      const tree = syntaxTree(view.state);
      const found: { from: number; to: number; deco: Decoration }[] = [];
      for (const { from, to } of view.visibleRanges) {
        const text = view.state.doc.sliceString(from, to);
        const re = /(`+)([\s\S]*?[^`])\1(?!`)/g;
        let m: RegExpExecArray | null;
        while ((m = re.exec(text))) {
          const start = from + m.index;
          const end = start + m[0].length;
          const kind = tool.inlineKind(m[2].trim());
          if (!kind) continue;
          let inCode = false;
          for (let n: any = tree.resolveInner(start + 1, 1); n; n = n.parent) {
            if (n.name === "FencedCode" || n.name === "CodeBlock" || n.name === "Frontmatter") {
              inCode = true;
              break;
            }
          }
          if (inCode) continue;
          if (sel.ranges.some((r) => r.from <= end && r.to >= start)) continue;
          found.push({ from: start, to: end, deco: Decoration.replace({ widget: new InlineWidget(kind, sourcePath, gen) }) });
        }
        if (tool.options.prettyRenderInlineFieldsInLivePreview) {
          for (let pos = from; pos <= to; ) {
            const line = view.state.doc.lineAt(pos);
            if (line.text.includes("::")) {
              for (const f of findInlineFields(line.text)) {
                const a = line.from + f.start, z = line.from + f.end;
                if (sel.ranges.some((r) => r.from <= z && r.to >= a)) continue;
                let skip = false;
                for (let n: any = tree.resolveInner(a + 1, 1); n; n = n.parent) {
                  if (/^(FencedCode|CodeBlock|Frontmatter|InlineCode|InlineCodeText|Comment)/.test(n.name)) {
                    skip = true;
                    break;
                  }
                }
                if (!skip) found.push({ from: a, to: z, deco: Decoration.replace({ widget: new FieldWidget(f.key, f.value, f.paren) }) });
              }
            }
            pos = line.to + 1;
          }
        }
      }
      found.sort((x, y) => x.from - y.from || x.to - y.to);
      let last = -1;
      for (const f of found) {
        if (f.from < last) continue;
        b.add(f.from, f.to, f.deco);
        last = f.to;
      }
      return b.finish();
    };
    return ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(readonly view: EditorView) {
          tool.views.add(view);
          this.decorations = view.dom.closest(".is-live-preview") || view.dom.classList.contains("is-live-preview") ? build(view) : Decoration.none;
        }
        update(u: ViewUpdate) {
          const refreshed = u.transactions.some((t) => t.effects.some((e) => e.is(refreshEffect)));
          if (refreshed) gen++;
          if (u.docChanged || u.selectionSet || u.viewportChanged || refreshed || u.geometryChanged) {
            const live = u.view.dom.classList.contains("is-live-preview");
            this.decorations = live ? build(u.view) : Decoration.none;
          }
        }
        destroy() {
          tool.views.delete(this.view);
        }
      },
      { decorations: (v) => v.decorations },
    );
  }

  // ── settings tab ─────────────────────────────────────────────────────────

  settingsTab() {
    return (el: HTMLElement) => {
      const o = this.options;
      const save = () => saveToolSettings(this.app, "dataview", o);
      el.createDiv({ cls: "setting-item-description", text: "Dataview is built into Ateliet: queries, inline queries and dataviewjs run in the app's core. Settings are read from the old plugin the first time." });
      new Setting(el).setName("Date format").setDesc("Luxon tokens, e.g. yyyy-MM-dd.").addText((t) => t.setValue(o.defaultDateFormat).onChange((v) => { o.defaultDateFormat = v; save(); }));
      new Setting(el).setName("Date and time format").addText((t) => t.setValue(o.defaultDateTimeFormat).onChange((v) => { o.defaultDateTimeFormat = v; save(); }));
      new Setting(el).setName("Empty value").setDesc("What empty fields show.").addText((t) => t.setValue(o.renderNullAs).onChange((v) => { o.renderNullAs = v; save(); }));
      new Setting(el).setName("Warn on empty result").addToggle((t) => t.setValue(o.warnOnEmptyResult).onChange((v) => { o.warnOnEmptyResult = v; save(); }));
      new Setting(el).setName("Show result count").addToggle((t) => t.setValue(o.showResultCount).onChange((v) => { o.showResultCount = v; save(); }));
      new Setting(el).setName("Automatic refresh").setDesc("Re-run queries when notes change.").addToggle((t) => t.setValue(o.refreshEnabled).onChange((v) => { o.refreshEnabled = v; save(); }));
      new Setting(el).setName("Refresh interval").setDesc("Milliseconds to wait after a change.").addText((t) => t.setValue(String(o.refreshInterval)).onChange((v) => { o.refreshInterval = Number(v) || 2500; save(); }));
      new Setting(el).setName("File column name").addText((t) => t.setValue(o.tableIdColumnName).onChange((v) => { o.tableIdColumnName = v; save(); }));
      new Setting(el).setName("Group column name").addText((t) => t.setValue(o.tableGroupColumnName).onChange((v) => { o.tableGroupColumnName = v; save(); }));
      new Setting(el).setName("Inline queries").setDesc(`\`${o.inlineQueryPrefix} expression\` in inline code.`).addToggle((t) => t.setValue(o.enableInlineDataview).onChange((v) => { o.enableInlineDataview = v; save(); }));
      new Setting(el).setName("JavaScript queries").setDesc("dataviewjs blocks and inline $= queries.").addToggle((t) => t.setValue(o.enableDataviewJs).onChange((v) => { o.enableDataviewJs = v; save(); }));
      new Setting(el).setName("HTML in results").addToggle((t) => t.setValue(o.allowHtml).onChange((v) => { o.allowHtml = v; save(); }));
    };
  }
}
