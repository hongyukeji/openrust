use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::{http::Kernel as _, routing::Registrar as _, support::Responsable},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        Body, HttpError, HttpFuture, HttpResponseException, Method, Request, Response, StatusCode,
        abort_responsable, abort_response,
    },
    routing::{Router, RoutingError},
    support::facades::Redirect,
};
use serde_json::json;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "openrust-render-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn page(&self, name: &str, content: &str) {
        let directory = self.0.join("resources/views/errors");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(name), content).unwrap();
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug)]
struct PrivateSource;
impl std::fmt::Display for PrivateSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<script>private-render-source</script>")
    }
}
impl std::error::Error for PrivateSource {}

fn fail(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(PrivateSource),
        })
    })
}

fn ready_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let response = Response::new(StatusCode::IM_A_TEAPOT, Body::from("prepared response"))
            .header(
                "content-type".parse().unwrap(),
                "text/plain".parse().unwrap(),
            )
            .header("x-prepared".parse().unwrap(), "kept".parse().unwrap())
            .with_exception(PrivateSource);
        Err(response.throw_response())
    })
}

fn ready_redirect(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(Redirect::away("/done", Default::default())?
            .with_exception(PrivateSource)
            .throw_response())
    })
}

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    for path in ["/fail", "/custom", "/no-content"] {
        router.get(path, fail)?;
    }
    router.add_route(&[Method::POST], "/post-only", Arc::new(fail))?;
    router.get("/ready", ready_response)?;
    router.get("/ready-override", ready_response)?;
    router.get("/ready-taken", ready_response)?;
    router.get("/ready-redirect", ready_redirect)?;
    Ok(router)
}

fn production_config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([(
        "app".into(),
        json!({"env":"production","debug":false}),
    )]))
}

fn debug_config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([(
        "app".into(),
        json!({"env":"local","debug":true}),
    )]))
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

async fn server(application: Application) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

struct Reply {
    headers: String,
    body: String,
}
impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }
}

async fn send(
    address: SocketAddr,
    method: &'static str,
    path: &'static str,
    accept: &'static str,
    status: u16,
) -> Reply {
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: {accept}\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with(&format!("HTTP/1.1 {status} ")), "{response}");
        Reply { headers: headers.into(), body: body.into() }
    }).await.unwrap()
}

