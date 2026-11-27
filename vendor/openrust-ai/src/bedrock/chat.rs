use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_sdk_bedrockruntime::{
    operation::RequestId,
    types::{
        JsonSchemaDefinition, OutputConfig, OutputFormat, OutputFormatStructure, OutputFormatType,
    },
};
use futures_util::StreamExt;
use openrust::{
    http::{StatusCode, client::Response},
    support::Str,
};
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, CHAT_REASONING_REPLAY, FinishReason, ReasoningPart, StepEvent, Usage,
};

use super::{
    BedrockMessage, BedrockProvider, ContentBlock, ConversationRole, InferenceConfiguration,
    ReasoningContentBlock, SystemContentBlock, TimeoutConfig,
    chat_media::ChatMedia,
    chat_replay::{self, ChatReplay},
    chat_tools::{ChatTools, ToolHistory, has_only, wire_call},
    converse_error, finish_reason,
    relay::{error_response, relay_headers, relay_response},
    stream_events,
    structured::OutputMode,
    tools, usage,
};

pub(super) struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<BedrockMessage>,
    system: Vec<SystemContentBlock>,
    inference: Option<InferenceConfiguration>,
    output: Option<OutputConfig>,
    stream: bool,
    tools: ChatTools,
}

impl<'a> ChatRequest<'a> {
    pub(super) fn parse(
        body: &'a Value,
        replay: Option<&ChatReplay>,
    ) -> Result<Self, &'static str> {
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
                        | "tools"
                        | "tool_choice"
                        | "parallel_tool_calls"
                        | "response_format"
                )
        }) {
            return Err("Bedrock Chat 请求包含未支持的字段");
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
        let mut chat_tools = ChatTools::parse(body)?;
        let mut tool_history = ToolHistory::default();
        let mut media = ChatMedia::default();
        let wire_messages = body["messages"]
            .as_array()
            .ok_or("Bedrock Chat 缺少消息列表")?;
        for (index, message) in wire_messages.iter().enumerate() {
            let role = message["role"].as_str();
            if role == Some("tool") {
                messages.extend(tool_history.accept(message)?);
                continue;
            }
            tool_history.require_finished()?;
            let allowed = if role == Some("assistant") {
                &[
                    "role",
                    "content",
                    "tool_calls",
                    "reasoning_content",
                    CHAT_REASONING_REPLAY,
                ][..]
            } else {
                &["role", "content"][..]
            };
            if !has_only(message, allowed) {
                return Err("Bedrock Chat 消息包含未支持的字段");
            }
            match role {
                Some("system" | "developer") if messages.is_empty() => {
                    system.extend(
                        text_parts(&message["content"])?
                            .into_iter()
                            .map(|part| SystemContentBlock::Text(part.into())),
                    );
                }
                Some("user") => {
                    messages.push(
                        BedrockMessage::builder()
                            .role(ConversationRole::User)
                            .set_content(Some(media.user_content(&message["content"])?))
                            .build()
                            .map_err(|_| "无法构造 Bedrock Chat 用户消息")?,
                    );
                }
                Some("assistant") => {
                    let calls = if message["tool_calls"].is_null() {
                        Vec::new()
                    } else {
                        chat_tools.history_calls(&message["tool_calls"])?
                    };
                    let parts = if (!calls.is_empty()
                        || message.get(CHAT_REASONING_REPLAY).is_some())
                        && message["content"].is_null()
                    {
                        Vec::new()
                    } else {
                        text_parts(&message["content"])?
                    };
                    let native = if let Some(token) = message.get(CHAT_REASONING_REPLAY) {
                        replay
                            .ok_or("当前 Bedrock 连接无法回放签名推理")?
                            .restore(
                                token,
                                &wire_messages[..index],
                                &parts.concat(),
                                &calls,
                                now(),
                            )
                            .map_err(|_| "Bedrock 签名推理状态无效、已过期或与当前请求不匹配")?
                    } else {
                        let mut content: Vec<_> = parts
                            .into_iter()
                            .map(|text| ContentBlock::Text(text.into()))
                            .collect();
                        for call in &calls {
                            content.push(
                                tools::call_block(call).map_err(|_| "无法构造 Bedrock 工具调用")?,
                            );
                        }
                        BedrockMessage::builder()
                            .role(ConversationRole::Assistant)
                            .set_content(Some(content))
                            .build()
                            .map_err(|_| "无法构造 Bedrock Chat 助手消息")?
                    };
                    messages.push(native);
                    tool_history.begin(&calls);
                }
                _ => return Err("Bedrock Chat 支持前置系统指令及 user、assistant 文本"),
            }
        }
        tool_history.require_finished()?;
        if messages
            .last()
            .is_none_or(|message| message.role() != &ConversationRole::User)
        {
            return Err("Bedrock Chat 的最后一条消息须为 user");
        }
        Ok(Self {
            model,
            messages,
            system,
            inference: inference(body)?,
            output: output_config(&body["response_format"])?,
            stream,
            tools: chat_tools,
        })
    }
}

