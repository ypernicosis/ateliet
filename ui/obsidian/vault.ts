// Vault, files and the file system adapter.

import { Events } from "./events.ts";
import { normalizePath } from "./util.ts";
import { fileUrl, invoke, readBytes, writeBytes } from "../tauri.ts";

export abstract class TAbstractFile {
  vault!: Vault;
  path = "";
  name = "";
  parent: TFolder | null = null;
}

export class TFile extends TAbstractFile {
  stat = { ctime: 0, mtime: 0, size: 0 };
  basename = "";
  extension = "";
  /** @internal */ deleted = false;

  /** @internal */ setPath(path: string) {
    this.path = path;
    this.name = path.split("/").pop() || path;
    const dot = this.name.lastIndexOf(".");
    this.basename = dot > 0 ? this.name.slice(0, dot) : this.name;
    this.extension = dot > 0 ? this.name.slice(dot + 1).toLowerCase() : "";
  }
}

export class TFolder extends TAbstractFile {
  children: TAbstractFile[] = [];
  isRoot(): boolean {
    return this.path === "/" || this.path === "";
  }
  /** @internal */ setPath(path: string) {
    this.path = path;
    this.name = path === "/" ? "" : path.split("/").pop() || path;
  }
}

export class TExternalFile {}

const dirname = (p: string) => (p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "");

// ── adapter ───────────────────────────────────────────────────────────────

export class FileSystemAdapter {
  basePath: string;
  /** Node-style shims, for plugins that reach for `adapter.fs` / `adapter.path`. */
  fs: any;
  path: any;
  url: any = { pathToFileURL: (p: string) => new URL("file://" + p) };

  constructor(basePath: string) {
    this.basePath = basePath.replace(/\/+$/, "");
  }

  getName(): string {
    return this.basePath.split("/").pop() || this.basePath;
  }
  getBasePath(): string {
    return this.basePath;
  }
  getFullPath(normalizedPath: string): string {
    if (!normalizedPath || normalizedPath === "/") return this.basePath;
    return this.basePath + "/" + normalizedPath.replace(/^\/+/, "");
  }
  getFullRealPath(p: string): string {
    return this.getFullPath(p);
  }
  getFilePath(normalizedPath: string): string {
    return "file://" + encodeURI(this.getFullPath(normalizedPath));
  }
  getResourcePath(normalizedPath: string): string {
    return fileUrl(this.getFullPath(normalizedPath)) + "?" + Date.now();
  }
  static readLocalFile(path: string) {
    return readBytes(path);
  }
  static mkdir(path: string) {
    return invoke("fs_mkdir", { path });
  }

