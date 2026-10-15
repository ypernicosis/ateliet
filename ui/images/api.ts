// The image library's bridge to the Rust core (src/gallery.rs), plus the
// helpers every image view shares: preview URLs, settings, inserting a picture
// into a note.

import { fileUrl, invoke, protocolBase, readBytes } from "../tauri.ts";
import { MarkdownView } from "../obsidian/views.ts";
import { Notice } from "../obsidian/ui.ts";
import { TFile } from "../obsidian/vault.ts";

export interface ImageItem {
  key: string;
  abs: string;
  name: string;
  folder: string;
  mtime: number;
  size: number;
  width: number;
  height: number;
  external: boolean;
  indexed: boolean;
  score?: number;
}

export interface ImagePage {
  total: number;
  offset: number;
  seed: number;
  mode: "browse" | "semantic" | "name" | "similar";
  items: ImageItem[];
}

export interface ImageQuery {
  search?: string;
  folder?: string | null;
  recursive?: boolean;
  sort?: string;
  seed?: number;
  offset?: number;
  count?: number;
  similarTo?: string | null;
  keys?: string[];
}

export interface ModelStatus {
  ready: boolean;
  loaded: boolean;
  downloading: boolean;
  received: number;
  total: number;
  error: string | null;
  size: number;
}

export interface LibraryStatus {
  total: number;
  indexed: number;
  failed: number;
  working: boolean;
  scanned: boolean;
  auto: boolean;
  model: ModelStatus;
}

export interface FolderInfo {
  folder: string;
  count: number;
  cover: string;
  external: boolean;
  latest: number;
}

export interface ImageSettings {
  /** "write" or "images": which of the two layouts is on screen. */
  mode: "write" | "images";
  /** Folders outside the vault whose pictures are part of the library. */
  external: string[];
  /** Analyse new pictures automatically once the model is downloaded. */
  auto: boolean;
  /** Preview size in the gallery. */
  tile: "small" | "medium" | "large";
  showNames: boolean;
  /** Where pictures dropped into the gallery are saved ("" = attachment folder). */
  importFolder: string;
  /** The search panel was added to the writing layout once already. */
  panelAdded: boolean;
}

export const DEFAULT_SETTINGS: ImageSettings = {
  mode: "write",
  external: [],
  auto: true,
  tile: "medium",
  showNames: false,
  importFolder: "",
  panelAdded: false,
};

export const api = {
  open: (vault: string, configDir: string, external: string[], auto: boolean) => invoke<LibraryStatus>("images_open", { vault, configDir, external, auto }),
  configure: (external: string[], auto: boolean) => invoke("images_configure", { external, auto }),
  refresh: (paths: string[]) => invoke("images_refresh", { paths }),
  rescan: () => invoke("images_rescan"),
  status: () => invoke<LibraryStatus>("images_status"),
  downloadModel: () => invoke("images_model_download"),
  reindex: () => invoke("images_reindex"),
  query: (query: ImageQuery) => invoke<ImagePage>("images_query", { query }),
  folders: () => invoke<FolderInfo[]>("images_folders"),
};

export const TILE_WIDTH = { small: 150, medium: 220, large: 320 };

/** A cached preview of at most `width` pixels (served by the Rust core). */
export function thumbnailUrl(abs: string, width = 480): string {
  return `${protocolBase()}/__thumb?path=${encodeURIComponent(abs)}&w=${width}`;
}

