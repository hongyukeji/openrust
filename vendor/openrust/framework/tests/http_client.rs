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
async fn batch_dispatches_callbacks_and_preserves_registration_order() {
    use std::{num::NonZeroUsize, sync::Mutex};

    let factory = Factory::new().unwrap();
    factory.stubUrl(
        "https://batch.test/ok",
        Response::new(StatusCode::OK, HeaderMap::new(), b"ok"),
    );
    factory.stubUrl(
        "https://batch.test/fail",
        Response::new(StatusCode::BAD_GATEWAY, HeaderMap::new(), b"bad"),
    );
    factory.preventStrayRequests(true);
    let calls = Mutex::new(Vec::new());
    let results = factory
        .batch(|batch| {
            batch
                .concurrency(NonZeroUsize::MIN)
                .r#as("first", |request| request.get("https://batch.test/ok"))
                .r#as("second", |request| request.get("https://batch.test/fail"))
                .r#as("third", |request| request.get("https://batch.test/stray"))
                .before(|counts| {
                    assert_eq!(counts.pending_requests, 3);
                    calls.lock().unwrap().push("before".into());
                })
                .progress(|counts, key, _| {
                    assert_eq!(counts.processedRequests(), 1);
                    calls.lock().unwrap().push(key.to_owned());
                })
                .catch(|counts, key, _| {
                    assert!(counts.hasFailures());
                    calls.lock().unwrap().push(key.to_owned());
                })
                .then(|_, _| panic!("a failed batch must skip then"))
                .finally(|counts, results| {
                    assert_eq!(counts.failed_requests, 2);
                    assert_eq!(results.len(), 3);
                    calls.lock().unwrap().push("finally".into());
                });
        })
        .send()
        .await;
    assert_eq!(
        results.keys().map(String::as_str).collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    assert_eq!(
        *calls.lock().unwrap(),
        ["before", "first", "second", "third", "finally"]
    );
}

#[tokio::test]
async fn http_factory_delivers_typed_events_to_its_own_application() {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        events::Dispatcher,
        foundation::Application,
        http::client::events::{
            ConnectionFailed, ConnectionFailure, RequestSending, ResponseReceived,
        },
    };

    let (base, server) = serve(Router::new().route("/ok", get(|| async { "ok" })).route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            "slow"
        }),
    ))
    .await;
    let mut application = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    application.boot().await.unwrap();
    let container = application.container();
    let events = container.make::<Dispatcher>().unwrap().fake(&[]);
    let factory = container.make::<Factory>().unwrap();
    assert!(
        factory
            .get(&format!("{base}/ok"))
            .await
            .unwrap()
            .successful()
    );
    assert!(matches!(
        factory
            .pendingRequest()
            .timeout(Duration::from_millis(20))
            .get(&format!("{base}/slow"))
            .await,
        Err(Error::Timeout)
    ));
    let requests = events.dispatched(r"OpenRust\Http\Client\Events\RequestSending");
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0]
            .downcast_ref::<RequestSending>()
            .unwrap()
            .request
            .url
            .path(),
        "/ok"
    );
    let replies = events.dispatched(r"OpenRust\Http\Client\Events\ResponseReceived");
    assert_eq!(replies.len(), 1);
    assert_eq!(
        replies[0]
            .downcast_ref::<ResponseReceived>()
            .unwrap()
            .response
            .body(),
        b"ok"
    );
    let failures = events.dispatched(r"OpenRust\Http\Client\Events\ConnectionFailed");
    assert_eq!(failures.len(), 1);
    assert_eq!(
        failures[0]
            .downcast_ref::<ConnectionFailed>()
            .unwrap()
            .exception,
        ConnectionFailure::Timeout
    );
    server.abort();
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
        .poolOrdered(|pool| {
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

#[tokio::test]
async fn bounded_pool_releases_slots_and_yields_results_in_completion_order() {
    use std::num::NonZeroUsize;
    use tokio::sync::Notify;

    let slow_started = Arc::new(Notify::new());
    let release_slow = Arc::new(Notify::new());
    let (base, server) = serve(
        Router::new()
            .route(
                "/slow",
                get({
                    let slow_started = slow_started.clone();
                    let release_slow = release_slow.clone();
                    move || async move {
                        slow_started.notify_one();
                        release_slow.notified().await;
                        "slow"
                    }
                }),
            )
            .route(
                "/fast",
                get(move || async move {
                    slow_started.notified().await;
                    "fast"
                }),
            )
            .route("/next", get(|| async { "next" })),
    )
    .await;
    let url = |suffix| format!("{base}/{suffix}");
    let (slow, fast, next) = (url("slow"), url("fast"), url("next"));
    let pending = Factory::new().unwrap().pendingRequest();
    let results = pending.pool(NonZeroUsize::new(2).unwrap(), |pool| {
        pool.r#as("4", |request| request.get(&slow))
            .r#as("fast", |request| request.get(&fast))
            .newRequest(|request| {
                request
                    .afterResponse(move |response| {
                        release_slow.notify_one();
                        Ok(response)
                    })
                    .get(&next)
            });
    });
    let results = tokio::time::timeout(Duration::from_secs(5), results)
        .await
        .unwrap();
    assert_eq!(
        results.keys().map(String::as_str).collect::<Vec<_>>(),
        ["fast", "5", "4"]
    );
    assert!(results.values().all(Result::is_ok));
    server.abort();
}

#[tokio::test]
async fn batch_empty_redirect_and_conditional_http_errors_preserve_callback_boundaries() {
    use std::sync::Mutex;

    let (base, server) = serve(
        Router::new()
            .route("/redirect", get(|| async { StatusCode::FOUND }))
            .route("/error", get(|| async { StatusCode::BAD_REQUEST })),
    )
    .await;
    let redirect = format!("{base}/redirect");
    let error = format!("{base}/error");
    let factory = Factory::new().unwrap();
    let phases = Mutex::new(Vec::new());
    let results = factory
        .batch(|batch| {
            batch
                .newRequest(|request| request.withoutRedirecting().get(&redirect))
                .newRequest(|request| request.throwIf(|response| response.badRequest()).get(&error))
                .progress(|_, _, _| panic!("neither response is successful"))
                .catch(|counts, key, result| {
                    assert_eq!(key, "1");
                    assert_eq!(counts.failed_requests, 1);
                    assert!(matches!(result, Err(Error::RequestFailed(response)) if response.badRequest()));
                })
                .then(|_, _| panic!("failed batch must skip then"))
                .finally(|counts, results| {
                    assert_eq!(counts.processedRequests(), 2);
                    assert_eq!(results.keys().map(String::as_str).collect::<Vec<_>>(), ["0", "1"]);
                });
        })
        .send()
        .await;
    assert!(results["0"].as_ref().unwrap().redirect());
    assert!(
        factory
            .pendingRequest()
            .throwIf(|_| false)
            .get(&error)
            .await
            .unwrap()
            .badRequest()
    );
    factory
        .batch(|batch| {
            batch
                .before(|counts| {
                    assert_eq!(counts.total_requests, 0);
                    phases.lock().unwrap().push("before");
                })
                .then(|_, results| {
                    assert!(results.is_empty());
                    phases.lock().unwrap().push("then");
                })
                .finally(|_, results| {
                    assert!(results.is_empty());
                    phases.lock().unwrap().push("finally");
                });
        })
        .send()
        .await;
    assert_eq!(*phases.lock().unwrap(), ["before", "then", "finally"]);
    assert!(matches!(
        factory.batch(|_| {}).defer(),
        Err(Error::OutsideLifecycle)
    ));
    server.abort();
}

#[tokio::test]
async fn events_cover_redirect_hops_retries_and_the_wire_request_on_failure() {
    use openrust::events::Dispatcher;
    use openrust::http::client::events::{
        ConnectionFailed, ConnectionFailure, RequestSending, ResponseReceived,
    };

    let (target, target_server) = serve(Router::new().route(
        "/target",
        get(|headers: HeaderMap| async move {
            assert!(!headers.contains_key(header::AUTHORIZATION));
            assert_eq!(headers["x-wire"], "sent");
            (StatusCode::SERVICE_UNAVAILABLE, "needs retry")
        }),
    ))
    .await;
    let broken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let broken_uri = format!("http://{}/disconnected", broken.local_addr().unwrap());
    let broken_server = tokio::spawn(async move {
        while let Ok((stream, _)) = broken.accept().await {
            drop(stream);
        }
    });
    let (base, server) = serve(
        Router::new()
            .route(
                "/redirect",
                get(move || async move {
                    (
                        StatusCode::FOUND,
                        [(header::LOCATION, format!("{target}/target"))],
                        "redirect body larger than the final body",
                    )
                }),
            )
            .route(
                "/broken",
                get(move || async move { (StatusCode::FOUND, [(header::LOCATION, broken_uri)]) }),
            )
            .route(
                "/loop",
                get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/loop")]) }),
            )
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    "slow"
                }),
            ),
    )
    .await;

    let dispatcher = Arc::new(Dispatcher::new());
    let events = dispatcher.fake(&[]);
    let factory = Factory::new().unwrap().withDispatcher(dispatcher);
    factory.fake(|_| None);
    let attrs: serde_json::Map<String, Value> =
        serde_json::from_value(json!({"trace": "local"})).unwrap();
    let pending = || {
        factory
            .pendingRequest()
            .withToken("local-test-token")
            .withAttributes(attrs.clone())
            .beforeSending(|request| {
                request
                    .headers_mut()
                    .insert("x-wire", HeaderValue::from_static("sent"));
                Ok(())
            })
    };

    let response = pending()
        .maxResponseBytes(16)
        .retry(2, Duration::ZERO)
        .throwOnRetryFailure(false)
        .afterResponse(|response| {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            Ok(Response::new(
                StatusCode::BAD_GATEWAY,
                HeaderMap::new(),
                b"rewritten",
            ))
        })
        .get(&format!("{base}/redirect"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(matches!(
        pending().get(&format!("{base}/broken")).await,
        Err(Error::Connection)
    ));
    assert!(matches!(
        factory.get(&format!("{base}/loop")).await,
        Err(Error::RedirectLimit)
    ));
    assert!(matches!(
        factory
            .pendingRequest()
            .timeout(Duration::from_millis(15))
            .get(&format!("{base}/slow"))
            .await,
        Err(Error::Timeout)
    ));

    let sending = events.dispatched(r"OpenRust\Http\Client\Events\RequestSending");
    let replies = events.dispatched(r"OpenRust\Http\Client\Events\ResponseReceived");
    let failures = events.dispatched(r"OpenRust\Http\Client\Events\ConnectionFailed");
    let paths = sending
        .iter()
        .map(|entry| {
            entry
                .downcast_ref::<RequestSending>()
                .unwrap()
                .request
                .url
                .path()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        &paths[..6],
        [
            "/redirect",
            "/target",
            "/redirect",
            "/target",
            "/broken",
            "/disconnected"
        ]
    );
    assert_eq!(paths.iter().filter(|&&path| path == "/loop").count(), 6);
    assert_eq!(paths.last(), Some(&"/slow"));

    let first = sending[0].downcast_ref::<RequestSending>().unwrap();
    assert_eq!(first.request.attributes["trace"], "local");
    assert!(!first.request.headers.contains_key("x-wire"));
    let cross_origin = sending[1].downcast_ref::<RequestSending>().unwrap();
    assert!(
        !cross_origin
            .request
            .headers
            .contains_key(header::AUTHORIZATION)
    );
    assert_eq!(replies.len(), 2);
    for reply in replies {
        let reply = reply.downcast_ref::<ResponseReceived>().unwrap();
        assert_eq!(reply.request.url.path(), "/target");
        assert_eq!(reply.response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(failures.len(), 2);
    let failure = failures[0].downcast_ref::<ConnectionFailed>().unwrap();
    assert_eq!(failure.exception, ConnectionFailure::Connection);
    assert_eq!(failure.request.url.path(), "/disconnected");
    assert_eq!(failure.request.headers["x-wire"], "sent");
    assert_eq!(failure.request.attributes["trace"], "local");
    let timed_out = failures[1].downcast_ref::<ConnectionFailed>().unwrap();
    assert_eq!(timed_out.exception, ConnectionFailure::Timeout);
    assert_eq!(timed_out.request.url.path(), "/slow");
    assert!(factory.recorded().last().unwrap().1.is_none());

    for server in [server, target_server, broken_server] {
        server.abort();
    }
}

#[tokio::test]
async fn digest_auth_replays_the_body_and_never_uses_credentials_on_another_origin() {
    use digest_auth::{AlgorithmType, AuthorizationHeader, Qop};

    async fn authenticated(request: axum::extract::Request) -> axum::response::Response {
        let (parts, body) = request.into_parts();
        let body = axum::body::to_bytes(body, 1024).await.unwrap();
        assert_eq!(body, r#"{"value":"body"}"#);
        let challenge = |nonce, stale| {
            let h = format!(
                "Basic realm=\"basic\", Digest realm=\"local, zone\", nonce=\"{nonce}\", algorithm=SHA-256, qop=\"auth-int\", stale={stale}"
            );
            (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, h)]).into_response()
        };
        let Some(auth) = parts.headers.get(header::AUTHORIZATION) else {
            return challenge("first", false);
        };
        let auth = AuthorizationHeader::parse(auth.to_str().unwrap()).unwrap();
        assert_eq!(auth.algorithm.algo, AlgorithmType::SHA2_256);
        assert_eq!(auth.username, "local-user");
        assert_eq!(auth.realm, "local, zone");
        assert_eq!(auth.qop, Some(Qop::AUTH_INT));
        assert_eq!(auth.uri, parts.uri.path_and_query().unwrap().as_str());
        assert_eq!(auth.nc, 1);

        let hash = |bytes: &[u8]| {
            use std::fmt::Write as _;
            let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes);
            let mut hex = String::with_capacity(64);
            for byte in digest.as_ref() {
                write!(hex, "{byte:02x}").unwrap();
            }
            hex
        };
        let entity = hash(&body);
        let h1 = hash(b"local-user:local, zone:local-key");
        let h2 = hash(format!("POST:{}:{entity}", auth.uri).as_bytes());
        let nonce = &auth.nonce;
        let cnonce = auth.cnonce.unwrap();
        let expected = hash(format!("{h1}:{nonce}:00000001:{cnonce}:auth-int:{h2}").as_bytes());
        assert_eq!(auth.response, expected);
        match nonce.as_str() {
            "first" => challenge("fresh", true),
            "fresh" => (StatusCode::OK, "validated").into_response(),
            _ => panic!("unexpected nonce"),
        }
    }

    let (other_origin, other_server) = serve(Router::new().route(
        "/challenge",
        get(|headers: HeaderMap| async move {
            assert!(!headers.contains_key(header::AUTHORIZATION));
            (
                StatusCode::UNAUTHORIZED,
                [(
                    header::WWW_AUTHENTICATE,
                    r#"Digest realm="other", nonce="fresh", qop="auth""#,
                )],
            )
        }),
    ))
    .await;
    let (base, server) = serve(Router::new().route("/digest", any(authenticated)).route(
        "/redirect",
        get(move || async move {
            (
                StatusCode::FOUND,
                [(header::LOCATION, format!("{other_origin}/challenge"))],
            )
        }),
    ))
    .await;
    let factory = Factory::new().unwrap();
    factory.fake(|_| None);
    let request = || {
        factory
            .pendingRequest()
            .withDigestAuth("local-user", "local-key")
    };
    let response = request()
        .maxResponseBytes(32)
        .post(
            &format!("{base}/digest?key=a%20b"),
            &json!({"value": "body"}),
        )
        .await
        .unwrap();
    assert_eq!(response.body(), b"validated");
    let recorded = factory.recorded();
    assert_eq!(recorded.len(), 3);
    assert!(!recorded[0].0.headers.contains_key(header::AUTHORIZATION));
    assert!(recorded[1].0.headers[header::AUTHORIZATION].is_sensitive());
    assert!(recorded[2].0.headers[header::AUTHORIZATION].is_sensitive());
    assert!(
        request()
            .get(&format!("{base}/redirect"))
            .await
            .unwrap()
            .unauthorized()
    );
    factory.assertSentCount(5);
    for task in [server, other_server] {
        task.abort();
    }
}

#[tokio::test]
async fn expands_uri_templates_before_parsing_the_outgoing_url() {
    let (origin, server) = serve(Router::new().route(
        "/{*path}",
        get(|request: axum::extract::Request| async move { request.uri().to_string() }),
    ))
    .await;
    let factory = Factory::new().unwrap();
    let parameters =
        |value| serde_json::from_value::<serde_json::Map<String, Value>>(value).unwrap();
    let request = factory.withUrlParameters(parameters(json!({
        "endpoint": origin, "path": "a/b", "search": "中文", "tags": ["a b", "c"],
        "filters": {"empty":"", "sort":"a:b"}, "absent": null, "unused": {"nested": ["valid JSON"]},
    })));
    let response = request
        .get("{+endpoint}/files/{path}{?search,tags*,filters*,absent}")
        .await
        .unwrap();
    assert_eq!(
        response.body(),
        b"/files/a%2Fb?search=%E4%B8%AD%E6%96%87&tags=a%20b&tags=c&empty=&sort=a%3Ab"
    );

    let nested = factory
        .pendingRequest()
        .baseUrl(&origin)
        .withUrlParameters(parameters(json!({
            "filters": {"range": {"from": 1, "to": 5}, "flags": [true, false]},
        })))
        .get("/files{?filters*}")
        .await
        .unwrap();
    assert_eq!(
        nested.body(),
        b"/files?range%5Bfrom%5D=1&range%5Bto%5D=5&flags%5B0%5D=1&flags%5B1%5D=0"
    );

    let relative = factory
        .pendingRequest()
        .baseUrl("{+endpoint}/api/{version}")
        .withUrlParameters(parameters(json!({"endpoint":origin,"version":"old"})))
        .withUrlParameters(parameters(json!({"version":"v2","search":"ignored"})))
        .withQueryParameters([("actual".to_owned(), "x y".to_owned())])
        .get("/folders{?search}")
        .await
        .unwrap();
    assert_eq!(relative.body(), b"/api/v2/folders?actual=x%20y");

    let literal = factory
        .get(&format!(
            r#"{origin}/files/{{draft}}.txt?where={{"status":"open"}}"#
        ))
        .await
        .unwrap();
    assert_eq!(
        literal.body(),
        b"/files/%7Bdraft%7D.txt?where=%7B%22status%22:%22open%22%7D"
    );

    let malformed = factory
        .withUrlParameters(parameters(json!({"endpoint":origin})))
        .get("{+endpoint}/broken/{page")
        .await;
    assert!(matches!(malformed, Err(Error::InvalidUriTemplate)));
    server.abort();
}

#[tokio::test]
async fn selective_stray_allowance_checks_redirects_and_keeps_fake_priority() {
    let (origin, server) = serve(
        Router::new()
            .route("/allowed/live", get(|| async { "local upstream" }))
            .route(
                "/allowed/fake",
                get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "fake should win") }),
            )
            .route(
                "/allowed/redirect",
                get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/blocked")]) }),
            )
            .route(
                "/blocked",
                get(|| async { "only reached after prevention is disabled" }),
            ),
    )
    .await;
    let factory = Factory::new().unwrap();
    factory.preventStrayRequests(true);
    assert!(factory.preventingStrayRequests());
    factory.allowStrayRequests([format!("{origin}/allowed/*")]);
    factory.stubUrl(
        format!("{origin}/allowed/fake"),
        Response::new(StatusCode::OK, HeaderMap::new(), b"fake"),
    );

    assert!(factory.isAllowedRequestUrl(&format!("{origin}/allowed/live")));
    assert!(!factory.isAllowedRequestUrl(&format!("{origin}/blocked")));
    assert_eq!(
        factory
            .get(&format!("{origin}/allowed/live"))
            .await
            .unwrap()
            .body(),
        b"local upstream"
    );
    assert_eq!(
        factory
            .get(&format!("{origin}/allowed/fake"))
            .await
            .unwrap()
            .body(),
        b"fake"
    );
    assert!(matches!(
        factory.get(&format!("{origin}/allowed/redirect")).await,
        Err(Error::StrayRequest)
    ));
    factory.assertSentCount(3);
    assert!(!Factory::new().unwrap().preventingStrayRequests());

    factory.allowStrayRequests([format!("{origin}/blocked")]);
    assert!(matches!(
        factory.get(&format!("{origin}/allowed/live")).await,
        Err(Error::StrayRequest)
    ));
    assert!(
        factory
            .get(&format!("{origin}/blocked"))
            .await
            .unwrap()
            .ok()
    );
    factory.preventStrayRequests(false);
    assert!(
        factory
            .get(&format!("{origin}/allowed/live"))
            .await
            .unwrap()
            .ok()
    );
    server.abort();
}