  async exists(normalizedPath: string, _sensitive?: boolean): Promise<boolean> {
    return invoke("fs_exists", { path: this.getFullPath(normalizedPath) });
  }
  async stat(normalizedPath: string): Promise<any> {
    return invoke("fs_stat", { path: this.getFullPath(normalizedPath) });
  }
  async list(normalizedPath: string): Promise<{ files: string[]; folders: string[] }> {
    const p = normalizedPath === "/" ? "" : normalizedPath.replace(/\/+$/, "");
    const r = await invoke<any>("fs_list", { path: this.getFullPath(p) });
    const join = (n: string) => (p ? p + "/" + n : n);
    return { files: r.files.map(join), folders: r.folders.map(join) };
  }
  async read(normalizedPath: string): Promise<string> {
    return invoke("fs_read_text", { path: this.getFullPath(normalizedPath) });
  }
  async readBinary(normalizedPath: string): Promise<ArrayBuffer> {
    return readBytes(this.getFullPath(normalizedPath));
  }
  async write(normalizedPath: string, data: string, _options?: any): Promise<void> {
    await invoke("fs_write_text", { path: this.getFullPath(normalizedPath), data });
  }
  async writeBinary(normalizedPath: string, data: ArrayBuffer, _options?: any): Promise<void> {
    await writeBytes(this.getFullPath(normalizedPath), data);
  }
  async append(normalizedPath: string, data: string, _options?: any): Promise<void> {
    await invoke("fs_append", { path: this.getFullPath(normalizedPath), data });
  }
  async appendBinary(normalizedPath: string, data: ArrayBuffer): Promise<void> {
    const prev = (await this.exists(normalizedPath)) ? new Uint8Array(await this.readBinary(normalizedPath)) : new Uint8Array();
    const next = new Uint8Array(prev.length + data.byteLength);
    next.set(prev);
    next.set(new Uint8Array(data), prev.length);
    await this.writeBinary(normalizedPath, next.buffer);
  }
  async process(normalizedPath: string, fn: (data: string) => string, options?: any): Promise<string> {
    const data = fn(await this.read(normalizedPath));
    await this.write(normalizedPath, data, options);
    return data;
  }
  async mkdir(normalizedPath: string): Promise<void> {
    await invoke("fs_mkdir", { path: this.getFullPath(normalizedPath) });
  }
  async trashSystem(normalizedPath: string): Promise<boolean> {
    try {
      await invoke("fs_trash", { path: this.getFullPath(normalizedPath) });
      return true;
    } catch {
      return false;
    }
  }
  async trashLocal(normalizedPath: string): Promise<void> {
    await invoke("fs_trash_local", { vault: this.basePath, rel: normalizedPath });
  }
  async rmdir(normalizedPath: string, recursive: boolean): Promise<void> {
    await invoke("fs_rmdir", { path: this.getFullPath(normalizedPath), recursive });
  }
  async remove(normalizedPath: string): Promise<void> {
    await invoke("fs_remove", { path: this.getFullPath(normalizedPath) });
  }
  async rename(from: string, to: string): Promise<void> {
    await invoke("fs_rename", { from: this.getFullPath(from), to: this.getFullPath(to) });
  }
  async copy(from: string, to: string): Promise<void> {
    await invoke("fs_copy", { from: this.getFullPath(from), to: this.getFullPath(to) });
  }
}

export class CapacitorAdapter {}

// ── vault ─────────────────────────────────────────────────────────────────

export class Vault extends Events {
  adapter: FileSystemAdapter;
  configDir = ".obsidian";
  fileMap: Record<string, TAbstractFile> = {};
  /** @internal lower-cased path → entries, so case-insensitive lookups don't scan the vault */
  private lowerMap = new Map<string, TAbstractFile[]>();
  root: TFolder;
  config: Record<string, any> = {};
  /** @internal */ private contentCache = new Map<string, { mtime: number; data: string }>();
  /** @internal */ app: any;

  constructor(basePath: string) {
    super();
    this.adapter = new FileSystemAdapter(basePath);
    this.root = new TFolder();
    this.root.vault = this;
    this.root.setPath("/");
    this.setEntry("/", this.root);
  }

  private setEntry(path: string, f: TAbstractFile) {
    this.fileMap[path] = f;
    const k = path.toLowerCase();
    const list = this.lowerMap.get(k);
    if (list) list.push(f);
    else this.lowerMap.set(k, [f]);
  }

  private deleteEntry(path: string) {
    const f = this.fileMap[path];
    delete this.fileMap[path];
    const k = path.toLowerCase();
    const list = this.lowerMap.get(k);
    if (!list) return;
    const rest = list.filter((x) => x !== f);
    if (rest.length) this.lowerMap.set(k, rest);
    else this.lowerMap.delete(k);
  }

  static recurseChildren(root: TFolder, cb: (file: TAbstractFile) => any): void {
    const stack: TAbstractFile[] = [root];
    while (stack.length) {
      const f = stack.pop()!;
      cb(f);
      if (f instanceof TFolder) stack.push(...f.children);
    }
  }

