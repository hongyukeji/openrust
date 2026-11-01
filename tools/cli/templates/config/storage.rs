//! 文件存储 disk、可见性和上传限制的应用默认值。

use openrust::config::{ConfigContext, ConfigError};
use openrust::storage::Visibility;
use serde::Serialize;
use std::collections::BTreeMap;

// 应用 DTO 省略未指定的 visibility，保持 driver 的既有默认策略与 Provider schema。
#[derive(Serialize)]
struct Disk {
    driver: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    visibility: Option<Visibility>,
    #[serde(flatten)]
    options: BTreeMap<String, serde_json::Value>,
}

/// 可序列化的应用存储默认项；实际 driver 由已安装的 Provider 登记。
#[derive(Serialize)]
pub struct Storage {
    default_disk: String,
    disks: BTreeMap<String, Disk>,
    max_upload_bytes: u64,
    max_chunk_bytes: usize,
}

/// 可选 S3 disk 默认不登记；启用前须编译 storage-s3 并通过部署来源提供所有必填项。
const S3_ENABLED: bool = false;

/// 配置 fs 不会自动编译 storage-local 或安装存储 Provider。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<Storage, ConfigError> {
    let mut disks = BTreeMap::from([(
        "local".to_owned(),
        Disk {
            driver: "fs".to_owned(),
            // None 采用 driver 默认可见性；Some(Visibility::Private/Public) 显式固定。
            visibility: None,
            // driver 专属选项由实际 driver 校验；fs 的相对 root 基于运行目录。
            options: BTreeMap::from([(
                "root".to_owned(),
                serde_json::Value::String("storage/app".to_owned()),
            )]),
        },
    )]);
    if S3_ENABLED {
        disks.insert(
            "s3".to_owned(),
            Disk {
                driver: "s3".to_owned(),
                visibility: None,
                // 与 S3Driver 实际允许的选项一致；空值/None 不代表有效服务配置。
                options: BTreeMap::from([
                    ("bucket".to_owned(), serde_json::json!("")),
                    ("region".to_owned(), serde_json::json!("")),
                    ("endpoint".to_owned(), serde_json::json!("")),
                    ("access_key_id".to_owned(), serde_json::Value::Null),
                    ("secret_access_key".to_owned(), serde_json::Value::Null),
                    ("root".to_owned(), serde_json::json!("")),
                ]),
            },
        );
    }
    Ok(Storage {
        default_disk: "local".to_owned(),
        disks,
        max_upload_bytes: 32 * 1024 * 1024,
        max_chunk_bytes: 1024 * 1024,
    })
}