#[tokio::test]
async fn retry_callback_modifies_the_next_request_and_preserves_changes_on_later_attempts() {
    use std::num::NonZeroUsize;

    let (origin, server) = serve(Router::new().route(
        "/retry",
        any(|request: axum::extract::Request| async move {
            let (parts, body) = request.into_parts();
            assert_eq!(parts.method, Method::POST);
            assert_eq!(
                axum::body::to_bytes(body, 1024).await.unwrap(),
                r#"{"preserve":"body"}"#
            );
            match parts
                .headers
                .get("x-phase")
                .and_then(|value| value.to_str().ok())
            {
                None => StatusCode::UNAUTHORIZED,
                Some("renewed") if !parts.headers.contains_key("x-later") => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                Some("renewed") => StatusCode::OK,
                _ => StatusCode::BAD_REQUEST,
            }
        }),
    ))
    .await;
    let factory = Factory::new().unwrap();
    factory.fake(|_| None);
    let url = format!("{origin}/retry");
    let body = json!({"preserve":"body"});
    let results = factory
        .pool(NonZeroUsize::MIN, |pool| {
            pool.newRequest(|request| {
                request
                    .retryDelays(vec![Duration::ZERO; 2])
                    .retryWhenRequest(|error, next| {
                        let Error::RequestFailed(response) = error else {
                            return Ok(false);
                        };
                        assert_eq!(next.method(), Method::POST);
                        if response.unauthorized() {
                            next.headers_mut()
                                .insert("x-phase", HeaderValue::from_static("renewed"));
                        } else {
                            assert_eq!(next.headers()["x-phase"], "renewed");
                            next.headers_mut()
                                .insert("x-later", HeaderValue::from_static("true"));
                        }
                        Ok(true)
                    })
                    .post(&url, &body)
            });
        })
        .await;
    assert!(results["0"].as_ref().unwrap().ok());
    factory.assertSentCount(3);

    let denied = factory
        .pendingRequest()
        .retry(3, Duration::ZERO)
        .retryWhenRequest(|_, _| Ok(false))
        .post(&url, &body)
        .await;
    assert!(matches!(denied, Err(Error::RequestFailed(response)) if response.unauthorized()));
    factory.assertSentCount(4);

    let failed = factory
        .pendingRequest()
        .retry(3, Duration::ZERO)
        .retryWhenRequest(|_, _| Err(Error::Configuration))
        .post(&url, &body)
        .await;
    assert!(matches!(failed, Err(Error::Configuration)));
    factory.assertSentCount(5);
    server.abort();
}

