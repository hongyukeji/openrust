//! 验证应用可显式选择 Redis 队列，且不需要数据库服务。
#![cfg(feature = "queue-redis")]

use figment::providers::Format;
use openrust::{
    BoxFuture, Error, Result,
    config::FrameworkConfig,
    queue::{Job, JobContext, Queue, QueueServiceProvider},
};
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
struct ProbeJob;

impl Job for ProbeJob {
    const NAME: &'static str = "tests.server.redis.probe.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn namespace() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("server-{}", nanos % 1_000_000_000_000_000)
}

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_owned())
}

async fn assembled(namespace: &str) -> Result<openrust::App> {
    let url = redis_url();
    let figment = figment::Figment::from(figment::providers::Toml::string(&format!(
        "[queue]\ndriver = 'redis'\nredis_url = {url:?}\nredis_namespace = '{namespace}'\n"
    )));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    let plugin = QueueServiceProvider::new();
    plugin.contributions().register_job::<ProbeJob>()?;
    builder.provider(plugin);
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

#[tokio::test]
async fn queue_plugin_selects_redis_without_a_database_plugin() -> Result<()> {
    let namespace = namespace();
    let app = assembled(&namespace).await?;
    let queue = app.state().resolve::<Queue>().await?;
    queue.dispatch("default", ProbeJob::NAME, &()).await?;
    assert!(queue.run_next("default").await?);
    assert!(!queue.run_next("default").await?);

    let client = redis::Client::open(redis_url())
        .map_err(|_| Error::custom("test_redis_url_invalid", "Redis test URL is invalid"))?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(Error::internal)?;
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(format!("{{openrust:{namespace}}}:*"))
        .query_async(&mut connection)
        .await
        .map_err(Error::internal)?;
    if !keys.is_empty() {
        let _: usize = redis::cmd("DEL")
            .arg(keys)
            .query_async(&mut connection)
            .await
            .map_err(Error::internal)?;
    }
    Ok(())
}

#[tokio::test]
async fn redis_queue_rejects_database_transactional_schedules_at_build() -> Result<()> {
    let namespace = namespace();
    let url = redis_url();
    let figment = figment::Figment::from(figment::providers::Toml::string(&format!(
        "[queue]\ndriver = 'redis'\nredis_url = {url:?}\nredis_namespace = '{namespace}'\n"
    )));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    let plugin = QueueServiceProvider::new();
    plugin.contributions().register_job::<ProbeJob>()?;
    plugin.contributions().schedule_interval(
        "probe",
        "default",
        ProbeJob::NAME,
        &(),
        std::time::Duration::from_secs(60),
    )?;
    builder.provider(plugin);
    let error = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
        .err()
        .ok_or_else(|| {
            Error::custom("test_expected_error", "Redis scheduler unexpectedly built")
        })?;
    assert_eq!(error.code(), "redis_queue_schedule_unsupported");
    Ok(())
}

#[derive(Default)]
struct AttemptTracker {
    next: AtomicUsize,
    closed: AtomicUsize,
    children_stopped: AtomicUsize,
    started: tokio::sync::Notify,
    cleanup: tokio::sync::Notify,
    state: std::sync::Mutex<Option<openrust::core::AppState>>,
    observed: std::sync::Mutex<Vec<(u64, usize)>>,
    scopes: std::sync::Mutex<Vec<Arc<openrust::core::container::Scope>>>,
}
struct AttemptInstance(usize);
struct AttemptBindings(Arc<AttemptTracker>);
impl openrust::core::ServiceProvider for AttemptBindings {
    fn register(&self, app: &mut openrust::core::RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::clone(&self.0));
        app.scoped::<AttemptInstance, _>(|resolver| {
            Box::pin(async move {
                let tracker = resolver.resolve::<AttemptTracker>().await?;
                let id = tracker.next.fetch_add(1, Ordering::SeqCst) + 1;
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        tracker.closed.fetch_add(1, Ordering::SeqCst);
                        tracker.cleanup.notify_one();
                        Ok(())
                    })
                })?;
                Ok(Arc::new(AttemptInstance(id)))
            })
        });
        Ok(())
    }
}
#[derive(serde::Deserialize)]
struct AttemptScopedJob {
    #[serde(default)]
    pending: bool,
}
impl Job for AttemptScopedJob {
    const NAME: &'static str = "tests.attempt-scope.v1";
    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let one = context.service::<AttemptInstance>().await?;
            let two = context.service::<AttemptInstance>().await?;
            assert!(Arc::ptr_eq(&one, &two));
            let tracker = context.service::<AttemptTracker>().await?;
            let state = tracker
                .state
                .lock()
                .map_err(|_| Error::custom("test_lock", "attempt tracker lock failed"))?
                .clone()
                .ok_or_else(|| Error::custom("test_state", "application state missing"))?;
            assert!(Arc::ptr_eq(
                &one,
                &state.resolve::<AttemptInstance>().await?
            ));
            let scope = context.scope()?;
            tracker
                .observed
                .lock()
                .map_err(|_| Error::custom("test_lock", "attempt tracker lock failed"))?
                .push((context.attempt(), one.0));
            tracker
                .scopes
                .lock()
                .map_err(|_| Error::custom("test_lock", "attempt tracker lock failed"))?
                .push(Arc::clone(&scope));
            let started = Arc::new(tokio::sync::Notify::new());
            let ready = Arc::clone(&started);
            let stopped = Arc::clone(&tracker);
            scope.spawn(
                context.metadata().clone(),
                None,
                move |cancellation| async move {
                    ready.notify_one();
                    cancellation.cancelled().await;
                    stopped.children_stopped.fetch_add(1, Ordering::SeqCst);
                },
            )?;
            started.notified().await;
            if self.pending {
                tracker.started.notify_one();
                return std::future::pending::<Result<()>>().await;
            }
            if context.attempt() == 1 {
                return Err(Error::custom("retry_once", "exercise retry"));
            }
            Ok(())
        })
    }
}

