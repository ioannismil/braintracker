import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { inboxApi, type InboxConfig, type InboxStatus } from "./api";
import { longDate, relative } from "./format";

/** Settings section: the inbox folder whose notes and images are imported automatically. */
export default function Inbox() {
  const [st, setSt] = useState<InboxStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    inboxApi.status().then(setSt).catch((e) => setError(String(e)));
    // Keep "last import" fresh while Settings is open.
    const t = window.setInterval(() => inboxApi.status().then(setSt).catch(() => {}), 5000);
    return () => window.clearInterval(t);
  }, []);

  if (!st) return null;

  const update = async (patch: Partial<InboxConfig>) => {
    try {
      setSt(await inboxApi.setConfig({ ...st.config, ...patch }));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  };

  const pickFolder = async () => {
    const dir = await open({ directory: true, title: "Choose the inbox folder", defaultPath: st.dir });
    if (typeof dir === "string") update({ dir });
  };

  const openFolder = async () => {
    try {
      revealItemInDir(await inboxApi.dirPath());
    } catch (e) {
      setError(String(e));
    }
  };

  const checkNow = async () => {
    setBusy(true);
    try {
      setSt(await inboxApi.checkNow());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const problem = error || st.lastError;

  return (
    <div className="setting block">
      <div className="setting-head">
        <div>
          <strong>Inbox folder</strong>
          <p>
            Notes (.md, .txt) and images you put in this folder become entries automatically, tagged #inbox, then
            move to its “Imported” subfolder.
          </p>
        </div>
        <button
          className={`switch ${st.config.enabled ? "on" : ""}`}
          role="switch"
          aria-checked={st.config.enabled}
          aria-label="Import from the inbox folder"
          onClick={() => update({ enabled: !st.config.enabled })}
        >
          <span />
        </button>
      </div>

      {st.config.enabled && (
        <>
          <div className="backup-folder">
            <span className="path" title={st.dir}>
              {st.dir}
            </span>
            <button className="tool" onClick={pickFolder}>
              Change…
            </button>
            {st.config.dir && (
              <button className="tool" onClick={() => update({ dir: null })} title="Documents\BrainTracker Inbox">
                Default
              </button>
            )}
            <button className="tool" onClick={openFolder}>
              Open
            </button>
          </div>
          <div className="backup-row">
            <span>
              {st.lastImport ? (
                <span title={longDate(st.lastImport)}>
                  Last import {relative(st.lastImport)}: {st.lastCount} {st.lastCount === 1 ? "entry" : "entries"}
                </span>
              ) : (
                "Checked every few seconds. Nothing imported yet."
              )}
            </span>
            <button className="tool" onClick={checkNow} disabled={busy}>
              {busy ? "Checking…" : "Check now"}
            </button>
          </div>
          <p className="hint">
            Tip: pick a folder inside OneDrive, Dropbox or Google Drive, then save notes or screenshots into it from
            your phone’s app for that service. They show up here, with no BrainTracker account involved.
          </p>
        </>
      )}
      {problem && <p className="msg bad">{problem}</p>}
    </div>
  );
}
