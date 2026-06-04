import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { api, pickDirectory } from "../api";
import type { Model, Task } from "../types";
import { parseScheduleKind } from "../types";
import {
  AlertIcon,
  FolderIcon,
  PlayIcon,
  SaveIcon,
  TrashIcon,
} from "./Icon";
import { ScheduleEditor } from "./ScheduleEditor";

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
      <div className="sticky-bar edit-toolbar">
        <div className="row" style={{ gap: 8 }}>
          <button
            className="btn primary"
            disabled={!dirty || !!validation || busy}
            onClick={save}
            title="Ctrl+S"
          >
            <SaveIcon size={14} />
            {busy ? "Saving…" : "Save"}
          </button>
          {dirty && (
            <button className="btn" onClick={() => setDraft(task)}>
              Revert
            </button>
          )}
          <button className="btn" disabled={isNew || busy} onClick={onRunNow}>
            <PlayIcon size={13} />
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
              <TrashIcon size={13} />
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
        <div className="field">
          <label className="field-label">Tags</label>
          <TagsField
            tags={draft.tags ?? []}
            onChange={(next) => set("tags", next)}
          />
        </div>
      </section>

      <section className="section">
        <div className="section-title">Schedule</div>
        <ScheduleEditor
          schedule={draft.schedule}
          onChange={(v) => set("schedule", v)}
        />
        {kind !== "manual" && (
          <label className="checkbox" style={{ marginTop: 14 }}>
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
              <FolderIcon size={13} />
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

        <div className="field">
          <label className="field-label">Timeout (minutes)</label>
          <div className="row">
            <input
              type="number"
              min={0}
              max={1440}
              step={5}
              className="input"
              style={{ maxWidth: 140 }}
              value={
                draft.timeout_secs == null
                  ? ""
                  : Math.round(draft.timeout_secs / 60)
              }
              onChange={(e) => {
                const v = e.target.value;
                if (v === "") return set("timeout_secs", null);
                const n = Math.max(0, Math.min(1440, parseInt(v, 10) || 0));
                set("timeout_secs", n === 0 ? null : n * 60);
              }}
            />
            <span className="help">
              {draft.timeout_secs && draft.timeout_secs > 0
                ? `gracefully cancel runs that exceed ${draft.timeout_secs}s`
                : "no timeout — run can take as long as opencode needs"}
            </span>
          </div>
        </div>

        <div>
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
            <div
              className="warn-text"
              style={{
                marginTop: 6,
                marginLeft: 22,
                display: "flex",
                alignItems: "center",
                gap: 6,
              }}
            >
              <AlertIcon size={13} />
              opencode will run without prompting you to allow tool calls.
            </div>
          )}
        </div>

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
              placeholder="leave empty to fork from current HEAD"
              onChange={(e) =>
                set("worktree_base", e.target.value || null)
              }
            />
            <div className="help" style={{ marginTop: 4 }}>
              When set (e.g. <code>origin/main</code>), the runner does{" "}
              <code>git fetch --all</code> first, verifies the ref, then
              creates the worktree from it; any failure aborts the run with
              no HEAD fallback.
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

function TagsField({
  tags,
  onChange,
}: {
  tags: string[];
  onChange: (next: string[]) => void;
}) {
  const [draft, setDraft] = useState("");
  function commit(raw: string) {
    const next = raw.trim().toLowerCase();
    if (!next) return;
    if (tags.includes(next)) return;
    onChange([...tags, next]);
  }
  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      commit(draft);
      setDraft("");
    } else if (e.key === "Backspace" && !draft && tags.length > 0) {
      onChange(tags.slice(0, -1));
    }
  }
  return (
    <div className="tags-input">
      {tags.map((t) => (
        <span key={t} className="chip accent tag-chip">
          {t}
          <button
            type="button"
            className="tag-chip-x"
            aria-label={`Remove tag ${t}`}
            onClick={() => onChange(tags.filter((x) => x !== t))}
          >
            ×
          </button>
        </span>
      ))}
      <input
        className="tag-input"
        value={draft}
        placeholder={tags.length ? "" : "review, daily, frontend…"}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => {
          if (draft) {
            commit(draft);
            setDraft("");
          }
        }}
      />
    </div>
  );
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
