mod messages;
mod responses;
mod tools;

use openrust_ai::TextEndpoint;
use serde_json::{Map, Value};

#[derive(Clone, Copy)]
pub(super) enum Bridge {
    ResponsesToMessages,
    MessagesToResponses,
}

impl Bridge {
    pub(super) fn between(inbound: TextEndpoint, upstream: TextEndpoint) -> Option<Self> {
        match (inbound, upstream) {
            (TextEndpoint::Responses, TextEndpoint::Messages) => Some(Self::ResponsesToMessages),
            (TextEndpoint::Messages, TextEndpoint::Responses) => Some(Self::MessagesToResponses),
            _ => None,
        }
    }

    pub(super) fn request(
        self,
        data: Map<String, Value>,
    ) -> Result<(Value, Translation), &'static str> {
        let response_fields = match self {
            Self::ResponsesToMessages
                if data
                    .get("tools")
                    .and_then(Value::as_array)
                    .is_some_and(|tools| !tools.is_empty()) =>
            {
                Some(Map::from_iter([
                    ("tools".into(), data["tools"].clone()),
                    (
                        "tool_choice".into(),
                        data.get("tool_choice")
                            .filter(|value| !value.is_null())
                            .cloned()
                            .unwrap_or_else(|| "auto".into()),
                    ),
                    (
                        "parallel_tool_calls".into(),
                        data.get("parallel_tool_calls")
                            .filter(|value| !value.is_null())
                            .cloned()
                            .unwrap_or(Value::Bool(true)),
                    ),
                ]))
            }
            _ => None,
        };
        let request = match self {
            Self::ResponsesToMessages => responses::to_messages(data)?,
            Self::MessagesToResponses => messages::to_responses(data)?,
        };
        Ok((
            request,
            Translation {
                bridge: self,
                response_fields,
            },
        ))
    }
}

pub(super) struct Translation {
    bridge: Bridge,
    response_fields: Option<Map<String, Value>>,
}

impl Translation {
    pub(super) fn response(self, name: String, data: &Value) -> Option<Value> {
        let mut response = match self.bridge {
            Bridge::ResponsesToMessages => responses::Response::new(name).buffered(data)?,
            Bridge::MessagesToResponses => messages::Message::new(name).buffered(data)?,
        };
        if let Some(fields) = self.response_fields {
            response.as_object_mut()?.extend(fields);
        }
        Some(response)
    }

    pub(super) fn stream(self, name: String) -> Stream {
        match self.bridge {
            Bridge::ResponsesToMessages => Stream::Responses(
                responses::Response::new(name),
                responses::StreamState::default(),
            ),
            Bridge::MessagesToResponses => Stream::Messages(
                messages::Message::new(name),
                messages::StreamState::default(),
            ),
        }
    }
}

pub(super) enum Stream {
    Responses(responses::Response, responses::StreamState),
    Messages(messages::Message, messages::StreamState),
}

impl Stream {
    pub(super) fn convert(&mut self, data: &Value) -> Option<(Vec<String>, bool)> {
        match self {
            Self::Responses(response, state) => state.convert(response, data),
            Self::Messages(message, state) => state.convert(message, data),
        }
    }
}
