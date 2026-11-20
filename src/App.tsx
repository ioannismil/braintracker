import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, parseTags, trashApi, type Entry, type EntrySummary, type TagCount } from "./api";
import { groupLabel, longDate, relative, shortWhen } from "./format";
import Settings, { type SettingsTab } from "./Settings";
import UpdateBanner from "./UpdateBanner";
import Trash from "./Trash";
import History from "./History";
import Snippet from "./Snippet";
import type { UpdateInfo } from "./api";
import { imageFiles, imageNames, insertImages, IMG_BASE } from "./images";
import { mergeNote, type Version } from "./merge";
import { handleMarkdownClick, isInsideCopyButton, renderMarkdown } from "./markdown";
import {
  DEFAULT_SHORTCUTS,
  matches,
  pretty,
  shortcutsApi,
  type ShortcutsInfo,
} from "./shortcuts";

const SEARCH_TIPS = ["tag:docker", "has:image", "has:code", "after:2026-10", "on:2026-11-17", "is:pinned"];

type Draft = {
  id: number;
  title: string;
  body: string;
  tagsText: string;
  pinned: boolean;
  createdAt: number;
  /** The saved version this draft is based on (to notice changes made elsewhere, e.g. `bt`). */
  updatedAt: number;
  /** The note as last loaded or saved, for merging with changes made elsewhere. */
  base: Version;
  dirty: boolean;
};
type SaveState = "idle" | "saving" | "saved" | "error" | "merged" | "conflict";

const versionOf = (e: Entry): Version => ({ title: e.title, body: e.body, tags: e.tags });

const toDraft = (e: Entry): Draft => ({
  id: e.id,
  title: e.title,
  body: e.body,
  tagsText: e.tags.join(", "),
  pinned: e.pinned,
  createdAt: e.createdAt,
  updatedAt: e.updatedAt,
  base: versionOf(e),
  dirty: false,
});

const isBlank = (d: Draft) => !d.title.trim() && !d.body.trim() && !d.tagsText.trim();

