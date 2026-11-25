use std::sync::Arc;

use tokio::time::Instant;

use crate::contracts::queue::{Job, Queue};

use super::{QueueFuture, QueueResult};

/// Selects a reservation for a named worker, using its configured connection.
pub type JobPopCallback = Arc<
    dyn for<'a> Fn(JobPopper<'a>, &'a str) -> QueueFuture<'a, QueueResult<Option<Box<dyn Job>>>>
        + Send
        + Sync,
>;

pub(super) type WorkerReservationGuard =
    Arc<dyn Fn() -> QueueFuture<'static, QueueResult<bool>> + Send + Sync>;

tokio::task_local! {
    static RESERVATION_GUARD: WorkerReservationGuard;
}

/// 队列传输结束等待后，在发出新的领取请求前重新检查。
pub(super) async fn may_resume_reservation() -> QueueResult<bool> {
    match RESERVATION_GUARD.try_with(|guard| guard()) {
        Ok(check) => check.await,
        Err(_) => Ok(true),
    }
}

/// Reserves a job using the current worker's polling limits.
pub struct JobPopper<'a> {
    connection: &'a dyn Queue,
    immediate: bool,
    deadline: Option<Instant>,
    reservation_guard: Option<&'a WorkerReservationGuard>,
}

impl<'a> JobPopper<'a> {
    pub(super) fn new(
        connection: &'a dyn Queue,
        immediate: bool,
        deadline: Option<Instant>,
        reservation_guard: Option<&'a WorkerReservationGuard>,
    ) -> Self {
        Self {
            connection,
            immediate,
            deadline,
            reservation_guard,
        }
    }

    /// Reserves a job on the given queue, or on the connection's default queue.
    pub async fn pop(&self, queue: Option<&str>) -> QueueResult<Option<Box<dyn Job>>> {
        if self.immediate {
            return self.pop_now(queue).await;
        }
        let pop = if let Some(deadline) = self.deadline {
            let wait = deadline.saturating_duration_since(Instant::now());
            self.connection.pop_within(queue, wait)
        } else {
            self.connection.pop(queue)
        };
        if let Some(guard) = self.reservation_guard {
            RESERVATION_GUARD.scope(Arc::clone(guard), pop).await
        } else {
            pop.await
        }
    }

    /// Checks a queue for an available job without waiting.
    pub async fn pop_now(&self, queue: Option<&str>) -> QueueResult<Option<Box<dyn Job>>> {
        self.connection.pop_now(queue).await
    }
}
