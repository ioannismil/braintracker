import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, parseTags, type TagCount } from "./api";
import { imageFiles, imageNames, insertImages, IMG_BASE } from "./images";
import {
  ACTIONS,
  comboFromEvent,
  DEFAULT_SHORTCUTS,
  hasCommandModifier,
  pretty,
  shortcutsApi,
  type Shortcuts,
  type ShortcutsInfo,
} from "./shortcuts";

/**
 * The quick-capture popup (system-wide shortcut, configurable in Settings).
 * Its own keys (save, save & close, jump to notes, open app, close) are configurable too.
 * Text survives if you dismiss it, so nothing is lost by clicking away.
 */
export default function Capture() {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [tagsText, setTagsText] = useState("");
  const [tags, setTags] = useState<TagCount[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");
  const [sc, setSc] = useState<Shortcuts>(DEFAULT_SHORTCUTS);

  const titleRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const tagsRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    shortcutsApi.get().then((i) => setSc(i.shortcuts)).catch(() => {});
    const un = listen<ShortcutsInfo>("shortcuts-changed", (e) => setSc(e.payload.shortcuts));
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!saved) return;
    const t = setTimeout(() => setSaved(""), 2500);
    return () => clearTimeout(t);
  }, [saved]);

  useEffect(() => {
    const onShown = () => {
      setError("");
      setSaved("");
      // Text selected in the app you were in (capture-selection shortcut or --capture-selection).
      api
        .takePending("selection")
        .then((sel) => sel && setBody((b) => (b.trim() ? `${b.trimEnd()}\n\n${sel}` : sel)))
        .catch(() => {});
      api.listTags().then(setTags).catch(() => {});
      setTimeout(() => {
        titleRef.current?.focus();
        titleRef.current?.select();
      }, 0);
    };
    onShown();
    const un = listen("capture-shown", onShown);
    return () => {
      un.then((f) => f());
    };
  }, []);

  const empty = !title.trim() && !body.trim();

  /** Saves the entry; `close` hides the popup, otherwise it clears for the next one. */
  const save = async (close: boolean) => {
    if (busy) return;
    if (empty) {
      if (close) api.hideCapture();
      return;
    }
    setBusy(true);
    try {
      const finalTitle = title.trim() || body.trim().split("\n")[0].replace(/!\[[^\]]*\]\([^)]*\)/g, "").slice(0, 80).trim() || "Untitled";
      await api.createEntry(finalTitle, parseTags(tagsText), body);
      setTitle("");
      setBody("");
      setTagsText("");
      setError("");
      if (close) {
        await api.captureDone(null);
      } else {
        await api.captureSaved();
        setSaved(finalTitle);
        api.listTags().then(setTags).catch(() => {});
        titleRef.current?.focus();
      }
    } catch (e) {
      setError(`Couldn’t save: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  // Tag suggestions for the fragment being typed
  const parts = tagsText.split(",");
  const frag = parts[parts.length - 1].trim().toLowerCase().replace(/^#/, "");
  const have = new Set(parseTags(tagsText));
  const suggestions = frag
    ? tags.map((t) => t.tag).filter((t) => !have.has(t) && t.startsWith(frag)).slice(0, 5)
    : [];
  const pickTag = (t: string) => {
    const p = tagsText.split(",").map((s) => s.trim());
    p[p.length - 1] = t;
    setTagsText(p.filter(Boolean).join(", ") + ", ");
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return;
    const combo = comboFromEvent(e.nativeEvent);
    if (!combo) return;
    const inNotes = e.target === bodyRef.current;
    const inTags = e.target === tagsRef.current;

    // Enter in the tags box completes the suggested tag first.
    if (inTags && combo === "Enter" && suggestions.length) {
      e.preventDefault();
      pickTag(suggestions[0]);
      return;
    }
    // Plain Enter / Shift+Enter type a new line in the notes box rather than firing a shortcut.
    const fires = (c: string) => c === combo && !(inNotes && c.endsWith("Enter") && !hasCommandModifier(c));
    const run = (fn: () => void) => {
      e.preventDefault();
      fn();
    };

    if (fires(sc.captureSaveClose)) run(() => save(true));
    else if (fires(sc.captureSave)) run(() => save(false));
    else if (fires(sc.captureNotes)) run(() => bodyRef.current?.focus());
    else if (fires(sc.captureOpenApp)) run(() => api.captureDone(null, true));
    else if (fires(sc.captureHide)) run(() => api.hideCapture());
    // Ctrl+Enter always saves (handy from the notes box) unless it's bound to something else.
    else if (combo === "Ctrl+Enter" && !ACTIONS.some((a) => a.scope === "capture" && sc[a.id] === combo))
      run(() => save(false));
  };

  const addImages = async (files: File[], e: React.SyntheticEvent) => {
    if (!files.length || !bodyRef.current) return;
    e.preventDefault();
    try {
      const v = await insertImages(bodyRef.current, files);
      if (v !== null) setBody(v);
    } catch (err) {
      setError(`Couldn’t attach image: ${err}`);
    }
  };

  const images = imageNames(body);

  return (
    <div className="capture" onKeyDown={onKeyDown}>
      <header className="capture-head" data-tauri-drag-region>
        <span className="logo" aria-hidden data-tauri-drag-region>
          ✓
        </span>
        <span className="capture-label" data-tauri-drag-region>
          Quick capture
        </span>
        <span className="spacer" data-tauri-drag-region />
        <button
          className="tool"
          onClick={() => api.captureDone(null, true)}
          title={`Open BrainTracker (${pretty(sc.captureOpenApp)})`}
        >
          Open app ↗
        </button>
        <button className="icon-btn" onClick={() => api.hideCapture()} title={`Close (${pretty(sc.captureHide)})`}>
          ✕
        </button>
      </header>

      <input
        ref={titleRef}
        className="capture-title"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        placeholder="What did you just do?"
        spellCheck={false}
      />

      <textarea
        ref={bodyRef}
        className="capture-body-input"
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onPaste={(e) => addImages(imageFiles(e.clipboardData), e)}
        onDrop={(e) => addImages(imageFiles(e.dataTransfer), e)}
        onDragOver={(e) => e.preventDefault()}
        placeholder="How? (optional) — commands, steps, links. Paste screenshots with Ctrl+V."
        spellCheck={false}
      />

      {images.length > 0 && (
        <div className="thumbs small">
          {images.map((n) => (
            <img key={n} src={IMG_BASE + n} alt="" />
          ))}
        </div>
      )}

      <div className="capture-tags">
        <span className="hash">#</span>
        <input
          ref={tagsRef}
          value={tagsText}
          onChange={(e) => setTagsText(e.target.value)}
          placeholder="tags (optional)"
          spellCheck={false}
        />
        {suggestions.map((t) => (
          <button key={t} className="chip" onMouseDown={(e) => e.preventDefault()} onClick={() => pickTag(t)}>
            #{t}
          </button>
        ))}
      </div>

      <footer className="capture-foot">
        {error ? (
          <span className="bad">{error}</span>
        ) : saved ? (
          <span className="good">✓ Saved “{saved}”</span>
        ) : (
          <span>
            <kbd>{pretty(sc.captureSave)}</kbd> save · <kbd>{pretty(sc.captureSaveClose)}</kbd> save &amp; close ·{" "}
            <kbd>{pretty(sc.captureNotes)}</kbd> notes · <kbd>{pretty(sc.captureHide)}</kbd> close
          </span>
        )}
        <span className="capture-actions">
          <button className="tool" disabled={busy || empty} onClick={() => save(false)}>
            Save
          </button>
          <button className="primary small" disabled={busy || empty} onClick={() => save(true)}>
            Save &amp; close
          </button>
        </span>
      </footer>
    </div>
  );
}
