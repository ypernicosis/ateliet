// Tools of the page kinds: panels (koma) and balloons.
//
// Panels — drag across a panel to cut it in two along the stroke, with the
// page's gutter (Shift keeps the cut straight); drag in empty space for a
// new panel (Alt: always a new panel); click a panel to select it, then drag
// it to move it or its corners to reshape it; Delete removes it; double
// click zooms to it; * stars it (thumbnails).
//
// Balloons — drag to make one (or click for a default size), type, click
// outside to finish; drag a balloon to move it, its corners to resize, the
// round handle to aim its tail; double click edits the text.

import { Editor, ToolHandler } from "./editor.ts";
import { Balloon, BalloonKind, Board, Panel } from "./model.ts";
import { BoardStep, cloneBoard } from "./history.ts";
import { Pt, bounds, cutPanel, inside, lineCuts, newBalloon, polyPath, rectPanel } from "./pages.ts";

export interface PageToolSettings {
  balloon: { kind: BalloonKind; vertical: boolean; size: number };
}

/** Records one undo step for changes made between `begin` and `end`. */
class Change {
  private before: Board;
  constructor(private ed: Editor, private label: string) {
    this.before = cloneBoard(ed.board);
  }
  end() {
    const after = cloneBoard(this.ed.board);
    if (JSON.stringify(after.frames) === JSON.stringify(this.before.frames)) return;
    this.ed.history.push(new BoardStep(this.label, this.ed.doc, this.before, after));
    this.ed.changed(true);
    this.ed.trigger("board");
  }
}

function screenDist(ed: Editor, a: Pt, b: Pt) {
  const p = ed.toStage(a.x, a.y), q = ed.toStage(b.x, b.y);
  return Math.hypot(p.x - q.x, p.y - q.y);
}

// ── panels ────────────────────────────────────────────────────────────────

export class PanelTool implements ToolHandler {
  id = "panel";
  cursor = "crosshair";
  selected: string | null = null;
  private drag: { mode: "line" | "move" | "vertex"; a: Pt; b: Pt; alt: boolean; shift: boolean; vertex?: number; orig?: Pt[]; change?: Change } | null = null;
  private lastDown = { t: 0, id: "" };

  constructor(private ed: Editor) {}

  private get panels(): Panel[] {
    return (this.ed.frame.panels ||= []);
  }

  private panel(id: string | null): Panel | undefined {
    return id ? this.panels.find((p) => p.id === id) : undefined;
  }

  down(e: PointerEvent, p: Pt) {
    const sel = this.panel(this.selected);
    if (sel) {
      const v = sel.points.findIndex((q) => screenDist(this.ed, q, p) < 10);
      if (v >= 0) {
        this.drag = { mode: "vertex", a: p, b: p, alt: e.altKey, shift: e.shiftKey, vertex: v, orig: sel.points.map((q) => ({ ...q })), change: new Change(this.ed, "Reshape panel") };
        return;
      }
    }
    const hit = e.altKey ? undefined : this.ed.panelAt(p.x, p.y);
    if (hit) {
      const now = performance.now();
      if (this.lastDown.id === hit.id && now - this.lastDown.t < 350) {
        this.zoomTo(hit);
        this.lastDown = { t: 0, id: "" };
        return;
      }
      this.lastDown = { t: now, id: hit.id };
      const wasSelected = this.selected === hit.id;
      this.selected = hit.id;
      this.ed.drawOverlay();
      this.ed.trigger("panel");
      if (wasSelected) this.drag = { mode: "move", a: p, b: p, alt: false, shift: false, orig: hit.points.map((q) => ({ ...q })), change: new Change(this.ed, "Move panel") };
      return;
    }
    this.drag = { mode: "line", a: p, b: p, alt: e.altKey, shift: e.shiftKey };
  }

  move(e: PointerEvent, p: Pt) {
    const d = this.drag;
    if (!d) return;
    d.b = p;
    d.shift = e.shiftKey;
    if (d.mode === "move" || d.mode === "vertex") {
      const sel = this.panel(this.selected);
      if (!sel || !d.orig) return;
      const dx = p.x - d.a.x, dy = p.y - d.a.y;
      if (d.mode === "move") sel.points = d.orig.map((q) => ({ x: Math.round(q.x + dx), y: Math.round(q.y + dy) }));
      else sel.points = d.orig.map((q, i) => (i === d.vertex ? this.snap({ x: q.x + dx, y: q.y + dy }) : q));
      this.ed.invalidate();
      return;
    }
    this.ed.drawOverlay();
  }

