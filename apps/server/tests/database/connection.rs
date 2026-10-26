//! Verifies the configured connection pool against the selected database service.
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::core::RateLimiter;
use openrust::database::{DatabasePlugin, Db};
use openrust::{AppBuilder, Error, config::FrameworkConfig, lifecycle};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

static DATABASE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn plugin_opens_pool_and_executes_a_query() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = DATABASE_TEST_LOCK.lock().await;
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database connection: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let configured_replica = std::env::var("OPENRUST_TEST_READ_REPLICA_URL").ok();
    let has_distinct_replica_url = configured_replica
        .as_deref()
        .is_some_and(|replica| replica != url);
    let replica_url = configured_replica.unwrap_or_else(|| url.clone());
    let source = format!("[database]\nurl = {url:?}\nread_replicas = [{replica_url:?}]\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin);

    let app = lifecycle::finalize(builder).await?;
    assert!(app.state().try_get::<RateLimiter>().is_ok());
    let database = app.state().try_get::<Db>()?.connection()?;
    database.execute_unprepared("SELECT 1").await?;
    let db = app.state().try_get::<Db>()?;
    let read_connection = db.read_connection()?;
    assert_eq!(
        read_connection.get_database_backend(),
        database.get_database_backend()
    );
    let primary_identity = database_identity(&database).await?;
    let read_identity = database_identity(&read_connection).await?;
    if has_distinct_replica_url {
        assert_ne!(
            read_identity, primary_identity,
            "the configured read replica must identify a separate database server"
        );
    } else {
        assert_eq!(read_identity, primary_identity);
    }
    let custom_db = Db::default();
    custom_db.initialize_with_replicas(database.clone(), vec![read_connection.clone()])?;
    assert_eq!(
        database_identity(&custom_db.read_connection()?).await?,
        read_identity,
        "custom database plugins must preserve explicit replica routing"
    );
    let scoped_read_backend = db
        .with_primary(async {
            let connection = db.read_connection()?;
            database_identity(&connection).await
        })
        .await??;
    assert_eq!(scoped_read_backend, primary_identity);

    #[cfg(feature = "db-mysql")]
    if url.starts_with("mysql://") {
        let row = database
            .query_one_raw(Statement::from_string(
                database.get_database_backend(),
                "SELECT @@session.time_zone AS openrust_timezone".to_owned(),
            ))
            .await?
            .ok_or("MySQL timezone query returned no row")?;
        let timezone = row.try_get::<String>("", "openrust_timezone")?;
        assert_eq!(timezone, "+00:00");

        let row = database
            .query_one_raw(Statement::from_string(
                database.get_database_backend(),
                "SHOW SESSION STATUS LIKE 'Ssl_cipher'".to_owned(),
            ))
            .await?
            .ok_or("MySQL TLS query returned no row")?;
        let cipher = row.try_get::<String>("", "Value")?;
        assert!(!cipher.is_empty(), "MySQL TLS must be active");
    }
    Ok(())
}

