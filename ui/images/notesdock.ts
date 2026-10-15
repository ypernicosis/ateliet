// The right side of image mode: the notes you write while looking at
// pictures. It holds real notes (the same editor as writing mode), with its
// own bar — which note, a new one, the pictures selected in the library put
// into it, carry on in writing mode — and, with no note open, a start page
// with recent notes and a note for the collection on screen.
//
// Only notes live here in image mode: the writing sidebar's panels
// (backlinks, outline, the picture panel…) are left out of it.

import { ImageItem, insertImages } from "./api.ts";
import { LIBRARY_VIEW } from "./library.ts";
import { MarkdownView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { FuzzySuggestModal, Menu, Notice } from "../obsidian/ui.ts";
import { TFile } from "../obsidian/vault.ts";

export interface NotesHost {
  isImageMode(): boolean;
  setMode(mode: "write" | "images"): Promise<void>;
  openNote(file: TFile, newLeaf?: boolean, focus?: boolean): Promise<void>;
}

class NotePicker extends FuzzySuggestModal<TFile> {
  constructor(app: any, private onPick: (f: TFile) => void) {
    super(app);
    this.setPlaceholder("Find a note to write in…");
  }
  getItems(): TFile[] {
    return this.app.vault.getMarkdownFiles().sort((a: TFile, b: TFile) => b.stat.mtime - a.stat.mtime);
  }
  getItemText(f: TFile): string {
    return f.path.replace(/\.md$/, "");
  }
  onChooseItem(f: TFile) {
    this.onPick(f);
  }
}

export class NotesDock {
  private bar: HTMLElement | null = null;
  private empty: HTMLElement | null = null;
  private wsRefs: any[] = [];
  private vaultRefs: any[] = [];
  private queued = 0;

  constructor(private app: any, private host: NotesHost) {}

  mount() {
    const ws = this.app.workspace;
    const later = () => this.queue();
    for (const ev of ["layout-change", "active-leaf-change", "file-open"]) this.wsRefs.push(ws.on(ev, later));
    this.vaultRefs.push(this.app.vault.on("rename", later), this.app.vault.on("delete", later));
    this.queue();
  }

  unmount() {
    for (const r of this.wsRefs) this.app.workspace.offref(r);
    for (const r of this.vaultRefs) this.app.vault.offref(r);
    this.wsRefs = [];
    this.vaultRefs = [];
    this.bar?.remove();
    this.empty?.remove();
    this.bar = this.empty = null;
  }

  private queue() {
    cancelAnimationFrame(this.queued);
    this.queued = requestAnimationFrame(() => this.render());
  }

  /** Leaves out of the image-mode notes side what belongs to the writing sidebar. */
  tidy() {
    const ws = this.app.workspace;
    const extra: any[] = [];
    ws.iterateLeaves(ws.rightSplit, (l: any) => {
      const t = l.view?.getViewType?.();
      if (t !== "markdown") extra.push(l);
    });
    const notes = this.notes();
    // One empty leaf stays when there is no note, so the side keeps its place.
    for (const l of extra) if (notes.length || l !== extra[0]) l.detach();
  }

  /** The notes open on the right. */
  private notes(): any[] {
    const ws = this.app.workspace;
    const out: any[] = [];
    ws.iterateLeaves(ws.rightSplit, (l: any) => {
      if (l.view instanceof MarkdownView && l.view.file) out.push(l);
    });
    return out;
  }

  /** The note shown on the right (the selected tab). */
  private current(): any | null {
    const notes = this.notes();
    return notes.find((l) => l.parent?.children?.[l.parent.currentTab] === l) || notes[0] || null;
  }

  private render() {
    const ws = this.app.workspace;
    const side: HTMLElement | undefined = ws.rightSplit?.containerEl;
    if (!side) return;
    if (!this.host.isImageMode()) {
      this.bar?.remove();
      this.empty?.remove();
      side.removeClass("image-notes-side", "has-no-note");
      return;
    }
    side.addClass("image-notes-side");
    const leaf = this.current();
    // The bar.
    if (!this.bar) this.bar = createDiv("image-notes-bar");
    if (side.firstChild !== this.bar) side.prepend(this.bar);
    this.renderBar(this.bar, leaf);
    // With no note: the start page.
    side.toggleClass("has-no-note", !leaf);
    if (!leaf) {
      if (!this.empty) this.empty = createDiv("image-notes-empty");
      if (this.empty.parentElement !== side) side.appendChild(this.empty);
      this.renderEmpty(this.empty);
    } else this.empty?.remove();
  }

  private renderBar(bar: HTMLElement, leaf: any | null) {
    bar.empty();
    const file: TFile | null = leaf?.view?.file || null;
    const title = bar.createDiv({ cls: "image-notes-title", attr: { "aria-label": "Choose the note", tabindex: "0" } });
    setIcon(title.createSpan("image-notes-title-icon"), "file-text");
    title.createSpan({ cls: "image-notes-title-text", text: file ? file.basename : "Notes" });
    setIcon(title.createSpan("image-notes-title-caret"), "chevron-down");
    title.addEventListener("click", (e) => this.noteMenu(e));
    const actions = bar.createDiv("image-notes-actions");
    const button = (icon: string, label: string, run: () => void, disabled = false) => {
      const b = actions.createDiv({ cls: "clickable-icon", attr: { "aria-label": label } });
      setIcon(b, icon);
      b.toggleClass("is-disabled", disabled);
      b.addEventListener("click", () => !disabled && run());
    };
    button("file-plus", "New note", () => this.newNote());
    button("image-plus", "Put the pictures selected in the library into this note (or drag them in)", () => this.insertSelected(), !file);
    button("pen-line", "Carry on in writing mode", () => this.toWriting(), !file);
    button("panel-right-close", "Hide notes", () => this.app.workspace.rightSplit.collapse());
  }

  private renderEmpty(el: HTMLElement) {
    el.empty();
    const box = el.createDiv("image-notes-start");
    box.createDiv({ cls: "image-notes-start-title", text: "Write while you look" });
    box.createDiv({ cls: "image-notes-start-text", text: "Notes open here, next to the pictures. Drag pictures from the library into a note to place them; they are copied into the vault when needed." });
    const row = box.createDiv("image-notes-start-buttons");
    const go = (text: string, icon: string, cta: boolean, run: () => void) => {
      const b = row.createEl("button", { cls: cta ? "mod-cta" : "" });
      setIcon(b.createSpan("image-notes-btn-icon"), icon);
      b.createSpan({ text });
      b.addEventListener("click", run);
    };
    go("New note", "file-plus", true, () => this.newNote());
    go("Open a note…", "search", false, () => this.pick());
    const coll = this.libraryCollection();
    if (coll) go(`Note for “${coll}”`, "notebook-pen", false, () => this.collectionNote(coll));
    const recent = this.recent(6);
    if (recent.length) {
      box.createDiv({ cls: "image-notes-start-heading", text: "Recent" });
      const list = box.createDiv("image-notes-recent");
      for (const f of recent) {
        const item = list.createDiv({ cls: "image-notes-recent-item", attr: { tabindex: "0" } });
        setIcon(item.createSpan("image-notes-recent-icon"), "file-text");
        item.createSpan({ cls: "image-notes-recent-name", text: f.basename });
        const folder = f.parent && !f.parent.isRoot() ? f.parent.path : "";
        if (folder) item.createSpan({ cls: "image-notes-recent-folder", text: folder });
        item.addEventListener("click", () => this.open(f));
      }
    }
  }

  private noteMenu(e: MouseEvent) {
    const menu = new Menu();
    const notes = this.notes();
    const cur = this.current();
    for (const l of notes) {
      menu.addItem((i: any) =>
        i.setSection("open").setTitle(l.view.file.basename).setIcon("file-text").setChecked(l === cur).onClick(() => {
          this.app.workspace.setActiveLeaf(l, { focus: true });
          this.queue();
        }),
      );
    }
    const open = new Set(notes.map((l) => l.view.file.path));
    for (const f of this.recent(7).filter((f) => !open.has(f.path))) {
      menu.addItem((i: any) => i.setSection("recent").setTitle(f.basename).setIcon("history").onClick(() => this.open(f)));
    }
    menu.addItem((i: any) => i.setSection("actions").setTitle("Find a note…").setIcon("search").onClick(() => this.pick()));
    menu.addItem((i: any) => i.setSection("actions").setTitle("New note").setIcon("file-plus").onClick(() => this.newNote()));
    const coll = this.libraryCollection();
    if (coll) menu.addItem((i: any) => i.setSection("actions").setTitle(`Note for “${coll}”`).setIcon("notebook-pen").onClick(() => this.collectionNote(coll)));
    if (cur) {
      menu.addItem((i: any) => i.setSection("close").setTitle("Open in a second note tab").setIcon("copy-plus").onClick(() => this.pick(true)));
      menu.addItem((i: any) => i.setSection("close").setTitle("Close this note").setIcon("x").onClick(() => {
        cur.detach();
        this.queue();
      }));
    }
    menu.showAtMouseEvent(e);
  }

  private recent(n: number): TFile[] {
    const files: TFile[] = [];
    for (const p of this.app.workspace.getLastOpenFiles?.() || []) {
      const f = this.app.vault.getAbstractFileByPath(p);
      if (f instanceof TFile && f.extension === "md") files.push(f);
      if (files.length >= n) break;
    }
    if (files.length < n) {
      const more = this.app.vault.getMarkdownFiles().sort((a: TFile, b: TFile) => b.stat.mtime - a.stat.mtime);
      for (const f of more) {
        if (files.length >= n) break;
        if (!files.includes(f)) files.push(f);
      }
    }
    return files;
  }

  private pick(second = false) {
    new NotePicker(this.app, (f) => this.open(f, second)).open();
  }

  private async open(f: TFile, second = false) {
    const ws = this.app.workspace;
    if (second) {
      const leaf = ws.getRightLeaf(false);
      await leaf.openFile(f);
      ws.setActiveLeaf(leaf, { focus: true });
    } else await this.host.openNote(f, false, true);
    this.queue();
  }

  private async newNote(name = "Untitled", content = "") {
    const fm = this.app.fileManager;
    const file: TFile = await fm.createNewMarkdownFile(fm.getNewFileParent(""), name, content);
    await this.host.openNote(file, false, true);
    // The title is the first thing to write.
    const view = this.app.workspace.getActiveViewOfType(MarkdownView);
    if (view?.file === file && name === "Untitled") {
      const t = view.inlineTitleEl;
      t?.focus?.();
      if (t) getSelection()?.selectAllChildren(t);
    }
    this.queue();
  }

  /** The collection (or folder) open in the library, by name. */
  private libraryCollection(): string | null {
    const leaf = this.app.workspace.getLeavesOfType(LIBRARY_VIEW)[0];
    const s = leaf?.view?.getState?.();
    const c = s?.collection;
    if (!c || c.startsWith("__")) return null;
    return c.split("/").pop() || c;
  }

  private async collectionNote(name: string) {
    const title = name.replace(/[-_]+/g, " ").replace(/^\w/, (c) => c.toUpperCase());
    const existing = this.app.vault.getMarkdownFiles().find((f: TFile) => f.basename.toLowerCase() === title.toLowerCase());
    if (existing) return this.open(existing);
    await this.newNote(title, `# ${title}\n\n`);
  }

  private selectedPictures(): ImageItem[] {
    const leaf = this.app.workspace.getLeavesOfType(LIBRARY_VIEW).find((l: any) => l.view?.selectedItems?.().length);
    return leaf?.view?.selectedItems?.() || [];
  }

  private async insertSelected() {
    const leaf = this.current();
    const items = this.selectedPictures();
    if (!items.length) {
      new Notice("Select pictures in the library first (click, Shift or Cmd/Ctrl-click), or drag them into the note.");
      return;
    }
    // Without a place chosen in the note (the cursor never moved), they go at the end.
    const cm = leaf?.view?.editMode?.cm;
    let at: number | undefined;
    if (cm && cm.state.selection.main.head === 0 && cm.state.doc.length) {
      if (!cm.state.doc.toString().endsWith("\n")) cm.dispatch({ changes: { from: cm.state.doc.length, insert: "\n" } });
      at = cm.state.doc.length;
    }
    if (await insertImages(this.app, items, leaf?.view || null, at)) new Notice(`${items.length} picture${items.length === 1 ? "" : "s"} added to “${leaf.view.file.basename}”.`);
  }

  private async toWriting() {
    const file: TFile | null = this.current()?.view?.file || null;
    if (!file) return;
    await this.host.setMode("write");
    await this.app.workspace.getLeaf(false).openFile(file);
  }
}
