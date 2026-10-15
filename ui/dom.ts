// The DOM/JS prototype extensions Obsidian installs globally (declared in the
// `declare global` block of obsidian.d.ts). Plugins call these everywhere:
// `containerEl.createDiv({ cls: "x" })`, `el.empty()`, `arr.contains(x)` …

const def = (proto: any, name: string, fn: any) => {
  if (!(name in proto)) Object.defineProperty(proto, name, { value: fn, configurable: true, writable: true });
};
const getter = (proto: any, name: string, fn: () => any) => {
  if (!(name in proto)) Object.defineProperty(proto, name, { get: fn, configurable: true });
};

// ── Object / Array / Math / String / Number ──────────────────────────────
def(Object, "isEmpty", (o: any) => { for (const _ in o) return false; return true; });
def(Object, "each", (o: any, cb: any, ctx?: any) => {
  for (const k in o) if (Object.prototype.hasOwnProperty.call(o, k) && cb.call(ctx, o[k], k) === false) return false;
  return true;
});
def(Array, "combine", (arrs: any[][]) => ([] as any[]).concat(...arrs));
const AP = Array.prototype as any;
def(AP, "first", function (this: any[]) { return this[0]; });
def(AP, "last", function (this: any[]) { return this[this.length - 1]; });
def(AP, "contains", function (this: any[], t: any) { return this.indexOf(t) !== -1; });
def(AP, "remove", function (this: any[], t: any) { const i = this.indexOf(t); if (i !== -1) this.splice(i, 1); });
def(AP, "shuffle", function (this: any[]) {
  for (let i = this.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [this[i], this[j]] = [this[j], this[i]]; }
  return this;
});
def(AP, "unique", function (this: any[]) { return Array.from(new Set(this)); });
def(AP, "findLastIndex", function (this: any[], p: any) { for (let i = this.length - 1; i >= 0; i--) if (p(this[i], i, this)) return i; return -1; });
def(Math, "clamp", (v: number, a: number, b: number) => Math.min(Math.max(v, a), b));
def(Math, "square", (v: number) => v * v);
def(String, "isString", (o: any) => typeof o === "string" || o instanceof String);
def(String.prototype, "contains", function (this: string, t: string) { return this.indexOf(t) !== -1; });
def(String.prototype, "format", function (this: string, ...args: string[]) {
  return this.replace(/{(\d+)}/g, (m, n) => (args[n] !== undefined ? args[n] : m));
});
def(Number, "isNumber", (o: any) => typeof o === "number");
(window as any).isBoolean = (o: any) => typeof o === "boolean";

// ── element creation ──────────────────────────────────────────────────────
const SVG_NS = "http://www.w3.org/2000/svg";

function applyInfo(el: any, o: any) {
  if (o == null) return;
  if (typeof o === "string") o = { cls: o };
  if (o.cls) Array.isArray(o.cls) ? el.classList.add(...o.cls.filter(Boolean)) : el.className = o.cls;
  if (o.text !== undefined) el.setText(o.text);
  if (o.attr) for (const k in o.attr) setAttrValue(el, k, o.attr[k]);
  if (o.title !== undefined) el.title = o.title;
  if (o.value !== undefined && "value" in el) el.value = o.value;
  if (o.type) el.type = o.type;
  if (o.placeholder) el.placeholder = o.placeholder;
  if (o.href) el.href = o.href;
  if (o.parent) o.prepend ? o.parent.insertBefore(el, o.parent.firstChild) : o.parent.appendChild(el);
}

function setAttrValue(el: Element, k: string, v: any) {
  if (v === null || v === undefined || v === false) el.removeAttribute(k);
  else el.setAttribute(k, v === true ? "" : String(v));
}

function createElement(tag: string, o?: any, cb?: (el: any) => void): any {
  const el = document.createElement(tag);
  applyInfo(el, o);
  cb?.(el);
  return el;
}

function createSvgElement(tag: string, o?: any, cb?: (el: any) => void): any {
  const el = document.createElementNS(SVG_NS, tag);
  if (o) {
    if (typeof o === "string") o = { cls: o };
    if (o.cls) el.classList.add(...(Array.isArray(o.cls) ? o.cls : o.cls.split(" ")).filter(Boolean));
    if (o.attr) for (const k in o.attr) setAttrValue(el, k, o.attr[k]);
    if (o.parent) o.prepend ? o.parent.insertBefore(el, o.parent.firstChild) : o.parent.appendChild(el);
  }
  cb?.(el);
  return el;
}

