//! 身份认证、密码哈希和共享 Session 配置。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(crate) const MIN_MEMORY_KIB: u32 = 19_456;

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

/// 新密码哈希使用的 Argon2id 参数。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PasswordConfig {
    /// 内存开销（KiB）。
    pub memory_kib: u32,
    /// Argon2 迭代次数。
    pub iterations: u32,
    /// Argon2 并行度。
    pub parallelism: u32,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            memory_kib: MIN_MEMORY_KIB,
            iterations: 2,
            parallelism: 1,
        }
    }
}

/// 共享 Cookie Session middleware 的设置。
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Cookie 名称。默认值为 `openrust_session`。
    pub cookie_name: String,
    /// 浏览器是否仅通过 HTTPS 发送 cookie。默认值为 `true`。
    pub secure: bool,
    /// JavaScript 是否无法读取该 cookie（`HttpOnly`）。默认值为 `true`。
    pub http_only: bool,
    /// `SameSite` 策略：`strict`、`lax` 或 `none`。默认值为 `lax`。
    pub same_site: String,
    /// 无活动有效期（秒）。默认值为两周。
    pub ttl_secs: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_name: "openrust_session".to_owned(),
            secure: true,
            http_only: true,
            same_site: "lax".to_owned(),
            ttl_secs: 1_209_600,
        }
    }
}

impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionConfig")
            .field("cookie_name", &self.cookie_name)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("ttl_secs", &self.ttl_secs)
            .finish()
    }
}

fn default_access_ttl() -> u64 {
    900
}

fn default_max_valid_ttl() -> u64 {
    3_600
}
