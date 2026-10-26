//! API response configuration.

use serde::{Deserialize, Serialize};

/// `[api]` section: JSON response policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    /// HTTP status policy for framework JSON responses.
    pub http_status: HttpStatusMode,
}

/// HTTP status policy for framework JSON responses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpStatusMode {
    /// Always return HTTP 200 and use the response code in the body.
    #[default]
    AlwaysOk,
    /// Preserve semantic HTTP status codes alongside the response body.
    Semantic,
}
