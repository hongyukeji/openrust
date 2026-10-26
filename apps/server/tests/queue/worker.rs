//! 验证真实数据库队列派发、worker 领取、失败和重试。
#![cfg(all(feature = "queue", any(feature = "db-postgres", feature = "db-mysql")))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::config::FrameworkConfig;
use openrust::core::{AppBuilder, BoxFuture, Error, Result as FrameworkResult, lifecycle};
use openrust::database::sea_orm_migration::MigrationTrait;
use openrust::database::{DatabasePlugin, Db, sea_orm_migration::MigratorTrait};
use openrust::events::{Events, EventsPlugin};
use openrust::queue::{
    DispatchOptions, GroupFailurePolicy, Job, JobContext, Queue, QueueMigration, QueuePlugin,
    QueueTask,
};
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};
use std::time::Duration;

static SUCCEEDED: AtomicUsize = AtomicUsize::new(0);
static FAILED: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_BATCH: AtomicUsize = AtomicUsize::new(0);
static PEAK_BATCH: AtomicUsize = AtomicUsize::new(0);
static QUEUED_EVENT_HANDLED: AtomicUsize = AtomicUsize::new(0);
static LAST_QUEUED_USER_ID: AtomicI64 = AtomicI64::new(0);
static EVENT_TRANSACTION_CONTEXTS: AtomicUsize = AtomicUsize::new(0);
static EVENT_NO_TRANSACTION_CONTEXTS: AtomicUsize = AtomicUsize::new(0);
static ORDERED: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

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
            let active = ACTIVE_BATCH.fetch_add(1, Ordering::SeqCst) + 1;
            PEAK_BATCH.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            ACTIVE_BATCH.fetch_sub(1, Ordering::SeqCst);
            SUCCEEDED.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[derive(serde::Deserialize)]
struct OrderedJob {
    value: usize,
}

