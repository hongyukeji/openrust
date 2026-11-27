use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
    time::Duration,
};

use aws_sdk_bedrockruntime::{
    config::ConfigBag,
    operation::RequestId,
    types::{
        ConverseTokensRequest, CountTokensInput, ToolConfiguration, ToolResultBlock,
        ToolResultContentBlock, ToolResultStatus,
    },
};
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        interceptors::{Intercept, context::AfterDeserializationInterceptorContextRef},
        runtime_components::RuntimeComponents,
    },
};
use http::StatusCode;
use openrust::http::client::Response;
use serde_json::{Value, json};

use super::{
    BedrockMessage, BedrockProvider, ContentBlock, ConversationRole, SystemContentBlock,
    TimeoutConfig,
    chat::text_parts,
    chat_media::ChatMedia,
    chat_tools::{ChatTools, has_only, id, name},
    converse_error,
    invoke::InvokeFailure,
    relay::{error_response, relay_response},
    tools,
};
use crate::{AiError, AiResult, ToolCall};

pub(super) struct CountRequest<'a> {
    model: &'a str,
    input: CountTokensInput,
}

impl<'a> CountRequest<'a> {
    pub(super) fn parse(body: &'a Value) -> Result<Self, &'static str> {
        if !has_only(
            body,
            &[
                "model",
                "messages",
                "system",
                "tools",
                "tool_choice",
                "stream",
            ],
        ) || !(body["stream"].is_null() || body["stream"] == false)
        {
            return Err("Bedrock 计数请求包含不支持的字段");
        }
        let model = body["model"]
            .as_str()
            .filter(|model| !model.trim().is_empty())
            .ok_or("Bedrock 计数需要模型")?;
        let system = if body["system"].is_null() {
            None
        } else {
            Some(
                text_parts(&body["system"])?
                    .into_iter()
                    .map(|text| SystemContentBlock::Text(text.into()))
                    .collect(),
            )
        };
        let messages = body["messages"]
            .as_array()
            .filter(|messages| !messages.is_empty())
            .ok_or("Bedrock 计数需要消息")?;
        let mut media = ChatMedia::default();
        let mut pending = HashSet::new();
        let mut seen = HashSet::new();
        let messages = messages
            .iter()
            .map(|message| parse_message(message, &mut media, &mut pending, &mut seen))
            .collect::<Result<Vec<_>, _>>()?;
        if !pending.is_empty() {
            return Err("Bedrock 计数需要完整的工具调用和结果");
        }
        let tool_config = tool_config(body)?;
        if tool_config.is_none() && !seen.is_empty() {
            return Err("Bedrock 计数的工具历史需要有效的工具定义");
        }
        let input = ConverseTokensRequest::builder()
            .set_system(system)
            .set_messages(Some(messages))
            .set_tool_config(tool_config)
            .build();
        Ok(Self {
            model,
            input: CountTokensInput::Converse(input),
        })
    }
}

fn tool_config(body: &Value) -> Result<Option<ToolConfiguration>, &'static str> {
    let choice = &body["tool_choice"];
    let choice = if choice.is_null() {
        Value::Null
    } else {
        if !has_only(choice, &["type", "name", "disable_parallel_tool_use"])
            || !(choice["disable_parallel_tool_use"].is_null()
                || choice["disable_parallel_tool_use"] == false)
        {
            return Err("Bedrock 计数不支持此工具选择选项");
        }
        match choice["type"].as_str() {
            Some(kind @ ("auto" | "none")) if choice["name"].is_null() => kind.into(),
            Some("any") if choice["name"].is_null() => "required".into(),
            Some("tool") => json!({"type":"function","function":{"name":name(&choice["name"])?}}),
            _ => return Err("Bedrock 计数工具选择无效"),
        }
    };
    let definitions = &body["tools"];
    let definitions = if definitions.is_null() {
        Vec::new()
    } else {
        definitions
            .as_array()
            .ok_or("Bedrock 计数的工具须为数组")?
            .iter()
            .map(|tool| {
                if !has_only(
                    tool,
                    &["type", "name", "description", "input_schema", "strict"],
                ) || !(tool["type"].is_null() || tool["type"] == "custom")
                    || !tool["input_schema"].is_object()
                {
                    return Err("Bedrock 计数只支持带输入 Schema 的客户端函数");
                }
                Ok(json!({"type":"function","function":{
                    "name":tool["name"],"description":tool["description"],
                    "parameters":tool["input_schema"],"strict":tool["strict"]
                }}))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    if definitions.is_empty() && !choice.is_null() {
        return Err("Bedrock 计数的工具选择需要工具定义");
    }
    Ok(ChatTools::parse(&json!({"tools":definitions,"tool_choice":choice}))?.config)
}

fn parse_message<'a>(
    message: &'a Value,
    media: &mut ChatMedia,
    pending: &mut HashSet<&'a str>,
    seen: &mut HashSet<&'a str>,
) -> Result<BedrockMessage, &'static str> {
    if !has_only(message, &["role", "content"]) {
        return Err("Bedrock 计数的消息包含不支持的字段");
    }
    let role = match message["role"].as_str() {
        Some("user") => ConversationRole::User,
        Some("assistant") if pending.is_empty() => ConversationRole::Assistant,
        _ => return Err("Bedrock 计数的消息角色或工具历史无效"),
    };
    let content = &message["content"];
    let blocks = if let Some(parts) = content.as_array() {
        parse_parts(parts, &role, media, pending, seen)?
    } else {
        text_parts(content)?
            .into_iter()
            .map(|text| ContentBlock::Text(text.into()))
            .collect()
    };
    if blocks.is_empty() || role == ConversationRole::User && !pending.is_empty() {
        return Err("Bedrock 计数的消息为空或缺少工具结果");
    }
    BedrockMessage::builder()
        .role(role)
        .set_content(Some(blocks))
        .build()
        .map_err(|_| "无法构造 Bedrock 计数消息")
}

fn parse_parts<'a>(
    parts: &'a [Value],
    role: &ConversationRole,
    media: &mut ChatMedia,
    pending: &mut HashSet<&'a str>,
    seen: &mut HashSet<&'a str>,
) -> Result<Vec<ContentBlock>, &'static str> {
    if parts.iter().any(|part| part["type"] == "document")
        && !parts.iter().any(|part| part["type"] == "text")
    {
        return Err("Bedrock 文档消息须同时包含文本说明");
    }
    parts
        .iter()
        .map(|part| match part["type"].as_str() {
            Some("text") if has_only(part, &["type", "text"]) => part["text"]
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(|text| ContentBlock::Text(text.into()))
                .ok_or("Bedrock 计数的文本块须为非空文本"),
            Some("tool_use") if *role == ConversationRole::Assistant => {
                if !has_only(part, &["type", "id", "name", "input"]) {
                    return Err("Bedrock 计数的工具调用包含不支持的字段");
                }
                let id = id(&part["id"])?;
                if !seen.insert(id) {
                    return Err("Bedrock 计数的工具调用 ID 重复");
                }
                pending.insert(id);
                tools::call_block(&ToolCall {
                    id: id.into(),
                    name: name(&part["name"])?.into(),
                    arguments: part["input"].as_object().ok_or("工具输入须为对象")?.clone(),
                    result_id: None,
                })
                .map_err(|_| "无法构造 Bedrock 计数的工具调用")
            }
            Some("tool_result") if *role == ConversationRole::User => tool_result(part, pending),
            Some(kind @ ("image" | "document")) if *role == ConversationRole::User => {
                media.counting_attachment(kind, part)
            }
            _ => Err("Bedrock 计数的消息包含不支持的内容"),
        })
        .collect()
}

