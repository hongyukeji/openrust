use std::{net::SocketAddr, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::Route,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn report(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let tenant = request.route("tenant").unwrap_or("public");
    let id = request.route("id").unwrap_or("unset");
    let response = Response::new(StatusCode::OK, Body::from(format!("{tenant}:{id}")));
    Box::pin(async move { Ok(response) })
}

fn register() -> Result<(), RoutingError> {
    Route::group(
        RouteGroup::new()
            .domain("{tenant}.example.test")
            .prefix("admin")
            .name("admin.")
            .r#where("id", "[0-9]+")?,
        |router| {
            router.group(RouteGroup::new().name("reports."), |router| {
                router
                    .get("/reports/{id?}", report)?
                    .name("show")
                    .defaults("id", "7");
                router
                    .r#match(&[Method::POST], "/reports/{id?}", report)?
                    .name("update");
                Ok(())
            })
        },
    )?;
    Route::group(
        RouteGroup::new().domain("other.test").prefix("admin"),
        |router| {
            router.get("/reports/{id?}", report)?.defaults("id", "99");
            Ok(())
        },
    )?;
    Route::get("/outside/{id}", report)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let (parts, body) = kernel
        .handle(&mut Request::from_parts(parts, body))
        .await
        .into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn send(
    address: SocketAddr,
    method: &str,
    host: &str,
    path: &str,
    status: u16,
) -> (String, String) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(
        headers.starts_with(&format!("HTTP/1.1 {status} ")),
        "{response}"
    );
    (headers.to_owned(), body.to_owned())
}

#[tokio::test]
async fn route_groups_match_domains_constraints_and_http_methods() {
    let app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::default().web(register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let host = "demo.example.test";
    let path = "/admin/reports";
    assert_eq!(send(address, "GET", host, path, 200).await.1, "demo:7");
    assert_eq!(
        send(address, "GET", host, "/admin/reports/42", 200).await.1,
        "demo:42"
    );
    assert_eq!(send(address, "POST", host, path, 200).await.1, "demo:unset");
    let (head, body) = send(address, "HEAD", host, path, 200).await;
    assert!(body.is_empty());
    assert!(head.contains("\r\ncontent-length: 6"));
    let (options, body) = send(address, "OPTIONS", host, path, 200).await;
    assert!(body.is_empty());
    assert!(options.contains("\r\nallow: GET,HEAD,POST"));
    let (method, _) = send(address, "DELETE", host, path, 405).await;
    assert!(method.contains("\r\nallow: GET, HEAD, POST"));
    send(address, "GET", host, "/admin/reports/invalid", 404).await;
    send(address, "GET", "example.test", path, 404).await;
    send(address, "OPTIONS", "example.test", path, 404).await;
    assert_eq!(
        send(address, "GET", "other.test", path, 200).await.1,
        "public:99"
    );
    let (only_get, _) = send(address, "OPTIONS", "other.test", path, 200).await;
    assert!(only_get.contains("\r\nallow: GET,HEAD"));
    assert_eq!(
        send(address, "GET", "example.test", "/outside/letters", 200)
            .await
            .1,
        "public:letters"
    );
    server.abort();
}
