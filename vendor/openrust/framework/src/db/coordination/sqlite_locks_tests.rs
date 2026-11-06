//! 真实文件与独立连接验证 SQLite 租约；子进程入口只由本文件的父测试显式调用。

use super::*;
use crate::db::sea_orm_migration::{MigrationTrait, SchemaManager};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, TransactionTrait};
use std::{collections::BTreeMap, time::Duration};

async fn connection(url: &str) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_owned());
    options
        .max_connections(1)
        .min_connections(1)
        .sqlx_logging(false);
    options.map_sqlx_sqlite_opts(|options| {
        options
            .journal_mode(sea_orm::sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_millis(500))
    });
    Database::connect(options).await.map_err(Error::internal)
}

fn database(connection: DatabaseConnection) -> Result<Db> {
    let db = Db::with_table_names(crate::db::DatabaseTableNames::new(
        "prefix_",
        BTreeMap::from([("shared_locks".into(), "mapped_locks".into())]),
    )?);
    db.initialize(connection)?;
    Ok(db)
}

async fn fixture() -> Result<(tempfile::TempDir, String, Db, Db)> {
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("locks.sqlite").display()
    );
    let first = connection(&url).await?;
    let db = database(first.clone())?;
    let manager = SchemaManager::new(&first);
    db.table_names()
        .scope(super::super::CoordinationMigration.up(&manager))
        .await
        .map_err(Error::internal)?;
    first
        .execute_unprepared("CREATE TABLE business_rows (id INTEGER PRIMARY KEY)")
        .await
        .map_err(Error::internal)?;
    let second = connection(&url).await?;
    Ok((directory, url, db, database(second)?))
}

fn failed(code: &'static str) -> Error {
    Error::custom(code, "SQLite 租约回归未满足预期")
}

async fn lease(db: &Db, key: &str) -> Result<LockLease> {
    match acquire(db, key.into(), super::super::random_token()?, 30_000).await? {
        LockAcquire::Acquired(lease) => Ok(lease),
        LockAcquire::Busy { .. } => Err(failed("test_lock_unexpectedly_busy")),
    }
}

