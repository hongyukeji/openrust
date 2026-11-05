//! 真实SQLite账户来源验证三字段CAS，而不是内存读后写替身。
use super::*;
use crate::{
    db::sea_orm::ConnectionTrait,
    db::sea_orm_migration::{MigrationTrait, MigratorTrait},
    testing::TestDatabase,
};
use chrono::TimeZone;
use serde_json::json;

struct NoMigrations;
impl MigratorTrait for NoMigrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

async fn owned<F>(callback: F) -> Result<()>
where
    F: FnOnce(Db) -> BoxFuture<'static, Result<()>>,
{
    let database = TestDatabase::sqlite_file::<NoMigrations>().await?;
    let db = Db::default();
    let outcome = async {
        db.initialize(database.connection()?)?;
        callback(db.clone()).await
    }
    .await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}

async fn schema(db: &Db, table: &str, key_type: &str) -> Result<()> {
    let table = db.quoted_table_name(table)?;
    // key_type仅由本测试固定调用，不接收应用输入。
    db.connection()?.execute_unprepared(&format!("CREATE TABLE {table} (account_key {key_type} PRIMARY KEY,two_factor_secret TEXT COLLATE NOCASE,two_factor_recovery_codes TEXT COLLATE NOCASE,two_factor_confirmed_at datetime)")).await.map_err(Error::internal)?;
    Ok(())
}

async fn insert(db: &Db, table: &str, identifier: SqlValue) -> Result<()> {
    db.table(table)?
        .insert(vec![("account_key", identifier)])
        .await?;
    Ok(())
}

fn pending(recovery: &str) -> TwoFactorState {
    TwoFactorState {
        secret: Some("Cipher-ABc".into()),
        recovery_codes: Some(recovery.into()),
        confirmed_at: None,
    }
}

