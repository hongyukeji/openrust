use std::{collections::HashSet, convert::Infallible, fmt::Write as _};

use eventsource_stream::Eventsource as _;
use futures_util::StreamExt as _;
use http::{HeaderMap, HeaderValue, header};
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client},
    routing::{RouteGroup, Router, RoutingError},
    support::{
        Str,
        facades::{Facade, RateLimiter, Route},
    },
};
use openrust_ai::{AiError, TextEndpoint};
use serde_json::{Value, json};

use crate::{
    accounts::Admission,
    codex_catalog,
    compatibility::{Bridge, Translation},
    gateway::{AttemptAdmission, Call, Gateway, Key, Target, UsageAdmission},
    invocation::{CallIdentity, ClientInvocation},
    localization,
    session_affinity::SessionAffinity,
};

type HttpResult = Result<Response, HttpError>;

mod embedding_batch;
mod gemini_batches;
mod gemini_caches;
mod gemini_count;
mod image_upload;
mod resources;
mod usage_summary;
mod websocket;
mod xai_custom_voices;
mod xai_speech;
mod xai_transcription;

pub(crate) fn routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().prefix("/v1"), |router| {
        shared_routes(router)?;
        router.post("/messages", messages)?;
        router.get("/usage", usage_summary::show)?;
        Ok(())
    })?;
    Route::group(RouteGroup::new().prefix("/backend-api/codex"), |router| {
        router.get("/models", codex_models)?;
        responses_routes(router)
    })?;
    Route::group(RouteGroup::new().prefix("/upload/v1beta"), |router| {
        router.post("/files", gemini_batches::upload::upload_route)?;
        Ok(())
    })?;
    Route::group(RouteGroup::new().prefix("/download/v1beta"), |router| {
        router.get("/files/{id}", gemini_batches::download_route)?;
        Ok(())
    })?;
    Route::group(RouteGroup::new().prefix("/v1beta"), |router| {
        router.get("/models", gemini_models)?;
        router.get("/models/{model}", gemini_model)?;
        router.post("/models/{model}", gemini_operation)?;
        gemini_batches::routes(router)?;
        gemini_caches::routes(router)
    })
}

pub(crate) fn unversioned_routes() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new(), shared_routes)
}

fn shared_routes(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/models", models)?;
    router.get("/models/{model}", model)?;
    router.post("/chat/completions", chat_completions)?;
    router.post("/embeddings", embeddings)?;
    router.post("/images/generations", images_generations)?;
    router.post("/images/edits", images_edits)?;
    router.post("/tts", xai_speech)?;
    router
        .post("/stt", xai_transcription)?
        .streaming_body(xai_transcription::MAX_BODY_BYTES);
    xai_custom_voices::routes(router)?;
    router.post("/messages/count_tokens", count_message_tokens)?;
    responses_routes(router)
}

fn responses_routes(router: &mut Router) -> Result<(), RoutingError> {
    router.post("/responses", responses)?;
    router.get("/responses", websocket)?;
    router.post("/responses/input_tokens", count_response_tokens)?;
    Ok(())
}

fn gemini_models(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move { catalog(Protocol::Gemini.token(request), None, Protocol::Gemini).await })
}

fn gemini_model(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        catalog(
            Protocol::Gemini.token(request),
            request.route("model"),
            Protocol::Gemini,
        )
        .await
    })
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
            "embedContent" => TextEndpoint::GeminiEmbedContent,
            "batchEmbedContents" => TextEndpoint::GeminiBatchEmbedContents,
            "batchGenerateContent" | "asyncBatchEmbedContent" => {
                let model = model.to_owned();
                let endpoint = if action == "batchGenerateContent" {
                    TextEndpoint::GeminiBatchGenerateContent
                } else {
                    TextEndpoint::GeminiAsyncBatchEmbedContent
                };
                return gemini_batches::create(request, model, endpoint).await;
            }
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
    Box::pin(async move {
        let protocol = catalog_protocol(request);
        if matches!(protocol, Protocol::OpenAi) {
            match codex_catalog::selected(request)? {
                Some(true) => return codex_models(request).await,
                Some(false) => {}
                None => {
                    return protocol.error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "client_version 无效",
                    );
                }
            }
        }
        catalog(protocol.token(request), None, protocol).await
    })
}

fn codex_models(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        let gateway = Gateway::get_facade_root()?;
        let protocol = Protocol::OpenAi;
        let key = match authorize(&gateway, protocol.token(request), protocol).await {
            Ok(key) => key,
            Err(response) => return response,
        };
        codex_catalog::respond(request, &gateway, &key)
    })
}

fn model(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        let protocol = catalog_protocol(request);
        catalog(protocol.token(request), request.route("model"), protocol).await
    })
}

fn catalog_protocol(request: &Request) -> Protocol {
    if request.has_header("x-api-key") {
        Protocol::Anthropic
    } else {
        Protocol::OpenAi
    }
}

const GEMINI_MODEL_OPERATIONS: [TextEndpoint; 7] = [
    TextEndpoint::GeminiGenerateContent,
    TextEndpoint::GeminiStreamGenerateContent,
    TextEndpoint::GeminiCountTokens,
    TextEndpoint::GeminiEmbedContent,
    TextEndpoint::GeminiBatchEmbedContents,
    TextEndpoint::GeminiBatchGenerateContent,
    TextEndpoint::GeminiAsyncBatchEmbedContent,
];

