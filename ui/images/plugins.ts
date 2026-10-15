// Pictogrep plugins inside image mode. A plugin is a web page in a sandboxed
// iframe (scripts only: no same-origin, no popups, no network by CSP) served
// from its own pgplugin:// origin. Its only way out is postMessage, which this
// view relays to the Rust core after the permission check there.

import { SharedPlugin, api, pluginBase } from "./api.ts";
import { ItemView } from "../obsidian/views.ts";
import { invoke } from "../tauri.ts";

export const PLUGIN_VIEW = "shared-plugin";

export class SharedPluginView extends ItemView {
  plugin: SharedPlugin | null = null;
  private frame: HTMLIFrameElement | null = null;
  private onMessage = (e: MessageEvent) => this.handle(e);

  getViewType() {
    return PLUGIN_VIEW;
  }
  getIcon() {
    return "puzzle";
  }
  getDisplayText() {
    return this.plugin?.name || "Plugin";
  }
  getState(): any {
    return { id: this.plugin?.id || "" };
  }

  async setState(state: any) {
    const id = state?.id;
    if (!id || id === this.plugin?.id) return;
    const info = await api.sharedInfo().catch(() => null);
    this.plugin = info?.plugins?.find((p) => p.id === id) || null;
    this.render();
    this.updateHeader();
  }

  async onOpen() {
    this.contentEl.addClass("shared-plugin-view");
    window.addEventListener("message", this.onMessage);
  }

  async onClose() {
    window.removeEventListener("message", this.onMessage);
  }

  private render() {
    this.contentEl.empty();
    if (!this.plugin) {
      this.contentEl.createDiv({ cls: "image-library-empty", text: "This plugin is not installed in the shared library." });
      return;
    }
    const entry = this.plugin.entry.split("/").map(encodeURIComponent).join("/");
    this.frame = this.contentEl.createEl("iframe", {
      cls: "shared-plugin-frame",
      attr: { sandbox: "allow-scripts", src: `${pluginBase()}/plugin/${encodeURIComponent(this.plugin.id)}/${entry}`, title: this.plugin.name },
    });
  }

  private async handle(e: MessageEvent) {
    if (!this.frame || e.source !== this.frame.contentWindow || !this.plugin) return;
    const { callId, method, args } = (e.data || {}) as any;
    if (!callId || typeof method !== "string") return;
    const reply = (ok: boolean, payload: any) => this.frame?.contentWindow?.postMessage(ok ? { callId, ok, result: payload } : { callId, ok, error: String(payload) }, "*");
    try {
      if (method === "ui.openExternal") {
        // Opened by the host: the frame has no popups. https only, so a
        // plugin cannot hand back a javascript:/file: address.
        if (!this.plugin.permissions.includes("ui.openExternal")) throw new Error(`plugin ${this.plugin.id} did not declare ui.openExternal`);
        const url = String(args?.url || "");
        if (!/^https:\/\//i.test(url)) throw new Error("ui.openExternal: only https:// URLs are allowed");
        await invoke("open_external", { url });
        return reply(true, { ok: true });
      }
      const result = await api.pluginCall(this.plugin.id, method, args || {});
      if (method === "images.tag") this.app.workspace.trigger("images:changed");
      reply(true, result);
    } catch (err: any) {
      reply(false, err?.message || err);
    }
  }
}
