use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use futures_util::StreamExt;
use openrust::{
    foundation::Application,
    support::facades::{App, Facade},
};

use crate::{
    AgentResponse, AiError, FinishReason, PendingToolApprovals, Step, Usage, events::StreamAttempt,
};

use super::{AgentEventStream, StreamEvent, StreamEventData, UrlCitation};

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
    pub fn citations(&self) -> impl Iterator<Item = &UrlCitation> {
        self.events.iter().filter_map(|event| match &event.data {
            StreamEventData::Citation { citation, .. } => Some(citation),
            _ => None,
        })
    }

    pub fn reasoning(&self) -> String {
        collect_reasoning(&self.events)
    }

    pub(super) fn from_events(
        events: Vec<StreamEvent>,
        conversation_id: Option<String>,
        metadata: Option<&StreamMetadata>,
    ) -> Option<Self> {
        let invocation_id = events.first()?.invocation_id.clone();
        let (conversation_id, pending_approvals) = match &events.last()?.data {
            StreamEventData::StreamEnd { .. } => (conversation_id, None),
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
        let mut response = AgentResponse::new("", Usage::default(), "", "");
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
                StreamEventData::Citation { citation, .. } => {
                    response.citations.push(citation.clone())
                }
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
        response.reasoning = collect_reasoning(&events);
        response.usage = usage.unwrap_or_default();
        response.steps = collect_steps(&events);
        if let Some(metadata) = metadata {
            metadata.apply(&mut response.steps);
            if let Some((first, remaining)) = response.steps.split_first() {
                response
                    .model
                    .clone_from(&remaining.last().unwrap_or(first).model);
                response.usage = remaining.iter().fold(first.usage.clone(), |total, step| {
                    step.usage.clone().combine(&total)
                });
            }
        }
        Some(Self {
            invocation_id,
            response,
            events,
            conversation_id,
            pending_approvals,
        })
    }
}

/// 供完成回调读取上游终帧的元数据，流的对外事件仍按原协议交付。
#[derive(Clone, Default)]
pub(crate) struct StreamMetadata(Arc<Mutex<Vec<StepMetadata>>>);

struct StepMetadata {
    model: Option<String>,
    usage: Usage,
    finish_reason: FinishReason,
}

impl StreamMetadata {
    pub(crate) fn record(&self, model: Option<String>, usage: Usage, finish_reason: FinishReason) {
        self.0
            .lock()
            .expect("模型步骤元数据锁中毒")
            .push(StepMetadata {
                model,
                usage,
                finish_reason,
            });
    }

    pub(super) fn apply(&self, steps: &mut [Step]) {
        let observed = self.0.lock().expect("模型步骤元数据锁中毒");
        for (step, meta) in steps.iter_mut().zip(observed.iter()) {
            if let Some(model) = &meta.model
                && !model.trim().is_empty()
            {
                step.model.clone_from(model);
            }
            step.usage.clone_from(&meta.usage);
            if meta.finish_reason != FinishReason::Unknown {
                step.finish_reason = meta.finish_reason;
            }
        }
    }
}

fn collect_steps(events: &[StreamEvent]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut current = None;
    for event in events {
        if matches!(event.data, StreamEventData::StreamStart { .. }) {
            current = Some(StreamStep::default());
        }
        if let Some(step) = current.as_mut()
            && step.record(&event.data)
            && let Some(step) = current.take()
        {
            steps.push(step.finish());
        }
    }
    steps
}

#[derive(Default)]
pub(super) struct StreamStep {
    step: Step,
    reasoning: ReasoningBlocks,
}

impl StreamStep {
    pub(super) fn record(&mut self, data: &StreamEventData) -> bool {
        match data {
            StreamEventData::StreamStart { provider, model } => {
                self.step.provider.clone_from(provider);
                self.step.model.clone_from(model);
            }
            StreamEventData::TextDelta { delta, .. } => self.step.text.push_str(delta),
            StreamEventData::Citation { citation, .. } => {
                self.step.citations.push(citation.clone())
            }
            StreamEventData::ToolCall { call } => self.step.tool_calls.push(call.clone()),
            StreamEventData::ToolResult { result } => self.step.tool_results.push(result.clone()),
            StreamEventData::StreamEnd { reason, usage } => {
                self.step.usage.clone_from(usage);
                self.step.finish_reason = FinishReason::from_wire(Some(reason));
                return true;
            }
            StreamEventData::ToolApprovalRequest { .. } => {
                self.step.finish_reason = FinishReason::ToolCalls;
                return true;
            }
            _ => self.reasoning.record(data),
        }
        false
    }

