use std::{borrow::Cow, collections::BTreeSet, sync::Arc, time::Duration};

use futures_util::StreamExt;
use openrust::{
    http::HttpFuture,
    pipeline::{Next, Pipeline},
};

use crate::{
    AgentPrompt, AiError, AiResult, Attachment, Message, ProviderOptions, StepEvent,
    StepEventStream, StepResponse, StructuredOutput, TextGenerationOptions, TextPrompt,
    TextProvider, ToolChoice, ToolDefinition, ToolStep, Usage, events::StepObservation,
};

/// Agent 中间件可以读取、修改或拦截单个模型生成步骤。
pub trait AgentMiddleware: Send + Sync {
    fn handle<'a, 'step: 'a>(
        &'a self,
        step: PendingStep<'step>,
        next: NextStep<'a, 'step>,
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        next.run(step)
    }

    fn handle_stream<'a, 'step: 'a>(
        &'a self,
        _step: PendingStep<'step>,
        _next: NextStreamStep<'a, 'step>,
    ) -> HttpFuture<'a, AiResult<StepResult>> {
        Box::pin(async { Err(AiError::StreamingMiddlewareNotSupported) })
    }
}

type StepPredicate = dyn Fn(&PendingStep<'_>) -> bool + Send + Sync;

/// 用布尔值或待执行步骤的谓词声明停止条件。
pub struct StopCondition(Box<StepPredicate>);

impl From<bool> for StopCondition {
    fn from(condition: bool) -> Self {
        Self(Box::new(move |_| condition))
    }
}

impl StopCondition {
    fn matches(&self, step: &PendingStep<'_>) -> bool {
        (self.0)(step)
    }
}

impl<F> From<F> for StopCondition
where
    F: Fn(&PendingStep<'_>) -> bool + Send + Sync + 'static,
{
    fn from(condition: F) -> Self {
        Self(Box::new(condition))
    }
}

/// 在满足条件的步骤发送模型请求前停止生成。
pub struct Stop(StopCondition);

impl Stop {
    pub fn new(condition: impl Into<StopCondition>) -> Self {
        Self(condition.into())
    }

    pub fn when(condition: impl Into<StopCondition>) -> Self {
        Self::new(condition)
    }

    pub fn unless(condition: impl Into<StopCondition>) -> Self {
        let StopCondition(condition) = condition.into();
        Self::new(move |step: &PendingStep<'_>| !condition(step))
    }
}

impl AgentMiddleware for Stop {
    fn handle<'a, 'step: 'a>(
        &'a self,
        step: PendingStep<'step>,
        next: NextStep<'a, 'step>,
    ) -> HttpFuture<'a, AiResult<StepResponse>> {
        if self.0.matches(&step) {
            Box::pin(async { Ok(StepResponse::stop()) })
        } else {
            next.run(step)
        }
    }

    fn handle_stream<'a, 'step: 'a>(
        &'a self,
        step: PendingStep<'step>,
        next: NextStreamStep<'a, 'step>,
    ) -> HttpFuture<'a, AiResult<StepResult>> {
        if self.0.matches(&step) {
            Box::pin(async { Ok(StepResult::stop()) })
        } else {
            next.run(step)
        }
    }
}

pub struct PendingStep<'a> {
    pub number: usize,
    pub is_final_step: bool,
    pub provider: &'a str,
    pub model: Cow<'a, str>,
    pub instructions: Cow<'a, str>,
    pub prompt: &'a str,
    pub options: TextGenerationOptions,
    pub provider_options: Option<Cow<'a, ProviderOptions>>,
    pub tool_choice: Option<ToolChoice>,
    pub messages: Cow<'a, [Message]>,
    pub attachments: Cow<'a, [Attachment]>,
    pub structured_output: Option<StructuredOutput<'a>>,
    pub steps: &'a [ToolStep],
    pub usage: Usage,
    pub timeout: Option<Duration>,
    pub invocation_id: &'a str,
    pub(crate) tools: Cow<'a, [ToolDefinition]>,
}

impl<'a> PendingStep<'a> {
    pub fn is_first_step(&self) -> bool {
        self.number == 0
    }

