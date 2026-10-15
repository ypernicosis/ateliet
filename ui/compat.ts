// Internal Obsidian APIs that popular plugins rely on, even though they are
// not part of obsidian.d.ts.

import { Component, Events } from "./obsidian/events.ts";
import { renderMarkdownInto } from "./obsidian/renderer.ts";
import { resolveSubpath } from "./obsidian/util.ts";
import { TFile } from "./obsidian/vault.ts";
import { EmbedEditView } from "./obsidian/views.ts";
import { javascript as legacyJavascript } from "@codemirror/legacy-modes/mode/javascript";

// ── CodeMirror 5 global (Obsidian still ships it for legacy modes) ────────
// Plugins like Templater build CM6 StreamLanguages from CM5 modes, so modes
// must be real: defineMode stores factories, getMode instantiates them.
const W = window as any;
const nullMode = () => ({ name: "null", startState: () => ({}), token: (s: any) => void s.skipToEnd() });

function startState(mode: any, ...a: any[]) {
  return mode.startState ? mode.startState(...a) : true;
}
function copyState(mode: any, state: any) {
  if (state === true) return state;
  if (mode.copyState) return mode.copyState(state);
  const n: any = {};
  for (const k in state) n[k] = Array.isArray(state[k]) ? state[k].concat([]) : state[k];
  return n;
}

/** CM5 overlay: run `overlay` on top of `base`, overlay tokens win. */
function overlayMode(base: any, overlay: any, combine?: boolean) {
  return {
    name: base.name,
    startState: () => ({ base: startState(base), overlay: startState(overlay), basePos: 0, baseCur: null, overlayPos: 0, overlayCur: null, streamSeen: null }),
    copyState: (s: any) => ({ base: copyState(base, s.base), overlay: copyState(overlay, s.overlay), basePos: s.basePos, baseCur: null, overlayPos: s.overlayPos, overlayCur: null }),
    token(stream: any, state: any) {
      if (stream !== state.streamSeen || Math.min(state.basePos, state.overlayPos) < stream.start) {
        state.streamSeen = stream;
        state.basePos = state.overlayPos = stream.start;
      }
      if (stream.start === state.basePos) {
        state.baseCur = base.token(stream, state.base);
        state.basePos = stream.pos;
      }
      if (stream.start === state.overlayPos) {
        stream.pos = stream.start;
        state.overlayCur = overlay.token(stream, state.overlay);
        state.overlayPos = stream.pos;
      }
      stream.pos = Math.min(state.basePos, state.overlayPos);
      if (state.overlayCur == null) return state.baseCur;
      if ((state.baseCur != null && overlay.combineTokens) || (combine && overlay.combineTokens == null)) return state.baseCur + " " + state.overlayCur;
      return state.overlayCur;
    },
    indent: base.indent && ((state: any, text: string, cx: any) => base.indent(state.base, text, cx)),
    blankLine(state: any) {
      if (base.blankLine) base.blankLine(state.base);
      if (overlay.blankLine) overlay.blankLine(state.overlay);
    },
    languageData: base.languageData,
  };
}

if (!W.CodeMirror) {
  const factories: Record<string, any> = {
    javascript: () => legacyJavascript,
    hypermd: () => ({ name: "hypermd", startState: () => ({}), token: (s: any) => void s.skipToEnd() }),
    markdown: () => ({ name: "markdown", startState: () => ({}), token: (s: any) => void s.skipToEnd() }),
    null: nullMode,
  };
  const CM5: any = function () {
    throw new Error("CodeMirror 5 editors are not available in Ateliet");
  };
  Object.assign(CM5, {
    modes: factories,
    mimeModes: {},
    modeInfo: [{ name: "JavaScript", mime: "text/javascript", mode: "javascript", ext: ["js"] }] as any[],
    defineMode(name: string, factory: any) {
      factories[name] = factory;
    },
    defineMIME() {},
    defineSimpleMode(name: string, _states: any) {
      factories[name] = nullMode;
    },
    getMode(config: any, spec: any) {
      const name = typeof spec === "string" ? spec : spec?.name;
      const f = factories[name];
      if (!f) return nullMode();
      try {
        const m = f(config || {}, typeof spec === "object" ? spec : { name });
        if (!m || typeof m.token !== "function") return nullMode();
        m.name ||= name;
        return m;
      } catch (e) {
        console.error("CodeMirror mode", name, e);
        return nullMode();
      }
    },
    findModeByName: (n: string) => CM5.modeInfo.find((m: any) => m.name?.toLowerCase() === n?.toLowerCase()) || null,
    findModeByExtension: () => null,
    findModeByMIME: () => null,
    overlayMode,
    startState,
    copyState,
    innerMode: (mode: any, state: any) => ({ mode, state }),
    registerHelper: () => {},
    defineExtension: () => {},
    defineOption: () => {},
    commands: {},
    keyMap: { default: {} },
    Pos: (line: number, ch: number) => ({ line, ch }),
  });
  W.CodeMirror = CM5;
}

