//! 标准应用具名队列连接与显式 worker 的默认值。

use openrust::config::{ConfigContext, ConfigError, QueueConfig};
use std::collections::HashMap;

/// 仅解析被选择的连接；编译 driver 不会自动连接或启动 worker。
pub fn config(ctx: &ConfigContext) -> Result<QueueConfig, ConfigError> {
    let redis_url = match ctx.env("QUEUE_REDIS_URL") {
        Some(url) => Some(url),
        None => ctx.get::<String>("cache.stores.redis.dsn")?,
    };
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
                    "redis_url": redis_url,
                    "redis_namespace": ctx.env_or("QUEUE_REDIS_NAMESPACE", "server"),
                    "options": {},
                }),
            ),
        ]),
        poll_interval_millis: 500,
        lease_secs: 60,
        job_timeout_secs: 300,
        worker_concurrency: 1,
        shutdown_timeout_secs: 10,
    })
}
