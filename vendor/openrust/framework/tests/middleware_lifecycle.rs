use std::{any::TypeId, net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::{Kernel as _, Middleware, TerminableMiddleware},
    foundation::{Application, BootstrapError, configuration::Routing, http::Kernel},
    http::{
        Body, HttpError, HttpFuture, Next, Request, RequestParts, Response, ResponseParts,
        StatusCode,
    },
    routing::{MiddlewareEntry, RoutingError},
    support::facades::Route,
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::{Notify, mpsc},
    time::timeout,
};

const TIMEOUT: Duration = Duration::from_secs(5);

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(incoming).await
}

async fn listen(kernel: Arc<Kernel>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(kernel).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server)
}

async fn send(address: SocketAddr, request: &str) -> String {
    timeout(TIMEOUT, async {
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        response
    })
    .await
    .unwrap()
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/data", data)?;
    Ok(())
}

fn data(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Ok(Response::new(StatusCode::OK, Body::from("data")).header(
            ::http::header::VARY,
            ::http::HeaderValue::from_static("Accept-Encoding, Origin"),
        ))
    })
}

#[tokio::test]
async fn cors_uses_host_paths_and_wraps_real_http_errors() {
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "cors".into(),
                json!({
                    "paths": {"0": "api/*", "restricted.test": ["/private/*"]},
                    "allowed_origins": ["https://client.test", "https://*.apps.test"],
                    "allowed_origins_patterns": ["#^https://preview[0-9]+\\.test$#"],
                    "allowed_methods": ["POST", "GET"],
                    "allowed_headers": ["*"],
                    "exposed_headers": ["X-Trace"],
                    "max_age": 600,
                    "supports_credentials": true
                }),
            )]
            .into())
        })
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|request, _| request.expectsJson());
        })
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let (address, server) = listen(Arc::new(kernel)).await;
    let preflight = send(address, "OPTIONS /api/missing HTTP/1.1\r\nHost: app.test\r\nOrigin: https://nested.dev.apps.test\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: X-Token\r\nConnection: close\r\n\r\n").await;
    assert!(preflight.starts_with("HTTP/1.1 204 "));
    assert!(preflight.ends_with("\r\n\r\n"));
    for header in [
        "access-control-allow-origin: https://nested.dev.apps.test",
        "access-control-allow-credentials: true",
        "access-control-allow-methods: POST, GET",
        "access-control-allow-headers: X-Token",
        "access-control-max-age: 600",
        "vary: access-control-request-method",
        "vary: access-control-request-headers",
    ] {
        assert!(preflight.contains(header), "{preflight}");
    }
    let denied = send(address, "OPTIONS /api/missing HTTP/1.1\r\nHost: app.test\r\nOrigin: https://apps.test.evil.test\r\nAccess-Control-Request-Method: POST\r\nConnection: close\r\n\r\n").await;
    assert!(denied.starts_with("HTTP/1.1 204 "));
    assert!(!denied.contains("access-control-allow-"));
    let error = send(address, "GET /api/missing HTTP/1.1\r\nHost: app.test\r\nAccept: application/json\r\nOrigin: https://preview42.test\r\nConnection: close\r\n\r\n").await;
    assert!(error.starts_with("HTTP/1.1 404 "));
    assert!(error.contains("access-control-allow-origin: https://preview42.test"));
    assert!(error.contains("access-control-expose-headers: X-Trace"));
    assert!(error.ends_with(r#"{"message":"Not Found"}"#));
    let plain_options = send(address, "OPTIONS /api/data HTTP/1.1\r\nHost: app.test\r\nOrigin: https://client.test\r\nConnection: close\r\n\r\n").await;
    assert!(
        !plain_options.starts_with("HTTP/1.1 204 "),
        "{plain_options}"
    );
    assert!(plain_options.contains("vary: access-control-request-method"));
    let existing = send(address, "GET /api/data HTTP/1.1\r\nHost: app.test\r\nOrigin: https://client.test\r\nConnection: close\r\n\r\n").await;
    assert!(existing.starts_with("HTTP/1.1 200 "));
    assert_eq!(existing.matches("\r\nvary: ").count(), 1, "{existing}");
    assert!(existing.contains("vary: Accept-Encoding, Origin"));
    let other_host = send(address, "GET /api/data HTTP/1.1\r\nHost: restricted.test:9000\r\nOrigin: https://client.test\r\nConnection: close\r\n\r\n").await;
    assert!(other_host.starts_with("HTTP/1.1 200 "));
    assert!(!other_host.contains("access-control-allow-origin"));
    let host_match = send(address, "OPTIONS /private/endpoint HTTP/1.1\r\nHost: restricted.test:9000\r\nOrigin: https://client.test\r\nAccess-Control-Request-Method: POST\r\nConnection: close\r\n\r\n").await;
    assert!(host_match.starts_with("HTTP/1.1 204 "));
    assert!(host_match.contains("access-control-allow-origin: https://client.test"));

    let disabled = Application::configure(std::env::temp_dir())
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    let mut disabled = Kernel::new(disabled);
    disabled.bootstrap().await.unwrap();
    let (disabled_address, disabled_server) = listen(Arc::new(disabled)).await;
    let without_cors = send(disabled_address, "GET /api/data HTTP/1.1\r\nHost: app.test\r\nOrigin: https://client.test\r\nConnection: close\r\n\r\n").await;
    assert!(without_cors.starts_with("HTTP/1.1 200 "));
    assert!(!without_cors.contains("access-control-allow-origin"));
    disabled_server.abort();
    server.abort();
}

#[tokio::test]
async fn invalid_cors_configuration_fails_consistently_on_bootstrap_retry() {
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "cors".into(),
                json!({
                    "paths": ["api/*"],
                    "allowed_origins_patterns": ["["]
                }),
            )]
            .into())
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    for _ in 0..2 {
        let error = kernel.bootstrap().await.unwrap_err();
        assert!(matches!(error, BootstrapError::ConfigurationFailed { .. }));
        assert!(error.to_string().contains("cors.allowed_origins_patterns"));
    }
}

