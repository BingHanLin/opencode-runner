//! Out-of-process MCP server that exposes a single run's/task's state as a
//! handful of tools the opencode agent can call mid-run:
//!   - `summary_*` — read/write THIS run's summary (what the agent did). Wired
//!     for every run.
//!   - `memory_*` — read/update THIS task's accumulated memory. Exposed only
//!     when the task opts into memory.
//!
//! The app runs this by re-invoking its own binary with the
//! `mcp-memory` subcommand (see `main.rs`), wired into opencode via an
//! ephemeral `OPENCODE_CONFIG_CONTENT` config (see [`opencode_config_content`]).
//! opencode speaks MCP to it over the child's stdin/stdout as newline-delimited
//! JSON-RPC 2.0.
//!
//! Scoping is the security-critical part: the task whose memory may be touched
//! is fixed by the `RUNNER_TASK_ID` env var and the run whose summary may be
//! touched by `RUNNER_RUN_ID` — both injected by the app at spawn and
//! NEVER tool arguments, so an agent cannot read or clobber another task's
//! memory or run's summary. The db file is likewise pinned by `RUNNER_DB_PATH`,
//! and `RUNNER_MEMORY_ENABLED` gates whether the memory tools are offered at all.
//!
//! All diagnostics go to stderr; stdout carries only JSON-RPC frames (anything
//! else there would corrupt the protocol).

use crate::db::Db;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

/// MCP server name. opencode exposes each tool to the model prefixed with this,
/// e.g. `runmem_memory_get`, and keys permission rules off the same prefix.
pub const SERVER_NAME: &str = "runmem";
/// argv[1] that routes a launch of our binary into [`run_stdio_server`].
pub const SUBCOMMAND: &str = "mcp-memory";
/// Fallback MCP protocol version if the client doesn't state one. We otherwise
/// echo whatever the client requested, which is the most version-robust reply.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

/// Build the ephemeral opencode config (as a JSON string for
/// `OPENCODE_CONFIG_CONTENT`) that registers this binary as a local MCP server
/// scoped to `run_id` (for summary) and `task_id` (for memory) against
/// `db_path`. `memory_enabled` controls whether the memory tools are offered;
/// the summary tools are always offered. Returns `None` only if we can't
/// resolve our own executable path, in which case the caller should run without
/// these tools rather than fail the whole task.
pub fn opencode_config_content(
    task_id: &str,
    run_id: i64,
    db_path: &Path,
    memory_enabled: bool,
) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let cfg = json!({
        "mcp": {
            SERVER_NAME: {
                "type": "local",
                "command": [exe.to_string_lossy(), SUBCOMMAND],
                "environment": {
                    "RUNNER_TASK_ID": task_id,
                    "RUNNER_RUN_ID": run_id.to_string(),
                    "RUNNER_DB_PATH": db_path.to_string_lossy(),
                    "RUNNER_MEMORY_ENABLED": if memory_enabled { "1" } else { "0" },
                },
                "enabled": true
            }
        },
        // Allow exactly our tools so a headless `opencode run` that isn't using
        // --dangerously-skip-permissions doesn't stall on an `ask` prompt. This
        // merges on top of (doesn't replace) the user's own permission config.
        "permission": {
            "runmem_*": "allow"
        }
    });
    serde_json::to_string(&cfg).ok()
}

/// Per-process scope, fixed by env at spawn — never influenced by tool args.
struct Scope {
    task_id: String,
    /// The run whose summary the `summary_*` tools touch. `None` only in the
    /// degenerate case where `RUNNER_RUN_ID` was missing/unparseable, which
    /// disables the summary tools rather than touching the wrong run.
    run_id: Option<i64>,
    /// Whether the task opted into memory; gates the `memory_*` tools.
    memory_enabled: bool,
}

