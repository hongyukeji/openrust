//! 真实 SQLite 验证码存储与服务回归；不以替身代替写事务竞争。
use super::*;
use crate::auth::verification::{
    DatabaseVerificationCodeStore, VerificationCodeStore, VerificationCodes,
};
use crate::database::Db;
use crate::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use crate::support::BoxFuture;
use crate::testing::TestDatabase;
use std::time::Duration;

struct CodesMigrator;
impl MigratorTrait for CodesMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(crate::auth::AuthVerificationCodeMigration)]
    }
}
fn check(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom("sqlite_verification_contract", message))
    }
}
fn passwords() -> Result<std::sync::Arc<crate::hashing::HashManager>> {
    Ok(std::sync::Arc::new(crate::hashing::HashManager::new(
        crate::hashing::HashingConfig {
            driver: "argon2id".into(),
            argon: crate::hashing::ArgonConfig {
                memory: 19_456,
                time: 1,
                threads: 1,
                verify: true,
            },
            ..Default::default()
        },
    )?))
}
fn policy() -> Result<VerificationCodePolicy> {
    VerificationCodePolicy::new(6, Duration::from_secs(60), Duration::from_secs(30), 2)
}
async fn owned<F>(file: bool, callback: F) -> Result<()>
where
    F: for<'a> FnOnce(&'a Db, &'a Db) -> BoxFuture<'a, Result<()>>,
{
    let guard = if file {
        TestDatabase::sqlite_file::<CodesMigrator>().await?
    } else {
        TestDatabase::sqlite_memory::<CodesMigrator>().await?
    };
    let first = Db::default();
    let second = Db::default();
    let outcome = async {
        first.initialize(guard.connection()?)?;
        second.initialize(guard.connection()?)?;
        callback(&first, &second).await
    }
    .await;
    drop(first);
    drop(second);
    let cleanup = guard.close().await;
    outcome?;
    cleanup
}
async fn update_time(db: &Db, key: &str, field: &str, value: String) -> Result<()> {
    // field仅由本文件固定调用，标识符不接受外部输入。
    let table = db.quoted_table_name("verification_codes")?;
    db.connection()?
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("UPDATE {table} SET {field}=? WHERE key_hash=?"),
            [value.into(), key.to_owned().into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}
async fn scalar(db: &Db, sql: String) -> Result<i64> {
    db.connection()?
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::custom("sqlite_verification_row", "缺少断言行"))?
        .try_get("", "value")
        .map_err(Error::internal)
}

