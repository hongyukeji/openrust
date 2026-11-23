use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use openrust::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        http::Kernel,
    },
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{Router, RoutingError},
    support::{defer, facades::Facade},
};

type Events = Arc<Mutex<Vec<(usize, usize, &'static str)>>>;
struct Buffer {
    id: usize,
    value: AtomicUsize,
    events: Events,
}
struct BufferFacade;
impl Facade for BufferFacade {
    type Service = Buffer;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Buffer>()
    }
}
fn schedule(buffer: Arc<Buffer>, label: &'static str, name: Option<&str>, always: bool) {
    let _ = defer(
        async move {
            buffer.events.lock().unwrap().push((
                buffer.id,
                buffer.value.load(Ordering::SeqCst),
                label,
            ));
            Ok::<_, std::io::Error>(())
        },
        name,
        always,
    )
    .unwrap();
}
fn handle(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let buffer = BufferFacade::get_facade_root().unwrap();
        schedule(buffer.clone(), "ordinary", None, false);
        schedule(buffer.clone(), "replaced", Some("buffer"), false);
        schedule(buffer.clone(), "always", Some("buffer"), true);
        assert!(
            buffer
                .events
                .lock()
                .unwrap()
                .iter()
                .all(|entry| entry.0 != buffer.id)
        );
        buffer.value.store(42, Ordering::SeqCst);
        match request.uri().path() {
            "/failed" => Err(openrust::http::abort(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid",
                Default::default(),
            )),
            "/cancel" => {
                std::future::pending::<()>().await;
                unreachable!()
            }
            "/nested" => {
                openrust::support::facades::Artisan::callSilently("probe:nested", &[])
                    .await
                    .unwrap();
                assert!(
                    buffer
                        .events
                        .lock()
                        .unwrap()
                        .iter()
                        .all(|entry| entry.0 != buffer.id)
                );
                assert_eq!(buffer.value.load(Ordering::SeqCst), 84);
                Ok(Response::from("nested"))
            }
            "/sync" => {
                use openrust::queue::Payload;
                let queue = openrust::support::facades::Queue::connection(Some("sync")).unwrap();
                queue
                    .push(
                        &Payload::new("probe:sync", serde_json::Value::Null).unwrap(),
                        None,
                    )
                    .await
                    .unwrap();
                assert!(
                    buffer
                        .events
                        .lock()
                        .unwrap()
                        .iter()
                        .all(|entry| entry.0 != buffer.id)
                );
                assert_eq!(buffer.value.load(Ordering::SeqCst), 84);
                Ok(Response::from("sync"))
            }
            "/stream" => Ok(Response::new(
                StatusCode::OK,
                Body::from_stream(futures_util::stream::pending::<
                    Result<&'static str, std::io::Error>,
                >()),
            )),
            _ => Ok(Response::from("ok")),
        }
    })
}
fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    for path in ["/ok", "/failed", "/cancel", "/stream", "/nested", "/sync"] {
        router.get(path, handle)?;
    }
    Ok(router)
}
async fn kernel() -> (Kernel, Events) {
    let events: Events = Arc::default();
    let app = Application::configure(std::env::temp_dir())
        .with_routing(Routing::new(routes).commands(register_nested))
        .create()
        .unwrap();
    let counter = AtomicUsize::new(0);
    let seen = events.clone();
    app.container()
        .scoped::<Buffer, _>(move |_| {
            Ok(Arc::new(Buffer {
                id: counter.fetch_add(1, Ordering::SeqCst),
                value: AtomicUsize::new(0),
                events: seen.clone(),
            }))
        })
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let app = kernel.get_application();
    app.config()
        .unwrap()
        .set(
            "queue.connections.sync",
            serde_json::json!({"driver":"sync"}),
        )
        .unwrap();
    app.container()
        .make::<openrust::queue::QueueManager>()
        .unwrap()
        .register("probe:sync", Arc::new(NestedJob))
        .unwrap();
    (kernel, events)
}
fn request(path: &str) -> Request {
    let (parts, _) = http::Request::builder()
        .uri(path)
        .body(())
        .unwrap()
        .into_parts();
    Request::from_parts(parts, Body::empty())
}
#[tokio::test]
async fn http_callbacks_keep_original_scoped_instances_and_run_once_after_response() {
    let (kernel, events) = kernel().await;
    let mut first = request("/ok");
    let mut second = request("/failed");
    let (a, b) = tokio::join!(kernel.handle(&mut first), kernel.handle(&mut second));
    assert!(events.lock().unwrap().is_empty());
    let (a, _) = a.into_parts();
    let (b, _) = b.into_parts();
    let (first, _) = first.into_parts();
    let (second, _) = second.into_parts();
    kernel.terminate(&second, &b).await;
    kernel.terminate(&first, &a).await;
    kernel.terminate(&first, &a).await;
    let entries = events.lock().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].2, "always");
    assert_ne!(entries[0].0, entries[1].0);
    assert_eq!(entries[1].0, entries[2].0);
    assert_eq!((entries[1].2, entries[2].2), ("ordinary", "always"));
    assert!(entries.iter().all(|entry| entry.1 == 42));
}
#[tokio::test]
async fn transport_waits_for_stream_body_drop_and_flushes_on_disconnect() {
    let (kernel, events) = kernel().await;
    let kernel = Arc::new(kernel);
    let response = kernel
        .handleTransport(
            http::Request::builder()
                .uri("/stream")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
    assert!(events.lock().unwrap().is_empty());
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), async {
        while events.lock().unwrap().len() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(events.lock().unwrap().iter().all(|entry| entry.1 == 42));
}
#[tokio::test]
async fn cancellation_before_response_does_not_claim_to_flush() {
    let (kernel, events) = kernel().await;
    let mut incoming = request("/cancel");
    assert!(
        tokio::time::timeout(Duration::from_millis(20), kernel.handle(&mut incoming))
            .await
            .is_err()
    );
    assert!(events.lock().unwrap().is_empty());
    let mut next = request("/ok");
    let response = kernel.handle(&mut next).await;
    let (next, _) = next.into_parts();
    let (response, _) = response.into_parts();
    kernel.terminate(&next, &response).await;
    assert_eq!(events.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn registering_without_lifecycle_returns_error() {
    assert!(matches!(
        defer(async { Ok::<_, std::io::Error>(()) }, None, false),
        Err(openrust::concurrency::Error::OutsideRequest)
    ));
}

struct ProbeCommand(openrust::console::CommandDefinition, i32, Arc<AtomicUsize>);
impl openrust::console::Command for ProbeCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            let buffer = BufferFacade::get_facade_root().unwrap();
            schedule(buffer.clone(), "ordinary", None, false);
            schedule(buffer.clone(), "always", None, true);
            buffer.value.store(42, Ordering::SeqCst);
            let _ = defer(
                async { Err(std::io::Error::other("flush failure")) },
                None,
                true,
            )
            .unwrap();
            let seen = self.2.clone();
            let _ = defer(
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    let _ = defer(
                        async move {
                            seen.fetch_add(1, Ordering::SeqCst);
                            Ok::<_, std::io::Error>(())
                        },
                        None,
                        true,
                    )
                    .unwrap();
                    Ok::<_, std::io::Error>(())
                },
                None,
                true,
            )
            .unwrap();
            if self.1 < 0 {
                return Err(openrust::console::ConsoleError::new("command failure"));
            }
            Ok(self.1)
        })
    }
}
#[tokio::test]
async fn artisan_flushes_success_nonzero_and_errors_and_reports_failures_without_stopping_callbacks()
 {
    use openrust::{
        console::{Application as Artisan, CommandDefinition},
        contracts::console::Kernel as _,
    };
    let events: Events = Arc::default();
    let seen = events.clone();
    let reports = Arc::new(AtomicUsize::new(0));
    let report_count = reports.clone();
    let app = Application::configure(std::env::temp_dir())
        .with_exceptions(move |exceptions| {
            exceptions.report::<std::io::Error>(move |_| {
                report_count.fetch_add(1, Ordering::SeqCst);
                ReportDecision::Stop
            });
        })
        .create()
        .unwrap();
    app.container()
        .scoped::<Buffer, _>(move |_| {
            Ok(Arc::new(Buffer {
                id: 0,
                value: AtomicUsize::new(0),
                events: seen.clone(),
            }))
        })
        .unwrap();
    let mut kernel = openrust::foundation::console::Kernel::new(app, Artisan::new());
    let calls = Arc::new(AtomicUsize::new(0));
    for (name, code) in [
        ("probe:success", 0),
        ("probe:nonzero", 3),
        ("probe:error", -1),
    ] {
        events.lock().unwrap().clear();
        let definition = CommandDefinition::new(clap::Command::new(name));
        let input = definition.parse(&[]).unwrap();
        kernel
            .register_command(Box::new(ProbeCommand(definition, code, calls.clone())))
            .unwrap();
        let result = kernel.call(name, &input, None).await;
        if code < 0 {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), code);
        }
        let events = events.lock().unwrap();
        assert_eq!(events.len(), if code == 0 { 2 } else { 1 });
        assert_eq!(events.last().unwrap().2, "always");
        assert!(events.iter().all(|entry| entry.1 == 42));
    }
    assert_eq!(reports.load(Ordering::SeqCst), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 6);
}