/** The original file. */
export function fullUrl(item: ImageItem): string {
  return fileUrl(item.abs);
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** Display name of a folder key ("" is the vault root). */
export function folderName(app: any, folder: string): string {
  if (!folder) return app.vault.getName();
  const parts = folder.split(/[\\/]/);
  return parts[parts.length - 1] || folder;
}

export function vaultFile(app: any, item: ImageItem): TFile | null {
  if (item.external) return null;
  const f = app.vault.getAbstractFileByPath(item.key);
  return f instanceof TFile ? f : null;
}

/** The note an image would go into: the active one, else the most recent. */
export function targetNote(app: any): MarkdownView | null {
  const active = app.workspace.getActiveViewOfType(MarkdownView);
  if (active?.file) return active;
  let best: MarkdownView | null = null;
  let bestTime = -1;
  app.workspace.iterateAllLeaves((leaf: any) => {
    if (leaf.view instanceof MarkdownView && leaf.view.file && (leaf.activeTime || 0) > bestTime) {
      best = leaf.view;
      bestTime = leaf.activeTime || 0;
    }
  });
  return best;
}

/**
 * Makes sure the picture is a file of the vault: external pictures are copied
 * into the attachment folder of `sourcePath` (or `folder`, when given).
 */
export async function ensureInVault(app: any, item: ImageItem, sourcePath: string, folder?: string): Promise<TFile> {
  const existing = vaultFile(app, item);
  if (existing) return existing;
  const data = await readBytes(item.abs);
  let path: string;
  if (folder !== undefined && folder !== "") {
    const base = folder.replace(/\/+$/, "");
    if (!app.vault.getAbstractFileByPath(base)) await app.vault.createFolder(base).catch(() => {});
    path = app.vault.getAvailablePath(`${base}/${item.name.replace(/\.[^.]+$/, "")}`, item.name.split(".").pop());
  } else {
    path = await app.fileManager.getAvailablePathForAttachment(item.name, sourcePath);
  }
  return app.vault.createBinary(path, data);
}

/** The `![[...]]` embed of a vault file, in the user's link format. */
export function embedLink(app: any, file: TFile, sourcePath: string): string {
  const link = app.fileManager.generateMarkdownLink(file, sourcePath);
  return link.startsWith("!") ? link : "!" + link;
}

/** Inserts the pictures at the cursor of `view` (default: the target note). */
export async function insertImages(app: any, items: ImageItem[], view: MarkdownView | null = targetNote(app), at?: number): Promise<boolean> {
  if (!view?.file) {
    new Notice("Open a note to insert the image into.");
    return false;
  }
  const src = view.file.path;
  const links: string[] = [];
  for (const item of items) {
    const file = await ensureInVault(app, item, src);
    links.push(embedLink(app, file, src));
  }
  const text = links.join("\n");
  const cm = view.editMode?.cm;
  if (at != null && cm) {
    cm.dispatch({ changes: { from: at, insert: text }, selection: { anchor: at + text.length } });
  } else {
    // A picture is a block: it takes the cursor's line when that is empty,
    // otherwise the line below it.
    const editor = view.editor;
    const cur = editor.getCursor();
    const line = editor.getLine(cur.line);
    if (editor.somethingSelected() || !line.trim()) {
      editor.replaceSelection(text);
    } else {
      editor.replaceRange("\n" + text, { line: cur.line, ch: line.length });
      editor.setCursor({ line: cur.line + links.length, ch: links[links.length - 1].length });
    }
  }
  app.workspace.setActiveLeaf(view.leaf, { focus: true });
  return true;
}

/** Notes that link to or embed a vault picture. */
export function notesUsing(app: any, item: ImageItem): TFile[] {
  if (item.external) return [];
  const out: TFile[] = [];
  const links = app.metadataCache.resolvedLinks || {};
  for (const src in links) {
    if (links[src][item.key]) {
      const f = app.vault.getAbstractFileByPath(src);
      if (f instanceof TFile) out.push(f);
    }
  }
  return out.sort((a, b) => b.stat.mtime - a.stat.mtime);
}

/** Drag payload understood by the editor drop handler (see app.handleDrop). */
export function setDragData(e: DragEvent, items: ImageItem[]) {
  if (!e.dataTransfer) return;
  e.dataTransfer.effectAllowed = "copy";
  e.dataTransfer.setData("text/x-ateliet-images", JSON.stringify(items));
  const first = items[0];
  if (first && !first.external) e.dataTransfer.setData("text/x-ateliet-path", first.key);
  e.dataTransfer.setData("text/plain", items.map((i) => (i.external ? i.abs : `![[${i.key}]]`)).join("\n"));
}
