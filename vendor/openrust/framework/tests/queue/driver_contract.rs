//! 通过公开 API 验证自定义队列 driver 的 typed Job 执行契约。

use openrust::{
    BoxFuture, Cancellation, Error, Result,
    queue::{DispatchOptions, Job, JobContext, JobRegistry, Queue, QueueDriver},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

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
    let receiver = Cancellation::new();
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

    let receiver = Cancellation::new();
    receiver.cancel();
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
            JobContext::new("job-3", 1, 1, Cancellation::new())?,
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

static DIRECT_JOB_RESULT: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize, Serialize)]
struct DirectJob;

impl Job for DirectJob {
    const NAME: &'static str = "contract.direct.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            DIRECT_JOB_RESULT.store(42, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[tokio::test]
async fn registry_execute_is_an_explicit_awaited_in_process_path() -> Result<()> {
    DIRECT_JOB_RESULT.store(0, Ordering::SeqCst);
    let mut registry = JobRegistry::default();
    registry.register_job::<DirectJob>()?;

    registry
        .execute(
            DirectJob::NAME,
            Value::Null,
            JobContext::new("direct-1", 1, 1, Cancellation::new())?,
        )
        .await?;

    assert_eq!(DIRECT_JOB_RESULT.load(Ordering::SeqCst), 42);
    Ok(())
}

#[tokio::test]
async fn progress_without_driver_reporter_fails_explicitly() -> Result<()> {
    let context = JobContext::new("job-1", 1, 1, Cancellation::new())?;
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
    assert!(JobContext::new("", 1, 1, Cancellation::new()).is_err());
    assert!(JobContext::new("job", 0, 1, Cancellation::new()).is_err());
    assert!(JobContext::new("job", 2, 1, Cancellation::new()).is_err());
}

struct LegacyDriver;

impl QueueDriver for LegacyDriver {
    fn dispatch<'a>(
        &'a self,
        _queue: &'a str,
        _job_name: &'a str,
        _payload: Value,
        _options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("legacy-job".to_owned()) })
    }

    fn dispatch_in_transaction<'a>(
        &'a self,
        _transaction: &'a openrust::database::DatabaseTransaction,
        _queue: &'a str,
        _job_name: &'a str,
        _payload: Value,
        _options: DispatchOptions,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("legacy-job".to_owned()) })
    }

    fn run_next<'a>(&'a self, _queue: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn retry_failed<'a>(&'a self, _failed_id: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("legacy-retry".to_owned()) })
    }

    fn reset_shutdown(&self) {}

    fn request_shutdown(&self) {}

    fn is_shutdown_requested(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn legacy_custom_driver_rejects_nonempty_execution_metadata() -> Result<()> {
    let config = openrust::config::QueueConfig::default();
    let queue = Queue::with_driver(LegacyDriver, &config)?;
    let scope = openrust::ExecutionContext::new(
        openrust::ExecutionMetadata::default().with_request_id("scope-123")?,
        None,
        Cancellation::new(),
    );
    let error = openrust::with_execution_context(
        scope,
        queue.dispatch("default", "contract.progress.v1", &json!({"value": 1})),
    )
    .await
    .err()
    .ok_or_else(|| Error::custom("test_expected_error", "metadata was silently discarded"))?;
    assert_eq!(error.code(), "queue_driver_metadata_unsupported");
    Ok(())
}

#[tokio::test]
async fn named_connections_parse_and_validate_only_when_selected() -> Result<()> {
    use figment::{Figment, providers::Serialized};
    use openrust::foundation::ProviderBuilder;
    use openrust::queue::QueueServiceProvider;

    let created = Arc::new(AtomicUsize::new(0));
    let validated = Arc::new(AtomicUsize::new(0));
    let provider = QueueServiceProvider::new();
    let creator_count = Arc::clone(&created);
    let validator_count = Arc::clone(&validated);
    provider.contributions().extend_validated(
        "legacy",
        move |_, config| {
            validator_count.fetch_add(1, Ordering::SeqCst);
            if config.options.get("valid").and_then(Value::as_bool) == Some(true) {
                Ok(())
            } else {
                Err(Error::custom("private", "secret-sentinel"))
            }
        },
        move |_, name, config, _, _| {
            assert_eq!(name, "good");
            assert_eq!(config.queue, "emails");
            creator_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Arc::new(LegacyDriver) as Arc<dyn QueueDriver>) })
        },
    )?;
    let mut builder = ProviderBuilder::new();
    builder.provider(provider);
    let runtime = builder
        .start(Figment::from(Serialized::defaults(json!({
            "queue": {"default":"good", "connections":{
                "good":{"driver":"legacy","queue":"emails","options":{"valid":true}},
                "malformed":"secret-sentinel",
                "unknown":{"driver":"unregistered"},
                "redis":{"driver":"redis","redis_url":null},
                "rejected":{"driver":"legacy","options":{"valid":false,"secret":"secret-sentinel"}}
            }}
        }))))
        .await?;
    let queue = runtime.container().resolve::<Queue>().await?;
    assert_eq!(created.load(Ordering::SeqCst), 0);
    assert_eq!(validated.load(Ordering::SeqCst), 0);
    let (first, second) = tokio::join!(queue.connection("good"), queue.connection("good"));
    first?;
    second?;
    assert_eq!(created.load(Ordering::SeqCst), 1);
    assert_eq!(validated.load(Ordering::SeqCst), 1);
    for name in ["malformed", "unknown", "redis", "rejected"] {
        let error = queue.connection(name).await.err().ok_or_else(|| {
            Error::custom(
                "test_expected_error",
                "invalid selected connection was accepted",
            )
        })?;
        assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
    }
    assert_eq!(created.load(Ordering::SeqCst), 1);
    assert_eq!(validated.load(Ordering::SeqCst), 2);
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}
