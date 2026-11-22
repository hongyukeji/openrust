use futures_util::TryStreamExt as _;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::{ConfigError, Environment, Repository},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{
        container::Container as _,
        events::{Dispatcher as _, Event as EventPayload, Listener},
        foundation::Application as _,
        http::Kernel as _,
    },
    database::{
        Bindings, Connection, ConnectionInterface, DatabaseError, DatabaseManager, Transaction,
        Value,
        events::{
            ConnectionEstablished, QueryBindings, QueryExecuted, QueryRole, TransactionBeginning,
            TransactionCommitted, TransactionCommitting, TransactionRolledBack,
        },
    },
    events::{
        Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerResponse,
    },
    foundation::{
        Application,
        configuration::{ApplicationBuilder, Routing},
        http::Kernel,
    },
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{Router, RoutingError},
    support::facades::DB,
};
use serde_json::json;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "openrust-sql-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir_all(root.join("database")).unwrap();
        fs::File::create(root.join("database/database.sqlite")).unwrap();
        Self(root)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manager(root: &Root, config: serde_json::Value) -> DatabaseManager {
    DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            config,
        )]))),
        &root.0,
    )
}

#[tokio::test]
async fn sqlite_uses_default_file_and_rejects_bad_connections_and_driver_errors() {
    let root = Root::new();
    let db = DatabaseManager::new(Arc::new(Repository::new(BTreeMap::new())), &root.0);
    assert!(
        matches!(db.connection(Some("missing")), Err(DatabaseError::UnknownConnection(name)) if name == "missing")
    );
    let conn = db.connection(None).unwrap();
    assert_eq!(conn.getName(), "sqlite");
    assert_eq!(
        conn.getDatabaseName(),
        root.0.join("database/database.sqlite").to_str().unwrap()
    );
    assert!(conn.unprepared("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, payload BLOB, note TEXT); CREATE TABLE links (item_id INTEGER REFERENCES items(id))").await.unwrap());
    assert!(matches!(
        conn.scalar("SELECT ?", &[]).await,
        Err(DatabaseError::InvalidBindings {
            expected: 1,
            actual: 0
        })
    ));
    assert!(matches!(
        conn.scalar("SELECT 1", &[1.into()]).await,
        Err(DatabaseError::InvalidBindings {
            expected: 0,
            actual: 1
        })
    ));
    let text = "O'Reilly'); DROP TABLE items; --";
    assert!(
        conn.insert(
            "INSERT INTO items VALUES (?, ?, ?, ?)",
            &[
                1.into(),
                text.into(),
                Value::Bytes(vec![1, 2, 3]),
                Value::Null
            ]
        )
        .await
        .unwrap()
    );
    let row = conn
        .selectOne(
            "SELECT name, payload, note FROM items WHERE id = ?",
            &[1.into()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("name"), Some(&Value::from(text)));
    assert_eq!(row.get_at(1), Some(&Value::Bytes(vec![1, 2, 3])));
    assert_eq!(row.get("note"), Some(&Value::Null));
    assert!(matches!(
        conn.scalar("SELECT id, name FROM items", &[]).await,
        Err(DatabaseError::InvalidScalar)
    ));
    assert_eq!(
        conn.scalar("SELECT id FROM items WHERE id = ?", &[99.into()])
            .await
            .unwrap(),
        Value::Null
    );
    let failed = conn
        .insert("INSERT INTO links VALUES (?)", &[123.into()])
        .await
        .unwrap_err();
    assert!(matches!(failed, DatabaseError::Driver { .. }));
    assert!(!format!("{failed:?}").contains("INSERT INTO"));
    assert_eq!(
        conn.update(
            "UPDATE items SET name = ? WHERE id = ?",
            &["changed".into(), 1.into()]
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        conn.delete("DELETE FROM items WHERE id = ?", &[1.into()])
            .await
            .unwrap(),
        1
    );

    let url_db = manager(
        &root,
        json!({
            "default":"memory",
            "connections":{
                "memory":{"driver":"mysql","url":"sqlite3::memory:"},
                "missing_file":{"driver":"sqlite","database":"missing.sqlite"},
                "unsupported":{"driver":"not-a-driver"}
            }
        }),
    );
    let memory = url_db.connection(None).unwrap();
    memory
        .statement("CREATE TABLE kept (id INTEGER)", &[])
        .await
        .unwrap();
    assert_eq!(
        url_db
            .connection(None)
            .unwrap()
            .scalar("SELECT COUNT(*) FROM kept", &[])
            .await
            .unwrap(),
        0.into()
    );
    let missing = url_db.connection(Some("missing_file")).unwrap();
    assert!(matches!(
        missing.scalar("SELECT 1", &[]).await,
        Err(DatabaseError::Driver { .. })
    ));
    assert!(!root.0.join("missing.sqlite").exists());
    assert!(matches!(
        url_db.connection(Some("unsupported")),
        Err(DatabaseError::UnsupportedDriver)
    ));
    let settings = json!({
        "default":"memory",
        "connections":{"memory":"sqlite://named?mode=memory"}
    });
    let first = manager(&root, settings.clone()).connection(None).unwrap();
    let second = manager(&root, settings).connection(None).unwrap();
    first
        .statement("CREATE TABLE app_local (id INTEGER)", &[])
        .await
        .unwrap();
    assert!(matches!(
        second.scalar("SELECT COUNT(*) FROM app_local", &[]).await,
        Err(DatabaseError::Driver { .. })
    ));
}

#[tokio::test]
async fn real_mysql_executes_prepared_queries_and_retires_cancelled_connections() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL acceptance requires its isolated URL"
        );
        return;
    };
    let root = Root::new();
    let db = manager(
        &root,
        json!({
            "default":"server",
            "connections":{
                "server":{"driver":"sqlite","url":url,"host":"invalid.invalid","port":1,"username":"unused","password":"private-config","max_connections":1,"acquire_timeout":5},
            "locker":{"url":url,"max_connections":1,"acquire_timeout":5}
            }
        }),
    );
    let conn = db.connection(None).unwrap();
    let Value::Text(version) = conn.scalar("SELECT VERSION()", &[]).await.unwrap() else {
        panic!("expected version text")
    };
    assert!(version.starts_with("8.4."));
    let cipher = conn
        .selectOne("SHOW SESSION STATUS LIKE 'Ssl_cipher'", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(cipher.get("Value"), Some(Value::Text(name)) if !name.is_empty()));
    conn.statement("DROP TABLE IF EXISTS rust_probe", &[])
        .await
        .unwrap();
    conn.statement("CREATE TABLE rust_probe (id BIGINT UNSIGNED PRIMARY KEY, title VARCHAR(64), amount DECIMAL(20,4), payload JSON, note TEXT NULL)", &[]).await.unwrap();
    let text = "O'Reilly'); --";
    conn.insert(
        "INSERT INTO rust_probe VALUES (?, ?, ?, ?, ?)",
        &[
            Value::Unsigned(u64::MAX),
            text.into(),
            "123456789.4321".into(),
            Value::Json(json!({"key":true})),
            Value::Null,
        ],
    )
    .await
    .unwrap();
    let row = conn
        .selectOne(
            "SELECT * FROM rust_probe WHERE id = ?",
            &[Value::Unsigned(u64::MAX)],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("id"), Some(&Value::Unsigned(u64::MAX)));
    assert_eq!(row.get("title"), Some(&text.into()));
    assert_eq!(row.get("amount"), Some(&"123456789.4321".into()));
    assert_eq!(row.get("payload"), Some(&Value::Json(json!({"key":true}))));
    assert_eq!(row.get("note"), Some(&Value::Null));
    let before = conn.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            conn.scalar("SELECT SLEEP(2)", &[])
        )
        .await
        .is_err()
    );
    let after = conn.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap();
    assert_ne!(before, after);
    let locker = db.connection(Some("locker")).unwrap();
    locker
        .unprepared("LOCK TABLES rust_probe WRITE")
        .await
        .unwrap();
    let interrupted = tokio::time::timeout(
        Duration::from_millis(100),
        conn.scalar("SELECT COUNT(*) FROM rust_probe /* cold prepare */", &[]),
    )
    .await;
    locker.unprepared("UNLOCK TABLES").await.unwrap();
    assert!(
        interrupted.is_err(),
        "the metadata lock must block cold prepare"
    );
    let reopened = conn.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap();
    assert_ne!(after, reopened);

    let error = conn
        .statement("SELECT * FROM private_missing_table", &[])
        .await
        .unwrap_err();
    assert!(!format!("{error:?}").contains("private_missing_table"));
    assert_eq!(
        conn.delete("DELETE FROM rust_probe WHERE title = ?", &[text.into()])
            .await
            .unwrap(),
        1
    );
    println!("MySQL {version}: prepared read/write, exact values and cancellation recovery passed");
}

fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, serde_json::Value>, ConfigError> {
    Ok(BTreeMap::from([(
        "database".into(),
        json!({"default":"split","connections":{"split":{
            "driver":"sqlite","database":"database/database.sqlite",
            "read":{"database":"database/reader.sqlite"},"sticky":true
        }}}),
    )]))
}

async fn probe_value() -> Result<Value, DatabaseError> {
    let id = DB::transaction(|transaction| Box::pin(interface_probe(transaction)), 1).await?;
    let mut connection = DB::connection(None)?;
    assert_eq!(interface_probe(&mut connection).await?, id);
    Ok(id)
}

async fn interface_probe(connection: &mut dyn ConnectionInterface) -> Result<Value, DatabaseError> {
    let id = connection
        .table("probe", Some("p"))
        .useWritePdo()
        .value("p.id")
        .await?;
    let found = connection
        .scalar(
            "SELECT id FROM probe WHERE id = :id",
            Bindings::Named(&[("id", id.clone())]),
            false,
        )
        .await?;
    assert_eq!(found, id);
    Ok(id)
}

fn replica(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let write = request.is("replica/write");
    let force_writer = request.is("replica/primary");
    Box::pin(async move {
        async fn read(write: bool, force_writer: bool) -> Result<Value, DatabaseError> {
            if write {
                DB::table("probe")?
                    .increment("id", 1, Default::default())
                    .await?;
            }
            let query = DB::table("probe")?;
            let query = if force_writer {
                query.useWritePdo()
            } else {
                query
            };
            let rows: Vec<_> = query.select(["id"]).cursor().try_collect().await?;
            Ok(rows
                .first()
                .and_then(|row| row.get_at(0))
                .cloned()
                .unwrap_or(Value::Null))
        }
        let id = read(write, force_writer).await?;
        let Value::Integer(id) = id else {
            panic!("expected integer")
        };
        Ok(Response::new(StatusCode::OK, Body::from(id.to_string())))
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    fn query(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let value = probe_value().await?;
            let Value::Integer(id) = value else {
                panic!("expected integer")
            };
            Ok(Response::new(StatusCode::OK, Body::from(id.to_string())))
        })
    }
    let mut router = Router::new();
    router.get("/database", query)?;
    router.get("/replica/{mode}", replica)?;

    Ok(router)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let mut request = Request::from_parts(parts, body);
    let (parts, body) = kernel.handle(&mut request).await.into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn prepare(root: &Root, id: i64) {
    let db = DatabaseManager::new(Arc::new(Repository::new(BTreeMap::new())), &root.0);
    let conn = db.connection(None).unwrap();
    let reader = root.0.join("database/reader.sqlite");
    fs::File::create(&reader).unwrap();
    let split = manager(
        root,
        json!({"connections":{"reader":{"driver":"sqlite","database":reader}}}),
    );
    let replica = split.connection(Some("reader")).unwrap();
    replica
        .statement("CREATE TABLE probe (id INTEGER)", &[])
        .await
        .unwrap();
    replica
        .insert("INSERT INTO probe VALUES (?)", &[(id * 10).into()])
        .await
        .unwrap();
    conn.statement("CREATE TABLE probe (id INTEGER)", &[])
        .await
        .unwrap();
    conn.insert("INSERT INTO probe VALUES (?)", &[id.into()])
        .await
        .unwrap();
}

async fn server(
    root: &Root,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<TransactionRecorder>,
    Arc<EstablishmentListener>,
) {
    let mut app = Application::configure(root.0.clone())
        .withConfig(config)
        .withRouting(Routing::new(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let recorder = transaction_recorder(&app);
    let established = establishment_listener(&app, false, false);
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
        recorder,
        established,
    )
}

async fn send(address: std::net::SocketAddr, path: &'static str, expected: &str) {
    let expected = expected.to_owned();
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (header, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(header.starts_with("HTTP/1.1 200 "));
        assert_eq!(body, expected);
    })
    .await
    .unwrap()
}

struct DatabaseCommand(CommandDefinition);
impl Command for DatabaseCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let value = probe_value()
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let Value::Integer(id) = value else {
                return Err(ConsoleError::new("expected integer"));
            };
            output.stdout.push_str(&format!("database probe {id}\n"));
            Ok(0)
        })
    }
}

fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(DatabaseCommand(CommandDefinition::new(
        clap::Command::new("db:probe"),
    ))))
}

#[tokio::test]
async fn db_facade_reads_sqlite_through_real_http_and_artisan() {
    const CHILD: &str = "OPENRUST_TEST_DATABASE_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        let mut app = Application::configure(root.into())
            .withCommands(commands)
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let recorder = transaction_recorder(&app);
        let established = establishment_listener(&app, false, false);
        assert_eq!(app.handleCommand(&["db:probe".into()]).await.unwrap(), 0);
        assert_eq!(established.calls.load(Ordering::SeqCst), 1);
        assert!(recorder.query_calls.load(Ordering::SeqCst) >= 2);
        assert_eq!(
            recorder.take(),
            [
                (TransactionBeginning::NAME, 1),
                (TransactionCommitting::NAME, 1),
                (TransactionCommitted::NAME, 0),
            ]
        );
        return;
    }
    let first = Root::new();
    let second = Root::new();
    tokio::join!(prepare(&first, 17), prepare(&second, 42));
    let ((a, x, events_a, established_a), (b, y, events_b, established_b)) =
        tokio::join!(server(&first), server(&second));
    tokio::join!(
        send(a, "/database", "17"),
        send(b, "/database", "42"),
        send(a, "/database", "17")
    );
    send(a, "/replica/read", "170").await;
    tokio::join!(
        send(a, "/replica/write", "18"),
        send(b, "/replica/read", "420")
    );
    send(a, "/replica/read", "170").await;
    send(a, "/replica/primary", "18").await;
    assert_eq!(established_a.calls.load(Ordering::SeqCst), 1);
    assert_eq!(established_b.calls.load(Ordering::SeqCst), 1);
    assert!(events_a.query_calls.load(Ordering::SeqCst) >= 4);
    assert!(events_b.query_calls.load(Ordering::SeqCst) >= 2);
    let mut observed = events_a.take();
    observed.sort_unstable();
    assert_eq!(
        observed,
        [
            (TransactionBeginning::NAME, 1),
            (TransactionBeginning::NAME, 1),
            (TransactionCommitted::NAME, 0),
            (TransactionCommitted::NAME, 0),
            (TransactionCommitting::NAME, 1),
            (TransactionCommitting::NAME, 1),
        ]
    );
    assert_eq!(
        events_b.take(),
        [
            (TransactionBeginning::NAME, 1),
            (TransactionCommitting::NAME, 1),
            (TransactionCommitted::NAME, 0),
        ]
    );
    for server in [x, y] {
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "db_facade_reads_sqlite_through_real_http_and_artisan",
            "--nocapture",
        ])
        .env(CHILD, &second.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("database probe 42"));
}

#[tokio::test]
async fn sqlite_manual_transactions_savepoints_and_cancellation_keep_the_writer_consistent() {
    let root = Root::new();
    let manager = manager(
        &root,
        json!({
            "default":"memory","connections":{"memory":"sqlite::memory:"}
        }),
    );
    let connection = manager.connection(None).unwrap();
    connection
        .statement("CREATE TABLE txn (id INTEGER PRIMARY KEY)", &[])
        .await
        .unwrap();
    let mut transaction = connection.beginTransaction().await.unwrap();
    transaction
        .insert("INSERT INTO txn VALUES (1)", &[])
        .await
        .unwrap();
    transaction.beginTransaction().await.unwrap();
    transaction
        .insert("INSERT INTO txn VALUES (2)", &[])
        .await
        .unwrap();
    transaction.beginTransaction().await.unwrap();
    transaction
        .insert("INSERT INTO txn VALUES (3)", &[])
        .await
        .unwrap();
    assert_eq!(transaction.transactionLevel(), 3);
    transaction.rollBack(Some(1)).await.unwrap();
    assert_eq!(transaction.transactionLevel(), 1);
    assert_eq!(
        transaction
            .scalar("SELECT COUNT(*) FROM txn", &[])
            .await
            .unwrap(),
        1.into()
    );
    transaction.commit().await.unwrap();
    assert!(matches!(
        transaction.scalar("SELECT 1", &[]).await,
        Err(DatabaseError::TransactionClosed)
    ));

    let error = connection
        .transaction(
            |tx| {
                Box::pin(async {
                    tx.insert("INSERT INTO txn VALUES (4)", &[]).await?;
                    tx.insert("INSERT INTO txn VALUES (1)", &[]).await?;
                    Ok(())
                })
            },
            3,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        DatabaseError::Driver {
            operation: "execute",
            ..
        }
    ));
    let mut transaction = connection.beginTransaction().await.unwrap();
    transaction
        .insert("INSERT INTO txn VALUES (5)", &[])
        .await
        .unwrap();
    transaction.beginTransaction().await.unwrap();
    transaction
        .insert("INSERT INTO txn VALUES (6)", &[])
        .await
        .unwrap();
    drop(transaction);
    assert_eq!(
        connection
            .scalar("SELECT COUNT(*) FROM txn", &[])
            .await
            .unwrap(),
        1.into()
    );
}

