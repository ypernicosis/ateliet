// Every storyboard in the vault, with its first frame, frame count, length
// and size: open, rename, duplicate, change settings, delete. New ones start
// here (blank, from a Clip Studio file, or from Pictogrep's drawings).

import { Board, frameStarts, isPageKind, timecode, totalFrames } from "./model.ts";
import { kindInfo } from "./kinds.ts";
import { peek } from "./store.ts";
import { STORYBOARD_VIEW } from "./view.ts";
import { ItemView } from "../obsidian/views.ts";
import { setIcon } from "../obsidian/icons.ts";
import { ConfirmationModal, Menu, Notice } from "../obsidian/ui.ts";
import { Component } from "../obsidian/events.ts";
import { TFile } from "../obsidian/vault.ts";

export const LIST_VIEW = "storyboard-list";

export interface StoryboardHost {
  create(opts?: { folder?: string }): Promise<TFile | null>;
  createFromClip(): Promise<TFile | null>;
  createFromPictogrep(): Promise<TFile | null>;
  open(file: TFile, newLeaf?: boolean): Promise<void>;
  rename(file: TFile): Promise<void>;
  duplicate(file: TFile): Promise<void>;
}

export class StoryboardListView extends ItemView {
  private grid!: HTMLElement;
  private filter = "";
  private sort: "modified" | "name" = "modified";
  private urls: string[] = [];
  private timer = 0;

  constructor(leaf: any, private host: StoryboardHost) {
    super(leaf);
  }
  getViewType() {
    return LIST_VIEW;
  }
  getIcon() {
    return "clapperboard";
  }
  getDisplayText() {
    return "Storyboards";
  }

  async onOpen() {
    const el = this.contentEl;
    el.addClass("sb-list-view");
    const bar = el.createDiv("sb-list-bar");
    bar.createDiv({ cls: "sb-list-title", text: "Storyboards" });
    const search = bar.createEl("input", { type: "search", cls: "sb-list-search", attr: { placeholder: "Filter" } });
    search.addEventListener("input", () => {
      this.filter = search.value.toLowerCase();
      this.render();
    });
    const sort = bar.createEl("select", { cls: "dropdown" });
    sort.createEl("option", { value: "modified", text: "Recently changed" });
    sort.createEl("option", { value: "name", text: "Name" });
    sort.addEventListener("change", () => {
      this.sort = sort.value as any;
      this.render();
    });
    const b = bar.createEl("button", { cls: "mod-cta", text: "New storyboard" });
    b.addEventListener("click", () => this.host.create());
    const more = bar.createEl("button", { cls: "clickable-icon", attr: { "aria-label": "More ways to start" } });
    setIcon(more, "chevron-down");
    more.addEventListener("click", (e) => {
      const m = new Menu();
      m.addItem((i) => i.setTitle("From a Clip Studio file (.clip)…").setIcon("layers").onClick(() => this.host.createFromClip()));
      m.addItem((i) => i.setTitle("From Pictogrep's storyboard drawings").setIcon("images").onClick(() => this.host.createFromPictogrep()));
      m.showAtMouseEvent(e);
    });
    this.grid = el.createDiv("sb-list-grid");
    const refresh = () => {
      clearTimeout(this.timer);
      this.timer = window.setTimeout(() => this.render(), 400);
    };
    for (const ev of ["create", "delete", "rename", "modify"]) {
      this.registerEvent(this.app.vault.on(ev, (f: any) => f?.extension === "storyboard" && refresh()));
    }
    this.render();
  }

  async onClose() {
    this.urls.forEach((u) => URL.revokeObjectURL(u));
  }

  private files(): TFile[] {
    const files: TFile[] = this.app.vault.getFiles().filter((f: TFile) => f.extension === "storyboard" && (!this.filter || f.path.toLowerCase().includes(this.filter)));
    return files.sort((a, b) => (this.sort === "name" ? a.basename.localeCompare(b.basename, undefined, { numeric: true }) : b.stat.mtime - a.stat.mtime));
  }

  async render() {
    this.urls.forEach((u) => URL.revokeObjectURL(u));
    this.urls = [];
    this.grid.empty();
    const files = this.files();
    if (!files.length) {
      const e = this.grid.createDiv("sb-list-empty");
      e.createDiv({ text: this.filter ? "No storyboard matches." : "No storyboards yet." });
      if (!this.filter) {
        e.createDiv({ cls: "sb-hint", text: "A storyboard is a file in the vault: frames on a timeline, each with layers, colours and its references beside it." });
        e.createEl("button", { cls: "mod-cta", text: "New storyboard" }).addEventListener("click", () => this.host.create());
      }
      return;
    }
    for (const f of files) this.card(f);
  }

