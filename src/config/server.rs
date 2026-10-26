//! HTTP 服务器和 middleware 配置。

use serde::{Deserialize, Serialize};

/// `[server]` 区段：HTTP listener 设置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// 绑定的网络接口。
    pub host: String,
    /// 绑定的 TCP 端口。
    pub port: u16,
    /// 单个请求的超时时间（秒）；超过后返回 `408 Request Timeout`。
    pub request_timeout_secs: u64,
    /// 收到关闭信号后，为处理中请求保留的宽限时间。
    pub shutdown_timeout_secs: u64,
    /// 允许提供 `X-Forwarded-For` 的代理 IP 地址或 CIDR。
    ///
    /// 列表为空时忽略所有转发头，并使用 socket 对端地址。
    pub trusted_proxy_ranges: Vec<String>,
    /// 响应压缩设置。
    pub compression: CompressionConfig,
    /// 跨域请求设置。
    pub cors: CorsConfig,
}

impl ServerConfig {
    /// 适用于绑定 listener 的 `host:port`。
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

/// `[server.compression]` 区段：协商使用 gzip 和 Brotli 响应压缩。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressionConfig {
    /// 启用响应压缩。
    pub enabled: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// `[server.cors]` 区段：跨域响应策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CorsConfig {
    /// 启用 CORS 响应。
    pub enabled: bool,
    /// 精确来源列表；`["*"]` 表示允许所有来源。
    pub allowed_origins: Vec<String>,
    /// 预检请求允许的方法。
    pub allowed_methods: Vec<String>,
    /// 预检请求允许的请求头。
    pub allowed_headers: Vec<String>,
    /// 浏览器客户端可见的响应头。
    pub expose_headers: Vec<String>,
    /// 是否允许浏览器客户端发送凭据。
    pub allow_credentials: bool,
    /// 预检结果的缓存时间（秒）。
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