async fn catalog(token: Option<&str>, id: Option<&str>, protocol: Protocol) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, token, protocol).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let key = key.as_ref();
    let mut accessible = gateway.models.iter().filter(|(name, model)| {
        key.allows_model(name)
            && match protocol {
                Protocol::OpenAi => [
                    TextEndpoint::ChatCompletions,
                    TextEndpoint::Responses,
                    TextEndpoint::CountResponseTokens,
                    TextEndpoint::Embeddings,
                    TextEndpoint::ImagesGenerations,
                    TextEndpoint::ImagesEdits,
                    TextEndpoint::XaiTextToSpeech,
                    TextEndpoint::XaiSpeechToText,
                ]
                .into_iter()
                .any(|endpoint| model.supports_for(endpoint, key)),
                Protocol::Anthropic => {
                    model.supports_for(TextEndpoint::Messages, key)
                        || model.supports_for(TextEndpoint::CountMessageTokens, key)
                }
                Protocol::Gemini => GEMINI_MODEL_OPERATIONS
                    .into_iter()
                    .any(|endpoint| model.supports_for(endpoint, key)),
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
            let methods = GEMINI_MODEL_OPERATIONS
                .into_iter()
                .filter(|endpoint| model.supports_for(*endpoint, key))
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

fn websocket(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(websocket::handshake(request))
}

fn count_response_tokens(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::CountResponseTokens, None))
}

fn images_generations(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::ImagesGenerations, None))
}

fn images_edits(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::ImagesEdits, None))
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

fn xai_speech(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::XaiTextToSpeech, None))
}

fn xai_transcription(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::XaiSpeechToText, None))
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
            &localization::error("invalid_api_key", "凭证缺失或无效"),
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
        format!("event: error\ndata: {}\n\n", self.stream_error_body())
    }

    fn stream_error_body(self) -> Value {
        let message = localization::error("stream", "上游流未能完成");
        match self {
            Self::OpenAi => error_body("upstream_error", &message),
            Self::Gemini => {
                json!({"error":{"code":502,"status":"UNAVAILABLE","message":message}})
            }
            Self::Anthropic => {
                json!({"type":"error","error":{"type":"api_error","message":message}})
            }
        }
    }
}

fn responses_tools(
    tools: Option<&Value>,
    allows_server_tool: impl Fn(&str, &Value) -> bool,
) -> Option<(bool, Vec<String>)> {
    let Some(tools) = tools.filter(|tools| !tools.is_null()) else {
        return Some((false, Vec::new()));
    };
    let mut native = false;
    let mut server_tools = Vec::new();
    for tool in tools.as_array()? {
        match tool["type"].as_str()? {
            "function" | "custom" => {}
            "namespace" => {
                let members = tool["tools"].as_array()?;
                if members.is_empty()
                    || !members.iter().all(|member| {
                        matches!(member["type"].as_str(), Some("function" | "custom"))
                    })
                {
                    return None;
                }
                native = true;
            }
            kind if allows_server_tool(kind, tool) => {
                native = true;
                server_tools.push(kind.to_owned());
            }
            _ => return None,
        }
    }
    Some((native, server_tools))
}

