use std::{collections::BTreeMap, sync::Arc, time::Duration};

use openrust::{broadcasting::BroadcastChannel, http::HttpFuture, json_schema::types::ObjectType};

use crate::{
    AgentMiddleware, AgentResponse, Ai, AiResult, Attachment, FakeTextGateway, Message,
    ProviderOptions, QueueableAgent, QueuedAgentResponse, StreamableAgentResponse,
    StructuredAnonymousAgent, TextGenerationOptions, TextModelTier, Tool, ToolChoice,
    agent_broadcast::StreamBroadcast,
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

    fn model_tier(&self) -> TextModelTier {
        TextModelTier::Default
    }

    fn generation_options(&self) -> TextGenerationOptions {
        TextGenerationOptions::default()
    }

    fn provider_options(&self, _provider: &str) -> ProviderOptions {
        ProviderOptions::new()
    }

    fn messages(&self) -> &[Message] {
        &[]
    }

    fn attachments(&self) -> &[Attachment] {
        &[]
    }

    fn tool_choice(&self) -> Option<ToolChoice> {
        None
    }

    fn tools(&self) -> Vec<Arc<dyn Tool>> {
        Vec::new()
    }

    fn middleware(&self) -> Vec<Arc<dyn AgentMiddleware>> {
        Vec::new()
    }

    fn max_steps(&self) -> Option<usize> {
        None
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
    fn queue(&self, prompt: impl Into<String>) -> QueuedAgentResponse<'_, Self>
    where
        Self: QueueableAgent,
    {
        self.queue_with(prompt, PromptOptions::default())
    }

    fn queue_with(
        &self,
        prompt: impl Into<String>,
        options: PromptOptions<'_>,
    ) -> QueuedAgentResponse<'_, Self>
    where
        Self: QueueableAgent,
    {
        QueuedAgentResponse::new(self, prompt, options)
    }

    fn without_broadcasting(&self) -> &'static [&'static str] {
        &[]
    }

    fn broadcast<'a>(
        &'a self,
        prompt: &'a str,
        channels: impl IntoIterator<Item = BroadcastChannel>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>>
    where
        Self: Sized,
    {
        self.broadcast_with(prompt, channels, PromptOptions::default(), false)
    }

    fn broadcast_now<'a>(
        &'a self,
        prompt: &'a str,
        channels: impl IntoIterator<Item = BroadcastChannel>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>>
    where
        Self: Sized,
    {
        self.broadcast_with(prompt, channels, PromptOptions::default(), true)
    }

    fn broadcast_with<'a>(
        &'a self,
        prompt: &'a str,
        channels: impl IntoIterator<Item = BroadcastChannel>,
        options: PromptOptions<'a>,
        immediate: bool,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>>
    where
        Self: Sized,
    {
        let broadcast = StreamBroadcast {
            channels: channels.into_iter().collect(),
            excluded: self.without_broadcasting(),
            immediate,
        };
        Box::pin(async move {
            broadcast.validate()?;
            self.stream_with(prompt, options)
                .await?
                .broadcasting(broadcast)
        })
    }

    fn broadcast_on_queue(
        &self,
        prompt: impl Into<String>,
        channels: impl IntoIterator<Item = BroadcastChannel>,
    ) -> QueuedAgentResponse<'_, Self>
    where
        Self: QueueableAgent,
    {
        self.broadcast_on_queue_with(prompt, channels, PromptOptions::default())
    }

    fn broadcast_on_queue_with(
        &self,
        prompt: impl Into<String>,
        channels: impl IntoIterator<Item = BroadcastChannel>,
        options: PromptOptions<'_>,
    ) -> QueuedAgentResponse<'_, Self>
    where
        Self: QueueableAgent,
    {
        self.queue_with(prompt, options)
            .broadcasting(channels.into_iter().collect())
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
    model_tier: TextModelTier,
    options: TextGenerationOptions,
    provider_options: BTreeMap<String, ProviderOptions>,
    messages: Vec<Message>,
    attachments: Vec<Attachment>,
    tools: Vec<Arc<dyn Tool>>,
    tool_choice: Option<ToolChoice>,
    middleware: Vec<Arc<dyn AgentMiddleware>>,
    max_steps: Option<usize>,
}

pub fn agent(instructions: impl Into<String>) -> AnonymousAgent {
    AnonymousAgent {
        instructions: instructions.into(),
        model_tier: TextModelTier::Default,
        options: TextGenerationOptions::default(),
        provider_options: BTreeMap::new(),
        messages: Vec::new(),
        attachments: Vec::new(),
        tools: Vec::new(),
        tool_choice: None,
        middleware: Vec::new(),
        max_steps: None,
    }
}

impl AnonymousAgent {
    pub fn with_model_tier(mut self, tier: TextModelTier) -> Self {
        self.model_tier = tier;
        self
    }

    pub fn with_generation_options(mut self, options: TextGenerationOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    pub fn with_provider_options(
        mut self,
        provider: impl Into<String>,
        options: ProviderOptions,
    ) -> Self {
        self.provider_options.insert(provider.into(), options);
        self
    }

    pub fn with_tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_middleware(mut self, middleware: Vec<Arc<dyn AgentMiddleware>>) -> Self {
        self.middleware = middleware;
        self
    }

    pub fn with_max_steps(mut self, steps: usize) -> Self {
        self.max_steps = Some(steps);
        self
    }

    pub fn with_schema(self, schema: ObjectType) -> StructuredAnonymousAgent {
        StructuredAnonymousAgent::new(self, schema)
    }

    pub fn with_attachments(
        mut self,
        attachments: impl IntoIterator<Item = impl Into<Attachment>>,
    ) -> Self {
        self.attachments = attachments.into_iter().map(Into::into).collect();
        self
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

    fn model_tier(&self) -> TextModelTier {
        self.model_tier
    }

    fn generation_options(&self) -> TextGenerationOptions {
        self.options
    }

    fn provider_options(&self, provider: &str) -> ProviderOptions {
        self.provider_options
            .get(provider)
            .cloned()
            .unwrap_or_default()
    }

    fn messages(&self) -> &[Message] {
        &self.messages
    }
    fn attachments(&self) -> &[Attachment] {
        &self.attachments
    }

    fn tool_choice(&self) -> Option<ToolChoice> {
        self.tool_choice.clone()
    }

    fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.clone()
    }

    fn middleware(&self) -> Vec<Arc<dyn AgentMiddleware>> {
        self.middleware.clone()
    }

    fn max_steps(&self) -> Option<usize> {
        self.max_steps
    }
}
