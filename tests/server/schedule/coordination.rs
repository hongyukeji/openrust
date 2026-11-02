//! 验证独立进程通过真实数据库对调度时间槽去重。
#![cfg(all(feature = "queue", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::config::FrameworkConfig;
use openrust::core::{App, BoxFuture, Result as FrameworkResult};
use openrust::database::{
    DatabaseServiceProvider, Db,
    sea_orm::Statement,
    sea_orm_migration::{MigrationTrait, MigratorTrait},
};
use openrust::queue::{Job, JobContext, QueueMigration, QueueServiceProvider};
use openrust::schedule::Scheduler;
use sea_orm::ConnectionTrait;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct ScheduleMigrator;

impl MigratorTrait for ScheduleMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(QueueMigration)]
    }
}

#[derive(serde::Deserialize)]
struct TickJob;

impl Job for TickJob {
    const NAME: &'static str = "tests.schedule-tick.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

async fn build_app(url: &str) -> FrameworkResult<App> {
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let queue_plugin = QueueServiceProvider::new();
    queue_plugin.contributions().register_job::<TickJob>()?;
    queue_plugin.contributions().schedule_interval(
        "tests.schedule-tick.v1",
        "schedule-tests",
        TickJob::NAME,
        &serde_json::json!({}),
        Duration::from_secs(60),
    )?;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(queue_plugin);
    builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)
}

#[tokio::test]
async fn independent_scheduler_processes_dispatch_one_job_per_slot()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping scheduler integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<ScheduleMigrator>(&admin_url).await?;
    let app = build_app(database.url()).await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    let db = app.state().resolve::<Db>().await?;
    let target = SystemTime::now() + Duration::from_secs(2);
    let target_millis = target.duration_since(UNIX_EPOCH)?.as_millis().to_string();
    let ready_path = format!("/tmp/openrust-scheduler-ready-{}", std::process::id());
    let _ = std::fs::remove_file(&ready_path);
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "child_scheduler_dispatches_due_slot",
            "--nocapture",
        ])
        .env("OPENRUST_SCHEDULE_CHILD_URL", database.url())
        .env("OPENRUST_SCHEDULE_START_AT", &target_millis)
        .env("OPENRUST_SCHEDULE_READY_FILE", &ready_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !std::path::Path::new(&ready_path).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    std::fs::remove_file(&ready_path)?;
    let child_wait = tokio::task::spawn_blocking(move || child.wait());
    wait_until(target).await;
    let (parent_report, status) = tokio::join!(scheduler.tick(), child_wait);
    let parent_report = parent_report?;
    let status = status??;
    assert_eq!(
        parent_report.dispatched + parent_report.already_dispatched,
        1
    );
    assert!(status.success());

    let mut crash_child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "child_scheduler_exits_after_committing_slot",
            "--nocapture",
        ])
        .env("OPENRUST_SCHEDULE_CRASH_URL", database.url())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let crash_status = tokio::task::spawn_blocking(move || crash_child.wait()).await??;
    assert!(!crash_status.success());

    let commands = app
        .commands()
        .map(openrust::core::Command::name)
        .collect::<Vec<_>>();
    assert!(commands.contains(&"schedule run"));
    assert!(commands.contains(&"schedule list"));
    app.run_command("schedule", vec!["run".into(), "--once".into()])
        .await?;

    let mut signal_child = Command::new(std::env::current_exe()?)
        .args(["--exact", "child_scheduler_stops_on_sigterm", "--nocapture"])
        .env("OPENRUST_SCHEDULE_SIGNAL_URL", database.url())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let signal_status = tokio::task::spawn_blocking(move || signal_child.wait()).await??;
    assert!(signal_status.success());

    let first = database_count(&db, "schedule_runs").await?;
    let jobs = database_count(&db, "jobs").await?;
    assert_eq!(first, 1);
    assert_eq!(jobs, 1);

    drop(scheduler);
    drop(db);
    drop(app);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn child_scheduler_dispatches_due_slot() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_SCHEDULE_CHILD_URL") else {
        return Ok(());
    };
    let app = build_app(&url).await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    let start = std::env::var("OPENRUST_SCHEDULE_START_AT")?.parse::<u64>()?;
    std::fs::write(std::env::var("OPENRUST_SCHEDULE_READY_FILE")?, b"ready")?;
    wait_until(SystemTime::UNIX_EPOCH + Duration::from_millis(start)).await;
    scheduler.tick().await?;
    Ok(())
}

