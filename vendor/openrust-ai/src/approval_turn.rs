use openrust::{
    database::{DatabaseError, DatabaseResult, Transaction},
    support::Str,
};
use serde::{Deserialize, Serialize};

use crate::{
    AgentResponse, AiError, AiResult, ConversationParticipant, PendingApproval, ToolResult,
    approval_store::{ApprovalScope, ApprovalStore},
    events::{ToolApprovalRequested, ToolApprovalResolved, dispatch_tool_approval},
    messages::PromptAttachmentArchive,
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
    archive: Option<PromptAttachmentArchive>,
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
            archive: None,
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

    pub(crate) fn set_archive(&mut self, archive: Option<PromptAttachmentArchive>) {
        self.archive = archive;
    }

    pub(crate) async fn pause(
        &self,
        mut checkpoint: ApprovalCheckpoint,
    ) -> AiResult<PendingToolApprovals> {
        let id = self
            .claim
            .as_ref()
            .map_or_else(|| Str::ulid().to_string(), |claim| claim.id.clone());
        checkpoint.attachments = match &self.archive {
            Some(archive) => Some(archive.clone()),
            None => {
                PromptAttachmentArchive::store(self.store.disk(), &id, &checkpoint.prompt).await?
            }
        };
        if let Some(claim) = &self.claim {
            if !self
                .store
                .settle(
                    &self.scope(),
                    &id,
                    &claim.token,
                    Some((claim.revision, &checkpoint)),
                )
                .await
                .map_err(|_| AiError::ConversationStorageUnavailable)?
            {
                if self.archive.is_none()
                    && let Some(archive) = &checkpoint.attachments
                {
                    archive.discard().await;
                }
                return Err(AiError::ApprovalConsumed);
            }
        } else {
            self.store
                .insert(&self.scope(), &id, &checkpoint)
                .await
                .map_err(|_| AiError::ConversationStorageUnavailable)?;
        }
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

    pub(crate) async fn discard_archived_attachments(&self) {
        if let Some(archive) = &self.archive {
            archive.discard().await;
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
            self.discard_archived_attachments().await;
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
