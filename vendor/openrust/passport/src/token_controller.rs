//! Token 端点的有限 HTTP 适配；正文只使用框架已提取的输入 bags。

use std::sync::Arc;

use openrust::http::{
    Request,
    axum::response::{IntoResponse, Response},
};
use serde_json::{Map, Value};

use crate::{AuthorizationServer, OAuthParameters};

/// 将同请求作用域的授权服务器接入显式注册的 Token 端点。
///
/// 不创建作用域、存储或身份权威，也不注册路由或引入 Session/Cookie 身份兜底。
/// 调用者须传入当前请求作用域解析的唯一服务器。
pub struct AccessTokenController {
    server: Arc<AuthorizationServer>,
}

impl AccessTokenController {
    /// 使用当前请求作用域的服务器构造适配器。
    #[must_use]
    pub fn new(server: Arc<AuthorizationServer>) -> Self {
        Self { server }
    }

    /// 处理已由框架执行预算及规范化的请求，直接返回 OAuth 协议响应。
    ///
    /// 框架输入错误保留为框架错误；首Authorization保原bytes，协议错误保留其状态、载荷与头。
    /// 空 JSON bag 的原文回读分支尚未实现，此处不再解析正文。
    pub async fn issue_token(&self, request: &Request) -> openrust::Result<Response> {
        let parameters = OAuthParameters::new(body_parameters(request)?);
        let authorization = request
            .headers()
            .get(openrust::http::axum::http::header::AUTHORIZATION)
            .map(openrust::http::axum::http::HeaderValue::as_bytes);
        Ok(match self.server.token(&parameters, authorization).await {
            Ok(response) => response.into_response(),
            Err(error) => error.into_response(),
        })
    }
}

impl std::fmt::Debug for AccessTokenController {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessTokenController")
            .finish_non_exhaustive()
    }
}

// 完整枚举而非 OAuth 字段白名单；post 只取表单直接键，query/file-only 键不会注入值。
fn body_parameters(request: &Request) -> openrust::Result<Map<String, Value>> {
    if request.is_json()? {
        return Ok(match request.deserialize_json::<Value>()? {
            Value::Object(values) => values,
            Value::Array(values) => values
                .into_iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), value))
                .collect(),
            // Core 已执行 PHP 顶层 array cast；保留同样投影以防输入实现返回标量。
            Value::Null => Map::new(),
            scalar => Map::from_iter([("0".to_owned(), scalar)]),
        });
    }
    Ok(request
        .keys()
        .into_iter()
        .filter_map(|key| request.post(&key).map(|value| (key, value)))
        .collect())
}

#[cfg(test)]
#[path = "token_controller_tests.rs"]
mod tests;
