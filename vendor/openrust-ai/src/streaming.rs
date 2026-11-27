use std::{
    collections::{BTreeMap, HashSet},
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};

use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt};
use openrust::{
    http::{Response, StatusCode, client::ResponseStream},
    support::{
        Str,
        facades::{App, Facade},
    },
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    AgentResponse, AiError, AiResult, TextEndpoint, ToolCall, ToolResult, Usage,
    agent_broadcast::StreamBroadcast, text::transport_error,
};

mod citations;
mod completion;
mod protocols;
mod reasoning;
pub use citations::UrlCitation;
pub use completion::StreamedAgentResponse;
use completion::{CompletionCallbacks, StreamStep};
pub(crate) use completion::{StreamMetadata, StreamResponse};
pub use protocols::{AgentUserInteractionProtocol, StreamProtocol, VercelDataProtocol};
pub use reasoning::ReasoningPart;

pub type AgentEventStream = Pin<Box<dyn Stream<Item = AiResult<StreamEvent>> + Send>>;

#[derive(Clone, Debug, Serialize)]
pub struct StreamEvent {
    pub id: String,
    pub invocation_id: String,
    pub timestamp: u64,
    #[serde(flatten)]
    pub data: StreamEventData,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEventData {
    StreamStart {
        provider: String,
        model: String,
    },
    TextStart {
        message_id: String,
    },
    TextDelta {
        message_id: String,
        delta: String,
    },
    TextEnd {
        message_id: String,
    },
    Citation {
        message_id: String,
        #[serde(serialize_with = "citations::serialize_event")]
        citation: UrlCitation,
    },
    ReasoningStart {
        reasoning_id: String,
    },
    ReasoningDelta {
        reasoning_id: String,
        delta: String,
    },
    ReasoningEnd {
        reasoning_id: String,
    },
    ToolCall {
        call: ToolCall,
    },
    ToolResult {
        result: ToolResult,
    },
    ToolApprovalRequest {
        conversation_id: String,
        approval_id: String,
        approvals: Vec<crate::PendingApproval>,
    },
    StreamEnd {
        reason: String,
        usage: Usage,
    },
}

pub struct StreamableAgentResponse {
    events: AgentEventStream,
    inspected: Vec<StreamEvent>,
    inspection_complete: bool,
    completion: CompletionCallbacks,
    conversation_id: Option<String>,
    metadata: Option<StreamMetadata>,
    protocol: Option<Box<dyn StreamProtocol>>,
}

fn replay_events(inspected: Vec<StreamEvent>, events: AgentEventStream) -> AgentEventStream {
    if inspected.is_empty() {
        events
    } else {
        Box::pin(futures_util::stream::iter(inspected.into_iter().map(Ok)).chain(events))
    }
}

impl StreamableAgentResponse {
    pub(crate) fn from_events(events: AgentEventStream) -> Self {
        Self {
            events,
            inspected: Vec::new(),
            inspection_complete: false,
            completion: CompletionCallbacks::default(),
            conversation_id: None,
            metadata: None,
            protocol: None,
        }
    }

    pub(crate) async fn probe(mut self) -> AiResult<Self> {
        if let Some(event) = self.events.next().await {
            self.inspected.push(event?);
        }
        Ok(self)
    }

