use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    auth: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendOptions<'a> {
    pub working_dir: &'a Path,
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub dangerously_skip_permissions: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendResult {
    pub session_id: String,
    pub project_id: Option<String>,
}

impl Client {
    pub fn new(base_url: impl Into<String>, auth: Option<(String, String)>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60 * 30))
                .build()
                .expect("reqwest client"),
            base_url: base_url.into(),
            auth,
        }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let mut b = self.http.request(method, url);
        if let Some((u, p)) = &self.auth {
            b = b.basic_auth(u, Some(p));
        }
        b
    }

    /// Create a new session and send the prompt as a single user message.
    /// Returns the session id (and project id if the server reports it).
    pub async fn run_once(&self, opts: SendOptions<'_>) -> Result<SendResult> {
        // 1. create session
        let create_body = serde_json::json!({
            "title": truncate(opts.prompt, 60),
            "directory": opts.working_dir.to_string_lossy(),
        });
        let session: serde_json::Value = self
            .req(reqwest::Method::POST, "/session")
            .json(&create_body)
            .send()
            .await
            .context("POST /session")?
            .error_for_status()
            .context("POST /session — server returned error")?
            .json()
            .await
            .context("decoding POST /session response")?;

        let session_id = session
            .get("id")
            .and_then(|v| v.as_str())
            .context("POST /session response missing `id`")?
            .to_string();
        let project_id = session
            .get("projectID")
            .or_else(|| session.get("project_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        // 2. send message
        let mut msg_body = serde_json::json!({
            "parts": [{ "type": "text", "text": opts.prompt }],
        });
        if let Some(m) = opts.model {
            // model is "provider/model" — opencode expects providerID + modelID
            if let Some((p, m)) = m.split_once('/') {
                msg_body["providerID"] = serde_json::Value::String(p.to_string());
                msg_body["modelID"] = serde_json::Value::String(m.to_string());
            }
        }
        if opts.dangerously_skip_permissions {
            msg_body["dangerouslySkipPermissions"] = serde_json::Value::Bool(true);
        }

        let path = format!("/session/{session_id}/message");
        let _ = self
            .req(reqwest::Method::POST, &path)
            .json(&msg_body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))?
            .error_for_status()
            .with_context(|| format!("POST {path} — server returned error"))?;

        Ok(SendResult {
            session_id,
            project_id,
        })
    }

    #[allow(dead_code)]
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let v: serde_json::Value = self
            .req(reqwest::Method::GET, "/session")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let arr = v.as_array().cloned().unwrap_or_default();
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            if let Ok(s) = serde_json::from_value::<SessionInfo>(item) {
                out.push(s);
            }
        }
        Ok(out)
    }
}

fn truncate(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}
