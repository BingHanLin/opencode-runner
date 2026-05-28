//! Wraps the `opencode` CLI for one-shot task execution.
//!
//! We shell out to `opencode run --dir X --format json --dangerously-skip-permissions [--model P/M] "<prompt>"`
//! per task. Each invocation creates its own session inside opencode; we capture
//! the session id from the JSON event stream so the orchestrator can later read
//! the conversation from `~/.local/share/opencode/storage/`.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
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
    /// `opencode models` — one `<providerID>/<modelID>` per line.
    pub async fn list_models(&self) -> Result<Vec<Model>> {
        let out = Command::new(&self.binary)
            .arg("models")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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
    /// caller can wire it to a run record.
    pub async fn run_task(
        &self,
        working_dir: &Path,
        prompt: &str,
        model: Option<&str>,
        dangerously_skip_permissions: bool,
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

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning `{} run`", self.binary))?;

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        // Watch stdout for the first `ses_...` token. opencode --format json
        // emits one JSON object per line; rather than parse every event we just
        // grab the first session id we see.
        let session_id_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            let mut found: Option<String> = None;
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "opencode.run.stdout", "{line}");
                if found.is_none() {
                    if let Some(sid) = extract_session_id(&line) {
                        found = Some(sid);
                    }
                }
            }
            found
        });

        // Keep stderr drained (and tail the last ~4KB for diagnostics).
        let stderr_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            let mut tail = String::new();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "opencode.run.stderr", "{line}");
                tail.push_str(&line);
                tail.push('\n');
                if tail.len() > 4096 {
                    let drop_to = tail.len() - 4096;
                    tail.drain(..drop_to);
                }
            }
            tail
        });

        let exit_status = child.wait().await.context("waiting on opencode run")?;
        let session_id = session_id_handle.await.ok().flatten();
        let stderr_tail = stderr_handle.await.unwrap_or_default();

        Ok(RunOutcome {
            session_id,
            exit_status,
            stderr_tail,
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
