//! 验证真实 Redis 上的共享领取、唯一任务、重试与进程崩溃恢复。

use openrust::{
    BoxFuture, Error, Result,
    config::QueueConfig,
    queue::{
        DispatchOptions, GroupFailurePolicy, Job, JobContext, JobRegistry, Queue, QueueTask,
        RedisQueueDriver,
    },
};
use redis::AsyncCommands as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static RUNS: AtomicUsize = AtomicUsize::new(0);
static RETRY_RUNS: AtomicUsize = AtomicUsize::new(0);
static CRASH_RUNS: AtomicUsize = AtomicUsize::new(0);

#[derive(Deserialize, Serialize)]
struct CountJob;

impl Job for CountJob {
    const NAME: &'static str = "tests.redis.count.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            RUNS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[derive(Deserialize, Serialize)]
struct RetryJob;

impl Job for RetryJob {
    const NAME: &'static str = "tests.redis.retry.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            context
                .set_progress(&json!({"attempt": context.attempt()}))
                .await?;
            if RETRY_RUNS.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(Error::custom("expected_retry", "retry test"))
            } else {
                Ok(())
            }
        })
    }
}

#[derive(Deserialize, Serialize)]
struct CrashJob;

impl Job for CrashJob {
    const NAME: &'static str = "tests.redis.crash.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            context.set_progress(&json!({"claimed": true})).await?;
            if std::env::var_os("OPENRUST_REDIS_CRASH_CHILD").is_some() {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
            }
            CRASH_RUNS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_owned())
}

fn namespace(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{label}-{}", nanos % 1_000_000_000_000_000)
}

fn registry() -> Result<JobRegistry> {
    let mut registry = JobRegistry::default();
    registry.register_job::<CountJob>()?;
    registry.register_job::<RetryJob>()?;
    registry.register_job::<CrashJob>()?;
    Ok(registry)
}

fn config() -> QueueConfig {
    QueueConfig {
        lease_secs: 1,
        job_timeout_secs: 10,
        shutdown_timeout_secs: 1,
        ..QueueConfig::default()
    }
}

#[tokio::test]
async fn redis_queue_fails_closed_for_bad_namespace_or_unavailable_service() -> Result<()> {
    assert!(
        RedisQueueDriver::new(
            "redis://127.0.0.1:6379/",
            "../unsafe",
            registry()?,
            config()
        )
        .is_err()
    );
    let driver = RedisQueueDriver::new("redis://127.0.0.1:1/", "offline", registry()?, config())?;
    let queue = Queue::with_driver(driver, &config())?;
    let error = queue
        .dispatch("default", CountJob::NAME, &())
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "test_expected_error",
                "unavailable Redis unexpectedly accepted a task",
            )
        })?;
    assert_eq!(error.code(), "queue_redis_unavailable");
    Ok(())
}

fn queue(namespace: &str) -> Result<Queue> {
    Queue::with_driver(
        RedisQueueDriver::new(&redis_url(), namespace, registry()?, config())?,
        &config(),
    )
}

async fn clean_namespace(namespace: &str) -> Result<()> {
    let client = redis::Client::open(redis_url())
        .map_err(|_| Error::custom("test_redis_url_invalid", "Redis test URL is invalid"))?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(Error::internal)?;
    let pattern = format!("{{openrust:{namespace}}}:*");
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(pattern)
        .query_async(&mut connection)
        .await
        .map_err(Error::internal)?;
    if !keys.is_empty() {
        let _: usize = connection.del(keys).await.map_err(Error::internal)?;
    }
    Ok(())
}

