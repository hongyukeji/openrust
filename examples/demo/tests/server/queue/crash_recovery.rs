//! 验证数据库任务租约能够恢复真实进程崩溃后的任务。
#![cfg(all(feature = "queue", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::config::FrameworkConfig;
use openrust::core::{
    App, AppBuilder, BoxFuture, Cancellation, Error, ExecutionContext, ExecutionMetadata,
    Result as FrameworkResult, lifecycle,
};
use openrust::database::{
    DatabasePlugin, Db, DbBackend,
    sea_orm::Statement,
    sea_orm_migration::{MigrationTrait, MigratorTrait},
};
use openrust::queue::{DispatchOptions, Job, JobContext, Queue, QueueMigration, QueuePlugin};
use sea_orm::ConnectionTrait;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

const QUEUE_TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

static LEASE_PROBE_STARTED: OnceLock<tokio::sync::Notify> = OnceLock::new();
static SHUTDOWN_TEST_DB: OnceLock<Db> = OnceLock::new();

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

#[derive(serde::Deserialize)]
struct ShutdownProbe {
    cooperative: bool,
    #[serde(default)]
    disconnect_on_cancel: bool,
}

impl Job for ShutdownProbe {
    const NAME: &'static str = "tests.shutdown-probe.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        let cooperative = self.cooperative;
        let disconnect_on_cancel = self.disconnect_on_cancel;
        Box::pin(async move {
            context
                .set_progress(&serde_json::json!({"started": true}))
                .await?;
            if cooperative {
                context.cancelled().await;
                if disconnect_on_cancel {
                    let db = SHUTDOWN_TEST_DB.get().ok_or_else(|| {
                        Error::custom("test_database_missing", "test database was not configured")
                    })?;
                    db.connection()?.close().await.map_err(Error::internal)?;
                    std::future::pending::<()>().await;
                }
                Ok(())
            } else {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(())
            }
        })
    }
}

#[derive(serde::Deserialize)]
struct StaleLeaseProbe;

impl Job for StaleLeaseProbe {
    const NAME: &'static str = "tests.stale-lease-probe.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async move {
            LEASE_PROBE_STARTED
                .get_or_init(tokio::sync::Notify::new)
                .notify_one();
            if context.attempt() == 1 {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            Ok(())
        })
    }
}

#[derive(serde::Deserialize)]
struct TenantRetryProbe;

impl Job for TenantRetryProbe {
    const NAME: &'static str = "tests.tenant-retry-probe.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async move {
            if openrust::tenant::TenantContext::current().as_ref() != context.metadata().tenant_id()
            {
                return Err(Error::custom(
                    "tenant_scope_missing",
                    "worker did not restore the trusted tenant context",
                ));
            }
            if context.attempt() == 1 {
                return Err(Error::custom(
                    "tenant_retry_probe",
                    "retry the tenant probe",
                ));
            }
            Ok(())
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
    queue_plugin.register_job::<ShutdownProbe>()?;
    queue_plugin.register_job::<StaleLeaseProbe>()?;
    queue_plugin.register_job::<TenantRetryProbe>()?;
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin).plugin(queue_plugin);
    lifecycle::finalize(builder).await
}

#[tokio::test]
async fn tenant_metadata_survives_a_separate_worker_process_and_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping tenant queue process integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<QueueMigrator>(&admin_url).await?;
    let app = build_app(database.url()).await?;
    let queue = app.state().try_get::<Queue>()?;
    let queue_name = format!("tenant-process-{}", std::process::id());
    let execution = ExecutionContext::new(
        ExecutionMetadata::default().with_traceparent(QUEUE_TRACEPARENT)?,
        None,
        Cancellation::new(),
    );
    let job_id = execution
        .run(async {
            openrust::tenant::TenantContext::scope(
                openrust::tenant::TenantId::new("tenant-process-a")?,
                async {
                    queue
                        .dispatch_with_options(
                            &queue_name,
                            TenantRetryProbe::NAME,
                            &(),
                            DispatchOptions {
                                max_attempts: 3,
                                delay: Duration::ZERO,
                                unique_key: None,
                            },
                        )
                        .await
                },
            )
            .await
        })
        .await???;
    drop(queue);
    drop(app);

    let child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "child_worker_processes_tenant_retry_probe",
            "--nocapture",
        ])
        .env("OPENRUST_TENANT_QUEUE_CHILD_URL", database.url())
        .env("OPENRUST_TENANT_QUEUE_CHILD_NAME", &queue_name)
        .env("RUST_LOG", "info")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()?;
    if !child.success() {
        return Err("separate worker process failed to handle tenant probe".into());
    }

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let retry_app = build_app(database.url()).await?;
    let retry_queue = retry_app.state().try_get::<Queue>()?;
    assert!(retry_queue.run_next(&queue_name).await?);
    let db = retry_app.state().try_get::<Db>()?;
    let jobs = db.quoted_table_name("jobs")?;
    let backend = db.backend()?;
    let query = match backend {
        DbBackend::Postgres => format!("SELECT COUNT(*) AS count FROM {jobs} WHERE id = $1"),
        DbBackend::MySql => format!("SELECT COUNT(*) AS count FROM {jobs} WHERE id = ?"),
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
        .ok_or("queue retry count query returned no row")?;
    assert_eq!(row.try_get::<i64>("", "count")?, 0);
    drop(retry_queue);
    drop(db);
    drop(retry_app);
    database.close().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::too_many_lines)] // Process setup, signal delivery, and database assertions are one end-to-end lifecycle test.
