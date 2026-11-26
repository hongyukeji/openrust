use std::{convert::Infallible, fmt::Write as _};

use eventsource_stream::Eventsource as _;
use futures_util::StreamExt as _;
use http::{HeaderMap, HeaderValue, header};
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client},
    routing::{RouteGroup, RoutingError},
    support::{
        Str,
        facades::{Facade, RateLimiter, Route},
    },
};
use openrust_ai::{AiError, TextEndpoint};
use serde_json::{Value, json};

use crate::{
    compatibility::Bridge,
    gateway::{Call, Gateway, Key, Target},
};

type HttpResult = Result<Response, HttpError>;

pub(crate) fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().prefix("/v1"), |router| {
        router.get("/models", models)?;
        router.get("/models/{model}", model)?;
        router.post("/chat/completions", chat_completions)?;
        router.post("/responses", responses)?;
        router.post("/embeddings", embeddings)?;
        router.post("/messages", messages)?;
        router.post("/messages/count_tokens", count_message_tokens)?;
        Ok(())
    })?;
    Route::group(RouteGroup::new().prefix("/v1beta"), |router| {
        router.get("/models", gemini_models)?;
        router.get("/models/{model}", gemini_model)?;
        router.post("/models/{model}", gemini_operation)?;
        Ok(())
    })
}

fn gemini_models(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move { catalog(request, None, Protocol::Gemini) })
}

fn gemini_model(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move { catalog(request, request.route("model"), Protocol::Gemini) })
}

fn gemini_operation(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        let name_and_action = request
            .route("model")
            .and_then(|value| value.rsplit_once(':'));
        let Some((model, action)) = name_and_action else {
            return Protocol::Gemini.error(
                StatusCode::NOT_FOUND,
                "unsupported_endpoint",
                "未知模型操作",
            );
        };
        let endpoint = match action {
            "generateContent" => TextEndpoint::GeminiGenerateContent,
            "streamGenerateContent" => TextEndpoint::GeminiStreamGenerateContent,
            "countTokens" => TextEndpoint::GeminiCountTokens,
            _ => {
                return Protocol::Gemini.error(
                    StatusCode::NOT_FOUND,
                    "unsupported_endpoint",
                    "未知模型操作",
                );
            }
        };
        relay(request, endpoint, Some(model.to_owned())).await
    })
}

fn models(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move { catalog(request, None, catalog_protocol(request)) })
}

fn model(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move { catalog(request, request.route("model"), catalog_protocol(request)) })
}

fn catalog_protocol(request: &Request) -> Protocol {
    if request.has_header("x-api-key") {
        Protocol::Anthropic
    } else {
        Protocol::OpenAi
    }
}

fn catalog(request: &Request, id: Option<&str>, protocol: Protocol) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let Some(key) = protocol
        .token(request)
        .and_then(|token| gateway.authorize(token))
    else {
        return protocol.unauthorized();
    };
    let mut accessible = gateway.models.iter().filter(|(name, model)| {
        key.models.contains(name)
            && match protocol {
                Protocol::OpenAi => [
                    TextEndpoint::ChatCompletions,
                    TextEndpoint::Responses,
                    TextEndpoint::Embeddings,
                ]
                .into_iter()
                .any(|endpoint| model.upstream_endpoint(endpoint).is_some()),
                Protocol::Anthropic => model.upstream_endpoint(TextEndpoint::Messages).is_some(),
                Protocol::Gemini => [
                    TextEndpoint::GeminiGenerateContent,
                    TextEndpoint::GeminiStreamGenerateContent,
                    TextEndpoint::GeminiCountTokens,
                ]
                .into_iter()
                .any(|endpoint| model.upstream_endpoint(endpoint).is_some()),
            }
    });
    let describe = |name: &str, model: &crate::gateway::Model| match protocol {
        Protocol::OpenAi => {
            json!({"id": name, "object": "model", "created": 0, "owned_by": "openrust"})
        }
        Protocol::Anthropic => {
            json!({"id": name, "type": "model", "display_name": name, "created_at": "1970-01-01T00:00:00Z"})
        }
        Protocol::Gemini => {
            let methods = [
                TextEndpoint::GeminiGenerateContent,
                TextEndpoint::GeminiStreamGenerateContent,
                TextEndpoint::GeminiCountTokens,
            ]
            .into_iter()
            .filter(|endpoint| model.upstream_endpoint(*endpoint).is_some())
            .map(TextEndpoint::path)
            .collect::<Vec<_>>();
            json!({"name":format!("models/{name}"),"displayName": name,"supportedGenerationMethods":methods})
        }
    };
    let data = if let Some(id) = id {
        let Some((_, model)) = accessible.find(|(name, _)| *name == id) else {
            return protocol.error(StatusCode::NOT_FOUND, "model_not_found", "模型不可用");
        };
        describe(id, model)
    } else {
        let data: Vec<Value> = accessible
            .map(|(name, model)| describe(name, model))
            .collect();
        match protocol {
            Protocol::OpenAi => json!({"object": "list", "data": data}),
            Protocol::Gemini => json!({"models":data}),
            Protocol::Anthropic => json!({
                "first_id": data.first().and_then(|item| item.get("id")),
                "last_id": data.last().and_then(|item| item.get("id")),
                "has_more": false,
                "data": data,
            }),
        }
    };
    Response::json(StatusCode::OK, data).map(|response| {
        response.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
    })
}