fn output_config(format: &Value) -> Result<Option<OutputConfig>, &'static str> {
    if format.is_null() || has_only(format, &["type"]) && format["type"] == "text" {
        return Ok(None);
    }
    if format["type"] != "json_schema" || !has_only(format, &["type", "json_schema"]) {
        return Err("Bedrock Chat 结构化输出需要 json_schema 格式");
    }
    let definition = &format["json_schema"];
    if !has_only(definition, &["name", "description", "schema", "strict"])
        || !(definition["strict"].is_null() || definition["strict"].is_boolean())
    {
        return Err("Bedrock Chat json_schema 定义无效");
    }
    let name = definition["name"]
        .as_str()
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b"_-".contains(&value))
        })
        .ok_or("json_schema.name 须为长度 1 至 64 的字母、数字、下划线或短横线")?;
    let description = match &definition["description"] {
        Value::Null => None,
        value => Some(
            value
                .as_str()
                .ok_or("json_schema.description 须为文本")?
                .to_owned(),
        ),
    };
    let schema = &definition["schema"];
    if !schema.is_object() || schema["type"] != "object" {
        return Err("json_schema.schema 须声明根对象");
    }
    let definition = JsonSchemaDefinition::builder()
        .name(name)
        .set_description(description)
        .schema(schema.to_string())
        .build()
        .map_err(|_| "无法构造 Bedrock JSON Schema 定义")?;
    let format = OutputFormat::builder()
        .r#type(OutputFormatType::JsonSchema)
        .structure(OutputFormatStructure::JsonSchema(definition))
        .build()
        .map_err(|_| "无法构造 Bedrock 输出格式")?;
    Ok(Some(OutputConfig::builder().text_format(format).build()))
}

fn valid_chat_json(structured: bool, reason: FinishReason, text: &str) -> bool {
    !structured
        || reason != FinishReason::Stop
        || serde_json::from_str::<serde_json::Map<String, Value>>(text).is_ok()
}