#[tokio::test]
async fn child_scheduler_stops_on_sigterm() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_SCHEDULE_SIGNAL_URL") else {
        return Ok(());
    };
    let app = build_app(&url).await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    let signaler = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        tokio::task::spawn_blocking(move || {
            Command::new("kill")
                .args(["-TERM", &std::process::id().to_string()])
                .status()
        })
        .await
        .map_err(std::io::Error::other)?
    });
    scheduler.run().await?;
    assert!(signaler.await??.success());
    Ok(())
}

#[tokio::test]
async fn child_scheduler_exits_after_committing_slot() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("OPENRUST_SCHEDULE_CRASH_URL") else {
        return Ok(());
    };
    let app = build_app(&url).await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    scheduler.tick().await?;
    std::process::exit(97);
}

async fn database_count(db: &Db, logical_table: &str) -> FrameworkResult<i64> {
    let backend = db.backend()?;
    let table = db.quoted_table_name(logical_table)?;
    let sql = format!("SELECT COUNT(*) AS count FROM {table}");
    let row = db
        .connection()?
        .query_one_raw(Statement::from_string(backend, sql))
        .await
        .map_err(openrust::Error::internal)?
        .ok_or_else(|| {
            openrust::Error::custom("test_count_missing", "count query returned no row")
        })?;
    let count: i64 = row
        .try_get("", "count")
        .map_err(openrust::Error::internal)?;
    Ok(count)
}

async fn wait_until(target: SystemTime) {
    if let Ok(delay) = target.duration_since(SystemTime::now()) {
        tokio::time::sleep(delay).await;
    }
}

