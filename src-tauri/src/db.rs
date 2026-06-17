use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub id: i64,
    pub task_id: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: String, // "running", "ok", "error"
    pub error: Option<String>,
    /// Timestamp of this run's most recent log line (`MAX(run_logs.ts)`), or
    /// `None` if it produced no output. The UI compares it against
    /// `finished_at` to flag runs that were killed after a long silence — a
    /// stalled model stream or hung tool call rather than genuine work.
    pub last_activity_at: Option<DateTime<Utc>>,
    /// The exact prompt sent to opencode for this run, including any memory /
    /// comment context injected by the runner. `None` for runs created before
    /// this was recorded. Surfaced read-only in the History tab.
    pub prompt: Option<String>,
}

/// One phase of a run — e.g. "Worktree: git fetch --all" or "Run opencode".
/// Persisted so the UI can render a timeline that survives restarts.
#[derive(Debug, Clone, Serialize)]
pub struct RunEvent {
    pub id: i64,
    pub run_id: i64,
    pub name: String,
    pub status: String, // "running", "ok", "error"
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
}

/// One line of raw stdout/stderr captured from the opencode child. Persisted
/// so the History tab can show what the CLI actually printed, including
/// stderr warnings the JSON event stream doesn't surface.
#[derive(Debug, Clone, Serialize)]
pub struct RunLog {
    pub id: i64,
    pub run_id: i64,
    pub stream: String, // "stdout" | "stderr"
    pub line_no: i64,
    pub ts: DateTime<Utc>,
    pub text: String,
}

/// A task's evolving memory — a single blob the agent updates across runs via
/// the scoped MCP memory tools (see `crate::mcp_memory`). Keyed by task id so it
/// survives task renames and lives independently of run history.
#[derive(Debug, Clone, Serialize)]
pub struct TaskMemory {
    pub task_id: String,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

/// A user-written comment attached to one run. Surfaced in the History tab and
/// fed back (most-recent-N) into the next run's prompt as standing feedback.
#[derive(Debug, Clone, Serialize)]
pub struct RunComment {
    pub id: i64,
    pub task_id: String,
    pub run_id: i64,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Db {
    inner: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening db at {}", path.display()))?;
        // WAL lets the main app and the out-of-process MCP memory server (which
        // opens this same file independently) read/write concurrently; the
        // busy_timeout makes a writer wait briefly for a competing writer
        // instead of failing with SQLITE_BUSY. Memory writes are tiny and rare,
        // so contention is negligible. Best-effort: a PRAGMA failure here
        // shouldn't stop the app from opening the db.
        let _ = conn.execute_batch(
            "PRAGMA journal_mode = WAL;\nPRAGMA busy_timeout = 5000;",
        );
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS runs (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id      TEXT NOT NULL,
                session_id   TEXT,
                project_id   TEXT,
                started_at   TEXT NOT NULL,
                finished_at  TEXT,
                status       TEXT NOT NULL,
                error        TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_runs_task_id ON runs(task_id);
            CREATE INDEX IF NOT EXISTS idx_runs_started_at ON runs(started_at DESC);

            CREATE TABLE IF NOT EXISTS run_events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id       INTEGER NOT NULL,
                name         TEXT NOT NULL,
                status       TEXT NOT NULL,
                started_at   TEXT NOT NULL,
                finished_at  TEXT,
                message      TEXT,
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_run_events_run_id ON run_events(run_id, id);

            CREATE TABLE IF NOT EXISTS run_logs (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id       INTEGER NOT NULL,
                stream       TEXT NOT NULL,
                line_no      INTEGER NOT NULL,
                ts           TEXT NOT NULL,
                text         TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_run_logs_run_id ON run_logs(run_id, id);

            CREATE TABLE IF NOT EXISTS task_memory (
                task_id      TEXT PRIMARY KEY,
                content      TEXT NOT NULL,
                updated_at   TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS run_comments (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id      TEXT NOT NULL,
                run_id       INTEGER NOT NULL,
                text         TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_run_comments_run_id  ON run_comments(run_id);
            CREATE INDEX IF NOT EXISTS idx_run_comments_task_id ON run_comments(task_id, id DESC);
            "#,
        )?;

        // Migration: older DBs predate the `prompt` column on `runs`. Add it if
        // missing so we can record the exact (memory/comment-augmented) prompt
        // actually sent to opencode for each run. Runs created before this stay
        // NULL and the UI just hides the section for them.
        let has_prompt: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('runs') WHERE name = 'prompt'")?
            .exists([])?;
        if !has_prompt {
            conn.execute("ALTER TABLE runs ADD COLUMN prompt TEXT", [])?;
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
            path: path.to_path_buf(),
        })
    }

    /// Filesystem path this db was opened from. Used to hand the out-of-process
    /// MCP memory server the exact same database file via an env var.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Mark every run still flagged `running` (plus any open step events) as
    /// `aborted`. Called once at startup: a freshly-launched process can't have
    /// any genuinely in-flight runs, so anything left `running` is a leftover
    /// from a previous process that died without finishing — a force-kill
    /// (Task Manager), crash, or OS shutdown/sleep that bypassed the tray-Quit
    /// graceful path. Without this, such rows show as perpetually "running" in
    /// the UI forever. `finished_at` is set to the run's last log timestamp so
    /// the recorded duration reflects when work actually stopped, not boot
    /// time. Returns the number of runs reconciled.
    pub fn reconcile_orphaned_runs(&self) -> Result<u64> {
        const MSG: &str = "interrupted: app exited while run was in progress";
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        // Close dangling step events first, falling back to boot time when a
        // run produced no logs to borrow a timestamp from.
        conn.execute(
            "UPDATE run_events
                SET status = 'aborted',
                    finished_at = COALESCE(
                        finished_at,
                        (SELECT MAX(ts) FROM run_logs WHERE run_logs.run_id = run_events.run_id),
                        ?1
                    ),
                    message = COALESCE(message, ?2)
              WHERE status = 'running'",
            params![now, MSG],
        )?;
        let n = conn.execute(
            "UPDATE runs
                SET status = 'aborted',
                    finished_at = COALESCE(
                        (SELECT MAX(ts) FROM run_logs WHERE run_logs.run_id = runs.id),
                        started_at
                    ),
                    error = ?1
              WHERE status = 'running'",
            params![MSG],
        )?;
        Ok(n as u64)
    }

    pub fn insert_run_start(&self, task_id: &str) -> Result<i64> {
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO runs (task_id, started_at, status) VALUES (?1, ?2, 'running')",
            params![task_id, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_run_session(&self, run_id: i64, session_id: &str, project_id: Option<&str>) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE runs SET session_id = ?1, project_id = ?2 WHERE id = ?3",
            params![session_id, project_id, run_id],
        )?;
        Ok(())
    }

    /// Record the exact prompt sent to opencode for this run (after any memory /
    /// comment augmentation), so the History tab can show what was really sent.
    pub fn set_run_prompt(&self, run_id: i64, prompt: &str) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE runs SET prompt = ?1 WHERE id = ?2",
            params![prompt, run_id],
        )?;
        Ok(())
    }

