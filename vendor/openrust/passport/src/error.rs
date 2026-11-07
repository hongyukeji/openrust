//! OAuth 本地 JSON 错误响应；保留协议状态、字段与认证头，不套业务响应封装。
//
// 依据 thephpleague/oauth2-server 9.4.1 / 9d2f6fc0a0b5aa1bb02506971d3a4ecff2c6526c
// src/Exception/OAuthServerException.php 对应；以下保留上游归因与许可。
// MIT License
// Copyright (C) Alex Bilbie
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use super::pkce::PkceError;
#[path = "scope_hint.rs"]
mod scope_hint;
use openrust::http::axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

/// 本地 OAuth 错误；序列化只包含协议字段，不接受任意重定向地址。
///
/// scope 提示只在协议响应中包含转义后的 scope 标识符，调试输出省略提示。
#[derive(Clone, serde::Serialize)]
pub struct OAuthError {
    #[serde(skip)]
    status: StatusCode,
    error: &'static str,
    error_description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    #[serde(skip)]
    authenticate: Option<&'static str>,
}

impl OAuthError {
    /// 缺少有效 scope 标识符时使用固定的默认提示。
    pub fn invalid_scope() -> Self {
        Self::invalid_scope_for("")
    }

    pub(crate) fn invalid_scope_for(scope: &str) -> Self {
        let hint = if scope.is_empty() {
            "Specify a scope in the request or set a default scope".to_owned()
        } else {
            format!("Check the `{}` scope", scope_hint::escape(scope))
        };
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "The requested scope is invalid, unknown, or malformed",
            Some(&hint),
        )
    }

    pub(crate) fn payload_pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            Some(("error", self.error)),
            Some(("error_description", self.error_description.as_str())),
            self.hint.as_deref().map(|hint| ("hint", hint)),
        ]
        .into_iter()
        .flatten()
    }

    fn new(
        status: StatusCode,
        error: &'static str,
        description: impl Into<String>,
        hint: Option<&str>,
    ) -> Self {
        Self {
            status,
            error,
            error_description: description.into(),
            hint: hint.map(str::to_owned),
            authenticate: None,
        }
    }

    /// 无效请求；未提供 hint 时使用所指参数的固定提示。
    pub fn invalid_request(parameter: &str, hint: Option<&str>) -> Self {
        let hint = hint.map_or_else(
            || format!("Check the `{parameter}` parameter"),
            str::to_owned,
        );
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The request is missing a required parameter, includes an invalid parameter value, includes a parameter more than once, or is otherwise malformed.",
            Some(&hint),
        )
    }

    /// 客户端验证失败；仅非空 Authorization 原头决定认证方案，不裁剪或保存原值。
    pub fn invalid_client(authorization: Option<&[u8]>) -> Self {
        let mut error = Self::new(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "Client authentication failed",
            None,
        );
        error.authenticate = authorization
            .filter(|value| !value.is_empty())
            .map(|value| {
                if value.starts_with(b"Bearer") {
                    "Bearer realm=\"OAuth\""
                } else {
                    "Basic realm=\"OAuth\""
                }
            });
        error
    }

    /// 无效授权；空 hint 仍作为显式字段输出，以保持固定上游语义。
    pub fn invalid_grant(hint: &str) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "The provided authorization grant (e.g., authorization code, resource owner credentials) or refresh token is invalid, expired, revoked, does not match the redirection URI used in the authorization request, or was issued to another client.",
            Some(hint),
        )
    }

    /// refresh token 无效；缺失 hint 不输出该字段。
    pub fn invalid_refresh_token(hint: Option<&str>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "The refresh token is invalid.",
            hint,
        )
    }

    /// 资源所有者或授权服务器拒绝访问；此构造器只返回本地 JSON。
    pub fn access_denied(hint: Option<&str>) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "access_denied",
            "The resource owner or authorization server denied the request.",
            hint,
        )
    }

    /// 已验证的客户端无权使用此授权类型。
    pub fn unauthorized_client(hint: Option<&str>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "unauthorized_client",
            "The authenticated client is not authorized to use this authorization grant type.",
            hint,
        )
    }

    /// 授权类型不受支持，使用固定参数提示。
    pub fn unsupported_grant_type() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "The authorization grant type is not supported by the authorization server.",
            Some("Check that all required parameters have been provided"),
        )
    }

    /// 十次合法新标识符生成机会耗尽；固定League三种kind共用此错误名和说明。
    ///
    /// 不包含碰撞ID、SQL信息或hint；上游内部异常code100不是HTTP JSON字段。
    pub fn identifier_collision() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "access_token_duplicate",
            "Could not create unique access token identifier",
            None,
        )
    }

    /// 服务内部失败；只接收安全静态说明，拼入 description，不另设 hint 字段。
    pub fn server_error(safe_hint: &'static str) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            format!(
                "The authorization server encountered an unexpected condition which prevented it from fulfilling the request: {safe_hint}"
            ),
            None,
        )
    }
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.error_description)
    }
}

impl std::fmt::Debug for OAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OAuthError")
            .field("status", &self.status)
            .field("error", &self.error)
            .field("error_description", &self.error_description)
            .finish_non_exhaustive()
    }
}
impl std::error::Error for OAuthError {}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let authenticate = self.authenticate;
        let mut response = (self.status, Json(self)).into_response();
        if let Some(authenticate) = authenticate {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                openrust::http::axum::http::HeaderValue::from_static(authenticate),
            );
        }
        response
    }
}

impl From<PkceError> for OAuthError {
    fn from(error: PkceError) -> Self {
        match error {
            PkceError::MissingChallenge => Self::invalid_request(
                "code_challenge",
                Some("Code challenge must be provided for public clients"),
            ),
            PkceError::InvalidChallenge => Self::invalid_request(
                "code_challenge",
                Some("Code challenge must follow the specifications of RFC-7636."),
            ),
            PkceError::InvalidMethod => Self::invalid_request(
                "code_challenge_method",
                Some("Code challenge method must be one of `S256`, `plain`"),
            ),
            PkceError::MissingVerifier => Self::invalid_request("code_verifier", None),
            PkceError::InvalidVerifier => Self::invalid_request(
                "code_verifier",
                Some("Code Verifier must follow the specifications of RFC-7636."),
            ),
            PkceError::UnexpectedVerifier => Self::invalid_request(
                "code_challenge",
                Some("code_verifier received when no code_challenge is present"),
            ),
            PkceError::Mismatch => Self::invalid_grant("Failed to verify `code_verifier`."),
            // 安全适配：不回显持久化来源中的未知方法原值，保留 server_error 分类。
            PkceError::UnsupportedStoredMethod => {
                Self::server_error("Unsupported stored code challenge method")
            }
            PkceError::InternalValidation => Self::server_error("Code challenge validation failed"),
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
