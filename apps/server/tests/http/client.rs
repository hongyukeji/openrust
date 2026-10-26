//! 通过聚合 server package 验证出站 HTTP 支持。
#![cfg(feature = "http-client")]

use axum::Json;
use axum::routing::get;
use openrust::http::{HttpClient, HttpClientConfig, HttpClientError, HttpTargetPolicy};
use serde::Deserialize;
use tokio::net::TcpListener;

#[derive(Deserialize)]
struct Payload {
    ok: bool,
}

#[tokio::test]
async fn aggregated_server_can_use_outbound_http_client() -> Result<(), Box<dyn std::error::Error>>
{
    let app = axum::Router::new().route(
        "/",
        get(|| async { Json(serde_json::json!({ "ok": true })) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    let client = HttpClient::new(HttpClientConfig::default())?;
    let response = client.get(&format!("http://{address}/")).send().await?;
    let payload: Payload = response.json().await?;
    assert!(payload.ok);

    server.abort();
    Ok(())
}

#[tokio::test]
async fn restricted_client_allows_declared_local_target_and_does_not_follow_redirects()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::response::Redirect;
    use axum::routing::get;

    let redirected_hits = Arc::new(AtomicUsize::new(0));
    let redirected_hits_handler = Arc::clone(&redirected_hits);
    let redirected_listener = TcpListener::bind("127.0.0.1:0").await?;
    let redirected_address = redirected_listener.local_addr()?;
    let redirected_app = axum::Router::new().route(
        "/",
        get(move || {
            let hits = Arc::clone(&redirected_hits_handler);
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                "redirect target reached"
            }
        }),
    );
    let redirected_server =
        tokio::spawn(async move { axum::serve(redirected_listener, redirected_app).await });

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let redirect_location = format!("http://{redirected_address}/");
    let app = axum::Router::new()
        .route("/", get(|| async { "restricted target reached" }))
        .route(
            "/redirect",
            get(move || async move { Redirect::temporary(&redirect_location) }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    let denied = HttpClient::restricted(HttpClientConfig::default(), HttpTargetPolicy::new())?;
    assert!(matches!(
        denied.get(&format!("http://{address}/")).send().await,
        Err(HttpClientError::TargetRejected)
    ));

    let mut policy = HttpTargetPolicy::new();
    policy.allow_internal("127.0.0.1", "127.0.0.1/32".parse()?, [address.port()])?;
    let client = HttpClient::restricted(HttpClientConfig::default(), policy)?;
    let mut mismatched_host = axum::http::HeaderMap::new();
    mismatched_host.insert(
        axum::http::header::HOST,
        axum::http::HeaderValue::from_static("localhost"),
    );
    assert!(matches!(
        client
            .get(&format!("http://{address}/"))
            .headers(mismatched_host)
            .send()
            .await,
        Err(HttpClientError::TargetRejected)
    ));
    assert!(matches!(
        client
            .get(&format!("http://localhost:{}/", address.port()))
            .send()
            .await,
        Err(HttpClientError::TargetRejected)
    ));
    assert!(matches!(
        client
            .get(&format!("http://{}/", address.port().saturating_add(1)))
            .send()
            .await,
        Err(HttpClientError::TargetRejected)
    ));
    let response = client.get(&format!("http://{address}/")).send().await?;
    assert_eq!(response.bytes().await?, b"restricted target reached");

    let response = client
        .get(&format!("http://{address}/redirect"))
        .bearer_auth("do-not-forward")
        .send()
        .await?;
    assert_eq!(
        response.status(),
        axum::http::StatusCode::TEMPORARY_REDIRECT
    );
    assert_eq!(redirected_hits.load(Ordering::SeqCst), 0);

    let proxy_config = HttpClientConfig {
        proxy: Some("http://127.0.0.1:8888".to_owned()),
        ..HttpClientConfig::default()
    };
    assert!(matches!(
        HttpClient::restricted(proxy_config, HttpTargetPolicy::new()),
        Err(HttpClientError::InvalidConfig)
    ));

    server.abort();
    redirected_server.abort();
    Ok(())
}

#[tokio::test]
async fn named_dependencies_bound_waiters_without_exhausting_each_other()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use tokio::sync::Notify;

    use openrust::http::{DependencyPolicy, HttpClientError};

    let slow_started = Arc::new(Notify::new());
    let slow_started_handler = Arc::clone(&slow_started);
    let app = axum::Router::new()
        .route(
            "/slow",
            get(move || {
                let started = Arc::clone(&slow_started_handler);
                async move {
                    started.notify_one();
                    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                    "slow"
                }
            }),
        )
        .route("/fast", get(|| async { "fast" }));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = HttpClient::new(HttpClientConfig::default())?;
    let policy = DependencyPolicy {
        max_in_flight: 1,
        max_waiting: 1,
        circuit_breaker: None,
    };
    let payments = client.dependency("payments", policy.clone())?;
    let payments_again = client.dependency("payments", policy)?;
    let analytics = client.dependency(
        "analytics",
        DependencyPolicy {
            max_in_flight: 1,
            max_waiting: 1,
            circuit_breaker: None,
        },
    )?;

    let first =
        tokio::spawn(async move { payments.get(&format!("http://{address}/slow")).send().await });
    slow_started.notified().await;
    let second = tokio::spawn(async move {
        payments_again
            .get(&format!("http://{address}/slow"))
            .send()
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert!(matches!(
        client
            .dependency(
                "payments",
                DependencyPolicy {
                    max_in_flight: 1,
                    max_waiting: 1,
                    circuit_breaker: None,
                }
            )?
            .get(&format!("http://{address}/fast"))
            .send()
            .await,
        Err(HttpClientError::DependencyOverloaded)
    ));
    assert_eq!(
        analytics
            .get(&format!("http://{address}/fast"))
            .send()
            .await?
            .status(),
        axum::http::StatusCode::OK
    );
    assert_eq!(first.await??.status(), axum::http::StatusCode::OK);
    assert_eq!(second.await??.status(), axum::http::StatusCode::OK);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn optional_circuit_breaker_opens_and_recovers_after_half_open_probe()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use openrust::http::{CircuitBreakerPolicy, DependencyPolicy, HttpClientError};

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_handler = Arc::clone(&calls);
    let app = axum::Router::new().route(
        "/{status}",
        get(
            move |axum::extract::Path(status): axum::extract::Path<u16>| {
                let calls = Arc::clone(&calls_handler);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    (
                        axum::http::StatusCode::from_u16(status)
                            .unwrap_or(axum::http::StatusCode::BAD_REQUEST),
                        "reply",
                    )
                }
            },
        ),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = HttpClient::new(HttpClientConfig::default())?;
    let dependency = client.dependency(
        "billing",
        DependencyPolicy {
            max_in_flight: 1,
            max_waiting: 1,
            circuit_breaker: Some(CircuitBreakerPolicy {
                failure_threshold: 1,
                reset_timeout: std::time::Duration::from_millis(20),
                half_open_max_calls: 1,
            }),
        },
    )?;

    assert_eq!(
        dependency
            .get(&format!("http://{address}/503"))
            .send()
            .await?
            .status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(matches!(
        dependency
            .get(&format!("http://{address}/200"))
            .send()
            .await,
        Err(HttpClientError::CircuitOpen)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert_eq!(
        dependency
            .get(&format!("http://{address}/200"))
            .send()
            .await?
            .status(),
        axum::http::StatusCode::OK
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn cancelling_a_queued_dependency_request_releases_its_queue_slot()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::Arc;
    use tokio::sync::Notify;

    use openrust::http::DependencyPolicy;

    let started = Arc::new(Notify::new());
    let started_handler = Arc::clone(&started);
    let app = axum::Router::new().route(
        "/slow",
        get(move || {
            let started = Arc::clone(&started_handler);
            async move {
                started.notify_one();
                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                "done"
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = HttpClient::new(HttpClientConfig::default())?;
    let policy = DependencyPolicy {
        max_in_flight: 1,
        max_waiting: 1,
        circuit_breaker: None,
    };
    let dependency = client.dependency("reports", policy)?;
    let first_dependency = dependency.clone();
    let first = tokio::spawn(async move {
        first_dependency
            .get(&format!("http://{address}/slow"))
            .send()
            .await
    });
    started.notified().await;
    let queued_dependency = dependency.clone();
    let queued = tokio::spawn(async move {
        queued_dependency
            .get(&format!("http://{address}/slow"))
            .send()
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    queued.abort();
    let _ = queued.await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let next_dependency = dependency.clone();
    let next = tokio::spawn(async move {
        next_dependency
            .get(&format!("http://{address}/slow"))
            .send()
            .await
    });
    assert_eq!(first.await??.status(), axum::http::StatusCode::OK);
    assert_eq!(next.await??.status(), axum::http::StatusCode::OK);
    server.abort();
    Ok(())
}

#[tokio::test]
async fn dependency_permit_covers_streaming_response_and_queue_wait_budget()
-> Result<(), Box<dyn std::error::Error>> {
    use openrust::http::{DependencyPolicy, HttpClientError};

    let app = axum::Router::new()
        .route(
            "/stream",
            get(|| async {
                axum::response::Response::new(axum::body::Body::from_stream(
                    futures_util::stream::pending::<Result<Vec<u8>, std::io::Error>>(),
                ))
            }),
        )
        .route("/fast", get(|| async { "ready" }));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = HttpClient::new(HttpClientConfig {
        timeout: std::time::Duration::from_millis(40),
        ..HttpClientConfig::default()
    })?;
    let dependency = client.dependency(
        "streaming-service",
        DependencyPolicy {
            max_in_flight: 1,
            max_waiting: 1,
            circuit_breaker: None,
        },
    )?;
    let response = dependency
        .get(&format!("http://{address}/stream"))
        .send()
        .await?;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert!(matches!(
        dependency
            .get(&format!("http://{address}/fast"))
            .send()
            .await,
        Err(HttpClientError::Timeout)
    ));
    drop(response);
    assert_eq!(
        dependency
            .get(&format!("http://{address}/fast"))
            .send()
            .await?
            .status(),
        axum::http::StatusCode::OK
    );
    server.abort();
    Ok(())
}
