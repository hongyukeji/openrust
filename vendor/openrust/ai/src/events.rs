use openrust::{
    contracts::events::Event,
    support::facades::{Event as Events, Log},
};

use crate::{AiError, ConversationParticipant, PendingApproval, ToolResult};

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
