//! 认证密钥环与密码哈希的应用默认值。

use openrust::config::{ConfigContext, ConfigError, PasswordConfig};
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

/// 认证应用默认项，显式呈现 JWT 与密码哈希设置。
#[derive(Serialize)]
pub struct Auth {
    jwt: Jwt,
    password: PasswordConfig,
}

/// 缺省继承最终应用身份；领域专属显式来源仍可独立覆写。
/// 这是现有 JWT 增强策略，不是 Laravel 核心的 JWT 默认。
pub fn config(ctx: &ConfigContext) -> Result<Auth, ConfigError> {
    Ok(Auth {
        jwt: Jwt {
            issuer: ctx.require("app.url")?,
            audience: ctx.require("app.name")?,
            active_key_id: "local",
            // 仅部署来源提供 Base64 随机密钥，源码不放真实密钥。
            keys: BTreeMap::new(),
            access_ttl_secs: 900,
            max_valid_ttl_secs: 3_600,
        },
        password: PasswordConfig {
            memory_kib: 19_456,
            iterations: 2,
            parallelism: 1,
        },
    })
}
