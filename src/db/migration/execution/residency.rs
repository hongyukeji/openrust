//! 以物理 SQLite handle 的原生 collation 析构见证确认显式归还，避免误用池计数。

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use sqlx::{Row, Sqlite, pool::PoolConnection};

use crate::{Error, Result};

type Comparator = fn(&str, &str) -> std::cmp::Ordering;

static COLLATION_NAME: OnceLock<std::result::Result<String, ()>> = OnceLock::new();
static WITNESSES: OnceLock<Mutex<HashMap<usize, Weak<Comparator>>>> = OnceLock::new();

/// 确认本次归还后的同一物理 handle 在观察时点仍活着；不关闭其他池连接。
///
/// 已被其他任务借出也可返回 true；false 不表示同一池的其他连接均已失效。
/// 仅适用于标准 SQLx/SQLite 正常析构，不承诺绕过安全 API 的扩展泄漏原生句柄时仍可靠。
pub(crate) async fn return_connection(connection: PoolConnection<Sqlite>) -> Result<bool> {
    let witness = return_with_witness(connection).await?;
    Ok(witness.upgrade().is_some())
}

// 私有两步边界便于确定性验证：归还后先由另一任务借出，再检查同一个见证。
async fn return_with_witness(mut connection: PoolConnection<Sqlite>) -> Result<Weak<Comparator>> {
    let witness = register_witness(&mut connection).await;
    // 注册失败也必须显式归还；不能让即将退出runtime上的Drop排队任务失去所有者。
    connection.return_to_pool().await;
    #[cfg(test)]
    if let Ok(gate) = RETURN_CHECK_GATE.try_with(Arc::clone) {
        gate.arrived.notify_one();
        gate.proceed.notified().await;
    }
    witness
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct ReturnCheckGate {
    pub(super) arrived: tokio::sync::Notify,
    pub(super) proceed: tokio::sync::Notify,
}

#[cfg(test)]
tokio::task_local! { static RETURN_CHECK_GATE: Arc<ReturnCheckGate>; }

#[cfg(test)]
pub(super) async fn with_return_check_gate<T>(
    gate: Arc<ReturnCheckGate>,
    future: impl std::future::Future<Output = T>,
) -> T {
    RETURN_CHECK_GATE.scope(gate, future).await
}

fn internal_name() -> Result<&'static str> {
    COLLATION_NAME
        .get_or_init(|| {
            let mut bytes = [0_u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| ())?;
            let mut name = String::with_capacity(96);
            name.push_str("__openrust_connection_residency_");
            for byte in bytes {
                write!(&mut name, "{byte:02x}").map_err(|_| ())?;
            }
            Ok(name)
        })
        .as_ref()
        .map(String::as_str)
        .map_err(|()| failure("migration_connection_witness_randomness_unavailable"))
}

async fn register_witness(connection: &mut PoolConnection<Sqlite>) -> Result<Weak<Comparator>> {
    let name = internal_name()?;
    // 查询先于lock_handle；持有原生handle时再发SQL会等待同一worker而自锁。
    let collations = sqlx::query("PRAGMA collation_list")
        .fetch_all(&mut **connection)
        .await
        .map_err(|_| failure("migration_connection_witness_inspection_failed"))?;
    let mut exists = false;
    for row in collations {
        let existing: String = row
            .try_get("name")
            .map_err(|_| failure("migration_connection_witness_inspection_failed"))?;
        // SQLite collation 名称忽略ASCII大小写。
        exists |= existing.eq_ignore_ascii_case(name);
    }
    let mut handle = connection
        .lock_handle()
        .await
        .map_err(|_| failure("migration_connection_witness_handle_unavailable"))?;
    // 只取安全公开API提供的地址作registry key，不解引用、不重建所有权。
    let identity = handle.as_raw_handle().as_ptr() as usize;
    let mut registry = WITNESSES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| failure("migration_connection_witness_registry_unavailable"))?;
    registry.retain(|_, witness| witness.strong_count() > 0);
    if exists
        && registry
            .get(&identity)
            .is_none_or(|witness| witness.strong_count() == 0)
    {
        return Err(failure("migration_connection_witness_name_collision"));
    }
    let owner = Arc::new(str::cmp as Comparator);
    let witness = Arc::downgrade(&owner);
    // 每物理handle复用一个进程内随机名。覆盖仅限registry证实仍活的自有closure，
    // SQLite同步析构旧closure；不在长寿命连接上累积每次调用的随机名称。
    handle
        .create_collation(name, move |left, right| owner(left, right))
        .map_err(|_| failure("migration_connection_witness_registration_failed"))?;
    registry.insert(identity, witness.clone());
    Ok(witness)
}

