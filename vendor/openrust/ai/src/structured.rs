use std::{ops::Deref, sync::Arc};

use openrust::{
    http::HttpFuture,
    json_schema::{JsonSchemaTypeFactory, Type, types::ObjectType},
};
use serde::{Serialize, Serializer, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::{
    Agent, AgentMiddleware, AgentResponse, Ai, AiError, AiResult, AnonymousAgent, Message,
    PromptOptions, StreamableAgentResponse, Tool,
};

#[derive(Clone, Copy)]
pub struct StructuredOutput<'a> {
    pub schema: &'a Value,
    pub strict: bool,
}

/// 声明模型应返回的 JSON 对象结构。
pub trait HasStructuredOutput: Agent {
    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType;

    fn strict_schema(&self) -> bool {
        false
    }

    fn prompt_structured<'a>(
        &'a self,
        prompt: &'a str,
    ) -> HttpFuture<'a, AiResult<StructuredAgentResponse>>
    where
        Self: Sized,
    {
        self.prompt_structured_with(prompt, PromptOptions::default())
    }

    fn prompt_structured_with<'a>(
        &'a self,
        prompt: &'a str,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StructuredAgentResponse>>
    where
        Self: Sized,
    {
        Box::pin(async move {
            let schema = schema_for(self)?;
            let response = Ai::manager()?
                .prompt_with_schema(
                    self,
                    prompt,
                    options,
                    Some(StructuredOutput {
                        schema: &schema,
                        strict: self.strict_schema(),
                    }),
                )
                .await?;
            StructuredAgentResponse::parse(response)
        })
    }
    fn stream_structured<'a>(
        &'a self,
        prompt: &'a str,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>>
    where
        Self: Sized,
    {
        Box::pin(async move {
            let schema = schema_for(self)?;
            Ai::manager()?
                .stream_with_schema(
                    self,
                    prompt,
                    options,
                    Some(StructuredOutput {
                        schema: &schema,
                        strict: self.strict_schema(),
                    }),
                )
                .await
        })
    }
}

pub(crate) fn schema_for(agent: &impl HasStructuredOutput) -> AiResult<Value> {
    Type::from(agent.schema(JsonSchemaTypeFactory))
        .to_array()
        .map_err(|_| AiError::InvalidSchema)
}

pub struct StructuredAnonymousAgent {
    agent: AnonymousAgent,
    schema: ObjectType,
    strict: bool,
}

impl StructuredAnonymousAgent {
    pub(crate) fn new(agent: AnonymousAgent, schema: ObjectType) -> Self {
        Self {
            agent,
            schema,
            strict: false,
        }
    }

    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    pub fn stream<'a>(
        &'a self,
        prompt: &'a str,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        self.stream_structured(prompt, PromptOptions::default())
    }

    pub fn stream_with<'a>(
        &'a self,
        prompt: &'a str,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StreamableAgentResponse>> {
        self.stream_structured(prompt, options)
    }

    pub fn prompt<'a>(
        &'a self,
        prompt: &'a str,
    ) -> HttpFuture<'a, AiResult<StructuredAgentResponse>> {
        self.prompt_structured(prompt)
    }

    pub fn prompt_with<'a>(
        &'a self,
        prompt: &'a str,
        options: PromptOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StructuredAgentResponse>> {
        self.prompt_structured_with(prompt, options)
    }
}

impl Agent for StructuredAnonymousAgent {
    fn instructions(&self) -> &str {
        self.agent.instructions()
    }

    fn messages(&self) -> &[Message] {
        self.agent.messages()
    }
    fn attachments(&self) -> &[crate::ImageAttachment] {
        self.agent.attachments()
    }

    fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.agent.tools()
    }

    fn middleware(&self) -> Vec<Arc<dyn AgentMiddleware>> {
        self.agent.middleware()
    }

    fn max_steps(&self) -> Option<usize> {
        self.agent.max_steps()
    }
}

impl HasStructuredOutput for StructuredAnonymousAgent {
    fn schema(&self, _: JsonSchemaTypeFactory) -> ObjectType {
        self.schema.clone()
    }

    fn strict_schema(&self) -> bool {
        self.strict
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredAgentResponse {
    pub structured: Map<String, Value>,
    pub response: AgentResponse,
}

impl StructuredAgentResponse {
    pub(crate) fn parse(response: AgentResponse) -> AiResult<Self> {
        let structured =
            serde_json::from_str(&response.text).map_err(|_| AiError::InvalidStructuredResponse)?;
        Ok(Self {
            structured,
            response,
        })
    }

    pub fn decode<T: DeserializeOwned>(&self) -> AiResult<T> {
        serde_json::from_value(Value::Object(self.structured.clone()))
            .map_err(|_| AiError::InvalidStructuredResponse)
    }
}

impl Deref for StructuredAgentResponse {
    type Target = AgentResponse;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl Serialize for StructuredAgentResponse {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.structured.serialize(serializer)
    }
}
