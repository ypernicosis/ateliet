// UI primitives of the Obsidian API: Scope/Keymap, Modal, Notice, Menu,
// Setting + components, SuggestModal & friends.

import { Component } from "./events.ts";
import { setIcon, setTooltip } from "./icons.ts";
import { isMac } from "../tauri.ts";
import { prepareFuzzySearch, renderMatches } from "./util.ts";

// ── keyboard ──────────────────────────────────────────────────────────────

export type Modifier = "Mod" | "Ctrl" | "Meta" | "Shift" | "Alt";

export interface KeymapEventHandler {
  scope: Scope;
  modifiers: string | null;
  key: string | null;
  func: (evt: KeyboardEvent, ctx: any) => any;
}

export function eventModifiers(evt: KeyboardEvent | MouseEvent): string {
  const m: string[] = [];
  if (isMac ? evt.metaKey : evt.ctrlKey) m.push("Mod");
  if (isMac && evt.ctrlKey) m.push("Ctrl");
  if (!isMac && evt.metaKey) m.push("Meta");
  if (evt.altKey) m.push("Alt");
  if (evt.shiftKey) m.push("Shift");
  return m.join(",");
}

export function normalizeModifiers(mods: string[]): string {
  const order = ["Mod", "Ctrl", "Meta", "Alt", "Shift"];
  return mods
    .map((m) => (m === "Cmd" ? "Mod" : m))
    .map((m) => (!isMac && m === "Ctrl" ? "Mod" : m))
    .filter((m, i, a) => a.indexOf(m) === i)
    .sort((a, b) => order.indexOf(a) - order.indexOf(b))
    .join(",");
}

export function normalizeKey(evt: KeyboardEvent): string {
  const code = evt.code;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  const k = evt.key;
  if (k === " ") return " ";
  if (k.length === 1) return k.toUpperCase();
  return k;
}

export class Scope {
  parent?: Scope;
  keys: KeymapEventHandler[] = [];

  constructor(parent?: Scope) {
    this.parent = parent;
  }

  register(modifiers: Modifier[] | null, key: string | null, func: (evt: KeyboardEvent, ctx: any) => any): KeymapEventHandler {
    const h: KeymapEventHandler = { scope: this, modifiers: modifiers ? normalizeModifiers(modifiers) : null, key: key, func };
    this.keys.push(h);
    return h;
  }

  unregister(handler: KeymapEventHandler): void {
    this.keys.remove(handler);
  }

  /** @internal returns false when the event was handled. */
  handleKey(evt: KeyboardEvent, info: { modifiers: string; key: string; vkey: string }): any {
    for (const h of this.keys.slice().reverse()) {
      if (h.modifiers !== null && h.modifiers !== info.modifiers) continue;
      if (h.key !== null && h.key.toLowerCase() !== info.key.toLowerCase() && h.key.toLowerCase() !== info.vkey.toLowerCase()) continue;
      const r = h.func(evt, info);
      if (r === false) {
        evt.preventDefault();
        return false;
      }
    }
    return this.parent ? this.parent.handleKey(evt, info) : undefined;
  }
}

export class Keymap {
  rootScope: Scope;
  scope: Scope;
  modifiers = "";
  private stack: Scope[] = [];

  constructor(rootScope: Scope) {
    this.rootScope = rootScope;
    this.scope = rootScope;
  }

  getRootScope(): Scope {
    return this.rootScope;
  }

  pushScope(scope: Scope): void {
    this.stack.remove(scope);
    this.stack.push(scope);
    this.scope = scope;
  }

  popScope(scope: Scope): void {
    this.stack.remove(scope);
    this.scope = this.stack.last() || this.rootScope;
  }

  static isModifier(evt: MouseEvent | TouchEvent | KeyboardEvent, modifier: Modifier): boolean {
    const e = evt as any;
    switch (modifier) {
      case "Mod": return isMac ? e.metaKey : e.ctrlKey;
      case "Ctrl": return e.ctrlKey;
      case "Meta": return e.metaKey;
      case "Shift": return e.shiftKey;
      case "Alt": return e.altKey;
    }
    return false;
  }

  static isModEvent(evt?: UserEvent | null): any {
    if (!evt) return false;
    const e = evt as any;
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (e.button === 1) return "tab";
    if (mod && e.altKey && e.shiftKey) return "window";
    if (mod && e.altKey) return "split";
    if (mod) return "tab";
    return false;
  }

  /** @internal dispatches a keydown through the scope stack. */
  onKeyDown(evt: KeyboardEvent): boolean {
    if (evt.isComposing) return false;
    const info = { modifiers: eventModifiers(evt), key: normalizeKey(evt), vkey: evt.key };
    this.modifiers = info.modifiers;
    return this.scope.handleKey(evt, info) === false;
  }
}

type UserEvent = MouseEvent | KeyboardEvent | TouchEvent | PointerEvent;

// ── Modal ─────────────────────────────────────────────────────────────────

let modalStack: Modal[] = [];

export class Modal {
  app: any;
  scope: Scope;
  containerEl: HTMLElement;
  modalEl: HTMLElement;
  bgEl: HTMLElement;
  headerEl: HTMLElement;
  titleEl: HTMLElement;
  contentEl: HTMLElement;
  shouldRestoreSelection = true;
  shouldAnimate = false;
  private closeCallback?: () => any;
  private lastFocus: Element | null = null;
  private isOpen = false;

