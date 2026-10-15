// Thin wrapper over Tauri's IPC. Everything that touches the disk goes
// through the Rust core.

const T = (window as any).__TAURI__;

export function invoke<R = any>(cmd: string, args?: any, opts?: any): Promise<R> {
  return T.core.invoke(cmd, args, opts);
}

export function listen(event: string, cb: (payload: any) => void): Promise<() => void> {
  return T.event.listen(event, (e: any) => cb(e.payload));
}

/** URL served by the `ateliet://` protocol for an absolute path. */
export function fileUrl(absPath: string): string {
  const p = absPath.split("/").map(encodeURIComponent).join("/");
  return T.core.convertFileSrc ? T.core.convertFileSrc(absPath, "ateliet") : `ateliet://localhost${p}`;
}

/** Base URL of the `ateliet://` protocol (platform dependent). */
export function protocolBase(): string {
  // convertFileSrc percent-encodes the whole path ("…/%2Fx"), so the suffix
  // is not always two characters long.
  return fileUrl("/x").replace(/\/(%2F)?x$/i, "");
}

export const isMac = navigator.userAgent.includes("Mac");
export const isWin = navigator.userAgent.includes("Windows");

export function currentWindow(): any {
  return T.window.getCurrentWindow();
}

export async function readBytes(path: string): Promise<ArrayBuffer> {
  const r = await invoke<any>("fs_read_bytes", { path });
  if (r instanceof ArrayBuffer) return r;
  if (r instanceof Uint8Array) return r.buffer.slice(r.byteOffset, r.byteOffset + r.byteLength) as ArrayBuffer;
  return new Uint8Array(r).buffer;
}

export function writeBytes(path: string, data: ArrayBuffer | Uint8Array, atomic = false): Promise<any> {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  const headers: Record<string, string> = { "x-path": encodeURIComponent(path) };
  if (atomic) headers["x-atomic"] = "1";
  return invoke("fs_write_bytes", bytes, { headers });
}

/** Synchronous fs access (for the Node `fs` shim) via sync XHR to the protocol. */
export function syncFs(op: string, path: string, extra: Record<string, string> = {}, body?: any, binary = false): any {
  const q = new URLSearchParams({ path, ...extra }).toString();
  const xhr = new XMLHttpRequest();
  xhr.open(body !== undefined ? "POST" : "GET", `${protocolBase()}/__fs/${op}?${q}`, false);
  if (binary) xhr.overrideMimeType("text/plain; charset=x-user-defined");
  xhr.send(body);
  if (xhr.status !== 200) {
    const err: any = new Error(`${xhr.responseText || "ENOENT"}: ${path}`);
    err.code = "ENOENT";
    throw err;
  }
  if (binary) {
    const s = xhr.responseText;
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 0xff;
    return out;
  }
  const ct = xhr.getResponseHeader("content-type") || "";
  return ct.includes("json") ? JSON.parse(xhr.responseText) : xhr.responseText;
}
