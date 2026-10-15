// Registers Ateliet's built-in (core) plugins and their settings tabs.

import { FileExplorerPlugin } from "./explorer.ts";
import { CommandPalettePlugin, GlobalSearchPlugin, SwitcherPlugin } from "./search.ts";
import { BacklinkView, BookmarksPlugin, OutgoingLinkView, OutlineView, TagView, simplePanePlugin } from "./panes.ts";
import { BacklinkCountStatus, DailyNotesPlugin, RandomNotePlugin, TemplatesPlugin, WordCountPlugin } from "./notes.ts";
import { LinkSuggest, TagSuggest, registerCoreCommands } from "./commands.ts";
import { Setting } from "../obsidian/ui.ts";
import { installPagePreview } from "../obsidian/renderer.ts";
import { registerFileViews } from "./media.ts";
import { ImagesPlugin, imagesSettingsTab } from "../images/index.ts";

export function registerCorePlugins(app: any) {
  registerFileViews(app);
  const ip = app.internalPlugins;
  const reg = (id: string, name: string, inst: any, description = "", defaultOn = true) => {
    inst.description = description;
    ip.register(id, name, inst, defaultOn);
  };
  reg("file-explorer", "Files", new FileExplorerPlugin(), "See all files in your vault.");
  reg("global-search", "Search", new GlobalSearchPlugin(), "Search for keywords in all files.");
  reg("switcher", "Quick switcher", new SwitcherPlugin(), "Jump to any file with your keyboard.");
  reg("command-palette", "Command palette", new CommandPalettePlugin(), "Run any action from your keyboard.");
  reg("backlink", "Backlinks", simplePanePlugin("backlink", "backlink", "Backlinks", "right", "links-coming-in", BacklinkView), "Show links to the active note.");
  reg("outgoing-link", "Outgoing links", simplePanePlugin("outgoing-links", "outgoing-link", "Outgoing links", "right", "links-going-out", OutgoingLinkView), "Show links from the active note.");
  reg("tag-pane", "Tags view", simplePanePlugin("tag-pane", "tag", "Tags", "right", "tags", TagView), "Show all tags with counts.");
  reg("outline", "Outline", simplePanePlugin("outline", "outline", "Outline", "right", "list", OutlineView), "Show headings of the active note.");
  reg("bookmarks", "Bookmarks", new BookmarksPlugin(), "Bookmark files and searches (shared with Obsidian).");
  reg("page-preview", "Page preview", { onEnable: (a: any) => installPagePreview(a) }, "Preview notes when hovering links.");
  reg("daily-notes", "Daily notes", new DailyNotesPlugin(), "Open today's note.");
  reg("templates", "Templates", new TemplatesPlugin(), "Insert templates into the active note.");
  reg("word-count", "Word count", new WordCountPlugin(), "Show word and character counts in the status bar.");
  reg("random-note", "Random note", new RandomNotePlugin(), "Open a random note.", false);
  reg("backlink-count", "Backlink count", Object.assign(new BacklinkCountStatus(), { hidden: true }), "", true);
  reg("images", "Images", new ImagesPlugin(DEFAULT_LAYOUT), "The picture library: image mode, search by description, pictures in notes.");

  registerCoreCommands(app);
  app.workspace.editorSuggest.addSuggest(new LinkSuggest(app));
  app.workspace.editorSuggest.addSuggest(new TagSuggest(app));

  // option tabs for core plugins (persisted in the same files Obsidian uses)
  const s = app.setting;
  s.addCorePluginTab("daily-notes", "Daily notes", "calendar-days", (el: HTMLElement) => {
    const inst = ip.getPluginById("daily-notes").instance;
    const save = () => app.writeJson("daily-notes.json", inst.options);
    new Setting(el).setName("Date format").addMomentFormat((m) => m.setDefaultFormat("YYYY-MM-DD").setValue(inst.options.format || "").onChange((v) => { inst.options.format = v; save(); }));
    new Setting(el).setName("New file location").addText((t) => t.setPlaceholder("/").setValue(inst.options.folder || "").onChange((v) => { inst.options.folder = v; save(); }));
    new Setting(el).setName("Template file location").addText((t) => t.setValue(inst.options.template || "").onChange((v) => { inst.options.template = v; save(); }));
    new Setting(el).setName("Open daily note on startup").addToggle((t) => t.setValue(!!inst.options.autorun).onChange((v) => { inst.options.autorun = v; save(); }));
  });
  s.addCorePluginTab("images", "Images", "images", imagesSettingsTab(app));
  s.addCorePluginTab("templates", "Templates", "files", (el: HTMLElement) => {
    const inst = ip.getPluginById("templates").instance;
    const save = () => app.writeJson("templates.json", inst.options);
    new Setting(el).setName("Template folder location").addText((t) => t.setValue(inst.options.folder || "").onChange((v) => { inst.options.folder = v; save(); }));
    new Setting(el).setName("Date format").addMomentFormat((m) => m.setDefaultFormat("YYYY-MM-DD").setValue(inst.options.dateFormat || "").onChange((v) => { inst.options.dateFormat = v; save(); }));
    new Setting(el).setName("Time format").addMomentFormat((m) => m.setDefaultFormat("HH:mm").setValue(inst.options.timeFormat || "").onChange((v) => { inst.options.timeFormat = v; save(); }));
  });
}

/** The layout used for a vault opened for the first time. */
export const DEFAULT_LAYOUT = {
  main: { type: "split", direction: "vertical", children: [{ type: "tabs", children: [{ type: "leaf", state: { type: "empty", state: {} } }] }] },
  left: { type: "split", direction: "horizontal", width: 280, children: [{ type: "tabs", children: [
    { type: "leaf", state: { type: "file-explorer", state: {} } },
    { type: "leaf", state: { type: "search", state: {} } },
    { type: "leaf", state: { type: "bookmarks", state: {} } },
  ] }] },
  right: { type: "split", direction: "horizontal", width: 300, collapsed: true, children: [{ type: "tabs", children: [
    { type: "leaf", state: { type: "backlink", state: {} } },
    { type: "leaf", state: { type: "outgoing-link", state: {} } },
    { type: "leaf", state: { type: "tag", state: {} } },
    { type: "leaf", state: { type: "outline", state: {} } },
  ] }] },
};