  constructor(app: any) {
    this.app = app;
    this.scope = new Scope(app?.scope);
    this.scope.register([], "Escape", () => {
      this.close();
      return false;
    });
    this.containerEl = createDiv("modal-container mod-dim");
    this.bgEl = this.containerEl.createDiv("modal-bg");
    this.modalEl = this.containerEl.createDiv("modal");
    const close = this.modalEl.createDiv("modal-close-button");
    setIcon(close, "x");
    close.addEventListener("click", () => this.close());
    this.headerEl = this.modalEl.createDiv("modal-header");
    this.titleEl = this.headerEl.createDiv("modal-title");
    this.contentEl = this.modalEl.createDiv("modal-content");
    this.bgEl.addEventListener("click", () => this.close());
  }

  open(): void {
    if (this.isOpen) return;
    this.isOpen = true;
    this.lastFocus = document.activeElement;
    document.body.appendChild(this.containerEl);
    modalStack.push(this);
    this.app?.keymap?.pushScope(this.scope);
    try {
      const r: any = this.onOpen();
      if (r && typeof r.catch === "function") r.catch(console.error);
    } catch (e) {
      console.error(e);
    }
    if (!this.modalEl.contains(document.activeElement)) {
      const f = this.modalEl.querySelector("input, textarea, select, button:not(.modal-close-button)") as HTMLElement | null;
      (f || this.modalEl).focus?.({ preventScroll: true });
      if (!f) this.modalEl.setAttribute("tabindex", "-1");
    }
  }

  close(): void {
    if (!this.isOpen) return;
    this.isOpen = false;
    modalStack = modalStack.filter((m) => m !== this);
    this.app?.keymap?.popScope(this.scope);
    this.containerEl.detach();
    try {
      this.onClose();
    } catch (e) {
      console.error(e);
    }
    this.closeCallback?.();
    if (this.shouldRestoreSelection && this.lastFocus instanceof HTMLElement) this.lastFocus.focus({ preventScroll: true });
  }

  onOpen(): void | Promise<void> {}
  onClose(): void {}

  setTitle(title: string): this {
    this.titleEl.setText(title);
    return this;
  }
  setContent(content: string | DocumentFragment): this {
    this.contentEl.setText(content as any);
    return this;
  }
  setCloseCallback(cb: () => any): this {
    this.closeCallback = cb;
    return this;
  }
}

// ── Notice ────────────────────────────────────────────────────────────────

let noticeContainer: HTMLElement | null = null;

export class Notice {
  noticeEl: HTMLElement;
  containerEl: HTMLElement;
  messageEl: HTMLElement;
  private timer: any;

  constructor(message: string | DocumentFragment, duration?: number) {
    if (!noticeContainer || !noticeContainer.isConnected) noticeContainer = document.body.createDiv("notice-container");
    this.containerEl = noticeContainer.createDiv("notice-container-inner");
    this.noticeEl = this.containerEl.createDiv("notice");
    this.messageEl = this.noticeEl.createDiv("notice-message");
    this.setMessage(message);
    this.noticeEl.addEventListener("click", () => this.hide());
    const d = duration ?? 4500;
    if (d > 0) this.timer = setTimeout(() => this.hide(), d);
  }

  setMessage(message: string | DocumentFragment): this {
    this.messageEl.setText(message as any);
    return this;
  }

  hide(): void {
    clearTimeout(this.timer);
    this.containerEl.detach();
  }
}

// ── Menu ──────────────────────────────────────────────────────────────────

let openMenu: Menu | null = null;

export class MenuItem {
  menu: Menu;
  dom: HTMLElement;
  iconEl: HTMLElement;
  titleEl: HTMLElement;
  checkIconEl?: HTMLElement;
  section = "";
  callback?: (evt: MouseEvent | KeyboardEvent) => any;
  disabled = false;
  submenu: Menu | null = null;

  constructor(menu: Menu) {
    this.menu = menu;
    this.dom = createDiv("menu-item");
    this.iconEl = this.dom.createDiv("menu-item-icon");
    this.titleEl = this.dom.createDiv("menu-item-title");
    this.dom.addEventListener("click", (e) => this.handleEvent(e));
    this.dom.addEventListener("mouseenter", () => menu.select(menu.items.indexOf(this)));
  }

  handleEvent(evt: MouseEvent | KeyboardEvent) {
    if (this.disabled) return;
    if (this.submenu) {
      this.openSubmenu();
      return;
    }
    this.menu.hide();
    try {
      this.callback?.(evt);
    } catch (e) {
      console.error(e);
    }
  }

  openSubmenu() {
    if (!this.submenu) return;
    const r = this.dom.getBoundingClientRect();
    this.submenu.parentMenu = this.menu;
    this.submenu.showAtPosition({ x: r.right - 4, y: r.top - 4 });
  }

  setTitle(title: string | DocumentFragment): this {
    this.titleEl.setText(title as any);
    return this;
  }
  setIcon(icon: string | null): this {
    if (icon) setIcon(this.iconEl, icon);
    else this.iconEl.empty();
    return this;
  }
  setChecked(checked: boolean | null): this {
    this.dom.toggleClass("mod-checked", !!checked);
    if (checked) {
      if (!this.checkIconEl) this.checkIconEl = this.dom.createDiv("menu-item-icon mod-checked");
      setIcon(this.checkIconEl, "check");
    } else this.checkIconEl?.detach(), (this.checkIconEl = undefined);
    return this;
  }
  setDisabled(disabled: boolean): this {
    this.disabled = disabled;
    this.dom.toggleClass("is-disabled", disabled);
    return this;
  }
  setWarning(isWarning: boolean): this {
    this.dom.toggleClass("is-warning", isWarning);
    return this;
  }
  setIsLabel(isLabel: boolean): this {
    this.dom.toggleClass("is-label", isLabel);
    return this;
  }
  setSection(section: string): this {
    this.section = section;
    return this;
  }
  setSubmenu(): Menu {
    this.submenu = new Menu();
    this.dom.addClass("has-submenu");
    const arrow = this.dom.createDiv("menu-item-icon mod-submenu");
    setIcon(arrow, "chevron-right");
    return this.submenu;
  }
  onClick(callback: (evt: MouseEvent | KeyboardEvent) => any): this {
    this.callback = callback;
    return this;
  }
}

