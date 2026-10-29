//! 用真实 PostgreSQL/MySQL 验证 Session 持久写入、撤销与授权截止。
//!
//! 此测试显式忽略，必须由双库验收传入 `OPENRUST_TEST_DATABASE_URL` 后执行；
//! 未配置服务或所需 backend feature 时失败，不将缺失服务作为测试成功。
#![cfg(all(
    feature = "session",
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use std::error::Error as StdError;

use chrono::{DateTime, Utc};
use openrust::auth::AuthMigration;
use openrust::database::sea_orm::{ConnectionTrait, DbBackend, Statement};
use openrust::database::sea_orm_migration::{MigratorTrait, prelude::*};
use openrust::database::{DatabaseBackend, DatabaseTableNames, Db};
use openrust::session::{
    DEFAULT_MAX_BYTES, DatabaseSessionStore, SessionId, SessionRecord, SessionStore,
    SessionWriteMode, SessionWriteOutcome,
};
use openrust::testing::TestDatabase;
use serde_json::json;
use time::{Duration, OffsetDateTime};

type TestResult<T = ()> = Result<T, Box<dyn StdError>>;

struct SessionMigrator;

impl MigratorTrait for SessionMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        // 复用正式账户迁移中的 sessions 表；不维护测试专用 schema。
        vec![Box::new(AuthMigration)]
    }
}

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn record(letter: char, marker: &str) -> TestResult<SessionRecord> {
    Ok(SessionRecord {
        id: SessionId::parse(&letter.to_string().repeat(40))?,
        data: json!({"marker":marker,"null":null,"space":"  keep  "}),
        expiry_date: (OffsetDateTime::now_utc() + Duration::hours(1))
            .replace_nanosecond(123_456_000)?,
    })
}

async fn loaded(store: &DatabaseSessionStore, record: &SessionRecord) -> TestResult<SessionRecord> {
    store
        .load(&record.id, DEFAULT_MAX_BYTES, record.expiry_date)
        .await?
        .ok_or_else(|| "Session record unexpectedly missing".into())
}

async fn write_is(
    store: &DatabaseSessionStore,
    record: &SessionRecord,
    mode: SessionWriteMode,
    expected: SessionWriteOutcome,
) -> TestResult {
    check(
        store.write(record, mode, DEFAULT_MAX_BYTES).await? == expected,
        "Session write outcome differs from the expected atomic contract",
    )
}

async fn insert_conflict_unchanged_update_and_microseconds(
    store: &DatabaseSessionStore,
) -> TestResult {
    let record = record('a', "original")?;
    write_is(
        store,
        &record,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Written,
    )
    .await?;
    let restored = loaded(store, &record).await?;
    check(
        restored.id == record.id,
        "Session identity did not round-trip",
    )?;
    check(
        restored.data == record.data,
        "Session attributes did not round-trip",
    )?;
    check(
        restored.expiry_date == record.expiry_date,
        "Session persistent expiry lost microsecond precision",
    )?;
    let mut collision = record.clone();
    collision.data = json!({"marker":"must not overwrite"});
    collision.expiry_date += Duration::hours(1);
    write_is(
        store,
        &collision,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Conflict,
    )
    .await?;
    let unchanged = loaded(store, &record).await?;
    check(
        unchanged.data == record.data && unchanged.expiry_date == record.expiry_date,
        "Insert collision overwrote an existing Session",
    )?;
    // MySQL 必须按匹配行报告成功；完全不变的两次 UPDATE 也不是 Missing。
    for _ in 0..2 {
        write_is(
            store,
            &record,
            SessionWriteMode::Update,
            SessionWriteOutcome::Written,
        )
        .await?;
    }
    Ok(())
}

async fn case_sensitive_ids_and_late_update(store: &DatabaseSessionStore) -> TestResult {
    let lower = record('b', "lower")?;
    let upper = SessionRecord {
        id: SessionId::parse(&"B".repeat(40))?,
        data: json!({"marker":"upper"}),
        expiry_date: lower.expiry_date,
    };
    for record in [&lower, &upper] {
        write_is(
            store,
            record,
            SessionWriteMode::Insert,
            SessionWriteOutcome::Written,
        )
        .await?;
        check(
            loaded(store, record).await?.data == record.data,
            "Case-sensitive Session IDs aliased each other",
        )?;
    }
    check(
        loaded(store, &lower).await?.data == lower.data,
        "Second case overwrote the first",
    )?;
    store.delete(&lower.id).await?;
    store.delete(&lower.id).await?;
    write_is(
        store,
        &lower,
        SessionWriteMode::Update,
        SessionWriteOutcome::Missing,
    )
    .await?;
    check(
        store
            .load(&lower.id, DEFAULT_MAX_BYTES, lower.expiry_date)
            .await?
            .is_none(),
        "Late Update recreated a deleted Session",
    )?;
    check(
        loaded(store, &upper).await?.data == upper.data,
        "Deleting one case revoked another",
    )
}

