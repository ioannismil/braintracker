import { useEffect, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, prefsApi, updateApi, type AppInfo, type UpdateInfo } from "./api";
import Backups from "./Backups";
import DataTools from "./DataTools";
import Inbox from "./Inbox";
import PhoneSync from "./PhoneSync";
import Placeholders from "./Placeholders";
import UpdateBanner from "./UpdateBanner";
import {
  ACTIONS,
  comboFromEvent,
  DEFAULT_SHORTCUTS,
  findClash,
  isUsableCombo,
  pretty,
  shortcutsApi,
  type ShortcutAction,
  type ShortcutScope,
  type Shortcuts,
  type ShortcutsInfo,
} from "./shortcuts";

const GROUPS: { scope: ShortcutScope; title: string }[] = [
  { scope: "global", title: "Work anywhere in Windows" },
  { scope: "app", title: "In the main window" },
  { scope: "capture", title: "In quick capture" },
];

interface Props {
  info: ShortcutsInfo;
  onChange: (info: ShortcutsInfo) => void;
  onClose: () => void;
  onUpdateFound: (u: UpdateInfo) => void;
  beforeInstall: () => Promise<void>;
  initialTab?: SettingsTab;
}

export type SettingsTab = "general" | "shortcuts" | "commands";

const TABS: { id: SettingsTab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "shortcuts", label: "Keyboard shortcuts" },
  { id: "commands", label: "Commands" },
];

