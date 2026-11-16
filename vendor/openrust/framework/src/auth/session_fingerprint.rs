use aws_lc_rs::hmac;

use crate::contracts::config::Repository;

pub(crate) struct SessionFingerprint(hmac::Key);

impl SessionFingerprint {
    pub(crate) fn session_key(guard: &str) -> String {
        format!("password_hash_{guard}")
    }

    pub(crate) fn from_config(config: &dyn Repository) -> Self {
        let key = config.get("app.key");
        let key = key
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .unwrap_or("base-key-for-password-hash-mac");
        Self(hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()))
    }

    pub(crate) fn sign(&self, password: Option<&str>) -> Vec<u8> {
        hmac::sign(&self.0, password.unwrap_or_default().as_bytes())
            .as_ref()
            .to_vec()
    }
}
