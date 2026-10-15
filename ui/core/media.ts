// File views for non-markdown files: images, audio, video, PDF, and a
// fallback for unsupported types.

import { FileView, MarkdownView } from "../obsidian/views.ts";
import { TFile } from "../obsidian/vault.ts";

abstract class MediaView extends FileView {
  async onLoadFile(file: TFile) {
    this.contentEl.empty();
    this.contentEl.addClass("media-view");
    this.render(file, this.app.vault.getResourcePath(file));
  }
  abstract render(file: TFile, url: string): void;
}

export class ImageView extends MediaView {
  getViewType() { return "image"; }
  getIcon() { return "image"; }
  render(_f: TFile, url: string) {
    const wrap = this.contentEl.createDiv("image-container");
    const img = wrap.createEl("img", { attr: { src: url } });
    img.addEventListener("click", () => wrap.toggleClass("is-zoomed", !wrap.hasClass("is-zoomed")));
  }
}

export class AudioView extends MediaView {
  getViewType() { return "audio"; }
  getIcon() { return "file-audio"; }
  render(_f: TFile, url: string) {
    const a = this.contentEl.createDiv("audio-container").createEl("audio", { attr: { src: url, controls: true } }) as HTMLAudioElement;
    a.controls = true;
  }
}

export class VideoView extends MediaView {
  getViewType() { return "video"; }
  getIcon() { return "file-video"; }
  render(_f: TFile, url: string) {
    const v = this.contentEl.createDiv("video-container").createEl("video", { attr: { src: url, controls: true } }) as HTMLVideoElement;
    v.controls = true;
  }
}

export class PdfView extends MediaView {
  getViewType() { return "pdf"; }
  getIcon() { return "file-text"; }
  render(_f: TFile, url: string) {
    this.contentEl.createEl("iframe", { cls: "pdf-container", attr: { src: url } });
  }
}

export class UnsupportedView extends MediaView {
  getViewType() { return "unsupported"; }
  getIcon() { return "file-question"; }
  render(file: TFile) {
    const c = this.contentEl.createDiv("empty-state");
    c.createDiv({ cls: "empty-state-title", text: `Ateliet can't display .${file.extension} files yet.` });
    const b = c.createEl("button", { text: "Open in default app", cls: "mod-cta" });
    b.addEventListener("click", () => this.app.openWithDefaultApp(file.path));
  }
}

export function registerFileViews(app: any) {
  const r = app.viewRegistry;
  r.registerView("markdown", (leaf: any) => new MarkdownView(leaf));
  r.registerExtensions(["md"], "markdown");
  r.registerView("image", (leaf: any) => new ImageView(leaf));
  r.registerExtensions(["png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "avif", "ico"], "image");
  r.registerView("audio", (leaf: any) => new AudioView(leaf));
  r.registerExtensions(["mp3", "wav", "m4a", "ogg", "3gp", "flac", "aac"], "audio");
  r.registerView("video", (leaf: any) => new VideoView(leaf));
  r.registerExtensions(["mp4", "webm", "ogv", "mov", "mkv", "m4v"], "video");
  r.registerView("pdf", (leaf: any) => new PdfView(leaf));
  r.registerExtensions(["pdf"], "pdf");
  r.registerView("unsupported", (leaf: any) => new UnsupportedView(leaf));
}