  up() {
    const d = this.drag;
    this.drag = null;
    if (!d) return;
    if (d.change) return d.change.end();
    const ed = this.ed;
    let { a, b } = d;
    if (screenDist(ed, a, b) < 6) {
      this.selected = null;
      ed.drawOverlay();
      ed.trigger("panel");
      return;
    }
    if (d.shift) b = Math.abs(b.x - a.x) > Math.abs(b.y - a.y) ? { x: b.x, y: a.y } : { x: a.x, y: b.y };
    const page = ed.board.page!;
    const cuts = d.alt ? [] : this.panels.filter((pn) => lineCuts(pn.points, a, b) && !inside(pn.points, a) && !inside(pn.points, b));
    // A spread drawn as one: the same line cuts the facing page's panels too.
    const other = d.alt ? null : this.facingCuts(a, b);
    if (cuts.length || other?.cuts.length) {
      // A mostly horizontal cut separates tiers (vertical gutter), and the other way round.
      const horiz = Math.abs(b.x - a.x) > Math.abs(b.y - a.y);
      const gap = horiz ? page.gutterY : page.gutterX;
      ed.mutate("Cut panel", () => {
        const f = ed.frame;
        f.panels = (f.panels || []).flatMap((pn) => (cuts.includes(pn) ? cutPanel(pn, a, b, gap) : [pn]));
        if (other?.cuts.length) {
          const fa = { x: a.x - other.dx, y: a.y }, fb = { x: b.x - other.dx, y: b.y };
          other.frame.panels = (other.frame.panels || []).flatMap((pn) => (other.cuts.includes(pn) ? cutPanel(pn, fa, fb, gap) : [pn]));
          ed.scheduleThumb(other.frame);
        }
      });
      this.selected = null;
      return;
    }
    const r = { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(b.x - a.x), h: Math.abs(b.y - a.y) };
    if (r.w < 8 || r.h < 8) return ed.drawOverlay();
    const p0 = this.snap({ x: r.x, y: r.y }), p1 = this.snap({ x: r.x + r.w, y: r.y + r.h });
    const panel = rectPanel({ x: Math.round(p0.x), y: Math.round(p0.y), w: Math.round(p1.x - p0.x), h: Math.round(p1.y - p0.y) });
    ed.mutate("New panel", () => (ed.frame.panels ||= []).push(panel));
    this.selected = panel.id;
  }

  /** On a spread drawn as one, the facing page's panels a cut line crosses. */
  private facingCuts(a: Pt, b: Pt): { frame: import("./model.ts").Frame; dx: number; cuts: import("./model.ts").Panel[] } | null {
    const f = this.ed.joinedFacing();
    if (!f) return null;
    const fa = { x: a.x - f.dx, y: a.y }, fb = { x: b.x - f.dx, y: b.y };
    const cuts = (f.frame.panels || []).filter((pn) => lineCuts(pn.points, fa, fb) && !inside(pn.points, fa) && !inside(pn.points, fb));
    return { frame: f.frame, dx: f.dx, cuts };
  }

  /** Corners stick to the inner frame and to other panels' edges. */
  private snap(p: Pt): Pt {
    const ed = this.ed;
    const tol = 10 / ed.view.scale;
    const page = ed.board.page!;
    const xs = [page.inner.x, page.inner.x + page.inner.w, 0, ed.board.width];
    const ys = [page.inner.y, page.inner.y + page.inner.h, 0, ed.board.height];
    for (const pn of this.panels) {
      if (pn.id === this.selected) continue;
      const r = bounds(pn.points);
      xs.push(r.x - page.gutterX, r.x + r.w + page.gutterX, r.x, r.x + r.w);
      ys.push(r.y - page.gutterY, r.y + r.h + page.gutterY, r.y, r.y + r.h);
    }
    const near = (v: number, list: number[]) => list.reduce((best, c) => (Math.abs(c - v) < Math.abs(best - v) ? c : best), Infinity);
    const nx = near(p.x, xs), ny = near(p.y, ys);
    return { x: Math.abs(nx - p.x) < tol ? nx : p.x, y: Math.abs(ny - p.y) < tol ? ny : p.y };
  }

