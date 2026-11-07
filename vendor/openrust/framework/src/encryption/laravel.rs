//! 由统一 Provider 以生效应用配置构造 Laravel 字符串加密服务。

use std::fmt;

use aws_lc_rs::hmac;
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

use crate::config::{AppConfig, app::AppCipher};
use crate::{Error, Result};

use super::laravel_payload;

const COOKIE_PREFIX_BYTES: usize = 41;
const HEX: &[u8; 16] = b"0123456789abcdef";

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

    // 使用当前密钥绑定实际出站 Cookie 名，再走同一 Laravel 字符串载荷。
    pub(crate) fn encrypt_cookie_value(&self, name: &str, value: &[u8]) -> Result<String> {
        if value.len() > laravel_payload::MAX_PLAINTEXT_BYTES - COOKIE_PREFIX_BYTES {
            return Err(Error::custom(
                "laravel_payload_encrypt_failed",
                "Cookie 明文超过加密限额",
            ));
        }
        let mut plain = Zeroizing::new(Vec::with_capacity(COOKIE_PREFIX_BYTES + value.len()));
        plain.extend_from_slice(&cookie_prefix(name, &self.key));
        plain.extend_from_slice(value);
        self.encrypt_string(&plain)
    }

    // 请求头在验证密文后仅剥去前缀。请求伪造层负责比对 Session token。
    #[cfg(feature = "session")]
    pub(crate) fn decrypt_xsrf_header(&self, wire: &str) -> Option<String> {
        let plain = Zeroizing::new(self.decrypt_string(wire).ok()?);
        let token = plain.get(COOKIE_PREFIX_BYTES..).unwrap_or_default();
        std::str::from_utf8(token).ok().map(str::to_owned)
    }

    // 已认证但前缀错配为叶值缺失。密文认证错误交上层废弃整个根 Cookie。
    pub(crate) fn decrypt_cookie_value(&self, name: &str, wire: &str) -> Result<Option<Vec<u8>>> {
        let plain = Zeroizing::new(self.decrypt_string(wire)?);
        let Some((prefix, value)) = plain.split_at_checked(COOKIE_PREFIX_BYTES) else {
            return Ok(None);
        };
        let mut valid = subtle::Choice::from(0);
        for key in std::iter::once(&self.key).chain(&self.previous_keys) {
            valid |= prefix.ct_eq(&cookie_prefix(name, key));
        }
        Ok(bool::from(valid).then(|| value.to_vec()))
    }
}

fn cookie_prefix(name: &str, key: &[u8]) -> [u8; COOKIE_PREFIX_BYTES] {
    // Laravel CookieValuePrefix 绑定的是名称与字面 v2；这里仅用于兼容现有 HMAC-SHA1 协议。
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
    let mut context = hmac::Context::with_key(&key);
    context.update(name.as_bytes());
    context.update(b"v2");
    let digest = context.sign();
    let mut prefix = [b'|'; COOKIE_PREFIX_BYTES];
    let (hex_chunks, _) = prefix.as_chunks_mut::<2>();
    for (byte, output) in digest.as_ref().iter().zip(hex_chunks) {
        output[0] = HEX[usize::from(byte >> 4)];
        output[1] = HEX[usize::from(byte & 0x0f)];
    }
    prefix
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
    fn cookie_prefix_binds_nested_name_and_accepts_current_or_historical_key() -> Result<()> {
        let name = "profile[options][theme]";
        let current_key = [0x15; 32];
        let previous_key = [0x42; 32];
        assert_eq!(
            &cookie_prefix(name, &current_key),
            b"4a5e95c703d0df51557444ad49ee7007a5ef263c|"
        );
        assert_eq!(
            &cookie_prefix(name, &previous_key),
            b"09aa735b3acbd428754ffda3615cfcc134dd8cd3|"
        );
        let make = |key: [u8; 32], previous_keys| {
            let key = String::from_utf8(key.to_vec()).map_err(Error::internal)?;
            LaravelEncrypter::from_config(AppConfig {
                key: Some(key),
                previous_keys,
                ..AppConfig::default()
            })
        };
        let current = make(current_key, vec!["B".repeat(32)])?;
        let previous = make(previous_key, Vec::new())?;
        let value = b"dark\0mode";
        for writer in [&current, &previous] {
            let wire = writer.encrypt_cookie_value(name, value)?;
            assert_eq!(
                current.decrypt_cookie_value(name, &wire)?.as_deref(),
                Some(value.as_slice())
            );
            assert!(current.decrypt_cookie_value("other", &wire)?.is_none());
        }
        // 载荷认证与前缀校验分别按完整密钥序列检查。
        let mut crossed = cookie_prefix(name, &previous_key).to_vec();
        crossed.extend_from_slice(value);
        let wire = current.encrypt_string(&crossed)?;
        assert_eq!(
            current.decrypt_cookie_value(name, &wire)?.as_deref(),
            Some(value.as_slice())
        );
        let empty = current.encrypt_cookie_value(name, b"")?;
        assert_eq!(
            current.decrypt_cookie_value(name, &empty)?,
            Some(Vec::new())
        );
        let no_prefix = current.encrypt_string(value)?;
        assert!(current.decrypt_cookie_value(name, &no_prefix)?.is_none());
        assert!(
            current
                .decrypt_cookie_value(name, "not a ciphertext")
                .is_err()
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    #[test]
    fn xsrf_header_removes_prefix_after_authentication() -> Result<()> {
        let encrypter = LaravelEncrypter::from_config(AppConfig {
            key: Some("A".repeat(32)),
            ..AppConfig::default()
        })?;
        for name in ["XSRF-TOKEN", "other"] {
            let wire = encrypter.encrypt_cookie_value(name, b"session-token")?;
            assert_eq!(
                encrypter.decrypt_xsrf_header(&wire).as_deref(),
                Some("session-token")
            );
            assert_eq!(
                encrypter
                    .decrypt_cookie_value("XSRF-TOKEN", &wire)?
                    .is_some(),
                name == "XSRF-TOKEN"
            );
        }
        let short = encrypter.encrypt_string(b"short")?;
        assert_eq!(encrypter.decrypt_xsrf_header(&short).as_deref(), Some(""));
        let invalid_utf8 = encrypter.encrypt_cookie_value("XSRF-TOKEN", &[0xff])?;
        assert!(encrypter.decrypt_xsrf_header(&invalid_utf8).is_none());
        assert!(encrypter.decrypt_xsrf_header("not a ciphertext").is_none());
        Ok(())
    }

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
