use std::{
    collections::BTreeMap,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use openrust::{
    config::{ConfigError, Environment, Repository, Value},
    console::ConsoleError,
    contracts::foundation::Application as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::{Level, LogContext, LogError, LogManager, events::MessageLogged},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Log, Route},
    },
};
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-log-listeners-{:032x}",
            fastrand::u128(..)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn manager(&self) -> Arc<LogManager> {
        Arc::new(LogManager::new(
            Arc::new(Repository::new(settings())),
            &self.0,
        ))
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct External(PathBuf);

impl openrust::contracts::log::Logger for External {
    fn log(&self, _: Level, message: &str, _: &LogContext) -> openrust::log::LogResult<()> {
        fs::write(&self.0, message).map_err(|error| LogError::WriteFailed(Box::new(error)))
    }
}

fn configuration(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(settings())
}

fn settings() -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            "app".into(),
            json!({"env":"testing", "host":"127.0.0.1", "port":0}),
        ),
        (
            "logging".into(),
            json!({"default":"file", "channels":{
                "file":{"driver":"single", "path":"observed.log", "replace_placeholders":true},
                "other":{"driver":"single", "path":"other.log", "level":"warning"},
                "null":{"driver":"null", "level":"error"},
                "broken":{"driver":"single", "path":"occupied/log"},
                "emergency":{"path":"emergency.log"}
            }}),
        ),
    ])
}

#[test]
fn accepted_channels_notify_once_after_writing_and_keep_the_original_payload() {
    let root = Root::new();
    let manager = root.manager();
    let cached = manager.channel(Some("file")).unwrap();
    let records = Arc::new(Mutex::new(Vec::new()));
    let capture = records.clone();
    let path = root.0.join("observed.log");
    manager
        .listen(move |event| {
            if event.message == "Order {id}" {
                assert!(fs::read_to_string(&path).unwrap().contains("Order 42"));
            }
            capture.lock().unwrap().push(event.clone());
            Ok(())
        })
        .unwrap();
    cached
        .info(
            "Order {id}",
            &LogContext::from_iter([("id".into(), json!(42))]),
        )
        .unwrap();
    manager
        .channel(Some("other"))
        .unwrap()
        .info("below threshold", &LogContext::new())
        .unwrap();
    manager
        .channel(Some("null"))
        .unwrap()
        .debug("also below", &LogContext::new())
        .unwrap();
    manager
        .channel(Some("null"))
        .unwrap()
        .error("discarded by null", &LogContext::new())
        .unwrap();
    manager
        .stack(&["file", "other"], None)
        .unwrap()
        .warning("stack once", &LogContext::new())
        .unwrap();
    cached
        .withName("renamed")
        .notice("renamed", &LogContext::new())
        .unwrap();
    manager
        .build(&json!({"driver":"single", "path":"temporary.log"}))
        .unwrap()
        .info("temporary", &LogContext::new())
        .unwrap();
    let path = root.0.join("external.log");
    manager.extend("external", move |_, _| Ok(Arc::new(External(path.clone()))));
    manager
        .build(&json!({"driver":"external"}))
        .unwrap()
        .info("external", &LogContext::new())
        .unwrap();
    manager.registerProcessor("rewrite", |_| {
        Ok(Arc::new(|record| {
            record.message = "processed".into();
            record.context.to_mut().insert("id".into(), json!(99));
            Ok(())
        }))
    });
    manager
        .build(&json!({"driver":"single", "path":"processed.log", "processors":["rewrite"]}))
        .unwrap()
        .info(
            "original",
            &LogContext::from_iter([("id".into(), json!(9))]),
        )
        .unwrap();
    manager.forgetChannel(Some("file"));
    manager.info("recreated", &LogContext::new()).unwrap();
    fs::write(root.0.join("occupied"), "file").unwrap();
    manager
        .channel(Some("broken"))
        .unwrap()
        .error("fallback", &LogContext::new())
        .unwrap();
    manager
        .channel(Some("missing"))
        .unwrap()
        .info("unresolved fallback", &LogContext::new())
        .unwrap();
    let events = records.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.message.as_str())
            .collect::<Vec<_>>(),
        [
            "Order {id}",
            "discarded by null",
            "stack once",
            "renamed",
            "temporary",
            "external",
            "original",
            "recreated",
            "Unable to write configured logger. Using emergency logger.",
            "fallback",
            "Unable to create configured logger. Using emergency logger.",
            "unresolved fallback"
        ]
    );
    assert_eq!(events[0].context["id"], 42);
    assert_eq!(events[0].level, Level::Info);
    assert_eq!(events[6].context["id"], 9);
    assert_eq!(
        fs::read_to_string(root.0.join("external.log")).unwrap(),
        "external"
    );
    assert!(
        fs::read_to_string(root.0.join("processed.log"))
            .unwrap()
            .contains("processed")
    );
    assert!(
        fs::read_to_string(root.0.join("emergency.log"))
            .unwrap()
            .contains("fallback")
    );
}

