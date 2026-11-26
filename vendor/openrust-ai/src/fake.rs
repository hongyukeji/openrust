use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use openrust::http::HttpFuture;
use serde_json::{Map, Value};

use crate::{
    AgentPrompt, AgentResponse, AiError, AiResult, Attachment, FinishReason, Message,
    ProviderOptions, Step, StepEventStream, StepResponse, StepResult, StepTextGateway,
    StreamableAgentResponse, TextGenerationOptions, TextPrompt, TextProvider, ToolCall, ToolChoice,
    ToolDefinition, ToolStep, Usage,
};

mod schema;

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
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordedQueuedPrompt {
    pub text: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub timeout: Option<Duration>,
    pub attachments: Vec<Attachment>,
    pub broadcast: bool,
}

pub enum FakeTextResult {
    Text(String),
    Structured(Map<String, Value>),
    Response(Box<AgentResponse>),
    ToolCall(ToolCall),
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

impl From<ToolCall> for FakeTextResult {
    fn from(value: ToolCall) -> Self {
        Self::ToolCall(value)
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
    queued: Mutex<Vec<RecordedQueuedPrompt>>,
    prevent_stray: AtomicBool,
}

impl FakeTextGateway {
    pub(super) fn new(responses: impl IntoIterator<Item = FakeTextResult>) -> Self {
        Self::from_source(FakeTextSource::Sequence(Mutex::new(
            responses.into_iter().collect(),
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
            queued: Mutex::default(),
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

    pub fn recorded_queued(&self) -> Vec<RecordedQueuedPrompt> {
        self.queued.lock().unwrap().clone()
    }

    pub(crate) fn record_queued(&self, prompt: RecordedQueuedPrompt) {
        self.queued.lock().unwrap().push(prompt);
    }

    #[track_caller]
    pub fn assert_queued(&self, predicate: impl Fn(&RecordedQueuedPrompt) -> bool) {
        assert!(
            self.recorded_queued().iter().any(predicate),
            "未发现匹配的排队 AI 提示"
        );
    }

    #[track_caller]
    pub fn assert_not_queued(&self, predicate: impl Fn(&RecordedQueuedPrompt) -> bool) {
        assert!(
            !self.recorded_queued().iter().any(predicate),
            "发现了不应排队的 AI 提示"
        );
    }

    #[track_caller]
    pub fn assert_never_queued(&self) {
        assert!(self.queued.lock().unwrap().is_empty(), "发现了排队 AI 提示");
    }

    fn respond(&self, prompt: RecordedPrompt) -> AiResult<AgentResponse> {
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
            Some(FakeTextResult::ToolCall(call)) => {
                let mut response = AgentResponse::new("", Usage::default(), provider, model);
                response.tool_calls.push(call);
                response
            }
            None if self.prevent_stray.load(Ordering::Relaxed) => {
                return Err(AiError::FakeExhausted);
            }
            None => {
                let text = if let Some(schema) = &prompt.schema {
                    schema::sample(schema)?.to_string()
                } else {
                    let preview = prompt
                        .text
                        .split_whitespace()
                        .take(10)
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("Fake response for prompt: {preview}")
                };
                AgentResponse::new(text, Usage::default(), provider, model)
            }
        };
        Ok(response)
    }
}

pub(crate) struct FakeTextProvider {
    gateway: Arc<FakeTextGateway>,
    provider: Option<String>,
    model: Option<String>,
}

impl FakeTextProvider {
    pub(crate) fn new(
        gateway: Arc<FakeTextGateway>,
        provider: Option<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            gateway,
            provider,
            model,
        }
    }

    pub(crate) fn model(&self) -> &str {
        self.model.as_deref().unwrap_or("fake")
    }

    pub(crate) fn record_prompt(&self, prompt: &AgentPrompt<'_>) {
        self.gateway
            .prompts
            .lock()
            .unwrap()
            .push(self.recorded(prompt, &[]));
    }

    fn recorded(&self, prompt: &AgentPrompt<'_>, history: &[ToolStep]) -> RecordedPrompt {
        RecordedPrompt {
            text: prompt.text.prompt.into(),
            options: prompt.options,
            provider_options: prompt.provider_options.cloned().unwrap_or_default(),
            tool_choice: prompt.tool_choice.cloned(),
            messages: prompt.messages.to_vec(),
            attachments: prompt.attachments.to_vec(),
            schema: prompt.structured_output.map(|output| output.schema.clone()),
            provider: self.provider.clone(),
            model: (self.model.is_some() || prompt.text.model != self.model())
                .then(|| prompt.text.model.to_owned()),
            steps: history
                .iter()
                .map(|step| step.public_step(self.name()))
                .collect(),
        }
    }

    fn respond_to_step(
        &self,
        prompt: &AgentPrompt<'_>,
        history: &[ToolStep],
    ) -> AiResult<StepResponse> {
        let response = self.gateway.respond(self.recorded(prompt, history))?;
        let reason = response.steps.last().map_or_else(
            || {
                if response.tool_calls.is_empty() {
                    FinishReason::Stop
                } else {
                    FinishReason::ToolCalls
                }
            },
            |step| step.finish_reason,
        );
        Ok(StepResponse {
            model: Some(response.model),
            text: Some(response.text),
            finish_reason: reason,
            reasoning: response.reasoning,
            usage: response.usage,
            citations: response.citations,
            tool_calls: response.tool_calls,
            provider_content: None,
        })
    }
}

impl TextProvider for FakeTextProvider {
    fn name(&self) -> &str {
        self.provider.as_deref().unwrap_or("fake")
    }

    fn default_text_model(&self) -> AiResult<&str> {
        Ok(self.model())
    }

    fn generate_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>> {
        self.generate(prompt.into())
    }

    fn generate<'a>(&'a self, prompt: AgentPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async move { self.gateway.respond(self.recorded(&prompt, &[])) })
    }

    fn stream_text<'a>(
        &'a self,
        prompt: TextPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        self.stream(prompt.into())
    }

    fn stream<'a>(
        &'a self,
        prompt: AgentPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        Box::pin(async move {
            self.gateway
                .respond(self.recorded(&prompt, &[]))
                .map(StreamableAgentResponse::from_text)
        })
    }

    fn step_text_gateway(&self) -> Option<&dyn StepTextGateway> {
        Some(self)
    }
}

impl StepTextGateway for FakeTextProvider {
    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        _: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        Box::pin(async move { self.respond_to_step(prompt, history) })
    }

    fn stream_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        _: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepEventStream>> {
        Box::pin(async move {
            self.respond_to_step(prompt, history)
                .map(|response| StepResult::from_response(response).into_stream())
        })
    }
}
