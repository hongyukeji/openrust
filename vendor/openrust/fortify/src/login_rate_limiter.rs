use crate::{Fortify, resolver::internal};
use openrust::{
    cache::RateLimiter,
    http::{HttpError, Request},
    support::Str,
};
use std::sync::Arc;

pub struct LoginRateLimiter {
    limiter: Arc<RateLimiter>,
    fortify: Arc<Fortify>,
}
impl LoginRateLimiter {
    pub fn new(limiter: Arc<RateLimiter>, fortify: Arc<Fortify>) -> Self {
        Self { limiter, fortify }
    }
    pub async fn throttle_key(&self, request: &mut Request) -> Result<String, HttpError> {
        let username = request
            .input(self.fortify.username().as_str())
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(Str::transliterate(&format!(
            "{}|{}",
            Str::lower(&username),
            request.ip().map(|v| v.to_string()).unwrap_or_default()
        )))
    }
    pub async fn attempts(&self, request: &mut Request) -> Result<u64, HttpError> {
        Ok(self
            .limiter
            .attempts(&self.throttle_key(request).await?)
            .await?)
    }
    pub async fn too_many_attempts(&self, request: &mut Request) -> Result<bool, HttpError> {
        Ok(self
            .limiter
            .too_many_attempts(&self.throttle_key(request).await?, 5)
            .await?)
    }
    pub async fn increment(&self, request: &mut Request) -> Result<(), HttpError> {
        self.limiter
            .hit(&self.throttle_key(request).await?, 60)
            .await
            .map_err(internal)?;
        Ok(())
    }
    pub async fn available_in(&self, request: &mut Request) -> Result<u64, HttpError> {
        Ok(self
            .limiter
            .available_in(&self.throttle_key(request).await?)
            .await?)
    }
    pub async fn clear(&self, request: &mut Request) -> Result<(), HttpError> {
        Ok(self
            .limiter
            .clear(&self.throttle_key(request).await?)
            .await?)
    }
}
