import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type UIEvent,
} from "react";
import { api } from "../api";
import type {
  ConversationPart,
  MessagePair,
  Run,
  RunEvent,
  RunLog,
  RunUpdate,
  Task,
} from "../types";
import { RefreshIcon, SquareIcon } from "./Icon";
import { StatusChip } from "./StatusChip";

const LOG_BUFFER_MAX = 800;

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
  const [logs, setLogs] = useState<RunLog[]>([]);

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
    setLogs([]);
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
    } else if (last.kind === "log_line" && last.run_id === activeRunId) {
      const incoming = last;
      setLogs((prev) => {
        // Dedup against any log already loaded by the initial fetch — the
        // tail query returns the latest N, so an event arriving moments
        // before the fetch resolves could otherwise be rendered twice.
        if (prev.some((l) => l.id === incoming.log_id)) return prev;
        const next = [
          ...prev,
          {
            id: incoming.log_id,
            run_id: incoming.run_id,
            stream: incoming.stream,
            line_no: incoming.line_no,
            ts: new Date().toISOString(),
            text: incoming.text,
          },
        ];
        return next.length > LOG_BUFFER_MAX
          ? next.slice(next.length - LOG_BUFFER_MAX)
          : next;
      });
    }
  }, [events, task.id, activeRunId, reload]);

  // Load events + conversation + logs for whichever run is selected.
  useEffect(() => {
    if (activeRunId == null) {
      setRunEvents([]);
      setConvo(null);
      setLogs([]);
      return;
    }
    api.listEvents(activeRunId).then(setRunEvents);
    api.listLogs(activeRunId).then(setLogs).catch(() => setLogs([]));
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

  // While a run is in flight and we have a session id, poll the on-disk
  // opencode conversation every 2s. opencode writes its session db as the
  // model streams, so this is the cheapest way to surface live output —
  // and stops as soon as the run leaves "running".
  useEffect(() => {
    if (!activeRun || activeRun.status !== "running" || !activeRun.session_id) return;
    const sid = activeRun.session_id;
    const id = setInterval(() => {
      api.loadConversation(sid).then(setConvo).catch(() => {});
    }, 2000);
    return () => clearInterval(id);
  }, [activeRun?.id, activeRun?.status, activeRun?.session_id]);

  // 1Hz tick to drive live elapsed-time labels on any running run (sidebar
  // cards + the active-run detail view + in-flight step durations). Only
  // armed when there's actually something running, so an idle History tab
  // doesn't re-render every second.
  const hasRunning =
    runs.some((r) => r.status === "running") ||
    runEvents.some((e) => e.finished_at == null);
  const [nowTick, setNowTick] = useState(0);
  useEffect(() => {
    if (!hasRunning) return;
    const id = setInterval(() => setNowTick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, [hasRunning]);
  // Force `now` to update on every tick re-render.
  const now = nowTick >= 0 ? Date.now() : Date.now();

  // Collapsible step rows. Default-collapsed; user clicks the row to toggle.
  // Reset when switching runs so cross-run state doesn't bleed over.
  const [expandedEvents, setExpandedEvents] = useState<Set<number>>(new Set());
  useEffect(() => {
    setExpandedEvents(new Set());
  }, [activeRunId]);
  function toggleEvent(id: number) {
    setExpandedEvents((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  return (
    <div className="panel" style={{ display: "flex", padding: 0 }}>
      <div className="history-layout" style={{ padding: "18px 24px 24px" }}>
        <div className="history-left">
          <div className="sticky-bar history-list-header">
            <span className="section-title" style={{ margin: 0 }}>
              Runs · {runs.length}
            </span>
            <button
              className="btn ghost icon"
              onClick={reload}
              aria-label="Refresh runs"
              title="Refresh"
            >
              <RefreshIcon size={15} />
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
                    started {formatTime(r.started_at)} · {duration(r, now)}
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
              logs={logs}
              conversation={convo}
              conversationError={convoError}
              now={now}
              expandedEvents={expandedEvents}
              onToggleEvent={toggleEvent}
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
  logs,
  conversation,
  conversationError,
  now,
  expandedEvents,
  onToggleEvent,
  onAbort,
}: {
  run: Run;
  events: RunEvent[];
  logs: RunLog[];
  conversation: MessagePair[] | null;
  conversationError: string | null;
  now: number;
  expandedEvents: Set<number>;
  onToggleEvent: (id: number) => void;
  onAbort: () => void;
}) {
  return (
    <div className="run-details">
      <div className="sticky-bar run-detail-header">
        <div className="row" style={{ gap: 8 }}>
          <span className="content-title" style={{ fontSize: 16 }}>
            Run #{run.id}
          </span>
          <StatusChip status={run.status} />
        </div>
        {run.status === "running" && (
          <button className="btn danger" onClick={onAbort}>
            <SquareIcon size={13} /> Stop
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
          <div className="help">
            {run.status === "running"
              ? "Waiting for first step…"
              : "No steps recorded."}
          </div>
        ) : (
          events.map((e) => {
            const expanded = expandedEvents.has(e.id);
            const hasMore = !!e.message;
            return (
              <div key={e.id}>
                <button
                  type="button"
                  className={`event-row ${hasMore ? "" : "event-row-flat"}`}
                  onClick={() => hasMore && onToggleEvent(e.id)}
                  aria-expanded={hasMore ? expanded : undefined}
                  disabled={!hasMore}
                  title={hasMore ? (expanded ? "Collapse" : "Expand") : ""}
                >
                  <span className="event-arrow">
                    {hasMore ? (expanded ? "▾" : "▸") : ""}
                  </span>
                  <StatusChip status={e.status} />
                  <span className="event-name">{e.name}</span>
                  <span className="help event-time">{formatTimeShort(e.started_at)}</span>
                  <span className="help event-dur">{stepDuration(e, now)}</span>
                </button>
                {expanded && hasMore && (
                  <div className="event-message">{e.message}</div>
                )}
              </div>
            );
          })
        )}
      </section>

      <LogsSection logs={logs} live={run.status === "running"} />

      <section className="section section-conversation">
        <div className="section-title">Conversation</div>
        {!run.session_id ? (
          <div className="help">
            {run.status === "running"
              ? "Waiting for opencode to allocate a session…"
              : "No session id captured for this run."}
          </div>
        ) : conversationError ? (
          <div className="error-text">{conversationError}</div>
        ) : conversation === null ? (
          <div className="help">Loading…</div>
        ) : conversation.length === 0 ? (
          <div className="help">
            {run.status === "running"
              ? "Streaming — first message will appear shortly…"
              : "Conversation is empty."}
          </div>
        ) : (
          conversation.map((pair, i) => (
            <div
              key={pair.message.id || i}
              className={`conv-msg ${pair.message.role === "assistant" ? "assistant" : ""}`}
            >
              <div className="conv-role">{pair.message.role ?? "?"}</div>
              {pair.parts.map((p) => (
                <ConvPart key={p.id} part={p} />
              ))}
            </div>
          ))
        )}
      </section>
    </div>
  );
}

// ============================================================================
//                                Logs (tail)
// ============================================================================

function LogsSection({ logs, live }: { logs: RunLog[]; live: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const [stickToBottom, setStickToBottom] = useState(true);
  const boxRef = useRef<HTMLPreElement | null>(null);

  // When new lines arrive on a live run, scroll to bottom only if the user is
  // already pinned there. Detaching from the bottom (by scrolling up) lets
  // them read older lines without being yanked back on every event.
  useEffect(() => {
    if (!expanded) return;
    const el = boxRef.current;
    if (!el || !stickToBottom) return;
    el.scrollTop = el.scrollHeight;
  }, [logs, expanded, stickToBottom]);

  function onScroll(e: UIEvent<HTMLPreElement>) {
    const el = e.currentTarget;
    const atBottom =
      el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    if (atBottom !== stickToBottom) setStickToBottom(atBottom);
  }

  const counts = countByStream(logs);
  const summary =
    logs.length === 0
      ? live
        ? "waiting…"
        : "no output captured"
      : `${logs.length} line${logs.length === 1 ? "" : "s"}` +
        (counts.stderr > 0 ? ` · ${counts.stderr} stderr` : "");

  return (
    <section className="section">
      <button
        type="button"
        className="logs-head"
        onClick={() => setExpanded((e) => !e)}
        aria-expanded={expanded}
      >
        <span className="event-arrow">{expanded ? "▾" : "▸"}</span>
        <span className="section-title" style={{ margin: 0 }}>
          Output
        </span>
        <span className="help logs-summary">{summary}</span>
        {live && expanded && !stickToBottom && (
          <span
            className="chip"
            onClick={(e) => {
              e.stopPropagation();
              setStickToBottom(true);
              const el = boxRef.current;
              if (el) el.scrollTop = el.scrollHeight;
            }}
            title="Scroll to bottom"
          >
            jump to live
          </span>
        )}
      </button>
      {expanded && (
        <pre
          ref={boxRef}
          className="logs-box mono"
          onScroll={onScroll}
        >
          {logs.length === 0 ? (
            <span className="help">No output captured yet.</span>
          ) : (
            logs.map((l) => (
              <span
                key={l.id}
                className={l.stream === "stderr" ? "log-line err" : "log-line"}
              >
                {l.text + "\n"}
              </span>
            ))
          )}
        </pre>
      )}
    </section>
  );
}

function countByStream(logs: RunLog[]): { stdout: number; stderr: number } {
  let stdout = 0;
  let stderr = 0;
  for (const l of logs) {
    if (l.stream === "stderr") stderr++;
    else stdout++;
  }
  return { stdout, stderr };
}

// ============================================================================
//                          Conversation part renderer
// ============================================================================

function ConvPart({ part }: { part: ConversationPart }) {
  const kind = part.kind ?? "text";
  if (kind === "text") return <PlainText text={part.text ?? ""} />;
  if (kind === "reasoning") return <Reasoning text={part.text ?? ""} />;
  if (kind === "tool") return <ToolCall extra={part.extra} />;
  if (kind === "step-start") return null;
  if (kind === "step-finish") return <StepFinish extra={part.extra} />;
  return (
    <div className="conv-part">
      <div className="conv-kind">{kind}</div>
      <pre className="conv-text mono">
        {JSON.stringify(part.extra, null, 2)}
      </pre>
    </div>
  );
}

function PlainText({ text }: { text: string }) {
  if (!text) return null;
  return (
    <div className="conv-part">
      <div className="conv-text">{text}</div>
    </div>
  );
}

function Reasoning({ text }: { text: string }) {
  if (!text) return null;
  return (
    <div className="conv-part conv-reasoning">
      <div className="conv-kind">Reasoning</div>
      <div className="conv-text">{text}</div>
    </div>
  );
}

function ToolCall({ extra }: { extra: Record<string, unknown> }) {
  const [expanded, setExpanded] = useState(false);
  const toolName = typeof extra.tool === "string" ? extra.tool : "tool";
  const state =
    (extra.state as Record<string, unknown> | undefined) ?? {};
  const status =
    typeof state.status === "string" ? (state.status as string) : "";
  const title = typeof state.title === "string" ? state.title : "";
  const input = state.input;
  const output =
    typeof state.output === "string"
      ? (state.output as string)
      : state.output != null
        ? JSON.stringify(state.output, null, 2)
        : "";

  return (
    <div className="conv-part tool-call">
      <button
        type="button"
        className="tool-head"
        onClick={() => setExpanded((e) => !e)}
        aria-expanded={expanded}
      >
        <span className="tool-arrow">{expanded ? "▾" : "▸"}</span>
        <span className="tool-name">{toolName}</span>
        {title && <span className="tool-title">{title}</span>}
        {status && <StatusChip status={statusToChip(status)} label={status} />}
      </button>
      {expanded && (
        <div className="tool-body">
          {input != null && (
            <div className="tool-section">
              <div className="conv-kind">input</div>
              <pre className="conv-text mono">
                {typeof input === "string"
                  ? input
                  : JSON.stringify(input, null, 2)}
              </pre>
            </div>
          )}
          {output && (
            <div className="tool-section">
              <div className="conv-kind">output</div>
              <pre className="conv-text mono">{output}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function statusToChip(s: string): string {
  if (s === "completed") return "ok";
  if (s === "running" || s === "pending") return "running";
  if (s === "error" || s === "failed") return "error";
  return "";
}

function StepFinish({ extra }: { extra: Record<string, unknown> }) {
  const tokens =
    (extra.tokens as Record<string, unknown> | undefined) ?? null;
  const cost = typeof extra.cost === "number" ? (extra.cost as number) : null;
  const reason = typeof extra.reason === "string" ? extra.reason : null;
  const inp = tokens && typeof tokens.input === "number" ? tokens.input : null;
  const out =
    tokens && typeof tokens.output === "number" ? tokens.output : null;
  const reas =
    tokens && typeof tokens.reasoning === "number" ? tokens.reasoning : null;
  const parts: string[] = [];
  if (reason) parts.push(reason);
  if (inp != null || out != null || reas != null) {
    const seg: string[] = [];
    if (inp != null) seg.push(`in ${inp}`);
    if (out != null) seg.push(`out ${out}`);
    if (reas != null && reas > 0) seg.push(`reasoning ${reas}`);
    parts.push(seg.join(" · "));
  }
  if (cost != null && cost > 0) parts.push(`$${cost.toFixed(4)}`);
  if (parts.length === 0) return null;
  return <div className="conv-part conv-step-finish">{parts.join(" · ")}</div>;
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString();
  } catch {
    return iso;
  }
}

// Time-only formatter for step rows — full date is shown in the run header.
function formatTimeShort(iso: string): string {
  try {
    const d = new Date(iso);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  } catch {
    return iso;
  }
}

function duration(r: Run, now: number): string {
  const end = r.finished_at ? new Date(r.finished_at).getTime() : now;
  return humanizeMs(end - new Date(r.started_at).getTime());
}

function stepDuration(e: RunEvent, now: number): string {
  const end = e.finished_at ? new Date(e.finished_at).getTime() : now;
  return humanizeMs(end - new Date(e.started_at).getTime());
}

function humanizeMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  if (m < 60) return `${m}m ${rem}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m ${rem}s`;
}
