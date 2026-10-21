// The built-in tools that replaced community plugins. Each one is a core
// plugin of Ateliet; the old plugins with the same ids are no longer loaded.

import { CalendarTool } from "./calendar.ts";
import { DataviewTool } from "./dataview.ts";
import { TemplaterTool } from "./templater.ts";

export { BUILT_IN_PLUGINS } from "./common.ts";

export function registerTools(app: any, reg: (id: string, name: string, inst: any, description?: string, defaultOn?: boolean) => void) {
  const dataview = new DataviewTool();
  const templater = new TemplaterTool();
  const calendar = new CalendarTool();
  reg("dataview", "Dataview", dataview, "Queries over your notes: dataview and dataviewjs blocks, inline queries.");
  reg("templater", "Templater", templater, "Templates with <% %> commands and scripts.");
  reg("calendar", "Calendar", calendar, "A month of daily notes in the sidebar.");
  const s = app.setting;
  s.addCorePluginTab("dataview", "Dataview", "database", dataview.settingsTab());
  s.addCorePluginTab("templater", "Templater", "layout-template", templater.settingsTab());
  s.addCorePluginTab("calendar", "Calendar", "calendar-check", calendar.settingsTab());
}