// ── property types (types.json, shared with Obsidian) ─────────────────────
export class MetadataTypeManager extends Events {
  app: any;
  types: Record<string, string> = {};
  properties: Record<string, any> = {};
  registeredTypeWidgets: Record<string, any> = {};
  constructor(app: any) {
    super();
    this.app = app;
  }
  async load() {
    const raw = await this.app.readJson("types.json", { types: {} });
    this.types = raw.types || {};
    this.updatePropertyInfoCache();
    this.app.metadataCache.on("resolved", () => this.updatePropertyInfoCache());
  }
  updatePropertyInfoCache() {
    this.properties = this.app.metadataCache.getAllPropertyInfos();
    for (const k in this.properties) {
      const t = this.types[k] || this.types[this.properties[k].name];
      if (t) this.properties[k].type = t;
    }
  }
  getAllProperties() {
    return this.properties;
  }
  getPropertyInfo(key: string) {
    return this.properties[key.toLowerCase()];
  }
  getAssignedType(key: string) {
    return this.types[key] ?? null;
  }
  getTypeInfo(key: string) {
    const t = this.getAssignedType(key) || this.getPropertyInfo(key)?.type || "text";
    return { expected: { type: t }, inferred: { type: t } };
  }
  async setType(key: string, type: string) {
    this.types[key] = type;
    await this.save();
  }
  async unsetType(key: string) {
    delete this.types[key];
    await this.save();
  }
  async save() {
    const raw = await this.app.readJson("types.json", {});
    raw.types = this.types;
    await this.app.writeJson("types.json", raw);
    this.updatePropertyInfoCache();
    this.trigger("changed");
  }
}

// ── markdown embeds (embedRegistry.embedByExtension.md) ───────────────────
class MarkdownEmbed extends Component {
  app: any;
  containerEl: HTMLElement;
  file: TFile | null;
  subpath: string;
  state: any;
  editable = false;
  constructor(ctx: any, file: TFile | null, subpath = "") {
    super();
    this.app = ctx.app;
    this.containerEl = ctx.containerEl || createDiv();
    this.state = ctx.state || {};
    this.file = file;
    this.subpath = subpath;
    this.containerEl.addClass("markdown-embed");
  }
  async loadFile() {
    if (!this.file) return;
    let text = await this.app.vault.cachedRead(this.file);
    if (this.subpath) {
      const r = resolveSubpath(this.app.metadataCache.getFileCache(this.file), this.subpath);
      if (r) {
        const lines = text.split("\n");
        text = lines.slice(r.start.line, r.end ? r.end.line + (r.type === "block" ? 1 : 0) : lines.length).join("\n");
      }
    }
    this.containerEl.empty();
    const view = this.containerEl.createDiv("markdown-embed-content").createDiv("markdown-preview-view markdown-rendered");
    await renderMarkdownInto(this.app, text, view, this.file.path, this, { sections: true });
  }
  editMode: EmbedEditView | null = null;
  showEditor() {
    if (!this.editMode) {
      this.editMode = new EmbedEditView(this.app, this.containerEl, this);
      this.addChild(this.editMode);
    }
  }
  showPreview() {
    return this.loadFile();
  }
  onload() {
    if (this.file) this.loadFile();
  }
}

