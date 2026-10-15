// FileManager: operations that need to keep links and frontmatter coherent.

import { getFrontMatterInfo, normalizePath, parseLinktext, parseYaml, stringifyYaml } from "./util.ts";
import { TAbstractFile, TFile, TFolder } from "./vault.ts";
import { relativePath } from "./metadata.ts";

export class FileManager {
  app: any;
  constructor(app: any) {
    this.app = app;
  }

  get vault() {
    return this.app.vault;
  }

  getNewFileParent(sourcePath: string, _newFilePath?: string): TFolder {
    const loc = this.vault.getConfig("newFileLocation");
    if (loc === "current" && sourcePath) {
      const src = this.vault.getAbstractFileByPath(sourcePath);
      if (src?.parent) return src.parent;
    }
    if (loc === "folder") {
      const f = this.vault.getAbstractFileByPath(this.vault.getConfig("newFileFolderPath") || "/");
      if (f instanceof TFolder) return f;
    }
    return this.vault.getRoot();
  }

  async createNewMarkdownFile(folder: TFolder | undefined, name = "Untitled", content = ""): Promise<TFile> {
    folder ||= this.getNewFileParent(this.app.workspace.getActiveFile()?.path || "");
    const base = folder.isRoot() ? name : `${folder.path}/${name}`;
    return this.vault.create(this.vault.getAvailablePath(base, "md"), content);
  }

  async createNewFile(folder: TFolder | undefined, name: string, ext: string, content = ""): Promise<TFile> {
    folder ||= this.getNewFileParent("");
    const base = folder.isRoot() ? name : `${folder.path}/${name}`;
    return this.vault.create(this.vault.getAvailablePath(base, ext), content);
  }

  async createNewFolder(parent?: TFolder): Promise<TFolder> {
    parent ||= this.vault.getRoot();
    const base = parent.isRoot() ? "Untitled" : `${parent.path}/Untitled`;
    return this.vault.createFolder(this.vault.getAvailablePath(base, ""));
  }

