//! Wraps the `opencode` CLI for one-shot task execution.
//!
//! We shell out to `opencode run --dir X --format json --dangerously-skip-permissions [--model P/M] "<prompt>"`
//! per task. Each invocation creates its own session inside opencode; we capture
//! the session id from the JSON event stream so the orchestrator can later read
//! the conversation from `~/.local/share/opencode/storage/`.

use crate::runner::CancelToken;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Sink for raw stdout/stderr lines emitted by the opencode child. The first
/// argument is `"stdout"` or `"stderr"`; the second is the captured line
/// (without trailing newline). The sink is held by an `Arc` so both stdout
/// and stderr reader tasks can share the same target.
pub type LogSink = Arc<dyn Fn(&'static str, String) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub provider_id: String,
    pub model_id: String,
}

impl Model {
    pub fn combined(&self) -> String {
        format!("{}/{}", self.provider_id, self.model_id)
    }
}

#[derive(Debug)]
pub struct RunOutcome {
    pub session_id: Option<String>,
    pub exit_status: std::process::ExitStatus,
    pub stderr_tail: String,
    /// True when the run ended because the caller fired `cancel.cancel()` —
    /// the runner uses this to mark the run as `aborted` rather than `error`.
    pub cancelled: bool,
}

#[derive(Clone)]
pub struct Cli {
    pub binary: String,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            binary: "opencode".to_string(),
        }
    }
}

impl Cli {
    /// Construct with an explicit binary path (absolute path recommended to
    /// avoid PATH hijacking). Accepts any `Path`-like value.
    pub fn with_binary(path: impl AsRef<Path>) -> Self {
        Self {
            binary: path.as_ref().to_string_lossy().into_owned(),
        }
    }

    /// Resolve a configured-path setting into a `Cli`. If the configured path
    /// exists, we use it; otherwise we silently fall back to PATH lookup of
    /// the bare `opencode` command so the orchestrator stays functional when
    /// the user's config is stale (binary moved, uninstalled, mis-typed).
    /// Returns the `Cli` and a boolean: `true` if the configured path was
    /// honored, `false` if we fell back.
    pub fn resolve(configured: Option<&Path>) -> (Self, bool) {
        match configured {
            Some(p) if p.exists() => (Self::with_binary(p), true),
            Some(_) | None => (Self::default(), false),
        }
    }
}

impl Cli {
    /// `opencode models` — one `<providerID>/<modelID>` per line.
    pub async fn list_models(&self) -> Result<Vec<Model>> {
        let mut cmd = Command::new(&self.binary);
        cmd.arg("models")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::proc::no_window(&mut cmd);
        let out = cmd
            .output()
            .await
            .with_context(|| format!("running `{} models`", self.binary))?;
        if !out.status.success() {
            anyhow::bail!(
                "`opencode models` failed (exit {:?}): {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut models = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if let Some((p, m)) = line.split_once('/') {
                if !p.is_empty() && !m.is_empty() {
                    models.push(Model {
                        provider_id: p.to_string(),
                        model_id: m.to_string(),
                    });
                }
            }
        }
        Ok(models)
    }

    /// Spawn `opencode run` for one task, returning the outcome once the
    /// process exits. We tail stdout for a session id (`ses_...`) so the
    /// caller can wire it to a run record. When `cancel` fires we kill the
    /// child and surface that in the outcome — callers should treat the
    /// `cancelled` flag distinctly from a non-zero exit.
    pub async fn run_task(
        &self,
        working_dir: &Path,
        prompt: &str,
        model: Option<&str>,
        dangerously_skip_permissions: bool,
        cancel: CancelToken,
        // Fires the first time we spot a `ses_...` token in opencode's stdout,
        // mid-stream, so callers can advertise the session id to the UI long
        // before the child process exits. Fires at most once.
        on_session: Option<Box<dyn FnOnce(String) + Send + 'static>>,
        // Optional stream sink: every captured line (stdout + stderr) is sent
        // here in order, tagged with which stream it came from. Used by the
        // runner to persist and broadcast logs without coupling cli.rs to db.
        log_sink: Option<LogSink>,
    ) -> Result<RunOutcome> {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            anyhow::bail!("prompt is empty — nothing to send to opencode");
        }

        let mut cmd = Command::new(&self.binary);
        cmd.arg("run")
            .arg("--dir")
            .arg(working_dir)
            .arg("--format")
            .arg("json");
        if dangerously_skip_permissions {
            cmd.arg("--dangerously-skip-permissions");
        }
        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }
        // `--` terminates flag parsing so a prompt that starts with `-` is not
        // mistaken for a flag; the prompt itself is one positional argument.
        cmd.arg("--").arg(trimmed);

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::proc::no_window(&mut cmd);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning `{} run`", self.binary))?;

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        // Watch stdout for the first `ses_...` token. opencode --format json
        // emits one JSON object per line; rather than parse every event we just
        // grab the first session id we see and fire `on_session` immediately so
        // the UI can subscribe to the live conversation while the run is still
        // going. The same id is also stashed in RunOutcome at the end. Every
        // line also goes to the optional log sink so the History tab can tail
        // raw CLI output.
        let stdout_sink = log_sink.clone();
        let session_id_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            let mut found: Option<String> = None;
            let mut on_session = on_session;
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "opencode.run.stdout", "{line}");
                if let Some(sink) = &stdout_sink {
                    sink("stdout", line.clone());
                }
                if found.is_none() {
                    if let Some(sid) = extract_session_id(&line) {
                        if let Some(cb) = on_session.take() {
                            cb(sid.clone());
                        }
                        found = Some(sid);
                    }
                }
            }
            found
        });

        // Keep stderr drained (and tail the last ~4KB for diagnostics).
        let stderr_sink = log_sink;
        let stderr_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            let mut tail = String::new();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "opencode.run.stderr", "{line}");
                if let Some(sink) = &stderr_sink {
                    sink("stderr", line.clone());
                }
                tail.push_str(&line);
                tail.push('\n');
                if tail.len() > 4096 {
                    let drop_to = tail.len() - 4096;
                    tail.drain(..drop_to);
                }
            }
            tail
        });

        // Race the child against the cancel token. If cancelled, fire a kill
        // and still wait for the process to actually exit so `wait()` reaps
        // it — leaving a zombie behind would confuse downstream cleanup.
        let mut cancelled = false;
        let exit_status = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                cancelled = true;
                let _ = child.start_kill();
                child.wait().await.context("waiting on opencode run after cancel")?
            }
            status = child.wait() => status.context("waiting on opencode run")?,
        };
        let session_id = session_id_handle.await.ok().flatten();
        let stderr_tail = stderr_handle.await.unwrap_or_default();

        Ok(RunOutcome {
            session_id,
            exit_status,
            stderr_tail,
            cancelled,
        })
    }
}

/// Find the first `ses_…` looking token in a line. opencode session ids match
/// `^ses` in the schema; we accept ses_ or ses- followed by url-safe chars.
fn extract_session_id(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if &bytes[i..i + 3] == b"ses" {
            let after = &bytes[i + 3..];
            if let Some(&c) = after.first() {
                if c == b'_' || c == b'-' {
                    let mut j = i + 4;
                    while j < bytes.len() {
                        let b = bytes[j];
                        if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if j - i > 8 {
                        return Some(line[i..j].to_string());
                    }
                }
            }
        }
        i += 1;
    }
    None
}
