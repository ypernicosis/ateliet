// The calendar, built in: a month of daily notes in the right sidebar. The
// grid, the note paths and the word counts come from Rust (src/calendar.rs);
// daily notes are created through the daily notes core plugin, so the folder,
// format and template are the same ones it uses.

import { setIcon } from "../obsidian/icons.ts";
import { Keymap, Modal, Setting } from "../obsidian/ui.ts";
import { debounce, moment } from "../obsidian/util.ts";
import { TFile } from "../obsidian/vault.ts";
import { ItemView } from "../obsidian/views.ts";
import { invoke } from "../tauri.ts";
import { loadToolSettings, locale, saveToolSettings, vaultPath } from "./common.ts";

export const CALENDAR_DEFAULTS = {
  shouldConfirmBeforeCreate: true,
  weekStart: "locale",
  wordsPerDot: 250,
  showWeeklyNote: false,
  localeOverride: "system-default",
};

type Options = typeof CALENDAR_DEFAULTS;

export const CALENDAR_VIEW = "calendar";
const WEEKDAYS = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

class ConfirmCreate extends Modal {
  constructor(app: any, private name: string, private onYes: () => void) {
    super(app);
  }
  onOpen() {
    this.titleEl.setText("New daily note");
    this.contentEl.createEl("p", { text: `The note "${this.name}" does not exist. Create it now?` });
    new Setting(this.contentEl)
      .addButton((b) => b.setButtonText("Never mind").onClick(() => this.close()))
      .addButton((b) =>
        b.setButtonText("Create").setCta().onClick(() => {
          this.close();
          this.onYes();
        }),
      );
  }
}

export class CalendarView extends ItemView {
  tool: CalendarTool;
  private shown = moment().startOf("month");
  private refresh = debounce(() => this.draw(), 400, true);
  private today = moment().format("YYYY-MM-DD");

  constructor(leaf: any, tool: CalendarTool) {
    super(leaf);
    this.tool = tool;
  }

  getViewType() {
    return CALENDAR_VIEW;
  }
  getDisplayText() {
    return "Calendar";
  }
  getIcon() {
    return "calendar-check";
  }

  async onOpen() {
    this.headerEl.hide();
    this.contentEl.addClass("ateliet-calendar");
    const v = this.app.vault;
    for (const ev of ["create", "delete", "rename", "modify"]) this.registerEvent(v.on(ev, () => this.refresh()));
    this.registerEvent(this.app.workspace.on("file-open", () => this.markActive()));
    // a new day starts: move the "today" mark
    this.registerInterval(window.setInterval(() => {
      const now = moment().format("YYYY-MM-DD");
      if (now !== this.today) {
        this.today = now;
        this.draw();
      }
    }, 60_000));
    await this.draw();
  }

  private dailyOptions(): any {
    return this.app.internalPlugins.getPluginById("daily-notes")?.instance?.options || {};
  }

  async draw() {
    const opts = this.tool.options;
    const daily = this.dailyOptions();
    const ws = WEEKDAYS.indexOf(String(opts.weekStart).toLowerCase());
    let data: any;
    try {
      data = await invoke("calendar_month", {
        vault: vaultPath(this.app),
        request: {
          year: this.shown.year(),
          month: this.shown.month() + 1,
          format: daily.format || "YYYY-MM-DD",
          folder: daily.folder || "",
          weekStart: ws >= 0 ? ws : null,
          wordsPerDot: Number(opts.wordsPerDot) || 250,
          locale: this.tool.locale(),
        },
      });
    } catch (e) {
      console.error("calendar", e);
      return;
    }
    const el = this.contentEl;
    el.empty();
    const box = el.createDiv({ cls: "container", attr: { id: "calendar-container" } });
    const nav = box.createDiv("nav");
    const title = nav.createEl("h3", { cls: "title" });
    title.createSpan({ cls: "month", text: data.month.charAt(0).toUpperCase() + data.month.slice(1) });
    title.appendText(" ");
    title.createSpan({ cls: "year", text: String(data.year) });
    const right = nav.createDiv("right-nav");
    const prev = right.createDiv({ cls: "arrow", attr: { "aria-label": "Previous month" } });
    setIcon(prev, "chevron-left");
    prev.addEventListener("click", () => this.move(-1));
    const reset = right.createDiv({ cls: "reset-button", text: this.tool.locale().startsWith("pt") ? "Hoje" : "Today" });
    reset.addEventListener("click", () => {
      this.shown = moment().startOf("month");
      this.draw();
    });
    const next = right.createDiv({ cls: "arrow", attr: { "aria-label": "Next month" } });
    setIcon(next, "chevron-right");
    next.addEventListener("click", () => this.move(1));

    const table = box.createEl("table", { cls: "calendar" });
    const cols = table.createEl("colgroup");
    if (opts.showWeeklyNote) cols.createEl("col");
    for (let i = 0; i < 7; i++) cols.createEl("col");
    const head = table.createEl("thead").createEl("tr");
    if (opts.showWeeklyNote) head.createEl("th", { text: "W" });
    for (const w of data.weekdays) head.createEl("th", { text: w });
    const body = table.createEl("tbody");
    for (const week of data.weeks) {
      const tr = body.createEl("tr");
      if (opts.showWeeklyNote) tr.createEl("td", { cls: "week-num", text: String(week.week) });
      for (const d of week.days) {
        const td = tr.createEl("td");
        const day = td.createDiv({ cls: "day", text: String(d.day), attr: { "data-date": d.date, "data-path": d.path } });
        if (!d.inMonth) day.addClass("adjacent-month");
        if (d.today) day.addClass("today");
        if (d.exists) day.addClass("has-note");
        if (d.weekend) day.addClass("weekend");
        const dots = day.createDiv("dot-container");
        for (let i = 0; i < d.dots; i++) dots.createDiv("dot filled");
        if (d.openTasks) dots.createDiv("dot hollow task");
        day.addEventListener("click", (e) => this.open(d, Keymap.isModEvent(e)));
        day.addEventListener("mouseover", (e) => {
          if (!d.exists) return;
          this.app.workspace.trigger("hover-link", { event: e, source: CALENDAR_VIEW, hoverParent: this, targetEl: day, linktext: d.path, sourcePath: "" });
        });
      }
    }
    this.markActive();
  }

