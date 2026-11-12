use std::{
    collections::BTreeMap,
    io::{Read as _, Write as _},
    net::{SocketAddr, TcpStream},
    path::Path,
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::{ConfigError, Environment},
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    database::{
        DatabaseManager, DatabaseResult, Value as DbValue,
        eloquent::{Attributes, Model, ModelMetadata, ModelState},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{ApiResponse, HttpFuture, Method, PageMeta, Request, StatusCode},
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
    send_method(address, "GET", path, status).await
}

async fn send_method(
    address: SocketAddr,
    method: &'static str,
    path: &'static str,
    status: u16,
) -> Value {
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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

struct RouteRecord(ModelState);

impl Model for RouteRecord {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("route_records")
            .timestamps(None, None)
            .fillable(&["name", "private"])
            .hidden(&["private"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

fn model_config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([(
        "database".into(),
        json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":":memory:"}}}),
    )]))
}

fn model_from_request(request: &mut Request) -> HttpFuture<'_, DatabaseResult<RouteRecord>> {
    Box::pin(async move {
        let id = if request.decodedPath() == "borrowed" {
            1
        } else {
            999
        };
        RouteRecord::findOrFail(id).await
    })
}

#[tokio::test]
async fn routes_return_models_and_collections_as_json_over_http() {
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(model_config)
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|request, _| request.expectsJson());
        })
        .withRouting(Routing::default().web(|| {
            Route::get("/model", || RouteRecord::findOrFail(1))?;
            Route::get("/models", RouteRecord::all)?;
            Route::get("/borrowed", model_from_request)?;
            Route::get("/missing-model", || RouteRecord::findOrFail(999))?;
            Route::get("/invalid-model", || {
                let mut record = RouteRecord::newInstance(Attributes::new()).unwrap();
                record.setAttribute("bad", DbValue::Bytes(vec![0xff]));
                record
            })?;
            Route::r#match(&[Method::POST], "/model", || {
                RouteRecord::create(Attributes::from([
                    ("name".into(), "example".into()),
                    ("private".into(), "private-serialization-sentinel".into()),
                ]))
            })
        }))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    db.statement(
        "CREATE TABLE route_records (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, private TEXT)",
        &[],
    ).await.unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

    assert_eq!(send(address, "/models", 200).await, json!([]));
    let expected = json!({"id": 1, "name": "example"});
    assert_eq!(send_method(address, "POST", "/model", 201).await, expected);
    let (read, borrowed, collection) = tokio::join!(
        send(address, "/model", 200),
        send(address, "/borrowed", 200),
        send(address, "/models", 200),
    );
    assert_eq!(read, expected);
    assert_eq!(borrowed, expected);
    assert_eq!(collection, json!([expected]));
    assert_eq!(
        send(address, "/missing-model", 404).await,
        json!({"message":"Not Found"})
    );
    assert_eq!(
        send(address, "/invalid-model", 500).await,
        json!({"message":"Server Error"})
    );
    assert_eq!(send(address, "/model", 200).await, expected);
    server.abort();
    let _ = server.await;
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