#[tokio::test]
async fn memory_issue_idempotency_cooldown_attempts_and_stale_challenge() -> Result<()> {
    owned(false, |db, _| {
        Box::pin(async move {
            let connection = db.connection()?;
            let table = db.quoted_table_name("verification_codes")?;
            // Argon2工作在任何写事务开始之前完成。
            let hash = crate::auth::EncodedPasswordHash::make(passwords()?.as_ref(), "123456").await?;
            issue(&connection, &table, "key", "old", &hash, policy()?).await?;
            check(
                !finish_attempt(&connection, &table, "key", "old", false).await?,
                "错误验证码被接受",
            )?;
            issue(&connection, &table, "key", "old", &hash, policy()?).await?;
            check(
                scalar(
                    db,
                    format!("SELECT attempt_count AS value FROM {table} WHERE key_hash='key'"),
                )
                .await?
                    == 1,
                "同challenge重试重置错误次数",
            )?;
            let deadline = Utc::now() + chrono::Duration::milliseconds(30_500);
            update_time(db, "key", "resend_after_at", deadline.to_rfc3339()).await?;
            let before = Utc::now();
            let cooldown = issue(&connection, &table, "key", "new", &hash, policy()?).await;
            let after = Utc::now();
            let ceil_seconds = |instant: DateTime<Utc>| -> Result<u64> {
                let micros = deadline.signed_duration_since(instant).num_microseconds()
                    .ok_or_else(|| Error::custom("sqlite_test_time", "测试时间超出范围"))?;
                u64::try_from((micros.max(1) - 1) / 1_000_000 + 1).map_err(Error::internal)
            };
            let minimum = ceil_seconds(after)?;
            let maximum = ceil_seconds(before)?;
            check(
                matches!(cooldown, Err(Error::RateLimitedWithRetryAfter(wait)) if (minimum..=maximum).contains(&wait)),
                "冷却重发未按实际判定窗口向上取整",
            )?;
            check(
                !finish_attempt(&connection, &table, "key", "old", false).await?,
                "错误尝试被接受",
            )?;
            check(
                load_active(&connection, &table, "key").await?.is_none(),
                "耗尽后仍可读取",
            )?;
            check(
                !finish_attempt(&connection, &table, "key", "old", true).await?,
                "耗尽后仍可消费",
            )?;
            update_time(
                db,
                "key",
                "resend_after_at",
                (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339(),
            )
            .await?;
            issue(&connection, &table, "key", "new", &hash, policy()?).await?;
            check(
                !finish_attempt(&connection, &table, "key", "old", true).await?,
                "旧challenge消费新挑战",
            )?;
            check(
                finish_attempt(&connection, &table, "key", "new", true).await?,
                "新challenge不可消费",
            )?;
            check(
                !finish_attempt(&connection, &table, "key", "new", true).await?,
                "重复消费成功",
            )
        })
    })
    .await
}

#[tokio::test]
async fn file_two_stores_consume_once_and_service_binds_purpose_identity() -> Result<()> {
    owned(true, |first, second| {
        Box::pin(async move {
            let one = DatabaseVerificationCodeStore::new(first.clone());
            let two = DatabaseVerificationCodeStore::new(second.clone());
            let hash =
                crate::auth::EncodedPasswordHash::make(passwords()?.as_ref(), "123456").await?;
            one.issue("concurrent", "challenge", &hash, policy()?)
                .await?;
            let (a, b) = tokio::join!(
                one.finish_attempt("concurrent", "challenge", true),
                two.finish_attempt("concurrent", "challenge", true)
            );
            check(
                usize::from(a?) + usize::from(b?) == 1,
                "两个store未实现单次消费",
            )?;
            let service = VerificationCodes::new(one, passwords()?);
            let issued = service
                .issue("email.verify", "owned@example.test", policy()?)
                .await?;
            check(
                !service
                    .verify("password.reset", "owned@example.test", issued.code())
                    .await?,
                "用途未隔离",
            )?;
            check(
                !service
                    .verify("email.verify", "other@example.test", issued.code())
                    .await?,
                "身份未隔离",
            )?;
            check(
                service
                    .verify("email.verify", "owned@example.test", issued.code())
                    .await?,
                "真实Argon2验证码不可消费",
            )?;
            check(
                !service
                    .verify("email.verify", "owned@example.test", issued.code())
                    .await?,
                "服务重复消费成功",
            )
        })
    })
    .await
}

#[tokio::test]
async fn file_offset_microsecond_expiry_and_prune_batch_preserve_future() -> Result<()> {
    owned(true, |db, _| Box::pin(async move {
        let connection = db.connection()?;
        let table = db.quoted_table_name("verification_codes")?;
        let hash = crate::auth::EncodedPasswordHash::make(passwords()?.as_ref(), "123456").await?;
        issue(&connection, &table, "offset", "challenge", &hash, policy()?).await?;
        let zone = chrono::FixedOffset::east_opt(8*3600).ok_or_else(|| Error::custom("test_offset", "无效offset"))?;
        let past = (Utc::now()-chrono::Duration::microseconds(1)).with_timezone(&zone)
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
        update_time(db, "offset", "expires_at", past).await?;
        check(load_active(&connection, &table, "offset").await?.is_none(), "offset期限被词法比较延长")?;
        check(!finish_attempt(&connection, &table, "offset", "challenge", true).await?, "精确过期仍可消费")?;
        connection.execute_raw(Statement::from_string(DbBackend::Sqlite,
            format!("DELETE FROM {table}"))).await.map_err(Error::internal)?;
        let now = Utc::now();
        // 单条递归CTE准备513个到期项，验证每轮上限，而非循环512次独立写事务。
        connection.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<513) INSERT INTO {table}(key_hash,challenge_id,code_hash,code_digits,attempt_count,max_attempts,issued_at,expires_at,resend_after_at) SELECT 'expired-'||x,'challenge',?,6,0,2,?,?,? FROM n"),
            [hash.as_str().to_owned().into(),utc_value(DbBackend::Sqlite,now),utc_value(DbBackend::Sqlite,now-chrono::Duration::seconds(1)),utc_value(DbBackend::Sqlite,now)]))
            .await.map_err(Error::internal)?;
        issue(&connection, &table, "future", "challenge", &hash, policy()?).await?;
        let future = (Utc::now()+chrono::Duration::seconds(60)).with_timezone(&zone)
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
        update_time(db, "future", "expires_at", future).await?;
        prune(&connection, &table).await?;
        check(scalar(db, format!("SELECT COUNT(*) AS value FROM {table} WHERE key_hash LIKE 'expired-%'")).await? == 1, "prune未遵守512上限")?;
        check(load_active(&connection, &table, "future").await?.is_some(), "prune删除未到期offset挑战")?;
        prune(&connection, &table).await?;
        check(scalar(db, format!("SELECT COUNT(*) AS value FROM {table}")).await? == 1, "第二轮prune未清理剩余到期项")
    })).await
}

