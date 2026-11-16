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
