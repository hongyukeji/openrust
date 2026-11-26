use std::sync::Arc;

use crate::http::{HeaderMap, Request, Response};

use super::{AfterCallback, ResponseCallback, Unlimited};

/// An attempt budget and decay period, with an optional key and response hooks.
#[derive(Clone)]
pub struct Limit {
    pub key: String,
    pub max_attempts: u64,
    pub decay_seconds: u64,
    pub after_callback: Option<Arc<AfterCallback>>,
    pub response_callback: Option<Arc<ResponseCallback>>,
}

impl Default for Limit {
    fn default() -> Self {
        Self::new("", 60, 60)
    }
}

impl Limit {
    pub fn new(key: impl Into<String>, max_attempts: u64, decay_seconds: u64) -> Self {
        Self {
            key: key.into(),
            max_attempts,
            decay_seconds,
            after_callback: None,
            response_callback: None,
        }
    }

    pub fn per_second(max_attempts: u64) -> Self {
        Self::new("", max_attempts, 1)
    }

    pub fn per_minute(max_attempts: u64) -> Self {
        Self::per_minutes(1, max_attempts)
    }

    pub fn per_minutes(decay_minutes: u64, max_attempts: u64) -> Self {
        Self::new("", max_attempts, decay_minutes.saturating_mul(60))
    }

    pub fn per_hour(max_attempts: u64) -> Self {
        Self::new("", max_attempts, 3_600)
    }

    pub fn per_day(max_attempts: u64) -> Self {
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
        self.after_callback = Some(Arc::new(callback));
        self
    }

    pub fn response(
        mut self,
        callback: impl Fn(&Request, HeaderMap) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.response_callback = Some(Arc::new(callback));
        self
    }

    pub fn fallback_key(&self) -> String {
        let suffix = format!(
            "attempts:{}:decay:{}",
            self.max_attempts, self.decay_seconds
        );
        if matches!(self.key.as_str(), "" | "0") {
            suffix
        } else {
            format!("{}:{suffix}", self.key)
        }
    }
}
