// Vault chooser: lists vaults known to Ateliet and to Obsidian.

import { setIcon } from "./obsidian/icons.ts";
import { invoke } from "./tauri.ts";

export async function showLauncher(error?: string) {
  document.title = "Ateliet";
  document.body.empty();
  document.body.addClass("is-launcher");
  const root = document.body.createDiv("ateliet-launcher");
  const side = root.createDiv("launcher-vaults");
  const main = root.createDiv("launcher-main");

  const brand = main.createDiv("launcher-brand");
  brand.createEl("img", { attr: { src: "icon.png", alt: "" } });
  brand.createEl("h1", { cls: "launcher-wordmark", attr: { "aria-label": "ateliet" } });
  brand.createDiv({ cls: "launcher-tagline", text: "a minimal workshop for your obsidian vaults." });
  if (error) main.createDiv({ cls: "launcher-error", text: error });

  const actions = main.createDiv("launcher-actions");
  const action = (title: string, desc: string, button: string, cta: boolean, cb: () => any) => {
    const row = actions.createDiv("launcher-action");
    const t = row.createDiv("launcher-action-text");
    t.createDiv({ cls: "launcher-action-title", text: title });
    t.createDiv({ cls: "launcher-action-desc", text: desc });
    const b = row.createEl("button", { text: button, cls: cta ? "mod-cta" : "" });
    b.addEventListener("click", cb);
  };
  action("Open folder as vault", "Choose an existing folder of Markdown files — including an Obsidian vault.", "Open", true, async () => {
    const p = await invoke<string | null>("pick_folder");
    if (p) open(p);
  });
  action("Create new vault", "Create a new, empty vault inside a folder.", "Create", false, async () => {
    const parent = await invoke<string | null>("pick_folder");
    if (!parent) return;
    const name = prompt("Vault name", "My vault");
    if (!name) return;
    const p = await invoke<string>("create_vault", { parent, name });
    open(p);
  });

  const { vaults } = await invoke<any>("known_vaults");
  side.createDiv({ cls: "launcher-vaults-title", text: "Vaults" });
  if (!vaults.length) side.createDiv({ cls: "launcher-empty", text: "No vaults yet." });
  for (const v of vaults) {
    const row = side.createDiv("launcher-vault");
    const text = row.createDiv("launcher-vault-text");
    text.createDiv({ cls: "launcher-vault-name", text: v.name });
    text.createDiv({ cls: "launcher-vault-path", text: v.path });
    if (v.source === "obsidian") {
      const badge = row.createDiv({ cls: "launcher-vault-badge", attr: { "aria-label": "Known to Obsidian" } });
      setIcon(badge, "gem");
    }
    const more = row.createDiv({ cls: "clickable-icon launcher-vault-remove", attr: { "aria-label": "Remove from list" } });
    setIcon(more, "x");
    more.addEventListener("click", async (e) => {
      e.stopPropagation();
      await invoke("forget_vault", { path: v.path });
      row.remove();
    });
    row.addEventListener("click", () => open(v.path));
  }
}

async function open(path: string) {
  try {
    const url = await invoke<string>("open_vault", { path, newWindow: false });
    location.href = url.startsWith("index.html") ? url : "index.html" + url;
  } catch (e) {
    showLauncher(String(e));
  }
}
