use indexmap::IndexMap;
use jiff::Timestamp;
use serde_json::{Map, Value, json};

use super::slack_message::filled;

/// A legacy Slack attachment with ordered fields and link actions.
#[derive(Default)]
pub struct SlackAttachment {
    attributes: Map<String, Value>,
    fields: IndexMap<FieldKey, SlackAttachmentField>,
    actions: Vec<Value>,
}

#[derive(Eq, Hash, PartialEq)]
enum FieldKey {
    Named(String),
    Custom(usize),
}

impl SlackAttachment {
    pub fn title(&mut self, title: impl Into<String>, url: Option<&str>) -> &mut Self {
        self.attribute("title", title.into());
        self.attribute("title_link", url)
    }

    pub fn pretext(&mut self, text: impl Into<String>) -> &mut Self {
        self.attribute("pretext", text.into())
    }

    pub fn content(&mut self, text: impl Into<String>) -> &mut Self {
        self.attribute("text", text.into())
    }

    pub fn fallback(&mut self, text: impl Into<String>) -> &mut Self {
        self.attribute("fallback", text.into())
    }

    pub fn color(&mut self, color: impl Into<String>) -> &mut Self {
        self.attribute("color", color.into())
    }

    pub fn field(&mut self, title: impl Into<String>, content: impl Into<Value>) -> &mut Self {
        let title = title.into();
        self.fields.insert(
            FieldKey::Named(title.clone()),
            SlackAttachmentField {
                title: Some(title),
                content: content.into(),
                short: true,
            },
        );
        self
    }

    pub fn field_with(&mut self, build: impl FnOnce(&mut SlackAttachmentField)) -> &mut Self {
        let mut field = SlackAttachmentField::default();
        build(&mut field);
        self.fields
            .insert(FieldKey::Custom(self.fields.len()), field);
        self
    }

    pub fn fields<K, V>(&mut self, fields: impl IntoIterator<Item = (K, V)>) -> &mut Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        self.fields.clear();
        for (title, content) in fields {
            self.field(title, content);
        }
        self
    }

    pub fn markdown(&mut self, fields: &[&str]) -> &mut Self {
        self.attribute("mrkdwn_in", fields)
    }

    pub fn image(&mut self, url: impl Into<String>) -> &mut Self {
        self.attribute("image_url", url.into())
    }

    pub fn thumb(&mut self, url: impl Into<String>) -> &mut Self {
        self.attribute("thumb_url", url.into())
    }

    pub fn action(&mut self, title: &str, url: &str, style: Option<&str>) -> &mut Self {
        self.actions.push(json!({
            "type": "button", "text": title, "url": url, "style": style.unwrap_or_default(),
        }));
        self
    }

    pub fn author(&mut self, name: &str, link: Option<&str>, icon: Option<&str>) -> &mut Self {
        self.attribute("author_name", name);
        self.attribute("author_link", link);
        self.attribute("author_icon", icon)
    }

    pub fn footer(&mut self, footer: impl Into<String>) -> &mut Self {
        self.attribute("footer", footer.into())
    }

    pub fn footer_icon(&mut self, icon: impl Into<String>) -> &mut Self {
        self.attribute("footer_icon", icon.into())
    }

    pub fn timestamp(&mut self, timestamp: Timestamp) -> &mut Self {
        self.attribute("ts", crate::support::timestamp_seconds(timestamp))
    }

    pub fn timestamp_after(&mut self, seconds: i64) -> &mut Self {
        self.attribute(
            "ts",
            crate::support::timestamp_seconds(Timestamp::now()).saturating_add(seconds),
        )
    }

    pub fn callback_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.attribute("callback_id", id.into())
    }

    fn attribute(&mut self, key: &str, value: impl Into<Value>) -> &mut Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    pub(super) fn payload(&self, default_color: Option<&str>) -> Map<String, Value> {
        let mut payload = self.attributes.clone();
        payload.retain(|_, value| filled(value));
        if let Some(color) = default_color {
            payload.entry("color").or_insert_with(|| color.into());
        }
        if !self.fields.is_empty() {
            payload.insert(
                "fields".into(),
                self.fields
                    .values()
                    .map(SlackAttachmentField::to_array)
                    .collect(),
            );
        }
        if !self.actions.is_empty() {
            payload.insert("actions".into(), self.actions.clone().into());
        }
        payload
    }
}

/// A legacy Slack attachment field. Fields are short by default.
pub struct SlackAttachmentField {
    title: Option<String>,
    content: Value,
    short: bool,
}

impl Default for SlackAttachmentField {
    fn default() -> Self {
        Self {
            title: None,
            content: Value::Null,
            short: true,
        }
    }
}

impl SlackAttachmentField {
    pub fn title(&mut self, title: impl Into<String>) -> &mut Self {
        self.title = Some(title.into());
        self
    }

    pub fn content(&mut self, value: impl Into<Value>) -> &mut Self {
        self.content = value.into();
        self
    }

    pub fn long(&mut self) -> &mut Self {
        self.short = false;
        self
    }

    pub fn to_array(&self) -> Value {
        json!({"title": self.title, "value": self.content, "short": self.short})
    }
}
