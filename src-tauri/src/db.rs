use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
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

#[derive(Clone)]
pub struct Db {
    inner: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening db at {}", path.display()))?;
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
            "#,
        )?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
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
            "SELECT id, task_id, session_id, project_id, started_at, finished_at, status, error
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
            "SELECT id, task_id, session_id, project_id, started_at, finished_at, status, error
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

    #[allow(dead_code)]
    pub fn get_run(&self, id: i64) -> Result<Option<Run>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, session_id, project_id, started_at, finished_at, status, error
             FROM runs WHERE id = ?1",
        )?;
        let row = stmt.query_row(params![id], row_to_run).optional()?;
        Ok(row)
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let started_at_s: String = row.get(4)?;
    let finished_at_s: Option<String> = row.get(5)?;
    Ok(Run {
        id: row.get(0)?,
        task_id: row.get(1)?,
        session_id: row.get(2)?,
        project_id: row.get(3)?,
        started_at: parse_rfc3339(&started_at_s),
        finished_at: finished_at_s.as_deref().map(parse_rfc3339),
        status: row.get(6)?,
        error: row.get(7)?,
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

fn parse_rfc3339(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
