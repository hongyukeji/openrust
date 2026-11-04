//! SQLite MFA前置限流的状态竞态与失败关闭回归。
use super::*;
use crate::auth::{
    AuthFlows, DatabaseVerificationCodeStore, JwtConfig, JwtTokens, PasswordConfig, RefreshTokens,
    VerificationCodes,
};
use crate::config::EncryptionConfig;
use crate::core::{RateLimitBucket, RateLimitFuture, RateLimitStore};
use crate::db::{DatabaseRateLimitMigration, DatabaseRateLimitStore};
use crate::testing::TestDatabase;
use base64::engine::general_purpose::STANDARD;
use std::collections::BTreeMap;

struct MfaMigrator;
impl MigratorTrait for MfaMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRefreshTokenMigration),
            Box::new(crate::auth::AuthVerificationCodeMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(AuthMfaMigration),
        ]
    }
}
fn check(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom("sqlite_mfa_contract", message))
    }
}

/// 真正消费数据库配额后，制造预采样与锁内复核之间的因子状态变化。
struct RemoveFactorOnLogin {
    db: Db,
    inner: DatabaseRateLimitStore,
}
impl RateLimitStore for RemoveFactorOnLogin {
    fn consume<'a>(
        &'a self,
        key: &'a str,
        window: Duration,
    ) -> RateLimitFuture<'a, Result<RateLimitBucket>> {
        Box::pin(async move {
            let bucket = self.inner.consume(key, window).await?;
            if key.starts_with("auth:mfa:login:") {
                let connection = self.db.connection()?;
                connection
                    .execute_raw(Statement::from_string(
                        DbBackend::Sqlite,
                        "UPDATE mfa_preflight_observation SET calls=calls+1".to_owned(),
                    ))
                    .await
                    .map_err(Error::internal)?;
                let factors = self.db.quoted_table_name("auth_mfa_factors")?;
                connection
                    .execute_raw(Statement::from_string(
                        DbBackend::Sqlite,
                        format!("DELETE FROM {factors}"),
                    ))
                    .await
                    .map_err(Error::internal)?;
            }
            Ok(bucket)
        })
    }
}
fn passwords() -> Result<Passwords> {
    Passwords::new(PasswordConfig {
        memory_kib: 19_456,
        iterations: 1,
        parallelism: 1,
    })
}
fn encryptor() -> Result<Encryptor> {
    Encryptor::new(EncryptionConfig::new(
        "test",
        BTreeMap::from([("test".to_owned(), STANDARD.encode([7_u8; 32]))]),
    ))
}
fn jwt() -> Result<JwtTokens> {
    JwtTokens::new(JwtConfig::new(
        "https://issuer.example.test",
        "sqlite-mfa-test",
        "test",
        BTreeMap::from([("test".to_owned(), STANDARD.encode([9_u8; 64]))]),
    ))
}
async fn count(db: &Db, table: &str) -> Result<i64> {
    db.connection()?
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT COUNT(*) AS value FROM {table}"),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::custom("sqlite_mfa_row", "缺少计数行"))?
        .try_get("", "value")
        .map_err(Error::internal)
}
async fn fixture(db: &Db) -> Result<i64> {
    let hash = passwords()?.hash("owned-test-password").await?;
    let users = db.quoted_table_name("users")?;
    let now = dialect::utc_value(DbBackend::Sqlite, Utc::now());
    let inserted = db.connection()?.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
        format!("INSERT INTO {users}(email,password_hash,email_verified_at,created_at,updated_at) VALUES (?,?,?,?,?)"),
        ["owned-mfa@example.test".into(),hash.as_str().to_owned().into(),now.clone(),now.clone(),now.clone()]))
        .await.map_err(Error::internal)?;
    let user = <i64 as std::convert::TryFrom<u64>>::try_from(inserted.last_insert_id())
        .map_err(Error::internal)?;
    let encrypted =
        encryptor()?.encrypt_with_context(&[3_u8; 20], factor_context(user).as_bytes())?;
    let factors = db.quoted_table_name("auth_mfa_factors")?;
    db.connection()?.execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite,
        format!("INSERT INTO {factors}(user_id,encrypted_secret,last_used_step,enabled_at) VALUES (?,?,?,?)"),
        [user.into(),encrypted.into(),0_i64.into(),now])).await.map_err(Error::internal)?;
    Ok(user)
}

#[tokio::test]
async fn login_factor_change_after_preflight_fails_without_replaying_consumption() -> Result<()> {
    let guard = TestDatabase::sqlite_file::<MfaMigrator>().await?;
    let db = Db::default();
    let outcome: Result<()> = async {
        db.initialize(guard.connection()?)?;
        let user = fixture(&db).await?;
        let connection = db.connection()?;
        connection
            .execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                "CREATE TABLE mfa_preflight_observation(calls INTEGER NOT NULL)".to_owned(),
            ))
            .await
            .map_err(Error::internal)?;
        connection
            .execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT INTO mfa_preflight_observation VALUES(0)".to_owned(),
            ))
            .await
            .map_err(Error::internal)?;
        let limiter = RateLimiter::new(RemoveFactorOnLogin {
            db: db.clone(),
            inner: DatabaseRateLimitStore::new(db.clone()),
        });
        let mfa = MfaService::new(db.clone(), encryptor()?, limiter.clone());
        let flows = AuthFlows::new(
            db.clone(),
            passwords()?,
            jwt()?,
            RefreshTokens::new(db.clone()),
            VerificationCodes::new(DatabaseVerificationCodeStore::new(db.clone()), passwords()?),
            limiter,
        )
        .with_mfa(Some(mfa));
        let result = flows
            .begin_login(
                "owned-mfa@example.test",
                "owned-test-password",
                "owned-source",
            )
            .await;
        check(
            matches!(result, Err(ref error) if error.code()=="auth_mfa_state_changed"),
            "状态竞态未安全拒绝登录",
        )?;
        let calls: i64 = connection
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT calls FROM mfa_preflight_observation".to_owned(),
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("sqlite_mfa_row", "缺少观察行"))?
            .try_get("", "calls")
            .map_err(Error::internal)?;
        check(calls == 1, "登录重放了前置限流副作用")?;
        let rate_table = db.quoted_table_name("rate_limits")?;
        let key = format!("auth:mfa:login:owned-source:{}", digest(&user.to_string()));
        let hits: i64 = connection
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT hit_count FROM {rate_table} WHERE key_hash=?"),
                [digest(&key).into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("sqlite_mfa_row", "缺少真实限流行"))?
            .try_get("", "hit_count")
            .map_err(Error::internal)?;
        check(hits == 1, "失败登录未保留且仅保留一次真实配额消费")?;
        check(
            count(&db, &db.quoted_table_name("auth_mfa_factors")?).await? == 0,
            "限流store真实写入未保留",
        )?;
        check(
            count(&db, &db.quoted_table_name("auth_mfa_challenges")?).await? == 0,
            "失败登录仍签发challenge",
        )?;
        check(
            count(&db, &db.quoted_table_name("auth_refresh_tokens")?).await? == 0,
            "失败登录仍签发持久凭据",
        )?;
        Ok(())
    }
    .await;
    drop(db);
    let cleanup = guard.close().await;
    outcome?;
    cleanup
}