fn chat_completions(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::ChatCompletions, None))
}

fn responses(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::Responses, None))
}

fn embeddings(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::Embeddings, None))
}

fn messages(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::Messages, None))
}

fn count_message_tokens(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::CountMessageTokens, None))
}

#[derive(Clone, Copy)]
enum Protocol {
    OpenAi,
    Anthropic,
    Gemini,
}

impl Protocol {
    fn token(self, request: &Request) -> Option<&str> {
        match self {
            Self::OpenAi => bearer(request),
            Self::Gemini => {
                single_header(request, "x-goog-api-key").filter(|token| valid_token(token))
            }
            Self::Anthropic if request.has_header("x-api-key") => {
                if request.has_header("authorization") {
                    return None;
                }
                single_header(request, "x-api-key").filter(|token| valid_token(token))
            }
            Self::Anthropic => bearer(request),
        }
    }

    fn unauthorized(self) -> HttpResult {
        self.error(
            StatusCode::UNAUTHORIZED,
            "invalid_api_key",
            "凭证缺失或无效",
        )
        .map(|response| {
            if matches!(self, Self::OpenAi) {
                response.header(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))
            } else {
                response
            }
        })
    }

    fn error(self, status: StatusCode, code: &str, message: &str) -> HttpResult {
        let data = match self {
            Self::OpenAi => error_body(code, message),
            Self::Gemini => {
                let kind = match status {
                    StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
                    StatusCode::FORBIDDEN => "PERMISSION_DENIED",
                    StatusCode::NOT_FOUND => "NOT_FOUND",
                    StatusCode::TOO_MANY_REQUESTS | StatusCode::PAYLOAD_TOO_LARGE => {
                        "RESOURCE_EXHAUSTED"
                    }
                    StatusCode::GATEWAY_TIMEOUT => "DEADLINE_EXCEEDED",
                    _ if status.is_server_error() => "UNAVAILABLE",
                    _ => "INVALID_ARGUMENT",
                };
                json!({"error":{"code":status.as_u16(),"status":kind,"message":message}})
            }
            Self::Anthropic => {
                let kind = match status {
                    StatusCode::UNAUTHORIZED => "authentication_error",
                    StatusCode::NOT_FOUND => "not_found_error",
                    StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
                    StatusCode::PAYLOAD_TOO_LARGE => "request_too_large",
                    _ if status.is_server_error() => "api_error",
                    _ => "invalid_request_error",
                };
                json!({"type":"error","error":{"type":kind,"message":message}})
            }
        };
        Response::json(status, data).map(|response| {
            response.header(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))
        })
    }

    fn stream_error(self) -> String {
        let error = match self {
            Self::OpenAi => error_body("upstream_error", "上游流未能完成"),
            Self::Gemini => {
                json!({"error":{"code":502,"status":"UNAVAILABLE","message":"上游流未能完成"}})
            }
            Self::Anthropic => {
                json!({"type":"error","error":{"type":"api_error","message":"上游流未能完成"}})
            }
        };
        format!("event: error\ndata: {error}\n\n")
    }
}

