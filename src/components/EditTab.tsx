import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { api, onRunUpdate, pickDirectory } from "../api";
import { useT } from "../LanguageProvider";
import type { MessageKey } from "../i18n";
import type { Model, Task, TaskMemory } from "../types";
import { parseScheduleKind } from "../types";
import {
  AlertIcon,
  CopyIcon,
  FolderIcon,
  PlayIcon,
  SaveIcon,
  TrashIcon,
} from "./Icon";
import { ScheduleEditor } from "./ScheduleEditor";
import { SectionNav, type SectionNavItem } from "./SectionNav";

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
  onDuplicate: () => void;
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
  onDuplicate,
}: Props) {
  const t = useT();
  const [models, setModels] = useState<Model[]>([]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const toolbarRef = useRef<HTMLDivElement>(null);
  // The sticky toolbar grows when a validation/save message appears, so the
  // section nav (and click-scroll offset) tracks its live height instead of a
  // guessed constant — otherwise the nav would slip behind the taller bar.
  const [toolbarH, setToolbarH] = useState(58);
  useEffect(() => {
    const el = toolbarRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => setToolbarH(el.offsetHeight));
    ro.observe(el);
    setToolbarH(el.offsetHeight);
    return () => ro.disconnect();
  }, []);

  const navItems: SectionNavItem[] = useMemo(
    () => [
      { id: "edit-basics", label: t("edit.section.basics") },
      { id: "edit-schedule", label: t("edit.section.schedule") },
      { id: "edit-execution", label: t("edit.section.execution") },
      { id: "edit-prompt", label: t("edit.section.prompt") },
      { id: "edit-memory", label: t("edit.section.memory") },
    ],
    [t],
  );

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
    <div className="panel" ref={panelRef}>
      <div className="sticky-bar edit-toolbar" ref={toolbarRef}>
        <div className="edit-toolbar-main">
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
          <button
            className="btn"
            disabled={isNew || busy}
            onClick={onDuplicate}
            title={t("edit.duplicate")}
          >
            <CopyIcon size={13} />
            {t("edit.duplicate")}
          </button>
        </div>
        <div className="row" style={{ gap: 8 }}>
          {isNew ? (
            // An unsaved new task isn't on disk yet, so there's nothing to
            // "delete" — this just throws the draft away (handled by onDelete's
            // new-task branch). One click, no confirm: it's the natural "cancel
            // creating this task" action.
            <button className="btn danger" onClick={onDelete}>
              <TrashIcon size={13} />
              {t("edit.discard")}
            </button>
          ) : confirmDelete ? (
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
              onClick={() => setConfirmDelete(true)}
            >
              <TrashIcon size={13} />
              {t("edit.delete")}
            </button>
          )}
        </div>
        </div>
        {validation && (
          <div className="edit-toolbar-status error-text">
            <AlertIcon size={13} />
            {t(validation)}
          </div>
        )}
        {message && <div className="edit-toolbar-status help">{message}</div>}
      </div>

      <div className="toc-layout">
        <div className="toc-main">
      <section className="section" id="edit-basics">
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

      <section className="section" id="edit-schedule">
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

      <section className="section" id="edit-execution">
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

      <section className="section" id="edit-prompt">
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

      <section className="section" id="edit-memory">
        <div className="section-title">{t("edit.section.memory")}</div>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={draft.memory_enabled}
            onChange={(e) => set("memory_enabled", e.target.checked)}
          />
          {t("edit.memory.enable")}
        </label>
        <div className="help" style={{ marginTop: 6, marginLeft: 22 }}>
          {t("edit.memory.enableHelp")}
        </div>
        {draft.memory_enabled && !isNew && <MemorySection taskId={task.id} />}
      </section>
        </div>
        <SectionNav
          items={navItems}
          containerRef={panelRef}
          topOffset={toolbarH + 12}
        />
      </div>
    </div>
  );
}

// Saved memory viewer/editor. Memory is DB-backed (not part of the task draft),
// so this self-loads by task id and persists through its own IPC calls.
function MemorySection({ taskId }: { taskId: string }) {
  const t = useT();
  const [content, setContent] = useState("");
  const [loaded, setLoaded] = useState<TaskMemory | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    setMessage(null);
    api
      .getTaskMemory(taskId)
      .then((m) => {
        setLoaded(m);
        setContent(m?.content ?? "");
      })
      .catch((e) => setMessage(t("edit.memory.loadFailed", { error: String(e) })));
  }, [taskId, t]);

  const dirty = content !== (loaded?.content ?? "");

  // A run writes the agent's `<memory>` block to the DB *before* it emits the
  // `finished` event, so by the time we hear about it the new memory is already
  // persisted — just re-fetch. We skip the refresh while the user has unsaved
  // edits (`dirty`) so we never clobber what they're typing; a ref keeps the
  // listener reading the latest dirty state without re-subscribing each render.
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    onRunUpdate((u) => {
      if (u.kind !== "finished" || u.task_id !== taskId) return;
      if (dirtyRef.current) return;
      api
        .getTaskMemory(taskId)
        .then((m) => {
          setLoaded(m);
          setContent(m?.content ?? "");
        })
        .catch(() => {});
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [taskId]);

  async function persist(next: string) {
    setBusy(true);
    setMessage(null);
    try {
      await api.setTaskMemory(taskId, next);
      const m = await api.getTaskMemory(taskId);
      setLoaded(m);
      setContent(m?.content ?? "");
      setMessage(t("edit.memory.saved"));
    } catch (e) {
      setMessage(t("edit.memory.saveFailed", { error: String(e) }));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="field" style={{ marginTop: 14 }}>
      <label className="field-label">{t("edit.memory.label")}</label>
      <textarea
        className="textarea"
        value={content}
        onChange={(e) => setContent(e.target.value)}
        placeholder={t("edit.memory.placeholder")}
      />
      <div className="row" style={{ gap: 8, marginTop: 8 }}>
        <button
          className="btn primary"
          disabled={!dirty || busy}
          onClick={() => persist(content)}
        >
          {t("edit.memory.save")}
        </button>
        <button
          className="btn"
          disabled={busy || (!content && !loaded)}
          onClick={() => persist("")}
        >
          {t("edit.memory.clear")}
        </button>
        {loaded && (
          <span className="help">
            {t("edit.memory.updated", {
              time: new Date(loaded.updated_at).toLocaleString(),
            })}
          </span>
        )}
      </div>
      {message && (
        <div className="help" style={{ marginTop: 6 }}>
          {message}
        </div>
      )}
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
