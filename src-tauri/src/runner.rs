use crate::config::Task;
use crate::db::{Db, RunComment};
use crate::opencode::{Cli, LogSink};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::HashMap;
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

/// Map of in-flight `run_id`s to their cancel token. The UI holds an `Arc`
/// clone; clicking Stop on a run pulls the token out and calls `cancel()`.
pub type CancelRegistry = Arc<Mutex<HashMap<i64, CancelToken>>>;

pub fn new_cancel_registry() -> CancelRegistry {
    Arc::new(Mutex::new(HashMap::new()))
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
) -> Result<i64> {
    let run_id = db.insert_run_start(&task.id)?;
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
    registry.lock().unwrap().insert(run_id, cancel.clone());

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
            registry.lock().unwrap().remove(&run_id);
            return Err(e);
        }
    };
    let effective_dir: &Path = worktree
        .as_ref()
        .map(|w| w.path.as_path())
        .unwrap_or(task.working_dir.as_path());

    // When the task opts into memory, fold its saved memory plus recent user
    // comments into the prompt (and tell the agent how to update its memory).
    // Otherwise the prompt is sent verbatim so non-opted tasks stay clean.
    let effective_prompt: String = if task.memory_enabled {
        let memory = db.get_task_memory(&task.id).ok().flatten();
        let comments = db
            .recent_comments_for_task(&task.id, 10)
            .unwrap_or_default();
        build_augmented_prompt(
            &task.prompt,
            memory.as_ref().map(|m| m.content.as_str()),
            &comments,
        )
    } else {
        task.prompt.clone()
    };

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
            // Guard against a runaway 10MB-on-one-line scenario blowing up
            // the sqlite row. 4 KB is plenty for any sensible log line.
            if text.len() > 4096 {
                text.truncate(4096);
                text.push_str("…[truncated]");
            }
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

    // If memory is on and the run produced a session, parse the agent's final
    // reply for a `<memory>…</memory>` block and persist it (replacing prior
    // memory). No block → memory left untouched. Best-effort: failures here
    // never change the run's recorded outcome.
    if task.memory_enabled {
        let session_id = match &outcome {
            Ok(o) => o.session_id.clone(),
            Err(_) => None,
        };
        if let Some(sid) = session_id {
            match extract_memory_from_session(&sid) {
                Some(content) => match db.set_task_memory(&task.id, &content) {
                    Ok(()) => tracing::info!(
                        task = %task.id,
                        "updated task memory ({} chars) from run {run_id}",
                        content.len()
                    ),
                    Err(e) => tracing::warn!(task = %task.id, "saving task memory failed: {e:#}"),
                },
                None => tracing::debug!(
                    task = %task.id,
                    "no <memory> block in run {run_id} final reply; memory unchanged"
                ),
            }
        }
    }

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
    registry.lock().unwrap().remove(&run_id);

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
        Ok(_) => Ok(run_id),
        Err(e) => Err(e),
    }
}

struct WorktreeHandle {
    repo: PathBuf,
    path: PathBuf,
}

