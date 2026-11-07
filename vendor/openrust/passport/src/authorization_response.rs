//! 只对已验证注册目标构造OAuth回调；未验证地址永不进入Location。

use openrust::http::axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{OAuthError, ValidatedRedirectUri};

/// 完成或拒绝授权后的回调；仅由服务器从已验证请求建立。
pub struct AuthorizationRedirect {
    target: String,
}

impl AuthorizationRedirect {
    pub(crate) fn approved(target: &ValidatedRedirectUri, code: &str, state: Option<&str>) -> Self {
        let mut parameters = vec![("code", code)];
        if let Some(state) = state {
            parameters.push(("state", state));
        }
        Self {
            target: append_parameters(target.as_str(), &parameters),
        }
    }

    pub(crate) fn denied(target: &ValidatedRedirectUri, state: Option<&str>) -> Self {
        let parameters = [
            ("error", "access_denied"),
            (
                "error_description",
                "The resource owner or authorization server denied the request.",
            ),
            ("hint", "The user denied the request"),
        ];
        Self::error(target, state, &parameters)
    }

    pub(crate) fn invalid_scope(
        target: &ValidatedRedirectUri,
        state: Option<&str>,
        error: &OAuthError,
    ) -> Self {
        let parameters: Vec<_> = error.payload_pairs().collect();
        Self::error(target, state, &parameters)
    }

    fn error(
        target: &ValidatedRedirectUri,
        state: Option<&str>,
        parameters: &[(&str, &str)],
    ) -> Self {
        // 上游先生成携带 state 的回调地址，再由异常响应追加错误字段。
        let state = state.map(|value| ("state", value));
        let target = append_parameters(target.as_str(), state.as_slice());
        Self {
            target: append_parameters(&target, parameters),
        }
    }

    /// 已编码的完整回调URL；仅供实际HTTP适配层设置Location。
    #[must_use]
    pub fn location(&self) -> &str {
        &self.target
    }
}

impl std::fmt::Debug for AuthorizationRedirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuthorizationRedirect([REDACTED])")
    }
}

impl IntoResponse for AuthorizationRedirect {
    fn into_response(self) -> Response {
        let Ok(location) = HeaderValue::from_str(&self.target) else {
            return OAuthError::server_error("Invalid registered redirect response")
                .into_response();
        };
        let mut response = StatusCode::FOUND.into_response();
        response.headers_mut().insert(header::LOCATION, location);
        response
    }
}

/// 授权请求的本地协议错误，或只指向已校验目标的协议重定向。
pub enum AuthorizationFailure {
    /// 未建立可信回调目标，或该错误按固定上游只能返回本地JSON。
    Local(OAuthError),
    /// 已验证目标上的拒绝或scope错误。
    Redirect(AuthorizationRedirect),
}

impl From<OAuthError> for AuthorizationFailure {
    fn from(error: OAuthError) -> Self {
        Self::Local(error)
    }
}
impl IntoResponse for AuthorizationFailure {
    fn into_response(self) -> Response {
        match self {
            Self::Local(error) => error.into_response(),
            Self::Redirect(redirect) => redirect.into_response(),
        }
    }
}

// 固定AbstractAuthorizeGrant直接按?是否存在追加，不重写已注册地址的字节。
fn append_parameters(target: &str, parameters: &[(&str, &str)]) -> String {
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(parameters.iter().copied())
        .finish()
        // WHATWG 表单编码保留星号，PHP 默认的 RFC 1738 查询编码转义星号。
        .replace('*', "%2A");
    format!(
        "{target}{}{encoded}",
        if target.contains('?') { '&' } else { '?' }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_encodes_code_state_without_changing_registered_query()
    -> Result<(), crate::RedirectUriError> {
        let target = ValidatedRedirectUri::from_authorization_request(
            Some("https://rp.test/cb?existing=1"),
            &["https://rp.test/cb?existing=1".into()],
        )?;
        let response =
            AuthorizationRedirect::approved(&target, "code&injected=true", Some("state = +"));
        assert_eq!(
            response.location(),
            "https://rp.test/cb?existing=1&code=code%26injected%3Dtrue&state=state+%3D+%2B"
        );
        assert!(!format!("{response:?}").contains("code&"));
        Ok(())
    }

    #[test]
    fn callback_uses_php_query_encoding_and_error_append_order()
    -> Result<(), crate::RedirectUriError> {
        let target = ValidatedRedirectUri::from_authorization_request(
            None,
            &["https://rp.test/cb?existing=*".into()],
        )?;
        let state = "星*~ +%2A";
        let approved = AuthorizationRedirect::approved(&target, "code*", Some(state));
        assert_eq!(
            approved.location(),
            "https://rp.test/cb?existing=*&code=code%2A&state=%E6%98%9F%2A%7E+%2B%252A"
        );
        let denied = AuthorizationRedirect::denied(&target, Some(state));
        assert!(denied.location().starts_with(
            "https://rp.test/cb?existing=*&state=%E6%98%9F%2A%7E+%2B%252A&error=access_denied&"
        ));
        let target =
            ValidatedRedirectUri::from_authorization_request(None, &["https://rp.test/cb".into()])?;
        let invalid =
            AuthorizationRedirect::invalid_scope(&target, None, &OAuthError::invalid_scope());
        assert!(
            invalid
                .location()
                .starts_with("https://rp.test/cb?&error=invalid_scope&")
        );
        Ok(())
    }
}
