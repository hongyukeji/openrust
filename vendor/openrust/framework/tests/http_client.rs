use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::State,
    response::IntoResponse,
    routing::{any, get},
};
use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use openrust::http::client::{Error, Factory, Part, Response};
use serde_json::{Value, json};

#[tokio::test]
async fn sends_real_requests_and_classifies_responses() {
    async fn echo(request: axum::extract::Request) -> impl IntoResponse {
        let (parts, body) = request.into_parts();
        let body = axum::body::to_bytes(body, 65536).await.unwrap();
        let method = parts.method;
        let uri = parts.uri;
        let headers = parts.headers;
        let value = json!({
            "method": method.as_str(),
            "query": uri.query(),
            "type": headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            "accept": headers.get(header::ACCEPT).and_then(|value| value.to_str().ok()),
            "authorization": headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()),
            "body": String::from_utf8_lossy(&body)
        });
        (
            [(header::CONTENT_TYPE, "application/json")],
            value.to_string(),
        )
    }
    let service = Router::new()
        .route("/api/echo", any(echo))
        .route(
            "/missing",
            get(|| async { (StatusCode::NOT_FOUND, "missing") }),
        )
        .route(
            "/redirect",
            get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/missing")]) }),
        );
    let (origin, task) = serve(service).await;
    let factory = Factory::new().unwrap();
    let response = factory
        .pendingRequest()
        .baseUrl(format!("{origin}/api"))
        .acceptJson()
        .withToken("example")
        .withQueryParameters([("q".into(), "有 & space".into())])
        .post("/echo?existing=yes", &json!({"name": "Rust"}))
        .await
        .unwrap();
    assert!(response.ok());
    assert_eq!(response.header("content-type").unwrap(), "application/json");
    let body = response.json::<Value>().unwrap();
    assert_eq!(body["query"], "q=%E6%9C%89%20%26%20space");
    assert_eq!(body["method"], "POST");
    assert_eq!(body["accept"], "application/json");
    assert_eq!(body["authorization"], "Bearer example");
    assert_eq!(body["type"], "application/json");
    assert_eq!(
        serde_json::from_str::<Value>(body["body"].as_str().unwrap()).unwrap(),
        json!({"name": "Rust"})
    );

    let url = format!("{origin}/api/echo");
    let form = factory
        .pendingRequest()
        .asForm()
        .withBasicAuth("foo", "bar")
        .put(&url, &[("title", "a b")])
        .await
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(form["body"], "title=a+b");
    assert_eq!(form["authorization"], "Basic Zm9vOmJhcg==");
    assert_eq!(form["query"], Value::Null);
    let patched = factory
        .pendingRequest()
        .beforeSending(|request| {
            request
                .headers_mut()
                .insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
            Ok(())
        })
        .afterResponse(|response| {
            assert!(response.ok());
            Response::from_json(StatusCode::CREATED, &response.json::<Value>()?)
        })
        .get(&url)
        .await
        .unwrap();
    assert!(patched.created());
    assert_eq!(patched.json::<Value>().unwrap()["accept"], "text/plain");
    let raw = factory
        .pendingRequest()
        .withBody("plain", HeaderValue::from_static("text/plain"))
        .send(Method::PATCH, &url)
        .await
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(raw["body"], "plain");
    let multipart = factory
        .pendingRequest()
        .attach("file", Part::text("body").file_name("note.txt"))
        .send(Method::POST, &url)
        .await
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert!(
        multipart["type"]
            .as_str()
            .unwrap()
            .starts_with("multipart/form-data;")
    );
    assert!(
        multipart["body"]
            .as_str()
            .unwrap()
            .contains("filename=\"note.txt\"")
    );

    let redirect = format!("{origin}/redirect");
    assert!(
        factory
            .pendingRequest()
            .withoutRedirecting()
            .get(&redirect)
            .await
            .unwrap()
            .found()
    );
    let missing = factory.get(&redirect).await.unwrap();
    assert!(missing.notFound());
    assert!(
        matches!(missing.throw(), Err(Error::RequestFailed(response)) if response.body() == b"missing")
    );
    task.abort();
}

#[tokio::test]
async fn retries_status_and_times_out_real_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Router::new()
        .route(
            "/retry",
            get(|State(calls): State<Arc<AtomicUsize>>| async move {
                if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::OK
                }
            }),
        )
        .route("/failed", get(|| async { StatusCode::BAD_GATEWAY }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(500)).await;
                "too late"
            }),
        )
        .route("/large", get(|| async { "123456789" }))
        .with_state(calls.clone());
    let (origin, task) = serve(service).await;
    let factory = Factory::new().unwrap();
    assert!(
        factory
            .pendingRequest()
            .retry(3, Duration::ZERO)
            .get(&format!("{origin}/retry"))
            .await
            .unwrap()
            .ok()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(matches!(
        factory.pendingRequest().retry(2, Duration::ZERO)
            .get(&format!("{origin}/failed")).await,
        Err(Error::RequestFailed(response)) if response.serverError()
    ));
    assert!(
        factory
            .pendingRequest()
            .retry(2, Duration::ZERO)
            .throwOnRetryFailure(false)
            .get(&format!("{origin}/failed"))
            .await
            .unwrap()
            .serverError()
    );
    assert!(matches!(
        factory
            .pendingRequest()
            .timeout(Duration::from_millis(25))
            .get(&format!("{origin}/slow"))
            .await,
        Err(Error::Timeout)
    ));
    assert!(matches!(
        factory
            .pendingRequest()
            .maxResponseBytes(8)
            .get(&format!("{origin}/large"))
            .await,
        Err(Error::BodyTooLarge)
    ));
    task.abort();
}

