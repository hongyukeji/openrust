use std::time::Duration;

use openrust::queue::{DispatchResult, QueueManager};
use tokio::time::{Instant, timeout_at};

use crate::{DeliveryService, Error, Result, delivery_store::WakeClaim, events};

const BATCH_TIME: Duration = Duration::from_secs(30);
const WAKE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default, serde::Serialize)]
pub struct RecoveryReport {
    pub candidates: usize,
    pub queued: usize,
    pub retired: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// 有界的待唤醒或收尾统计，只包含数量和最早可用时间。
#[derive(Debug, serde::Serialize)]
pub struct RecoveryBacklog {
    pub candidates: usize,
    pub oldest_available_at: Option<i64>,
    pub has_more: bool,
}

impl DeliveryService {
    /// 读取恢复候选的积压，无需建立队列连接。
    pub async fn inspect_recovery_backlog(&self, limit: u32) -> Result<RecoveryBacklog> {
        self.store.wake_backlog(limit).await
    }

    /// 有界扫描只唤醒原生 Worker。多个调度进程通过数据库协调同一投递。
    pub async fn recover_due(
        &self,
        queue: &QueueManager,
        limit: u32,
        pretend: bool,
    ) -> Result<RecoveryReport> {
        let stop_at = Instant::now() + BATCH_TIME;
        let ids = self.store.wake_ids(limit).await?;
        let mut report = RecoveryReport {
            candidates: ids.len(),
            ..Default::default()
        };
        if pretend {
            return Ok(report);
        }

        for id in ids {
            if Instant::now() >= stop_at {
                break;
            }
            match self.store.claim_wake(&id).await? {
                WakeClaim::Retired(retired) => {
                    report.retired += 1;
                    let _ = timeout_at(
                        stop_at,
                        events::reconciled_unknown(
                            self.store.scope(),
                            &id,
                            retired.unknown_attempt,
                        ),
                    )
                    .await;
                }
                WakeClaim::Unavailable => report.skipped += 1,
                WakeClaim::Ready => {
                    let until = stop_at.min(Instant::now() + WAKE_TIMEOUT);
                    match timeout_at(until, self.enqueue_delivery(queue, &id, false)).await {
                        Ok(Ok(DispatchResult::Queued(_))) => report.queued += 1,
                        Ok(Err(Error::EndpointPaused)) => report.skipped += 1,
                        _ => report.failed += 1,
                    }
                }
            }
        }
        Ok(report)
    }
}
