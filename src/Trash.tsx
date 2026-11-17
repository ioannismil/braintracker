import { useEffect, useState } from "react";
import { trashApi, type TrashItem } from "./api";
import { longDate, relative } from "./format";

interface Props {
  onClose: () => void;
  /** Called after an entry is restored (so the app can open it) */
  onRestored: (id: number) => void;
  /** Called after anything changes (to refresh counts) */
  onChanged: () => void;
}

const DAY = 86_400_000;
const KEEP_DAYS = 30;

export default function Trash({ onClose, onRestored, onChanged }: Props) {
  const [items, setItems] = useState<TrashItem[] | null>(null);
  const [confirm, setConfirm] = useState<number | "all" | null>(null);

  const load = () => trashApi.list().then(setItems);

  useEffect(() => {
    load();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const restore = async (id: number) => {
    await trashApi.restore(id);
    onChanged();
    onRestored(id);
  };

  const purge = async (id: number) => {
    await trashApi.purge(id);
    setConfirm(null);
    await load();
    onChanged();
  };

  const emptyAll = async () => {
    await trashApi.empty();
    setConfirm(null);
    await load();
    onChanged();
  };

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Trash">
        <header>
          <h2>Trash</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </header>

        {items === null ? null : items.length === 0 ? (
          <p className="empty-note">The trash is empty. Deleted entries stay here for {KEEP_DAYS} days.</p>
        ) : (
          <>
            <ul className="trash-list">
              {items.map((t) => {
                const daysLeft = Math.max(0, Math.ceil((t.deletedAt + KEEP_DAYS * DAY - Date.now()) / DAY));
                return (
                  <li key={t.id}>
                    <div className="trash-main">
                      <strong>{t.title.trim() || <em>Untitled</em>}</strong>
                      {t.snippet && <span className="trash-snippet">{t.snippet}</span>}
                      <span className="trash-when" title={longDate(t.deletedAt)}>
                        Deleted {relative(t.deletedAt)} · removed for good in {daysLeft} day{daysLeft === 1 ? "" : "s"}
                      </span>
                    </div>
                    <div className="trash-actions">
                      {confirm === t.id ? (
                        <>
                          <button className="tool danger" onClick={() => purge(t.id)}>
                            Delete forever
                          </button>
                          <button className="tool" onClick={() => setConfirm(null)}>
                            Cancel
                          </button>
                        </>
                      ) : (
                        <>
                          <button className="tool on" onClick={() => restore(t.id)}>
                            Restore
                          </button>
                          <button className="tool" onClick={() => setConfirm(t.id)} title="Delete forever">
                            ✕
                          </button>
                        </>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
            <div className="dialog-foot">
              <span className="hint">Entries are removed for good {KEEP_DAYS} days after you delete them.</span>
              {confirm === "all" ? (
                <span>
                  <button className="tool danger" onClick={emptyAll}>
                    Delete all {items.length} for good
                  </button>
                  <button className="tool" onClick={() => setConfirm(null)}>
                    Cancel
                  </button>
                </span>
              ) : (
                <button className="tool" onClick={() => setConfirm("all")}>
                  Empty trash
                </button>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
