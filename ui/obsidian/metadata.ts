// MetadataCache: parsed metadata comes from the Rust core (`md_parse`,
// `vault_metadata_all`); link resolution lives here because plugins call it
// synchronously.

import { Events } from "./events.ts";
import { getLinkpath, iterateCacheRefs, normalizePath, parseFrontMatterAliases, parseFrontMatterTags } from "./util.ts";
import { TFile, Vault } from "./vault.ts";
import { invoke } from "../tauri.ts";

export class MetadataCache extends Events {
  vault: Vault;
  app: any;
  resolvedLinks: Record<string, Record<string, number>> = {};
  unresolvedLinks: Record<string, Record<string, number>> = {};
  /** path → { mtime, size, hash } (hash = path here) */
  fileCache: Record<string, { mtime: number; size: number; hash: string }> = {};
  /** hash → CachedMetadata */
  metadataCache: Record<string, any> = {};
  initialized = false;
  resolved = false;
  inProgressTaskCount = 0;
  private byName = new Map<string, TFile[]>(); // lower basename (md) or name → files
  /** lower last segment of each link target → source paths, so a file appearing or
   *  disappearing only re-resolves the notes that could link to it */
  private linkSources = new Map<string, Set<string>>();
  private sourceKeys = new Map<string, string[]>();
  private queue = new Map<string, Promise<void>>();

  constructor(app: any, vault: Vault) {
    super();
    this.app = app;
    this.vault = vault;
  }

  // ── public API ───────────────────────────────────────────────────────────
  getFileCache(file: TFile | null): any {
    if (!file) return null;
    return this.getCache(file.path);
  }

  getCache(path: string): any {
    const fc = this.fileCache[path];
    return fc ? this.metadataCache[fc.hash] ?? null : null;
  }

