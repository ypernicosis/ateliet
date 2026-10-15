// Minimal Node/Electron surface for desktop-only plugins. Obsidian runs in
// Electron with node integration; Ateliet runs in the system webview, so the
// commonly used parts are re-implemented on top of the Rust core.

import { Buffer } from "buffer";
import { invoke, listen, readBytes, syncFs, writeBytes, isMac, isWin } from "../tauri.ts";

const W = window as any;
W.Buffer ||= Buffer;
W.global ||= window;
W.process ||= {
  argv: [],
  argv0: "ateliet",
  execPath: "/",
  execArgv: [],
  pid: 1,
  ppid: 0,
  title: "ateliet",
  exitCode: 0,
  exit: () => {},
  emitWarning: (w: any) => console.warn(w),
  memoryUsage: () => ({ rss: 0, heapTotal: 0, heapUsed: 0, external: 0 }),
  umask: () => 0o22,
  getuid: () => 501,
  addListener: () => W.process,
  removeListener: () => W.process,
  removeAllListeners: () => W.process,
  once: () => W.process,
  off: () => W.process,
  emit: () => false,
  listeners: () => [],
  stdout: { write: (s: string) => (console.log(s), true), isTTY: false },
  stderr: { write: (s: string) => (console.error(s), true), isTTY: false },
  platform: isMac ? "darwin" : isWin ? "win32" : "linux",
  env: {},
  versions: { node: "20.0.0", electron: "30.0.0", chrome: "124.0.0" },
  arch: "arm64",
  cwd: () => "/",
  nextTick: (fn: (...a: any[]) => void, ...args: any[]) => queueMicrotask(() => fn(...args)),
  on: () => W.process,
  hrtime: Object.assign(() => [0, 0], { bigint: () => BigInt(Math.round(performance.now() * 1e6)) }),
};

let home = "/";
export function setHome(h: string) {
  home = h || "/";
}

// ── path ──────────────────────────────────────────────────────────────────

function normalizeArray(parts: string[], allowAbove: boolean) {
  const res: string[] = [];
  for (const p of parts) {
    if (!p || p === ".") continue;
    if (p === "..") {
      if (res.length && res[res.length - 1] !== "..") res.pop();
      else if (allowAbove) res.push("..");
    } else res.push(p);
  }
  return res;
}

export const path: any = {
  sep: "/",
  delimiter: ":",
  normalize(p: string) {
    const abs = p.startsWith("/");
    const trailing = p.endsWith("/");
    let out = normalizeArray(p.split("/"), !abs).join("/");
    if (!out && !abs) out = ".";
    if (out && trailing) out += "/";
    return (abs ? "/" : "") + out;
  },
  join(...parts: string[]) {
    return path.normalize(parts.filter((x) => typeof x === "string" && x).join("/"));
  },
  resolve(...parts: string[]) {
    let r = "";
    for (let i = parts.length - 1; i >= 0 && !r.startsWith("/"); i--) if (parts[i]) r = parts[i] + (r ? "/" + r : "");
    if (!r.startsWith("/")) r = "/" + r;
    return path.normalize(r).replace(/\/$/, "") || "/";
  },
  isAbsolute: (p: string) => p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p),
  dirname(p: string) {
    const i = p.replace(/\/+$/, "").lastIndexOf("/");
    if (i === -1) return ".";
    return i === 0 ? "/" : p.slice(0, i);
  },
  basename(p: string, ext?: string) {
    let b = p.replace(/\/+$/, "").split("/").pop() || "";
    if (ext && b.endsWith(ext)) b = b.slice(0, -ext.length);
    return b;
  },
  extname(p: string) {
    const b = path.basename(p);
    const i = b.lastIndexOf(".");
    return i <= 0 ? "" : b.slice(i);
  },
  relative(from: string, to: string) {
    const a = path.resolve(from).split("/").filter(Boolean);
    const b = path.resolve(to).split("/").filter(Boolean);
    let i = 0;
    while (i < a.length && i < b.length && a[i] === b[i]) i++;
    return [...Array(a.length - i).fill(".."), ...b.slice(i)].join("/");
  },
  parse(p: string) {
    const base = path.basename(p);
    const ext = path.extname(p);
    return { root: p.startsWith("/") ? "/" : "", dir: path.dirname(p), base, ext, name: ext ? base.slice(0, -ext.length) : base };
  },
  format(o: any) {
    return (o.dir ? o.dir + "/" : o.root || "") + (o.base || (o.name || "") + (o.ext || ""));
  },
  toNamespacedPath: (p: string) => p,
};
path.posix = path;
path.win32 = path;

