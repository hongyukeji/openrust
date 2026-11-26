use std::{borrow::Cow, collections::BTreeSet, sync::Arc, time::Duration};

use openrust::{
    http::HttpFuture,
    pipeline::{Next, Pipeline},
};

use crate::{
    AgentPrompt, AiError, AiResult, Attachment, Message, ProviderOptions, StepResponse,
    StructuredOutput, TextGenerationOptions, TextPrompt, TextProvider, ToolChoice, ToolDefinition,
    ToolStep, Usage,
};

/// Agent 中间件可以读取、修改或拦截单个模型生成步骤。
pub trait AgentMiddleware: Send + Sync {
    fn handle<'a, 'step: 'a>(
        &'a self,
        step: PendingStep<'step>,
        next: NextStep<'a, 'step>,
    ) -> HttpFuture<'a, AiResult<StepResponse>>;
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

struct StepContext<'a> {
    provider: &'a dyn TextProvider,
    allowed: BTreeSet<String>,
}

pub(crate) async fn generate_step(
    provider: &dyn TextProvider,
    step: PendingStep<'_>,
    middleware: &[Arc<dyn AgentMiddleware>],
) -> AiResult<(StepResponse, BTreeSet<String>)> {
    let mut context = StepContext {
        provider,
        allowed: BTreeSet::new(),
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
                if step.model.trim().is_empty() {
                    return Err(AiError::MissingModel);
                }
                crate::attachments::validate_prompt_attachments(&step.attachments)?;
                step.options.validate()?;
                let choice = step.tool_choice.as_ref();
                if let Some(choice) = choice {
                    choice.validate(&step.tools)?;
                }
                context
                    .allowed
                    .extend(step.tools.iter().map(|tool| tool.name.clone()));
                let request = AgentPrompt {
                    text: TextPrompt {
                        instructions: &step.instructions,
                        prompt: step.prompt,
                        model: &step.model,
                        timeout: step.timeout,
                    },
                    options: step.options,
                    provider_options: step.provider_options.as_deref(),
                    tool_choice: choice,
                    messages: &step.messages,
                    attachments: &step.attachments,
                    structured_output: step.structured_output,
                };
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
        .await?;
    Ok((response, context.allowed))
}
