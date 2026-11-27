use std::collections::HashMap;

use aws_sdk_bedrockruntime::{
    primitives::Blob,
    types::{
        AnyToolChoice, AutoToolChoice, ContentBlock, ConversationRole, Message as BedrockMessage,
        ReasoningContentBlock, ReasoningTextBlock, SpecificToolChoice, Tool as AwsTool,
        ToolChoice as AwsToolChoice, ToolConfiguration, ToolInputSchema, ToolResultBlock,
        ToolResultContentBlock, ToolSpecification, ToolUseBlock,
    },
};
use aws_smithy_types::{Document, Number};
use openrust::support::Str;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{AiError, AiResult, ToolCall, ToolChoice, ToolDefinition, ToolResult, ToolStep};

pub(super) fn configuration(
    tools: &[ToolDefinition],
    choice: Option<&ToolChoice>,
) -> AiResult<Option<ToolConfiguration>> {
    if tools.is_empty() || choice == Some(&ToolChoice::None) {
        return Ok(None);
    }
    let mut specs = Vec::with_capacity(tools.len());
    for tool in tools {
        let spec = ToolSpecification::builder()
            .name(&tool.name)
            .description(&tool.description)
            .input_schema(ToolInputSchema::Json(document(&tool.schema)?))
            .build()
            .map_err(|_| AiError::InvalidTools)?;
        specs.push(AwsTool::ToolSpec(spec));
    }
    let choice = match choice {
        Some(ToolChoice::Required) => AwsToolChoice::Any(AnyToolChoice::builder().build()),
        Some(ToolChoice::Tool(name)) => AwsToolChoice::Tool(
            SpecificToolChoice::builder()
                .name(name)
                .build()
                .map_err(|_| AiError::InvalidToolChoice)?,
        ),
        _ => AwsToolChoice::Auto(AutoToolChoice::builder().build()),
    };
    ToolConfiguration::builder()
        .set_tools(Some(specs))
        .tool_choice(choice)
        .build()
        .map(Some)
        .map_err(|_| AiError::InvalidTools)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReplayBlock {
    Text(String),
    SignedReasoning { text: String, signature: String },
    RedactedReasoning(String),
    Tool(ToolCall),
}

impl ReplayBlock {
    fn from_aws(block: &ContentBlock) -> AiResult<Option<Self>> {
        let replay = match block {
            ContentBlock::Text(text) => Self::Text(text.to_owned()),
            ContentBlock::ReasoningContent(ReasoningContentBlock::ReasoningText(reasoning)) => {
                let Some(signature) = reasoning.signature() else {
                    return Ok(None);
                };
                Self::SignedReasoning {
                    text: reasoning.text().to_owned(),
                    signature: signature.to_owned(),
                }
            }
            ContentBlock::ReasoningContent(ReasoningContentBlock::RedactedContent(bytes)) => {
                Self::RedactedReasoning(Str::to_base64(bytes.as_ref()))
            }
            ContentBlock::ToolUse(tool) => Self::Tool(read_call(tool)?),
            _ => return Ok(None),
        };
        Ok(Some(replay))
    }

    fn into_aws(self) -> AiResult<ContentBlock> {
        Ok(match self {
            Self::Text(text) => ContentBlock::Text(text),
            Self::SignedReasoning { text, signature } => {
                ContentBlock::ReasoningContent(ReasoningContentBlock::ReasoningText(
                    ReasoningTextBlock::builder()
                        .text(text)
                        .signature(signature)
                        .build()
                        .map_err(|_| AiError::InvalidResponse)?,
                ))
            }
            Self::RedactedReasoning(content) => {
                ContentBlock::ReasoningContent(ReasoningContentBlock::RedactedContent(Blob::new(
                    Str::from_base64(&content).map_err(|_| AiError::InvalidResponse)?,
                )))
            }
            Self::Tool(call) => call_block(&call)?,
        })
    }
}

pub(super) fn read_output(blocks: &[ContentBlock]) -> AiResult<Vec<ReplayBlock>> {
    blocks
        .iter()
        .filter_map(|block| ReplayBlock::from_aws(block).transpose())
        .collect()
}

pub(super) fn output_state(
    blocks: &[ReplayBlock],
    capture_signed_text: bool,
) -> AiResult<(Vec<ToolCall>, Option<Value>)> {
    let calls: Vec<_> = blocks
        .iter()
        .filter_map(|block| match block {
            ReplayBlock::Tool(call) => Some(call.clone()),
            _ => None,
        })
        .collect();
    let signed = capture_signed_text
        && blocks.iter().any(|block| {
            matches!(
                block,
                ReplayBlock::SignedReasoning { .. } | ReplayBlock::RedactedReasoning(_)
            )
        });
    let replay = if calls.is_empty() && !signed {
        None
    } else {
        Some(
            json!({"bedrock_blocks":serde_json::to_value(blocks).map_err(|_| AiError::InvalidResponse)?}),
        )
    };
    Ok((calls, replay))
}

pub(super) fn messages(history: &[ToolStep]) -> AiResult<Vec<BedrockMessage>> {
    let mut messages = Vec::with_capacity(history.len() * 2);
    for step in history {
        let replay = step
            .provider_content
            .as_ref()
            .and_then(|content| content.get("bedrock_blocks"))
            .and_then(Value::as_array)
            .map(Vec::as_slice);
        messages.push(assistant(
            step.text.as_deref().unwrap_or_default(),
            &step.calls,
            replay,
        )?);
        messages.push(results(&step.results)?);
    }
    Ok(messages)
}

pub(super) fn assistant(
    text: &str,
    calls: &[ToolCall],
    replay: Option<&[Value]>,
) -> AiResult<BedrockMessage> {
    let content = if let Some(replay) = replay {
        replay
            .iter()
            .map(|block| {
                ReplayBlock::deserialize(block)
                    .map_err(|_| AiError::InvalidResponse)?
                    .into_aws()
            })
            .collect::<AiResult<Vec<_>>>()?
    } else {
        let mut content = Vec::with_capacity(calls.len() + 1);
        if !text.is_empty() {
            content.push(ContentBlock::Text(text.into()));
        }
        for call in calls {
            content.push(call_block(call)?);
        }
        content
    };
    BedrockMessage::builder()
        .role(ConversationRole::Assistant)
        .set_content(Some(content))
        .build()
        .map_err(|_| AiError::InvalidResponse)
}

pub(super) fn results(results: &[ToolResult]) -> AiResult<BedrockMessage> {
    let content = results
        .iter()
        .map(|result| {
            ToolResultBlock::builder()
                .tool_use_id(result.wire_id())
                .content(ToolResultContentBlock::Text(result.text().into()))
                .build()
                .map(ContentBlock::ToolResult)
                .map_err(|_| AiError::InvalidResponse)
        })
        .collect::<AiResult<Vec<_>>>()?;
    BedrockMessage::builder()
        .role(ConversationRole::User)
        .set_content(Some(content))
        .build()
        .map_err(|_| AiError::InvalidResponse)
}

pub(super) fn read_call(tool: &ToolUseBlock) -> AiResult<ToolCall> {
    let Value::Object(arguments) = json_value(tool.input())? else {
        return Err(AiError::InvalidToolArguments);
    };
    Ok(ToolCall {
        id: tool.tool_use_id().into(),
        name: tool.name().into(),
        arguments,
        result_id: None,
    })
}

pub(super) fn call_block(call: &ToolCall) -> AiResult<ContentBlock> {
    ToolUseBlock::builder()
        .tool_use_id(call.wire_id())
        .name(&call.name)
        .input(document(&Value::Object(call.arguments.clone()))?)
        .build()
        .map(ContentBlock::ToolUse)
        .map_err(|_| AiError::InvalidToolArguments)
}

fn document(value: &Value) -> AiResult<Document> {
    Ok(match value {
        Value::Null => Document::Null,
        Value::Bool(value) => Document::Bool(*value),
        Value::String(value) => Document::String(value.clone()),
        Value::Array(values) => {
            Document::Array(values.iter().map(document).collect::<AiResult<Vec<_>>>()?)
        }
        Value::Object(fields) => Document::Object(
            fields
                .iter()
                .map(|(key, value)| Ok((key.clone(), document(value)?)))
                .collect::<AiResult<HashMap<_, _>>>()?,
        ),
        Value::Number(number) => Document::Number(if let Some(n) = number.as_u64() {
            Number::PosInt(n)
        } else if let Some(n) = number.as_i64() {
            Number::NegInt(n)
        } else {
            Number::Float(number.as_f64().ok_or(AiError::InvalidToolArguments)?)
        }),
    })
}

fn json_value(value: &Document) -> AiResult<Value> {
    Ok(match value {
        Document::Null => Value::Null,
        Document::Bool(value) => Value::Bool(*value),
        Document::String(value) => Value::String(value.clone()),
        Document::Array(values) => Value::Array(
            values
                .iter()
                .map(json_value)
                .collect::<AiResult<Vec<_>>>()?,
        ),
        Document::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| Ok((key.clone(), json_value(value)?)))
                .collect::<AiResult<Map<_, _>>>()?,
        ),
        Document::Number(Number::PosInt(number)) => (*number).into(),
        Document::Number(Number::NegInt(number)) => (*number).into(),
        Document::Number(Number::Float(number)) => serde_json::Number::from_f64(*number)
            .map(Value::Number)
            .ok_or(AiError::InvalidToolArguments)?,
    })
}
