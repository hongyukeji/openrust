use std::{fmt::Write as _, sync::Arc, time::Duration};

use aws_lc_rs::{digest, hmac, rand};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use jiff::Timestamp;

use crate::{
    auth::{AuthError, AuthResult},
    contracts::{auth::CanResetPassword, hashing::Hasher},
    hashing::HashOptions,
};

/// Generates high-entropy tokens and applies the configured expiry and throttle.
pub(super) struct TokenPolicy {
    pub(super) bindings: Arc<super::user_bindings::UserBindings>,
    key: hmac::Key,
    hasher: Arc<dyn Hasher>,
    expires: jiff::SignedDuration,
    throttle: jiff::SignedDuration,
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
            jiff::SignedDuration::try_from(value)
                .ok()
                .filter(|value| value.as_secs() <= i64::from(u32::MAX))
                .ok_or(AuthError::Configuration("auth.passwords"))
        };
        if expires.as_secs() == 0 {
            return Err(AuthError::Configuration("auth.passwords.expire"));
        }
        Ok(Self {
            bindings: Arc::default(),
            key: hmac::Key::new(hmac::HMAC_SHA256, &key),
            hasher,
            expires: duration(expires)?,
            throttle: duration(throttle)?,
        })
    }

    pub(super) async fn generate(
        &self,
        user: &dyn CanResetPassword,
    ) -> AuthResult<(String, String)> {
        let mut bytes = [0u8; 32];
        rand::fill(&mut bytes).map_err(AuthError::backend)?;
        let digest = hmac::sign(&self.key, &bytes);
        let mut token = String::with_capacity(64);
        for byte in digest.as_ref() {
            write!(token, "{byte:02x}").expect("writing to a String");
        }
        let material = self.material(user, &token)?;
        let hash = self
            .hasher
            .make(material.as_bytes(), &HashOptions::default())
            .await?;
        Ok((token, hash))
    }

    pub(super) async fn matches(
        &self,
        user: &dyn CanResetPassword,
        token: &str,
        hash: &str,
    ) -> AuthResult<bool> {
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(false);
        }
        let material = self.material(user, token)?;
        self.hasher
            .check(material.as_bytes(), hash, &HashOptions::default())
            .await
            .map_err(AuthError::backend)
    }

    fn material(&self, user: &dyn CanResetPassword, token: &str) -> AuthResult<String> {
        let values = self.bindings.values(user)?;
        if values.is_empty() {
            return Ok(token.to_owned());
        }
        // 固定长度预哈希避免bcrypt的72字节上限；不改变邮件中的64位随机Token。
        let material = serde_json::to_vec(&("user-reset-binding:v1", token, values))?;
        let hash = digest::digest(&digest::SHA256, &material);
        Ok(hash
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }

    pub(super) fn lifetime(&self) -> i64 {
        self.expires.as_secs()
    }

    pub(super) fn expiry_cutoff(&self, now: Timestamp) -> Timestamp {
        now - self.expires
    }

    pub(super) fn throttle_cutoff(&self, now: Timestamp) -> Option<Timestamp> {
        (!self.throttle.is_zero()).then(|| now - self.throttle)
    }
}

pub(super) fn now() -> Timestamp {
    crate::support::utc_now_for(crate::support::facades::current_container().as_deref())
}
