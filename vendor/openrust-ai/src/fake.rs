use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use serde_json::{Map, Value};

use crate::{
    AgentResponse, AiError, AiResult, Attachment, Message, ProviderOptions, TextGenerationOptions,
    ToolChoice, Usage,
};

#[derive(Clone, Debug, PartialEq)]
pub struct RecordedPrompt {
    pub text: String,
    pub options: TextGenerationOptions,
    pub provider_options: ProviderOptions,
    pub tool_choice: Option<ToolChoice>,
    pub messages: Vec<Message>,
    pub attachments: Vec<Attachment>,
    pub schema: Option<Value>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

pub enum FakeTextResult {
    Text(String),
    Structured(Map<String, Value>),
    Response(Box<AgentResponse>),
}

impl From<String> for FakeTextResult {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for FakeTextResult {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}

impl From<Map<String, Value>> for FakeTextResult {
    fn from(value: Map<String, Value>) -> Self {
        Self::Structured(value)
    }
}

impl From<AgentResponse> for FakeTextResult {
    fn from(value: AgentResponse) -> Self {
        Self::Response(Box::new(value))
    }
}

type ResolvePrompt = dyn Fn(&RecordedPrompt) -> AiResult<FakeTextResult> + Send + Sync;

enum FakeTextSource {
    Sequence(Mutex<VecDeque<FakeTextResult>>),
    Callback(Box<ResolvePrompt>),
}

/// 应用内的文本替身及调用记录。
pub struct FakeTextGateway {
    source: FakeTextSource,
    prompts: Mutex<Vec<RecordedPrompt>>,
    prevent_stray: AtomicBool,
}

impl FakeTextGateway {
    pub(super) fn new(responses: impl IntoIterator<Item = String>) -> Self {
        Self::from_source(FakeTextSource::Sequence(Mutex::new(
            responses.into_iter().map(FakeTextResult::from).collect(),
        )))
    }

    pub(super) fn with(
        resolve: impl Fn(&RecordedPrompt) -> AiResult<FakeTextResult> + Send + Sync + 'static,
    ) -> Self {
        Self::from_source(FakeTextSource::Callback(Box::new(resolve)))
    }

    fn from_source(source: FakeTextSource) -> Self {
        Self {
            source,
            prompts: Mutex::default(),
            prevent_stray: AtomicBool::new(false),
        }
    }

    pub fn prevent_stray_prompts(&self, prevent: bool) -> &Self {
        self.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn recorded(&self) -> Vec<RecordedPrompt> {
        self.prompts.lock().unwrap().clone()
    }

    #[track_caller]
    pub fn assert_prompted(&self, predicate: impl Fn(&RecordedPrompt) -> bool) {
        assert!(
            self.recorded().iter().any(predicate),
            "未发现匹配的 AI 提示"
        );
    }

    #[track_caller]
    pub fn assert_not_prompted(&self, predicate: impl Fn(&RecordedPrompt) -> bool) {
        assert!(
            !self.recorded().iter().any(predicate),
            "发现了不应发送的 AI 提示"
        );
    }

    #[track_caller]
    pub fn assert_prompted_times(&self, count: usize) {
        assert_eq!(
            self.prompts.lock().unwrap().len(),
            count,
            "AI 提示次数不匹配"
        );
    }

    #[track_caller]
    pub fn assert_never_prompted(&self) {
        self.assert_prompted_times(0);
    }

    pub(super) fn respond(&self, prompt: RecordedPrompt) -> AiResult<AgentResponse> {
        let result = match &self.source {
            FakeTextSource::Sequence(responses) => responses.lock().unwrap().pop_front(),
            FakeTextSource::Callback(resolve) => Some(resolve(&prompt)?),
        };
        let provider = prompt.provider.as_deref().unwrap_or_default();
        let model = prompt.model.as_deref().unwrap_or_default();
        let response = match result {
            Some(FakeTextResult::Text(text)) => {
                AgentResponse::new(text, Usage::default(), provider, model)
            }
            Some(FakeTextResult::Structured(data)) => AgentResponse::new(
                Value::Object(data).to_string(),
                Usage::default(),
                provider,
                model,
            ),
            Some(FakeTextResult::Response(response)) => *response,
            None if self.prevent_stray.load(Ordering::Relaxed) => {
                return Err(AiError::FakeExhausted);
            }
            None => {
                let preview = prompt
                    .text
                    .split_whitespace()
                    .take(10)
                    .collect::<Vec<_>>()
                    .join(" ");
                AgentResponse::new(
                    format!("Fake response for prompt: {preview}"),
                    Usage::default(),
                    provider,
                    model,
                )
            }
        };
        self.prompts.lock().unwrap().push(prompt);
        Ok(response)
    }
}
