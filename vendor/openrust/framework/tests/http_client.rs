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
        .pending_request()
        .base_url(format!("{origin}/api"))
        .accept_json()
        .with_token("example")
        .with_query_parameters([("q".into(), "有 & space".into())])
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
        .pending_request()
        .as_form()
        .with_basic_auth("foo", "bar")
        .put(&url, &[("title", "a b")])
        .await
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(form["body"], "title=a+b");
    assert_eq!(form["authorization"], "Basic Zm9vOmJhcg==");
    assert_eq!(form["query"], Value::Null);

    struct CountedForm(AtomicUsize);
    impl serde::Serialize for CountedForm {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let _ = self.0.fetch_add(1, Ordering::Relaxed);
            [("repeat", json!(false)), ("repeat", json!(2))].serialize(serializer)
        }
    }
    factory.fake(|_| None);
    let data = CountedForm(AtomicUsize::new(0));
    let form: Value = factory
        .pending_request()
        .as_form()
        .post(&url, &data)
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(data.0.load(Ordering::Relaxed), 1);
    assert_eq!(form["body"], "repeat=false&repeat=2");
    let requests = factory.recorded();
    assert_eq!(
        requests.last().unwrap().0.diagnostic_data(),
        json!({"repeat":2})
    );
    assert_eq!(
        requests.last().unwrap().0.data(),
        Some(json!({"repeat":"2"}))
    );

    let form: Value = factory
        .pending_request()
        .as_form()
        .before_sending(|request| {
            *request.body_mut() = Some("replacement=yes".into());
            Ok(())
        })
        .post(&url, &data)
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(data.0.load(Ordering::Relaxed), 2);
    assert_eq!(form["body"], "replacement=yes");
    assert_eq!(
        factory.recorded().last().unwrap().0.diagnostic_data(),
        json!({"replacement":"yes"})
    );

    let patched = factory
        .pending_request()
        .before_sending(|request| {
            request
                .headers_mut()
                .insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
            Ok(())
        })
        .after_response(|response| {
            assert!(response.ok());
            Response::from_json(StatusCode::CREATED, &response.json::<Value>()?)
        })
        .get(&url)
        .await
        .unwrap();
    assert!(patched.created());
    assert_eq!(patched.json::<Value>().unwrap()["accept"], "text/plain");
    let raw = factory
        .pending_request()
        .with_body("plain", HeaderValue::from_static("text/plain"))
        .send(Method::PATCH, &url)
        .await
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(raw["body"], "plain");
    let multipart = factory
        .pending_request()
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
            .pending_request()
            .without_redirecting()
            .get(&redirect)
            .await
            .unwrap()
            .found()
    );
    let missing = factory.get(&redirect).await.unwrap();
    assert!(missing.not_found());
    assert!(
        matches!(missing.throw(), Err(Error::RequestFailed(response)) if response.body().unwrap().as_ref() == b"missing")
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
            .pending_request()
            .retry(3, Duration::ZERO)
            .get(&format!("{origin}/retry"))
            .await
            .unwrap()
            .ok()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(matches!(
        factory.pending_request().retry(2, Duration::ZERO)
            .get(&format!("{origin}/failed")).await,
        Err(Error::RequestFailed(response)) if response.server_error()
    ));
    assert!(
        factory
            .pending_request()
            .retry(2, Duration::ZERO)
            .throw_on_retry_failure(false)
            .get(&format!("{origin}/failed"))
            .await
            .unwrap()
            .server_error()
    );
    assert!(matches!(
        factory
            .pending_request()
            .timeout(Duration::from_millis(25))
            .get(&format!("{origin}/slow"))
            .await,
        Err(Error::Timeout)
    ));
    assert!(matches!(
        factory
            .pending_request()
            .max_response_bytes(8)
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
    left.prevent_stray_requests(true);
    right.stub_url(
        "example.test/*",
        Response::from_json(StatusCode::OK, &json!({"side": "right"})).unwrap(),
    );
    right.prevent_stray_requests(true);
    let url = "https://example.test/resource";
    let response = left
        .pending_request()
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
    right.assert_sent_count(1);
    right.assert_sent(|request, response| {
        request.url.as_str() == url && response.is_some_and(Response::ok)
    });
    right.assert_not_sent(|request, _| request.method == Method::POST);
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
    factory.stub_url(
        "https://batch.test/ok",
        Response::new(StatusCode::OK, HeaderMap::new(), b"ok"),
    );
    factory.stub_url(
        "https://batch.test/fail",
        Response::new(StatusCode::BAD_GATEWAY, HeaderMap::new(), b"bad"),
    );
    factory.prevent_stray_requests(true);
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
                    assert_eq!(counts.processed_requests(), 1);
                    calls.lock().unwrap().push(key.to_owned());
                })
                .catch(|counts, key, _| {
                    assert!(counts.has_failures());
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
            .pending_request()
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
            .body()
            .unwrap()
            .as_ref(),
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
    let sequence = factory.fake_sequence("https://pool.test/slow");
    sequence.push(Err(Error::Connection)).push(Ok(Response::new(
        StatusCode::OK,
        HeaderMap::new(),
        b"retried",
    )));
    factory.stub_url(
        "https://pool.test/fast",
        Response::new(StatusCode::NOT_FOUND, HeaderMap::new(), b"missing"),
    );
    factory.prevent_stray_requests(true);
    let results = factory
        .pool_ordered(|pool| {
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
    assert_eq!(
        results["slow"].as_ref().unwrap().body().unwrap().as_ref(),
        b"retried"
    );
    assert!(results["fast"].as_ref().unwrap().not_found());
    assert!(matches!(results["unknown"], Err(Error::StrayRequest)));
    assert!(sequence.is_empty());
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
            .with_routing(Routing::default().web(|| Route::get("/upstream", upstream)))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        let factory = kernel
            .get_application()
            .container()
            .get::<Factory>(ServiceKey::typed::<Factory>())
            .unwrap();
        factory.stub_url(
            "https://upstream.test/*",
            Response::from_json(StatusCode::OK, &json!({"side": side})).unwrap(),
        );
        factory.prevent_stray_requests(true);
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
    let pending = Factory::new().unwrap().pending_request();
    let results = pending.pool(NonZeroUsize::new(2).unwrap(), |pool| {
        pool.r#as("4", |request| request.get(&slow))
            .r#as("fast", |request| request.get(&fast))
            .new_request(|request| {
                request
                    .after_response(move |response| {
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
                .new_request(|request| request.without_redirecting().get(&redirect))
                .new_request(|request| request.throw_if(|response| response.bad_request()).get(&error))
                .progress(|_, _, _| panic!("neither response is successful"))
                .catch(|counts, key, result| {
                    assert_eq!(key, "1");
                    assert_eq!(counts.failed_requests, 1);
                    assert!(matches!(result, Err(Error::RequestFailed(response)) if response.bad_request()));
                })
                .then(|_, _| panic!("failed batch must skip then"))
                .finally(|counts, results| {
                    assert_eq!(counts.processed_requests(), 2);
                    assert_eq!(results.keys().map(String::as_str).collect::<Vec<_>>(), ["0", "1"]);
                });
        })
        .send()
        .await;
    assert!(results["0"].as_ref().unwrap().redirect());
    assert!(
        factory
            .pending_request()
            .throw_if(|_| false)
            .get(&error)
            .await
            .unwrap()
            .bad_request()
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
                        [(
                            header::LOCATION,
                            format!("{target}/target?destination=value"),
                        )],
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
    let factory = Factory::new().unwrap().with_dispatcher(dispatcher);
    factory.fake(|_| None);
    let attrs: serde_json::Map<String, Value> =
        serde_json::from_value(json!({"trace": "local"})).unwrap();
    let pending = || {
        factory
            .pending_request()
            .with_token("local-test-token")
            .with_attributes(attrs.clone())
            .before_sending(|request| {
                let path = HeaderValue::from_str(request.url().path()).unwrap();
                request.headers_mut().insert("x-callback-path", path);
                request
                    .headers_mut()
                    .insert("x-wire", HeaderValue::from_static("sent"));
                Ok(())
            })
    };

    let response = pending()
        .max_response_bytes(16)
        .retry(2, Duration::ZERO)
        .throw_on_retry_failure(false)
        .after_response(|response| {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            Ok(Response::new(
                StatusCode::BAD_GATEWAY,
                HeaderMap::new(),
                b"rewritten",
            ))
        })
        .get(&format!("{base}/redirect?initial=value"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(matches!(
        pending().get(&format!("{base}/broken?ignored=value")).await,
        Err(Error::Connection)
    ));
    assert!(matches!(
        factory.get(&format!("{base}/loop")).await,
        Err(Error::RedirectLimit)
    ));
    assert!(matches!(
        factory
            .pending_request()
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
    assert_eq!(cross_origin.request.url.query(), Some("destination=value"));
    assert_eq!(
        cross_origin.request.diagnostic_data(),
        json!({"initial":"value"})
    );
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
        assert_eq!(reply.request.diagnostic_data(), json!({"initial":"value"}));
        assert_eq!(reply.request.headers["x-callback-path"], "/redirect");
        assert_eq!(reply.response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(failures.len(), 2);
    let failure = failures[0].downcast_ref::<ConnectionFailed>().unwrap();
    assert_eq!(failure.exception, ConnectionFailure::Connection);
    assert_eq!(failure.request.url.path(), "/disconnected");
    assert_eq!(failure.request.headers["x-wire"], "sent");
    assert_eq!(failure.request.headers["x-callback-path"], "/disconnected");
    assert_eq!(failure.request.attributes["trace"], "local");
    assert_eq!(failure.request.diagnostic_data(), json!([]));
    let timed_out = failures[1].downcast_ref::<ConnectionFailed>().unwrap();
    assert_eq!(timed_out.exception, ConnectionFailure::Timeout);
    assert_eq!(timed_out.request.url.path(), "/slow");
    assert!(factory.recorded().last().unwrap().1.is_none());

    for server in [server, target_server, broken_server] {
        server.abort();
    }
}

#[tokio::test]
async fn redirect_events_keep_structured_input_and_apply_request_middleware_before_callbacks() {
    use openrust::events::Dispatcher;
    use openrust::http::client::events::ResponseReceived;

    let (base, server) = serve(
        Router::new()
            .route(
                "/preserve",
                any(|| async {
                    (
                        StatusCode::TEMPORARY_REDIRECT,
                        [(header::LOCATION, "/final")],
                    )
                }),
            )
            .route(
                "/drop",
                any(|| async { (StatusCode::FOUND, [(header::LOCATION, "/final?next=yes")]) }),
            )
            .route(
                "/final",
                any(|request: axum::extract::Request| async {
                    assert_eq!(request.headers()["x-global"], "global");
                    assert_eq!(request.headers()["x-local"], "local");
                    let (parts, body) = request.into_parts();
                    let body = axum::body::to_bytes(body, 512).await.unwrap();
                    if parts.method == Method::POST {
                        assert_eq!(&body[..], b"flag=false&count=2");
                    } else {
                        assert_eq!(parts.method, Method::GET);
                        assert!(body.is_empty());
                    }
                    StatusCode::OK
                }),
            ),
    )
    .await;
    let dispatcher = Arc::new(Dispatcher::new());
    let events = dispatcher.fake(&[]);
    let factory = Factory::new().unwrap().with_dispatcher(dispatcher);
    factory.global_request_middleware(|request| {
        request
            .headers_mut()
            .insert("x-global", HeaderValue::from_static("global"));
        Ok(())
    });
    let pending = || {
        factory.pending_request().before_sending(|request| {
            request
                .headers_mut()
                .insert("x-local", HeaderValue::from_static("local"));
            Ok(())
        })
    };
    assert!(
        pending()
            .as_form()
            .post(
                &format!("{base}/preserve"),
                &[("flag", json!(false)), ("count", json!(2))]
            )
            .await
            .unwrap()
            .ok()
    );
    assert!(
        pending()
            .with_body(
                r#"{"raw":true}"#,
                HeaderValue::from_static("application/json")
            )
            .send(Method::POST, &format!("{base}/drop"))
            .await
            .unwrap()
            .ok()
    );
    let replies = events.dispatched(r"OpenRust\Http\Client\Events\ResponseReceived");
    assert_eq!(replies.len(), 2);
    let form = replies[0].downcast_ref::<ResponseReceived>().unwrap();
    assert_eq!(form.request.method, Method::POST);
    assert_eq!(
        form.request.diagnostic_data(),
        json!({"flag":false,"count":2})
    );
    let raw = replies[1].downcast_ref::<ResponseReceived>().unwrap();
    assert_eq!(raw.request.method, Method::GET);
    assert_eq!(raw.request.url.query(), Some("next=yes"));
    assert_eq!(raw.request.diagnostic_data(), json!([]));
    assert_eq!(raw.request.headers["x-global"], "global");
    // Guzzle 在重定向创建的新请求中沿用上一跳实际发送的头部。
    assert_eq!(raw.request.headers["x-local"], "local");
    server.abort();
}

#[tokio::test]
async fn redirects_select_methods_and_bodies_like_the_fixed_guzzle_client() {
    use axum::extract::Path;
    use openrust::events::Dispatcher;
    use openrust::http::client::events::ResponseReceived;

    let (base, task) = serve(
        Router::new()
            .route(
                "/redirect/{code}",
                any(
                    |Path(code): Path<u16>, request: axum::extract::Request| async move {
                        assert!(request.headers().contains_key(header::REFERER));
                        assert_eq!(request.headers()[header::CONTENT_TYPE], "text/plain");
                        let body = axum::body::to_bytes(request.into_body(), 512)
                            .await
                            .unwrap();
                        assert_eq!(&body[..], b"payload");
                        (
                            StatusCode::from_u16(code).unwrap(),
                            [(header::LOCATION, "/target")],
                        )
                    },
                ),
            )
            .route(
                "/target",
                any(|request: axum::extract::Request| async {
                    assert!(!request.headers().contains_key(header::REFERER));
                    assert_eq!(request.headers()[header::CONTENT_TYPE], "text/plain");
                    let (parts, body) = request.into_parts();
                    let body = axum::body::to_bytes(body, 512).await.unwrap();
                    let body = if body.is_empty() {
                        assert!(!parts.headers.contains_key(header::TRANSFER_ENCODING));
                        assert!(
                            parts
                                .headers
                                .get(header::CONTENT_LENGTH)
                                .is_none_or(|value| value == "0")
                        );
                        "empty"
                    } else {
                        assert_eq!(&body[..], b"payload");
                        "preserved"
                    };
                    (
                        StatusCode::OK,
                        [
                            ("x-method", parts.method.as_str().to_owned()),
                            ("x-body", body.to_owned()),
                        ],
                    )
                }),
            ),
    )
    .await;
    let dispatcher = Arc::new(Dispatcher::new());
    let events = dispatcher.fake(&[]);
    let factory = Factory::new().unwrap().with_dispatcher(dispatcher);
    let mut expected_methods = Vec::new();
    for (code, drop_body) in [
        (300, true),
        (301, true),
        (302, true),
        (303, true),
        (304, false),
        (305, false),
        (307, false),
        (308, false),
        (399, false),
    ] {
        for method in [
            Method::GET,
            Method::HEAD,
            Method::OPTIONS,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ] {
            let expected =
                if drop_body && !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
                    Method::GET
                } else {
                    method.clone()
                };
            let response = factory
                .pending_request()
                .with_header(
                    header::REFERER,
                    HeaderValue::from_static("http://example.test/source"),
                )
                .with_body("payload", HeaderValue::from_static("text/plain"))
                .send(method, &format!("{base}/redirect/{code}"))
                .await
                .unwrap();
            assert!(response.ok());
            assert_eq!(response.header("x-method").unwrap(), expected.as_str());
            assert_eq!(
                response.header("x-body").unwrap(),
                if drop_body { "empty" } else { "preserved" }
            );
            expected_methods.push(expected);
        }
    }
    let replies = events.dispatched(r"OpenRust\Http\Client\Events\ResponseReceived");
    assert_eq!(replies.len(), expected_methods.len());
    for (reply, expected) in replies.iter().zip(expected_methods) {
        let reply = reply.downcast_ref::<ResponseReceived>().unwrap();
        assert_eq!(reply.request.url.path(), "/target");
        assert_eq!(reply.request.method, expected);
    }
    task.abort();
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
            .pending_request()
            .with_digest_auth("local-user", "local-key")
    };
    let response = request()
        .max_response_bytes(32)
        .post(
            &format!("{base}/digest?key=a%20b"),
            &json!({"value": "body"}),
        )
        .await
        .unwrap();
    assert_eq!(response.body().unwrap().as_ref(), b"validated");
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
    factory.assert_sent_count(5);
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
    let request = factory.with_url_parameters(parameters(json!({
        "endpoint": origin, "path": "a/b", "search": "中文", "tags": ["a b", "c"],
        "filters": {"empty":"", "sort":"a:b"}, "absent": null, "unused": {"nested": ["valid JSON"]},
    })));
    let response = request
        .get("{+endpoint}/files/{path}{?search,tags*,filters*,absent}")
        .await
        .unwrap();
    assert_eq!(
        response.body().unwrap().as_ref(),
        b"/files/a%2Fb?search=%E4%B8%AD%E6%96%87&tags=a%20b&tags=c&empty=&sort=a%3Ab"
    );

    let nested = factory
        .pending_request()
        .base_url(&origin)
        .with_url_parameters(parameters(json!({
            "filters": {"range": {"from": 1, "to": 5}, "flags": [true, false]},
        })))
        .get("/files{?filters*}")
        .await
        .unwrap();
    assert_eq!(
        nested.body().unwrap().as_ref(),
        b"/files?range%5Bfrom%5D=1&range%5Bto%5D=5&flags%5B0%5D=1&flags%5B1%5D=0"
    );

    let relative = factory
        .pending_request()
        .base_url("{+endpoint}/api/{version}")
        .with_url_parameters(parameters(json!({"endpoint":origin,"version":"old"})))
        .with_url_parameters(parameters(json!({"version":"v2","search":"ignored"})))
        .with_query_parameters([("actual".to_owned(), "x y".to_owned())])
        .get("/folders{?search}")
        .await
        .unwrap();
    assert_eq!(
        relative.body().unwrap().as_ref(),
        b"/api/v2/folders?actual=x%20y"
    );

    let literal = factory
        .get(&format!(
            r#"{origin}/files/{{draft}}.txt?where={{"status":"open"}}"#
        ))
        .await
        .unwrap();
    assert_eq!(
        literal.body().unwrap().as_ref(),
        b"/files/%7Bdraft%7D.txt?where=%7B%22status%22:%22open%22%7D"
    );

    let malformed = factory
        .with_url_parameters(parameters(json!({"endpoint":origin})))
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
    factory.prevent_stray_requests(true);
    assert!(factory.preventing_stray_requests());
    factory.allow_stray_requests([format!("{origin}/allowed/*")]);
    factory.stub_url(
        format!("{origin}/allowed/fake"),
        Response::new(StatusCode::OK, HeaderMap::new(), b"fake"),
    );

    assert!(factory.is_allowed_request_url(&format!("{origin}/allowed/live")));
    assert!(!factory.is_allowed_request_url(&format!("{origin}/blocked")));
    assert_eq!(
        factory
            .get(&format!("{origin}/allowed/live"))
            .await
            .unwrap()
            .body()
            .unwrap()
            .as_ref(),
        b"local upstream"
    );
    assert_eq!(
        factory
            .get(&format!("{origin}/allowed/fake"))
            .await
            .unwrap()
            .body()
            .unwrap()
            .as_ref(),
        b"fake"
    );
    assert!(matches!(
        factory.get(&format!("{origin}/allowed/redirect")).await,
        Err(Error::StrayRequest)
    ));
    factory.assert_sent_count(3);
    assert!(!Factory::new().unwrap().preventing_stray_requests());

    factory.allow_stray_requests([format!("{origin}/blocked")]);
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
    factory.prevent_stray_requests(false);
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
            pool.new_request(|request| {
                request
                    .retry_delays(vec![Duration::ZERO; 2])
                    .retry_when_request(|error, next| {
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
    factory.assert_sent_count(3);

    let denied = factory
        .pending_request()
        .retry(3, Duration::ZERO)
        .retry_when_request(|_, _| Ok(false))
        .post(&url, &body)
        .await;
    assert!(matches!(denied, Err(Error::RequestFailed(response)) if response.unauthorized()));
    factory.assert_sent_count(4);

    let failed = factory
        .pending_request()
        .retry(3, Duration::ZERO)
        .retry_when_request(|_, _| Err(Error::Configuration))
        .post(&url, &body)
        .await;
    assert!(matches!(failed, Err(Error::Configuration)));
    factory.assert_sent_count(5);
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
    factory.global_middleware(move |request, next| {
        let observed = observed.clone();
        let path = request.url().path().to_owned();
        Box::pin(async move {
            observed.lock().unwrap().push(format!("enter:{path}"));
            let response = next.run(request).await?;
            assert!(response.handler_stats()["total_time"] > 0.0);
            observed.lock().unwrap().push(format!("leave:{path}"));
            Ok(response)
        })
    });
    factory.global_request_middleware(|request| {
        request
            .headers_mut()
            .insert("x-global", HeaderValue::from_static("present"));
        Ok(())
    });
    factory.global_response_middleware(|response| {
        assert!(response.handler_stats()["total_time"] > 0.0);
        Ok(response)
    });
    let observed = phases.clone();
    let response = factory
        .pending_request()
        .with_middleware(move |request, next| {
            let observed = observed.clone();
            Box::pin(async move {
                observed.lock().unwrap().push("local:enter".into());
                let response = next.run(request).await?;
                observed.lock().unwrap().push("local:leave".into());
                Ok(response)
            })
        })
        .with_response_middleware(|response| {
            Ok(Response::new(
                response.status(),
                response.headers().clone(),
                response.body().unwrap().as_ref().to_vec(),
            ))
        })
        .retry(2, Duration::ZERO)
        .get(&format!("{base}/redirect"))
        .await
        .unwrap();
    assert_eq!(response.body().unwrap().as_ref(), b"measured");
    assert!(response.handler_stats()["total_time"] >= 0.008);
    assert_eq!(
        response.handler_stats()["total_time"],
        response
            .transfer_stats
            .as_ref()
            .unwrap()
            .get_transfer_time()
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
    let before = factory.pending_request();
    factory.global_request_middleware(|request| {
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
    assert_eq!(response.transfer_stats.unwrap().get_transfer_time(), 0.0);
    let response = factory.get("https://fixture.test/fake").await.unwrap();
    assert!(response.handler_stats().is_empty());
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
    short.prevent_stray_requests(true);
    short.global_middleware(|_, _| {
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
    let factory = Factory::new().unwrap().with_dispatcher(dispatcher);
    let failures = Arc::new(Mutex::new(Vec::new()));
    let observed = failures.clone();
    factory.global_middleware(move |request, next| {
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
        .pending_request()
        .redact_payload()
        .get(&format!("{base}/slow?secret=hidden"))
        .await
        .unwrap();
    assert!(response.handler_stats()["total_time"] >= 0.03);
    let events = events.dispatched(r"OpenRust\Http\Client\Events\ResponseReceived");
    let event = events[0].downcast_ref::<ResponseReceived>().unwrap();
    assert!(event.request.url.query().is_none());
    assert!(event.response.body().unwrap().as_ref().is_empty());
    assert_eq!(
        event.response.handler_stats()["total_time"],
        response.handler_stats()["total_time"]
    );
    assert!(matches!(
        factory
            .pending_request()
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
    assert_eq!(response.body().unwrap().as_ref(), b"aa");
    assert!(response.handler_stats()["total_time"] >= 0.02);
    server.abort();
}

#[tokio::test]
async fn reference_header_replacement_preserves_other_keys_multi_values_and_sink_failures() {
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
        .pending_request()
        .with_headers(initial)
        .replace_headers(replacement)
        .content_type(HeaderValue::from_static("text/plain"))
        .sink(&file)
        .get(&origin)
        .await
        .unwrap();
    let body: Value = response.json().unwrap();
    assert_eq!(body["keep"], "keep");
    assert_eq!(body["replace"], json!(["one", "two"]));
    assert_eq!(body["type"], "text/plain");
    assert_eq!(
        std::fs::read(file).unwrap(),
        response.body().unwrap().as_ref()
    );
    assert!(matches!(
        factory
            .pending_request()
            .with_header(
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
async fn multipart_accepts_ordered_descriptors_and_rejects_invalid_values() {
    use openrust::http::client::RecordedMultipartField;

    async fn echo(body: axum::body::Bytes) -> axum::body::Bytes {
        body
    }
    let (url, task) = serve(Router::new().route("/", any(echo))).await;
    let factory = Factory::new().unwrap();
    factory.fake(|_| None);
    let data = json!([
        {"name": "dup", "contents": "first"},
        {"name": "dup", "contents": "last"},
        {"name": "file", "contents": "ascii", "filename": "input/sample.txt", "headers": {"X-Part": "observed"}},
        {"name": "logical", "contents": true, "headers": {"Content-Disposition": "form-data; name=\"wire\""}},
        {"name": "number", "contents": 42},
        {"name": "false", "contents": false}
    ]);
    let response = factory
        .pending_request()
        .as_multipart()
        .post(&url, &data)
        .await
        .unwrap();
    let body = String::from_utf8(response.body().unwrap().to_vec()).unwrap();
    assert!(body.find("\r\n\r\nfirst\r\n").unwrap() < body.find("\r\n\r\nlast\r\n").unwrap());
    assert!(body.contains("name=\"file\"; filename=\"sample.txt\""));
    assert!(body.contains("content-length: 5\r\n"));
    assert!(body.contains("content-type: text/plain\r\n"));
    assert!(body.contains("x-part: observed\r\n"));
    assert!(body.contains("name=\"wire\""));
    assert!(!body.contains("name=\"logical\""));
    let recorded = factory.recorded();
    let fields = recorded[0].0.multipart.as_ref().unwrap();
    for (name, expected) in [
        ("dup", json!("last")),
        ("logical", json!(true)),
        ("number", json!(42)),
        ("false", json!(false)),
    ] {
        assert!(
            matches!(&fields[name], RecordedMultipartField::Value(value) if value == &expected)
        );
    }

    let mixed =
        json!({"zeta": "first", "outer": {"name": "renamed", "contents": 2}, "alpha": null});
    let response = factory
        .pending_request()
        .as_multipart()
        .post(&url, &mixed)
        .await
        .unwrap();
    let body = String::from_utf8(response.body().unwrap().to_vec()).unwrap();
    let order =
        ["zeta", "renamed", "alpha"].map(|name| body.find(&format!("name=\"{name}\"")).unwrap());
    assert!(order[0] < order[1] && order[1] < order[2]);
    assert!(body.contains("name=\"alpha\"\r\n\r\n\r\n"));

    let response = factory
        .pending_request()
        .as_multipart()
        .post(&url, &json!([]))
        .await
        .unwrap();
    let recorded = factory.recorded();
    let boundary = recorded[2].0.headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .split_once("boundary=")
        .unwrap()
        .1;
    assert_eq!(
        response.body().unwrap().as_ref(),
        format!("--{boundary}--\r\n").as_bytes()
    );

    for invalid in [
        json!([{"name": "null", "contents": null}]),
        json!([{"name": 0, "contents": "body"}]),
        json!([{"name": "file", "contents": "body", "filename": 42}]),
        json!(["unlabelled"]),
        json!([{"name": "list", "contents": [1, 2]}]),
        json!({"object": {"contents": "missing name"}}),
        json!([{"name": "bad", "contents": "body", "headers": {"x-test": "line\n"}}]),
    ] {
        assert!(matches!(
            factory
                .pending_request()
                .as_multipart()
                .post(&url, &invalid)
                .await,
            Err(Error::InvalidRequest)
        ));
    }
    assert_eq!(factory.recorded().len(), 3);
    task.abort();
}

#[tokio::test]
async fn multipart_redirect_limits_and_invalid_header_state_use_the_real_transport() {
    use openrust::http::client::RecordedMultipartField;

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
        .pending_request()
        .as_multipart()
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
    factory.fake(|_| None);
    let polls = Arc::new(AtomicUsize::new(0));
    let stream = futures_util::stream::once({
        let polls = Arc::clone(&polls);
        async move {
            polls.fetch_add(1, Ordering::Relaxed);
            Ok::<_, std::io::Error>("stream-content")
        }
    });
    let observed_before_sending = Arc::clone(&polls);
    let body: Value = factory
        .pending_request()
        .attach("stream", Part::stream(reqwest::Body::wrap_stream(stream)))
        .before_sending(move |_| {
            assert_eq!(observed_before_sending.load(Ordering::Relaxed), 0);
            Ok(())
        })
        .send(Method::POST, &format!("{base}/echo"))
        .await
        .unwrap()
        .json()
        .unwrap();
    assert!(body["body"].as_str().unwrap().contains("stream-content"));
    assert_eq!(polls.load(Ordering::Relaxed), 1);
    let requests = factory.recorded();
    let recorded = &requests.last().unwrap().0;
    assert!(matches!(
        recorded.multipart.as_ref().unwrap()["stream"],
        RecordedMultipartField::Attachment {
            filename: None,
            length: None,
            ..
        }
    ));
    assert!(recorded.body.is_none());
    assert!(matches!(
        factory
            .pending_request()
            .max_redirects(0)
            .get(&format!("{base}/redirect"))
            .await,
        Err(Error::RedirectLimit)
    ));
    assert!(matches!(
        factory
            .pending_request()
            .with_token("invalid\nvalue")
            .replace_headers(HeaderMap::new())
            .get(&base)
            .await,
        Err(Error::InvalidRequest)
    ));
    task.abort();
}

#[tokio::test]
async fn multipart_replays_buffered_and_file_parts_on_redirect_and_retry() {
    type Upload = (Method, String, HeaderMap, axum::body::Bytes);

    async fn capture(
        State(uploads): State<Arc<std::sync::Mutex<Vec<Upload>>>>,
        request: axum::extract::Request,
    ) -> impl IntoResponse {
        let (parts, body) = request.into_parts();
        let body = axum::body::to_bytes(body, 256 * 1024).await.unwrap();
        let path = parts.uri.path().to_owned();
        let status = match path.as_str() {
            "/redirect307" => StatusCode::TEMPORARY_REDIRECT,
            "/redirect308" => StatusCode::PERMANENT_REDIRECT,
            "/retry" if !parts.headers.contains_key("x-retry") => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::OK,
        };
        uploads
            .lock()
            .unwrap()
            .push((parts.method, path, parts.headers, body));
        (status, [(header::LOCATION, "/target")])
    }

    let uploads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (base, task) = serve(
        Router::new()
            .route("/{path}", any(capture))
            .with_state(Arc::clone(&uploads)),
    )
    .await;
    let factory = Factory::new().unwrap();
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("upload.txt");
    let contents = format!("start-{}-end", "0123456789".repeat(7000));
    std::fs::write(&path, &contents).unwrap();
    let multipart = || async {
        factory
            .pending_request()
            .attach_body(
                "document",
                "attachment",
                Some("file.txt"),
                HeaderMap::new(),
                None,
            )
            .attach("native", Part::text("native-part"))
            .attach_file("file", &path)
            .await
            .unwrap()
            .attach("native_file", Part::file(&path).await.unwrap())
    };
    assert!(matches!(
        factory.pending_request().attach_file("missing", path.with_extension("missing")).await,
        Err(Error::Attachment(error)) if error.kind() == std::io::ErrorKind::NotFound
    ));
    assert!(matches!(
        factory.pending_request().attach_file("directory", files.path()).await,
        Err(Error::Attachment(error)) if error.kind() == std::io::ErrorKind::InvalidInput
    ));
    let data = json!({"title":"original"});
    for status in [307, 308] {
        assert!(
            multipart()
                .await
                .post(&format!("{base}/redirect{status}"), &data)
                .await
                .unwrap()
                .ok()
        );
    }
    let original_path = path.clone();
    assert!(
        multipart()
            .await
            .retry(2, Duration::ZERO)
            .retry_when_request(move |error, request| {
                assert!(matches!(error, Error::RequestFailed(response) if response.status() == StatusCode::UNPROCESSABLE_ENTITY));
                std::fs::rename(&original_path, original_path.with_extension("moved")).unwrap();
                std::fs::write(&original_path, b"replacement").unwrap();
                request.headers_mut().insert("x-retry", HeaderValue::from_static("yes"));
                Ok(true)
            })
            .post(&format!("{base}/retry"), &data)
            .await
            .unwrap()
            .ok()
    );
    let stream = futures_util::stream::once(async { Ok::<_, std::io::Error>("one shot") });
    assert!(matches!(
        factory
            .pending_request()
            .attach_body(
                "stream",
                reqwest::Body::wrap_stream(stream),
                None,
                HeaderMap::new(),
                None
            )
            .retry(2, Duration::ZERO)
            .post(&format!("{base}/retry"), &data)
            .await,
        Err(Error::UnreplayableRequest)
    ));
    let uploads = uploads.lock().unwrap();
    assert_eq!(
        uploads
            .iter()
            .map(|item| item.1.as_str())
            .collect::<Vec<_>>(),
        [
            "/redirect307",
            "/target",
            "/redirect308",
            "/target",
            "/retry",
            "/retry"
        ]
    );
    for attempt in uploads.chunks_exact(2) {
        let first = &attempt[0];
        let second = &attempt[1];
        assert_eq!(first.0, Method::POST);
        assert_eq!(second.0, Method::POST);
        assert_eq!(
            first.2[header::CONTENT_TYPE],
            second.2[header::CONTENT_TYPE]
        );
        assert_eq!(
            first.2[header::CONTENT_LENGTH],
            second.2[header::CONTENT_LENGTH]
        );
        assert_eq!(first.3, second.3);
        let body = std::str::from_utf8(&first.3).unwrap();
        assert!(body.contains("name=\"title\"\r\n\r\noriginal"));
        assert!(body.contains("filename=\"file.txt\"\r\n\r\nattachment"));
        assert!(body.contains("name=\"native\"\r\n\r\nnative-part"));
        assert!(body.contains("name=\"file\"; filename=\"upload.txt\""));
        assert!(body.contains("name=\"native_file\"; filename=\"upload.txt\""));
        assert_eq!(body.matches(&contents).count(), 2);
    }
    assert!(!uploads[4].2.contains_key("x-retry"));
    assert_eq!(uploads[5].2["x-retry"], "yes");
    task.abort();
}