async fn relay(
    request: &mut Request,
    endpoint: TextEndpoint,
    path_model: Option<String>,
) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let protocol = match endpoint {
        TextEndpoint::Messages | TextEndpoint::CountMessageTokens => Protocol::Anthropic,
        TextEndpoint::GeminiGenerateContent
        | TextEndpoint::GeminiStreamGenerateContent
        | TextEndpoint::GeminiCountTokens => Protocol::Gemini,
        _ => Protocol::OpenAi,
    };
    let Some(key) = protocol
        .token(request)
        .and_then(|token| gateway.authorize(token))
    else {
        return protocol.unauthorized();
    };
    let body = match request.body_bytes().await {
        Ok(body) => body,
        Err(HttpError::PayloadTooLarge) => {
            return protocol.error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "请求正文超出大小限制",
            );
        }
        Err(_) => {
            return protocol.error(StatusCode::BAD_REQUEST, "invalid_json", "无法读取请求正文");
        }
    };
    // 协议正文保留提示词和工具参数原文，绕开应用对 JSON 输入的清理。
    let Ok(Value::Object(mut data)) = serde_json::from_slice(&body) else {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "请求正文必须是 JSON 对象",
        );
    };
    let Some(name) = path_model.or_else(|| {
        data.get("model")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }) else {
        return protocol.error(StatusCode::BAD_REQUEST, "missing_model", "必须指定模型");
    };
    let Some(model) = gateway
        .models
        .get(&name)
        .filter(|_| key.models.contains(&name))
    else {
        return protocol.error(StatusCode::NOT_FOUND, "model_not_found", "模型不可用");
    };
    if model.upstream_endpoint(endpoint).is_none() {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_endpoint",
            "该模型不支持此端点",
        );
    }
    let streaming = if matches!(protocol, Protocol::Gemini) {
        endpoint == TextEndpoint::GeminiStreamGenerateContent
    } else {
        match data.get("stream") {
            None | Some(Value::Bool(false) | Value::Null) => false,
            Some(Value::Bool(true))
                if !matches!(
                    endpoint,
                    TextEndpoint::CountMessageTokens | TextEndpoint::Embeddings
                ) =>
            {
                true
            }
            _ => {
                return protocol.error(
                    StatusCode::BAD_REQUEST,
                    "invalid_stream",
                    "stream 必须为布尔值",
                );
            }
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
            return protocol.error(
                StatusCode::BAD_REQUEST,
                "unsupported_state",
                "此端点仅支持无状态请求及客户端工具",
            );
        }
        data.insert("store".into(), Value::Bool(false));
    }
    if matches!(protocol, Protocol::Anthropic)
        && ["container", "mcp_servers"]
            .iter()
            .any(|field| data.get(*field).is_some_and(|value| !value.is_null()))
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "此端点仅支持无状态请求",
        );
    }
    if matches!(protocol, Protocol::Gemini)
        && [
            "model",
            "cachedContent",
            "cached_content",
            "generateContentRequest",
            "generate_content_request",
        ]
        .iter()
        .any(|field| data.get(*field).is_some_and(|value| !value.is_null()))
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "Gemini 模型由路径选择，暂不支持嵌套请求或上游缓存资源",
        );
    }
    let may_replay = data
        .get("tools")
        .is_none_or(|tools| tools.is_null() || tools.as_array().is_some_and(Vec::is_empty))
        && data.get("store") != Some(&Value::Bool(true));
    let request_id = Str::ulid().to_string();
    let mut admitted = false;
    let mut failure = None;
    let mut conversion_error = None;
    let mut cooling = None::<u64>;
    let cache_store = gateway.account_cache_store.as_deref();
    let mut targets = model.targets_for(endpoint).peekable();
    while let Some((target, upstream_endpoint)) = targets.next() {
        let mut payload = if targets.peek().is_some() {
            data.clone()
        } else {
            std::mem::take(&mut data)
        };
        payload.insert("model".into(), json!(target.upstream_model));
        let bridge = Bridge::between(endpoint, upstream_endpoint);
        let payload = if let Some(bridge) = bridge {
            match bridge.request(payload) {
                Ok(payload) => payload,
                Err(message) => {
                    conversion_error = Some(message);
                    continue;
                }
            }
        } else {
            Value::Object(payload)
        };
        if let Some(account) = &target.cooldown {
            match account.remaining(cache_store).await {
                Ok(Some(seconds)) => {
                    cooling = Some(cooling.map_or(seconds, |current| current.min(seconds)));
                    continue;
                }
                Ok(None) => {}
                Err(_) => return account_state_unavailable(protocol),
            }
        }
        if !admitted {
            if let Some(response) = admit(key, protocol).await? {
                return Ok(response);
            }
            admitted = true;
        }
        let mut call = gateway.usage(
            key,
            &name,
            target,
            (endpoint, upstream_endpoint),
            &request_id,
        );
        match request_upstream(target, upstream_endpoint, payload, protocol, &mut call).await {
            Ok(upstream) => {
                return respond(upstream, name, endpoint, streaming, protocol, bridge, call);
            }
            Err(rejected) => {
                if let (Some(account), Some(until)) = (&target.cooldown, rejected.pause_until)
                    && account.pause(cache_store, until).await.is_err()
                {
                    return account_state_unavailable(protocol);
                }
                if !rejected.retryable || !may_replay {
                    return rejected.response;
                }
                failure = Some(rejected.response);
            }
        }
    }
    if let Some(failure) = failure {
        return failure;
    }
    if let Some(seconds) = cooling {
        return protocol
            .error(
                StatusCode::SERVICE_UNAVAILABLE,
                "upstream_unavailable",
                "上游连接正在冷却",
            )
            .map(|response| response.header(header::RETRY_AFTER, HeaderValue::from(seconds)));
    }
    protocol.error(
        StatusCode::BAD_REQUEST,
        "unsupported_conversion",
        conversion_error.unwrap_or("没有支持此请求的上游连接"),
    )
}

