// Undo and redo. Two kinds of steps:
//
// - pixels: the tiles a stroke (or fill, or transform…) touched, before and
//   after, as canvases;
// - structure: board.json before and after (frames, layers, settings), which
//   is small, so a copy of the whole thing is simplest and always right.
//
// Layers deleted by a structural step keep their pixels while a step still
// refers to them (see StoryDoc.retain).

import { Board } from "./model.ts";
import { StoryDoc } from "./store.ts";

export interface Step {
  label: string;
  undo(): void;
  redo(): void;
  /** Layers this step needs to keep. */
  layers(): string[];
  /** Tiles held, for the memory budget. */
  weight: number;
}

export class TileStep implements Step {
  before = new Map<string, HTMLCanvasElement | null>();
  after = new Map<string, HTMLCanvasElement | null>();
  weight = 0;
  constructor(public label: string, private doc: StoryDoc, public layer: string) {}

  /** Records a tile's state before it is first changed. */
  touch(tx: number, ty: number) {
    const k = `${tx}_${ty}`;
    if (this.before.has(k)) return;
    this.before.set(k, this.doc.tiles.snapshot(this.layer, tx, ty));
  }

  /** Records the state after the change; returns false when nothing changed. */
  finish(): boolean {
    for (const k of this.before.keys()) {
      const [tx, ty] = k.split("_").map(Number);
      this.after.set(k, this.doc.tiles.snapshot(this.layer, tx, ty));
    }
    this.weight = this.before.size * 2;
    return this.before.size > 0;
  }

  private apply(m: Map<string, HTMLCanvasElement | null>) {
    for (const [k, snap] of m) {
      const [tx, ty] = k.split("_").map(Number);
      this.doc.tiles.restore(this.layer, tx, ty, snap);
    }
  }
  undo() {
    this.apply(this.before);
  }
  redo() {
    this.apply(this.after);
  }
  layers() {
    return [this.layer];
  }
}

export function cloneBoard(b: Board): Board {
  return JSON.parse(JSON.stringify(b));
}

export class BoardStep implements Step {
  weight = 0;
  constructor(public label: string, private doc: StoryDoc, private before: Board, private after: Board, private extra: Step[] = []) {}
  undo() {
    for (const s of [...this.extra].reverse()) s.undo();
    this.doc.board = cloneBoard(this.before);
  }
  redo() {
    this.doc.board = cloneBoard(this.after);
    for (const s of this.extra) s.redo();
  }
  layers() {
    return [...Object.keys(this.before.layers), ...Object.keys(this.after.layers), ...this.extra.flatMap((s) => s.layers())];
  }
}

export class History {
  private done: Step[] = [];
  private undone: Step[] = [];
  limit = 80;
  /** Tiles kept at most (each about 1 MB decoded). */
  budget = 400;
  onChange: () => void = () => {};

  push(step: Step) {
    this.done.push(step);
    this.undone = [];
    let weight = this.done.reduce((n, s) => n + s.weight, 0);
    while (this.done.length > this.limit || (weight > this.budget && this.done.length > 1)) {
      weight -= this.done.shift()!.weight;
    }
    this.onChange();
  }

  undo(): Step | null {
    const s = this.done.pop();
    if (!s) return null;
    s.undo();
    this.undone.push(s);
    this.onChange();
    return s;
  }

  redo(): Step | null {
    const s = this.undone.pop();
    if (!s) return null;
    s.redo();
    this.done.push(s);
    this.onChange();
    return s;
  }

  get canUndo() {
    return this.done.length > 0;
  }
  get canRedo() {
    return this.undone.length > 0;
  }
  peekUndo() {
    return this.done[this.done.length - 1];
  }
  peekRedo() {
    return this.undone[this.undone.length - 1];
  }

  layersInUse(): Set<string> {
    return new Set([...this.done, ...this.undone].flatMap((s) => s.layers()));
  }

  clear() {
    this.done = [];
    this.undone = [];
    this.onChange();
  }
}
