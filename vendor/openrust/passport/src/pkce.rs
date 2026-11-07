//! 固定 League 授权码流程的 PKCE 校验；错误和调试输出不包含挑战或验证值。

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use pcre2::bytes::Regex;
use sha2::{Digest, Sha256};
use std::fmt;
use subtle::ConstantTimeEq as _;

/// 授权码请求支持的挑战方法；序列化名称与 OAuth 参数一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CodeChallengeMethod {
    /// 直接以恒定时间比较原始验证值与挑战。
    #[serde(rename = "plain")]
    Plain,
    /// 比较 SHA-256 后无填充的 Base64 URL 安全编码。
    #[serde(rename = "S256")]
    S256,
}

/// PKCE 的协议失败类别；HTTP 状态与 OAuth 错误映射由调用层负责。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PkceError {
    /// 公开客户端未提供必须的挑战。
    #[error("公开客户端必须提供代码挑战")]
    MissingChallenge,
    /// 挑战不匹配固定上游的字符和长度表达式。
    #[error("代码挑战格式无效")]
    InvalidChallenge,
    /// 请求指定了不支持的非空挑战方法。
    #[error("代码挑战方法无效")]
    InvalidMethod,
    /// 已绑定挑战的授权码交换缺少验证值。
    #[error("缺少代码验证值")]
    MissingVerifier,
    /// 验证值不匹配固定上游的字符和长度表达式。
    #[error("代码验证值格式无效")]
    InvalidVerifier,
    /// 未绑定挑战的授权码交换却提交验证值。
    #[error("无代码挑战时不允许提交验证值")]
    UnexpectedVerifier,
    /// 格式有效但验证值与已绑定挑战不匹配。
    #[error("代码验证值与挑战不匹配")]
    Mismatch,
    /// 持久化挑战使用了当前服务无法识别的方法。
    #[error("已保存的代码挑战方法不受支持")]
    UnsupportedStoredMethod,
    /// 固定正则无法编译或执行；不得视为普通匹配失败。
    #[error("代码挑战校验器内部失败")]
    InternalValidation,
}

/// 已按请求层规则裁剪并验证格式的代码挑战。
#[derive(Clone)]
pub struct CodeChallenge {
    challenge: String,
    method: CodeChallengeMethod,
}

impl fmt::Debug for CodeChallenge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeChallenge")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl CodeChallenge {
    /// 已验证挑战的持久化快照；仅供本包的认证密文封装使用。
    pub(crate) fn stored_parts(&self) -> (&str, &'static str) {
        let method = match self.method {
            CodeChallengeMethod::Plain => "plain",
            CodeChallengeMethod::S256 => "S256",
        };
        (&self.challenge, method)
    }

    /// 从授权请求建立挑战；公开客户端必须提供，保密客户端可不启用 PKCE。
    ///
    /// 参数先按 PHP `trim` 的固定六字符集合裁剪；空挑战视为未提供，空方法默认 `plain`。
    /// 存在挑战时先校验方法，再复用上游 PCRE 表达式验证格式。
    pub fn from_request(
        challenge: Option<&str>,
        method: Option<&str>,
        public_client: bool,
    ) -> Result<Option<Self>, PkceError> {
        let Some(challenge) = request_parameter(challenge) else {
            return if public_client {
                Err(PkceError::MissingChallenge)
            } else {
                Ok(None)
            };
        };
        let method = match request_parameter(method).unwrap_or("plain") {
            "plain" => CodeChallengeMethod::Plain,
            "S256" => CodeChallengeMethod::S256,
            _ => return Err(PkceError::InvalidMethod),
        };
        if !matches_fixed_expression(challenge)? {
            return Err(PkceError::InvalidChallenge);
        }
        Ok(Some(Self {
            challenge: challenge.to_owned(),
            method,
        }))
    }

    /// 在授权码交换时裁剪并验证请求参数；只在格式有效后进行恒定时间比较。
    ///
    /// 与固定请求解析器一致，裁剪后为空视为缺失；S256 对裁剪后的字节求摘要。
    pub fn validate_verifier(&self, verifier: Option<&str>) -> Result<(), PkceError> {
        let verifier = request_parameter(verifier).ok_or(PkceError::MissingVerifier)?;
        if !matches_fixed_expression(verifier)? {
            return Err(PkceError::InvalidVerifier);
        }
        let matched = match self.method {
            CodeChallengeMethod::Plain => {
                bool::from(verifier.as_bytes().ct_eq(self.challenge.as_bytes()))
            }
            CodeChallengeMethod::S256 => {
                let encoded = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                bool::from(encoded.as_bytes().ct_eq(self.challenge.as_bytes()))
            }
        };
        if matched {
            Ok(())
        } else {
            Err(PkceError::Mismatch)
        }
    }
}

/// 验证授权码所绑定的可选挑战；无挑战时提交非空验证值必须拒绝，防止降级。
pub fn verify_optional_challenge(
    challenge: Option<&CodeChallenge>,
    verifier: Option<&str>,
) -> Result<(), PkceError> {
    match (challenge, request_parameter(verifier)) {
        (Some(challenge), verifier) => challenge.validate_verifier(verifier),
        (None, Some(_)) => Err(PkceError::UnexpectedVerifier),
        (None, None) => Ok(()),
    }
}

// PHP trim 默认集合不包含 form feed 或 Unicode 空白，不能使用 Rust str::trim。
use super::parameters::normalized as request_parameter;

fn matches_fixed_expression(value: &str) -> Result<bool, PkceError> {
    Regex::new("^[A-Za-z0-9-._~]{43,128}$")
        .map_err(|_| PkceError::InternalValidation)?
        .is_match(value.as_bytes())
        .map_err(|_| PkceError::InternalValidation)
}

#[cfg(test)]
#[path = "pkce_tests.rs"]
mod tests;
