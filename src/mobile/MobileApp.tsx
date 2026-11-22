//! The phone app: one screen at a time (notes list → a note, sync, trash), for touch.
//! Uses the same commands as the desktop (lib.rs); sync with the PC is in src-tauri/src/sync/.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  parseTags,
  syncApi,
  trashApi,
  type Entry,
  type EntrySummary,
  type PhoneSyncStatus,
  type TagCount,
  type TrashItem,
} from "../api";
import { insertImages } from "../images";
import { handleMarkdownClick, renderMarkdown } from "../markdown";
import { mergeNote } from "../merge";
import { relative, shortWhen } from "../format";
import Snippet from "../Snippet";

type View = { kind: "list" } | { kind: "note"; id: number; edit: boolean } | { kind: "sync" } | { kind: "trash" };

/** Ask for a sync this long after the last edit (quicker than the every-45-seconds one). */
const SYNC_AFTER_EDIT_MS = 2500;
let syncTimer: number | undefined;
function syncSoon() {
  window.clearTimeout(syncTimer);
  syncTimer = window.setTimeout(() => syncApi.syncNow().catch(() => {}), SYNC_AFTER_EDIT_MS);
}

export default function MobileApp() {
  const [ready, setReady] = useState(false);
  const [view, setView] = useState<View>({ kind: "list" });
  const [sync, setSync] = useState<PhoneSyncStatus | null>(null);
  /** Bumped whenever notes change outside the open screen (sync), so lists reload. */
  const [outside, setOutside] = useState(0);

  // The notes open a moment after the window does.
  useEffect(() => {
    let done = false;
    const check = () =>
      invoke<boolean>("db_ready").then((ok) => {
        if (ok && !done) {
          done = true;
          setReady(true);
        }
      });
    check();
    const un = listen("db-ready", check);
    const t = window.setInterval(check, 300);
    return () => {
      window.clearInterval(t);
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!ready) return;
    const loadSync = () => syncApi.phoneStatus().then(setSync).catch(() => {});
    loadSync();
    const uns = [listen("sync-changed", loadSync), listen("outside-change", () => setOutside((n) => n + 1))];
    // Back in the app: catch up with the PC.
    const onVisible = () => document.visibilityState === "visible" && syncApi.syncNow().then(setSync).catch(() => {});
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      uns.forEach((u) => u.then((f) => f()));
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [ready]);

  // Android's back button/gesture goes back in the webview's history: each screen is a step.
  useEffect(() => {
    history.replaceState({ kind: "list" }, "");
    const onPop = (e: PopStateEvent) => setView((e.state as View) ?? { kind: "list" });
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const go = useCallback((v: View) => {
    history.pushState(v, "");
    setView(v);
  }, []);
  const back = useCallback(() => history.back(), []);

  if (!ready) return <div className="m-loading">Opening your notes…</div>;

  switch (view.kind) {
    case "note":
      return <NoteScreen key={view.id} id={view.id} startEditing={view.edit} outside={outside} onBack={back} />;
    case "sync":
      return <SyncScreen status={sync} onStatus={setSync} onBack={back} />;
    case "trash":
      return <TrashScreen outside={outside} onBack={back} />;
    default:
      return <ListScreen sync={sync} outside={outside} go={go} />;
  }
}

// ---------- the list ----------

function SyncBadge({ sync, onClick }: { sync: PhoneSyncStatus | null; onClick: () => void }) {
  let label = "Not synced";
  let cls = "";
  if (sync?.syncing) [label, cls] = ["Syncing…", "busy"];
  else if (sync?.offline) [label, cls] = ["Offline", ""];
  else if (sync?.paired && sync.lastError) [label, cls] = ["Sync problem", "bad"];
  else if (sync?.paired) [label, cls] = [sync.lastSync ? `Synced ${relative(sync.lastSync)}` : "Paired", "ok"];
  return (
    <button className={`m-badge ${cls}`} onClick={onClick}>
      <span className="dot" />
      {label}
    </button>
  );
}

function ListScreen({ sync, outside, go }: { sync: PhoneSyncStatus | null; outside: number; go: (v: View) => void }) {
  const [query, setQuery] = useState("");
  const [tag, setTag] = useState<string | null>(null);
  const [entries, setEntries] = useState<EntrySummary[]>([]);
  const [tags, setTags] = useState<TagCount[]>([]);
  const [menu, setMenu] = useState(false);

  useEffect(() => {
    const t = window.setTimeout(() => {
      const q = query.trim() || null;
      api
        .listEntries(q, tag)
        .then(async (found) => (found.length || !q ? found : api.searchAny(q)))
        .then(setEntries)
        .catch(() => {});
    }, 120);
    return () => window.clearTimeout(t);
  }, [query, tag, outside]);

  useEffect(() => {
    api.listTags().then(setTags).catch(() => {});
  }, [outside]);

  const create = async () => {
    const e = await api.createEntry("", tag ? [tag] : []);
    go({ kind: "note", id: e.id, edit: true });
  };

  return (
    <div className="m-screen">
      <header className="m-bar">
        <h1>BrainTracker</h1>
        <SyncBadge sync={sync} onClick={() => go({ kind: "sync" })} />
        <button className="m-icon" aria-label="More" onClick={() => setMenu(!menu)}>
          ⋮
        </button>
        {menu && (
          <div className="m-menu" onClick={() => setMenu(false)}>
            <button onClick={() => go({ kind: "sync" })}>Sync with PC</button>
            <button onClick={() => go({ kind: "trash" })}>Trash</button>
          </div>
        )}
      </header>
      <div className="m-search">
        <input
          type="search"
          placeholder="Search notes, commands, error codes…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          enterKeyHint="search"
        />
      </div>
      {tags.length > 0 && (
        <div className="m-tags">
          {tags.map((t) => (
            <button key={t.tag} className={tag === t.tag ? "on" : ""} onClick={() => setTag(tag === t.tag ? null : t.tag)}>
              #{t.tag}
            </button>
          ))}
        </div>
      )}
      <main className="m-list">
        {entries.map((e) => (
          <button key={e.id} className="m-row" onClick={() => go({ kind: "note", id: e.id, edit: false })}>
            <div className="m-row-top">
              <span className="m-row-title">
                {e.pinned && <span className="pin">📌 </span>}
                {e.title || <em>Untitled</em>}
              </span>
              <span className="m-row-when">{shortWhen(e.updatedAt)}</span>
            </div>
            {e.snippet && (
              <div className="m-row-snippet">
                <Snippet text={e.snippet} />
              </div>
            )}
            {e.tags.length > 0 && <div className="m-row-tags">{e.tags.map((t) => `#${t}`).join("  ")}</div>}
          </button>
        ))}
        {entries.length === 0 && (
          <div className="m-empty">
            {query || tag ? (
              "Nothing found."
            ) : sync?.paired ? (
              "No notes yet. Tap + to write one."
            ) : (
              <>
                <p>No notes yet.</p>
                <button className="m-primary" onClick={() => go({ kind: "sync" })}>
                  Get your notes from your PC
                </button>
              </>
            )}
          </div>
        )}
      </main>
      <button className="m-fab" aria-label="New note" onClick={create}>
        +
      </button>
    </div>
  );
}

// ---------- one note ----------

interface Draft {
  title: string;
  body: string;
  tagText: string;
}

const toDraft = (e: Entry): Draft => ({ title: e.title, body: e.body, tagText: e.tags.join(", ") });

function NoteScreen({
  id,
  startEditing,
  outside,
  onBack,
}: {
  id: number;
  startEditing: boolean;
  outside: number;
  onBack: () => void;
}) {
  const [entry, setEntry] = useState<Entry | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [editing, setEditing] = useState(startEditing);
  const [missing, setMissing] = useState(false);
  const [note, setNote] = useState("");
  const base = useRef<Entry | null>(null); // the version the draft started from
  const dirty = useRef(false);
  const saveTimer = useRef<number | undefined>(undefined);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const draftRef = useRef(draft);
  draftRef.current = draft;

  const load = useCallback(async () => {
    const e = await api.getEntry(id);
    if (!e) return setMissing(true);
    base.current = e;
    setEntry(e);
    setDraft(toDraft(e));
  }, [id]);

  useEffect(() => {
    load();
  }, [load]);

  const save = useCallback(async (): Promise<void> => {
    window.clearTimeout(saveTimer.current);
    const d = draftRef.current;
    const b = base.current;
    if (!d || !b || !dirty.current) return;
    dirty.current = false;
    const mine = { title: d.title, body: d.body, tags: parseTags(d.tagText) };
    let r = await api.updateEntry(id, mine.title, mine.body, mine.tags, b.updatedAt);
    if (r.status === "conflict") {
      // Changed by sync meanwhile: combine the two; if they can't be, this version wins and
      // the other is kept in History.
      const merged = mergeNote(b, mine, r.entry);
      const v = merged ?? mine;
      r = await api.updateEntry(id, v.title, v.body, v.tags, r.entry.updatedAt, true);
      if (r.status === "saved") {
        setDraft(toDraft(r.entry));
        if (!merged) setNote("This note was also changed on your PC. Your version was kept; the PC’s is in History there.");
      }
    }
    if (r.status === "saved") {
      base.current = r.entry;
      setEntry(r.entry);
      syncSoon();
    } else if (r.status === "missing") setMissing(true);
  }, [id]);

  const change = (patch: Partial<Draft>) => {
    setDraft((d) => (d ? { ...d, ...patch } : d));
    dirty.current = true;
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(save, 700);
  };

  // Notes changed by sync: show the new version unless there are unsaved edits (those are
  // merged when they're saved).
  useEffect(() => {
    if (outside && !dirty.current) load();
  }, [outside, load]);

  // Leaving the screen or the app: save now. A note left completely empty is removed.
  useEffect(() => {
    const onHide = () => document.visibilityState === "hidden" && save();
    document.addEventListener("visibilitychange", onHide);
    return () => {
      document.removeEventListener("visibilitychange", onHide);
      const d = draftRef.current;
      save().then(() => {
        if (d && !d.title.trim() && !d.body.trim() && !d.tagText.trim()) api.discardEntry(id).catch(() => {});
      });
    };
  }, [save, id]);

  const html = useMemo(() => (draft && !editing ? renderMarkdown(draft.body) : ""), [draft?.body, editing]);

  if (missing)
    return (
      <div className="m-screen">
        <header className="m-bar">
          <button className="m-icon" onClick={onBack} aria-label="Back">
            ←
          </button>
        </header>
        <div className="m-empty">This note was deleted.</div>
      </div>
    );
  if (!draft || !entry) return <div className="m-screen" />;

  const togglePin = async () => {
    await api.setPinned(id, !entry.pinned);
    setEntry({ ...entry, pinned: !entry.pinned });
    syncSoon();
  };
  const trash = async () => {
    await save();
    await api.deleteEntry(id);
    syncSoon();
    onBack();
  };
  const addImage = async (files: FileList | null) => {
    const ta = bodyRef.current;
    if (!files?.length || !ta) return;
    const value = await insertImages(ta, Array.from(files));
    if (value !== null) change({ body: value });
  };

  return (
    <div className="m-screen">
      <header className="m-bar">
        <button className="m-icon" onClick={onBack} aria-label="Back">
          ←
        </button>
        <span className="m-bar-fill" />
        {editing && (
          <button className="m-icon" aria-label="Add a picture" onClick={() => fileRef.current?.click()}>
            🖼
          </button>
        )}
        <button className={`m-icon ${entry.pinned ? "on" : ""}`} aria-label={entry.pinned ? "Unpin" : "Pin"} onClick={togglePin}>
          📌
        </button>
        <button className="m-icon" aria-label="Move to trash" onClick={trash}>
          🗑
        </button>
        <button
          className="m-text-btn"
          onClick={() => {
            if (editing) save();
            setEditing(!editing);
          }}
        >
          {editing ? "Done" : "Edit"}
        </button>
        <input ref={fileRef} type="file" accept="image/*" hidden onChange={(e) => addImage(e.target.files)} />
      </header>
      {note && (
        <p className="m-note" onClick={() => setNote("")}>
          {note}
        </p>
      )}
      <main className="m-note-body">
        {editing ? (
          <>
            <input
              className="m-title"
              placeholder="Title"
              value={draft.title}
              onChange={(e) => change({ title: e.target.value })}
            />
            <input
              className="m-tag-input"
              placeholder="Tags, separated by commas"
              value={draft.tagText}
              onChange={(e) => change({ tagText: e.target.value })}
              autoCapitalize="none"
            />
            <textarea
              ref={bodyRef}
              className="m-body"
              placeholder="Notes, commands, links… (Markdown)"
              value={draft.body}
              onChange={(e) => change({ body: e.target.value })}
              autoFocus={startEditing}
            />
          </>
        ) : (
          <>
            <h2 className="m-view-title" onClick={() => setEditing(true)}>
              {draft.title || <em>Untitled</em>}
            </h2>
            {entry.tags.length > 0 && <div className="m-row-tags">{entry.tags.map((t) => `#${t}`).join("  ")}</div>}
            <div
              className="markdown m-markdown"
              onClick={handleMarkdownClick}
              dangerouslySetInnerHTML={{ __html: html || "<p class='m-faint'>Empty. Tap Edit to write.</p>" }}
            />
            <p className="m-faint">Changed {relative(entry.updatedAt)}</p>
          </>
        )}
      </main>
    </div>
  );
}

// ---------- sync ----------

function SyncScreen({
  status,
  onStatus,
  onBack,
}: {
  status: PhoneSyncStatus | null;
  onStatus: (s: PhoneSyncStatus) => void;
  onBack: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [typing, setTyping] = useState(false);
  const [code, setCode] = useState("");

  const run = async (f: () => Promise<PhoneSyncStatus>) => {
    setBusy(true);
    setError("");
    try {
      onStatus(await f());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const scan = async () => {
    setError("");
    try {
      const { scan, Format, checkPermissions, requestPermissions } = await import("@tauri-apps/plugin-barcode-scanner");
      let perm = await checkPermissions();
      if (perm !== "granted") perm = await requestPermissions();
      if (perm !== "granted") {
        setError("BrainTracker needs the camera to scan the code. You can also paste the code as text.");
        return;
      }
      const found = await scan({ windowed: false, formats: [Format.QRCode] });
      if (found?.content) run(() => syncApi.pairWith(found.content));
    } catch (e) {
      const msg = String(e);
      if (!/cancel/i.test(msg)) setError(msg);
    }
  };

  const unpair = () => {
    if (confirm("Stop syncing with this PC? The notes stay on this phone.")) run(syncApi.unpair);
  };

  return (
    <div className="m-screen">
      <header className="m-bar">
        <button className="m-icon" onClick={onBack} aria-label="Back">
          ←
        </button>
        <h1>Sync with PC</h1>
      </header>
      <main className="m-pane">
        {status?.paired ? (
          <>
            <p>
              Paired with <strong>{status.pcName}</strong>.
            </p>
            <p className="m-faint">
              {status.syncing
                ? "Syncing now…"
                : status.lastSync
                  ? `Last synced ${relative(status.lastSync)}.`
                  : "Not synced yet."}{" "}
              Notes sync when you open the app, after you edit, and every minute or so while it’s open, whenever the
              phone and the PC are on the same Wi-Fi.
            </p>
            {status.offline && <p className="m-warn">Offline mode is on, so nothing syncs.</p>}
            {status.lastError && !status.syncing && <p className="m-warn">{status.lastError}</p>}
            <button className="m-primary" disabled={busy || status.syncing} onClick={() => run(syncApi.syncNow)}>
              {busy || status.syncing ? "Syncing…" : "Sync now"}
            </button>
            <button className="m-secondary" disabled={busy} onClick={unpair}>
              Stop syncing with this PC
            </button>
          </>
        ) : (
          <>
            <p>Keep the notes on your PC and this phone in step, over your Wi-Fi. Nothing goes through the internet.</p>
            <ol className="m-steps">
              <li>
                On the PC, open BrainTracker → Settings → <strong>Sync with your phone</strong> → Pair a phone.
              </li>
              <li>Scan the QR code it shows.</li>
            </ol>
            <button className="m-primary" disabled={busy} onClick={scan}>
              {busy ? "Pairing…" : "Scan the QR code"}
            </button>
            {typing ? (
              <>
                <textarea
                  className="m-code"
                  placeholder="BTSYNC1:…"
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  autoCapitalize="none"
                />
                <button className="m-secondary" disabled={busy || !code.trim()} onClick={() => run(() => syncApi.pairWith(code))}>
                  Pair
                </button>
              </>
            ) : (
              <button className="m-link" onClick={() => setTyping(true)}>
                Paste the code as text instead
              </button>
            )}
          </>
        )}
        {error && <p className="m-warn">{error}</p>}
      </main>
    </div>
  );
}

// ---------- trash ----------

function TrashScreen({ outside, onBack }: { outside: number; onBack: () => void }) {
  const [items, setItems] = useState<TrashItem[]>([]);
  const load = useCallback(() => trashApi.list().then(setItems).catch(() => {}), []);
  useEffect(() => {
    load();
  }, [load, outside]);

  const act = async (f: () => Promise<unknown>) => {
    await f();
    syncSoon();
    load();
  };

  return (
    <div className="m-screen">
      <header className="m-bar">
        <button className="m-icon" onClick={onBack} aria-label="Back">
          ←
        </button>
        <h1>Trash</h1>
        <span className="m-bar-fill" />
        {items.length > 0 && (
          <button
            className="m-text-btn"
            onClick={() => confirm("Delete everything in the trash for good?") && act(trashApi.empty)}
          >
            Empty
          </button>
        )}
      </header>
      <main className="m-list">
        {items.map((t) => (
          <div key={t.id} className="m-row static">
            <div className="m-row-top">
              <span className="m-row-title">{t.title || <em>Untitled</em>}</span>
              <span className="m-row-when">{shortWhen(t.deletedAt)}</span>
            </div>
            {t.snippet && <div className="m-row-snippet">{t.snippet}</div>}
            <div className="m-row-actions">
              <button onClick={() => act(() => trashApi.restore(t.id))}>Restore</button>
              <button className="danger" onClick={() => act(() => trashApi.purge(t.id))}>
                Delete for good
              </button>
            </div>
          </div>
        ))}
        {items.length === 0 && <div className="m-empty">The trash is empty. Notes stay here for 30 days.</div>}
      </main>
    </div>
  );
}
