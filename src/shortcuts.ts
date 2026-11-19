import { invoke } from "@tauri-apps/api/core";

export interface Shortcuts {
  showMain: string;
  capture: string;
  newEntry: string;
  search: string;
  preview: string;
  settings: string;
  captureSave: string;
  captureSaveClose: string;
  captureNotes: string;
  captureOpenApp: string;
  captureHide: string;
}
export type ShortcutAction = keyof Shortcuts;

export interface ShortcutsInfo {
  shortcuts: Shortcuts;
  mainRegistered: boolean;
  captureRegistered: boolean;
}

export const DEFAULT_SHORTCUTS: Shortcuts = {
  showMain: "Ctrl+Shift+Space",
  capture: "Ctrl+Shift+E",
  newEntry: "Ctrl+T",
  search: "Ctrl+K",
  preview: "Ctrl+E",
  settings: "Ctrl+,",
  captureSave: "Enter",
  captureSaveClose: "Ctrl+Shift+Enter",
  captureNotes: "Shift+Enter",
  captureOpenApp: "Ctrl+O",
  captureHide: "Escape",
};

/** Where a shortcut works: everywhere in Windows, in the main window, or in the quick-capture popup. */
export type ShortcutScope = "global" | "app" | "capture";

export const ACTIONS: { id: ShortcutAction; label: string; scope: ShortcutScope }[] = [
  { id: "showMain", label: "Show / hide BrainTracker", scope: "global" },
  { id: "capture", label: "Quick capture", scope: "global" },
  { id: "newEntry", label: "New entry", scope: "app" },
  { id: "search", label: "Search", scope: "app" },
  { id: "preview", label: "Toggle preview", scope: "app" },
  { id: "settings", label: "Open settings", scope: "app" },
  { id: "captureSave", label: "Save (keep open for the next one)", scope: "capture" },
  { id: "captureSaveClose", label: "Save & close", scope: "capture" },
  { id: "captureNotes", label: "Jump to notes", scope: "capture" },
  { id: "captureOpenApp", label: "Open BrainTracker", scope: "capture" },
  { id: "captureHide", label: "Close without saving", scope: "capture" },
];

/** Two shortcuts collide if either works everywhere, or both work in the same window. */
export function findClash(shortcuts: Shortcuts, id: ShortcutAction, combo: string) {
  const scope = ACTIONS.find((a) => a.id === id)!.scope;
  return ACTIONS.find(
    (a) => a.id !== id && shortcuts[a.id] === combo && (a.scope === scope || a.scope === "global" || scope === "global"),
  );
}

export const shortcutsApi = {
  get: () => invoke<ShortcutsInfo>("get_shortcuts"),
  set: (shortcuts: Shortcuts) => invoke<ShortcutsInfo>("set_shortcuts", { shortcuts }),
  suspend: () => invoke<void>("suspend_shortcuts"),
  resume: () => invoke<ShortcutsInfo>("resume_shortcuts"),
};

const PUNCT: Record<string, string> = {
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
};

/** Uses the physical key (e.code), so shortcuts work the same on any keyboard layout. */
function keyFromCode(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code;
  if (code === "Space") return "Space";
  if (code === "Enter" || code === "NumpadEnter") return "Enter";
  if (code === "Escape") return "Escape";
  return PUNCT[code] ?? null;
}

/** "Ctrl+Shift+E" for a key event, or null for a lone modifier / unsupported key. */
export function comboFromEvent(e: KeyboardEvent): string | null {
  const key = keyFromCode(e.code);
  if (!key) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  return [...mods, key].join("+");
}

/** True if the combo has Ctrl, Alt or Win — i.e. it can't be produced by ordinary typing. */
export function hasCommandModifier(combo: string): boolean {
  return combo.split("+").some((p) => p === "Ctrl" || p === "Alt" || p === "Super");
}

/**
 * A shortcut needs Ctrl, Alt or Win (or be an F-key) so it never fires while you type.
 * The quick-capture popup also allows Enter and Esc (with or without Shift).
 */
export function isUsableCombo(combo: string, scope: ShortcutScope = "app"): boolean {
  const parts = combo.split("+");
  const key = parts[parts.length - 1];
  if (hasCommandModifier(combo) || /^F\d{1,2}$/.test(key)) return true;
  return scope === "capture" && (key === "Enter" || key === "Escape");
}

export function matches(e: KeyboardEvent, combo: string): boolean {
  return comboFromEvent(e) === combo;
}

/** For <kbd>: "Ctrl+Shift+E" -> "Ctrl Shift E" */
export function pretty(combo: string): string {
  return combo.split("+").join(" ").replace("Escape", "Esc");
}
