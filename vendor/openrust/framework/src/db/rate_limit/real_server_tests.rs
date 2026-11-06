//! 真实MySQL/PostgreSQL饱和与衰减计数，不依赖SQLite feature。
use super::*;
use crate::{Error, testing::TestDatabase};
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
        Err(Error::custom(
            "database_rate_limit_contract_failed",
            message,
        ))
    }
}
const KEY: &str = "sensitive.identity@example.test";

#[tokio::test]
#[ignore = "显式真实MySQL/PostgreSQL，缺少凭据即错误，不以skip冒充通过"]
async fn real_server_counter_saturates_before_integer_addition() -> crate::Result<()> {
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let outcome: crate::Result<()> = async {
        let seconds = <u64 as std::convert::TryFrom<i64>>::try_from(chrono::Utc::now().timestamp())
            .map_err(Error::internal)?
            + 3600;
        let store = DatabaseRateLimitStore::new(db.clone());
        check(
            store
                .consume(KEY, std::time::Duration::from_secs(seconds))
                .await?
                .count()
                == 1,
            "真实后端未建立首个计数",
        )?;
        let table = db.quoted_table_name("rate_limits")?;
        db.connection()?
            .execute_unprepared(&format!("UPDATE {table} SET hit_count=9223372036854775807"))
            .await
            .map_err(Error::internal)?;
        check(
            store
                .consume(KEY, std::time::Duration::from_secs(seconds))
                .await?
                .count()
                == i64::MAX.cast_unsigned(),
            "真实后端饱和前发生整数溢出",
        )
    }
    .await;
    drop(db);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[tokio::test]
#[ignore = "显式真实MySQL/PostgreSQL衰减窗口，缺少凭据即错误"]
async fn real_server_decay_preserves_first_expiry_counts_concurrently_and_clears()
-> crate::Result<()> {
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let db = Db::default();
    db.initialize(database.connection()?)?;
    let outcome: crate::Result<()> = async {
        let store = DatabaseRateLimitStore::new(db.clone());
        let first = store
            .hit("decay:real", std::time::Duration::from_secs(600))
            .await?;
        check(
            first.count() == 1 && first.reset_after_secs() >= 595,
            "真实后端首个衰减窗口错误",
        )?;
        check(
            store
                .hit("decay:real", std::time::Duration::from_secs(1))
                .await?
                .reset_after_secs()
                >= 595,
            "后续hit缩短了首次TTL",
        )?;
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..12 {
            let store = store.clone();
            tasks.spawn(async move {
                store
                    .hit("decay:real", std::time::Duration::from_secs(600))
                    .await
            });
        }
        let mut counts = Vec::new();
        while let Some(result) = tasks.join_next().await {
            counts.push(result.map_err(Error::internal)??.count());
        }
        counts.sort_unstable();
        check(
            counts == (3..=14).collect::<Vec<_>>(),
            "真实后端并发衰减计数丢失",
        )?;
        check(
            store
                .attempts("decay:real")
                .await?
                .is_some_and(|bucket| bucket.count() == 14),
            "真实后端只读计数错误",
        )?;
        let table = db.quoted_table_name("rate_limits")?;
        db.connection()?
            .execute_unprepared(&format!(
                "UPDATE {table} SET window_started_at=0,window_seconds=1"
            ))
            .await
            .map_err(Error::internal)?;
        check(
            store.attempts("decay:real").await?.is_none(),
            "真实后端到期状态未消失",
        )?;
        check(
            store
                .hit("decay:real", std::time::Duration::from_secs(60))
                .await?
                .count()
                == 1,
            "真实后端到期计数没有重开",
        )?;
        store.clear("decay:real").await?;
        check(
            store.attempts("decay:real").await?.is_none(),
            "真实后端clear无效",
        )
    }
    .await;
    drop(db);
    let cleanup = database.close().await;
    outcome?;
    cleanup
}
