//! HTTP 服务器和 middleware 配置。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `[server]` 区段：HTTP listener 设置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// 绑定的网络接口。
    pub host: String,
    /// 绑定的 TCP 端口。
    pub port: u16,
    /// 网站公开目录；相对路径以应用工作目录为基准。
    pub public_dir: PathBuf,
    /// 根路径目标，支持 `file:/index.html`、`forward:/path`、`redirect:/path` 和可选 `view:index.html`。
    pub root_target: String,
    /// 单个请求的超时时间（秒）；超过后返回 `408 Request Timeout`。
    pub request_timeout_secs: u64,
    /// 同时处理的 HTTP 请求上限；超出时立即以 503 拒绝，不排入无界队列。
    pub max_concurrent_requests: usize,
    /// 收到关闭信号后，为处理中请求保留的宽限时间（1..=3600 秒）。
    pub shutdown_timeout_secs: u64,
    /// 显式开放 Prometheus 指标采集端点；仅在启用 `metrics` feature 时生效。
    pub metrics_endpoint_enabled: bool,
    /// 允许提供 `X-Forwarded-For` 的代理 IP 地址或 CIDR。
    ///
    /// `None` 为官方未配置；显式空列表禁止 Forge/Vapor/Cloud 自动信任。
    /// 普通 Host 未配置时仍只采用 socket 对端地址。
    pub trusted_proxy_ranges: Option<Vec<String>>,
    /// 可信代理可提供的转发头位掩码；默认使用 X-Forwarded 集合。
    pub trusted_proxy_headers: u8,
    /// 可信主机模式；`None` 表示不启用主机信任列表。
    pub trusted_hosts: Option<TrustedHostsConfig>,
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
            public_dir: PathBuf::from("public"),
            root_target: "file:/index.html".to_owned(),
            request_timeout_secs: 30,
            max_concurrent_requests: 1024,
            shutdown_timeout_secs: 10,
            metrics_endpoint_enabled: false,
            trusted_proxy_ranges: None,
            trusted_proxy_headers: crate::http::TrustedProxyHeaders::DEFAULT,
            trusted_hosts: None,
            compression: CompressionConfig::default(),
            cors: CorsConfig::default(),
        }
    }
}

/// `[server.trusted_hosts]`：公开模式是 PCRE 模式体，不自动添加锚点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustedHostsConfig {
    /// `None` 使用 `app.url` 主机；显式空列表可与 `subdomains=false` 一起关闭限制。
    pub at: Option<Vec<String>>,
    /// 向显式列表追加 `app.url` 主机及任意层子域；默认启用。
    pub subdomains: bool,
}

impl Default for TrustedHostsConfig {
    fn default() -> Self {
        Self {
            at: None,
            subdomains: true,
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