#[tokio::test]
async fn fake_records_retries_and_rejects_stray_requests_per_factory() {
    let left = Factory::new().unwrap();
    let right = Factory::new().unwrap();
    let calls = AtomicUsize::new(0);
    left.fake(move |request| {
        (request.url().host_str() == Some("example.test")).then(|| {
            if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                Err(Error::Connection)
            } else {
                Response::from_json(StatusCode::OK, &json!({"side": "left"}))
            }
        })
    });
    left.preventStrayRequests(true);
    right.stubUrl(
        "example.test/*",
        Response::from_json(StatusCode::OK, &json!({"side": "right"})).unwrap(),
    );
    right.preventStrayRequests(true);
    let url = "https://example.test/resource";
    let response = left
        .pendingRequest()
        .retry(2, Duration::ZERO)
        .get(url)
        .await
        .unwrap();
    assert_eq!(response.json::<Value>().unwrap()["side"], "left");
    assert_eq!(
        right.get(url).await.unwrap().json::<Value>().unwrap()["side"],
        "right"
    );
    assert!(matches!(
        left.get("http://127.0.0.1:1/missing").await,
        Err(Error::StrayRequest)
    ));
    let recorded = left.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].0.url.as_str(), url);
    assert!(recorded[0].1.is_none());
    assert!(recorded[1].1.as_ref().unwrap().ok());
    right.assertSentCount(1);
    right.assertSent(|request, response| {
        request.url.as_str() == url && response.is_some_and(Response::ok)
    });
    right.assertNotSent(|request, _| request.method == Method::POST);
}

async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (origin, task)
}

#[tokio::test]
async fn named_pool_keeps_order_and_per_request_results() {
    let factory = Factory::new().unwrap();
    let sequence = factory.fakeSequence("https://pool.test/slow");
    sequence.push(Err(Error::Connection)).push(Ok(Response::new(
        StatusCode::OK,
        HeaderMap::new(),
        b"retried",
    )));
    factory.stubUrl(
        "https://pool.test/fast",
        Response::new(StatusCode::NOT_FOUND, HeaderMap::new(), b"missing"),
    );
    factory.preventStrayRequests(true);
    let results = factory
        .pool(2.try_into().unwrap(), |pool| {
            pool.r#as("slow", |request| {
                request
                    .retry(2, Duration::ZERO)
                    .get("https://pool.test/slow")
            })
            .r#as("fast", |request| request.get("https://pool.test/fast"))
            .r#as("unknown", |request| {
                request.get("https://pool.test/unknown")
            });
        })
        .await;
    assert_eq!(
        results.keys().map(String::as_str).collect::<Vec<_>>(),
        ["slow", "fast", "unknown"]
    );
    assert_eq!(results["slow"].as_ref().unwrap().body(), b"retried");
    assert!(results["fast"].as_ref().unwrap().notFound());
    assert!(matches!(results["unknown"], Err(Error::StrayRequest)));
    assert!(sequence.isEmpty());
    assert!(matches!(
        factory.get("https://pool.test/slow").await,
        Err(Error::SequenceExhausted)
    ));
}

#[tokio::test]
async fn facade_resolves_the_factory_for_each_serving_application() {
    use openrust::{
        container::ServiceKey,
        contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{HttpError, HttpFuture, Request, Response as InboundResponse},
        support::facades::{Http, Route},
    };

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

    fn upstream(_request: &mut Request) -> HttpFuture<'_, Result<InboundResponse, HttpError>> {
        Box::pin(async {
            let response = Http::get("https://upstream.test/config")
                .await
                .map_err(|error| HttpError::Internal {
                    source: Box::new(error),
                })?;
            InboundResponse::json(response.status(), response.json::<Value>().unwrap())
        })
    }

    async fn start(side: &str) -> (String, tokio::task::JoinHandle<()>) {
        let application = Application::configure(std::env::temp_dir())
            .withRouting(Routing::default().web(|| Route::get("/upstream", upstream)))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        let factory = kernel
            .get_application()
            .container()
            .get::<Factory>(ServiceKey::typed::<Factory>())
            .unwrap();
        factory.stubUrl(
            "https://upstream.test/*",
            Response::from_json(StatusCode::OK, &json!({"side": side})).unwrap(),
        );
        factory.preventStrayRequests(true);
        serve(
            Router::new()
                .fallback(transport)
                .with_state(Arc::new(kernel)),
        )
        .await
    }

    let ((left, left_task), (right, right_task)) = tokio::join!(start("left"), start("right"));
    let client = Factory::new().unwrap();
    let left = format!("{left}/upstream");
    let right = format!("{right}/upstream");
    let (left, right) = tokio::join!(client.get(&left), client.get(&right));
    assert_eq!(left.unwrap().json::<Value>().unwrap()["side"], "left");
    assert_eq!(right.unwrap().json::<Value>().unwrap()["side"], "right");
    left_task.abort();
    right_task.abort();
}
