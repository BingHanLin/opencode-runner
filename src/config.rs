use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Internal stable identifier. Auto-generated for new tasks and hidden from
    /// the UI; users key tasks by name. Kept in TOML so run history survives
    /// renames.
    #[serde(default = "generate_id")]
    pub id: String,
    pub name: String,
    /// e.g. "cron:0 9 * * 1-5" or "once:2026-05-28T09:00:00Z" or "manual"
    pub schedule: String,
    pub working_dir: PathBuf,
    #[serde(default)]
    pub model: Option<String>,
    pub prompt: String,
    #[serde(default = "default_true")]
    pub dangerously_skip_permissions: bool,
    #[serde(default)]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
pub enum Schedule {
    Cron(String),
    Once(DateTime<Utc>),
    Manual,
}

impl Task {
    pub fn parse_schedule(&self) -> Result<Schedule> {
        let s = self.schedule.trim();
        if s == "manual" {
            return Ok(Schedule::Manual);
        }
        if let Some(expr) = s.strip_prefix("cron:") {
            cron::Schedule::try_from(expr.trim())
                .with_context(|| format!("bad cron expression: {expr}"))?;
            return Ok(Schedule::Cron(expr.trim().to_string()));
        }
        if let Some(when) = s.strip_prefix("once:") {
            let dt = DateTime::parse_from_rfc3339(when.trim())
                .with_context(|| format!("bad RFC3339 timestamp: {when}"))?
                .with_timezone(&Utc);
            return Ok(Schedule::Once(dt));
        }
        Err(anyhow!(
            "schedule must start with 'cron:', 'once:', or be 'manual' (got: {s})"
        ))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TasksFile {
    #[serde(default, rename = "task")]
    pub tasks: Vec<Task>,
}

pub fn tasks_file_path() -> PathBuf {
    PathBuf::from("tasks.toml")
}

pub fn load(path: &Path) -> Result<TasksFile> {
    if !path.exists() {
        return Ok(TasksFile::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let parsed: TasksFile = toml::from_str(&raw)
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(parsed)
}

#[allow(dead_code)]
pub fn save(path: &Path, file: &TasksFile) -> Result<()> {
    let s = toml::to_string_pretty(file)?;
    std::fs::write(path, s).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
