//! Redis 缓存与同进程加载限制的应用默认值。

use openrust::config::{
    CacheConfig, CacheSingleflightConfig, CacheStoreConfig, CacheSwrConfig, ConfigContext,
    ConfigError,
};
use std::collections::BTreeMap;

/// 仅展示可选本机 store，默认不加入 store 表，不改变 redis 选择及初始化行为。
const LOCAL_STORES_ENABLED: bool = false;

/// 默认缓存使用独立 Redis DB 1；部署连接通过统一配置来源覆盖。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<CacheConfig, ConfigError> {
    let mut stores = BTreeMap::from([(
        "redis".to_owned(),
        CacheStoreConfig {
            driver: "redis".to_owned(),
            namespace: "server".to_owned(),
            // driver 专属选项；memory 可用空表，file 使用 path，非所有 driver 都读取 dsn。
            options: BTreeMap::from([(
                "dsn".to_owned(),
                serde_json::Value::String("redis://127.0.0.1:6379/1".to_owned()),
            )]),
        },
    )]);
    if LOCAL_STORES_ENABLED {
        stores.insert(
            "memory".to_owned(),
            CacheStoreConfig {
                driver: "memory".to_owned(),
                namespace: "local".to_owned(),
                options: BTreeMap::new(),
            },
        );
        stores.insert(
            "file".to_owned(),
            CacheStoreConfig {
                driver: "file".to_owned(),
                namespace: "local".to_owned(),
                options: BTreeMap::from([(
                    "path".to_owned(),
                    serde_json::json!("storage/framework/cache"),
                )]),
            },
        );
    }
    Ok(CacheConfig {
        default: ctx.env_or("CACHE_STORE", "redis"),
        stores,
        singleflight: CacheSingleflightConfig {
            max_in_flight_keys: 128,
            max_waiters_per_key: 64,
            loader_timeout_secs: 30,
        },
        swr: CacheSwrConfig {
            max_in_flight_refreshes: 64,
        },
    })
}
