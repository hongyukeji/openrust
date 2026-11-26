use std::{any::type_name, sync::Arc};

use openrust::{contracts::auth::Authenticatable, support::Str};

use crate::{
    Agent, AgentResponse, Ai, AiError, AiManager, AiResult, ApprovalResponse,
    ConversationParticipant, ConversationStore, Decisions, HasStructuredOutput, Message,
    PendingToolApprovals, PromptOptions, StreamableAgentResponse, StructuredAgentResponse,
    StructuredOutput,
    approval_store::{ApprovalScope, SavedApproval},
    approval_turn::ApprovalSession,
    conversation_stream,
    structured::schema_for,
    tools::ApprovalCheckpoint,
};

/// 实现此契约即可按参与者保存和继续 Agent 对话。
pub trait RemembersConversations: Agent + Sized {
    fn for_participant(
        &self,
        participant: ConversationParticipant,
    ) -> PendingConversation<'_, Self> {
        self.continue_or_start(None, participant)
    }

    fn for_user(&self, user: &impl Authenticatable) -> PendingConversation<'_, Self> {
        self.for_participant(ConversationParticipant::for_user(user))
    }

    fn r#continue(
        &self,
        conversation_id: impl Into<String>,
        participant: ConversationParticipant,
    ) -> PendingConversation<'_, Self> {
        self.continue_or_start(Some(conversation_id.into()), participant)
    }

    fn continue_or_start(
        &self,
        conversation_id: Option<String>,
        participant: ConversationParticipant,
    ) -> PendingConversation<'_, Self> {
        PendingConversation {
            agent: self,
            participant,
            target: conversation_id.map_or(ConversationTarget::New, ConversationTarget::Existing),
        }
    }

    fn continue_last_conversation(
        &self,
        participant: ConversationParticipant,
    ) -> PendingConversation<'_, Self> {
        PendingConversation {
            agent: self,
            participant,
            target: ConversationTarget::Latest,
        }
    }
}

impl ConversationParticipant {
    pub fn for_user<U: Authenticatable>(user: &U) -> Self {
        Self::new(type_name::<U>(), user.get_auth_identifier())
    }
}

enum ConversationTarget {
    New,
    Existing(String),
    Latest,
}

pub struct PendingConversation<'a, A> {
    agent: &'a A,
    participant: ConversationParticipant,
    target: ConversationTarget,
}

pub struct ConversationResponse<R = AgentResponse> {
    pub conversation_id: String,
    pub response: R,
}

struct ConversationTurn {
    store: Arc<dyn ConversationStore>,
    id: String,
    existing: bool,
    participant: ConversationParticipant,
    agent_name: &'static str,
}

impl ConversationTurn {
    async fn persist(&self, prompt: &str, response: &AgentResponse) -> AiResult<()> {
        self.persist_with_approval(prompt, response, None).await
    }

    async fn persist_with_approval(
        &self,
        prompt: &str,
        response: &AgentResponse,
        approval: Option<&ApprovalSession>,
    ) -> AiResult<()> {
        let stored = if let Some(approval) = approval {
            self.store
                .as_database()
                .ok_or(AiError::ConversationStorageUnavailable)?
                .store_turn(
                    self.approval_scope(),
                    self.existing,
                    prompt,
                    response,
                    Some(approval),
                )
                .await
                .map_err(|_| AiError::ConversationStorageUnavailable)?
        } else {
            self.store
                .store_turn(
                    &self.id,
                    &self.participant,
                    self.agent_name,
                    self.existing,
                    prompt,
                    response,
                )
                .await?
        };
        if stored {
            Ok(())
        } else {
            Err(AiError::ConversationNotFound)
        }
    }

    fn approval_scope(&self) -> ApprovalScope<'_> {
        ApprovalScope {
            conversation_id: &self.id,
            participant: &self.participant,
            agent: self.agent_name,
        }
    }

    fn approval_session(&self) -> Option<ApprovalSession> {
        let store = self.store.as_database()?.approval_store();
        Some(ApprovalSession::new(
            store,
            self.approval_scope(),
            self.existing,
        ))
    }

    fn reply<R>(self, response: R) -> ConversationResponse<R> {
        ConversationResponse {
            conversation_id: self.id,
            response,
        }
    }
}

