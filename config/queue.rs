//! 标准应用具名队列连接与 worker 的默认值。

use openrust::config::{ConfigContext, ConfigError, QueueConfig};
use std::collections::HashMap;

/// 仅解析被选择的连接；编译 driver 不会自动连接或启动 worker。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<QueueConfig, ConfigError> {
    Ok(QueueConfig {
        default: ctx.env_or("QUEUE_CONNECTION", "database"),
        connections: HashMap::from([
            (
                "database".to_owned(),
                serde_json::json!({
                    "driver": "database",
                    "queue": "default",
                    "options": {},
                }),
            ),
            (
                "redis".to_owned(),
                serde_json::json!({
                    "driver": "redis",
                    "queue": "default",
                    "redis_url": "redis://127.0.0.1:6379/0",
                    "redis_namespace": ctx.env_or("QUEUE_REDIS_NAMESPACE", "server"),
                    "options": {},
                }),
            ),
        ]),
        poll_interval_millis: 500,
        lease_secs: 60,
        job_timeout_secs: 300,
        worker_concurrency: 1,
        embedded_queues: vec!["default".into()],
        shutdown_timeout_secs: 10,
    })
}
