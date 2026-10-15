// Line classes (both modes) and Live Preview (formatting hidden away from the
// cursor, checkbox/bullet/image widgets, clickable links).

import { syntaxTree } from "@codemirror/language";
import { EditorSelection, Facet, Prec, RangeSetBuilder, StateField, type EditorState, type Extension, type Range } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";

export interface LivePreviewHost {
  openLink(linktext: string, evt: MouseEvent): void;
  /** render markdown (a callout, a table, a code block with a processor…) into `el`; the returned function frees what the rendering keeps alive */
  renderMarkdown?(markdown: string, el: HTMLElement): (() => void) | void;
  /** true when a code block in this language renders as something else in reading view (a code block processor, mermaid) */
  hasCodeBlockRenderer?(language: string): boolean;
  /** typeset TeX */
  renderMath?(source: string, display: boolean): HTMLElement;
  /** render the note's properties into `el`; returns false if properties are shown as source */
  renderProperties?(el: HTMLElement): boolean;
  resolveEmbed(linktext: string): { url: string; kind: "image" | "audio" | "video" | "pdf" | "note" | "other"; render?: (el: HTMLElement) => void } | null;
  isLinkResolved(linktext: string): boolean;
}

export const livePreviewHost = Facet.define<LivePreviewHost | null, LivePreviewHost | null>({
  combine: (v) => v.find(Boolean) ?? null,
});

/** Mirrors Obsidian's `editorLivePreviewField`. */
export const editorLivePreviewField = StateField.define<boolean>({
  create: () => false,
  update: (v) => v,
});

// ── line classes ──────────────────────────────────────────────────────────

const lineDeco = (cls: string) => Decoration.line({ class: cls });

function buildLineClasses(view: EditorView): DecorationSet {
  const b = new RangeSetBuilder<Decoration>();
  const doc = view.state.doc;
  const lines = new Map<number, string[]>();
  const add = (pos: number, cls: string) => {
    const l = doc.lineAt(pos).from;
    (lines.get(l) || lines.set(l, []).get(l)!).push(cls);
  };
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(view.state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        const h = /^(ATX|Setext)Heading(\d)$/.exec(name);
        if (h) {
          add(node.from, `HyperMD-header HyperMD-header-${h[2]}`);
          return false;
        }
        if (name === "Blockquote") {
          for (let p = node.from; p <= node.to; ) {
            const line = doc.lineAt(p);
            add(line.from, "HyperMD-quote HyperMD-quote-1");
            p = line.to + 1;
          }
        } else if (name === "FencedCode" || name === "CodeBlock") {
          const first = doc.lineAt(node.from).number;
          const last = doc.lineAt(node.to).number;
          for (let n = first; n <= last; n++) {
            const line = doc.line(n);
            let cls = "HyperMD-codeblock HyperMD-codeblock-bg";
            if (n === first) cls += " HyperMD-codeblock-begin HyperMD-codeblock-begin-bg";
            if (n === last) cls += " HyperMD-codeblock-end HyperMD-codeblock-end-bg";
            add(line.from, cls);
          }
          return false;
        } else if (name === "ListItem") {
          let depth = 0;
          for (let p = node.node.parent; p; p = p.parent) if (p.name === "BulletList" || p.name === "OrderedList") depth++;
          add(node.from, `HyperMD-list-line HyperMD-list-line-${depth}`);
        } else if (name === "Frontmatter") {
          const first = doc.lineAt(node.from).number;
          const last = doc.lineAt(node.to).number;
          for (let n = first; n <= last; n++) add(doc.line(n).from, "HyperMD-frontmatter" + (n === first ? " HyperMD-frontmatter-begin" : "") + (n === last ? " HyperMD-frontmatter-end" : ""));
          return false;
        } else if (name === "HorizontalRule") {
          add(node.from, "hr");
        } else if (name === "Table") {
          const first = doc.lineAt(node.from).number;
          const last = doc.lineAt(node.to).number;
          for (let n = first; n <= last; n++) add(doc.line(n).from, "HyperMD-table-row");
        }
      },
    });
  }
  for (const pos of Array.from(lines.keys()).sort((a, b) => a - b)) b.add(pos, pos, lineDeco(lines.get(pos)!.join(" ")));
  return b.finish();
}

