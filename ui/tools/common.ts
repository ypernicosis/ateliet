// Shared helpers for the built-in tools (Dataview, Templater, Calendar).

/** Community plugins now built into Ateliet: never loaded from .obsidian/plugins. */
export const BUILT_IN_PLUGINS: Record<string, string> = {
  dataview: "Dataview",
  "templater-obsidian": "Templater",
  calendar: "Calendar",
};

/** Settings live in `.obsidian/ateliet/<id>.json`; the first time, the old plugin's data.json is read. */
export async function loadToolSettings<T extends object>(app: any, id: string, pluginId: string, defaults: T): Promise<T> {
  const own = await app.readJson(`ateliet/${id}.json`, null);
  const legacy = own ? null : await app.readJson(`plugins/${pluginId}/data.json`, null);
  return { ...defaults, ...(legacy || {}), ...(own || {}) };
}

export function saveToolSettings(app: any, id: string, data: any): Promise<void> {
  return app.writeJson(`ateliet/${id}.json`, data);
}

export function vaultPath(app: any): string {
  return app.vault.adapter.basePath;
}

/** The interface language for month and weekday names. */
export function locale(): string {
  return (navigator.language || "en").toLowerCase();
}
