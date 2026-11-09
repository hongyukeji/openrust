//! 应用统一 `HashManager` 的可编辑默认；成本配置不归认证或 Passport 私有持有。

use openrust::config::{ArgonConfig, BcryptConfig, ConfigContext, ConfigError, HashingConfig};

/// 应用默认使用 bcrypt，可显式切换至 Argon。
pub fn config(ctx: &ConfigContext) -> Result<HashingConfig, ConfigError> {
    let verify = ctx.bool_or("HASH_VERIFY", true)?;
    let limit = ctx.nullable_string_or("BCRYPT_LIMIT", None)
        .map(|value| if value.is_empty() { Ok(0) } else { value.parse::<usize>().map_err(|_| invalid("BCRYPT_LIMIT")) })
        .transpose()?;
    Ok(HashingConfig {
        driver: ctx.string_or("HASH_DRIVER", "bcrypt")?,
        bcrypt: BcryptConfig { rounds: number(ctx, "BCRYPT_ROUNDS", 12)?, verify, limit },
        argon: ArgonConfig {
            memory: number(ctx, "ARGON_MEMORY", 65_536)?,
            time: number(ctx, "ARGON_TIME", 4)?,
            threads: number(ctx, "ARGON_THREADS", 1)?,
            verify,
        },
        rehash_on_login: true,
        // 通用安全增强：部署可在此显式调整执行预算，不改变官方参数比较语义。
        execution: openrust::config::hashing::ExecutionLimits::default(),
    })
}
fn number(ctx: &ConfigContext, name: &str, default: u32) -> Result<u32, ConfigError> {
    ctx.env(name).map_or(Ok(default), |value| value.parse().map_err(|_| invalid(name)))
}
fn invalid(name: &str) -> ConfigError {
    ConfigError::Invalid(format!("{name}: 哈希成本须为非负整数"))
}