  private markActive() {
    const active = this.app.workspace.getActiveFile()?.path;
    for (const d of Array.from(this.contentEl.querySelectorAll<HTMLElement>(".day"))) d.toggleClass("active", !!active && d.dataset.path === active);
  }

  private move(n: number) {
    this.shown = this.shown.clone().add(n, "month");
    this.draw();
  }

  private async open(d: any, newLeaf: boolean) {
    const existing = this.app.vault.getAbstractFileByPath(d.path);
    const openFile = async (f: TFile) => this.app.workspace.getLeaf(newLeaf).openFile(f);
    if (existing instanceof TFile) return openFile(existing);
    const create = async () => {
      const dn = this.app.internalPlugins.getPluginById("daily-notes")?.instance;
      if (!dn) return;
      if (!dn.app) {
        // the daily notes plugin is off: still use its settings
        dn.app = this.app;
        dn.options = await this.app.readJson("daily-notes.json", {});
      }
      const f = await dn.getDailyNote(moment(d.date, "YYYY-MM-DD"), true);
      if (f) await openFile(f);
    };
    if (this.tool.options.shouldConfirmBeforeCreate) new ConfirmCreate(this.app, d.path.split("/").pop().replace(/\.md$/, ""), create).open();
    else create();
  }
}

export class CalendarTool {
  app: any;
  plugin: any;
  options: Options = { ...CALENDAR_DEFAULTS };

  async onEnable(app: any) {
    this.app = app;
    this.options = await loadToolSettings(app, "calendar", "calendar", CALENDAR_DEFAULTS);
    app.workspace.viewRegistry.registerView(CALENDAR_VIEW, (leaf: any) => new CalendarView(leaf, this));
    app.commands.addCommand({ id: "calendar:show-calendar-view", name: "Calendar: Open view", icon: "calendar-check", callback: () => app.workspace.ensureSideLeaf(CALENDAR_VIEW, "right", { reveal: true }) });
    app.commands.addCommand({
      id: "calendar:reveal-active-note",
      name: "Calendar: Reveal open note",
      callback: () => {
        const f = app.workspace.getActiveFile();
        const leaf = app.workspace.getLeavesOfType(CALENDAR_VIEW)[0];
        if (!f || !leaf) return;
        const daily = app.internalPlugins.getPluginById("daily-notes")?.instance?.options || {};
        const folder = String(daily.folder || "").replace(/^\/+|\/+$/g, "");
        const rel = (folder && f.path.startsWith(folder + "/") ? f.path.slice(folder.length + 1) : f.path).replace(/\.md$/, "");
        const m = moment(rel, daily.format || "YYYY-MM-DD", true);
        if (m.isValid()) {
          (leaf.view as any).shown = m.startOf("month");
          (leaf.view as any).draw();
        }
      },
    });
    app.workspace.onLayoutReady(() => {
      if (!app.workspace.getLeavesOfType(CALENDAR_VIEW).length) app.workspace.ensureSideLeaf(CALENDAR_VIEW, "right", { reveal: false });
    });
  }

  onDisable() {
    for (const leaf of this.app.workspace.getLeavesOfType(CALENDAR_VIEW)) leaf.detach();
  }

  /** The calendar's language: its own setting, or the system's. */
  locale(): string {
    const o = String(this.options.localeOverride || "");
    return o && o !== "system-default" ? o.toLowerCase() : locale();
  }

  redraw() {
    for (const leaf of this.app.workspace.getLeavesOfType(CALENDAR_VIEW)) (leaf.view as any).draw?.();
  }

  settingsTab() {
    return (el: HTMLElement) => {
      const o = this.options;
      const save = () => {
        saveToolSettings(this.app, "calendar", o);
        this.redraw();
      };
      el.createDiv({ cls: "setting-item-description", text: "The calendar opens and creates daily notes with the folder, format and template of the Daily notes settings." });
      new Setting(el).setName("Start week on").addDropdown((d) => {
        d.addOption("locale", "Locale default");
        WEEKDAYS.forEach((w) => d.addOption(w, w.charAt(0).toUpperCase() + w.slice(1)));
        d.setValue(String(o.weekStart)).onChange((v) => { o.weekStart = v; save(); });
      });
      new Setting(el).setName("Language").setDesc("Month and weekday names.").addDropdown((d) => {
        d.addOption("system-default", "System default");
        d.addOption("en", "English");
        d.addOption("pt-br", "Português");
        d.setValue(String(o.localeOverride || "system-default")).onChange((v) => { o.localeOverride = v; save(); });
      });
      new Setting(el).setName("Words per dot").setDesc("Each dot under a day stands for this many words (up to 5 dots).").addText((t) => t.setValue(String(o.wordsPerDot)).onChange((v) => { o.wordsPerDot = Number(v) || 250; save(); }));
      new Setting(el).setName("Confirm before creating a note").addToggle((t) => t.setValue(o.shouldConfirmBeforeCreate).onChange((v) => { o.shouldConfirmBeforeCreate = v; save(); }));
      new Setting(el).setName("Show week numbers").addToggle((t) => t.setValue(o.showWeeklyNote).onChange((v) => { o.showWeeklyNote = v; save(); }));
    };
  }
}