    pub(crate) fn with_step_metadata(mut self, metadata: StreamMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub(crate) fn step_metadata(&self) -> Option<StreamMetadata> {
        self.metadata.clone()
    }

    pub(crate) fn observe_step(mut self, observation: crate::events::StepObservation) -> Self {
        let Ok(app) = App::get_facade_root() else {
            return self;
        };
        if !observation.wants_result() {
            return self;
        }
        let mut upstream = self.events;
        let metadata = self.metadata.clone();
        self.events = Box::pin(async_stream::stream! {
            let mut step = observation.wants_response().then(StreamStep::default);
            let mut observation = Some(observation);
            while let Some(result) = upstream.next().await {
                match &result {
                    Ok(event) => {
                        if let Some(step) = &mut step {
                            step.record(&event.data);
                        }
                        if matches!(event.data, StreamEventData::StreamEnd { .. })
                            && let Some(observation) = observation.take()
                            && let Some(step) = step.take()
                        {
                            let mut step = step.finish();
                            if let Some(metadata) = &metadata {
                                metadata.apply(std::slice::from_mut(&mut step));
                            }
                            app.scope(observation.complete(|_| step)).await;
                        }
                    }
                    Err(error) => {
                        if let Some(observation) = observation.take() {
                            app.scope(observation.fail(error)).await;
                        }
                    }
                }
                yield result;
            }
            if let Some(observation) = observation {
                app.scope(observation.fail(&AiError::InvalidResponse)).await;
            }
        });
        self
    }

    pub(crate) fn observe_attempt(mut self, attempt: Option<crate::events::StreamAttempt>) -> Self {
        use crate::events::{AgentFailed, AgentStreamed};
        use openrust::support::facades::Event;

        let Some(attempt) = attempt else {
            return self;
        };
        let (Ok(app), Ok(dispatcher)) = (App::get_facade_root(), Event::dispatcher()) else {
            return self;
        };
        self.completion.observe(
            attempt,
            app,
            dispatcher.observes(AgentStreamed::NAME),
            dispatcher.observes(AgentFailed::NAME),
        );
        self
    }

    pub(crate) fn within_conversation(
        mut self,
        conversation_id: String,
        wrap: impl FnOnce(AgentEventStream) -> AgentEventStream,
    ) -> Self {
        self.conversation_id = Some(conversation_id);
        self.events = wrap(replay_events(
            std::mem::take(&mut self.inspected),
            self.events,
        ));
        self
    }

    pub(crate) fn broadcasting(mut self, broadcast: StreamBroadcast) -> AiResult<Self> {
        let app = App::get_facade_root().map_err(|_| AiError::Unavailable)?;
        let mut events = self.events;
        self.events = Box::pin(async_stream::try_stream! {
            while let Some(event) = events.next().await {
                let event = event?;
                // HTTP 正文可能在路由请求作用域退出后才开始读取。
                app.scope(broadcast.publish(&event)).await;
                yield event;
            }
        });
        Ok(self)
    }

    pub fn into_stream(self) -> AgentEventStream {
        let events = replay_events(self.inspected, self.events);
        if self.completion.is_empty() {
            events
        } else {
            self.completion
                .stream(events, self.conversation_id, self.metadata)
        }
    }

    /// Visits each event and returns the response for further use. Returning false pauses the visit.
    pub async fn each(mut self, mut callback: impl FnMut(&StreamEvent) -> bool) -> AiResult<Self> {
        for event in &self.inspected {
            if !callback(event) {
                return Ok(self);
            }
        }
        while let Some(event) = self.events.next().await {
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    std::mem::take(&mut self.completion).fail(&error).await;
                    return Err(error);
                }
            };
            let keep_reading = callback(&event);
            self.inspected.push(event);
            if !keep_reading {
                return Ok(self);
            }
        }
        self.events = Box::pin(futures_util::stream::empty());
        self.inspection_complete = true;
        if !self.completion.is_empty() {
            std::mem::take(&mut self.completion)
                .notify(
                    self.inspected.clone(),
                    self.conversation_id.clone(),
                    self.metadata.as_ref(),
                )
                .await;
        }
        Ok(self)
    }

    /// Registers a callback for a stream's terminal result.
    pub fn then(mut self, callback: impl FnOnce(&StreamedAgentResponse) + Send + 'static) -> Self {
        if self.inspection_complete {
            if let Some(response) = StreamedAgentResponse::from_events(
                self.inspected.clone(),
                self.conversation_id.clone(),
                self.metadata.as_ref(),
            ) {
                callback(&response);
            }
        } else {
            self.completion.push(callback);
        }
        self
    }

    /// Registers a callback for the first error reported while consuming the stream.
    pub fn catch(mut self, callback: impl FnOnce(&AiError) + Send + 'static) -> Self {
        let app = App::get_facade_root().ok();
        let mut source = self.events;
        self.events = Box::pin(async_stream::stream! {
            let mut callback = Some(callback);
            while let Some(event) = source.next().await {
                if let Err(error) = &event
                    && let Some(callback) = callback.take()
                {
                    // HTTP 客户端读取正文时，路由的应用作用域可能已经退出。
                    if let Some(app) = &app {
                        app.scope(async { callback(error) }).await;
                    } else {
                        callback(error);
                    }
                }
                yield event;
            }
        });
        self
    }

    pub fn using_protocol(mut self, protocol: impl StreamProtocol + 'static) -> Self {
        self.protocol = Some(Box::new(protocol));
        self
    }

    pub fn using_vercel_data_protocol(self, message_id: Option<String>) -> Self {
        self.using_protocol(VercelDataProtocol::new(message_id))
    }

    pub fn using_agent_user_interaction_protocol(
        self,
        thread_id: Option<String>,
        run_id: Option<String>,
    ) -> Self {
        self.using_protocol(AgentUserInteractionProtocol::new(thread_id, run_id))
    }

