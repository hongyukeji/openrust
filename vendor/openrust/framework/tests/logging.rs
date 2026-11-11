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
    config::{ConfigError, Environment, Repository, Value},
    console::ConsoleError,
    contracts::{config::Repository as _, container::Container as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::{LogContext, LogManager},
    routing::RoutingError,
    support::facades::{Artisan, Log, Route},
};
use serde_json::json;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-log-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join(path)).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn manager(root: &Root, logging: Value) -> (LogManager, Arc<Repository>) {
    let config = Arc::new(Repository::new(BTreeMap::from([
        ("app".into(), json!({"env":"testing"})),
        ("logging".into(), logging),
    ])));
    (LogManager::new(config.clone(), &root.0), config)
}

#[test]
fn resolves_cached_stacks_with_severity_bubbling_and_fresh_driver_settings() {
    let root = Root::new();
    let (manager, config) = manager(
        &root,
        json!({
            "default":"stack",
            "channels": {
                "stack":{"driver":"stack", "name":"worker", "channels":"first, second"},
                "first":{"driver":"single", "path":"first.log", "level":"error", "bubble":false, "locking":true, "permission":0o600, "replace_placeholders":true},
                "second":{"driver":"single", "path":"second.log"},
                "quiet":{"driver":"null"},
            }
        }),
    );
    let stack = manager.channel(None).unwrap();
    assert_eq!(stack.name(), "worker");
    assert!(Arc::ptr_eq(&stack, &manager.driver(Some("stack")).unwrap()));
    let context = LogContext::from_iter([("value".into(), json!("first\nsecond\u{2028}"))]);
    stack.info("received {value}", &context).unwrap();
    assert!(!root.0.join("first.log").exists());
    assert!(
        root.read("second.log")
            .contains("worker.INFO: received first\\nsecond\\u{2028}")
    );
    assert_eq!(root.read("second.log").lines().count(), 1);
    stack
        .error("error stays in first", &LogContext::new())
        .unwrap();
    assert!(
        root.read("first.log")
            .contains("worker.ERROR: error stays in first")
    );
    assert!(!root.read("second.log").contains("error stays"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(root.0.join("first.log"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    manager
        .stack(&["quiet", "second"], Some("muted"))
        .unwrap()
        .info("discard", &LogContext::new())
        .unwrap();
    assert!(!root.read("second.log").contains("discard"));
    manager.setDefaultDriver("second").unwrap();
    assert_eq!(manager.getDefaultDriver().as_deref(), Some("second"));
    let old = manager.channel(None).unwrap();
    config
        .set("logging.channels.second.path", json!("new.log"))
        .unwrap();
    manager.forgetChannel(None);
    manager.warning("new target", &LogContext::new()).unwrap();
    old.warning("old target", &LogContext::new()).unwrap();
    assert!(root.read("new.log").contains("new target"));
    assert!(root.read("second.log").contains("old target"));
    config
        .set("logging.channels.null", json!({"driver":"null"}))
        .unwrap();
    config.set("logging.default", Value::Null).unwrap();
    assert_eq!(manager.getDefaultDriver(), None);
    manager
        .info("test null default", &LogContext::new())
        .unwrap();
    assert!(!root.0.join("storage/logs/openrust.log").exists());
}

#[test]
fn emergency_recovers_resolution_and_file_errors_and_ignored_failures_continue() {
    let root = Root::new();
    fs::write(root.0.join("blocked"), "not a directory").unwrap();
    let (manager, config) = manager(
        &root,
        json!({
            "default":"missing",
            "channels":{
                "emergency":{"path":"emergency.log"},
                "blocked":{"driver":"single","path":"blocked/file.log"},
                "later":{"driver":"single","path":"later.log"},
                "strict":{"driver":"stack","channels":["blocked","later"]},
            "ignored":{"driver":"stack","channels":["strict"],"ignore_exceptions":true},
                "cycle":{"driver":"stack","channels":["cycle"]},
            "invalid":{"driver":"stderr","formatter":"unsupported"}
            }
        }),
    );
    manager
        .info("configuration fallback", &LogContext::new())
        .unwrap();
    let blocked = manager.channel(Some("blocked")).unwrap();
    blocked.info("write fallback", &LogContext::new()).unwrap();
    manager
        .channel(Some("ignored"))
        .unwrap()
        .info("continued", &LogContext::new())
        .unwrap();
    manager
        .channel(Some("cycle"))
        .unwrap()
        .warning("cycle fallback", &LogContext::new())
        .unwrap();
    manager
        .channel(Some("invalid"))
        .unwrap()
        .warning("formatter fallback", &LogContext::new())
        .unwrap();
    let emergency = root.read("emergency.log");
    for marker in [
        "Unable to create configured logger",
        "configuration fallback",
        "Unable to write configured logger",
        "write fallback",
        "cyclic or excessively deep log stack",
        "cycle fallback",
        "formatter fallback",
    ] {
        assert!(emergency.contains(marker), "{marker}");
    }
    assert!(!emergency.contains(root.0.to_str().unwrap()));
    assert!(!emergency.contains("continued"));
    assert!(root.read("later.log").contains("continued"));
    fs::remove_file(root.0.join("blocked")).unwrap();
    blocked
        .info("retry original sink", &LogContext::new())
        .unwrap();
    assert!(
        root.read("blocked/file.log")
            .contains("retry original sink")
    );
    config
        .set(
            "logging.channels.missing",
            json!({"driver":"single","path":"recovered.log"}),
        )
        .unwrap();
    manager
        .info("retry missing channel", &LogContext::new())
        .unwrap();
    assert!(root.read("recovered.log").contains("retry missing channel"));
}

fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([(
        "app".into(),
        json!({"env":"test","host":"127.0.0.1","port":0}),
    )]))
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/log/{id}", record)
}

fn record(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let context = LogContext::from_iter([("id".into(), json!(request.route("id")))]);
        Log::info("request {id}", &context).map_err(|source| HttpError::Internal {
            source: source.into(),
        })?;
        let container = openrust::container::Container::getInstance();
        let manager = container.make::<LogManager>().unwrap();
        let logger = container
            .make::<dyn openrust::contracts::log::Logger>()
            .unwrap();
        let shared: Arc<dyn openrust::contracts::log::Logger> = manager;
        assert!(Arc::ptr_eq(&logger, &shared));
        Ok(Response::new(StatusCode::OK, Body::from("logged")))
    })
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

async fn server(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let application = Application::configure(root.0.clone())
        .withConfig(config)
        .withRouting(Routing::default().web(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

async fn send(address: std::net::SocketAddr, id: u32) {
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(
            stream,
            "GET /log/{id} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.ends_with("logged"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn real_http_logs_concurrent_requests_to_their_own_application() {
    let first = Root::new();
    let second = Root::new();
    let (a, first_task) = server(&first).await;
    let (b, second_task) = server(&second).await;
    tokio::join!(send(a, 101), send(a, 102), send(b, 201));
    let first_log = first.read("storage/logs/openrust.log");
    let second_log = second.read("storage/logs/openrust.log");
    assert_eq!(first_log.lines().count(), 2);
    assert_eq!(second_log.lines().count(), 1);
    for id in [101, 102] {
        assert!(first_log.contains(&format!("test.INFO: request {id}")));
    }
    assert!(second_log.contains("test.INFO: request 201"));
    for task in [first_task, second_task] {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

fn console_routes() -> Result<(), ConsoleError> {
    Artisan::command("log:probe", |_| {
        Log::warning("artisan wrote a file", &LogContext::new()).unwrap();
        Log::channel(Some("stderr"))
            .unwrap()
            .notice("artisan stderr probe", &LogContext::new())
            .unwrap();
        openrust::container::Container::getInstance()
            .get::<dyn openrust::contracts::config::Repository>("config")
            .unwrap()
            .set(
                "logging.channels.emergency.path",
                json!("storage/logs/openrust.log/emergency"),
            )
            .unwrap();
        Log::channel(Some("absent"))
            .unwrap()
            .warning("emergency stderr probe", &LogContext::new())
            .unwrap();
    })?
    .purpose("Write a log test record")
}

#[tokio::test]
async fn artisan_logs_to_file_and_stderr() {
    const CHILD_ROOT: &str = "OPENRUST_TEST_LOG_CHILD_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let application = Application::configure(PathBuf::from(root))
            .withConfig(config)
            .withRouting(Routing::default().commands(console_routes))
            .create()
            .unwrap();
        assert_eq!(
            application
                .handleCommand(&["log:probe".into()])
                .await
                .unwrap(),
            0
        );
        return;
    }
    let root = Root::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "artisan_logs_to_file_and_stderr", "--nocapture"])
        .env(CHILD_ROOT, &root.0)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}\n{stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(stderr.contains("test.NOTICE: artisan stderr probe"));
    assert!(stderr.contains("openrust.EMERGENCY: Unable to create configured logger"));
    assert!(stderr.contains("openrust.WARNING: emergency stderr probe"));
    assert!(
        root.read("storage/logs/openrust.log")
            .contains("test.WARNING: artisan wrote a file")
    );
}

#[test]
fn simultaneous_threads_share_a_channel_and_append_complete_records() {
    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({
            "default":"single",
            "channels":{"single":{"driver":"single","path":"threads.log","locking":true}}
        }),
    );
    let barrier = std::sync::Barrier::new(6);
    let channels = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..6)
            .map(|index| {
                let manager = &manager;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let channel = manager.channel(None).unwrap();
                    channel
                        .info(&format!("thread {index}"), &LogContext::new())
                        .unwrap();
                    channel
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(
        channels
            .iter()
            .all(|channel| Arc::ptr_eq(&channels[0], channel))
    );
    let records = root.read("threads.log");
    assert_eq!(records.lines().count(), 6);
    for index in 0..6 {
        assert!(
            records
                .lines()
                .any(|line| line.ends_with(&format!("testing.INFO: thread {index}")))
        );
    }
}
