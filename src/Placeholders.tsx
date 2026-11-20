import { useEffect, useState } from "react";
import { prefsApi, type PlaceholderConfig } from "./api";

/** Settings tab: fill-in blanks like {{host}} in saved commands. */
export default function Placeholders() {
  const [cfg, setCfg] = useState<PlaceholderConfig | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    prefsApi.getPlaceholders().then(setCfg).catch((e) => setError(String(e)));
  }, []);

  if (!cfg) return error ? <p className="msg bad">{error}</p> : null;

  const save = async (next: PlaceholderConfig) => {
    setCfg(next);
    try {
      setCfg(await prefsApi.setPlaceholders(next));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  };

  const names = Object.keys(cfg.values).sort((a, b) => a.localeCompare(b));
  const setValue = (name: string, value: string) => setCfg({ ...cfg, values: { ...cfg.values, [name]: value } });
  const forget = (name: string) => {
    const values = { ...cfg.values };
    delete values[name];
    save({ ...cfg, values });
  };

  return (
    <>
      <div className="setting block">
        <strong>Fill-in blanks in commands</strong>
        <p>
          Write a blank as <code>{"{{name}}"}</code> in a code block, or <code>{"{{name:default}}"}</code> with a
          default. When you click <b>Copy</b>, BrainTracker asks for the values and copies the finished command.
        </p>
        <pre className="ph-example">
          {"ssh {{user}}@{{host}} -p {{port:22}}\nkubectl -n {{namespace}} rollout restart deploy/{{app}}"}
        </pre>
        <p className="hint">
          GitHub Actions expressions like <code>{"${{ secrets.X }}"}</code> aren’t treated as blanks. If a block uses
          {" {{ }} "}for something else, the form has a “Copy as is” button.
        </p>
      </div>

      <div className="setting">
        <div>
          <strong>Ask for the blanks when copying</strong>
          <p>Turn off to always copy code exactly as written.</p>
        </div>
        <button
          className={`switch ${cfg.ask ? "on" : ""}`}
          role="switch"
          aria-checked={cfg.ask}
          aria-label="Ask for the blanks when copying"
          onClick={() => save({ ...cfg, ask: !cfg.ask })}
        >
          <span />
        </button>
      </div>

      <div className="setting">
        <div>
          <strong>Remember what I type</strong>
          <p>
            Suggests the last value for each blank next time. Blanks whose names look secret (password, token, key,
            secret…) are never remembered.
          </p>
        </div>
        <button
          className={`switch ${cfg.remember ? "on" : ""}`}
          role="switch"
          aria-checked={cfg.remember}
          aria-label="Remember what I type"
          onClick={() => save({ ...cfg, remember: !cfg.remember })}
        >
          <span />
        </button>
      </div>

      <div className="setting block">
        <div className="setting-head">
          <div>
            <strong>Remembered values</strong>
            <p>{names.length ? "Edit or remove them here." : "None yet. They appear after you copy a command with blanks."}</p>
          </div>
          {names.length > 0 && (
            <button className="tool" onClick={() => save({ ...cfg, values: {} })}>
              Forget all
            </button>
          )}
        </div>
        {names.length > 0 && (
          <div className="ph-values">
            {names.map((n) => (
              <div key={n} className="ph-value-row">
                <code className="ph-value-name">{n}</code>
                <input
                  value={cfg.values[n]}
                  spellCheck={false}
                  onChange={(e) => setValue(n, e.target.value)}
                  onBlur={() => save(cfg)}
                  onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                  aria-label={`Value for ${n}`}
                />
                <button className="icon-btn" onClick={() => forget(n)} aria-label={`Forget ${n}`} title="Forget">
                  ✕
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
      {error && <p className="msg bad">{error}</p>}
    </>
  );
}