#[tokio::test]
async fn numeric_and_uuid_accounts_keep_separate_rows_and_official_null_fields() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            schema(&db, "numeric_accounts", "INTEGER").await?;
            schema(&db, "uuid_accounts", "TEXT").await?;
            insert(&db, "numeric_accounts", 7_i64.into()).await?;
            insert(&db, "numeric_accounts", 8_i64.into()).await?;
            insert(
                &db,
                "uuid_accounts",
                "tenant|550e8400-e29b-41d4-a716-446655440000".into(),
            )
            .await?;
            let number = DatabaseTwoFactorState::new(
                db.clone(),
                "numeric_accounts",
                "account_key",
                json!(7),
            )?;
            let neighbor = DatabaseTwoFactorState::new(
                db.clone(),
                "numeric_accounts",
                "account_key",
                json!(8),
            )?;
            let uuid = DatabaseTwoFactorState::new(
                db.clone(),
                "uuid_accounts",
                "account_key",
                json!("tenant|550e8400-e29b-41d4-a716-446655440000"),
            )?;
            let empty = TwoFactorState::default();
            assert_eq!(number.load().await?, empty);
            let enabled = pending("Codes-AbC");
            assert!(number.compare_and_set(&empty, &enabled).await?);
            assert_eq!(number.load().await?, enabled);
            assert_eq!(neighbor.load().await?, empty);
            assert_eq!(uuid.load().await?, empty);
            let uuid_state = pending("UUID-Codes");
            assert!(uuid.compare_and_set(&empty, &uuid_state).await?);
            assert_eq!(uuid.load().await?, uuid_state);
            assert_eq!(number.load().await?, enabled);
            assert_eq!(format!("{uuid:?}"), "DatabaseTwoFactorState([REDACTED])");
            for bad in [
                Value::Null,
                json!(true),
                json!({"id":7}),
                json!([7]),
                json!(1.5),
                json!(""),
            ] {
                assert!(
                    DatabaseTwoFactorState::new(db.clone(), "numeric_accounts", "account_key", bad)
                        .is_err()
                );
            }
            assert!(
                DatabaseTwoFactorState::new(
                    db.clone(),
                    "numeric_accounts",
                    "account_key OR 1=1",
                    json!(7)
                )
                .is_err()
            );
            assert!(
                DatabaseTwoFactorState::new(
                    db,
                    "numeric_accounts; DROP TABLE users",
                    "account_key",
                    json!(7)
                )
                .is_err()
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn snapshot_conditions_are_byte_exact_and_confirmation_and_disable_are_atomic() -> Result<()>
{
    owned(|db| {
        Box::pin(async move {
            schema(&db, "accounts", "INTEGER").await?;
            insert(&db, "accounts", 7_i64.into()).await?;
            let store = DatabaseTwoFactorState::new(db, "accounts", "account_key", json!(7))?;
            let empty = TwoFactorState::default();
            let enabled = pending("Codes-AbC");
            assert!(store.compare_and_set(&empty, &enabled).await?);
            for expected in [
                TwoFactorState {
                    secret: Some("cipher-abc".into()),
                    ..enabled.clone()
                },
                TwoFactorState {
                    recovery_codes: Some("codes-abc".into()),
                    ..enabled.clone()
                },
                TwoFactorState {
                    recovery_codes: Some("Codes-AbC ".into()),
                    ..enabled.clone()
                },
                TwoFactorState {
                    secret: None,
                    ..enabled.clone()
                },
                TwoFactorState {
                    recovery_codes: None,
                    ..enabled.clone()
                },
            ] {
                assert!(!store.compare_and_set(&expected, &empty).await?);
                assert_eq!(store.load().await?, enabled);
            }
            let timestamp = Utc
                .timestamp_opt(1_800_000_000, 123_456_000)
                .single()
                .ok_or_else(|| state_error("test_timestamp_invalid", "测试时间无效"))?;
            let confirmed = TwoFactorState {
                confirmed_at: Some(timestamp),
                ..enabled.clone()
            };
            assert!(store.compare_and_set(&enabled, &confirmed).await?);
            assert_eq!(store.load().await?, confirmed);
            assert!(!store.compare_and_set(&enabled, &empty).await?);
            let stale = TwoFactorState {
                confirmed_at: Some(timestamp + chrono::Duration::microseconds(1)),
                ..confirmed.clone()
            };
            assert!(!store.compare_and_set(&stale, &empty).await?);
            assert!(store.compare_and_set(&confirmed, &empty).await?);
            assert_eq!(store.load().await?, empty);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn concurrent_recovery_replacement_has_exactly_one_winner() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            schema(&db, "accounts", "INTEGER").await?;
            insert(&db, "accounts", 7_i64.into()).await?;
            let store = DatabaseTwoFactorState::new(db, "accounts", "account_key", json!(7))?;
            let expected = pending("same-encrypted-recovery-list");
            assert!(
                store
                    .compare_and_set(&TwoFactorState::default(), &expected)
                    .await?
            );
            let first = pending("first-replacement-list");
            let second = pending("second-replacement-list");
            let clone = store.clone();
            let (a, b) = tokio::join!(
                store.compare_and_set(&expected, &first),
                clone.compare_and_set(&expected, &second)
            );
            let (a, b) = (a?, b?);
            assert_ne!(a, b, "同一恢复码快照只能由一个事务替换");
            assert_eq!(store.load().await?, if a { first } else { second });
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn missing_account_columns_invalid_values_and_storage_failures_are_not_empty_state()
-> Result<()> {
    owned(|db| Box::pin(async move {
        schema(&db, "accounts", "INTEGER").await?;
        let absent = DatabaseTwoFactorState::new(db.clone(), "accounts", "account_key", json!(7))?;
        let error = absent.load().await.err().ok_or_else(|| state_error("test_missing_error", "缺失账户被当作空状态"))?;
        assert_eq!(error.code(), "fortify_account_missing");
        assert!(!absent.compare_and_set(&TwoFactorState::default(), &pending("unused")).await?);
        db.connection()?.execute_unprepared("CREATE TABLE missing_columns (id INTEGER PRIMARY KEY,two_factor_secret TEXT); INSERT INTO missing_columns(id) VALUES(7)").await.map_err(Error::internal)?;
        let missing = DatabaseTwoFactorState::new(db.clone(), "missing_columns", "id", json!(7))?;
        let error = missing.load().await.err().ok_or_else(|| state_error("test_missing_error", "缺列被当作null"))?;
        assert_eq!(error.code(), "fortify_state_column_missing");
        assert!(missing.compare_and_set(&TwoFactorState::default(), &pending("unused")).await.is_err());
        insert(&db, "accounts", 7_i64.into()).await?;
        db.connection()?.execute_unprepared("UPDATE accounts SET two_factor_confirmed_at='not-a-timestamp' WHERE account_key=7").await.map_err(Error::internal)?;
        assert!(absent.load().await.is_err());
        db.connection()?.execute_unprepared("UPDATE accounts SET two_factor_confirmed_at=NULL,two_factor_secret=X'FF' WHERE account_key=7").await.map_err(Error::internal)?;
        assert!(absent.load().await.is_err());
        db.connection()?.execute_unprepared("DROP TABLE accounts").await.map_err(Error::internal)?;
        assert!(absent.load().await.is_err());
        assert!(absent.compare_and_set(&TwoFactorState::default(), &pending("unused")).await.is_err());
        Ok(())
    })).await
}

#[tokio::test]
async fn ambiguous_identifier_update_rolls_back_all_matched_accounts() -> Result<()> {
    owned(|db| Box::pin(async move {
        db.connection()?.execute_unprepared("CREATE TABLE accounts (account_key INTEGER,two_factor_secret TEXT,two_factor_recovery_codes TEXT,two_factor_confirmed_at datetime); INSERT INTO accounts(account_key) VALUES(7),(7)").await.map_err(Error::internal)?;
        let store = DatabaseTwoFactorState::new(db.clone(), "accounts", "account_key", json!(7))?;
        let error = store.load().await.err().ok_or_else(|| state_error("test_missing_error", "不唯一标识意外返回首行"))?;
        assert_eq!(error.code(), "fortify_account_identifier_ambiguous");
        let error = store.compare_and_set(&TwoFactorState::default(), &pending("must-rollback")).await.err().ok_or_else(|| state_error("test_missing_error", "多行更新未失败"))?;
        assert_eq!(error.code(), "fortify_account_identifier_ambiguous");
        let rows = db.table("accounts")?.get().await?;
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.try_get::<Option<String>>("", SECRET).map_err(Error::internal)?, None);
            assert_eq!(row.try_get::<Option<String>>("", RECOVERY).map_err(Error::internal)?, None);
        }
        Ok(())
    })).await
}

#[tokio::test]
async fn state_load_uses_primary_instead_of_a_stale_read_replica() -> Result<()> {
    let primary = TestDatabase::sqlite_file::<NoMigrations>().await?;
    let replica = match TestDatabase::sqlite_file::<NoMigrations>().await {
        Ok(database) => database,
        Err(error) => {
            let closed = primary.close().await;
            closed?;
            return Err(error);
        }
    };
    let db = Db::default();
    let outcome: Result<()> = async {
        let primary_seed = Db::default();
        primary_seed.initialize(primary.connection()?)?;
        let replica_seed = Db::default();
        replica_seed.initialize(replica.connection()?)?;
        for source in [&primary_seed, &replica_seed] {
            schema(source, "accounts", "INTEGER").await?;
            insert(source, "accounts", 7_i64.into()).await?;
        }
        primary_seed
            .table("accounts")?
            .update(vec![(SECRET, "primary-secret".into())])
            .await?;
        replica_seed
            .table("accounts")?
            .update(vec![(SECRET, "stale-replica-secret".into())])
            .await?;
        db.initialize_with_replicas(primary.connection()?, vec![replica.connection()?])?;
        let store = DatabaseTwoFactorState::new(db.clone(), "accounts", "account_key", json!(7))?;
        assert_eq!(
            store.load().await?.secret.as_deref(),
            Some("primary-secret")
        );
        // 作用域退出后，普通可延迟查询仍按原Db策略读副本。
        let row = db
            .table("accounts")?
            .first()
            .await?
            .ok_or_else(|| state_error("test_missing_row", "副本fixture缺少账户"))?;
        assert_eq!(
            row.try_get::<String>("", SECRET).map_err(Error::internal)?,
            "stale-replica-secret"
        );
        Ok(())
    }
    .await;
    drop(db);
    let closed_primary = primary.close().await;
    let closed_replica = replica.close().await;
    outcome?;
    closed_primary?;
    closed_replica
}

#[tokio::test]
async fn sqlite_historical_timestamp_text_keeps_instant_equality_without_overwriting_changes()
-> Result<()> {
    owned(|db| Box::pin(async move {
        schema(&db, "accounts", "INTEGER").await?;
        insert(&db, "accounts", 7_i64.into()).await?;
        let store = DatabaseTwoFactorState::new(db.clone(), "accounts", "account_key", json!(7))?;
        db.connection()?.execute_unprepared("UPDATE accounts SET two_factor_confirmed_at=CURRENT_TIMESTAMP WHERE account_key=7").await.map_err(Error::internal)?;
        let current_timestamp = store.load().await?;
        assert!(current_timestamp.confirmed_at.is_some());
        let replacement = TwoFactorState { secret: Some("history-secret".into()), recovery_codes: Some("history-codes".into()), ..current_timestamp.clone() };
        assert!(store.compare_and_set(&current_timestamp, &replacement).await?);
        assert_eq!(store.load().await?, replacement);

        let timestamp = Utc.timestamp_opt(1_800_000_000, 123_456_000).single().ok_or_else(|| state_error("test_timestamp_invalid", "测试时间无效"))?;
        let naive = timestamp.naive_utc().format("%F %T%.f").to_string();
        db.table("accounts")?.where_eq("account_key", 7_i64)?.update(vec![(CONFIRMED, naive.into())]).await?;
        let historical = store.load().await?;
        assert_eq!(historical.confirmed_at, Some(timestamp));
        let rotated = TwoFactorState { recovery_codes: Some("rotated-codes".into()), ..historical.clone() };
        assert!(store.compare_and_set(&historical, &rotated).await?);
        assert_eq!(store.load().await?, rotated);

        // load之后出现实际状态变化，不能以“兼容时间文本”为由覆盖新状态。
        let before = store.load().await?;
        db.table("accounts")?.where_eq("account_key", 7_i64)?.update(vec![(RECOVERY, "changed-after-snapshot".into())]).await?;
        assert!(!store.compare_and_set(&before, &TwoFactorState::default()).await?);
        let changed = store.load().await?;
        assert_eq!(changed.recovery_codes.as_deref(), Some("changed-after-snapshot"));
        assert_eq!(changed.confirmed_at, Some(timestamp));
        assert!(store.compare_and_set(&changed, &TwoFactorState::default()).await?);
        assert_eq!(store.load().await?, TwoFactorState::default());
        Ok(())
    })).await
}
