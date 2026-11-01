//! 队列连接与显式 worker 的应用默认值；仅 queue feature 编译。

use openrust::config::{
    ConfigContext, ConfigError, QueueConfig, QueueConnectionConfig, QueueDriverKind,
};
use std::collections::HashMap;

/// false 保持当前旧版 default 配置路径；true 显式采用下方具名连接表。
const NAMED_CONNECTIONS_ENABLED: bool = false;

/// 仅激活所选连接；配置文件不会启动 worker、编译 driver 或自动运行迁移。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<QueueConfig, ConfigError> {
    let connections = if NAMED_CONNECTIONS_ENABLED {
        HashMap::from([
            (
                "default".to_owned(),
                QueueConnectionConfig {
                    driver: "database".to_owned(),
                    // 数据库 driver 不使用 Redis 连接设置。
                    redis_url: None,
                    redis_namespace: None,
                    options: serde_json::json!({}),
                },
            ),
            (
                "redis".to_owned(),
                QueueConnectionConfig {
                    // 选择前仍须编译 queue-redis 并提供实际连接；不静默回退。
                    driver: "redis".to_owned(),
                    redis_url: None,
                    redis_namespace: None,
                    // 内置 driver 只接受自己的已实现选项；外部 driver 可定义该对象。
                    options: serde_json::json!({}),
                },
            ),
        ])
    } else {
        HashMap::new()
    };
    Ok(QueueConfig {
        default_connection: "default".to_owned(),
        connections,
        // connections 为空时使用此既有 default 路径，命名连接整改另行交付。
        driver: QueueDriverKind::Database,
        // None 不在源码指定 Redis URL/命名空间；部署来源可提供所选 driver 的设置。
        redis_url: None,
        redis_namespace: None,
        poll_interval_millis: 500,
        lease_secs: 60,
        job_timeout_secs: 300,
        worker_concurrency: 1,
        shutdown_timeout_secs: 10,
    })
}
