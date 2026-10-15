// Import from Pinterest: a board, a pin or a pin.it link, into the shared
// library (Pictogrep's), optionally collected and followed. The download runs
// in the Rust core and survives closing this dialog.

import { DownloadJob, api } from "./api.ts";
import { ImagesHost } from "./actions.ts";
import { Modal, Notice, Setting } from "../obsidian/ui.ts";

export function describeJob(job: DownloadJob): string {
  if (job.state === "running") {
    if (job.stopping) return `Stopping ${job.board}… (keeping what already arrived)`;
    if (job.phase === "importing") return `${job.board}: adding ${job.done} of ${job.total}`;
    return `${job.board}: downloading${job.done ? ` (${job.done})` : ""}…`;
  }
  if (job.state === "error") return `${job.board}: ${job.error}`;
  if ((job.state === "done" || job.state === "cancelled") && job.result) {
    const r = job.result;
    const parts = [`${r.imported} new`];
    if (r.linked) parts.push(`${r.linked} already in the library added to the collection`);
    if (r.skipped) parts.push(`${r.skipped} duplicates skipped`);
    if (r.failed) parts.push(`${r.failed} failed${r.lastError ? ` (${r.lastError})` : ""}`);
    return `${job.board}: ${parts.join(", ")}${job.state === "cancelled" ? " (stopped)" : ""}.`;
  }
  return "";
}

export class PinterestModal extends Modal {
  private statusEl!: HTMLElement;
  private startButton!: HTMLButtonElement;
  private off: (() => void) | null = null;

  constructor(app: any, private host: ImagesHost, private initial: { url?: string; collection?: string } = {}) {
    super(app);
  }

  async onOpen() {
    this.setTitle("Import from Pinterest");
    this.modalEl.addClass("pinterest-modal");
    const info = await api.sharedInfo().catch(() => ({ home: null }) as any);
    const shared = !!info.home;
    const collections: string[] = (info.collections || []).map((c: any) => c.name);
    const req = { url: this.initial.url || "", mode: this.initial.collection ? "existing" : "new", folder: this.initial.collection || "", backfill: true, follow: false, skipExisting: false, vaultFolder: this.host.settings.pinterestFolder || "Pinterest" };

    const el = this.contentEl;
    el.createEl("p", {
      cls: "setting-item-description",
      text: shared
        ? `Pictures go into the shared library (${info.library}), which Pictogrep sees as well.`
        : `No shared library is in use, so pictures go into the vault folder "${req.vaultFolder}". Settings → Images can point to Pictogrep's folder.`,
    });
    new Setting(el).setName("Board, pin or pin.it link").addText((t) => {
      t.setPlaceholder("https://pinterest.com/someone/a-board/").setValue(req.url).onChange((v) => (req.url = v.trim()));
      t.inputEl.style.width = "100%";
      setTimeout(() => t.inputEl.focus(), 0);
    });
    const folderSetting = new Setting(el).setName("Where to put the pictures");
    let folderInput: HTMLElement | null = null;
    folderSetting.addDropdown((d) => {
      d.addOption("new", shared ? "A new collection named after the board" : "A new folder named after the board");
      if (shared ? collections.length : true) d.addOption("existing", shared ? "A collection I already have" : "A folder of the vault");
      d.addOption("direct", shared ? "Straight into the library, no collection" : `Straight into "${req.vaultFolder}"`);
      d.setValue(req.mode).onChange((v) => {
        req.mode = v;
        renderFolder();
      });
    });
    const folderRow = el.createDiv();
    const renderFolder = () => {
      folderRow.empty();
      folderInput = null;
      if (req.mode === "direct") return;
      const s = new Setting(folderRow).setName(req.mode === "new" ? "Name (empty uses the board's name)" : shared ? "Collection" : "Vault folder");
      if (req.mode === "existing" && shared) {
        s.addDropdown((d) => {
          for (const c of collections) d.addOption(c, c);
          if (!collections.includes(req.folder)) req.folder = collections[0] || "";
          d.setValue(req.folder).onChange((v) => (req.folder = v));
        });
      } else {
        s.addText((t) => t.setValue(req.mode === "new" ? "" : req.folder).onChange((v) => (req.folder = v)));
      }
      folderInput = s.settingEl;
    };
    renderFolder();
    new Setting(el)
      .setName("Download the pins already on the board")
      .setDesc("Off: only follow the board from now on (its newest pins).")
      .addToggle((t) => t.setValue(req.backfill).onChange((v) => (req.backfill = v)));
    new Setting(el)
      .setName("Follow this board")
      .setDesc("New pins are fetched once a day, by Ateliet or by Pictogrep, whichever is open.")
      .addToggle((t) => t.setValue(req.follow).onChange((v) => (req.follow = v)));
    new Setting(el)
      .setName("Leave pictures already in the library where they are")
      .setDesc("Off: an exact duplicate is added to the collection without a second copy.")
      .addToggle((t) => t.setValue(req.skipExisting).onChange((v) => (req.skipExisting = v)));
    if (!info.galleryDl) {
      el.createEl("p", { cls: "setting-item-description", text: "gallery-dl is not installed, so Ateliet reads the board itself (public boards and pins)." });
    }
    this.statusEl = el.createDiv("pinterest-status");
    const buttons = this.modalEl.createDiv("modal-button-container");
    this.startButton = buttons.createEl("button", { text: "Download", cls: "mod-cta" });
    const stop = buttons.createEl("button", { text: "Stop" });
    stop.addEventListener("click", () => api.pinterestCancel().catch(() => {}));
    this.startButton.addEventListener("click", async () => {
      try {
        const job = await api.pinterestStart(req);
        this.show(job);
      } catch (e: any) {
        this.statusEl.setText(String(e?.message || e));
        this.statusEl.addClass("mod-warning");
      }
    });
    const ref = this.app.workspace.on("pinterest:progress", (job: DownloadJob) => this.show(job));
    this.off = () => this.app.workspace.offref(ref);
    this.show(await api.pinterestStatus());
    void folderInput;
  }

  private show(job: DownloadJob) {
    this.statusEl.removeClass("mod-warning");
    this.statusEl.setText(describeJob(job));
    this.statusEl.toggleClass("mod-warning", job.state === "error");
    this.startButton.disabled = job.state === "running";
  }

  onClose() {
    this.off?.();
    this.contentEl.empty();
  }
}

/** Followed boards, for the settings tab. */
export async function renderFollowed(el: HTMLElement) {
  el.empty();
  const data = await api.pinterestFollowed().catch(() => null);
  if (!data) return;
  if (!data.boards.length) {
    el.createDiv({ cls: "setting-item-description", text: "No followed boards. Turn on \"Follow this board\" when importing one." });
    return;
  }
  for (const b of data.boards) {
    const when = b.lastSyncAt ? new Date(b.lastSyncAt * 1000).toLocaleString() : "never";
    new Setting(el)
      .setName(b.name || b.url)
      .setDesc(`${b.url} · into ${b.folder || "the library"} · last checked ${when}`)
      .addButton((btn) => btn.setButtonText("Check now").onClick(() => api.pinterestCheck(b.url).catch((e) => new Notice(String(e)))))
      .addExtraButton((x) =>
        x.setIcon("x").setTooltip("Stop following").onClick(async () => {
          await api.pinterestForget(b.url);
          renderFollowed(el);
        }),
      );
  }
  if (!data.autoSync) el.createDiv({ cls: "setting-item-description", text: "Automatic checks are switched off in Pictogrep's settings." });
}