  zoomTo(pn: Panel) {
    const ed = this.ed;
    const r = bounds(pn.points);
    const st = ed.stage.getBoundingClientRect();
    ed.view.rotation = 0;
    ed.view.scale = Math.min((st.width - 60) / r.w, (st.height - 60) / r.h, 8);
    ed.centerOn(r.x + r.w / 2, r.y + r.h / 2);
  }

  deleteSelected() {
    const id = this.selected;
    if (!id) return false;
    this.ed.mutate("Delete panel", () => (this.ed.frame.panels = this.panels.filter((p) => p.id !== id)));
    this.selected = null;
    return true;
  }

  toggleStar() {
    const pn = this.panel(this.selected);
    if (!pn) return;
    this.ed.mutate(pn.star ? "Unstar panel" : "Star panel", () => (pn.star = !pn.star));
  }

  key(e: KeyboardEvent): boolean {
    if ((e.key === "Delete" || e.key === "Backspace") && this.selected) return this.deleteSelected();
    if (e.key === "Escape" && this.selected) {
      this.selected = null;
      this.ed.drawOverlay();
      this.ed.trigger("panel");
      return true;
    }
    if (e.key === "*" && this.selected) {
      this.toggleStar();
      return true;
    }
    return false;
  }

  deactivate() {
    this.selected = null;
  }

  overlay(ctx: CanvasRenderingContext2D) {
    const ed = this.ed;
    const px = 1 / ed.view.scale;
    const sel = this.panel(this.selected);
    ctx.save();
    if (sel) {
      ctx.lineWidth = 2 * px;
      ctx.strokeStyle = "#2f80ed";
      ctx.stroke(polyPath(sel.points));
      ctx.fillStyle = "#fff";
      for (const q of sel.points) {
        ctx.fillRect(q.x - 5 * px, q.y - 5 * px, 10 * px, 10 * px);
        ctx.strokeRect(q.x - 5 * px, q.y - 5 * px, 10 * px, 10 * px);
      }
    }
    const d = this.drag;
    if (d && d.mode === "line") {
      let b = d.b;
      if (d.shift) b = Math.abs(b.x - d.a.x) > Math.abs(b.y - d.a.y) ? { x: b.x, y: d.a.y } : { x: d.a.x, y: b.y };
      const cuts = d.alt ? [] : this.panels.filter((pn) => lineCuts(pn.points, d.a, b) && !inside(pn.points, d.a) && !inside(pn.points, b));
      ctx.lineWidth = 2 * px;
      if (cuts.length || (!d.alt && this.facingCuts(d.a, b)?.cuts.length)) {
        ctx.strokeStyle = "#e5534b";
        ctx.setLineDash([8 * px, 5 * px]);
        ctx.beginPath();
        ctx.moveTo(d.a.x, d.a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
      } else {
        ctx.strokeStyle = "#2f80ed";
        ctx.setLineDash([6 * px, 4 * px]);
        ctx.strokeRect(Math.min(d.a.x, b.x), Math.min(d.a.y, b.y), Math.abs(b.x - d.a.x), Math.abs(b.y - d.a.y));
      }
    }
    ctx.restore();
  }
}

// ── balloons ──────────────────────────────────────────────────────────────

type BHandle = "move" | "tail" | "nw" | "ne" | "se" | "sw";

export class BalloonTool implements ToolHandler {
  id = "balloon";
  cursor = "crosshair";
  selected: string | null = null;
  private drag: { mode: BHandle | "new"; a: Pt; b: Pt; orig?: Balloon; change?: Change } | null = null;
  private editor: HTMLTextAreaElement | null = null;
  private lastDown = { t: 0, id: "" };

  constructor(private ed: Editor, private settings: () => PageToolSettings) {}

  private get balloons(): Balloon[] {
    return (this.ed.frame.balloons ||= []);
  }

  private balloon(id: string | null) {
    return id ? this.balloons.find((b) => b.id === id) : undefined;
  }

  private hitHandle(bl: Balloon, p: Pt): BHandle | null {
    const ed = this.ed;
    if (bl.tail && screenDist(ed, bl.tail, p) < 10) return "tail";
    const corners: [BHandle, Pt][] = [["nw", { x: bl.x, y: bl.y }], ["ne", { x: bl.x + bl.w, y: bl.y }], ["se", { x: bl.x + bl.w, y: bl.y + bl.h }], ["sw", { x: bl.x, y: bl.y + bl.h }]];
    for (const [h, q] of corners) if (screenDist(ed, q, p) < 10) return h;
    return null;
  }

