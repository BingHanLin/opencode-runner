//! Tauri IPC surface. Every `#[tauri::command]` here is callable from the
//! React frontend via `invoke()`. Errors are surfaced as JSON strings so the
//! frontend doesn't need to special-case anyhow's structured chain.

use crate::config::{self, Settings, Task, TasksFile};
use crate::db::{Run, RunComment, RunEvent, RunLog, TaskMemory};
use crate::opencode::storage::{self, Message, Part};
use crate::opencode::{Cli, Model};
use crate::runner::{self, is_git_repo, RunNotifier};
use crate::scheduler::Scheduler;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

/// Convert any error chain into a string the webview can render directly.
fn s<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

// ---------- tasks file ----------

#[derive(Serialize, Deserialize)]
pub struct TasksFileDto {
    pub settings: Settings,
    pub tasks: Vec<Task>,
}

#[tauri::command]
pub fn get_tasks_file(state: State<'_, AppState>) -> Result<TasksFileDto, String> {
    let file = config::load(&state.config_path).map_err(s)?;
    Ok(TasksFileDto {
        settings: file.settings,
        tasks: file.tasks,
    })
}

#[tauri::command]
pub fn save_tasks_file(
    state: State<'_, AppState>,
    file: TasksFileDto,
) -> Result<(), String> {
    let on_disk = TasksFile {
        settings: file.settings,
        tasks: file.tasks,
    };
    config::save(&state.config_path, &on_disk).map_err(s)
}

// ---------- run history ----------

#[tauri::command]
pub fn list_runs(state: State<'_, AppState>, limit: Option<i64>) -> Result<Vec<Run>, String> {
    state.db.list_recent(limit.unwrap_or(200)).map_err(s)
}

#[tauri::command]
pub fn list_runs_for_task(
    state: State<'_, AppState>,
    task_id: String,
    limit: Option<i64>,
) -> Result<Vec<Run>, String> {
    state
        .db
        .list_recent_for_task(&task_id, limit.unwrap_or(100))
        .map_err(s)
}

#[tauri::command]
pub fn list_events(state: State<'_, AppState>, run_id: i64) -> Result<Vec<RunEvent>, String> {
    state.db.list_events_for_run(run_id).map_err(s)
}

#[tauri::command]
pub fn clear_runs_for_task(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<u64, String> {
    state.db.clear_finished_runs_for_task(&task_id).map_err(s)
}

#[tauri::command]
pub fn list_logs(
    state: State<'_, AppState>,
    run_id: i64,
    limit: Option<i64>,
) -> Result<Vec<RunLog>, String> {
    state.db.list_logs_for_run(run_id, limit.unwrap_or(500)).map_err(s)
}

// ---------- task memory & comments ----------

#[tauri::command]
pub fn get_task_memory(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Option<TaskMemory>, String> {
    state.db.get_task_memory(&task_id).map_err(s)
}

#[tauri::command]
pub fn set_task_memory(
    state: State<'_, AppState>,
    task_id: String,
    content: String,
) -> Result<(), String> {
    state.db.set_task_memory(&task_id, &content).map_err(s)
}

#[tauri::command]
pub fn list_comments_for_run(
    state: State<'_, AppState>,
    run_id: i64,
) -> Result<Vec<RunComment>, String> {
    state.db.list_comments_for_run(run_id).map_err(s)
}

#[tauri::command]
pub fn add_comment(
    state: State<'_, AppState>,
    task_id: String,
    run_id: i64,
    text: String,
) -> Result<RunComment, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("comment is empty".to_string());
    }
    state.db.add_comment(&task_id, run_id, trimmed).map_err(s)
}

#[tauri::command]
pub fn delete_comment(state: State<'_, AppState>, comment_id: i64) -> Result<(), String> {
    state.db.delete_comment(comment_id).map_err(s)
}

#[derive(Serialize)]
pub struct MessagePair {
    pub message: Message,
    pub parts: Vec<Part>,
}

#[tauri::command]
pub fn load_conversation(session_id: String) -> Result<Vec<MessagePair>, String> {
    let convo = storage::load_conversation(&session_id).map_err(s)?;
    Ok(convo
        .into_iter()
        .map(|(message, parts)| MessagePair { message, parts })
        .collect())
}