impl<A: Agent> PendingConversation<'_, A> {
    pub fn current_conversation(&self) -> Option<&str> {
        match &self.target {
            ConversationTarget::Existing(id) => Some(id),
            _ => None,
        }
    }

    pub fn has_conversation_participant(&self) -> bool {
        true
    }

    pub fn conversation_participant(&self) -> &ConversationParticipant {
        &self.participant
    }

    pub async fn prompt(self, prompt: &str) -> AiResult<ConversationResponse> {
        self.prompt_with(prompt, PromptOptions::default()).await
    }

    pub async fn prompt_with(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<ConversationResponse> {
        let manager = Ai::manager()?;
        let (turn, history) = self.prepare(&manager, prompt).await?;
        let response = manager
            .prompt_with_history(self.agent, prompt, options, None, &history)
            .await?;
        turn.persist(prompt, &response).await?;
        Ok(turn.reply(response))
    }

    pub async fn prompt_with_approvals(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<ConversationResponse<ApprovalResponse>> {
        let manager = Ai::manager()?;
        let (turn, history) = self.prepare(&manager, prompt).await?;
        let session = turn.approval_session();
        let outcome = manager
            .prompt_in_session(
                self.agent,
                prompt,
                options,
                None,
                &history,
                session.as_ref(),
            )
            .await;
        resolve_approval_turn(turn, prompt, outcome, None).await
    }

    pub async fn pending_approvals_for(&self, id: &str) -> AiResult<PendingToolApprovals> {
        let manager = Ai::manager()?;
        let (store, _) = manager.conversation_store()?;
        let scope = self.approval_scope()?;
        let (checkpoint, _) = load_approval(store.as_ref(), &scope, id).await?;
        Ok(PendingToolApprovals {
            id: id.to_owned(),
            approvals: checkpoint.pending,
        })
    }

    pub async fn resume_approval(
        self,
        id: &str,
        decisions: Decisions,
    ) -> AiResult<ConversationResponse<ApprovalResponse>> {
        let manager = Ai::manager()?;
        let (store, _) = manager.conversation_store()?;
        let scope = self.approval_scope()?;
        let (checkpoint, revision) = load_approval(store.as_ref(), &scope, id).await?;
        if store
            .conversation_belongs_to(scope.conversation_id, scope.participant)
            .await?
            != checkpoint.existing
        {
            return Err(AiError::ApprovalNotFound);
        }
        let provider = manager.text_provider(Some(&checkpoint.provider))?;
        let prompt = checkpoint.prompt.text.clone();
        let turn = ConversationTurn {
            id: scope.conversation_id.to_owned(),
            existing: checkpoint.existing,
            participant: self.participant.clone(),
            agent_name: type_name::<A>(),
            store,
        };
        let mut session = turn.approval_session().ok_or(AiError::ApprovalNotFound)?;
        let invocation_id = checkpoint.invocation_id().to_owned();
        let outcome = crate::request_context::in_agent_invocation(
            &invocation_id,
            0,
            Some(type_name::<A>()),
            checkpoint.resume(
                provider.as_ref(),
                crate::load_skill::tools_for(self.agent)?,
                self.agent.middleware(),
                &decisions,
                &mut session,
                (id, revision),
            ),
        )
        .await;
        if matches!(outcome, Err(ref error) if !matches!(error, AiError::ApprovalPaused(_))) {
            session.settle().await?;
        }
        let response = resolve_approval_turn(turn, &prompt, outcome, Some(&session)).await?;
        session.publish_resolution().await;
        Ok(response)
    }

    fn approval_scope(&self) -> AiResult<ApprovalScope<'_>> {
        let conversation_id = self
            .current_conversation()
            .ok_or(AiError::ApprovalNotFound)?;
        self.participant
            .validate()
            .map_err(|_| AiError::InvalidConversationParticipant)?;
        Ok(ApprovalScope {
            conversation_id,
            participant: &self.participant,
            agent: type_name::<A>(),
        })
    }

    pub async fn stream(
        self,
        prompt: &str,
    ) -> AiResult<ConversationResponse<StreamableAgentResponse>> {
        self.stream_with(prompt, PromptOptions::default()).await
    }

    pub async fn stream_with(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<ConversationResponse<StreamableAgentResponse>> {
        self.stream_inner(prompt, options, None, false).await
    }

    pub async fn stream_with_approvals(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<ConversationResponse<StreamableAgentResponse>> {
        self.stream_inner(prompt, options, None, true).await
    }

    async fn stream_inner(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
        schema: Option<StructuredOutput<'_>>,
        with_approvals: bool,
    ) -> AiResult<ConversationResponse<StreamableAgentResponse>> {
        let manager = Ai::manager()?;
        let (turn, history) = self.prepare(&manager, prompt).await?;
        let response = manager
            .stream_in_session(
                self.agent,
                prompt,
                options,
                schema,
                &history,
                with_approvals.then(|| turn.approval_session()).flatten(),
            )
            .await?;
        let conversation_id = turn.id.clone();
        let prompt = prompt.to_owned();
        let response = conversation_stream::remember(
            response,
            conversation_id.clone(),
            schema.is_some(),
            move |response| async move { turn.persist(&prompt, &response).await },
        );
        Ok(ConversationResponse {
            conversation_id,
            response,
        })
    }

    async fn prepare(
        &self,
        manager: &AiManager,
        prompt: &str,
    ) -> AiResult<(ConversationTurn, Vec<Message>)> {
        if prompt.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        let (store, history_limit) = manager.conversation_store()?;
        self.participant
            .validate()
            .map_err(|_| AiError::InvalidConversationParticipant)?;
        let agent_name = type_name::<A>();
        let existing = match &self.target {
            ConversationTarget::New => None,
            ConversationTarget::Existing(id) => Some(id.clone()),
            ConversationTarget::Latest => {
                store
                    .latest_conversation_id(&self.participant, agent_name)
                    .await?
            }
        };
        let history = match existing.as_deref() {
            Some(id) => {
                if store.as_database().is_none()
                    && !store.conversation_belongs_to(id, &self.participant).await?
                {
                    return Err(AiError::ConversationNotFound);
                }
                store
                    .get_latest_conversation_messages(id, &self.participant, history_limit)
                    .await?
                    .ok_or(AiError::ConversationNotFound)?
            }
            None => self.agent.messages().to_vec(),
        };
        let turn = ConversationTurn {
            store,
            existing: existing.is_some(),
            id: existing.unwrap_or_else(|| Str::uuid7().to_string()),
            participant: self.participant.clone(),
            agent_name,
        };
        Ok((turn, history))
    }
}

async fn load_approval(
    store: &dyn ConversationStore,
    scope: &ApprovalScope<'_>,
    id: &str,
) -> AiResult<(ApprovalCheckpoint, i64)> {
    match store
        .as_database()
        .ok_or(AiError::ApprovalNotFound)?
        .approval_store()
        .load(scope, id)
        .await
        .map_err(|_| AiError::ConversationStorageUnavailable)?
    {
        Some(SavedApproval::Pending { snapshot, revision }) => Ok((snapshot, revision)),
        Some(SavedApproval::Expired) => Err(AiError::ApprovalExpired),
        Some(SavedApproval::Consumed) => Err(AiError::ApprovalConsumed),
        None => Err(AiError::ApprovalNotFound),
    }
}

async fn resolve_approval_turn(
    turn: ConversationTurn,
    prompt: &str,
    outcome: AiResult<AgentResponse>,
    approval: Option<&ApprovalSession>,
) -> AiResult<ConversationResponse<ApprovalResponse>> {
    match outcome {
        Ok(response) => {
            turn.persist_with_approval(prompt, &response, approval)
                .await?;
            Ok(turn.reply(ApprovalResponse::Completed(Box::new(response))))
        }
        Err(AiError::ApprovalPaused(pending)) => Ok(turn.reply(ApprovalResponse::Pending(pending))),
        Err(error) => Err(error),
    }
}

impl<A: HasStructuredOutput> PendingConversation<'_, A> {
    pub async fn prompt_structured(
        self,
        prompt: &str,
    ) -> AiResult<ConversationResponse<StructuredAgentResponse>> {
        self.prompt_structured_with(prompt, PromptOptions::default())
            .await
    }

    pub async fn prompt_structured_with(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<ConversationResponse<StructuredAgentResponse>> {
        let schema = schema_for(self.agent)?;
        let schema = StructuredOutput {
            schema: &schema,
            strict: self.agent.strict_schema(),
        };
        let manager = Ai::manager()?;
        let (turn, history) = self.prepare(&manager, prompt).await?;
        let response = manager
            .prompt_with_history(self.agent, prompt, options, Some(schema), &history)
            .await?;
        let response = StructuredAgentResponse::parse(response)?;
        turn.persist(prompt, &response.response).await?;
        Ok(turn.reply(response))
    }

    pub async fn stream_structured(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<ConversationResponse<StreamableAgentResponse>> {
        let schema = schema_for(self.agent)?;
        let schema = StructuredOutput {
            schema: &schema,
            strict: self.agent.strict_schema(),
        };
        self.stream_inner(prompt, options, Some(schema), false)
            .await
    }
}
