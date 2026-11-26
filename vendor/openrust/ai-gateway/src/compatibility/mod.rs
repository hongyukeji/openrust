mod chat;
mod chat_messages;
mod chat_tools;
mod messages;
mod messages_chat;
mod responses;
mod tools;

use openrust_ai::TextEndpoint;
use serde_json::{Map, Value};

#[derive(Clone, Copy)]
pub(super) enum Bridge {
    ResponsesToMessages,
    ResponsesToChat,
    ChatToMessages,
    MessagesToResponses,
    MessagesToChat,
}

impl Bridge {
    pub(super) fn between(inbound: TextEndpoint, upstream: TextEndpoint) -> Option<Self> {
        match (inbound, upstream) {
            (TextEndpoint::Responses, TextEndpoint::Messages) => Some(Self::ResponsesToMessages),
            (TextEndpoint::Responses, TextEndpoint::ChatCompletions) => Some(Self::ResponsesToChat),
            (TextEndpoint::ChatCompletions, TextEndpoint::Messages) => Some(Self::ChatToMessages),
            (TextEndpoint::Messages, TextEndpoint::Responses) => Some(Self::MessagesToResponses),
            (TextEndpoint::Messages, TextEndpoint::ChatCompletions) => Some(Self::MessagesToChat),
            _ => None,
        }
    }

    pub(super) fn request(
        self,
        data: Map<String, Value>,
    ) -> Result<(Value, Translation), &'static str> {
        let allows_tools = data
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| !tools.is_empty());
        let mut response_fields = Map::new();
        if allows_tools && matches!(self, Self::ResponsesToMessages | Self::ResponsesToChat) {
            response_fields.extend([
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
            ]);
        }
        if matches!(self, Self::ResponsesToChat)
            && let Some(text) = data.get("text").filter(|text| !text.is_null())
        {
            response_fields.insert("text".into(), text.clone());
        }
        let include_usage =
            matches!(self, Self::ChatToMessages) && chat_messages::include_usage(&data)?;
        let request = match self {
            Self::ResponsesToMessages => responses::to_messages(data)?,
            Self::ResponsesToChat => chat::from_responses(data)?,
            Self::ChatToMessages => chat_messages::to_messages(data)?,
            Self::MessagesToResponses => messages::to_responses(data)?,
            Self::MessagesToChat => messages_chat::request(data)?,
        };
        Ok((
            request,
            Translation {
                bridge: self,
                response_fields: (!response_fields.is_empty()).then_some(response_fields),
                allows_tools,
                include_usage,
            },
        ))
    }
}

pub(super) struct Translation {
    bridge: Bridge,
    response_fields: Option<Map<String, Value>>,
    allows_tools: bool,
    include_usage: bool,
}

impl Translation {
    pub(super) fn response(self, name: String, data: &Value) -> Option<Value> {
        match self.bridge {
            Bridge::ResponsesToMessages => {
                responses::Response::new(name, self.response_fields).buffered(data)
            }
            Bridge::ResponsesToChat => {
                chat::response(name, self.response_fields, self.allows_tools, data)
            }
            Bridge::ChatToMessages => chat_messages::Completion::new(name, self.include_usage)
                .buffered(data, self.allows_tools),
            Bridge::MessagesToResponses => {
                messages::Message::new(name, self.allows_tools).buffered(data)
            }
            Bridge::MessagesToChat => messages_chat::response(name, self.allows_tools, data),
        }
    }

    pub(super) fn stream(self, name: String) -> Stream {
        match self.bridge {
            Bridge::ResponsesToMessages => Stream::Responses(
                Box::new(responses::Response::new(name, self.response_fields)),
                responses::StreamState::default(),
            ),
            Bridge::ResponsesToChat => Stream::Chat(Box::new(chat::StreamState::new(
                name,
                self.response_fields,
                self.allows_tools,
            ))),
            Bridge::ChatToMessages => Stream::ChatClient(Box::new(
                chat_messages::StreamState::new(name, self.include_usage, self.allows_tools),
            )),
            Bridge::MessagesToResponses => Stream::Messages(
                messages::Message::new(name, self.allows_tools),
                messages::StreamState::default(),
            ),
            Bridge::MessagesToChat => Stream::MessagesClient(Box::new(
                messages_chat::StreamState::new(name, self.allows_tools),
            )),
        }
    }
}

pub(super) enum Stream {
    Responses(Box<responses::Response>, responses::StreamState),
    Chat(Box<chat::StreamState>),
    ChatClient(Box<chat_messages::StreamState>),
    Messages(messages::Message, messages::StreamState),
    MessagesClient(Box<messages_chat::StreamState>),
}

impl Stream {
    pub(super) fn convert(&mut self, data: &Value) -> Option<(Vec<String>, bool)> {
        match self {
            Self::Responses(response, state) => state.convert(response, data).map(response_wire),
            Self::Chat(state) => state.convert(data).map(response_wire),
            Self::ChatClient(state) => state.convert(data),
            Self::Messages(message, state) => state.convert(message, data),
            Self::MessagesClient(state) => state.convert(data),
        }
    }
    pub(super) fn finish(&mut self) -> Option<(Vec<String>, bool)> {
        match self {
            Self::Chat(state) => state.finish().map(response_wire),
            Self::MessagesClient(state) => state.finish(),
            _ => None,
        }
    }
}

fn response_wire((frames, terminal): (Vec<responses::Frame>, bool)) -> (Vec<String>, bool) {
    (
        frames.into_iter().map(responses::Frame::wire).collect(),
        terminal,
    )
}