/// Entry point for the `mcp-memory` subcommand. Reads `RUNNER_TASK_ID` /
/// `RUNNER_RUN_ID` / `RUNNER_DB_PATH` / `RUNNER_MEMORY_ENABLED` from the environment,
/// then serves MCP over stdio until stdin closes. Exits the process non-zero on
/// fatal setup errors (missing task id, db won't open) — opencode then simply
/// reports the server unavailable and the run proceeds without these tools.
pub fn run_stdio_server() {
    let task_id = match std::env::var("RUNNER_TASK_ID") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("runmem: RUNNER_TASK_ID not set");
            std::process::exit(1);
        }
    };
    let db_path = match std::env::var("RUNNER_DB_PATH") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("runmem: RUNNER_DB_PATH not set");
            std::process::exit(1);
        }
    };
    let run_id: Option<i64> = std::env::var("RUNNER_RUN_ID")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok());
    let memory_enabled = matches!(
        std::env::var("RUNNER_MEMORY_ENABLED").ok().as_deref(),
        Some("1") | Some("true")
    );
    let scope = Scope {
        task_id,
        run_id,
        memory_enabled,
    };
    let db = match Db::open(Path::new(&db_path)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("runmem: opening db at {db_path} failed: {e:#}");
            std::process::exit(1);
        }
    };

    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF: opencode closed the pipe, we're done.
            Ok(_) => {}
            Err(e) => {
                eprintln!("runmem: stdin read error: {e}");
                break;
            }
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("runmem: ignoring unparseable frame: {e}");
                continue;
            }
        };
        if let Some(resp) = handle_message(&db, &scope, &msg) {
            if serde_json::to_writer(&mut out, &resp).is_err() {
                break;
            }
            if out.write_all(b"\n").is_err() || out.flush().is_err() {
                break;
            }
        }
    }
}

/// Dispatch one JSON-RPC message. Returns the response to send, or `None` for
/// notifications (id-less messages, e.g. `notifications/initialized`), which
/// the spec says must not be answered.
fn handle_message(db: &Db, scope: &Scope, msg: &Value) -> Option<Value> {
    // A request carries a non-null id; anything else is a notification.
    let id = match msg.get("id") {
        Some(v) if !v.is_null() => v.clone(),
        _ => return None,
    };
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "initialize" => {
            let pv = msg
                .get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL_VERSION)
                .to_string();
            Some(ok(
                id,
                json!({
                    "protocolVersion": pv,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") }
                }),
            ))
        }
        "ping" => Some(ok(id, json!({}))),
        "tools/list" => Some(ok(id, json!({ "tools": tool_defs(scope) }))),
        "tools/call" => Some(handle_tools_call(db, scope, id, msg)),
        other => Some(err(id, -32601, &format!("method not found: {other}"))),
    }
}

/// The tool descriptors offered for this scope: summary tools whenever a run id
/// is known, memory tools only when the task opted into memory. Names are
/// unprefixed here; opencode adds the `runmem_` prefix when exposing them.
fn tool_defs(scope: &Scope) -> Value {
    let mut tools = Vec::new();
    if scope.run_id.is_some() {
        tools.push(json!({
            "name": "summary_get",
            "description": "Read the summary recorded so far for THIS run. Returns the stored text, or a note if nothing is written yet.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }));
        tools.push(json!({
            "name": "summary_set",
            "description": "Record (or replace) a concise summary of THIS run — what you did and how it turned out. Call this before you finish. Pass an empty string to clear it.",
            "inputSchema": {
                "type": "object",
                "properties": { "content": { "type": "string", "description": "The full run summary." } },
                "required": ["content"],
                "additionalProperties": false
            }
        }));
        tools.push(json!({
            "name": "summary_append",
            "description": "Append a note to THIS run's summary without rewriting what's already there. Useful for jotting progress during a long run.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string", "description": "Text to append on a new line." } },
                "required": ["text"],
                "additionalProperties": false
            }
        }));
    }
    if scope.memory_enabled {
        tools.push(json!({
            "name": "memory_get",
            "description": "Read your current saved memory for this task. Returns the stored text, or a note if nothing is saved yet.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }));
        tools.push(json!({
            "name": "memory_set",
            "description": "Replace your entire saved memory for this task with the given content. Use this to rewrite memory wholesale; pass an empty string to clear it.",
            "inputSchema": {
                "type": "object",
                "properties": { "content": { "type": "string", "description": "The full new memory content." } },
                "required": ["content"],
                "additionalProperties": false
            }
        }));
        tools.push(json!({
            "name": "memory_append",
            "description": "Append a line or section to your saved memory for this task without rewriting what's already there. Prefer this for incremental notes.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string", "description": "Text to append on a new line." } },
                "required": ["text"],
                "additionalProperties": false
            }
        }));
    }
    Value::Array(tools)
}

