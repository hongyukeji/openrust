use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value};

use crate::notifications::{NotificationResult, delivery};

use super::EventMetadata;
use super::blocks::{
    ActionsBlock, ContextBlock, DividerBlock, HeaderBlock, ImageBlock, SectionBlock,
};

/// Payload for Slack's Bot API or an incoming webhook.
#[derive(Default)]
pub struct SlackMessage {
    fields: Map<String, Value>,
    blocks: Vec<Value>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
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
        self.headerBlockWith(text, |_| {})
    }

    pub fn headerBlockWith(
        self,
        text: impl Into<String>,
        build: impl FnOnce(&mut HeaderBlock),
    ) -> Self {
        let mut block = HeaderBlock::new(text);
        build(&mut block);
        self.block(block.into_json())
    }

    pub fn dividerBlock(self) -> Self {
        self.dividerBlockWith(|_| {})
    }

    pub fn dividerBlockWith(self, build: impl FnOnce(&mut DividerBlock)) -> Self {
        let mut block = DividerBlock::default();
        build(&mut block);
        self.block(block.into_json())
    }

    pub fn imageBlock(self, url: impl Into<String>, alt_text: impl Into<String>) -> Self {
        self.imageBlockWith(url, |block| {
            block.alt(alt_text);
        })
    }

    pub fn imageBlockWith(
        self,
        url: impl Into<String>,
        build: impl FnOnce(&mut ImageBlock),
    ) -> Self {
        let mut block = ImageBlock::new(url);
        build(&mut block);
        self.block(block.into_json())
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

    pub fn tryActionsBlock(
        self,
        build: impl FnOnce(&mut ActionsBlock) -> NotificationResult<()>,
    ) -> NotificationResult<Self> {
        let mut block = ActionsBlock::default();
        build(&mut block)?;
        Ok(self.block(block.into_json()))
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
        self.withMetadata(EventMetadata::make(event_type, payload))
    }

    pub fn withMetadata(self, metadata: EventMetadata) -> Self {
        self.field("metadata", metadata.into_array())
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

    /// Remove the parent message timestamp from this message.
    pub fn clearThreadTimestamp(mut self) -> Self {
        self.fields.remove("thread_ts");
        self
    }

    /// Set the broadcast flag, or pass `None` to omit it from the Slack request.
    pub fn broadcastReply(mut self, enable: impl Into<Option<bool>>) -> Self {
        if let Some(enable) = enable.into() {
            self.fields.insert("reply_broadcast".into(), enable.into());
        } else {
            self.fields.remove("reply_broadcast");
        }
        self
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
    pub fn to_array(&self) -> NotificationResult<Map<String, Value>> {
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
        let mut fields = self.to_array()?;
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
        Some("header") => check_plain_text(&block["text"])?,
        Some("image") => {
            check_alt(block)?;
            check_length(block, "image_url", 3000)?;
            check_length(block, "alt_text", 2000)?;
            if let Some(title) = block.get("title") {
                check_plain_text(title)?;
            }
        }
        Some("context") => {
            check_items(&block["elements"], 1, 10)?;
            for element in block["elements"].as_array().into_iter().flatten() {
                match element["type"].as_str() {
                    Some("plain_text" | "mrkdwn") => check_text(element)?,
                    Some("image") => check_alt(element)?,
                    _ => {}
                }
            }
        }
        Some("actions") => {
            check_items(&block["elements"], 1, 25)?;
            for element in block["elements"].as_array().into_iter().flatten() {
                validate_action(element)?;
            }
        }
        Some("section") => {
            let min = usize::from(block["text"].is_null());
            check_items(&block["fields"], min, 10)?;
            if !block["text"].is_null() {
                check_text(&block["text"])?;
            }
            for field in block["fields"].as_array().into_iter().flatten() {
                check_text(field)?;
            }
            validate_action(&block["accessory"])?;
        }
        Some(kind) if !kind.trim().is_empty() => {}
        _ => return Err(delivery("Each Slack block requires a type.")),
    }
    check_length(block, "block_id", 255)
}

fn validate_action(element: &Value) -> NotificationResult<()> {
    match element["type"].as_str() {
        Some("button") => {
            check_text(&element["text"])?;
            check_length(element, "url", 3000)?;
            check_length(element, "value", 2000)?;
            check_length(element, "accessibility_label", 75)?;
            if let Some(dialog) = element.get("confirm") {
                for key in ["title", "text", "confirm", "deny"] {
                    check_text(&dialog[key])?;
                }
            }
        }
        Some("static_select" | "users_select") => {
            if let Some(placeholder) = element.get("placeholder") {
                check_text(placeholder)?;
            }
            for option in element["options"].as_array().into_iter().flatten() {
                check_text(&option["text"])?;
            }
            if let Some(option) = element.get("initial_option") {
                check_text(&option["text"])?;
            }
        }
        _ => return Ok(()),
    }
    check_length(element, "action_id", 255)
}

fn check_text(object: &Value) -> NotificationResult<()> {
    if object["text"].as_str().is_none_or(str::is_empty) {
        return Err(delivery("A Slack Block Kit text object requires text."));
    }
    Ok(())
}

fn check_plain_text(object: &Value) -> NotificationResult<()> {
    if object["type"].as_str() != Some("plain_text") {
        return Err(delivery(
            "The Slack Block Kit text object must use plain text.",
        ));
    }
    check_text(object)
}

fn check_alt(image: &Value) -> NotificationResult<()> {
    if !image["alt_text"].is_string() {
        return Err(delivery(
            "A Slack Block Kit image requires alternative text.",
        ));
    }
    Ok(())
}

fn check_length(object: &Value, field: &str, limit: usize) -> NotificationResult<()> {
    if object[field]
        .as_str()
        .is_some_and(|value| value.len() > limit)
    {
        return Err(delivery(format!(
            "The Slack {field} field may contain at most {limit} bytes."
        )));
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
