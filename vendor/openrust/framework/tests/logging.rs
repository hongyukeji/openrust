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
        Log::build(&json!({"driver":"daily", "path":"http.log"}))
            .and_then(|logger| logger.info("daily HTTP", &context))
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
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
    let date = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let daily = format!("http-{}.log", date.format("%Y-%m-%d"));
    assert_eq!(first.read(&daily).lines().count(), 2);
    assert_eq!(second.read(&daily).lines().count(), 1);
    for task in [first_task, second_task] {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

fn console_routes() -> Result<(), ConsoleError> {
    Artisan::command("log:probe", |_| {
        Log::warning("artisan wrote a file", &LogContext::new()).unwrap();
        let manager = openrust::container::Container::getInstance()
            .make::<LogManager>()
            .unwrap();
        manager.registerFactory("console", |manager, _| {
            Ok(manager.build(&json!({"driver":"monthly","path":"console.log"}))?)
        });
        Log::build(&json!({"driver":"custom","via":"console"}))
            .unwrap()
            .notice("monthly Artisan", &LogContext::new())
            .unwrap();
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
    let date = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    assert!(
        root.read(&format!("console-{}.log", date.format("%Y-%m")))
            .contains("monthly Artisan")
    );
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

#[test]
fn daily_and_monthly_channels_prune_their_own_periods() {
    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({
            "default":"daily",
            "channels": {
                "daily":{"driver":"daily","path":"shared.log","level":"warning","days":1},
                "monthly":{"driver":"monthly","path":"shared.log","max_files":1}
            }
        }),
    );
    for name in [
        "shared-2000-01-01.log",
        "shared-2000-01.log",
        "shared-note.log",
    ] {
        fs::write(root.0.join(name), "old").unwrap();
    }
    let context = LogContext::new();
    manager.info("filtered", &context).unwrap();
    assert!(root.0.join("shared-2000-01-01.log").exists());
    manager.warning("today", &context).unwrap();
    assert!(!root.0.join("shared-2000-01-01.log").exists());
    assert!(root.0.join("shared-2000-01.log").exists());
    manager
        .channel(Some("monthly"))
        .unwrap()
        .info("this month", &context)
        .unwrap();
    assert!(!root.0.join("shared-2000-01.log").exists());
    assert!(root.0.join("shared-note.log").exists());
    let date = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    assert!(
        root.read(&format!("shared-{}.log", date.format("%Y-%m-%d")))
            .contains("today")
    );
    assert!(
        root.read(&format!("shared-{}.log", date.format("%Y-%m")))
            .contains("this month")
    );
}

#[test]
fn registered_factories_are_lazy_and_mix_with_on_demand_channels() {
    use openrust::log::{LogError, StackChannel};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({
            "default":"audit",
            "channels": {
                "audit":{"driver":"custom","via":"audit-factory","target":"audit.log"},
                "named":{"driver":"application","target":"named.log","level":"warning"},
                "emergency":{"path":"emergency.log"}
            }
        }),
    );
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    manager.registerFactory("audit-factory", move |manager, config| {
        if hits.fetch_add(1, Ordering::Relaxed) == 0 {
            return Err(LogError::UnknownChannel("private-factory-error".into()));
        }
        Ok(manager.build(&json!({"driver":"single","path": config["target"]}))?)
    });
    manager.extend("application", |manager, config| {
        Ok(manager.build(&json!({"driver":"single","path":config["target"]}))?)
    });
    assert_eq!(count.load(Ordering::Relaxed), 0);
    manager
        .info("first factory error", &LogContext::new())
        .unwrap();
    let emergency = root.read("emergency.log");
    assert!(emergency.contains("custom log channel factory failed"));
    assert!(!emergency.contains("private-factory-error"));
    let audit = manager.channel(None).unwrap();
    assert!(Arc::ptr_eq(&audit, &manager.channel(None).unwrap()));
    let temp_config = json!({"driver":"single","path":"temporary.log"});
    let temporary = manager.build(&temp_config).unwrap();
    assert!(!Arc::ptr_eq(
        &temporary,
        &manager.build(&temp_config).unwrap()
    ));
    let combined = manager
        .stackChannels(
            &[
                StackChannel::Named("audit"),
                StackChannel::Logger(&temporary),
                StackChannel::Named("named"),
            ],
            Some("combined"),
        )
        .unwrap();
    combined.info("mixed record", &LogContext::new()).unwrap();
    assert!(root.read("audit.log").contains("mixed record"));
    assert!(
        root.read("temporary.log")
            .contains("combined.INFO: mixed record")
    );
    assert!(!root.0.join("named.log").exists());
    combined
        .warning("high severity", &LogContext::new())
        .unwrap();
    assert!(root.read("named.log").contains("high severity"));
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[test]
fn forgetting_a_channel_during_factory_execution_prevents_stale_publication() {
    let root = Root::new();
    let (manager, config) = manager(
        &root,
        json!({
            "default":"selected",
            "channels":{"selected":{"driver":"extension","target":"old.log"}}
        }),
    );
    let started = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let factory_started = started.clone();
    let factory_release = release.clone();
    manager.extend("extension", move |manager, config| {
        if config["target"] == "old.log" {
            factory_started.wait();
            factory_release.wait();
        }
        Ok(manager.build(&json!({"driver":"single","path":config["target"]}))?)
    });
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| manager.channel(None).unwrap());
        started.wait();
        config
            .set("logging.channels.selected.target", json!("new.log"))
            .unwrap();
        manager.forgetChannel(None);
        let current = manager.channel(None).unwrap();
        release.wait();
        let old = pending.join().unwrap();
        assert!(!Arc::ptr_eq(&current, &old));
        assert!(Arc::ptr_eq(&current, &manager.channel(None).unwrap()));
        current.info("current", &LogContext::new()).unwrap();
        old.info("in flight", &LogContext::new()).unwrap();
    });
    assert!(root.read("new.log").contains("current"));
    assert!(root.read("old.log").contains("in flight"));
}
