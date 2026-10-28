//! 文件存储配置。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `[storage]` 区段：默认 disk 与最大上传资源限制。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct StorageConfig {
    /// 未指定时使用的具名 disk。
    pub default_disk: String,
    /// 已配置的 default 与 named disks。
    pub disks: BTreeMap<String, StorageDiskConfig>,
    /// 单次上传允许的最大字节数。
    pub max_upload_bytes: u64,
    /// inspector 和 driver 单次接收的最大字节块。
    pub max_chunk_bytes: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        let mut disks = BTreeMap::new();
        disks.insert("local".into(), StorageDiskConfig::local_default());
        Self {
            default_disk: "local".into(),
            disks,
            max_upload_bytes: 32 * 1024 * 1024,
            max_chunk_bytes: 1024 * 1024,
        }
    }
}

/// 单个存储 disk 配置；driver 专属字段留在 options 中。
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StorageDiskConfig {
    /// Driver 注册名，例如 `fs` 或 `s3`。
    pub driver: String,
    /// Driver 自有选项，例如本地 `root` 或 S3 endpoint/凭据。
    #[serde(flatten)]
    pub options: BTreeMap<String, serde_json::Value>,
}

impl StorageDiskConfig {
    fn local_default() -> Self {
        let mut options = BTreeMap::new();
        options.insert(
            "root".into(),
            serde_json::Value::String("storage/app".into()),
        );
        Self {
            driver: "fs".into(),
            options,
        }
    }
}

impl std::fmt::Debug for StorageConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StorageConfig")
            .field("default_disk", &self.default_disk)
            .field("disk_names", &self.disks.keys().collect::<Vec<_>>())
            .field("max_upload_bytes", &self.max_upload_bytes)
            .field("max_chunk_bytes", &self.max_chunk_bytes)
            .finish()
    }
}

impl std::fmt::Debug for StorageDiskConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StorageDiskConfig")
            .field("driver", &self.driver)
            .field("option_names", &self.options.keys().collect::<Vec<_>>())
            .finish()
    }
}
