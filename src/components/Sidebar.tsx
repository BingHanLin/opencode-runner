import { useMemo, useState } from "react";
import type { Task } from "../types";
import { useT } from "../LanguageProvider";
import { PlusIcon, SettingsIcon } from "./Icon";
import { ScheduleChip } from "./StatusChip";

interface Props {
  tasks: Task[];
  activeId: string | null;
  view: "task" | "settings" | "history-all" | "empty";
  runningTaskIds: Set<string>;
  /** Task ids with unsaved edits, flagged with a marker in the list. */
  dirtyTaskIds: Set<string>;
  /** The unsaved, never-persisted new task (if any), flagged as a draft. */
  newTaskId: string | null;
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
  dirtyTaskIds,
  newTaskId,
  onSelect,
  onNew,
  onSettings,
  onReorder,
}: Props) {
  const tr = useT();
  const [query, setQuery] = useState("");
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  // Natural height of the row being dragged, measured at drag start. Drives
  // both the collapse of that row and the size of the slot other rows open.
  const [dragHeight, setDragHeight] = useState(0);
  // Set one frame after drag start so the row animates from its full height
  // down to zero (a transition needs a concrete start value to animate from).
  const [collapsed, setCollapsed] = useState(false);

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

  function resetDrag() {
    setDragId(null);
    setDragOverId(null);
    setCollapsed(false);
    setDragHeight(0);
  }

  function onDrop(targetId: string) {
    if (!dragId || dragId === targetId || filterActive) {
      resetDrag();
      return;
    }
    const ids = tasks.map((t) => t.id);
    const from = ids.indexOf(dragId);
    const to = ids.indexOf(targetId);
    if (from < 0 || to < 0) {
      resetDrag();
      return;
    }
    ids.splice(from, 1);
    ids.splice(to, 0, dragId);
    resetDrag();
    onReorder(ids);
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">
          {tr("sidebar.tasks", {
            count: filterActive
              ? `${filtered.length}/${tasks.length}`
              : tasks.length,
          })}
        </span>
        <button
          className="btn ghost icon"
          onClick={onNew}
          title={tr("sidebar.newTask")}
          aria-label={tr("sidebar.newTask")}
        >
          <PlusIcon size={16} />
        </button>
      </div>

      <div className="sidebar-filters">
        <input
          className="input sidebar-search"
          value={query}
          placeholder={tr("sidebar.search")}
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
            <div>{tr("sidebar.noTasks")}</div>
            <button className="btn primary" onClick={onNew}>
              {tr("sidebar.createOne")}
            </button>
          </div>
        ) : filtered.length === 0 ? (
          <div className="empty-state" style={{ padding: 24 }}>
            <div>{tr("sidebar.noMatch")}</div>
          </div>
        ) : (
          filtered.map((t) => {
            // The insertion line is drawn on the edge where the dragged row
            // will actually land: below the target when dragging downward,
            // above it when dragging upward (matches the splice in onDrop).
            const showDropLine =
              dragOverId === t.id && dragId != null && dragId !== t.id;
            let dropEdge = "";
            if (showDropLine) {
              const from = filtered.findIndex((x) => x.id === dragId);
              const to = filtered.findIndex((x) => x.id === t.id);
              dropEdge = from < to ? "drag-over-bottom" : "drag-over-top";
            }
            const isDragging = dragId === t.id;
            // The dragged row collapses to nothing; the row next to the drop
            // point opens a same-height slot so the list "makes way" for it.
            const rowStyle = isDragging
              ? {
                  height: collapsed ? 0 : dragHeight || undefined,
                  paddingTop: collapsed ? 0 : undefined,
                  paddingBottom: collapsed ? 0 : undefined,
                  marginTop: 0,
                  marginBottom: 0,
                  opacity: collapsed ? 0 : 0.5,
                }
              : dropEdge === "drag-over-top"
                ? { marginTop: dragHeight }
                : dropEdge === "drag-over-bottom"
                  ? { marginBottom: dragHeight }
                  : undefined;
            return (
            <div
              key={t.id}
              style={rowStyle}
              className={[
                "task-row",
                activeId === t.id && view === "task" ? "active" : "",
                isDragging ? "dragging" : "",
                dropEdge,
              ]
                .filter(Boolean)
                .join(" ")}
              onClick={() => onSelect(t.id)}
              role="button"
              tabIndex={0}
              draggable={!filterActive}
              onDragStart={(e) => {
                if (filterActive) return;
                setDragHeight(e.currentTarget.getBoundingClientRect().height);
                setCollapsed(false);
                setDragId(t.id);
                e.dataTransfer.effectAllowed = "move";
                // Some browsers need data set or the drag is aborted on drop.
                e.dataTransfer.setData("text/plain", t.id);
                // Collapse one frame later so the height has a value to
                // animate from (a transition can't start from `auto`).
                requestAnimationFrame(() =>
                  requestAnimationFrame(() => setCollapsed(true)),
                );
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
                resetDrag();
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect(t.id);
                }
              }}
            >
              <div className="task-name">
                {dirtyTaskIds.has(t.id) && t.id !== newTaskId && (
                  <span
                    className="unsaved-dot"
                    title={tr("sidebar.unsaved")}
                    aria-label={tr("sidebar.unsaved")}
                  />
                )}
                <span>{t.name || tr("sidebar.unnamed")}</span>
                {t.id === newTaskId && (
                  <span className="chip accent" title={tr("sidebar.draftTitle")}>
                    {tr("sidebar.draft")}
                  </span>
                )}
                {runningTaskIds.has(t.id) && (
                  <span className="chip info">
                    <span className="pulse" />
                    {tr("sidebar.running")}
                  </span>
                )}
              </div>
              <div className="task-meta">
                <ScheduleChip schedule={t.schedule} />
                {!t.enabled && <span className="chip">{tr("sidebar.disabled")}</span>}
                {(t.tags ?? []).map((tag) => (
                  <span key={tag} className="chip tag-chip-mini">
                    {tag}
                  </span>
                ))}
              </div>
            </div>
            );
          })
        )}
      </div>
      <div className="sidebar-footer">
        <button
          className={`btn ghost ${view === "settings" ? "active" : ""}`}
          onClick={onSettings}
          style={{ flex: 1, justifyContent: "flex-start" }}
        >
          <SettingsIcon size={15} />
          {tr("sidebar.settings")}
        </button>
      </div>
    </aside>
  );
}