#[tokio::test]
async fn default_renderer_negotiates_and_serves_status_pages_with_headers() {
    let root = Root::new();
    root.page("404.html", "<h1>App missing page</h1>");
    root.page("4xx.html", "<h1>App client error</h1>");
    root.page("5xx.html", "<h1>App server error family</h1>");
    let application = Application::configure(root.0.clone())
        .with_config(production_config)
        .with_routing(Routing::new(routes))
        .create()
        .unwrap();
    let (address, task) = server(application).await;

    let (missing, error) = tokio::join!(
        send(address, "GET", "/missing", "text/html", 404),
        send(address, "GET", "/fail", "application/json", 500),
    );
    assert_eq!(missing.body, "<h1>App missing page</h1>");
    assert_eq!(
        missing.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&error.body).unwrap(),
        json!({"message":"Server Error"})
    );
    let html = send(address, "GET", "/fail", "text/html", 500).await;
    assert!(html.body.contains("<h1>500</h1>"));
    assert!(!html.body.contains("private-render-source"));

    let wrong_method = send(address, "POST", "/fail", "text/html", 405).await;
    assert_eq!(wrong_method.body, "<h1>App client error</h1>");
    assert_eq!(wrong_method.header("allow"), Some("GET, HEAD"));
    let method_head = send(address, "HEAD", "/post-only", "text/html", 405).await;
    assert!(method_head.body.is_empty());
    assert_eq!(method_head.header("allow"), Some("POST"));
    assert_eq!(
        method_head
            .header("content-length")
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        wrong_method.body.len()
    );

    let head = send(address, "HEAD", "/fail", "application/json", 500).await;
    assert!(head.body.is_empty());
    assert_eq!(head.header("content-type"), error.header("content-type"));
    assert_eq!(
        head.header("content-length")
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        error.body.len()
    );

    root.page("500.html", "<h1>App server error</h1>");
    let page = send(address, "GET", "/fail", "text/html", 500).await;
    assert_eq!(page.body, "<h1>App server error</h1>");
    let exact_json = send(address, "GET", "/missing", "application/json", 404).await;
    assert_eq!(
        serde_json::from_str::<Value>(&exact_json.body).unwrap(),
        json!({"message":"Not Found"})
    );

    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn custom_renderers_precede_fallback_and_debug_sources_are_escaped() {
    let root = Root::new();
    root.page("500.html", "<h1>App production page</h1>");
    let application = Application::configure(root.0.clone())
        .with_config(debug_config)
        .with_routing(Routing::new(routes))
        .with_exceptions(|exceptions| {
            exceptions
                .render::<PrivateSource>(|_, _| None)
                .render::<PrivateSource>(|_, request| match request.uri().path() {
                    "/custom" => Some(Response::new(
                        StatusCode::CONFLICT,
                        Body::from("custom-renderer"),
                    )),
                    "/no-content" => Some(Response::empty(StatusCode::NO_CONTENT)),
                    _ => None,
                })
                .should_render_json_when(|request, _| request.expects_json())
                .respond(|mut response, _, _| {
                    response
                        .parts_mut()
                        .headers
                        .insert("x-final", "yes".parse().unwrap());
                    response
                });
        })
        .create()
        .unwrap();
    let (address, task) = server(application).await;
    let (custom, debug) = tokio::join!(
        send(address, "GET", "/custom", "application/json", 409),
        send(address, "GET", "/fail", "application/json", 500),
    );
    assert_eq!(custom.body, "custom-renderer");
    assert_eq!(custom.header("x-final"), Some("yes"));
    assert_eq!(debug.header("x-final"), Some("yes"));
    let payload: Value = serde_json::from_str(&debug.body).unwrap();
    assert_eq!(payload["message"], "<script>private-render-source</script>");
    assert_eq!(
        payload["sources"],
        json!(["<script>private-render-source</script>"])
    );
    let html = send(address, "GET", "/fail", "text/html", 500).await;
    assert!(
        html.body
            .contains("&lt;script&gt;private-render-source&lt;/script&gt;")
    );
    assert!(!html.body.contains("<script>"));
    assert!(!html.body.contains("App production page"));
    let head = send(address, "HEAD", "/custom", "application/json", 409).await;
    assert_eq!(head.header("x-final"), Some("yes"));
    assert_eq!(head.header("content-length"), Some("15"));
    assert!(head.body.is_empty());
    let empty = send(address, "HEAD", "/no-content", "application/json", 204).await;
    assert!(empty.body.is_empty());
    assert_eq!(empty.header("content-length"), None);
    assert_eq!(empty.header("x-final"), Some("yes"));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn prepared_responses_keep_their_payload_and_run_exception_hooks() {
    let root = Root::new();
    let reports = Arc::new(AtomicUsize::new(0));
    let observed = reports.clone();
    let app = Application::configure(root.0.clone())
        .with_config(production_config)
        .with_routing(Routing::new(routes))
        .with_exceptions(move |exceptions| {
            exceptions
                .report::<HttpResponseException>(move |_| {
                    observed.fetch_add(1, Ordering::Relaxed);
                    openrust::foundation::configuration::ReportDecision::Stop
                })
                .render::<HttpResponseException>(|error, request| match request.uri().path() {
                    "/ready-override" => {
                        assert_eq!(error.status(), StatusCode::IM_A_TEAPOT);
                        Some(Response::new(StatusCode::CONFLICT, Body::from("override")))
                    }
                    "/ready-taken" => {
                        let response = error.take_response().unwrap();
                        assert!(error.take_response().is_none());
                        Some(response.header("x-taken".parse().unwrap(), "yes".parse().unwrap()))
                    }
                    _ => None,
                })
                .respond(|mut response, error, _| {
                    assert!(error.downcast_ref::<HttpResponseException>().is_some());
                    if response
                        .exception()
                        .and_then(|e| e.downcast_ref::<PrivateSource>())
                        .is_some()
                    {
                        response
                            .parts_mut()
                            .headers
                            .insert("x-has-exception", "yes".parse().unwrap());
                    }
                    response.header("x-final".parse().unwrap(), "yes".parse().unwrap())
                });
        })
        .create()
        .unwrap();
    let (address, task) = server(app).await;
    let response = send(address, "GET", "/ready", "application/json", 418).await;
    assert_eq!(response.body, "prepared response");
    assert_eq!(response.header("content-type"), Some("text/plain"));
    assert_eq!(response.header("x-prepared"), Some("kept"));
    assert_eq!(response.header("x-has-exception"), Some("yes"));
    assert_eq!(response.header("x-final"), Some("yes"));
    let head = send(address, "HEAD", "/ready", "text/html", 418).await;
    assert!(head.body.is_empty());
    assert_eq!(head.header("content-length"), Some("17"));
    let override_response = send(address, "GET", "/ready-override", "text/html", 409).await;
    assert_eq!(override_response.body, "override");
    assert_eq!(override_response.header("x-final"), Some("yes"));
    assert_eq!(override_response.header("x-has-exception"), None);
    let taken = send(address, "GET", "/ready-taken", "application/json", 418).await;
    assert_eq!(taken.body, "prepared response");
    assert_eq!(taken.header("x-taken"), Some("yes"));
    assert_eq!(taken.header("x-final"), Some("yes"));
    let redirect = send(address, "GET", "/ready-redirect", "application/json", 302).await;
    assert_eq!(redirect.header("location"), Some("/done"));
    assert_eq!(redirect.header("x-has-exception"), Some("yes"));
    assert!(redirect.body.contains("href=\"/done\""));
    assert_eq!(reports.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test]
async fn abort_accepts_native_and_request_aware_responses_over_http() {
    struct RequestResponse;

    impl Responsable for RequestResponse {
        async fn to_response(self, request: &mut Request) -> Result<Response, HttpError> {
            let subject = request.route("subject").unwrap().to_owned();
            if subject == "missing" {
                return Err(HttpError::NotFound);
            }
            tokio::task::yield_now().await;
            Response::json(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({"subject": subject, "query": request.query(Some("source"))?}),
            )
        }
    }

    fn context(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move { Err(abort_responsable(RequestResponse, request).await) })
    }

    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .with_config(production_config)
        .with_routing(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/abort-native", || {
                let native = http::Response::builder()
                    .status(409)
                    .header("content-type", "text/plain")
                    .header("x-native", "kept")
                    .body(Body::from("native payload"))
                    .unwrap();
                Err::<Response, _>(abort_response(native))
            })?;
            router.get("/abort-context/{subject}", context)?;
            Ok(router)
        }))
        .with_exceptions(|exceptions| {
            exceptions.respond(|response, error, _| {
                if error.downcast_ref::<HttpResponseException>().is_some() {
                    response.header("x-prepared".parse().unwrap(), "yes".parse().unwrap())
                } else {
                    response
                }
            });
        })
        .create()
        .unwrap();
    let (address, task) = server(app).await;
    let native = send(address, "GET", "/abort-native", "application/json", 409).await;
    assert_eq!(native.header("x-native"), Some("kept"));
    assert_eq!(native.header("x-prepared"), Some("yes"));
    assert_eq!(native.body, "native payload");
    let response = send(
        address,
        "GET",
        "/abort-context/Ada?source=query",
        "text/html",
        422,
    )
    .await;
    assert_eq!(response.header("x-prepared"), Some("yes"));
    assert_eq!(
        serde_json::from_str::<Value>(&response.body).unwrap(),
        json!({"subject":"Ada","query":"query"})
    );
    let missing = send(
        address,
        "GET",
        "/abort-context/missing",
        "application/json",
        404,
    )
    .await;
    assert_eq!(missing.header("x-prepared"), None);
    assert_eq!(
        serde_json::from_str::<Value>(&missing.body).unwrap(),
        json!({"message":"Not Found"})
    );
    task.abort();
}

#[tokio::test]
async fn collection_dd_responds_to_the_current_request_only() {
    use openrust::support::LazyCollection;

    fn debug_dump(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            match LazyCollection::from(["<diagnostic>"]).dd_with(&[&"request stop"])? {}
        })
    }

    fn alive(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async { Ok(Response::new(StatusCode::OK, Body::from("ready"))) })
    }

    fn debug_routes(_: &Application) -> Result<Router, RoutingError> {
        let mut router = Router::new();
        router.get("/debug", debug_dump)?;
        router.get("/alive", alive)?;
        Ok(router)
    }

    let root = Root::new();
    let application = Application::configure(root.0.clone())
        .with_config(production_config)
        .with_routing(Routing::new(debug_routes))
        .create()
        .unwrap();
    let (address, task) = server(application).await;
    let dump = send(address, "GET", "/debug", "application/json", 500).await;
    assert_eq!(
        dump.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert!(dump.body.contains("<diagnostic>"));
    assert!(dump.body.contains("request stop"));
    let normal = send(address, "GET", "/alive", "text/plain", 200).await;
    assert_eq!(normal.body, "ready");
    task.abort();
}
