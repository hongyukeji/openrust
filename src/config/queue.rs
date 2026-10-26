//! 数据库队列 worker 配置。

use serde::Deserialize;

/// 设置轮询、任务租约和 worker 优雅停机的边界。
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    /// 队列为空时，两次轮询之间的延迟（毫秒）。
    pub poll_interval_millis: u64,
    /// 已领取任务的租约时长（秒）。
    pub lease_secs: u64,
    /// 单次 handler 尝试的最长执行时间（秒）。
    pub job_timeout_secs: u64,
    /// 单个 worker 进程中同时运行的 handler 最大数量。
    pub worker_concurrency: usize,
    /// 停机时等待当前 handler 完成的最长时间（秒）。
    pub shutdown_timeout_secs: u64,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            poll_interval_millis: 500,
            lease_secs: 60,
            job_timeout_secs: 300,
            worker_concurrency: 1,
            shutdown_timeout_secs: 10,
        }
    }
}