fn account_state_unavailable(protocol: Protocol) -> HttpResult {
    protocol.error(
        StatusCode::SERVICE_UNAVAILABLE,
        "account_state_unavailable",
        "上游账号状态暂不可用",
    )
}

async fn admit(key: &Key, protocol: Protocol) -> Result<Option<Response>, HttpError> {
    let Some(limit) = key.requests_per_minute else {
        return Ok(None);
    };
    let limiter = RateLimiter::get_facade_root()?;
    match limiter
        .reserve(&format!("ai-gateway:{}", key.name), limit, 60)
        .await
    {
        Ok(Some(seconds)) => protocol
            .error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_exceeded",
                "已达到每分钟请求限额",
            )
            .map(|response| Some(response.header(header::RETRY_AFTER, HeaderValue::from(seconds)))),
        Ok(None) => Ok(None),
        Err(_) => protocol
            .error(
                StatusCode::SERVICE_UNAVAILABLE,
                "rate_limit_unavailable",
                "限流服务暂不可用",
            )
            .map(Some),
    }
}

struct UpstreamFailure {
    retryable: bool,
    pause_until: Option<i64>,
    response: HttpResult,
}

async fn request_upstream(
    target: &Target,
    endpoint: TextEndpoint,
    data: Value,
    protocol: Protocol,
    call: &mut Call,
) -> Result<client::Response, UpstreamFailure> {
    let upstream = match target.provider.relay(endpoint, data).await {
        Ok(response) => response,
        Err(error) => {
            let (status, outcome, code, message) = match error {
                AiError::Timeout => (
                    StatusCode::GATEWAY_TIMEOUT,
                    "timeout",
                    "upstream_timeout",
                    "上游请求超时",
                ),
                _ => (
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    "upstream_error",
                    "上游连接失败",
                ),
            };
            call.outcome(outcome);
            call.status(status);
            return Err(UpstreamFailure {
                retryable: matches!(
                    error,
                    AiError::Timeout | AiError::Transport | AiError::MissingCredential
                ),
                pause_until: None,
                response: protocol.error(status, code, message),
            });
        }
    };
    let status = upstream.status();
    call.status(status);
    if status.is_success() {
        return Ok(upstream);
    }
    call.outcome("upstream_error");
    let retryable = status.is_server_error()
        || matches!(
            status,
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
        );
    let status = if status.is_client_error() || status.is_server_error() {
        status
    } else {
        StatusCode::BAD_GATEWAY
    };
    let headers = downstream_headers(upstream.headers());
    let response = protocol
        .error(status, "upstream_error", "上游拒绝或未能处理该请求")
        .map(|response| response.with_headers(headers));
    let pause_until = match status.as_u16() {
        429 | 503 | 529 => target
            .cooldown
            .as_ref()
            .and_then(|policy| policy.deadline(upstream.headers())),
        _ => None,
    };
    Err(UpstreamFailure {
        retryable,
        pause_until,
        response,
    })
}

fn respond(
    upstream: client::Response,
    name: String,
    endpoint: TextEndpoint,
    streaming: bool,
    protocol: Protocol,
    bridge: Option<Bridge>,
    mut call: Call,
) -> HttpResult {
    let status = upstream.status();
    let headers = downstream_headers(upstream.headers());
    if streaming {
        return stream(upstream, headers, name, call, protocol, bridge);
    }
    let Ok(mut data) = upstream.json::<Value>() else {
        call.outcome("invalid_upstream_response");
        return protocol.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回有效 JSON",
        );
    };
    if !matches!(
        endpoint,
        TextEndpoint::CountMessageTokens | TextEndpoint::GeminiCountTokens
    ) {
        call.observe_usage(&data);
    }
    if let Some(bridge) = bridge {
        let Some(response) = bridge.response(name.clone(), &data) else {
            call.outcome("invalid_upstream_response");
            return protocol.error(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "无法转换上游协议响应",
            );
        };
        data = response;
    }
    call.outcome("completed");
    rename_model(&mut data, &name);
    Response::json(status, data).map(|response| response.with_headers(headers))
}

