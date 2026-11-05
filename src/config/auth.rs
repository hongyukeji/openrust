//! 身份认证和共享 Session 配置。

use std::collections::BTreeMap;

use serde::Deserialize;

/// JWT 签发与验证设置。密钥值是经 Base64 编码的 CSPRNG 随机输出。
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtConfig {
    pub(crate) issuer: String,
    pub(crate) audience: String,
    pub(crate) active_key_id: String,
    pub(crate) keys: BTreeMap<String, String>,
    #[serde(default = "default_access_ttl")]
    pub(crate) access_ttl_secs: u64,
    #[serde(default = "default_max_valid_ttl")]
    pub(crate) max_valid_ttl_secs: u64,
}

impl JwtConfig {
    /// 使用签发者、受众、当前密钥 ID 和 Base64 密钥环创建 JWT 设置。
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        active_key_id: impl Into<String>,
        keys: BTreeMap<String, String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            active_key_id: active_key_id.into(),
            keys,
            access_ttl_secs: default_access_ttl(),
            max_valid_ttl_secs: default_max_valid_ttl(),
        }
    }

    /// 设置 access token 的签发有效期和允许接受的最长有效期（秒）。
    #[must_use]
    pub fn with_ttl(mut self, access_ttl_secs: u64, max_valid_ttl_secs: u64) -> Self {
        self.access_ttl_secs = access_ttl_secs;
        self.max_valid_ttl_secs = max_valid_ttl_secs;
        self
    }
}

impl std::fmt::Debug for JwtConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JwtConfig")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("active_key_id", &self.active_key_id)
            .field("key_ids", &self.keys.keys().collect::<Vec<_>>())
            .field("keys", &"[REDACTED]")
            .field("access_ttl_secs", &self.access_ttl_secs)
            .field("max_valid_ttl_secs", &self.max_valid_ttl_secs)
            .finish()
    }
}

fn default_access_ttl() -> u64 {
    900
}

fn default_max_valid_ttl() -> u64 {
    3_600
}
