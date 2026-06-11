// Top banner that surfaces an available app update and installs it in place
// on click. Self-contained: it checks the updater endpoint once on mount and
// renders nothing unless a newer signed release exists. The whole flow
// (check → download → install → relaunch) is driven by tauri-plugin-updater.

import { useEffect, useState } from "react";
import { checkForUpdate, relaunchApp, type Update } from "../api";
import { useT } from "../LanguageProvider";
import { RefreshIcon, XIcon } from "./Icon";

type Phase = "available" | "downloading" | "ready" | "error";

export function UpdateBanner() {
  const t = useT();
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<Phase>("available");
  const [percent, setPercent] = useState(0);
  const [error, setError] = useState<string>("");
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    // Best-effort: offline / unreachable endpoint just means "no update".
    checkForUpdate()
      .then((u) => {
        if (!cancelled && u) setUpdate(u);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  if (!update || dismissed) return null;

  async function install() {
    if (!update) return;
    setPhase("downloading");
    setError("");
    setPercent(0);
    let downloaded = 0;
    let total = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) setPercent(Math.round((downloaded / total) * 100));
        } else if (event.event === "Finished") {
          setPercent(100);
        }
      });
      setPhase("ready");
      // Installer has staged the new binary; relaunch to run it.
      await relaunchApp();
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  }

  const message =
    phase === "downloading"
      ? t("update.downloading", { percent })
      : phase === "ready"
        ? t("update.ready")
        : phase === "error"
          ? t("update.error", { error })
          : t("update.available", { version: update.version });

  return (
    <div className="update-banner" role="status">
      <RefreshIcon
        size={15}
        className={phase === "downloading" ? "update-banner-icon spin" : "update-banner-icon"}
      />
      <span className="update-banner-text">{message}</span>
      <div className="update-banner-actions">
        {(phase === "available" || phase === "error") && (
          <button className="btn primary" onClick={install}>
            {phase === "error" ? t("update.retry") : t("update.install")}
          </button>
        )}
        <button
          className="btn icon ghost"
          aria-label={t("update.dismiss")}
          title={t("update.dismiss")}
          onClick={() => setDismissed(true)}
          disabled={phase === "downloading"}
        >
          <XIcon size={14} />
        </button>
      </div>
    </div>
  );
}
