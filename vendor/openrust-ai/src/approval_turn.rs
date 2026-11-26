use std::collections::BTreeMap;

use openrust::{
    database::{DatabaseError, DatabaseResult, Transaction},
    support::Str,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    AgentResponse, AiError, AiResult, ConversationParticipant, DatabaseConversationStore, Message,
    PendingApproval, ToolResult,
    approval_store::{ApprovalScope, ApprovalStore},
    conversation_store::{PausePersistence, TurnPersistence},
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
    database: DatabaseConversationStore,
    store: ApprovalStore,
    pub(crate) conversation_id: String,
    participant: ConversationParticipant,
    agent: String,
    pub(crate) existing: bool,
    pub(crate) history_message_id: Option<String>,
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
    pub(crate) fn new(
        database: DatabaseConversationStore,
        scope: ApprovalScope<'_>,
        existing: bool,
    ) -> Self {
        ApprovalSession {
            store: database.approval_store(),
            database,
            conversation_id: scope.conversation_id.into(),
            participant: scope.participant.clone(),
            agent: scope.agent.into(),
            existing,
            history_message_id: None,
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
        response: &AgentResponse,
    ) -> AiResult<PendingToolApprovals> {
        let id = self
            .claim
            .as_ref()
            .map_or_else(|| Str::ulid().to_string(), |claim| claim.id.clone());
        checkpoint.attachments = self.archive.clone();
        let user = Message::user_with_attachments(
            &checkpoint.prompt.text,
            checkpoint.prompt.attachments.iter().cloned(),
        );
        if !self
            .database
            .store_turn_with_approval(
                self.scope(),
                self.existing,
                &user,
                TurnPersistence {
                    response,
                    approval: Some(self),
                    title: None,
                    failure: None,
                    pause: Some(PausePersistence {
                        id: &id,
                        checkpoint: &mut checkpoint,
                    }),
                },
            )
            .await
            .map_err(|_| AiError::ConversationStorageUnavailable)?
        {
            return Err(AiError::ApprovalConsumed);
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

    pub(crate) async fn persist_pause(
        &self,
        transaction: &mut Transaction,
        id: &str,
        checkpoint: &ApprovalCheckpoint,
    ) -> DatabaseResult<bool> {
        if let Some(claim) = &self.claim {
            self.store
                .replace_paused(
                    transaction,
                    &self.scope(),
                    id,
                    &claim.token,
                    claim.revision,
                    checkpoint,
                )
                .await
        } else {
            self.store
                .insert(transaction, &self.scope(), id, checkpoint)
                .await?;
            Ok(true)
        }
    }

    pub(crate) async fn store_approval_results(
        &mut self,
        results: &[ToolResult],
        edits: &BTreeMap<String, Map<String, Value>>,
    ) -> AiResult<()> {
        let claim = self.claim.as_ref().ok_or(AiError::ApprovalConsumed)?;
        let revision = self
            .store
            .store_approval_results(
                &self.scope(),
                &claim.id,
                Some(&claim.token),
                results,
                Some(edits),
            )
            .await?;
        self.claim.as_mut().expect("审批已领取").revision = revision;
        Ok(())
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

    pub(crate) async fn publish_resolution(&self) {
        if let Some(event) = &self.resolved {
            dispatch_tool_approval(event.clone()).await;
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
            .settle(&self.scope(), &claim.id, &claim.token)
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
