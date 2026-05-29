import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, onRunUpdate } from "./api";
import { EditTab } from "./components/EditTab";
import { HistoryTab } from "./components/HistoryTab";
import { SettingsPanel } from "./components/SettingsPanel";
import { Sidebar } from "./components/Sidebar";
import { ThemeToggle } from "./components/ThemeToggle";
import { applyTheme, getInitialTheme, saveTheme, type Theme } from "./theme";
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
  const [file, setFile] = useState<TasksFile | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [tab, setTab] = useState<TabId>("edit");
  const [view, setView] = useState<View>("empty");
  const [newDraft, setNewDraft] = useState<Task | null>(null);
  const [events, setEvents] = useState<RunUpdate[]>([]);
  const [status, setStatus] = useState<string | null>(null);
  const [theme, setTheme] = useState<Theme>(() => getInitialTheme());

  useEffect(() => {
    applyTheme(theme);
    saveTheme(theme);
  }, [theme]);

  // Track which task ids currently have a running run, for the sidebar pill.
  // We derive from RunUpdate events to avoid an extra DB poll on every render.
  const runningRef = useRef<Set<string>>(new Set());
  const [runningTick, setRunningTick] = useState(0);

  const tasks: Task[] = useMemo(() => {
    if (!file) return [];
    if (newDraft) return [newDraft, ...file.tasks];
    return file.tasks;
  }, [file, newDraft]);

  const active = tasks.find((t) => t.id === activeId) ?? null;
  const isNew = !!newDraft && newDraft.id === activeId;

  const refresh = useCallback(async () => {
    const next = await api.getTasksFile();
    setFile(next);
  }, []);

  useEffect(() => {
    refresh().catch((e) => setStatus(`Load failed: ${e}`));
  }, [refresh]);

  // Subscribe to run lifecycle events from the backend.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onRunUpdate((u) => {
      setEvents((prev) => [...prev.slice(-200), u]);
      if (u.kind === "started") {
        runningRef.current.add(u.task_id);
        setRunningTick((t) => t + 1);
        flash(setStatus, `Run #${u.run_id} started`);
      } else if (u.kind === "finished") {
        // We don't have task_id on finish; refresh the run list to update
        // the pill state lazily. For accuracy we re-derive by listing runs.
        api.listRuns(50).then((runs) => {
          const stillRunning = new Set(
            runs.filter((r) => r.status === "running").map((r) => r.task_id),
          );
          runningRef.current = stillRunning;
          setRunningTick((t) => t + 1);
        });
        flash(setStatus, `Run #${u.run_id} ${u.status}`);
      }
    }).then((un) => (unlisten = un));
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Prime the running-pill state on boot so existing in-flight runs appear.
  useEffect(() => {
    api.listRuns(50).then((runs) => {
      runningRef.current = new Set(
        runs.filter((r) => r.status === "running").map((r) => r.task_id),
      );
      setRunningTick((t) => t + 1);
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
    setFile(nextFile);
    setActiveId(updated.id);
    flash(setStatus, "Saved & scheduler restarted");
  }

  async function deleteTask() {
    if (!file || !active) return;
    if (newDraft && newDraft.id === active.id) {
      setNewDraft(null);
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
    setFile(nextFile);
    setActiveId(nextFile.tasks[0]?.id ?? null);
    setView(nextFile.tasks[0] ? "task" : "empty");
    flash(setStatus, "Deleted");
  }

  async function runActive() {
    if (!active) return;
    await api.runNow(active.id);
    setTab("history");
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
        Loading…
      </div>
    );
  }

  return (
    <div className="app">
      <Sidebar
        tasks={tasks}
        activeId={activeId}
        view={view === "task" ? "task" : view === "settings" ? "settings" : "empty"}
        runningTaskIds={runningRef.current}
        onSelect={selectTask}
        onNew={newTask}
        onSettings={openSettings}
        key={`sidebar-${runningTick}`}
      />

      <main className="content">
        {view === "task" && active ? (
          <>
            <div className="content-header">
              <span className="content-title">{active.name || "Untitled"}</span>
              <span className="help">id: {active.id}</span>
            </div>
            <div className="tabs">
              <button
                className={`tab ${tab === "edit" ? "active" : ""}`}
                onClick={() => setTab("edit")}
              >
                Edit
              </button>
              <button
                className={`tab ${tab === "history" ? "active" : ""}`}
                onClick={() => setTab("history")}
                disabled={isNew}
                title={isNew ? "Save the task first to see history" : ""}
              >
                History
              </button>
            </div>
            {tab === "edit" ? (
              <EditTab
                task={active}
                isNew={isNew}
                onSave={saveTask}
                onDelete={deleteTask}
                onRunNow={runActive}
              />
            ) : (
              <HistoryTab task={active} events={events} />
            )}
          </>
        ) : view === "settings" ? (
          <SettingsPanel settings={file.settings} onSave={saveSettings} />
        ) : (
          <div className="empty-state">
            <h2>No task selected</h2>
            <p>Pick a task on the left or create a new one.</p>
          </div>
        )}
      </main>

      <div className="status-bar">
        <span>
          {file.tasks.length} task{file.tasks.length === 1 ? "" : "s"} ·{" "}
          {file.tasks.filter((t) => t.enabled).length} enabled
        </span>
        <div className="status-bar-right">
          <span>{status ?? "ready"}</span>
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
