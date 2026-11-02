//! 具名队列连接与显式 worker；标准项目已编译数据库和 Redis 驱动。

use openrust::config::{ConfigContext, ConfigError, QueueConfig};
use std::collections::HashMap;

/// 配置不启动 worker 或执行迁移；只建立实际选择的连接，不静默切换后端。
pub fn config(ctx: &ConfigContext) -> Result<QueueConfig, ConfigError> {
    let redis_url = match ctx.env("QUEUE_REDIS_URL") {
        Some(url) => Some(url),
        // CacheStoreConfig 的专属选项已 flatten；继承同次最终缓存来源，不重复拼接 Redis 环境。
        None => ctx.get::<String>("cache.stores.redis.dsn")?,
    };
    // app.name 可含空格/Unicode；默认隔离名限制为 Redis driver 接受的安全 ASCII。
    let mut namespace = ctx
        .require::<String>("app.name")?
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(63)
        .collect::<String>();
    if namespace.is_empty() {
        namespace.push_str("app");
    }
    let namespace = ctx.env_or("QUEUE_REDIS_NAMESPACE", &namespace);
    Ok(QueueConfig {
        default: ctx.env_or("QUEUE_CONNECTION", "database"),
        connections: HashMap::from([
            (
                "database".to_owned(),
                serde_json::json!({
                    "driver":"database",
                    "queue":"default",
                    // 数据库连接复用统一 Db；不使用 Redis 配置。
                    "options":{},
                }),
            ),
            (
                "redis".to_owned(),
                serde_json::json!({
                    "driver":"redis",
                    "queue":"default",
                    // null 表示没有配置 URL，选择此连接时必须明确报错；未选连接不建立服务连接。
                    "redis_url":redis_url,
                    "redis_namespace":namespace,
                    "options":{},
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
