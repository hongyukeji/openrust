//! 真实MySQL：两个until事务入口必须传同一事务，并按到达位置排队或短路。
use super::QueueMigrator;
use futures_util::FutureExt as _;
use openrust::{
    Error, Result,
    database::{
        DatabaseServiceProvider, DatabaseTransaction, Db,
        sea_orm::{ConnectionTrait, Statement},
    },
    events::{Event, EventContext, Events, EventsServiceProvider},
    foundation::{ProviderBuilder, ProviderRuntime},
    queue::{Job, JobContext, Queue, QueueServiceProvider},
    support::{BoxFuture, DataValue},
    testing::TestDatabase,
};
use std::{
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const REACHED_QUEUE: &str = "until-reached";
const STOPPED_QUEUE: &str = "until-stopped";

struct Reached {
    transaction: usize,
    jobs_table: String,
    expected_jobs: i64,
}
impl Event for Reached {
    const NAME: &'static str = "Tests\\Events\\UntilReached";
}
struct Stopped {
    transaction: usize,
}
impl Event for Stopped {
    const NAME: &'static str = "Tests\\Events\\UntilStopped";
}
#[derive(serde::Serialize, serde::Deserialize)]
struct UntilJob;
impl Job for UntilJob {
    const NAME: &'static str = "tests.events.until.v1";
    fn handle(&self, _: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
#[derive(Default)]
struct Observed {
    reached_mapper: AtomicUsize,
    reached_reply: AtomicUsize,
    stopped_mapper: AtomicUsize,
    stopped_reply: AtomicUsize,
    tail: AtomicUsize,
}
fn fail(code: &str) -> Error {
    Error::custom(code, "until事务回归边界失败")
}
fn require_transaction(context: EventContext<'_>, expected: usize) -> Result<&DatabaseTransaction> {
    let transaction = context
        .transaction()
        .ok_or_else(|| fail("until_transaction_missing"))?;
    assert_eq!(std::ptr::from_ref(transaction).addr(), expected);
    Ok(transaction)
}
async fn reached_rows(transaction: &DatabaseTransaction, table: &str) -> Result<i64> {
    let row = transaction
        .query_one_raw(Statement::from_sql_and_values(
            transaction.get_database_backend(),
            format!("SELECT CAST(COUNT(*) AS SIGNED) AS total FROM {table} WHERE queue = ?"),
            [REACHED_QUEUE.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| fail("until_count_missing"))?;
    row.try_get("", "total").map_err(Error::internal)
}
fn events_provider(observed: &Arc<Observed>) -> EventsServiceProvider {
    let mut provider = EventsServiceProvider::new();
    let seen = Arc::clone(observed);
    provider.listen_queued::<Reached, UntilJob, _>(REACHED_QUEUE, move |_| {
        seen.reached_mapper.fetch_add(1, Ordering::SeqCst);
        Ok(UntilJob)
    });
    let seen = Arc::clone(observed);
    provider.listen_reply::<Reached, _>(move |event, context| {
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            let transaction = require_transaction(context, event.transaction)?;
            assert_eq!(context.event_name(), Reached::NAME);
            assert_eq!(
                reached_rows(transaction, &event.jobs_table).await?,
                event.expected_jobs
            );
            seen.reached_reply.fetch_add(1, Ordering::SeqCst);
            Ok(DataValue::Number(0.into()))
        })
    });
    let seen = Arc::clone(observed);
    provider.listen::<Reached, _>(move |_, _| {
        seen.tail.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(fail("until_reached_tail")) })
    });
    let seen = Arc::clone(observed);
    provider.listen_reply::<Stopped, _>(move |event, context| {
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            require_transaction(context, event.transaction)?;
            seen.stopped_reply.fetch_add(1, Ordering::SeqCst);
            Ok(DataValue::Bool(false))
        })
    });
    let seen = Arc::clone(observed);
    provider.listen_queued::<Stopped, UntilJob, _>(STOPPED_QUEUE, move |_| {
        seen.stopped_mapper.fetch_add(1, Ordering::SeqCst);
        Err(fail("until_stopped_mapper_reached"))
    });
    provider
}
fn assert_zero(response: &DataValue) {
    assert!(matches!(response, DataValue::Number(number) if number.as_i64() == Some(0)));
}
async fn rollback_cases(db: &Db, events: &Events) -> Result<()> {
    let events = events.clone();
    let jobs_table = db.quoted_table_name("jobs")?;
    let marker = db.quoted_table_name("until_marks")?;
    let result: Result<()> = db
        .transaction(move |transaction| {
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!("INSERT INTO {marker} (id) VALUES (1)"))
                    .await
                    .map_err(Error::internal)?;
                let pointer = std::ptr::from_ref(transaction).addr();
                let first = Reached {
                    transaction: pointer,
                    jobs_table: jobs_table.clone(),
                    expected_jobs: 1,
                };
                assert_zero(&events.until_in_transaction(transaction, &first).await?);
                let second = Reached {
                    transaction: pointer,
                    jobs_table,
                    expected_jobs: 2,
                };
                assert_zero(
                    &events
                        .until_named_in_transaction(transaction, Reached::NAME, &second)
                        .await?,
                );
                let stopped = Stopped {
                    transaction: pointer,
                };
                let typed = events.until_in_transaction(transaction, &stopped).await?;
                let named = events
                    .until_named_in_transaction(transaction, Stopped::NAME, &stopped)
                    .await?;
                assert!(matches!(typed, DataValue::Bool(false)));
                assert!(matches!(named, DataValue::Bool(false)));
                Err(fail("until_expected_rollback"))
            })
        })
        .await;
    assert_eq!(
        result.err().map(|error| error.code().to_owned()).as_deref(),
        Some("until_expected_rollback")
    );
    assert_eq!(db.table("until_marks")?.count().await?, 0);
    assert_eq!(db.table("jobs")?.count().await?, 0);
    Ok(())
}