  getFirstLinkpathDest(linkpath: string, sourcePath: string): TFile | null {
    linkpath = getLinkpath(linkpath).trim();
    if (!linkpath) {
      const src = this.vault.getAbstractFileByPath(sourcePath);
      return src instanceof TFile ? src : null;
    }
    linkpath = linkpath.replace(/^\.\//, "");
    let lp = normalizePath(linkpath.startsWith("/") ? linkpath.slice(1) : linkpath);
    try {
      lp = decodeURIComponent(lp);
    } catch { /* keep raw */ }
    const lower = lp.toLowerCase();
    const hasExt = /\.[^/.]+$/.test(lp);
    const direct = (p: string) => {
      const f = this.vault.getAbstractFileByPath(p) || this.vault.getAbstractFileByPathInsensitive(p);
      return f instanceof TFile ? f : null;
    };
    // 1. absolute path, 2. relative to the source folder
    const srcDir = sourcePath.includes("/") ? sourcePath.slice(0, sourcePath.lastIndexOf("/")) : "";
    for (const base of [lp, srcDir ? resolveRelative(srcDir, lp) : null]) {
      if (!base) continue;
      const f = direct(base) || direct(base + ".md");
      if (f) return f;
    }
    // 3. by name, preferring the same folder, then the shortest path
    const name = lower.split("/").pop()!;
    const candidates = (this.byName.get(name) || []).concat(hasExt ? [] : this.byName.get(name + ".md") || []);
    const matching = candidates.filter((f) => {
      const p = f.path.toLowerCase();
      const noExt = f.extension === "md" ? p.slice(0, -3) : p;
      return lower.includes("/") ? noExt.endsWith(lower) || p.endsWith(lower) : true;
    });
    if (matching.length === 0) return null;
    if (matching.length === 1) return matching[0];
    const same = matching.find((f) => (f.parent?.path === "/" ? "" : f.parent?.path) === srcDir);
    if (same) return same;
    return matching.slice().sort((a, b) => a.path.length - b.path.length || a.path.localeCompare(b.path))[0];
  }

  fileToLinktext(file: TFile, sourcePath: string, omitMdExtension = true): string {
    const format = this.vault.getConfig("newLinkFormat");
    const strip = (p: string) => (omitMdExtension && file.extension === "md" ? p.slice(0, -3) : p);
    if (format === "absolute") return strip(file.path);
    if (format === "relative") return strip(relativePath(sourcePath, file.path));
    // shortest: bare name if unambiguous
    const name = omitMdExtension && file.extension === "md" ? file.basename : file.name;
    const dest = this.getFirstLinkpathDest(name, sourcePath);
    return dest === file ? name : strip(file.path);
  }

  getBacklinksForFile(file: TFile): any {
    const data = new Map<string, any[]>();
    for (const src in this.resolvedLinks) {
      if (!this.resolvedLinks[src][file.path]) continue;
      const cache = this.getCache(src);
      const refs: any[] = [];
      iterateCacheRefs(cache, (ref: any) => {
        if (this.getFirstLinkpathDest(ref.link, src) === file) refs.push(ref);
      });
      if (refs.length) data.set(src, refs);
    }
    return {
      data,
      count: () => data.size,
      keys: () => Array.from(data.keys()),
      get: (k: string) => data.get(k) ?? null,
    };
  }

  getTags(): Record<string, number> {
    const out: Record<string, number> = {};
    for (const path in this.fileCache) {
      const c = this.getCache(path);
      if (!c) continue;
      for (const t of c.tags || []) out[t.tag] = (out[t.tag] || 0) + 1;
      for (const t of parseFrontMatterTags(c.frontmatter) || []) out[t] = (out[t] || 0) + 1;
    }
    return out;
  }

  getLinkSuggestions(): { file: TFile | null; path: string; alias?: string }[] {
    const out: any[] = [];
    for (const f of this.vault.getFiles()) {
      out.push({ file: f, path: f.path });
      for (const a of parseFrontMatterAliases(this.getFileCache(f)?.frontmatter) || []) out.push({ file: f, path: f.path, alias: a });
    }
    for (const src in this.unresolvedLinks) for (const l in this.unresolvedLinks[src]) out.push({ file: null, path: l });
    return out;
  }

  getFrontmatterPropertyValuesForKey(key: string): any[] {
    const s = new Set<any>();
    for (const p in this.fileCache) {
      const v = this.getCache(p)?.frontmatter?.[key];
      if (v == null) continue;
      (Array.isArray(v) ? v : [v]).forEach((x) => typeof x !== "object" && s.add(x));
    }
    return Array.from(s);
  }

  getAllPropertyInfos(): Record<string, any> {
    const out: Record<string, any> = {};
    for (const p in this.fileCache) {
      const fm = this.getCache(p)?.frontmatter;
      if (!fm) continue;
      for (const k in fm) {
        const o = (out[k.toLowerCase()] ||= { name: k, type: guessType(fm[k]), count: 0 });
        o.count++;
      }
    }
    return out;
  }

  isUserIgnored(path: string): boolean {
    const filters: string[] = this.vault.getConfig("userIgnoreFilters") || [];
    return filters.some((f) => {
      if (f.startsWith("/") && f.endsWith("/") && f.length > 2) {
        try { return new RegExp(f.slice(1, -1)).test(path); } catch { return false; }
      }
      return path.startsWith(f);
    });
  }

  // ── internal maintenance ─────────────────────────────────────────────────
  /** @internal */ indexName(file: TFile, add: boolean) {
    const keys = [file.name.toLowerCase()];
    for (const k of keys) {
      const list = this.byName.get(k) || [];
      if (add) {
        if (!list.includes(file)) list.push(file);
      } else list.remove(file);
      if (list.length) this.byName.set(k, list);
      else this.byName.delete(k);
    }
  }

  private prefetched: Promise<ArrayBuffer> | null = null;

  /** Starts reading the notes' metadata (the cache, or parsing) while the rest of the app loads. */
  /** @internal */ prefetch(): Promise<ArrayBuffer> {
    return (this.prefetched ||= invoke<ArrayBuffer>("vault_metadata_all", { path: this.vault.adapter.basePath, configDir: this.vault.configDir }));
  }

  /** @internal */ async initialize(): Promise<void> {
    for (const f of this.vault.getFiles()) this.indexName(f, true);
    const t0 = performance.now();
    const raw = await (this.prefetched || this.prefetch());
    this.prefetched = null;
    const tr = performance.now();
    const all: Record<string, any> = JSON.parse(new TextDecoder().decode(raw));
    const t1 = performance.now();
    for (const path in all) {
      const { meta, mtime, size } = all[path];
      this.fileCache[path] = { mtime, size, hash: path };
      this.metadataCache[path] = meta;
    }
    this.initialized = true;
    const t2 = performance.now();
    this.resolveAll();
    console.info(`[ateliet] metadata: wait ${Math.round(tr - t0)} ms, parse ${Math.round(t1 - tr)} ms (${(raw.byteLength / 1e6).toFixed(1)} MB), store ${Math.round(t2 - t1)} ms, resolve links ${Math.round(performance.now() - t2)} ms`);
    this.registerVaultEvents();
  }

  private registerVaultEvents() {
    const v = this.vault;
    v.on("create", (f: any) => {
      if (!(f instanceof TFile)) return;
      this.indexName(f, true);
      if (f.extension === "md") this.reparse(f);
      this.resolveLinksTo([f.path]); // pending links may resolve to the new file
    });
    v.on("modify", (f: any) => f instanceof TFile && f.extension === "md" && this.reparse(f));
    v.on("delete", (f: any) => {
      if (!(f instanceof TFile)) return;
      this.indexName(f, false);
      const prev = this.getCache(f.path);
      delete this.fileCache[f.path];
      delete this.metadataCache[f.path];
      delete this.resolvedLinks[f.path];
      delete this.unresolvedLinks[f.path];
      this.forgetSource(f.path);
      this.trigger("deleted", f, prev);
      this.resolveLinksTo([f.path]);
    });
    v.on("rename", (f: any, oldPath: string) => {
      if (!(f instanceof TFile)) return;
      const oldName = oldPath.split("/").pop()!.toLowerCase();
      const list = this.byName.get(oldName);
      list?.remove(f);
      if (list && !list.length) this.byName.delete(oldName);
      this.indexName(f, true);
      if (this.fileCache[oldPath]) {
        this.fileCache[f.path] = { ...this.fileCache[oldPath], hash: f.path };
        this.metadataCache[f.path] = this.metadataCache[oldPath];
        delete this.fileCache[oldPath];
        delete this.metadataCache[oldPath];
      }
      delete this.resolvedLinks[oldPath];
      delete this.unresolvedLinks[oldPath];
      this.forgetSource(oldPath);
      if (f.extension === "md" && !this.fileCache[f.path]) this.reparse(f);
      else if (this.fileCache[f.path]) this.resolveFile(f.path); // its relative links moved with it
      this.resolveLinksTo([oldPath, f.path]);
    });
  }

  /** @internal */ reparse(file: TFile): Promise<void> {
    const prev = this.queue.get(file.path) || Promise.resolve();
    const next = prev.then(async () => {
      if (file.deleted) return;
      this.inProgressTaskCount++;
      try {
        const data = await this.vault.cachedRead(file);
        const meta = await invoke("md_parse", { text: data });
        this.fileCache[file.path] = { mtime: file.stat.mtime, size: file.stat.size, hash: file.path };
        this.metadataCache[file.path] = meta;
        this.trigger("changed", file, data, meta);
        this.resolveFile(file.path);
        this.trigger("resolve", file);
      } catch (e) {
        console.error("metadata", file.path, e);
      } finally {
        this.inProgressTaskCount--;
        if (this.inProgressTaskCount === 0) this.trigger("resolved");
      }
    });
    this.queue.set(file.path, next);
    return next;
  }

  private resolveFile(path: string) {
    const resolved: Record<string, number> = {};
    const unresolved: Record<string, number> = {};
    const keys = new Set<string>();
    iterateCacheRefs(this.getCache(path), (ref: any) => {
      const lp = getLinkpath(ref.link);
      if (!lp) return;
      keys.add(linkKey(lp));
      let dest: TFile | null;
      const memo = this.resolveMemo;
      if (memo) {
        // During a full pass the vault does not change: a link text resolves the same from every note of a folder.
        const key = (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "") + "\0" + lp;
        dest = memo.get(key) ?? null;
        if (!memo.has(key)) {
          dest = this.getFirstLinkpathDest(lp, path);
          memo.set(key, dest);
        }
      } else dest = this.getFirstLinkpathDest(lp, path);
      if (dest) resolved[dest.path] = (resolved[dest.path] || 0) + 1;
      else unresolved[lp] = (unresolved[lp] || 0) + 1;
    });
    this.resolvedLinks[path] = resolved;
    this.unresolvedLinks[path] = unresolved;
    this.forgetSource(path);
    this.sourceKeys.set(path, [...keys]);
    for (const k of keys) {
      const set = this.linkSources.get(k);
      if (set) set.add(path);
      else this.linkSources.set(k, new Set([path]));
    }
  }

  private forgetSource(path: string) {
    for (const k of this.sourceKeys.get(path) || []) {
      const set = this.linkSources.get(k);
      set?.delete(path);
      if (set && !set.size) this.linkSources.delete(k);
    }
    this.sourceKeys.delete(path);
  }

  /** Re-resolves the notes whose links could point at these (added, removed or renamed) paths. */
  private resolveLinksTo(paths: string[]) {
    const sources = new Set<string>();
    for (const p of paths) {
      const name = p.split("/").pop()!.toLowerCase();
      for (const k of name.endsWith(".md") ? [name, name.slice(0, -3)] : [name]) {
        for (const src of this.linkSources.get(k) || []) sources.add(src);
      }
    }
    for (const src of sources) if (this.fileCache[src]) this.resolveFile(src);
    this.resolved = true;
    this.trigger("resolved");
  }

  private resolveMemo: Map<string, TFile | null> | null = null;

  /** @internal */ resolveAll() {
    this.resolveMemo = new Map();
    try {
      for (const path in this.fileCache) this.resolveFile(path);
    } finally {
      this.resolveMemo = null;
    }
    this.resolved = true;
    this.trigger("resolved");
  }
}

/** Key a link target is indexed under: its last path segment, lower-cased, without `.md`. */
function linkKey(linkpath: string): string {
  let name = linkpath.split("/").pop()!.toLowerCase();
  try {
    name = decodeURIComponent(name);
  } catch { /* keep raw */ }
  return name.endsWith(".md") ? name.slice(0, -3) : name;
}

function guessType(v: any): string {
  if (Array.isArray(v)) return "multitext";
  if (typeof v === "number") return "number";
  if (typeof v === "boolean") return "checkbox";
  if (typeof v === "string" && /^\d{4}-\d{2}-\d{2}$/.test(v)) return "date";
  if (typeof v === "string" && /^\d{4}-\d{2}-\d{2}T/.test(v)) return "datetime";
  return "text";
}

export function resolveRelative(dir: string, rel: string): string {
  const parts = dir ? dir.split("/") : [];
  for (const seg of rel.split("/")) {
    if (seg === "..") parts.pop();
    else if (seg !== "." && seg !== "") parts.push(seg);
  }
  return parts.join("/");
}

export function relativePath(from: string, to: string): string {
  const a = from.split("/").slice(0, -1);
  const b = to.split("/");
  let i = 0;
  while (i < a.length && i < b.length - 1 && a[i] === b[i]) i++;
  return [...Array(a.length - i).fill(".."), ...b.slice(i)].join("/") || to;
}
