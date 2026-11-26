use std::sync::Arc;

use futures_util::StreamExt;
use openrust::{
    foundation::Application,
    support::facades::{App, Facade},
};

use crate::{AgentResponse, PendingToolApprovals, Usage};

use super::{AgentEventStream, StreamEvent, StreamEventData};

/// 消费完整流后得到的响应与原始事件。等待审批的流包含其审批批次。
#[derive(Clone, Debug)]
pub struct StreamedAgentResponse {
    pub invocation_id: String,
    pub response: AgentResponse,
    pub events: Vec<StreamEvent>,
    pub conversation_id: Option<String>,
    pub pending_approvals: Option<PendingToolApprovals>,
}

impl StreamedAgentResponse {
    pub(super) fn from_events(events: Vec<StreamEvent>) -> Option<Self> {
        let invocation_id = events.first()?.invocation_id.clone();
        let (conversation_id, pending_approvals) = match &events.last()?.data {
            StreamEventData::StreamEnd { .. } => (None, None),
            StreamEventData::ToolApprovalRequest {
                conversation_id,
                approval_id,
                approvals,
            } => (
                Some(conversation_id.clone()),
                Some(PendingToolApprovals {
                    id: approval_id.clone(),
                    approvals: approvals.clone(),
                }),
            ),
            _ => return None,
        };
        let mut response = AgentResponse {
            text: String::new(),
            usage: Usage::default(),
            provider: String::new(),
            model: String::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        };
        let mut usage: Option<Usage> = None;
        let mut texts = Vec::new();
        let mut text = String::new();
        let mut step_has_tool_result = false;
        for event in &events {
            match &event.data {
                StreamEventData::StreamStart { provider, model } => {
                    response.provider.clone_from(provider);
                    response.model.clone_from(model);
                    if step_has_tool_result {
                        texts.push(std::mem::take(&mut text));
                        step_has_tool_result = false;
                    }
                }
                StreamEventData::TextDelta { delta, .. } => text.push_str(delta),
                StreamEventData::ToolCall { call } => response.tool_calls.push(call.clone()),
                StreamEventData::ToolResult { result } => {
                    response.tool_results.push(result.clone());
                    step_has_tool_result = true;
                }
                StreamEventData::StreamEnd { usage: step, .. } => {
                    usage = Some(match usage.as_ref() {
                        Some(previous) => step.clone().combine(previous),
                        None => step.clone(),
                    });
                }
                _ => {}
            }
        }
        texts.push(text);
        texts.retain(|text| !text.trim().is_empty());
        response.text = texts.join("\n\n");
        response.usage = usage.unwrap_or_default();
        Some(Self {
            invocation_id,
            response,
            events,
            conversation_id,
            pending_approvals,
        })
    }
}

type Callback = Box<dyn FnOnce(&StreamedAgentResponse) + Send>;

#[derive(Default)]
pub(super) struct CompletionCallbacks {
    app: Option<Arc<Application>>,
    callbacks: Vec<Callback>,
}

impl CompletionCallbacks {
    pub(super) fn push(&mut self, callback: impl FnOnce(&StreamedAgentResponse) + Send + 'static) {
        if self.callbacks.is_empty() {
            self.app = App::get_facade_root().ok();
        }
        self.callbacks.push(Box::new(callback));
    }

    pub(super) fn is_empty(&self) -> bool {
        self.callbacks.is_empty()
    }

    pub(super) async fn notify(self, events: Vec<StreamEvent>) {
        let Some(response) = StreamedAgentResponse::from_events(events) else {
            return;
        };
        let notify = async move {
            for callback in self.callbacks {
                callback(&response);
            }
        };
        if let Some(app) = self.app {
            app.scope(notify).await;
        } else {
            notify.await;
        }
    }

    pub(super) fn stream(self, mut source: AgentEventStream) -> AgentEventStream {
        Box::pin(async_stream::stream! {
            let mut completion = Some(self);
            let mut events = Vec::new();
            while let Some(result) = source.next().await {
                if completion.is_some() {
                    match &result {
                        Ok(event) => {
                            events.push(event.clone());
                            // UI 适配器将审批请求当成终帧，发送后结束正文。
                            if matches!(event.data, StreamEventData::ToolApprovalRequest { .. })
                                && let Some(callbacks) = completion.take()
                            {
                                callbacks.notify(std::mem::take(&mut events)).await;
                            }
                        }
                        Err(_) => {
                            completion = None;
                            events.clear();
                        }
                    }
                }
                yield result;
            }
            if let Some(callbacks) = completion {
                callbacks.notify(events).await;
            }
        })
    }
}
