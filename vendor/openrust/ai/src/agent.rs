use std::{sync::Arc, time::Duration};

use openrust::{http::HttpFuture, json_schema::types::ObjectType};

use crate::{
    AgentResponse, Ai, AiResult, FakeTextGateway, Message, StreamableAgentResponse,
    StructuredAnonymousAgent,
};

#[derive(Clone, Copy, Default)]
pub struct PromptOptions<'a> {
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub timeout: Option<Duration>,
}

/// 应用可实现此契约，或用 `agent` 构造匿名 Agent。
pub trait Agent: Send + Sync + 'static {
    fn instructions(&self) -> &str;

    fn provider(&self) -> Option<&str> {
        None
    }

    fn model(&self) -> Option<&str> {
        None
    }

    fn messages(&self) -> &[Message] {
        &[]
    }

    fn fake(responses: impl IntoIterator<Item = String>) -> AiResult<Arc<FakeTextGateway>>
    where
        Self: Sized,
    {
        Ai::fake::<Self>(responses)
    }

    fn is_faked() -> AiResult<bool>
    where
        Self: Sized,
    {
        Ok(Ai::manager()?.text_fake::<Self>().is_some())
    }

    fn prompt<'a>(&'a self, prompt: &'a str) -> HttpFuture<'a, AiResult<AgentResponse>>
    where
        Self: Sized,
    {
        self.prompt_with(prompt, PromptOptions::default())
    }

    fn prompt_with<'a>(
        &'a self,
        prompt: &'a str,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<AgentResponse>>
    where
        Self: Sized,
    {
        Box::pin(async move { Ai::manager()?.prompt(self, prompt, options).await })
    }
    fn stream<'a>(&'a self, prompt: &'a str) -> HttpFuture<'a, AiResult<StreamableAgentResponse>>
    where
        Self: Sized,
    {
        self.stream_with(prompt, PromptOptions::default())
    }

    fn stream_with<'a>(
        &'a self,
        prompt: &'a str,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>>
    where
        Self: Sized,
    {
        Box::pin(async move { Ai::manager()?.stream(self, prompt, options).await })
    }
}

pub struct AnonymousAgent {
    instructions: String,
    messages: Vec<Message>,
}

pub fn agent(instructions: impl Into<String>) -> AnonymousAgent {
    AnonymousAgent {
        instructions: instructions.into(),
        messages: Vec::new(),
    }
}

impl AnonymousAgent {
    pub fn with_schema(self, schema: ObjectType) -> StructuredAnonymousAgent {
        StructuredAnonymousAgent::new(self, schema)
    }

    pub fn with_messages(mut self, messages: impl IntoIterator<Item = Message>) -> Self {
        self.messages = messages.into_iter().collect();
        self
    }
}

impl Agent for AnonymousAgent {
    fn instructions(&self) -> &str {
        &self.instructions
    }

    fn messages(&self) -> &[Message] {
        &self.messages
    }
}
