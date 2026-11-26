use std::any::type_name;

use openrust::contracts::auth::Authenticatable;

use crate::conversation_store::ConversationParticipant;
use crate::{Agent, AgentResponse, Ai, AiError, AiResult, PromptOptions};

/// 实现此契约即可按参与者保存和继续 Agent 对话。
pub trait RemembersConversations: Agent + Sized {
    fn for_participant(
        &self,
        participant: ConversationParticipant,
    ) -> PendingConversation<'_, Self> {
        self.continue_or_start(None, participant)
    }

    fn for_user(&self, user: &impl Authenticatable) -> PendingConversation<'_, Self> {
        self.for_participant(participant_for_user(user))
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

fn participant_for_user<U: Authenticatable>(user: &U) -> ConversationParticipant {
    ConversationParticipant::new(type_name::<U>(), user.get_auth_identifier())
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

pub struct ConversationResponse {
    pub conversation_id: String,
    pub response: AgentResponse,
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
        if prompt.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        let manager = Ai::manager()?;
        let (store, history_limit) = manager.conversation_store()?;
        self.participant
            .validate()
            .map_err(|_| AiError::InvalidConversationParticipant)?;
        let agent_name = type_name::<A>();
        let existing = match self.target {
            ConversationTarget::New => None,
            ConversationTarget::Existing(id) => Some(id),
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
        let response = manager
            .prompt_with_history(self.agent, prompt, options, None, &history)
            .await?;
        let conversation_id = store
            .store_turn(
                existing.as_deref(),
                &self.participant,
                agent_name,
                prompt,
                &response,
            )
            .await
            .map_err(|_| AiError::ConversationStorageUnavailable)?
            .ok_or(AiError::ConversationNotFound)?;
        Ok(ConversationResponse {
            conversation_id,
            response,
        })
    }
}
