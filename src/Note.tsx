import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Entry } from "./api";
import { relative } from "./format";
import { handleMarkdownClick, renderMarkdown } from "./markdown";

const id = Number(new URLSearchParams(window.location.search).get("id"));

/**
 * A note popped out into its own small window, on top of other apps by default, so you can
 * follow its steps while you work. Read-only here; "Edit" opens it in the main window.
 * It refreshes whenever the note is saved elsewhere.
 */
export default function Note() {
  const [entry, setEntry] = useState<Entry | null>(null);
  const [missing, setMissing] = useState(false);
  const [onTop, setOnTop] = useState(true);

  const load = useCallback(async () => {
    try {
      const e = await api.getEntry(id);
      setEntry(e);
      setMissing(!e);
      if (e) getCurrentWindow().setTitle(e.title.trim() || "Untitled").catch(() => {});
    } catch {
      setMissing(true);
    }
  }, []);

  useEffect(() => {
    load();
    const un = [
      listen<number>("entry-updated", (e) => e.payload === id && load()),
      listen("entries-changed", () => load()),
      listen("outside-change", () => load()), // edited with `bt`
      listen("data-restored", () => load()),
    ];
    // Edited from the `bt` command line meanwhile? Catch up when you come back to it.
    window.addEventListener("focus", load);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") getCurrentWindow().close();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      un.forEach((u) => u.then((f) => f()));
      window.removeEventListener("focus", load);
      window.removeEventListener("keydown", onKey);
    };
  }, [load]);

  const toggleOnTop = async () => {
    const next = !onTop;
    await getCurrentWindow().setAlwaysOnTop(next).catch(() => {});
    setOnTop(next);
  };

  const html = useMemo(() => (entry ? renderMarkdown(entry.body) : ""), [entry?.body]);

  if (missing) {
    return (
      <div className="note-pop">
        <div className="note-empty">This note no longer exists.</div>
      </div>
    );
  }
  if (!entry) return <div className="note-pop" />;

  return (
    <div className="note-pop">
      <header className="note-pop-head">
        <div className="note-pop-title">{entry.title.trim() || <em>Untitled</em>}</div>
        <div className="note-pop-meta">
          {entry.tags.map((t) => (
            <span key={t} className="spot-tag">
              #{t}
            </span>
          ))}
          <span>edited {relative(entry.updatedAt)}</span>
        </div>
        <div className="note-pop-tools">
          <button className={`tool ${onTop ? "on" : ""}`} onClick={toggleOnTop} title="Keep this window above other apps">
            📌 {onTop ? "On top" : "Not on top"}
          </button>
          <button className="tool" onClick={() => api.openInMain(entry.id)} title="Edit in BrainTracker">
            ✎ Edit
          </button>
        </div>
      </header>
      <article
        className="markdown note-pop-body"
        onClick={(e) => handleMarkdownClick(e)}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  );
}