export default function App() {
  const [query, setQuery] = useState("");
  const [tag, setTag] = useState<string | null>(null);
  const [entries, setEntries] = useState<EntrySummary[]>([]);
  const [tags, setTags] = useState<TagCount[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  /** Saved elsewhere while you edited, in a way that can't be combined with your edit. */
  const [conflict, setConflict] = useState<Entry | null>(null);
  const conflictRef = useRef<Entry | null>(null);
  conflictRef.current = conflict;
  /** A save is on its way (its text is no longer marked dirty, but isn't stored yet). */
  const savingRef = useRef(false);
  const [preview, setPreview] = useState(false);
  /** Which Settings tab is showing, or null when Settings is closed. */
  const [settingsTab, setSettingsTab] = useState<SettingsTab | null>(null);
  const settingsOpen = settingsTab !== null;
  const [tagFocus, setTagFocus] = useState(false);
  const [searchFocus, setSearchFocus] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [trashCount, setTrashCount] = useState(0);
  const [undo, setUndo] = useState<{ id: number; title: string } | null>(null);
  const undoTimer = useRef<number | undefined>(undefined);
  const dialogOpen = settingsOpen || trashOpen || historyOpen;
  const [, tick] = useState(0);
  const [scInfo, setScInfo] = useState<ShortcutsInfo>({
    shortcuts: DEFAULT_SHORTCUTS,
    registered: ["showMain", "capture", "captureSelection", "spotlight", "spotlightSelection"],
  });
  const sc = scInfo.shortcuts;
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updateDismissed, setUpdateDismissed] = useState<string | null>(null);

  useEffect(() => {
    const un = listen<UpdateInfo>("update-available", (e) => setUpdate(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    shortcutsApi.get().then(setScInfo).catch(console.error);
    const un = listen<ShortcutsInfo>("shortcuts-changed", (e) => setScInfo(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const draftRef = useRef<Draft | null>(null);
  const saveTimer = useRef<number | undefined>(undefined);
  const filterRef = useRef({ query, tag });
  filterRef.current = { query, tag };
  const seq = useRef(0);

  const searchRef = useRef<HTMLInputElement>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  const setCurrent = (d: Draft | null) => {
    draftRef.current = d;
    setDraft(d);
  };

  // ---------- data ----------

  const refresh = useCallback(async () => {
    const my = ++seq.current;
    const { query, tag } = filterRef.current;
    const [list, t, trashed] = await Promise.all([
      api.listEntries(query.trim() || null, tag),
      api.listTags(),
      trashApi.count(),
    ]);
    if (my !== seq.current) return; // a newer search already answered
    setEntries(list);
    setTags(t);
    setTrashCount(trashed);
  }, []);

  useEffect(() => {
    const t = window.setTimeout(refresh, 90);
    return () => window.clearTimeout(t);
  }, [query, tag, refresh]);

  const flush = useCallback(async () => {
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = undefined;
    }
    const d = draftRef.current;
    if (!d || !d.dirty || conflictRef.current) return; // a conflict waits for your choice
    d.dirty = false;
    savingRef.current = true;
    setSaveState("saving");
    const tags = parseTags(d.tagsText);
    try {
      // Only saved if nobody else (bt) changed the note since this window loaded it.
      const res = await api.updateEntry(d.id, d.title, d.body, tags, d.updatedAt);
      const cur = draftRef.current;
      if (res.status === "saved") {
        if (cur && cur.id === d.id)
          setCurrent({ ...cur, updatedAt: res.entry.updatedAt, base: { title: d.title, body: d.body, tags } });
        setSaveState("saved");
      } else if (res.status === "conflict") {
        if (cur && cur.id === d.id) resolveRef.current({ ...cur, dirty: true }, res.entry);
      } else if (cur && cur.id === d.id) {
        setCurrent(null); // deleted for good elsewhere
      }
    } catch (e) {
      console.error(e);
      d.dirty = true;
      setSaveState("error");
    } finally {
      savingRef.current = false;
    }
    refresh();
  }, [refresh]);

  /**
   * The note was saved elsewhere (bt) while you were editing it here. Combine both changes
   * when that's safe (e.g. bt appended text); otherwise ask which version to keep.
   */
  const resolveConflict = (cur: Draft, theirs: Entry) => {
    if (theirs.trashed) {
      setCurrent(null);
      setUndo({ id: theirs.id, title: theirs.title.trim() || "Untitled" });
      return;
    }
    const mine: Version = { title: cur.title, body: cur.body, tags: parseTags(cur.tagsText) };
    const merged = mergeNote(cur.base, mine, versionOf(theirs));
    const rebased = { ...cur, pinned: theirs.pinned, updatedAt: theirs.updatedAt, base: versionOf(theirs) };
    if (!merged) {
      setCurrent({ ...cur, dirty: true });
      setConflict(theirs);
      setSaveState("conflict");
      return;
    }
    setCurrent({ ...rebased, title: merged.title, body: merged.body, tagsText: merged.tags.join(", "), dirty: true });
    setSaveState("merged");
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(flush, 0);
  };
  const resolveRef = useRef(resolveConflict);
  resolveRef.current = resolveConflict;

  /** Conflict: keep what's in this window (the other version stays in History). */
  const keepMine = async () => {
    const cur = draftRef.current;
    const theirs = conflictRef.current;
    if (!cur || !theirs) return;
    setConflict(null);
    const res = await api.updateEntry(cur.id, cur.title, cur.body, parseTags(cur.tagsText), theirs.updatedAt, true);
    if (res.status === "saved") {
      setCurrent({ ...draftRef.current!, updatedAt: res.entry.updatedAt, base: versionOf(res.entry), dirty: false });
      setSaveState("saved");
    } else if (res.status === "conflict") {
      resolveConflict(draftRef.current!, res.entry); // changed yet again
    }
    refresh();
  };

  const keepMineRef = useRef(keepMine);
  keepMineRef.current = keepMine;

  /** Conflict: take the version saved elsewhere (this window's text stays in History). */
  const takeTheirs = async () => {
    const cur = draftRef.current;
    const theirs = conflictRef.current;
    if (!cur || !theirs) return;
    setConflict(null);
    const tags = parseTags(cur.tagsText);
    // Store mine (theirs goes to History), then put theirs back (mine goes to History).
    const mine = await api.updateEntry(cur.id, cur.title, cur.body, tags, theirs.updatedAt, true);
    if (mine.status !== "saved") return mine.status === "conflict" ? resolveConflict(cur, mine.entry) : undefined;
    const back = await api.updateEntry(cur.id, theirs.title, theirs.body, theirs.tags, mine.entry.updatedAt, true);
    if (back.status === "saved") setCurrent(toDraft(back.entry));
    setSaveState("saved");
    refresh();
  };

  /** Another program (bt) changed the notes: reload the list and, if it changed, the open note. */
  const syncWithOutside = useCallback(async () => {
    refresh();
    const d = draftRef.current;
    if (!d) return;
    const e = await api.getEntry(d.id).catch(() => null);
    const cur = draftRef.current;
    if (!cur || cur.id !== d.id) return;
    if (!e) return setCurrent(null);
    if (e.trashed) return resolveRef.current(cur, e);
    if (e.updatedAt === cur.updatedAt) {
      if (e.pinned !== cur.pinned) setCurrent({ ...cur, pinned: e.pinned });
      return;
    }
    if (cur.dirty || savingRef.current || conflictRef.current) {
      // Unsaved edits here: the next save notices the change and merges (sooner if idle).
      if (cur.dirty && !savingRef.current && !conflictRef.current) flush();
      return;
    }
    setCurrent({ ...toDraft(e) });
  }, [refresh, flush]);

  const edit = (patch: Partial<Draft>) => {
    const cur = draftRef.current;
    if (!cur) return;
    setCurrent({ ...cur, ...patch, dirty: true });
    setSaveState("idle");
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(flush, 450);
  };

  /** Save the open entry; throw away entries that were created but never filled in. */
  const leaveCurrent = async () => {
    // Leaving with an unanswered conflict: keep this window's text (the other is in History).
    if (conflictRef.current) await keepMineRef.current();
    await flush();
    const d = draftRef.current;
    if (d && isBlank(d)) await api.discardEntry(d.id);
  };

  const openEntry = async (id: number, focusBody = false) => {
    if (draftRef.current?.id !== id) {
      await leaveCurrent();
      const e = await api.getEntry(id);
      if (!e) return refresh();
      setCurrent(toDraft(e));
      // Notes open in preview (double-click or the shortcut to edit); an empty one opens for writing.
      setPreview(!!e.body.trim());
      setSaveState("idle");
      refresh();
    }
    if (focusBody) setTimeout(() => bodyRef.current?.focus(), 0); // only exists when editing
  };

  const newEntry = async (title = "") => {
    await leaveCurrent();
    const e = await api.createEntry(title, tag ? [tag] : []);
    setCurrent(toDraft(e));
    setPreview(false);
    setSaveState("idle");
    filterRef.current = { ...filterRef.current, query: "" };
    setQuery("");
    await refresh();
    setTimeout(() => (title ? bodyRef.current : titleRef.current)?.focus(), 0);
  };

  const togglePin = async () => {
    const d = draftRef.current;
    if (!d) return;
    await api.setPinned(d.id, !d.pinned);
    setCurrent({ ...d, pinned: !d.pinned });
    refresh();
  };

  /** Moves the open entry to the trash and offers an Undo for a few seconds. */
  const deleteCurrent = async () => {
    await flush();
    const d = draftRef.current;
    if (!d) return;
    if (isBlank(d)) {
      await api.discardEntry(d.id);
    } else {
      await api.deleteEntry(d.id);
      setUndo({ id: d.id, title: d.title.trim() || "Untitled" });
      if (undoTimer.current) window.clearTimeout(undoTimer.current);
      undoTimer.current = window.setTimeout(() => setUndo(null), 8000);
    }
    setCurrent(null);
    refresh();
  };

  const undoDelete = async () => {
    if (!undo) return;
    if (undoTimer.current) window.clearTimeout(undoTimer.current);
    const id = undo.id;
    setUndo(null);
    await trashApi.restore(id);
    await refresh();
    openEntry(id);
  };

  /** Opens the note in a small always-on-top window (saved first, so it shows what you see). */
  const popOut = async () => {
    await flush();
    const d = draftRef.current;
    if (d) api.popOut(d.id).catch(console.error);
  };

  const openHistory = async () => {
    await flush();
    setHistoryOpen(true);
  };

  const focusSearch = () => {
    searchRef.current?.focus();
    searchRef.current?.select();
  };

  // ---------- global wiring ----------

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (dialogOpen) return; // dialogs handle their own keys
      if (matches(e, sc.newEntry)) {
        e.preventDefault();
        newEntry();
      } else if (matches(e, sc.search)) {
        e.preventDefault();
        focusSearch();
      } else if (matches(e, sc.preview)) {
        e.preventDefault();
        if (draftRef.current) setPreview((p) => !p);
      } else if (matches(e, sc.settings)) {
        e.preventDefault();
        setSettingsTab("general");
      } else if (matches(e, "Ctrl+S")) {
        e.preventDefault();
        flush(); // it auto-saves anyway; this just saves immediately
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const openRef = useRef(openEntry);
  openRef.current = openEntry;

  useEffect(() => {
    const onBlur = () => flush();
    window.addEventListener("blur", onBlur);
    // Notes may have been changed from the `bt` command line while the window was in the background.
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    const openPending = () =>
      api
        .takePending("open")
        .then((id) => {
          if (id) openRef.current(Number(id), true);
        })
        .catch(() => {});
    openPending();
    const unlisteners = [
      listen("summoned", () => focusSearch()),
      // Something was saved from the quick-capture popup
      listen("entries-changed", () => refresh()),
      // Notes changed by another program (the `bt` command line)
      listen("outside-change", () => syncWithOutside()),
      listen<number>("open-entry", (e) => openRef.current(e.payload, true)),
      // `bt show <id> --open`
      listen("pending-open", () => openPending()),
      // A backup was restored: drop the open entry (it may no longer exist) without saving it.
      listen("data-restored", () => {
        if (saveTimer.current) window.clearTimeout(saveTimer.current);
        saveTimer.current = undefined;
        draftRef.current = null;
        setDraft(null);
        refresh();
      }),
    ];
    const clock = window.setInterval(() => tick((n) => n + 1), 30_000);
    return () => {
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
      unlisteners.forEach((u) => u.then((f) => f()));
      window.clearInterval(clock);
    };
  }, [flush, refresh, syncWithOutside]);

  // ---------- search box keys ----------

  const onSearchKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    const idx = entries.findIndex((x) => x.id === draft?.id);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!entries.length) return;
      const next =
        e.key === "ArrowDown" ? Math.min(entries.length - 1, idx + 1) : Math.max(0, idx < 0 ? 0 : idx - 1);
      openEntry(entries[next].id);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (e.ctrlKey || !entries.length) {
        if (query.trim()) newEntry(query.trim());
        else newEntry();
      } else {
        openEntry(entries[idx >= 0 ? idx : 0].id, true);
      }
    } else if (e.key === "Escape") {
      if (query) setQuery("");
      else searchRef.current?.blur();
    }
  };

  // ---------- derived ----------

  const searching = query.trim().length > 0;

  const groups = useMemo(() => {
    if (searching) return [{ label: `${entries.length} result${entries.length === 1 ? "" : "s"}`, items: entries }];
    const out: { label: string; items: EntrySummary[] }[] = [];
    for (const e of entries) {
      const label = e.pinned ? "Pinned" : groupLabel(e.updatedAt);
      const last = out[out.length - 1];
      if (last && last.label === label) last.items.push(e);
      else out.push({ label, items: [e] });
    }
    return out;
  }, [entries, searching]);

  const html = useMemo(() => (draft && preview ? renderMarkdown(draft.body) : ""), [draft?.body, preview]);

  const images = useMemo(() => (draft ? imageNames(draft.body) : []), [draft?.body]);

  const addImages = async (files: File[], e: React.SyntheticEvent) => {
    if (!files.length || !bodyRef.current) return;
    e.preventDefault();
    try {
      const v = await insertImages(bodyRef.current, files);
      if (v !== null) edit({ body: v });
    } catch (err) {
      console.error(err);
      setSaveState("error");
    }
  };

  const tagSuggestions = useMemo(() => {
    if (!draft || !tagFocus) return [];
    const parts = draft.tagsText.split(",");
    const frag = parts[parts.length - 1].trim().toLowerCase().replace(/^#/, "");
    const have = new Set(parseTags(draft.tagsText));
    return tags
      .map((t) => t.tag)
      .filter((t) => !have.has(t) && (!frag || t.startsWith(frag)))
      .slice(0, 8);
  }, [draft, tagFocus, tags]);

  const addTag = (t: string) => {
    if (!draft) return;
    const parts = draft.tagsText.split(",").map((s) => s.trim());
    parts[parts.length - 1] = t;
    edit({ tagsText: parts.filter(Boolean).join(", ") + ", " });
  };

  const onBodyKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Tab" && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      const ta = e.currentTarget;
      ta.setRangeText("  ", ta.selectionStart, ta.selectionEnd, "end");
      edit({ body: ta.value });
    }
  };

  const status =
    saveState === "saving"
      ? "Saving…"
      : saveState === "error"
        ? "Couldn’t save — retrying on next edit"
        : saveState === "conflict"
          ? "Changed elsewhere — not saved"
          : saveState === "merged"
            ? "Merged with a change from outside this window"
            : draft?.dirty
              ? "Editing"
              : "Saved";

  // ---------- render ----------

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="logo" aria-hidden>
            ✓
          </span>
          <span className="brand-name">BrainTracker</span>
          <button className="icon-btn" title={`Settings (${sc.settings})`} onClick={() => setSettingsTab("general")}>
            ⚙
          </button>
        </div>

        <div className="search">
          <span className="search-icon" aria-hidden>
            ⌕
          </span>
          <input
            ref={searchRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onSearchKey}
            onFocus={() => setSearchFocus(true)}
            onBlur={() => setTimeout(() => setSearchFocus(false), 150)}
            placeholder="Search everything…"
            spellCheck={false}
          />
          <kbd>{pretty(sc.search)}</kbd>
        </div>
        {searchFocus && !query && (
          <div className="search-tips">
            <span>Filters:</span>
            {SEARCH_TIPS.map((t) => (
              <button
                key={t}
                className="chip"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => setQuery(t.startsWith("tag:") ? "tag:" : `${t} `)}
              >
                {t}
              </button>
            ))}
          </div>
        )}

        {update && updateDismissed !== update.version && (
          <UpdateBanner info={update} beforeInstall={flush} onDismiss={() => setUpdateDismissed(update.version)} />
        )}

        <button className="new-btn" onClick={() => newEntry(searching && !entries.length ? query.trim() : "")}>
          <span>＋ New entry</span>
          <kbd>{pretty(sc.newEntry)}</kbd>
        </button>

        {tags.length > 0 && (
          <div className="tag-bar">
            <button className={`chip ${tag === null ? "on" : ""}`} onClick={() => setTag(null)}>
              All
            </button>
            {tags.map((t) => (
              <button
                key={t.tag}
                className={`chip ${tag === t.tag ? "on" : ""}`}
                onClick={() => setTag(tag === t.tag ? null : t.tag)}
              >
                #{t.tag} <span className="count">{t.count}</span>
              </button>
            ))}
          </div>
        )}

        <nav className="list">
          {groups.map((g) => (
            <section key={g.label}>
              <h3 className="group">{g.label}</h3>
              {g.items.map((e) => (
                <button
                  key={e.id}
                  className={`row ${draft?.id === e.id ? "active" : ""}`}
                  onClick={() => openEntry(e.id)}
                >
                  <div className="row-top">
                    <span className="row-title">
                      {e.pinned && <span className="pin-dot">●</span>}
                      {e.title.trim() || <em>Untitled</em>}
                    </span>
                    <span className="row-when">{shortWhen(e.updatedAt)}</span>
                  </div>
                  {e.snippet && (
                    <div className="row-snippet">
                      <Snippet text={e.snippet} />
                    </div>
                  )}
                  {e.tags.length > 0 && (
                    <div className="row-tags">
                      {e.tags.slice(0, 4).map((t) => (
                        <span key={t}>#{t}</span>
                      ))}
                    </div>
                  )}
                </button>
              ))}
            </section>
          ))}

          {entries.length === 0 && (
            <div className="list-empty">
              {searching ? (
                <>
                  <p>Nothing matches “{query.trim()}”.</p>
                  <button className="link" onClick={() => newEntry(query.trim())}>
                    Create an entry called “{query.trim()}” <kbd>Enter</kbd>
                  </button>
                </>
              ) : tag ? (
                <p>No entries tagged #{tag}.</p>
              ) : (
                <p>Your log is empty. Hit {pretty(sc.newEntry)} and write down the first thing you did today.</p>
              )}
            </div>
          )}
        </nav>

        <footer className="sidebar-foot">
          <button className="tool" onClick={() => setTrashOpen(true)} title="Deleted entries (kept for 30 days)">
            🗑 Trash{trashCount > 0 && <span className="count"> {trashCount}</span>}
          </button>
        </footer>
      </aside>

      <main className="editor">
        {draft ? (
          <>
            <div className="toolbar">
              <span className={`status ${saveState === "error" || saveState === "conflict" ? "bad" : ""}`}>{status}</span>
              <div className="spacer" />
              <button className={`tool ${draft.pinned ? "on" : ""}`} onClick={togglePin} title="Pin to top">
                {draft.pinned ? "★ Pinned" : "☆ Pin"}
              </button>
              <button className={`tool ${preview ? "on" : ""}`} onClick={() => setPreview((p) => !p)} title={sc.preview}>
                {preview ? "✎ Edit" : "◉ Preview"}
              </button>
              <button className="tool" onClick={popOut} title="Open in a small window that stays on top, to follow along while you work">
                ⧉ Pop out
              </button>
              <button className="tool" onClick={openHistory} title="Earlier versions of this entry">
                ↺ History
              </button>
              <button className="tool" onClick={deleteCurrent} title="Move to trash (you can undo)">
                🗑
              </button>
            </div>

            {conflict && (
              <div className="conflict" role="alert">
                <span>
                  This note was changed outside this window (for example with <code>bt</code>) while you were editing,
                  and the two changes can’t be combined. Keep which one? The other stays in ↺ History.
                </span>
                <span className="conflict-actions">
                  <button className="primary small" onClick={keepMine}>
                    Keep mine
                  </button>
                  <button className="tool" onClick={takeTheirs}>
                    Use the other version
                  </button>
                </span>
              </div>
            )}

            <div className="sheet">
              <input
                ref={titleRef}
                className="title"
                value={draft.title}
                onChange={(e) => edit({ title: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    setPreview(false);
                    setTimeout(() => bodyRef.current?.focus(), 0);
                  }
                }}
                placeholder="What did you do?"
              />

              <div className="tags-field">
                <span className="hash">#</span>
                <input
                  value={draft.tagsText}
                  onChange={(e) => edit({ tagsText: e.target.value })}
                  onFocus={() => setTagFocus(true)}
                  onBlur={() => setTimeout(() => setTagFocus(false), 150)}
                  placeholder="tags, separated by commas"
                  spellCheck={false}
                />
                {tagSuggestions.length > 0 && (
                  <div className="suggest">
                    {tagSuggestions.map((t) => (
                      <button key={t} className="chip" onMouseDown={(e) => e.preventDefault()} onClick={() => addTag(t)}>
                        #{t}
                      </button>
                    ))}
                  </div>
                )}
              </div>

              <div className="meta" title={`Created ${longDate(draft.createdAt)}`}>
                Created {longDate(draft.createdAt)} · edited {relative(draft.updatedAt)}
              </div>

              {preview ? (
                <article
                  className="markdown"
                  onClick={handleMarkdownClick}
                  onDoubleClick={(e) => !isInsideCopyButton(e) && setPreview(false)}
                  dangerouslySetInnerHTML={{ __html: html }}
                />
              ) : (
                <textarea
                  ref={bodyRef}
                  className="body"
                  value={draft.body}
                  onChange={(e) => edit({ body: e.target.value })}
                  onKeyDown={onBodyKey}
                  onPaste={(e) => addImages(imageFiles(e.clipboardData), e)}
                  onDrop={(e) => addImages(imageFiles(e.dataTransfer), e)}
                  onDragOver={(e) => e.preventDefault()}
                  spellCheck={false}
                  placeholder={
                    "How did you do it?\n\nSteps, commands, links, gotchas, what you’d do differently…\nMarkdown works: `code`, ```blocks```, - lists, **bold**, [links](https://…)\nPaste or drop screenshots right here."
                  }
                />
              )}
              {!preview && images.length > 0 && (
                <div className="thumbs" title="Click to view in preview">
                  {images.map((n) => (
                    <img key={n} src={IMG_BASE + n} alt="" onClick={() => setPreview(true)} />
                  ))}
                </div>
              )}
            </div>
          </>
        ) : (
          <div className="welcome">
            <div className="welcome-mark">✓</div>
            <h1>What did you get done?</h1>
            <p>Write down what you did and how you did it, so future-you can find it in seconds.</p>
            <button className="primary" onClick={() => newEntry()}>
              ＋ New entry
            </button>
            <ul className="keys">
              <li>
                <kbd>{pretty(sc.newEntry)}</kbd> new entry
              </li>
              <li>
                <kbd>{pretty(sc.search)}</kbd> search · <kbd>↑</kbd>
                <kbd>↓</kbd> browse · <kbd>Enter</kbd> open
              </li>
              <li>
                <kbd>{pretty(sc.preview)}</kbd> toggle preview
              </li>
              <li>
                <kbd>{pretty(sc.capture)}</kbd> quick capture from anywhere
              </li>
              <li>
                <kbd>{pretty(sc.captureSelection)}</kbd> quick capture with the text you have selected
              </li>
              <li>
                <kbd>{pretty(sc.spotlight)}</kbd> quick search from anywhere
              </li>
              <li>
                <kbd>{pretty(sc.spotlightSelection)}</kbd> quick search for the text you have selected
              </li>
              <li>
                <kbd>{pretty(sc.showMain)}</kbd> show / hide this window from anywhere
              </li>
              <li>
                <button className="link" onClick={() => setSettingsTab("shortcuts")}>
                  Change shortcuts…
                </button>
              </li>
            </ul>
          </div>
        )}
      </main>

      {undo && (
        <div className="toast" role="status">
          <span>
            Moved “{undo.title}” to the trash
          </span>
          <button className="link" onClick={undoDelete}>
            Undo
          </button>
          <button className="icon-btn" onClick={() => setUndo(null)} aria-label="Dismiss">
            ✕
          </button>
        </div>
      )}

      {trashOpen && (
        <Trash
          onClose={() => setTrashOpen(false)}
          onChanged={refresh}
          onRestored={(id) => {
            setTrashOpen(false);
            openEntry(id);
          }}
        />
      )}

      {historyOpen && draft && (
        <History
          entryId={draft.id}
          onClose={() => setHistoryOpen(false)}
          onRestored={(e) => {
            setHistoryOpen(false);
            setCurrent(toDraft(e));
            setSaveState("saved");
            refresh();
          }}
        />
      )}

      {settingsOpen && (
        <Settings
          info={scInfo}
          initialTab={settingsTab ?? "general"}
          onChange={setScInfo}
          onClose={() => setSettingsTab(null)}
          onUpdateFound={(u) => {
            setUpdate(u);
            setUpdateDismissed(null);
          }}
          beforeInstall={flush}
        />
      )}
    </div>
  );
}
