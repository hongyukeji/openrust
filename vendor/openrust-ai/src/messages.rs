use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AiError, AiResult, Attachment, ProviderOptions, StructuredOutput, TextGenerationOptions,
    TextPrompt, ToolCall, ToolChoice, ToolResult,
    attachment_storage::{self, StoredFiles},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    ToolResult,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::ToolResult => "tool_result",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    #[serde(skip)]
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResult>,
}

impl Message {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            attachments: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    pub fn user_with_attachments(
        content: impl Into<String>,
        attachments: impl IntoIterator<Item = impl Into<Attachment>>,
    ) -> Self {
        Self {
            attachments: attachments.into_iter().map(Into::into).collect(),
            ..Self::user(content)
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        calls: impl IntoIterator<Item = ToolCall>,
    ) -> Self {
        Self {
            tool_calls: calls.into_iter().collect(),
            ..Self::assistant(content)
        }
    }

    pub fn tool_results(results: impl IntoIterator<Item = ToolResult>) -> Self {
        Self {
            tool_results: results.into_iter().collect(),
            ..Self::new(MessageRole::ToolResult, "")
        }
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
    #[serde(deserialize_with = "deserialize_checkpoint_messages")]
    messages: Vec<Message>,
    #[serde(
        default,
        skip_serializing,
        deserialize_with = "deserialize_inline_attachments"
    )]
    pub(crate) attachments: Vec<Attachment>,
    schema: Option<(Value, bool)>,
}

impl From<AgentPrompt<'_>> for OwnedPrompt {
    fn from(value: AgentPrompt<'_>) -> Self {
        Self::from(&value)
    }
}

impl From<&AgentPrompt<'_>> for OwnedPrompt {
    fn from(value: &AgentPrompt<'_>) -> Self {
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
                .as_ref()
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

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct PromptAttachmentArchive {
    files: StoredFiles,
    current: usize,
    history: Vec<usize>,
}

impl PromptAttachmentArchive {
    pub(crate) async fn store(
        disk: &str,
        record: &str,
        prompt: &OwnedPrompt,
    ) -> AiResult<Option<Self>> {
        let input = prompt.as_prompt(0);
        crate::attachments::validate_prompt_attachments(input.attachments, input.messages)?;
        let attachments: Vec<_> = input
            .attachments
            .iter()
            .chain(
                input
                    .messages
                    .iter()
                    .flat_map(|message| &message.attachments),
            )
            .cloned()
            .collect();
        if attachments.is_empty() {
            return Ok(None);
        }
        let files = attachment_storage::write(disk, record, &attachments).await?;
        Ok(Some(Self {
            files,
            current: input.attachments.len(),
            history: input
                .messages
                .iter()
                .map(|message| message.attachments.len())
                .collect(),
        }))
    }

    pub(crate) async fn restore(&self, record: &str, prompt: &mut OwnedPrompt) -> AiResult<()> {
        let files = self.files.read(record).await?;
        let count = self
            .history
            .iter()
            .fold(self.current, |total, count| total.saturating_add(*count));
        if self.history.len() != prompt.messages.len() || count != files.len() {
            return Err(AiError::ConversationStorageUnavailable);
        }
        let mut files = files.into_iter();
        prompt.attachments = files.by_ref().take(self.current).collect();
        for (message, count) in prompt.messages.iter_mut().zip(&self.history) {
            message.attachments = files.by_ref().take(*count).collect();
        }
        Ok(())
    }

    pub(crate) async fn verify(&self, record: &str, prompt: &OwnedPrompt) -> AiResult<()> {
        let saved = self.files.read(record).await?;
        let expected = prompt.attachments.iter().chain(
            prompt
                .messages
                .iter()
                .flat_map(|message| &message.attachments),
        );
        if !saved.iter().eq(expected) {
            return Err(AiError::ConversationStorageUnavailable);
        }
        Ok(())
    }

    pub(crate) async fn discard(&self) {
        self.files.discard().await;
    }
}

fn deserialize_inline_attachments<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Attachment>, D::Error> {
    crate::attachments::restore_inline_attachments(Vec::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

fn deserialize_checkpoint_messages<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Message>, D::Error> {
    #[derive(Deserialize)]
    struct OldMessage {
        #[serde(flatten)]
        message: Message,
        #[serde(default, deserialize_with = "deserialize_inline_attachments")]
        attachments: Vec<Attachment>,
    }

    Ok(Vec::<OldMessage>::deserialize(deserializer)?
        .into_iter()
        .map(|message| Message {
            attachments: message.attachments,
            ..message.message
        })
        .collect())
}
