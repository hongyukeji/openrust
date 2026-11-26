use std::any::type_name;

use openrust::{contracts::auth::Authenticatable, support::Str};

use crate::{
    Agent, AgentResponse, Ai, AiError, AiManager, AiResult, ConversationParticipant,
    DatabaseConversationStore, HasStructuredOutput, Message, PromptOptions,
    StreamableAgentResponse, StructuredAgentResponse, StructuredOutput, conversation_stream,
    structured::schema_for,
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
    store: DatabaseConversationStore,
    id: String,
    existing: bool,
    participant: ConversationParticipant,
    agent_name: &'static str,
}

impl ConversationTurn {
    async fn persist(&self, prompt: &str, response: &AgentResponse) -> AiResult<()> {
        let stored = self
            .store
            .store_turn(
                &self.id,
                self.existing,
                &self.participant,
                self.agent_name,
                prompt,
                response,
            )
            .await
            .map_err(|_| AiError::ConversationStorageUnavailable)?;
        if stored {
            Ok(())
        } else {
            Err(AiError::ConversationNotFound)
        }
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
        self.stream_inner(prompt, options, None).await
    }

    async fn stream_inner(
        self,
        prompt: &str,
        options: PromptOptions<'_>,
        schema: Option<StructuredOutput<'_>>,
    ) -> AiResult<ConversationResponse<StreamableAgentResponse>> {
        let manager = Ai::manager()?;
        let (turn, history) = self.prepare(&manager, prompt).await?;
        let response = manager
            .stream_with_history(self.agent, prompt, options, schema, &history)
            .await?;
        let conversation_id = turn.id.clone();
        let prompt = prompt.to_owned();
        let response =
            conversation_stream::remember(response, schema.is_some(), move |response| async move {
                turn.persist(&prompt, &response).await
            });
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
            ConversationTarget::Latest => store
                .latest_conversation_id(&self.participant, agent_name)
                .await
                .map_err(|_| AiError::ConversationStorageUnavailable)?,
        };
        let history = match existing.as_deref() {
            Some(id) => store
                .get_latest_conversation_messages(id, &self.participant, history_limit)
                .await
                .map_err(|_| AiError::ConversationStorageUnavailable)?
                .ok_or(AiError::ConversationNotFound)?,
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
        self.stream_inner(prompt, options, Some(schema)).await
    }
}
