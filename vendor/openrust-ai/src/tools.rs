use std::{
    borrow::Cow,
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
    AgentMiddleware, AgentPrompt, AgentResponse, AiError, AiResult, Decision, Decisions,
    PendingApproval, PendingStep, PendingToolApprovals, TextProvider, Usage,
    approval_turn::ApprovalSession, error::ProviderAttemptError, messages::OwnedPrompt,
};

/// 由应用声明并执行的 Agent 工具。
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn localized_description(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.description())
    }
    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType;
    #[doc(hidden)]
    fn as_load_skill(&self) -> Option<&crate::LoadSkill> {
        None
    }

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

#[derive(Serialize, Deserialize)]
pub struct ToolStep {
    pub text: Option<String>,
    pub calls: Vec<ToolCall>,
    pub results: Vec<ToolResult>,
    /// 供同一协议重放的原始模型内容，例如签名与推理块。
    pub provider_content: Option<Value>,
}

#[derive(Serialize, Deserialize)]
pub struct StepResponse {
    pub model: Option<String>,
    pub text: Option<String>,
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<crate::UrlCitation>,
    pub tool_calls: Vec<ToolCall>,
    pub provider_content: Option<Value>,
}

pub enum StepEvent {
    TextDelta(String),
    TextPartDelta {
        part: (String, u64),
        delta: String,
    },
    TextPartEnd {
        part: (String, u64),
    },
    Citation {
        part: Option<(String, u64)>,
        citation: crate::UrlCitation,
    },
    Reasoning(crate::ReasoningPart),
    Finished {
        response: Box<StepResponse>,
        reason: String,
    },
}

pub type StepEventStream = Pin<Box<dyn Stream<Item = AiResult<StepEvent>> + Send>>;

/// 实现一次生成步骤的协议适配器。
pub trait StepTextGateway: Send + Sync {
    /// 返回用于恢复审批的稳定连接标识。
    fn approval_replay_scope(&self) -> AiResult<Option<String>> {
        Ok(None)
    }

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

#[derive(Serialize, Deserialize)]
pub(crate) struct ToolRun {
    pub(crate) invocation_id: String,
    pub(crate) history: Vec<ToolStep>,
    usage: Option<Usage>,
    max_steps: usize,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ApprovalCheckpoint {
    pub(crate) provider: String,
    pub(crate) prompt: OwnedPrompt,
    pub(crate) pending: Vec<PendingApproval>,
    pub(crate) existing: bool,
    replay_scope: String,
    run: ToolRun,
    response: StepResponse,
    allowed: BTreeSet<String>,
    invocation_ids: Vec<Option<String>>,
}

impl ApprovalCheckpoint {
    pub(crate) fn invocation_id(&self) -> &str {
        &self.run.invocation_id
    }

    pub(crate) async fn resume(
        self,
        provider: &dyn TextProvider,
        registered: Vec<Arc<dyn Tool>>,
        middleware: Vec<Arc<dyn AgentMiddleware>>,
        decisions: &Decisions,
        session: &mut ApprovalSession,
        pending_record: (&str, i64),
    ) -> AiResult<AgentResponse> {
        let replay_scope = provider
            .step_text_gateway()
            .ok_or(AiError::ApprovalNotFound)?
            .approval_replay_scope()?;
        if replay_scope.as_deref() != Some(&self.replay_scope) {
            return Err(AiError::ApprovalNotFound);
        }
        let (definitions, tools) = prepare(registered)?;
        let mut plan = plan_execution(
            &self.response.tool_calls,
            &tools,
            Some(&self.allowed),
            false,
            Some(&self.invocation_ids),
        )?;
        if plan.pending != self.pending {
            return Err(AiError::InvalidApprovalDecisions);
        }
        plan.apply_decisions(decisions)?;
        session.claim(pending_record.0, pending_record.1).await?;
        let results = plan.run().await;
        let approved_ids: BTreeSet<_> = self.pending.iter().map(|approval| &approval.id).collect();
        let resolved = results
            .iter()
            .filter(|result| approved_ids.contains(&result.id))
            .cloned()
            .collect();
        session.record_resolution(self.invocation_id(), pending_record.0, resolved);
        let mut run = self.run;
        run.add_step(self.response, results);
        run.generate(
            provider,
            self.prompt.as_prompt(0),
            &definitions,
            &tools,
            &middleware,
            Some(session),
        )
        .await
        .map_err(ProviderAttemptError::into_error)
    }
}

pub(crate) async fn run(
    provider: &dyn TextProvider,
    prompt: AgentPrompt<'_>,
    tools: Vec<Arc<dyn Tool>>,
    max_steps: Option<usize>,
    middleware: Vec<Arc<dyn AgentMiddleware>>,
    approvals: Option<&ApprovalSession>,
) -> Result<AgentResponse, ProviderAttemptError> {
    let max_steps = step_limit(tools.len(), max_steps);
    let (definitions, tools) = prepare(tools)?;
    ToolRun::new(max_steps)
        .generate(
            provider,
            prompt,
            &definitions,
            &tools,
            &middleware,
            approvals,
        )
        .await
}

impl ToolRun {
    pub(crate) fn new(max_steps: usize) -> Self {
        Self {
            invocation_id: crate::request_context::invocation_id()
                .unwrap_or_else(|| Str::ulid().to_string()),
            history: Vec::new(),
            usage: None,
            max_steps,
        }
    }

