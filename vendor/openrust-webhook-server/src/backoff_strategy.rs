/// 返回本次尝试之后的等待秒数；首个实际尝试编号为1。
pub trait BackoffStrategy: Send + Sync {
    fn wait_in_seconds_after_attempt(&self, attempt: u32) -> u64;
}
pub struct ExponentialBackoffStrategy;
impl BackoffStrategy for ExponentialBackoffStrategy {
    fn wait_in_seconds_after_attempt(&self, attempt: u32) -> u64 {
        if attempt > 4 {
            100_000
        } else {
            10u64.pow(attempt)
        }
    }
}
