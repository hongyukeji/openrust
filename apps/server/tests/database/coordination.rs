//! 在真实 PostgreSQL/MySQL 隔离数据库中验证共享锁与请求幂等。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use std::time::Duration;

use openrust::core::Error;
use openrust::database::{
    CoordinationMigration, DatabaseTableNames, Db, IdempotencyClaim, IdempotencyResponse,
    LockAcquire, RequestIdempotency, SharedLocks, sea_orm, sea_orm_migration,
};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, Statement};
use sea_orm_migration::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;
use testing::TestDatabase;

struct CoordinationMigrator;

impl MigratorTrait for CoordinationMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(CoordinationMigration)]
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // 将跨实例锁竞争与相关持久化语义保留在同一个集成场景中。
async fn shared_locks_and_request_idempotency_are_database_scoped()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database coordination: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let table_names = DatabaseTableNames::new("coord_", BTreeMap::new())?;
    let database = TestDatabase::create_with_table_names::<CoordinationMigrator>(
        &admin_url,
        table_names.clone(),
    )
    .await?;
    let connection = database.connection()?;
    let first_db = Db::with_table_names(table_names.clone());
    first_db.initialize(connection.clone())?;
    let mut options = ConnectOptions::new(database.url().to_owned());
    options.max_connections(2).min_connections(0);
    #[cfg(feature = "db-mysql")]
    if database.url().starts_with("mysql:") {
        options.map_sqlx_mysql_opts(|mysql| {
            mysql.ssl_mode(sea_orm::sqlx::mysql::MySqlSslMode::Required)
        });
        options.after_connect(|connection| {
            Box::pin(async move {
                connection
                    .execute_unprepared("SET time_zone = '+00:00'")
                    .await?;
                Ok(())
            })
        });
    }
    let second_connection = Database::connect(options).await?;
    let second_db = Db::with_table_names(table_names);
    second_db.initialize(second_connection.clone())?;

    let locks_a = SharedLocks::new(first_db.clone());
    let locks_b = SharedLocks::new(second_db.clone());
    let lease_a = match locks_a
        .acquire("daily-report", Duration::from_secs(30))
        .await?
    {
        LockAcquire::Acquired(lease) => lease,
        LockAcquire::Busy { .. } => return Err("first lock acquisition unexpectedly busy".into()),
    };
    assert!(matches!(
        locks_b
            .acquire("daily-report", Duration::from_secs(30))
            .await?,
        LockAcquire::Busy { .. }
    ));
    let lock_table = first_db.table_name("shared_locks")?;
    first_db
        .connection()?
        .execute_unprepared(&format!("UPDATE {lock_table} SET expires_at_ms = 0"))
        .await?;
    let lease_b = match locks_b
        .acquire("daily-report", Duration::from_secs(30))
        .await?
    {
        LockAcquire::Acquired(lease) => lease,
        LockAcquire::Busy { .. } => return Err("expired lock was not reclaimed".into()),
    };
    assert!(lease_b.fencing_token() > lease_a.fencing_token());
    assert!(
        !locks_a.release(&lease_a).await?,
        "stale owner must not release a reclaimed lock"
    );
    let fenced = first_db
        .transaction(|transaction| {
            let locks_a = locks_a.clone();
            let lease_a = lease_a.clone();
            Box::pin(async move {
                locks_a
                    .assert_current_in_transaction(transaction, &lease_a)
                    .await
            })
        })
        .await;
    assert!(
        fenced.is_err(),
        "a stale fencing token must fail in the business transaction"
    );
    assert!(locks_b.release(&lease_b).await?);

    let idem_a = RequestIdempotency::new(first_db.clone());
    let idem_b = RequestIdempotency::new(second_db.clone());
    let (claim_a, claim_b) = tokio::join!(
        idem_a.claim(
            "user:42:create-order",
            "request-1",
            b"normalized-order-v1",
            Duration::from_secs(30)
        ),
        idem_b.claim(
            "user:42:create-order",
            "request-1",
            b"normalized-order-v1",
            Duration::from_secs(30)
        ),
    );
    let ((IdempotencyClaim::Acquired(lease), other) | (other, IdempotencyClaim::Acquired(lease))) =
        (claim_a?, claim_b?)
    else {
        return Err("concurrent request claims did not produce exactly one owner".into());
    };
    assert!(matches!(other, IdempotencyClaim::InProgress { .. }));
    assert!(matches!(
        idem_b
            .claim(
                "user:42:create-order",
                "request-1",
                b"different-order",
                Duration::from_secs(30)
            )
            .await?,
        IdempotencyClaim::Conflict
    ));
    assert!(
        matches!(
            idem_b
                .claim(
                    "user:43:create-order",
                    "request-1",
                    b"normalized-order-v1",
                    Duration::from_secs(30)
                )
                .await?,
            IdempotencyClaim::Acquired(_)
        ),
        "caller scopes must be isolated"
    );

    let probe_table = first_db.table_name("coordination_probe")?;
    let backend = first_db.backend()?;
    if !matches!(
        backend,
        sea_orm::DbBackend::Postgres | sea_orm::DbBackend::MySql
    ) {
        return Err("unexpected database backend".into());
    }
    let create_probe = format!("CREATE TABLE {probe_table} (id VARCHAR(64) PRIMARY KEY)");
    first_db
        .connection()?
        .execute_unprepared(&create_probe)
        .await?;
    let response = IdempotencyResponse::new(201, json!({"order_id": "order-1"}))?;
    let IdempotencyClaim::Acquired(rollback_lease) = idem_a
        .claim("rollback", "rollback-1", b"body", Duration::from_secs(30))
        .await?
    else {
        return Err("rollback request did not acquire its lease".into());
    };
    let rollback_table = probe_table.clone();
    let rollback_idem = idem_a.clone();
    let rollback_result: openrust::Result<()> = first_db
        .transaction(|transaction| {
            let rollback_table = rollback_table.clone();
            let rollback_idem = rollback_idem.clone();
            let rollback_lease = rollback_lease.clone();
            let response = response.clone();
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!(
                        "INSERT INTO {rollback_table} (id) VALUES ('rolled-back')"
                    ))
                    .await
                    .map_err(Error::internal)?;
                rollback_idem
                    .complete_in_transaction(
                        transaction,
                        &rollback_lease,
                        &response,
                        Duration::from_secs(60),
                    )
                    .await?;
                Err(Error::custom("test_rollback", "force transaction rollback"))
            })
        })
        .await;
    assert!(rollback_result.is_err());
    let count = first_db
        .connection()?
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT COUNT(*) AS row_count FROM {probe_table}"),
        ))
        .await?
        .ok_or("probe count missing")?
        .try_get::<i64>("", "row_count")?;
    assert_eq!(
        count, 0,
        "business write must roll back with the response record"
    );
    assert!(idem_a.release(&rollback_lease).await?);

    let write_table = probe_table.clone();
    let write_idem = idem_a.clone();
    first_db
        .transaction(|transaction| {
            let table = write_table.clone();
            let idem = write_idem.clone();
            let lease = lease.clone();
            let response = response.clone();
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!("INSERT INTO {table} (id) VALUES ('order-1')"))
                    .await
                    .map_err(Error::internal)?;
                idem.complete_in_transaction(
                    transaction,
                    &lease,
                    &response,
                    Duration::from_secs(60),
                )
                .await
            })
        })
        .await?;
    match idem_b
        .claim(
            "user:42:create-order",
            "request-1",
            b"normalized-order-v1",
            Duration::from_secs(30),
        )
        .await?
    {
        IdempotencyClaim::Replay(replay) => {
            assert_eq!(replay.status(), 201);
            assert_eq!(replay.body(), &json!({"order_id": "order-1"}));
        }
        _ => return Err("completed request was not replayable".into()),
    }
    let IdempotencyClaim::Acquired(expiring) = idem_a
        .claim("expiry", "request-expiry", b"body", Duration::from_secs(30))
        .await?
    else {
        return Err("expiring request did not acquire its lease".into());
    };
    let idempotency_table = first_db.table_name("request_idempotency")?;
    first_db
        .connection()?
        .execute_unprepared(&format!("UPDATE {idempotency_table} SET expires_at_ms = 0"))
        .await?;
    let IdempotencyClaim::Acquired(replacement) = idem_b
        .claim("expiry", "request-expiry", b"body", Duration::from_secs(30))
        .await?
    else {
        return Err("expired idempotency record was not reclaimed".into());
    };
    assert!(
        !idem_a.release(&expiring).await?,
        "expired request owner must not remove the replacement lease"
    );
    assert!(idem_b.release(&replacement).await?);
    first_db
        .connection()?
        .execute_unprepared(&format!("UPDATE {idempotency_table} SET expires_at_ms = 0"))
        .await?;
    assert_eq!(
        idem_a.prune_expired(1).await?,
        1,
        "cleanup must honor its per-call bound"
    );
    assert!(idem_a.prune_expired(100).await? > 0);
    second_connection.close().await?;
    database.close().await?;
    Ok(())
}
