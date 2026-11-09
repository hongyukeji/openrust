//! 认证密钥环与具名身份的应用默认值。

use openrust::config::{ConfigContext, ConfigError};
use serde::Serialize;
use std::collections::BTreeMap;

// JWT 的框架类型不公开秘密字段；可序列化应用 DTO 显式呈现所有已实现选项。
#[derive(Serialize)]
struct Jwt {
    issuer: String,
    audience: String,
    active_key_id: &'static str,
    keys: BTreeMap<String, String>,
    access_ttl_secs: u64,
    max_valid_ttl_secs: u64,
}

/// 认证应用默认项，显式呈现具名Guard/Provider、时间保护与JWT设置。
#[derive(Serialize)]
pub struct Auth {
    jwt: Jwt,
    defaults: serde_json::Value,
    guards: serde_json::Value,
    providers: serde_json::Value,
    timebox_duration: u64,
    password_timeout: u64,
}

/// 缺省继承最终应用身份；领域专属显式来源仍可独立覆写。
/// JWT 签发策略独立于默认 Guard 的选择。
pub fn config(ctx: &ConfigContext) -> Result<Auth, ConfigError> {
    Ok(Auth {
        defaults: serde_json::json!({"guard":"web", "passwords":"users", "provider":null}),
        guards: serde_json::json!({"web":{"driver":"session", "provider":"users", "remember":576_000}}),
        providers: serde_json::json!({"users":{"driver":"app-users", "table":"users"}}),
        timebox_duration: 200_000,
        password_timeout: 10_800,
        jwt: Jwt {
            issuer: ctx.require("app.url")?,
            audience: ctx.require("app.name")?,
            active_key_id: "local",
            // 仅部署来源提供 Base64 随机密钥，源码不放真实密钥。
            keys: BTreeMap::new(),
            access_ttl_secs: 900,
            max_valid_ttl_secs: 3_600,
        },

    })
}
