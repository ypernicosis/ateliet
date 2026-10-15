// Debug bridge (only active when the app runs with ATELIET_DEBUG=<dir>):
// mirrors the console to <dir>/console.log and evaluates <dir>/eval.js.

import { invoke } from "./tauri.ts";

const fmt = (a: any) => {
  if (a instanceof Error) return `${a.name}: ${a.message}\n${a.stack || ""}`;
  if (a && typeof a === "object" && "message" in a && "stack" in a) return `${a.message}\n${a.stack}`;
  if (typeof a === "string") return a;
  try {
    return JSON.stringify(a);
  } catch {
    return String(a);
  }
};

export async function installDebugBridge() {
  if (!(await invoke<boolean>("debug_enabled"))) return;
  for (const level of ["log", "info", "warn", "error"] as const) {
    const orig = console[level].bind(console);
    console[level] = (...args: any[]) => {
      orig(...args);
      invoke("debug_log", { level, msg: args.map(fmt).join(" ") }).catch(() => {});
    };
  }
  window.addEventListener("error", (e) => console.error("uncaught", e.error || e.message));
  window.addEventListener("unhandledrejection", (e) => console.error("unhandled rejection", e.reason));
  setInterval(async () => {
    const code = await invoke<string | null>("debug_take_eval");
    if (!code) return;
    try {
      const r = await (0, eval)(`(async () => { ${code} })()`);
      invoke("debug_log", { level: "eval", msg: fmt(r) });
    } catch (e) {
      invoke("debug_log", { level: "eval-error", msg: fmt(e) });
    }
  }, 400);
}
