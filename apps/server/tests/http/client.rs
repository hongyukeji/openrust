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
