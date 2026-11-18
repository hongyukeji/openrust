use serde_json::{Map, Value};

/// Structured metadata delivered with a Slack message.
#[derive(Clone)]
pub struct EventMetadata {
    event_type: String,
    payload: Map<String, Value>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Slack event metadata API")]
impl EventMetadata {
    pub fn make(event_type: impl Into<String>, payload: Map<String, Value>) -> Self {
        Self {
            event_type: event_type.into(),
            payload,
        }
    }

    pub fn toArray(&self) -> Map<String, Value> {
        self.clone().into_array()
    }

    pub(super) fn into_array(self) -> Map<String, Value> {
        [
            ("event_type".into(), self.event_type.into()),
            ("event_payload".into(), self.payload.into()),
        ]
        .into_iter()
        .collect()
    }
}
