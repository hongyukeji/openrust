use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Attachment, ProviderOptions, StructuredOutput, TextGenerationOptions, TextPrompt, ToolChoice,
};

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
    pub options: TextGenerationOptions,
    pub provider_options: Option<&'a ProviderOptions>,
    pub tool_choice: Option<&'a ToolChoice>,
    pub messages: &'a [Message],
    pub attachments: &'a [Attachment],
    pub structured_output: Option<StructuredOutput<'a>>,
}

impl<'a> From<TextPrompt<'a>> for AgentPrompt<'a> {
    fn from(text: TextPrompt<'a>) -> Self {
        Self {
            text,
            options: TextGenerationOptions::default(),
            provider_options: None,
            tool_choice: None,
            messages: &[],
            attachments: &[],
            structured_output: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct OwnedPrompt {
    instructions: String,
    pub(crate) text: String,
    model: String,
    timeout: Option<Duration>,
    options: crate::TextGenerationOptions,
    provider_options: Option<crate::ProviderOptions>,
    tool_choice: Option<crate::ToolChoice>,
    messages: Vec<Message>,
    #[serde(skip)]
    attachments: Vec<crate::Attachment>,
    schema: Option<(Value, bool)>,
}

impl From<AgentPrompt<'_>> for OwnedPrompt {
    fn from(value: AgentPrompt<'_>) -> Self {
        Self {
            instructions: value.text.instructions.into(),
            text: value.text.prompt.into(),
            model: value.text.model.into(),
            timeout: value.text.timeout,
            options: value.options,
            provider_options: value.provider_options.cloned(),
            tool_choice: value.tool_choice.cloned(),
            messages: value.messages.to_vec(),
            attachments: value.attachments.to_vec(),
            schema: value
                .structured_output
                .map(|output| (output.schema.clone(), output.strict)),
        }
    }
}

impl OwnedPrompt {
    pub(crate) fn choice_for(&self, step: usize) -> Option<&crate::ToolChoice> {
        self.tool_choice
            .as_ref()
            .and_then(|choice| choice.for_step(step))
    }

    pub(crate) fn as_prompt(&self, step: usize) -> AgentPrompt<'_> {
        AgentPrompt {
            text: TextPrompt {
                instructions: &self.instructions,
                prompt: &self.text,
                model: &self.model,
                timeout: self.timeout,
            },
            options: self.options,
            provider_options: self.provider_options.as_ref(),
            tool_choice: self.choice_for(step),
            messages: &self.messages,
            attachments: &self.attachments,
            structured_output: self
                .schema
                .as_ref()
                .map(|(schema, strict)| StructuredOutput {
                    schema,
                    strict: *strict,
                }),
        }
    }
}