#[tokio::test]
async fn real_mysql_transactions_retry_lock_failure_and_retire_cancelled_sessions() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = manager(
        &root,
        json!({"default":"writer","connections":{
            "writer":{"url":url,"max_connections":1,"acquire_timeout":5},
            "locker":{"url":url,"max_connections":1,"acquire_timeout":5}
        }}),
    );
    fn writer_config(
        _: &Path,
        _: &Environment,
    ) -> Result<BTreeMap<String, serde_json::Value>, ConfigError> {
        let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
        Ok([("database".into(), json!({
            "default":"writer","connections":{"writer":{"url":url,"max_connections":1,"acquire_timeout":5}}
        }))].into())
    }
    let (_app, writer, events) =
        record_transactions(Application::configure(root.0.clone()).withConfig(writer_config)).await;
    writer.unprepared("DROP TABLE IF EXISTS rust_txn; CREATE TABLE rust_txn (id INT PRIMARY KEY, n INT); INSERT INTO rust_txn VALUES (1, 0)").await.unwrap();
    let mut locker = db
        .connection(Some("locker"))
        .unwrap()
        .beginTransaction()
        .await
        .unwrap();
    locker
        .update("UPDATE rust_txn SET n = 100 WHERE id = 1", &[])
        .await
        .unwrap();
    let (release, released) = tokio::sync::oneshot::channel();
    let release = Arc::new(std::sync::Mutex::new(Some(release)));
    let locker = tokio::spawn(async move {
        released.await.unwrap();
        locker.rollBack(None).await.unwrap();
    });
    let attempts = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let before_calls = Arc::new(AtomicUsize::new(0));
    let starts = before_calls.clone();
    writer.beforeStartingTransaction(move |_, _| {
        let starts = starts.clone();
        Box::pin(async move {
            starts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    writer
        .transaction(
            |tx| {
                let release = release.clone();
                let attempts = attempts.clone();
                let rollbacks = rollbacks.clone();
                Box::pin(async move {
                    tx.afterRollBack(move || {
                        Box::pin(async move {
                            rollbacks.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        })
                    })?;
                    let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                    tx.unprepared("SET SESSION innodb_lock_wait_timeout = 1")
                        .await?;
                    let result = tx
                        .update("UPDATE rust_txn SET n = n + 1 WHERE id = 1", &[])
                        .await;
                    if attempt == 0 {
                        assert!(result.is_err(), "the competing lock must force a retry");
                        release.lock().unwrap().take().unwrap().send(()).unwrap();
                    }
                    result
                })
            },
            2,
        )
        .await
        .unwrap();
    locker.await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(before_calls.load(Ordering::SeqCst), 2);
    assert_eq!(rollbacks.load(Ordering::SeqCst), 1);
    assert_eq!(
        events.take(),
        [
            (TransactionBeginning::NAME, 1),
            (TransactionRolledBack::NAME, 0),
            (TransactionBeginning::NAME, 1),
            (TransactionCommitting::NAME, 1),
            (TransactionCommitted::NAME, 0),
        ]
    );
    assert_eq!(
        writer
            .scalar("SELECT n FROM rust_txn WHERE id = 1", &[])
            .await
            .unwrap(),
        1.into()
    );

    let mut tx = writer.beginTransaction().await.unwrap();
    let before = tx.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap();
    tx.update("UPDATE rust_txn SET n = 900 WHERE id = 1", &[])
        .await
        .unwrap();
    let nested: Result<(), _> = tx
        .transaction(|nested| {
            Box::pin(async {
                nested
                    .update("UPDATE rust_txn SET n = 800 WHERE id = 1", &[])
                    .await?;
                nested
                    .scalar("SELECT * FROM rust_missing_txn_table", &[])
                    .await?;
                Ok(())
            })
        })
        .await;
    assert!(nested.is_err());
    assert_eq!(
        tx.scalar("SELECT n FROM rust_txn WHERE id = 1", &[])
            .await
            .unwrap(),
        900.into()
    );
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            tx.scalar("SELECT SLEEP(2)", &[])
        )
        .await
        .is_err()
    );
    assert!(matches!(
        tx.commit().await,
        Err(DatabaseError::TransactionInterrupted)
    ));
    drop(tx);
    assert_ne!(
        writer.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap(),
        before
    );
    assert_eq!(
        writer
            .scalar("SELECT n FROM rust_txn WHERE id = 1", &[])
            .await
            .unwrap(),
        1.into()
    );
}

#[tokio::test]
async fn sqlite_direct_alias_uses_the_writer_and_reports_its_own_name() {
    let root = Root::new();
    fs::File::create(root.0.join("database/reader.sqlite")).unwrap();
    fs::File::create(root.0.join("database/writer.sqlite")).unwrap();
    let db = manager(
        &root,
        json!({"default":"split","connections":{
            "split":{"driver":"sqlite","database":"database/database.sqlite",
                     "read":{"database":"database/reader.sqlite"},
                     "write":{"database":"database/writer.sqlite"}}
        }}),
    );
    let direct = db.connection(Some("split::direct")).unwrap();
    assert_eq!(direct.getName(), "split::direct");
    direct.enableQueryLog();
    direct.unprepared("PRAGMA user_version = 21").await.unwrap();
    assert_eq!(
        direct.scalar("PRAGMA user_version", &[]).await.unwrap(),
        21.into()
    );
    assert!(
        direct
            .getQueryLog()
            .iter()
            .all(|entry| entry.read_write_type == QueryRole::Direct)
    );
    let regular = db.connection(None).unwrap();
    assert_eq!(
        regular.scalar("PRAGMA user_version", &[]).await.unwrap(),
        0.into()
    );
    assert_eq!(
        regular
            .selectFromWriteConnection("PRAGMA user_version", &[])
            .await
            .unwrap()[0]
            .get_at(0),
        Some(&21.into())
    );
    let reconnected = db.reconnect(Some("split::direct")).await.unwrap();
    assert_eq!(reconnected.getName(), "split::direct");
    assert_eq!(
        reconnected
            .scalar("PRAGMA user_version", &[])
            .await
            .unwrap(),
        21.into()
    );
}

#[tokio::test]
async fn postgres_direct_endpoint_and_migration_role_resolve_without_connecting() {
    use openrust::database::migrations::{MigrationRegistry, Migrator};

    let root = Root::new();
    let manager = Arc::new(manager(
        &root,
        json!({"default":"split","connections":{
            "split":{
                "url":"postgresql://base_user@localhost/base_db", "prefix":"app_",
                "read":{"database":"read_db"},
                "write":{"database":"write_db"},
                "direct":{"url":"postgresql://direct_user@localhost/direct_db"}
            },
            "fallback":{"driver":"pgsql","database":"fallback_db","direct":{}},
            "empty_url":{"driver":"pgsql","database":"base_db",
                         "write":{"database":"only_writer"},"direct":""},
            "invalid":{"driver":"pgsql","direct":{"url":"mysql://localhost/wrong_driver"}}
        }}),
    ));
    let base = manager.connection(None).unwrap();
    assert!(base.hasDirectConnection());
    assert_eq!(base.getDatabaseName(), "write_db");
    let direct = manager.connection(Some("split::direct")).unwrap();
    assert!(direct.hasDirectConnection());
    assert_eq!(direct.getName(), "split::direct");
    assert_eq!(direct.getDatabaseName(), "direct_db");
    assert_eq!(direct.getTablePrefix(), "app_");

    let migrator = Migrator::new(
        manager.clone(),
        Arc::new(MigrationRegistry(&[])),
        "migrations".into(),
        root.0.clone(),
    );
    for (name, database) in [
        (None, "direct_db"),
        (Some("split"), "direct_db"),
        (Some("split::direct"), "direct_db"),
        (Some("split::write"), "write_db"),
        (Some("split::read"), "read_db"),
        (Some("fallback"), "fallback_db"),
        (Some("fallback::direct"), "fallback_db"),
        (Some("empty_url::direct"), "only_writer"),
    ] {
        assert_eq!(
            migrator.resolveConnection(name).unwrap().getDatabaseName(),
            database
        );
    }
    assert!(
        !manager
            .connection(Some("fallback"))
            .unwrap()
            .hasDirectConnection()
    );
    assert!(matches!(
        migrator.resolveConnection(Some("invalid")),
        Err(DatabaseError::InvalidConfiguration("direct driver"))
    ));
    assert_eq!(manager.getDefaultConnection().unwrap(), "split");
}

#[tokio::test]
async fn mysql_read_write_routing_recovers_after_a_server_disconnect() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = manager(
        &root,
        json!({"default":"split","connections":{
            "split":{"url":url,"read":{"host":["127.0.0.1","127.0.0.2"]},"sticky":true,
                     "direct":{"url":"postgresql://localhost/unused"},
                     "max_connections":1,"acquire_timeout":2},
            "control":{"url":url,"max_connections":1,"acquire_timeout":2}
        }}),
    );
    let direct = db.connection(Some("split::direct")).unwrap();
    assert_eq!(direct.getName(), "split::direct");
    assert!(!direct.hasDirectConnection());
    direct
        .statement("SET @openrust_direct_probe = 23", &[])
        .await
        .unwrap();
    assert_eq!(
        direct
            .scalar("SELECT CAST(@openrust_direct_probe AS SIGNED)", &[])
            .await
            .unwrap(),
        23.into()
    );
    let mut split = db.connection(None).unwrap();
    let writer = split
        .selectFromWriteConnection("SELECT CONNECTION_ID()", &[])
        .await
        .unwrap();
    let writer_id = writer[0].get_at(0).unwrap().clone();
    let reader_id = split.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap();
    assert_ne!(writer_id, reader_id);
    let mut transaction = split.beginTransaction().await.unwrap();
    assert_eq!(
        transaction
            .scalar("SELECT CONNECTION_ID()", &[])
            .await
            .unwrap(),
        writer_id
    );
    transaction.rollBack(None).await.unwrap();
    let role_probe = "SELECT CONNECTION_ID()";
    for (read, expected) in [(true, &reader_id), (false, &writer_id)] {
        let connection: &mut dyn ConnectionInterface = &mut split;
        assert_eq!(
            &connection
                .scalar(role_probe, (&[]).into(), read)
                .await
                .unwrap(),
            expected
        );
        let rows: Vec<_> = connection
            .cursor(role_probe, (&[]).into(), read)
            .try_collect()
            .await
            .unwrap();
        assert_eq!(rows[0].get_at(0), Some(expected));
    }
    let mut transaction = split.beginTransaction().await.unwrap();
    let connection: &mut dyn ConnectionInterface = &mut transaction;
    assert_eq!(connection.getDatabaseName(), split.getDatabaseName());
    assert_eq!(
        connection
            .scalar(role_probe, (&[]).into(), true)
            .await
            .unwrap(),
        writer_id
    );
    transaction.rollBack(None).await.unwrap();

    split
        .statement("CREATE TABLE IF NOT EXISTS rust_split_probe (id INT)", &[])
        .await
        .unwrap();
    let mut transaction = split.beginTransaction().await.unwrap();
    let connection: &mut dyn ConnectionInterface = &mut transaction;
    assert!(
        connection
            .getSchemaBuilder()
            .hasTable("rust_split_probe")
            .await
            .unwrap()
    );
    assert!(
        connection
            .insert(
                "INSERT INTO rust_split_probe (id) VALUES (:id)",
                Bindings::Named(&[("id", 31.into())]),
            )
            .await
            .unwrap()
    );
    let expression = connection.raw("q.id + 1 as incremented");
    let row = connection
        .table("rust_split_probe", Some("q"))
        .select([expression])
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("incremented"), Some(&32.into()));
    transaction.rollBack(None).await.unwrap();
    assert_eq!(split.table("rust_split_probe").count("*").await.unwrap(), 0);
    assert_eq!(
        split.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap(),
        writer_id
    );
    split.forgetRecordModificationState();
    assert_eq!(
        split
            .update("UPDATE rust_split_probe SET id = 1 WHERE 1 = 0", &[])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        split.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap(),
        reader_id
    );
    split.useWriteConnectionWhenReading(true);
    assert_eq!(
        split.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap(),
        writer_id
    );
    split.useWriteConnectionWhenReading(false);

    let reader_number = match reader_id {
        Value::Unsigned(value) => value,
        Value::Integer(value) => u64::try_from(value).unwrap(),
        _ => panic!("expected numeric MySQL connection id"),
    };
    let control = db.connection(Some("control")).unwrap();
    control
        .unprepared(&format!("KILL CONNECTION {reader_number}"))
        .await
        .unwrap();
    let recovered = split.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap();
    assert_ne!(recovered, reader_id);
    assert_ne!(recovered, writer_id);

    assert_ne!(
        db.connection(Some("split::read"))
            .unwrap()
            .scalar("SELECT CONNECTION_ID()", &[])
            .await
            .unwrap(),
        db.connection(Some("split::write"))
            .unwrap()
            .scalar("SELECT CONNECTION_ID()", &[])
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn mysql_retries_a_nested_deadlock_from_the_outer_transaction() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = manager(
        &root,
        json!({"default":"writer","connections":{
            "writer":{"url":url,"max_connections":1,"acquire_timeout":5},
            "locker":{"url":url,"max_connections":1,"acquire_timeout":5}
        }}),
    );
    let writer = db.connection(None).unwrap();
    writer.unprepared("DROP TABLE IF EXISTS rust_deadlock; CREATE TABLE rust_deadlock (id INT PRIMARY KEY, n INT); INSERT INTO rust_deadlock VALUES (1,0),(2,0),(3,0),(4,0),(5,0),(6,0),(7,0),(8,0)").await.unwrap();
    let mut locker = db
        .connection(Some("locker"))
        .unwrap()
        .beginTransaction()
        .await
        .unwrap();
    locker
        .update("UPDATE rust_deadlock SET n = 100 WHERE id >= 2", &[])
        .await
        .unwrap();
    let locked = Arc::new(Notify::new());
    let attempts = Arc::new(AtomicUsize::new(0));
    let rollbacks = Arc::new(AtomicUsize::new(0));
    let competing = async {
        locked.notified().await;
        locker
            .update("UPDATE rust_deadlock SET n = 100 WHERE id = 1", &[])
            .await
            .unwrap();
        locker.rollBack(None).await.unwrap();
    };
    let run = writer.transaction(
        |tx| {
            let locked = locked.clone();
            let attempts = attempts.clone();
            let rollbacks = rollbacks.clone();
            Box::pin(async move {
                tx.afterRollBack(move || {
                    Box::pin(async move {
                        rollbacks.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                let first = attempts.fetch_add(1, Ordering::SeqCst) == 0;
                tx.update("UPDATE rust_deadlock SET n = n + 1 WHERE id = 1", &[])
                    .await?;
                if first {
                    locked.notify_one();
                }
                let nested = tx
                    .transaction(|savepoint| {
                        Box::pin(async {
                            savepoint
                                .update("UPDATE rust_deadlock SET n = n + 1 WHERE id = 2", &[])
                                .await
                        })
                    })
                    .await;
                if first {
                    let Err(DatabaseError::Driver { source, .. }) = &nested else {
                        panic!("the first nested write must hit a database deadlock");
                    };
                    let error = source
                        .as_database_error()
                        .unwrap()
                        .downcast_ref::<sqlx::mysql::MySqlDatabaseError>();
                    assert_eq!(error.number(), 1213);
                }
                nested
            })
        },
        2,
    );
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(run, competing)
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap(), 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(rollbacks.load(Ordering::SeqCst), 1);
    assert_eq!(
        writer
            .scalar("SELECT SUM(n) FROM rust_deadlock WHERE id IN (1, 2)", &[])
            .await
            .unwrap(),
        "2".into()
    );
}

async fn verify_named_bindings(db: &Connection) {
    db.statement("DROP TABLE IF EXISTS named_binding_items", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE named_binding_items (id INTEGER PRIMARY KEY, note VARCHAR(80))",
        &[],
    )
    .await
    .unwrap();

    let note = Value::from("héllo :literal ? ' ;");
    db.insert(
        "INSERT INTO named_binding_items VALUES (:id, :note)",
        Bindings::Named(&[("note", note.clone()), (":id", 7.into())]),
    )
    .await
    .unwrap();
    let integer_key = [("id", 7.into())];
    let key = Bindings::Named(&integer_key);
    let row = db.selectFromWriteConnection(
        "SELECT ':literal ? café' AS literal, note AS `:alias` /* :comment ? */ FROM named_binding_items WHERE id = :id -- :trailing ?",
        key,
    ).await.unwrap();
    assert_eq!(row[0].get("literal"), Some(&":literal ? café".into()));
    assert_eq!(row[0].get(":alias"), Some(&note));

    let mut tx = db.beginTransaction().await.unwrap();
    assert_eq!(
        tx.update(
            "UPDATE named_binding_items SET note = :note WHERE id = :id",
            Bindings::Named(&[("id", 7.into()), ("note", "changed".into())]),
        )
        .await
        .unwrap(),
        1
    );
    let mut cursor = tx.cursor("SELECT note FROM named_binding_items WHERE id = :id", key);
    assert_eq!(
        cursor.try_next().await.unwrap().unwrap().get_at(0),
        Some(&"changed".into())
    );
    assert!(cursor.try_next().await.unwrap().is_none());
    drop(cursor);
    tx.rollBack(None).await.unwrap();
    assert_eq!(
        db.scalar("SELECT note FROM named_binding_items WHERE id = :id", key)
            .await
            .unwrap(),
        note,
    );
    let mut cursor = db.cursor("SELECT note FROM named_binding_items WHERE id = :id", key);
    assert_eq!(
        cursor.try_next().await.unwrap().unwrap().get_at(0),
        Some(&note)
    );
    assert!(cursor.try_next().await.unwrap().is_none());
    assert_eq!(
        db.delete("DELETE FROM named_binding_items WHERE id = :id", key)
            .await
            .unwrap(),
        1
    );

    for (sql, params, reason) in [
        ("SELECT :missing", key, "missing binding key"),
        ("SELECT 1", key, "unused binding key"),
        (
            "SELECT :id, ?",
            key,
            "cannot mix named and positional placeholders",
        ),
        (
            "SELECT :id",
            Bindings::Named(&[("id", 1.into()), (":id", 2.into())]),
            "duplicate binding key",
        ),
    ] {
        assert!(matches!(
            db.select(sql, params).await,
            Err(DatabaseError::InvalidNamedBindings(failure)) if failure == reason
        ));
    }
    db.statement("DROP TABLE named_binding_items", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_supports_named_bindings_and_reused_parameters() {
    let root = Root::new();
    let config = json!({"default":"sqlite", "connections":{"sqlite":{"driver":"sqlite","database":":memory:"}}});
    let db = manager(&root, config).connection(None).unwrap();
    verify_named_bindings(&db).await;
    let rows = db
        .select(
            r#"SELECT :value AS ":name", :value AS [:brackets], 'it''s :text ?' AS text"#,
            Bindings::Named(&[("value", 12.into())]),
        )
        .await
        .unwrap();
    assert_eq!(rows[0].get(":name"), Some(&12.into()));
    assert_eq!(rows[0].get(":brackets"), Some(&12.into()));
    assert_eq!(rows[0].get("text"), Some(&"it's :text ?".into()));
}

#[tokio::test]
async fn mysql_supports_named_bindings_in_prepared_procedures() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "MySQL acceptance requires an isolated database"
        );
        return;
    };
    let root = Root::new();
    let config = json!({"default":"mysql", "connections":{"mysql":{"url":url}}});
    let db = manager(&root, config).connection(None).unwrap();
    verify_named_bindings(&db).await;
    let param = [("value", 12.into())];
    let value = Bindings::Named(&param);
    let rows = db
        .select(
            r"SELECT 'it\'s :text ?' AS txt, :value AS bound # :comment ?",
            value,
        )
        .await
        .unwrap();
    assert_eq!(rows[0].get("txt"), Some(&"it's :text ?".into()));
    assert_eq!(rows[0].get("bound"), Some(&12.into()));
    assert!(matches!(
        db.select("SELECT :value, :value", value).await,
        Err(DatabaseError::InvalidNamedBindings(
            "repeated placeholder on MySQL"
        ))
    ));
    db.unprepared("DROP PROCEDURE IF EXISTS openrust_named_rowsets")
        .await
        .unwrap();
    db.unprepared(
        "CREATE PROCEDURE openrust_named_rowsets(IN n INT) BEGIN SELECT n AS value; SELECT n + 1 AS value; END",
    ).await.unwrap();
    let results = db
        .selectResultSets("CALL openrust_named_rowsets(:value)", value, true)
        .await
        .unwrap();
    assert_eq!(results.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1, 0]);
    assert_eq!(results[0][0].get("value"), Some(&12.into()));
    assert_eq!(results[1][0].get("value"), Some(&13.into()));
    db.unprepared("DROP PROCEDURE openrust_named_rowsets")
        .await
        .unwrap();
}

#[tokio::test]
async fn mysql_tls_distinguishes_ca_and_server_identity() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_TLS_URL") else {
        return;
    };
    // The fixture presents a certificate with an IP SAN for 127.0.0.1 only.
    let root = Root::new();
    let connect = |url: &str| {
        manager(
            &root,
            json!({"default":"mysql","connections":{"mysql":{"url":url}}}),
        )
        .connection(None)
        .unwrap()
    };
    assert_eq!(
        connect(&url).scalar("SELECT 1", &[]).await.unwrap(),
        1.into()
    );

    let mut mismatch = url::Url::parse(&url).unwrap();
    mismatch.set_host(Some("localhost")).unwrap();
    let error = connect(mismatch.as_str())
        .scalar("SELECT 1", &[])
        .await
        .unwrap_err();
    let DatabaseError::Driver {
        source: sqlx::Error::Io(io_error),
        ..
    } = error
    else {
        panic!("expected a TLS handshake I/O error, got {error}");
    };
    assert!(matches!(
        io_error
            .get_ref()
            .and_then(|source| source.downcast_ref::<rustls::Error>()),
        Some(rustls::Error::InvalidCertificate(
            rustls::CertificateError::NotValidForName
                | rustls::CertificateError::NotValidForNameContext { .. }
        ))
    ));

    mismatch
        .query_pairs_mut()
        .append_pair("ssl-mode", "verify_ca");
    assert_eq!(
        connect(mismatch.as_str())
            .scalar("SELECT 1", &[])
            .await
            .unwrap(),
        1.into()
    );
}

