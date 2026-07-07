use crate::config::Task;
use crate::db::{Db, RunComment};
use crate::opencode::{Cli, LogSink};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::process::Command;
use tokio::sync::Notify;

/// One-shot cancellation primitive shared with `cli.run_task` so the runner
/// can cut a long-running opencode child short on user request. Once cancelled
/// it stays cancelled — checks made after the fact still observe the signal.
/// Carries an optional `reason` so the cause (timeout / user / shutdown) can
/// be surfaced in the run record without inventing a separate channel.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<CancelInner>);

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    notify: Notify,
    reason: Mutex<Option<String>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }
    pub fn cancel_with_reason(&self, reason: impl Into<String>) {
        // First reason wins — avoids a later cancel (e.g. user clicking Stop
        // on an already-timed-out run) overwriting the more specific cause.
        let mut guard = self.0.reason.lock().unwrap();
        if guard.is_none() {
            *guard = Some(reason.into());
        }
        drop(guard);
        self.cancel();
    }
    pub fn reason(&self) -> Option<String> {
        self.0.reason.lock().unwrap().clone()
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }
    /// Resolves immediately if already cancelled; otherwise resolves on the
    /// next `cancel()` call. Safe to `tokio::select!` against — woken by
    /// `notify_waiters` even when invoked from a sync context.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.0.notify.notified().await;
    }
}

/// In-flight run bookkeeping behind one mutex: cancel tokens keyed by `run_id`
/// (the UI's Stop pulls a token out and calls `cancel()`), plus the set of task
/// ids that currently have a run in flight. Co-locating them lets "is this task
/// already running? if not, reserve it" happen atomically, so the same task
/// can't start two overlapping runs that would race on its memory/state.
#[derive(Default)]
pub struct RunRegistry {
    by_run: HashMap<i64, CancelToken>,
    active_tasks: HashSet<String>,
}

impl RunRegistry {
    /// Reserve `task_id` for a new run: returns `true` if it was free (caller
    /// may start) or `false` if a run for this task is already in flight (caller
    /// must skip). Atomic under the registry lock.
    pub fn try_reserve(&mut self, task_id: &str) -> bool {
        self.active_tasks.insert(task_id.to_string())
    }
    /// Drop a reservation taken by `try_reserve` before a `run_id` existed
    /// (e.g. the run row couldn't be created).
    pub fn unreserve(&mut self, task_id: &str) {
        self.active_tasks.remove(task_id);
    }
    /// Bind a reserved run's cancel token once its `run_id` is known.
    pub fn attach(&mut self, run_id: i64, token: CancelToken) {
        self.by_run.insert(run_id, token);
    }
    /// Release a finished/failed run: both its token and its task reservation.
    pub fn release(&mut self, run_id: i64, task_id: &str) {
        self.by_run.remove(&run_id);
        self.active_tasks.remove(task_id);
    }
    /// Cancel token for a run, if still in flight (used by the Stop command).
    pub fn token_for(&self, run_id: i64) -> Option<CancelToken> {
        self.by_run.get(&run_id).cloned()
    }
    /// Snapshot of all in-flight cancel tokens (used by graceful shutdown).
    pub fn tokens(&self) -> Vec<CancelToken> {
        self.by_run.values().cloned().collect()
    }
    /// Number of in-flight runs.
    pub fn len(&self) -> usize {
        self.by_run.len()
    }
    /// True when no run is in flight.
    pub fn is_empty(&self) -> bool {
        self.by_run.is_empty()
    }
}

/// Shared `RunRegistry`. The UI/scheduler/runner all hold an `Arc` clone.
pub type CancelRegistry = Arc<Mutex<RunRegistry>>;

pub fn new_cancel_registry() -> CancelRegistry {
    Arc::new(Mutex::new(RunRegistry::default()))
}

/// RAII handle that releases a run's registry slot (cancel token + task
/// reservation) on drop, so every exit path from `execute` — including early
/// `?` returns — frees the slot and unblocks the next run of the task.
struct RunSlot<'a> {
    registry: &'a CancelRegistry,
    run_id: i64,
    task_id: String,
}

