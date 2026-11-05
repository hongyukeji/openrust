//! 真实SQLite并发计数与安全事务：能发现跨进程丢计数、溢出及取消后脏事务复用。
use super::*;
use crate::{Error, db::sea_orm::Database, testing::TestDatabase};
use sea_orm_migration::{MigrationTrait, MigratorTrait};

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(DatabaseRateLimitMigration)]
    }
}
fn check(value: bool, message: &'static str) -> crate::Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::custom("sqlite_limit_contract_failed", message))
    }
}
const CHILD: &str = "openrust-sqlite-security-limit-child";
const KEY: &str = "sensitive.identity@example.test";

#[test]
#[ignore = "父测试仅向显式子进程传递自有SQLite地址"]
fn sqlite_process_counter_child() -> crate::Result<()> {
    let path = std::env::var("OPENRUST_SQLITE_LIMITER_CHILD_URL").map_err(Error::internal)?;
    let seconds = std::env::var("OPENRUST_SQLITE_LIMITER_CHILD_WINDOW")
        .map_err(Error::internal)?
        .parse::<u64>()
        .map_err(Error::internal)?;
    check(
        std::env::var("OPENRUST_SQLITE_LIMITER_CHILD").as_deref() == Ok(CHILD),
        "缺少父进程标记",
    )?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::internal)?
        .block_on(async {
            let connection = Database::connect(&path).await.map_err(Error::internal)?;
            let db = Db::default();
            db.initialize(connection.clone())?;
            let store = DatabaseRateLimitStore::new(db);
            let outcome: crate::Result<()> = async {
                for _ in 0..20 {
                    store
                        .consume(KEY, std::time::Duration::from_secs(seconds))
                        .await?;
                }
                Ok(())
            }
            .await;
            let stopped = connection.close().await.map_err(Error::internal);
            outcome?;
            stopped
        })
}

#[tokio::test]
async fn sqlite_two_processes_keep_every_increment_and_saturate_without_returning()
-> crate::Result<()> {
    let database = TestDatabase::sqlite_file::<Migrator>().await?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let outcome: crate::Result<()> = async {
        // 本测试直接验证driver而非应用策略：将窗口尾置于一小时后，避免自然时钟跨窗口假失败。
        let seconds = <u64 as std::convert::TryFrom<i64>>::try_from(chrono::Utc::now().timestamp())
            .map_err(Error::internal)?
            + 3600;
        let executable = std::env::current_exe().map_err(Error::internal)?;
        let mut children = Vec::new();
        for _ in 0..2 {
            let child = tokio::process::Command::new(&executable)
                .kill_on_drop(true)
                .args([
                    "db::rate_limit::tests::sqlite_process_counter_child",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .env("OPENRUST_SQLITE_LIMITER_CHILD", CHILD)
                .env("OPENRUST_SQLITE_LIMITER_CHILD_URL", database.url())
                .env("OPENRUST_SQLITE_LIMITER_CHILD_WINDOW", seconds.to_string())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(Error::internal)?;
            children.push(child);
        }
        for child in children {
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                    .await
                    .map_err(Error::internal)?
                    .map_err(Error::internal)?;
            check(output.status.success(), "SQLite计数子进程失败")?;
            check(
                String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored"),
                "子进程未实际执行计数",
            )?;
        }
        let store = DatabaseRateLimitStore::new(db.clone());
        let bucket = store
            .consume(KEY, std::time::Duration::from_secs(seconds))
            .await?;
        check(bucket.count() == 41, "两个进程的原子增量丢失")?;
        let connection = db.connection()?;
        let row = connection
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT key_hash FROM rate_limits",
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("missing_limit_row", "缺少计数行"))?;
        let hashed: String = row.try_get("", "key_hash").map_err(Error::internal)?;
        check(
            hashed.len() == 64 && !hashed.contains(KEY),
            "限流身份没有摘要化",
        )?;
        connection
            .execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                "UPDATE rate_limits SET hit_count=9223372036854775807",
            ))
            .await
            .map_err(Error::internal)?;
        check(
            store
                .consume(KEY, std::time::Duration::from_secs(seconds))
                .await?
                .count()
                == i64::MAX.cast_unsigned(),
            "饱和计数溢出或变成REAL",
        )?;
        Ok(())
    }
    .await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}