#[derive(Default)]
struct TransactionRecorder {
    query_calls: AtomicUsize,
    calls: Mutex<Vec<(&'static str, u32)>>,
    fail: Mutex<Option<&'static str>>,
}

impl TransactionRecorder {
    fn take(&self) -> Vec<(&'static str, u32)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl Listener for TransactionRecorder {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventPayload,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if event.downcast_ref::<QueryExecuted>().is_some() {
                self.query_calls.fetch_add(1, Ordering::SeqCst);
                return Ok(None);
            }
            macro_rules! payload {
                ($kind:ty) => {
                    event
                        .downcast_ref::<$kind>()
                        .map(|event| (<$kind>::NAME, &event.connection, event.level))
                };
            }
            let (name, connection, level) = payload!(TransactionBeginning)
                .or_else(|| payload!(TransactionCommitting))
                .or_else(|| payload!(TransactionCommitted))
                .or_else(|| payload!(TransactionRolledBack))
                .ok_or(EventError::InvalidPayload)?;
            assert_eq!(
                context.transaction().map(|tx| tx.transactionLevel()),
                (level > 0).then_some(level)
            );
            let scoped = DB::connection(None).map_err(EventError::Transaction)?;
            assert_eq!(connection.getName(), scoped.getName());
            assert_eq!(connection.getDatabaseName(), scoped.getDatabaseName());
            self.calls.lock().unwrap().push((name, level));
            if *self.fail.lock().unwrap() == Some(name) {
                return Err(EventError::Listener {
                    source: std::io::Error::other("transaction event probe").into(),
                });
            }
            Ok(None)
        })
    }
}

fn transaction_recorder(app: &Application) -> Arc<TransactionRecorder> {
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    let recorder = Arc::new(TransactionRecorder::default());
    dispatcher
        .listen(
            r"OpenRust\Database\Events\Transaction*",
            recorder.clone(),
            Delivery::Sync,
        )
        .unwrap();
    dispatcher
        .listen(QueryExecuted::NAME, recorder.clone(), Delivery::Sync)
        .unwrap();
    recorder
}

async fn record_transactions(
    builder: ApplicationBuilder,
) -> (Application, Connection, Arc<TransactionRecorder>) {
    let mut app = builder.create().unwrap();
    app.boot().await.unwrap();
    let recorder = transaction_recorder(&app);
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    (app, connection, recorder)
}

async fn verify_transaction_lifecycle(db: &Connection, recorder: &Arc<TransactionRecorder>) {
    use openrust::database::events::{
        TransactionBeginning as Began, TransactionCommitted as Committed,
        TransactionCommitting as Committing, TransactionRolledBack as RolledBack,
    };
    db.statement("DROP TABLE IF EXISTS transaction_event_records", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE transaction_event_records (id INTEGER PRIMARY KEY)",
        &[],
    )
    .await
    .unwrap();

    let mut tx = db.beginTransaction().await.unwrap();
    tx.beginTransaction().await.unwrap();
    tx.insert("INSERT INTO transaction_event_records VALUES (1)", &[])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    tx.beginTransaction().await.unwrap();
    tx.beginTransaction().await.unwrap();
    tx.insert("INSERT INTO transaction_event_records VALUES (2)", &[])
        .await
        .unwrap();
    tx.rollBack(Some(1)).await.unwrap();
    tx.rollBack(Some(1)).await.unwrap();
    let callbacks = recorder.clone();
    tx.afterCommit(move || {
        Box::pin(async move {
            callbacks.calls.lock().unwrap().push(("afterCommit", 0));
            Ok(())
        })
    })
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        recorder.take(),
        [
            (Began::NAME, 1),
            (Began::NAME, 2),
            (Committed::NAME, 1),
            (Began::NAME, 2),
            (Began::NAME, 3),
            (RolledBack::NAME, 1),
            (Committing::NAME, 1),
            ("afterCommit", 0),
            (Committed::NAME, 0),
        ]
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM transaction_event_records", &[])
            .await
            .unwrap(),
        1.into()
    );

    *recorder.fail.lock().unwrap() = Some(Committing::NAME);
    let result = db
        .transaction(
            |tx| {
                Box::pin(async move {
                    tx.insert("INSERT INTO transaction_event_records VALUES (3)", &[])
                        .await
                })
            },
            2,
        )
        .await;
    assert!(matches!(result, Err(DatabaseError::ConnectionEvent(_))));
    assert_eq!(
        recorder.take(),
        [
            (Began::NAME, 1),
            (Committing::NAME, 1),
            (RolledBack::NAME, 0)
        ]
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM transaction_event_records", &[])
            .await
            .unwrap(),
        1.into()
    );

    *recorder.fail.lock().unwrap() = Some(Committed::NAME);
    let result = db
        .transaction(
            |tx| {
                Box::pin(async move {
                    tx.insert("INSERT INTO transaction_event_records VALUES (4)", &[])
                        .await
                })
            },
            2,
        )
        .await;
    assert!(matches!(result, Err(DatabaseError::AfterCommit { .. })));
    assert_eq!(
        recorder.take(),
        [
            (Began::NAME, 1),
            (Committing::NAME, 1),
            (Committed::NAME, 0)
        ]
    );
    assert_eq!(
        db.scalar("SELECT count(*) FROM transaction_event_records", &[])
            .await
            .unwrap(),
        2.into()
    );

    db.statement("DROP TABLE transaction_event_records", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_transaction_events_are_ordered_and_application_local() {
    fn sqlite_config(
        _: &Path,
        _: &Environment,
    ) -> Result<BTreeMap<String, serde_json::Value>, ConfigError> {
        Ok(BTreeMap::new())
    }
    let root = Root::new();
    let other_root = Root::new();
    let (_app, connection, events) =
        record_transactions(Application::configure(root.0.clone()).withConfig(sqlite_config)).await;
    let (_other_app, other, other_events) =
        record_transactions(Application::configure(other_root.0.clone()).withConfig(sqlite_config))
            .await;
    verify_transaction_lifecycle(&connection, &events).await;
    assert!(other_events.take().is_empty());
    other
        .beginTransaction()
        .await
        .unwrap()
        .rollBack(None)
        .await
        .unwrap();
    assert_eq!(
        other_events.take(),
        [
            (TransactionBeginning::NAME, 1),
            (TransactionRolledBack::NAME, 0)
        ]
    );
    assert!(events.take().is_empty());
}

#[tokio::test]
async fn mysql_transactions_deliver_events_after_confirmed_driver_operations() {
    fn mysql_config(
        _: &Path,
        _: &Environment,
    ) -> Result<BTreeMap<String, serde_json::Value>, ConfigError> {
        let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
        Ok([(
            "database".into(),
            json!({"default":"mysql","connections":{"mysql":{"url":url}}}),
        )]
        .into())
    }
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    let root = Root::new();
    let (_app, connection, events) =
        record_transactions(Application::configure(root.0.clone()).withConfig(mysql_config)).await;
    verify_transaction_lifecycle(&connection, &events).await;
}

struct EstablishmentListener {
    calls: AtomicUsize,
    initialize: bool,
    fail: bool,
}

impl Listener for EstablishmentListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event
                .downcast_ref::<ConnectionEstablished>()
                .ok_or(EventError::InvalidPayload)?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            let resolved = DB::connection(None).map_err(EventError::Transaction)?;
            assert_eq!(
                event.connection.getDatabaseName(),
                resolved.getDatabaseName()
            );
            assert_eq!(
                resolved
                    .scalar("SELECT 1", &[])
                    .await
                    .map_err(EventError::Transaction)?,
                1.into()
            );
            if self.initialize {
                event
                    .connection
                    .statement(
                        "CREATE TABLE establishment_probe (id INTEGER NOT NULL)",
                        &[],
                    )
                    .await
                    .map_err(EventError::Transaction)?;
                event
                    .connection
                    .insert("INSERT INTO establishment_probe VALUES (?)", &[9.into()])
                    .await
                    .map_err(EventError::Transaction)?;
            }
            if self.fail {
                return Err(EventError::Listener {
                    source: std::io::Error::other("establishment listener probe").into(),
                });
            }
            Ok(None)
        })
    }
}

