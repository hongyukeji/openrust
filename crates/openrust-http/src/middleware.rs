//! Default middleware stack.
//!
//! Outermost to innermost: set `x-request-id` → response context → propagate it to the response → tracing span →
//! CORS → status mapping → request timeout → compression → panic capture. Application layers added via [`Routes::layer`](crate::Routes::layer)
//! run inside this stack.

use std::any::Any;
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use openrust_core::config::{ApiConfig, ConfigError, CorsConfig, ServerConfig};
use openrust_core::{Error, ResponseContext, Result, with_response_context};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any as AnyOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

/// Wraps `router` in the default stack.
pub fn apply(router: Router, server: &ServerConfig, api: &ApiConfig) -> Result<Router> {
    let cors = if server.cors.enabled {
        Some(cors_layer(&server.cors)?)
    } else {
        None
    };
    let router = router.layer(CatchPanicLayer::custom(panic_response));
    let router = if server.compression.enabled {
        router.layer(CompressionLayer::new())
    } else {
        router
    };
    let router = router.layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(server.request_timeout_secs),
    ));
    let router = router.layer(axum::middleware::map_response(envelope_status));
    let router = if let Some(cors) = cors {
        router.layer(cors)
    } else {
        router
    };
    Ok(router
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(axum::middleware::from_fn_with_state(
            api.clone(),
            response_context,
        ))
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(axum::middleware::from_fn(normalize_request_id)))
}

async fn normalize_request_id(mut request: Request, next: axum::middleware::Next) -> Response {
    if request
        .headers()
        .get("x-request-id")
        .is_some_and(|value| value.to_str().is_err())
    {
        request.headers_mut().remove("x-request-id");
    }
    next.run(request).await
}

async fn response_context(
    State(api): State<ApiConfig>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    with_response_context(
        ResponseContext {
            request_id,
            http_status: api.http_status,
        },
        next.run(request),
    )
    .await
}

async fn envelope_status(response: Response) -> Response {
    if response.status() == StatusCode::REQUEST_TIMEOUT
        && !response
            .headers()
            .contains_key(axum::http::header::CONTENT_TYPE)
    {
        Error::custom_with_status(
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            "request timed out",
        )
        .into_response()
    } else if response.status() == StatusCode::PAYLOAD_TOO_LARGE
        && response.headers().get(axum::http::header::CONTENT_TYPE)
            != Some(&axum::http::HeaderValue::from_static("application/json"))
    {
        Error::custom_with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request body too large",
        )
        .into_response()
    } else {
        response
    }
}

fn invalid(key: &str, detail: &str) -> Error {
    ConfigError::Invalid(format!("{key}: {detail}")).into()
}

fn parse_list<T: std::str::FromStr>(values: &[String], key: &str) -> Result<Vec<T>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.parse().map_err(|_| {
                invalid(
                    &format!("server.cors.{key}[{index}]"),
                    &format!("invalid value {value:?}"),
                )
            })
        })
        .collect()
}

fn validate_wildcard(values: &[String], key: &str, allow_credentials: bool) -> Result<()> {
    if let Some(index) = values.iter().position(|value| value == "*") {
        let item_key = format!("server.cors.{key}[{index}]");
        if allow_credentials {
            return Err(invalid(
                &item_key,
                "cannot be \"*\" when server.cors.allow_credentials = true",
            ));
        }
        if values.len() != 1 {
            return Err(invalid(
                &item_key,
                "\"*\" cannot be mixed with concrete values",
            ));
        }
    }
    Ok(())
}

fn validate_origin(origin: &str, index: usize) -> Result<HeaderValue> {
    let key = format!("server.cors.allowed_origins[{index}]");
    let valid = origin.parse::<Uri>().is_ok_and(|uri| {
        matches!(uri.scheme_str(), Some("http" | "https"))
            && uri.authority().is_some_and(|authority| {
                let authority_text = authority.as_str();
                let has_port = if authority_text.starts_with('[') {
                    authority_text
                        .split_once(']')
                        .is_some_and(|(_, rest)| rest.starts_with(':'))
                } else {
                    authority_text.contains(':')
                };
                !authority.host().is_empty()
                    && !authority_text.contains('@')
                    && (!has_port || authority.port_u16().is_some())
                    && origin == format!("{}://{authority}", uri.scheme_str().unwrap_or_default())
            })
    });
    if !valid {
        return Err(invalid(&key, &format!("invalid origin {origin:?}")));
    }
    origin
        .parse()
        .map_err(|_| invalid(&key, &format!("invalid origin {origin:?}")))
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
        layer = layer.allow_origin(
            config
                .allowed_origins
                .iter()
                .enumerate()
                .map(|(index, origin)| validate_origin(origin, index))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    validate_wildcard(
        &config.allowed_methods,
        "allowed_methods",
        config.allow_credentials,
    )?;
    validate_wildcard(
        &config.allowed_headers,
        "allowed_headers",
        config.allow_credentials,
    )?;
    validate_wildcard(
        &config.expose_headers,
        "expose_headers",
        config.allow_credentials,
    )?;
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
