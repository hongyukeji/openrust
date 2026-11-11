use std::{
    collections::BTreeMap,
    io::{Read as _, Write as _},
    net::{SocketAddr, TcpStream},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{ApiResponse, PageMeta, Request, StatusCode},
    support::{Collection, facades::Route},
};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};

struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("private-serialization-sentinel"))
    }
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

async fn send(address: SocketAddr, path: &'static str, status: u16) -> Value {
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with(&format!("HTTP/1.1 {status} ")), "{response}");
        assert!(!body.contains("private-serialization-sentinel"));
        if status == 204 {
            assert!(!headers.contains("content-type:"));
            assert!(body.is_empty());
            return Value::Null;
        }
        assert!(headers.contains("\r\ncontent-type: application/json"));
        serde_json::from_str(body).unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn opt_in_json_responses_use_semantic_status_and_default_exception_handling() {
    let application = Application::configure(std::env::temp_dir())
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|request, _| request.expectsJson());
        })
        .withRouting(Routing::default().health("/up").web(|| {
            Route::get("/created", || {
                ApiResponse::success(json!({"id": 1})).with_status(StatusCode::CREATED)
            })?;
            Route::get("/items", || {
                ApiResponse::success([1, 2]).with_meta(PageMeta {
                    page: 2,
                    per_page: 2,
                    total: 4,
                    total_pages: 2,
                })
            })?;
            Route::get("/invalid", || {
                ApiResponse::failure(StatusCode::UNPROCESSABLE_ENTITY, "invalid", "Invalid input")
                    .with_errors(BTreeMap::from([("name".into(), vec!["Required".into()])]))
            })?;
            Route::get("/empty", ApiResponse::empty)?;
            Route::get("/no-content", || {
                ApiResponse::empty().with_status(StatusCode::NO_CONTENT)
            })?;
            Route::get("/collection/keyed", || {
                ApiResponse::success(Collection::fromEntries([(2, 10), (4, 20)]))
            })?;
            Route::get("/collection/values", || {
                ApiResponse::success(Collection::fromEntries([(2, 10), (4, 20)]).values())
            })?;
            Route::get("/collection/nonfinite", || {
                ApiResponse::success(Collection::make([f64::INFINITY]))
            })?;
            Route::get("/unserializable", || ApiResponse::success(Unserializable))
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

    assert_eq!(
        send(address, "/created", 201).await,
        json!({"success": true, "code": "ok", "message": "ok", "data": {"id": 1}})
    );
    assert_eq!(
        send(address, "/items", 200).await,
        json!({
            "success": true, "code": "ok", "message": "ok", "data": [1, 2],
            "meta": {"page": 2, "per_page": 2, "total": 4, "total_pages": 2}
        })
    );
    assert_eq!(
        send(address, "/invalid", 422).await,
        json!({
            "success": false, "code": "invalid", "message": "Invalid input",
            "errors": {"name": ["Required"]}
        })
    );
    assert_eq!(
        send(address, "/empty", 200).await,
        json!({"success": true, "code": "ok", "message": "ok"})
    );
    assert_eq!(send(address, "/no-content", 204).await, Value::Null);
    assert_eq!(
        send(address, "/unserializable", 500).await,
        json!({"message": "Server Error"})
    );
    assert_eq!(
        send(address, "/missing", 404).await,
        json!({"message": "Not Found"})
    );
    assert_eq!(
        send(address, "/collection/keyed", 200).await["data"],
        json!({"2": 10, "4": 20})
    );
    assert_eq!(
        send(address, "/collection/values", 200).await["data"],
        json!([10, 20])
    );
    assert_eq!(
        send(address, "/collection/nonfinite", 500).await,
        json!({"message": "Server Error"})
    );
    assert_eq!(send(address, "/up", 200).await, json!({"status": "up"}));

    server.abort();
    let _ = server.await;
}
