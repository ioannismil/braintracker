import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, parseTags, type TagCount } from "./api";
import { imageFiles, imageNames, insertImages, IMG_BASE } from "./images";

/**
 * The quick-capture popup (Ctrl+Shift+Space from anywhere).
 * Enter on the title saves a one-liner; notes and tags are optional.
 * Text survives if you dismiss it, so nothing is lost by clicking away.
 */
export default function Capture() {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [tagsText, setTagsText] = useState("");
  const [tags, setTags] = useState<TagCount[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const titleRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const onShown = () => {
      setError("");
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

  const save = async (openAfter: boolean) => {
    if (busy) return;
    if (empty) {
      if (openAfter) api.captureDone(null, true);
      return;
    }
    setBusy(true);
    try {
      const finalTitle = title.trim() || body.trim().split("\n")[0].replace(/!\[[^\]]*\]\([^)]*\)/g, "").slice(0, 80).trim() || "Untitled";
      const e = await api.createEntry(finalTitle, parseTags(tagsText), body);
      setTitle("");
      setBody("");
      setTagsText("");
      await api.captureDone(openAfter ? e.id : null);
    } catch (e) {
      setError(`Couldn’t save: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      api.hideCapture();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      save(e.shiftKey);
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "o") {
      e.preventDefault();
      api.captureDone(null, true);
    }
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
        <button className="tool" onClick={() => api.captureDone(null, true)} title="Open BrainTracker (Ctrl+O)">
          Open app ↗
        </button>
        <button className="icon-btn" onClick={() => api.hideCapture()} title="Close (Esc)">
          ✕
        </button>
      </header>

      <input
        ref={titleRef}
        className="capture-title"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
            e.preventDefault();
            if (e.shiftKey) bodyRef.current?.focus();
            else save(false);
          }
        }}
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
          value={tagsText}
          onChange={(e) => setTagsText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.ctrlKey) {
              e.preventDefault();
              if (suggestions.length) pickTag(suggestions[0]);
              else save(false);
            }
          }}
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
        ) : (
          <span>
            <kbd>Enter</kbd> save · <kbd>Shift Enter</kbd> add notes · <kbd>Ctrl Shift Enter</kbd> save &amp; open ·{" "}
            <kbd>Esc</kbd> hide
          </span>
        )}
        <button className="primary small" disabled={busy || empty} onClick={() => save(false)}>
          Save
        </button>
      </footer>
    </div>
  );
}