#[tokio::test]
async fn redis_queue_shares_atomic_claims_uniqueness_progress_and_retry() -> Result<()> {
    RUNS.store(0, Ordering::SeqCst);
    RETRY_RUNS.store(0, Ordering::SeqCst);
    let namespace = namespace("contract");
    let first = queue(&namespace)?;
    let second = queue(&namespace)?;
    let group_error = first
        .dispatch_chain(
            "default",
            vec![QueueTask::new(CountJob::NAME, &())?],
            GroupFailurePolicy::Stop,
        )
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "test_expected_error",
                "Redis task group unexpectedly dispatched",
            )
        })?;
    assert_eq!(group_error.code(), "queue_driver_group_unsupported");
    let unique_a = first
        .dispatch_unique("default", CountJob::NAME, "same", &())
        .await?;
    let unique_b = second
        .dispatch_unique("default", CountJob::NAME, "same", &())
        .await?;
    assert_eq!(unique_a, unique_b);
    for _ in 0..40 {
        first.dispatch("default", CountJob::NAME, &()).await?;
    }
    let failed_id = first
        .dispatch_with_options(
            "default",
            RetryJob::NAME,
            &(),
            DispatchOptions {
                max_attempts: 1,
                ..DispatchOptions::default()
            },
        )
        .await?;
    let mut workers = Vec::new();
    for worker in [first.clone(), second.clone(), first.clone(), second.clone()] {
        workers.push(tokio::spawn(async move {
            while worker.run_next("default").await? {}
            Result::<()>::Ok(())
        }));
    }
    for worker in workers {
        worker.await.map_err(Error::internal)??;
    }
    assert_eq!(RUNS.load(Ordering::SeqCst), 41);
    assert_eq!(RETRY_RUNS.load(Ordering::SeqCst), 1);
    let replacement_a = first.retry_failed(&failed_id).await?;
    let replacement_b = second.retry_failed(&failed_id).await?;
    assert_eq!(replacement_a, replacement_b);
    assert!(first.run_next("default").await?);
    assert_eq!(RETRY_RUNS.load(Ordering::SeqCst), 2);
    assert!(!first.run_next("default").await?);
    clean_namespace(&namespace).await
}

#[tokio::test]
async fn redis_queue_recovers_a_task_after_its_worker_process_is_killed() -> Result<()> {
    if std::env::var_os("OPENRUST_REDIS_CRASH_CHILD").is_some() {
        let namespace = std::env::var("OPENRUST_REDIS_CRASH_NAMESPACE").map_err(Error::internal)?;
        let queue = queue(&namespace)?;
        queue.run_next("default").await?;
        return Ok(());
    }

    let namespace = namespace("crash");
    CRASH_RUNS.store(0, Ordering::SeqCst);
    let queue = queue(&namespace)?;
    let id = queue.dispatch("default", CrashJob::NAME, &()).await?;
    let mut child = Command::new(std::env::current_exe().map_err(Error::internal)?)
        .arg("--exact")
        .arg("redis_queue_recovers_a_task_after_its_worker_process_is_killed")
        .arg("--nocapture")
        .env("OPENRUST_REDIS_CRASH_CHILD", "1")
        .env("OPENRUST_REDIS_CRASH_NAMESPACE", &namespace)
        .env("REDIS_URL", redis_url())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Error::internal)?;

    let client = redis::Client::open(redis_url())
        .map_err(|_| Error::custom("test_redis_url_invalid", "Redis test URL is invalid"))?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(Error::internal)?;
    let job_key = format!("{{openrust:{namespace}}}:jobs");
    let mut claimed = false;
    for _ in 0..100 {
        let raw: Option<String> = connection
            .hget(&job_key, &id)
            .await
            .map_err(Error::internal)?;
        if raw
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|record| record.get("attempts").and_then(serde_json::Value::as_u64))
            == Some(1)
        {
            claimed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if !claimed {
        let _ = child.kill();
        let _ = child.wait();
        clean_namespace(&namespace).await?;
        return Err(Error::custom(
            "redis_test_worker_not_started",
            "child worker did not claim the Redis task",
        ));
    }
    child.kill().map_err(Error::internal)?;
    child.wait().map_err(Error::internal)?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!queue.run_next("default").await?);
    tokio::time::sleep(Duration::from_millis(1050)).await;
    assert!(queue.run_next("default").await?);
    assert_eq!(CRASH_RUNS.load(Ordering::SeqCst), 1);
    clean_namespace(&namespace).await
}
