use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
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

fn parse_rfc3339(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