#[tokio::test]
async fn global_middleware_wraps_each_redirect_and_retry_with_network_transfer_stats() {
    use std::sync::Mutex;
    let calls = Arc::new(AtomicUsize::new(0));
    let pending = calls.clone();
    let (base, server) = serve(
        Router::new()
            .route(
                "/redirect",
                get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/measured")]) }),
            )
            .route(
                "/measured",
                get(move |headers: HeaderMap| {
                    let calls = pending.clone();
                    async move {
                        assert_eq!(headers["x-global"], "present");
                        tokio::time::sleep(Duration::from_millis(8)).await;
                        let status = if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::OK
                        };
                        (status, "measured")
                    }
                }),
            ),
    )
    .await;
    let factory = Factory::new().unwrap();
    let phases = Arc::new(Mutex::new(Vec::new()));
    let observed = phases.clone();
    factory.globalMiddleware(move |request, next| {
        let observed = observed.clone();
        let path = request.url().path().to_owned();
        Box::pin(async move {
            observed.lock().unwrap().push(format!("enter:{path}"));
            let response = next.run(request).await?;
            assert!(response.handlerStats()["total_time"] > 0.0);
            observed.lock().unwrap().push(format!("leave:{path}"));
            Ok(response)
        })
    });
    factory.globalRequestMiddleware(|request| {
        request
            .headers_mut()
            .insert("x-global", HeaderValue::from_static("present"));
        Ok(())
    });
    factory.globalResponseMiddleware(|response| {
        assert!(response.handlerStats()["total_time"] > 0.0);
        Ok(response)
    });
    let observed = phases.clone();
    let response = factory
        .pendingRequest()
        .withMiddleware(move |request, next| {
            let observed = observed.clone();
            Box::pin(async move {
                observed.lock().unwrap().push("local:enter".into());
                let response = next.run(request).await?;
                observed.lock().unwrap().push("local:leave".into());
                Ok(response)
            })
        })
        .withResponseMiddleware(|response| {
            Ok(Response::new(
                response.status(),
                response.headers().clone(),
                response.body().to_vec(),
            ))
        })
        .retry(2, Duration::ZERO)
        .get(&format!("{base}/redirect"))
        .await
        .unwrap();
    assert_eq!(response.body(), b"measured");
    assert!(response.handlerStats()["total_time"] >= 0.008);
    assert_eq!(
        response.handlerStats()["total_time"],
        response.transferStats.as_ref().unwrap().getTransferTime()
    );
    let expected: Vec<String> = ["/redirect", "/measured", "/redirect", "/measured"]
        .into_iter()
        .flat_map(|path| {
            [
                format!("enter:{path}"),
                "local:enter".into(),
                "local:leave".into(),
                format!("leave:{path}"),
            ]
        })
        .collect();
    assert_eq!(*phases.lock().unwrap(), expected);
    server.abort();
}

