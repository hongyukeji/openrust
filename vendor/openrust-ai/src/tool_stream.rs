use std::{collections::HashSet, future::Future, sync::Arc};

use futures_util::StreamExt;
use openrust::{
    foundation::Application,
    support::facades::{App, Lang},
};

use crate::{
    AgentMiddleware, AgentPrompt, AgentResponse, AiError, AiResult, StepEvent, StepEventStream,
    StreamEventData, StreamableAgentResponse, TextProvider, Tool, ToolDefinition, ToolStep, Usage,
    approval_turn::ApprovalSession,
    events::StepObservation,
    messages::OwnedPrompt,
    middleware::{PreparedStreamStep, generate_stream_step},
    streaming::{StreamMetadata, TextMessages, event},
    tools,
};

pub(crate) struct PreparedToolStream {
    provider: Arc<dyn TextProvider>,
    prompt: OwnedPrompt,
    definitions: Vec<crate::ToolDefinition>,
    registered: tools::ToolHandlers,
    run: tools::ToolRun,
    max_steps: usize,
    opened: Option<PreparedStreamStep>,
    middleware: Vec<Arc<dyn AgentMiddleware>>,
    agent: Option<&'static str>,
}

pub(crate) async fn prepare(
    provider: Arc<dyn TextProvider>,
    prompt: AgentPrompt<'_>,
    registered: Vec<Arc<dyn Tool>>,
    max_steps: Option<usize>,
    middleware: Vec<Arc<dyn AgentMiddleware>>,
    probe: bool,
) -> AiResult<PreparedToolStream> {
    let max_steps = tools::step_limit(registered.len(), max_steps);
    let (definitions, registered) = tools::prepare(registered)?;
    let run = tools::ToolRun::new(max_steps);
    let mut opened = generate_stream_step(
        provider.as_ref(),
        run.pending_step(provider.name(), &prompt, &definitions),
        &middleware,
    )
    .await?;
    if probe {
        opened.result = opened.result.probe().await?;
    }
    Ok(PreparedToolStream {
        provider,
        prompt: OwnedPrompt::from(prompt),
        definitions,
        registered,
        run,
        max_steps,
        opened: Some(opened),
        middleware,
        agent: crate::request_context::agent_invocation().map(|(agent, _)| agent),
    })
}

pub(crate) fn response(
    prepared: PreparedToolStream,
    approvals: Option<ApprovalSession>,
) -> StreamableAgentResponse {
    events(prepared, approvals.map(Arc::new), StreamMetadata::default())
}

pub(crate) fn resume(
    provider: Arc<dyn TextProvider>,
    resolved: tools::ResolvedApproval,
    middleware: Vec<Arc<dyn AgentMiddleware>>,
    session: Arc<ApprovalSession>,
    prefix: AgentResponse,
    agent: &'static str,
) -> StreamableAgentResponse {
    let max_steps = resolved.run.max_steps();
    events(
        PreparedToolStream {
            provider,
            prompt: resolved.prompt,
            definitions: resolved.definitions,
            registered: resolved.tools,
            run: resolved.run,
            max_steps,
            opened: None,
            middleware,
            agent: Some(agent),
        },
        Some(session),
        StreamMetadata::with_resume(prefix),
    )
}

