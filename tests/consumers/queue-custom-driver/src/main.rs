//! 展示独立 crate 如何实现 QueueDriver 并复用 typed Job 执行器。

use openrust::{
    BoxFuture, Error, Result,
    config::QueueConfig,
    db::DatabaseTransaction,
    queue::{DispatchOptions, Job, JobContext, JobRegistry, Queue, QueueDriver},
};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::VecDeque, sync::Mutex};

#[derive(Deserialize)]
struct Greeting {
    name: String,
}

impl Job for Greeting {
    const NAME: &'static str = "example.greeting.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            context
                .set_progress(&serde_json::json!({"greeted": self.name}))
                .await
        })
    }
}

struct PendingJob {
    name: String,
    payload: Value,
}

struct MemoryQueue {
    jobs: Mutex<VecDeque<PendingJob>>,
    registry: JobRegistry,
}

impl QueueDriver for MemoryQueue {
    fn dispatch<'a>(
        &'a self,
        _queue: &'a str,
        job_name: &'a str,
        payload: Value,
        _options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.jobs
                .lock()
                .map_err(|_| Error::custom("queue_lock_poisoned", "queue lock poisoned"))?
                .push_back(PendingJob {
                    name: job_name.to_owned(),
                    payload,
                });
            Ok("memory-1".to_owned())
        })
    }

    fn dispatch_in_transaction<'a>(
        &'a self,
        _transaction: &'a DatabaseTransaction,
        _queue: &'a str,
        _job_name: &'a str,
        _payload: Value,
        _options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async {
            Err(Error::custom(
                "queue_transaction_unsupported",
                "memory driver has no database transaction",
            ))
        })
    }

    fn run_next<'a>(&'a self, _queue: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let Some(job) = self
                .jobs
                .lock()
                .map_err(|_| Error::custom("queue_lock_poisoned", "queue lock poisoned"))?
                .pop_front()
            else {
                return Ok(false);
            };
            let cancelled = openrust::Cancellation::new();
            let context = JobContext::new("memory-1", 1, 1, cancelled)?
                .with_progress_reporter(|_value| Box::pin(async { Ok(()) }));
            self.registry
                .execute(&job.name, job.payload, context)
                .await?;
            Ok(true)
        })
    }

    fn retry_failed<'a>(&'a self, _failed_id: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async {
            Err(Error::custom(
                "queue_retry_unsupported",
                "memory driver does not retain failed jobs",
            ))
        })
    }
    fn reset_shutdown(&self) {}
    fn request_shutdown(&self) {}
    fn is_shutdown_requested(&self) -> bool {
        false
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut registry = JobRegistry::default();
    registry.register_job::<Greeting>()?;
    let driver = MemoryQueue {
        jobs: Mutex::new(VecDeque::new()),
        registry,
    };
    let queue = Queue::with_driver(driver, &QueueConfig::default())?;
    queue
        .dispatch(
            "default",
            Greeting::NAME,
            &serde_json::json!({"name": "OpenRust"}),
        )
        .await?;
    if !queue.run_next("default").await? {
        return Err(Error::custom(
            "queue_job_missing",
            "dispatched job was not available",
        ));
    }
    Ok(())
}
