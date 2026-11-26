use std::error::Error;

use crate::queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult};

use super::{JobMiddleware, JobNext};

/// Fails a job immediately when a downstream exception matches the policy.
pub struct FailOnException {
    matches: Box<ExceptionPredicate>,
}

type ExceptionPredicate = dyn Fn(&QueueError, &Payload) -> bool + Send + Sync;

impl FailOnException {
    pub fn new(predicate: impl Fn(&QueueError, &Payload) -> bool + Send + Sync + 'static) -> Self {
        Self {
            matches: Box::new(predicate),
        }
    }

    /// Matches a typed exception in the error chain.
    pub fn for_exception<E: Error + 'static>() -> Self {
        Self::new(|error, _| {
            let mut cause: Option<&(dyn Error + 'static)> = Some(error);
            while let Some(error) = cause {
                if error.is::<E>() {
                    return true;
                }
                cause = error.source();
            }
            false
        })
    }
}

impl JobMiddleware for FailOnException {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
        next: JobNext<'a>,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let result = next.run(context, payload).await;
            if let Err(error) = &result
                && (self.matches)(error, payload)
            {
                context.fail_immediately = true;
            }
            result
        })
    }
}
