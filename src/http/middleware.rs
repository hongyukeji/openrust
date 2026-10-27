//! 默认 middleware stack。
//!
//! 从最外层到最内层依次为：设置 `x-request-id` → 响应上下文 → 将 ID 写入响应 → tracing span →
//! CORS → 状态映射 → 请求超时 → 压缩 → panic 捕获。通过 [`Routes::layer`](crate::http::Routes::layer) 添加的应用 layer
//! 在此 stack 内运行。

use std::any::Any;
use std::time::Duration;

use crate::config::{ApiConfig, ConfigError, CorsConfig, ServerConfig};
use crate::core::{
    CancelOnDrop, Cancellation, Error, ExecutionContext, ExecutionMetadata, ResponseContext,
    Result, with_execution_context, with_response_context,
};
use crate::i18n::{I18n, with_request_locale};
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any as AnyOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

/// 使用默认 middleware stack 包装 `router`。
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
            ResponseSettings {
                api: api.clone(),
                request_timeout: Duration::from_secs(server.request_timeout_secs),
            },
            response_context,
        ))
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(axum::middleware::from_fn(normalize_request_id)))
}

#[derive(Clone)]
struct ResponseSettings {
    api: ApiConfig,
    request_timeout: Duration,
}

async fn normalize_request_id(mut request: Request, next: axum::middleware::Next) -> Response {
    let invalid_request_id = request.headers().get("x-request-id").is_some_and(|value| {
        value.to_str().map_or(true, |request_id| {
            request_id.is_empty()
                || request_id.len() > 128
                || !request_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        })
    });
    if invalid_request_id {
        request.headers_mut().remove("x-request-id");
    }
    if request
        .headers()
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            crate::core::ExecutionMetadata::default()
                .with_traceparent(value.to_owned())
                .is_err()
        })
        || request
            .headers()
            .get("traceparent")
            .is_some_and(|value| value.to_str().is_err())
    {
        request.headers_mut().remove("traceparent");
    }
    next.run(request).await
}

async fn response_context(
    State(settings): State<ResponseSettings>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    let locale_service = request
        .extensions()
        .get::<crate::core::AppState>()
        .and_then(crate::core::AppState::get::<I18n>);
    let server_readiness = request
        .extensions()
        .get::<crate::core::AppState>()
        .and_then(crate::core::AppState::get::<crate::core::Readiness>);
    let accept_language = request
        .headers()
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let locale = locale_service
        .as_ref()
        .map(|i18n| i18n.locale_for_request(accept_language.as_deref()));
    let mut metadata = ExecutionMetadata::default();
    if let Ok(updated) = metadata.clone().with_request_id(request_id.clone()) {
        metadata = updated;
    }
    if let Some(traceparent) = request
        .headers()
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        && let Ok(updated) = metadata.clone().with_traceparent(traceparent.to_owned())
    {
        metadata = updated;
    }
    if let Some(locale) = &locale {
        metadata = metadata.with_locale(locale.clone());
    }
    let cancellation = server_readiness
        .as_ref()
        .map_or_else(Cancellation::new, |readiness| {
            readiness.request_cancellation()
        });
    let cancel_on_drop = CancelOnDrop::new(cancellation.clone());
    let execution = ExecutionContext::new(
        metadata,
        Some(tokio::time::Instant::now() + settings.request_timeout),
        cancellation.clone(),
    );
    with_execution_context(execution, async move {
        let response = with_response_context(
            ResponseContext {
                request_id,
                http_status: settings.api.http_status,
            },
            async move {
                let _cancel_on_drop = cancel_on_drop;
                tokio::select! {
                    biased;
                    response = next.run(request) => response,
                    () = cancellation.cancelled() => Error::custom_with_status(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "server_draining",
                        "server is shutting down",
                    ).into_response(),
                }
            },
        );
        match (locale_service, locale) {
            (Some(i18n), Some(locale)) => {
                with_request_locale(i18n.as_ref().clone(), Some(locale.as_str()), response).await
            }
            _ => response.await,
        }
    })
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
    } else if response.status() == StatusCode::INTERNAL_SERVER_ERROR
        && response.headers().get(axum::http::header::CONTENT_TYPE)
            != Some(&axum::http::HeaderValue::from_static("application/json"))
    {
        Error::internal(UnwrappedInternalResponse).into_response()
    } else {
        response
    }
}

#[derive(Debug)]
struct UnwrappedInternalResponse;

impl std::fmt::Display for UnwrappedInternalResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an HTTP middleware returned an unwrapped internal error response")
    }
}

impl std::error::Error for UnwrappedInternalResponse {}

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
    let traceparent = request
        .headers()
        .get("traceparent")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-");
    tracing::info_span!(
        "request",
        method = %request.method(),
        uri = %request.uri(),
        request_id = %request_id,
        traceparent = %traceparent,
    )
}

#[derive(Debug, thiserror::Error)]
#[error("handler panicked: {0}")]
struct HandlerPanicked(String);

// `CatchPanicLayer::custom` 要求按值传入 payload。
#[allow(clippy::needless_pass_by_value)]
fn panic_response(payload: Box<dyn Any + Send + 'static>) -> Response {
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "non-string panic payload".to_owned());
    Error::internal(HandlerPanicked(message)).into_response()
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::{
        config::{ApiConfig, ServerConfig},
        core::ExecutionContext,
    };
    use axum::{Router, routing::get};
    use tower::ServiceExt;

    #[tokio::test]
    async fn request_context_accepts_only_valid_traceparent()
    -> Result<(), Box<dyn std::error::Error>> {
        let router = Router::new().route(
            "/",
            get(|| async {
                ExecutionContext::current()
                    .and_then(|context| context.metadata().traceparent().map(str::to_owned))
                    .unwrap_or_default()
            }),
        );
        let app = apply(router, &ServerConfig::default(), &ApiConfig::default())?;
        let valid = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("traceparent", valid)
                    .body(axum::body::Body::empty())?,
            )
            .await?;
        let body = axum::body::to_bytes(response.into_body(), 128).await?;
        assert_eq!(body.as_ref(), valid.as_bytes());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("traceparent", "not-a-traceparent")
                    .body(axum::body::Body::empty())?,
            )
            .await?;
        let body = axum::body::to_bytes(response.into_body(), 128).await?;
        assert!(body.is_empty());
        Ok(())
    }
}
