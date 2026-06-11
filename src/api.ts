// Thin wrappers over Tauri's `invoke`. Centralized so component code never
// touches the IPC strings directly — easier to audit, easier to refactor.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

import type {
  BinaryStatus,
  MessagePair,
  Model,
  Run,
  RunEvent,
  RunLog,
  RunUpdate,
  StoragePaths,
  TasksFile,
} from "./types";

export const api = {
  getTasksFile: () => invoke<TasksFile>("get_tasks_file"),
  saveTasksFile: (file: TasksFile) => invoke<void>("save_tasks_file", { file }),
  listRuns: (limit?: number) => invoke<Run[]>("list_runs", { limit }),
  listRunsForTask: (taskId: string, limit?: number) =>
    invoke<Run[]>("list_runs_for_task", { taskId, limit }),
  listEvents: (runId: number) => invoke<RunEvent[]>("list_events", { runId }),
  listLogs: (runId: number, limit?: number) =>
    invoke<RunLog[]>("list_logs", { runId, limit }),
  loadConversation: (sessionId: string) =>
    invoke<MessagePair[]>("load_conversation", { sessionId }),
  opencodeBinaryStatus: () =>
    invoke<BinaryStatus>("opencode_binary_status"),
  listModels: () => invoke<Model[]>("list_models"),
  runNow: (taskId: string) => invoke<void>("run_now", { taskId }),
  abortRun: (runId: number) => invoke<void>("abort_run", { runId }),
  clearRunsForTask: (taskId: string) =>
    invoke<number>("clear_runs_for_task", { taskId }),
  restartScheduler: () => invoke<void>("restart_scheduler"),
  isGitRepoPath: (path: string) =>
    invoke<boolean>("is_git_repo_path", { path }),
  showMainWindow: () => invoke<void>("show_main_window"),
  storagePaths: () => invoke<StoragePaths>("storage_paths"),
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

// ---- App auto-update (tauri-plugin-updater) ----

export type { Update } from "@tauri-apps/plugin-updater";

/** Ask the updater endpoint whether a newer signed release exists. Resolves
 *  to the Update handle when one is available, or null when up to date. */
export function checkForUpdate(): Promise<Update | null> {
  return check();
}

/** Restart the app — called after an update has been installed in place. */
export function relaunchApp(): Promise<void> {
  return relaunch();
}