fn establishment_listener(
    app: &Application,
    initialize: bool,
    fail: bool,
) -> Arc<EstablishmentListener> {
    let listener = Arc::new(EstablishmentListener {
        calls: AtomicUsize::new(0),
        initialize,
        fail,
    });
    app.container()
        .make::<Dispatcher>()
        .unwrap()
        .listen(
            ConnectionEstablished::NAME,
            listener.clone(),
            Delivery::Sync,
        )
        .unwrap();
    listener
}

async fn verify_establishment(root: &Root, builder: ApplicationBuilder) {
    let mut app = builder.create().unwrap();
    app.boot().await.unwrap();
    let first_listener = establishment_listener(&app, true, false);
    let manager = app.container().make::<DatabaseManager>().unwrap();
    let first = manager.connection(None).unwrap();
    let second = manager.connection(None).unwrap();
    assert_eq!(first_listener.calls.load(Ordering::SeqCst), 0);
    let (a, b) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            first.scalar("SELECT id FROM establishment_probe", &[]),
            second.scalar("SELECT id FROM establishment_probe", &[]),
        )
    })
    .await
    .expect("an asynchronous listener may query its own connection");
    assert_eq!(a.unwrap(), 9.into());
    assert_eq!(b.unwrap(), 9.into());
    first.establish().await.unwrap();
    assert_eq!(first_listener.calls.load(Ordering::SeqCst), 1);
    first
        .statement("DROP TABLE establishment_probe", &[])
        .await
        .unwrap();

    let mut second_app = Application::configure(root.0.clone()).create().unwrap();
    second_app.boot().await.unwrap();
    let second_listener = establishment_listener(&second_app, false, true);
    let manager = second_app.container().make::<DatabaseManager>().unwrap();
    let connection = manager.connection(None).unwrap();
    assert!(matches!(
        connection.establish().await,
        Err(DatabaseError::ConnectionEvent(_))
    ));
    assert_eq!(connection.scalar("SELECT 1", &[]).await.unwrap(), 1.into());
    assert_eq!(second_listener.calls.load(Ordering::SeqCst), 1);
    assert_eq!(first_listener.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sqlite_connection_established_runs_once_before_concurrent_queries() {
    let root = Root::new();
    let second = Root::new();
    verify_establishment(&second, Application::configure(root.0.clone())).await;
}

#[tokio::test]
async fn mysql_connection_established_can_initialize_its_own_connection() {
    fn config(
        _: &Path,
        _: &Environment,
    ) -> Result<BTreeMap<String, serde_json::Value>, ConfigError> {
        let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
        Ok([(
            "database".into(),
            json!({"default":"mysql","connections":{"mysql":{"url":url}}}),
        )]
        .into())
    }
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    let root = Root::new();
    let second = Root::new();
    verify_establishment(
        &second,
        Application::configure(root.0.clone()).withConfig(config),
    )
    .await;
}

#[tokio::test]
async fn canceled_establishment_allows_the_next_operation_to_dispatch() {
    #[derive(Default)]
    struct PauseFirst(AtomicUsize);
    impl Listener for PauseFirst {
        fn handle<'a>(
            &'a self,
            _: &'a dyn EventPayload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                    std::future::pending::<()>().await;
                }
                Ok(None)
            })
        }
    }

    let root = Root::new();
    let mut app = Application::configure(root.0.clone()).create().unwrap();
    app.boot().await.unwrap();
    let listener = Arc::new(PauseFirst::default());
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    dispatcher
        .listen(
            ConnectionEstablished::NAME,
            listener.clone(),
            Delivery::Sync,
        )
        .unwrap();
    let manager = app.container().make::<DatabaseManager>().unwrap();
    let connection = manager.connection(None).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), connection.establish())
            .await
            .is_err()
    );
    assert_eq!(listener.0.load(Ordering::SeqCst), 1);
    assert_eq!(connection.scalar("SELECT 1", &[]).await.unwrap(), 1.into());
    assert_eq!(listener.0.load(Ordering::SeqCst), 2);
}

