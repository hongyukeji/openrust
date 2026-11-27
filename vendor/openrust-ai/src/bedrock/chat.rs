use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_sdk_bedrockruntime::operation::RequestId;
use futures_util::StreamExt;
use openrust::{
    http::{StatusCode, client::Response},
    support::Str,
};
use serde_json::{Value, json};

use crate::{AiError, AiResult, FinishReason, ReasoningPart, StepEvent, Usage};

use super::{
    BedrockMessage, BedrockProvider, ContentBlock, ConversationRole, InferenceConfiguration,
    ReasoningContentBlock, SystemContentBlock, TimeoutConfig, converse_error, finish_reason,
    relay::{error_response, relay_headers, relay_response},
    stream_events,
    structured::OutputMode,
    usage,
};

pub(super) struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<BedrockMessage>,
    system: Vec<SystemContentBlock>,
    inference: Option<InferenceConfiguration>,
    stream: bool,
}

impl<'a> ChatRequest<'a> {
    pub(super) fn parse(body: &'a Value) -> Result<Self, &'static str> {
        let object = body.as_object().ok_or("Bedrock Chat 请求需要 JSON 对象")?;
        if object.iter().any(|(key, value)| {
            !value.is_null()
                && !matches!(
                    key.as_str(),
                    "model"
                        | "messages"
                        | "max_tokens"
                        | "max_completion_tokens"
                        | "temperature"
                        | "top_p"
                        | "stop"
                        | "n"
                        | "stream"
                        | "stream_options"
                        | "user"
                )
        }) {
            return Err("当前 Bedrock Chat 只转换纯文本及其生成参数");
        }
        if !body["n"].is_null() && body["n"].as_u64() != Some(1) {
            return Err("Bedrock Chat 每次只支持一个候选回答");
        }
        let stream = match &body["stream"] {
            Value::Bool(value) => *value,
            Value::Null => false,
            _ => return Err("stream 必须为布尔值"),
        };
        if !body["stream_options"].is_null()
            && (!stream
                || !body["stream_options"].as_object().is_some_and(|options| {
                    options
                        .iter()
                        .all(|(name, value)| name == "include_usage" && value.is_boolean())
                }))
        {
            return Err("Bedrock Chat 流选项只支持 include_usage");
        }
        let model = body["model"]
            .as_str()
            .filter(|model| !model.trim().is_empty())
            .ok_or("Bedrock Chat 缺少模型")?;
        let mut system = Vec::new();
        let mut messages = Vec::new();
        for message in body["messages"]
            .as_array()
            .ok_or("Bedrock Chat 缺少消息列表")?
        {
            let object = message.as_object().ok_or("Bedrock Chat 消息须为对象")?;
            if object
                .keys()
                .any(|key| !matches!(key.as_str(), "role" | "content"))
            {
                return Err("Bedrock Chat 暂不转换带名称、工具或其他扩展字段的消息");
            }
            let parts = text_parts(&message["content"])?;
            match message["role"].as_str() {
                Some("system" | "developer") if messages.is_empty() => {
                    system.extend(
                        parts
                            .into_iter()
                            .map(|part| SystemContentBlock::Text(part.into())),
                    );
                }
                Some("user" | "assistant") => {
                    let role = if message["role"] == "user" {
                        ConversationRole::User
                    } else {
                        ConversationRole::Assistant
                    };
                    let native = BedrockMessage::builder()
                        .role(role)
                        .set_content(Some(
                            parts
                                .into_iter()
                                .map(|text| ContentBlock::Text(text.into()))
                                .collect(),
                        ))
                        .build()
                        .map_err(|_| "无法构造 Bedrock Chat 消息")?;
                    messages.push(native);
                }
                _ => return Err("Bedrock Chat 支持前置系统指令及 user、assistant 文本"),
            }
        }
        if messages
            .last()
            .is_none_or(|message| message.role() != &ConversationRole::User)
        {
            return Err("Bedrock Chat 的最后一条消息须为 user 文本");
        }
        Ok(Self {
            model,
            messages,
            system,
            inference: inference(body)?,
            stream,
        })
    }
}

fn text_parts(body: &Value) -> Result<Vec<&str>, &'static str> {
    let parts = match body {
        Value::String(text) => Some(vec![text.as_str()]),
        Value::Array(items) => items
            .iter()
            .map(|part| {
                (part["type"] == "text"
                    && part.as_object().is_some_and(|object| {
                        object
                            .keys()
                            .all(|key| matches!(key.as_str(), "type" | "text"))
                    }))
                .then(|| part["text"].as_str())
                .flatten()
            })
            .collect(),
        _ => None,
    };
    parts
        .filter(|items| !items.is_empty() && items.iter().all(|text| !text.trim().is_empty()))
        .ok_or("Bedrock Chat 消息须包含非空文本")
}

