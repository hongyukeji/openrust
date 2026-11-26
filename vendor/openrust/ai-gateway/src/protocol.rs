use std::{convert::Infallible, fmt::Write as _};

use eventsource_stream::Eventsource as _;
use futures_util::StreamExt as _;
use http::{HeaderMap, HeaderValue, header};
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client},
    routing::{RouteGroup, RoutingError},
    support::facades::{Facade, RateLimiter, Route},
};
use openrust_ai::{AiError, TextEndpoint};
use serde_json::{Value, json};

use crate::gateway::{Call, Gateway};

type HttpResult = Result<Response, HttpError>;

pub(crate) fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().prefix("/v1"), |router| {
        router.get("/models", models)?;
        router.post("/chat/completions", chat_completions)?;
        router.post("/responses", responses)?;
        Ok(())
    })
}

fn models(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        let gateway = Gateway::get_facade_root()?;
        let Some(key) = bearer(request).and_then(|token| gateway.authorize(token)) else {
            return unauthorized();
        };
        let data: Vec<Value> = gateway
            .models
            .keys()
            .filter(|name| key.models.contains(name))
            .map(
                |name| json!({"id": name, "object": "model", "created": 0, "owned_by": "openrust"}),
            )
            .collect();
        Response::json(StatusCode::OK, json!({"object": "list", "data": data})).map(|response| {
            response.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        })
    })
}

fn chat_completions(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::ChatCompletions))
}

fn responses(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::Responses))
}

async fn relay(request: &mut Request, endpoint: TextEndpoint) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let Some(key) = bearer(request).and_then(|token| gateway.authorize(token)) else {
        return unauthorized();
    };
    let body = match request.body_bytes().await {
        Ok(body) => body,
        Err(HttpError::PayloadTooLarge) => {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "请求正文超出大小限制",
            );
        }
        Err(_) => {
            return error(StatusCode::BAD_REQUEST, "invalid_json", "无法读取请求正文");
        }
    };
    // 协议正文保留提示词和工具参数原文，绕开应用对 JSON 输入的清理。
    let Ok(Value::Object(mut data)) = serde_json::from_slice(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "请求正文必须是 JSON 对象",
        );
    };
    let Some(name) = data
        .get("model")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
    else {
        return error(StatusCode::BAD_REQUEST, "missing_model", "必须指定模型");
    };
    let Some(model) = gateway
        .models
        .get(&name)
        .filter(|_| key.models.contains(&name))
    else {
        return error(StatusCode::NOT_FOUND, "model_not_found", "模型不可用");
    };
    if !model.provider.supports_endpoint(endpoint) {
        return error(
            StatusCode::BAD_REQUEST,
            "unsupported_endpoint",
            "该模型不支持此端点",
        );
    }
    let streaming = match data.get("stream") {
        None | Some(Value::Bool(false) | Value::Null) => false,
        Some(Value::Bool(true)) => true,
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_stream",
                "stream 必须为布尔值",
            );
        }
    };
    if endpoint == TextEndpoint::Responses {
        // 共享上游 Key 暂不承载跨请求资源，防止不同下游 Key 通过资源 ID 互访。
        if ["store", "background"].iter().any(|field| {
            !matches!(
                data.get(*field),
                None | Some(Value::Null | Value::Bool(false))
            )
        }) || ["previous_response_id", "conversation", "prompt"]
            .iter()
            .any(|field| data.get(*field).is_some_and(|value| !value.is_null()))
            || data
                .get("input")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().any(|item| item["type"] == "item_reference"))
            || data
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| {
                    tools
                        .iter()
                        .any(|tool| !matches!(tool["type"].as_str(), Some("function" | "custom")))
                })
        {
            return error(
                StatusCode::BAD_REQUEST,
                "unsupported_state",
                "此端点仅支持无状态请求及客户端工具",
            );
        }
        data.insert("store".into(), Value::Bool(false));
    }
    if let Some(limit) = key.requests_per_minute {
        let limiter = RateLimiter::get_facade_root()?;
        match limiter
            .reserve(&format!("ai-gateway:{}", key.name), limit, 60)
            .await
        {
            Ok(Some(seconds)) => {
                return error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "rate_limit_exceeded",
                    "已达到每分钟请求限额",
                )
                .map(|response| response.header(header::RETRY_AFTER, HeaderValue::from(seconds)));
            }
            Ok(None) => {}
            Err(_) => {
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "rate_limit_unavailable",
                    "限流服务暂不可用",
                );
            }
        }
    }
    data.insert("model".into(), json!(model.upstream_model));
    let mut call = gateway.usage(key, &name, model, endpoint);
    let upstream = match model.provider.relay(endpoint, Value::Object(data)).await {
        Ok(response) => response,
        Err(AiError::Timeout) => {
            call.outcome("timeout");
            call.status(StatusCode::GATEWAY_TIMEOUT);
            return error(
                StatusCode::GATEWAY_TIMEOUT,
                "upstream_timeout",
                "上游请求超时",
            );
        }
        Err(_) => {
            call.outcome("upstream_error");
            call.status(StatusCode::BAD_GATEWAY);
            return error(StatusCode::BAD_GATEWAY, "upstream_error", "上游连接失败");
        }
    };
    let status = upstream.status();
    call.status(status);
    let headers = downstream_headers(upstream.headers());
    if !status.is_success() {
        call.outcome("upstream_error");
        let status = if status.is_client_error() || status.is_server_error() {
            status
        } else {
            StatusCode::BAD_GATEWAY
        };
        return error(status, "upstream_error", "上游拒绝或未能处理该请求")
            .map(|response| response.with_headers(headers));
    }
    if streaming {
        return stream(upstream, headers, name, call);
    }
    let Ok(mut data) = upstream.json::<Value>() else {
        call.outcome("invalid_upstream_response");
        return error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回有效 JSON",
        );
    };
    call.observe_usage(&data);
    call.outcome("completed");
    rename_model(&mut data, &name);
    Response::json(status, data).map(|response| response.with_headers(headers))
}

