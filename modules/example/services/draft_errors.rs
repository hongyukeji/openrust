//! 文章草稿审核错误包：持久化后仍保留业务化的首条消息行为。

use openrust::support::{
    ArrayKey, Collection, MessageBag, MessageBagContract, MessageKeys, MessageLookup,
    MessageProvider, MessageValue,
};
use openrust::{Error, Result};
use serde::Serialize;
use serde_json::Value;

/// 示例业务自己的错误包，公开 codec 恢复后仍实现同一行为。
pub(crate) struct DraftErrors {
    messages: MessageBag,
    guidance: String,
}

impl DraftErrors {
    pub(crate) fn new(guidance: impl Into<String>) -> Result<Self> {
        let mut messages = MessageBag::new();
        messages.add("title", "标题需要审核")?;
        Ok(Self {
            messages,
            guidance: guidance.into(),
        })
    }
}

impl MessageProvider for DraftErrors {
    fn message_bag(&self) -> &dyn MessageBagContract {
        self
    }
}

impl MessageBagContract for DraftErrors {
    fn keys(&self) -> Vec<ArrayKey> {
        self.messages.keys()
    }
    fn messages(&self) -> &Collection<Collection<MessageValue>> {
        self.messages.messages()
    }
    fn format(&self) -> &str {
        self.messages.format()
    }
    fn set_format(&mut self, format: String) {
        self.messages.set_format(format);
    }
    fn add(&mut self, key: ArrayKey, message: String) -> Result<()> {
        self.messages.add(key, message)
    }
    fn merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> Result<()> {
        self.messages.merge(messages)
    }
    fn has(&self, keys: MessageKeys) -> Result<bool> {
        self.messages.has(keys)
    }
    fn first(&self, key: Option<&str>, format: Option<&str>) -> Result<Option<String>> {
        if key == Some("title") && format.is_none() {
            return Ok(Some(format!("草稿审核：{}", self.guidance)));
        }
        self.messages.first(key, format)
    }
    fn get(&self, key: &str, format: Option<&str>) -> Result<MessageLookup> {
        self.messages.get(key, format)
    }
    fn all(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        self.messages.all(format)
    }
    fn forget(&mut self, key: &ArrayKey) {
        self.messages.forget(key.clone());
    }
    fn count(&self) -> usize {
        self.messages.count()
    }
    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
    fn is_not_empty(&self) -> bool {
        self.messages.is_not_empty()
    }
}

#[derive(Serialize)]
struct DraftErrorPayload<'a> {
    guidance: &'a str,
}

pub(crate) fn encode(bag: &DraftErrors) -> Result<Value> {
    serde_json::to_value(DraftErrorPayload {
        guidance: &bag.guidance,
    })
    .map_err(Error::internal)
}

pub(crate) fn decode(value: Value) -> Result<DraftErrors> {
    let Value::Object(mut payload) = value else {
        return Err(invalid_payload());
    };
    let Some(Value::String(guidance)) = payload.remove("guidance") else {
        return Err(invalid_payload());
    };
    DraftErrors::new(guidance)
}

fn invalid_payload() -> Error {
    Error::custom("example_draft_errors_invalid", "草稿审核错误包内容无效")
}