#[tokio::test]
async fn global_middleware_is_factory_local_snapshotted_and_runs_on_fakes_and_short_circuits() {
    let factory = Factory::new().unwrap();
    factory.fake(|request| {
        Some(Ok(Response::from_json(
            StatusCode::OK,
            &json!({"header":request.headers().contains_key("x-global")}),
        )
        .unwrap()))
    });
    let before = factory.pendingRequest();
    factory.globalRequestMiddleware(|request| {
        request
            .headers_mut()
            .insert("x-global", HeaderValue::from_static("present"));
        Ok(())
    });
    let response = before.get("https://fixture.test/before").await.unwrap();
    assert_eq!(response.json::<Value>().unwrap()["header"], false);
    let response = factory
        .clone()
        .get("https://fixture.test/after")
        .await
        .unwrap();
    assert_eq!(response.json::<Value>().unwrap()["header"], true);
    assert_eq!(response.transferStats.unwrap().getTransferTime(), 0.0);
    let response = factory.get("https://fixture.test/fake").await.unwrap();
    assert!(response.handlerStats().is_empty());
    let isolated = Factory::new().unwrap();
    isolated.fake(|request| {
        assert!(!request.headers().contains_key("x-global"));
        Some(Ok(Response::new(
            StatusCode::OK,
            HeaderMap::new(),
            Vec::new(),
        )))
    });
    isolated.get("https://fixture.test/isolated").await.unwrap();
    let short = Factory::new().unwrap();
    short.preventStrayRequests(true);
    short.globalMiddleware(|_, _| {
        Box::pin(async {
            Ok(Response::new(
                StatusCode::ACCEPTED,
                HeaderMap::new(),
                b"intercepted",
            ))
        })
    });
    assert_eq!(
        short
            .get("https://fixture.test/no-network")
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert!(short.recorded().is_empty());
}

#[tokio::test]
async fn global_middleware_observes_failures_and_redacted_events_keep_measured_duration() {
    use openrust::http::client::events::ResponseReceived;
    use std::sync::Mutex;
    let (base, server) = serve(Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            "private response"
        }),
    ))
    .await;
    let dispatcher = Arc::new(openrust::events::Dispatcher::new());
    let events = dispatcher.fake(&[]);
    let factory = Factory::new().unwrap().withDispatcher(dispatcher);
    let failures = Arc::new(Mutex::new(Vec::new()));
    let observed = failures.clone();
    factory.globalMiddleware(move |request, next| {
        let observed = observed.clone();
        Box::pin(async move {
            let result = next.run(request).await;
            if let Err(error) = &result {
                observed.lock().unwrap().push(error.to_string());
            }
            result
        })
    });
    let response = factory
        .pendingRequest()
        .redactPayload()
        .get(&format!("{base}/slow?secret=hidden"))
        .await
        .unwrap();
    assert!(response.handlerStats()["total_time"] >= 0.03);
    let events = events.dispatched(r"OpenRust\Http\Client\Events\ResponseReceived");
    let event = events[0].downcast_ref::<ResponseReceived>().unwrap();
    assert!(event.request.url.query().is_none());
    assert!(event.response.body().is_empty());
    assert_eq!(
        event.response.handlerStats()["total_time"],
        response.handlerStats()["total_time"]
    );
    assert!(matches!(
        factory
            .pendingRequest()
            .timeout(Duration::from_millis(1))
            .get(&format!("{base}/slow"))
            .await,
        Err(Error::Timeout)
    ));
    // Transport timeout and connection failures both return through the middleware continuation.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let broken = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        drop(stream);
    });
    assert!(matches!(
        factory.get(&format!("http://{address}/broken")).await,
        Err(Error::Connection)
    ));
    assert_eq!(failures.lock().unwrap().len(), 2);
    broken.await.unwrap();
    server.abort();
}

