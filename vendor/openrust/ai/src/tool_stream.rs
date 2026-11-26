use std::{collections::HashSet, future::Future, sync::Arc};

use futures_util::StreamExt;
use openrust::{
    foundation::Application,
    support::{
        Str,
        facades::{App, Lang},
    },
};

use crate::{
    AgentPrompt, AiError, AiResult, StepEvent, StreamEventData, StreamableAgentResponse,
    TextProvider, Tool, approval_turn::ApprovalSession, messages::OwnedPrompt, streaming::event,
    tools,
};

pub(crate) async fn stream(
    provider: Arc<dyn TextProvider>,
    prompt: AgentPrompt<'_>,
    registered: Vec<Arc<dyn Tool>>,
    max_steps: Option<usize>,
    approvals: Option<ApprovalSession>,
) -> AiResult<StreamableAgentResponse> {
    let application = App::get_instance().ok();
    let locale = Lang::get_locale().ok();
    let max_steps = tools::step_limit(registered.len(), max_steps);
    let (definitions, registered) = tools::prepare(registered)?;
    if let Some(choice) = prompt.tool_choice {
        choice.validate(&definitions)?;
    }
    let gateway = provider
        .step_text_gateway()
        .ok_or(AiError::ToolsNotSupported)?;
    let mut run = tools::ToolRun::new(max_steps);
    let invocation_id = run.invocation_id.clone();
    let mut upstream = crate::request_context::in_invocation(
        &invocation_id,
        0,
        gateway.stream_text_step(&prompt, &definitions, &[]),
    )
    .await?;
    let prompt = OwnedPrompt::from(prompt);
    let stream = async_stream::try_stream! {
        let mut seen = HashSet::new();
        for step in 1..=max_steps {
            yield event(&invocation_id, StreamEventData::StreamStart {
                provider: provider.name().into(),
                model: prompt.as_prompt(step - 1).text.model.into(),
            });
            let mut message_id = None::<String>;
            let (completed, reason) = loop {
                let part = upstream.next().await.ok_or(AiError::InvalidResponse)??;
                match part {
                    StepEvent::TextDelta(delta) if !delta.is_empty() => {
                        let id = match &message_id {
                            Some(id) => id.clone(),
                            None => {
                                let id = Str::ulid().to_string();
                                message_id = Some(id.clone());
                                yield event(&invocation_id, StreamEventData::TextStart { message_id: id.clone() });
                                id
                            }
                        };
                        yield event(&invocation_id, StreamEventData::TextDelta { message_id: id, delta });
                    }
                    StepEvent::TextDelta(_) => {}
                    StepEvent::Finished { response, reason } => break (*response, reason),
                }
            };
            drop(upstream);
            let message_id = match (message_id, completed.text.as_ref().filter(|text| !text.is_empty())) {
                (None, Some(text)) => {
                    let id = Str::ulid().to_string();
                    yield event(&invocation_id, StreamEventData::TextStart { message_id: id.clone() });
                    yield event(&invocation_id, StreamEventData::TextDelta { message_id: id.clone(), delta: text.clone() });
                    Some(id)
                }
                (existing, _) => existing,
            };
            if let Some(message_id) = message_id {
                yield event(&invocation_id, StreamEventData::TextEnd { message_id });
            } else if completed.tool_calls.is_empty() {
                Err(AiError::InvalidResponse)?;
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
    Ok(StreamableAgentResponse::from_events(Box::pin(stream)))
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
