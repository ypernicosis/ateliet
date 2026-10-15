// Self-update: Rust checks the git checkout Ateliet was built from (src/update.rs).
// The ribbon button is always there; it lights up while the upstream branch
// has commits this build lacks.

import { Modal, Notice } from "../obsidian/index.ts";
import { invoke, listen } from "../tauri.ts";

const CHECK_EVERY = 30 * 60 * 1000;

interface UpdateStatus {
  available: boolean;
  behind: number;
  current: string;
  latest: string;
  upstream: string;
  commits: string[];
}

interface Progress {
  kind: "step" | "log" | "done" | "error";
  text: string;
}

const plural = (n: number) => `${n} new commit${n === 1 ? "" : "s"}`;

export function installUpdater(app: any, flush: () => Promise<void>) {
  let notified = "";
  let restarting = false;

  const button = app.workspace.leftRibbon.addSettingsButton("circle-arrow-down", "Update Ateliet", () => new UpdateModal(app, check).open());
  button.addClass("mod-update");

  /** Asks Rust; keeps the button in step. Throws when the check itself fails. */
  async function check(): Promise<UpdateStatus> {
    const status = await invoke<UpdateStatus>("update_check");
    button.toggleClass("is-available", status.available);
    button.setAttr("aria-label", status.available ? `Update Ateliet (${plural(status.behind)})` : "Update Ateliet");
    return status;
  }

  async function background() {
    try {
      const status = await check();
      if (status.available && notified !== status.latest) {
        notified = status.latest;
        new Notice(`Ateliet has an update (${plural(status.behind)}). Click the arrow at the bottom of the ribbon.`, 8000);
      }
    } catch (e) {
      console.warn("update check:", e);
    }
  }

  // Every window saves its notes before the app restarts; the window that started the update restarts it.
  listen("update-progress", async (p: Progress) => {
    if (p.kind !== "done") return;
    await flush().catch(console.error);
    if (UpdateModal.initiator && !restarting) {
      restarting = true;
      setTimeout(() => invoke("update_restart"), 500);
    }
  });

  app.commands.addCommand({ id: "ateliet:update", name: "Update Ateliet", icon: "circle-arrow-down", callback: () => new UpdateModal(app, check).open() });
  setTimeout(background, 5000);
  setInterval(background, CHECK_EVERY);
}

class UpdateModal extends Modal {
  static initiator = false;
  private unlisten: (() => void) | null = null;

  constructor(app: any, private check: () => Promise<UpdateStatus>) {
    super(app);
  }

  async onOpen() {
    this.modalEl.addClass("mod-ateliet-update");
    this.setTitle("Update Ateliet");
    const c = this.contentEl;
    const info = c.createDiv();
    info.createEl("p", { text: "Checking for updates…", cls: "mod-muted" });
    const stepEl = c.createDiv({ cls: "ateliet-update-step" });
    const logEl = c.createEl("pre", { cls: "ateliet-update-log" });
    logEl.hide();
    const buttons = this.modalEl.createDiv("modal-button-container");
    const go = buttons.createEl("button", { cls: "mod-cta", text: "Update and restart" });
    const cancel = buttons.createEl("button", { text: "Close" });
    go.disabled = true;
    cancel.addEventListener("click", () => this.close());

    let status: UpdateStatus;
    try {
      status = await this.check();
    } catch (e) {
      info.empty();
      info.createEl("p", { text: "Could not check for updates:" });
      info.createEl("pre", { cls: "ateliet-update-log", text: String(e) });
      go.remove();
      return;
    }
    info.empty();
    if (status.available) {
      info.createEl("p", { text: `${plural(status.behind)} on ${status.upstream}. Ateliet will download them, rebuild itself and restart.` });
      const list = info.createEl("ul", { cls: "ateliet-update-commits" });
      for (const line of status.commits) {
        const [hash, ...msg] = line.split(" ");
        const li = list.createEl("li");
        li.createEl("code", { text: hash });
        li.appendText(" " + msg.join(" "));
      }
      if (status.behind > status.commits.length) list.createEl("li", { text: `… and ${status.behind - status.commits.length} more`, cls: "mod-muted" });
    } else {
      info.createEl("p", { text: `Ateliet is up to date with ${status.upstream} (${status.current.slice(0, 7)}).` });
      go.setText("Rebuild anyway");
      go.removeClass("mod-cta");
    }
    go.disabled = false;

    go.addEventListener("click", async () => {
      go.disabled = true;
      cancel.disabled = true;
      logEl.empty();
      logEl.show();
      stepEl.empty();
      this.unlisten ??= await listen("update-progress", (p: Progress) => {
        if (p.kind === "step") stepEl.setText(p.text + "…");
        else if (p.kind === "log") {
          logEl.appendText(p.text + "\n");
          logEl.scrollTop = logEl.scrollHeight;
        } else if (p.kind === "done") stepEl.setText("Restarting…");
        else if (p.kind === "error") {
          UpdateModal.initiator = false;
          stepEl.empty();
          stepEl.createSpan({ cls: "mod-warning", text: p.text });
          go.disabled = false;
          cancel.disabled = false;
          go.setText("Try again");
        }
      });
      UpdateModal.initiator = true;
      try {
        await invoke("update_apply");
      } catch (e) {
        UpdateModal.initiator = false;
        stepEl.empty();
        stepEl.createSpan({ cls: "mod-warning", text: String(e) });
        go.disabled = false;
        cancel.disabled = false;
      }
    });
  }

  onClose() {
    this.unlisten?.();
    this.unlisten = null;
  }
}
