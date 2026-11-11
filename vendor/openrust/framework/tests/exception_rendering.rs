use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::{http::Kernel as _, routing::Registrar as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::{Router, RoutingError},
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

fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    for path in ["/fail", "/custom", "/no-content"] {
        router.get(path, fail)?;
    }
    router.add_route(&[Method::POST], "/post-only", Arc::new(fail))?;
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
        .withConfig(production_config)
        .withRouting(Routing::new(routes))
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
    assert!(html.body.contains("<h1>500 Internal Server Error</h1>"));
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
        .withConfig(debug_config)
        .withRouting(Routing::new(routes))
        .withExceptions(|exceptions| {
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
                .shouldRenderJsonWhen(|request, _| request.expectsJson())
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
