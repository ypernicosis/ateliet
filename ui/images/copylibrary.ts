// A library of Ateliet's own: Pictogrep's whole folder (pictures, collections,
// analysed pictures, followed boards, plugins and their data, storyboards)
// copied into a folder Ateliet then uses, so nothing depends on Pictogrep
// being installed, open, or where it was.

import { invoke, listen } from "../tauri.ts";
import { Modal, Notice, Setting } from "../obsidian/ui.ts";

interface CopyPlan {
  from: string | null;
  to: string;
  running?: boolean;
  plan?: { files: number; bytes: number; pictures: number; collections: number; plugins: number; embeddings: boolean; storyboards: number };
}

const size = (n: number) => (n > 1e9 ? `${(n / 1e9).toFixed(1)} GB` : n > 1e6 ? `${Math.round(n / 1e6)} MB` : `${Math.round(n / 1e3)} KB`);

export class CopyLibraryModal extends Modal {
  private from = "";
  private to = "";
  private use = true;
  private busy = false;
  private off: (() => void) | null = null;

  constructor(app: any, private host: { updateSettings(patch: any): Promise<void> }) {
    super(app);
  }

  async onOpen() {
    this.setTitle("Copy Pictogrep's library into Ateliet");
    this.modalEl.addClass("copy-library-modal");
    await this.load();
  }

  private async load(from?: string) {
    const el = this.contentEl;
    el.empty();
    el.createDiv({ cls: "setting-item-description", text: "Loading…" });
    let p: CopyPlan;
    try {
      p = await invoke<CopyPlan>("pictogrep_copy_plan", { from: from ?? null });
    } catch (e) {
      p = { from: from || null, to: this.to };
      el.empty();
      el.createDiv({ cls: "mod-warning", text: String(e) });
    }
    this.from = p.from || "";
    this.to ||= p.to;
    this.render(p);
  }

  private render(p: CopyPlan) {
    const el = this.contentEl;
    if (p.plan) el.empty();
    el.createEl("p", {
      text: "Everything in Pictogrep's folder is copied into a folder of Ateliet's own: pictures, collections, the pictures already analysed for search, followed Pinterest boards, plugins and what they saved, and Pictogrep's storyboards. Pictogrep keeps its folder as it is; from then on the two libraries are separate, and Ateliet works the same with Pictogrep open, closed or uninstalled.",
    });
    new Setting(el)
      .setName("From")
      .setDesc(p.from ? p.from : "Pictogrep's folder was not found. Choose it (Pictogrep shows it in its settings).")
      .addButton((b) => b.setButtonText(p.from ? "Another folder…" : "Choose…").onClick(async () => {
        const path: string | null = await invoke("pick_folder");
        if (path) await this.load(path);
      }));
    if (p.plan) {
      const x = p.plan;
      const n = (k: number, one: string, many: string) => `${k.toLocaleString()} ${k === 1 ? one : many}`;
      const parts = [n(x.pictures, "picture", "pictures"), n(x.collections, "collection", "collections")];
      if (x.storyboards) parts.push(n(x.storyboards, "storyboard frame", "storyboard frames"));
      if (x.plugins) parts.push(n(x.plugins, "plugin", "plugins"));
      if (x.embeddings) parts.push("search data");
      el.createDiv({ cls: "copy-library-summary", text: `${parts.join(" · ")} — ${n(x.files, "file", "files")}, ${size(x.bytes)}.` });
      el.createDiv({ cls: "setting-item-description", text: "On disks that support it (APFS on macOS, Btrfs or XFS on Linux) the copied pictures share space with the originals until one of them changes, so the copy is fast and takes almost no room." });
      if (p.running) el.createDiv({ cls: "setting-item-description mod-warning", text: "Pictogrep is open: what it changes during the copy may not be in it. Closing it first is safer." });
    }
    const toSetting = new Setting(el).setName("To").setDesc(this.to).addButton((b) => b.setButtonText("Choose…").onClick(async () => {
      const path: string | null = await invoke("pick_folder");
      if (path) {
        this.to = path;
        toSetting.setDesc(path);
      }
    }));
    el.createDiv({ cls: "setting-item-description", text: "A new or empty folder." });
    new Setting(el).setName("Use the copy from now on").setDesc("Settings → Images → Pictogrep folder will point at the copy.").addToggle((t) => t.setValue(this.use).onChange((v) => (this.use = v)));
    const bar = el.createDiv("copy-library-progress");
    const fill = bar.createDiv("copy-library-fill");
    const status = el.createDiv({ cls: "setting-item-description copy-library-status" });
    bar.hide();
    const buttons = el.createDiv("modal-button-container");
    const go = buttons.createEl("button", { text: "Copy", cls: "mod-cta" });
    const cancel = buttons.createEl("button", { text: "Cancel" });
    go.disabled = !p.plan;
    cancel.addEventListener("click", () => (this.busy ? invoke("pictogrep_copy_cancel") : this.close()));
    go.addEventListener("click", async () => {
      if (this.busy) return;
      this.busy = true;
      go.disabled = true;
      bar.show();
      status.setText("Copying…");
      this.off = await listen("pictogrep-copy", (pr: any) => {
        const k = pr.totalBytes ? pr.bytes / pr.totalBytes : pr.totalFiles ? pr.files / pr.totalFiles : 1;
        fill.style.width = `${Math.round(k * 100)}%`;
        status.setText(`${pr.files.toLocaleString()} of ${pr.totalFiles.toLocaleString()} files · ${size(pr.bytes)} of ${size(pr.totalBytes)}`);
      });
      try {
        const res = await invoke<{ home: string }>("pictogrep_copy", { from: this.from, to: this.to });
        if (this.use) await this.host.updateSettings({ shared: res.home });
        new Notice(this.use ? `The library is now Ateliet's own, in ${res.home}.` : `Copied into ${res.home}.`);
        this.busy = false;
        this.close();
      } catch (e) {
        this.busy = false;
        go.disabled = false;
        bar.hide();
        status.setText(String(e) === "cancelled" ? "Stopped; nothing was kept." : `The copy failed: ${e}`);
      } finally {
        this.off?.();
        this.off = null;
      }
    });
  }

  onClose() {
    if (this.busy) invoke("pictogrep_copy_cancel");
    this.off?.();
    this.contentEl.empty();
  }
}