async fn worker_sigterm_drains_cooperative_handler_and_times_out_other_handlers()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping queue shutdown integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<QueueMigrator>(&admin_url).await?;
    for (cooperative, disconnect_on_cancel) in [(true, false), (false, false), (true, true)] {
        let app = build_app(database.url()).await?;
        let queue = app.state().try_get::<Queue>()?;
        let db = app.state().try_get::<Db>()?;
        let queue_name = format!("drain-{cooperative}-{disconnect_on_cancel}");
        let job_id = queue
            .dispatch_with_options(
                &queue_name,
                ShutdownProbe::NAME,
                &serde_json::json!({"cooperative": cooperative, "disconnect_on_cancel": disconnect_on_cancel}),
                DispatchOptions {
                    max_attempts: 3,
                    delay: Duration::ZERO,
                    unique_key: None,
                },
            )
            .await?;
        drop(queue);
        drop(db);
        drop(app);

        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "child_queue_worker_drains_on_sigterm",
                "--nocapture",
            ])
            .env("OPENRUST_QUEUE_SHUTDOWN_URL", database.url())
            .env("OPENRUST_QUEUE_SHUTDOWN_NAME", &queue_name)
            .env(
                "OPENRUST_QUEUE_SHUTDOWN_COOPERATIVE",
                cooperative.to_string(),
            )
            .env(
                "OPENRUST_QUEUE_SHUTDOWN_DISCONNECT",
                disconnect_on_cancel.to_string(),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let observe = build_app(database.url()).await?;
        let db = observe.state().try_get::<Db>()?;
        let backend = db.backend()?;
        let table = db.quoted_table_name("jobs")?;
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
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        if observed.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("worker did not start the shutdown probe".into());
        }
        let observe_queue = observe.state().try_get::<Queue>()?;
        let pending_id = observe_queue
            .dispatch_with_options(
                &queue_name,
                ShutdownProbe::NAME,
                &serde_json::json!({"cooperative": true}),
                DispatchOptions {
                    max_attempts: 3,
                    delay: Duration::ZERO,
                    unique_key: None,
                },
            )
            .await?;
        let signal = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status();
        if !signal.is_ok_and(|status| status.success()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("could not send SIGTERM to the queue worker".into());
        }
        let status = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    return Ok::<_, std::io::Error>(status);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        let status = if let Ok(status) = status {
            status?
        } else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("queue worker did not stop within the bounded shutdown window".into());
        };
        assert!(
            status.success(),
            "worker child failed for cooperative={cooperative}"
        );

        let row = db
            .connection()?
            .query_one_raw(Statement::from_string(
                backend,
                format!("SELECT reserved_by FROM {table} WHERE id = '{job_id}'"),
            ))
            .await?;
        if cooperative && !disconnect_on_cancel {
            assert!(
                row.is_none(),
                "cooperative handler should have been acknowledged"
            );
        } else {
            let reserved_by: Option<String> = row
                .ok_or("timed out job was not retained for retry")?
                .try_get("", "reserved_by")?;
            if disconnect_on_cancel {
                assert!(
                    reserved_by.is_some(),
                    "database failure must leave the lease for expiry recovery"
                );
            } else {
                assert!(
                    reserved_by.is_none(),
                    "timed out job lease was not released"
                );
            }
        }
        let pending = db
            .connection()?
            .query_one_raw(Statement::from_string(
                backend,
                format!("SELECT reserved_by FROM {table} WHERE id = '{pending_id}'"),
            ))
            .await?
            .ok_or("worker claimed another job after shutdown was requested")?;
        let pending_owner: Option<String> = pending.try_get("", "reserved_by")?;
        assert!(
            pending_owner.is_none(),
            "worker claimed a new job during shutdown"
        );
        db.connection()?.close().await?;
        drop(db);
        drop(observe);
    }
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn stale_worker_token_cannot_acknowledge_a_reclaimed_job()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping stale lease integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = openrust::testing::TestDatabase::create::<QueueMigrator>(&admin_url).await?;
    let app = build_app(database.url()).await?;
    let queue = app.state().try_get::<Queue>()?;
    let db = app.state().try_get::<Db>()?;
    let queue_name = format!("stale{}", std::process::id());
    let job_id = queue
        .dispatch_with_options(
            &queue_name,
            StaleLeaseProbe::NAME,
            &serde_json::Value::Null,
            DispatchOptions {
                max_attempts: 3,
                delay: Duration::ZERO,
                unique_key: None,
            },
        )
        .await?;
    let first_queue = queue.clone();
    let first_name = queue_name.clone();
    let mut first_worker = tokio::spawn(async move { first_queue.run_next(&first_name).await });
    let started = LEASE_PROBE_STARTED
        .get_or_init(tokio::sync::Notify::new)
        .notified();
    tokio::pin!(started);
    tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(10), &mut started) => {
            result.map_err(|_| "stale lease probe handler did not start")?;
        }
        result = &mut first_worker => return Err(format!("old worker exited before entering the handler: {result:?}").into()),
    }
    let backend = db.backend()?;
    let table = db.quoted_table_name("jobs")?;
    let expire_sql = match backend {
        DbBackend::Postgres => format!(
            "UPDATE {table} SET reserved_until = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE id = $1"
        ),
        DbBackend::MySql => format!(
            "UPDATE {table} SET reserved_until = UTC_TIMESTAMP(6) - INTERVAL 1 SECOND WHERE id = ?"
        ),
        _ => return Err("unsupported test database".into()),
    };
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(
            backend,
            expire_sql,
            vec![job_id.clone().into()],
        ))
        .await?;
    assert!(queue.run_next(&queue_name).await?);
    let stale_result = tokio::time::timeout(Duration::from_secs(5), first_worker).await??;
    assert!(stale_result.is_err());
    let row = db
        .connection()?
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT id FROM {table} WHERE id = '{job_id}'"),
        ))
        .await?;
    assert!(
        row.is_none(),
        "stale worker changed the replacement worker's completion"
    );
    db.connection()?.close().await?;
    drop(db);
    drop(queue);
    drop(app);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn child_queue_worker_drains_on_sigterm() -> Result<(), Box<dyn std::error::Error>> {
    let (Ok(url), Ok(queue_name), Ok(cooperative)) = (
        std::env::var("OPENRUST_QUEUE_SHUTDOWN_URL"),
        std::env::var("OPENRUST_QUEUE_SHUTDOWN_NAME"),
        std::env::var("OPENRUST_QUEUE_SHUTDOWN_COOPERATIVE"),
    ) else {
        return Ok(());
    };
    let cooperative = cooperative.parse::<bool>()?;
    let app = build_app(&url).await?;
    let db = app.state().try_get::<Db>()?;
    let _ = SHUTDOWN_TEST_DB.set((*db).clone());
    match app
        .run_command("queue", vec!["work".into(), queue_name])
        .await
    {
        Ok(()) if cooperative => Ok(()),
        Err(error) if !cooperative && error.code() == "queue_shutdown_timeout" => Ok(()),
        Err(error)
            if cooperative
                && std::env::var("OPENRUST_QUEUE_SHUTDOWN_DISCONNECT").as_deref() == Ok("true")
                && error.code() == "internal_error" =>
        {
            Ok(())
        }
        Ok(()) => Err("uncooperative handler shutdown should report timeout".into()),
        Err(error) => Err(format!("unexpected worker shutdown error: {}", error.code()).into()),
    }
}

#[tokio::test]
async fn process_crash_releases_job_after_lease_expiry() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping queue crash recovery: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let test_database =
        openrust::testing::TestDatabase::create::<QueueMigrator>(&admin_url).await?;
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

#[tokio::test]
async fn child_worker_processes_tenant_retry_probe() -> Result<(), Box<dyn std::error::Error>> {
    let (Ok(url), Ok(queue_name)) = (
        std::env::var("OPENRUST_TENANT_QUEUE_CHILD_URL"),
        std::env::var("OPENRUST_TENANT_QUEUE_CHILD_NAME"),
    ) else {
        return Ok(());
    };
    let config = FrameworkConfig::default();
    lifecycle::init_tracing(&config.log);
    let app = build_app(&url).await?;
    let queue = app.state().try_get::<Queue>()?;
    if !queue.run_next(&queue_name).await? {
        return Err("child worker could not claim tenant retry probe".into());
    }
    drop(queue);
    drop(app);
    lifecycle::flush_telemetry().await;
    Ok(())
}
