import { useMemo, useState } from "react";
import type { Task } from "../types";
import { PlusIcon, SettingsIcon } from "./Icon";
import { ScheduleChip } from "./StatusChip";

interface Props {
  tasks: Task[];
  activeId: string | null;
  view: "task" | "settings" | "history-all" | "empty";
  runningTaskIds: Set<string>;
  onSelect: (id: string) => void;
  onNew: () => void;
  onSettings: () => void;
  /** Persist a new task order (full id list, top-to-bottom). */
  onReorder: (orderedIds: string[]) => void;
}

export function Sidebar({
  tasks,
  activeId,
  view,
  runningTaskIds,
  onSelect,
  onNew,
  onSettings,
  onReorder,
}: Props) {
  const [query, setQuery] = useState("");
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);

  const allTags = useMemo(() => {
    const s = new Set<string>();
    for (const t of tasks) for (const x of t.tags ?? []) s.add(x);
    return [...s].sort();
  }, [tasks]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return tasks.filter((t) => {
      if (activeTag && !(t.tags ?? []).includes(activeTag)) return false;
      if (!q) return true;
      if (t.name.toLowerCase().includes(q)) return true;
      if ((t.tags ?? []).some((tag) => tag.toLowerCase().includes(q)))
        return true;
      return false;
    });
  }, [tasks, query, activeTag]);

  // Reordering is disabled while a filter is active — dropping in a filtered
  // view would silently move tasks past hidden siblings, which is surprising.
  const filterActive = query.trim().length > 0 || activeTag != null;

  function onDrop(targetId: string) {
    if (!dragId || dragId === targetId || filterActive) {
      setDragId(null);
      setDragOverId(null);
      return;
    }
    const ids = tasks.map((t) => t.id);
    const from = ids.indexOf(dragId);
    const to = ids.indexOf(targetId);
    if (from < 0 || to < 0) return;
    ids.splice(from, 1);
    ids.splice(to, 0, dragId);
    setDragId(null);
    setDragOverId(null);
    onReorder(ids);
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">
          Tasks · {filterActive ? `${filtered.length}/${tasks.length}` : tasks.length}
        </span>
        <button
          className="btn ghost icon"
          onClick={onNew}
          title="New task"
          aria-label="New task"
        >
          <PlusIcon size={16} />
        </button>
      </div>

      <div className="sidebar-filters">
        <input
          className="input sidebar-search"
          value={query}
          placeholder="Search…"
          onChange={(e) => setQuery(e.target.value)}
        />
        {allTags.length > 0 && (
          <div className="sidebar-tag-row">
            {allTags.map((tag) => (
              <button
                key={tag}
                type="button"
                className={`chip ${activeTag === tag ? "accent" : ""} tag-filter`}
                onClick={() =>
                  setActiveTag((cur) => (cur === tag ? null : tag))
                }
              >
                {tag}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="sidebar-body">
        {tasks.length === 0 ? (
          <div className="empty-state" style={{ padding: 24 }}>
            <div>No tasks yet.</div>
            <button className="btn primary" onClick={onNew}>
              Create one
            </button>
          </div>
        ) : filtered.length === 0 ? (
          <div className="empty-state" style={{ padding: 24 }}>
            <div>No tasks match.</div>
          </div>
        ) : (
          filtered.map((t) => (
            <div
              key={t.id}
              className={[
                "task-row",
                activeId === t.id && view === "task" ? "active" : "",
                dragId === t.id ? "dragging" : "",
                dragOverId === t.id && dragId && dragId !== t.id
                  ? "drag-over"
                  : "",
              ]
                .filter(Boolean)
                .join(" ")}
              onClick={() => onSelect(t.id)}
              role="button"
              tabIndex={0}
              draggable={!filterActive}
              onDragStart={(e) => {
                if (filterActive) return;
                setDragId(t.id);
                e.dataTransfer.effectAllowed = "move";
                // Some browsers need data set or the drag is aborted on drop.
                e.dataTransfer.setData("text/plain", t.id);
              }}
              onDragOver={(e) => {
                if (!dragId || filterActive) return;
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                if (dragOverId !== t.id) setDragOverId(t.id);
              }}
              onDragLeave={() => {
                if (dragOverId === t.id) setDragOverId(null);
              }}
              onDrop={(e) => {
                e.preventDefault();
                onDrop(t.id);
              }}
              onDragEnd={() => {
                setDragId(null);
                setDragOverId(null);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect(t.id);
                }
              }}
            >
              <div className="task-name">
                <span>{t.name || "(unnamed)"}</span>
                {runningTaskIds.has(t.id) && (
                  <span className="chip info">
                    <span className="pulse" />
                    running
                  </span>
                )}
              </div>
              <div className="task-meta">
                <ScheduleChip schedule={t.schedule} />
                {!t.enabled && <span className="chip">disabled</span>}
                {(t.tags ?? []).map((tag) => (
                  <span key={tag} className="chip tag-chip-mini">
                    {tag}
                  </span>
                ))}
              </div>
            </div>
          ))
        )}
      </div>
      <div className="sidebar-footer">
        <button
          className={`btn ghost ${view === "settings" ? "active" : ""}`}
          onClick={onSettings}
          style={{ flex: 1, justifyContent: "flex-start" }}
        >
          <SettingsIcon size={15} />
          Settings
        </button>
      </div>
    </aside>
  );
}
