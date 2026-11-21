#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Value {
    pub timestamp: i64,
    pub r#type: String,
    pub key: String,
    pub value: String,
}
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
    pub fn attributes(&self) -> serde_json::Value {
        serde_json::json!({"timestamp":self.timestamp,"type":self.r#type,"key":self.key,"value":self.value})
    }
}
