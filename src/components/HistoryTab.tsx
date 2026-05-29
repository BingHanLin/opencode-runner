import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type {
  MessagePair,
  Run,
  RunEvent,
  RunUpdate,
  Task,
} from "../types";
import { StatusChip } from "./StatusChip";

interface Props {
  task: Task;
  /** Incoming RunUpdate events for any task; we filter to this task's runs. */
  events: RunUpdate[];
}

export function HistoryTab({ task, events }: Props) {
  const [runs, setRuns] = useState<Run[]>([]);
  const [activeRunId, setActiveRunId] = useState<number | null>(null);
  const [runEvents, setRunEvents] = useState<RunEvent[]>([]);
  const [convo, setConvo] = useState<MessagePair[] | null>(null);
  const [convoError, setConvoError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    const list = await api.listRunsForTask(task.id, 100);
    setRuns(list);
    if (activeRunId == null && list.length > 0) {
      setActiveRunId(list[0].id);
    }
  }, [task.id, activeRunId]);

  useEffect(() => {
    setActiveRunId(null);
    setRunEvents([]);
    setConvo(null);
    reload();
  }, [task.id]);

  // React to backend RunUpdate events — refresh the run list whenever a run
  // for this task starts or finishes, refresh the event timeline whenever a
  // step changes for the currently-selected run.
  useEffect(() => {
    if (events.length === 0) return;
    const last = events[events.length - 1];
    if (last.kind === "started" && last.task_id === task.id) {
      reload();
      setActiveRunId(last.run_id);
    } else if (last.kind === "finished") {
      reload();
      if (last.run_id === activeRunId) {
        api.listEvents(last.run_id).then(setRunEvents);
      }
    } else if (
      (last.kind === "event_started" || last.kind === "event_finished") &&
      last.run_id === activeRunId
    ) {
      api.listEvents(last.run_id).then(setRunEvents);
    } else if (
      last.kind === "session_assigned" &&
      last.run_id === activeRunId
    ) {
      reload();
    }
  }, [events, task.id, activeRunId, reload]);

  // Load events + conversation for whichever run is selected.
  useEffect(() => {
    if (activeRunId == null) {
      setRunEvents([]);
      setConvo(null);
      return;
    }
    api.listEvents(activeRunId).then(setRunEvents);
    const run = runs.find((r) => r.id === activeRunId);
    if (run?.session_id) {
      setConvoError(null);
      api
        .loadConversation(run.session_id)
        .then(setConvo)
        .catch((e) => {
          setConvo(null);
          setConvoError(String(e));
        });
    } else {
      setConvo(null);
      setConvoError(null);
    }
  }, [activeRunId, runs]);

  const activeRun = runs.find((r) => r.id === activeRunId) ?? null;

  return (
    <div className="panel" style={{ display: "flex", padding: 0 }}>
      <div className="history-layout" style={{ padding: "18px 24px 24px" }}>
        <div className="history-left">
          <div
            className="row"
            style={{ justifyContent: "space-between", marginBottom: 10 }}
          >
            <span className="section-title" style={{ margin: 0 }}>
              Runs · {runs.length}
            </span>
            <button className="btn ghost" onClick={reload}>
              ⟳
            </button>
          </div>
          {runs.length === 0 ? (
            <div className="empty-state">No runs yet for this task.</div>
          ) : (
            <div className="run-list">
              {runs.map((r) => (
                <div
                  key={r.id}
                  className={`run-card ${activeRunId === r.id ? "active" : ""}`}
                  onClick={() => setActiveRunId(r.id)}
                >
                  <div className="run-row">
                    <span>#{r.id}</span>
                    <StatusChip status={r.status} />
                  </div>
                  <div className="run-meta">
                    started {formatTime(r.started_at)} · {duration(r)}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        <div className="history-right">
          {activeRun ? (
            <RunDetails
              run={activeRun}
              events={runEvents}
              conversation={convo}
              conversationError={convoError}
              onAbort={() => api.abortRun(activeRun.id)}
            />
          ) : (
            <div className="empty-state">Select a run on the left.</div>
          )}
        </div>
      </div>
    </div>
  );
}

function RunDetails({
  run,
  events,
  conversation,
  conversationError,
  onAbort,
}: {
  run: Run;
  events: RunEvent[];
  conversation: MessagePair[] | null;
  conversationError: string | null;
  onAbort: () => void;
}) {
  return (
    <>
      <div className="run-detail-header">
        <div className="row" style={{ gap: 8 }}>
          <span className="content-title" style={{ fontSize: 16 }}>
            Run #{run.id}
          </span>
          <StatusChip status={run.status} />
        </div>
        {run.status === "running" && (
          <button className="btn danger" onClick={onAbort}>
            Stop
          </button>
        )}
      </div>

      <div className="help" style={{ marginBottom: 12 }}>
        started {formatTime(run.started_at)}
        {run.finished_at && ` · finished ${formatTime(run.finished_at)}`}
        {run.session_id && ` · session ${run.session_id}`}
      </div>

      {run.error && (
        <div
          className="section"
          style={{ borderColor: "rgba(236,113,109,0.4)" }}
        >
          <div className="section-title" style={{ color: "var(--error)" }}>
            Error
          </div>
          <div className="conv-text mono">{run.error}</div>
        </div>
      )}

      <section className="section">
        <div className="section-title">Steps</div>
        {events.length === 0 ? (
          <div className="help">No steps recorded.</div>
        ) : (
          events.map((e) => (
            <div key={e.id}>
              <div className="event-row">
                <StatusChip status={e.status} />
                <span className="event-name">{e.name}</span>
                <span className="help">{stepDuration(e)}</span>
              </div>
              {e.message && <div className="event-message">{e.message}</div>}
            </div>
          ))
        )}
      </section>

      <section className="section">
        <div className="section-title">Conversation</div>
        {!run.session_id ? (
          <div className="help">No session id captured for this run.</div>
        ) : conversationError ? (
          <div className="error-text">{conversationError}</div>
        ) : conversation === null ? (
          <div className="help">Loading…</div>
        ) : conversation.length === 0 ? (
          <div className="help">Conversation is empty.</div>
        ) : (
          conversation.map((pair, i) => (
            <div
              key={pair.message.id || i}
              className={`conv-msg ${pair.message.role === "assistant" ? "assistant" : ""}`}
            >
              <div className="conv-role">{pair.message.role ?? "?"}</div>
              {pair.parts.map((p) => (
                <div className="conv-part" key={p.id}>
                  {p.kind && p.kind !== "text" && (
                    <div className="conv-kind">{p.kind}</div>
                  )}
                  {p.text && (
                    <div
                      className={`conv-text ${p.kind && p.kind !== "text" ? "mono" : ""}`}
                    >
                      {p.text}
                    </div>
                  )}
                </div>
              ))}
            </div>
          ))
        )}
      </section>
    </>
  );
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString();
  } catch {
    return iso;
  }
}

function duration(r: Run): string {
  if (!r.finished_at) return "in progress";
  const ms = new Date(r.finished_at).getTime() - new Date(r.started_at).getTime();
  return humanizeMs(ms);
}

function stepDuration(e: RunEvent): string {
  if (!e.finished_at) return "…";
  const ms = new Date(e.finished_at).getTime() - new Date(e.started_at).getTime();
  return humanizeMs(ms);
}

function humanizeMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}
