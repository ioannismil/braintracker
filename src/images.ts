import { invoke } from "@tauri-apps/api/core";

/** Pasted images are served by the Rust side at this origin (custom "btimg" protocol on Windows). */
export const IMG_BASE = "http://btimg.localhost/";

const IMG_REF = /\]\(img:([A-Za-z0-9._-]+)\)/g;

/** Markdown stores images as ![…](img:<file>); point them at the image server for rendering. */
export function resolveImages(markdown: string): string {
  return markdown.replace(IMG_REF, `](${IMG_BASE}$1)`);
}

export function imageNames(markdown: string): string[] {
  return [...markdown.matchAll(IMG_REF)].map((m) => m[1]);
}

export function imageFiles(dt: DataTransfer | null): File[] {
  if (!dt) return [];
  const files: File[] = [];
  for (const item of Array.from(dt.items ?? [])) {
    if (item.kind === "file" && item.type.startsWith("image/")) {
      const f = item.getAsFile();
      if (f) files.push(f);
    }
  }
  if (!files.length) {
    for (const f of Array.from(dt.files ?? [])) if (f.type.startsWith("image/")) files.push(f);
  }
  return files;
}

async function saveImage(file: File): Promise<string> {
  const ext = (file.type.split("/")[1] || "png").replace("jpeg", "jpg");
  const bytes = new Uint8Array(await file.arrayBuffer());
  return invoke<string>("save_image", bytes, { headers: { "x-ext": ext } });
}

/**
 * Saves images from a paste/drop and inserts Markdown references at the textarea's caret.
 * Returns the textarea's new value, or null if there were no images.
 */
export async function insertImages(ta: HTMLTextAreaElement, files: File[]): Promise<string | null> {
  if (!files.length) return null;
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  const names = await Promise.all(files.map(saveImage));
  const before = ta.value.slice(0, start);
  const lead = before && !before.endsWith("\n") ? "\n" : "";
  const text = lead + names.map((n) => `![screenshot](img:${n})`).join("\n") + "\n";
  ta.setRangeText(text, start, end, "end");
  ta.focus();
  return ta.value;
}