impl Drop for RunSlot<'_> {
    fn drop(&mut self) {
        self.registry
            .lock()
            .unwrap()
            .release(self.run_id, &self.task_id);
    }
}

/// Lifecycle event emitted by the runner — consumed by the Tauri layer to
/// push real-time updates into the React frontend. Stays GUI-agnostic so the
/// runner can be tested standalone.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunUpdate {
    Started { run_id: i64, task_id: String },
    EventStarted { run_id: i64, event_id: i64, name: String },
    EventFinished { run_id: i64, event_id: i64, status: String, message: Option<String> },
    SessionAssigned { run_id: i64, session_id: String },
    LogLine {
        run_id: i64,
        log_id: i64,
        stream: String,
        line_no: i64,
        text: String,
    },
    Finished { run_id: i64, task_id: String, status: String, error: Option<String> },
}

pub type RunNotifier = Arc<dyn Fn(RunUpdate) + Send + Sync>;

/// Bundles `db` + optional event notifier so step-event writes and frontend
/// emits stay in lockstep. Cheap to clone-by-reference.
struct RunCtx<'a> {
    db: &'a Db,
    notifier: Option<&'a RunNotifier>,
}

impl<'a> RunCtx<'a> {
    fn emit(&self, u: RunUpdate) {
        if let Some(n) = self.notifier {
            n(u);
        }
    }
    fn start_event(&self, run_id: i64, name: &str) -> Option<i64> {
        let id = self.db.start_event(run_id, name).ok();
        if let Some(id) = id {
            self.emit(RunUpdate::EventStarted {
                run_id,
                event_id: id,
                name: name.to_string(),
            });
        }
        id
    }
    fn finish_event(&self, run_id: i64, event_id: i64, status: &str, message: Option<&str>) {
        let _ = self.db.finish_event(event_id, status, message);
        self.emit(RunUpdate::EventFinished {
            run_id,
            event_id,
            status: status.to_string(),
            message: message.map(str::to_string),
        });
    }
}

