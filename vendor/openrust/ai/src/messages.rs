use serde::{Deserialize, Serialize};

use crate::{Attachment, StructuredOutput, TextPrompt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }
}

/// 当前文本提示词及本次调用随附的历史消息。
pub struct AgentPrompt<'a> {
    pub text: TextPrompt<'a>,
    pub messages: &'a [Message],
    pub attachments: &'a [Attachment],
    pub structured_output: Option<StructuredOutput<'a>>,
}

impl<'a> From<TextPrompt<'a>> for AgentPrompt<'a> {
    fn from(text: TextPrompt<'a>) -> Self {
        Self {
            text,
            messages: &[],
            attachments: &[],
            structured_output: None,
        }
    }
}
