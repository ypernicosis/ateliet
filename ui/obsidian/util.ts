// Stateless helpers exported by the `obsidian` module.

import yaml from "js-yaml";
import moment from "moment";
import { invoke, isMac, isWin } from "../tauri.ts";

export { moment };

export const apiVersion = "1.12.7";

export function requireApiVersion(version: string): boolean {
  const a = apiVersion.split(".").map(Number);
  const b = version.split(".").map(Number);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] || 0, y = b[i] || 0;
    if (x !== y) return x > y;
  }
  return true;
}

export const Platform = {
  isDesktop: true,
  isMobile: false,
  isDesktopApp: true,
  isMobileApp: false,
  isIosApp: false,
  isAndroidApp: false,
  isPhone: false,
  isTablet: false,
  isMacOS: isMac,
  isWin: isWin,
  isLinux: !isMac && !isWin,
  isSafari: false,
  resourcePathPrefix: "ateliet://localhost/",
  canPopoutWindow: false,
};

export function normalizePath(path: string): string {
  path = path.replace(/([\\/])+/g, "/").replace(/(^\/+|\/+$)/g, "");
  if (path === "") path = "/";
  path = path.replace(/ | /g, " ");
  return path.normalize("NFC");
}

export function debounce<T extends unknown[], V>(cb: (...args: T) => V, timeout = 0, resetTimer = false): any {
  let timer: any = null;
  let lastArgs: T | null = null;
  let lastThis: any = null;
  const run = () => {
    timer = null;
    if (lastArgs) {
      const a = lastArgs;
      const ctx = lastThis;
      lastArgs = lastThis = null;
      cb.apply(ctx, a);
    }
  };
  // `this` of the caller is forwarded, like Obsidian's debounce.
  const fn: any = function (this: any, ...args: T) {
    lastArgs = args;
    lastThis = this;
    if (timer && resetTimer) {
      clearTimeout(timer);
      timer = null;
    }
    if (!timer) timer = setTimeout(run, timeout);
    return fn;
  };
  fn.cancel = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    lastArgs = null;
    return fn;
  };
  fn.run = () => {
    if (timer) clearTimeout(timer);
    run();
    return undefined;
  };
  return fn;
}

export function parseLinktext(linktext: string): { path: string; subpath: string } {
  const i = linktext.indexOf("#");
  if (i === -1) return { path: linktext, subpath: "" };
  return { path: linktext.slice(0, i), subpath: linktext.slice(i) };
}

export function getLinkpath(linktext: string): string {
  return parseLinktext(linktext).path;
}