impl Job for OrderedJob {
    const NAME: &'static str = "tests.ordered.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        let value = self.value;
        Box::pin(async move {
            match ORDERED.lock() {
                Ok(mut order) => {
                    order.push(value);
                    Ok(())
                }
                Err(_) => Err(Error::custom(
                    "test_order_lock",
                    "test order lock is poisoned",
                )),
            }
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

#[derive(serde::Deserialize, serde::Serialize)]
struct SignupNotificationJob {
    user_id: i64,
}

impl Job for SignupNotificationJob {
    const NAME: &'static str = "tests.signup_notification.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, FrameworkResult<()>> {
        Box::pin(async move {
            LAST_QUEUED_USER_ID.store(self.user_id, Ordering::SeqCst);
            QUEUED_EVENT_HANDLED.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

struct SignupEvent {
    user_id: i64,
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // The assertions share one isolated real database queue.
async fn database_queue_dispatch_worker_concurrency_failure_and_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database queue: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let test_database = testing::TestDatabase::create::<QueueMigrator>(&admin_url).await?;
    let mut queue_plugin = QueuePlugin::default();
    queue_plugin.register_job::<CountJob>()?;
    queue_plugin.register_job::<AlwaysFailJob>()?;
    queue_plugin.register_job::<SlowJob>()?;
    queue_plugin.register_job::<OrderedJob>()?;
    queue_plugin.register_job::<SignupNotificationJob>()?;
    let event_queue = format!("event{}", std::process::id());
    let mut events_plugin = EventsPlugin::new();
    events_plugin.listen::<SignupEvent, _>(|_, context| {
        Box::pin(async move {
            if context.transaction().is_some() {
                EVENT_TRANSACTION_CONTEXTS.fetch_add(1, Ordering::SeqCst);
            } else {
                EVENT_NO_TRANSACTION_CONTEXTS.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        })
    });
    events_plugin.listen_queued::<SignupEvent, SignupNotificationJob, _>(
        event_queue.clone(),
        |event| {
            Ok(SignupNotificationJob {
                user_id: event.user_id,
            })
        },
    );
    let mut builder = AppBuilder::new(
        FrameworkConfig::default(),
        Figment::new().merge(Toml::string(&format!(
            "[database]\nurl = {:?}\n[queue]\njob_timeout_secs = 1\n",
            test_database.url()
        ))),
    );
    builder
        .plugin(DatabasePlugin)
        .plugin(queue_plugin)
        .plugin(events_plugin);
    let app = lifecycle::finalize(builder).await?;
    let queue = app.state().try_get::<Queue>()?;
    let db = app.state().try_get::<Db>()?;
    let events = app.state().try_get::<Events>()?;

    QUEUED_EVENT_HANDLED.store(0, Ordering::SeqCst);
    LAST_QUEUED_USER_ID.store(0, Ordering::SeqCst);
    EVENT_TRANSACTION_CONTEXTS.store(0, Ordering::SeqCst);
    EVENT_NO_TRANSACTION_CONTEXTS.store(0, Ordering::SeqCst);
    let rolled_back = db
        .transaction(|transaction| {
            let events = Arc::clone(&events);
            Box::pin(async move {
                events
                    .dispatch_in_transaction(transaction, &SignupEvent { user_id: 41 })
                    .await?;
                Err::<(), _>(Error::custom("rollback_probe", "rollback event dispatch"))
            })
        })
        .await;
    assert!(rolled_back.is_err());
    assert_eq!(EVENT_TRANSACTION_CONTEXTS.load(Ordering::SeqCst), 1);
    assert!(!queue.run_next(&event_queue).await?);
    assert_eq!(QUEUED_EVENT_HANDLED.load(Ordering::SeqCst), 0);

    db.transaction(|transaction| {
        let events = Arc::clone(&events);
        Box::pin(async move {
            events
                .dispatch_in_transaction(transaction, &SignupEvent { user_id: 42 })
                .await
        })
    })
    .await?;
    assert_eq!(EVENT_TRANSACTION_CONTEXTS.load(Ordering::SeqCst), 2);
    assert!(queue.run_next(&event_queue).await?);
    assert_eq!(QUEUED_EVENT_HANDLED.load(Ordering::SeqCst), 1);
    assert_eq!(LAST_QUEUED_USER_ID.load(Ordering::SeqCst), 42);

    events.dispatch(&SignupEvent { user_id: 43 }).await?;
    assert_eq!(EVENT_NO_TRANSACTION_CONTEXTS.load(Ordering::SeqCst), 1);
    assert!(queue.run_next(&event_queue).await?);
    assert_eq!(QUEUED_EVENT_HANDLED.load(Ordering::SeqCst), 2);
    assert_eq!(LAST_QUEUED_USER_ID.load(Ordering::SeqCst), 43);

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

    let unique_queue = format!("unique{}", std::process::id());
    let unique_payload_one = serde_json::json!({"value": 1});
    let unique_payload_two = serde_json::json!({"value": 2});
    let first = queue.dispatch_unique(
        &unique_queue,
        CountJob::NAME,
        "account-42:daily-report",
        &unique_payload_one,
    );
    let second = queue.dispatch_unique(
        &unique_queue,
        CountJob::NAME,
        "account-42:daily-report",
        &unique_payload_two,
    );
    let (first, second) = tokio::try_join!(first, second)?;
    assert_eq!(first, second);
    assert!(queue.run_next(&unique_queue).await?);
    let after_completion = queue
        .dispatch_unique(
            &unique_queue,
            CountJob::NAME,
            "account-42:daily-report",
            &serde_json::json!({"value": 3}),
        )
        .await?;
    assert_ne!(first, after_completion);

    let transactional_queue = format!("unique-tx{}", std::process::id());
    let tx_queue = transactional_queue.clone();
    let tx_dispatcher = queue.clone();
    let transaction_ids = db
        .transaction(|transaction| {
            Box::pin(async move {
                let first = tx_dispatcher
                    .dispatch_unique_in_transaction(
                        transaction,
                        &tx_queue,
                        CountJob::NAME,
                        "account-42:signup-mail",
                        &serde_json::json!({"value": 1}),
                    )
                    .await?;
                let second = tx_dispatcher
                    .dispatch_unique_in_transaction(
                        transaction,
                        &tx_queue,
                        CountJob::NAME,
                        "account-42:signup-mail",
                        &serde_json::json!({"value": 2}),
                    )
                    .await?;
                Ok((first, second))
            })
        })
        .await?;
    assert_eq!(transaction_ids.0, transaction_ids.1);

    let chain_queue = format!("chain{}", std::process::id());
    ORDERED.lock()?.clear();
    let chain = [1, 2, 3]
        .into_iter()
        .map(|value| QueueTask::new(OrderedJob::NAME, &serde_json::json!({"value": value})))
        .collect::<FrameworkResult<Vec<_>>>()?;
    queue
        .dispatch_chain(&chain_queue, chain, GroupFailurePolicy::Stop)
        .await?;
    assert!(queue.run_next(&chain_queue).await?);
    assert_eq!(*ORDERED.lock()?, vec![1, 2, 3]);

    ORDERED.lock()?.clear();
    let transactional_chain_queue = format!("chain-tx{}", std::process::id());
    let tx_chain_queue = transactional_chain_queue.clone();
    let tx_chain_dispatcher = queue.clone();
    db.transaction(|transaction| {
        Box::pin(async move {
            let task = QueueTask::new(OrderedJob::NAME, &serde_json::json!({"value": 4}))?;
            tx_chain_dispatcher
                .dispatch_chain_in_transaction(
                    transaction,
                    &tx_chain_queue,
                    vec![task],
                    GroupFailurePolicy::Stop,
                )
                .await?;
            Ok(())
        })
    })
    .await?;
    assert!(queue.run_next(&transactional_chain_queue).await?);
    assert_eq!(*ORDERED.lock()?, vec![4]);

    ORDERED.lock()?.clear();
    let continue_chain = vec![
        QueueTask::new(AlwaysFailJob::NAME, &())?,
        QueueTask::new(OrderedJob::NAME, &serde_json::json!({"value": 7}))?,
    ];
    let continue_queue = format!("continue-chain{}", std::process::id());
    queue
        .dispatch_chain(
            &continue_queue,
            continue_chain,
            GroupFailurePolicy::Continue,
        )
        .await?;
    assert!(queue.run_next(&continue_queue).await?);
    assert_eq!(*ORDERED.lock()?, vec![7]);

    ORDERED.lock()?.clear();
    let stop_chain = vec![
        QueueTask::new(AlwaysFailJob::NAME, &())?,
        QueueTask::new(OrderedJob::NAME, &serde_json::json!({"value": 8}))?,
    ];
    let stop_queue = format!("stop-chain{}", std::process::id());
    queue
        .dispatch_chain(&stop_queue, stop_chain, GroupFailurePolicy::Stop)
        .await?;
    assert!(queue.run_next(&stop_queue).await?);
    assert!(ORDERED.lock()?.is_empty());

    assert!(
        queue
            .dispatch_chain(&chain_queue, Vec::new(), GroupFailurePolicy::Stop)
            .await
            .is_err()
    );
    let validation_task = QueueTask::new(OrderedJob::NAME, &serde_json::json!({"value": 9}))?;
    assert!(
        queue
            .dispatch_batch(
                &chain_queue,
                vec![validation_task],
                0,
                GroupFailurePolicy::Stop,
            )
            .await
            .is_err()
    );

    let continue_batch_queue = format!("continue-batch{}", std::process::id());
    let succeeded_before = SUCCEEDED.load(Ordering::SeqCst);
    let continue_batch = vec![
        QueueTask::new(AlwaysFailJob::NAME, &())?,
        QueueTask::new(CountJob::NAME, &serde_json::json!({"value": 1}))?,
        QueueTask::new(CountJob::NAME, &serde_json::json!({"value": 2}))?,
    ];
    queue
        .dispatch_batch(
            &continue_batch_queue,
            continue_batch,
            1,
            GroupFailurePolicy::Continue,
        )
        .await?;
    assert!(queue.run_next(&continue_batch_queue).await?);
    assert_eq!(SUCCEEDED.load(Ordering::SeqCst), succeeded_before + 2);

    let stop_batch_queue = format!("stop-batch{}", std::process::id());
    let succeeded_before = SUCCEEDED.load(Ordering::SeqCst);
    let stop_batch = vec![
        QueueTask::new(AlwaysFailJob::NAME, &())?,
        QueueTask::new(CountJob::NAME, &serde_json::json!({"value": 3}))?,
    ];
    queue
        .dispatch_batch(&stop_batch_queue, stop_batch, 1, GroupFailurePolicy::Stop)
        .await?;
    assert!(queue.run_next(&stop_batch_queue).await?);
    assert_eq!(SUCCEEDED.load(Ordering::SeqCst), succeeded_before);

    let batch_queue = format!("batch{}", std::process::id());
    let batch = (0..6)
        .map(|value| QueueTask::new(CountJob::NAME, &serde_json::json!({"value": value})))
        .collect::<FrameworkResult<Vec<_>>>()?;
    PEAK_BATCH.store(0, Ordering::SeqCst);
    queue
        .dispatch_batch(&batch_queue, batch, 2, GroupFailurePolicy::Continue)
        .await?;
    assert!(queue.run_next(&batch_queue).await?);
    assert_eq!(PEAK_BATCH.load(Ordering::SeqCst), 2);

    let timeout_queue = format!("timeout{}", std::process::id());
    queue
        .dispatch_with_options(
            &timeout_queue,
            SlowJob::NAME,
            &serde_json::json!({}),
            DispatchOptions {
                max_attempts: 1,
                delay: Duration::ZERO,
                unique_key: None,
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
                unique_key: None,
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

    drop(events);
    drop(queue);
    drop(db);
    drop(app);
    test_database.close().await?;
    Ok(())
}