fn events(
    prepared: PreparedToolStream,
    approvals: Option<Arc<ApprovalSession>>,
    metadata: StreamMetadata,
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
        mut opened,
        middleware,
        agent,
    } = prepared;
    let invocation_id = run.invocation_id.clone();
    let replay_scope = provider
        .step_text_gateway()
        .and_then(|gateway| gateway.approval_replay_scope().ok().flatten());
    let observed = metadata.clone();
    let stream = async_stream::try_stream! {
        let mut seen: HashSet<_> = run.history.iter().flat_map(|step| step.calls.iter().map(|call| call.id.clone())).collect();
        let first_step = run.history.len() + 1;
        if let Some(prefix) = observed.resumed() {
            yield event(&invocation_id, StreamEventData::StreamStart {
                provider: provider.name().into(), model: prefix.model.clone(),
            });
            for result in prefix.steps.last().into_iter().flat_map(|step| &step.tool_results) {
                yield event(&invocation_id, StreamEventData::ToolResult { result: result.clone() });
            }
        }
        for step in first_step..=max_steps {
            let PreparedStreamStep { result, model, allowed, choice } = match opened.take() {
                Some(opened) => opened,
                None => {
                    let request = prompt.as_prompt(0);
                    let open = crate::request_context::in_agent_invocation(
                        &invocation_id, step - 1, agent,
                        generate_stream_step(
                            provider.as_ref(),
                            run.pending_step(provider.name(), &request, &definitions),
                            &middleware,
                        ),
                    );
                    in_stream_scope(application.as_deref(), locale.as_deref(), open).await??
                }
            };
            let resumed_first = step == first_step && observed.resumed().is_some();
            if result.stopped() {
                observed.mark_stopped();
                if step == 1 {
                    yield event(&invocation_id, StreamEventData::StreamStart {
                        provider: provider.name().into(), model,
                    });
                }
                yield event(&invocation_id, StreamEventData::StreamEnd {
                    reason:"cancelled".into(), usage:Usage::zero(),
                });
                return;
            }
            if !resumed_first {
                yield event(&invocation_id, StreamEventData::StreamStart {
                    provider: provider.name().into(),
                    model: model.clone(),
                });
            }
            let mut upstream = result.into_stream();
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
            if let Some(choice) = choice {
                choice.validate_calls(&completed.tool_calls)?;
            }
            tools::validate_calls(&completed.tool_calls, &mut seen)?;
            let replay = crate::provider_replay::ProviderReplay::capture(
                replay_scope.as_deref(), &model,
                completed.provider_content.as_ref().filter(|_| !completed.tool_calls.is_empty()),
            );
            observed.record(completed.model.clone(), completed.usage.clone(), completed.finish_reason, replay);
            let plan = || tools::plan_execution(
                &completed.tool_calls, &registered, Some(&allowed), step == max_steps, None,
            );
            let execution = in_stream_scope(application.as_deref(), locale.as_deref(), async { plan() }).await??;
            if execution.requires_approval() {
                let plan = execution.for_pause();
                let calls = completed.tool_calls.clone();
                let pause = run.pause(
                    approvals.as_deref(), provider.as_ref(), prompt.as_prompt(0),
                    crate::middleware::CompletedStep {
                        response: completed,
                        allowed,
                        requested_model: Some(model),
                    },
                    plan,
                );
                let pending = in_stream_scope(application.as_deref(), locale.as_deref(), pause).await??;
                let session = approvals.as_ref().expect("已保存审批会话");
                in_stream_scope(application.as_deref(), locale.as_deref(), session.publish_resolution()).await?;
                for call in calls {
                    yield event(&invocation_id, StreamEventData::ToolCall { call });
                }
                yield event(&invocation_id, StreamEventData::ToolApprovalRequest {
                    conversation_id: session.conversation_id.clone(),
                    approval_id: pending.id,
                    approvals: pending.approvals,
                });
                return;
            }
            for call in &completed.tool_calls {
                yield event(&invocation_id, StreamEventData::ToolCall { call: call.clone() });
            }
            let execution = crate::request_context::in_agent_invocation(
                &invocation_id, step - 1, agent, execution.run(),
            );
            let results = in_stream_scope(application.as_deref(), locale.as_deref(), execution).await?;
            for result in &results {
                yield event(&invocation_id, StreamEventData::ToolResult { result: result.clone() });
            }
            yield event(&invocation_id, StreamEventData::StreamEnd { reason, usage: completed.usage.clone() });
            if completed.tool_calls.is_empty() || step == max_steps {
                return;
            }
            run.add_step(completed, results, Some(model));
        }
    };
    StreamableAgentResponse::from_events(Box::pin(stream)).with_step_metadata(metadata)
}

pub(crate) async fn stream_step(
    provider: &dyn TextProvider,
    prompt: &AgentPrompt<'_>,
    definitions: &[ToolDefinition],
    history: &[ToolStep],
    final_step: bool,
) -> AiResult<StepEventStream> {
    let gateway = provider
        .step_text_gateway()
        .ok_or(AiError::ToolsNotSupported)?;
    let observation =
        StepObservation::start(provider.name(), prompt, history, history.len(), final_step).await;
    let mut events = match gateway.stream_text_step(prompt, definitions, history).await {
        Ok(events) => events,
        Err(error) => {
            if let Some(observation) = observation {
                observation.fail(&error).await;
            }
            return Err(error);
        }
    };
    let Some(observation) = observation.filter(StepObservation::wants_result) else {
        return Ok(events);
    };
    let app = App::get_instance().ok();
    let locale = Lang::get_locale().ok();
    Ok(Box::pin(async_stream::try_stream! {
        let mut observation = Some(observation);
        while let Some(event) = events.next().await {
            match &event {
                Ok(StepEvent::Finished { response, .. }) => {
                    if let Some(observation) = observation.take() {
                        let complete = observation.complete(|step| {
                            response.public_step(&step.prompt.provider, step.prompt.as_prompt().text.model)
                        });
                        in_stream_scope(app.as_deref(), locale.as_deref(), complete).await?;
                    }
                }
                Err(error) => {
                    if let Some(observation) = observation.take() {
                        in_stream_scope(app.as_deref(), locale.as_deref(), observation.fail(error)).await?;
                    }
                }
                _ => {}
            }
            yield event?;
        }
        if let Some(observation) = observation {
            in_stream_scope(app.as_deref(), locale.as_deref(), observation.fail(&AiError::InvalidResponse)).await?;
        }
    }))
}

pub(crate) async fn in_stream_scope<T>(
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
