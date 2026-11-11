use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::Repository,
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

fn routes(_: &Application) -> Result<Router, RoutingError> {
    fn query(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let value = DB::scalar("SELECT id FROM probe", &[])
                .await
                .map_err(|source| HttpError::Internal {
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
    conn.statement("CREATE TABLE probe (id INTEGER)", &[])
        .await
        .unwrap();
    conn.insert("INSERT INTO probe VALUES (?)", &[id.into()])
        .await
        .unwrap();
}

async fn server(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root.0.clone())
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
            let value = DB::scalar("SELECT id FROM probe", &[])
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
