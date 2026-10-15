// Self-update: Rust checks the git checkout Ateliet was built from (src/update.rs).
// A ribbon button appears while the upstream branch has commits this build lacks.

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

export function installUpdater(app: any, flush: () => Promise<void>) {
  let status: UpdateStatus | null = null;
  let notified = "";
  let restarting = false;

  const button = app.workspace.leftRibbon.addSettingsButton("circle-arrow-down", "Update Ateliet", () => openModal());
  button.addClass("mod-update");
  button.hide();

  async function check(manual = false) {
    try {
      status = await invoke<UpdateStatus>("update_check");
    } catch (e) {
      if (manual) new Notice(`Could not check for updates: ${e}`);
      return;
    }
    button.toggle(status.available);
    if (!status.available) {
      if (manual) new Notice("Ateliet is up to date.");
      return;
    }
    const label = `Update Ateliet (${status.behind} new commit${status.behind === 1 ? "" : "s"})`;
    button.setAttr("aria-label", label);
    if (manual) openModal();
    else if (notified !== status.latest) {
      notified = status.latest;
      new Notice(`${label} — click the arrow at the bottom of the ribbon.`, 8000);
    }
  }

  function openModal() {
    if (status?.available) new UpdateModal(app, status).open();
    else check(true);
  }

  // Every window flushes its notes before the app restarts; the one that started the update restarts it.
  listen("update-progress", async (p: { kind: string; text: string }) => {
    if (p.kind !== "done") return;
    await flush().catch(console.error);
    if (UpdateModal.initiator && !restarting) {
      restarting = true;
      setTimeout(() => invoke("update_restart"), 500);
    }
  });

  app.commands.addCommand({ id: "ateliet:check-for-updates", name: "Check for updates", icon: "circle-arrow-down", callback: () => check(true) });
  setTimeout(() => check(), 5000);
  setInterval(() => check(), CHECK_EVERY);
}

class UpdateModal extends Modal {
  static initiator = false;
  private unlisten: (() => void) | null = null;

  constructor(app: any, readonly status: UpdateStatus) {
    super(app);
  }

  onOpen() {
    const s = this.status;
    this.modalEl.addClass("mod-ateliet-update");
    this.setTitle("Update Ateliet");
    const c = this.contentEl;
    c.createEl("p", { text: `${s.behind} new commit${s.behind === 1 ? "" : "s"} on ${s.upstream}. Ateliet will pull them, rebuild and restart.` });
    const list = c.createEl("ul", { cls: "ateliet-update-commits" });
    for (const line of s.commits) {
      const [hash, ...msg] = line.split(" ");
      const li = list.createEl("li");
      li.createEl("code", { text: hash });
      li.appendText(" " + msg.join(" "));
    }
    if (s.behind > s.commits.length) list.createEl("li", { text: `… and ${s.behind - s.commits.length} more`, cls: "mod-muted" });
    const stepEl = c.createDiv({ cls: "ateliet-update-step" });
    const logEl = c.createEl("pre", { cls: "ateliet-update-log" });
    logEl.hide();
    const buttons = this.modalEl.createDiv("modal-button-container");
    const go = buttons.createEl("button", { cls: "mod-cta", text: "Update and restart" });
    const cancel = buttons.createEl("button", { text: "Later" });
    cancel.addEventListener("click", () => this.close());
    go.addEventListener("click", async () => {
      go.disabled = true;
      cancel.disabled = true;
      logEl.empty();
      logEl.show();
      this.unlisten ??= await listen("update-progress", (p: { kind: string; text: string }) => {
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
        stepEl.setText(String(e));
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
