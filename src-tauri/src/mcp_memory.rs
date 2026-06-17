//! Out-of-process MCP server that exposes a single task's saved memory as a
//! handful of tools the opencode agent can call mid-run.
//!
//! The orchestrator runs this by re-invoking its own binary with the
//! `mcp-memory` subcommand (see `main.rs`), wired into opencode via an
//! ephemeral `OPENCODE_CONFIG_CONTENT` config (see [`opencode_config_content`]).
//! opencode speaks MCP to it over the child's stdin/stdout as newline-delimited
//! JSON-RPC 2.0.
//!
//! Scoping is the security-critical part: the task whose memory may be touched
//! is fixed by the `ORCH_TASK_ID` env var the orchestrator injects at spawn —
//! it is NEVER a tool argument, so an agent cannot read or clobber a different
//! task's memory. The db file is likewise pinned by `ORCH_DB_PATH`.
//!
//! All diagnostics go to stderr; stdout carries only JSON-RPC frames (anything
//! else there would corrupt the protocol).

use crate::db::Db;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

/// MCP server name. opencode exposes each tool to the model prefixed with this,
/// e.g. `orchmem_memory_get`, and keys permission rules off the same prefix.
pub const SERVER_NAME: &str = "orchmem";
/// argv[1] that routes a launch of our binary into [`run_stdio_server`].
pub const SUBCOMMAND: &str = "mcp-memory";
/// Fallback MCP protocol version if the client doesn't state one. We otherwise
/// echo whatever the client requested, which is the most version-robust reply.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

/// Build the ephemeral opencode config (as a JSON string for
/// `OPENCODE_CONFIG_CONTENT`) that registers this binary as a local MCP server
/// scoped to `task_id` against `db_path`. Returns `None` only if we can't
/// resolve our own executable path, in which case the caller should run without
/// memory tools rather than fail the whole task.
pub fn opencode_config_content(task_id: &str, db_path: &Path) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let cfg = json!({
        "mcp": {
            SERVER_NAME: {
                "type": "local",
                "command": [exe.to_string_lossy(), SUBCOMMAND],
                "environment": {
                    "ORCH_TASK_ID": task_id,
                    "ORCH_DB_PATH": db_path.to_string_lossy(),
                },
                "enabled": true
            }
        },
        // Allow exactly our tools so a headless `opencode run` that isn't using
        // --dangerously-skip-permissions doesn't stall on an `ask` prompt. This
        // merges on top of (doesn't replace) the user's own permission config.
        "permission": {
            "orchmem_*": "allow"
        }
    });
    serde_json::to_string(&cfg).ok()
}

/// Entry point for the `mcp-memory` subcommand. Reads `ORCH_TASK_ID` /
/// `ORCH_DB_PATH` from the environment, then serves MCP over stdio until stdin
/// closes. Exits the process non-zero on fatal setup errors (missing env, db
/// won't open) — opencode then simply reports the server unavailable and the
/// run proceeds without memory tools.
pub fn run_stdio_server() {
    let task_id = match std::env::var("ORCH_TASK_ID") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("orchmem: ORCH_TASK_ID not set");
            std::process::exit(1);
        }
    };
    let db_path = match std::env::var("ORCH_DB_PATH") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("orchmem: ORCH_DB_PATH not set");
            std::process::exit(1);
        }
    };
    let db = match Db::open(Path::new(&db_path)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("orchmem: opening db at {db_path} failed: {e:#}");
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
                eprintln!("orchmem: stdin read error: {e}");
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
                eprintln!("orchmem: ignoring unparseable frame: {e}");
                continue;
            }
        };
        if let Some(resp) = handle_message(&db, &task_id, &msg) {
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
fn handle_message(db: &Db, task_id: &str, msg: &Value) -> Option<Value> {
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
        "tools/list" => Some(ok(id, json!({ "tools": tool_defs() }))),
        "tools/call" => Some(handle_tools_call(db, task_id, id, msg)),
        other => Some(err(id, -32601, &format!("method not found: {other}"))),
    }
}

/// The three memory tools, as MCP tool descriptors. Names are unprefixed here;
/// opencode adds the `orchmem_` prefix when exposing them to the model.
fn tool_defs() -> Value {
    json!([
        {
            "name": "memory_get",
            "description": "Read your current saved memory for this task. Returns the stored text, or a note if nothing is saved yet.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "memory_set",
            "description": "Replace your entire saved memory for this task with the given content. Use this to rewrite memory wholesale; pass an empty string to clear it.",
            "inputSchema": {
                "type": "object",
                "properties": { "content": { "type": "string", "description": "The full new memory content." } },
                "required": ["content"],
                "additionalProperties": false
            }
        },
        {
            "name": "memory_append",
            "description": "Append a line or section to your saved memory for this task without rewriting what's already there. Prefer this for incremental notes.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string", "description": "Text to append on a new line." } },
                "required": ["text"],
                "additionalProperties": false
            }
        }
    ])
}

fn handle_tools_call(db: &Db, task_id: &str, id: Value, msg: &Value) -> Value {
    let params = msg.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let args = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    let result: Result<String, String> = match name {
        "memory_get" => mem_get(db, task_id),
        "memory_set" => {
            let content = args.get("content").and_then(Value::as_str).unwrap_or("");
            mem_set(db, task_id, content)
        }
        "memory_append" => {
            let text = args.get("text").and_then(Value::as_str).unwrap_or("");
            mem_append(db, task_id, text)
        }
        other => Err(format!("unknown tool: {other}")),
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
