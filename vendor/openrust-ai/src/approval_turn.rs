use openrust::database::{DatabaseError, DatabaseResult, Transaction};
use serde::{Deserialize, Serialize};

use crate::{
    AgentResponse, AiError, AiResult, ConversationParticipant, PendingApproval, ToolResult,
    approval_store::{ApprovalScope, ApprovalStore},
    events::{ToolApprovalRequested, ToolApprovalResolved, dispatch_tool_approval},
    tools::ApprovalCheckpoint,
};

pub enum ApprovalResponse {
    Completed(Box<AgentResponse>),
    Pending(PendingToolApprovals),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingToolApprovals {
    pub id: String,
    pub approvals: Vec<PendingApproval>,
}

pub(crate) struct ApprovalSession {
    store: ApprovalStore,
    pub(crate) conversation_id: String,
    participant: ConversationParticipant,
    agent: String,
    pub(crate) existing: bool,
    claim: Option<ApprovalClaim>,
    resolved: Option<ToolApprovalResolved>,
}

struct ApprovalClaim {
    id: String,
    revision: i64,
    token: String,
}

impl ApprovalSession {
    pub(crate) fn new(store: ApprovalStore, scope: ApprovalScope<'_>, existing: bool) -> Self {
        ApprovalSession {
            store,
            conversation_id: scope.conversation_id.into(),
            participant: scope.participant.clone(),
            agent: scope.agent.into(),
            existing,
            claim: None,
            resolved: None,
        }
    }

    fn scope(&self) -> ApprovalScope<'_> {
        ApprovalScope {
            conversation_id: &self.conversation_id,
            participant: &self.participant,
            agent: &self.agent,
        }
    }

    pub(crate) async fn claim(&mut self, id: &str, revision: i64) -> AiResult<()> {
        let token = self
            .store
            .claim(&self.scope(), id, revision)
            .await
            .map_err(|_| AiError::ConversationStorageUnavailable)?
            .ok_or(AiError::ApprovalConsumed)?;
        self.claim = Some(ApprovalClaim {
            id: id.into(),
            revision,
            token,
        });
        Ok(())
    }

    pub(crate) async fn pause(
        &self,
        checkpoint: &ApprovalCheckpoint,
    ) -> AiResult<PendingToolApprovals> {
        let id = match &self.claim {
            Some(claim) => {
                if !self
                    .store
                    .settle(
                        &self.scope(),
                        &claim.id,
                        &claim.token,
                        Some((claim.revision, checkpoint)),
                    )
                    .await
                    .map_err(|_| AiError::ConversationStorageUnavailable)?
                {
                    return Err(AiError::ApprovalConsumed);
                }
                claim.id.clone()
            }
            None => self
                .store
                .insert(&self.scope(), checkpoint)
                .await
                .map_err(|_| AiError::ConversationStorageUnavailable)?,
        };
        let pending = PendingToolApprovals {
            id,
            approvals: checkpoint.pending.clone(),
        };
        dispatch_tool_approval(ToolApprovalRequested {
            invocation_id: checkpoint.invocation_id().into(),
            conversation_id: self.conversation_id.clone(),
            approval_id: pending.id.clone(),
            agent: self.agent.clone(),
            participant: self.participant.clone(),
            pending_approvals: pending.approvals.clone(),
        })
        .await;
        Ok(pending)
    }

    pub(crate) fn record_resolution(
        &mut self,
        invocation_id: &str,
        approval_id: &str,
        results: Vec<ToolResult>,
    ) {
        self.resolved = Some(ToolApprovalResolved {
            invocation_id: invocation_id.into(),
            conversation_id: self.conversation_id.clone(),
            approval_id: approval_id.into(),
            agent: self.agent.clone(),
            participant: self.participant.clone(),
            tool_results: results,
        });
    }

    pub(crate) async fn publish_resolution(self) {
        if let Some(event) = self.resolved {
            dispatch_tool_approval(event).await;
        }
    }

    pub(crate) async fn settle(&self) -> AiResult<()> {
        let Some(claim) = &self.claim else {
            return Ok(());
        };
        if self
            .store
            .settle::<ApprovalCheckpoint>(&self.scope(), &claim.id, &claim.token, None)
            .await
            .map_err(|_| AiError::ConversationStorageUnavailable)?
        {
            Ok(())
        } else {
            Err(AiError::ApprovalConsumed)
        }
    }

    pub(crate) async fn complete(&self, transaction: &mut Transaction) -> DatabaseResult<bool> {
        let Some(claim) = &self.claim else {
            return Err(DatabaseError::InvalidQuery("AI 工具审批尚未领取"));
        };
        self.store
            .complete(transaction, &self.scope(), &claim.id, &claim.token)
            .await
    }
}