    pub fn with_model(mut self, model: impl Into<Cow<'a, str>>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_instructions(mut self, instructions: impl Into<Cow<'a, str>>) -> Self {
        self.instructions = instructions.into();
        self
    }

    pub fn with_messages(mut self, messages: impl Into<Cow<'a, [Message]>>) -> Self {
        self.messages = messages.into();
        self
    }

    pub fn with_attachments(mut self, attachments: impl Into<Cow<'a, [Attachment]>>) -> Self {
        self.attachments = attachments.into();
        self
    }

    pub fn with_generation_options(mut self, options: TextGenerationOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(Cow::Owned(options));
        self
    }

    pub fn with_tool_choice(mut self, choice: Option<ToolChoice>) -> Self {
        self.tool_choice = choice;
        self
    }

    pub fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    pub fn only_tools(mut self, names: &[&str]) -> Self {
        self.tools
            .to_mut()
            .retain(|tool| names.contains(&tool.name.as_str()));
        self
    }

    pub fn without_tools(mut self, names: &[&str]) -> Self {
        self.tools
            .to_mut()
            .retain(|tool| !names.contains(&tool.name.as_str()));
        self
    }

    fn validate(&self) -> AiResult<()> {
        if self.model.trim().is_empty() {
            return Err(AiError::MissingModel);
        }
        crate::attachments::validate_prompt_attachments(&self.attachments, &self.messages)?;
        self.options.validate()?;
        if let Some(choice) = &self.tool_choice {
            choice.validate(&self.tools)?;
        }
        Ok(())
    }

    fn as_prompt(&self) -> AgentPrompt<'_> {
        AgentPrompt {
            text: TextPrompt {
                instructions: &self.instructions,
                prompt: self.prompt,
                model: &self.model,
                timeout: self.timeout,
            },
            options: self.options,
            provider_options: self.provider_options.as_deref(),
            tool_choice: self.tool_choice.as_ref(),
            messages: &self.messages,
            attachments: &self.attachments,
            structured_output: self.structured_output,
        }
    }
}

/// 继续执行剩余中间件及当前模型请求。
pub struct NextStep<'a, 'step: 'a>(
    Next<'a, PendingStep<'step>, StepResponse, AiError, StepContext<'step>>,
);

impl<'a, 'step: 'a> NextStep<'a, 'step> {
    pub fn run(self, step: PendingStep<'step>) -> HttpFuture<'a, AiResult<StepResponse>> {
        self.0.run(step)
    }
}

/// 流式步骤可继续读取供应商事件，也可由中间件直接交付结果。
pub enum StepResult {
    Stream(StepEventStream),
    Response(Box<StepResponse>),
    Resolved {
        events: Vec<StepEvent>,
        response: Box<StepResponse>,
    },
    Failed(AiError),
    Stop,
}

impl StepResult {
    pub fn from_response(response: StepResponse) -> Self {
        Self::Response(Box::new(response))
    }

    pub fn streamed(&self) -> bool {
        matches!(
            self,
            Self::Stream(_) | Self::Resolved { .. } | Self::Failed(_)
        )
    }

    /// 结束本轮生成。中间件须在调用 `next.run` 前返回此结果。
    pub fn stop() -> Self {
        Self::Stop
    }

    pub fn stopped(&self) -> bool {
        matches!(self, Self::Stop)
    }

    /// 按需消费并保留步骤事件，后续仍可通过正常响应重放这些事件。
    pub async fn response(&mut self) -> AiResult<Option<&StepResponse>> {
        let result = std::mem::replace(self, Self::Failed(AiError::InvalidResponse));
        *self = match result {
            Self::Stream(mut source) => {
                let mut events = Vec::new();
                loop {
                    let event = match source.next().await {
                        Some(Ok(event)) => event,
                        Some(Err(error)) => break Self::Failed(error),
                        None => break Self::Failed(AiError::InvalidResponse),
                    };
                    if let StepEvent::Finished { response, .. } = &event {
                        let response = response.clone();
                        events.push(event);
                        break Self::Resolved { events, response };
                    }
                    events.push(event);
                }
            }
            result => result,
        };
        match self {
            Self::Response(response) | Self::Resolved { response, .. } => Ok(Some(response)),
            Self::Failed(error) => Err(error.clone()),
            Self::Stop => Ok(None),
            Self::Stream(_) => Err(AiError::InvalidResponse),
        }
    }