pub(super) fn text_parts(body: &Value) -> Result<Vec<&str>, &'static str> {
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
    pub(super) async fn relay_chat(&self, body: Value, scope: Option<&str>) -> AiResult<Response> {
        let authentication = self.replay_authentication(scope.is_some()).await?;
        let replay = ChatReplay::new(
            authentication.scope.as_deref(),
            scope,
            body["model"].as_str(),
        );
        let request =
            ChatRequest::parse(&body, replay.as_ref()).map_err(|_| AiError::InvalidPrompt)?;
        let mut chat_tools = request.tools;
        let tool_config = chat_tools.config.take();
        let client = self.client().await?;
        let structured = request.output.is_some();
        // AWS 首次编译新 Schema 约需 90 秒，结构化调用须为编译与生成保留时间。
        let duration = Duration::from_secs(if structured { 120 } else { 60 });
        let timeout = authentication.pin(
            aws_sdk_bedrockruntime::config::Builder::new()
                .timeout_config(TimeoutConfig::builder().operation_timeout(duration).build()),
        );
        let system = (!request.system.is_empty()).then_some(request.system);
        let history = replay.as_ref().and_then(|_| {
            body["messages"]
                .as_array()
                .map(|messages| chat_replay::history(messages))
        });
        let identity = ChatIdentity::new(body["stream_options"]["include_usage"] == true);
        if request.stream {
            let deadline = tokio::time::Instant::now() + duration;
            let allow_tools = tool_config.is_some();
            let result = client
                .converse_stream()
                .model_id(request.model)
                .set_messages(Some(request.messages))
                .set_system(system)
                .set_inference_config(request.inference)
                .set_tool_config(tool_config)
                .set_output_config(request.output)
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
            let mut events = stream_events(
                stream,
                deadline,
                allow_tools,
                OutputMode::Text,
                replay.is_some(),
                None,
            );
            let stream = async_stream::stream! {
                yield Ok(identity.frame(json!([{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]), None));
                while let Some(event) = events.next().await {
                    let delta = match event {
                        Ok(StepEvent::TextPartDelta {delta, ..} | StepEvent::TextDelta(delta)) => json!({"content":delta}),
                        Ok(StepEvent::Reasoning(ReasoningPart::Delta {delta,..})) => json!({"reasoning_content":delta}),
                        Ok(StepEvent::Finished {response,..}) => {
                            let tool_turn = chat_tools.accepts(response.finish_reason, &response.tool_calls)
                                && valid_chat_json(structured, response.finish_reason, response.text.as_deref().unwrap_or_default());
                            let private = chat_replay::private_blocks(response.provider_content.as_ref());
                            let token = seal(replay.as_ref(), history, response.text.as_deref().unwrap_or_default(), &response.tool_calls, private);
                            let Ok(token) = token.and_then(|value| tool_turn.then_some(value).ok_or(AiError::InvalidResponse)) else {
                                let failure = json!({"error":{"message":"Bedrock Chat 响应无法转换"},
                                    "usage":usage_json(&response.usage)});
                                yield Ok(format!("data: {failure}\n\n").into());
                                return;
                            };
                            if let Some(token) = token {
                                yield Ok(identity.frame(json!([{"index":0,"delta":{CHAT_REASONING_REPLAY:token},"finish_reason":null}]), None));
                            }
                            if !response.tool_calls.is_empty() {
                                let calls = response.tool_calls.iter().enumerate().map(|(index, call)| {
                                    let mut data = wire_call(call);
                                    data["index"] = index.into();
                                    data
                                }).collect::<Vec<_>>();
                                yield Ok(identity.frame(json!([{"index":0,"delta":{"tool_calls":calls},"finish_reason":null}]), None));
                            }
                            yield Ok(identity.frame(json!([{"index":0,"delta":{},"finish_reason":response.finish_reason.as_str()}]), None));
                            yield Ok(identity.frame(json!([]), Some(usage_json(&response.usage))));
                            yield Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n"));
                            return;
                        }
                        Err(_) => {
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
            .set_tool_config(tool_config)
            .set_output_config(request.output)
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
        let usage = usage_json(&usage(response.usage()));
        let invalid = || {
            relay_response(
                StatusCode::BAD_GATEWAY,
                &json!({"usage":usage}),
                response.request_id(),
            )
        };
        let message = response
            .output()
            .and_then(|output| output.as_message().ok());
        let Some(message) =
            message.filter(|message| message.role() == &ConversationRole::Assistant)
        else {
            return invalid();
        };
        let mut text = String::new();
        let mut reasoning = Vec::new();
        let mut calls = Vec::new();
        let mut signed = false;
        for part in message.content() {
            match part {
                ContentBlock::Text(part) => text.push_str(part),
                ContentBlock::ReasoningContent(ReasoningContentBlock::ReasoningText(part)) => {
                    reasoning.push(part.text());
                    signed |= part.signature().is_some();
                }
                ContentBlock::ReasoningContent(ReasoningContentBlock::RedactedContent(_)) => {
                    signed = true
                }
                ContentBlock::ToolUse(call) if call.r#type().is_none() => {
                    let Ok(call) = tools::read_call(call) else {
                        return invalid();
                    };
                    calls.push(call);
                }
                _ => return invalid(),
            }
        }
        if !chat_tools.accepts(reason, &calls) || !valid_chat_json(structured, reason, &text) {
            return invalid();
        }
        let token = if signed
            && (!calls.is_empty() || (replay.is_some() && reason == FinishReason::Stop))
        {
            let Ok(blocks) = tools::read_output(message.content()) else {
                return invalid();
            };
            let Ok(Value::Array(blocks)) = serde_json::to_value(blocks) else {
                return invalid();
            };
            match seal(replay.as_ref(), history, &text, &calls, Some(&blocks)) {
                Ok(token) => token,
                Err(_) => return invalid(),
            }
        } else {
            None
        };
        let content = if text.is_empty() && !calls.is_empty() {
            Value::Null
        } else {
            text.into()
        };
        let mut message = json!({"role":"assistant","content":content});
        if !calls.is_empty() {
            message["tool_calls"] = Value::Array(calls.iter().map(wire_call).collect());
        }
        if let Some(token) = token {
            message[CHAT_REASONING_REPLAY] = token.into();
        }
        if !reasoning.is_empty() {
            message["reasoning_content"] = reasoning.join("\n\n").into();
        }
        let data = json!({
            "id":identity.id,"created":identity.created,"object":"chat.completion","model":"",
            "choices":[{"index":0,"message":message,"finish_reason":reason.as_str()}],
            "usage":usage,
        });
        relay_response(StatusCode::OK, &data, response.request_id())
    }
}

pub(super) fn chat_reason(reason: FinishReason) -> bool {
    matches!(
        reason,
        FinishReason::Stop | FinishReason::Length | FinishReason::ContentFilter
    )
}

fn seal(
    replay: Option<&ChatReplay>,
    history: Option<String>,
    text: &str,
    calls: &[crate::ToolCall],
    blocks: Option<&[Value]>,
) -> AiResult<Option<String>> {
    let Some(blocks) = blocks else {
        return Ok(None);
    };
    let (Some(replay), Some(history)) = (replay, history) else {
        return Err(AiError::InvalidResponse);
    };
    replay.seal(history, text, calls, blocks, now()).map(Some)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
            created: now(),
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