  getName(): string {
    return this.adapter.getName();
  }
  getRoot(): TFolder {
    return this.root;
  }
  getAbstractFileByPath(path: string): TAbstractFile | null {
    const p = normalizePath(path);
    return this.fileMap[p] || null;
  }
  getAbstractFileByPathInsensitive(path: string): TAbstractFile | null {
    return this.lowerMap.get(normalizePath(path).toLowerCase())?.[0] || null;
  }
  getFileByPath(path: string): TFile | null {
    const f = this.getAbstractFileByPath(path);
    return f instanceof TFile ? f : null;
  }
  getFolderByPath(path: string): TFolder | null {
    const f = this.getAbstractFileByPath(path);
    return f instanceof TFolder ? f : null;
  }
  getFiles(): TFile[] {
    return Object.values(this.fileMap).filter((f): f is TFile => f instanceof TFile);
  }
  getMarkdownFiles(): TFile[] {
    return this.getFiles().filter((f) => f.extension === "md");
  }
  getAllLoadedFiles(): TAbstractFile[] {
    return Object.values(this.fileMap);
  }
  getAllFolders(includeRoot = false): TFolder[] {
    return Object.values(this.fileMap).filter((f): f is TFolder => f instanceof TFolder && (includeRoot || !f.isRoot()));
  }

  getConfig(key: string): any {
    if (key in this.config) return this.config[key];
    return VAULT_DEFAULTS[key];
  }
  setConfig(key: string, value: any): void {
    this.config[key] = value;
    this.app?.onConfigChanged?.(key);
    this.app?.saveAppConfig?.();
    this.trigger("config-changed", key, value);
  }

  // ── tree maintenance (internal) ──────────────────────────────────────────
  /** @internal */ addEntry(path: string, folder: boolean, stat?: any): TAbstractFile {
    path = normalizePath(path);
    const existing = this.fileMap[path];
    if (existing) {
      if (existing instanceof TFile && stat) existing.stat = { ctime: stat.ctime, mtime: stat.mtime, size: stat.size };
      return existing;
    }
    const parentPath = dirname(path) || "/";
    const parent = (this.fileMap[parentPath] as TFolder) || (this.addEntry(parentPath, true) as TFolder);
    let f: TAbstractFile;
    if (folder) {
      const d = new TFolder();
      d.setPath(path);
      f = d;
    } else {
      const t = new TFile();
      t.setPath(path);
      if (stat) t.stat = { ctime: stat.ctime, mtime: stat.mtime, size: stat.size };
      f = t;
    }
    f.vault = this;
    f.parent = parent;
    parent.children.push(f);
    this.setEntry(path, f);
    return f;
  }

  /** @internal */ removeEntry(file: TAbstractFile): void {
    if (file instanceof TFolder) for (const c of file.children.slice()) this.removeEntry(c);
    this.deleteEntry(file.path);
    if (file.parent) file.parent.children.remove(file);
    if (file instanceof TFile) {
      file.deleted = true;
      this.contentCache.delete(file.path);
    }
  }

  /** @internal */ movePath(file: TAbstractFile, newPath: string): void {
    const oldPath = file.path;
    this.deleteEntry(oldPath);
    file.parent?.children.remove(file);
    (file as any).setPath(newPath);
    const parentPath = dirname(newPath) || "/";
    const parent = (this.fileMap[parentPath] as TFolder) || (this.addEntry(parentPath, true) as TFolder);
    file.parent = parent;
    parent.children.push(file);
    this.setEntry(newPath, file);
    const cached = this.contentCache.get(oldPath);
    if (cached) {
      this.contentCache.delete(oldPath);
      this.contentCache.set(newPath, cached);
    }
    if (file instanceof TFolder) {
      for (const c of file.children.slice()) {
        const childOld = c.path;
        this.movePath(c, newPath + "/" + c.name);
        this.trigger("rename", c, childOld);
      }
    }
  }

  /** @internal */ load(entries: { path: string; folder: boolean; size: number; ctime: number; mtime: number }[]) {
    entries.sort((a, b) => a.path.length - b.path.length);
    for (const e of entries) this.addEntry(e.path, e.folder, e);
  }

  /** @internal */ isIgnoredPath(path: string): boolean {
    return path.split("/").some((seg) => seg.startsWith("."));
  }

