//! HTTP server and middleware configuration.

use serde::{Deserialize, Serialize};

/// `[server]` section: HTTP listener settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Interface to bind.
    pub host: String,
    /// TCP port to bind.
    pub port: u16,
    /// Per-request timeout in seconds; exceeding it yields `408 Request Timeout`.
    pub request_timeout_secs: u64,
    /// Grace period for in-flight requests after a shutdown signal.
    pub shutdown_timeout_secs: u64,
    /// Proxy IP addresses or CIDRs allowed to supply `X-Forwarded-For`.
    ///
    /// An empty list ignores all forwarding headers and uses the socket peer address.
    pub trusted_proxy_ranges: Vec<String>,
    /// Response compression settings.
    pub compression: CompressionConfig,
    /// Cross-origin request settings.
    pub cors: CorsConfig,
}

impl ServerConfig {
    /// `host:port` suitable for binding a listener.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: 3000,
            request_timeout_secs: 30,
            shutdown_timeout_secs: 10,
            trusted_proxy_ranges: Vec::new(),
            compression: CompressionConfig::default(),
            cors: CorsConfig::default(),
        }
    }
}

/// `[server.compression]` section: negotiated gzip and Brotli responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressionConfig {
    /// Enables response compression.
    pub enabled: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `[server.cors]` section: cross-origin response policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CorsConfig {
    /// Enables CORS responses.
    pub enabled: bool,
    /// Exact origins, or `["*"]` for every origin.
    pub allowed_origins: Vec<String>,
    /// Methods permitted in preflight requests.
    pub allowed_methods: Vec<String>,
    /// Request headers permitted in preflight requests.
    pub allowed_headers: Vec<String>,
    /// Response headers visible to browser clients.
    pub expose_headers: Vec<String>,
    /// Allows browser clients to send credentials.
    pub allow_credentials: bool,
    /// Preflight cache lifetime in seconds.
    pub max_age_secs: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_origins: Vec::new(),
            allowed_methods: ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]
                .map(str::to_owned)
                .to_vec(),
            allowed_headers: ["content-type", "authorization", "x-request-id"]
                .map(str::to_owned)
                .to_vec(),
            expose_headers: vec!["x-request-id".to_owned()],
            allow_credentials: false,
            max_age_secs: 600,
        }
    }
}
