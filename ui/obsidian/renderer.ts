// Reading-view rendering: HTML blocks come from the Rust core, then the JS
// side runs Obsidian's built-in post-processing and plugin post-processors.

import { Component } from "./events.ts";
import { getIcon, setIcon } from "./icons.ts";
import { loadMermaid, parseLinktext, renderMath, resolveSubpath } from "./util.ts";
import { TFile } from "./vault.ts";
import { invoke } from "../tauri.ts";
import { highlightCode } from "@lezer/highlight";
import { languages } from "@codemirror/language-data";
import { LanguageDescription } from "@codemirror/language";
import { readingHighlight } from "../editor/syntax.ts";

export type MarkdownPostProcessor = ((el: HTMLElement, ctx: MarkdownPostProcessorContext) => Promise<any> | void) & { sortOrder?: number };

export interface MarkdownSectionInformation {
  text: string;
  lineStart: number;
  lineEnd: number;
}

export interface MarkdownPostProcessorContext {
  docId: string;
  sourcePath: string;
  frontmatter: any;
  containerEl: HTMLElement;
  el: HTMLElement;
  addChild(child: MarkdownRenderChild): void;
  getSectionInfo(el: HTMLElement): MarkdownSectionInformation | null;
  displayMode?: boolean;
}

export class MarkdownRenderChild extends Component {
  containerEl: HTMLElement;
  constructor(containerEl: HTMLElement) {
    super();
    this.containerEl = containerEl;
  }
}

const postProcessors: MarkdownPostProcessor[] = [];
/** language → post processor (plain object, like Obsidian's internal map). */
const codeBlockProcessors: Record<string, MarkdownPostProcessor> = {};

export class MarkdownPreviewRenderer {
  static codeBlockPostProcessors = codeBlockProcessors;
  static postProcessors = postProcessors;
  static registerPostProcessor(pp: MarkdownPostProcessor, sortOrder?: number): void {
    if (sortOrder !== undefined) pp.sortOrder = sortOrder;
    postProcessors.push(pp);
    postProcessors.sort((a, b) => (a.sortOrder || 0) - (b.sortOrder || 0));
  }
  static unregisterPostProcessor(pp: MarkdownPostProcessor): void {
    postProcessors.remove(pp);
  }
  static createCodeBlockPostProcessor(language: string, handler: (source: string, el: HTMLElement, ctx: MarkdownPostProcessorContext) => Promise<any> | void): MarkdownPostProcessor {
    const pp: MarkdownPostProcessor = (el, ctx) => {
      const promises: any[] = [];
      for (const code of Array.from(el.querySelectorAll(`pre > code.language-${CSS.escape(language)}`))) {
        const pre = code.parentElement!;
        const div = createDiv(`block-language-${language}`);
        pre.replaceWith(div);
        const source = (code.textContent || "").replace(/\n$/, "");
        try {
          promises.push(handler(source, div, ctx));
        } catch (e) {
          console.error(e);
          div.setText(String(e));
        }
      }
      return Promise.all(promises);
    };
    return pp;
  }
  /** @internal */ static registerCodeBlock(language: string, pp: MarkdownPostProcessor) {
    codeBlockProcessors[language] = pp;
  }
  /** @internal */ static unregisterCodeBlock(language: string) {
    delete codeBlockProcessors[language];
  }
  /** @internal */ static hasCodeBlockProcessor(language: string) {
    return language in codeBlockProcessors;
  }
}

// ── rendering ─────────────────────────────────────────────────────────────

interface RenderOptions {
  /** keep one wrapper `div` per block (reading view); otherwise unwrap. */
  sections?: boolean;
  depth?: number;
  frontmatter?: boolean;
  displayMode?: boolean;
}

let docCounter = 0;

