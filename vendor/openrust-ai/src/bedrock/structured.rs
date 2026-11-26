use aws_sdk_bedrockruntime::types::ToolConfiguration;
use serde_json::{Map, Value};

use crate::{
    AgentPrompt, AiError, AiResult, FinishReason, StepResponse, ToolChoice, ToolDefinition,
};

use super::tools;

const SCHEMA_TOOL: &str = "structured_output";
const UNFORCEABLE: &[&str] = &[
    "claude-sonnet-5-5",
    "claude-opus-5-5",
    "claude-fable-5-1",
    "claude-mythos-5-1",
];

#[derive(Clone, Copy)]
pub(super) enum OutputMode {
    Text,
    Tool { forced: bool },
    JsonInstruction,
}

impl OutputMode {
    pub(super) fn buffers_text(self) -> bool {
        matches!(self, Self::Tool { .. })
    }

    pub(super) fn finish(self, mut response: StepResponse) -> AiResult<StepResponse> {
        if let Self::Tool { forced } = self {
            let mut schema_calls = response
                .tool_calls
                .iter()
                .filter(|call| call.name == SCHEMA_TOOL);
            if let Some(call) = schema_calls.next() {
                if response.tool_calls.len() != 1 {
                    return Err(AiError::InvalidStructuredResponse);
                }
                response.text = Some(Value::Object(call.arguments.clone()).to_string());
                response.tool_calls.clear();
                response.provider_content = None;
                response.finish_reason = FinishReason::Stop;
            } else if forced && !response.tool_calls.is_empty() {
                return Err(AiError::InvalidStructuredResponse);
            }
        }
        if !matches!(self, Self::Text) && response.tool_calls.is_empty() {
            let text = response
                .text
                .as_deref()
                .ok_or(AiError::InvalidStructuredResponse)?;
            serde_json::from_str::<Map<String, Value>>(text)
                .map_err(|_| AiError::InvalidStructuredResponse)?;
        }
        Ok(response)
    }
}

pub(super) struct PreparedOutput {
    pub mode: OutputMode,
    pub instruction: Option<String>,
    pub tool_config: Option<ToolConfiguration>,
}

pub(super) fn prepare(
    prompt: &AgentPrompt<'_>,
    definitions: &[ToolDefinition],
    final_step: bool,
) -> AiResult<PreparedOutput> {
    let choice = prompt.tool_choice;
    let Some(schema) = prompt.structured_output else {
        return Ok(PreparedOutput {
            mode: OutputMode::Text,
            instruction: None,
            tool_config: tools::configuration(definitions, choice)?,
        });
    };
    if definitions.iter().any(|tool| tool.name == SCHEMA_TOOL) {
        return Err(AiError::InvalidTools);
    }
    let schema = schema.object_schema();
    if UNFORCEABLE
        .iter()
        .any(|model| prompt.text.model.contains(model))
    {
        return Ok(PreparedOutput {
            mode: OutputMode::JsonInstruction,
            instruction: Some(format!(
                "JSON schema:\n{schema}\n\nYou must answer with only a JSON object that matches the JSON schema above. Do not wrap it in markdown fences or include any other text."
            )),
            tool_config: tools::configuration(definitions, choice)?,
        });
    }
    let forced = final_step || definitions.is_empty() || choice == Some(&ToolChoice::None);
    let explicit_local = matches!(choice, Some(ToolChoice::Required | ToolChoice::Tool(_)));
    if forced && explicit_local {
        return Err(AiError::InvalidToolChoice);
    }
    if explicit_local {
        return Ok(PreparedOutput {
            mode: OutputMode::Tool { forced: false },
            instruction: None,
            tool_config: tools::configuration(definitions, choice)?,
        });
    }
    let mut schema_tools = Vec::with_capacity(definitions.len() + 1);
    schema_tools.push(ToolDefinition {
        name: SCHEMA_TOOL.into(),
        description:
            "Return the response as a structured JSON object matching the provided schema.".into(),
        schema,
    });
    if choice != Some(&ToolChoice::None) {
        schema_tools.extend_from_slice(definitions);
    }
    let choice = forced.then(|| ToolChoice::tool(SCHEMA_TOOL));
    Ok(PreparedOutput {
        mode: OutputMode::Tool { forced },
        instruction: None,
        tool_config: tools::configuration(&schema_tools, choice.as_ref())?,
    })
}
