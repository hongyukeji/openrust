use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    pin::Pin,
    sync::Arc,
};

use futures_util::Stream;
use openrust::{
    http::HttpFuture,
    json_schema::{JsonSchemaTypeFactory, Type, types::ObjectType},
    support::{Str, facades::Validator},
    validation::{Data, RuleSet, ValidatedInput, ValidationResult},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::{
    AgentMiddleware, AgentPrompt, AgentResponse, AiError, AiResult, PendingStep, TextProvider,
    Usage,
};

/// 由应用声明并执行的 Agent 工具。
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType;
    fn should_request_approval(&self, _request: &ToolRequest<'_>) -> Option<crate::Approval> {
        None
    }

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

#[derive(Clone)]
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

pub enum StepEvent {
    TextDelta(String),
    Finished {
        response: StepResponse,
        reason: String,
    },
}

pub type StepEventStream = Pin<Box<dyn Stream<Item = AiResult<StepEvent>> + Send>>;

/// 实现一次生成步骤的协议适配器。
pub trait StepTextGateway: Send + Sync {
    fn generate_text_step<'a>(
        &'a self,
        prompt: &'a AgentPrompt<'a>,
        tools: &'a [ToolDefinition],
        history: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepResponse>>;
    fn stream_text_step<'a>(
        &'a self,
        _: &'a AgentPrompt<'a>,
        _: &'a [ToolDefinition],
        _: &'a [ToolStep],
    ) -> HttpFuture<'a, AiResult<StepEventStream>> {
        Box::pin(async { Err(AiError::ToolsNotSupported) })
    }
}

pub(crate) async fn run(
    provider: &dyn TextProvider,
    prompt: AgentPrompt<'_>,
    tools: Vec<Arc<dyn Tool>>,
    max_steps: Option<usize>,
    middleware: Vec<Arc<dyn AgentMiddleware>>,
) -> AiResult<AgentResponse> {
    let max_steps = step_limit(tools.len(), max_steps);
    let (definitions, tools) = prepare(tools)?;

    let invocation_id = Str::ulid().to_string();
    let mut history = Vec::new();
    let mut seen = HashSet::new();
    let mut usage = None::<Usage>;
    for step in 1..=max_steps {
        let (response, allowed) = crate::middleware::generate_step(
            provider,
            PendingStep {
                number: step - 1,
                is_final_step: step == max_steps,
                provider: provider.name(),
                model: prompt.text.model.into(),
                instructions: prompt.text.instructions.into(),
                prompt: prompt.text.prompt,
                options: prompt.options,
                provider_options: prompt.provider_options.map(std::borrow::Cow::Borrowed),
                tool_choice: prompt
                    .tool_choice
                    .and_then(|choice| choice.for_step(step - 1))
                    .cloned(),
                messages: prompt.messages.into(),
                attachments: prompt.attachments.into(),
                structured_output: prompt.structured_output,
                steps: &history,
                usage: usage.clone().unwrap_or_default(),
                timeout: prompt.text.timeout,
                invocation_id: &invocation_id,
                tools: std::borrow::Cow::Borrowed(&definitions),
            },
            &middleware,
        )
        .await?;
        let mut totals = response.usage;
        if let Some(previous) = usage.as_ref() {
            totals.prompt_tokens = sum(previous.prompt_tokens, totals.prompt_tokens);
            totals.completion_tokens = sum(previous.completion_tokens, totals.completion_tokens);
        }
        validate_calls(&response.tool_calls, &mut seen)?;
        let results = prepare_execution(
            &response.tool_calls,
            &tools,
            Some(&allowed),
            step == max_steps,
        )?
        .run()
        .await;
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

pub(crate) type ToolHandlers = BTreeMap<String, Arc<dyn Tool>>;

pub(crate) fn step_limit(count: usize, requested: Option<usize>) -> usize {
    let default_steps = count.saturating_mul(3).div_ceil(2).min(25);
    requested.unwrap_or(default_steps).max(1)
}

pub(crate) fn prepare(tools: Vec<Arc<dyn Tool>>) -> AiResult<(Vec<ToolDefinition>, ToolHandlers)> {
    let mut definitions = Vec::with_capacity(tools.len());
    let mut handlers = BTreeMap::new();
    for tool in tools {
        let name = tool.name();
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || handlers.contains_key(name)
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
        handlers.insert(name.into(), tool);
    }
    Ok((definitions, handlers))
}

pub(crate) fn validate_calls(calls: &[ToolCall], seen: &mut HashSet<String>) -> AiResult<()> {
    for call in calls {
        if call.id.trim().is_empty() || !seen.insert(call.id.clone()) {
            return Err(AiError::InvalidResponse);
        }
    }
    Ok(())
}

pub(crate) struct ExecutionPlan<'a> {
    calls: &'a [ToolCall],
    actions: Vec<ToolAction<'a>>,
}

enum ToolAction<'a> {
    Run {
        tool: &'a dyn Tool,
        invocation_id: String,
    },
    Skip(&'static str),
}

pub(crate) fn prepare_execution<'a>(
    calls: &'a [ToolCall],
    tools: &'a ToolHandlers,
    allowed: Option<&BTreeSet<String>>,
    final_step: bool,
) -> AiResult<ExecutionPlan<'a>> {
    let mut pending = Vec::new();
    let mut actions = Vec::with_capacity(calls.len());
    for call in calls {
        let action = match tools.get(&call.name) {
            _ if final_step => ToolAction::Skip("已达到 Agent 的最大步骤数"),
            Some(tool) if allowed.is_none_or(|names| names.contains(&call.name)) => {
                let invocation_id = Str::ulid().to_string();
                let request = ToolRequest {
                    call,
                    invocation_id: &invocation_id,
                };
                if let Some(approval) = tool.should_request_approval(&request) {
                    pending.push(crate::PendingApproval::for_call(call, approval));
                }
                ToolAction::Run {
                    tool: tool.as_ref(),
                    invocation_id,
                }
            }
            _ => ToolAction::Skip("该步骤未启用该工具"),
        };
        actions.push(action);
    }
    if !pending.is_empty() {
        return Err(AiError::ApprovalNotResumable { pending });
    }
    Ok(ExecutionPlan { calls, actions })
}

impl ExecutionPlan<'_> {
    pub(crate) async fn run(self) -> Vec<ToolResult> {
        let mut results = Vec::with_capacity(self.calls.len());
        for (call, action) in self.calls.iter().zip(self.actions) {
            let output = match action {
                ToolAction::Run {
                    tool,
                    invocation_id,
                } => tool
                    .handle(ToolRequest {
                        call,
                        invocation_id: &invocation_id,
                    })
                    .await
                    .map_err(|_| "工具执行失败"),
                ToolAction::Skip(message) => Err(message),
            };
            results.push(ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                failed: output.is_err(),
                result: output.unwrap_or_else(str::to_owned),
            });
        }
        results
    }
}

fn sum(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    first
        .zip(second)
        .map(|(first, second)| first.saturating_add(second))
}
