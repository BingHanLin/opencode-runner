//! Per-repository serialization of ref-mutating git operations.
//!
//! Two scheduled tasks that share a `working_dir` (e.g. both open a throwaway
//! worktree off the same repo) can fire on the same cron tick and run
//! `git fetch --all` concurrently. Git updates remote-tracking refs with a
//! compare-and-swap, and two simultaneous fetches race on it — the loser aborts
//! with `fetching ref refs/remotes/origin/... failed: incorrect old value
//! provided`. `git worktree add`/`remove` mutate the same on-disk ref state and
//! race the same way.
//!
//! This module hands out one async lock per repo path. Callers hold the guard
//! across the git commands that touch refs, so those run one-at-a-time *per
//! repo* — different repos stay fully parallel, and the long opencode run
//! itself never holds the lock (the guard is released the moment worktree setup
//! finishes).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// repo path -> its git lock. The outer std `Mutex` guards only the brief
/// get-or-create of the entry; the actual waiting happens on the inner tokio
/// `Mutex` and never blocks the async runtime.
static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>> {
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Acquire the git lock for `repo`, awaiting if another task on the same repo
/// holds it. Hold the returned guard for the duration of the ref-touching git
/// commands; drop it (let it fall out of scope, or `drop()` it explicitly) as
/// soon as they finish so the run itself doesn't serialize other repos' work.
pub async fn acquire(repo: &Path) -> OwnedMutexGuard<()> {
    // Canonicalize so different spellings of the same repo (`./repo`, `repo`,
    // an absolute path, trailing-slash variants) resolve to one lock. Fall
    // back to the raw path if the dir can't be canonicalized — still correct,
    // just keyed on the literal path.
    let key = std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    let lock = {
        let mut map = registry().lock().unwrap();
        map.entry(key)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    };
    lock.lock_owned().await
}
