use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use openrust::{
    http::HttpFuture,
    json_schema::{JsonSchemaTypeFactory, Type, types::ObjectType},
    support::{Str, facades::Validator},
    validation::{Data, RuleSet, ValidatedInput, ValidationResult},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::{AgentPrompt, AgentResponse, AiError, AiResult, TextProvider, Usage};

/// 由应用声明并执行的 Agent 工具。
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType;
    fn handle<'a>(&'a self, request: ToolRequest<'a>) -> HttpFuture<'a, AiResult<String>>;
}

pub struct ToolRequest<'a> {
    call: &'a ToolCall,
    invocation_id: &'a str,
}

impl ToolRequest<'_> {
    pub fn arguments(&self) -> &Map<String, Value> {
        &self.call.arguments
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.arguments().get(key)
    }

    pub fn decode<T: DeserializeOwned>(&self) -> AiResult<T> {
        serde_json::from_value(Value::Object(self.arguments().clone()))
            .map_err(|_| AiError::InvalidToolArguments)
    }

    pub async fn validate(&self, rules: RuleSet) -> ValidationResult<ValidatedInput> {
        Validator::validate(Data::new(Value::Object(self.arguments().clone()))?, rules).await
    }

    pub fn tool_call_id(&self) -> &str {
        &self.call.id
    }

    pub fn tool_invocation_id(&self) -> &str {
        self.invocation_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub name: String,
    pub result: String,
    pub failed: bool,
}

pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

pub struct ToolStep {
    pub text: Option<String>,
    pub calls: Vec<ToolCall>,
    pub results: Vec<ToolResult>,
    /// 供同一协议重放的原始模型内容，例如签名与推理块。
    pub provider_content: Option<Value>,
}

pub struct StepResponse {
    pub model: Option<String>,
    pub text: Option<String>,
    pub usage: Usage,
    pub tool_calls: Vec<ToolCall>,
    pub provider_content: Option<Value>,
}

/// 实现一次生成步骤的协议适配器。
pub trait StepTextGateway: Send + Sync {
    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>>;
}

pub(crate) async fn run(
    provider: &dyn TextProvider,
    prompt: AgentPrompt<'_>,
    tools: Vec<Arc<dyn Tool>>,
    max_steps: Option<usize>,
) -> AiResult<AgentResponse> {
    let gateway = provider
        .step_text_gateway()
        .ok_or(AiError::ToolsNotSupported)?;
    let default_steps = tools.len().saturating_mul(3).div_ceil(2).min(25);
    let max_steps = max_steps.unwrap_or(default_steps).max(1);
    let mut definitions = Vec::with_capacity(tools.len());
    let mut names = BTreeMap::new();
    for tool in &tools {
        let name = tool.name();
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || names.insert(name, tool).is_some()
        {
            return Err(AiError::InvalidTools);
        }
        definitions.push(ToolDefinition {
            name: name.into(),
            description: tool.description().into(),
            schema: Type::from(tool.schema(JsonSchemaTypeFactory))
                .to_array()
                .map_err(|_| AiError::InvalidSchema)?,
        });
    }

    let mut history = Vec::new();
    let mut seen = HashSet::new();
    let mut usage = None::<Usage>;
    for step in 1..=max_steps {
        let response = gateway
            .generate_text_step(&prompt, &definitions, &history)
            .await?;
        let mut totals = response.usage;
        if let Some(previous) = usage.as_ref() {
            totals.prompt_tokens = sum(previous.prompt_tokens, totals.prompt_tokens);
            totals.completion_tokens = sum(previous.completion_tokens, totals.completion_tokens);
        }
        for call in &response.tool_calls {
            if call.id.trim().is_empty() || !seen.insert(call.id.clone()) {
                return Err(AiError::InvalidResponse);
            }
        }
        let mut results = Vec::with_capacity(response.tool_calls.len());
        for call in &response.tool_calls {
            let output = match names.get(call.name.as_str()) {
                _ if step == max_steps => Err("已达到 Agent 的最大步骤数"),
                Some(tool) => tool
                    .handle(ToolRequest {
                        call,
                        invocation_id: &Str::ulid().to_string(),
                    })
                    .await
                    .map_err(|_| "工具执行失败"),
                None => Err("该工具未注册"),
            };
            let failed = output.is_err();
            results.push(ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                result: output.unwrap_or_else(str::to_owned),
                failed,
            });
        }
        let finished = response.tool_calls.is_empty() || step == max_steps;
        history.push(ToolStep {
            text: response.text.clone(),
            calls: response.tool_calls,
            results,
            provider_content: response.provider_content,
        });
        if finished {
            let mut tool_calls = Vec::new();
            let mut tool_results = Vec::new();
            for step in history {
                tool_calls.extend(step.calls);
                tool_results.extend(step.results);
            }
            if response.text.is_none() && tool_calls.is_empty() {
                return Err(AiError::InvalidResponse);
            }
            return Ok(AgentResponse {
                text: response.text.unwrap_or_default(),
                usage: totals,
                provider: provider.name().into(),
                model: response.model.unwrap_or_else(|| prompt.text.model.into()),
                tool_calls,
                tool_results,
            });
        }
        usage = Some(totals);
    }
    Err(AiError::InvalidResponse)
}

fn sum(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    first
        .zip(second)
        .map(|(first, second)| first.saturating_add(second))
}