// 真实 Redis 状态机必须在确认、重试与取消时关闭 attempt，不能由数据库测试代证。
#[tokio::test]
async fn redis_attempts_isolate_scopes_and_reclaim_after_timeout_and_abort() -> Result<()> {
    let namespace = namespace();
    let tracker = Arc::new(AttemptTracker::default());
    let app = assembled_attempts(&namespace, &tracker).await?;
    let queue = app.state().resolve::<Queue>().await?;
    let client = redis::Client::open(redis_url())
        .map_err(|_| Error::custom("test_redis_url_invalid", "Redis test URL is invalid"))?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(Error::internal)?;
    let id = queue
        .dispatch("retry", AttemptScopedJob::NAME, &serde_json::json!({}))
        .await?;
    assert!(queue.run_next_with_state("retry", app.state()).await?);
    assert_attempt_closed(&tracker, 1).await?;
    // 只推进本测试命名空间内的重试，不依靠固定 sleep 等待后端 backoff。
    let _: usize = redis::cmd("ZADD")
        .arg(format!("{{openrust:{namespace}}}:retry:due"))
        .arg(0)
        .arg(&id)
        .query_async(&mut connection)
        .await
        .map_err(Error::internal)?;
    assert!(queue.run_next_with_state("retry", app.state()).await?);
    assert_attempt_closed(&tracker, 2).await?;
    let observed = tracker
        .observed
        .lock()
        .map_err(|_| Error::custom("test_lock", "observation lock failed"))?
        .clone();
    assert_eq!((observed[0].0, observed[1].0), (1, 2));
    assert_ne!(observed[0].1, observed[1].1);
    assert!(!queue.run_next_with_state("retry", app.state()).await?);

    queue
        .dispatch(
            "timeout",
            AttemptScopedJob::NAME,
            &serde_json::json!({"pending":true}),
        )
        .await?;
    let state = app.state().clone();
    let worker_queue = Arc::clone(&queue);
    let worker =
        tokio::spawn(async move { worker_queue.run_next_with_state("timeout", &state).await });
    tokio::time::timeout(Duration::from_secs(5), tracker.started.notified())
        .await
        .map_err(Error::internal)?;
    assert!(
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .map_err(Error::internal)?
            .map_err(Error::internal)??
    );
    assert_attempt_closed(&tracker, 3).await?;

    queue
        .dispatch(
            "abort",
            AttemptScopedJob::NAME,
            &serde_json::json!({"pending":true}),
        )
        .await?;
    let state = app.state().clone();
    let worker_queue = Arc::clone(&queue);
    let worker =
        tokio::spawn(async move { worker_queue.run_next_with_state("abort", &state).await });
    tokio::time::timeout(Duration::from_secs(5), tracker.started.notified())
        .await
        .map_err(Error::internal)?;
    worker.abort();
    assert!(worker.await.is_err());
    tokio::time::timeout(Duration::from_secs(5), async {
        while tracker.closed.load(Ordering::SeqCst) < 4 {
            tracker.cleanup.notified().await;
        }
    })
    .await
    .map_err(Error::internal)?;
    assert_attempt_closed(&tracker, 4).await?;
    *tracker
        .state
        .lock()
        .map_err(|_| Error::custom("test_lock", "state lock failed"))? = None;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    // 父关闭不得重复执行已经回收的四个 attempt 的回调。
    assert_eq!(tracker.closed.load(Ordering::SeqCst), 4);
    clear_namespace(&namespace, &mut connection).await?;
    Ok(())
}

async fn assert_attempt_closed(tracker: &AttemptTracker, count: usize) -> Result<()> {
    assert_eq!(tracker.closed.load(Ordering::SeqCst), count);
    assert_eq!(tracker.children_stopped.load(Ordering::SeqCst), count);
    let scope = tracker
        .scopes
        .lock()
        .map_err(|_| Error::custom("test_lock", "scope lock failed"))?[count - 1]
        .clone();
    assert_eq!(
        scope
            .resolve::<AttemptInstance>()
            .await
            .err()
            .map(|e| e.code().to_owned())
            .as_deref(),
        Some("scope_closed")
    );
    Ok(())
}

async fn assembled_attempts(
    namespace: &str,
    tracker: &Arc<AttemptTracker>,
) -> Result<openrust::App> {
    let provider = QueueServiceProvider::new();
    provider
        .contributions()
        .register_job::<AttemptScopedJob>()?;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    )).merge(figment::providers::Toml::string(&format!(
        "[queue]\ndriver = 'redis'\nredis_url = {:?}\nredis_namespace = '{namespace}'\njob_timeout_secs = 1\n",
        redis_url()
    )));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(AttemptBindings(Arc::clone(tracker)))
        .provider(provider);
    let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
    *tracker
        .state
        .lock()
        .map_err(|_| Error::custom("test_lock", "state lock failed"))? = Some(app.state().clone());
    Ok(app)
}

async fn clear_namespace(
    namespace: &str,
    connection: &mut redis::aio::MultiplexedConnection,
) -> Result<()> {
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(format!("{{openrust:{namespace}}}:*"))
        .query_async(&mut *connection)
        .await
        .map_err(Error::internal)?;
    if !keys.is_empty() {
        let _: usize = redis::cmd("DEL")
            .arg(keys)
            .query_async(&mut *connection)
            .await
            .map_err(Error::internal)?;
    }
    Ok(())
}
