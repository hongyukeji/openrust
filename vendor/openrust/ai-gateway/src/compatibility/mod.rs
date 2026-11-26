mod messages;
mod responses;

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

    pub(super) fn request(self, data: Map<String, Value>) -> Result<Value, &'static str> {
        match self {
            Self::ResponsesToMessages => responses::to_messages(data),
            Self::MessagesToResponses => messages::to_responses(data),
        }
    }

    pub(super) fn response(self, name: String, data: &Value) -> Option<Value> {
        match self {
            Self::ResponsesToMessages => responses::Response::new(name).buffered(data),
            Self::MessagesToResponses => messages::Message::new(name).buffered(data),
        }
    }

    pub(super) fn stream(self, name: String) -> Stream {
        match self {
            Self::ResponsesToMessages => Stream::Responses(
                responses::Response::new(name),
                responses::StreamState::default(),
            ),
            Self::MessagesToResponses => Stream::Messages(
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
