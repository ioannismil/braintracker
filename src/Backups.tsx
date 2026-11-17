import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { backupApi, type BackupStatus } from "./api";
import { longDate, relative } from "./format";

const KEEP_OPTIONS = [7, 14, 30, 60, 90];

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Settings section: automatic backups, backup folder, retention and restore. */
export default function Backups() {
  const [st, setSt] = useState<BackupStatus | null>(null);
  const [busy, setBusy] = useState<"backup" | "restore" | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [note, setNote] = useState<{ text: string; bad?: boolean } | null>(null);
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    backupApi.status().then(setSt).catch((e) => setNote({ text: String(e), bad: true }));
  }, []);

  if (!st) return null;

  const update = async (patch: Partial<BackupStatus["config"]>) => {
    try {
      setSt(await backupApi.setConfig({ ...st.config, ...patch }));
      setNote(null);
    } catch (e) {
      setNote({ text: String(e), bad: true });
    }
  };

  const pickFolder = async () => {
    const dir = await open({ directory: true, title: "Choose a folder for backups", defaultPath: st.dir });
    if (typeof dir === "string") update({ dir });
  };

  const backupNow = async () => {
    setBusy("backup");
    try {
      setSt(await backupApi.now());
      setNote({ text: "Backup saved." });
    } catch (e) {
      setNote({ text: `Backup failed: ${e}`, bad: true });
    } finally {
      setBusy(null);
    }
  };

  const restore = async (name: string) => {
    setBusy("restore");
    setConfirm(null);
    try {
      setSt(await backupApi.restore(name));
      setNote({ text: "Restored. Your previous data was saved as a “before-restore” backup." });
    } catch (e) {
      setNote({ text: String(e), bad: true });
    } finally {
      setBusy(null);
    }
  };

  const list = showAll ? st.backups : st.backups.slice(0, 5);

  return (
    <div className="setting block">
      <div className="setting-head">
        <div>
          <strong>Automatic backups</strong>
          <p>
            A snapshot every day you make changes. Last backup:{" "}
            {st.lastBackup ? <span title={longDate(st.lastBackup)}>{relative(st.lastBackup)}</span> : "never"}
          </p>
        </div>
        <button
          className={`switch ${st.config.enabled ? "on" : ""}`}
          role="switch"
          aria-checked={st.config.enabled}
          onClick={() => update({ enabled: !st.config.enabled })}
        >
          <span />
        </button>
      </div>

      <div className="backup-folder">
        <span className="path" title={st.dir}>
          {st.dir}
        </span>
        <button className="tool" onClick={pickFolder}>
          Change…
        </button>
        <button className="tool" onClick={() => revealItemInDir(st.dir)}>
          Open
        </button>
        {st.config.dir && st.dir !== st.defaultDir && (
          <button className="tool" onClick={() => update({ dir: null })} title={st.defaultDir}>
            Use default
          </button>
        )}
      </div>

      <div className="backup-row">
        <label>
          Keep the newest{" "}
          <select value={st.config.keep} onChange={(e) => update({ keep: Number(e.target.value) })}>
            {[...new Set([...KEEP_OPTIONS, st.config.keep])].sort((a, b) => a - b).map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>{" "}
          backups
        </label>
        <button className="tool on" onClick={backupNow} disabled={busy !== null}>
          {busy === "backup" ? "Backing up…" : "Back up now"}
        </button>
      </div>

      {st.lastError && !note && <p className="sc-error">Last automatic backup failed: {st.lastError}</p>}
      {note && <p className={note.bad ? "sc-error" : "msg"}>{note.text}</p>}

      {st.backups.length > 0 && (
        <ul className="backup-list">
          {list.map((b) => (
            <li key={b.name}>
              <span className="backup-when" title={b.name}>
                {longDate(b.createdAt)}
                {b.name.includes("before-restore") && <span className="badge">before restore</span>}
              </span>
              <span className="backup-size">{size(b.size)}</span>
              {confirm === b.name ? (
                <>
                  <button className="tool danger" onClick={() => restore(b.name)} disabled={busy !== null}>
                    Replace current data
                  </button>
                  <button className="tool" onClick={() => setConfirm(null)}>
                    Cancel
                  </button>
                </>
              ) : (
                <button className="tool" onClick={() => setConfirm(b.name)} disabled={busy !== null}>
                  {busy === "restore" ? "Restoring…" : "Restore"}
                </button>
              )}
            </li>
          ))}
          {st.backups.length > 5 && (
            <li>
              <button className="link" onClick={() => setShowAll((s) => !s)}>
                {showAll ? "Show fewer" : `Show all ${st.backups.length}`}
              </button>
            </li>
          )}
        </ul>
      )}
    </div>
  );
}
