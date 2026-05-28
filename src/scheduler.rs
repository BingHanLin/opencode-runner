use crate::config::{Schedule, Task};
use crate::db::Db;
use crate::opencode::Cli;
use crate::runner;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_cron_scheduler::{Job, JobScheduler};

/// Wraps a tokio-cron-scheduler with the orchestrator's task list.
/// `repaint` is used to wake the GUI thread when a run completes.
pub struct Scheduler {
    sched: JobScheduler,
    cli: Cli,
    db: Db,
    repaint: Arc<Notify>,
}

impl Scheduler {
    pub async fn new(cli: Cli, db: Db, repaint: Arc<Notify>) -> Result<Self> {
        let sched = JobScheduler::new().await.context("JobScheduler::new")?;
        sched.start().await.context("JobScheduler::start")?;
        Ok(Self {
            sched,
            cli,
            db,
            repaint,
        })
    }

    pub async fn register(&self, task: Task) -> Result<()> {
        if !task.enabled {
            return Ok(());
        }
        let schedule = match task.parse_schedule() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(task = %task.id, "skipping: {e}");
                return Ok(());
            }
        };

        let cli = self.cli.clone();
        let db = self.db.clone();
        let repaint = self.repaint.clone();

        match schedule {
            Schedule::Cron(expr) => {
                let task_id = task.id.clone();
                let task_for_closure = task.clone();
                let job = Job::new_async(expr.as_str(), move |_uuid, _l| {
                    let task = task_for_closure.clone();
                    let cli = cli.clone();
                    let db = db.clone();
                    let repaint = repaint.clone();
                    Box::pin(async move {
                        let _ = runner::execute(&task, &cli, &db).await;
                        repaint.notify_waiters();
                    })
                })
                .with_context(|| format!("creating cron job for {task_id}"))?;
                self.sched.add(job).await?;
            }
            Schedule::Once(when) => {
                let now = chrono::Utc::now();
                if when <= now {
                    tracing::warn!(task = %task.id, "once: time {} already passed; skipping", when);
                    return Ok(());
                }
                let delay = (when - now).to_std().unwrap_or_default();
                let task = task.clone();
                let cli = cli.clone();
                let db = db.clone();
                let repaint = repaint.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = runner::execute(&task, &cli, &db).await;
                    repaint.notify_waiters();
                });
            }
            Schedule::Manual => {
                // nothing to register; UI's "Run now" triggers it
            }
        }
        Ok(())
    }
}
