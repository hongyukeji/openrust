use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

use crate::{Meta, ToolCall, ToolResult, UrlCitation, Usage};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Continue,
    Length,
    ContentFilter,
    Error,
    Cancelled,
    #[default]
    Unknown,
}

impl FinishReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::ToolCalls => "tool_calls",
            Self::Continue => "continue",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn from_wire(reason: Option<&str>) -> Self {
        match reason {
            Some("stop" | "end_turn" | "stop_sequence" | "STOP" | "completed") => Self::Stop,
            Some("tool_calls" | "function_call" | "tool_use") => Self::ToolCalls,
            Some("continue" | "pause_turn") => Self::Continue,
            Some("length" | "max_tokens" | "MAX_TOKENS" | "model_context_window_exceeded") => {
                Self::Length
            }
            Some("content_filter" | "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII") => {
                Self::ContentFilter
            }
            Some("error" | "failed") => Self::Error,
            Some("cancelled" | "canceled") => Self::Cancelled,
            _ => Self::Unknown,
        }
    }
}

/// 单次模型生成与该步骤的工具结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub finish_reason: FinishReason,
    pub usage: Usage,
    pub provider: String,
    pub model: String,
    pub citations: Vec<UrlCitation>,
    pub reasoning: String,
    pub(crate) provider_replay: Option<crate::provider_replay::ProviderReplay>,
}

impl Step {
    pub fn meta(&self) -> Meta<'_> {
        Meta {
            provider: &self.provider,
            model: &self.model,
            citations: &self.citations,
        }
    }
}

impl Serialize for Step {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_struct("Step", 7)?;
        fields.serialize_field("text", &self.text)?;
        fields.serialize_field("tool_calls", &self.tool_calls)?;
        fields.serialize_field("tool_results", &self.tool_results)?;
        fields.serialize_field("finish_reason", &self.finish_reason)?;
        fields.serialize_field("usage", &self.usage)?;
        fields.serialize_field("meta", &self.meta())?;
        fields.serialize_field("reasoning", &self.reasoning)?;
        fields.end()
    }
}