    pub fn finish_run(&self, run_id: i64, status: &str, error: Option<&str>) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE runs SET finished_at = ?1, status = ?2, error = ?3 WHERE id = ?4",
            params![now, status, error, run_id],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: i64) -> Result<Vec<Run>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, session_id, project_id, started_at, finished_at, status, error,
                    (SELECT MAX(ts) FROM run_logs WHERE run_id = runs.id) AS last_activity_at,
                    prompt
             FROM runs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_run)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_recent_for_task(&self, task_id: &str, limit: i64) -> Result<Vec<Run>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, session_id, project_id, started_at, finished_at, status, error,
                    (SELECT MAX(ts) FROM run_logs WHERE run_id = runs.id) AS last_activity_at,
                    prompt
             FROM runs WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![task_id, limit], row_to_run)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn start_event(&self, run_id: i64, name: &str) -> Result<i64> {
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO run_events (run_id, name, status, started_at) VALUES (?1, ?2, 'running', ?3)",
            params![run_id, name, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_event(&self, event_id: i64, status: &str, message: Option<&str>) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE run_events SET finished_at = ?1, status = ?2, message = ?3 WHERE id = ?4",
            params![now, status, message, event_id],
        )?;
        Ok(())
    }

    pub fn list_events_for_run(&self, run_id: i64) -> Result<Vec<RunEvent>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, run_id, name, status, started_at, finished_at, message
             FROM run_events WHERE run_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![run_id], row_to_event)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn append_log(
        &self,
        run_id: i64,
        stream: &str,
        line_no: i64,
        text: &str,
    ) -> Result<i64> {
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO run_logs (run_id, stream, line_no, ts, text) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![run_id, stream, line_no, now, text],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Latest `limit` log lines for a run, returned in chronological order.
    /// We page from the tail because runs can produce thousands of lines and
    /// the UI only ever shows a tail; reading the whole table is wasteful.
    pub fn list_logs_for_run(&self, run_id: i64, limit: i64) -> Result<Vec<RunLog>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM (
                 SELECT id, run_id, stream, line_no, ts, text
                 FROM run_logs WHERE run_id = ?1
                 ORDER BY id DESC LIMIT ?2
             ) ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![run_id, limit], row_to_log)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Drop history for a task: every finished run plus its associated step
    /// events and log lines. In-flight runs (status == 'running') are kept
    /// untouched so a user clicking Clear mid-run doesn't blow away state the
    /// runner is still writing to. Returns the number of runs removed.
    pub fn clear_finished_runs_for_task(&self, task_id: &str) -> Result<u64> {
        let mut conn = self.inner.lock().unwrap();
        let tx = conn.transaction()?;
        let ids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM runs WHERE task_id = ?1 AND status != 'running'",
            )?;
            let rows = stmt.query_map(params![task_id], |r| r.get::<_, i64>(0))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            out
        };
        if ids.is_empty() {
            tx.commit()?;
            return Ok(0);
        }
        let removed = delete_runs_and_children(&tx, &ids)?;
        tx.commit()?;
        Ok(removed)
    }

    /// Retention prune: keep only the most recent `keep` finished runs for a
    /// task, deleting older finished runs (plus their logs, events, and
    /// comments). In-flight runs (`status == 'running'`) are never counted or
    /// touched, so a running job can't be pruned out from under the runner.
    /// `keep == 0` means unlimited and is a no-op. Returns the number of runs
    /// removed. Called after each run finishes when `max_run_history` is set.
    pub fn prune_finished_runs_for_task(&self, task_id: &str, keep: u64) -> Result<u64> {
        if keep == 0 {
            return Ok(0);
        }
        let mut conn = self.inner.lock().unwrap();
        let tx = conn.transaction()?;
        let ids: Vec<i64> = {
            // `LIMIT -1 OFFSET ?` skips the `keep` newest finished runs and
            // returns everything older, which is exactly what we delete.
            let mut stmt = tx.prepare(
                "SELECT id FROM runs
                  WHERE task_id = ?1 AND status != 'running'
                  ORDER BY id DESC LIMIT -1 OFFSET ?2",
            )?;
            let rows = stmt.query_map(params![task_id, keep as i64], |r| r.get::<_, i64>(0))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            out
        };
        if ids.is_empty() {
            tx.commit()?;
            return Ok(0);
        }
        let removed = delete_runs_and_children(&tx, &ids)?;
        tx.commit()?;
        Ok(removed)
    }

    #[allow(dead_code)]
    pub fn get_run(&self, id: i64) -> Result<Option<Run>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, session_id, project_id, started_at, finished_at, status, error,
                    (SELECT MAX(ts) FROM run_logs WHERE run_id = runs.id) AS last_activity_at,
                    prompt
             FROM runs WHERE id = ?1",
        )?;
        let row = stmt.query_row(params![id], row_to_run).optional()?;
        Ok(row)
    }

    // ---------- task memory ----------

    pub fn get_task_memory(&self, task_id: &str) -> Result<Option<TaskMemory>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT task_id, content, updated_at FROM task_memory WHERE task_id = ?1",
        )?;
        let row = stmt
            .query_row(params![task_id], row_to_task_memory)
            .optional()?;
        Ok(row)
    }

    /// Replace a task's memory. An empty (post-trim) string clears it entirely
    /// — deleting the row rather than storing a blank — so `get_task_memory`
    /// reports `None` and the prompt builder shows "(empty)".
    pub fn set_task_memory(&self, task_id: &str, content: &str) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        let trimmed = content.trim();
        if trimmed.is_empty() {
            conn.execute(
                "DELETE FROM task_memory WHERE task_id = ?1",
                params![task_id],
            )?;
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO task_memory (task_id, content, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id) DO UPDATE SET content = ?2, updated_at = ?3",
            params![task_id, trimmed, now],
        )?;
        Ok(())
    }

    /// Atomically append `text` as a new line to a task's memory in a single
    /// statement. Unlike a read-then-`set_task_memory`, this can't lose a
    /// concurrent append from another writer (the orchestrator's manual edit, or
    /// — defensively — another run), since the read-modify-write happens inside
    /// one UPDATE under the write lock. Creates the row with `text` as the whole
    /// content if none exists. Empty/whitespace `text` is a no-op.
    pub fn append_task_memory(&self, task_id: &str, text: &str) -> Result<()> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        // char(10) is a newline; on an existing row this concatenates
        // "<old>\n<new>". A stored row is always non-empty (set_task_memory
        // deletes on empty), so there's never a leading blank line.
        conn.execute(
            "INSERT INTO task_memory (task_id, content, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(task_id) DO UPDATE SET
                 content = content || char(10) || ?2,
                 updated_at = ?3",
            params![task_id, trimmed, now],
        )?;
        Ok(())
    }

    // ---------- run comments ----------

    /// Most recent comments for a task (newest first), capped at `limit`. Used
    /// to inject standing feedback into the next run's prompt.
    pub fn recent_comments_for_task(&self, task_id: &str, limit: i64) -> Result<Vec<RunComment>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, run_id, text, created_at
             FROM run_comments WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![task_id, limit], row_to_comment)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// All comments on a single run, oldest first (chronological) for the UI.
    pub fn list_comments_for_run(&self, run_id: i64) -> Result<Vec<RunComment>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, run_id, text, created_at
             FROM run_comments WHERE run_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![run_id], row_to_comment)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn add_comment(&self, task_id: &str, run_id: i64, text: &str) -> Result<RunComment> {
        let conn = self.inner.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO run_comments (task_id, run_id, text, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![task_id, run_id, text, now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(RunComment {
            id,
            task_id: task_id.to_string(),
            run_id,
            text: text.to_string(),
            created_at: parse_rfc3339(&now),
        })
    }

    pub fn delete_comment(&self, id: i64) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute("DELETE FROM run_comments WHERE id = ?1", params![id])?;
        Ok(())
    }
}

