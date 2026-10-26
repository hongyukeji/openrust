//! Verifies outbound HTTP support through the aggregate server package.
#![cfg(feature = "http-client")]

use axum::Json;
use axum::routing::get;
use openrust::http::{HttpClient, HttpClientConfig};
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
