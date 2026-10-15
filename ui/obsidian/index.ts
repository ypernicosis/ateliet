// The `obsidian` module handed to plugins via `require("obsidian")`.

export * from "./events.ts";
export * from "./util.ts";
export * from "./icons.ts";
export * from "./vault.ts";
export * from "./metadata.ts";
export * from "./filemanager.ts";
export * from "./ui.ts";
export * from "./editor.ts";
export * from "./renderer.ts";
export * from "./views.ts";
export * from "./workspace.ts";
export * from "./plugin.ts";
export { editorLivePreviewField } from "../editor/livepreview.ts";
export { App } from "../app.ts";

import { ViewPlugin } from "@codemirror/view";

/** Present for plugins that `import { livePreviewState }`. */
export const livePreviewState = ViewPlugin.define(() => ({ mousedown: false }));

export enum PopoverState {
  Showing = 0,
  Shown = 1,
  Hiding = 2,
  Hidden = 3,
}

// Bases API (Obsidian 1.9+) — present so imports resolve; views are not supported.
export class Value {
  toString() {
    return "";
  }
  isTruthy() {
    return false;
  }
}
export class NullValue extends Value {}
export class NotNullValue extends Value {}
export class PrimitiveValue<T = any> extends NotNullValue {
  constructor(public value: T) {
    super();
  }
  toString() {
    return String(this.value);
  }
  isTruthy() {
    return !!this.value;
  }
}
export class StringValue extends PrimitiveValue<string> {}
export class NumberValue extends PrimitiveValue<number> {}
export class BooleanValue extends PrimitiveValue<boolean> {}
export class DateValue extends PrimitiveValue<Date> {}
export class DurationValue extends NotNullValue {}
export class RelativeDateValue extends DateValue {}
export class LinkValue extends StringValue {}
export class UrlValue extends StringValue {}
export class TagValue extends StringValue {}
export class FileValue extends NotNullValue {}
export class ImageValue extends StringValue {}
export class IconValue extends StringValue {}
export class HTMLValue extends StringValue {}
export class RegExpValue extends NotNullValue {}
export class ListValue extends NotNullValue {}
export class ObjectValue extends NotNullValue {}
export class BasesView {}
export class BasesEntry {}
export class BasesEntryGroup {}
export class BasesQueryResult {}
export class BasesViewConfig {}
export class QueryController {}
export class RenderContext {}
export class DisplayValueComponent {}
export class Tasks {}
export class SecretStorage {
  getSecret() {
    return null;
  }
  setSecret() {}
  listSecrets() {
    return [];
  }
}
export class TextComponent2 {}

export function parsePropertyId(id: string) {
  const i = id.indexOf(".");
  return i === -1 ? { type: "note", name: id } : { type: id.slice(0, i), name: id.slice(i + 1) };
}
