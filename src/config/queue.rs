//! 数据库与 Redis 队列 worker 配置。

use serde::Deserialize;

/// 任务队列的显式存储后端。
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueDriverKind {
    /// 使用应用数据库保存任务，支持数据库事务派发。
    #[default]
    Database,
    /// 使用 Redis 保存共享任务；不支持与业务数据库原子事务派发。
    Redis,
}

/// 设置轮询、任务租约和 worker 优雅停机的边界。
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    /// 持久化任务后端；默认使用应用数据库。
    pub driver: QueueDriverKind,
    /// Redis 队列连接 URL；仅 Redis 后端使用，建议由 `APP_QUEUE__REDIS_URL` 注入。
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
