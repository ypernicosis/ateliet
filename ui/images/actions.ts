// What can be done with a picture, from any image view: the context menu and
// the commands behind it. Pictures and notes meet here: insert into a note,
// start a note from a picture, see the notes that use it.

import { ImageItem, embedLink, ensureInVault, fullUrl, insertImages, notesUsing, vaultFile } from "./api.ts";
import { ConfirmationModal, FuzzySuggestModal, Menu, Notice } from "../obsidian/ui.ts";
import { TFile, TFolder } from "../obsidian/vault.ts";
import { invoke } from "../tauri.ts";

/** What the views need from the images plugin. */
export interface ImagesHost {
  app: any;
  settings: any;
  /** Opens (or focuses) the gallery with this state. */
  openLibrary(state: any, newLeaf?: boolean): Promise<void>;
  /** Opens a note next to the pictures (the companion pane in image mode). */
  openNote(file: TFile, newLeaf?: boolean): Promise<void>;
  isImageMode(): boolean;
  /** Last known state of the library (indexing progress, model). */
  status: any;
  updateSettings(patch: any): Promise<void>;
  addExternalFolder(): Promise<void>;
  setMode(mode: "write" | "images"): Promise<void>;
  removeExternalFolder(path: string): Promise<void>;
}

class FolderPicker extends FuzzySuggestModal<TFolder> {
  constructor(app: any, private done: (f: TFolder) => void) {
    super(app);
    this.setPlaceholder("Move to folder…");
  }
  getItems(): TFolder[] {
    return this.app.vault.getAllLoadedFiles().filter((f: any) => f instanceof TFolder);
  }
  getItemText(f: TFolder): string {
    return f.isRoot() ? "/" : f.path;
  }
  onChooseItem(f: TFolder) {
    this.done(f);
  }
}

export async function moveToFolder(host: ImagesHost, items: ImageItem[]) {
  const files = items.map((i) => vaultFile(host.app, i)).filter((f): f is TFile => !!f);
  const external = items.filter((i) => i.external);
  new FolderPicker(host.app, async (folder) => {
    const base = folder.isRoot() ? "" : folder.path + "/";
    for (const f of files) {
      const dest = host.app.vault.getAvailablePath(base + f.basename, f.extension);
      await host.app.fileManager.renameFile(f, dest);
    }
    for (const item of external) await ensureInVault(host.app, item, "", folder.isRoot() ? "/" : folder.path);
    new Notice(`${items.length} ${items.length === 1 ? "picture" : "pictures"} moved to ${folder.isRoot() ? "the vault root" : folder.path}.`);
  }).open();
}

export async function newNoteWithImages(host: ImagesHost, items: ImageItem[]) {
  const app = host.app;
  const first = items[0];
  const folder = first && !first.external ? first.folder : app.fileManager.getNewFileParent?.("")?.path || "";
  const base = (folder ? folder + "/" : "") + (items.length === 1 ? first.name.replace(/\.[^.]+$/, "") : "Untitled");
  const path = app.vault.getAvailablePath(base, "md");
  const lines: string[] = [];
  for (const item of items) lines.push(embedLink(app, await ensureInVault(app, item, path), path));
  const file = await app.vault.create(path, lines.join("\n") + "\n\n");
  await host.openNote(file);
}

async function copyImage(item: ImageItem) {
  try {
    const blob = await (await fetch(fullUrl(item))).blob();
    let png = blob;
    if (blob.type !== "image/png") {
      const bmp = await createImageBitmap(blob);
      const canvas = document.createElement("canvas");
      canvas.width = bmp.width;
      canvas.height = bmp.height;
      canvas.getContext("2d")!.drawImage(bmp, 0, 0);
      png = await new Promise<Blob>((r) => canvas.toBlob((b) => r(b!), "image/png"));
    }
    await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
    new Notice("Image copied.");
  } catch (e) {
    new Notice("Could not copy the image: " + e);
  }
}

export async function deleteImages(host: ImagesHost, items: ImageItem[]) {
  const files = items.map((i) => vaultFile(host.app, i)).filter((f): f is TFile => !!f);
  if (!files.length) return;
  const used = files.filter((f) => notesUsing(host.app, { key: f.path, external: false } as ImageItem).length > 0).length;
  const msg = (files.length === 1 ? `"${files[0].name}" will be moved to the trash.` : `${files.length} pictures will be moved to the trash.`) + (used ? ` ${used === 1 && files.length === 1 ? "It is" : `${used} of them are`} used in notes.` : "");
  const modal = new ConfirmationModal(host.app, files.length === 1 ? "Delete picture" : "Delete pictures", msg);
  modal.open();
  if (!(await modal.result)) return;
  for (const f of files) await host.app.fileManager.trashFile(f);
}

/** The context menu of one picture, or of a selection. */
export function imageMenu(host: ImagesHost, items: ImageItem[], extra?: (menu: Menu) => void): Menu {
  const app = host.app;
  const menu = new Menu();
  const one = items.length === 1 ? items[0] : null;
  const file = one ? vaultFile(app, one) : null;
  menu.addItem((i) => i.setTitle(items.length > 1 ? `Insert ${items.length} pictures into note` : "Insert into note").setIcon("file-input").onClick(() => insertImages(app, items)));
  menu.addItem((i) => i.setTitle(items.length > 1 ? "New note with these pictures" : "New note with this picture").setIcon("file-plus").onClick(() => newNoteWithImages(host, items)));
  if (one) {
    const notes = notesUsing(app, one);
    if (notes.length) {
      menu.addItem((i) => {
        i.setTitle(`Used in ${notes.length} ${notes.length === 1 ? "note" : "notes"}`).setIcon("links-coming-in");
        const sub = (i as any).setSubmenu?.();
        if (sub) for (const n of notes.slice(0, 20)) sub.addItem((s: any) => s.setTitle(n.basename).setIcon("file-text").onClick(() => host.openNote(n)));
        else i.onClick(() => host.openNote(notes[0]));
      });
    }
  }
  menu.addSeparator();
  if (one) {
    menu.addItem((i) => i.setTitle("Find similar pictures").setIcon("sparkles").setDisabled(!one.indexed).onClick(() => host.openLibrary({ similarTo: one.key, similarName: one.name, search: "" })));
    if (file) menu.addItem((i) => i.setTitle("Open in new tab").setIcon("file-image").onClick(() => app.workspace.getLeaf("tab").openFile(file)));
    menu.addItem((i) => i.setTitle("Copy image").setIcon("copy").onClick(() => copyImage(one)));
    if (file) menu.addItem((i) => i.setTitle("Copy embed link").setIcon("link").onClick(() => navigator.clipboard.writeText(embedLink(app, file, ""))));
    menu.addItem((i) => i.setTitle("Show in system explorer").setIcon("folder-open").onClick(() => invoke("show_in_folder", { path: one.abs })));
  }
  if (items.some((i) => i.external)) {
    menu.addItem((i) =>
      i.setTitle("Copy into vault").setIcon("download").onClick(async () => {
        for (const it of items.filter((x) => x.external)) await ensureInVault(app, it, "", host.settings.importFolder || undefined);
        new Notice("Copied into the vault.");
      }),
    );
  }
  menu.addItem((i) => i.setTitle("Move to folder…").setIcon("folder-input").onClick(() => moveToFolder(host, items)));
  extra?.(menu);
  if (items.some((i) => !i.external)) {
    menu.addSeparator();
    menu.addItem((i) => i.setTitle("Delete").setIcon("trash-2").setWarning?.(true).onClick(() => deleteImages(host, items)));
  }
  return menu;
}
