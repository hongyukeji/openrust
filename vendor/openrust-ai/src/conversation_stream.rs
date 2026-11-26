use std::future::Future;

use futures_util::StreamExt;

use crate::{
    AgentEventStream, AgentResponse, AiError, AiResult, StreamEventData, StreamableAgentResponse,
    StructuredAgentResponse, Usage,
};

/// 在确认上游完整结束并写入会话之后，才交付最后一个结束事件。
pub(crate) fn remember<F, Fut>(
    source: StreamableAgentResponse,
    conversation_id: String,
    structured: bool,
    persist: F,
) -> StreamableAgentResponse
where
    F: FnOnce(AgentResponse) -> Fut + Send + 'static,
    Fut: Future<Output = AiResult<()>> + Send + 'static,
{
    source.within_conversation(conversation_id, |events| {
        persisted_events(events, structured, persist)
    })
}

fn persisted_events<F, Fut>(
    mut source: AgentEventStream,
    structured: bool,
    persist: F,
) -> AgentEventStream
where
    F: FnOnce(AgentResponse) -> Fut + Send + 'static,
    Fut: Future<Output = AiResult<()>> + Send + 'static,
{
    let events = async_stream::try_stream! {
        let mut final_event = None;
        let mut response = None;
        let mut usage = None;
        while let Some(event) = source.next().await {
            let event = event?;
            if let Some(previous) = final_event.take() {
                yield previous;
            }
            if let StreamEventData::StreamStart { provider, model } = &event.data {
                let response = response.get_or_insert_with(|| AgentResponse::new("", Usage::default(), "", ""));
                response.provider.clone_from(provider);
                response.model.clone_from(model);
                if structured {
                    response.text.clear();
                }
            }
            let response = response.as_mut().ok_or(AiError::InvalidResponse)?;
            match &event.data {
                StreamEventData::ToolApprovalRequest { .. } => {
                    yield event;
                    return;
                }
                StreamEventData::TextDelta { delta, .. } => response.text.push_str(delta),
                StreamEventData::Citation { citation, .. } => response.citations.push(citation.clone()),
                StreamEventData::ToolCall { call } => response.tool_calls.push(call.clone()),
                StreamEventData::ToolResult { result } => response.tool_results.push(result.clone()),
                StreamEventData::StreamEnd { usage: step, .. } => {
                    let step = step.clone();
                    usage = Some(match usage.as_ref() {
                        Some(previous) => step.combine(previous),
                        None => step,
                    });
                    final_event = Some(event);
                    continue;
                }
                _ => {}
            }
            yield event;
        }
        let end = final_event.ok_or(AiError::InvalidResponse)?;
        let mut response = response.ok_or(AiError::InvalidResponse)?;
        response.usage = usage.unwrap_or_default();
        if structured {
            response = StructuredAgentResponse::parse(response)?.response;
        }
        persist(response).await?;
        yield end;
    };
    Box::pin(events)
}
