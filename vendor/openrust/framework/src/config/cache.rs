//! 缓存 store 配置。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `[cache]` 区段：默认 store 与命名实例。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CacheConfig {
    /// 未显式选择 store 时使用的名称。
    pub default: String,
    /// 按名称登记的缓存实例。
    pub stores: BTreeMap<String, CacheStoreConfig>,
    /// 同进程同键 loader 的资源限制。
    pub singleflight: CacheSingleflightConfig,
    /// stale-while-revalidate 的并发限制。
    pub swr: CacheSwrConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            default: "redis".into(),
            stores: BTreeMap::new(),
            singleflight: CacheSingleflightConfig::default(),
            swr: CacheSwrConfig::default(),
        }
    }
}

/// 同进程同键加载合并的容量与超时设置。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CacheSingleflightConfig {
    /// 同时执行 loader 的不同 key 上限（1–4096）。
    pub max_in_flight_keys: usize,
    /// 每个 key 可等待的调用数上限（1–4096）。
    pub max_waiters_per_key: usize,
    /// leader loader 的超时秒数（1–300）。
    pub loader_timeout_secs: u64,
}

impl Default for CacheSingleflightConfig {
    fn default() -> Self {
        Self {
            max_in_flight_keys: 128,
            max_waiters_per_key: 64,
            loader_timeout_secs: 30,
        }
    }
}

/// SWR 后台刷新的资源上限。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CacheSwrConfig {
    /// 同一应用实例中同时运行的刷新 loader 上限（1–256）。
    pub max_in_flight_refreshes: usize,
}

impl Default for CacheSwrConfig {
    fn default() -> Self {
        Self {
            max_in_flight_refreshes: 64,
        }
    }
}

/// 单个缓存 store 配置。未识别的字段会交给所选 driver 解析。
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CacheStoreConfig {
    /// Driver 注册名称，如 `redis`、`memory` 或 `file`。
    pub driver: String,
    /// 加在逻辑 key 前的可选命名空间。
    pub namespace: String,
    /// Driver 专属选项，例如 Redis 的 `dsn` 或 file 的 `path`。
    #[serde(flatten)]
    pub options: BTreeMap<String, serde_json::Value>,
}

impl std::fmt::Debug for CacheStoreConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CacheStoreConfig")
            .field("driver", &self.driver)
            .field("namespace", &self.namespace)
            .field("option_names", &self.options.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::CacheConfig;

    #[test]
    fn default_store_selector_serializes_as_official_default_field() -> Result<(), serde_json::Error>
    {
        let config = CacheConfig::default();
        assert_eq!(config.default, "redis");
        let value = serde_json::to_value(&config)?;
        assert_eq!(value["default"], "redis");
        assert!(value.get("default_store").is_none());
        let selected: CacheConfig = serde_json::from_value(serde_json::json!({
            "default": "local", "stores": {"local": {"driver": "memory"}}
        }))?;
        assert_eq!(selected.default, "local");
        assert_eq!(selected.stores["local"].driver, "memory");
        let legacy: CacheConfig = serde_json::from_value(serde_json::json!({
            "default_store": "legacy"
        }))?;
        assert_eq!(legacy.default, "redis");
        Ok(())
    }
}