    pub(crate) async fn probe(self) -> AiResult<Self> {
        let Self::Stream(mut source) = self else {
            return Ok(self);
        };
        match source.next().await.transpose()? {
            Some(event) => Ok(Self::Stream(Box::pin(
                futures_util::stream::iter([Ok(event)]).chain(source),
            ))),
            None => Ok(Self::Stream(source)),
        }
    }

    pub(crate) fn into_stream(self) -> StepEventStream {
        match self {
            Self::Stream(source) => source,
            Self::Resolved { events, .. } => {
                Box::pin(futures_util::stream::iter(events.into_iter().map(Ok)))
            }
            Self::Failed(error) => Box::pin(futures_util::stream::iter([Err(error)])),
            Self::Stop => Box::pin(futures_util::stream::empty()),
            Self::Response(response) => Box::pin(async_stream::stream! {
                if let Some(text) = &response.text
                    && !text.is_empty()
                {
                    yield Ok(StepEvent::TextDelta(text.clone()));
                }
                for citation in &response.citations {
                    yield Ok(StepEvent::Citation { part:None, citation:citation.clone() });
                }
                if !response.reasoning.is_empty() {
                    yield Ok(StepEvent::Reasoning(crate::ReasoningPart::End {
                        part:("middleware".into(), 0),
                        text:Some(response.reasoning.clone()),
                    }));
                }
                let reason = response.finish_reason.as_str().into();
                yield Ok(StepEvent::Finished { response, reason });
            }),
        }
    }

    pub fn then(self, callback: impl FnOnce(&StepResponse) + Send + 'static) -> Self {
        match self {
            Self::Response(response) => {
                callback(&response);
                Self::Response(response)
            }
            Self::Resolved { events, response } => {
                callback(&response);
                Self::Resolved { events, response }
            }
            Self::Stream(mut source) => {
                let app = openrust::support::facades::App::get_instance().ok();
                Self::Stream(Box::pin(async_stream::try_stream! {
                    let mut callback = Some(callback);
                    while let Some(event) = source.next().await {
                        let event = event?;
                        if let StepEvent::Finished { response, .. } = &event
                            && let Some(callback) = callback.take()
                        {
                            if let Some(app) = &app {
                                app.scope(async { callback(response) }).await;
                            } else {
                                callback(response);
                            }
                        }
                        yield event;
                    }
                }))
            }
            result => result,
        }
    }
}

pub struct NextStreamStep<'a, 'step: 'a>(
    Next<'a, PendingStep<'step>, StepResult, AiError, StreamStepContext<'step>>,
);

impl<'a, 'step: 'a> NextStreamStep<'a, 'step> {
    pub fn run(self, step: PendingStep<'step>) -> HttpFuture<'a, AiResult<StepResult>> {
        self.0.run(step)
    }
}

struct StreamStepContext<'a> {
    provider: &'a dyn TextProvider,
    model: String,
    allowed: BTreeSet<String>,
    choice: Option<ToolChoice>,
    attempted: bool,
}

pub(crate) struct PreparedStreamStep {
    pub(crate) result: StepResult,
    pub(crate) model: String,
    pub(crate) allowed: BTreeSet<String>,
    pub(crate) choice: Option<ToolChoice>,
}