#[tokio::test]
async fn handler_stats_include_network_response_body_reads() {
    let (base, server) = serve(Router::new().route(
        "/body",
        get(|| async {
            axum::body::Body::from_stream(futures_util::stream::unfold(0, |index| async move {
                if index == 2 {
                    return None;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"a")),
                    index + 1,
                ))
            }))
        }),
    ))
    .await;
    let response = Factory::new()
        .unwrap()
        .get(&format!("{base}/body"))
        .await
        .unwrap();
    assert_eq!(response.body(), b"aa");
    assert!(response.handlerStats()["total_time"] >= 0.02);
    server.abort();
}

#[tokio::test]
async fn laravel_header_replacement_preserves_other_keys_multi_values_and_sink_failures() {
    async fn echo(headers: HeaderMap) -> String {
        serde_json::json!({
            "keep": headers.get("x-keep").unwrap().to_str().unwrap(),
            "replace": headers.get_all("x-replace").iter().map(|v| v.to_str().unwrap()).collect::<Vec<_>>(),
            "type": headers.get("content-type").map(|v| v.to_str().unwrap())
        }).to_string()
    }
    let (origin, task) = serve(Router::new().route("/", any(echo))).await;
    let factory = Factory::new().unwrap();
    let mut initial = HeaderMap::new();
    initial.insert("x-keep", HeaderValue::from_static("keep"));
    initial.append("x-replace", HeaderValue::from_static("old"));
    let mut replacement = HeaderMap::new();
    replacement.append("x-replace", HeaderValue::from_static("one"));
    replacement.append("x-replace", HeaderValue::from_static("two"));
    let output = tempfile::tempdir().unwrap();
    let file = output.path().join("response.json");
    let response = factory
        .pendingRequest()
        .withHeaders(initial)
        .replaceHeaders(replacement)
        .contentType(HeaderValue::from_static("text/plain"))
        .sink(&file)
        .get(&origin)
        .await
        .unwrap();
    let body: Value = response.json().unwrap();
    assert_eq!(body["keep"], "keep");
    assert_eq!(body["replace"], json!(["one", "two"]));
    assert_eq!(body["type"], "text/plain");
    assert_eq!(std::fs::read(file).unwrap(), response.body());
    assert!(matches!(
        factory
            .pendingRequest()
            .withHeader(
                http::HeaderName::from_static("x-keep"),
                HeaderValue::from_static("keep")
            )
            .sink(output.path().join("missing/out"))
            .get(&origin)
            .await,
        Err(Error::Sink(_))
    ));
    task.abort();
}

