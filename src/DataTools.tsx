import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { dataApi } from "./api";

function size(bytes: number) {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Settings sections: import Markdown/text, and clean up unused screenshots. */
export default function DataTools() {
  const [importing, setImporting] = useState(false);
  const [importMsg, setImportMsg] = useState<{ text: string; bad?: boolean } | null>(null);
  const [unused, setUnused] = useState<{ count: number; bytes: number } | null>(null);
  const [cleanMsg, setCleanMsg] = useState("");

  const runImport = async (paths: string[] | string | null) => {
    if (!paths || (Array.isArray(paths) && !paths.length)) return;
    setImporting(true);
    setImportMsg(null);
    try {
      const r = await dataApi.importMarkdown(Array.isArray(paths) ? paths : [paths]);
      const parts = [`Imported ${r.imported} entr${r.imported === 1 ? "y" : "ies"}`];
      if (r.skipped) parts.push(`skipped ${r.skipped} (empty or already imported)`);
      if (r.failed.length) parts.push(`${r.failed.length} failed: ${r.failed.join("; ")}`);
      setImportMsg({ text: parts.join(" · ") + ".", bad: r.failed.length > 0 && r.imported === 0 });
    } catch (e) {
      setImportMsg({ text: `Import failed: ${e}`, bad: true });
    } finally {
      setImporting(false);
    }
  };

  const importFolder = async () =>
    runImport(await open({ directory: true, title: "Import a folder of Markdown or text notes" }));

  const importFiles = async () =>
    runImport(
      await open({
        multiple: true,
        title: "Import Markdown or text files",
        filters: [{ name: "Markdown or text", extensions: ["md", "markdown", "txt"] }],
      }),
    );

  const findUnused = async () => {
    setCleanMsg("");
    try {
      const u = await dataApi.findUnusedImages();
      setUnused(u);
      if (!u.count) setCleanMsg("No unused screenshots — nothing to clean up.");
    } catch (e) {
      setCleanMsg(`Couldn’t check: ${e}`);
    }
  };

  const clean = async () => {
    try {
      const done = await dataApi.deleteUnusedImages();
      setUnused(null);
      setCleanMsg(`Removed ${done.count} unused screenshot${done.count === 1 ? "" : "s"} (${size(done.bytes)}).`);
    } catch (e) {
      setCleanMsg(`Cleanup failed: ${e}`);
    }
  };

  return (
    <>
      <div className="setting block">
        <div className="setting-head">
          <div>
            <strong>Import</strong>
            <p>
              Bring in Markdown or text notes — including a BrainTracker export. Titles, tags, dates and local images
              are picked up; files already imported are skipped.
            </p>
          </div>
          <div className="btn-row">
            <button className="tool" onClick={importFolder} disabled={importing}>
              Folder…
            </button>
            <button className="tool" onClick={importFiles} disabled={importing}>
              Files…
            </button>
          </div>
        </div>
        {importing && <p className="msg">Importing…</p>}
        {importMsg && <p className={importMsg.bad ? "sc-error" : "msg"}>{importMsg.text}</p>}
      </div>

      <div className="setting block">
        <div className="setting-head">
          <div>
            <strong>Clean up screenshots</strong>
            <p>Remove pasted images that no entry uses anymore — including the trash and edit history.</p>
          </div>
          {unused && unused.count > 0 ? (
            <div className="btn-row">
              <button className="tool danger" onClick={clean}>
                Delete {unused.count} ({size(unused.bytes)})
              </button>
              <button className="tool" onClick={() => setUnused(null)}>
                Cancel
              </button>
            </div>
          ) : (
            <button className="tool" onClick={findUnused}>
              Check
            </button>
          )}
        </div>
        {cleanMsg && <p className="msg">{cleanMsg}</p>}
      </div>
    </>
  );
}
