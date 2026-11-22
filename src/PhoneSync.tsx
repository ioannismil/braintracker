import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { syncApi, type PcSyncStatus } from "./api";
import { longDate, relative } from "./format";

/** Settings section: sync with the BrainTracker phone app over Wi-Fi (sync/server.rs). */
export default function PhoneSync() {
  const [st, setSt] = useState<PcSyncStatus | null>(null);
  const [error, setError] = useState("");
  const [showCode, setShowCode] = useState(false);

  useEffect(() => {
    const load = () => syncApi.pcStatus().then(setSt).catch((e) => setError(String(e)));
    load();
    // A phone pairing or syncing updates this straight away; the timer keeps "last sync" fresh
    // and notices the pairing code expiring.
    const un = listen("sync-changed", load);
    const t = window.setInterval(load, 10000);
    return () => {
      window.clearInterval(t);
      un.then((f) => f());
    };
  }, []);

  if (!st) return null;

  const run = async (f: () => Promise<PcSyncStatus>) => {
    try {
      setSt(await f());
      setError("");
    } catch (e) {
      setError(String(e));
    }
  };

  const remove = (id: string, name: string) => {
    if (confirm(`Stop syncing with ${name}? Its notes stay on it; pair it again to sync again.`))
      run(() => syncApi.removePeer(id));
  };

  const problem = error || st.error;

  return (
    <div className="setting block">
      <div className="setting-head">
        <div>
          <strong>Sync with your phone</strong>
          <p>
            The BrainTracker Android app keeps a copy of your notes and syncs with this PC over your Wi-Fi —
            directly and encrypted, never through the internet.
          </p>
        </div>
        <button
          className={`switch ${st.enabled ? "on" : ""}`}
          role="switch"
          aria-checked={st.enabled}
          aria-label="Sync with your phone"
          onClick={() => run(() => syncApi.setEnabled(!st.enabled))}
        >
          <span />
        </button>
      </div>

      {st.enabled && st.offline && <p className="hint">Paused while offline mode is on.</p>}

      {st.enabled && !st.offline && (
        <>
          {st.peers.map((p) => (
            <div className="backup-row" key={p.id}>
              <span>
                <strong>{p.name}</strong>{" "}
                {p.lastSync ? (
                  <span title={longDate(p.lastSync)}>· synced {relative(p.lastSync)}</span>
                ) : (
                  <span>· paired, not synced yet</span>
                )}
              </span>
              <button className="tool" onClick={() => remove(p.id, p.name)}>
                Remove
              </button>
            </div>
          ))}

          {st.pairing ? (
            <div className="pair-box">
              <div className="pair-qr" dangerouslySetInnerHTML={{ __html: st.pairing.qrSvg }} />
              <div>
                <p>
                  In the BrainTracker app on your phone, tap <strong>Pair with PC</strong> and scan this code. The
                  phone must be on the same Wi-Fi as this PC.
                </p>
                <p className="hint">Works once, until {new Date(st.pairing.expiresAt).toLocaleTimeString()}.</p>
                {showCode ? (
                  <textarea className="pair-code" readOnly value={st.pairing.code} onFocus={(e) => e.target.select()} />
                ) : (
                  <button className="link" onClick={() => setShowCode(true)}>
                    Can’t scan? Show the code as text
                  </button>
                )}
                <div className="btn-row">
                  <button className="tool" onClick={() => run(syncApi.cancelPairing)}>
                    Cancel
                  </button>
                </div>
              </div>
            </div>
          ) : (
            <div className="backup-row">
              <span className="hint">
                {st.running
                  ? `Listening on ${st.addresses.join(", ") || "this PC"} (port ${st.port}).`
                  : "Not running."}
              </span>
              <button className="tool" onClick={() => run(syncApi.startPairing)}>
                {st.peers.length ? "Pair another phone" : "Pair a phone"}
              </button>
            </div>
          )}
          <p className="hint">
            The first time, Windows may ask whether BrainTracker can use private networks: allow it, or the phone
            can’t reach this PC.
          </p>
        </>
      )}
      {problem && <p className="msg bad">{problem}</p>}
    </div>
  );
}
