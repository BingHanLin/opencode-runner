import { useEffect, useMemo, useState } from "react";
import { api, pickDirectory } from "../api";
import type { Model, Task } from "../types";
import { parseScheduleKind } from "../types";

interface Props {
  task: Task;
  isNew: boolean;
  onSave: (updated: Task) => Promise<void>;
  onDelete: () => Promise<void>;
  onRunNow: () => Promise<void>;
}

export function EditTab({ task, isNew, onSave, onDelete, onRunNow }: Props) {
  const [draft, setDraft] = useState<Task>(task);
  const [models, setModels] = useState<Model[]>([]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setDraft(task);
    setConfirmDelete(false);
  }, [task.id]);

  useEffect(() => {
    api
      .listModels()
      .then(setModels)
      .catch(() => setModels([]));
  }, []);

  const kind = parseScheduleKind(draft.schedule);
  const dirty = JSON.stringify(draft) !== JSON.stringify(task);
  const validation = useMemo(() => validate(draft), [draft]);

  function set<K extends keyof Task>(k: K, v: Task[K]) {
    setDraft((d) => ({ ...d, [k]: v }));
  }

  async function browseWorkingDir() {
    const p = await pickDirectory();
    if (p) set("working_dir", p);
  }

  async function save() {
    if (validation) return;
    setBusy(true);
    setMessage(null);
    try {
      await onSave(draft);
      setMessage("Saved. Scheduler restarted.");
    } catch (e) {
      setMessage(`Save failed: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <div
        className="row"
        style={{ justifyContent: "space-between", marginBottom: 14 }}
      >
        <div className="row" style={{ gap: 8 }}>
          <button
            className="btn primary"
            disabled={!dirty || !!validation || busy}
            onClick={save}
            title="Ctrl+S"
          >
            {busy ? "Saving…" : "Save"}
          </button>
          {dirty && (
            <button className="btn" onClick={() => setDraft(task)}>
              Revert
            </button>
          )}
          <button className="btn" disabled={isNew || busy} onClick={onRunNow}>
            Run now
          </button>
        </div>
        <div className="row" style={{ gap: 8 }}>
          {confirmDelete ? (
            <>
              <span className="warn-text">Delete this task?</span>
              <button className="btn danger" onClick={onDelete}>
                Confirm delete
              </button>
              <button
                className="btn"
                onClick={() => setConfirmDelete(false)}
              >
                Cancel
              </button>
            </>
          ) : (
            <button
              className="btn danger"
              disabled={isNew}
              onClick={() => setConfirmDelete(true)}
            >
              Delete
            </button>
          )}
        </div>
      </div>

      {validation && <div className="error-text">{validation}</div>}
      {message && <div className="help">{message}</div>}

      <section className="section">
        <div className="section-title">Basics</div>
        <div className="field">
          <label className="field-label">Name</label>
          <input
            className="input"
            value={draft.name}
            onChange={(e) => set("name", e.target.value)}
          />
        </div>
      </section>

      <section className="section">
        <div className="section-title">Schedule</div>
        <div className="row" style={{ gap: 6, marginBottom: 12 }}>
          {(["manual", "cron", "once"] as const).map((k) => (
            <button
              key={k}
              className={`btn ${kind === k ? "primary" : ""}`}
              onClick={() => set("schedule", switchScheduleKind(draft.schedule, k))}
            >
              {k}
            </button>
          ))}
        </div>

        {kind === "manual" && (
          <div className="help">
            Manual tasks only run when you click <strong>Run now</strong>.
          </div>
        )}

        {kind === "cron" && (
          <>
            <div className="field">
              <label className="field-label">
                Quartz cron expression (6 or 7 fields: sec min hour day month
                dow [year])
              </label>
              <input
                className="input"
                value={draft.schedule.slice(5)}
                placeholder="0 0 9 ? * MON-FRI"
                onChange={(e) => set("schedule", `cron:${e.target.value}`)}
              />
            </div>
            <div className="help">
              `day-of-month` and `day-of-week` can't both be specific values —
              put `?` in the one you don't want to constrain.
            </div>
          </>
        )}

        {kind === "once" && (
          <div className="field">
            <label className="field-label">RFC3339 timestamp</label>
            <input
              className="input"
              value={draft.schedule.slice(5)}
              placeholder="2026-06-01T09:00:00Z"
              onChange={(e) => set("schedule", `once:${e.target.value}`)}
            />
          </div>
        )}

        {kind !== "manual" && (
          <label className="checkbox" style={{ marginTop: 12 }}>
            <input
              type="checkbox"
              checked={draft.enabled}
              onChange={(e) => set("enabled", e.target.checked)}
            />
            Enabled
          </label>
        )}
      </section>

      <section className="section">
        <div className="section-title">Execution</div>
        <div className="field">
          <label className="field-label">Working directory</label>
          <div className="row">
            <input
              className="input grow"
              value={draft.working_dir}
              onChange={(e) => set("working_dir", e.target.value)}
              placeholder="C:/projects/example"
            />
            <button className="btn" onClick={browseWorkingDir}>
              Browse…
            </button>
          </div>
        </div>

        <div className="field">
          <label className="field-label">Model</label>
          <select
            className="select"
            value={draft.model ?? ""}
            onChange={(e) => set("model", e.target.value || null)}
          >
            <option value="">(opencode default)</option>
            {models.map((m) => {
              const v = `${m.provider_id}/${m.model_id}`;
              return (
                <option key={v} value={v}>
                  {v}
                </option>
              );
            })}
          </select>
        </div>

        <label className="checkbox">
          <input
            type="checkbox"
            checked={draft.dangerously_skip_permissions}
            onChange={(e) =>
              set("dangerously_skip_permissions", e.target.checked)
            }
          />
          --dangerously-skip-permissions
        </label>
        {draft.dangerously_skip_permissions && (
          <div className="warn-text" style={{ marginTop: 4 }}>
            ⚠ opencode will run without prompting you to allow tool calls.
          </div>
        )}

        <div style={{ marginTop: 10 }}>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={draft.run_in_worktree}
              onChange={(e) => set("run_in_worktree", e.target.checked)}
            />
            Run in throwaway git worktree
          </label>
        </div>
        {draft.run_in_worktree && (
          <div className="field" style={{ marginTop: 10 }}>
            <label className="field-label">Worktree base ref (optional)</label>
            <input
              className="input"
              value={draft.worktree_base ?? ""}
              placeholder="origin/main"
              onChange={(e) =>
                set("worktree_base", e.target.value || null)
              }
            />
            <div className="help" style={{ marginTop: 4 }}>
              When set, the runner does `git fetch --all` first, verifies the
              base, then creates the worktree from it. Leave blank to fork
              from HEAD.
            </div>
          </div>
        )}
      </section>

      <section className="section">
        <div className="section-title">Prompt</div>
        <textarea
          className="textarea"
          value={draft.prompt}
          onChange={(e) => set("prompt", e.target.value)}
          placeholder="Describe the work for opencode to do…"
        />
        <div className="help" style={{ marginTop: 6 }}>
          {draft.prompt.length} chars · {draft.prompt.split("\n").length} lines
        </div>
      </section>
    </div>
  );
}

function switchScheduleKind(
  current: string,
  next: "manual" | "cron" | "once",
): string {
  if (next === "manual") return "manual";
  const body =
    current.startsWith("cron:") || current.startsWith("once:")
      ? current.slice(5)
      : "";
  return `${next}:${body}`;
}

function validate(t: Task): string | null {
  if (!t.name.trim()) return "Name is required.";
  if (!t.working_dir.trim()) return "Working directory is required.";
  if (!t.prompt.trim()) return "Prompt is empty.";
  if (t.schedule.startsWith("cron:") && !t.schedule.slice(5).trim())
    return "Cron expression is empty.";
  if (t.schedule.startsWith("once:") && !t.schedule.slice(5).trim())
    return "Once timestamp is empty.";
  return null;
}
