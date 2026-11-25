use std::time::SystemTime;

use crate::queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult, UniqueJob};

use super::Interruptible;

/// Executes a registered application job.
pub trait JobHandler: Send + Sync {
    /// 返回入队任务处理器的具体类型。
    fn queue_subject_type(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn interruptible(&self) -> Option<&dyn Interruptible> {
        None
    }

    fn middleware(&self) -> Vec<std::sync::Arc<dyn crate::queue::middleware::JobMiddleware>> {
        Vec::new()
    }

    /// Determines whether a dispatched payload holds an application cache lock.
    fn unique_job(&self, _payload: &Payload) -> Option<UniqueJob> {
        None
    }

    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>>;

    /// Resolves the deadline for an explicit retry. The default keeps the stored deadline.
    fn retry_until(&self, payload: &Payload) -> QueueResult<Option<SystemTime>> {
        Ok(payload.options().retry_until)
    }

    /// Runs after a job reaches terminal failure, with the original payload.
    fn failed<'a>(
        &'a self,
        _context: &'a mut JobContext<'_>,
        _payload: &'a Payload,
        _error: &'a QueueError,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
