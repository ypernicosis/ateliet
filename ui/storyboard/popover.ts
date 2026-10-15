// A small panel under a button (settings that need sliders, not a menu).
// One at a time; a click outside or Escape closes it.

let open: { el: HTMLElement; close: () => void } | null = null;

export function closePopover() {
  open?.close();
}

export function openPopover(anchor: HTMLElement, build: (el: HTMLElement, close: () => void) => void, cls = ""): HTMLElement {
  const wasMine = open?.el.dataset.anchor === anchor.dataset.popoverId && anchor.dataset.popoverId;
  closePopover();
  if (wasMine) return anchor; // a second click on the button closes it
  anchor.dataset.popoverId ||= Math.random().toString(36).slice(2);
  const el = document.body.createDiv({ cls: `sb-popover ${cls}`, attr: { "data-anchor": anchor.dataset.popoverId } });
  const onDown = (e: PointerEvent) => {
    if (!el.contains(e.target as Node) && !anchor.contains(e.target as Node) && !(e.target as HTMLElement).closest?.(".menu")) close();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      close();
    }
  };
  const close = () => {
    document.removeEventListener("pointerdown", onDown, true);
    document.removeEventListener("keydown", onKey, true);
    el.remove();
    anchor.removeClass("has-popover");
    if (open?.el === el) open = null;
  };
  build(el, close);
  const r = anchor.getBoundingClientRect();
  const w = el.offsetWidth, h = el.offsetHeight;
  const below = r.bottom + 6 + h < innerHeight;
  el.style.left = `${Math.max(8, Math.min(innerWidth - w - 8, r.left + r.width / 2 - w / 2))}px`;
  el.style.top = `${below ? r.bottom + 6 : Math.max(8, r.top - h - 6)}px`;
  anchor.addClass("has-popover");
  setTimeout(() => {
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey, true);
  }, 0);
  open = { el, close };
  return el;
}