export class MenuSeparator {
  dom = createDiv("menu-separator");
  section = "";
}

const SECTION_ORDER = ["title", "correction", "spellcheck", "open", "selection", "clipboard", "action", "view", "info", "", "danger", "system"];

export class Menu extends Component {
  dom: HTMLElement;
  scrollEl: HTMLElement;
  items: (MenuItem | MenuSeparator)[] = [];
  parentMenu: Menu | null = null;
  selected = -1;
  private hideCallbacks: (() => any)[] = [];
  private onDocClick = (e: MouseEvent) => {
    if (!this.dom.contains(e.target as Node) && !this.childContains(e.target as Node)) this.hide();
  };
  private onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); this.hide(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); this.move(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); this.move(-1); }
    else if (e.key === "Enter") {
      e.preventDefault();
      const it = this.items[this.selected];
      if (it instanceof MenuItem) it.handleEvent(e);
    }
  };

  constructor() {
    super();
    this.dom = createDiv("menu");
    this.scrollEl = this.dom.createDiv("menu-scroll");
    this.dom.addEventListener("contextmenu", (e) => e.preventDefault());
  }

  private childContains(n: Node): boolean {
    return this.items.some((i) => i instanceof MenuItem && i.submenu && (i.submenu.dom.contains(n) || (i.submenu as any).childContains(n)));
  }

  setNoIcon(): this {
    this.dom.addClass("mod-no-icon");
    return this;
  }
  setUseNativeMenu(_b: boolean): this {
    return this;
  }
  addItem(cb: (item: MenuItem) => any): this {
    const item = new MenuItem(this);
    this.items.push(item);
    cb(item);
    return this;
  }
  addSeparator(): this {
    this.items.push(new MenuSeparator());
    return this;
  }
  /** @internal */ select(i: number) {
    this.items.forEach((it, j) => it instanceof MenuItem && it.dom.toggleClass("selected", i === j));
    this.selected = i;
  }
  private move(d: number) {
    const n = this.items.length;
    for (let k = 1; k <= n; k++) {
      const i = (this.selected + d * k + n * 10) % n;
      const it = this.items[i];
      if (it instanceof MenuItem && !it.disabled) return this.select(i);
    }
  }

  private render() {
    this.scrollEl.empty();
    // Stable sort by section, then insert separators between sections.
    const withIndex = this.items.map((it, i) => ({ it, i }));
    const rank = (s: string) => {
      const base = s.split(".")[0];
      const r = SECTION_ORDER.indexOf(base);
      return r === -1 ? SECTION_ORDER.indexOf("") : r;
    };
    withIndex.sort((a, b) => rank(a.it.section) - rank(b.it.section) || a.i - b.i);
    const ordered: (MenuItem | MenuSeparator)[] = [];
    let last: string | null = null;
    for (const { it } of withIndex) {
      if (it instanceof MenuSeparator) {
        if (ordered.length && !(ordered.last() instanceof MenuSeparator)) ordered.push(it);
        continue;
      }
      if (last !== null && it.section !== last && ordered.length && !(ordered.last() instanceof MenuSeparator)) ordered.push(new MenuSeparator());
      ordered.push(it);
      last = it.section;
    }
    while (ordered.last() instanceof MenuSeparator) ordered.pop();
    this.items = ordered;
    for (const it of ordered) this.scrollEl.appendChild(it.dom);
  }

  showAtMouseEvent(evt: MouseEvent): this {
    evt.preventDefault?.();
    return this.showAtPosition({ x: evt.clientX, y: evt.clientY });
  }

  showAtPosition(pos: { x: number; y: number; width?: number; overlap?: boolean; left?: boolean }, _doc?: Document): this {
    if (!this.parentMenu && openMenu && openMenu !== this) openMenu.hide();
    if (!this.parentMenu) openMenu = this;
    this.render();
    if (this.items.length === 0) return this;
    document.body.appendChild(this.dom);
    this.load();
    const r = this.dom.getBoundingClientRect();
    let x = pos.left ? pos.x - r.width : pos.x;
    let y = pos.y;
    if (x + r.width > window.innerWidth - 4) x = Math.max(4, window.innerWidth - r.width - 4);
    if (y + r.height > window.innerHeight - 4) y = Math.max(4, window.innerHeight - r.height - 4);
    this.dom.style.left = x + "px";
    this.dom.style.top = y + "px";
    setTimeout(() => {
      document.addEventListener("mousedown", this.onDocClick, true);
      if (!this.parentMenu) document.addEventListener("keydown", this.onKey, true);
    });
    return this;
  }

  hide(): void {
    if (!this.dom.isConnected) return;
    for (const it of this.items) if (it instanceof MenuItem && it.submenu) it.submenu.hide();
    this.dom.detach();
    document.removeEventListener("mousedown", this.onDocClick, true);
    document.removeEventListener("keydown", this.onKey, true);
    if (openMenu === this) openMenu = null;
    this.unload();
    for (const cb of this.hideCallbacks) cb();
    this.parentMenu?.hide();
  }
  close(): void {
    this.hide();
  }
  onHide(cb: () => any): void {
    this.hideCallbacks.push(cb);
  }

  static forEvent(evt: MouseEvent): Menu {
    const m = new Menu();
    setTimeout(() => m.showAtMouseEvent(evt));
    return m;
  }
}

