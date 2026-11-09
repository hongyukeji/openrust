//! 针对所选数据库服务验证已配置的连接池。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::Figment;
use figment::providers::{Format, Toml};
use openrust::core::RateLimiter;
use openrust::database::{DatabaseServiceProvider, Db};
use openrust::events::EventsServiceProvider;
use openrust::{Error, config::FrameworkConfig, http::HttpServiceProvider};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::watch,
    task::{JoinHandle, JoinSet},
};

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
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(DatabaseServiceProvider);

    let app = builder
        .start(sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    assert!(app.state().resolve::<RateLimiter>().await.is_ok());
    let readiness = app.state().resolve::<openrust::core::Readiness>().await?;
    assert!(readiness.dependencies_ready().await);
    let database = app.state().resolve::<Db>().await?.connection()?;
    database.execute_unprepared("SELECT 1").await?;
    let db = app.state().resolve::<Db>().await?;
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

#[tokio::test]
async fn runtime_database_readiness_fails_and_recovers_through_a_proxy()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = DATABASE_TEST_LOCK.lock().await;
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping database readiness failure check: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let parsed = url::Url::parse(&url)?;
    let upstream_host = parsed
        .host_str()
        .ok_or("database test URL has no host")?
        .to_owned();
    let upstream = (
        upstream_host,
        parsed
            .port_or_known_default()
            .ok_or("database URL has no port")?,
    );
    let proxy = DatabaseProxy::start(upstream).await?;
    let mut proxied_url = parsed;
    proxied_url
        .set_host(Some("127.0.0.1"))
        .map_err(|_| "could not set test proxy host")?;
    proxied_url
        .set_port(Some(proxy.address.port()))
        .map_err(|()| "could not set test proxy port")?;
    let source = format!(
        "[database]\nurl = {:?}\nread_replicas = [{:?}]\n",
        proxied_url.as_str(),
        proxied_url.as_str()
    );
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(Figment::new().merge(Toml::string(&source)));
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(DatabaseServiceProvider);
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    let readiness = app.state().resolve::<openrust::core::Readiness>().await?;
    assert!(readiness.dependencies_ready().await);

    proxy.set_enabled(false);
    assert!(wait_for_readiness(&readiness, false).await);
    proxy.set_enabled(true);
    assert!(wait_for_readiness(&readiness, true).await);
    Ok(())
}

async fn wait_for_readiness(readiness: &openrust::core::Readiness, expected: bool) -> bool {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if readiness.dependencies_ready().await == expected {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .is_ok()
}

struct DatabaseProxy {
    address: SocketAddr,
    enabled: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl DatabaseProxy {
    async fn start(upstream: (String, u16)) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (enabled, mut state) = watch::channel(true);
        let task = tokio::spawn(async move {
            let mut sessions = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((mut client, _peer)) = accepted else { break };
                        if *state.borrow() {
                            let upstream = upstream.clone();
                            sessions.spawn(async move {
                                let Ok(mut server) = TcpStream::connect(upstream).await else { return };
                                let _ = copy_bidirectional(&mut client, &mut server).await;
                            });
                        }
                    }
                    changed = state.changed() => {
                        if changed.is_err() { break; }
                        if !*state.borrow() { sessions.abort_all(); }
                    }
                    Some(_) = sessions.join_next(), if !sessions.is_empty() => {}
                }
            }
        });
        Ok(Self {
            address,
            enabled,
            task,
        })
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.send_replace(enabled);
    }
}

