use std::future::Future;

use futures_util::StreamExt;
use openrust::support::facades::Log;

use crate::{
    AgentEventStream, AgentResponse, AiError, AiResult, StreamEventData, StreamableAgentResponse,
    StructuredAgentResponse,
    streaming::{StreamMetadata, StreamResponse},
};

/// 在确认上游完整结束并写入会话之后，才交付最后一个结束事件。
pub(crate) fn remember<F, Fut>(
    source: StreamableAgentResponse,
    conversation_id: String,
    structured: bool,
    persist: F,
) -> StreamableAgentResponse
where
    F: FnOnce(AgentResponse, Option<AiError>) -> Fut + Send + 'static,
    Fut: Future<Output = AiResult<()>> + Send + 'static,
{
    let metadata = source.step_metadata();
    source.within_conversation(conversation_id, |events| {
        persisted_events(events, metadata, structured, persist)
    })
}

fn persisted_events<F, Fut>(
    mut source: AgentEventStream,
    metadata: Option<StreamMetadata>,
    structured: bool,
    persist: F,
) -> AgentEventStream
where
    F: FnOnce(AgentResponse, Option<AiError>) -> Fut + Send + 'static,
    Fut: Future<Output = AiResult<()>> + Send + 'static,
{
    let events = async_stream::try_stream! {
        let mut final_event = None;
        let mut response = StreamResponse::default();
        let mut started = false;
        let failure = loop {
            let event = match source.next().await {
                None => break None,
                Some(Err(error)) => break Some(error),
                Some(Ok(event)) => event,
            };
            if let Some(previous) = final_event.take() {
                yield previous;
            }
            if !started {
                if !matches!(event.data, StreamEventData::StreamStart { .. }) {
                    Err(AiError::InvalidResponse)?;
                }
                started = true;
            }
            response.record(&event.data);
            match &event.data {
                StreamEventData::ToolApprovalRequest { .. } => {
                    yield event;
                    return;
                }
                StreamEventData::StreamEnd { .. } => {
                    final_event = Some(event);
                    continue;
                }
                _ => {}
            }
            yield event;
        };
        let result = match (failure, final_event) {
            (None, Some(end)) => {
                let mut response = response.finish(metadata.as_ref());
                let stopped = metadata.as_ref().is_some_and(StreamMetadata::stopped);
                if response.steps.is_empty() && stopped {
                    yield end;
                    return;
                }
                if structured && !stopped {
                    if let Some(last) = response.steps.last() {
                        response.text.clone_from(&last.text);
                    }
                    StructuredAgentResponse::parse_with_history(response)
                        .map(|complete| (complete.response, end))
                        .map_err(|response| (AiError::InvalidStructuredResponse, response.into_completed_tool_steps()))
                } else {
                    Ok((response, end))
                }
            }
            (failure, _) => Err((
                failure.unwrap_or(AiError::InvalidResponse),
                response.failed_turn(metadata.as_ref()),
            )),
        };
        match result {
            Ok((response, end)) => {
                persist(response, None).await?;
                yield end;
            }
            Err((error, partial)) => {
                if let Some(partial) = partial
                    && persist(partial, Some(error.clone())).await.is_err()
                {
                    let _ = Log::warning("AI 流式会话的已完成工具步骤保存失败", &Default::default());
                }
                Err(error)?;
            }
        }
    };
    Box::pin(events)
}