async fn authorize(
    gateway: &Gateway,
    token: Option<&str>,
    protocol: Protocol,
) -> Result<std::sync::Arc<Key>, HttpResult> {
    let Some(token) = token else {
        return Err(protocol.unauthorized());
    };
    match gateway.authorize(token).await {
        Ok(Some(key)) => Ok(key),
        Ok(None) => Err(protocol.unauthorized()),
        Err(_) => Err(protocol.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "authorization_unavailable",
            "网关凭证存储暂不可用",
        )),
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
        | TextEndpoint::GeminiCountTokens
        | TextEndpoint::GeminiEmbedContent
        | TextEndpoint::GeminiBatchEmbedContents
        | TextEndpoint::GeminiBatchGenerateContent
        | TextEndpoint::GeminiAsyncBatchEmbedContent => Protocol::Gemini,
        _ => Protocol::OpenAi,
    };
    let key = match authorize(&gateway, protocol.token(request), protocol).await {
        Ok(key) => key,
        Err(response) => return response,
    };
    let key = key.as_ref();
    let (mut data, mut uploads) = if endpoint == TextEndpoint::XaiSpeechToText {
        match xai_transcription::parse(request, &gateway.upload_directory).await {
            Ok((data, parts)) => (data, Some(parts)),
            Err(HttpError::PayloadTooLarge) => return request_too_large(protocol),
            Err(HttpError::Internal { .. }) => {
                return protocol.error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "upload_unavailable",
                    "语音上传暂存失败",
                );
            }
            Err(_) => {
                return protocol.error(
                    StatusCode::BAD_REQUEST,
                    "invalid_multipart",
                    "语音转录需要一个有效的 multipart 音频文件及原生字段",
                );
            }
        }
    } else if endpoint == TextEndpoint::ImagesEdits && image_upload::is_multipart(request) {
        match image_upload::parse(request).await {
            Ok((data, parts)) => (data, Some(parts)),
            Err(HttpError::PayloadTooLarge) => return request_too_large(protocol),
            Err(_) => {
                return protocol.error(
                    StatusCode::BAD_REQUEST,
                    "invalid_multipart",
                    "图片编辑需要有效的 multipart 图片和字段",
                );
            }
        }
    } else {
        let body = match request.body_bytes().await {
            Ok(body) => body,
            Err(HttpError::PayloadTooLarge) => return request_too_large(protocol),
            Err(_) => {
                return protocol.error(StatusCode::BAD_REQUEST, "invalid_json", "无法读取请求正文");
            }
        };
        // 协议正文保留提示词和工具参数原文，绕开应用对 JSON 输入的清理。
        let Ok(Value::Object(data)) = serde_json::from_slice(&body) else {
            return protocol.error(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "请求正文必须是 JSON 对象",
            );
        };
        (data, None)
    };
    let speech = if endpoint == TextEndpoint::XaiTextToSpeech {
        match xai_speech::validate(&data) {
            Ok(format) => Some(format),
            Err(message) => {
                return protocol.error(StatusCode::BAD_REQUEST, "invalid_request", message);
            }
        }
    } else {
        None
    };
    let implicit_model = if endpoint.is_xai_voice() && !data.contains_key("model") {
        match xai_speech::implicit_model(&gateway, key, endpoint) {
            Ok(model) => Some(model.to_owned()),
            Err((status, code, message)) => return protocol.error(status, code, message),
        }
    } else {
        None
    };
    let Some(name) = path_model.or(implicit_model).or_else(|| {
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
        .filter(|model| key.allows_model(&name) && model.supports_key(key))
    else {
        return protocol.error(StatusCode::NOT_FOUND, "model_not_found", "模型不可用");
    };
    if !model.supports_for(endpoint, key) {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_endpoint",
            "该模型不支持此端点",
        );
    }
    let custom_voice = if endpoint == TextEndpoint::XaiTextToSpeech {
        if let Some(id) = data
            .get("voice_id")
            .and_then(Value::as_str)
            .filter(|id| !xai_speech::builtin_voice(id))
        {
            match xai_custom_voices::load(&gateway, key, id).await {
                Ok(voice) => Some(voice),
                Err(response) => return response,
            }
        } else {
            None
        }
    } else {
        None
    };
    if endpoint.is_image()
        && (data.get("file_id").is_some_and(|value| !value.is_null())
            || data.values().any(resources::image_file_references))
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "图片端点仅接收当前请求内的图片引用",
        );
    }
    let stream_format = if endpoint == TextEndpoint::GeminiStreamGenerateContent {
        let alt = match (request.query(Some("alt")), request.query(Some("$alt"))) {
            (Ok(None), Ok(value)) | (Ok(value), Ok(None)) => Ok(value),
            _ => Err(()),
        };
        match alt {
            Ok(None) => Some(StreamFormat::JsonArray { opened: false }),
            Ok(Some(Value::String(alt))) if alt == "json" => {
                Some(StreamFormat::JsonArray { opened: false })
            }
            Ok(Some(Value::String(alt))) if alt == "sse" => Some(StreamFormat::EventSource),
            _ => {
                return protocol.error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Gemini 流的 alt 只支持 json 或 sse",
                );
            }
        }
    } else if matches!(protocol, Protocol::Gemini) {
        None
    } else {
        match data.get("stream") {
            None | Some(Value::Bool(false) | Value::Null) => None,
            Some(Value::String(value)) if uploads.is_some() && value == "false" => None,
            Some(Value::String(value)) if uploads.is_some() && value == "true" => {
                Some(StreamFormat::EventSource)
            }
            Some(Value::Bool(true))
                if !endpoint.is_counting() && endpoint != TextEndpoint::Embeddings =>
            {
                Some(StreamFormat::EventSource)
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
    let stream_options = stream_format.map(|format| StreamOptions {
        format,
        gemini_candidates: (endpoint == TextEndpoint::GeminiStreamGenerateContent).then(|| {
            data.get("generationConfig")
                .or_else(|| data.get("generation_config"))
                .and_then(|config| {
                    config
                        .get("candidateCount")
                        .or_else(|| config.get("candidate_count"))
                })
                .and_then(Value::as_u64)
                .filter(|count| *count != 0)
                .unwrap_or(1)
        }),
    });
    let mut native_responses_only = false;
    let mut server_tools = Vec::new();
    if matches!(
        endpoint,
        TextEndpoint::Responses | TextEndpoint::CountResponseTokens
    ) {
        // 共享上游 Key 暂不承载跨请求资源，防止不同下游 Key 通过资源 ID 互访。
        if ["store", "background"].iter().any(|field| {
            !matches!(
                data.get(*field),
                None | Some(Value::Null | Value::Bool(false))
            )
        }) || ["previous_response_id", "conversation", "prompt"]
            .iter()
            .any(|field| data.get(*field).is_some_and(|value| !value.is_null()))
            || data.get("input").is_some_and(|input| {
                if endpoint == TextEndpoint::Responses {
                    resources::openai_references(input)
                } else {
                    input.as_array().is_some_and(|items| {
                        items.iter().any(|item| item["type"] == "item_reference")
                    })
                }
            })
        {
            return protocol.error(
                StatusCode::BAD_REQUEST,
                "unsupported_state",
                "此端点仅支持无状态请求和获准的工具",
            );
        }
        (native_responses_only, server_tools) =
            match responses_tools(data.get("tools"), |kind, tool| {
                endpoint == TextEndpoint::Responses && model.allows_server_tool(kind, tool)
            }) {
                Some(requirements) => requirements,
                None => {
                    return protocol.error(
                        StatusCode::BAD_REQUEST,
                        "unsupported_state",
                        "当前模型不支持所请求的 Responses 工具",
                    );
                }
            };
        if endpoint == TextEndpoint::CountResponseTokens {
            if data
                .get("input")
                .is_some_and(|input| !countable_input(input))
            {
                return protocol.error(
                    StatusCode::BAD_REQUEST,
                    "unsupported_count_input",
                    "计数目前只接收文本和内联客户端函数项",
                );
            }
            for field in ["store", "background", "stream"] {
                data.remove(field);
            }
        } else {
            data.insert("store".into(), Value::Bool(false));
        }
    }
    if endpoint == TextEndpoint::ChatCompletions
        && data
            .get("messages")
            .is_some_and(resources::openai_references)
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "Chat Completions 仅接收内联文件及音频",
        );
    }
    if matches!(protocol, Protocol::Anthropic)
        && (["container", "mcp_servers"]
            .iter()
            .any(|field| data.get(*field).is_some_and(|value| !value.is_null()))
            || ["messages", "system"].iter().any(|field| {
                data.get(*field)
                    .is_some_and(resources::anthropic_references)
            }))
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "此端点仅支持无状态请求及内联附件",
        );
    }
    let gemini_count_envelope = if endpoint == TextEndpoint::GeminiCountTokens {
        match gemini_count::unwrap(&mut data, &name) {
            Ok(wrapped) => wrapped,
            Err(message) => {
                return protocol.error(StatusCode::BAD_REQUEST, "invalid_request", message);
            }
        }
    } else {
        false
    };
    if endpoint == TextEndpoint::GeminiCountTokens
        && !gemini_count_envelope
        && ["cachedContent", "cached_content"]
            .iter()
            .any(|field| resources::present_field(&data, field))
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Gemini 计数缓存须放在生成配置中",
        );
    }
    let gemini_cache = if matches!(protocol, Protocol::Gemini) {
        match gemini_caches::resolve(&gateway, key, &name, endpoint, &mut data).await {
            Ok(cache) => cache,
            Err(error) => return error,
        }
    } else {
        None
    };
    if matches!(protocol, Protocol::Gemini)
        && ((!endpoint.is_gemini_embedding() && resources::present_field(&data, "model"))
            || ["generateContentRequest", "generate_content_request"]
                .iter()
                .any(|field| resources::present_field(&data, field))
            || resources::gemini_file_references(&data))
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "Gemini 模型由路径选择，仅支持当前请求内的资源",
        );
    }
    if endpoint.is_gemini_embedding()
        && !resources::gemini_embedding_model_matches(&data, &name, endpoint)
    {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "嵌入请求的模型须与路径中的公开模型一致",
        );
    }
    let sealed_responses = endpoint == TextEndpoint::Responses
        && data
            .get("input")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item["type"] == "reasoning"
                        && item["encrypted_content"].as_str().is_some_and(|token| {
                            token.starts_with(crate::compatibility::BEDROCK_REASONING_PREFIX)
                        })
                })
            });
    let sealed_chat = endpoint == TextEndpoint::ChatCompletions
        && data
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages
                    .iter()
                    .any(|message| message.get(openrust_ai::CHAT_REASONING_REPLAY).is_some())
            });
    let may_replay = !endpoint.is_image()
        && !endpoint.is_xai_voice()
        && gemini_cache.is_none()
        && !sealed_responses
        && !sealed_chat
        && data
            .get("tools")
            .is_none_or(|tools| tools.is_null() || tools.as_array().is_some_and(Vec::is_empty))
        && !includes_tool_history(&data)
        && data.get("store") != Some(&Value::Bool(true));
    let request_id = Str::ulid().to_string();
    let invocation = ClientInvocation::from_headers(request.headers());
    let mut admitted = false;
    let mut failure = None;
    let mut conversion_error = None;
    let mut budget_error = None;
    let mut account_budget_retry = None::<u64>;
    let mut cooling = None::<u64>;
    let mut busy = false;
    let cache_store = gateway.account_cache_store.as_deref();
    let affinity = if let Some(seconds) = gateway.account_session_ttl {
        match SessionAffinity::for_request(
            request.headers(),
            &key.limiter_key(),
            &name,
            cache_store,
            seconds,
        ) {
            Ok(affinity) => affinity,
            Err(()) => {
                return protocol.error(
                    StatusCode::BAD_REQUEST,
                    "invalid_session_id",
                    "X-Session-ID 无效",
                );
            }
        }
    } else {
        None
    };
    let mut targets: Vec<_> = model
        .targets_for(endpoint, key)
        .filter(|(target, upstream)| {
            gemini_cache
                .as_ref()
                .is_none_or(|cache| gemini_caches::cache_matches(cache, target))
                && custom_voice.as_ref().is_none_or(|voice| {
                    xai_custom_voices::scope(&name, target).as_ref() == Some(&voice.scope)
                })
                && (!sealed_responses
                    || (*upstream == TextEndpoint::ChatCompletions
                        && target.provider.supports_chat_reasoning_replay()))
                && (!native_responses_only || *upstream == endpoint)
                && server_tools
                    .iter()
                    .all(|kind| target.provider.supports_responses_server_tool(kind))
        })
        .collect();
    if gemini_cache.is_some() && targets.is_empty() {
        return gemini_caches::not_found();
    }
    if custom_voice.is_some() && targets.is_empty() {
        return xai_custom_voices::not_found();
    }
    if native_responses_only && targets.is_empty() {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_endpoint",
            "所选工具要求当前凭证可访问原生 Responses 上游",
        );
    }
    if sealed_responses && targets.is_empty() {
        return protocol.error(
            StatusCode::BAD_REQUEST,
            "unsupported_state",
            "当前连接无法回放 Bedrock 签名推理",
        );
    }
    if let Some(affinity) = &affinity
        && let Some(preferred) = affinity.preferred().await
    {
        targets.sort_by_key(|(target, _)| target.affinity_key != preferred);
    }
    let mut targets = targets.into_iter().peekable();
    'targets: while let Some((target, upstream_endpoint)) = targets.next() {
        let mut payload = if targets.peek().is_some() {
            data.clone()
        } else {
            std::mem::take(&mut data)
        };
        if let Some(cache) = &gemini_cache {
            payload.insert("cachedContent".into(), json!(cache.upstream_name));
        }
        if let Some(voice) = &custom_voice {
            payload.insert("voice_id".into(), json!(voice.upstream_id));
        }
        if gemini_count_envelope {
            payload = serde_json::Map::from_iter([(
                "generateContentRequest".into(),
                Value::Object(payload),
            )]);
        }
        payload.insert("model".into(), json!(target.upstream_model));
        let signed_reasoning = endpoint == TextEndpoint::Responses
            && upstream_endpoint == TextEndpoint::ChatCompletions
            && target.provider.supports_chat_reasoning_replay();
        let (mut payload, bridge) =
            if let Some(bridge) = Bridge::between(endpoint, upstream_endpoint) {
                match bridge.request(payload, signed_reasoning) {
                    Ok((payload, translation)) => (payload, Some(translation)),
                    Err(message) => {
                        conversion_error = Some(message);
                        continue;
                    }
                }
            } else {
                (Value::Object(payload), None)
            };
        let batch = if upstream_endpoint == TextEndpoint::Embeddings {
            match target.provider.split_embedding_relay(&payload) {
                Ok(batch) => batch,
                Err(message) => {
                    conversion_error = Some(message);
                    continue;
                }
            }
        } else {
            None
        };
        let relay_scope = if signed_reasoning {
            Some(crate::session_affinity::key(&[
                &key.relay_scope(&name, target),
                endpoint.path(),
            ]))
        } else if endpoint == TextEndpoint::ChatCompletions && upstream_endpoint == endpoint {
            Some(key.relay_scope(&name, target))
        } else {
            None
        };
        for part in batch.as_deref().unwrap_or(std::slice::from_ref(&payload)) {
            if let Err(message) = target
                .provider
                .validate_scoped_relay(upstream_endpoint, part, relay_scope.as_deref())
                .await
            {
                conversion_error = Some(message);
                continue 'targets;
            }
        }
        let charges = gateway
            .charge_plan(key, &name, target, upstream_endpoint, &payload)
            .map(|plan| {
                if let Some((_, characters)) = speech {
                    plan.with_input_characters(characters)
                } else if endpoint == TextEndpoint::XaiSpeechToText {
                    let redacted = uploads
                        .as_ref()
                        .is_some_and(|parts| parts.iter().any(|(name, _)| name == "redact"));
                    plan.with_audio_redaction(redacted)
                } else {
                    plan
                }
            });
        let budget = match gateway.budget_reservations(
            key,
            target,
            upstream_endpoint,
            &mut payload,
            charges.as_ref(),
        ) {
            Ok(reservation) => reservation,
            Err(message) => {
                budget_error = Some(message);
                continue;
            }
        };
        let reservation = match key.output_reservation(upstream_endpoint, target, &mut payload) {
            Ok(reservation) => reservation,
            Err(message) => {
                conversion_error = Some(message);
                continue;
            }
        };
        if let Some(cooldown) = target
            .account
            .as_ref()
            .and_then(|account| account.cooldown.as_ref())
        {
            match cooldown.remaining(cache_store).await {
                Ok(Some(seconds)) => {
                    cooling = Some(cooling.map_or(seconds, |current| current.min(seconds)));
                    continue;
                }
                Ok(None) => {}
                Err(_) => return account_state_unavailable(protocol),
            }
        }
        let lease = if let Some(account) = &target.account {
            match account.acquire(cache_store).await {
                Ok(Admission::Allowed(lease)) => lease,
                Ok(Admission::Busy) => {
                    busy = true;
                    continue;
                }
                Err(_) => return account_state_unavailable(protocol),
            }
        } else {
            None
        };
        if !admitted {
            if let Some(response) = admit(key, protocol).await? {
                return Ok(response);
            }
            admitted = true;
        }
        let mut calls = match gateway
            .usage(
                key,
                &name,
                target,
                (endpoint, upstream_endpoint),
                CallIdentity {
                    request_id: &request_id,
                    invocation: invocation.as_ref(),
                },
                AttemptAdmission {
                    resource_id: gemini_cache
                        .as_ref()
                        .map(|cache| format!("cachedContents/{}", cache.id)),
                    input_characters: speech.map(|(_, characters)| characters),
                    lease,
                    quota: reservation,
                    budget,
                    charges,
                    count: batch.as_ref().map_or(1, Vec::len),
                    operation: target.provider.relay_operation(upstream_endpoint, &payload),
                    hide_chat_usage: endpoint == TextEndpoint::ChatCompletions
                        && upstream_endpoint == endpoint
                        && target.provider.hide_chat_relay_usage(&payload),
                },
            )
            .await
        {
            Ok(calls) => calls,
            Err(UsageAdmission::Limited(retry_after)) => {
                return protocol
                    .error(
                        StatusCode::TOO_MANY_REQUESTS,
                        "output_quota_exceeded",
                        "分组输出 Token 配额不足",
                    )
                    .map(|response| {
                        response.header(header::RETRY_AFTER, HeaderValue::from(retry_after))
                    });
            }
            Err(UsageAdmission::BudgetLimited(retry_after)) => {
                return protocol
                    .error(
                        StatusCode::TOO_MANY_REQUESTS,
                        "budget_exceeded",
                        "分组货币预算不足",
                    )
                    .map(|response| {
                        response.header(header::RETRY_AFTER, HeaderValue::from(retry_after))
                    });
            }
            Err(UsageAdmission::AccountBudgetLimited(retry_after)) => {
                account_budget_retry =
                    Some(account_budget_retry.map_or(retry_after, |value| value.min(retry_after)));
                continue;
            }
            Err(UsageAdmission::Unavailable) => {
                return protocol.error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "usage_unavailable",
                    "AI 网关用量登记暂不可用",
                );
            }
        };
        if let Some(affinity) = &affinity {
            calls[0].bind_affinity(affinity.clone(), target.affinity_key.clone());
        }
        if let Some(batch) = batch {
            return embedding_batch::relay(target, calls, batch, name, protocol, cache_store).await;
        }
        let mut call = calls.pop().expect("已登记单次请求");
        match request_upstream(
            target,
            upstream_endpoint,
            payload,
            uploads.take(),
            relay_scope,
            protocol,
            &mut call,
        )
        .await
        {
            Ok(upstream) => {
                if let Some((format, _)) = speech {
                    return xai_speech::respond(upstream, format, call).await;
                }
                return respond(
                    upstream,
                    name,
                    endpoint,
                    stream_options,
                    protocol,
                    bridge,
                    call,
                )
                .await;
            }
            Err(rejected) => {
                call.settle().await;
                let cooldown = target
                    .account
                    .as_ref()
                    .and_then(|account| account.cooldown.as_ref());
                if let (Some(cooldown), Some(until)) = (cooldown, rejected.pause_until)
                    && cooldown.pause(cache_store, until).await.is_err()
                {
                    return account_state_unavailable(protocol);
                }
                call.release().await;
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
    if busy {
        return protocol.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_busy",
            "上游连接并发槽位已满",
        );
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
    if let Some(retry_after) = account_budget_retry {
        return protocol
            .error(
                StatusCode::TOO_MANY_REQUESTS,
                "upstream_budget_exceeded",
                "上游连接成本预算不足",
            )
            .map(|response| response.header(header::RETRY_AFTER, HeaderValue::from(retry_after)));
    }
    if let Some(message) = budget_error {
        return protocol.error(StatusCode::BAD_REQUEST, "budget_unbounded", message);
    }
    protocol.error(
        StatusCode::BAD_REQUEST,
        "unsupported_conversion",
        conversion_error.unwrap_or("没有支持此请求的上游连接"),
    )
}

fn countable_input(input: &Value) -> bool {
    if input.is_string() || input.is_null() {
        return true;
    }
    input.as_array().is_some_and(|items| {
        items.iter().all(|item| match item["type"].as_str() {
            Some("function_call") => {
                item["call_id"].is_string()
                    && item["name"].is_string()
                    && item["arguments"].as_str().is_some_and(|arguments| {
                        serde_json::from_str::<serde_json::Map<String, Value>>(arguments).is_ok()
                    })
            }
            Some("function_call_output") => {
                item["call_id"].is_string() && item["output"].is_string()
            }
            Some("message") | None => {
                let content = &item["content"];
                content.is_string()
                    || content.as_array().is_some_and(|parts| {
                        parts.iter().all(|part| {
                            matches!(part["type"].as_str(), Some("input_text" | "output_text"))
                                && part["text"].is_string()
                        })
                    })
            }
            _ => false,
        })
    })
}

fn includes_tool_history(data: &serde_json::Map<String, Value>) -> bool {
    let has_items = |field: &str, predicate: fn(&Value) -> bool| {
        data.get(field)
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(predicate))
    };
    has_items("input", |item| {
        matches!(
            item["type"].as_str(),
            Some(
                "function_call"
                    | "function_call_output"
                    | "custom_tool_call"
                    | "custom_tool_call_output"
                    | "web_search_call"
                    | "code_interpreter_call"
                    | "file_search_call"
            )
        )
    }) || has_items("messages", |message| {
        matches!(message["role"].as_str(), Some("tool" | "function"))
            || message
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|calls| !calls.is_empty())
            || message["content"].as_array().is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    matches!(
                        block["type"].as_str(),
                        Some("tool_use" | "tool_result" | "server_tool_use")
                    )
                })
            })
    }) || has_items("contents", |content| {
        content["parts"].as_array().is_some_and(|parts| {
            parts.iter().any(|part| {
                part.get("functionCall").is_some() || part.get("functionResponse").is_some()
            })
        })
    })
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
    match limiter.reserve(&key.limiter_key(), limit, 60).await {
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

fn request_too_large(protocol: Protocol) -> HttpResult {
    protocol.error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "request_too_large",
        "请求正文超出大小限制",
    )
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
    uploads: Option<Vec<(String, client::Part)>>,
    scope: Option<String>,
    protocol: Protocol,
    call: &mut Call,
) -> Result<client::Response, UpstreamFailure> {
    if !call.begin_dispatch().await {
        return Err(usage_storage_failure(protocol, call));
    }
    let dispatched = match uploads {
        Some(parts) => target.provider.relay_multipart(endpoint, data, parts).await,
        None => target.provider.scoped_relay(endpoint, data, scope).await,
    };
    let upstream = match dispatched {
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
    call.observe_headers(upstream.headers());
    if !call.persist_observation().await {
        return Err(usage_storage_failure(protocol, call));
    }
    if status.is_success() {
        return Ok(upstream);
    }
    if !endpoint.is_xai_voice()
        && let Ok(data) = upstream.json::<Value>()
    {
        call.observe_usage(&data);
        call.observe_bedrock_error_usage(&data);
        call.observe_final_cost(&data);
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
            .account
            .as_ref()
            .and_then(|account| account.cooldown.as_ref())
            .and_then(|policy| policy.deadline(upstream.headers())),
        _ => None,
    };
    Err(UpstreamFailure {
        retryable,
        pause_until,
        response,
    })
}

fn usage_storage_failure(protocol: Protocol, call: &mut Call) -> UpstreamFailure {
    call.outcome("usage_storage_error");
    UpstreamFailure {
        retryable: false,
        pause_until: None,
        response: protocol.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "usage_unavailable",
            "AI 网关无法持久化上游调用的用量证据",
        ),
    }
}

