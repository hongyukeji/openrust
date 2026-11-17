use std::{net::SocketAddr, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        HttpError, HttpFuture, Request, RequestSecurity, Response, StatusCode, TrustedProxyHeaders,
    },
    routing::Router,
    support::facades::URL,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn proxy_info(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let current = URL::current().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Response::json(
            StatusCode::OK,
            json!({
                "secure": request.secure(),
                "ips": request.ips(),
                "url": current,
                "path": request.uri().path(),
                "base": request.getBaseUrl(),
                "original_host": request.headers().get("host").and_then(|value| value.to_str().ok()),
            }),
        )
    })
}

async fn entry(
    State(kernel): State<Arc<Kernel>>,
    mut request: axum::extract::Request,
) -> axum::response::Response {
    request.extensions_mut().insert(RequestSecurity(false));
    kernel.handleTransport(request).await
}

async fn start(
    at: &'static [&'static str],
    headers: u8,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withMiddleware(move |middleware| {
            middleware.trustProxies(at, headers).unwrap();
        })
        .withRouting(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/probe", proxy_info)?;
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = entry
        .with_state(Arc::new(kernel))
        .into_make_service_with_connect_info::<SocketAddr>();
    (
        address,
        tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }),
    )
}

async fn request(address: SocketAddr, uri: &str, fields: &str, status: u16) -> Value {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!(
        "GET {uri} HTTP/1.1\r\nHost: backend.test\r\nAccept: application/json\r\nConnection: close\r\n{fields}\r\n",
    ).as_bytes()).await.unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.unwrap();
    let (headers, body) = reply.split_once("\r\n\r\n").unwrap();
    assert!(
        headers.starts_with(&format!("HTTP/1.1 {status} ")),
        "{reply}"
    );
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
async fn uses_trusted_proxy_metadata_without_changing_the_route_path() {
    let (address, server) =
        start(&["127.0.0.0/8", "10.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let data = request(
        address,
        "/probe?keep=1",
        concat!(
            "X-Forwarded-For: 203.0.113.7\r\nX-Forwarded-For: 10.0.0.8\r\n",
            "X-Forwarded-Proto: https\r\nX-Forwarded-Host: edge.test\r\n",
            "X-Forwarded-Port: 8443\r\nX-Forwarded-Prefix: /portal/\r\n",
        ),
        200,
    )
    .await;
    assert_eq!(
        data,
        json!({
            "secure": true, "ips": ["203.0.113.7"], "url": "https://edge.test:8443/portal/probe",
            "path": "/probe", "base": "/portal", "original_host": "backend.test",
        })
    );
    server.abort();
}

#[tokio::test]
async fn requires_a_trusted_peer_and_rejects_conflicting_forwarding_fields() {
    let (untrusted, first) = start(&["10.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let data = request(
        untrusted,
        "https://backend.test/probe",
        concat!(
            "X-Forwarded-Proto: https\r\nX-Forwarded-For: 203.0.113.7\r\n",
            "X-Forwarded-Host: edge.test\r\n",
        ),
        200,
    )
    .await;
    assert_eq!(data["secure"], false);
    assert_eq!(data["ips"], json!(["127.0.0.1"]));
    assert_eq!(data["url"], "http://backend.test/probe");
    first.abort();

    let (trusted, second) = start(
        &["REMOTE_ADDR"],
        TrustedProxyHeaders::FORWARDED | TrustedProxyHeaders::DEFAULT,
    )
    .await;
    let data = request(
        trusted,
        "/probe",
        "Forwarded: for=\"[2001:db8::7]:1234\";proto=https;host=\"edge.test:444\"\r\n",
        200,
    )
    .await;
    assert_eq!(data["url"], "https://edge.test:444/probe");
    assert_eq!(data["ips"], json!(["2001:db8::7"]));
    request(
        trusted,
        "/probe",
        "Forwarded: proto=https;host=edge.test\r\nX-Forwarded-Proto: http\r\n",
        400,
    )
    .await;
    second.abort();
}
