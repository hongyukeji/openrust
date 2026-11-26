use std::{future::Future, sync::Arc};

use openrust::{
    contracts::events::Event,
    support::facades::{Event as Events, Log},
};

use crate::{
    AgentPrompt, AgentResponse, AiError, ConversationParticipant, PendingApproval,
    StreamedAgentResponse, ToolResult, error::ProviderAttemptError, messages::OwnedPrompt,
    tools::BufferedOutcome,
};

mod step;
mod tool;

pub(crate) use step::StepObservation;
pub use step::{ModelStep, StartingStep, StepCompleted, StepFailed};
pub(crate) use tool::observe_tool;
pub use tool::{InvokingTool, ToolFailed, ToolInvocation, ToolInvoked};

/// 事件监听器可读取本次尝试的 Agent、连接和提示参数。
pub struct AgentPromptSnapshot {
    pub agent: &'static str,
    pub provider: String,
    prompt: OwnedPrompt,
}

impl AgentPromptSnapshot {
    pub fn as_prompt(&self) -> AgentPrompt<'_> {
        self.prompt.as_prompt(0)
    }
}

pub struct PromptingAgent {
    pub invocation_id: String,
    pub prompt: Arc<AgentPromptSnapshot>,
}

impl PromptingAgent {
    pub const NAME: &str = "openrust_ai::events::PromptingAgent";
}

impl Event for PromptingAgent {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct AgentPrompted {
    pub invocation_id: String,
    pub prompt: Arc<AgentPromptSnapshot>,
    pub response: AgentResponse,
}

impl AgentPrompted {
    pub const NAME: &str = "openrust_ai::events::AgentPrompted";
}

impl Event for AgentPrompted {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct AgentFailed {
    pub invocation_id: String,
    pub prompt: Arc<AgentPromptSnapshot>,
    pub error: AiError,
}

impl AgentFailed {
    pub const NAME: &str = "openrust_ai::events::AgentFailed";
}

impl Event for AgentFailed {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct StreamingAgent {
    pub invocation_id: String,
    pub prompt: Arc<AgentPromptSnapshot>,
}

impl StreamingAgent {
    pub const NAME: &str = "openrust_ai::events::StreamingAgent";
}

impl Event for StreamingAgent {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct AgentStreamed {
    pub invocation_id: String,
    pub prompt: Arc<AgentPromptSnapshot>,
    pub response: Arc<StreamedAgentResponse>,
}

impl AgentStreamed {
    pub const NAME: &str = "openrust_ai::events::AgentStreamed";
}

impl Event for AgentStreamed {
    fn name(&self) -> &str {
        Self::NAME
    }
}

#[derive(Clone)]
pub(crate) struct StreamAttempt {
    invocation_id: String,
    prompt: Arc<AgentPromptSnapshot>,
}

impl StreamAttempt {
    pub(crate) async fn complete(self, response: Arc<StreamedAgentResponse>) {
        dispatch_prompt_event(AgentStreamed {
            invocation_id: self.invocation_id,
            prompt: self.prompt,
            response,
        })
        .await;
    }