export async function renderMarkdownInto(app: any, markdown: string, el: HTMLElement, sourcePath: string, component: Component, opts: RenderOptions = {}): Promise<HTMLElement[]> {
  if (sourcePath && !el.closest?.("[data-source-path]")) el.setAttribute("data-source-path", sourcePath);
  const blocks: { html: string; lineStart: number; lineEnd: number; kind: string }[] = await invoke("md_render", { text: markdown });
  const docId = "doc-" + ++docCounter;
  const sectionInfo = new WeakMap<HTMLElement, { lineStart: number; lineEnd: number }>();
  const holder = document.createElement("div");
  const sections: HTMLElement[] = [];
  const file = app.vault.getAbstractFileByPath(sourcePath);
  const fm = file instanceof TFile ? app.metadataCache.getFileCache(file)?.frontmatter : undefined;

  for (const b of blocks) {
    if (b.kind === "yaml") {
      if (opts.frontmatter && opts.sections) {
        const div = holder.createDiv("mod-frontmatter mod-ui");
        renderProperties(app, div, markdown, sourcePath);
        sectionInfo.set(div, b);
        sections.push(div);
      }
      continue;
    }
    const div = holder.createDiv({ attr: { "data-line-start": b.lineStart } });
    div.innerHTML = b.html;
    sectionInfo.set(div, b);
    sections.push(div);
  }

  const ctxFor = (sectionEl: HTMLElement): MarkdownPostProcessorContext => ({
    docId,
    sourcePath,
    frontmatter: fm,
    containerEl: el,
    el: sectionEl,
    displayMode: opts.displayMode ?? true,
    addChild: (child: MarkdownRenderChild) => component.addChild(child),
    getSectionInfo: (target: HTMLElement) => {
      let n: HTMLElement | null = target;
      while (n && !sectionInfo.has(n)) n = n.parentElement;
      const info = n ? sectionInfo.get(n) : null;
      return info ? { text: markdown, lineStart: info.lineStart, lineEnd: info.lineEnd } : null;
    },
  });

  // Move into place before post-processing so plugins can measure layout.
  if (opts.sections) for (const s of sections) el.appendChild(s);

  const pending: Promise<any>[] = [];
  for (const section of sections) {
    const ctx = ctxFor(section);
    builtinPostProcess(app, section, ctx, component, opts.depth ?? 0);
    for (const [lang, pp] of Object.entries(codeBlockProcessors)) {
      if (section.querySelector(`pre > code.language-${CSS.escape(lang)}`)) {
        try {
          const r = pp(section, ctx);
          if (r && typeof (r as any).then === "function") pending.push((r as any).catch(console.error));
        } catch (e) {
          console.error(e);
        }
      }
    }
    for (const pp of postProcessors) {
      try {
        const r = pp(section, ctx);
        if (r && typeof (r as any).then === "function") pending.push((r as any).catch(console.error));
      } catch (e) {
        console.error(e);
      }
    }
    highlightCodeBlocks(section);
  }

  if (!opts.sections) {
    for (const s of sections) {
      while (s.firstChild) {
        const n = s.firstChild;
        // drop the newlines between block elements (they break inline use)
        if (n.nodeType === 3 && !n.textContent!.trim()) s.removeChild(n);
        else el.appendChild(n);
      }
    }
  }
  await Promise.all(pending);
  return sections;
}

export class MarkdownRenderer extends MarkdownRenderChild {
  app: any;
  hoverPopover: any = null;
  constructor(app: any, containerEl: HTMLElement) {
    super(containerEl);
    this.app = app;
  }

  static async render(app: any, markdown: string, el: HTMLElement, sourcePath: string, component: Component): Promise<void> {
    el.addClass?.("markdown-rendered");
    await renderMarkdownInto(app, markdown, el, sourcePath, component, { displayMode: false });
  }

  static async renderMarkdown(markdown: string, el: HTMLElement, sourcePath: string, component: Component): Promise<void> {
    return MarkdownRenderer.render((window as any).app, markdown, el, sourcePath, component);
  }
}

// ── built-in post processing ──────────────────────────────────────────────

