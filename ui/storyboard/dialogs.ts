// Creating a storyboard and changing one later: name, folder, canvas size
// (presets for film, video, print and Pictogrep's aspects, or any size),
// an infinite canvas instead, paper colour and frame rate.

import { PAPER, SIZE_PRESETS } from "./model.ts";
import { Modal, Setting } from "../obsidian/ui.ts";

export interface BoardOptions {
  name: string;
  folder: string;
  width: number;
  height: number;
  infinite: boolean;
  background: string;
  fps: number;
  /** When resizing an existing canvas: where the old picture stays. */
  anchor: "center" | "top-left" | "top" | "left";
}

const PAPERS: Record<string, string> = { [PAPER]: "Pictogrep paper", "#ffffff": "White", "#f4efe6": "Cream", "#1e1e1e": "Dark", transparent: "Transparent", custom: "Other colour…" };

export class BoardOptionsModal extends Modal {
  private o: BoardOptions;
  private resolve!: (o: BoardOptions | null) => void;
  result: Promise<BoardOptions | null>;
  private done = false;

  constructor(app: any, initial: Partial<BoardOptions>, private mode: "new" | "edit") {
    super(app);
    this.o = { name: "Storyboard", folder: "Storyboards", width: 1920, height: 1080, infinite: false, background: PAPER, fps: 24, anchor: "center", ...initial };
    this.result = new Promise((r) => (this.resolve = r));
  }

  onOpen() {
    const o = this.o;
    this.setTitle(this.mode === "new" ? "New storyboard" : "Storyboard settings");
    this.modalEl.addClass("sb-options-modal");
    const el = this.contentEl;
    new Setting(el).setName("Name").addText((t) => {
      t.setValue(o.name).onChange((v) => (o.name = v));
      t.inputEl.style.width = "100%";
      setTimeout(() => t.inputEl.select(), 0);
    });
    if (this.mode === "new") new Setting(el).setName("Folder").setDesc("In the vault; created if missing.").addText((t) => t.setValue(o.folder).onChange((v) => (o.folder = v.trim().replace(/^\/+|\/+$/g, ""))));

    new Setting(el).setName("Canvas").setHeading();
    new Setting(el)
      .setName("Infinite canvas")
      .setDesc("No edges: draw anywhere, pan as far as you like. Frames are exported at the size of what was drawn. The size below is only where the view starts and where guides show.")
      .addToggle((t) => t.setValue(o.infinite).onChange((v) => {
        o.infinite = v;
        anchorRow.settingEl.toggle(this.mode === "edit" && !v);
      }));
    const preset = SIZE_PRESETS.find((p) => p.width === o.width && p.height === o.height)?.id || "custom";
    let wText: any, hText: any;
    new Setting(el).setName("Size").addDropdown((d) => {
      for (const p of SIZE_PRESETS) d.addOption(p.id, p.name);
      d.addOption("custom", "Custom size");
      d.setValue(preset).onChange((v) => {
        const p = SIZE_PRESETS.find((x) => x.id === v);
        if (!p) return;
        o.width = p.width;
        o.height = p.height;
        wText.setValue(String(p.width));
        hText.setValue(String(p.height));
      });
    });
    const size = new Setting(el).setName("Width × height (pixels)");
    size.addText((t) => {
      wText = t;
      t.inputEl.type = "number";
      t.inputEl.style.width = "90px";
      t.setValue(String(o.width)).onChange((v) => (o.width = clamp(v)));
    });
    size.controlEl.createSpan({ text: " × " });
    size.addText((t) => {
      hText = t;
      t.inputEl.type = "number";
      t.inputEl.style.width = "90px";
      t.setValue(String(o.height)).onChange((v) => (o.height = clamp(v)));
    });
    size.addExtraButton((b) => b.setIcon("arrow-left-right").setTooltip("Swap (portrait / landscape)").onClick(() => {
      [o.width, o.height] = [o.height, o.width];
      wText.setValue(String(o.width));
      hText.setValue(String(o.height));
    }));
    const anchorRow = new Setting(el).setName("When the size changes, keep the drawing at").addDropdown((d) => d.addOptions({ center: "The centre", "top-left": "The top left corner", top: "The top", left: "The left" }).setValue(o.anchor).onChange((v) => (o.anchor = v as any)));
    anchorRow.settingEl.toggle(this.mode === "edit" && !o.infinite);

    const known = o.background in PAPERS ? o.background : "custom";
    const bgRow = new Setting(el).setName("Paper").addDropdown((d) => d.addOptions(PAPERS).setValue(known).onChange((v) => {
      if (v !== "custom") o.background = v;
      picker.toggle(v === "custom");
    }));
    const picker = bgRow.controlEl.createEl("input", { type: "color" });
    picker.value = /^#[0-9a-f]{6}$/i.test(o.background) ? o.background : "#ffffff";
    picker.toggle(known === "custom");
    picker.addEventListener("input", () => (o.background = picker.value));

    new Setting(el).setName("Timeline").setHeading();
    new Setting(el).setName("Frames per second").setDesc("24 for film, 25 or 30 for video, 12 for animation on twos.").addDropdown((d) => {
      for (const v of [8, 12, 15, 24, 25, 30, 48, 50, 60]) d.addOption(String(v), String(v));
      if (![8, 12, 15, 24, 25, 30, 48, 50, 60].includes(o.fps)) d.addOption(String(o.fps), String(o.fps));
      d.setValue(String(o.fps)).onChange((v) => (o.fps = Number(v)));
    });

    const buttons = this.modalEl.createDiv("modal-button-container");
    buttons.createEl("button", { text: this.mode === "new" ? "Create" : "Apply", cls: "mod-cta" }).addEventListener("click", () => {
      this.done = true;
      o.name = o.name.trim() || "Storyboard";
      this.resolve({ ...o });
      this.close();
    });
    buttons.createEl("button", { text: "Cancel" }).addEventListener("click", () => this.close());
  }

  onClose() {
    if (!this.done) this.resolve(null);
    this.contentEl.empty();
  }
}

function clamp(v: string): number {
  return Math.max(16, Math.min(16384, Math.round(Number(v)) || 16));
}
