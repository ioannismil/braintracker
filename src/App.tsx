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
  updatedAt: number;
  dirty: boolean;
};
type SaveState = "idle" | "saving" | "saved" | "error";

const toDraft = (e: Entry): Draft => ({
  id: e.id,
  title: e.title,
  body: e.body,
  tagsText: e.tags.join(", "),
  pinned: e.pinned,
  createdAt: e.createdAt,
  updatedAt: e.updatedAt,
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
    registered: ["showMain", "capture", "captureSelection", "spotlight"],
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
    if (!d || !d.dirty) return;
    d.dirty = false;
    setSaveState("saving");
    try {
      const saved = await api.updateEntry(d.id, d.title, d.body, parseTags(d.tagsText));
      const cur = draftRef.current;
      if (saved && cur && cur.id === saved.id) setCurrent({ ...cur, updatedAt: saved.updatedAt });
      setSaveState("saved");
    } catch (e) {
      console.error(e);
      d.dirty = true;
      setSaveState("error");
    }
    refresh();
  }, [refresh]);

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
      setSaveState("idle");
      refresh();
    }
    if (focusBody) setTimeout(() => bodyRef.current?.focus(), 0);
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
    const unlisteners = [
      listen("summoned", () => focusSearch()),
      // Something was saved from the quick-capture popup
      listen("entries-changed", () => refresh()),
      listen<number>("open-entry", (e) => openRef.current(e.payload, true)),
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
      unlisteners.forEach((u) => u.then((f) => f()));
      window.clearInterval(clock);
    };
  }, [flush, refresh]);

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
    saveState === "saving" ? "Saving…" : saveState === "error" ? "Couldn’t save — retrying on next edit" : draft?.dirty ? "Editing" : "Saved";

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
              <span className={`status ${saveState === "error" ? "bad" : ""}`}>{status}</span>
              <div className="spacer" />
              <button className={`tool ${draft.pinned ? "on" : ""}`} onClick={togglePin} title="Pin to top">
                {draft.pinned ? "★ Pinned" : "☆ Pin"}
              </button>
              <button className={`tool ${preview ? "on" : ""}`} onClick={() => setPreview((p) => !p)} title={sc.preview}>
                {preview ? "✎ Edit" : "◉ Preview"}
              </button>
              <button className="tool" onClick={openHistory} title="Earlier versions of this entry">
                ↺ History
              </button>
              <button className="tool" onClick={deleteCurrent} title="Move to trash (you can undo)">
                🗑
              </button>
            </div>

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
