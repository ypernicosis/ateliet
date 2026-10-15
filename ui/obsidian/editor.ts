// Editor (the CM6 wrapper plugins use) + EditorSuggest + editor state fields.

import { EditorSelection, StateField, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { undo, redo, selectAll, indentMore, indentLess, deleteLine, cursorLineUp, toggleComment } from "@codemirror/commands";
import { foldAll, unfoldAll, foldCode, unfoldCode } from "@codemirror/language";
import { openSearchPanel } from "@codemirror/search";
import { PopoverSuggest } from "./ui.ts";

export interface EditorPosition {
  line: number;
  ch: number;
}
export interface EditorRange {
  from: EditorPosition;
  to: EditorPosition;
}

/** MarkdownFileInfo of the editor (the owning view). */
export const editorInfoField = StateField.define<any>({ create: () => null, update: (v) => v });
/** Deprecated aliases kept for older plugins. */
export const editorViewField = editorInfoField;
export const editorEditorField = StateField.define<EditorView | null>({ create: () => null, update: (v) => v });

export function infoExtension(info: any): Extension {
  return editorInfoField.init(() => info);
}

export class Editor {
  cm: EditorView;
  /** @internal */ owner: any;

  constructor(cm: EditorView, owner?: any) {
    this.cm = cm;
    this.owner = owner;
  }

  private get doc() {
    return this.cm.state.doc;
  }

  posToOffset(pos: EditorPosition): number {
    const lines = this.doc.lines;
    const line = Math.max(0, Math.min(pos.line, lines - 1));
    const l = this.doc.line(line + 1);
    return Math.min(l.from + Math.max(0, pos.ch), l.to);
  }

  offsetToPos(offset: number): EditorPosition {
    offset = Math.max(0, Math.min(offset, this.doc.length));
    const l = this.doc.lineAt(offset);
    return { line: l.number - 1, ch: offset - l.from };
  }

  getDoc(): this {
    return this;
  }
  refresh(): void {
    this.cm.requestMeasure();
  }
  getValue(): string {
    return this.doc.toString();
  }
  setValue(content: string): void {
    this.cm.dispatch({ changes: { from: 0, to: this.doc.length, insert: content } });
  }
  getLine(line: number): string {
    if (line < 0 || line >= this.doc.lines) return "";
    return this.doc.line(line + 1).text;
  }
  setLine(n: number, text: string): void {
    const l = this.doc.line(n + 1);
    this.cm.dispatch({ changes: { from: l.from, to: l.to, insert: text } });
  }
  lineCount(): number {
    return this.doc.lines;
  }
  lastLine(): number {
    return this.doc.lines - 1;
  }
  getSelection(): string {
    const s = this.cm.state.selection.main;
    return this.cm.state.sliceDoc(s.from, s.to);
  }
  somethingSelected(): boolean {
    return this.cm.state.selection.ranges.some((r) => !r.empty);
  }
  getRange(from: EditorPosition, to: EditorPosition): string {
    return this.cm.state.sliceDoc(this.posToOffset(from), this.posToOffset(to));
  }
  replaceSelection(replacement: string, origin?: string): void {
    this.cm.dispatch(this.cm.state.replaceSelection(replacement), { userEvent: origin || "input" } as any);
  }
  replaceRange(replacement: string, from: EditorPosition, to?: EditorPosition, origin?: string): void {
    const a = this.posToOffset(from);
    const b = to ? this.posToOffset(to) : a;
    this.cm.dispatch({ changes: { from: Math.min(a, b), to: Math.max(a, b), insert: replacement }, userEvent: origin });
  }
  getCursor(which: "from" | "to" | "head" | "anchor" = "head"): EditorPosition {
    const s = this.cm.state.selection.main;
    const off = which === "from" ? s.from : which === "to" ? s.to : which === "anchor" ? s.anchor : s.head;
    return this.offsetToPos(off);
  }
  listSelections(): { anchor: EditorPosition; head: EditorPosition }[] {
    return this.cm.state.selection.ranges.map((r) => ({ anchor: this.offsetToPos(r.anchor), head: this.offsetToPos(r.head) }));
  }
  setCursor(pos: EditorPosition | number, ch?: number): void {
    const p = typeof pos === "number" ? { line: pos, ch: ch ?? 0 } : pos;
    const off = this.posToOffset(p);
    this.cm.dispatch({ selection: { anchor: off }, scrollIntoView: true });
  }
  setSelection(anchor: EditorPosition, head?: EditorPosition): void {
    this.cm.dispatch({ selection: { anchor: this.posToOffset(anchor), head: this.posToOffset(head || anchor) }, scrollIntoView: true });
  }
  setSelections(ranges: { anchor: EditorPosition; head?: EditorPosition }[], main?: number): void {
    const sel = EditorSelection.create(
      ranges.map((r) => EditorSelection.range(this.posToOffset(r.anchor), this.posToOffset(r.head || r.anchor))),
      main ?? 0,
    );
    this.cm.dispatch({ selection: sel });
  }
  focus(): void {
    this.cm.focus();
  }
  blur(): void {
    this.cm.contentDOM.blur();
  }
  hasFocus(): boolean {
    return this.cm.hasFocus;
  }
  getScrollInfo(): { top: number; left: number; height: number; clientHeight: number; width: number; clientWidth: number } {
    const s = this.cm.scrollDOM;
    return { top: s.scrollTop, left: s.scrollLeft, height: s.scrollHeight, clientHeight: s.clientHeight, width: s.scrollWidth, clientWidth: s.clientWidth };
  }
  scrollTo(x?: number | null, y?: number | null): void {
    if (x != null) this.cm.scrollDOM.scrollLeft = x;
    if (y != null) this.cm.scrollDOM.scrollTop = y;
  }
  scrollIntoView(range: EditorRange, center?: boolean): void {
    const from = this.posToOffset(range.from);
    this.cm.dispatch({ effects: EditorView.scrollIntoView(from, { y: center ? "center" : "nearest" }) });
  }
  undo(): void {
    undo(this.cm);
  }
  redo(): void {
    redo(this.cm);
  }
  exec(command: string): void {
    const cm = this.cm;
    const cmds: Record<string, () => any> = {
      goUp: () => cursorLineUp(cm),
      selectAll: () => selectAll(cm),
      indentMore: () => indentMore(cm),
      indentLess: () => indentLess(cm),
      deleteLine: () => deleteLine(cm),
      undo: () => undo(cm),
      redo: () => redo(cm),
      toggleComment: () => toggleComment(cm),
      foldAll: () => foldAll(cm),
      unfoldAll: () => unfoldAll(cm),
      toggleFold: () => foldCode(cm) || unfoldCode(cm),
      find: () => openSearchPanel(cm),
      newlineAndIndent: () => this.replaceSelection("\n"),
      goStart: () => cm.dispatch({ selection: { anchor: 0 } }),
      goEnd: () => cm.dispatch({ selection: { anchor: cm.state.doc.length } }),
      swapLineUp: () => (window as any).app?.commands.executeCommandById("editor:swap-line-up"),
      swapLineDown: () => (window as any).app?.commands.executeCommandById("editor:swap-line-down"),
    };
    cmds[command]?.();
  }
  transaction(tx: { changes?: { from: EditorPosition; to?: EditorPosition; text: string }[]; selection?: { from: EditorPosition; to?: EditorPosition }; selections?: { from: EditorPosition; to?: EditorPosition }[]; replaceSelection?: string }, origin?: string): void {
    const spec: any = { userEvent: origin };
    if (tx.changes) spec.changes = tx.changes.map((c) => ({ from: this.posToOffset(c.from), to: c.to ? this.posToOffset(c.to) : undefined, insert: c.text }));
    if (tx.replaceSelection !== undefined) {
      this.cm.dispatch(this.cm.state.replaceSelection(tx.replaceSelection));
    }
    if (tx.selection) spec.selection = { anchor: this.posToOffset(tx.selection.from), head: this.posToOffset(tx.selection.to || tx.selection.from) };
    if (tx.selections) spec.selection = EditorSelection.create(tx.selections.map((s) => EditorSelection.range(this.posToOffset(s.from), this.posToOffset(s.to || s.from))));
    if (spec.changes || spec.selection) this.cm.dispatch(spec);
  }
  wordAt(pos: EditorPosition): EditorRange | null {
    const r = this.cm.state.wordAt(this.posToOffset(pos));
    return r ? { from: this.offsetToPos(r.from), to: this.offsetToPos(r.to) } : null;
  }
  processLines<T>(read: (line: number, lineText: string) => T | null, write: (line: number, lineText: string, value: T | null) => any, ignoreEmpty = true): void {
    const changes: any[] = [];
    for (let i = 0; i < this.doc.lines; i++) {
      const text = this.getLine(i);
      if (ignoreEmpty && !text) continue;
      const v = read(i, text);
      const w = write(i, text, v);
      if (w) changes.push({ from: this.posToOffset(w.from), to: this.posToOffset(w.to ?? w.from), insert: w.text });
    }
    if (changes.length) this.cm.dispatch({ changes });
  }
  getClickableTokenAt(_pos: EditorPosition): any {
    return null;
  }
  setHeading?(level: number): void;
  toggleMarkdownFormatting?(syntax: string): void;
}

// ── EditorSuggest ─────────────────────────────────────────────────────────

export interface EditorSuggestTriggerInfo {
  start: EditorPosition;
  end: EditorPosition;
  query: string;
}

export abstract class EditorSuggest<T> extends PopoverSuggest<T> {
  context: any = null;
  limit = 100;
  instructionsEl: HTMLElement;

  constructor(app: any) {
    super(app);
    this.instructionsEl = this.suggestEl.createDiv("prompt-instructions");
    this.instructionsEl.hide();
  }

  setInstructions(instructions: { command: string; purpose: string }[]): void {
    this.instructionsEl.empty();
    this.instructionsEl.toggle(instructions.length > 0);
    for (const i of instructions) {
      const d = this.instructionsEl.createDiv("prompt-instruction");
      d.createSpan({ cls: "prompt-instruction-command", text: i.command });
      d.createSpan({ text: i.purpose });
    }
  }

  /** @internal called by the editor after each user edit. */
  trigger(editor: Editor, file: any, force: boolean): boolean {
    const cursor = editor.getCursor();
    if (!force && editor.somethingSelected()) return false;
    let info: EditorSuggestTriggerInfo | null = null;
    try {
      info = this.onTrigger(cursor, editor, file);
    } catch (e) {
      console.error(e);
    }
    if (!info) {
      this.close();
      return false;
    }
    this.context = { ...info, editor, file };
    const r: any = this.getSuggestions(this.context);
    const apply = (list: T[]) => {
      list = (list || []).slice(0, this.limit);
      if (!list.length) return this.close();
      this.open();
      this.chooser.setSuggestions(list);
      const coords = editor.cm.coordsAtPos(editor.posToOffset(info!.start));
      if (coords) this.reposition({ left: coords.left, top: coords.top, bottom: coords.bottom });
    };
    if (r && typeof r.then === "function") r.then(apply);
    else apply(r);
    return true;
  }

  close(): void {
    super.close();
    this.context = null;
  }

  abstract onTrigger(cursor: EditorPosition, editor: Editor, file: any): EditorSuggestTriggerInfo | null;
  abstract getSuggestions(context: any): T[] | Promise<T[]>;
}