#[test]
fn listeners_can_reenter_and_register_without_holding_the_registry_lock() {
    let root = Root::new();
    let manager = root.manager();
    let weak = Arc::downgrade(&manager);
    let nested = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capture = seen.clone();
    manager
        .listen(move |event| {
            capture
                .lock()
                .unwrap()
                .push(format!("first {}", event.message));
            if !nested.swap(true, Ordering::SeqCst) {
                let capture = capture.clone();
                let manager = weak.upgrade().unwrap();
                manager.listen(move |event| {
                    capture
                        .lock()
                        .unwrap()
                        .push(format!("second {}", event.message));
                    Ok(())
                })?;
                manager.info("nested", &LogContext::new())?;
            }
            Ok(())
        })
        .unwrap();
    manager.info("outer", &LogContext::new()).unwrap();
    manager.info("later", &LogContext::new()).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "first outer",
            "first nested",
            "second nested",
            "first later",
            "second later"
        ]
    );
}

#[test]
fn listener_errors_propagate_after_the_record_and_stop_later_listeners() {
    let root = Root::new();
    let manager = root.manager();
    manager
        .listen(|_| Err(LogError::InvalidConfiguration))
        .unwrap();
    manager
        .listen(|_| panic!("later listener must not run"))
        .unwrap();
    assert!(matches!(
        manager.info("persisted first", &LogContext::new()),
        Err(LogError::InvalidConfiguration)
    ));
    assert!(
        fs::read_to_string(root.0.join("observed.log"))
            .unwrap()
            .contains("persisted first")
    );
}

#[test]
fn normalized_exceptions_are_present_in_the_listener_context() {
    let root = Root::new();
    let manager = root.manager();
    let seen = Arc::new(AtomicBool::new(false));
    let capture = seen.clone();
    manager
        .listen(move |event| {
            assert_eq!(event.message, "failure");
            assert!(
                event.context["exception"]
                    .to_string()
                    .contains("listener exception")
            );
            assert_eq!(event.context["id"], 7);
            capture.store(true, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    manager
        .logException(
            Level::Error,
            "failure",
            &std::io::Error::other("listener exception"),
            &LogContext::from_iter([("id".into(), json!(7))]),
        )
        .unwrap();
    assert!(seen.load(Ordering::SeqCst));
}

struct Observe;

impl ServiceProvider for Observe {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let path = app.base_path().join("events.jsonl");
            Log::listen(move |event: &MessageLogged| {
                let write = || -> std::io::Result<()> {
                    let mut file = fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)?;
                    writeln!(file, "{}", event.message)
                };
                write().map_err(|error| LogError::WriteFailed(Box::new(error)))
            })?;
            Ok(())
        })
    }
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/probe", record)
}

fn record(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Log::info("HTTP observed", &LogContext::new()).unwrap();
        Ok(Response::new(StatusCode::OK, Body::from("observed")))
    })
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command("listener:probe", |output| {
        Log::info("Artisan observed", &LogContext::new()).unwrap();
        output.comment("observed");
    })?
    .purpose("Verify log listeners")
}

fn application(root: &Root) -> Application {
    Application::configure(root.0.clone())
        .with_config(configuration)
        .with_providers(vec![Box::new(Observe)])
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap()
}

async fn server(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(application(root));
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let service = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handleTransport(request));
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

async fn request(address: std::net::SocketAddr) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut bytes = Vec::new();
    socket.read_to_end(&mut bytes).await.unwrap();
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
}

