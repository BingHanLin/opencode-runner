use crate::config::Task;
use crate::db::Db;
use crate::opencode::Cli;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::process::Command;
use tokio::sync::Notify;

/// One-shot cancellation primitive shared with `cli.run_task` so the runner
/// can cut a long-running opencode child short on user request. Once cancelled
/// it stays cancelled — checks made after the fact still observe the signal.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<(AtomicBool, Notify)>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0 .0.store(true, Ordering::SeqCst);
        self.0 .1.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0 .0.load(Ordering::SeqCst)
    }
    /// Resolves immediately if already cancelled; otherwise resolves on the
    /// next `cancel()` call. Safe to `tokio::select!` against — woken by
    /// `notify_waiters` even when invoked from a sync context.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.0 .1.notified().await;
    }
}

/// Map of in-flight `run_id`s to their cancel token. The UI holds an `Arc`
/// clone; clicking Stop on a run pulls the token out and calls `cancel()`.
pub type CancelRegistry = Arc<Mutex<HashMap<i64, CancelToken>>>;

pub fn new_cancel_registry() -> CancelRegistry {
    Arc::new(Mutex::new(HashMap::new()))
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
pub async fn execute(task: &Task, cli: &Cli, db: &Db, registry: &CancelRegistry) -> Result<i64> {
    let run_id = db.insert_run_start(&task.id)?;
    tracing::info!(task = %task.id, run_id, "starting task");

    let cancel = CancelToken::new();
    registry
        .lock()
        .unwrap()
        .insert(run_id, cancel.clone());

    // Optionally swap working_dir for a throwaway git worktree. prepare_worktree
    // owns its own per-step event emissions so the timeline shows fetch /
    // verify / add / .worktreeinclude individually.
    let worktree = match prepare_worktree(task, db, run_id).await {
        Ok(w) => w,
        Err(e) => {
            let msg = format!("worktree setup failed: {e:#}");
            db.finish_run(run_id, "error", Some(&msg))?;
            tracing::error!(task = %task.id, run_id, "{msg}");
            registry.lock().unwrap().remove(&run_id);
            return Err(e);
        }
    };
    let effective_dir: &Path = worktree
        .as_ref()
        .map(|w| w.path.as_path())
        .unwrap_or(task.working_dir.as_path());

    let opencode_evt = db.start_event(run_id, "Run opencode").ok();
    let outcome = cli
        .run_task(
            effective_dir,
            &task.prompt,
            task.model.as_deref(),
            task.dangerously_skip_permissions,
            cancel.clone(),
        )
        .await;

    let result = match outcome {
        Ok(o) if o.cancelled => {
            if let Some(sid) = &o.session_id {
                let _ = db.set_run_session(run_id, sid, None);
            }
            let msg = "aborted by user";
            if let Some(id) = opencode_evt {
                let _ = db.finish_event(id, "aborted", Some(msg));
            }
            db.finish_run(run_id, "aborted", Some(msg))?;
            tracing::info!(task = %task.id, run_id, session = ?o.session_id, "task aborted by user");
            Ok(run_id)
        }
        Ok(o) => {
            if let Some(sid) = &o.session_id {
                let _ = db.set_run_session(run_id, sid, None);
            }
            if o.exit_status.success() {
                if let Some(id) = opencode_evt {
                    let _ = db.finish_event(id, "ok", o.session_id.as_deref());
                }
                db.finish_run(run_id, "ok", None)?;
                tracing::info!(task = %task.id, run_id, session = ?o.session_id, "task ok");
            } else {
                let msg = format!(
                    "opencode run exited {:?}\n{}",
                    o.exit_status.code(),
                    o.stderr_tail.trim()
                );
                if let Some(id) = opencode_evt {
                    let _ = db.finish_event(id, "error", Some(&msg));
                }
                db.finish_run(run_id, "error", Some(&msg))?;
                tracing::warn!(task = %task.id, run_id, "task failed: {msg}");
            }
            Ok(run_id)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            if let Some(id) = opencode_evt {
                let _ = db.finish_event(id, "error", Some(&msg));
            }
            db.finish_run(run_id, "error", Some(&msg))?;
            tracing::error!(task = %task.id, run_id, "task failed to launch: {msg}");
            Err(e)
        }
    };

    // Tear the worktree down regardless of success/failure. Cleanup errors
    // are logged but don't override the run's outcome — a dangling worktree
    // is best-effort cleaned up by `git worktree prune` later.
    if let Some(w) = worktree {
        let evt = db.start_event(run_id, "Worktree: cleanup").ok();
        match w.cleanup().await {
            Ok(()) => {
                if let Some(id) = evt {
                    let _ = db.finish_event(id, "ok", None);
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if let Some(id) = evt {
                    let _ = db.finish_event(id, "error", Some(&msg));
                }
                tracing::warn!(task = %task.id, "worktree cleanup failed: {msg}");
            }
        }
    }

    registry.lock().unwrap().remove(&run_id);
    result
}

struct WorktreeHandle {
    repo: PathBuf,
    path: PathBuf,
}

impl WorktreeHandle {
    async fn cleanup(self) -> Result<()> {
        // `git worktree remove --force` un-registers and deletes the dir.
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(&self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !out.status.success() {
            tracing::warn!(
                "git worktree remove failed for {:?}: {}",
                self.path,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        // Belt-and-suspenders: if the dir still exists (remove failed, or
        // opencode left a stray process holding a file), nuke it directly.
        if self.path.exists() {
            tokio::fs::remove_dir_all(&self.path).await.ok();
        }
        Ok(())
    }
}

async fn prepare_worktree(task: &Task, db: &Db, run_id: i64) -> Result<Option<WorktreeHandle>> {
    if !task.run_in_worktree {
        return Ok(None);
    }
    if !is_git_repo(&task.working_dir) {
        // Configured but not a git repo — fall back to running in-place so a
        // stale checkbox doesn't break the task entirely. The UI hides the
        // checkbox for non-git dirs, so this is a "tasks.toml drift" path.
        tracing::warn!(
            task = %task.id,
            "run_in_worktree set but {:?} is not a git repo; running in original directory",
            task.working_dir
        );
        return Ok(None);
    }

    // When the user pinned a base (e.g. "origin/main"), refresh remote refs
    // first and verify the base resolves; both steps abort the run on failure
    // rather than silently falling back to HEAD.
    let base = task.worktree_base.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if let Some(b) = base {
        let evt = db.start_event(run_id, "Worktree: git fetch --all").ok();
        let fetch = Command::new("git")
            .arg("-C")
            .arg(&task.working_dir)
            .arg("fetch")
            .arg("--all")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !fetch.status.success() {
            let msg = format!(
                "exit {:?}: {}",
                fetch.status.code(),
                String::from_utf8_lossy(&fetch.stderr).trim()
            );
            if let Some(id) = evt {
                let _ = db.finish_event(id, "error", Some(&msg));
            }
            return Err(anyhow!("git fetch --all failed ({msg})"));
        }
        if let Some(id) = evt {
            let _ = db.finish_event(id, "ok", None);
        }

        let evt = db.start_event(run_id, &format!("Worktree: verify base `{b}`")).ok();
        let verify = Command::new("git")
            .arg("-C")
            .arg(&task.working_dir)
            .arg("rev-parse")
            .arg("--verify")
            .arg("--quiet")
            .arg(format!("{b}^{{commit}}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !verify.status.success() {
            let msg = format!(
                "ref {b:?} not found after fetch — check the ref name (remote refs use `<remote>/<branch>` form)"
            );
            if let Some(id) = evt {
                let _ = db.finish_event(id, "error", Some(&msg));
            }
            return Err(anyhow!(
                "worktree base {b:?} not found in {:?} after fetch — aborting",
                task.working_dir
            ));
        }
        if let Some(id) = evt {
            let _ = db.finish_event(id, "ok", None);
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
    let evt = db.start_event(run_id, &add_label).ok();
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
    let out = add
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !out.status.success() {
        let msg = format!(
            "exit {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
        if let Some(id) = evt {
            let _ = db.finish_event(id, "error", Some(&msg));
        }
        return Err(anyhow!("git worktree add failed ({msg})"));
    }
    if let Some(id) = evt {
        let _ = db.finish_event(id, "ok", Some(&tmp.display().to_string()));
    }
    tracing::info!(task = %task.id, worktree = ?tmp, base = ?base, "created worktree");

    // Best-effort: copy entries listed in `.worktreeinclude`. Failures here
    // don't fail the run — the worktree itself is usable, the included files
    // are a convenience. We log every skip/copy decision.
    if task.working_dir.join(".worktreeinclude").exists() {
        let evt = db.start_event(run_id, "Worktree: apply .worktreeinclude").ok();
        match apply_worktree_include(&task.working_dir, &tmp).await {
            Ok(()) => {
                if let Some(id) = evt {
                    let _ = db.finish_event(id, "ok", None);
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if let Some(id) = evt {
                    let _ = db.finish_event(id, "error", Some(&msg));
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

/// Replicate the `.worktreeinclude` convention: copy git-ignored files (and
/// directories) listed in `<repo>/.worktreeinclude` into the new worktree, so
/// untracked but locally-essential files (`.env`, `.env.local`, …) are
/// available without the user having to seed each worktree manually.
///
/// Safety rules:
///   * Only ignored entries are copied. Tracked code is never overwritten.
///   * Absolute paths and `..` traversal are rejected.
///   * Missing source entries are skipped silently.
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
        if Path::new(entry).is_absolute()
            || entry.split(['/', '\\']).any(|c| c == "..")
        {
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
    // `git check-ignore -q <path>`: exit 0 = ignored, 1 = not ignored, 128 = error.
    matches!(
        Command::new("git")
            .arg("-C").arg(repo)
            .arg("check-ignore").arg("-q").arg("--").arg(rel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await,
        Ok(s) if s.success()
    )
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