pub(crate) async fn generate_stream_step(
    provider: &dyn TextProvider,
    step: PendingStep<'_>,
    middleware: &[Arc<dyn AgentMiddleware>],
) -> AiResult<PreparedStreamStep> {
    let mut context = StreamStepContext {
        provider,
        model: step.model.to_string(),
        allowed: step.tools.iter().map(|tool| tool.name.clone()).collect(),
        choice: step.tool_choice.clone(),
        attempted: false,
    };
    let mut pipeline = Pipeline::send(step);
    for middleware in middleware {
        let middleware = middleware.clone();
        pipeline = pipeline.pipe(move |step, next| {
            Box::pin(async move { middleware.handle_stream(step, NextStreamStep(next)).await })
        });
    }
    let result = pipeline
        .then_with(&mut context, |step, context| {
            Box::pin(async move {
                step.validate()?;
                context.model = step.model.to_string();
                context.allowed = step.tools.iter().map(|tool| tool.name.clone()).collect();
                context.choice = step.tool_choice.clone();
                context.attempted = true;
                crate::tool_stream::stream_step(
                    context.provider,
                    &step.as_prompt(),
                    &step.tools,
                    step.steps,
                    step.is_final_step,
                )
                .await
                .map(StepResult::Stream)
            })
        })
        .await?;
    if result.stopped() && context.attempted {
        return Err(AiError::InvalidResponse);
    }
    Ok(PreparedStreamStep {
        result,
        model: context.model,
        allowed: context.allowed,
        choice: context.choice,
    })
}

pub(crate) struct CompletedStep {
    pub(crate) response: StepResponse,
    pub(crate) allowed: BTreeSet<String>,
    pub(crate) requested_model: Option<String>,
}

struct StepContext<'a> {
    provider: &'a dyn TextProvider,
    requested_model: Option<String>,
    allowed: BTreeSet<String>,
    observation: Option<StepObservation>,
}

pub(crate) async fn generate_step(
    provider: &dyn TextProvider,
    step: PendingStep<'_>,
    middleware: &[Arc<dyn AgentMiddleware>],
) -> AiResult<CompletedStep> {
    let mut context = StepContext {
        provider,
        requested_model: None,
        allowed: BTreeSet::new(),
        observation: None,
    };
    let mut pipeline = Pipeline::send(step);
    for middleware in middleware {
        let middleware = middleware.clone();
        pipeline = pipeline.pipe(move |step, next| {
            Box::pin(async move { middleware.handle(step, NextStep(next)).await })
        });
    }
    let response = pipeline
        .then_with(&mut context, |step, context| {
            Box::pin(async move {
                step.validate()?;
                let choice = step.tool_choice.as_ref();
                context
                    .allowed
                    .extend(step.tools.iter().map(|tool| tool.name.clone()));
                let request = step.as_prompt();
                context.requested_model = Some(request.text.model.to_owned());
                context.observation = StepObservation::start(
                    context.provider.name(),
                    &request,
                    step.steps,
                    step.number,
                    step.is_final_step,
                )
                .await;
                if step.tools.is_empty() && step.steps.is_empty() {
                    let response = crate::request_context::in_invocation(
                        step.invocation_id,
                        step.number,
                        context.provider.generate(request),
                    )
                    .await?;
                    if let Some(choice) = choice {
                        choice.validate_calls(&response.tool_calls)?;
                    }
                    return Ok(StepResponse {
                        model: Some(response.model),
                        text: Some(response.text),
                        finish_reason: response
                            .steps
                            .last()
                            .map(|step| step.finish_reason)
                            .unwrap_or_default(),
                        reasoning: response.reasoning,
                        usage: response.usage,
                        citations: response.citations,
                        tool_calls: response.tool_calls,
                        provider_content: None,
                    });
                }
                let gateway = context
                    .provider
                    .step_text_gateway()
                    .ok_or(AiError::ToolsNotSupported)?;
                let mut response = crate::request_context::in_invocation(
                    step.invocation_id,
                    step.number,
                    gateway.generate_text_step(&request, &step.tools, step.steps),
                )
                .await?;
                if let Some(choice) = choice {
                    choice.validate_calls(&response.tool_calls)?;
                }
                if response.model.is_none() {
                    response.model = Some(step.model.into_owned());
                }
                Ok(response)
            })
        })
        .await
        .and_then(|response| {
            if response.stopped() && context.requested_model.is_some() {
                return Err(AiError::InvalidResponse);
            }
            Ok(response)
        });
    if let Some(observation) = context.observation {
        match &response {
            Ok(response) => {
                observation
                    .complete(|step| {
                        response
                            .public_step(&step.prompt.provider, step.prompt.as_prompt().text.model)
                    })
                    .await;
            }
            Err(error) => observation.fail(error).await,
        }
    }
    Ok(CompletedStep {
        response: response?,
        allowed: context.allowed,
        requested_model: context.requested_model,
    })
}
