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
  enabled: boolean;
}

export interface Settings {
  opencode_binary: string | null;
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

export interface Model {
  provider_id: string;
  model_id: string;
}

export interface BinaryStatus {
  configured: string | null;
  resolved_path: string;
  honored_configured: boolean;
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
      kind: "finished";
      run_id: number;
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
    enabled: false,
  };
}
