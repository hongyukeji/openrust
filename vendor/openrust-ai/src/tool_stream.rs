use std::{collections::HashSet, future::Future, sync::Arc};

use futures_util::StreamExt;
use openrust::{
    foundation::Application,
    support::facades::{App, Lang},
};

use crate::{
    AgentPrompt, AiError, AiResult, StepEvent, StepEventStream, StreamEventData,
    StreamableAgentResponse, TextProvider, Tool,
    approval_turn::ApprovalSession,
    messages::OwnedPrompt,
    streaming::{TextMessages, event},
    tools,
};

pub(crate) struct PreparedToolStream {
    provider: Arc<dyn TextProvider>,
    prompt: OwnedPrompt,
    definitions: Vec<crate::ToolDefinition>,
    registered: tools::ToolHandlers,
    run: tools::ToolRun,
    max_steps: usize,
    upstream: StepEventStream,
}

pub(crate) async fn prepare(
    provider: Arc<dyn TextProvider>,
    prompt: AgentPrompt<'_>,
    registered: Vec<Arc<dyn Tool>>,
    max_steps: Option<usize>,
    probe: bool,
) -> AiResult<PreparedToolStream> {
    let max_steps = tools::step_limit(registered.len(), max_steps);
    let (definitions, registered) = tools::prepare(registered)?;
    if let Some(choice) = prompt.tool_choice {
        choice.validate(&definitions)?;
    }
    let gateway = provider
        .step_text_gateway()
        .ok_or(AiError::ToolsNotSupported)?;
    let run = tools::ToolRun::new(max_steps);
    let mut upstream = gateway.stream_text_step(&prompt, &definitions, &[]).await?;
    if probe {
        let first = upstream.next().await.transpose()?;
        if let Some(first) = first {
            upstream = Box::pin(futures_util::stream::iter([Ok(first)]).chain(upstream));
        }
    }
    Ok(PreparedToolStream {
        provider,
        prompt: OwnedPrompt::from(prompt),
        definitions,
        registered,
        run,
        max_steps,
        upstream,
    })
}

pub(crate) fn response(
    prepared: PreparedToolStream,
    approvals: Option<ApprovalSession>,
) -> StreamableAgentResponse {
    let application = App::get_instance().ok();
    let locale = Lang::get_locale().ok();
    let PreparedToolStream {
        provider,
        prompt,
        definitions,
        registered,
        mut run,
        max_steps,
        mut upstream,
    } = prepared;
    let invocation_id = run.invocation_id.clone();
    let stream = async_stream::try_stream! {
        let mut seen = HashSet::new();
        for step in 1..=max_steps {
            yield event(&invocation_id, StreamEventData::StreamStart {
                provider: provider.name().into(),
                model: prompt.as_prompt(step - 1).text.model.into(),
            });
            let mut messages = TextMessages::default();
            let mut reasoning = TextMessages::default();
            let mut has_text = false;
            let (completed, reason) = loop {
                let (part, delta) = match upstream.next().await.ok_or(AiError::InvalidResponse)?? {
                    StepEvent::TextDelta(delta) => (None, delta),
                    StepEvent::TextPartDelta { part, delta } => (Some(part), delta),
                    StepEvent::TextPartEnd { part } => {
                        if let Some(message_id) = messages.close(Some(&part)) {
                            yield event(&invocation_id, StreamEventData::TextEnd { message_id });
                        }
                        continue;
                    }
                    StepEvent::Citation { part, citation } => {
                        yield event(&invocation_id, messages.citation(part.as_ref(), citation));
                        continue;
                    }
                    StepEvent::Reasoning(part) => {
                        for data in reasoning.reasoning(part).into_iter().flatten() {
                            yield event(&invocation_id, data);
                        }
                        continue;
                    }
                    StepEvent::Finished { response, reason } => break (*response, reason),
                };
                if !delta.is_empty() {
                    has_text = true;
                    for data in messages.delta(part.as_ref(), delta).into_iter().flatten() {
                        yield event(&invocation_id, data);
                    }
                }
            };
            drop(upstream);
            for reasoning_id in reasoning.open_ids() {
                yield event(&invocation_id, StreamEventData::ReasoningEnd { reasoning_id });
            }
            if !has_text {
                if let Some(text) = completed.text.as_ref().filter(|text| !text.is_empty()) {
                    for data in messages.delta(None, text.clone()).into_iter().flatten() {
                        yield event(&invocation_id, data);
                    }
                } else if completed.tool_calls.is_empty() {
                    Err(AiError::InvalidResponse)?;
                }
            }
            for message_id in messages.open_ids() {
                yield event(&invocation_id, StreamEventData::TextEnd { message_id });
            }
            if let Some(choice) = prompt.choice_for(step - 1) {
                choice.validate_calls(&completed.tool_calls)?;
            }
            tools::validate_calls(&completed.tool_calls, &mut seen)?;
            let plan = || tools::plan_execution(
                &completed.tool_calls, &registered, None, step == max_steps, None,
            );
            let execution = in_tool_scope(application.as_deref(), locale.as_deref(), async { plan() }).await??;
            if execution.requires_approval() {
                let plan = execution.for_pause();
                let calls = completed.tool_calls.clone();
                let allowed = registered.keys().cloned().collect();
                let pause = run.pause(
                    approvals.as_ref(), provider.as_ref(), prompt.as_prompt(0),
                    completed, plan, allowed,
                );
                let pending = in_tool_scope(application.as_deref(), locale.as_deref(), pause).await??;
                let conversation_id = approvals.as_ref().expect("已保存审批会话").conversation_id.clone();
                for call in calls {
                    yield event(&invocation_id, StreamEventData::ToolCall { call });
                }
                yield event(&invocation_id, StreamEventData::ToolApprovalRequest {
                    conversation_id,
                    approval_id: pending.id,
                    approvals: pending.approvals,
                });
                return;
            }
            for call in &completed.tool_calls {
                yield event(&invocation_id, StreamEventData::ToolCall { call: call.clone() });
            }
            let execution = execution.run();
            let results = in_tool_scope(application.as_deref(), locale.as_deref(), execution).await?;
            for result in &results {
                yield event(&invocation_id, StreamEventData::ToolResult { result: result.clone() });
            }
            yield event(&invocation_id, StreamEventData::StreamEnd { reason, usage: completed.usage.clone() });
            if completed.tool_calls.is_empty() || step == max_steps {
                return;
            }
            run.add_step(completed, results);
            let gateway = provider.step_text_gateway().ok_or(AiError::ToolsNotSupported)?;
            upstream = crate::request_context::in_invocation(
                &invocation_id, step,
                gateway.stream_text_step(&prompt.as_prompt(step), &definitions, &run.history),
            ).await?;
        }
    };
    StreamableAgentResponse::from_events(Box::pin(stream))
}

async fn in_tool_scope<T>(
    application: Option<&Application>,
    locale: Option<&str>,
    operation: impl Future<Output = T>,
) -> AiResult<T> {
    let Some(application) = application else {
        return Ok(operation.await);
    };
    application
        .scope(async {
            // 在异步 HTTP 正文中恢复创建流时的语言。
            match locale {
                Some(locale) => Lang::with_locale(locale, operation)
                    .await
                    .map_err(|_| AiError::Unavailable),
                None => Ok(operation.await),
            }
        })
        .await
}