async fn database_identity(connection: &sea_orm::DatabaseConnection) -> openrust::Result<String> {
    let (backend, query) = match connection.get_database_backend() {
        DbBackend::Postgres => (
            DbBackend::Postgres,
            "SELECT current_database() || ':' || COALESCE(inet_server_addr()::text, 'local') || ':' || COALESCE(inet_server_port()::text, '0') AS identity",
        ),
        DbBackend::MySql => (
            DbBackend::MySql,
            "SELECT CONCAT(DATABASE(), ':', @@server_uuid) AS identity",
        ),
        other => {
            return Err(Error::internal(std::io::Error::other(format!(
                "unexpected database backend: {other:?}"
            ))));
        }
    };
    let row = connection
        .query_one_raw(Statement::from_string(backend, query.to_owned()))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| {
            Error::internal(std::io::Error::other(
                "database identity query returned no row",
            ))
        })?;
    row.try_get::<String>("", "identity")
        .map_err(Error::internal)
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // Keep the pause, stale-read, primary-read, and catch-up assertions together.
async fn primary_scope_reads_fresh_data_while_replica_apply_is_paused()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = DATABASE_TEST_LOCK.lock().await;
    if std::env::var("OPENRUST_TEST_VERIFY_REPLICA_LAG").as_deref() != Ok("1") {
        eprintln!(
            "skipping replication lag check: set OPENRUST_TEST_VERIFY_REPLICA_LAG=1 with a controllable replica"
        );
        return Ok(());
    }
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
    let replica_url = std::env::var("OPENRUST_TEST_READ_REPLICA_URL")?;
    let source = format!("[database]\nurl = {url:?}\nread_replicas = [{replica_url:?}]\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin);
    let app = lifecycle::finalize(builder).await?;
    let db = app.state().try_get::<Db>()?;
    let peer_figment = Figment::new().merge(Toml::string(&source));
    let mut peer_builder = AppBuilder::new(FrameworkConfig::default(), peer_figment);
    peer_builder.plugin(DatabasePlugin);
    let peer_app = lifecycle::finalize(peer_builder).await?;
    let peer_db = peer_app.state().try_get::<Db>()?;
    let primary = db.primary_connection()?;
    let replica = db.read_connection()?;
    if database_identity(&primary).await? == database_identity(&replica).await? {
        return Err("primary and replica URLs must resolve to distinct database servers".into());
    }
    let table = format!(
        "openrust_replica_lag_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    primary
        .execute_unprepared(&format!(
            "CREATE TABLE {table} (id VARCHAR(128) PRIMARY KEY)"
        ))
        .await?;
    wait_for_row_count(&replica, &table, 0).await?;

    match primary.get_database_backend() {
        DbBackend::Postgres => {
            replica
                .query_one_raw(Statement::from_string(
                    DbBackend::Postgres,
                    "SELECT pg_wal_replay_pause()".to_owned(),
                ))
                .await?;
        }
        DbBackend::MySql => {
            replica
                .execute_unprepared("STOP REPLICA SQL_THREAD")
                .await?;
        }
        other => return Err(format!("unexpected database backend: {other:?}").into()),
    }

    let marker = format!("marker_{}", std::process::id());
    primary
        .execute_unprepared(&format!("INSERT INTO {table} (id) VALUES ('{marker}')"))
        .await?;
    let replica_count = row_count(&replica, &table).await?;
    let (primary_count, peer_replica_count) = db
        .with_primary(async {
            let scoped_primary = row_count(&db.read_connection()?, &table).await?;
            let peer_replica = row_count(&peer_db.read_connection()?, &table).await?;
            Ok::<_, Box<dyn std::error::Error>>((scoped_primary, peer_replica))
        })
        .await??;
    let spawned_db = db.clone();
    let spawned_table = table.clone();
    let spawned_task_count = tokio::spawn(async move {
        let connection = spawned_db
            .read_connection()
            .map_err(|error| error.to_string())?;
        row_count(&connection, &spawned_table)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(std::io::Error::other)??;
    let peer_primary_count = peer_db
        .with_primary(async { row_count(&peer_db.read_connection()?, &table).await })
        .await??;

    match primary.get_database_backend() {
        DbBackend::Postgres => {
            replica
                .query_one_raw(Statement::from_string(
                    DbBackend::Postgres,
                    "SELECT pg_wal_replay_resume()".to_owned(),
                ))
                .await?;
        }
        DbBackend::MySql => {
            replica
                .execute_unprepared("START REPLICA SQL_THREAD")
                .await?;
        }
        other => return Err(format!("unexpected database backend: {other:?}").into()),
    }
    let catch_up = wait_for_row_count(&replica, &table, 1).await;
    primary
        .execute_unprepared(&format!("DROP TABLE {table}"))
        .await?;

    assert_eq!(
        replica_count, 0,
        "paused replica should return its stale value"
    );
    assert_eq!(
        primary_count, 1,
        "primary scope must see the committed write"
    );
    assert_eq!(
        peer_replica_count, 0,
        "one node's primary scope must not change another node's read route"
    );
    assert_eq!(
        spawned_task_count, 0,
        "independent Tokio tasks must not inherit the current task's primary scope"
    );
    assert_eq!(
        peer_primary_count, 1,
        "each API node can explicitly read its own primary"
    );
    catch_up?;
    Ok(())
}

async fn row_count(
    connection: &sea_orm::DatabaseConnection,
    table: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    let row = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {table}"),
        ))
        .await?
        .ok_or("replica count query returned no row")?;
    Ok(row.try_get::<i64>("", "row_count")?)
}

async fn wait_for_row_count(
    connection: &sea_orm::DatabaseConnection,
    table: &str,
    expected: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut last_error = None;
    for _ in 0..100 {
        match row_count(connection, table).await {
            Ok(count) if count == expected => return Ok(()),
            Ok(_) => {}
            Err(error) => last_error = Some(error.to_string()),
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err(format!(
        "database replica did not reach row count {expected}; last query error: {}",
        last_error.unwrap_or_else(|| "none".to_owned())
    )
    .into())
}

#[tokio::test]
async fn transaction_commits_success_and_rolls_back_callback_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = DATABASE_TEST_LOCK.lock().await;
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database transaction: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin);

    let app = lifecycle::finalize(builder).await?;
    let database = app.state().try_get::<Db>()?;
    let connection = database.connection()?;
    let table = format!(
        "openrust_db_transaction_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    connection
        .execute_unprepared(&format!("CREATE TABLE {table} (id BIGINT PRIMARY KEY)"))
        .await?;

    let table_for_commit = table.clone();
    let committed_id = database
        .transaction(|transaction| {
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!("INSERT INTO {table_for_commit} (id) VALUES (1)"))
                    .await
                    .map_err(Error::internal)?;
                Ok(1_i64)
            })
        })
        .await?;
    assert_eq!(committed_id, 1);

    let table_for_rollback = table.clone();
    let rolled_back = database
        .transaction(|transaction| {
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!(
                        "INSERT INTO {table_for_rollback} (id) VALUES (2)"
                    ))
                    .await
                    .map_err(Error::internal)?;
                Err::<(), _>(Error::internal(std::io::Error::other("force rollback")))
            })
        })
        .await;
    assert!(rolled_back.is_err());

    let count = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {table}"),
        ))
        .await?
        .ok_or("transaction row count query returned no row")?
        .try_get::<i64>("", "row_count")?;
    connection
        .execute_unprepared(&format!("DROP TABLE {table}"))
        .await?;
    assert_eq!(count, 1, "callback errors must roll back their writes");
    Ok(())
}