fn failure(code: &'static str) -> Error {
    Error::custom(code, "无法确认 SQLite 迁移物理连接的归还状态")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> Result<sqlx::SqlitePool> {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(0)
            .connect("sqlite::memory:")
            .await
            .map_err(Error::internal)
    }

    #[tokio::test]
    async fn returned_handle_remains_alive_when_another_task_checks_it_out() -> Result<()> {
        let pool = pool().await?;
        let connection = pool.acquire().await.map_err(Error::internal)?;
        let witness = return_with_witness(connection).await?;
        let borrower_pool = pool.clone();
        let (entered, checked_out) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let borrower = tokio::spawn(async move {
            let mut connection = borrower_pool.acquire().await.map_err(Error::internal)?;
            let value: i64 = sqlx::query_scalar("SELECT 73")
                .fetch_one(&mut *connection)
                .await
                .map_err(Error::internal)?;
            assert_eq!(value, 73);
            let _ = entered.send(());
            let _ = released.await;
            connection.close().await.map_err(Error::internal)
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), checked_out)
            .await
            .map_err(|_| failure("test_borrower_timeout"))?
            .map_err(|_| failure("test_borrower_lost"))?;
        assert!(witness.upgrade().is_some());
        assert!(!pool.is_closed());
        let _ = release.send(());
        tokio::time::timeout(std::time::Duration::from_secs(10), borrower)
            .await
            .map_err(|_| failure("test_borrower_timeout"))?
            .map_err(|_| failure("test_borrower_lost"))??;
        assert!(witness.upgrade().is_none());
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn rejected_return_destroys_witness_without_closing_whole_pool() -> Result<()> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(0)
            .after_release(|_, _| Box::pin(async { Ok(false) }))
            .connect("sqlite::memory:")
            .await
            .map_err(Error::internal)?;
        let connection = pool.acquire().await.map_err(Error::internal)?;
        assert!(!return_connection(connection).await?);
        assert!(!pool.is_closed());
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn repeated_registration_replaces_owned_closure_and_does_not_accumulate_names()
    -> Result<()> {
        let pool = pool().await?;
        let mut connection = pool.acquire().await.map_err(Error::internal)?;
        let first = register_witness(&mut connection).await?;
        let second = register_witness(&mut connection).await?;
        assert!(first.upgrade().is_none());
        assert!(second.upgrade().is_some());
        let names = sqlx::query("PRAGMA collation_list")
            .fetch_all(&mut *connection)
            .await
            .map_err(Error::internal)?;
        let mut count = 0;
        for row in names {
            let name: String = row.try_get("name").map_err(Error::internal)?;
            count += usize::from(name.eq_ignore_ascii_case(internal_name()?));
        }
        assert_eq!(count, 1);
        let identity = connection
            .lock_handle()
            .await
            .map_err(Error::internal)?
            .as_raw_handle()
            .as_ptr() as usize;
        {
            let registry = WITNESSES
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .map_err(|_| failure("test_registry_unavailable"))?;
            assert!(
                registry
                    .get(&identity)
                    .is_some_and(|stored| Weak::ptr_eq(stored, &second))
            );
        }
        connection.close().await.map_err(Error::internal)?;
        assert!(second.upgrade().is_none());
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn user_collation_collision_is_refused_without_overwriting_comparator() -> Result<()> {
        let pool = pool().await?;
        let mut connection = pool.acquire().await.map_err(Error::internal)?;
        {
            let mut handle = connection.lock_handle().await.map_err(Error::internal)?;
            handle
                .create_collation(internal_name()?, |left, right| right.cmp(left))
                .map_err(Error::internal)?;
        }
        let result = register_witness(&mut connection).await;
        assert_eq!(
            result.err().map(|error| error.code().to_owned()),
            Some("migration_connection_witness_name_collision".to_owned())
        );
        // 标识符仅来自内部固定ASCII前缀和随机hex，无外部输入。
        let query = format!(
            "SELECT CASE WHEN 'a' COLLATE \"{}\" < 'b' THEN 1 ELSE 0 END",
            internal_name()?
        );
        let comparison: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_one(&mut *connection)
            .await
            .map_err(Error::internal)?;
        assert_eq!(comparison, 0);
        connection.close().await.map_err(Error::internal)?;
        pool.close().await;
        Ok(())
    }
}
