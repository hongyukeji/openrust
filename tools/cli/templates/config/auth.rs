//! 认证密钥环的应用默认结构。

use openrust::config::{ConfigContext, ConfigError};
use serde::Serialize;

#[derive(Serialize)]
struct Jwt {
    issuer: String,
    audience: String,
    active_key_id: &'static str,
    keys: std::collections::BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct Auth {
    jwt: Jwt,
}

/// 应用可在此根据环境选择 JWT 签发者与受众。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(ctx: &ConfigContext) -> Result<Auth, ConfigError> {
    Ok(Auth {
        jwt: Jwt {
            issuer: ctx.env_or("APP_URL", "http://localhost:3000"),
            audience: ctx.env_or("APP_NAME", "{{name}}"),
            active_key_id: "local",
            keys: std::collections::BTreeMap::new(),
        },
    })
}
