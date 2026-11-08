//! 数据库与 Redis 队列 worker 配置。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 具名队列连接的存储后端与专属连接参数。
#[derive(Clone, Deserialize, Serialize)]
pub struct QueueConnectionConfig {
    /// 已注册的 driver 名称。
    pub driver: String,
    /// 此连接的默认队列；连接名和队列名互相独立。
    #[serde(default = "default_queue")]
    pub queue: String,
    /// Redis 连接 URL，仅 Redis driver 使用。
    #[serde(default)]
    pub redis_url: Option<String>,
    /// Redis 键隔离命名空间，仅 Redis driver 使用。
    #[serde(default)]
    pub redis_namespace: Option<String>,
    /// 自定义 driver 的连接选项；内置 driver 会验证允许的键。
    #[serde(default)]
    pub options: Value,
}

impl std::fmt::Debug for QueueConnectionConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueueConnectionConfig")
            .field("driver", &self.driver)
            .field("queue", &self.queue)
            .field("redis_url", &self.redis_url.as_ref().map(|_| "[redacted]"))
            .field(
                "redis_namespace",
                &self.redis_namespace.as_ref().map(|_| "[redacted]"),
            )
            .field("options", &"[redacted]")
            .finish()
    }
}

/// 队列默认连接、具名连接与 worker 运行边界。
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueueConfig {
    /// 默认具名连接。
    pub default: String,
    /// 原始具名连接配置；只在选用该连接时解析、校验和构造 driver。
    pub connections: HashMap<String, Value>,
    /// 队列为空时，两次轮询之间的延迟（毫秒）。
    pub poll_interval_millis: u64,
    /// 已领取任务的租约时长（秒）。
    pub lease_secs: u64,
    /// 单次 handler 尝试的最长执行时间（秒）。
    pub job_timeout_secs: u64,
    /// 单个 worker 进程中同时运行的 handler 最大数量。
    pub worker_concurrency: usize,
    /// `start` 模式在默认连接上额外消费的队列；始终包含连接自己的默认队列。
    pub embedded_queues: Vec<String>,
    /// 停机时先发出合作取消信号，并等待当前 handler 完成的最长时间（秒）。
    /// 超时后尝试释放租约以供重试；数据库不可用时保留租约，等待其自然过期。
    pub shutdown_timeout_secs: u64,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            default: "database".to_owned(),
            connections: HashMap::from([
                (
                    "database".to_owned(),
                    serde_json::json!({"driver":"database","queue":"default"}),
                ),
                (
                    "redis".to_owned(),
                    serde_json::json!({"driver":"redis","queue":"default","redis_url":null,"redis_namespace":null}),
                ),
            ]),
            poll_interval_millis: 500,
            lease_secs: 60,
            job_timeout_secs: 300,
            worker_concurrency: 1,
            embedded_queues: Vec::new(),
            shutdown_timeout_secs: 10,
        }
    }
}

fn default_queue() -> String {
    "default".to_owned()
}
