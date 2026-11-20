import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { api, type Entry, type EntrySummary } from "./api";
import { longDate, shortWhen } from "./format";
import { handleMarkdownClick, renderMarkdown } from "./markdown";
import Snippet from "./Snippet";

const WIDTH = 680;
/** How many recent entries to show before you've typed anything. */
const RECENT = 6;

/**
 * Quick search (system-wide shortcut, configurable in Settings): a Spotlight-style bar.
 * Type to see matching entries below it; Enter expands one to read in full,
 * Enter again (or Ctrl+O) opens it in BrainTracker. The window grows and shrinks with its content.
 */
export default function Spotlight() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<EntrySummary[]>([]);
  /** No note has every word, so these are the notes matching the most of them. */
  const [closest, setClosest] = useState(false);
  const [sel, setSel] = useState(0);
  const [open, setOpen] = useState<Entry | null>(null);
  const [error, setError] = useState("");

  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const noteRef = useRef<HTMLDivElement>(null);
  const seq = useRef(0);

  const search = useCallback(async (q: string) => {
    const n = ++seq.current;
    try {
      const text = q.trim();
      const all = await api.listEntries(text || null, null);
      if (n !== seq.current) return; // a newer search already started
      let shown = text ? all : all.slice(0, RECENT);
      // Searching for a whole error message (e.g. the selected text): if no note has every
      // word, show the ones that match the most of them instead of nothing.
      let loose = false;
      if (!all.length && text.split(/\s+/).length > 1) {
        const any = await api.searchAny(text);
        if (n !== seq.current) return;
        if (any.length) {
          shown = any;
          loose = true;
        }
      }
      setResults(shown);
      setClosest(loose);
      setSel(0);
      setError("");
    } catch (e) {
      if (n === seq.current) setError(String(e));
    }
  }, []);

  useEffect(() => {
    const t = setTimeout(() => search(query), 60);
    return () => clearTimeout(t);
  }, [query, search]);

  // Every time the popup is summoned: back to the list, fresh results, query selected.
  useEffect(() => {
    const onShown = async () => {
      setOpen(null);
      // `bt search <words> --open` hands over a query to start with.
      const q = await api.takePending("search").catch(() => null);
      if (q !== null) setQuery(q);
      else search(inputRef.current?.value ?? "");
      setTimeout(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      }, 0);
    };
    onShown();
    const un = listen("spotlight-shown", onShown);
    const onFocus = () => inputRef.current?.focus();
    window.addEventListener("focus", onFocus);
    return () => {
      un.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  }, [search]);

  // Size the window to fit whatever is showing, so the bar stays still and content grows below it.
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const win = getCurrentWindow();
    let last = 0;
    const fit = () => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (h && h !== last) {
        last = h;
        win.setSize(new LogicalSize(WIDTH, h)).catch(() => {});
      }
    };
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    fit();
    return () => ro.disconnect();
  }, []);

  // Keep the highlighted row in view.
  useEffect(() => {
    listRef.current?.querySelector(".spot-row.sel")?.scrollIntoView({ block: "nearest" });
  }, [sel, results]);

  const expand = async (id: number) => {
    try {
      const e = await api.getEntry(id);
      if (e) {
        setOpen(e);
        noteRef.current?.scrollTo(0, 0);
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const move = (delta: number) => {
    if (!results.length) return;
    const next = (sel + delta + results.length) % results.length;
    setSel(next);
    if (open) expand(results[next].id);
  };

  const openInApp = (id: number | null) => api.spotlightOpen(id);
  const popOut = (id: number | null) => {
    if (id === null) return;
    api.popOut(id).catch((e) => setError(String(e)));
    api.hideSpotlight();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return;
    const current = results[sel];
    const run = (fn: () => void) => {
      e.preventDefault();
      fn();
    };
    if (e.key === "ArrowDown") run(() => move(1));
    else if (e.key === "ArrowUp") run(() => move(-1));
    else if (e.key === "PageDown" && open) run(() => noteRef.current?.scrollBy({ top: 240, behavior: "smooth" }));
    else if (e.key === "PageUp" && open) run(() => noteRef.current?.scrollBy({ top: -240, behavior: "smooth" }));
    else if (e.key === "Enter" && (e.ctrlKey || open)) run(() => openInApp(open?.id ?? current?.id ?? null));
    else if (e.key === "Enter") run(() => current && expand(current.id));
    else if (e.code === "KeyO" && e.ctrlKey) run(() => openInApp(open?.id ?? current?.id ?? null));
    else if (e.code === "KeyP" && e.ctrlKey) run(() => popOut(open?.id ?? current?.id ?? null));
    else if (e.key === "Escape")
      run(() => {
        if (open) setOpen(null);
        else if (query) setQuery("");
        else api.hideSpotlight();
      });
  };

  const html = useMemo(() => (open ? renderMarkdown(open.body) : ""), [open]);
  const searching = !!query.trim();

  return (
    <div className="spot" ref={rootRef} onKeyDown={onKeyDown}>
      <div className="spot-bar" data-tauri-drag-region>
        <span className="spot-icon" aria-hidden data-tauri-drag-region>
          ⌕
        </span>
        <input
          ref={inputRef}
          autoFocus
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setOpen(null);
          }}
          placeholder="Search your notes…"
          spellCheck={false}
          aria-label="Search your notes"
        />
        {open && (
          <button className="tool" onClick={() => setOpen(null)} title="Back to results (Esc)">
            ← Back
          </button>
        )}
      </div>

      {open ? (
        <div className="spot-note">
          <header className="spot-note-head">
            <div className="spot-note-title">{open.title || <em>Untitled</em>}</div>
            <div className="spot-note-meta">
              {open.tags.map((t) => (
                <span key={t} className="spot-tag">
                  #{t}
                </span>
              ))}
              <span>{longDate(open.updatedAt)}</span>
            </div>
          </header>
          <div
            ref={noteRef}
            className="markdown spot-markdown"
            onClick={(e) => handleMarkdownClick(e)}
            dangerouslySetInnerHTML={{ __html: html }}
          />
          <footer className="spot-foot">
            <span>
              <kbd>↑</kbd> <kbd>↓</kbd> next note · <kbd>Enter</kbd> edit · <kbd>Ctrl</kbd>+<kbd>P</kbd> pop out ·{" "}
              <kbd>Esc</kbd> back
            </span>
            <span className="spot-actions">
              <button className="tool" onClick={() => popOut(open.id)} title="Keep it open on top while you work (Ctrl+P)">
                ⧉ Pop out
              </button>
              <button className="primary small" onClick={() => openInApp(open.id)}>
                Open in BrainTracker ↗
              </button>
            </span>
          </footer>
        </div>
      ) : (
        (results.length > 0 || searching || error) && (
          <>
            <div className="spot-list" ref={listRef}>
              {!searching && results.length > 0 && <div className="spot-group">Recent</div>}
              {searching && closest && (
                <div className="spot-group">No note has all of those words. Closest matches</div>
              )}
              {results.map((r, i) => (
                <button
                  key={r.id}
                  className={`spot-row ${i === sel ? "sel" : ""}`}
                  onMouseMove={() => i !== sel && setSel(i)}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => {
                    setSel(i);
                    expand(r.id);
                  }}
                >
                  <div className="spot-row-top">
                    <span className="spot-row-title">
                      {r.pinned && <span className="pin-dot">●</span>}
                      {r.title || <em>Untitled</em>}
                    </span>
                    {r.tags.slice(0, 3).map((t) => (
                      <span key={t} className="spot-tag">
                        #{t}
                      </span>
                    ))}
                    <span className="spot-when">{shortWhen(r.updatedAt)}</span>
                  </div>
                  {r.snippet && (
                    <div className="spot-snippet">
                      <Snippet text={r.snippet} />
                    </div>
                  )}
                </button>
              ))}
              {searching && !results.length && !error && (
                <div className="spot-empty">No notes match “{query.trim()}”.</div>
              )}
              {error && <div className="spot-empty bad">{error}</div>}
            </div>
            <footer className="spot-foot">
              <span>
                <kbd>↑</kbd> <kbd>↓</kbd> choose · <kbd>Enter</kbd> show · <kbd>Ctrl</kbd>+<kbd>Enter</kbd> open in
                BrainTracker · <kbd>Esc</kbd> {searching ? "clear" : "close"}
              </span>
              {searching && (
                <span>
                  {results.length} {results.length === 1 ? "note" : "notes"}
                </span>
              )}
            </footer>
          </>
        )
      )}
    </div>
  );
}
