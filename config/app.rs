//! 应用身份默认值；依赖清单中已完成的 server 区段。

use openrust::config::{AppConfig, ConfigContext, ConfigError, Environment};

/// 在同次 dotenv 快照上求值；其他领域通过仓储继承应用身份。
pub fn config(ctx: &ConfigContext) -> Result<AppConfig, ConfigError> {
    let url = match ctx.env("APP_URL") {
        // 显式空值也是显式配置，保留给统一校验处理。
        Some(url) => url,
        None => format!("http://localhost:{}", ctx.require::<u16>("server.port")?),
    };
    Ok(AppConfig {
        name: ctx.env_or("APP_NAME", "openrust"),
        env: Environment::Production,
        debug: ctx.bool_or("APP_DEBUG", false)?,
        url,
        cipher: "AES-256-CBC".to_owned(),
        // 源码不保存密钥；可用 key:generate 写入选定的环境文件。
        key: None,
        previous_keys: Vec::new(),
        timezone: "UTC".to_owned(),
    })
}
