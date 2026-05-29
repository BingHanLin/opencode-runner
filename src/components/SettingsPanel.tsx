import { useEffect, useState } from "react";
import { api, pickFile } from "../api";
import type { BinaryStatus, Settings } from "../types";
import { FolderIcon } from "./Icon";

interface Props {
  settings: Settings;
  onSave: (settings: Settings) => Promise<void>;
}

export function SettingsPanel({ settings, onSave }: Props) {
  const [draft, setDraft] = useState<Settings>(settings);
  const [status, setStatus] = useState<BinaryStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => setDraft(settings), [settings]);

  useEffect(() => {
    api.opencodeBinaryStatus().then(setStatus).catch(() => setStatus(null));
  }, [settings]);

  const dirty = draft.opencode_binary !== settings.opencode_binary;

  async function browse() {
    const p = await pickFile();
    if (p) setDraft({ ...draft, opencode_binary: p });
  }

  async function save() {
    setBusy(true);
    setMessage(null);
    try {
      await onSave(draft);
      const next = await api.opencodeBinaryStatus();
      setStatus(next);
      setMessage("Saved.");
    } catch (e) {
      setMessage(`Save failed: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel panel-pad-top">
      <h2 className="content-title" style={{ marginBottom: 12 }}>
        Settings
      </h2>

      <section className="section">
        <div className="section-title">opencode binary</div>
        <div className="field">
          <label className="field-label">
            Absolute path to the `opencode` binary
          </label>
          <div className="row">
            <input
              className="input grow"
              value={draft.opencode_binary ?? ""}
              placeholder="(leave empty to fall back to PATH lookup)"
              onChange={(e) =>
                setDraft({
                  ...draft,
                  opencode_binary: e.target.value || null,
                })
              }
            />
            <button className="btn" onClick={browse}>
              <FolderIcon size={13} />
              Browse…
            </button>
          </div>
          <div className="help" style={{ marginTop: 8 }}>
            Production setups should set this explicitly — PATH lookup is
            vulnerable to PATH hijacking.
          </div>
        </div>

        {status && (
          <div
            className={`chip ${status.honored_configured ? "success" : "warn"}`}
          >
            resolved: {status.resolved_path}
          </div>
        )}

        <div
          className="row"
          style={{ marginTop: 14, justifyContent: "flex-end", gap: 8 }}
        >
          <button
            className="btn"
            disabled={!dirty || busy}
            onClick={() => setDraft(settings)}
          >
            Revert
          </button>
          <button
            className="btn primary"
            disabled={!dirty || busy}
            onClick={save}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
        {message && (
          <div className="help" style={{ marginTop: 8 }}>
            {message}
          </div>
        )}
      </section>
    </div>
  );
}
