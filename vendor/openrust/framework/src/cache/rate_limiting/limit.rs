use std::sync::Arc;

use crate::http::{HeaderMap, Request, Response};

use super::{AfterCallback, ResponseCallback, Unlimited};

/// An attempt budget and decay period, with an optional key and response hooks.
#[derive(Clone)]
#[allow(non_snake_case, reason = "Matches Laravel's Limit API")]
pub struct Limit {
    pub key: String,
    pub maxAttempts: u64,
    pub decaySeconds: u64,
    pub afterCallback: Option<Arc<AfterCallback>>,
    pub responseCallback: Option<Arc<ResponseCallback>>,
}

impl Default for Limit {
    fn default() -> Self {
        Self::new("", 60, 60)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Limit API")]
impl Limit {
    pub fn new(key: impl Into<String>, max_attempts: u64, decay_seconds: u64) -> Self {
        Self {
            key: key.into(),
            maxAttempts: max_attempts,
            decaySeconds: decay_seconds,
            afterCallback: None,
            responseCallback: None,
        }
    }

    pub fn perSecond(max_attempts: u64) -> Self {
        Self::new("", max_attempts, 1)
    }

    pub fn perMinute(max_attempts: u64) -> Self {
        Self::perMinutes(1, max_attempts)
    }

    pub fn perMinutes(decay_minutes: u64, max_attempts: u64) -> Self {
        Self::new("", max_attempts, decay_minutes.saturating_mul(60))
    }

    pub fn perHour(max_attempts: u64) -> Self {
        Self::new("", max_attempts, 3_600)
    }

    pub fn perDay(max_attempts: u64) -> Self {
        Self::new("", max_attempts, 86_400)
    }

    pub fn none() -> Unlimited {
        Unlimited
    }

    pub fn by(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }

    pub fn after(mut self, callback: impl Fn(&Response) -> bool + Send + Sync + 'static) -> Self {
        self.afterCallback = Some(Arc::new(callback));
        self
    }

    pub fn response(
        mut self,
        callback: impl Fn(&Request, HeaderMap) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.responseCallback = Some(Arc::new(callback));
        self
    }

    pub fn fallbackKey(&self) -> String {
        let suffix = format!("attempts:{}:decay:{}", self.maxAttempts, self.decaySeconds);
        if matches!(self.key.as_str(), "" | "0") {
            suffix
        } else {
            format!("{}:{suffix}", self.key)
        }
    }
}
