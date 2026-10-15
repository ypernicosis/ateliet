// Icons: Obsidian ships Lucide (ids like `lucide-file-text`, or bare
// `file-text`) plus a set of legacy ids. Plugins can add their own with
// `addIcon(id, svgContent)` using a 0 0 100 100 viewBox.

import * as lucide from "lucide";

const custom: Record<string, string> = {};

const LEGACY: Record<string, string> = {
  "any-key": "keyboard",
  "audio-file": "file-audio",
  "blocks": "blocks",
  "bold-glyph": "bold",
  "bracket-glyph": "brackets",
  "broken-link": "unlink",
  "bullet-list": "list",
  "bullet-list-glyph": "list",
  "calendar-with-checkmark": "calendar-check",
  "check-in-circle": "check-circle",
  "check-small": "check",
  "checkbox-glyph": "check-square",
  "checkmark": "check",
  "clock": "clock",
  "cloud": "cloud",
  "code-glyph": "code",
  "create-new": "file-plus",
  "cross": "x",
  "cross-in-box": "x-square",
  "crossed-star": "star-off",
  "csv": "file-spreadsheet",
  "deleteColumn": "minus",
  "deleteRow": "minus",
  "dice": "dice-5",
  "document": "file",
  "documents": "files",
  "dot-network": "git-fork",
  "double-down-arrow-glyph": "chevrons-down",
  "double-up-arrow-glyph": "chevrons-up",
  "down-arrow-with-tail": "arrow-down",
  "down-chevron-glyph": "chevron-down",
  "enter": "corner-down-left",
  "exit-fullscreen": "minimize",
  "expand-vertically": "unfold-vertical",
  "filled-pin": "pin",
  "folder": "folder",
  "formula": "sigma",
  "forward-arrow": "arrow-right",
  "fullscreen": "maximize",
  "gear": "settings",
  "go-to-file": "file-search",
  "hashtag": "hash",
  "heading-glyph": "heading",
  "help": "help-circle",
  "highlight-glyph": "highlighter",
  "horizontal-split": "separator-horizontal",
  "image-file": "file-image",
  "image-glyph": "image",
  "indent-glyph": "indent",
  "info": "info",
  "install": "download",
  "italic-glyph": "italic",
  "keyboard-glyph": "keyboard",
  "languages": "languages",
  "left-arrow": "arrow-left",
  "left-arrow-with-tail": "arrow-left",
  "left-chevron-glyph": "chevron-left",
  "lines-of-text": "align-left",
  "link": "link",
  "link-glyph": "link",
  "logo-crystal": "gem",
  "magnifying-glass": "search",
  "microphone": "mic",
  "microphone-filled": "mic",
  "minus-with-circle": "minus-circle",
  "moveColumnLeft": "chevron-left",
  "moveColumnRight": "chevron-right",
  "moveRowDown": "chevron-down",
  "moveRowUp": "chevron-up",
  "note-glyph": "sticky-note",
  "number-list-glyph": "list-ordered",
  "open-vault": "folder-open",
  "pane-layout": "layout",
  "paper-plane": "send",
  "paused": "pause",
  "pdf-file": "file-text",
  "pencil": "pencil",
  "percent-sign-glyph": "percent",
  "pin": "pin",
  "plus-with-circle": "plus-circle",
  "popup-open": "external-link",
  "presentation": "presentation",
  "price-tag-glyph": "tag",
  "quote-glyph": "quote",
  "redo-glyph": "redo",
  "reset": "rotate-ccw",
  "right-arrow": "arrow-right",
  "right-arrow-with-tail": "arrow-right",
  "right-chevron-glyph": "chevron-right",
  "right-triangle": "chevron-right",
  "run-command": "terminal-square",
  "search": "search",
  "sheets-in-box": "archive",
  "sortAsc": "sort-asc",
  "sortDesc": "sort-desc",
  "spreadsheet": "sheet",
  "stacked-levels": "layers",
  "star": "star",
  "star-list": "list-checks",
  "strikethrough-glyph": "strikethrough",
  "switch": "repeat",
  "sync": "refresh-cw",
  "sync-small": "refresh-cw",
  "tag-glyph": "tag",
  "three-horizontal-bars": "menu",
  "trash": "trash-2",
  "undo-glyph": "undo",
  "unindent-glyph": "outdent",
  "up-and-down-arrows": "arrow-up-down",
  "up-arrow-with-tail": "arrow-up",
  "up-chevron-glyph": "chevron-up",
  "uppercase-lowercase-a": "case-sensitive",
  "vault": "vault",
  "vertical-split": "columns-2",
  "vertical-three-dots": "more-vertical",
  "wrench-screwdriver-glyph": "wrench",
};

const pascal = (id: string) => id.split(/[-_]/).filter(Boolean).map((p) => p[0].toUpperCase() + p.slice(1)).join("");