async fn respond(
    upstream: client::Response,
    name: String,
    endpoint: TextEndpoint,
    stream_options: Option<StreamOptions>,
    protocol: Protocol,
    bridge: Option<Translation>,
    mut call: Call,
) -> HttpResult {
    if let Some(options) = stream_options {
        return stream(upstream, name, call, protocol, bridge, endpoint, options).await;
    }
    let status = upstream.status();
    let headers = downstream_headers(upstream.headers());
    call.release().await;
    let Ok(mut data) = upstream.json::<Value>() else {
        call.outcome("invalid_upstream_response");
        call.settle().await;
        return protocol.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回有效 JSON",
        );
    };
    if endpoint == TextEndpoint::CountResponseTokens
        || (endpoint == TextEndpoint::CountMessageTokens && bridge.is_some())
    {
        let Some(count) = data["input_tokens"].as_u64() else {
            call.outcome("invalid_upstream_response");
            call.settle().await;
            return protocol.error(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "上游未返回有效的 Token 数",
            );
        };
        data = if endpoint == TextEndpoint::CountResponseTokens {
            json!({"object":"response.input_tokens","input_tokens":count})
        } else {
            json!({"input_tokens":count})
        };
    } else if endpoint == TextEndpoint::GeminiCountTokens && !gemini_count::valid_response(&data) {
        call.outcome("invalid_upstream_response");
        call.settle().await;
        return protocol.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回有效的 Gemini Token 数",
        );
    } else if endpoint == TextEndpoint::XaiSpeechToText {
        let Some(duration_ms) = xai_transcription::validate_response(&mut data) else {
            call.outcome("invalid_upstream_response");
            call.settle().await;
            return protocol.error(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "上游未返回有效的语音转录",
            );
        };
        call.observe_audio_duration(duration_ms, data["model"].as_str());
    } else if !endpoint.is_counting() {
        call.observe_usage(&data);
        call.observe_final_cost(&data);
        call.observe_buffered_images(&data);
    }
    if let Some(bridge) = bridge {
        let Some(response) = bridge.response(name.clone(), &data) else {
            call.outcome("invalid_upstream_response");
            call.settle().await;
            return protocol.error(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "无法转换上游协议响应",
            );
        };
        data = response;
    }
    if !call.complete().await {
        return protocol.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "usage_unavailable",
            "AI 网关无法持久化本次上游用量",
        );
    }
    if endpoint == TextEndpoint::Embeddings
        && let Some(object) = data.as_object_mut()
    {
        object.insert("model".into(), name.into());
    } else {
        rename_model(&mut data, &name);
    }
    Response::json(status, data).map(|response| response.with_headers(headers))
}