#[cfg(feature = "auth")]
#[tokio::test]
async fn sqlite_security_transaction_cancellation_rolls_back_before_next_owner() -> crate::Result<()>
{
    let database = TestDatabase::sqlite_memory::<Migrator>().await?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let outcome=async {
        let connection=db.connection()?;
        connection.execute_unprepared("CREATE TABLE security_probe(id INTEGER PRIMARY KEY); INSERT INTO security_probe VALUES(1)").await.map_err(Error::internal)?;
        let entered=std::sync::Arc::new(tokio::sync::Notify::new());
        let notice=entered.clone();let owned=db.clone();
        let task=tokio::spawn(async move {owned.transaction_for_update(|tx|Box::pin(async move{
            tx.execute_unprepared("INSERT INTO security_probe VALUES(2)").await.map_err(Error::internal)?;
            notice.notify_one();std::future::pending::<crate::Result<()>>().await
        })).await});
        tokio::time::timeout(std::time::Duration::from_secs(5),entered.notified()).await.map_err(Error::internal)?;
        task.abort();let cancelled=task.await;
        check(cancelled.is_err_and(|error|error.is_cancelled()),"取消事务任务未终止")?;
        tokio::time::timeout(std::time::Duration::from_secs(5),db.transaction_for_update(|tx|Box::pin(async move {
            let row=tx.query_one_raw(Statement::from_string(DbBackend::Sqlite,"SELECT count(*) AS count FROM security_probe"))
                .await.map_err(Error::internal)?.ok_or_else(||Error::custom("missing_probe","缺少事务见证行"))?;
            check(row.try_get::<i64>("","count").map_err(Error::internal)?==1,"取消写泄漏或原内存数据库丢失")
        }))).await.map_err(Error::internal)?
    }.await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}

#[tokio::test]
async fn sqlite_first_hit_decay_is_not_aligned_and_reads_clear_expiry_are_real() -> crate::Result<()>
{
    let database = TestDatabase::sqlite_file::<Migrator>().await?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let outcome: crate::Result<()> = async {
        let store = DatabaseRateLimitStore::new(db.clone());
        let key = "decay:first-hit@example.test";
        let first = store.hit(key, std::time::Duration::from_hours(8_760)).await?;
        check(first.count() == 1 && first.reset_after_secs() > 31_535_990, "首次衰减窗口没有从当前秒开始")?;
        let table = db.quoted_table_name("rate_limits")?;
        let row = db.connection()?.query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("SELECT window_started_at, CAST(strftime('%s','now') AS INTEGER) AS now_seconds FROM {table} WHERE key_hash=?"), [digest(key).into()])).await.map_err(Error::internal)?.ok_or_else(|| Error::custom("missing_bucket", "缺少计数行"))?;
        let started: i64 = row.try_get("", "window_started_at").map_err(Error::internal)?;
        let now: i64 = row.try_get("", "now_seconds").map_err(Error::internal)?;
        check((now-started).abs() <= 2, "首次窗口不是数据库当前epoch秒")?;
        let read1 = store.attempts(key).await?.ok_or_else(|| Error::custom("missing_bucket", "缺少计数"))?;
        let read2 = store.attempts(key).await?.ok_or_else(|| Error::custom("missing_bucket", "缺少计数"))?;
        check(read1.count() == 1 && read2.count() == 1, "只读查询消耗了额度")?;
        let second = store.hit(key, std::time::Duration::from_secs(1)).await?;
        check(second.count() == 2 && second.reset_after_secs() > 31_535_990, "后续命中改变了首次decay或重新对齐窗口")?;
        db.connection()?.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("UPDATE {table} SET window_started_at=CAST(strftime('%s','now') AS INTEGER)-100, window_seconds=1 WHERE key_hash=?"), [digest(key).into()])).await.map_err(Error::internal)?;
        check(store.attempts(key).await?.is_none(), "到期计数仍可见")?;
        let fresh = store.hit(key, std::time::Duration::from_secs(60)).await?;
        check(fresh.count() == 1 && fresh.reset_after_secs() >= 58, "到期命中没有新建完整decay窗口")?;
        store.clear(key).await?; store.clear(key).await?;
        check(store.attempts(key).await?.is_none(), "clear没有删除计数")?;
        check(store.hit(key, std::time::Duration::from_secs(60)).await?.count() == 1, "clear后计数未从1开始")
    }.await;
    drop(db);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
async fn sqlite_decay_concurrent_hit_keeps_every_increment_and_missing_table_fails_closed()
-> crate::Result<()> {
    let database = TestDatabase::sqlite_file::<Migrator>().await?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let outcome: crate::Result<()> = async {
        let store = DatabaseRateLimitStore::new(db.clone());
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..24 {
            let store = store.clone();
            tasks.spawn(async move {
                store
                    .hit("decay:concurrent", std::time::Duration::from_secs(600))
                    .await
            });
        }
        let mut counts = Vec::new();
        while let Some(result) = tasks.join_next().await {
            counts.push(result.map_err(Error::internal)??.count());
        }
        counts.sort_unstable();
        check(counts == (1..=24).collect::<Vec<_>>(), "并发衰减增量丢失")?;
        check(
            store
                .attempts("decay:concurrent")
                .await?
                .is_some_and(|bucket| bucket.count() == 24),
            "只读最终计数不正确",
        )?;
        let table = db.quoted_table_name("rate_limits")?;
        db.connection()?
            .execute_unprepared(&format!("DROP TABLE {table}"))
            .await
            .map_err(Error::internal)?;
        check(
            store
                .hit("decay:failure", std::time::Duration::from_secs(60))
                .await
                .is_err(),
            "hit存储失败被放行",
        )?;
        check(
            store.attempts("decay:failure").await.is_err(),
            "attempts存储失败成为缺失",
        )?;
        check(
            store.clear("decay:failure").await.is_err(),
            "clear存储失败成为成功",
        )
    }
    .await;
    drop(db);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}