/// Cheap heuristic so both the UI and the runner agree on what counts as a
/// git repo. `.git` is either a directory (normal checkout) or a file
/// (linked worktree); both qualify.
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Execute one task via the `opencode run` CLI; log start/finish in db.
///
/// `registry` lets the UI cancel an in-flight run by `run_id`. The token is
/// inserted on start, passed into `cli.run_task`, and removed once the run
/// has fully wound down (including worktree cleanup).
/// `notifier`, if set, gets lifecycle and step events for the Tauri layer to
/// fan out to the webview.
pub async fn execute(
    task: &Task,
    cli: &Cli,
    db: &Db,
    registry: &CancelRegistry,
    notifier: Option<RunNotifier>,
    // Per-task run-history cap from settings. After the run finishes, older
    // finished runs beyond this many are pruned. None/Some(0) = unlimited.
    max_history: Option<u64>,
) -> Result<Option<i64>> {
    // One run per task at a time: if a run for this task is already in flight,
    // skip rather than start a second that would race on its memory/state.
    // Reserve atomically so two near-simultaneous triggers can't both pass.
    if !registry.lock().unwrap().try_reserve(&task.id) {
        tracing::warn!(task = %task.id, "skipping run: a run for this task is already in flight");
        return Ok(None);
    }
    let run_id = match db.insert_run_start(&task.id) {
        Ok(id) => id,
        Err(e) => {
            // Couldn't create the run row — drop the reservation we just took.
            registry.lock().unwrap().unreserve(&task.id);
            return Err(e);
        }
    };
    tracing::info!(task = %task.id, run_id, "starting task");

    let ctx = RunCtx {
        db,
        notifier: notifier.as_ref(),
    };
    ctx.emit(RunUpdate::Started {
        run_id,
        task_id: task.id.clone(),
    });

    let cancel = CancelToken::new();
    registry.lock().unwrap().attach(run_id, cancel.clone());
    // From here on, every exit path (incl. early `?` returns) frees the slot.
    let _slot = RunSlot {
        registry,
        run_id,
        task_id: task.id.clone(),
    };

    // Per-task timeout: spawn a sleep that flips the cancel token with a
    // distinct reason. Aborted via `JoinHandle::abort` once execute() falls
    // off so a successfully-finished run doesn't leave an orphan timer.
    let timeout_handle = task.timeout_secs.filter(|s| *s > 0).map(|secs| {
        let cancel_for_timeout = cancel.clone();
        let task_id = task.id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            if !cancel_for_timeout.is_cancelled() {
                tracing::warn!(task = %task_id, "run timed out after {secs}s; cancelling");
                cancel_for_timeout.cancel_with_reason(format!("timed out after {secs}s"));
            }
        })
    });

    // Optionally swap working_dir for a throwaway git worktree.
    let worktree = match prepare_worktree(task, &ctx, run_id).await {
        Ok(w) => w,
        Err(e) => {
            let msg = format!("worktree setup failed: {e:#}");
            db.finish_run(run_id, "error", Some(&msg))?;
            ctx.emit(RunUpdate::Finished {
                run_id,
                task_id: task.id.clone(),
                status: "error".into(),
                error: Some(msg.clone()),
            });
            tracing::error!(task = %task.id, run_id, "{msg}");
            // `_slot` releases the registry slot on return.
            return Err(e);
        }
    };
    let effective_dir: &Path = worktree
        .as_ref()
        .map(|w| w.path.as_path())
        .unwrap_or(task.working_dir.as_path());

    // Wire the scoped MCP server (this binary, `mcp-memory` subcommand) for
    // EVERY run: it exposes per-run `summary_*` tools the agent uses to record
    // what it did, plus — when the task opts into memory — the task-scoped
    // `memory_*` tools. `None` only if we can't resolve our own exe path, in
    // which case the run proceeds with no MCP tools and the prompt's tool
    // instructions are suppressed.
    let mcp_config: Option<String> = crate::mcp_memory::opencode_config_content(
        &task.id,
        run_id,
        db.path(),
        task.memory_enabled,
    );

    // Memory-only context: saved memory + recent user comments are folded into
    // the prompt only when the task opts into memory; otherwise the prompt keeps
    // just its objective plus the (always-on) summary instructions.
    let saved_memory = if task.memory_enabled {
        db.get_task_memory(&task.id).ok().flatten()
    } else {
        None
    };
    let comments = if task.memory_enabled {
        db.recent_comments_for_task(&task.id, 10).unwrap_or_default()
    } else {
        Vec::new()
    };
    let effective_prompt: String = build_augmented_prompt(
        &task.prompt,
        task.memory_enabled,
        saved_memory.as_ref().map(|m| m.content.as_str()),
        &comments,
        task.memory_enabled && mcp_config.is_some(),
        mcp_config.is_some(),
    );

    // Persist exactly what we're about to send so the History tab can show it
    // verbatim — independent of whether opencode allocates a session.
    if let Err(e) = db.set_run_prompt(run_id, &effective_prompt) {
        tracing::warn!(task = %task.id, "recording run prompt failed: {e:#}");
    }

    let opencode_evt = ctx.start_event(run_id, "Run opencode");

    // Callback: as soon as opencode emits its `ses_…` token, surface it to
    // the UI (and stash in db) so the conversation viewer can attach mid-run
    // instead of waiting for the process to exit.
    let on_session_db = db.clone();
    let on_session_notifier = notifier.clone();
    let on_session: Option<Box<dyn FnOnce(String) + Send + 'static>> =
        Some(Box::new(move |sid: String| {
            let _ = on_session_db.set_run_session(run_id, &sid, None);
            if let Some(n) = on_session_notifier.as_ref() {
                n(RunUpdate::SessionAssigned {
                    run_id,
                    session_id: sid,
                });
            }
        }));

    // Per-line stdout/stderr sink: persist to db (so the History tab can show
    // older runs) and emit a `LogLine` for any live listeners. Line numbers
    // are run-scoped and monotonic across both streams so the UI can render
    // them in a single chronological tail.
    let log_sink: LogSink = {
        let db = db.clone();
        let notifier_for_sink = notifier.clone();
        let counter = Arc::new(AtomicI64::new(0));
        Arc::new(move |stream, mut text| {
            cap_log_line(&mut text);
            let line_no = counter.fetch_add(1, Ordering::SeqCst);
            match db.append_log(run_id, stream, line_no, &text) {
                Ok(log_id) => {
                    if let Some(n) = notifier_for_sink.as_ref() {
                        n(RunUpdate::LogLine {
                            run_id,
                            log_id,
                            stream: stream.to_string(),
                            line_no,
                            text,
                        });
                    }
                }
                Err(e) => tracing::warn!("append_log failed for run {run_id}: {e}"),
            }
        })
    };

    let outcome = cli
        .run_task(
            effective_dir,
            &effective_prompt,
            task.model.as_deref(),
            task.dangerously_skip_permissions,
            cancel.clone(),
            on_session,
            Some(log_sink),
            mcp_config.as_deref(),
        )
        .await;

    // Stop the timeout timer — if the run already finished we don't want it
    // to fire later and try to cancel a fresh re-run by accident.
    if let Some(h) = timeout_handle {
        h.abort();
    }

    let (final_status, final_error): (&str, Option<String>) = match &outcome {
        Ok(o) if o.cancelled => {
            // Session id (if any) was already emitted mid-stream by the
            // `on_session` callback above — no duplicate emission here.
            let msg = cancel
                .reason()
                .unwrap_or_else(|| "aborted by user".to_string());
            if let Some(id) = opencode_evt {
                ctx.finish_event(run_id, id, "aborted", Some(&msg));
            }
            tracing::info!(task = %task.id, run_id, session = ?o.session_id, "task aborted: {msg}");
            ("aborted", Some(msg))
        }
        Ok(o) => {
            if o.exit_status.success() {
                if let Some(id) = opencode_evt {
                    ctx.finish_event(run_id, id, "ok", o.session_id.as_deref());
                }
                tracing::info!(task = %task.id, run_id, session = ?o.session_id, "task ok");
                ("ok", None)
            } else {
                let msg = format!(
                    "opencode run exited {:?}\n{}",
                    o.exit_status.code(),
                    o.stderr_tail.trim()
                );
                if let Some(id) = opencode_evt {
                    ctx.finish_event(run_id, id, "error", Some(&msg));
                }
                tracing::warn!(task = %task.id, run_id, "task failed: {msg}");
                ("error", Some(msg))
            }
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if let Some(id) = opencode_evt {
                ctx.finish_event(run_id, id, "error", Some(&msg));
            }
            tracing::error!(task = %task.id, run_id, "task failed to launch: {msg}");
            ("error", Some(msg))
        }
    };

    db.finish_run(run_id, final_status, final_error.as_deref())?;

    // No post-run memory handling: a memory-enabled task updates its memory
    // in-run by calling the scoped MCP tools (runmem_*), which write straight
    // to the db. The History tab's memory panel reflects those writes via the
    // row's updated_at.

    // Tear the worktree down regardless of success/failure. Cleanup errors
    // are logged but don't override the run's outcome — a dangling worktree
    // is best-effort cleaned up by `git worktree prune` later.
    if let Some(w) = worktree {
        let evt = ctx.start_event(run_id, "Worktree: cleanup");
        match w.cleanup().await {
            Ok(()) => {
                if let Some(id) = evt {
                    ctx.finish_event(run_id, id, "ok", None);
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if let Some(id) = evt {
                    ctx.finish_event(run_id, id, "error", Some(&msg));
                }
                tracing::warn!(task = %task.id, "worktree cleanup failed: {msg}");
            }
        }
    }

    ctx.emit(RunUpdate::Finished {
        run_id,
        task_id: task.id.clone(),
        status: final_status.to_string(),
        error: final_error,
    });
    // `_slot` releases the registry slot when execute() returns below.

    // Enforce the run-history retention cap now that this run is recorded.
    // Best-effort: a prune failure is logged but never changes the outcome.
    if let Some(keep) = max_history.filter(|k| *k > 0) {
        match db.prune_finished_runs_for_task(&task.id, keep) {
            Ok(n) if n > 0 => {
                tracing::info!(task = %task.id, "pruned {n} old run(s) over retention cap {keep}")
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(task = %task.id, "pruning run history failed: {e:#}"),
        }
    }

    // Map Err outcome from cli back to a top-level error so callers see it.
    match outcome {
        Ok(_) => Ok(Some(run_id)),
        Err(e) => Err(e),
    }
}

struct WorktreeHandle {
    repo: PathBuf,
    path: PathBuf,
}

impl WorktreeHandle {
    async fn cleanup(self) -> Result<()> {
        // `git worktree remove` rewrites the repo's worktree/ref metadata, so
        // take the same per-repo lock the setup path uses — otherwise cleanup
        // could race a sibling task's concurrent fetch/add on this repo.
        let _git_lock = crate::gitlock::acquire(&self.repo).await;
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&self.repo)
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(&self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::proc::no_window(&mut cmd);
        let out = cmd.output().await?;
        if !out.status.success() {
            tracing::warn!(
                "git worktree remove failed for {:?}: {}",
                self.path,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        if self.path.exists() {
            tokio::fs::remove_dir_all(&self.path).await.ok();
        }
        Ok(())
    }
}

async fn prepare_worktree(
    task: &Task,
    ctx: &RunCtx<'_>,
    run_id: i64,
) -> Result<Option<WorktreeHandle>> {
    if !task.run_in_worktree {
        return Ok(None);
    }
    if !is_git_repo(&task.working_dir) {
        tracing::warn!(
            task = %task.id,
            "run_in_worktree set but {:?} is not a git repo; running in original directory",
            task.working_dir
        );
        return Ok(None);
    }

    // Serialize ref-mutating git ops (fetch / worktree add) against any other
    // task sharing this repo. Two tasks on the same `working_dir` firing on the
    // same cron tick would otherwise run `git fetch --all` concurrently and race
    // on git's ref compare-and-swap ("incorrect old value provided"). Held only
    // through worktree setup — the opencode run afterwards never holds it.
    let git_lock = crate::gitlock::acquire(&task.working_dir).await;

    let base = task
        .worktree_base
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(b) = base {
        let evt = ctx.start_event(run_id, "Worktree: git fetch --all");
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&task.working_dir)
            .arg("fetch")
            .arg("--all")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::proc::no_window(&mut cmd);
        let fetch = cmd.output().await?;
        if !fetch.status.success() {
            let msg = format!(
                "exit {:?}: {}",
                fetch.status.code(),
                String::from_utf8_lossy(&fetch.stderr).trim()
            );
            if let Some(id) = evt {
                ctx.finish_event(run_id, id, "error", Some(&msg));
            }
            return Err(anyhow!("git fetch --all failed ({msg})"));
        }
        if let Some(id) = evt {
            ctx.finish_event(run_id, id, "ok", None);
        }

        let evt = ctx.start_event(run_id, &format!("Worktree: verify base `{b}`"));
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&task.working_dir)
            .arg("rev-parse")
            .arg("--verify")
            .arg("--quiet")
            .arg(format!("{b}^{{commit}}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        crate::proc::no_window(&mut cmd);
        let verify = cmd.output().await?;
        if !verify.status.success() {
            let msg = format!(
                "ref {b:?} not found after fetch — check the ref name (remote refs use `<remote>/<branch>` form)"
            );
            if let Some(id) = evt {
                ctx.finish_event(run_id, id, "error", Some(&msg));
            }
            return Err(anyhow!(
                "worktree base {b:?} not found in {:?} after fetch — aborting",
                task.working_dir
            ));
        }
        if let Some(id) = evt {
            ctx.finish_event(run_id, id, "ok", None);
        }
    }

    let tmp = std::env::temp_dir().join(format!(
        "opencode-runner-wt-{}",
        uuid::Uuid::new_v4()
    ));
    let add_label = match base {
        Some(b) => format!("Worktree: git worktree add (from `{b}`)"),
        None => "Worktree: git worktree add (from HEAD)".to_string(),
    };
    let evt = ctx.start_event(run_id, &add_label);
    let mut add = Command::new("git");
    add.arg("-C")
        .arg(&task.working_dir)
        .arg("worktree")
        .arg("add")
        .arg("--detach")
        .arg(&tmp);
    if let Some(b) = base {
        add.arg(b);
    }
    add.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::proc::no_window(&mut add);
    let out = add.output().await?;
    if !out.status.success() {
        let msg = format!(
            "exit {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
        if let Some(id) = evt {
            ctx.finish_event(run_id, id, "error", Some(&msg));
        }
        return Err(anyhow!("git worktree add failed ({msg})"));
    }
    if let Some(id) = evt {
        ctx.finish_event(run_id, id, "ok", Some(&tmp.display().to_string()));
    }
    tracing::info!(task = %task.id, worktree = ?tmp, base = ?base, "created worktree");

    // Refs are settled — release the repo lock so another task can fetch/add
    // while this run copies files and then runs opencode. The include copy
    // below writes into the new worktree, not the shared repo's refs.
    drop(git_lock);

    if task.working_dir.join(".worktreeinclude").exists() {
        let evt = ctx.start_event(run_id, "Worktree: apply .worktreeinclude");
        match apply_worktree_include(&task.working_dir, &tmp).await {
            Ok(()) => {
                if let Some(id) = evt {
                    ctx.finish_event(run_id, id, "ok", None);
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if let Some(id) = evt {
                    ctx.finish_event(run_id, id, "error", Some(&msg));
                }
                tracing::warn!(task = %task.id, "applying .worktreeinclude failed: {msg}");
            }
        }
    }

    Ok(Some(WorktreeHandle {
        repo: task.working_dir.clone(),
        path: tmp,
    }))
}

async fn apply_worktree_include(repo: &Path, worktree: &Path) -> Result<()> {
    let include_path = repo.join(".worktreeinclude");
    if !include_path.exists() {
        return Ok(());
    }
    let raw = tokio::fs::read_to_string(&include_path).await?;
    for line in raw.lines() {
        let entry = line.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        // Trim leading `/` and `\` so users can write `/foo` for "from repo
        // root" (gitignore convention). Without this, on Windows `repo.join`
        // would interpret `/foo` as "keep drive prefix, replace the rest" and
        // look outside the repo entirely.
        let entry = entry.trim_start_matches(['/', '\\']);
        if entry.is_empty() {
            continue;
        }
        if Path::new(entry).is_absolute() || entry.split(['/', '\\']).any(|c| c == "..") {
            tracing::warn!(".worktreeinclude: refusing suspicious path {entry:?}");
            continue;
        }
        let src = repo.join(entry);
        if !src.exists() {
            tracing::debug!(".worktreeinclude: {entry:?} not present in source; skipping");
            continue;
        }
        if !is_git_ignored(repo, entry).await {
            tracing::warn!(
                ".worktreeinclude: {entry:?} is tracked by git; refusing to copy (would clobber tracked code)"
            );
            continue;
        }
        let dst = worktree.join(entry);
        let src_owned = src.clone();
        let dst_owned = dst.clone();
        let res = tokio::task::spawn_blocking(move || copy_recursive_sync(&src_owned, &dst_owned))
            .await
            .map_err(|e| anyhow!("copy task join failed: {e}"))?;
        match res {
            Ok(()) => tracing::info!(".worktreeinclude: copied {entry:?}"),
            Err(e) => tracing::warn!(".worktreeinclude: copying {entry:?} failed: {e}"),
        }
    }
    Ok(())
}

async fn is_git_ignored(repo: &Path, rel: &str) -> bool {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo)
        .arg("check-ignore").arg("-q").arg("--").arg(rel)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::proc::no_window(&mut cmd);
    matches!(cmd.status().await, Ok(s) if s.success())
}

fn copy_recursive_sync(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive_sync(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

// ============================================================================
//                          Memory / comment injection
// ============================================================================

/// Compose the prompt actually sent to opencode, as a sequence of `##`-headed
/// sections: the user's objective; (when `memory_enabled`) the saved memory,
/// recent user comments, and a pointer to the MCP memory tools; and (when
/// `summary_tools_available`) a pointer to the per-run summary tools the agent
/// uses to record what it did. `memory_tools_available` / `summary_tools_available`
/// are false in the degenerate case where the MCP server couldn't be wired in,
/// in which case those sections are omitted. With nothing to augment (memory off
/// and no summary tools), the prompt is returned verbatim so a plain task stays
/// clean. Pure (no I/O) so it can be unit-tested.
fn build_augmented_prompt(
    prompt: &str,
    memory_enabled: bool,
    memory: Option<&str>,
    comments: &[RunComment],
    memory_tools_available: bool,
    summary_tools_available: bool,
) -> String {
    // Nothing to add — send the user's prompt exactly as written.
    if !memory_enabled && !summary_tools_available {
        return prompt.to_string();
    }

    let mut out = String::with_capacity(prompt.len() + 512);
    out.push_str("## Current objective\n");
    out.push_str(prompt.trim());
    out.push('\n');

    if memory_enabled {
        out.push_str("\n## Your memory (accumulated from previous runs)\n");
        match memory.map(str::trim).filter(|m| !m.is_empty()) {
            Some(m) => {
                out.push_str(m);
                out.push('\n');
            }
            None => out.push_str("(empty — nothing saved yet)\n"),
        }

        if !comments.is_empty() {
            out.push_str("\n## User feedback (most recent first)\n");
            for c in comments {
                // Comments arrive newest-first; render one bullet each, tagged
                // with the run they were left on and when, so the agent can
                // weigh them.
                let when = c.created_at.format("%Y-%m-%d %H:%M UTC");
                out.push_str(&format!(
                    "- [run #{}, {}] {}\n",
                    c.run_id,
                    when,
                    c.text.trim()
                ));
            }
        }

        if memory_tools_available {
            out.push_str(
                "\n## Updating your memory\n\
                 You have memory tools (provided over MCP, exposed with the `runmem_` prefix):\n\
                 - `runmem_memory_get` — read your current saved memory.\n\
                 - `runmem_memory_set` — replace your entire saved memory with new content.\n\
                 - `runmem_memory_append` — add a note to your memory without rewriting it.\n\
                 Your current memory is shown above for convenience. If anything is worth remembering\n\
                 for your future runs of this task, call `runmem_memory_append` (for incremental\n\
                 notes) or `runmem_memory_set` (to rewrite) before you finish. If nothing changed,\n\
                 don't call them. These tools only affect THIS task's memory.\n",
            );
        }
    }

    if summary_tools_available {
        out.push_str(
            "\n## Writing your run summary\n\
             Before you finish this run, call `runmem_summary_set` (provided over MCP) to\n\
             record a summary of this run. If the objective above asks you to produce a\n\
             report, answer, or output in a particular form, write THAT as the summary —\n\
             follow the user's requested format. Otherwise, write a concise summary of what\n\
             you did and how it turned out: what you changed or produced, the key result, and\n\
             anything notable or that failed. Use `runmem_summary_append` to build it up\n\
             incrementally during a long run. Always write a summary before you finish — even\n\
             if the outcome was \"nothing to do\". This summary is shown in the app's\n\
             run history; it is specific to THIS run.\n",
        );
    }

    out
}

/// Cap a log line at 4 KB so a runaway 10MB-on-one-line scenario can't blow
/// up the sqlite row. The cut must land on a char boundary: `String::truncate`
/// panics mid-character, and with `panic = "abort"` that killed the whole app
/// whenever a >4 KB line carried multi-byte text at the boundary (v0.8.0 crash
/// during translation runs).
fn cap_log_line(text: &mut String) {
    const MAX: usize = 4096;
    if text.len() > MAX {
        let mut cut = MAX;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("…[truncated]");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn comment(run_id: i64, text: &str) -> RunComment {
        RunComment {
            id: run_id,
            task_id: "t".into(),
            run_id,
            text: text.into(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn cap_log_line_short_line_untouched() {
        let mut s = "短行 short line".to_string();
        cap_log_line(&mut s);
        assert_eq!(s, "短行 short line");
    }

    #[test]
    fn cap_log_line_ascii_cuts_at_limit() {
        let mut s = "a".repeat(5000);
        cap_log_line(&mut s);
        assert_eq!(s, format!("{}…[truncated]", "a".repeat(4096)));
    }

    #[test]
    fn cap_log_line_backs_off_mid_char_boundary() {
        // Byte 4096 lands inside the 3-byte '中' (bytes 4095..4098); a plain
        // `truncate(4096)` panics here — the v0.8.0 crash.
        let mut s = "a".repeat(4095);
        s.push_str(&"中".repeat(200));
        cap_log_line(&mut s);
        assert_eq!(s, format!("{}…[truncated]", "a".repeat(4095)));
    }

    #[test]
    fn augmented_prompt_includes_memory_and_comments() {
        let p = build_augmented_prompt(
            "do the thing",
            true,
            Some("old note"),
            &[comment(3, "be careful"), comment(2, "use bullets")],
            true,
            true,
        );
        assert!(p.starts_with("## Current objective\ndo the thing"));
        assert!(p.contains("## Your memory"));
        assert!(p.contains("old note"));
        assert!(p.contains("## User feedback"));
        assert!(p.contains("[run #3"));
        assert!(p.contains("be careful"));
        // Tool mode advertises the MCP memory tools.
        assert!(p.contains("runmem_memory_set"));
        assert!(p.contains("runmem_memory_append"));
        // Summary tools are always advertised when available.
        assert!(p.contains("## Writing your run summary"));
        assert!(p.contains("runmem_summary_set"));
    }

    #[test]
    fn augmented_prompt_without_tools_omits_update_section() {
        // Degenerate case: MCP couldn't be wired in → memory shown read-only,
        // with no update mechanism and no summary instructions.
        let p = build_augmented_prompt("do the thing", true, Some("old note"), &[], false, false);
        assert!(p.contains("old note"));
        assert!(!p.contains("runmem_"));
        assert!(!p.contains("Updating your memory"));
        assert!(!p.contains("Writing your run summary"));
    }

    #[test]
    fn augmented_prompt_handles_empty_memory_and_no_comments() {
        let p = build_augmented_prompt("task", true, None, &[], false, false);
        assert!(p.contains("(empty — nothing saved yet)"));
        assert!(!p.contains("User feedback"));
    }

    #[test]
    fn summary_only_when_memory_disabled() {
        // The common case now: memory off, summary tools on for every run. The
        // prompt carries the objective + summary instructions, no memory section.
        let p = build_augmented_prompt("do it", false, None, &[], false, true);
        assert!(p.starts_with("## Current objective\ndo it"));
        assert!(!p.contains("Your memory"));
        assert!(!p.contains("runmem_memory_"));
        assert!(p.contains("## Writing your run summary"));
        assert!(p.contains("runmem_summary_set"));
    }

    #[test]
    fn verbatim_when_nothing_to_augment() {
        // Memory off and no summary tools (MCP unavailable) → prompt sent as-is.
        let p = build_augmented_prompt("raw prompt", false, None, &[], false, false);
        assert_eq!(p, "raw prompt");
    }
}
