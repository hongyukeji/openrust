use std::{
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};

use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt};
use openrust::{
    http::{HeaderMap, Response, StatusCode, client::ResponseStream},
    support::Str,
};
use serde::Serialize;
use serde_json::Value;

use crate::{AgentResponse, AiError, AiResult, TextEndpoint, Usage, text::transport_error};

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
    StreamStart { provider: String, model: String },
    TextStart { message_id: String },
    TextDelta { message_id: String, delta: String },
    TextEnd { message_id: String },
    StreamEnd { reason: String, usage: Usage },
}

pub struct StreamableAgentResponse {
    events: AgentEventStream,
}

impl StreamableAgentResponse {
    pub fn into_stream(self) -> AgentEventStream {
        self.events
    }

    /// 以 Laravel 默认的 SSE 数据帧返回 Agent 事件。
    pub fn to_response(self) -> Response {
        let mut events = self.events;
        let body = async_stream::try_stream! {
            while let Some(event) = events.next().await {
                let serialized = serde_json::to_string(&event?).map_err(|_| AiError::InvalidResponse)?;
                yield format!("data: {serialized}\n\n");
            }
            yield "data: [DONE]\n\n".to_owned();
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            "text/event-stream".parse().expect("valid header"),
        );
        headers.insert(
            "cache-control",
            "no-cache, no-transform".parse().expect("valid header"),
        );
        Response::stream::<String, AiError>(StatusCode::OK, body).with_headers(headers)
    }

    pub(crate) fn from_text(response: AgentResponse) -> Self {
        let invocation = Str::ulid().to_string();
        let message_id = Str::ulid().to_string();
        let events = [
            StreamEventData::StreamStart {
                provider: response.provider,
                model: response.model,
            },
            StreamEventData::TextStart {
                message_id: message_id.clone(),
            },
            StreamEventData::TextDelta {
                message_id: message_id.clone(),
                delta: response.text,
            },
            StreamEventData::TextEnd { message_id },
            StreamEventData::StreamEnd {
                reason: "stop".into(),
                usage: response.usage,
            },
        ];
        Self {
            events: Box::pin(futures_util::stream::iter(
                events.map(|data| Ok(event(&invocation, data))),
            )),
        }
    }

    pub(crate) fn from_upstream(
        protocol: TextEndpoint,
        stream: ResponseStream,
        provider: String,
        requested_model: String,
    ) -> Self {
        let events = async_stream::try_stream! {
            let mut source = stream.eventsource();
            let invocation = Str::ulid().to_string();
            let mut message = None::<String>;
            let mut started = false;
            let mut reason = None;
            let mut usage = Usage::default();
            while let Some(frame) = source.next().await {
                let data = frame.map_err(|error| match error {
                    EventStreamError::Transport(error) => transport_error(error),
                    _ => AiError::InvalidResponse,
                })?.data;
                let part = protocol.decode(&data)?;
                if !started {
                    started = true;
                    yield event(&invocation, StreamEventData::StreamStart {
                        provider: provider.clone(),
                        model: part.model.unwrap_or_else(|| requested_model.clone()),
                    });
                }
                if let Some(delta) = part.delta.filter(|delta| !delta.is_empty()) {
                    let message_id = match &message {
                        Some(message_id) => message_id.clone(),
                        None => {
                            let message_id = Str::ulid().to_string();
                            message = Some(message_id.clone());
                            yield event(&invocation, StreamEventData::TextStart { message_id: message_id.clone() });
                            message_id
                        }
                    };
                    yield event(&invocation, StreamEventData::TextDelta { message_id, delta });
                }
                if part.end_message && let Some(message_id) = message.take() {
                    yield event(&invocation, StreamEventData::TextEnd { message_id });
                }
                if let Some(update) = part.usage {
                    usage.prompt_tokens = update.prompt_tokens.or(usage.prompt_tokens);
                    usage.completion_tokens = update.completion_tokens.or(usage.completion_tokens);
                }
                if let Some(update) = part.reason {
                    reason = Some(update);
                }
                if part.done {
                    break;
                }
            }
            let reason = reason.ok_or(AiError::InvalidResponse)?;
            if let Some(message_id) = message {
                yield event(&invocation, StreamEventData::TextEnd { message_id });
            }
            yield event(&invocation, StreamEventData::StreamEnd { reason, usage });
        };
        Self {
            events: Box::pin(events),
        }
    }
}

pub(crate) fn is_event_stream(response: &openrust::http::client::Response) -> bool {
    response
        .header("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

fn event(invocation: &str, data: StreamEventData) -> StreamEvent {
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

#[derive(Default)]
struct Part {
    model: Option<String>,
    delta: Option<String>,
    usage: Option<Usage>,
    reason: Option<String>,
    end_message: bool,
    done: bool,
}

impl TextEndpoint {
    fn decode(self, data: &str) -> AiResult<Part> {
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
                    usage: usage.is_object().then(|| Usage {
                        prompt_tokens: usage["prompt_tokens"].as_u64(),
                        completion_tokens: usage["completion_tokens"].as_u64(),
                    }),
                    ..Default::default()
                })
            }
            Self::Responses => {
                let response = &value["response"];
                let usage = &response["usage"];
                let model = string(&response["model"]);
                match value["type"].as_str() {
                    Some("response.output_text.delta") => Ok(Part {
                        delta: string(&value["delta"]),
                        ..Default::default()
                    }),
                    Some("response.output_text.done") => Ok(Part {
                        end_message: true,
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
                        usage: usage.is_object().then(|| Usage {
                            prompt_tokens: usage["input_tokens"].as_u64(),
                            completion_tokens: usage["output_tokens"].as_u64(),
                        }),
                        done: true,
                        ..Default::default()
                    }),
                    _ => Ok(Part {
                        model,
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
                    usage: usage.map(|usage| Usage {
                        prompt_tokens: usage["input_tokens"].as_u64(),
                        completion_tokens: usage["output_tokens"].as_u64(),
                    }),
                    end_message: value["type"] == "content_block_stop",
                    done: value["type"] == "message_stop",
                })
            }
            Self::CountMessageTokens => Err(AiError::StreamingNotSupported),
        }
    }
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}