type Event = (&'static str, &'static str);

struct Record<const ID: u8> {
    name: &'static str,
    events: mpsc::UnboundedSender<Event>,
    gate: Option<Arc<Notify>>,
}

impl<const ID: u8> Middleware for Record<ID> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            self.events.send(("handle", self.name)).unwrap();
            let response = next.run(request).await;
            if let Some(gate) = self
                .gate
                .clone()
                .filter(|_| request.uri().path() == "/api/data")
            {
                let first = futures_util::stream::once(async { Ok::<_, std::io::Error>("first") });
                let last = futures_util::stream::once(async move {
                    gate.notified().await;
                    Ok::<_, std::io::Error>("last")
                });
                use futures_util::StreamExt as _;
                return Ok(Response::stream(response.status(), first.chain(last)));
            }
            Ok(response)
        })
    }
}

impl<const ID: u8> TerminableMiddleware for Record<ID> {
    fn terminate<'a>(
        &'a self,
        request: &'a RequestParts,
        response: &'a ResponseParts,
    ) -> HttpFuture<'a, ()> {
        Box::pin(async move {
            assert!(request.uri.path().starts_with("/api/"));
            assert!(matches!(
                response.status,
                StatusCode::OK | StatusCode::NOT_FOUND
            ));
            self.events.send(("terminate", self.name)).unwrap();
        })
    }
}

async fn receive(events: &mut mpsc::UnboundedReceiver<Event>, expected: Event) {
    assert_eq!(
        timeout(TIMEOUT, events.recv()).await.unwrap(),
        Some(expected)
    );
}

fn lifecycle_routes() -> Result<(), RoutingError> {
    Route::getWith("/data", data, |route| {
        route.middleware_name("ordered");
        Ok(())
    })?;
    Route::getWith("/excluded", data, |route| {
        route.middleware_name("ordered").withoutMiddleware("later");
        Ok(())
    })
}

#[tokio::test]
async fn termination_waits_for_the_stream_and_follows_resolved_route_order() {
    let gate = Arc::new(Notify::new());
    let (events, mut received) = mpsc::unbounded_channel();
    let first = Arc::new(Record::<1> {
        name: "first",
        events: events.clone(),
        gate: None,
    });
    let later = Arc::new(Record::<2> {
        name: "later",
        events: events.clone(),
        gate: None,
    });
    let global = Arc::new(Record::<3> {
        name: "global",
        events,
        gate: Some(gate.clone()),
    });
    let app = Application::configure(std::env::temp_dir())
        .withMiddleware(move |middleware| {
            middleware
                .appendTerminable(global)
                .aliasTerminable("later", later.clone())
                .group(
                    "ordered",
                    [
                        MiddlewareEntry::terminable(later),
                        MiddlewareEntry::named("later"),
                        MiddlewareEntry::terminable(first),
                    ],
                )
                .priority([TypeId::of::<Record<1>>(), TypeId::of::<Record<2>>()]);
        })
        .withRouting(Routing::default().api(lifecycle_routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    let (address, server) = listen(kernel.clone()).await;
    let mut client = TcpStream::connect(address).await.unwrap();
    client
        .write_all(b"GET /api/data HTTP/1.1\r\nHost: app.test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut received_bytes = Vec::new();
    timeout(TIMEOUT, async {
        while !received_bytes.windows(5).any(|window| window == b"first") {
            assert_ne!(client.read_buf(&mut received_bytes).await.unwrap(), 0);
        }
    })
    .await
    .unwrap();
    for name in ["global", "first", "later"] {
        receive(&mut received, ("handle", name)).await;
    }
    assert!(received.try_recv().is_err(), "terminated during streaming");

    let excluded = send(
        address,
        "GET /api/excluded HTTP/1.1\r\nHost: app.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(excluded.starts_with("HTTP/1.1 200 "));
    for event in [
        ("handle", "global"),
        ("handle", "first"),
        ("terminate", "first"),
        ("terminate", "global"),
    ] {
        receive(&mut received, event).await;
    }
    gate.notify_one();
    timeout(TIMEOUT, client.read_to_end(&mut received_bytes))
        .await
        .unwrap()
        .unwrap();
    assert!(received_bytes.windows(4).any(|window| window == b"last"));
    for name in ["first", "later", "global"] {
        receive(&mut received, ("terminate", name)).await;
    }

    let mut cancelled = TcpStream::connect(address).await.unwrap();
    cancelled
        .write_all(b"GET /api/data HTTP/1.1\r\nHost: app.test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut bytes = Vec::new();
    timeout(TIMEOUT, async {
        while !bytes.windows(5).any(|window| window == b"first") {
            assert_ne!(cancelled.read_buf(&mut bytes).await.unwrap(), 0);
        }
    })
    .await
    .unwrap();
    for name in ["global", "first", "later"] {
        receive(&mut received, ("handle", name)).await;
    }
    drop(cancelled);
    for name in ["first", "later", "global"] {
        receive(&mut received, ("terminate", name)).await;
    }

    let (parts, body) = ::http::Request::builder()
        .uri("/api/absent")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let mut request = Request::from_parts(parts, body);
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    receive(&mut received, ("handle", "global")).await;
    kernel.terminate(request.parts(), response.parts()).await;
    kernel.terminate(request.parts(), response.parts()).await;
    receive(&mut received, ("terminate", "global")).await;
    assert!(received.try_recv().is_err(), "duplicate termination");
    server.abort();
}
