//! HTTP listener、可信请求边界和中间件的应用默认值。

use openrust::config::{
    CompressionConfig, ConfigContext, ConfigError, CorsConfig, ServerConfig, TrustedHostsConfig,
};

/// 只控制默认是否声明可信主机策略；显式来源仍可设置 `server.trusted_hosts`。
const TRUST_HOSTS_ENABLED: bool = false;

/// 根应用默认提供 public/index.html；生成应用可选用视图目标。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<ServerConfig, ConfigError> {
    Ok(ServerConfig {
        host: "127.0.0.1".to_owned(),
        port: 3000,
        public_dir: "public".into(),
        root_target: "file:/index.html".to_owned(),
        request_timeout_secs: 30,
        max_concurrent_requests: 1024,
        shutdown_timeout_secs: 10,
        // 还需编译 metrics feature；声明配置不会编译或启动可选能力。
        metrics_endpoint_enabled: false,
        // None 未配置；Some(Vec::new()) 显式禁止平台自动信任；Some 列表信任指定 IP/CIDR。
        trusted_proxy_ranges: None,
        trusted_proxy_headers: openrust::http::TrustedProxyHeaders::DEFAULT,
        // at=None 使用最终 app.url 主机；subdomains=true 追加该主机及所有层级子域。
        // at=Some(vec!["^example\\.com$".to_owned()]) 可显式选择 PCRE 模式体。
        trusted_hosts: if TRUST_HOSTS_ENABLED {
            Some(TrustedHostsConfig {
                at: None,
                subdomains: true,
            })
        } else {
            None
        },
        compression: CompressionConfig { enabled: true },
        cors: CorsConfig {
            enabled: false,
            // ["*"] 允许任意来源；携带凭据时必须使用精确来源。
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
        },
    })
}
