// Creating a storyboard and changing one later. First the kind (animatic,
// presentation board, thumbnails, name, comic, webtoon), then what that
// kind needs: canvas size and frame rate; page size, resolution, reading
// direction and spreads; cell shape; strip width and length.

import { BoardKind, PAPER, SIZE_PRESETS, isPageKind } from "./model.ts";
import { KINDS, KindOptions, PAGE_SIZES, THUMB_ASPECTS, WEBTOON_WIDTHS, defaultKindOptions } from "./kinds.ts";
import { LAYOUTS } from "./pages.ts";
import { setIcon } from "../obsidian/icons.ts";
import { Modal, Setting } from "../obsidian/ui.ts";

export interface BoardOptions extends KindOptions {
  name: string;
  folder: string;
  /** When resizing an existing canvas: where the old picture stays. */
  anchor: "center" | "top-left" | "top" | "left";
}

const PAPERS: Record<string, string> = { [PAPER]: "Pictogrep paper", "#ffffff": "White", "#f4efe6": "Cream", "#1e1e1e": "Dark", transparent: "Transparent", custom: "Other colour…" };

export class BoardOptionsModal extends Modal {
  private o: BoardOptions;
  private resolve!: (o: BoardOptions | null) => void;
  result: Promise<BoardOptions | null>;
  private done = false;
  private body!: HTMLElement;
  private initialKind: BoardKind;

  constructor(app: any, initial: Partial<BoardOptions>, private mode: "new" | "edit") {
    super(app);
    const kind = (initial.kind || "animatic") as BoardKind;
    this.initialKind = kind;
    this.o = { ...defaultKindOptions(kind), name: "Storyboard", folder: "Storyboards", anchor: "center", ...initial } as BoardOptions;
    this.result = new Promise((r) => (this.resolve = r));
  }

  onOpen() {
    this.setTitle(this.mode === "new" ? "New storyboard" : "Storyboard settings");
    this.modalEl.addClass("sb-options-modal");
    const el = this.contentEl;
    new Setting(el).setName("Name").addText((t) => {
      t.setValue(this.o.name).onChange((v) => (this.o.name = v));
      t.inputEl.style.width = "100%";
      setTimeout(() => t.inputEl.select(), 0);
    });
    if (this.mode === "new") new Setting(el).setName("Folder").setDesc("In the vault; created if missing.").addText((t) => t.setValue(this.o.folder).onChange((v) => (this.o.folder = v.trim().replace(/^\/+|\/+$/g, ""))));
    const kinds = el.createDiv("sb-kinds");
    const paint = () => {
      kinds.querySelectorAll(".sb-kind").forEach((c: any) => c.toggleClass("is-active", c.dataset.kind === this.o.kind));
    };
    for (const k of KINDS) {
      const card = kinds.createDiv({ cls: "sb-kind", attr: { "data-kind": k.id, tabindex: "0" } });
      setIcon(card.createDiv("sb-kind-icon"), k.icon);
      card.createDiv({ cls: "sb-kind-name", text: k.name });
      card.createDiv({ cls: "sb-kind-blurb", text: k.blurb });
      card.addEventListener("click", () => {
        if (this.o.kind === k.id) return;
        // A new kind brings its own defaults (in edit mode, only the workspace changes).
        const keep = { name: this.o.name, folder: this.o.folder, anchor: this.o.anchor };
        this.o = this.mode === "new" ? ({ ...defaultKindOptions(k.id), ...keep } as BoardOptions) : { ...this.o, kind: k.id };
        paint();
        this.renderBody();
      });
    }
    paint();
    this.body = el.createDiv();
    this.renderBody();
    const buttons = this.modalEl.createDiv("modal-button-container");
    buttons.createEl("button", { text: this.mode === "new" ? "Create" : "Apply", cls: "mod-cta" }).addEventListener("click", () => {
      this.done = true;
      this.o.name = this.o.name.trim() || "Storyboard";
      this.resolve({ ...this.o });
      this.close();
    });
    buttons.createEl("button", { text: "Cancel" }).addEventListener("click", () => this.close());
  }

