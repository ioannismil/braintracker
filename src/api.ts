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

export interface BackupConfig {
  enabled: boolean;
  dir: string | null;
  keep: number;
}

export interface BackupStatus {
  config: BackupConfig;
  dir: string;
  defaultDir: string;
  lastBackup: number | null;
  lastError: string | null;
  backups: { name: string; createdAt: number; size: number }[];
}

export interface TrashItem {
  id: number;
  title: string;
  snippet: string;
  tags: string[];
  deletedAt: number;
}

export interface VersionSummary {
  id: number;
  title: string;
  snippet: string;
  chars: number;
  savedAt: number;
}

export interface Version {
  id: number;
  entryId: number;
  title: string;
  body: string;
  tags: string[];
  savedAt: number;
}

export interface ImportResult {
  imported: number;
  skipped: number;
  failed: string[];
}

export const trashApi = {
  list: () => invoke<TrashItem[]>("list_trash"),
  count: () => invoke<number>("trash_count"),
  restore: (id: number) => invoke<void>("restore_entry", { id }),
  purge: (id: number) => invoke<void>("purge_entry", { id }),
  empty: () => invoke<number>("empty_trash"),
};

export const historyApi = {
  list: (entryId: number) => invoke<VersionSummary[]>("list_versions", { entryId }),
  get: (id: number) => invoke<Version | null>("get_version", { id }),
  restore: (entryId: number, versionId: number) =>
    invoke<Entry | null>("restore_version", { entryId, versionId }),
};

export const dataApi = {
  importMarkdown: (paths: string[]) => invoke<ImportResult>("import_markdown", { paths }),
  findUnusedImages: () => invoke<{ count: number; bytes: number }>("find_unused_images"),
  deleteUnusedImages: () => invoke<{ count: number; bytes: number }>("delete_unused_images"),
};

export interface UpdateInfo {
  version: string;
  current: string;
  notes: string | null;
  portable: boolean;
  releasesUrl: string;
}

export const updateApi = {
  check: () => invoke<UpdateInfo | null>("check_for_update"),
  install: () => invoke<void>("install_update"),
};

export const backupApi = {
  status: () => invoke<BackupStatus>("backup_status"),
  setConfig: (config: BackupConfig) => invoke<BackupStatus>("set_backup_config", { config }),
  now: () => invoke<BackupStatus>("backup_now"),
  restore: (name: string) => invoke<BackupStatus>("restore_backup", { name }),
};

export const api = {
  listEntries: (query: string | null, tag: string | null) =>
    invoke<EntrySummary[]>("list_entries", { query, tag }),
  getEntry: (id: number) => invoke<Entry | null>("get_entry", { id }),
  createEntry: (title?: string, tags?: string[], body?: string) =>
    invoke<Entry>("create_entry", { title: title ?? null, body: body ?? null, tags: tags ?? null }),
  hideCapture: () => invoke<void>("hide_capture"),
  captureSaved: () => invoke<void>("capture_saved"),
  captureDone: (openId: number | null, openMain = false) =>
    invoke<void>("capture_done", { openId, openMain }),
  updateEntry: (id: number, title: string, body: string, tags: string[]) =>
    invoke<Entry | null>("update_entry", { id, title, body, tags }),
  setPinned: (id: number, pinned: boolean) => invoke<void>("set_pinned", { id, pinned }),
  /** Moves to the trash. */
  deleteEntry: (id: number) => invoke<void>("delete_entry", { id }),
  /** Deletes for good (blank entries that were never filled in). */
  discardEntry: (id: number) => invoke<void>("discard_entry", { id }),
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