export const lineClasses = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    tree: any;
    constructor(view: EditorView) {
      this.decorations = buildLineClasses(view);
      this.tree = syntaxTree(view.state);
    }
    update(u: ViewUpdate) {
      const tree = syntaxTree(u.state);
      if (u.docChanged || u.viewportChanged || tree !== this.tree) {
        this.tree = tree;
        this.decorations = buildLineClasses(u.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

// ── widgets ───────────────────────────────────────────────────────────────

class CheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean, readonly char: string) {
    super();
  }
  // Position is read from the DOM on click, so a shifted checkbox keeps its DOM.
  eq(o: CheckboxWidget) {
    return o.checked === this.checked && o.char === this.char;
  }
  toDOM(view: EditorView) {
    const wrap = document.createElement("label");
    wrap.className = "task-list-label";
    const input = document.createElement("input");
    input.type = "checkbox";
    input.className = "task-list-item-checkbox";
    input.checked = this.checked;
    input.setAttribute("data-task", this.char);
    input.addEventListener("mousedown", (e) => e.preventDefault());
    input.addEventListener("click", (e) => {
      e.preventDefault();
      const start = view.posAtDOM(wrap);
      const at = view.state.doc.sliceString(start, start + 40).indexOf("[") + start + 1; // inside "[ ]"
      const next = this.checked ? " " : "x";
      view.dispatch({ changes: { from: at, to: at + 1, insert: next }, userEvent: "input" });
    });
    wrap.appendChild(input);
    return wrap;
  }
  ignoreEvent() {
    return false;
  }
}

class BulletWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "list-bullet";
    s.textContent = "•";
    return s;
  }
}

class HrWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const d = document.createElement("div");
    d.className = "cm-hr-widget";
    d.appendChild(document.createElement("hr"));
    return d;
  }
}

class EmbedWidget extends WidgetType {
  constructor(readonly url: string, readonly kind: string, readonly alt: string, readonly render?: (el: HTMLElement) => void) {
    super();
  }
  eq(o: EmbedWidget) {
    return o.url === this.url && o.alt === this.alt;
  }
  toDOM() {
    const span = document.createElement("span");
    span.className = "internal-embed media-embed image-embed is-loaded";
    if (this.render) {
      span.className = "internal-embed file-embed is-loaded";
      this.render(span);
      return span;
    }
    if (this.kind === "image") {
      const img = document.createElement("img");
      img.src = this.url;
      const size = /^(\d+)(?:x(\d+))?$/.exec(this.alt);
      if (size) {
        img.width = Number(size[1]);
        if (size[2]) img.height = Number(size[2]);
      } else if (this.alt) img.alt = this.alt;
      span.appendChild(img);
    } else if (this.kind === "audio" || this.kind === "video") {
      const m = document.createElement(this.kind);
      m.controls = true;
      m.src = this.url;
      span.appendChild(m);
    }
    return span;
  }
  ignoreEvent() {
    return false;
  }
}

/** The fence's language, as in "```dataviewjs" or "~~~ yaml {x}". */
function fenceLanguage(state: EditorState, node: { node: any }): string {
  const info = node.node.getChild("CodeInfo");
  return info ? state.doc.sliceString(info.from, info.to).trim().split(/\s/)[0].toLowerCase() : "";
}

