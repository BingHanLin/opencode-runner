import { useCallback, useEffect, useMemo, useState } from "react";
import { api, onRunUpdate } from "./api";
import { EditTab } from "./components/EditTab";
import { HistoryTab } from "./components/HistoryTab";
import { SettingsPanel } from "./components/SettingsPanel";
import { Sidebar } from "./components/Sidebar";
import { ThemeToggle } from "./components/ThemeToggle";
import { UpdateBanner } from "./components/UpdateBanner";
import { applyTheme, getInitialTheme, saveTheme, type Theme } from "./theme";
import { useT } from "./LanguageProvider";
import type {
  RunUpdate,
  Settings,
  Task,
  TasksFile,
} from "./types";
import { newBlankTask } from "./types";

type View = "task" | "settings" | "empty";
type TabId = "edit" | "history";

export default function App() {
  const t = useT();
  const [file, setFile] = useState<TasksFile | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [tab, setTab] = useState<TabId>("edit");
  const [view, setView] = useState<View>("empty");
  const [newDraft, setNewDraft] = useState<Task | null>(null);
  // In-progress, unsaved edits keyed by task id. Lifted out of EditTab so they
  // survive task switching: leaving a task and coming back restores the edits,
  // and the sidebar can flag which tasks have pending changes.
  const [drafts, setDrafts] = useState<Record<string, Task>>({});
  const [events, setEvents] = useState<RunUpdate[]>([]);
  const [status, setStatus] = useState<string | null>(null);
  const [theme, setTheme] = useState<Theme>(() => getInitialTheme());
  const [version, setVersion] = useState("");

  useEffect(() => {
    applyTheme(theme);
    saveTheme(theme);
  }, [theme]);

  useEffect(() => {
    api.appVersion().then(setVersion).catch(() => {});
  }, []);

  // Track which task ids currently have a running run, for the sidebar pill.
  // Stored as state (not a ref) so the Set's identity changes on each update
  // — React's prop equality check would skip the Sidebar re-render otherwise.
  const [runningTaskIds, setRunningTaskIds] = useState<Set<string>>(
    () => new Set(),
  );

  const tasks: Task[] = useMemo(() => {
    if (!file) return [];
    if (newDraft) return [newDraft, ...file.tasks];
    return file.tasks;
  }, [file, newDraft]);

  const active = tasks.find((t) => t.id === activeId) ?? null;
  const isNew = !!newDraft && newDraft.id === activeId;

  // The working copy shown in the editor: a stashed draft if one exists,
  // otherwise the saved task itself.
  const activeDraft = active ? drafts[active.id] ?? active : null;

  // Tasks whose stashed draft differs from their saved (or, for the new task,
  // blank) baseline — drives the sidebar's unsaved-changes marker.
  const dirtyIds = useMemo(() => {
    const s = new Set<string>();
    for (const tk of tasks) {
      const d = drafts[tk.id];
      if (d && JSON.stringify(d) !== JSON.stringify(tk)) s.add(tk.id);
    }
    return s;
  }, [tasks, drafts]);

  function updateDraft(updated: Task) {
    setDrafts((d) => ({ ...d, [updated.id]: updated }));
  }

  function clearDraft(id: string) {
    setDrafts((d) => {
      if (!(id in d)) return d;
      const next = { ...d };
      delete next[id];
      return next;
    });
  }

  const refresh = useCallback(async () => {
    const next = await api.getTasksFile();
    setFile(next);
  }, []);

  useEffect(() => {
    refresh().catch((e) => setStatus(t("app.flash.loadFailed", { error: String(e) })));
  }, [refresh, t]);

  // Subscribe to run lifecycle events from the backend.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onRunUpdate((u) => {
      setEvents((prev) => [...prev.slice(-200), u]);
      if (u.kind === "started") {
        setRunningTaskIds((prev) => {
          const next = new Set(prev);
          next.add(u.task_id);
          return next;
        });
        flash(setStatus, t("app.flash.runStarted", { id: u.run_id }));
      } else if (u.kind === "finished") {
        // We don't have task_id on finish; refresh the run list to update
        // the pill state lazily. For accuracy we re-derive by listing runs.
        api.listRuns(50).then((runs) => {
          setRunningTaskIds(
            new Set(
              runs.filter((r) => r.status === "running").map((r) => r.task_id),
            ),
          );
        });
        flash(setStatus, t("app.flash.runFinished", { id: u.run_id, status: u.status }));
      }
    }).then((un) => (unlisten = un));
    return () => {
      if (unlisten) unlisten();
    };
  }, [t]);

  // Prime the running-pill state on boot so existing in-flight runs appear.
  useEffect(() => {
    api.listRuns(50).then((runs) => {
      setRunningTaskIds(
        new Set(runs.filter((r) => r.status === "running").map((r) => r.task_id)),
      );
    });
  }, []);

  function selectTask(id: string) {
    setActiveId(id);
    setView("task");
    setTab("edit");
  }

  function newTask() {
    const draft = newBlankTask();
    setNewDraft(draft);
    setActiveId(draft.id);
    setView("task");
    setTab("edit");
  }

  // Clone the currently shown task (including any unsaved edits) into a fresh,
  // unsaved draft with a new id. It opens in the editor as a pending new task —
  // nothing is written until the user saves. Memory/run history are per-task in
  // the db and intentionally not carried over. The schedule is disabled so a
  // duplicated cron task can't silently start firing alongside the original.
  function duplicateTask() {
    const base = activeDraft;
    if (!base) return;
    const copy: Task = {
      ...structuredClone(base),
      id: crypto.randomUUID(),
      name: t("edit.nameCopySuffix", { name: base.name || t("sidebar.unnamed") }),
      enabled: false,
    };
    setNewDraft(copy);
    setActiveId(copy.id);
    setView("task");
    setTab("edit");
    flash(setStatus, t("app.flash.duplicated"));
  }

  function openSettings() {
    setView("settings");
    setActiveId(null);
  }

  async function saveTask(updated: Task) {
    if (!file) return;
    const others = newDraft
      ? file.tasks
      : file.tasks.filter((t) => t.id !== updated.id);
    const nextFile: TasksFile = {
      settings: file.settings,
      tasks: newDraft ? [...others, updated] : [updated, ...others],
    };
    // Preserve original ordering for existing tasks: replace in place.
    if (!newDraft) {
      const idx = file.tasks.findIndex((t) => t.id === updated.id);
      if (idx >= 0) {
        const arr = file.tasks.slice();
        arr[idx] = updated;
        nextFile.tasks = arr;
      }
    }
    await api.saveTasksFile(nextFile);
    await api.restartScheduler();
    setNewDraft(null);
    clearDraft(updated.id);
    setFile(nextFile);
    setActiveId(updated.id);
    flash(setStatus, t("app.flash.savedRestarted"));
  }

  async function deleteTask() {
    if (!file || !active) return;
    if (newDraft && newDraft.id === active.id) {
      setNewDraft(null);
      clearDraft(active.id);
      setActiveId(file.tasks[0]?.id ?? null);
      setView(file.tasks[0] ? "task" : "empty");
      return;
    }
    const nextFile: TasksFile = {
      settings: file.settings,
      tasks: file.tasks.filter((t) => t.id !== active.id),
    };
    await api.saveTasksFile(nextFile);
    await api.restartScheduler();
    clearDraft(active.id);
    setFile(nextFile);
    setActiveId(nextFile.tasks[0]?.id ?? null);
    setView(nextFile.tasks[0] ? "task" : "empty");
    flash(setStatus, t("app.flash.deleted"));
  }

  async function runActive() {
    if (!active) return;
    await api.runNow(active.id);
    setTab("history");
  }

  async function reorderTasks(orderedIds: string[]) {
    if (!file) return;
    const byId = new Map(file.tasks.map((t) => [t.id, t]));
    const reordered = orderedIds
      .map((id) => byId.get(id))
      .filter((t): t is Task => !!t);
    // Belt-and-suspenders: if the reorder somehow dropped a task, splice it
    // back in at the end so we never lose state on a buggy drag interaction.
    for (const t of file.tasks) {
      if (!orderedIds.includes(t.id)) reordered.push(t);
    }
    const next: TasksFile = { settings: file.settings, tasks: reordered };
    setFile(next);
    await api.saveTasksFile(next);
    flash(setStatus, t("app.flash.reordered"));
  }

  async function saveSettings(settings: Settings) {
    if (!file) return;
    const next: TasksFile = { settings, tasks: file.tasks };
    await api.saveTasksFile(next);
    setFile(next);
    await api.restartScheduler();
  }

  if (!file) {
    return (
      <div className="empty-state" style={{ height: "100vh" }}>
        {t("app.loading")}
      </div>
    );
  }

  return (
    <div className="app">
      <Sidebar
        tasks={tasks}
        activeId={activeId}
        view={view === "task" ? "task" : view === "settings" ? "settings" : "empty"}
        runningTaskIds={runningTaskIds}
        dirtyTaskIds={dirtyIds}
        newTaskId={newDraft?.id ?? null}
        onSelect={selectTask}
        onNew={newTask}
        onSettings={openSettings}
        onReorder={reorderTasks}
      />

      <main className="content">
        <UpdateBanner />
        {view === "task" && active ? (
          <>
            <div className="content-header">
              <span className="content-title">{active.name || t("app.untitled")}</span>
              <span className="help">{t("app.idLabel", { id: active.id })}</span>
            </div>
            <div className="tabs">
              <button
                className={`tab ${tab === "edit" ? "active" : ""}`}
                onClick={() => setTab("edit")}
              >
                {t("app.tab.edit")}
              </button>
              <button
                className={`tab ${tab === "history" ? "active" : ""}`}
                onClick={() => setTab("history")}
                disabled={isNew}
                title={isNew ? t("app.tab.historyDisabled") : ""}
              >
                {t("app.tab.history")}
              </button>
            </div>
            {tab === "edit" ? (
              <EditTab
                task={active}
                draft={activeDraft ?? active}
                isNew={isNew}
                onChange={updateDraft}
                onRevert={() => clearDraft(active.id)}
                onSave={saveTask}
                onDelete={deleteTask}
                onRunNow={runActive}
                onDuplicate={duplicateTask}
              />
            ) : (
              <HistoryTab task={active} events={events} />
            )}
          </>
        ) : view === "settings" ? (
          <SettingsPanel settings={file.settings} onSave={saveSettings} />
        ) : (
          <div className="empty-state">
            <h2>{t("app.empty.title")}</h2>
            <p>{t("app.empty.body")}</p>
          </div>
        )}
      </main>

      <div className="status-bar">
        <span>
          {t(
            file.tasks.length === 1 ? "app.status.tasksOne" : "app.status.tasksMany",
            {
              count: file.tasks.length,
              enabled: file.tasks.filter((task) => task.enabled).length,
            },
          )}
        </span>
        <div className="status-bar-right">
          {version && <span className="status-version">v{version}</span>}
          <span>{status ?? t("app.status.ready")}</span>
          <ThemeToggle
            theme={theme}
            onToggle={() => setTheme(theme === "dark" ? "light" : "dark")}
          />
        </div>
      </div>
    </div>
  );
}

function flash(setStatus: (s: string | null) => void, msg: string) {
  setStatus(msg);
  setTimeout(() => setStatus(null), 3000);
}
