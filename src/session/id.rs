//! Session ID 与 CSRF token 使用相同的固定源随机字母数字集合。

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Result};

const LENGTH: usize = 40;
const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// 大小写敏感的40位 ASCII 字母数字 Session ID。
#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SessionId(String);

impl SessionId {
    /// 使用系统随机源生成ID；拒绝采样避免字符分布偏差。
    pub fn generate() -> Result<Self> {
        Ok(Self(random_token()?))
    }

    /// 校验一个完整ID；不回显不合法的凭据。
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() != LENGTH || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(Error::custom_with_status(
                http::StatusCode::BAD_REQUEST,
                "session_id_invalid",
                "Session ID is invalid",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    /// 取得完整的大小写敏感ID；调用方不得将其写入日志。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionId([REDACTED])")
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SessionId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

impl Serialize for SessionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(|_| serde::de::Error::custom("Session ID is invalid"))
    }
}

pub(super) fn random_token() -> Result<String> {
    let mut output = String::with_capacity(LENGTH);
    // 每批64字节，最多八批；随机源异常不能形成无界CPU循环。
    for _ in 0..8 {
        let mut bytes = [0_u8; 64];
        getrandom::fill(&mut bytes).map_err(Error::internal)?;
        for byte in bytes {
            if byte < 248 {
                output.push(char::from(ALPHABET[usize::from(byte % 62)]));
                if output.len() == LENGTH {
                    return Ok(output);
                }
            }
        }
    }
    Err(Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "session_random_unavailable",
        "Session randomness is unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_exact_case_sensitive_and_redacted() -> Result<()> {
        let lower = SessionId::parse(&"a".repeat(40))?;
        let upper = SessionId::parse(&"A".repeat(40))?;
        assert_ne!(lower, upper);
        assert!(!format!("{lower:?}").contains(lower.as_str()));
        for invalid in [
            "a".repeat(39),
            "a".repeat(41),
            "_".repeat(40),
            "é".repeat(20),
        ] {
            assert!(SessionId::parse(&invalid).is_err());
        }
        let first = SessionId::generate()?;
        let second = SessionId::generate()?;
        assert_ne!(first, second);
        assert_eq!(first.as_str().len(), 40);
        assert!(
            first
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        );
        Ok(())
    }
}