export function stripHeading(heading: string): string {
  return heading.replace(/[!"#$%&()*+,.:;<=>?@^`{|}~\/\[\]\\]/g, " ").replace(/\s+/g, " ").trim();
}

export function stripHeadingForLink(heading: string): string {
  return heading.replace(/[#|^\[\]]/g, " ").replace(/\s+/g, " ").trim();
}

export function parseYaml(text: string): any {
  return yaml.load(text);
}

export function stringifyYaml(obj: any): string {
  return yaml.dump(obj, { lineWidth: -1 });
}

export function getFrontMatterInfo(content: string) {
  const m = /^---\r?\n/.exec(content);
  if (!m) return { exists: false, frontmatter: "", from: 0, to: 0, contentStart: 0 };
  const re = /^---[ \t]*$/m;
  re.lastIndex = 0;
  const rest = content.slice(m[0].length);
  const end = /(^|\n)---[ \t]*(\r?\n|$)/.exec(rest);
  if (!end) return { exists: false, frontmatter: "", from: 0, to: 0, contentStart: 0 };
  const from = m[0].length;
  const to = from + end.index + (end[1] ? 1 : 0);
  return { exists: true, frontmatter: content.slice(from, to), from, to, contentStart: from + end.index + end[0].length };
}

export function parseFrontMatterEntry(frontmatter: any, key: string | RegExp): any {
  if (!frontmatter) return null;
  if (typeof key === "string") return frontmatter[key] ?? null;
  for (const k in frontmatter) if (key.test(k)) return frontmatter[k];
  return null;
}

export function parseFrontMatterStringArray(frontmatter: any, key: string | RegExp, nospaces = false): string[] | null {
  const v = parseFrontMatterEntry(frontmatter, key);
  if (v == null) return null;
  let arr: any[] = Array.isArray(v) ? v : typeof v === "string" ? v.split(nospaces ? /[,\s]+/ : /,/) : [v];
  arr = arr.filter((x) => x != null).map((x) => String(x).trim()).filter(Boolean);
  return arr.length ? arr : null;
}

export function parseFrontMatterTags(frontmatter: any): string[] | null {
  const a = parseFrontMatterStringArray(frontmatter, /^tags?$/i, true);
  return a ? a.map((t) => (t.startsWith("#") ? t : "#" + t)) : null;
}

export function parseFrontMatterAliases(frontmatter: any): string[] | null {
  return parseFrontMatterStringArray(frontmatter, /^alias(es)?$/i);
}

export function getAllTags(cache: any): string[] | null {
  if (!cache) return null;
  const out: string[] = [];
  const fm = parseFrontMatterTags(cache.frontmatter);
  if (fm) out.push(...fm);
  if (cache.tags) for (const t of cache.tags) out.push(t.tag);
  return out;
}

export function iterateRefs(refs: any[], cb: (ref: any) => boolean | void): boolean {
  for (const r of refs || []) if (cb(r)) return true;
  return false;
}

export function iterateCacheRefs(cache: any, cb: (ref: any) => boolean | void): boolean {
  if (!cache) return false;
  return iterateRefs(cache.links, cb) || iterateRefs(cache.embeds, cb) || iterateRefs(cache.frontmatterLinks, cb);
}

export function resolveSubpath(cache: any, subpath: string): any {
  if (!cache || !subpath) return null;
  const parts = subpath.split("#").filter(Boolean);
  if (parts.length === 0) return null;
  const last = parts[parts.length - 1];
  if (last.startsWith("^")) {
    const id = last.slice(1);
    const block = cache.blocks?.[id] || cache.blocks?.[id.toLowerCase()];
    if (!block) return null;
    const list = cache.listItems?.find((l: any) => l.id === id);
    return { type: "block", block, list, start: block.position.start, end: block.position.end };
  }
  const headings = cache.headings || [];
  let idx = -1;
  let from = 0;
  for (const part of parts) {
    const want = stripHeading(part).toLowerCase();
    let found = -1;
    for (let i = from; i < headings.length; i++) {
      if (stripHeading(headings[i].heading).toLowerCase() === want) { found = i; break; }
    }
    if (found === -1) return null;
    idx = found;
    from = found + 1;
  }
  const current = headings[idx];
  let next = null;
  for (let i = idx + 1; i < headings.length; i++) if (headings[i].level <= current.level) { next = headings[i]; break; }
  return { type: "heading", current, next, start: current.position.start, end: next ? next.position.start : null };
}

export function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

export function base64ToArrayBuffer(b64: string): ArrayBuffer {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out.buffer;
}

export function arrayBufferToHex(data: ArrayBuffer): string {
  return Array.from(new Uint8Array(data)).map((b) => b.toString(16).padStart(2, "0")).join("");
}

export function hexToArrayBuffer(hex: string): ArrayBuffer {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out.buffer;
}

export async function getBlobArrayBuffer(blob: Blob): Promise<ArrayBuffer> {
  return blob.arrayBuffer();
}

export function getLanguage(): string {
  try {
    return localStorage.getItem("language") || "en";
  } catch {
    return "en";
  }
}

// ── network ───────────────────────────────────────────────────────────────

export function requestUrl(request: any): any {
  const req = typeof request === "string" ? { url: request } : request;
  const p = (async () => {
    let body: string | undefined;
    let body_b64: string | undefined;
    if (req.body instanceof ArrayBuffer) body_b64 = arrayBufferToBase64(req.body);
    else if (ArrayBuffer.isView(req.body)) body_b64 = arrayBufferToBase64((req.body as any).buffer);
    else if (req.body != null) body = String(req.body);
    const headers = { ...(req.headers || {}) };
    if (req.contentType) headers["Content-Type"] = req.contentType;
    const r = await invoke<any>("http_request", { req: { url: req.url, method: req.method || "GET", headers, body, body_b64 } });
    const arrayBuffer = base64ToArrayBuffer(r.body_b64);
    let textCache: string | null = null;
    const res = {
      status: r.status,
      headers: r.headers,
      arrayBuffer,
      get text() {
        return (textCache ??= new TextDecoder().decode(arrayBuffer));
      },
      get json() {
        return JSON.parse(this.text);
      },
    };
    if (r.status >= 400 && req.throw !== false) {
      const err: any = new Error(`Request failed, status ${r.status}`);
      err.status = r.status;
      err.headers = r.headers;
      throw err;
    }
    return res;
  })();
  const out: any = p;
  out.arrayBuffer = p.then((r) => r.arrayBuffer);
  out.json = p.then((r) => r.json);
  out.text = p.then((r) => r.text);
  return out;
}

export async function request(req: any): Promise<string> {
  return (await requestUrl(req)).text;
}

// ── fuzzy search ──────────────────────────────────────────────────────────

export function prepareQuery(query: string) {
  return { query, tokens: query.toLowerCase().split(/\s+/).filter(Boolean), fuzzy: query.toLowerCase().split("") };
}

export function prepareFuzzySearch(query: string): (text: string) => any {
  const q = query.toLowerCase().replace(/\s+/g, "");
  return (text: string) => {
    if (!q) return { score: 0, matches: [] };
    const t = text.toLowerCase();
    const matches: [number, number][] = [];
    let ti = 0;
    let score = 0;
    let lastMatch = -2;
    for (const ch of q) {
      const i = t.indexOf(ch, ti);
      if (i === -1) return null;
      if (i === lastMatch + 1 && matches.length) matches[matches.length - 1][1] = i + 1;
      else matches.push([i, i + 1]);
      // Reward consecutive matches and word starts, penalise gaps.
      score -= i - ti;
      if (i === 0 || /[\s\-_/.]/.test(t[i - 1])) score += 3;
      if (i === lastMatch + 1) score += 2;
      lastMatch = i;
      ti = i + 1;
    }
    score -= (t.length - q.length) * 0.01;
    return { score, matches };
  };
}

export function prepareSimpleSearch(query: string): (text: string) => any {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return (text: string) => {
    const t = text.toLowerCase();
    const matches: [number, number][] = [];
    let score = 0;
    for (const w of words) {
      const i = t.indexOf(w);
      if (i === -1) return null;
      matches.push([i, i + w.length]);
      score -= i;
    }
    matches.sort((a, b) => a[0] - b[0]);
    return { score, matches };
  };
}

export function fuzzySearch(q: any, text: string) {
  return prepareFuzzySearch(typeof q === "string" ? q : q.query)(text);
}

export function sortSearchResults(results: { match: { score: number } }[]): void {
  results.sort((a, b) => b.match.score - a.match.score);
}

export function renderMatches(el: HTMLElement | DocumentFragment, text: string, matches: [number, number][] | null, offset = 0): void {
  let last = 0;
  for (const [s0, e0] of matches || []) {
    const s = s0 + offset, e = e0 + offset;
    if (e <= 0 || s >= text.length) continue;
    const ss = Math.max(0, s), ee = Math.min(text.length, e);
    if (ss > last) el.appendChild(document.createTextNode(text.slice(last, ss)));
    const m = document.createElement("span");
    m.className = "suggestion-highlight";
    m.textContent = text.slice(ss, ee);
    el.appendChild(m);
    last = ee;
  }
  if (last < text.length) el.appendChild(document.createTextNode(text.slice(last)));
}

export function renderResults(el: HTMLElement, text: string, result: any, offset = 0): void {
  renderMatches(el, text, result?.matches, offset);
}

// ── HTML helpers ──────────────────────────────────────────────────────────

export function sanitizeHTMLToDom(html: string): DocumentFragment {
  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  tpl.content.querySelectorAll("script, iframe[srcdoc], object, embed").forEach((e) => e.remove());
  tpl.content.querySelectorAll("*").forEach((el) => {
    for (const a of Array.from(el.attributes)) {
      if (/^on/i.test(a.name) || (/^(href|src|action)$/i.test(a.name) && /^\s*javascript:/i.test(a.value))) el.removeAttribute(a.name);
    }
  });
  return tpl.content;
}

export function htmlToMarkdown(html: string | HTMLElement | Document | DocumentFragment): string {
  let root: Node;
  if (typeof html === "string") {
    root = sanitizeHTMLToDom(html);
  } else root = html as any;
  const walk = (n: Node, ctx: { list?: string; idx?: number } = {}): string => {
    if (n.nodeType === 3) return (n.textContent || "").replace(/\s+/g, " ");
    if (n.nodeType !== 1 && n.nodeType !== 11 && n.nodeType !== 9) return "";
    const el = n as HTMLElement;
    const kids = () => Array.from(n.childNodes).map((c) => walk(c, ctx)).join("");
    switch (el.tagName?.toLowerCase()) {
      case "h1": case "h2": case "h3": case "h4": case "h5": case "h6":
        return "\n\n" + "#".repeat(+el.tagName[1]) + " " + kids().trim() + "\n\n";
      case "p": case "div": return "\n\n" + kids().trim() + "\n\n";
      case "br": return "\n";
      case "strong": case "b": return "**" + kids() + "**";
      case "em": case "i": return "*" + kids() + "*";
      case "del": case "s": return "~~" + kids() + "~~";
      case "mark": return "==" + kids() + "==";
      case "code": return el.parentElement?.tagName === "PRE" ? kids() : "`" + kids() + "`";
      case "pre": return "\n\n```\n" + (el.textContent || "") + "\n```\n\n";
      case "a": return `[${kids()}](${el.getAttribute("href") || ""})`;
      case "img": return `![${el.getAttribute("alt") || ""}](${el.getAttribute("src") || ""})`;
      case "blockquote": return "\n\n" + kids().trim().split("\n").map((l) => "> " + l).join("\n") + "\n\n";
      case "hr": return "\n\n---\n\n";
      case "ul": case "ol": {
        let i = 0;
        const items = Array.from(el.children).map((li) => {
          i++;
          const bullet = el.tagName === "OL" ? `${i}. ` : "- ";
          return bullet + walk(li).trim().replace(/\n/g, "\n  ");
        });
        return "\n\n" + items.join("\n") + "\n\n";
      }
      case "script": case "style": return "";
      default: return kids();
    }
  };
  return walk(root).replace(/\n{3,}/g, "\n\n").trim();
}

// ── lazy libraries ────────────────────────────────────────────────────────

const scripts: Record<string, Promise<void>> = {};
export function loadScript(src: string): Promise<void> {
  return (scripts[src] ||= new Promise((resolve, reject) => {
    const s = document.createElement("script");
    s.src = src;
    s.onload = () => resolve();
    s.onerror = () => reject(new Error("failed to load " + src));
    document.head.appendChild(s);
  }));
}

export async function loadMathJax(): Promise<any> {
  const W = window as any;
  if (!W.MathJax?.tex2svg) {
    W.MathJax = { tex: { inlineMath: [["$", "$"]] }, svg: { fontCache: "global" }, startup: { typeset: false } };
    await loadScript("https://cdn.jsdelivr.net/npm/mathjax@3.2.2/es5/tex-svg.js");
    await W.MathJax.startup?.promise;
  }
  return W.MathJax;
}

export function renderMath(source: string, display: boolean): HTMLElement {
  const el = document.createElement(display ? "div" : "span");
  el.className = display ? "math math-block" : "math math-inline";
  el.textContent = source;
  loadMathJax()
    .then((MJ) => {
      el.empty();
      el.appendChild(MJ.tex2svg(source, { display }));
      el.addClass("is-loaded");
    })
    .catch(() => {});
  return el;
}

export async function finishRenderMath(): Promise<void> {}

export async function loadMermaid(): Promise<any> {
  const W = window as any;
  if (!W.mermaid) {
    await loadScript("https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.min.js");
    W.mermaid.initialize({ startOnLoad: false, theme: document.body.hasClass("theme-dark") ? "dark" : "default" });
  }
  return W.mermaid;
}

export async function loadPrism(): Promise<any> {
  const W = window as any;
  if (!W.Prism) {
    await loadScript("https://cdn.jsdelivr.net/npm/prismjs@1.29.0/prism.min.js");
    await loadScript("https://cdn.jsdelivr.net/npm/prismjs@1.29.0/plugins/autoloader/prism-autoloader.min.js");
  }
  return W.Prism;
}

export async function loadPdfJs(): Promise<any> {
  const W = window as any;
  if (!W.pdfjsLib) {
    await loadScript("https://cdn.jsdelivr.net/npm/pdfjs-dist@3.11.174/build/pdf.min.js");
    W.pdfjsLib.GlobalWorkerOptions.workerSrc = "https://cdn.jsdelivr.net/npm/pdfjs-dist@3.11.174/build/pdf.worker.min.js";
  }
  return W.pdfjsLib;
}
