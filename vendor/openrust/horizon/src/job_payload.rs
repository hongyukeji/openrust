use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openrust::{
    queue::{JobType, Payload, QueueError, QueueResult},
    support::facades::{Config, Facade},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result, Tags};

const KEY: &str = "horizon";

/// 原始任务 JSON 及可独立修改的解码字段。
#[derive(Clone, Debug)]
pub struct JobPayload {
    value: String,
    decoded: Map<String, Value>,
}

impl JobPayload {
    /// 解析任务对象，保留原始 JSON 文本。
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let decoded = serde_json::from_str(&value).map_err(|_| Error::InvalidRecord)?;
        Ok(Self { value, decoded })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn decoded(&self) -> &Map<String, Value> {
        &self.decoded
    }

    pub fn id(&self) -> Option<&str> {
        self.decoded
            .get("uuid")
            .filter(|value| !value.is_null())
            .or_else(|| self.decoded.get("id"))
            .and_then(Value::as_str)
    }

    pub fn command_name(&self) -> Option<&str> {
        self.native_name()
            .or_else(|| self.offset_get("data")?.get("commandName"))
            .and_then(Value::as_str)
    }

    pub fn display_name(&self) -> Option<&str> {
        self.offset_get("displayName")
            .or_else(|| {
                self.native_name()
                    .map(|name| self.offset_get("display_name").unwrap_or(name))
            })
            .and_then(Value::as_str)
    }

    /// 缺失时返回空数组，已声明的空值保留为 JSON null。
    pub fn tags(&self) -> &Value {
        static EMPTY: Value = Value::Array(Vec::new());
        self.recorded_field("tags").unwrap_or(&EMPTY)
    }

    pub fn is_silenced(&self) -> bool {
        self.recorded_field("silenced")
            .and_then(Value::as_bool)
            .unwrap_or_default()
    }

    pub fn retry_of(&self) -> Option<&str> {
        self.offset_get("retry_of").and_then(Value::as_str)
    }

    pub fn is_retry(&self) -> bool {
        self.offset_get("retry_of")
            .is_some_and(|value| !value.is_null())
    }

    pub fn offset_exists(&self, key: &str) -> bool {
        self.decoded.contains_key(key)
    }

    pub fn offset_get(&self, key: &str) -> Option<&Value> {
        self.decoded.get(key)
    }

    fn native_name(&self) -> Option<&Value> {
        self.offset_get("version")?.as_u64()?;
        self.offset_get("options")?.as_object()?;
        self.offset_get("name")
    }

    fn recorded_field(&self, key: &str) -> Option<&Value> {
        self.offset_get(key)
            .or_else(|| self.offset_get("redis_metadata")?.get(KEY)?.get(key))
    }

    /// 仅修改解码字段。调用 `set` 后，原始文本才会反映这些修改。
    pub fn offset_set(&mut self, key: impl Into<String>, value: Value) {
        self.decoded.insert(key.into(), value);
    }

    /// 仅从解码字段移除键。
    pub fn offset_unset(&mut self, key: &str) {
        self.decoded.shift_remove(key);
    }

    /// 合并字段，并将全部解码字段重新编码为原始 JSON。
    pub fn set(&mut self, values: Map<String, Value>) -> &mut Self {
        self.decoded.extend(values);
        self.value = serde_json::to_string(&self.decoded).expect("JSON 对象可重新编码");
        self
    }

    /// 为原始投递计算 Horizon 字段。没有任务对象的重投递保留原标签。
    pub fn prepare(&mut self, job: Option<&Payload>) -> Result<&mut Self> {
        let tags = decode_tags(self.recorded_field("tags"))?;
        let (job_type, tags, silenced) = match job {
            Some(job) => {
                let (tags, silenced) = resolve(job, tags)?;
                (job.job_type(), tags, silenced)
            }
            None => (JobType::Job, tags, false),
        };
        Ok(self.set(Map::from_iter([
            (
                "type".into(),
                serde_json::to_value(job_type).map_err(|_| Error::InvalidRecord)?,
            ),
            ("tags".into(), Value::from(tags)),
            ("silenced".into(), Value::Bool(silenced)),
            ("pushedAt".into(), Value::String(pushed_at())),
        ])))
    }

    /// 在实际入队前为任务设置预置标签，顺序和重复项均保留。
    pub fn with_tags(payload: Payload, tags: Vec<String>) -> Payload {
        payload.with_redis_metadata_input(KEY, Value::from(tags))
    }

    /// 读取原生 Redis 投递保存的观测元数据。旧任务可能没有该记录。
    pub fn from_payload(payload: &Payload) -> Result<Option<JobPayloadMetadata>> {
        JobPayloadMetadata::from_payload(payload)
    }
}

/// Horizon 随原生 Redis 投递保存的观测元数据。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobPayloadMetadata {
    #[serde(default, rename = "type")]
    job_type: JobType,
    tags: Vec<String>,
    silenced: bool,
    #[serde(rename = "pushedAt")]
    pushed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_of: Option<String>,
}

