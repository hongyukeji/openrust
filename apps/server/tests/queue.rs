//! Real database queue dispatch, worker claim, failure and retry coverage.
#![cfg(all(feature = "queue", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::config::FrameworkConfig;
use openrust::core::{AppBuilder, BoxFuture, Error, Result as FrameworkResult, lifecycle};
use openrust::database::sea_orm_migration::MigrationTrait;
use openrust::database::{DatabasePlugin, Db, sea_orm_migration::MigratorTrait};
use openrust::queue::{DispatchOptions, Job, JobContext, Queue, QueueMigration, QueuePlugin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static SUCCEEDED: AtomicUsize = AtomicUsize::new(0);
static FAILED: AtomicUsize = AtomicUsize::new(0);

struct QueueMigrator;

impl MigratorTrait for QueueMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(QueueMigration)]
    }
}

#[derive(serde::Deserialize)]
struct CountJob {
    value: usize,
}

impl Job for CountJob {
    const NAME: &'static str = "tests.count.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async move {
            if self.value > 7 {
                return Err(Error::custom(
                    "invalid_test_job",
                    "test value is out of range",
                ));
            }
            context
                .set_progress(&serde_json::json!({"done": self.value}))
                .await?;
            tokio::time::sleep(Duration::from_millis(40)).await;
            SUCCEEDED.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[derive(serde::Deserialize)]
struct AlwaysFailJob;

impl Job for AlwaysFailJob {
    const NAME: &'static str = "tests.fail.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async {
            FAILED.fetch_add(1, Ordering::SeqCst);
            Err(Error::custom("test_failure", "expected failure"))
        })
    }
}

#[derive(serde::Deserialize)]
struct SlowJob;

impl Job for SlowJob {
    const NAME: &'static str = "tests.slow.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok(())
        })
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // The assertions share one isolated real database queue.
async fn database_queue_dispatch_worker_concurrency_failure_and_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database queue: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let test_database = openrust_test::TestDatabase::create::<QueueMigrator>(&admin_url).await?;
    let mut queue_plugin = QueuePlugin::default();
    queue_plugin.register_job::<CountJob>()?;
    queue_plugin.register_job::<AlwaysFailJob>()?;
    queue_plugin.register_job::<SlowJob>()?;
    let mut builder = AppBuilder::new(
        FrameworkConfig::default(),
        Figment::new().merge(Toml::string(&format!(
            "[database]\nurl = {:?}\n[queue]\njob_timeout_secs = 1\n",
            test_database.url()
        ))),
    );
    builder.plugin(DatabasePlugin).plugin(queue_plugin);
    let app = lifecycle::finalize(builder).await?;
    let queue = app.state().try_get::<Queue>()?;
    let db = app.state().try_get::<Db>()?;

    SUCCEEDED.store(0, Ordering::SeqCst);
    let queue_name = format!("concurrent{}", std::process::id());
    for value in 0..8 {
        queue
            .dispatch(
                &queue_name,
                CountJob::NAME,
                &serde_json::json!({"value": value}),
            )
            .await?;
    }
    let mut workers = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let queue = queue.clone();
        let queue_name = queue_name.clone();
        workers.spawn(async move { queue.run_next(&queue_name).await });
    }
    while let Some(result) = workers.join_next().await {
        assert!(result??);
    }
    assert_eq!(SUCCEEDED.load(Ordering::SeqCst), 8);
    assert!(!queue.run_next(&queue_name).await?);

    let timeout_queue = format!("timeout{}", std::process::id());
    queue
        .dispatch_with_options(
            &timeout_queue,
            SlowJob::NAME,
            &serde_json::json!({}),
            DispatchOptions {
                max_attempts: 1,
                delay: Duration::ZERO,
            },
        )
        .await?;
    assert!(queue.run_next(&timeout_queue).await?);
    queue
        .dispatch(
            &timeout_queue,
            CountJob::NAME,
            &serde_json::json!({"value": 0}),
        )
        .await?;
    assert!(queue.run_next(&timeout_queue).await?);

    FAILED.store(0, Ordering::SeqCst);
    let failed_queue = format!("failed{}", std::process::id());
    let failed_id = queue
        .dispatch_with_options(
            &failed_queue,
            AlwaysFailJob::NAME,
            &(),
            DispatchOptions {
                max_attempts: 1,
                delay: Duration::ZERO,
            },
        )
        .await?;
    assert!(queue.run_next(&failed_queue).await?);
    assert_eq!(FAILED.load(Ordering::SeqCst), 1);
    let retry_id = queue.retry_failed(&failed_id).await?;
    assert_ne!(retry_id, failed_id);
    assert_eq!(retry_id, queue.retry_failed(&failed_id).await?);

    let rollback_queue = format!("rollback{}", std::process::id());
    let rollback_queue_for_tx = rollback_queue.clone();
    let rollback_dispatcher = queue.clone();
    let result = db
        .transaction(|transaction| {
            Box::pin(async move {
                rollback_dispatcher
                    .dispatch_in_transaction(
                        transaction,
                        &rollback_queue_for_tx,
                        CountJob::NAME,
                        &serde_json::json!({"value": 0}),
                    )
                    .await?;
                Err::<(), _>(Error::custom("rollback_probe", "rollback transaction"))
            })
        })
        .await;
    assert!(result.is_err());
    assert!(!queue.run_next(&rollback_queue).await?);

    let committed_queue = format!("committed{}", std::process::id());
    let committed_queue_for_tx = committed_queue.clone();
    let committed_dispatcher = queue.clone();
    db.transaction(|transaction| {
        Box::pin(async move {
            committed_dispatcher
                .dispatch_in_transaction(
                    transaction,
                    &committed_queue_for_tx,
                    CountJob::NAME,
                    &serde_json::json!({"value": 1}),
                )
                .await?;
            Ok(())
        })
    })
    .await?;
    assert!(queue.run_next(&committed_queue).await?);

    drop(queue);
    drop(db);
    drop(app);
    test_database.close().await?;
    Ok(())
}