fn inference(body: &Value) -> Result<Option<InferenceConfiguration>, &'static str> {
    let max_tokens = match (&body["max_tokens"], &body["max_completion_tokens"]) {
        (Value::Null, Value::Null) => None,
        (Value::Null, value) | (value, Value::Null) => Some(
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or("最大输出 Token 须为正整数")?,
        ),
        _ => return Err("请只指定一个最大输出 Token 参数"),
    };
    let probability = |field: &str| match &body[field] {
        Value::Null => Ok(None),
        value => value
            .as_f64()
            .filter(|value| (0.0..=1.0).contains(value))
            .map(|value| Some(value as f32))
            .ok_or("Bedrock 的 temperature 和 top_p 须介于 0 与 1"),
    };
    let temperature = probability("temperature")?;
    let top_p = probability("top_p")?;
    let stop = match &body["stop"] {
        Value::Null => Some(Vec::new()),
        Value::String(text) => Some(vec![text.clone()]),
        Value::Array(items) => items
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    }
    .filter(|items| items.len() <= 4 && items.iter().all(|item| !item.is_empty()))
    .ok_or("Bedrock Chat 支持最多四个非空停止字符串")?;
    Ok(
        (max_tokens.is_some() || temperature.is_some() || top_p.is_some() || !stop.is_empty())
            .then(|| {
                InferenceConfiguration::builder()
                    .set_max_tokens(max_tokens)
                    .set_temperature(temperature)
                    .set_top_p(top_p)
                    .set_stop_sequences((!stop.is_empty()).then_some(stop))
                    .build()
            }),
    )
}

