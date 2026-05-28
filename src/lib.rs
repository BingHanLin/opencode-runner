//! Shared backend used by both the egui and Iced binaries.
//!
//! Anything that is not tied to a specific GUI framework lives here. The two
//! frontends (`src/main.rs` + `src/ui/` for egui, `src/bin/iced_preview.rs`
//! for Iced) consume these modules through `opencode_orchestrator::*`.

pub mod config;
pub mod db;
pub mod opencode;
pub mod runner;
pub mod scheduler;
