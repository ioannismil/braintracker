import { useEffect, useMemo, useState } from "react";
import { historyApi, type Entry, type Version, type VersionSummary } from "./api";
import { longDate, relative } from "./format";
import { handleMarkdownClick, renderMarkdown } from "./markdown";

interface Props {
  entryId: number;
  onClose: () => void;
  onRestored: (entry: Entry) => void;
}

/** Earlier versions of one entry, with a preview and one-click restore. */
export default function History({ entryId, onClose, onRestored }: Props) {
  const [versions, setVersions] = useState<VersionSummary[] | null>(null);
  const [selected, setSelected] = useState<Version | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    historyApi.list(entryId).then((v) => {
      setVersions(v);
      if (v.length) historyApi.get(v[0].id).then(setSelected);
    });
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [entryId, onClose]);

  const html = useMemo(() => (selected ? renderMarkdown(selected.body) : ""), [selected]);

  const restore = async () => {
    if (!selected) return;
    try {
      const e = await historyApi.restore(entryId, selected.id);
      if (e) onRestored(e);
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog wide" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="History">
        <header>
          <h2>History</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </header>

        {versions === null ? null : versions.length === 0 ? (
          <p className="empty-note">
            No earlier versions yet. While you edit, BrainTracker keeps a version at most every 10 minutes (the
            last 50 per entry).
          </p>
        ) : (
          <div className="history">
            <ul className="history-list">
              {versions.map((v) => (
                <li key={v.id}>
                  <button
                    className={`history-item ${selected?.id === v.id ? "active" : ""}`}
                    onClick={() => historyApi.get(v.id).then(setSelected)}
                  >
                    <span className="history-when" title={longDate(v.savedAt)}>
                      {relative(v.savedAt)}
                    </span>
                    <span className="history-date">{longDate(v.savedAt)}</span>
                    <span className="history-snippet">{v.snippet || <em>empty</em>}</span>
                    <span className="history-size">{v.chars.toLocaleString()} characters</span>
                  </button>
                </li>
              ))}
            </ul>
            <div className="history-preview">
              {selected && (
                <>
                  <h3>{selected.title.trim() || <em>Untitled</em>}</h3>
                  {selected.tags.length > 0 && (
                    <div className="row-tags">
                      {selected.tags.map((t) => (
                        <span key={t}>#{t}</span>
                      ))}
                    </div>
                  )}
                  <article className="markdown" onClick={handleMarkdownClick} dangerouslySetInnerHTML={{ __html: html }} />
                </>
              )}
            </div>
          </div>
        )}

        {selected && (
          <div className="dialog-foot">
            <span className="hint">Your current text is kept as a version too, so restoring can be undone.</span>
            <button className="primary small" onClick={restore}>
              Restore this version
            </button>
          </div>
        )}
        {error && <p className="sc-error">{error}</p>}
      </div>
    </div>
  );
}
