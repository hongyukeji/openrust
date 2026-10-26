//! Verifies that a real process crash is recovered by the database job lease.
#![cfg(all(feature = "queue", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::config::FrameworkConfig;
use openrust::core::{App, AppBuilder, BoxFuture, Error, Result as FrameworkResult, lifecycle};
use openrust::database::{
    DatabasePlugin, Db, DbBackend,
    sea_orm::Statement,
    sea_orm_migration::{MigrationTrait, MigratorTrait},
};
use openrust::queue::{DispatchOptions, Job, JobContext, Queue, QueueMigration, QueuePlugin};
use sea_orm::ConnectionTrait;
use std::process::{Command, Stdio};
use std::time::Duration;

struct QueueMigrator;

impl MigratorTrait for QueueMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(QueueMigration)]
    }
}

#[derive(serde::Deserialize)]
struct CrashProbe;

impl Job for CrashProbe {
    const NAME: &'static str = "tests.crash-probe.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        let attempt = context.attempt();
        Box::pin(async move {
            context
                .set_progress(&serde_json::json!({"attempt": attempt}))
                .await?;
            if attempt == 1 {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
            Err(Error::custom("probe_failed", "crash recovery probe"))
        })
    }
}

async fn build_app(url: &str) -> FrameworkResult<App> {
    let source = format!(
        "[database]\nurl = {url:?}\n[queue]\npoll_interval_millis = 20\nlease_secs = 1\njob_timeout_secs = 60\nworker_concurrency = 1\nshutdown_timeout_secs = 2\n"
    );
    let figment = Figment::new().merge(Toml::string(&source));
    let mut queue_plugin = QueuePlugin::default();
    queue_plugin.register_job::<CrashProbe>()?;
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin).plugin(queue_plugin);
    lifecycle::finalize(builder).await
}

#[tokio::test]
async fn process_crash_releases_job_after_lease_expiry() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping queue crash recovery: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let test_database = testing::TestDatabase::create::<QueueMigrator>(&admin_url).await?;
    let app = build_app(test_database.url()).await?;
    let queue = app.state().try_get::<Queue>()?;
    let db = app.state().try_get::<Db>()?;
    let queue_name = format!("crash{}", std::process::id());
    let job_id = queue
        .dispatch_with_options(
            &queue_name,
            CrashProbe::NAME,
            &(),
            DispatchOptions {
                max_attempts: 2,
                delay: Duration::ZERO,
                unique_key: None,
            },
        )
        .await?;

    let mut child = Command::new(std::env::current_exe()?)
        .args(["--exact", "child_worker_holds_crash_probe", "--nocapture"])
        .env("OPENRUST_QUEUE_CHILD_URL", test_database.url())
        .env("OPENRUST_QUEUE_CHILD_NAME", &queue_name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let table = db.quoted_table_name("jobs")?;
    let backend = db.backend()?;
    let query = match backend {
        DbBackend::Postgres => format!("SELECT progress FROM {table} WHERE id = $1"),
        DbBackend::MySql => format!("SELECT progress FROM {table} WHERE id = ?"),
        _ => return Err("unsupported test database".into()),
    };
    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let row = db
                .connection()?
                .query_one_raw(Statement::from_sql_and_values(
                    backend,
                    query.clone(),
                    vec![job_id.clone().into()],
                ))
                .await
                .map_err(Error::internal)?;
            if row.is_some_and(|row| {
                row.try_get::<Option<serde_json::Value>>("", "progress")
                    .ok()
                    .flatten()
                    .is_some()
            }) {
                return Ok::<(), Error>(());
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    if observed.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("worker process did not claim the probe job".into());
    }
    child.kill()?;
    child.wait()?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(queue.run_next(&queue_name).await?);

    let failed_table = db.quoted_table_name("failed_jobs")?;
    let query = match backend {
        DbBackend::Postgres => format!("SELECT attempts FROM {failed_table} WHERE id = $1"),
        DbBackend::MySql => format!("SELECT attempts FROM {failed_table} WHERE id = ?"),
        _ => return Err("unsupported test database".into()),
    };
    let row = db
        .connection()?
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            query,
            vec![job_id.into()],
        ))
        .await?
        .ok_or("recovered job was not moved to failed_jobs")?;
    assert_eq!(row.try_get::<i64>("", "attempts")?, 2);

    drop(queue);
    drop(db);
    drop(app);
    test_database.close().await?;
    Ok(())
}

#[tokio::test]
async fn child_worker_holds_crash_probe() -> Result<(), Box<dyn std::error::Error>> {
    let (Ok(url), Ok(queue_name)) = (
        std::env::var("OPENRUST_QUEUE_CHILD_URL"),
        std::env::var("OPENRUST_QUEUE_CHILD_NAME"),
    ) else {
        return Ok(());
    };
    let app = build_app(&url).await?;
    let queue = app.state().try_get::<Queue>()?;
    if !queue.run_next(&queue_name).await? {
        return Err("child worker could not claim probe job".into());
    }
    Ok(())
}