/// Delete the given runs and all of their dependent rows (logs, events,
/// comments) within an open transaction. Shared by the per-task "clear
/// history" action and the retention prune. `task_memory` is intentionally
/// left untouched — it's keyed by task, not run, and outlives any single run.
///
/// rusqlite doesn't bind a `Vec` directly to an `IN` clause, so we expand the
/// placeholders ourselves. Safe because `ids` always come from our own queries,
/// never user input.
fn delete_runs_and_children(tx: &Transaction<'_>, ids: &[i64]) -> Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut placeholders = String::with_capacity(ids.len() * 2);
    for i in 0..ids.len() {
        if i > 0 {
            placeholders.push(',');
        }
        placeholders.push('?');
    }
    tx.execute(
        &format!("DELETE FROM run_logs     WHERE run_id IN ({placeholders})"),
        params_from_iter(ids.iter()),
    )?;
    tx.execute(
        &format!("DELETE FROM run_events   WHERE run_id IN ({placeholders})"),
        params_from_iter(ids.iter()),
    )?;
    tx.execute(
        &format!("DELETE FROM run_comments WHERE run_id IN ({placeholders})"),
        params_from_iter(ids.iter()),
    )?;
    let removed = tx.execute(
        &format!("DELETE FROM runs         WHERE id     IN ({placeholders})"),
        params_from_iter(ids.iter()),
    )?;
    Ok(removed as u64)
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let started_at_s: String = row.get(4)?;
    let finished_at_s: Option<String> = row.get(5)?;
    let last_activity_s: Option<String> = row.get(8)?;
    Ok(Run {
        id: row.get(0)?,
        task_id: row.get(1)?,
        session_id: row.get(2)?,
        project_id: row.get(3)?,
        started_at: parse_rfc3339(&started_at_s),
        finished_at: finished_at_s.as_deref().map(parse_rfc3339),
        status: row.get(6)?,
        error: row.get(7)?,
        last_activity_at: last_activity_s.as_deref().map(parse_rfc3339),
        prompt: row.get(9)?,
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunEvent> {
    let started_at_s: String = row.get(4)?;
    let finished_at_s: Option<String> = row.get(5)?;
    Ok(RunEvent {
        id: row.get(0)?,
        run_id: row.get(1)?,
        name: row.get(2)?,
        status: row.get(3)?,
        started_at: parse_rfc3339(&started_at_s),
        finished_at: finished_at_s.as_deref().map(parse_rfc3339),
        message: row.get(6)?,
    })
}

fn row_to_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunLog> {
    let ts_s: String = row.get(4)?;
    Ok(RunLog {
        id: row.get(0)?,
        run_id: row.get(1)?,
        stream: row.get(2)?,
        line_no: row.get(3)?,
        ts: parse_rfc3339(&ts_s),
        text: row.get(5)?,
    })
}

fn row_to_task_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskMemory> {
    let updated_at_s: String = row.get(2)?;
    Ok(TaskMemory {
        task_id: row.get(0)?,
        content: row.get(1)?,
        updated_at: parse_rfc3339(&updated_at_s),
    })
}

fn row_to_comment(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunComment> {
    let created_at_s: String = row.get(4)?;
    Ok(RunComment {
        id: row.get(0)?,
        task_id: row.get(1)?,
        run_id: row.get(2)?,
        text: row.get(3)?,
        created_at: parse_rfc3339(&created_at_s),
    })
}

fn parse_rfc3339(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
