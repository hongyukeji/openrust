//! Framework errors and the shared JSON response envelope.

use std::collections::BTreeMap;

use crate::config::HttpStatusMode;
use axum_core::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use serde::Serialize;
use std::future::Future;

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
/// Every failure that can reach a handler, a command or a plugin is expressed with this type,
/// so `?` works uniformly and HTTP rendering happens in exactly one place. Client errors carry a
/// message that is safe to show to callers; [`Error::Internal`] hides its source and is only logged.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Malformed request: unreadable body or bad parameters.
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
    /// Parsed input failed structural validation with a readable reason.
    #[error("{detail}")]
    ValidationDetail {
        /// Explanation of the parse failure.
        detail: String,
        /// Field paths available from the parser.
        errors: ValidationErrors,
    },
    /// The caller exceeded a rate limit.
    #[error("too many requests")]
    RateLimited,
    /// The caller exceeded a rate limit; retry after the given seconds.
    #[error("too many requests")]
    RateLimitedWithRetryAfter(u64),
    /// Configuration could not be loaded or is invalid. Only occurs during bootstrap.
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
    /// Application-defined error with an optional semantic status and stable code.
    #[error("{message}")]
    Custom {
        /// HTTP status to respond with.
        status: Option<StatusCode>,
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
    pub fn custom(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Custom {
            status: None,
            code: code.into(),
            message: message.into(),
        }
    }

    /// Builds an application-defined error with an explicit semantic HTTP status.
    pub fn custom_with_status(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Custom {
            status: Some(status),
            code: code.into(),
            message: message.into(),
        }
    }

    /// Builds a rate-limit error carrying a `Retry-After` delay in seconds.
    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self::RateLimitedWithRetryAfter(retry_after_secs)
    }

    /// Builds a structural validation error with a readable detail.
    pub fn validation_with_detail(detail: impl Into<String>, errors: ValidationErrors) -> Self {
        Self::ValidationDetail {
            detail: detail.into(),
            errors,
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
            Self::Validation(_) | Self::ValidationDetail { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited | Self::RateLimitedWithRetryAfter(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Custom { status, .. } => status.unwrap_or(StatusCode::BAD_REQUEST),
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
            Self::Validation(_) | Self::ValidationDetail { .. } => "validation_failed",
            Self::RateLimited | Self::RateLimitedWithRetryAfter(_) => "rate_limited",
            Self::Config(_) => "configuration_error",
            Self::Custom { code, .. } => code,
            Self::Internal(_) => "internal_error",
        }
    }

    /// `true` for 5xx errors whose details must not leak to callers.
    pub fn is_server_error(&self) -> bool {
        self.status().is_server_error()
    }

    /// Renders the error in the common JSON envelope, hiding server details.
    pub fn to_api_response(&self) -> ApiResponse<()> {
        let message = if self.is_server_error() {
            "internal server error".to_owned()
        } else {
            self.to_string()
        };
        let errors = match self {
            Self::Validation(errors) => Some(errors.clone()),
            Self::ValidationDetail { errors, .. } if !errors.is_empty() => Some(errors.clone()),
            _ => None,
        };
        ApiResponse::failure(self.code(), message, self.status(), errors)
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
        let mut response = self.to_api_response().into_response();
        if let Self::RateLimitedWithRetryAfter(seconds) = self
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

/// Per-request response context set by the HTTP middleware.
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// Correlation ID shared by the response header and body.
    pub request_id: String,
    /// Configured HTTP status policy.
    pub http_status: HttpStatusMode,
}

tokio::task_local! {
    static RESPONSE_CONTEXT: ResponseContext;
}

/// Runs a request future with its immutable rendering context.
pub async fn with_response_context<F: Future>(context: ResponseContext, future: F) -> F::Output {
    RESPONSE_CONTEXT.scope(context, future).await
}

fn response_context() -> ResponseContext {
    RESPONSE_CONTEXT
        .try_with(Clone::clone)
        .unwrap_or(ResponseContext {
            request_id: "-".to_owned(),
            http_status: HttpStatusMode::AlwaysOk,
        })
}

/// Returns the status policy active for the current request.
pub fn current_http_status_mode() -> HttpStatusMode {
    response_context().http_status
}

/// JSON envelope shared by successful and failed API responses.
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T: Serialize> {
    /// Whether the operation succeeded.
    pub success: bool,
    /// Stable machine-readable code.
    pub code: String,
    /// Message safe to display to the caller.
    pub message: String,
    /// Response payload; `null` for errors and empty results.
    pub data: Option<T>,
    /// Pagination metadata when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    /// Field-level validation errors when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<ValidationErrors>,
    /// Correlation ID, equal to the `x-request-id` header.
    pub request_id: String,
    #[serde(skip)]
    status: StatusCode,
    #[serde(skip)]
    mode: HttpStatusMode,
}

impl<T: Serialize> ApiResponse<T> {
    /// Wraps a successful payload.
    pub fn success(data: T) -> Self {
        let context = response_context();
        Self {
            success: true,
            code: "ok".to_owned(),
            message: "ok".to_owned(),
            data: Some(data),
            meta: None,
            errors: None,
            request_id: context.request_id,
            status: StatusCode::OK,
            mode: context.http_status,
        }
    }

    /// Adds pagination metadata to a successful response.
    #[must_use]
    pub fn with_meta<M: Serialize>(mut self, meta: M) -> Self {
        self.meta = serde_json::to_value(meta).ok();
        self
    }

    /// Sets the semantic HTTP status, used for created results.
    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    fn failure(
        code: &str,
        message: String,
        status: StatusCode,
        errors: Option<ValidationErrors>,
    ) -> Self {
        let context = response_context();
        Self {
            success: false,
            code: code.to_owned(),
            message,
            data: None,
            meta: None,
            errors,
            request_id: context.request_id,
            status,
            mode: context.http_status,
        }
    }
}

impl ApiResponse<()> {
    /// Builds a successful response with `data: null`.
    pub fn empty() -> Self {
        let mut response = Self::success(());
        response.data = None;
        response
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let request_id = HeaderValue::from_str(&self.request_id).ok();
        let mode = self.mode;
        let (semantic_status, body) = match serde_json::to_vec(&self) {
            Ok(body) => (self.status, body),
            Err(error) => {
                tracing::error!(%error, "response serialization failed");
                let mut fallback = Self::failure(
                    "internal_error",
                    "internal server error".to_owned(),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    None,
                );
                fallback.request_id.clone_from(&self.request_id);
                let body = serde_json::to_vec(&fallback).unwrap_or_else(|_| {
                    b"{\"success\":false,\"code\":\"internal_error\",\"message\":\"internal server error\",\"data\":null,\"request_id\":\"-\"}".to_vec()
                });
                (StatusCode::INTERNAL_SERVER_ERROR, body)
            }
        };
        let status = if mode == HttpStatusMode::AlwaysOk {
            StatusCode::OK
        } else {
            semantic_status
        };
        let mut response = (status, body).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if let Some(request_id) = request_id {
            response.headers_mut().insert("x-request-id", request_id);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_error_has_code_and_message() {
        let response = Error::NotFound("post 42 not found".into()).to_api_response();
        assert_eq!(response.code, "not_found");
        assert_eq!(response.message, "post 42 not found");
        assert!(!response.success);
    }

    #[test]
    fn internal_error_hides_source() {
        let response = Error::internal(std::io::Error::other("secret")).to_api_response();
        assert_eq!(response.message, "internal server error");
        assert!(
            !serde_json::to_string(&response)
                .unwrap_or_default()
                .contains("secret")
        );
    }

    #[test]
    fn validation_errors_are_listed_per_field() {
        let mut errors = ValidationErrors::new();
        errors.add("email", "invalid");
        let response = Error::Validation(errors).to_api_response();
        assert_eq!(response.errors.as_ref().map(|e| e.iter().count()), Some(1));
    }

    #[test]
    fn custom_status_defaults_to_bad_request() {
        assert_eq!(
            Error::custom("quota_exceeded", "limit reached").status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            Error::custom_with_status(
                StatusCode::PAYMENT_REQUIRED,
                "quota_exceeded",
                "limit reached"
            )
            .status(),
            StatusCode::PAYMENT_REQUIRED
        );
    }
}
