use std::future::Future;

use futures_util::StreamExt;

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
    F: FnOnce(AgentResponse) -> Fut + Send + 'static,
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
    F: FnOnce(AgentResponse) -> Fut + Send + 'static,
    Fut: Future<Output = AiResult<()>> + Send + 'static,
{
    let events = async_stream::try_stream! {
        let mut final_event = None;
        let mut response = StreamResponse::default();
        let mut started = false;
        while let Some(event) = source.next().await {
            let event = event?;
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
        }
        let end = final_event.ok_or(AiError::InvalidResponse)?;
        let mut response = response.finish(metadata.as_ref());
        if structured {
            if let Some(last) = response.steps.last() {
                response.text.clone_from(&last.text);
            }
            response = StructuredAgentResponse::parse(response)?.response;
        }
        persist(response).await?;
        yield end;
    };
    Box::pin(events)
}
