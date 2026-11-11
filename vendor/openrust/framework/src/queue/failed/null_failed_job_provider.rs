use std::time::Duration;

use super::{FailedJob, FailedJobProviderInterface};
use crate::queue::{QueueFuture, QueueResult};

pub(crate) struct NullFailedJobProvider;

impl FailedJobProviderInterface for NullFailedJobProvider {
    fn log<'a>(&'a self, _: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(async { Ok(None) })
    }
    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn find<'a>(&'a self, _: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>> {
        Box::pin(async { Ok(None) })
    }
    fn forget<'a>(&'a self, _: &'a str) -> QueueFuture<'a, QueueResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn flush(&self, _: Option<Duration>) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
