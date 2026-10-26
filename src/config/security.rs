//! 应用数据加密配置。

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;

pub(crate) struct SecretString(pub(crate) Zeroizing<String>);

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// 可选加密服务使用的 Base64 编码 AES-256-GCM-SIV 密钥。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptionConfig {
    pub(crate) active_key_id: String,
    pub(crate) keys: BTreeMap<String, SecretString>,
}

impl EncryptionConfig {
    /// 使用当前密钥 ID 和 Base64 编码的密钥值创建加密密钥环。
    pub fn new(active_key_id: impl Into<String>, keys: BTreeMap<String, String>) -> Self {
        Self {
            active_key_id: active_key_id.into(),
            keys: keys
                .into_iter()
                .map(|(key_id, value)| (key_id, SecretString(Zeroizing::new(value))))
                .collect(),
        }
    }
}

impl std::fmt::Debug for EncryptionConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EncryptionConfig")
            .field("active_key_id", &self.active_key_id)
            .field("key_ids", &self.keys.keys().collect::<Vec<_>>())
            .field("keys", &"[REDACTED]")
            .finish()
    }
}
