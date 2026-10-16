// Self-update: Rust checks the git checkout Ateliet was built from (src/update.rs).
// The ribbon button is always there; it lights up while the upstream branch
// has commits this build lacks. With automatic updates on (the default), the
// background check also pulls and rebuilds; the new build runs at the next
// start, or right away from the notice that offers to restart.

import { Modal, Notice, Setting } from "../obsidian/index.ts";
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

/** App-wide, not per vault: kept in the webview's storage. */
const AUTO_KEY = "ateliet-auto-update";
export function autoUpdateEnabled(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) !== "off";
  } catch {
    return true;
  }
}
export function setAutoUpdate(on: boolean) {
  try {
    localStorage.setItem(AUTO_KEY, on ? "on" : "off");
  } catch {
    /* storage unavailable: stays on */
  }
}

/** The automatic update in this run of the app: idle, building, built (waits for a restart) or failed. */
const auto = { state: "idle" as "idle" | "running" | "ready" | "failed", latest: "", step: "" };

export function installUpdater(app: any, flush: () => Promise<void>) {
  let notified = "";
  let restarting = false;

  const button = app.workspace.leftRibbon.addSettingsButton("circle-arrow-down", "Update Ateliet", () => new UpdateModal(app, check).open());
  button.addClass("mod-update");

  /** Asks Rust; keeps the button in step. Throws when the check itself fails. */
  async function check(): Promise<UpdateStatus> {
    const status = await invoke<UpdateStatus>("update_check");
    button.toggleClass("is-available", status.available);
    if (auto.state !== "ready") button.setAttr("aria-label", status.available ? `Update Ateliet (${plural(status.behind)})` : "Update Ateliet");
    return status;
  }

  async function background() {
    try {
      const status = await check();
      if (!status.available || UpdateModal.initiator) return;
      if (autoUpdateEnabled()) {
        // A build already made or one that failed for this same commit is not tried again every half hour.
        if (auto.state === "running" || (auto.latest === status.latest && auto.state !== "idle")) return;
        auto.latest = status.latest;
        auto.state = "running";
        try {
          await invoke("update_apply");
          new Notice(`Updating Ateliet in the background (${plural(status.behind)}).`, 5000);
        } catch (e) {
          // Another window started it: its progress reaches this one too.
          if (!String(e).includes("already running")) fail(String(e));
        }
      } else if (notified !== status.latest) {
        notified = status.latest;
        new Notice(`Ateliet has an update (${plural(status.behind)}). Click the arrow at the bottom of the ribbon.`, 8000);
      }
    } catch (e) {
      console.warn("update check:", e);
    }
  }

  function fail(text: string) {
    auto.state = "failed";
    console.warn("automatic update:", text);
    new Notice(`The automatic update failed: ${text.split("\n")[0]} Open "Update Ateliet" for the full log.`, 12000);
  }

  // Every window saves its notes before the app restarts; the window that started the update restarts it.
  listen("update-progress", async (p: Progress) => {
    if (p.kind === "step") {
      auto.step = p.text;
      if (!UpdateModal.initiator && auto.state !== "ready") auto.state = "running";
      return;
    }
    if (p.kind === "error") {
      if (!UpdateModal.initiator && auto.state === "running") fail(p.text);
      return;
    }
    if (p.kind !== "done") return;
    await flush().catch(console.error);
    if (UpdateModal.initiator) {
      if (!restarting) {
        restarting = true;
        setTimeout(() => invoke("update_restart"), 500);
      }
      return;
    }
    // Built in the background: the new version starts with the app. Offer to restart now.
    auto.state = "ready";
    button.addClass("is-ready");
    button.setAttr("aria-label", "Ateliet was updated: restart to use the new version");
    const n = new Notice("Ateliet was updated. Click here to restart now; otherwise the new version starts next time.", 0);
    n.noticeEl.addClass("mod-ateliet-update-ready");
    n.noticeEl.addEventListener("click", async () => {
      await flush().catch(console.error);
      invoke("update_restart");
    });
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
    new Setting(c).setName("Update automatically").setDesc("Every half hour Ateliet checks its source. When there are new commits it pulls and rebuilds in the background; the new version starts with the app, or right away if you choose to restart.").addToggle((t) => t.setValue(autoUpdateEnabled()).onChange(setAutoUpdate));
    if (auto.state === "running" || auto.state === "ready") {
      const p = c.createEl("p", { cls: "mod-muted", text: auto.state === "ready" ? "An update was built in the background. Restart Ateliet to use it." : `Updating in the background: ${auto.step || "starting"}…` });
      if (auto.state === "ready") {
        const b = this.modalEl.createDiv("modal-button-container").createEl("button", { cls: "mod-cta", text: "Restart now" });
        b.addEventListener("click", () => invoke("update_restart"));
      } else {
        this.unlisten = await listen("update-progress", (e: Progress) => {
          if (e.kind === "step") p.setText(`Updating in the background: ${e.text}…`);
          else if (e.kind === "done") p.setText("An update was built in the background. Restart Ateliet to use it.");
          else if (e.kind === "error") p.setText(`The update failed:\n${e.text}`);
        });
      }
      return;
    }
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