// ── components ────────────────────────────────────────────────────────────

export abstract class BaseComponent {
  disabled = false;
  then(cb: (component: this) => any): this {
    cb(this);
    return this;
  }
  setDisabled(disabled: boolean): this {
    this.disabled = disabled;
    return this;
  }
}

export abstract class ValueComponent<T> extends BaseComponent {
  registerOptionListener(listeners: Record<string, (value?: T) => T>, key: string): this {
    listeners[key] = (value?: T) => {
      if (value !== undefined) this.setValue(value);
      return this.getValue();
    };
    return this;
  }
  abstract getValue(): T;
  abstract setValue(value: T): this;
}

export abstract class AbstractTextComponent<T extends HTMLInputElement | HTMLTextAreaElement> extends ValueComponent<string> {
  inputEl: T;
  changeCallback?: (value: string) => any;
  constructor(inputEl: T) {
    super();
    this.inputEl = inputEl;
    inputEl.addEventListener("input", () => this.onChanged());
  }
  setDisabled(disabled: boolean): this {
    super.setDisabled(disabled);
    this.inputEl.disabled = disabled;
    return this;
  }
  getValue(): string {
    return this.inputEl.value;
  }
  setValue(value: string): this {
    this.inputEl.value = value ?? "";
    return this;
  }
  setPlaceholder(placeholder: string): this {
    this.inputEl.placeholder = placeholder;
    return this;
  }
  onChanged(): void {
    this.changeCallback?.(this.getValue());
  }
  onChange(callback: (value: string) => any): this {
    this.changeCallback = callback;
    return this;
  }
}

export class TextComponent extends AbstractTextComponent<HTMLInputElement> {
  constructor(containerEl: HTMLElement) {
    super(containerEl.createEl("input", { type: "text", attr: { spellcheck: "false" } }));
  }
}

export class TextAreaComponent extends AbstractTextComponent<HTMLTextAreaElement> {
  constructor(containerEl: HTMLElement) {
    super(containerEl.createEl("textarea"));
  }
}

export class SearchComponent extends AbstractTextComponent<HTMLInputElement> {
  clearButtonEl: HTMLElement;
  containerEl: HTMLElement;
  constructor(containerEl: HTMLElement) {
    const wrap = containerEl.createDiv("search-input-container");
    super(wrap.createEl("input", { type: "search", attr: { enterkeyhint: "search", spellcheck: "false" } }));
    this.containerEl = wrap;
    this.clearButtonEl = wrap.createDiv("search-input-clear-button");
    this.clearButtonEl.addEventListener("click", () => {
      this.setValue("");
      this.onChanged();
      this.inputEl.focus();
    });
    this.inputEl.addEventListener("input", () => this.updateClear());
    this.updateClear();
  }
  private updateClear() {
    this.clearButtonEl.toggle(!!this.inputEl.value);
  }
  setValue(value: string): this {
    super.setValue(value);
    this.updateClear?.();
    return this;
  }
  setClass(cls: string): this {
    this.containerEl.addClass(cls);
    return this;
  }
}

export class MomentFormatComponent extends TextComponent {
  sampleEl!: HTMLElement;
  defaultFormat = "";
  setDefaultFormat(f: string): this {
    this.defaultFormat = f;
    this.setPlaceholder(f);
    this.updateSample();
    return this;
  }
  setSampleEl(el: HTMLElement): this {
    this.sampleEl = el;
    this.updateSample();
    return this;
  }
  setValue(value: string): this {
    super.setValue(value);
    this.updateSample();
    return this;
  }
  onChanged(): void {
    this.updateSample();
    super.onChanged();
  }
  updateSample(): void {
    if (!this.sampleEl) return;
    const m = (window as any).moment;
    this.sampleEl.setText(m ? m().format(this.getValue() || this.defaultFormat) : "");
  }
}

export class ToggleComponent extends ValueComponent<boolean> {
  toggleEl: HTMLElement;
  value = false;
  private cb?: (v: boolean) => any;
  constructor(containerEl: HTMLElement) {
    super();
    this.toggleEl = containerEl.createDiv("checkbox-container");
    this.toggleEl.createEl("input", { type: "checkbox", attr: { tabindex: "0" } });
    this.toggleEl.addEventListener("click", (e) => this.onClick(e));
  }
  setDisabled(disabled: boolean): this {
    super.setDisabled(disabled);
    this.toggleEl.toggleClass("is-disabled", disabled);
    return this;
  }
  getValue(): boolean {
    return this.value;
  }
  setValue(on: boolean): this {
    this.value = !!on;
    this.toggleEl.toggleClass("is-enabled", this.value);
    (this.toggleEl.querySelector("input") as HTMLInputElement).checked = this.value;
    return this;
  }
  setTooltip(tooltip: string, options?: any): this {
    setTooltip(this.toggleEl, tooltip, options);
    return this;
  }
  onClick(e?: Event): void {
    e?.preventDefault();
    if (this.disabled) return;
    this.setValue(!this.value);
    this.cb?.(this.value);
  }
  onChange(callback: (value: boolean) => any): this {
    this.cb = callback;
    return this;
  }
}