struct StreamOptions {
    format: StreamFormat,
    gemini_candidates: Option<u64>,
}

fn gemini_finished(data: &Value, candidates: u64, finished: &mut HashSet<u64>) -> bool {
    fn reason(data: &Value, unspecified: &str) -> bool {
        data.as_str()
            .is_some_and(|reason| !reason.is_empty() && reason != unspecified)
    }

    if let Some(parts) = data["candidates"].as_array() {
        finished.extend(parts.iter().filter_map(|candidate| {
            let index = candidate["index"].as_u64().unwrap_or(0);
            (index < candidates && reason(&candidate["finishReason"], "FINISH_REASON_UNSPECIFIED"))
                .then_some(index)
        }));
    }
    finished.len() as u64 == candidates
        || reason(
            &data["promptFeedback"]["blockReason"],
            "BLOCK_REASON_UNSPECIFIED",
        )
}

enum StreamFormat {
    EventSource,
    JsonArray { opened: bool },
}

impl StreamFormat {
    fn content_type(&self) -> &'static str {
        match self {
            Self::EventSource => "text/event-stream",
            Self::JsonArray { .. } => "application/json",
        }
    }

    fn item(&mut self, data: &Value) -> String {
        match self {
            Self::EventSource => format!("data: {data}\n\n"),
            Self::JsonArray { opened } => {
                let delimiter = if std::mem::replace(opened, true) {
                    ','
                } else {
                    '['
                };
                format!("{delimiter}{data}")
            }
        }
    }

    fn event(&mut self, event: eventsource_stream::Event, data: &Value) -> String {
        if matches!(self, Self::JsonArray { .. }) {
            return self.item(data);
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
        frame
    }

    fn error(&mut self, error: &Value) -> String {
        match self {
            Self::EventSource => format!("event: error\ndata: {error}\n\n"),
            Self::JsonArray { .. } => self.item(error) + "]",
        }
    }

    fn done(&self) -> &'static str {
        match self {
            Self::EventSource => "data: [DONE]\n\n",
            Self::JsonArray { opened: true } => "]",
            Self::JsonArray { opened: false } => "[]",
        }
    }
}