fn stream(
    upstream: client::Response,
    mut headers: HeaderMap,
    name: String,
    mut call: Call,
    protocol: Protocol,
    bridge: Option<Bridge>,
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
        return protocol.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回 SSE",
        );
    }
    let Ok(body) = upstream.into_stream() else {
        call.outcome("invalid_upstream_response");
        return protocol.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "无法读取上游 SSE",
        );
    };
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    if let Some(bridge) = bridge {
        let stream = bridged_stream(body, bridge.stream(name), call, protocol);
        return Ok(Response::stream(status, stream).with_headers(headers));
    }
    let stream = async_stream::stream! {
        let mut events = body.eventsource();
        let mut terminal = false;
        while let Some(event) = events.next().await {
            let Ok(event) = event else {
                call.outcome("stream_error");
                yield Ok::<_, Infallible>(protocol.stream_error());
                return;
            };
            if event.data == "[DONE]" {
                call.outcome("completed");
                yield Ok("data: [DONE]\n\n".to_owned());
                return;
            }
            let Ok(mut data) = serde_json::from_str::<Value>(&event.data) else {
                call.outcome("stream_error");
                yield Ok(protocol.stream_error());
                return;
            };
            if matches!(data["type"].as_str(), Some("error" | "response.failed")) || event.event == "error" || data.get("error").is_some_and(|value| !value.is_null()) {
                call.outcome("upstream_error");
                yield Ok(protocol.stream_error());
                return;
            }
            terminal |= matches!(data["type"].as_str(), Some("response.completed" | "response.incomplete" | "message_stop"));
            if matches!(protocol, Protocol::Gemini) {
                terminal |= data["candidates"][0]["finishReason"].as_str().is_some_and(|reason| reason != "FINISH_REASON_UNSPECIFIED")
                    || data["promptFeedback"]["blockReason"].as_str().is_some_and(|reason| reason != "BLOCK_REASON_UNSPECIFIED");
            }
            if terminal {
                call.outcome("completed");
            }
            call.observe_usage(&data);
            rename_model(&mut data, &name);
            for field in ["response", "message"] {
                if let Some(response) = data.get_mut(field) {
                    call.observe_usage(response);
                    rename_model(response, &name);
                }
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
            yield Ok(protocol.stream_error());
        }
    };
    Ok(Response::stream(status, stream).with_headers(headers))
}

fn bridged_stream(
    body: client::ResponseStream,
    mut state: crate::compatibility::Stream,
    mut call: Call,
    protocol: Protocol,
) -> impl futures_util::Stream<Item = Result<String, Infallible>> {
    async_stream::stream! {
        let mut events = body.eventsource();
        while let Some(event) = events.next().await {
            let data = event.ok().and_then(|event| serde_json::from_str::<Value>(&event.data).ok());
            let Some(data) = data else {
                call.outcome("stream_error");
                yield Ok(protocol.stream_error());
                return;
            };
            call.observe_usage(&data);
            call.observe_usage(&data["message"]);
            call.observe_usage(&data["response"]);
            let Some((frames, terminal)) = state.convert(&data) else {
                call.outcome("invalid_upstream_response");
                yield Ok(protocol.stream_error());
                return;
            };
            if terminal {
                call.outcome("completed");
            }
            for frame in frames {
                yield Ok(frame);
            }
            if terminal {
                return;
            }
        }
        call.outcome("incomplete_stream");
        yield Ok(protocol.stream_error());
    }
}

fn rename_model(data: &mut Value, name: &str) {
    for field in ["model", "modelVersion"] {
        if let Some(model) = data.get_mut(field).filter(|model| model.is_string()) {
            *model = Value::String(name.into());
        }
    }
}

fn bearer(request: &Request) -> Option<&str> {
    let (scheme, token) = single_header(request, "authorization")?.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("Bearer") && valid_token(token)).then_some(token)
}

fn single_header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    let mut values = request.headers().get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

fn valid_token(token: &str) -> bool {
    !token.is_empty() && !token.contains(',') && !token.chars().any(char::is_whitespace)
}

fn downstream_headers(upstream: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(value) = upstream.get(header::RETRY_AFTER) {
        headers.insert(header::RETRY_AFTER, value.clone());
    }
    headers
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