  private at(p: Pt): Balloon | undefined {
    const list = this.balloons;
    for (let i = list.length - 1; i >= 0; i--) {
      const b = list[i];
      if (p.x >= b.x && p.x <= b.x + b.w && p.y >= b.y && p.y <= b.y + b.h) return b;
    }
    return undefined;
  }

  down(_e: PointerEvent, p: Pt) {
    this.finishEditing();
    const sel = this.balloon(this.selected);
    if (sel) {
      const h = this.hitHandle(sel, p);
      if (h) {
        this.drag = { mode: h, a: p, b: p, orig: JSON.parse(JSON.stringify(sel)), change: new Change(this.ed, h === "tail" ? "Aim balloon tail" : "Resize balloon") };
        return;
      }
    }
    const hit = this.at(p);
    if (hit) {
      const now = performance.now();
      if (this.lastDown.id === hit.id && now - this.lastDown.t < 350) {
        this.selected = hit.id;
        this.edit(hit);
        return;
      }
      this.lastDown = { t: now, id: hit.id };
      this.selected = hit.id;
      this.drag = { mode: "move", a: p, b: p, orig: JSON.parse(JSON.stringify(hit)), change: new Change(this.ed, "Move balloon") };
      this.ed.drawOverlay();
      this.ed.trigger("balloon");
      return;
    }
    this.selected = null;
    this.drag = { mode: "new", a: p, b: p };
    this.ed.trigger("balloon");
  }

  move(_e: PointerEvent, p: Pt) {
    const d = this.drag;
    if (!d) return;
    d.b = p;
    const sel = this.balloon(this.selected);
    if (d.mode !== "new" && sel && d.orig) {
      const dx = p.x - d.a.x, dy = p.y - d.a.y, o = d.orig;
      if (d.mode === "move") {
        sel.x = o.x + dx;
        sel.y = o.y + dy;
        if (o.tail && sel.tail) sel.tail = { x: o.tail.x + dx, y: o.tail.y + dy };
      } else if (d.mode === "tail") sel.tail = { x: p.x, y: p.y };
      else {
        const left = d.mode === "nw" || d.mode === "sw", top = d.mode === "nw" || d.mode === "ne";
        const x0 = left ? o.x + dx : o.x, x1 = left ? o.x + o.w : o.x + o.w + dx;
        const y0 = top ? o.y + dy : o.y, y1 = top ? o.y + o.h : o.y + o.h + dy;
        sel.x = Math.min(x0, x1);
        sel.y = Math.min(y0, y1);
        sel.w = Math.max(30, Math.abs(x1 - x0));
        sel.h = Math.max(24, Math.abs(y1 - y0));
      }
      this.ed.invalidate();
      return;
    }
    this.ed.drawOverlay();
  }

  up() {
    const d = this.drag;
    this.drag = null;
    if (!d) return;
    if (d.change) return d.change.end();
    const s = this.settings().balloon;
    const ed = this.ed;
    let r = { x: Math.min(d.a.x, d.b.x), y: Math.min(d.a.y, d.b.y), w: Math.abs(d.b.x - d.a.x), h: Math.abs(d.b.y - d.a.y) };
    if (screenDist(ed, d.a, d.b) < 8) {
      // A click: a balloon sized for a short line at the chosen text size.
      const w = s.vertical ? s.size * 4.5 : s.size * 9, h = s.vertical ? s.size * 8 : s.size * 4.2;
      r = { x: d.a.x - w / 2, y: d.a.y - h / 2, w, h };
    }
    const bl = newBalloon(s.kind, r, s.vertical, s.size);
    ed.mutate("New balloon", () => this.balloons.push(bl));
    this.selected = bl.id;
    ed.trigger("balloon");
    this.edit(bl);
  }

