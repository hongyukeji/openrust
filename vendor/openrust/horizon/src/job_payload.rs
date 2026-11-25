use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openrust::{
    queue::{Payload, QueueError, QueueResult},
    support::facades::{Config, Facade},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result, Tags};

const KEY: &str = "horizon";

/// Horizon 随 Redis 投递保存的观测元数据。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobPayload {
    tags: Vec<String>,
    silenced: bool,
    #[serde(rename = "pushedAt")]
    pushed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_of: Option<String>,
}

impl JobPayload {
    /// 读取已保存的元数据。旧版本入队的任务可能没有此记录。
    pub fn from_payload(payload: &Payload) -> Result<Option<Self>> {
        payload
            .redis_metadata(KEY)
            .map(|value| serde_json::from_value(value.clone()).map_err(|_| Error::InvalidRecord))
            .transpose()
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
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
        let (tags, silenced) = if payload.is_retry_delivery() {
            let original = Self::from_payload(payload).map_err(QueueError::backend)?;
            let tags = match original {
                Some(snapshot) => snapshot.tags,
                None => Tags::for_payload(payload).map_err(QueueError::backend)?,
            };
            (tags, false)
        } else {
            Self::resolve(payload).map_err(QueueError::backend)?
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        serde_json::to_value(Self {
            tags,
            silenced,
            pushed_at: format!("{}.{:06}", now.as_secs(), now.subsec_micros()),
            retry_of: payload.retry_of().map(str::to_owned),
        })
        .map_err(QueueError::backend)
    }

    pub(crate) fn tags_and_silence(payload: &Payload) -> Result<(Vec<String>, bool)> {
        match Self::from_payload(payload)? {
            Some(snapshot) => Ok((snapshot.tags, snapshot.silenced)),
            None => Self::resolve(payload),
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

    fn resolve(payload: &Payload) -> Result<(Vec<String>, bool)> {
        let tags = Tags::for_payload(payload)?;
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
        let silenced = matches("horizon.silenced", &|name| name == payload.name())
            || matches("horizon.silenced_tags", &|tag| {
                tags.iter().any(|value| value == tag)
            });
        Ok((tags, silenced))
    }
}