function lucideNode(id: string): any | null {
  const L = lucide as any;
  return L[pascal(id)] || L.icons?.[pascal(id)] || null;
}

function build(node: any[]): SVGSVGElement {
  const [tag, attrs, children] = node;
  const el = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const k in attrs) el.setAttribute(k, String(attrs[k]));
  for (const c of children || []) el.appendChild(build(c));
  return el as SVGSVGElement;
}

export function getIcon(iconId: string): SVGSVGElement | null {
  if (!iconId) return null;
  if (custom[iconId] !== undefined) {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("xmlns", "http://www.w3.org/2000/svg");
    svg.setAttribute("viewBox", "0 0 100 100");
    svg.setAttribute("width", "24");
    svg.setAttribute("height", "24");
    svg.setAttribute("fill", "currentColor");
    svg.innerHTML = custom[iconId];
    svg.classList.add("svg-icon", iconId);
    return svg;
  }
  const id = iconId.startsWith("lucide-") ? iconId.slice(7) : iconId;
  const node = lucideNode(id) || (LEGACY[id] && lucideNode(LEGACY[id]));
  if (!node) return null;
  const svg = build(node);
  svg.classList.add("svg-icon", iconId.startsWith("lucide-") ? iconId : "lucide-" + (LEGACY[id] || id));
  return svg;
}

export function setIcon(parent: HTMLElement, iconId: string, _size?: number): void {
  parent.empty?.();
  const svg = getIcon(iconId);
  if (svg) parent.appendChild(svg);
}

export function addIcon(iconId: string, svgContent: string): void {
  custom[iconId] = svgContent;
}

export function removeIcon(iconId: string): void {
  delete custom[iconId];
}

export function getIconIds(): string[] {
  const L = lucide as any;
  const ids = Object.keys(L.icons || {}).map((k) => "lucide-" + k.replace(/([a-z0-9])([A-Z])/g, "$1-$2").replace(/([A-Z])([A-Z][a-z])/g, "$1-$2").toLowerCase());
  return [...ids, ...Object.keys(custom), ...Object.keys(LEGACY)];
}

// ── tooltips ──────────────────────────────────────────────────────────────

let tipEl: HTMLElement | null = null;
let tipTimer: any = null;

export function displayTooltip(target: HTMLElement, content: string | DocumentFragment, options: any = {}): void {
  hideTooltip();
  const el = document.createElement("div");
  el.className = "tooltip" + (options.classes ? " " + options.classes.join(" ") : "");
  if (typeof content === "string") el.textContent = content;
  else el.appendChild(content);
  const arrow = document.createElement("div");
  arrow.className = "tooltip-arrow";
  el.appendChild(arrow);
  document.body.appendChild(el);
  const r = target.getBoundingClientRect();
  const placement = options.placement || "bottom";
  const tr = el.getBoundingClientRect();
  let top = r.bottom + 8;
  let left = r.left + r.width / 2 - tr.width / 2;
  if (placement === "top") top = r.top - tr.height - 8;
  if (placement === "left") { top = r.top + r.height / 2 - tr.height / 2; left = r.left - tr.width - 8; }
  if (placement === "right") { top = r.top + r.height / 2 - tr.height / 2; left = r.right + 8; }
  el.addClass("mod-" + placement);
  el.style.top = Math.max(4, Math.min(window.innerHeight - tr.height - 4, top)) + "px";
  el.style.left = Math.max(4, Math.min(window.innerWidth - tr.width - 4, left)) + "px";
  tipEl = el;
}

export function hideTooltip(): void {
  clearTimeout(tipTimer);
  tipEl?.remove();
  tipEl = null;
}

export function setTooltip(el: HTMLElement, tooltip: string, options: any = {}): void {
  el.setAttribute("aria-label", tooltip);
  if (options.placement) el.setAttribute("data-tooltip-position", options.placement);
  if (options.delay !== undefined) el.setAttribute("data-tooltip-delay", String(options.delay));
}

// Global hover tooltips driven by `aria-label`, like Obsidian.
document.addEventListener("mouseover", (ev) => {
  const t = (ev.target as any)?.closest?.("[aria-label]") as HTMLElement | null;
  if (!t || t.closest(".tooltip") || t.hasAttribute("data-no-tooltip")) return;
  const label = t.getAttribute("aria-label");
  if (!label) return;
  clearTimeout(tipTimer);
  const delay = Number(t.getAttribute("data-tooltip-delay") ?? 500);
  tipTimer = setTimeout(() => {
    if (!t.isConnected || !t.matches(":hover")) return;
    displayTooltip(t, label, { placement: t.getAttribute("data-tooltip-position") || "bottom" });
  }, delay);
  const leave = () => { hideTooltip(); t.removeEventListener("mouseleave", leave); };
  t.addEventListener("mouseleave", leave);
});
document.addEventListener("mousedown", hideTooltip, true);