/** Rebuild open editors (plugins that render once at construction, e.g. Dataview inline queries). */
export function refreshEditors(app: any) {
  app.workspace.iterateAllLeaves((l: any) => {
    const v = l.view;
    if (!v?.editMode?.cm || !v.loaded) return;
    const cm = v.editMode.cm;
    const sel = cm.state.selection;
    const top = cm.scrollDOM.scrollTop;
    cm.setState(v.createState(cm.state.doc.toString()));
    try {
      cm.dispatch({ selection: sel });
    } catch { /* doc changed */ }
    cm.scrollDOM.scrollTop = top;
  });
}

// ── canvas (core plugin) — just enough for Excalidraw's embedded notes ─────
class CanvasFileNode extends Component {
  app: any;
  containerEl: HTMLElement;
  contentEl: HTMLElement;
  file: TFile | null = null;
  subpath = "";
  child: any = { editor: null };
  constructor(app: any) {
    super();
    this.app = app;
    this.containerEl = createDiv("canvas-node");
    this.contentEl = this.containerEl.createDiv("canvas-node-container").createDiv("canvas-node-content markdown-embed");
  }
  setFilePath(path: string, subpath = "") {
    this.file = this.app.vault.getAbstractFileByPath(path);
    this.subpath = subpath;
  }
  render() {
    const embed = new MarkdownEmbed({ app: this.app, containerEl: this.contentEl }, this.file, this.subpath);
    this.addChild(embed);
    this.child = embed;
    this.load();
  }
  isEditable() {
    return false;
  }
  startEditing() {}
  blur() {}
  destroy() {
    this.unload();
    this.containerEl.remove();
  }
}

function makeCanvasPlugin(app: any) {
  return {
    id: "canvas",
    name: "Canvas",
    _loaded: true,
    enabled: true,
    instance: { options: {}, hidden: true },
    async load() {},
    async enable() {},
    async disable() {},
    views: {
      canvas: (leaf: any) => ({
        leaf,
        canvas: {
          nodes: new Map(),
          createFileNode(o: any) {
            const n = new CanvasFileNode(app);
            if (o?.file) n.setFilePath(o.file.path, o.subpath || "");
            return n;
          },
          removeNode(n: any) {
            n?.destroy?.();
          },
          requestSave() {},
        },
      }),
    },
  };
}

export function installCompat(app: any) {
  app.internalPlugins.plugins.canvas ||= makeCanvasPlugin(app);
  app.metadataCache.on("dataview:index-ready", () => setTimeout(() => refreshEditors(app), 50));
  app.metadataTypeManager = new MetadataTypeManager(app);
  app.metadataCache.getCachedFiles = () => Object.keys(app.metadataCache.fileCache);
  app.metadataCache.getAllProperties = () => app.metadataTypeManager.getAllProperties();
  app.embedRegistry.registerExtension("md", (ctx: any, file: TFile | null, subpath?: string) => new MarkdownEmbed(ctx, file, subpath));
  app.dragManager = app.dragManager || { draggable: null, setAction() {}, dragFile() { return {}; }, dragFiles() { return {}; }, handleDrop() {}, handleDrag() {}, onDragStart() {} };
  app.foldManager = { load: async () => null, save: async () => {} };
  app.account = { email: "", name: "", company: "", key: null, expiry: 0 };
  app.isMobile = false;
  app.workspace.recentFileTracker = {
    lastOpenFiles: app.workspace.lastOpenFiles,
    getRecentFiles: (o: any = {}) => app.workspace.getLastOpenFiles().slice(0, o.maxCount ?? 10),
    getLastOpenFiles: () => app.workspace.getLastOpenFiles(),
    collect() {},
    onFileOpen() {},
  };
  app.mobileToolbar = null;
  app.title = "Ateliet";
  app.vault.getAvailablePathForAttachments ||= (base: string, ext: string, file: any) => app.fileManager.getAvailablePathForAttachment(`${base}.${ext}`, file?.path);
}