#[tokio::test]
async fn supported_backends_skip_rows_locked_by_another_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = DATABASE_TEST_LOCK.lock().await;
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping SKIP LOCKED check: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    builder.plugin(DatabasePlugin);

    let app = lifecycle::finalize(builder).await?;
    let database = app.state().try_get::<Db>()?;
    let connection = database.connection()?;
    let table = format!(
        "openrust_skip_locked_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    connection
        .execute_unprepared(&format!("CREATE TABLE {table} (id BIGINT PRIMARY KEY)"))
        .await?;
    connection
        .execute_unprepared(&format!("INSERT INTO {table} (id) VALUES (1), (2)"))
        .await?;

    let first = connection.begin().await?;
    first
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT id FROM {table} WHERE id = 1 FOR UPDATE"),
        ))
        .await?
        .ok_or("locking query returned no row")?;

    let table_for_second = table.clone();
    let second_id = database
        .transaction(|transaction| {
            Box::pin(async move {
                let row = transaction
                    .query_one_raw(Statement::from_string(
                        transaction.get_database_backend(),
                        format!(
                            "SELECT id FROM {table_for_second} ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED"
                        ),
                    ))
                    .await
                    .map_err(Error::internal)?
                    .ok_or_else(|| Error::internal(std::io::Error::other("no row was unlocked")))?;
                row.try_get::<i64>("", "id").map_err(Error::internal)
            })
        })
        .await?;
    first.rollback().await?;

    connection
        .execute_unprepared(&format!("DROP TABLE {table}"))
        .await?;
    assert_eq!(
        second_id, 2,
        "SKIP LOCKED must return the next available row"
    );
    Ok(())
}