#[tokio::test]
async fn multipart_redirect_limits_and_invalid_header_state_use_the_real_transport() {
    async fn echo(request: axum::extract::Request) -> impl IntoResponse {
        let (parts, body) = request.into_parts();
        let body = axum::body::to_bytes(body, 65536).await.unwrap();
        json!({"type":parts.headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap(),"body":String::from_utf8_lossy(&body)}).to_string()
    }
    let (base, task) = serve(Router::new().route("/echo", any(echo)).route(
        "/redirect",
        get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/echo")]) }),
    ))
    .await;
    let factory = Factory::new().unwrap();
    let body: Value = factory
        .pendingRequest()
        .asMultipart()
        .post(&format!("{base}/echo"), &json!({"title":"value"}))
        .await
        .unwrap()
        .json()
        .unwrap();
    assert!(
        body["type"]
            .as_str()
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    assert!(
        body["body"]
            .as_str()
            .unwrap()
            .contains("name=\"title\"\r\n\r\nvalue")
    );
    assert!(matches!(
        factory
            .pendingRequest()
            .maxRedirects(0)
            .get(&format!("{base}/redirect"))
            .await,
        Err(Error::RedirectLimit)
    ));
    assert!(matches!(
        factory
            .pendingRequest()
            .withToken("invalid\nvalue")
            .replaceHeaders(HeaderMap::new())
            .get(&base)
            .await,
        Err(Error::InvalidRequest)
    ));
    task.abort();
}