  /** Types into the balloon in place. */
  edit(bl: Balloon) {
    this.finishEditing();
    const ed = this.ed;
    const p0 = ed.toStage(bl.x, bl.y), p1 = ed.toStage(bl.x + bl.w, bl.y + bl.h);
    const ta = ed.stage.createEl("textarea", { cls: "sb-balloon-editor", attr: { placeholder: "Type the line…  (Esc: done)" } });
    ta.value = bl.text;
    const w = Math.max(160, Math.abs(p1.x - p0.x)), h = Math.max(70, Math.abs(p1.y - p0.y));
    ta.style.left = `${Math.min(p0.x, p1.x)}px`;
    ta.style.top = `${Math.min(p0.y, p1.y)}px`;
    ta.style.width = `${w}px`;
    ta.style.height = `${h}px`;
    ta.style.fontSize = `${Math.max(12, Math.min(28, bl.size * ed.view.scale))}px`;
    const before = cloneBoard(ed.board);
    ta.addEventListener("input", () => {
      bl.text = ta.value;
      ed.invalidate();
    });
    ta.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape" || (e.key === "Enter" && (e.metaKey || e.ctrlKey))) {
        e.preventDefault();
        this.finishEditing();
      }
    });
    ta.addEventListener("blur", () => this.finishEditing());
    (ta as any)._done = () => {
      const after = cloneBoard(ed.board);
      if (JSON.stringify(after.frames) !== JSON.stringify(before.frames)) {
        ed.history.push(new BoardStep("Balloon text", ed.doc, before, after));
        ed.changed(true);
      }
    };
    this.editor = ta;
    setTimeout(() => ta.focus(), 0);
  }

  finishEditing() {
    const ta = this.editor;
    if (!ta) return;
    this.editor = null;
    (ta as any)._done?.();
    ta.remove();
    this.ed.stage.focus({ preventScroll: true });
  }

  deleteSelected() {
    const id = this.selected;
    if (!id) return false;
    this.ed.mutate("Delete balloon", () => (this.ed.frame.balloons = this.balloons.filter((b) => b.id !== id)));
    this.selected = null;
    return true;
  }

  /** Applies the tool options to the selected balloon. */
  applyToSelected(patch: Partial<Balloon>) {
    const bl = this.balloon(this.selected);
    if (!bl) return;
    this.ed.mutate("Balloon style", () => Object.assign(bl, patch));
  }

  key(e: KeyboardEvent): boolean {
    if ((e.key === "Delete" || e.key === "Backspace") && this.selected) return this.deleteSelected();
    if (e.key === "Enter" && this.selected) {
      const bl = this.balloon(this.selected);
      if (bl) this.edit(bl);
      return true;
    }
    if (e.key === "Escape" && this.selected) {
      this.selected = null;
      this.ed.drawOverlay();
      return true;
    }
    return false;
  }

  deactivate() {
    this.finishEditing();
    this.selected = null;
  }

  overlay(ctx: CanvasRenderingContext2D) {
    const ed = this.ed;
    const px = 1 / ed.view.scale;
    ctx.save();
    const sel = this.balloon(this.selected);
    if (sel) {
      ctx.lineWidth = 1.5 * px;
      ctx.strokeStyle = "#2f80ed";
      ctx.setLineDash([5 * px, 3 * px]);
      ctx.strokeRect(sel.x, sel.y, sel.w, sel.h);
      ctx.setLineDash([]);
      ctx.fillStyle = "#fff";
      for (const q of [{ x: sel.x, y: sel.y }, { x: sel.x + sel.w, y: sel.y }, { x: sel.x + sel.w, y: sel.y + sel.h }, { x: sel.x, y: sel.y + sel.h }]) {
        ctx.fillRect(q.x - 4 * px, q.y - 4 * px, 8 * px, 8 * px);
        ctx.strokeRect(q.x - 4 * px, q.y - 4 * px, 8 * px, 8 * px);
      }
      if (sel.tail) {
        ctx.beginPath();
        ctx.arc(sel.tail.x, sel.tail.y, 6 * px, 0, Math.PI * 2);
        ctx.fillStyle = "#2f80ed";
        ctx.fill();
      }
    }
    const d = this.drag;
    if (d && d.mode === "new") {
      ctx.strokeStyle = "#2f80ed";
      ctx.lineWidth = px;
      ctx.setLineDash([5 * px, 3 * px]);
      ctx.strokeRect(Math.min(d.a.x, d.b.x), Math.min(d.a.y, d.b.y), Math.abs(d.b.x - d.a.x), Math.abs(d.b.y - d.a.y));
    }
    ctx.restore();
  }
}
