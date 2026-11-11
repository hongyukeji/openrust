use std::{fmt::Write as _, sync::Arc, time::Duration};

use aws_lc_rs::{hmac, rand};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};

use crate::{
    auth::{AuthError, AuthResult},
    contracts::hashing::Hasher,
    hashing::HashOptions,
};

/// Generates high-entropy tokens and applies the configured expiry and throttle.
pub(super) struct TokenPolicy {
    key: hmac::Key,
    hasher: Arc<dyn Hasher>,
    expires: chrono::TimeDelta,
    throttle: chrono::TimeDelta,
}

impl TokenPolicy {
    pub(super) fn new(
        app_key: &str,
        hasher: Arc<dyn Hasher>,
        expires: Duration,
        throttle: Duration,
    ) -> AuthResult<Self> {
        let key = match app_key.strip_prefix("base64:") {
            Some(key) => STANDARD
                .decode(key)
                .map_err(|_| AuthError::Configuration("app.key"))?,
            None => app_key.as_bytes().to_vec(),
        };
        if key.is_empty() {
            return Err(AuthError::Configuration("app.key"));
        }
        let duration = |value| {
            chrono::TimeDelta::from_std(value)
                .ok()
                .filter(|value| value.num_seconds() <= i64::from(u32::MAX))
                .ok_or(AuthError::Configuration("auth.passwords"))
        };
        if expires.as_secs() == 0 {
            return Err(AuthError::Configuration("auth.passwords.expire"));
        }
        Ok(Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, &key),
            hasher,
            expires: duration(expires)?,
            throttle: duration(throttle)?,
        })
    }

    pub(super) async fn generate(&self) -> AuthResult<(String, String)> {
        let mut bytes = [0u8; 32];
        rand::fill(&mut bytes).map_err(AuthError::backend)?;
        let digest = hmac::sign(&self.key, &bytes);
        let mut token = String::with_capacity(64);
        for byte in digest.as_ref() {
            write!(token, "{byte:02x}").expect("writing to a String");
        }
        let hash = self
            .hasher
            .make(token.as_bytes(), &HashOptions::default())
            .await
            .map_err(AuthError::backend)?;
        Ok((token, hash))
    }

    pub(super) async fn matches(&self, token: &str, hash: &str) -> AuthResult<bool> {
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(false);
        }
        self.hasher
            .check(token.as_bytes(), hash, &HashOptions::default())
            .await
            .map_err(AuthError::backend)
    }

    pub(super) fn lifetime(&self) -> i64 {
        self.expires.num_seconds()
    }

    pub(super) fn expiry_cutoff(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now - self.expires
    }

    pub(super) fn throttle_cutoff(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        (!self.throttle.is_zero()).then(|| now - self.throttle)
    }
}

pub(super) fn now() -> DateTime<Utc> {
    DateTime::from(std::time::SystemTime::now())
}