  generateMarkdownLink(file: TFile, sourcePath: string, subpath = "", alias = ""): string {
    const useMd = this.vault.getConfig("useMarkdownLinks");
    const embed = file.extension !== "md" && ["png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "avif"].includes(file.extension);
    let linktext = this.app.metadataCache.fileToLinktext(file, sourcePath, !useMd);
    if (useMd) {
      const enc = encodeURI(linktext + (subpath || "")).replace(/\(/g, "%28").replace(/\)/g, "%29");
      return `${embed ? "!" : ""}[${alias || file.basename}](${enc})`;
    }
    return `${embed ? "!" : ""}[[${linktext}${subpath || ""}${alias ? "|" + alias : ""}]]`;
  }

  async processFrontMatter(file: TFile, fn: (frontmatter: any) => void, options?: any): Promise<void> {
    await this.vault.process(
      file,
      (content: string) => {
        const info = getFrontMatterInfo(content);
        const data = info.exists ? parseYaml(info.frontmatter) || {} : {};
        fn(data);
        const yaml = Object.keys(data).length ? stringifyYaml(data) : "";
        const body = info.exists ? content.slice(info.contentStart) : content;
        if (!yaml) return info.exists ? body : content;
        return `---\n${yaml}---\n${body}`;
      },
      options,
    );
  }

  async trashFile(file: TAbstractFile): Promise<void> {
    const opt = this.vault.getConfig("trashOption");
    if (opt === "none") return this.vault.delete(file, true);
    return this.vault.trash(file, opt !== "local");
  }

  async promptForDeletion(file: TAbstractFile): Promise<void> {
    if (this.vault.getConfig("promptDelete")) {
      const ok = await this.app.confirm(`Delete "${file.name}"?`, file instanceof TFolder ? "The folder and all of its contents will be deleted." : "");
      if (!ok) return;
    }
    await this.trashFile(file);
  }

  async getAvailablePathForAttachment(filename: string, sourcePath?: string): Promise<string> {
    const setting: string = this.vault.getConfig("attachmentFolderPath") || "/";
    const dot = filename.lastIndexOf(".");
    const base = dot > 0 ? filename.slice(0, dot) : filename;
    const ext = dot > 0 ? filename.slice(dot + 1) : "";
    let folder = "";
    const srcDir = sourcePath && sourcePath.includes("/") ? sourcePath.slice(0, sourcePath.lastIndexOf("/")) : "";
    if (setting === "/" || setting === "") folder = "";
    else if (setting === "./" || setting === ".") folder = srcDir;
    else if (setting.startsWith("./")) folder = normalizePath((srcDir ? srcDir + "/" : "") + setting.slice(2));
    else folder = normalizePath(setting);
    if (folder && folder !== "/" && !this.vault.getAbstractFileByPath(folder)) await this.vault.createFolder(folder).catch(() => {});
    return this.vault.getAvailablePath(folder && folder !== "/" ? `${folder}/${base}` : base, ext);
  }

  /** Rename and rewrite every link pointing at the file (Obsidian's "Automatically update internal links"). */
  async renameFile(file: TAbstractFile, newPath: string): Promise<void> {
    const mc = this.app.metadataCache;
    const update = this.vault.getConfig("alwaysUpdateLinks") !== false;
    const affected: TFile[] = [];
    if (file instanceof TFile) affected.push(file);
    else if (file instanceof TFolder) this.vault.constructor.recurseChildren(file, (f: any) => f instanceof TFile && affected.push(f));

    // Capture who links to what before the move, with the resolution as it is now.
    type Edit = { source: string; ref: any; target: TFile };
    const edits: Edit[] = [];
    if (update) {
      const targets = new Set(affected);
      for (const src in mc.resolvedLinks) {
        const cache = mc.getCache(src);
        for (const ref of [...(cache?.links || []), ...(cache?.embeds || []), ...(cache?.frontmatterLinks || [])]) {
          const dest = mc.getFirstLinkpathDest(parseLinktext(ref.link).path, src);
          if (dest && targets.has(dest)) edits.push({ source: src, ref, target: dest });
        }
      }
    }
    const oldPaths = new Map(affected.map((f) => [f, f.path]));
    await this.vault.rename(file, newPath);
    if (!update || edits.length === 0) return;

    // Group by source file; sources may themselves have moved.
    const bySource = new Map<TFile, Edit[]>();
    for (const e of edits) {
      let src = this.vault.getAbstractFileByPath(e.source) as TFile | null;
      if (!src) for (const [f, p] of oldPaths) if (p === e.source) src = f;
      if (!(src instanceof TFile)) continue;
      (bySource.get(src) || bySource.set(src, []).get(src)!).push(e);
    }
    for (const [src, list] of bySource) {
      await this.vault.process(src, (content: string) => {
        const sorted = list.filter((e) => e.ref.position).sort((a, b) => b.ref.position.start.offset - a.ref.position.start.offset);
        for (const e of sorted) {
          const { start, end } = e.ref.position;
          const original = content.slice(start.offset, end.offset);
          if (original !== e.ref.original) continue;
          content = content.slice(0, start.offset) + this.rewriteLink(e.ref, e.target, src.path) + content.slice(end.offset);
        }
        // frontmatter links (no position): textual replace
        for (const e of list.filter((x) => !x.ref.position)) {
          content = content.replace(e.ref.original, this.rewriteLink(e.ref, e.target, src.path));
        }
        return content;
      });
    }
  }

  private rewriteLink(ref: any, target: TFile, sourcePath: string): string {
    const original: string = ref.original;
    const { subpath } = parseLinktext(ref.link);
    const isEmbed = original.startsWith("!");
    if (original.includes("[[")) {
      const m = /\|([^\]]*)\]\]$/.exec(original);
      const alias = m ? m[1] : "";
      const text = this.app.metadataCache.fileToLinktext(target, sourcePath, true);
      return `${isEmbed ? "!" : ""}[[${text}${subpath}${alias ? "|" + alias : ""}]]`;
    }
    const m = /^!?\[([^\]]*)\]/.exec(original);
    const display = m ? m[1] : target.basename;
    const fmt = this.vault.getConfig("newLinkFormat");
    const p = fmt === "absolute" ? target.path : relativePath(sourcePath, target.path);
    return `${isEmbed ? "!" : ""}[${display}](${encodeURI(p + subpath)})`;
  }
}