fn handle_tools_call(db: &Db, scope: &Scope, id: Value, msg: &Value) -> Value {
    let params = msg.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let args = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Gate each tool on its scope flag so a disabled tool can't be invoked even
    // if the model somehow names it — matching what `tool_defs` advertised.
    let result: Result<String, String> = match name {
        "summary_get" if scope.run_id.is_some() => sum_get(db, scope.run_id),
        "summary_set" if scope.run_id.is_some() => {
            let content = args.get("content").and_then(Value::as_str).unwrap_or("");
            sum_set(db, scope.run_id, content)
        }
        "summary_append" if scope.run_id.is_some() => {
            let text = args.get("text").and_then(Value::as_str).unwrap_or("");
            sum_append(db, scope.run_id, text)
        }
        "memory_get" if scope.memory_enabled => mem_get(db, &scope.task_id),
        "memory_set" if scope.memory_enabled => {
            let content = args.get("content").and_then(Value::as_str).unwrap_or("");
            mem_set(db, &scope.task_id, content)
        }
        "memory_append" if scope.memory_enabled => {
            let text = args.get("text").and_then(Value::as_str).unwrap_or("");
            mem_append(db, &scope.task_id, text)
        }
        other => Err(format!("unknown or unavailable tool: {other}")),
    };

    // Per MCP, tool failures are a normal result with isError:true (so the
    // model sees the message), not a transport-level JSON-RPC error.
    match result {
        Ok(text) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
        ),
        Err(e) => ok(
            id,
            json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
        ),
    }
}

fn sum_get(db: &Db, run_id: Option<i64>) -> Result<String, String> {
    let run_id = run_id.ok_or_else(|| "no run id in scope".to_string())?;
    match db.get_run_summary(run_id).map_err(|e| e.to_string())? {
        Some(s) => Ok(s),
        None => Ok("(no summary written yet)".to_string()),
    }
}

fn sum_set(db: &Db, run_id: Option<i64>, content: &str) -> Result<String, String> {
    let run_id = run_id.ok_or_else(|| "no run id in scope".to_string())?;
    db.set_run_summary(run_id, content)
        .map_err(|e| e.to_string())?;
    let n = content.trim().len();
    if n == 0 {
        Ok("Summary cleared.".to_string())
    } else {
        Ok(format!("Summary saved ({n} chars)."))
    }
}

fn sum_append(db: &Db, run_id: Option<i64>, text: &str) -> Result<String, String> {
    let run_id = run_id.ok_or_else(|| "no run id in scope".to_string())?;
    if text.trim().is_empty() {
        return Err("nothing to append: text is empty".to_string());
    }
    db.append_run_summary(run_id, text)
        .map_err(|e| e.to_string())?;
    let total = db
        .get_run_summary(run_id)
        .map_err(|e| e.to_string())?
        .map(|s| s.len())
        .unwrap_or(0);
    Ok(format!("Appended ({total} chars total)."))
}

fn mem_get(db: &Db, task_id: &str) -> Result<String, String> {
    match db.get_task_memory(task_id).map_err(|e| e.to_string())? {
        Some(m) if !m.content.trim().is_empty() => Ok(m.content),
        _ => Ok("(no memory saved yet)".to_string()),
    }
}

fn mem_set(db: &Db, task_id: &str, content: &str) -> Result<String, String> {
    db.set_task_memory(task_id, content)
        .map_err(|e| e.to_string())?;
    let n = content.trim().len();
    if n == 0 {
        Ok("Memory cleared.".to_string())
    } else {
        Ok(format!("Memory saved ({n} chars)."))
    }
}

fn mem_append(db: &Db, task_id: &str, text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("nothing to append: text is empty".to_string());
    }
    db.append_task_memory(task_id, text)
        .map_err(|e| e.to_string())?;
    // Re-read only to report the new size; a slightly stale count here is
    // harmless (the append itself was atomic).
    let total = db
        .get_task_memory(task_id)
        .map_err(|e| e.to_string())?
        .map(|m| m.content.len())
        .unwrap_or(0);
    Ok(format!("Appended ({total} chars total)."))
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