// ── fs ────────────────────────────────────────────────────────────────────

function statObj(s: any) {
  return {
    isFile: () => s.type === "file",
    isDirectory: () => s.type === "folder",
    isSymbolicLink: () => false,
    size: s.size,
    mtimeMs: s.mtime,
    ctimeMs: s.ctime,
    birthtimeMs: s.ctime,
    atimeMs: s.mtime,
    mtime: new Date(s.mtime),
    ctime: new Date(s.ctime),
    birthtime: new Date(s.ctime),
    atime: new Date(s.mtime),
    mode: s.type === "folder" ? 0o40755 : 0o100644,
  };
}

const enoent = (p: string) => Object.assign(new Error(`ENOENT: no such file or directory, '${p}'`), { code: "ENOENT", path: p });
const encOf = (o: any) => (typeof o === "string" ? o : o?.encoding);
const toBytes = (data: any): Uint8Array => {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return new TextEncoder().encode(String(data));
};
const decode = (bytes: Uint8Array, enc?: string) => (enc ? (enc === "base64" ? Buffer.from(bytes).toString("base64") : new TextDecoder().decode(bytes)) : Buffer.from(bytes));

const promises = {
  async readFile(p: string, o?: any) {
    const enc = encOf(o);
    if (enc === "utf8" || enc === "utf-8") return invoke<string>("fs_read_text", { path: String(p) });
    return decode(new Uint8Array(await readBytes(String(p))), enc);
  },
  async writeFile(p: string, data: any) {
    if (typeof data === "string") await invoke("fs_write_text", { path: String(p), data });
    else await writeBytes(String(p), toBytes(data));
  },
  async appendFile(p: string, data: any) {
    await invoke("fs_append", { path: String(p), data: typeof data === "string" ? data : new TextDecoder().decode(toBytes(data)) });
  },
  async mkdir(p: string) {
    await invoke("fs_mkdir", { path: String(p) });
  },
  async readdir(p: string, o?: any) {
    const r = await invoke<any>("fs_list", { path: String(p) });
    const names = [...r.folders, ...r.files].sort();
    if (o?.withFileTypes) {
      const dirs = new Set(r.folders);
      return names.map((name: string) => ({ name, isDirectory: () => dirs.has(name), isFile: () => !dirs.has(name), isSymbolicLink: () => false }));
    }
    return names;
  },
  async stat(p: string) {
    const s = await invoke<any>("fs_stat", { path: String(p) });
    if (!s) throw enoent(p);
    return statObj(s);
  },
  async lstat(p: string) {
    return promises.stat(p);
  },
  async access(p: string) {
    if (!(await invoke("fs_exists", { path: String(p) }))) throw enoent(p);
  },
  async unlink(p: string) {
    await invoke("fs_remove", { path: String(p) });
  },
  async rm(p: string, o?: any) {
    const s = await invoke<any>("fs_stat", { path: String(p) });
    if (!s) {
      if (o?.force) return;
      throw enoent(p);
    }
    if (s.type === "folder") await invoke("fs_rmdir", { path: String(p), recursive: !!o?.recursive });
    else await invoke("fs_remove", { path: String(p) });
  },
  async rmdir(p: string, o?: any) {
    await invoke("fs_rmdir", { path: String(p), recursive: !!o?.recursive });
  },
  async rename(a: string, b: string) {
    await invoke("fs_rename", { from: String(a), to: String(b) });
  },
  async copyFile(a: string, b: string) {
    await invoke("fs_copy", { from: String(a), to: String(b) });
  },
  async cp(a: string, b: string) {
    await invoke("fs_copy", { from: String(a), to: String(b) });
  },
  async realpath(p: string) {
    return p;
  },
};