fn stream(
    upstream: client::Response,
    mut headers: HeaderMap,
    name: String,
    mut call: Call,
) -> HttpResult {
    let status = upstream.status();
    let is_sse = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));
    if !is_sse {
        call.outcome("invalid_upstream_response");
        return error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回 SSE",
        );
    }
    let Ok(body) = upstream.into_stream() else {
        call.outcome("invalid_upstream_response");
        return error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "无法读取上游 SSE",
        );
    };
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    let stream = async_stream::stream! {
        let mut events = body.eventsource();
        let mut terminal = false;
        while let Some(event) = events.next().await {
            let Ok(event) = event else {
                call.outcome("stream_error");
                yield Ok::<_, Infallible>(stream_error());
                return;
            };
            if event.data == "[DONE]" {
                call.outcome("completed");
                yield Ok("data: [DONE]\n\n".to_owned());
                return;
            }
            let Ok(mut data) = serde_json::from_str::<Value>(&event.data) else {
                call.outcome("stream_error");
                yield Ok(stream_error());
                return;
            };
            if matches!(data["type"].as_str(), Some("error" | "response.failed")) || event.event == "error" || data.get("error").is_some_and(|value| !value.is_null()) {
                call.outcome("upstream_error");
                yield Ok(stream_error());
                return;
            }
            terminal |= matches!(data["type"].as_str(), Some("response.completed" | "response.incomplete"));
            call.observe_usage(&data);
            rename_model(&mut data, &name);
            if let Some(response) = data.get_mut("response") {
                call.observe_usage(response);
                rename_model(response, &name);
            }
            let mut frame = String::new();
            if event.event != "message" {
                let _ = writeln!(frame, "event: {}", event.event);
            }
            if !event.id.is_empty() {
                let _ = writeln!(frame, "id: {}", event.id);
            }
            if let Some(retry) = event.retry {
                let _ = writeln!(frame, "retry: {}", retry.as_millis());
            }
            let _ = writeln!(frame, "data: {data}\n");
            yield Ok(frame);
        }
        call.outcome(if terminal { "completed" } else { "incomplete_stream" });
        if !terminal {
            yield Ok(stream_error());
        }
    };
    Ok(Response::stream(status, stream).with_headers(headers))
}

fn rename_model(data: &mut Value, name: &str) {
    if let Some(model) = data.get_mut("model").filter(|model| model.is_string()) {
        *model = Value::String(name.into());
    }
}

fn bearer(request: &Request) -> Option<&str> {
    let mut values = request.headers().get_all(header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("Bearer")
        && !token.is_empty()
        && !token.contains(',')
        && !token.chars().any(char::is_whitespace))
    .then_some(token)
}

fn downstream_headers(upstream: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(value) = upstream.get(header::RETRY_AFTER) {
        headers.insert(header::RETRY_AFTER, value.clone());
    }
    headers
}

fn unauthorized() -> HttpResult {
    error(
        StatusCode::UNAUTHORIZED,
        "invalid_api_key",
        "凭证缺失或无效",
    )
    .map(|response| response.header(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer")))
}

fn error(status: StatusCode, code: &str, message: &str) -> HttpResult {
    Response::json(status, error_body(code, message)).map(|response| {
        response.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
    })
}

fn error_body(code: &str, message: &str) -> Value {
    let kind = match code {
        "invalid_api_key" => "authentication_error",
        "rate_limit_exceeded" => "rate_limit_error",
        "upstream_timeout"
        | "upstream_error"
        | "invalid_upstream_response"
        | "rate_limit_unavailable" => "server_error",
        _ => "invalid_request_error",
    };
    json!({"error": {"message": message, "type": kind, "code": code, "param": null}})
}

fn stream_error() -> String {
    format!(
        "event: error\ndata: {}\n\n",
        error_body("upstream_error", "上游流未能完成")
    )
}
