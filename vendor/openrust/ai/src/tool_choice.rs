use serde::{Deserialize, Serialize};

use crate::{AiError, AiResult, ToolCall, ToolDefinition};

/// 控制模型能否调用工具，或要求第一步调用一个工具。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool(String),
}

impl ToolChoice {
    pub fn tool(name: impl Into<String>) -> Self {
        Self::Tool(name.into())
    }

    pub(crate) fn for_step(&self, step: usize) -> Option<&Self> {
        if step == 0 || matches!(self, Self::Auto | Self::None) {
            Some(self)
        } else {
            None
        }
    }

    pub(crate) fn validate(&self, tools: &[ToolDefinition]) -> AiResult<()> {
        match self {
            Self::Required if tools.is_empty() => Err(AiError::InvalidToolChoice),
            Self::Tool(name) if !tools.iter().any(|tool| &tool.name == name) => {
                Err(AiError::InvalidToolChoice)
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn validate_calls(&self, calls: &[ToolCall]) -> AiResult<()> {
        let valid = match self {
            Self::Auto => true,
            Self::None => calls.is_empty(),
            Self::Required => !calls.is_empty(),
            Self::Tool(name) => !calls.is_empty() && calls.iter().all(|call| &call.name == name),
        };
        if valid {
            Ok(())
        } else {
            Err(AiError::InvalidResponse)
        }
    }
}