#[derive(Serialize)]
pub struct StoragePaths {
    /// `tasks.toml` — task definitions and settings live here.
    pub config_path: String,
    /// `runs.db` — this app's own SQLite db for run history, events, logs.
    pub runs_db: String,
    /// `opencode.db` — opencode CLI's session storage. We read it for the
    /// History tab's Conversation view; opencode itself owns the writes.
    pub opencode_session_db: String,
    /// OS temp dir. Worktree-enabled runs create
    /// `opencode-orchestrator-wt-<uuid>/` directories here and tear them
    /// down after the run; nothing is persisted across runs.
    pub worktree_root: String,
}

#[tauri::command]
pub fn storage_paths(state: State<'_, AppState>) -> Result<StoragePaths, String> {
    Ok(StoragePaths {
        config_path: state.config_path.display().to_string(),
        runs_db: state.db_path.display().to_string(),
        opencode_session_db: storage::db_path().display().to_string(),
        worktree_root: std::env::temp_dir().display().to_string(),
    })
}

// ---------- opencode CLI ----------

#[derive(Serialize)]
pub struct BinaryStatus {
    pub configured: Option<String>,
    pub resolved_path: String,
    pub honored_configured: bool,
}

fn current_cli(state: &AppState) -> (Cli, BinaryStatus) {
    let file = config::load(&state.config_path).unwrap_or_default();
    let configured = file.settings.opencode_binary.clone();
    let configured_ref = configured.as_deref();
    let (cli, honored) = Cli::resolve(configured_ref);
    let status = BinaryStatus {
        configured: configured.map(|p| p.display().to_string()),
        resolved_path: cli.binary.clone(),
        honored_configured: honored,
    };
    (cli, status)
}

#[tauri::command]
pub fn opencode_binary_status(state: State<'_, AppState>) -> BinaryStatus {
    current_cli(&state).1
}

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Result<Vec<Model>, String> {
    let (cli, _) = current_cli(&state);
    cli.list_models().await.map_err(s)
}

// ---------- task runs ----------

/// Build a notifier closure that broadcasts every `RunUpdate` to the webview
/// over the `run-update` Tauri event. The React side listens with
/// `listen('run-update', ...)`.
pub fn make_notifier(app: AppHandle) -> RunNotifier {
    use std::sync::Arc;
    Arc::new(move |update| {
        if let Err(e) = app.emit("run-update", &update) {
            tracing::warn!("emit run-update failed: {e}");
        }
    })
}

/// Fire-and-forget: spawns the run on a background tokio task and returns
/// immediately. The frontend correlates by listening for the matching
/// `RunUpdate::Started { task_id }` event on the `run-update` channel.
#[tauri::command]
pub async fn run_now(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let file = config::load(&state.config_path).map_err(s)?;
    let max_history = file.settings.max_run_history;
    let task = file
        .tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("task {task_id:?} not found in tasks.toml"))?;

    let (cli, _) = current_cli(&state);
    let db = state.db.clone();
    let registry = state.registry.clone();
    let notifier = Some(make_notifier(app.clone()));

    tokio::spawn(async move {
        let _ = runner::execute(&task, &cli, &db, &registry, notifier, max_history).await;
    });
    Ok(())
}

#[tauri::command]
pub fn abort_run(state: State<'_, AppState>, run_id: i64) -> Result<(), String> {
    let registry = state.registry.lock().unwrap();
    match registry.get(&run_id) {
        Some(token) => {
            token.cancel();
            Ok(())
        }
        None => Err(format!("run {run_id} is not currently active")),
    }
}

// ---------- scheduler ----------

#[tauri::command]
pub async fn restart_scheduler(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let file = config::load(&state.config_path).map_err(s)?;
    let (cli, _) = current_cli(&state);
    let notifier = Some(make_notifier(app.clone()));

    let mut guard = state.scheduler.lock().await;
    if let Some(old) = guard.take() {
        old.shutdown().await;
    }
    let new = Scheduler::new(
        cli,
        state.db.clone(),
        state.registry.clone(),
        notifier,
        file.settings.max_run_history,
    )
    .await
    .map_err(s)?;
    for task in file.tasks {
        new.register(task).await.map_err(s)?;
    }
    *guard = Some(new);
    Ok(())
}

// ---------- misc ----------

#[tauri::command]
pub fn is_git_repo_path(path: String) -> bool {
    is_git_repo(&PathBuf::from(path))
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    Ok(())
}
