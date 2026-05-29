// Thin wrappers over Tauri's `invoke`. Centralized so component code never
// touches the IPC strings directly — easier to audit, easier to refactor.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

import type {
  BinaryStatus,
  MessagePair,
  Model,
  Run,
  RunEvent,
  RunUpdate,
  TasksFile,
} from "./types";

export const api = {
  getTasksFile: () => invoke<TasksFile>("get_tasks_file"),
  saveTasksFile: (file: TasksFile) => invoke<void>("save_tasks_file", { file }),
  listRuns: (limit?: number) => invoke<Run[]>("list_runs", { limit }),
  listRunsForTask: (taskId: string, limit?: number) =>
    invoke<Run[]>("list_runs_for_task", { taskId, limit }),
  listEvents: (runId: number) => invoke<RunEvent[]>("list_events", { runId }),
  loadConversation: (sessionId: string) =>
    invoke<MessagePair[]>("load_conversation", { sessionId }),
  opencodeBinaryStatus: () =>
    invoke<BinaryStatus>("opencode_binary_status"),
  listModels: () => invoke<Model[]>("list_models"),
  runNow: (taskId: string) => invoke<void>("run_now", { taskId }),
  abortRun: (runId: number) => invoke<void>("abort_run", { runId }),
  restartScheduler: () => invoke<void>("restart_scheduler"),
  isGitRepoPath: (path: string) =>
    invoke<boolean>("is_git_repo_path", { path }),
  showMainWindow: () => invoke<void>("show_main_window"),
};

export async function pickDirectory(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false });
  if (typeof picked === "string") return picked;
  return null;
}

export async function pickFile(): Promise<string | null> {
  const picked = await open({ directory: false, multiple: false });
  if (typeof picked === "string") return picked;
  return null;
}

export function onRunUpdate(
  handler: (u: RunUpdate) => void,
): Promise<UnlistenFn> {
  return listen<RunUpdate>("run-update", (e) => handler(e.payload));
}