async fn verify_query_preview(connection: Connection) {
    connection
        .unprepared("CREATE TABLE query_preview_probe (n BIGINT)")
        .await
        .unwrap();
    connection.forgetRecordModificationState();
    connection.enableQueryLog();
    connection.scalar("SELECT 1", &[]).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    connection.beforeExecuting(move |sql, _, _, _| {
        let seen = seen.clone();
        Box::pin(async move {
            if sql.starts_with("INSERT INTO query_preview_probe") {
                seen.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        })
    });

    let (preview, real) = tokio::join!(
        connection.pretend(|db| Box::pin(async move {
            assert!(db.clone().pretending());
            tokio::task::yield_now().await;
            assert!(
                db.statement("INSERT INTO query_preview_probe VALUES (?)", &[8.into()])
                    .await?
            );
            assert!(!db.hasModifiedRecords());
            assert_eq!(
                db.affectingStatement("DELETE FROM query_preview_probe", &[])
                    .await?,
                0
            );
            assert_eq!(db.scalar("SELECT ? AS n", &[16.into()]).await?, Value::Null);
            assert!(
                db.cursor("SELECT 26", &[])
                    .try_collect::<Vec<_>>()
                    .await?
                    .is_empty()
            );
            assert!(matches!(
                db.beginTransaction().await,
                Err(DatabaseError::UnsupportedOperation(_))
            ));
            let nested = db
                .pretend(|db| {
                    Box::pin(async move {
                        db.unprepared("INSERT INTO query_preview_probe VALUES (100)")
                            .await?;
                        Ok(())
                    })
                })
                .await?;
            assert_eq!(nested.len(), 1);
            assert!(db.pretending());
            assert_eq!(
                db.withoutPretending(async {
                    assert!(!db.pretending());
                    db.scalar("SELECT 12", &[]).await
                })
                .await?,
                12.into()
            );
            assert!(db.pretending());
            Ok(())
        })),
        async {
            assert!(!connection.pretending());
            connection.scalar("SELECT 7", &[]).await
        }
    );
    assert_eq!(real.unwrap(), 7.into());
    let preview = preview.unwrap();
    assert_eq!(
        preview
            .iter()
            .map(|entry| entry.query.as_str())
            .collect::<Vec<_>>(),
        [
            "INSERT INTO query_preview_probe VALUES (8)",
            "DELETE FROM query_preview_probe",
            "SELECT 16 AS n",
            "SELECT 26",
            "SELECT 12",
        ]
    );
    assert!(
        matches!(&preview[0].bindings, QueryBindings::Positional(values) if values == &[8.into()])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!connection.pretending());
    assert_eq!(
        connection
            .getQueryLog()
            .iter()
            .map(|entry| entry.query.as_str())
            .collect::<Vec<_>>(),
        ["SELECT 1", "SELECT 7", "SELECT 12"]
    );
    assert_eq!(
        connection
            .scalar("SELECT COUNT(*) FROM query_preview_probe", &[])
            .await
            .unwrap(),
        0.into()
    );

    assert!(matches!(
        connection
            .pretend(|_| Box::pin(async {
                Err(DatabaseError::InvalidConfiguration("preview interrupted"))
            }))
            .await,
        Err(DatabaseError::InvalidConfiguration("preview interrupted"))
    ));
    let mut preview = std::pin::pin!(connection.pretend(|_| Box::pin(std::future::pending())));
    assert!(futures_util::poll!(&mut preview).is_pending());
    assert!(!connection.pretending());

    let mut transaction = connection.beginTransaction().await.unwrap();
    let held = connection
        .pretend(move |_| {
            Box::pin(async move {
                transaction
                    .statement("INSERT INTO query_preview_probe VALUES (99)", &[])
                    .await?;
                assert!(
                    transaction
                        .cursor("SELECT 2", &[])
                        .try_collect::<Vec<_>>()
                        .await?
                        .is_empty()
                );
                assert!(matches!(
                    transaction.commit().await,
                    Err(DatabaseError::UnsupportedOperation(_))
                ));
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(held.len(), 2);
    connection
        .unprepared("DROP TABLE query_preview_probe")
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_query_preview_scopes_capture_to_the_current_future() {
    let root = Root::new();
    let db = manager(
        &root,
        json!({"default":"fixture","connections":{
            "fixture":{"driver":"sqlite","max_connections":1},
            "missing":{"driver":"sqlite","database":"missing.sqlite"}
        }}),
    );
    verify_query_preview(db.connection(None).unwrap()).await;
    let missing = db.connection(Some("missing")).unwrap();
    let preview = missing
        .pretend(|db| {
            Box::pin(async move {
                assert!(
                    db.select("SELECT * FROM missing_table", &[])
                        .await?
                        .is_empty()
                );
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(preview.len(), 1);
    assert!(
        missing
            .select("SELECT * FROM missing_table", &[])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn mysql_query_preview_scopes_capture_to_the_current_future() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = manager(
        &root,
        json!({"default":"fixture","connections":{
            "fixture":{"url":url,"max_connections":1}
        }}),
    );
    verify_query_preview(db.connection(None).unwrap()).await;
}

async fn verify_before_executing(connection: Connection) {
    connection
        .unprepared("CREATE TABLE query_hook_probe (n BIGINT)")
        .await
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    connection.beforeExecuting(move |sql, bindings, connection, transaction| {
        let recorded = recorded.clone();
        Box::pin(async move {
            assert_eq!(connection.getName(), "fixture");
            recorded.lock().unwrap().push(format!("first {sql}"));
            if sql == "INSERT INTO query_hook_probe VALUES (99)" {
                return Err(DatabaseError::InvalidConfiguration(
                    "query rejected by hook",
                ));
            }
            if sql == "SELECT :bound AS marker" {
                assert!(matches!(
                    bindings,
                    Bindings::Named([("bound", Value::Integer(23))])
                ));
            }
            if sql == "SELECT 41 AS marker" {
                let transaction = transaction.expect("the query runs inside a transaction");
                assert_eq!(
                    transaction
                        .scalar("SELECT n FROM query_hook_probe", &[])
                        .await?,
                    41.into()
                );
            }
            Ok(())
        })
    });
    let recorded = calls.clone();
    connection.beforeExecuting(move |sql, _, _, _| {
        let recorded = recorded.clone();
        Box::pin(async move {
            recorded.lock().unwrap().push(format!("second {sql}"));
            Ok(())
        })
    });
    let clone = connection.clone();
    connection.enableQueryLog();
    let sql = "SELECT :bound AS marker";
    let named = [("bound", 23.into())];
    let cursor = clone.cursor(sql, Bindings::Named(&named));
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(
        cursor.try_collect::<Vec<_>>().await.unwrap()[0].get_at(0),
        Some(&23.into())
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [format!("first {sql}"), format!("second {sql}")]
    );

    let rejected = "INSERT INTO query_hook_probe VALUES (99)";
    assert!(matches!(
        clone.unprepared(rejected).await,
        Err(DatabaseError::InvalidConfiguration(
            "query rejected by hook"
        ))
    ));
    assert_eq!(connection.getQueryLog().len(), 1);
    assert_eq!(calls.lock().unwrap().len(), 3);
    assert_eq!(
        clone
            .scalar("SELECT COUNT(*) FROM query_hook_probe", &[])
            .await
            .unwrap(),
        0.into()
    );

    let mut transaction = clone.beginTransaction().await.unwrap();
    assert!(transaction.unprepared(rejected).await.is_err());
    transaction
        .unprepared("INSERT INTO query_hook_probe VALUES (41)")
        .await
        .unwrap();
    let sql = "SELECT 41 AS marker";
    let rows = transaction
        .cursor(sql, &[])
        .try_collect::<Vec<_>>()
        .await
        .unwrap();
    assert_eq!(rows[0].get_at(0), Some(&41.into()));
    transaction.rollBack(None).await.unwrap();
    assert_eq!(
        clone
            .scalar("SELECT COUNT(*) FROM query_hook_probe", &[])
            .await
            .unwrap(),
        0.into()
    );
    assert!(calls.lock().unwrap().windows(4).any(|sequence| sequence
        == [
            "first SELECT 41 AS marker",
            "first SELECT n FROM query_hook_probe",
            "second SELECT n FROM query_hook_probe",
            "second SELECT 41 AS marker",
        ]));
    connection
        .unprepared("DROP TABLE query_hook_probe")
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_runs_before_executing_for_ordinary_and_transaction_queries() {
    let root = Root::new();
    let db = manager(
        &root,
        json!({"default":"fixture","connections":{
            "fixture":{"driver":"sqlite","max_connections":1}
        }}),
    );
    verify_before_executing(db.connection(None).unwrap()).await;
}

#[tokio::test]
async fn mysql_runs_before_executing_for_ordinary_and_transaction_queries() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = manager(
        &root,
        json!({"default":"fixture","connections":{
            "fixture":{"url":url,"max_connections":1}
        }}),
    );
    verify_before_executing(db.connection(None).unwrap()).await;
}

#[derive(Default)]
struct QueryRecorder {
    records: Mutex<Vec<(String, QueryRole, bool, Duration)>>,
}

impl Listener for QueryRecorder {
    fn handle<'a>(
        &'a self,
        payload: &'a dyn EventPayload,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = payload
                .downcast_ref::<QueryExecuted>()
                .ok_or(EventError::InvalidPayload)?;
            if event.sql == "SELECT :number AS n" {
                let QueryBindings::Named(values) = &event.bindings else {
                    return Err(EventError::InvalidPayload);
                };
                assert_eq!(values, &[("number".to_owned(), 11.into())]);
            }
            if event.sql == "SELECT 23 AS reentrant" {
                assert_eq!(
                    event
                        .connection
                        .scalar("SELECT 42", &[])
                        .await
                        .map_err(EventError::Transaction)?,
                    42.into()
                );
            }
            self.records.lock().unwrap().push((
                event.sql.clone(),
                event.read_write_type,
                context.transaction().is_some(),
                event.time,
            ));
            Ok(None)
        })
    }
}

async fn verify_query_observation(mut app: Application) {
    app.boot().await.unwrap();
    let manager = app.container().make::<DatabaseManager>().unwrap();
    let db = manager.connection(None).unwrap();
    let read_alias = manager.connection(Some("main::read")).unwrap();
    let records = Arc::new(QueryRecorder::default());
    let threshold = Arc::new(QueryRecorder::default());
    db.listen(records.clone()).unwrap();
    db.whenQueryingForLongerThan(Duration::ZERO, threshold.clone())
        .unwrap();

    let named = Bindings::Named(&[("number", 11.into())]);
    let measured = async {
        assert_eq!(
            db.scalar("SELECT :number AS n", named).await.unwrap(),
            11.into()
        );
        assert_eq!(
            db.scalar("SELECT 23 AS reentrant", &[]).await.unwrap(),
            23.into()
        );
    };
    tokio::time::timeout(Duration::from_secs(5), measured)
        .await
        .unwrap();
    assert_eq!(threshold.records.lock().unwrap().len(), 1);
    let before_error = db.totalQueryDuration();
    assert!(
        db.scalar("SELECT unknown_column_from_query_probe", &[])
            .await
            .is_err()
    );
    assert_eq!(db.totalQueryDuration(), before_error);

    let mut tx = db.beginTransaction().await.unwrap();
    assert_eq!(tx.scalar("SELECT 7", &[]).await.unwrap(), 7.into());
    tx.commit().await.unwrap();
    assert_eq!(
        read_alias
            .selectFromWriteConnection("SELECT 8", &[])
            .await
            .unwrap()[0]
            .get_at(0),
        Some(&8.into())
    );

    let mut cursor = db.cursor("SELECT 9", &[]);
    assert_eq!(
        cursor.try_next().await.unwrap().unwrap().get_at(0),
        Some(&9.into())
    );
    assert_eq!(records.records.lock().unwrap().len(), 5);
    assert!(cursor.try_next().await.unwrap().is_none());
    drop(cursor);

    {
        let seen = records.records.lock().unwrap();
        let actual: Vec<_> = seen
            .iter()
            .map(|(sql, role, transaction, _)| (sql.as_str(), *role, *transaction))
            .collect();
        assert_eq!(
            actual,
            [
                ("SELECT :number AS n", QueryRole::Read, false),
                ("SELECT 42", QueryRole::Read, false),
                ("SELECT 23 AS reentrant", QueryRole::Read, false),
                ("SELECT 7", QueryRole::Write, true),
                ("SELECT 8", QueryRole::Read, false),
                ("SELECT 9", QueryRole::Read, false),
            ]
        );
        let duration: Duration = seen
            .iter()
            .filter(|(sql, _, _, _)| sql != "SELECT 8")
            .map(|(_, _, _, time)| *time)
            .sum();
        assert_eq!(db.totalQueryDuration(), duration);
    }
    assert_eq!(threshold.records.lock().unwrap().len(), 1);
    db.resetTotalQueryDuration();
    assert_eq!(db.totalQueryDuration(), Duration::ZERO);
    db.allowQueryDurationHandlersToRunAgain();
    let (a, b) = tokio::join!(db.scalar("SELECT 10", &[]), db.scalar("SELECT 11", &[]));
    assert_eq!(a.unwrap(), 10.into());
    assert_eq!(b.unwrap(), 11.into());
    assert_eq!(threshold.records.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn sqlite_reports_query_bindings_pool_role_and_cumulative_duration() {
    let root = Root::new();
    let app = Application::configure(root.0.clone()).withConfig(|_, _| {
        Ok([("database".into(), json!({
            "default":"main","connections":{"main":{
                "driver":"sqlite","database":":memory:","read":{"database":":memory:"},"max_connections":1
            }}
        }))].into())
    }).create().unwrap();
    verify_query_observation(app).await;
}

#[tokio::test]
async fn mysql_reports_query_bindings_pool_role_and_cumulative_duration() {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    let root = Root::new();
    let app = Application::configure(root.0.clone()).withConfig(|_, _| {
        let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
        Ok([("database".into(), json!({
            "default":"main","connections":{"main":{"url":url,"read":{"url":url},"max_connections":1}}
        }))].into())
    }).create().unwrap();
    verify_query_observation(app).await;
}

const DIAGNOSTIC_QUERY: &str =
    "SELECT :label AS shown, ':unchanged ?' AS literal /* :ignored ? */ -- ?";

#[derive(Default)]
struct RawSqlRecorder(Mutex<Vec<(String, usize)>>);

impl Listener for RawSqlRecorder {
    fn handle<'a>(
        &'a self,
        payload: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = payload
                .downcast_ref::<QueryExecuted>()
                .ok_or(EventError::InvalidPayload)?;
            if event.sql == DIAGNOSTIC_QUERY {
                let rendered = event.toRawSql().map_err(EventError::Transaction)?;
                self.0
                    .lock()
                    .unwrap()
                    .push((rendered, event.connection.getQueryLog().len()));
            }
            Ok(None)
        })
    }
}

async fn verify_query_diagnostics(mut app: Application, mysql: bool) {
    app.boot().await.unwrap();
    let manager = app.container().make::<DatabaseManager>().unwrap();
    let db = manager.connection(None).unwrap();
    let other = manager.connection(Some("main::write")).unwrap();
    let recorder = Arc::new(RawSqlRecorder::default());
    db.listen(recorder.clone()).unwrap();
    assert!(!db.logging());
    db.select("SELECT 1", &[]).await.unwrap();
    assert!(db.getQueryLog().is_empty());

    let label = r"O'Reilly\notes";
    db.enableQueryLog();
    assert!(db.logging());
    let original = db
        .select(
            DIAGNOSTIC_QUERY,
            Bindings::Named(&[("label", label.into())]),
        )
        .await
        .unwrap();
    assert_eq!(original[0].get("shown"), Some(&label.into()));
    assert!(
        db.select("SELECT nonexistent_from_diagnostics_test", &[])
            .await
            .is_err()
    );
    other.select("SELECT 1", &[]).await.unwrap();
    assert!(other.getQueryLog().is_empty());
    let log = db.getQueryLog();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].query, DIAGNOSTIC_QUERY);
    assert_eq!(log[0].read_write_type, QueryRole::Write);
    let QueryBindings::Named(values) = &log[0].bindings else {
        panic!("expected original named bindings");
    };
    assert_eq!(values, &[("label".to_owned(), label.into())]);
    let escaped = if mysql {
        r"O''Reilly\\notes"
    } else {
        r"O''Reilly\notes"
    };
    let expected = DIAGNOSTIC_QUERY.replace(":label AS", &format!("'{escaped}' AS"));
    let rendered = db.getRawQueryLog().unwrap();
    assert_eq!(rendered[0].raw_query, expected);
    assert_eq!(rendered[0].time, log[0].time);
    assert_eq!(*recorder.0.lock().unwrap(), [(expected, 0)]);
    db.disableQueryLog();
    assert_eq!(
        db.select(&rendered[0].raw_query, &[]).await.unwrap(),
        original
    );
    assert_eq!(db.getQueryLog().len(), 1);
    db.flushQueryLog();
    assert!(db.getQueryLog().is_empty());

    db.enableQueryLog();
    let original = db
        .select(
            "SELECT ? AS data_value, ? AS nil, ? AS flag_value, ? AS exact_number",
            &[
                Value::Bytes(vec![0, 255, 16]),
                Value::Null,
                true.into(),
                19.into(),
            ],
        )
        .await
        .unwrap();
    if !mysql {
        db.select(
            "SELECT ?2 AS second, ?1 AS first, ?2 AS repeated;",
            &[17.into(), 31.into()],
        )
        .await
        .unwrap();
        db.select(
            "SELECT ?2 AS second, ? AS anonymous;",
            &[17.into(), 31.into(), 53.into()],
        )
        .await
        .unwrap();
        db.select("SELECT $2 AS second, $1 AS first;", &[17.into(), 31.into()])
            .await
            .unwrap();
    }
    db.disableQueryLog();
    let log = db.getRawQueryLog().unwrap();
    assert_eq!(
        log[0].raw_query,
        "SELECT x'00ff10' AS data_value, null AS nil, 1 AS flag_value, 19 AS exact_number"
    );
    let reparsed = db.select(&log[0].raw_query, &[]).await.unwrap();
    assert_eq!(reparsed[0].get("data_value"), original[0].get("data_value"));
    assert_eq!(reparsed[0].get("nil"), original[0].get("nil"));
    if !mysql {
        assert_eq!(
            log[1].raw_query,
            "SELECT 31 AS second, 17 AS first, 31 AS repeated;"
        );
        assert_eq!(log[2].raw_query, "SELECT 31 AS second, 31 AS anonymous;");
        assert_eq!(log[3].raw_query, "SELECT 31 AS second, 17 AS first;");
        for (statement, values, entry) in [
            (
                "SELECT ?2 AS second, ?1 AS first, ?2 AS repeated;",
                &[17.into(), 31.into()][..],
                &log[1],
            ),
            (
                "SELECT ?2 AS second, ? AS anonymous;",
                &[17.into(), 31.into(), 53.into()][..],
                &log[2],
            ),
            (
                "SELECT $2 AS second, $1 AS first;",
                &[17.into(), 31.into()][..],
                &log[3],
            ),
        ] {
            assert_eq!(
                db.select(statement, values).await.unwrap(),
                db.select(&entry.raw_query, &[]).await.unwrap(),
            );
        }
    }

    db.flushQueryLog();
    db.enableQueryLog();
    db.select("SELECT ?", &[Value::Text("embedded\0nul".into())])
        .await
        .unwrap();
    assert!(matches!(
        db.getRawQueryLog(),
        Err(DatabaseError::InvalidQuery(
            "text binding contains a null byte"
        ))
    ));
    assert_eq!(db.getQueryLog().len(), 1);
}

#[tokio::test]
async fn sqlite_renders_the_original_bindings_in_opt_in_query_diagnostics() {
    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .withConfig(|_, _| {
            Ok([("database".into(), json!({
            "default":"main","connections":{"main":{"driver":"sqlite","database":":memory:"}}
        }))].into())
        })
        .create()
        .unwrap();
    verify_query_diagnostics(app, false).await;
}

#[tokio::test]
async fn mysql_renders_the_original_bindings_in_opt_in_query_diagnostics() {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .withConfig(|_, _| {
            let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
            Ok([(
                "database".into(),
                json!({"default":"main","connections":{"main":{"url":url}}}),
            )]
            .into())
        })
        .create()
        .unwrap();
    verify_query_diagnostics(app, true).await;
}

#[tokio::test]
async fn sqlite_reconnect_reloads_pools_and_purge_replaces_the_logical_connection() {
    use openrust::contracts::config::Repository as _;

    let root = Root::new();
    let original = root.0.join("database/database.sqlite");
    let replacement = root.0.join("database/replacement.sqlite");
    fs::File::create(&replacement).unwrap();
    let repository = Arc::new(Repository::new(
        [(
            "database".into(),
            json!({
                "default":"main",
                "connections":{"main":{"driver":"sqlite","database":"database/database.sqlite"}}
            }),
        )]
        .into(),
    ));
    let manager = DatabaseManager::new(repository.clone(), &root.0);
    let connection = manager.connection(None).unwrap();
    connection.enableQueryLog();
    connection
        .unprepared("PRAGMA user_version = 17")
        .await
        .unwrap();
    let mut active = connection.beginTransaction().await.unwrap();
    active.unprepared("PRAGMA user_version = 19").await.unwrap();

    repository
        .set(
            "database.connections.main.database",
            "database/replacement.sqlite".into(),
        )
        .unwrap();
    let reconnected = manager.reconnect(None).await.unwrap();
    assert!(reconnected.logging());
    assert_eq!(connection.getDatabaseName(), original.to_string_lossy());
    let (a, b) = tokio::join!(
        connection.scalar("PRAGMA user_version", &[]),
        reconnected.scalar("PRAGMA user_version", &[]),
    );
    assert_eq!(a.unwrap(), 0.into());
    assert_eq!(b.unwrap(), 0.into());
    assert_eq!(
        active.scalar("PRAGMA user_version", &[]).await.unwrap(),
        19.into()
    );
    active.commit().await.unwrap();

    let legacy = self::manager(
        &root,
        json!({"default":"legacy","connections":{"legacy":{
            "driver":"sqlite","database":"database/database.sqlite"
        }}}),
    )
    .connection(None)
    .unwrap();
    assert_eq!(
        legacy.scalar("PRAGMA user_version", &[]).await.unwrap(),
        19.into()
    );
    assert_eq!(
        connection.getQueryLog().len(),
        reconnected.getQueryLog().len()
    );

    manager.disconnect(None).unwrap();
    assert_eq!(
        connection.scalar("PRAGMA user_version", &[]).await.unwrap(),
        0.into()
    );
    manager.purge(None).unwrap();
    let fresh = manager.connection(None).unwrap();
    assert_eq!(fresh.getDatabaseName(), replacement.to_string_lossy());
    assert!(!fresh.logging());
    assert!(fresh.getQueryLog().is_empty());
    assert!(connection.logging());
    assert_eq!(connection.scalar("SELECT 5", &[]).await.unwrap(), 5.into());
    assert!(fresh.getQueryLog().is_empty());

    manager.disconnect(Some("unknown")).unwrap();
    manager.purge(Some("unknown")).unwrap();
    assert!(matches!(
        manager.reconnect(Some("unknown")).await,
        Err(DatabaseError::UnknownConnection(_))
    ));
}

#[tokio::test]
async fn mysql_reconnect_replaces_read_write_sessions_with_an_active_transaction() {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .withConfig(|_, _| {
            let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
            Ok([("database".into(), json!({
            "default":"main",
            "connections":{
                "main":{"url":url, "read":{"url":url}, "max_connections":1, "acquire_timeout":5}
            }
        }))].into())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let observer = establishment_listener(&app, false, false);
    let manager = app.container().make::<DatabaseManager>().unwrap();
    let connection = manager.connection(None).unwrap();
    let id = "SELECT CONNECTION_ID()";
    let read_id = connection.scalar(id, &[]).await.unwrap();
    let mut old_tx = connection.beginTransaction().await.unwrap();
    let old_writer = old_tx.scalar(id, &[]).await.unwrap();
    assert_ne!(read_id, old_writer);
    assert_eq!(observer.calls.load(Ordering::SeqCst), 1);

    let mut pending = Box::pin(connection.selectFromWriteConnection(id, &[]));
    tokio::select! {
        _ = &mut pending => panic!("the original single-connection write pool must be busy"),
        () = tokio::time::sleep(Duration::from_millis(50)) => {},
    }
    manager.disconnect(None).unwrap();
    let new_writer = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(new_writer[0].get_at(0), Some(&old_writer));
    assert_eq!(old_tx.scalar(id, &[]).await.unwrap(), old_writer);
    let new_read = connection.scalar(id, &[]).await.unwrap();
    assert_ne!(new_read, read_id);
    assert_eq!(observer.calls.load(Ordering::SeqCst), 2);

    let reconnect = manager.reconnect(None).await.unwrap();
    assert_ne!(reconnect.scalar(id, &[]).await.unwrap(), new_read);
    assert_eq!(observer.calls.load(Ordering::SeqCst), 3);
    old_tx.rollBack(None).await.unwrap();
    let old_ids = [old_writer, read_id];
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let remaining = reconnect
                .scalar(
                    "SELECT count(*) FROM information_schema.processlist WHERE ID IN (?, ?)",
                    &old_ids,
                )
                .await
                .unwrap();
            if remaining == 0.into() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("retired physical MySQL sessions must close after their leases return");
    let before = connection.scalar(id, &[]).await.unwrap();
    manager.purge(None).unwrap();
    let fresh = manager.connection(None).unwrap();
    assert_ne!(fresh.scalar(id, &[]).await.unwrap(), before);
    assert_eq!(observer.calls.load(Ordering::SeqCst), 4);
}

fn record_rollback(
    tx: &mut Transaction,
    observed: &Arc<Mutex<Vec<&'static str>>>,
    label: &'static str,
) {
    let observed = observed.clone();
    tx.afterRollBack(move || {
        Box::pin(async move {
            observed.lock().unwrap().push(label);
            Ok(())
        })
    })
    .unwrap();
}

async fn verify_transaction_callbacks(db: Connection) {
    const COUNT: &str = "SELECT count(*) FROM transaction_callbacks_test";
    db.statement("DROP TABLE IF EXISTS transaction_callbacks_test", &[])
        .await
        .unwrap();
    db.statement(
        "CREATE TABLE transaction_callbacks_test (id INTEGER PRIMARY KEY)",
        &[],
    )
    .await
    .unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let starts = Arc::new(Mutex::new(Vec::new()));
    let hook_starts = starts.clone();
    db.beforeStartingTransaction(move |connection, active| {
        let starts = hook_starts.clone();
        Box::pin(async move {
            let (level, rows) = if let Some(active) = active {
                (active.transactionLevel(), active.scalar(COUNT, &[]).await?)
            } else {
                (0, connection.scalar(COUNT, &[]).await?)
            };
            starts.lock().unwrap().push((level, rows));
            Ok(())
        })
    });

    let mut tx = tokio::time::timeout(Duration::from_secs(5), db.beginTransaction())
        .await
        .unwrap()
        .unwrap();
    let released = db.clone();
    let result = Arc::new(Mutex::new(None));
    let visible = result.clone();
    tx.afterRollBack(move || {
        Box::pin(async move {
            let visible_count = released.scalar(COUNT, &[]).await?;
            *visible.lock().unwrap() = Some(visible_count);
            Ok(())
        })
    })
    .unwrap();
    record_rollback(&mut tx, &observed, "root");
    tx.insert("INSERT INTO transaction_callbacks_test VALUES (1)", &[])
        .await
        .unwrap();
    tx.beginTransaction().await.unwrap();
    record_rollback(&mut tx, &observed, "parent");
    tx.beginTransaction().await.unwrap();
    record_rollback(&mut tx, &observed, "committed-grandchild");
    tx.insert("INSERT INTO transaction_callbacks_test VALUES (2)", &[])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    tx.rollBack(Some(1)).await.unwrap();
    assert_eq!(
        *observed.lock().unwrap(),
        ["committed-grandchild", "parent"]
    );
    assert_eq!(tx.scalar(COUNT, &[]).await.unwrap(), 1.into());

    tx.beginTransaction().await.unwrap();
    record_rollback(&mut tx, &observed, "committed-child");
    tx.commit().await.unwrap();
    tx.beginTransaction().await.unwrap();
    record_rollback(&mut tx, &observed, "active-child");
    tokio::time::timeout(Duration::from_secs(5), tx.rollBack(Some(0)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        *observed.lock().unwrap(),
        [
            "committed-grandchild",
            "parent",
            "committed-child",
            "active-child",
            "root"
        ]
    );
    assert_eq!(*result.lock().unwrap(), Some(0.into()));
    assert_eq!(
        *starts.lock().unwrap(),
        [
            (0, 0.into()),
            (1, 1.into()),
            (2, 1.into()),
            (1, 1.into()),
            (1, 1.into())
        ]
    );
    assert!(matches!(
        tx.afterRollBack(|| Box::pin(async { Ok(()) })),
        Err(DatabaseError::TransactionClosed)
    ));

    let mut committed = db.beginTransaction().await.unwrap();
    record_rollback(&mut committed, &observed, "discarded-on-commit");
    committed.commit().await.unwrap();
    assert_eq!(observed.lock().unwrap().len(), 5);

    let mut failed = db.beginTransaction().await.unwrap();
    failed
        .insert("INSERT INTO transaction_callbacks_test VALUES (3)", &[])
        .await
        .unwrap();
    failed
        .afterRollBack(|| Box::pin(async { Err(DatabaseError::InvalidQuery("rollback probe")) }))
        .unwrap();
    assert!(matches!(
        failed.rollBack(None).await,
        Err(DatabaseError::AfterRollback { .. })
    ));
    assert_eq!(failed.transactionLevel(), 0);
    assert_eq!(db.scalar(COUNT, &[]).await.unwrap(), 0.into());

    db.beforeStartingTransaction(|_, _| {
        Box::pin(async { Err(DatabaseError::InvalidQuery("blocked begin")) })
    });
    assert!(matches!(
        db.beginTransaction().await,
        Err(DatabaseError::InvalidQuery("blocked begin"))
    ));
    assert_eq!(db.scalar(COUNT, &[]).await.unwrap(), 0.into());
    db.statement("DROP TABLE transaction_callbacks_test", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_transaction_callbacks_observe_savepoint_ancestry() {
    let root = Root::new();
    let manager = manager(
        &root,
        json!({"default":"main","connections":{"main":{
            "driver":"sqlite", "database":":memory:"
        }}}),
    );
    verify_transaction_callbacks(manager.connection(None).unwrap()).await;
}

#[tokio::test]
async fn mysql_transaction_callbacks_observe_savepoint_ancestry() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let manager = manager(
        &root,
        json!({"default":"main","connections":{"main":{
            "url":url, "max_connections":1
        }}}),
    );
    verify_transaction_callbacks(manager.connection(None).unwrap()).await;
}

#[tokio::test]
async fn sqlite_schema_rollback_callbacks_observe_restored_session_settings() {
    struct ObserveSchema(Arc<Mutex<Vec<(Value, Value)>>>);

    impl Listener for ObserveSchema {
        fn handle<'a>(
            &'a self,
            payload: &'a dyn EventPayload,
            context: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let event = payload
                    .downcast_ref::<TransactionBeginning>()
                    .ok_or(EventError::InvalidPayload)?;
                let transaction = context.transaction().ok_or(EventError::InvalidPayload)?;
                let connection = event.connection.clone();
                let observed = self.0.clone();
                transaction
                    .afterRollBack(move || {
                        Box::pin(async move {
                            let foreign_keys =
                                connection.scalar("PRAGMA foreign_keys", &[]).await?;
                            let legacy =
                                connection.scalar("PRAGMA legacy_alter_table", &[]).await?;
                            observed.lock().unwrap().push((foreign_keys, legacy));
                            Ok(())
                        })
                    })
                    .map_err(EventError::Transaction)?;
                Ok(None)
            })
        }
    }

    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .withConfig(|_, _| {
            Ok([("database".into(), json!({
            "default":"fixture","connections":{"fixture":{"driver":"sqlite","database":":memory:"}}
        }))].into())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().make::<DatabaseManager>().unwrap();
    let db = manager.connection(None).unwrap();
    db.unprepared(
        "CREATE TABLE callback_schema (name TEXT); INSERT INTO callback_schema VALUES (NULL)",
    )
    .await
    .unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = app.container().make::<Dispatcher>().unwrap();
    dispatcher
        .listen(
            TransactionBeginning::NAME,
            Arc::new(ObserveSchema(observed.clone())),
            Delivery::Sync,
        )
        .unwrap();

    let mut schema = db.getSchemaBuilder();
    let failed = tokio::time::timeout(
        Duration::from_secs(5),
        schema.table("callback_schema", |table| {
            table.text("name").change();
        }),
    )
    .await
    .expect("the callback must not wait on the SQLite schema transaction's lease");
    assert!(matches!(failed, Err(DatabaseError::Driver { .. })));
    assert_eq!(*observed.lock().unwrap(), [(1.into(), 0.into())]);
    assert_eq!(
        db.scalar("SELECT count(*) FROM callback_schema", &[])
            .await
            .unwrap(),
        1.into()
    );
}

async fn terminate_running_query(control: &Connection, marker: &'static str) {
    let pattern = format!("%{marker}%");
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let id = control.scalar(
                "SELECT ID FROM information_schema.processlist WHERE ID <> CONNECTION_ID() AND COMMAND IN ('Execute', 'Query') AND INFO LIKE ? LIMIT 1",
                &[pattern.clone().into()],
            ).await.unwrap();
            match id {
                Value::Integer(id) => break id.to_string(),
                Value::Unsigned(id) => break id.to_string(),
                Value::Null => tokio::time::sleep(Duration::from_millis(20)).await,
                _ => panic!("expected a numeric MySQL session id"),
            }
        }
    }).await.expect("the fixture SQL must be executing before its session is terminated");
    control
        .unprepared(&format!("KILL CONNECTION {id}"))
        .await
        .unwrap();
}

#[tokio::test]
async fn mysql_replays_running_reads_and_writes_once_and_preserves_transaction_boundaries() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let control = manager(
        &root,
        json!({"default":"control","connections":{"control":{"url":url,"max_connections":1}}}),
    )
    .connection(None)
    .unwrap();
    let mut app =
        Application::configure(root.0.clone())
            .withConfig(|_, _| {
                let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
                Ok([("database".into(), json!({"default":"main","connections":{
            "main":{"url":url,"read":{"url":url},"max_connections":1,"acquire_timeout":5},
                "parallel":{"url":url,"max_connections":2,"acquire_timeout":5}
        }}))].into())
            })
            .create()
            .unwrap();
    app.boot().await.unwrap();
    let listener = establishment_listener(&app, false, false);
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    db.enableQueryLog();
    db.unprepared("DROP TABLE IF EXISTS sql_replay_probe; CREATE TABLE sql_replay_probe (id INTEGER PRIMARY KEY, n INTEGER NOT NULL); INSERT INTO sql_replay_probe VALUES (1, 0)").await.unwrap();

    let read_marker = "fixture_replay_running_read";
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    db.beforeExecuting(move |sql, _, _, _| {
        let count = count.clone();
        Box::pin(async move {
            if sql.contains(read_marker) {
                count.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        })
    });
    let read_id = [db.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap()];
    let sql = "SELECT IF(CONNECTION_ID() = ?, SLEEP(20), 17) /* fixture_replay_running_read */";
    let (result, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            db.scalar(sql, &read_id),
            terminate_running_query(&control, read_marker)
        )
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap(), 17.into());
    assert_eq!(listener.calls.load(Ordering::SeqCst), 2);
    let log = db.getQueryLog();
    let replayed: Vec<_> = log.iter().filter(|entry| entry.query == sql).collect();
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].read_write_type, QueryRole::Read);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);

    let write_id = db
        .selectFromWriteConnection("SELECT CONNECTION_ID()", &[])
        .await
        .unwrap();
    let write_ids = [write_id[0].get_at(0).unwrap().clone()];
    let sql = "UPDATE sql_replay_probe SET n = n + 1 WHERE id = IF(CONNECTION_ID() = ?, SLEEP(20), 1) /* fixture_replay_running_write */";
    let (changed, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            db.update(sql, &write_ids),
            terminate_running_query(&control, "fixture_replay_running_write")
        )
    })
    .await
    .unwrap();
    assert_eq!(changed.unwrap(), 1);
    assert_eq!(
        db.scalar("SELECT n FROM sql_replay_probe WHERE id = 1", &[])
            .await
            .unwrap(),
        1.into()
    );
    assert_eq!(listener.calls.load(Ordering::SeqCst), 3);

    let mut tx = db.beginTransaction().await.unwrap();
    tx.update("UPDATE sql_replay_probe SET n = 99 WHERE id = 1", &[])
        .await
        .unwrap();
    let (transaction, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            tx.scalar("SELECT SLEEP(20) /* fixture_no_transaction_replay */", &[]),
            terminate_running_query(&control, "fixture_no_transaction_replay")
        )
    })
    .await
    .unwrap();
    assert!(matches!(transaction, Err(DatabaseError::Driver { .. })));
    assert!(matches!(
        tx.commit().await,
        Err(DatabaseError::TransactionInterrupted)
    ));
    drop(tx);
    assert_eq!(listener.calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        db.scalar("SELECT n FROM sql_replay_probe WHERE id = 1", &[])
            .await
            .unwrap(),
        1.into()
    );

    let (failed, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            db.scalar("SELECT SLEEP(20) /* fixture_replay_fails_twice */", &[]),
            async {
                for _ in 0..2 {
                    terminate_running_query(&control, "fixture_replay_fails_twice").await;
                }
            }
        )
    })
    .await
    .unwrap();
    assert!(matches!(failed, Err(DatabaseError::Driver { .. })));
    assert_eq!(listener.calls.load(Ordering::SeqCst), 4);
    assert!(
        db.getQueryLog()
            .iter()
            .all(|entry| !entry.query.contains("fixture_replay_fails_twice"))
    );
    let parallel = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(Some("parallel"))
        .unwrap();
    let held = "SELECT IF(SLEEP(0.05) = 0, CONNECTION_ID(), 0)";
    let (one, two) = tokio::join!(parallel.scalar(held, &[]), parallel.scalar(held, &[]));
    let sessions = [one.unwrap(), two.unwrap()];
    assert_ne!(sessions[0], sessions[1]);
    let before_parallel = listener.calls.load(Ordering::SeqCst);
    let (a, b, (), ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            parallel.scalar("SELECT IF(CONNECTION_ID() IN (?, ?), SLEEP(20), 12) /* fixture_parallel_replay_a */", &sessions),
            parallel.scalar("SELECT IF(CONNECTION_ID() IN (?, ?), SLEEP(20), 13) /* fixture_parallel_replay_b */", &sessions),
            terminate_running_query(&control, "fixture_parallel_replay_a"),
            terminate_running_query(&control, "fixture_parallel_replay_b"),
        )
    }).await.unwrap();
    assert_eq!(a.unwrap(), 12.into());
    assert_eq!(b.unwrap(), 13.into());
    assert_eq!(listener.calls.load(Ordering::SeqCst), before_parallel + 1);
    db.unprepared("DROP TABLE sql_replay_probe").await.unwrap();
}