const callbackify = (fn: (...a: any[]) => Promise<any>) => (...args: any[]) => {
  const cb = typeof args[args.length - 1] === "function" ? args.pop() : null;
  fn(...args).then((r) => cb?.(null, r), (e) => cb?.(e));
};

export const fs: any = {
  promises,
  constants: { F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1 },
  existsSync(p: string) {
    try {
      syncFs("stat", String(p));
      return true;
    } catch {
      return false;
    }
  },
  statSync(p: string, o?: any) {
    try {
      return statObj(syncFs("stat", String(p)));
    } catch (e) {
      if (o?.throwIfNoEntry === false) return undefined;
      throw enoent(p);
    }
  },
  lstatSync(p: string, o?: any) {
    return fs.statSync(p, o);
  },
  readFileSync(p: string, o?: any) {
    const bytes: Uint8Array = syncFs("read", String(p), {}, undefined, true);
    return decode(bytes, encOf(o) === "utf-8" ? "utf8" : encOf(o));
  },
  writeFileSync(p: string, data: any) {
    syncFs("write", String(p), {}, toBytes(data));
  },
  appendFileSync(p: string, data: any) {
    const prev = fs.existsSync(p) ? (fs.readFileSync(p) as Uint8Array) : new Uint8Array();
    const add = toBytes(data);
    const out = new Uint8Array(prev.length + add.length);
    out.set(prev);
    out.set(add, prev.length);
    fs.writeFileSync(p, out);
  },
  mkdirSync(p: string) {
    syncFs("mkdir", String(p));
  },
  readdirSync(p: string, o?: any) {
    const r = syncFs("list", String(p));
    const names = [...r.folders, ...r.files].sort();
    if (o?.withFileTypes) {
      const dirs = new Set(r.folders);
      return names.map((name: string) => ({ name, isDirectory: () => dirs.has(name), isFile: () => !dirs.has(name), isSymbolicLink: () => false }));
    }
    return names;
  },
  unlinkSync(p: string) {
    syncFs("remove", String(p));
  },
  rmSync(p: string, o?: any) {
    try {
      syncFs("remove", String(p));
    } catch (e) {
      if (!o?.force) throw e;
    }
  },
  rmdirSync(p: string) {
    syncFs("remove", String(p));
  },
  renameSync(a: string, b: string) {
    syncFs("rename", String(a), { to: String(b) });
  },
  copyFileSync(a: string, b: string) {
    fs.writeFileSync(b, fs.readFileSync(a));
  },
  accessSync(p: string) {
    if (!fs.existsSync(p)) throw enoent(p);
  },
  realpathSync: Object.assign((p: string) => p, { native: (p: string) => p }),
  watch() {
    return { close() {}, on() {} };
  },
  createReadStream() {
    throw new Error("fs.createReadStream is not supported in Ateliet");
  },
  createWriteStream() {
    throw new Error("fs.createWriteStream is not supported in Ateliet");
  },
};
for (const k of Object.keys(promises)) fs[k] = callbackify((promises as any)[k]);
fs.exists = (p: string, cb: (b: boolean) => void) => invoke<boolean>("fs_exists", { path: String(p) }).then(cb);

// ── os / electron / misc ──────────────────────────────────────────────────

export const os = {
  platform: () => W.process.platform,
  type: () => (isMac ? "Darwin" : isWin ? "Windows_NT" : "Linux"),
  homedir: () => home,
  tmpdir: () => (isWin ? "C:/Windows/Temp" : "/tmp"),
  hostname: () => "localhost",
  arch: () => W.process.arch,
  release: () => "",
  cpus: () => [],
  totalmem: () => 0,
  freemem: () => 0,
  userInfo: () => ({ username: home.split("/").pop() || "user", homedir: home }),
  EOL: isWin ? "\r\n" : "\n",
  constants: { errno: {}, signals: { SIGINT: 2, SIGTERM: 15, SIGKILL: 9 }, priority: {} },
  loadavg: () => [0, 0, 0],
  uptime: () => performance.now() / 1000,
  networkInterfaces: () => ({}),
  endianness: () => "LE",
};

