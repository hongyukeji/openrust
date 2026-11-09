//! 真实 SQLite 事务在提交前监听器结束后做末位校验；拒绝与取消均回滚事务内写入。

use super::{
    ConnectionTrait, Database, DatabaseConnection, DatabaseTransaction, Db, DbBackend, Statement,
    TransactionBeforeCommit, TransactionCommitted, begin_for_update,
};
use crate::Error;
use crate::Result;
use crate::events::Dispatcher;
use crate::events::EventsServiceProvider;
use crate::foundation::ProviderBuilder;
use crate::support::BoxFuture;
use crate::testing::TestDatabase;
use sea_orm_migration::{MigrationTrait, MigratorTrait};
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    },
    time::Duration,
};

struct EmptyMigrations;
impl MigratorTrait for EmptyMigrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

// 返回值可跨 await 移动，但不提供 Sync 或 Clone。
struct OnlySend(Cell<u8>);

async fn insert(connection: &DatabaseTransaction, mark: i64) -> Result<()> {
    connection
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO marks (stage) VALUES (?)",
            [mark.into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

async fn stages(connection: &impl ConnectionTrait) -> Result<Vec<i64>> {
    connection
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT stage FROM marks ORDER BY rowid",
        ))
        .await
        .map_err(Error::internal)?
        .into_iter()
        .map(|row| row.try_get("", "stage").map_err(Error::internal))
        .collect()
}

fn write_business(tx: &DatabaseTransaction) -> BoxFuture<'_, Result<OnlySend>> {
    Box::pin(async move {
        insert(tx, 1).await?;
        Ok(OnlySend(Cell::new(7)))
    })
}

async fn succeed_or_reject(
    db: &Db,
    observer: &DatabaseConnection,
    after: &AtomicUsize,
    last_visible: &AtomicI64,
) -> Result<()> {
    let rejected = db
        .transaction_for_update_checked(write_business, |tx| {
            Box::pin(async move {
                assert_eq!(stages(tx).await?, [1, 2]);
                insert(tx, 3).await?;
                Err(Error::Unauthorized)
            })
        })
        .await;
    assert!(matches!(rejected, Err(Error::Unauthorized)));
    assert_eq!(stages(observer).await?, [] as [i64; 0]);
    assert_eq!(after.load(Ordering::SeqCst), 0);

    let value = db
        .transaction_for_update_checked(write_business, |tx| {
            Box::pin(async move {
                assert_eq!(stages(tx).await?, [1, 2]);
                insert(tx, 3).await
            })
        })
        .await?;
    assert_eq!(value.0.get(), 7);
    assert_eq!(stages(observer).await?, [1, 2, 3]);
    assert_eq!(after.load(Ordering::SeqCst), 1);
    assert_eq!(last_visible.load(Ordering::SeqCst), 3);

    observer
        .execute_unprepared("DELETE FROM marks")
        .await
        .map_err(Error::internal)?;
    let value = db.transaction(write_business).await?;
    assert_eq!(value.0.get(), 7);
    assert_eq!(stages(observer).await?, [1, 2]);
    assert_eq!(after.load(Ordering::SeqCst), 2);
    assert_eq!(last_visible.load(Ordering::SeqCst), 2);
    observer
        .execute_unprepared("DELETE FROM marks")
        .await
        .map_err(Error::internal)?;
    Ok(())
}

async fn cancel_while_checking(
    db: &Db,
    observer: &DatabaseConnection,
    after: &AtomicUsize,
) -> Result<()> {
    let (reached, waiting) = tokio::sync::oneshot::channel();
    {
        let transaction = db.transaction_for_update_checked(write_business, move |tx| {
            Box::pin(async move {
                assert_eq!(stages(tx).await?, [1, 2]);
                insert(tx, 3).await?;
                reached
                    .send(())
                    .map_err(|()| Error::custom("final_check_observer_gone", "取消观察端已关闭"))?;
                std::future::pending::<Result<()>>().await
            })
        });
        tokio::pin!(transaction);
        tokio::select! {
            result = &mut transaction => {
                result?;
                return Err(Error::custom("final_check_returned", "末位校验意外提前结束"));
            }
            result = waiting => result.map_err(Error::internal)?,
        }
        // 此处释放实际处于末位校验中的事务 future。
    }
    // 独立连接拿到写锁之后，原写事务必已结束，读不到暂时的未提交结果不算回滚证据。
    let barrier = begin_for_update(observer).await?;
    let persisted = stages(&barrier).await?;
    barrier.rollback().await.map_err(Error::internal)?;
    assert_eq!(persisted, [] as [i64; 0]);
    assert_eq!(after.load(Ordering::SeqCst), 2);
    Ok(())
}

async fn exercise(database: &TestDatabase, observer: &DatabaseConnection) -> Result<()> {
    let db = Db::default();
    db.initialize(database.connection()?)?;
    observer
        .execute_unprepared("CREATE TABLE marks (stage INTEGER NOT NULL)")
        .await
        .map_err(Error::internal)?;
    let after = Arc::new(AtomicUsize::new(0));
    let last_visible = Arc::new(AtomicI64::new(0));
    let mut events = EventsServiceProvider::new();
    events.listen::<TransactionBeforeCommit, _>(|_, context| {
        Box::pin(async move {
            let tx = context.transaction().ok_or(Error::Unauthorized)?;
            assert_eq!(stages(tx).await?, [1]);
            insert(tx, 2).await
        })
    });
    let recorded = after.clone();
    let visible = last_visible.clone();
    let reader = observer.clone();
    events.listen::<TransactionCommitted, _>(move |_, context| {
        let recorded = recorded.clone();
        let visible = visible.clone();
        let reader = reader.clone();
        Box::pin(async move {
            assert!(context.transaction().is_none());
            recorded.fetch_add(1, Ordering::SeqCst);
            let stages = stages(&reader).await?;
            visible.store(*stages.last().ok_or(Error::Unauthorized)?, Ordering::SeqCst);
            Ok(())
        })
    });
    let mut builder = ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(events);
    let runtime = builder.start(figment::Figment::new()).await?;
    let result: Result<()> = async {
        db.install_events((*runtime.container().resolve::<Dispatcher>().await?).clone());
        succeed_or_reject(&db, observer, &after, &last_visible).await?;
        tokio::time::timeout(
            Duration::from_secs(5),
            cancel_while_checking(&db, observer, &after),
        )
        .await
        .map_err(Error::internal)?
    }
    .await;
    let stopped = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    result.and(stopped)
}

#[tokio::test]
async fn final_check_follows_listeners_and_cancellation_rolls_back_before_commit() -> Result<()> {
    let database = TestDatabase::sqlite_file::<EmptyMigrations>().await?;
    let result: Result<()> = async {
        let observer = Database::connect(database.url())
            .await
            .map_err(Error::internal)?;
        let result = exercise(&database, &observer).await;
        let closed = observer.close().await.map_err(Error::internal);
        result.and(closed)
    }
    .await;
    result.and(database.close().await)
}