export default function Settings({ info, onChange, onClose, onUpdateFound, beforeInstall, initialTab = "general" }: Props) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  const [checking, setChecking] = useState(false);
  const [found, setFound] = useState<UpdateInfo | null>(null);
  const [updateMsg, setUpdateMsg] = useState("");

  const checkUpdates = async () => {
    setChecking(true);
    setUpdateMsg("");
    try {
      const u = await updateApi.check();
      setFound(u);
      if (u) onUpdateFound(u);
      else setUpdateMsg("You’re on the latest version.");
    } catch (e) {
      setUpdateMsg(`Couldn’t check for updates: ${e}`);
    } finally {
      setChecking(false);
    }
  };
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [offline, setOffline] = useState<boolean | null>(null);
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [msg, setMsg] = useState("");
  const [recording, setRecording] = useState<ShortcutAction | null>(null);
  const [scError, setScError] = useState("");
  const infoRef = useRef(info);
  infoRef.current = info;

  useEffect(() => {
    api.getAutostart().then(setAutostart).catch(() => setAutostart(false));
    prefsApi.getOffline().then(setOffline).catch(() => setOffline(false));
    api.appInfo().then(setAppInfo);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const apply = async (next: Shortcuts) => {
    try {
      onChange(await shortcutsApi.set(next));
      setScError("");
    } catch (e) {
      setScError(String(e));
      shortcutsApi.resume().then(onChange).catch(() => {});
    }
  };

  // Recording a new shortcut: grab the next key combo before anything else sees it.
  useEffect(() => {
    if (!recording) return;
    const applied = { current: false };
    // Release system-wide shortcuts so pressing the current combo doesn't fire it.
    shortcutsApi.suspend().catch(() => {});

    const scope = ACTIONS.find((a) => a.id === recording)!.scope;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      // Esc cancels — except in quick capture, where Esc is a valid key (click the button again to cancel).
      if (scope !== "capture" && e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        setRecording(null);
        return;
      }
      const combo = comboFromEvent(e);
      if (!combo) return; // only a modifier so far — keep waiting
      if (!isUsableCombo(combo, scope)) {
        setScError(
          scope === "capture"
            ? "Use Enter or Esc (Shift allowed), an F-key, or include Ctrl, Alt or Win."
            : "Include Ctrl, Alt or Win so it doesn’t fire while you type.",
        );
        return;
      }
      const current = infoRef.current.shortcuts;
      const clash = findClash(current, recording, combo);
      if (clash) {
        setScError(`${combo} is already used for “${clash.label}”.`);
        return;
      }
      applied.current = true;
      apply({ ...current, [recording]: combo });
      setRecording(null);
    };

    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      if (!applied.current) shortcutsApi.resume().then(onChange).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording]);

  const toggleAutostart = async () => {
    try {
      setAutostart(await api.setAutostart(!autostart));
    } catch (e) {
      setMsg(`Couldn’t change startup setting: ${e}`);
    }
  };

  const toggleOffline = async () => {
    try {
      setOffline(await prefsApi.setOffline(!offline));
      setFound(null);
      setUpdateMsg("");
    } catch (e) {
      setMsg(`Couldn’t change offline mode: ${e}`);
    }
  };

  const doExport = async () => {
    try {
      const dir = await api.exportMarkdown();
      setMsg(`Exported to ${dir}`);
      revealItemInDir(dir);
    } catch (e) {
      setMsg(`Export failed: ${e}`);
    }
  };

  const isDefault = ACTIONS.every((a) => info.shortcuts[a.id] === DEFAULT_SHORTCUTS[a.id]);
  const unavailable = (id: ShortcutAction) =>
    ACTIONS.find((a) => a.id === id)?.scope === "global" && !!info.shortcuts[id] && !info.registered.includes(id);

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Settings">
        <header>
          <h2>Settings</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </header>

        <nav className="tabs" role="tablist">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={tab === t.id}
              className={`tab ${tab === t.id ? "on" : ""}`}
              onClick={() => {
                setRecording(null);
                setScError("");
                setTab(t.id);
              }}
            >
              {t.label}
            </button>
          ))}
        </nav>

        {tab === "shortcuts" && (
          <div className="setting block">
            <div className="setting-head">
              <div>
                <p>Click a shortcut, then press the new keys. Click it again to cancel.</p>
              </div>
              {!isDefault && (
                <button className="tool" onClick={() => apply(DEFAULT_SHORTCUTS)}>
                  Reset to defaults
                </button>
              )}
            </div>
            {GROUPS.map((g) => (
              <div key={g.scope}>
                <h3 className="sc-group">{g.title}</h3>
                <div className="sc-list">
                  {ACTIONS.filter((a) => a.scope === g.scope).map((a) => (
                    <div key={a.id} className="sc-row">
                      <span className="sc-label">
                        {a.label}
                        {unavailable(a.id) && recording !== a.id && (
                          <span className="sc-warn">in use by another app — pick another</span>
                        )}
                      </span>
                      <button
                        className={`sc-key ${recording === a.id ? "rec" : ""} ${unavailable(a.id) ? "bad" : ""}`}
                        onClick={() => {
                          setScError("");
                          setRecording(recording === a.id ? null : a.id);
                        }}
                      >
                        {recording === a.id ? "Press keys…" : pretty(info.shortcuts[a.id])}
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            ))}
            {scError && <p className="sc-error">{scError}</p>}
            <p className="hint">
              In quick capture, a plain Enter or Shift Enter shortcut doesn’t fire inside the notes box (it adds a
              line there) — use <kbd>Ctrl Enter</kbd> to save from the notes.
            </p>
          </div>
        )}

        {tab === "commands" && <Placeholders />}

        {tab === "general" && (
          <>
            <div className="setting">
              <div>
                <strong>Launch when Windows starts</strong>
                <p>Starts quietly in the system tray so it’s always one keystroke away.</p>
              </div>
              <button
                className={`switch ${autostart ? "on" : ""}`}
                role="switch"
                aria-checked={!!autostart}
                disabled={autostart === null}
                onClick={toggleAutostart}
              >
                <span />
              </button>
            </div>

            <div className="setting">
              <div>
                <strong>Offline mode</strong>
                <p>
                  BrainTracker never connects to the internet: no update checks, and web images in notes aren’t
                  loaded. Links still open in your browser when you click them.
                </p>
              </div>
              <button
                className={`switch ${offline ? "on" : ""}`}
                role="switch"
                aria-checked={!!offline}
                aria-label="Offline mode"
                disabled={offline === null}
                onClick={toggleOffline}
              >
                <span />
              </button>
            </div>

            <div className="setting block">
              <div className="setting-head">
                <div>
                  <strong>Updates</strong>
                  <p>
                    Version {appInfo?.version ?? "…"} ·{" "}
                    {offline
                      ? "Update checks are off while offline mode is on."
                      : "BrainTracker checks for new versions automatically and offers to install them."}
                  </p>
                </div>
                <button className="tool" onClick={checkUpdates} disabled={checking || !!offline}>
                  {checking ? "Checking…" : "Check now"}
                </button>
              </div>
              {found && (
                <div style={{ marginTop: 10 }}>
                  <UpdateBanner info={found} beforeInstall={beforeInstall} />
                </div>
              )}
              {updateMsg && <p className="msg">{updateMsg}</p>}
            </div>

            <Backups />

            <Inbox />

            <PhoneSync />

            <DataTools />

            <div className="setting">
              <div>
                <strong>Your data{appInfo?.portable && <span className="badge">portable mode</span>}</strong>
                <p className="path">{appInfo?.dataDir ?? "…"}</p>
                <p>Entries live in braintracker.db, screenshots in the images folder.</p>
              </div>
              <button className="tool" onClick={() => appInfo && revealItemInDir(appInfo.dataDir)}>
                Open folder
              </button>
            </div>

            <div className="setting">
              <div>
                <strong>Export</strong>
                <p>Save every entry as a Markdown file in Documents\BrainTracker Export.</p>
              </div>
              <button className="tool" onClick={doExport}>
                Export
              </button>
            </div>

            <p className="hint">
              Closing the window keeps BrainTracker running in the tray. Use the tray icon → Quit to exit.
            </p>
            {msg && <p className="msg">{msg}</p>}
          </>
        )}
      </div>
    </div>
  );
}
