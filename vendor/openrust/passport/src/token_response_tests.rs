//! OAuth 顶层成功载荷、固定类型和最终 HTTP 缓存头。

use super::*;
use openrust::http::axum::{body::to_bytes, http::StatusCode};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn getters_expose_immutable_standard_fields() {
    let response = TokenResponse::new("access-value".into(), 3600, Some("refresh-value".into()));
    assert_eq!(response.token_type(), "Bearer");
    assert_eq!(response.expires_in(), 3600);
    assert_eq!(response.access_token(), "access-value");
    assert_eq!(response.refresh_token(), Some("refresh-value"));
}

#[tokio::test]
async fn response_is_top_level_oauth_json_with_final_cache_headers() -> TestResult {
    let response = TokenResponse::new("access-value".into(), 3600, Some("refresh-value".into()))
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/json; charset=UTF-8"
    );
    assert_eq!(response.headers()[header::PRAGMA], "no-cache");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "no-store, private"
    );
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let payload: Value = serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
    assert_eq!(
        payload,
        json!({"token_type": "Bearer", "expires_in": 3600, "access_token": "access-value", "refresh_token": "refresh-value"})
    );
    assert!(payload.get("data").is_none());
    assert!(payload.get("id_token").is_none());
    Ok(())
}

#[tokio::test]
async fn absent_refresh_token_is_omitted_instead_of_null() -> TestResult {
    let response = TokenResponse::new("access-only".into(), 0, None);
    assert_eq!(response.refresh_token(), None);
    let serialized = serde_json::to_value(&response)?;
    assert_eq!(
        serialized,
        json!({"token_type": "Bearer", "expires_in": 0, "access_token": "access-only"})
    );
    let payload: Value =
        serde_json::from_slice(&to_bytes(response.into_response().into_body(), 4096).await?)?;
    assert_eq!(payload, serialized);
    assert!(payload.get("refresh_token").is_none());
    Ok(())
}