    pub(super) fn finish(mut self) -> Step {
        self.step.reasoning = self.reasoning.finish();
        self.step
    }
}

fn collect_reasoning(events: &[StreamEvent]) -> String {
    let mut reasoning = ReasoningBlocks::default();
    for event in events {
        reasoning.record(&event.data);
    }
    reasoning.finish()
}

#[derive(Default)]
struct ReasoningBlocks {
    indices: BTreeMap<String, usize>,
    blocks: Vec<String>,
}

impl ReasoningBlocks {
    fn record(&mut self, data: &StreamEventData) {
        if let StreamEventData::ReasoningDelta {
            reasoning_id,
            delta,
        } = data
        {
            let index = match self.indices.get(reasoning_id) {
                Some(index) => *index,
                None => {
                    let index = self.blocks.len();
                    self.blocks.push(String::new());
                    self.indices.insert(reasoning_id.clone(), index);
                    index
                }
            };
            self.blocks[index].push_str(delta);
        }
    }

    fn finish(mut self) -> String {
        self.blocks.retain(|block| !block.trim().is_empty());
        self.blocks.join("\n\n")
    }
}

type Callback = Box<dyn FnOnce(&StreamedAgentResponse) + Send>;

#[derive(Default)]
pub(super) struct CompletionCallbacks {
    app: Option<Arc<Application>>,
    callbacks: Vec<Callback>,
    completed: Option<StreamAttempt>,
    failed: Option<StreamAttempt>,
}

impl CompletionCallbacks {
    pub(super) fn push(&mut self, callback: impl FnOnce(&StreamedAgentResponse) + Send + 'static) {
        if self.app.is_none() {
            self.app = App::get_facade_root().ok();
        }
        self.callbacks.push(Box::new(callback));
    }

    pub(super) fn observe(
        &mut self,
        attempt: StreamAttempt,
        app: Arc<Application>,
        completed: bool,
        failed: bool,
    ) {
        self.app = Some(app);
        self.completed = completed.then(|| attempt.clone());
        self.failed = failed.then_some(attempt);
    }

    fn retains_events(&self) -> bool {
        !self.callbacks.is_empty() || self.completed.is_some()
    }

    pub(super) fn is_empty(&self) -> bool {
        !self.retains_events() && self.failed.is_none()
    }

    pub(super) async fn notify(
        self,
        events: Vec<StreamEvent>,
        conversation_id: Option<String>,
        metadata: Option<&StreamMetadata>,
    ) {
        if !self.retains_events() {
            return;
        }
        let Some(response) = StreamedAgentResponse::from_events(events, conversation_id, metadata)
        else {
            return;
        };
        let notify = async move {
            let response = Arc::new(response);
            if let Some(attempt) = self.completed {
                attempt.complete(response.clone()).await;
            }
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

    pub(super) async fn fail(self, error: &AiError) {
        if let (Some(attempt), Some(app)) = (self.failed, self.app) {
            app.scope(attempt.fail(error)).await;
        }
    }

    pub(super) fn stream(
        self,
        mut source: AgentEventStream,
        conversation_id: Option<String>,
        metadata: Option<StreamMetadata>,
    ) -> AgentEventStream {
        Box::pin(async_stream::stream! {
            let mut completion = Some(self);
            let mut events = Vec::new();
            while let Some(result) = source.next().await {
                match &result {
                    Ok(event) if completion.as_ref().is_some_and(Self::retains_events) => {
                        events.push(event.clone());
                        // UI 适配器将审批请求当成终帧，发送后结束正文。
                        if matches!(event.data, StreamEventData::ToolApprovalRequest { .. })
                            && let Some(callbacks) = completion.take()
                        {
                            callbacks.notify(std::mem::take(&mut events), conversation_id.clone(), metadata.as_ref()).await;
                        }
                    },
                    Err(error) => {
                        if let Some(callbacks) = completion.take() {
                            events.clear();
                            callbacks.fail(error).await;
                        }
                    },
                    _ => {}
                }
                yield result;
            }
            if let Some(callbacks) = completion {
                callbacks.notify(events, conversation_id, metadata.as_ref()).await;
            }
        })
    }
}
