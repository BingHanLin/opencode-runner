import type { Task } from "../types";
import { ScheduleChip } from "./StatusChip";

interface Props {
  tasks: Task[];
  activeId: string | null;
  view: "task" | "settings" | "history-all" | "empty";
  runningTaskIds: Set<string>;
  onSelect: (id: string) => void;
  onNew: () => void;
  onSettings: () => void;
}

export function Sidebar({
  tasks,
  activeId,
  view,
  runningTaskIds,
  onSelect,
  onNew,
  onSettings,
}: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">Tasks · {tasks.length}</span>
        <button className="btn ghost" onClick={onNew} title="New task">
          +
        </button>
      </div>
      <div className="sidebar-body">
        {tasks.length === 0 ? (
          <div className="empty-state" style={{ padding: 24 }}>
            <div>No tasks yet.</div>
            <button className="btn primary" onClick={onNew}>
              Create one
            </button>
          </div>
        ) : (
          tasks.map((t) => (
            <div
              key={t.id}
              className={`task-row ${activeId === t.id && view === "task" ? "active" : ""}`}
              onClick={() => onSelect(t.id)}
            >
              <div className="task-name">
                {t.name || "(unnamed)"}{" "}
                {runningTaskIds.has(t.id) && (
                  <span className="chip info" style={{ marginLeft: 6 }}>
                    running
                  </span>
                )}
              </div>
              <div className="task-meta">
                <ScheduleChip schedule={t.schedule} />
                {!t.enabled && <span className="chip">disabled</span>}
              </div>
            </div>
          ))
        )}
      </div>
      <div className="sidebar-footer">
        <button
          className={`btn ghost ${view === "settings" ? "active" : ""}`}
          onClick={onSettings}
          style={{ flex: 1 }}
        >
          ⚙ Settings
        </button>
      </div>
    </aside>
  );
}
