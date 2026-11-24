use axum::handler::Handler as _;
use openrust::{
    console::Output,
    contracts::{
        container::Container as _,
        debug::{Exception, ExceptionHandler, SharedException},
        foundation::Application as _,
        http::Kernel as _,
    },
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        http::Kernel,
    },
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogResult,
    support::facades::{App, Exceptions, Facade, Route},
    view::{Factory, ViewError},
};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
#[derive(Default)]
struct Trace(Mutex<String>);
#[derive(Debug)]
struct MutableError(Arc<Trace>);
impl fmt::Debug for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Trace")
    }
}
impl fmt::Display for MutableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.0.lock().unwrap())
    }
}
impl Error for MutableError {}
impl openrust::contracts::debug::ExceptionBehavior for MutableError {
    fn render_async<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Option<Response>, HttpError>> {
        Box::pin(async move {
            if request.route("id") != Some("own-async") {
                return Ok(None);
            }
            let data =
                openrust::view::ViewData::from_iter([("message".into(), self.to_string().into())]);
            request
                .with_container(async move {
                    openrust::support::facades::Response::view(
                        "errors.custom",
                        data,
                        StatusCode::CONFLICT,
                        Default::default(),
                    )
                    .await
                    .map(Some)
                })
                .await
        })
    }
}
fn fail(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let code = request.route("code").unwrap();
        if code == "source" {
            return Err(HttpError::Internal {
                source: Box::new(SharedException::new(MutableError(
                    App::make::<Trace>().await?,
                ))),
            });
        }
        let status = StatusCode::from_u16(code.parse().unwrap()).unwrap();
        let mut headers = openrust::http::HeaderMap::new();
        headers.insert("x-proof", "preserved".parse().unwrap());
        Err(openrust::http::abort(
            status,
            format!(
                "failure-{code}-{} <entry>",
                request.route("id").unwrap_or("base")
            ),
            headers,
        ))
    })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/error/{code}", fail)?;
    Route::get("/error/{code}/{id}", fail)?;
    Ok(())
}
#[derive(Default)]
struct Delays {
    entered: tokio::sync::Notify,
    owners: Mutex<Vec<Weak<HttpError>>>,
}
struct Yielding(Arc<dyn ExceptionHandler>, Arc<Delays>);
impl ExceptionHandler for Yielding {
    fn should_report<'a>(&'a self, e: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        self.0.should_report(e)
    }
    fn report<'a>(&'a self, e: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        self.0.report(e)
    }
    fn render(&self, r: &Request, e: &Exception<'_>) -> Response {
        self.0.render(r, e)
    }
    fn render_for_console(&self, o: &mut Output, e: &Exception<'_>) {
        self.0.render_for_console(o, e)
    }
    fn handle_http<'a>(
        &'a self,
        r: &'a mut Request,
        e: Arc<HttpError>,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move {
            if r.uri().path().ends_with("/cancel") {
                self.1.owners.lock().unwrap().push(Arc::downgrade(&e));
                self.1.entered.notify_one();
                std::future::pending::<()>().await;
            }
            tokio::task::yield_now().await;
            self.0.handle_http(r, e).await
        })
    }
}
struct Server {
    address: std::net::SocketAddr,
    kernel: Arc<Kernel>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn production_config(
    _: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    Ok(std::collections::BTreeMap::from([
        (
            "app".into(),
            serde_json::json!({"debug":false,"key":"a".repeat(32)}),
        ),
        (
            "session".into(),
            serde_json::json!({"driver":"array","lottery":[0,100]}),
        ),
    ]))
}
fn debug_config(
    base: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    let mut config = production_config(base, env)?;
    config.get_mut("app").unwrap()["debug"] = true.into();
    Ok(config)
}
async fn start(
    root: &std::path::Path,
    debug: bool,
    delay: Option<Arc<Delays>>,
    failures: Arc<AtomicUsize>,
) -> Server {
    let observed = failures.clone();
    let mut app = Application::configure(root.to_owned())
        .with_config(if debug {
            debug_config
        } else {
            production_config
        })
        .with_exceptions(move |e| {
            e.use_exception::<MutableError>();
            e.render_async::<std::io::Error>(|error, _| {
                Box::pin(async move {
                    if error.to_string() != "mapped-async-original" {
                        return Ok(None);
                    }
                    tokio::task::yield_now().await;
                    Ok(Some(Response::new(
                        StatusCode::CONFLICT,
                        Body::from(error.to_string()),
                    )))
                })
            });
            e.render::<HttpError>(|_, request| {
                (request.route("id") == Some("sync-first"))
                    .then(|| Response::new(StatusCode::CONFLICT, Body::from("SYNC-FIRST")))
            });
            e.render_async::<HttpError>(|error, request| {
                Box::pin(async move {
                    if request.route("id") == Some("async-response") {
                        return Err(openrust::http::HttpResponseException::new(Response::new(
                            StatusCode::IM_A_TEAPOT,
                            Body::from("PREPARED-ASYNC"),
                        ))
                        .into());
                    }
                    match request.route("id") {
                        Some("async" | "async-fail") => {}
                        _ => return Ok(None),
                    }
                    let name = if request.route("id") == Some("async-fail") {
                        "errors.absent"
                    } else {
                        "errors.custom"
                    };
                    let data = openrust::view::ViewData::from_iter([(
                        "message".into(),
                        error.to_string().into(),
                    )]);
                    tokio::task::yield_now().await;
                    openrust::support::facades::Response::view_for_request(
                        name,
                        data,
                        StatusCode::CONFLICT,
                        Default::default(),
                        request,
                    )
                    .await
                    .map(Some)
                })
            });
            e.render::<HttpError>(|_, request| {
                (request.route("id") == Some("async-fallback"))
                    .then(|| Response::new(StatusCode::GONE, Body::from("SYNC-AFTER-NONE")))
            });
            e.map_optional::<HttpError, _>(|error| match error {
                HttpError::Aborted {
                    status, headers, ..
                } if *status == StatusCode::IM_A_TEAPOT => Some(SharedException::new(
                    openrust::http::abort(StatusCode::GONE, "mapped <entry>", (**headers).clone()),
                )),
                HttpError::Aborted { status, .. } if *status == StatusCode::PRECONDITION_FAILED => {
                    Some(SharedException::new(std::io::Error::other(
                        "mapped-async-original",
                    )))
                }
                _ => None,
            });
            e.report::<ViewError>(move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                ReportDecision::Stop
            });
            e.respond(move |mut response, exception, request| {
                let count: usize = response
                    .headers()
                    .get("x-respond-count")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                response
                    .parts_mut()
                    .headers
                    .insert("x-respond-count", (count + 1).to_string().parse().unwrap());
                if request.uri().path() == "/error/404" && failures.load(Ordering::SeqCst) > 0 {
                    assert_eq!(exception.downcast_ref::<ViewError>().is_some(), debug);
                }
                response
            });
        })
        .with_routing(Routing::default().web(routes))
        .create()
        .unwrap();
    let trace = Arc::new(Trace(Mutex::new("before".into())));
    app.container().instance(trace.clone()).unwrap();
    app.boot().await.unwrap();
    let views = app.container().make::<Factory>().unwrap();
    views.composer("errors.500", move |_| {
        *trace.0.lock().unwrap() = "mutated".into();
        Ok(())
    });
    App::set_facade_application(Some(&app), async {
        Exceptions::renderable_async::<HttpError>(|_, request| {
            Box::pin(async move {
                if request.route("id") != Some("facade-async") {
                    return Ok(None);
                }
                openrust::support::facades::Response::view_for_request(
                    "errors.custom",
                    Default::default(),
                    StatusCode::CONFLICT,
                    Default::default(),
                    request,
                )
                .await
                .map(Some)
            })
        })
        .unwrap();
    })
    .await;
    if let Some(delay) = delay {
        App::set_facade_application(Some(&app), async {
            let original = Exceptions::handler().unwrap();
            Exceptions::swap(Arc::new(Yielding(original, delay))).unwrap();
        })
        .await;
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let owned = kernel.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, transport.with_state(owned).into_make_service())
            .await
            .unwrap();
    });
    Server {
        address,
        kernel,
        task,
    }
}
async fn send(address: std::net::SocketAddr, path: String, accept: &str) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: {accept}\r\nConnection: close\r\n\r\n");
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        response
    }).await.unwrap()
}
fn template(root: &std::path::Path, name: &str, text: &str) {
    let dir = root.join("resources/views/errors");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.blade.html")), text).unwrap();
}
#[tokio::test]
async fn exact_pages_use_real_exception_objects_headers_and_empty_error_bags() {
    let root = tempfile::tempdir().unwrap();
    template(
        root.path(),
        "410",
        r#"<h2>{{ $exception->getMessage() }}</h2><code>{{ $exception->getStatusCode() }}</code>@if($errors->any())
BAD
@else
EMPTY
@endif
@foreach($exception->getHeaders() as $name => $value)<i>{{ $name }}={{ $value }}</i>@endforeach"#,
    );
    template(
        root.path(),
        "500",
        r#"<cause>{{ $exception->getPrevious()->getMessage() }}</cause>"#,
    );
    let server = start(root.path(), false, None, Arc::default()).await;
    let text = send(server.address, "/error/410".into(), "text/html").await;
    assert!(text.starts_with("HTTP/1.1 410"), "{text}");
    for expected in [
        "failure-410-base &lt;entry&gt;",
        "<code>410</code>",
        "EMPTY",
        "<i>x-proof=preserved</i>",
        "\r\nx-proof: preserved\r\n",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    let mapped = send(server.address, "/error/418".into(), "text/html").await;
    assert!(mapped.starts_with("HTTP/1.1 410"));
    assert!(mapped.contains("<h2>mapped &lt;entry&gt;</h2>"), "{mapped}");
    let text = send(server.address, "/error/source".into(), "text/html").await;
    assert!(text.starts_with("HTTP/1.1 500"));
    assert!(text.contains("<cause>mutated</cause>"), "{text}");
}
#[tokio::test]
async fn family_fallback_does_not_override_eight_dedicated_statuses_or_json() {
    let root = tempfile::tempdir().unwrap();
    template(root.path(), "4xx", "<p>FOUR-FAMILY</p>");
    template(root.path(), "5xx", "<p>FIVE-FAMILY</p>");
    template(root.path(), "404", "<p>EXACT-404</p>");
    let server = start(root.path(), false, None, Arc::default()).await;
    for status in [401, 402, 403, 404, 419, 429, 500, 503, 410, 502] {
        let text = send(server.address, format!("/error/{status}"), "text/html").await;
        assert!(text.starts_with(&format!("HTTP/1.1 {status}")), "{text}");
        if status == 404 {
            assert!(text.contains("EXACT-404"));
        } else {
            assert_eq!(text.contains("FOUR-FAMILY"), status == 410, "{text}");
            assert_eq!(text.contains("FIVE-FAMILY"), status == 502, "{text}");
        }
    }
    let text = send(server.address, "/error/410".into(), "application/json").await;
    assert!(text.contains("application/json"));
    assert!(!text.contains("FAMILY"));
}
#[tokio::test]
async fn failing_error_templates_report_before_respond_and_debug_selects_the_new_failure() {
    for debug in [false, true] {
        let root = tempfile::tempdir().unwrap();
        template(root.path(), "404", "{{ $exception->missingMethod() }}");
        let failures = Arc::new(AtomicUsize::new(0));
        let server = start(root.path(), debug, None, failures.clone()).await;
        let text = send(server.address, "/error/404".into(), "text/html").await;
        let status = if debug { 500 } else { 404 };
        assert!(text.starts_with(&format!("HTTP/1.1 {status}")), "{text}");
        assert_eq!(failures.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn error_object_owners_are_request_local_and_cancellation_releases_the_real_error() {
    let root = tempfile::tempdir().unwrap();
    template(
        root.path(),
        "410",
        "<own>{{ $exception->getMessage() }}</own>",
    );
    let delays = Arc::new(Delays::default());
    let server = start(root.path(), false, Some(delays.clone()), Arc::default()).await;
    let tasks = (0..32)
        .map(|id| {
            let address = server.address;
            tokio::spawn(async move {
                let text = send(address, format!("/error/410/{id}"), "text/html").await;
                assert!(
                    text.contains(&format!("<own>failure-410-{id} &lt;entry&gt;</own>")),
                    "{text}"
                );
            })
        })
        .collect::<Vec<_>>();
    for task in tasks {
        task.await.unwrap();
    }
    let kernel = server.kernel.clone();
    let task = tokio::spawn(async move {
        kernel
            .handle_transport(
                http::Request::builder()
                    .uri("/error/410/cancel")
                    .header("host", "example.test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), delays.entered.notified())
        .await
        .unwrap();
    task.abort();
    let _ = task.await;
    assert_eq!(delays.owners.lock().unwrap()[0].strong_count(), 0);
    let text = send(server.address, "/error/410/after".into(), "text/html").await;
    assert!(
        text.contains("<own>failure-410-after &lt;entry&gt;</own>"),
        "{text}"
    );
}

#[tokio::test]
async fn artisan_publishes_eight_default_pages_without_overwriting_customizations() {
    let root = tempfile::tempdir().unwrap();
    async fn publish(root: &std::path::Path, force: bool) {
        let mut arguments = vec![
            "vendor:publish".into(),
            "--tag=laravel-errors".into(),
            "--no-interaction".into(),
        ];
        if force {
            arguments.push("--force".into());
        }
        let code = Application::configure(root.to_owned())
            .with_config(production_config)
            .create()
            .unwrap()
            .handle_command(&arguments)
            .await
            .unwrap();
        assert_eq!(code, 0);
    }
    publish(root.path(), false).await;
    let directory = root.path().join("resources/views/errors");
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 8);
    let original = std::fs::read_to_string(directory.join("404.blade.html")).unwrap();
    template(root.path(), "404", "<h1>custom-not-found</h1>");
    publish(root.path(), false).await;
    assert_eq!(
        std::fs::read_to_string(directory.join("404.blade.html")).unwrap(),
        "<h1>custom-not-found</h1>"
    );
    publish(root.path(), true).await;
    assert_eq!(
        std::fs::read_to_string(directory.join("404.blade.html")).unwrap(),
        original
    );
    let server = start(root.path(), false, None, Arc::default()).await;
    for status in [401, 402, 403, 404, 419, 429, 500, 503] {
        let text = send(server.address, format!("/error/{status}"), "text/html").await;
        assert!(text.starts_with(&format!("HTTP/1.1 {status}")), "{text}");
        assert!(text.contains(&format!("<h1>{status}</h1>")), "{text}");
        assert!(!text.contains("failure-"), "{text}");
    }
}

#[tokio::test]
async fn formal_three_language_blade_examples_render_the_original_http_error() {
    for document in [
        include_str!("../../../../docs/en/the-basics/errors.md"),
        include_str!("../../../../docs/zh-Hans/the-basics/errors.md"),
        include_str!("../../../../docs/zh-Hant/the-basics/errors.md"),
    ] {
        let section = document.split("{#custom-http-error-pages}").nth(1).unwrap();
        let source = section
            .split("```blade\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        template(root.path(), "410", source);
        let server = start(root.path(), false, None, Arc::default()).await;
        let text = send(server.address, "/error/410".into(), "text/html").await;
        assert!(text.starts_with("HTTP/1.1 410"));
        assert!(
            text.contains("<h2>failure-410-base &lt;entry&gt;</h2>"),
            "{text}"
        );
        assert!(text.contains("<code>410</code>"), "{text}");
    }
}

#[tokio::test]
async fn asynchronous_exception_renderers_use_named_response_views_and_preserve_order_and_failures()
{
    let root = tempfile::tempdir().unwrap();
    template(
        root.path(),
        "custom",
        "<async>VIEW {{ $message ?? 'facade' }}</async>",
    );
    let failures = Arc::new(AtomicUsize::new(0));
    let server = start(root.path(), false, None, failures.clone()).await;
    for (path, status, body) in [
        ("/error/410/async", 409, "<async>VIEW"),
        ("/error/410/async-response", 418, "PREPARED-ASYNC"),
        ("/error/412", 409, "mapped-async-original"),
        ("/error/410/nobody", 410, "<h1>410"),
        ("/error/410/sync-first", 409, "SYNC-FIRST"),
        ("/error/410/async-fallback", 410, "SYNC-AFTER-NONE"),
        ("/error/410/facade-async", 409, "<async>VIEW facade</async>"),
        ("/error/source/own-async", 409, "<async>VIEW before</async>"),
        ("/error/410/async-fail", 500, "<h1>500</h1>"),
    ] {
        let text = send(server.address, path.into(), "text/html").await;
        assert!(text.starts_with(&format!("HTTP/1.1 {status}")), "{text}");
        assert!(text.contains(body), "{text}");
        assert!(text.contains("\r\nx-respond-count: 1\r\n"), "{text}");
    }
    assert_eq!(failures.load(Ordering::SeqCst), 1);
}
