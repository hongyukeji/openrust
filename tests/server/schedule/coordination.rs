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
