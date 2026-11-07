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

/// Laravel 应用级加密服务的可替换契约。
///
/// 字符串方法处理原始字节。值方法默认使用 Laravel 对 PHP 字符串值的序列化格式。
/// 密钥读取用于 Cookie 名称认证及密钥轮换，调用方不得记录或序列化密钥。
pub trait ApplicationEncrypter: Send + Sync {
    /// 使用当前密钥加密原始字节，返回应用加密载荷。
    fn encrypt_string(&self, plaintext: &[u8]) -> Result<String>;

    /// 认证并解密应用加密载荷，返回原始字节。
    fn decrypt_string(&self, payload: &str) -> Result<Vec<u8>>;

    /// 借用当前加密密钥的原始字节。
    fn current_key(&self) -> &[u8];

    /// 按配置顺序借用历史解密密钥。
    fn previous_keys(&self) -> Vec<&[u8]>;

    /// 按当前、历史的顺序借用所有密钥。
    fn all_keys(&self) -> Vec<&[u8]> {
        let mut keys = self.previous_keys();
        keys.insert(0, self.current_key());
        keys
    }

    /// 按 PHP 字符串值格式加密原始字节，返回 Laravel Base64 载荷。
    ///
    /// 长度字段记录字节数。加密上限包含序列化前缀及结束符。
    fn encrypt_serialized_string(&self, plaintext: &[u8]) -> Result<String> {
        let length = plaintext.len().to_string();
        let framing = length.len() + 6;
        if plaintext.len() > laravel_payload::MAX_PLAINTEXT_BYTES - framing {
            return Err(Error::custom(
                "laravel_payload_encrypt_failed",
                "序列化字符串超过加密限额",
            ));
        }
        let mut value = Zeroizing::new(Vec::with_capacity(plaintext.len() + framing));
        value.extend_from_slice(b"s:");
        value.extend_from_slice(length.as_bytes());
        value.extend_from_slice(b":\"");
        value.extend_from_slice(plaintext);
        value.extend_from_slice(b"\";");
        self.encrypt_string(&value)
    }

    /// 认证并解密 PHP 字符串值，返回按析构清零的原始字节。
    ///
    /// 字符串长度和结束符必须与整个已认证正文一致。
    fn decrypt_serialized_string(&self, payload: &str) -> Result<Zeroizing<Vec<u8>>> {
        let serialized = Zeroizing::new(self.decrypt_string(payload)?);
        let value = serialized_string_value(&serialized).ok_or_else(|| {
            Error::custom(
                "laravel_serialized_string_invalid",
                "已认证载荷不是有效的 PHP 字符串值",
            )
        })?;
        Ok(Zeroizing::new(value.to_vec()))
    }
}

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
    pub(crate) fn from_config(config: AppConfig) -> Result<Self> {
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
        laravel_payload::decrypt_string(self.cipher, &self.all_keys(), payload)
    }

    /// 按 PHP 字符串值格式加密原始字节，返回 Laravel Base64 载荷。
    pub fn encrypt_serialized_string(&self, plaintext: &[u8]) -> Result<String> {
        ApplicationEncrypter::encrypt_serialized_string(self, plaintext)
    }

    /// 认证并解密 PHP 字符串值，返回按析构清零的原始字节。
    pub fn decrypt_serialized_string(&self, payload: &str) -> Result<Zeroizing<Vec<u8>>> {
        ApplicationEncrypter::decrypt_serialized_string(self, payload)
    }
}

// 使用当前密钥绑定实际出站 Cookie 名，再走同一 Laravel 字符串载荷。
pub(crate) fn encrypt_cookie_value(
    encrypter: &dyn ApplicationEncrypter,
    name: &str,
    value: &[u8],
) -> Result<String> {
    if value.len() > laravel_payload::MAX_PLAINTEXT_BYTES - COOKIE_PREFIX_BYTES {
        return Err(Error::custom(
            "laravel_payload_encrypt_failed",
            "Cookie 明文超过加密限额",
        ));
    }
    let mut plain = Zeroizing::new(Vec::with_capacity(COOKIE_PREFIX_BYTES + value.len()));
    plain.extend_from_slice(&cookie_prefix(name, encrypter.current_key()));
    plain.extend_from_slice(value);
    encrypter.encrypt_string(&plain)
}

// 请求头在验证密文后仅剥去前缀。请求伪造层负责比对 Session token。
#[cfg(feature = "session")]
pub(crate) fn decrypt_xsrf_header(
    encrypter: &dyn ApplicationEncrypter,
    wire: &str,
) -> Option<String> {
    let plain = Zeroizing::new(encrypter.decrypt_string(wire).ok()?);
    let token = plain.get(COOKIE_PREFIX_BYTES..).unwrap_or_default();
    std::str::from_utf8(token).ok().map(str::to_owned)
}

// 已认证但前缀错配为叶值缺失。密文认证错误交上层废弃整个根 Cookie。
pub(crate) fn decrypt_cookie_value(
    encrypter: &dyn ApplicationEncrypter,
    name: &str,
    wire: &str,
) -> Result<Option<Vec<u8>>> {
    let plain = Zeroizing::new(encrypter.decrypt_string(wire)?);
    let Some((prefix, value)) = plain.split_at_checked(COOKIE_PREFIX_BYTES) else {
        return Ok(None);
    };
    let mut valid = subtle::Choice::from(0);
    for key in encrypter.all_keys() {
        valid |= prefix.ct_eq(&cookie_prefix(name, key));
    }
    Ok(bool::from(valid).then(|| value.to_vec()))
}

