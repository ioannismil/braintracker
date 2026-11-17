import { useEffect, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, updateApi, type AppInfo, type UpdateInfo } from "./api";
import Backups from "./Backups";
import DataTools from "./DataTools";
import UpdateBanner from "./UpdateBanner";
import {
  ACTIONS,
  comboFromEvent,
  DEFAULT_SHORTCUTS,
  isUsableCombo,
  pretty,
  shortcutsApi,
  type ShortcutAction,
  type Shortcuts,
  type ShortcutsInfo,
} from "./shortcuts";

interface Props {
  info: ShortcutsInfo;
  onChange: (info: ShortcutsInfo) => void;
  onClose: () => void;
  onUpdateFound: (u: UpdateInfo) => void;
  beforeInstall: () => Promise<void>;
}

export default function Settings({ info, onChange, onClose, onUpdateFound, beforeInstall }: Props) {
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
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [msg, setMsg] = useState("");
  const [recording, setRecording] = useState<ShortcutAction | null>(null);
  const [scError, setScError] = useState("");
  const infoRef = useRef(info);
  infoRef.current = info;

  useEffect(() => {
    api.getAutostart().then(setAutostart).catch(() => setAutostart(false));
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

    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        setRecording(null);
        return;
      }
      const combo = comboFromEvent(e);
      if (!combo) return; // only a modifier so far — keep waiting
      if (!isUsableCombo(combo)) {
        setScError("Include Ctrl, Alt or Win so it doesn’t fire while you type.");
        return;
      }
      const current = infoRef.current.shortcuts;
      const clash = ACTIONS.find((a) => a.id !== recording && current[a.id] === combo);
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
    (id === "showMain" && !info.mainRegistered) || (id === "capture" && !info.captureRegistered);

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Settings">
        <header>
          <h2>Settings</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </header>

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

        <div className="setting block">
          <div className="setting-head">
            <div>
              <strong>Keyboard shortcuts</strong>
              <p>Click a shortcut, then press the new keys. Esc cancels.</p>
            </div>
            {!isDefault && (
              <button className="tool" onClick={() => apply(DEFAULT_SHORTCUTS)}>
                Reset to defaults
              </button>
            )}
          </div>
          <div className="sc-list">
            {ACTIONS.map((a) => (
              <div key={a.id} className="sc-row">
                <span className="sc-label">
                  {a.label}
                  {a.global && <span className="badge">works anywhere</span>}
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
          {scError && <p className="sc-error">{scError}</p>}
          <p className="hint">
            In quick capture: <kbd>Enter</kbd> save · <kbd>Shift Enter</kbd> notes · <kbd>Ctrl Shift Enter</kbd> save
            &amp; open · <kbd>Ctrl O</kbd> open app · <kbd>Esc</kbd> hide
          </p>
        </div>

        <div className="setting block">
          <div className="setting-head">
            <div>
              <strong>Updates</strong>
              <p>
                Version {appInfo?.version ?? "…"} · BrainTracker checks for new versions automatically and offers to
                install them.
              </p>
            </div>
            <button className="tool" onClick={checkUpdates} disabled={checking}>
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
      </div>
    </div>
  );
}