class EventEmitter {
  private l: Record<string, ((...a: any[]) => void)[]> = {};
  on(e: string, f: (...a: any[]) => void) {
    (this.l[e] ||= []).push(f);
    return this;
  }
  addListener(e: string, f: any) {
    return this.on(e, f);
  }
  once(e: string, f: (...a: any[]) => void) {
    const w = (...a: any[]) => {
      this.off(e, w);
      f(...a);
    };
    return this.on(e, w);
  }
  off(e: string, f: any) {
    this.l[e] = (this.l[e] || []).filter((x) => x !== f);
    return this;
  }
  removeListener(e: string, f: any) {
    return this.off(e, f);
  }
  removeAllListeners(e?: string) {
    if (e) delete this.l[e];
    else this.l = {};
    return this;
  }
  emit(e: string, ...a: any[]) {
    (this.l[e] || []).slice().forEach((f) => f(...a));
    return !!this.l[e]?.length;
  }
  setMaxListeners() {
    return this;
  }
}

const currentWindowStub = {
  isMaximized: () => false,
  isFullScreen: () => false,
  setAlwaysOnTop() {},
  isAlwaysOnTop: () => false,
  on() {},
  off() {},
  removeListener() {},
  webContents: { on() {}, send() {}, openDevTools() {}, getZoomFactor: () => 1, setZoomFactor() {} },
  getBounds: () => ({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }),
  focus: () => window.focus(),
};

export const electron = {
  shell: {
    openExternal: (url: string) => invoke("open_external", { url }),
    openPath: async (p: string) => {
      await invoke("open_external", { url: p });
      return "";
    },
    showItemInFolder: (p: string) => invoke("show_in_folder", { path: p }),
    trashItem: (p: string) => invoke("fs_trash", { path: p }),
  },
  clipboard: {
    readText: () => "",
    writeText: (t: string) => navigator.clipboard.writeText(t),
    readHTML: () => "",
    writeHTML: (h: string) => navigator.clipboard.writeText(h),
    readImage: () => ({ isEmpty: () => true }),
  },
  ipcRenderer: { send() {}, sendSync() {}, on() {}, once() {}, removeListener() {}, invoke: async () => null },
  webFrame: { setZoomFactor() {}, getZoomFactor: () => 1 },
  remote: {
    app: { getPath: (n: string) => (n === "home" ? home : n === "temp" ? os.tmpdir() : home), getVersion: () => "1.12.7", getName: () => "Ateliet" },
    getCurrentWindow: () => currentWindowStub,
    getCurrentWebContents: () => currentWindowStub.webContents,
    dialog: {
      showOpenDialog: async () => {
        const p = await invoke<string | null>("pick_folder");
        return { canceled: !p, filePaths: p ? [p] : [] };
      },
      showSaveDialog: async () => ({ canceled: true }),
      showMessageBox: async () => ({ response: 0 }),
    },
    BrowserWindow: class {},
    Menu: class {},
    MenuItem: class {},
    nativeTheme: { shouldUseDarkColors: matchMedia("(prefers-color-scheme: dark)").matches },
    session: { defaultSession: {} },
  },
};

const url = {
  pathToFileURL: (p: string) => new URL("file://" + encodeURI(p)),
  fileURLToPath: (u: string | URL) => decodeURI(new URL(String(u)).pathname),
  URL,
  URLSearchParams,
};

const util = {
  promisify: (fn: any) => fn[Symbol.for("nodejs.util.promisify.custom")] || ((...args: any[]) => new Promise((res, rej) => fn(...args, (e: any, r: any) => (e ? rej(e) : res(r))))),
  inspect: (o: any) => JSON.stringify(o),
  format: (...a: any[]) => a.join(" "),
  TextDecoder,
  TextEncoder,
  types: { isDate: (d: any) => d instanceof Date },
};

const crypto = {
  randomBytes: (n: number) => Buffer.from(globalThis.crypto.getRandomValues(new Uint8Array(n))),
  randomUUID: () => globalThis.crypto.randomUUID(),
  webcrypto: globalThis.crypto,
  createHash: () => {
    throw new Error("crypto.createHash is not available in Ateliet");
  },
};

// ── stream (minimal) ──────────────────────────────────────────────────────

