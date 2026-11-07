//! 防止第三方存储领域失败被误当500，或把基础设施故障伪装成凭据不存在。

use super::storage_error;
use crate::{GrantOperation as O, GrantStorageError as E};
use openrust::http::axum::{body::to_bytes, http::StatusCode, response::IntoResponse};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn typed_storage_failures_have_distinct_protocol_classes_and_statuses() -> TestResult {
    for (error, operation, code, status) in [
        (
            E::ClientChanged,
            O::Redeem,
            "invalid_client",
            StatusCode::UNAUTHORIZED,
        ),
        (
            E::CodeRevoked,
            O::Redeem,
            "invalid_grant",
            StatusCode::BAD_REQUEST,
        ),
        (
            E::RefreshRevoked,
            O::Refresh,
            "invalid_grant",
            StatusCode::BAD_REQUEST,
        ),
        (
            E::Expired,
            O::Redeem,
            "invalid_grant",
            StatusCode::BAD_REQUEST,
        ),
        (
            E::Expired,
            O::Refresh,
            "invalid_grant",
            StatusCode::BAD_REQUEST,
        ),
        (
            E::BindingMismatch,
            O::Redeem,
            "invalid_grant",
            StatusCode::BAD_REQUEST,
        ),
        (
            E::AuthorityDenied,
            O::Authorize,
            "access_denied",
            StatusCode::UNAUTHORIZED,
        ),
        (
            E::AuthorityDenied,
            O::Redeem,
            "invalid_grant",
            StatusCode::BAD_REQUEST,
        ),
        (
            E::AuthorityDenied,
            O::Refresh,
            "invalid_grant",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = storage_error(error, operation, None).into_response();
        assert_eq!(response.status(), status);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
        assert_eq!(body["error"], code);
    }
    Ok(())
}

#[tokio::test]
async fn unavailable_failure_does_not_expose_source_or_claim_invalid_credentials() -> TestResult {
    use std::error::Error as _;
    let error = E::Unavailable(openrust::Error::custom(
        "driver-failed",
        "private-binding-db-detail",
    ));
    assert!(!format!("{error:?} {error}").contains("private-binding-db-detail"));
    assert!(error.source().is_some());
    let response = storage_error(
        error,
        O::Refresh,
        Some(b"Basic private-client-value".as_slice()),
    )
    .into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = to_bytes(response.into_body(), 4096).await?;
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["error"], "server_error");
    let text = std::str::from_utf8(&bytes)?;
    assert!(!text.contains("private-binding-db-detail"));
    assert!(!text.contains("private-client-value"));
    assert!(!text.contains("driver-failed"));
    Ok(())
}
