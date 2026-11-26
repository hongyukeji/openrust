use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

/// A measurement whose aggregation flags remain shared with the recording buffer.
#[derive(Clone, Debug)]
pub struct Entry {
    pub timestamp: i64,
    pub r#type: String,
    pub key: String,
    pub value: Option<i64>,
    original_key_bytes: Option<Vec<u8>>,
    aggregations: Arc<RwLock<Vec<String>>>,
    only_buckets: Arc<AtomicBool>,
}
impl Entry {
    pub fn new(
        timestamp: i64,
        r#type: impl Into<String>,
        key: impl Into<String>,
        value: Option<i64>,
    ) -> Self {
        Self {
            timestamp,
            r#type: r#type.into(),
            key: key.into(),
            value,
            original_key_bytes: None,
            aggregations: Arc::default(),
            only_buckets: Arc::default(),
        }
    }

    /// 保留非 UTF-8 分组键。文本字段仍可供现有读取方显示和编辑。
    pub fn from_key_bytes(
        timestamp: i64,
        r#type: impl Into<String>,
        key: Vec<u8>,
        value: Option<i64>,
    ) -> Self {
        match String::from_utf8(key) {
            Ok(key) => Self::new(timestamp, r#type, key, value),
            Err(error) => {
                let bytes = error.into_bytes();
                let mut entry =
                    Self::new(timestamp, r#type, String::from_utf8_lossy(&bytes), value);
                entry.original_key_bytes = Some(bytes);
                entry
            }
        }
    }

    /// 用于聚合和持久化的键。显式编辑文本字段后，以编辑值为准。
    pub fn key_bytes(&self) -> &[u8] {
        self.non_utf8_key().unwrap_or(self.key.as_bytes())
    }

    pub(crate) fn non_utf8_key(&self) -> Option<&[u8]> {
        self.original_key_bytes
            .as_deref()
            .filter(|bytes| String::from_utf8_lossy(bytes) == self.key)
    }
    fn aggregate(self, name: &str) -> Self {
        self.aggregations
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(name.into());
        self
    }
    pub fn count(self) -> Self {
        self.aggregate("count")
    }
    pub fn min(self) -> Self {
        self.aggregate("min")
    }
    pub fn max(self) -> Self {
        self.aggregate("max")
    }
    pub fn sum(self) -> Self {
        self.aggregate("sum")
    }
    pub fn avg(self) -> Self {
        self.aggregate("avg")
    }
    pub fn only_buckets(self) -> Self {
        self.only_buckets.store(true, Ordering::Relaxed);
        self
    }
    pub fn aggregations(&self) -> Vec<String> {
        self.aggregations
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn is_count(&self) -> bool {
        self.aggregations().iter().any(|a| a == "count")
    }
    pub fn is_min(&self) -> bool {
        self.aggregations().iter().any(|a| a == "min")
    }
    pub fn is_max(&self) -> bool {
        self.aggregations().iter().any(|a| a == "max")
    }
    pub fn is_sum(&self) -> bool {
        self.aggregations().iter().any(|a| a == "sum")
    }
    pub fn is_avg(&self) -> bool {
        self.aggregations().iter().any(|a| a == "avg")
    }
    pub fn is_only_buckets(&self) -> bool {
        self.only_buckets.load(Ordering::Relaxed)
    }
    pub fn attributes(&self) -> serde_json::Value {
        serde_json::json!({"timestamp":self.timestamp,"type":self.r#type,"key":self.key,"value":self.value})
    }
}
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum SerializedKey {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Serialize, Deserialize)]
struct SerializedEntry {
    timestamp: i64,
    r#type: String,
    key: SerializedKey,
    value: Option<i64>,
    aggregations: Vec<String>,
    #[serde(rename = "onlyBuckets")]
    only_buckets: bool,
}
impl Serialize for Entry {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        SerializedEntry {
            timestamp: self.timestamp,
            r#type: self.r#type.clone(),
            key: match self.non_utf8_key() {
                Some(bytes) => SerializedKey::Bytes(bytes.to_vec()),
                None => SerializedKey::Text(self.key.clone()),
            },
            value: self.value,
            aggregations: self.aggregations(),
            only_buckets: self.is_only_buckets(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let e = SerializedEntry::deserialize(d)?;
        if e.aggregations
            .iter()
            .any(|a| !["count", "min", "max", "sum", "avg"].contains(&a.as_str()))
        {
            return Err(serde::de::Error::custom("invalid Pulse aggregation"));
        }
        let entry = match e.key {
            SerializedKey::Text(key) => Self::new(e.timestamp, e.r#type, key, e.value),
            SerializedKey::Bytes(key) => Self::from_key_bytes(e.timestamp, e.r#type, key, e.value),
        };
        *entry
            .aggregations
            .write()
            .unwrap_or_else(|e| e.into_inner()) = e.aggregations;
        entry.only_buckets.store(e.only_buckets, Ordering::Relaxed);
        Ok(entry)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "class", content = "data")]
pub enum Item {
    Entry(Entry),
    Value(crate::Value),
}
impl From<Entry> for Item {
    fn from(e: Entry) -> Self {
        Self::Entry(e)
    }
}
impl From<crate::Value> for Item {
    fn from(v: crate::Value) -> Self {
        Self::Value(v)
    }
}