async fn stream(
    upstream: client::Response,
    name: String,
    mut call: Call,
    protocol: Protocol,
    bridge: Option<Translation>,
    endpoint: TextEndpoint,
    options: StreamOptions,
) -> HttpResult {
    let mut format = options.format;
    let status = upstream.status();
    let mut headers = downstream_headers(upstream.headers());
    let is_sse = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));
    if !is_sse {
        call.outcome("invalid_upstream_response");
        call.settle().await;
        return protocol.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回 SSE",
        );
    }
    let Ok(body) = upstream.into_stream() else {
        call.outcome("invalid_upstream_response");
        call.settle().await;
        return protocol.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "无法读取上游 SSE",
        );
    };
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    // HTTP 正文可能在请求语言作用域退出后才开始读取。
    if let Some(bridge) = bridge {
        let stream = bridged_stream(body, bridge.stream(name), call, protocol.stream_error());
        return Ok(Response::stream(status, stream).with_headers(headers));
    }
    let error = protocol.stream_error_body();
    let stream = async_stream::stream! {
        let mut events = body.eventsource();
        let mut terminal = false;
        let mut gemini_completed = HashSet::new();
        while let Some(event) = events.next().await {
            let Ok(event) = event else {
                call.outcome("stream_error");
                call.settle().await;
                yield Ok::<_, Infallible>(format.error(&error));
                return;
            };
            if event.data == "[DONE]" {
                if (endpoint == TextEndpoint::Responses || endpoint.is_image()
                    || endpoint == TextEndpoint::GeminiStreamGenerateContent) && !terminal {
                    call.outcome("incomplete_stream");
                    call.settle().await;
                    yield Ok(format.error(&error));
                    return;
                }
                if !call.complete().await {
                    yield Ok(format.error(&error));
                    return;
                }
                yield Ok(format.done().to_owned());
                return;
            }
            let Ok(mut data) = serde_json::from_str::<Value>(&event.data) else {
                call.outcome("stream_error");
                call.settle().await;
                yield Ok(format.error(&error));
                return;
            };
            if endpoint == TextEndpoint::GeminiStreamGenerateContent && !data.is_object() {
                call.outcome("stream_error");
                call.settle().await;
                yield Ok(format.error(&error));
                return;
            }
            if matches!(data["type"].as_str(), Some("error" | "response.failed")) || event.event == "error" || data.get("error").is_some_and(|value| !value.is_null()) {
                call.observe_bedrock_error_usage(&data);
                if endpoint == TextEndpoint::Responses && data["type"] == "response.failed" {
                    call.observe_usage(&data["response"]);
                    call.observe_final_cost(&data["response"]);
                }
                call.outcome("upstream_error");
                call.settle().await;
                yield Ok(format.error(&error));
                return;
            }
            let ended = match endpoint {
                TextEndpoint::ImagesGenerations => data["type"] == "image_generation.completed",
                TextEndpoint::ImagesEdits => data["type"] == "image_edit.completed",
                TextEndpoint::Responses => matches!(data["type"].as_str(), Some("response.completed" | "response.incomplete")),
                TextEndpoint::Messages => data["type"] == "message_stop",
                TextEndpoint::ChatCompletions => data["choices"].as_array().is_some_and(Vec::is_empty)
                    && data["usage"]["prompt_tokens"].is_u64()
                    && data["usage"]["completion_tokens"].is_u64(),
                _ => false,
            };
            terminal |= ended;
            if let Some(candidates) = options.gemini_candidates {
                terminal |= gemini_finished(&data, candidates, &mut gemini_completed);
            }
            let mut observed = call.observe_usage(&data);
            rename_model(&mut data, &name);
            for field in ["response", "message"] {
                if let Some(response) = data.get_mut(field) {
                    observed |= call.observe_usage(response);
                    rename_model(response, &name);
                }
            }
            if ended && endpoint == TextEndpoint::Responses {
                call.observe_final_cost(&data["response"]);
            }
            let final_usage = ended || (matches!(protocol, Protocol::Gemini)
                && terminal && data["usageMetadata"]["totalTokenCount"].is_u64());
            if final_usage && !call.complete().await {
                yield Ok(format.error(&error));
                return;
            }
            if !final_usage && observed && !call.persist_observation().await {
                call.outcome("usage_storage_error");
                yield Ok(format.error(&error));
                return;
            }
            if ended && endpoint == TextEndpoint::ChatCompletions && call.hide_chat_usage {
                continue;
            }
            yield Ok(format.event(event, &data));
        }
        if terminal {
            if !call.complete().await {
                yield Ok(format.error(&error));
            } else if matches!(format, StreamFormat::JsonArray { .. }) {
                yield Ok(format.done().to_owned());
            }
        } else {
            call.outcome("incomplete_stream");
            call.settle().await;
            yield Ok(format.error(&error));
        }
    };
    Ok(Response::stream(status, stream).with_headers(headers))
}