#[tokio::test]
async fn mysql_cursor_only_replays_before_delivering_a_row() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let manager = manager(
        &root,
        json!({"default":"fixture","connections":{
            "fixture":{"url":url,"max_connections":1,"acquire_timeout":5},
            "control":{"url":url,"max_connections":1}
        }}),
    );
    let control = manager.connection(Some("control")).unwrap();
    let db = manager.connection(None).unwrap();
    db.enableQueryLog();
    let id = [db.scalar("SELECT CONNECTION_ID()", &[]).await.unwrap()];
    let sql =
        "SELECT IF(CONNECTION_ID() = ?, SLEEP(20), 29) /* fixture_cursor_replay_before_row */";
    let (rows, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            db.cursor(sql, &id).try_collect::<Vec<_>>(),
            terminate_running_query(&control, "fixture_cursor_replay_before_row")
        )
    })
    .await
    .unwrap();
    assert_eq!(rows.unwrap()[0].get_at(0), Some(&29.into()));
    assert_eq!(
        db.getQueryLog()
            .iter()
            .filter(|entry| entry.query == sql)
            .count(),
        1
    );

    let sql = "SELECT REPEAT('a', 262144) AS payload UNION ALL SELECT CONCAT('b', SLEEP(20)) /* fixture_cursor_no_replay_after_row */";
    let mut cursor = db.cursor(sql, &[]);
    let first = tokio::time::timeout(Duration::from_secs(5), cursor.try_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(first.get_at(0), Some(Value::Text(text)) if text.len() == 262144));
    terminate_running_query(&control, "fixture_cursor_no_replay_after_row").await;
    let failed = tokio::time::timeout(Duration::from_secs(5), cursor.try_next())
        .await
        .unwrap();
    assert!(matches!(failed, Err(DatabaseError::Driver { .. })));
    drop(cursor);
    assert!(db.getQueryLog().iter().all(|entry| entry.query != sql));
    assert_eq!(db.scalar("SELECT 1", &[]).await.unwrap(), 1.into());
}
