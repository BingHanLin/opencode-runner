//! Reads opencode's on-disk session/message data so the UI can show conversations
//! without going through the HTTP API.
//!
//! As of opencode's SQLite migration, all session/message/part data lives in
//! a single SQLite database at `$OPENCODE_DATA_DIR/opencode.db` (default
//! `~/.local/share/opencode/opencode.db`). Each row carries a JSON blob in
//! `data`; the columns we care about (`id`, `session_id`, `message_id`) are
//! still real columns, and the rest comes out of the JSON.
//!
//! We open the DB *read-only* (`?mode=ro`). Crucially we do NOT pass
//! `immutable=1`: that flag makes SQLite ignore the `-wal` file entirely and
//! read only the main database file, so any rows the live `opencode` server has
//! written but not yet checkpointed (which, with a long-lived server, is every
//! recent message) are invisible. That silently broke memory extraction — the
//! agent's final reply, still WAL-resident at read time, was never seen. A
//! plain read-only connection reads the WAL via the writer's existing `-shm`.
//! If that open fails (e.g. opencode isn't running and left an orphaned `-wal`
//! with no `-shm` to attach to), we fall back to `immutable=1` — a stale view
//! is better than no view.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Ok(env) = std::env::var("OPENCODE_DATA_DIR") {
        let first = env.split(',').next().unwrap_or("").trim();
        if !first.is_empty() {
            return PathBuf::from(first);
        }
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("share").join("opencode");
    }
    PathBuf::from(".")
}

pub fn db_path() -> PathBuf {
    data_dir().join("opencode.db")
}

/// A message row as the UI consumes it. `role` and everything else lives in
/// the row's JSON `data` blob; `id` and `session_id` come from real columns.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub role: Option<String>,
    pub session_id: Option<String>,
    pub created_at: Option<i64>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Part {
    pub id: String,
    pub message_id: Option<String>,
    pub kind: Option<String>,
    pub text: Option<String>,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MessageData {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    time: Option<TimeBlock>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct PartData {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct TimeBlock {
    #[serde(default)]
    created: Option<i64>,
}

fn open_ro() -> Result<Connection> {
    let p = db_path();
    if !p.exists() {
        anyhow::bail!(
            "opencode.db not found at {} — has opencode run yet on this machine?",
            p.display()
        );
    }
    // Read-only so we can never modify opencode's data. We deliberately avoid
    // `immutable=1` so the connection reads the live `-wal` (via the writer's
    // `-shm`) and sees recently-written rows. Only if that fails — e.g. an
    // orphaned `-wal` with no `-shm` to attach to while opencode is down — do
    // we retry with `immutable=1`, which ignores the WAL but at least opens.
    let path_str = p.to_string_lossy().replace('\\', "/");
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let uri = format!("file:{}?mode=ro", path_str);
    match Connection::open_with_flags(&uri, flags) {
        Ok(conn) => Ok(conn),
        Err(e) => {
            tracing::debug!(
                "read-only open of {} failed ({e}); retrying with immutable=1 (stale WAL)",
                p.display()
            );
            let uri = format!("file:{}?mode=ro&immutable=1", path_str);
            Connection::open_with_flags(&uri, flags)
                .with_context(|| format!("opening {}", p.display()))
        }
    }
}

pub fn list_messages(session_id: &str) -> Result<Vec<Message>> {
    let conn = open_ro()?;
    let mut stmt = conn.prepare(
        "SELECT id, session_id, time_created, data
         FROM message
         WHERE session_id = ?1
         ORDER BY time_created ASC, id ASC",
    )?;
    let rows = stmt.query_map([session_id], |row| {
        let id: String = row.get(0)?;
        let sid: Option<String> = row.get(1).ok();
        let created: Option<i64> = row.get(2).ok();
        let data: String = row.get(3)?;
        Ok((id, sid, created, data))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id, sid, created, data) = r?;
        let parsed: MessageData = serde_json::from_str(&data).unwrap_or(MessageData {
            role: None,
            time: None,
            extra: serde_json::Map::new(),
        });
        out.push(Message {
            id,
            role: parsed.role,
            session_id: sid,
            created_at: created.or_else(|| parsed.time.and_then(|t| t.created)),
            extra: parsed.extra,
        });
    }
    Ok(out)
}

pub fn list_parts(message_id: &str) -> Result<Vec<Part>> {
    let conn = open_ro()?;
    let mut stmt = conn.prepare(
        "SELECT id, message_id, data
         FROM part
         WHERE message_id = ?1
         ORDER BY time_created ASC, id ASC",
    )?;
    let rows = stmt.query_map([message_id], |row| {
        let id: String = row.get(0)?;
        let mid: Option<String> = row.get(1).ok();
        let data: String = row.get(2)?;
        Ok((id, mid, data))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id, mid, data) = r?;
        let parsed: PartData = serde_json::from_str(&data).unwrap_or(PartData {
            kind: None,
            text: None,
            extra: serde_json::Map::new(),
        });
        out.push(Part {
            id,
            message_id: mid,
            kind: parsed.kind,
            text: parsed.text,
            extra: parsed.extra,
        });
    }
    Ok(out)
}

/// Load a session's full conversation as (message, parts) pairs in chronological order.
pub fn load_conversation(session_id: &str) -> Result<Vec<(Message, Vec<Part>)>> {
    let messages = list_messages(session_id)?;
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        let parts = list_parts(&m.id).unwrap_or_default();
        out.push((m, parts));
    }
    Ok(out)
}
