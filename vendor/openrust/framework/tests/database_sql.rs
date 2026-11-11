use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::{ConfigError, Environment, Repository},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::http::Kernel as _,
    database::{DatabaseError, DatabaseManager, Value},
    foundation::{Application, configuration::Routing, http::Kernel},
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
                "unsupported":{"driver":"pgsql"}
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
    DB::transaction(
        |transaction| Box::pin(transaction.table("probe").value("id")),
        1,
    )
    .await
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
            query.value("id").await
        }
        let id = read(write, force_writer)
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        let Value::Integer(id) = id else {
            panic!("expected integer")
        };
        Ok(Response::new(StatusCode::OK, Body::from(id.to_string())))
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    fn query(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let value = probe_value().await.map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
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

async fn server(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root.0.clone())
        .withConfig(config)
        .withRouting(Routing::new(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
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
        let app = Application::configure(root.into())
            .withCommands(commands)
            .create()
            .unwrap();
        assert_eq!(app.handleCommand(&["db:probe".into()]).await.unwrap(), 0);
        return;
    }
    let first = Root::new();
    let second = Root::new();
    tokio::join!(prepare(&first, 17), prepare(&second, 42));
    let ((a, x), (b, y)) = tokio::join!(server(&first), server(&second));
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
    let writer = db.connection(None).unwrap();
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
    writer
        .transaction(
            |tx| {
                let release = release.clone();
                let attempts = attempts.clone();
                Box::pin(async move {
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
                     "max_connections":1,"acquire_timeout":2},
            "control":{"url":url,"max_connections":1,"acquire_timeout":2}
        }}),
    );
    let split = db.connection(None).unwrap();
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

    split
        .statement("CREATE TABLE IF NOT EXISTS rust_split_probe (id INT)", &[])
        .await
        .unwrap();
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
            Box::pin(async move {
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
    assert_eq!(
        writer
            .scalar("SELECT SUM(n) FROM rust_deadlock WHERE id IN (1, 2)", &[])
            .await
            .unwrap(),
        "2".into()
    );
}