fn finish(stage: &str, result: std::thread::Result<Result<()>>, cleanup: Result<()>) -> Result<()> {
    // 两层清理各自先await；保主失败，同时单独记录每个清理阶段的稳定码。
    if let Err(error) = &cleanup {
        eprintln!(
            "events_until_cleanup_failed stage={stage} code={}",
            error.code()
        );
    }
    match result {
        Ok(Err(primary)) => Err(primary),
        Ok(Ok(())) => cleanup,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

async fn exercise(database: &TestDatabase) -> Result<()> {
    let observed = Arc::new(Observed::default());
    let queue = QueueServiceProvider::new();
    queue.contributions().register_job::<UntilJob>()?;
    let mut builder = ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(queue)
        .provider(events_provider(&observed));
    let runtime: ProviderRuntime = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(
                serde_json::json!({"database":{"url":database.url()}}),
            ),
        ))
        .await?;
    let result = AssertUnwindSafe(async {
        let db = runtime.container().resolve::<Db>().await?;
        let events = runtime.container().resolve::<Events>().await?;
        let table = db.quoted_table_name("until_marks")?;
        db.connection()?
            .execute_unprepared(&format!(
                "CREATE TABLE {table} (id BIGINT PRIMARY KEY) ENGINE=InnoDB"
            ))
            .await
            .map_err(Error::internal)?;
        rollback_cases(&db, &events).await?;
        assert_eq!(observed.reached_mapper.load(Ordering::SeqCst), 2);
        assert_eq!(observed.reached_reply.load(Ordering::SeqCst), 2);
        assert_eq!(observed.stopped_mapper.load(Ordering::SeqCst), 0);
        assert_eq!(observed.stopped_reply.load(Ordering::SeqCst), 2);
        assert_eq!(observed.tail.load(Ordering::SeqCst), 0);
        let queue = runtime.container().resolve::<Queue>().await?;
        assert!(!queue.run_next(REACHED_QUEUE).await?);
        assert!(!queue.run_next(STOPPED_QUEUE).await?);
        Ok(())
    })
    .catch_unwind()
    .await;
    finish(
        "runtime_shutdown",
        result,
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await,
    )
}
#[tokio::test]
#[ignore = "需要真实MySQL8.4；日常事务队列专跑必须显式执行"]
async fn until_typed_and_named_share_transaction_and_queue_reachability() -> Result<()> {
    let admin = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    if !admin.starts_with("mysql://") {
        return Err(fail("until_mysql_required"));
    }
    let database = TestDatabase::create::<QueueMigrator>(&admin).await?;
    let result = AssertUnwindSafe(exercise(&database)).catch_unwind().await;
    finish("test_database_close", result, database.close().await)
}
