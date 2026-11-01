//! Redis 缓存默认值与公开 store 配置。

use openrust::config::{CacheConfig, CacheStoreConfig, ConfigContext, ConfigError};

/// 使用统一 Redis URL 与 Laravel 风格 `CACHE_STORE` 选择。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<CacheConfig, ConfigError> {
    let mut cache = CacheConfig {
        default: ctx.env_or("CACHE_STORE", "redis"),
        ..CacheConfig::default()
    };
    cache.stores.insert(
        "redis".to_owned(),
        CacheStoreConfig {
            driver: "redis".to_owned(),
            namespace: "server".to_owned(),
            options: std::collections::BTreeMap::from([(
                "dsn".to_owned(),
                serde_json::Value::String(ctx.env_or("REDIS_URL", "redis://127.0.0.1:6379/")),
            )]),
        },
    );
    Ok(cache)
}