fn bridged_stream(
    body: client::ResponseStream,
    mut state: crate::compatibility::Stream,
    mut call: Call,
    error_frame: String,
) -> impl futures_util::Stream<Item = Result<String, Infallible>> {
    async_stream::stream! {
        let mut events = body.eventsource();
        while let Some(event) = events.next().await {
            let Ok(event) = event else {
                call.outcome("stream_error");
                call.settle().await;
                yield Ok(error_frame);
                return;
            };
            if event.event == "error" {
                call.outcome("upstream_error");
                call.settle().await;
                yield Ok(error_frame);
                return;
            }
            let mut observed = false;
            let converted = if event.data == "[DONE]" {
                state.finish()
            } else {
                let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
                    call.outcome("stream_error");
                    call.settle().await;
                    yield Ok(error_frame);
                    return;
                };
                if data.get("error").is_some_and(|value| !value.is_null()) {
                    call.observe_bedrock_error_usage(&data);
                    call.outcome("upstream_error");
                    call.settle().await;
                    yield Ok(error_frame);
                    return;
                }
                observed |= call.observe_usage(&data);
                observed |= call.observe_usage(&data["message"]);
                observed |= call.observe_usage(&data["response"]);
                state.convert(&data)
            };
            let Some((frames, terminal)) = converted else {
                call.outcome("invalid_upstream_response");
                call.settle().await;
                yield Ok(error_frame);
                return;
            };
            if terminal && !call.complete().await {
                yield Ok(error_frame);
                return;
            }
            if !terminal && observed && !call.persist_observation().await {
                call.outcome("usage_storage_error");
                yield Ok(error_frame);
                return;
            }
            for frame in frames {
                yield Ok(frame);
            }
            if terminal {
                return;
            }
        }
        call.outcome("incomplete_stream");
        call.settle().await;
        yield Ok(error_frame);
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
        "rate_limit_exceeded"
        | "output_quota_exceeded"
        | "budget_exceeded"
        | "upstream_budget_exceeded" => "rate_limit_error",
        "upstream_timeout"
        | "upstream_error"
        | "invalid_upstream_response"
        | "rate_limit_unavailable"
        | "usage_unavailable" => "server_error",
        _ => "invalid_request_error",
    };
    json!({"error": {"message": message, "type": kind, "code": code, "param": null}})
}