export class DropdownComponent extends ValueComponent<string> {
  selectEl: HTMLSelectElement;
  private cb?: (v: string) => any;
  constructor(containerEl: HTMLElement) {
    super();
    this.selectEl = containerEl.createEl("select", "dropdown");
    this.selectEl.addEventListener("change", () => this.cb?.(this.selectEl.value));
  }
  setDisabled(disabled: boolean): this {
    super.setDisabled(disabled);
    this.selectEl.disabled = disabled;
    return this;
  }
  addOption(value: string, display: string): this {
    this.selectEl.createEl("option", { value, text: display });
    return this;
  }
  addOptions(options: Record<string, string>): this {
    for (const k in options) this.addOption(k, options[k]);
    return this;
  }
  getValue(): string {
    return this.selectEl.value;
  }
  setValue(value: string): this {
    this.selectEl.value = value;
    return this;
  }
  onChange(callback: (value: string) => any): this {
    this.cb = callback;
    return this;
  }
}

export class SliderComponent extends ValueComponent<number> {
  sliderEl: HTMLInputElement;
  private cb?: (v: number) => any;
  private instant = true;
  constructor(containerEl: HTMLElement) {
    super();
    this.sliderEl = containerEl.createEl("input", { type: "range", cls: "slider" });
    this.sliderEl.addEventListener("input", () => this.instant && this.cb?.(this.getValue()));
    this.sliderEl.addEventListener("change", () => !this.instant && this.cb?.(this.getValue()));
  }
  setDisabled(disabled: boolean): this {
    super.setDisabled(disabled);
    this.sliderEl.disabled = disabled;
    return this;
  }
  setInstant(instant: boolean): this {
    this.instant = instant;
    return this;
  }
  setLimits(min: number | null, max: number | null, step: number | "any"): this {
    if (min !== null) this.sliderEl.min = String(min);
    if (max !== null) this.sliderEl.max = String(max);
    this.sliderEl.step = String(step);
    return this;
  }
  getValuePretty(): string {
    return String(this.getValue());
  }
  setDynamicTooltip(): this {
    const upd = () => this.sliderEl.setAttribute("aria-label", this.getValuePretty());
    this.sliderEl.addEventListener("input", upd);
    upd();
    return this;
  }
  showTooltip(): void {}
  getValue(): number {
    return Number(this.sliderEl.value);
  }
  setValue(value: number): this {
    this.sliderEl.value = String(value);
    return this;
  }
  onChange(callback: (value: number) => any): this {
    this.cb = callback;
    return this;
  }
}

export class ButtonComponent extends BaseComponent {
  buttonEl: HTMLButtonElement;
  private cb?: (evt: MouseEvent) => any;
  constructor(containerEl: HTMLElement) {
    super();
    this.buttonEl = containerEl.createEl("button");
    this.buttonEl.addEventListener("click", (e) => !this.disabled && this.cb?.(e));
  }
  setDisabled(disabled: boolean): this {
    super.setDisabled(disabled);
    this.buttonEl.disabled = disabled;
    return this;
  }
  setCta(): this {
    this.buttonEl.addClass("mod-cta");
    return this;
  }
  removeCta(): this {
    this.buttonEl.removeClass("mod-cta");
    return this;
  }
  setWarning(): this {
    this.buttonEl.addClass("mod-warning");
    return this;
  }
  setTooltip(tooltip: string, options?: any): this {
    setTooltip(this.buttonEl, tooltip, options);
    return this;
  }
  setButtonText(name: string): this {
    this.buttonEl.setText(name);
    return this;
  }
  setIcon(icon: string): this {
    setIcon(this.buttonEl, icon);
    return this;
  }
  setClass(cls: string): this {
    this.buttonEl.addClass(cls);
    return this;
  }
  onClick(callback: (evt: MouseEvent) => any): this {
    this.cb = callback;
    return this;
  }
}

export class ExtraButtonComponent extends BaseComponent {
  extraSettingsEl: HTMLElement;
  private cb?: () => any;
  constructor(containerEl: HTMLElement) {
    super();
    this.extraSettingsEl = containerEl.createDiv("clickable-icon extra-setting-button");
    this.extraSettingsEl.addEventListener("click", () => !this.disabled && this.cb?.());
  }
  setDisabled(disabled: boolean): this {
    super.setDisabled(disabled);
    this.extraSettingsEl.toggleClass("is-disabled", disabled);
    return this;
  }
  setTooltip(tooltip: string, options?: any): this {
    setTooltip(this.extraSettingsEl, tooltip, options);
    return this;
  }
  setIcon(icon: string): this {
    setIcon(this.extraSettingsEl, icon);
    return this;
  }
  onClick(callback: () => any): this {
    this.cb = callback;
    return this;
  }
}

export class ColorComponent extends ValueComponent<string> {
  colorPickerEl: HTMLInputElement;
  private cb?: (v: string) => any;
  constructor(containerEl: HTMLElement) {
    super();
    this.colorPickerEl = containerEl.createEl("input", { type: "color" });
    this.colorPickerEl.addEventListener("input", () => this.cb?.(this.getValue()));
  }
  getValue(): string {
    return this.colorPickerEl.value;
  }
  getValueRgb() {
    const h = this.getValue().replace("#", "");
    return { r: parseInt(h.slice(0, 2), 16), g: parseInt(h.slice(2, 4), 16), b: parseInt(h.slice(4, 6), 16) };
  }
  getValueHsl() {
    let { r, g, b } = this.getValueRgb();
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b);
    let h = 0, s = 0;
    const l = (max + min) / 2;
    if (max !== min) {
      const d = max - min;
      s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
      h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
      h /= 6;
    }
    return { h: h * 360, s, l };
  }
  setValue(value: string): this {
    this.colorPickerEl.value = value;
    return this;
  }
  setValueRgb(rgb: { r: number; g: number; b: number }): this {
    const hex = (n: number) => Math.round(n).toString(16).padStart(2, "0");
    return this.setValue(`#${hex(rgb.r)}${hex(rgb.g)}${hex(rgb.b)}`);
  }
  setValueHsl(hsl: { h: number; s: number; l: number }): this {
    const { h, s, l } = hsl;
    const a = s * Math.min(l, 1 - l);
    const f = (n: number) => {
      const k = (n + h / 30) % 12;
      return 255 * (l - a * Math.max(Math.min(k - 3, 9 - k, 1), -1));
    };
    return this.setValueRgb({ r: f(0), g: f(8), b: f(4) });
  }
  onChange(callback: (value: string) => any): this {
    this.cb = callback;
    return this;
  }
}