  /** @internal Reconcile external file system changes reported by the watcher. */
  async reconcile(paths: string[]): Promise<void> {
    for (const raw of paths) {
      const path = normalizePath(raw);
      this.trigger("raw", path);
      if (this.isIgnoredPath(path)) continue;
      const stat = await invoke<any>("fs_stat", { path: this.adapter.getFullPath(path) });
      const known = this.fileMap[path];
      if (!stat) {
        if (known) {
          this.removeEntry(known);
          this.trigger("delete", known);
        }
        continue;
      }
      if (!known) {
        const f = this.addEntry(path, stat.type === "folder", stat);
        this.trigger("create", f);
        if (f instanceof TFolder) {
          // A whole folder appeared (moved in, unzipped …): pick up its content.
          const list = await this.adapter.list(path);
          await this.reconcile([...list.folders, ...list.files]);
        }
        continue;
      }
      if (known instanceof TFile && stat.type === "file" && (stat.mtime !== known.stat.mtime || stat.size !== known.stat.size)) {
        known.stat = { ctime: stat.ctime, mtime: stat.mtime, size: stat.size };
        this.contentCache.delete(path);
        this.trigger("modify", known);
      }
    }
  }

  // ── reading ──────────────────────────────────────────────────────────────
  async read(file: TFile): Promise<string> {
    const data = await this.adapter.read(file.path);
    this.contentCache.set(file.path, { mtime: file.stat.mtime, data });
    return data;
  }
  async cachedRead(file: TFile): Promise<string> {
    const c = this.contentCache.get(file.path);
    if (c && c.mtime === file.stat.mtime) return c.data;
    return this.read(file);
  }
  readBinary(file: TFile): Promise<ArrayBuffer> {
    return this.adapter.readBinary(file.path);
  }
  getResourcePath(file: TFile): string {
    return fileUrl(this.adapter.getFullPath(file.path)) + "?" + file.stat.mtime;
  }

  // ── writing ──────────────────────────────────────────────────────────────
  private async afterWrite(file: TFile, data: string | null, stat: any) {
    if (stat) file.stat = { ctime: stat.ctime, mtime: stat.mtime, size: stat.size };
    if (data !== null) this.contentCache.set(file.path, { mtime: file.stat.mtime, data });
    else this.contentCache.delete(file.path);
  }

  private checkNew(path: string) {
    path = normalizePath(path);
    if (this.fileMap[path]) throw new Error("File already exists.");
    return path;
  }

  async create(path: string, data: string, _options?: any): Promise<TFile> {
    path = this.checkNew(path);
    const stat = await invoke<any>("fs_write_text", { path: this.adapter.getFullPath(path), data });
    const f = this.addEntry(path, false, stat) as TFile;
    await this.afterWrite(f, data, stat);
    this.trigger("create", f);
    return f;
  }
  async createBinary(path: string, data: ArrayBuffer, _options?: any): Promise<TFile> {
    path = this.checkNew(path);
    const stat = await writeBytes(this.adapter.getFullPath(path), data);
    const f = this.addEntry(path, false, stat) as TFile;
    this.trigger("create", f);
    return f;
  }
  async createFolder(path: string): Promise<TFolder> {
    path = normalizePath(path);
    if (this.fileMap[path]) throw new Error("Folder already exists.");
    await this.adapter.mkdir(path);
    const f = this.addEntry(path, true) as TFolder;
    this.trigger("create", f);
    return f;
  }
  async modify(file: TFile, data: string, _options?: any): Promise<void> {
    const stat = await invoke<any>("fs_write_text", { path: this.adapter.getFullPath(file.path), data });
    await this.afterWrite(file, data, stat);
    this.trigger("modify", file);
  }
  async modifyBinary(file: TFile, data: ArrayBuffer, _options?: any): Promise<void> {
    const stat = await writeBytes(this.adapter.getFullPath(file.path), data);
    await this.afterWrite(file, null, stat);
    this.trigger("modify", file);
  }
  async append(file: TFile, data: string, _options?: any): Promise<void> {
    const stat = await invoke<any>("fs_append", { path: this.adapter.getFullPath(file.path), data });
    await this.afterWrite(file, null, stat);
    this.trigger("modify", file);
  }
  async appendBinary(file: TFile, data: ArrayBuffer): Promise<void> {
    await this.adapter.appendBinary(file.path, data);
    const stat = await this.adapter.stat(file.path);
    await this.afterWrite(file, null, stat);
    this.trigger("modify", file);
  }
  async process(file: TFile, fn: (data: string) => string, options?: any): Promise<string> {
    const data = fn(await this.read(file));
    await this.modify(file, data, options);
    return data;
  }