impl ApplicationEncrypter for LaravelEncrypter {
    fn encrypt_string(&self, plaintext: &[u8]) -> Result<String> {
        Self::encrypt_string(self, plaintext)
    }

    fn decrypt_string(&self, payload: &str) -> Result<Vec<u8>> {
        Self::decrypt_string(self, payload)
    }

    fn current_key(&self) -> &[u8] {
        self.key.as_slice()
    }

    fn previous_keys(&self) -> Vec<&[u8]> {
        self.previous_keys
            .iter()
            .map(|key| key.as_slice())
            .collect()
    }
}

fn serialized_string_value(serialized: &[u8]) -> Option<&[u8]> {
    let framed = serialized.strip_prefix(b"s:")?;
    let separator = framed.iter().position(|byte| *byte == b':')?;
    let (length, value) = framed.split_at(separator);
    if length.is_empty() || !length.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let length = std::str::from_utf8(length).ok()?.parse::<usize>().ok()?;
    let value = value.strip_prefix(b":\"")?.strip_suffix(b"\";")?;
    (value.len() == length).then_some(value)
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
            let wire = encrypt_cookie_value(writer, name, value)?;
            assert_eq!(
                decrypt_cookie_value(&current, name, &wire)?.as_deref(),
                Some(value.as_slice())
            );
            assert!(decrypt_cookie_value(&current, "other", &wire)?.is_none());
        }
        // 载荷认证与前缀校验分别按完整密钥序列检查。
        let mut crossed = cookie_prefix(name, &previous_key).to_vec();
        crossed.extend_from_slice(value);
        let wire = current.encrypt_string(&crossed)?;
        assert_eq!(
            decrypt_cookie_value(&current, name, &wire)?.as_deref(),
            Some(value.as_slice())
        );
        let empty = encrypt_cookie_value(&current, name, b"")?;
        assert_eq!(
            decrypt_cookie_value(&current, name, &empty)?,
            Some(Vec::new())
        );
        let no_prefix = current.encrypt_string(value)?;
        assert!(decrypt_cookie_value(&current, name, &no_prefix)?.is_none());
        assert!(decrypt_cookie_value(&current, name, "not a ciphertext").is_err());
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
            let wire = encrypt_cookie_value(&encrypter, name, b"session-token")?;
            assert_eq!(
                decrypt_xsrf_header(&encrypter, &wire).as_deref(),
                Some("session-token")
            );
            assert_eq!(
                decrypt_cookie_value(&encrypter, "XSRF-TOKEN", &wire)?.is_some(),
                name == "XSRF-TOKEN"
            );
        }
        let short = encrypter.encrypt_string(b"short")?;
        assert_eq!(decrypt_xsrf_header(&encrypter, &short).as_deref(), Some(""));
        let invalid_utf8 = encrypt_cookie_value(&encrypter, "XSRF-TOKEN", &[0xff])?;
        assert!(decrypt_xsrf_header(&encrypter, &invalid_utf8).is_none());
        assert!(decrypt_xsrf_header(&encrypter, "not a ciphertext").is_none());
        Ok(())
    }

    #[test]
    fn laravel_serialized_string_uses_byte_lengths_and_authenticates_before_parsing() -> Result<()>
    {
        let old = LaravelEncrypter::from_config(AppConfig {
            key: Some("B".repeat(32)),
            ..AppConfig::default()
        })?;
        let reader = LaravelEncrypter::from_config(AppConfig {
            key: Some("A".repeat(32)),
            previous_keys: vec!["B".repeat(32)],
            ..AppConfig::default()
        })?;
        let bytes = b"\xe9\x9b\xaa\0+\"";
        let framed = b"s:6:\"\xe9\x9b\xaa\0+\"\";";
        for writer in [&old, &reader] {
            let wire = writer.encrypt_serialized_string(bytes)?;
            assert_eq!(&*reader.decrypt_serialized_string(&wire)?, bytes);
            assert_eq!(reader.decrypt_string(&wire)?, framed);
        }
        let empty = reader.encrypt_serialized_string(b"")?;
        assert_eq!(&*reader.decrypt_serialized_string(&empty)?, b"");

        for invalid in [
            b"s:2:\"x\";".as_slice(),
            b"s:+1:\"x\";",
            b"s:1:\"x\";trailing",
            b"s:99999999999999999999999999:\"x\";",
            b"a:0:{}",
            b"raw string",
        ] {
            let wire = reader.encrypt_string(invalid)?;
            let Err(error) = reader.decrypt_serialized_string(&wire) else {
                return Err(Error::custom("test_expected_failure", "无效字符串帧被接受"));
            };
            assert_eq!(error.code(), "laravel_serialized_string_invalid");
        }
        assert!(reader.decrypt_serialized_string("invalid wire").is_err());
        let oversized = vec![0; laravel_payload::MAX_PLAINTEXT_BYTES];
        let Err(error) = reader.encrypt_serialized_string(&oversized) else {
            return Err(Error::custom(
                "test_expected_failure",
                "超长序列化正文被接受",
            ));
        };
        assert_eq!(error.code(), "laravel_payload_encrypt_failed");
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
