//! API 响应配置。

use serde::{Deserialize, Serialize};

/// `[api]` 区段：JSON 响应策略。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    /// 框架 JSON 响应的 HTTP 状态码策略。
    pub http_status: HttpStatusMode,
}

/// 框架 JSON 响应的 HTTP 状态码策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpStatusMode {
    /// 始终返回 HTTP 200，并在响应正文中提供业务响应码；仅在应用显式选择时使用。
    AlwaysOk,
    /// 保留语义 HTTP 状态码，同时返回响应正文。
    #[default]
    Semantic,
}
