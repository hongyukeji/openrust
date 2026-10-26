//! 通过公开 API 验证自定义队列 driver 的 typed Job 执行契约。

use openrust::{
    BoxFuture, Error, Result,
    queue::{Job, JobContext, JobRegistry},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Deserialize, Serialize)]
struct ProgressJob {
    value: u64,
}

impl Job for ProgressJob {
    const NAME: &'static str = "contract.progress.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { context.set_progress(&json!({"value": self.value})).await })
    }
}

#[derive(Deserialize, Serialize)]
struct CancelledJob;

impl Job for CancelledJob {
    const NAME: &'static str = "contract.cancelled.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if context.is_cancelled() {
                Err(Error::custom("cancelled_by_driver", "cancelled"))
            } else {
                Ok(())
            }
        })
    }
}

#[tokio::test]
async fn public_registry_executes_typed_jobs_with_custom_context_behavior() -> Result<()> {
    let mut registry = JobRegistry::default();
    registry.register_job::<ProgressJob>()?;
    assert!(registry.register_job::<ProgressJob>().is_err());
    registry.register_job::<CancelledJob>()?;

    let progress = Arc::new(Mutex::new(None::<Value>));
    let progress_writer = Arc::clone(&progress);
    let (_cancel, receiver) = tokio::sync::watch::channel(false);
    let context = JobContext::new("job-1", 1, 3, receiver)?.with_progress_reporter(move |value| {
        let progress_writer = Arc::clone(&progress_writer);
        Box::pin(async move {
            *progress_writer
                .lock()
                .map_err(|_| Error::custom("poisoned_test_lock", "test lock poisoned"))? =
                Some(value);
            Ok(())
        })
    });
    registry
        .execute(ProgressJob::NAME, json!({"value": 7}), context)
        .await?;
    assert_eq!(
        progress
            .lock()
            .map_err(|_| Error::custom("poisoned_test_lock", "test lock poisoned"))?
            .as_ref(),
        Some(&json!({"value": 7}))
    );

    let (_cancel, receiver) = tokio::sync::watch::channel(true);
    let error = registry
        .execute(
            CancelledJob::NAME,
            Value::Null,
            JobContext::new("job-2", 1, 1, receiver)?,
        )
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "test_expected_error",
                "cancelled handler unexpectedly succeeded",
            )
        })?;
    assert_eq!(error.code(), "cancelled_by_driver");

    let error = registry
        .execute(
            "contract.missing.v1",
            json!({}),
            JobContext::new("job-3", 1, 1, tokio::sync::watch::channel(false).1)?,
        )
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "test_expected_error",
                "unregistered job unexpectedly succeeded",
            )
        })?;
    assert_eq!(error.code(), "unregistered_queue_job");
    Ok(())
}

#[tokio::test]
async fn progress_without_driver_reporter_fails_explicitly() -> Result<()> {
    let context = JobContext::new("job-1", 1, 1, tokio::sync::watch::channel(false).1)?;
    let error = context
        .set_progress(&json!({"step": 1}))
        .await
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "progress unexpectedly succeeded"))?;
    assert_eq!(error.code(), "queue_progress_unsupported");
    Ok(())
}

#[test]
fn public_context_rejects_invalid_attempt_metadata() {
    assert!(JobContext::new("", 1, 1, tokio::sync::watch::channel(false).1).is_err());
    assert!(JobContext::new("job", 0, 1, tokio::sync::watch::channel(false).1).is_err());
    assert!(JobContext::new("job", 2, 1, tokio::sync::watch::channel(false).1).is_err());
}