impl WorktreeHandle {
    async fn cleanup(self) -> Result<()> {
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
        "opencode-orchestrator-wt-{}",
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

/// Marker tags the agent uses to write memory. Kept here so the prompt builder
/// and the parser can't drift apart.
const MEMORY_OPEN: &str = "<memory>";
const MEMORY_CLOSE: &str = "</memory>";

/// Compose the prompt actually sent to opencode for a memory-enabled task:
/// the user's prompt, then an orchestrator-managed context block carrying the
/// saved memory, recent user comments, and instructions for updating memory.
/// Pure (no I/O) so it can be unit-tested.
fn build_augmented_prompt(prompt: &str, memory: Option<&str>, comments: &[RunComment]) -> String {
    let mut out = String::with_capacity(prompt.len() + 512);
    out.push_str(prompt.trim_end());
    out.push_str(
        "\n\n================ ORCHESTRATOR CONTEXT ================\n\
         These sections are managed by the orchestrator across runs of this task.\n",
    );

    out.push_str("\n## Your memory (saved from previous runs)\n");
    match memory.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => {
            out.push_str(m);
            out.push('\n');
        }
        None => out.push_str("(empty — nothing saved yet)\n"),
    }

    if !comments.is_empty() {
        out.push_str("\n## User feedback on previous runs (most recent first)\n");
        for c in comments {
            // Comments arrive newest-first; render one bullet each, tagged with
            // the run they were left on and when, so the agent can weigh them.
            let when = c.created_at.format("%Y-%m-%d %H:%M UTC");
            out.push_str(&format!(
                "- [run #{}, {}] {}\n",
                c.run_id,
                when,
                c.text.trim()
            ));
        }
    }

    out.push_str(&format!(
        "\n## Saving memory\n\
         If anything is worth remembering for your future runs of this task, end your reply\n\
         with a block exactly like:\n\
         {MEMORY_OPEN}\n\
         …what to remember…\n\
         {MEMORY_CLOSE}\n\
         This block REPLACES your entire current memory shown above, so include anything from\n\
         it you still want to keep. Omit the block to leave your memory unchanged.\n\
         =====================================================\n"
    ));
    out
}

/// Pull the agent's final assistant reply out of the opencode session and
/// extract its `<memory>` block, if any. Best-effort: any storage error or
/// missing session yields `None`.
fn extract_memory_from_session(session_id: &str) -> Option<String> {
    let convo = crate::opencode::storage::load_conversation(session_id).ok()?;
    let mut text = String::new();
    for (msg, parts) in convo.iter().rev() {
        if msg.role.as_deref() == Some("assistant") {
            for p in parts {
                if p.kind.as_deref() == Some("text") {
                    if let Some(t) = &p.text {
                        text.push_str(t);
                        text.push('\n');
                    }
                }
            }
            break;
        }
    }
    extract_memory_block(&text)
}

/// Extract the inner content of the last `<memory>…</memory>` block in `text`,
/// trimmed. Case-insensitive on the tags, spans newlines, and returns `None`
/// when there's no block or the block is empty. Pure, for unit testing.
fn extract_memory_block(text: &str) -> Option<String> {
    let open_pos = rfind_ci(text, MEMORY_OPEN)?;
    let after = open_pos + MEMORY_OPEN.len();
    let close_rel = find_ci(&text[after..], MEMORY_CLOSE)?;
    let inner = text[after..after + close_rel].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// First byte index of `needle` in `haystack`, ASCII-case-insensitively.
/// `needle` is ASCII (our tags), so matches land on char boundaries.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() || hb.len() < nb.len() {
        return None;
    }
    (0..=hb.len() - nb.len()).find(|&i| hb[i..i + nb.len()].eq_ignore_ascii_case(nb))
}

/// Last byte index of `needle` in `haystack`, ASCII-case-insensitively.
fn rfind_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() || hb.len() < nb.len() {
        return None;
    }
    (0..=hb.len() - nb.len())
        .rev()
        .find(|&i| hb[i..i + nb.len()].eq_ignore_ascii_case(nb))
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
    fn extracts_simple_block() {
        let r = extract_memory_block("blah\n<memory>\nremember this\n</memory>\nbye");
        assert_eq!(r.as_deref(), Some("remember this"));
    }

    #[test]
    fn extraction_is_case_insensitive() {
        let r = extract_memory_block("<MEMORY>X</Memory>");
        assert_eq!(r.as_deref(), Some("X"));
    }

    #[test]
    fn takes_last_block_when_multiple() {
        let r = extract_memory_block("<memory>first</memory> mid <memory>second</memory>");
        assert_eq!(r.as_deref(), Some("second"));
    }

    #[test]
    fn no_block_returns_none() {
        assert_eq!(extract_memory_block("nothing here"), None);
    }

    #[test]
    fn empty_block_returns_none() {
        assert_eq!(extract_memory_block("<memory>   \n  </memory>"), None);
    }

    #[test]
    fn handles_non_ascii_before_tag() {
        // Byte offsets must stay on char boundaries even with multibyte text.
        let r = extract_memory_block("前言。。。<memory>記住這個</memory>");
        assert_eq!(r.as_deref(), Some("記住這個"));
    }

    #[test]
    fn augmented_prompt_includes_memory_and_comments() {
        let p = build_augmented_prompt(
            "do the thing",
            Some("old note"),
            &[comment(3, "be careful"), comment(2, "use bullets")],
        );
        assert!(p.starts_with("do the thing"));
        assert!(p.contains("ORCHESTRATOR CONTEXT"));
        assert!(p.contains("old note"));
        assert!(p.contains("[run #3"));
        assert!(p.contains("be careful"));
        assert!(p.contains(MEMORY_OPEN));
    }

    #[test]
    fn augmented_prompt_handles_empty_memory_and_no_comments() {
        let p = build_augmented_prompt("task", None, &[]);
        assert!(p.contains("(empty — nothing saved yet)"));
        assert!(!p.contains("User feedback"));
    }
}