fn tool_result<'a>(
    part: &'a Value,
    pending: &mut HashSet<&'a str>,
) -> Result<ContentBlock, &'static str> {
    if !has_only(part, &["type", "tool_use_id", "is_error", "content"]) {
        return Err("Bedrock 计数的工具结果包含不支持的字段");
    }
    let id = id(&part["tool_use_id"])?;
    if !pending.remove(id) {
        return Err("Bedrock 计数的工具结果与前一调用不匹配");
    }
    let status = match part["is_error"] {
        Value::Bool(true) => Some(ToolResultStatus::Error),
        Value::Bool(false) => Some(ToolResultStatus::Success),
        Value::Null => None,
        _ => return Err("is_error 须为布尔值"),
    };
    let content = if part["content"].is_null() {
        vec![ToolResultContentBlock::Text(String::new())]
    } else if let Some(text) = part["content"].as_str() {
        vec![ToolResultContentBlock::Text(text.into())]
    } else {
        text_parts(&part["content"])?
            .into_iter()
            .map(|text| ToolResultContentBlock::Text(text.into()))
            .collect()
    };
    ToolResultBlock::builder()
        .tool_use_id(id)
        .set_content(Some(content))
        .set_status(status)
        .build()
        .map(ContentBlock::ToolResult)
        .map_err(|_| "无法构造 Bedrock 计数的工具结果")
}

impl BedrockProvider {
    pub(super) async fn relay_count(&self, body: &Value) -> AiResult<Response> {
        let request = CountRequest::parse(body).map_err(|_| AiError::InvalidPrompt)?;
        let timeout = TimeoutConfig::builder()
            .operation_timeout(Duration::from_secs(30))
            .build();
        let confirmed_count = Arc::new(OnceLock::new());
        let result = self
            .client()
            .await?
            .count_tokens()
            .model_id(request.model)
            .input(request.input)
            .customize()
            .config_override(aws_sdk_bedrockruntime::config::Builder::new().timeout_config(timeout))
            .interceptor(CountResponseEvidence(confirmed_count.clone()))
            .send()
            .await;
        match result {
            Ok(output) => match confirmed_count.get() {
                Some(&count) if count == output.input_tokens() => relay_response(
                    StatusCode::OK,
                    &json!({"input_tokens":count}),
                    output.request_id(),
                ),
                _ => relay_response(StatusCode::BAD_GATEWAY, &json!({}), output.request_id()),
            },
            Err(error) => {
                let request_id = error.request_id().map(str::to_owned);
                let error = if error
                    .raw_response()
                    .is_some_and(|raw| raw.status().is_success())
                {
                    AiError::InvalidResponse
                } else {
                    converse_error(error)
                };
                error_response(InvokeFailure { error, request_id })
            }
        }
    }
}

#[derive(Debug)]
struct CountResponseEvidence(Arc<OnceLock<i32>>);

impl Intercept for CountResponseEvidence {
    fn name(&self) -> &'static str {
        "BedrockCountResponseEvidence"
    }

    fn read_after_deserialization(
        &self,
        context: &AfterDeserializationInterceptorContextRef<'_>,
        _: &RuntimeComponents,
        _: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let response = context.response();
        if response.status().is_success()
            && let Some(bytes) = response.body().bytes()
            && let Ok(payload) = serde_json::from_slice::<Value>(bytes)
            && let Some(value) = payload["inputTokens"].as_u64()
            && let Ok(count) = i32::try_from(value)
        {
            // AWS SDK 会把缺失的计数初始化为零，只接受原始响应中的实际值。
            let _ = self.0.set(count);
        }
        Ok(())
    }
}
