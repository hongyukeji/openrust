//! OAuth 成功响应的顶层 JSON 与禁止缓存响应头，不套业务响应封装。

use openrust::http::axum::{
    Json,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};

/// OAuth 令牌成功响应，令牌字段仅供显式序列化或读取，不实现调试输出。
///
/// 固定 League 默认提供 Bearer 类型与顶层令牌字段。HTTP 缓存头采用 Laravel
/// Passport 路径的最终 `no-store, private`：League 原始响应为 `no-store`，
/// `private` 对应 Laravel HTTP 响应层的缓存控制补充。
#[derive(serde::Serialize)]
pub struct TokenResponse {
    token_type: &'static str,
    expires_in: u64,
    access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
}

impl TokenResponse {
    pub(crate) fn new(
        access_token: String,
        expires_in: u64,
        refresh_token: Option<String>,
    ) -> Self {
        Self {
            token_type: "Bearer",
            expires_in,
            access_token,
            refresh_token,
        }
    }

    // 消费原响应，仅重算提交后有效期，避免复制敏感 bearer 字符串。
    pub(crate) fn with_expires_in(mut self, expires_in: u64) -> Self {
        self.expires_in = expires_in;
        self
    }

    /// 返回固定的 Bearer 令牌类型。
    #[must_use]
    pub fn token_type(&self) -> &'static str {
        self.token_type
    }

    /// 返回访问令牌剩余有效秒数。
    #[must_use]
    pub fn expires_in(&self) -> u64 {
        self.expires_in
    }

    /// 显式读取访问令牌；调用方不得将其写入日志。
    #[must_use]
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// 显式读取可选刷新令牌；未签发时不序列化该字段。
    #[must_use]
    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }
}

impl IntoResponse for TokenResponse {
    fn into_response(self) -> Response {
        let mut response = Json(self).into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=UTF-8"),
        );
        headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        );
        response
    }
}

#[cfg(test)]
#[path = "token_response_tests.rs"]
mod tests;
