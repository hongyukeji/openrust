use std::{future::Future, sync::Arc};

use openrust::{
    contracts::events::Event,
    support::facades::{Event as Events, Log},
};

use crate::{
    AgentPrompt, AgentResponse, AiError, ConversationParticipant, PendingApproval, ToolResult,
    error::ProviderAttemptError, messages::OwnedPrompt,
};

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

pub(crate) async fn observe_prompt<'a, Fut>(
    agent: &'static str,
    provider: &str,
    request: AgentPrompt<'a>,
    send: impl FnOnce(AgentPrompt<'a>) -> Fut,
) -> Result<AgentResponse, ProviderAttemptError>
where
    Fut: Future<Output = Result<AgentResponse, ProviderAttemptError>>,
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
        Ok(response) => {
            dispatch_prompt_event(AgentPrompted {
                invocation_id,
                prompt,
                response: response.clone(),
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
