//! 由统一 Provider 以生效应用配置构造 Laravel 字符串加密服务。

use std::fmt;

use zeroize::Zeroizing;

use crate::config::{AppConfig, app::AppCipher};
use crate::{Error, Result};

use super::laravel_payload;

/// 对应 Laravel `encryptString` 与 `decryptString` 的应用加密服务。
///
/// 由 [`super::EncryptionServiceProvider`] 延迟装配。新密文使用当前应用密钥；
/// 解密依次尝试当前和历史密钥，均受同一 cipher 约束。
pub struct LaravelEncrypter {
    cipher: AppCipher,
    key: Zeroizing<Vec<u8>>,
    previous_keys: Vec<Zeroizing<Vec<u8>>>,
}

impl LaravelEncrypter {
    pub(super) fn from_config(config: AppConfig) -> Result<Self> {
        // 先接管本次配置快照中的每个临时密钥，任一校验提前返回都要清理其余值。
        let key = config.key.map(Zeroizing::new);
        let previous_keys: Vec<_> = config
            .previous_keys
            .into_iter()
            .map(Zeroizing::new)
            .collect();
        let key = parse_key(key.as_deref().map_or("", String::as_str))?;
        let cipher = AppCipher::parse(&config.cipher).ok_or_else(|| {
            Error::custom("laravel_cipher_unsupported", "Laravel 加密算法不受支持")
        })?;
        validate_key_length(&key, cipher)?;
        let previous_keys = previous_keys
            .iter()
            .map(|raw| parse_key(raw))
            .collect::<Result<Vec<_>>>()?;
        for key in &previous_keys {
            validate_key_length(key, cipher)?;
        }
        Ok(Self {
            cipher,
            key,
            previous_keys,
        })
    }

    /// 加密至多 1 MiB 的原始字节，返回 Laravel Base64 载荷。
    ///
    /// 字节保持原样，支持非 UTF-8 明文。
    pub fn encrypt_string(&self, plaintext: &[u8]) -> Result<String> {
        laravel_payload::encrypt_string(self.cipher, &self.key, plaintext)
    }

    /// 验证并解密 Laravel 字符串载荷，返回原始字节。
    pub fn decrypt_string(&self, payload: &str) -> Result<Vec<u8>> {
        let mut keys = Vec::with_capacity(self.previous_keys.len() + 1);
        keys.push(self.key.as_slice());
        keys.extend(self.previous_keys.iter().map(|key| key.as_slice()));
        laravel_payload::decrypt_string(self.cipher, &keys, payload)
    }
}

impl fmt::Debug for LaravelEncrypter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LaravelEncrypter")
            .field("previous_key_count", &self.previous_keys.len())
            .finish_non_exhaustive()
    }
}

fn parse_key(raw: &str) -> Result<Zeroizing<Vec<u8>>> {
    if matches!(raw, "" | "0") {
        return Err(Error::custom("laravel_app_key_missing", "应用加密密钥缺失"));
    }
    if let Some(encoded) = raw.strip_prefix("base64:") {
        laravel_payload::decode_php_base64_key(encoded)
            .ok_or_else(|| Error::custom("laravel_app_key_invalid", "应用加密密钥解码失败"))
    } else {
        Ok(Zeroizing::new(raw.as_bytes().to_vec()))
    }
}

fn validate_key_length(bytes: &[u8], cipher: AppCipher) -> Result<()> {
    if bytes.len() != cipher.key_size() {
        return Err(Error::custom(
            "laravel_app_key_invalid",
            "应用加密密钥长度与算法不匹配",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_historical_keys_before_validating_their_lengths() -> Result<()> {
        for missing in ["", "0"] {
            let config = AppConfig {
                key: Some("0123456789abcdef".to_owned()),
                cipher: "aes-128-cbc".to_owned(),
                previous_keys: vec!["short".to_owned(), missing.to_owned()],
                ..AppConfig::default()
            };
            let Err(error) = LaravelEncrypter::from_config(config) else {
                return Err(Error::custom("test_expected_failure", "空历史密钥被接受"));
            };
            assert_eq!(error.code(), "laravel_app_key_missing");
        }
        Ok(())
    }
}