impl BedrockProvider {
    pub(super) async fn relay_chat(&self, body: Value) -> AiResult<Response> {
        let request = ChatRequest::parse(&body).map_err(|_| AiError::InvalidPrompt)?;
        let client = self.client().await?;
        let duration = Duration::from_secs(60);
        let timeout = aws_sdk_bedrockruntime::config::Builder::new()
            .timeout_config(TimeoutConfig::builder().operation_timeout(duration).build());
        let system = (!request.system.is_empty()).then_some(request.system);
        let identity = ChatIdentity::new(body["stream_options"]["include_usage"] == true);
        if request.stream {
            let deadline = tokio::time::Instant::now() + duration;
            let result = client
                .converse_stream()
                .model_id(request.model)
                .set_messages(Some(request.messages))
                .set_system(system)
                .set_inference_config(request.inference)
                .customize()
                .config_override(timeout)
                .send()
                .await;
            let stream = match result {
                Ok(stream) => stream,
                Err(error) => {
                    let request_id = error
                        .as_service_error()
                        .and_then(RequestId::request_id)
                        .map(str::to_owned);
                    return error_response(super::invoke::InvokeFailure {
                        error: converse_error(error),
                        request_id,
                    });
                }
            };
            let headers = relay_headers("text/event-stream", stream.request_id());
            let mut events = stream_events(stream, deadline, false, OutputMode::Text);
            let stream = async_stream::stream! {
                yield Ok(identity.frame(json!([{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]), None));
                while let Some(event) = events.next().await {
                    let delta = match event {
                        Ok(StepEvent::TextPartDelta {delta, ..} | StepEvent::TextDelta(delta)) => json!({"content":delta}),
                        Ok(StepEvent::Reasoning(ReasoningPart::Delta {delta,..})) => json!({"reasoning_content":delta}),
                        Ok(StepEvent::Finished {response,..}) if chat_reason(response.finish_reason) => {
                            yield Ok(identity.frame(json!([{"index":0,"delta":{},"finish_reason":response.finish_reason.as_str()}]), None));
                            yield Ok(identity.frame(json!([]), Some(usage_json(&response.usage))));
                            yield Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n"));
                            return;
                        }
                        Ok(StepEvent::Finished {..}) | Err(_) => {
                            yield Ok(bytes::Bytes::from_static(b"data: {\"error\":{\"message\":\"Bedrock stream incomplete\"}}\n\n"));
                            return;
                        }
                        _ => continue,
                    };
                    yield Ok(identity.frame(json!([{"index":0,"delta":delta,"finish_reason":null}]), None));
                }
            };
            return Ok(Response::new_stream(
                StatusCode::OK,
                headers,
                Box::pin(stream),
            ));
        }
        let result = client
            .converse()
            .model_id(request.model)
            .set_messages(Some(request.messages))
            .set_system(system)
            .set_inference_config(request.inference)
            .customize()
            .config_override(timeout)
            .send()
            .await;
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                let request_id = error
                    .as_service_error()
                    .and_then(RequestId::request_id)
                    .map(str::to_owned);
                return error_response(super::invoke::InvokeFailure {
                    error: converse_error(error),
                    request_id,
                });
            }
        };
        let reason = finish_reason(response.stop_reason().as_str());
        let message = response
            .output()
            .and_then(|output| output.as_message().ok());
        let Some(message) = message.filter(|message| {
            message.role() == &ConversationRole::Assistant && chat_reason(reason)
        }) else {
            return relay_response(StatusCode::BAD_GATEWAY, &json!({}), response.request_id());
        };
        let mut text = String::new();
        let mut reasoning = Vec::new();
        for part in message.content() {
            match part {
                ContentBlock::Text(part) => text.push_str(part),
                ContentBlock::ReasoningContent(ReasoningContentBlock::ReasoningText(part)) => {
                    reasoning.push(part.text())
                }
                ContentBlock::ReasoningContent(ReasoningContentBlock::RedactedContent(_)) => {}
                _ => {
                    return relay_response(
                        StatusCode::BAD_GATEWAY,
                        &json!({}),
                        response.request_id(),
                    );
                }
            }
        }
        let mut message = json!({"role":"assistant","content":text});
        if !reasoning.is_empty() {
            message["reasoning_content"] = reasoning.join("\n\n").into();
        }
        let data = json!({
            "id":identity.id,"created":identity.created,"object":"chat.completion","model":"",
            "choices":[{"index":0,"message":message,"finish_reason":reason.as_str()}],
            "usage":usage_json(&usage(response.usage())),
        });
        relay_response(StatusCode::OK, &data, response.request_id())
    }
}

fn chat_reason(reason: FinishReason) -> bool {
    matches!(
        reason,
        FinishReason::Stop | FinishReason::Length | FinishReason::ContentFilter
    )
}

fn usage_json(usage: &Usage) -> Value {
    let total = usage
        .prompt_tokens
        .zip(usage.completion_tokens)
        .and_then(|(input, output)| input.checked_add(output));
    json!({
        "prompt_tokens":usage.prompt_tokens,"completion_tokens":usage.completion_tokens,
        "total_tokens":total,"prompt_tokens_details":{
            "cached_tokens":usage.cache_read_input_tokens,
            "cache_write_tokens":usage.cache_write_input_tokens,
        },
    })
}

struct ChatIdentity {
    id: String,
    created: u64,
    include_usage: bool,
}

impl ChatIdentity {
    fn new(include_usage: bool) -> Self {
        Self {
            id: format!("chatcmpl-{}", Str::ulid()),
            created: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            include_usage,
        }
    }

    fn frame(&self, choices: Value, usage: Option<Value>) -> bytes::Bytes {
        let mut chunk = json!({
            "id":self.id,"created":self.created,"object":"chat.completion.chunk","model":"",
            "choices":choices,
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        } else if self.include_usage {
            chunk["usage"] = Value::Null;
        }
        format!("data: {chunk}\n\n").into()
    }
}
