//! 签名链接配置。

use std::collections::BTreeMap;

use serde::Deserialize;

/// 仅供应用签名链接使用的密钥环和签发者。
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLinksConfig {
    pub(crate) issuer: String,
    pub(crate) active_key_id: String,
    pub(crate) keys: BTreeMap<String, String>,
    #[serde(default = "default_max_ttl_secs")]
    pub(crate) max_ttl_secs: u64,
}

impl SignedLinksConfig {
    /// 使用签发者、当前密钥 ID 和 Base64 密钥环创建签名链接设置。
    pub fn new(
        issuer: impl Into<String>,
        active_key_id: impl Into<String>,
        keys: BTreeMap<String, String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            active_key_id: active_key_id.into(),
            keys,
            max_ttl_secs: default_max_ttl_secs(),
        }
    }

    /// 设置链接允许的最长有效期（秒）。
    #[must_use]
    pub const fn with_max_ttl_secs(mut self, max_ttl_secs: u64) -> Self {
        self.max_ttl_secs = max_ttl_secs;
        self
    }
}

impl std::fmt::Debug for SignedLinksConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignedLinksConfig")
            .field("issuer", &self.issuer)
            .field("active_key_id", &self.active_key_id)
            .field("key_ids", &self.keys.keys().collect::<Vec<_>>())
            .field("keys", &"[REDACTED]")
            .field("max_ttl_secs", &self.max_ttl_secs)
            .finish()
    }
}

fn default_max_ttl_secs() -> u64 {
    86_400
}
