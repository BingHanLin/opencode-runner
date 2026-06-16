import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { api, pickDirectory } from "../api";
import { useT } from "../LanguageProvider";
import type { MessageKey } from "../i18n";
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
  /** The working copy being edited; persisted by the parent across switches. */
  draft: Task;
  isNew: boolean;
  onChange: (updated: Task) => void;
  onRevert: () => void;
  onSave: (updated: Task) => Promise<void>;
  onDelete: () => Promise<void>;
  onRunNow: () => Promise<void>;
}

export function EditTab({
  task,
  draft,
  isNew,
  onChange,
  onRevert,
  onSave,
  onDelete,
  onRunNow,
}: Props) {
  const t = useT();
  const [models, setModels] = useState<Model[]>([]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setConfirmDelete(false);
    setMessage(null);
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
    onChange({ ...draft, [k]: v });
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
      setMessage(t("edit.savedRestarted"));
    } catch (e) {
      setMessage(t("settings.saveFailed", { error: String(e) }));
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
            {busy ? t("btn.saving") : t("btn.save")}
          </button>
          {dirty && (
            <button className="btn" onClick={onRevert}>
              {t("btn.revert")}
            </button>
          )}
          <button className="btn" disabled={isNew || busy} onClick={onRunNow}>
            <PlayIcon size={13} />
            {t("edit.runNow")}
          </button>
        </div>
        <div className="row" style={{ gap: 8 }}>
          {confirmDelete ? (
            <>
              <span className="warn-text">{t("edit.confirmDeleteQ")}</span>
              <button className="btn danger" onClick={onDelete}>
                {t("edit.confirmDelete")}
              </button>
              <button
                className="btn"
                onClick={() => setConfirmDelete(false)}
              >
                {t("btn.cancel")}
              </button>
            </>
          ) : (
            <button
              className="btn danger"
              disabled={isNew}
              onClick={() => setConfirmDelete(true)}
            >
              <TrashIcon size={13} />
              {t("edit.delete")}
            </button>
          )}
        </div>
      </div>

      {validation && <div className="error-text">{t(validation)}</div>}
      {message && <div className="help">{message}</div>}

      <section className="section">
        <div className="section-title">{t("edit.section.basics")}</div>
        <div className="field">
          <label className="field-label">{t("edit.name")}</label>
          <input
            className="input"
            value={draft.name}
            onChange={(e) => set("name", e.target.value)}
          />
        </div>
        <div className="field">
          <label className="field-label">{t("edit.tags")}</label>
          <TagsField
            tags={draft.tags ?? []}
            onChange={(next) => set("tags", next)}
          />
        </div>
      </section>

      <section className="section">
        <div className="section-title">{t("edit.section.schedule")}</div>
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
            {t("edit.enabled")}
          </label>
        )}
      </section>

      <section className="section">
        <div className="section-title">{t("edit.section.execution")}</div>
        <div className="field">
          <label className="field-label">{t("edit.workingDir")}</label>
          <div className="row">
            <input
              className="input grow"
              value={draft.working_dir}
              onChange={(e) => set("working_dir", e.target.value)}
              placeholder="C:/projects/example"
            />
            <button className="btn" onClick={browseWorkingDir}>
              <FolderIcon size={13} />
              {t("btn.browse")}
            </button>
          </div>
        </div>

        <div className="field">
          <label className="field-label">{t("edit.model")}</label>
          <select
            className="select"
            value={draft.model ?? ""}
            onChange={(e) => set("model", e.target.value || null)}
          >
            <option value="">{t("edit.modelDefault")}</option>
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
          <label className="field-label">{t("edit.timeout")}</label>
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
                ? t("edit.timeout.set", { secs: draft.timeout_secs })
                : t("edit.timeout.none")}
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
              {t("edit.skipPerms.warn")}
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
            {t("edit.worktree.toggle")}
          </label>
        </div>
        {draft.run_in_worktree && (
          <>
            <div className="help" style={{ marginTop: 6 }}>
              {t("edit.worktree.help")}
            </div>
            <div className="field" style={{ marginTop: 10 }}>
              <label className="field-label">{t("edit.worktree.baseLabel")}</label>
            <input
              className="input"
              value={draft.worktree_base ?? ""}
              placeholder={t("edit.worktree.basePlaceholder")}
              onChange={(e) =>
                set("worktree_base", e.target.value || null)
              }
            />
            <div className="help" style={{ marginTop: 4 }}>
              {t("edit.worktree.baseHelp")}
            </div>
          </div>
          </>
        )}
      </section>

      <section className="section">
        <div className="section-title">{t("edit.section.prompt")}</div>
        <textarea
          className="textarea"
          value={draft.prompt}
          onChange={(e) => set("prompt", e.target.value)}
          placeholder={t("edit.promptPlaceholder")}
        />
        <div className="help" style={{ marginTop: 6 }}>
          {t("edit.promptStats", {
            chars: draft.prompt.length,
            lines: draft.prompt.split("\n").length,
          })}
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
  const t = useT();
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
      {tags.map((tag) => (
        <span key={tag} className="chip accent tag-chip">
          {tag}
          <button
            type="button"
            className="tag-chip-x"
            aria-label={t("edit.removeTag", { tag })}
            onClick={() => onChange(tags.filter((x) => x !== tag))}
          >
            ×
          </button>
        </span>
      ))}
      <input
        className="tag-input"
        value={draft}
        placeholder={tags.length ? "" : t("edit.tagsPlaceholder")}
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

function validate(task: Task): MessageKey | null {
  if (!task.name.trim()) return "edit.validate.name";
  if (!task.working_dir.trim()) return "edit.validate.workingDir";
  if (!task.prompt.trim()) return "edit.validate.prompt";
  if (task.schedule.startsWith("cron:") && !task.schedule.slice(5).trim())
    return "edit.validate.cron";
  if (task.schedule.startsWith("once:") && !task.schedule.slice(5).trim())
    return "edit.validate.once";
  return null;
}