impl Drop for DatabaseProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
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
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider);
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    let db = app.state().resolve::<Db>().await?;
    let peer_figment = Figment::new().merge(Toml::string(&source));
    let peer_builder_sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(peer_figment);
    let mut peer_builder = openrust::foundation::ProviderBuilder::new();
    peer_builder.provider(DatabaseServiceProvider);
    let peer_app = peer_builder
        .start(peer_builder_sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    let peer_db = peer_app.state().resolve::<Db>().await?;
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
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider);

    let app = builder
        .start(sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    let database = app.state().resolve::<Db>().await?;
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
    let table_for_business_error = table.clone();
    let preserved = database
        .transaction(|transaction| {
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!(
                        "INSERT INTO {table_for_business_error} (id) VALUES (3)"
                    ))
                    .await
                    .map_err(Error::internal)?;
                Err::<(), _>(Error::custom_with_status(
                    openrust::http::axum::http::StatusCode::CONFLICT,
                    "business_rejected",
                    "business rule rejected",
                ))
            })
        })
        .await;
    let preserved = preserved
        .err()
        .ok_or("business error unexpectedly accepted")?;
    assert_eq!(preserved.code(), "business_rejected");
    assert_eq!(
        preserved.status(),
        openrust::http::axum::http::StatusCode::CONFLICT
    );

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
#[allow(clippy::too_many_lines)] // 同一隔离事务覆盖回调、提交前与提交后错误顺序。
async fn transaction_events_rollback_before_commit_and_ignore_after_commit_errors()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    let _guard = DATABASE_TEST_LOCK.lock().await;
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping transaction events: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let source = format!("[database]\nurl = {url:?}\n");
    let figment = Figment::new().merge(Toml::string(&source));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::foundation::ProviderBuilder::new();
    let reject = Arc::new(AtomicBool::new(false));
    let after_calls = Arc::new(AtomicUsize::new(0));
    let mut events = EventsServiceProvider::new();
    let reject_listener = Arc::clone(&reject);
    events.listen::<openrust::database::TransactionBeforeCommit, _>(move |_, _| {
        let reject = Arc::clone(&reject_listener);
        Box::pin(async move {
            if reject.load(Ordering::SeqCst) {
                Err(Error::custom(
                    "commit_rejected",
                    "commit rejected by listener",
                ))
            } else {
                Ok(())
            }
        })
    });
    let after_listener = Arc::clone(&after_calls);
    events.listen::<openrust::database::TransactionCommitted, _>(move |_, _| {
        let after_calls = Arc::clone(&after_listener);
        Box::pin(async move {
            after_calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::custom("after_commit_failed", "notification failed"))
        })
    });
    builder
        .provider(openrust::events::EventServiceProvider)
        .provider(DatabaseServiceProvider)
        .provider(events);
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    let db = app.state().resolve::<Db>().await?;
    let connection = db.connection()?;
    let table = format!("openrust_tx_events_{}", std::process::id());
    connection
        .execute_unprepared(&format!("CREATE TABLE {table} (id BIGINT PRIMARY KEY)"))
        .await?;

    reject.store(true, Ordering::SeqCst);
    let rejected = db
        .transaction(|transaction| {
            let table = table.clone();
            Box::pin(async move {
                transaction
                    .execute_unprepared(&format!("INSERT INTO {table} (id) VALUES (1)"))
                    .await
                    .map_err(Error::internal)?;
                Ok(())
            })
        })
        .await;
    assert_eq!(
        rejected.err().map(|error| error.code().to_owned()),
        Some("commit_rejected".into())
    );
    assert_eq!(
        after_calls.load(Ordering::SeqCst),
        0,
        "a rejected transaction must not emit after-commit"
    );

    reject.store(false, Ordering::SeqCst);
    db.transaction(|transaction| {
        let table = table.clone();
        Box::pin(async move {
            transaction
                .execute_unprepared(&format!("INSERT INTO {table} (id) VALUES (2)"))
                .await
                .map_err(Error::internal)?;
            Ok(())
        })
    })
    .await?;
    let count = connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            format!("SELECT COUNT(*) AS row_count FROM {table}"),
        ))
        .await?
        .ok_or("transaction event count query returned no row")?
        .try_get::<i64>("", "row_count")?;
    connection
        .execute_unprepared(&format!("DROP TABLE {table}"))
        .await?;
    assert_eq!(
        count, 1,
        "the after-commit listener error must not disguise a committed write"
    );
    assert_eq!(after_calls.load(Ordering::SeqCst), 1);
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
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment);
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider);

    let app = builder
        .start(sources)
        .await
        .and_then(openrust::Application::from_provider_runtime)?;
    let database = app.state().resolve::<Db>().await?;
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