impl JobPayloadMetadata {
    pub(crate) fn from_payload(payload: &Payload) -> Result<Option<Self>> {
        payload
            .redis_metadata(KEY)
            .map(|value| serde_json::from_value(value.clone()).map_err(|_| Error::InvalidRecord))
            .transpose()
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn job_type(&self) -> JobType {
        self.job_type
    }

    pub fn is_silenced(&self) -> bool {
        self.silenced
    }

    /// 以十进制秒表示的投递时间。
    pub fn pushed_at(&self) -> &str {
        &self.pushed_at
    }

    pub fn retry_of(&self) -> Option<&str> {
        self.retry_of.as_deref()
    }

    pub fn is_retry(&self) -> bool {
        self.retry_of.is_some()
    }

    pub(crate) fn register(manager: &openrust::queue::QueueManager) {
        manager.register_redis_payload_metadata(KEY, Self::prepare);
    }

    fn prepare(payload: &Payload) -> QueueResult<Value> {
        let input = payload.redis_metadata_input(KEY);
        let preset = || decode_tags(input).map_err(QueueError::backend);
        let (tags, silenced) = if payload.is_retry_delivery() {
            let original = Self::from_payload(payload).map_err(QueueError::backend)?;
            let tags = match original {
                Some(snapshot) => snapshot.tags,
                None if input.is_some() => preset()?,
                None => Tags::for_payload(payload).map_err(QueueError::backend)?,
            };
            (tags, false)
        } else {
            resolve(payload, preset()?).map_err(QueueError::backend)?
        };
        serde_json::to_value(Self {
            job_type: if payload.is_retry_delivery() {
                JobType::Job
            } else {
                payload.job_type()
            },
            tags,
            silenced,
            pushed_at: pushed_at(),
            retry_of: payload.retry_of().map(str::to_owned),
        })
        .map_err(QueueError::backend)
    }

    pub(crate) fn tags_and_silence(payload: &Payload) -> Result<(Vec<String>, bool)> {
        match Self::from_payload(payload)? {
            Some(snapshot) => Ok((snapshot.tags, snapshot.silenced)),
            None => resolve(payload, decode_tags(payload.redis_metadata_input(KEY))?),
        }
    }

    pub(crate) fn retry_deadline(payload: &Payload) -> Result<Option<SystemTime>> {
        let Some(deadline) = payload.options().retry_until else {
            return Ok(None);
        };
        let Some(snapshot) = Self::from_payload(payload)? else {
            return Ok(Some(deadline));
        };
        let (seconds, micros) = snapshot
            .pushed_at
            .split_once('.')
            .ok_or(Error::InvalidRecord)?;
        if micros.len() != 6 || !micros.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Error::InvalidRecord);
        }
        let seconds = seconds.parse::<u64>().map_err(|_| Error::InvalidRecord)?;
        let micros = micros.parse::<u32>().map_err(|_| Error::InvalidRecord)?;
        let pushed_at = UNIX_EPOCH
            .checked_add(Duration::new(seconds, micros * 1_000))
            .ok_or(Error::InvalidRecord)?;
        let now = UNIX_EPOCH
            + Duration::from_secs(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
        let deadline = match deadline.duration_since(pushed_at) {
            Ok(window) => {
                let seconds = window
                    .as_secs()
                    .saturating_add(u64::from(window.subsec_nanos() != 0));
                now.checked_add(Duration::from_secs(seconds))
            }
            Err(error) => now.checked_sub(Duration::from_secs(error.duration().as_secs())),
        };
        deadline.map(Some).ok_or(Error::InvalidRecord)
    }
}

fn pushed_at() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:06}", now.as_secs(), now.subsec_micros())
}

fn decode_tags(value: Option<&Value>) -> Result<Vec<String>> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| Error::InvalidRecord),
    }
}

fn resolve(payload: &Payload, mut tags: Vec<String>) -> Result<(Vec<String>, bool)> {
    tags.extend(Tags::for_payload(payload)?);
    let config = Config::get_facade_root().map_err(|_| Error::Unavailable)?;
    let matches = |key: &str, matches: &dyn Fn(&str) -> bool| {
        config
            .get(key)
            .and_then(|value| {
                value
                    .as_array()
                    .map(|values| values.iter().filter_map(Value::as_str).any(matches))
            })
            .unwrap_or(false)
    };
    let subject = payload.subject_type().unwrap_or(payload.name());
    let silenced = matches("horizon.silenced", &|name| {
        name == subject || (payload.job_type() == JobType::Job && name == payload.name())
    }) || crate::silenced::SilencedTypes::contains(subject)
        .map_err(|_| Error::Unavailable)?
        || matches("horizon.silenced_tags", &|tag| {
            tags.iter().any(|value| value == tag)
        });
    Ok((tags, silenced))
}
