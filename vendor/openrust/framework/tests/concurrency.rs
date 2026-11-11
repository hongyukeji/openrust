use std::{ffi::OsString, path::PathBuf, time::Duration};

use indexmap::IndexMap;
use openrust::{
    concurrency::{Error, ProcessDriver, Registry, Task},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    process::Factory,
    support::{ProviderResult, ServiceProvider},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-concurrency-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Deserialize, Serialize)]
struct Probe {
    value: String,
    delay_ms: u64,
    marker: Option<PathBuf>,
}

struct Worker;

impl ServiceProvider for Worker {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let registry = app.container().make::<Registry>()?;
            registry.register("fixture:probe", |probe: Probe| async move {
                if let Some(path) = &probe.marker {
                    std::fs::write(
                        path.with_extension("started"),
                        std::process::id().to_string(),
                    )?;
                }
                // Worker stdout may contain arbitrary application output.
                println!("noise @@CONCURRENCY-V1@@unsigned.payload");
                tokio::time::sleep(Duration::from_millis(probe.delay_ms)).await;
                if let Some(path) = &probe.marker {
                    std::fs::write(path, "completed")?;
                }
                Ok::<_, std::io::Error>(json!({"value": probe.value, "pid": std::process::id()}))
            })?;
            registry.register("fixture:fail", |_: ()| async {
                Err::<(), _>("deliberate failure")
            })?;
            Ok(())
        })
    }
}

#[tokio::test]
#[ignore = "launched as a real Artisan worker by the process driver tests"]
async fn artisan_worker_fixture() {
    let app = Application::configure(std::env::current_dir().unwrap())
        .withProviders(vec![Box::new(Worker)])
        .create()
        .unwrap();
    let exit = app
        .handleCommand(&["invoke-serialized-closure".into()])
        .await
        .unwrap();
    assert_eq!(exit, 0);
}

fn driver(root: &TestDirectory) -> ProcessDriver {
    let mut command = vec![std::env::current_exe().unwrap().into_os_string()];
    command.extend(
        [
            "--ignored",
            "--exact",
            "artisan_worker_fixture",
            "--nocapture",
        ]
        .map(OsString::from),
    );
    ProcessDriver::new(Factory::new(), command, root.0.clone())
}

fn probe(value: &str, delay_ms: u64, marker: Option<PathBuf>) -> Task {
    Task::new(
        "fixture:probe",
        Probe {
            value: value.into(),
            delay_ms,
            marker,
        },
    )
    .unwrap()
}

#[tokio::test]
async fn process_driver_returns_ordered_named_results_from_real_artisan_children() {
    let root = TestDirectory::new();
    let driver = driver(&root);
    let parent = std::process::id();
    let results = driver
        .runNamed(
            IndexMap::from([
                ("slower".into(), probe("first", 70, None)),
                ("faster".into(), probe("second", 0, None)),
            ]),
            Some(Duration::from_secs(5)),
        )
        .await
        .unwrap();
    assert_eq!(
        results.keys().map(String::as_str).collect::<Vec<_>>(),
        ["slower", "faster"]
    );
    assert_eq!(results["slower"]["value"], "first");
    assert_eq!(results["faster"]["value"], "second");
    assert_ne!(results["slower"]["pid"], parent);
    assert_ne!(results["faster"]["pid"], parent);
    assert_ne!(results["slower"]["pid"], results["faster"]["pid"]);
    let failure = driver
        .runNamed(
            IndexMap::from([
                ("failed".into(), Task::new("fixture:fail", ()).unwrap()),
                (
                    "other".into(),
                    probe("completed", 20, Some(root.0.join("sibling"))),
                ),
            ]),
            None,
        )
        .await;
    assert!(
        matches!(failure, Err(Error::TaskFailed { task, message }) if task == "failed" && message.contains("deliberate failure"))
    );
    assert!(root.0.join("sibling").is_file());
}

#[tokio::test]
async fn timeout_and_cancellation_reclaim_live_worker_processes() {
    let root = TestDirectory::new();
    let driver = driver(&root);
    let cancel_marker = root.0.join("cancel");
    let timeout_marker = root.0.join("timeout");
    let cancellation = tokio::spawn({
        let driver = driver.clone();
        let cancel_marker = cancel_marker.clone();
        async move {
            driver
                .runNamed(
                    IndexMap::from([("cancel".into(), probe("cancel", 1000, Some(cancel_marker)))]),
                    None,
                )
                .await
        }
    });
    wait_for_started(&cancel_marker).await;
    cancellation.abort();
    assert!(cancellation.await.unwrap_err().is_cancelled());

    let result = driver
        .runNamed(
            IndexMap::from([(
                "timeout".into(),
                probe("timeout", 1000, Some(timeout_marker.clone())),
            )]),
            Some(Duration::from_millis(600)),
        )
        .await;
    assert!(matches!(
        result,
        Err(Error::Process(openrust::process::Error::TimedOut {
            idle: false
        }))
    ));
    assert!(timeout_marker.with_extension("started").exists());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!cancel_marker.exists());
    assert!(!timeout_marker.exists());
}