async fn expire(db: &Db, key: &str) -> Result<()> {
    db.primary_connection()?
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE mapped_locks SET expires_at_ms=0 WHERE key_hash=?",
            [key.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

async fn finish(first: Db, second: Db) -> Result<()> {
    let first_result = first
        .primary_connection()?
        .close()
        .await
        .map_err(Error::internal);
    let second_result = second
        .primary_connection()?
        .close()
        .await
        .map_err(Error::internal);
    drop(first);
    drop(second);
    first_result?;
    second_result
}

#[tokio::test]
async fn db_clock_mapping_and_busy_preserve_owner_until_expiry() -> Result<()> {
    let (_directory, _url, first, second) = fixture().await?;
    let before = first
        .primary_connection()?
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT {NOW_MS} AS clock"),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| failed("test_clock_missing"))?
        .try_get::<i64>("", "clock")
        .map_err(Error::internal)?;
    let original = lease(&first, "clock-key").await?;
    assert_eq!(original.fencing_token(), 1);
    assert!(original.expires_at_ms >= before + 30_000);
    match acquire(
        &second,
        "clock-key".into(),
        super::super::random_token()?,
        30_000,
    )
    .await?
    {
        LockAcquire::Busy { expires_at_ms } => assert_eq!(expires_at_ms, original.expires_at_ms),
        LockAcquire::Acquired(_) => return Err(failed("test_owner_overwritten")),
    }
    let other = lease(&second, "different-key").await?;
    assert_ne!(original.owner_token, other.owner_token);
    assert!(renew(&first, &original, 60_000).await?);
    expire(&first, "clock-key").await?;
    assert!(!renew(&first, &original, 60_000).await?);
    let expired_transaction = first
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    assert!(
        matches!(assert_current_in_transaction(&first, &expired_transaction, &original).await, Err(error) if error.code() == "shared_lock_lost")
    );
    expired_transaction
        .rollback()
        .await
        .map_err(Error::internal)?;
    let replacement = lease(&second, "clock-key").await?;
    assert!(replacement.fencing_token() > original.fencing_token());
    let mut wrong_fence = replacement.clone();
    wrong_fence.fencing_token -= 1;
    let mut wrong_owner = replacement.clone();
    wrong_owner.owner_token = original.owner_token.clone();
    for invalid in [wrong_fence, wrong_owner] {
        assert!(!renew(&second, &invalid, 30_000).await?);
        assert!(
            !super::super::SharedLocks::new(second.clone())
                .release(&invalid)
                .await?
        );
        let transaction = second
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;
        assert!(
            matches!(assert_current_in_transaction(&second, &transaction, &invalid).await, Err(error) if error.code() == "shared_lock_lost")
        );
        transaction.rollback().await.map_err(Error::internal)?;
    }
    assert!(!renew(&first, &original, 30_000).await?);
    let locks = super::super::SharedLocks::new(first.clone());
    assert!(!locks.release(&original).await?);
    let transaction = first
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    assert!(
        matches!(assert_current_in_transaction(&first, &transaction, &original).await, Err(error) if error.code() == "shared_lock_lost")
    );
    transaction.rollback().await.map_err(Error::internal)?;
    assert!(
        super::super::SharedLocks::new(second.clone())
            .release(&replacement)
            .await?
    );
    finish(first, second).await
}

#[tokio::test]
async fn two_independent_pools_have_one_owner_and_one_busy_result() -> Result<()> {
    use std::sync::Arc;
    let (_directory, _url, first, second) = fixture().await?;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let start = Arc::clone(&barrier);
    let left = first.clone();
    let left_task = tokio::spawn(async move {
        start.wait().await;
        acquire(&left, "race".into(), super::super::random_token()?, 30_000).await
    });
    let start = Arc::clone(&barrier);
    let right = second.clone();
    let right_task = tokio::spawn(async move {
        start.wait().await;
        acquire(&right, "race".into(), super::super::random_token()?, 30_000).await
    });
    barrier.wait().await;
    let left = left_task
        .await
        .map_err(|_| failed("test_lock_task_panicked"))??;
    let right = right_task
        .await
        .map_err(|_| failed("test_lock_task_panicked"))??;
    let ((LockAcquire::Acquired(owner), LockAcquire::Busy { expires_at_ms })
    | (LockAcquire::Busy { expires_at_ms }, LockAcquire::Acquired(owner))) = (left, right)
    else {
        return Err(failed("test_lock_race_not_exclusive"));
    };
    assert_eq!(owner.expires_at_ms, expires_at_ms);
    assert_eq!(owner.fencing_token(), 1);
    finish(first, second).await
}

#[tokio::test]
async fn transaction_assertion_holds_writer_lock_and_business_rollback_is_preserved() -> Result<()>
{
    let (_directory, _url, first, second) = fixture().await?;
    let original = lease(&first, "transaction").await?;
    let tx = first
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    assert_current_in_transaction(&first, &tx, &original).await?;
    tx.execute_unprepared("INSERT INTO business_rows(id) VALUES(1)")
        .await
        .map_err(Error::internal)?;
    // 竞争者不能把数据库 busy 当作已取得租约或无锁校验成功。
    assert!(
        acquire(
            &second,
            "transaction".into(),
            super::super::random_token()?,
            30_000
        )
        .await
        .is_err()
    );
    tx.rollback().await.map_err(Error::internal)?;
    let row = second
        .primary_connection()?
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS total FROM business_rows".to_owned(),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| failed("test_count_missing"))?;
    assert_eq!(row.try_get::<i64>("", "total").map_err(Error::internal)?, 0);
    assert!(matches!(
        acquire(
            &second,
            "transaction".into(),
            super::super::random_token()?,
            30_000
        )
        .await?,
        LockAcquire::Busy { .. }
    ));
    expire(&second, "transaction").await?;
    assert!(lease(&second, "transaction").await?.fencing_token() > original.fencing_token());
    finish(first, second).await
}

#[tokio::test]
async fn stale_read_snapshot_upgrade_is_an_error_instead_of_an_implicit_new_transaction()
-> Result<()> {
    let (_directory, _url, first, second) = fixture().await?;
    let original = lease(&first, "snapshot").await?;
    let tx = first
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    tx.query_one_raw(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT owner_token FROM mapped_locks WHERE key_hash='snapshot'".to_owned(),
    ))
    .await
    .map_err(Error::internal)?;
    assert!(renew(&second, &original, 60_000).await?);
    assert!(
        assert_current_in_transaction(&first, &tx, &original)
            .await
            .is_err()
    );
    tx.rollback().await.map_err(Error::internal)?;
    assert!(renew(&first, &original, 30_000).await?);
    finish(first, second).await
}

#[tokio::test]
async fn cancellation_and_decode_failure_do_not_leave_a_committed_owner_or_open_transaction()
-> Result<()> {
    let (_directory, _url, first, second) = fixture().await?;
    let original = lease(&first, "held").await?;
    let tx = first
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    assert_current_in_transaction(&first, &tx, &original).await?;
    let cancelled = tokio::time::timeout(
        Duration::from_millis(10),
        acquire(
            &second,
            "cancelled".into(),
            super::super::random_token()?,
            30_000,
        ),
    )
    .await;
    assert!(cancelled.is_err());
    tx.rollback().await.map_err(Error::internal)?;
    let row = tokio::time::timeout(
        Duration::from_secs(5),
        second
            .primary_connection()?
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS total FROM mapped_locks WHERE key_hash='cancelled'".to_owned(),
            )),
    )
    .await
    .map_err(|_| failed("test_cancelled_transaction_stuck"))?
    .map_err(Error::internal)?
    .ok_or_else(|| failed("test_count_missing"))?;
    assert_eq!(row.try_get::<i64>("", "total").map_err(Error::internal)?, 0);
    // SQLite 动态类型允许故障注入；读回 fencing 失败时原 Db 事务必须回滚。
    first
        .primary_connection()?
        .execute_unprepared(
            "INSERT INTO mapped_locks VALUES('broken','retained',0,'not-an-integer')",
        )
        .await
        .map_err(Error::internal)?;
    // 未过期损坏行不被 UPSERT 覆写，迫使严格 i64 解码失败。
    first
        .primary_connection()?
        .execute_unprepared(
            "UPDATE mapped_locks SET expires_at_ms=9223372036854775807 WHERE key_hash='broken'",
        )
        .await
        .map_err(Error::internal)?;
    assert!(
        acquire(
            &second,
            "broken".into(),
            super::super::random_token()?,
            30_000
        )
        .await
        .is_err()
    );
    let lease = lease(&second, "after-failure").await?;
    assert_eq!(lease.fencing_token(), 1);
    finish(first, second).await
}

