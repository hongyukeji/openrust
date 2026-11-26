use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Describes the application execution that emitted a captured message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Console(Console),
    Http(Http),
    Queue(Queue),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Console {
    pub command: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Http {
    pub method: String,
    pub path: String,
    #[serde(rename = "auth_id", default, deserialize_with = "identifier")]
    pub auth_id: Option<String>,
    #[serde(rename = "auth_email")]
    pub auth_email: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Queue {
    pub queue: String,
    pub job: String,
    pub command: Option<String>,
}

macro_rules! from_array {
    ($($kind:ty),+ $(,)?) => {$(
        impl $kind {
            pub fn from_array(value: &Value) -> Result<Self, serde_json::Error> {
                serde_json::from_value(value.clone())
            }
        }
    )+};
}
from_array!(Console, Http, Queue);

impl Origin {
    pub fn from_array(value: &Value) -> Result<Self, serde_json::Error> {
        match value.get("type").and_then(Value::as_str) {
            Some("console") => Console::from_array(value).map(Self::Console),
            Some("queue") => Queue::from_array(value).map(Self::Queue),
            _ => Http::from_array(value).map(Self::Http),
        }
    }
}

pub(super) fn identifier<'de, D: Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(input)? {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        Value::Number(value) => Ok(Some(value.to_string())),
        Value::Bool(value) => Ok(Some(if value { "1" } else { "" }.into())),
        _ => Err(serde::de::Error::custom(
            "authentication ID must be scalar or null",
        )),
    }
}
