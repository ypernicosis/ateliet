// Ateliet front-end entry point.

import "./dom.ts";
import * as obsidian from "./obsidian/index.ts";
import * as cmAutocomplete from "@codemirror/autocomplete";
import * as cmCollab from "@codemirror/collab";
import * as cmCommands from "@codemirror/commands";
import * as cmLanguage from "@codemirror/language";
import * as cmLint from "@codemirror/lint";
import * as cmSearch from "@codemirror/search";
import * as cmState from "@codemirror/state";
import * as cmView from "@codemirror/view";
import * as lezerCommon from "@lezer/common";
import * as lezerHighlight from "@lezer/highlight";
import * as lezerLr from "@lezer/lr";
import { nodeRequire, setHome } from "./obsidian/nodeshim.ts";
import { App } from "./app.ts";
import { SettingsModal } from "./core/settings.ts";
import { DEFAULT_LAYOUT, registerCorePlugins } from "./core/index.ts";
import { showLauncher } from "./launcher.ts";
import { installDebugBridge } from "./debug.ts";
import { installCompat } from "./compat.ts";
import { currentWindow, invoke, isMac, isWin, listen } from "./tauri.ts";
import { moment } from "./obsidian/util.ts";
import { tokenClassNodeProp } from "./editor/syntax.ts";
import { MarkdownView } from "./obsidian/views.ts";
import { installUpdater } from "./core/update.ts";

// Same module map Obsidian exposes to plugins.
const MODULES: Record<string, any> = {
  obsidian,
  "@codemirror/autocomplete": cmAutocomplete,
  "@codemirror/collab": cmCollab,
  "@codemirror/commands": cmCommands,
  "@codemirror/language": { ...cmLanguage, tokenClassNodeProp },
  "@codemirror/lint": cmLint,
  "@codemirror/search": cmSearch,
  "@codemirror/state": cmState,
  "@codemirror/text": cmState,
  "@codemirror/view": cmView,
  "@lezer/common": lezerCommon,
  "@lezer/lr": lezerLr,
  "@lezer/highlight": lezerHighlight,
};
// Pre-1.0 CodeMirror 6 package names, mapped like Obsidian does.
const DEPRECATED: Record<string, any> = {
  "@codemirror/closebrackets": cmAutocomplete,
  "@codemirror/comment": cmCommands,
  "@codemirror/fold": cmLanguage,
  "@codemirror/gutter": cmView,
  "@codemirror/highlight": cmLanguage,
  "@codemirror/history": cmCommands,
  "@codemirror/matchbrackets": cmLanguage,
  "@codemirror/panel": cmView,
  "@codemirror/rangeset": cmState,
  "@codemirror/rectangular-selection": cmView,
  "@codemirror/stream-parser": cmLanguage,
  "@codemirror/tooltip": cmView,
};

function makeRequire(pluginId: string) {
  return (name: string) => {
    if (name in MODULES) return MODULES[name];
    if (name in DEPRECATED) {
      console.warn(`[${pluginId}] uses deprecated package "${name}"`);
      return DEPRECATED[name];
    }
    return nodeRequire(name);
  };
}

const W = window as any;
W.require = makeRequire("global");
W.moment = moment;
W.obsidian = obsidian;
W.Keymap = obsidian.Keymap;
W.i18next = { t: (k: string) => k.split(".").pop(), language: "en", on() {}, off() {} };

async function boot() {
  await installDebugBridge();
  const params = new URLSearchParams(location.search);
  const vaultPath = params.get("vault");
  const info = await invoke<any>("app_info");
  setHome(info.home);
  document.body.addClass(isMac ? "mod-macos" : isWin ? "mod-windows" : "mod-linux");
  if (isMac) {
    document.body.addClass("is-frameless", "is-hidden-frameless");
    // The traffic lights disappear in full screen; the header padding for them must too.
    const syncFullscreen = () => currentWindow().isFullscreen().then((fs: boolean) => document.body.toggleClass("is-fullscreen", fs)).catch(() => {});
    window.addEventListener("resize", syncFullscreen);
    syncFullscreen();
  }
  document.body.addClass("theme-dark", "is-focused");
  if (!vaultPath) return showLauncher();

  document.body.addClass("is-loading");
  // How long each step of opening the vault takes (in the console, and in
  // ATELIET_DEBUG's console.log), to find what makes a launch slow.
  const t0 = performance.now();
  let tPrev = t0;
  const step = (name: string) => {
    const now = performance.now();
    console.info(`[ateliet] boot ${name}: ${Math.round(now - tPrev)} ms`);
    tPrev = now;
  };
  let vinfo: any;
  try {
    vinfo = await invoke("vault_init", { path: vaultPath });
  } catch (e) {
    document.body.removeClass("is-loading");
    return showLauncher(String(e));
  }

  step(`vault listing (${vinfo.files.length} entries)`);
  const app = new App(vinfo.path, makeRequire) as any;
  app.version = info.version;
  W.app = app;
  app.vault.load(vinfo.files);
  app.metadataCache.prefetch();
  installCompat(app);
  await app.loadConfig();
  app.setting = new SettingsModal(app);
  registerCorePlugins(app);
  await app.internalPlugins.loadConfig();
  await app.customCss.reload();
  document.body.toggleClass("show-inline-title", !!app.vault.getConfig("showInlineTitle"));
  document.body.toggleClass("show-view-header", app.vault.getConfig("showViewHeader") !== false);
  step("config and core plugins registered");
  await app.metadataCache.initialize();
  step("note metadata");
  await app.metadataTypeManager.load();
  await app.internalPlugins.enableAll();
  step("core plugins enabled");
  buildRibbon(app);

  // External changes (Obsidian running on the same vault, sync, git …).
  await listen("vault-fs", (paths: string[]) => onFsEvents(app, paths));

  await app.hotkeyManager.load();
  await app.plugins.initialize();
  step("community plugins");

  const layout = (await app.loadWorkspaceLayout()) || DEFAULT_LAYOUT;
  try {
    await app.workspace.loadLayout(layout);
  } catch (e) {
    console.error("layout", e);
    await app.workspace.loadLayout(DEFAULT_LAYOUT);
  }
  ensureCoreLeaves(app);
  app.hotkeyManager.bake();
  step("layout");
  console.info(`[ateliet] boot total: ${Math.round(performance.now() - t0)} ms`);
  document.body.removeClass("is-loading");
  app.workspace.setLayoutReady();
  app.workspace.updateTitle();
  installCloseHandler(app);
  installUpdater(app, () => flush(app));
}

