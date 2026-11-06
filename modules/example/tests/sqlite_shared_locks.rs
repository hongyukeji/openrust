//! article-import 命名空间的 SQLite 同机租约保护写；不启动尚未适配的 SQLite Queue。

use crate::modules::example::{
    models::article_import,
    services::article_transfer::{terminal, with_import_lease},
};
use openrust::database::sea_orm::EntityTrait;
use openrust::database::{
    CoordinationMigration, Db, LockAcquire, LockLease, SharedLocks,
    sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection},
    sea_orm_migration::{MigrationTrait, MigratorTrait},
};
use openrust::testing::TestDatabase;
use openrust::{Error, Result};
use std::{sync::Arc, time::Duration};

const OWNER: i64 = 42;
const IMPORT_ID: &str = "019b531a-1010-7000-8000-000000000001";
const TTL: Duration = Duration::from_secs(30);

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations: Vec<Box<dyn MigrationTrait>> = vec![Box::new(CoordinationMigration)];
        migrations.extend(
            crate::modules::example::database::migrations::Migrator::migrations()
                .into_iter()
                .filter(|migration| migration.name() == "m20261028_000006_example_article_imports"),
        );
        migrations
    }
}

struct Fixture {
    database: TestDatabase,
    first: Db,
    second: Db,
    second_connection: DatabaseConnection,
}
impl Fixture {
    async fn new() -> Result<Self> {
        let database = TestDatabase::sqlite_file::<Migrator>().await?;
        let setup: Result<(Db, Db, DatabaseConnection)> = async {
            let connection = database.connection()?;
            let first = Db::default();
            first.initialize(connection)?;
            first
                .table("example_article_imports")?
                .insert(vec![
                    ("import_id", IMPORT_ID.into()),
                    ("owner_id", OWNER.into()),
                    (
                        "source_key",
                        format!("example/articles/{OWNER}/imports/{IMPORT_ID}/source").into(),
                    ),
                    ("format", "csv".into()),
                    ("state", "open".into()),
                    ("created_at_ms", 0i64.into()),
                    ("source_removed", false.into()),
                ])
                .await?;
            let mut options = ConnectOptions::new(database.url().to_owned());
            options
                .max_connections(1)
                .min_connections(1)
                .sqlx_logging(false);
            let second_connection = Database::connect(options).await.map_err(Error::internal)?;
            let second = Db::default();
            second.initialize(second_connection.clone())?;
            Ok((first, second, second_connection))
        }
        .await;
        match setup {
            Ok((first, second, second_connection)) => Ok(Self {
                database,
                first,
                second,
                second_connection,
            }),
            Err(error) => {
                database.close().await?;
                Err(error)
            }
        }
    }

    async fn close(self) -> Result<()> {
        let Self {
            database,
            first,
            second,
            second_connection,
        } = self;
        // 先结束所有外部连接与借用，再由唯一 TestDatabase 回收自有文件及 sidecar。
        drop(first);
        drop(second);
        let second_result = second_connection.close().await.map_err(Error::internal);
        let database_result = database.close().await;
        second_result?;
        database_result
    }
}

fn import_lock_key() -> String {
    format!("example:article-import:{OWNER}:{IMPORT_ID}")
}

async fn acquire_import(locks: &SharedLocks) -> Result<LockLease> {
    match locks.acquire(&import_lock_key(), TTL).await? {
        LockAcquire::Acquired(lease) => Ok(lease),
        LockAcquire::Busy { .. } => Err(Error::custom(
            "example_import_lock_busy",
            "示例导入租约正在使用",
        )),
    }
}

async fn receipt(db: &Db) -> Result<article_import::Model> {
    article_import::Entity::find_by_id(IMPORT_ID)
        .one(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::NotFound("示例导入回执不存在".into()))
}

