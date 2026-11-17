import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { updateApi, type UpdateInfo } from "./api";

interface Props {
  info: UpdateInfo;
  /** Save any pending edits before the app restarts. */
  beforeInstall: () => Promise<void>;
  onDismiss?: () => void;
}

export default function UpdateBanner({ info, beforeInstall, onDismiss }: Props) {
  const [state, setState] = useState<"idle" | "installing" | "error">("idle");
  const [error, setError] = useState("");

  const install = async () => {
    setState("installing");
    try {
      await beforeInstall();
      await updateApi.install(); // the app restarts when this finishes
    } catch (e) {
      setError(String(e));
      setState("error");
    }
  };

  return (
    <div className="update-banner" role="status">
      <span>
        <strong>BrainTracker {info.version}</strong> is available{" "}
        <span style={{ color: "var(--ink-2)" }}>(you have {info.current})</span>
      </span>
      {state === "error" && <span className="bad">Update failed: {error}</span>}
      <div className="actions">
        {info.portable ? (
          <button className="tool on" onClick={() => openUrl(info.releasesUrl)}>
            Download portable zip ↗
          </button>
        ) : (
          <button className="tool on" onClick={install} disabled={state === "installing"}>
            {state === "installing" ? "Downloading… the app will restart" : "Install & restart"}
          </button>
        )}
        <button className="tool" onClick={() => openUrl(info.releasesUrl)}>
          What’s new
        </button>
        {onDismiss && state !== "installing" && (
          <button className="tool" onClick={onDismiss}>
            Later
          </button>
        )}
      </div>
    </div>
  );
}
