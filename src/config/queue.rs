//! 数据库与 Redis 队列 worker 配置。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 任务队列的显式存储后端。
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueDriverKind {
    /// 使用应用数据库保存任务，支持数据库事务派发。
    #[default]
    Database,
    /// 使用 Redis 保存共享任务；不支持与业务数据库原子事务派发。
    Redis,
}

/// 设置轮询、任务租约和 worker 优雅停机的边界。
/// 具名队列连接的存储后端与专属连接参数。
#[derive(Clone, Deserialize, Serialize)]
pub struct QueueConnectionConfig {
    /// 已注册的 driver 名称。
    pub driver: String,
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
#[serde(default)]
pub struct QueueConfig {
    /// 默认具名连接。
    pub default_connection: String,
    /// 具名连接的 driver 与连接参数；空表时使用旧版 default 配置。
    pub connections: HashMap<String, QueueConnectionConfig>,
    /// 持久化任务后端；默认使用应用数据库。
    pub driver: QueueDriverKind,
    /// Redis 队列连接 URL；仅 Redis 后端使用，默认由共享的 `REDIS_*` 连接设置生成。
    pub redis_url: Option<String>,
    /// Redis 队列键隔离命名空间；仅 Redis 后端使用。
    pub redis_namespace: Option<String>,
    /// 队列为空时，两次轮询之间的延迟（毫秒）。
    pub poll_interval_millis: u64,
    /// 已领取任务的租约时长（秒）。
    pub lease_secs: u64,
    /// 单次 handler 尝试的最长执行时间（秒）。
    pub job_timeout_secs: u64,
    /// 单个 worker 进程中同时运行的 handler 最大数量。
    pub worker_concurrency: usize,
    /// 停机时先发出合作取消信号，并等待当前 handler 完成的最长时间（秒）。
    /// 超时后尝试释放租约以供重试；数据库不可用时保留租约，等待其自然过期。
    pub shutdown_timeout_secs: u64,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            default_connection: "default".to_owned(),
            connections: HashMap::new(),
            driver: QueueDriverKind::Database,
            redis_url: None,
            redis_namespace: None,
            poll_interval_millis: 500,
            lease_secs: 60,
            job_timeout_secs: 300,
            worker_concurrency: 1,
            shutdown_timeout_secs: 10,
        }
    }
}
