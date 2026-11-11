use std::{
    ffi::OsString,
    fs,
    io::{self, Read as _, Write as _},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{debug::SharedException, http::Kernel as _},
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        http::Kernel,
    },
    http::{HttpError, HttpFuture, Request, Response},
    log::{Level, LogContext},
    routing::{Router, RoutingError},
    support::facades::Context,
};
use serde_json::json;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "openrust-exceptions-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn log(&self) -> String {
        fs::read_to_string(self.0.join("storage/logs/openrust.log")).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug)]
struct Failure(&'static str);
impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for Failure {}

fn fail(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let kind = request.route("kind").unwrap();
        Context::add("trace", json!(kind)).unwrap();
        Context::addHidden("token", json!("hidden-exception-token")).unwrap();
        let source: Box<dyn std::error::Error + Send + Sync> = if kind == "shared" {
            Box::new(
                request
                    .parts()
                    .extensions
                    .get::<SharedException>()
                    .unwrap()
                    .clone(),
            )
        } else {
            Box::new(Failure(match kind {
                "stop" => "stop",
                "ignored" => "ignored",
                "conditional" => "conditional",
                _ => "private-error-detail",
            }))
        };
        if kind == "ignored" {
            return Err(HttpError::Internal {
                source: Box::new(io::Error::other(source)),
            });
        }
        Err(HttpError::Internal { source })
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/failure/{kind}", fail)?;
    Ok(router)
}

#[derive(Clone)]
struct ServerState(Arc<Kernel>, SharedException);

async fn transport(
    State(state): State<ServerState>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (mut parts, body) = incoming.into_parts();
    parts.extensions.insert(state.1.clone());
    let mut request = Request::from_parts(parts, body);
    let (parts, body) = state.0.handle(&mut request).await.into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn send(address: SocketAddr, path: &'static str, status: u16) {
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with(&format!("HTTP/1.1 {status} ")), "{response}");
        let response: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(response["message"], if status == 500 { "Server Error" } else { "Not Found" });
        assert!(!body.contains("private-error-detail"));
    }).await.unwrap();
}

#[tokio::test]
async fn http_reports_default_logs_with_context_and_deduplicates_only_shared_instances() {
    let root = Root::new();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let observed = callbacks.clone();
    let application = Application::configure(root.0.clone())
        .withRouting(Routing::new(routes))
        .withExceptions(move |exceptions| {
            exceptions
                .dontReportDuplicates()
                .dontReport::<io::Error>()
                .dontReportWhen(|error| {
                    error
                        .downcast_ref::<Failure>()
                        .is_some_and(|cause| cause.0 == "conditional")
                })
                .level::<Failure>(Level::Critical)
                .level::<HttpError>(Level::Notice)
                .level::<Failure>(Level::Warning)
                .context(|_| LogContext::from_iter([("application".into(), json!("probe"))]))
                .report::<Failure>(move |failure| {
                    if failure.0 == "stop" {
                        ReportDecision::Stop
                    } else {
                        observed.fetch_add(1, Ordering::Relaxed);
                        ReportDecision::Continue
                    }
                })
                .report::<HttpError>(|error| {
                    assert!(std::error::Error::source(error).is_some());
                    ReportDecision::Continue
                });
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = ServerState(
        Arc::new(kernel),
        SharedException::new(Failure("private-error-detail")),
    );
    let service = transport.with_state(state).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    tokio::join!(
        send(address, "/failure/shared", 500),
        send(address, "/failure/shared", 500),
        send(address, "/failure/default", 500),
    );
    for path in [
        "/failure/default",
        "/failure/stop",
        "/failure/ignored",
        "/failure/conditional",
    ] {
        send(address, path, 500).await;
    }
    send(address, "/missing", 404).await;
    assert_eq!(callbacks.load(Ordering::Relaxed), 3);
    let log = root.log();
    assert_eq!(log.lines().count(), 3);
    assert_eq!(
        log.matches(
            r#"production.WARNING: 500 Internal Server Error {"application":"probe","status":500}"#
        )
        .count(),
        3
    );
    assert_eq!(log.matches(r#"{"trace":"shared"}"#).count(), 1);
    assert_eq!(log.matches(r#"{"trace":"default"}"#).count(), 2);
    assert!(!log.contains("private-error-detail"));
    assert!(!log.contains("hidden-exception-token"));
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn ignored_shared_error_can_report_later_and_deduplication_is_application_local() {
    let root = Root::new();
    let other = Root::new();
    let ignore = Arc::new(AtomicBool::new(true));
    let observed = ignore.clone();
    let mut app = Application::configure(root.0.clone())
        .withExceptions(move |exceptions| {
            exceptions
                .dontReportDuplicates()
                .dontReportWhen(move |_| observed.load(Ordering::Relaxed))
                .stopIgnoring::<HttpError>();
        })
        .create()
        .unwrap();
    let mut second = Application::configure(other.0.clone())
        .withExceptions(|exceptions| {
            exceptions.dontReportDuplicates();
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    second.boot().await.unwrap();
    let error = SharedException::new(Failure("shared explicit report"));
    app.report(&error).unwrap();
    assert!(!root.0.join("storage/logs/openrust.log").exists());
    ignore.store(false, Ordering::Relaxed);
    app.report(&error).unwrap();
    app.report(&error.clone()).unwrap();
    second.report(&error).unwrap();
    app.report(&HttpError::NotFound).unwrap();
    second.report(&HttpError::NotFound).unwrap();
    let concurrent = SharedException::new(Failure("one concurrent report"));
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                barrier.wait();
                app.report(&concurrent).unwrap();
            });
        }
    });
    assert_eq!(root.log().matches("one concurrent report").count(), 1);
    assert_eq!(root.log().matches("shared explicit report").count(), 1);
    assert_eq!(other.log().matches("shared explicit report").count(), 1);
    assert!(root.log().contains(r#"404 Not Found {"status":404}"#));
    assert!(!other.log().contains("404 Not Found"));
}

struct FailingCommand(CommandDefinition);
impl Command for FailingCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            Context::add("entry", json!("artisan")).unwrap();
            Context::addHidden("token", json!("hidden-command-token")).unwrap();
            Err(ConsoleError::new("artisanal failure"))
        })
    }
}

fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(FailingCommand(CommandDefinition::new(
        clap::Command::new("report:probe"),
    ))))
}

async fn run_artisan(root: &Path) -> i32 {
    Application::configure(root.to_path_buf())
        .withCommands(commands)
        .create()
        .unwrap()
        .handleCommand(&[OsString::from("report:probe")])
        .await
        .unwrap()
}

#[tokio::test]
async fn artisan_failure_logs_and_keeps_exit_code_and_stderr() {
    const CHILD: &str = "OPENRUST_TEST_EXCEPTION_REPORTING_ROOT";
    if let Some(root) = std::env::var_os(CHILD) {
        std::process::exit(run_artisan(Path::new(&root)).await);
    }
    let root = Root::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "artisan_failure_logs_and_keeps_exit_code_and_stderr",
            "--nocapture",
        ])
        .env(CHILD, &root.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "artisanal failure\n"
    );
    let log = root.log();
    assert!(log.contains(r#"production.ERROR: artisanal failure {} {"entry":"artisan"}"#));
    assert!(!log.contains("hidden-command-token"));
}
