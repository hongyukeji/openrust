//! 真实SQLite只读attach授权：当前raw与完整家族一致，失败也不得暗中撤销。
use super::*;
use crate::{
    auth::{credential::CredentialFactorContext, fortify::CredentialFactorMigration},
    database::sea_orm_migration::{MigrationTrait, MigratorTrait},
    support::BoxFuture,
    testing::TestDatabase,
};

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            Box::new(crate::auth::AuthRefreshTokenMigration),
            Box::new(CredentialFactorMigration::new()),
        ]
    }
}
fn context(user_id: i64) -> CredentialFactorContext {
    CredentialFactorContext {
        scope: None,
        version: 1,
        identifier: user_id.into(),
        source: "refresh-attach-source".into(),
        generation: "refresh-attach-generation".into(),
        factor: None,
    }
}
async fn owned<F>(callback: F) -> Result<()>
where
    F: FnOnce(Db) -> BoxFuture<'static, Result<()>>,
{
    let database = TestDatabase::sqlite_file::<Migrator>().await?;
    let db = Db::default();
    let outcome: Result<()> = async {
        db.initialize(database.connection()?)?;
        for id in [42_i64, 43_i64] {
            let now = crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now());
            db.table("users")?
                .insert(vec![
                    ("id", id.into()),
                    ("email", format!("attach-{id}@example.test").into()),
                    ("password_hash", "unused".into()),
                    ("created_at", now.clone()),
                    ("updated_at", now),
                ])
                .await?;
        }
        callback(db.clone()).await
    }
    .await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}
