use serde_json::{Map, Value};

use super::SlackAttachment;

/// legacy Slack webhook message with attachments.
#[derive(Default)]
pub struct SlackMessage {
    fields: Map<String, Value>,
    attachments: Vec<SlackAttachment>,
    content: Option<String>,
    color: Option<&'static str>,
}

impl SlackMessage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn info(mut self) -> Self {
        self.color = None;
        self
    }

    pub fn success(mut self) -> Self {
        self.color = Some("good");
        self
    }

    pub fn warning(mut self) -> Self {
        self.color = Some("warning");
        self
    }

    pub fn error(mut self) -> Self {
        self.color = Some("danger");
        self
    }

    pub fn from(mut self, username: impl Into<String>, icon: Option<&str>) -> Self {
        self.fields
            .insert("username".into(), username.into().into());
        if let Some(icon) = icon {
            self.fields.insert("icon_emoji".into(), icon.into());
        }
        self
    }

    pub fn image(self, url: impl Into<String>) -> Self {
        self.field("icon_url", url.into())
    }

    pub fn to(self, channel: impl Into<String>) -> Self {
        self.field("channel", channel.into())
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn attachment(mut self, build: impl FnOnce(&mut SlackAttachment)) -> Self {
        let mut attachment = SlackAttachment::default();
        build(&mut attachment);
        self.attachments.push(attachment);
        self
    }

    pub fn color(&self) -> Option<&str> {
        self.color
    }

    pub fn link_names(self) -> Self {
        self.field("link_names", 1)
    }

    pub fn unfurl_links(self, enable: bool) -> Self {
        self.field("unfurl_links", enable)
    }

    pub fn unfurl_media(self, enable: bool) -> Self {
        self.field("unfurl_media", enable)
    }

    pub fn to_array(&self) -> Map<String, Value> {
        let mut payload = self.fields.clone();
        payload.retain(|_, value| filled(value));
        payload.insert("text".into(), self.content.as_deref().into());
        payload.insert(
            "attachments".into(),
            self.attachments
                .iter()
                .map(|attachment| Value::Object(attachment.payload(self.color)))
                .collect::<Vec<_>>()
                .into(),
        );
        payload
    }

    fn field(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

// the framework filters top-level optional values and attachment fields with array_filter.
pub(super) fn filled(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty() && value != "0",
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}
