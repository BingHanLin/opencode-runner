use crate::config::Task;
use crate::db::Db;
use crate::opencode::Cli;
use anyhow::Result;

/// Execute one task via the `opencode run` CLI; log start/finish in db.
pub async fn execute(task: &Task, cli: &Cli, db: &Db) -> Result<i64> {
    let run_id = db.insert_run_start(&task.id)?;
    tracing::info!(task = %task.id, run_id, "starting task");

    let outcome = cli
        .run_task(
            &task.working_dir,
            &task.prompt,
            task.model.as_deref(),
            task.dangerously_skip_permissions,
        )
        .await;

    match outcome {
        Ok(o) => {
            if let Some(sid) = &o.session_id {
                let _ = db.set_run_session(run_id, sid, None);
            }
            if o.exit_status.success() {
                db.finish_run(run_id, "ok", None)?;
                tracing::info!(task = %task.id, run_id, session = ?o.session_id, "task ok");
            } else {
                let msg = format!(
                    "opencode run exited {:?}\n{}",
                    o.exit_status.code(),
                    o.stderr_tail.trim()
                );
                db.finish_run(run_id, "error", Some(&msg))?;
                tracing::warn!(task = %task.id, run_id, "task failed: {msg}");
            }
            Ok(run_id)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            db.finish_run(run_id, "error", Some(&msg))?;
            tracing::error!(task = %task.id, run_id, "task failed to launch: {msg}");
            Err(e)
        }
    }
}