class CodeFlairWidget extends WidgetType {
  constructor(readonly lang: string) {
    super();
  }
  eq(o: CodeFlairWidget) {
    return o.lang === this.lang;
  }
  toDOM(view: EditorView) {
    const s = document.createElement("span");
    s.className = "code-block-flair";
    s.textContent = this.lang;
    s.setAttribute("aria-label", "Copy");
    s.addEventListener("mousedown", (e) => {
      e.preventDefault();
      // the code between the fences
      const first = view.state.doc.lineAt(view.posAtDOM(s));
      const tree = syntaxTree(view.state).resolveInner(first.to, -1);
      let block: any = tree;
      while (block && block.name !== "FencedCode") block = block.parent;
      if (!block) return;
      const last = view.state.doc.lineAt(block.to);
      const end = /^\s*(```|~~~)/.test(last.text) && last.number > first.number ? last.from - 1 : last.to;
      navigator.clipboard?.writeText(view.state.doc.sliceString(Math.min(first.to + 1, end), end));
    });
    return s;
  }
  ignoreEvent() {
    return true;
  }
}

class MathWidget extends WidgetType {
  constructor(readonly source: string, readonly display: boolean) {
    super();
  }
  eq(o: MathWidget) {
    return o.source === this.source && o.display === this.display;
  }
  toDOM(view: EditorView) {
    const host = view.state.facet(livePreviewHost);
    const el = host?.renderMath?.(this.source, this.display) || document.createElement("span");
    if (!host?.renderMath) el.textContent = this.source;
    const wrap = document.createElement("span");
    wrap.className = "math-inline-widget";
    wrap.appendChild(el);
    // A click puts the cursor inside the formula, which shows the TeX.
    wrap.addEventListener("mousedown", (e) => {
      e.preventDefault();
      view.dispatch({ selection: { anchor: view.posAtDOM(wrap) + (this.display ? 2 : 1) } });
      view.focus();
    });
    return wrap;
  }
  ignoreEvent() {
    return true;
  }
}

// ── live preview decorations ──────────────────────────────────────────────

const hide = Decoration.replace({});

function touches(state: EditorState, from: number, to: number, wholeLines = false): boolean {
  for (const r of state.selection.ranges) {
    if (wholeLines) {
      const a = state.doc.lineAt(from).from, b = state.doc.lineAt(to).to;
      if (r.to >= a && r.from <= b) return true;
    } else if (r.to >= from && r.from <= to) return true;
  }
  return false;
}

const normalizeLabel = (s: string) => s.trim().replace(/\s+/g, " ").toLowerCase();

/** The URL of a "[label]: url" definition in the document, or null. */
function linkReference(state: EditorState, label: string): string | null {
  const want = normalizeLabel(label);
  if (!want) return null;
  let found: string | null = null;
  syntaxTree(state).iterate({
    enter: (n) => {
      if (found !== null) return false;
      if (n.name !== "LinkReference") return n.name === "Document" || n.name === "Blockquote" || n.name.endsWith("List") || n.name === "ListItem";
      const l = n.node.getChild("LinkLabel");
      const u = n.node.getChild("URL");
      if (l && normalizeLabel(state.doc.sliceString(l.from + 1, l.to - 1)) === want) found = u ? state.doc.sliceString(u.from, u.to) : "";
      return false;
    },
  });
  return found;
}

function buildLivePreview(view: EditorView): DecorationSet {
  const state = view.state;
  const host = state.facet(livePreviewHost);
  const deco: Range<Decoration>[] = [];
  const doc = state.doc;
  const focused = view.hasFocus;
  const touched = (a: number, b: number, lines = false) => focused && touches(state, a, b, lines);

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const n = node.name;
        switch (n) {
          case "HeaderMark": {
            const parent = node.node.parent;
            if (parent && /^ATXHeading/.test(parent.name) && !touched(parent.from, parent.to, true)) {
              let end = node.to;
              if (doc.sliceString(end, end + 1) === " ") end++;
              if (node.from === parent.from) deco.push(hide.range(node.from, end));
              else deco.push(hide.range(node.from - (doc.sliceString(node.from - 1, node.from) === " " ? 1 : 0), node.to));
            }
            break;
          }
          case "EmphasisMark":
          case "StrikethroughMark":
          case "CodeMark":
          case "HighlightMark": {
            const parent = node.node.parent;
            if (!parent || parent.name === "FencedCode") break;
            if (!touched(parent.from, parent.to)) deco.push(hide.range(node.from, node.to));
            break;
          }
          case "InlineCodeMark": {
            // flat inline code: hide both marks unless the cursor is inside the span
            const isOpen = node.node.nextSibling?.name === "InlineCodeText" || node.node.nextSibling?.name === "InlineCodeMark";
            let from = node.from, to = node.to;
            if (isOpen) {
              let n = node.node.nextSibling;
              if (n?.name === "InlineCodeText") n = n.nextSibling;
              to = n ? n.to : node.to;
            } else {
              let p = node.node.prevSibling;
              if (p?.name === "InlineCodeText") p = p.prevSibling;
              from = p ? p.from : node.from;
            }
            if (!touched(from, to)) deco.push(hide.range(node.from, node.to));
            break;
          }
          case "TaskMarker": {
            const text = doc.sliceString(node.from, node.to);
            const ch = text[1] || " ";
            const item = node.node.parent?.parent;
            const lineFrom = doc.lineAt(node.from).from;
            // Hide the list mark too, and replace "[ ]" with a checkbox.
            const listMark = item?.getChild("ListMark");
            if (!touched(node.from, node.to)) {
              const start = listMark && listMark.from >= lineFrom ? listMark.from : node.from;
              deco.push(Decoration.replace({ widget: new CheckboxWidget(ch !== " ", ch) }).range(start, node.to));
            }
            if (ch !== " ") {
              const line = doc.lineAt(node.from);
              deco.push(Decoration.line({ class: "is-checked", attributes: { "data-task": ch } }).range(line.from));
            }
            return false;
          }
          case "ListMark": {
            const item = node.node.parent;
            const list = item?.parent;
            if (list?.name !== "BulletList") break;
            if (item?.getChild("Task")) break; // handled by TaskMarker
            const task = item && doc.sliceString(node.to, node.to + 4);
            if (task && /^ \[.\]/.test(task)) break;
            if (!touched(node.from, node.to)) deco.push(Decoration.replace({ widget: new BulletWidget() }).range(node.from, node.to));
            break;
          }
          case "HorizontalRule":
            if (!touched(node.from, node.to, true)) deco.push(Decoration.replace({ widget: new HrWidget() }).range(node.from, node.to));
            break;
          case "QuoteMark":
            if (!touched(node.from, node.to, true)) {
              let end = node.to;
              if (doc.sliceString(end, end + 1) === " ") end++;
              deco.push(hide.range(node.from, end));
            }
            break;
          case "WikiLink":
          case "Embed": {
            const full = doc.sliceString(node.from, node.to);
            const isEmbed = n === "Embed";
            const inner = full.slice(isEmbed ? 3 : 2, -2);
            const pipe = inner.indexOf("|");
            const target = (pipe === -1 ? inner : inner.slice(0, pipe)).replace(/\\$/, "");
            const alias = pipe === -1 ? "" : inner.slice(pipe + 1);
            if (isEmbed && host && !touched(node.from, node.to)) {
              const r = host.resolveEmbed(target);
              if (r && (r.kind === "image" || r.kind === "audio" || r.kind === "video" || r.render)) {
                deco.push(Decoration.replace({ widget: new EmbedWidget(r.render ? r.url.replace(/\?\d+$/, "") : r.url, r.kind, alias, r.render) }).range(node.from, node.to));
                return false;
              }
            }
            const open = node.from + (isEmbed ? 3 : 2);
            const close = node.to - 2;
            const resolved = host ? host.isLinkResolved(target) : true;
            const linkMark = Decoration.mark({ class: "cm-underline" + (resolved ? "" : " is-unresolved"), attributes: { "data-href": target } });
            if (!touched(node.from, node.to)) {
              deco.push(hide.range(node.from, open));
              if (pipe !== -1) {
                deco.push(hide.range(open, open + pipe + 1));
                if (open + pipe + 1 < close) deco.push(linkMark.range(open + pipe + 1, close));
              } else if (open < close) {
                deco.push(linkMark.range(open, close));
              }
              deco.push(hide.range(close, node.to));
            } else if (open < close) {
              deco.push(linkMark.range(open, close));
            }
            return false;
          }
          case "Link": {
            const marks = node.node.getChildren("LinkMark");
            const url = node.node.getChild("URL");
            if (marks.length < 2) break;
            const textFrom = marks[0].to;
            const textTo = marks[1].from;
            let href = url ? doc.sliceString(url.from, url.to) : "";
            if (!url) {
              // Lezer makes every "[text]" a reference link; it is one only if "[text]: url" is defined.
              const label = node.node.getChild("LinkLabel");
              const key = label ? doc.sliceString(label.from + 1, label.to - 1) : doc.sliceString(textFrom, textTo);
              const def = linkReference(state, key);
              if (def === null) break;
              href = def;
            }
            if (textTo > textFrom) deco.push(Decoration.mark({ class: "cm-underline", attributes: { "data-href": href, "data-external": /^[a-z][a-z0-9+.-]*:/i.test(href) ? "1" : "" } }).range(textFrom, textTo));
            if (!touched(node.from, node.to)) {
              deco.push(hide.range(node.from, textFrom));
              deco.push(hide.range(textTo, node.to));
            }
            return false;
          }
          case "Image": {
            const url = node.node.getChild("URL");
            if (!url || touched(node.from, node.to)) break;
            const href = doc.sliceString(url.from, url.to);
            const external = /^[a-z][a-z0-9+.-]*:/i.test(href);
            const r = external ? { url: href, kind: "image" as const } : host?.resolveEmbed(decodeURI(href));
            if (r && r.kind === "image") deco.push(Decoration.replace({ widget: new EmbedWidget(r.url, "image", "") }).range(node.from, node.to));
            return false;
          }
          case "Hashtag":
            deco.push(Decoration.mark({ class: "cm-hashtag-decoration", attributes: { "data-tag": doc.sliceString(node.from, node.to) } }).range(node.from, node.to));
            return false;
          case "ObsidianComment":
            break;
          case "InlineMath": {
            if (!host?.renderMath || touched(node.from, node.to)) return false;
            const text = doc.sliceString(node.from, node.to);
            const display = text.startsWith("$$");
            const parent = node.node.parent;
            // a paragraph made only of $$…$$ is a block widget (StateField); a formula over several lines stays text
            if (text.includes("\n") || (display && parent?.name === "Paragraph" && parent.from === node.from && parent.to === node.to)) return false;
            const open = display ? 2 : 1;
            deco.push(Decoration.replace({ widget: new MathWidget(text.slice(open, -open), display) }).range(node.from, node.to));
            return false;
          }
          case "FencedCode": {
            // Blocks a code block processor renders are block widgets (StateField).
            const lang = fenceLanguage(state, node);
            if (host?.hasCodeBlockRenderer?.(lang) || touched(node.from, node.to, true)) return false;
            // Away from the cursor the fences are hidden; the opening one leaves the language as a flair.
            const marks = node.node.getChildren("CodeMark");
            const first = doc.lineAt(node.from);
            if (marks[0] && marks[0].from < first.to) {
              deco.push((lang ? Decoration.replace({ widget: new CodeFlairWidget(lang) }) : hide).range(marks[0].from, first.to));
            }
            const close = marks.length > 1 ? marks[marks.length - 1] : null;
            if (close && doc.lineAt(close.from).number > first.number) deco.push(hide.range(close.from, close.to));
            return false;
          }
          case "Frontmatter":
            return false;
        }
      },
    });
  }
  return Decoration.set(deco, true);
}

const livePreviewPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    tree: any;
    constructor(view: EditorView) {
      this.decorations = buildLivePreview(view);
      this.tree = syntaxTree(view.state);
    }
    update(u: ViewUpdate) {
      const tree = syntaxTree(u.state);
      if (u.docChanged || u.viewportChanged || u.selectionSet || u.focusChanged || tree !== this.tree) {
        this.tree = tree;
        this.decorations = buildLivePreview(u.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

/** Clicking links: plain click in Live Preview, Mod+click in source mode. */
function linkClicks(live: boolean): Extension {
  return EditorView.domEventHandlers({
    mousedown(e, view) {
      if (e.button !== 0 && e.button !== 1) return false;
      const target = e.target as HTMLElement;
      const tagEl = target.closest?.("[data-tag]") as HTMLElement | null;
      const el = target.closest?.("[data-href]") as HTMLElement | null;
      const mod = e.metaKey || e.ctrlKey;
      if (!el && !tagEl) return false;
      if (!live && !mod) return false;
      // In live preview, a click on the visible text of a link follows it,
      // unless the cursor is already inside the link (editing it).
      const pos = view.posAtDOM(target);
      const sel = view.state.selection.main;
      if (live && !mod && view.hasFocus && Math.abs(sel.head - pos) <= 1 && sel.empty) return false;
      const host = view.state.facet(livePreviewHost);
      e.preventDefault();
      if (tagEl && !el) {
        (window as any).app?.internalPlugins?.getPluginById("global-search")?.instance?.openGlobalSearch("tag:" + tagEl.dataset.tag);
        return true;
      }
      const href = el!.dataset.href || "";
      if (el!.dataset.external) (window as any).app?.openExternal?.(href);
      else host?.openLink(href, e);
      return true;
    },
  });
}

// ── block widgets (callouts, properties, tables) — must come from a StateField ─────

/** What a rendering keeps alive (plugin render children), freed when CodeMirror drops the widget's DOM. */
const disposers = new WeakMap<HTMLElement, () => void>();

function renderInto(view: EditorView, markdown: string, el: HTMLElement) {
  const dispose = view.state.facet(livePreviewHost)?.renderMarkdown?.(markdown, el);
  if (dispose) disposers.set(el, dispose);
}

function disposeRendering(dom: HTMLElement) {
  disposers.get(dom)?.();
  disposers.delete(dom);
}

const EDIT_ICON = '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="svg-icon lucide-code-2"><path d="m18 16 4-4-4-4"/><path d="m6 8-4 4 4 4"/><path d="m14.5 4-5 16"/></svg>';

/**
 * A block that reading view renders and Live Preview shows rendered while the
 * cursor is elsewhere: code blocks with a processor (dataviewjs, mermaid…),
 * display math, note and PDF embeds, HTML blocks. "click" blocks are edited by
 * clicking them; "button" blocks hold interactive content, so they get
 * Obsidian's edit button instead.
 */
class RenderedBlockWidget extends WidgetType {
  constructor(readonly text: string, readonly cls: string, readonly edit: "click" | "button", readonly caret: number) {
    super();
  }
  eq(o: RenderedBlockWidget) {
    return o.text === this.text && o.cls === this.cls;
  }
  toDOM(view: EditorView) {
    const el = document.createElement("div");
    el.className = `cm-embed-block ${this.cls} markdown-rendered`;
    renderInto(view, this.text, el);
    const enter = () => {
      view.dispatch({ selection: { anchor: Math.min(view.posAtDOM(el) + this.caret, view.state.doc.length) } });
      view.focus();
    };
    if (this.edit === "button") {
      const btn = document.createElement("div");
      btn.className = "edit-block-button";
      btn.setAttribute("aria-label", "Edit this block");
      btn.innerHTML = EDIT_ICON;
      btn.addEventListener("mousedown", (e) => e.preventDefault());
      btn.addEventListener("click", (e) => {
        e.preventDefault();
        enter();
      });
      el.appendChild(btn);
    } else {
      el.addEventListener("mousedown", (e) => {
        if (e.button !== 0 || (e.target as HTMLElement).closest("a, input, button")) return;
        e.preventDefault();
        enter();
      });
    }
    return el;
  }
  destroy(dom: HTMLElement) {
    disposeRendering(dom);
  }
  ignoreEvent() {
    return true;
  }
}

class CalloutWidget extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  // Typing above a callout shifts `from`; comparing it would re-render every callout below on each keystroke.
  eq(o: CalloutWidget) {
    return o.text === this.text;
  }
  toDOM(view: EditorView) {
    const el = document.createElement("div");
    el.className = "cm-embed-block cm-callout markdown-rendered";
    renderInto(view, this.text, el);
    el.addEventListener("mousedown", (e) => {
      if ((e.target as HTMLElement).closest("a, input, .callout-fold, .callout-title")) return;
      e.preventDefault();
      view.dispatch({ selection: { anchor: view.posAtDOM(el) + 2 } });
      view.focus();
    });
    return el;
  }
  destroy(dom: HTMLElement) {
    disposeRendering(dom);
  }
  ignoreEvent() {
    return true;
  }
}

class PropertiesWidget extends WidgetType {
  constructor(readonly text: string, readonly to: number) {
    super();
  }
  eq(o: PropertiesWidget) {
    return o.text === this.text;
  }
  toDOM(view: EditorView) {
    const el = document.createElement("div");
    el.className = "cm-embed-block metadata-container-widget";
    const host = view.state.facet(livePreviewHost);
    host?.renderProperties?.(el);
    el.addEventListener("mousedown", (e) => {
      if ((e.target as HTMLElement).closest("a")) return;
      e.preventDefault();
      view.dispatch({ selection: { anchor: 4 } });
      view.focus();
    });
    return el;
  }
  ignoreEvent() {
    return true;
  }
}

/** Offset of the start of cell `col` in a table row (inside its leading space). */
function cellOffset(text: string, col: number): number {
  const pipes: number[] = [];
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "\\") i++;
    else if (text[i] === "|") pipes.push(i);
  }
  const lead = /^\s*\|/.test(text) ? 1 : 0;
  const pipe = pipes[col + lead - 1];
  let at = col + lead === 0 ? text.search(/\S|$/) : pipe === undefined ? text.length : pipe + 1;
  if (text[at] === " ") at++;
  return at;
}

class TableWidget extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  eq(o: TableWidget) {
    return o.text === this.text;
  }
  toDOM(view: EditorView) {
    const el = document.createElement("div");
    el.className = "cm-embed-block cm-table-widget markdown-rendered";
    renderInto(view, this.text, el);
    // A click puts the cursor in the clicked cell of the source, which shows the table as text.
    el.addEventListener("mousedown", (e) => {
      if (e.button !== 0 || (e.target as HTMLElement).closest("a, input")) return;
      e.preventDefault();
      const start = view.posAtDOM(el);
      const cell = (e.target as HTMLElement).closest("td, th") as HTMLTableCellElement | null;
      const tr = cell?.parentElement as HTMLTableRowElement | null;
      let pos = start;
      if (cell && tr) {
        const body = tr.parentElement?.tagName === "TBODY" ? Array.prototype.indexOf.call(tr.parentElement.children, tr) + 2 : 0;
        const doc = view.state.doc;
        const lineNo = Math.min(doc.lineAt(start).number + body, doc.lines);
        const line = doc.line(lineNo);
        pos = line.from + cellOffset(line.text, Array.prototype.indexOf.call(tr.children, cell));
      }
      view.dispatch({ selection: { anchor: pos } });
      view.focus();
    });
    return el;
  }
  destroy(dom: HTMLElement) {
    disposeRendering(dom);
  }
  ignoreEvent() {
    return true;
  }
}

function buildBlocks(state: EditorState): DecorationSet {
  const out: Range<Decoration>[] = [];
  const doc = state.doc;
  const sel = state.selection.ranges;
  const inside = (a: number, b: number) => sel.some((r) => r.to >= a && r.from <= b);
  const host = state.facet(livePreviewHost);
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name === "Frontmatter") {
        if (!inside(node.from, node.to) && host?.renderProperties && (host as any).propertiesMode?.() !== "source") {
          out.push(Decoration.replace({ widget: new PropertiesWidget(doc.sliceString(node.from, node.to), node.to), block: true }).range(node.from, node.to));
        }
        return false;
      }
      if (node.name === "Blockquote") {
        const first = doc.lineAt(node.from).text;
        if (/^\s*>\s*\[![^\]]+\]/.test(first) && !inside(node.from, node.to) && host?.renderMarkdown) {
          const end = doc.lineAt(node.to).to;
          out.push(Decoration.replace({ widget: new CalloutWidget(doc.sliceString(node.from, end)), block: true }).range(doc.lineAt(node.from).from, end));
        }
        return false;
      }
      if (node.name === "Table") {
        const from = doc.lineAt(node.from).from, to = doc.lineAt(node.to).to;
        if (!inside(from, to) && host?.renderMarkdown) {
          out.push(Decoration.replace({ widget: new TableWidget(doc.sliceString(from, to)), block: true }).range(from, to));
        }
        return false;
      }
      if (node.name === "FencedCode") {
        const lang = fenceLanguage(state, node);
        const from = doc.lineAt(node.from).from, to = doc.lineAt(node.to).to;
        if (host?.renderMarkdown && host.hasCodeBlockRenderer?.(lang) && !inside(from, to)) {
          const firstLine = doc.lineAt(from);
          out.push(Decoration.replace({ widget: new RenderedBlockWidget(doc.sliceString(from, to), "cm-preview-code-block cm-lang-" + lang, "button", firstLine.length + 1), block: true }).range(from, to));
        }
        return false;
      }
      if (node.name === "HTMLBlock") {
        const from = doc.lineAt(node.from).from, to = doc.lineAt(node.to).to;
        if (host?.renderMarkdown && !inside(from, to)) out.push(Decoration.replace({ widget: new RenderedBlockWidget(doc.sliceString(from, to), "cm-html-embed", "click", 0), block: true }).range(from, to));
        return false;
      }
      if (node.name === "Paragraph") {
        const from = doc.lineAt(node.from).from, to = doc.lineAt(node.to).to;
        if (!host?.renderMarkdown || inside(from, to) || doc.sliceString(from, node.from).trim() || doc.sliceString(node.to, to).trim()) return false;
        const only = node.node.firstChild;
        if (!only || only.nextSibling || only.from !== node.from || only.to !== node.to) return false;
        const text = doc.sliceString(from, to);
        // $$…$$ alone in its paragraph: display math
        if (only.name === "InlineMath" && text.trim().startsWith("$$")) {
          out.push(Decoration.replace({ widget: new RenderedBlockWidget(text, "math-block-widget", "click", 2), block: true }).range(from, to));
        }
        // ![[note]] alone on its line: the note (or PDF…) itself, as in reading view
        if (only.name === "Embed") {
          const inner = doc.sliceString(only.from + 3, only.to - 2);
          const target = inner.split("|")[0].replace(/\\$/, "");
          const r = host.resolveEmbed(target);
          if (r && !r.render && r.kind !== "image" && r.kind !== "audio" && r.kind !== "video") {
            out.push(Decoration.replace({ widget: new RenderedBlockWidget(text, "markdown-embed-widget", "button", 3), block: true }).range(from, to));
          }
        }
        return false;
      }
      if (node.name === "Document") return true;
      return false;
    },
  });
  return Decoration.set(out, true);
}

const blockWidgets = StateField.define<DecorationSet>({
  create: (s) => buildBlocks(s),
  update(value, tr) {
    if (tr.docChanged || tr.selection || syntaxTree(tr.state) !== syntaxTree(tr.startState)) return buildBlocks(tr.state);
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

export function livePreview(): Extension {
  // Low precedence: plugin decorations (e.g. Dataview inline queries) must win.
  return [Prec.low(livePreviewPlugin), Prec.low(blockWidgets), linkClicks(true), editorLivePreviewField.init(() => true), EditorView.editorAttributes.of({ class: "is-live-preview" })];
}

export function sourceMode(): Extension {
  return [linkClicks(false), editorLivePreviewField.init(() => false)];
}

export { EditorSelection };
