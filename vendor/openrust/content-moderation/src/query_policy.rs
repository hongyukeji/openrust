use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ModerationError, ModerationResult};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct QueryPolicy {
    pub(crate) interval_seconds: u32,
    pub(crate) max_attempts: u16,
}

impl Default for QueryPolicy {
    fn default() -> Self {
        Self {
            interval_seconds: 30,
            max_attempts: 120,
        }
    }
}

impl QueryPolicy {
    pub(crate) fn configured(profile: &Value) -> ModerationResult<Self> {
        let policy = Self::default();
        let bounded = |key, default, max| match profile.get(key) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .filter(|value| (1..=max).contains(value))
                .ok_or(ModerationError::InvalidConfiguration(key)),
        };
        Ok(Self {
            interval_seconds: bounded(
                "query_interval_seconds",
                u64::from(policy.interval_seconds),
                300,
            )? as u32,
            max_attempts: bounded("query_max_attempts", u64::from(policy.max_attempts), 300)?
                as u16,
        })
    }

    pub(crate) fn interval(self) -> i64 {
        i64::from(self.interval_seconds)
    }

    pub(crate) fn retry_delay(self, attempt: u16, review: &str) -> i64 {
        let base = self.interval() * 2_i64.pow(u32::from(attempt.min(5)));
        let jitter = review
            .as_bytes()
            .first()
            .map_or(0, |byte| i64::from(*byte % 5));
        (base + jitter).min(300)
    }
}
