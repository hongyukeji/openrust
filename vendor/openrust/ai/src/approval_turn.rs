use openrust::database::{DatabaseError, DatabaseResult, Transaction};
use serde::{Deserialize, Serialize};

use crate::{
    AgentResponse, AiError, AiResult, ConversationParticipant, PendingApproval,
    approval_store::{ApprovalScope, ApprovalStore},
    tools::ApprovalCheckpoint,
};

pub enum ApprovalResponse {
    Completed(AgentResponse),
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
        Ok(PendingToolApprovals {
            id,
            approvals: checkpoint.pending.clone(),
        })
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
