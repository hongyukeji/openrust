//! 缓存 store 配置。

use std::collections::BTreeMap;

use serde::Deserialize;

/// `[cache]` 区段：默认 store 与命名实例。
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    /// 未显式选择 store 时使用的名称。
    pub default_store: String,
    /// 按名称登记的缓存实例。
    pub stores: BTreeMap<String, CacheStoreConfig>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            default_store: "default".into(),
            stores: BTreeMap::new(),
        }
    }
}

/// 单个缓存 store 配置。未识别的字段会交给所选 driver 解析。
#[derive(Clone, Default, Deserialize)]
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