    pub(crate) async fn fail(self, error: &AiError) {
        dispatch_prompt_event(AgentFailed {
            invocation_id: self.invocation_id,
            prompt: self.prompt,
            error: error.clone(),
        })
        .await;
    }
}

pub(crate) async fn observe_stream_start<'a, T, Fut>(
    agent: &'static str,
    provider: &str,
    request: AgentPrompt<'a>,
    send: impl FnOnce(AgentPrompt<'a>) -> Fut,
) -> Result<(T, Option<StreamAttempt>), ProviderAttemptError>
where
    Fut: Future<Output = Result<T, ProviderAttemptError>>,
{
    if !Events::dispatcher().is_ok_and(|dispatcher| {
        [StreamingAgent::NAME, AgentStreamed::NAME, AgentFailed::NAME]
            .into_iter()
            .any(|name| dispatcher.observes(name))
    }) {
        return send(request).await.map(|response| (response, None));
    }
    let Some(invocation_id) = crate::request_context::invocation_id() else {
        return send(request).await.map(|response| (response, None));
    };
    let prompt = Arc::new(AgentPromptSnapshot {
        agent,
        provider: provider.into(),
        prompt: OwnedPrompt::from(&request),
    });
    let attempt = StreamAttempt {
        invocation_id,
        prompt,
    };
    dispatch_prompt_event(StreamingAgent {
        invocation_id: attempt.invocation_id.clone(),
        prompt: attempt.prompt.clone(),
    })
    .await;
    match send(request).await {
        Ok(response) => Ok((response, Some(attempt))),
        Err(error) => {
            attempt.fail(error.error()).await;
            Err(error)
        }
    }
}

pub(crate) async fn observe_prompt<'a, Fut>(
    agent: &'static str,
    provider: &str,
    request: AgentPrompt<'a>,
    send: impl FnOnce(AgentPrompt<'a>) -> Fut,
) -> Result<BufferedOutcome, ProviderAttemptError>
where
    Fut: Future<Output = Result<BufferedOutcome, ProviderAttemptError>>,
{
    let Some(invocation_id) = crate::request_context::invocation_id() else {
        return send(request).await;
    };
    let prompt = Arc::new(AgentPromptSnapshot {
        agent,
        provider: provider.into(),
        prompt: OwnedPrompt::from(&request),
    });
    dispatch_prompt_event(PromptingAgent {
        invocation_id: invocation_id.clone(),
        prompt: prompt.clone(),
    })
    .await;
    let result = send(request).await;
    match &result {
        Ok(outcome) => {
            dispatch_prompt_event(AgentPrompted {
                invocation_id,
                prompt,
                response: outcome.response.clone(),
            })
            .await;
        }
        Err(error) => {
            dispatch_prompt_event(AgentFailed {
                invocation_id,
                prompt,
                error: error.error().clone(),
            })
            .await;
        }
    }
    result
}

async fn dispatch_prompt_event(event: impl Event) {
    if Events::dispatch(event).await.is_err() {
        let _ = Log::warning("AI 提示事件监听器失败", &Default::default());
    }
}

pub struct AgentFailedOver {
    pub invocation_id: String,
    pub agent: &'static str,
    pub provider: String,
    pub model: String,
    pub error: AiError,
}

impl AgentFailedOver {
    pub const NAME: &str = "openrust_ai::events::AgentFailedOver";
}

impl Event for AgentFailedOver {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub(crate) async fn dispatch_agent_failover(event: AgentFailedOver) {
    if Events::dispatch(event).await.is_err() {
        let _ = Log::warning("AI 供应商故障转移事件监听器失败", &Default::default());
    }
}

pub struct ToolApprovalRequested {
    pub invocation_id: String,
    pub conversation_id: String,
    pub approval_id: String,
    pub agent: String,
    pub participant: ConversationParticipant,
    pub pending_approvals: Vec<PendingApproval>,
}

impl ToolApprovalRequested {
    pub const NAME: &str = "openrust_ai::events::ToolApprovalRequested";
}

impl Event for ToolApprovalRequested {
    fn name(&self) -> &str {
        Self::NAME
    }
}

#[derive(Clone)]
pub struct ToolApprovalResolved {
    pub invocation_id: String,
    pub conversation_id: String,
    pub approval_id: String,
    pub agent: String,
    pub participant: ConversationParticipant,
    pub tool_results: Vec<ToolResult>,
}

impl ToolApprovalResolved {
    pub const NAME: &str = "openrust_ai::events::ToolApprovalResolved";
}

impl Event for ToolApprovalResolved {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub(crate) async fn dispatch_tool_approval(event: impl Event) {
    if Events::dispatch(event).await.is_err() {
        let _ = Log::warning("AI 工具审批事件监听器失败", &Default::default());
    }
}
