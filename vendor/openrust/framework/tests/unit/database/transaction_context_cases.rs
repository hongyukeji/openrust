//! 独立外包真实MySQL回归；无环境/错误后端必须失败，不把skip当通过。
use super::framework::{
    Error, Result,
    database::{
        DatabaseServiceProvider, DatabaseTransaction, Db, TransactionBeforeCommit,
        TransactionCommitted,
        sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    events::EventsServiceProvider,
    foundation::ProviderBuilder,
    testing::TestDatabase,
};
use figment::{Figment, providers::Serialized};
use futures_util::FutureExt as _;
use std::{
    cell::Cell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

struct EmptyMigrations;
impl MigratorTrait for EmptyMigrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
fn fail(code: &'static str) -> Error {
    Error::custom(code, "事务context测试拒绝")
}
fn finish(outcome: std::thread::Result<Result<()>>, closed: Result<()>) -> Result<()> {
    match outcome {
        Ok(result) => {
            if result.is_err()
                && let Err(error) = &closed
            {
                eprintln!("context cleanup failed: {}", error.code());
            }
            result.and(closed)
        }
        Err(payload) => {
            if let Err(error) = closed {
                eprintln!("context cleanup failed: {}", error.code());
            }
            std::panic::resume_unwind(payload)
        }
    }
}
async fn insert(tx: &DatabaseTransaction, lane: i64, stage: i64, value: i64) -> Result<()> {
    tx.execute_raw(Statement::from_sql_and_values(
        DbBackend::MySql,
        "INSERT INTO context_marks(lane,stage,mark_value) VALUES(?,?,?)",
        [lane.into(), stage.into(), value.into()],
    ))
    .await
    .map_err(Error::internal)?;
    Ok(())
}
async fn rows(connection: &impl ConnectionTrait, lane: i64) -> Result<Vec<i64>> {
    connection
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::MySql,
            "SELECT stage FROM context_marks WHERE lane=? ORDER BY stage",
            [lane.into()],
        ))
        .await
        .map_err(Error::internal)?
        .into_iter()
        .map(|row| row.try_get("", "stage").map_err(Error::internal))
        .collect()
}
fn mark(trace: &Mutex<Vec<&'static str>>, stage: &'static str) -> Result<()> {
    trace
        .lock()
        .map_err(|_| fail("trace_poisoned"))?
        .push(stage);
    Ok(())
}
fn trace(trace: &Mutex<Vec<&'static str>>) -> Result<Vec<&'static str>> {
    Ok(trace.lock().map_err(|_| fail("trace_poisoned"))?.clone())
}
fn clear_trace(trace: &Mutex<Vec<&'static str>>) -> Result<()> {
    trace.lock().map_err(|_| fail("trace_poisoned"))?.clear();
    Ok(())
}
#[derive(Default)]
struct Observers {
    reject_listener: AtomicBool,
    committed: AtomicUsize,
    raw: AtomicUsize,
    trace: Mutex<Vec<&'static str>>,
}
fn events(observers: Arc<Observers>) -> EventsServiceProvider {
    let mut events = EventsServiceProvider::new();
    let before = Arc::clone(&observers);
    events.listen::<TransactionBeforeCommit, _>(move |_, event| {
        let before = Arc::clone(&before);
        Box::pin(async move {
            let tx = event
                .transaction()
                .ok_or_else(|| fail("listener_missing_transaction"))?;
            assert_eq!(
                std::ptr::from_ref(tx).addr(),
                before.raw.load(Ordering::SeqCst)
            );
            assert_eq!(rows(tx, 1).await?, [1]);
            mark(&before.trace, "listener")?;
            insert(tx, 1, 2, 8).await?;
            if before.reject_listener.load(Ordering::SeqCst) {
                return Err(fail("listener_rejected"));
            }
            Ok(())
        })
    });
    events.listen::<TransactionCommitted, _>(move |_, event| {
        let observers = Arc::clone(&observers);
        Box::pin(async move {
            assert!(event.transaction().is_none());
            mark(&observers.trace, "committed")?;
            observers.committed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    events
}
async fn serial_case(db: &Db, observers: Arc<Observers>, reject_check: bool) -> Result<Cell<u8>> {
    let source = db.clone();
    let final_source = db.clone();
    let work = Arc::clone(&observers);
    db.transaction_for_update_checked_context(
        move |context| {
            Box::pin(async move {
                assert!(context.is_from(&source));
                work.raw.store(
                    std::ptr::from_ref(context.transaction()).addr(),
                    Ordering::SeqCst,
                );
                mark(&work.trace, "work")?;
                insert(context.transaction(), 1, 1, 7).await?;
                Ok(Cell::new(7))
            })
        },
        move |context| {
            Box::pin(async move {
                assert!(context.is_from(&final_source));
                assert_eq!(
                    std::ptr::from_ref(context.transaction()).addr(),
                    observers.raw.load(Ordering::SeqCst)
                );
                assert_eq!(rows(context.transaction(), 1).await?, [1, 2]);
                mark(&observers.trace, "check")?;
                insert(context.transaction(), 1, 3, 9).await?;
                if reject_check {
                    return Err(fail("check_rejected"));
                }
                Ok(())
            })
        },
    )
    .await
}
async fn rollback_and_order(
    db: &Db,
    reader: &DatabaseConnection,
    observers: Arc<Observers>,
) -> Result<()> {
    observers.reject_listener.store(true, Ordering::SeqCst);
    let rejected = serial_case(db, Arc::clone(&observers), false).await;
    assert!(matches!(rejected, Err(error) if error.code()=="listener_rejected"));
    assert_eq!(trace(&observers.trace)?, ["work", "listener"]);
    assert_eq!(rows(reader, 1).await?, [] as [i64; 0]);
    assert_eq!(observers.committed.load(Ordering::SeqCst), 0);
    clear_trace(&observers.trace)?;
    observers.reject_listener.store(false, Ordering::SeqCst);
    let rejected = serial_case(db, Arc::clone(&observers), true).await;
    assert!(matches!(rejected, Err(error) if error.code()=="check_rejected"));
    assert_eq!(trace(&observers.trace)?, ["work", "listener", "check"]);
    assert_eq!(rows(reader, 1).await?, [] as [i64; 0]);
    assert_eq!(observers.committed.load(Ordering::SeqCst), 0);
    clear_trace(&observers.trace)?;
    assert_eq!(
        serial_case(db, Arc::clone(&observers), false).await?.get(),
        7
    );
    assert_eq!(rows(reader, 1).await?, [1, 2, 3]);
    assert_eq!(
        trace(&observers.trace)?,
        ["work", "listener", "check", "committed"]
    );
    assert_eq!(observers.committed.load(Ordering::SeqCst), 1);
    reader
        .execute_unprepared("DELETE FROM context_marks")
        .await
        .map_err(Error::internal)?;
    Ok(())
}
async fn cancelled(
    db: &Db,
    reader: &DatabaseConnection,
    observers: Arc<Observers>,
    in_check: bool,
) -> Result<()> {
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered)));
    let work_signal = Arc::clone(&entered);
    let work = Arc::clone(&observers);
    let check = Arc::clone(&observers);
    clear_trace(&observers.trace)?;
    {
        let future = db.transaction_for_update_checked_context(
            move |context| {
                Box::pin(async move {
                    work.raw.store(
                        std::ptr::from_ref(context.transaction()).addr(),
                        Ordering::SeqCst,
                    );
                    insert(context.transaction(), 1, 1, 7).await?;
                    if !in_check {
                        signal(&work_signal)?;
                        return std::future::pending::<Result<()>>().await;
                    }
                    Ok(())
                })
            },
            move |context| {
                Box::pin(async move {
                    assert_eq!(
                        std::ptr::from_ref(context.transaction()).addr(),
                        check.raw.load(Ordering::SeqCst)
                    );
                    assert_eq!(rows(context.transaction(), 1).await?, [1, 2]);
                    insert(context.transaction(), 1, 3, 9).await?;
                    signal(&entered)?;
                    std::future::pending::<Result<()>>().await
                })
            },
        );
        tokio::pin!(future);
        tokio::select! {
            result=&mut future => { result?; return Err(fail("cancelled_future_returned")); },
            result=waiting => result.map_err(Error::internal)?,
        }
    }
    // 同PK写冲突屏障：必须等旧交易释放锁；单独SELECT不可见不能证明rollback。
    tokio::time::timeout(Duration::from_secs(5), async {
        let barrier = reader.begin().await.map_err(Error::internal)?;
        insert(&barrier, 1, 1, 99).await?;
        barrier.rollback().await.map_err(Error::internal)
    })
    .await
    .map_err(Error::internal)??;
    assert_eq!(rows(reader, 1).await?, [] as [i64; 0]);
    assert_eq!(observers.committed.load(Ordering::SeqCst), 1);
    Ok(())
}
fn signal(sender: &Mutex<Option<tokio::sync::oneshot::Sender<()>>>) -> Result<()> {
    sender
        .lock()
        .map_err(|_| fail("signal_poisoned"))?
        .take()
        .ok_or_else(|| fail("signal_repeated"))?
        .send(())
        .map_err(|()| fail("signal_receiver_gone"))
}
// 这是消费方显式SQL不变量；is_from只证明issuer，不能替代这项每次交易的读取。
async fn require_lane(tx: &DatabaseTransaction, lane: i64) -> Result<()> {
    if rows(tx, lane).await? != [1] {
        return Err(fail("transaction_lane_mismatch"));
    }
    Ok(())
}
async fn concurrent_lane(
    db: &Db,
    barrier: Arc<tokio::sync::Barrier>,
    addresses: Arc<[AtomicUsize; 2]>,
    lane: usize,
) -> Result<()> {
    let source = db.clone();
    let final_source = db.clone();
    let work_barrier = Arc::clone(&barrier);
    let work_addresses = Arc::clone(&addresses);
    let own = i64::try_from(lane + 10).map_err(Error::internal)?;
    db.transaction_for_update_checked_context(
        move |context| {
            Box::pin(async move {
                assert!(context.is_from(&source));
                work_addresses[lane].store(
                    std::ptr::from_ref(context.transaction()).addr(),
                    Ordering::SeqCst,
                );
                insert(context.transaction(), own, 1, own).await?;
                work_barrier.wait().await;
                assert_ne!(
                    work_addresses[0].load(Ordering::SeqCst),
                    work_addresses[1].load(Ordering::SeqCst)
                );
                Ok(())
            })
        },
        move |context| {
            Box::pin(async move {
                assert!(context.is_from(&final_source));
                assert_eq!(
                    std::ptr::from_ref(context.transaction()).addr(),
                    addresses[lane].load(Ordering::SeqCst)
                );
                require_lane(context.transaction(), own).await?;
                let other = if own == 10 { 11 } else { 10 };
                assert!(matches!(require_lane(context.transaction(), other).await, Err(error) if error.code() == "transaction_lane_mismatch"));
                assert_eq!(
                    rows(context.transaction(), if own == 10 { 11 } else { 10 }).await?,
                    [] as [i64; 0]
                );
                barrier.wait().await;
                Ok(())
            })
        },
    )
    .await
}
async fn source_and_concurrency(db: &Db) -> Result<()> {
    // 真正包装同一pool，但新的Db issuer身份不同，未私自安装Events。
    let alias = Db::default();
    alias.initialize(db.primary_connection()?)?;
    let origin = alias.clone();
    let other = db.clone();
    let final_origin = alias.clone();
    alias
        .transaction_for_update_checked_context(
            move |context| {
                Box::pin(async move {
                    assert!(context.is_from(&origin));
                    assert!(!context.is_from(&other));
                    Ok(())
                })
            },
            move |context| {
                Box::pin(async move {
                    assert!(context.is_from(&final_origin));
                    Ok(())
                })
            },
        )
        .await?;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let addresses = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    tokio::time::timeout(Duration::from_secs(5), async {
        let (a, b) = tokio::join!(
            concurrent_lane(&alias, Arc::clone(&barrier), Arc::clone(&addresses), 0),
            concurrent_lane(&alias, barrier, addresses, 1)
        );
        a?;
        b
    })
    .await
    .map_err(Error::internal)??;
    Ok(())
}
async fn exercise(database: &TestDatabase) -> Result<()> {
    let observers = Arc::new(Observers::default());
    let mut builder = ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(events(Arc::clone(&observers)))
        .provider(DatabaseServiceProvider);
    let runtime = builder
        .start(Figment::from(Serialized::defaults(serde_json::json!({
            "database":{"url":database.url(),"min_connections":1,"max_connections":8}
        }))))
        .await?;
    let result=std::panic::AssertUnwindSafe(async {
        let db=runtime.container().resolve::<Db>().await?;
        let reader=database.connection()?;
        reader.execute_unprepared("CREATE TABLE context_marks(lane BIGINT NOT NULL,stage BIGINT NOT NULL,mark_value BIGINT NOT NULL,PRIMARY KEY(lane,stage)) ENGINE=InnoDB").await.map_err(Error::internal)?;
        rollback_and_order(&db,&reader,Arc::clone(&observers)).await?;
        cancelled(&db,&reader,Arc::clone(&observers),false).await?;
        cancelled(&db,&reader,Arc::clone(&observers),true).await?;
        source_and_concurrency(&db).await
    }).catch_unwind().await;
    let closed = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
        .await;
    finish(result, closed)
}
pub(super) async fn run_contract(admin: &str) -> Result<()> {
    if !admin.starts_with("mysql://") {
        return Err(fail("mysql_required"));
    }
    let database = TestDatabase::create::<EmptyMigrations>(admin).await?;
    let result = std::panic::AssertUnwindSafe(exercise(&database))
        .catch_unwind()
        .await;
    finish(result, database.close().await)
}
