use crate::config::{Schedule, Task};
use crate::db::Db;
use crate::opencode::Cli;
use crate::runner::{self, CancelRegistry, RunNotifier};
use anyhow::{Context, Result};
use tokio_cron_scheduler::{Job, JobScheduler};

/// Wraps a tokio-cron-scheduler with the orchestrator's task list.
/// `notifier`, if set, is forwarded to each `runner::execute` invocation so
/// scheduled runs surface step events to the Tauri webview the same way
/// "Run now" invocations do.
pub struct Scheduler {
    sched: JobScheduler,
    cli: Cli,
    db: Db,
    registry: CancelRegistry,
    notifier: Option<RunNotifier>,
}

impl Scheduler {
    pub async fn new(
        cli: Cli,
        db: Db,
        registry: CancelRegistry,
        notifier: Option<RunNotifier>,
    ) -> Result<Self> {
        let sched = JobScheduler::new().await.context("JobScheduler::new")?;
        sched.start().await.context("JobScheduler::start")?;
        Ok(Self {
            sched,
            cli,
            db,
            registry,
            notifier,
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
        let registry = self.registry.clone();
        let notifier = self.notifier.clone();

        match schedule {
            Schedule::Cron(expr) => {
                let task_id = task.id.clone();
                let task_for_closure = task.clone();
                let job = Job::new_async(expr.as_str(), move |_uuid, _l| {
                    let task = task_for_closure.clone();
                    let cli = cli.clone();
                    let db = db.clone();
                    let registry = registry.clone();
                    let notifier = notifier.clone();
                    Box::pin(async move {
                        let _ = runner::execute(&task, &cli, &db, &registry, notifier).await;
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
                let registry = registry.clone();
                let notifier = notifier.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = runner::execute(&task, &cli, &db, &registry, notifier).await;
                });
            }
            Schedule::Manual => {
                // nothing to register; UI's "Run now" triggers it
            }
        }
        Ok(())
    }

    pub async fn shutdown(mut self) {
        if let Err(e) = self.sched.shutdown().await {
            tracing::warn!("scheduler shutdown failed: {e}");
        }
    }
}
