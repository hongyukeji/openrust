use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};

use crate::notifications::{NotificationResult, delivery};

use super::blocks::{ActionsBlock, ContextBlock, SectionBlock, plain_text};

/// Payload for Slack's Bot API or an incoming webhook.
#[derive(Default)]
pub struct SlackMessage {
    fields: Map<String, Value>,
    blocks: Vec<Value>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Slack message API")]
impl SlackMessage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn to(self, channel: impl Into<String>) -> Self {
        self.field("channel", channel.into())
    }

    pub fn text(self, text: impl Into<String>) -> Self {
        self.field("text", text.into())
    }

    pub fn headerBlock(self, text: impl Into<String>) -> Self {
        self.block(json!({"type":"header","text":plain_text(text)}))
    }

    pub fn dividerBlock(self) -> Self {
        self.block(json!({"type":"divider"}))
    }

    pub fn imageBlock(self, url: impl Into<String>, alt_text: impl Into<String>) -> Self {
        self.block(json!({"type":"image","image_url":url.into(),"alt_text":alt_text.into()}))
    }

    pub fn contextBlock(self, build: impl FnOnce(&mut ContextBlock)) -> Self {
        let mut block = ContextBlock::default();
        build(&mut block);
        self.block(block.into_json())
    }

    pub fn sectionBlock(self, build: impl FnOnce(&mut SectionBlock)) -> Self {
        let mut block = SectionBlock::default();
        build(&mut block);
        self.block(block.into_json())
    }

    pub fn actionsBlock(self, build: impl FnOnce(&mut ActionsBlock)) -> Self {
        let mut block = ActionsBlock::default();
        build(&mut block);
        self.block(block.into_json())
    }

    pub fn emoji(mut self, emoji: impl Into<String>) -> Self {
        self.fields.remove("icon_url");
        self.field("icon_emoji", emoji.into())
    }

    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.fields.remove("icon_emoji");
        self.field("icon_url", url.into())
    }

    pub fn username(self, username: impl Into<String>) -> Self {
        self.field("username", username.into())
    }

    pub fn metadata(self, event_type: impl Into<String>, payload: Map<String, Value>) -> Self {
        self.field(
            "metadata",
            json!({"event_type":event_type.into(),"event_payload":payload}),
        )
    }

    pub fn disableMarkdownParsing(self) -> Self {
        self.field("mrkdwn", false)
    }

    pub fn unfurlLinks(self, enable: bool) -> Self {
        self.field("unfurl_links", enable)
    }

    pub fn unfurlMedia(self, enable: bool) -> Self {
        self.field("unfurl_media", enable)
    }

    pub fn threadTimestamp(self, timestamp: impl Into<String>) -> Self {
        self.field("thread_ts", timestamp.into())
    }

    pub fn broadcastReply(self, enable: bool) -> Self {
        self.field("reply_broadcast", enable)
    }

    /// Appends blocks from a Slack Block Kit Builder JSON document.
    pub fn usingBlockKitTemplate(mut self, template: &str) -> NotificationResult<Self> {
        let mut input: Value = serde_json::from_str(template)
            .map_err(|_| delivery("The Slack Block Kit template is invalid JSON."))?;
        let Some(Value::Array(blocks)) = input.get_mut("blocks").map(Value::take) else {
            return Err(delivery(
                "The Slack Block Kit template requires a blocks array.",
            ));
        };
        self.blocks.extend(blocks);
        Ok(self)
    }

    /// Returns the outbound Slack JSON fields after checking the Block Kit structure.
    pub fn toArray(&self) -> NotificationResult<Map<String, Value>> {
        if self.blocks.is_empty() && !self.fields.contains_key("text") {
            return Err(delivery(
                "A Slack message requires text or at least one block.",
            ));
        }
        if self.blocks.len() > 50 {
            return Err(delivery("A Slack message may contain at most 50 blocks."));
        }
        for block in &self.blocks {
            validate_block(block)?;
        }
        let mut fields = self.fields.clone();
        if !self.blocks.is_empty() {
            fields.insert("blocks".into(), Value::Array(self.blocks.clone()));
        }
        Ok(fields)
    }

    /// Builds a Slack Block Kit Builder URL without sending the message.
    pub fn toBlockKitBuilderUrl(&self) -> NotificationResult<String> {
        let mut fields = self.toArray()?;
        for key in ["channel", "text", "username"] {
            fields.remove(key);
        }
        let payload = Value::Object(fields).to_string();
        Ok(format!(
            "https://app.slack.com/block-kit-builder#{}",
            utf8_percent_encode(&payload, NON_ALPHANUMERIC)
        ))
    }

    fn field(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    fn block(mut self, block: Value) -> Self {
        self.blocks.push(block);
        self
    }
}

fn validate_block(block: &Value) -> NotificationResult<()> {
    match block["type"].as_str() {
        Some("context") => check_items(&block["elements"], 1, 10)?,
        Some("actions") => check_items(&block["elements"], 1, 25)?,
        Some("section") => {
            let min = usize::from(block["text"].is_null());
            check_items(&block["fields"], min, 10)?;
        }
        Some(kind) if !kind.trim().is_empty() => {}
        _ => return Err(delivery("Each Slack block requires a type.")),
    }
    if block["block_id"]
        .as_str()
        .is_some_and(|id| id.chars().count() > 255)
    {
        return Err(delivery(
            "A Slack block ID may contain at most 255 characters.",
        ));
    }
    Ok(())
}

fn check_items(items: &Value, min: usize, max: usize) -> NotificationResult<()> {
    let count = items.as_array().map_or(0, Vec::len);
    if !(min..=max).contains(&count) || (!items.is_null() && !items.is_array()) {
        return Err(delivery(format!(
            "The Slack block requires between {min} and {max} items."
        )));
    }
    Ok(())
}