    /// 以 Laravel 默认的 SSE 数据帧返回 Agent 事件。
    pub fn to_response(mut self) -> Response {
        let protocol = self.protocol.take();
        let mut events = self.into_stream();
        if let Some(protocol) = protocol {
            return protocol.response(events);
        }
        let body = async_stream::try_stream! {
            while let Some(event) = events.next().await {
                let serialized = serde_json::to_string(&event?).map_err(|_| AiError::InvalidResponse)?;
                yield format!("data: {serialized}\n\n");
            }
            yield "data: [DONE]\n\n".to_owned();
        };
        Response::stream::<String, AiError>(StatusCode::OK, body).with_headers(protocols::headers())
    }

    pub(crate) fn from_text(response: AgentResponse) -> Self {
        let invocation =
            crate::request_context::invocation_id().unwrap_or_else(|| Str::ulid().to_string());
        let steps = if response.steps.is_empty() {
            vec![response.public_step(crate::FinishReason::Stop)]
        } else {
            response.steps
        };
        let events = async_stream::stream! {
            for step in steps {
                yield Ok(event(&invocation, StreamEventData::StreamStart {
                    provider: step.provider,
                    model: step.model,
                }));
                if !step.reasoning.is_empty() {
                    let reasoning_id = Str::ulid().to_string();
                    yield Ok(event(&invocation, StreamEventData::ReasoningStart { reasoning_id: reasoning_id.clone() }));
                    yield Ok(event(&invocation, StreamEventData::ReasoningDelta {
                        reasoning_id: reasoning_id.clone(), delta: step.reasoning,
                    }));
                    yield Ok(event(&invocation, StreamEventData::ReasoningEnd { reasoning_id }));
                }
                let message_id = Str::ulid().to_string();
                yield Ok(event(&invocation, StreamEventData::TextStart { message_id:message_id.clone() }));
                yield Ok(event(&invocation, StreamEventData::TextDelta { message_id:message_id.clone(), delta:step.text }));
                for citation in step.citations {
                    yield Ok(event(&invocation, StreamEventData::Citation { message_id:message_id.clone(), citation }));
                }
                yield Ok(event(&invocation, StreamEventData::TextEnd { message_id }));
                for call in step.tool_calls {
                    yield Ok(event(&invocation, StreamEventData::ToolCall { call }));
                }
                for result in step.tool_results {
                    yield Ok(event(&invocation, StreamEventData::ToolResult { result }));
                }
                yield Ok(event(&invocation, StreamEventData::StreamEnd {
                    reason:step.finish_reason.as_str().into(),
                    usage:step.usage,
                }));
            }
        };
        Self::from_events(Box::pin(events))
    }

    pub(crate) fn from_upstream(
        protocol: TextEndpoint,
        stream: ResponseStream,
        provider: String,
        requested_model: String,
        xai: bool,
    ) -> Self {
        let invocation =
            crate::request_context::invocation_id().unwrap_or_else(|| Str::ulid().to_string());
        let metadata = StreamMetadata::default();
        let observed = metadata.clone();
        let events = async_stream::try_stream! {
            let mut source = stream.eventsource();
            let mut messages = TextMessages::default();
            let mut reasoning = TextMessages::default();
            let mut cited_urls = HashSet::new();
            let mut started = false;
            let mut resolved_model = None;
            let mut reason = None;
            let mut usage = Usage::default();
            while let Some(frame) = source.next().await {
                let data = frame.map_err(|error| match error {
                    EventStreamError::Transport(error) => transport_error(error),
                    _ => AiError::InvalidResponse,
                })?.data;
                let part = protocol.decode(&data, xai)?;
                if let Some(model) = part.model {
                    resolved_model = Some(model);
                }
                if !started {
                    started = true;
                    yield event(&invocation, StreamEventData::StreamStart {
                        provider: provider.clone(),
                        model: resolved_model.clone().unwrap_or_else(|| requested_model.clone()),
                    });
                }
                if let Some(delta) = part.delta.filter(|delta| !delta.is_empty()) {
                    for data in messages.delta(part.output_part.as_ref(), delta).into_iter().flatten() {
                        yield event(&invocation, data);
                    }
                }
                for citation in part.citations {
                    if !matches!(protocol, TextEndpoint::Messages | TextEndpoint::GeminiStreamGenerateContent)
                        || cited_urls.insert(citation.url.clone())
                    {
                        yield event(&invocation, messages.citation(part.output_part.as_ref(), citation));
                    }
                }
                if let Some(part) = part.reasoning {
                    for data in reasoning.reasoning(part).into_iter().flatten() {
                        yield event(&invocation, data);
                    }
                }
                if part.end_message && let Some(message_id) = messages.close(part.output_part.as_ref()) {
                    yield event(&invocation, StreamEventData::TextEnd { message_id });
                }
                if let Some(update) = part.usage {
                    usage.update(update);
                }
                if let Some(update) = part.reason {
                    reason = Some(update);
                }
                if part.done {
                    break;
                }
            }
            let reason = reason.ok_or(AiError::InvalidResponse)?;
            for reasoning_id in reasoning.open_ids() {
                yield event(&invocation, StreamEventData::ReasoningEnd { reasoning_id });
            }
            for message_id in messages.open_ids() {
                yield event(&invocation, StreamEventData::TextEnd { message_id });
            }
            let usage = match protocol {
                TextEndpoint::Messages => usage.include_cached_input(),
                TextEndpoint::GeminiStreamGenerateContent => usage.include_reasoning_output(),
                _ => usage,
            };
            observed.record(resolved_model, usage.clone(), crate::FinishReason::from_wire(Some(&reason)), None);
            yield event(&invocation, StreamEventData::StreamEnd { reason, usage });
        };
        Self::from_events(Box::pin(events)).with_step_metadata(metadata)
    }
}

