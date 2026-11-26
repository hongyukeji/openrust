#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Value<K = String> {
    pub timestamp: i64,
    pub r#type: String,
    pub key: K,
    pub value: String,
}

/// 保留原始键字节的仪表值。
pub type ByteValue = Value<Vec<u8>>;

impl Value {
    pub fn new(
        timestamp: i64,
        r#type: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            timestamp,
            r#type: r#type.into(),
            key: key.into(),
            value: value.into(),
        }
    }
}

impl ByteValue {
    pub fn from_key_bytes(
        timestamp: i64,
        r#type: impl Into<String>,
        key: Vec<u8>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            timestamp,
            r#type: r#type.into(),
            key,
            value: value.into(),
        }
    }
}

impl<K: serde::Serialize> Value<K> {
    pub fn attributes(&self) -> serde_json::Value {
        serde_json::json!({"timestamp":self.timestamp,"type":self.r#type,"key":self.key,"value":self.value})
    }
}