export class ProgressBarComponent extends ValueComponent<number> {
  progressBar: HTMLElement;
  lineEl: HTMLElement;
  value = 0;
  constructor(containerEl: HTMLElement) {
    super();
    this.progressBar = containerEl.createDiv("setting-progress-bar");
    this.lineEl = this.progressBar.createDiv("setting-progress-bar-inner");
  }
  getValue(): number {
    return this.value;
  }
  setValue(value: number): this {
    this.value = value;
    this.lineEl.style.width = `${Math.clamp(value, 0, 100)}%`;
    return this;
  }
}

export class SecretComponent extends TextComponent {
  constructor(app: any, containerEl: HTMLElement) {
    super(containerEl);
    this.inputEl.type = "password";
    void app;
  }
}

export class Setting {
  settingEl: HTMLElement;
  infoEl: HTMLElement;
  nameEl: HTMLElement;
  descEl: HTMLElement;
  controlEl: HTMLElement;
  components: BaseComponent[] = [];

  constructor(containerEl: HTMLElement) {
    this.settingEl = containerEl.createDiv("setting-item");
    this.infoEl = this.settingEl.createDiv("setting-item-info");
    this.nameEl = this.infoEl.createDiv("setting-item-name");
    this.descEl = this.infoEl.createDiv("setting-item-description");
    this.controlEl = this.settingEl.createDiv("setting-item-control");
  }

  setName(name: string | DocumentFragment): this {
    this.nameEl.setText(name as any);
    return this;
  }
  setDesc(desc: string | DocumentFragment): this {
    this.descEl.setText(desc as any);
    return this;
  }
  setClass(cls: string): this {
    this.settingEl.addClass(cls);
    return this;
  }
  setTooltip(tooltip: string, options?: any): this {
    setTooltip(this.nameEl, tooltip, options);
    return this;
  }
  setHeading(): this {
    this.settingEl.addClass("setting-item-heading");
    return this;
  }
  setDisabled(disabled: boolean): this {
    this.settingEl.toggleClass("is-disabled", disabled);
    this.components.forEach((c) => c.setDisabled(disabled));
    return this;
  }
  private add<C extends BaseComponent>(c: C, cb: (c: C) => any): this {
    this.components.push(c);
    cb(c);
    return this;
  }
  addButton(cb: (c: ButtonComponent) => any): this {
    return this.add(new ButtonComponent(this.controlEl), cb);
  }
  addExtraButton(cb: (c: ExtraButtonComponent) => any): this {
    return this.add(new ExtraButtonComponent(this.controlEl), cb);
  }
  addToggle(cb: (c: ToggleComponent) => any): this {
    return this.add(new ToggleComponent(this.controlEl), cb);
  }
  addText(cb: (c: TextComponent) => any): this {
    return this.add(new TextComponent(this.controlEl), cb);
  }
  addSearch(cb: (c: SearchComponent) => any): this {
    return this.add(new SearchComponent(this.controlEl), cb);
  }
  addTextArea(cb: (c: TextAreaComponent) => any): this {
    return this.add(new TextAreaComponent(this.controlEl), cb);
  }
  addMomentFormat(cb: (c: MomentFormatComponent) => any): this {
    return this.add(new MomentFormatComponent(this.controlEl), cb);
  }
  addDropdown(cb: (c: DropdownComponent) => any): this {
    return this.add(new DropdownComponent(this.controlEl), cb);
  }
  addColorPicker(cb: (c: ColorComponent) => any): this {
    return this.add(new ColorComponent(this.controlEl), cb);
  }
  addProgressBar(cb: (c: ProgressBarComponent) => any): this {
    return this.add(new ProgressBarComponent(this.controlEl), cb);
  }
  addSlider(cb: (c: SliderComponent) => any): this {
    return this.add(new SliderComponent(this.controlEl), cb);
  }
  addComponent<T extends BaseComponent>(cb: (el: HTMLElement) => T): this {
    const c = cb(this.controlEl);
    if (c) this.components.push(c);
    return this;
  }
  then(cb: (setting: this) => any): this {
    cb(this);
    return this;
  }
  clear(): this {
    this.controlEl.empty();
    this.components = [];
    return this;
  }
}

export class SettingGroup {
  containerEl: HTMLElement;
  headingEl: HTMLElement;
  itemsEl: HTMLElement;
  constructor(containerEl: HTMLElement) {
    this.containerEl = containerEl.createDiv("setting-group");
    this.headingEl = this.containerEl.createDiv("setting-item setting-item-heading");
    this.itemsEl = this.containerEl.createDiv("setting-items");
  }
  setHeading(text: string | DocumentFragment): this {
    this.headingEl.setText(text as any);
    return this;
  }
  addSetting(cb: (s: Setting) => any): this {
    cb(new Setting(this.itemsEl));
    return this;
  }
}

// ── suggestions ───────────────────────────────────────────────────────────

class Chooser<T> {
  owner: any;
  containerEl: HTMLElement;
  values: T[] = [];
  suggestions: HTMLElement[] = [];
  selectedItem = 0;