  private renderBody() {
    const el = this.body;
    const o = this.o;
    el.empty();
    const kind = o.kind;
    if (kind === "animatic" || kind === "presentation") this.canvasOptions(el);
    else if (kind === "name" || kind === "comic") {
      if (this.mode === "new") {
        new Setting(el).setName("Page size").setDesc("Trim size; bleed and the inner frame (where panels and text belong) come with it, and can be changed later.").addDropdown((d) => {
          for (const p of PAGE_SIZES[kind]) d.addOption(p.id, p.name);
          d.setValue(PAGE_SIZES[kind].some((p) => p.id === o.pageSize) ? o.pageSize : PAGE_SIZES[kind][0].id).onChange((v) => (o.pageSize = v));
          o.pageSize = d.getValue();
        });
        new Setting(el).setName("Resolution").setDesc(kind === "name" ? "A name is rough: 150 dpi is plenty and stays fast." : "150 for planning; 300–600 if these pages will be finished here.").addDropdown((d) => d.addOptions({ "150": "150 dpi", "300": "300 dpi", "350": "350 dpi", "600": "600 dpi" }).setValue(String(o.dpi)).onChange((v) => (o.dpi = Number(v))));
        new Setting(el).setName("Pages").addDropdown((d) => {
          for (const n of [1, 2, 4, 8, 16, 24, 32]) d.addOption(String(n), String(n));
          d.setValue(String(o.pages)).onChange((v) => (o.pages = Number(v)));
        });
        new Setting(el).setName("Panels to start from").setDesc("One panel to cut with the panel tool, or a layout.").addDropdown((d) => {
          for (const l of LAYOUTS) d.addOption(l.id, l.name);
          d.setValue(o.layout).onChange((v) => (o.layout = v));
        });
      }
      new Setting(el).setName("Reading direction").addDropdown((d) => d.addOptions({ rtl: "Right to left (manga)", ltr: "Left to right" }).setValue(o.rtl ? "rtl" : "ltr").onChange((v) => (o.rtl = v === "rtl")));
      new Setting(el).setName("Spreads").setDesc("Facing pages side by side (page 1 alone, then pairs); click the other page to draw on it.").addToggle((t) => t.setValue(o.spreads).onChange((v) => (o.spreads = v)));
      this.paper(el);
    } else if (kind === "thumbnails") {
      if (this.mode === "new") {
        new Setting(el).setName("Cell shape").setDesc("The frame of what is being planned.").addDropdown((d) => {
          for (const [v, n] of THUMB_ASPECTS) d.addOption(v, n);
          d.setValue(o.aspect).onChange((v) => (o.aspect = v));
        });
        new Setting(el).setName("Sheet").addDropdown((d) => d.addOptions({ landscape: "A4 landscape", portrait: "A4 portrait" }).setValue(o.landscape ? "landscape" : "portrait").onChange((v) => (o.landscape = v === "landscape")));
        new Setting(el).setName("Sheets").addDropdown((d) => {
          for (const n of [1, 2, 4, 8]) d.addOption(String(n), String(n));
          d.setValue(String(o.pages)).onChange((v) => (o.pages = Number(v)));
        });
      }
      this.paper(el);
    } else if (kind === "webtoon") {
      if (this.mode === "new") {
        new Setting(el).setName("Strip width").addDropdown((d) => {
          for (const [w, n] of WEBTOON_WIDTHS) d.addOption(String(w), n);
          d.setValue(String(o.stripWidth)).onChange((v) => (o.stripWidth = Number(v)));
        });
        new Setting(el).setName("Length").setDesc("In upload slices of 1280 px; it grows as you add panels.").addDropdown((d) => {
          for (const n of [5, 10, 20, 40]) d.addOption(String(n), `${n} slices (${n * 1280} px)`);
          d.setValue(String(o.slices)).onChange((v) => (o.slices = Number(v)));
        });
      }
      this.paper(el);
    }
    if (this.mode === "edit" && isPageKind(kind) !== isPageKind(this.initialKind)) {
      el.createDiv({ cls: "sb-hint sb-kind-warning", text: "Changing between frames and pages keeps every drawing; panels and balloons only show on pages." });
    }
  }

  private canvasOptions(el: HTMLElement) {
    const o = this.o;
    new Setting(el)
      .setName("Infinite canvas")
      .setDesc("No edges: draw anywhere. Frames are exported at the size of what was drawn; the size below is where the view starts.")
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
    this.paper(el);
    new Setting(el).setName("Frames per second").setDesc("24 for film, 25 or 30 for video, 12 for animation on twos.").addDropdown((d) => {
      for (const v of [8, 12, 15, 24, 25, 30, 48, 50, 60]) d.addOption(String(v), String(v));
      if (![8, 12, 15, 24, 25, 30, 48, 50, 60].includes(o.fps)) d.addOption(String(o.fps), String(o.fps));
      d.setValue(String(o.fps)).onChange((v) => (o.fps = Number(v)));
    });
  }

  private paper(el: HTMLElement) {
    const o = this.o;
    const known = o.background in PAPERS ? o.background : "custom";
    const row = new Setting(el).setName("Paper").addDropdown((d) => d.addOptions(PAPERS).setValue(known).onChange((v) => {
      if (v !== "custom") o.background = v;
      picker.toggle(v === "custom");
    }));
    const picker = row.controlEl.createEl("input", { type: "color" });
    picker.value = /^#[0-9a-f]{6}$/i.test(o.background) ? o.background : "#ffffff";
    picker.toggle(known === "custom");
    picker.addEventListener("input", () => (o.background = picker.value));
  }

  onClose() {
    if (!this.done) this.resolve(null);
    this.contentEl.empty();
  }
}

function clamp(v: string): number {
  return Math.max(16, Math.min(16384, Math.round(Number(v)) || 16));
}
