use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Next, Request as HttpRequest, Response, StatusCode},
    routing::{Router, RoutingError},
    support::facades::Request,
    validation::RuleSet,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
struct ValidateBeforeDispatch(Arc<AtomicUsize>);
impl openrust::contracts::http::Middleware for ValidateBeforeDispatch {
    fn handle<'a>(
        &'a self,
        request: &'a mut HttpRequest,
        _next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.is_attempting_precognition() {
                assert!(request.is_precognitive());
            }
            let rules = RuleSet::parse([("name", "required|string"), ("email", "required|email")])?;
            let named_bag = request.headers().contains_key("x-named-bag");
            let validated = request
                .with_container(async {
                    if named_bag {
                        Request::validate_with_bag("profile", rules).await
                    } else {
                        Request::validate(rules).await
                    }
                })
                .await?;
            self.0.fetch_add(1, Ordering::SeqCst);
            Response::json(StatusCode::OK, validated.into_value())
        })
    }
}
async fn start(
    calls: Arc<AtomicUsize>,
) -> (
    tempfile::TempDir,
    std::net::SocketAddr,
    tokio::task::JoinHandle<()>,
) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"locale":"en","supported_locales":[]}),
            )]))
        })
        .with_routing(Routing::new(|app| {
            let mut router = Router::new();
            let calls = app.container().make::<AtomicUsize>().unwrap();
            router
                .post("/validate", || Response::empty(StatusCode::OK))?
                .middleware_name("precognitive")
                .middleware(Arc::new(ValidateBeforeDispatch(calls)));
            Ok::<_, RoutingError>(router)
        }))
        .create()
        .unwrap();
    app.container().instance(calls).unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (root, address, server)
}
async fn post(address: std::net::SocketAddr, body: &str, headers: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("POST /validate HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();response
    }).await.unwrap()
}

#[tokio::test]
async fn request_validation_selects_precognitive_fields_and_stops_before_side_effects() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (_root, address, server) = start(calls.clone()).await;
    for named in ["", "X-Named-Bag: profile\r\n"] {
        let headers = format!("Precognition: true\r\nPrecognition-Validate-Only: name\r\n{named}");
        let response = post(address, r#"{"name":"Ada"}"#, &headers).await;
        assert!(response.starts_with("HTTP/1.1 204 "), "{response}");
        assert!(
            response.contains("precognition-success: true"),
            "{response}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let response = post(address, r#"{"name":""}"#, &headers).await;
        assert!(response.starts_with("HTTP/1.1 422 "), "{response}");
        let value: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(value["errors"]["name"].is_array());
        assert!(value["errors"].get("email").is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn ordinary_request_validation_still_validates_all_fields() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (_root, address, server) = start(calls.clone()).await;
    let response = post(address, r#"{"name":"Ada"}"#, "").await;
    assert!(response.starts_with("HTTP/1.1 422 "), "{response}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let response = post(
        address,
        r#"{"name":"Ada","email":"ada@example.test","admin":true}"#,
        "",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    let value: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(value, json!({"name":"Ada","email":"ada@example.test"}));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = server.await;
}