  constructor(owner: any, containerEl: HTMLElement, scope: Scope) {
    this.owner = owner;
    this.containerEl = containerEl;
    containerEl.on("click", ".suggestion-item", (e: MouseEvent, el: HTMLElement) => {
      e.preventDefault();
      const i = this.suggestions.indexOf(el);
      this.setSelectedItem(i, e);
      this.useSelectedItem(e);
    });
    containerEl.on("mousemove", ".suggestion-item", (_e: MouseEvent, el: HTMLElement) => {
      const i = this.suggestions.indexOf(el);
      if (i !== this.selectedItem) this.setSelectedItem(i, null);
    });
    scope.register([], "ArrowUp", (e) => { if (!e.isComposing) { this.moveUp(e); return false; } });
    scope.register([], "ArrowDown", (e) => { if (!e.isComposing) { this.moveDown(e); return false; } });
    scope.register(["Ctrl"], "p", (e) => { this.moveUp(e); return false; });
    scope.register(["Ctrl"], "n", (e) => { this.moveDown(e); return false; });
    scope.register([], "Enter", (e) => { if (!e.isComposing) { this.useSelectedItem(e); return false; } });
    scope.register(["Mod"], "Enter", (e) => { if (!e.isComposing) { this.useSelectedItem(e); return false; } });
    scope.register(["Shift"], "Enter", (e) => { if (!e.isComposing) { this.useSelectedItem(e); return false; } });
  }

  moveUp(e: any) {
    if (this.values.length) this.setSelectedItem((this.selectedItem - 1 + this.values.length) % this.values.length, e);
  }
  moveDown(e: any) {
    if (this.values.length) this.setSelectedItem((this.selectedItem + 1) % this.values.length, e);
  }

  setSuggestions(values: T[] | null) {
    this.containerEl.empty();
    this.suggestions = [];
    this.values = values || [];
    for (const v of this.values) {
      const el = this.containerEl.createDiv("suggestion-item");
      this.owner.renderSuggestion(v, el);
      this.suggestions.push(el);
    }
    this.setSelectedItem(0, null);
  }

  setSelectedItem(i: number, evt: any) {
    if (i < 0 || i >= this.suggestions.length) {
      this.selectedItem = 0;
      return;
    }
    this.suggestions[this.selectedItem]?.removeClass("is-selected");
    this.selectedItem = i;
    const el = this.suggestions[i];
    el.addClass("is-selected");
    if (evt) el.scrollIntoView({ block: "nearest" });
  }

  useSelectedItem(evt: any): boolean {
    const v = this.values[this.selectedItem];
    if (v === undefined) return false;
    this.owner.selectSuggestion(v, evt);
    return true;
  }
}

export abstract class SuggestModal<T> extends Modal {
  limit = 100;
  emptyStateText = "No results found.";
  inputEl: HTMLInputElement;
  resultContainerEl: HTMLElement;
  instructionsEl: HTMLElement;
  chooser: Chooser<T>;
  private updateToken = 0;

  constructor(app: any) {
    super(app);
    this.containerEl.removeClass("mod-dim");
    this.modalEl.className = "prompt";
    this.modalEl.empty();
    const inputWrap = this.modalEl.createDiv("prompt-input-container");
    this.inputEl = inputWrap.createEl("input", { cls: "prompt-input", attr: { enterkeyhint: "done", spellcheck: "false" } });
    inputWrap.createDiv("prompt-input-cta");
    this.resultContainerEl = this.modalEl.createDiv("prompt-results");
    this.instructionsEl = this.modalEl.createDiv("prompt-instructions");
    this.chooser = new Chooser<T>(this, this.resultContainerEl, this.scope);
    this.inputEl.addEventListener("input", () => this.onInput());
  }

  setPlaceholder(placeholder: string): void {
    this.inputEl.placeholder = placeholder;
  }

  setInstructions(instructions: { command: string; purpose: string }[]): void {
    this.instructionsEl.empty();
    for (const i of instructions) {
      const d = this.instructionsEl.createDiv("prompt-instruction");
      d.createSpan({ cls: "prompt-instruction-command", text: i.command });
      d.createSpan({ text: i.purpose });
    }
  }

  onOpen(): void {
    this.inputEl.focus();
    this.onInput();
  }

  onInput(): void {
    const token = ++this.updateToken;
    const r: any = this.getSuggestions(this.inputEl.value);
    const apply = (list: T[]) => {
      if (token !== this.updateToken) return;
      list = (list || []).slice(0, this.limit);
      if (list.length === 0) {
        this.chooser.setSuggestions(null);
        this.onNoSuggestion();
      } else this.chooser.setSuggestions(list);
    };
    if (r && typeof r.then === "function") r.then(apply);
    else apply(r);
  }

  onNoSuggestion(): void {
    this.resultContainerEl.empty();
    this.resultContainerEl.createDiv({ cls: "suggestion-empty", text: this.emptyStateText });
  }

  selectSuggestion(value: T, evt: MouseEvent | KeyboardEvent): void {
    this.close();
    this.onChooseSuggestion(value, evt);
  }

  abstract getSuggestions(query: string): T[] | Promise<T[]>;
  abstract renderSuggestion(value: T, el: HTMLElement): any;
  abstract onChooseSuggestion(item: T, evt: MouseEvent | KeyboardEvent): any;
}

export interface FuzzyMatch<T> {
  item: T;
  match: { score: number; matches: [number, number][] };
}