async fn set_column_expiry(db: &Db, expiry: OffsetDateTime) -> TestResult {
    let connection = db.connection()?;
    let backend = connection.get_database_backend();
    let marker = match backend {
        DbBackend::Postgres => "$1",
        DbBackend::MySql => "?",
        _ => return Err("Session fixture requires PostgreSQL or MySQL".into()),
    };
    let expiry = DateTime::<Utc>::from_timestamp(expiry.unix_timestamp(), expiry.nanosecond())
        .ok_or("Fixture expiry cannot be represented for SQL")?;
    // 只操作本次 TestDatabase 的正式表；这里不复刻 driver 的编码或身份摘要。
    connection
        .execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "UPDATE {} SET expires_at={marker}",
                db.quoted_table_name("sessions")?
            ),
            [expiry.into()],
        ))
        .await?;
    Ok(())
}

async fn column_cutoff_and_expired_update(db: &Db, store: &DatabaseSessionStore) -> TestResult {
    let record = record('c', "column cutoff")?;
    write_is(
        store,
        &record,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Written,
    )
    .await?;
    let earlier = record.expiry_date - Duration::minutes(30);
    set_column_expiry(db, earlier).await?;
    let restored = loaded(store, &record).await?;
    check(
        restored.expiry_date == earlier && restored.expiry_date < record.expiry_date,
        "Envelope extended authorization beyond the SQL expiry column",
    )?;
    check(
        restored.data == record.data,
        "Column cutoff changed Session attributes",
    )?;
    set_column_expiry(db, OffsetDateTime::now_utc() - Duration::hours(1)).await?;
    // 不先 load（它可清理过期行），直接证明 UPDATE 本身拒绝过期目标。
    write_is(
        store,
        &record,
        SessionWriteMode::Update,
        SessionWriteOutcome::Missing,
    )
    .await?;
    check(
        store
            .load(&record.id, DEFAULT_MAX_BYTES, record.expiry_date)
            .await?
            .is_none(),
        "Expired Session remained authorized or late Update recreated it",
    )
}

// 同秒内已经过期的记录也必须拒绝旧句柄 UPDATE，不能被 SQL 秒精度复活。
async fn subsecond_expired_update(db: &Db, store: &DatabaseSessionStore) -> TestResult {
    let connection = db.connection()?;
    let backend = connection.get_database_backend();
    let (clock, expired) = match backend {
        DbBackend::Postgres => (
            "SELECT CAST(EXTRACT(EPOCH FROM clock_timestamp()) * 1000000 AS BIGINT) AS micros",
            "clock_timestamp() - INTERVAL '1 microsecond'",
        ),
        DbBackend::MySql => (
            "SELECT CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(6)) * 1000000 AS SIGNED) AS micros",
            "CURRENT_TIMESTAMP(6) - INTERVAL 1 MICROSECOND",
        ),
        _ => return Err("Session fixture requires PostgreSQL or MySQL".into()),
    };
    let record = record('e', "subsecond expiry")?;
    write_is(
        store,
        &record,
        SessionWriteMode::Insert,
        SessionWriteOutcome::Written,
    )
    .await?;
    for _ in 0..100 {
        let row = connection
            .query_one_raw(Statement::from_string(backend, clock))
            .await?
            .ok_or("Database clock row missing")?;
        let before: i64 = row.try_get("", "micros")?;
        if !(200_000..500_000).contains(&before.rem_euclid(1_000_000)) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            continue;
        }
        connection
            .execute_raw(Statement::from_string(
                backend,
                format!(
                    "UPDATE {} SET expires_at={expired}",
                    db.quoted_table_name("sessions")?
                ),
            ))
            .await?;
        let outcome = store
            .write(&record, SessionWriteMode::Update, DEFAULT_MAX_BYTES)
            .await?;
        let row = connection
            .query_one_raw(Statement::from_string(backend, clock))
            .await?
            .ok_or("Database clock row missing")?;
        let after: i64 = row.try_get("", "micros")?;
        // 跨秒时重试，避免把秒精度 SQL 也会拒绝的样本误记为回归覆盖。
        if before / 1_000_000 == after / 1_000_000 {
            return check(
                outcome == SessionWriteOutcome::Missing,
                "Subsecond-expired Session was revived by an existing request",
            );
        }
    }
    Err("Could not obtain a same-second database expiry sample".into())
}