async fn issue(db: &Db) -> Result<ManagedRefreshIssue> {
    let service = RefreshTokens::new(db.clone());
    let expiry = DateTime::<Utc>::from_timestamp(Utc::now().timestamp() + 3_600, 123_456_000)
        .ok_or(Error::Unauthorized)?;
    db.transaction_for_update(move |tx| {
        Box::pin(async move {
            service
                .issue_context_in_transaction(tx, 42, expiry, &context(42))
                .await
        })
    })
    .await
}
type Attachment = (String, DateTime<Utc>, CredentialFactorContext);
async fn attach(db: &Db, user: i64, raw: &str) -> Result<Result<Attachment>> {
    let service = RefreshTokens::new(db.clone());
    let db_copy = db.clone();
    let raw = raw.to_owned();
    db.transaction_for_update(move |tx| {
        Box::pin(async move {
            lock_user(tx, &db_copy, user).await?;
            // 即使helper拒绝也提交外层事务，避免回滚掩盖helper意外写入的撤销/消费。
            Ok(service.attach_context_in_transaction(tx, user, &raw).await)
        })
    })
    .await
}
async fn rotate(db: &Db, raw: &str) -> Result<ManagedRefreshRotation> {
    let service = RefreshTokens::new(db.clone());
    let raw = raw.to_owned();
    db.transaction_for_update(move |tx| {
        Box::pin(async move {
            let locked = service
                .lock_context_in_transaction(tx, &raw)
                .await?
                .ok_or(Error::Unauthorized)?;
            service.rotate_context_in_transaction(locked).await
        })
    })
    .await
}
#[derive(PartialEq, Eq)]
struct RowState {
    owner: i64,
    family: String,
    hash: String,
    context: Option<String>,
    expiry: DateTime<Utc>,
    consumed: Option<DateTime<Utc>>,
    revoked: Option<DateTime<Utc>>,
    replaced: Option<String>,
    created: DateTime<Utc>,
}
async fn snapshot(db: &Db) -> Result<Vec<RowState>> {
    db.table("auth_refresh_tokens")?
        .order_by("token_hash", crate::database::query::SortDirection::Asc)?
        .get()
        .await?
        .iter()
        .map(|row| {
            Ok(RowState {
                owner: row.try_get("", "user_id").map_err(Error::internal)?,
                family: row.try_get("", "family_id").map_err(Error::internal)?,
                hash: row.try_get("", "token_hash").map_err(Error::internal)?,
                context: row.try_get("", "factor_context").map_err(Error::internal)?,
                expiry: row.try_get("", "expires_at").map_err(Error::internal)?,
                consumed: row.try_get("", "consumed_at").map_err(Error::internal)?,
                revoked: row.try_get("", "revoked_at").map_err(Error::internal)?,
                replaced: row
                    .try_get("", "replaced_by_hash")
                    .map_err(Error::internal)?,
                created: row.try_get("", "created_at").map_err(Error::internal)?,
            })
        })
        .collect()
}
async fn rejected_without_write(db: &Db, user: i64, raw: &str) -> Result<()> {
    let before = snapshot(db).await?;
    assert!(attach(db, user, raw).await?.is_err());
    assert!(snapshot(db).await? == before);
    Ok(())
}
#[tokio::test]
async fn attach_is_repeatable_read_only_and_consumed_ancestor_does_not_revoke_live_successor()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let first = issue(&db).await?;
            let before = snapshot(&db).await?;
            for _ in 0..2 {
                let (family, expiry, verified) = attach(&db, 42, first.token.expose()).await??;
                assert!(family == first.family_id && verified == context(42));
                assert!(before.first().is_some_and(|row| row.expiry == expiry));
            }
            assert!(snapshot(&db).await? == before);
            let next = rotate(&db, first.token.expose()).await?;
            rejected_without_write(&db, 42, first.token.expose()).await?;
            let before = snapshot(&db).await?;
            let (family, expiry, verified) = attach(&db, 42, next.token.expose()).await??;
            assert!(
                family == first.family_id && expiry == next.expires_at && verified == context(42)
            );
            assert!(snapshot(&db).await? == before && before.len() == 2);
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn attach_rejects_wrong_owner_null_or_corrupt_context_and_expired_current_raw_without_writes()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let issued = issue(&db).await?;
            let raw = issued.token.expose();
            rejected_without_write(&db, 43, raw).await?;
            rejected_without_write(&db, 42, "").await?;
            rejected_without_write(&db, 42, &"x".repeat(1_025)).await?;
            for encoded in [None, Some("{}"), Some("not-json")] {
                db.table("auth_refresh_tokens")?
                    .where_eq("token_hash", sha256_hex(raw))?
                    .update(vec![("factor_context", encoded.map(str::to_owned).into())])
                    .await?;
                rejected_without_write(&db, 42, raw).await?;
            }
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", sha256_hex(raw))?
                .update(vec![("factor_context", context(42).encode(42)?.into())])
                .await?;
            let expired = (Utc::now() - chrono::Duration::seconds(1))
                .with_timezone(
                    &chrono::FixedOffset::east_opt(8 * 3_600).ok_or(Error::Unauthorized)?,
                )
                .to_rfc3339();
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", sha256_hex(raw))?
                .update(vec![("expires_at", expired.into())])
                .await?;
            rejected_without_write(&db, 42, raw).await?;
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn attach_checks_all_historical_family_contexts_owners_revocations_and_exact_expiry()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let first = issue(&db).await?;
            let next = rotate(&db, first.token.expose()).await?;
            let old_hash = sha256_hex(first.token.expose());
            let mut wrong = context(42);
            wrong.generation = "other-generation".into();
            for bad in [
                sea_orm::Value::String(None),
                "{bad".into(),
                wrong.encode(42)?.into(),
            ] {
                db.table("auth_refresh_tokens")?
                    .where_eq("token_hash", old_hash.clone())?
                    .update(vec![("factor_context", bad)])
                    .await?;
                rejected_without_write(&db, 42, next.token.expose()).await?;
            }
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", old_hash.clone())?
                .update(vec![
                    ("factor_context", context(42).encode(42)?.into()),
                    ("user_id", 43_i64.into()),
                ])
                .await?;
            rejected_without_write(&db, 42, next.token.expose()).await?;
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", old_hash.clone())?
                .update(vec![
                    ("user_id", 42_i64.into()),
                    (
                        "expires_at",
                        crate::database::dialect::utc_value(
                            DbBackend::Sqlite,
                            next.expires_at + chrono::Duration::hours(1),
                        ),
                    ),
                ])
                .await?;
            rejected_without_write(&db, 42, next.token.expose()).await?;
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", old_hash.clone())?
                .update(vec![
                    (
                        "expires_at",
                        crate::database::dialect::utc_value(DbBackend::Sqlite, next.expires_at),
                    ),
                    (
                        "revoked_at",
                        crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now()),
                    ),
                ])
                .await?;
            rejected_without_write(&db, 42, next.token.expose()).await?;
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn attach_rejects_multiple_live_leaves_revoked_leaf_and_invalid_family_without_writes()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let first = issue(&db).await?;
            let next = rotate(&db, first.token.expose()).await?;
            let old_hash = sha256_hex(first.token.expose());
            let current_hash = sha256_hex(next.token.expose());
            let consumed: Option<DateTime<Utc>> = db
                .table("auth_refresh_tokens")?
                .where_eq("token_hash", old_hash.clone())?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?
                .try_get("", "consumed_at")
                .map_err(Error::internal)?;
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", old_hash.clone())?
                .update(vec![("consumed_at", sea_orm::Value::String(None))])
                .await?;
            rejected_without_write(&db, 42, next.token.expose()).await?;
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", old_hash)?
                .update(vec![(
                    "consumed_at",
                    crate::database::dialect::utc_value(
                        DbBackend::Sqlite,
                        consumed.ok_or(Error::Unauthorized)?,
                    ),
                )])
                .await?;
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", current_hash.clone())?
                .update(vec![("family_id", "not-a-family".into())])
                .await?;
            rejected_without_write(&db, 42, next.token.expose()).await?;
            db.table("auth_refresh_tokens")?
                .where_eq("token_hash", current_hash)?
                .update(vec![
                    ("family_id", first.family_id.into()),
                    (
                        "revoked_at",
                        crate::database::dialect::utc_value(DbBackend::Sqlite, Utc::now()),
                    ),
                ])
                .await?;
            rejected_without_write(&db, 42, next.token.expose()).await?;
            Ok(())
        })
    })
    .await
}