export abstract class FuzzySuggestModal<T> extends SuggestModal<FuzzyMatch<T>> {
  getSuggestions(query: string): FuzzyMatch<T>[] {
    const items = this.getItems();
    if (!query.trim()) return items.map((item) => ({ item, match: { score: 0, matches: [] } }));
    const fuzzy = prepareFuzzySearch(query);
    const out: FuzzyMatch<T>[] = [];
    for (const item of items) {
      const m = fuzzy(this.getItemText(item));
      if (m) out.push({ item, match: m });
    }
    out.sort((a, b) => b.match.score - a.match.score);
    return out;
  }
  renderSuggestion(item: FuzzyMatch<T>, el: HTMLElement): void {
    renderMatches(el, this.getItemText(item.item), item.match.matches);
  }
  onChooseSuggestion(item: FuzzyMatch<T>, evt: MouseEvent | KeyboardEvent): void {
    this.onChooseItem(item.item, evt);
  }
  abstract getItems(): T[];
  abstract getItemText(item: T): string;
  abstract onChooseItem(item: T, evt: MouseEvent | KeyboardEvent): void;
}

export abstract class PopoverSuggest<T> {
  app: any;
  scope: Scope;
  suggestEl: HTMLElement;
  suggestions: HTMLElement;
  isOpen = false;
  chooser: Chooser<T>;

  constructor(app: any, scope?: Scope) {
    this.app = app;
    this.scope = scope || new Scope(app?.scope);
    this.scope.register([], "Escape", () => {
      this.close();
      return false;
    });
    this.suggestEl = createDiv("suggestion-container");
    this.suggestions = this.suggestEl.createDiv("suggestion");
    this.chooser = new Chooser<T>(this, this.suggestions, this.scope);
    this.suggestEl.addEventListener("mousedown", (e) => e.preventDefault());
  }

  open(): void {
    if (this.isOpen) return;
    this.isOpen = true;
    document.body.appendChild(this.suggestEl);
    this.app?.keymap?.pushScope(this.scope);
  }

  close(): void {
    if (!this.isOpen) return;
    this.isOpen = false;
    this.app?.keymap?.popScope(this.scope);
    this.chooser.setSuggestions([]);
    this.suggestEl.detach();
  }

  /** @internal */ reposition(rect: { left: number; right?: number; top: number; bottom: number }) {
    const el = this.suggestEl;
    el.style.left = "0px";
    el.style.top = "0px";
    const r = el.getBoundingClientRect();
    let left = rect.left;
    let top = rect.bottom + 4;
    if (left + r.width > window.innerWidth - 8) left = Math.max(8, window.innerWidth - r.width - 8);
    if (top + r.height > window.innerHeight - 8) top = Math.max(8, rect.top - r.height - 4);
    el.style.left = left + "px";
    el.style.top = top + "px";
  }

  abstract renderSuggestion(value: T, el: HTMLElement): void;
  abstract selectSuggestion(value: T, evt: MouseEvent | KeyboardEvent): void;
}

export abstract class AbstractInputSuggest<T> extends PopoverSuggest<T> {
  limit = 100;
  textInputEl: HTMLInputElement | HTMLDivElement;
  private selectCb?: (value: T, evt: MouseEvent | KeyboardEvent) => any;

  constructor(app: any, textInputEl: HTMLInputElement | HTMLDivElement) {
    super(app);
    this.textInputEl = textInputEl;
    textInputEl.addEventListener("input", () => this.onInputChanged());
    textInputEl.addEventListener("focus", () => this.onInputChanged());
    textInputEl.addEventListener("blur", () => this.close());
  }

  setValue(value: string): void {
    if (this.textInputEl instanceof HTMLInputElement) this.textInputEl.value = value;
    else this.textInputEl.textContent = value;
  }
  getValue(): string {
    return this.textInputEl instanceof HTMLInputElement ? this.textInputEl.value : this.textInputEl.textContent || "";
  }
  onSelect(cb: (value: T, evt: MouseEvent | KeyboardEvent) => any): this {
    this.selectCb = cb;
    return this;
  }
  selectSuggestion(value: T, evt: MouseEvent | KeyboardEvent): void {
    this.selectCb?.(value, evt);
  }

  /** @internal */ onInputChanged(): void {
    const r: any = this.getSuggestions(this.getValue());
    const apply = (list: T[]) => {
      list = (list || []).slice(0, this.limit);
      if (!list.length) return this.close();
      this.open();
      this.chooser.setSuggestions(list);
      this.reposition(this.textInputEl.getBoundingClientRect());
    };
    if (r && typeof r.then === "function") r.then(apply);
    else apply(r);
  }

  protected abstract getSuggestions(query: string): T[] | Promise<T[]>;
}

// ── confirm helper ────────────────────────────────────────────────────────

export class ConfirmationModal extends Modal {
  private resolve!: (v: boolean) => void;
  result: Promise<boolean>;
  constructor(app: any, title: string, message: string, cta = "Delete", warning = true) {
    super(app);
    this.result = new Promise((r) => (this.resolve = r));
    this.setTitle(title);
    if (message) this.contentEl.createEl("p", { text: message });
    const buttons = this.modalEl.createDiv("modal-button-container");
    const ok = buttons.createEl("button", { text: cta, cls: warning ? "mod-warning" : "mod-cta" });
    const cancel = buttons.createEl("button", { text: "Cancel" });
    ok.addEventListener("click", () => {
      this.resolve(true);
      this.close();
    });
    cancel.addEventListener("click", () => this.close());
    this.scope.register([], "Enter", () => {
      this.resolve(true);
      this.close();
      return false;
    });
  }
  onClose() {
    this.resolve(false);
  }
}

export class ConfirmationButton extends ButtonComponent {}