#[cfg(unix)]
struct ChildProcess(std::process::Child);
#[cfg(unix)]
impl Drop for ChildProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn killed_process_releases_sqlite_writer_without_stealing_live_lease() -> Result<()> {
    let (directory, url, first, second) = fixture().await?;
    let original = lease(&first, "process").await?;
    let ready = directory.path().join("child-ready");
    let module = module_path!()
        .split_once("::")
        .map(|(_, module)| module)
        .ok_or_else(|| failed("test_module_path_invalid"))?;
    let mut child = ChildProcess(
        std::process::Command::new(std::env::current_exe().map_err(Error::internal)?)
            .args([
                "--exact",
                &format!("{module}::sqlite_process_owner"),
                "--ignored",
                "--nocapture",
            ])
            .env("OPENRUST_SQLITE_LOCK_CHILD_URL", url)
            .env("OPENRUST_SQLITE_LOCK_CHILD_READY", &ready)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(Error::internal)?,
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !ready.is_file() {
        if child.0.try_wait().map_err(Error::internal)?.is_some() {
            return Err(failed("test_child_exited_before_lock"));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(failed("test_child_lock_timeout"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(renew(&second, &original, 30_000).await.is_err());
    child.0.kill().map_err(Error::internal)?;
    child.0.wait().map_err(Error::internal)?;
    // 杀进程只释放物理事务锁，不擅自使持久 TTL 过期或认领该租约。
    assert!(matches!(
        acquire(
            &second,
            "process".into(),
            super::super::random_token()?,
            30_000
        )
        .await?,
        LockAcquire::Busy { .. }
    ));
    expire(&second, "process").await?;
    assert!(lease(&second, "process").await?.fencing_token() > original.fencing_token());
    finish(first, second).await
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "仅由跨进程父测试携带显式临时文件启动"]
async fn sqlite_process_owner() -> Result<()> {
    let url = std::env::var("OPENRUST_SQLITE_LOCK_CHILD_URL")
        .map_err(|_| failed("test_child_url_missing"))?;
    let ready = std::env::var_os("OPENRUST_SQLITE_LOCK_CHILD_READY")
        .ok_or_else(|| failed("test_child_ready_missing"))?;
    let db = database(connection(&url).await?)?;
    let row = db.primary_connection()?.query_one_raw(Statement::from_string(DbBackend::Sqlite,
        "SELECT key_hash,owner_token,fencing_token,expires_at_ms FROM mapped_locks WHERE key_hash='process'".to_owned()))
        .await.map_err(Error::internal)?.ok_or_else(|| failed("test_child_lease_missing"))?;
    let original = LockLease {
        key_hash: row.try_get("", "key_hash").map_err(Error::internal)?,
        owner_token: row.try_get("", "owner_token").map_err(Error::internal)?,
        fencing_token: row.try_get("", "fencing_token").map_err(Error::internal)?,
        expires_at_ms: row.try_get("", "expires_at_ms").map_err(Error::internal)?,
    };
    let tx = db
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    assert_current_in_transaction(&db, &tx, &original).await?;
    std::fs::write(ready, b"locked").map_err(Error::internal)?;
    tokio::time::sleep(Duration::from_secs(30)).await;
    tx.rollback().await.map_err(Error::internal)?;
    Err(failed("test_child_not_terminated"))
}

#[tokio::test]
async fn public_shared_locks_dispatches_sqlite_and_preserves_input_owner_and_fence_checks()
-> Result<()> {
    use super::super::SharedLocks;
    let (_directory, _url, first, second) = fixture().await?;
    let left = SharedLocks::new(first.clone());
    let right = SharedLocks::new(second.clone());
    assert!(left.acquire("", Duration::from_secs(30)).await.is_err());
    assert!(left.acquire("public", Duration::ZERO).await.is_err());
    assert!(
        left.acquire("public", Duration::from_secs(86_401))
            .await
            .is_err()
    );
    let original = match left.acquire("public", Duration::from_secs(30)).await? {
        LockAcquire::Acquired(lease) => lease,
        LockAcquire::Busy { .. } => return Err(failed("test_lock_unexpectedly_busy")),
    };
    assert!(matches!(
        right.acquire("public", Duration::from_secs(30)).await?,
        LockAcquire::Busy { .. }
    ));
    assert!(right.renew(&original, Duration::from_secs(60)).await?);
    expire(&first, &super::super::digest(b"public")).await?;
    let replacement = match right.acquire("public", Duration::from_secs(30)).await? {
        LockAcquire::Acquired(lease) => lease,
        LockAcquire::Busy { .. } => return Err(failed("test_expired_lock_not_acquired")),
    };
    assert!(replacement.fencing_token() > original.fencing_token());
    assert!(!left.renew(&original, Duration::from_secs(30)).await?);
    assert!(!left.release(&original).await?);
    let transaction = first
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    assert!(
        matches!(left.assert_current_in_transaction(&transaction, &original).await, Err(error) if error.code() == "shared_lock_lost")
    );
    transaction.rollback().await.map_err(Error::internal)?;
    let transaction = second
        .primary_connection()?
        .begin()
        .await
        .map_err(Error::internal)?;
    right
        .assert_current_in_transaction(&transaction, &replacement)
        .await?;
    transaction.rollback().await.map_err(Error::internal)?;
    assert!(right.release(&replacement).await?);
    finish(first, second).await
}
