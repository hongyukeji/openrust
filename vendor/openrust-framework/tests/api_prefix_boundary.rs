use std::{collections::BTreeMap, path::Path, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::{ConfigError, Environment},
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response},
    support::facades::Route,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn configuration(_: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        ("app".into(), json!({"locale":"en","debug":false})),
        (
            "api".into(),
            json!({
                "prefix": env.get_utf8("API_PREFIX")?.unwrap_or("api"),
            }),
        ),
    ]))
}

fn unauthenticated(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Err(HttpError::Unauthenticated) })
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

async fn send(address: std::net::SocketAddr, path: &str, accept: &str) -> (String, String) {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!(
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: {accept}\r\nConnection: close\r\n\r\n"
    ).as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    (headers.to_owned(), body.to_owned())
}

#[tokio::test]
async fn exact_api_prefix_and_children_render_api_results() {
    for prefix in ["api", "api/admin"] {
        for enabled in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "api-prefix-boundary-{}-{}-{enabled}",
                std::process::id(),
                prefix.replace('/', "-")
            ));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                root.join(".env"),
                format!("API_PREFIX={prefix}\nAPI_RESPONSE_ENVELOPE={enabled}\n"),
            )
            .unwrap();
            let application = Application::configure(root.clone())
                .with_config(configuration)
                .with_exceptions(|exceptions| {
                    exceptions.should_render_json_when(|request, _| {
                        request.is_api_path() || request.expects_json()
                    });
                })
                .with_routing(Routing::default().api(|| {
                    Route::get("/", unauthenticated)?;
                    Route::get("/child", unauthenticated)?;
                    Ok(())
                }))
                .create()
                .unwrap();
            let mut kernel = Kernel::new(application);
            kernel.bootstrap().await.unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let service = transport.with_state(Arc::new(kernel)).into_make_service();
            let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

            for path in [format!("/{prefix}"), format!("/{prefix}/child")] {
                let (headers, body) = send(address, &path, "text/html").await;
                assert!(headers.starts_with("HTTP/1.1 401 "), "{headers}");
                assert!(
                    headers.contains("content-type: application/json"),
                    "{headers}"
                );
                let value: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(
                    value,
                    json!({"success":false,"code":"unauthenticated","message":"Unauthenticated."})
                );
            }
            for path in [format!("/{prefix}/missing"), format!("/{prefix}?query=1")] {
                let (headers, body) = send(address, &path, "text/html").await;
                assert!(headers.contains("content-type: application/json"));
                let value: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(value["success"], false);
            }
            for path in [format!("/{prefix}x"), format!("/other/{prefix}")] {
                let (headers, _) = send(address, &path, "text/html").await;
                assert!(headers.starts_with("HTTP/1.1 404 "));
                assert!(headers.contains("content-type: text/html"), "{headers}");
            }
            let (headers, body) = send(address, "/outside", "application/json").await;
            assert!(headers.contains("content-type: application/json"));
            let value: Value = serde_json::from_str(&body).unwrap();
            assert!(value.get("success").is_none());
            server.abort();
            let _ = server.await;
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[tokio::test]
async fn empty_api_prefix_does_not_classify_all_paths_as_api() {
    let root = std::env::temp_dir().join(format!("api-prefix-empty-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(".env"),
        "API_PREFIX=\nAPI_RESPONSE_ENVELOPE=true\n",
    )
    .unwrap();
    let application = Application::configure(root.clone())
        .with_config(configuration)
        .with_exceptions(|exceptions| {
            exceptions.should_render_json_when(|request, _| {
                request.is_api_path() || request.expects_json()
            });
        })
        .with_routing(Routing::default().api(|| {
            Route::get("/child", unauthenticated)?;
            Ok(())
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    for path in ["/", "/api", "/api/child"] {
        let (headers, _) = send(address, path, "text/html").await;
        assert!(headers.contains("content-type: text/html"));
    }
    let (headers, body) = send(address, "/child", "text/html").await;
    assert!(headers.starts_with("HTTP/1.1 401"));
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"success":false,"code":"unauthenticated","message":"Unauthenticated."})
    );
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}