    pub(crate) fn add_step(&mut self, response: StepResponse, results: Vec<ToolResult>) {
        self.usage = Some(match self.usage.as_ref() {
            Some(previous) => response.usage.combine(previous),
            None => response.usage,
        });
        self.history.push(ToolStep {
            text: response.text,
            calls: response.tool_calls,
            results,
            provider_content: response.provider_content,
        });
    }

    pub(crate) async fn pause(
        self,
        approval: Option<&ApprovalSession>,
        provider: &dyn TextProvider,
        prompt: AgentPrompt<'_>,
        response: StepResponse,
        plan: ApprovalPlan,
        allowed: BTreeSet<String>,
    ) -> AiResult<PendingToolApprovals> {
        let Some(session) = approval.filter(|_| prompt.attachments.is_empty()) else {
            return Err(AiError::ApprovalNotResumable {
                pending: plan.pending,
            });
        };
        let Some(replay_scope) = provider
            .step_text_gateway()
            .ok_or(AiError::ToolsNotSupported)?
            .approval_replay_scope()?
        else {
            return Err(AiError::ApprovalNotResumable {
                pending: plan.pending,
            });
        };
        session
            .pause(&ApprovalCheckpoint {
                provider: provider.name().to_owned(),
                prompt: OwnedPrompt::from(prompt),
                pending: plan.pending,
                existing: session.existing,
                replay_scope,
                run: self,
                response,
                allowed,
                invocation_ids: plan.invocation_ids,
            })
            .await
    }

    async fn generate(
        mut self,
        provider: &dyn TextProvider,
        prompt: AgentPrompt<'_>,
        definitions: &[ToolDefinition],
        tools: &ToolHandlers,
        middleware: &[Arc<dyn AgentMiddleware>],
        approvals: Option<&ApprovalSession>,
    ) -> Result<AgentResponse, ProviderAttemptError> {
        let mut seen = self
            .history
            .iter()
            .flat_map(|step| step.calls.iter().map(|call| call.id.clone()))
            .collect::<HashSet<_>>();
        for step in self.history.len()..self.max_steps {
            let final_step = step + 1 == self.max_steps;
            let (mut response, allowed) = crate::middleware::generate_step(
                provider,
                PendingStep {
                    number: step,
                    is_final_step: final_step,
                    provider: provider.name(),
                    model: prompt.text.model.into(),
                    instructions: prompt.text.instructions.into(),
                    prompt: prompt.text.prompt,
                    options: prompt.options,
                    provider_options: prompt.provider_options.map(std::borrow::Cow::Borrowed),
                    tool_choice: prompt
                        .tool_choice
                        .and_then(|choice| choice.for_step(step))
                        .cloned(),
                    messages: prompt.messages.into(),
                    attachments: prompt.attachments.into(),
                    structured_output: prompt.structured_output,
                    steps: &self.history,
                    usage: self.usage.clone().unwrap_or_default(),
                    timeout: prompt.text.timeout,
                    invocation_id: &self.invocation_id,
                    tools: std::borrow::Cow::Borrowed(definitions),
                },
                middleware,
            )
            .await
            .map_err(|error| {
                if step == 0 && middleware.is_empty() {
                    ProviderAttemptError::BeforeCommit(error)
                } else {
                    ProviderAttemptError::Committed(error)
                }
            })?;
            validate_calls(&response.tool_calls, &mut seen)?;
            let plan = plan_execution(
                &response.tool_calls,
                tools,
                Some(&allowed),
                final_step,
                None,
            )?;
            if plan.requires_approval() {
                let approval = plan.for_pause();
                let pending = self
                    .pause(approvals, provider, prompt, response, approval, allowed)
                    .await?;
                return Err(AiError::ApprovalPaused(pending).into());
            }
            let results = plan.run().await;
            let finished = response.tool_calls.is_empty() || final_step;
            let model = response.model.clone();
            let citations = if finished {
                std::mem::take(&mut response.citations)
            } else {
                Vec::new()
            };
            self.add_step(response, results);
            if finished {
                let text = self.history.last_mut().and_then(|step| step.text.take());
                let mut tool_calls = Vec::new();
                let mut tool_results = Vec::new();
                for step in self.history {
                    tool_calls.extend(step.calls);
                    tool_results.extend(step.results);
                }
                if text.is_none() && tool_calls.is_empty() {
                    return Err(AiError::InvalidResponse.into());
                }
                return Ok(AgentResponse {
                    text: text.unwrap_or_default(),
                    usage: self.usage.unwrap_or_default(),
                    provider: provider.name().into(),
                    model: model.unwrap_or_else(|| prompt.text.model.into()),
                    citations,
                    tool_calls,
                    tool_results,
                });
            }
        }
        Err(AiError::InvalidResponse.into())
    }
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
            description: tool.localized_description().into_owned(),
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
    pending: Vec<PendingApproval>,
}

enum ToolAction<'a> {
    Run {
        tool: &'a dyn Tool,
        invocation_id: String,
        edit: Option<Map<String, Value>>,
    },
    Reject(String),
    Skip(&'static str),
}

pub(crate) struct ApprovalPlan {
    pending: Vec<PendingApproval>,
    invocation_ids: Vec<Option<String>>,
}

pub(crate) fn plan_execution<'a>(
    calls: &'a [ToolCall],
    tools: &'a ToolHandlers,
    allowed: Option<&BTreeSet<String>>,
    final_step: bool,
    invocation_ids: Option<&[Option<String>]>,
) -> AiResult<ExecutionPlan<'a>> {
    let mut pending = Vec::new();
    let mut actions = Vec::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        let action = match tools.get(&call.name) {
            _ if final_step => ToolAction::Skip("已达到 Agent 的最大步骤数"),
            Some(tool) if allowed.is_none_or(|names| names.contains(&call.name)) => {
                let invocation_id = match invocation_ids {
                    None => Str::ulid().to_string(),
                    Some(ids) => ids
                        .get(index)
                        .and_then(Option::as_ref)
                        .cloned()
                        .ok_or(AiError::InvalidApprovalDecisions)?,
                };
                let request = ToolRequest {
                    call,
                    invocation_id: &invocation_id,
                };
                if let Some(approval) = tool.should_request_approval(&request) {
                    pending.push(PendingApproval::for_call(call, approval));
                }
                ToolAction::Run {
                    tool: tool.as_ref(),
                    invocation_id,
                    edit: None,
                }
            }
            _ => {
                if invocation_ids
                    .is_some_and(|ids| ids.get(index).and_then(Option::as_ref).is_some())
                {
                    return Err(AiError::InvalidApprovalDecisions);
                }
                ToolAction::Skip("该步骤未启用该工具")
            }
        };
        actions.push(action);
    }
    Ok(ExecutionPlan {
        calls,
        actions,
        pending,
    })
}