#[tokio::test]
async fn file_prune_orders_instants_without_offset_text_starving_expired_rows() -> Result<()> {
    owned(true, |db, _| {
        Box::pin(async move {
            let connection = db.connection()?;
            let table = db.quoted_table_name("verification_codes")?;
            let hash = crate::auth::EncodedPasswordHash::make(passwords()?.as_ref(), "123456").await?;
            let now = Utc::now();
            let zone = chrono::FixedOffset::west_opt(12 * 3600)
                .ok_or_else(|| Error::custom("test_offset", "无效offset"))?;
            // 未来绝对时刻的负offset文本早于过期UTC文本，文本LIMIT会永久挡住到期项。
            let future = (now + chrono::Duration::hours(1))
                .with_timezone(&zone)
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
            let past = (now - chrono::Duration::minutes(1))
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
            check(future < past, "fixture未形成offset文本排序反转")?;
            connection
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<513) INSERT INTO {table}(key_hash,challenge_id,code_hash,code_digits,attempt_count,max_attempts,issued_at,expires_at,resend_after_at) SELECT 'future-'||x,'challenge',?,6,0,2,?,?,? FROM n"),
                    [hash.as_str().to_owned().into(), utc_value(DbBackend::Sqlite, now), future.into(), utc_value(DbBackend::Sqlite, now)],
                ))
                .await
                .map_err(Error::internal)?;
            connection
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!("INSERT INTO {table}(key_hash,challenge_id,code_hash,code_digits,attempt_count,max_attempts,issued_at,expires_at,resend_after_at) VALUES ('expired','challenge',?,6,0,2,?,?,?)"),
                    [hash.as_str().to_owned().into(), utc_value(DbBackend::Sqlite, now), past.into(), utc_value(DbBackend::Sqlite, now)],
                ))
                .await
                .map_err(Error::internal)?;
            prune(&connection, &table).await?;
            check(
                scalar(db, format!("SELECT COUNT(*) AS value FROM {table} WHERE key_hash='expired'")).await? == 0,
                "offset未来记录饿死到期项",
            )?;
            check(
                scalar(db, format!("SELECT COUNT(*) AS value FROM {table} WHERE key_hash LIKE 'future-%'")).await? == 513,
                "首轮prune删除尚未到期offset记录",
            )?;
            prune(&connection, &table).await?;
            check(
                scalar(db, format!("SELECT COUNT(*) AS value FROM {table}")).await? == 513,
                "第二轮prune删除尚未到期offset记录",
            )
        })
    })
    .await
}

#[tokio::test]
async fn file_prune_scans_across_future_page_and_accepts_fifteen_hour_offset() -> Result<()> {
    owned(true, |db, _| Box::pin(async move {
        let connection = db.connection()?;
        let table = db.quoted_table_name("verification_codes")?;
        let hash = crate::auth::EncodedPasswordHash::make(passwords()?.as_ref(), "123456").await?;
        let now = Utc::now();
        let zone = chrono::FixedOffset::east_opt(15 * 3600)
            .ok_or_else(|| Error::custom("test_offset", "无效offset"))?;
        let future = (now + chrono::Duration::hours(1)).with_timezone(&zone)
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
        connection.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<513) INSERT INTO {table}(key_hash,challenge_id,code_hash,code_digits,attempt_count,max_attempts,issued_at,expires_at,resend_after_at) SELECT 'a-future-'||x,'challenge',?,6,0,2,?,?,? FROM n"),
            [hash.as_str().to_owned().into(),utc_value(DbBackend::Sqlite,now),future.into(),utc_value(DbBackend::Sqlite,now)]))
            .await.map_err(Error::internal)?;
        connection.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
            format!("INSERT INTO {table}(key_hash,challenge_id,code_hash,code_digits,attempt_count,max_attempts,issued_at,expires_at,resend_after_at) VALUES ('z-expired','challenge',?,6,0,2,?,?,?)"),
            [hash.as_str().to_owned().into(),utc_value(DbBackend::Sqlite,now),utc_value(DbBackend::Sqlite,now-chrono::Duration::minutes(1)),utc_value(DbBackend::Sqlite,now)]))
            .await.map_err(Error::internal)?;
        prune(&connection,&table).await?;
        check(scalar(db,format!("SELECT COUNT(*) AS value FROM {table} WHERE key_hash='z-expired'")).await?==0,
            "未来主键页挡住后续到期项")?;
        check(scalar(db,format!("SELECT COUNT(*) AS value FROM {table}")).await?==513,
            "+15小时未来项被删除")?;
        prune(&connection,&table).await?;
        check(scalar(db,format!("SELECT COUNT(*) AS value FROM {table}")).await?==513,
            "重复prune删除未来项")?;
        let past = (Utc::now()-chrono::Duration::minutes(1)).with_timezone(&zone)
            .to_rfc3339_opts(chrono::SecondsFormat::Micros,false);
        update_time(db,"a-future-513","expires_at",past).await?;
        prune(&connection,&table).await?;
        check(scalar(db,format!("SELECT COUNT(*) AS value FROM {table} WHERE key_hash='a-future-513'")).await?==0,
            "+15小时已过期项被跳过")?;
        check(scalar(db,format!("SELECT COUNT(*) AS value FROM {table}")).await?==512,
            "清理+15小时到期项误删其余未来项")
    })).await
}
