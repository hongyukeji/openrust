//! The framework error type and its RFC 9457 rendering.

use std::collections::BTreeMap;

use axum_core::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use serde::Serialize;

use crate::config::ConfigError;

/// Result alias used throughout the framework and generated applications.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Field-level validation failures keyed by field path (`user.email`, `items[2].qty`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ValidationErrors(BTreeMap<String, Vec<String>>);

impl ValidationErrors {
    /// Creates an empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a failure message for `field`.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.0.entry(field.into()).or_default().push(message.into());
    }

    /// Returns `true` when no failure has been recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over `(field, messages)` pairs in field order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}

/// The single error type of the framework.
///
/// Every failure that can reach a handler, a command or a provider is expressed with this type,
/// so `?` works uniformly and HTTP rendering happens in exactly one place. Client errors carry a
/// message that is safe to show to callers; [`Error::Internal`] hides its source and is only logged.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Malformed request: unreadable body, wrong content type, bad parameters.
    #[error("{0}")]
    BadRequest(String),
    /// The caller is not authenticated.
    #[error("authentication required")]
    Unauthorized,
    /// The caller is authenticated but not allowed to perform the action.
    #[error("{0}")]
    Forbidden(String),
    /// The requested resource does not exist.
    #[error("{0}")]
    NotFound(String),
    /// The request conflicts with current state (duplicate key, stale version).
    #[error("{0}")]
    Conflict(String),
    /// Input was well-formed but failed validation rules.
    #[error("validation failed")]
    Validation(ValidationErrors),
    /// The caller exceeded a rate limit.
    #[error("too many requests")]
    RateLimited,
    /// Configuration could not be loaded or is invalid. Only occurs during bootstrap.
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
    /// Application-defined error with an explicit status and stable machine-readable code.
    #[error("{message}")]
    Custom {
        /// HTTP status to respond with.
        status: StatusCode,
        /// Stable `snake_case` code for clients to branch on.
        code: String,
        /// Human-readable detail safe to expose.
        message: String,
    },
    /// Unexpected failure. The source is logged; callers only see a generic message.
    #[error("internal error: {0}")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// Wraps any error as [`Error::Internal`].
    pub fn internal<E>(source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Internal(Box::new(source))
    }

    /// Builds an application-defined error with a stable `code`.
    pub fn custom(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Custom {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    /// HTTP status this error renders as.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Custom { status, .. } => *status,
            Self::Config(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable machine-readable code (`not_found`, `validation_failed`, …).
    pub fn code(&self) -> &str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Validation(_) => "validation_failed",
            Self::RateLimited => "rate_limited",
            Self::Config(_) => "configuration_error",
            Self::Custom { code, .. } => code,
            Self::Internal(_) => "internal_error",
        }
    }

    /// `true` for 5xx errors whose details must not leak to callers.
    pub fn is_server_error(&self) -> bool {
        self.status().is_server_error()
    }

    /// Renders the error as a problem document, hiding server-side details.
    pub fn to_problem(&self) -> ProblemDetails {
        let status = self.status();
        let detail = if self.is_server_error() {
            None
        } else {
            match self {
                Self::Validation(_) => Some("One or more fields failed validation.".to_owned()),
                other => Some(other.to_string()),
            }
        };
        let errors = match self {
            Self::Validation(errors) => Some(errors.clone()),
            _ => None,
        };
        ProblemDetails {
            type_uri: format!("urn:openrust:problem:{}", self.code()),
            title: status.canonical_reason().unwrap_or("Error").to_owned(),
            status: status.as_u16(),
            code: self.code().to_owned(),
            detail,
            errors,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Self::internal(source)
    }
}

impl From<ValidationErrors> for Error {
    fn from(errors: ValidationErrors) -> Self {
        Self::Validation(errors)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if self.is_server_error() {
            tracing::error!(error = %self, code = self.code(), "request failed");
        } else {
            tracing::debug!(error = %self, code = self.code(), "request rejected");
        }
        self.to_problem().into_response()
    }
}

/// RFC 9457 problem document. Serialized as `application/problem+json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProblemDetails {
    /// Problem type URI; OpenRust uses `urn:openrust:problem:<code>`.
    #[serde(rename = "type")]
    pub type_uri: String,
    /// Short human-readable summary, the HTTP reason phrase by default.
    pub title: String,
    /// HTTP status code.
    pub status: u16,
    /// Stable machine-readable code, mirrors [`Error::code`].
    pub code: String,
    /// Human-readable explanation specific to this occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Field-level validation failures, present for `validation_failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<ValidationErrors>,
}

impl IntoResponse for ProblemDetails {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = serde_json::to_vec(&self).unwrap_or_else(|_| b"{}".to_vec());
        let mut response = (status, body).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_errors_expose_detail_and_code() {
        let error = Error::NotFound("post 42 not found".into());
        let problem = error.to_problem();
        assert_eq!(problem.status, 404);
        assert_eq!(problem.code, "not_found");
        assert_eq!(problem.detail.as_deref(), Some("post 42 not found"));
        assert_eq!(problem.type_uri, "urn:openrust:problem:not_found");
    }

    #[test]
    fn internal_errors_hide_their_source() {
        let error = Error::internal(std::io::Error::other("database password is hunter2"));
        let problem = error.to_problem();
        assert_eq!(problem.status, 500);
        assert_eq!(problem.detail, None);
        assert!(
            !serde_json::to_string(&problem)
                .unwrap_or_default()
                .contains("hunter2")
        );
    }

    #[test]
    fn validation_errors_are_listed_per_field() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "must be a valid email");
        errors.add("email", "is already taken");
        let problem = Error::from(errors).to_problem();
        assert_eq!(problem.status, 422);
        let json = serde_json::to_value(&problem).unwrap_or_default();
        assert_eq!(json["errors"]["email"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn custom_errors_keep_status_and_code() {
        let error = Error::custom(
            StatusCode::PAYMENT_REQUIRED,
            "quota_exceeded",
            "plan limit reached",
        );
        assert_eq!(error.status(), StatusCode::PAYMENT_REQUIRED);
        assert_eq!(error.code(), "quota_exceeded");
        assert_eq!(
            error.to_problem().detail.as_deref(),
            Some("plan limit reached")
        );
    }

    #[test]
    fn problem_response_uses_problem_json_content_type() {
        let response = Error::Unauthorized.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/problem+json")
        );
    }
}