impl ExecutionPlan<'_> {
    pub(crate) fn requires_approval(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(crate) fn for_pause(self) -> ApprovalPlan {
        let invocation_ids = self
            .actions
            .into_iter()
            .map(|action| match action {
                ToolAction::Run { invocation_id, .. } => Some(invocation_id),
                _ => None,
            })
            .collect();
        ApprovalPlan {
            pending: self.pending,
            invocation_ids,
        }
    }

    fn apply_decisions(&mut self, decisions: &Decisions) -> AiResult<()> {
        let pending: BTreeSet<_> = self
            .pending
            .iter()
            .map(|approval| approval.id.as_str())
            .collect();
        if decisions
            .all()
            .keys()
            .any(|id| id != "*" && !pending.contains(id.as_str()))
        {
            return Err(AiError::InvalidApprovalDecisions);
        }
        for (call, action) in self.calls.iter().zip(&mut self.actions) {
            if !pending.contains(call.id.as_str()) {
                continue;
            }
            let decision = decisions
                .get(&call.id)
                .or_else(|| decisions.get("*"))
                .ok_or(AiError::InvalidApprovalDecisions)?;
            match decision {
                Decision::Approve => {}
                Decision::Reject { result } => {
                    *action = ToolAction::Reject(
                        result.clone().unwrap_or_else(|| "工具调用未获批准".into()),
                    );
                }
                Decision::Edit { arguments } => {
                    let ToolAction::Run { edit, .. } = action else {
                        return Err(AiError::InvalidApprovalDecisions);
                    };
                    *edit = Some(arguments.clone());
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn run(self) -> Vec<ToolResult> {
        let mut results = Vec::with_capacity(self.calls.len());
        for (call, action) in self.calls.iter().zip(self.actions) {
            let output = match action {
                ToolAction::Run {
                    tool,
                    invocation_id,
                    edit,
                } => {
                    let edited = edit.map(|arguments| ToolCall {
                        arguments,
                        ..call.clone()
                    });
                    tool.handle(ToolRequest {
                        call: edited.as_ref().unwrap_or(call),
                        invocation_id: &invocation_id,
                    })
                    .await
                    .map_err(|_| "工具执行失败".to_owned())
                }
                ToolAction::Reject(message) => Err(message),
                ToolAction::Skip(message) => Err(message.to_owned()),
            };
            let failed = output.is_err();
            results.push(ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                failed,
                result: output.unwrap_or_else(std::convert::identity),
            });
        }
        results
    }
}