class Stream extends EventEmitter {
  readable = true;
  writable = true;
  encoding: string | null = null;
  setEncoding(e: string) {
    this.encoding = e;
    return this;
  }
  pipe(dest: any) {
    this.on("data", (d: any) => dest.write?.(d));
    this.on("end", () => dest.end?.());
    return dest;
  }
  write(chunk: any) {
    this.emit("data", chunk);
    return true;
  }
  end(chunk?: any) {
    if (chunk !== undefined) this.write(chunk);
    this.emit("end");
    this.emit("finish");
    this.emit("close");
  }
  destroy() {
    this.emit("close");
  }
  pause() {
    return this;
  }
  resume() {
    return this;
  }
}
class Readable extends Stream {
  static from(it: Iterable<any>) {
    const r = new Readable();
    queueMicrotask(() => {
      for (const x of it) r.emit("data", x);
      r.emit("end");
    });
    return r;
  }
}
class Writable extends Stream {}
class Duplex extends Stream {}
class Transform extends Stream {}
class PassThrough extends Stream {}
const stream = Object.assign(Stream, { Stream, Readable, Writable, Duplex, Transform, PassThrough, default: Stream });

// ── child_process (backed by the Rust core) ───────────────────────────────

const children = new Map<number, ChildProcess>();
let procListener: Promise<any> | null = null;
function ensureProcListener() {
  procListener ||= listen("proc", (e: any) => {
    const c = children.get(e.id);
    if (!c) return;
    if (e.kind === "exit") {
      c.exitCode = e.code;
      children.delete(e.id);
      c.stdout.emit("end");
      c.stderr.emit("end");
      c.emit("exit", e.code, null);
      c.emit("close", e.code, null);
    } else {
      const s = e.kind === "stdout" ? c.stdout : c.stderr;
      s.emit("data", s.encoding ? e.data : Buffer.from(e.data));
    }
  });
  return procListener;
}

class ChildProcess extends EventEmitter {
  pid = 0;
  id = 0;
  exitCode: number | null = null;
  killed = false;
  stdout = new Readable();
  stderr = new Readable();
  stdin: any;
  constructor() {
    super();
    const self = this;
    this.stdin = {
      write(d: any) {
        if (self.id) invoke("proc_write", { id: self.id, data: typeof d === "string" ? d : new TextDecoder().decode(toBytes(d)) }).catch(() => {});
        return true;
      },
      end(d?: any) {
        if (d !== undefined) this.write(d);
        if (self.id) invoke("proc_close_stdin", { id: self.id });
      },
      on() {},
      once() {},
    };
  }
  kill() {
    this.killed = true;
    if (this.id) invoke("proc_kill", { id: this.id });
    return true;
  }
}

function spawnProc(cmd: string, args: string[] = [], opts: any = {}): ChildProcess {
  const c = new ChildProcess();
  ensureProcListener().then(() =>
    invoke<[number, number]>("proc_spawn", { cmd, args: args.map(String), cwd: opts.cwd ?? null, env: opts.env ?? null, shell: !!opts.shell })
      .then(([id, pid]) => {
        c.id = id;
        c.pid = pid;
        children.set(id, c);
        c.emit("spawn");
      })
      .catch((e) => c.emit("error", Object.assign(new Error(String(e)), { code: "ENOENT" }))),
  );
  return c;
}

function execProc(cmd: string, opts?: any, cb?: any): ChildProcess {
  if (typeof opts === "function") [cb, opts] = [opts, {}];
  const c = spawnProc(cmd, [], { ...opts, shell: true });
  let out = "", err = "";
  c.stdout.setEncoding("utf8");
  c.stderr.setEncoding("utf8");
  c.stdout.on("data", (d: string) => (out += d));
  c.stderr.on("data", (d: string) => (err += d));
  c.on("error", (e: any) => cb?.(e, out, err));
  c.on("close", (code: number) => {
    const e = code ? Object.assign(new Error(`Command failed: ${cmd}\n${err}`), { code, stdout: out, stderr: err }) : null;
    cb?.(e, out, err);
  });
  return c;
}