#[tokio::test]
async fn http_and_artisan_observers_remain_in_their_application() {
    let left = Root::new();
    let right = Root::new();
    let (left_address, left_server) = server(&left).await;
    let (right_address, right_server) = server(&right).await;
    tokio::join!(request(left_address), request(right_address));
    assert_eq!(
        application(&left)
            .handle_command(&["listener:probe".into()])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        fs::read_to_string(left.0.join("events.jsonl")).unwrap(),
        "HTTP observed\nArtisan observed\n"
    );
    assert_eq!(
        fs::read_to_string(right.0.join("events.jsonl")).unwrap(),
        "HTTP observed\n"
    );
    left_server.abort();
    right_server.abort();
    let _ = left_server.await;
    let _ = right_server.await;
}

#[tokio::test(flavor = "current_thread")]
async fn request_observers_await_after_writes_and_preserve_filtering_order_and_snapshots() {
    let root = Root::new();
    let manager = root.manager();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sync_seen = seen.clone();
    manager
        .listen(move |_| {
            sync_seen.lock().unwrap().push("sync");
            Ok(())
        })
        .unwrap();
    let fallback = seen.clone();
    let awaited = seen.clone();
    let owner = Arc::downgrade(&manager);
    let written = root.0.join("observed.log");
    manager
        .listenForRequest(
            move |_| {
                fallback.lock().unwrap().push("snapshot");
                Ok(())
            },
            Arc::new(move |event, request| {
                let seen = awaited.clone();
                let path = written.clone();
                let owner = owner.clone();
                Box::pin(async move {
                    assert!(fs::read_to_string(path).unwrap().contains(&event.message));
                    assert_eq!(request.path(), "observed");
                    tokio::task::yield_now().await;
                    seen.lock().unwrap().push("awaited");
                    let newly_seen = seen.clone();
                    owner.upgrade().unwrap().listen(move |_| {
                        newly_seen.lock().unwrap().push("new");
                        Ok(())
                    })?;
                    Ok(())
                })
            }),
        )
        .unwrap();
    let (parts, ()) = axum::http::Request::builder()
        .uri("/observed")
        .body(())
        .unwrap()
        .into_parts();
    let mut request = Request::from_parts(parts, Body::empty());
    manager
        .channel(Some("other"))
        .unwrap()
        .logWithRequest(&mut request, Level::Info, "filtered", &LogContext::new())
        .await
        .unwrap();
    assert!(seen.lock().unwrap().is_empty());
    manager
        .logWithRequest(
            &mut request,
            Level::Info,
            "written first",
            &LogContext::new(),
        )
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), ["sync", "awaited"]);
    manager.info("synchronous", &LogContext::new()).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        ["sync", "awaited", "sync", "snapshot", "new"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn request_observer_errors_and_cancellation_keep_written_logs_and_allow_recovery() {
    let root = Root::new();
    let manager = root.manager();
    let after = Arc::new(AtomicBool::new(false));
    manager
        .listenForRequest(
            |_| Ok(()),
            Arc::new(|event, _| {
                Box::pin(async move {
                    match event.message.as_str() {
                        "failure" => Err(LogError::FactoryFailed),
                        "cancel" => std::future::pending().await,
                        _ => Ok(()),
                    }
                })
            }),
        )
        .unwrap();
    let flag = after.clone();
    manager
        .listen(move |_| {
            flag.store(true, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    let (parts, ()) = axum::http::Request::new(()).into_parts();
    let mut request = Request::from_parts(parts, Body::empty());
    assert!(matches!(
        manager
            .logWithRequest(&mut request, Level::Info, "failure", &LogContext::new())
            .await,
        Err(LogError::FactoryFailed)
    ));
    assert!(!after.load(Ordering::SeqCst));
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            manager.logWithRequest(&mut request, Level::Info, "cancel", &LogContext::new())
        )
        .await
        .is_err()
    );
    assert!(!after.load(Ordering::SeqCst));
    let contents = fs::read_to_string(root.0.join("observed.log")).unwrap();
    assert!(contents.contains("failure") && contents.contains("cancel"));
    manager
        .logWithRequest(&mut request, Level::Info, "recovered", &LogContext::new())
        .await
        .unwrap();
    assert!(after.load(Ordering::SeqCst));
}
