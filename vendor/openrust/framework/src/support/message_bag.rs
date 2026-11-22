use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// 精确键返回消息列表，通配键保留字段到消息列表的映射。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum MessageLookup {
    Exact(Vec<String>),
    Wildcard(IndexMap<String, Vec<String>>),
}

impl MessageLookup {
    /// 显式扁平化，适合只消费消息文本的调用方。
    #[allow(non_snake_case)]
    pub fn intoMessages(self) -> Vec<String> {
        match self {
            Self::Exact(messages) => messages,
            Self::Wildcard(messages) => messages.into_values().flatten().collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Exact(messages) => messages.is_empty(),
            Self::Wildcard(messages) => messages.is_empty(),
        }
    }
}

/// 消息键选择；Any 对应未指定键，空列表仍是显式的空选择。
pub enum MessageKeys {
    Any,
    Keys(Vec<String>),
}

impl From<&str> for MessageKeys {
    fn from(key: &str) -> Self {
        Self::Keys(vec![key.to_owned()])
    }
}
impl<const N: usize> From<[&str; N]> for MessageKeys {
    fn from(keys: [&str; N]) -> Self {
        Self::Keys(keys.into_iter().map(str::to_owned).collect())
    }
}
impl From<Vec<String>> for MessageKeys {
    fn from(keys: Vec<String>) -> Self {
        Self::Keys(keys)
    }
}
impl From<Option<&str>> for MessageKeys {
    fn from(key: Option<&str>) -> Self {
        key.map_or(Self::Any, Self::from)
    }
}

/// 按字段和消息插入顺序保存验证错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageBag {
    messages: IndexMap<String, Vec<String>>,
    format: String,
}

impl Default for MessageBag {
    fn default() -> Self {
        Self {
            messages: IndexMap::new(),
            format: ":message".into(),
        }
    }
}

#[allow(non_snake_case)]
impl MessageBag {
    /// 构造时逐字段去重；空消息字段仍保留。
    pub fn new<K, S, V>(messages: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        S: Into<String>,
        V: IntoIterator<Item = S>,
    {
        let mut bag = Self::default();
        for (key, values) in messages {
            let mut unique = Vec::new();
            for message in values {
                let message = message.into();
                if !unique.contains(&message) {
                    unique.push(message);
                }
            }
            bag.messages.insert(key.into(), unique);
        }
        bag
    }

    pub fn keys(&self) -> Vec<&str> {
        self.messages.keys().map(String::as_str).collect()
    }

    pub fn add(&mut self, key: impl Into<String>, message: impl Into<String>) -> &mut Self {
        let messages = self.messages.entry(key.into()).or_default();
        let message = message.into();
        if !messages.contains(&message) {
            messages.push(message);
        }
        self
    }

    pub fn addIf(
        &mut self,
        condition: bool,
        key: impl Into<String>,
        message: impl Into<String>,
    ) -> &mut Self {
        if condition {
            self.add(key, message);
        }
        self
    }

    /// 合并保留重复项，与单条 add 的去重职责不同。
    pub fn merge(&mut self, messages: &MessageBag) -> &mut Self {
        for (key, values) in &messages.messages {
            self.messages
                .entry(key.clone())
                .or_default()
                .extend(values.iter().cloned());
        }
        self
    }

    pub fn has(&self, keys: impl Into<MessageKeys>) -> bool {
        if self.isEmpty() {
            return false;
        }
        match keys.into() {
            MessageKeys::Any => self.any(),
            MessageKeys::Keys(keys) => keys
                .iter()
                .all(|key| !self.first(Some(key), None).is_empty()),
        }
    }

    pub fn hasAny(&self, keys: impl Into<MessageKeys>) -> bool {
        if self.isEmpty() {
            return false;
        }
        match keys.into() {
            MessageKeys::Any => self.any(),
            MessageKeys::Keys(keys) => keys.iter().any(|key| self.has(key.as_str())),
        }
    }

    pub fn missing(&self, keys: impl Into<MessageKeys>) -> bool {
        !self.hasAny(keys)
    }

    pub fn first(&self, key: Option<&str>, format: Option<&str>) -> String {
        match key.map(|key| self.get(key, format)) {
            None => self.all(format).into_iter().next().unwrap_or_default(),
            Some(MessageLookup::Exact(messages)) => messages.into_iter().next().unwrap_or_default(),
            Some(MessageLookup::Wildcard(messages)) => messages
                .into_values()
                .next()
                .and_then(|messages| messages.into_iter().next())
                .unwrap_or_default(),
        }
    }

    pub fn get(&self, key: &str, format: Option<&str>) -> MessageLookup {
        if let Some(messages) = self.messages.get(key) {
            return MessageLookup::Exact(self.transform(messages, key, format));
        }
        if key.contains('*') {
            return MessageLookup::Wildcard(
                self.messages
                    .iter()
                    .filter(|(field, _)| super::Str::is(&[key], field, false))
                    .map(|(field, messages)| {
                        (field.clone(), self.transform(messages, field, format))
                    })
                    .collect(),
            );
        }
        MessageLookup::Exact(Vec::new())
    }

    pub fn all(&self, format: Option<&str>) -> Vec<String> {
        self.messages
            .iter()
            .flat_map(|(key, messages)| self.transform(messages, key, format))
            .collect()
    }

    pub fn unique(&self, format: Option<&str>) -> Vec<String> {
        let mut messages = Vec::new();
        for message in self.all(format) {
            if !messages.contains(&message) {
                messages.push(message);
            }
        }
        messages
    }

    pub fn forget(&mut self, key: &str) -> &mut Self {
        self.messages.shift_remove(key);
        self
    }
    pub fn messages(&self) -> &IndexMap<String, Vec<String>> {
        &self.messages
    }
    pub fn getMessages(&self) -> &IndexMap<String, Vec<String>> {
        self.messages()
    }
    pub fn getMessageBag(&self) -> &Self {
        self
    }
    pub fn getFormat(&self) -> &str {
        &self.format
    }
    pub fn setFormat(&mut self, format: impl Into<String>) -> &mut Self {
        self.format = format.into();
        self
    }
    pub fn isEmpty(&self) -> bool {
        !self.any()
    }
    pub fn isNotEmpty(&self) -> bool {
        self.any()
    }
    pub fn any(&self) -> bool {
        self.count() > 0
    }
    pub fn count(&self) -> usize {
        self.messages.values().map(Vec::len).sum()
    }
    pub fn len(&self) -> usize {
        self.count()
    }
    pub fn is_empty(&self) -> bool {
        self.isEmpty()
    }
    pub fn toArray(&self) -> &IndexMap<String, Vec<String>> {
        self.messages()
    }
    pub fn jsonSerialize(&self) -> &IndexMap<String, Vec<String>> {
        self.messages()
    }
    pub fn toJson(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
    pub fn toPrettyJson(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    fn transform(&self, messages: &[String], key: &str, format: Option<&str>) -> Vec<String> {
        let format = format
            .filter(|format| !format.is_empty() && *format != "0")
            .unwrap_or(&self.format);
        messages
            .iter()
            .map(|message| format.replace(":message", message).replace(":key", key))
            .collect()
    }
}

impl Serialize for MessageBag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.messages.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for MessageBag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            messages: IndexMap::deserialize(deserializer)?,
            ..Self::default()
        })
    }
}
impl fmt::Display for MessageBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.toJson().map_err(|_| fmt::Error)?)
    }
}