  async delete(file: TAbstractFile, _force?: boolean): Promise<void> {
    if (file instanceof TFolder) await this.adapter.rmdir(file.path, true);
    else await this.adapter.remove(file.path);
    this.removeEntry(file);
    this.trigger("delete", file);
  }
  async trash(file: TAbstractFile, system: boolean): Promise<void> {
    let ok = false;
    if (system) ok = await this.adapter.trashSystem(file.path);
    if (!ok) await this.adapter.trashLocal(file.path);
    this.removeEntry(file);
    this.trigger("delete", file);
  }
  async rename(file: TAbstractFile, newPath: string): Promise<void> {
    newPath = normalizePath(newPath);
    if (newPath === file.path) return;
    if (this.fileMap[newPath] && newPath.toLowerCase() !== file.path.toLowerCase()) throw new Error("Destination file already exists!");
    const oldPath = file.path;
    await this.adapter.rename(oldPath, newPath);
    this.movePath(file, newPath);
    this.trigger("rename", file, oldPath);
  }
  async copy<T extends TAbstractFile>(file: T, newPath: string): Promise<T> {
    newPath = this.checkNew(newPath);
    await this.adapter.copy(file.path, newPath);
    const stat = await this.adapter.stat(newPath);
    const f = this.addEntry(newPath, file instanceof TFolder, stat);
    if (f instanceof TFolder) {
      const list = await this.adapter.list(newPath);
      await this.reconcile([...list.folders, ...list.files]);
    }
    this.trigger("create", f);
    return f as T;
  }

  /** @internal */ getAvailablePath(base: string, ext: string): string {
    let i = 0;
    for (;;) {
      const p = normalizePath(`${base}${i ? " " + i : ""}${ext ? "." + ext : ""}`);
      if (!this.getAbstractFileByPathInsensitive(p)) return p;
      i++;
    }
  }
  /** @internal */ getAvailablePathForAttachments(base: string, ext: string, sourceFile: TFile | null): Promise<string> {
    return this.app.fileManager.getAvailablePathForAttachment(`${base}.${ext}`, sourceFile?.path);
  }
}

export const VAULT_DEFAULTS: Record<string, any> = {
  accentColor: "",
  alwaysUpdateLinks: true,
  attachmentFolderPath: "/",
  autoConvertHtml: true,
  autoPairBrackets: true,
  autoPairMarkdown: true,
  baseFontSize: 16,
  baseFontSizeAction: true,
  cssTheme: "",
  defaultViewMode: "source",
  enabledCssSnippets: [],
  focusNewTab: true,
  foldHeading: true,
  foldIndent: true,
  interfaceFontFamily: "",
  legacyEditor: false,
  livePreview: true,
  mobileToolbarCommands: [],
  monospaceFontFamily: "",
  newFileFolderPath: "/",
  newFileLocation: "root",
  newLinkFormat: "shortest",
  nativeMenus: false,
  promptDelete: true,
  propertiesInDocument: "visible",
  readableLineLength: true,
  rightToLeft: false,
  showFrontmatter: false,
  showIndentGuide: true,
  showInlineTitle: true,
  showLineNumber: false,
  showRibbon: true,
  showUnsupportedFiles: false,
  showViewHeader: true,
  smartIndentList: true,
  spellcheck: false,
  spellcheckLanguages: null,
  strictLineBreaks: false,
  tabSize: 4,
  textFontFamily: "",
  theme: "obsidian",
  translucency: false,
  trashOption: "system",
  types: {},
  useMarkdownLinks: false,
  useTab: true,
  userIgnoreFilters: null,
  vimMode: false,
};
