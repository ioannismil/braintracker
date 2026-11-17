import { invoke } from "@tauri-apps/api/core";

export interface Shortcuts {
  showMain: string;
  capture: string;
  newEntry: string;
  search: string;
  preview: string;
  settings: string;
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
};

export const ACTIONS: { id: ShortcutAction; label: string; global: boolean }[] = [
  { id: "showMain", label: "Show / hide BrainTracker", global: true },
  { id: "capture", label: "Quick capture", global: true },
  { id: "newEntry", label: "New entry", global: false },
  { id: "search", label: "Search", global: false },
  { id: "preview", label: "Toggle preview", global: false },
  { id: "settings", label: "Open settings", global: false },
];

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

/** A shortcut needs Ctrl, Alt or Win (or be an F-key) so it never fires while you type. */
export function isUsableCombo(combo: string): boolean {
  const parts = combo.split("+");
  const key = parts[parts.length - 1];
  return parts.some((p) => p === "Ctrl" || p === "Alt" || p === "Super") || /^F\d{1,2}$/.test(key);
}

export function matches(e: KeyboardEvent, combo: string): boolean {
  return comboFromEvent(e) === combo;
}

/** For <kbd>: "Ctrl+Shift+E" -> "Ctrl Shift E" */
export function pretty(combo: string): string {
  return combo.split("+").join(" ");
}