const NP = Node.prototype as any;
def(NP, "createEl", function (this: Node, tag: string, o?: any, cb?: any) {
  const el = createElement(tag, o);
  if (!(o && typeof o === "object" && o.parent)) {
    if (o && typeof o === "object" && o.prepend) this.insertBefore(el, this.firstChild);
    else this.appendChild(el);
  }
  cb?.(el);
  return el;
});
def(NP, "createDiv", function (this: any, o?: any, cb?: any) { return this.createEl("div", o, cb); });
def(NP, "createSpan", function (this: any, o?: any, cb?: any) { return this.createEl("span", o, cb); });
def(NP, "createSvg", function (this: Node, tag: string, o?: any, cb?: any) {
  const el = createSvgElement(tag, o);
  if (!(o && typeof o === "object" && o.parent)) {
    if (o && typeof o === "object" && o.prepend) this.insertBefore(el, this.firstChild);
    else this.appendChild(el);
  }
  cb?.(el);
  return el;
});
def(NP, "detach", function (this: Node) { this.parentNode?.removeChild(this); });
def(NP, "empty", function (this: Node) { while (this.firstChild) this.removeChild(this.firstChild); });
def(NP, "insertAfter", function (this: Node, node: Node, child: Node | null) {
  this.insertBefore(node, child ? child.nextSibling : this.firstChild);
  return node;
});
def(NP, "indexOf", function (this: Node, other: Node) { return Array.prototype.indexOf.call(this.childNodes, other); });
def(NP, "setChildrenInPlace", function (this: Node, children: Node[]) {
  const set = new Set(children);
  for (const c of Array.from(this.childNodes)) if (!set.has(c)) this.removeChild(c);
  let ref: Node | null = this.firstChild;
  for (const c of children) {
    if (c === ref) { ref = ref.nextSibling; continue; }
    this.insertBefore(c, ref);
  }
});
def(NP, "appendText", function (this: Node, v: string) { this.appendChild(document.createTextNode(v)); });
def(NP, "instanceOf", function (this: any, type: any) { return this instanceof type; });
getter(NP, "doc", function (this: Node) { return this.ownerDocument || document; });
getter(NP, "win", function (this: Node) { return (this.ownerDocument || document).defaultView || window; });
getter(NP, "constructorWin", function () { return window; });

const EP = Element.prototype as any;
def(EP, "getText", function (this: Element) { return this.textContent || ""; });
def(EP, "setText", function (this: Element, v: any) {
  if (v instanceof DocumentFragment || v instanceof Node) { (this as any).empty(); this.appendChild(v); }
  else this.textContent = v == null ? "" : String(v);
});
def(EP, "addClass", function (this: Element, ...c: string[]) { this.classList.add(...c.flatMap((x) => (x || "").split(" ")).filter(Boolean)); });
def(EP, "addClasses", function (this: Element, c: string[]) { (this as any).addClass(...c); });
def(EP, "removeClass", function (this: Element, ...c: string[]) { this.classList.remove(...c.flatMap((x) => (x || "").split(" ")).filter(Boolean)); });
def(EP, "removeClasses", function (this: Element, c: string[]) { (this as any).removeClass(...c); });
def(EP, "toggleClass", function (this: Element, c: string | string[], v: boolean) {
  for (const x of Array.isArray(c) ? c : [c]) if (x) this.classList.toggle(x, v);
});
def(EP, "hasClass", function (this: Element, c: string) { return this.classList.contains(c); });
def(EP, "setAttr", function (this: Element, k: string, v: any) { setAttrValue(this, k, v); });
def(EP, "setAttrs", function (this: Element, o: any) { for (const k in o) setAttrValue(this, k, o[k]); });
def(EP, "getAttr", function (this: Element, k: string) { return this.getAttribute(k); });
def(EP, "matchParent", function (this: Element, sel: string, last?: Element) {
  let el: Element | null = this;
  while (el) { if (el.matches?.(sel)) return el; if (el === last) return null; el = el.parentElement; }
  return null;
});
def(EP, "getCssPropertyValue", function (this: Element, p: string, pseudo?: string) { return getComputedStyle(this, pseudo).getPropertyValue(p); });
def(EP, "isActiveElement", function (this: Element) { return this.ownerDocument.activeElement === this; });
def(EP, "find", function (this: Element, s: string) { return this.querySelector(s); });
def(EP, "findAll", function (this: Element, s: string) { return Array.from(this.querySelectorAll(s)); });
def(EP, "findAllSelf", function (this: Element, s: string) {
  const r = Array.from(this.querySelectorAll(s)); if (this.matches(s)) r.unshift(this as any); return r;
});
const DFP = DocumentFragment.prototype as any;
def(DFP, "find", function (this: DocumentFragment, s: string) { return this.querySelector(s); });
def(DFP, "findAll", function (this: DocumentFragment, s: string) { return Array.from(this.querySelectorAll(s)); });

