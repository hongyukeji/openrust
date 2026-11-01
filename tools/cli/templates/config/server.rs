//! HTTP 服务默认值。

use openrust::config::{ConfigContext, ConfigError, ServerConfig};

/// 保留当前根站点目标与 listener 默认值。
///
/// 如需限制主机，在返回前设置 `server.trusted_hosts` 为
/// `Some(openrust::config::TrustedHostsConfig { at: None, subdomains: true })`；
/// `at: None` 从 `app.url` 推导主域，默认不启用此限制。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<ServerConfig, ConfigError> {
    let mut server = ServerConfig::default();
    server.root_target = "view:index.html".to_owned();
    Ok(server)
}
