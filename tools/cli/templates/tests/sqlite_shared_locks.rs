//! SQLite 同机文件共享租约：所有权、fence、到期与业务事务保护回归。

use openrust::{
    Error, Result,
    db::{
        CoordinationMigration, Db, LockAcquire, LockLease, SharedLocks,
        sea_orm::{ConnectionTrait, Database},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use std::time::Duration;

struct Migrations;
impl MigratorTrait for Migrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(CoordinationMigration)]
    }
}

fn check(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom(
            "generated_sqlite_lock_contract",
            "SQLite 共享租约契约不符",
        ))
    }
}

async fn acquire(locks: &SharedLocks) -> Result<LockLease> {
    match locks
        .acquire("generated:report:42", Duration::from_secs(30))
        .await?
    {
        LockAcquire::Acquired(lease) => Ok(lease),
        LockAcquire::Busy { .. } => Err(Error::custom(
            "generated_sqlite_lock_busy",
            "自有测试租约意外占用",
        )),
    }
}

#[tokio::test]
async fn sqlite_file_leases_guard_the_same_business_transaction() -> Result<()> {
    let database = TestDatabase::sqlite_file::<Migrations>().await?;
    let outcome: Result<()> = async {
        let independent = Database::connect(database.url())
            .await
            .map_err(Error::internal)?;
        let result: Result<()> = async {
            let first = Db::default();
            first.initialize(database.connection()?)?;
            let second = Db::default();
            second.initialize(independent.clone())?;
            exercise(&first, &second).await
        }
        .await;
        independent.close().await.map_err(Error::internal)?;
        result
    }
    .await;
    database.close().await?;
    outcome
}

async fn exercise(first: &Db, second: &Db) -> Result<()> {
    first
        .connection()?
        .execute_unprepared(
            "CREATE TABLE protected_reports (id INTEGER PRIMARY KEY, revision INTEGER NOT NULL)",
        )
        .await
        .map_err(Error::internal)?;
    first
        .table("protected_reports")?
        .insert(vec![("id", 42_i64.into()), ("revision", 0_i64.into())])
        .await?;
    let owner = SharedLocks::new(first.clone());
    let peer = SharedLocks::new(second.clone());
    let original = acquire(&owner).await?;
    check(original.fencing_token() == 1 && original.expires_at_ms() > 0)?;
    check(
        matches!(peer.acquire("generated:report:42", Duration::from_secs(30)).await?,
        LockAcquire::Busy { expires_at_ms } if expires_at_ms == original.expires_at_ms()),
    )?;
    check(owner.renew(&original, Duration::from_secs(30)).await?)?;
    protected_write(first, &owner, &original, 1).await?;
    check(revision(second).await? == Some(1))?;

    // 只修改本fixture唯一租约的数据库到期值，不依赖墙钟sleep或私有lease字段。
    mutate_lease(
        first,
        original.fencing_token(),
        "expires_at_ms",
        0_i64.into(),
    )
    .await?;
    check(!owner.renew(&original, Duration::from_secs(30)).await?)?;
    rejected_write(first, second, &owner, &original).await?;
    let replacement = acquire(&peer).await?;
    check(replacement.fencing_token() > original.fencing_token())?;
    check(!owner.release(&original).await?)?;
    protected_write(second, &peer, &replacement, 2).await?;
    check(revision(first).await? == Some(2))?;

    // owner不变而fence失配时，旧句柄也不能续约、释放或提交业务写入。
    let next_fence = replacement.fencing_token() + 1;
    mutate_lease(
        first,
        replacement.fencing_token(),
        "fencing_token",
        next_fence.into(),
    )
    .await?;
    check(!peer.renew(&replacement, Duration::from_secs(30)).await?)?;
    check(!peer.release(&replacement).await?)?;
    rejected_write(second, first, &peer, &replacement).await?;
    mutate_lease(first, next_fence, "expires_at_ms", 0_i64.into()).await?;
    let current = acquire(&owner).await?;
    check(current.fencing_token() > next_fence)?;

    // fence和到期不变，仅owner变化也必须拒绝旧owner；不读取或输出随机owner凭据。
    mutate_lease(
        first,
        current.fencing_token(),
        "owner_token",
        "fixture-other-owner".into(),
    )
    .await?;
    check(!owner.renew(&current, Duration::from_secs(30)).await?)?;
    check(!owner.release(&current).await?)?;
    rejected_write(first, second, &owner, &current).await?;
    mutate_lease(
        first,
        current.fencing_token(),
        "expires_at_ms",
        0_i64.into(),
    )
    .await?;
    let final_lease = acquire(&peer).await?;
    check(final_lease.fencing_token() > current.fencing_token())?;
    check(peer.release(&final_lease).await?)?;
    check(!peer.release(&final_lease).await?)?;
    check(first.table("shared_locks")?.count().await? == 0)
}

async fn mutate_lease(
    db: &Db,
    fence: i64,
    column: &str,
    value: openrust::db::sea_orm::Value,
) -> Result<()> {
    check(db.table("shared_locks")?.count().await? == 1)?;
    check(
        db.table("shared_locks")?
            .where_eq("fencing_token", fence)?
            .update(vec![(column, value)])
            .await?
            == 1,
    )
}

async fn revision(db: &Db) -> Result<Option<i64>> {
    db.table("protected_reports")?
        .where_eq("id", 42_i64)?
        .value("revision")
        .await
}

async fn protected_write(
    db: &Db,
    locks: &SharedLocks,
    lease: &LockLease,
    revision: i64,
) -> Result<()> {
    let business = db.clone();
    let locks = locks.clone();
    let lease = lease.clone();
    db.transaction(move |transaction| {
        Box::pin(async move {
            // 故意先产生业务副作用；校验失败必须连同该写入回滚，不能另开校验事务。
            check(
                business
                    .table("protected_reports")?
                    .using_transaction(transaction)
                    .where_eq("id", 42_i64)?
                    .update(vec![("revision", revision.into())])
                    .await?
                    == 1,
            )?;
            locks
                .assert_current_in_transaction(transaction, &lease)
                .await
        })
    })
    .await
}

async fn rejected_write(
    writer: &Db,
    observer: &Db,
    locks: &SharedLocks,
    lease: &LockLease,
) -> Result<()> {
    let before = revision(observer).await?;
    check(matches!(protected_write(writer, locks, lease, 99).await,
        Err(error) if error.code() == "shared_lock_lost"))?;
    check(revision(observer).await? == before)
}
