//! Long-lived state plumbed into every Tauri command through `tauri::State`.
//!
//! `Scheduler` lives behind a `tokio::sync::Mutex<Option<...>>` because it
//! gets replaced (not mutated) when the user saves new tasks — we shut the
//! old scheduler down cleanly, build a fresh one from the new tasks.toml,
//! and swap it in.

use crate::db::Db;
use crate::runner::CancelRegistry;
use crate::scheduler::Scheduler;
use std::path::PathBuf;
use tokio::sync::Mutex;

pub struct AppState {
    pub db: Db,
    pub registry: CancelRegistry,
    pub scheduler: Mutex<Option<Scheduler>>,
    pub config_path: PathBuf,
}
