import { invoke } from "@tauri-apps/api/core";

export interface Entry {
  id: number;
  title: string;
  body: string;
  tags: string[];
  pinned: boolean;
  createdAt: number;
  updatedAt: number;
  /** In the trash (e.g. deleted with `bt delete` while it was open). */
  trashed: boolean;
}

export type SaveResult =
  | { status: "saved"; entry: Entry }
  /** Changed elsewhere since it was loaded: nothing was written; this is the note now. */
  | { status: "conflict"; entry: Entry }
  | { status: "missing" };

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

export interface InboxConfig {
  enabled: boolean;
  dir: string | null;
}

export interface InboxStatus {
  config: InboxConfig;
  dir: string;
  lastImport: number | null;
  lastCount: number;
  lastError: string | null;
}

export const inboxApi = {
  status: () => invoke<InboxStatus>("inbox_status"),
  setConfig: (config: InboxConfig) => invoke<InboxStatus>("set_inbox_config", { config }),
  checkNow: () => invoke<InboxStatus>("inbox_check_now"),
  /** The folder (created if needed), for "Open folder". */
  dirPath: () => invoke<string>("inbox_dir_path"),
};

export interface PlaceholderConfig {
  /** Copying a code block with {{blanks}} asks for them first. */
  ask: boolean;
  /** Remember typed values to suggest next time (never for names like password or token). */
  remember: boolean;
  values: Record<string, string>;
}

export const prefsApi = {
  getOffline: () => invoke<boolean>("get_offline"),
  setOffline: (enabled: boolean) => invoke<boolean>("set_offline", { enabled }),
  getPlaceholders: () => invoke<PlaceholderConfig>("get_placeholders"),
  setPlaceholders: (config: PlaceholderConfig) => invoke<PlaceholderConfig>("set_placeholders", { config }),
  rememberPlaceholders: (values: Record<string, string>) => invoke<void>("remember_placeholders", { values }),
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
  /** Entries matching any of the words, best first (for when none has all of them). */
  searchAny: (query: string) => invoke<EntrySummary[]>("search_any", { query }),
  getEntry: (id: number) => invoke<Entry | null>("get_entry", { id }),
  createEntry: (title?: string, tags?: string[], body?: string) =>
    invoke<Entry>("create_entry", { title: title ?? null, body: body ?? null, tags: tags ?? null }),
  hideCapture: () => invoke<void>("hide_capture"),
  captureSaved: () => invoke<void>("capture_saved"),
  /**
   * Something waiting for this window (taken once): "selection" (text for quick capture's notes),
   * "search" (a query for quick search) or "open" (an entry id for the main window).
   */
  takePending: (key: "selection" | "search" | "open") => invoke<string | null>("take_pending", { key }),
  captureDone: (openId: number | null, openMain = false) =>
    invoke<void>("capture_done", { openId, openMain }),
  hideSpotlight: () => invoke<void>("hide_spotlight"),
  spotlightOpen: (openId: number | null) => invoke<void>("spotlight_open", { openId }),
  /** Opens the note in its own small always-on-top window. */
  popOut: (id: number) => invoke<void>("pop_out", { id }),
  /** From a popped-out note: edit it in the main window. */
  openInMain: (id: number) => invoke<void>("open_in_main", { id }),
  /**
   * Saves, but only if the note is still the version `baseUpdatedAt` (when given); otherwise
   * returns the current note as a conflict. `keepPrevious` puts the replaced version in History.
   */
  updateEntry: (
    id: number,
    title: string,
    body: string,
    tags: string[],
    baseUpdatedAt?: number,
    keepPrevious = false,
  ) =>
    invoke<SaveResult>("update_entry", { id, title, body, tags, baseUpdatedAt: baseUpdatedAt ?? null, keepPrevious }),
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

// ---------- phone sync (src-tauri/src/sync/) ----------

/** The PC side: Settings → Sync with your phone. */
export interface PcSyncStatus {
  enabled: boolean;
  offline: boolean;
  running: boolean;
  port: number | null;
  addresses: string[];
  error: string | null;
  peers: { id: string; name: string; pairedAt: number; lastSync: number | null }[];
  /** A pairing code on screen, waiting for a phone to scan it. */
  pairing: { qrSvg: string; code: string; expiresAt: number } | null;
}

/** The phone side. */
export interface PhoneSyncStatus {
  paired: boolean;
  pcName: string | null;
  lastSync: number | null;
  lastError: string | null;
  syncing: boolean;
  offline: boolean;
}

export const syncApi = {
  pcStatus: () => invoke<PcSyncStatus>("sync_status"),
  setEnabled: (enabled: boolean) => invoke<PcSyncStatus>("set_sync_enabled", { enabled }),
  startPairing: () => invoke<PcSyncStatus>("start_pairing"),
  cancelPairing: () => invoke<PcSyncStatus>("cancel_pairing"),
  removePeer: (id: string) => invoke<PcSyncStatus>("remove_peer", { id }),

  phoneStatus: () => invoke<PhoneSyncStatus>("sync_status"),
  pairWith: (code: string) => invoke<PhoneSyncStatus>("pair_with", { code }),
  syncNow: () => invoke<PhoneSyncStatus>("sync_now"),
  unpair: () => invoke<PhoneSyncStatus>("unpair"),
};
