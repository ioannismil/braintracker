import { invoke } from "@tauri-apps/api/core";

export interface Entry {
  id: number;
  title: string;
  body: string;
  tags: string[];
  pinned: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface EntrySummary {
  id: number;
  title: string;
  /** Search hits are wrapped in \u0001 … \u0002 */
  snippet: string;
  tags: string[];
  pinned: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface TagCount {
  tag: string;
  count: number;
}

export interface AppInfo {
  dataDir: string;
  portable: boolean;
  version: string;
}

export const api = {
  listEntries: (query: string | null, tag: string | null) =>
    invoke<EntrySummary[]>("list_entries", { query, tag }),
  getEntry: (id: number) => invoke<Entry | null>("get_entry", { id }),
  createEntry: (title?: string, tags?: string[], body?: string) =>
    invoke<Entry>("create_entry", { title: title ?? null, body: body ?? null, tags: tags ?? null }),
  hideCapture: () => invoke<void>("hide_capture"),
  captureDone: (openId: number | null, openMain = false) =>
    invoke<void>("capture_done", { openId, openMain }),
  updateEntry: (id: number, title: string, body: string, tags: string[]) =>
    invoke<Entry | null>("update_entry", { id, title, body, tags }),
  setPinned: (id: number, pinned: boolean) => invoke<void>("set_pinned", { id, pinned }),
  deleteEntry: (id: number) => invoke<void>("delete_entry", { id }),
  listTags: () => invoke<TagCount[]>("list_tags"),
  appInfo: () => invoke<AppInfo>("app_info"),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  exportMarkdown: () => invoke<string>("export_markdown"),
};

export function parseTags(text: string): string[] {
  return text
    .split(",")
    .map((t) => t.trim().replace(/^#/, "").toLowerCase().split(/\s+/).join("-"))
    .filter(Boolean);
}
