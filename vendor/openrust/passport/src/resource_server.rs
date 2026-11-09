//! League `ResourceServer` 的资源凭据验证与请求属性投影。

use std::{fmt, sync::Arc};

use openrust::{
    container::ParameterValue,
    http::{
        Request,
        axum::http::{HeaderValue, header::AUTHORIZATION},
    },
    support::calendar::{CalendarClock as _, SystemClock},
};

use crate::{GrantStorage, OAuthAccessVerifier, OAuthError};

/// 验证资源请求上的 OAuth access token，并检查同一存储中的撤销状态。
pub struct ResourceServer {
    verifier: Arc<OAuthAccessVerifier>,
    storage: Arc<dyn GrantStorage>,
}

impl ResourceServer {
    /// 使用 OAuth 公钥和当前应用的权威令牌存储。
    #[must_use]
    pub fn new(verifier: Arc<OAuthAccessVerifier>, storage: Arc<dyn GrantStorage>) -> Self {
        Self { verifier, storage }
    }

    /// 校验首个 Authorization 请求头，成功后设置 League 的四个 `oauth_*` 请求属性。
    ///
    /// ID 属性保存 `String`，scope 保存 `Vec<String>`。令牌未包含 subject 或 scope 时
    /// 对应属性为显式 null。属性仅表示本次资源验证结果，用户解析和策略由调用方负责。
    /// 无效或已撤销的凭据返回 401。存储和运行时故障以脱敏错误阻断访问。
    pub async fn validate_authenticated_request(
        &self,
        request: &Request,
    ) -> Result<(), OAuthError> {
        let authorization = request
            .headers()
            .get(AUTHORIZATION)
            .map(HeaderValue::as_bytes);
        let token = bearer(authorization)?;
        check_compact_parts(token)?;
        let now = SystemClock
            .now()
            .map_err(|_| OAuthError::server_error("OAuth resource clock unavailable"))?;
        let claims = self
            .verifier
            .verify_resource_at(token, now)
            .map_err(|_| invalid_token())?;
        let client = claims.aud.into_iter().next().ok_or_else(invalid_token)?;
        if self
            .storage
            .is_access_revoked(&claims.jti)
            .await
            .map_err(|_| OAuthError::server_error("OAuth revocation lookup unavailable"))?
        {
            return Err(OAuthError::access_denied(Some(
                "Access token has been revoked",
            )));
        }
        for (key, value) in [
            ("oauth_access_token_id", ParameterValue::new(claims.jti)),
            ("oauth_client_id", ParameterValue::new(client)),
            (
                "oauth_user_id",
                claims
                    .sub
                    .map_or_else(ParameterValue::null, ParameterValue::new),
            ),
            (
                "oauth_scopes",
                claims
                    .scopes
                    .map_or_else(ParameterValue::null, ParameterValue::new),
            ),
        ] {
            request
                .attributes()
                .insert(key, value)
                .map_err(|_| OAuthError::server_error("OAuth request attributes unavailable"))?;
        }
        Ok(())
    }
}

impl fmt::Debug for ResourceServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResourceServer").finish_non_exhaustive()
    }
}

fn invalid_token() -> OAuthError {
    OAuthError::access_denied(Some("Access token could not be verified"))
}

fn check_compact_parts(token: &str) -> Result<(), OAuthError> {
    let mut parts = token.split('.');
    let hint = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(""), Some(_), Some(_), None) => "The JWT string is missing the Header part",
        (Some(_), Some(""), Some(_), None) => "The JWT string is missing the Claim part",
        (Some(_), Some(_), Some(""), None) => "The JWT string is missing the Signature part",
        (Some(_), Some(_), Some(_), None) => return Ok(()),
        _ => "The JWT string must have two dots",
    };
    Err(OAuthError::access_denied(Some(hint)))
}

fn bearer(authorization: Option<&[u8]>) -> Result<&str, OAuthError> {
    let value = authorization
        .ok_or_else(|| OAuthError::access_denied(Some("Missing \"Authorization\" header")))?;
    let leading = value
        .iter()
        .position(|byte| !pcre_space(*byte))
        .unwrap_or(value.len());
    let candidate = &value[leading..];
    // 固定 League 只去掉起始 Bearer 方案和一位空白，也接受首头中的裸 compact JWT。
    let raw = if candidate
        .get(..6)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"Bearer"))
        && candidate.get(6).is_some_and(|byte| pcre_space(*byte))
    {
        &candidate[7..]
    } else {
        value
    };
    let token = std::str::from_utf8(raw)
        .map_err(|_| invalid_token())?
        .trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\0']);
    if token.is_empty() {
        return Err(OAuthError::access_denied(Some("Missing \"Bearer\" token")));
    }
    Ok(token)
}

const fn pcre_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

#[cfg(test)]
#[path = "../tests/unit/resource_server.rs"]
pub(crate) mod tests;