async fn wait_for_started(marker: &std::path::Path) {
    let started = marker.with_extension("started");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !started.is_file() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn sync_driver_uses_registered_tasks_without_spawning() {
    let root = TestDirectory::new();
    let mut application = Application::configure(root.0.clone())
        .withProviders(vec![Box::new(Worker)])
        .create()
        .unwrap();
    application.boot().await.unwrap();
    let manager = application
        .container()
        .make::<openrust::concurrency::Manager>()
        .unwrap();
    assert!(matches!(
        manager.driver(Some("invalid")),
        Err(Error::UnknownDriver(_))
    ));
    let results = manager
        .driver(Some("sync"))
        .unwrap()
        .run([probe("local", 0, None), probe("again", 0, None)], None)
        .await
        .unwrap();
    assert_eq!(
        results
            .iter()
            .map(|value| &value["pid"])
            .collect::<Vec<_>>(),
        [&json!(std::process::id()), &json!(std::process::id())]
    );
    assert_eq!(results[0]["value"], "local");
    assert_eq!(results[1]["value"], "again");
    let error = manager
        .driver(Some("sync"))
        .unwrap()
        .run([Task::new("missing", Value::Null).unwrap()], None)
        .await;
    assert!(matches!(error, Err(Error::UnknownTask(name)) if name == "missing"));
}

#[tokio::test]
async fn defer_runs_after_the_actual_http_response_and_respects_failure() {
    use std::sync::Arc;

    use axum::{Router, extract::State};
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{configuration::Routing, http::Kernel},
        http::{HttpError, Request, Response, StatusCode, client::Factory as HttpFactory},
        support::facades::{Concurrency, Route},
    };
    use tokio::sync::{Notify, mpsc};

    struct DeferredWorker {
        started: mpsc::UnboundedSender<String>,
        release: Arc<Notify>,
    }

    impl ServiceProvider for DeferredWorker {
        fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                let registry = app.container().make::<Registry>()?;
                let started = self.started.clone();
                let release = self.release.clone();
                registry.register("fixture:deferred", move |label: String| {
                    let started = started.clone();
                    let release = release.clone();
                    async move {
                        let context = openrust::log::context::Context::get("test")
                            .map_err(|error| error.to_string())?;
                        if context.as_ref().and_then(Value::as_str) != Some("request") {
                            return Err("request context was not carried".to_owned());
                        }
                        started.send(label).map_err(|error| error.to_string())?;
                        release.notified().await;
                        Ok::<_, String>(())
                    }
                })?;
                Ok(())
            })
        }
    }

    fn enqueue(_request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            openrust::log::context::Context::add("test", json!("request")).unwrap();
            let driver = Concurrency::driver(Some("sync")).unwrap();
            let _deferred = driver
                .defer([Task::new("fixture:deferred", "ok").unwrap()])
                .unwrap();
            Response::json(StatusCode::OK, json!({"sent": true}))
        })
    }

    fn enqueue_failed(_request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            openrust::log::context::Context::add("test", json!("request")).unwrap();
            let driver = Concurrency::driver(Some("sync")).unwrap();
            let _skipped = driver
                .defer([Task::new("fixture:deferred", "skipped").unwrap()])
                .unwrap();
            let _always = driver
                .defer([Task::new("fixture:deferred", "always").unwrap()])
                .unwrap()
                .always();
            Ok(Response::empty(StatusCode::BAD_REQUEST))
        })
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handleTransport(incoming).await
    }

    let root = TestDirectory::new();
    let (started, mut observed) = mpsc::unbounded_channel();
    let release = Arc::new(Notify::new());
    let application = Application::configure(root.0.clone())
        .withProviders(vec![Box::new(DeferredWorker {
            started,
            release: release.clone(),
        })])
        .withRouting(Routing::default().web(|| {
            Route::get("/ok", enqueue)?;
            Route::get("/failed", enqueue_failed)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let router = Router::new()
        .fallback(transport)
        .with_state(Arc::new(kernel));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = HttpFactory::new().unwrap();

    let response = client
        .pendingRequest()
        .timeout(Duration::from_secs(2))
        .get(&format!("{url}/ok"))
        .await
        .unwrap();
    assert!(response.ok());
    assert_eq!(response.json::<Value>().unwrap(), json!({"sent": true}));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), observed.recv())
            .await
            .unwrap()
            .as_deref(),
        Some("ok")
    );
    release.notify_one();

    let response = client.get(&format!("{url}/failed")).await.unwrap();
    assert!(response.badRequest());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), observed.recv())
            .await
            .unwrap()
            .as_deref(),
        Some("always")
    );
    release.notify_one();
    server.abort();
}