#[derive(Default)]
struct TriggerObservations {
    next: std::sync::atomic::AtomicUsize,
    closed: std::sync::atomic::AtomicUsize,
    mode: std::sync::atomic::AtomicUsize,
    started: tokio::sync::Notify,
    cleaned: tokio::sync::Notify,
    ids: std::sync::Mutex<Vec<usize>>,
    queued: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
    state: std::sync::Mutex<Option<openrust::core::AppState>>,
}
struct TriggerValue(usize);
struct TriggerDriver(
    std::sync::Arc<TriggerObservations>,
    openrust::queue::JobRegistry,
);
impl openrust::queue::QueueDriver for TriggerDriver {
    fn dispatch<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
        _: serde_json::Value,
        _: openrust::queue::DispatchOptions,
    ) -> BoxFuture<'a, FrameworkResult<String>> {
        Box::pin(async {
            Err(openrust::core::Error::custom(
                "unexpected_dispatch",
                "expected transaction",
            ))
        })
    }
    fn dispatch_in_transaction<'a>(
        &'a self,
        _: &'a openrust::database::DatabaseTransaction,
        _: &'a str,
        job_name: &'a str,
        payload: serde_json::Value,
        _: openrust::queue::DispatchOptions,
    ) -> BoxFuture<'a, FrameworkResult<String>> {
        Box::pin(async move {
            assert!(openrust::tenant::TenantContext::current().is_none());
            assert!(
                openrust::core::ExecutionContext::current()
                    .is_some_and(|context| context.metadata().is_empty())
            );
            let state = self
                .0
                .state
                .lock()
                .map_err(|_| openrust::core::Error::custom("test_lock", "lock failed"))?
                .clone()
                .ok_or_else(|| openrust::core::Error::custom("test_state", "missing state"))?;
            let one = state.resolve::<TriggerValue>().await?;
            let two = state.resolve::<TriggerValue>().await?;
            assert!(std::sync::Arc::ptr_eq(&one, &two));
            self.0
                .ids
                .lock()
                .map_err(|_| openrust::core::Error::custom("test_lock", "lock failed"))?
                .push(one.0);
            match self.0.mode.load(std::sync::atomic::Ordering::SeqCst) {
                1 => {
                    return Err(openrust::core::Error::custom(
                        "trigger_failed",
                        "test dispatch failed",
                    ));
                }
                2 => {
                    self.0.started.notify_one();
                    return std::future::pending().await;
                }
                _ => {}
            }
            self.0
                .queued
                .lock()
                .map_err(|_| openrust::core::Error::custom("test_lock", "lock failed"))?
                .push((job_name.to_owned(), payload));
            Ok(format!("{:032x}", one.0))
        })
    }
    fn run_next<'a>(&'a self, _: &'a str) -> BoxFuture<'a, FrameworkResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn run_next_with_state<'a>(
        &'a self,
        _: &'a str,
        state: &'a openrust::core::AppState,
    ) -> BoxFuture<'a, FrameworkResult<bool>> {
        Box::pin(async move {
            let task = self
                .0
                .queued
                .lock()
                .map_err(|_| openrust::core::Error::custom("test_lock", "lock failed"))?
                .pop();
            if let Some((name, payload)) = task {
                let context =
                    JobContext::new("scheduled-job", 1, 1, openrust::core::Cancellation::new())?
                        .with_app_state(state.clone());
                self.1.execute(&name, payload, context).await?;
                Ok(true)
            } else {
                Ok(false)
            }
        })
    }
    fn retry_failed<'a>(&'a self, _: &'a str) -> BoxFuture<'a, FrameworkResult<String>> {
        Box::pin(async { Ok(String::new()) })
    }
    fn reset_shutdown(&self) {}
    fn request_shutdown(&self) {}
    fn is_shutdown_requested(&self) -> bool {
        false
    }
}
#[derive(serde::Deserialize)]
struct TriggeredJob {}
impl Job for TriggeredJob {
    const NAME: &'static str = "tests.scheduler-scope-job";
    fn handle(&self, context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async move {
            assert!(context.metadata().is_empty());
            assert!(openrust::tenant::TenantContext::current().is_none());
            let value = context.service::<TriggerValue>().await?;
            let tracker = context.service::<TriggerObservations>().await?;
            tracker
                .ids
                .lock()
                .map_err(|_| openrust::core::Error::custom("test_lock", "lock failed"))?
                .push(value.0);
            Ok(())
        })
    }
}
struct TriggerProvider(std::sync::Arc<TriggerObservations>);
impl openrust::core::ServiceProvider for TriggerProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, FrameworkResult<()>> {
        Box::pin(async move {
            let observations = std::sync::Arc::clone(&self.0);
            app.instance(std::sync::Arc::clone(&observations)).await?;
            app.scoped::<TriggerValue, _>(|resolver| {
                Box::pin(async move {
                    use std::sync::atomic::Ordering;
                    let observations = resolver.resolve::<TriggerObservations>().await?;
                    let id = observations.next.fetch_add(1, Ordering::SeqCst) + 1;
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            observations.closed.fetch_add(1, Ordering::SeqCst);
                            observations.cleaned.notify_one();
                            Ok(())
                        })
                    })?;
                    Ok(std::sync::Arc::new(TriggerValue(id)))
                })
            })
            .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn each_due_slot_uses_and_closes_its_own_scope() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, atomic::Ordering};
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping scheduler trigger scope: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<ScheduleMigrator>(&admin_url).await?;
    let observations = Arc::new(TriggerObservations::default());
    let queue = QueueServiceProvider::new();
    queue.contributions().register_job::<TriggeredJob>()?;
    let driver_observations = Arc::clone(&observations);
    queue
        .contributions()
        .extend("trigger-test", move |_, _, _, _, registry| {
            let observations = Arc::clone(&driver_observations);
            Box::pin(async move {
                Ok(Arc::new(TriggerDriver(observations, registry))
                    as Arc<dyn openrust::queue::QueueDriver>)
            })
        })?;
    for key in ["scope-first", "scope-second"] {
        queue.contributions().schedule_interval(
            key,
            "scope-tests",
            TriggeredJob::NAME,
            &serde_json::json!({}),
            Duration::from_secs(60),
        )?;
    }
    let source = Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Toml::string(&format!(
        "[database]\nurl = {:?}\n[queue]\ndefault = 'default'\n[queue.connections.default]\ndriver = 'trigger-test'\n",
        database.url()
    )));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(queue)
        .provider(TriggerProvider(Arc::clone(&observations)));
    let app = App::from_provider_runtime(builder.start(source).await?)?;
    *observations.state.lock().map_err(|_| "test lock")? = Some(app.state().clone());
    let scheduler = app.state().resolve::<Scheduler>().await?;
    let metadata = openrust::core::ExecutionMetadata::default()
        .with_request_id("request-secret")?
        .with_tenant_id(openrust::tenant::TenantId::new("tenant-a")?);
    let execution =
        openrust::core::ExecutionContext::new(metadata, None, openrust::core::Cancellation::new());
    let report = Box::pin(openrust::tenant::TenantContext::scope(
        openrust::tenant::TenantId::new("tenant-a")?,
        openrust::core::with_execution_context(execution, scheduler.tick()),
    ))
    .await??;
    assert_eq!(report.dispatched, 2);
    assert_eq!(
        *observations.ids.lock().map_err(|_| "test lock")?,
        vec![1, 2]
    );
    assert_eq!(observations.closed.load(Ordering::SeqCst), 2);
    let queue = app.state().resolve::<openrust::queue::Queue>().await?;
    assert!(
        queue
            .run_next_with_state("scope-tests", app.state())
            .await?
    );
    assert_eq!(
        *observations.ids.lock().map_err(|_| "test lock")?,
        vec![1, 2, 3]
    );
    assert_eq!(observations.closed.load(Ordering::SeqCst), 3);
    drop(queue);
    *observations.state.lock().map_err(|_| "test lock")? = None;
    drop(scheduler);
    drop(app);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn failed_and_cancelled_trigger_close_scoped_resources()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, atomic::Ordering};
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping scheduler trigger cancellation: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<ScheduleMigrator>(&admin_url).await?;
    let observations = Arc::new(TriggerObservations::default());
    let queue = QueueServiceProvider::new();
    queue.contributions().register_job::<TickJob>()?;
    queue.contributions().schedule_interval(
        "failure-scope",
        "scope-tests",
        TickJob::NAME,
        &serde_json::json!({}),
        Duration::from_secs(60),
    )?;
    let driver_observations = Arc::clone(&observations);
    queue
        .contributions()
        .extend("trigger-test", move |_, _, _, _, registry| {
            let observations = Arc::clone(&driver_observations);
            Box::pin(async move {
                Ok(Arc::new(TriggerDriver(observations, registry))
                    as Arc<dyn openrust::queue::QueueDriver>)
            })
        })?;
    let source = Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Toml::string(&format!(
        "[database]\nurl = {:?}\n[queue]\ndefault = 'default'\n[queue.connections.default]\ndriver = 'trigger-test'\n",
        database.url()
    )));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(queue)
        .provider(TriggerProvider(Arc::clone(&observations)));
    let app = App::from_provider_runtime(builder.start(source).await?)?;
    *observations.state.lock().map_err(|_| "test lock")? = Some(app.state().clone());
    let scheduler = app.state().resolve::<Scheduler>().await?;
    observations.mode.store(1, Ordering::SeqCst);
    assert_eq!(
        scheduler
            .tick()
            .await
            .err()
            .map(|e| e.code().to_owned())
            .as_deref(),
        Some("trigger_failed")
    );
    assert_eq!(observations.closed.load(Ordering::SeqCst), 1);
    observations.mode.store(2, Ordering::SeqCst);
    let task_scheduler = Arc::clone(&scheduler);
    let task = tokio::spawn(async move { task_scheduler.tick().await });
    observations.started.notified().await;
    task.abort();
    let _ = task.await;
    while observations.closed.load(Ordering::SeqCst) < 2 {
        observations.cleaned.notified().await;
    }
    assert_eq!(
        *observations.ids.lock().map_err(|_| "test lock")?,
        vec![1, 2]
    );
    *observations.state.lock().map_err(|_| "test lock")? = None;
    drop(scheduler);
    drop(app);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn database_scheduled_jobs_use_distinct_attempt_scopes()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{Arc, atomic::Ordering};
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database scheduler worker scope: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<ScheduleMigrator>(&admin_url).await?;
    let observations = Arc::new(TriggerObservations::default());
    let provider = QueueServiceProvider::new();
    provider.contributions().register_job::<TriggeredJob>()?;
    for key in ["db-scope-first", "db-scope-second"] {
        provider.contributions().schedule_interval(
            key,
            "db-scope-tests",
            TriggeredJob::NAME,
            &serde_json::json!({}),
            Duration::from_secs(60),
        )?;
    }
    let source = Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Toml::string(&format!(
        "[database]\nurl = {:?}\n",
        database.url()
    )));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(provider)
        .provider(TriggerProvider(Arc::clone(&observations)));
    let app = App::from_provider_runtime(builder.start(source).await?)?;
    let root = app.state().resolve::<TriggerValue>().await?;
    let scheduler = app.state().resolve::<Scheduler>().await?;
    let metadata = openrust::core::ExecutionMetadata::default()
        .with_request_id("caller-request")?
        .with_tenant_id(openrust::tenant::TenantId::new("tenant-a")?);
    let execution =
        openrust::core::ExecutionContext::new(metadata, None, openrust::core::Cancellation::new());
    let report = Box::pin(openrust::tenant::TenantContext::scope(
        openrust::tenant::TenantId::new("tenant-a")?,
        openrust::core::with_execution_context(execution, scheduler.tick()),
    ))
    .await??;
    assert_eq!(report.dispatched, 2);
    let queue = app.state().resolve::<openrust::queue::Queue>().await?;
    assert!(
        queue
            .run_next_with_state("db-scope-tests", app.state())
            .await?
    );
    assert!(
        queue
            .run_next_with_state("db-scope-tests", app.state())
            .await?
    );
    assert_eq!(root.0, 1);
    assert_eq!(
        *observations.ids.lock().map_err(|_| "test lock")?,
        vec![2, 3]
    );
    assert_eq!(observations.closed.load(Ordering::SeqCst), 2);
    drop(queue);
    drop(scheduler);
    drop(root);
    drop(app);
    database.close().await?;
    Ok(())
}
