//! 授权码与刷新凭据的认证封装；复用框架加密器，不承诺 League PHP 密文字节兼容。

use std::sync::Arc;

use openrust::{Error, Result, encryption::ContextualEncrypter};
use serde::{Deserialize, Serialize};

use crate::{CodeChallenge, GrantAuthorityBinding};

const CODE_CONTEXT: &[u8] = b"openrust:passport:authorization-code:v1";
const REFRESH_CONTEXT: &[u8] = b"openrust:passport:refresh-token:v1";

/// 受认证密文保护的授权码绑定；不实现 Debug，避免日志披露挑战或用户标识。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodePayload {
    pub(crate) client_id: String,
    pub(crate) redirect_uri: Option<String>,
    pub(crate) auth_code_id: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) user_id: String,
    pub(crate) expire_time: i64,
    pub(crate) authority_binding: String,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

impl CodePayload {
    pub(crate) fn new(
        record: &crate::grant_records::CodeRecord,
        redirect_uri: Option<String>,
        challenge: Option<&CodeChallenge>,
    ) -> Self {
        let (code_challenge, code_challenge_method) = match challenge {
            Some(challenge) => {
                let (value, method) = challenge.stored_parts();
                (Some(value.to_owned()), Some(method.to_owned()))
            }
            None => (None, None),
        };
        Self {
            client_id: record.client_id.clone(),
            redirect_uri,
            auth_code_id: record.id.clone(),
            scopes: record.scopes.clone(),
            user_id: record.user_id.clone(),
            expire_time: record.expires_at,
            authority_binding: record.authority.as_str().to_owned(),
            code_challenge,
            code_challenge_method,
        }
    }

    pub(crate) fn authority(&self) -> Result<GrantAuthorityBinding> {
        GrantAuthorityBinding::new(self.authority_binding.clone())
    }

    /// 解密并不豁免格式校验；只通过同一请求校验器重新建立挑战。
    pub(crate) fn challenge(&self) -> Result<Option<CodeChallenge>> {
        match (&self.code_challenge, &self.code_challenge_method) {
            (None, None) => Ok(None),
            (Some(challenge), Some(method)) if matches!(method.as_str(), "S256" | "plain") => {
                CodeChallenge::from_request(Some(challenge), Some(method), true)
                    .map_err(|_| invalid_envelope())
            }
            _ => Err(invalid_envelope()),
        }
    }
}

/// 固定 League 刷新负载关联；不储存原始 bearer 或另建用户登录状态。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefreshPayload {
    pub(crate) client_id: String,
    pub(crate) refresh_token_id: String,
    pub(crate) access_token_id: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) user_id: String,
    pub(crate) expire_time: i64,
    pub(crate) authority_binding: String,
}

impl RefreshPayload {
    pub(crate) fn authority(&self) -> Result<GrantAuthorityBinding> {
        GrantAuthorityBinding::new(self.authority_binding.clone())
    }
}

#[derive(Clone)]
pub(crate) struct GrantTokenCodec {
    encryption: Arc<ContextualEncrypter>,
}

impl GrantTokenCodec {
    pub(crate) fn new(encryption: Arc<ContextualEncrypter>) -> Self {
        Self { encryption }
    }

    pub(crate) fn encode_code(&self, payload: &CodePayload) -> Result<String> {
        self.encode(payload, CODE_CONTEXT)
    }

    pub(crate) fn decode_code(&self, token: &str) -> Result<CodePayload> {
        let payload: CodePayload = self.decode(token, CODE_CONTEXT)?;
        payload.challenge()?;
        validate_id(&payload.auth_code_id)?;
        validate_identity(&payload.client_id)?;
        validate_identity(&payload.user_id)?;
        payload.authority()?;
        Ok(payload)
    }

    pub(crate) fn encode_refresh(&self, payload: &RefreshPayload) -> Result<String> {
        self.encode(payload, REFRESH_CONTEXT)
    }

    pub(crate) fn decode_refresh(&self, token: &str) -> Result<RefreshPayload> {
        let payload: RefreshPayload = self.decode(token, REFRESH_CONTEXT)?;
        validate_id(&payload.refresh_token_id)?;
        validate_id(&payload.access_token_id)?;
        validate_identity(&payload.client_id)?;
        validate_identity(&payload.user_id)?;
        payload.authority()?;
        Ok(payload)
    }

    fn encode<T: Serialize>(&self, payload: &T, context: &[u8]) -> Result<String> {
        let plaintext =
            zeroize::Zeroizing::new(serde_json::to_vec(payload).map_err(|_| invalid_envelope())?);
        self.encryption.encrypt_with_context(&plaintext, context)
    }

    fn decode<T: serde::de::DeserializeOwned>(&self, token: &str, context: &[u8]) -> Result<T> {
        let plaintext = zeroize::Zeroizing::new(
            self.encryption
                .decrypt_with_context(token, context)
                .map_err(|_| invalid_envelope())?,
        );
        serde_json::from_slice(&plaintext).map_err(|_| invalid_envelope())
    }
}

/// League 默认为40个随机字节的十六进制ID；OS错误不能降级成时间戳或UUID。
pub(crate) fn random_identifier() -> Result<String> {
    use std::fmt::Write as _;
    let mut bytes = [0_u8; 40];
    getrandom::fill(&mut bytes).map_err(|_| entropy_unavailable())?;
    let mut value = String::with_capacity(80);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").map_err(|_| entropy_unavailable())?;
    }
    Ok(value)
}

/// 生成Passport的40字符字母数字明文secret；赋值须走同一模型的hashed setter。
///
/// 仅生成随机值，不保存模型，不将持久hash作为明文返回；拒绝采样避免模偏差。
pub fn generate_client_secret() -> Result<String> {
    const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut secret = String::with_capacity(40);
    while secret.len() < 40 {
        let mut bytes = zeroize::Zeroizing::new([0_u8; 64]);
        getrandom::fill(bytes.as_mut()).map_err(|_| entropy_unavailable())?;
        for byte in bytes.iter().copied().filter(|byte| *byte < 248) {
            secret.push(char::from(ALPHABET[usize::from(byte % 62)]));
            if secret.len() == 40 {
                break;
            }
        }
    }
    Ok(secret)
}

fn validate_id(id: &str) -> Result<()> {
    if id.len() == 80 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(invalid_envelope())
    }
}

fn validate_identity(id: &str) -> Result<()> {
    if id.is_empty() {
        Err(invalid_envelope())
    } else {
        Ok(())
    }
}

fn invalid_envelope() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::BAD_REQUEST,
        "passport_credential_invalid",
        "OAuth凭据不可验证",
    )
}

fn entropy_unavailable() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "passport_entropy_unavailable",
        "OAuth安全随机源不可用",
    )
}

#[cfg(test)]
#[path = "protocol_token_tests.rs"]
mod tests;
