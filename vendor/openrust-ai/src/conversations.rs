use std::{any::type_name, sync::Arc};

use openrust::{
    contracts::auth::Authenticatable,
    support::{Str, facades::Log},
};

use crate::{
    Agent, AgentResponse, Ai, AiError, AiManager, AiResult, ApprovalResponse, Attachment,
    ConversationParticipant, ConversationStore, Decisions, HasStructuredOutput, Message,
    PendingToolApprovals, PromptOptions, StreamableAgentResponse, StructuredAgentResponse,
    StructuredOutput, TextProvider, ToolResult,
    approval_store::{ApprovalScope, SavedApproval},
    approval_turn::ApprovalSession,
    conversation_store::TurnPersistence,
    conversation_stream,
    error::ProviderAttemptError,
    structured::schema_for,
    tools::{ApprovalCheckpoint, BufferedOutcome},
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
    attachments: Vec<Attachment>,
    faked: bool,
}

struct ApprovalResume {
    turn: ConversationTurn,
    checkpoint: ApprovalCheckpoint,
    provider: Arc<dyn TextProvider>,
    session: ApprovalSession,
    revision: i64,
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
        self.persist_turn(prompt, response, approval, None).await
    }

    async fn persist_turn(
        &self,
        prompt: &str,
        response: &AgentResponse,
        approval: Option<&ApprovalSession>,
        failure: Option<&AiError>,
    ) -> AiResult<()> {
        let user = Message::user_with_attachments(prompt, self.attachments.iter().cloned());
        let stored = if let Some(database) = self.store.as_database() {
            let title = if self.existing {
                None
            } else {
                Some(
                    crate::conversation_title::title(
                        prompt,
                        &response.provider,
                        database.generates_titles() && failure.is_none(),
                        self.faked,
                    )
                    .await,
                )
            };
            database
                .store_turn_with_approval(
                    self.approval_scope(),
                    self.existing,
                    &user,
                    TurnPersistence {
                        response,
                        approval,
                        title: title.as_deref(),
                        failure,
                        pause: None,
                    },
                )
                .await
                .map_err(|_| AiError::ConversationStorageUnavailable)?
        } else {
            if approval.is_some() {
                return Err(AiError::ConversationStorageUnavailable);
            }
            if failure.is_some() {
                return Ok(());
            }
            self.store
                .store_turn_with_message(
                    &self.id,
                    &self.participant,
                    self.agent_name,
                    self.existing,
                    &user,
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

    async fn save_failed_steps(&self, prompt: &str, partial: &AgentResponse, error: &AiError) {
        if self
            .persist_turn(prompt, partial, None, Some(error))
            .await
            .is_err()
        {
            let _ = Log::warning("AI 会话的已完成工具步骤保存失败", &Default::default());
        }
    }

    async fn save_failed_approval(
        &self,
        prompt: &str,
        partial: &AgentResponse,
        session: &ApprovalSession,
        error: &AiError,
    ) {
        if self
            .persist_turn(prompt, partial, Some(session), Some(error))
            .await
            .is_ok()
        {
            session.publish_resolution().await;
        } else {
            let _ = Log::warning("AI 审批恢复后的已完成工具步骤保存失败", &Default::default());
        }
    }

    async fn buffered_outcome(
        &self,
        prompt: &str,
        outcome: Result<BufferedOutcome, ProviderAttemptError>,
    ) -> AiResult<BufferedOutcome> {
        match outcome {
            Err(ProviderAttemptError::CommittedWithSteps(error, partial)) => {
                self.save_failed_steps(prompt, &partial, &error).await;
                Err(error)
            }
            outcome => outcome.map_err(ProviderAttemptError::into_error),
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
        let database = self.store.as_database()?.clone();
        Some(ApprovalSession::new(
            database,
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
        let outcome = manager
            .prompt_in_session(self.agent, prompt, options, None, &history, None)
            .await;
        let outcome = turn.buffered_outcome(prompt, outcome).await?;
        if outcome.should_persist() {
            turn.persist(prompt, &outcome.response).await?;
        }
        Ok(turn.reply(outcome.response))
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
        let outcome = turn.buffered_outcome(prompt, outcome).await;
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

    /// 读取当前审批步骤已持久化、尚待写入会话历史的工具结果。
    pub async fn approval_results_for(&self, id: &str) -> AiResult<Option<Vec<ToolResult>>> {
        let (store, _) = Ai::manager()?.conversation_store()?;
        let scope = self.approval_scope()?;
        store
            .as_database()
            .ok_or(AiError::ApprovalNotFound)?
            .approval_store()
            .approval_results(&scope, id)
            .await
    }

    pub async fn resume_approval(
        self,
        id: &str,
        decisions: Decisions,
    ) -> AiResult<ConversationResponse<ApprovalResponse>> {
        let ApprovalResume {
            turn,
            checkpoint,
            provider,
            mut session,
            revision,
        } = self.prepare_approval_resume(id).await?;
        let prompt = checkpoint.prompt.text.clone();
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
        let outcome = match outcome {
            Err(ProviderAttemptError::CommittedWithSteps(error, partial)) => {
                turn.save_failed_approval(&prompt, &partial, &session, &error)
                    .await;
                return Err(error);
            }
            outcome => outcome.map_err(ProviderAttemptError::into_error),
        };
        if matches!(outcome, Err(ref error) if !matches!(error, AiError::ApprovalPaused(_))) {
            session.settle().await?;
        }
        let response = resolve_approval_turn(turn, &prompt, outcome, Some(&session)).await?;
        session.publish_resolution().await;
        Ok(response)
    }

    /// 从已保存的审批继续生成，通过流发送工具结果及模型增量。
    pub async fn resume_approval_stream(
        self,
        id: &str,
        decisions: Decisions,
    ) -> AiResult<ConversationResponse<StreamableAgentResponse>> {
        let ApprovalResume {
            turn,
            checkpoint,
            provider,
            mut session,
            revision,
        } = self.prepare_approval_resume(id).await?;
        let agent = type_name::<A>();
        let invocation = checkpoint.invocation_id().to_owned();
        let mut resolved = crate::request_context::in_agent_invocation(
            &invocation,
            0,
            Some(agent),
            checkpoint.resolve(
                crate::load_skill::tools_for(self.agent)?,
                &decisions,
                &mut session,
                (id, revision),
            ),
        )
        .await?;
        let request = resolved.prompt.as_prompt(0);
        let prefix = resolved
            .run
            .clone()
            .completed_response(provider.as_ref(), request.text.model)?;
        let structured = request.structured_output.is_some();
        let prompt = resolved.prompt.text.clone();
        if let Err(error) = std::mem::replace(&mut resolved.stored, Ok(())) {
            turn.save_failed_approval(&prompt, &prefix, &session, &error)
                .await;
            return Err(error);
        }
        let session = Arc::new(session);
        let response = crate::tool_stream::resume(
            provider,
            resolved,
            self.agent.middleware(),
            session.clone(),
            prefix,
            agent,
        );
        let conversation_id = turn.id.clone();
        let response = conversation_stream::remember(
            response,
            conversation_id.clone(),
            structured,
            move |response, error| async move {
                turn.persist_turn(&prompt, &response, Some(&session), error.as_ref())
                    .await?;
                session.publish_resolution().await;
                Ok(())
            },
        );
        Ok(ConversationResponse {
            conversation_id,
            response,
        })
    }

    async fn prepare_approval_resume(&self, id: &str) -> AiResult<ApprovalResume> {
        let manager = Ai::manager()?;
        let (store, _) = manager.conversation_store()?;
        let scope = self.approval_scope()?;
        let (mut checkpoint, revision) = load_approval(store.as_ref(), &scope, id).await?;
        if store
            .conversation_belongs_to(scope.conversation_id, scope.participant)
            .await?
            != (checkpoint.existing || checkpoint.history_message_id.is_some())
        {
            return Err(AiError::ApprovalNotFound);
        }
        let provider = manager.text_provider(Some(&checkpoint.provider))?;
        checkpoint.verify_connection(provider.as_ref())?;
        if let Some(message) = &checkpoint.history_message_id
            && !store
                .as_database()
                .ok_or(AiError::ApprovalNotFound)?
                .approval_store()
                .has_paused_history(&scope, message)
                .await?
        {
            load_approval(store.as_ref(), &scope, id).await?;
            return Err(AiError::ApprovalNotFound);
        }
        if let Some(archive) = &checkpoint.attachments
            && let Err(error) = archive.restore(id, &mut checkpoint.prompt).await
        {
            // 另一个请求可能已经处理并删除了同一审批的归档。
            load_approval(store.as_ref(), &scope, id).await?;
            return Err(error);
        }
        let turn = ConversationTurn {
            id: scope.conversation_id.to_owned(),
            existing: checkpoint.existing,
            participant: self.participant.clone(),
            agent_name: type_name::<A>(),
            attachments: checkpoint.prompt.attachments.clone(),
            faked: false,
            store,
        };
        let mut session = turn.approval_session().ok_or(AiError::ApprovalNotFound)?;
        session.set_archive(checkpoint.attachments.clone());
        session
            .history_message_id
            .clone_from(&checkpoint.history_message_id);
        Ok(ApprovalResume {
            turn,
            checkpoint,
            provider,
            session,
            revision,
        })
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
            move |response, error| async move {
                turn.persist_turn(&prompt, &response, None, error.as_ref())
                    .await
            },
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
        let attachments = self.agent.attachments();
        if prompt.trim().is_empty() && attachments.is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        let (store, history_limit) = manager.conversation_store()?;
        if !attachments.is_empty() && !store.supports_attachments() {
            return Err(AiError::ConversationStorageUnavailable);
        }
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
            attachments: attachments.to_vec(),
            faked: manager.text_fake::<A>().is_some(),
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
    outcome: AiResult<BufferedOutcome>,
    approval: Option<&ApprovalSession>,
) -> AiResult<ConversationResponse<ApprovalResponse>> {
    match outcome {
        Ok(outcome) => {
            if outcome.should_persist() {
                turn.persist_with_approval(prompt, &outcome.response, approval)
                    .await?;
            }
            Ok(turn.reply(ApprovalResponse::Completed(Box::new(outcome.response))))
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
        let outcome = manager
            .prompt_in_session(self.agent, prompt, options, Some(schema), &history, None)
            .await;
        let outcome = turn.buffered_outcome(prompt, outcome).await?;
        let should_persist = outcome.should_persist();
        let response = match StructuredAgentResponse::from_outcome(outcome) {
            Ok(response) => response,
            Err(response) => {
                if let Some(partial) = response.into_completed_tool_steps() {
                    turn.save_failed_steps(prompt, &partial, &AiError::InvalidStructuredResponse)
                        .await;
                }
                return Err(AiError::InvalidStructuredResponse);
            }
        };
        if should_persist {
            turn.persist(prompt, &response.response).await?;
        }
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
