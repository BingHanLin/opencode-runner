// Types mirror the serde output of the Rust backend (`src-tauri/src/`).
// Keep field names in sync — Tauri uses serde's `#[serde(rename_all)]` defaults.

export interface Task {
  id: string;
  name: string;
  /** "manual" | "cron:<expr>" | "once:<RFC3339>" */
  schedule: string;
  working_dir: string;
  model: string | null;
  prompt: string;
  dangerously_skip_permissions: boolean;
  run_in_worktree: boolean;
  worktree_base: string | null;
  /** Hard time budget per run, in seconds. 0/null = unbounded. Default 3600. */
  timeout_secs: number | null;
  /** Free-form labels for sidebar filtering. */
  tags: string[];
  /** When true, the runner injects saved memory + recent comments into the
   * prompt and exposes MCP memory tools the agent uses to update it mid-run. */
  memory_enabled: boolean;
  enabled: boolean;
}

export interface TaskMemory {
  task_id: string;
  content: string;
  updated_at: string;
}

export interface RunComment {
  id: number;
  task_id: string;
  run_id: number;
  text: string;
  created_at: string;
}

export interface Settings {
  opencode_binary: string | null;
  /** Max finished runs to retain per task; null or 0 = unlimited. Older runs
   * are pruned after each run finishes. */
  max_run_history: number | null;
}

export interface TasksFile {
  settings: Settings;
  tasks: Task[];
}

export interface Run {
  id: number;
  task_id: string;
  session_id: string | null;
  project_id: string | null;
  started_at: string;
  finished_at: string | null;
  status: string;
  error: string | null;
  /** Timestamp of the run's most recent log line, or null if it produced no
   * output. Used to flag runs killed after a long silence (a stall). */
  last_activity_at: string | null;
  /** The exact prompt sent to opencode (incl. injected memory/comments), or
   * null for runs recorded before this was captured. */
  prompt: string | null;
}

export interface RunEvent {
  id: number;
  run_id: number;
  name: string;
  status: string;
  started_at: string;
  finished_at: string | null;
  message: string | null;
}

export interface RunLog {
  id: number;
  run_id: number;
  stream: string; // "stdout" | "stderr"
  line_no: number;
  ts: string;
  text: string;
}

export interface Model {
  provider_id: string;
  model_id: string;
}

export interface BinaryStatus {
  configured: string | null;
  resolved_path: string;
  honored_configured: boolean;
}

export interface StoragePaths {
  config_path: string;
  runs_db: string;
  opencode_session_db: string;
  worktree_root: string;
}

export interface ConversationMessage {
  id: string;
  role: string | null;
  session_id: string | null;
  created_at: number | null;
  extra: Record<string, unknown>;
}

export interface ConversationPart {
  id: string;
  message_id: string | null;
  kind: string | null;
  text: string | null;
  extra: Record<string, unknown>;
}

export interface MessagePair {
  message: ConversationMessage;
  parts: ConversationPart[];
}

export type RunUpdate =
  | { kind: "started"; run_id: number; task_id: string }
  | { kind: "event_started"; run_id: number; event_id: number; name: string }
  | {
      kind: "event_finished";
      run_id: number;
      event_id: number;
      status: string;
      message: string | null;
    }
  | { kind: "session_assigned"; run_id: number; session_id: string }
  | {
      kind: "log_line";
      run_id: number;
      log_id: number;
      stream: string;
      line_no: number;
      text: string;
    }
  | {
      kind: "finished";
      run_id: number;
      task_id: string;
      status: string;
      error: string | null;
    };

export type ScheduleKind = "manual" | "cron" | "once";

export function parseScheduleKind(s: string): ScheduleKind {
  if (s.startsWith("cron:")) return "cron";
  if (s.startsWith("once:")) return "once";
  return "manual";
}

export function scheduleBody(s: string): string {
  if (s.startsWith("cron:")) return s.slice(5);
  if (s.startsWith("once:")) return s.slice(5);
  return "";
}

export function newBlankTask(): Task {
  return {
    id: crypto.randomUUID(),
    name: "Untitled task",
    schedule: "manual",
    working_dir: "",
    model: null,
    prompt: "",
    dangerously_skip_permissions: false,
    run_in_worktree: false,
    worktree_base: null,
    timeout_secs: 3600,
    tags: [],
    memory_enabled: false,
    enabled: false,
  };
}