struct NestedCommand(openrust::console::CommandDefinition);
impl openrust::console::Command for NestedCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async {
            let buffer = BufferFacade::get_facade_root().unwrap();
            schedule(buffer.clone(), "nested", None, true);
            buffer.value.store(84, Ordering::SeqCst);
            Ok(0)
        })
    }
}
fn register_nested() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(NestedCommand(
        openrust::console::CommandDefinition::new(clap::Command::new("probe:nested")),
    )))
}
struct NestedJob;
impl openrust::contracts::queue::JobHandler for NestedJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        _: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async {
            let buffer = BufferFacade::get_facade_root().unwrap();
            schedule(buffer.clone(), "nested", None, true);
            buffer.value.store(84, Ordering::SeqCst);
            Ok(())
        })
    }
}
#[tokio::test]
async fn nested_artisan_and_sync_jobs_flush_at_the_enclosing_http_boundary() {
    let (kernel, events) = kernel().await;
    for path in ["/nested", "/sync"] {
        events.lock().unwrap().clear();
        let mut incoming = request(path);
        let response = kernel.handle(&mut incoming).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(events.lock().unwrap().is_empty());
        let (incoming, _) = incoming.into_parts();
        let (response, _) = response.into_parts();
        kernel.terminate(&incoming, &response).await;
        let entries = events.lock().unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|entry| entry.1 == 84));
    }
}