const child_process: any = {
  spawn: (cmd: string, args?: any, opts?: any) => (Array.isArray(args) ? spawnProc(cmd, args, opts) : spawnProc(cmd, [], args)),
  exec: execProc,
  execFile: (file: string, args?: any, opts?: any, cb?: any) => {
    if (typeof args === "function") [cb, args, opts] = [args, [], {}];
    if (typeof opts === "function") [cb, opts] = [opts, {}];
    const quoted = [file, ...(args || [])].map((a: string) => (/[^\w\/.:=-]/.test(a) ? `'${String(a).replace(/'/g, "'\\''")}'` : a)).join(" ");
    return execProc(quoted, opts, cb);
  },
  execSync: (cmd: string, opts: any = {}) => {
    const r = syncFs("exec", "", { cmd, cwd: opts.cwd || "" }, opts.input ?? "");
    if (r.status) {
      const e: any = new Error(`Command failed: ${cmd}\n${r.stderr || r.error || ""}`);
      Object.assign(e, { status: r.status, stdout: r.stdout, stderr: r.stderr });
      throw e;
    }
    return opts.encoding && opts.encoding !== "buffer" ? r.stdout : Buffer.from(r.stdout || "");
  },
  spawnSync: (cmd: string, args: string[] = [], opts: any = {}) => {
    const r = syncFs("exec", "", { cmd: [cmd, ...args].join(" "), cwd: opts.cwd || "" }, opts.input ?? "");
    return { status: r.status, stdout: opts.encoding ? r.stdout : Buffer.from(r.stdout || ""), stderr: opts.encoding ? r.stderr : Buffer.from(r.stderr || ""), error: r.error ? new Error(r.error) : undefined };
  },
  fork: () => {
    throw new Error("child_process.fork is not supported in Ateliet");
  },
  ChildProcess,
};
child_process.exec[Symbol.for("nodejs.util.promisify.custom")] = (cmd: string, opts?: any) =>
  new Promise((res, rej) => execProc(cmd, opts, (e: any, stdout: string, stderr: string) => (e ? rej(e) : res({ stdout, stderr }))));

const NODE_MODULES: Record<string, any> = {
  fs,
  "fs/promises": promises,
  "node:fs": fs,
  "node:fs/promises": promises,
  path,
  "node:path": path,
  "path/posix": path,
  os,
  "node:os": os,
  electron,
  "@electron/remote": electron.remote,
  events: Object.assign(EventEmitter, { EventEmitter, default: EventEmitter }),
  "node:events": Object.assign(EventEmitter, { EventEmitter }),
  url,
  "node:url": url,
  util,
  "node:util": util,
  buffer: { Buffer },
  "node:buffer": { Buffer },
  crypto,
  "node:crypto": crypto,
  process: W.process,
  "node:process": W.process,
  child_process,
  "node:child_process": child_process,
  stream,
  "node:stream": stream,
  "stream/promises": { pipeline: async () => {}, finished: async () => {} },
  assert: Object.assign((v: any, m?: string) => { if (!v) throw new Error(m || "Assertion failed"); }, { ok: (v: any, m?: string) => { if (!v) throw new Error(m || "Assertion failed"); }, equal: () => {}, strictEqual: () => {}, deepEqual: () => {} }),
  string_decoder: { StringDecoder: class { constructor(public enc = "utf8") {} write(b: any) { return new TextDecoder().decode(b); } end() { return ""; } } },
  timers: { setTimeout, clearTimeout, setInterval, clearInterval, setImmediate: (f: any) => setTimeout(f, 0) },
  querystring: { parse: (s: string) => Object.fromEntries(new URLSearchParams(s)), stringify: (o: any) => new URLSearchParams(o).toString() },
};

W.electron ||= electron;
W.__dirname ||= "/";
W.__filename ||= "/index.html";
W.setImmediate ||= (f: (...a: any[]) => void, ...a: any[]) => setTimeout(() => f(...a), 0);
W.clearImmediate ||= (id: any) => clearTimeout(id);

export function nodeRequire(name: string): any {
  if (name in NODE_MODULES) return NODE_MODULES[name];
  const err: any = new Error(`Cannot find module '${name}' (not available in Ateliet)`);
  err.code = "MODULE_NOT_FOUND";
  throw err;
}