async fn text_order_nul_budget_and_authorization(
    db: &Db,
    store: &DatabaseSessionStore,
) -> TestResult {
    let mut record = record('d', "text")?;
    record.data = serde_json::from_str(
        r#"{"z":"a\u0000b","a":1,"m":null,"openrust":{"auth":{"user_id":42}}}"#,
    )?;
    let encoded = record.encode(DEFAULT_MAX_BYTES)?;
    check(
        store
            .write(&record, SessionWriteMode::Insert, encoded.len())
            .await?
            == SessionWriteOutcome::Written,
        "Exact encoded budget was rejected",
    )?;
    let restored = store
        .load(&record.id, encoded.len(), record.expiry_date)
        .await?
        .ok_or("Text Session missing")?;
    check(
        serde_json::to_vec(&restored.data)? == serde_json::to_vec(&record.data)?,
        "Session text changed key order or NUL",
    )?;
    let too_small = store
        .load(&record.id, encoded.len() - 1, record.expiry_date)
        .await;
    check(
        too_small.as_ref().is_err_and(|error| {
            error.code() == "session_limits_exceeded"
                && error.status() == http::StatusCode::PAYLOAD_TOO_LARGE
        }),
        "Encoded read budget did not reject payload",
    )?;
    let rejected = store
        .write(&record, SessionWriteMode::Update, encoded.len() - 1)
        .await;
    check(
        rejected
            .as_ref()
            .is_err_and(|error| error.code() == "session_limits_exceeded"),
        "Encoded write budget did not reject payload",
    )?;
    let session = openrust::session::Session::new(
        Some(record.id.clone()),
        std::sync::Arc::new(store.clone()),
        None,
    )?;
    session.start().await?;
    let earlier = record.expiry_date - Duration::minutes(15);
    set_column_expiry(db, earlier).await?;
    let until = openrust::session::SessionStoreHandle::new(store.clone())
        .authorized_until(&session, 42)
        .await?;
    let expected = std::time::UNIX_EPOCH
        + std::time::Duration::new(
            <u64 as TryFrom<i64>>::try_from(earlier.unix_timestamp())?,
            earlier.nanosecond(),
        );
    check(
        until <= expected,
        "authorized_until exceeded SQL column cutoff",
    )
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL/MySQL 与 OPENRUST_TEST_DATABASE_URL；分别启用对应 db feature 执行 --ignored"]
async fn database_session_atomic_writes_precision_and_revocation() -> TestResult {
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL")
        .map_err(|_| "OPENRUST_TEST_DATABASE_URL is required for this ignored integration test")?;
    let backend = DatabaseBackend::from_url(&url)
        .map_err(|_| "Session integration database backend is invalid")?;
    check(
        backend.is_enabled(),
        "Selected database backend feature is disabled",
    )?;
    let names = DatabaseTableNames::new("session_it_", std::collections::BTreeMap::new())?;
    let database = TestDatabase::create_with_table_names::<SessionMigrator>(&url, names.clone())
        .await
        .map_err(|_| "Could not create the isolated Session integration database")?;
    let outcome = async {
        let db = Db::with_table_names(names);
        db.initialize(database.connection()?)?;
        let store = DatabaseSessionStore::new(db.clone());
        insert_conflict_unchanged_update_and_microseconds(&store).await?;
        case_sensitive_ids_and_late_update(&store).await?;
        column_cutoff_and_expired_update(&db, &store).await?;
        subsecond_expired_update(&db, &store).await?;
        text_order_nul_budget_and_authorization(&db, &store).await
    }
    .await;
    // 不用 panic/assert 提前跳过清理；即使业务检查失败也等待独占数据库删除。
    let cleanup = database
        .close()
        .await
        .map_err(|_| "Could not clean up the isolated Session integration database");
    cleanup?;
    outcome
}