  private card(file: TFile) {
    const card = this.grid.createDiv("sb-card");
    const pic = card.createDiv("sb-card-pic");
    const info = card.createDiv("sb-card-info");
    info.createDiv({ cls: "sb-card-name", text: file.basename });
    const meta = info.createDiv({ cls: "sb-card-meta", text: file.parent && !file.parent.isRoot() ? file.parent.path : "" });
    card.addEventListener("click", (e) => this.host.open(file, e.metaKey || e.ctrlKey));
    card.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      this.menu(file).showAtMouseEvent(e);
    });
    peek(this.app.vault.adapter.getFullPath(file.path))
      .then(({ board, preview }) => {
        if (preview) {
          const u = URL.createObjectURL(new Blob([preview as BlobPart], { type: "image/png" }));
          this.urls.push(u);
          pic.createEl("img", { attr: { src: u } });
        } else pic.addClass("is-empty");
        meta.setText(describe(board) + (file.parent && !file.parent.isRoot() ? ` · ${file.parent.path}` : ""));
      })
      .catch(() => meta.setText("Could not be read"));
  }

  private menu(file: TFile): Menu {
    const m = new Menu();
    m.addItem((i) => i.setTitle("Open").setIcon("clapperboard").onClick(() => this.host.open(file)));
    m.addItem((i) => i.setTitle("Open in new tab").setIcon("file-plus").onClick(() => this.host.open(file, true)));
    m.addItem((i) => i.setTitle("Rename…").setIcon("pencil").onClick(() => this.host.rename(file)));
    m.addItem((i) => i.setTitle("Duplicate").setIcon("copy").onClick(() => this.host.duplicate(file)));
    m.addSeparator();
    m.addItem((i) => i.setTitle("Delete").setIcon("trash-2").setWarning(true).onClick(() => confirmDelete(this.app, file)));
    return m;
  }
}

export function describe(b: Board): string {
  const k = kindInfo(b.kind);
  const n = b.frames.length;
  const count = `${n} ${n === 1 ? k.unit : k.units}`;
  void frameStarts;
  if (isPageKind(b.kind)) return `${k.name} · ${count}${b.page?.rtl ? " · right to left" : ""}`;
  if (b.kind === "canvas") return k.name;
  return `${k.name} · ${count} · ${timecode(totalFrames(b), b.fps)} · ${b.infinite ? "infinite canvas" : `${b.width} × ${b.height}`}`;
}

export async function confirmDelete(app: any, file: TFile) {
  const m = new ConfirmationModal(app, "Delete storyboard", `"${file.basename}" goes to the trash, with every frame in it.`);
  m.open();
  if (await m.result) {
    await app.vault.trash(file, true).catch(() => app.vault.trash(file, false));
    new Notice(`"${file.basename}" moved to the trash.`);
  }
}

/** `![[x.storyboard]]` in a note: the frames in a strip, with notes on hover. */
export class StoryboardEmbed extends Component {
  private urls: string[] = [];
  constructor(private ctx: any, private file: TFile, private open: (f: TFile, newLeaf: boolean) => void) {
    super();
  }
  async loadFile() {
    const el: HTMLElement = this.ctx.containerEl;
    el.empty();
    el.addClass("sb-embed");
    const head = el.createDiv("sb-embed-head");
    setIcon(head.createSpan("sb-embed-icon"), "clapperboard");
    head.createSpan({ cls: "sb-embed-name", text: this.file.basename });
    const meta = head.createSpan("sb-embed-meta");
    const strip = el.createDiv("sb-embed-strip");
    el.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      this.open(this.file, e.metaKey || e.ctrlKey);
    });
    try {
      const { board, thumbs, preview } = await peek(this.ctx.app.vault.adapter.getFullPath(this.file.path));
      meta.setText(describe(board));
      const frames = board.frames.slice(0, 24);
      frames.forEach((f, i) => {
        const cell = strip.createDiv({ cls: "sb-embed-frame", attr: { "aria-label": [f.notes.action, f.notes.dialogue && `“${f.notes.dialogue}”`].filter(Boolean).join("\n") || `Frame ${i + 1}` } });
        const bytes = thumbs.get(f.id) || (i === 0 ? preview : null);
        if (bytes) {
          const u = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "image/png" }));
          this.urls.push(u);
          cell.createEl("img", { attr: { src: u } });
        }
        cell.createDiv({ cls: "sb-embed-num", text: `${i + 1}${f.name ? " · " + f.name : ""}` });
      });
      if (board.frames.length > frames.length) strip.createDiv({ cls: "sb-embed-more", text: `+${board.frames.length - frames.length}` });
    } catch (e) {
      meta.setText(`could not be read: ${e}`);
    }
  }
  onunload() {
    this.urls.forEach((u) => URL.revokeObjectURL(u));
  }
}

export { STORYBOARD_VIEW };