const HP = HTMLElement.prototype as any;
const SP = SVGElement.prototype as any;
for (const P of [HP, SP]) {
  def(P, "setCssStyles", function (this: HTMLElement, s: any) { Object.assign(this.style, s); });
  def(P, "setCssProps", function (this: HTMLElement, p: Record<string, string>) { for (const k in p) this.style.setProperty(k, p[k]); });
}
def(HP, "show", function (this: HTMLElement) { this.style.display = ""; if (getComputedStyle(this).display === "none") this.style.display = "block"; });
def(HP, "hide", function (this: HTMLElement) { this.style.display = "none"; });
def(HP, "toggle", function (this: any, s: boolean) { s ? this.show() : this.hide(); });
def(HP, "toggleVisibility", function (this: HTMLElement, v: boolean) { this.style.visibility = v ? "" : "hidden"; });
def(HP, "isShown", function (this: HTMLElement) { return !!this.offsetParent || this.style.display !== "none" && this.isConnected; });
getter(HP, "innerWidth", function (this: HTMLElement) {
  const s = getComputedStyle(this); return this.clientWidth - parseFloat(s.paddingLeft) - parseFloat(s.paddingRight);
});
getter(HP, "innerHeight", function (this: HTMLElement) {
  const s = getComputedStyle(this); return this.clientHeight - parseFloat(s.paddingTop) - parseFloat(s.paddingBottom);
});

// ── delegated events ──────────────────────────────────────────────────────
function on(this: any, type: string, selector: string, listener: any, options?: any) {
  if (!this._EVENTS) this._EVENTS = {};
  if (!this._EVENTS[type]) this._EVENTS[type] = [];
  const callback = (ev: Event) => {
    const t = ev.target as any;
    if (!t || !t.matchParent) return;
    const delegate = t.matchParent(selector, this);
    if (delegate) listener.call(this, ev, delegate);
  };
  this._EVENTS[type].push({ selector, listener, options, callback });
  this.addEventListener(type, callback, options);
}
function off(this: any, type: string, selector: string, listener: any, options?: any) {
  const list = this._EVENTS?.[type];
  if (!list) return;
  for (let i = list.length - 1; i >= 0; i--) {
    const e = list[i];
    if (e.selector === selector && e.listener === listener) {
      this.removeEventListener(type, e.callback, options);
      list.splice(i, 1);
    }
  }
}
def(HP, "on", on);
def(HP, "off", off);
def(Document.prototype, "on", on);
def(Document.prototype, "off", off);
def(HP, "onClickEvent", function (this: HTMLElement, l: any, o?: any) {
  this.addEventListener("click", l, o);
  this.addEventListener("auxclick", l, o);
});
def(HP, "onNodeInserted", function (this: HTMLElement, l: () => any, once?: boolean) {
  const handler = (ev: AnimationEvent) => {
    if (ev.animationName !== "node-inserted" || ev.target !== this) return;
    if (once) this.removeEventListener("animationstart", handler as any);
    l();
  };
  this.classList.add("node-insert-event");
  this.addEventListener("animationstart", handler as any);
  // Fallback for elements that are already connected.
  if (this.isConnected) queueMicrotask(() => { if (this.isConnected) l(); });
  return () => this.removeEventListener("animationstart", handler as any);
});
def(HP, "onWindowMigrated", function () { return () => {}; });
def(HP, "trigger", function (this: HTMLElement, t: string) { this.dispatchEvent(new Event(t)); });

const UP = UIEvent.prototype as any;
getter(UP, "targetNode", function (this: UIEvent) { return this.target as Node; });
getter(UP, "win", function (this: UIEvent) { return (this as any).view || window; });
getter(UP, "doc", function (this: UIEvent) { return ((this as any).view || window).document; });
def(UP, "instanceOf", function (this: any, t: any) { return this instanceof t; });

// ── globals ───────────────────────────────────────────────────────────────
const W = window as any;
W.createEl = (tag: string, o?: any, cb?: any) => createElement(tag, o, cb);
W.createDiv = (o?: any, cb?: any) => createElement("div", o, cb);
W.createSpan = (o?: any, cb?: any) => createElement("span", o, cb);
W.createSvg = (tag: string, o?: any, cb?: any) => createSvgElement(tag, o, cb);
W.createFragment = (cb?: (f: DocumentFragment) => void) => { const f = document.createDocumentFragment(); cb?.(f); return f; };
W.fish = (s: string) => document.querySelector(s);
W.fishAll = (s: string) => Array.from(document.querySelectorAll(s));
W.activeWindow = window;
W.activeDocument = document;
W.sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
W.nextFrame = () => new Promise((r) => requestAnimationFrame(() => r(undefined)));
W.ready = (fn: () => any) => document.readyState !== "loading" ? fn() : document.addEventListener("DOMContentLoaded", fn);
W.ajax = (o: any) => {
  const req: XMLHttpRequest = o.req || new XMLHttpRequest();
  req.open(o.method || "GET", o.url, true);
  if (o.headers) for (const k in o.headers) req.setRequestHeader(k, o.headers[k]);
  req.withCredentials = !!o.withCredentials;
  req.onload = () => (req.status >= 200 && req.status < 300 ? o.success?.(req.response, req) : o.error?.(req.statusText, req));
  req.onerror = (e) => o.error?.(e, req);
  req.send(typeof o.data === "object" && !(o.data instanceof ArrayBuffer) ? JSON.stringify(o.data) : o.data);
};
W.ajaxPromise = (o: any) => new Promise((resolve, reject) => W.ajax({ ...o, success: resolve, error: reject }));

export {};
