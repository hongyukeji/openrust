//! Default middleware stack.
//!
//! Outermost to innermost: set `x-request-id` → propagate it to the response → tracing span →
//! request timeout → panic capture. Application layers added via [`Routes::layer`](crate::Routes::layer)
//! run inside this stack.

use std::any::Any;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use openrust_core::Error;
use openrust_core::config::ServerConfig;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

/// Wraps `router` in the default stack.
pub fn apply(router: Router, server: &ServerConfig) -> Router {
    router
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(server.request_timeout_secs),
        ))
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

fn request_span(request: &Request) -> tracing::Span {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-");
    tracing::info_span!(
        "request",
        method = %request.method(),
        uri = %request.uri(),
        request_id = %request_id,
    )
}

#[derive(Debug, thiserror::Error)]
#[error("handler panicked: {0}")]
struct HandlerPanicked(String);

// `CatchPanicLayer::custom` requires the payload by value.
#[allow(clippy::needless_pass_by_value)]
fn panic_response(payload: Box<dyn Any + Send + 'static>) -> Response {
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "non-string panic payload".to_owned());
    Error::internal(HandlerPanicked(message)).into_response()
}
