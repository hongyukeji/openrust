//! Default middleware stack.
//!
//! Outermost to innermost: set `x-request-id` → propagate it to the response → tracing span →
//! request timeout → compression → CORS → panic capture. Application layers added via [`Routes::layer`](crate::Routes::layer)
//! run inside this stack.

use std::any::Any;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use openrust_core::config::{ConfigError, CorsConfig, ServerConfig};
use openrust_core::{Error, Result};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any as AnyOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

/// Wraps `router` in the default stack.
pub fn apply(router: Router, server: &ServerConfig) -> Result<Router> {
    let cors = if server.cors.enabled {
        Some(cors_layer(&server.cors)?)
    } else {
        None
    };
    let router = router.layer(CatchPanicLayer::custom(panic_response));
    let router = if let Some(cors) = cors {
        router.layer(cors)
    } else {
        router
    };
    let router = if server.compression.enabled {
        router.layer(CompressionLayer::new())
    } else {
        router
    };
    Ok(router
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(server.request_timeout_secs),
        ))
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid)))
}

fn invalid(key: &str, detail: &str) -> Error {
    ConfigError::Invalid(format!("{key}: {detail}")).into()
}

fn parse_list<T: std::str::FromStr>(values: &[String], key: &str) -> Result<Vec<T>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .parse()
                .map_err(|_| invalid(&format!("server.cors.{key}[{index}]"), "invalid value"))
        })
        .collect()
}

fn cors_layer(config: &CorsConfig) -> Result<CorsLayer> {
    if config.allowed_origins.is_empty() {
        return Err(invalid(
            "server.cors.allowed_origins",
            "must not be empty when enabled",
        ));
    }
    let wildcard = config.allowed_origins.iter().any(|origin| origin == "*");
    if wildcard && config.allowed_origins.len() != 1 {
        return Err(invalid(
            "server.cors.allowed_origins",
            "* cannot be mixed with origins",
        ));
    }
    if wildcard && config.allow_credentials {
        return Err(invalid(
            "server.cors.allow_credentials",
            "cannot be true with server.cors.allowed_origins = [\"*\"]",
        ));
    }
    let mut layer = CorsLayer::new();
    if wildcard {
        layer = layer.allow_origin(AnyOrigin);
    } else {
        layer = layer.allow_origin(parse_list::<HeaderValue>(
            &config.allowed_origins,
            "allowed_origins",
        )?);
    }
    let methods = parse_list::<Method>(&config.allowed_methods, "allowed_methods")?;
    let headers = parse_list::<HeaderName>(&config.allowed_headers, "allowed_headers")?;
    let exposed = parse_list::<HeaderName>(&config.expose_headers, "expose_headers")?;
    layer = layer
        .allow_methods(methods)
        .allow_headers(headers)
        .expose_headers(exposed)
        .allow_credentials(config.allow_credentials)
        .max_age(Duration::from_secs(config.max_age_secs));
    Ok(layer)
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