function buildRibbon(app: any) {
  const r = app.workspace.leftRibbon;
  const toggle = r.containerEl.createDiv({ cls: "sidebar-toggle-button mod-left clickable-icon", attr: { "aria-label": "Toggle left sidebar", "data-tooltip-position": "right" } });
  obsidian.setIcon(toggle, "panel-left");
  toggle.addEventListener("click", () => app.workspace.leftSplit.toggle());
  r.containerEl.insertBefore(toggle, r.containerEl.firstChild);
  r.addRibbonItemButton("switcher:open", "file-search", "Open quick switcher", () => app.commands.executeCommandById("switcher:open"));
  r.addRibbonItemButton("command-palette:open", "terminal-square", "Open command palette", () => app.commands.executeCommandById("command-palette:open"));
  r.addSettingsButton("vault", "Open another vault", () => app.openVaultChooser());
  r.addSettingsButton("settings", "Settings", () => app.setting.open());
  const right = createDiv({ cls: "sidebar-toggle-button mod-right clickable-icon", attr: { "aria-label": "Toggle right sidebar" } });
  obsidian.setIcon(right, "panel-right");
  right.addEventListener("click", () => app.workspace.rightSplit.toggle());
  app.workspace.containerEl.appendChild(right);
}

/** Obsidian's workspace.json may reference views Ateliet lacks; make sure the essentials exist. */
function ensureCoreLeaves(app: any) {
  const ws = app.workspace;
  const need: [string, "left" | "right"][] = [["file-explorer", "left"], ["search", "left"], ["backlink", "right"], ["outline", "right"]];
  // Image mode's right side holds only notes.
  const images = document.body.hasClass("mode-images");
  for (const [type, side] of need) {
    if (images && side === "right") continue;
    if (!ws.viewRegistry.viewByType[type] || ws.getLeavesOfType(type).length) continue;
    const leaf = side === "left" ? ws.getLeftLeaf(false) : ws.getRightLeaf(false);
    leaf.setViewState({ type, state: {} });
  }
  for (const dock of [ws.leftSplit, ws.rightSplit]) {
    for (const tabs of dock.children) if (tabs.children && !tabs.children[tabs.currentTab]) tabs.selectTabIndex?.(0);
  }
}

async function onFsEvents(app: any, paths: string[]) {
  const configDir = app.vault.configDir;
  const vaultPaths: string[] = [];
  let cssChanged = false;
  for (const p of paths) {
    if (p === configDir || p.startsWith(configDir + "/")) {
      if (p.startsWith(`${configDir}/ateliet/`)) continue;
      if (p.endsWith("/data.json")) app.plugins.onConfigFileChange(p);
      if (p.endsWith(".css")) cssChanged = true;
      continue;
    }
    vaultPaths.push(p);
  }
  if (cssChanged) app.customCss.reload();
  if (vaultPaths.length) await app.vault.reconcile(vaultPaths);
}

/** Saves open notes and the layout. */
async function flush(app: any) {
  const saves: Promise<any>[] = [];
  app.workspace.iterateAllLeaves((l: any) => {
    if (l.view instanceof MarkdownView) saves.push(l.view.save());
  });
  await Promise.all(saves);
  await app.workspace.saveLayout();
}

function installCloseHandler(app: any) {
  window.addEventListener("blur", () => flush(app));
  try {
    currentWindow().onCloseRequested(async () => {
      app.workspace.trigger("quit", { add() {}, });
      await flush(app);
    });
  } catch (e) {
    console.warn(e);
  }
}

boot().catch((e) => {
  console.error(e);
  document.body.removeClass("is-loading");
  document.body.createDiv({ cls: "boot-error", text: "Ateliet failed to start: " + (e?.stack || e) });
});