#[tokio::test]
async fn article_import_receipt_rejects_stale_owner_and_commits_only_current_lease() -> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let first = fixture.first.clone();
        let second = fixture.second.clone();
        let result: Result<()> = with_import_lease(Arc::new(first.clone()), OWNER, IMPORT_ID, move |left, old| async move {
            let right = SharedLocks::new(second.clone());
            assert!(matches!(right.acquire(&import_lock_key(), TTL).await?, LockAcquire::Busy { .. }));
            assert_eq!(first.table("shared_locks")?.update(vec![("expires_at_ms", 0i64.into())]).await?, 1);
            let current = acquire_import(&right).await?;
            assert!(current.fencing_token() > old.fencing_token());
            assert!(!left.release(&old).await?);
            assert!(!left.renew(&old, TTL).await?);
            let stale = terminal(&first, &left, &old, receipt(&first).await?, "failed", Some("stale_owner")).await;
            assert!(matches!(&stale, Err(error) if error.code() == "shared_lock_lost"));
            let unchanged = receipt(&first).await?;
            assert_eq!(unchanged.state, "open");
            assert_eq!(unchanged.error_code, None);
            assert_eq!(unchanged.terminal_at_ms, None);
            terminal(&second, &right, &current, receipt(&second).await?, "completed", None).await?;
            assert!(matches!(terminal(&first, &left, &old, receipt(&first).await?, "failed", Some("stale_owner")).await, Err(error) if error.code() == "shared_lock_lost"));
            let committed = receipt(&first).await?;
            assert_eq!(committed.state, "completed");
            assert_eq!(committed.error_code, None);
            assert!(committed.terminal_at_ms.is_some());
            assert!(right.release(&current).await?);
            // 真实 wrapper 必须保留业务原错，即使旧 owner 的 release 已失效。
            stale
        }).await;
        assert!(matches!(result, Err(error) if error.code() == "shared_lock_lost"));
        Ok(())
    }.await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn article_import_failure_rolls_back_protected_write_and_releases_transaction_lock()
-> Result<()> {
    let fixture = Fixture::new().await?;
    let outcome: Result<()> = async {
        let connection = fixture.first.primary_connection()?;
        connection.execute_unprepared("CREATE TABLE terminal_side_effects (import_id TEXT NOT NULL)").await.map_err(Error::internal)?;
        // AFTER trigger 已尝试写副作用，再主动失败；生产 terminal 必须回滚整个事务。
        connection.execute_unprepared("CREATE TRIGGER reject_import_terminal AFTER UPDATE OF state ON example_article_imports WHEN NEW.error_code = 'fixture_failure' BEGIN INSERT INTO terminal_side_effects(import_id) VALUES(NEW.import_id); SELECT RAISE(ABORT, 'example_import_terminal_rejected'); END").await.map_err(Error::internal)?;
        let db = fixture.first.clone();
        let result = with_import_lease(Arc::new(db.clone()), OWNER, IMPORT_ID, move |locks, lease| async move {
            terminal(&db, &locks, &lease, receipt(&db).await?, "failed", Some("fixture_failure")).await
        }).await;
        assert!(matches!(result, Err(Error::Internal(error)) if error.to_string().contains("example_import_terminal_rejected")));
        let unchanged = receipt(&fixture.second).await?;
        assert_eq!(unchanged.state, "open");
        assert_eq!(unchanged.error_code, None);
        assert_eq!(unchanged.terminal_at_ms, None);
        assert_eq!(fixture.second.table("terminal_side_effects")?.count().await?, 0);
        // 真实 wrapper 在失败后 release；另一物理连接可直接进入同一生产边界重试。
        let db = fixture.second.clone();
        with_import_lease(Arc::new(db.clone()), OWNER, IMPORT_ID, move |locks, lease| async move {
            terminal(&db, &locks, &lease, receipt(&db).await?, "completed", None).await
        }).await?;
        let committed = receipt(&fixture.first).await?;
        assert_eq!(committed.state, "completed");
        assert_eq!(committed.error_code, None);
        assert!(committed.terminal_at_ms.is_some());
        let locks = SharedLocks::new(fixture.first.clone());
        let lease = acquire_import(&locks).await?;
        assert!(locks.release(&lease).await?);
        Ok(())
    }.await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}
