//! 本地 OAuth 实际 HTTP 响应的状态、字段、认证头与错误分类。

use super::*;
use openrust::http::axum::{body::to_bytes, http::HeaderMap};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn response(
    error: OAuthError,
) -> Result<(StatusCode, HeaderMap, Value), Box<dyn std::error::Error>> {
    let (parts, body) = error.into_response().into_parts();
    assert_eq!(
        parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    assert!(parts.headers.get(header::CACHE_CONTROL).is_none());
    assert!(parts.headers.get(header::PRAGMA).is_none());
    assert!(parts.headers.get(header::LOCATION).is_none());
    let body = to_bytes(body, 16_384).await?;
    let payload: Value = serde_json::from_slice(&body)?;
    assert!(payload.get("data").is_none());
    assert!(payload.get("status").is_none());
    assert!(payload.get("authenticate").is_none());
    Ok((parts.status, parts.headers, payload))
}

#[tokio::test]
async fn invalid_request_preserves_default_hint_and_protocol_description() -> TestResult {
    let (status, headers, body) =
        response(OAuthError::invalid_request("code_verifier", None)).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({"error":"invalid_request","error_description":"The request is missing a required parameter, includes an invalid parameter value, includes a parameter more than once, or is otherwise malformed.","hint":"Check the `code_verifier` parameter"})
    );
    assert!(headers.get(header::WWW_AUTHENTICATE).is_none());
    Ok(())
}

#[tokio::test]
async fn explicit_empty_request_hint_is_not_removed_or_defaulted() -> TestResult {
    let (_, _, body) = response(OAuthError::invalid_request("code", Some(""))).await?;
    assert_eq!(body["hint"], "");
    let (_, _, body) = response(OAuthError::invalid_request(
        "code",
        Some("Authorization code malformed"),
    ))
    .await?;
    assert_eq!(body["hint"], "Authorization code malformed");
    Ok(())
}

#[tokio::test]
async fn invalid_client_authenticate_follows_exact_nonempty_case_sensitive_prefix() -> TestResult {
    for (authorization, expected) in [
        (None, None),
        (Some(""), None),
        (
            Some("Bearer secret-sentinel"),
            Some("Bearer realm=\"OAuth\""),
        ),
        (Some("Bearer"), Some("Bearer realm=\"OAuth\"")),
        (Some("BearerSuffix"), Some("Bearer realm=\"OAuth\"")),
        (
            Some("bearer secret-sentinel"),
            Some("Basic realm=\"OAuth\""),
        ),
        (
            Some(" Bearer secret-sentinel"),
            Some("Basic realm=\"OAuth\""),
        ),
        (Some(" "), Some("Basic realm=\"OAuth\"")),
        (Some("Basic secret-sentinel"), Some("Basic realm=\"OAuth\"")),
    ] {
        let (status, headers, body) =
            response(OAuthError::invalid_client(authorization.map(str::as_bytes))).await?;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            headers
                .get(header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok()),
            expected
        );
        assert_eq!(
            body,
            json!({"error":"invalid_client","error_description":"Client authentication failed"})
        );
        assert!(!body.to_string().contains("secret-sentinel"));
    }
    Ok(())
}

#[tokio::test]
async fn invalid_grant_keeps_even_empty_hint() -> TestResult {
    let (status, headers, body) = response(OAuthError::invalid_grant("")).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({"error":"invalid_grant","error_description":"The provided authorization grant (e.g., authorization code, resource owner credentials) or refresh token is invalid, expired, revoked, does not match the redirection URI used in the authorization request, or was issued to another client.","hint":""})
    );
    assert!(headers.get(header::WWW_AUTHENTICATE).is_none());
    Ok(())
}

#[tokio::test]
async fn optional_hints_distinguish_none_from_explicit_empty() -> TestResult {
    for hint in [None, Some(""), Some("safe-protocol-hint")] {
        for error in [
            OAuthError::invalid_refresh_token(hint),
            OAuthError::access_denied(hint),
            OAuthError::unauthorized_client(hint),
        ] {
            let (_, headers, body) = response(error).await?;
            assert_eq!(body.get("hint"), hint.map(Value::from).as_ref());
            assert!(headers.get(header::WWW_AUTHENTICATE).is_none());
        }
    }
    Ok(())
}

#[tokio::test]
async fn refresh_denial_and_unauthorized_client_keep_distinct_status_and_description() -> TestResult
{
    for (error, status, kind, description) in [
        (
            OAuthError::invalid_refresh_token(None),
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "The refresh token is invalid.",
        ),
        (
            OAuthError::access_denied(None),
            StatusCode::UNAUTHORIZED,
            "access_denied",
            "The resource owner or authorization server denied the request.",
        ),
        (
            OAuthError::unauthorized_client(None),
            StatusCode::BAD_REQUEST,
            "unauthorized_client",
            "The authenticated client is not authorized to use this authorization grant type.",
        ),
    ] {
        let (actual, _, body) = response(error).await?;
        assert_eq!(actual, status);
        assert_eq!(body, json!({"error":kind,"error_description":description}));
    }
    Ok(())
}

#[tokio::test]
async fn unsupported_grant_and_server_error_use_fixed_distinct_payloads() -> TestResult {
    let (status, _, body) = response(OAuthError::unsupported_grant_type()).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        json!({"error":"unsupported_grant_type","error_description":"The authorization grant type is not supported by the authorization server.","hint":"Check that all required parameters have been provided"})
    );
    let (status, _, body) = response(OAuthError::server_error("Safe static failure")).await?;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({"error":"server_error","error_description":"The authorization server encountered an unexpected condition which prevented it from fulfilling the request: Safe static failure"})
    );
    Ok(())
}

#[tokio::test]
async fn pkce_request_errors_have_exact_parameter_specific_hints() -> TestResult {
    for (error, hint) in [
        (
            PkceError::MissingChallenge,
            "Code challenge must be provided for public clients",
        ),
        (
            PkceError::InvalidChallenge,
            "Code challenge must follow the specifications of RFC-7636.",
        ),
        (
            PkceError::InvalidMethod,
            "Code challenge method must be one of `S256`, `plain`",
        ),
        (
            PkceError::MissingVerifier,
            "Check the `code_verifier` parameter",
        ),
        (
            PkceError::InvalidVerifier,
            "Code Verifier must follow the specifications of RFC-7636.",
        ),
        (
            PkceError::UnexpectedVerifier,
            "code_verifier received when no code_challenge is present",
        ),
    ] {
        let (status, headers, body) = response(error.into()).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
        assert_eq!(body["hint"], hint);
        assert!(headers.get(header::WWW_AUTHENTICATE).is_none());
    }
    Ok(())
}

#[tokio::test]
async fn pkce_mismatch_is_invalid_grant_while_internal_failures_are_server_error() -> TestResult {
    let (status, _, body) = response(PkceError::Mismatch.into()).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_grant");
    assert_eq!(body["hint"], "Failed to verify `code_verifier`.");
    for (error, safe_description) in [
        (
            PkceError::UnsupportedStoredMethod,
            "Unsupported stored code challenge method",
        ),
        (
            PkceError::InternalValidation,
            "Code challenge validation failed",
        ),
    ] {
        let (status, headers, body) = response(error.into()).await?;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "server_error");
        assert_eq!(
            body["error_description"],
            format!(
                "The authorization server encountered an unexpected condition which prevented it from fulfilling the request: {safe_description}"
            )
        );
        assert!(body.get("hint").is_none());
        assert!(headers.get(header::WWW_AUTHENTICATE).is_none());
    }
    Ok(())
}