pub(crate) fn is_event_stream(response: &openrust::http::client::Response) -> bool {
    response
        .header("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

pub(crate) fn event(invocation: &str, data: StreamEventData) -> StreamEvent {
    StreamEvent {
        id: Str::ulid().to_string(),
        invocation_id: invocation.into(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        data,
    }
}

type OutputPart = (String, u64);

pub(crate) fn output_part(value: &Value) -> Option<OutputPart> {
    value["item_id"]
        .as_str()
        .zip(value["content_index"].as_u64())
        .map(|(item, index)| (item.to_owned(), index))
}

#[derive(Default)]
pub(crate) struct TextMessages {
    indexed: BTreeMap<OutputPart, TextMessage>,
    anonymous: Option<TextMessage>,
    last_anonymous_id: Option<String>,
}

struct TextMessage {
    id: String,
    open: bool,
}

impl TextMessages {
    pub(crate) fn delta(
        &mut self,
        part: Option<&OutputPart>,
        delta: String,
    ) -> [Option<StreamEventData>; 2] {
        let (message_id, starts) = self.open(part);
        let start = starts.then(|| StreamEventData::TextStart {
            message_id: message_id.clone(),
        });
        [
            start,
            Some(StreamEventData::TextDelta { message_id, delta }),
        ]
    }

    fn open(&mut self, part: Option<&OutputPart>) -> (String, bool) {
        let message = self.get(part);
        let starts = !message.open;
        message.open = true;
        (message.id.clone(), starts)
    }

    pub(crate) fn citation(
        &mut self,
        part: Option<&OutputPart>,
        citation: UrlCitation,
    ) -> StreamEventData {
        let message_id = if part.is_none()
            && self.anonymous.is_none()
            && let Some(previous) = &self.last_anonymous_id
        {
            previous.clone()
        } else {
            self.get(part).id.clone()
        };
        StreamEventData::Citation {
            message_id,
            citation,
        }
    }

    fn get(&mut self, part: Option<&OutputPart>) -> &mut TextMessage {
        let create = || TextMessage {
            id: Str::ulid().to_string(),
            open: false,
        };
        match part {
            Some(part) => self.indexed.entry(part.clone()).or_insert_with(create),
            None => self.anonymous.get_or_insert_with(create),
        }
    }

    pub(crate) fn close(&mut self, part: Option<&OutputPart>) -> Option<String> {
        match part {
            Some(part) => {
                let message = self.indexed.get_mut(part)?;
                std::mem::take(&mut message.open).then(|| message.id.clone())
            }
            None => {
                if !self.anonymous.as_ref().is_some_and(|message| message.open) {
                    return None;
                }
                let message = self.anonymous.take()?;
                self.last_anonymous_id = Some(message.id.clone());
                Some(message.id)
            }
        }
    }

    pub(crate) fn open_ids(self) -> impl Iterator<Item = String> {
        self.anonymous
            .into_iter()
            .chain(self.indexed.into_values())
            .filter(|message| message.open)
            .map(|message| message.id)
    }
}

#[derive(Default)]
struct Part {
    model: Option<String>,
    delta: Option<String>,
    output_part: Option<OutputPart>,
    citations: Vec<UrlCitation>,
    reasoning: Option<ReasoningPart>,
    usage: Option<Usage>,
    reason: Option<String>,
    end_message: bool,
    done: bool,
}

impl TextEndpoint {
    fn decode(self, data: &str, xai: bool) -> AiResult<Part> {
        if data == "[DONE]" && matches!(self, Self::ChatCompletions) {
            return Ok(Part {
                done: true,
                ..Default::default()
            });
        }
        let value: Value = serde_json::from_str(data).map_err(|_| AiError::InvalidResponse)?;
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Err(AiError::InvalidResponse);
        }
        match self {
            Self::ChatCompletions => {
                let usage = &value["usage"];
                let choice = &value["choices"][0];
                if choice["delta"]["refusal"]
                    .as_str()
                    .is_some_and(|refusal| !refusal.is_empty())
                {
                    return Err(AiError::Refused);
                }
                Ok(Part {
                    model: string(&value["model"]),
                    delta: string(&choice["delta"]["content"]),
                    reason: string(&choice["finish_reason"]),
                    usage: usage.is_object().then(|| Usage::from_openai(usage)),
                    ..Default::default()
                })
            }
            Self::Responses => {
                let response = &value["response"];
                let usage = &response["usage"];
                let model = string(&response["model"]);
                let output_part = output_part(&value);
                match value["type"].as_str() {
                    Some("response.output_text.delta") => Ok(Part {
                        delta: string(&value["delta"]),
                        output_part,
                        ..Default::default()
                    }),
                    Some("response.output_text.done") => Ok(Part {
                        end_message: true,
                        output_part,
                        ..Default::default()
                    }),
                    Some("response.output_text.annotation.added") => Ok(Part {
                        output_part,
                        citations: UrlCitation::from_annotation(&value["annotation"])
                            .into_iter()
                            .collect(),
                        ..Default::default()
                    }),
                    Some("response.refusal.delta" | "response.refusal.done") => {
                        Err(AiError::Refused)
                    }
                    Some("response.failed" | "error") => Err(AiError::InvalidResponse),
                    Some("response.completed" | "response.incomplete") => Ok(Part {
                        model,
                        reason: Some(
                            response["incomplete_details"]["reason"]
                                .as_str()
                                .unwrap_or("stop")
                                .into(),
                        ),
                        usage: usage.is_object().then(|| {
                            if xai {
                                Usage::from_xai_responses(usage)
                            } else {
                                Usage::from_openai(usage)
                            }
                        }),
                        done: true,
                        ..Default::default()
                    }),
                    _ => Ok(Part {
                        model,
                        reasoning: ReasoningPart::from_openai_frame(&value),
                        ..Default::default()
                    }),
                }
            }
            Self::Messages => {
                let message = &value["message"];
                let usage = value.get("usage").or_else(|| message.get("usage"));
                Ok(Part {
                    model: string(&message["model"]),
                    delta: match value["type"].as_str() {
                        Some("content_block_start") if value["content_block"]["type"] == "text" => {
                            string(&value["content_block"]["text"])
                        }
                        Some("content_block_delta") if value["delta"]["type"] == "text_delta" => {
                            string(&value["delta"]["text"])
                        }
                        _ => None,
                    },
                    reason: match value["delta"]["stop_reason"].as_str() {
                        Some("end_turn" | "stop_sequence") => Some("stop".into()),
                        Some("max_tokens" | "model_context_window_exceeded") => {
                            Some("length".into())
                        }
                        Some("refusal") => return Err(AiError::Refused),
                        Some(reason) => Some(reason.into()),
                        None => None,
                    },
                    usage: usage.map(Usage::from_anthropic),
                    citations: crate::anthropic::stream_citations(&value),
                    end_message: value["type"] == "content_block_stop",
                    done: value["type"] == "message_stop",
                    ..Default::default()
                })
            }
            Self::GeminiStreamGenerateContent => {
                let candidate = &value["candidates"][0];
                let reason = crate::gemini::finish_reason(&value, candidate)?;
                let delta: String = crate::gemini::text_parts(candidate).collect();
                Ok(Part {
                    model: string(&value["modelVersion"]),
                    delta: (!delta.is_empty()).then_some(delta),
                    reason,
                    usage: value.get("usageMetadata").map(Usage::from_gemini),
                    citations: crate::gemini::citations(candidate),
                    ..Default::default()
                })
            }
            Self::CountResponseTokens
            | Self::CountMessageTokens
            | Self::GeminiCountTokens
            | Self::GeminiGenerateContent
            | Self::Embeddings
            | Self::GeminiEmbedContent
            | Self::GeminiBatchEmbedContents
            | Self::GeminiBatchGenerateContent
            | Self::GeminiAsyncBatchEmbedContent
            | Self::ImagesGenerations
            | Self::ImagesEdits => Err(AiError::StreamingNotSupported),
        }
    }
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}