const IMAGE_EXT = ["png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "avif", "ico", "tif", "tiff"];
const AUDIO_EXT = ["mp3", "wav", "m4a", "ogg", "3gp", "flac", "webm", "aac"];
const VIDEO_EXT = ["mp4", "webm", "ogv", "mov", "mkv", "m4v"];

export function embedKind(ext: string): "image" | "audio" | "video" | "pdf" | "note" | "other" {
  ext = ext.toLowerCase();
  if (IMAGE_EXT.includes(ext)) return "image";
  if (VIDEO_EXT.includes(ext) && ext !== "webm") return "video";
  if (AUDIO_EXT.includes(ext)) return "audio";
  if (ext === "pdf") return "pdf";
  if (ext === "md") return "note";
  return "other";
}

const CALLOUT_ICONS: Record<string, string> = {
  note: "pencil", abstract: "clipboard-list", summary: "clipboard-list", tldr: "clipboard-list",
  info: "info", todo: "check-circle-2", tip: "flame", hint: "flame", important: "flame",
  success: "check", check: "check", done: "check", question: "help-circle", help: "help-circle", faq: "help-circle",
  warning: "alert-triangle", caution: "alert-triangle", attention: "alert-triangle",
  failure: "x", fail: "x", missing: "x", danger: "zap", error: "zap", bug: "bug",
  example: "list", quote: "quote", cite: "quote",
};
const CALLOUT_ALIAS: Record<string, string> = {
  summary: "abstract", tldr: "abstract", hint: "tip", important: "tip", check: "success", done: "success",
  help: "question", faq: "question", caution: "warning", attention: "warning", fail: "failure", missing: "failure",
  error: "danger", cite: "quote",
};

function builtinPostProcess(app: any, el: HTMLElement, ctx: MarkdownPostProcessorContext, component: Component, depth: number) {
  const mc = app.metadataCache;
  // internal links
  for (const a of Array.from(el.querySelectorAll<HTMLAnchorElement>("a.internal-link"))) {
    const href = a.getAttribute("data-href") || "";
    const { path } = parseLinktext(href);
    if (path && !mc.getFirstLinkpathDest(path, ctx.sourcePath)) a.addClass("is-unresolved");
  }
  // callouts
  for (const c of Array.from(el.querySelectorAll<HTMLElement>(".callout"))) {
    const type = c.getAttribute("data-callout") || "note";
    const icon = c.querySelector<HTMLElement>(".callout-icon");
    if (icon && !icon.firstChild) {
      const custom = getComputedStyle(c).getPropertyValue("--callout-icon").trim().replace(/^["']|["']$/g, "");
      setIcon(icon, custom || CALLOUT_ICONS[type] || "pencil");
    }
    const canonical = CALLOUT_ALIAS[type];
    if (canonical) c.setAttribute("data-callout-canonical", canonical);
    const fold = c.querySelector<HTMLElement>(".callout-fold");
    if (fold) {
      setIcon(fold, "chevron-down");
      if (c.hasClass("is-collapsed")) fold.addClass("is-collapsed");
      c.querySelector(".callout-title")?.addEventListener("click", (e) => {
        e.preventDefault();
        const collapsed = !c.hasClass("is-collapsed");
        c.toggleClass("is-collapsed", collapsed);
        fold.toggleClass("is-collapsed", collapsed);
        const content = c.querySelector<HTMLElement>(":scope > .callout-content");
        if (content) content.style.display = collapsed ? "none" : "";
      });
    }
  }
  // task checkboxes: write back to the source file
  for (const input of Array.from(el.querySelectorAll<HTMLInputElement>("input.task-list-item-checkbox"))) {
    input.addEventListener("click", async (e) => {
      const info = ctx.getSectionInfo(input);
      const file = app.vault.getAbstractFileByPath(ctx.sourcePath);
      if (!info || !(file instanceof TFile)) return;
      e.preventDefault();
      const line = info.lineStart + Number(input.getAttribute("data-line") || 0);
      await app.vault.process(file, (text: string) => {
        const lines = text.split("\n");
        const l = lines[line];
        if (l === undefined) return text;
        lines[line] = l.replace(/^(\s*(?:[-*+]|\d+[.)])\s+\[)(.)(\])/, (_m, a, ch, b) => a + (ch === " " ? "x" : " ") + b);
        return lines.join("\n");
      });
    });
  }
  // math
  for (const m of Array.from(el.querySelectorAll<HTMLElement>(".math:not(.is-loaded)"))) {
    const display = m.hasClass("math-block");
    m.replaceWith(renderMath(m.textContent || "", display));
  }
  // mermaid
  for (const code of Array.from(el.querySelectorAll<HTMLElement>("pre > code.language-mermaid"))) {
    if ("mermaid" in codeBlockProcessors) continue;
    const pre = code.parentElement!;
    const div = createDiv("mermaid");
    const src = code.textContent || "";
    pre.replaceWith(div);
    loadMermaid()
      .then(async (mermaid: any) => {
        const { svg } = await mermaid.render("m" + Math.random().toString(36).slice(2), src);
        div.innerHTML = svg;
      })
      .catch((e: any) => div.setText(String(e?.message || e)));
  }
  // code block copy buttons
  for (const pre of Array.from(el.querySelectorAll<HTMLElement>("pre"))) {
    if (pre.querySelector(".copy-code-button")) continue;
    const btn = pre.createEl("button", { cls: "copy-code-button" });
    setIcon(btn, "copy");
    btn.addEventListener("click", () => {
      navigator.clipboard.writeText(pre.querySelector("code")?.textContent || "");
      btn.addClass("is-copied");
      setTimeout(() => btn.removeClass("is-copied"), 1200);
    });
  }
  // embeds
  for (const span of Array.from(el.querySelectorAll<HTMLElement>("span.internal-embed"))) {
    processEmbed(app, span, ctx.sourcePath, component, depth);
  }
  // external images: lazy
  for (const img of Array.from(el.querySelectorAll<HTMLImageElement>("img"))) img.loading = "lazy";
}

function processEmbed(app: any, span: HTMLElement, sourcePath: string, component: Component, depth: number) {
  const src = span.getAttribute("src") || "";
  const alt = span.getAttribute("alt") || "";
  const { path, subpath } = parseLinktext(src);
  const file: TFile | null = app.metadataCache.getFirstLinkpathDest(path, sourcePath);
  span.empty();
  if (!file) {
    span.addClass("file-embed", "mod-empty");
    span.createDiv({ cls: "file-embed-title", text: `"${path}" could not be found.` });
    return;
  }
  const kind = embedKind(file.extension);
  span.addClass("is-loaded");
  const url = app.vault.getResourcePath(file);
  if (kind === "image") {
    span.addClass("media-embed", "image-embed");
    const img = span.createEl("img", { attr: { src: url, alt: /^\d+(x\d+)?$/.test(alt) ? file.basename : alt } });
    const size = /^(\d+)(?:x(\d+))?$/.exec(alt);
    if (size) {
      img.width = Number(size[1]);
      if (size[2]) img.height = Number(size[2]);
    }
  } else if (kind === "audio" || kind === "video") {
    span.addClass("media-embed", kind === "audio" ? "audio-embed" : "video-embed");
    const m = span.createEl(kind, { attr: { controls: true, src: url } }) as HTMLMediaElement;
    m.controls = true;
  } else if (kind === "pdf") {
    span.addClass("pdf-embed");
    span.createEl("iframe", { attr: { src: url + (subpath ? subpath : ""), style: "width:100%;height:var(--embed-max-height, 600px);border:none;" } });
  } else if (kind === "note") {
    span.addClass("markdown-embed", "inline-embed", "is-loaded");
    if (depth > 4) {
      span.setText("(embed depth limit)");
      return;
    }
    const content = span.createDiv("markdown-embed-content");
    const link = span.createDiv("markdown-embed-link");
    setIcon(link, "link");
    link.setAttr("aria-label", "Open link");
    link.addEventListener("click", (e) => {
      e.preventDefault();
      app.workspace.openLinkText(src, sourcePath, (window as any).Keymap?.isModEvent?.(e) || false);
    });
    app.vault.cachedRead(file).then((text: string) => {
      let part = text;
      if (subpath) {
        const cache = app.metadataCache.getFileCache(file);
        const r = resolveSubpath(cache, subpath);
        if (r) {
          const lines = text.split("\n");
          const end = r.end ? r.end.line + (r.type === "block" ? 1 : 0) : lines.length;
          part = lines.slice(r.start.line, end).join("\n");
        }
      } else {
        // strip frontmatter for full-note embeds
        part = text.replace(/^---\r?\n[\s\S]*?\r?\n---[ \t]*(\r?\n|$)/, "");
      }
      const view = content.createDiv("markdown-preview-view markdown-rendered");
      renderMarkdownInto(app, part, view, file.path, component, { depth: depth + 1, displayMode: true });
    });
  } else if (app.embedRegistry?.embedByExtension?.[file.extension]) {
    // A registered embed (storyboards…) draws itself.
    span.addClass("file-embed");
    const c = app.embedRegistry.embedByExtension[file.extension]({ app, containerEl: span, sourcePath, linktext: src }, file, subpath);
    if (c) {
      component.addChild?.(c);
      c.loadFile?.();
    }
  } else {
    span.addClass("file-embed");
    const t = span.createDiv("file-embed-title");
    const ic = t.createSpan("file-embed-icon");
    setIcon(ic, "file");
    t.appendText(" " + file.name);
    span.addEventListener("click", () => app.openWithDefaultApp?.(file.path));
  }
}

// ── code highlighting ─────────────────────────────────────────────────────

const langCache = new Map<string, Promise<any> | null>();

function getParser(lang: string): Promise<any> | null {
  if (langCache.has(lang)) return langCache.get(lang)!;
  const desc = LanguageDescription.matchLanguageName(languages, lang, true);
  const p = desc ? desc.load().then((support: any) => support.language.parser).catch(() => null) : null;
  langCache.set(lang, p);
  return p;
}

function highlightCodeBlocks(el: HTMLElement) {
  for (const code of Array.from(el.querySelectorAll<HTMLElement>("pre > code[class*='language-']"))) {
    const m = /language-([\w+#-]+)/.exec(code.className);
    if (!m) continue;
    const p = getParser(m[1]);
    if (!p) continue;
    const text = code.textContent || "";
    p.then((parser: any) => {
      if (!parser || !code.isConnected && !code.closest("div")) return;
      const tree = parser.parse(text);
      const frag = document.createDocumentFragment();
      highlightCode(
        text,
        tree,
        readingHighlight,
        (t: string, cls: string) => {
          if (!cls) frag.appendChild(document.createTextNode(t));
          else {
            const s = document.createElement("span");
            s.className = cls;
            s.textContent = t;
            frag.appendChild(s);
          }
        },
        () => frag.appendChild(document.createTextNode("\n")),
      );
      code.empty();
      code.appendChild(frag);
    });
  }
}

// ── properties (frontmatter) in reading view ─────────────────────────────

export function renderProperties(app: any, el: HTMLElement, markdown: string, sourcePath: string) {
  const file = app.vault.getAbstractFileByPath(sourcePath);
  const fm = file instanceof TFile ? app.metadataCache.getFileCache(file)?.frontmatter : null;
  if (!fm || Object.keys(fm).length === 0) return;
  void markdown;
  const container = el.createDiv("metadata-container");
  const heading = container.createDiv("metadata-properties-heading");
  heading.createDiv({ cls: "metadata-properties-title", text: "Properties" });
  const props = container.createDiv("metadata-properties");
  for (const key in fm) {
    const row = props.createDiv({ cls: "metadata-property", attr: { "data-property-key": key } });
    const k = row.createDiv("metadata-property-key");
    const ic = k.createSpan("metadata-property-icon");
    const v = fm[key];
    setIcon(ic, Array.isArray(v) ? "list" : typeof v === "number" ? "binary" : typeof v === "boolean" ? "check-square" : /^\d{4}-\d{2}-\d{2}/.test(String(v)) ? "calendar" : key === "tags" ? "tags" : "text");
    k.createSpan({ cls: "metadata-property-key-input", text: key });
    const val = row.createDiv("metadata-property-value");
    const values = Array.isArray(v) ? v : [v];
    if (key === "tags" || key === "tag" || Array.isArray(v)) {
      const pills = val.createDiv("multi-select-container");
      for (const x of values) {
        if (x == null) continue;
        const pill = pills.createDiv("multi-select-pill");
        const s = String(x);
        const content = pill.createDiv("multi-select-pill-content");
        renderPropertyValue(app, content, key === "tags" || key === "tag" ? (s.startsWith("#") ? s : "#" + s) : s, sourcePath, key);
      }
    } else if (typeof v === "boolean") {
      val.createEl("input", { type: "checkbox", attr: { disabled: true, checked: v || null } });
    } else {
      renderPropertyValue(app, val.createDiv("metadata-input-longtext"), v == null ? "" : typeof v === "object" ? JSON.stringify(v) : String(v), sourcePath, key);
    }
  }
}

function renderPropertyValue(app: any, el: HTMLElement, s: string, sourcePath: string, key: string) {
  const wl = /^\[\[([^\]|]+)(?:\|([^\]]+))?\]\]$/.exec(s.trim());
  if (wl) {
    const a = el.createEl("a", { cls: "internal-link", text: wl[2] || wl[1], attr: { "data-href": wl[1], href: wl[1] } });
    if (!app.metadataCache.getFirstLinkpathDest(wl[1], sourcePath)) a.addClass("is-unresolved");
  } else if ((key === "tags" || key === "tag") && s.startsWith("#")) {
    el.createEl("a", { cls: "tag", text: s, attr: { href: s } });
  } else if (/^https?:\/\//.test(s)) {
    el.createEl("a", { cls: "external-link", text: s, attr: { href: s } });
  } else el.setText(s);
}

// ── hover popovers (page preview) ─────────────────────────────────────────

export class HoverPopover extends Component {
  hoverEl: HTMLElement;
  state = 0;
  parent: any;
  targetEl: HTMLElement | null;
  private showTimer: any;
  private hideTimer: any;

  constructor(parent: any, targetEl: HTMLElement | null, waitTime = 300, _staticPos?: any) {
    super();
    this.parent = parent;
    this.targetEl = targetEl;
    this.hoverEl = createDiv("popover hover-popover");
    if (parent) {
      parent.hoverPopover?.hide?.();
      parent.hoverPopover = this;
    }
    const leaveCheck = () => {
      clearTimeout(this.hideTimer);
      this.hideTimer = setTimeout(() => {
        if (!this.hoverEl.matches(":hover") && !targetEl?.matches(":hover")) this.hide();
      }, 300);
    };
    targetEl?.addEventListener("mouseleave", leaveCheck);
    this.hoverEl.addEventListener("mouseleave", leaveCheck);
    this.hoverEl.addEventListener("mouseenter", () => clearTimeout(this.hideTimer));
    targetEl?.addEventListener("mousedown", () => this.hide());
    this.showTimer = setTimeout(() => this.show(), waitTime);
  }

  show() {
    if (this.targetEl && !this.targetEl.isConnected) return;
    document.body.appendChild(this.hoverEl);
    this.state = 2;
    this.load();
    this.position();
  }

  position() {
    const t = this.targetEl?.getBoundingClientRect();
    if (!t) return;
    const r = this.hoverEl.getBoundingClientRect();
    let top = t.bottom + 6;
    if (top + r.height > window.innerHeight - 8) top = Math.max(8, t.top - r.height - 6);
    let left = t.left;
    if (left + r.width > window.innerWidth - 8) left = Math.max(8, window.innerWidth - r.width - 8);
    this.hoverEl.style.top = top + "px";
    this.hoverEl.style.left = left + "px";
  }

  hide() {
    clearTimeout(this.showTimer);
    clearTimeout(this.hideTimer);
    this.hoverEl.detach();
    this.state = 0;
    if (this.parent?.hoverPopover === this) this.parent.hoverPopover = null;
    this.unload();
  }
}

/** Page preview core plugin: listens to `hover-link` and shows the note. */
export function installPagePreview(app: any) {
  app.workspace.on("hover-link", (info: any) => {
    const { event, targetEl, linktext, sourcePath, hoverParent } = info;
    if (!targetEl || !linktext) return;
    const source = info.source;
    const requireMod = source === "editor" || source === "preview" && false;
    if (requireMod && !(event?.metaKey || event?.ctrlKey)) return;
    const { path, subpath } = parseLinktext(linktext);
    const file: TFile | null = app.metadataCache.getFirstLinkpathDest(path, sourcePath || "");
    if (!file) return;
    const pop = new HoverPopover(hoverParent || {}, targetEl, 350);
    pop.hoverEl.addClass("is-loaded");
    const kind = embedKind(file.extension);
    if (kind === "image") {
      pop.hoverEl.createEl("img", { attr: { src: app.vault.getResourcePath(file) } });
      return;
    }
    if (kind !== "note") return;
    const embed = pop.hoverEl.createDiv("markdown-embed is-loaded");
    const content = embed.createDiv("markdown-embed-content");
    const view = content.createDiv("markdown-preview-view markdown-rendered");
    app.vault.cachedRead(file).then((text: string) => {
      let part = text;
      if (subpath) {
        const r = resolveSubpath(app.metadataCache.getFileCache(file), subpath);
        if (r) {
          const lines = text.split("\n");
          part = lines.slice(r.start.line, r.end ? r.end.line + (r.type === "block" ? 1 : 0) : lines.length).join("\n");
        }
      }
      renderMarkdownInto(app, part, view, file.path, pop, { sections: true, frontmatter: !subpath }).then(() => pop.position());
    });
  });
}

export { getIcon };
