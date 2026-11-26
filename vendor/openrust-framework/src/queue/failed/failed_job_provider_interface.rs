use std::{pin::Pin, time::Duration};

use futures_util::{Stream, stream};

use crate::{
    database::Transaction,
    queue::{QueueFuture, QueueResult},
};

use super::{FailedJob, PrunableFailedJobProvider};

/// Entries to retry in storage order. Each key identifies the stored record to remove.
pub type FailedJobRetryStream<'a> =
    Pin<Box<dyn Stream<Item = QueueResult<(String, FailedJob)>> + Send + 'a>>;

/// Stores and retrieves failed queue jobs.
pub trait FailedJobProviderInterface: Send + Sync {
    /// 存储未提供计数能力时由包装器按固定契约返回零。
    fn countable(&self) -> Option<&dyn super::CountableFailedJobProvider> {
        None
    }

    fn prunable(&self) -> Option<&dyn PrunableFailedJobProvider> {
        None
    }

    fn log<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>>;
    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>>;

    /// List the retry identifiers, optionally limited to the given queue.
    fn ids<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<Vec<String>>> {
        Box::pin(async move {
            Ok(self
                .all()
                .await?
                .into_iter()
                .filter(|job| queue.is_none_or(|queue| job.queue == queue))
                .map(|job| job.id)
                .collect())
        })
    }
    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>>;

    /// Resolve a retry identifier to one or more entries. An empty stream represents an existing
    /// selection without any records. The default adapts the provider's single-record lookup.
    fn find_for_retry<'a>(
        &'a self,
        id: &'a str,
    ) -> QueueFuture<'a, QueueResult<Option<FailedJobRetryStream<'a>>>> {
        Box::pin(async move {
            let Some(job) = self.find(id).await? else {
                return Ok(None);
            };
            let entries: FailedJobRetryStream<'a> =
                Box::pin(stream::iter([Ok((id.to_owned(), job))]));
            Ok(Some(entries))
        })
    }
    fn forget<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<bool>>;
    fn forget_retry<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<bool>> {
        self.forget(&job.id)
    }
    /// Remove an unchanged failure inside the queue transaction if both stores share it.
    fn forget_retry_in_transaction<'a>(
        &'a self,
        _job: &'a FailedJob,
        _transaction: &'a mut Transaction,
    ) -> QueueFuture<'a, QueueResult<Option<bool>>> {
        Box::pin(async { Ok(None) })
    }
    fn flush(&self, older_than: Option<Duration>) -> QueueFuture<'_, QueueResult<()>>;
    /// 指定绝对截止时间，允许保留小时数为小数或负数；不支持时拒绝误删。
    fn flush_before<'a>(
        &'a self,
        before: Option<jiff::Timestamp>,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        if before.is_none() {
            self.flush_queue(None, queue)
        } else {
            Box::pin(async {
                Err(crate::queue::backend(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "当前失败任务存储不支持指定截止时间的清理",
                )))
            })
        }
    }
    /// 队列过滤区分未指定与显式空名称，默认实现只保留原有无过滤入口。
    fn flush_queue<'a>(
        &'a self,
        older_than: Option<Duration>,
        queue: Option<&'a str>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        if queue.is_none() {
            self.flush(older_than)
        } else {
            Box::pin(async {
                Err(crate::queue::backend(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "当前失败任务存储不支持按队列清理",
                )))
            })
        }
    }
}
